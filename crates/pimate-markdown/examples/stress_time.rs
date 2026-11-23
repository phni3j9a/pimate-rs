//! Synthetic upper-tail samples; includes tree construction and destruction on a constrained stack.
use pimate_markdown::parse;
use std::{hint::black_box, time::Instant};
fn main() {
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            for n in [1000, 10000, 50000] {
                for (label, text) in [
                    ("quotes", format!("{}text", ">".repeat(n))),
                    ("open links", "[x".repeat(n)),
                    ("open emphasis", "*a ".repeat(n)),
                    (
                        "sparse table",
                        format!(
                            "{}\n{}\n{}",
                            "|h".repeat(128),
                            "|-".repeat(128),
                            "|x\n".repeat(n)
                        ),
                    ),
                ] {
                    let start = Instant::now();
                    drop(black_box(parse(&text, false)));
                    println!("{label} n={n}: {:?}", start.elapsed());
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
