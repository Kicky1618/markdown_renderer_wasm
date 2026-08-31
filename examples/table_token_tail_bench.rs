use std::time::Instant;
use streamdown::Parser;

fn run(prefix: &str, n: usize) {
    let mut p = Parser::new();
    p.append("a|b\n---|---\n");
    p.append(prefix);
    let t = Instant::now();
    for _ in 0..n {
        p.append("token ");
    }
    let d = t.elapsed();
    println!(
        "{prefix:?} n={n}: {:?}, {:.0} append/s",
        d,
        n as f64 / d.as_secs_f64()
    );
}
fn main() {
    let n = 20_000;
    run("x | ", n);
    run("**x** | ", n);
    run("[x](u) | ", n);
}
