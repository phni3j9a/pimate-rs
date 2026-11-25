use super::*;

impl Desktop {
    pub(super) fn conversation(
        &self,
        key: &EditorKey,
        p: &Colors,
        dark: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = &self.machines[&key.host].rows;
        let entries = rows.group("entry");
        let n = entries.len()
            + usize::from(
                rows.get("stream", "text")
                    .as_str()
                    .is_some_and(|s| !s.is_empty()),
            );
        // Top-align short answers; follow growing content only while the reader is at the end.
        let reader = w.use_state(cx, |_, cx| cx.focus_handle()).read(cx).clone();
        let at_end = self.scroll.max_offset_for_scrollbar().height
            + self.scroll.scroll_px_offset_for_scrollbar().y
            < px(2.);
        let old = self.scroll.item_count();
        if old != n {
            self.scroll.splice(old.min(n)..old, n.saturating_sub(old));
        }
        if at_end && !reader.contains_focused(w, cx) {
            self.scroll.scroll_to(ListOffset {
                item_ix: n,
                offset_in_item: px(0.),
            });
        }
        if n == 0 {
            return div()
                .flex_1()
                .p_4()
                .child(p.caption("まだメッセージはありません"))
                .into_any_element();
        }
        let desktop = cx.entity();
        let k = key.clone();
        let messages = w.use_keyed_state(
            SharedString::from(format!("documents:{k:?}")),
            cx,
            |_, _| markdown::Messages::default(),
        );
        let list = list(self.scroll.clone(), move |i, w, cx| {
            desktop.update(cx, |this, cx| {
                let data = &this.machines[&k.host].rows;
                let entries = data.group("entry");
                let p = Colors::new(dark);
                let row = entries.get(i);
                let v = row.map(|(_, v)| v).unwrap_or(&Value::Null);
                let stream = row.is_none();
                let role = match v["kind"].as_str() {
                    Some("user") => "あなた",
                    Some("assistant") => "Pi",
                    Some("tool_result") => "ツールの結果",
                    _ if stream => "Pi · 応答中",
                    _ => "作業",
                };
                // A completed reply replaces its streamed slot without remounting the reader.
                let id = if stream || v["kind"] == "assistant" {
                    format!(
                        "reply-after:{:?}",
                        i.checked_sub(1)
                            .and_then(|n| entries.get(n))
                            .map(|(id, _)| id)
                    )
                } else {
                    format!("entry:{}", row.unwrap().0)
                };
                let text = if stream {
                    data.get("stream", "text").as_str()
                } else {
                    v["text"].as_str()
                }
                .unwrap_or("");
                if v["kind"] == "tool_result" {
                    let name = v["toolName"].as_str().unwrap_or("ツール");
                    let open = w.use_keyed_state(
                        SharedString::from(format!("{k:?}/detail/{id}")),
                        cx,
                        |_, _| false,
                    );
                    let expanded = *open.read(cx);
                    let summary = format!(
                        "{}　{name} · {}",
                        if expanded { "▾" } else { "▸" },
                        if v["isError"] == true {
                            "失敗"
                        } else {
                            "完了"
                        }
                    );
                    return div()
                        .px_4()
                        .py_1()
                        .child(
                            Button::new(SharedString::from(format!("tool:{id}")))
                                .ghost()
                                .justify_start()
                                .small()
                                .label(summary)
                                .on_click(move |_, _, cx| {
                                    open.update(cx, |show, cx| {
                                        *show = !*show;
                                        cx.notify()
                                    })
                                }),
                        )
                        .when(expanded, |panel| {
                            panel.child(div().rounded_md().p_2().bg(p.surface).child(
                                messages.update(cx, |cache, cx| cache.view(id, text, false, w, cx)),
                            ))
                        })
                        .into_any_element();
                }
                if text.is_empty()
                    && let Some(calls) = v["toolCalls"].as_array().filter(|cs| !cs.is_empty())
                {
                    return div()
                        .px_4()
                        .pt_2()
                        .child(p.caption(format!("Pi · {} 件のツールを使用", calls.len())))
                        .into_any_element();
                }
                let images = v["imageCount"]
                    .as_u64()
                    .unwrap_or_else(|| v["images"].as_array().map_or(0, |a| a.len() as u64));
                let user = role == "あなた";
                div()
                    .w_full()
                    .px_4()
                    .py_2()
                    .flex()
                    .flex_col()
                    .when(user, |v| v.items_end())
                    .child(p.caption(role).mb_1().font_weight(FontWeight::SEMIBOLD))
                    .child(
                        div()
                            .when(user, |v| {
                                v.max_w(relative(0.9))
                                    .rounded_lg()
                                    .bg(p.bubble)
                                    .px_3()
                                    .py_2()
                            })
                            .child(
                                messages
                                    .update(cx, |cache, cx| cache.view(id, text, stream, w, cx)),
                            ),
                    )
                    .when(images > 0, |v| {
                        v.child(p.caption(format!("画像 {images} 枚（プレビュー未対応）")))
                    })
                    .into_any_element()
            })
        })
        .size_full();
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .track_focus(&reader)
            .child(list)
            .vertical_scrollbar(&self.scroll)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::transcript;
    use gpui::{Context, ListOffset, ListState, Window, div, list, point, prelude::*, px, size};
    #[gpui::test]
    fn ten_thousand_history_rows_only_mount_near_the_viewport(cx: &mut gpui::TestAppContext) {
        use std::{cell::RefCell, rc::Rc};
        struct Probe(ListState, Rc<RefCell<Vec<usize>>>);
        impl Render for Probe {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let mounted = self.1.clone();
                list(self.0.clone(), move |i, _, _| {
                    mounted.borrow_mut().push(i);
                    div().h(px(40.)).into_any_element()
                })
                .size_full()
            }
        }
        let state = transcript();
        state.splice(0..0, 10_000);
        let mounted = Rc::new(RefCell::new(Vec::new()));
        let window = cx.add_empty_window();
        for ix in [10_000, 615] {
            state.scroll_to(ListOffset {
                item_ix: ix,
                offset_in_item: px(0.),
            });
            mounted.borrow_mut().clear();
            window.draw(point(px(0.), px(0.)), size(px(600.), px(480.)), |_, cx| {
                cx.new(|_| Probe(state.clone(), mounted.clone()))
            });
            let indices = mounted.borrow();
            assert!(!indices.is_empty());
            assert!(
                indices.len() < 40 && indices.iter().all(|i| i.abs_diff(ix) < 20),
                "unexpected layout: {indices:?}"
            );
        }
    }
}
