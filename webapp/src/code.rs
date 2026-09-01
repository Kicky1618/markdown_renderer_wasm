#[path = "japanese.rs"]
mod japanese;
#[path = "languages.rs"]
mod languages;

use languages::{DeclarationKind, Language, normalize_fence_name};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Plain,
    Keyword,
    Type,
    Function,
    String,
    Number,
    Comment,
    Macro,
    Operator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SyntaxKind {
    Identifier,
    String,
    Number,
    Comment,
    Macro,
    AttributeStart,
    Operator,
    Open(u8),
    Close(u8),
    Punctuation,
    Trivia,
}

#[derive(Clone, Copy, Debug)]
struct Token {
    start: usize,
    end: usize,
    kind: SyntaxKind,
}

/// A small Tree-sitter-style scanner. It owns the lexical context (most
/// importantly whether `/` can start a JavaScript regexp) and yields byte
/// ranged tokens without allocating a token list.
const MAX_PENDING_SHELL_HEREDOCS: usize = 8;

#[derive(Clone, Copy, Debug, Default)]
struct ShellHeredoc {
    delimiter_start: usize,
    delimiter_end: usize,
    body_start: usize,
    strip_tabs: bool,
    quote_removal: bool,
}

struct Scanner<'a> {
    source: &'a str,
    language: Language,
    cpp_raw_strings: bool,
    cursor: usize,
    expression_expected: bool,
    preprocessor_header_expected: bool,
    pending_shell_heredocs: [ShellHeredoc; MAX_PENDING_SHELL_HEREDOCS],
    pending_shell_heredoc_count: usize,
}

impl<'a> Scanner<'a> {
    fn new(source: &'a str, language: Option<&str>) -> Self {
        let normalized = normalize_fence_name(language.unwrap_or(""));
        let fence = normalized.as_ref();
        let language = Language::from_fence(fence);
        let cpp_raw_strings = ["cpp", "c++", "cc", "cxx", "hpp", "cuda", "cu", "cuh", "mm"]
            .iter()
            .any(|alias| fence.eq_ignore_ascii_case(alias));
        Self {
            source,
            language,
            cpp_raw_strings,
            cursor: 0,
            expression_expected: true,
            preprocessor_header_expected: false,
            pending_shell_heredocs: [ShellHeredoc::default(); MAX_PENDING_SHELL_HEREDOCS],
            pending_shell_heredoc_count: 0,
        }
    }

    fn next(&mut self) -> Option<Token> {
        let bytes = self.source.as_bytes();
        let start = self.cursor;
        if start == bytes.len() {
            return None;
        }

        if let Some(heredoc) = self.pending_shell_heredoc() {
            if start >= heredoc.body_start {
                let end = shell_heredoc_body_end(self.source, start, heredoc);
                self.pop_shell_heredoc();
                self.cursor = end;
                let token = Token {
                    start,
                    end,
                    kind: SyntaxKind::String,
                };
                self.update_lexical_context(token);
                return Some(token);
            }
        }

        let (end, kind) = if self.preprocessor_header_expected && bytes[start] == b'<' {
            (header_string_end(self.source, start), SyntaxKind::String)
        } else if let Some(end) = lua_long_comment_end(self.source, start, self.language) {
            (end, SyntaxKind::Comment)
        } else if let Some(end) = cmake_bracket_comment_end(self.source, start, self.language) {
            (end, SyntaxKind::Comment)
        } else if let Some((opener, closer, nested)) =
            block_comment_delimiters(bytes, start, self.language)
        {
            (
                block_comment_end(self.source, start, opener, closer, nested),
                SyntaxKind::Comment,
            )
        } else if is_line_comment(bytes, start, self.language) {
            (
                self.source[start..]
                    .find('\n')
                    .map_or(bytes.len(), |offset| start + offset),
                SyntaxKind::Comment,
            )
        } else if let Some(end) = preprocessor_end(bytes, start, self.language) {
            (end, SyntaxKind::Macro)
        } else if let Some(end) =
            language_specific_string_end(self.source, start, self.language, self.cpp_raw_strings)
        {
            (end, SyntaxKind::String)
        } else if self.language.decorators() && bytes[start] == b'@' {
            let mut end = start + 1;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'_' | b'.'))
            {
                end += 1;
            }
            (end, SyntaxKind::Macro)
        } else if let Some(end) = rust_attribute_start(bytes, start, self.language) {
            (end, SyntaxKind::AttributeStart)
        } else if let Some(end) = macro_metavariable_end(bytes, start, self.language) {
            (end, SyntaxKind::Macro)
        } else if let Some(end) = lua_long_string_end(self.source, start, self.language) {
            (end, SyntaxKind::String)
        } else if let Some(end) = cmake_bracket_string_end(self.source, start, self.language) {
            (end, SyntaxKind::String)
        } else if let Some((quote_at, quote, triple)) =
            python_string_start(bytes, start, self.language)
        {
            (
                quoted_string_end(self.source, quote_at, quote, triple, true),
                SyntaxKind::String,
            )
        } else if ((bytes[start] == b'"' && self.language.triple_double_strings())
            || (bytes[start] == b'\'' && self.language.triple_single_strings()))
            && bytes
                .get(start..start + 3)
                .is_some_and(|delimiter| delimiter.iter().all(|byte| *byte == bytes[start]))
        {
            (
                quoted_string_end(self.source, start, bytes[start], true, true),
                SyntaxKind::String,
            )
        } else if self.language.javascript_lexing()
            && self.expression_expected
            && bytes[start] == b'/'
            && !matches!(bytes.get(start + 1), Some(b'/') | Some(b'*'))
        {
            (js_regex_end(self.source, start), SyntaxKind::String)
        } else if let Some(end) = verilog_number_end(bytes, start, self.language) {
            (end, SyntaxKind::Number)
        } else if let Some(end) = backtick_macro_end(bytes, start, self.language) {
            (end, SyntaxKind::Macro)
        } else if bytes[start] == b'\''
            && let Some(end) = apostrophe_operator_end(bytes, start, self.language)
        {
            (end, SyntaxKind::Operator)
        } else if matches!(bytes[start], b'\'' | b'"') {
            (
                quoted_string_end(
                    self.source,
                    start,
                    bytes[start],
                    false,
                    self.language.multiline_strings(),
                ),
                SyntaxKind::String,
            )
        } else if bytes[start] == b'`' {
            if self.language.backtick_strings() {
                (
                    quoted_string_end(self.source, start, b'`', false, true),
                    SyntaxKind::String,
                )
            } else if self.language.backtick_identifiers() {
                (
                    quoted_identifier_end(self.source, start),
                    SyntaxKind::Identifier,
                )
            } else if self.language.backtick_operators() {
                (start + 1, SyntaxKind::Operator)
            } else {
                (start + 1, SyntaxKind::Trivia)
            }
        } else if bytes[start].is_ascii_digit() {
            (
                number_end(bytes, start, self.language.apostrophe_digit_separators()),
                SyntaxKind::Number,
            )
        } else if is_ident_start(bytes[start], self.language) {
            let mut end = start + 1;
            while end < bytes.len() && is_ident_continue(bytes[end], self.language) {
                end += 1;
            }
            (end, SyntaxKind::Identifier)
        } else if is_operator(bytes[start]) {
            if let Some(heredoc) = shell_heredoc_start(self.source, start, self.language) {
                self.push_shell_heredoc(heredoc);
            }
            let mut end = start + 1;
            while end < bytes.len() && is_operator(bytes[end]) {
                end += 1;
            }
            (end, SyntaxKind::Operator)
        } else if matches!(bytes[start], b'(' | b'[' | b'{') {
            (start + 1, SyntaxKind::Open(bytes[start]))
        } else if matches!(bytes[start], b')' | b']' | b'}') {
            (start + 1, SyntaxKind::Close(bytes[start]))
        } else if matches!(bytes[start], b',' | b';') {
            (start + 1, SyntaxKind::Punctuation)
        } else {
            let body_start = self
                .pending_shell_heredoc()
                .map(|heredoc| heredoc.body_start)
                .filter(|body_start| *body_start > start);
            let mut end = start + self.source[start..].chars().next().unwrap().len_utf8();
            while end < bytes.len()
                && body_start.is_none_or(|body_start| end < body_start)
                && !starts_token(bytes, end, self.language)
            {
                end += self.source[end..].chars().next().unwrap().len_utf8();
            }
            if let Some(body_start) = body_start {
                end = end.min(body_start);
            }
            (end, SyntaxKind::Trivia)
        };

        self.cursor = end;
        let token = Token { start, end, kind };
        self.update_lexical_context(token);
        Some(token)
    }

    fn pending_shell_heredoc(&self) -> Option<ShellHeredoc> {
        (self.pending_shell_heredoc_count != 0).then_some(self.pending_shell_heredocs[0])
    }

    fn push_shell_heredoc(&mut self, heredoc: ShellHeredoc) {
        if self.pending_shell_heredoc_count < MAX_PENDING_SHELL_HEREDOCS {
            self.pending_shell_heredocs[self.pending_shell_heredoc_count] = heredoc;
            self.pending_shell_heredoc_count += 1;
        }
    }

    fn pop_shell_heredoc(&mut self) {
        if self.pending_shell_heredoc_count == 0 {
            return;
        }
        self.pending_shell_heredoc_count -= 1;
        self.pending_shell_heredocs
            .copy_within(1..=self.pending_shell_heredoc_count, 0);
    }

    fn update_lexical_context(&mut self, token: Token) {
        let text = &self.source[token.start..token.end];
        if token.kind == SyntaxKind::Macro {
            self.preprocessor_header_expected = self.language.header_after(text);
        } else if token.kind == SyntaxKind::Identifier
            && self.language.header_after_identifier(text)
        {
            self.preprocessor_header_expected = true;
        } else if self.preprocessor_header_expected && token.kind == SyntaxKind::Open(b'(') {
            // Function-like header probes use `__has_include(<header>)`.
        } else if !matches!(token.kind, SyntaxKind::Comment | SyntaxKind::Trivia) {
            self.preprocessor_header_expected = false;
        }
        self.expression_expected = match token.kind {
            SyntaxKind::Identifier => self.language.is_expression_prefix(text),
            SyntaxKind::Number | SyntaxKind::String | SyntaxKind::Close(_) => false,
            SyntaxKind::Operator
            | SyntaxKind::Open(_)
            | SyntaxKind::AttributeStart
            | SyntaxKind::Punctuation => true,
            SyntaxKind::Comment | SyntaxKind::Macro | SyntaxKind::Trivia => {
                self.expression_expected
            }
        };
    }
}

