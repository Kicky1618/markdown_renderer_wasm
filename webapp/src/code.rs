#[path = "languages.rs"]
mod languages;

use languages::{DeclarationKind, Language};

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
struct Scanner<'a> {
    source: &'a str,
    language: Language,
    cursor: usize,
    expression_expected: bool,
    preprocessor_header_expected: bool,
}

impl<'a> Scanner<'a> {
    fn new(source: &'a str, language: Option<&str>) -> Self {
        Self {
            source,
            language: Language::from_fence(language.unwrap_or("")),
            cursor: 0,
            expression_expected: true,
            preprocessor_header_expected: false,
        }
    }

    fn next(&mut self) -> Option<Token> {
        let bytes = self.source.as_bytes();
        let start = self.cursor;
        if start == bytes.len() {
            return None;
        }

        let (end, kind) = if self.preprocessor_header_expected && bytes[start] == b'<' {
            (header_string_end(self.source, start), SyntaxKind::String)
        } else if is_line_comment(bytes, start, self.language) {
            (
                self.source[start..]
                    .find('\n')
                    .map_or(bytes.len(), |offset| start + offset),
                SyntaxKind::Comment,
            )
        } else if self.language.block_comments() && bytes.get(start..start + 2) == Some(b"/*") {
            (
                block_comment_end(self.source, start, self.language.nested_block_comments()),
                SyntaxKind::Comment,
            )
        } else if let Some(end) = preprocessor_end(bytes, start, self.language) {
            (end, SyntaxKind::Macro)
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
        } else if let Some(end) = rust_raw_string_end(self.source, start, self.language) {
            (end, SyntaxKind::String)
        } else if let Some((quote_at, quote, triple)) =
            python_string_start(bytes, start, self.language)
        {
            (
                quoted_string_end(self.source, quote_at, quote, triple, true),
                SyntaxKind::String,
            )
        } else if self.language.javascript_lexing()
            && self.expression_expected
            && bytes[start] == b'/'
            && !matches!(bytes.get(start + 1), Some(b'/') | Some(b'*'))
        {
            (js_regex_end(self.source, start), SyntaxKind::String)
        } else if matches!(bytes[start], b'\'' | b'"' | b'`') {
            if bytes[start] == b'\'' && is_rust_lifetime(bytes, start, self.language) {
                (start + 1, SyntaxKind::Operator)
            } else {
                let quote = bytes[start];
                (
                    quoted_string_end(
                        self.source,
                        start,
                        quote,
                        false,
                        quote == b'`' || self.language.multiline_strings(),
                    ),
                    SyntaxKind::String,
                )
            }
        } else if bytes[start].is_ascii_digit() {
            (number_end(bytes, start), SyntaxKind::Number)
        } else if is_ident_start(bytes[start], self.language) {
            let mut end = start + 1;
            while end < bytes.len() && is_ident_continue(bytes[end], self.language) {
                end += 1;
            }
            (end, SyntaxKind::Identifier)
        } else if is_operator(bytes[start]) {
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
            let mut end = start + self.source[start..].chars().next().unwrap().len_utf8();
            while end < bytes.len() && !starts_token(bytes, end, self.language) {
                end += self.source[end..].chars().next().unwrap().len_utf8();
            }
            (end, SyntaxKind::Trivia)
        };

        self.cursor = end;
        let token = Token { start, end, kind };
        self.update_lexical_context(token);
        Some(token)
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
fn block_comment_end(source: &str, start: usize, nested: bool) -> usize {
    let bytes = source.as_bytes();
    let mut cursor = start + 2;
    let mut depth = 1usize;
    while cursor < bytes.len() {
        if nested && bytes.get(cursor..cursor + 2) == Some(b"/*") {
            depth += 1;
            cursor += 2;
        } else if bytes.get(cursor..cursor + 2) == Some(b"*/") {
            depth -= 1;
            cursor += 2;
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

fn is_line_comment(bytes: &[u8], i: usize, language: Language) -> bool {
    (bytes.get(i..i + 2) == Some(b"//") && language.slash_comments())
        || (bytes.get(i..i + 2) == Some(b"--") && language.dash_comments())
        || (bytes.get(i) == Some(&b'#') && language.hash_comments())
}

fn is_special_start(bytes: &[u8], i: usize, language: Language) -> bool {
    is_line_comment(bytes, i, language)
        || (language.block_comments() && bytes.get(i..i + 2) == Some(b"/*"))
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
    byte.is_ascii_alphanumeric() || byte == b'_' || (byte == b'$' && language.dollar_identifiers())
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
    if bytes.get(cursor) == Some(&b'b') {
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
    let quote = i + usize::from(bytes.get(i) == Some(&b'b')) + 1 + hashes;
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

fn number_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    if bytes.get(i) == Some(&b'0')
        && matches!(
            bytes.get(i + 1),
            Some(b'x') | Some(b'X') | Some(b'o') | Some(b'O') | Some(b'b') | Some(b'B')
        )
    {
        i += 2;
        while bytes
            .get(i)
            .is_some_and(|b| b.is_ascii_hexdigit() || *b == b'_')
        {
            i += 1;
        }
    } else {
        while bytes
            .get(i)
            .is_some_and(|b| b.is_ascii_digit() || *b == b'_')
        {
            i += 1;
        }
        if bytes.get(i) == Some(&b'.') && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
            i += 1;
            while bytes
                .get(i)
                .is_some_and(|b| b.is_ascii_digit() || *b == b'_')
            {
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
            while bytes
                .get(i)
                .is_some_and(|b| b.is_ascii_digit() || *b == b'_')
            {
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
