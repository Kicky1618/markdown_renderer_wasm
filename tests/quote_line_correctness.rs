use streamdown::{Block, Op, Parser};

fn whole(markdown: &str) -> Vec<Block> {
    let mut parser = Parser::new();
    parser.append(markdown);
    parser.blocks().to_vec()
}

fn chunked(chunks: &[&str]) -> Vec<Block> {
    let mut parser = Parser::new();
    for chunk in chunks {
        parser.append(chunk);
    }
    parser.blocks().to_vec()
}

#[test]
fn complete_quote_line_preserves_previous_hard_break() {
    let chunks = ["> seed  \n", "> next\n"];
    assert_eq!(chunked(&chunks), whole("> seed  \n> next\n"));
}

#[test]
fn partial_quote_tail_preserves_previous_hard_break() {
    let chunks = ["> seed  \n", ">", " ", "next"];
    assert_eq!(chunked(&chunks), whole("> seed  \n> next"));
}

#[test]
fn empty_first_quote_line_keeps_leading_soft_break() {
    let chunks = [">\n", "> next\n"];
    assert_eq!(chunked(&chunks), whole(">\n> next\n"));
}

#[test]
fn cross_line_delimiter_falls_back_to_whole_parse() {
    let chunks = ["> *open\n", "> close*\n"];
    assert_eq!(chunked(&chunks), whole("> *open\n> close*\n"));
}

#[test]
fn self_contained_rich_lines_keep_fast_path() {
    let mut parser = Parser::new();
    parser.append("> one\n");
    let delta = parser.append("> **two** [x](u)\n");
    assert!(matches!(delta.ops.as_slice(), [Op::SpliceQuoteTail { .. }]));
    assert_eq!(parser.blocks(), whole("> one\n> **two** [x](u)\n"));
}

#[test]
fn consecutive_self_contained_rich_lines_keep_fast_path() {
    let mut parser = Parser::new();
    parser.append("> one\n");
    assert!(matches!(
        parser.append("> **two**\n").ops.as_slice(),
        [Op::SpliceQuoteTail { .. }]
    ));
    assert!(matches!(
        parser.append("> `three`\n").ops.as_slice(),
        [Op::SpliceQuoteTail { .. }]
    ));
    assert_eq!(parser.blocks(), whole("> one\n> **two**\n> `three`\n"));
}

#[test]
fn unclosed_strong_run_does_not_hide_behind_empty_emphasis() {
    let chunks = ["> **open\n", "> **bold**\n"];
    assert_eq!(chunked(&chunks), whole("> **open\n> **bold**\n"));
}

#[test]
fn quote_ambiguity_survives_plain_middle_lines_until_reparse() {
    let chunks = ["> *open\n", "> plain\n", "> *em*\n"];
    assert_eq!(chunked(&chunks), whole("> *open\n> plain\n> *em*\n"));
}