/// Compact parse stack corresponding to Tree-sitter's state stack. Delimiter
/// errors are recovered by popping to a matching ancestor, so incomplete LLM
/// output never prevents the remaining code from being highlighted.
struct ParseStack {
    delimiters: [u8; 64],
    depth: usize,
    expected_name: Option<DeclarationKind>,
    attribute_delimiter: Option<usize>,
    attribute_path: bool,
}

impl ParseStack {
    fn new() -> Self {
        Self {
            delimiters: [0; 64],
            depth: 0,
            expected_name: None,
            attribute_delimiter: None,
            attribute_path: false,
        }
    }

    fn shift(&mut self, kind: SyntaxKind) {
        match kind {
            SyntaxKind::AttributeStart if self.depth < self.delimiters.len() => {
                self.attribute_delimiter = Some(self.depth);
                self.attribute_path = true;
                self.delimiters[self.depth] = b'[';
                self.depth += 1;
            }
            SyntaxKind::Open(delimiter) if self.depth < self.delimiters.len() => {
                if delimiter == b'(' && self.attribute_delimiter.is_some() {
                    self.attribute_path = false;
                }
                self.delimiters[self.depth] = delimiter;
                self.depth += 1;
            }
            SyntaxKind::Close(delimiter) => {
                let wanted = match delimiter {
                    b')' => b'(',
                    b']' => b'[',
                    b'}' => b'{',
                    _ => return,
                };
                if let Some(found) = self.delimiters[..self.depth]
                    .iter()
                    .rposition(|candidate| *candidate == wanted)
                {
                    self.depth = found;
                    if self.attribute_delimiter == Some(found) {
                        self.attribute_delimiter = None;
                        self.attribute_path = false;
                    }
                }
            }
            _ => {}
        }
    }

    fn captures_attribute_macro(&self, source: &str, token: Token) -> bool {
        self.attribute_delimiter.is_some()
            && (self.attribute_path || next_significant_byte(source, token.end) == Some(b'('))
    }
}

/// Parse complete or still-streaming code with a custom scanner + pushdown
/// parser inspired by Tree-sitter's scan/shift/recover/capture pipeline.
/// Unterminated constructs are accepted as provisional tokens, so callers do
/// not select a separate fallback lexer while a fence is open. Returning
/// `false` stops parsing immediately after the visible range has been emitted.
pub fn highlight<'a>(
    source: &'a str,
    language: Option<&str>,
    mut emit: impl FnMut(&'a str, TokenKind) -> bool,
) {
    if japanese::is_fence(language) {
        japanese::highlight(source, emit);
        return;
    }

    let mut scanner = Scanner::new(source, language);
    let language = scanner.language;
    let mut stack = ParseStack::new();
    while let Some(token) = scanner.next() {
        let text = &source[token.start..token.end];
        let kind = capture_kind(source, token, language, &mut stack);
        stack.shift(token.kind);
        if !emit(text, kind) {
            return;
        }
    }
}

fn capture_kind(
    source: &str,
    token: Token,
    language: Language,
    stack: &mut ParseStack,
) -> TokenKind {
    let text = &source[token.start..token.end];
    match token.kind {
        SyntaxKind::String => TokenKind::String,
        SyntaxKind::Number => TokenKind::Number,
        SyntaxKind::Comment => TokenKind::Comment,
        SyntaxKind::Macro => {
            if language.macro_operand_after(text) {
                stack.expected_name = Some(DeclarationKind::Macro);
            }
            TokenKind::Macro
        }
        SyntaxKind::AttributeStart => TokenKind::Macro,
        SyntaxKind::Operator => TokenKind::Operator,
        SyntaxKind::Identifier => {
            let expected = stack.expected_name.take();
            if language.is_macro_identifier(text) {
                if language.macro_operand_after_identifier(text) {
                    stack.expected_name = Some(DeclarationKind::Macro);
                }
                return TokenKind::Macro;
            }
            if is_bang_macro(source, token, language) {
                if language.is_bang_macro_declaration(text) {
                    stack.expected_name = Some(DeclarationKind::Macro);
                }
                return TokenKind::Macro;
            }
            if expected == Some(DeclarationKind::Macro)
                || stack.captures_attribute_macro(source, token)
                || (language.uppercase_macros() && looks_like_macro_constant(text))
            {
                return TokenKind::Macro;
            }
            if language.is_builtin_type(text) {
                return TokenKind::Type;
            }
            if language.is_keyword(text) {
                stack.expected_name = language.declaration_after(text);
                return TokenKind::Keyword;
            }
            if expected == Some(DeclarationKind::Function)
                || next_significant_byte(source, token.end) == Some(b'(')
            {
                TokenKind::Function
            } else if expected == Some(DeclarationKind::Type) || looks_like_type(text) {
                TokenKind::Type
            } else {
                TokenKind::Plain
            }
        }
        SyntaxKind::Trivia => {
            if text.contains('\n') && stack.expected_name == Some(DeclarationKind::Macro) {
                stack.expected_name = None;
            }
            TokenKind::Plain
        }
        _ => TokenKind::Plain,
    }
}

