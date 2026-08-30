use streamdown::{Block, Delta, Inline, Op, Parser};

fn parse_whole(markdown: &str) -> Vec<Block> {
    let mut parser = Parser::new();
    parser.append(markdown);
    parser.finish();
    parser.blocks().to_vec()
}

fn parse_chunks<'a>(chunks: impl IntoIterator<Item = &'a str>) -> Vec<Block> {
    let mut parser = Parser::new();
    let mut mirror = Vec::new();
    for chunk in chunks {
        let delta = parser.append(chunk);
        apply(&mut mirror, &delta);
        assert_eq!(
            mirror,
            parser.blocks(),
            "delta mirror diverged after {chunk:?}"
        );
    }
    let delta = parser.finish();
    apply(&mut mirror, &delta);
    assert_eq!(mirror, parser.blocks(), "delta mirror diverged on finish");
    parser.blocks().to_vec()
}

fn utf8_boundaries(text: &str) -> Vec<usize> {
    text.char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
        .collect()
}

fn assert_every_single_split(markdown: &str) {
    let expected = parse_whole(markdown);
    let boundaries = utf8_boundaries(markdown);
    for &split in &boundaries {
        let (left, right) = markdown.split_at(split);
        let actual = parse_chunks([left, right]);
        assert_eq!(actual, expected, "AST changed at UTF-8 split {split}");
    }
}

#[test]
fn llm_inline_extensions_are_chunk_boundary_independent() {
    assert_every_single_split(
        "根拠 [[cite:doc-42|仕様書]] と @[source:turn7search2] を参照。 **重要** ✅",
    );
}

#[test]
fn llm_semantic_fence_is_chunk_boundary_independent_exhaustively() {
    assert_every_single_split(
        "Before\n\n:::llm tool name=\"web search\" id=q1\n{\"query\":\"rust wasm 日本語\"}\n:::\n\nAfter",
    );
}

#[test]
fn long_llm_fence_with_short_colons_is_chunk_boundary_independent() {
    assert_every_single_split(
        "Before\n\n::::llm artifact mime=text/plain\nalpha\n:::\nomega\n::::\n\nAfter",
    );
}

#[test]
fn multiline_inline_tail_fast_path_is_chunk_boundary_independent() {
    assert_every_single_split(
        "First **bold** line\ncontinuation token token [[cite:doc-1|source]] and more text.",
    );
}

#[test]
fn mixed_markdown_plain_fast_path_is_chunk_boundary_independent() {
    assert_every_single_split(
        "A long plain token stream keeps going until **bold**, then `code`, then [[cite:bench-1]].",
    );
}

#[test]
fn inline_tail_fast_path_respects_escape_and_link_completion() {
    assert_every_single_split(
        "Prefix **bold** escaped \\. and [docs](https://example.com/path) suffix",
    );
}

#[test]
fn plain_delimiter_bodies_are_character_stream_independent() {
    for markdown in [
        "prefix `code` suffix",
        "prefix $x+1$ suffix",
        "prefix *bold* suffix",
        "prefix _em_ suffix",
        "prefix **bold** suffix",
        "prefix __bold__ suffix",
        "prefix `日本語✅` $数式$ *強調* _文字_ **太字** __強調__ suffix",
    ] {
        let expected = parse_whole(markdown);
        let boundaries = utf8_boundaries(markdown);
        let actual = parse_chunks(
            boundaries
                .windows(2)
                .map(|window| &markdown[window[0]..window[1]]),
        );
        assert_eq!(
            actual, expected,
            "character stream changed AST for {markdown:?}"
        );
    }
}

#[test]
fn display_math_plain_body_is_character_stream_independent() {
    for markdown in ["prefix $$x+1$$ suffix", "prefix $$日本語✅$$ suffix"] {
        let expected = parse_whole(markdown);
        let boundaries = utf8_boundaries(markdown);
        let actual = parse_chunks(
            boundaries
                .windows(2)
                .map(|window| &markdown[window[0]..window[1]]),
        );
        assert_eq!(
            actual, expected,
            "character stream changed display math AST for {markdown:?}"
        );
    }
}

#[test]
fn delimiter_run_periods_match_whole_parse_exhaustively() {
    for delimiter in ['`', '$', '*', '_'] {
        for length in 1..=128 {
            let markdown = format!("prefix {}", delimiter.to_string().repeat(length));
            let expected = parse_whole(&markdown);
            let actual = parse_chunks((0..markdown.len()).map(|index| &markdown[index..index + 1]));
            assert_eq!(actual, expected, "delimiter={delimiter:?} length={length}");
        }
    }
}

#[test]
fn delimiter_fast_paths_respect_cross_chunk_escapes() {
    assert_every_single_split(r"prefix \* literal \_ literal \$ literal \` literal");
}

