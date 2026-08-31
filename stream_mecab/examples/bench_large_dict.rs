use std::{hint::black_box, time::Instant};
use stream_mecab::{FIRST_USER_TAG, Model, StreamDelta};

fn surface(mut value: usize) -> String {
    // 2048 CJK code points produce realistic UTF-8 trie fan-out without using
    // any external dictionary data.
    let mut out = String::with_capacity(12);
    for _ in 0..4 {
        let ch = char::from_u32(0x4e00 + (value & 0x7ff) as u32).unwrap();
        out.push(ch);
        value >>= 11;
    }
    out
}

fn main() {
    const WORDS: usize = 100_000;
    let build_start = Instant::now();
    let mut model = Model::new();
    model.set_max_unknown_chars(4);
    for i in 0..WORDS {
        let word = surface(i);
        model
            .add_entry(&word, &word, "", FIRST_USER_TAG + (i % 32) as u16, (i % 600) as i32)
            .unwrap();
    }
    let build = build_start.elapsed();
    let stats = model.stats();

    let chunks: Vec<String> = (0..256).map(|i| surface(i * 313 % WORDS)).collect();
    let mut stream = model.stream_delta();
    let mut delta = StreamDelta::default();
    for _ in 0..10 {
        for chunk in &chunks {
            stream.append_into(chunk, &mut delta);
        }
    }
    let rounds = 200;
    let start = Instant::now();
    for _ in 0..rounds {
        for chunk in &chunks {
            stream.append_into(black_box(chunk), &mut delta);
            black_box(&delta);
        }
    }
    let elapsed = start.elapsed();
    let appends = rounds * chunks.len();
    println!(
        "large-dict: words={WORDS} build={build:?} appends={appends} elapsed={elapsed:?} throughput={:.3} M/s tail={}B buffered={} trie_nodes={} dense_nodes={} dense_KiB={:.1}",
        appends as f64 / elapsed.as_secs_f64() / 1e6,
        stream.tail_bytes(),
        stream.buffered_tokens(),
        stats.trie_nodes,
        stats.dense_dispatch_nodes,
        stats.dense_dispatch_bytes as f64 / 1024.0,
    );
}
