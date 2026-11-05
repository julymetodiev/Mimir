//! The page's state and its events: the socket, the mouse, the frame loop.

use crate::events::{connect, listen, load_static};
use crate::gpu::{Camera, Gpu};
use crate::graph::{Model, Places};
use crate::ui::{LABELS, Ui};
use latt_view_wire::{ClientMessage, ServerMessage, Snapshot};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{HtmlCanvasElement, WebSocket};

/// Frames the layout moves after a snapshot, before the GPU may rest.
const SETTLE_FRAMES: u32 = 900;
/// Frames drawn after anything else happens, so that it can finish.
const AWAKE_FRAMES: u32 = 150;
/// Frames between reads of the places, for picking.
const READ_EVERY: u32 = 45;
const CLOSEST_PICK_PIXELS: f64 = 10.0;
/// From this zoom on, the names of what is near the middle show.
const NAMES_FROM_ZOOM: f32 = 6.0;
/// Labels closer than this would be written over each other.
const LABEL_WIDTH: f64 = 110.0;
const LABEL_HEIGHT: f64 = 15.0;

pub struct App {
    gpu: Gpu,
    pub ui: Ui,
    pub canvas: HtmlCanvasElement,
    pub socket: Option<WebSocket>,
    /// A page with no server has every symbol's source already.
    pub sources: Option<HashMap<u64, (u32, String)>>,
    pub model: Model,
    places: Places,
    flags: Vec<u32>,
    chosen: Option<u32>,
    related: Vec<u32>,
    pub hovered: Option<u32>,
    camera: Camera,
    pub wanted: Camera,
    settle: u32,
    awake: u32,
    frame: u32,
    pub drag: Option<(f64, f64)>,
    /// How far the press now going on or just ended has moved.
    pub dragged: f64,
    fitted: bool,
}

pub fn log(error: &JsValue) {
    web_sys::console::error_1(error);
}

pub fn window() -> Result<web_sys::Window, JsValue> {
    web_sys::window().ok_or_else(|| JsValue::from_str("no window"))
}

impl App {
    fn size(&self) -> (f64, f64) {
        let rect = self.canvas.get_bounding_client_rect();
        (rect.width().max(1.0), rect.height().max(1.0))
    }

    /// A place on the screen as a place in the graph, seen from `camera`.
    pub fn to_world(&self, x: f64, y: f64, camera: Camera) -> (f32, f32) {
        let (width, height) = self.size();
        let aspect = (width / height) as f32;
        let across = (x / width * 2.0 - 1.0) as f32;
        let up = (1.0 - y / height * 2.0) as f32;
        (
            across * aspect.max(1.0) / camera.zoom + camera.x,
            up / aspect.min(1.0) / camera.zoom + camera.y,
        )
    }

    fn to_screen(&self, place: [f32; 2]) -> (f64, f64) {
        let (width, height) = self.size();
        let aspect = (width / height) as f32;
        let camera = self.camera;
        let across = (place[0] - camera.x) * camera.zoom / aspect.max(1.0);
        let up = (place[1] - camera.y) * camera.zoom * aspect.min(1.0);
        (
            (f64::from(across) + 1.0) / 2.0 * width,
            (1.0 - f64::from(up)) / 2.0 * height,
        )
    }

    pub fn pick(&self, x: f64, y: f64) -> Option<u32> {
        let mut best = None;
        let mut closest = CLOSEST_PICK_PIXELS * CLOSEST_PICK_PIXELS;
        for (symbol, place) in self.places.at.iter().enumerate() {
            let (px, py) = self.to_screen(*place);
            let distance = (px - x) * (px - x) + (py - y) * (py - y);
            if distance < closest {
                closest = distance;
                best = Some(symbol as u32);
            }
        }
        best
    }

    fn base_flags(model: &Model) -> Vec<u32> {
        model
            .snapshot
            .kinds
            .iter()
            .map(|kind| u32::from(*kind))
            .collect()
    }

    fn apply_focus(&mut self) {
        self.flags = Self::base_flags(&self.model);
        for symbol in &self.related {
            if let Some(flag) = self.flags.get_mut(*symbol as usize) {
                *flag |= 1 << 8;
            }
        }
        if let Some(flag) = self.chosen.and_then(|s| self.flags.get_mut(s as usize)) {
            *flag = (*flag & 255) | 2 << 8;
        }
        self.gpu.set_flags(&self.flags);
    }

