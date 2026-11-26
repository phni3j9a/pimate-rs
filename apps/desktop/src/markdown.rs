//! Render the shared AST; never interpret message content as HTML.
use crate::screen::chat_media::{Gallery, Media, Viewer};
use gpui::{
    AnyElement, App, ClipboardItem, Context, Entity, FocusHandle, FontStyle, FontWeight,
    HighlightStyle, MouseButton, SharedString, StrikethroughStyle, StyledText, Task,
    UnderlineStyle, Window, div, prelude::*, px, rems,
};
use gpui_component::ActiveTheme;
use gpui_component::{Sizable, button::Button};
use pimate_markdown::{Block, Content, Inline, Style as SpanStyle};
use std::{
    collections::{HashMap, VecDeque},
    ops::Range,
    sync::Arc,
    time::Duration,
};

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
    displayed: SharedString,
}
impl Selection {
    fn reconcile(&mut self, text: &SharedString) {
        let range = self.range(&self.displayed);
        // Re-parsing can remove markup before the selection, and the same words can reappear
        // elsewhere (e.g. a new link label). Keep the range only when everything up to its end
        // is unchanged; otherwise drop it rather than guess.
        if self.displayed.as_str() != text.as_str()
            && !text.starts_with(&self.displayed[..range.end])
        {
            self.anchor = 0;
            self.end = 0;
        }
        self.displayed = text.clone();
    }
    fn range(&self, text: &str) -> Range<usize> {
        let r = self.anchor.min(self.end)..self.anchor.max(self.end);
        if text.get(r.clone()).is_some() {
            r
        } else {
            0..0
        }
    }
}

