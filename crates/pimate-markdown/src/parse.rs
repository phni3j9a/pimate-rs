use super::*;
use markdown::{
    ParseOptions,
    mdast::{AlignKind, Node},
};
use regex::Regex;
use std::{collections::HashMap, sync::LazyLock};

pub fn parse(source: &str, file_images: bool) -> Document {
    // Later reference definitions can change already emitted links and images.
    let mut options = ParseOptions::gfm();
    options.constructs.gfm_footnote_definition = false;
    options.constructs.gfm_label_start_footnote = false;
    let root =
        markdown::to_mdast(source, &options).expect("GFM without MDX cannot return syntax errors");
    let mut defs = HashMap::new();
    definitions(&root, &mut defs);
    let mut parser = Render {
        source,
        file_images,
        defs,
        doc: Document::default(),
    };
    parser.doc.blocks = parser.blocks(&root);
    parser.doc
}
fn definitions<'a>(n: &'a Node, defs: &mut HashMap<&'a str, &'a str>) {
    if let Node::Definition(d) = n {
        defs.entry(&d.identifier).or_insert(&d.url);
    }
    if let Some(children) = n.children() {
        for c in children {
            definitions(c, defs);
        }
    }
}
struct Render<'a> {
    source: &'a str,
    file_images: bool,
    defs: HashMap<&'a str, &'a str>,
    doc: Document,
}
fn children(n: &Node) -> &[Node] {
    n.children().map_or(&[], Vec::as_slice)
}
impl Render<'_> {
    fn blocks(&mut self, n: &Node) -> Vec<Block> {
        let parent_end = n.position().expect("parsed node position").end.offset;
        children(n)
            .iter()
            .filter_map(|c| self.block(c, parent_end))
            .collect()
    }
    fn block(&mut self, n: &Node, parent_end: usize) -> Option<Block> {
        let pos = n.position()?;
        let start = pos.start.offset;
        let end = pos.end.offset;
        let content = match n {
            Node::Paragraph(_) => Content::Paragraph(self.spans(n)),
            Node::Heading(h) => Content::Heading {
                level: h.depth,
                spans: self.spans(n),
            },
            Node::ThematicBreak(_) => Content::Rule,
            Node::Blockquote(_) => Content::Quote(self.blocks(n)),
            Node::List(l) => Content::List {
                start: l.start,
                items: self.blocks(n),
            },
            Node::ListItem(i) => Content::Item {
                checked: i.checked,
                children: self.blocks(n),
            },
            Node::Code(c) => Content::Code {
                language: c.lang.clone(),
                text: c.value.replace("\r\n", "\n"),
            },
            Node::Table(t) => {
                let align = t
                    .align
                    .iter()
                    .map(|a| match a {
                        AlignKind::None => Alignment::Default,
                        AlignKind::Left => Alignment::Left,
                        AlignKind::Center => Alignment::Center,
                        AlignKind::Right => Alignment::Right,
                    })
                    .collect::<Vec<_>>();
                let mut rows = t.children.iter().map(|row| {
                    let mut cells: Vec<_> = children(row)
                        .iter()
                        .take(align.len())
                        .map(|c| self.spans(c))
                        .collect();
                    cells.resize_with(align.len(), Vec::new);
                    cells
                });
                let header = rows.next().unwrap_or_default();
                let rows = rows.collect();
                let markdown = table_markdown(self.source, &t.children, start, end, parent_end);
                Content::Table(Table {
                    align,
                    header,
                    rows,
                    markdown,
                })
            }
            Node::Definition(_) => return None,
            _ => Content::Paragraph(vec![Inline::Text {
                text: self.source[start..end].to_owned(),
                style: Style::default(),
            }]),
        };
        Some(Block {
            id: start as u64,
            end: end as u64,
            content,
        })
    }
    fn spans(&mut self, n: &Node) -> Vec<Inline> {
        let mut out = vec![];
        for c in children(n) {
            self.inline(c, &Style::default(), &mut out);
        }
        out
    }
    fn link(&mut self, url: &str) -> Option<String> {
        image::web(url).then(|| {
            self.doc.links.push(url.to_owned());
            url.to_owned()
        })
    }
    fn inline(&mut self, n: &Node, base: &Style, out: &mut Vec<Inline>) {
        let mut style = base.clone();
        match n {
            Node::Image(_) | Node::ImageReference(_) => {
                let (src, alt) = match n {
                    Node::Image(i) => (i.url.as_str(), &i.alt),
                    Node::ImageReference(i) => {
                        (*self.defs.get(i.identifier.as_str()).unwrap_or(&""), &i.alt)
                    }
                    _ => unreachable!(),
                };
                if let Some(kind) =
                    classify_image(src).filter(|k| *k != ImageKind::File || self.file_images)
                {
                    let img = Image {
                        kind,
                        src: src.to_owned(),
                        alt: alt.clone(),
                        index: self.doc.images.len() as u64,
                    };
                    self.doc.images.push(img.clone());
                    out.push(Inline::Image(img));
                } else {
                    style.href = self.link(src).or(style.href);
                    out.push(Inline::Text {
                        text: if alt.is_empty() {
                            src.to_owned()
                        } else {
                            alt.clone()
                        },
                        style,
                    });
                }
                return;
            }
            Node::Strong(_) => style.bold = true,
            Node::Emphasis(_) => style.italic = true,
            Node::Delete(_) => style.strike = true,
            Node::Link(l) => style.href = self.link(&l.url),
            Node::LinkReference(l) => {
                style.href = self
                    .defs
                    .get(l.identifier.as_str())
                    .copied()
                    .and_then(|u| self.link(u))
            }
            Node::Text(t) => {
                let text = t.value.replace("\r\n", "\n");
                static BOLD: LazyLock<Regex> =
                    LazyLock::new(|| Regex::new(r"\*\*([^*\n]+)\*\*").unwrap());
                let mut last = 0;
                let raw = n
                    .position()
                    .map(|p| &self.source[p.start.offset..p.end.offset]); // Don't revive escaped markup.
                if text.contains("**") && !raw.is_some_and(|r| r.contains("\\*")) {
                    for m in BOLD.captures_iter(&text) {
                        let whole = m.get(0).unwrap();
                        push(out, &text[last..whole.start()], &style);
                        push(
                            out,
                            &m[1],
                            &Style {
                                bold: true,
                                ..style.clone()
                            },
                        );
                        last = whole.end();
                    }
                }
                push(out, &text[last..], &style);
                return;
            }
            Node::Break(_) => {
                push(out, "\n", &style);
                return;
            }
            Node::InlineCode(c) => {
                style.code = true;
                push(out, &c.value, &style);
                return;
            }
            Node::Html(h) => {
                push(out, &h.value, &style);
                return;
            }
            _ => {}
        }
        for c in children(n) {
            self.inline(c, &style, out);
        }
    }
}
fn push(out: &mut Vec<Inline>, text: &str, style: &Style) {
    if !text.is_empty() {
        out.push(Inline::Text {
            text: text.to_owned(),
            style: style.clone(),
        });
    }
}

