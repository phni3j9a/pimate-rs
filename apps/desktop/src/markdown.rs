//! Render the shared AST, never HTML. Images remain alt text; no automatic network or file reads.
use gpui::{
    AnyElement, App, ClipboardItem, Context, FocusHandle, FontStyle, FontWeight, HighlightStyle,
    MouseButton, SharedString, StrikethroughStyle, StyledText, Task, UnderlineStyle, Window, div,
    prelude::*, px, rems,
};
use gpui_component::ActiveTheme;
use pimate_markdown::{Block, Content, Inline, Style as SpanStyle};
use std::{ops::Range, sync::Arc, time::Duration};

fn web_link(s: &str) -> bool {
    s.split_once("://").is_some_and(|(scheme, host)| {
        !host.is_empty()
            && (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
    })
}

struct Selection {
    focus: FocusHandle,
    anchor: usize,
    end: usize,
}
impl Selection {
    fn range(&self, text: &str) -> Range<usize> {
        let r = self.anchor.min(self.end)..self.anchor.max(self.end);
        if text.get(r.clone()).is_some() {
            r
        } else {
            0..0
        }
    }
}

fn line(id: &str, spans: &[Inline], w: &mut Window, cx: &mut App) -> AnyElement {
    let state = w.use_keyed_state(
        SharedString::from(format!("selection:{id}")),
        cx,
        |_, cx| Selection {
            focus: cx.focus_handle(),
            anchor: 0,
            end: 0,
        },
    );
    let selected = state.read(cx);
    let focus = selected.focus.clone();
    let selected = if focus.is_focused(w) {
        selected.anchor.min(selected.end)..selected.anchor.max(selected.end)
    } else {
        0..0
    };
    let mut text = String::new();
    let mut runs = Vec::new();
    let mut links = Vec::new();
    for s in spans {
        let start = text.len();
        let mut style = HighlightStyle::default();
        match s {
            Inline::Image(i) => text.push_str(&i.alt),
            Inline::Text { text: t, style: m } => {
                text.push_str(t);
                style.font_weight = m.bold.then_some(FontWeight::BOLD);
                style.font_style = m.italic.then_some(FontStyle::Italic);
                style.strikethrough = m.strike.then_some(StrikethroughStyle {
                    thickness: px(1.),
                    ..Default::default()
                });
                style.background_color = m.code.then(|| cx.theme().muted);
                if let Some(url) = m.href.as_ref().filter(|u| web_link(u)) {
                    links.push((start..text.len(), url.clone()));
                    style.color = Some(cx.theme().link);
                    style.underline = Some(UnderlineStyle {
                        thickness: px(1.),
                        ..Default::default()
                    });
                }
            }
        }
        // StyledText requires sorted, non-overlapping UTF-8 byte ranges.
        let end = text.len();
        let mut a = start;
        for b in [selected.start, selected.end, end] {
            let b = b.min(end);
            if a < b && text.is_char_boundary(b) {
                let mut hi = style;
                if selected.contains(&a) {
                    hi.background_color = Some(cx.theme().selection);
                }
                runs.push((a..b, hi));
                a = b;
            }
        }
    }
    let text: SharedString = text.into();
    let styled = StyledText::new(text.clone()).with_highlights(runs);
    let pos = styled.layout().clone();
    let move_pos = pos.clone();
    let up_pos = pos.clone();
    let down = state.clone();
    let drag = state.clone();
    let up = state.clone();
    let keyboard_links = links.clone();
    div()
        .id(SharedString::from(id.to_owned()))
        .track_focus(&focus.tab_stop(!links.is_empty()))
        .when(!links.is_empty(), |d| {
            d.focus(|style| style.bg(cx.theme().muted))
        })
        .cursor_text()
        .on_mouse_down(MouseButton::Left, move |e, w, cx| {
            let i = pos.index_for_position(e.position).unwrap_or_else(|i| i);
            down.update(cx, |s, cx| {
                s.focus.focus(w);
                s.anchor = i;
                s.end = i;
                cx.notify()
            });
        })
        .on_mouse_move(move |e, w, cx| {
            if e.dragging() && drag.read(cx).focus.is_focused(w) {
                let i = move_pos
                    .index_for_position(e.position)
                    .unwrap_or_else(|i| i);
                drag.update(cx, |s, cx| {
                    if s.end != i {
                        s.end = i;
                        cx.notify()
                    }
                });
            }
        })
        .on_mouse_up(MouseButton::Left, move |e, w, cx| {
            if let Ok(i) = up_pos.index_for_position(e.position)
                && up.read(cx).focus.is_focused(w)
                && up.read(cx).anchor == i
                && up.read(cx).end == i
                && let Some((_, url)) = links.iter().find(|(r, _)| r.contains(&i))
            {
                cx.open_url(url)
            }
        })
        .on_key_down(move |e, _, cx| {
            // One tab stop per linked paragraph, arrows select its next/previous link.
            if !keyboard_links.is_empty() && !e.keystroke.modifiers.modified() {
                let selected = state.read(cx).range(&text);
                let current = keyboard_links.iter().position(|(r, _)| *r == selected);
                let count = keyboard_links.len();
                let next = match e.keystroke.key.as_str() {
                    "left" => Some(current.map_or(count - 1, |i| (i + count - 1) % count)),
                    "right" => Some(current.map_or(0, |i| (i + 1) % count)),
                    "enter" => {
                        cx.open_url(&keyboard_links[current.unwrap_or(0)].1);
                        cx.stop_propagation();
                        None
                    }
                    _ => None,
                };
                if let Some(i) = next {
                    let r = &keyboard_links[i].0;
                    state.update(cx, |s, cx| {
                        s.anchor = r.start;
                        s.end = r.end;
                        cx.notify();
                    });
                    cx.stop_propagation();
                }
            }
            if e.keystroke.modifiers.secondary() && e.keystroke.key == "c" {
                let r = state.read(cx).range(&text);
                if !r.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text[r].into()));
                    cx.stop_propagation();
                }
            }
        })
        .child(styled)
        .into_any_element()
}

