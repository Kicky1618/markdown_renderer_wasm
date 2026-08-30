struct View {
    width: f32,
    height: f32,
    scroll: f32,
    math_scroll: f32,
};

@group(0) @binding(0) var<uniform> view: View;

struct VertexIn {
    @location(0) geometry: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) flags: u32,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexIn, @builtin(vertex_index) vertex_index: u32) -> VertexOut {
    let corners = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0),
    );
    let corner = corners[vertex_index];
    var position: vec2<f32>;
    if (input.flags & 8u) != 0u {
        let delta = input.geometry.zw - input.geometry.xy;
        let segment_length = max(length(delta), 0.001);
        let line_width = f32(input.flags >> 8u) / 256.0;
        let normal = vec2<f32>(-delta.y, delta.x) / segment_length * line_width * 0.5;
        position = mix(input.geometry.xy, input.geometry.zw, corner.x)
            + normal * (1.0 - corner.y * 2.0);
    } else {
        position = input.geometry.xy + input.geometry.zw * corner;
    }
    let fixed = f32(input.flags & 1u);
    let math = f32((input.flags >> 1u) & 1u);
    var x = position.x - view.math_scroll * math;
    var y = position.y - view.scroll * (1.0 - fixed);
    if (input.flags & 4u) != 0u {
        x = round(x);
        y = round(y);
    }
    var out: VertexOut;
    out.position = vec4<f32>(x / view.width * 2.0 - 1.0,
                             1.0 - y / view.height * 2.0, 0.0, 1.0);
    out.color = input.color;
    return out;
}

@fragment
fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    return input.color;
}
