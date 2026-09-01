#[path = "../src/code.rs"]
mod code;

use code::TokenKind;

fn tokens(source: &str, language: &str) -> Vec<(String, TokenKind)> {
    let mut out = Vec::new();
    code::highlight(source, Some(language), |text, kind| {
        out.push((text.to_owned(), kind));
        true
    });
    out
}

fn has(tokens: &[(String, TokenKind)], text: &str, kind: TokenKind) -> bool {
    tokens.iter().any(|token| token == &(text.to_owned(), kind))
}

fn has_text(tokens: &[(String, TokenKind)], text: &str) -> bool {
    tokens.iter().any(|(candidate, _)| candidate == text)
}

fn source(tokens: &[(String, TokenKind)]) -> String {
    tokens.iter().map(|(text, _)| text.as_str()).collect()
}

#[test]
fn backticks_are_scoped_by_language() {
    for (language, code) in [
        ("javascript", "const x = `value ${n}`;"),
        ("go", "var x = `raw\\ntext`"),
        ("ruby", "output = `printf ok`"),
        ("shell", "value=`printf ok`"),
        ("perl", "my $x = `printf ok`;"),
        ("julia", "run(`echo ok`)"),
        ("jl", "run(`echo ok`)"),
    ] {
        let highlighted = tokens(code, language);
        assert!(
            highlighted
                .iter()
                .any(|(_, kind)| *kind == TokenKind::String),
            "{language}: {highlighted:?}"
        );
        assert_eq!(source(&highlighted), code);
    }

    for (language, code, identifier) in [
        ("kotlin", "val `when` = 1", "`when`"),
        ("kts", "val `when` = 1", "`when`"),
        ("scala", "val `type` = 1", "`type`"),
        ("sc", "val `type` = 1", "`type`"),
        ("swift", "let `class` = 1", "`class`"),
        ("r", "`odd name` <- 1", "`odd name`"),
        ("rlang", "`odd name` <- 1", "`odd name`"),
    ] {
        let highlighted = tokens(code, language);
        assert!(
            has_text(&highlighted, identifier),
            "{language}: {highlighted:?}"
        );
        assert!(!has(&highlighted, identifier, TokenKind::String));
        assert_eq!(source(&highlighted), code);
    }

    let haskell = tokens("value `seq` other", "haskell");
    assert!(has(&haskell, "`", TokenKind::Operator), "{haskell:?}");
    assert!(!haskell.iter().any(|(_, kind)| *kind == TokenKind::String));

    let ocaml = tokens("match x with `Foo -> 1 | `Bar -> 2", "ocaml");
    assert!(has(&ocaml, "`", TokenKind::Operator), "{ocaml:?}");
    assert!(!ocaml.iter().any(|(_, kind)| *kind == TokenKind::String));

    let postgres = tokens("SELECT `not_postgres_quote` FROM data", "postgresql");
    assert!(!postgres.iter().any(|(_, kind)| *kind == TokenKind::String));
}

#[test]
fn apostrophe_names_and_identifier_suffixes_do_not_open_fake_strings() {
    for (language, code, identifier) in [
        ("ocaml", "let map' f x = f x", "map'"),
        ("fsharp", "let map' f x = f x", "map'"),
        ("sml", "fun map' f x = f x", "map'"),
        ("haskell", "foldl' f z xs", "foldl'"),
    ] {
        let highlighted = tokens(code, language);
        assert!(
            has_text(&highlighted, identifier),
            "{language}: {highlighted:?}"
        );
        assert!(
            !highlighted
                .iter()
                .any(|(_, kind)| *kind == TokenKind::String)
        );
        assert_eq!(source(&highlighted), code);
    }

    for (language, code) in [
        ("ocaml", "type 'a box = Some of 'a option"),
        ("fsharp", "let inline id<'T> (x: 'T) = x"),
        ("sml", "val id : 'a -> 'a = fn x => x"),
        ("haskell", "name = 'value"),
        ("vhdl", "if clk'event then null; end if;"),
    ] {
        let highlighted = tokens(code, language);
        assert!(
            !highlighted
                .iter()
                .any(|(_, kind)| *kind == TokenKind::String),
            "{language}: {highlighted:?}"
        );
        assert_eq!(source(&highlighted), code);
    }

    let vhdl_char = tokens("if clk'event and clk = '1' then", "vhdl");
    assert!(has(&vhdl_char, "'1'", TokenKind::String), "{vhdl_char:?}");

    let haskell_char = tokens("char = 'x'; quoted = ''Maybe", "haskell");
    assert!(
        has(&haskell_char, "'x'", TokenKind::String),
        "{haskell_char:?}"
    );
    assert!(
        has(&haskell_char, "''", TokenKind::Operator),
        "{haskell_char:?}"
    );
}

