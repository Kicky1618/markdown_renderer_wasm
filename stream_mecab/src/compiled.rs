#[cfg(any(target_arch = "wasm32", test))]
use crate::{FrozenNode, FrozenTrie};
use crate::{LexiconEntry, Model, ModelError, TrieNode};
use std::collections::HashMap;
use std::sync::Arc;

const MAGIC: &[u8; 4] = b"SMD1";
const VERSION: u32 = 1;

impl Model {
    /// Compile this model into the clean-room SMD1 format. SMD1 is deliberately
    /// unrelated to MeCab/IPADIC/UniDic dictionary formats and stores the trie
    /// exactly as this crate consumes it.
    pub fn to_compiled(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        put_u32(&mut out, VERSION);
        put_u32(
            &mut out,
            self.max_unknown_chars.min(u32::MAX as usize) as u32,
        );
        put_u32(&mut out, self.entries.len().min(u32::MAX as usize) as u32);
        put_u32(&mut out, self.trie.len().min(u32::MAX as usize) as u32);
        put_u32(
            &mut out,
            self.transitions.len().min(u32::MAX as usize) as u32,
        );

        for entry in self.entries.iter().take(u32::MAX as usize) {
            put_str(&mut out, &entry.surface);
            put_str(&mut out, &entry.lemma);
            put_str(&mut out, &entry.reading);
            put_u16(&mut out, entry.tag);
            put_i32(&mut out, entry.cost);
        }
        for node in self.trie.iter().take(u32::MAX as usize) {
            put_u32(&mut out, node.next.len().min(u32::MAX as usize) as u32);
            put_u32(&mut out, node.entries.len().min(u32::MAX as usize) as u32);
            for &(byte, target) in node.next.iter().take(u32::MAX as usize) {
                out.push(byte);
                put_u32(&mut out, target.min(u32::MAX as usize) as u32);
            }
            for &entry in node.entries.iter().take(u32::MAX as usize) {
                put_u32(&mut out, entry.min(u32::MAX as usize) as u32);
            }
        }
        let mut transitions: Vec<_> = self.transitions.iter().collect();
        transitions.sort_unstable_by_key(|(pair, _)| **pair);
        for (pair, cost) in transitions.into_iter().take(u32::MAX as usize) {
            let (previous, next) = *pair;
            put_u16(&mut out, previous);
            put_u16(&mut out, next);
            put_i32(&mut out, *cost);
        }
        out
    }