    pub fn choose(&mut self, symbol: Option<u32>, fly: bool) -> Result<(), JsValue> {
        self.chosen = symbol;
        self.awake = AWAKE_FRAMES;
        match symbol {
            Some(symbol) => {
                let related = self.model.related(symbol);
                self.ui.show_symbol(&self.model, symbol, &related)?;
                self.related = related.all().collect();
                if let Some(id) = self.model.snapshot.ids.get(symbol as usize) {
                    if let Some((first_line, text)) =
                        self.sources.as_ref().and_then(|sources| sources.get(id))
                    {
                        self.ui.show_source(*first_line as usize, text)?;
                    } else if let Some(socket) = &self.socket
                        && socket.ready_state() == WebSocket::OPEN
                    {
                        socket.send_with_u8_array(&ClientMessage::Source { id: *id }.encode())?;
                    }
                }
                if fly && let Some(place) = self.places.at.get(symbol as usize) {
                    self.wanted = Camera {
                        zoom: self.wanted.zoom.max(NAMES_FROM_ZOOM * 2.0),
                        x: place[0],
                        y: place[1],
                    };
                }
            }
            None => {
                self.related.clear();
                self.ui.hide_symbol()?;
            }
        }
        self.apply_focus();
        Ok(())
    }

    fn take_snapshot(&mut self, snapshot: Snapshot) -> Result<(), JsValue> {
        let model = Model::new(snapshot);
        self.places = model.places_after(&self.model, &self.places);
        let chosen = self
            .chosen
            .and_then(|symbol| model.same_symbol(&self.model, symbol));
        self.model = model;
        self.fit_first_time();
        self.flags = Self::base_flags(&self.model);
        self.gpu.load(
            &self.places.at,
            &self.places.home,
            &self.flags,
            &self.model.snapshot.edges,
        );
        self.ui.show_counts(&self.model);
        self.settle = SETTLE_FRAMES;
        self.choose(chosen, false)
    }

    /// Show the whole graph, the first time there is one.
    fn fit_first_time(&mut self) {
        if self.fitted || self.places.at.is_empty() {
            return;
        }
        self.fitted = true;
        let mut low = [f32::MAX; 2];
        let mut high = [f32::MIN; 2];
        for place in &self.places.at {
            for axis in 0..2 {
                low[axis] = low[axis].min(place[axis]);
                high[axis] = high[axis].max(place[axis]);
            }
        }
        let (width, height) = self.size();
        let aspect = (width / height) as f32;
        let across = (high[0] - low[0]).max(0.001) / aspect.max(1.0);
        let up = (high[1] - low[1]).max(0.001) * aspect.min(1.0);
        self.wanted = Camera {
            zoom: 1.85 / across.max(up),
            x: (low[0] + high[0]) / 2.0,
            y: (low[1] + high[1]) / 2.0,
        };
        self.camera = self.wanted;
    }

    pub fn receive(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        let message =
            ServerMessage::decode(bytes).map_err(|e| JsValue::from_str(&e.to_string()))?;
        match message {
            ServerMessage::Snapshot(snapshot) => self.take_snapshot(snapshot)?,
            ServerMessage::Activity { symbols, edit } => {
                for id in symbols {
                    if let Some(symbol) = self.model.place_of(id) {
                        self.gpu.glow(symbol, if edit { -1.0 } else { 1.0 });
                    }
                }
                self.awake = AWAKE_FRAMES;
            }
            ServerMessage::Source {
                id,
                first_line,
                text,
            } => {
                let chosen = self
                    .chosen
                    .and_then(|s| self.model.snapshot.ids.get(s as usize));
                if chosen == Some(&id) {
                    self.ui.show_source(first_line as usize, &text)?;
                }
            }
        }
        Ok(())
    }

    pub fn update_labels(&self) -> Result<(), JsValue> {
        let (width, height) = self.size();
        let mut shown: Vec<u32> = Vec::with_capacity(LABELS);
        shown.extend(self.chosen);
        shown.extend(self.hovered.filter(|h| Some(*h) != self.chosen));
        shown.extend(self.related.iter().take(LABELS - shown.len()));
        if self.chosen.is_none() && self.camera.zoom >= NAMES_FROM_ZOOM {
            let mut near: Vec<(f64, u32)> = self
                .places
                .at
                .iter()
                .enumerate()
                .filter_map(|(symbol, place)| {
                    let (x, y) = self.to_screen(*place);
                    let seen = (0.0..width).contains(&x) && (0.0..height).contains(&y);
                    let (dx, dy) = (x - width / 2.0, y - height / 2.0);
                    seen.then_some((dx * dx + dy * dy, symbol as u32))
                })
                .collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
            shown.extend(
                near.iter()
                    .map(|(_, symbol)| *symbol)
                    .take(LABELS - shown.len()),
            );
        }
        let mut labels: Vec<(f64, f64, &str)> = Vec::with_capacity(shown.len());
        for symbol in shown {
            if let Some(place) = self.places.at.get(symbol as usize) {
                let (x, y) = self.to_screen(*place);
                let free = labels.iter().all(|(other_x, other_y, _)| {
                    (x - other_x).abs() > LABEL_WIDTH || (y - other_y).abs() > LABEL_HEIGHT
                });
                if free {
                    labels.push((x, y, self.model.name_of(symbol)));
                }
            }
        }
        self.ui.show_labels(&labels)
    }

