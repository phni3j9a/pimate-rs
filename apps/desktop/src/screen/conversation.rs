use super::*;
use std::collections::{HashMap, HashSet};

fn tool_subject(name: &str, args: &Value, details: &Value) -> String {
    let subject = match name {
        "bash" => args["command"].as_str(),
        "edit" | "write" | "read" => args["path"].as_str().or(args["file_path"].as_str()),
        _ => None,
    }
    .unwrap_or("")
    .lines()
    .next()
    .unwrap_or("")
    .chars()
    .take(100)
    .collect::<String>();
    let patch = if name == "edit" {
        &details["patch"]
    } else if name == "write" {
        &details["pimate"]["patch"]
    } else {
        &Value::Null
    };
    if let Some(patch) = patch.as_str() {
        let patch = state::review::Patch::parse(patch);
        if !patch.hunks.is_empty() {
            return format!("{subject} +{} −{}", patch.added, patch.removed);
        }
    }
    subject
}
fn work_summary(start: Option<u64>, end: Option<u64>, calls: usize) -> String {
    let secs = start
        .zip(end)
        .map_or(0, |(a, b)| b.saturating_sub(a) / 1000);
    let mut label = if secs > 0 {
        format!("{secs}秒 作業")
    } else {
        "作業".into()
    };
    if calls > 0 {
        label.push_str(&format!(" · ツール {calls} 回"));
    }
    label
}
fn should_follow(changed: bool, at_end: bool, manual: bool, focused: bool) -> bool {
    changed && at_end && !manual && !focused
}
fn thought_label(active: bool, ms: Option<u64>) -> String {
    if active {
        "Thinking…".into()
    } else if let Some(ms) = ms.filter(|ms| *ms >= 1000) {
        format!("Thought for {}s", ms.saturating_add(500) / 1000)
    } else {
        "Thought".into()
    }
}

