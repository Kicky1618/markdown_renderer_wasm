use crate::parser::Inline;

/// Deliberately single-pass and bounded: malformed delimiters never backtrack.
pub(crate) fn parse_inlines(source: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    let b = source.as_bytes();

    // Once a delimiter search fails for the current suffix, it cannot succeed
    // at any later byte offset. Remember that fact to keep malformed or
    // half-streamed Markdown from repeatedly scanning the same suffix.
    let mut no_citation_close = false;
    let mut no_inline_math_close = false;
    let mut no_display_math_close = false;
    let mut no_code_close = false;
    let mut no_strong_star_close = false;
    let mut no_strong_underscore_close = false;
    let mut no_em_star_close = false;
    let mut no_em_underscore_close = false;
    let mut no_link_label_close = false;
    let mut no_link_destination_close = false;

    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() && is_punctuation(b[i + 1]) {
            plain.push(b[i + 1] as char);
            i += 2;
            continue;
        }

        // LLM/RAG citation. It is normalized to the existing Link AST so old
        // renderers can display it without learning a new inline node.
        if b[i] == b'['
            && source[i..].starts_with("[[cite:")
            && let Some((end, citation_source, label)) =
                scan_llm_citation(source, i, &mut no_citation_close)
        {
            flush(&mut plain, &mut out);
            let visible = label.unwrap_or(citation_source);
            out.push(Inline::Link {
                label: vec![Inline::Text(visible.to_owned())],
                destination: format!("llm:cite:{citation_source}"),
            });
            i = end;
            continue;
        }

        if b[i] == b'$' {
            let display = b.get(i + 1) == Some(&b'$');
            let delimiter_len = if display { 2 } else { 1 };
            let body_start = i + delimiter_len;
            let (delimiter, exhausted) = if display {
                ("$$", &mut no_display_math_close)
            } else {
                ("$", &mut no_inline_math_close)
            };
            if let Some(end) = find_cached(source, body_start, delimiter, exhausted) {
                flush(&mut plain, &mut out);
                out.push(Inline::Math {
                    source: source[body_start..end].to_owned(),
                    display,
                });
                i = end + delimiter_len;
                continue;
            }
        }

        if b[i] == b'\n' {
            let hard = plain
                .as_bytes()
                .iter()
                .rev()
                .take_while(|&&byte| byte == b' ')
                .count()
                >= 2;
            if hard {
                let trimmed = plain.trim_end_matches(' ');
                plain.truncate(trimmed.len());
            }
            flush(&mut plain, &mut out);
            if i + 1 < b.len() {
                out.push(if hard {
                    Inline::HardBreak
                } else {
                    Inline::SoftBreak
                });
            }
            i += 1;
            continue;
        }

        if b[i] == b'`'
            && let Some(end) = find_cached(source, i + 1, "`", &mut no_code_close)
        {
            flush(&mut plain, &mut out);
            out.push(Inline::Code(source[i + 1..end].to_owned()));
            i = end + 1;
            continue;
        }

        if i + 1 < b.len()
            && ((b[i] == b'*' && b[i + 1] == b'*') || (b[i] == b'_' && b[i + 1] == b'_'))
        {
            let (delim, exhausted) = if b[i] == b'*' {
                ("**", &mut no_strong_star_close)
            } else {
                ("__", &mut no_strong_underscore_close)
            };
            if let Some(end) = find_cached(source, i + 2, delim, exhausted) {
                flush(&mut plain, &mut out);
                out.push(Inline::Strong(parse_inlines(&source[i + 2..end])));
                i = end + 2;
                continue;
            }
        }

        if b[i] == b'*' || b[i] == b'_' {
            let (delim, exhausted) = if b[i] == b'*' {
                ("*", &mut no_em_star_close)
            } else {
                ("_", &mut no_em_underscore_close)
            };
            if let Some(end) = find_cached(source, i + 1, delim, exhausted) {
                flush(&mut plain, &mut out);
                out.push(Inline::Emphasis(parse_inlines(&source[i + 1..end])));
                i = end + 1;
                continue;
            }
        }

        // General machine-readable reference. The visible text remains the
        // literal token while the destination carries structured metadata.
        if b[i] == b'@'
            && b.get(i + 1) == Some(&b'[')
            && let Some((end, kind, id)) = scan_llm_reference(source, i)
        {
            flush(&mut plain, &mut out);
            let label = source[i..end].to_owned();
            out.push(Inline::Link {
                label: vec![Inline::Text(label)],
                destination: format!("llm:{kind}:{id}"),
            });
            i = end;
            continue;
        }

        if b[i] == b'['
            && let Some(close) = find_cached(source, i + 1, "](", &mut no_link_label_close)
        {
            if let Some(end) = find_cached(source, close + 2, ")", &mut no_link_destination_close) {
                flush(&mut plain, &mut out);
                out.push(Inline::Link {
                    label: parse_inlines(&source[i + 1..close]),
                    destination: source[close + 2..end].to_owned(),
                });
                i = end + 1;
                continue;
            }
            // If there is no ')' after the earliest ](, no later '[' can
            // form a complete link either. Avoid rescanning the same suffix.
            no_link_label_close = true;
        }

        let ch = source[i..].chars().next().unwrap();
        plain.push(ch);
        i += ch.len_utf8();
    }

    flush(&mut plain, &mut out);
    out
}

