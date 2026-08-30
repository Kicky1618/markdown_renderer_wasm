use streamdown::{Block, Inline, Parser};

fn main() {
    let mut parser = Parser::new();
    for chunk in [
        "結論は [[cite:spec-42|仕様書]] を参照。\n\n",
        ":::llm tool name=search id=q1\n",
        "{\"query\":\"streaming markdown\"}",
        "\n:::\n\n生成物は @[artifact:plot-1]。",
    ] {
        let delta = parser.append(chunk);
        println!("delta ops: {}", delta.ops.len());
    }
    parser.finish();

    for (index, block) in parser.blocks().iter().enumerate() {
        match block {
            Block::CodeBlock {
                language: Some(language),
                text,
                closed,
            } if language == "llm" || language.starts_with("llm:") => {
                println!("llm block #{index}: {language} closed={closed} body={text:?}");
            }
            Block::Paragraph(inlines) => visit_inlines(index, inlines),
            _ => {}
        }
    }
}

fn visit_inlines(block: usize, inlines: &[Inline]) {
    for inline in inlines {
        match inline {
            Inline::Link { label, destination } if destination.starts_with("llm:") => {
                let label = label
                    .iter()
                    .filter_map(|node| match node {
                        Inline::Text(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>();
                println!("llm ref in block #{block}: {destination} label={label:?}");
            }
            Inline::Emphasis(children) | Inline::Strong(children) => visit_inlines(block, children),
            Inline::Link { label, .. } => visit_inlines(block, label),
            _ => {}
        }
    }
}
