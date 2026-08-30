//! Lazy language profiles for the code-block highlighter.
//!
//! The scanner/parser in `code.rs` is language agnostic. Only the small
//! vocabulary/feature profile is language-specific. On wasm those profiles are
//! fetched from `langpacks/<name>.langpack` the first time a fenced language is
//! observed; until the fetch completes the block is rendered as plain text.

use std::cell::RefCell;

#[cfg(target_arch = "wasm32")]
use std::collections::HashSet;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::{JsFuture, spawn_local};
#[cfg(target_arch = "wasm32")]
use web_sys::{HtmlCanvasElement, Response};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeclarationKind {
    Function,
    Type,
    Macro,
}

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
        }
    };
}

const PLAIN: LanguageProfile = empty_profile!(&[]);
#[cfg(target_arch = "wasm32")]
const MAX_REQUESTED_PACKS: usize = 128;

// Aliases are intentionally tiny and stay in the initial wasm. The expensive
// keyword/type/macro dictionaries live only in the external langpacks.
const PACK_ALIASES: &[(&str, &str)] = &[
    ("rs", "rust"),
    ("rust", "rust"),
    ("js", "javascript"),
    ("javascript", "javascript"),
    ("node", "javascript"),
    ("mjs", "javascript"),
    ("cjs", "javascript"),
    ("jsx", "javascript"),
    ("ts", "javascript"),
    ("typescript", "javascript"),
    ("mts", "javascript"),
    ("cts", "javascript"),
    ("tsx", "javascript"),
    ("py", "python"),
    ("python", "python"),
    ("python3", "python"),
    ("sh", "shell"),
    ("bash", "shell"),
    ("shell", "shell"),
    ("zsh", "shell"),
    ("c", "cpp"),
    ("h", "cpp"),
    ("cpp", "cpp"),
    ("c++", "cpp"),
    ("cc", "cpp"),
    ("cxx", "cpp"),
    ("hpp", "cpp"),
    ("java", "java"),
    ("go", "go"),
    ("golang", "go"),
    ("json", "json"),
    ("jsonc", "json"),
    ("css", "css"),
    ("scss", "css"),
    ("sql", "sql"),
    ("postgres", "sql"),
    ("postgresql", "sql"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("toml", "toml"),
];

thread_local! {
    static LOADED: RefCell<Vec<&'static LanguageProfile>> = const { RefCell::new(Vec::new()) };
    #[cfg(target_arch = "wasm32")]
    static REQUESTED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Language(&'static LanguageProfile);

impl Language {
    pub(super) fn from_fence(name: &str) -> Self {
        let name = name.trim();
        if name.is_empty() {
            return Self(&PLAIN);
        }
        if let Some(profile) = find_loaded(name) {
            return Self(profile);
        }
        let Some(pack) = pack_name_for(name) else {
            return Self(&PLAIN);
        };
        ensure_pack(&pack);
        find_loaded(name).map_or(Self(&PLAIN), Self)
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

fn pack_name_for(name: &str) -> Option<String> {
    if let Some((_, pack)) = PACK_ALIASES
        .iter()
        .find(|(alias, _)| name.eq_ignore_ascii_case(alias))
    {
        return Some((*pack).to_owned());
    }
    // This also makes the loader extensible: dropping `kotlin.langpack` next to
    // the app is enough for a ```kotlin fence without rebuilding the wasm.
    let normalized = name.to_ascii_lowercase();
    (!normalized.is_empty()
        && normalized.len() <= 48
        && normalized
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
    .then_some(normalized)
}

#[cfg(not(target_arch = "wasm32"))]
fn ensure_pack(pack: &str) {
    if find_loaded(pack).is_some() {
        return;
    }
    if let Some(source) = embedded_pack(pack) {
        let _ = register_pack_source(source);
    }
}

#[cfg(target_arch = "wasm32")]
fn ensure_pack(pack: &str) {
    let pack = pack.to_owned();
    let should_request = REQUESTED.with(|requested| {
        let mut requested = requested.borrow_mut();
        if requested.contains(&pack) {
            return false;
        }
        if requested.len() >= MAX_REQUESTED_PACKS {
            return false;
        }
        requested.insert(pack.clone());
        true
    });
    if !should_request {
        return;
    }
    spawn_local(async move {
        let result = fetch_and_register(&pack).await;
        if let Err(error) = result {
            web_sys::console::warn_1(
                &format!("language pack {pack:?} unavailable: {error}").into(),
            );
        }
    });
}

#[cfg(target_arch = "wasm32")]
async fn fetch_and_register(pack: &str) -> Result<(), String> {
    let window = web_sys::window().ok_or_else(|| "window unavailable".to_owned())?;
    let url = format!("./langpacks/{pack}.langpack");
    let value = JsFuture::from(window.fetch_with_str(&url))
        .await
        .map_err(js_error)?;
    let response: Response = value
        .dyn_into()
        .map_err(|_| "fetch did not return a Response".to_owned())?;
    if !response.ok() {
        return Err(format!("HTTP {}", response.status()));
    }
    let text = JsFuture::from(response.text().map_err(js_error)?)
        .await
        .map_err(js_error)?
        .as_string()
        .ok_or_else(|| "langpack response was not text".to_owned())?;
    register_pack_source(&text)?;
    invalidate_renderer(pack);
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn js_error(value: wasm_bindgen::JsValue) -> String {
    value.as_string().unwrap_or_else(|| format!("{value:?}"))
}

#[cfg(target_arch = "wasm32")]
fn invalidate_renderer(_pack: &str) {
    // Both renderer backends already detect backing-canvas size changes every
    // animation frame. Perturbing the width by one pixel requests a full reflow
    // without coupling the language registry to either backend implementation.
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let Some(element) = document.get_element_by_id("app") else {
        return;
    };
    let Ok(canvas) = element.dyn_into::<HtmlCanvasElement>() else {
        return;
    };
    // Both backends detect a backing-store mismatch on the next animation
    // frame and rebuild their scene. The normal resize path immediately
    // restores the correct backing size, so CSS layout never changes.
    canvas.set_width(canvas.width().saturating_add(1));
}

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
    let profile = Box::leak(Box::new(profile));
    LOADED.with(|loaded| loaded.borrow_mut().push(profile));
    Ok(true)
}

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

fn leak_words(values: Vec<&str>) -> &'static [&'static str] {
    let words = values
        .into_iter()
        .map(|value| -> &'static str { Box::leak(value.to_owned().into_boxed_str()) })
        .collect::<Vec<_>>();
    Box::leak(words.into_boxed_slice())
}

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
        _ => return Err(format!("unknown langpack flag: {flag}")),
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn embedded_pack(name: &str) -> Option<&'static str> {
    Some(match name {
        "rust" => include_str!("../langpacks/rust.langpack"),
        "javascript" => include_str!("../langpacks/javascript.langpack"),
        "python" => include_str!("../langpacks/python.langpack"),
        "shell" => include_str!("../langpacks/shell.langpack"),
        "cpp" => include_str!("../langpacks/cpp.langpack"),
        "java" => include_str!("../langpacks/java.langpack"),
        "go" => include_str!("../langpacks/go.langpack"),
        "json" => include_str!("../langpacks/json.langpack"),
        "css" => include_str!("../langpacks/css.langpack"),
        "sql" => include_str!("../langpacks/sql.langpack"),
        "yaml" => include_str!("../langpacks/yaml.langpack"),
        "toml" => include_str!("../langpacks/toml.langpack"),
        _ => return None,
    })
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
