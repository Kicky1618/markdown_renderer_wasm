use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use stream_mecab::{FIRST_USER_TAG, Model, StreamDelta};

struct CountingAllocator;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

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
    let mut stream = model.stream();
    let mut delta = StreamDelta::default();
    for _ in 0..1000 {
        for chunk in chunks {
            stream.append_into(chunk, &mut delta);
        }
    }
    let rounds = 1000usize;
    let mut counts = vec![(0usize, 0usize); chunks.len()];
    for _ in 0..rounds {
        for (i, chunk) in chunks.iter().enumerate() {
            ALLOCS.store(0, Ordering::Relaxed);
            BYTES.store(0, Ordering::Relaxed);
            stream.append_into(black_box(chunk), &mut delta);
            black_box(&delta);
            counts[i].0 += ALLOCS.load(Ordering::Relaxed);
            counts[i].1 += BYTES.load(Ordering::Relaxed);
        }
    }
    for (chunk, (allocs, bytes)) in chunks.into_iter().zip(counts) {
        println!(
            "{chunk:?}: {:.3} alloc, {:.1} B / append",
            allocs as f64 / rounds as f64,
            bytes as f64 / rounds as f64
        );
    }
}
