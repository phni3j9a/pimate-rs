use super::*;
use pulldown_cmark::{CodeBlockKind, Event, LinkType, Options, Parser, Tag};
use regex::Regex;
use std::{
    collections::HashMap,
    iter::Peekable,
    ops::Range,
    sync::{Arc, LazyLock},
};

const DEPTH: usize = 32;
use table_guard::CELLS;
const OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS);
type Events<'a> = Peekable<Box<dyn Iterator<Item = (Event<'a>, Range<usize>)> + 'a>>;

pub fn parse(source: &str, file_images: bool) -> Document {
    let input = newlines::prepare(source);
    let (prepared, oversized) = table_guard::bound_sparse_tables(&input.text);
    let oversized = oversized.into_iter().map(|p| input.original(p)).collect();
    let first = Parser::new_ext(&prepared, OPTIONS);
    if first.reference_definitions().iter().next().is_none() {
        return render(
            source,
            file_images,
            first,
            Default::default(),
            &oversized,
            &input,
        );
    }
    let defs = first.reference_definitions().clone();
    // Keep reference destinations out of the event stream: the parser's reference expansion
    // budget would otherwise discard later uses of a large shared data URI. Preserve offsets.
    let mut bytes = prepared.as_bytes().to_vec();
    for (_, def) in defs.iter() {
        if let Some(colon) = definition_colon(&prepared[def.span.clone()]) {
            let value = &mut bytes[def.span.start + colon + 1..def.span.end];
            if let Some((first, rest)) = value.split_first_mut() {
                *first = b'x';
                rest.fill(b' ');
            }
        }
    }
    let compact = String::from_utf8(bytes).expect("replaced complete UTF-8 ranges");
    render(
        source,
        file_images,
        Parser::new_ext(&compact, OPTIONS),
        defs,
        &oversized,
        &input,
    )
}
fn render(
    source: &str,
    file_images: bool,
    parser: Parser,
    defs: pulldown_cmark::RefDefs,
    oversized: &std::collections::HashSet<usize>,
    input: &newlines::Input,
) -> Document {
    let iter = parser.into_offset_iter().map(|(e, r)| {
        (
            e,
            input.original(r.start)..if r.is_empty() {
                input.original(r.end)
            } else {
                input.end(r.end)
            },
        )
    });
    let mut events: Events = (Box::new(iter) as Box<dyn Iterator<Item = _>>).peekable();
    let mut render = Render {
        input,
        source,
        oversized,
        defs,
        file_images,
        doc: Document::default(),
        images: HashMap::new(),
        links: HashMap::new(),
        containers: vec![],
        expansion: 4 * 1024 * 1024,
    };
    render.doc.blocks = render.blocks(&mut events, 0, source.len(), &mut None);
    render.doc
}
fn definition_colon(s: &str) -> Option<usize> {
    let mut it = s.char_indices();
    while let Some((i, c)) = it.next() {
        if c == '\\' {
            it.next();
        } else if c == ']' && s[i + 1..].starts_with(':') {
            return Some(i + 1);
        }
    }
    None
}
struct Render<'a> {
    input: &'a newlines::Input<'a>,
    source: &'a str,
    oversized: &'a std::collections::HashSet<usize>,
    defs: pulldown_cmark::RefDefs<'a>,
    file_images: bool,
    doc: Document,
    images: HashMap<usize, (Arc<str>, Option<ImageKind>)>,
    links: HashMap<usize, Arc<str>>,
    expansion: usize,
    containers: Vec<Container>,
}
enum Container {
    Quote,
    Item {
        start: usize,
        content: usize,
        columns: usize,
    },
}
fn advance(col: usize, b: u8) -> usize {
    if b == b'\t' {
        col + 4 - col % 4
    } else {
        col + 1
    }
}
fn column(s: &str, p: usize) -> usize {
    s[..p]
        .rsplit(['\r', '\n'])
        .next()
        .unwrap_or("")
        .bytes()
        .fold(0, advance)
}
fn list_prefix(source: &str, start: usize, raw: &str, parents: &[Container]) -> Container {
    let mark = start + raw.len() - raw.trim_start_matches([' ', '\t']).len();
    let marker_end = mark
        + source[mark..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count()
        + 1;
    let after = &source[marker_end..];
    let padding = after
        .bytes()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .count();
    let col = column(source, marker_end);
    let padded = after[..padding].bytes().fold(col, advance);
    let base = table_line_start(source, mark, parents);
    let (content, content_col) = if (1..=4).contains(&(padded - col)) {
        (marker_end + padding, padded)
    } else {
        (marker_end + usize::from(padding != 0), col + 1)
    };
    Container::Item {
        start: mark,
        content,
        columns: content_col - column(source, base),
    }
}
// Strip only ancestor prefixes. Spaces after the consumed quote/list margin belong to the table.
fn table_line_start(source: &str, at: usize, containers: &[Container]) -> usize {
    let mut p = source[..at].rfind(['\r', '\n']).map_or(0, |n| n + 1);
    for c in containers {
        match *c {
            Container::Quote => {
                if let Some(i) = source[p..at].find('>') {
                    p += i + 1;
                    if source[p..at].starts_with([' ', '\t']) {
                        p += 1;
                    }
                }
            }
            Container::Item {
                start,
                content,
                columns,
            } => {
                if (p..at).contains(&start) {
                    p = content.min(at);
                } else {
                    let col = column(source, p);
                    let mut current = col;
                    for b in source[p..at].bytes() {
                        if current - col >= columns || !matches!(b, b' ' | b'\t') {
                            break;
                        }
                        current = advance(current, b);
                        p += 1;
                    }
                }
            }
        }
    }
    p
}
fn skip(e: &mut Events) {
    let mut depth = 1;
    for (event, _) in e.by_ref() {
        match event {
            Event::Start(_) => depth += 1,
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
    }
}
fn block_tag(t: &Tag) -> bool {
    matches!(
        t,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote(_)
            | Tag::List(_)
            | Tag::Item
            | Tag::CodeBlock(_)
            | Tag::Table(_)
            | Tag::HtmlBlock
    )
}
fn trim_eol(s: &str) -> &str {
    s.trim_end_matches(['\r', '\n'])
}
fn literal(s: &str) -> Content {
    Content::Paragraph(vec![Inline::Text {
        text: s.into(),
        style: Style::default(),
    }])
}
impl Render<'_> {
    fn blocks(
        &mut self,
        e: &mut Events,
        depth: usize,
        parent_end: usize,
        checked: &mut Option<bool>,
    ) -> Vec<Block> {
        let mut out = Vec::new();
        while let Some((ev, r)) = e.peek().cloned() {
            let raw = &self.source[r.clone()];
            let mut start = r.start;
            let mut end = start + trim_eol(raw).len();
            let content = match ev {
                Event::End(_) => {
                    e.next();
                    break;
                }
                Event::TaskListMarker(v) => {
                    *checked = Some(v);
                    e.next();
                    continue;
                }
                Event::Rule => {
                    e.next();
                    Content::Rule
                }
                Event::Start(t) if block_tag(&t) => {
                    e.next();
                    if depth >= DEPTH {
                        skip(e);
                        literal(trim_eol(raw))
                    } else {
                        match t {
                            Tag::HtmlBlock => {
                                let mut text = String::new();
                                for (ev, _) in e.by_ref() {
                                    match ev {
                                        Event::Html(v) => text.push_str(&v),
                                        Event::End(_) => break,
                                        _ => {}
                                    }
                                }
                                literal(text.trim())
                            }
                            Tag::Paragraph | Tag::Heading { .. } => {
                                if let Some((Event::TaskListMarker(v), _)) = e.peek() {
                                    *checked = Some(*v);
                                    e.next();
                                }
                                let spans = self.spans(e, false).0;
                                if let Tag::Heading { level, .. } = t {
                                    Content::Heading {
                                        level: level as u8,
                                        spans,
                                    }
                                } else {
                                    Content::Paragraph(spans)
                                }
                            }
                            Tag::BlockQuote(_) => {
                                self.containers.push(Container::Quote);
                                let c = self.blocks(e, depth + 1, end, &mut None);
                                self.containers.pop();
                                Content::Quote(c)
                            }
                            Tag::List(num) => Content::List {
                                start: num.map(|n| n as u32),
                                items: self.blocks(e, depth + 1, end, &mut None),
                            },
                            Tag::Item => {
                                self.containers.push(list_prefix(
                                    self.source,
                                    start,
                                    raw,
                                    &self.containers,
                                ));
                                let mut checked = None;
                                let children = self.blocks(e, depth + 1, end, &mut checked);
                                self.containers.pop();
                                Content::Item { checked, children }
                            }
                            Tag::CodeBlock(kind) => {
                                let mut text = String::new();
                                for (ev, _) in e.by_ref() {
                                    match ev {
                                        Event::Text(v) => text.push_str(&v),
                                        Event::End(_) => break,
                                        _ => {}
                                    }
                                }
                                if text.ends_with('\n') {
                                    text.pop();
                                }
                                let language = match kind {
                                    CodeBlockKind::Fenced(info) => {
                                        info.split_whitespace().next().map(str::to_owned)
                                    }
                                    _ => None,
                                };
                                Content::Code { language, text }
                            }
                            Tag::Table(align) => {
                                if self.oversized.contains(&start)
                                    || table_guard::rows(raw).saturating_mul(align.len()) > CELLS
                                {
                                    skip(e);
                                    literal(raw)
                                } else {
                                    start = table_line_start(self.source, start, &self.containers);
                                    Content::Table(self.table(e, r.clone(), parent_end, align))
                                }
                            }
                            _ => unreachable!(),
                        }
                    }
                }
                _ => {
                    if matches!(&ev, Event::Text(_)) && self.source[..start].ends_with('\\') {
                        start -= 1;
                    }
                    let (spans, inline_end) = self.spans(e, true);
                    end = inline_end;
                    Content::Paragraph(spans)
                }
            };
            out.push(Block {
                id: start as u64,
                end: end as u64,
                content,
            });
        }
        out
    }
    fn table(
        &mut self,
        e: &mut Events,
        r: Range<usize>,
        parent_end: usize,
        align: Vec<pulldown_cmark::Alignment>,
    ) -> Table {
        let mut header = Vec::new();
        let mut rows = Vec::new();
        let mut markdown = String::new();
        while let Some((ev, row)) = e.next() {
            if matches!(ev, Event::End(_)) {
                break;
            }
            let head = matches!(ev, Event::Start(Tag::TableHead));
            let mut cells = Vec::new();
            while let Some((ev, cell)) = e.next() {
                if matches!(ev, Event::End(_)) {
                    break;
                }
                let spans = self.spans(e, false).0;
                if cell.start != row.end || !spans.is_empty() {
                    cells.push(spans);
                }
            }
            let start = table_line_start(self.source, row.start, &self.containers);
            if head {
                header = cells;
                markdown.push_str(trim_eol(&self.source[start..row.end]));
                markdown.push('\n');
                let delimiter = self.source[row.end..r.end]
                    .split(['\r', '\n'])
                    .next()
                    .unwrap_or("");
                let first = row.end + delimiter.len()
                    - delimiter.trim_start_matches(['>', ' ', '\t']).len();
                let start = table_line_start(self.source, first, &self.containers);
                markdown.push_str(&self.source[start..row.end + delimiter.len()]);
            } else {
                rows.push(cells);
                markdown.push('\n');
                markdown.push_str(trim_eol(&self.source[start..row.end]));
            }
        }
        if r.end <= parent_end
            && self.source[r.clone()].ends_with(['\r', '\n'])
            && !self.source[r.end..].starts_with(['\r', '\n'])
        {
            markdown.push('\n');
        }
        Table {
            align: align
                .into_iter()
                .map(|a| match a {
                    pulldown_cmark::Alignment::None => Alignment::Default,
                    pulldown_cmark::Alignment::Left => Alignment::Left,
                    pulldown_cmark::Alignment::Center => Alignment::Center,
                    pulldown_cmark::Alignment::Right => Alignment::Right,
                })
                .collect(),
            header,
            rows,
            markdown,
        }
    }
    fn spans(&mut self, e: &mut Events, tight: bool) -> (Vec<Inline>, usize) {
        let mut end = 0;
        let mut out = Vec::new();
        let mut link_depth = None;
        let mut styles = vec![Style::default()];
        while let Some((ev, r)) = e.peek().cloned() {
            if tight
                && styles.len() == 1
                && (matches!(ev, Event::End(_) | Event::Rule)
                    || matches!(&ev,Event::Start(t) if block_tag(t)))
            {
                break;
            }
            e.next();
            end = r.end;
            let mut style = styles.last().unwrap().clone();
            let raw = &self.source[r.clone()];
            match ev {
                Event::Start(_) if styles.len() >= DEPTH => {
                    skip(e);
                    push(&mut out, raw, &style)
                }
                Event::Start(Tag::Image {
                    dest_url,
                    id,
                    link_type,
                    ..
                }) => {
                    let mut alt = String::new();
                    let mut depth = 1;
                    for (ev, _) in e.by_ref() {
                        match ev {
                            Event::Start(_) => depth += 1,
                            Event::End(_) => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            Event::Text(s) | Event::Code(s) => alt.push_str(&s),
                            Event::SoftBreak | Event::HardBreak => alt.push('\n'),
                            _ => {}
                        }
                    }
                    let reference = (link_type != LinkType::Inline)
                        .then(|| self.defs.get(&id))
                        .flatten();
                    let (src, kind) = if let Some(d) = reference {
                        self.images
                            .entry(d.span.start)
                            .or_insert_with(|| {
                                let s = image_src(
                                    &self.source[self.input.original(d.span.start)
                                        ..self.input.end(d.span.end)],
                                    &d.dest,
                                    true,
                                );
                                let k = classify_image(&s);
                                (s, k)
                            })
                            .clone()
                    } else {
                        let s = image_src(raw, &dest_url, false);
                        let k = classify_image(&s);
                        (s, k)
                    };
                    if let Some(kind) = kind.filter(|k| *k != ImageKind::File || self.file_images) {
                        let image = Image {
                            src,
                            kind,
                            alt,
                            index: self.doc.images.len() as u64,
                        };
                        self.doc.images.push(image.clone());
                        out.push(Inline::Image(image));
                    } else if (!alt.is_empty() && !image::web(&src))
                        || reference.is_none()
                        || self.charge(src.len() * 3)
                    {
                        if image::web(&src) {
                            style.href = self.link(&src);
                        }
                        push(&mut out, if alt.is_empty() { &src } else { &alt }, &style);
                    } else {
                        push(&mut out, raw, &style)
                    }
                }
                Event::Start(t) => {
                    match t {
                        Tag::Emphasis => style.italic = true,
                        Tag::Strong => style.bold = true,
                        Tag::Strikethrough => style.strike = true,
                        Tag::Link {
                            dest_url,
                            id,
                            link_type,
                            ..
                        } => {
                            let reference = matches!(
                                link_type,
                                LinkType::Reference | LinkType::Collapsed | LinkType::Shortcut
                            )
                            .then(|| self.defs.get(&id))
                            .flatten();
                            let url = if let Some(d) = reference {
                                self.links
                                    .entry(d.span.start)
                                    .or_insert_with(|| Arc::from(d.dest.as_ref()))
                                    .clone()
                            } else {
                                Arc::from(dest_url.as_ref())
                            };
                            if reference.is_some()
                                && image::web(&url)
                                && !self.charge(url.len() * 2)
                            {
                                skip(e);
                                push(&mut out, raw, &style);
                                continue;
                            }
                            link_depth = Some(styles.len() + 1);
                            style.href = self.link(&url);
                        }
                        _ => {}
                    }
                    styles.push(style);
                }
                Event::End(_) => {
                    if styles.len() == 1 {
                        break;
                    }
                    if link_depth == Some(styles.len()) {
                        link_depth = None;
                    }
                    styles.pop();
                }
                Event::Text(v) => {
                    let mut text = v.into_string();
                    let mut mask = String::new();
                    mask_part(&mut mask, &text, raw, self.source, r.start);
                    while let Some((Event::Text(_), _)) = e.peek() {
                        let Some((Event::Text(v), r)) = e.next() else {
                            unreachable!()
                        };
                        mask_part(&mut mask, &v, &self.source[r.clone()], self.source, r.start);
                        text.push_str(&v);
                        end = r.end;
                    }
                    self.plain(&mut out, &text, &mask, &style, link_depth.is_none());
                }
                Event::Code(v) => {
                    style.code = true;
                    push(&mut out, &v, &style)
                }
                Event::SoftBreak | Event::HardBreak => push(&mut out, "\n", &style),
                Event::Html(v) | Event::InlineHtml(v) => push(&mut out, &v, &style),
                _ => {}
            }
        }
        (out, end)
    }
    fn charge(&mut self, n: usize) -> bool {
        if n > self.expansion {
            return false;
        }
        self.expansion -= n;
        true
    }
    fn link(&mut self, url: &str) -> Option<Arc<str>> {
        image::web(url).then(|| {
            self.doc.links.push(url.into());
            Arc::from(url)
        })
    }
    fn plain(&mut self, out: &mut Vec<Inline>, s: &str, mask: &str, style: &Style, autolink: bool) {
        static BOLD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*([^*\n]+)\*\*").unwrap());
        let mut last = 0;
        for m in BOLD.captures_iter(mask) {
            let all = m.get(0).unwrap();
            self.urls(out, &s[last..all.start()], style, autolink);
            self.urls(
                out,
                &s[m.get(1).unwrap().range()],
                &Style {
                    bold: true,
                    ..style.clone()
                },
                autolink,
            );
            last = all.end();
        }
        self.urls(out, &s[last..], style, autolink);
    }
    fn urls(&mut self, out: &mut Vec<Inline>, s: &str, style: &Style, autolink: bool) {
        static FIND: LazyLock<linkify::LinkFinder> = LazyLock::new(|| {
            let mut finder = linkify::LinkFinder::new();
            finder.url_must_have_scheme(false);
            finder
        });
        let mut last = 0;
        if autolink && style.href.is_none() {
            for link in FIND.links(s) {
                let uri = link.as_str();
                let url = if uri.starts_with("www.") {
                    format!("http://{uri}")
                } else if image::web(uri) {
                    uri.into()
                } else {
                    continue;
                };
                push(out, &s[last..link.start()], style);
                push(
                    out,
                    uri,
                    &Style {
                        href: self.link(&url),
                        ..style.clone()
                    },
                );
                last = link.end();
            }
        }
        push(out, &s[last..], style);
    }
}
fn push(out: &mut Vec<Inline>, s: &str, style: &Style) {
    if !s.is_empty() {
        out.push(Inline::Text {
            text: s.into(),
            style: style.clone(),
        });
    }
}
fn mask_part(mask: &mut String, v: &str, raw: &str, source: &str, start: usize) {
    if raw != v {
        mask.push_str(&v.replace('*', "!"));
    } else if v.starts_with('*')
        && source[..start]
            .bytes()
            .rev()
            .take_while(|b| *b == b'\\')
            .count()
            % 2
            == 1
    {
        mask.push('!');
        mask.push_str(&v[1..]);
    } else {
        mask.push_str(v);
    }
}
fn image_src(raw: &str, decoded: &str, def: bool) -> Arc<str> {
    if !raw.contains('&') {
        return decoded.into();
    }
    let protected = raw.replace('&', "&pimate.");
    let parser = Parser::new(&protected);
    let url = if def {
        parser
            .reference_definitions()
            .iter()
            .next()
            .map(|(_, d)| d.dest.to_string())
    } else {
        parser.into_iter().find_map(|e| {
            if let Event::Start(Tag::Image { dest_url, .. }) = e {
                Some(dest_url.into_string())
            } else {
                None
            }
        })
    };
    url.unwrap_or_default().replace("&pimate.", "&").into()
}
