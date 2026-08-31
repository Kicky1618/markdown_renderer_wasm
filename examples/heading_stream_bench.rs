use std::time::Instant;
use streamdown::Parser;

fn main() {
    let n = 20_000;
    let mut parser = Parser::new();
    parser.append("## ");
    let started = Instant::now();
    for _ in 0..n {
        parser.append("token ");
    }
    let elapsed = started.elapsed();
    println!(
        "heading: {:?}, {:.0} append/s",
        elapsed,
        n as f64 / elapsed.as_secs_f64()
    );
}
