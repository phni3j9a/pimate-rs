//! Parse LF as marked does, while exposing byte ranges in the original CRLF source.
use std::borrow::Cow;
pub(super) struct Input<'a> {
    pub text: Cow<'a, str>,
    removed: Vec<usize>,
}
impl Input<'_> {
    pub fn original(&self, p: usize) -> usize {
        p + self.removed.partition_point(|i| *i <= p)
    }
    pub fn end(&self, p: usize) -> usize {
        p + self.removed.partition_point(|i| *i < p)
    }
}
pub(super) fn prepare(source: &str) -> Input<'_> {
    let removed: Vec<_> = source
        .match_indices("\r\n")
        .enumerate()
        .map(|(n, (i, _))| i - n)
        .collect();
    let text = if removed.is_empty() {
        Cow::Borrowed(source)
    } else {
        Cow::Owned(source.replace("\r\n", "\n"))
    };
    Input { text, removed }
}
