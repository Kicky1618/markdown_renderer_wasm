use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, RwLock},
};

use ab_glyph::{Font, FontRef};
use ratex_font::FontId;
use ratex_font_loader::FontSet;
use ratex_types::color::Color;
use ratex_types::display_item::{DisplayItem, DisplayList};
use tiny_skia::{
    FillRule, FilterQuality, IntSize, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform,
};

const INCREMENTAL_GUARD_PIXELS: u32 = 64;
const TINY_SKIA_MAX_UNTILED_DIMENSION: u32 = 8191;

/// Options controlling PNG output.
pub struct RenderOptions {
    pub font_size: f32,
    pub padding: f32,
    /// Background fill color for the output PNG. Set alpha to 0.0 for transparency.
    pub background_color: Color,
    /// Directory containing KaTeX `.ttf` files. Used only when `embed-fonts` is disabled.
    pub font_dir: String,
    /// Multiplies pixels-per-em (and padding) so the same layout renders at higher resolution
    /// (e.g. 2.0 to align RaTeX PNG pixel density with Puppeteer `deviceScaleFactor: 2` refs).
    pub device_pixel_ratio: f32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            font_size: 40.0,
            padding: 10.0,
            background_color: Color::WHITE,
            font_dir: String::new(),
            device_pixel_ratio: 1.0,
        }
    }
}

/// Premultiplied RGBA raster output. `pixels` uses byte-order RGBA and each
/// RGB channel is already multiplied by alpha, matching tiny-skia's native
/// representation. This avoids PNG encode/decode round-trips for in-memory
/// consumers such as the Streamdown Web renderer.
pub struct PremultipliedRgbaImage {
    /// Active image width. Pixels to the right of this are capacity only.
    pub width: u32,
    pub height: u32,
    /// Pixel stride of each row. `stride >= width`; normal full renders use equality.
    pub stride: u32,
    pub pixels: Vec<u8>,
}

pub fn render_to_rgba_premultiplied(
    display_list: &DisplayList,
    options: &RenderOptions,
) -> Result<PremultipliedRgbaImage, String> {
    let pixmap = render_to_pixmap(display_list, options)?;
    let width = pixmap.width();
    let height = pixmap.height();
    Ok(PremultipliedRgbaImage {
        width,
        height,
        stride: width,
        pixels: pixmap.take(),
    })
}

fn tiny_skia_tiled(width: u32, height: u32) -> bool {
    width > TINY_SKIA_MAX_UNTILED_DIMENSION || height > TINY_SKIA_MAX_UNTILED_DIMENSION
}

fn incremental_capacity(width: u32, height: u32) -> u32 {
    let guarded = width.saturating_add(INCREMENTAL_GUARD_PIXELS);
    let desired = guarded.checked_next_power_of_two().unwrap_or(guarded);
    if !tiny_skia_tiled(width, height) {
        desired.min(TINY_SKIA_MAX_UNTILED_DIMENSION)
    } else {
        desired.max(width)
    }
}

/// Reuse an earlier transparent raster when `display_list` only appends drawing
/// items. Returns `Ok(None)` whenever exact incremental reuse is not safe.
///
/// With unchanged capacity, only appended DisplayItems are rendered in-place.
/// If the backing surface must grow, the previous prefix is first re-rendered
/// into the larger surface so clipped edge coverage cannot leak across updates.
pub fn render_to_rgba_premultiplied_append(
    previous_display_list: &DisplayList,
    mut previous_image: PremultipliedRgbaImage,
    display_list: &DisplayList,
    options: &RenderOptions,
) -> Result<Option<PremultipliedRgbaImage>, String> {
    if options.background_color.a != 0.0
        || display_list.items.len() < previous_display_list.items.len()
        || display_list.items[..previous_display_list.items.len()]
            != previous_display_list.items[..]
    {
        return Ok(None);
    }

    let (em_px, pad_px, dpr, previous_width, previous_height) =
        raster_metrics(previous_display_list, options);
    let (_, _, _, width, height) = raster_metrics(display_list, options);
    if previous_image.width != previous_width
        || previous_image.height != previous_height
        || previous_image.stride < previous_image.width
        || previous_image.pixels.len()
            != previous_image.stride as usize * previous_image.height as usize * 4
        || height != previous_height
        || width < previous_width
    {
        return Ok(None);
    }

    // tiny-skia changes fill_rect from its scan converter to tiled fill_path
    // above 8191px. Reusing pixels across that boundary would preserve the old
    // algorithm's coverage, so force a full reraster exactly at the transition.
    if tiny_skia_tiled(previous_width, previous_height) != tiny_skia_tiled(width, height) {
        return Ok(None);
    }

    let desired_stride = incremental_capacity(width, height);
    let stride = if width <= previous_image.stride && desired_stride <= previous_image.stride {
        previous_image.stride
    } else {
        desired_stride
    };
    let mut pixmap = if stride == previous_image.stride {
        let size = IntSize::from_wh(stride, height)
            .ok_or_else(|| format!("Invalid pixmap size {}x{}", stride, height))?;
        Pixmap::from_vec(std::mem::take(&mut previous_image.pixels), size)
            .ok_or_else(|| format!("Failed to reuse pixmap {}x{}", stride, height))?
    } else {
        // Re-render the previous prefix when capacity grows instead of copying the
        // old rows. The exact-width/capacity edge may have clipped an existing
        // glyph or rule just beyond the old stride; rebuilding restores those
        // hidden pixels before the appended items are composited.
        let mut grown = Pixmap::new(stride, height)
            .ok_or_else(|| format!("Failed to create pixmap {}x{}", stride, height))?;
        render_with_fonts(
            &mut grown,
            previous_display_list,
            options,
            em_px,
            pad_px,
            dpr,
        )?;
        grown
    };

    let suffix = &display_list.items[previous_display_list.items.len()..];
    if !suffix.is_empty() {
        render_items_with_fonts(&mut pixmap, suffix, options, em_px, pad_px, dpr)?;
    }
    Ok(Some(PremultipliedRgbaImage {
        width,
        height,
        stride,
        pixels: pixmap.take(),
    }))
}

