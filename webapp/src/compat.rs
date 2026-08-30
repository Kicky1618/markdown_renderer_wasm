//! Renderer compatibility policy shared by the browser startup path and tests.
//!
//! Keep this module free of `web_sys`/`wgpu` types.  Backend probing belongs to
//! the caller; this file only defines deterministic fallback order and the
//! lowest-common-denominator rendering policy.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RendererPreference {
    #[default]
    Auto,
    WebGpu,
    WebGl2,
    Canvas2d,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererBackend {
    WebGpu,
    WebGl2,
    Canvas2d,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceEvent {
    Presented,
    Reconfigure,
    Timeout,
    Occluded,
    Validation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceAction {
    Continue,
    Reconfigure,
    Recover,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SurfaceFailureTracker {
    consecutive_losses: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRecoveryTrace {
    pub origin: String,
    pub depth: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuCanvasMetrics {
    pub logical_width: u32,
    pub logical_height: u32,
    pub backing_width: u32,
    pub backing_height: u32,
    pub backing_scale: f64,
}

pub const GPU_MAX_DPR: f64 = 2.0;
pub const GPU_MAX_BACKING_PIXELS: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RendererPolicy {
    /// Minimum delay between expensive scene rebuilds. Presentation can still
    /// happen every animation frame because scrolling is applied by a uniform.
    pub scene_rebuild_interval_ms: f64,
    /// Coverage values are rounded to this quantum before GPU rectangle runs
    /// are generated. WebGL2 benefits from fewer tiny runs; WebGPU keeps full
    /// 8-bit coverage.
    pub glyph_coverage_quantum: u8,
    /// Whether this backend uses the common instanced-rectangle GPU scene.
    pub gpu_scene: bool,
}

const AUTO_CHAIN: [RendererBackend; 3] = [
    RendererBackend::WebGpu,
    RendererBackend::WebGl2,
    RendererBackend::Canvas2d,
];
const WEBGL_CHAIN: [RendererBackend; 2] = [RendererBackend::WebGl2, RendererBackend::Canvas2d];
const CANVAS_CHAIN: [RendererBackend; 1] = [RendererBackend::Canvas2d];

impl RendererPreference {
    pub fn from_search(search: &str) -> Self {
        search
            .trim_start_matches('?')
            .split('&')
            .filter_map(|part| part.split_once('='))
            .find_map(|(key, value)| {
                (key == "renderer").then(|| match value.to_ascii_lowercase().as_str() {
                    "webgpu" => Self::WebGpu,
                    "webgl" | "webgl2" => Self::WebGl2,
                    "canvas" | "canvas2d" => Self::Canvas2d,
                    "auto" => Self::Auto,
                    _ => Self::Auto,
                })
            })
            .unwrap_or_default()
    }

    pub fn fallback_chain(self) -> &'static [RendererBackend] {
        match self {
            Self::Auto | Self::WebGpu => &AUTO_CHAIN,
            Self::WebGl2 => &WEBGL_CHAIN,
            Self::Canvas2d => &CANVAS_CHAIN,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::WebGpu => "webgpu",
            Self::WebGl2 => "webgl2",
            Self::Canvas2d => "canvas2d",
        }
    }
}

impl RendererBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WebGpu => "webgpu",
            Self::WebGl2 => "webgl",
            Self::Canvas2d => "canvas2d",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::WebGpu => "WEBGPU",
            Self::WebGl2 => "WEBGL2",
            Self::Canvas2d => "CANVAS2D",
        }
    }

    pub const fn recovery_preference(self) -> Option<RendererPreference> {
        match self {
            Self::WebGpu => Some(RendererPreference::WebGl2),
            Self::WebGl2 => Some(RendererPreference::Canvas2d),
            Self::Canvas2d => None,
        }
    }

    pub const fn policy(self) -> RendererPolicy {
        match self {
            Self::WebGpu => RendererPolicy {
                scene_rebuild_interval_ms: 0.0,
                glyph_coverage_quantum: 1,
                gpu_scene: true,
            },
            Self::WebGl2 => RendererPolicy {
                // WebGL command submission and many tiny coverage runs are much
                // more expensive in browsers, so rebuild the overscanned scene
                // at at most ~30 Hz while still presenting smooth scroll frames.
                scene_rebuild_interval_ms: 33.0,
                glyph_coverage_quantum: 32,
                gpu_scene: true,
            },
            Self::Canvas2d => RendererPolicy {
                scene_rebuild_interval_ms: 0.0,
                glyph_coverage_quantum: 1,
                gpu_scene: false,
            },
        }
    }
}

pub fn quantize_coverage(value: u8, quantum: u8) -> u8 {
    if quantum <= 1 || value == 0 || value == u8::MAX {
        return value;
    }
    let quantum = quantum as u16;
    ((((value as u16 + quantum / 2) / quantum) * quantum).min(u8::MAX as u16)) as u8
}

impl SurfaceFailureTracker {
    pub fn observe(&mut self, event: SurfaceEvent) -> SurfaceAction {
        match event {
            SurfaceEvent::Presented => {
                self.consecutive_losses = 0;
                SurfaceAction::Continue
            }
            SurfaceEvent::Reconfigure => {
                self.consecutive_losses = self.consecutive_losses.saturating_add(1);
                if self.consecutive_losses >= 3 {
                    SurfaceAction::Recover
                } else {
                    SurfaceAction::Reconfigure
                }
            }
            SurfaceEvent::Validation => SurfaceAction::Recover,
            SurfaceEvent::Timeout | SurfaceEvent::Occluded => SurfaceAction::Continue,
        }
    }
}

fn replace_search_param(search: &str, key: &str, value: &str) -> String {
    let mut parts = Vec::new();
    let mut replaced = false;
    for part in search.trim_start_matches('?').split('&') {
        if part.is_empty() {
            continue;
        }
        let part_key = part.split_once('=').map_or(part, |(part_key, _)| part_key);
        if part_key == key {
            if !replaced {
                parts.push(format!("{key}={value}"));
                replaced = true;
            }
        } else {
            parts.push(part.to_owned());
        }
    }
    if !replaced {
        parts.push(format!("{key}={value}"));
    }
    format!("?{}", parts.join("&"))
}

pub fn replace_renderer_search(search: &str, preference: RendererPreference) -> String {
    let mut parts = Vec::new();
    let mut replaced = false;
    for part in search.trim_start_matches('?').split('&') {
        if part.is_empty() {
            continue;
        }
        let key = part.split_once('=').map_or(part, |(key, _)| key);
        if key == "renderer" {
            if !replaced {
                parts.push(format!("renderer={}", preference.as_str()));
                replaced = true;
            }
        } else {
            parts.push(part.to_owned());
        }
    }
    if !replaced {
        parts.insert(0, format!("renderer={}", preference.as_str()));
    }
    format!("?{}", parts.join("&"))
}

pub fn runtime_recovery_trace(search: &str) -> Option<RuntimeRecoveryTrace> {
    let mut origin = None;
    let mut depth = None;
    for part in search.trim_start_matches('?').split('&') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key {
            "renderer_runtime_origin" if !value.is_empty() => origin = Some(value.to_owned()),
            "renderer_runtime_depth" => depth = value.parse::<usize>().ok(),
            _ => {}
        }
    }
    let depth = depth.filter(|depth| *depth > 0)?;
    Some(RuntimeRecoveryTrace {
        origin: origin.unwrap_or_else(|| "unknown".to_owned()),
        depth,
    })
}