// Only standalone references in prose, never the middle of a URL or email address.
fn references(text: &str) -> Vec<(Range<usize>, bool)> {
    let mut spans = Vec::new();
    let mut it = text.char_indices().peekable();
    while let Some((i, marker @ ('/' | '@'))) = it.find(|(_, c)| matches!(c, '/' | '@')) {
        if i > 0
            && !text[..i]
                .ends_with(|c: char| c.is_whitespace() || matches!(c, '(' | '[' | '（' | '「'))
        {
            continue;
        }
        let file = marker == '@';
        let mut end = i + 1;
        while let Some(&(j, c)) = it.peek() {
            if !(c.is_alphanumeric() || matches!(c, '-' | '_') || file && matches!(c, '/' | '.')) {
                break;
            }
            end = j + c.len_utf8();
            it.next();
        }
        if end > i + 1 && (file || !text[end..].starts_with('/')) {
            spans.push((i..end, file));
        }
    }
    spans
}
fn line(id: &str, spans: &[Inline], w: &mut Window, cx: &mut App) -> AnyElement {
    let state = w.use_keyed_state(
        SharedString::from(format!("selection:{id}")),
        cx,
        |_, cx| Selection {
            focus: cx.focus_handle(),
            anchor: 0,
            end: 0,
            displayed: SharedString::default(),
        },
    );
    let text: SharedString = spans
        .iter()
        .map(|s| match s {
            Inline::Image(i) => i.alt.as_str(),
            Inline::Text { text, .. } => text.as_str(),
        })
        .collect::<String>()
        .into();
    state.update(cx, |s, _| s.reconcile(&text));
    let selected = state.read(cx);
    let focus = selected.focus.clone();
    let selected = if focus.is_focused(w) {
        selected.anchor.min(selected.end)..selected.anchor.max(selected.end)
    } else {
        0..0
    };
    let mut end = 0;
    let mut runs = Vec::new();
    let mut links = Vec::new();
    for s in spans {
        let start = end;
        let mut style = HighlightStyle::default();
        let mut refs = Vec::new();
        match s {
            Inline::Image(i) => end += i.alt.len(),
            Inline::Text { text: t, style: m } => {
                end += t.len();
                style.font_weight = m.bold.then_some(FontWeight::BOLD);
                style.font_style = m.italic.then_some(FontStyle::Italic);
                style.strikethrough = m.strike.then_some(StrikethroughStyle {
                    thickness: px(1.),
                    ..Default::default()
                });
                if !m.code && m.href.is_none() {
                    refs = references(t);
                }
                style.background_color = m.code.then(|| cx.theme().muted);
                if let Some(url) = m.href.as_ref().filter(|u| web_link(u)) {
                    links.push((start..end, url.clone()));
                    style.color = Some(cx.theme().link);
                    style.underline = Some(UnderlineStyle {
                        thickness: px(1.),
                        ..Default::default()
                    });
                }
            }
        }
        // StyledText requires sorted, non-overlapping UTF-8 byte ranges.
        let mut cuts = vec![
            start,
            selected.start.clamp(start, end),
            selected.end.clamp(start, end),
            end,
        ];
        for (r, _) in &refs {
            cuts.extend([start + r.start, start + r.end]);
        }
        cuts.sort_unstable();
        let mut refs = refs.into_iter().peekable();
        for range in cuts.windows(2) {
            let (a, b) = (range[0], range[1]);
            if a == b {
                continue;
            }
            while refs.peek().is_some_and(|(r, _)| start + r.end <= a) {
                refs.next();
            }
            let mut hi = style;
            if let Some((r, file)) = refs.peek()
                && r.contains(&(a - start))
            {
                hi.color = Some(if *file {
                    cx.theme().link
                } else {
                    cx.theme().chart_2
                });
            }
            if selected.contains(&a) {
                hi.background_color = Some(cx.theme().selection);
            }
            runs.push((a..b, hi));
        }
    }
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

fn rich_line(
    id: &str,
    spans: &[Inline],
    media: Option<(&Media, &Gallery)>,
    w: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Some((m, g)) = media else {
        return line(id, spans, w, cx);
    };
    if !spans.iter().any(|s| matches!(s, Inline::Image(_))) {
        return line(&format!("{id}:0"), spans, w, cx);
    }
    let mut col = div().flex().flex_col();
    let mut start = 0;
    for (j, s) in spans.iter().enumerate() {
        if let Inline::Image(i) = s {
            if j > start {
                col = col.child(line(&format!("{id}:{start}"), &spans[start..j], w, cx));
            }
            start = j + 1;
            col = col.child(
                if let Some(link) = g.http_link(i.index, format!("link:{id}:{j}")) {
                    link.into_any_element()
                } else {
                    m.tile(
                        &format!("md:{id}:{j}:{:p}", i.src.as_ptr()),
                        g.clone(),
                        i.index,
                        w,
                        cx,
                    )
                },
            );
        }
    }
    if start < spans.len() {
        col = col.child(line(&format!("{id}:{start}"), &spans[start..], w, cx))
    }
    col.into_any_element()
}
fn block(
    parent: &str,
    b: &Block,
    viewer: Option<&Entity<Viewer>>,
    media: Option<(&Media, &Gallery)>,
    w: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let id = format!("{parent}/{}", b.id);
    let col = div().flex().flex_col().gap_1().min_w_0();
    match &b.content {
        Content::Paragraph(s) => rich_line(&id, s, media, w, cx),
        Content::Heading { level, spans } => col
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(rems(if *level <= 2 { 1.18 } else { 1. }))
            .child(rich_line(&id, spans, media, w, cx))
            .into_any_element(),
        Content::Rule => div().h(px(1.)).bg(cx.theme().border).into_any_element(),
        Content::Code { text, language } => crate::screen::context::on_right_click(
            col.bg(cx.theme().muted)
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
                )),
            {
                let text = text.clone();
                move |m| m.item(crate::screen::context::copy("コードをコピー", text.clone()))
            },
        )
        .into_any_element(),
        Content::Quote(children) | Content::Item { children, .. } => col
            .pl_2()
            .when(matches!(b.content, Content::Quote(_)), |c| {
                c.border_l_2().border_color(cx.theme().border)
            })
            .children(children.iter().map(|c| block(&id, c, viewer, media, w, cx)))
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
                div()
                    .flex()
                    .child(mark)
                    .child(block(&id, c, viewer, media, w, cx))
            }))
            .into_any_element(),
        Content::Table(t) => col
            .child(table(&id, t, media, w, cx))
            .when_some(viewer, |c, viewer| {
                let viewer = viewer.clone();
                let t = t.clone();
                let g = media.map(|(_, g)| g.clone());
                c.child(
                    Button::new(SharedString::from(format!("expand:{id}")))
                        .small()
                        .debug_selector(|| "open-markdown-table".into())
                        .label("表を開く")
                        .on_click(move |_, _, cx| {
                            viewer.update(cx, |v, cx| {
                                v.table = Some((t.clone(), g.clone()));
                                cx.notify()
                            })
                        }),
                )
            })
            .into_any_element(),
    }
}

