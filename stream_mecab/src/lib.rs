//! Clean-room, streaming-first Japanese morphological tokenization.
//!
//! This crate does not link to MeCab and does not ship or parse a MeCab/IPADIC
//! dictionary format.  The model is intentionally small: a UTF-8 byte trie,
//! user-defined transition costs, bounded unknown-word candidates, and an
//! incremental Viterbi front end that publishes `retract + push` deltas.
//!
//! The key streaming invariant is that tokens are committed only when they are
//! both (a) common to every live end-state path and (b) farther from the input
//! edge than the longest token the model can ever create.  Committed input can
//! then be dropped permanently; only the short ambiguous tail is reparsed.

mod compiled;
#[cfg(target_arch = "wasm32")]
mod wasm;
mod wire;

pub use wire::encode_delta_into;

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

pub type TagId = u16;

// Reserved coarse tags used only by the built-in unknown-word fallback.
pub const TAG_BOS_EOS: TagId = 0;
pub const TAG_UNKNOWN_HAN: TagId = 1;
pub const TAG_UNKNOWN_HIRAGANA: TagId = 2;
pub const TAG_UNKNOWN_KATAKANA: TagId = 3;
pub const TAG_UNKNOWN_LATIN: TagId = 4;
pub const TAG_UNKNOWN_NUMBER: TagId = 5;
pub const TAG_UNKNOWN_SPACE: TagId = 6;
pub const TAG_UNKNOWN_PUNCT: TagId = 7;
pub const TAG_UNKNOWN_OTHER: TagId = 8;
pub const FIRST_USER_TAG: TagId = 9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenOrigin {
    Lexicon,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    /// UTF-8 byte offset in the complete stream.
    pub start: usize,
    /// Exclusive UTF-8 byte offset in the complete stream.
    pub end: usize,
    pub surface: Arc<str>,
    pub lemma: Arc<str>,
    pub reading: Arc<str>,
    pub tag: TagId,
    pub word_cost: i32,
    pub origin: TokenOrigin,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreamDelta {
    /// Remove this many tokens from the end of the previously published result.
    pub retract: usize,
    /// Append these tokens after the retraction.
    pub push: Vec<Token>,
}

impl StreamDelta {
    pub fn apply(&self, tokens: &mut Vec<Token>) {
        let keep = tokens.len().saturating_sub(self.retract);
        tokens.truncate(keep);
        tokens.extend(self.push.iter().cloned());
    }
}

#[derive(Clone, Debug)]
struct LexiconEntry {
    surface: Arc<str>,
    lemma: Arc<str>,
    reading: Arc<str>,
    tag: TagId,
    cost: i32,
}

#[derive(Clone, Debug, Default)]
struct TrieNode {
    // UTF-8 byte edges are kept sorted. Typical nodes have only one or two
    // children, where a HashMap is substantially larger and less cache-local.
    next: Vec<(u8, usize)>,
    entries: Vec<usize>,
}

impl TrieNode {
    fn child(&self, byte: u8) -> Option<usize> {
        self.next
            .iter()
            .find_map(|&(edge, target)| (edge == byte).then_some(target))
    }
}

/// Morphological model. No external dictionary is bundled with the crate.
#[derive(Clone, Debug)]
pub struct Model {
    entries: Vec<LexiconEntry>,
    trie: Vec<TrieNode>,
    transitions: HashMap<(TagId, TagId), i32>,
    max_lexicon_bytes: usize,
    max_unknown_chars: usize,
    empty: Arc<str>,
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            trie: vec![TrieNode::default()],
            transitions: HashMap::new(),
            max_lexicon_bytes: 0,
            max_unknown_chars: 8,
            empty: Arc::from(""),
        }
    }

    /// Bound unknown Latin/number/katakana/space runs. This bound is what lets
    /// the streaming analyzer permanently commit old input.
    pub fn set_max_unknown_chars(&mut self, chars: usize) {
        self.max_unknown_chars = chars.clamp(1, 1024);
    }

    pub fn add_entry(
        &mut self,
        surface: impl Into<String>,
        lemma: impl Into<String>,
        reading: impl Into<String>,
        tag: TagId,
        cost: i32,
    ) -> Result<(), ModelError> {
        let surface: Arc<str> = Arc::from(surface.into());
        if surface.is_empty() {
            return Err(ModelError::EmptySurface);
        }
        if tag < FIRST_USER_TAG {
            return Err(ModelError::ReservedTag(tag));
        }
        let lemma: Arc<str> = Arc::from(lemma.into());
        let reading: Arc<str> = Arc::from(reading.into());
        let entry_index = self.entries.len();
        self.max_lexicon_bytes = self.max_lexicon_bytes.max(surface.len());
        self.entries.push(LexiconEntry {
            surface: surface.clone(),
            lemma,
            reading,
            tag,
            cost,
        });

        let mut node = 0usize;
        for byte in surface.bytes() {
            let search = self.trie[node]
                .next
                .binary_search_by_key(&byte, |&(edge, _)| edge);
            let next = match search {
                Ok(index) => self.trie[node].next[index].1,
                Err(index) => {
                    let next = self.trie.len();
                    self.trie.push(TrieNode::default());
                    self.trie[node].next.insert(index, (byte, next));
                    next
                }
            };
            node = next;
        }
        self.trie[node].entries.push(entry_index);
        Ok(())
    }

    /// Set a first-order transition cost from the previous token tag to the
    /// next token tag. Missing transitions cost zero.
    pub fn set_transition(&mut self, previous: TagId, next: TagId, cost: i32) {
        self.transitions.insert((previous, next), cost);
    }

    /// Load the crate's deliberately simple, license-neutral TSV format:
    /// `surface<TAB>lemma<TAB>reading<TAB>tag-id<TAB>word-cost`.
    pub fn add_tsv(&mut self, tsv: &str) -> Result<usize, ModelError> {
        let mut added = 0usize;
        for (line_index, raw) in tsv.lines().enumerate() {
            let line = raw.trim_end();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() != 5 {
                return Err(ModelError::InvalidTsv {
                    line: line_index + 1,
                    message: "expected 5 tab-separated fields".to_owned(),
                });
            }
            let tag = fields[3]
                .parse::<TagId>()
                .map_err(|_| ModelError::InvalidTsv {
                    line: line_index + 1,
                    message: "tag-id is not u16".to_owned(),
                })?;
            let cost = fields[4]
                .parse::<i32>()
                .map_err(|_| ModelError::InvalidTsv {
                    line: line_index + 1,
                    message: "word-cost is not i32".to_owned(),
                })?;
            self.add_entry(fields[0], fields[1], fields[2], tag, cost)?;
            added += 1;
        }
        Ok(added)
    }

    pub fn max_token_bytes(&self) -> usize {
        self.max_lexicon_bytes
            .max(self.max_unknown_chars.saturating_mul(4))
            .max(1)
    }

    pub fn tokenize(&self, text: &str) -> Vec<Token> {
        self.analyze(text, 0, TAG_BOS_EOS)
    }

    pub fn stream(self) -> StreamAnalyzer {
        StreamAnalyzer::new(self, true)
    }

    /// Streaming analyzer optimized for consumers that apply every delta and
    /// therefore do not need the analyzer to retain committed token history.
    /// Internal memory stays bounded by the ambiguous tail.
    pub fn stream_delta(self) -> DeltaStreamAnalyzer {
        DeltaStreamAnalyzer {
            inner: StreamAnalyzer::new(self, false),
        }
    }

    fn transition_cost(&self, previous: TagId, next: TagId) -> i32 {
        self.transitions
            .get(&(previous, next))
            .copied()
            .unwrap_or(0)
    }

    fn dictionary_candidates(&self, text: &str, start: usize, out: &mut Vec<Candidate>) {
        let bytes = text.as_bytes();
        let mut node = 0usize;
        let mut end = start;
        while end < bytes.len() {
            let Some(next) = self.trie[node].child(bytes[end]) else {
                break;
            };
            node = next;
            end += 1;
            for &entry in &self.trie[node].entries {
                // Every inserted surface is valid UTF-8. A complete byte-trie
                // match therefore necessarily lands on a char boundary.
                out.push(Candidate::Lexicon { end, entry });
            }
        }
    }

    fn unknown_candidates(&self, text: &str, start: usize, out: &mut Vec<Candidate>) {
        let first = text[start..]
            .chars()
            .next()
            .expect("start is a char boundary");
        let class = classify(first);
        let first_end = start + first.len_utf8();
        out.push(Candidate::Unknown {
            end: first_end,
            class,
            chars: 1,
        });

        if !class.groups_runs() || self.max_unknown_chars <= 1 {
            return;
        }
        let mut chars = 1usize;
        let mut end = first_end;
        for ch in text[first_end..].chars() {
            if chars >= self.max_unknown_chars || classify(ch) != class {
                break;
            }
            chars += 1;
            end += ch.len_utf8();
        }
        if chars > 1 {
            out.push(Candidate::Unknown { end, class, chars });
        }
    }

    fn analyze(&self, text: &str, base: usize, start_tag: TagId) -> Vec<Token> {
        let mut scratch = Scratch::default();
        self.analyze_with_scratch(text, base, start_tag, &mut scratch);
        scratch.best
    }

    fn analyze_with_scratch(
        &self,
        text: &str,
        base: usize,
        start_tag: TagId,
        scratch: &mut Scratch,
    ) -> usize {
        scratch.prepare(text.len());
        if text.is_empty() {
            return 0;
        }

        let Scratch {
            frontier,
            nodes,
            candidates,
            best,
        } = scratch;
        frontier[0].push(State {
            tag: start_tag,
            cost: 0,
            node: None,
        });

        for start in 0..text.len() {
            if frontier[start].is_empty() || !text.is_char_boundary(start) {
                continue;
            }
            candidates.clear();
            self.dictionary_candidates(text, start, candidates);
            self.unknown_candidates(text, start, candidates);

            for candidate in candidates.iter().copied() {
                let (end, tag, word_cost) = candidate.meta(self);
                let mut best_prev: Option<(i64, Option<usize>)> = None;
                for state in &frontier[start] {
                    let total = state
                        .cost
                        .saturating_add(self.transition_cost(state.tag, tag) as i64)
                        .saturating_add(word_cost as i64);
                    if best_prev.is_none_or(|(cost, _)| total < cost) {
                        best_prev = Some((total, state.node));
                    }
                }
                let Some((cost, prev)) = best_prev else {
                    continue;
                };
                if let Some(existing) = frontier[end].iter_mut().find(|state| state.tag == tag) {
                    if cost < existing.cost {
                        let index = nodes.len();
                        let depth = prev.map_or(1, |index| nodes[index].depth + 1);
                        nodes.push(PathNode {
                            start,
                            end,
                            candidate,
                            prev,
                            depth,
                        });
                        existing.cost = cost;
                        existing.node = Some(index);
                    }
                } else {
                    let index = nodes.len();
                    let depth = prev.map_or(1, |index| nodes[index].depth + 1);
                    nodes.push(PathNode {
                        start,
                        end,
                        candidate,
                        prev,
                        depth,
                    });
                    frontier[end].push(State {
                        tag,
                        cost,
                        node: Some(index),
                    });
                }
            }
        }

        let end_states = &frontier[text.len()];
        debug_assert!(
            !end_states.is_empty(),
            "unknown fallback must make every UTF-8 string reachable"
        );
        let mut best_index = 0usize;
        let mut best_cost = i64::MAX;
        for (index, state) in end_states.iter().enumerate() {
            let total = state
                .cost
                .saturating_add(self.transition_cost(state.tag, TAG_BOS_EOS) as i64);
            if total < best_cost {
                best_cost = total;
                best_index = index;
            }
        }

        let best_node = end_states[best_index].node;
        reconstruct_tokens_into(self, text, base, nodes, best_node, best);

        // A future token that crosses today's stream edge can start anywhere
        // inside the final max-token-width window. A currently suboptimal state
        // there may become optimal after more bytes arrive. Path nodes share
        // backpointers, so the deepest common prefix is their integer LCA; no
        // Token/String materialization is needed for this stability test.
        let max_token_bytes = self.max_token_bytes();
        let live_start = text.len().saturating_sub(max_token_bytes);
        let mut common: Option<usize> = None;
        let mut initialized = false;
        'frontiers: for states in &frontier[live_start..=text.len()] {
            for state in states {
                if !initialized {
                    common = state.node;
                    initialized = true;
                } else {
                    common = common_ancestor(nodes, common, state.node);
                }
                if initialized && common.is_none() {
                    break 'frontiers;
                }
            }
        }

        let safe_local_end = text.len().saturating_sub(max_token_bytes);
        while let Some(index) = common {
            if nodes[index].end <= safe_local_end {
                break;
            }
            common = nodes[index].prev;
        }
        common.map_or(0, |index| nodes[index].depth)
    }
}

