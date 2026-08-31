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

fn assert_single_string(source: &str, language: &str) {
    assert_eq!(
        tokens(source, language),
        vec![(source.to_owned(), TokenKind::String)],
        "language={language} source={source:?}",
    );
}

#[test]
fn double_quoted_here_strings_are_single_tokens() {
    assert_single_string("@\"\nhello $name\n\"@", "powershell");
    assert_single_string("@\"\r\nhello\r\n\"@", "pwsh");
}

#[test]
fn single_quoted_here_strings_are_single_tokens() {
    assert_single_string("@'\nhello $name\n'@", "powershell");
    assert_single_string("@'\r\nhello\r\n'@", "ps1");
}

#[test]
fn closing_marker_must_be_at_line_start() {
    let source = "@\"\ntext \"@ is content\n  \"@ still content\n\"@";
    assert_single_string(source, "powershell");
}

#[test]
fn closing_marker_must_fill_the_line() {
    let source = "@'\n'@ trailing is content\n'@";
    assert_single_string(source, "powershell");
}

#[test]
fn opener_requires_an_immediate_newline() {
    let source = "@\"not a here string\"\nbody\n\"@";
    assert_ne!(
        tokens(source, "powershell"),
        vec![(source.to_owned(), TokenKind::String)],
    );
}

#[test]
fn here_string_syntax_is_powershell_scoped() {
    for language in ["objectivec", "csharp", "javascript"] {
        let source = "@\"\nhello\n\"@";
        let highlighted = tokens(source, language);
        assert_ne!(
            highlighted,
            vec![(source.to_owned(), TokenKind::String)],
            "language={language}"
        );
    }
}

#[test]
fn block_comments_still_work() {
    assert_eq!(
        tokens("<# comment #>", "powershell"),
        vec![("<# comment #>".to_owned(), TokenKind::Comment)],
    );
}
