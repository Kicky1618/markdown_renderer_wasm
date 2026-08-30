use crate::parser::Inline;

/// Deliberately single-pass and bounded: malformed delimiters never backtrack.
pub(crate) fn parse_inlines(source: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    let b = source.as_bytes();

    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() && is_punctuation(b[i + 1]) {
            plain.push(b[i + 1] as char);
            i += 2;
            continue;
        }

        // LLM/RAG citation. It is normalized to the existing Link AST so old
        // renderers can display it without learning a new inline node.
        if b[i] == b'['
            && b.get(i + 1) == Some(&b'[')
            && let Some(rel_end) = source[i + 2..].find("]]")
        {
            let end = i + 2 + rel_end;
            if let Some((citation_source, label)) = llm_citation(&source[i + 2..end]) {
                flush(&mut plain, &mut out);
                let visible = label.unwrap_or(citation_source);
                out.push(Inline::Link {
                    label: vec![Inline::Text(visible.to_owned())],
                    destination: format!("llm:cite:{citation_source}"),
                });
                i = end + 2;
                continue;
            }
        }

        if b[i] == b'$' {
            let display = b.get(i + 1) == Some(&b'$');
            let delimiter_len = if display { 2 } else { 1 };
            let body_start = i + delimiter_len;
            let delimiter = if display { "$$" } else { "$" };
            if let Some(end) = source[body_start..]
                .find(delimiter)
                .map(|offset| body_start + offset)
            {
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
            && let Some(end) = source[i + 1..].find('`').map(|x| i + 1 + x)
        {
            flush(&mut plain, &mut out);
            out.push(Inline::Code(source[i + 1..end].to_owned()));
            i = end + 1;
            continue;
        }

        if i + 1 < b.len()
            && ((b[i] == b'*' && b[i + 1] == b'*') || (b[i] == b'_' && b[i + 1] == b'_'))
        {
            let delim = &source[i..i + 2];
            if let Some(end) = source[i + 2..].find(delim).map(|x| i + 2 + x) {
                flush(&mut plain, &mut out);
                out.push(Inline::Strong(parse_inlines(&source[i + 2..end])));
                i = end + 2;
                continue;
            }
        }

        if b[i] == b'*' || b[i] == b'_' {
            let delim = b[i] as char;
            if let Some(end) = source[i + 1..].find(delim).map(|x| i + 1 + x) {
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
            && let Some(rel_end) = source[i + 2..].find(']')
        {
            let end = i + 2 + rel_end;
            if let Some((kind, id)) = llm_reference(&source[i + 2..end]) {
                flush(&mut plain, &mut out);
                let label = source[i..=end].to_owned();
                out.push(Inline::Link {
                    label: vec![Inline::Text(label)],
                    destination: format!("llm:{kind}:{id}"),
                });
                i = end + 1;
                continue;
            }
        }

        if b[i] == b'['
            && let Some(close) = source[i + 1..].find("](").map(|x| i + 1 + x)
            && let Some(end) = source[close + 2..].find(')').map(|x| close + 2 + x)
        {
            flush(&mut plain, &mut out);
            out.push(Inline::Link {
                label: parse_inlines(&source[i + 1..close]),
                destination: source[close + 2..end].to_owned(),
            });
            i = end + 1;
            continue;
        }

        let ch = source[i..].chars().next().unwrap();
        plain.push(ch);
        i += ch.len_utf8();
    }

    flush(&mut plain, &mut out);
    out
}

fn flush(plain: &mut String, out: &mut Vec<Inline>) {
    if !plain.is_empty() {
        out.push(Inline::Text(std::mem::take(plain)));
    }
}

fn is_punctuation(b: u8) -> bool {
    matches!(b, b'!'..=b'/' | b':'..=b'@' | b'['..=b'`' | b'{'..=b'~')
}

fn llm_citation(body: &str) -> Option<(&str, Option<&str>)> {
    let body = body.strip_prefix("cite:")?;
    let (source, label) = match body.split_once('|') {
        Some((source, label)) => (source.trim(), Some(label.trim())),
        None => (body.trim(), None),
    };
    if !valid_reference_atom(source) {
        return None;
    }
    Some((source, label.filter(|label| !label.is_empty())))
}

fn llm_reference(body: &str) -> Option<(&str, &str)> {
    let (kind, id) = body.split_once(':')?;
    if kind.is_empty()
        || !kind
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        || !valid_reference_atom(id)
    {
        return None;
    }
    Some((kind, id))
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
}
