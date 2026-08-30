use std::{cell::RefCell, collections::HashMap, rc::Rc};

use ratex_layout::{layout, to_display_list, LayoutOptions};
use ratex_parser::parse;
use ratex_render::{render_to_rgba_premultiplied, PremultipliedRgbaImage, RenderOptions};
use ratex_types::{color::Color, math_style::MathStyle};

const INLINE_FONT_SIZE: f32 = 16.0;
const DISPLAY_FONT_SIZE: f32 = 16.0;
const PADDING: f32 = 2.0;
const SUPERSAMPLE: u32 = 2;
const MAX_CACHE_ENTRIES: usize = 256;

pub struct MathImage {
    pub width: u32,
    pub height: u32,
    /// Baseline measured from the top edge of the rasterized RaTeX image.
    pub baseline: u32,
    /// Retain full RGBA pixels only in tests. Runtime renderers consume `runs`,
    /// so cached images avoid keeping a second, much larger representation.
    #[cfg(test)]
    pub pixels: Vec<u8>,
    /// Horizontal runs of identical RGBA pixels, prepared once at rasterize time.
    pub runs: Vec<MathRun>,
}

#[derive(Clone, Copy)]
pub struct MathRun {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub rgba: [u8; 4],
}

impl MathRun {
    /// Pack the already-quantized run color exactly as the GPU instance format
    /// expects. Only alpha depends on scene opacity, so avoid converting RGB
    /// bytes to floats and immediately packing them back every frame.
    #[inline]
    pub fn packed_color(&self, opacity: f32) -> u32 {
        if opacity == 1.0 {
            return u32::from_le_bytes(self.rgba);
        }
        let alpha = ((self.rgba[3] as f32 / 255.0 * opacity)
            .clamp(0.0, 1.0)
            * 255.0)
            .round() as u8;
        u32::from_le_bytes([self.rgba[0], self.rgba[1], self.rgba[2], alpha])
    }
}

thread_local! {
    static CACHE: RefCell<HashMap<(String, bool, u16), Rc<MathImage>>> = RefCell::new(HashMap::new());
}

/// Typesets LaTeX with RaTeX and keeps the raster in memory as RGBA. The
/// renderer exposes tiny-skia's premultiplied pixels directly, avoiding the old
/// PNG encode/decode round-trip before supersample reduction.
pub fn rasterize(source: &str, display: bool, text_scale: f32) -> Result<Rc<MathImage>, String> {
    let scale_key = (text_scale.clamp(0.5, 4.0) * 64.0).round() as u16;
    let key = (source.to_owned(), display, scale_key);
    if let Some(image) = CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return Ok(image);
    }

    let image = Rc::new(rasterize_uncached(
        source,
        display,
        scale_key as f32 / 64.0,
    )?);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= MAX_CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(key, image.clone());
    });
    Ok(image)
}

fn rasterize_uncached(source: &str, display: bool, text_scale: f32) -> Result<MathImage, String> {
    let nodes = parse(source).map_err(|error| format!("RaTeX parse error: {error}"))?;
    let foreground = Color::new(0.92, 0.96, 1.0, 1.0);
    let mut layout_options = LayoutOptions::default().with_color(foreground);
    if !display {
        layout_options = layout_options.with_style(MathStyle::Text);
    }
    let layout_box = layout(&nodes, &layout_options);
    let display_list = to_display_list(&layout_box);
    let font_size = if display {
        DISPLAY_FONT_SIZE
    } else {
        INLINE_FONT_SIZE
    } * text_scale;
    let baseline = (display_list.height as f32 * font_size + PADDING)
        .round()
        .max(0.0) as u32;
    let high_resolution = render_to_rgba_premultiplied(
        &display_list,
        &RenderOptions {
            font_size,
            padding: PADDING,
            background_color: Color::new(0.0, 0.0, 0.0, 0.0),
            font_dir: String::new(),
            device_pixel_ratio: SUPERSAMPLE as f32,
        },
    )
    .map_err(|error| format!("RaTeX render error: {error}"))?;
    Ok(downsample_premultiplied_rgba(
        high_resolution,
        SUPERSAMPLE,
        baseline,
    ))
}

