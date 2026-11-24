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
                let url = style
                    .href
                    .as_deref()
                    .filter(|u| u.starts_with("https://") || u.starts_with("http://"));
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
        "[web](https://example.com) [bad](file:///secret) ![ALT](https://example.com/img) `<>`",
    );
    assert!(out.contains("href=\"https://example.com\""));
    assert!(out.contains("ALT"));
    assert!(!out.contains("file:"));
    assert!(!out.contains("<img"));
    assert!(out.contains("&lt;&gt;"));
}

use gpui::{App, SharedString, Task, Window};
use gpui_component::text::TextView;
struct Cached {
    source: String,
    markup: SharedString,
    pending: Option<Task<()>>,
}
/// Parse away from the UI thread. Replacing the task prevents a late parse replacing newer text.
pub fn view(id: String, text: &str, w: &mut Window, cx: &mut App) -> TextView {
    let id: SharedString = id.into();
    let cache = w.use_keyed_state(SharedString::from(format!("parsed-{id}")), cx, |_, _| {
        Cached {
            source: String::new(),
            markup: "".into(),
            pending: None,
        }
    });
    if cache.read(cx).source != text {
        cache.update(cx, |c, cx| {
            c.source = text.into();
            let input = c.source.clone();
            let parse = cx.background_executor().spawn(async move { html(&input) });
            c.pending = Some(cx.spawn(async move |this, cx| {
                let markup = parse.await;
                let _ = this.update(cx, |c, cx| {
                    c.markup = markup.into();
                    cx.notify()
                });
            }));
        });
    }
    TextView::html(id, cache.read(cx).markup.clone(), w, cx).selectable(true)
}
