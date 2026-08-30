#![cfg(target_arch = "wasm32")]

mod canvas2d;
mod code;
mod compat;
mod font;
mod math;
mod search;

use bytemuck::{Pod, Zeroable};
use compat::{RendererBackend, RendererPreference};
use futures_util::{
    FutureExt,
    future::{Either, select},
};
use js_sys::Promise;
use search::SearchTrie;
use std::{cell::RefCell, rc::Rc};
use streamdown::{Block, Delta, Inline, Op, Parser};
use wasm_bindgen::{JsCast, closure::Closure, prelude::*};
use wasm_bindgen_futures::JsFuture;
use web_sys::{HtmlCanvasElement, KeyboardEvent, MouseEvent, WheelEvent};
use wgpu::util::DeviceExt;

type AnimationLoop = Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>;

const DEFAULT_MOCK: &str = include_str!("../../FORMAT.md");
const EASY_MOCK: &str = include_str!("../../easy_test.md");
const STRESS_MOCK: &str = include_str!("../../math_stress_test.md");
const CODE_MOCK: &str = include_str!("../../code_test.md");
const DEFAULT_TPS: f64 = 50_000.0;
const DEFAULT_REPEATS: usize = 250;
const DEFAULT_FONT_SIZE: f32 = 16.0;
const DEFAULT_FADE_MS: f64 = 180.0;
const MAX_TOKENS_PER_FRAME: usize = 250_000;
const GPU_INIT_TIMEOUT_MS: i32 = 5_000;
const BG: [f32; 4] = [0.035, 0.047, 0.071, 1.0];
const FG: [f32; 4] = [0.82, 0.86, 0.91, 1.0];
const MUTED: [f32; 4] = [0.47, 0.53, 0.62, 1.0];
const CYAN: [f32; 4] = [0.25, 0.86, 0.88, 1.0];
const GREEN: [f32; 4] = [0.45, 0.88, 0.62, 1.0];
const ORANGE: [f32; 4] = [0.95, 0.67, 0.38, 1.0];
const PURPLE: [f32; 4] = [0.78, 0.62, 0.95, 1.0];
const BLUE: [f32; 4] = [0.45, 0.72, 0.98, 1.0];
const YELLOW: [f32; 4] = [0.92, 0.84, 0.48, 1.0];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RectInstance {
    // Rectangles store x/y/width/height. Lines store x1/y1/x2/y2 and encode
    // their width in `flags`; the vertex shader expands both into a quad.
    geometry: [f32; 4],
    color: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct View {
    width: f32,
    height: f32,
    scroll: f32,
    math_scroll: f32,
}

#[derive(Clone, Copy)]
struct BlockLayout {
    y: f64,
    height: f64,
    born_at: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TextPos {
    block: u32,
    offset: u32,
}

impl TextPos {
    fn after(self) -> Self {
        Self {
            block: self.block,
            offset: self.offset.saturating_add(1),
        }
    }
}

#[derive(Clone)]
struct TextCell {
    pos: TextPos,
    text: CellText,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    scroll_x: bool,
}

#[derive(Clone)]
enum CellText {
    Char(char),
    Span(String),
}

impl CellText {
    fn first_char(&self) -> Option<char> {
        match self {
            Self::Char(c) => Some(*c),
            Self::Span(text) => text.chars().next(),
        }
    }

    fn push_to(&self, output: &mut String) {
        match self {
            Self::Char(c) => output.push(*c),
            Self::Span(text) => output.push_str(text),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SelectionGranularity {
    Character,
    Word,
    Block,
}

struct App {
    canvas: HtmlCanvasElement,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    view_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    instance_count: u32,
    instance_staging: Vec<RectInstance>,
    parser: Parser,
    tokens: Vec<String>,
    next_token: usize,
    current_repeat: usize,
    repeats: usize,
    tps: f64,
    font_scale: f32,
    fade_ms: f64,
    fade_until: f64,
    emitted_tokens: usize,
    layouts: Vec<BlockLayout>,
    auto_scroll: bool,
    paused: bool,
    dragging_scrollbar: bool,
    selecting_text: bool,
    selection_anchor: Option<TextPos>,
    selection_focus: Option<TextPos>,
    selection_granularity: SelectionGranularity,
    selection_initial_unit: Option<(TextPos, TextPos)>,
    text_cells: Vec<TextCell>,
    token_credit: f64,
    last_time: f64,
    scroll: f64,
    scroll_target: f64,
    math_scroll: f32,
    math_scroll_target: f32,
    math_content_width: f32,
    content_height: f64,
    dirty_scene: bool,
    needs_present: bool,
    last_scene_time: f64,
    scene_scroll_anchor: f64,
    fps: f64,
    backend: RendererBackend,
    backend_name: String,
    search_index: SearchTrie<TextPos>,
    search_query: String,
    search_matches: Vec<(TextPos, TextPos)>,
    search_active: usize,
    search_dirty: bool,
    search_indexed_at: f64,
}

#[wasm_bindgen(start)]
pub fn start() {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&info.to_string().into())
    }));
    wasm_bindgen_futures::spawn_local(async {
        if let Err(error) = run().await {
            web_sys::console::error_1(&error);
        }
    });
}

async fn run() -> Result<(), JsValue> {
    let window = web_sys::window().ok_or("window unavailable")?;
    let document = window.document().ok_or("document unavailable")?;
    if let Some(accessible_content) = document.get_element_by_id("accessible-content") {
        accessible_content.set_text_content(Some(selected_mock()));
    }
    let canvas: HtmlCanvasElement = window
        .document()
        .ok_or("document unavailable")?
        .get_element_by_id("app")
        .ok_or("#app canvas missing")?
        .dyn_into()?;
    let search = window.location().search().unwrap_or_default();
    let preference = RendererPreference::from_search(&search);
    let chain = preference.fallback_chain();
    let mut canvas = canvas;
    let mut gpu_app = None;

    for (fallback_depth, backend) in chain.iter().copied().enumerate() {
        set_renderer_metadata(&canvas, preference, backend, fallback_depth);
        match backend {
            RendererBackend::Canvas2d => {
                let (tps, repeats, auto_scroll, font_size, fade_ms) = runtime_config();
                return canvas2d::start(
                    canvas,
                    selected_mock(),
                    tps,
                    repeats,
                    auto_scroll,
                    font_size,
                    fade_ms,
                );
            }
            RendererBackend::WebGpu | RendererBackend::WebGl2 => {
                match init_gpu_with_timeout(canvas.clone(), backend).await {
                    Ok(app) => {
                        gpu_app = Some(app);
                        break;
                    }
                    Err(error) => {
                        let next = chain.get(fallback_depth + 1).copied();
                        web_sys::console::warn_1(
                            &format!(
                                "{} unavailable: {error:?}; {}",
                                backend.display_name(),
                                next.map_or("no renderer fallback remains".to_owned(), |next| {
                                    format!("trying {}", next.display_name())
                                })
                            )
                            .into(),
                        );
                        if next.is_some() {
                            canvas = replace_canvas(&canvas)?;
                        }
                    }
                }
            }
        }
    }

    let app =
        Rc::new(RefCell::new(gpu_app.ok_or_else(|| {
            JsValue::from_str("no compatible renderer available")
        })?));

    let wheel_app = app.clone();
    let wheel = Closure::<dyn FnMut(WheelEvent)>::new(move |event: WheelEvent| {
        event.prevent_default();
        let mut app = wheel_app.borrow_mut();
        if event.shift_key() {
            app.math_scroll_target = (app.math_scroll_target + event.delta_y() as f32).max(0.0);
            app.auto_scroll = false;
            return;
        }
        if event.delta_y() < 0.0 {
            app.auto_scroll = false;
        }
        app.scroll_target = (app.scroll_target + event.delta_y()).max(0.0);
        let max_scroll = (app.content_height - app.config.height as f64 + 48.0).max(0.0);
        if event.delta_y() > 0.0 && app.scroll_target >= max_scroll - 24.0 {
            app.auto_scroll = true;
        }
        app.sync_control_state();
    });
    canvas.add_event_listener_with_callback("wheel", wheel.as_ref().unchecked_ref())?;
    wheel.forget();

    let down_app = app.clone();
    let down = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
        let mut app = down_app.borrow_mut();
        if app.pointer_on_scrollbar(&event) {
            app.dragging_scrollbar = true;
            let _ = app.canvas.set_attribute("data-cursor", "grabbing");
            app.update_scrollbar(event.client_y() as f32);
        } else if event.button() == 0 {
            app.begin_text_selection(&event);
        }
    });
    canvas.add_event_listener_with_callback("mousedown", down.as_ref().unchecked_ref())?;
    down.forget();

    let move_app = app.clone();
    let moved = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
        let mut app = move_app.borrow_mut();
        if app.dragging_scrollbar {
            app.update_scrollbar(event.client_y() as f32);
        } else if app.selecting_text {
            app.update_text_selection(&event);
        }
        app.update_cursor(&event);
    });
    window.add_event_listener_with_callback("mousemove", moved.as_ref().unchecked_ref())?;
    moved.forget();

    let up_app = app.clone();
    let up = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
        let mut app = up_app.borrow_mut();
        app.dragging_scrollbar = false;
        app.selecting_text = false;
        app.update_cursor(&event);
    });
    window.add_event_listener_with_callback("mouseup", up.as_ref().unchecked_ref())?;
    up.forget();

    let key_app = app.clone();
    let key = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
        if (event.ctrl_key() || event.meta_key()) && event.key().eq_ignore_ascii_case("c") {
            if key_app.borrow_mut().copy_selection() {
                event.prevent_default();
            }
        } else if event.key() == "Escape" {
            let mut app = key_app.borrow_mut();
            app.selection_anchor = None;
            app.selection_focus = None;
            app.dirty_scene = true;
        } else if event
            .target()
            .and_then(|target| target.dyn_into::<HtmlCanvasElement>().ok())
            .is_some()
        {
            let mut app = key_app.borrow_mut();
            let viewport = app.config.height as f64;
            let max_scroll = (app.content_height - viewport + 48.0).max(0.0);
            let key = event.key();
            if matches!(key.as_str(), "F3" | "F3Previous") {
                event.prevent_default();
                app.navigate_search(key == "F3Previous" || event.shift_key());
            } else if key.eq_ignore_ascii_case("p") {
                event.prevent_default();
                app.paused = !app.paused;
                app.dirty_scene = true;
            } else if key.eq_ignore_ascii_case("a") {
                event.prevent_default();
                app.auto_scroll = !app.auto_scroll;
                if app.auto_scroll {
                    app.scroll_target = max_scroll;
                }
                app.dirty_scene = true;
            } else if matches!(key.as_str(), "+" | "=") {
                event.prevent_default();
                app.font_scale = (app.font_scale + 0.125).min(2.5);
                app.reflow_from(0);
                app.dirty_scene = true;
            } else if matches!(key.as_str(), "-" | "_") {
                event.prevent_default();
                app.font_scale = (app.font_scale - 0.125).max(0.625);
                app.reflow_from(0);
                app.dirty_scene = true;
            } else {
                let target = match key.as_str() {
                    "ArrowUp" => Some(app.scroll_target - 48.0),
                    "ArrowDown" => Some(app.scroll_target + 48.0),
                    "PageUp" => Some(app.scroll_target - viewport * 0.85),
                    "PageDown" => Some(app.scroll_target + viewport * 0.85),
                    " " if event.shift_key() => Some(app.scroll_target - viewport * 0.85),
                    " " => Some(app.scroll_target + viewport * 0.85),
                    "Home" => Some(0.0),
                    "End" => Some(max_scroll),
                    _ => None,
                };
                if let Some(target) = target {
                    event.prevent_default();
                    app.auto_scroll = key == "End";
                    app.scroll_target = target.clamp(0.0, max_scroll);
                    app.needs_present = true;
                }
            }
            app.sync_control_state();
        }
    });
    window.add_event_listener_with_callback("keydown", key.as_ref().unchecked_ref())?;
    key.forget();

    let callback: AnimationLoop = Rc::new(RefCell::new(None));
    let callback_copy = callback.clone();
    let frame_app = app.clone();
    *callback.borrow_mut() = Some(Closure::new(move |time: f64| {
        frame_app.borrow_mut().frame(time);
        if let Some(cb) = callback_copy.borrow().as_ref() {
            let _ = web_sys::window()
                .unwrap()
                .request_animation_frame(cb.as_ref().unchecked_ref());
        }
    }));
    window.request_animation_frame(callback.borrow().as_ref().unwrap().as_ref().unchecked_ref())?;
    Ok(())
}