/// Find a C-style block comment terminator. Rust comments nest, so for Rust we
/// mirror the external scanner behavior used by its Tree-sitter grammar.
fn block_comment_end(
    source: &str,
    start: usize,
    opener: &[u8],
    closer: &[u8],
    nested: bool,
) -> usize {
    let bytes = source.as_bytes();
    let mut cursor = start + opener.len();
    let mut depth = 1usize;
    while cursor < bytes.len() {
        if nested && bytes.get(cursor..cursor + opener.len()) == Some(opener) {
            depth += 1;
            cursor += opener.len();
        } else if bytes.get(cursor..cursor + closer.len()) == Some(closer) {
            depth -= 1;
            cursor += closer.len();
            if depth == 0 {
                return cursor;
            }
        } else {
            cursor += source[cursor..].chars().next().unwrap().len_utf8();
        }
    }
    bytes.len()
}

/// The lexer equivalent of Tree-sitter's lookahead table: stop a trivia span
/// whenever the next byte can begin a real token.
fn starts_token(bytes: &[u8], i: usize, language: Language) -> bool {
    is_special_start(bytes, i, language)
        || bytes[i].is_ascii_digit()
        || is_ident_start(bytes[i], language)
        || is_operator(bytes[i])
        || matches!(
            bytes[i],
            b'(' | b'[' | b'{' | b')' | b']' | b'}' | b',' | b';'
        )
}

fn next_significant_byte(source: &str, from: usize) -> Option<u8> {
    source.as_bytes()[from..]
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
}

fn next_significant_index(source: &str, from: usize) -> Option<usize> {
    source.as_bytes()[from..]
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map(|offset| from + offset)
}

fn is_bang_macro(source: &str, token: Token, language: Language) -> bool {
    if !language.bang_macros() {
        return false;
    }
    let Some(bang) = next_significant_index(source, token.end) else {
        return false;
    };
    if source.as_bytes()[bang] != b'!' {
        return false;
    }
    if language.is_bang_macro_declaration(&source[token.start..token.end]) {
        return true;
    }
    next_significant_index(source, bang + 1)
        .is_none_or(|next| matches!(source.as_bytes()[next], b'(' | b'[' | b'{'))
}

fn looks_like_macro_constant(word: &str) -> bool {
    let mut has_letter = false;
    word.bytes().all(|byte| {
        if byte.is_ascii_alphabetic() {
            has_letter = true;
            byte.is_ascii_uppercase()
        } else {
            byte.is_ascii_digit() || byte == b'_'
        }
    }) && has_letter
}

fn shell_heredoc_start(source: &str, start: usize, language: Language) -> Option<ShellHeredoc> {
    if !language.shell_heredocs() {
        return None;
    }
    let bytes = source.as_bytes();
    if bytes.get(start..start + 2) != Some(b"<<") || bytes.get(start + 2) == Some(&b'<') {
        return None;
    }

    let mut cursor = start + 2;
    let strip_tabs = bytes.get(cursor) == Some(&b'-');
    if strip_tabs {
        cursor += 1;
    }
    while matches!(bytes.get(cursor), Some(b' ' | b'\t')) {
        cursor += 1;
    }

    let delimiter_start = cursor;
    let (delimiter_end, quote_removal) = shell_heredoc_word_end(bytes, delimiter_start)?;
    if delimiter_end == delimiter_start {
        return None;
    }

    let newline = bytes
        .get(delimiter_end..)?
        .iter()
        .position(|byte| *byte == b'\n')?
        + delimiter_end;
    Some(ShellHeredoc {
        delimiter_start,
        delimiter_end,
        body_start: newline + 1,
        strip_tabs,
        quote_removal,
    })
}

/// Return the end of the shell word used as a here-doc delimiter. Quotes and
/// backslash escapes are kept in this range; `shell_heredoc_delimiter_matches`
/// performs quote removal lazily while comparing a candidate terminator line.
/// This keeps the scanner allocation-free even for long delimiters.
fn shell_heredoc_word_end(bytes: &[u8], start: usize) -> Option<(usize, bool)> {
    let mut cursor = start;
    let mut quote = None;
    let mut quote_removal = false;
    while let Some(&byte) = bytes.get(cursor) {
        match quote {
            Some(b'\'') => {
                cursor += 1;
                if byte == b'\'' {
                    quote = None;
                } else if byte == b'\n' {
                    return None;
                }
            }
            Some(b'"') => {
                cursor += 1;
                if byte == b'"' {
                    quote = None;
                } else if byte == b'\\' {
                    let escaped = *bytes.get(cursor)?;
                    cursor += 1;
                    if escaped == b'\n' {
                        quote_removal = true;
                    }
                } else if byte == b'\n' {
                    return None;
                }
            }
            None => match byte {
                b'\'' | b'"' => {
                    quote = Some(byte);
                    quote_removal = true;
                    cursor += 1;
                }
                b'\\' => {
                    quote_removal = true;
                    let _escaped = *bytes.get(cursor + 1)?;
                    cursor += 2;
                }
                b if b.is_ascii_whitespace()
                    || matches!(b, b';' | b'|' | b'&' | b'(' | b')' | b'<' | b'>') =>
                {
                    break;
                }
                _ => cursor += 1,
            },
            _ => unreachable!(),
        }
    }
    quote.is_none().then_some((cursor, quote_removal))
}

fn shell_heredoc_delimiter_matches(line: &[u8], word: &[u8]) -> bool {
    let mut line_cursor = 0;
    let mut word_cursor = 0;
    let mut quote = None;

    while word_cursor < word.len() {
        let byte = word[word_cursor];
        match quote {
            Some(b'\'') => {
                word_cursor += 1;
                if byte == b'\'' {
                    quote = None;
                } else if line.get(line_cursor) == Some(&byte) {
                    line_cursor += 1;
                } else {
                    return false;
                }
            }
            Some(b'"') => {
                word_cursor += 1;
                if byte == b'"' {
                    quote = None;
                } else if byte == b'\\' {
                    let Some(&escaped) = word.get(word_cursor) else {
                        return false;
                    };
                    if escaped == b'\n' {
                        word_cursor += 1;
                    } else if matches!(escaped, b'$' | b'`' | b'"' | b'\\') {
                        word_cursor += 1;
                        if line.get(line_cursor) != Some(&escaped) {
                            return false;
                        }
                        line_cursor += 1;
                    } else {
                        if line.get(line_cursor) != Some(&b'\\') {
                            return false;
                        }
                        line_cursor += 1;
                    }
                } else if line.get(line_cursor) == Some(&byte) {
                    line_cursor += 1;
                } else {
                    return false;
                }
            }
            None => match byte {
                b'\'' | b'"' => {
                    quote = Some(byte);
                    word_cursor += 1;
                }
                b'\\' => {
                    let Some(&escaped) = word.get(word_cursor + 1) else {
                        return false;
                    };
                    word_cursor += 2;
                    if escaped != b'\n' {
                        if line.get(line_cursor) != Some(&escaped) {
                            return false;
                        }
                        line_cursor += 1;
                    }
                }
                _ => {
                    word_cursor += 1;
                    if line.get(line_cursor) != Some(&byte) {
                        return false;
                    }
                    line_cursor += 1;
                }
            },
            _ => unreachable!(),
        }
    }

    quote.is_none() && line_cursor == line.len()
}

fn shell_heredoc_body_end(source: &str, start: usize, heredoc: ShellHeredoc) -> usize {
    let bytes = source.as_bytes();
    let delimiter_word = &bytes[heredoc.delimiter_start..heredoc.delimiter_end];
    let mut line_start = start;
    while line_start <= bytes.len() {
        let newline = bytes
            .get(line_start..)
            .and_then(|tail| tail.iter().position(|byte| *byte == b'\n'))
            .map(|offset| line_start + offset);
        let physical_end = newline.unwrap_or(bytes.len());
        let line_end = if physical_end > line_start && bytes[physical_end - 1] == b'\r' {
            physical_end - 1
        } else {
            physical_end
        };
        let mut compare_start = line_start;
        if heredoc.strip_tabs {
            while bytes.get(compare_start) == Some(&b'\t') && compare_start < line_end {
                compare_start += 1;
            }
        }
        let candidate = &bytes[compare_start..line_end];
        let matches = if heredoc.quote_removal {
            shell_heredoc_delimiter_matches(candidate, delimiter_word)
        } else {
            candidate == delimiter_word
        };
        if matches {
            return newline.map_or(line_end, |newline| newline + 1);
        }
        let Some(newline) = newline else {
            break;
        };
        line_start = newline + 1;
    }
    bytes.len()
}