fn find_cached(source: &str, start: usize, needle: &str, exhausted: &mut bool) -> Option<usize> {
    if *exhausted {
        return None;
    }
    match source[start..].find(needle) {
        Some(offset) => Some(start + offset),
        None => {
            *exhausted = true;
            None
        }
    }
}

fn flush(plain: &mut String, out: &mut Vec<Inline>) {
    if !plain.is_empty() {
        out.push(Inline::Text(std::mem::take(plain)));
    }
}

fn is_punctuation(b: u8) -> bool {
    matches!(b, b'!'..=b'/' | b':'..=b'@' | b'['..=b'`' | b'{'..=b'~')
}

fn scan_llm_citation<'a>(
    source: &'a str,
    start: usize,
    no_close: &mut bool,
) -> Option<(usize, &'a str, Option<&'a str>)> {
    const PREFIX: &str = "[[cite:";
    let bytes = source.as_bytes();
    let mut i = start + PREFIX.len();
    let source_start = i;

    while i < bytes.len() {
        match bytes[i] {
            b'[' => return None,
            b']' if bytes.get(i + 1) == Some(&b']') => {
                let citation_source = source[source_start..i].trim();
                if !valid_reference_atom(citation_source) {
                    return None;
                }
                return Some((i + 2, citation_source, None));
            }
            b']' => return None,
            b'|' => {
                let citation_source = source[source_start..i].trim();
                if !valid_reference_atom(citation_source) {
                    return None;
                }
                let label_start = i + 1;
                let close = find_cached(source, label_start, "]]", no_close)?;
                let label = source[label_start..close].trim();
                return Some((
                    close + 2,
                    citation_source,
                    (!label.is_empty()).then_some(label),
                ));
            }
            byte if byte.is_ascii_control() => return None,
            _ => i += 1,
        }
    }
    None
}

fn scan_llm_reference(source: &str, start: usize) -> Option<(usize, &str, &str)> {
    let bytes = source.as_bytes();
    let mut i = start + 2;
    let kind_start = i;

    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'-')) {
        i += 1;
    }
    if i == kind_start || bytes.get(i) != Some(&b':') {
        return None;
    }
    let kind = &source[kind_start..i];
    i += 1;
    let id_start = i;

    while i < bytes.len() {
        match bytes[i] {
            b']' if i != id_start => return Some((i + 1, kind, &source[id_start..i])),
            b'[' | b'|' => return None,
            byte if byte.is_ascii_control() || byte.is_ascii_whitespace() => return None,
            _ => i += 1,
        }
    }
    None
}

fn valid_reference_atom(value: &str) -> bool {
    !value.is_empty()
        && !value.bytes().any(|b| {
            b.is_ascii_control() || b.is_ascii_whitespace() || matches!(b, b'[' | b']' | b'|')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_citation_reuses_link_ast() {
        assert_eq!(
            parse_inlines("Fact [[cite:doc-42|spec]]."),
            vec![
                Inline::Text("Fact ".to_owned()),
                Inline::Link {
                    label: vec![Inline::Text("spec".to_owned())],
                    destination: "llm:cite:doc-42".to_owned(),
                },
                Inline::Text(".".to_owned()),
            ]
        );
        assert_eq!(
            parse_inlines("[[cite:turn7search2]]"),
            vec![Inline::Link {
                label: vec![Inline::Text("turn7search2".to_owned())],
                destination: "llm:cite:turn7search2".to_owned(),
            }]
        );
    }

    #[test]
    fn malformed_citation_stays_text() {
        assert_eq!(
            parse_inlines("[[cite:bad id]]"),
            vec![Inline::Text("[[cite:bad id]]".to_owned())]
        );
    }

    #[test]
    fn llm_semantic_reference_reuses_link_ast() {
        assert_eq!(
            parse_inlines("See @[source:turn7search2]."),
            vec![
                Inline::Text("See ".to_owned()),
                Inline::Link {
                    label: vec![Inline::Text("@[source:turn7search2]".to_owned())],
                    destination: "llm:source:turn7search2".to_owned(),
                },
                Inline::Text(".".to_owned()),
            ]
        );
    }

    #[test]
    fn malformed_llm_reference_stays_text() {
        assert_eq!(
            parse_inlines("@[bad kind:id]"),
            vec![Inline::Text("@[bad kind:id]".to_owned())]
        );
    }

    #[test]
    fn malformed_outer_llm_tokens_do_not_hide_valid_inner_tokens() {
        assert_eq!(
            parse_inlines("@[bad @[source:ok]"),
            vec![
                Inline::Text("@[bad ".to_owned()),
                Inline::Link {
                    label: vec![Inline::Text("@[source:ok]".to_owned())],
                    destination: "llm:source:ok".to_owned(),
                },
            ]
        );
        assert_eq!(
            parse_inlines("[[cite:bad [[cite:doc]]"),
            vec![
                Inline::Text("[[cite:bad ".to_owned()),
                Inline::Link {
                    label: vec![Inline::Text("doc".to_owned())],
                    destination: "llm:cite:doc".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn malformed_link_without_destination_close_stays_text() {
        assert_eq!(
            parse_inlines("[[[label]("),
            vec![Inline::Text("[[[label](".to_owned())]
        );
    }
}
