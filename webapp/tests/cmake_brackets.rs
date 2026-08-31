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

#[test]
fn cmake_bracket_arguments_are_single_string_tokens() {
    for source in [
        "[[first\nsecond]]",
        "[=[text with ]] inside]=]",
        "[==[text with ]=] inside]==]",
    ] {
        assert_eq!(
            tokens(source, "cmake"),
            vec![(source.to_owned(), TokenKind::String)],
            "source={source:?}",
        );
    }
}

#[test]
fn cmake_bracket_comments_preempt_hash_line_comments() {
    for source in [
        "#[[first\nsecond]]",
        "#[=[comment with ]] inside]=]",
        "#[==[comment with ]=] inside]==]",
    ] {
        assert_eq!(
            tokens(source, "cmake"),
            vec![(source.to_owned(), TokenKind::Comment)],
            "source={source:?}",
        );
    }
}

#[test]
fn cmake_bracket_comment_stops_at_matching_delimiter() {
    let got = tokens("#[=[comment]=] set(X 1)", "cmake");
    assert_eq!(got[0], ("#[=[comment]=]".to_owned(), TokenKind::Comment));
    assert!(got
        .iter()
        .any(|(text, kind)| text == "set" && *kind == TokenKind::Keyword));
}

#[test]
fn ordinary_cmake_hash_comments_are_preserved() {
    assert_eq!(
        tokens("# ordinary comment", "cmake"),
        vec![("# ordinary comment".to_owned(), TokenKind::Comment)],
    );
}

#[test]
fn ordinary_brackets_are_not_cmake_strings() {
    let got = tokens("list[index]", "cmake");
    assert!(got.iter().all(|(_, kind)| *kind != TokenKind::String));
}

#[test]
fn cmake_brackets_are_language_scoped() {
    let got = tokens("[=[value]=]", "javascript");
    assert!(got.iter().all(|(_, kind)| *kind != TokenKind::String));

    let got = tokens("#[=[comment]=]", "rust");
    assert!(got.iter().all(|(_, kind)| *kind != TokenKind::Comment));
}
