#[path = "../src/code.rs"]
mod code;

use code::TokenKind;

fn tokens(source: &str, language: &str) -> Vec<(String, TokenKind)> {
    let mut tokens = Vec::new();
    code::highlight(source, Some(language), |text, kind| {
        tokens.push((text.to_owned(), kind));
        true
    });
    tokens
}

fn has(tokens: &[(String, TokenKind)], text: &str, kind: TokenKind) -> bool {
    tokens.iter().any(|token| token == &(text.to_owned(), kind))
}

#[test]
fn numbers_stop_at_operators_and_keep_exponents() {
    let tokens = tokens("let value = 1+2.5e-3; let range = 0..10;", "rust");
    assert!(has(&tokens, "1", TokenKind::Number));
    assert!(has(&tokens, "+", TokenKind::Operator));
    assert!(has(&tokens, "2.5e-3", TokenKind::Number));
    assert!(has(&tokens, "0", TokenKind::Number));
    assert!(has(&tokens, "10", TokenKind::Number));
}

#[test]
fn comment_markers_follow_the_language() {
    let python = tokens("value = left // right  # floor division", "python");
    assert!(has(&python, "//", TokenKind::Operator));
    assert!(has(&python, "# floor division", TokenKind::Comment));

    let rust = tokens("let url = value; // comment", "rust");
    assert!(has(&rust, "// comment", TokenKind::Comment));
}

#[test]
fn rust_lifetimes_chars_and_raw_strings_are_distinct() {
    let tokens = tokens(
        "fn borrow<'a>(x: &'a str) { let c = 'x'; let s = r#\"a\\b\"#; }",
        "rust",
    );
    assert!(has(&tokens, "'", TokenKind::Operator));
    assert!(has(&tokens, "a", TokenKind::Plain));
    assert!(has(&tokens, "'x'", TokenKind::String));
    assert!(has(&tokens, "r#\"a\\b\"#", TokenKind::String));
}

#[test]
fn python_prefixed_triple_strings_and_decorators_stay_whole() {
    let tokens = tokens(
        "@dataclass\ntext = f\"\"\"hello {name}\nworld\"\"\"",
        "python",
    );
    assert!(has(&tokens, "@dataclass", TokenKind::Macro));
    assert!(has(
        &tokens,
        "f\"\"\"hello {name}\nworld\"\"\"",
        TokenKind::String
    ));
}

#[test]
fn javascript_regex_is_not_confused_with_division() {
    let tokens = tokens(
        "const re = /[a-z\\/]+/gi; const ratio = left / right; return /ok/;",
        "javascript",
    );
    assert!(has(&tokens, "/[a-z\\/]+/gi", TokenKind::String));
    assert!(has(&tokens, "/", TokenKind::Operator));
    assert!(has(&tokens, "/ok/", TokenKind::String));
}

#[test]
fn declaration_states_capture_function_and_type_names() {
    let rust = tokens("fn render() {} struct Document; render();", "rust");
    assert!(has(&rust, "render", TokenKind::Function));
    assert!(has(&rust, "Document", TokenKind::Type));

    let python = tokens(
        "def render_page(): pass\nclass document_model: pass",
        "python3",
    );
    assert!(has(&python, "render_page", TokenKind::Function));
    assert!(has(&python, "document_model", TokenKind::Type));
}

#[test]
fn nested_rust_block_comments_use_external_scanner_semantics() {
    let source = "/* outer /* nested */ still outer */ let value = 1;";
    let tokens = tokens(source, "rust");
    assert!(has(
        &tokens,
        "/* outer /* nested */ still outer */",
        TokenKind::Comment
    ));
    assert_eq!(
        tokens
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<String>(),
        source
    );
}

#[test]
fn streaming_input_uses_contextual_parser_and_recovers_at_eof() {
    let source = "fn render() { /* unfinished\ncomment";
    let tokens = tokens(source, "rust");
    assert!(has(&tokens, "fn", TokenKind::Keyword));
    assert!(has(&tokens, "render", TokenKind::Function));
    assert!(has(&tokens, "/* unfinished\ncomment", TokenKind::Comment));
    assert_eq!(
        tokens
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<String>(),
        source
    );
}