pub(crate) fn table(
    id: &str,
    t: &pimate_markdown::Table,
    media: Option<(&Media, &Gallery)>,
    w: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let measured = w.use_keyed_state(
        SharedString::from(format!("table-width:{id}")),
        cx,
        |_, _| px(0.),
    );
    let available = f32::from(*measured.read(cx));
    let mut natural = vec![0; t.header.len()];
    for row in std::iter::once(&t.header).chain(&t.rows) {
        natural.resize(natural.len().max(row.len()), 0);
        for (n, cell) in natural.iter_mut().zip(row) {
            *n = (*n).max(pimate_markdown::cell_width(cell));
        }
    }
    let widths = pimate_markdown::table_widths(
        &natural,
        (available - 2.).max(0.) as f64,
        f32::from(w.rem_size()) as f64 / 16.,
    );
    let scroll = div()
        .flex()
        .flex_col()
        .min_w_0()
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
                                .flex_none()
                                .w(px(widths[k] as f32))
                                .px_2()
                                .py_1()
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .child(rich_line(&format!("{id}/{r}/{k}"), s, media, w, cx))
                        }))
                }),
        )
        .w_full();
    div()
        .w_full()
        .min_w_0()
        .child(scroll)
        .on_children_prepainted(move |bounds, _, cx| {
            if let Some(b) = bounds.first()
                && *measured.read(cx) != b.size.width
            {
                measured.update(cx, |v, cx| {
                    *v = b.size.width;
                    cx.notify()
                });
            }
        })
        .into_any_element()
}