// Keep the virtual list: opening a turn mounts only the work rows in the viewport.
#[derive(Default)]
struct LayoutRows {
    indices: Vec<usize>,
    summaries: HashMap<usize, String>,
    results: HashSet<String>,
    folded_answers: HashSet<usize>,
    calls: HashMap<String, (usize, usize, usize)>,
}
impl LayoutRows {
    fn new(entries: &[(String, Value)], running: bool, expanded: &HashSet<String>) -> Self {
        let mut layout = Self::default();
        let mut begin = 0;
        while begin < entries.len() {
            let end = begin
                + 1
                + entries[begin + 1..]
                    .iter()
                    .take_while(|(_, v)| v["kind"] != "user")
                    .count();
            let mut calls = 0;
            let mut work = false;
            for (ix, (_, v)) in entries[begin..end].iter().enumerate() {
                for (j, c) in v["toolCalls"].as_array().into_iter().flatten().enumerate() {
                    calls += 1;
                    if let Some(id) = c["id"].as_str() {
                        layout.calls.insert(id.into(), (begin + ix, j, begin));
                    }
                }
                if let Some(id) = v["toolCallId"].as_str() {
                    layout.results.insert(id.into());
                }
                work |= v["kind"] == "tool_result"
                    || v["thinking"].as_str().is_some_and(|t| !t.is_empty());
            }
            let (id, first) = &entries[begin];
            let summary =
                first["kind"] == "user" && (work || calls > 0) && (end < entries.len() || !running);
            if summary {
                layout.summaries.insert(
                    begin,
                    work_summary(
                        first["timestamp"].as_u64(),
                        entries[end - 1].1["timestamp"].as_u64(),
                        calls,
                    ),
                );
            }
            let show = !summary || expanded.contains(id);
            for (offset, (_, v)) in entries[begin..end].iter().enumerate() {
                let idx = begin + offset;
                if !show {
                    if v["kind"] == "tool_result" {
                        continue;
                    }
                    if v["kind"] == "assistant" {
                        if v["text"].as_str().unwrap_or("").is_empty()
                            && v["errorMessage"].is_null()
                            && v["stopReason"] != "aborted"
                        {
                            continue;
                        }
                        layout.folded_answers.insert(idx);
                    }
                }
                layout.indices.push(idx);
            }
            begin = end;
        }
        layout
    }
}
#[derive(Default)]
struct Conversation {
    documents: markdown::Messages,
    expanded: HashSet<String>,
    turns: HashSet<String>,
    thought: HashMap<String, bool>,
    manual: bool,
    last_content: Option<(u64, usize, usize, usize)>,
    layout: LayoutRows,
    layout_version: Option<(u64, bool)>,
    // The append boundary from the last time we saw a live answer, even if its row was offscreen.
    // Increment at completion/replacement, not on deltas or an intervening user row.
    stream_generation: u64,
    stream_tail: Option<(usize, Option<String>)>,
}
impl Conversation {
    fn prepare(
        &mut self,
        entries: &[(String, Value)],
        streaming: bool,
        list: &ListState,
        running: bool,
        revision: u64,
        live_count: usize,
    ) {
        self.documents.begin_frame();
        let mut committed = None;
        if let Some((old, last)) = &self.stream_tail {
            if *old <= entries.len() && old.checked_sub(1).map(|i| &entries[i].0) == last.as_ref() {
                let added = &entries[*old..];
                let done = added.iter().position(|(_, v)| v["kind"] == "assistant");
                committed = done.map(|i| old + i);
                if done.is_some() || !streaming {
                    if let Some(i) = done
                        && let Some(open) = self
                            .thought
                            .remove(&format!("stream:{}", self.stream_generation))
                    {
                        self.thought.insert(format!("entry:{}", added[i].0), open);
                    }
                    self.stream_generation += 1;
                    self.documents
                        .finish_stream(done.map(|i| added[i].0.as_str()));
                }
            } else {
                // A replaced history is not an acknowledgement of the old stream.
                self.stream_generation += 1;
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
        if self.layout_version != Some((revision, running)) {
            self.layout_version = Some((revision, running));
            let next = LayoutRows::new(entries, running, &self.turns);
            let prefix = self
                .layout
                .indices
                .iter()
                .zip(&next.indices)
                .take_while(|(a, b)| a == b)
                .count();
            let suffix = self.layout.indices[prefix..]
                .iter()
                .rev()
                .zip(next.indices[prefix..].iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            let history = self.layout.indices.len();
            if prefix == history
                && list.item_count() == history + 1
                && let Some(slot) = committed
                    .and_then(|ix| next.indices.binary_search(&ix).ok())
                    .filter(|slot| *slot >= prefix)
            {
                // Reuse the measured stream slot for message_end, inserting a steer *before* it.
                list.splice(prefix..prefix, slot - prefix);
                list.splice(slot + 1..slot + 1, next.indices.len() - slot - 1);
            } else if list.item_count() >= history
                && (prefix + suffix != history || prefix + suffix != next.indices.len())
            {
                list.splice(
                    prefix..history - suffix,
                    next.indices.len() - prefix - suffix,
                );
            }
            self.layout = next;
        }
        let n = self.layout.indices.len() + live_count + usize::from(streaming);
        let old = list.item_count();
        if old != n {
            list.splice(old.min(n)..old, n.saturating_sub(old));
        }
    }
}

struct ToolLine<'a> {
    name: &'a str,
    text: &'a str,
    state: &'static str,
    subject: String,
    duration: Option<u64>,
}
fn tool_result(
    conversation: &Entity<Conversation>,
    p: &Colors,
    id: String,
    line: ToolLine<'_>,
    review: Option<(Entity<super::Inspector>, state::Page, Option<String>)>,
    w: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let ToolLine {
        name,
        text,
        state,
        subject,
        duration,
    } = line;
    let open = conversation.clone();
    let tool = id.clone();
    let expanded = open.read(cx).expanded.contains(&tool);
    let summary = format!(
        "{}　{name} {subject} · {} · {}行{}",
        if expanded { "▾" } else { "▸" },
        state,
        text.lines().count(),
        duration
            .map(|ms| format!(" · {:.1}秒", ms as f64 / 1000.))
            .unwrap_or_default()
    );
    let hover_review = review.clone();
    let copied = text.to_owned();
    let panel = div()
        .group("tool")
        .relative()
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
                        c.manual = true;
                        if !c.expanded.remove(&tool) {
                            c.expanded.insert(tool.clone());
                        }
                        cx.notify()
                    })
                }),
        )
        .child(
            div()
                .invisible()
                .absolute()
                .right_1()
                .top_0()
                .bg(p.bg)
                .group_hover("tool", |s| s.visible())
                .flex()
                .gap_1()
                .child(
                    Button::new(SharedString::from(format!("copy:{id}")))
                        .ghost()
                        .small()
                        .label("コピー")
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
                        }),
                )
                .when_some(hover_review, |d, (ins, page, turn)| {
                    d.child(
                        Button::new(SharedString::from(format!("diff:{id}")))
                            .ghost()
                            .small()
                            .label("差分を開く")
                            .on_click(move |_, _, cx| {
                                ins.update(cx, |s, cx| {
                                    s.load(page.clone(), turn.clone(), false, cx)
                                })
                            }),
                    )
                }),
        )
        .when(expanded, |panel| {
            panel.child(div().rounded_md().p_2().bg(p.surface).child(
                conversation.update(cx, |c, cx| c.documents.view(id, text, false, None, w, cx)),
            ))
        });
    let output = text.to_owned();
    context::on_right_click(panel, move |m| {
        m.item(context::copy("出力をコピー", output.clone()))
            .when_some(review.clone(), |m, (ins, page, turn)| {
                m.item(
                    gpui_component::menu::PopupMenuItem::new("変更をレビュー").on_click(
                        move |_, _, cx| {
                            ins.update(cx, |s, cx| s.load(page.clone(), turn.clone(), false, cx))
                        },
                    ),
                )
            })
    })
    .into_any_element()
}

