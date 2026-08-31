use std::time::Instant;
use streamdown::Parser;

fn run(chunk: &str, n: usize) {
    let mut parser = Parser::new();
    parser.append("## seed ");
    let started = Instant::now();
    for _ in 0..n {
        parser.append(chunk);
    }
    let elapsed = started.elapsed();
    println!(
        "{chunk:?}: {:?}, {:.0} append/s",
        elapsed,
        n as f64 / elapsed.as_secs_f64()
    );
}

fn main() {
    let n = 2_000;
    for chunk in ["**x** ", "[x](u) ", "`x` ", "[[cite:d|x]] "] {
        run(chunk, n);
    }
}