fn block_comment_delimiters(
    bytes: &[u8],
    i: usize,
    language: Language,
) -> Option<(&'static [u8], &'static [u8], bool)> {
    let nested = language.nested_block_comments();
    if bytes.get(i..i + 2) == Some(b"/*") && language.block_comments() {
        Some((b"/*", b"*/", nested))
    } else if bytes.get(i..i + 2) == Some(b"(*") && language.paren_star_comments() {
        Some((b"(*", b"*)", nested))
    } else if bytes.get(i..i + 2) == Some(b"{-") && language.brace_dash_comments() {
        Some((b"{-", b"-}", nested))
    } else if bytes.get(i..i + 2) == Some(b"(;") && language.paren_semicolon_comments() {
        Some((b"(;", b";)", nested))
    } else if bytes.get(i) == Some(&b'{') && language.brace_comments() {
        Some((b"{", b"}", false))
    } else if bytes.get(i..i + 2) == Some(b"<#") && language.angle_hash_comments() {
        Some((b"<#", b"#>", false))
    } else if bytes.get(i..i + 2) == Some(b"#|") && language.hash_pipe_comments() {
        Some((b"#|", b"|#", true))
    } else if bytes.get(i..i + 2) == Some(b"#=") && language.hash_equals_comments() {
        Some((b"#=", b"=#", true))
    } else if bytes.get(i..i + 2) == Some(b"#[") && language.hash_bracket_comments() {
        Some((b"#[", b"]#", true))
    } else if bytes.get(i..i + 2) == Some(b"/+") && language.slash_plus_comments() {
        Some((b"/+", b"+/", true))
    } else {
        None
    }
}

fn is_line_comment(bytes: &[u8], i: usize, language: Language) -> bool {
    (bytes.get(i..i + 2) == Some(b";;") && language.double_semicolon_comments())
        || (bytes.get(i..i + 2) == Some(b"//") && language.slash_comments())
        || (bytes.get(i..i + 2) == Some(b"--") && language.dash_comments())
        || (bytes.get(i) == Some(&b'#') && language.hash_comments())
        || (bytes.get(i) == Some(&b';') && language.semicolon_comments())
        || (bytes.get(i) == Some(&b'%') && language.percent_comments())
        || (bytes.get(i) == Some(&b'\'') && language.apostrophe_comments())
        || (bytes.get(i) == Some(&b'!') && language.bang_comments())
}

fn is_special_start(bytes: &[u8], i: usize, language: Language) -> bool {
    is_line_comment(bytes, i, language)
        || block_comment_delimiters(bytes, i, language).is_some()
        || extended_string_can_start_in_trivia(bytes, i, language)
        || preprocessor_end(bytes, i, language).is_some()
        || (language.decorators() && bytes.get(i) == Some(&b'@'))
        || rust_attribute_start(bytes, i, language).is_some()
        || macro_metavariable_end(bytes, i, language).is_some()
        || rust_raw_string_end_bytes(bytes, i, language).is_some()
        || python_string_start(bytes, i, language).is_some()
        || matches!(bytes[i], b'\'' | b'"' | b'`')
        || is_operator(bytes[i])
}

fn is_ident_start(byte: u8, language: Language) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || (byte == b'$' && language.dollar_identifiers())
}

fn is_ident_continue(byte: u8, language: Language) -> bool {
    byte.is_ascii_alphanumeric()
        || byte == b'_'
        || (byte == b'$' && language.dollar_identifiers())
        || (byte == b'-' && language.hyphen_identifiers())
        || (byte == b'?' && language.question_identifiers())
        || (byte == b'!' && language.bang_identifiers())
        || (byte == b'\'' && language.apostrophe_identifiers())
}

fn is_operator(byte: u8) -> bool {
    matches!(
        byte,
        b'+' | b'-'
            | b'*'
            | b'/'
            | b'%'
            | b'='
            | b'!'
            | b'<'
            | b'>'
            | b'&'
            | b'|'
            | b'^'
            | b'~'
            | b'?'
            | b':'
    )
}

fn preprocessor_end(bytes: &[u8], i: usize, language: Language) -> Option<usize> {
    if !language.preprocessor() || bytes.get(i) != Some(&b'#') {
        return None;
    }
    if !bytes[..i]
        .iter()
        .rev()
        .take_while(|byte| **byte != b'\n')
        .all(u8::is_ascii_whitespace)
    {
        return None;
    }
    let mut cursor = i + 1;
    while bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_whitespace() && *byte != b'\n')
    {
        cursor += 1;
    }
    while bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        cursor += 1;
    }
    Some(cursor.max(i + 1))
}

fn rust_attribute_start(bytes: &[u8], i: usize, language: Language) -> Option<usize> {
    if !language.rust_attributes() || bytes.get(i) != Some(&b'#') {
        return None;
    }
    if bytes.get(i..i + 2) == Some(b"#[") {
        Some(i + 2)
    } else if bytes.get(i..i + 3) == Some(b"#![") {
        Some(i + 3)
    } else {
        None
    }
}

fn macro_metavariable_end(bytes: &[u8], i: usize, language: Language) -> Option<usize> {
    if !language.macro_metavariables() || bytes.get(i) != Some(&b'$') {
        return None;
    }
    let mut cursor = i + 1;
    if bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
    {
        cursor += 1;
        while bytes
            .get(cursor)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            cursor += 1;
        }
    }
    Some(cursor)
}

fn header_string_end(source: &str, start: usize) -> usize {
    source.as_bytes()[start + 1..]
        .iter()
        .position(|byte| matches!(byte, b'>' | b'\n'))
        .map_or(source.len(), |offset| {
            let end = start + 1 + offset;
            end + usize::from(source.as_bytes()[end] == b'>')
        })
}

fn looks_like_type(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next().is_some_and(char::is_uppercase) && chars.any(char::is_lowercase)
}

fn is_rust_lifetime(bytes: &[u8], i: usize, language: Language) -> bool {
    if !language.rust_syntax() || bytes.get(i) != Some(&b'\'') {
        return false;
    }
    let Some(&next) = bytes.get(i + 1) else {
        return false;
    };
    if !(next.is_ascii_alphabetic() || next == b'_') {
        return false;
    }
    let mut end = i + 2;
    while bytes
        .get(end)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        end += 1;
    }
    bytes.get(end) != Some(&b'\'')
}

#[inline]
fn csharp_interpolated_raw_quote_start(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'$') {
        return None;
    }
    let mut quote_start = start;
    while bytes.get(quote_start) == Some(&b'$') {
        quote_start += 1;
    }
    let mut quote_end = quote_start;
    while bytes.get(quote_end) == Some(&b'"') {
        quote_end += 1;
    }
    (quote_end - quote_start >= 3).then_some(quote_start)
}

#[inline]
fn extended_string_can_start_in_trivia(bytes: &[u8], start: usize, language: Language) -> bool {
    let tail = bytes.get(start..).unwrap_or_default();
    match bytes.get(start).copied() {
        Some(b'$') => {
            (language.csharp_prefixed_strings()
                && (tail.starts_with(b"$\"")
                    || tail.starts_with(b"$@\"")
                    || csharp_interpolated_raw_quote_start(bytes, start).is_some()))
                || (language.fsharp_interpolated_strings()
                    && (tail.starts_with(b"$\"") || tail.starts_with(b"$@\"")))
                || (language.shell_prefixed_strings()
                    && (tail.starts_with(b"$'") || tail.starts_with(b"$\"")))
                || (language.dollar_quoted_strings()
                    && dollar_quote_delimiter_end(bytes, start).is_some())
                || (language.groovy_dollar_slashy() && tail.starts_with(b"$/"))
        }
        Some(b'@') => {
            (language.csharp_prefixed_strings()
                && (tail.starts_with(b"@\"") || tail.starts_with(b"@$\"")))
                || (language.objective_c_strings() && tail.starts_with(b"@\""))
                || (language.powershell_here_strings()
                    && (tail.starts_with(b"@\"\n")
                        || tail.starts_with(b"@\"\r\n")
                        || tail.starts_with(b"@'\n")
                        || tail.starts_with(b"@'\r\n")))
        }
        Some(b'#') => {
            language.swift_hash_raw_strings() && hash_raw_string_start(bytes, start).is_some()
        }
        _ => false,
    }
}

