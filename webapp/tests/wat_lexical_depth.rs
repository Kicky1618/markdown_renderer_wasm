#[path = "../src/code.rs"]
mod code;

use code::TokenKind;

fn tokens(source: &str) -> Vec<(String, TokenKind)> {
    let mut out = Vec::new();
    code::highlight(source, Some("WebAssembly Text Format"), |text, kind| {
        out.push((text.to_owned(), kind));
        true
    });
    out
}

#[test]
fn wat_double_semicolon_is_a_line_comment() {
    assert_eq!(
        tokens(";; line comment"),
        vec![(";; line comment".to_owned(), TokenKind::Comment)]
    );
}

#[test]
fn wat_single_semicolon_is_not_a_line_comment() {
    assert!(
        tokens("; not-a-comment")
            .iter()
            .all(|(_, kind)| *kind != TokenKind::Comment)
    );
}

#[test]
fn wat_nested_block_comments_stay_one_comment_token() {
    let source = "(; outer (; inner ;) tail ;)";
    assert_eq!(
        tokens(source),
        vec![(source.to_owned(), TokenKind::Comment)]
    );
}

#[test]
fn wat_block_comment_does_not_swallow_following_code() {
    let got = tokens("(; note ;) (module (func))");
    assert_eq!(got[0], ("(; note ;)".to_owned(), TokenKind::Comment));
    assert!(
        got.iter()
            .any(|(text, kind)| text == "module" && *kind == TokenKind::Keyword)
    );
    assert!(
        got.iter()
            .any(|(text, kind)| text == "func" && *kind == TokenKind::Keyword)
    );
}
