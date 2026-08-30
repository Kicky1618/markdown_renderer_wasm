#[path = "../src/compat.rs"]
mod compat;

use compat::{
    RendererBackend, RendererPreference, SurfaceAction, SurfaceEvent, SurfaceFailureTracker,
    gpu_canvas_metrics, prefer_display_encoded_format, quantize_coverage, replace_renderer_search,
    runtime_recovery_search, runtime_recovery_trace,
};

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

#[test]
fn gpu_hidpi_separates_logical_and_backing_dimensions() {
    let retina = gpu_canvas_metrics(800, 600, 2.0, 8192);
    assert_eq!((retina.logical_width, retina.logical_height), (800, 600));
    assert_eq!((retina.backing_width, retina.backing_height), (1600, 1200));
    assert!((retina.backing_scale - 2.0).abs() < 1e-9);

    let capped_dpr = gpu_canvas_metrics(800, 600, 3.5, 8192);
    assert_eq!(
        (capped_dpr.backing_width, capped_dpr.backing_height),
        (1600, 1200)
    );

    let dimension_limited = gpu_canvas_metrics(800, 600, 2.0, 1024);
    assert_eq!(dimension_limited.backing_width, 1024);
    assert_eq!(dimension_limited.backing_height, 768);

    let area_limited = gpu_canvas_metrics(5000, 5000, 2.0, 8192);
    assert_eq!(
        (area_limited.backing_width, area_limited.backing_height),
        (4096, 4096)
    );
    assert!(area_limited.backing_scale < 1.0);

    let uneven_area_limited = gpu_canvas_metrics(5001, 4999, 2.0, 8192);
    assert!(
        uneven_area_limited.backing_width as u64 * uneven_area_limited.backing_height as u64
            <= compat::GPU_MAX_BACKING_PIXELS
    );

    let invalid_dpr = gpu_canvas_metrics(640, 480, f64::NAN, 8192);
    assert_eq!(
        (invalid_dpr.backing_width, invalid_dpr.backing_height),
        (640, 480)
    );
}

#[test]
fn runtime_recovery_only_moves_to_lower_backends() {
    assert_eq!(
        RendererBackend::WebGpu.recovery_preference(),
        Some(RendererPreference::WebGl2)
    );
    assert_eq!(
        RendererBackend::WebGl2.recovery_preference(),
        Some(RendererPreference::Canvas2d)
    );
    assert_eq!(RendererBackend::Canvas2d.recovery_preference(), None);
}

#[test]
fn renderer_search_rewrite_preserves_unrelated_runtime_options() {
    assert_eq!(
        replace_renderer_search(
            "?renderer=webgpu&doc=easy&tps=1000",
            RendererPreference::WebGl2
        ),
        "?renderer=webgl2&doc=easy&tps=1000"
    );
    assert_eq!(
        replace_renderer_search("?doc=easy&smoke=1", RendererPreference::Canvas2d),
        "?renderer=canvas2d&doc=easy&smoke=1"
    );
}

#[test]
fn runtime_recovery_search_preserves_origin_and_accumulates_depth() {
    let first = runtime_recovery_search(
        "?renderer=auto&doc=easy&smoke=1",
        RendererPreference::WebGl2,
    );
    assert_eq!(
        first,
        "?renderer=webgl2&doc=easy&smoke=1&renderer_runtime_origin=auto&renderer_runtime_depth=1"
    );
    let trace = runtime_recovery_trace(&first).unwrap();
    assert_eq!(trace.origin, "auto");
    assert_eq!(trace.depth, 1);

    let second = runtime_recovery_search(&first, RendererPreference::Canvas2d);
    assert_eq!(
        second,
        "?renderer=canvas2d&doc=easy&smoke=1&renderer_runtime_origin=auto&renderer_runtime_depth=2"
    );
    let trace = runtime_recovery_trace(&second).unwrap();
    assert_eq!(trace.origin, "auto");
    assert_eq!(trace.depth, 2);
}

#[test]
fn repeated_surface_loss_escalates_but_timeout_and_occlusion_do_not() {
    let mut tracker = SurfaceFailureTracker::default();
    assert_eq!(
        tracker.observe(SurfaceEvent::Timeout),
        SurfaceAction::Continue
    );
    assert_eq!(
        tracker.observe(SurfaceEvent::Occluded),
        SurfaceAction::Continue
    );
    assert_eq!(
        tracker.observe(SurfaceEvent::Reconfigure),
        SurfaceAction::Reconfigure
    );
    assert_eq!(
        tracker.observe(SurfaceEvent::Reconfigure),
        SurfaceAction::Reconfigure
    );
    assert_eq!(
        tracker.observe(SurfaceEvent::Reconfigure),
        SurfaceAction::Recover
    );

    let mut tracker = SurfaceFailureTracker::default();
    assert_eq!(
        tracker.observe(SurfaceEvent::Reconfigure),
        SurfaceAction::Reconfigure
    );
    assert_eq!(
        tracker.observe(SurfaceEvent::Presented),
        SurfaceAction::Continue
    );
    assert_eq!(
        tracker.observe(SurfaceEvent::Reconfigure),
        SurfaceAction::Reconfigure
    );
    assert_eq!(
        tracker.observe(SurfaceEvent::Validation),
        SurfaceAction::Recover
    );
}

#[test]
fn surface_format_prefers_display_encoded_target() {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Format {
        Linear,
        Srgb,
    }
    let selected = prefer_display_encoded_format(&[Format::Srgb, Format::Linear], |format| {
        format == Format::Srgb
    });
    assert_eq!(selected, Some(Format::Linear));

    let srgb_only = prefer_display_encoded_format(&[Format::Srgb], |_| true);
    assert_eq!(srgb_only, Some(Format::Srgb));
    assert_eq!(
        prefer_display_encoded_format::<Format>(&[], |_| false),
        None
    );
}