#[derive(Default)]
struct Cached {
    latest: Arc<str>,
    published: Arc<str>,
    blocks: Vec<Block>,
    images: Arc<[pimate_markdown::Image]>,
    files: bool,
    pending: Option<Task<()>>,
}
fn sample(cache: &mut Cached, cx: &mut Context<Cached>, streaming: bool) {
    if cache.pending.is_some() || cache.latest == cache.published {
        return;
    }
    let files = cache.files;
    cache.pending = Some(cx.spawn(async move |this, cx| {
        if streaming {
            cx.background_executor()
                .timer(Duration::from_millis(125))
                .await;
        }
        let Ok(source) = this.read_with(cx, |c, _| c.latest.clone()) else {
            return;
        };
        let (source, doc) = cx
            .background_executor()
            .spawn(async move {
                let doc = pimate_markdown::parse(&source, files);
                (source, doc)
            })
            .await;
        let _ = this.update(cx, |c, cx| {
            c.published = source;
            c.blocks = doc.blocks;
            c.images = doc.images.into();
            c.pending = None;
            cx.notify();
        });
    }));
}
/// Keep recently visited messages measured when the virtual list unmounts a row.
/// The cache belongs to one editor; old sessions and their text are not retained.
#[derive(Default)]
pub struct Messages {
    mounted: HashMap<String, (bool, gpui::Entity<Cached>)>,
    offscreen: VecDeque<(String, gpui::Entity<Cached>)>,
}
pub(crate) enum DocumentControls<'a> {
    Chat(Option<(&'a Media, &'a str)>),
    TablesOnly(&'a Entity<Viewer>),
}
impl Messages {
    /// Layout decides which rows are visible. Only the unmounted pool is bounded.
    pub fn begin_frame(&mut self) {
        self.mounted.retain(|id, (seen, doc)| {
            if std::mem::take(seen) {
                return true;
            }
            self.offscreen.push_front((id.clone(), doc.clone()));
            false
        });
        self.offscreen.truncate(64);
    }
    fn take_offscreen(&mut self, id: &str) -> Option<gpui::Entity<Cached>> {
        let i = self.offscreen.iter().position(|(k, _)| k == id)?;
        self.offscreen.remove(i).map(|(_, doc)| doc)
    }
    pub fn finish_stream(&mut self, entry: Option<&str>) {
        let doc = self
            .mounted
            .remove("stream")
            .map(|(_, doc)| doc)
            .or_else(|| self.take_offscreen("stream"));
        if let (Some(doc), Some(id)) = (doc, entry) {
            self.mounted.insert(format!("entry:{id}"), (true, doc));
        }
    }
    pub fn view(
        &mut self,
        id: String,
        text: &str,
        streaming: bool,
        media: Option<(&Media, &str)>,
        w: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        self.render(id, text, streaming, DocumentControls::Chat(media), w, cx)
    }
    pub(crate) fn render(
        &mut self,
        id: String,
        text: &str,
        streaming: bool,
        controls: DocumentControls<'_>,
        w: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let (viewer, media) = match controls {
            DocumentControls::Chat(media) => (media.map(|(m, _)| &m.viewer), media),
            DocumentControls::TablesOnly(viewer) => (Some(viewer), None),
        };
        let cache = if let Some((seen, doc)) = self.mounted.get_mut(&id) {
            *seen = true;
            doc.clone()
        } else {
            let doc = self.take_offscreen(&id).unwrap_or_else(|| {
                let doc = cx.new(|_| Cached::default());
                let view = w.current_view();
                cx.observe(&doc, move |_, cx| cx.notify(view)).detach();
                doc
            });
            self.mounted.insert(id, (true, doc.clone()));
            doc
        };
        let col = div().flex().flex_col().gap_2().min_w_0();
        let namespace = cache.entity_id().as_u64().to_string();
        cache.update(cx, |c, cx| {
            let files = media.is_some_and(|(m, _)| m.files);
            if files != c.files {
                c.files = files;
                c.published = Arc::default();
                c.pending = None;
            }
            if c.latest.as_ref() != text {
                c.latest = text.into();
            }
            sample(c, cx, streaming);
            let gallery =
                media.map(|(m, id)| (m, Gallery::markdown(&m.page, id, c.images.clone())));
            col.children(c.blocks.iter().map(|b| {
                block(
                    &namespace,
                    b,
                    viewer,
                    gallery.as_ref().map(|(m, g)| (*m, g)),
                    w,
                    cx,
                )
            }))
            .into_any_element()
        })
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

#[cfg(test)]
#[gpui::test]
fn reparsed_markup_never_retargets_the_selection(cx: &mut gpui::TestAppContext) {
    let plain = |source| pimate_markdown::parse(source, false).blocks[0].plain_text();
    cx.update(|cx| {
        let mut sel = Selection {
            focus: cx.focus_handle(),
            anchor: 7,
            end: 2,
            displayed: plain("**alpha beta").into(),
        };
        // Plain appends keep the selection.
        sel.reconcile(&plain("**alpha beta gamma").into());
        assert_eq!(&sel.displayed[sel.range(&sel.displayed)], "alpha");
        // Closing the markup shifts the text before the selection: drop, never copy "pha b".
        sel.reconcile(&plain("**alpha beta gamma**").into());
        assert!(sel.range(&sel.displayed).is_empty());
        // A selected word that becomes a link target must not jump to a new label with the same text.
        let shown: SharedString = plain("[doc](https://old.example").into();
        let at = shown.find("old").unwrap();
        let mut sel = Selection {
            focus: cx.focus_handle(),
            anchor: at,
            end: at + 3,
            displayed: shown,
        };
        assert_eq!(&sel.displayed[sel.range(&sel.displayed)], "old");
        sel.reconcile(&plain("[doc](https://old.example) [old](https://new.example)").into());
        assert!(sel.range(&sel.displayed).is_empty());
    });
}

#[cfg(test)]
#[gpui::test]
fn selected_prefix_copies_after_image_appends(cx: &mut gpui::TestAppContext) {
    use gpui::point;
    struct Text(String);
    impl Render for Text {
        fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let doc = pimate_markdown::parse(&self.0, true);
            let Content::Paragraph(spans) = &doc.blocks[0].content else {
                panic!()
            };
            let media = Media {
                page: crate::state::dummy_page(),
                files: true,
                viewer: w.use_state(cx, |_, _| Default::default()),
            };
            let gallery = Gallery::markdown(&media.page, "a", doc.images.into());
            rich_line("answer", spans, Some((&media, &gallery)), w, cx)
        }
    }
    cx.update(gpui_component::init);
    let (v, w) = cx.add_window_view(|_, _| Text("stable paragraph".into()));
    let a = point(px(1.), px(8.));
    let b = point(px(39.), px(8.));
    w.simulate_mouse_down(a, MouseButton::Left, Default::default());
    w.simulate_mouse_move(b, MouseButton::Left, Default::default());
    w.simulate_mouse_up(b, MouseButton::Left, Default::default());
    let copy = if cfg!(target_os = "macos") {
        "cmd-c"
    } else {
        "ctrl-c"
    };
    w.simulate_keystrokes(copy);
    let first = w
        .update(|_, cx| cx.read_from_clipboard())
        .and_then(|c| c.text())
        .unwrap();
    assert!(!first.is_empty() && "stable paragraph".starts_with(&first));
    w.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("not copied".into())));
    v.update(w, |v, cx| {
        v.0.push_str(" ![plot](http://example.org/image.png)");
        cx.notify()
    });
    w.run_until_parked();
    w.simulate_keystrokes(copy);
    assert_eq!(cx.read_from_clipboard().and_then(|c| c.text()), Some(first));
}

