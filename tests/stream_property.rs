use streamdown::{Block, Delta, Inline, Op, Parser};

fn parse_whole(markdown: &str) -> Vec<Block> {
    let mut parser = Parser::new();
    parser.append(markdown);
    parser.finish();
    parser.blocks().to_vec()
}

fn parse_by_cuts(markdown: &str, cuts: &[usize]) -> Vec<Block> {
    let mut parser = Parser::new();
    let mut mirror = Vec::new();
    let mut start = 0;

    for &end in cuts.iter().chain(std::iter::once(&markdown.len())) {
        if end < start || !markdown.is_char_boundary(end) {
            continue;
        }
        let delta = parser.append(&markdown[start..end]);
        apply(&mut mirror, &delta);
        assert_eq!(
            mirror,
            parser.blocks(),
            "delta mirror diverged at byte {end}"
        );
        start = end;
    }

    let delta = parser.finish();
    apply(&mut mirror, &delta);
    assert_eq!(mirror, parser.blocks(), "delta mirror diverged on finish");
    parser.blocks().to_vec()
}

fn boundaries(markdown: &str) -> Vec<usize> {
    markdown
        .char_indices()
        .map(|(index, _)| index)
        .skip(1)
        .collect()
}

fn random_cuts(markdown: &str, seed: u64) -> Vec<usize> {
    let mut state = seed | 1;
    let mut cuts = Vec::new();
    for boundary in boundaries(markdown) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        if state & 3 == 0 {
            cuts.push(boundary);
        }
    }
    cuts
}

#[test]
fn randomized_multi_chunk_streaming_matches_whole_parse() {
    let corpus = [
        "# Heading\n\nplain token stream with unicode 日本語 ✅ and **bold** then `code`.\n",
        "prefix [[cite:doc-42|仕様書]] middle @[artifact:plot-1] suffix",
        ":::llm tool name=\"web search\" id=q1\n{\"query\":\"rust wasm 日本語\"}\n:::\n",
        "```rust\nfn main() {\n    println!(\"日本語\");\n}\n```\n",
        "a|b\n---|---\n1|2\n3|4\n",
        "> quote one\n> quote **two**\n\n1. first\n2. second\n",
        "$$\nA_3=\\begin{pmatrix}\na_1 & x_1 \\\\\na_2 & \\frac{1}{2}\n\\end{pmatrix}\n$$\n",
        "plain until syntax begins **in another chunk** and then [[cite:source]].",
        ":::llm artifact mime=application/json name=plan\n{\"steps\":[1,2,3]}\n:::\nAfter",
        "::::llm artifact mime=text/plain\nalpha\n:::\nomega\n::::\nAfter",
        "#\n## not-heading-until-space\n---\n- item\n+ other\n",
    ];

    for (case, markdown) in corpus.iter().enumerate() {
        let expected = parse_whole(markdown);
        for round in 0..256u64 {
            let seed = ((case as u64 + 1) << 32) ^ round.wrapping_mul(0x9e37_79b9_7f4a_7c15);
            let actual = parse_by_cuts(markdown, &random_cuts(markdown, seed));
            assert_eq!(
                actual, expected,
                "streaming AST changed for corpus case {case}, seed {seed:#x}"
            );
        }
    }
}

#[test]
fn byte_sized_ascii_streaming_matches_whole_parse() {
    let markdown = "Before [[cite:doc|label]] @[source:s1]\n\n:::llm tool id=t1\n{\"x\":1}\n:::\n\nAfter **bold**.";
    let expected = parse_whole(markdown);
    let cuts = (1..markdown.len())
        .filter(|&index| markdown.is_char_boundary(index))
        .collect::<Vec<_>>();
    assert_eq!(parse_by_cuts(markdown, &cuts), expected);
}

#[test]
fn single_line_fence_delta_volume_is_linear() {
    let mut parser = Parser::new();
    let mut mirror = Vec::new();
    apply(
        &mut mirror,
        &parser.append(":::llm artifact mime=application/json\n"),
    );

    let chunk = "0123456789abcdef";
    let chunks = 4096usize;
    let mut splice_append_bytes = 0usize;
    for _ in 0..chunks {
        let delta = parser.append(chunk);
        splice_append_bytes += delta
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::SpliceCode { append, .. } => Some(append.len()),
                _ => None,
            })
            .sum::<usize>();
        apply(&mut mirror, &delta);
        assert_eq!(mirror, parser.blocks());
    }

    let payload_bytes = chunks * chunk.len();
    assert_eq!(splice_append_bytes, payload_bytes);

    let close = parser.append("\n:::\n");
    apply(&mut mirror, &close);
    assert_eq!(mirror, parser.blocks());
    assert!(matches!(
        parser.blocks(),
        [Block::CodeBlock { text, closed: true, .. }]
            if text.len() == payload_bytes + 1
    ));
}

fn apply(document: &mut Vec<Block>, delta: &Delta) {
    for op in &delta.ops {
        match op {
            Op::Truncate { from } => document.truncate(*from as usize),
            Op::Push(block) => document.push(block.clone()),
            Op::SpliceCode {
                block,
                truncate_bytes,
                append,
            } => {
                let Block::CodeBlock { text, .. } = &mut document[*block as usize] else {
                    panic!("SpliceCode target is not code")
                };
                text.truncate(text.len() - *truncate_bytes as usize);
                text.push_str(append);
            }
            Op::SealCode { block } => {
                let Block::CodeBlock { closed, .. } = &mut document[*block as usize] else {
                    panic!("SealCode target is not code")
                };
                *closed = true;
            }
            Op::AppendText { block, append } => {
                let Block::Paragraph(nodes) = &mut document[*block as usize] else {
                    panic!("AppendText target is not paragraph")
                };
                let [Inline::Text(text)] = nodes.as_mut_slice() else {
                    panic!("AppendText target is not a single text node")
                };
                text.push_str(append);
            }
            Op::AppendInlineText { block, append } => {
                let Block::Paragraph(nodes) = &mut document[*block as usize] else {
                    panic!("AppendInlineText target is not paragraph")
                };
                if let Some(Inline::Text(text)) = nodes.last_mut() {
                    text.push_str(append);
                } else {
                    nodes.push(Inline::Text(append.clone()));
                }
            }
        }
    }
}