fn dollar_quote_delimiter_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'$') {
        return None;
    }
    if start > 0 && bytes[start - 1].is_ascii_alphanumeric()
        || (start > 0 && matches!(bytes[start - 1], b'_' | b'$'))
    {
        return None;
    }
    let mut cursor = start + 1;
    if bytes.get(cursor) == Some(&b'$') {
        return Some(cursor + 1);
    }
    if !bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
    {
        return None;
    }
    cursor += 1;
    while bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        cursor += 1;
    }
    (bytes.get(cursor) == Some(&b'$')).then_some(cursor + 1)
}

fn dollar_quoted_string_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let delimiter_end = dollar_quote_delimiter_end(bytes, start)?;
    let delimiter = &source[start..delimiter_end];
    source[delimiter_end..]
        .find(delimiter)
        .map_or(Some(source.len()), |offset| {
            Some(delimiter_end + offset + delimiter.len())
        })
}

fn shell_prefixed_string_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let quote = match bytes.get(start..start + 2) {
        Some(b"$'") => b'\'',
        Some(b"$\"") => b'"',
        _ => return None,
    };
    Some(quoted_string_end(source, start + 1, quote, false, true))
}

fn ocaml_quoted_string_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(start) != Some(&b'{') {
        return None;
    }
    let tag_start = start + 1;
    let mut open_bar = tag_start;
    while bytes
        .get(open_bar)
        .is_some_and(|byte| byte.is_ascii_lowercase() || *byte == b'_')
    {
        open_bar += 1;
    }
    if bytes.get(open_bar) != Some(&b'|') {
        return None;
    }
    let tag = &bytes[tag_start..open_bar];
    let mut cursor = open_bar + 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'|' {
            let tag_end = cursor + 1 + tag.len();
            if bytes.get(cursor + 1..tag_end) == Some(tag) && bytes.get(tag_end) == Some(&b'}') {
                return Some(tag_end + 1);
            }
        }
        cursor += source[cursor..].chars().next().unwrap().len_utf8();
    }
    Some(bytes.len())
}

fn delimiter_pair(open: u8) -> (u8, bool) {
    match open {
        b'(' => (b')', true),
        b'[' => (b']', true),
        b'{' => (b'}', true),
        b'<' => (b'>', true),
        _ => (open, false),
    }
}

fn escaped_delimited_literal_end(
    source: &str,
    delimiter_at: usize,
    close: u8,
    nested: bool,
) -> usize {
    let bytes = source.as_bytes();
    let open = bytes[delimiter_at];
    let mut cursor = delimiter_at + 1;
    let mut depth = 1usize;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\\' && open != b'\\' {
            cursor = (cursor + 2).min(bytes.len());
        } else if nested && bytes[cursor] == open {
            depth += 1;
            cursor += 1;
        } else if bytes[cursor] == close {
            depth -= 1;
            cursor += 1;
            if depth == 0 {
                return cursor;
            }
        } else {
            cursor += source[cursor..].chars().next().unwrap().len_utf8();
        }
    }
    bytes.len()
}

fn ruby_percent_literal_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(start) != Some(&b'%') {
        return None;
    }
    let kind = *bytes.get(start + 1)?;
    if !matches!(
        kind,
        b'q' | b'Q' | b'w' | b'W' | b'i' | b'I' | b'r' | b's' | b'x'
    ) {
        return None;
    }
    let delimiter_at = start + 2;
    let open = *bytes.get(delimiter_at)?;
    if open.is_ascii_alphanumeric() || open.is_ascii_whitespace() {
        return None;
    }
    let (close, nested) = delimiter_pair(open);
    let mut end = escaped_delimited_literal_end(source, delimiter_at, close, nested);
    if kind == b'r' && end < bytes.len() {
        while bytes.get(end).is_some_and(u8::is_ascii_alphabetic) {
            end += 1;
        }
    }
    Some(end)
}

fn elixir_sigil_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(start) != Some(&b'~') {
        return None;
    }
    let first = *bytes.get(start + 1)?;
    let mut delimiter_at = start + 2;
    if first.is_ascii_lowercase() {
        // Lowercase custom sigils are exactly one character.
    } else if first.is_ascii_uppercase() {
        while bytes
            .get(delimiter_at)
            .is_some_and(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            delimiter_at += 1;
        }
    } else {
        return None;
    }

    if let Some(&quote @ (b'\'' | b'"')) = bytes.get(delimiter_at)
        && bytes.get(delimiter_at..delimiter_at + 3) == Some(&[quote, quote, quote])
    {
        let mut end = quoted_string_end(source, delimiter_at, quote, true, true);
        while bytes
            .get(end)
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        {
            end += 1;
        }
        return Some(end);
    }

    let open = *bytes.get(delimiter_at)?;
    if !matches!(open, b'/' | b'|' | b'"' | b'\'' | b'(' | b'[' | b'{' | b'<') {
        return None;
    }
    let (close, nested) = delimiter_pair(open);
    let mut end = escaped_delimited_literal_end(source, delimiter_at, close, nested);
    while bytes
        .get(end)
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
    {
        end += 1;
    }
    Some(end)
}

fn php_heredoc_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(start..start + 3) != Some(b"<<<") {
        return None;
    }
    let mut cursor = start + 3;
    let quote = match bytes.get(cursor) {
        Some(b'\'' | b'"') => {
            let quote = bytes[cursor];
            cursor += 1;
            Some(quote)
        }
        _ => None,
    };
    let label_start = cursor;
    if !bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
    {
        return None;
    }
    cursor += 1;
    while bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        cursor += 1;
    }
    let label_end = cursor;
    if let Some(quote) = quote {
        if bytes.get(cursor) != Some(&quote) {
            return None;
        }
        cursor += 1;
    }
    cursor = match bytes.get(cursor..) {
        Some(tail) if tail.starts_with(b"\r\n") => cursor + 2,
        Some(tail) if tail.starts_with(b"\n") => cursor + 1,
        _ => return None,
    };
    let label = &bytes[label_start..label_end];

    while cursor <= bytes.len() {
        let line_end = bytes
            .get(cursor..)
            .and_then(|tail| tail.iter().position(|byte| *byte == b'\n'))
            .map_or(bytes.len(), |offset| cursor + offset);
        let mut marker = cursor;
        while marker < line_end && matches!(bytes[marker], b' ' | b'\t') {
            marker += 1;
        }
        if bytes.get(marker..marker + label.len()) == Some(label) {
            return Some(marker + label.len());
        }
        if line_end == bytes.len() {
            break;
        }
        cursor = line_end + 1;
    }
    Some(bytes.len())
}

fn r_raw_string_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if !matches!(bytes.get(start), Some(b'r' | b'R')) || bytes.get(start + 1) != Some(&b'"') {
        return None;
    }
    let mut open_at = start + 2;
    while bytes.get(open_at) == Some(&b'-') {
        open_at += 1;
    }
    let dashes = open_at - (start + 2);
    let open = *bytes.get(open_at)?;
    let close = match open {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        _ => return None,
    };
    let mut cursor = open_at + 1;
    while cursor < bytes.len() {
        if bytes[cursor] == close {
            let dash_end = cursor + 1 + dashes;
            if bytes
                .get(cursor + 1..dash_end)
                .is_some_and(|tail| tail.iter().all(|byte| *byte == b'-'))
                && bytes.get(dash_end) == Some(&b'"')
            {
                return Some(dash_end + 1);
            }
        }
        cursor += source[cursor..].chars().next().unwrap().len_utf8();
    }
    Some(bytes.len())
}