pub fn runtime_recovery_search(search: &str, next: RendererPreference) -> String {
    let previous = runtime_recovery_trace(search);
    let origin = previous
        .as_ref()
        .map(|trace| trace.origin.clone())
        .unwrap_or_else(|| RendererPreference::from_search(search).as_str().to_owned());
    let depth = previous.map_or(1, |trace| trace.depth.saturating_add(1));
    let rewritten = replace_renderer_search(search, next);
    let rewritten = replace_search_param(&rewritten, "renderer_runtime_origin", &origin);
    replace_search_param(&rewritten, "renderer_runtime_depth", &depth.to_string())
}

pub fn gpu_canvas_metrics(
    logical_width: u32,
    logical_height: u32,
    device_pixel_ratio: f64,
    max_dimension: u32,
) -> GpuCanvasMetrics {
    let logical_width = logical_width.max(1);
    let logical_height = logical_height.max(1);
    let max_dimension = max_dimension.max(1);
    let requested_scale = if device_pixel_ratio.is_finite() && device_pixel_ratio > 0.0 {
        device_pixel_ratio.clamp(1.0, GPU_MAX_DPR)
    } else {
        1.0
    };
    let dimension_scale = (max_dimension as f64 / logical_width as f64)
        .min(max_dimension as f64 / logical_height as f64);
    let logical_pixels = logical_width as f64 * logical_height as f64;
    let area_scale = (GPU_MAX_BACKING_PIXELS as f64 / logical_pixels).sqrt();
    let scale = requested_scale.min(dimension_scale).min(area_scale);
    let backing_width =
        ((logical_width as f64 * scale).round() as u64).clamp(1, max_dimension as u64) as u32;
    let backing_height =
        ((logical_height as f64 * scale).round() as u64).clamp(1, max_dimension as u64) as u32;
    let backing_scale = (backing_width as f64 / logical_width as f64)
        .min(backing_height as f64 / logical_height as f64);
    GpuCanvasMetrics {
        logical_width,
        logical_height,
        backing_width,
        backing_height,
        backing_scale,
    }
}

/// Prefer a non-sRGB presentation format for the renderer's existing palette.
///
/// GPU color constants are authored as display/sRGB byte values to match the
/// Canvas2D CSS palette. Rendering those values into an sRGB attachment would
/// apply an extra transfer function and make WebGPU/WebGL2 visibly diverge.
pub fn prefer_display_encoded_format<T: Copy>(
    formats: &[T],
    is_srgb: impl Fn(T) -> bool,
) -> Option<T> {
    formats
        .iter()
        .copied()
        .find(|format| !is_srgb(*format))
        .or_else(|| formats.first().copied())
}
