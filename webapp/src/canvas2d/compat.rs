// Pure Canvas2D backing-store policy. Kept free of web_sys so it can be
// exercised by native tests as well as the wasm renderer.

pub const MAX_CANVAS_BACKING_DIMENSION: f64 = 4096.0;
pub const MAX_CANVAS_BACKING_AREA: f64 = 16_777_216.0;
const MAX_DEVICE_PIXEL_RATIO: f64 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasBackingSize {
    pub width: u32,
    pub height: u32,
    pub scale_x: f64,
    pub scale_y: f64,
}

pub fn backing_size(
    logical_width: f64,
    logical_height: f64,
    requested_dpr: f64,
) -> CanvasBackingSize {
    let logical_width = logical_width.max(1.0);
    let logical_height = logical_height.max(1.0);
    let requested_dpr = if requested_dpr.is_finite() && requested_dpr > 0.0 {
        requested_dpr.min(MAX_DEVICE_PIXEL_RATIO)
    } else {
        1.0
    };

    let dimension_scale = (MAX_CANVAS_BACKING_DIMENSION / logical_width)
        .min(MAX_CANVAS_BACKING_DIMENSION / logical_height);
    let area_scale = (MAX_CANVAS_BACKING_AREA / (logical_width * logical_height)).sqrt();
    let scale = requested_dpr
        .min(dimension_scale)
        .min(area_scale)
        .max(f64::MIN_POSITIVE);
    let width = (logical_width * scale)
        .floor()
        .clamp(1.0, MAX_CANVAS_BACKING_DIMENSION) as u32;
    let height = (logical_height * scale)
        .floor()
        .clamp(1.0, MAX_CANVAS_BACKING_DIMENSION) as u32;

    CanvasBackingSize {
        width,
        height,
        // Derive the transform from the integer backing size so the logical
        // viewport maps exactly onto the full raster after rounding.
        scale_x: width as f64 / logical_width,
        scale_y: height as f64 / logical_height,
    }
}

/// Convert WheelEvent deltaY to CSS pixels. Firefox and some input devices
/// report line- or page-based deltas instead of pixel deltas.
pub fn wheel_delta_css_pixels(
    delta_y: f64,
    delta_mode: u32,
    line_height: f64,
    viewport_height: f64,
) -> f64 {
    if !delta_y.is_finite() {
        return 0.0;
    }
    match delta_mode {
        1 => delta_y * line_height.max(1.0),
        2 => delta_y * viewport_height.max(1.0),
        _ => delta_y,
    }
}
