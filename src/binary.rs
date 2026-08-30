use crate::parser::{Block, Delta, Inline, Op};

/// Compact little-endian wire format. See FORMAT.md.
pub fn encode_delta(delta: &Delta) -> Vec<u8> {
    let mut w = Writer(Vec::with_capacity(64));
    w.0.extend_from_slice(b"MDA1");
    w.u32(delta.ops.len() as u32);
    for op in &delta.ops {
        match op {
            Op::Truncate { from } => {
                w.u8(1);
                w.u32(*from);
            }
            Op::Push(block) => {
                w.u8(2);
                w.block(block);
            }
            Op::SpliceCode {
                block,
                truncate_bytes,
                append,
            } => {
                w.u8(3);
                w.u32(*block);
                w.u32(*truncate_bytes);
                w.string(append);
            }
            Op::SealCode { block } => {
                w.u8(4);
                w.u32(*block);
            }
            Op::AppendText { block, append } => {
                w.u8(5);
                w.u32(*block);
                w.string(append);
            }
        }
    }
    w.0
}

struct Writer(Vec<u8>);
impl Writer {
    fn u8(&mut self, x: u8) {
        self.0.push(x);
    }
    fn u32(&mut self, x: u32) {
        self.0.extend_from_slice(&x.to_le_bytes());
    }
    fn string(&mut self, x: &str) {
        self.u32(x.len() as u32);
        self.0.extend_from_slice(x.as_bytes());
    }
    fn inlines(&mut self, xs: &[Inline]) {
        self.u32(xs.len() as u32);
        for x in xs {
            match x {
                Inline::Text(s) => {
                    self.u8(1);
                    self.string(s);
                }
                Inline::Emphasis(v) => {
                    self.u8(2);
                    self.inlines(v);
                }
                Inline::Strong(v) => {
                    self.u8(3);
                    self.inlines(v);
                }
                Inline::Code(s) => {
                    self.u8(4);
                    self.string(s);
                }
                Inline::Math { source, display } => {
                    self.u8(8);
                    self.u8(*display as u8);
                    self.string(source);
                }
                Inline::Link { label, destination } => {
                    self.u8(5);
                    self.inlines(label);
                    self.string(destination);
                }
                Inline::SoftBreak => self.u8(6),
                Inline::HardBreak => self.u8(7),
            }
        }
    }
    fn block(&mut self, b: &Block) {
        match b {
            Block::Paragraph(v) => {
                self.u8(1);
                self.inlines(v);
            }
            Block::Heading { level, content } => {
                self.u8(2);
                self.u8(*level);
                self.inlines(content);
            }
            Block::CodeBlock {
                language,
                text,
                closed,
            } => {
                self.u8(3);
                self.u8(*closed as u8);
                self.string(language.as_deref().unwrap_or(""));
                self.string(text);
            }
            Block::BlockQuote(v) => {
                self.u8(4);
                self.inlines(v);
            }
            Block::UnorderedList(items) => {
                self.u8(5);
                self.u32(items.len() as u32);
                for x in items {
                    self.inlines(x);
                }
            }
            Block::OrderedList { start, items } => {
                self.u8(6);
                self.u32(*start);
                self.u32(items.len() as u32);
                for x in items {
                    self.inlines(x);
                }
            }
            Block::ThematicBreak => self.u8(7),
            Block::Table { headers, rows } => {
                self.u8(8);
                self.u32(headers.len() as u32);
                for cell in headers {
                    self.inlines(cell);
                }
                self.u32(rows.len() as u32);
                for row in rows {
                    self.u32(row.len() as u32);
                    for cell in row {
                        self.inlines(cell);
                    }
                }
            }
        }
    }
}
