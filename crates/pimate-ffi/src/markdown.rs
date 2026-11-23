use pimate_markdown::{
    Alignment, Block, Changes, Content, Image as MarkdownImage, ImageKind, Inline, Style, Table,
};
use std::sync::Arc;
type SharedString = Arc<str>;
// SAFETY: metadata and every wire operation delegate to UniFFI's String converter.
// Only the owned Rust representation differs (shared Markdown reference definitions).
use uniffi::{FfiConverter, FfiConverterArc};
type Tag = crate::UniFfiTag;
unsafe impl FfiConverterArc<Tag> for str {
    type FfiType = <String as FfiConverter<Tag>>::FfiType;
    const TYPE_ID_META: uniffi::MetadataBuffer = <String as FfiConverter<Tag>>::TYPE_ID_META;
    fn lower(s: Arc<str>) -> Self::FfiType {
        <String as FfiConverter<Tag>>::lower(s.to_string())
    }
    fn try_lift(v: Self::FfiType) -> uniffi::Result<Arc<str>> {
        Ok(<String as FfiConverter<Tag>>::try_lift(v)?.into())
    }
    fn write(s: Arc<str>, buf: &mut Vec<u8>) {
        <String as FfiConverter<Tag>>::write(s.to_string(), buf)
    }
    fn try_read(buf: &mut &[u8]) -> uniffi::Result<Arc<str>> {
        Ok(<String as FfiConverter<Tag>>::try_read(buf)?.into())
    }
}
#[uniffi::remote(Record)]
pub struct MarkdownImage {
    pub src: SharedString,
    pub alt: String,
    pub kind: ImageKind,
    pub index: u64,
}

#[uniffi::remote(Record)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub href: Option<SharedString>,
}

#[uniffi::remote(Enum)]
pub enum Inline {
    Text { text: String, style: Style },
    Image(MarkdownImage),
}

#[uniffi::remote(Enum)]
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
    #[uniffi(name = "BlockList")]
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

#[uniffi::remote(Record)]
pub struct Block {
    /// Start byte in original UTF-8 Markdown; also the append-stable sibling ID.
    pub id: u64,
    /// Exclusive end byte.
    pub end: u64,
    pub content: Content,
}

#[uniffi::remote(Record)]
pub struct Changes {
    pub upsert: Vec<Block>,
    pub removed: Vec<u64>,
}

#[uniffi::remote(Enum)]
pub enum Alignment {
    Default,
    Left,
    Center,
    Right,
}

#[uniffi::remote(Record)]
pub struct Table {
    pub markdown: String,
    pub align: Vec<Alignment>,
    pub header: Vec<Vec<Inline>>,
    pub rows: Vec<Vec<Vec<Inline>>>,
}

#[uniffi::remote(Enum)]
pub enum ImageKind {
    Remote,
    Data,
    File,
}
#[derive(uniffi::Object)]
pub struct Document(pimate_markdown::Document);
#[uniffi::export]
pub fn parse(source: String, file_images: bool) -> Arc<Document> {
    Arc::new(Document(pimate_markdown::parse(&source, file_images)))
}
#[uniffi::export]
impl Document {
    pub fn blocks(&self) -> Vec<Block> {
        self.0.blocks.clone()
    }
    pub fn images(&self) -> Vec<MarkdownImage> {
        self.0.images.clone()
    }
    pub fn links(&self) -> Vec<String> {
        self.0.links.clone()
    }
    pub fn changes_since(&self, previous: Arc<Document>) -> Changes {
        self.0.changes_since(&previous.0)
    }
}
#[uniffi::export]
pub fn plain_text(block: Block) -> String {
    block.plain_text()
}
#[uniffi::export]
pub fn classify_image(src: String) -> Option<ImageKind> {
    pimate_markdown::classify_image(&src)
}
#[uniffi::export]
pub fn cell_width(cell: Vec<Inline>) -> u64 {
    pimate_markdown::cell_width(&cell)
}
#[uniffi::export]
pub fn table_widths(widths: Vec<u64>, available: f64, font_ratio: f64) -> Vec<f64> {
    pimate_markdown::table_widths(&widths, available, font_ratio)
}
#[derive(uniffi::Record, Debug, PartialEq)]
pub struct Utf16Range {
    pub start: u64,
    pub end: u64,
}
#[uniffi::export]
pub fn utf16_range(source: &str, start: u64, end: u64) -> Option<Utf16Range> {
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    let prefix = source.get(..start)?.encode_utf16().count() as u64;
    Some(Utf16Range {
        start: prefix,
        end: prefix + source.get(start..end)?.encode_utf16().count() as u64,
    })
}
#[test]
fn utf16_checks_scalar_boundaries() {
    let s = "あ😀b";
    assert_eq!(utf16_range(s, 3, 7), Some(Utf16Range { start: 1, end: 3 }));
    for (a, b) in [(1, 7), (3, 5), (7, 3), (0, u64::MAX)] {
        assert_eq!(utf16_range(s, a, b), None)
    }
}