    pub fn from_compiled(bytes: &[u8]) -> Result<Self, ModelError> {
        let mut reader = Reader::new(bytes);
        if reader.take(4)? != MAGIC {
            return Err(ModelError::InvalidCompiled("invalid SMD1 magic".to_owned()));
        }
        if reader.u32()? != VERSION {
            return Err(ModelError::InvalidCompiled(
                "unsupported SMD1 version".to_owned(),
            ));
        }
        let max_unknown_chars = reader.u32()? as usize;
        let entry_count = reader.u32()? as usize;
        let node_count = reader.u32()? as usize;
        let transition_count = reader.u32()? as usize;
        validate_top_level_counts(&reader, entry_count, node_count, transition_count)?;
        if node_count == 0 {
            return Err(ModelError::InvalidCompiled(
                "SMD1 trie has no root node".to_owned(),
            ));
        }
        let mut entries = Vec::with_capacity(entry_count);
        let mut max_lexicon_bytes = 0usize;
        for _ in 0..entry_count {
            let surface: Arc<str> = Arc::from(reader.string()?);
            if surface.is_empty() {
                return Err(ModelError::InvalidCompiled(
                    "SMD1 contains empty surface".to_owned(),
                ));
            }
            max_lexicon_bytes = max_lexicon_bytes.max(surface.len());
            entries.push(LexiconEntry {
                surface,
                lemma: Arc::from(reader.string()?),
                reading: Arc::from(reader.string()?),
                tag: reader.u16()?,
                cost: reader.i32()?,
            });
        }
        let mut trie = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            let edge_count = reader.u32()? as usize;
            let terminal_count = reader.u32()? as usize;
            validate_node_counts(&reader, edge_count, terminal_count)?;
            let mut next = Vec::with_capacity(edge_count);
            let mut previous_byte = None;
            for _ in 0..edge_count {
                let byte = reader.u8()?;
                if previous_byte.is_some_and(|previous| byte <= previous) {
                    return Err(ModelError::InvalidCompiled(
                        "SMD1 trie edges are not strictly sorted".to_owned(),
                    ));
                }
                previous_byte = Some(byte);
                let target = reader.u32()? as usize;
                if target >= node_count {
                    return Err(ModelError::InvalidCompiled(
                        "SMD1 trie target out of range".to_owned(),
                    ));
                }
                next.push((byte, target));
            }
            let mut terminal_entries = Vec::with_capacity(terminal_count);
            for _ in 0..terminal_count {
                let entry = reader.u32()? as usize;
                if entry >= entry_count {
                    return Err(ModelError::InvalidCompiled(
                        "SMD1 entry index out of range".to_owned(),
                    ));
                }
                terminal_entries.push(entry);
            }
            let mut node = TrieNode {
                next,
                entries: terminal_entries,
                dense: None,
            };
            if node.next.len() >= TrieNode::DENSE_THRESHOLD {
                let mut dense = Box::new([0u32; 256]);
                for &(edge, child) in &node.next {
                    dense[edge as usize] = child as u32 + 1;
                }
                node.dense = Some(dense);
            }
            trie.push(node);
        }
        let mut transitions = HashMap::with_capacity(transition_count);
        for _ in 0..transition_count {
            let previous = reader.u16()?;
            let next = reader.u16()?;
            let cost = reader.i32()?;
            if transitions.insert((previous, next), cost).is_some() {
                return Err(ModelError::InvalidCompiled(
                    "duplicate SMD1 transition".to_owned(),
                ));
            }
        }
        if !reader.is_empty() {
            return Err(ModelError::InvalidCompiled(
                "trailing bytes after SMD1 dictionary".to_owned(),
            ));
        }
        Ok(Self {
            entries,
            trie,
            frozen_trie: None,
            transitions,
            dense_transitions: None,
            max_lexicon_bytes,
            max_unknown_chars: max_unknown_chars.clamp(1, 1024),
            empty: Arc::from(""),
        })
    }

    /// Load SMD1 directly into the compact streaming trie, avoiding the
    /// allocation-heavy mutable builder trie. This is crate-internal because a
    /// frozen model cannot accept additional lexicon entries; the public
    /// `from_compiled` API intentionally keeps its mutable semantics.
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn from_compiled_for_stream(bytes: &[u8]) -> Result<Self, ModelError> {
        let mut reader = Reader::new(bytes);
        if reader.take(4)? != MAGIC {
            return Err(ModelError::InvalidCompiled("invalid SMD1 magic".to_owned()));
        }
        if reader.u32()? != VERSION {
            return Err(ModelError::InvalidCompiled(
                "unsupported SMD1 version".to_owned(),
            ));
        }
        let max_unknown_chars = reader.u32()? as usize;
        let entry_count = reader.u32()? as usize;
        let node_count = reader.u32()? as usize;
        let transition_count = reader.u32()? as usize;
        validate_top_level_counts(&reader, entry_count, node_count, transition_count)?;
        if node_count == 0 {
            return Err(ModelError::InvalidCompiled(
                "SMD1 trie has no root node".to_owned(),
            ));
        }

        let mut entries = Vec::with_capacity(entry_count);
        let mut max_lexicon_bytes = 0usize;
        for _ in 0..entry_count {
            let surface: Arc<str> = Arc::from(reader.string()?);
            if surface.is_empty() {
                return Err(ModelError::InvalidCompiled(
                    "SMD1 contains empty surface".to_owned(),
                ));
            }
            max_lexicon_bytes = max_lexicon_bytes.max(surface.len());
            entries.push(LexiconEntry {
                surface,
                lemma: Arc::from(reader.string()?),
                reading: Arc::from(reader.string()?),
                tag: reader.u16()?,
                cost: reader.i32()?,
            });
        }

        // SMD1 already stores trie nodes in the same byte-edge order the
        // runtime needs. Pack them in one pass instead of constructing hundreds
        // of thousands of per-node Vec allocations and freezing them later.
        let mut frozen = FrozenTrie {
            nodes: Vec::with_capacity(node_count),
            edge_bytes: Vec::new(),
            edge_targets: Vec::new(),
            entries: Vec::new(),
            dense: Vec::new(),
        };
        for _ in 0..node_count {
            let edge_count = reader.u32()? as usize;
            let terminal_count = reader.u32()? as usize;
            validate_node_counts(&reader, edge_count, terminal_count)?;
            if edge_count > 256 {
                return Err(ModelError::InvalidCompiled(
                    "SMD1 trie node has more than 256 byte edges".to_owned(),
                ));
            }
            if terminal_count > u16::MAX as usize {
                return Err(ModelError::InvalidCompiled(
                    "SMD1 trie node has too many terminal entries for streaming runtime".to_owned(),
                ));
            }
            if frozen.edge_bytes.len() > u32::MAX as usize
                || frozen.entries.len() > u32::MAX as usize
            {
                return Err(ModelError::InvalidCompiled(
                    "SMD1 streaming trie exceeds u32 address space".to_owned(),
                ));
            }

            let edge_start = frozen.edge_bytes.len() as u32;
            let entry_start = frozen.entries.len() as u32;
            let mut previous_byte = None;
            let mut dense = (edge_count >= TrieNode::DENSE_THRESHOLD).then_some([0u32; 256]);
            for _ in 0..edge_count {
                let byte = reader.u8()?;
                if previous_byte.is_some_and(|previous| byte <= previous) {
                    return Err(ModelError::InvalidCompiled(
                        "SMD1 trie edges are not strictly sorted".to_owned(),
                    ));
                }
                previous_byte = Some(byte);
                let target = reader.u32()? as usize;
                if target >= node_count {
                    return Err(ModelError::InvalidCompiled(
                        "SMD1 trie target out of range".to_owned(),
                    ));
                }
                frozen.edge_bytes.push(byte);
                frozen.edge_targets.push(target as u32);
                if let Some(table) = &mut dense {
                    table[byte as usize] = target as u32 + 1;
                }
            }
            for _ in 0..terminal_count {
                let entry = reader.u32()? as usize;
                if entry >= entry_count {
                    return Err(ModelError::InvalidCompiled(
                        "SMD1 entry index out of range".to_owned(),
                    ));
                }
                frozen.entries.push(entry as u32);
            }
            let dense_index = if let Some(table) = dense {
                if frozen.dense.len() >= u32::MAX as usize {
                    return Err(ModelError::InvalidCompiled(
                        "SMD1 has too many dense trie nodes".to_owned(),
                    ));
                }
                let index = frozen.dense.len() as u32;
                frozen.dense.push(table);
                index
            } else {
                FrozenTrie::NO_DENSE
            };
            frozen.nodes.push(FrozenNode {
                edge_start,
                entry_start,
                dense_index,
                edge_len: edge_count as u16,
                entry_len: terminal_count as u16,
            });
        }

        let mut transitions = HashMap::with_capacity(transition_count);
        for _ in 0..transition_count {
            let previous = reader.u16()?;
            let next = reader.u16()?;
            let cost = reader.i32()?;
            if transitions.insert((previous, next), cost).is_some() {
                return Err(ModelError::InvalidCompiled(
                    "duplicate SMD1 transition".to_owned(),
                ));
            }
        }
        if !reader.is_empty() {
            return Err(ModelError::InvalidCompiled(
                "trailing bytes after SMD1 dictionary".to_owned(),
            ));
        }
        Ok(Self {
            entries,
            trie: Vec::new(),
            frozen_trie: Some(frozen),
            transitions,
            dense_transitions: None,
            max_lexicon_bytes,
            max_unknown_chars: max_unknown_chars.clamp(1, 1024),
            empty: Arc::from(""),
        })
    }

    /// Preserve the WASM API's historical ability to append source TSV after
    /// `loadCompiled()`. The common compiled-dictionary path stays frozen; only
    /// an actual mutation pays the cost of rebuilding mutable trie nodes.
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn add_tsv_after_compiled_load(&mut self, tsv: &str) -> Result<usize, ModelError> {
        if self.trie.is_empty()
            && let Some(frozen) = self.frozen_trie.take()
        {
            let mut trie = Vec::with_capacity(frozen.nodes.len());
            for meta in &frozen.nodes {
                let edge_start = meta.edge_start as usize;
                let edge_end = edge_start + meta.edge_len as usize;
                let entry_start = meta.entry_start as usize;
                let entry_end = entry_start + meta.entry_len as usize;
                let mut next = Vec::with_capacity(meta.edge_len as usize);
                for (&byte, &target) in frozen.edge_bytes[edge_start..edge_end]
                    .iter()
                    .zip(&frozen.edge_targets[edge_start..edge_end])
                {
                    next.push((byte, target as usize));
                }
                let entries = frozen.entries[entry_start..entry_end]
                    .iter()
                    .map(|&entry| entry as usize)
                    .collect();
                let dense = if meta.dense_index != FrozenTrie::NO_DENSE {
                    Some(Box::new(frozen.dense[meta.dense_index as usize]))
                } else {
                    None
                };
                trie.push(TrieNode {
                    next,
                    entries,
                    dense,
                });
            }
            self.trie = trie;
        }
        self.add_tsv(tsv)
    }
}

