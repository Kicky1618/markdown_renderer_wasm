use crate::inline::parse_inlines;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Code(String),
    Math {
        source: String,
        display: bool,
    },
    Link {
        label: Vec<Inline>,
        destination: String,
    },
    SoftBreak,
    HardBreak,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Inline>),
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    CodeBlock {
        language: Option<String>,
        text: String,
        closed: bool,
    },
    BlockQuote(Vec<Inline>),
    UnorderedList(Vec<Vec<Inline>>),
    OrderedList {
        start: u32,
        items: Vec<Vec<Inline>>,
    },
    ThematicBreak,
    Table {
        headers: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// Delete every top-level block at and after this index.
    Truncate { from: u32 },
    /// Append one block to the document.
    Push(Block),
    /// Edit only the tail of an open code block's UTF-8 text.
    SpliceCode {
        block: u32,
        truncate_bytes: u32,
        append: String,
    },
    /// Change an open code block to closed.
    SealCode { block: u32 },
    /// Append plain UTF-8 text to the sole text inline of a live paragraph.
    AppendText { block: u32, append: String },
    /// Append plain UTF-8 text to the inline tail of a live paragraph.
    /// If the paragraph does not currently end in `Text`, a new text node is created.
    AppendInlineText { block: u32, append: String },
    /// Remove UTF-8 bytes from the final Text inline and append replacement nodes.
    /// This keeps delimiter completion local to a live paragraph tail.
    SpliceInlineTail {
        block: u32,
        remove_nodes: u32,
        truncate_bytes: u32,
        append: Vec<Inline>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Delta {
    pub ops: Vec<Op>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingKind {
    Paragraph,
    Quote,
    Unordered,
    Ordered,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpecialBracketKind {
    None,
    Reference,
    Citation,
}

#[derive(Debug)]
enum Mode {
    Normal,
    Fence {
        block: usize,
        marker: u8,
        marker_len: usize,
        line_start: usize,
    },
}

/// Stateful append-only parser. One instance corresponds to one document.
#[derive(Debug)]
pub struct Parser {
    blocks: Vec<Block>,
    mode: Mode,
    line: String,
    pending: String,
    pending_kind: Option<PendingKind>,
    committed: usize,
    has_live: bool,
    live_plain: bool,
    live_inline_appendable: bool,
    trailing_backslash_odd: bool,
    trailing_backtick_mod2: u8,
    trailing_backtick_fast: bool,
    trailing_dollar_mod4: u8,
    trailing_dollar_fast: bool,
    trailing_star_mod4: u8,
    trailing_star_fast: bool,
    trailing_underscore_mod4: u8,
    trailing_underscore_fast: bool,
    live_special_bracket: SpecialBracketKind,
    live_has_link_label_open: bool,
    live_link_label_just_closed: bool,
    live_has_link_destination_start: bool,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            mode: Mode::Normal,
            line: String::new(),
            pending: String::new(),
            pending_kind: None,
            committed: 0,
            has_live: false,
            live_plain: false,
            live_inline_appendable: false,
            trailing_backslash_odd: false,
            trailing_backtick_mod2: 0,
            trailing_backtick_fast: true,
            trailing_dollar_mod4: 0,
            trailing_dollar_fast: true,
            trailing_star_mod4: 0,
            trailing_star_fast: true,
            trailing_underscore_mod4: 0,
            trailing_underscore_fast: true,
            live_special_bracket: SpecialBracketKind::None,
            live_has_link_label_open: false,
            live_link_label_just_closed: false,
            live_has_link_destination_start: false,
        }
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Clear all parser state and return the mutation needed by a renderer.
    pub fn reset(&mut self) -> Delta {
        let had_blocks = !self.blocks.is_empty();
        *self = Self::new();
        Delta {
            ops: if had_blocks {
                vec![Op::Truncate { from: 0 }]
            } else {
                Vec::new()
            },
        }
    }

    /// Replace the current document and return one combined renderer delta.
    pub fn replace(&mut self, input: &str) -> Delta {
        let mut delta = self.reset();
        delta.ops.extend(self.append(input).ops);
        delta
    }

    /// Finalize an assistant response after its last chunk has arrived.
    ///
    /// This recognizes a closing code fence at EOF even when the provider did
    /// not include a trailing newline. A finalized normal or closed-fence tail
    /// will no longer be revised by a later `append` call.
    pub fn finish(&mut self) -> Delta {
        let mut delta = Delta::default();
        match self.mode {
            Mode::Normal => {
                self.committed = self.blocks.len();
                self.has_live = false;
                self.live_plain = false;
                self.live_inline_appendable = false;
                self.trailing_backslash_odd = false;
                self.reset_delimiter_runs();
                self.live_special_bracket = SpecialBracketKind::None;
                self.live_has_link_label_open = false;
                self.live_link_label_just_closed = false;
                self.live_has_link_destination_start = false;
                self.line.clear();
                self.pending.clear();
                self.pending_kind = None;
            }
            Mode::Fence {
                block,
                marker,
                marker_len,
                line_start,
            } => {
                let text = code_text_mut(&mut self.blocks[block]);
                let tail = &text[line_start..];
                if fence_close(tail, marker, marker_len) {
                    let truncate_bytes = tail.len() as u32;
                    text.truncate(line_start);
                    if truncate_bytes != 0 {
                        delta.ops.push(Op::SpliceCode {
                            block: block as u32,
                            truncate_bytes,
                            append: String::new(),
                        });
                    }
                    if let Block::CodeBlock { closed, .. } = &mut self.blocks[block] {
                        *closed = true;
                    }
                    delta.ops.push(Op::SealCode {
                        block: block as u32,
                    });
                    self.mode = Mode::Normal;
                    self.committed = self.blocks.len();
                    self.trailing_backslash_odd = false;
                    self.reset_delimiter_runs();
                    self.live_special_bracket = SpecialBracketKind::None;
                    self.live_has_link_label_open = false;
                    self.live_link_label_just_closed = false;
                    self.live_has_link_destination_start = false;
                }
            }
        }
        delta
    }

    /// Append a UTF-8 chunk and return only mutations caused by that chunk.
    pub fn append(&mut self, input: &str) -> Delta {
        let mut delta = Delta::default();
        self.append_into(input, &mut delta);
        delta
    }

    /// Append into a caller-owned delta buffer so high-frequency callers can
    /// reuse the `ops` allocation across token chunks. Existing operations are
    /// cleared before parsing.
    pub fn append_into(&mut self, input: &str, delta: &mut Delta) {
        delta.ops.clear();
        if self.try_append_plain(input, delta) {
            return;
        }
        if self.try_append_inline_text(input, delta) {
            return;
        }
        let mut rest = input;
        loop {
            match self.mode {
                Mode::Normal => {
                    if rest.is_empty() {
                        break;
                    }
                    self.begin_normal_update(delta);
                    rest = self.consume_normal(rest, delta);
                    if matches!(self.mode, Mode::Normal) {
                        self.publish_live(delta);
                        break;
                    }
                    if rest.is_empty() {
                        break;
                    }
                }
                Mode::Fence { .. } => {
                    rest = self.consume_fence(rest, delta);
                    if rest.is_empty() {
                        break;
                    }
                }
            }
        }
    }

    fn try_append_plain(&mut self, input: &str, delta: &mut Delta) -> bool {
        if input.is_empty()
            || !matches!(self.mode, Mode::Normal)
            || !self.has_live
            || !self.live_plain
            || !input.bytes().all(is_plain_stream_byte)
        {
            return false;
        }

        let Some(block_index) = self.blocks.len().checked_sub(1) else {
            return false;
        };
        let Some(Block::Paragraph(nodes)) = self.blocks.get_mut(block_index) else {
            return false;
        };
        let [Inline::Text(text)] = nodes.as_mut_slice() else {
            return false;
        };
        text.push_str(input);
        self.line.push_str(input);
        self.trailing_backslash_odd = false;
        self.reset_delimiter_runs();
        delta.ops.push(Op::AppendText {
            block: block_index as u32,
            append: input.to_owned(),
        });
        true
    }

    fn try_append_inline_text(&mut self, input: &str, delta: &mut Delta) -> bool {
        if input.is_empty()
            || !matches!(self.mode, Mode::Normal)
            || !self.has_live
            || !self.live_inline_appendable
        {
            return false;
        }

        let Some(block_index) = self.blocks.len().checked_sub(1) else {
            return false;
        };
        let Some(Block::Paragraph(nodes)) = self.blocks.get_mut(block_index) else {
            return false;
        };

        if input.bytes().all(|byte| byte == b'`')
            && self.trailing_backtick_fast
            && !self.trailing_backslash_odd
        {
            let old = self.trailing_backtick_mod2 as usize;
            let total = old + input.len();
            let pairs = total / 2;
            let remainder = total % 2;
            if pairs == 0 {
                append_inline_text(nodes, input);
                self.line.push_str(input);
                self.trailing_backtick_mod2 = remainder as u8;
                self.break_dollar_run();
                self.break_star_run();
                self.break_underscore_run();
                self.trailing_backslash_odd = false;
                delta.ops.push(Op::AppendInlineText {
                    block: block_index as u32,
                    append: input.to_owned(),
                });
            } else {
                let mut replacement = Vec::with_capacity(pairs + remainder);
                replacement.extend((0..pairs).map(|_| Inline::Code(String::new())));
                if remainder != 0 {
                    replacement.push(Inline::Text("`".to_owned()));
                }
                splice_inline_tail(nodes, old, 0, &replacement);
                self.line.push_str(input);
                self.trailing_backtick_mod2 = remainder as u8;
                self.break_dollar_run();
                self.break_star_run();
                self.break_underscore_run();
                self.trailing_backslash_odd = false;
                delta.ops.push(Op::SpliceInlineTail {
                    block: block_index as u32,
                    remove_nodes: 0,
                    truncate_bytes: old as u32,
                    append: replacement,
                });
            }
            return true;
        }

        if input.bytes().all(|byte| byte == b'$')
            && self.trailing_dollar_fast
            && !self.trailing_backslash_odd
        {
            let old = self.trailing_dollar_mod4 as usize;
            let total = old + input.len();
            let groups = total / 4;
            let remainder = total % 4;
            if groups == 0 {
                append_inline_text(nodes, input);
                self.line.push_str(input);
                self.trailing_dollar_mod4 = remainder as u8;
                self.break_backtick_run();
                self.break_star_run();
                self.break_underscore_run();
                self.trailing_backslash_odd = false;
                delta.ops.push(Op::AppendInlineText {
                    block: block_index as u32,
                    append: input.to_owned(),
                });
            } else {
                let mut replacement = Vec::with_capacity(groups + usize::from(remainder != 0));
                replacement.extend((0..groups).map(|_| Inline::Math {
                    source: String::new(),
                    display: true,
                }));
                if remainder != 0 {
                    replacement.push(Inline::Text("$".repeat(remainder)));
                }
                splice_inline_tail(nodes, old, 0, &replacement);
                self.line.push_str(input);
                self.trailing_dollar_mod4 = remainder as u8;
                self.break_backtick_run();
                self.break_star_run();
                self.break_underscore_run();
                self.trailing_backslash_odd = false;
                delta.ops.push(Op::SpliceInlineTail {
                    block: block_index as u32,
                    remove_nodes: 0,
                    truncate_bytes: old as u32,
                    append: replacement,
                });
            }
            return true;
        }

        if input.bytes().all(|byte| byte == b'*')
            && self.trailing_star_fast
            && !self.trailing_backslash_odd
        {
            let old = self.trailing_star_mod4 as usize;
            let (remove_nodes, truncate_bytes, replacement, remainder) =
                emphasis_run_splice(old, input.len(), '*');
            splice_inline_tail(nodes, truncate_bytes, remove_nodes, &replacement);
            self.line.push_str(input);
            self.trailing_star_mod4 = remainder as u8;
            self.break_backtick_run();
            self.break_dollar_run();
            self.break_underscore_run();
            self.trailing_backslash_odd = false;
            delta.ops.push(Op::SpliceInlineTail {
                block: block_index as u32,
                remove_nodes: remove_nodes as u32,
                truncate_bytes: truncate_bytes as u32,
                append: replacement,
            });
            return true;
        }

        if input.bytes().all(|byte| byte == b'_')
            && self.trailing_underscore_fast
            && !self.trailing_backslash_odd
        {
            let old = self.trailing_underscore_mod4 as usize;
            let (remove_nodes, truncate_bytes, replacement, remainder) =
                emphasis_run_splice(old, input.len(), '_');
            splice_inline_tail(nodes, truncate_bytes, remove_nodes, &replacement);
            self.line.push_str(input);
            self.trailing_underscore_mod4 = remainder as u8;
            self.break_backtick_run();
            self.break_dollar_run();
            self.break_star_run();
            self.trailing_backslash_odd = false;
            delta.ops.push(Op::SpliceInlineTail {
                block: block_index as u32,
                remove_nodes: remove_nodes as u32,
                truncate_bytes: truncate_bytes as u32,
                append: replacement,
            });
            return true;
        }

        // A closing bracket/paren is inert when the live paragraph has no raw
        // opener that it could possibly complete. Keep these common malformed
        // streaming runs on the append-only path without weakening correctness
        // for real references/links.
        let inert_closer = if input.bytes().all(|byte| byte == b']') {
            match self.live_special_bracket {
                SpecialBracketKind::None => true,
                SpecialBracketKind::Reference => !llm_reference_suffix_can_close(&self.line),
                SpecialBracketKind::Citation => {
                    !llm_citation_suffix_can_close(&self.line, input.len())
                }
            }
        } else if input.bytes().all(|byte| byte == b')') {
            !self.live_has_link_destination_start
        } else {
            false
        };
        if inert_closer {
            if let Some(Inline::Text(text)) = nodes.last_mut() {
                text.push_str(input);
            } else {
                nodes.push(Inline::Text(input.to_owned()));
            }
            self.line.push_str(input);
            if input.as_bytes().first() == Some(&b']') {
                let closed_label = self.live_has_link_label_open && input.len() == 1;
                self.live_has_link_label_open = false;
                self.live_link_label_just_closed = closed_label;
                self.live_special_bracket = match self.live_special_bracket {
                    SpecialBracketKind::Reference => SpecialBracketKind::None,
                    SpecialBracketKind::Citation
                        if citation_suffix_waits_for_second_close(&self.line) =>
                    {
                        SpecialBracketKind::Citation
                    }
                    _ => SpecialBracketKind::None,
                };
            }
            self.trailing_backslash_odd = false;
            self.break_delimiter_runs();
            delta.ops.push(Op::AppendInlineText {
                block: block_index as u32,
                append: input.to_owned(),
            });
            return true;
        }

        // A run of backslashes is special: Markdown collapses each escaped pair
        // into one literal backslash. Track the raw trailing-run parity so a
        // token-at-a-time LLM stream does not reparse the whole paragraph on
        // every second byte. Any later escapable punctuation still takes the
        // conservative full-reparse path below.
        if input.bytes().all(|byte| byte == b'\\') {
            let append_len = if self.trailing_backslash_odd {
                input.len() / 2
            } else {
                input.len().div_ceil(2)
            };
            if append_len != 0 {
                let append = "\\".repeat(append_len);
                if let Some(Inline::Text(text)) = nodes.last_mut() {
                    text.push_str(&append);
                } else {
                    nodes.push(Inline::Text(append.clone()));
                }
                delta.ops.push(Op::AppendInlineText {
                    block: block_index as u32,
                    append,
                });
            }
            self.line.push_str(input);
            if input.len() % 2 != 0 {
                self.trailing_backslash_odd = !self.trailing_backslash_odd;
            }
            self.break_delimiter_runs();
            return true;
        }

        if !inline_tail_append_is_safe(&self.line, input) {
            return false;
        }
        let cross_link_destination = self.live_link_label_just_closed && input.starts_with('(');
        if let Some(kind) = special_bracket_opener_kind(&self.line, input) {
            self.live_special_bracket = kind;
        }
        self.live_has_link_label_open |= input.as_bytes().contains(&b'[');
        self.live_link_label_just_closed = false;
        self.live_has_link_destination_start |= cross_link_destination;
        if let Some(Inline::Text(text)) = nodes.last_mut() {
            text.push_str(input);
        } else {
            nodes.push(Inline::Text(input.to_owned()));
        }
        self.line.push_str(input);
        self.trailing_backslash_odd =
            trailing_backslash_odd_after(self.trailing_backslash_odd, input);
        self.break_delimiter_runs();
        delta.ops.push(Op::AppendInlineText {
            block: block_index as u32,
            append: input.to_owned(),
        });
        true
    }

    fn reset_delimiter_runs(&mut self) {
        self.trailing_backtick_mod2 = 0;
        self.trailing_backtick_fast = true;
        self.trailing_dollar_mod4 = 0;
        self.trailing_dollar_fast = true;
        self.trailing_star_mod4 = 0;
        self.trailing_star_fast = true;
        self.trailing_underscore_mod4 = 0;
        self.trailing_underscore_fast = true;
    }

    fn break_backtick_run(&mut self) {
        if self.trailing_backtick_mod2 != 0 {
            self.trailing_backtick_fast = false;
        }
        self.trailing_backtick_mod2 = 0;
    }

    fn break_dollar_run(&mut self) {
        if self.trailing_dollar_mod4 != 0 {
            self.trailing_dollar_fast = false;
        }
        self.trailing_dollar_mod4 = 0;
    }

    fn break_star_run(&mut self) {
        if self.trailing_star_mod4 != 0 {
            self.trailing_star_fast = false;
        }
        self.trailing_star_mod4 = 0;
    }

    fn break_underscore_run(&mut self) {
        if self.trailing_underscore_mod4 != 0 {
            self.trailing_underscore_fast = false;
        }
        self.trailing_underscore_mod4 = 0;
    }

    fn break_delimiter_runs(&mut self) {
        self.break_backtick_run();
        self.break_dollar_run();
        self.break_star_run();
        self.break_underscore_run();
    }

    fn begin_normal_update(&mut self, delta: &mut Delta) {
        if self.has_live {
            self.blocks.truncate(self.committed);
            delta.ops.push(Op::Truncate {
                from: self.committed as u32,
            });
            self.has_live = false;
            self.live_plain = false;
            self.live_inline_appendable = false;
            self.trailing_backslash_odd = false;
            self.reset_delimiter_runs();
            self.live_special_bracket = SpecialBracketKind::None;
            self.live_has_link_label_open = false;
            self.live_link_label_just_closed = false;
            self.live_has_link_destination_start = false;
        }
    }

    fn consume_normal<'a>(&mut self, input: &'a str, delta: &mut Delta) -> &'a str {
        let bytes = input.as_bytes();
        let mut offset = 0;
        while let Some(rel) = bytes[offset..].iter().position(|&b| b == b'\n') {
            let end = offset + rel;
            self.line.push_str(&input[offset..end]);
            if self.line.ends_with('\r') {
                self.line.pop();
            }
            let line = std::mem::take(&mut self.line);
            offset = end + 1;
            if let Some((marker_len, language)) = llm_fence_open(&line) {
                self.finish_pending(delta);
                let index = self.blocks.len();
                self.push(
                    Block::CodeBlock {
                        language: Some(language),
                        text: String::new(),
                        closed: false,
                    },
                    delta,
                );
                self.committed = self.blocks.len();
                self.mode = Mode::Fence {
                    block: index,
                    marker: b':',
                    marker_len,
                    line_start: 0,
                };
                return &input[offset..];
            }
            if let Some((marker, marker_len, language)) = fence_open(&line) {
                self.finish_pending(delta);
                let index = self.blocks.len();
                self.push(
                    Block::CodeBlock {
                        language,
                        text: String::new(),
                        closed: false,
                    },
                    delta,
                );
                self.committed = self.blocks.len();
                self.mode = Mode::Fence {
                    block: index,
                    marker,
                    marker_len,
                    line_start: 0,
                };
                return &input[offset..];
            }
            self.accept_line(&line, delta);
        }
        self.line.push_str(&input[offset..]);
        ""
    }

    fn accept_line(&mut self, line: &str, delta: &mut Delta) {
        if line.trim().is_empty() {
            self.finish_pending(delta);
            return;
        }
        if is_thematic(line) {
            self.finish_pending(delta);
            self.push(Block::ThematicBreak, delta);
            self.committed = self.blocks.len();
            return;
        }
        if heading(line).is_some() {
            self.finish_pending(delta);
            self.push(block_from_complete(line, PendingKind::Paragraph), delta);
            self.committed = self.blocks.len();
            return;
        }
        let kind = classify(line);
        if self.pending_kind.is_some_and(|old| old != kind) {
            self.finish_pending(delta);
        }
        self.pending_kind = Some(kind);
        self.pending.push_str(line);
        self.pending.push('\n');
    }

    fn finish_pending(&mut self, delta: &mut Delta) {
        if let Some(kind) = self.pending_kind.take() {
            let source = self.pending.trim_end_matches('\n');
            if !source.is_empty() {
                self.push(block_from_complete(source, kind), delta);
            }
            self.pending.clear();
            self.committed = self.blocks.len();
        }
    }

    fn publish_live(&mut self, delta: &mut Delta) {
        let mut source = self.pending.clone();
        source.push_str(&self.line);
        if source.is_empty() {
            self.trailing_backslash_odd = false;
            self.reset_delimiter_runs();
            self.live_special_bracket = SpecialBracketKind::None;
            self.live_has_link_label_open = false;
            self.live_link_label_just_closed = false;
            self.live_has_link_destination_start = false;
            return;
        }
        if self.pending_kind.is_none() && is_thematic(&self.line) {
            self.live_plain = false;
            self.trailing_backslash_odd = false;
            self.reset_delimiter_runs();
            self.live_special_bracket = SpecialBracketKind::None;
            self.live_has_link_label_open = false;
            self.live_link_label_just_closed = false;
            self.live_has_link_destination_start = false;
            self.live_inline_appendable = false;
            self.push(Block::ThematicBreak, delta);
            self.has_live = true;
            return;
        }
        let kind = self.pending_kind.unwrap_or_else(|| classify(&self.line));
        let block = block_from_complete(source.trim_end_matches('\n'), kind);
        let is_paragraph = matches!(block, Block::Paragraph(_));
        let stable_single_line_paragraph = is_paragraph
            && self.pending_kind.is_none()
            && first_line_paragraph_is_stable(&self.line);
        let stable_multiline_paragraph = is_paragraph
            && self.pending_kind == Some(PendingKind::Paragraph)
            && !self.line.is_empty()
            && multiline_paragraph_tail_is_stable(&self.pending, &self.line);
        self.live_plain = stable_single_line_paragraph && source.bytes().all(is_plain_stream_byte);
        self.live_inline_appendable =
            (stable_single_line_paragraph && !self.live_plain) || stable_multiline_paragraph;
        self.trailing_backslash_odd = self
            .line
            .as_bytes()
            .iter()
            .rev()
            .take_while(|&&byte| byte == b'\\')
            .count()
            % 2
            != 0;
        let (backtick_mod, backtick_fast) = delimiter_run_state(&block, &source, b'`', 2);
        self.trailing_backtick_mod2 = backtick_mod;
        self.trailing_backtick_fast = backtick_fast;
        let (dollar_mod, dollar_fast) = delimiter_run_state(&block, &source, b'$', 4);
        self.trailing_dollar_mod4 = dollar_mod;
        self.trailing_dollar_fast = dollar_fast;
        let (star_mod, star_fast) = emphasis_run_state(&block, &source, b'*');
        self.trailing_star_mod4 = star_mod;
        self.trailing_star_fast = star_fast;
        let (underscore_mod, underscore_fast) = emphasis_run_state(&block, &source, b'_');
        self.trailing_underscore_mod4 = underscore_mod;
        self.trailing_underscore_fast = underscore_fast;
        self.live_special_bracket = special_bracket_kind_from_source(&source);
        let (link_label_open, link_label_just_closed) = link_label_state_from_source(&source);
        self.live_has_link_label_open = link_label_open;
        self.live_link_label_just_closed = link_label_just_closed;
        self.live_has_link_destination_start = closing_paren_sensitive(&source);
        self.push(block, delta);
        self.has_live = true;
    }

    fn consume_fence<'a>(&mut self, input: &'a str, delta: &mut Delta) -> &'a str {
        let Mode::Fence {
            block,
            marker,
            marker_len,
            line_start,
        } = self.mode
        else {
            unreachable!()
        };
        let old_len = code_text(&self.blocks[block]).len();
        let mut cursor = 0;
        let mut current_start = line_start;
        let mut closed_at = None;
        {
            let text = code_text_mut(&mut self.blocks[block]);
            text.push_str(input);
            while let Some(rel) = text.as_bytes()[current_start..]
                .iter()
                .position(|&b| b == b'\n')
            {
                let end = current_start + rel;
                let candidate = text[current_start..end].trim_end_matches('\r');
                if fence_close(candidate, marker, marker_len) {
                    // Number of input bytes through the closing line.
                    cursor = end + 1 - old_len;
                    text.truncate(current_start);
                    closed_at = Some(current_start);
                    break;
                }
                current_start = end + 1;
            }
        }

        // The external mirror already owns every byte before this append. Keep
        // ordinary fence streaming strictly append-only; only retract bytes if
        // a closing fence began inside the previously published partial line.
        let (truncate_bytes, append) = if let Some(close_start) = closed_at {
            if close_start < old_len {
                ((old_len - close_start) as u32, String::new())
            } else {
                (0, input[..close_start - old_len].to_owned())
            }
        } else {
            (0, input.to_owned())
        };
        if truncate_bytes != 0 || !append.is_empty() {
            delta.ops.push(Op::SpliceCode {
                block: block as u32,
                truncate_bytes,
                append,
            });
        }
        if closed_at.is_some() {
            if let Block::CodeBlock { closed, .. } = &mut self.blocks[block] {
                *closed = true;
            }
            delta.ops.push(Op::SealCode {
                block: block as u32,
            });
            self.mode = Mode::Normal;
            self.committed = self.blocks.len();
            &input[cursor..]
        } else {
            self.mode = Mode::Fence {
                block,
                marker,
                marker_len,
                line_start: current_start,
            };
            ""
        }
    }

    fn push(&mut self, block: Block, delta: &mut Delta) {
        self.blocks.push(block.clone());
        delta.ops.push(Op::Push(block));
    }
}

