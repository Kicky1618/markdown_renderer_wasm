use std::{hint::black_box, time::Instant};

#[path = "../../../webapp/src/code.rs"]
mod code;

const DEFAULT_TARGET_BYTES: usize = 4 * 1024 * 1024;

fn repeated_source(seed: &str, target_bytes: usize) -> String {
    let repeats = target_bytes.div_ceil(seed.len());
    seed.repeat(repeats)
}

fn benchmark(label: &str, language: &str, seed: &str, target_bytes: usize) {
    let source = repeated_source(seed, target_bytes);
    let mut warmup = 0usize;
    code::highlight(black_box(&source), Some(language), |text, kind| {
        warmup = warmup.wrapping_add(text.len()).wrapping_add(kind as usize);
        true
    });
    black_box(warmup);

    let start = Instant::now();
    let mut checksum = 0usize;
    code::highlight(black_box(&source), Some(language), |text, kind| {
        checksum = checksum
            .wrapping_add(text.len())
            .wrapping_add(kind as usize);
        true
    });
    let elapsed = start.elapsed();
    black_box(checksum);

    let mib = source.len() as f64 / 1_048_576.0;
    println!(
        "syntax-v2 {label}: {mib:.2} MiB in {elapsed:?} ({:.1} MiB/s)",
        mib / elapsed.as_secs_f64()
    );
}

fn main() {
    let target_bytes = std::env::var("SYNTAX_BENCH_BYTES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_TARGET_BYTES);

    let cases = [
        (
            "rust",
            "rust",
            "pub fn parse<'a>(input: &'a str) -> Result<Vec<u8>, Error> { let raw = cr#\"hello\"#; let n = 1_000_000u64; input.bytes().collect() } // comment\n",
        ),
        (
            "cpp",
            "cpp",
            "template<class T> auto parse(T value) -> std::vector<int> { auto raw = R\"tag(raw text)tag\"; auto n = 1'000'000ULL; return {1,2,3}; } // comment\n",
        ),
        (
            "javascript",
            "javascript",
            "export function parse(value) { const text = `item ${value}`; const re = /[a-z]+/gi; return value / 2 + text.length; } // comment\n",
        ),
        (
            "postgresql",
            "postgresql",
            "DO $body$ BEGIN RAISE NOTICE 'hello'; END $body$; SELECT id, name FROM items WHERE score >= 100 AND note = $$raw text$$; -- comment\n",
        ),
        (
            "ruby",
            "ruby",
            "def parse(value) text = %Q{item #{value}}; words = %w(one two three); re = %r<foo/bar>im; value.to_s + text end # comment\n",
        ),
    ];

    for (label, language, seed) in cases {
        benchmark(label, language, seed, target_bytes);
    }
}

// CI probe: syntax-suite shard only.
