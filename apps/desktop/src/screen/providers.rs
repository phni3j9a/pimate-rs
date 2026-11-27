use super::*;
use pimate_protocol::{
    AuthBeginParams, AuthCancelParams, AuthProviderId, AuthRespondParams, ProviderAuthType,
};
use settings::{line, section};
use state::{AuthAction, CredentialOutcome as Outcome, Login};
#[derive(Default)]
pub(super) struct ProviderData {
    opening: bool,
    login: Option<Arc<Login>>,
    pending: bool,
    following: Option<String>,
    superseded_flow: Option<String>,
    list_failed: bool,
    logout: Option<(AuthProviderId, String)>,
    prompt: Option<(String, String, Entity<InputState>)>,
}
fn vendors(rows: &[Value]) -> std::collections::BTreeMap<&str, Vec<&Value>> {
    let mut groups = std::collections::BTreeMap::new();
    for row in rows {
        if let Some(label) = row["vendor"].as_str().or(row["id"].as_str()) {
            groups.entry(label).or_insert_with(Vec::new).push(row);
        }
    }
    groups
}
fn device_code(flow: &str, code: &str, w: &mut Window, cx: &mut App) -> Div {
    let state = w.use_keyed_state(
        SharedString::from(format!("device-code:{flow}")),
        cx,
        |_, _| None::<String>,
    );
    let shown = state.read(cx).as_deref() == Some(code);
    let current = code.to_owned();
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            Button::new("reveal-device-code")
                .label(if shown {
                    "認証用コードを隠す"
                } else {
                    "認証用コードを表示"
                })
                .on_click(move |_, _, cx| {
                    state.update(cx, |s, cx| {
                        *s = (!shown).then(|| current.clone());
                        cx.notify();
                    })
                }),
        )
        .when(shown, |el| {
            el.child(div().font_family("monospace").child(code.to_owned()))
        })
}
impl settings::Nav {
    fn show_login_result(&self, flow: &Value) -> bool {
        !flow.is_null()
            && self
                .providers
                .superseded_flow
                .as_deref()
                .is_none_or(|id| flow["id"] != id)
    }
    fn start_auth(&mut self, action: &AuthAction, flow: &Value) {
        if matches!(action, AuthAction::Begin(_)) {
            self.providers.superseded_flow = flow["id"].as_str().map(str::to_owned);
        }
        self.providers.pending = true;
        self.providers.prompt = None;
        self.note.clear();
    }
    fn finish_auth(&mut self, result: Result<bool, Error>) {
        self.providers.pending = false;
        match result {
            Ok(list_failed) => self.providers.list_failed = list_failed,
            Err(_) => {
                self.note =
                    "資格情報の状態は未確認です。再入力する前にログイン状態を再確認してください。"
                        .into()
            }
        }
    }
    fn logout_ack(&mut self, result: Result<Value, Error>, old_flow: Option<String>) -> bool {
        self.note = match result {
            Ok(v) if v["refreshRequired"] == true => {
                "保存した認証情報を削除しました。反映するには PC の Pi で /reload または再起動してください。"
            }
            Ok(_) => "ログアウトしました。起動中の Pi には /reload または再起動後に反映されます。",
            Err(_) => {
                self.providers.pending = false;
                self.providers.login = None;
                self.note = "資格情報の削除結果は不明です。状態を再確認してください。".into();
                return false;
            }
        }
        .into();
        if old_flow.is_some() {
            self.providers.superseded_flow = old_flow;
        }
        true
    }
    fn logout_list(&mut self, result: Result<Value, Error>) {
        self.providers.pending = false;
        self.providers.list_failed = false;
        if result.is_err() {
            self.note
                .push_str(" ログイン一覧の再取得に失敗しました。状態を再確認してください。");
            self.providers.login = None;
        }
    }
}
impl Desktop {
    fn auth_request(
        &self,
        n: &Navigation,
        action: AuthAction,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let s = &n.read(cx).providers;
        let Some(login) = s.login.clone().filter(|_| !s.pending) else {
            return;
        };
        n.update(cx, |s, _| {
            let flow = s
                .page
                .as_ref()
                .and_then(|page| self.machines.get(page.host()))
                .map(|machine| machine.rows.get("auth", login.instance()))
                .unwrap_or(&Value::Null);
            s.start_auth(&action, flow);
        });
        self.page_task(
            n,
            w,
            cx,
            async move |_| {
                login.request(action).await?;
                Ok::<_, Error>(login.request(AuthAction::List).await.is_err())
            },
            |_, n, result, _, cx| {
                n.update(cx, |s, _| s.finish_auth(result));
            },
        );
    }
    pub(super) fn providers_view(
        &mut self,
        n: &Navigation,
        p: &Colors,
        id: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let online = self
            .machines
            .get(id)
            .is_some_and(|m| m.rows.get("host", "connection") == "connected");
        let page = n.read(cx).page.as_ref().is_some_and(|p| p.visible(self));
        if !page && (online || n.read(cx).providers.login.is_some()) {
            self.navigate(n, Route::Providers(id.into()), cx);
        }
        if !online {
            return div().child("PC に接続していません。再接続後にログイン状態を取り直します。");
        }
        let reload = {
            self.nav_button(
                n,
                "auth-reload",
                "ログイン状態を再確認",
                Route::Providers(id.into()),
                cx,
            )
        };
        if !n.read(cx).providers.opening && n.read(cx).providers.login.is_none() {
            n.update(cx, |s, _| s.providers.opening = true);
            self.page_task(
                n,
                w,
                cx,
                async move |p| {
                    let l = p.login().await?;
                    l.request(AuthAction::List).await?;
                    Ok::<_, Error>(l)
                },
                |_, n, r, _, cx| {
                    n.update(cx, |s, _| match r {
                        Ok(l) => s.providers.login = Some(Arc::new(l)),
                        Err(_) => {
                            s.note = "専用の Pi を開けませんでした。接続を確認してください。".into()
                        }
                    });
                },
            );
        }
        let Some(login) = n.read(cx).providers.login.clone() else {
            return div()
                .child(if n.read(cx).note.is_empty() {
                    "専用の Pi を開いています…"
                } else {
                    "ログイン状態は未確認です"
                })
                .child(self.nav_button(
                    n,
                    "auth-retry",
                    "再確認",
                    Route::Providers(id.into()),
                    cx,
                ));
        };
        let rows = &self.machines[id].rows;
        let flow = rows.get("auth", login.instance());
        let providers = rows.get("provider", login.instance());
        let busy = n.read(cx).providers.pending;
        let active = matches!(Outcome::from_flow(flow), Outcome::Active);
        let mut v = div()
            .flex()
            .flex_col()
            .gap_3()
            .when(n.read(cx).providers.list_failed, |d| {
                d.child(p.caption("ログイン一覧の再取得に失敗しました。状態を再確認してください。"))
            });
        if active {
            let fid = flow["id"].as_str().unwrap_or_default().to_string();
            if n.read(cx).providers.following.as_ref() != Some(&fid) {
                let l = login.clone();
                n.update(cx, |s, _| s.providers.following = Some(fid.clone()));
                self.page_task(
                    n, w, cx,
                    async move |_| {
                        while l.follow_once().await? {}
                        Ok::<_, Error>(l.request(AuthAction::List).await.is_err())
                    },
                    |_, n, result, _, cx| {
                        n.update(cx, |s, _| match result {
                            Ok(failed) => s.providers.list_failed = failed,
                            Err(_) => s.note = "ログインの進行状況は未確認です。再入力前に状態を再確認してください。".into(),
                        });
                    },
                );
            }
            let mut form = section(
                flow["provider"].as_str().unwrap_or("ログイン").to_string(),
                p,
            )
            .child(match flow["status"].as_str() {
                Some("input") => "入力を待っています",
                Some("saving") => "資格情報を保存しています…",
                _ => "ログイン中…",
            });
            if let Some(url) = flow["authorization"]["url"]
                .as_str()
                .and_then(|s| reqwest::Url::parse(s).ok())
                .filter(|u| {
                    u.scheme() == "https"
                        && u.has_host()
                        && u.username().is_empty()
                        && u.password().is_none()
                })
            {
                form = form.child(
                    Button::new("oauth")
                        .label("ブラウザで認証 ↗")
                        .on_click(move |_, _, cx| cx.open_url(url.as_str())),
                );
                if let Some(code) = flow["authorization"]["userCode"]
                    .as_str()
                    .filter(|code| !code.is_empty())
                {
                    form = form.child(device_code(&fid, code, w, cx));
                }
            }
            if let Some(pid) = flow["prompt"]["id"].as_str().filter(|_| !busy) {
                let pid = pid.to_string();
                let label = if flow["prompt"]["kind"] == "api_key" {
                    "API キー"
                } else {
                    "認証コード"
                };
                if n.read(cx)
                    .providers
                    .prompt
                    .as_ref()
                    .is_none_or(|(a, b, _)| a != &fid || b != &pid)
                {
                    let i = cx.new(|cx| InputState::new(w, cx).placeholder(label).masked(true));
                    n.update(cx, |s, _| {
                        s.providers.prompt = Some((fid.clone(), pid.clone(), i))
                    });
                }
                let input = n.read(cx).providers.prompt.as_ref().unwrap().2.clone();
                let nav = n.clone();
                let f = fid.clone();
                form = form
                    .child(p.caption(label))
                    .child(
                        div()
                            .capture_action(|_: &gpui_component::input::Copy, _, cx| {
                                cx.stop_propagation()
                            })
                            .capture_action(|_: &gpui_component::input::Cut, _, cx| {
                                cx.stop_propagation()
                            })
                            .child(context::field(&input, false)),
                    )
                    .child(p.primary("respond", cx).label("送信").on_click(cx.listener(
                        move |app, _, w, cx| {
                            if input.update(cx, |i, cx| i.marked_text_range(w, cx).is_some()) {
                                return;
                            }
                            let value = input.read(cx).value().to_string();
                            if value.is_empty() {
                                return;
                            }
                            app.auth_request(
                                &nav,
                                AuthAction::Respond(AuthRespondParams {
                                    flow_id: f.clone(),
                                    prompt_id: pid.clone(),
                                    value,
                                }),
                                w,
                                cx,
                            );
                        },
                    )));
            }
            let nav = n.clone();
            return v
                .child(form)
                .child(
                    Button::new("auth-cancel")
                        .label("ログインをキャンセル")
                        .disabled(busy)
                        .on_click(cx.listener(move |app, _, w, cx| {
                            app.auth_request(
                                &nav,
                                AuthAction::Cancel(AuthCancelParams {
                                    flow_id: fid.clone(),
                                }),
                                w,
                                cx,
                            )
                        })),
                )
                .when(!n.read(cx).note.is_empty(), |v| {
                    v.child(reload.disabled(busy))
                });
        }
        if n.read(cx).providers.prompt.is_some() || n.read(cx).providers.following.is_some() {
            n.update(cx, |s, _| {
                s.providers.prompt = None;
                s.providers.following = None;
            });
        }
        v = v.child(p.caption("認証情報は PC のログイン専用の Pi で保存します。作業中の Pi に反映するには /reload または再起動が必要です。"));
        if n.read(cx).show_login_result(flow) {
            v = v.child(p.caption(match Outcome::from_flow(flow) {
                Outcome::Saved => {
                    "ログインしました。起動中の Pi には /reload または再起動後に反映されます。"
                }
                Outcome::NeedsReload => {
                    "認証情報は保存されました。反映するには PC の Pi で /reload または再起動してください。再入力は不要です。"
                }
                Outcome::StorageUnknown => {
                    "認証情報が保存されたか不明です。再入力する前にプロバイダーの一覧を確認してください。"
                }
                Outcome::Failed => "ログインできませんでした",
                Outcome::Expired => "期限が切れました",
                _ => "ログイン操作は終了しました",
            }));
        }
        if let Some((provider, name)) = &n.read(cx).providers.logout {
            let provider = *provider;
            let nav = n.clone();
            let back = n.clone();
            let label = self
                .hosts
                .iter()
                .find(|h| h.id == id)
                .map(|h| format!("{}（{}）", h.name, h.address))
                .unwrap_or_default();
            let cancel = Button::new("dismiss-logout")
                .label("キャンセル")
                .on_click(cx.listener(move |_, _, _, cx| {
                    back.update(cx, |s, _| s.providers.logout = None);
                    cx.notify();
                }));
            let old_flow = flow["id"]
                .as_str()
                .filter(|_| flow["provider"].as_str() == Some(name.as_str()))
                .map(str::to_owned);
            let confirm = Button::new("logout-yes")
                .danger()
                .label("ログアウト")
                .on_click(cx.listener(move |app, _, w, cx| {
                    nav.update(cx, |s, _| {
                        s.providers.logout = None;
                        s.providers.pending = true;
                    });
                    let l = login.clone();
                    let list = login.clone();
                    let old_flow = old_flow.clone();
                    app.page_task(
                        &nav,
                        w,
                        cx,
                        async move |_| l.logout_confirmation(provider).confirm().await,
                        move |app, n, r, w, cx| {
                            if n.update(cx, |s, _| s.logout_ack(r, old_flow)) {
                                app.page_task(
                                    n,
                                    w,
                                    cx,
                                    async move |_| list.request(AuthAction::List).await,
                                    |_, n, result, _, cx| {
                                        n.update(cx, |s, _| s.logout_list(result))
                                    },
                                );
                            }
                        },
                    );
                }));
            return v.child(
                section("ログアウトの確認", p)
                    .child(format!(
                        "{label} の {name} に保存された資格情報を削除します。"
                    ))
                    .child(line().child(cancel).child(confirm)),
            );
        }
        match providers.as_array() {
            None => v = v.child("一覧は未確認です。"),
            Some(a) if a.is_empty() => v = v.child("プロバイダーはありません。"),
            Some(a) => {
                for (vendor, members) in vendors(a) {
                    let mut section = section(vendor.to_owned(), p);
                    for row in members {
                        let Ok(provider) =
                            serde_json::from_value::<AuthProviderId>(row["id"].clone())
                        else {
                            continue;
                        };
                        let name = row["id"].as_str().unwrap_or_default().to_string();
                        let mut controls = line();
                        for m in row["methods"].as_array().into_iter().flatten() {
                            let Ok(method) = serde_json::from_value::<ProviderAuthType>(m.clone())
                            else {
                                continue;
                            };
                            let nav = n.clone();
                            controls = controls.child(
                                Button::new(SharedString::from(format!("{name}-{method:?}")))
                                    .label(if method == ProviderAuthType::ApiKey {
                                        "API キーを設定"
                                    } else {
                                        "ブラウザでログイン"
                                    })
                                    .disabled(busy)
                                    .on_click(cx.listener(move |app, _, w, cx| {
                                        app.auth_request(
                                            &nav,
                                            AuthAction::Begin(AuthBeginParams { provider, method }),
                                            w,
                                            cx,
                                        )
                                    })),
                            );
                        }
                        if row["stored"].is_string() {
                            let nav = n.clone();
                            let shown = name.clone();
                            controls = controls.child(
                                Button::new(SharedString::from(format!("logout-{name}")))
                                    .danger()
                                    .label("ログアウト…")
                                    .disabled(busy)
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        nav.update(cx, |s, _| {
                                            s.providers.logout = Some((provider, shown.clone()))
                                        });
                                        cx.notify();
                                    })),
                            );
                        }
                        section = section.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .children(
                                    row["member"]
                                        .as_str()
                                        .filter(|s| !s.is_empty())
                                        .map(str::to_owned),
                                )
                                .child(if row["configured"] == true {
                                    "設定済み"
                                } else {
                                    "未設定"
                                })
                                .child(controls),
                        );
                    }
                    v = v.child(section);
                }
            }
        }
        v.child(reload.disabled(busy))
    }
}

