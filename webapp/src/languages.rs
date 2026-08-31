//! Lazy language profiles for the code-block highlighter.
//!
//! The scanner/parser in `code.rs` is language agnostic. Only the small
//! vocabulary/feature profile is language-specific. On wasm those profiles are
//! fetched from `langpacks/<name>.langpack` the first time a fenced language is
//! observed; until the fetch completes the block is rendered as plain text.

use std::{borrow::Cow, cell::RefCell};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(inline_js = r#"
export function requestLanguagePack(name) {
    window.dispatchEvent(new CustomEvent("streamdown-language-request", { detail: name }));
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = requestLanguagePack)]
    fn request_language_pack(name: &str);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeclarationKind {
    Function,
    Type,
    Macro,
}

const COMMENT_STYLE_BRACE: u8 = 1 << 0;
const COMMENT_STYLE_ANGLE_HASH: u8 = 1 << 1;
const COMMENT_STYLE_HASH_PIPE: u8 = 1 << 2;
const COMMENT_STYLE_HASH_EQUALS: u8 = 1 << 3;
const COMMENT_STYLE_HASH_BRACKET: u8 = 1 << 4;
const COMMENT_STYLE_SLASH_PLUS: u8 = 1 << 5;
#[cfg(target_arch = "wasm32")]
const EXTENDED_SECTIONS_FLAG: u32 = 1 << 31;

#[derive(Debug)]
struct LanguageProfile {
    aliases: &'static [&'static str],
    keywords: &'static [&'static str],
    builtin_types: &'static [&'static str],
    function_declarations: &'static [&'static str],
    type_declarations: &'static [&'static str],
    macro_declarations: &'static [&'static str],
    preprocessor_macro_operands: &'static [&'static str],
    preprocessor_headers: &'static [&'static str],
    bang_macro_declarations: &'static [&'static str],
    macro_identifiers: &'static [&'static str],
    macro_operand_identifiers: &'static [&'static str],
    header_macro_identifiers: &'static [&'static str],
    expression_prefixes: &'static [&'static str],
    case_insensitive_keywords: bool,
    slash_comments: bool,
    dash_comments: bool,
    hash_comments: bool,
    block_comments: bool,
    nested_block_comments: bool,
    preprocessor: bool,
    decorators: bool,
    dollar_identifiers: bool,
    javascript_lexing: bool,
    python_strings: bool,
    rust_syntax: bool,
    multiline_strings: bool,
    rust_attributes: bool,
    bang_macros: bool,
    uppercase_macros: bool,
    macro_metavariables: bool,
    semicolon_comments: bool,
    percent_comments: bool,
    apostrophe_comments: bool,
    bang_comments: bool,
    hyphen_identifiers: bool,
    question_identifiers: bool,
    bang_identifiers: bool,
    paren_star_comments: bool,
    brace_dash_comments: bool,
    triple_double_strings: bool,
    triple_single_strings: bool,
    double_semicolon_comments: bool,
    paren_semicolon_comments: bool,
    lua_long_brackets: bool,
    comment_styles: u8,
}

macro_rules! empty_profile {
    ($aliases:expr) => {
        LanguageProfile {
            aliases: $aliases,
            keywords: &[],
            builtin_types: &[],
            function_declarations: &[],
            type_declarations: &[],
            macro_declarations: &[],
            preprocessor_macro_operands: &[],
            preprocessor_headers: &[],
            bang_macro_declarations: &[],
            macro_identifiers: &[],
            macro_operand_identifiers: &[],
            header_macro_identifiers: &[],
            expression_prefixes: &[],
            case_insensitive_keywords: false,
            slash_comments: false,
            dash_comments: false,
            hash_comments: false,
            block_comments: false,
            nested_block_comments: false,
            preprocessor: false,
            decorators: false,
            dollar_identifiers: false,
            javascript_lexing: false,
            python_strings: false,
            rust_syntax: false,
            multiline_strings: false,
            rust_attributes: false,
            bang_macros: false,
            uppercase_macros: false,
            macro_metavariables: false,
            semicolon_comments: false,
            percent_comments: false,
            apostrophe_comments: false,
            bang_comments: false,
            hyphen_identifiers: false,
            question_identifiers: false,
            bang_identifiers: false,
            paren_star_comments: false,
            brace_dash_comments: false,
            triple_double_strings: false,
            triple_single_strings: false,
            double_semicolon_comments: false,
            paren_semicolon_comments: false,
            lua_long_brackets: false,
            comment_styles: 0,
        }
    };
}

