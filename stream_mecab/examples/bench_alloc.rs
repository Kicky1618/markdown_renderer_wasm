use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use stream_mecab::{FIRST_USER_TAG, Model, StreamDelta};

struct CountingAllocator;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static DEALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        DEALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn model() -> Model {
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
    model
}

fn main() {
    let chunks = ["今日", "は", "東京", "大学", "の", "学生", "です", "。"];
    let rounds = 10_000usize;
    let mut stream = model().stream();
    let mut delta = StreamDelta::default();
    for _ in 0..100 {
        for chunk in chunks {
            stream.append_into(chunk, &mut delta);
            black_box(&delta);
        }
    }
    ALLOCS.store(0, Ordering::Relaxed);
    DEALLOCS.store(0, Ordering::Relaxed);
    BYTES.store(0, Ordering::Relaxed);
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
        "{} appends {:?}: {:.3} M/s, {:.2} alloc/append, {:.1} B allocated/append, live_alloc_delta={}",
        appends,
        elapsed,
        appends as f64 / elapsed.as_secs_f64() / 1e6,
        ALLOCS.load(Ordering::Relaxed) as f64 / appends as f64,
        BYTES.load(Ordering::Relaxed) as f64 / appends as f64,
        ALLOCS.load(Ordering::Relaxed) as isize - DEALLOCS.load(Ordering::Relaxed) as isize,
    );
}
