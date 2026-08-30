#[path = "../src/search.rs"]
mod search;

use search::SearchTrie;

#[test]
fn trie_returns_all_case_insensitive_prefix_positions() {
    let mut trie = SearchTrie::default();
    trie.insert("Stream", 3);
    trie.insert("Streaming", 8);
    trie.insert("other", 13);
    assert_eq!(trie.search("STREAM"), &[3, 8]);
    assert!(trie.search("missing").is_empty());
    trie.clear();
    assert!(trie.search("stream").is_empty());
}

#[test]
fn suffix_entries_enable_partial_word_search() {
    let mut trie = SearchTrie::default();
    let word = "streamdown";
    for (offset, _) in word.char_indices() {
        trie.insert(&word[offset..], 20 + offset);
    }
    assert_eq!(trie.search("down"), &[26]);
}