fn haskell_quasiquote_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(start) != Some(&b'[') {
        return None;
    }
    let mut cursor = start + 1;
    if !bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
    {
        return None;
    }
    cursor += 1;
    while bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'\'' | b'.'))
    {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'|') {
        return None;
    }
    let quoter = &source[start + 1..cursor];
    if matches!(quoter, "e" | "t" | "d" | "p") {
        return None;
    }
    let content_start = cursor + 1;
    source[content_start..]
        .find("|]")
        .map_or(Some(source.len()), |offset| {
            Some(content_start + offset + 2)
        })
}

fn groovy_dollar_slashy_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(start..start + 2) != Some(b"$/") {
        return None;
    }
    let mut cursor = start + 2;
    while cursor < bytes.len() {
        if bytes[cursor] == b'$' && matches!(bytes.get(cursor + 1), Some(b'$' | b'/')) {
            cursor = (cursor + 2).min(bytes.len());
        } else if bytes.get(cursor..cursor + 2) == Some(b"/$") {
            return Some(cursor + 2);
        } else {
            cursor += source[cursor..].chars().next().unwrap().len_utf8();
        }
    }
    Some(bytes.len())
}

fn csharp_raw_string_end(source: &str, quote_start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(quote_start) != Some(&b'"') {
        return None;
    }
    let mut cursor = quote_start;
    while bytes.get(cursor) == Some(&b'"') {
        cursor += 1;
    }
    let quotes = cursor - quote_start;
    if quotes < 3 {
        return None;
    }
    while cursor < bytes.len() {
        if bytes
            .get(cursor..cursor + quotes)
            .is_some_and(|run| run.iter().all(|byte| *byte == b'"'))
        {
            return Some(cursor + quotes);
        }
        cursor += source[cursor..].chars().next().unwrap().len_utf8();
    }
    Some(bytes.len())
}

fn csharp_interpolated_raw_string_end(source: &str, start: usize) -> Option<usize> {
    let quote_start = csharp_interpolated_raw_quote_start(source.as_bytes(), start)?;
    csharp_raw_string_end(source, quote_start)
}

fn csharp_prefixed_string_end(source: &str, start: usize) -> Option<usize> {
    let tail = source.as_bytes().get(start..)?;
    if let Some(end) = csharp_interpolated_raw_string_end(source, start) {
        return Some(end);
    }
    if tail.starts_with(b"$@\"") || tail.starts_with(b"@$\"") {
        return Some(verbatim_quoted_string_end(source, start + 2, b'"'));
    }
    if tail.starts_with(b"@\"") {
        return Some(verbatim_quoted_string_end(source, start + 1, b'"'));
    }
    if tail.starts_with(b"$\"") {
        return Some(quoted_string_end(source, start + 1, b'"', false, false));
    }
    None
}

fn fsharp_interpolated_string_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let tail = bytes.get(start..)?;
    if tail.starts_with(b"$@\"") {
        return Some(verbatim_quoted_string_end(source, start + 2, b'"'));
    }
    if !tail.starts_with(b"$\"") {
        return None;
    }
    let quote_at = start + 1;
    let triple = bytes.get(quote_at..quote_at + 3) == Some(b"\"\"\"");
    Some(quoted_string_end(source, quote_at, b'"', triple, triple))
}

fn simple_prefixed_string_end(source: &str, start: usize, language: Language) -> Option<usize> {
    let bytes = source.as_bytes();
    if language.rust_syntax() {
        let (quote_at, quote) = match bytes.get(start..start + 2) {
            Some(b"b\"") => (start + 1, b'"'),
            Some(b"b'") => (start + 1, b'\''),
            Some(b"c\"") => (start + 1, b'"'),
            _ => (usize::MAX, 0),
        };
        if quote_at != usize::MAX {
            return Some(quoted_string_end(source, quote_at, quote, false, false));
        }
    }
    if language.c_family_prefixed_strings() {
        let quote_at = if matches!(bytes.get(start..start + 3), Some(b"u8\"") | Some(b"u8'")) {
            start + 2
        } else if matches!(bytes.get(start), Some(b'u' | b'U' | b'L'))
            && matches!(bytes.get(start + 1), Some(b'\'' | b'"'))
        {
            start + 1
        } else {
            return None;
        };
        return Some(quoted_string_end(
            source,
            quote_at,
            bytes[quote_at],
            false,
            false,
        ));
    }
    None
}

fn language_specific_string_end(
    source: &str,
    start: usize,
    language: Language,
    cpp_raw_strings: bool,
) -> Option<usize> {
    let bytes = source.as_bytes();
    match bytes.get(start).copied()? {
        b'$' => {
            if language.groovy_dollar_slashy() && bytes.get(start..start + 2) == Some(b"$/") {
                return groovy_dollar_slashy_end(source, start);
            }
            if language.dollar_quoted_strings()
                && let Some(end) = dollar_quoted_string_end(source, start)
            {
                return Some(end);
            }
            if language.shell_prefixed_strings()
                && let Some(end) = shell_prefixed_string_end(source, start)
            {
                return Some(end);
            }
            if language.csharp_prefixed_strings()
                && let Some(end) = csharp_prefixed_string_end(source, start)
            {
                return Some(end);
            }
            if language.fsharp_interpolated_strings()
                && let Some(end) = fsharp_interpolated_string_end(source, start)
            {
                return Some(end);
            }
        }
        b'@' => {
            if language.powershell_here_strings()
                && let Some(end) = powershell_here_string_end(source, start, language)
            {
                return Some(end);
            }
            if language.csharp_prefixed_strings()
                && let Some(end) = csharp_prefixed_string_end(source, start)
            {
                return Some(end);
            }
            if language.objective_c_strings() && bytes.get(start..start + 2) == Some(b"@\"") {
                return Some(quoted_string_end(source, start + 1, b'"', false, false));
            }
        }
        b'#' if language.swift_hash_raw_strings() => {
            return hash_raw_string_end(source, start);
        }
        b'"' if language.csharp_prefixed_strings() => {
            return csharp_raw_string_end(source, start);
        }
        b'%' if language.ruby_percent_literals() => {
            return ruby_percent_literal_end(source, start);
        }
        b'~' if language.elixir_sigils() => {
            return elixir_sigil_end(source, start);
        }
        b'<' if language.php_heredocs() => {
            return php_heredoc_end(source, start);
        }
        b'{' if language.ocaml_quoted_strings() => {
            return ocaml_quoted_string_end(source, start);
        }
        b'[' if language.haskell_quasiquotes() => {
            return haskell_quasiquote_end(source, start);
        }
        b'r' | b'R' => {
            if language.r_raw_strings()
                && let Some(end) = r_raw_string_end(source, start)
            {
                return Some(end);
            }
            if let Some(end) = rust_raw_string_end(source, start, language) {
                return Some(end);
            }
            if cpp_raw_strings && cpp_raw_string_start(bytes, start).is_some() {
                return cpp_raw_string_end(source, start);
            }
        }
        b'b' | b'c' => {
            if let Some(end) = rust_raw_string_end(source, start, language) {
                return Some(end);
            }
            if let Some(end) = simple_prefixed_string_end(source, start, language) {
                return Some(end);
            }
        }
        b'u' | b'U' | b'L' => {
            if cpp_raw_strings && cpp_raw_string_start(bytes, start).is_some() {
                return cpp_raw_string_end(source, start);
            }
            if let Some(end) = simple_prefixed_string_end(source, start, language) {
                return Some(end);
            }
        }
        _ => {}
    }
    None
}

fn quoted_identifier_end(source: &str, start: usize) -> usize {
    let bytes = source.as_bytes();
    let mut cursor = start + 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor = (cursor + 2).min(bytes.len()),
            b'`' if bytes.get(cursor + 1) == Some(&b'`') => cursor += 2,
            b'`' => return cursor + 1,
            b'\n' => return cursor,
            _ => cursor += source[cursor..].chars().next().unwrap().len_utf8(),
        }
    }
    cursor
}

