use std::{hint::black_box, time::Instant};
use stream_mecab::{FIRST_USER_TAG, Model};

fn main() {
    let mut model = Model::new();
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
    let rounds = 100_000usize;
    let start = Instant::now();
    let mut bytes = 0usize;
    let mut deltas = 0usize;
    for _ in 0..rounds {
        let mut stream = model.clone().stream();
        for chunk in chunks {
            bytes += chunk.len();
            let delta = stream.append(black_box(chunk));
            deltas += delta.push.len() + delta.retract;
        }
        stream.finish();
        black_box(stream.tokens());
    }
    let elapsed = start.elapsed();
    println!(
        "stream: {} appends in {:?} ({:.1} M append/s, {:.1} MiB/s), delta_ops={}",
        rounds * chunks.len(),
        elapsed,
        rounds as f64 * chunks.len() as f64 / elapsed.as_secs_f64() / 1e6,
        bytes as f64 / 1_048_576.0 / elapsed.as_secs_f64(),
        deltas
    );

    let text = "今日は東京大学の学生です。";
    let start = Instant::now();
    for _ in 0..rounds {
        black_box(model.tokenize(black_box(text)));
    }
    let elapsed = start.elapsed();
    println!(
        "batch: {rounds} parses in {:?} ({:.1} k parse/s)",
        elapsed,
        rounds as f64 / elapsed.as_secs_f64() / 1e3
    );
}