const PLAIN: LanguageProfile = empty_profile!(&[]);
// Native tests use this tiny manifest to synchronously load the same packs.
// Browser alias resolution lives in language-loader.js, outside the wasm.
#[cfg(not(target_arch = "wasm32"))]
const PACK_ALIASES: &[(&str, &str)] = &[
    ("aarch64asm", "assembly"),
    ("asm", "assembly"),
    ("assembly", "assembly"),
    ("astro", "html"),
    ("bash", "shell"),
    ("c", "cpp"),
    ("c#", "csharp"),
    ("c++", "cpp"),
    ("cairo", "cairo"),
    ("cairo1", "cairo"),
    ("cc", "cpp"),
    ("cfg", "ini"),
    ("cjs", "javascript"),
    ("cl", "lisp"),
    ("clc", "opencl"),
    ("clj", "clojure"),
    ("cljc", "clojure"),
    ("cljs", "clojure"),
    ("clojure", "clojure"),
    ("common-lisp", "lisp"),
    ("commonlisp", "lisp"),
    ("commonmark", "markdown"),
    ("comp", "glsl"),
    ("conf", "ini"),
    ("containerfile", "dockerfile"),
    ("cpp", "cpp"),
    ("cs", "csharp"),
    ("csharp", "csharp"),
    ("css", "css"),
    ("cts", "javascript"),
    ("cu", "cuda"),
    ("cuda", "cuda"),
    ("cuh", "cuda"),
    ("cxx", "cpp"),
    ("d", "dlang"),
    ("dart", "dart"),
    ("dlang", "dlang"),
    ("docker", "dockerfile"),
    ("dockerfile", "dockerfile"),
    ("dotnet", "csharp"),
    ("edn", "clojure"),
    ("elixir", "elixir"),
    ("erl", "erlang"),
    ("erlang", "erlang"),
    ("ex", "elixir"),
    ("exs", "elixir"),
    ("f#", "fsharp"),
    ("f03", "fortran"),
    ("f08", "fortran"),
    ("f77", "fortran"),
    ("f90", "fortran"),
    ("f95", "fortran"),
    ("fortran", "fortran"),
    ("frag", "glsl"),
    ("fs", "fsharp"),
    ("fsharp", "fsharp"),
    ("fsx", "fsharp"),
    ("fx", "hlsl"),
    ("gas", "assembly"),
    ("geom", "glsl"),
    ("glsl", "glsl"),
    ("gnumake", "makefile"),
    ("go", "go"),
    ("golang", "go"),
    ("gql", "graphql"),
    ("gradle", "groovy"),
    ("graphql", "graphql"),
    ("groovy", "groovy"),
    ("h", "cpp"),
    ("haskell", "haskell"),
    ("hcl", "terraform"),
    ("hlsl", "hlsl"),
    ("hpp", "cpp"),
    ("hrl", "erlang"),
    ("hs", "haskell"),
    ("htm", "html"),
    ("html", "html"),
    ("ini", "ini"),
    ("java", "java"),
    ("javascript", "javascript"),
    ("jl", "julia"),
    ("js", "javascript"),
    ("json", "json"),
    ("jsonc", "json"),
    ("jsx", "javascript"),
    ("julia", "julia"),
    ("kotlin", "kotlin"),
    ("kt", "kotlin"),
    ("kts", "kotlin"),
    ("lhs", "haskell"),
    ("lisp", "lisp"),
    ("lua", "lua"),
    ("m", "matlab"),
    ("make", "makefile"),
    ("makefile", "makefile"),
    ("markdown", "markdown"),
    ("matlab", "matlab"),
    ("md", "markdown"),
    ("mdx", "markdown"),
    ("mjs", "javascript"),
    ("ml", "ocaml"),
    ("mli", "ocaml"),
    ("mm", "objectivec"),
    ("move", "move"),
    ("movelang", "move"),
    ("mts", "javascript"),
    ("nasm", "assembly"),
    ("nim", "nim"),
    ("nimrod", "nim"),
    ("node", "javascript"),
    ("objc", "objectivec"),
    ("objective-c", "objectivec"),
    ("objectivec", "objectivec"),
    ("ocaml", "ocaml"),
    ("octave", "matlab"),
    ("opencl", "opencl"),
    ("perl", "perl"),
    ("perl5", "perl"),
    ("php", "php"),
    ("php8", "php"),
    ("pl", "perl"),
    ("plg", "prolog"),
    ("plist", "xml"),
    ("pm", "perl"),
    ("postgres", "sql"),
    ("postgresql", "sql"),
    ("powershell", "powershell"),
    ("prolog", "prolog"),
    ("properties", "ini"),
    ("proto", "protobuf"),
    ("proto3", "protobuf"),
    ("protobuf", "protobuf"),
    ("ps1", "powershell"),
    ("pwsh", "powershell"),
    ("py", "python"),
    ("python", "python"),
    ("python3", "python"),
    ("r", "r"),
    ("racket", "scheme"),
    ("rb", "ruby"),
    ("rkt", "scheme"),
    ("rlang", "r"),
    ("rs", "rust"),
    ("rscript", "r"),
    ("ruby", "ruby"),
    ("rust", "rust"),
    ("sc", "scala"),
    ("scala", "scala"),
    ("scheme", "scheme"),
    ("scm", "scheme"),
    ("scss", "css"),
    ("sh", "shell"),
    ("shader", "glsl"),
    ("shaderlab", "hlsl"),
    ("shell", "shell"),
    ("sol", "solidity"),
    ("solidity", "solidity"),
    ("sql", "sql"),
    ("sv", "verilog"),
    ("svelte", "html"),
    ("svg", "xml"),
    ("svh", "verilog"),
    ("swift", "swift"),
    ("systemverilog", "verilog"),
    ("terraform", "terraform"),
    ("tf", "terraform"),
    ("toml", "toml"),
    ("ts", "javascript"),
    ("tsx", "javascript"),
    ("typescript", "javascript"),
    ("v", "verilog"),
    ("vala", "vala"),
    ("vapi", "vala"),
    ("vb", "vbnet"),
    ("vbnet", "vbnet"),
    ("verilog", "verilog"),
    ("vert", "glsl"),
    ("vhd", "vhdl"),
    ("vhdl", "vhdl"),
    ("visualbasic", "vbnet"),
    ("vue", "html"),
    ("webgpu", "wgsl"),
    ("wgsl", "wgsl"),
    ("x86_64asm", "assembly"),
    ("x86asm", "assembly"),
    ("xhtml", "html"),
    ("xml", "xml"),
    ("xsd", "xml"),
    ("xsl", "xml"),
    ("xslt", "xml"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("zig", "zig"),
    ("zsh", "shell"),
];

thread_local! {
    static LOADED: RefCell<Vec<&'static LanguageProfile>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Language(&'static LanguageProfile);

impl Language {
    pub(super) fn from_fence(name: &str) -> Self {
        let name = name.trim();
        if name.is_empty() {
            return Self(&PLAIN);
        }
        let lookup = normalize_fence_name(name);
        let lookup = lookup.as_ref();
        if let Some(profile) = find_loaded(lookup) {
            return Self(profile);
        }
        #[cfg(target_arch = "wasm32")]
        request_language_pack(lookup);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(pack) = pack_name_for(lookup) {
            ensure_pack(&pack);
        }
        find_loaded(lookup).map_or(Self(&PLAIN), Self)
    }

    #[cfg(test)]
    pub(super) fn is_plain(self) -> bool {
        std::ptr::eq(self.0, &PLAIN)
    }

    pub(super) fn is_keyword(self, word: &str) -> bool {
        contains_word(self.0.keywords, word, self.0.case_insensitive_keywords)
    }

    pub(super) fn is_builtin_type(self, word: &str) -> bool {
        self.0.builtin_types.contains(&word)
    }

    pub(super) fn declaration_after(self, word: &str) -> Option<DeclarationKind> {
        if self.0.function_declarations.contains(&word) {
            Some(DeclarationKind::Function)
        } else if self.0.type_declarations.contains(&word) {
            Some(DeclarationKind::Type)
        } else if self.0.macro_declarations.contains(&word) {
            Some(DeclarationKind::Macro)
        } else {
            None
        }
    }

    pub(super) fn is_expression_prefix(self, word: &str) -> bool {
        self.0.expression_prefixes.contains(&word)
    }

    pub(super) fn slash_comments(self) -> bool {
        self.0.slash_comments
    }
    pub(super) fn dash_comments(self) -> bool {
        self.0.dash_comments
    }
    pub(super) fn hash_comments(self) -> bool {
        self.0.hash_comments
    }
    pub(super) fn block_comments(self) -> bool {
        self.0.block_comments
    }
    pub(super) fn nested_block_comments(self) -> bool {
        self.0.nested_block_comments
    }
    pub(super) fn preprocessor(self) -> bool {
        self.0.preprocessor
    }
    pub(super) fn decorators(self) -> bool {
        self.0.decorators
    }
    pub(super) fn dollar_identifiers(self) -> bool {
        self.0.dollar_identifiers
    }
    pub(super) fn javascript_lexing(self) -> bool {
        self.0.javascript_lexing
    }
    pub(super) fn python_strings(self) -> bool {
        self.0.python_strings
    }
    pub(super) fn rust_syntax(self) -> bool {
        self.0.rust_syntax
    }
    pub(super) fn multiline_strings(self) -> bool {
        self.0.multiline_strings
    }
    pub(super) fn rust_attributes(self) -> bool {
        self.0.rust_attributes
    }
    pub(super) fn bang_macros(self) -> bool {
        self.0.bang_macros
    }

    pub(super) fn is_bang_macro_declaration(self, word: &str) -> bool {
        self.0.bang_macro_declarations.contains(&word)
    }

    pub(super) fn uppercase_macros(self) -> bool {
        self.0.uppercase_macros
    }
    pub(super) fn macro_metavariables(self) -> bool {
        self.0.macro_metavariables
    }

    pub(super) fn semicolon_comments(self) -> bool {
        self.0.semicolon_comments
    }
    pub(super) fn percent_comments(self) -> bool {
        self.0.percent_comments
    }
    pub(super) fn apostrophe_comments(self) -> bool {
        self.0.apostrophe_comments
    }
    pub(super) fn bang_comments(self) -> bool {
        self.0.bang_comments
    }
    pub(super) fn hyphen_identifiers(self) -> bool {
        self.0.hyphen_identifiers
    }
    pub(super) fn question_identifiers(self) -> bool {
        self.0.question_identifiers
    }
    pub(super) fn bang_identifiers(self) -> bool {
        self.0.bang_identifiers
    }
    pub(super) fn paren_star_comments(self) -> bool {
        self.0.paren_star_comments
    }
    pub(super) fn brace_dash_comments(self) -> bool {
        self.0.brace_dash_comments
    }
    pub(super) fn triple_double_strings(self) -> bool {
        self.0.triple_double_strings
    }
    pub(super) fn triple_single_strings(self) -> bool {
        self.0.triple_single_strings
    }
    pub(super) fn double_semicolon_comments(self) -> bool {
        self.0.double_semicolon_comments
    }
    pub(super) fn paren_semicolon_comments(self) -> bool {
        self.0.paren_semicolon_comments
    }
    pub(super) fn lua_long_brackets(self) -> bool {
        self.0.lua_long_brackets
    }
    pub(super) fn brace_comments(self) -> bool {
        self.0.comment_styles & COMMENT_STYLE_BRACE != 0
    }
    pub(super) fn angle_hash_comments(self) -> bool {
        self.0.comment_styles & COMMENT_STYLE_ANGLE_HASH != 0
    }
    pub(super) fn hash_pipe_comments(self) -> bool {
        self.0.comment_styles & COMMENT_STYLE_HASH_PIPE != 0
    }
    pub(super) fn hash_equals_comments(self) -> bool {
        self.0.comment_styles & COMMENT_STYLE_HASH_EQUALS != 0
    }
    pub(super) fn hash_bracket_comments(self) -> bool {
        self.0.comment_styles & COMMENT_STYLE_HASH_BRACKET != 0
    }
    pub(super) fn slash_plus_comments(self) -> bool {
        self.0.comment_styles & COMMENT_STYLE_SLASH_PLUS != 0
    }

    pub(super) fn is_macro_identifier(self, word: &str) -> bool {
        self.0.macro_identifiers.contains(&word)
    }

    pub(super) fn macro_operand_after_identifier(self, word: &str) -> bool {
        self.0.macro_operand_identifiers.contains(&word)
    }

    pub(super) fn header_after_identifier(self, word: &str) -> bool {
        self.0.header_macro_identifiers.contains(&word)
    }

    pub(super) fn macro_operand_after(self, directive: &str) -> bool {
        directive_name(directive)
            .is_some_and(|name| self.0.preprocessor_macro_operands.contains(&name))
    }

    pub(super) fn header_after(self, directive: &str) -> bool {
        directive_name(directive).is_some_and(|name| self.0.preprocessor_headers.contains(&name))
    }
}

fn find_loaded(name: &str) -> Option<&'static LanguageProfile> {
    LOADED.with(|loaded| {
        loaded.borrow().iter().copied().find(|profile| {
            profile
                .aliases
                .iter()
                .any(|alias| name.eq_ignore_ascii_case(alias))
        })
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn pack_name_for(name: &str) -> Option<String> {
    if let Some((_, pack)) = PACK_ALIASES
        .iter()
        .find(|(alias, _)| name.eq_ignore_ascii_case(alias))
    {
        return Some((*pack).to_owned());
    }
    let normalized = normalize_dynamic_pack_name(name)?;
    let direct = native_pack_dir().join(format!("{normalized}.langpack"));
    if direct.is_file() {
        return Some(normalized);
    }
    native_pack_aliases()
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(&normalized))
        .map(|(_, pack)| pack.clone())
}

#[cfg(not(target_arch = "wasm32"))]
fn normalize_dynamic_pack_name(name: &str) -> Option<String> {
    let normalized = name.to_ascii_lowercase();
    (!normalized.is_empty()
        && normalized.len() <= 48
        && normalized
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'+' | b'#' | b'-')))
    .then_some(normalized)
}

