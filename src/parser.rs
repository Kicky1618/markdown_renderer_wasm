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
    /// Append one item to the tail of a live ordered/unordered list.
    AppendListItem { block: u32, item: Vec<Inline> },
    /// Edit only the inline tail of the final item in a live list.
    SpliceListItemTail {
        block: u32,
        remove_nodes: u32,
        truncate_bytes: u32,
        append: Vec<Inline>,
    },
    /// Edit only the inline tail of a live block quote.
    SpliceQuoteTail {
        block: u32,
        remove_nodes: u32,
        truncate_bytes: u32,
        append: Vec<Inline>,
    },
    /// Append a parsed data row to a live table.
    AppendTableRow { block: u32, row: Vec<Vec<Inline>> },
    /// Append one parsed cell to the final row of a live table.
    AppendTableCell { block: u32, cell: Vec<Inline> },
    /// Edit only the inline tail of the final cell in the final live table row.
    SpliceTableCellTail {
        block: u32,
        remove_nodes: u32,
        truncate_bytes: u32,
        append: Vec<Inline>,
    },
    /// Edit only the inline tail of a live heading.
    SpliceHeadingTail {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TailPendingKind {
    Code,
    Math,
    DisplayMath,
    EmphasisStar,
    EmphasisUnderscore,
    StrongStar,
    StrongUnderscore,
}

impl TailPendingKind {
    fn delimiter(self) -> u8 {
        match self {
            Self::Code => b'`',
            Self::Math | Self::DisplayMath => b'$',
            Self::EmphasisStar | Self::StrongStar => b'*',
            Self::EmphasisUnderscore | Self::StrongUnderscore => b'_',
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TailPending {
    kind: TailPendingKind,
    opener: usize,
    close_seen: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingLineBreak {
    Soft,
    Hard { truncate_bytes: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinkTailPending {
    Label {
        opener: usize,
        opener_node: usize,
        opener_text_prefix: usize,
    },
    Closed {
        opener: usize,
        label_close: usize,
        opener_node: usize,
        opener_text_prefix: usize,
    },
    Destination {
        opener: usize,
        label_close: usize,
        destination_open: usize,
        opener_node: usize,
        opener_text_prefix: usize,
    },
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
    live_quote_ambiguous: bool,
    live_table_row: bool,
    live_table_cell_safe: bool,
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
    live_special_opener: usize,
    live_has_link_label_open: bool,
    live_link_label_just_closed: bool,
    live_has_link_destination_start: bool,
    live_link_tail_pending: Option<LinkTailPending>,
    live_link_fast_ambiguous: bool,
    live_thematic_marker: u8,
    live_tail_pending: Option<TailPending>,
    live_multiline_plain_safe: bool,
    live_pending_line_break: Option<PendingLineBreak>,
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
            live_quote_ambiguous: false,
            live_table_row: false,
            live_table_cell_safe: false,
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
            live_special_opener: 0,
            live_has_link_label_open: false,
            live_link_label_just_closed: false,
            live_has_link_destination_start: false,
            live_link_tail_pending: None,
            live_link_fast_ambiguous: false,
            live_thematic_marker: 0,
            live_tail_pending: None,
            live_multiline_plain_safe: false,
            live_pending_line_break: None,
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
                self.live_quote_ambiguous = false;
                self.live_table_row = false;
                self.live_table_cell_safe = false;
                self.trailing_backslash_odd = false;
                self.reset_delimiter_runs();
                self.live_special_bracket = SpecialBracketKind::None;
                self.live_special_opener = 0;
                self.live_has_link_label_open = false;
                self.live_link_label_just_closed = false;
                self.live_has_link_destination_start = false;
                self.live_link_tail_pending = None;
                self.live_link_fast_ambiguous = false;
                self.live_thematic_marker = 0;
                self.live_tail_pending = None;
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
                    self.live_special_opener = 0;
                    self.live_has_link_label_open = false;
                    self.live_link_label_just_closed = false;
                    self.live_has_link_destination_start = false;
                    self.live_link_tail_pending = None;
                    self.live_link_fast_ambiguous = false;
                    self.live_thematic_marker = 0;
                    self.live_tail_pending = None;
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
        let force_normal = self.live_pending_line_break.is_some();
        if self.try_append_stable_multiline_paragraph(input, delta) {
            return;
        }
        if force_normal {
            self.live_pending_line_break = None;
            self.live_multiline_plain_safe = false;
        } else {
            if self.try_append_thematic(input) {
                return;
            }
            if self.try_append_list_tail(input, delta) {
                return;
            }
            if self.try_append_quote_tail(input, delta) {
                return;
            }
            if self.try_append_table_tail(input, delta) {
                return;
            }
            if self.try_append_plain(input, delta) {
                return;
            }
            if self.try_append_heading_tail(input, delta) {
                return;
            }
            if self.try_append_complete_semantic_chunk(input, delta) {
                return;
            }
            if self.try_append_complete_link_chunk(input, delta) {
                return;
            }
            if self.try_append_inline_text(input, delta) {
                return;
            }
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

    fn try_append_stable_multiline_paragraph(&mut self, input: &str, delta: &mut Delta) -> bool {
        if input.is_empty()
            || !matches!(self.mode, Mode::Normal)
            || !self.has_live
            || self.pending_kind != Some(PendingKind::Paragraph)
        {
            return false;
        }
        let Some(block_index) = self.blocks.len().checked_sub(1) else {
            return false;
        };
        let Some(Block::Paragraph(nodes)) = self.blocks.get_mut(block_index) else {
            return false;
        };

        if let Some(pending_break) = self.live_pending_line_break {
            if input.contains(['\n', '\r']) || !input.bytes().all(is_plain_stream_byte) {
                return false;
            }
            let (break_node, truncate_bytes) = match pending_break {
                PendingLineBreak::Soft => (Inline::SoftBreak, 0),
                PendingLineBreak::Hard { truncate_bytes } => (Inline::HardBreak, truncate_bytes),
            };
            let append = vec![break_node, Inline::Text(input.to_owned())];
            splice_inline_tail(nodes, truncate_bytes, 0, &append);
            self.line.push_str(input);
            self.live_pending_line_break = None;
            self.live_plain = false;
            self.live_inline_appendable = true;
            self.trailing_backslash_odd = false;
            self.reset_delimiter_runs();
            self.live_special_bracket = SpecialBracketKind::None;
            self.live_special_opener = 0;
            self.live_has_link_label_open = false;
            self.live_link_label_just_closed = false;
            self.live_has_link_destination_start = false;
            self.live_link_tail_pending = None;
            self.live_link_fast_ambiguous = false;
            self.live_tail_pending = None;
            delta.ops.push(Op::SpliceInlineTail {
                block: block_index as u32,
                remove_nodes: 0,
                truncate_bytes: truncate_bytes as u32,
                append,
            });
            return true;
        }

        if input != "\n"
            || !self.live_multiline_plain_safe
            || self.line.trim().is_empty()
            || classify(&self.line) != PendingKind::Paragraph
            || heading(&self.line).is_some()
            || is_thematic(&self.line)
            || llm_fence_open(&self.line).is_some()
            || fence_open(&self.line).is_some()
        {
            return false;
        }

        let trailing_spaces = self
            .line
            .as_bytes()
            .iter()
            .rev()
            .take_while(|&&byte| byte == b' ')
            .count();
        self.live_pending_line_break = Some(if trailing_spaces >= 2 {
            PendingLineBreak::Hard {
                truncate_bytes: trailing_spaces,
            }
        } else {
            PendingLineBreak::Soft
        });
        self.pending.push_str(&self.line);
        self.pending.push('\n');
        self.line.clear();
        self.live_plain = false;
        self.live_inline_appendable = false;
        self.trailing_backslash_odd = false;
        self.reset_delimiter_runs();
        self.live_special_bracket = SpecialBracketKind::None;
        self.live_special_opener = 0;
        self.live_has_link_label_open = false;
        self.live_link_label_just_closed = false;
        self.live_has_link_destination_start = false;
        self.live_link_tail_pending = None;
        self.live_link_fast_ambiguous = false;
        self.live_tail_pending = None;
        true
    }

    fn try_append_thematic(&mut self, input: &str) -> bool {
        let marker = self.live_thematic_marker;
        if marker == 0
            || input.is_empty()
            || !matches!(self.mode, Mode::Normal)
            || !self.has_live
            || !matches!(self.blocks.last(), Some(Block::ThematicBreak))
            || !input.bytes().all(|byte| {
                byte != b'\n' && byte != b'\r' && (byte == marker || byte.is_ascii_whitespace())
            })
        {
            return false;
        }
        self.line.push_str(input);
        true
    }

    fn try_append_table_tail(&mut self, input: &str, delta: &mut Delta) -> bool {
        if input.is_empty()
            || !matches!(self.mode, Mode::Normal)
            || !self.has_live
            || self.pending_kind != Some(PendingKind::Paragraph)
            || input.contains('\r')
            || !pending_has_two_complete_lines(&self.pending)
        {
            return false;
        }
        let Some(block_index) = self.blocks.len().checked_sub(1) else {
            return false;
        };
        if !matches!(self.blocks.get(block_index), Some(Block::Table { .. })) {
            return false;
        }

        if self.try_append_complete_table_row(block_index, input, delta) {
            return true;
        }
        if self.try_append_table_cell_token(block_index, input, delta) {
            return true;
        }

        if input == "\n" {
            if self.line.is_empty() {
                return false;
            }
            self.pending.push_str(&self.line);
            self.pending.push('\n');
            self.line.clear();
            self.live_table_row = false;
            self.live_table_cell_safe = false;
            return true;
        }
        if input.contains('\n') || input.chars().count() != 1 {
            return false;
        }

        let old_row = split_table_row(&self.line);
        let mut candidate = self.line.clone();
        candidate.push_str(input);
        let new_row = split_table_row(&candidate);

        let Some(new_row) = new_row else {
            self.line = candidate;
            self.live_table_row = false;
            self.live_table_cell_safe = false;
            return true;
        };
        if !table_stream_row_is_plain(&new_row)
            || old_row
                .as_ref()
                .is_some_and(|row| !table_stream_row_is_plain(row))
        {
            return false;
        }

        let transition = match old_row {
            None => TableTailTransition::AppendRow(
                new_row
                    .iter()
                    .map(|cell| plain_cell_inlines(cell))
                    .collect(),
            ),
            Some(old_row) if new_row.len() == old_row.len() => {
                if old_row.is_empty()
                    || old_row[..old_row.len() - 1] != new_row[..new_row.len() - 1]
                {
                    return false;
                }
                let old_last = &old_row[old_row.len() - 1];
                let new_last = &new_row[new_row.len() - 1];
                if new_last == old_last {
                    TableTailTransition::None
                } else if let Some(suffix) = new_last.strip_prefix(old_last) {
                    TableTailTransition::AppendCellText(suffix.to_owned())
                } else {
                    return false;
                }
            }
            Some(old_row) if new_row.len() == old_row.len() + 1 => {
                if old_row != new_row[..old_row.len()] {
                    return false;
                }
                TableTailTransition::AppendCell(plain_cell_inlines(&new_row[new_row.len() - 1]))
            }
            Some(_) => return false,
        };

        let Some(Block::Table { rows, .. }) = self.blocks.get_mut(block_index) else {
            unreachable!()
        };
        match transition {
            TableTailTransition::None => {}
            TableTailTransition::AppendRow(row) => {
                rows.push(row.clone());
                delta.ops.push(Op::AppendTableRow {
                    block: block_index as u32,
                    row,
                });
            }
            TableTailTransition::AppendCell(cell) => {
                let Some(row) = rows.last_mut() else {
                    return false;
                };
                row.push(cell.clone());
                delta.ops.push(Op::AppendTableCell {
                    block: block_index as u32,
                    cell,
                });
            }
            TableTailTransition::AppendCellText(append_text) => {
                let Some(cell) = rows.last_mut().and_then(|row| row.last_mut()) else {
                    return false;
                };
                let append = if append_text.is_empty() {
                    Vec::new()
                } else {
                    vec![Inline::Text(append_text)]
                };
                splice_inline_tail(cell, 0, 0, &append);
                delta.ops.push(Op::SpliceTableCellTail {
                    block: block_index as u32,
                    remove_nodes: 0,
                    truncate_bytes: 0,
                    append,
                });
            }
        }
        self.line = candidate;
        self.live_table_row = !self.line.trim_end().ends_with('|');
        self.live_table_cell_safe = self.live_table_row;
        true
    }

    fn try_append_table_cell_token(
        &mut self,
        block_index: usize,
        input: &str,
        delta: &mut Delta,
    ) -> bool {
        if !self.live_table_row
            || input.is_empty()
            || input.contains('|')
            || input.contains('\n')
            || !input.bytes().all(is_plain_stream_byte)
        {
            return false;
        }

        if !self.live_table_cell_safe {
            return false;
        }
        let Some(cell) = self.blocks.get(block_index).and_then(|block| match block {
            Block::Table { rows, .. } => rows.last().and_then(|row| row.last()),
            _ => None,
        }) else {
            return false;
        };
        let cell_is_empty = cell.is_empty();

        if input.chars().all(char::is_whitespace) {
            self.line.push_str(input);
            return true;
        }

        let append_text = if cell_is_empty {
            input.trim().to_owned()
        } else {
            let visible_input = input.trim_end();
            let old_visible_end = self.line.trim_end().len();
            let old_trailing = &self.line[old_visible_end..];
            let mut append = String::with_capacity(old_trailing.len() + visible_input.len());
            append.push_str(old_trailing);
            append.push_str(visible_input);
            append
        };
        self.line.push_str(input);
        if append_text.is_empty() {
            return true;
        }

        let append = vec![Inline::Text(append_text)];
        let Some(cell) = self
            .blocks
            .get_mut(block_index)
            .and_then(|block| match block {
                Block::Table { rows, .. } => rows.last_mut().and_then(|row| row.last_mut()),
                _ => None,
            })
        else {
            unreachable!("live table row lost its final cell");
        };
        splice_inline_tail(cell, 0, 0, &append);
        delta.ops.push(Op::SpliceTableCellTail {
            block: block_index as u32,
            remove_nodes: 0,
            truncate_bytes: 0,
            append,
        });
        true
    }

    fn try_append_complete_table_row(
        &mut self,
        block_index: usize,
        input: &str,
        delta: &mut Delta,
    ) -> bool {
        // A whole-row fast path is only safe between completed rows. Partial
        // rows continue through the existing cell-tail state machine.
        if !self.line.is_empty() {
            return false;
        }
        let (raw, sealed) = if let Some(raw) = input.strip_suffix('\n') {
            if raw.contains('\n') {
                return false;
            }
            (raw, true)
        } else {
            if input.contains('\n') {
                return false;
            }
            (input, false)
        };
        if raw.is_empty() {
            return false;
        }
        let Some(cells) = split_table_row(raw) else {
            return false;
        };
        if cells.is_empty() {
            return false;
        }
        let row = cells
            .iter()
            .map(|cell| parse_inlines(cell))
            .collect::<Vec<_>>();
        let cell_safe = row
            .last()
            .is_some_and(|cell| quote_line_inlines_are_self_contained(cell));
        let Some(Block::Table { rows, .. }) = self.blocks.get_mut(block_index) else {
            return false;
        };
        rows.push(row.clone());
        delta.ops.push(Op::AppendTableRow {
            block: block_index as u32,
            row,
        });
        if sealed {
            self.pending.push_str(raw);
            self.pending.push('\n');
            self.line.clear();
            self.live_table_row = false;
            self.live_table_cell_safe = false;
        } else {
            self.line.push_str(raw);
            self.live_table_row = !self.line.trim_end().ends_with('|');
            self.live_table_cell_safe = self.live_table_row && cell_safe;
        }
        true
    }

    fn try_append_quote_tail(&mut self, input: &str, delta: &mut Delta) -> bool {
        if input.is_empty()
            || !matches!(self.mode, Mode::Normal)
            || !self.has_live
            || self.pending_kind != Some(PendingKind::Quote)
            || input.contains('\r')
        {
            return false;
        }
        let Some(block_index) = self.blocks.len().checked_sub(1) else {
            return false;
        };
        if self.try_append_complete_quote_line(block_index, input, delta) {
            return true;
        }
        let Some(Block::BlockQuote(nodes)) = self.blocks.get_mut(block_index) else {
            return false;
        };

        let old_body = quote_line_body(&self.line);
        if !old_body.bytes().all(is_plain_stream_byte) {
            return false;
        }

        if input == "\n" {
            if self.line.is_empty() {
                return false;
            }
            self.pending.push_str(&self.line);
            self.pending.push('\n');
            self.line.clear();
            return true;
        }
        if input.contains('\n') || !input.bytes().all(is_plain_stream_byte) {
            return false;
        }

        let old_body_len = old_body.len();
        self.line.push_str(input);
        let new_body = quote_line_body(&self.line);
        if new_body.len() < old_body_len {
            return false;
        }
        let append_text = &new_body[old_body_len..];
        if append_text.is_empty() {
            return true;
        }

        let starts_new_line = old_body_len == 0 && !self.pending.is_empty();
        if starts_new_line && !quote_previous_line_can_append_break(&self.pending) {
            self.line.truncate(self.line.len() - input.len());
            return false;
        }

        let mut append = Vec::with_capacity(2);
        if starts_new_line {
            append.push(Inline::SoftBreak);
        }
        append.push(Inline::Text(append_text.to_owned()));
        splice_inline_tail(nodes, 0, 0, &append);
        delta.ops.push(Op::SpliceQuoteTail {
            block: block_index as u32,
            remove_nodes: 0,
            truncate_bytes: 0,
            append,
        });
        true
    }

    fn try_append_complete_quote_line(
        &mut self,
        block_index: usize,
        input: &str,
        delta: &mut Delta,
    ) -> bool {
        if !self.line.is_empty() {
            return false;
        }
        let Some(raw) = input.strip_suffix('\n') else {
            return false;
        };
        if raw.contains('\n') || classify(raw) != PendingKind::Quote {
            return false;
        }
        let body = quote_line_body(raw);

        // A new quote line can be appended locally only when the previous
        // source line truly contributes a SoftBreak. HardBreaks, empty lines,
        // or delimiter state that can cross the newline must fall back to a
        // full parse before we publish any new inline nodes.
        if !quote_previous_line_can_append_break(&self.pending) {
            return false;
        }

        // Keep an empty quote line pending but unpublished. Whole parsing of a
        // trailing empty quote line does not expose a SoftBreak until another
        // line arrives, so publishing one here would break chunk equivalence.
        if body.is_empty() {
            self.pending.push_str(raw);
            self.pending.push('\n');
            return true;
        }

        let append_nodes = if body.bytes().all(is_plain_stream_byte) {
            vec![Inline::Text(body.to_owned())]
        } else {
            if self.live_quote_ambiguous {
                return false;
            }
            let parsed = parse_inlines(body);
            if !quote_line_inlines_are_self_contained(&parsed) {
                return false;
            }
            parsed
        };

        let Some(Block::BlockQuote(nodes)) = self.blocks.get_mut(block_index) else {
            return false;
        };
        let mut append = Vec::with_capacity(1 + append_nodes.len());
        append.push(Inline::SoftBreak);
        append.extend(append_nodes);
        splice_inline_tail(nodes, 0, 0, &append);
        delta.ops.push(Op::SpliceQuoteTail {
            block: block_index as u32,
            remove_nodes: 0,
            truncate_bytes: 0,
            append,
        });
        self.pending.push_str(raw);
        self.pending.push('\n');
        true
    }

    fn try_append_heading_tail(&mut self, input: &str, delta: &mut Delta) -> bool {
        if input.is_empty()
            || !matches!(self.mode, Mode::Normal)
            || !self.has_live
            || self.pending_kind.is_some()
            || !input.bytes().all(is_plain_stream_byte)
        {
            return false;
        }
        let Some(block_index) = self.blocks.len().checked_sub(1) else {
            return false;
        };
        let Some(Block::Heading { .. }) = self.blocks.get(block_index) else {
            return false;
        };
        let Some((_, old_end)) = heading_content_range(&self.line) else {
            return false;
        };

        self.line.push_str(input);
        let Some((_, new_end)) = heading_content_range(&self.line) else {
            unreachable!("plain append cannot change an established heading prefix");
        };
        debug_assert!(new_end >= old_end);
        if new_end == old_end {
            return true;
        }

        let append = vec![Inline::Text(self.line[old_end..new_end].to_owned())];
        let Some(Block::Heading { content, .. }) = self.blocks.get_mut(block_index) else {
            unreachable!();
        };
        splice_inline_tail(content, 0, 0, &append);
        delta.ops.push(Op::SpliceHeadingTail {
            block: block_index as u32,
            remove_nodes: 0,
            truncate_bytes: 0,
            append,
        });
        true
    }

    fn try_append_list_tail(&mut self, input: &str, delta: &mut Delta) -> bool {
        if input.is_empty()
            || !matches!(self.mode, Mode::Normal)
            || !self.has_live
            || input.contains('\r')
        {
            return false;
        }

        let Some(block_index) = self.blocks.len().checked_sub(1) else {
            return false;
        };
        let kind = match self.blocks.get(block_index) {
            Some(Block::UnorderedList(_)) => PendingKind::Unordered,
            Some(Block::OrderedList { .. }) => PendingKind::Ordered,
            _ => return false,
        };
        if self.pending_kind.is_some_and(|pending| pending != kind) {
            return false;
        }

        if self.try_append_complete_list_item_chunk(block_index, kind, input, delta) {
            return true;
        }

        if input == "\n" {
            if self.line.is_empty() || !line_is_complete_list_item(kind, &self.line) {
                return false;
            }
            self.pending_kind = Some(kind);
            self.pending.push_str(&self.line);
            self.pending.push('\n');
            self.line.clear();
            self.live_plain = false;
            self.live_inline_appendable = false;
            self.trailing_backslash_odd = false;
            self.reset_delimiter_runs();
            self.live_tail_pending = None;
            return true;
        }
        if input.contains('\n') {
            return false;
        }

        match kind {
            PendingKind::Unordered => self.try_append_unordered_tail(block_index, input, delta),
            PendingKind::Ordered => self.try_append_ordered_tail(block_index, input, delta),
            _ => false,
        }
    }

    fn try_append_complete_list_item_chunk(
        &mut self,
        block_index: usize,
        kind: PendingKind,
        input: &str,
        delta: &mut Delta,
    ) -> bool {
        // Only start a whole-item fast path between completed list lines. A
        // partial current item remains owned by the existing tail machinery.
        if !self.line.is_empty() || self.pending_kind != Some(kind) {
            return false;
        }

        let (raw, sealed) = if let Some(raw) = input.strip_suffix('\n') {
            if raw.contains('\n') {
                return false;
            }
            (raw, true)
        } else {
            if input.contains('\n') {
                return false;
            }
            (input, false)
        };
        if raw.is_empty() || is_thematic(raw) {
            return false;
        }

        let trimmed = raw.trim_start();
        let item = match kind {
            PendingKind::Unordered
                if trimmed.starts_with("- ")
                    || trimmed.starts_with("* ")
                    || trimmed.starts_with("+ ") =>
            {
                parse_inlines(trimmed.get(2..).unwrap_or(""))
            }
            PendingKind::Ordered => {
                let Some((_, body)) = ordered_item(trimmed) else {
                    return false;
                };
                parse_inlines(body)
            }
            _ => return false,
        };

        match self.blocks.get_mut(block_index) {
            Some(Block::UnorderedList(items)) if kind == PendingKind::Unordered => {
                items.push(item.clone());
            }
            Some(Block::OrderedList { items, .. }) if kind == PendingKind::Ordered => {
                items.push(item.clone());
            }
            _ => return false,
        }
        delta.ops.push(Op::AppendListItem {
            block: block_index as u32,
            item,
        });

        if sealed {
            self.pending.push_str(raw);
            self.pending.push('\n');
            self.line.clear();
            self.live_plain = false;
            self.live_inline_appendable = false;
            self.trailing_backslash_odd = false;
            self.reset_delimiter_runs();
            self.live_tail_pending = None;
        } else {
            self.line.push_str(raw);
        }
        true
    }

    fn try_append_unordered_tail(
        &mut self,
        block_index: usize,
        input: &str,
        delta: &mut Delta,
    ) -> bool {
        if self.line.is_empty() && self.pending_kind == Some(PendingKind::Unordered) {
            if input.len() != 1 || !input.is_ascii() {
                return false;
            }
            let item = Vec::new();
            let Some(Block::UnorderedList(items)) = self.blocks.get_mut(block_index) else {
                return false;
            };
            items.push(item.clone());
            self.line.push_str(input);
            delta.ops.push(Op::AppendListItem {
                block: block_index as u32,
                item,
            });
            return true;
        }

        let trimmed = self.line.trim_start();
        if matches!(trimmed, "-" | "*" | "+") && input == " " {
            self.line.push(' ');
            return true;
        }
        if !(trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ")) {
            return false;
        }
        if !input.bytes().all(is_plain_stream_byte) {
            return self.replace_rich_list_item_tail(
                block_index,
                PendingKind::Unordered,
                input,
                delta,
            );
        }
        let Some(Block::UnorderedList(items)) = self.blocks.get_mut(block_index) else {
            return false;
        };
        splice_list_item_tail(items, 0, 0, &[Inline::Text(input.to_owned())]);
        self.line.push_str(input);
        delta.ops.push(Op::SpliceListItemTail {
            block: block_index as u32,
            remove_nodes: 0,
            truncate_bytes: 0,
            append: vec![Inline::Text(input.to_owned())],
        });
        true
    }

    fn try_append_ordered_tail(
        &mut self,
        block_index: usize,
        input: &str,
        delta: &mut Delta,
    ) -> bool {
        if self.line.is_empty() && self.pending_kind == Some(PendingKind::Ordered) {
            if input.len() != 1 || !input.as_bytes()[0].is_ascii_digit() {
                return false;
            }
            let item = vec![Inline::Text(input.to_owned())];
            let Some(Block::OrderedList { items, .. }) = self.blocks.get_mut(block_index) else {
                return false;
            };
            items.push(item.clone());
            self.line.push_str(input);
            delta.ops.push(Op::AppendListItem {
                block: block_index as u32,
                item,
            });
            return true;
        }

        let trimmed = self.line.trim_start();
        let digits = trimmed
            .as_bytes()
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digits != 0
            && digits == trimmed.len()
            && digits < 9
            && input.len() == 1
            && input.as_bytes()[0].is_ascii_digit()
        {
            return self.append_ordered_raw_tail(block_index, input, delta);
        }
        if digits != 0 && digits == trimmed.len() && input == "." {
            return self.append_ordered_raw_tail(block_index, input, delta);
        }
        if digits != 0
            && digits + 1 == trimmed.len()
            && trimmed.as_bytes()[digits] == b'.'
            && input == " "
        {
            let truncate_bytes = trimmed.len();
            let Some(Block::OrderedList { items, .. }) = self.blocks.get_mut(block_index) else {
                return false;
            };
            splice_list_item_tail(items, truncate_bytes, 0, &[]);
            self.line.push(' ');
            delta.ops.push(Op::SpliceListItemTail {
                block: block_index as u32,
                remove_nodes: 0,
                truncate_bytes: truncate_bytes as u32,
                append: Vec::new(),
            });
            return true;
        }
        if ordered_item(trimmed).is_none() {
            return false;
        }
        if !input.bytes().all(is_plain_stream_byte) {
            return self.replace_rich_list_item_tail(
                block_index,
                PendingKind::Ordered,
                input,
                delta,
            );
        }
        self.append_ordered_raw_tail(block_index, input, delta)
    }

    fn replace_rich_list_item_tail(
        &mut self,
        block_index: usize,
        kind: PendingKind,
        input: &str,
        delta: &mut Delta,
    ) -> bool {
        if input.is_empty() || input.contains('\n') || input.contains('\r') {
            return false;
        }

        let mut candidate = self.line.clone();
        candidate.push_str(input);
        let trimmed = candidate.trim_start();
        let body = match kind {
            PendingKind::Unordered
                if trimmed.starts_with("- ")
                    || trimmed.starts_with("* ")
                    || trimmed.starts_with("+ ") =>
            {
                trimmed.get(2..).unwrap_or("")
            }
            PendingKind::Ordered => {
                let Some((_, body)) = ordered_item(trimmed) else {
                    return false;
                };
                body
            }
            _ => return false,
        };
        let replacement = parse_inlines(body);

        let item = match self.blocks.get_mut(block_index) {
            Some(Block::UnorderedList(items)) if kind == PendingKind::Unordered => items.last_mut(),
            Some(Block::OrderedList { items, .. }) if kind == PendingKind::Ordered => {
                items.last_mut()
            }
            _ => return false,
        };
        let Some(item) = item else {
            return false;
        };
        let (truncate_bytes, remove_nodes) = match item.last() {
            Some(Inline::Text(text)) if !text.is_empty() => (text.len(), item.len() - 1),
            _ => (0, item.len()),
        };
        splice_inline_tail(item, truncate_bytes, remove_nodes, &replacement);
        self.line = candidate;
        delta.ops.push(Op::SpliceListItemTail {
            block: block_index as u32,
            remove_nodes: remove_nodes as u32,
            truncate_bytes: truncate_bytes as u32,
            append: replacement,
        });
        true
    }

    fn append_ordered_raw_tail(
        &mut self,
        block_index: usize,
        input: &str,
        delta: &mut Delta,
    ) -> bool {
        let Some(Block::OrderedList { items, .. }) = self.blocks.get_mut(block_index) else {
            return false;
        };
        splice_list_item_tail(items, 0, 0, &[Inline::Text(input.to_owned())]);
        self.line.push_str(input);
        delta.ops.push(Op::SpliceListItemTail {
            block: block_index as u32,
            remove_nodes: 0,
            truncate_bytes: 0,
            append: vec![Inline::Text(input.to_owned())],
        });
        true
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

    fn try_append_complete_semantic_chunk(&mut self, input: &str, delta: &mut Delta) -> bool {
        if input.is_empty()
            || !matches!(self.mode, Mode::Normal)
            || !self.has_live
            || !(self.live_plain || self.live_inline_appendable)
            || self.live_tail_pending.is_some()
            || self.trailing_backslash_odd
            || self.live_special_bracket != SpecialBracketKind::None
            || self.live_has_link_label_open
            || self.live_link_label_just_closed
            || self.live_has_link_destination_start
            || !self.trailing_backtick_fast
            || !self.trailing_dollar_fast
            || !self.trailing_star_fast
            || !self.trailing_underscore_fast
            || self.trailing_backtick_mod2 != 0
            || self.trailing_dollar_mod4 != 0
            || self.trailing_star_mod4 != 0
            || self.trailing_underscore_mod4 != 0
        {
            return false;
        }

        let Some(append) = complete_semantic_chunk_inlines(input) else {
            return false;
        };
        let Some(block_index) = self.blocks.len().checked_sub(1) else {
            return false;
        };
        let Some(Block::Paragraph(nodes)) = self.blocks.get_mut(block_index) else {
            return false;
        };

        splice_inline_tail(nodes, 0, 0, &append);
        self.line.push_str(input);
        self.live_plain = false;
        self.live_inline_appendable = true;
        self.live_multiline_plain_safe = false;
        self.live_pending_line_break = None;
        self.live_special_bracket = SpecialBracketKind::None;
        self.live_special_opener = 0;
        self.live_has_link_label_open = false;
        self.live_link_label_just_closed = false;
        self.live_has_link_destination_start = false;
        self.live_link_tail_pending = None;
        self.live_tail_pending = None;
        self.trailing_backslash_odd = false;
        self.reset_delimiter_runs();
        delta.ops.push(Op::SpliceInlineTail {
            block: block_index as u32,
            remove_nodes: 0,
            truncate_bytes: 0,
            append,
        });
        true
    }

    fn try_append_complete_link_chunk(&mut self, input: &str, delta: &mut Delta) -> bool {
        if input.is_empty()
            || !matches!(self.mode, Mode::Normal)
            || !self.has_live
            || !(self.live_plain || self.live_inline_appendable)
            || self.live_tail_pending.is_some()
            || self.trailing_backslash_odd
            || self.live_special_bracket != SpecialBracketKind::None
            || self.live_has_link_label_open
            || self.live_link_label_just_closed
            || self.live_has_link_destination_start
            || self.live_link_tail_pending.is_some()
            || self.live_link_fast_ambiguous
            || !self.trailing_backtick_fast
            || !self.trailing_dollar_fast
            || !self.trailing_star_fast
            || !self.trailing_underscore_fast
            || self.trailing_backtick_mod2 != 0
            || self.trailing_dollar_mod4 != 0
            || self.trailing_star_mod4 != 0
            || self.trailing_underscore_mod4 != 0
        {
            return false;
        }

        let Some(append) = complete_link_chunk_inlines(input) else {
            return false;
        };
        let Some(block_index) = self.blocks.len().checked_sub(1) else {
            return false;
        };
        let Some(Block::Paragraph(nodes)) = self.blocks.get_mut(block_index) else {
            return false;
        };

        splice_inline_tail(nodes, 0, 0, &append);
        self.line.push_str(input);
        self.live_plain = false;
        self.live_inline_appendable = true;
        self.live_multiline_plain_safe = false;
        self.live_pending_line_break = None;
        self.live_special_bracket = SpecialBracketKind::None;
        self.live_special_opener = 0;
        self.live_has_link_label_open = false;
        self.live_link_label_just_closed = false;
        self.live_has_link_destination_start = false;
        self.live_link_tail_pending = None;
        self.live_link_fast_ambiguous = false;
        self.live_tail_pending = None;
        self.trailing_backslash_odd = false;
        self.reset_delimiter_runs();
        delta.ops.push(Op::SpliceInlineTail {
            block: block_index as u32,
            remove_nodes: 0,
            truncate_bytes: 0,
            append,
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
        let outer_link_destination = match self.live_link_tail_pending {
            Some(state @ LinkTailPending::Destination { .. }) => Some(state),
            _ => None,
        };
        if !input.bytes().all(is_plain_stream_byte) {
            self.live_multiline_plain_safe = false;
            self.live_pending_line_break = None;
        }

        if let Some(mut pending) = self.live_tail_pending
            && input.len() == 1
            && input.as_bytes()[0] == pending.kind.delimiter()
            && !self.trailing_backslash_odd
            && pending.opener < self.line.len()
            && self.line.as_bytes()[pending.opener] == pending.kind.delimiter()
        {
            if pending.kind == TailPendingKind::DisplayMath && pending.close_seen == 0 {
                let body = self.line[pending.opener + 2..].to_owned();
                let append = vec![
                    Inline::Text("$".to_owned()),
                    Inline::Math {
                        source: body,
                        display: false,
                    },
                ];
                let truncate_bytes = self.line.len() - pending.opener;
                splice_inline_tail(nodes, truncate_bytes, 0, &append);
                self.line.push('$');
                pending.close_seen = 1;
                self.live_tail_pending = Some(pending);
                self.reset_delimiter_runs();
                self.trailing_backslash_odd = false;
                delta.ops.push(Op::SpliceInlineTail {
                    block: block_index as u32,
                    remove_nodes: 0,
                    truncate_bytes: truncate_bytes as u32,
                    append,
                });
                return true;
            }

            if pending.kind == TailPendingKind::DisplayMath && pending.close_seen == 1 {
                let body_start = pending.opener + 2;
                let body_end = self.line.len() - 1;
                let body = self.line[body_start..body_end].to_owned();
                splice_inline_tail(nodes, 0, 1, &[]);
                delta.ops.push(Op::SpliceInlineTail {
                    block: block_index as u32,
                    remove_nodes: 1,
                    truncate_bytes: 0,
                    append: Vec::new(),
                });
                let append = vec![Inline::Math {
                    source: body,
                    display: true,
                }];
                splice_inline_tail(nodes, 1, 0, &append);
                delta.ops.push(Op::SpliceInlineTail {
                    block: block_index as u32,
                    remove_nodes: 0,
                    truncate_bytes: 1,
                    append,
                });
                self.line.push('$');
                self.live_tail_pending = None;
                self.reset_delimiter_runs();
                self.trailing_backslash_odd = false;
                return true;
            }

            if matches!(
                pending.kind,
                TailPendingKind::StrongStar | TailPendingKind::StrongUnderscore
            ) && pending.close_seen == 0
            {
                append_inline_text(nodes, input);
                self.line.push_str(input);
                pending.close_seen = 1;
                self.live_tail_pending = Some(pending);
                self.reset_delimiter_runs();
                self.trailing_backslash_odd = false;
                delta.ops.push(Op::AppendInlineText {
                    block: block_index as u32,
                    append: input.to_owned(),
                });
                return true;
            }

            if matches!(
                pending.kind,
                TailPendingKind::StrongStar | TailPendingKind::StrongUnderscore
            ) && pending.close_seen == 1
            {
                let body_start = pending.opener + 2;
                let body_end = self.line.len() - 1;
                let body = self.line[body_start..body_end].to_owned();
                let append = vec![Inline::Strong(parse_inlines(&body))];
                splice_inline_tail(nodes, 1, 2, &append);
                self.line.push_str(input);
                self.live_tail_pending = None;
                self.reset_delimiter_runs();
                self.trailing_backslash_odd = false;
                delta.ops.push(Op::SpliceInlineTail {
                    block: block_index as u32,
                    remove_nodes: 2,
                    truncate_bytes: 1,
                    append,
                });
                return true;
            }

            let body = self.line[pending.opener + 1..].to_owned();
            let append = vec![match pending.kind {
                TailPendingKind::Code => Inline::Code(body),
                TailPendingKind::Math => Inline::Math {
                    source: body,
                    display: false,
                },
                TailPendingKind::EmphasisStar | TailPendingKind::EmphasisUnderscore => {
                    Inline::Emphasis(parse_inlines(&body))
                }
                TailPendingKind::StrongStar
                | TailPendingKind::StrongUnderscore
                | TailPendingKind::DisplayMath => unreachable!(),
            }];
            let truncate_bytes = self.line.len() - pending.opener;
            splice_inline_tail(nodes, truncate_bytes, 0, &append);
            self.line.push_str(input);
            self.live_tail_pending = None;
            self.reset_delimiter_runs();
            self.trailing_backslash_odd = false;
            delta.ops.push(Op::SpliceInlineTail {
                block: block_index as u32,
                remove_nodes: 0,
                truncate_bytes: truncate_bytes as u32,
                append,
            });
            return true;
        }

        let closes_tracked_link = input == ")"
            && matches!(
                self.live_link_tail_pending,
                Some(LinkTailPending::Destination { .. })
            )
            && !self.live_link_fast_ambiguous;
        if self.live_tail_pending.is_some()
            && !input.bytes().all(is_plain_stream_byte)
            && !closes_tracked_link
        {
            return false;
        }

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

        if input == "]"
            && self.live_special_bracket != SpecialBracketKind::None
            && !self.trailing_backslash_odd
        {
            let replacement = match self.live_special_bracket {
                SpecialBracketKind::Reference => {
                    llm_reference_tail_link(&self.line, self.live_special_opener)
                }
                SpecialBracketKind::Citation => {
                    llm_citation_tail_link(&self.line, self.live_special_opener)
                }
                SpecialBracketKind::None => None,
            };
            if let Some(link) = replacement {
                let truncate_bytes = self.line.len() - self.live_special_opener;
                let append = vec![link];
                splice_inline_tail(nodes, truncate_bytes, 0, &append);
                self.line.push(']');
                self.live_special_bracket = SpecialBracketKind::None;
                self.live_special_opener = 0;
                self.live_has_link_label_open = false;
                self.live_link_label_just_closed = false;
                if let Some(destination) = outer_link_destination {
                    self.live_has_link_destination_start = true;
                    self.live_link_tail_pending = Some(destination);
                } else {
                    self.live_has_link_destination_start = false;
                    self.live_link_tail_pending = None;
                }
                self.live_tail_pending = None;
                self.reset_delimiter_runs();
                self.trailing_backslash_odd = false;
                delta.ops.push(Op::SpliceInlineTail {
                    block: block_index as u32,
                    remove_nodes: 0,
                    truncate_bytes: truncate_bytes as u32,
                    append,
                });
                return true;
            }
        }

        if input == ")"
            && !self.trailing_backslash_odd
            && !self.live_link_fast_ambiguous
            && let Some(LinkTailPending::Destination {
                opener,
                label_close,
                destination_open,
                opener_node,
                opener_text_prefix,
            }) = self.live_link_tail_pending
        {
            let label = &self.line[opener + 1..label_close];
            let destination = &self.line[destination_open + 1..];
            if opener_node < nodes.len()
                && matches!(nodes.get(opener_node), Some(Inline::Text(text)) if text.len() >= opener_text_prefix)
            {
                let link = Inline::Link {
                    label: parse_inlines(label),
                    destination: destination.to_owned(),
                };

                // Formatted labels can turn the raw label into several AST nodes
                // while the opening `[` stays inside an older Text node. Remove
                // everything after that opener node first, then trim only the
                // opener-owned Text suffix and append the completed Link. This
                // keeps the already-rendered paragraph prefix out of the delta.
                let nodes_after_opener = nodes.len() - opener_node - 1;
                if nodes_after_opener != 0 {
                    let (tail_truncate, remove_nodes) = match nodes.last() {
                        Some(Inline::Text(tail)) if !tail.is_empty() => {
                            (tail.len(), nodes_after_opener - 1)
                        }
                        _ => (0, nodes_after_opener),
                    };
                    splice_inline_tail(nodes, tail_truncate, remove_nodes, &[]);
                    delta.ops.push(Op::SpliceInlineTail {
                        block: block_index as u32,
                        remove_nodes: remove_nodes as u32,
                        truncate_bytes: tail_truncate as u32,
                        append: Vec::new(),
                    });
                }

                let Some(Inline::Text(opener_text)) = nodes.last() else {
                    return false;
                };
                if nodes.len() != opener_node + 1 || opener_text.len() < opener_text_prefix {
                    return false;
                }
                let opener_truncate = opener_text.len() - opener_text_prefix;
                if opener_truncate == 0 {
                    return false;
                }
                let append = vec![link];
                splice_inline_tail(nodes, opener_truncate, 0, &append);
                delta.ops.push(Op::SpliceInlineTail {
                    block: block_index as u32,
                    remove_nodes: 0,
                    truncate_bytes: opener_truncate as u32,
                    append,
                });

                self.line.push(')');
                self.live_link_tail_pending = None;
                self.live_link_fast_ambiguous = false;
                self.live_has_link_label_open = false;
                self.live_link_label_just_closed = false;
                self.live_has_link_destination_start = false;
                self.live_special_bracket = SpecialBracketKind::None;
                self.live_special_opener = 0;
                self.live_tail_pending = None;
                self.reset_delimiter_runs();
                self.trailing_backslash_odd = false;
                return true;
            }
        }

        // A closing bracket/paren is inert when the live paragraph has no raw
        // opener that it could possibly complete. Keep these common malformed
        // streaming runs on the append-only path without weakening correctness
        // for real references/links.
        let inert_closer = if self.trailing_backslash_odd {
            false
        } else if input.bytes().all(|byte| byte == b']') {
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
            if input == "]" && outer_link_destination.is_none() {
                self.live_link_tail_pending = match self.live_link_tail_pending {
                    Some(LinkTailPending::Label {
                        opener,
                        opener_node,
                        opener_text_prefix,
                    }) if !self.live_link_fast_ambiguous => Some(LinkTailPending::Closed {
                        opener,
                        label_close: self.line.len(),
                        opener_node,
                        opener_text_prefix,
                    }),
                    Some(_) => {
                        self.live_link_fast_ambiguous = true;
                        None
                    }
                    None => None,
                };
            }
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
                if self.live_special_bracket == SpecialBracketKind::None {
                    self.live_special_opener = 0;
                }
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
        let body_is_plain = input.bytes().all(is_plain_stream_byte);
        if self.live_tail_pending.is_some() && !body_is_plain {
            return false;
        }
        if body_is_plain
            && self
                .live_tail_pending
                .is_some_and(|pending| pending.close_seen != 0)
        {
            self.live_tail_pending = None;
        }
        let new_tail_pending = if self.live_tail_pending.is_none() && body_is_plain {
            if self.trailing_backtick_fast && self.trailing_backtick_mod2 == 1 {
                Some(TailPending {
                    kind: TailPendingKind::Code,
                    opener: self.line.len() - 1,
                    close_seen: 0,
                })
            } else if self.trailing_dollar_fast && self.trailing_dollar_mod4 == 2 {
                Some(TailPending {
                    kind: TailPendingKind::DisplayMath,
                    opener: self.line.len() - 2,
                    close_seen: 0,
                })
            } else if self.trailing_dollar_fast && self.trailing_dollar_mod4 == 1 {
                Some(TailPending {
                    kind: TailPendingKind::Math,
                    opener: self.line.len() - 1,
                    close_seen: 0,
                })
            } else if self.trailing_star_fast && self.trailing_star_mod4 == 2 {
                Some(TailPending {
                    kind: TailPendingKind::StrongStar,
                    opener: self.line.len() - 2,
                    close_seen: 0,
                })
            } else if self.trailing_underscore_fast && self.trailing_underscore_mod4 == 2 {
                Some(TailPending {
                    kind: TailPendingKind::StrongUnderscore,
                    opener: self.line.len() - 2,
                    close_seen: 0,
                })
            } else if self.trailing_star_fast && self.trailing_star_mod4 == 1 {
                Some(TailPending {
                    kind: TailPendingKind::EmphasisStar,
                    opener: self.line.len() - 1,
                    close_seen: 0,
                })
            } else if self.trailing_underscore_fast && self.trailing_underscore_mod4 == 1 {
                Some(TailPending {
                    kind: TailPendingKind::EmphasisUnderscore,
                    opener: self.line.len() - 1,
                    close_seen: 0,
                })
            } else {
                None
            }
        } else {
            None
        };
        let cross_link_destination = self.live_link_label_just_closed && input.starts_with('(');
        if let Some((kind, opener)) = special_bracket_opener(&self.line, input) {
            self.live_special_bracket = kind;
            self.live_special_opener = opener;
            if outer_link_destination.is_none() {
                self.live_link_tail_pending = None;
                self.live_link_fast_ambiguous = true;
            }
        }

        if input == "[" && outer_link_destination.is_none() {
            if !self.live_has_link_label_open
                && !self.live_link_fast_ambiguous
                && self.live_special_bracket == SpecialBracketKind::None
            {
                let (opener_node, opener_text_prefix) = match nodes.last() {
                    Some(Inline::Text(text)) => (nodes.len() - 1, text.len()),
                    _ => (nodes.len(), 0),
                };
                self.live_link_tail_pending = Some(LinkTailPending::Label {
                    opener: self.line.len(),
                    opener_node,
                    opener_text_prefix,
                });
            } else {
                self.live_link_tail_pending = None;
                self.live_link_fast_ambiguous = true;
            }
        } else if input == "(" && outer_link_destination.is_none() {
            self.live_link_tail_pending = match self.live_link_tail_pending {
                Some(LinkTailPending::Closed {
                    opener,
                    label_close,
                    opener_node,
                    opener_text_prefix,
                }) if self.live_link_label_just_closed && !self.live_link_fast_ambiguous => {
                    Some(LinkTailPending::Destination {
                        opener,
                        label_close,
                        destination_open: self.line.len(),
                        opener_node,
                        opener_text_prefix,
                    })
                }
                Some(_) => None,
                None => None,
            };
        } else if matches!(
            self.live_link_tail_pending,
            Some(LinkTailPending::Closed { .. })
        ) {
            // A standalone `[label]` is still a candidate opener for a later
            // `](` in this parser's link grammar. Once it is not followed
            // immediately by `(`, stop using the local link fast path for the
            // rest of the live line rather than forgetting that old opener.
            self.live_link_tail_pending = None;
            self.live_link_fast_ambiguous = true;
        }

        if outer_link_destination.is_none() {
            self.live_has_link_label_open |= input.as_bytes().contains(&b'[');
        }
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
        if let Some(pending) = new_tail_pending {
            self.reset_delimiter_runs();
            self.live_tail_pending = Some(pending);
        } else if self.live_tail_pending.is_some() {
            self.reset_delimiter_runs();
        } else {
            self.break_delimiter_runs();
        }
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
            self.live_quote_ambiguous = false;
            self.live_table_row = false;
            self.live_table_cell_safe = false;
            self.trailing_backslash_odd = false;
            self.reset_delimiter_runs();
            self.live_special_bracket = SpecialBracketKind::None;
            self.live_special_opener = 0;
            self.live_has_link_label_open = false;
            self.live_link_label_just_closed = false;
            self.live_has_link_destination_start = false;
            self.live_thematic_marker = 0;
            self.live_tail_pending = None;
            self.live_multiline_plain_safe = false;
            self.live_pending_line_break = None;
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
        // A pending list ends once the unfinished next line can no longer
        // become another item of that same list kind. Marker prefixes such as
        // `3.` remain live until the following space arrives.
        if let Some(pending_kind @ (PendingKind::Unordered | PendingKind::Ordered)) =
            self.pending_kind
            && !self.line.is_empty()
            && !line_can_continue_list_kind(pending_kind, &self.line)
        {
            self.finish_pending(delta);
        }

        let mut source = self.pending.clone();
        source.push_str(&self.line);
        if source.is_empty() {
            self.live_quote_ambiguous = false;
            self.live_table_row = false;
            self.live_table_cell_safe = false;
            self.trailing_backslash_odd = false;
            self.reset_delimiter_runs();
            self.live_special_bracket = SpecialBracketKind::None;
            self.live_special_opener = 0;
            self.live_has_link_label_open = false;
            self.live_link_label_just_closed = false;
            self.live_has_link_destination_start = false;
            self.live_thematic_marker = 0;
            self.live_tail_pending = None;
            self.live_multiline_plain_safe = false;
            self.live_pending_line_break = None;
            return;
        }
        if self.pending_kind.is_none() && is_thematic(&self.line) {
            self.live_plain = false;
            self.live_quote_ambiguous = false;
            self.live_table_row = false;
            self.live_table_cell_safe = false;
            self.trailing_backslash_odd = false;
            self.reset_delimiter_runs();
            self.live_special_bracket = SpecialBracketKind::None;
            self.live_special_opener = 0;
            self.live_has_link_label_open = false;
            self.live_link_label_just_closed = false;
            self.live_has_link_destination_start = false;
            self.live_inline_appendable = false;
            self.live_thematic_marker = thematic_marker(&self.line);
            self.live_tail_pending = None;
            self.live_multiline_plain_safe = false;
            self.live_pending_line_break = None;
            self.push(Block::ThematicBreak, delta);
            self.has_live = true;
            return;
        }
        let kind = self.pending_kind.unwrap_or_else(|| classify(&self.line));
        let normalized_source = source.trim_end_matches('\n');
        let block = block_from_complete(normalized_source, kind);
        self.live_quote_ambiguous = kind == PendingKind::Quote
            && !quote_source_boundary_is_self_contained(normalized_source);
        self.live_table_row = matches!(block, Block::Table { .. })
            && !self.line.is_empty()
            && !self.line.trim_end().ends_with('|')
            && split_table_row(&self.line).is_some();
        self.live_table_cell_safe = self.live_table_row
            && match &block {
                Block::Table { rows, .. } => rows
                    .last()
                    .and_then(|row| row.last())
                    .is_some_and(|cell| quote_line_inlines_are_self_contained(cell)),
                _ => false,
            };
        let literal_link_openers = paragraph_literal_link_opener_count(&block);
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
        let (special_kind, special_opener) = special_bracket_state_from_source(&source);
        self.live_special_bracket = special_kind;
        self.live_special_opener = special_opener;
        let (link_label_open, link_label_just_closed) = link_label_state_from_source(&source);
        self.live_has_link_label_open = link_label_open;
        self.live_link_label_just_closed = link_label_just_closed;
        self.live_has_link_destination_start = closing_paren_sensitive(&source);
        self.live_link_tail_pending = simple_link_label_tail_from_source(&block, &source);
        let tracked_tail_openers = usize::from(matches!(
            self.live_link_tail_pending,
            Some(LinkTailPending::Label { .. })
        ));
        self.live_link_fast_ambiguous = literal_link_openers > tracked_tail_openers
            || (link_label_open && self.live_link_tail_pending.is_none())
            || link_label_just_closed;
        self.live_thematic_marker = 0;
        self.live_tail_pending = None;
        self.live_multiline_plain_safe = stable_multiline_paragraph
            && source
                .bytes()
                .all(|byte| byte == b'\n' || is_plain_stream_byte(byte));
        self.live_pending_line_break = None;
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

fn splice_list_item_tail(
    items: &mut [Vec<Inline>],
    truncate_bytes: usize,
    remove_nodes: usize,
    append: &[Inline],
) {
    let Some(item) = items.last_mut() else {
        unreachable!("list tail splice requires a final item")
    };
    splice_inline_tail(item, truncate_bytes, remove_nodes, append);
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

fn special_bracket_opener(line: &str, input: &str) -> Option<(SpecialBracketKind, usize)> {
    let line_bytes = line.as_bytes();
    let input_bytes = input.as_bytes();
    let reference = crossing_marker_start(line_bytes, input_bytes, b"@[")
        .map(|position| (position, SpecialBracketKind::Reference));
    let citation = crossing_marker_start(line_bytes, input_bytes, b"[[cite:")
        .map(|position| (position, SpecialBracketKind::Citation));
    reference
        .into_iter()
        .chain(citation)
        .max_by_key(|(position, _)| *position)
        .map(|(position, kind)| {
            let opener = if position < 0 {
                line.len() - (-position) as usize
            } else {
                line.len() + position as usize
            };
            (kind, opener)
        })
}

fn complete_link_chunk_inlines(input: &str) -> Option<Vec<Inline>> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut plain_start = 0;
    let mut i = 0;
    let mut found_link = false;

    while i < bytes.len() {
        if is_plain_stream_byte(bytes[i]) {
            i += 1;
            continue;
        }
        if bytes[i] != b'[' {
            return None;
        }

        if plain_start < i {
            out.push(Inline::Text(input[plain_start..i].to_owned()));
        }
        let label_start = i + 1;
        let mut label_close = label_start;
        while label_close < bytes.len() && is_plain_stream_byte(bytes[label_close]) {
            label_close += 1;
        }
        if bytes.get(label_close) != Some(&b']') || bytes.get(label_close + 1) != Some(&b'(') {
            return None;
        }

        let destination_start = label_close + 2;
        let mut destination_close = destination_start;
        while destination_close < bytes.len() && is_plain_stream_byte(bytes[destination_close]) {
            destination_close += 1;
        }
        if bytes.get(destination_close) != Some(&b')') {
            return None;
        }

        let label = &input[label_start..label_close];
        let destination = &input[destination_start..destination_close];
        out.push(Inline::Link {
            label: parse_inlines(label),
            destination: destination.to_owned(),
        });
        found_link = true;
        i = destination_close + 1;
        plain_start = i;
    }

    if plain_start < input.len() {
        out.push(Inline::Text(input[plain_start..].to_owned()));
    }
    found_link.then_some(out)
}

fn complete_semantic_chunk_inlines(input: &str) -> Option<Vec<Inline>> {
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut found_semantic = false;

    while i < bytes.len() {
        if is_plain_stream_byte(bytes[i]) {
            i += 1;
            continue;
        }
        if input[i..].starts_with("@[") {
            i = complete_llm_reference_end(input, i)?;
            found_semantic = true;
            continue;
        }
        if input[i..].starts_with("[[cite:") {
            i = complete_llm_citation_end(input, i)?;
            found_semantic = true;
            continue;
        }
        return None;
    }

    found_semantic.then(|| parse_inlines(input))
}

fn complete_llm_reference_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = start + 2;
    let kind_start = i;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'-')) {
        i += 1;
    }
    if i == kind_start || bytes.get(i) != Some(&b':') {
        return None;
    }
    i += 1;
    let id_start = i;
    while i < bytes.len() {
        match bytes[i] {
            b']' if i != id_start => return Some(i + 1),
            b'[' | b'|' => return None,
            byte if byte.is_ascii_control() || byte.is_ascii_whitespace() => return None,
            _ => i += 1,
        }
    }
    None
}

fn complete_llm_citation_end(source: &str, start: usize) -> Option<usize> {
    const PREFIX: &str = "[[cite:";
    let bytes = source.as_bytes();
    let mut i = start + PREFIX.len();
    let source_start = i;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => return None,
            b']' if bytes.get(i + 1) == Some(&b']') => {
                return citation_source_is_valid(source[source_start..i].trim()).then_some(i + 2);
            }
            b']' => return None,
            b'|' => {
                if !citation_source_is_valid(source[source_start..i].trim()) {
                    return None;
                }
                let close = source[i + 1..].find("]]")? + i + 1;
                return Some(close + 2);
            }
            byte if byte.is_ascii_control() => return None,
            _ => i += 1,
        }
    }
    None
}

fn paragraph_literal_link_opener_count(block: &Block) -> usize {
    fn inlines(nodes: &[Inline]) -> usize {
        nodes
            .iter()
            .map(|node| match node {
                Inline::Text(text) => text.bytes().filter(|&byte| byte == b'[').count(),
                Inline::Emphasis(children) | Inline::Strong(children) => inlines(children),
                // These nodes consume their source delimiters as complete syntax.
                // Their visible text must not be mistaken for a raw outer-link opener.
                Inline::Link { .. }
                | Inline::Code(_)
                | Inline::Math { .. }
                | Inline::SoftBreak
                | Inline::HardBreak => 0,
            })
            .sum()
    }

    match block {
        Block::Paragraph(nodes) => inlines(nodes),
        _ => 0,
    }
}

fn simple_link_label_tail_from_source(block: &Block, source: &str) -> Option<LinkTailPending> {
    let opener = source.len().checked_sub(1)?;
    if source.as_bytes().get(opener) != Some(&b'[') {
        return None;
    }
    let escaped = source.as_bytes()[..opener]
        .iter()
        .rev()
        .take_while(|&&byte| byte == b'\\')
        .count()
        % 2
        != 0;
    if escaped || special_bracket_state_from_source(source).0 != SpecialBracketKind::None {
        return None;
    }
    let prefix = &source[..opener];
    let (open, just_closed) = link_label_state_from_source(prefix);
    if open || just_closed {
        return None;
    }
    let Block::Paragraph(nodes) = block else {
        return None;
    };
    let opener_node = nodes.len().checked_sub(1)?;
    let Inline::Text(text) = &nodes[opener_node] else {
        return None;
    };
    let opener_text_prefix = text.len().checked_sub(1)?;
    text.ends_with('[').then_some(LinkTailPending::Label {
        opener,
        opener_node,
        opener_text_prefix,
    })
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

fn llm_reference_tail_link(source: &str, opener: usize) -> Option<Inline> {
    let tail = source.get(opener + 2..)?;
    if tail.contains(']') {
        return None;
    }
    let bytes = tail.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'-')) {
        i += 1;
    }
    if i == 0 || bytes.get(i) != Some(&b':') {
        return None;
    }
    let kind = &tail[..i];
    let id = &tail[i + 1..];
    if !valid_reference_atom_streaming(id) {
        return None;
    }
    let label = format!("@[{kind}:{id}]");
    Some(Inline::Link {
        label: vec![Inline::Text(label)],
        destination: format!("llm:{kind}:{id}"),
    })
}

fn llm_citation_tail_link(source: &str, opener: usize) -> Option<Inline> {
    const PREFIX: &str = "[[cite:";
    let tail = source.get(opener + PREFIX.len()..)?;
    if tail.contains("]]") || !tail.ends_with(']') {
        return None;
    }
    let body = &tail[..tail.len() - 1];
    let (citation_source, label) = if let Some(pipe) = body.find('|') {
        let citation_source = body[..pipe].trim();
        if !citation_source_is_valid(citation_source) {
            return None;
        }
        let label = body[pipe + 1..].trim();
        (citation_source, (!label.is_empty()).then_some(label))
    } else {
        let citation_source = body.trim();
        if !citation_source_is_valid(citation_source) {
            return None;
        }
        (citation_source, None)
    };
    let visible = label.unwrap_or(citation_source);
    Some(Inline::Link {
        label: vec![Inline::Text(visible.to_owned())],
        destination: format!("llm:cite:{citation_source}"),
    })
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

fn special_bracket_state_from_source(source: &str) -> (SpecialBracketKind, usize) {
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
        .map_or((SpecialBracketKind::None, 0), |(open, kind)| (kind, open))
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

enum TableTailTransition {
    None,
    AppendRow(Vec<Vec<Inline>>),
    AppendCell(Vec<Inline>),
    AppendCellText(String),
}

fn pending_has_two_complete_lines(pending: &str) -> bool {
    let Some(first) = pending.find('\n') else {
        return false;
    };
    pending[first + 1..].contains('\n')
}

fn table_stream_row_is_plain(row: &[String]) -> bool {
    row.iter()
        .all(|cell| cell.bytes().all(is_plain_stream_byte))
}

fn plain_cell_inlines(cell: &str) -> Vec<Inline> {
    if cell.is_empty() {
        Vec::new()
    } else {
        vec![Inline::Text(cell.to_owned())]
    }
}

fn quote_line_body(line: &str) -> &str {
    let trimmed = line.trim_start();
    let body = trimmed.strip_prefix('>').unwrap_or(trimmed);
    body.trim_start()
}

fn quote_previous_line_can_append_break(pending: &str) -> bool {
    let source = pending.strip_suffix('\n').unwrap_or(pending);
    let previous = source.rsplit('\n').next().unwrap_or("");
    let body = quote_line_body(previous);
    if body.is_empty() {
        return false;
    }
    let trailing_spaces = body
        .as_bytes()
        .iter()
        .rev()
        .take_while(|&&byte| byte == b' ')
        .count();
    if trailing_spaces >= 2 {
        return false;
    }
    if body.bytes().all(is_plain_stream_byte) {
        return true;
    }
    let parsed = parse_inlines(body);
    quote_line_inlines_are_self_contained(&parsed)
}

fn quote_source_boundary_is_self_contained(source: &str) -> bool {
    let mut body = String::with_capacity(source.len());
    for (index, line) in source.lines().enumerate() {
        if index != 0 {
            body.push('\n');
        }
        body.push_str(quote_line_body(line));
    }
    let parsed = parse_inlines(&body);
    quote_line_inlines_are_self_contained(&parsed)
}

fn quote_line_inlines_are_self_contained(nodes: &[Inline]) -> bool {
    nodes.iter().all(|node| match node {
        Inline::Text(text) => text.bytes().all(is_plain_stream_byte),
        // Empty emphasis/strong nodes are the parser's representation of raw
        // delimiter runs such as `**open` / `close**`; those can pair across a
        // later quote-line boundary and must remain ambiguous.
        Inline::Emphasis(children) | Inline::Strong(children) => {
            !children.is_empty() && quote_line_inlines_are_self_contained(children)
        }
        Inline::Link { .. } | Inline::Code(_) | Inline::Math { .. } => true,
        Inline::SoftBreak | Inline::HardBreak => true,
    })
}

fn line_can_continue_list_kind(kind: PendingKind, line: &str) -> bool {
    let trimmed = line.trim_start();
    match kind {
        PendingKind::Unordered => {
            matches!(trimmed, "-" | "*" | "+")
                || trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("+ ")
        }
        PendingKind::Ordered => {
            let digits = trimmed
                .as_bytes()
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            (digits != 0 && digits <= 9 && digits == trimmed.len())
                || (digits != 0
                    && digits <= 9
                    && digits + 1 == trimmed.len()
                    && trimmed.as_bytes()[digits] == b'.')
                || ordered_item(trimmed).is_some()
        }
        _ => false,
    }
}

fn line_is_complete_list_item(kind: PendingKind, line: &str) -> bool {
    if is_thematic(line) {
        return false;
    }
    let trimmed = line.trim_start();
    match kind {
        PendingKind::Unordered => {
            trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ")
        }
        PendingKind::Ordered => ordered_item(trimmed).is_some(),
        _ => false,
    }
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
    let (start, end) = heading_content_range(line)?;
    let t = line.trim_start();
    let count = t.as_bytes().iter().take_while(|&&b| b == b'#').count();
    Some((count as u8, &line[start..end]))
}

fn heading_content_range(line: &str) -> Option<(usize, usize)> {
    let t = line.trim_start();
    let leading = line.len() - t.len();
    let count = t.as_bytes().iter().take_while(|&&b| b == b'#').count();
    if !(1..=6).contains(&count) || t.as_bytes().get(count) != Some(&b' ') {
        return None;
    }
    let start = leading + count + 1;
    let visible = line[start..].trim_end_matches('#').trim_end();
    Some((start, start + visible.len()))
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

fn thematic_marker(line: &str) -> u8 {
    line.bytes()
        .find(|byte| !byte.is_ascii_whitespace())
        .filter(|byte| matches!(byte, b'-' | b'*' | b'_'))
        .unwrap_or(0)
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

        // A complete semantic token can stay local to the paragraph tail.
        let delta = parser.append("[[cite:doc-1]]");
        assert!(matches!(
            delta.ops.as_slice(),
            [Op::SpliceInlineTail { truncate_bytes: 0, append, .. }]
                if matches!(append.as_slice(), [Inline::Link { destination, .. }]
                    if destination == "llm:cite:doc-1")
        ));
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
    fn live_thematic_break_extends_without_reparse() {
        let mut parser = Parser::new();
        parser.append("---");
        let delta = parser.append("----------------");
        assert!(delta.ops.is_empty());
        assert!(matches!(parser.blocks(), [Block::ThematicBreak]));

        let mut whole = Parser::new();
        whole.append("-------------------");
        assert_eq!(parser.blocks(), whole.blocks());

        let mixed = parser.append("_");
        assert!(matches!(mixed.ops.first(), Some(Op::Truncate { from: 0 })));
        let mut whole = Parser::new();
        whole.append("-------------------_");
        assert_eq!(parser.blocks(), whole.blocks());
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
        assert!(matches!(
            close.ops.as_slice(),
            [Op::SpliceInlineTail { append, .. }] if append == &vec![Inline::Code("body".to_owned())]
        ));
        let mut whole_code = Parser::new();
        whole_code.append("`body`");
        assert_eq!(normal_code.blocks(), whole_code.blocks());

        let mut normal_math = Parser::new();
        normal_math.append("$");
        normal_math.append("x+1");
        let close = normal_math.append("$");
        assert!(matches!(
            close.ops.as_slice(),
            [Op::SpliceInlineTail { append, .. }]
                if append == &vec![Inline::Math { source: "x+1".to_owned(), display: false }]
        ));
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
            assert!(matches!(
                close.ops.as_slice(),
                [Op::SpliceInlineTail { append, .. }]
                    if append == &vec![Inline::Emphasis(vec![Inline::Text("body".to_owned())])]
            ));
            let mut whole = Parser::new();
            whole.append(&format!("prefix {delimiter}body{delimiter}"));
            assert_eq!(normal.blocks(), whole.blocks());
        }
    }

    #[test]
    fn alternating_plain_delimiter_bodies_match_whole_parse() {
        for delimiter in ['`', '$', '*', '_'] {
            let mut parser = Parser::new();
            parser.append("prefix ");
            let token = delimiter.to_string();
            for _ in 0..64 {
                parser.append(&token);
                parser.append("x");
            }
            let source = format!("prefix {}", format!("{delimiter}x").repeat(64));
            let mut whole = Parser::new();
            whole.append(&source);
            assert_eq!(parser.blocks(), whole.blocks(), "delimiter={delimiter:?}");
        }
    }

    #[test]
    fn display_math_plain_body_splices_in_two_closes() {
        let mut parser = Parser::new();
        parser.append("prefix $");
        parser.append("$");
        parser.append("x+1");
        let first = parser.append("$");
        assert!(matches!(
            first.ops.as_slice(),
            [Op::SpliceInlineTail { append, .. }]
                if matches!(append.as_slice(), [Inline::Text(text), Inline::Math { source, display: false }]
                    if text == "$" && source == "x+1")
        ));
        let second = parser.append("$");
        assert!(matches!(
            second.ops.as_slice(),
            [
                Op::SpliceInlineTail { remove_nodes: 1, truncate_bytes: 0, .. },
                Op::SpliceInlineTail { remove_nodes: 0, truncate_bytes: 1, append, .. }
            ] if matches!(append.as_slice(), [Inline::Math { source, display: true }] if source == "x+1")
        ));
        let mut whole = Parser::new();
        whole.append("prefix $$x+1$$");
        assert_eq!(parser.blocks(), whole.blocks());
    }

    #[test]
    fn strong_plain_bodies_splice_on_second_close() {
        for delimiter in ['*', '_'] {
            let d = delimiter.to_string();
            let mut parser = Parser::new();
            parser.append("prefix ");
            parser.append(&d);
            parser.append(&d);
            parser.append("body");
            let first = parser.append(&d);
            assert!(matches!(
                first.ops.as_slice(),
                [Op::AppendInlineText { .. }]
            ));
            let second = parser.append(&d);
            assert!(matches!(
                second.ops.as_slice(),
                [Op::SpliceInlineTail { remove_nodes: 2, truncate_bytes: 1, append, .. }]
                    if matches!(append.as_slice(), [Inline::Strong(children)]
                        if children == &vec![Inline::Text("body".to_owned())])
            ));
            let mut whole = Parser::new();
            whole.append(&format!("prefix {d}{d}body{d}{d}"));
            assert_eq!(parser.blocks(), whole.blocks());
        }
    }

    #[test]
    fn complete_semantic_chunks_splice_without_republishing_paragraph() {
        let mut parser = Parser::new();
        parser.append("prefix ");
        let reference = parser.append("@[source:id] suffix ");
        assert!(matches!(
            reference.ops.as_slice(),
            [Op::SpliceInlineTail {
                remove_nodes: 0,
                truncate_bytes: 0,
                append,
                ..
            }] if matches!(append.as_slice(),
                [Inline::Link { destination, .. }, Inline::Text(text)]
                    if destination == "llm:source:id" && text == " suffix ")
        ));

        let citation = parser.append("[[cite:doc|Spec]] tail");
        assert!(matches!(
            citation.ops.as_slice(),
            [Op::SpliceInlineTail {
                remove_nodes: 0,
                truncate_bytes: 0,
                append,
                ..
            }] if matches!(append.as_slice(),
                [Inline::Link { label, destination }, Inline::Text(text)]
                    if destination == "llm:cite:doc"
                        && label == &vec![Inline::Text("Spec".to_owned())]
                        && text == " tail")
        ));

        let mut whole = Parser::new();
        whole.append("prefix @[source:id] suffix [[cite:doc|Spec]] tail");
        assert_eq!(parser.blocks(), whole.blocks());
    }

    #[test]
    fn complete_semantic_chunk_fast_path_is_conservative() {
        let mut unresolved = Parser::new();
        unresolved.append("prefix [");
        let delta = unresolved.append("@[source:id] suffix");
        assert!(matches!(delta.ops.first(), Some(Op::Truncate { .. })));
        let mut whole = Parser::new();
        whole.append("prefix [@[source:id] suffix");
        assert_eq!(unresolved.blocks(), whole.blocks());

        let mut mixed = Parser::new();
        mixed.append("prefix ");
        let delta = mixed.append("@[source:id] **bold**");
        assert!(matches!(delta.ops.first(), Some(Op::Truncate { .. })));
        let mut whole = Parser::new();
        whole.append("prefix @[source:id] **bold**");
        assert_eq!(mixed.blocks(), whole.blocks());
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
        assert!(matches!(
            close.ops.as_slice(),
            [Op::SpliceInlineTail { truncate_bytes, append, .. }]
                if *truncate_bytes == "@[source:id".len() as u32
                    && matches!(append.as_slice(), [Inline::Link { destination, .. }] if destination == "llm:source:id")
        ));
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
        assert!(matches!(
            second.ops.as_slice(),
            [Op::SpliceInlineTail { truncate_bytes, append, .. }]
                if *truncate_bytes == "[[cite:doc]".len() as u32
                    && matches!(append.as_slice(), [Inline::Link { destination, .. }] if destination == "llm:cite:doc")
        ));
        assert!(matches!(
            citation.blocks(),
            [Block::Paragraph(nodes)] if nodes.iter().any(|node| matches!(
                node,
                Inline::Link { destination, .. } if destination == "llm:cite:doc"
            ))
        ));

        let mut labeled = Parser::new();
        labeled.append("prefix [[cite:doc| spec ");
        labeled.append("]");
        let close = labeled.append("]");
        assert!(matches!(
            close.ops.as_slice(),
            [Op::SpliceInlineTail { append, .. }]
                if matches!(append.as_slice(), [Inline::Link { label, destination }]
                    if destination == "llm:cite:doc"
                        && label == &vec![Inline::Text("spec".to_owned())])
        ));
        let mut whole = Parser::new();
        whole.append("prefix [[cite:doc| spec ]]");
        assert_eq!(labeled.blocks(), whole.blocks());

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
    fn complete_link_chunks_splice_without_republishing_paragraph() {
        let mut parser = Parser::new();
        parser.append("prefix ");
        let first = parser.append("[x](url) suffix ");
        assert!(matches!(
            first.ops.as_slice(),
            [Op::SpliceInlineTail {
                remove_nodes: 0,
                truncate_bytes: 0,
                append,
                ..
            }] if matches!(append.as_slice(),
                [Inline::Link { label, destination }, Inline::Text(text)]
                    if destination == "url"
                        && label == &vec![Inline::Text("x".to_owned())]
                        && text == " suffix ")
        ));

        let second = parser.append("[日本語](https://例.jp) tail");
        assert!(matches!(
            second.ops.as_slice(),
            [Op::SpliceInlineTail { truncate_bytes: 0, append, .. }]
                if matches!(append.as_slice(),
                    [Inline::Link { label, destination }, Inline::Text(text)]
                        if destination == "https://例.jp"
                            && label == &vec![Inline::Text("日本語".to_owned())]
                            && text == " tail")
        ));

        let mut whole = Parser::new();
        whole.append("prefix [x](url) suffix [日本語](https://例.jp) tail");
        assert_eq!(parser.blocks(), whole.blocks());
    }

    #[test]
    fn complete_link_chunk_fast_path_is_conservative() {
        let mut unresolved = Parser::new();
        unresolved.append("prefix [");
        let delta = unresolved.append("[x](url) suffix");
        assert!(matches!(delta.ops.first(), Some(Op::Truncate { .. })));
        let mut whole = Parser::new();
        whole.append("prefix [[x](url) suffix");
        assert_eq!(unresolved.blocks(), whole.blocks());

        let mut mixed = Parser::new();
        mixed.append("prefix ");
        let delta = mixed.append("[x](url) **bold**");
        assert!(matches!(delta.ops.first(), Some(Op::Truncate { .. })));
        let mut whole = Parser::new();
        whole.append("prefix [x](url) **bold**");
        assert_eq!(mixed.blocks(), whole.blocks());

        let mut nested = Parser::new();
        nested.append("prefix ");
        let delta = nested.append("[**x**](url)");
        assert!(matches!(delta.ops.first(), Some(Op::Truncate { .. })));
        let mut whole = Parser::new();
        whole.append("prefix [**x**](url)");
        assert_eq!(nested.blocks(), whole.blocks());
    }

    #[test]
    fn standalone_link_label_keeps_future_link_closure_ambiguous() {
        for markdown in ["prefix [old] [x](u)", "prefix [old] tail [x](u)"] {
            let mut streamed = Parser::new();
            for ch in markdown.chars() {
                let mut buf = [0; 4];
                streamed.append(ch.encode_utf8(&mut buf));
            }
            let mut whole = Parser::new();
            whole.append(markdown);
            assert_eq!(streamed.blocks(), whole.blocks(), "markdown={markdown:?}");
        }
    }

    #[test]
    fn simple_links_splice_only_without_outer_openers() {
        let mut parser = Parser::new();
        parser.append("prefix ");
        parser.append("[");
        parser.append("x");
        parser.append("]");
        parser.append("(");
        parser.append("url");
        let close = parser.append(")");
        assert!(matches!(
            close.ops.as_slice(),
            [Op::SpliceInlineTail { truncate_bytes, append, .. }]
                if *truncate_bytes == 7
                    && matches!(append.as_slice(), [Inline::Link { label, destination }]
                        if destination == "url"
                            && matches!(label.as_slice(), [Inline::Text(text)] if text == "x"))
        ));
        let mut whole = Parser::new();
        whole.append("prefix [x](url)");
        assert_eq!(parser.blocks(), whole.blocks());

        let mut escaped = Parser::new();
        let markdown = r"\[x](u)";
        for ch in markdown.chars() {
            let mut buf = [0; 4];
            escaped.append(ch.encode_utf8(&mut buf));
        }
        let mut whole = Parser::new();
        whole.append(markdown);
        assert_eq!(escaped.blocks(), whole.blocks());

        let mut outer = Parser::new();
        let markdown = "[[x](u)](v)";
        for ch in markdown.chars() {
            let mut buf = [0; 4];
            outer.append(ch.encode_utf8(&mut buf));
        }
        let mut whole = Parser::new();
        whole.append(markdown);
        assert_eq!(outer.blocks(), whole.blocks());
    }

    #[test]
    fn formatted_link_labels_splice_from_recorded_ast_boundary() {
        for markdown in [
            "[*x*](u)",
            "prefix [**x**](url) suffix",
            "prefix [`x`](u)",
            "prefix [a *x* b](url)",
            "prefix [日 **本** 語](u)",
        ] {
            let mut parser = Parser::new();
            let mut mirror = Vec::new();
            let mut close = Delta::default();
            for ch in markdown.chars() {
                let mut buf = [0; 4];
                let chunk = ch.encode_utf8(&mut buf);
                let delta = parser.append(chunk);
                apply(&mut mirror, &delta);
                assert_eq!(mirror, parser.blocks(), "mirror diverged for {markdown:?}");
                if chunk == ")" {
                    close = delta;
                }
            }
            let mut whole = Parser::new();
            whole.append(markdown);
            assert_eq!(parser.blocks(), whole.blocks(), "markdown={markdown:?}");
            assert!(
                close
                    .ops
                    .iter()
                    .all(|op| matches!(op, Op::SpliceInlineTail { .. })),
                "formatted link close republished paragraph: {markdown:?} {close:?}"
            );
        }

        // A nested raw opener remains ambiguous and must keep the conservative
        // fallback instead of applying the local rich-label splice.
        let markdown = "[[*x*](u)](v)";
        let mut streamed = Parser::new();
        for ch in markdown.chars() {
            let mut buf = [0; 4];
            streamed.append(ch.encode_utf8(&mut buf));
        }
        let mut whole = Parser::new();
        whole.append(markdown);
        assert_eq!(streamed.blocks(), whole.blocks());
    }

    #[test]
    fn link_destination_syntax_keeps_outer_tail_state() {
        for markdown in [
            "[x](foo_bar)",
            "[x](a$b$c)",
            "[x](a`b`c)",
            "[x](a@b)",
            "[x](a[b])",
            "[x](a(b)",
            "[x](a@[source:y])",
            "[x](a[[cite:d]])",
            "[*x*](a@[source:y])",
        ] {
            let mut streamed = Parser::new();
            let mut prefix = String::new();
            let mut close = Delta::default();
            for ch in markdown.chars() {
                let mut buf = [0; 4];
                let chunk = ch.encode_utf8(&mut buf);
                let delta = streamed.append(chunk);
                prefix.push(ch);

                let mut whole_prefix = Parser::new();
                whole_prefix.append(&prefix);
                assert_eq!(
                    streamed.blocks(),
                    whole_prefix.blocks(),
                    "prefix mismatch: markdown={markdown:?} prefix={prefix:?}"
                );
                if chunk == ")" {
                    close = delta;
                }
            }
            assert!(
                close
                    .ops
                    .iter()
                    .all(|op| matches!(op, Op::SpliceInlineTail { .. })),
                "destination syntax republished paragraph: {markdown:?} {close:?}"
            );
        }
    }

    #[test]
    fn escaped_closers_crossing_chunks_reparse() {
        for closer in [']', ')'] {
            let mut parser = Parser::new();
            parser.append("\\");
            let delta = parser.append(&closer.to_string());
            assert!(matches!(delta.ops.first(), Some(Op::Truncate { from: 0 })));

            let mut whole = Parser::new();
            whole.append(&format!("\\{closer}"));
            assert_eq!(parser.blocks(), whole.blocks());
        }

        let mut semantic = Parser::new();
        semantic.append("@[source:id\\");
        let delta = semantic.append("]");
        assert!(matches!(delta.ops.first(), Some(Op::Truncate { from: 0 })));
        let mut whole = Parser::new();
        whole.append("@[source:id\\]");
        assert_eq!(semantic.blocks(), whole.blocks());
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
            link_close.ops.as_slice(),
            [Op::SpliceInlineTail { append, .. }]
                if matches!(append.as_slice(), [Inline::Link { destination, .. }] if destination == "url")
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

    #[test]
    fn complete_table_rows_append_without_republishing_table() {
        let mut parser = Parser::new();
        parser.append("a|b\n---|---\n");
        let rich = parser.append("**x**|[y](u)\n");
        assert!(matches!(
            rich.ops.as_slice(),
            [Op::AppendTableRow { row, .. }]
                if matches!(row.as_slice(), [left, right]
                    if matches!(left.as_slice(), [Inline::Strong(_)])
                        && matches!(right.as_slice(), [Inline::Link { destination, .. }] if destination == "u"))
        ));
        let open = parser.append("z|q");
        assert!(matches!(open.ops.as_slice(), [Op::AppendTableRow { .. }]));
        assert!(parser.append("\n").ops.is_empty());
        let mut whole = Parser::new();
        whole.append("a|b\n---|---\n**x**|[y](u)\nz|q\n");
        assert_eq!(parser.blocks(), whole.blocks());
    }

    #[test]
    fn complete_table_row_fast_path_is_conservative() {
        let mut multiline = Parser::new();
        multiline.append("a|b\n---|---\n");
        let delta = multiline.append("x|y\nz|q\n");
        assert!(!matches!(delta.ops.as_slice(), [Op::AppendTableRow { .. }]));
        let mut whole = Parser::new();
        whole.append("a|b\n---|---\nx|y\nz|q\n");
        assert_eq!(multiline.blocks(), whole.blocks());

        let mut plain = Parser::new();
        plain.append("a|b\n---|---\n");
        let delta = plain.append("not a row");
        assert!(!matches!(delta.ops.as_slice(), [Op::AppendTableRow { .. }]));
        let mut whole = Parser::new();
        whole.append("a|b\n---|---\nnot a row");
        assert_eq!(plain.blocks(), whole.blocks());
    }

    #[test]
    fn stable_multiline_paragraph_defers_break_delta_until_next_line_starts() {
        let mut parser = Parser::new();
        parser.append("a|b\n---|--x");
        let newline = parser.append("\n");
        assert!(newline.ops.is_empty());

        let next = parser.append("next");
        assert!(matches!(
            next.ops.as_slice(),
            [Op::SpliceInlineTail {
                truncate_bytes: 0,
                append,
                ..
            }] if matches!(append.as_slice(), [Inline::SoftBreak, Inline::Text(text)] if text == "next")
        ));

        let mut hard = Parser::new();
        hard.append("a|b\n---|--x  ");
        assert!(hard.append("\n").ops.is_empty());
        let next = hard.append("next");
        assert!(matches!(
            next.ops.as_slice(),
            [Op::SpliceInlineTail {
                truncate_bytes: 2,
                append,
                ..
            }] if matches!(append.as_slice(), [Inline::HardBreak, Inline::Text(text)] if text == "next")
        ));

        let mut whole = Parser::new();
        whole.append("a|b\n---|--x  \nnext");
        assert_eq!(hard.blocks(), whole.blocks());
    }

    #[test]
    fn heading_tail_deltas_preserve_hidden_space_and_closing_hashes() {
        let mut parser = Parser::new();
        let mut mirror = Vec::new();
        for chunk in ["## ", "title", " ", "#", "x"] {
            let delta = parser.append(chunk);
            apply(&mut mirror, &delta);
            assert_eq!(
                mirror,
                parser.blocks(),
                "heading mirror diverged after {chunk:?}"
            );
        }
        let mut whole = Parser::new();
        whole.append("## title #x");
        assert_eq!(parser.blocks(), whole.blocks());
        assert!(matches!(
            parser.blocks(),
            [Block::Heading { level: 2, content }]
                if content == &vec![Inline::Text("title #x".to_owned())]
        ));
    }

    #[test]
    fn rich_heading_tail_uses_heading_delta_without_republishing() {
        for prefix in [
            "## **bold** ",
            "## [x](u) ",
            "## `code` ",
            "## [[cite:doc|spec]] ",
        ] {
            let mut parser = Parser::new();
            parser.append(prefix);
            let delta = parser.append("token");
            assert!(matches!(
                delta.ops.as_slice(),
                [Op::SpliceHeadingTail { .. }]
            ));

            let mut whole = Parser::new();
            whole.append(&format!("{prefix}token"));
            assert_eq!(parser.blocks(), whole.blocks(), "prefix={prefix:?}");
        }

        let mut closing_hash = Parser::new();
        closing_hash.append("## **bold** #");
        assert!(closing_hash.append("x").ops.iter().any(|op| matches!(
            op,
            Op::SpliceHeadingTail { append, .. }
                if append == &vec![Inline::Text(" #x".to_owned())]
        )));
        let mut whole = Parser::new();
        whole.append("## **bold** #x");
        assert_eq!(closing_hash.blocks(), whole.blocks());
    }

    #[test]
    fn table_token_tail_preserves_trim_and_rich_cell_semantics() {
        for prefix in ["x | ", "**x** | ", "[x](u) | "] {
            let mut parser = Parser::new();
            parser.append("a|b\n---|---\n");
            parser.append(prefix);

            // A trailing pipe is still an outer-border candidate. The first
            // non-space token changes row structure and must reparse once.
            let first = parser.append("token ");
            assert!(matches!(first.ops.first(), Some(Op::Truncate { from: 0 })));

            // Once the final cell exists, plain suffix tokens splice directly.
            let fast = parser.append("  more  ");
            assert!(matches!(
                fast.ops.as_slice(),
                [Op::SpliceTableCellTail { .. }]
            ));

            let mut whole = Parser::new();
            whole.append(&format!("a|b\n---|---\n{prefix}token   more  "));
            assert_eq!(parser.blocks(), whole.blocks(), "prefix={prefix:?}");
        }

        // A row whose final cell is already established can use the fast path
        // immediately, including whitespace that becomes internal text.
        let mut rich_last = Parser::new();
        rich_last.append("a|b\n---|---\na | **x**   ");
        let delta = rich_last.append("token ");
        assert!(matches!(
            delta.ops.as_slice(),
            [Op::SpliceTableCellTail { .. }]
        ));
        let mut whole = Parser::new();
        whole.append("a|b\n---|---\na | **x**   token ");
        assert_eq!(rich_last.blocks(), whole.blocks());
    }

    #[test]
    fn table_token_tail_rechecks_safety_after_inline_reparse() {
        let mut parser = Parser::new();
        parser.append("a|b\n---|---\na | **open");
        let ambiguous = parser.append(" token");
        assert!(matches!(
            ambiguous.ops.first(),
            Some(Op::Truncate { from: 0 })
        ));

        parser.append("**");
        let fast = parser.append(" tail");
        assert!(matches!(
            fast.ops.as_slice(),
            [Op::SpliceTableCellTail { .. }]
        ));

        let mut whole = Parser::new();
        whole.append("a|b\n---|---\na | **open token** tail");
        assert_eq!(parser.blocks(), whole.blocks());
    }

    #[test]
    fn table_tail_deltas_match_streamed_rows() {
        let markdown = "a|b\n---|---\nx|y\nz|日本語";
        let mut parser = Parser::new();
        let mut mirror = Vec::new();
        for ch in markdown.chars() {
            let mut buf = [0; 4];
            let chunk = ch.encode_utf8(&mut buf);
            let delta = parser.append(chunk);
            apply(&mut mirror, &delta);
            assert_eq!(
                mirror,
                parser.blocks(),
                "table mirror diverged after {chunk:?}"
            );
        }
        let mut whole = Parser::new();
        whole.append(markdown);
        assert_eq!(parser.blocks(), whole.blocks());
    }

    #[test]
    fn quote_tail_deltas_match_streamed_quote() {
        let mut parser = Parser::new();
        let mut mirror = Vec::new();
        for chunk in [">", " ", "x", "\n", ">", " ", "日", "本", "語"] {
            let delta = parser.append(chunk);
            apply(&mut mirror, &delta);
            assert_eq!(
                mirror,
                parser.blocks(),
                "quote mirror diverged after {chunk:?}"
            );
        }
        let mut whole = Parser::new();
        whole.append("> x\n> 日本語");
        assert_eq!(parser.blocks(), whole.blocks());
        assert!(matches!(
            parser.blocks(),
            [Block::BlockQuote(nodes)] if nodes == &vec![
                Inline::Text("x".to_owned()),
                Inline::SoftBreak,
                Inline::Text("日本語".to_owned()),
            ]
        ));
    }

    #[test]
    fn complete_list_item_chunks_append_without_republishing_list() {
        let mut unordered = Parser::new();
        unordered.append("- one\n");
        let rich = unordered.append("- **two** [x](u)\n");
        assert!(matches!(
            rich.ops.as_slice(),
            [Op::AppendListItem { item, .. }]
                if matches!(item.as_slice(),
                    [Inline::Strong(_), Inline::Text(space), Inline::Link { destination, .. }]
                        if space == " " && destination == "u")
        ));
        let open = unordered.append("- three");
        assert!(matches!(open.ops.as_slice(), [Op::AppendListItem { .. }]));
        assert!(unordered.append("\n").ops.is_empty());
        let mut whole = Parser::new();
        whole.append("- one\n- **two** [x](u)\n- three\n");
        assert_eq!(unordered.blocks(), whole.blocks());

        let mut ordered = Parser::new();
        ordered.append("1. one\n");
        let second = ordered.append("7. `two`\n");
        assert!(matches!(
            second.ops.as_slice(),
            [Op::AppendListItem { item, .. }]
                if matches!(item.as_slice(), [Inline::Code(code)] if code == "two")
        ));
        let mut whole = Parser::new();
        whole.append("1. one\n7. `two`\n");
        assert_eq!(ordered.blocks(), whole.blocks());
    }

    #[test]
    fn complete_list_item_chunk_fast_path_is_conservative() {
        let mut thematic = Parser::new();
        thematic.append("- one\n");
        let delta = thematic.append("- - -\n");
        assert!(matches!(delta.ops.first(), Some(Op::Truncate { .. })));
        let mut whole = Parser::new();
        whole.append("- one\n- - -\n");
        assert_eq!(thematic.blocks(), whole.blocks());

        let mut mixed = Parser::new();
        mixed.append("- one\n");
        let delta = mixed.append("2. two\n");
        assert!(matches!(delta.ops.first(), Some(Op::Truncate { .. })));
        let mut whole = Parser::new();
        whole.append("- one\n2. two\n");
        assert_eq!(mixed.blocks(), whole.blocks());

        let mut multiline = Parser::new();
        multiline.append("- one\n");
        let delta = multiline.append("- two\n- three\n");
        assert!(matches!(delta.ops.first(), Some(Op::Truncate { .. })));
        let mut whole = Parser::new();
        whole.append("- one\n- two\n- three\n");
        assert_eq!(multiline.blocks(), whole.blocks());
    }

    #[test]
    fn streamed_list_kind_switch_finalizes_pending_block_before_newline() {
        for markdown in [
            "1. one\n2. two\n- other\n",
            "- one\n- two\n3. other\n",
            "- one\n- two\nparagraph tail",
            "- one\n- two\n- - -\n",
        ] {
            let cut = markdown.len() - usize::from(markdown.ends_with('\n'));
            let mut streamed = Parser::new();
            streamed.append(&markdown[..cut]);
            if cut != markdown.len() {
                streamed.append(&markdown[cut..]);
            }
            let mut whole = Parser::new();
            whole.append(markdown);
            assert_eq!(streamed.blocks(), whole.blocks(), "markdown={markdown:?}");
        }
    }

    #[test]
    fn list_tail_deltas_match_streamed_lists() {
        for markdown in ["- one\n- two\n- three", "1. one\n2. two\n3. three"] {
            let mut parser = Parser::new();
            let mut mirror = Vec::new();
            for ch in markdown.chars() {
                let mut buf = [0; 4];
                let delta = parser.append(ch.encode_utf8(&mut buf));
                apply(&mut mirror, &delta);
                assert_eq!(mirror, parser.blocks(), "after {ch:?} in {markdown:?}");
            }
            let mut whole = Parser::new();
            whole.append(markdown);
            assert_eq!(parser.blocks(), whole.blocks());
        }
    }

    #[test]
    fn rich_list_item_syntax_replaces_only_the_final_item() {
        let mut unordered = Parser::new();
        unordered.append("- *x");
        let close = unordered.append("*");
        assert!(
            !close.ops.is_empty()
                && close
                    .ops
                    .iter()
                    .all(|op| matches!(op, Op::SpliceListItemTail { .. }))
        );
        let mut whole = Parser::new();
        whole.append("- *x*");
        assert_eq!(unordered.blocks(), whole.blocks());

        let mut ordered = Parser::new();
        ordered.append("1. [x](u");
        let close = ordered.append(")");
        assert!(
            !close.ops.is_empty()
                && close
                    .ops
                    .iter()
                    .all(|op| matches!(op, Op::SpliceListItemTail { .. }))
        );
        let mut whole = Parser::new();
        whole.append("1. [x](u)");
        assert_eq!(ordered.blocks(), whole.blocks());
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
                Op::AppendListItem { block, item } => match &mut document[*block as usize] {
                    Block::UnorderedList(items) | Block::OrderedList { items, .. } => {
                        items.push(item.clone());
                    }
                    _ => panic!("not list"),
                },
                Op::SpliceListItemTail {
                    block,
                    remove_nodes,
                    truncate_bytes,
                    append,
                } => match &mut document[*block as usize] {
                    Block::UnorderedList(items) | Block::OrderedList { items, .. } => {
                        splice_list_item_tail(
                            items,
                            *truncate_bytes as usize,
                            *remove_nodes as usize,
                            append,
                        );
                    }
                    _ => panic!("not list"),
                },
                Op::AppendTableRow { block, row } => {
                    let Block::Table { rows, .. } = &mut document[*block as usize] else {
                        panic!("not table")
                    };
                    rows.push(row.clone());
                }
                Op::AppendTableCell { block, cell } => {
                    let Block::Table { rows, .. } = &mut document[*block as usize] else {
                        panic!("not table")
                    };
                    rows.last_mut()
                        .expect("table row missing")
                        .push(cell.clone());
                }
                Op::SpliceTableCellTail {
                    block,
                    remove_nodes,
                    truncate_bytes,
                    append,
                } => {
                    let Block::Table { rows, .. } = &mut document[*block as usize] else {
                        panic!("not table")
                    };
                    let cell = rows
                        .last_mut()
                        .and_then(|row| row.last_mut())
                        .expect("table cell missing");
                    splice_inline_tail(
                        cell,
                        *truncate_bytes as usize,
                        *remove_nodes as usize,
                        append,
                    );
                }
                Op::SpliceQuoteTail {
                    block,
                    remove_nodes,
                    truncate_bytes,
                    append,
                } => {
                    let Block::BlockQuote(nodes) = &mut document[*block as usize] else {
                        panic!("not quote")
                    };
                    splice_inline_tail(
                        nodes,
                        *truncate_bytes as usize,
                        *remove_nodes as usize,
                        append,
                    );
                }
                Op::SpliceHeadingTail {
                    block,
                    remove_nodes,
                    truncate_bytes,
                    append,
                } => {
                    let Block::Heading { content, .. } = &mut document[*block as usize] else {
                        panic!("not heading")
                    };
                    splice_inline_tail(
                        content,
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
