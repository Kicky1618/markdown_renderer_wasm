use std::{cell::RefCell, collections::HashMap, io::Cursor, rc::Rc};

use ratex_layout::{LayoutOptions, layout, to_display_list};
use ratex_parser::parse;
use ratex_render::{RenderOptions, render_to_png};
use ratex_types::{color::Color, math_style::MathStyle};

const INLINE_FONT_SIZE: f32 = 16.0;
const DISPLAY_FONT_SIZE: f32 = 16.0;
const PADDING: f32 = 2.0;
const SUPERSAMPLE: u32 = 2;
const MAX_CACHE_ENTRIES: usize = 256;

pub struct MathImage {
    pub width: u32,
    pub height: u32,
    /// Baseline measured from the top edge of the decoded RaTeX image.
    pub baseline: u32,
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

thread_local! {
    static CACHE: RefCell<HashMap<(String, bool, u16), Rc<MathImage>>> = RefCell::new(HashMap::new());
}

/// Typesets LaTeX with RaTeX, rasterizes its display list into a transparent
/// PNG with the bundled KaTeX fonts, then decodes it to RGBA for wgpu.
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
    let png = render_to_png(
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
    let high_resolution = decode_png(&png, baseline * SUPERSAMPLE)?;
    Ok(downsample_rgba(high_resolution, SUPERSAMPLE, baseline))
}

/// Box-filter a supersampled RaTeX image in premultiplied-alpha space. Averaging
/// straight RGB would create pale fringes around thin mathematical strokes.
fn downsample_rgba(source: MathImage, factor: u32, baseline: u32) -> MathImage {
    let width = source.width.div_ceil(factor);
    let height = source.height.div_ceil(factor);
    let mut pixels = vec![0; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let mut alpha_sum = 0.0f32;
            let mut premultiplied = [0.0f32; 3];
            let mut samples = 0.0f32;
            for sy in 0..factor {
                for sx in 0..factor {
                    let source_x = x * factor + sx;
                    let source_y = y * factor + sy;
                    if source_x >= source.width || source_y >= source.height {
                        continue;
                    }
                    let index = ((source_y * source.width + source_x) * 4) as usize;
                    let alpha = source.pixels[index + 3] as f32 / 255.0;
                    alpha_sum += alpha;
                    for (channel, sum) in premultiplied.iter_mut().enumerate() {
                        *sum += source.pixels[index + channel] as f32 * alpha;
                    }
                    samples += 1.0;
                }
            }
            let alpha = alpha_sum / samples.max(1.0);
            let target = ((y * width + x) * 4) as usize;
            if alpha_sum > 0.0 {
                for (channel, sum) in premultiplied.iter().enumerate() {
                    pixels[target + channel] = (*sum / alpha_sum).round().clamp(0.0, 255.0) as u8;
                }
            }
            pixels[target + 3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    let runs = build_runs(width, height, &pixels);
    MathImage {
        width,
        height,
        baseline: baseline.min(height),
        pixels,
        runs,
    }
}

fn build_runs(width: u32, height: u32, pixels: &[u8]) -> Vec<MathRun> {
    let mut runs = Vec::new();
    for y in 0..height {
        let mut x = 0;
        while x < width {
            let index = ((y * width + x) * 4) as usize;
            let mut rgba = [
                pixels[index],
                pixels[index + 1],
                pixels[index + 2],
                pixels[index + 3],
            ];
            // A small coverage palette dramatically reduces rectangle count on
            // WebGL while retaining the supersampled edge shape.
            rgba[3] = (((rgba[3] as u16 + 8) / 16) * 16).min(255) as u8;
            if rgba[3] == 0 {
                x += 1;
                continue;
            }
            let start = x;
            x += 1;
            while x < width {
                let next = ((y * width + x) * 4) as usize;
                let mut next_rgba = [
                    pixels[next],
                    pixels[next + 1],
                    pixels[next + 2],
                    pixels[next + 3],
                ];
                next_rgba[3] = (((next_rgba[3] as u16 + 8) / 16) * 16).min(255) as u8;
                if next_rgba != rgba {
                    break;
                }
                x += 1;
            }
            runs.push(MathRun {
                x: start,
                y,
                width: x - start,
                rgba,
            });
        }
    }
    runs
}

fn decode_png(bytes: &[u8], baseline: u32) -> Result<MathImage, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|error| format!("RaTeX PNG header error: {error}"))?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|error| format!("RaTeX PNG decode error: {error}"))?;
    let source = &buffer[..info.buffer_size()];
    let pixels = match info.color_type {
        png::ColorType::Rgba => source.to_vec(),
        png::ColorType::Rgb => source
            .chunks_exact(3)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect(),
        other => return Err(format!("unsupported RaTeX PNG color type: {other:?}")),
    };
    Ok(MathImage {
        width: info.width,
        height: info.height,
        baseline: baseline.min(info.height),
        pixels,
        runs: Vec::new(),
    })
}
