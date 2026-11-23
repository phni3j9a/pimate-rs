use pimate_markdown::{Document, parse};
use std::{hint::black_box, time::Instant};
fn main() {
    let chunk="## 計測\n\n本文 **重要** [docs](https://example.com)\n\n|列|値|\n|:-|-:|\n|一|`42`|\n\n```rs\nlet x = 1;\n```\n\n".repeat(4);
    let mut source = String::new();
    let mut prev = Document::default();
    let mut times = Vec::new();
    for _ in 0..100 {
        source.push_str(&chunk);
        let t = Instant::now();
        let doc = parse(&source, false);
        black_box(doc.changes_since(&prev));
        prev = doc;
        times.push(t.elapsed());
    }
    let last = times[99];
    times.sort();
    println!(
        "{} chars / {} bytes / {} updates: median {:?}, p95 {:?}, last {:?}",
        source.chars().count(),
        source.len(),
        times.len(),
        times[49],
        times[94],
        last
    );
}
