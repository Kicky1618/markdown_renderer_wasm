use ratex_parser::parse;
use streamdown::{Block, Inline, Parser};

fn main() {
    check("easy_test.md", None);
    check("math_stress_test.md", Some("### 4."));
}

fn check(path: &str, stop_at: Option<&str>) {
    let source = std::fs::read_to_string(path).unwrap();
    let source = stop_at
        .and_then(|marker| source.find(marker).map(|end| &source[..end]))
        .unwrap_or(&source);
    let mut markdown = Parser::new();
    markdown.append(source);
    let mut formulas = Vec::new();
    for block in markdown.blocks() {
        match block {
            Block::Paragraph(nodes)
            | Block::Heading { content: nodes, .. }
            | Block::BlockQuote(nodes) => collect(nodes, &mut formulas),
            Block::UnorderedList(items) | Block::OrderedList { items, .. } => {
                for nodes in items {
                    collect(nodes, &mut formulas);
                }
            }
            Block::CodeBlock { .. } | Block::ThematicBreak | Block::Table { .. } => {}
        }
    }
    let mut failures = 0;
    for (index, formula) in formulas.iter().enumerate() {
        if let Err(error) = parse(formula) {
            failures += 1;
            eprintln!("{path}: formula {index}: {error}\n{formula}");
        }
    }
    println!("{path}: {} formulas, {failures} failures", formulas.len());
    assert_eq!(failures, 0);
}

fn collect<'a>(nodes: &'a [Inline], out: &mut Vec<&'a str>) {
    for node in nodes {
        match node {
            Inline::Math { source, .. } => out.push(source),
            Inline::Emphasis(children) | Inline::Strong(children) => collect(children, out),
            Inline::Link { label, .. } => collect(label, out),
            Inline::Text(_) | Inline::Code(_) | Inline::SoftBreak | Inline::HardBreak => {}
        }
    }
}
