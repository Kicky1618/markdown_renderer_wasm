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
fn c_family_prefixed_strings_stay_whole() {
    assert_single_string("$\"hello {name}\\n\"", "csharp");
    assert_single_string("@\"C:\\\\tmp\\\\file \"\"quoted\"\"\"", "csharp");
    assert_single_string("$@\"C:\\\\tmp\\\\{name}\"", "cs");
    assert_single_string("@$\"C:\\\\tmp\\\\{name}\"", "c#");
    assert_single_string("@\"Objective-C NSString\"", "objective-c");
}

#[test]
fn cpp_raw_strings_accept_custom_delimiters_and_prefixes() {
    assert_single_string("R\"tag(a \\\"quote\\\" and \\n)tag\"", "cpp");
    assert_single_string("u8R\"json({\\\"key\\\": 1})json\"", "c++");
    assert_single_string("LR\"x(wide raw)x\"", "cuda");
}

#[test]
fn hash_delimited_raw_strings_cover_swift_multiline_forms() {
    assert_single_string("#\"raw \\\\ literal\"#", "swift");
    assert_single_string("##\"contains #\" safely\"##", "swift");
    assert_single_string("#\"\"\"first\\nsecond\"\"\"#", "swift");
}

#[test]
fn ordinary_hash_comment_languages_do_not_turn_hash_quotes_into_strings() {
    let python = tokens("#\"still a comment\"", "python");
    assert_eq!(
        python,
        vec![("#\"still a comment\"".to_owned(), TokenKind::Comment)]
    );
}
