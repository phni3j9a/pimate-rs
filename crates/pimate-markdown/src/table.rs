use crate::Inline;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Default,
    Left,
    Center,
    Right,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub markdown: String,
    pub align: Vec<Alignment>,
    pub header: Vec<Vec<Inline>>,
    pub rows: Vec<Vec<Vec<Inline>>>,
}
/// Approximate display columns, including the tile/inline-code padding in the TS UI.
pub fn cell_width(cell: &[Inline]) -> u64 {
    cell.iter().map(|s|match s {Inline::Image(_)=>24, Inline::Text{text,style}=>2*u64::from(style.code)+text.chars().map(|c|if matches!(c,'\u{3000}'..='\u{9fff}'|'\u{ac00}'..='\u{d7af}'|'\u{ff00}'..='\u{ffef}') {2}else{1}).sum::<u64>()}).sum()
}
pub fn table_widths(widths: &[u64], available: f64, font_ratio: f64) -> Vec<f64> {
    let min = 16. + 84. * font_ratio;
    let natural: Vec<_> = widths
        .iter()
        .map(|&w| (16. + (w as f64 * 7.5).max(40.) * font_ratio).min(240.))
        .collect();
    let total = natural.iter().sum::<f64>();
    let floor = natural.iter().map(|w| w.min(min)).sum::<f64>();
    natural
        .into_iter()
        .map(|w| {
            if total < available {
                w / total * available
            } else if total == available || floor > available || w <= min {
                w
            } else {
                min + (w - min) * (available - floor) / (total - floor)
            }
        })
        .collect()
}