/// Box-filter tiny-skia's native premultiplied RGBA directly. The old path
/// demultiplied every high-resolution pixel for PNG, compressed it, decoded it,
/// then premultiplied it again here. Summing premultiplied bytes preserves the
/// same coverage model while requiring only one final demultiply per output pixel.
fn downsample_premultiplied_rgba(
    source: PremultipliedRgbaImage,
    factor: u32,
    baseline: u32,
) -> MathImage {
    if factor == 2 {
        return downsample_premultiplied_rgba_2x(source, baseline);
    }
    downsample_premultiplied_rgba_generic(source, factor, baseline)
}

fn downsample_premultiplied_rgba_generic(
    source: PremultipliedRgbaImage,
    factor: u32,
    baseline: u32,
) -> MathImage {
    let width = source.width.div_ceil(factor);
    let height = source.height.div_ceil(factor);
    let mut pixels = vec![0; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let mut alpha_sum = 0u32;
            let mut premultiplied = [0u32; 3];
            let mut samples = 0u32;
            for sy in 0..factor {
                for sx in 0..factor {
                    let source_x = x * factor + sx;
                    let source_y = y * factor + sy;
                    if source_x >= source.width || source_y >= source.height {
                        continue;
                    }
                    let index = ((source_y * source.width + source_x) * 4) as usize;
                    alpha_sum += source.pixels[index + 3] as u32;
                    for (channel, sum) in premultiplied.iter_mut().enumerate() {
                        *sum += source.pixels[index + channel] as u32;
                    }
                    samples += 1;
                }
            }

            let target = ((y * width + x) * 4) as usize;
            if alpha_sum != 0 {
                for (channel, sum) in premultiplied.iter().enumerate() {
                    pixels[target + channel] =
                        (((*sum * 255) + alpha_sum / 2) / alpha_sum).min(255) as u8;
                }
            }
            pixels[target + 3] = ((alpha_sum + samples / 2) / samples.max(1)).min(255) as u8;
        }
    }
    let runs = build_runs(width, height, &pixels);
    MathImage {
        width,
        height,
        baseline: baseline.min(height),
        #[cfg(test)]
        pixels,
        runs,
    }
}

#[inline]
fn store_downsampled_pixel(
    pixels: &mut [u8],
    target: usize,
    premultiplied: [u32; 3],
    alpha_sum: u32,
    samples: u32,
) {
    if alpha_sum != 0 {
        pixels[target] = (((premultiplied[0] * 255) + alpha_sum / 2) / alpha_sum).min(255) as u8;
        pixels[target + 1] =
            (((premultiplied[1] * 255) + alpha_sum / 2) / alpha_sum).min(255) as u8;
        pixels[target + 2] =
            (((premultiplied[2] * 255) + alpha_sum / 2) / alpha_sum).min(255) as u8;
    }
    pixels[target + 3] = ((alpha_sum + samples / 2) / samples).min(255) as u8;
}

