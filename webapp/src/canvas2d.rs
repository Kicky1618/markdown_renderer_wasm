use std::{cell::RefCell, collections::HashMap, rc::Rc};

use js_sys::{Promise, Reflect};
use streamdown::{Block, Inline, Op, Parser};
use wasm_bindgen::{JsCast, closure::Closure, prelude::JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, KeyboardEvent, MouseEvent, WheelEvent};

use super::{
    code, content_gutter,
    math::{self, MathImage},
    search::SearchTrie,
};

type AnimationLoop = Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>;

#[path = "canvas2d/compat.rs"]
mod canvas_compat;
use canvas_compat::{backing_size, wheel_delta_css_pixels};

struct HitLine {
    start: usize,
    end: usize,
    y: f64,
    height: f64,
    boundaries: Vec<(usize, f64)>,
}

struct State {
    canvas: HtmlCanvasElement,
    context: CanvasRenderingContext2d,
    source: String,
    parser: Parser,
    source_offset: usize,
    current_repeat: usize,
    repeats: usize,
    auto_scroll: bool,
    paused: bool,
    credit: f64,
    tps: f64,
    font_scale: f64,
    fade_ms: f64,
    fade_until: f64,
    block_births: Vec<f64>,
    last_time: f64,
    scroll: f64,
    content_height: f64,
    dragging_scrollbar: bool,
    selecting_text: bool,
    touch_last_y: Option<f64>,
    selection_anchor: Option<usize>,
    selection_focus: Option<usize>,
    selection_text: String,
    hit_lines: Vec<HitLine>,
    search_lines: Vec<(usize, usize, f64)>,
    math_images: HashMap<(String, bool, u16), Rc<MathImage>>,
    search_index: SearchTrie<usize>,
    search_query: String,
    search_matches: Vec<(usize, usize)>,
    search_active: usize,
    search_dirty: bool,
    search_indexed_at: f64,
    dirty: bool,
}