fn code_text(block: &Block) -> &str {
    match block {
        Block::CodeBlock { text, .. } => text,
        _ => unreachable!(),
    }
}
fn code_text_mut(block: &mut Block) -> &mut String {
    match block {
        Block::CodeBlock { text, .. } => text,
        _ => unreachable!(),
    }
}

fn is_plain_stream_byte(b: u8) -> bool {
    !matches!(
        b,
        b'\n' | b'\r' | b'\\' | b'$' | b'`' | b'*' | b'_' | b'[' | b']' | b'(' | b')' | b'@'
    )
}

fn append_inline_text(nodes: &mut Vec<Inline>, append: &str) {
    if let Some(Inline::Text(text)) = nodes.last_mut() {
        text.push_str(append);
    } else {
        nodes.push(Inline::Text(append.to_owned()));
    }
}

fn splice_inline_tail(
    nodes: &mut Vec<Inline>,
    truncate_bytes: usize,
    remove_nodes: usize,
    append: &[Inline],
) {
    if truncate_bytes != 0 {
        let Some(Inline::Text(text)) = nodes.last_mut() else {
            unreachable!("inline tail splice requires trailing text")
        };
        let new_len = text
            .len()
            .checked_sub(truncate_bytes)
            .expect("inline tail splice exceeds trailing text");
        assert!(text.is_char_boundary(new_len));
        text.truncate(new_len);
        if text.is_empty() {
            nodes.pop();
        }
    }
    if remove_nodes != 0 {
        let new_len = nodes
            .len()
            .checked_sub(remove_nodes)
            .expect("inline tail splice removes too many nodes");
        nodes.truncate(new_len);
    }
    for node in append {
        if let Inline::Text(value) = node
            && let Some(Inline::Text(text)) = nodes.last_mut()
        {
            text.push_str(value);
        } else {
            nodes.push(node.clone());
        }
    }
}

