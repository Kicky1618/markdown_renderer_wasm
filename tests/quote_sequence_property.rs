use streamdown::Parser;
fn whole(s: &str) -> Vec<streamdown::Block> {
    let mut p = Parser::new();
    p.append(s);
    p.blocks().to_vec()
}
#[test]
fn randomized_quote_line_sequences_match_whole() {
    let bodies = [
        "plain",
        "**bold**",
        "*em*",
        "__bold__",
        "_em_",
        "`code`",
        "$x$",
        "[x](u)",
        "@[source:id]",
        "[[cite:doc|Spec]]",
        "*open",
        "close*",
        "**open",
        "close**",
        "_open",
        "close_",
        "__open",
        "close__",
        "plain  ",
        "",
        "***triple***",
        "\\*escaped*",
        "[open",
        "close](u)",
    ];
    let mut state = 0x9e3779b97f4a7c15u64;
    for case in 0..10_000usize {
        let mut lines = Vec::with_capacity(6);
        for _ in 0..6 {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            lines.push(bodies[(state as usize) % bodies.len()]);
        }
        let mut source = String::new();
        let mut streamed = Parser::new();
        for body in &lines {
            let line = format!("> {body}\n");
            source.push_str(&line);
            streamed.append(&line);
        }
        let expected = whole(&source);
        assert_eq!(
            streamed.blocks(),
            expected.as_slice(),
            "case={case} lines={lines:?}"
        );
    }
}
