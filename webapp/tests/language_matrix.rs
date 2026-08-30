#[path = "../src/code.rs"]
mod code;

use code::TokenKind;
use std::{fs, path::PathBuf};

fn highlight(source: &str, language: &str) -> Vec<(String, TokenKind)> {
    let mut out = Vec::new();
    code::highlight(source, Some(language), |text, kind| {
        out.push((text.to_owned(), kind));
        true
    });
    out
}

fn pack_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("langpacks")
}

#[test]
fn every_pack_and_alias_resolves_to_its_keyword_profile() {
    let mut checked_packs = 0usize;
    let mut checked_aliases = 0usize;

    for entry in fs::read_dir(pack_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("langpack") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        let mut aliases = None;
        let mut keyword = None;
        for line in source.lines() {
            let mut fields = line.split('\t');
            match fields.next() {
                Some("aliases") => aliases = Some(fields.map(str::to_owned).collect::<Vec<_>>()),
                Some("keywords") => keyword = fields.next().map(str::to_owned),
                _ => {}
            }
        }

        let aliases = aliases.unwrap_or_else(|| panic!("{} has no aliases", path.display()));
        if let Some(keyword) = keyword {
            checked_packs += 1;
            for alias in aliases {
                let tokens = highlight(&keyword, &alias);
                assert!(
                    tokens.iter().any(|(text, kind)| text == &keyword && *kind == TokenKind::Keyword),
                    "alias {alias:?} did not resolve {} to keyword {keyword:?}",
                    path.display()
                );
                checked_aliases += 1;
            }
        }
    }

    assert!(checked_packs >= 50, "expected broad language coverage, got {checked_packs}");
    assert!(checked_aliases >= 150, "expected broad alias coverage, got {checked_aliases}");
}

#[test]
fn expanded_comment_markers_are_language_scoped() {
    let cases = [
        ("clojure", "; comment"),
        ("erlang", "% comment"),
        ("vbnet", "' comment"),
        ("fortran", "! comment"),
    ];
    for (language, source) in cases {
        let tokens = highlight(source, language);
        assert_eq!(tokens, vec![(source.to_owned(), TokenKind::Comment)], "{language}");
    }

    let rust = highlight("value != 0;", "rust");
    assert!(!rust.iter().any(|(_, kind)| *kind == TokenKind::Comment));
}

#[test]
fn lisp_and_ruby_identifier_suffixes_stay_whole() {
    let clojure = highlight("defn- ready?", "clj");
    assert!(clojure.iter().any(|(text, kind)| text == "defn-" && *kind == TokenKind::Keyword));
    assert!(clojure.iter().any(|(text, _)| text == "ready?"));

    let ruby = highlight("save!() valid?()", "rb");
    assert!(ruby.iter().any(|(text, kind)| text == "save!" && *kind == TokenKind::Function));
    assert!(ruby.iter().any(|(text, kind)| text == "valid?" && *kind == TokenKind::Function));
}

#[test]
fn nested_non_c_block_comments_follow_language_rules() {
    let haskell = "{- outer {- nested -} tail -}";
    assert_eq!(
        highlight(haskell, "hs"),
        vec![(haskell.to_owned(), TokenKind::Comment)]
    );

    let ocaml = "(* outer (* nested *) tail *)";
    assert_eq!(
        highlight(ocaml, "ocaml"),
        vec![(ocaml.to_owned(), TokenKind::Comment)]
    );

    let fsharp = "(* outer (* nested *) tail *)";
    assert_eq!(
        highlight(fsharp, "fs"),
        vec![(fsharp.to_owned(), TokenKind::Comment)]
    );
}

#[test]
fn triple_quoted_strings_are_enabled_only_for_languages_that_use_them() {
    let kotlin = "\"\"\"hello\nworld\"\"\"";
    assert_eq!(
        highlight(kotlin, "kt"),
        vec![(kotlin.to_owned(), TokenKind::String)]
    );

    let dart = "'''hello\nworld'''";
    assert_eq!(
        highlight(dart, "dart"),
        vec![(dart.to_owned(), TokenKind::String)]
    );

    let kotlin_single = "'''not a kotlin multiline string'''";
    let tokens = highlight(kotlin_single, "kotlin");
    assert_ne!(tokens, vec![(kotlin_single.to_owned(), TokenKind::String)]);
}