#[cfg(not(target_arch = "wasm32"))]
fn native_pack_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("langpacks")
}

#[cfg(not(target_arch = "wasm32"))]
fn native_pack_aliases() -> &'static [(String, String)] {
    use std::sync::OnceLock;
    static INDEX: OnceLock<Vec<(String, String)>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let Ok(entries) = std::fs::read_dir(native_pack_dir()) else {
            return Vec::new();
        };
        let mut index = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("langpack") {
                continue;
            }
            let Some(pack) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some(line) = source.lines().find(|line| line.starts_with("aliases\t")) {
                index.extend(
                    line.split('\t')
                        .skip(1)
                        .filter_map(normalize_dynamic_pack_name)
                        .map(|alias| (alias, pack.to_owned())),
                );
            }
        }
        index
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn ensure_pack(pack: &str) {
    if find_loaded(pack).is_some() {
        return;
    }
    if let Some(source) = embedded_pack(pack) {
        let _ = register_pack_source(source);
        return;
    }
    if let Ok(source) = std::fs::read_to_string(native_pack_dir().join(format!("{pack}.langpack")))
    {
        let _ = register_pack_source(&source);
    }
}

#[cfg(target_arch = "wasm32")]
const BINARY_MAGIC: &[u8; 4] = b"SLP1";
#[cfg(target_arch = "wasm32")]
const MAX_BINARY_WORDS: usize = 4096;
#[cfg(target_arch = "wasm32")]
const MAX_BINARY_BYTES: usize = 16 * 1024;
#[cfg(target_arch = "wasm32")]
const BINARY_WORD_SECTIONS: usize = 13;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn register_language_pack_binary(bytes: &[u8]) -> bool {
    if bytes.len() > MAX_BINARY_BYTES {
        return false;
    }
    let Some(flags) = validate_binary_profile(bytes) else {
        return false;
    };
    if binary_aliases_conflict(bytes) {
        return false;
    }
    let Some(profile) = decode_validated_binary_profile(bytes, flags) else {
        return false;
    };
    register_profile(profile)
}