pub fn start(
    canvas: HtmlCanvasElement,
    source: &str,
    tps: f64,
    repeats: usize,
    auto_scroll: bool,
    font_size: f32,
    fade_ms: f64,
) -> Result<(), JsValue> {
    let _ = canvas.set_attribute("data-renderer", "canvas2d");
    let _ = canvas.set_attribute("data-paused", "false");
    let _ = canvas.set_attribute("data-auto-scroll", &auto_scroll.to_string());
    let _ = canvas.set_attribute("data-font-size", &format!("{font_size:.0}"));
    let _ = canvas.set_attribute("data-search-count", "0");
    let _ = canvas.set_attribute("data-search-active", "0");
    let context: CanvasRenderingContext2d = canvas
        .get_context("2d")?
        .ok_or("Canvas2D context unavailable")?
        .dyn_into()?;
    resize(&canvas, &context);
    let state = Rc::new(RefCell::new(State {
        canvas: canvas.clone(),
        context,
        source: source.to_owned(),
        parser: Parser::new(),
        source_offset: 0,
        current_repeat: 0,
        repeats,
        auto_scroll,
        paused: false,
        credit: 1.0,
        tps,
        font_scale: font_size as f64 / 16.0,
        fade_ms,
        fade_until: 0.0,
        block_births: Vec::new(),
        last_time: 0.0,
        scroll: 0.0,
        content_height: 0.0,
        dragging_scrollbar: false,
        selecting_text: false,
        touch_last_y: None,
        selection_anchor: None,
        selection_focus: None,
        selection_text: String::new(),
        hit_lines: Vec::new(),
        search_lines: Vec::new(),
        math_images: HashMap::new(),
        search_index: SearchTrie::default(),
        search_query: String::new(),
        search_matches: Vec::new(),
        search_active: 0,
        search_dirty: true,
        search_indexed_at: 0.0,
        dirty: true,
    }));
    install_font_redraw(&state);

    let wheel_state = state.clone();
    let wheel = Closure::<dyn FnMut(WheelEvent)>::new(move |event: WheelEvent| {
        event.prevent_default();
        let mut state = wheel_state.borrow_mut();
        let viewport = logical_height(&state.canvas);
        let max = (state.content_height - viewport).max(0.0);
        let delta = wheel_delta_css_pixels(
            event.delta_y(),
            event.delta_mode(),
            (25.0 * state.font_scale).max(16.0),
            viewport,
        );
        if delta < 0.0 {
            state.auto_scroll = false;
        }
        state.scroll = (state.scroll + delta).clamp(0.0, max);
        if delta > 0.0 && state.scroll >= max - 24.0 {
            state.auto_scroll = true;
        }
        state.dirty = true;
        state.sync_control_state();
    });
    canvas.add_event_listener_with_callback("wheel", wheel.as_ref().unchecked_ref())?;
    wheel.forget();

    // Touch fallback for mobile Safari/Chrome. The canvas intentionally uses
    // `touch-action: none`, so native page panning is disabled and we must
    // provide document scrolling ourselves. Generic Event + Reflect keeps this
    // path working without relying on newer TouchEvent web-sys bindings.
    let touch_start_state = state.clone();
    let touch_start = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        if let Some(y) = touch_client_y(&event) {
            touch_start_state.borrow_mut().touch_last_y = Some(y);
        }
    });
    canvas.add_event_listener_with_callback("touchstart", touch_start.as_ref().unchecked_ref())?;
    touch_start.forget();

    let touch_move_state = state.clone();
    let touch_move = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        let Some(y) = touch_client_y(&event) else {
            return;
        };
        let mut state = touch_move_state.borrow_mut();
        let Some(previous_y) = state.touch_last_y.replace(y) else {
            return;
        };
        let delta = previous_y - y;
        if delta.abs() < f64::EPSILON {
            return;
        }
        event.prevent_default();
        let viewport = logical_height(&state.canvas);
        let max = (state.content_height - viewport).max(0.0);
        state.auto_scroll = false;
        state.scroll = (state.scroll + delta).clamp(0.0, max);
        if delta > 0.0 && state.scroll >= max - 24.0 {
            state.auto_scroll = true;
        }
        state.dirty = true;
        state.sync_control_state();
    });
    canvas.add_event_listener_with_callback("touchmove", touch_move.as_ref().unchecked_ref())?;
    touch_move.forget();

    for event_name in ["touchend", "touchcancel"] {
        let touch_end_state = state.clone();
        let touch_end = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            touch_end_state.borrow_mut().touch_last_y = None;
        });
        canvas.add_event_listener_with_callback(event_name, touch_end.as_ref().unchecked_ref())?;
        touch_end.forget();
    }

    let down_state = state.clone();
    let down = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
        let mut state = down_state.borrow_mut();
        let rect = state.canvas.get_bounding_client_rect();
        let x = event.client_x() as f64 - rect.left();
        if event.button() == 0 && x >= logical_width(&state.canvas) - 28.0 {
            state.dragging_scrollbar = true;
            state.update_scrollbar(event.client_y() as f64);
            let _ = state.canvas.set_attribute("data-cursor", "grabbing");
        } else if event.button() == 0 {
            let y = event.client_y() as f64 - rect.top();
            if let Some(position) = state.nearest_text_position(x, y) {
                state.auto_scroll = false;
                state.sync_control_state();
                state.selecting_text = true;
                if !event.shift_key() || state.selection_anchor.is_none() {
                    state.selection_anchor = Some(position);
                }
                state.selection_focus = Some(position);
                state.dirty = true;
            }
        }
    });
    canvas.add_event_listener_with_callback("mousedown", down.as_ref().unchecked_ref())?;
    down.forget();

    let move_state = state.clone();
    let moved = Closure::<dyn FnMut(MouseEvent)>::new(move |event: MouseEvent| {
        let mut state = move_state.borrow_mut();
        if state.dragging_scrollbar {
            state.update_scrollbar(event.client_y() as f64);
        } else if state.selecting_text {
            let rect = state.canvas.get_bounding_client_rect();
            let x = event.client_x() as f64 - rect.left();
            let y = event.client_y() as f64 - rect.top();
            if y < 92.0 {
                state.scroll = (state.scroll - 24.0).max(0.0);
            } else if y > logical_height(&state.canvas) - 54.0 {
                let max_scroll = (state.content_height - logical_height(&state.canvas)).max(0.0);
                state.scroll = (state.scroll + 24.0).min(max_scroll);
            }
            if let Some(position) = state.nearest_text_position(x, y) {
                state.selection_focus = Some(position);
                state.dirty = true;
            }
        } else {
            let rect = state.canvas.get_bounding_client_rect();
            let x = event.client_x() as f64 - rect.left();
            let y = event.client_y() as f64 - rect.top();
            let cursor = if x >= logical_width(&state.canvas) - 28.0 {
                "grab"
            } else if y >= 68.0 {
                "text"
            } else {
                "default"
            };
            let _ = state.canvas.set_attribute("data-cursor", cursor);
        }
    });
    web_sys::window()
        .ok_or("window unavailable")?
        .add_event_listener_with_callback("mousemove", moved.as_ref().unchecked_ref())?;
    moved.forget();

    let up_state = state.clone();
    let up = Closure::<dyn FnMut(MouseEvent)>::new(move |_event: MouseEvent| {
        let mut state = up_state.borrow_mut();
        state.dragging_scrollbar = false;
        state.selecting_text = false;
        let _ = state.canvas.set_attribute("data-cursor", "default");
    });
    web_sys::window()
        .ok_or("window unavailable")?
        .add_event_listener_with_callback("mouseup", up.as_ref().unchecked_ref())?;
    up.forget();

    let key_state = state.clone();
    let key = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
        let mut state = key_state.borrow_mut();
        if (event.ctrl_key() || event.meta_key()) && event.key().eq_ignore_ascii_case("c") {
            if state.copy_selection() {
                event.prevent_default();
            }
        } else if event.key() == "Escape" {
            state.selection_anchor = None;
            state.selection_focus = None;
            state.dirty = true;
        } else if event
            .target()
            .and_then(|target| target.dyn_into::<HtmlCanvasElement>().ok())
            .is_some()
        {
            let viewport = logical_height(&state.canvas);
            let max_scroll = (state.content_height - viewport).max(0.0);
            let key = event.key();
            if matches!(key.as_str(), "F3" | "F3Previous") {
                event.prevent_default();
                state.navigate_search(key == "F3Previous" || event.shift_key());
            } else if key.eq_ignore_ascii_case("p") {
                event.prevent_default();
                state.paused = !state.paused;
                state.dirty = true;
            } else if key.eq_ignore_ascii_case("a") {
                event.prevent_default();
                state.auto_scroll = !state.auto_scroll;
                if state.auto_scroll {
                    state.scroll = max_scroll;
                }
                state.dirty = true;
            } else if matches!(key.as_str(), "+" | "=") {
                event.prevent_default();
                state.font_scale = (state.font_scale + 0.125).min(2.5);
                state.dirty = true;
            } else if matches!(key.as_str(), "-" | "_") {
                event.prevent_default();
                state.font_scale = (state.font_scale - 0.125).max(0.625);
                state.dirty = true;
            } else {
                let target = match key.as_str() {
                    "ArrowUp" => Some(state.scroll - 48.0),
                    "ArrowDown" => Some(state.scroll + 48.0),
                    "PageUp" => Some(state.scroll - viewport * 0.85),
                    "PageDown" => Some(state.scroll + viewport * 0.85),
                    " " if event.shift_key() => Some(state.scroll - viewport * 0.85),
                    " " => Some(state.scroll + viewport * 0.85),
                    "Home" => Some(0.0),
                    "End" => Some(max_scroll),
                    _ => None,
                };
                if let Some(target) = target {
                    event.prevent_default();
                    state.auto_scroll = key == "End";
                    state.scroll = target.clamp(0.0, max_scroll);
                    state.dirty = true;
                }
            }
            state.sync_control_state();
        }
    });
    web_sys::window()
        .ok_or("window unavailable")?
        .add_event_listener_with_callback("keydown", key.as_ref().unchecked_ref())?;
    key.forget();

    let callback: AnimationLoop = Rc::new(RefCell::new(None));
    let callback_copy = callback.clone();
    *callback.borrow_mut() = Some(Closure::new(move |time: f64| {
        state.borrow_mut().frame(time);
        if let Some(callback) = callback_copy.borrow().as_ref() {
            let _ = web_sys::window()
                .expect("window")
                .request_animation_frame(callback.as_ref().unchecked_ref());
        }
    }));
    web_sys::window()
        .ok_or("window unavailable")?
        .request_animation_frame(callback.borrow().as_ref().unwrap().as_ref().unchecked_ref())?;
    Ok(())
}