#[test]
fn builtins_constants_preprocessors_and_go_keywords_are_classified() {
    let rust = tokens("const MAX_RETRIES: usize = 3; struct User;", "rust");
    assert!(has(&rust, "MAX_RETRIES", TokenKind::Plain));
    assert!(has(&rust, "usize", TokenKind::Type));
    assert!(has(&rust, "User", TokenKind::Type));

    let cpp = tokens("  #include <vector>\nint main() {}", "cpp");
    assert!(has(&cpp, "#include", TokenKind::Macro));
    assert!(has(&cpp, "<vector>", TokenKind::String));
    assert!(has(&cpp, "int", TokenKind::Type));

    let go = tokens("func send(ch chan int) { defer close(ch) }", "go");
    assert!(has(&go, "func", TokenKind::Keyword));
    assert!(has(&go, "chan", TokenKind::Keyword));
    assert!(has(&go, "defer", TokenKind::Keyword));
}

#[test]
fn rust_macro_invocations_declarations_and_attributes_are_contextual() {
    let source = r#"#[derive(Debug, Clone)]
#[tokio::main]
async fn main() {
    println ! ("hello");
    let values = vec![1, 2];
}
macro_rules! collect_items {
    ($($item:item),*) => { $crate::consume!($($item),*) }
}
macro generated($item:item) {}
"#;
    let tokens = tokens(source, "rust");

    for name in [
        "derive",
        "tokio",
        "main",
        "println",
        "vec",
        "macro_rules",
        "collect_items",
        "generated",
    ] {
        assert!(
            has(&tokens, name, TokenKind::Macro),
            "missing macro: {name}"
        );
    }
    assert!(has(&tokens, "Debug", TokenKind::Type));
    assert!(has(&tokens, "Clone", TokenKind::Type));
    assert!(has(&tokens, "main", TokenKind::Function));
    assert!(has(&tokens, "$item", TokenKind::Macro));
    assert!(has(&tokens, "$crate", TokenKind::Macro));
    assert!(has(&tokens, "$", TokenKind::Macro));
    assert_eq!(
        tokens
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<String>(),
        source
    );
}

#[test]
fn c_preprocessor_highlights_structure_instead_of_the_whole_line() {
    let source = "# include <stdio.h>\n#define MAX(a, b) ((a) > (b) ? (a) : (b))\n#if defined(FeatureFlag) && __has_include(<stdint.h>)\nint value = MAX(1, 2);\n#endif";
    let tokens = tokens(source, "cxx");

    assert!(has(&tokens, "# include", TokenKind::Macro));
    assert!(has(&tokens, "<stdio.h>", TokenKind::String));
    assert!(has(&tokens, "#define", TokenKind::Macro));
    assert!(has(&tokens, "#if", TokenKind::Macro));
    assert!(has(&tokens, "#endif", TokenKind::Macro));
    assert!(has(&tokens, "MAX", TokenKind::Macro));
    assert!(has(&tokens, "FeatureFlag", TokenKind::Macro));
    assert!(has(&tokens, "defined", TokenKind::Macro));
    assert!(has(&tokens, "__has_include", TokenKind::Macro));
    assert!(has(&tokens, "<stdint.h>", TokenKind::String));
    assert!(has(&tokens, "int", TokenKind::Type));
    assert!(has(&tokens, "1", TokenKind::Number));
    assert_eq!(
        tokens
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<String>(),
        source
    );
}

#[test]
fn incomplete_streaming_macros_recover_without_losing_captures() {
    let rust_source = "#[derive(Debug\nmacro_rules! pending { ($value:expr) => { println!(\"";
    let rust = tokens(rust_source, "rust");
    assert!(has(&rust, "derive", TokenKind::Macro));
    assert!(has(&rust, "macro_rules", TokenKind::Macro));
    assert!(has(&rust, "pending", TokenKind::Macro));
    assert!(has(&rust, "$value", TokenKind::Macro));
    assert!(has(&rust, "println", TokenKind::Macro));
    assert_eq!(
        rust.iter()
            .map(|(text, _)| text.as_str())
            .collect::<String>(),
        rust_source
    );

    let c_source = "#include <unfinished/header\n#define Pending(value";
    let c = tokens(c_source, "cpp");
    assert!(has(&c, "<unfinished/header", TokenKind::String));
    assert!(has(&c, "Pending", TokenKind::Macro));
    assert_eq!(
        c.iter().map(|(text, _)| text.as_str()).collect::<String>(),
        c_source
    );
}

#[test]
fn virtualized_highlighting_stops_after_the_visible_range() {
    let mut emitted = 0;
    code::highlight(
        "let first = 1;\nlet second = 2;\nlet third = 3;",
        Some("rust"),
        |_, _| {
            emitted += 1;
            false
        },
    );
    assert_eq!(emitted, 1);
}
