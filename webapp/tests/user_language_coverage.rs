#[path = "../src/languages.rs"]
mod languages;

use languages::Language;

const REQUESTED: &str = include_str!("user_language_fences.tsv");

#[test]
fn user_requested_language_fences_resolve() {
    let requested = REQUESTED
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.split_once('\t')
                .unwrap_or_else(|| panic!("invalid requested-language row: {line:?}"))
        })
        .collect::<Vec<_>>();

    assert_eq!(requested.len(), 100);
    for (label, fence) in requested {
        assert!(
            !Language::from_fence(fence).is_plain(),
            "requested language {label} did not resolve fence {fence:?}"
        );
    }
}
