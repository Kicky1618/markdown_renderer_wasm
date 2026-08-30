use std::collections::HashMap;

struct Node<P> {
    children: HashMap<char, usize>,
    matches: Vec<P>,
}

impl<P> Default for Node<P> {
    fn default() -> Self {
        Self {
            children: HashMap::new(),
            matches: Vec::new(),
        }
    }
}

/// Case-insensitive prefix index. Every trie node owns the positions matching
/// that prefix, so lookup is O(query length + result count).
pub struct SearchTrie<P> {
    nodes: Vec<Node<P>>,
}

impl<P: Copy> Default for SearchTrie<P> {
    fn default() -> Self {
        Self {
            nodes: vec![Node::default()],
        }
    }
}

impl<P: Copy> SearchTrie<P> {
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.nodes.push(Node::default());
    }

    pub fn insert(&mut self, word: &str, position: P) {
        let mut node = 0;
        for character in word.chars().take(128).flat_map(char::to_lowercase) {
            let next = if let Some(next) = self.nodes[node].children.get(&character) {
                *next
            } else {
                let next = self.nodes.len();
                self.nodes.push(Node::default());
                self.nodes[node].children.insert(character, next);
                next
            };
            node = next;
            self.nodes[node].matches.push(position);
        }
    }

    pub fn search(&self, query: &str) -> &[P] {
        let mut node = 0;
        let mut saw_character = false;
        for character in query.trim().chars().flat_map(char::to_lowercase) {
            saw_character = true;
            let Some(next) = self.nodes[node].children.get(&character) else {
                return &[];
            };
            node = *next;
        }
        if saw_character {
            &self.nodes[node].matches
        } else {
            &[]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_case_insensitive_prefixes() {
        let mut trie = SearchTrie::default();
        trie.insert("Streaming", 2);
        trie.insert("Streamdown", 7);
        trie.insert("other", 9);
        assert_eq!(trie.search("STREAM"), &[2, 7]);
        assert!(trie.search("missing").is_empty());
    }
}