fn block(parent: &str, b: &Block, w: &mut Window, cx: &mut App) -> AnyElement {
    let id = format!("{parent}/{}", b.id);
    let col = div().flex().flex_col().gap_1().min_w_0();
    match &b.content {
        Content::Paragraph(s) => line(&id, s, w, cx),
        Content::Heading { level, spans } => col
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(rems(if *level <= 2 { 1.18 } else { 1. }))
            .child(line(&id, spans, w, cx))
            .into_any_element(),
        Content::Rule => div().h(px(1.)).bg(cx.theme().border).into_any_element(),
        Content::Code { text, language } => col
            .bg(cx.theme().muted)
            .rounded_md()
            .p_3()
            .font_family(cx.theme().mono_font_family.clone())
            .when_some(language.as_ref(), |c, l| {
                c.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(l.clone()),
                )
            })
            .child(line(
                &id,
                &[Inline::Text {
                    text: text.clone(),
                    style: SpanStyle::default(),
                }],
                w,
                cx,
            ))
            .into_any_element(),
        Content::Quote(children) | Content::Item { children, .. } => col
            .pl_2()
            .when(matches!(b.content, Content::Quote(_)), |c| {
                c.border_l_2().border_color(cx.theme().border)
            })
            .children(children.iter().map(|c| block(&id, c, w, cx)))
            .into_any_element(),
        Content::List { start, items } => col
            .children(items.iter().enumerate().map(|(i, c)| {
                let mark = match c.content {
                    Content::Item {
                        checked: Some(v), ..
                    } => {
                        if v {
                            "☑".into()
                        } else {
                            "☐".into()
                        }
                    }
                    _ => start
                        .map_or_else(|| "•".into(), |n| format!("{}.", u64::from(n) + i as u64)),
                };
                div().flex().child(mark).child(block(&id, c, w, cx))
            }))
            .into_any_element(),
        Content::Table(t) => col
            .id(SharedString::from(format!("table:{id}")))
            .overflow_x_scroll()
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .gap_0()
            .children(
                std::iter::once(&t.header)
                    .chain(&t.rows)
                    .enumerate()
                    .map(|(r, row)| {
                        div()
                            .flex()
                            .when(r == 0, |v| {
                                v.bg(cx.theme().muted).font_weight(FontWeight::SEMIBOLD)
                            })
                            .children(row.iter().enumerate().map(|(k, s)| {
                                div()
                                    .flex_1()
                                    .min_w(rems(6.))
                                    .px_2()
                                    .py_1()
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .child(line(&format!("{id}/{r}/{k}"), s, w, cx))
                            }))
                    }),
            )
            .into_any_element(),
    }
}

