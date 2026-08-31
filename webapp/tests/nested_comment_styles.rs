#[path = "../src/code.rs"]
mod code;

use code::TokenKind;

fn tokens(source: &str, language: &str) -> Vec<(String, TokenKind)> {
    let mut out = Vec::new();
    code::highlight(source, Some(language), |text, kind| {
        out.push((text.to_owned(), kind));
        true
    });
    out
}

fn assert_single_comment(source: &str, language: &str) {
    assert_eq!(
        tokens(source, language),
        vec![(source.to_owned(), TokenKind::Comment)],
        "language={language} source={source:?}",
    );
}

#[test]
fn julia_hash_equals_comments_nest_and_preempt_hash_line_comments() {
    assert_single_comment("#= outer #= inner =# tail =#", "julia");
    let line = tokens("# ordinary\nx = 1", "julia");
    assert_eq!(line[0], ("# ordinary".to_owned(), TokenKind::Comment));
}

#[test]
fn nim_hash_bracket_comments_nest_and_preempt_hash_line_comments() {
    assert_single_comment("#[ outer #[ inner ]# tail ]#", "nim");
    let line = tokens("# ordinary\nlet x = 1", "nim");
    assert_eq!(line[0], ("# ordinary".to_owned(), TokenKind::Comment));
}

#[test]
fn d_slash_plus_comments_nest() {
    assert_single_comment("/+ outer /+ inner +/ tail +/", "dlang");
}

#[test]
fn d_c_style_block_comments_remain_non_nested() {
    let source = "/* outer /* inner */ tail */";
    let got = tokens(source, "dlang");
    assert_eq!(
        got[0],
        ("/* outer /* inner */".to_owned(), TokenKind::Comment)
    );
    assert!(
        got.len() > 1,
        "the trailing tail must remain outside the first comment"
    );
}

#[test]
fn new_comment_delimiters_are_language_scoped() {
    for (source, language) in [
        ("#= not julia =#", "go"),
        ("#[ not nim ]#", "go"),
        ("/+ not d +/", "go"),
    ] {
        let got = tokens(source, language);
        assert_ne!(got, vec![(source.to_owned(), TokenKind::Comment)]);
    }
}