async fn timeout_after(milliseconds: i32) {
    let promise = Promise::new(&mut |resolve, _reject| {
        if let Some(window) = web_sys::window() {
            let _ = window
                .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, milliseconds);
        } else {
            let _ = resolve.call0(&JsValue::NULL);
        }
    });
    let _ = JsFuture::from(promise).await;
}

async fn init_gpu_with_timeout(
    canvas: HtmlCanvasElement,
    backend: RendererBackend,
) -> Result<App, JsValue> {
    let init = App::new(canvas, backend).fuse();
    let timeout = timeout_after(GPU_INIT_TIMEOUT_MS).fuse();
    futures_util::pin_mut!(init, timeout);
    match select(init, timeout).await {
        Either::Left((result, _)) => result,
        Either::Right(((), _)) => Err(JsValue::from_str(&format!(
            "{} initialization timed out after {} ms",
            backend.display_name(),
            GPU_INIT_TIMEOUT_MS,
        ))),
    }
}

impl App {
    async fn new(canvas: HtmlCanvasElement, backend: RendererBackend) -> Result<Self, JsValue> {
        resize_canvas(&canvas);
        let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_descriptor.backends = match backend {
            RendererBackend::WebGpu => wgpu::Backends::BROWSER_WEBGPU,
            RendererBackend::WebGl2 => wgpu::Backends::GL,
            RendererBackend::Canvas2d => {
                return Err(JsValue::from_str("Canvas2D is not a wgpu backend"));
            }
        };
        let instance = match backend {
            RendererBackend::WebGpu => {
                wgpu::util::new_instance_with_webgpu_detection(instance_descriptor).await
            }
            RendererBackend::WebGl2 => wgpu::Instance::new(instance_descriptor),
            RendererBackend::Canvas2d => unreachable!(),
        };
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let adapter_backend = format!("{:?}", adapter.get_info().backend).to_uppercase();
        let backend_name = backend.display_name().to_owned();
        let _ = canvas.set_attribute("data-renderer", backend.as_str());
        let _ = canvas.set_attribute("data-renderer-api", &adapter_backend);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("streamdown device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                ..Default::default()
            })
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|x| x.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| JsValue::from_str("renderer surface exposes no texture formats"))?;
        let alpha_mode = caps
            .alpha_modes
            .first()
            .copied()
            .ok_or_else(|| JsValue::from_str("renderer surface exposes no alpha modes"))?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: canvas.width().max(1),
            height: canvas.height().max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("streamdown shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let view_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("view uniform"),
            contents: bytemuck::bytes_of(&View {
                width: config.width as f32,
                height: config.height as f32,
                scroll: 0.0,
                math_scroll: 0.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("view layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("view bind group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: view_buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Unorm8x4, 2 => Uint32];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rect pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: size_of::<RectInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let instance_capacity = 1024;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene instances"),
            size: (instance_capacity * size_of::<RectInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tokens = tokenize(selected_mock());
        let (tps, repeats, auto_scroll, font_size, fade_ms) = runtime_config();
        let _ = canvas.set_attribute("data-paused", "false");
        let _ = canvas.set_attribute("data-auto-scroll", &auto_scroll.to_string());
        let _ = canvas.set_attribute("data-font-size", &format!("{font_size:.0}"));
        let _ = canvas.set_attribute("data-search-count", "0");
        let _ = canvas.set_attribute("data-search-active", "0");
        Ok(Self {
            canvas,
            surface,
            device,
            queue,
            config,
            pipeline,
            bind_group,
            view_buffer,
            instance_buffer,
            instance_capacity,
            instance_count: 0,
            instance_staging: Vec::with_capacity(instance_capacity),
            parser: Parser::new(),
            tokens,
            next_token: 0,
            current_repeat: 0,
            repeats,
            tps,
            font_scale: font_size / DEFAULT_FONT_SIZE,
            fade_ms,
            fade_until: 0.0,
            emitted_tokens: 0,
            layouts: Vec::new(),
            auto_scroll,
            paused: false,
            dragging_scrollbar: false,
            selecting_text: false,
            selection_anchor: None,
            selection_focus: None,
            selection_granularity: SelectionGranularity::Character,
            selection_initial_unit: None,
            text_cells: Vec::new(),
            token_credit: 1.0,
            last_time: 0.0,
            scroll: 0.0,
            scroll_target: 0.0,
            math_scroll: 0.0,
            math_scroll_target: 0.0,
            math_content_width: 0.0,
            content_height: 0.0,
            dirty_scene: true,
            needs_present: true,
            last_scene_time: 0.0,
            scene_scroll_anchor: 0.0,
            fps: 60.0,
            backend,
            backend_name,
            search_index: SearchTrie::default(),
            search_query: String::new(),
            search_matches: Vec::new(),
            search_active: 0,
            search_dirty: false,
            search_indexed_at: 0.0,
        })
    }

    fn frame(&mut self, now: f64) {
        let dt = if self.last_time == 0.0 {
            0.0
        } else {
            ((now - self.last_time) / 1000.0).min(0.1)
        };
        self.last_time = now;
        self.sync_search(now);
        if dt > 0.0 {
            self.fps = self.fps * 0.92 + (1.0 / dt) * 0.08;
        }
        if !self.paused && self.current_repeat < self.repeats {
            self.token_credit += dt * self.tps;
            let due = (self.token_credit.floor() as usize).min(MAX_TOKENS_PER_FRAME);
            if due != 0 {
                let emitted = self.emit_tokens(due);
                self.token_credit -= emitted as f64;
            }
        }
        let resized = resize_canvas(&self.canvas);
        if resized
            || self.config.width != self.canvas.width()
            || self.config.height != self.canvas.height()
        {
            self.config.width = self.canvas.width().max(1);
            self.config.height = self.canvas.height().max(1);
            self.surface.configure(&self.device, &self.config);
            self.reflow_from(0);
            self.dirty_scene = true;
        }
        let max_scroll = (self.content_height - self.config.height as f64 + 48.0).max(0.0);
        if self.auto_scroll {
            self.scroll_target = max_scroll;
        } else {
            self.scroll_target = self.scroll_target.clamp(0.0, max_scroll);
        }
        let previous_scroll = self.scroll;
        self.scroll += (self.scroll_target - self.scroll) * (1.0 - (-18.0 * dt).exp());
        if (self.scroll - previous_scroll).abs() > 0.01 {
            self.needs_present = true;
        }
        // The scene already contains a 160px overscan. Move it with the view
        // uniform and only rebuild after consuming most of that margin.
        if (self.scroll - self.scene_scroll_anchor).abs() > 112.0 {
            self.dirty_scene = true;
        }
        let math_max = (self.math_content_width - self.config.width as f32 + 48.0).max(0.0);
        self.math_scroll_target = self.math_scroll_target.clamp(0.0, math_max);
        let previous_math_scroll = self.math_scroll;
        self.math_scroll +=
            (self.math_scroll_target - self.math_scroll) * (1.0 - (-18.0 * dt as f32).exp());
        if (self.math_scroll - previous_math_scroll).abs() > 0.01 {
            self.needs_present = true;
        }
        if now < self.fade_until {
            self.dirty_scene = true;
        }
        let policy = self.backend.policy();
        let scene_throttled = policy.scene_rebuild_interval_ms > 0.0
            && self.instance_count > 0
            && now - self.last_scene_time < policy.scene_rebuild_interval_ms;
        if self.dirty_scene && !scene_throttled {
            self.rebuild_scene();
            self.last_scene_time = now;
        }
        self.draw();
    }

    fn scrollbar_geometry(&self) -> (f32, f32, f32, f64) {
        let track_top = 76.0;
        let track_height = (self.config.height as f32 - 88.0).max(20.0);
        let max_scroll = (self.content_height - self.config.height as f64 + 48.0).max(0.0);
        let thumb_height = if max_scroll <= 0.0 {
            track_height
        } else {
            (track_height as f64 * self.config.height as f64 / self.content_height)
                .clamp(28.0, track_height as f64) as f32
        };
        (track_top, track_height, thumb_height, max_scroll)
    }

    fn pointer_on_scrollbar(&self, event: &MouseEvent) -> bool {
        let rect = self.canvas.get_bounding_client_rect();
        let x = event.client_x() as f32 - rect.left() as f32;
        let y = event.client_y() as f32 - rect.top() as f32;
        let (top, height, _, max_scroll) = self.scrollbar_geometry();
        max_scroll > 0.0 && x >= self.content_width() - 28.0 && y >= top && y <= top + height
    }

    fn update_scrollbar(&mut self, client_y: f32) {
        let rect = self.canvas.get_bounding_client_rect();
        let y = client_y - rect.top() as f32;
        let (top, track_height, thumb_height, max_scroll) = self.scrollbar_geometry();
        if max_scroll <= 0.0 || track_height <= thumb_height {
            return;
        }
        let ratio =
            ((y - top - thumb_height * 0.5) / (track_height - thumb_height)).clamp(0.0, 1.0);
        self.auto_scroll = false;
        self.scroll_target = ratio as f64 * max_scroll;
        self.sync_control_state();
    }

    fn sync_control_state(&self) {
        let _ = self
            .canvas
            .set_attribute("data-paused", &self.paused.to_string());
        let _ = self
            .canvas
            .set_attribute("data-auto-scroll", &self.auto_scroll.to_string());
        let _ = self.canvas.set_attribute(
            "data-font-size",
            &format!("{:.0}", self.font_scale * DEFAULT_FONT_SIZE),
        );
    }

    fn sync_search(&mut self, now: f64) {
        let query = self
            .canvas
            .get_attribute("data-search-query")
            .unwrap_or_default();
        let changed = query != self.search_query;
        if changed {
            self.search_query = query;
            self.search_active = 0;
        }
        if self.search_query.trim().is_empty() {
            if !self.search_matches.is_empty() || changed {
                self.search_matches.clear();
                self.sync_search_state();
                self.dirty_scene = true;
            }
            return;
        }
        if self.search_dirty && (changed || now - self.search_indexed_at >= 200.0) {
            self.rebuild_search_index();
            self.search_indexed_at = now;
            self.search_dirty = false;
            self.dirty_scene = true;
        } else if changed {
            self.update_search_matches();
            self.dirty_scene = true;
        }
    }

    fn rebuild_search_index(&mut self) {
        self.search_index.clear();
        for (block, node) in self.parser.blocks().iter().enumerate() {
            index_block(&mut self.search_index, block as u32, node);
        }
        self.update_search_matches();
    }

    fn update_search_matches(&mut self) {
        let length = self.search_query.trim().chars().count() as u32;
        self.search_matches = self
            .search_index
            .search(&self.search_query)
            .iter()
            .copied()
            .map(|start| {
                (
                    start,
                    TextPos {
                        block: start.block,
                        offset: start.offset.saturating_add(length),
                    },
                )
            })
            .collect();
        if self.search_active >= self.search_matches.len() {
            self.search_active = 0;
        }
        self.sync_search_state();
    }

    fn navigate_search(&mut self, backwards: bool) {
        if self.search_matches.is_empty() {
            return;
        }
        self.search_active = if backwards {
            self.search_active
                .checked_sub(1)
                .unwrap_or(self.search_matches.len() - 1)
        } else {
            (self.search_active + 1) % self.search_matches.len()
        };
        let position = self.search_matches[self.search_active].0;
        let block = position.block as usize;
        if let Some(layout) = self.layouts.get(block) {
            let max_scroll = (self.content_height - self.config.height as f64 + 48.0).max(0.0);
            let mut target_y = layout.y;
            if let Some(Block::CodeBlock { text, .. }) = self.parser.blocks().get(block) {
                let mut offset = 0_u32;
                let mut lines = 0_u32;
                for character in text.chars() {
                    if character == '\n' {
                        lines += 1;
                    } else {
                        if offset >= position.offset {
                            break;
                        }
                        offset += 1;
                    }
                }
                target_y += (22.0 + lines as f32 * 18.0) as f64 * self.font_scale as f64;
            }
            self.scroll_target = (target_y - 92.0).clamp(0.0, max_scroll);
            self.auto_scroll = false;
        }
        self.sync_control_state();
        self.sync_search_state();
        self.dirty_scene = true;
    }

    fn sync_search_state(&self) {
        let _ = self
            .canvas
            .set_attribute("data-search-count", &self.search_matches.len().to_string());
        let active = if self.search_matches.is_empty() {
            0
        } else {
            self.search_active + 1
        };
        let _ = self
            .canvas
            .set_attribute("data-search-active", &active.to_string());
    }

    fn update_cursor(&self, event: &MouseEvent) {
        let rect = self.canvas.get_bounding_client_rect();
        let x = event.client_x() as f32 - rect.left() as f32;
        let y = event.client_y() as f32 - rect.top() as f32;
        let inside = x >= 0.0 && y >= 0.0 && x <= rect.width() as f32 && y <= rect.height() as f32;
        let cursor = if !inside {
            "default"
        } else if self.dragging_scrollbar {
            "grabbing"
        } else {
            if self.pointer_on_scrollbar(event) {
                "grab"
            } else if self.pointer_in_document(event) {
                "text"
            } else {
                "default"
            }
        };
        let _ = self.canvas.set_attribute("data-cursor", cursor);
    }

    fn copy_selection(&mut self) -> bool {
        let text = self.selected_text();
        if text.is_empty() {
            return false;
        }
        let _ = web_sys::window().map(|window| window.navigator().clipboard().write_text(&text));
        true
    }

    fn pointer_canvas_position(&self, event: &MouseEvent) -> (f32, f32) {
        let rect = self.canvas.get_bounding_client_rect();
        (
            event.client_x() as f32 - rect.left() as f32,
            event.client_y() as f32 - rect.top() as f32,
        )
    }

    fn text_cell_screen_geometry(&self, cell: &TextCell) -> (f32, f32, f32, f32) {
        (
            cell.x - if cell.scroll_x { self.math_scroll } else { 0.0 },
            cell.y - (self.scroll - self.scene_scroll_anchor) as f32,
            cell.width,
            cell.height,
        )
    }

    fn pointer_in_document(&self, event: &MouseEvent) -> bool {
        let (x, y) = self.pointer_canvas_position(event);
        y >= 68.0 && y <= self.config.height as f32 && x >= 0.0 && x < self.content_width()
    }

    /// Hit-tests like a browser text caret: choose a visual line from the
    /// vertical coordinate first, then choose the character boundary from the
    /// horizontal coordinate. This keeps clicks in a line's trailing whitespace
    /// at that line's end instead of jumping to text on an adjacent line.
    fn nearest_text_hit(&self, x: f32, y: f32) -> Option<(TextPos, usize)> {
        let vertical_distance = |cell: &TextCell| {
            let (_, cell_y, _, height) = self.text_cell_screen_geometry(cell);
            if y < cell_y {
                cell_y - y
            } else if y > cell_y + height {
                y - cell_y - height
            } else {
                0.0
            }
        };
        let line_seed = self
            .text_cells
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| vertical_distance(a).total_cmp(&vertical_distance(b)))?;
        let seed = line_seed.1;
        let overlaps_seed = |cell: &TextCell| {
            let (_, cell_y, _, height) = self.text_cell_screen_geometry(cell);
            let (_, seed_y, _, seed_height) = self.text_cell_screen_geometry(seed);
            cell.pos.block == seed.pos.block
                && cell_y < seed_y + seed_height
                && seed_y < cell_y + height
        };
        let mut line: Vec<(usize, &TextCell)> = self
            .text_cells
            .iter()
            .enumerate()
            .filter(|(_, cell)| overlaps_seed(cell))
            .collect();
        line.sort_by(|(_, a), (_, b)| {
            let (a_x, _, _, _) = self.text_cell_screen_geometry(a);
            let (b_x, _, _, _) = self.text_cell_screen_geometry(b);
            a_x.total_cmp(&b_x)
        });

        let nearest_index = line
            .iter()
            .min_by(|(_, a), (_, b)| {
                let horizontal_distance = |cell: &TextCell| {
                    let (cell_x, _, width, _) = self.text_cell_screen_geometry(cell);
                    if x < cell_x {
                        cell_x - x
                    } else if x > cell_x + width {
                        x - cell_x - width
                    } else {
                        0.0
                    }
                };
                horizontal_distance(a).total_cmp(&horizontal_distance(b))
            })?
            .0;
        let pos = line
            .iter()
            .find(|(_, cell)| {
                let (cell_x, _, width, _) = self.text_cell_screen_geometry(cell);
                x < cell_x + width * 0.5
            })
            .map_or_else(
                || {
                    line.last()
                        .expect("a visual line is non-empty")
                        .1
                        .pos
                        .after()
                },
                |(_, cell)| cell.pos,
            );
        Some((pos, nearest_index))
    }

    fn begin_text_selection(&mut self, event: &MouseEvent) {
        if !self.pointer_in_document(event) {
            return;
        }
        let (x, y) = self.pointer_canvas_position(event);
        let Some((pos, index)) = self.nearest_text_hit(x, y) else {
            return;
        };
        self.auto_scroll = false;
        self.sync_control_state();
        self.selecting_text = true;
        if event.shift_key() && self.selection_anchor.is_some() {
            self.selection_granularity = SelectionGranularity::Character;
            self.selection_initial_unit = None;
            self.selection_focus = Some(pos);
            self.dirty_scene = true;
            return;
        }
        if event.detail() >= 3 {
            let unit = self.selection_unit_at(index, SelectionGranularity::Block);
            self.selection_granularity = SelectionGranularity::Block;
            self.selection_initial_unit = Some(unit);
            self.selection_anchor = Some(unit.0);
            self.selection_focus = Some(unit.1);
            self.dirty_scene = true;
            return;
        }
        if event.detail() == 2 {
            let unit = self.selection_unit_at(index, SelectionGranularity::Word);
            self.selection_granularity = SelectionGranularity::Word;
            self.selection_initial_unit = Some(unit);
            self.selection_anchor = Some(unit.0);
            self.selection_focus = Some(unit.1);
            self.dirty_scene = true;
            return;
        }
        self.selection_granularity = SelectionGranularity::Character;
        self.selection_initial_unit = None;
        self.selection_anchor = Some(pos);
        self.selection_focus = Some(pos);
        self.dirty_scene = true;
    }

    fn selection_unit_at(
        &self,
        index: usize,
        granularity: SelectionGranularity,
    ) -> (TextPos, TextPos) {
        let block = self.text_cells[index].pos.block;
        let mut start = index;
        let mut end = index;
        match granularity {
            SelectionGranularity::Character => {}
            SelectionGranularity::Word => {
                let is_word = |cell: &TextCell| {
                    cell.text
                        .first_char()
                        .is_some_and(|c| c.is_alphanumeric() || c == '_')
                };
                while is_word(&self.text_cells[index])
                    && start > 0
                    && self.text_cells[start - 1].pos.block == block
                    && is_word(&self.text_cells[start - 1])
                {
                    start -= 1;
                }
                while is_word(&self.text_cells[index])
                    && end + 1 < self.text_cells.len()
                    && self.text_cells[end + 1].pos.block == block
                    && is_word(&self.text_cells[end + 1])
                {
                    end += 1;
                }
            }
            SelectionGranularity::Block => {
                while start > 0 && self.text_cells[start - 1].pos.block == block {
                    start -= 1;
                }
                while end + 1 < self.text_cells.len() && self.text_cells[end + 1].pos.block == block
                {
                    end += 1;
                }
            }
        }
        (self.text_cells[start].pos, self.text_cells[end].pos.after())
    }

    fn update_text_selection(&mut self, event: &MouseEvent) {
        let (x, y) = self.pointer_canvas_position(event);
        let rect = self.canvas.get_bounding_client_rect();
        let viewport_y = event.client_y() as f32 - rect.top() as f32;
        if viewport_y < 92.0 {
            self.scroll_target = (self.scroll_target - 24.0).max(0.0);
        } else if viewport_y > self.config.height as f32 - 54.0 {
            let max_scroll = (self.content_height - self.config.height as f64 + 48.0).max(0.0);
            self.scroll_target = (self.scroll_target + 24.0).min(max_scroll);
        }
        if let Some((pos, index)) = self.nearest_text_hit(x, y) {
            let (anchor, focus) = if let Some(initial) = self.selection_initial_unit {
                let unit = self.selection_unit_at(index, self.selection_granularity);
                if unit.1 <= initial.0 {
                    (initial.1, unit.0)
                } else if unit.0 >= initial.1 {
                    (initial.0, unit.1)
                } else {
                    initial
                }
            } else {
                (self.selection_anchor.unwrap_or(pos), pos)
            };
            if self.selection_anchor != Some(anchor) || self.selection_focus != Some(focus) {
                self.selection_anchor = Some(anchor);
                self.selection_focus = Some(focus);
                self.dirty_scene = true;
            }
        }
    }

    fn selection_range(&self) -> Option<(TextPos, TextPos)> {
        let a = self.selection_anchor?;
        let b = self.selection_focus?;
        if a == b {
            None
        } else {
            Some(if a < b { (a, b) } else { (b, a) })
        }
    }

    fn selected_text(&self) -> String {
        let Some((start, end)) = self.selection_range() else {
            return String::new();
        };
        let mut cells: Vec<&TextCell> = self
            .text_cells
            .iter()
            .filter(|cell| cell.pos >= start && cell.pos < end)
            .collect();
        cells.sort_by_key(|cell| cell.pos);
        let mut out = String::new();
        let mut previous: Option<&TextCell> = None;
        for cell in cells {
            if let Some(last) = previous
                && (cell.pos.block != last.pos.block || cell.y > last.y + last.height * 0.65)
            {
                out.push('\n');
            }
            cell.text.push_to(&mut out);
            previous = Some(cell);
        }
        out
    }

    fn emit_tokens(&mut self, count: usize) -> usize {
        let mut chunk = String::with_capacity(count.saturating_mul(7));
        let mut emitted = 0;
        while emitted < count && self.current_repeat < self.repeats {
            chunk.push_str(&self.tokens[self.next_token]);
            self.next_token += 1;
            emitted += 1;
            if self.next_token == self.tokens.len() {
                self.next_token = 0;
                self.current_repeat += 1;
                if self.current_repeat < self.repeats {
                    chunk.push_str("\n\n");
                }
            }
        }
        if !chunk.is_empty() {
            let delta = self.parser.append(&chunk);
            self.sync_layout(&delta);
            self.emitted_tokens += emitted;
            self.search_dirty = true;
            self.dirty_scene = true;
        }
        emitted
    }

    fn sync_layout(&mut self, delta: &Delta) {
        let changed = delta
            .ops
            .iter()
            .map(|op| match op {
                Op::Truncate { from } => *from as usize,
                Op::SpliceCode { block, .. }
                | Op::SealCode { block }
                | Op::AppendText { block, .. }
                | Op::AppendInlineText { block, .. } => *block as usize,
                Op::Push(_) => self.layouts.len(),
            })
            .min()
            .unwrap_or(self.layouts.len());
        self.reflow_from(changed.min(self.parser.blocks().len()));
    }

    fn content_width(&self) -> f32 {
        self.config.width as f32
    }

    fn reflow_from(&mut self, from: usize) {
        let old_births: Vec<f64> = self.layouts[from..]
            .iter()
            .map(|layout| layout.born_at)
            .collect();
        self.layouts.truncate(from);
        let mut y = self
            .layouts
            .last()
            .map_or(104.0, |last| last.y + last.height);
        for (offset, block) in self.parser.blocks()[from..].iter().enumerate() {
            let height = measure_block(block, self.content_width(), self.font_scale) as f64;
            let born_at = old_births.get(offset).copied().unwrap_or(self.last_time);
            if offset >= old_births.len() && self.fade_ms > 0.0 {
                self.fade_until = self.fade_until.max(born_at + self.fade_ms);
            }
            self.layouts.push(BlockLayout { y, height, born_at });
            y += height;
        }
        self.content_height = y + 60.0;
    }

    fn rebuild_scene(&mut self) {
        // Keep all GPU coordinates close to zero. Absolute document positions can
        // grow beyond the precision range of f32 in stress documents.
        let scene_origin = self.scroll;
        self.scene_scroll_anchor = scene_origin;
        let content_width = self.content_width();
        let instances = std::mem::take(&mut self.instance_staging);
        let text_cells = std::mem::take(&mut self.text_cells);
        let mut scene = Scene::reuse(content_width, instances, text_cells);
        scene.glyph_coverage_quantum = self.backend.policy().glyph_coverage_quantum;
        scene.clip_top = -160.0;
        scene.clip_bottom = self.config.height as f32 + 160.0;
        let visible_top = (self.scroll - 160.0).max(0.0);
        let visible_bottom = self.scroll + self.config.height as f64 + 160.0;
        let first = self
            .layouts
            .partition_point(|layout| layout.y + layout.height < visible_top);
        let last =
            first + self.layouts[first..].partition_point(|layout| layout.y <= visible_bottom);
        for index in first..last {
            let block = &self.parser.blocks()[index];
            let layout = self.layouts[index];
            scene.y = (layout.y - scene_origin) as f32;
            let fade = if self.fade_ms <= 0.0 {
                1.0
            } else {
                let t = ((self.last_time - layout.born_at) / self.fade_ms).clamp(0.0, 1.0) as f32;
                t * t * (3.0 - 2.0 * t)
            };
            scene.opacity = fade;
            scene.begin_block(index as u32);
            scene.block(block, self.font_scale, layout.height as f32);
        }
        scene.finish_block();
        // Selection is interaction chrome, not newly streamed document content.
        // Do not inherit the last rendered block's entrance opacity.
        scene.opacity = 1.0;
        if !self.search_matches.is_empty() {
            scene.search_overlay(&self.search_matches, self.search_active);
        }
        if let Some(range) = self.selection_range() {
            scene.selection_overlay(range);
        }
        // Fixed chrome is emitted last, so scrolling content cannot paint over it.
        scene.rect(
            0.0,
            0.0,
            self.config.width as f32,
            68.0,
            [0.055, 0.075, 0.105, 0.98],
            1.0,
        );
        let renderer_title = if self.config.width < 480 {
            "STREAMDOWN".to_owned()
        } else {
            format!("STREAMDOWN / {}", self.backend_name)
        };
        scene.text(&renderer_title, 24.0, 22.0, 1.0, CYAN, 1.0);
        let status = format!(
            "TOKENS {} | BLOCKS {} | {:.0} FPS | {:.0} TPS | X{} | {}",
            self.emitted_tokens,
            self.parser.blocks().len(),
            self.fps,
            self.tps,
            self.repeats,
            if self.paused {
                "PAUSED"
            } else if self.auto_scroll {
                "AUTO"
            } else {
                "FREE"
            },
        );
        if self.config.width >= 720 {
            let status_width: f32 = status.chars().map(|c| font::advance(c, 1.0)).sum();
            let sx = self.config.width as f32 - status_width - 24.0;
            scene.text(&status, sx, 25.0, 1.0, MUTED, 1.0);
        }
        let (track_top, track_height, thumb_height, max_scroll) = self.scrollbar_geometry();
        let thumb_y = if max_scroll > 0.0 {
            track_top
                + (track_height - thumb_height) * (self.scroll / max_scroll).clamp(0.0, 1.0) as f32
        } else {
            track_top
        };
        scene.scrollbar(
            self.content_width() - 14.0,
            track_top,
            track_height,
            thumb_y,
            thumb_height,
        );
        self.math_content_width = scene.math_width;
        self.text_cells = std::mem::take(&mut scene.text_cells);
        self.instance_count = scene.instances.len() as u32;
        if scene.instances.len() > self.instance_capacity {
            self.instance_capacity = scene.instances.len().next_power_of_two();
            self.instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("scene instances"),
                size: (self.instance_capacity * size_of::<RectInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !scene.instances.is_empty() {
            self.queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&scene.instances),
            );
        }
        self.instance_staging = std::mem::take(&mut scene.instances);
        self.dirty_scene = false;
        self.scene_scroll_anchor = scene_origin;
        self.needs_present = true;
    }

    fn draw(&mut self) {
        if !self.needs_present {
            return;
        }
        self.queue.write_buffer(
            &self.view_buffer,
            0,
            bytemuck::bytes_of(&View {
                width: self.config.width as f32,
                height: self.config.height as f32,
                scroll: (self.scroll - self.scene_scroll_anchor) as f32,
                math_scroll: self.math_scroll,
            }),
        );
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                self.dirty_scene = true;
                frame
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return,
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("canvas pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: BG[0] as f64,
                            g: BG[1] as f64,
                            b: BG[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
            pass.draw(0..4, 0..self.instance_count);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        self.needs_present = false;
    }
}

struct Scene {
    instances: Vec<RectInstance>,
    width: f32,
    y: f32,
    snap_text: bool,
    math_mode: bool,
    math_width: f32,
    opacity: f32,
    glyph_coverage_quantum: u8,
    text_cells: Vec<TextCell>,
    text_block: Option<u32>,
    text_offset: u32,
    clip_top: f32,
    clip_bottom: f32,
}
impl Scene {
    fn reuse(width: f32, mut instances: Vec<RectInstance>, mut text_cells: Vec<TextCell>) -> Self {
        instances.clear();
        text_cells.clear();
        Self {
            instances,
            width,
            y: 0.0,
            snap_text: false,
            math_mode: false,
            math_width: 0.0,
            opacity: 1.0,
            glyph_coverage_quantum: 1,
            text_cells,
            text_block: None,
            text_offset: 0,
            clip_top: f32::NEG_INFINITY,
            clip_bottom: f32::INFINITY,
        }
    }
    fn begin_block(&mut self, block: u32) {
        self.text_block = Some(block);
        self.text_offset = 0;
    }

    fn finish_block(&mut self) {
        self.text_block = None;
    }

    fn selection_overlay(&mut self, (start, end): (TextPos, TextPos)) {
        let selected: Vec<(f32, f32, f32, f32, bool)> = self
            .text_cells
            .iter()
            .filter(|cell| cell.pos >= start && cell.pos < end)
            .map(|cell| {
                (
                    cell.x,
                    cell.y,
                    cell.width.max(2.0),
                    cell.height,
                    cell.scroll_x,
                )
            })
            .collect();
        for (x, y, width, height, scroll_x) in selected {
            self.math_mode = scroll_x;
            self.rect(x, y, width, height, [0.20, 0.48, 0.78, 0.42], 0.0);
        }
        self.math_mode = false;
    }

    fn search_overlay(&mut self, matches: &[(TextPos, TextPos)], active: usize) {
        let cells: Vec<(f32, f32, f32, f32, bool, bool)> = self
            .text_cells
            .iter()
            .filter_map(|cell| {
                let index = matches.partition_point(|(_, end)| *end <= cell.pos);
                matches.get(index).and_then(|(start, end)| {
                    (cell.pos >= *start && cell.pos < *end).then_some((
                        cell.x,
                        cell.y,
                        cell.width.max(2.0),
                        cell.height,
                        cell.scroll_x,
                        index == active,
                    ))
                })
            })
            .collect();
        for (x, y, width, height, scroll_x, is_active) in cells {
            self.math_mode = scroll_x;
            self.rect(
                x,
                y,
                width,
                height,
                if is_active {
                    [0.98, 0.55, 0.20, 0.68]
                } else {
                    [0.92, 0.80, 0.22, 0.38]
                },
                0.0,
            );
        }
        self.math_mode = false;
    }

    fn record_text_cell(&mut self, c: char, x: f32, y: f32, width: f32, height: f32) {
        self.record_text_cell_value(
            CellText::Char(c),
            x,
            y - height * 0.2,
            width,
            height,
            self.math_mode,
        );
    }

    fn record_text_span(
        &mut self,
        text: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        scroll_x: bool,
    ) {
        self.record_text_cell_value(CellText::Span(text), x, y, width, height, scroll_x);
    }

    fn record_text_cell_value(
        &mut self,
        text: CellText,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        scroll_x: bool,
    ) {
        let Some(block) = self.text_block else {
            return;
        };
        self.text_cells.push(TextCell {
            pos: TextPos {
                block,
                offset: self.text_offset,
            },
            text,
            x,
            y,
            width,
            height,
            scroll_x,
        });
        self.text_offset += 1;
    }
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4], fixed: f32) {
        let mut color = color;
        color[3] *= self.opacity;
        let flags = u32::from(fixed > 0.5)
            | (u32::from(self.math_mode) << 1)
            | (u32::from(self.snap_text) << 2);
        self.instances.push(RectInstance {
            geometry: [x, y, w, h],
            color: pack_color(color),
            flags,
        });
    }
    fn text(
        &mut self,
        text: &str,
        mut x: f32,
        mut y: f32,
        scale: f32,
        color: [f32; 4],
        fixed: f32,
    ) -> f32 {
        let origin = x;
        let line = 18.0 * scale;
        let limit = self.width - 30.0;
        self.snap_text = true;
        for c in text.chars() {
            let glyph = font::glyph(c, scale);
            let advance = glyph.advance;
            if c == '\n' || (x + advance > limit && x > origin) {
                x = origin;
                y += line;
                if c == '\n' {
                    continue;
                }
            }
            self.glyph(c, glyph, x, y, scale, color, fixed);
            x += advance;
        }
        self.snap_text = false;
        y + 7.0 * scale
    }

    #[allow(clippy::too_many_arguments)]
    fn code_text(
        &mut self,
        source: &str,
        language: Option<&str>,
        mut x: f32,
        mut y: f32,
        scale: f32,
        fixed: f32,
    ) -> f32 {
        let origin = x;
        let line = 18.0 * scale;
        let limit = self.width - 30.0;
        let clip_top = self.clip_top;
        let clip_bottom = self.clip_bottom;
        let mut past_view = false;
        let mut advances = MonoAdvances::new(scale);
        self.snap_text = true;
        code::highlight(source, language, |span, kind| {
            if past_view {
                return false;
            }
            let color = match kind {
                code::TokenKind::Plain => FG,
                code::TokenKind::Keyword => PURPLE,
                code::TokenKind::Type => CYAN,
                code::TokenKind::Function => BLUE,
                code::TokenKind::String => GREEN,
                code::TokenKind::Number => ORANGE,
                code::TokenKind::Comment => MUTED,
                code::TokenKind::Macro => YELLOW,
                code::TokenKind::Operator => [0.68, 0.74, 0.82, 1.0],
            };
            for c in span.chars() {
                if y > clip_bottom {
                    past_view = true;
                    break;
                }
                let advance = advances.get(c);
                if c == '\n' || (x + advance > limit && x > origin) {
                    x = origin;
                    y += line;
                    if c == '\n' {
                        continue;
                    }
                }
                if y + line >= clip_top {
                    self.glyph(c, font::mono_glyph(c, scale), x, y, scale, color, fixed);
                } else {
                    self.text_offset = self.text_offset.saturating_add(1);
                }
                x += advance;
            }
            !past_view
        });
        self.snap_text = false;
        y + 7.0 * scale
    }

    #[allow(clippy::too_many_arguments)]
    fn glyph(
        &mut self,
        c: char,
        glyph: std::rc::Rc<font::GlyphBitmap>,
        x: f32,
        y: f32,
        scale: f32,
        color: [f32; 4],
        fixed: f32,
    ) {
        self.record_text_cell(c, x, y, glyph.advance, 18.0 * scale);
        self.draw_glyph(glyph, x, y, color, fixed);
    }

    fn draw_glyph(
        &mut self,
        glyph: std::rc::Rc<font::GlyphBitmap>,
        mut x: f32,
        mut y: f32,
        color: [f32; 4],
        fixed: f32,
    ) {
        // Snap the glyph origin once. Snapping every rectangle vertex in WGSL can
        // collapse thin antialiased coverage runs at fractional scroll positions.
        let snap = self.snap_text;
        if snap {
            x = x.round();
            y = y.round();
            self.snap_text = false;
        }
        for py in 0..glyph.height {
            let mut px = 0;
            while px < glyph.width {
                let coverage =
                    |value: u8| compat::quantize_coverage(value, self.glyph_coverage_quantum);
                let alpha = coverage(glyph.coverage[(py * glyph.width + px) as usize]);
                if alpha == 0 {
                    px += 1;
                    continue;
                }
                let mut run_end = px + 1;
                while run_end < glyph.width
                    && coverage(glyph.coverage[(py * glyph.width + run_end) as usize]) == alpha
                {
                    run_end += 1;
                }
                let mut shaded = color;
                shaded[3] *= alpha as f32 / 255.0;
                self.rect(
                    x + glyph.left as f32 + px as f32,
                    y + glyph.top as f32 + py as f32,
                    (run_end - px) as f32,
                    1.0,
                    shaded,
                    fixed,
                );
                px = run_end;
            }
        }
        self.snap_text = snap;
    }

    fn rich_text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        color: [f32; 4],
        fixed: f32,
    ) -> f32 {
        // Most table cells (and ordinary paragraphs) contain no math. Avoid the
        // delimiter scanner and use the compact glyph path directly.
        if !text.as_bytes().contains(&b'$') {
            return self.text(text, x, y, scale, color, fixed);
        }
        let mut cursor_x = x;
        let mut cursor_y = y;
        let mut line_extra = 0.0;
        let mut rest = text;

        while let Some(open) = rest.find('$') {
            self.inline_text_run(
                &rest[..open],
                x,
                &mut cursor_x,
                &mut cursor_y,
                &mut line_extra,
                scale,
                color,
                fixed,
            );
            let double = rest[open..].starts_with("$$");
            let delimiter_len = if double { 2 } else { 1 };
            let expression_start = open + delimiter_len;
            let delimiter = if double { "$$" } else { "$" };
            let Some(close_rel) = rest[expression_start..].find(delimiter) else {
                self.inline_text_run(
                    &rest[open..],
                    x,
                    &mut cursor_x,
                    &mut cursor_y,
                    &mut line_extra,
                    scale,
                    color,
                    fixed,
                );
                rest = "";
                break;
            };
            let close = expression_start + close_rel;
            let expression = &rest[expression_start..close];
            let image = match math::rasterize(expression, double, scale) {
                Ok(image) => image,
                Err(error) => {
                    web_sys::console::warn_1(&error.into());
                    self.inline_text_run(
                        expression,
                        x,
                        &mut cursor_x,
                        &mut cursor_y,
                        &mut line_extra,
                        scale,
                        color,
                        fixed,
                    );
                    rest = &rest[close + delimiter_len..];
                    continue;
                }
            };

            if double {
                if cursor_x > x {
                    cursor_y += 18.0 * scale + line_extra;
                }
                line_extra = 0.0;
                cursor_x = x;
                self.record_text_span(
                    format!("$${expression}$$"),
                    cursor_x,
                    cursor_y,
                    image.width as f32,
                    image.height as f32,
                    true,
                );
                self.math_image(&image, cursor_x, cursor_y, true);
                cursor_y += image.height as f32 + 6.0;
                cursor_x = x;
            } else {
                if cursor_x + image.width as f32 > self.width - 30.0 && cursor_x > x {
                    cursor_x = x;
                    cursor_y += 18.0 * scale + line_extra;
                    line_extra = 0.0;
                }
                let text_baseline = cursor_y + font::baseline(scale);
                let image_y = text_baseline - image.baseline as f32;
                self.record_text_span(
                    format!("${expression}$"),
                    cursor_x,
                    image_y,
                    image.width as f32,
                    image.height as f32,
                    false,
                );
                self.math_image(&image, cursor_x, image_y, false);
                let image_bottom = image_y + image.height as f32;
                line_extra = line_extra.max(image_bottom - cursor_y - 18.0 * scale);
                cursor_x += image.width as f32 + 2.0 * scale;
            }
            rest = &rest[close + delimiter_len..];
        }

        self.inline_text_run(
            rest,
            x,
            &mut cursor_x,
            &mut cursor_y,
            &mut line_extra,
            scale,
            color,
            fixed,
        );
        cursor_y + 7.0 * scale + line_extra
    }

    #[allow(clippy::too_many_arguments)]
    fn inline_text_run(
        &mut self,
        text: &str,
        origin: f32,
        cursor_x: &mut f32,
        cursor_y: &mut f32,
        line_extra: &mut f32,
        scale: f32,
        color: [f32; 4],
        fixed: f32,
    ) {
        let line = 18.0 * scale;
        let limit = self.width - 30.0;
        self.snap_text = true;
        for c in text.chars() {
            let glyph = font::glyph(c, scale);
            let advance = glyph.advance;
            if c == '\n' || (*cursor_x + advance > limit && *cursor_x > origin) {
                *cursor_x = origin;
                *cursor_y += line + *line_extra;
                *line_extra = 0.0;
                if c == '\n' {
                    continue;
                }
            }
            self.glyph(c, glyph, *cursor_x, *cursor_y, scale, color, fixed);
            *cursor_x += advance;
        }
        self.snap_text = false;
    }

    fn scrollbar(&mut self, x: f32, top: f32, track_height: f32, thumb_y: f32, thumb_height: f32) {
        self.rect(x, top, 8.0, track_height, [0.09, 0.12, 0.17, 0.9], 1.0);
        self.rect(x, thumb_y, 8.0, thumb_height, [0.32, 0.62, 0.68, 0.95], 1.0);
    }

    fn math_image(
        &mut self,
        image: &math::MathImage,
        x: f32,
        y: f32,
        horizontally_scrollable: bool,
    ) -> f32 {
        // Snap the image origin once; snapping every 1px coverage rectangle in
        // WGSL destroys the supersampled edge distribution during scrolling.
        let x = x.round();
        let y = y.round();
        self.snap_text = false;
        self.math_mode = horizontally_scrollable;
        for run in &image.runs {
            self.rect(
                x + run.x as f32,
                y + run.y as f32,
                run.width as f32,
                1.0,
                [
                    run.rgba[0] as f32 / 255.0,
                    run.rgba[1] as f32 / 255.0,
                    run.rgba[2] as f32 / 255.0,
                    run.rgba[3] as f32 / 255.0,
                ],
                0.0,
            );
        }
        self.math_mode = false;
        self.snap_text = false;
        if horizontally_scrollable {
            self.math_width = self.math_width.max(x + image.width as f32);
        }
        image.width as f32 + 4.0
    }

    fn block(&mut self, block: &Block, font_scale: f32, block_height: f32) {
        let x = content_gutter(self.width);
        match block {
            Block::Heading { level, content } => {
                let scale = match level {
                    1 => 2.0,
                    2 => 2.0,
                    _ => 1.0,
                } * font_scale;
                self.y = self.rich_text(&plain(content), x, self.y, scale, CYAN, 0.0)
                    + 22.0 * font_scale;
            }
            Block::Paragraph(content) => {
                self.y = self.rich_text(&plain(content), x, self.y, font_scale, FG, 0.0)
                    + 20.0 * font_scale
            }
            Block::BlockQuote(content) => {
                let top = self.y - 6.0;
                self.rect(x, top, 4.0, 42.0, CYAN, 0.0);
                self.y = self.rich_text(&plain(content), x + 18.0, self.y, font_scale, MUTED, 0.0)
                    + 22.0 * font_scale;
            }
            Block::CodeBlock { language, text, .. } => {
                self.rect(
                    x,
                    self.y - 12.0,
                    self.width - 2.0 * x,
                    block_height + 7.0 * font_scale,
                    [0.055, 0.075, 0.105, 1.0],
                    0.0,
                );
                if let Some(lang) = language {
                    self.text(lang, x + 14.0, self.y, font_scale, GREEN, 0.0);
                }
                self.code_text(
                    text,
                    language.as_deref(),
                    x + 14.0,
                    self.y + 22.0 * font_scale,
                    font_scale,
                    0.0,
                );
                self.y += block_height;
            }
            Block::UnorderedList(items) => {
                for item in items {
                    self.y = self.text(
                        &format!("* {}", plain(item)),
                        x + 10.0,
                        self.y,
                        font_scale,
                        FG,
                        0.0,
                    ) + 10.0 * font_scale;
                }
            }
            Block::OrderedList { start, items } => {
                for (i, item) in items.iter().enumerate() {
                    self.y = self.text(
                        &format!("{}. {}", *start as usize + i, plain(item)),
                        x + 10.0,
                        self.y,
                        font_scale,
                        FG,
                        0.0,
                    ) + 10.0 * font_scale;
                }
            }
            Block::ThematicBreak => {
                self.rect(x, self.y + 4.0, self.width - 2.0 * x, 2.0, MUTED, 0.0);
                self.y += 28.0;
            }
            Block::Table { headers, rows } => {
                self.table(headers, rows, x, font_scale);
            }
        }
    }

    fn table(&mut self, headers: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], x: f32, scale: f32) {
        let columns = headers
            .len()
            .max(rows.iter().map(Vec::len).max().unwrap_or(0))
            .max(1);
        let table_width = self.width - x * 2.0;
        let column_width = (table_width / columns as f32).max(80.0);
        let row_height = 24.0 * scale;
        let mut y = self.y;
        let header_height = row_height + 8.0 * scale;
        self.rect(
            x,
            y,
            column_width * columns as f32,
            header_height,
            [0.10, 0.19, 0.25, 1.0],
            0.0,
        );
        self.table_row(headers, x, y + 4.0 * scale, column_width, scale, FG);
        y += header_height;
        self.table_rule(x, y, column_width * columns as f32);
        for (index, row) in rows.iter().enumerate() {
            if index % 2 == 0 {
                self.rect(
                    x,
                    y,
                    column_width * columns as f32,
                    row_height + 8.0 * scale,
                    [0.055, 0.085, 0.12, 1.0],
                    0.0,
                );
            }
            self.table_row(row, x, y + 4.0 * scale, column_width, scale, FG);
            y += row_height + 8.0 * scale;
            self.table_rule(x, y, column_width * columns as f32);
        }
        for column in 0..=columns {
            let line_x = x + column as f32 * column_width;
            self.rect(line_x, self.y, 1.0, y - self.y, MUTED, 0.0);
        }
        self.y = y + 12.0 * scale;
    }

    fn table_row(
        &mut self,
        cells: &[Vec<Inline>],
        x: f32,
        y: f32,
        column_width: f32,
        scale: f32,
        color: [f32; 4],
    ) {
        for (column, cell) in cells.iter().enumerate() {
            let cell_x = x + column as f32 * column_width + 8.0;
            self.rich_text(&plain(cell), cell_x, y, scale, color, 0.0);
        }
    }

    fn table_rule(&mut self, x: f32, y: f32, width: f32) {
        self.rect(x, y, width, 1.0, MUTED, 0.0);
    }
}