/// Fast path for the fixed 2× supersampling used by the web renderer. Interior
/// pixels always consume a complete 2×2 source block, so avoid the generic
/// nested sample loops and four bounds checks per output pixel.
fn downsample_premultiplied_rgba_2x(source: PremultipliedRgbaImage, baseline: u32) -> MathImage {
    let width = source.width.div_ceil(2);
    let height = source.height.div_ceil(2);
    let mut pixels = vec![0; (width * height * 4) as usize];
    let source_stride = source.width as usize * 4;
    let full_width = source.width / 2;
    let full_height = source.height / 2;

    for y in 0..full_height {
        let row0 = y as usize * 2 * source_stride;
        let row1 = row0 + source_stride;
        let target_row = y as usize * width as usize * 4;
        for x in 0..full_width {
            let i0 = row0 + x as usize * 8;
            let i1 = i0 + 4;
            let i2 = row1 + x as usize * 8;
            let i3 = i2 + 4;
            let alpha_sum = source.pixels[i0 + 3] as u32
                + source.pixels[i1 + 3] as u32
                + source.pixels[i2 + 3] as u32
                + source.pixels[i3 + 3] as u32;
            let sums = [
                source.pixels[i0] as u32
                    + source.pixels[i1] as u32
                    + source.pixels[i2] as u32
                    + source.pixels[i3] as u32,
                source.pixels[i0 + 1] as u32
                    + source.pixels[i1 + 1] as u32
                    + source.pixels[i2 + 1] as u32
                    + source.pixels[i3 + 1] as u32,
                source.pixels[i0 + 2] as u32
                    + source.pixels[i1 + 2] as u32
                    + source.pixels[i2 + 2] as u32
                    + source.pixels[i3 + 2] as u32,
            ];
            store_downsampled_pixel(&mut pixels, target_row + x as usize * 4, sums, alpha_sum, 4);
        }

        if source.width & 1 != 0 {
            let i0 = row0 + (source.width as usize - 1) * 4;
            let i1 = row1 + (source.width as usize - 1) * 4;
            let alpha_sum = source.pixels[i0 + 3] as u32 + source.pixels[i1 + 3] as u32;
            let sums = [
                source.pixels[i0] as u32 + source.pixels[i1] as u32,
                source.pixels[i0 + 1] as u32 + source.pixels[i1 + 1] as u32,
                source.pixels[i0 + 2] as u32 + source.pixels[i1 + 2] as u32,
            ];
            store_downsampled_pixel(
                &mut pixels,
                target_row + full_width as usize * 4,
                sums,
                alpha_sum,
                2,
            );
        }
    }

    if source.height & 1 != 0 {
        let row = (source.height as usize - 1) * source_stride;
        let target_row = full_height as usize * width as usize * 4;
        for x in 0..full_width {
            let i0 = row + x as usize * 8;
            let i1 = i0 + 4;
            let alpha_sum = source.pixels[i0 + 3] as u32 + source.pixels[i1 + 3] as u32;
            let sums = [
                source.pixels[i0] as u32 + source.pixels[i1] as u32,
                source.pixels[i0 + 1] as u32 + source.pixels[i1 + 1] as u32,
                source.pixels[i0 + 2] as u32 + source.pixels[i1 + 2] as u32,
            ];
            store_downsampled_pixel(&mut pixels, target_row + x as usize * 4, sums, alpha_sum, 2);
        }
        if source.width & 1 != 0 {
            let i = row + (source.width as usize - 1) * 4;
            store_downsampled_pixel(
                &mut pixels,
                target_row + full_width as usize * 4,
                [
                    source.pixels[i] as u32,
                    source.pixels[i + 1] as u32,
                    source.pixels[i + 2] as u32,
                ],
                source.pixels[i + 3] as u32,
                1,
            );
        }
    }

    let runs = build_runs(width, height, &pixels);
    MathImage {
        width,
        height,
        baseline: baseline.min(height),
        #[cfg(test)]
        pixels,
        runs,
    }
}

#[inline]
fn quantized_rgba(pixel: &[u8]) -> u32 {
    let alpha = (((pixel[3] as u16 + 8) / 16) * 16).min(255) as u8;
    u32::from_le_bytes([pixel[0], pixel[1], pixel[2], alpha])
}