impl State {
    fn sync_search(&mut self) {
        let query = self
            .canvas
            .get_attribute("data-search-query")
            .unwrap_or_default();
        if query != self.search_query {
            self.search_query = query;
            self.search_active = 0;
            if !self.search_dirty {
                self.update_search_matches();
            } else {
                self.search_indexed_at = 0.0;
            }
            self.dirty = true;
        }
        if self.search_query.trim().is_empty() && !self.search_matches.is_empty() {
            self.search_matches.clear();
            self.sync_search_state();
            self.dirty = true;
        }
    }

    fn rebuild_search_index(&mut self) {
        self.search_index.clear();
        let mut word = String::new();
        let mut start = 0;
        for (offset, character) in self.selection_text.char_indices() {
            if character.is_alphanumeric() || character == '_' {
                if word.is_empty() {
                    start = offset;
                }
                word.push(character);
            } else if !word.is_empty() {
                for (byte_offset, _) in word.char_indices().take(64) {
                    self.search_index
                        .insert(&word[byte_offset..], start + byte_offset);
                }
                word.clear();
            }
        }
        if !word.is_empty() {
            for (byte_offset, _) in word.char_indices().take(64) {
                self.search_index
                    .insert(&word[byte_offset..], start + byte_offset);
            }
        }
        self.update_search_matches();
        self.search_dirty = false;
        self.search_indexed_at = self.last_time;
    }

    fn update_search_matches(&mut self) {
        let length = self.search_query.trim().len();
        self.search_matches = self
            .search_index
            .search(&self.search_query)
            .iter()
            .map(|start| (*start, start.saturating_add(length)))
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
        if let Some((_, _, line_y)) = self
            .search_lines
            .iter()
            .find(|(start, end, _)| position >= *start && position <= *end)
        {
            let max_scroll = (self.content_height - logical_height(&self.canvas)).max(0.0);
            self.scroll = (self.scroll + *line_y - 104.0).clamp(0.0, max_scroll);
            self.auto_scroll = false;
        }
        self.sync_control_state();
        self.sync_search_state();
        self.dirty = true;
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

    fn sync_control_state(&self) {
        let _ = self
            .canvas
            .set_attribute("data-paused", &self.paused.to_string());
        let _ = self
            .canvas
            .set_attribute("data-auto-scroll", &self.auto_scroll.to_string());
        let _ = self
            .canvas
            .set_attribute("data-font-size", &format!("{:.0}", self.font_scale * 16.0));
    }

    fn update_scrollbar(&mut self, client_y: f64) {
        let max_scroll = (self.content_height - logical_height(&self.canvas)).max(0.0);
        if max_scroll <= 0.0 {
            return;
        }
        let rect = self.canvas.get_bounding_client_rect();
        let track_height = (logical_height(&self.canvas) - 84.0).max(20.0);
        let thumb_height = (track_height * logical_height(&self.canvas) / self.content_height)
            .clamp(28.0, track_height);
        let ratio = ((client_y - rect.top() - 76.0 - thumb_height * 0.5)
            / (track_height - thumb_height).max(1.0))
        .clamp(0.0, 1.0);
        self.auto_scroll = false;
        self.scroll = ratio * max_scroll;
        self.dirty = true;
        self.sync_control_state();
    }

    fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.selection_anchor?;
        let focus = self.selection_focus?;
        (anchor != focus).then_some(if anchor < focus {
            (anchor, focus)
        } else {
            (focus, anchor)
        })
    }

    fn nearest_text_position(&self, x: f64, y: f64) -> Option<usize> {
        if y < 68.0 {
            return None;
        }
        let line = self.hit_lines.iter().min_by(|a, b| {
            let distance = |line: &HitLine| {
                if y < line.y {
                    line.y - y
                } else if y > line.y + line.height {
                    y - line.y - line.height
                } else {
                    0.0
                }
            };
            distance(a).total_cmp(&distance(b))
        })?;
        line.boundaries
            .iter()
            .min_by(|(_, a), (_, b)| (a - x).abs().total_cmp(&(b - x).abs()))
            .map(|(position, _)| *position)
    }

    fn copy_selection(&self) -> bool {
        let Some((start, end)) = self.selection_range() else {
            return false;
        };
        let Some(text) = self.selection_text.get(start..end) else {
            return false;
        };
        let _ = web_sys::window().map(|window| window.navigator().clipboard().write_text(text));
        true
    }