fn emphasis_run_splice(
    old: usize,
    added: usize,
    delimiter: char,
) -> (usize, usize, Vec<Inline>, usize) {
    let (remove_nodes, truncate_bytes) = match old {
        0 => (0, 0),
        1 => (0, 1),
        2 => (1, 0),
        3 => (1, 1),
        _ => unreachable!(),
    };
    let total = old + added;
    let groups = total / 4;
    let remainder = total % 4;
    let mut replacement = Vec::with_capacity(groups + 2);
    replacement.extend((0..groups).map(|_| Inline::Strong(Vec::new())));
    match remainder {
        0 => {}
        1 => replacement.push(Inline::Text(delimiter.to_string())),
        2 => replacement.push(Inline::Emphasis(Vec::new())),
        3 => {
            replacement.push(Inline::Emphasis(Vec::new()));
            replacement.push(Inline::Text(delimiter.to_string()));
        }
        _ => unreachable!(),
    }
    (remove_nodes, truncate_bytes, replacement, remainder)
}

fn emphasis_run_state(block: &Block, source: &str, delimiter: u8) -> (u8, bool) {
    let Block::Paragraph(nodes) = block else {
        return (0, true);
    };
    let last = nodes.last();
    let trailing_text = match last {
        Some(Inline::Text(text)) => text
            .as_bytes()
            .iter()
            .rev()
            .take_while(|&&byte| byte == delimiter)
            .count(),
        _ => 0,
    };
    let mut remainder = if trailing_text == 1 {
        let previous_empty_emphasis = nodes.len() >= 2
            && matches!(&nodes[nodes.len() - 2], Inline::Emphasis(children) if children.is_empty());
        if previous_empty_emphasis { 3 } else { 1 }
    } else if trailing_text == 0
        && matches!(last, Some(Inline::Emphasis(children)) if children.is_empty())
    {
        2
    } else {
        0
    };

    let mut fast = trailing_text <= 1;
    for (index, node) in nodes.iter().enumerate() {
        let Inline::Text(text) = node else { continue };
        let allowed = if index + 1 == nodes.len() && matches!(remainder, 1 | 3) {
            1
        } else {
            0
        };
        let prefix_len = text.len().saturating_sub(allowed);
        if text.as_bytes()[..prefix_len].contains(&delimiter) {
            fast = false;
        }
    }

    if remainder != 0 {
        let raw_trailing = source
            .as_bytes()
            .iter()
            .rev()
            .take_while(|&&byte| byte == delimiter)
            .count();
        if raw_trailing % 4 != remainder as usize {
            fast = false;
            remainder = 0;
        } else {
            let start = source.len() - raw_trailing;
            let escaped = source.as_bytes()[..start]
                .iter()
                .rev()
                .take_while(|&&byte| byte == b'\\')
                .count()
                % 2
                != 0;
            fast &= !escaped;
        }
    }
    (remainder, fast)
}

