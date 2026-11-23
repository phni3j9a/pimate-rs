//! Owned, presentation-neutral Markdown. IDs are unique among sibling blocks of one message.
mod budget;
mod image;
mod parse;
mod table;
pub use image::{ImageKind, classify_image};
pub use parse::parse;
pub use table::{Alignment, Table, cell_width, table_widths};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub src: std::sync::Arc<str>,
    pub alt: String,
    pub kind: ImageKind,
    pub index: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub href: Option<std::sync::Arc<str>>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    Text { text: String, style: Style },
    Image(Image),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    Paragraph(Vec<Inline>),
    Heading {
        level: u8,
        spans: Vec<Inline>,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    Quote(Vec<Block>),
    List {
        start: Option<u32>,
        items: Vec<Block>,
    },
    Item {
        checked: Option<bool>,
        children: Vec<Block>,
    },
    Table(Table),
    Rule,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// Start byte in original UTF-8 Markdown; also the append-stable sibling ID.
    pub id: u64,
    /// Exclusive end byte.
    pub end: u64,
    pub content: Content,
}
fn text(spans: &[Inline]) -> String {
    spans
        .iter()
        .map(|s| match s {
            Inline::Text { text, .. } => text.as_str(),
            Inline::Image(i) => &i.alt,
        })
        .collect()
}
impl Block {
    pub fn plain_text(&self) -> String {
        match &self.content {
            Content::Paragraph(s) | Content::Heading { spans: s, .. } => text(s),
            Content::Code { text, .. } => text.replace('\t', "  "),
            Content::Quote(c)
            | Content::List { items: c, .. }
            | Content::Item { children: c, .. } => c
                .iter()
                .map(Block::plain_text)
                .collect::<Vec<_>>()
                .join("\n"),
            Content::Rule => String::new(),
            Content::Table(t) => std::iter::once(&t.header)
                .chain(&t.rows)
                .map(|r| r.iter().map(|s| text(s)).collect::<Vec<_>>().join("\t"))
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Document {
    pub blocks: Vec<Block>,
    pub images: Vec<Image>,
    pub links: Vec<String>,
}
#[derive(Debug, PartialEq, Eq)]
pub struct Changes {
    pub upsert: Vec<Block>,
    pub removed: Vec<u64>,
}
impl Document {
    /// Apply removals/upserts to top-level blocks keyed (and ordered) by id.
    pub fn changes_since(&self, previous: &Self) -> Changes {
        let mut old = previous.blocks.iter().peekable();
        let mut upsert = Vec::new();
        let mut removed = Vec::new();
        for b in &self.blocks {
            while let Some(obsolete) = old.next_if(|o| o.id < b.id) {
                removed.push(obsolete.id);
            }
            if old.next_if(|o| o.id == b.id) != Some(b) {
                upsert.push(b.clone());
            }
        }
        removed.extend(old.map(|b| b.id));
        Changes { upsert, removed }
    }
}
