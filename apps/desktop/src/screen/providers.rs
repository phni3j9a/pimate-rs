use super::*;
use pimate_protocol::{
    AuthBeginParams, AuthCancelParams, AuthProviderId, AuthRespondParams, ProviderAuthType,
};
use settings::{group, line};
use state::{AuthAction, CredentialOutcome as Outcome, Login};
#[derive(Default)]
pub(super) struct ProviderData {
    opening: bool,
    login: Option<Arc<Login>>,
    pending: bool,
    following: Option<String>,
    logout: Option<(AuthProviderId, String)>,
    prompt: Option<(String, String, Entity<InputState>)>,
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
            s.providers.pending = true;
            s.providers.prompt = None;
            s.note.clear();
        });
        self.page_task(n,w,cx,async move|_|{
            login.request(action).await?;
            login.request(AuthAction::List).await
        },|_,n,result,_,cx|{n.update(cx,|s,_|{
            s.providers.pending=false;
            if result.is_err(){ s.note="資格情報の状態は未確認です。再入力する前にログイン状態を再確認してください。".into();}
        });});
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
        let mut v = div().flex().flex_col().gap_3();
        if active {
            let fid = flow["id"].as_str().unwrap_or_default().to_string();
            if n.read(cx).providers.following.as_ref() != Some(&fid) {
                let l = login.clone();
                n.update(cx, |s, _| s.providers.following = Some(fid.clone()));
                self.page_task(n,w,cx,async move|_|{while l.follow_once().await?{}l.request(AuthAction::List).await},|_,n,r,_,cx|{if r.is_err(){n.update(cx,|s,_|s.note="ログインの進行状況は未確認です。再入力前に状態を再確認してください。".into());}});
            }
            let mut form = group(
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
                if let Some(code) = flow["authorization"]["userCode"].as_str() {
                    form = form.child(format!("認証用コード　{code}"));
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
                            .child(Input::new(&input)),
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
            return v.child(form).child(
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
            );
        }
        if n.read(cx).providers.prompt.is_some() || n.read(cx).providers.following.is_some() {
            n.update(cx, |s, _| {
                s.providers.prompt = None;
                s.providers.following = None;
            });
        }
        if !flow.is_null() {
            v = v.child(p.caption(match Outcome::from_flow(flow) {
                Outcome::Saved => "ログインしました。",
                Outcome::NeedsReload => {
                    "資格情報は保存済みです。PC の Pi を /reload してください。再入力は不要です。"
                }
                Outcome::StorageUnknown => {
                    "資格情報の保存結果は不明です。再入力する前にログイン状態を再確認してください。"
                }
                Outcome::Failed => "ログインできませんでした。",
                Outcome::Expired => "ログインの期限が切れました。",
                _ => "ログイン操作は終了しました。",
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
            let confirm = Button::new("logout-yes")
                .danger()
                .label("ログアウト")
                .on_click(cx.listener(move |app, _, w, cx| {
                    nav.update(cx, |s, _| {
                        s.providers.logout = None;
                        s.providers.pending = true;
                    });
                    let l = login.clone();
                    app.page_task(
                        &nav,
                        w,
                        cx,
                        async move |_| {
                            let reply = l.logout_confirmation(provider).confirm().await?;
                            l.request(AuthAction::List).await?;
                            Ok::<_, Error>(reply)
                        },
                        |_, n, r, _, cx| {
                            n.update(cx, |s, _| {
                                s.providers.pending = false;
                                s.note = match r {
                                    Ok(v) if v["refreshRequired"] == true => {
                                        "削除済みです。PC の Pi を /reload してください。"
                                    }
                                    Ok(_) => "ログアウトしました。",
                                    Err(_) => {
                                        "資格情報の削除結果は不明です。状態を再確認してください。"
                                    }
                                }
                                .into();
                            });
                        },
                    );
                }));
            return v.child(
                group("ログアウトの確認", p)
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
                for row in a {
                    let Ok(provider) = serde_json::from_value::<AuthProviderId>(row["id"].clone())
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
                    v = v.child(
                        group(name, p)
                            .child(if row["configured"] == true {
                                "設定済み"
                            } else {
                                "未設定"
                            })
                            .child(controls),
                    );
                }
            }
        }
        v.child(reload.disabled(busy))
    }
}
