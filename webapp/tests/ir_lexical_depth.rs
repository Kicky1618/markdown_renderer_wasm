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

fn assert_has(source: &str, language: &str, text: &str, kind: TokenKind) {
    let got = tokens(source, language);
    assert!(
        got.iter().any(|(token, token_kind)| token == text && *token_kind == kind),
        "language={language} source={source:?} expected {text:?} as {kind:?}, got {got:?}",
    );
}

fn assert_not_comment(source: &str, language: &str, needle: &str) {
    let got = tokens(source, language);
    assert!(
        !got.iter().any(|(token, kind)| *kind == TokenKind::Comment && token.contains(needle)),
        "language={language} source={source:?} unexpectedly commented {needle:?}: {got:?}",
    );
}

#[test]
fn llvm_ir_keeps_attribute_groups_out_of_comments() {
    let source = "define i32 @f() #0 { ret i32 0 } ; trailing comment";
    assert_has(source, "LLVM IR", "define", TokenKind::Keyword);
    assert_has(source, "LLVM IR", "; trailing comment", TokenKind::Comment);
    assert_not_comment(source, "LLVM IR", "#0");
}

#[test]
fn mlir_hash_attributes_are_not_hash_comments() {
    let source = "#map = affine_map<(d0) -> (d0)>\narith.addi %a, %b : i32 // comment";
    assert_has(source, "MLIR", "arith", TokenKind::Keyword);
    assert_has(source, "MLIR", "// comment", TokenKind::Comment);
    assert_not_comment(source, "MLIR", "#map");
}

#[test]
fn circt_and_stablehlo_follow_mlir_comment_rules() {
    for language in ["CIRCT HW dialect", "StableHLO", "TensorFlow MLIR"] {
        let source = "#attr = 1 // comment";
        assert_has(source, language, "// comment", TokenKind::Comment);
        assert_not_comment(source, language, "#attr");
    }
}

#[test]
fn ptx_uses_c_style_comments_not_hash_or_semicolon_comments() {
    let source = ".entry kernel() { ld.global.u32 %r1, [%rd1]; // line\n/* block */ }";
    assert_has(source, "NVIDIA PTX", "ld", TokenKind::Keyword);
    assert_has(source, "NVIDIA PTX", "// line", TokenKind::Comment);
    assert_has(source, "NVIDIA PTX", "/* block */", TokenKind::Comment);
    assert_not_comment(source, "NVIDIA PTX", ";");
}

#[test]
fn spirv_assembly_uses_semicolon_comments() {
    let source = "OpCapability Shader ; capability";
    assert_has(source, "SPIR-V Assembly", "OpCapability", TokenKind::Keyword);
    assert_has(source, "SPIR-V Assembly", "; capability", TokenKind::Comment);
}

#[test]
fn smali_descriptor_semicolons_are_not_comments() {
    let source = "invoke-static {v0}, Ljava/lang/String;->valueOf(I)Ljava/lang/String; # tail";
    assert_has(source, "Smali", "invoke-static", TokenKind::Keyword);
    assert_has(source, "Smali", "# tail", TokenKind::Comment);
    assert_not_comment(source, "Smali", "->valueOf");
}

#[test]
fn qbe_and_cranelift_have_distinct_comment_markers() {
    assert_has("export function w $f() { # qbe", "QBE IL", "export", TokenKind::Keyword);
    assert_has("export function w $f() { # qbe", "QBE IL", "# qbe", TokenKind::Comment);
    assert_has("function %f() { ; clif", "Cranelift IR", "function", TokenKind::Keyword);
    assert_has("function %f() { ; clif", "Cranelift IR", "; clif", TokenKind::Comment);
}
