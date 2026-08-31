use std::{hint::black_box, time::Instant};
use stream_mecab::{FIRST_USER_TAG, Model, StreamDelta, TAG_BOS_EOS};

fn main() {
    const TAGS: usize = 16;
    let mut model = Model::new();
    model.set_max_unknown_chars(4);
    let words = ["今日", "東京", "大学", "学生", "研究", "解析", "日本", "です"];
    for (word_index, word) in words.iter().enumerate() {
        for tag_index in 0..TAGS {
            let tag = FIRST_USER_TAG + tag_index as u16;
            model
                .add_entry(
                    *word,
                    format!("{word}-{tag_index}"),
                    "",
                    tag,
                    ((word_index * 17 + tag_index * 31) % 400) as i32,
                )
                .unwrap();
        }
    }
    for previous in TAG_BOS_EOS..FIRST_USER_TAG + TAGS as u16 {
        for next in TAG_BOS_EOS..FIRST_USER_TAG + TAGS as u16 {
            let cost = ((previous as i32 * 37 + next as i32 * 19) % 401) - 200;
            model.set_transition(previous, next, cost);
        }
    }

    let chunks = words;
    let mut stream = model.stream_delta();
    let stats = stream.model_stats();
    let mut delta = StreamDelta::default();
    for _ in 0..1000 {
        for chunk in chunks {
            stream.append_into(chunk, &mut delta);
        }
    }
    let rounds = 20_000usize;
    let start = Instant::now();
    for _ in 0..rounds {
        for chunk in chunks {
            stream.append_into(black_box(chunk), &mut delta);
            black_box(&delta);
        }
    }
    let elapsed = start.elapsed();
    let appends = rounds * chunks.len();
    println!(
        "transitions: {appends} appends in {elapsed:?} ({:.3} M append/s), buffered={} tail={}B transition_KiB={:.1}",
        appends as f64 / elapsed.as_secs_f64() / 1e6,
        stream.buffered_tokens(),
        stream.tail_bytes(),
        stats.runtime_transition_bytes as f64 / 1024.0,
    );
}