fn register_profile(profile: LanguageProfile) -> bool {
    if profile.aliases.is_empty() {
        return false;
    }
    if profile
        .aliases
        .iter()
        .all(|alias| find_loaded(alias).is_some())
    {
        return false;
    }
    if profile
        .aliases
        .iter()
        .any(|alias| find_loaded(alias).is_some())
    {
        return false;
    }
    let profile = Box::leak(Box::new(profile));
    LOADED.with(|loaded| loaded.borrow_mut().push(profile));
    true
}

#[cfg(target_arch = "wasm32")]
struct BinaryCursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

#[cfg(target_arch = "wasm32")]
impl<'a> BinaryCursor<'a> {
    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(len)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    fn u16(&mut self) -> Option<usize> {
        let bytes = self.take(2)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]) as usize)
    }

    fn u32(&mut self) -> Option<u32> {
        let bytes = self.take(4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn skip_words(&mut self) -> Option<usize> {
        let count = self.u16()?;
        if count > MAX_BINARY_WORDS {
            return None;
        }
        for _ in 0..count {
            let len = self.u16()?;
            std::str::from_utf8(self.take(len)?).ok()?;
        }
        Some(count)
    }

    fn comment_styles(&mut self) -> Option<u8> {
        let count = self.u16()?;
        if count > 16 {
            return None;
        }
        let mut styles = 0u8;
        for _ in 0..count {
            let len = self.u16()?;
            let style = std::str::from_utf8(self.take(len)?).ok()?;
            styles |= comment_style_bit(style)?;
        }
        Some(styles)
    }

    fn words(&mut self) -> Option<&'static [&'static str]> {
        let count = self.u16()?;
        if count > MAX_BINARY_WORDS {
            return None;
        }
        let mut words = Vec::with_capacity(count);
        for _ in 0..count {
            let len = self.u16()?;
            let word = std::str::from_utf8(self.take(len)?).ok()?;
            let word: &'static str = Box::leak(word.to_owned().into_boxed_str());
            words.push(word);
        }
        Some(Box::leak(words.into_boxed_slice()))
    }
}

