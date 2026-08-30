use streamdown::{Block, Inline, Op, Parser};

fn whole(markdown: &str) -> Vec<Block> {
    let mut parser = Parser::new();
    parser.append(markdown);
    parser.blocks().to_vec()
}

#[test]
fn complete_plain_quote_line_splices_without_republishing_quote() {
    let mut parser = Parser::new();
    parser.append("> seed\n");

    let delta = parser.append("> token token\n");
    assert_eq!(
        delta.ops,
        vec![Op::SpliceQuoteTail {
            block: 0,
            remove_nodes: 0,
            truncate_bytes: 0,
            append: vec![Inline::SoftBreak, Inline::Text("token token".to_owned()),],
        }]
    );
    assert_eq!(parser.blocks(), whole("> seed\n> token token\n"));

    let delta = parser.append("> 日本語✅\n");
    assert!(matches!(
        delta.ops.as_slice(),
        [Op::SpliceQuoteTail { append, .. }]
            if append == &vec![Inline::SoftBreak, Inline::Text("日本語✅".to_owned())]
    ));
    assert_eq!(
        parser.blocks(),
        whole("> seed\n> token token\n> 日本語✅\n")
    );
}

#[test]
fn unsealed_plain_quote_chunk_stays_incremental() {
    let mut parser = Parser::new();
    parser.append("> seed\n");
    let delta = parser.append("> token");
    assert!(matches!(delta.ops.as_slice(), [Op::SpliceQuoteTail { .. }]));
    assert_eq!(parser.blocks(), whole("> seed\n> token"));

    let tail = parser.append(" tail");
    assert!(matches!(tail.ops.as_slice(), [Op::SpliceQuoteTail { .. }]));
    assert_eq!(parser.blocks(), whole("> seed\n> token tail"));
}

#[test]
fn quote_hard_break_boundary_falls_back_to_reparse() {
    let mut parser = Parser::new();
    parser.append("> seed  \n");
    let delta = parser.append("> next");
    assert!(matches!(delta.ops.first(), Some(Op::Truncate { from: 0 })));
    assert_eq!(parser.blocks(), whole("> seed  \n> next"));
    assert!(matches!(
        parser.blocks(),
        [Block::BlockQuote(nodes)] if nodes.iter().any(|node| matches!(node, Inline::HardBreak))
    ));
}

#[test]
fn empty_or_formatted_quote_boundaries_fall_back_safely() {
    let mut empty = Parser::new();
    empty.append("> seed\n");
    assert!(empty.append(">\n").ops.is_empty());
    let delta = empty.append("> next");
    assert!(matches!(delta.ops.first(), Some(Op::Truncate { from: 0 })));
    assert_eq!(empty.blocks(), whole("> seed\n>\n> next"));

    let mut formatted = Parser::new();
    formatted.append("> *open\n");
    let delta = formatted.append("> close*");
    assert!(matches!(delta.ops.first(), Some(Op::Truncate { from: 0 })));
    assert_eq!(formatted.blocks(), whole("> *open\n> close*"));

    let mut current = Parser::new();
    current.append("> seed\n");
    let delta = current.append("> **bold**\n");
    assert!(matches!(delta.ops.first(), Some(Op::Truncate { from: 0 })));
    assert_eq!(current.blocks(), whole("> seed\n> **bold**\n"));
}