fn plain(nodes: &[Inline]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Inline::Text(s) | Inline::Code(s) => out.push_str(s),
            Inline::Math { source, display } => {
                let delimiter = if *display { "$$" } else { "$" };
                out.push_str(delimiter);
                out.push_str(source);
                out.push_str(delimiter);
            }
            Inline::Emphasis(v) | Inline::Strong(v) => out.push_str(&plain(v)),
            Inline::Link { label, .. } => out.push_str(&plain(label)),
            Inline::SoftBreak => {
                while out.ends_with([' ', '\t']) {
                    out.pop();
                }
                out.push(' ');
            }
            Inline::HardBreak => out.push('\n'),
        }
    }
    out
}

struct SearchWord<'a> {
    trie: &'a mut SearchTrie<TextPos>,
    block: u32,
    offset: u32,
    start: u32,
    word: String,
}

impl SearchWord<'_> {
    fn character(&mut self, character: char, recorded: bool) {
        if character.is_alphanumeric() || character == '_' {
            if self.word.is_empty() {
                self.start = self.offset;
            }
            self.word.push(character);
        } else {
            self.flush();
        }
        if recorded {
            self.offset = self.offset.saturating_add(1);
        }
    }

    fn text(&mut self, text: &str) {
        for character in text.chars() {
            self.character(character, character != '\n');
        }
    }

    fn inlines(&mut self, nodes: &[Inline]) {
        for node in nodes {
            match node {
                Inline::Text(text) | Inline::Code(text) => self.text(text),
                Inline::Math { .. } => {
                    self.flush();
                    self.offset = self.offset.saturating_add(1);
                }
                Inline::Emphasis(children) | Inline::Strong(children) => self.inlines(children),
                Inline::Link { label, .. } => self.inlines(label),
                Inline::SoftBreak => self.character(' ', true),
                Inline::HardBreak => self.character('\n', false),
            }
        }
    }

    fn flush(&mut self) {
        if !self.word.is_empty() {
            for (character_offset, (byte_offset, _)) in
                self.word.char_indices().take(64).enumerate()
            {
                self.trie.insert(
                    &self.word[byte_offset..],
                    TextPos {
                        block: self.block,
                        offset: self.start.saturating_add(character_offset as u32),
                    },
                );
            }
            self.word.clear();
        }
    }
}

