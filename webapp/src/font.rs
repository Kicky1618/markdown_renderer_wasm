//! Antialiased text rasterization from embedded Noto Sans fonts.
//! Glyph bitmaps are cached because the streaming renderer rebuilds visible
//! blocks frequently while the unstable Markdown tail changes.

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::OnceLock};

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};

const FALLBACK_FONT_BYTES: &[u8] = include_bytes!("../assets/NotoSansCJKjp-Streamdown.ttf");
const MONO_FONT_BYTES: &[u8] = include_bytes!("../assets/NotoSansMono-Regular.ttf");
const BASE_FONT_SIZE: f32 = 16.0;
const CACHE_LIMIT: usize = 4096;

#[derive(Debug)]
pub struct GlyphBitmap {
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
    pub advance: f32,
    pub coverage: Vec<u8>,
}

static FALLBACK_FONT: OnceLock<FontRef<'static>> = OnceLock::new();
static MONO_FONT: OnceLock<FontRef<'static>> = OnceLock::new();

thread_local! {
    static GLYPHS: RefCell<HashMap<(char, u16, bool), Rc<GlyphBitmap>>> = RefCell::new(HashMap::new());
}

fn fallback_font() -> &'static FontRef<'static> {
    FALLBACK_FONT.get_or_init(|| {
        FontRef::try_from_slice(FALLBACK_FONT_BYTES).expect("embedded Noto font is valid")
    })
}

fn primary_font() -> &'static FontRef<'static> {
    fallback_font()
}

fn mono_font() -> &'static FontRef<'static> {
    MONO_FONT.get_or_init(|| {
        FontRef::try_from_slice(MONO_FONT_BYTES).expect("embedded Noto Sans Mono font is valid")
    })
}

fn cache_scale(scale: f32) -> u16 {
    (scale.clamp(0.25, 8.0) * 64.0).round() as u16
}

pub fn glyph(c: char, scale: f32) -> Rc<GlyphBitmap> {
    glyph_for(c, scale, false)
}

pub fn mono_glyph(c: char, scale: f32) -> Rc<GlyphBitmap> {
    glyph_for(c, scale, true)
}

fn glyph_for(c: char, scale: f32, mono: bool) -> Rc<GlyphBitmap> {
    let key = (c, cache_scale(scale), mono);
    if let Some(glyph) = GLYPHS.with(|cache| cache.borrow().get(&key).cloned()) {
        return glyph;
    }
    let glyph = Rc::new(rasterize(c, key.1 as f32 / 64.0, mono));
    GLYPHS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, glyph.clone());
    });
    glyph
}

pub fn advance(c: char, scale: f32) -> f32 {
    glyph(c, scale).advance
}

pub fn mono_advance(c: char, scale: f32) -> f32 {
    mono_glyph(c, scale).advance
}

pub fn baseline(scale: f32) -> f32 {
    let px_scale = PxScale::from(BASE_FONT_SIZE * scale);
    primary_font().as_scaled(px_scale).ascent()
}

fn rasterize(c: char, scale: f32, mono: bool) -> GlyphBitmap {
    let primary = if mono { mono_font() } else { primary_font() };
    let fallback = fallback_font();
    let px_scale = PxScale::from(BASE_FONT_SIZE * scale);
    let mut font = primary;
    let mut id = primary.glyph_id(c);
    if id.0 == 0 && c != '\0' {
        font = fallback;
        id = fallback.glyph_id(c);
        if id.0 == 0 {
            id = fallback.glyph_id('\u{fffd}');
        }
    }
    let scaled = font.as_scaled(px_scale);
    let advance = scaled.h_advance(id);
    let positioned = id.with_scale_and_position(px_scale, point(0.0, scaled.ascent()));
    let Some(outlined) = font.outline_glyph(positioned) else {
        return GlyphBitmap {
            width: 0,
            height: 0,
            left: 0,
            top: 0,
            advance,
            coverage: Vec::new(),
        };
    };
    let bounds = outlined.px_bounds();
    let width = bounds.width().ceil().max(0.0) as u32;
    let height = bounds.height().ceil().max(0.0) as u32;
    let mut coverage = vec![0; (width * height) as usize];
    outlined.draw(|x, y, value| {
        if x < width && y < height {
            coverage[(y * width + x) as usize] = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    });
    GlyphBitmap {
        width,
        height,
        left: bounds.min.x.floor() as i32,
        top: bounds.min.y.floor() as i32,
        advance,
        coverage,
    }
}
