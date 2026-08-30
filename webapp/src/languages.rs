//! Declarative language registry for the code-block parser.
//!
//! Add a language by defining its vocabulary and one `LanguageProfile`, then
//! append that profile to `LANGUAGES`. Scanner and capture behavior should be
//! expressed through profile flags instead of language-name checks.

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

#[derive(Clone, Copy, Debug)]
pub(super) struct Language(&'static LanguageProfile);

impl Language {
    pub(super) fn from_fence(name: &str) -> Self {
        let name = name.trim();
        LANGUAGES
            .iter()
            .find(|profile| {
                profile
                    .aliases
                    .iter()
                    .any(|alias| name.eq_ignore_ascii_case(alias))
            })
            .map_or(Self(&PLAIN), Self)
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

macro_rules! profile {
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

const PLAIN: LanguageProfile = profile!(&[]);

const RUST: LanguageProfile = LanguageProfile {
    keywords: &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "macro", "match", "mod",
        "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
        "trait", "true", "type", "union", "unsafe", "use", "where", "while",
    ],
    builtin_types: &[
        "bool", "char", "str", "usize", "isize", "u8", "u16", "u32", "u64", "u128", "i8", "i16",
        "i32", "i64", "i128", "f32", "f64",
    ],
    function_declarations: &["fn"],
    type_declarations: &["struct", "enum", "trait", "union", "type"],
    macro_declarations: &["macro"],
    bang_macro_declarations: &["macro_rules"],
    slash_comments: true,
    block_comments: true,
    nested_block_comments: true,
    rust_syntax: true,
    multiline_strings: true,
    rust_attributes: true,
    bang_macros: true,
    macro_metavariables: true,
    ..profile!(&["rs", "rust"])
};

const JAVASCRIPT: LanguageProfile = LanguageProfile {
    keywords: &[
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "from",
        "function",
        "if",
        "implements",
        "import",
        "in",
        "instanceof",
        "interface",
        "keyof",
        "let",
        "new",
        "null",
        "of",
        "return",
        "static",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "type",
        "typeof",
        "undefined",
        "unknown",
        "var",
        "void",
        "while",
        "yield",
    ],
    builtin_types: &[
        "any", "bigint", "boolean", "never", "number", "object", "string", "symbol", "unknown",
    ],
    function_declarations: &["function"],
    type_declarations: &["class", "interface", "type"],
    expression_prefixes: &[
        "return",
        "throw",
        "case",
        "delete",
        "typeof",
        "void",
        "yield",
        "await",
        "new",
        "in",
        "of",
        "instanceof",
    ],
    slash_comments: true,
    block_comments: true,
    dollar_identifiers: true,
    javascript_lexing: true,
    ..profile!(&[
        "js",
        "javascript",
        "node",
        "mjs",
        "cjs",
        "jsx",
        "ts",
        "typescript",
        "mts",
        "cts",
        "tsx"
    ])
};

const PYTHON: LanguageProfile = LanguageProfile {
    keywords: &[
        "and", "as", "assert", "async", "await", "break", "case", "class", "continue", "def",
        "del", "elif", "else", "except", "False", "finally", "for", "from", "global", "if",
        "import", "in", "is", "lambda", "match", "None", "nonlocal", "not", "or", "pass", "raise",
        "return", "True", "try", "while", "with", "yield",
    ],
    builtin_types: &[
        "bool",
        "bytes",
        "dict",
        "float",
        "frozenset",
        "int",
        "list",
        "set",
        "str",
        "tuple",
    ],
    function_declarations: &["def"],
    type_declarations: &["class"],
    hash_comments: true,
    decorators: true,
    python_strings: true,
    ..profile!(&["py", "python", "python3"])
};

const SHELL: LanguageProfile = LanguageProfile {
    keywords: &[
        "case", "do", "done", "elif", "else", "esac", "fi", "for", "function", "if", "in",
        "select", "then", "time", "until", "while",
    ],
    function_declarations: &["function"],
    hash_comments: true,
    ..profile!(&["sh", "bash", "shell", "zsh"])
};

const C_FAMILY: LanguageProfile = LanguageProfile {
    keywords: &[
        "auto",
        "break",
        "case",
        "class",
        "const",
        "constexpr",
        "continue",
        "default",
        "do",
        "else",
        "enum",
        "false",
        "for",
        "friend",
        "if",
        "namespace",
        "new",
        "private",
        "protected",
        "public",
        "register",
        "reinterpret_cast",
        "return",
        "sizeof",
        "static",
        "static_assert",
        "static_cast",
        "struct",
        "switch",
        "this",
        "thread_local",
        "throw",
        "true",
        "try",
        "typedef",
        "union",
        "using",
        "virtual",
        "volatile",
        "while",
    ],
    builtin_types: &[
        "bool", "char", "double", "float", "int", "long", "short", "signed", "unsigned", "void",
        "wchar_t", "size_t",
    ],
    type_declarations: &["class", "struct", "enum", "union"],
    slash_comments: true,
    block_comments: true,
    preprocessor: true,
    preprocessor_macro_operands: &["define", "ifdef", "ifndef", "undef"],
    preprocessor_headers: &["include", "include_next", "import"],
    macro_identifiers: &[
        "defined",
        "__has_include",
        "__has_include_next",
        "__has_cpp_attribute",
        "__has_builtin",
        "__has_feature",
        "__cplusplus",
    ],
    macro_operand_identifiers: &["defined"],
    header_macro_identifiers: &["__has_include", "__has_include_next"],
    uppercase_macros: true,
    ..profile!(&["c", "h", "cpp", "c++", "cc", "cxx", "hpp"])
};

const JAVA: LanguageProfile = LanguageProfile {
    keywords: &[
        "abstract",
        "assert",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "default",
        "do",
        "else",
        "enum",
        "extends",
        "false",
        "final",
        "finally",
        "for",
        "if",
        "implements",
        "import",
        "instanceof",
        "interface",
        "native",
        "new",
        "null",
        "package",
        "private",
        "protected",
        "public",
        "return",
        "static",
        "strictfp",
        "super",
        "switch",
        "synchronized",
        "this",
        "throw",
        "throws",
        "transient",
        "true",
        "try",
        "volatile",
        "while",
    ],
    builtin_types: &[
        "boolean", "byte", "char", "double", "float", "int", "long", "short", "void",
    ],
    type_declarations: &["class", "interface", "enum"],
    slash_comments: true,
    block_comments: true,
    ..profile!(&["java"])
};

const GO: LanguageProfile = LanguageProfile {
    keywords: &[
        "break",
        "case",
        "chan",
        "const",
        "continue",
        "default",
        "defer",
        "else",
        "fallthrough",
        "false",
        "for",
        "func",
        "go",
        "goto",
        "if",
        "import",
        "interface",
        "iota",
        "map",
        "nil",
        "package",
        "range",
        "return",
        "select",
        "struct",
        "switch",
        "true",
        "type",
        "var",
    ],
    builtin_types: &[
        "any",
        "bool",
        "byte",
        "complex64",
        "complex128",
        "error",
        "float32",
        "float64",
        "int",
        "int8",
        "int16",
        "int32",
        "int64",
        "rune",
        "string",
        "uint",
        "uint8",
        "uint16",
        "uint32",
        "uint64",
        "uintptr",
    ],
    function_declarations: &["func"],
    type_declarations: &["type"],
    slash_comments: true,
    block_comments: true,
    ..profile!(&["go", "golang"])
};

const JSON: LanguageProfile = LanguageProfile {
    keywords: &["true", "false", "null"],
    ..profile!(&["json", "jsonc"])
};

const CSS: LanguageProfile = LanguageProfile {
    slash_comments: true,
    block_comments: true,
    ..profile!(&["css", "scss"])
};

const SQL: LanguageProfile = LanguageProfile {
    keywords: &[
        "select", "from", "where", "insert", "into", "values", "update", "delete", "create",
        "alter", "drop", "table", "index", "join", "left", "right", "inner", "outer", "on", "as",
        "and", "or", "not", "null", "true", "false", "group", "by", "order", "having", "limit",
        "offset", "union", "all", "distinct", "case", "when", "then", "else", "end",
    ],
    case_insensitive_keywords: true,
    dash_comments: true,
    block_comments: true,
    ..profile!(&["sql", "postgres", "postgresql"])
};

const YAML: LanguageProfile = LanguageProfile {
    hash_comments: true,
    ..profile!(&["yaml", "yml"])
};

const TOML: LanguageProfile = LanguageProfile {
    hash_comments: true,
    ..profile!(&["toml"])
};

const LANGUAGES: &[LanguageProfile] = &[
    RUST, JAVASCRIPT, PYTHON, SHELL, C_FAMILY, JAVA, GO, JSON, CSS, SQL, YAML, TOML,
];

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
    fn registry_aliases_are_unique_and_declarations_are_keywords() {
        for (profile_index, profile) in LANGUAGES.iter().enumerate() {
            assert!(!profile.aliases.is_empty());
            for alias in profile.aliases {
                let duplicates = LANGUAGES
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
    }
}