    fn record_line(&mut self, text: &str, x: f64, y: f64, height: f64, visible: bool) {
        if !self.selection_text.is_empty() {
            self.selection_text.push('\n');
        }
        let start = self.selection_text.len();
        self.selection_text.push_str(text);
        let end = self.selection_text.len();
        self.search_lines.push((start, end, y));
        if !visible {
            return;
        }
        let mut boundaries = Vec::with_capacity(text.chars().count() + 1);
        let mut cursor = x;
        boundaries.push((start, cursor));
        for (offset, character) in text.char_indices() {
            cursor += self
                .context
                .measure_text(&character.to_string())
                .map_or(0.0, |metrics| metrics.width());
            boundaries.push((start + offset + character.len_utf8(), cursor));
        }
        self.hit_lines.push(HitLine {
            start,
            end,
            y: y - height * 0.8,
            height,
            boundaries,
        });
    }

    fn draw_selection(&self) {
        let Some((start, end)) = self.selection_range() else {
            return;
        };
        self.context.save();
        self.context.set_global_alpha(0.38);
        self.context.set_fill_style_str("#347ac7");
        for line in &self.hit_lines {
            if end <= line.start || start >= line.end || line.boundaries.is_empty() {
                continue;
            }
            let from = start.max(line.start);
            let to = end.min(line.end);
            let x1 = line
                .boundaries
                .iter()
                .find(|(position, _)| *position >= from)
                .map_or(line.boundaries[0].1, |(_, x)| *x);
            let x2 = line
                .boundaries
                .iter()
                .rev()
                .find(|(position, _)| *position <= to)
                .map_or(x1, |(_, x)| *x);
            self.context
                .fill_rect(x1, line.y, (x2 - x1).max(2.0), line.height);
        }
        self.context.restore();
    }

    fn draw_search(&self) {
        if self.search_matches.is_empty() {
            return;
        }
        self.context.save();
        for line in &self.hit_lines {
            if line.boundaries.is_empty() {
                continue;
            }
            let first = self
                .search_matches
                .partition_point(|(_, end)| *end <= line.start);
            let last = self
                .search_matches
                .partition_point(|(start, _)| *start < line.end);
            for index in first..last {
                let (start, end) = self.search_matches[index];
                self.context
                    .set_global_alpha(if index == self.search_active {
                        0.72
                    } else {
                        0.38
                    });
                self.context
                    .set_fill_style_str(if index == self.search_active {
                        "#fa8c33"
                    } else {
                        "#ead33c"
                    });
                let from = start.max(line.start);
                let to = end.min(line.end);
                let x1 = line
                    .boundaries
                    .iter()
                    .find(|(position, _)| *position >= from)
                    .map_or(line.boundaries[0].1, |(_, x)| *x);
                let x2 = line
                    .boundaries
                    .iter()
                    .rev()
                    .find(|(position, _)| *position <= to)
                    .map_or(x1, |(_, x)| *x);
                self.context
                    .fill_rect(x1, line.y, (x2 - x1).max(2.0), line.height);
            }
        }
        self.context.restore();
    }

    fn frame(&mut self, time: f64) {
        self.sync_search();
        if self.search_dirty
            && !self.search_query.trim().is_empty()
            && (self.search_indexed_at == 0.0 || time - self.search_indexed_at >= 200.0)
        {
            self.dirty = true;
        }
        if resize(&self.canvas, &self.context) {
            self.dirty = true;
        }
        let dt = if self.last_time == 0.0 {
            0.0
        } else {
            ((time - self.last_time) / 1_000.0).min(0.1)
        };
        self.last_time = time;
        if !self.paused {
            self.credit += dt * self.tps;
        }
        let budget = if self.paused {
            0
        } else {
            self.credit.floor() as usize
        };
        let appended = self.append_stream(budget, time);
        if appended > 0 {
            self.credit -= appended as f64;
            self.fade_until = time + self.fade_ms;
            self.dirty = true;
            self.search_dirty = true;
        }
        if time < self.fade_until {
            self.dirty = true;
        }
        if self.dirty {
            self.draw();
            if self.auto_scroll {
                let max_scroll = (self.content_height - logical_height(&self.canvas)).max(0.0);
                if (self.scroll - max_scroll).abs() > 0.01 {
                    self.scroll = max_scroll;
                    self.draw();
                }
            }
            self.dirty = false;
        }
    }

    fn append_stream(&mut self, budget: usize, time: f64) -> usize {
        let mut appended = 0;
        while appended < budget && self.current_repeat < self.repeats && !self.source.is_empty() {
            let available = self.source.len() - self.source_offset;
            let requested = (budget - appended).min(available);
            let end = self
                .source
                .floor_char_boundary(self.source_offset + requested);
            if end == self.source_offset {
                break;
            }
            let chunk = &self.source[self.source_offset..end];
            let delta = self.parser.append(chunk);
            for operation in delta.ops {
                match operation {
                    Op::Truncate { from } => self.block_births.truncate(from as usize),
                    Op::Push(_) => self.block_births.push(time),
                    Op::SpliceCode { .. }
                    | Op::SealCode { .. }
                    | Op::AppendText { .. }
                    | Op::AppendInlineText { .. }
                    | Op::SpliceInlineTail { .. }
                    | Op::AppendListItem { .. }
                    | Op::SpliceListItemTail { .. }
                    | Op::SpliceQuoteTail { .. }
                    | Op::AppendTableRow { .. }
                    | Op::AppendTableCell { .. }
                    | Op::SpliceTableCellTail { .. } => {}
                }
            }
            appended += end - self.source_offset;
            self.source_offset = end;
            if self.source_offset == self.source.len() {
                self.source_offset = 0;
                self.current_repeat += 1;
            }
        }
        appended
    }