#[derive(Default)]
struct Cached {
    latest: Arc<str>,
    published: Arc<str>,
    blocks: Vec<Block>,
    pending: Option<Task<()>>,
}
fn sample(cache: &mut Cached, cx: &mut Context<Cached>, streaming: bool) {
    if cache.pending.is_some() || cache.latest == cache.published {
        return;
    }
    cache.pending = Some(cx.spawn(async move |this, cx| {
        if streaming {
            cx.background_executor()
                .timer(Duration::from_millis(125))
                .await;
        }
        let Ok(source) = this.read_with(cx, |c, _| c.latest.clone()) else {
            return;
        };
        let (source, blocks) = cx
            .background_executor()
            .spawn(async move {
                let doc = pimate_markdown::parse(&source, false);
                (source, doc.blocks)
            })
            .await;
        let _ = this.update(cx, |c, cx| {
            c.published = source;
            c.blocks = blocks;
            c.pending = None;
            cx.notify();
        });
    }));
}
/// Keep recently visited messages measured when the virtual list unmounts a row.
/// The cache belongs to one editor; old sessions and their text are not retained.
#[derive(Default)]
pub struct Messages(std::collections::VecDeque<(String, gpui::Entity<Cached>)>);
impl Messages {
    pub fn view(
        &mut self,
        id: String,
        text: &str,
        streaming: bool,
        w: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let cache = if let Some(i) = self.0.iter().position(|(k, _)| k == &id) {
            self.0.remove(i).unwrap().1
        } else {
            let doc = cx.new(|_| Cached::default());
            let view = w.current_view();
            cx.observe(&doc, move |_, cx| cx.notify(view)).detach();
            doc
        };
        let col = div().flex().flex_col().gap_2().min_w_0();
        let result = cache.update(cx, |c, cx| {
            if c.latest.as_ref() != text {
                c.latest = text.into();
            }
            sample(c, cx, streaming);
            col.children(c.blocks.iter().map(|b| block(&id, b, w, cx)))
                .into_any_element()
        });
        self.0.push_front((id, cache));
        self.0.truncate(64);
        result
    }
}

#[test]
fn only_web_links() {
    for s in ["https://example.org", "HtTp://localhost"] {
        assert!(web_link(s));
    }
    for s in ["file:///x", "javascript:alert(1)", "HTTPS://", "/local"] {
        assert!(!web_link(s));
    }
}
#[cfg(test)]
#[gpui::test]
fn publishes_while_fifty_ms_deltas_are_still_arriving(cx: &mut gpui::TestAppContext) {
    let cache = cx.new(|_| Cached::default());
    for n in 0..6 {
        cache.update(cx, |c, cx| {
            c.latest = format!("stable\n\ndelta {n}").into();
            sample(c, cx, true)
        });
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    cx.run_until_parked();
    cache.read_with(cx, |c, _| {
        assert_eq!(c.blocks[0].id, 0);
        assert!(c.blocks[1].plain_text().starts_with("delta"));
        assert_ne!(c.published.as_ref(), "stable\n\ndelta 0");
    });
}