fn apostrophe_operator_end(bytes: &[u8], start: usize, language: Language) -> Option<usize> {
    if bytes.get(start) != Some(&b'\'') {
        return None;
    }
    if language.rust_syntax() && is_rust_lifetime(bytes, start, language) {
        return Some(start + 1);
    }
    if language.verilog_numbers() && matches!(bytes.get(start + 1), Some(b'(' | b'{')) {
        return Some(start + 1);
    }
    if language.postfix_apostrophe_operators()
        && bytes[..start]
            .iter()
            .rev()
            .copied()
            .find(|byte| !byte.is_ascii_whitespace())
            .is_some_and(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b')' | b']' | b'}' | b'.')
            })
    {
        return Some(start + 1);
    }
    if !language.apostrophe_names() {
        return None;
    }
    let mut cursor = start + 1;
    if bytes.get(cursor) == Some(&b'\'') {
        cursor += 1;
    }
    if !bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
    {
        return None;
    }
    cursor += 1;
    while bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        cursor += 1;
    }
    (bytes.get(cursor) != Some(&b'\'')).then_some(if bytes.get(start + 1) == Some(&b'\'') {
        start + 2
    } else {
        start + 1
    })
}

fn powershell_here_string_end(source: &str, start: usize, language: Language) -> Option<usize> {
    if !language.powershell_here_strings() {
        return None;
    }
    let bytes = source.as_bytes();
    let quote = match bytes.get(start..start + 2) {
        Some(b"@\"") => b'"',
        Some(b"@'") => b'\'',
        _ => return None,
    };
    let content_start = match bytes.get(start + 2..) {
        Some(tail) if tail.starts_with(b"\r\n") => start + 4,
        Some(tail) if tail.starts_with(b"\n") => start + 3,
        _ => return None,
    };

    let mut line_start = content_start;
    while line_start <= bytes.len() {
        if bytes.get(line_start) == Some(&quote) && bytes.get(line_start + 1) == Some(&b'@') {
            let end = line_start + 2;
            if end == bytes.len()
                || bytes.get(end) == Some(&b'\n')
                || bytes.get(end..end + 2) == Some(b"\r\n")
            {
                return Some(end);
            }
        }
        let Some(relative_newline) = bytes
            .get(line_start..)?
            .iter()
            .position(|byte| *byte == b'\n')
        else {
            break;
        };
        line_start += relative_newline + 1;
    }
    Some(bytes.len())
}

fn verbatim_quoted_string_end(source: &str, quote_at: usize, quote: u8) -> usize {
    let bytes = source.as_bytes();
    let mut i = quote_at + 1;
    while i < bytes.len() {
        if bytes[i] == quote && bytes.get(i + 1) == Some(&quote) {
            i += 2;
        } else if bytes[i] == quote {
            return i + 1;
        } else {
            i += source[i..].chars().next().unwrap().len_utf8();
        }
    }
    bytes.len()
}

#[derive(Clone, Copy)]
struct CppRawStart {
    quote_at: usize,
    open_paren: usize,
}

fn cpp_raw_string_start(bytes: &[u8], start: usize) -> Option<CppRawStart> {
    let quote_at = if bytes.get(start..start + 2) == Some(b"R\"") {
        start + 1
    } else if bytes.get(start..start + 4) == Some(b"u8R\"") {
        start + 3
    } else if matches!(bytes.get(start), Some(b'u' | b'U' | b'L'))
        && bytes.get(start + 1..start + 3) == Some(b"R\"")
    {
        start + 2
    } else {
        return None;
    };

    let delimiter_start = quote_at + 1;
    let mut cursor = delimiter_start;
    while cursor < bytes.len() && cursor - delimiter_start <= 16 {
        match bytes[cursor] {
            b'(' => {
                return Some(CppRawStart {
                    quote_at,
                    open_paren: cursor,
                });
            }
            b' ' | b'\\' | b')' | b'\t' | b'\r' | b'\n' => return None,
            byte if byte.is_ascii_control() => return None,
            _ => cursor += 1,
        }
    }
    None
}

fn cpp_raw_string_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let raw = cpp_raw_string_start(bytes, start)?;
    let delimiter = &bytes[raw.quote_at + 1..raw.open_paren];
    let mut cursor = raw.open_paren + 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b')' {
            let after_delimiter = cursor + 1 + delimiter.len();
            if bytes.get(cursor + 1..after_delimiter) == Some(delimiter)
                && bytes.get(after_delimiter) == Some(&b'"')
            {
                return Some(after_delimiter + 1);
            }
        }
        cursor += source[cursor..].chars().next().unwrap().len_utf8();
    }
    Some(bytes.len())
}

#[derive(Clone, Copy)]
struct HashRawStart {
    hashes: usize,
    quote_at: usize,
    quote_len: usize,
}

fn hash_raw_string_start(bytes: &[u8], start: usize) -> Option<HashRawStart> {
    if bytes.get(start) != Some(&b'#') {
        return None;
    }
    let mut quote_at = start;
    while bytes.get(quote_at) == Some(&b'#') {
        quote_at += 1;
    }
    let hashes = quote_at - start;
    if bytes.get(quote_at) != Some(&b'"') {
        return None;
    }
    let quote_len = if bytes.get(quote_at..quote_at + 3) == Some(b"\"\"\"") {
        3
    } else {
        1
    };
    Some(HashRawStart {
        hashes,
        quote_at,
        quote_len,
    })
}

fn hash_raw_string_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let raw = hash_raw_string_start(bytes, start)?;
    let mut cursor = raw.quote_at + raw.quote_len;
    while cursor < bytes.len() {
        let quote_end = cursor + raw.quote_len;
        let hash_end = quote_end + raw.hashes;
        if bytes
            .get(cursor..quote_end)
            .is_some_and(|quotes| quotes.iter().all(|byte| *byte == b'"'))
            && bytes
                .get(quote_end..hash_end)
                .is_some_and(|hashes| hashes.iter().all(|byte| *byte == b'#'))
        {
            return Some(hash_end);
        }
        cursor += source[cursor..].chars().next().unwrap().len_utf8();
    }
    Some(bytes.len())
}

#[inline]
fn equals_bracket_open(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    if bytes.get(start) != Some(&b'[') {
        return None;
    }
    let mut cursor = start + 1;
    while bytes.get(cursor) == Some(&b'=') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'[') {
        return None;
    }
    Some((cursor - start - 1, cursor + 1))
}

fn equals_bracket_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let (equals, mut cursor) = equals_bracket_open(bytes, start)?;
    while cursor < bytes.len() {
        if bytes[cursor] == b']' {
            let equals_end = cursor + 1 + equals;
            if bytes
                .get(cursor + 1..equals_end)
                .is_some_and(|tail| tail.iter().all(|byte| *byte == b'='))
                && bytes.get(equals_end) == Some(&b']')
            {
                return Some(equals_end + 1);
            }
        }
        cursor += source[cursor..].chars().next().unwrap().len_utf8();
    }
    Some(bytes.len())
}

#[inline]
fn lua_long_comment_end(source: &str, start: usize, language: Language) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(start..start + 3) != Some(b"--[") || !language.lua_long_brackets() {
        return None;
    }
    equals_bracket_end(source, start + 2)
}

#[inline]
fn lua_long_string_end(source: &str, start: usize, language: Language) -> Option<usize> {
    if source.as_bytes().get(start) != Some(&b'[') || !language.lua_long_brackets() {
        return None;
    }
    equals_bracket_end(source, start)
}

#[inline]
fn cmake_bracket_comment_end(source: &str, start: usize, language: Language) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(start) != Some(&b'#') || !language.cmake_brackets() {
        return None;
    }
    equals_bracket_end(source, start + 1)
}

#[inline]
fn cmake_bracket_string_end(source: &str, start: usize, language: Language) -> Option<usize> {
    if source.as_bytes().get(start) != Some(&b'[') || !language.cmake_brackets() {
        return None;
    }
    equals_bracket_end(source, start)
}