/// Row positions omit outer list/quote markers; the delimiter is the only line without a row node.
fn table_markdown(
    source: &str,
    rows: &[Node],
    start: usize,
    end: usize,
    parent_end: usize,
) -> String {
    let row = |node: &Node| {
        let p = node.position().expect("table row position");
        &source[p.start.offset..p.end.offset]
    };
    let mut raw = row(&rows[0]).to_owned();
    let prefix = source[..start].rsplit(['\n', '\r']).next().unwrap_or("");
    let after_header = &source[start + raw.len()..end];
    let delimiter = after_header
        .trim_start_matches(['\r', '\n'])
        .split(['\r', '\n'])
        .next()
        .unwrap_or("");
    let indent = delimiter
        .bytes()
        .take_while(|b| matches!(b, b' ' | b'\t' | b'>'))
        .take(prefix.len())
        .count();
    raw.push('\n');
    raw.push_str(&delimiter[indent..]);
    for node in &rows[1..] {
        raw.push('\n');
        raw.push_str(row(node));
    }
    // marked includes a single line ending before EOF or another block, not a blank separator.
    let tail = &source[end..parent_end];
    if tail
        .strip_prefix("\r\n")
        .or_else(|| tail.strip_prefix(['\r', '\n']))
        .is_some_and(|s| !s.starts_with(['\n', '\r']))
    {
        raw.push('\n');
    }
    raw
}