fn index_block(trie: &mut SearchTrie<TextPos>, block: u32, node: &Block) {
    let mut word = SearchWord {
        trie,
        block,
        offset: 0,
        start: 0,
        word: String::new(),
    };
    match node {
        Block::Heading { content, .. } | Block::Paragraph(content) | Block::BlockQuote(content) => {
            word.inlines(content)
        }
        Block::CodeBlock { text, .. } => word.text(text),
        Block::UnorderedList(items) => {
            for item in items {
                word.text("* ");
                word.inlines(item);
                word.flush();
            }
        }
        Block::OrderedList { start, items } => {
            for (index, item) in items.iter().enumerate() {
                word.text(&format!("{}. ", *start as usize + index));
                word.inlines(item);
                word.flush();
            }
        }
        Block::Table { headers, rows } => {
            for cell in headers.iter().chain(rows.iter().flatten()) {
                word.inlines(cell);
                word.flush();
            }
        }
        Block::ThematicBreak => {}
    }
    word.flush();
}

fn measure_block(block: &Block, width: f32, font_scale: f32) -> f32 {
    let x = content_gutter(width);
    match block {
        Block::Heading { level, content } => {
            let scale = match level {
                1 => 2.0,
                2 => 2.0,
                _ => 1.0,
            } * font_scale;
            measure_rich_text(&plain(content), x, 0.0, scale, width) + 22.0 * font_scale
        }
        Block::Paragraph(content) => {
            measure_rich_text(&plain(content), x, 0.0, font_scale, width) + 20.0 * font_scale
        }
        Block::BlockQuote(content) => {
            measure_rich_text(&plain(content), x + 18.0, 0.0, font_scale, width) + 22.0 * font_scale
        }
        Block::CodeBlock { text, .. } => {
            measure_code_text(text, x + 14.0, 22.0 * font_scale, font_scale, width)
                + 24.0 * font_scale
        }
        Block::UnorderedList(items) => items
            .iter()
            .map(|item| {
                measure_text(
                    &format!("* {}", plain(item)),
                    x + 10.0,
                    0.0,
                    font_scale,
                    width,
                ) + 10.0 * font_scale
            })
            .sum(),
        Block::OrderedList { start, items } => items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                measure_text(
                    &format!("{}. {}", *start as usize + i, plain(item)),
                    x + 10.0,
                    0.0,
                    font_scale,
                    width,
                ) + 10.0 * font_scale
            })
            .sum(),
        Block::ThematicBreak => 28.0,
        Block::Table { rows, .. } => ((rows.len() + 1) as f32 * (24.0 + 8.0) + 16.0) * font_scale,
    }
}