fn delimiter_run_state(block: &Block, source: &str, delimiter: u8, modulus: usize) -> (u8, bool) {
    let Block::Paragraph(nodes) = block else {
        return (0, true);
    };

    let mut trailing = 0usize;
    let mut fast = true;
    for (index, node) in nodes.iter().enumerate() {
        let Inline::Text(text) = node else {
            continue;
        };
        let is_last = index + 1 == nodes.len();
        let allowed_tail = if is_last {
            text.as_bytes()
                .iter()
                .rev()
                .take_while(|&&byte| byte == delimiter)
                .count()
        } else {
            0
        };
        let prefix_len = text.len() - allowed_tail;
        if text.as_bytes()[..prefix_len].contains(&delimiter) {
            fast = false;
        }
        if is_last {
            trailing = allowed_tail;
        }
    }

    if trailing != 0 {
        let raw_trailing = source
            .as_bytes()
            .iter()
            .rev()
            .take_while(|&&byte| byte == delimiter)
            .count();
        if raw_trailing < trailing {
            fast = false;
        } else {
            let start = source.len() - raw_trailing;
            let escaped = source.as_bytes()[..start]
                .iter()
                .rev()
                .take_while(|&&byte| byte == b'\\')
                .count()
                % 2
                != 0;
            fast &= !escaped;
        }
    }

    ((trailing % modulus) as u8, fast)
}

fn crossing_marker_start(line: &[u8], input: &[u8], marker: &[u8]) -> Option<isize> {
    let internal = input
        .windows(marker.len())
        .rposition(|window| window == marker)
        .map(|index| index as isize);
    let crossing = (1..marker.len())
        .filter(|&split| line.ends_with(&marker[..split]) && input.starts_with(&marker[split..]))
        .map(|split| -(split as isize))
        .max();
    internal.into_iter().chain(crossing).max()
}

fn special_bracket_opener_kind(line: &str, input: &str) -> Option<SpecialBracketKind> {
    let line = line.as_bytes();
    let input = input.as_bytes();
    let reference = crossing_marker_start(line, input, b"@[")
        .map(|position| (position, SpecialBracketKind::Reference));
    let citation = crossing_marker_start(line, input, b"[[cite:")
        .map(|position| (position, SpecialBracketKind::Citation));
    reference
        .into_iter()
        .chain(citation)
        .max_by_key(|(position, _)| *position)
        .map(|(_, kind)| kind)
}

fn link_label_state_from_source(source: &str) -> (bool, bool) {
    if source.ends_with(']') {
        let prefix = &source[..source.len() - 1];
        let open = prefix.rfind('[');
        let prior_close = prefix.rfind(']');
        return (
            false,
            open.is_some_and(|open| prior_close.is_none_or(|close| open > close)),
        );
    }
    let open = source.rfind('[');
    let close = source.rfind(']');
    (
        open.is_some_and(|open| close.is_none_or(|close| open > close)),
        false,
    )
}

fn valid_reference_atom_streaming(value: &str) -> bool {
    !value.is_empty()
        && !value.bytes().any(|byte| {
            byte.is_ascii_control()
                || byte.is_ascii_whitespace()
                || matches!(byte, b'[' | b']' | b'|')
        })
}

fn llm_reference_suffix_can_close(source: &str) -> bool {
    let Some(open) = source.rfind("@[") else {
        return false;
    };
    let tail = &source[open + 2..];
    if tail.contains(']') {
        return false;
    }
    let bytes = tail.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'-')) {
        i += 1;
    }
    if i == 0 || bytes.get(i) != Some(&b':') {
        return false;
    }
    valid_reference_atom_streaming(&tail[i + 1..])
}