fn validate_top_level_counts(
    reader: &Reader<'_>,
    entry_count: usize,
    node_count: usize,
    transition_count: usize,
) -> Result<(), ModelError> {
    // An entry needs at least three u32 string lengths, one non-empty surface
    // byte, a tag and a cost (19B); a trie node needs two counts (8B); a
    // transition is 8B. Check before capacities are allocated.
    let minimum_payload = entry_count
        .checked_mul(19)
        .and_then(|value| {
            node_count
                .checked_mul(8)
                .and_then(|nodes| value.checked_add(nodes))
        })
        .and_then(|value| {
            transition_count
                .checked_mul(8)
                .and_then(|transitions| value.checked_add(transitions))
        })
        .ok_or_else(|| ModelError::InvalidCompiled("SMD1 count overflow".to_owned()))?;
    if minimum_payload > reader.remaining() {
        return Err(ModelError::InvalidCompiled(
            "SMD1 counts exceed input size".to_owned(),
        ));
    }
    Ok(())
}

fn validate_node_counts(
    reader: &Reader<'_>,
    edge_count: usize,
    terminal_count: usize,
) -> Result<(), ModelError> {
    let node_payload = edge_count
        .checked_mul(5)
        .and_then(|edges| {
            terminal_count
                .checked_mul(4)
                .and_then(|terminals| edges.checked_add(terminals))
        })
        .ok_or_else(|| ModelError::InvalidCompiled("SMD1 node count overflow".to_owned()))?;
    if node_payload > reader.remaining() {
        return Err(ModelError::InvalidCompiled(
            "SMD1 node counts exceed input size".to_owned(),
        ));
    }
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], ModelError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| ModelError::InvalidCompiled("SMD1 offset overflow".to_owned()))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| ModelError::InvalidCompiled("truncated SMD1 dictionary".to_owned()))?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, ModelError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, ModelError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, ModelError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, ModelError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn string(&mut self) -> Result<&'a str, ModelError> {
        let len = self.u32()? as usize;
        std::str::from_utf8(self.take(len)?)
            .map_err(|_| ModelError::InvalidCompiled("SMD1 string is not UTF-8".to_owned()))
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    fn is_empty(&self) -> bool {
        self.remaining() == 0
    }
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_str(out: &mut Vec<u8>, value: &str) {
    put_u32(out, value.len().min(u32::MAX as usize) as u32);
    out.extend_from_slice(&value.as_bytes()[..value.len().min(u32::MAX as usize)]);
}

