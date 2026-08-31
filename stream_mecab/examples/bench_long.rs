use std::{hint::black_box, time::Instant};
use stream_mecab::{FIRST_USER_TAG, Model, StreamDelta};

fn main() {
    let mut model = Model::new();
    model.set_max_unknown_chars(4);
    let noun = FIRST_USER_TAG;
    let particle = FIRST_USER_TAG + 1;
    let aux = FIRST_USER_TAG + 2;
    for (surface, reading, tag, cost) in [
        ("今日", "キョウ", noun, 180),
        ("東京", "トウキョウ", noun, 180),
        ("大学", "ダイガク", noun, 180),
        ("東京大学", "トウキョウダイガク", noun, 80),
        ("学生", "ガクセイ", noun, 180),
        ("です", "デス", aux, 120),
        ("は", "ハ", particle, 100),
        ("の", "ノ", particle, 100),
    ] {
        model
            .add_entry(surface, surface, reading, tag, cost)
            .unwrap();
    }

    let chunks = ["今日", "は", "東京", "大学", "の", "学生", "です", "。"];
    let rounds = 20_000usize;
    let mut stream = model.stream();
    let mut delta = StreamDelta::default();
    let mut max_tail = 0usize;
    let start = Instant::now();
    for _ in 0..rounds {
        for chunk in chunks {
            stream.append_into(black_box(chunk), &mut delta);
            black_box(&delta);
            max_tail = max_tail.max(stream.tail_bytes());
        }
    }
    let elapsed = start.elapsed();
    black_box(stream.tokens());
    println!(
        "long-session: {} appends in {:?} ({:.3} M append/s), max_tail={}B committed={} published={}",
        rounds * chunks.len(),
        elapsed,
        rounds as f64 * chunks.len() as f64 / elapsed.as_secs_f64() / 1e6,
        max_tail,
        stream.committed_tokens(),
        stream.tokens().len(),
    );
}