    fn draw(&mut self) {
        let width = logical_width(&self.canvas);
        let height = logical_height(&self.canvas);
        self.selection_text.clear();
        self.hit_lines.clear();
        self.search_lines.clear();
        self.context.set_global_alpha(1.0);
        self.context.set_fill_style_str("#090c12");
        self.context.fill_rect(0.0, 0.0, width, height);
        let parser = std::mem::replace(&mut self.parser, Parser::new());
        let mut y = 104.0 - self.scroll;
        for (index, block) in parser.blocks().iter().enumerate() {
            let born_at = self.block_births.get(index).copied().unwrap_or(0.0);
            self.context.set_global_alpha(self.fade_opacity(born_at));
            y += self.draw_block(block, y, width, height);
        }
        if self.search_dirty
            && !self.search_query.trim().is_empty()
            && (self.search_indexed_at == 0.0 || self.last_time - self.search_indexed_at >= 200.0)
        {
            self.rebuild_search_index();
        }
        self.draw_search();
        self.draw_selection();
        self.parser = parser;
        self.context.set_global_alpha(1.0);
        self.context.set_fill_style_str("#0e1a25");
        self.context.fill_rect(0.0, 0.0, width, 68.0);
        self.set_font(16.0, "\"Streamdown Noto Sans\"");
        self.context.set_fill_style_str("#40dcdf");
        let title = if width < 480.0 {
            "STREAMDOWN"
        } else if self.paused {
            "STREAMDOWN / PAUSED"
        } else {
            "STREAMDOWN / CANVAS2D"
        };
        let _ = self.context.fill_text(title, 24.0, 40.0);
        self.content_height = y + self.scroll + 40.0;
        if self.content_height > height {
            let track = height - 84.0;
            let thumb = (track * height / self.content_height).clamp(28.0, track);
            let max_scroll = (self.content_height - height).max(1.0);
            let thumb_y = 76.0 + (track - thumb) * self.scroll / max_scroll;
            self.context.set_fill_style_str("#24454e");
            self.context.fill_rect(width - 14.0, thumb_y, 8.0, thumb);
        }
    }

    fn draw_block(&mut self, block: &Block, y: f64, width: f64, viewport_height: f64) -> f64 {
        let font_scale = self.font_scale;
        let gutter = content_gutter(width as f32) as f64;
        match block {
            Block::Heading { level, content } => {
                let (font_size, scale, line_height) = match level {
                    1 => (30.0, 30.0 / 16.0, 44.0),
                    2 => (24.0, 24.0 / 16.0, 36.0),
                    _ => (18.0, 18.0 / 16.0, 30.0),
                };
                self.set_font(font_size, "\"Streamdown Noto Sans\"");
                self.context.set_fill_style_str("#40dcdf");
                self.draw_lines(
                    &plain(content),
                    gutter,
                    y,
                    (scale * font_scale) as f32,
                    line_height * font_scale,
                    viewport_height,
                ) + 8.0 * font_scale
            }
            Block::Paragraph(content) => {
                if let [
                    Inline::Math {
                        source,
                        display: true,
                    },
                ] = content.as_slice()
                {
                    self.set_font(16.0, "\"Streamdown Noto Sans\"");
                    let selected = format!("$${source}$$");
                    self.record_line(
                        &selected,
                        gutter,
                        y,
                        25.0 * font_scale,
                        y > 62.0 && y < viewport_height + 25.0 * font_scale,
                    );
                    return self.draw_display_math(source, gutter, y, viewport_height) + 12.0;
                }
                self.set_font(16.0, "\"Streamdown Noto Sans\"");
                self.context.set_fill_style_str("#d1dbe8");
                self.draw_lines(
                    &plain(content),
                    gutter,
                    y,
                    font_scale as f32,
                    25.0 * font_scale,
                    viewport_height,
                ) + 8.0 * font_scale
            }
            Block::BlockQuote(content) => {
                self.context.set_fill_style_str("#40dcdf");
                if y + 36.0 > 68.0 && y < viewport_height {
                    self.context.fill_rect(gutter, y - 16.0, 4.0, 36.0);
                }
                self.set_font(16.0, "\"Streamdown Noto Sans\"");
                self.context.set_fill_style_str("#78879e");
                self.draw_lines(
                    &plain(content),
                    gutter + 18.0,
                    y,
                    font_scale as f32,
                    25.0 * font_scale,
                    viewport_height,
                ) + 8.0 * font_scale
            }
            Block::CodeBlock { language, text, .. } => {
                self.draw_code_block(text, language.as_deref(), y, width, viewport_height)
            }
            Block::UnorderedList(items) => {
                let mut used = 0.0;
                self.set_font(16.0, "\"Streamdown Noto Sans\"");
                self.context.set_fill_style_str("#d1dbe8");
                for item in items {
                    used += self.draw_lines(
                        &format!("• {}", plain(item)),
                        gutter + 10.0,
                        y + used,
                        font_scale as f32,
                        25.0 * font_scale,
                        viewport_height,
                    );
                }
                used + 8.0 * font_scale
            }
            Block::OrderedList { start, items } => {
                let mut used = 0.0;
                self.set_font(16.0, "\"Streamdown Noto Sans\"");
                self.context.set_fill_style_str("#d1dbe8");
                for (index, item) in items.iter().enumerate() {
                    used += self.draw_lines(
                        &format!("{}. {}", *start as usize + index, plain(item)),
                        gutter + 10.0,
                        y + used,
                        font_scale as f32,
                        25.0 * font_scale,
                        viewport_height,
                    );
                }
                used + 8.0 * font_scale
            }
            Block::ThematicBreak => {
                if y > 68.0 && y < viewport_height {
                    self.context.set_fill_style_str("#4c5868");
                    self.context
                        .fill_rect(gutter, y, (width - gutter * 2.0).max(0.0), 1.0);
                }
                28.0 * font_scale
            }
            Block::Table { headers, rows } => {
                self.draw_table(headers, rows, y, width, viewport_height)
            }
        }
    }