fn measure_text(text: &str, origin: f32, mut y: f32, scale: f32, width: f32) -> f32 {
    let mut x = origin;
    let line = 18.0 * scale;
    let limit = width - 30.0;
    for c in text.chars() {
        let advance = font::advance(c, scale);
        if c == '\n' || (x + advance > limit && x > origin) {
            x = origin;
            y += line;
            if c == '\n' {
                continue;
            }
        }
        x += advance;
    }
    y + 7.0 * scale
}

fn measure_code_text(text: &str, origin: f32, mut y: f32, scale: f32, width: f32) -> f32 {
    let mut x = origin;
    let line = 18.0 * scale;
    let limit = width - 30.0;
    let mut advances = MonoAdvances::new(scale);
    for c in text.chars() {
        let advance = advances.get(c);
        if c == '\n' || (x + advance > limit && x > origin) {
            x = origin;
            y += line;
            if c == '\n' {
                continue;
            }
        }
        x += advance;
    }
    y + 7.0 * scale
}

struct MonoAdvances {
    scale: f32,
    ascii: [f32; 128],
}

impl MonoAdvances {
    fn new(scale: f32) -> Self {
        Self {
            scale,
            ascii: [f32::NAN; 128],
        }
    }

    fn get(&mut self, c: char) -> f32 {
        if c.is_ascii() {
            let index = c as usize;
            let cached = self.ascii[index];
            if !cached.is_nan() {
                return cached;
            }
            let advance = font::mono_advance(c, self.scale);
            self.ascii[index] = advance;
            advance
        } else {
            font::mono_advance(c, self.scale)
        }
    }
}

