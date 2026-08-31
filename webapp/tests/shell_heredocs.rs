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

fn strings(source: &str, language: &str) -> Vec<String> {
    tokens(source, language)
        .into_iter()
        .filter_map(|(text, kind)| (kind == TokenKind::String).then_some(text))
        .collect()
}

#[test]
fn basic_shell_heredoc_body_is_one_string_token() {
    let source = "cat <<EOF\nhello $USER\nsecond line\nEOF\nprintf done\n";
    assert_eq!(
        strings(source, "bash"),
        vec!["hello $USER\nsecond line\nEOF\n".to_owned()]
    );
}

#[test]
fn quoted_delimiters_are_supported() {
    for (source, expected_body) in [
        ("cat <<'EOF'\nliteral $USER\nEOF\n", "literal $USER\nEOF\n"),
        ("cat <<\"EOF\"\ndouble quoted delimiter\nEOF\n", "double quoted delimiter\nEOF\n"),
        ("cat <<\\EOF\nbackslash quoted delimiter\nEOF\n", "backslash quoted delimiter\nEOF\n"),
    ] {
        let got = strings(source, "sh");
        assert_eq!(
            got.last().map(String::as_str),
            Some(expected_body),
            "source={source:?}: {got:?}"
        );
    }
}

#[test]
fn delimiter_may_be_separated_from_operator_by_spaces() {
    let source = "cat <<   EOF\nbody\nEOF\n";
    assert_eq!(strings(source, "shell"), vec!["body\nEOF\n".to_owned()]);
}

#[test]
fn dash_heredoc_accepts_tab_indented_terminator() {
    let source = "cat <<-EOF\n\tbody\n\tEOF\nprintf done\n";
    assert_eq!(
        strings(source, "zsh"),
        vec!["\tbody\n\tEOF\n".to_owned()]
    );
}

#[test]
fn ordinary_heredoc_does_not_accept_indented_terminator() {
    let source = "cat <<EOF\nbody\n\tEOF\nstill body\nEOF\n";
    assert_eq!(
        strings(source, "bash"),
        vec!["body\n\tEOF\nstill body\nEOF\n".to_owned()]
    );
}

#[test]
fn terminator_must_fill_its_line() {
    let source = "cat <<EOF\nbody\nEOF suffix\nstill body\nEOF\n";
    assert_eq!(
        strings(source, "bash"),
        vec!["body\nEOF suffix\nstill body\nEOF\n".to_owned()]
    );
}

#[test]
fn command_tail_after_declaration_is_not_colored_as_string() {
    let source = "cat <<EOF >output\nbody\nEOF\n";
    let got = tokens(source, "bash");
    let string = got
        .iter()
        .find(|(_, kind)| *kind == TokenKind::String)
        .expect("heredoc body string");
    assert_eq!(string.0, "body\nEOF\n");
    assert!(got.iter().any(|(text, kind)| text == ">" && *kind == TokenKind::Operator));
    assert!(got.iter().any(|(text, _)| text == "output"));
}

#[test]
fn multiple_heredocs_on_one_command_are_queued() {
    let source = "cat <<A <<B\nfirst\nA\nsecond\nB\n";
    assert_eq!(
        strings(source, "bash"),
        vec!["first\nA\n".to_owned(), "second\nB\n".to_owned()]
    );
}

#[test]
fn crlf_heredocs_are_supported() {
    let source = "cat <<EOF\r\nbody\r\nEOF\r\nprintf done\r\n";
    assert_eq!(
        strings(source, "bash"),
        vec!["body\r\nEOF\r\n".to_owned()]
    );
}

#[test]
fn incomplete_heredoc_runs_to_end_of_stream() {
    let source = "cat <<EOF\npartial\nstill partial";
    assert_eq!(
        strings(source, "bash"),
        vec!["partial\nstill partial".to_owned()]
    );
}

#[test]
fn triple_less_than_is_not_treated_as_heredoc() {
    let source = "cat <<< value\nnext line\n";
    assert!(strings(source, "bash").is_empty());
}

#[test]
fn heredoc_syntax_is_shell_scoped() {
    let source = "value << EOF\nnot a string\nEOF\n";
    assert!(strings(source, "cpp").is_empty());
}
