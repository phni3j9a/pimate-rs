use super::*;
use std::collections::HashSet;

#[derive(Default)]
struct Conversation {
    documents: markdown::Messages,
    expanded: HashSet<String>,
    // The append boundary from the last time we saw a live answer, even if its row was offscreen.
    stream_tail: Option<(usize, Option<String>)>,
}
impl Conversation {
    fn prepare(
        &mut self,
        entries: &[(String, Value)],
        streaming: bool,
        list: &ListState,
        stream_focused: bool,
    ) {
        self.documents.begin_frame();
        if let Some((old, last)) = &self.stream_tail {
            if *old <= entries.len() && old.checked_sub(1).map(|i| &entries[i].0) == last.as_ref() {
                let added = &entries[*old..];
                let done = added.iter().position(|(_, v)| v["kind"] == "assistant");
                let inserted = done.unwrap_or(added.len());
                if inserted > 0 && list.item_count() == old + 1 {
                    // Interruptions are inserted *before* the streamed row, not after it.
                    list.splice(*old..*old, inserted);
                    if stream_focused && list.logical_scroll_top().item_ix < *old + inserted {
                        list.scroll_to(ListOffset {
                            item_ix: *old + inserted,
                            offset_in_item: px(0.),
                        });
                    }
                }
                if done.is_some() || !streaming {
                    self.documents
                        .finish_stream(done.map(|i| added[i].0.as_str()));
                }
            } else {
                // A replaced history is not an acknowledgement of the old stream.
                self.documents.finish_stream(None);
            }
        }
        if streaming {
            if self.stream_tail.as_ref().is_none_or(|(n, last)| {
                *n != entries.len() || last.as_ref() != entries.last().map(|(id, _)| id)
            }) {
                self.stream_tail = Some((entries.len(), entries.last().map(|(id, _)| id.clone())));
            }
        } else {
            self.stream_tail = None;
        }
        let n = entries.len() + usize::from(streaming);
        let old = list.item_count();
        if old != n {
            list.splice(old.min(n)..old, n.saturating_sub(old));
        }
    }
}

fn tool_result(
    conversation: &Entity<Conversation>,
    p: &Colors,
    id: String,
    v: &Value,
    w: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let text = v["text"].as_str().unwrap_or("");
    let name = v["toolName"].as_str().unwrap_or("ツール");
    let open = conversation.clone();
    let tool = id.clone();
    let expanded = open.read(cx).expanded.contains(&tool);
    let summary = format!(
        "{}　{name} · {}",
        if expanded { "▾" } else { "▸" },
        if v["isError"] == true {
            "失敗"
        } else {
            "完了"
        }
    );
    div()
        .px_4()
        .py_1()
        .child(
            Button::new(SharedString::from(format!("tool:{id}")))
                .ghost()
                .justify_start()
                .small()
                .label(summary)
                .on_click(move |_, _, cx| {
                    open.update(cx, |c, cx| {
                        if !c.expanded.remove(&tool) {
                            c.expanded.insert(tool.clone());
                        }
                        cx.notify()
                    })
                }),
        )
        .when(expanded, |panel| {
            panel.child(
                div().rounded_md().p_2().bg(p.surface).child(
                    conversation.update(cx, |c, cx| c.documents.view(id, text, false, w, cx)),
                ),
            )
        })
        .into_any_element()
}

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
        let streaming = rows
            .get("stream", "text")
            .as_str()
            .is_some_and(|s| !s.is_empty());
        let conversation = w.use_keyed_state(
            SharedString::from(format!("documents:{key:?}")),
            cx,
            |_, _| Conversation::default(),
        );
        let reader = w.use_state(cx, |_, cx| cx.focus_handle()).read(cx).clone();
        let stream_reader = w.use_state(cx, |_, cx| cx.focus_handle()).read(cx).clone();
        let at_end = self.scroll.max_offset_for_scrollbar().height
            + self.scroll.scroll_px_offset_for_scrollbar().y
            < px(2.);
        let focused = stream_reader.contains_focused(w, cx);
        conversation.update(cx, |c, _| {
            c.prepare(entries, streaming, &self.scroll, focused)
        });
        let n = self.scroll.item_count();
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
                let id = row.map_or_else(|| "stream".to_owned(), |(id, _)| format!("entry:{id}"));
                let text = if stream {
                    data.get("stream", "text").as_str()
                } else {
                    v["text"].as_str()
                }
                .unwrap_or("");
                if v["kind"] == "tool_result" {
                    return tool_result(&conversation, &p, id, v, w, cx);
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
                    .when(stream, |d| d.track_focus(&stream_reader))
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
                                conversation
                                    .update(cx, |c, cx| c.documents.view(id, text, stream, w, cx)),
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
    #[gpui::test]
    fn expanded_tool_survives_a_real_unmount(cx: &mut gpui::TestAppContext) {
        struct Tools {
            list: ListState,
            conversation: gpui::Entity<super::Conversation>,
        }
        impl Render for Tools {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let c = self.conversation.clone();
                list(self.list.clone(), move |i, w, cx| {
                    div()
                        .id(("row", i))
                        .debug_selector(|| format!("tool-{i}"))
                        .child(super::tool_result(
                            &c,
                            &super::Colors::new(false),
                            i.to_string(),
                            &serde_json::json!({"toolName":"read","text":"complete output"}),
                            w,
                            cx,
                        ))
                        .into_any_element()
                })
                .size_full()
            }
        }
        cx.update(gpui_component::init);
        let (view, w) = cx.add_window_view(|_, cx| Tools {
            list: ListState::new(300, gpui::ListAlignment::Top, px(0.)),
            conversation: cx.new(|_| Default::default()),
        });
        let bounds = w.debug_bounds("tool-0").unwrap();
        w.simulate_click(
            point(bounds.left() + px(50.), bounds.center().y),
            Default::default(),
        );
        // The production view observes the Conversation through use_keyed_state.
        view.update(w, |_, cx| cx.notify());
        w.run_until_parked();
        let expanded = w.debug_bounds("tool-0").unwrap().size.height;
        assert!(expanded > bounds.size.height);
        let list = view.read_with(w, |v, _| v.list.clone());
        list.scroll_to(ListOffset {
            item_ix: 200,
            offset_in_item: px(0.),
        });
        view.update(w, |_, cx| cx.notify());
        w.run_until_parked();
        assert_eq!(list.logical_scroll_top().item_ix, 200);
        assert!(list.bounds_for_item(0).is_none());
        list.scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
        view.update(w, |_, cx| cx.notify());
        w.run_until_parked();
        assert_eq!(w.debug_bounds("tool-0").unwrap().size.height, expanded);
    }
}
