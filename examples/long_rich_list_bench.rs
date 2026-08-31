use std::time::Instant;
use streamdown::Parser;

fn make(marker: &str, pat: &str, n: usize) -> String {
    let mut s = marker.to_owned();
    while s.len() < n { s.push_str(pat); }
    s.truncate(n);
    while !s.is_char_boundary(s.len()) { s.pop(); }
    s
}
fn run(marker: &str, pat: &str, n: usize) -> f64 {
    let text = make(marker, pat, n);
    let mut p = Parser::new();
    let t = Instant::now();
    for ch in text.chars() { let mut b=[0;4]; p.append(ch.encode_utf8(&mut b)); }
    t.elapsed().as_secs_f64()*1e6
}
fn main(){
 for (name,m,p) in [("ul-em","- ","*x* "),("ol-em","1. ","*x* "),("ul-link","- ","[x](u) "),("ul-mix","- ","**x** `y` [z](u) ")] {
   for n in [4000,8000,16000,32000] { println!("{name} bytes={n} us={:.3}",run(m,p,n)); }
 }
}
