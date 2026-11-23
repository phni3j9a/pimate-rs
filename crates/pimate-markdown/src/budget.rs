//! Reject expensive syntax *before* building mdast. Keep complete preceding paragraphs.
const BYTES: usize = 1024 * 1024; // One maximum raster data URI plus the conversation.
const TOKENS: usize = 32768;
pub(crate) const DEPTH: usize = 32;
pub(crate) const CELLS: usize = 8192;

pub(crate) fn prefix(source: &str) -> usize {
    let mut offset = 0;
    let mut paragraph = 0;
    let mut inline = 0;
    let mut tokens = 0;
    let (mut columns, mut rows) = (0, 0usize);
    // Include CR-only lines, but never treat the LF of CRLF as a blank paragraph.
    while offset < source.len() {
        let rest = &source[offset..];
        let window = &rest.as_bytes()[..rest.len().min(BYTES - offset + 1)];
        let len = window
            .iter()
            .position(|b| matches!(b, b'\r' | b'\n'))
            .unwrap_or(window.len());
        if offset + len > BYTES {
            return paragraph;
        }
        let line = &rest[..len];
        let newline = if rest[len..].starts_with("\r\n") {
            2
        } else {
            usize::from(len < rest.len())
        };
        offset += len + newline;
        if offset > BYTES {
            return paragraph;
        }
        if line.trim_matches([' ', '\t']).is_empty() {
            paragraph = offset;
            inline = 0;
        }
        // The delimiter row also bounds the width of the header (including quoted/list tables).
        let delimiter = line.trim_start_matches([' ', '\t', '>']);
        if delimiter.contains('-')
            && delimiter.contains('|')
            && delimiter
                .bytes()
                .all(|b| matches!(b, b'-' | b':' | b'|' | b' ' | b'\t'))
        {
            columns = delimiter.trim().trim_matches('|').split('|').count();
            rows = 1;
        } else if line.trim().is_empty() {
            columns = 0;
        } else {
            rows += 1;
        }
        if rows.saturating_mul(columns) > CELLS {
            return paragraph;
        }
        // A list/quote ancestor needs a marker or indentation on its opening line.
        let prefix = line.bytes().take_while(|b| {
            matches!(
                b,
                b' ' | b'\t' | b'>' | b'-' | b'*' | b'+' | b'0'..=b'9' | b'.' | b')'
            )
        });
        let columns = prefix
            .map(|b| if b == b'\t' { 4 } else { 1 })
            .sum::<usize>();
        inline += line
            .bytes()
            .filter(|b| matches!(b, b'[' | b'*' | b'_' | b'~' | b'('))
            .count();
        tokens += 1 + line.bytes().filter(u8::is_ascii_punctuation).count();
        if columns > DEPTH || inline > DEPTH * 2 || tokens > TOKENS {
            return paragraph.min(offset - len - newline);
        }
    }
    offset
}
