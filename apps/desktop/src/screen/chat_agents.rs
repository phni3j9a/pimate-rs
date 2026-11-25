use super::chat_review::Inspector;
use super::*;
use state::{Child, Instruction, Page, WorkRecord};
fn state_label(s: &str) -> &str {
    match s {
        "running" => "実行中",
        "queued" => "待機中",
        "waiting" => "入力待ち",
        "complete" => "完了",
        "paused" => "一時停止",
        "failed" => "失敗",
        "stopping" => "停止中",
        "stopped" => "停止済み",
        _ => "状態不明",
    }
}
fn instruction(c: &Value) -> Option<(&'static str, bool)> {
    let a = c["actions"].as_array()?;
    if a.iter().any(|x| x == "steer") {
        Some(("割り込む", true))
    } else if c["state"] != "running" && a.iter().any(|x| x == "resume") {
        Some(("指示して再開", false))
    } else {
        None
    }
}
type StopScope<'a> = (&'a str, Option<&'a str>, &'a str);
fn stop_key(scope: StopScope<'_>) -> SharedString {
    format!("stop:{scope:?}").into()
}
fn matches_scope(shown: StopScope<'_>, current: StopScope<'_>) -> bool {
    shown == current
}
struct Transcript {
    task: SharedString,
    lines: Vec<SharedString>,
    truncated: bool,
    more: bool,
}
impl From<&WorkRecord> for Transcript {
    fn from(r: &WorkRecord) -> Self {
        Self {
            task: r
                .task
                .clone()
                .unwrap_or_else(|| "依頼文は未取得です".into())
                .into(),
            lines: r
                .items
                .iter()
                .filter_map(|v| {
                    v["text"]
                        .as_str()
                        .or(v["toolName"].as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_owned().into())
                })
                .collect(),
            truncated: r.truncated,
            more: r.more,
        }
    }
}
#[derive(Default)]
struct Record {
    reader: Option<WorkRecord>,
    shown: Option<Transcript>,
    task: Option<Task<()>>,
    error: Option<Error>,
    busy: bool,
    again: bool,
    observed: Value,
    next_run: Option<String>,
}
impl Record {
    fn load(&mut self, pg: Page, child: Child, cx: &mut Context<Self>) {
        if self.task.is_some() {
            self.again = true;
            return;
        }
        let old = self.reader.take();
        self.error = None;
        self.task = Some(cx.spawn(async move |this, cx| {
            let r = async {
                let mut record = match old {
                    Some(r) => r,
                    None => pg.work_record(child.clone())?,
                };
                let r = record.load().await;
                Ok((record, r))
            }
            .await;
            let _ = this.update(cx, |s, cx| {
                s.task = None;
                match r {
                    Ok((record, r)) => {
                        // Pages fetched before a failing page are valid; show them with the error.
                        s.shown = Some(Transcript::from(&record));
                        s.reader = Some(record);
                        s.error = r.err()
                    }
                    Err(e) => s.error = Some(e),
                }
                if std::mem::take(&mut s.again) {
                    s.load(pg, child, cx)
                }
                cx.notify()
            });
        }));
        cx.notify()
    }
}
impl Desktop {
    pub(super) fn agent_summary(&self, rows: &Rows, ins: &Entity<Inspector>) -> Option<Button> {
        let counts = rows.subagent_counts();
        if counts.is_empty() {
            return None;
        }
        let ins = ins.clone();
        Some(
            Button::new("subagents")
                .small()
                .label(format!(
                    "サブエージェント　{}　›",
                    counts
                        .iter()
                        .map(|(s, n)| format!("{} {n}", state_label(s)))
                        .collect::<Vec<_>>()
                        .join(" · ")
                ))
                .on_click(move |_, _, cx| {
                    ins.update(cx, |s, cx| {
                        s.agents = true;
                        s.run = None;
                        s.child = None;
                        cx.notify()
                    })
                }),
        )
    }
    fn stop_button(
        pg: &Page,
        id: &str,
        child: Option<&str>,
        action: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let scope = (id, child, action);
        // An inline confirmation is unmounted as soon as this exact control disappears.
        // A native OS prompt cannot be withdrawn when the Host changes its scope.
        let pending = w.use_keyed_state(stop_key(scope), cx, |_, _| None::<state::Stop>);
        let (label, warning) = match action {
            "stop_child" => ("この子を停止", "選択した子だけが対象です。"),
            "abort_parent" => (
                "親の Pi ごと停止",
                "この実行と親の Pi のターンも停止します。",
            ),
            "interrupt" => (
                "実行全体を一時停止",
                "この実行のすべての子が一時停止します。",
            ),
            "stop" => ("実行全体を停止", "この実行のすべての子が停止します。"),
            _ => return div(),
        };
        let pg = pg.clone();
        let key = stop_key(scope);
        if pending.read(cx).is_some() {
            let cancel = pending.clone();
            return div()
                .flex()
                .flex_col()
                .gap_1()
                .child(format!("{label}しますか？ {warning}"))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new(SharedString::from(format!("cancel:{key}")))
                                .small()
                                .label("キャンセル")
                                .on_click(move |_, _, cx| {
                                    cancel.update(cx, |s, cx| {
                                        s.take();
                                        cx.notify()
                                    })
                                }),
                        )
                        .child(
                            Button::new(SharedString::from(format!("confirm:{key}")))
                                .danger()
                                .small()
                                .label(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !pg.visible(this) {
                                        return;
                                    }
                                    let Some(stop) = pending.update(cx, |s, cx| {
                                        cx.notify();
                                        s.take()
                                    }) else {
                                        return;
                                    };
                                    let pg = pg.clone();
                                    cx.spawn(async move |this, cx| {
                                        let r = stop.confirm().await;
                                        let _ = this.update(cx, |this, cx| {
                                            if pg.visible(this) {
                                                this.finish_send_notice(r.map(|_| String::new()));
                                                cx.notify()
                                            }
                                        });
                                    })
                                    .detach();
                                })),
                        ),
                );
        }
        let id = id.to_owned();
        let child = child.map(str::to_owned);
        let action = action.to_owned();
        div().child(
            Button::new(key)
                .danger()
                .small()
                .label(format!("{label}…"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !pg.visible(this) {
                        return;
                    }
                    match pg.stop(&id, child.as_deref()) {
                        Ok(stop)
                            if matches_scope((&id, child.as_deref(), &action), stop.scope()) =>
                        {
                            pending.update(cx, |s, cx| {
                                *s = Some(stop);
                                cx.notify()
                            })
                        }
                        _ => {
                            this.notice =
                                "停止の対象が変わりました。現在の状態を確認してください。".into();
                            cx.notify()
                        }
                    }
                })),
        )
    }
    pub(super) fn agent_panel(
        &self,
        key: &EditorKey,
        page: &Page,
        ins: &Entity<Inspector>,
        p: &Colors,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let rows = &self.machines[&key.host].rows;
        let child = ins.read(cx).child.clone();
        let filter = ins.read(cx).run.clone();
        let mut body = div()
            .id("agent-scroll")
            .overflow_y_scroll()
            .flex_1()
            .min_h_0()
            .p_4()
            .flex()
            .flex_col()
            .gap_2();
        let mut title = "サブエージェント".to_owned();
        if let Some(unit) = child.as_ref() {
            let run = rows.get("run", &unit.run);
            if let Some(c) = run["children"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|c| c["id"] == unit.child)
            {
                title = c["title"]
                    .as_str()
                    .or(c["agent"].as_str())
                    .unwrap_or("子の詳細")
                    .to_owned();
                body = body.child(p.caption(format!(
                    "{} · {}",
                    state_label(c["state"].as_str().unwrap_or("")),
                    c["agent"].as_str().unwrap_or("")
                )));
                let rec = w.use_keyed_state(
                    SharedString::from(format!("record:{:?}:{:?}", unit.run, unit.child)),
                    cx,
                    |_, _| Record::default(),
                );
                if c["hasTranscript"] == true && rec.read(cx).observed != *c {
                    rec.update(cx, |r, cx| {
                        r.observed = c.clone();
                        r.load(page.clone(), unit.clone(), cx)
                    })
                }
                if let Some(next) = &rec.read(cx).next_run {
                    if let Some(dest) = self.state.instructions.resumed(page, unit, next).cloned() {
                        ins.update(cx, |s, cx| {
                            s.child = Some(dest);
                            cx.notify()
                        });
                    }
                    body = body.child(p.caption("再開先の作業記録を待っています…"))
                }
                let r = rec.read(cx);
                let mut log = div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .bg(p.surface)
                    .rounded_md()
                    .p_3();
                if let Some(record) = &r.shown {
                    log = log
                        .child(p.caption("親の Pi からの依頼"))
                        .child(record.task.clone())
                        .child(p.caption("子の作業記録"))
                        .children(record.lines.iter().map(|s| div().py_1().child(s.clone())));
                    if record.truncated {
                        log = log.child(p.caption("記録の一部を省略しています"))
                    }
                } else if r.task.is_some() {
                    log = log.child("作業記録を読み込み中…")
                } else if r.error.is_none() {
                    log = log.child("作業記録は提供されていません")
                }
                if let Some(e) = r.error {
                    log = log.child(format!("読み込めません: {e}"))
                }
                if c["hasTranscript"] == true {
                    let rec = rec.clone();
                    let pg = page.clone();
                    let unit = unit.clone();
                    log = log.child(
                        Button::new("refresh-log")
                            .small()
                            .label(if r.shown.as_ref().is_some_and(|r| r.more) {
                                "続きの記録"
                            } else {
                                "記録を更新"
                            })
                            .disabled(r.task.is_some())
                            .on_click(move |_, _, cx| {
                                rec.update(cx, |r, cx| r.load(pg.clone(), unit.clone(), cx))
                            }),
                    )
                }
                body = body.child(log);
                for field in ["question", "error"] {
                    if let Some(text) = c[field].as_str() {
                        body = body.child(text.to_owned())
                    }
                }
                for sent in self.state.instructions.for_child(page, unit) {
                    body = body.child(p.caption(format!("あなた · {}", sent.text)))
                }
                if let Some((label, steer)) = instruction(c) {
                    let disable = r.busy || r.next_run.is_some();
                    let input = w.use_keyed_state(
                        SharedString::from(format!("agent-input:{}:{}", unit.run, unit.child)),
                        cx,
                        |w, cx| InputState::new(w, cx).placeholder("サブエージェントへの指示"),
                    );
                    let text = input.read(cx).value().to_string();
                    let unit = unit.clone();
                    let pg = page.clone();
                    body = body.child(Input::new(&input).disabled(disable)).child(
                        p.primary("instruct-child", cx)
                            .label(label)
                            .disabled(disable || text.trim().is_empty())
                            .on_click(cx.listener(move |this, _, w, cx| {
                                if !pg.visible(this)
                                    || rec.read(cx).busy
                                    || !committed_input_matches(&input, &text, w, cx)
                                {
                                    return;
                                }
                                rec.update(cx, |r, cx| {
                                    r.busy = true;
                                    cx.notify()
                                });
                                let pg = pg.clone();
                                let unit = unit.clone();
                                let text = text.clone();
                                let rec = rec.clone();
                                let input = input.clone();
                                cx.spawn_in(w, async move |this, cx| {
                                    let r = pg
                                        .instruct(
                                            unit.clone(),
                                            if steer {
                                                Instruction::Steer
                                            } else {
                                                Instruction::Resume
                                            },
                                            text.clone(),
                                        )
                                        .await;
                                    let _ = this.update_in(cx, |this, w, cx| {
                                        rec.update(cx, |r, cx| {
                                            r.busy = false;
                                            cx.notify()
                                        });
                                        match r {
                                            Ok(sent) => {
                                                let next = sent.new_run.clone();
                                                this.accept_instruction(sent);
                                                if pg.visible(this) {
                                                    if committed_input_matches(&input, &text, w, cx)
                                                    {
                                                        input.update(cx, |s, cx| {
                                                            s.set_value("", w, cx)
                                                        })
                                                    }
                                                    rec.update(cx, |r, cx| {
                                                        r.next_run = next;
                                                        r.load(pg, unit, cx)
                                                    });
                                                }
                                            }
                                            Err(e) if pg.visible(this) => {
                                                this.notice =
                                                    format!("指示の結果を確認できません: {e}")
                                            }
                                            _ => {}
                                        }
                                        cx.notify()
                                    });
                                })
                                .detach();
                            })),
                    );
                } else {
                    body = body.child(p.caption("この子の作業記録は閲覧のみです"))
                }
                if run["active"] == true
                    && matches!(c["state"].as_str(), Some("waiting" | "running" | "queued"))
                {
                    let a = if c["actions"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|v| v == "stop_child")
                    {
                        Some("stop_child")
                    } else {
                        run["stopAction"].as_str()
                    };
                    if let Some(a) = a {
                        body = body.child(Self::stop_button(
                            page,
                            &unit.run,
                            Some(&unit.child),
                            a,
                            w,
                            cx,
                        ))
                    }
                }
            } else {
                body = body.child("この子は一覧にありません")
            }
        } else {
            for open in [true, false] {
                let mut first = true;
                for (id, r) in rows.group("run") {
                    if filter.as_ref().is_some_and(|v| v != id) || (r["open"] == true) != open {
                        continue;
                    }
                    if first {
                        body = body.child(div().font_weight(FontWeight::BOLD).child(if open {
                            "実行中・再開可能"
                        } else {
                            "以前の実行"
                        }));
                        first = false
                    }
                    let mut card = div()
                        .p_3()
                        .rounded_md()
                        .bg(p.surface)
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(p.caption(format!(
                            "{} · {}",
                            r["backend"].as_str().unwrap_or(""),
                            r["mode"].as_str().unwrap_or("")
                        )));
                    for c in r["children"].as_array().into_iter().flatten() {
                        let Some(child) = c["id"].as_str() else {
                            continue;
                        };
                        let unit = Child {
                            run: id.clone(),
                            child: child.into(),
                        };
                        let ins = ins.clone();
                        card = card.child(
                            Button::new(SharedString::from(format!("unit:{id}:{child}")))
                                .label(format!(
                                    "{}　· {}　›",
                                    c["title"].as_str().or(c["agent"].as_str()).unwrap_or(child),
                                    state_label(c["state"].as_str().unwrap_or(""))
                                ))
                                .on_click(move |_, _, cx| {
                                    ins.update(cx, |s, cx| {
                                        s.child = Some(unit.clone());
                                        cx.notify()
                                    })
                                }),
                        )
                    }
                    if r["active"] == true
                        && let Some(a) = r["stopAction"].as_str()
                    {
                        card = card.child(Self::stop_button(page, id, None, a, w, cx))
                    }
                    body = body.child(card)
                }
            }
        }
        let detail = child.is_some();
        let back = ins.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px_4()
                    .py_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(p.border)
                    .child(title)
                    .child(
                        Button::new("back-agents")
                            .label(if detail {
                                "一覧に戻る"
                            } else {
                                "会話に戻る"
                            })
                            .on_click(move |_, _, cx| {
                                back.update(cx, |s, cx| {
                                    if detail {
                                        s.child = None
                                    } else {
                                        *s = Inspector::default()
                                    }
                                    cx.notify()
                                })
                            }),
                    ),
            )
            .child(body)
    }
}
#[cfg(test)]
mod tests {
    use super::{instruction, matches_scope, stop_key};
    #[test]
    fn child_stop_never_opens_or_keeps_a_parent_confirmation() {
        let shown = ("run", Some("child"), "stop_child");
        assert!(matches_scope(shown, shown));
        for changed in [
            ("run", Some("child"), "abort_parent"),
            ("run", None, "stop"),
            ("run", Some("other"), "stop_child"),
            ("next", Some("child"), "stop_child"),
        ] {
            assert!(!matches_scope(shown, changed));
            assert_ne!(stop_key(shown), stop_key(changed));
        }
    }
    #[gpui::test]
    fn work_log_survives_in_flight_coalesced_and_failed_refreshes(cx: &mut gpui::TestAppContext) {
        use super::{Child, Record, Transcript};
        use gpui::prelude::*;
        let view = cx.new(|_| Record {
            shown: Some(Transcript {
                task: "parent task".into(),
                lines: vec!["page one".into()],
                truncated: false,
                more: true,
            }),
            ..Default::default()
        });
        view.update(cx, |r, cx| {
            let page = crate::state::dummy_page();
            let child = Child {
                run: "r".into(),
                child: "c".into(),
            };
            for _ in 0..2 {
                r.load(page.clone(), child.clone(), cx);
                assert_eq!(r.shown.as_ref().unwrap().lines[0].as_str(), "page one");
            }
            assert!(r.task.is_some() && r.again);
        });
        cx.run_until_parked();
        view.read_with(cx, |r, _| {
            assert!(r.error.is_some() && r.task.is_none());
            assert_eq!(r.shown.as_ref().unwrap().lines[0].as_str(), "page one");
        });
    }
    use serde_json::json;
    #[test]
    fn while_running_only_steer() {
        assert!(
            instruction(&json!({"state":"running","actions":["resume","follow_up"]})).is_none()
        );
        assert_eq!(
            instruction(&json!({"state":"running","actions":["steer","resume"]})),
            Some(("割り込む", true))
        );
    }
}