#[test]
fn c_rust_swift_and_dotnet_literal_prefixes_are_language_scoped() {
    let cpp_source = "auto a = R\"tag(raw)tag\"; auto b = u8\"utf8\"; auto c = L\"wide\";";
    let cpp = tokens(cpp_source, "cpp");
    assert!(has(&cpp, "R\"tag(raw)tag\"", TokenKind::String), "{cpp:?}");
    assert!(has(&cpp, "u8\"utf8\"", TokenKind::String), "{cpp:?}");
    assert!(has(&cpp, "L\"wide\"", TokenKind::String), "{cpp:?}");

    let c_source = "const char *x = R\"(not-c-raw)\"; const char16_t *y = u\"ok\";";
    let c = tokens(c_source, "c");
    assert!(!has(&c, "R\"(not-c-raw)\"", TokenKind::String), "{c:?}");
    assert!(has(&c, "u\"ok\"", TokenKind::String), "{c:?}");

    let cuda = tokens("auto s = R\"(cuda raw)\";", "CUDA C++");
    assert!(has(&cuda, "R\"(cuda raw)\"", TokenKind::String), "{cuda:?}");

    let swift = tokens(
        "let raw = #\"swift raw\"#; let multiline = ##\"\"\"x\"\"\"##",
        "swift",
    );
    assert!(
        has(&swift, "#\"swift raw\"#", TokenKind::String),
        "{swift:?}"
    );
    assert!(
        has(&swift, "##\"\"\"x\"\"\"##", TokenKind::String),
        "{swift:?}"
    );

    let rust_source =
        "let b = b\"abc\"; let c = c\"ffi\"; let raw = br#\"bytes\"#; let craw = cr#\"ffi\"#;";
    let rust = tokens(rust_source, "rust");
    for literal in ["b\"abc\"", "c\"ffi\"", "br#\"bytes\"#", "cr#\"ffi\"#"] {
        assert!(
            has(&rust, literal, TokenKind::String),
            "missing {literal}: {rust:?}"
        );
    }

    let objc = tokens(
        "NSString *s = @\"hello\"; const char16_t *u = u\"ok\";",
        "objectivec",
    );
    assert!(has(&objc, "@\"hello\"", TokenKind::String), "{objc:?}");
    assert!(has(&objc, "u\"ok\"", TokenKind::String), "{objc:?}");
}