#[cfg(test)]
mod tests {
    use super::device_code;
    use gpui::{
        ClipboardItem, Context, Modifiers, TestAppContext, Window, div, point, prelude::*, px,
    };

    #[test]
    fn list_failure_preserves_acknowledged_logout_and_reload_instruction() {
        use crate::screen::settings::Nav;
        use pimate_core::Error;
        let mut nav = Nav::default();
        let previous =
            serde_json::json!({"id":"saved", "status":"succeeded","refreshRequired":true});
        nav.providers.list_failed = true;
        nav.note = "照会に失敗しました".into();
        assert!(nav.show_login_result(&previous));
        nav.providers.pending = true;
        assert!(nav.logout_ack(
            Ok(serde_json::json!({"refreshRequired":true})),
            Some("saved".into())
        ));
        assert!(nav.note.contains("/reload") && nav.providers.pending);
        nav.logout_list(Ok(serde_json::Value::Null));
        assert!(!nav.show_login_result(&previous));
        assert!(nav.show_login_result(&serde_json::json!({"id":"next","status":"succeeded"})));
        nav.logout_list(Err(Error::Timeout));
        assert!(
            !nav.providers.pending
                && nav.note.contains("保存した認証情報を削除しました")
                && nav.note.contains("/reload")
                && nav.note.contains("再取得に失敗")
        );
        assert!(!nav.logout_ack(Err(Error::OutcomeUnknown), None));
        assert!(
            nav.note.contains("削除結果は不明")
                && !nav.note.contains("保存した認証情報を削除しました")
        );
    }
    #[test]
    fn timed_out_begin_hides_previous_success_but_list_failure_keeps_new_success() {
        use super::{AuthAction, AuthBeginParams, AuthProviderId, ProviderAuthType};
        let mut nav = crate::screen::settings::Nav::default();
        let previous = serde_json::json!({"id":"first","provider":"openai","status":"succeeded"});
        let next = serde_json::json!({"id":"second","provider":"anthropic","status":"succeeded","refreshRequired":true});
        let begin = AuthAction::Begin(AuthBeginParams {
            provider: AuthProviderId::Anthropic,
            method: ProviderAuthType::ApiKey,
        });
        assert!(nav.show_login_result(&previous));
        nav.start_auth(&begin, &previous);
        nav.finish_auth(Err(pimate_core::Error::Timeout));
        assert!(nav.note.contains("未確認") && !nav.show_login_result(&previous));
        nav.start_auth(&begin, &previous);
        nav.finish_auth(Ok(true));
        assert!(nav.providers.list_failed && nav.show_login_result(&next));
    }
    #[test]
    fn vendors_keep_member_methods_and_ids_separate() {
        let rows = serde_json::json!([{"vendor":"OpenAI","id":"openai","member":"API キー","methods":["api_key"]},{"vendor":"Anthropic","id":"anthropic"},{"vendor":"OpenAI","id":"openai-codex","member":"ChatGPT","methods":["oauth"]}]);
        let groups = super::vendors(rows.as_array().unwrap());
        assert_eq!(
            groups.keys().copied().collect::<Vec<_>>(),
            ["Anthropic", "OpenAI"]
        );
        assert_eq!(groups["OpenAI"][1]["member"], "ChatGPT");
        assert_eq!(groups["OpenAI"][0]["methods"][0], "api_key");
    }
    #[gpui::test]
    fn device_code_requires_explicit_reveal(cx: &mut TestAppContext) {
        struct Probe(&'static str);
        impl Render for Probe {
            fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                div()
                    .w(px(240.))
                    .child(device_code("demo-flow", self.0, w, cx))
                    .child(div().h(px(1.)).debug_selector(|| "after-code".into()))
            }
        }
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.write_to_clipboard(ClipboardItem::new_string("before".into()));
        });
        let (probe, w) = cx.add_window_view(|_, _| Probe("DEMO-ONLY"));
        let hidden = w.debug_bounds("after-code").unwrap().top();
        w.simulate_click(point(px(40.), px(15.)), Modifiers::none());
        assert!(w.debug_bounds("after-code").unwrap().top() > hidden);
        w.simulate_click(point(px(40.), px(15.)), Modifiers::none());
        assert_eq!(w.debug_bounds("after-code").unwrap().top(), hidden);
        w.simulate_click(point(px(40.), px(15.)), Modifiers::none());
        probe.update(w, |p, cx| {
            p.0 = "REPLACEMENT";
            cx.notify()
        });
        w.run_until_parked();
        assert_eq!(w.debug_bounds("after-code").unwrap().top(), hidden);
        assert_eq!(
            w.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "before"
        );
    }
}
