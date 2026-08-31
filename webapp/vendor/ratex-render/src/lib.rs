mod renderer;

pub use renderer::{
    render_to_png, render_to_rgba_premultiplied, render_to_rgba_premultiplied_append,
    PremultipliedRgbaImage, RenderOptions,
};