fn measure_rich_text(text: &str, origin: f32, y: f32, scale: f32, width: f32) -> f32 {
    if !text.as_bytes().contains(&b'$') {
        return measure_text(text, origin, y, scale, width);
    }
    let mut cursor_x = origin;
    let mut cursor_y = y;
    let mut line_extra = 0.0;
    let mut rest = text;

    while let Some(open) = rest.find('$') {
        measure_text_run(
            &rest[..open],
            origin,
            &mut cursor_x,
            &mut cursor_y,
            &mut line_extra,
            scale,
            width,
        );
        let double = rest[open..].starts_with("$$");
        let delimiter_len = if double { 2 } else { 1 };
        let expression_start = open + delimiter_len;
        let delimiter = if double { "$$" } else { "$" };
        let Some(close_rel) = rest[expression_start..].find(delimiter) else {
            measure_text_run(
                &rest[open..],
                origin,
                &mut cursor_x,
                &mut cursor_y,
                &mut line_extra,
                scale,
                width,
            );
            rest = "";
            break;
        };
        let close = expression_start + close_rel;
        let expression = &rest[expression_start..close];
        match math::rasterize(expression, double, scale) {
            Ok(image) if double => {
                if cursor_x > origin {
                    cursor_y += 18.0 * scale + line_extra;
                }
                line_extra = 0.0;
                cursor_x = origin;
                cursor_y += image.height as f32 + 6.0;
            }
            Ok(image) => {
                if cursor_x + image.width as f32 > width - 30.0 && cursor_x > origin {
                    cursor_x = origin;
                    cursor_y += 18.0 * scale + line_extra;
                    line_extra = 0.0;
                }
                let text_baseline = cursor_y + font::baseline(scale);
                let image_y = text_baseline - image.baseline as f32;
                let image_bottom = image_y + image.height as f32;
                line_extra = line_extra.max(image_bottom - cursor_y - 18.0 * scale);
                cursor_x += image.width as f32 + 2.0 * scale;
            }
            Err(_) => measure_text_run(
                expression,
                origin,
                &mut cursor_x,
                &mut cursor_y,
                &mut line_extra,
                scale,
                width,
            ),
        }
        rest = &rest[close + delimiter_len..];
    }

    measure_text_run(
        rest,
        origin,
        &mut cursor_x,
        &mut cursor_y,
        &mut line_extra,
        scale,
        width,
    );
    cursor_y + 7.0 * scale + line_extra
}