fn quoted_string_end(
    source: &str,
    quote_at: usize,
    quote: u8,
    triple: bool,
    multiline: bool,
) -> usize {
    let bytes = source.as_bytes();
    let delimiter_len = if triple { 3 } else { 1 };
    let mut i = quote_at + delimiter_len;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i = (i + 2).min(bytes.len());
        } else if triple && bytes.get(i..i + 3) == Some(&[quote, quote, quote]) {
            return i + 3;
        } else if !triple && bytes[i] == quote {
            return i + 1;
        } else if !multiline && bytes[i] == b'\n' {
            return i;
        } else {
            i += source[i..].chars().next().unwrap().len_utf8();
        }
    }
    i
}

fn python_string_start(bytes: &[u8], i: usize, language: Language) -> Option<(usize, u8, bool)> {
    if !language.python_strings() {
        return None;
    }
    if matches!(bytes.get(i), Some(b'\'') | Some(b'"')) {
        let quote = bytes[i];
        let triple = bytes.get(i..i + 3) == Some(&[quote, quote, quote]);
        return triple.then_some((i, quote, true));
    }
    let mut quote_at = i;
    while quote_at < bytes.len()
        && quote_at - i < 2
        && matches!(
            bytes[quote_at].to_ascii_lowercase(),
            b'r' | b'u' | b'b' | b'f'
        )
    {
        quote_at += 1;
    }
    if quote_at == i || !matches!(bytes.get(quote_at), Some(b'\'') | Some(b'"')) {
        return None;
    }
    let quote = bytes[quote_at];
    let triple = bytes.get(quote_at..quote_at + 3) == Some(&[quote, quote, quote]);
    Some((quote_at, quote, triple))
}

fn rust_raw_string_end_bytes(bytes: &[u8], i: usize, language: Language) -> Option<usize> {
    if !language.rust_syntax() {
        return None;
    }
    let mut cursor = i;
    if matches!(bytes.get(cursor), Some(b'b' | b'c')) {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'r') {
        return None;
    }
    cursor += 1;
    let hashes = cursor;
    while bytes.get(cursor) == Some(&b'#') {
        cursor += 1;
    }
    (bytes.get(cursor) == Some(&b'"')).then_some(cursor - hashes)
}

fn rust_raw_string_end(source: &str, i: usize, language: Language) -> Option<usize> {
    let bytes = source.as_bytes();
    let hashes = rust_raw_string_end_bytes(bytes, i, language)?;
    let quote = i + usize::from(matches!(bytes.get(i), Some(b'b' | b'c'))) + 1 + hashes;
    let mut cursor = quote + 1;
    while cursor < bytes.len() {
        if bytes[cursor] == b'"'
            && bytes
                .get(cursor + 1..cursor + 1 + hashes)
                .is_some_and(|tail| tail.iter().all(|b| *b == b'#'))
        {
            return Some(cursor + 1 + hashes);
        }
        cursor += source[cursor..].chars().next().unwrap().len_utf8();
    }
    Some(bytes.len())
}

fn js_regex_end(source: &str, start: usize) -> usize {
    let bytes = source.as_bytes();
    let mut i = start + 1;
    let mut in_class = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i = (i + 2).min(bytes.len()),
            b'[' => {
                in_class = true;
                i += 1;
            }
            b']' => {
                in_class = false;
                i += 1;
            }
            b'/' if !in_class => {
                i += 1;
                while bytes.get(i).is_some_and(u8::is_ascii_alphabetic) {
                    i += 1;
                }
                return i;
            }
            b'\n' => return i,
            _ => i += 1,
        }
    }
    i
}

fn backtick_macro_end(bytes: &[u8], start: usize, language: Language) -> Option<usize> {
    if !language.backtick_macros() || bytes.get(start) != Some(&b'`') {
        return None;
    }
    if matches!(bytes.get(start + 1), Some(b'`' | b'"')) {
        return Some(start + 2);
    }
    let mut cursor = start + 1;
    if !bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
    {
        return None;
    }
    cursor += 1;
    while bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'$'))
    {
        cursor += 1;
    }
    Some(cursor)
}

fn verilog_number_end(bytes: &[u8], start: usize, language: Language) -> Option<usize> {
    if !language.verilog_numbers() {
        return None;
    }
    let mut cursor = start;
    if bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        while bytes
            .get(cursor)
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_')
        {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'\'') {
            return None;
        }
    } else if bytes.get(cursor) != Some(&b'\'') {
        return None;
    }
    cursor += 1;
    if matches!(bytes.get(cursor), Some(b's') | Some(b'S')) {
        cursor += 1;
    }
    if matches!(
        bytes.get(cursor),
        Some(b'0' | b'1' | b'x' | b'X' | b'z' | b'Z')
    ) {
        return Some(cursor + 1);
    }
    let base = *bytes.get(cursor)?;
    if !matches!(base, b'b' | b'B' | b'o' | b'O' | b'd' | b'D' | b'h' | b'H') {
        return None;
    }
    cursor += 1;
    let digits_start = cursor;
    while bytes.get(cursor).is_some_and(|byte| {
        byte.is_ascii_hexdigit() || matches!(*byte, b'x' | b'X' | b'z' | b'Z' | b'?' | b'_')
    }) {
        cursor += 1;
    }
    (cursor > digits_start).then_some(cursor)
}

fn number_end(bytes: &[u8], start: usize, apostrophe_separators: bool) -> usize {
    #[inline]
    fn separated_digit(
        bytes: &[u8],
        i: usize,
        enabled: bool,
        valid_digit: impl Fn(u8) -> bool,
    ) -> bool {
        enabled
            && bytes.get(i) == Some(&b'\'')
            && i > 0
            && bytes.get(i - 1).copied().is_some_and(&valid_digit)
            && bytes.get(i + 1).copied().is_some_and(valid_digit)
    }

    let mut i = start;
    if bytes.get(i) == Some(&b'0')
        && matches!(
            bytes.get(i + 1),
            Some(b'x') | Some(b'X') | Some(b'o') | Some(b'O') | Some(b'b') | Some(b'B')
        )
    {
        let base = bytes[i + 1].to_ascii_lowercase();
        i += 2;
        let valid_digit = |byte: u8| match base {
            b'x' => byte.is_ascii_hexdigit(),
            b'o' => matches!(byte, b'0'..=b'7'),
            b'b' => matches!(byte, b'0' | b'1'),
            _ => false,
        };
        while bytes.get(i).copied().is_some_and(|byte| {
            valid_digit(byte)
                || byte == b'_'
                || separated_digit(bytes, i, apostrophe_separators, valid_digit)
        }) {
            i += 1;
        }
    } else {
        let decimal_digit = |byte: u8| byte.is_ascii_digit();
        while bytes.get(i).copied().is_some_and(|byte| {
            byte.is_ascii_digit()
                || byte == b'_'
                || separated_digit(bytes, i, apostrophe_separators, decimal_digit)
        }) {
            i += 1;
        }
        if bytes.get(i) == Some(&b'.') && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
            i += 1;
            while bytes.get(i).copied().is_some_and(|byte| {
                byte.is_ascii_digit()
                    || byte == b'_'
                    || separated_digit(bytes, i, apostrophe_separators, decimal_digit)
            }) {
                i += 1;
            }
        }
        if matches!(bytes.get(i), Some(b'e') | Some(b'E')) {
            let exponent = i;
            i += 1;
            if matches!(bytes.get(i), Some(b'+') | Some(b'-')) {
                i += 1;
            }
            let digits = i;
            let decimal_digit = |byte: u8| byte.is_ascii_digit();
            while bytes.get(i).copied().is_some_and(|byte| {
                byte.is_ascii_digit()
                    || byte == b'_'
                    || separated_digit(bytes, i, apostrophe_separators, decimal_digit)
            }) {
                i += 1;
            }
            if i == digits {
                i = exponent;
            }
        }
    }
    while bytes
        .get(i)
        .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        i += 1;
    }
    i
}