fn citation_source_is_valid(source: &str) -> bool {
    let source = source.trim();
    valid_reference_atom_streaming(source)
        && !source
            .bytes()
            .any(|byte| byte == b'[' || byte == b']' || byte.is_ascii_control())
}

fn llm_citation_suffix_can_close(source: &str, close_count: usize) -> bool {
    let Some(open) = source.rfind("[[cite:") else {
        return false;
    };
    let tail = &source[open + "[[cite:".len()..];
    if tail.contains("]]") {
        return false;
    }

    if let Some(pipe) = tail.find('|') {
        if !citation_source_is_valid(&tail[..pipe]) {
            return false;
        }
        let label = &tail[pipe + 1..];
        label.ends_with(']') || close_count >= 2
    } else {
        let pending_close = usize::from(tail.ends_with(']'));
        let source_tail = if pending_close != 0 {
            &tail[..tail.len() - 1]
        } else {
            tail
        };
        citation_source_is_valid(source_tail) && pending_close + close_count >= 2
    }
}

fn citation_suffix_waits_for_second_close(source: &str) -> bool {
    let Some(open) = source.rfind("[[cite:") else {
        return false;
    };
    let tail = &source[open + "[[cite:".len()..];
    if tail.contains("]]") || !tail.ends_with(']') {
        return false;
    }
    if let Some(pipe) = tail.find('|') {
        citation_source_is_valid(&tail[..pipe])
    } else {
        citation_source_is_valid(&tail[..tail.len() - 1])
    }
}

fn special_bracket_kind_from_source(source: &str) -> SpecialBracketKind {
    let last_close = source.rfind(']');
    let reference = source
        .rfind("@[")
        .filter(|&open| last_close.is_none_or(|close| open > close))
        .map(|open| (open, SpecialBracketKind::Reference));
    let citation = source.rfind("[[cite:").and_then(|open| {
        (!source[open + "[[cite:".len()..].contains("]]"))
            .then_some((open, SpecialBracketKind::Citation))
    });
    reference
        .into_iter()
        .chain(citation)
        .max_by_key(|(position, _)| *position)
        .map_or(SpecialBracketKind::None, |(_, kind)| kind)
}

fn closing_paren_sensitive(source: &str) -> bool {
    let Some(open) = source.rfind("](") else {
        return false;
    };
    !source[open + 2..].contains(')')
}

fn inline_tail_append_is_safe(line: &str, input: &str) -> bool {
    let bytes = input.as_bytes();
    if line.ends_with('\\')
        && bytes
            .first()
            .is_some_and(|&byte| is_escapable_punctuation(byte))
    {
        return false;
    }

    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            // These bytes can complete syntax that began anywhere in the live
            // paragraph, so they must force a full reparse.
            b'\n' | b'\r' | b'$' | b'`' | b'*' | b'_' | b']' | b')' => return false,
            // A backslash only changes the current AST when it escapes the next
            // punctuation byte. A trailing/unpaired backslash is still literal.
            b'\\'
                if bytes
                    .get(i + 1)
                    .is_some_and(|&byte| is_escapable_punctuation(byte)) =>
            {
                return false;
            }
            _ => {}
        }
        i += 1;
    }
    true
}

fn trailing_backslash_odd_after(old_odd: bool, input: &str) -> bool {
    let trailing = input
        .as_bytes()
        .iter()
        .rev()
        .take_while(|&&byte| byte == b'\\')
        .count();
    if trailing == 0 {
        false
    } else if trailing == input.len() {
        old_odd ^ (trailing % 2 != 0)
    } else {
        trailing % 2 != 0
    }
}

fn is_escapable_punctuation(byte: u8) -> bool {
    matches!(byte, b'!'..=b'/' | b':'..=b'@' | b'['..=b'`' | b'{'..=b'~')
}

fn multiline_paragraph_tail_is_stable(pending: &str, line: &str) -> bool {
    // A pipe table can only appear when the first two lines form the header and
    // separator. Once two complete paragraph lines are pending, or the first
    // line has no pipe, appending to a later/current line cannot turn the block
    // into a table.
    let complete_lines = pending.bytes().filter(|&byte| byte == b'\n').count();
    if complete_lines >= 2 {
        return true;
    }
    if complete_lines == 0 {
        return false;
    }
    let first_line = pending.trim_end_matches('\n');
    if !first_line.contains('|') {
        return true;
    }

    // While editing the potential separator line, stay on the conservative
    // reparse path until a character makes table-separator syntax impossible.
    line.bytes()
        .any(|byte| !matches!(byte, b' ' | b'\t' | b'|' | b':' | b'-'))
}

fn first_line_paragraph_is_stable(line: &str) -> bool {
    let text = line.trim_start();
    if text.is_empty() {
        return false;
    }

    let hashes = text
        .as_bytes()
        .iter()
        .take_while(|&&byte| byte == b'#')
        .count();
    if hashes == text.len() && hashes <= 6 {
        return false;
    }

    if matches!(text, "-" | "*" | "+") || (text.len() < 3 && text.bytes().all(|byte| byte == b'-'))
    {
        return false;
    }

    let digits = text
        .as_bytes()
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits == text.len() && digits <= 9 {
        return false;
    }
    if digits != 0 && digits <= 9 && digits + 1 == text.len() && text.as_bytes()[digits] == b'.' {
        return false;
    }

    true
}

fn classify(line: &str) -> PendingKind {
    let t = line.trim_start();
    if t.starts_with("> ") || t == ">" {
        PendingKind::Quote
    } else if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        PendingKind::Unordered
    } else if ordered_item(t).is_some() {
        PendingKind::Ordered
    } else {
        PendingKind::Paragraph
    }
}

fn block_from_complete(source: &str, kind: PendingKind) -> Block {
    if let Some((level, text)) = heading(source) {
        return Block::Heading {
            level,
            content: parse_inlines(text),
        };
    }
    if kind == PendingKind::Paragraph
        && let Some(table) = table_from_complete(source)
    {
        return table;
    }
    match kind {
        PendingKind::Paragraph => Block::Paragraph(parse_multiline(source)),
        PendingKind::Quote => {
            let text = source
                .lines()
                .map(|l| l.trim_start().strip_prefix('>').unwrap_or(l).trim_start())
                .collect::<Vec<_>>()
                .join("\n");
            Block::BlockQuote(parse_multiline(&text))
        }
        PendingKind::Unordered => Block::UnorderedList(
            source
                .lines()
                .map(|l| {
                    let t = l.trim_start();
                    parse_inlines(t.get(2..).unwrap_or(""))
                })
                .collect(),
        ),
        PendingKind::Ordered => {
            let start = ordered_item(source.lines().next().unwrap_or("")).map_or(1, |x| x.0);
            let items = source
                .lines()
                .map(|l| {
                    let t = l.trim_start();
                    let body = ordered_item(t).map_or(t, |x| x.1);
                    parse_inlines(body)
                })
                .collect();
            Block::OrderedList { start, items }
        }
    }
}

fn table_from_complete(source: &str) -> Option<Block> {
    let mut lines = source.lines();
    let headers = split_table_row(lines.next()?)?;
    let separator = split_table_row(lines.next()?)?;
    if headers.is_empty()
        || separator.is_empty()
        || separator.len() != headers.len()
        || !separator.iter().all(|cell| is_table_separator(cell))
    {
        return None;
    }
    let rows = lines
        .filter_map(split_table_row)
        .filter(|row| !row.is_empty())
        .collect::<Vec<_>>();
    Some(Block::Table {
        headers: headers
            .into_iter()
            .map(|cell| parse_inlines(&cell))
            .collect(),
        rows: rows
            .into_iter()
            .map(|row| row.into_iter().map(|cell| parse_inlines(&cell)).collect())
            .collect(),
    })
}

fn split_table_row(line: &str) -> Option<Vec<String>> {
    if !line.contains('|') {
        return None;
    }
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    Some(line.split('|').map(|cell| cell.trim().to_owned()).collect())
}

fn is_table_separator(cell: &str) -> bool {
    let cell = cell.trim();
    let cell = cell.strip_prefix(':').unwrap_or(cell);
    let cell = cell.strip_suffix(':').unwrap_or(cell);
    cell.len() >= 3 && cell.chars().all(|c| c == '-')
}

