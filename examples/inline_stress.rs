use std::{hint::black_box, time::Instant};
use streamdown::Parser;

fn run(label: &str, input: String) {
    let mut parser = Parser::new();
    let bytes = input.len();
    let start = Instant::now();
    black_box(parser.append(&input));
    let elapsed = start.elapsed();
    println!(
        "{label}: {bytes} bytes in {elapsed:?} ({:.1} MiB/s)",
        bytes as f64 / 1_048_576.0 / elapsed.as_secs_f64()
    );
}

fn main() {
    let n = std::env::var("N")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(20_000usize);
    run("unclosed brackets", "[".repeat(n));
    run("broken destinations", "[x](".repeat(n / 4));
    let mut late_broken = "[".repeat(n);
    late_broken.push_str("](");
    run("late broken link", late_broken);
    run("dollar-heavy", "$x".repeat(n / 2));
    run("unclosed links", "[label".repeat(n / 6));
}