    fn draw_table(
        &mut self,
        headers: &[Vec<Inline>],
        rows: &[Vec<Vec<Inline>>],
        y: f64,
        width: f64,
        viewport_height: f64,
    ) -> f64 {
        let scale = self.font_scale;
        let gutter = content_gutter(width as f32) as f64;
        let columns = headers
            .len()
            .max(rows.iter().map(Vec::len).max().unwrap_or(0))
            .max(1);
        let available_width = (width - gutter * 2.0).max(1.0);
        let column_width = (available_width / columns as f64).max(80.0);
        let table_width = column_width * columns as f64;
        let row_height = 24.0 * scale;
        let painted_row_height = row_height + 8.0 * scale;
        let mut row_top = y - 18.0 * scale;

        self.set_font(15.0, "\"Streamdown Noto Sans\"");
        self.draw_table_row_background(
            gutter,
            row_top,
            table_width,
            painted_row_height,
            "#1a3040",
            viewport_height,
        );
        self.draw_table_row(
            headers,
            gutter,
            row_top,
            column_width,
            painted_row_height,
            "#d1dbe8",
            viewport_height,
        );
        row_top += painted_row_height;
        self.draw_table_rule(gutter, row_top, table_width, viewport_height);

        for (index, row) in rows.iter().enumerate() {
            if index % 2 == 0 {
                self.draw_table_row_background(
                    gutter,
                    row_top,
                    table_width,
                    painted_row_height,
                    "#0e161f",
                    viewport_height,
                );
            }
            self.draw_table_row(
                row,
                gutter,
                row_top,
                column_width,
                painted_row_height,
                "#d1dbe8",
                viewport_height,
            );
            row_top += painted_row_height;
            self.draw_table_rule(gutter, row_top, table_width, viewport_height);
        }

        let table_top = y - 18.0 * scale;
        let table_bottom = row_top;
        self.context.set_fill_style_str("#78879e");
        if table_bottom > 68.0 && table_top < viewport_height {
            for column in 0..=columns {
                self.context.fill_rect(
                    gutter + column as f64 * column_width,
                    table_top.max(68.0),
                    1.0,
                    (table_bottom.min(viewport_height) - table_top.max(68.0)).max(0.0),
                );
            }
        }

        (table_bottom - table_top) + 12.0 * scale
    }

    fn draw_table_row_background(
        &self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        color: &str,
        viewport_height: f64,
    ) {
        if y + height <= 68.0 || y >= viewport_height {
            return;
        }
        self.context.set_fill_style_str(color);
        self.context.fill_rect(
            x,
            y.max(68.0),
            width,
            (y + height).min(viewport_height) - y.max(68.0),
        );
    }

    fn draw_table_row(
        &mut self,
        cells: &[Vec<Inline>],
        x: f64,
        row_top: f64,
        column_width: f64,
        row_height: f64,
        color: &str,
        viewport_height: f64,
    ) {
        let baseline = row_top + row_height * 0.70;
        for (column, cell) in cells.iter().enumerate() {
            let cell_x = x + column as f64 * column_width;
            let text_x = cell_x + 8.0 * self.font_scale;
            let text = plain(cell);
            let visible = row_top + row_height > 68.0 && row_top < viewport_height;
            self.record_line(&text, text_x, baseline, row_height, visible);
            if !visible {
                continue;
            }
            self.context.save();
            self.context.begin_path();
            self.context.rect(
                cell_x + 1.0,
                row_top.max(68.0),
                (column_width - 2.0).max(0.0),
                ((row_top + row_height).min(viewport_height) - row_top.max(68.0)).max(0.0),
            );
            self.context.clip();
            self.context.set_fill_style_str(color);
            let _ = self.context.fill_text(&text, text_x, baseline);
            self.context.restore();
        }
    }

    fn draw_table_rule(&self, x: f64, y: f64, width: f64, viewport_height: f64) {
        if y < 68.0 || y > viewport_height {
            return;
        }
        self.context.set_fill_style_str("#78879e");
        self.context.fill_rect(x, y, width, 1.0);
    }

    fn draw_lines(
        &mut self,
        text: &str,
        x: f64,
        y: f64,
        scale: f32,
        line_height: f64,
        viewport_height: f64,
    ) -> f64 {
        let mut used = 0.0;
        for line in text.split('\n') {
            let baseline = y + used;
            let visible = baseline > 62.0 && baseline < viewport_height + line_height;
            self.record_line(line, x, baseline, line_height, visible);
            used += if visible {
                self.draw_rich_line(line, x, baseline, scale, line_height, viewport_height)
            } else {
                self.measure_rich_line(line, scale, line_height)
            };
        }
        used.max(line_height)
    }

