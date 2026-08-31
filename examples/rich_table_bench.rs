use std::time::Instant;
use streamdown::Parser;

fn run(name: &str, unit: &str, target: usize) {
    let mut s = String::new();
    s.push_str("a|b\n---|---\n");
    while s.len() < target {
        s.push_str(unit);
    }
    s.truncate(target.min(s.len()));
    let mut p = Parser::new();
    let t = Instant::now();
    for ch in s.chars() {
        let mut b = [0; 4];
        p.append(ch.encode_utf8(&mut b));
    }
    println!(
        "{name} bytes={} us={:.3}",
        s.len(),
        t.elapsed().as_secs_f64() * 1e6
    );
}
fn main() {
    for (name, unit) in [
        ("plain", "x|y\n"),
        ("em", "*x*|y\n"),
        ("strong", "**x**|y\n"),
        ("code", "`x`|y\n"),
        ("link", "[x](u)|y\n"),
        ("semantic", "@[source:x]|y\n"),
        ("rich2", "*x*|[y](u)\n"),
    ] {
        for n in [8000usize, 16000, 32000, 64000] {
            run(name, unit, n);
        }
        println!();
    }
}
