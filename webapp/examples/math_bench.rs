#[path = "../src/math.rs"]
mod math;

use std::hint::black_box;
use std::time::{Duration, Instant};

use ratex_parser::{Parser as RatexParser, parse};

fn timed(mut f: impl FnMut()) -> Duration {
    let start = Instant::now();
    f();
    start.elapsed()
}

fn main() {
    const PARSE_N: usize = 2_000;
    const STREAM_N: usize = 256;

    // Warm lazy/static data outside the measurements.
    black_box(parse(r"x^2+y^2=25").unwrap());
    black_box(math::rasterize(r"x^2+y^2=25", true, 1.0).unwrap());

    let ctor = timed(|| {
        for _ in 0..PARSE_N {
            black_box(RatexParser::new(black_box(r"x^2+y^2=25")));
        }
    });
    let one_parse = timed(|| {
        for _ in 0..PARSE_N {
            black_box(parse(black_box("x")).unwrap());
        }
    });
    let short_parse = timed(|| {
        for _ in 0..PARSE_N {
            black_box(parse(black_box(r"x^2+y^2=25")).unwrap());
        }
    });
    let long_plain = "x+".repeat(64) + "x";
    let long_parse = timed(|| {
        for _ in 0..PARSE_N {
            black_box(parse(black_box(long_plain.as_str())).unwrap());
        }
    });
    let macro_parse = timed(|| {
        for _ in 0..PARSE_N {
            black_box(parse(black_box(r"\frac{-b\pm\sqrt{b^2-4ac}}{2a}")).unwrap());
        }
    });

    let mut source = String::with_capacity(STREAM_N * 2);
    let streaming_parse = timed(|| {
        for i in 0..STREAM_N {
            if i != 0 {
                source.push('+');
            }
            source.push('x');
            black_box(parse(black_box(source.as_str())).unwrap());
        }
    });

    // Use unique expressions so MathImage cache cannot turn this into a hot-cache benchmark.
    let cold_raster_n = 32usize;
    let cold_raster = timed(|| {
        for i in 0..cold_raster_n {
            let expression = format!(r"x_{{{i}}}^2+y_{{{i}}}^2={}", i * i);
            black_box(math::rasterize(black_box(&expression), true, 1.0).unwrap());
        }
    });

    let hot_raster_n = 20_000usize;
    let hot_raster = timed(|| {
        for _ in 0..hot_raster_n {
            black_box(math::rasterize(black_box(r"x^2+y^2=25"), true, 1.0).unwrap());
        }
    });

    println!("RaTeX Parser::new:       {:8.3} us/op", ctor.as_secs_f64() * 1e6 / PARSE_N as f64);
    println!("one-char parse:          {:8.3} us/op", one_parse.as_secs_f64() * 1e6 / PARSE_N as f64);
    println!("short parse:             {:8.3} us/op", short_parse.as_secs_f64() * 1e6 / PARSE_N as f64);
    println!("129-char plain parse:    {:8.3} us/op", long_parse.as_secs_f64() * 1e6 / PARSE_N as f64);
    println!("macro parse:             {:8.3} us/op", macro_parse.as_secs_f64() * 1e6 / PARSE_N as f64);
    println!("stream prefix parse:     {:8.3} ms / {STREAM_N} updates", streaming_parse.as_secs_f64() * 1e3);
    println!("cold rasterize:          {:8.3} ms/op", cold_raster.as_secs_f64() * 1e3 / cold_raster_n as f64);
    println!("hot MathImage cache hit: {:8.3} ns/op", hot_raster.as_secs_f64() * 1e9 / hot_raster_n as f64);
}
