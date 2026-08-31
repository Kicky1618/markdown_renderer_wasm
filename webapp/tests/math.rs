#[path = "../src/math.rs"]
mod math;

use ratex_layout::{layout, to_display_list, LayoutOptions};
use ratex_parser::parse;
use ratex_render::{
    render_to_rgba_premultiplied, render_to_rgba_premultiplied_append, PremultipliedRgbaImage,
    RenderOptions,
};
use ratex_types::{color::Color, display_item::DisplayList};

#[test]
fn supersampled_math_retains_partial_coverage() {
    let image = math::rasterize(r"x^2+y^2=25", true, 1.0).expect("RaTeX image");
    assert!(image.width > 20 && image.height > 8);
    assert!(image
        .pixels
        .chunks_exact(4)
        .any(|pixel| pixel[3] > 0 && pixel[3] < 255));
}

fn incremental_display_list(source: &str) -> DisplayList {
    let nodes = parse(source).expect("RaTeX parse");
    to_display_list(&layout(
        &nodes,
        &LayoutOptions::default().with_color(Color::new(0.92, 0.96, 1.0, 1.0)),
    ))
}

fn incremental_render_options() -> RenderOptions {
    RenderOptions {
        font_size: 16.0,
        padding: 2.0,
        background_color: Color::new(0.0, 0.0, 0.0, 0.0),
        font_dir: String::new(),
        device_pixel_ratio: 2.0,
    }
}

fn assert_active_pixels_eq(actual: &PremultipliedRgbaImage, expected: &PremultipliedRgbaImage) {
    assert_eq!(
        (actual.width, actual.height),
        (expected.width, expected.height)
    );
    for y in 0..actual.height {
        let actual_start = (y * actual.stride * 4) as usize;
        let expected_start = (y * expected.stride * 4) as usize;
        let row_bytes = (actual.width * 4) as usize;
        assert_eq!(
            &actual.pixels[actual_start..actual_start + row_bytes],
            &expected.pixels[expected_start..expected_start + row_bytes],
            "active raster differs on row {y}"
        );
    }
}

#[test]
fn incremental_raster_matches_full_render_across_capacity_growth() {
    let options = incremental_render_options();
    // A single `x` has different vertical metrics from `x+x`; start from the
    // stable two-term layout so every following update is an append-only list.
    let mut source = String::from("x+x");
    let mut previous_display_list = incremental_display_list(&source);
    let mut previous_image =
        render_to_rgba_premultiplied(&previous_display_list, &options).expect("initial raster");

    for _ in 2..32 {
        source.push_str("+x");
        let display_list = incremental_display_list(&source);
        let full = render_to_rgba_premultiplied(&display_list, &options).expect("full raster");
        let incremental = render_to_rgba_premultiplied_append(
            &previous_display_list,
            previous_image,
            &display_list,
            &options,
        )
        .expect("incremental raster")
        .expect("prefix should be reusable");
        assert_active_pixels_eq(&incremental, &full);
        previous_display_list = display_list;
        previous_image = incremental;
    }
}

#[test]
fn incremental_raster_falls_back_at_tiny_skia_8191_boundary() {
    let options = incremental_render_options();
    let previous_source = std::iter::repeat_n(r"\sqrt{x}", 97)
        .collect::<Vec<_>>()
        .join("+");
    let current_source = std::iter::repeat_n(r"\sqrt{x}", 98)
        .collect::<Vec<_>>()
        .join("+");
    let previous_display_list = incremental_display_list(&previous_source);
    let current_display_list = incremental_display_list(&current_source);
    let previous_image =
        render_to_rgba_premultiplied(&previous_display_list, &options).expect("previous raster");
    let current_full =
        render_to_rgba_premultiplied(&current_display_list, &options).expect("current raster");

    assert!(previous_image.width <= 8191, "expected pre-tile raster");
    assert!(current_full.width > 8191, "expected tiled raster");
    assert!(
        render_to_rgba_premultiplied_append(
            &previous_display_list,
            previous_image,
            &current_display_list,
            &options,
        )
        .expect("boundary decision")
        .is_none(),
        "crossing tiny-skia's 8191px raster mode boundary must force a full render"
    );
}
