use super::*;
use markdown::{
    ParseOptions,
    mdast::{AlignKind, Node},
};
use regex::Regex;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

pub fn parse(source: &str, file_images: bool) -> Document {
    let limit = budget::prefix(source);
    let mut doc = parse_bounded(&source[..limit], file_images);
    if limit < source.len() {
        doc.blocks.push(literal(source, limit, source.len()));
    }
    doc
}
fn literal(source: &str, start: usize, end: usize) -> Block {
    Block {
        id: start as u64,
        end: end as u64,
        content: Content::Paragraph(vec![Inline::Text {
            text: source[start..end].to_owned(),
            style: Style::default(),
        }]),
    }
}
fn parse_bounded(source: &str, file_images: bool) -> Document {
    // Later reference definitions can change already emitted links and images.
    let mut options = ParseOptions::gfm();
    options.constructs.gfm_footnote_definition = false;
    options.constructs.gfm_label_start_footnote = false;
    let mut root =
        markdown::to_mdast(source, &options).expect("GFM without MDX cannot return syntax errors");
    bound_tree(&mut root, source, 0);
    let mut defs = HashMap::new();
    definitions(&root, source, &mut defs);
    let mut parser = Render {
        source,
        file_images,
        defs,
        image_refs: HashMap::new(),
        expansion: 4 * 1024 * 1024,
        doc: Document::default(),
    };
    parser.doc.blocks = parser.blocks(&root);
    parser.doc
}
// A second, exact check bounds every recursive visitor and destructor after the syntax preflight.
fn bound_tree(n: &mut Node, source: &str, depth: usize) {
    if depth == budget::DEPTH && n.children().is_some() {
        let literal = markdown::mdast::Html {
            value: raw(source, n).to_owned(),
            position: n.position().cloned(),
        };
        let mut pending = vec![std::mem::replace(n, Node::Html(literal))];
        while let Some(mut old) = pending.pop() {
            if let Some(children) = old.children_mut() {
                pending.append(children);
            }
        }
    } else if let Some(children) = n.children_mut() {
        for child in children {
            bound_tree(child, source, depth + 1);
        }
    }
}
fn definitions<'a>(n: &'a Node, source: &str, defs: &mut HashMap<&'a str, (&'a str, Arc<str>)>) {
    if let Node::Definition(d) = n {
        defs.entry(&d.identifier)
            .or_insert_with(|| (&d.url, image_src(raw(source, n), &d.url, true)));
    }
    if let Some(children) = n.children() {
        for c in children {
            definitions(c, source, defs);
        }
    }
}
struct Render<'a> {
    source: &'a str,
    file_images: bool,
    defs: HashMap<&'a str, (&'a str, Arc<str>)>,
    image_refs: HashMap<String, Option<ImageKind>>,
    expansion: usize,
    doc: Document,
}
fn raw<'a>(source: &'a str, n: &Node) -> &'a str {
    let p = n.position().expect("mdast source position");
    &source[p.start.offset..p.end.offset]
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
                if t.children.len().saturating_mul(t.align.len()) > budget::CELLS {
                    return Some(literal(self.source, start, end));
                }
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
                    children(row)
                        .iter()
                        .take(align.len())
                        .map(|c| self.spans(c))
                        .collect()
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
    fn charge(&mut self, bytes: usize) -> bool {
        if let Some(left) = self.expansion.checked_sub(bytes) {
            self.expansion = left;
            true
        } else {
            false
        }
    }
    fn charge_link(&mut self, url: &str) -> bool {
        !image::web(url) || self.charge(url.len() * 2)
    }
    fn link(&mut self, url: &str) -> Option<Arc<str>> {
        image::web(url).then(|| {
            self.doc.links.push(url.to_owned());
            Arc::from(url)
        })
    }
    fn inline(&mut self, n: &Node, base: &Style, out: &mut Vec<Inline>) {
        let mut style = base.clone();
        match n {
            Node::Image(_) | Node::ImageReference(_) => {
                let (src, alt, kind) = match n {
                    Node::Image(i) => {
                        let src = image_src(raw(self.source, n), &i.url, false);
                        let kind = classify_image(&src);
                        (src, &i.alt, kind)
                    }
                    Node::ImageReference(i) => {
                        let Some((_, src)) = self.defs.get(i.identifier.as_str()) else {
                            return;
                        };
                        let kind = *self
                            .image_refs
                            .entry(i.identifier.clone())
                            .or_insert_with(|| classify_image(src));
                        (src.clone(), &i.alt, kind)
                    }
                    _ => unreachable!(),
                };
                if let Some(kind) = kind.filter(|k| *k != ImageKind::File || self.file_images) {
                    let img = Image {
                        kind,
                        src,
                        alt: alt.clone(),
                        index: self.doc.images.len() as u64,
                    };
                    self.doc.images.push(img.clone());
                    out.push(Inline::Image(img));
                } else {
                    if (alt.is_empty() || image::web(&src)) && !self.charge(src.len() * 3) {
                        push(out, raw(self.source, n), base);
                        return;
                    }
                    style.href = self.link(&src).or(style.href);
                    out.push(Inline::Text {
                        text: if alt.is_empty() {
                            src.to_string()
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
            Node::Link(l) => {
                if !self.charge_link(&l.url) {
                    push(out, raw(self.source, n), base);
                    return;
                }
                style.href = self.link(&l.url);
            }
            Node::LinkReference(l) => {
                if let Some(&(url, _)) = self.defs.get(l.identifier.as_str()) {
                    if !self.charge_link(url) {
                        push(out, raw(self.source, n), base);
                        return;
                    }
                    style.href = self.link(url);
                }
            }
            Node::Text(t) => {
                let text = t.value.replace("\r\n", "\n");
                static BOLD: LazyLock<Regex> =
                    LazyLock::new(|| Regex::new(r"\*\*([^*\n]+)\*\*").unwrap());
                let mut last = 0;
                if text.contains("**") {
                    let mask = bold_mask(raw(self.source, n), &text);
                    for m in BOLD.captures_iter(&mask) {
                        let whole = m.get(0).unwrap();
                        push(out, &text[last..whole.start()], &style);
                        push(
                            out,
                            &text[m.get(1).unwrap().range()],
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
                let raw = raw(self.source, n);
                let inner = raw.trim_matches('`');
                let mut value = c.value.replace("\r\n", " ").replace(['\r', '\n'], " ");
                if c.value.contains(['\n', '\r']) {
                    // mdast trims literal edge spaces before newline normalization; marked does it after.
                    if inner.starts_with(' ')
                        && inner.ends_with(' ')
                        && !inner.bytes().all(|b| b == b' ')
                    {
                        value = format!(" {value} ");
                    }
                    if value.starts_with(' ')
                        && value.ends_with(' ')
                        && !value.trim_matches(' ').is_empty()
                    {
                        value = value[1..value.len() - 1].to_owned();
                    }
                }
                push(out, &value, &style);
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
    let mut delim = delimiter;
    let mut prefix_tail = prefix;
    while let Some((_, tail)) = prefix_tail.split_once('>') {
        if let Some((_, remaining)) = delim.split_once('>') {
            delim = remaining.strip_prefix([' ', '\t']).unwrap_or(remaining);
        }
        prefix_tail = tail.strip_prefix([' ', '\t']).unwrap_or(tail);
    }
    let indent = delim
        .bytes()
        .take_while(|b| matches!(b, b' ' | b'\t' | b'>'))
        .take(prefix_tail.len())
        .count();
    raw.push('\n');
    raw.push_str(&delim[indent..]);
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

// mdast decodes entities in URLs, whereas marked only unescapes punctuation. Recover the
// destination token from the source; never use an entity-decoded scheme for auto-loading.
fn image_src(raw: &str, decoded: &str, definition: bool) -> Arc<str> {
    if !raw.contains('&') {
        return Arc::from(decoded);
    }
    // Let the same lexer find the real destination (a label or title can itself contain `](`).
    // A non-entity prefix after every ampersand preserves literal and backslash-escaped ampersands.
    let protected = raw.replace('&', "&pimate.");
    if budget::prefix(&protected) != protected.len() {
        return Arc::from("");
    }
    let Ok(mut root) = markdown::to_mdast(&protected, &ParseOptions::gfm()) else {
        return Arc::from("");
    };
    bound_tree(&mut root, &protected, 0);
    let url = match children(&root) {
        [Node::Definition(d)] if definition => Some(&d.url),
        [p @ Node::Paragraph(_)] if !definition => match children(p) {
            [Node::Image(i)] => Some(&i.url),
            _ => None,
        },
        _ => None,
    };
    url.map_or_else(|| Arc::from(""), |s| s.replace("&pimate.", "&").into())
}

// Keep byte offsets in the decoded span, masking only stars originating in escapes/entities.
fn bold_mask(raw: &str, text: &str) -> String {
    let mut stars = Vec::new();
    let mut it = raw.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        match c {
            '\\' if it.peek().is_some_and(|(_, c)| c.is_ascii_punctuation()) => {
                if it.next().unwrap().1 == '*' {
                    stars.push(false);
                }
            }
            '*' => stars.push(true),
            '&' => {
                let tail = &raw[i + 1..];
                if let Some(entity) = tail
                    .as_bytes()
                    .iter()
                    .take(33)
                    .position(|b| *b == b';')
                    .map(|len| &tail[..len])
                {
                    let numeric = entity.strip_prefix('#').and_then(|n| {
                        if let Some(hex) = n.strip_prefix(['x', 'X']) {
                            if hex.len() <= 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                                u32::from_str_radix(hex, 16).ok()
                            } else {
                                None
                            }
                        } else {
                            if n.len() <= 7 && n.bytes().all(|b| b.is_ascii_digit()) {
                                n.parse().ok()
                            } else {
                                None
                            }
                        }
                    });
                    if numeric == Some(42)
                        || markdown::decode_named(entity, true).as_deref() == Some("*")
                    {
                        stars.push(false);
                    }
                }
            }
            _ => {}
        }
    }
    let mut allowed = stars.into_iter();
    text.chars()
        .map(|c| {
            if c == '*' && !allowed.next().unwrap_or(false) {
                '!'
            } else {
                c
            }
        })
        .collect()
}