#[test]
fn delimiter_run_splice_is_byte_stream_independent() {
    let markdown = "prefix ```` $$$$$$$$ suffix";
    let expected = parse_whole(markdown);
    let actual = parse_chunks((0..markdown.len()).map(|index| &markdown[index..index + 1]));
    assert_eq!(actual, expected);
    assert_every_single_split("escaped \\`` and \\$$$$ stay literal");
}

#[test]
fn special_bracket_state_is_chunk_boundary_independent() {
    assert_every_single_split("@[x] [] ]() [[cite:bad id]] @[source:ok] [[cite:doc]] [label](url)");
}

#[test]
fn simple_and_nested_links_are_chunk_boundary_independent() {
    for markdown in [
        "prefix [x](u) [y](v) suffix",
        "[[x](u)](v)",
        "prefix [[x](u)](v) [z](w) suffix",
    ] {
        assert_every_single_split(markdown);
    }
}

#[test]
fn escaped_closers_are_chunk_boundary_independent() {
    assert_every_single_split(r"prefix \] \) @[source:id\] suffix");
}

#[test]
fn inert_closer_fast_path_is_chunk_boundary_independent() {
    assert_every_single_split("]]]))) prefix [x](https://example.test) suffix ))]");
}

#[test]
fn backslash_run_fast_path_is_chunk_boundary_independent() {
    let markdown = format!("prefix **bold** {}* escaped tail", "\\".repeat(17));
    assert_every_single_split(&markdown);
}

#[test]
fn opener_heavy_inline_fast_path_is_chunk_boundary_independent() {
    assert_every_single_split(
        "prefix [[[[[@[@[source:id] suffix (((( [label](https://example.test)",
    );
}

#[test]
fn list_tail_deltas_are_character_stream_independent() {
    for markdown in ["- one\n- two\n- three", "1. one\n2. two\n3. three"] {
        let expected = parse_whole(markdown);
        let boundaries = utf8_boundaries(markdown);
        let actual = parse_chunks(
            boundaries
                .windows(2)
                .map(|window| &markdown[window[0]..window[1]]),
        );
        assert_eq!(
            actual, expected,
            "character stream changed list AST for {markdown:?}"
        );
    }
}

#[test]
fn table_separator_becoming_valid_during_plain_append_reparses() {
    let markdown = "a|b\n---|---";
    let expected = parse_whole(markdown);
    let actual = parse_chunks(["a|b\n---|", "---"]);
    assert_eq!(actual, expected);
    assert!(matches!(actual.as_slice(), [Block::Table { .. }]));
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
                let Some(Inline::Text(text)) = nodes.last_mut() else {
                    panic!("AppendText target has no trailing text")
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
            Op::SpliceInlineTail {
                block,
                remove_nodes,
                truncate_bytes,
                append,
            } => {
                let Block::Paragraph(nodes) = &mut document[*block as usize] else {
                    panic!("SpliceInlineTail target is not paragraph")
                };
                if *truncate_bytes != 0 {
                    let Some(Inline::Text(text)) = nodes.last_mut() else {
                        panic!("SpliceInlineTail target has no trailing text")
                    };
                    text.truncate(text.len() - *truncate_bytes as usize);
                    if text.is_empty() {
                        nodes.pop();
                    }
                }
                if *remove_nodes != 0 {
                    nodes.truncate(nodes.len() - *remove_nodes as usize);
                }
                for incoming in append {
                    if let Inline::Text(value) = incoming
                        && let Some(Inline::Text(text)) = nodes.last_mut()
                    {
                        text.push_str(value);
                    } else {
                        nodes.push(incoming.clone());
                    }
                }
            }
            Op::AppendListItem { block, item } => match &mut document[*block as usize] {
                Block::UnorderedList(items) | Block::OrderedList { items, .. } => {
                    items.push(item.clone())
                }
                _ => panic!("AppendListItem target is not a list"),
            },
            Op::SpliceListItemTail {
                block,
                remove_nodes,
                truncate_bytes,
                append,
            } => {
                let items = match &mut document[*block as usize] {
                    Block::UnorderedList(items) | Block::OrderedList { items, .. } => items,
                    _ => panic!("SpliceListItemTail target is not a list"),
                };
                let item = items
                    .last_mut()
                    .expect("SpliceListItemTail target has no final item");
                if *truncate_bytes != 0 {
                    let Inline::Text(text) = item
                        .last_mut()
                        .expect("SpliceListItemTail target has no tail")
                    else {
                        panic!("SpliceListItemTail target has no trailing text")
                    };
                    text.truncate(text.len() - *truncate_bytes as usize);
                    if text.is_empty() {
                        item.pop();
                    }
                }
                if *remove_nodes != 0 {
                    item.truncate(item.len() - *remove_nodes as usize);
                }
                for incoming in append {
                    if let Inline::Text(value) = incoming
                        && let Some(Inline::Text(text)) = item.last_mut()
                    {
                        text.push_str(value);
                    } else {
                        item.push(incoming.clone());
                    }
                }
            }
        }
    }
}
