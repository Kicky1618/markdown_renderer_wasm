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
fn lua_long_strings_are_single_tokens() {
    for source in [
        "[[first\nsecond]]",
        "[=[text with ]] inside]=]",
        "[==[text with ]=] inside]==]",
    ] {
        assert_eq!(
            tokens(source, "lua"),
            vec![(source.to_owned(), TokenKind::String)],
            "source={source:?}",
        );
    }
}

#[test]
fn lua_long_comments_are_single_tokens() {
    for source in [
        "--[[first\nsecond]]",
        "--[=[comment with ]] inside]=]",
        "--[==[comment with ]=] inside]==]",
    ] {
        assert_eq!(
            tokens(source, "lua"),
            vec![(source.to_owned(), TokenKind::Comment)],
            "source={source:?}",
        );
    }
}

#[test]
fn lua_short_comment_behavior_is_preserved() {
    assert_eq!(
        tokens("-- ordinary comment", "lua"),
        vec![("-- ordinary comment".to_owned(), TokenKind::Comment)],
    );
    assert_eq!(
        tokens("--[not a long opener", "lua"),
        vec![("--[not a long opener".to_owned(), TokenKind::Comment)],
    );
}

#[test]
fn ordinary_brackets_are_not_long_strings() {
    let got = tokens("array[index]", "lua");
    assert!(!got.iter().any(|(_, kind)| *kind == TokenKind::String));
}

#[test]
fn long_brackets_are_lua_scoped() {
    let got = tokens("[[value]]", "javascript");
    assert!(!got.iter().any(|(_, kind)| *kind == TokenKind::String));
}
