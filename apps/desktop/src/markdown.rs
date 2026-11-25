//! Convert only the shared parser's vocabulary. Never pass host HTML, local paths or remote images to GPUI.
use pimate_markdown::{Block, Content, Inline};
use std::fmt::Write;
fn escape(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}
fn spans(out: &mut String, spans: &[Inline]) {
    for span in spans {
        match span {
            Inline::Image(i) => escape(out, &i.alt),
            Inline::Text { text, style } => {
                let url = style.href.as_deref().filter(|u| {
                    u.split_once("://").is_some_and(|(scheme, _)| {
                        scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http")
                    })
                });
                if let Some(u) = url {
                    out.push_str("<a href=\"");
                    escape(out, u);
                    out.push_str("\">")
                };
                let tags = [
                    (style.bold, "b"),
                    (style.italic, "i"),
                    (style.strike, "s"),
                    (style.code, "code"),
                ];
                for &(on, t) in &tags {
                    if on {
                        write!(out, "<{t}>").unwrap()
                    }
                }
                escape(out, text);
                for &(on, t) in tags.iter().rev() {
                    if on {
                        write!(out, "</{t}>").unwrap()
                    }
                }
                if url.is_some() {
                    out.push_str("</a>")
                }
            }
        }
    }
}
fn blocks(out: &mut String, children: &[Block]) {
    for block in children {
        match &block.content {
            Content::Heading { level, spans: s } => {
                write!(out, "<h{level}>").unwrap();
                spans(out, s);
                write!(out, "</h{level}>").unwrap()
            }
            Content::Paragraph(s) => {
                out.push_str("<p>");
                spans(out, s);
                out.push_str("</p>")
            }
            Content::Rule => out.push_str("<hr>"),
            Content::Code { text, .. } => {
                out.push_str("<pre><code>");
                escape(out, text);
                out.push_str("</code></pre>")
            }
            Content::Quote(c) => {
                out.push_str("<blockquote>");
                blocks(out, c);
                out.push_str("</blockquote>")
            }
            Content::List { items, start } => {
                if let Some(n) = start {
                    write!(out, "<ol start=\"{n}\">").unwrap()
                } else {
                    out.push_str("<ul>")
                };
                blocks(out, items);
                out.push_str(if start.is_some() { "</ol>" } else { "</ul>" })
            }
            Content::Item { children, checked } => {
                out.push_str("<li>");
                if let Some(c) = checked {
                    out.push_str(if *c { "☑ " } else { "☐ " })
                };
                blocks(out, children);
                out.push_str("</li>")
            }
            Content::Table(t) => {
                out.push_str("<table>");
                for (i, row) in std::iter::once(&t.header).chain(&t.rows).enumerate() {
                    out.push_str("<tr>");
                    for cell in row {
                        out.push_str(if i == 0 { "<th>" } else { "<td>" });
                        spans(out, cell);
                        out.push_str(if i == 0 { "</th>" } else { "</td>" });
                    }
                    out.push_str("</tr>");
                }
                out.push_str("</table>");
            }
        }
    }
}
pub fn html(text: &str) -> String {
    let mut out = String::new();
    blocks(&mut out, &pimate_markdown::parse(text, false).blocks);
    out
}
#[test]
fn only_web_links_and_no_image_loads() {
    let out = html(
        "[web](HTTPS://example.com) [bad](file:///secret) ![ALT](https://example.com/img) `<>`",
    );
    assert!(out.contains("href=\"HTTPS://example.com\""));
    assert!(out.contains("ALT"));
    assert!(!out.contains("file:"));
    assert!(!out.contains("<img"));
    assert!(out.contains("&lt;&gt;"));
}

use gpui::{App, SharedString, Styled, Task, Window};
use gpui_component::text::TextView;
#[derive(Default)]
struct Cached {
    latest: String,
    published: String,
    markup: SharedString,
    generation: u64,
    pending: Option<Task<()>>,
}
fn sample(cache: &mut Cached, cx: &mut gpui::Context<Cached>, streaming: bool) {
    if cache.pending.is_some() || cache.latest == cache.published {
        return;
    }
    cache.pending = Some(cx.spawn(async move |this, cx| {
        // Keep the first deadline even when more deltas arrive. Only one parse in flight.
        if streaming {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(125))
                .await;
        }
        let Ok(source) = this.read_with(cx, |c, _| c.latest.clone()) else {
            return;
        };
        let parsed = cx
            .background_executor()
            .spawn(async move {
                let markup = html(&source);
                (source, markup)
            })
            .await;
        let _ = this.update(cx, |c, cx| {
            c.published = parsed.0;
            c.markup = parsed.1.into();
            c.generation += 1;
            c.pending = None;
            cx.notify();
        });
    }));
}
pub fn view(id: String, text: &str, streaming: bool, w: &mut Window, cx: &mut App) -> TextView {
    let cache = w.use_keyed_state(SharedString::from(format!("parsed-{id}")), cx, |_, _| {
        Cached::default()
    });
    cache.update(cx, |c, cx| {
        if c.latest != text {
            c.latest = text.into();
        }
        sample(c, cx, streaming)
    });
    let c = cache.read(cx);
    // TextView debounces same-ID updates until 200 ms of silence. Mount the immutable
    // stream sample with its own ID; settled entries retain a stable selection identity.
    let id: SharedString = if streaming {
        format!("{id}-{}", c.generation)
    } else {
        id
    }
    .into();
    TextView::html(id, c.markup.clone(), w, cx)
        .selectable(true)
        .h_auto()
}

#[cfg(test)]
#[gpui::test]
fn publishes_while_fifty_ms_deltas_are_still_arriving(cx: &mut gpui::TestAppContext) {
    use gpui::AppContext;
    use std::time::Duration;
    let cache = cx.new(|_| Cached::default());
    for n in 0..6 {
        cache.update(cx, |c, cx| {
            c.latest = format!("delta {n}");
            sample(c, cx, true);
        });
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(50));
    }
    cx.run_until_parked();
    cache.read_with(cx, |c, _| {
        assert!(c.generation >= 2);
        assert!(c.markup.contains("delta"));
    });
}
