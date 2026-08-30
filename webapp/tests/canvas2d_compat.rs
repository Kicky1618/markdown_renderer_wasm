#[path = "../src/canvas2d/compat.rs"]
mod canvas_compat;

use canvas_compat::{MAX_CANVAS_BACKING_AREA, backing_size, wheel_delta_css_pixels};

#[test]
fn hidpi_uses_native_backing_resolution_when_safe() {
    let size = backing_size(800.0, 600.0, 2.0);
    assert_eq!((size.width, size.height), (1600, 1200));
    assert!((size.scale_x - 2.0).abs() < 1e-9);
    assert!((size.scale_y - 2.0).abs() < 1e-9);
}

#[test]
fn large_hidpi_canvas_is_clamped_without_changing_logical_space() {
    let size = backing_size(3840.0, 2160.0, 2.0);
    assert_eq!(size.width, 4096);
    assert!(size.height <= 4096);
    assert!((size.width as u64) * (size.height as u64) <= MAX_CANVAS_BACKING_AREA as u64);
    assert!(size.scale_x > 1.0 && size.scale_x < 2.0);
}

#[test]
fn oversized_css_viewport_can_downsample_to_stay_compatible() {
    let size = backing_size(10_000.0, 1_000.0, 2.0);
    assert_eq!(size.width, 4096);
    assert!(size.scale_x < 1.0);
    assert!(size.height >= 1);
}

#[test]
fn invalid_or_extreme_dpr_is_sanitized() {
    assert_eq!(backing_size(100.0, 50.0, f64::NAN).width, 100);
    assert_eq!(backing_size(100.0, 50.0, 99.0).width, 400);
}

#[test]
fn backing_area_never_exceeds_conservative_limit() {
    for (width, height, dpr) in [
        (3000.0, 3000.0, 2.0),
        (7680.0, 4320.0, 2.0),
        (1920.0, 1080.0, 4.0),
        (10_000.0, 10_000.0, 1.0),
    ] {
        let size = backing_size(width, height, dpr);
        assert!((size.width as u64) * (size.height as u64) <= MAX_CANVAS_BACKING_AREA as u64);
    }
}

#[test]
fn wheel_modes_are_normalized_to_css_pixels() {
    assert_eq!(wheel_delta_css_pixels(12.5, 0, 24.0, 800.0), 12.5);
    assert_eq!(wheel_delta_css_pixels(3.0, 1, 24.0, 800.0), 72.0);
    assert_eq!(wheel_delta_css_pixels(-1.0, 2, 24.0, 800.0), -800.0);
}

#[test]
fn malformed_wheel_delta_is_ignored() {
    assert_eq!(wheel_delta_css_pixels(f64::NAN, 0, 24.0, 800.0), 0.0);
    assert_eq!(wheel_delta_css_pixels(f64::INFINITY, 1, 24.0, 800.0), 0.0);
}