#[derive(Clone, Copy, Debug)]
enum Candidate {
    Lexicon {
        end: usize,
        entry: usize,
    },
    Unknown {
        end: usize,
        class: UnknownClass,
        chars: usize,
    },
}

impl Candidate {
    fn meta(self, model: &Model) -> (usize, TagId, i32) {
        match self {
            Self::Lexicon { end, entry } => {
                let entry = &model.entries[entry];
                (end, entry.tag, entry.cost)
            }
            Self::Unknown { end, class, chars } => (end, class.tag(), class.cost(chars)),
        }
    }

    fn token(self, model: &Model, text: &str, base: usize, start: usize) -> Token {
        match self {
            Self::Lexicon { end, entry } => {
                let entry = &model.entries[entry];
                Token {
                    start: base + start,
                    end: base + end,
                    surface: entry.surface.clone(),
                    lemma: entry.lemma.clone(),
                    reading: entry.reading.clone(),
                    tag: entry.tag,
                    word_cost: entry.cost,
                    origin: TokenOrigin::Lexicon,
                }
            }
            Self::Unknown { end, class, chars } => {
                let surface: Arc<str> = Arc::from(&text[start..end]);
                Token {
                    start: base + start,
                    end: base + end,
                    lemma: surface.clone(),
                    reading: model.empty.clone(),
                    surface,
                    tag: class.tag(),
                    word_cost: class.cost(chars),
                    origin: TokenOrigin::Unknown,
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnknownClass {
    Han,
    Hiragana,
    Katakana,
    Latin,
    Number,
    Space,
    Punct,
    Other,
}

impl UnknownClass {
    fn tag(self) -> TagId {
        match self {
            Self::Han => TAG_UNKNOWN_HAN,
            Self::Hiragana => TAG_UNKNOWN_HIRAGANA,
            Self::Katakana => TAG_UNKNOWN_KATAKANA,
            Self::Latin => TAG_UNKNOWN_LATIN,
            Self::Number => TAG_UNKNOWN_NUMBER,
            Self::Space => TAG_UNKNOWN_SPACE,
            Self::Punct => TAG_UNKNOWN_PUNCT,
            Self::Other => TAG_UNKNOWN_OTHER,
        }
    }

    fn groups_runs(self) -> bool {
        matches!(
            self,
            Self::Katakana | Self::Latin | Self::Number | Self::Space
        )
    }

    fn cost(self, chars: usize) -> i32 {
        let extra = chars.saturating_sub(1).min(i32::MAX as usize) as i32;
        match self {
            Self::Space => extra.saturating_mul(2),
            Self::Punct => 100,
            Self::Latin | Self::Number => 900i32.saturating_add(extra.saturating_mul(8)),
            Self::Katakana => 1_600i32.saturating_add(extra.saturating_mul(20)),
            Self::Han | Self::Hiragana => 3_200,
            Self::Other => 3_600,
        }
    }
}

fn classify(ch: char) -> UnknownClass {
    let code = ch as u32;
    if ch.is_whitespace() {
        UnknownClass::Space
    } else if ch.is_ascii_digit() || ('０'..='９').contains(&ch) {
        UnknownClass::Number
    } else if ch.is_ascii_alphabetic() || ('Ａ'..='Ｚ').contains(&ch) || ('ａ'..='ｚ').contains(&ch)
    {
        UnknownClass::Latin
    } else if (0x3040..=0x309f).contains(&code) {
        UnknownClass::Hiragana
    } else if (0x30a0..=0x30ff).contains(&code) || (0xff65..=0xff9f).contains(&code) {
        UnknownClass::Katakana
    } else if (0x3400..=0x4dbf).contains(&code)
        || (0x4e00..=0x9fff).contains(&code)
        || (0xf900..=0xfaff).contains(&code)
    {
        UnknownClass::Han
    } else if ch.is_ascii_punctuation()
        || (0x3000..=0x303f).contains(&code)
        || (0xff01..=0xff0f).contains(&code)
        || (0xff1a..=0xff20).contains(&code)
        || (0xff3b..=0xff40).contains(&code)
        || (0xff5b..=0xff65).contains(&code)
    {
        UnknownClass::Punct
    } else {
        UnknownClass::Other
    }
}

#[derive(Clone, Copy, Debug)]
struct State {
    tag: TagId,
    cost: i64,
    node: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
struct PathNode {
    start: usize,
    end: usize,
    candidate: Candidate,
    prev: Option<usize>,
    depth: usize,
}

#[derive(Default)]
struct Scratch {
    frontier: Vec<Vec<State>>,
    nodes: Vec<PathNode>,
    candidates: Vec<Candidate>,
    best: Vec<Token>,
}

impl Scratch {
    fn prepare(&mut self, text_len: usize) {
        let needed = text_len + 1;
        if self.frontier.len() < needed {
            self.frontier.resize_with(needed, Vec::new);
        }
        for states in self.frontier.iter_mut().take(needed) {
            states.clear();
        }
        self.nodes.clear();
        self.candidates.clear();
        self.best.clear();
        if self.candidates.capacity() < 12 {
            self.candidates.reserve(12 - self.candidates.capacity());
        }
    }
}

fn reconstruct_tokens_into(
    model: &Model,
    text: &str,
    base: usize,
    nodes: &[PathNode],
    mut node: Option<usize>,
    out: &mut Vec<Token>,
) {
    out.clear();
    let capacity = node.map_or(0, |index| nodes[index].depth);
    if out.capacity() < capacity {
        out.reserve(capacity - out.capacity());
    }
    while let Some(index) = node {
        let current = nodes[index];
        out.push(current.candidate.token(model, text, base, current.start));
        node = current.prev;
    }
    out.reverse();
}

fn common_ancestor(
    nodes: &[PathNode],
    mut left: Option<usize>,
    mut right: Option<usize>,
) -> Option<usize> {
    let (Some(mut l), Some(mut r)) = (left, right) else {
        return None;
    };
    while nodes[l].depth > nodes[r].depth {
        left = nodes[l].prev;
        l = left?;
    }
    while nodes[r].depth > nodes[l].depth {
        right = nodes[r].prev;
        r = right?;
    }
    while l != r {
        left = nodes[l].prev;
        right = nodes[r].prev;
        l = left?;
        r = right?;
    }
    Some(l)
}

fn delta_between_into(old: &[Token], new: &[Token], out: &mut StreamDelta) {
    let mut common = 0usize;
    while common < old.len() && common < new.len() && old[common] == new[common] {
        common += 1;
    }
    out.retract = old.len() - common;
    out.push.clear();
    out.push.extend_from_slice(&new[common..]);
}

/// Stateful incremental analyzer. Only `tail` is reparsed on each append.
pub struct StreamAnalyzer {
    model: Model,
    tail: String,
    tail_offset: usize,
    start_tag: TagId,
    committed_total: usize,
    published: Vec<Token>,
    scratch: Scratch,
    retain_history: bool,
}

impl StreamAnalyzer {
    fn new(model: Model, retain_history: bool) -> Self {
        Self {
            model,
            tail: String::new(),
            tail_offset: 0,
            start_tag: TAG_BOS_EOS,
            committed_total: 0,
            published: Vec::new(),
            scratch: Scratch::default(),
            retain_history,
        }
    }

    pub fn append(&mut self, chunk: &str) -> StreamDelta {
        let mut delta = StreamDelta::default();
        self.append_into(chunk, &mut delta);
        delta
    }

    /// Allocation-reusing hot path. `delta.push` keeps its capacity across
    /// calls, which is important for token-at-a-time LLM streams.
    pub fn append_into(&mut self, chunk: &str, delta: &mut StreamDelta) {
        self.tail.push_str(chunk);
        let stable = self.model.analyze_with_scratch(
            &self.tail,
            self.tail_offset,
            self.start_tag,
            &mut self.scratch,
        );

        let best = &self.scratch.best;
        let old_start = if self.retain_history {
            self.committed_total
        } else {
            0
        };
        let old_tail = &self.published[old_start..];
        delta_between_into(old_tail, best, delta);
        let keep = old_tail.len().saturating_sub(delta.retract);
        self.published.truncate(old_start + keep);
        self.published.extend(delta.push.iter().cloned());

        // A future token can only reach back by at most max_token_bytes().
        // Also require every current end-state to share the prefix, which keeps
        // transition-cost alternatives from being committed prematurely.
        if stable != 0 {
            let stable_tokens = &best[..stable];
            let last = stable_tokens.last().expect("stable is nonzero");
            let new_offset = last.end;
            let drain = new_offset - self.tail_offset;
            self.start_tag = last.tag;
            self.committed_total += stable;
            if !self.retain_history {
                self.published.drain(..stable);
            }
            self.tail.drain(..drain);
            self.tail_offset = new_offset;
        }
    }

    /// Finalize the current tail. Usually returns an empty delta because append
    /// already publishes the current best path; this method merely makes it
    /// permanent and releases the tail buffer.
    pub fn finish(&mut self) -> StreamDelta {
        let mut delta = StreamDelta::default();
        self.finish_into(&mut delta);
        delta
    }

    pub fn finish_into(&mut self, delta: &mut StreamDelta) {
        self.model.analyze_with_scratch(
            &self.tail,
            self.tail_offset,
            self.start_tag,
            &mut self.scratch,
        );
        let best = &self.scratch.best;
        let old_start = if self.retain_history {
            self.committed_total
        } else {
            0
        };
        let old_tail = &self.published[old_start..];
        delta_between_into(old_tail, best, delta);
        let keep = old_tail.len().saturating_sub(delta.retract);
        self.published.truncate(old_start + keep);
        self.published.extend(delta.push.iter().cloned());
        if let Some(last) = best.last() {
            self.start_tag = last.tag;
        }
        if self.retain_history {
            self.committed_total = self.published.len();
        } else {
            self.committed_total += self.published.len();
            self.published.clear();
        }
        self.tail_offset = self.tail_offset.saturating_add(self.tail.len());
        self.tail.clear();
    }

    pub fn tokens(&self) -> &[Token] {
        &self.published
    }

    pub fn tail_bytes(&self) -> usize {
        self.tail.len()
    }

    pub fn committed_tokens(&self) -> usize {
        self.committed_total
    }

    pub fn absolute_bytes(&self) -> usize {
        self.tail_offset + self.tail.len()
    }
}

/// History-free streaming facade. Apply every returned delta to the consumer's
/// own token list; this object only retains the small provisional tail.
pub struct DeltaStreamAnalyzer {
    inner: StreamAnalyzer,
}

impl DeltaStreamAnalyzer {
    pub fn append(&mut self, chunk: &str) -> StreamDelta {
        self.inner.append(chunk)
    }

    pub fn append_into(&mut self, chunk: &str, delta: &mut StreamDelta) {
        self.inner.append_into(chunk, delta);
    }

    pub fn finish(&mut self) -> StreamDelta {
        self.inner.finish()
    }

    pub fn finish_into(&mut self, delta: &mut StreamDelta) {
        self.inner.finish_into(delta);
    }

    pub fn tail_bytes(&self) -> usize {
        self.inner.tail_bytes()
    }

    pub fn committed_tokens(&self) -> usize {
        self.inner.committed_tokens()
    }

    pub fn buffered_tokens(&self) -> usize {
        self.inner.published.len()
    }

    pub fn absolute_bytes(&self) -> usize {
        self.inner.absolute_bytes()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelError {
    EmptySurface,
    ReservedTag(TagId),
    InvalidTsv { line: usize, message: String },
    InvalidCompiled(String),
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySurface => write!(f, "dictionary surface must not be empty"),
            Self::ReservedTag(tag) => write!(
                f,
                "tag {tag} is reserved; user tags start at {FIRST_USER_TAG}"
            ),
            Self::InvalidTsv { line, message } => {
                write!(f, "invalid dictionary TSV at line {line}: {message}")
            }
            Self::InvalidCompiled(message) => write!(f, "invalid compiled dictionary: {message}"),
        }
    }
}

impl std::error::Error for ModelError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_model() -> Model {
        let mut model = Model::new();
        // Tiny synthetic lexicon for tests only; no external dictionary data.
        for (surface, lemma, reading, tag, cost) in [
            ("私", "私", "ワタシ", 9, 400),
            ("は", "は", "ハ", 10, 300),
            ("学生", "学生", "ガクセイ", 9, 300),
            ("です", "です", "デス", 11, 250),
            ("東京", "東京", "トウキョウ", 9, 250),
            ("大学", "大学", "ダイガク", 9, 250),
            ("東京大学", "東京大学", "トウキョウダイガク", 9, 100),
        ] {
            model.add_entry(surface, lemma, reading, tag, cost).unwrap();
        }
        model
    }

    #[test]
    fn dictionary_longest_path_can_beat_shorter_words() {
        let model = demo_model();
        let tokens = model.tokenize("東京大学です");
        assert_eq!(
            tokens
                .iter()
                .map(|t| t.surface.as_ref())
                .collect::<Vec<_>>(),
            ["東京大学", "です"]
        );
    }

    #[test]
    fn delta_application_tracks_streaming_result() {
        let model = demo_model();
        let mut stream = model.clone().stream();
        let mut mirror = Vec::new();
        for chunk in ["東", "京", "大", "学", "で", "す"] {
            stream.append(chunk).apply(&mut mirror);
            assert_eq!(mirror, stream.tokens());
        }
        stream.finish().apply(&mut mirror);
        assert_eq!(mirror, model.tokenize("東京大学です"));
    }

    #[test]
    fn every_utf8_split_matches_batch_after_finish() {
        let model = demo_model();
        let text = "私は東京大学の学生ですABC123。";
        let expected = model.tokenize(text);
        let boundaries: Vec<usize> = (0..=text.len())
            .filter(|&i| text.is_char_boundary(i))
            .collect();
        for &split in &boundaries {
            let mut stream = model.clone().stream();
            stream.append(&text[..split]);
            stream.append(&text[split..]);
            stream.finish();
            assert_eq!(stream.tokens(), expected, "split={split}");
        }
    }

    #[test]
    fn published_prefix_matches_batch_after_every_character() {
        let mut model = demo_model();
        model.set_max_unknown_chars(4);
        // Add overlapping entries so future input repeatedly changes the best
        // local path while old prefixes are being committed.
        model
            .add_entry("学生です", "学生です", "ガクセイデス", 12, 120)
            .unwrap();
        model
            .add_entry("大学の", "大学", "ダイガクノ", 13, 160)
            .unwrap();
        model.set_transition(9, 10, -30);
        model.set_transition(10, 9, -20);

        let text = "私は東京大学の学生です。私は東京大学の学生です。私は東京大学の学生です。";
        let mut stream = model.clone().stream();
        let mut prefix = String::new();
        let mut mirror = Vec::new();
        for ch in text.chars() {
            prefix.push(ch);
            stream.append(&ch.to_string()).apply(&mut mirror);
            let expected = model.tokenize(&prefix);
            assert_eq!(mirror, expected, "prefix={prefix:?}");
            assert_eq!(stream.tokens(), expected);
        }
        stream.finish().apply(&mut mirror);
        assert_eq!(mirror, model.tokenize(text));
        assert!(stream.committed_tokens() > 0);
    }

    #[test]
    fn long_stream_keeps_the_reparsed_tail_small_on_normal_text() {
        let mut model = demo_model();
        model.set_max_unknown_chars(4);
        let sentence = "私は東京大学の学生です。";
        let mut stream = model.stream();
        let mut max_tail = 0usize;
        for _ in 0..100 {
            for ch in sentence.chars() {
                stream.append(&ch.to_string());
                max_tail = max_tail.max(stream.tail_bytes());
            }
        }
        assert!(stream.committed_tokens() > 500);
        assert!(max_tail <= 64, "max_tail={max_tail}");
    }

    #[test]
    fn history_free_stream_matches_batch_and_stays_bounded() {
        let model = demo_model();
        let sentence = "私は東京大学の学生です。";
        let mut stream = model.clone().stream_delta();
        let mut mirror = Vec::new();
        let mut max_buffered = 0usize;
        for _ in 0..200 {
            for ch in sentence.chars() {
                stream.append(&ch.to_string()).apply(&mut mirror);
                max_buffered = max_buffered.max(stream.buffered_tokens());
            }
        }
        stream.finish().apply(&mut mirror);
        assert_eq!(mirror, model.tokenize(&sentence.repeat(200)));
        assert!(max_buffered <= 16, "max_buffered={max_buffered}");
        assert_eq!(stream.buffered_tokens(), 0);
    }

    #[test]
    fn randomized_streaming_matches_batch_at_every_prefix() {
        // Deterministic xorshift; no test-only dependency. The generated model
        // deliberately contains overlapping surfaces and negative connection
        // costs so locally suboptimal paths can become optimal later.
        let mut seed = 0x6a09_e667_f3bc_c909u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let alphabet = ["東", "京", "大", "学", "生", "の", "は", "。"];

        for case in 0..64 {
            let mut model = Model::new();
            model.set_max_unknown_chars(3);
            for entry_index in 0..32 {
                let len = 1 + (next() as usize % 4);
                let mut surface = String::new();
                for _ in 0..len {
                    surface.push_str(alphabet[next() as usize % alphabet.len()]);
                }
                let tag = FIRST_USER_TAG + (next() as TagId % 5);
                let cost = (next() % 1200) as i32 - 400;
                model
                    .add_entry(
                        surface.clone(),
                        format!("lemma-{case}-{entry_index}"),
                        "",
                        tag,
                        cost,
                    )
                    .unwrap();
            }
            for previous in TAG_BOS_EOS..FIRST_USER_TAG + 5 {
                for following in TAG_BOS_EOS..FIRST_USER_TAG + 5 {
                    if next() & 3 == 0 {
                        model.set_transition(previous, following, (next() % 401) as i32 - 200);
                    }
                }
            }

            let mut text = String::new();
            for _ in 0..48 {
                text.push_str(alphabet[next() as usize % alphabet.len()]);
            }
            let mut stream = model.clone().stream_delta();
            let mut mirror = Vec::new();
            let mut prefix = String::new();
            for ch in text.chars() {
                prefix.push(ch);
                stream.append(&ch.to_string()).apply(&mut mirror);
                assert_eq!(
                    mirror,
                    model.tokenize(&prefix),
                    "case={case} prefix={prefix:?}"
                );
            }
            stream.finish().apply(&mut mirror);
            assert_eq!(mirror, model.tokenize(&text), "case={case} final");
        }
    }

    #[test]
    fn custom_tsv_is_not_mecab_dictionary_format() {
        let mut model = Model::new();
        assert_eq!(
            model
                .add_tsv("猫\t猫\tネコ\t9\t100\n犬\t犬\tイヌ\t9\t120\n")
                .unwrap(),
            2
        );
        assert_eq!(
            model
                .tokenize("猫犬")
                .iter()
                .map(|t| t.surface.as_ref())
                .collect::<Vec<_>>(),
            ["猫", "犬"]
        );
    }
}
