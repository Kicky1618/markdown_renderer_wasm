#[path = "../src/compat.rs"]
mod compat;

use compat::{RendererBackend, RendererPreference, quantize_coverage};

#[test]
fn parses_renderer_preference() {
    assert_eq!(
        RendererPreference::from_search(""),
        RendererPreference::Auto
    );
    assert_eq!(
        RendererPreference::from_search("?renderer=webgpu&tps=1"),
        RendererPreference::WebGpu
    );
    assert_eq!(
        RendererPreference::from_search("?renderer=webgl2"),
        RendererPreference::WebGl2
    );
    assert_eq!(
        RendererPreference::from_search("?renderer=canvas"),
        RendererPreference::Canvas2d
    );
    assert_eq!(
        RendererPreference::from_search("?renderer=future"),
        RendererPreference::Auto
    );
}

#[test]
fn fallback_order_never_upgrades_a_forced_lower_backend() {
    assert_eq!(
        RendererPreference::Auto.fallback_chain(),
        &[
            RendererBackend::WebGpu,
            RendererBackend::WebGl2,
            RendererBackend::Canvas2d,
        ]
    );
    assert_eq!(
        RendererPreference::WebGl2.fallback_chain(),
        &[RendererBackend::WebGl2, RendererBackend::Canvas2d]
    );
    assert_eq!(
        RendererPreference::Canvas2d.fallback_chain(),
        &[RendererBackend::Canvas2d]
    );
}

#[test]
fn webgl_policy_reduces_scene_churn_without_changing_scene_model() {
    let webgpu = RendererBackend::WebGpu.policy();
    let webgl = RendererBackend::WebGl2.policy();
    assert!(webgpu.gpu_scene && webgl.gpu_scene);
    assert_eq!(webgpu.scene_rebuild_interval_ms, 0.0);
    assert!(webgl.scene_rebuild_interval_ms >= 30.0);
    assert_eq!(webgpu.glyph_coverage_quantum, 1);
    assert!(webgl.glyph_coverage_quantum > 1);
}

#[test]
fn coverage_quantization_preserves_extremes() {
    assert_eq!(quantize_coverage(0, 32), 0);
    assert_eq!(quantize_coverage(255, 32), 255);
    assert_eq!(quantize_coverage(1, 1), 1);
    assert_eq!(quantize_coverage(64, 32), 64);
    assert_eq!(quantize_coverage(250, 32), 255);
}