pub fn render_to_png(
    display_list: &DisplayList,
    options: &RenderOptions,
) -> Result<Vec<u8>, String> {
    let pixmap = render_to_pixmap(display_list, options)?;
    encode_png(&pixmap)
}

fn raster_metrics(
    display_list: &DisplayList,
    options: &RenderOptions,
) -> (f32, f32, f32, u32, u32) {
    let dpr = options.device_pixel_ratio.clamp(0.01, 16.0);
    let em_px = options.font_size * dpr;
    let pad_px = options.padding * dpr;
    let total_h = display_list.height + display_list.depth;
    let width = (display_list.width as f32 * em_px + 2.0 * pad_px)
        .ceil()
        .max(1.0) as u32;
    let height = (total_h as f32 * em_px + 2.0 * pad_px).ceil().max(1.0) as u32;
    (em_px, pad_px, dpr, width, height)
}

fn render_to_pixmap(display_list: &DisplayList, options: &RenderOptions) -> Result<Pixmap, String> {
    let (em_px, pad_px, dpr, img_w, img_h) = raster_metrics(display_list, options);

    let mut pixmap = Pixmap::new(img_w, img_h)
        .ok_or_else(|| format!("Failed to create pixmap {}x{}", img_w, img_h))?;

    // Pixmap::new() is already zero-filled. Transparent math is the hot path in
    // Streamdown, so avoid writing the entire supersampled surface a second time.
    if options.background_color.a > 0.0 {
        pixmap.fill(to_tiny_skia_color(options.background_color));
    }

    // Lazy font loading is shared across renderers and source-aware by font_dir.
    render_with_fonts(&mut pixmap, display_list, options, em_px, pad_px, dpr)?;
    Ok(pixmap)
}

/// Load fonts lazily and render the DisplayList.
fn render_with_fonts(
    pixmap: &mut Pixmap,
    display_list: &DisplayList,
    options: &RenderOptions,
    em_px: f32,
    pad_px: f32,
    dpr: f32,
) -> Result<(), String> {
    render_items_with_fonts(pixmap, &display_list.items, options, em_px, pad_px, dpr)
}

fn render_items_with_fonts(
    pixmap: &mut Pixmap,
    items: &[DisplayItem],
    options: &RenderOptions,
    em_px: f32,
    pad_px: f32,
    dpr: f32,
) -> Result<(), String> {
    let fonts = ratex_font_loader::load_fonts_for_items(&options.font_dir, items)?;
    let font_refs = build_font_refs(&fonts)?;
    render_items(pixmap, items, &font_refs, em_px, pad_px, dpr);
    Ok(())
}

fn to_tiny_skia_color(color: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(
        color.r.clamp(0.0, 1.0),
        color.g.clamp(0.0, 1.0),
        color.b.clamp(0.0, 1.0),
        color.a.clamp(0.0, 1.0),
    )
    .unwrap_or(tiny_skia::Color::TRANSPARENT)
}