fn measure_text_run(
    text: &str,
    origin: f32,
    cursor_x: &mut f32,
    cursor_y: &mut f32,
    line_extra: &mut f32,
    scale: f32,
    width: f32,
) {
    let line = 18.0 * scale;
    let limit = width - 30.0;
    for c in text.chars() {
        let advance = font::advance(c, scale);
        if c == '\n' || (*cursor_x + advance > limit && *cursor_x > origin) {
            *cursor_x = origin;
            *cursor_y += line + *line_extra;
            *line_extra = 0.0;
            if c == '\n' {
                continue;
            }
        }
        *cursor_x += advance;
    }
}

fn runtime_config() -> (f64, usize, bool, f32, f64) {
    let search = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default();
    let number = |name: &str| {
        search.trim_start_matches('?').split('&').find_map(|part| {
            let (key, value) = part.split_once('=')?;
            (key == name).then(|| value.parse::<f64>().ok()).flatten()
        })
    };
    let tps = number("tps")
        .unwrap_or(DEFAULT_TPS)
        .clamp(1.0, 10_000_000.0);
    let repeats = number("repeat")
        .unwrap_or(DEFAULT_REPEATS as f64)
        .clamp(1.0, 100_000.0) as usize;
    let auto_scroll = number("autoscroll").unwrap_or(1.0) != 0.0;
    let font_size = number("fontsize")
        .unwrap_or(DEFAULT_FONT_SIZE as f64)
        .clamp(10.0, 40.0) as f32;
    let reduced_motion = web_sys::window()
        .and_then(|window| {
            window
                .match_media("(prefers-reduced-motion: reduce)")
                .ok()
                .flatten()
        })
        .is_some_and(|query| query.matches());
    let fade_ms = number("fade")
        .unwrap_or(if reduced_motion { 0.0 } else { DEFAULT_FADE_MS })
        .clamp(0.0, 2_000.0);
    (tps, repeats, auto_scroll, font_size, fade_ms)
}