#[cfg(target_arch = "wasm32")]
fn validate_binary_profile(bytes: &[u8]) -> Option<u32> {
    if bytes.len() > MAX_BINARY_BYTES {
        return None;
    }
    let mut cursor = BinaryCursor { bytes, at: 0 };
    if cursor.take(4)? != BINARY_MAGIC {
        return None;
    }
    let flags = cursor.u32()?;
    if cursor.skip_words()? == 0 {
        return None;
    }
    for _ in 1..BINARY_WORD_SECTIONS {
        cursor.skip_words()?;
    }
    if flags & EXTENDED_SECTIONS_FLAG != 0 {
        cursor.comment_styles()?;
    }
    (cursor.at == bytes.len()).then_some(flags)
}

#[cfg(target_arch = "wasm32")]
fn binary_aliases_conflict(bytes: &[u8]) -> bool {
    let mut cursor = BinaryCursor { bytes, at: 0 };
    if cursor.take(4) != Some(BINARY_MAGIC) || cursor.u32().is_none() {
        return true;
    }
    let Some(count) = cursor.u16() else {
        return true;
    };
    for _ in 0..count {
        let Some(len) = cursor.u16() else {
            return true;
        };
        let Some(alias) = cursor
            .take(len)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
        else {
            return true;
        };
        if find_loaded(alias).is_some() {
            return true;
        }
    }
    false
}

#[cfg(target_arch = "wasm32")]
fn decode_validated_binary_profile(bytes: &[u8], flags: u32) -> Option<LanguageProfile> {
    // The complete binary and all aliases were checked before this allocation
    // pass. Failed or duplicate external input therefore cannot retain decoded
    // strings in the WASM heap.
    let mut cursor = BinaryCursor { bytes, at: 0 };
    cursor.take(4)?;
    cursor.u32()?;
    let mut profile = empty_profile!(&[]);
    profile.aliases = cursor.words()?;
    profile.keywords = cursor.words()?;
    profile.builtin_types = cursor.words()?;
    profile.function_declarations = cursor.words()?;
    profile.type_declarations = cursor.words()?;
    profile.macro_declarations = cursor.words()?;
    profile.preprocessor_macro_operands = cursor.words()?;
    profile.preprocessor_headers = cursor.words()?;
    profile.bang_macro_declarations = cursor.words()?;
    profile.macro_identifiers = cursor.words()?;
    profile.macro_operand_identifiers = cursor.words()?;
    profile.header_macro_identifiers = cursor.words()?;
    profile.expression_prefixes = cursor.words()?;
    if flags & EXTENDED_SECTIONS_FLAG != 0 {
        profile.comment_styles = cursor.comment_styles()?;
    }
    if profile.aliases.is_empty() || cursor.at != bytes.len() {
        return None;
    }
    profile.case_insensitive_keywords = flags & (1 << 0) != 0;
    profile.slash_comments = flags & (1 << 1) != 0;
    profile.dash_comments = flags & (1 << 2) != 0;
    profile.hash_comments = flags & (1 << 3) != 0;
    profile.block_comments = flags & (1 << 4) != 0;
    profile.nested_block_comments = flags & (1 << 5) != 0;
    profile.preprocessor = flags & (1 << 6) != 0;
    profile.decorators = flags & (1 << 7) != 0;
    profile.dollar_identifiers = flags & (1 << 8) != 0;
    profile.javascript_lexing = flags & (1 << 9) != 0;
    profile.python_strings = flags & (1 << 10) != 0;
    profile.rust_syntax = flags & (1 << 11) != 0;
    profile.multiline_strings = flags & (1 << 12) != 0;
    profile.rust_attributes = flags & (1 << 13) != 0;
    profile.bang_macros = flags & (1 << 14) != 0;
    profile.uppercase_macros = flags & (1 << 15) != 0;
    profile.macro_metavariables = flags & (1 << 16) != 0;
    profile.semicolon_comments = flags & (1 << 17) != 0;
    profile.percent_comments = flags & (1 << 18) != 0;
    profile.apostrophe_comments = flags & (1 << 19) != 0;
    profile.bang_comments = flags & (1 << 20) != 0;
    profile.hyphen_identifiers = flags & (1 << 21) != 0;
    profile.question_identifiers = flags & (1 << 22) != 0;
    profile.bang_identifiers = flags & (1 << 23) != 0;
    profile.paren_star_comments = flags & (1 << 24) != 0;
    profile.brace_dash_comments = flags & (1 << 25) != 0;
    profile.triple_double_strings = flags & (1 << 26) != 0;
    profile.triple_single_strings = flags & (1 << 27) != 0;
    profile.double_semicolon_comments = flags & (1 << 28) != 0;
    profile.paren_semicolon_comments = flags & (1 << 29) != 0;
    profile.lua_long_brackets = flags & (1 << 30) != 0;
    Some(profile)
}