    fn draw_code_block(
        &mut self,
        source: &str,
        language: Option<&str>,
        y: f64,
        width: f64,
        viewport_height: f64,
    ) -> f64 {
        let gutter = content_gutter(width as f32) as f64;
        let code_x = gutter + 14.0;
        let line_height = 22.0 * self.font_scale;
        let lines = source.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let total_height = 38.0 * self.font_scale + lines as f64 * line_height;
        if y + total_height > 68.0 && y < viewport_height {
            self.context.set_fill_style_str("#0e1a25");
            self.context.fill_rect(
                gutter,
                y - 14.0,
                (width - gutter * 2.0).max(0.0),
                total_height,
            );
        }
        self.set_font(
            15.0,
            "\"Streamdown Noto Sans Mono\", \"Streamdown Noto Sans\", monospace",
        );
        if let Some(language) = language
            && y > 62.0
            && y < viewport_height
        {
            self.context.set_fill_style_str("#73e09e");
            let _ = self
                .context
                .fill_text(language, code_x, y + 4.0 * self.font_scale);
        }

        let context = self.context.clone();
        let mut cursor_x = code_x;
        let mut cursor_y = y + 28.0 * self.font_scale;
        for (index, line) in source.split('\n').enumerate() {
            let baseline = cursor_y + index as f64 * line_height;
            self.record_line(
                line,
                code_x,
                baseline,
                line_height,
                baseline > 62.0 && baseline < viewport_height + line_height,
            );
        }
        code::highlight(source, language, |span, kind| {
            let color = match kind {
                code::TokenKind::Plain => "#d1dbe8",
                code::TokenKind::Keyword => "#c79ef2",
                code::TokenKind::Type => "#40dcdf",
                code::TokenKind::Function => "#73b8fa",
                code::TokenKind::String => "#73e09e",
                code::TokenKind::Number => "#f2ab61",
                code::TokenKind::Comment => "#78879e",
                code::TokenKind::Macro => "#ead67a",
                code::TokenKind::Operator => "#adbccc",
            };
            context.set_fill_style_str(color);
            for part in span.split_inclusive('\n') {
                let text = part.strip_suffix('\n').unwrap_or(part);
                if cursor_y > 62.0 && cursor_y < viewport_height + line_height {
                    let _ = context.fill_text(text, cursor_x, cursor_y);
                }
                cursor_x += context
                    .measure_text(text)
                    .map_or(0.0, |metrics| metrics.width());
                if part.ends_with('\n') {
                    cursor_x = code_x;
                    cursor_y += line_height;
                }
            }
            cursor_y <= viewport_height + line_height
        });
        total_height + 10.0 * self.font_scale
    }

    fn set_font(&self, size: f64, family: &str) {
        self.context
            .set_font(&format!("{:.1}px {family}", size * self.font_scale));
    }

    fn fade_opacity(&self, born_at: f64) -> f64 {
        if self.fade_ms <= 0.0 {
            return 1.0;
        }
        let t = ((self.last_time - born_at) / self.fade_ms).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    fn cached_math(&mut self, source: &str, display: bool, scale: f32) -> Option<Rc<MathImage>> {
        let scale_key = (scale.clamp(0.5, 4.0) * 64.0).round() as u16;
        let key = (source.to_owned(), display, scale_key);
        if let Some(image) = self.math_images.get(&key) {
            return Some(image.clone());
        }
        match math::rasterize(source, display, scale) {
            Ok(image) => {
                self.math_images.insert(key, image.clone());
                Some(image)
            }
            Err(error) => {
                web_sys::console::warn_1(&error.into());
                None
            }
        }
    }

    fn draw_display_math(&mut self, source: &str, x: f64, y: f64, viewport_height: f64) -> f64 {
        let Some(image) = self.cached_math(source, true, self.font_scale as f32) else {
            self.set_font(16.0, "\"Streamdown Noto Sans\"");
            self.context.set_fill_style_str("#d1dbe8");
            let _ = self.context.fill_text(source, x, y + 18.0);
            return 32.0 * self.font_scale;
        };
        if y + image.height as f64 > 62.0 && y < viewport_height {
            self.draw_math_image(&image, x, y);
        }
        image.height as f64 + 12.0
    }

    fn measure_rich_line(&mut self, line: &str, scale: f32, line_height: f64) -> f64 {
        if !line.as_bytes().contains(&b'$') {
            return line_height;
        }
        let mut occupied = line_height;
        for (source, display) in math_spans(line) {
            if let Some(image) = self.cached_math(source, display, scale) {
                occupied = occupied.max(image.height as f64 + 4.0);
            }
        }
        occupied
    }

    fn draw_rich_line(
        &mut self,
        line: &str,
        x: f64,
        baseline: f64,
        scale: f32,
        line_height: f64,
        viewport_height: f64,
    ) -> f64 {
        let mut cursor = x;
        let mut occupied = line_height;
        let mut rest = line;
        while let Some(open) = rest.find('$') {
            let before = &rest[..open];
            let _ = self.context.fill_text(before, cursor, baseline);
            cursor += self
                .context
                .measure_text(before)
                .map_or(0.0, |metrics| metrics.width());

            let display = rest[open..].starts_with("$$");
            let delimiter = if display { "$$" } else { "$" };
            let start = open + delimiter.len();
            let Some(close) = rest[start..].find(delimiter).map(|at| start + at) else {
                let _ = self.context.fill_text(&rest[open..], cursor, baseline);
                return occupied;
            };
            let source = &rest[start..close];
            if let Some(image) = self.cached_math(source, display, scale) {
                let image_y = baseline - image.baseline as f64;
                if image_y + image.height as f64 > 62.0 && image_y < viewport_height {
                    self.draw_math_image(&image, cursor, image_y);
                }
                cursor += image.width as f64 + 2.0;
                occupied = occupied.max(image.height as f64 + 4.0);
            } else {
                let fallback = &rest[open..close + delimiter.len()];
                let _ = self.context.fill_text(fallback, cursor, baseline);
                cursor += self
                    .context
                    .measure_text(fallback)
                    .map_or(0.0, |metrics| metrics.width());
            }
            rest = &rest[close + delimiter.len()..];
        }
        let _ = self.context.fill_text(rest, cursor, baseline);
        occupied
    }

    fn draw_math_image(&self, image: &MathImage, x: f64, y: f64) {
        self.context.save();
        for run in &image.runs {
            self.context.set_fill_style_str(&format!(
                "rgba({},{},{},{:.3})",
                run.rgba[0],
                run.rgba[1],
                run.rgba[2],
                run.rgba[3] as f64 / 255.0
            ));
            self.context
                .fill_rect(x + run.x as f64, y + run.y as f64, run.width as f64, 1.0);
        }
        self.context.restore();
    }
}

fn plain(nodes: &[Inline]) -> String {
    let mut output = String::new();
    for node in nodes {
        match node {
            Inline::Text(text) | Inline::Code(text) => output.push_str(text),
            Inline::Math { source, display } => {
                let delimiter = if *display { "$$" } else { "$" };
                output.push_str(delimiter);
                output.push_str(source);
                output.push_str(delimiter);
            }
            Inline::Emphasis(children) | Inline::Strong(children) => {
                output.push_str(&plain(children));
            }
            Inline::Link { label, .. } => output.push_str(&plain(label)),
            Inline::SoftBreak => output.push(' '),
            Inline::HardBreak => output.push('\n'),
        }
    }
    output
}

fn math_spans(mut text: &str) -> Vec<(&str, bool)> {
    let mut spans = Vec::new();
    while let Some(open) = text.find('$') {
        let display = text[open..].starts_with("$$");
        let delimiter = if display { "$$" } else { "$" };
        let start = open + delimiter.len();
        let Some(close) = text[start..].find(delimiter).map(|at| start + at) else {
            break;
        };
        spans.push((&text[start..close], display));
        text = &text[close + delimiter.len()..];
    }
    spans
}

fn touch_client_y(event: &web_sys::Event) -> Option<f64> {
    let touches = Reflect::get(event.as_ref(), &JsValue::from_str("touches")).ok()?;
    let first = Reflect::get(&touches, &JsValue::from_f64(0.0)).ok()?;
    Reflect::get(&first, &JsValue::from_str("clientY"))
        .ok()?
        .as_f64()
}

fn request_redraw(state: &Rc<RefCell<State>>, font_redraw_marker: bool) {
    let mut state = state.borrow_mut();
    state.dirty = true;
    if font_redraw_marker {
        let _ = state
            .canvas
            .set_attribute("data-canvas-font-redraw", "true");
    }
}

fn schedule_font_redraw(state: &Rc<RefCell<State>>, delay_ms: i32) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let redraw_state = state.clone();
    let callback = Closure::<dyn FnMut()>::new(move || {
        request_redraw(&redraw_state, false);
    });
    if window
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            delay_ms,
        )
        .is_ok()
    {
        callback.forget();
    }
}

