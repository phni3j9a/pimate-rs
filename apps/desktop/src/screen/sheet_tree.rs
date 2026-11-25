use super::sheet_chat::action;
use super::*;
use state::{Page, SheetAction as A};
fn depths(nodes: &[Value]) -> HashMap<&str, usize> {
    let mut n = HashMap::new();
    let mut d = HashMap::new();
    for v in nodes {
        if let Some(p) = v["parentId"].as_str() {
            *n.entry(p).or_insert(0) += 1;
        }
    }
    for v in nodes {
        if let Some(id) = v["id"].as_str() {
            let parent = v["parentId"].as_str().unwrap_or("");
            let depth = d.get(parent).copied().unwrap_or(0)
                + usize::from(n.get(parent).copied().unwrap_or(0) > 1);
            d.insert(id, depth);
        }
    }
    d
}
impl Desktop {
    pub(super) fn tree(
        &self,
        key: &EditorKey,
        pg: &Page,
        p: &Colors,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sheet = self.active_sheet().unwrap();
        let selected = w.use_keyed_state("tree-selection", cx, |_, _| None::<String>);
        let rows = &self.machines[&key.host].rows;
        let busy = sheet.loading() || !rows.group("busy").is_empty();
        let idle = rows.get("view", "status") == "idle" && rows.get("view", "agentActive") != true;
        let mut card = div()
            .p_4()
            .bg(p.surface)
            .rounded_md()
            .flex()
            .flex_col()
            .gap_2();
        if let Some(id) = selected.read(cx).clone() {
            let v = rows.get("node", &id);
            let back = selected.clone();
            card = card
                .child(
                    Button::new("back-tree")
                        .small()
                        .label("ツリーに戻る")
                        .on_click(move |_, _, cx| {
                            back.update(cx, |s, cx| {
                                *s = None;
                                cx.notify()
                            })
                        }),
                )
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("移動先を確認"),
                )
                .child(v["preview"].as_str().unwrap_or("本文なし").to_owned());
            let label = w.use_keyed_state("node-label", cx, |w, cx| {
                InputState::new(w, cx)
                    .placeholder("ラベル")
                    .default_value(v["label"].as_str().unwrap_or("").to_owned())
            });
            let text = label.read(cx).value().to_string();
            let trimmed = text.trim().to_owned();
            let entry = id.clone();
            let input = label.clone();
            let page = pg.clone();
            card = card.child(Input::new(&label).disabled(busy)).child(
                Button::new("save-label")
                    .label("ラベルを保存")
                    .disabled(
                        busy || trimmed.is_empty()
                            || trimmed.encode_utf16().count() > 200
                            || v["label"] == trimmed,
                    )
                    .on_click(cx.listener(move |this, _, w, cx| {
                        if page.visible(this) && committed_input_matches(&input, &text, w, cx) {
                            this.sheet_action(
                                A::Label(pimate_protocol::SetLabelParams {
                                    entry_id: entry.clone(),
                                    label: Some(trimmed.clone()),
                                }),
                                cx,
                            )
                        }
                    })),
            );
            if v["label"].is_string() {
                let entry = id.clone();
                card = card.child(action(
                    "clear-label",
                    "ラベルを外す",
                    busy,
                    pg,
                    move || {
                        A::Label(pimate_protocol::SetLabelParams {
                            entry_id: entry.clone(),
                            label: None,
                        })
                    },
                    cx,
                ))
            }
            let summarize = w.use_keyed_state("summarize", cx, |_, _| false);
            let on = *summarize.read(cx);
            card = card
                .child(
                    Button::new("summarize")
                        .label(if on {
                            "✓ 離れる枝を要約する"
                        } else {
                            "○ 離れる枝を要約する"
                        })
                        .on_click(move |_, _, cx| {
                            summarize.update(cx, |s, cx| {
                                *s = !*s;
                                cx.notify()
                            })
                        }),
                )
                .child(p.caption("要約には時間と利用料金がかかります。元の枝は残ります。"))
                .child(action(
                    "move-node",
                    "ここへ移動",
                    busy || !idle || v.is_null() || v["isCurrent"] == true,
                    pg,
                    move || A::Move(id.clone(), on),
                    cx,
                ));
        } else {
            let raw = sheet.reply.as_ref().and_then(|r| r["nodes"].as_array());
            let list = raw
                .cloned()
                .unwrap_or_else(|| rows.group("node").iter().map(|(_, v)| v.clone()).collect());
            let depth = depths(&list);
            card = card.child(p.caption("移動先のメッセージを選択"));
            if sheet.reply.as_ref().is_some_and(|r| r["truncated"] == true) {
                card = card.child("大きなツリーのため一部を省略しています")
            }
            if list.is_empty() && !busy && sheet.error.is_none() {
                card = card.child("まだ会話がありません")
            }
            for v in &list {
                let Some(id) = v["id"].as_str() else { continue };
                let d = depth.get(id).copied().unwrap_or(0);
                let id = id.to_owned();
                let sel = selected.clone();
                card = card.child(
                    div()
                        .ml(rems(d.min(6) as f32))
                        .border_l_2()
                        .border_color(if v["onBranch"] == true {
                            p.accent
                        } else {
                            p.border
                        })
                        .pl_2()
                        .child(p.caption(format!(
                            "{} · {}",
                            if v["isCurrent"] == true {
                                "現在位置"
                            } else if v["onBranch"] == true {
                                "現在の枝"
                            } else {
                                "別の枝"
                            },
                            v["kind"].as_str().unwrap_or("")
                        )))
                        .child(
                            Button::new(SharedString::from(format!("node:{id}")))
                                .small()
                                .label(super::sheet_chat::preview(
                                    v["label"]
                                        .as_str()
                                        .or(v["preview"].as_str())
                                        .unwrap_or("本文なし"),
                                ))
                                .on_click(move |_, _, cx| {
                                    sel.update(cx, |s, cx| {
                                        *s = Some(id.clone());
                                        cx.notify()
                                    })
                                }),
                        ),
                );
            }
        }
        card.into_any_element()
    }
}
#[cfg(test)]
mod tests {
    use super::depths;
    use serde_json::json;
    #[test]
    fn indent_only_at_forks() {
        let nodes = vec![
            json!({"id":"a"}),
            json!({"id":"b","parentId":"a"}),
            json!({"id":"c","parentId":"b"}),
            json!({"id":"d","parentId":"a"}),
        ];
        let d = depths(&nodes);
        assert_eq!((d["a"], d["b"], d["c"], d["d"]), (0, 1, 1, 1));
    }
}
