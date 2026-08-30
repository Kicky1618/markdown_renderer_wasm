#[path = "../src/languages.rs"]
mod languages;

use languages::Language;

const REQUESTED_IR: &str = include_str!("ir_requested_names.tsv");

#[test]
fn every_requested_ir_bytecode_and_isa_name_resolves() {
    let mut checked = 0usize;
    let mut missing = Vec::new();
    for line in REQUESTED_IR.lines().filter(|line| !line.is_empty()) {
        let (display, canonical) = line
            .split_once('\t')
            .unwrap_or_else(|| panic!("invalid IR catalog row: {line:?}"));
        if Language::from_fence(display).is_plain() {
            missing.push(format!("{display} -> {canonical}"));
        }
        assert!(
            !Language::from_fence(canonical).is_plain(),
            "canonical IR pack is missing: {canonical}"
        );
        checked += 1;
    }
    assert_eq!(checked, 233, "IR catalog size changed unexpectedly");
    assert!(
        missing.is_empty(),
        "unresolved requested IR names: {missing:?}"
    );
}