#[cfg(test)]
mod tests {
    use crate::{FIRST_USER_TAG, Model};

    #[test]
    fn compiled_round_trip_preserves_segmentation_and_transitions() {
        let mut model = Model::new();
        model.set_max_unknown_chars(5);
        model
            .add_entry("東京", "東京", "トウキョウ", FIRST_USER_TAG, 100)
            .unwrap();
        model
            .add_entry("大学", "大学", "ダイガク", FIRST_USER_TAG, 100)
            .unwrap();
        model
            .add_entry(
                "東京大学",
                "東京大学",
                "トウキョウダイガク",
                FIRST_USER_TAG,
                20,
            )
            .unwrap();
        model.set_transition(FIRST_USER_TAG, FIRST_USER_TAG, -3);
        let bytes = model.to_compiled();
        let restored = Model::from_compiled(&bytes).unwrap();
        assert_eq!(restored.max_token_bytes(), model.max_token_bytes());
        assert_eq!(
            restored.tokenize("東京大学です"),
            model.tokenize("東京大学です")
        );
        assert_eq!(restored.to_compiled(), bytes);
    }

    #[test]
    fn streaming_loader_matches_regular_compiled_model() {
        let mut model = Model::new();
        model.set_max_unknown_chars(5);
        model
            .add_entry("東京", "東京", "トウキョウ", FIRST_USER_TAG, 100)
            .unwrap();
        model
            .add_entry("大学", "大学", "ダイガク", FIRST_USER_TAG, 100)
            .unwrap();
        model
            .add_entry(
                "東京大学",
                "東京大学",
                "トウキョウダイガク",
                FIRST_USER_TAG,
                20,
            )
            .unwrap();
        model.set_transition(FIRST_USER_TAG, FIRST_USER_TAG, -3);
        let bytes = model.to_compiled();
        let regular = Model::from_compiled(&bytes).unwrap();
        let direct = Model::from_compiled_for_stream(&bytes).unwrap();
        assert_eq!(direct.max_token_bytes(), regular.max_token_bytes());
        assert_eq!(
            direct.tokenize("東京大学です"),
            regular.tokenize("東京大学です")
        );
        let stats = direct.stats();
        assert_eq!(stats.builder_trie_bytes, 0);
        assert!(stats.runtime_trie_bytes > 0);
    }

