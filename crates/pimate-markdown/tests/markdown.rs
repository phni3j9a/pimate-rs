use pimate_markdown::*;
#[test]
fn render_chat() {
    let md = "## 見出し\r\n\r\nこれは**「太字」**です *it* ~~old~~ `code` [docs](https://x.dev) [js](javascript:bad) https://x.dev/auto\r\n\r\n3. 三\n   - [x] 済\n   - [ ] 未\n4. 四\n\n> 引用\n\n---\n\n```ts example\n\tlet x = 1;\n```";
    let d = parse(md, false);
    assert_eq!(d.links, ["https://x.dev", "https://x.dev/auto"]);
    let b = &d.blocks;
    assert!(matches!(b[0].content, Content::Heading { level: 2, .. }));
    assert_eq!(&md[b[0].id as usize..b[0].end as usize], "## 見出し");
    let Content::Paragraph(spans) = &b[1].content else {
        panic!()
    };
    for (value, flag) in [("「太字」", 0), ("it", 1), ("old", 2), ("code", 3)] {
        assert!(spans.iter().any(|s|matches!(s,Inline::Text{text,style} if text==value && [style.bold,style.italic,style.strike,style.code][flag])));
    }
    let Content::List {
        start: Some(3),
        items,
    } = &b[2].content
    else {
        panic!()
    };
    assert_eq!(items[1].plain_text(), "四");
    let Content::Item { children, .. } = &items[0].content else {
        panic!()
    };
    let Content::List { start: None, items } = &children[1].content else {
        panic!()
    };
    for (i, checked) in [true, false].into_iter().enumerate() {
        assert!(matches!(items[i].content,Content::Item{checked:Some(c),..} if c==checked));
    }
    assert_eq!(b[3].plain_text(), "引用");
    assert_eq!(b[4].content, Content::Rule);
    assert_eq!(
        b[5].content,
        Content::Code {
            language: Some("ts".into()),
            text: "\tlet x = 1;".into()
        }
    );
}
#[test]
fn tables_and_images() {
    let md = "# ![a](a.png)\n\n> [![b](b.png)](https://x.dev)\n\n- [x] before ![c](c.png) after\n\n| ![d](d.png) | 漢字 |\r\n| :- | -: |\r\n| A ![e](e.png) | `abc` |";
    let d = parse(md, true);
    assert_eq!(
        d.images
            .iter()
            .map(|i| (i.index, i.src.as_ref()))
            .collect::<Vec<_>>(),
        [
            (0, "a.png"),
            (1, "b.png"),
            (2, "c.png"),
            (3, "d.png"),
            (4, "e.png")
        ]
    );
    assert_eq!(d.links, ["https://x.dev"]);
    let b = d.blocks.last().unwrap();
    let Content::Table(t) = &b.content else {
        panic!()
    };
    assert_eq!(t.markdown, md[b.id as usize..].replace("\r\n", "\n"));
    assert_eq!(t.align, [Alignment::Left, Alignment::Right]);
    assert_eq!(b.plain_text(), "d\t漢字\nA e\tabc");
    assert_eq!(cell_width(&t.header[0]), 24);
    assert_eq!(cell_width(&t.header[1]), 4);
    assert_eq!(cell_width(&t.rows[0][1]), 5);
    assert!(parse(md, false).images.is_empty());
    assert_eq!(table_widths(&[2, 12], 324., 1.), [112., 212.]);
    assert_eq!(table_widths(&[100, 100], 300., 2.), [240., 240.]);
    let widths = table_widths(&[2, 20, 100], 320., 1.);
    assert_eq!(widths[0], 56.);
    assert!(widths[1] >= 100. && widths[1] < widths[2]);
    assert!((widths.iter().sum::<f64>() - 320.).abs() < 1e-8);
}
#[test]
fn image_sources() {
    for s in [
        "https://example.com/x.png?a=1",
        "http://localhost:3000/image",
        "HTTPS://example.com/x.webp",
    ] {
        assert_eq!(classify_image(s), Some(ImageKind::Remote));
    }
    for s in [
        "/tmp/x.png",
        "./x.png",
        "../x.png",
        "x.png",
        "~/x.png",
        "file:///tmp/a%20b.png",
        "images/no-extension",
        "/tmp/日本語.png",
        "./100%real.png",
    ] {
        assert_eq!(classify_image(s), Some(ImageKind::File));
    }
    for s in [
        "",
        "javascript:alert(1)",
        "content://image",
        "ftp://example.com/a.png",
        "//example.com/x.png",
        "data:image/svg+xml;base64,YQ==",
        "data:text/plain;base64,YQ==",
        "data:image/png;base64,Y Q==",
        "data:image/png;base64,YQ=",
        "data:image/png;base64,",
        "https://",
        "https://user:secret@example.com/a",
        "file://remote/a.png",
        "file:///tmp/a%ZZ.png",
        "file:///tmp/a.png?q=1",
        "a\0.png",
        "https://example.com/a.SVG?q=1",
        "https://example.com/a%2Esvg#x",
        "./x.svg",
        "~/x.svgz",
    ] {
        assert_eq!(classify_image(s), None, "{s}");
    }
    for mime in ["png", "jpeg", "webp", "gif"] {
        assert_eq!(
            classify_image(&format!("data:image/{mime};base64,YQ==")),
            Some(ImageKind::Data)
        );
    }
    let lim = pimate_protocol::MAX_PROMPT_IMAGE_BYTES;
    let mut s = format!(
        "data:image/png;base64,{}=",
        "A".repeat(lim.div_ceil(3) * 4 - 1)
    );
    assert_eq!(classify_image(&s), Some(ImageKind::Data));
    s.pop();
    s.push('A');
    assert_eq!(classify_image(&s), None);
}
#[test]
fn append_deltas() {
    let mut prev = Document::default();
    let mut rendered = std::collections::BTreeMap::new();
    let mut md = String::new();
    for suffix in [
        "# 永続\n\n[x][url]\n\n",
        "|a|b|",
        "\n|-|-|\n|1|2|",
        "\n\n```rs\n![a](a.png)",
        "\n```\n\n`![x](x.png)`\n\n![x](a.png",
        ")\n\n[url]: https://a.dev",
    ] {
        md += suffix;
        let d = parse(&md, true);
        let changes = d.changes_since(&prev);
        if !prev.blocks.is_empty() {
            assert!(!changes.upsert.iter().any(|b| b.id == 0));
        }
        for id in changes.removed {
            rendered.remove(&id);
        }
        for b in changes.upsert {
            rendered.insert(b.id, b);
        }
        assert!(rendered.values().eq(d.blocks.iter()));
        if suffix.starts_with("\n|-|") {
            assert!(matches!(d.blocks[2].content, Content::Table(_)));
        }
        if suffix.starts_with("\n\n```") {
            assert_eq!(d.blocks.last().unwrap().plain_text(), "![a](a.png)");
        }
        if !suffix.starts_with(')') {
            assert!(d.images.is_empty());
        }
        prev = d;
    }
    assert_eq!(prev.images.len(), 1);
    assert_eq!(prev.links, ["https://a.dev"]);
}
#[test]
fn raw_table_preserves_spelling_and_marked_line_endings() {
    let table = "| H |\n|:--:|\n| \\| |";
    for (suffix, ending) in [
        ("", ""),
        ("\r\n", "\n"),
        ("\n# Next", "\n"),
        ("\n\nNext", ""),
    ] {
        let d = parse(&(table.to_owned() + suffix), false);
        let Content::Table(t) = &d.blocks[0].content else {
            panic!()
        };
        assert_eq!(t.markdown, table.to_owned() + ending);
    }
    let d = parse("> | H |\n>|:--:|\n> | \\| |\n", false);
    let Content::Quote(q) = &d.blocks[0].content else {
        panic!()
    };
    let Content::Table(t) = &q[0].content else {
        panic!()
    };
    assert_eq!(&t.markdown, table);
}
#[test]
fn table_cr_only_and_duplicate_image_identity() {
    let d = parse("|H|\r|-|\r|b|\r", false);
    let Content::Table(t) = &d.blocks[0].content else {
        panic!()
    };
    assert_eq!(t.markdown, "|H|\n|-|\n|b|\n");
    let md = "![one](https://x.dev/i.png) ![two](https://x.dev/i.png)";
    let before = parse(md, false);
    let after = parse(&(md.to_owned() + " more"), false);
    assert_eq!(before.images, after.images);
    assert_eq!(
        before.images.iter().map(|i| i.index).collect::<Vec<_>>(),
        [0, 1]
    );
}