impl Desktop {
    pub(super) fn conversation(
        &self,
        key: &EditorKey,
        p: &Colors,
        media: super::chat_media::Media,
        inspector: Entity<super::chat_review::Inspector>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = &self.machines[&key.host].rows;
        let entries = rows.group("entry");
        let streaming = ["text", "thinking"].iter().any(|k| {
            rows.get("stream", k)
                .as_str()
                .is_some_and(|s| !s.is_empty())
        });
        let conversation = w.use_keyed_state(
            SharedString::from(format!("documents:{key:?}")),
            cx,
            |_, _| Conversation::default(),
        );
        let reader = w.use_state(cx, |_, cx| cx.focus_handle()).read(cx).clone();
        let stream_reader = w.use_state(cx, |_, cx| cx.focus_handle()).read(cx).clone();
        let old_count = self.scroll.item_count();
        let at_end = old_count == 0
            || self
                .scroll
                .bounds_for_item(old_count - 1)
                .is_some_and(|last| {
                    last.bottom() <= self.scroll.viewport_bounds().bottom() + px(2.)
                });
        let live: Vec<usize> = rows
            .group("tool")
            .iter()
            .enumerate()
            .filter_map(|(i, (_, v))| {
                (v["status"] == "running" && v["parentToolCallId"].is_null()).then_some(i)
            })
            .collect();
        let content = (
            rows.entry_revision,
            rows.get("stream", "text").as_str().map_or(0, str::len),
            rows.get("stream", "thinking").as_str().map_or(0, str::len),
            live.len(),
        );
        let new_content = old_count == 0 || conversation.read(cx).last_content != Some(content);
        let stream_focused = stream_reader.contains_focused(w, cx);
        let scroll_top = self.scroll.logical_scroll_top();
        let before = conversation.read(cx).stream_tail.as_ref().map(|p| p.0);
        let running = rows.get("view", "status") == "running" || streaming || !live.is_empty();
        let manual = conversation.read(cx).manual;
        conversation.update(cx, |c, _| {
            c.manual = false;
            c.last_content = Some(content);
            c.prepare(
                entries,
                streaming,
                &self.scroll,
                running,
                rows.entry_revision,
                live.len(),
            )
        });
        let stream_answer = format!(
            "stream:{}:{}",
            conversation.entity_id().as_u64(),
            conversation.read(cx).stream_generation
        );
        let n = self.scroll.item_count();
        if stream_focused
            && let Some(before) = before
            && before < entries.len()
        {
            // message_end replaces the stream; an intervening steer inserts above it.
            // A splice must not remount the selected heading at the bottom of a tall answer.
            let committed = entries[before..]
                .iter()
                .position(|(_, v)| v["kind"] == "assistant");
            let row = committed
                .and_then(|i| {
                    conversation
                        .read(cx)
                        .layout
                        .indices
                        .binary_search(&(before + i))
                        .ok()
                })
                .unwrap_or(n.saturating_sub(1));
            self.scroll.scroll_to(ListOffset {
                item_ix: row,
                offset_in_item: if scroll_top.item_ix == old_count.saturating_sub(1) {
                    scroll_top.offset_in_item
                } else {
                    px(0.)
                },
            });
        } else if should_follow(new_content, at_end, manual, reader.contains_focused(w, cx)) {
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
        let latest = self.scroll.clone();
        let p = *p;
        let desktop = cx.entity();
        let k = key.clone();
        let list = list(self.scroll.clone(), move |i, w, cx| {
            let item = desktop.update(cx, |this, cx| {
                let data = &this.machines[&k.host].rows;
                let entries = data.group("entry");
                let history_len = conversation.read(cx).layout.indices.len();
                if let Some(ix) = i.checked_sub(history_len).and_then(|j| live.get(j))
                    && let Some((id, v)) = data.group("tool").get(*ix)
                {
                    let text = v["partial"]["content"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|b| b["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let name = v["toolName"].as_str().unwrap_or("ツール");
                    return tool_result(
                        &conversation,
                        &p,
                        format!("live:{id}"),
                        ToolLine {
                            name,
                            text: &text,
                            state: "実行中",
                            subject: tool_subject(name, &v["args"], &Value::Null),
                            duration: v["startedAt"].as_u64().map(|t| {
                                (chrono::Utc::now().timestamp_millis() as u64).saturating_sub(t)
                            }),
                        },
                        None,
                        w,
                        cx,
                    );
                }
                let index = conversation
                    .read(cx)
                    .layout
                    .indices
                    .get(i)
                    .copied()
                    .unwrap_or(entries.len());
                let row = entries.get(index);
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
                let images = state::images::history_count(v);

                let open_run = |call: &str| -> Option<Button> {
                    let run = data.original_run(call)?.to_owned();
                    let ins = inspector.clone();
                    Some(
                        Button::new(SharedString::from(format!("original:{id}:{call}")))
                            .small()
                            .label("サブエージェントの実行を開く")
                            .on_click(move |_, _, cx| {
                                ins.update(cx, |s, cx| {
                                    s.agents = true;
                                    s.child = None;
                                    s.run = Some(run.clone());
                                    cx.notify()
                                })
                            }),
                    )
                };
                if v["kind"] == "tool_result" {
                    let cid = v["toolCallId"].as_str().unwrap_or("");
                    let call = conversation.read(cx).layout.calls.get(cid).copied();
                    let live = data.get("tool", cid);
                    let args =
                        call.map_or(
                            &live["args"],
                            |(ix, j, _)| &entries[ix].1["toolCalls"][j]["args"],
                        );
                    let subject =
                        tool_subject(v["toolName"].as_str().unwrap_or(""), args, &v["details"]);
                    let run = open_run(cid);
                    return div()
                        .child(tool_result(
                            &conversation,
                            &p,
                            id,
                            ToolLine {
                                name: v["toolName"].as_str().unwrap_or("ツール"),
                                text,
                                state: if v["isError"] == true {
                                    "失敗"
                                } else {
                                    "完了"
                                },
                                subject,
                                duration: live["durationMs"].as_u64(),
                            },
                            matches!(v["toolName"].as_str(), Some("edit" | "write")).then(|| {
                                (
                                    inspector.clone(),
                                    media.page.clone(),
                                    call.map(|(_, _, start)| entries[start].0.clone()),
                                )
                            }),
                            w,
                            cx,
                        ))
                        .children(run)
                        .when(images > 0, |b| {
                            b.child(media.history(
                                &row.unwrap().0,
                                v,
                                data.get("view", "entryImages") == true,
                                w,
                                cx,
                            ))
                        })
                        .into_any_element();
                }
                let show_work = !conversation.read(cx).layout.folded_answers.contains(&index);
                let thinking = if !show_work {
                    &Value::Null
                } else if stream {
                    data.get("stream", "thinking")
                } else {
                    &v["thinking"]
                };
                let thought = thinking.as_str().filter(|s| !s.is_empty()).map(|t| {
                    let active =
                        stream && text.is_empty() && data.get("stream", "thinkingUntil").is_null();
                    let ms = if stream {
                        data.get("stream", "thinkingUntil")
                            .as_u64()
                            .zip(data.get("stream", "thinkingSince").as_u64())
                            .map(|(a, b)| a.saturating_sub(b))
                    } else {
                        data.get("thinkingMs", &row.unwrap().0).as_u64()
                    };
                    let tid = if stream {
                        format!("stream:{}", conversation.read(cx).stream_generation)
                    } else {
                        id.clone()
                    };
                    let open = conversation
                        .read(cx)
                        .thought
                        .get(&tid)
                        .copied()
                        .unwrap_or(active);
                    let c = conversation.clone();
                    div()
                        .child(
                            Button::new(SharedString::from(format!("thought:{id}")))
                                .ghost()
                                .small()
                                .label(format!(
                                    "{} {}",
                                    if open { "▾" } else { "▸" },
                                    thought_label(active, ms)
                                ))
                                .on_click(move |_, _, cx| {
                                    c.update(cx, |s, cx| {
                                        s.manual = true;
                                        s.thought.insert(tid.clone(), !open);
                                        cx.notify()
                                    })
                                }),
                        )
                        .when(open, |d| d.child(p.caption(t.to_owned())))
                });
                let summary = conversation.read(cx).layout.summaries.get(&index).map(|s| {
                    let uid = entries[index].0.clone();
                    let open = conversation.read(cx).turns.contains(&uid);
                    let c = conversation.clone();
                    Button::new(SharedString::from(format!("turn:{id}")))
                        .ghost()
                        .small()
                        .label(format!("{} {s}", if open { "▾" } else { "▸" }))
                        .on_click(move |_, _, cx| {
                            c.update(cx, |s, cx| {
                                s.manual = true;
                                s.layout_version = None;
                                if !s.turns.remove(&uid) {
                                    s.turns.insert(uid.clone());
                                }
                                cx.notify()
                            })
                        })
                });
                let user = role == "あなた";
                let copy = text.to_owned();
                let message = div()
                    .relative()
                    .group("message")
                    .debug_selector(|| format!("message-{id}"))
                    .w_full()
                    .when(stream, |d| d.track_focus(&stream_reader))
                    .px_4()
                    .py_2()
                    .flex()
                    .flex_col()
                    .when(user, |v| v.items_end())
                    .children(thought)
                    .when(!text.is_empty() || images > 0, |d| {
                        d.child(p.caption(role).mb_1().font_weight(FontWeight::SEMIBOLD))
                    })
                    .child(
                        div()
                            .when(user, |v| {
                                v.max_w(relative(0.9))
                                    .rounded_lg()
                                    .bg(p.bubble)
                                    .px_3()
                                    .py_2()
                            })
                            .child(conversation.update(cx, |c, cx| {
                                let answer = if stream { &stream_answer } else { &id };
                                c.documents.view(
                                    id.clone(),
                                    text,
                                    stream,
                                    Some((&media, answer)),
                                    w,
                                    cx,
                                )
                            })),
                    )
                    .when(!text.is_empty(), |d| {
                        d.child(
                            div()
                                .absolute()
                                .right_1()
                                .top_0()
                                .invisible()
                                .group_hover("message", |s| s.visible())
                                .bg(p.bg)
                                .child(
                                    Button::new(SharedString::from(format!("copy-message:{id}")))
                                        .ghost()
                                        .small()
                                        .label("コピー")
                                        .on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                copy.clone(),
                                            ))
                                        }),
                                ),
                        )
                    })
                    .children(summary.map(|button| div().w_full().child(button)))
                    .children(
                        v["toolCalls"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|c| {
                                show_work
                                    && data.get("tool", c["id"].as_str().unwrap_or(""))["status"]
                                        != "running"
                                    && !conversation
                                        .read(cx)
                                        .layout
                                        .results
                                        .contains(c["id"].as_str().unwrap_or(""))
                            })
                            .map(|c| {
                                div()
                                    .child(p.caption(format!(
                                        "{} {}",
                                        c["name"].as_str().unwrap_or("ツール"),
                                        tool_subject(
                                            c["name"].as_str().unwrap_or(""),
                                            &c["args"],
                                            &Value::Null
                                        )
                                    )))
                                    .children(c["id"].as_str().and_then(open_run))
                            }),
                    )
                    .when(v["stopReason"] == "aborted", |d| {
                        d.child(p.caption(
                            "中断しました。変更は差分で、送信待ちは履歴で確認してください。",
                        ))
                    })
                    .children(v["errorMessage"].as_str().map(|e| p.caption(e.to_owned())))
                    .when(v["kind"] == "assistant", |d| {
                        let mut meta = Vec::new();
                        if let Some(m) = v["model"]["id"].as_str() {
                            meta.push(m.to_owned())
                        }
                        if v["usage"].is_object() {
                            let tokens = ["input", "output", "cacheRead", "cacheWrite"]
                                .iter()
                                .filter_map(|k| v["usage"][k].as_f64())
                                .sum::<f64>();
                            meta.push(format!("{tokens:.0} トークン"))
                        }
                        d.when(!meta.is_empty(), |d| d.child(p.caption(meta.join(" · "))))
                    })
                    .when(images > 0, |v| {
                        v.child(media.history(
                            &row.unwrap().0,
                            &row.unwrap().1,
                            data.get("view", "entryImages") == true,
                            w,
                            cx,
                        ))
                    });
                let raw = text.to_owned();
                let pg = media.page.clone();
                let app = cx.weak_entity();
                let fork = row
                    .filter(|_| user && data.get("view", "status") == "idle")
                    .map(|(id, _)| id.clone());
                context::on_right_click(message, move |m| {
                    let owner = app.clone();
                    let source = pg.clone();
                    let pg = pg.clone();
                    let app = app.clone();
                    let quoted = raw.clone();
                    m.item(context::copy("メッセージ全文をコピー", raw.clone()))
                        .item(
                            gpui_component::menu::PopupMenuItem::new("引用して返信").on_click(
                                move |_, w, cx| {
                                    let _ = app.update(cx, |app, cx| {
                                        if pg.visible(app)
                                            && !app.composing
                                            && let Some((_, input, _)) = &app.input
                                        {
                                            input.update(cx, |s, cx| {
                                                s.insert(
                                                    format!(
                                                        "{}\n",
                                                        quoted
                                                            .lines()
                                                            .map(|s| format!("> {s}"))
                                                            .collect::<Vec<_>>()
                                                            .join("\n")
                                                    ),
                                                    w,
                                                    cx,
                                                );
                                                s.focus(w, cx)
                                            });
                                        }
                                    });
                                },
                            ),
                        )
                        .when_some(fork.clone(), |m, id| {
                            m.item(
                                gpui_component::menu::PopupMenuItem::new("ここから分岐").on_click(
                                    move |_, _, cx| {
                                        let _ = owner.update(cx, |app, cx| {
                                            if source.visible(app)
                                                && app.open_sheet(state::SheetPage::Fork).is_ok()
                                            {
                                                app.sheet_action(
                                                    state::SheetAction::Fork(id.clone()),
                                                    cx,
                                                );
                                            }
                                        });
                                    },
                                ),
                            )
                        })
                })
                .into_any_element()
            });
            theme::column(item).into_any_element()
        })
        .size_full();
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .track_focus(&reader)
            .child(list)
            .vertical_scrollbar(&self.scroll)
            .when(!at_end, |d| {
                d.child(
                    div().absolute().bottom_2().right_4().child(
                        Button::new("latest")
                            .small()
                            .label("最新へ ↓")
                            .on_click(move |_, w, _| {
                                latest.scroll_to(ListOffset {
                                    item_ix: latest.item_count(),
                                    offset_in_item: px(0.),
                                });
                                w.refresh()
                            }),
                    ),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::transcript;
    use gpui::{Context, ListOffset, ListState, Window, div, list, point, prelude::*, px, size};
    #[test]
    fn follow_only_new_content_from_the_tail() {
        assert!(super::should_follow(true, true, false, false));
        for (changed, tail, manual, focused) in [
            (true, false, false, false),
            (false, true, false, false),
            (true, true, true, false),
            (true, true, false, true),
        ] {
            assert!(!super::should_follow(changed, tail, manual, focused));
        }
    }
    #[test]
    fn folding_keeps_mixed_replies_and_aborts_but_hides_their_work() {
        use serde_json::json;
        let entries = vec![
            ("u".into(), json!({"kind":"user"})),
            (
                "a".into(),
                json!({"kind":"assistant","text":"読みました","thinking":"考察","toolCalls":[{"id":"t"}]}),
            ),
            ("r".into(), json!({"kind":"tool_result","toolCallId":"t"})),
            (
                "end".into(),
                json!({"kind":"assistant","stopReason":"aborted"}),
            ),
        ];
        let mut opened = std::collections::HashSet::new();
        let folded = super::LayoutRows::new(&entries, false, &opened);
        assert_eq!(folded.indices, [0, 1, 3]);
        assert!(folded.folded_answers.contains(&1));
        assert_eq!(folded.calls["t"], (1, 0, 0));
        opened.insert("u".into());
        let expanded = super::LayoutRows::new(&entries, false, &opened);
        assert_eq!(expanded.indices, [0, 1, 2, 3]);
        assert!(expanded.folded_answers.is_empty());
    }
    #[test]
    fn tool_subjects_and_turn_caption() {
        use serde_json::json;
        for (name, args, details, expected) in [
            ("read", json!({"path":"a.rs"}), json!({}), "a.rs"),
            ("read", json!({"file_path":"日本語"}), json!({}), "日本語"),
            ("bash", json!({"command":"pwd\nls"}), json!({}), "pwd"),
            ("bash", json!({"path":"x"}), json!({}), ""),
            (
                "edit",
                json!({"path":"a"}),
                json!({"patch":"@@ -1 +1,2 @@\n-a\n+b\n+c"}),
                "a +2 −1",
            ),
            ("edit", json!({"path":"a"}), json!({"patch":"unknown"}), "a"),
            ("write", json!({"path":"new"}), json!({}), "new"),
            ("read", json!(null), json!(null), ""),
            (
                "unknown",
                json!({"password":"secret","path":"secret"}),
                json!({}),
                "",
            ),
            ("write", json!({"path":42}), json!({}), ""),
        ] {
            assert_eq!(super::tool_subject(name, &args, &details), expected);
        }
        assert_eq!(
            super::work_summary(Some(2000), Some(20000), 4),
            "18秒 作業 · ツール 4 回"
        );
        assert_eq!(super::thought_label(true, None), "Thinking…");
        assert_eq!(super::thought_label(false, Some(500)), "Thought");
        assert_eq!(super::thought_label(false, Some(1600)), "Thought for 2s");
    }
    #[gpui::test]
    fn stream_image_identity_survives_interruptions_but_not_the_next_answer(
        _cx: &mut gpui::TestAppContext,
    ) {
        let list = transcript();
        let mut c = super::Conversation::default();
        let mut entries = vec![];
        c.prepare(&entries, true, &list, false, entries.len() as u64, 0);
        let first = c.stream_generation;
        entries.push(("interrupt".into(), serde_json::json!({"kind":"user"})));
        c.prepare(&entries, true, &list, false, entries.len() as u64, 0);
        assert_eq!(c.stream_generation, first);
        entries.push(("done".into(), serde_json::json!({"kind":"assistant"})));
        c.prepare(&entries, true, &list, false, entries.len() as u64, 0);
        let second = c.stream_generation;
        assert_ne!(first, second);
        c.prepare(&entries, true, &list, false, entries.len() as u64, 0);
        assert_eq!(second, c.stream_generation);
        c.prepare(&entries, false, &list, false, entries.len() as u64, 0);
        c.prepare(&entries, true, &list, false, entries.len() as u64, 0);
        assert_ne!(second, c.stream_generation);
    }
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
                            super::ToolLine {
                                name: "read",
                                text: "complete output",
                                state: "完了",
                                subject: String::new(),
                                duration: None,
                            },
                            None,
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
