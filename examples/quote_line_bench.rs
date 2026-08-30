use std::time::Instant;
use streamdown::Parser;

fn main() {
    let n = 5_000;
    let mut parser = Parser::new();
    parser.append("> seed\n");
    let started = Instant::now();
    for _ in 0..n {
        parser.append("> token token token\n");
    }
    let elapsed = started.elapsed();
    println!(
        "quote-line: {:?}, {:.0} append/s",
        elapsed,
        n as f64 / elapsed.as_secs_f64()
    );
}