#[test]
fn sparse_and_oversized_tables() {
    let md = |n| {
        format!(
            "{}\n{}\n{}",
            "|a".repeat(n),
            "|-".repeat(n),
            "|x\n".repeat(128)
        )
    };
    let d = parse(&md(32), false);
    let Content::Table(t) = &d.blocks[0].content else {
        panic!()
    };
    assert_eq!(t.header.len(), 32);
    assert!(t.rows.iter().all(|r| r.len() == 1));
    let big = md(128);
    let d = parse(&big, false);
    assert_eq!(d.blocks[0].plain_text(), big);
}
#[test]
fn complexity_fallback_preserves_text() {
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            for (marker, count) in [(">", 20000), ("- ", 2000)] {
                let tail = format!("{}leaf", marker.repeat(count));
                let md = format!("# keep\n\n{tail}\n\n## after");
                let d = parse(&md, false);
                assert_eq!(d.blocks[0].plain_text(), "keep");
                assert_eq!(d.blocks.last().unwrap().plain_text(), "after");
                let leaf = d.blocks[1].plain_text();
                assert!(leaf.ends_with("leaf") && leaf.starts_with(marker));
                assert!(tail.ends_with(&leaf));
            }
            for tail in ["[".repeat(30000), "*a ".repeat(30000), "字".repeat(400000)] {
                let d = parse(&tail, false);
                assert_eq!(d.blocks[0].plain_text(), tail.trim_end());
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
#[test]
fn dense_ordinary_answers_remain_formatted() {
    let md = format!(
        "{}\n```rs\n{}\n```\n\n## after",
        "- **result** `a_b(x)`\n".repeat(1024),
        "fn a_b(x: i32) { println!(\"{}\", x); }\n".repeat(2048)
    );
    let d = parse(&md, false);
    let Content::List { items, .. } = &d.blocks[0].content else {
        panic!()
    };
    assert_eq!(items.len(), 1024);
    assert!(matches!(d.blocks[1].content, Content::Code { .. }));
    assert!(matches!(d.blocks[2].content, Content::Heading { .. }));
    let table = format!("|h|v|\n|-|-|\n{}", "|a|`b_c`|\n".repeat(2000));
    assert!(
        matches!(&parse(&table,false).blocks[0].content,Content::Table(t) if t.rows.len()==2000)
    );
}
#[test]
fn repeated_data_definition_shares_storage() {
    let uri = format!("data:image/png;base64,{}", "AAAA".repeat(65536));
    let md = format!("{}\n[r]: {uri}", "![alt][r]\n\n".repeat(100));
    let d = parse(&md, true);
    assert_eq!(d.images.len(), 100);
    for (index, image) in d.images.iter().enumerate() {
        assert_eq!(image.index, index as u64);
        assert!(std::sync::Arc::ptr_eq(&image.src, &d.images[0].src));
        let Content::Paragraph(p) = &d.blocks[index].content else {
            panic!()
        };
        assert!(matches!(&p[0],Inline::Image(i) if std::sync::Arc::ptr_eq(&i.src,&image.src)));
    }
}
#[test]
fn image_entities_do_not_enable_remote_loading() {
    for md in [
        "![a](https&#58;//x.dev/i.png)",
        "![a][r]\n\n[r]: https&#58;//x.dev/i.png",
    ] {
        assert!(parse(md, false).images.is_empty());
        let d = parse(md, true);
        assert_eq!(d.images[0].kind, ImageKind::File);
        assert_eq!(d.images[0].src.as_ref(), "https&#58;//x.dev/i.png");
    }
}
#[test]
fn delimiter_with_variable_quote_prefix() {
    let d = parse(">>H|H\n> > |-|-|\n>>a|b", false);
    let Content::Quote(q) = &d.blocks[0].content else {
        panic!()
    };
    let Content::Quote(q) = &q[0].content else {
        panic!()
    };
    let Content::Table(t) = &q[0].content else {
        panic!()
    };
    assert_eq!(t.markdown, "H|H\n|-|-|\na|b");
}
#[test]
fn multiline_code_span() {
    for newline in ["\r\n", "\n", "\r"] {
        for (body, expected) in [
            (format!("a{newline}b"), "a b"),
            (format!("{newline}a{newline}"), "a"),
            (format!(" {newline}a{newline} "), " a "),
            (format!(" {newline} "), "   "),
        ] {
            assert_eq!(
                parse(&format!("`{body}`"), false).blocks[0].plain_text(),
                expected
            );
        }
    }
}
#[test]
fn cjk_bold_uses_literal_markers() {
    let d = parse(r"\* &#42;&#42;x&#42;&#42;とこれは**「太字」**です", false);
    assert_eq!(d.blocks[0].plain_text(), "* **x**とこれは「太字」です");
    let Content::Paragraph(p) = &d.blocks[0].content else {
        panic!()
    };
    assert!(
        p.iter()
            .any(|s| matches!(s,Inline::Text{text,style} if text=="「太字」" && style.bold))
    );
}
#[test]
fn code_selection_expands_tabs_but_full_copy_preserves_them() {
    let d = parse("```\n\tx();\n```", false);
    assert_eq!(d.blocks[0].plain_text(), "  x();");
    assert!(matches!(&d.blocks[0].content,Content::Code{text,..} if text=="\tx();"));
}
#[test]
fn encoded_image_uses_real_destination_not_code_in_alt() {
    let md = "![`x](https://wrong.test/x)`](https&#58;//real.test/x)";
    assert!(parse(md, false).images.is_empty());
    let d = parse(md, true);
    assert_eq!(d.images[0].src.as_ref(), "https&#58;//real.test/x");
}
#[test]
fn expanded_link_budget_uses_literal_after_limit() {
    let url = format!("https://e.test/{}", "a".repeat(64 * 1024));
    let md = format!("{}[url]: {url}\n\n## after", "[x][url]\n\n".repeat(64));
    let d = parse(&md, false);
    assert_eq!(d.blocks[0].plain_text(), "x");
    assert_eq!(d.blocks[d.blocks.len() - 2].plain_text(), "[x][url]");
    assert!(matches!(
        d.blocks.last().unwrap().content,
        Content::Heading { level: 2, .. }
    ));
    assert!(d.links.iter().map(String::len).sum::<usize>() <= 2 * 1024 * 1024);
}
#[test]
fn wide_tables_only_flatten_the_actual_overflow() {
    let header = "|H".repeat(200);
    let delimiter = "|-".repeat(200);
    let sparse = "|x\n".repeat(80);
    let big = format!("{header}\n{delimiter}\n{sparse}");
    let md = format!("```\n{big}```\n\n{header}\n{delimiter}\n|x\n\n{big}\n# after");
    let d = parse(&md, false);
    assert!(matches!(&d.blocks[0].content,Content::Code{text,..} if text==big.trim_end()));
    assert!(
        matches!(&d.blocks[1].content,Content::Table(t) if t.header.len()==200 && t.rows.len()==1)
    );
    assert_eq!(d.blocks[2].plain_text(), big);
    assert!(matches!(d.blocks[3].content, Content::Heading { .. }));
}
#[test]
fn tight_item_spans_keep_real_byte_ranges() {
    let md = "- **日本**と `x`\n- [x] 完了";
    let d = parse(md, false);
    let Content::List { items, .. } = &d.blocks[0].content else {
        panic!()
    };
    for (item, raw) in items.iter().zip(["**日本**と `x`", "完了"]) {
        let Content::Item { children, .. } = &item.content else {
            panic!()
        };
        let b = &children[0];
        assert_eq!(&md[b.id as usize..b.end as usize], raw);
    }
}