#[test]
fn csharp_and_fsharp_raw_and_interpolated_strings_stay_whole() {
    let csharp_source = r##"var a = $"hello {name}";
var b = @"c:\tmp";
var vb = @"a""b";
var c = """raw " content""";
var d = $"""value {name}""";
var e = $$"""value {{name}}""";
var f = """"contains """ quotes"""";"##;
    let csharp = tokens(csharp_source, "csharp");
    for literal in [
        "$\"hello {name}\"",
        "@\"c:\\tmp\"",
        "@\"a\"\"b\"",
        "\"\"\"raw \" content\"\"\"",
        "$\"\"\"value {name}\"\"\"",
        "$$\"\"\"value {{name}}\"\"\"",
        "\"\"\"\"contains \"\"\" quotes\"\"\"\"",
    ] {
        assert!(
            has(&csharp, literal, TokenKind::String),
            "missing {literal}: {csharp:?}"
        );
    }
    assert_eq!(source(&csharp), csharp_source);

    let fsharp_source = "$\"value {name}\"; $@\"c:\\tmp\"; $@\"a\"\"b\"; $\"\"\"multi {name}\"\"\"";
    let fsharp = tokens(fsharp_source, "fsharp");
    for literal in [
        "$\"value {name}\"",
        "$@\"c:\\tmp\"",
        "$\"\"\"multi {name}\"\"\"",
    ] {
        assert!(
            has(&fsharp, literal, TokenKind::String),
            "missing {literal}: {fsharp:?}"
        );
    }
}

#[test]
fn cpp_digit_separators_stay_inside_numeric_tokens() {
    let source_text = "auto n = 1'000'000; auto hex = 0xff'00u; auto bin = 0b1010'0011;";
    let highlighted = tokens(source_text, "cpp");
    for literal in ["1'000'000", "0xff'00u", "0b1010'0011"] {
        assert!(
            has(&highlighted, literal, TokenKind::Number),
            "missing {literal}: {highlighted:?}"
        );
    }
    assert_eq!(source(&highlighted), source_text);
}

#[test]
fn systemverilog_numbers_macros_and_cast_apostrophes_are_contextual() {
    let source_text = "`define WIDTH 8\nlogic [`WIDTH-1:0] x = 8'hFF; logic y = '1; logic [15:0] z = 'shx_FF; int q = int'(x); logic [3:0] p = '{default:'0}; `ifdef FOO\n`endif";
    let highlighted = tokens(source_text, "systemverilog");
    for literal in ["8'hFF", "'1", "'shx_FF", "'0"] {
        assert!(
            has(&highlighted, literal, TokenKind::Number),
            "missing {literal}: {highlighted:?}"
        );
    }
    for directive in ["`define", "`WIDTH", "`ifdef", "`endif"] {
        assert!(
            has(&highlighted, directive, TokenKind::Macro),
            "missing {directive}: {highlighted:?}"
        );
    }
    assert!(
        has(&highlighted, "'", TokenKind::Operator),
        "cast/pattern apostrophe missing: {highlighted:?}"
    );
    assert_eq!(source(&highlighted), source_text);
}

#[test]
fn julia_and_matlab_postfix_apostrophes_are_operators_not_strings() {
    let julia = tokens("adj = A'; ch = 'x'; nested = (A + B)'", "julia");
    assert!(has(&julia, "'", TokenKind::Operator), "{julia:?}");
    assert!(has(&julia, "'x'", TokenKind::String), "{julia:?}");

    let matlab = tokens("B = A'; C = A.'; s = 'text'; D = (A + B)'", "matlab");
    assert!(has(&matlab, "'", TokenKind::Operator), "{matlab:?}");
    assert!(has(&matlab, "'text'", TokenKind::String), "{matlab:?}");
    assert_eq!(source(&matlab), "B = A'; C = A.'; s = 'text'; D = (A + B)'");
}

#[test]
fn incomplete_extended_literals_remain_streamable() {
    for (language, code) in [
        ("csharp", "var x = $$\"\"\"unfinished {value}"),
        ("swift", "let x = ##\"unfinished"),
        ("cpp", "auto x = R\"tag(unfinished"),
        ("rust", "let x = cr##\"unfinished"),
    ] {
        let highlighted = tokens(code, language);
        assert_eq!(source(&highlighted), code, "{language}: {highlighted:?}");
        assert!(
            highlighted
                .iter()
                .any(|(_, kind)| *kind == TokenKind::String),
            "{language}: {highlighted:?}"
        );
    }
}

#[test]
fn postgres_dollar_quoted_strings_are_single_multiline_tokens() {
    let source_text =
        "DO $body$\nBEGIN\n  RAISE NOTICE 'hello';\nEND\n$body$; SELECT $$plain $ text$$;";
    let highlighted = tokens(source_text, "postgresql");
    assert!(
        has(
            &highlighted,
            "$body$\nBEGIN\n  RAISE NOTICE 'hello';\nEND\n$body$",
            TokenKind::String,
        ),
        "{highlighted:?}"
    );
    assert!(
        has(&highlighted, "$$plain $ text$$", TokenKind::String),
        "{highlighted:?}"
    );
    assert_eq!(source(&highlighted), source_text);

    let adjacent = tokens("SELECT foo$tag$not_a_quote$tag$ FROM t;", "postgresql");
    assert!(
        !adjacent.iter().any(|(_, kind)| *kind == TokenKind::String),
        "dollar quote after identifier must be separated: {adjacent:?}"
    );

    let javascript = tokens("const x = $tag$not_js$tag$;", "javascript");
    assert!(
        !javascript
            .iter()
            .any(|(text, kind)| text == "$tag$not_js$tag$" && *kind == TokenKind::String),
        "{javascript:?}"
    );
}

#[test]
fn shell_dollar_prefixed_quotes_are_language_scoped_and_multiline() {
    let source_text = "printf %s $'line\\nnext'; value=$\"hello $USER\"; multi=$'a\nb'";
    let highlighted = tokens(source_text, "bash");
    for literal in ["$'line\\nnext'", "$\"hello $USER\"", "$'a\nb'"] {
        assert!(
            has(&highlighted, literal, TokenKind::String),
            "missing {literal}: {highlighted:?}"
        );
    }
    assert_eq!(source(&highlighted), source_text);

    let rust = tokens("let x = $\"not shell\";", "rust");
    assert!(
        !rust
            .iter()
            .any(|(text, kind)| text == "$\"not shell\"" && *kind == TokenKind::String),
        "{rust:?}"
    );
}

#[test]
fn ocaml_quoted_strings_support_empty_and_named_delimiters() {
    let source_text =
        "let a = {|raw \" text\nnext|}\nlet b = {tag|contains |} but not close\nthen close|tag}";
    let highlighted = tokens(source_text, "ocaml");
    assert!(
        has(&highlighted, "{|raw \" text\nnext|}", TokenKind::String),
        "{highlighted:?}"
    );
    assert!(
        has(
            &highlighted,
            "{tag|contains |} but not close\nthen close|tag}",
            TokenKind::String,
        ),
        "{highlighted:?}"
    );
    assert_eq!(source(&highlighted), source_text);

    for invalid in [
        "let x = {Tag|not quoted|Tag}",
        "let x = {tag1|not quoted|tag1}",
    ] {
        let highlighted = tokens(invalid, "ocaml");
        assert!(
            !highlighted
                .iter()
                .any(|(_, kind)| *kind == TokenKind::String),
            "invalid quoted-string-id accepted: {highlighted:?}"
        );
    }

    let rust = tokens("let x = {tag|not ocaml|tag};", "rust");
    assert!(
        !rust
            .iter()
            .any(|(text, kind)| text == "{tag|not ocaml|tag}" && *kind == TokenKind::String),
        "{rust:?}"
    );
}

#[test]
fn newly_scoped_multiline_literals_recover_at_stream_end() {
    for (language, code) in [
        ("postgresql", "SELECT $tag$unfinished\nbody"),
        ("bash", "printf %s $'unfinished\nbody"),
        ("ocaml", "let x = {tag|unfinished\nbody"),
    ] {
        let highlighted = tokens(code, language);
        assert_eq!(source(&highlighted), code, "{language}: {highlighted:?}");
        assert!(
            highlighted
                .iter()
                .any(|(_, kind)| *kind == TokenKind::String),
            "{language}: {highlighted:?}"
        );
    }
}

#[test]
fn ruby_percent_literals_keep_nested_delimiters_and_regex_modifiers() {
    let source_text = "a = %q{raw {nested} text}; b = %Q[hello #{name}]; c = %w(one two); r = %r<foo/bar>im; cmd = %x(echo ok)";
    let highlighted = tokens(source_text, "ruby");
    for literal in [
        "%q{raw {nested} text}",
        "%Q[hello #{name}]",
        "%w(one two)",
        "%r<foo/bar>im",
        "%x(echo ok)",
    ] {
        assert!(
            has(&highlighted, literal, TokenKind::String),
            "missing {literal}: {highlighted:?}"
        );
    }
    assert_eq!(source(&highlighted), source_text);

    let modulo = tokens("x %= 2; y = x % q", "ruby");
    assert!(
        !modulo.iter().any(|(_, kind)| *kind == TokenKind::String),
        "modulo misread as percent literal: {modulo:?}"
    );
}

#[test]
fn elixir_sigils_support_all_delimiter_families_modifiers_and_heredocs() {
    let source_text = "r = ~r/foo|bar/iu\ns = ~s(hello (nested) #{name})\nw = ~w[one two]a\nraw = ~S\"\"\"\nno #{interp}\n\"\"\"\ndate = ~U[2026-09-01 12:34:56Z]";
    let highlighted = tokens(source_text, "elixir");
    for literal in [
        "~r/foo|bar/iu",
        "~s(hello (nested) #{name})",
        "~w[one two]a",
        "~S\"\"\"\nno #{interp}\n\"\"\"",
        "~U[2026-09-01 12:34:56Z]",
    ] {
        assert!(
            has(&highlighted, literal, TokenKind::String),
            "missing {literal}: {highlighted:?}"
        );
    }
    assert_eq!(source(&highlighted), source_text);

    let rust = tokens("let x = ~r/not_elixir/;", "rust");
    assert!(
        !rust
            .iter()
            .any(|(text, kind)| text == "~r/not_elixir/" && *kind == TokenKind::String),
        "{rust:?}"
    );
}

#[test]
fn php_heredoc_and_nowdoc_are_whole_strings_with_flexible_closers() {
    let source_text = "<?php\n$a = <<<END\nhello $name\n  END;\n$b = <<<'RAW'\nliteral $name and 'quotes'\n    RAW, $tail;\n$c = <<<\"DQ\"\nquoted opener\nDQ;";
    let highlighted = tokens(source_text, "php");
    for literal in [
        "<<<END\nhello $name\n  END",
        "<<<'RAW'\nliteral $name and 'quotes'\n    RAW",
        "<<<\"DQ\"\nquoted opener\nDQ",
    ] {
        assert!(
            has(&highlighted, literal, TokenKind::String),
            "missing {literal}: {highlighted:?}"
        );
    }
    assert_eq!(source(&highlighted), source_text);

    let cpp = tokens("auto x = a <<< b;", "cpp");
    assert!(
        !cpp.iter().any(|(_, kind)| *kind == TokenKind::String),
        "{cpp:?}"
    );
}

#[test]
fn ruby_elixir_and_php_extended_literals_recover_at_eof() {
    for (language, code) in [
        ("ruby", "x = %q{unfinished {nested}"),
        ("elixir", "x = ~S\"\"\"unfinished\nbody"),
        ("php", "$x = <<<'END'\nunfinished\nbody"),
    ] {
        let highlighted = tokens(code, language);
        assert_eq!(source(&highlighted), code, "{language}: {highlighted:?}");
        assert!(
            highlighted
                .iter()
                .any(|(_, kind)| *kind == TokenKind::String),
            "{language}: {highlighted:?}"
        );
    }
}

#[test]
fn r_raw_strings_support_bracket_families_and_dash_delimiters() {
    let source_text = "a = r\"(raw \\\\ and \\\" quotes)\"; b = R\"--[contains ] but closes later]--\"; c = r\"{line\\ntext}\"";
    let highlighted = tokens(source_text, "r");
    for literal in [
        "r\"(raw \\\\ and \\\" quotes)\"",
        "R\"--[contains ] but closes later]--\"",
        "r\"{line\\ntext}\"",
    ] {
        assert!(
            has(&highlighted, literal, TokenKind::String),
            "missing {literal}: {highlighted:?}"
        );
    }
    assert_eq!(source(&highlighted), source_text);

    let ordinary = tokens("x = r\"not raw\"", "r");
    assert!(
        !ordinary
            .iter()
            .any(|(text, kind)| text == "r\"not raw\"" && *kind == TokenKind::String),
        "{ordinary:?}"
    );
}

#[test]
fn haskell_quasiquotes_are_whole_multiline_strings_without_swallowing_th_quotes() {
    let source_text = "query = [sql|SELECT 'x'\nFROM t\nWHERE y = 1|]\nhtml = [Text.Shakespeare.hamlet|<b>ok</b>|]";
    let highlighted = tokens(source_text, "haskell");
    for literal in [
        "[sql|SELECT 'x'\nFROM t\nWHERE y = 1|]",
        "[Text.Shakespeare.hamlet|<b>ok</b>|]",
    ] {
        assert!(
            has(&highlighted, literal, TokenKind::String),
            "missing {literal}: {highlighted:?}"
        );
    }
    assert_eq!(source(&highlighted), source_text);

    let th_quote = tokens("x = [e| a + b |]", "haskell");
    assert!(
        !th_quote
            .iter()
            .any(|(text, kind)| text == "[e| a + b |]" && *kind == TokenKind::String),
        "{th_quote:?}"
    );
}

#[test]
fn groovy_dollar_slashy_strings_honor_dollar_escapes_and_multiline_content() {
    let source_text = "def x = $/\npath / raw\\backslash\nescaped close $/$$ still inside\n/$";
    let highlighted = tokens(source_text, "groovy");
    assert!(
        has(
            &highlighted,
            "$/\npath / raw\\backslash\nescaped close $/$$ still inside\n/$",
            TokenKind::String
        ),
        "{highlighted:?}"
    );
    assert_eq!(source(&highlighted), source_text);

    let javascript = tokens("const x = $/not_groovy/$;", "javascript");
    assert!(
        !javascript
            .iter()
            .any(|(text, kind)| text == "$/not_groovy/$" && *kind == TokenKind::String),
        "{javascript:?}"
    );
}

#[test]
fn r_haskell_and_groovy_extended_literals_recover_at_eof() {
    for (language, code) in [
        ("r", "x = r\"---(unfinished\nbody"),
        ("haskell", "x = [sql|unfinished\nbody"),
        ("groovy", "x = $/unfinished\nbody"),
    ] {
        let highlighted = tokens(code, language);
        assert_eq!(source(&highlighted), code, "{language}: {highlighted:?}");
        assert!(
            highlighted
                .iter()
                .any(|(_, kind)| *kind == TokenKind::String),
            "{language}: {highlighted:?}"
        );
    }
}
