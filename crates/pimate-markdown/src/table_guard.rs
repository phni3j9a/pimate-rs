//! pulldown pads sparse rows during its first (block) pass. Size wide candidate tables with
//! a three-column probe first, so many oversized tables cannot allocate all those empty cells.
use pulldown_cmark::{Event, Options, Parser, Tag};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    ops::Range,
};

pub(super) const CELLS: usize = 8192;
pub(super) fn rows(s: &str) -> usize {
    s.split_inclusive(['\r', '\n'])
        .filter(|l| *l != "\n")
        .count()
        .saturating_sub(1)
}
struct Candidate {
    header: Range<usize>,
    delimiter: Range<usize>,
    header_pipes: usize,
    first_pipe: usize,
    width: usize,
}
impl Candidate {
    fn narrow(&self, bytes: &mut [u8]) {
        for b in &mut bytes[self.header.clone()] {
            if *b == b'|' {
                *b = b'h';
            }
        }
        bytes[self.delimiter.clone()].fill(b'-');
    }
}
// Merge exactly width-3 internal separators; the first and last pipes (including optional
// borders) stay put. Ignore backslash-escaped pipes as in the GFM first pass.
fn pipes(line: &str, offset: usize) -> impl Iterator<Item = usize> + '_ {
    line.match_indices('|')
        .filter(|(i, _)| *i == 0 || line.as_bytes()[i - 1] != b'\\')
        .map(move |(i, _)| offset + i)
}
fn middle(line: &str, offset: usize, width: usize) -> Option<Range<usize>> {
    let mut pipes = pipes(line, offset);
    let start = pipes.next()? + 1;
    Some(start..pipes.nth(width - 3)?)
}
pub(super) fn prepare(source: &str) -> (Cow<'_, str>, HashSet<usize>) {
    let none = || (Cow::Borrowed(source), HashSet::new());
    if !source.contains('|') {
        return none();
    }
    let line_bound = source
        .bytes()
        .filter(|b| matches!(b, b'\r' | b'\n'))
        .count()
        + 1;
    let mut candidates = HashMap::new();
    let mut header = "";
    let mut offset = 0;
    let mut previous = 0;
    for line in source.split_inclusive(['\r', '\n']) {
        let pos = offset;
        offset += line.len();
        if line == "\n" && pos > 0 && source.as_bytes()[pos - 1] == b'\r' {
            continue;
        }
        let d = line.trim_start_matches(['>', ' ', '\t']).trim_end();
        let width = d.trim_matches('|').bytes().filter(|b| *b == b'|').count() + 1;
        if width > 3
            && width.saturating_mul(line_bound) > CELLS
            && d.contains('-')
            && d.bytes()
                .all(|b| matches!(b, b'-' | b':' | b'|' | b' ' | b'\t'))
            && let (Some(h), Some(d)) = (middle(header, previous, width), middle(line, pos, width))
        {
            candidates.insert(
                pos,
                Candidate {
                    header: h.clone(),
                    delimiter: d,
                    header_pipes: pipes(header, previous).count(),
                    first_pipe: h.start - 1,
                    width,
                },
            );
        }
        previous = pos;
        header = line;
    }
    if candidates.is_empty() {
        return none();
    }
    let mut probe = source.as_bytes().to_vec();
    for c in candidates.values() {
        c.narrow(&mut probe);
    }
    let probe = String::from_utf8(probe).unwrap();
    let mut big = HashSet::new();
    let mut final_bytes = None;
    let mut events = Parser::new_ext(&probe, Options::ENABLE_TABLES).into_offset_iter();
    while let Some((event, r)) = events.next() {
        if !matches!(event, Event::Start(Tag::Table(_))) {
            continue;
        }
        let Some((Event::Start(Tag::TableHead), head)) = events.next() else {
            continue;
        };
        let Some(c) = candidates.get(&head.end) else {
            continue;
        };
        let header = source[head.clone()].trim();
        if head.start > c.first_pipe
            || c.header_pipes + 1
                - usize::from(header.starts_with('|'))
                - usize::from(header.ends_with('|'))
                != c.width
        {
            continue;
        }
        if rows(&source[r.clone()]).saturating_mul(c.width) > CELLS {
            big.insert(r.start);
            c.narrow(final_bytes.get_or_insert_with(|| source.as_bytes().to_vec()));
        }
    }
    match final_bytes {
        None => none(),
        Some(b) => (Cow::Owned(String::from_utf8(b).unwrap()), big),
    }
}
