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
fn pascal_brace_comments_are_supported() {
    assert_single_comment("{ ordinary Pascal comment }", "pascal");
    assert_single_comment("{ Delphi comment }", "objectpascal");
    assert_single_comment("(* existing Pascal comment *)", "pascal");
}

#[test]
fn powershell_angle_hash_block_comments_are_supported() {
    assert_single_comment("<# first\nsecond #>", "powershell");
    let tokens = tokens("<# comment #> function Test {}", "ps1");
    assert_eq!(tokens[0], ("<# comment #>".to_owned(), TokenKind::Comment));
    assert!(tokens
        .iter()
        .any(|(text, kind)| text == "function" && *kind == TokenKind::Keyword));
}

#[test]
fn scheme_hash_pipe_comments_nest() {
    assert_single_comment("#| outer #| inner |# outer |#", "scheme");
    assert_single_comment("#| racket #| nested |# comment |#", "racket");
}

#[test]
fn extension_comment_styles_are_language_scoped() {
    for source in ["{ not a Rust comment }", "<# not Rust #>", "#| not Rust |#"] {
        let highlighted = tokens(source, "rust");
        assert!(
            highlighted
                .iter()
                .all(|(_, kind)| *kind != TokenKind::Comment),
            "{source:?}"
        );
    }
}

#[test]
fn scheme_semicolon_line_comments_still_work() {
    assert_single_comment("; ordinary Scheme comment", "scheme");
}