    fn fit_canvas(&mut self) -> Result<(), JsValue> {
        let ratio = window()?.device_pixel_ratio();
        let (width, height) = self.size();
        let (width, height) = ((width * ratio) as u32, (height * ratio) as u32);
        if self.canvas.width() != width || self.canvas.height() != height {
            self.canvas.set_width(width);
            self.canvas.set_height(height);
            self.gpu.resize(width, height);
            self.awake = AWAKE_FRAMES;
        }
        Ok(())
    }

    /// One frame, or nothing if nothing moves: an idle graph does not spend the battery.
    fn tick(&mut self) -> Result<(), JsValue> {
        self.fit_canvas()?;
        if let Some(places) = self.gpu.take_places()
            && places.len() == self.places.at.len()
        {
            self.places.at = places;
        }
        let ease = 0.18;
        let moved = (self.wanted.zoom - self.camera.zoom).abs() / self.camera.zoom > 0.001
            || (self.wanted.x - self.camera.x).abs() * self.camera.zoom > 0.0005
            || (self.wanted.y - self.camera.y).abs() * self.camera.zoom > 0.0005;
        if moved {
            self.camera.zoom += (self.wanted.zoom - self.camera.zoom) * ease;
            self.camera.x += (self.wanted.x - self.camera.x) * ease;
            self.camera.y += (self.wanted.y - self.camera.y) * ease;
            self.awake = self.awake.max(2);
        }
        if self.settle == 0 && self.awake == 0 {
            return Ok(());
        }
        self.frame = self.frame.wrapping_add(1);
        let simulate = self.settle > 0;
        let last_read = self.settle == 1;
        let read = simulate && (self.frame.is_multiple_of(READ_EVERY) || last_read);
        self.gpu
            .frame(self.camera, self.chosen.is_some(), simulate, read);
        self.settle = self.settle.saturating_sub(1);
        self.awake = self.awake.saturating_sub(1);
        if self.frame.is_multiple_of(6) || self.awake == 0 {
            self.update_labels()?;
        }
        Ok(())
    }
}

pub type Shared = Rc<RefCell<App>>;

fn run_frames(app: Shared) -> Result<(), JsValue> {
    type Frame = Option<Closure<dyn FnMut()>>;
    let handle: Rc<RefCell<Frame>> = Rc::new(RefCell::new(None));
    let next = handle.clone();
    *handle.borrow_mut() = Some(Closure::new(move || {
        if let Err(error) = app.borrow_mut().tick() {
            log(&error);
        }
        let again = next.borrow();
        if let (Some(frame), Ok(window)) = (again.as_ref(), window())
            && let Err(error) = window.request_animation_frame(frame.as_ref().unchecked_ref())
        {
            log(&error);
        }
    }));
    let first = handle.borrow();
    if let Some(frame) = first.as_ref() {
        window()?.request_animation_frame(frame.as_ref().unchecked_ref())?;
    }
    Ok(())
}

/// Start the page.
#[wasm_bindgen]
pub async fn start() -> Result<(), JsValue> {
    let window = window()?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let canvas: HtmlCanvasElement = document
        .get_element_by_id("canvas")
        .ok_or_else(|| JsValue::from_str("no canvas"))?
        .dyn_into()?;
    let ratio = window.device_pixel_ratio();
    canvas.set_width((f64::from(canvas.client_width()) * ratio) as u32);
    canvas.set_height((f64::from(canvas.client_height()) * ratio) as u32);
    let gpu = Gpu::new(canvas.clone()).await?;
    let model = Model::new(Snapshot::default());
    let places = model.first_places();
    let camera = Camera {
        zoom: 1.0,
        x: 0.0,
        y: 0.0,
    };
    let app = Rc::new(RefCell::new(App {
        gpu,
        ui: Ui::new(document.clone())?,
        canvas: canvas.clone(),
        socket: None,
        sources: None,
        model,
        places,
        flags: Vec::new(),
        chosen: None,
        related: Vec::new(),
        hovered: None,
        camera,
        wanted: camera,
        settle: 0,
        awake: 0,
        frame: 0,
        drag: None,
        dragged: 0.0,
        fitted: false,
    }));
    listen(&app, &canvas, &document)?;
    let is_static = document.query_selector("meta[name=latt-static]")?.is_some();
    if is_static {
        load_static(&app).await?;
    } else {
        connect(&app)?;
    }
    run_frames(app)
}
