//! Adapt parser input without losing original byte ranges. Only structural events from the
//! first pass authorize table edits (never rewrite a look-alike inside code or HTML).
use super::table_guard;
use pulldown_cmark::{Event, Parser, Tag};
use std::{borrow::Cow, ops::Range};

pub(super) struct Input<'a> {
    pub text: Cow<'a, str>,
    pub masked: Vec<usize>,
    edits: Vec<(usize, isize)>,
    removed_cr: Vec<usize>,
}
impl Input<'_> {
    pub fn original(&self, p: usize) -> usize {
        let i = self.edits.partition_point(|(pos, _)| *pos <= p);
        if i == 0 {
            p
        } else {
            p.saturating_add_signed(self.edits[i - 1].1)
        }
    }
    pub fn end(&self, p: usize) -> usize {
        self.original(p) - usize::from(self.removed_cr.binary_search(&p).is_ok())
    }
}
fn valid(d: &str) -> bool {
    let d = d.strip_prefix('|').unwrap_or(d);
    d.strip_suffix('|').unwrap_or(d).split('|').all(|s| {
        let s = s.trim();
        let s = s.strip_prefix(':').unwrap_or(s);
        let s = s.strip_suffix(':').unwrap_or(s);
        !s.is_empty() && s.bytes().all(|b| b == b'-')
    })
}
pub(super) fn prepare(source: &str) -> Input<'_> {
    let mut removed_cr = Vec::new();
    let mut edits = Vec::<(Range<usize>, &str)>::new();
    let mut masked = Vec::new();
    let mut candidates = Vec::new();
    let mut prev = None;
    let mut pos = 0;
    for line in source.split_inclusive(['\r', '\n']) {
        let start = pos;
        pos += line.len();
        if line == "\n" && start > 0 && source.as_bytes()[start - 1] == b'\r' {
            continue;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        let d = line.trim_start_matches(['>', ' ', '\t']).trim_end();
        if (d.contains('|') || d.contains(':'))
            && d.contains('-')
            && d.bytes()
                .all(|b| matches!(b, b'|' | b'-' | b':' | b' ' | b'\t'))
            && let Some((h, header)) = prev
        {
            let good = valid(d);
            let cell = d.trim_matches('|');
            if !good || (!cell.contains('|') && (!header_has_pipe(header) || !d.contains('|'))) {
                candidates.push((
                    h,
                    start,
                    good,
                    !header_has_pipe(header),
                    (!d.contains('|')).then_some(start + line.find(['-', ':']).unwrap_or(0)),
                ));
            }
        }
        prev = Some((start, line));
    }
    if !candidates.is_empty() {
        let (narrow, _) = table_guard::prepare(source);
        let mut candidates = candidates.into_iter().peekable();
        let mut paragraph = false;
        let mut table = false;
        for (ev, r) in Parser::new_ext(&narrow, super::parse::OPTIONS).into_offset_iter() {
            match ev {
                Event::Start(Tag::Paragraph) => paragraph = true,
                Event::End(pulldown_cmark::TagEnd::Paragraph) => paragraph = false,
                Event::Start(Tag::Table(_)) => table = true,
                Event::End(pulldown_cmark::TagEnd::Table) => table = false,
                _ => {}
            }
            while let Some(&(h, d, good, header_pipe, delimiter_pipe)) = candidates.peek() {
                if good
                    && paragraph
                    && matches!(
                        ev,
                        Event::Text(_)
                            | Event::Code(_)
                            | Event::Start(
                                Tag::Strong
                                    | Tag::Emphasis
                                    | Tag::Strikethrough
                                    | Tag::Image { .. }
                                    | Tag::Link { .. }
                            )
                    )
                    && r.start >= h
                    && r.start < d
                {
                    let head = &source[h..r.start];
                    if !head.contains(['\r', '\n']) {
                        if header_pipe {
                            let at = r.start - usize::from(source[..r.start].ends_with('\\'));
                            edits.push((at..at, "|"));
                        }
                        if let Some(at) = delimiter_pipe {
                            edits.push((at..at, "|"));
                        }
                    }
                    candidates.next();
                    continue;
                }
                if !good && table && matches!(ev, Event::Start(Tag::TableHead)) && r.end == d {
                    let col = d + source[d..].find(['-', ':', '|']).expect("delimiter");
                    masked.push(col);
                    edits.push((col..col + 1, "!"));
                    candidates.next();
                    continue;
                }
                if r.start > d {
                    candidates.next();
                    continue;
                }
                break;
            }
        }
    }
    for (i, _) in source.match_indices("\r\n") {
        edits.push((i..i + 1, ""));
    }
    edits.sort_by_key(|(r, _)| (r.start, r.end));
    if edits.is_empty() {
        return Input {
            text: Cow::Borrowed(source),
            edits: vec![],
            masked,
            removed_cr,
        };
    }
    let mut text = String::new();
    let mut map = Vec::new();
    let mut old = 0;
    for (r, v) in edits {
        text.push_str(&source[old..r.start]);
        text.push_str(v);
        old = r.end;
        if v.is_empty() {
            removed_cr.push(text.len());
        }
        map.push((text.len(), old as isize - text.len() as isize));
    }
    text.push_str(&source[old..]);
    Input {
        text: Cow::Owned(text),
        edits: map,
        removed_cr,
        masked,
    }
}
fn header_has_pipe(line: &str) -> bool {
    line.match_indices('|')
        .any(|(i, _)| i == 0 || line.as_bytes()[i - 1] != b'\\')
}