#[cfg(not(target_arch = "wasm32"))]
fn register_pack_source(source: &str) -> Result<bool, String> {
    let profile = parse_pack(source)?;
    let aliases = profile.aliases;
    let already_loaded = aliases.iter().all(|alias| find_loaded(alias).is_some());
    if already_loaded {
        return Ok(false);
    }
    if let Some(alias) = aliases.iter().find(|alias| find_loaded(alias).is_some()) {
        return Err(format!("language alias already registered: {alias}"));
    }
    Ok(register_profile(profile))
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_pack(source: &str) -> Result<LanguageProfile, String> {
    let mut lines = source.lines();
    if lines.next() != Some("STREAMDOWN_LANGPACK\t1") {
        return Err("invalid langpack header".to_owned());
    }
    let mut profile = empty_profile!(&[]);
    let mut saw_aliases = false;
    for (index, line) in lines.enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split('\t');
        let key = fields.next().unwrap_or_default();
        let values: Vec<&str> = fields.filter(|value| !value.is_empty()).collect();
        match key {
            "aliases" => {
                if values.is_empty() {
                    return Err("langpack has no aliases".to_owned());
                }
                profile.aliases = leak_words(values);
                saw_aliases = true;
            }
            "keywords" => profile.keywords = leak_words(values),
            "builtin_types" => profile.builtin_types = leak_words(values),
            "function_declarations" => profile.function_declarations = leak_words(values),
            "type_declarations" => profile.type_declarations = leak_words(values),
            "macro_declarations" => profile.macro_declarations = leak_words(values),
            "preprocessor_macro_operands" => {
                profile.preprocessor_macro_operands = leak_words(values)
            }
            "preprocessor_headers" => profile.preprocessor_headers = leak_words(values),
            "bang_macro_declarations" => profile.bang_macro_declarations = leak_words(values),
            "macro_identifiers" => profile.macro_identifiers = leak_words(values),
            "macro_operand_identifiers" => profile.macro_operand_identifiers = leak_words(values),
            "header_macro_identifiers" => profile.header_macro_identifiers = leak_words(values),
            "expression_prefixes" => profile.expression_prefixes = leak_words(values),
            "comment_styles" => {
                profile.comment_styles = parse_comment_styles(&values)?;
            }
            "flags" => {
                for flag in values {
                    set_flag(&mut profile, flag)?;
                }
            }
            _ => {
                return Err(format!(
                    "unknown langpack field {key:?} at line {}",
                    index + 2
                ));
            }
        }
    }
    if !saw_aliases {
        return Err("langpack is missing aliases".to_owned());
    }
    Ok(profile)
}

