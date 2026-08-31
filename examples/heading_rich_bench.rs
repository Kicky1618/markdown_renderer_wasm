use std::time::Instant;
use streamdown::Parser;

fn run(label: &str, prefix: &str, n: usize) {
    let mut parser = Parser::new();
    parser.append(prefix);
    let start = Instant::now();
    for _ in 0..n {
        parser.append("token ");
    }
    let elapsed = start.elapsed();
    println!(
        "{label}: {elapsed:?}, {:.0} append/s",
        n as f64 / elapsed.as_secs_f64()
    );
}

fn main() {
    let n = 20_000;
    run("plain-heading", "## ", n);
    run("strong-heading", "## **bold** ", n);
    run("link-heading", "## [x](u) ", n);
    run("code-heading", "## `x` ", n);
}