fn parse_multiline(source: &str) -> Vec<Inline> {
    let trimmed = source.trim();
    if let Some(math) = trimmed
        .strip_prefix("[\n")
        .and_then(|body| body.strip_suffix("\n]"))
        .or_else(|| {
            trimmed
                .strip_prefix("\\[\n")
                .and_then(|body| body.strip_suffix("\n\\]"))
        })
    {
        return vec![Inline::Math {
            source: math.to_owned(),
            display: true,
        }];
    }
    parse_inlines(source)
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let t = line.trim_start();
    let count = t.as_bytes().iter().take_while(|&&b| b == b'#').count();
    if !(1..=6).contains(&count) || t.as_bytes().get(count) != Some(&b' ') {
        return None;
    }
    Some((count as u8, t[count + 1..].trim_end_matches('#').trim_end()))
}

fn llm_fence_open(line: &str) -> Option<(usize, String)> {
    let t = line.trim_start();
    let marker_len = t.as_bytes().iter().take_while(|&&b| b == b':').count();
    if marker_len < 3 {
        return None;
    }
    let rest = t.get(marker_len..)?.strip_prefix("llm")?;
    if rest
        .as_bytes()
        .first()
        .is_some_and(|b| !b.is_ascii_whitespace())
    {
        return None;
    }
    let info = rest.trim();
    let language = if info.is_empty() {
        "llm".to_owned()
    } else {
        let mut language = String::with_capacity(4 + info.len());
        language.push_str("llm:");
        language.push_str(info);
        language
    };
    Some((marker_len, language))
}

fn fence_open(line: &str) -> Option<(u8, usize, Option<String>)> {
    let t = line.trim_start();
    let marker = *t.as_bytes().first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let len = t.as_bytes().iter().take_while(|&&b| b == marker).count();
    if len < 3 {
        return None;
    }
    let info = t[len..].trim();
    Some((
        marker,
        len,
        (!info.is_empty()).then(|| info.split_whitespace().next().unwrap().to_owned()),
    ))
}

fn fence_close(line: &str, marker: u8, min_len: usize) -> bool {
    let t = line.trim();
    let count = t.as_bytes().iter().take_while(|&&b| b == marker).count();
    count >= min_len && count == t.len()
}

fn ordered_item(line: &str) -> Option<(u32, &str)> {
    let digits = line
        .as_bytes()
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if digits == 0
        || digits > 9
        || line.as_bytes().get(digits) != Some(&b'.')
        || line.as_bytes().get(digits + 1) != Some(&b' ')
    {
        return None;
    }
    Some((line[..digits].parse().ok()?, &line[digits + 2..]))
}

