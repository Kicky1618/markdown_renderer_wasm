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

fn has(tokens: &[(String, TokenKind)], text: &str, kind: TokenKind) -> bool {
    tokens.iter().any(|token| token == &(text.to_owned(), kind))
}

fn source(tokens: &[(String, TokenKind)]) -> String {
    tokens.iter().map(|(text, _)| text.as_str()).collect()
}

#[test]
fn japanese_fence_aliases_use_morphological_highlighting() {
    let input = "日本語を解析する。";
    for language in ["日本語", "japanese", "nihongo", "ja", "jp"] {
        let highlighted = tokens(input, language);
        assert_eq!(source(&highlighted), input, "{language}: {highlighted:?}");
        assert!(
            has(&highlighted, "日本語", TokenKind::Type),
            "{language}: {highlighted:?}"
        );
        assert!(
            has(&highlighted, "を", TokenKind::Keyword),
            "{language}: {highlighted:?}"
        );
        assert!(
            has(&highlighted, "解析", TokenKind::Type),
            "{language}: {highlighted:?}"
        );
        assert!(
            has(&highlighted, "する", TokenKind::Function),
            "{language}: {highlighted:?}"
        );
    }
}

#[test]
fn japanese_unknown_scripts_and_numbers_keep_useful_classes() {
    let input = "GPUで超高速化する 123";
    let highlighted = tokens(input, "ja");
    assert_eq!(source(&highlighted), input);
    assert!(
        has(&highlighted, "GPU", TokenKind::Plain),
        "{highlighted:?}"
    );
    assert!(
        has(&highlighted, "で", TokenKind::Keyword),
        "{highlighted:?}"
    );
    assert!(
        highlighted
            .iter()
            .any(|(text, kind)| text.contains("超高速化") && *kind == TokenKind::Type),
        "{highlighted:?}"
    );
    assert!(
        has(&highlighted, "123", TokenKind::Number),
        "{highlighted:?}"
    );
}

#[test]
fn japanese_highlighting_preserves_early_stop() {
    let mut calls = 0usize;
    code::highlight(
        "日本語を解析する。\n次の行です。",
        Some("ja"),
        |_, _| {
            calls += 1;
            false
        },
    );
    assert_eq!(calls, 1);
}

#[test]
fn japanese_line_cache_is_output_stable() {
    let input = "シンタックスハイライトを高速化する。\n同じ行を表示する。\n";
    let first = tokens(input, "japanese");
    let second = tokens(input, "japanese");
    assert_eq!(first, second);
    assert_eq!(source(&second), input);
}

#[test]
fn ordinary_languages_do_not_enter_japanese_mode() {
    let highlighted = tokens("function 日本語() { return 1; }", "javascript");
    assert!(
        has(&highlighted, "function", TokenKind::Keyword),
        "{highlighted:?}"
    );
    assert!(
        !has(&highlighted, "日本語", TokenKind::Type),
        "{highlighted:?}"
    );
}