#[cfg(test)]
#[gpui::test]
fn tall_viewport_keeps_every_mounted_document(cx: &mut gpui::TestAppContext) {
    use gpui::{Entity, ListAlignment, ListOffset, ListState, list, size};
    struct History {
        docs: Entity<Messages>,
        scroll: ListState,
    }
    impl Render for History {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.docs.update(cx, |docs, _| docs.begin_frame());
            let docs = self.docs.clone();
            list(self.scroll.clone(), move |i, w, cx| {
                div()
                    .h(px(24.))
                    .child(docs.update(cx, |docs, cx| {
                        docs.view(i.to_string(), "short", false, None, w, cx)
                    }))
                    .into_any_element()
            })
            .size_full()
        }
    }
    cx.update(gpui_component::init);
    let (history, w) = cx.add_window_view(|_, cx| History {
        docs: cx.new(|_| Messages::default()),
        scroll: ListState::new(500, ListAlignment::Top, px(0.)),
    });
    w.simulate_resize(size(px(600.), px(2400.)));
    w.run_until_parked();
    let (docs, scroll) = history.read_with(w, |h, _| (h.docs.clone(), h.scroll.clone()));
    let mounted = |w: &gpui::VisualTestContext| {
        docs.read_with(w, |d, cx| {
            d.mounted
                .iter()
                .filter(|(_, (seen, _))| *seen)
                .map(|(id, (_, doc))| {
                    assert_eq!(doc.read(cx).published.as_ref(), "short");
                    (id.clone(), doc.entity_id())
                })
                .collect::<HashMap<_, _>>()
        })
    };
    let initial = mounted(w);
    assert!(initial.len() > 64, "only {} rows mounted", initial.len());
    history.update(w, |_, cx| cx.notify());
    w.run_until_parked();
    assert_eq!(initial, mounted(w));
    scroll.scroll_to(ListOffset {
        item_ix: 250,
        offset_in_item: px(0.),
    });
    history.update(w, |_, cx| cx.notify());
    w.run_until_parked();
    history.update(w, |_, cx| cx.notify());
    w.run_until_parked();
    let later = mounted(w);
    assert!(later.len() > 64 && initial.keys().all(|id| !later.contains_key(id)));
    docs.read_with(w, |d, _| assert_eq!(d.offscreen.len(), 64));
}

#[test]
fn references_ignore_addresses_and_keep_unicode_byte_ranges() {
    let text = "@場所/名.rs と /help user@example.org https://a/b /src/file";
    assert_eq!(
        references(text)
            .into_iter()
            .map(|(r, file)| (&text[r], file))
            .collect::<Vec<_>>(),
        [("@場所/名.rs", true), ("/help", false)]
    );
}