fn build_runs(width: u32, height: u32, pixels: &[u8]) -> Vec<MathRun> {
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let row_bytes = width as usize * 4;
    // Typical supersampled math produces roughly one run per 6 output pixels.
    // Reserve enough for common formulas while capping pathological over-allocation.
    let estimate = (width as usize)
        .saturating_mul(height as usize)
        .div_ceil(6)
        .min(16_384);
    let mut runs = Vec::with_capacity(estimate);

    for (y, row) in pixels
        .chunks_exact(row_bytes)
        .take(height as usize)
        .enumerate()
    {
        let mut x = 0usize;
        while x < width as usize {
            let rgba = quantized_rgba(&row[x * 4..x * 4 + 4]);
            if rgba >> 24 == 0 {
                x += 1;
                continue;
            }

            let start = x;
            x += 1;
            while x < width as usize && quantized_rgba(&row[x * 4..x * 4 + 4]) == rgba {
                x += 1;
            }
            runs.push(MathRun {
                x: start as u32,
                y: y as u32,
                width: (x - start) as u32,
                rgba: rgba.to_le_bytes(),
            });
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic(width: u32, height: u32) -> PremultipliedRgbaImage {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let alpha = ((x * 37 + y * 61 + 17) % 256) as u8;
                let straight = [
                    ((x * 29 + y * 11 + 31) % 256) as u8,
                    ((x * 7 + y * 43 + 79) % 256) as u8,
                    ((x * 53 + y * 5 + 127) % 256) as u8,
                ];
                pixels.extend(
                    straight.map(|channel| ((channel as u16 * alpha as u16 + 127) / 255) as u8),
                );
                pixels.push(alpha);
            }
        }
        PremultipliedRgbaImage {
            width,
            height,
            pixels,
        }
    }

    #[test]
    fn packed_run_color_matches_float_pack_semantics() {
        fn reference(rgba: [u8; 4], opacity: f32) -> u32 {
            let color = [
                rgba[0] as f32 / 255.0,
                rgba[1] as f32 / 255.0,
                rgba[2] as f32 / 255.0,
                rgba[3] as f32 / 255.0 * opacity,
            ];
            color
                .into_iter()
                .enumerate()
                .fold(0, |packed, (shift, channel)| {
                    packed
                        | ((channel.clamp(0.0, 1.0) * 255.0).round() as u32)
                            << (shift * 8)
                })
        }

        for byte in 0..=255u8 {
            let run = MathRun {
                x: 0,
                y: 0,
                width: 1,
                rgba: [byte, 255 - byte, byte / 2, byte],
            };
            for step in 0..=100 {
                let opacity = step as f32 / 100.0;
                assert_eq!(run.packed_color(opacity), reference(run.rgba, opacity));
            }
        }
    }

    #[test]
    fn build_runs_quantizes_alpha_and_coalesces_equal_pixels() {
        let pixels = [
            9, 9, 9, 0,
            9, 9, 9, 7,
            1, 2, 3, 8,
            1, 2, 3, 15,
            1, 2, 3, 24,
        ];
        let runs = build_runs(5, 1, &pixels);
        assert_eq!(runs.len(), 2);
        assert_eq!((runs[0].x, runs[0].y, runs[0].width, runs[0].rgba), (2, 0, 2, [1, 2, 3, 16]));
        assert_eq!((runs[1].x, runs[1].y, runs[1].width, runs[1].rgba), (4, 0, 1, [1, 2, 3, 32]));
        assert!(build_runs(0, 1, &[]).is_empty());
    }

    #[test]
    fn two_x_downsample_matches_generic_for_odd_and_even_edges() {
        for (width, height) in [(1, 1), (2, 2), (3, 2), (2, 3), (3, 3), (8, 7), (9, 10)] {
            let fast = downsample_premultiplied_rgba_2x(synthetic(width, height), 3);
            let generic = downsample_premultiplied_rgba_generic(synthetic(width, height), 2, 3);
            assert_eq!(
                (fast.width, fast.height, fast.baseline),
                (generic.width, generic.height, generic.baseline)
            );
            assert_eq!(fast.pixels, generic.pixels, "{width}x{height}");
            assert_eq!(
                fast.runs.len(),
                generic.runs.len(),
                "{width}x{height} run count"
            );
            for (a, b) in fast.runs.iter().zip(&generic.runs) {
                assert_eq!((a.x, a.y, a.width, a.rgba), (b.x, b.y, b.width, b.rgba));
            }
        }
    }
}