fn schedule_font_redraw_marked(state: &Rc<RefCell<State>>, delay_ms: i32) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let redraw_state = state.clone();
    let callback = Closure::<dyn FnMut()>::new(move || {
        request_redraw(&redraw_state, true);
    });
    if window
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            delay_ms,
        )
        .is_ok()
    {
        callback.forget();
    }
}

fn install_font_redraw(state: &Rc<RefCell<State>>) {
    // Timed redraws cover older WebKit implementations without FontFaceSet
    // and pages where a CSS font becomes available after the first frame.
    schedule_font_redraw(state, 250);
    schedule_font_redraw_marked(state, 1_500);

    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let Ok(fonts) = Reflect::get(document.as_ref(), &JsValue::from_str("fonts")) else {
        return;
    };
    if fonts.is_null() || fonts.is_undefined() {
        return;
    }

    // Modern browsers emit loadingdone when late webfont loads complete.
    if let Ok(target) = fonts.clone().dyn_into::<web_sys::EventTarget>() {
        let redraw_state = state.clone();
        let callback = Closure::<dyn FnMut()>::new(move || {
            request_redraw(&redraw_state, true);
        });
        if target
            .add_event_listener_with_callback("loadingdone", callback.as_ref().unchecked_ref())
            .is_ok()
        {
            callback.forget();
        }
    }

    // `fonts.ready` also handles fonts that were already loading before this
    // renderer installed its loadingdone listener. Feature-detect it so old
    // Safari/embedded webviews simply use the timed fallback above.
    if let Ok(ready) = Reflect::get(&fonts, &JsValue::from_str("ready"))
        && ready.is_instance_of::<Promise>()
    {
        let promise: Promise = ready.unchecked_into();
        let redraw_state = state.clone();
        spawn_local(async move {
            let _ = JsFuture::from(promise).await;
            request_redraw(&redraw_state, true);
        });
    }
}

fn logical_width(canvas: &HtmlCanvasElement) -> f64 {
    canvas.client_width().max(1) as f64
}

fn logical_height(canvas: &HtmlCanvasElement) -> f64 {
    canvas.client_height().max(1) as f64
}

fn requested_device_pixel_ratio() -> f64 {
    web_sys::window()
        .map(|window| window.device_pixel_ratio())
        .filter(|ratio| ratio.is_finite() && *ratio > 0.0)
        .unwrap_or(1.0)
}

fn resize(canvas: &HtmlCanvasElement, context: &CanvasRenderingContext2d) -> bool {
    let logical_width = logical_width(canvas);
    let logical_height = logical_height(canvas);
    let backing = backing_size(
        logical_width,
        logical_height,
        requested_device_pixel_ratio(),
    );
    let changed = canvas.width() != backing.width || canvas.height() != backing.height;
    if canvas.width() != backing.width {
        canvas.set_width(backing.width);
    }
    if canvas.height() != backing.height {
        canvas.set_height(backing.height);
    }

    // Assigning width/height resets the Canvas2D state. Reapply an absolute
    // transform every frame so DPR changes, browser zoom and monitor moves do
    // not accumulate scale or leave a stale transform.
    let _ = context.set_transform(backing.scale_x, 0.0, 0.0, backing.scale_y, 0.0, 0.0);
    if changed {
        let _ = canvas.set_attribute(
            "data-canvas-backing-scale",
            &format!("{:.3}", backing.scale_x.min(backing.scale_y)),
        );
    }
    changed
}
