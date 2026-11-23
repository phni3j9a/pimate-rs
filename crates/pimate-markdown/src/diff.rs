use crate::{Block, Content, Inline};
use std::{collections::HashMap, sync::Arc};
/// Only valid for one diff: both immutable documents outlive every allocation-address key.
#[derive(Default)]
pub(super) struct Equality {
    images: HashMap<(usize, usize), bool>,
}
impl Equality {
    fn spans(&mut self, a: &[Inline], b: &[Inline]) -> bool {
        a.len() == b.len()
            && a.iter().zip(b).all(|(a, b)| match (a, b) {
                (Inline::Image(a), Inline::Image(b)) => {
                    a.index == b.index
                        && a.kind == b.kind
                        && a.alt == b.alt
                        && (Arc::ptr_eq(&a.src, &b.src)
                            || *self
                                .images
                                .entry((a.src.as_ptr() as usize, b.src.as_ptr() as usize))
                                .or_insert_with(|| a.src == b.src))
                }
                _ => a == b,
            })
    }
    fn blocks(&mut self, a: &[Block], b: &[Block]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| self.block(a, b))
    }
    pub fn block(&mut self, a: &Block, b: &Block) -> bool {
        if a.id != b.id || a.end != b.end {
            return false;
        }
        match (&a.content, &b.content) {
            (Content::Paragraph(a), Content::Paragraph(b)) => self.spans(a, b),
            (Content::Heading { level: x, spans: a }, Content::Heading { level: y, spans: b }) => {
                x == y && self.spans(a, b)
            }
            (Content::Quote(a), Content::Quote(b)) => self.blocks(a, b),
            (Content::List { start: x, items: a }, Content::List { start: y, items: b }) => {
                x == y && self.blocks(a, b)
            }
            (
                Content::Item {
                    checked: x,
                    children: a,
                },
                Content::Item {
                    checked: y,
                    children: b,
                },
            ) => x == y && self.blocks(a, b),
            (Content::Table(a), Content::Table(b)) => {
                a.markdown == b.markdown
                    && a.align == b.align
                    && a.rows.len() == b.rows.len()
                    && std::iter::once(&a.header)
                        .chain(&a.rows)
                        .zip(std::iter::once(&b.header).chain(&b.rows))
                        .all(|(a, b)| {
                            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| self.spans(a, b))
                        })
            }
            _ => a == b,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_uri_compared_once() {
        let uri = format!("data:image/png;base64,{}", "AAAA".repeat(65536));
        let md = format!("{}\n[r]: {uri}", "![a][r]\n\n".repeat(512));
        let d = crate::parse(&md, false);
        let next = crate::parse(&(md + "\n\nend"), false);
        let mut eq = Equality::default();
        assert!(eq.blocks(&d.blocks, &next.blocks[..512]));
        assert_eq!(eq.images.len(), 1);
        assert_eq!(next.changes_since(&d).upsert.len(), 1);
    }
}