fn is_thematic(line: &str) -> bool {
    let mut marker = 0;
    let mut count = 0;
    for byte in line.bytes() {
        if byte.is_ascii_whitespace() {
            continue;
        }
        if !matches!(byte, b'-' | b'*' | b'_') {
            return false;
        }
        if marker == 0 {
            marker = byte;
        } else if byte != marker {
            return false;
        }
        count += 1;
    }
    count >= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_equals_single_append() {
        let markdown = "# Hello\n\nA **fast** [parser](https://example.com).\n\n- one\n- two\n";
        let mut whole = Parser::new();
        whole.append(markdown);
        let mut streamed = Parser::new();
        for ch in markdown.chars() {
            streamed.append(ch.encode_utf8(&mut [0; 4]));
        }
        assert_eq!(whole.blocks(), streamed.blocks());
    }

    #[test]
    fn code_delta_is_tail_splice() {
        let mut p = Parser::new();
        p.append("```rust\n");
        p.append("let a = 1;\n");
        let d = p.append("let b");
        assert!(
            matches!(&d.ops[..], [Op::SpliceCode { truncate_bytes: 0, append, .. }] if append == "let b")
        );
        let d = p.append(" = 2;\n```\n");
        assert!(matches!(d.ops.last(), Some(Op::SealCode { .. })));
    }

    #[test]
    fn every_delta_reconstructs_document_including_unicode() {
        let chunks = [
            "# 速",
            "い\n\n段落 **途",
            "中**\n\n```rust\n",
            "println!(\"日",
            "本語\");\n",
            "```\n- a\n",
            "- b",
        ];
        let mut parser = Parser::new();
        let mut mirror = Vec::new();
        for chunk in chunks {
            let delta = parser.append(chunk);
            apply(&mut mirror, &delta);
            assert_eq!(mirror, parser.blocks());
        }
    }

    #[test]
    fn math_source_bypasses_markdown_inline_parsing() {
        let markdown = r#"$$
A_3=\begin{pmatrix}
a_1 & x_1 \\
a_2 & \frac{1}{2}
\end{pmatrix}
$$
"#;
        let mut parser = Parser::new();
        parser.append(markdown);
        let Block::Paragraph(nodes) = &parser.blocks()[0] else {
            panic!("expected paragraph")
        };
        let Inline::Math { source, display } = &nodes[0] else {
            panic!("expected math node")
        };
        assert!(*display);
        assert!(source.contains(r"a_1 & x_1 \"));
        assert!(source.contains(r"\frac{1}{2}"));
        assert!(!nodes.iter().any(|node| matches!(node, Inline::Emphasis(_))));
    }

    #[test]
    fn standalone_brackets_are_display_math() {
        let mut parser = Parser::new();
        parser.append("[\n\\rho=\\frac{N}{D}\n]\n");
        let Block::Paragraph(nodes) = &parser.blocks()[0] else {
            panic!("expected paragraph")
        };
        assert_eq!(
            nodes,
            &[Inline::Math {
                source: "\\rho=\\frac{N}{D}".to_owned(),
                display: true,
            }]
        );
    }

    #[test]
    fn pipe_table_becomes_table_ast() {
        let mut parser = Parser::new();
        parser.append("| Name | Value |\n| --- | ---: |\n| alpha | 42 |\n");
        let Block::Table { headers, rows } = &parser.blocks()[0] else {
            panic!("expected table")
        };
        assert_eq!(headers.len(), 2);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), 2);
    }

    #[test]
    fn soft_break_is_distinct_from_space_hard_break() {
        let mut parser = Parser::new();
        parser.append("one\ntwo\nthree  \nfour   \nfive\n");
        let Block::Paragraph(nodes) = &parser.blocks()[0] else {
            panic!("expected paragraph")
        };
        assert!(matches!(nodes[1], Inline::SoftBreak));
        assert!(matches!(nodes[5], Inline::HardBreak));
        assert!(matches!(nodes[7], Inline::HardBreak));
    }

    #[test]
    fn reset_clears_live_and_fenced_state() {
        let mut parser = Parser::new();
        parser.append("```rust\nlet unfinished");
        let delta = parser.reset();
        assert!(parser.is_empty());
        assert_eq!(delta.ops, vec![Op::Truncate { from: 0 }]);

        parser.append("# Fresh");
        assert!(matches!(parser.blocks(), [Block::Heading { .. }]));
        assert!(parser.reset().ops.len() == 1);
        assert!(parser.reset().ops.is_empty());
    }

    #[test]
    fn replace_returns_a_delta_for_the_old_document() {
        let mut parser = Parser::new();
        parser.append("Old response");
        let delta = parser.replace("# New response");
        assert!(matches!(delta.ops.first(), Some(Op::Truncate { from: 0 })));
        assert!(matches!(
            delta.ops.last(),
            Some(Op::Push(Block::Heading { .. }))
        ));

        let mut mirror = vec![Block::Paragraph(vec![Inline::Text("Old response".into())])];
        apply(&mut mirror, &delta);
        assert_eq!(mirror, parser.blocks());
    }

    #[test]
    fn finish_closes_a_fence_without_a_trailing_newline() {
        let mut parser = Parser::new();
        parser.append("```rust\nlet answer = 42;\n```");
        let delta = parser.finish();
        assert!(matches!(delta.ops.last(), Some(Op::SealCode { block: 0 })));
        assert!(matches!(
            parser.blocks(),
            [Block::CodeBlock {
                text,
                closed: true,
                ..
            }] if text == "let answer = 42;\n"
        ));
    }

    #[test]
    fn llm_semantic_fence_uses_tail_splices() {
        let mut parser = Parser::new();
        let open = parser.append(":::llm tool name=search id=q1\n");
        assert!(matches!(
            open.ops.as_slice(),
            [Op::Push(Block::CodeBlock { language: Some(language), closed: false, .. })]
                if language == "llm:tool name=search id=q1"
        ));

        let body = parser.append("{\"query\":\"rust wasm\"}");
        assert!(matches!(
            body.ops.as_slice(),
            [Op::SpliceCode { truncate_bytes: 0, append, .. }]
                if append == "{\"query\":\"rust wasm\"}"
        ));

        let close = parser.append("\n:::\n");
        assert!(matches!(close.ops.last(), Some(Op::SealCode { block: 0 })));
        assert!(matches!(
            parser.blocks(),
            [Block::CodeBlock { language: Some(language), text, closed: true }]
                if language == "llm:tool name=search id=q1"
                    && text == "{\"query\":\"rust wasm\"}\n"
        ));
    }

    #[test]
    fn longer_llm_fence_allows_short_colons_in_body() {
        let mut parser = Parser::new();
        parser.append("::::llm artifact mime=text/plain\n");
        let middle = parser.append("alpha\n:::\nomega\n");
        assert!(
            !middle
                .ops
                .iter()
                .any(|op| matches!(op, Op::SealCode { .. }))
        );
        let close = parser.append("::::\n");
        assert!(matches!(close.ops.last(), Some(Op::SealCode { block: 0 })));
        assert!(matches!(
            parser.blocks(),
            [Block::CodeBlock { text, closed: true, .. }]
                if text == "alpha\n:::\nomega\n"
        ));
    }

    #[test]
    fn llm_fence_is_chunk_boundary_independent() {
        let markdown =
            "before\n\n:::llm artifact mime=application/json name=plan\n{\"step\":1}\n:::\n\nafter";
        let mut whole = Parser::new();
        whole.append(markdown);

        let mut streamed = Parser::new();
        for chunk in [
            "before\n\n:::l",
            "lm artifact mime=application/json name=plan\n{\"st",
            "ep\":1}\n::",
            ":\n\nafter",
        ] {
            streamed.append(chunk);
        }
        assert_eq!(whole.blocks(), streamed.blocks());
    }

    #[test]
    fn ordinary_colon_fences_remain_markdown_text() {
        let mut parser = Parser::new();
        parser.append(":::note\nnot an llm fence\n:::\n");
        assert!(matches!(parser.blocks(), [Block::Paragraph(_)]));

        let mut parser = Parser::new();
        parser.append(":::llmish tool\nnot an llm fence\n");
        assert!(matches!(parser.blocks(), [Block::Paragraph(_)]));
    }

    #[test]
    fn formatted_live_paragraph_uses_inline_tail_delta() {
        let mut parser = Parser::new();
        parser.append("Answer with **important** context: ");
        let delta = parser.append("token ");
        assert_eq!(
            delta.ops,
            vec![Op::AppendInlineText {
                block: 0,
                append: "token ".to_owned(),
            }]
        );
        assert!(matches!(
            parser.blocks(),
            [Block::Paragraph(nodes)]
                if matches!(nodes.as_slice(), [Inline::Text(_), Inline::Strong(_), Inline::Text(text)] if text == " context: token ")
        ));

        // Syntax-sensitive input must still reparse the complete live source.
        let delta = parser.append("[[cite:doc-1]]");
        assert!(matches!(delta.ops.first(), Some(Op::Truncate { from: 0 })));
        assert!(matches!(
            parser.blocks(),
            [Block::Paragraph(nodes)]
                if nodes.iter().any(|node| matches!(node, Inline::Link { destination, .. } if destination == "llm:cite:doc-1"))
        ));
    }

    #[test]
    fn multiline_formatted_paragraph_uses_inline_tail_delta() {
        let mut parser = Parser::new();
        parser.append("Answer with **important** first line\ncontinuation ");
        let delta = parser.append("token ");
        assert_eq!(
            delta.ops,
            vec![Op::AppendInlineText {
                block: 0,
                append: "token ".to_owned(),
            }]
        );
        assert!(matches!(
            parser.blocks(),
            [Block::Paragraph(nodes)]
                if nodes.iter().any(|node| matches!(node, Inline::SoftBreak))
                    && matches!(nodes.last(), Some(Inline::Text(text)) if text.ends_with("continuation token "))
        ));
    }

    #[test]
    fn first_token_after_soft_break_reparses_then_enables_inline_tail() {
        let mut parser = Parser::new();
        parser.append("Answer with **important** first line\n");
        let first = parser.append("continuation ");
        assert!(matches!(first.ops.first(), Some(Op::Truncate { from: 0 })));
        let second = parser.append("token ");
        assert!(matches!(
            second.ops.as_slice(),
            [Op::AppendInlineText { block: 0, .. }]
        ));
        assert!(matches!(
            parser.blocks(),
            [Block::Paragraph(nodes)]
                if nodes.iter().any(|node| matches!(node, Inline::SoftBreak))
        ));
    }

    #[test]
    fn inline_tail_fast_path_respects_cross_chunk_escape() {
        let mut parser = Parser::new();
        parser.append("Formatted **x** \\");
        let delta = parser.append("!");
        assert!(matches!(delta.ops.first(), Some(Op::Truncate { from: 0 })));
        assert!(matches!(
            parser.blocks(),
            [Block::Paragraph(nodes)]
                if matches!(nodes.last(), Some(Inline::Text(text)) if text.ends_with(" !"))
        ));
    }

    #[test]
    fn two_dashes_reparse_when_they_become_thematic_break() {
        let mut parser = Parser::new();
        parser.append("--");
        let delta = parser.append("-\n");
        assert!(matches!(delta.ops.first(), Some(Op::Truncate { from: 0 })));
        assert!(matches!(parser.blocks(), [Block::ThematicBreak]));
    }

    #[test]
    fn delimiter_runs_splice_only_the_inline_tail() {
        let mut code = Parser::new();
        code.append("`");
        let close = code.append("`");
        assert_eq!(
            close.ops,
            vec![Op::SpliceInlineTail {
                block: 0,
                remove_nodes: 0,
                truncate_bytes: 1,
                append: vec![Inline::Code(String::new())],
            }]
        );
        assert_eq!(
            code.blocks(),
            &[Block::Paragraph(vec![Inline::Code(String::new())])]
        );
        let more = code.append("```");
        assert!(matches!(
            more.ops.as_slice(),
            [Op::SpliceInlineTail { truncate_bytes: 0, append, .. }]
                if append == &vec![Inline::Code(String::new()), Inline::Text("`".to_owned())]
        ));

        let mut math = Parser::new();
        math.append("$");
        math.append("$");
        math.append("$");
        let close = math.append("$");
        assert_eq!(
            close.ops,
            vec![Op::SpliceInlineTail {
                block: 0,
                remove_nodes: 0,
                truncate_bytes: 3,
                append: vec![Inline::Math {
                    source: String::new(),
                    display: true,
                }],
            }]
        );
        assert_eq!(
            math.blocks(),
            &[Block::Paragraph(vec![Inline::Math {
                source: String::new(),
                display: true,
            }])]
        );

        let mut normal_code = Parser::new();
        normal_code.append("`");
        normal_code.append("body");
        let close = normal_code.append("`");
        assert!(matches!(close.ops.first(), Some(Op::Truncate { from: 0 })));
        let mut whole_code = Parser::new();
        whole_code.append("`body`");
        assert_eq!(normal_code.blocks(), whole_code.blocks());

        let mut normal_math = Parser::new();
        normal_math.append("$");
        normal_math.append("x+1");
        let close = normal_math.append("$");
        assert!(matches!(close.ops.first(), Some(Op::Truncate { from: 0 })));
        let mut whole_math = Parser::new();
        whole_math.append("$x+1$");
        assert_eq!(normal_math.blocks(), whole_math.blocks());

        let mut escaped = Parser::new();
        escaped.append("\\`");
        let fallback = escaped.append("`");
        assert!(matches!(
            fallback.ops.first(),
            Some(Op::Truncate { from: 0 })
        ));
        let mut whole = Parser::new();
        whole.append("\\``");
        assert_eq!(escaped.blocks(), whole.blocks());
    }

    #[test]
    fn emphasis_delimiter_runs_splice_tail_nodes() {
        for delimiter in ['*', '_'] {
            let token = delimiter.to_string();
            let mut parser = Parser::new();
            parser.append("prefix ");
            parser.append(&token);
            let second = parser.append(&token);
            assert!(matches!(
                second.ops.as_slice(),
                [Op::SpliceInlineTail {
                    remove_nodes: 0,
                    truncate_bytes: 1,
                    append,
                    ..
                }] if append == &vec![Inline::Emphasis(Vec::new())]
            ));
            parser.append(&token);
            let fourth = parser.append(&token);
            assert!(matches!(
                fourth.ops.as_slice(),
                [Op::SpliceInlineTail {
                    remove_nodes: 1,
                    truncate_bytes: 1,
                    append,
                    ..
                }] if append == &vec![Inline::Strong(Vec::new())]
            ));

            let mut whole = Parser::new();
            whole.append(&format!("prefix {}", token.repeat(4)));
            assert_eq!(parser.blocks(), whole.blocks());

            let mut normal = Parser::new();
            normal.append("prefix ");
            normal.append(&token);
            normal.append("body");
            let close = normal.append(&token);
            assert!(matches!(close.ops.first(), Some(Op::Truncate { from: 0 })));
            let mut whole = Parser::new();
            whole.append(&format!("prefix {delimiter}body{delimiter}"));
            assert_eq!(normal.blocks(), whole.blocks());
        }
    }

    #[test]
    fn special_bracket_closer_only_reparses_valid_llm_tokens() {
        let mut invalid_ref = Parser::new();
        invalid_ref.append("@[x");
        let close = invalid_ref.append("]");
        assert!(matches!(
            close.ops.as_slice(),
            [Op::AppendInlineText { append, .. }] if append == "]"
        ));
        let mut whole = Parser::new();
        whole.append("@[x]");
        assert_eq!(invalid_ref.blocks(), whole.blocks());

        let mut valid_ref = Parser::new();
        valid_ref.append("@[source:id");
        let close = valid_ref.append("]");
        assert!(matches!(close.ops.first(), Some(Op::Truncate { from: 0 })));
        assert!(matches!(
            valid_ref.blocks(),
            [Block::Paragraph(nodes)] if nodes.iter().any(|node| matches!(
                node,
                Inline::Link { destination, .. } if destination == "llm:source:id"
            ))
        ));

        let mut citation = Parser::new();
        citation.append("[[cite:doc");
        let first = citation.append("]");
        assert!(matches!(
            first.ops.as_slice(),
            [Op::AppendInlineText { append, .. }] if append == "]"
        ));
        let second = citation.append("]");
        assert!(matches!(second.ops.first(), Some(Op::Truncate { from: 0 })));
        assert!(matches!(
            citation.blocks(),
            [Block::Paragraph(nodes)] if nodes.iter().any(|node| matches!(
                node,
                Inline::Link { destination, .. } if destination == "llm:cite:doc"
            ))
        ));

        let mut invalid_cite = Parser::new();
        invalid_cite.append("[[cite:bad id");
        invalid_cite.append("]");
        let second = invalid_cite.append("]");
        assert!(matches!(
            second.ops.as_slice(),
            [Op::AppendInlineText { append, .. }] if append == "]"
        ));
        let mut whole = Parser::new();
        whole.append("[[cite:bad id]]");
        assert_eq!(invalid_cite.blocks(), whole.blocks());
    }

    #[test]
    fn inert_closer_runs_append_until_a_real_opener_exists() {
        let mut parser = Parser::new();
        parser.append("]");
        let brackets = parser.append("]]]");
        assert_eq!(
            brackets.ops,
            vec![Op::AppendInlineText {
                block: 0,
                append: "]]]".to_owned(),
            }]
        );

        parser.append("[");
        let label_close = parser.append("]");
        assert!(matches!(
            label_close.ops.as_slice(),
            [Op::AppendInlineText { append, .. }] if append == "]"
        ));
        let destination_open = parser.append("(");
        assert!(matches!(
            destination_open.ops.as_slice(),
            [Op::AppendInlineText { append, .. }] if append == "("
        ));
        parser.append("url");
        let link_close = parser.append(")");
        assert!(matches!(
            link_close.ops.first(),
            Some(Op::Truncate { from: 0 })
        ));
        assert!(matches!(
            parser.blocks(),
            [Block::Paragraph(nodes)] if nodes.iter().any(|node| matches!(
                node,
                Inline::Link { destination, .. } if destination == "url"
            ))
        ));
        let stale_bracket = parser.append("]");
        assert!(matches!(
            stale_bracket.ops.as_slice(),
            [Op::AppendInlineText { append, .. }] if append == "]"
        ));

        let mut parens = Parser::new();
        parens.append(")");
        let inert = parens.append(")))");
        assert!(matches!(
            inert.ops.as_slice(),
            [Op::AppendInlineText { append, .. }] if append == ")))"
        ));

        parens.append(" [x](url");
        let real_close = parens.append(")");
        assert!(matches!(
            real_close.ops.first(),
            Some(Op::Truncate { from: 0 })
        ));
        assert!(matches!(
            parens.blocks(),
            [Block::Paragraph(nodes)] if nodes.iter().any(|node| matches!(
                node,
                Inline::Link { destination, .. } if destination == "url"
            ))
        ));
        let stale_paren = parens.append(")");
        assert!(matches!(
            stale_paren.ops.as_slice(),
            [Op::AppendInlineText { append, .. }] if append == ")"
        ));
    }

    #[test]
    fn backslash_runs_use_parity_delta_then_reparse_on_escape_completion() {
        let mut parser = Parser::new();
        parser.append("\\");

        let even = parser.append("\\");
        assert!(even.ops.is_empty());
        assert_eq!(
            parser.blocks(),
            &[Block::Paragraph(vec![Inline::Text("\\".to_owned())])]
        );

        let odd = parser.append("\\");
        assert_eq!(
            odd.ops,
            vec![Op::AppendInlineText {
                block: 0,
                append: "\\".to_owned(),
            }]
        );
        assert_eq!(
            parser.blocks(),
            &[Block::Paragraph(vec![Inline::Text("\\\\".to_owned())])]
        );

        let completed_escape = parser.append("*");
        assert!(matches!(
            completed_escape.ops.first(),
            Some(Op::Truncate { from: 0 })
        ));

        let mut whole = Parser::new();
        whole.append(r"\\\*");
        assert_eq!(parser.blocks(), whole.blocks());
    }

    #[test]
    fn unmatched_inline_openers_append_without_reparse() {
        let mut parser = Parser::new();
        parser.append("[");
        let delta = parser.append("[@[");
        assert_eq!(
            delta.ops,
            vec![Op::AppendInlineText {
                block: 0,
                append: "[@[".to_owned(),
            }]
        );
        assert!(matches!(
            parser.blocks(),
            [Block::Paragraph(nodes)]
                if matches!(nodes.as_slice(), [Inline::Text(text)] if text == "[[@[")
        ));

        // A closing delimiter can complete syntax from arbitrarily far back, so
        // it must leave the append-only path and rebuild the live paragraph.
        let delta = parser.append("source:id]");
        assert!(matches!(delta.ops.first(), Some(Op::Truncate { from: 0 })));
        assert!(matches!(
            parser.blocks(),
            [Block::Paragraph(nodes)]
                if nodes.iter().any(|node| matches!(
                    node,
                    Inline::Link { destination, .. } if destination == "llm:source:id"
                ))
        ));
    }

    #[test]
    fn plain_live_paragraph_uses_append_text_delta() {
        let mut parser = Parser::new();
        parser.append("token ");
        let delta = parser.append("stream ");
        assert_eq!(
            delta.ops,
            vec![Op::AppendText {
                block: 0,
                append: "stream ".to_owned(),
            }]
        );
        assert_eq!(
            parser.blocks(),
            &[Block::Paragraph(vec![Inline::Text(
                "token stream ".to_owned()
            )])]
        );

        // A syntax-sensitive byte forces the normal reparse path.
        let delta = parser.append("**fast**");
        assert!(matches!(delta.ops.first(), Some(Op::Truncate { from: 0 })));
        assert!(matches!(
            parser.blocks(),
            [Block::Paragraph(nodes)] if nodes.iter().any(|n| matches!(n, Inline::Strong(_)))
        ));
    }

    fn apply(document: &mut Vec<Block>, delta: &Delta) {
        for op in &delta.ops {
            match op {
                Op::Truncate { from } => document.truncate(*from as usize),
                Op::Push(block) => document.push(block.clone()),
                Op::AppendText { block, append } => {
                    let Block::Paragraph(nodes) = &mut document[*block as usize] else {
                        panic!("not paragraph")
                    };
                    let [Inline::Text(text)] = nodes.as_mut_slice() else {
                        panic!("paragraph is not plain text")
                    };
                    text.push_str(append);
                }
                Op::AppendInlineText { block, append } => {
                    let Block::Paragraph(nodes) = &mut document[*block as usize] else {
                        panic!("not paragraph")
                    };
                    if let Some(Inline::Text(text)) = nodes.last_mut() {
                        text.push_str(append);
                    } else {
                        nodes.push(Inline::Text(append.clone()));
                    }
                }
                Op::SealCode { block } => {
                    let Block::CodeBlock { closed, .. } = &mut document[*block as usize] else {
                        panic!("not code")
                    };
                    *closed = true;
                }
                Op::SpliceInlineTail {
                    block,
                    remove_nodes,
                    truncate_bytes,
                    append,
                } => {
                    let Block::Paragraph(nodes) = &mut document[*block as usize] else {
                        panic!("not paragraph")
                    };
                    splice_inline_tail(
                        nodes,
                        *truncate_bytes as usize,
                        *remove_nodes as usize,
                        append,
                    );
                }
                Op::SpliceCode {
                    block,
                    truncate_bytes,
                    append,
                } => {
                    let Block::CodeBlock { text, .. } = &mut document[*block as usize] else {
                        panic!("not code")
                    };
                    text.truncate(text.len() - *truncate_bytes as usize);
                    text.push_str(append);
                }
            }
        }
    }
}