    #[test]
    fn streaming_loader_can_thaw_for_followup_tsv_mutation() {
        let mut model = Model::new();
        model
            .add_entry("東京", "東京", "トウキョウ", FIRST_USER_TAG, 100)
            .unwrap();
        let bytes = model.to_compiled();
        let mut direct = Model::from_compiled_for_stream(&bytes).unwrap();
        direct
            .add_tsv_after_compiled_load("大学\t大学\tダイガク\t9\t100\n")
            .unwrap();
        assert_eq!(
            direct
                .tokenize("東京大学")
                .into_iter()
                .map(|token| token.surface.to_string())
                .collect::<Vec<_>>(),
            ["東京", "大学"]
        );
        assert!(direct.stats().builder_trie_bytes > 0);
    }

    #[test]
    fn compiled_reader_rejects_corruption() {
        let mut model = Model::new();
        model
            .add_entry("猫", "猫", "ネコ", FIRST_USER_TAG, 10)
            .unwrap();
        let mut bytes = model.to_compiled();
        bytes.truncate(bytes.len() - 1);
        assert!(Model::from_compiled(&bytes).is_err());
    }

    #[test]
    fn compiled_reader_rejects_impossible_counts_before_allocation() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"SMD1");
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes()); // entries
        bytes.extend_from_slice(&1u32.to_le_bytes()); // root node
        bytes.extend_from_slice(&u32::MAX.to_le_bytes()); // transitions
        let error = Model::from_compiled(&bytes).unwrap_err().to_string();
        assert!(error.contains("counts exceed input size") || error.contains("count overflow"));
    }
}
