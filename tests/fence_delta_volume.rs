use streamdown::{Block, Op, Parser};

#[test]
fn single_line_llm_payload_delta_volume_is_linear() {
    let mut parser = Parser::new();
    parser.append(":::llm artifact mime=application/json\n");

    let chunk = "0123456789abcdef";
    let chunks = 4096usize;
    let mut appended_bytes = 0usize;

    for _ in 0..chunks {
        let delta = parser.append(chunk);
        for op in delta.ops {
            if let Op::SpliceCode {
                truncate_bytes,
                append,
                ..
            } = op
            {
                assert_eq!(truncate_bytes, 0);
                appended_bytes += append.len();
            }
        }
    }

    let payload_bytes = chunks * chunk.len();
    assert_eq!(appended_bytes, payload_bytes);

    let close = parser.append("\n:::\n");
    assert!(matches!(close.ops.last(), Some(Op::SealCode { block: 0 })));
    assert!(matches!(
        parser.blocks(),
        [Block::CodeBlock { text, closed: true, .. }]
            if text.len() == payload_bytes + 1
    ));
}

#[test]
fn split_closing_fence_retracts_only_the_candidate_line() {
    let mut parser = Parser::new();
    parser.append("```text\nhello\n``");

    let close = parser.append("`\n");
    assert!(matches!(
        close.ops.as_slice(),
        [
            Op::SpliceCode {
                truncate_bytes: 2,
                append,
                ..
            },
            Op::SealCode { block: 0 }
        ] if append.is_empty()
    ));
    assert!(matches!(
        parser.blocks(),
        [Block::CodeBlock { text, closed: true, .. }] if text == "hello\n"
    ));
}
