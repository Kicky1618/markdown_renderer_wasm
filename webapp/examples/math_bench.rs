#[path = "../src/math.rs"]
mod math;

use std::hint::black_box;
use std::time::{Duration, Instant};

use ratex_parser::{parse, Parser as RatexParser};

fn timed(mut f: impl FnMut()) -> Duration {
    let start = Instant::now();
    f();
    start.elapsed()
}

#[derive(Clone, Copy)]
struct RectLike {
    geometry: [f32; 4],
    color: u32,
    flags: u32,
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

    // Simulate streaming math: every prefix is a new MathImage cache key, while
    // glyphs already present in the previous prefix stay at the same positions.
    const RASTER_STREAM_N: usize = 128;
    let mut raster_source = String::with_capacity(RASTER_STREAM_N * 2);
    let streaming_raster = timed(|| {
        for i in 0..RASTER_STREAM_N {
            if i != 0 {
                raster_source.push('+');
            }
            raster_source.push('x');
            black_box(math::rasterize(black_box(raster_source.as_str()), true, 1.0).unwrap());
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

    // Approximate the WebGPU Scene::math_image hot path: a cached MathImage is
    // expanded into one RectInstance per horizontal run on every scene rebuild.
    let final_image = math::rasterize(&raster_source, true, 1.0).unwrap();
    let run_expand_n = 2_000usize;
    let mut instances = Vec::with_capacity(final_image.runs.len());
    let run_expand = timed(|| {
        for _ in 0..run_expand_n {
            instances.clear();
            for run in &final_image.runs {
                instances.push(RectLike {
                    geometry: [run.x as f32, run.y as f32, run.width as f32, 1.0],
                    color: run.packed_color(1.0),
                    flags: 2,
                });
            }
            black_box(instances.as_slice());
        }
    });
    black_box(instances.iter().fold(0u64, |sum, instance| {
        sum.wrapping_add(instance.color as u64)
            .wrapping_add(instance.flags as u64)
            .wrapping_add(instance.geometry[0].to_bits() as u64)
    }));

    println!(
        "RaTeX Parser::new:       {:8.3} us/op",
        ctor.as_secs_f64() * 1e6 / PARSE_N as f64
    );
    println!(
        "one-char parse:          {:8.3} us/op",
        one_parse.as_secs_f64() * 1e6 / PARSE_N as f64
    );
    println!(
        "short parse:             {:8.3} us/op",
        short_parse.as_secs_f64() * 1e6 / PARSE_N as f64
    );
    println!(
        "129-char plain parse:    {:8.3} us/op",
        long_parse.as_secs_f64() * 1e6 / PARSE_N as f64
    );
    println!(
        "macro parse:             {:8.3} us/op",
        macro_parse.as_secs_f64() * 1e6 / PARSE_N as f64
    );
    println!(
        "stream prefix parse:     {:8.3} ms / {STREAM_N} updates",
        streaming_parse.as_secs_f64() * 1e3
    );
    println!(
        "stream prefix raster:    {:8.3} ms / {RASTER_STREAM_N} updates",
        streaming_raster.as_secs_f64() * 1e3
    );
    println!(
        "cold rasterize:          {:8.3} ms/op",
        cold_raster.as_secs_f64() * 1e3 / cold_raster_n as f64
    );
    println!(
        "hot MathImage cache hit: {:8.3} ns/op",
        hot_raster.as_secs_f64() * 1e9 / hot_raster_n as f64
    );
    println!(
        "cached run expansion:    {:8.3} us/op, {} runs, {:6.2} ns/run",
        run_expand.as_secs_f64() * 1e6 / run_expand_n as f64,
        final_image.runs.len(),
        run_expand.as_secs_f64() * 1e9 / (run_expand_n * final_image.runs.len()) as f64
    );
}