#[cfg(not(target_arch = "wasm32"))]
fn leak_words(values: Vec<&str>) -> &'static [&'static str] {
    let words = values
        .into_iter()
        .map(|value| -> &'static str { Box::leak(value.to_owned().into_boxed_str()) })
        .collect::<Vec<_>>();
    Box::leak(words.into_boxed_slice())
}

fn comment_style_bit(style: &str) -> Option<u8> {
    Some(match style {
        "brace" => COMMENT_STYLE_BRACE,
        "angle_hash" => COMMENT_STYLE_ANGLE_HASH,
        "hash_pipe" => COMMENT_STYLE_HASH_PIPE,
        "hash_equals" => COMMENT_STYLE_HASH_EQUALS,
        "hash_bracket" => COMMENT_STYLE_HASH_BRACKET,
        "slash_plus" => COMMENT_STYLE_SLASH_PLUS,
        _ => return None,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_comment_styles(values: &[&str]) -> Result<u8, String> {
    values.iter().try_fold(0u8, |styles, style| {
        comment_style_bit(style)
            .map(|bit| styles | bit)
            .ok_or_else(|| format!("unknown comment style: {style}"))
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn set_flag(profile: &mut LanguageProfile, flag: &str) -> Result<(), String> {
    match flag {
        "case_insensitive_keywords" => profile.case_insensitive_keywords = true,
        "slash_comments" => profile.slash_comments = true,
        "dash_comments" => profile.dash_comments = true,
        "hash_comments" => profile.hash_comments = true,
        "block_comments" => profile.block_comments = true,
        "nested_block_comments" => profile.nested_block_comments = true,
        "preprocessor" => profile.preprocessor = true,
        "decorators" => profile.decorators = true,
        "dollar_identifiers" => profile.dollar_identifiers = true,
        "javascript_lexing" => profile.javascript_lexing = true,
        "python_strings" => profile.python_strings = true,
        "rust_syntax" => profile.rust_syntax = true,
        "multiline_strings" => profile.multiline_strings = true,
        "rust_attributes" => profile.rust_attributes = true,
        "bang_macros" => profile.bang_macros = true,
        "uppercase_macros" => profile.uppercase_macros = true,
        "macro_metavariables" => profile.macro_metavariables = true,
        "semicolon_comments" => profile.semicolon_comments = true,
        "percent_comments" => profile.percent_comments = true,
        "apostrophe_comments" => profile.apostrophe_comments = true,
        "bang_comments" => profile.bang_comments = true,
        "hyphen_identifiers" => profile.hyphen_identifiers = true,
        "question_identifiers" => profile.question_identifiers = true,
        "bang_identifiers" => profile.bang_identifiers = true,
        "paren_star_comments" => profile.paren_star_comments = true,
        "brace_dash_comments" => profile.brace_dash_comments = true,
        "triple_double_strings" => profile.triple_double_strings = true,
        "triple_single_strings" => profile.triple_single_strings = true,
        "double_semicolon_comments" => profile.double_semicolon_comments = true,
        "paren_semicolon_comments" => profile.paren_semicolon_comments = true,
        "lua_long_brackets" => profile.lua_long_brackets = true,
        _ => return Err(format!("unknown langpack flag: {flag}")),
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn embedded_pack(name: &str) -> Option<&'static str> {
    Some(match name {
        "assembly" => include_str!("../langpacks/assembly.langpack"),
        "cairo" => include_str!("../langpacks/cairo.langpack"),
        "clojure" => include_str!("../langpacks/clojure.langpack"),
        "cpp" => include_str!("../langpacks/cpp.langpack"),
        "csharp" => include_str!("../langpacks/csharp.langpack"),
        "css" => include_str!("../langpacks/css.langpack"),
        "cuda" => include_str!("../langpacks/cuda.langpack"),
        "dart" => include_str!("../langpacks/dart.langpack"),
        "dlang" => include_str!("../langpacks/dlang.langpack"),
        "dockerfile" => include_str!("../langpacks/dockerfile.langpack"),
        "elixir" => include_str!("../langpacks/elixir.langpack"),
        "erlang" => include_str!("../langpacks/erlang.langpack"),
        "fortran" => include_str!("../langpacks/fortran.langpack"),
        "fsharp" => include_str!("../langpacks/fsharp.langpack"),
        "glsl" => include_str!("../langpacks/glsl.langpack"),
        "go" => include_str!("../langpacks/go.langpack"),
        "graphql" => include_str!("../langpacks/graphql.langpack"),
        "groovy" => include_str!("../langpacks/groovy.langpack"),
        "haskell" => include_str!("../langpacks/haskell.langpack"),
        "hlsl" => include_str!("../langpacks/hlsl.langpack"),
        "html" => include_str!("../langpacks/html.langpack"),
        "ini" => include_str!("../langpacks/ini.langpack"),
        "java" => include_str!("../langpacks/java.langpack"),
        "javascript" => include_str!("../langpacks/javascript.langpack"),
        "json" => include_str!("../langpacks/json.langpack"),
        "julia" => include_str!("../langpacks/julia.langpack"),
        "kotlin" => include_str!("../langpacks/kotlin.langpack"),
        "lisp" => include_str!("../langpacks/lisp.langpack"),
        "lua" => include_str!("../langpacks/lua.langpack"),
        "makefile" => include_str!("../langpacks/makefile.langpack"),
        "markdown" => include_str!("../langpacks/markdown.langpack"),
        "matlab" => include_str!("../langpacks/matlab.langpack"),
        "move" => include_str!("../langpacks/move.langpack"),
        "nim" => include_str!("../langpacks/nim.langpack"),
        "objectivec" => include_str!("../langpacks/objectivec.langpack"),
        "ocaml" => include_str!("../langpacks/ocaml.langpack"),
        "opencl" => include_str!("../langpacks/opencl.langpack"),
        "perl" => include_str!("../langpacks/perl.langpack"),
        "php" => include_str!("../langpacks/php.langpack"),
        "powershell" => include_str!("../langpacks/powershell.langpack"),
        "prolog" => include_str!("../langpacks/prolog.langpack"),
        "protobuf" => include_str!("../langpacks/protobuf.langpack"),
        "python" => include_str!("../langpacks/python.langpack"),
        "r" => include_str!("../langpacks/r.langpack"),
        "ruby" => include_str!("../langpacks/ruby.langpack"),
        "rust" => include_str!("../langpacks/rust.langpack"),
        "scala" => include_str!("../langpacks/scala.langpack"),
        "scheme" => include_str!("../langpacks/scheme.langpack"),
        "shell" => include_str!("../langpacks/shell.langpack"),
        "solidity" => include_str!("../langpacks/solidity.langpack"),
        "sql" => include_str!("../langpacks/sql.langpack"),
        "swift" => include_str!("../langpacks/swift.langpack"),
        "terraform" => include_str!("../langpacks/terraform.langpack"),
        "toml" => include_str!("../langpacks/toml.langpack"),
        "vala" => include_str!("../langpacks/vala.langpack"),
        "vbnet" => include_str!("../langpacks/vbnet.langpack"),
        "verilog" => include_str!("../langpacks/verilog.langpack"),
        "vhdl" => include_str!("../langpacks/vhdl.langpack"),
        "wgsl" => include_str!("../langpacks/wgsl.langpack"),
        "xml" => include_str!("../langpacks/xml.langpack"),
        "yaml" => include_str!("../langpacks/yaml.langpack"),
        "zig" => include_str!("../langpacks/zig.langpack"),
        _ => return None,
    })
}

fn normalize_fence_name(name: &str) -> Cow<'_, str> {
    let name = name.trim();
    if let Some(canonical) = special_fence_name(name) {
        return Cow::Borrowed(canonical);
    }

    // Already-safe fence names are the hot path. Keep them borrowed so
    // ordinary `rust`, `cpp`, `python`, ... code blocks allocate nothing.
    if name.len() <= 48
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'+' | b'#' | b'-'))
    {
        return Cow::Borrowed(name);
    }

    // Human-facing names such as `LLVM Machine IR` or `IBM z/Architecture
    // assembly` are deterministically mapped to the canonical pack filename.
    // Only ASCII-safe bytes are emitted; punctuation runs become one `-`.
    let mut slug = String::with_capacity(name.len().min(48));
    let mut separator = false;
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'+' | b'#' | b'-') {
            if separator && !slug.is_empty() && !slug.ends_with('-') {
                slug.push('-');
            }
            separator = false;
            if slug.len() == 48 {
                break;
            }
            slug.push((byte as char).to_ascii_lowercase());
        } else {
            separator = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(slug)
    }
}

fn special_fence_name(name: &str) -> Option<&'static str> {
    if name == "V" || name.eq_ignore_ascii_case("V language") {
        return Some("vlang");
    }
    if name == "><>" {
        return Some("fish-esolang");
    }
    if name == "///" {
        return Some("slashes-esolang");
    }
    if matches!(name, "P′′" | "P''" | "p′′" | "p''") {
        return Some("p-prime-prime");
    }
    if name.eq_ignore_ascii_case("Ook!") {
        return Some("ook");
    }
    if name.eq_ignore_ascii_case("Ink!") {
        return Some("ink");
    }
    if name == "GAS" {
        return Some("gas-syntax");
    }
    for (display, canonical) in [
        ("Delphi / Object Pascal", "objectpascal"),
        ("Object Pascal", "objectpascal"),
        ("Visual Basic", "vbnet"),
        ("LabVIEW G", "labview"),
        ("PL/SQL", "plsql"),
        ("PL/I", "pli"),
        ("PL/M", "plm"),
        ("Common Lisp", "lisp"),
        ("Coq / Gallina", "coq"),
        ("Standard ML", "sml"),
        ("CUDA C++", "cuda"),
        ("OpenCL C", "opencl"),
        ("WebAssembly Text Format", "wat"),
        ("BBC BASIC", "bbcbasic"),
        ("Component Pascal", "componentpascal"),
        ("ALGOL 60", "algol60"),
        ("ALGOL 68", "algol68"),
        ("Monkey X", "monkeyx"),
        ("Chef DSL", "chef-dsl"),
        ("Emacs Lisp", "elisp"),
        ("Guile Scheme", "guile"),
        ("Chez Scheme", "chez"),
        ("Chicken Scheme", "chicken"),
        ("Wolfram Language", "wolfram"),
        ("Max/MSP", "maxmsp"),
        ("Shakespeare Programming Language", "shakespeare"),
        ("Lazy K", "lazyk"),
        ("SKI combinator calculus", "ski"),
        ("Binary Lambda Calculus", "blc"),
        // IR/bytecode names whose natural slug intentionally differs from the
        // canonical filename to avoid collisions with source languages.
        (".NET CIL", "dotnet-cil"),
        ("F# IL", "fsharp-il"),
        ("AT&T assembly syntax", "att-assembly-syntax"),
        ("CIR", "cir-ir"),
        ("MLIR", "mlir-ir"),
        ("ANF", "anf-ir"),
        ("SPIR", "spir-ir"),
        ("DXIL", "dxil-ir"),
        ("DXBC", "dxbc-bytecode"),
        ("MSIL", "msil-bytecode"),
    ] {
        if name.eq_ignore_ascii_case(display) {
            return Some(canonical);
        }
    }
    None
}

fn directive_name(directive: &str) -> Option<&str> {
    directive.strip_prefix('#')?.split_whitespace().next()
}

fn contains_word(words: &[&str], word: &str, case_insensitive: bool) -> bool {
    if case_insensitive {
        words
            .iter()
            .any(|candidate| word.eq_ignore_ascii_case(candidate))
    } else {
        words.contains(&word)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_resolve_case_insensitively() {
        let typescript = Language::from_fence(" TypeScript ");
        assert!(typescript.javascript_lexing());
        assert!(typescript.is_keyword("interface"));
        assert!(Language::from_fence("python3").python_strings());
        assert!(Language::from_fence("CXX").preprocessor());
    }

    #[test]
    fn unknown_languages_keep_the_generic_profile() {
        let unknown = Language::from_fence("brainfuck");
        assert!(!unknown.is_keyword("while"));
        assert!(!unknown.block_comments());
    }

    #[test]
    fn external_pack_can_add_a_language_without_code_changes() {
        register_pack_source(
            "STREAMDOWN_LANGPACK\t1\naliases\tkotlin\tkt\nkeywords\tfun\tclass\nflags\tslash_comments\tblock_comments\n",
        )
        .unwrap();
        let kotlin = Language::from_fence("kt");
        assert!(kotlin.is_keyword("fun"));
        assert!(kotlin.slash_comments());
        assert!(kotlin.block_comments());
    }

    #[test]
    fn registry_aliases_are_unique_and_declarations_are_keywords() {
        for &(_, pack) in PACK_ALIASES {
            ensure_pack(pack);
        }
        LOADED.with(|loaded| {
            let loaded = loaded.borrow();
            for (profile_index, profile) in loaded.iter().enumerate() {
                assert!(!profile.aliases.is_empty());
                for alias in profile.aliases {
                    let duplicates = loaded
                        .iter()
                        .flat_map(|candidate| candidate.aliases.iter())
                        .filter(|candidate| alias.eq_ignore_ascii_case(candidate))
                        .count();
                    assert_eq!(duplicates, 1, "duplicate language alias: {alias}");
                }
                for declaration in profile
                    .function_declarations
                    .iter()
                    .chain(profile.type_declarations)
                    .chain(profile.macro_declarations)
                {
                    assert!(
                        contains_word(
                            profile.keywords,
                            declaration,
                            profile.case_insensitive_keywords
                        ),
                        "profile {profile_index} declaration is not a keyword: {declaration}"
                    );
                }
            }
        });
    }

    #[test]
    fn malformed_pack_is_rejected() {
        assert!(parse_pack("bad\n").is_err());
        assert!(parse_pack("STREAMDOWN_LANGPACK\t1\naliases\tx\nflags\tmagic\n").is_err());
    }

    #[test]
    fn dynamic_pack_names_are_sanitized() {
        assert_eq!(pack_name_for("Kotlin").as_deref(), Some("kotlin"));
        assert!(pack_name_for("../rust").is_none());
        assert!(pack_name_for("rust/../../x").is_none());
        assert!(pack_name_for(&"x".repeat(49)).is_none());
    }
}
