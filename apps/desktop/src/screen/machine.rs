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
    pending: Option<(String, Decision)>,
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
        if let Some((warning, _)) = &m.pending {
            let cancel = n.clone();
            let run = n.clone();
            let target = h.clone();
            return group("操作の確認",p).child(format!("{host_label}　·　{}",h.address)).child(warning.clone()).child(line().child(Button::new("cancel-action").label("キャンセル").on_click(cx.listener(move|_,_,_,cx|{cancel.update(cx,|s,_|s.machine.pending=None);cx.notify();}))).child(Button::new("confirm-action").danger().label("確認して実行").on_click(cx.listener(move|app,_,w,cx|{if !app.hosts.contains(&target){app.navigate(&run,Route::Settings,cx);return;}let Some((_,decision))=run.update(cx,|s,_|s.machine.pending.take())else{return;};match decision {Decision::Host(confirm)=>{run.update(cx,|s,_|{s.machine.busy=true;s.note="PC の応答を確認中…".into();});app.page_task(&run,w,cx,async move|_|confirm.confirm().await,|_,n,r,_,cx|{n.update(cx,|s,_|{s.machine=Default::default();s.note=match r{Ok(v)=>v["to"].as_str().map(|v|format!("{v} に更新しました。")).unwrap_or_else(||"PC が操作を受け付けました。".into()),Err(_)=>"操作結果は不明です。再実行する前に状態を確認してください。".into()};});});},Decision::Local(forget)=>{let nav=run.clone();let host=target.clone();run.update(cx,|s,_|s.machine.busy=true);cx.spawn(async move|app,cx|{let r=cx.background_executor().spawn(async move{forget.confirm()}).await;let _=app.update(cx,|app,cx|{let same=nav.read(cx).route==Route::Machine(host.id.clone());match r{Ok(h)=>{if same{app.navigate(&nav,Route::Settings,cx);}app.removed(&h,cx);},Err(_)=>if same{nav.update(cx,|s,_|{s.machine.busy=false;s.note="削除結果を確認できません。登録一覧を読み直してください。".into();});}}cx.notify();});}).detach();}}cx.notify();}))));
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
            Button::new(SharedString::from(format!("confirm-{label}-{}",target.as_deref().unwrap_or("")))).danger().label(label).disabled(m.busy).on_click(cx.listener(move|app,_,_,cx|{if !app.hosts.contains(&h){return;} let fixed=match &op {None=>app.forget_confirmation(&h.id).map(|f|("この端末の登録と、このマシンの下書きを削除します。PC 側の登録は取り消しません。".to_string(),Decision::Local(f))),Some(a)=>{let Some(page)=&page else{return;};let op=match a{Maintenance::UpdateLatest=>Maintenance::UpdateLatest,Maintenance::Restart=>Maintenance::Restart,Maintenance::Revoke(i)=>Maintenance::Revoke(i.clone())};page.confirmation(op).map(|c|(format!("{} {}",target.as_deref().unwrap_or(""),c.warning()),Decision::Host(c)))}};nav.update(cx,|s,_|match fixed{Ok(v)=>s.machine.pending=Some(v),Err(_)=>s.note="対象が変わりました。マシンを開き直してください。".into()});cx.notify();}))
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
            caps["platform"].as_str().unwrap_or("OS 未確認"),
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
                    .disabled(!supports("authSession")),
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
}
