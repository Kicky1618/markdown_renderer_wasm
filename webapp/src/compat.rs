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
