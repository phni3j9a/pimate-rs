use super::*;
use settings::{group, line};
use state::{Confirmation, Forget, MachineQuery, Maintenance};
#[derive(Default)]
pub(super) struct MachineData {
    loaded: bool,
    check: Value,
    devices: Value,
    github: Value,
    busy: bool,
    pending: Option<(Host, String, Decision)>,
}
enum Decision {
    Host(Confirmation),
    Local(Forget),
}
fn github_status(v: &Value) -> String {
    match (v["installed"].as_bool(), v["loggedIn"].as_bool()) {
        (Some(false), _) => "PC に GitHub CLI（gh）がありません".into(),
        (Some(true), Some(false)) => "PC で gh auth login を実行してください".into(),
        (Some(true), Some(true)) => {
            format!("{} でログイン中", v["login"].as_str().unwrap_or("GitHub"))
        }
        _ => "ログイン状態は未確認です".into(),
    }
}
fn provider_supported(caps: &Value) -> bool {
    caps["authSession"] == true && caps["providerAuth"] == true
}
impl Desktop {
    pub(super) fn machine_view(
        &mut self,
        n: &Navigation,
        p: &Colors,
        id: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(h) = self.hosts.iter().find(|h| h.id == id).cloned() else {
            return div().child("このマシンは登録されていません。");
        };
        if n.read(cx).page.as_ref().is_some_and(|p| !p.visible(self))
            || n.read(cx)
                .machine
                .pending
                .as_ref()
                .is_some_and(|(shown, _, _)| shown != &h)
        {
            self.navigate(n, Route::Machine(id.into()), cx);
            n.update(cx, |s, _| {
                s.note = "接続先が変わりました。実行済みの操作は自動再送しません。".into()
            });
        }
        // Registration can appear before Desktop::attach supplies its connection.
        if n.read(cx).page.is_none()
            && let Ok(page) = self.open_machine(id)
        {
            n.update(cx, |s, _| s.page = Some(page));
        }
        let page = n.read(cx).page.clone().filter(|p| p.visible(self));
        let rows = self.machines.get(id).map(|m| &m.rows);
        let caps = rows.map(|r| r.get("host", "info")).unwrap_or(&Value::Null);
        let online =
            page.is_some() && rows.is_some_and(|r| r.get("host", "connection") == "connected");
        let supports = |name: &str| online && caps[name] == true;
        let dev = supports("pairedDevices");
        let gh = supports("githubStatus");
        let upd = supports("packageUpdate");
        if online && !n.read(cx).machine.loaded {
            n.update(cx, |s, _| {
                s.machine.loaded = true;
                s.machine.busy = true;
            });
            self.page_task(
                n,
                w,
                cx,
                async move |p| {
                    let mut results = [Value::Null, Value::Null, Value::Null];
                    let mut error = false;
                    for (i, (ok, q)) in [
                        (upd, MachineQuery::UpdateCheck),
                        (dev, MachineQuery::Devices),
                        (gh, MachineQuery::GithubStatus),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        if ok {
                            match p.query(q).await {
                                Ok(v) => results[i] = v,
                                Err(_) => error = true,
                            }
                        }
                    }
                    (results, error)
                },
                |_, n, ([check, devices, github], error), _, cx| {
                    n.update(cx, |s, _| {
                        s.machine.busy = false;
                        s.machine.check = check;
                        s.machine.devices = devices["devices"].clone();
                        s.machine.github = github["status"].clone();
                        if error {
                            s.note = "PC の状態を一部確認できませんでした。".into();
                        }
                    });
                },
            );
        }
        let m = &n.read(cx).machine;
        let id = h.id.clone();
        let host_label = self.host_label(&h);
        if let Some((fixed, warning, _)) = &m.pending {
            let cancel = n.clone();
            let run = n.clone();
            let cancel = Button::new("cancel-action")
                .label("キャンセル")
                .on_click(cx.listener(move |_, _, _, cx| {
                    cancel.update(cx, |s, _| s.machine.pending = None);
                    cx.notify();
                }));
            let confirm = Button::new("confirm-action")
                .danger()
                .label("確認して実行")
                .on_click(cx.listener(move |app, _, w, cx| app.confirm_machine(&run, w, cx)));
            return group("操作の確認", p)
                .child(format!("{}　·　{}", fixed.name, fixed.address))
                .child(warning.clone())
                .child(line().child(cancel).child(confirm));
        }
        let c = &m.check;
        let mut maint = group("PC の PiMate", p).child(format!(
            "現在の版　{}",
            c["current"]
                .as_str()
                .or(caps["version"].as_str())
                .unwrap_or("未確認")
        ));
        maint = maint.child(p.caption(if c["reason"] == "not_package" {
            "開発版は PC で更新してください。".into()
        } else if c["latest"].is_string() && c["latest"] == c["current"] {
            "最新です".into()
        } else {
            format!(
                "照会時の最新版　{}",
                c["latest"].as_str().unwrap_or("未確認")
            )
        }));
        let button = |label: &str, op: Option<Maintenance>, target: Option<String>| {
            let label = label.to_string();
            let page = page.clone();
            let h = h.clone();
            let nav = n.clone();
            let id = SharedString::from(format!(
                "confirm-{label}-{}",
                target.as_deref().unwrap_or("")
            ));
            let handler=cx.listener(move|app,_,_: &mut Window,cx:&mut Context<Self>|{
                if !app.hosts.contains(&h){return;}
                let fixed=match &op {
                    None=>app.forget_confirmation(&h.id).map(|f| ("この端末の登録と、このマシンの下書きを削除します。PC 側の登録は取り消しません。".into(),Decision::Local(f))),
                    Some(a)=>{let Some(page)=&page else{return;};
                        let op=match a{Maintenance::UpdateLatest=>Maintenance::UpdateLatest,Maintenance::Restart=>Maintenance::Restart,Maintenance::Revoke(i)=>Maintenance::Revoke(i.clone())};
                        page.confirmation(op).map(|c|(format!("{} {}",target.as_deref().unwrap_or(""),c.warning()),Decision::Host(c)))
                    }
                };
                nav.update(cx,|s,_|match fixed {Ok((warning,decision))=>s.machine.pending=Some((h.clone(),warning,decision)),Err(_)=>s.note="対象が変わりました。マシンを開き直してください。".into()});cx.notify();
            });
            Button::new(id)
                .danger()
                .label(label)
                .disabled(m.busy)
                .on_click(handler)
        };
        let mut actions = line();
        if upd && c["canUpdate"] == true && c["latest"] != c["current"] {
            actions = actions.child(button(
                "実行時の最新版に更新…",
                Some(Maintenance::UpdateLatest),
                None,
            ));
        }
        if supports("hostRestart") {
            actions = actions.child(button("Host を再起動…", Some(Maintenance::Restart), None));
        }
        let mut devices = group("ペアリング済みの端末", p);
        if let Some(list) = m.devices.as_array().filter(|_| online) {
            if list.is_empty() {
                devices = devices.child(p.caption("登録された端末はありません"));
            }
            for d in list {
                let name = d["name"].as_str().unwrap_or("端末");
                let mut row = line().justify_between().child(name.to_string());
                if d["current"] == true {
                    row = row.child(p.caption("この端末"));
                } else if let Some(id) = d["deviceId"].as_str().filter(|_| dev) {
                    row = row.child(button(
                        "取り消し…",
                        Some(Maintenance::Revoke(id.into())),
                        Some(format!("{name}（{id}）")),
                    ));
                }
                devices = devices.child(row);
            }
        } else {
            devices = devices.child(p.caption("端末一覧は未確認です"));
        }
        let summary = format!(
            "{}　·　{}　{}",
            connection(&h, rows),
            match caps["platform"].as_str() {
                Some("darwin") => "macOS",
                Some("linux") => "Linux",
                Some(v) => v,
                None => "OS 未確認",
            },
            rows.filter(|r| r.get("host", "listKnown") == true)
                .map(|r| format!("·　Pi {} 件", r.group("instance").len()))
                .unwrap_or_default()
        );
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                group(host_label, p)
                    .child(summary)
                    .child(p.caption(h.address.clone())),
            )
            .child(maint.child(actions))
            .child(
                group("プロバイダー", p).child(
                    self.nav_button(
                        n,
                        "providers",
                        "プロバイダーのログイン　›",
                        Route::Providers(id.clone()),
                        cx,
                    )
                    .disabled(!online || !provider_supported(caps)),
                ),
            )
            .child(group("GitHub", p).child(github_status(if online {
                &m.github
            } else {
                &Value::Null
            })))
            .child(devices)
            .when(m.busy, |v| v.child(p.caption("応答を確認中…")))
            .child(
                line()
                    .child(
                        self.nav_button(
                            n,
                            "refresh-machine",
                            "状態を再読み込み",
                            Route::Machine(id),
                            cx,
                        )
                        .disabled(online && m.busy),
                    )
                    .child(button("このマシンを削除…", None, None)),
            )
    }
    fn confirm_machine(&mut self, n: &Navigation, w: &mut Window, cx: &mut Context<Self>) {
        let Some((target, _, decision)) = n.update(cx, |s, _| s.machine.pending.take()) else {
            return;
        };
        if !self.hosts.contains(&target) {
            self.navigate(n, Route::Settings, cx);
            return;
        }
        n.update(cx, |s, _| {
            s.machine.busy = true;
            s.note = "操作結果を確認中…".into();
        });
        match decision {
            Decision::Host(confirm) => self.page_task(
                n,
                w,
                cx,
                async move |_| confirm.confirm().await,
                |_, n, r, _, cx| {
                    n.update(cx, |s, _| {
                        s.machine = Default::default();
                        s.note = match r {
                            Ok(v) => v["to"]
                                .as_str()
                                .map(|v| format!("{v} に更新しました。"))
                                .unwrap_or_else(|| "PC が操作を受け付けました。".into()),
                            Err(_) => {
                                "操作結果は不明です。再実行する前に状態を確認してください。".into()
                            }
                        };
                    });
                },
            ),
            Decision::Local(forget) => {
                let nav = n.clone();
                let generation = nav.read(cx).generation.clone();
                // Dispatch can outlive this screen. Reconcile storage even if the user navigated away.
                cx.spawn(async move |app, cx| {
                    let result = cx
                        .background_executor()
                        .spawn(async move { forget.confirm() })
                        .await;
                    let _ = app.update(cx, |app, cx| {
                        let same = nav.read(cx).route == Route::Machine(target.id.clone())
                            && Arc::ptr_eq(&generation, &nav.read(cx).generation);
                        match result {
                            Ok(h) => {
                                if same {
                                    app.navigate(&nav, Route::Settings, cx);
                                }
                                app.removed(&h, cx);
                            }
                            Err(_) if same => {
                                nav.update(cx, |s, _| {
                                    s.machine.busy = false;
                                    s.note =
                                        "削除結果を確認できません。登録一覧を読み直してください。"
                                            .into();
                                });
                            }
                            Err(_) => {}
                        }
                        cx.notify();
                    });
                })
                .detach();
                cx.notify();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::provider_supported;
    #[test]
    fn provider_login_needs_both_capabilities() {
        for session in [false, true] {
            for provider in [false, true] {
                assert_eq!(
                    provider_supported(
                        &serde_json::json!({"authSession":session,"providerAuth":provider})
                    ),
                    session && provider
                );
            }
        }
    }
    #[gpui::test]
    fn detail_opened_before_attach_loads_without_manual_refresh(cx: &mut gpui::TestAppContext) {
        use super::{Colors, Desktop, Machine, Route, Rows, settings::Nav};
        use gpui::AppContext;
        use pimate_core::{Item, Snapshot};
        use std::sync::Arc;
        let (_socket, h, core) = super::super::tests::test_host();
        cx.update(gpui_component::init);
        let (app, w) = cx.add_window_view(|w, cx| Desktop::new(core.clone(), w, cx));
        let nav = w.new(|_| Nav::default());
        w.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.hosts = vec![h.clone()];
                app.navigate(&nav, Route::Machine(h.id.clone()), cx);
                let render = |app: &mut Desktop,
                              window: &mut gpui::Window,
                              cx: &mut gpui::Context<Desktop>| {
                    let _ = app.machine_view(&nav, &Colors::new(false), &h.id, window, cx);
                };
                render(app, window, cx);
                assert!(nav.read(cx).page.is_none() && !nav.read(cx).machine.loaded);
                let (tx, _rx) = crate::Delivery::channel();
                let conn = core.connect(h.id.clone(), Arc::new(tx)).unwrap();
                let rows = Rows::snapshot(Snapshot {
                    revision: 1,
                    items: vec![Item {
                        position: 0,
                        collection: "host".into(),
                        id: "connection".into(),
                        json: "\"connected\"".into(),
                    }],
                });
                app.machines.insert(
                    h.id.clone(),
                    Machine {
                        registration: h.clone(),
                        conn,
                        rows,
                    },
                );
                render(app, window, cx);
                assert!(
                    nav.read(cx).page.as_ref().unwrap().visible(app) && nav.read(cx).machine.loaded
                );
                render(app, window, cx);
                assert_eq!(nav.read(cx).tasks.len(), 1);
            })
        });
    }
}