fn rgba8_for_color(color: &Color) -> [u8; 4] {
    [
        (color.r * 255.0) as u8,
        (color.g * 255.0) as u8,
        (color.b * 255.0) as u8,
        (color.a.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

fn paint_for_color(color: &Color) -> Paint<'static> {
    let [r, g, b, a] = rgba8_for_color(color);
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, a);
    paint
}

fn normalized_alpha(alpha: f32) -> f32 {
    if alpha.is_finite() {
        alpha.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// Build a `FontId → FontRef` map from the raw font data (borrowed from the cache lock).
fn build_font_refs(data: &FontSet) -> Result<HashMap<FontId, FontRef<'_>>, String> {
    let mut font_refs = HashMap::new();
    for (id, bytes) in data.iter() {
        let font = FontRef::try_from_slice_and_index(bytes, sfnt_collection_index(*id))
            .map_err(|e| format!("Failed to parse font {:?}: {}", id, e))?;
        font_refs.insert(*id, font);
    }

    if !font_refs.contains_key(&FontId::MainRegular) {
        return Err("Main-Regular font not found".to_string());
    }

    Ok(font_refs)
}

/// Render all items in the DisplayList using the given font cache.
fn render_items(
    pixmap: &mut Pixmap,
    items: &[DisplayItem],
    font_cache: &HashMap<FontId, FontRef<'_>>,
    em_px: f32,
    pad_px: f32,
    dpr: f32,
) {
    let mut font_id_cache: HashMap<&str, FontId> = HashMap::new();
    for item in items {
        match item {
            DisplayItem::GlyphPath {
                x,
                y,
                scale,
                font,
                char_code,
                color,
            } => {
                let glyph_em = em_px * *scale as f32;
                let font_id = *font_id_cache
                    .entry(font.as_str())
                    .or_insert_with(|| FontId::parse(font).unwrap_or(FontId::MainRegular));
                render_glyph(
                    pixmap,
                    *x as f32 * em_px + pad_px,
                    *y as f32 * em_px + pad_px,
                    font_id,
                    *char_code,
                    color,
                    font_cache,
                    glyph_em,
                );
            }
            DisplayItem::Line {
                x,
                y,
                width,
                thickness,
                color,
                dashed,
            } => {
                render_line(
                    pixmap,
                    *x as f32 * em_px + pad_px,
                    *y as f32 * em_px + pad_px,
                    *width as f32 * em_px,
                    *thickness as f32 * em_px,
                    color,
                    *dashed,
                );
            }
            DisplayItem::Rect {
                x,
                y,
                width,
                height,
                color,
            } => {
                render_rect(
                    pixmap,
                    *x as f32 * em_px + pad_px,
                    *y as f32 * em_px + pad_px,
                    *width as f32 * em_px,
                    *height as f32 * em_px,
                    color,
                );
            }
            DisplayItem::Path {
                x,
                y,
                commands,
                fill,
                color,
            } => {
                render_path(
                    pixmap,
                    *x as f32 * em_px + pad_px,
                    *y as f32 * em_px + pad_px,
                    commands,
                    *fill,
                    color,
                    em_px,
                    1.5 * dpr,
                );
            }
        }
    }
}

fn sfnt_collection_index(id: FontId) -> u32 {
    match id {
        FontId::EmojiFallback => ratex_unicode_font::emoji_font_face_index().unwrap_or(0),
        FontId::CjkRegular => ratex_unicode_font::unicode_font_face_index().unwrap_or(0),
        FontId::CjkFallback => ratex_unicode_font::fallback_font_face_index().unwrap_or(0),
        _ => 0,
    }
}

/// After `.notdef` or a cmap slot with **no drawable outline** (common for emoji in text fonts),
/// try KaTeX Main → `CjkRegular` → **Emoji** (color font, vector + sbix bitmap) → `CjkFallback`.
///
/// Emoji is tried **before** the broad text fallback so supplementary-plane / color glyphs are not
/// stuck behind Arial-style faces that often lack drawable outlines for emoji.
///
/// When `skip_main_regular` is `true`, skips `Main-Regular` (caller already tried that face).
#[allow(clippy::too_many_arguments)]
fn try_system_unicode_fallback(
    pixmap: &mut Pixmap,
    px: f32,
    py: f32,
    ch: char,
    color: &Color,
    em: f32,
    font_cache: &HashMap<FontId, FontRef<'_>>,
    skip_main_regular: bool,
) -> bool {
    if !skip_main_regular {
        if let Some(fallback) = font_cache.get(&FontId::MainRegular) {
            let fid = fallback.glyph_id(ch);
            if fid.0 != 0
                && render_glyph_with_font(
                    pixmap,
                    px,
                    py,
                    FontGlyph {
                        font_id: FontId::MainRegular,
                        font: fallback,
                        glyph_id: fid,
                    },
                    color,
                    em,
                )
            {
                return true;
            }
        }
    }
    if let Some(cjk_font) = font_cache.get(&FontId::CjkRegular) {
        let fid = cjk_font.glyph_id(ch);
        if fid.0 != 0
            && render_glyph_with_font(
                pixmap,
                px,
                py,
                FontGlyph {
                    font_id: FontId::CjkRegular,
                    font: cjk_font,
                    glyph_id: fid,
                },
                color,
                em,
            )
        {
            return true;
        }
    }
    if try_emoji_vector_then_bitmap(pixmap, px, py, ch, color, em, font_cache) {
        return true;
    }
    if let Some(fb_font) = font_cache.get(&FontId::CjkFallback) {
        let fid = fb_font.glyph_id(ch);
        if fid.0 != 0
            && render_glyph_with_font(
                pixmap,
                px,
                py,
                FontGlyph {
                    font_id: FontId::CjkFallback,
                    font: fb_font,
                    glyph_id: fid,
                },
                color,
                em,
            )
        {
            return true;
        }
    }
    false
}

/// Color fonts (e.g. Apple Color Emoji) often expose a minimal `glyf` outline for COLR masking
/// while the visible glyph lives in `sbix` / `CBDT`. `ab_glyph` then "succeeds" with an
/// effectively invisible path — so **raster strike first**, then outline.
#[allow(clippy::too_many_arguments)]
fn try_emoji_vector_then_bitmap(
    pixmap: &mut Pixmap,
    px: f32,
    py: f32,
    ch: char,
    color: &Color,
    em: f32,
    font_cache: &HashMap<FontId, FontRef<'_>>,
) -> bool {
    if try_blit_emoji_raster_fallback(pixmap, px, py, em, ch, color) {
        return true;
    }
    if let Some(emoji_font) = font_cache.get(&FontId::EmojiFallback) {
        let eid = emoji_font.glyph_id(ch);
        if eid.0 != 0
            && render_glyph_with_font(
                pixmap,
                px,
                py,
                FontGlyph {
                    font_id: FontId::EmojiFallback,
                    font: emoji_font,
                    glyph_id: eid,
                },
                color,
                em,
            )
        {
            return true;
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn render_glyph(
    pixmap: &mut Pixmap,
    px: f32,
    py: f32,
    font_id: FontId,
    char_code: u32,
    color: &Color,
    font_cache: &HashMap<FontId, FontRef<'_>>,
    em: f32,
) {
    let font = match font_cache.get(&font_id) {
        Some(f) => f,
        None => match font_cache.get(&FontId::MainRegular) {
            Some(f) => f,
            None => return,
        },
    };

    let ch = ratex_font::katex_ttf_glyph_char(font_id, char_code);
    let glyph_id = font.glyph_id(ch);

    if glyph_id.0 == 0 {
        let _ = try_system_unicode_fallback(pixmap, px, py, ch, color, em, font_cache, false);
        return;
    }

    if font_id == FontId::EmojiFallback {
        if try_blit_emoji_raster_fallback(pixmap, px, py, em, ch, color) {
            return;
        }
        let _ = render_glyph_with_font(
            pixmap,
            px,
            py,
            FontGlyph {
                font_id,
                font,
                glyph_id,
            },
            color,
            em,
        );
        return;
    }

    // `RATEX_UNICODE_FONT` may map a codepoint to a non-.notdef glyph with no outlines; try system fallback.
    if font_id == FontId::CjkRegular {
        if render_glyph_with_font(
            pixmap,
            px,
            py,
            FontGlyph {
                font_id: FontId::CjkRegular,
                font,
                glyph_id,
            },
            color,
            em,
        ) {
            return;
        }
        if try_emoji_vector_then_bitmap(pixmap, px, py, ch, color, em, font_cache) {
            return;
        }
        if let Some(fb_font) = font_cache.get(&FontId::CjkFallback) {
            let fid = fb_font.glyph_id(ch);
            if fid.0 != 0
                && render_glyph_with_font(
                    pixmap,
                    px,
                    py,
                    FontGlyph {
                        font_id: FontId::CjkFallback,
                        font: fb_font,
                        glyph_id: fid,
                    },
                    color,
                    em,
                )
            {
                return;
            }
        }
        return;
    }

    if font_id == FontId::CjkFallback {
        if render_glyph_with_font(
            pixmap,
            px,
            py,
            FontGlyph {
                font_id: FontId::CjkFallback,
                font,
                glyph_id,
            },
            color,
            em,
        ) {
            return;
        }
        let _ = try_emoji_vector_then_bitmap(pixmap, px, py, ch, color, em, font_cache);
        return;
    }

    if render_glyph_with_font(
        pixmap,
        px,
        py,
        FontGlyph {
            font_id,
            font,
            glyph_id,
        },
        color,
        em,
    ) {
        return;
    }
    // cmap had a non-zero GID but no `glyf` outline (e.g. blank text-font slot for emoji).
    let skip_main = font_id == FontId::MainRegular;
    let _ = try_system_unicode_fallback(pixmap, px, py, ch, color, em, font_cache, skip_main);
}

struct FontGlyph<'a> {
    font_id: FontId,
    font: &'a FontRef<'a>,
    glyph_id: ab_glyph::GlyphId,
}

struct RasterGlyphParams {
    px: f32,
    py: f32,
    em: f32,
    ch: char,
    opacity: f32,
}

fn render_glyph_with_font(
    pixmap: &mut Pixmap,
    px: f32,
    py: f32,
    g: FontGlyph<'_>,
    color: &Color,
    em: f32,
) -> bool {
    let path = match get_or_compute_glyph_path(&g) {
        Some(path) => path,
        None => return false,
    };

    let units_per_em = g.font.units_per_em().unwrap_or(1000.0);
    let mut scale = em / units_per_em;

    // Emoji outline fallback has no KaTeX metrics; scale it to the 1.0em width that layout
    // allocates for missing emoji so Windows vector fallback does not overflow.
    if g.font_id == FontId::EmojiFallback {
        let actual_advance = g.font.h_advance_unscaled(g.glyph_id);
        let actual_advance_em = actual_advance / units_per_em;
        let assumed_width = 1.0;
        if actual_advance_em > 0.01 && actual_advance_em > assumed_width * 1.01 {
            scale *= assumed_width / actual_advance_em;
        }
    }

    // Streaming formulas repeatedly redraw the same prefix. Cache the final AA
    // glyph raster at an exact scale/subpixel position, then blit it at the
    // integer baseline offset. This preserves positioning while bypassing
    // tiny-skia path rasterization for glyphs already seen in earlier prefixes.
    if let Some((glyph, base_x, base_y)) =
        get_or_compute_glyph_raster(&path, &g, color, scale, px, py)
    {
        blit_cached_glyph(
            pixmap,
            base_x + glyph.offset_x,
            base_y + glyph.offset_y,
            &glyph.pixmap,
        );
        return true;
    }

    // Oversized or otherwise uncacheable glyphs retain the original vector path.
    let mut paint = paint_for_color(color);
    paint.anti_alias = true;
    let transform = Transform::from_row(scale, 0.0, 0.0, -scale, px, py);
    pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);
    true
}

#[inline(always)]
fn highp_source_over_byte(source: u8, destination: u8, source_alpha: u8) -> u8 {
    const BYTE_TO_UNIT: f32 = 1.0 / 255.0;
    let source = source as f32 * BYTE_TO_UNIT;
    let destination = destination as f32 * BYTE_TO_UNIT;
    let alpha = source_alpha as f32 * BYTE_TO_UNIT;
    ((source + destination * (1.0 - alpha)).clamp(0.0, 1.0) * 255.0).round() as u8
}

fn blit_cached_glyph(destination: &mut Pixmap, x: i32, y: i32, source: &Pixmap) {
    let width = source.width() as i32;
    let height = source.height() as i32;
    // tiny-skia's generic draw_pixmap uses special clipping/pattern semantics.
    // Preserve it at image edges; the sprite path is only for fully in-bounds
    // integer glyphs, which is the overwhelmingly common math case.
    if x < 0
        || y < 0
        || x + width > destination.width() as i32
        || y + height > destination.height() as i32
    {
        destination.draw_pixmap(
            x,
            y,
            source.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        return;
    }

    let destination_width = destination.width() as usize;
    let source_width = source.width() as usize;
    let source_height = source.height() as usize;
    let destination_x = x as usize;
    let destination_y = y as usize;
    let source_pixels = source.data();
    let destination_pixels = destination.data_mut();

    for row in 0..source_height {
        let mut source_index = row * source_width * 4;
        let mut destination_index = ((destination_y + row) * destination_width + destination_x) * 4;
        for _ in 0..source_width {
            let source_alpha = source_pixels[source_index + 3];
            if source_alpha != 0 {
                // On a transparent destination SourceOver is exactly source.
                // Opaque source also replaces destination exactly. Most cached
                // glyph pixels hit one of these two branches.
                if destination_pixels[destination_index + 3] == 0 || source_alpha == 255 {
                    destination_pixels[destination_index..destination_index + 4]
                        .copy_from_slice(&source_pixels[source_index..source_index + 4]);
                } else {
                    destination_pixels[destination_index] = highp_source_over_byte(
                        source_pixels[source_index],
                        destination_pixels[destination_index],
                        source_alpha,
                    );
                    destination_pixels[destination_index + 1] = highp_source_over_byte(
                        source_pixels[source_index + 1],
                        destination_pixels[destination_index + 1],
                        source_alpha,
                    );
                    destination_pixels[destination_index + 2] = highp_source_over_byte(
                        source_pixels[source_index + 2],
                        destination_pixels[destination_index + 2],
                        source_alpha,
                    );
                    destination_pixels[destination_index + 3] = highp_source_over_byte(
                        source_alpha,
                        destination_pixels[destination_index + 3],
                        source_alpha,
                    );
                }
            }
            source_index += 4;
            destination_index += 4;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphRasterKey {
    font_id: FontId,
    glyph_id: ab_glyph::GlyphId,
    scale_bits: u32,
    frac_x_bits: u32,
    frac_y_bits: u32,
    rgba: [u8; 4],
}

struct CachedGlyphRaster {
    pixmap: Pixmap,
    offset_x: i32,
    offset_y: i32,
}

const MAX_GLYPH_RASTER_CACHE_ENTRIES: usize = 4096;
const MAX_CACHED_GLYPH_DIMENSION: u32 = 512;

static GLYPH_RASTER_CACHE: LazyLock<RwLock<HashMap<GlyphRasterKey, Arc<CachedGlyphRaster>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

fn get_or_compute_glyph_raster(
    path: &tiny_skia::Path,
    g: &FontGlyph<'_>,
    color: &Color,
    scale: f32,
    px: f32,
    py: f32,
) -> Option<(Arc<CachedGlyphRaster>, i32, i32)> {
    let base_x_f = px.floor();
    let base_y_f = py.floor();
    let base_x = base_x_f as i32;
    let base_y = base_y_f as i32;
    let frac_x = px - base_x_f;
    let frac_y = py - base_y_f;
    let key = GlyphRasterKey {
        font_id: g.font_id,
        glyph_id: g.glyph_id,
        scale_bits: scale.to_bits(),
        frac_x_bits: frac_x.to_bits(),
        frac_y_bits: frac_y.to_bits(),
        rgba: rgba8_for_color(color),
    };

    if let Some(glyph) = GLYPH_RASTER_CACHE.read().ok()?.get(&key).cloned() {
        return Some((glyph, base_x, base_y));
    }

    let bounds = path.bounds();
    let min_x = bounds.left() * scale + frac_x;
    let max_x = bounds.right() * scale + frac_x;
    let min_y = -bounds.bottom() * scale + frac_y;
    let max_y = -bounds.top() * scale + frac_y;
    // One transparent pixel around the conservative path bounds retains AA
    // coverage that can extend beyond the mathematical outline.
    let left = min_x.floor() as i32 - 1;
    let right = max_x.ceil() as i32 + 1;
    let top = min_y.floor() as i32 - 1;
    let bottom = max_y.ceil() as i32 + 1;
    let width = right.checked_sub(left)? as u32;
    let height = bottom.checked_sub(top)? as u32;
    if width == 0
        || height == 0
        || width > MAX_CACHED_GLYPH_DIMENSION
        || height > MAX_CACHED_GLYPH_DIMENSION
    {
        return None;
    }

    let mut glyph_pixmap = Pixmap::new(width, height)?;
    let mut paint = paint_for_color(color);
    paint.anti_alias = true;
    let transform = Transform::from_row(
        scale,
        0.0,
        0.0,
        -scale,
        frac_x - left as f32,
        frac_y - top as f32,
    );
    glyph_pixmap.fill_path(path, &paint, tiny_skia::FillRule::Winding, transform, None);
    let glyph = Arc::new(CachedGlyphRaster {
        pixmap: glyph_pixmap,
        offset_x: left,
        offset_y: top,
    });

    let mut cache = GLYPH_RASTER_CACHE.write().ok()?;
    if cache.len() >= MAX_GLYPH_RASTER_CACHE_ENTRIES {
        cache.clear();
    }
    let cached = cache
        .entry(key)
        .or_insert_with(|| Arc::clone(&glyph))
        .clone();
    Some((cached, base_x, base_y))
}

static GLYPH_PATH_CACHE: LazyLock<
    RwLock<HashMap<(FontId, ab_glyph::GlyphId), Arc<tiny_skia::Path>>>,
> = LazyLock::new(|| RwLock::new(HashMap::new()));

fn get_or_compute_glyph_path(g: &FontGlyph<'_>) -> Option<Arc<tiny_skia::Path>> {
    let key = (g.font_id, g.glyph_id);
    if let Some(path) = GLYPH_PATH_CACHE.read().ok()?.get(&key).cloned() {
        return Some(path);
    }

    let curves =
        ratex_font_loader::outline_cache::get_or_compute_outline(g.font_id, g.font, g.glyph_id)?;
    if curves.is_empty() {
        return None;
    }

    let mut builder = PathBuilder::new();
    let mut last_end: Option<(f32, f32)> = None;
    for curve in curves.iter() {
        use ab_glyph::OutlineCurve;
        let (start, end) = match curve {
            OutlineCurve::Line(p0, p1) => ((p0.x, p0.y), (p1.x, p1.y)),
            OutlineCurve::Quad(p0, _, p2) => ((p0.x, p0.y), (p2.x, p2.y)),
            OutlineCurve::Cubic(p0, _, _, p3) => ((p0.x, p0.y), (p3.x, p3.y)),
        };

        let need_move = match last_end {
            None => true,
            Some((lx, ly)) => (lx - start.0).abs() > 0.01 || (ly - start.1).abs() > 0.01,
        };
        if need_move {
            if last_end.is_some() {
                builder.close();
            }
            builder.move_to(start.0, start.1);
        }

        match curve {
            OutlineCurve::Line(_, p1) => builder.line_to(p1.x, p1.y),
            OutlineCurve::Quad(_, p1, p2) => builder.quad_to(p1.x, p1.y, p2.x, p2.y),
            OutlineCurve::Cubic(_, p1, p2, p3) => {
                builder.cubic_to(p1.x, p1.y, p2.x, p2.y, p3.x, p3.y)
            }
        }
        last_end = Some(end);
    }
    if last_end.is_some() {
        builder.close();
    }
    let path = Arc::new(builder.finish()?);

    let mut cache = GLYPH_PATH_CACHE.write().ok()?;
    Some(
        cache
            .entry(key)
            .or_insert_with(|| Arc::clone(&path))
            .clone(),
    )
}

/// Color emoji (sbix / CBDT / etc.) often have no `glyf` outlines; `ttf-parser` embedded strikes + PNG.
fn try_blit_emoji_raster_fallback(
    pixmap: &mut Pixmap,
    px: f32,
    py: f32,
    em: f32,
    ch: char,
    color: &Color,
) -> bool {
    let Some(bytes) = ratex_unicode_font::load_emoji_font_arc() else {
        return false;
    };
    let idx = ratex_unicode_font::emoji_font_face_index().unwrap_or(0);
    try_blit_raster_glyph(
        pixmap,
        RasterGlyphParams {
            px,
            py,
            em,
            ch,
            opacity: normalized_alpha(color.a),
        },
        bytes.as_slice(),
        idx,
    )
}

fn try_blit_raster_glyph(
    pixmap: &mut Pixmap,
    params: RasterGlyphParams,
    font_bytes: &[u8],
    face_index: u32,
) -> bool {
    let face = match ttf_parser::Face::parse(font_bytes, face_index) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let gid = match face.glyph_index(params.ch) {
        Some(g) => g,
        None => return false,
    };
    let strike = params.em.round().clamp(8.0, 256.0) as u16;
    let img = face
        .glyph_raster_image(gid, strike)
        .or_else(|| face.glyph_raster_image(gid, u16::MAX));
    let Some(img) = img else {
        return false;
    };
    let glyph_pm = match raster_glyph_image_to_pixmap(&img) {
        Some(p) => p,
        None => return false,
    };
    let ppm = f32::from(img.pixels_per_em.max(1));
    let mut scale = params.em / ppm;
    // Scale emoji to fit 1.0em layout width if it's wider (prevents overflow).
    let actual_width_em = f32::from(img.width) / ppm;
    let assumed_width = 1.0;
    if actual_width_em > 0.01 && actual_width_em > assumed_width * 1.01 {
        scale *= assumed_width / actual_width_em;
    }
    let top_x = params.px + f32::from(img.x) * scale;
    // `ttf-parser` / OpenType: `RasterGlyphImage::{x,y}` are in strike pixels; `y` is the
    // **bottom** edge of the bitmap in y-up coordinates (sbix yOffset to bottom; CBDT normalized
    // the same way). Top edge = y + height — using `y` alone shifts the glyph down by ~full height.
    let mut top_y = params.py - (f32::from(img.y) + f32::from(img.height)) * scale;
    // sbix places the bitmap bottom on the math baseline, but tall (~1em) color strikes put the
    // ink centroid near 0.5em above baseline. Binary/relation glyphs (+, =) are centered on the
    // math axis (~0.25em). Nudge the bitmap so its vertical center matches the axis — matches
    // mixed `\text{emoji} … formula` rows without changing layout baselines.
    let center_strike = (f32::from(img.y) + f32::from(img.height) / 2.0) / ppm;
    let axis = ratex_font::get_global_metrics(0).axis_height as f32;
    top_y += (center_strike - axis) * params.em;
    let paint = PixmapPaint {
        opacity: params.opacity,
        quality: FilterQuality::Bilinear,
        ..Default::default()
    };
    let transform = Transform::from_row(scale, 0.0, 0.0, scale, top_x, top_y);
    pixmap.draw_pixmap(0, 0, glyph_pm.as_ref(), &paint, transform, None);
    true
}

fn raster_glyph_image_to_pixmap(img: &ttf_parser::RasterGlyphImage<'_>) -> Option<Pixmap> {
    use ttf_parser::RasterImageFormat;
    let w = u32::from(img.width);
    let h = u32::from(img.height);
    let size = tiny_skia::IntSize::from_wh(w, h)?;
    match img.format {
        RasterImageFormat::PNG => Pixmap::decode_png(img.data).ok(),
        RasterImageFormat::BitmapPremulBgra32 => {
            let expected = 4usize * w as usize * h as usize;
            if img.data.len() != expected {
                return None;
            }
            let mut v = Vec::with_capacity(expected);
            for px in img.data.chunks_exact(4) {
                let b = px[0];
                let g = px[1];
                let r = px[2];
                let a = px[3];
                v.extend_from_slice(&[r, g, b, a]);
            }
            Pixmap::from_vec(v, size)
        }
        RasterImageFormat::BitmapGray8 => {
            let mut v = Vec::with_capacity(4 * img.data.len());
            for &g in img.data {
                v.extend_from_slice(&[g, g, g, 255]);
            }
            Pixmap::from_vec(v, size)
        }
        _ => None,
    }
}

fn render_line(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: &Color,
    dashed: bool,
) {
    let t = thickness.max(1.0);
    let paint = paint_for_color(color);

    if dashed {
        // Draw a dashed line: dash length = 4t, gap = 4t.
        let dash_len = (4.0 * t).max(2.0);
        let gap_len = (4.0 * t).max(2.0);
        let period = dash_len + gap_len;
        let top = y - t / 2.0;
        let mut cur_x = x;
        while cur_x < x + width {
            let seg_width = (dash_len).min(x + width - cur_x);
            let seg_width = seg_width.max(2.0);
            if let Some(rect) = tiny_skia::Rect::from_xywh(cur_x, top, seg_width, t) {
                pixmap.fill_rect(rect, &paint, Transform::identity(), None);
            }
            cur_x += period;
        }
    } else if let Some(rect) = tiny_skia::Rect::from_xywh(x, y - t / 2.0, width, t) {
        pixmap.fill_rect(rect, &paint, Transform::identity(), None);
    }
}

fn render_rect(pixmap: &mut Pixmap, x: f32, y: f32, width: f32, height: f32, color: &Color) {
    // tiny-skia's fill_rect fast path requires a full interior pixel. Preserve
    // sub-2px TeX rules by routing them through the anti-aliased path filler.
    if width < 2.0 || height < 2.0 {
        let Some(rect) = tiny_skia::Rect::from_xywh(x, y, width, height) else {
            return;
        };
        let path = PathBuilder::from_rect(rect);
        let mut paint = paint_for_color(color);
        paint.anti_alias = true;
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
        return;
    }
    let rect = tiny_skia::Rect::from_xywh(x, y, width, height);
    if let Some(rect) = rect {
        let paint = paint_for_color(color);
        pixmap.fill_rect(rect, &paint, Transform::identity(), None);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_path(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    commands: &[ratex_types::path_command::PathCommand],
    fill: bool,
    color: &Color,
    em: f32,
    stroke_width_px: f32,
) {
    // For filled paths, render each subpath (delimited by MoveTo) as a separate
    // fill_path call.  KaTeX stretchy arrows are assembled from multiple path
    // components (e.g. "lefthook" + "rightarrow") whose winding directions can
    // be opposite.  Combining them into a single fill_path with FillRule::Winding
    // causes the shaft region to cancel out (net winding = 0 → unfilled).
    // Drawing each subpath independently avoids cross-component winding interactions.
    if fill {
        let mut start = 0;
        for i in 1..commands.len() {
            if matches!(
                commands[i],
                ratex_types::path_command::PathCommand::MoveTo { .. }
            ) {
                render_path_segment(
                    pixmap,
                    x,
                    y,
                    &commands[start..i],
                    fill,
                    color,
                    em,
                    stroke_width_px,
                );
                start = i;
            }
        }
        render_path_segment(
            pixmap,
            x,
            y,
            &commands[start..],
            fill,
            color,
            em,
            stroke_width_px,
        );
        return;
    }
    render_path_segment(pixmap, x, y, commands, fill, color, em, stroke_width_px);
}

#[allow(clippy::too_many_arguments)]
fn render_path_segment(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    commands: &[ratex_types::path_command::PathCommand],
    fill: bool,
    color: &Color,
    em: f32,
    stroke_width_px: f32,
) {
    let mut builder = PathBuilder::new();
    for cmd in commands {
        match cmd {
            ratex_types::path_command::PathCommand::MoveTo { x: cx, y: cy } => {
                builder.move_to(x + *cx as f32 * em, y + *cy as f32 * em);
            }
            ratex_types::path_command::PathCommand::LineTo { x: cx, y: cy } => {
                builder.line_to(x + *cx as f32 * em, y + *cy as f32 * em);
            }
            ratex_types::path_command::PathCommand::CubicTo {
                x1,
                y1,
                x2,
                y2,
                x: cx,
                y: cy,
            } => {
                builder.cubic_to(
                    x + *x1 as f32 * em,
                    y + *y1 as f32 * em,
                    x + *x2 as f32 * em,
                    y + *y2 as f32 * em,
                    x + *cx as f32 * em,
                    y + *cy as f32 * em,
                );
            }
            ratex_types::path_command::PathCommand::QuadTo {
                x1,
                y1,
                x: cx,
                y: cy,
            } => {
                builder.quad_to(
                    x + *x1 as f32 * em,
                    y + *y1 as f32 * em,
                    x + *cx as f32 * em,
                    y + *cy as f32 * em,
                );
            }
            ratex_types::path_command::PathCommand::Close => {
                builder.close();
            }
        }
    }

    if let Some(path) = builder.finish() {
        let mut paint = paint_for_color(color);
        if fill {
            paint.anti_alias = true;
            // Even-odd: KaTeX `tallDelim` vert uses two subpaths (outline + stem); nonzero winding
            // double-fills the stem and inflates ink vs reference PNGs.
            pixmap.fill_path(
                &path,
                &paint,
                FillRule::EvenOdd,
                Transform::identity(),
                None,
            );
        } else {
            let stroke = Stroke {
                width: stroke_width_px,
                ..Default::default()
            };
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }
}

fn encode_png(pixmap: &Pixmap) -> Result<Vec<u8>, String> {
    pixmap
        .encode_png()
        .map_err(|e| format!("PNG encode error: {}", e))
}