fn set_renderer_metadata(
    canvas: &HtmlCanvasElement,
    preference: RendererPreference,
    backend: RendererBackend,
    fallback_depth: usize,
) {
    let _ = canvas.set_attribute("data-renderer-requested", preference.as_str());
    let _ = canvas.set_attribute("data-renderer-candidate", backend.as_str());
    let _ = canvas.set_attribute("data-renderer-fallback-depth", &fallback_depth.to_string());
    let _ = canvas.set_attribute("data-renderer-common-profile", "webgl2-limits");
}

fn selected_mock() -> &'static str {
    let search = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default();
    for part in search.trim_start_matches('?').split('&') {
        match part {
            "doc=easy" => return EASY_MOCK,
            "doc=stress" => return STRESS_MOCK,
            "doc=code" => return CODE_MOCK,
            _ => {}
        }
    }
    DEFAULT_MOCK
}

fn content_gutter(width: f32) -> f32 {
    if width < 480.0 { 16.0 } else { 34.0 }
}

fn pack_color(color: [f32; 4]) -> u32 {
    color
        .into_iter()
        .enumerate()
        .fold(0, |packed, (shift, channel)| {
            packed | ((channel.clamp(0.0, 1.0) * 255.0).round() as u32) << (shift * 8)
        })
}

fn tokenize(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut token = String::new();
    for c in source.chars() {
        token.push(c);
        if c.is_whitespace() || token.len() >= 8 {
            out.push(std::mem::take(&mut token));
        }
    }
    if !token.is_empty() {
        out.push(token);
    }
    out
}

fn replace_canvas(canvas: &HtmlCanvasElement) -> Result<HtmlCanvasElement, JsValue> {
    canvas.set_outer_html(
        r#"<canvas id="app" tabindex="0" role="region" aria-label="Rendered streaming Markdown document" aria-describedby="canvas-help">Your browser does not support canvas. Use the text version on this page.</canvas>"#,
    );
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("app"))
        .ok_or_else(|| JsValue::from_str("failed to replace renderer canvas"))?
        .dyn_into()
        .map_err(Into::into)
}

fn resize_canvas(canvas: &HtmlCanvasElement) -> bool {
    // Keep scene units equal to CSS pixels. This also caps fill rate on HiDPI screens.
    let width = canvas.client_width().max(1) as u32;
    let height = canvas.client_height().max(1) as u32;
    if canvas.width() == width && canvas.height() == height {
        false
    } else {
        canvas.set_width(width);
        canvas.set_height(height);
        true
    }
}
