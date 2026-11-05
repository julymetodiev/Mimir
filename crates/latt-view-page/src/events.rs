//! What the page listens to: the socket, the mouse, the keyboard and the search.

use crate::app::{App, Shared, log, window};
use latt_view_wire::decode_sources;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use web_sys::{BinaryType, HtmlCanvasElement, HtmlInputElement, WebSocket};

const MOST_RESULTS: usize = 12;
/// A press that moved further than this was a drag, and its release is no click.
const DRAG_PIXELS: f64 = 4.0;

/// Run a handler on the page's state, reporting what it fails with.
fn on<E: JsCast + 'static>(
    target: &web_sys::EventTarget,
    event: &str,
    app: &Shared,
    handler: impl Fn(&mut App, E) -> Result<(), JsValue> + 'static,
) -> Result<(), JsValue> {
    let app = app.clone();
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        if let Ok(event) = event.dyn_into::<E>()
            && let Err(error) = handler(&mut app.borrow_mut(), event)
        {
            log(&error);
        }
    });
    target.add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())?;
    closure.forget();
    Ok(())
}

async fn fetched(path: &str) -> Result<Vec<u8>, JsValue> {
    let response = JsFuture::from(window()?.fetch_with_str(path)).await?;
    let response: web_sys::Response = response.dyn_into()?;
    if !response.ok() {
        return Err(JsValue::from_str(&format!("cannot load {path}")));
    }
    let bytes = JsFuture::from(response.array_buffer()?).await?;
    Ok(js_sys::Uint8Array::new(&bytes).to_vec())
}

/// Take the graph and the source from files, for a page with no server.
pub async fn load_static(app: &Shared) -> Result<(), JsValue> {
    let graph = fetched("graph.bin").await?;
    let sources = fetched("sources.bin").await?;
    let sources = decode_sources(&sources).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let mut app = app.borrow_mut();
    app.sources = Some(
        sources
            .into_iter()
            .map(|(id, first_line, text)| (id, (first_line, text)))
            .collect(),
    );
    app.receive(&graph)?;
    app.ui.show_static();
    Ok(())
}

pub fn connect(app: &Shared) -> Result<(), JsValue> {
    let location = window()?.location();
    let address = format!("ws://{}{}socket", location.host()?, location.pathname()?);
    let socket = WebSocket::new(&address)?;
    socket.set_binary_type(BinaryType::Arraybuffer);
    let target: &web_sys::EventTarget = socket.as_ref();
    on(
        target,
        "message",
        app,
        |app, event: web_sys::MessageEvent| {
            let bytes = js_sys::Uint8Array::new(&event.data()).to_vec();
            app.receive(&bytes)
        },
    )?;
    let again = app.clone();
    on(target, "close", app, move |app, _: web_sys::Event| {
        app.ui.show_lost();
        let again = again.clone();
        let retry = Closure::once_into_js(move || {
            if let Err(error) = connect(&again) {
                log(&error);
            }
        });
        window()?
            .set_timeout_with_callback_and_timeout_and_arguments_0(retry.unchecked_ref(), 1000)?;
        Ok(())
    })?;
    app.borrow_mut().socket = Some(socket);
    Ok(())
}

pub fn listen(
    app: &Shared,
    canvas: &HtmlCanvasElement,
    document: &web_sys::Document,
) -> Result<(), JsValue> {
    let target: &web_sys::EventTarget = canvas.as_ref();
    on(target, "wheel", app, |app, event: web_sys::WheelEvent| {
        event.prevent_default();
        let (x, y) = (f64::from(event.offset_x()), f64::from(event.offset_y()));
        let before = app.to_world(x, y, app.wanted);
        let factor = (-event.delta_y() * 0.002).exp() as f32;
        app.wanted.zoom = (app.wanted.zoom * factor).clamp(0.5, 600.0);
        let after = app.to_world(x, y, app.wanted);
        app.wanted.x += before.0 - after.0;
        app.wanted.y += before.1 - after.1;
        Ok(())
    })?;
    on(
        target,
        "pointerdown",
        app,
        |app, event: web_sys::PointerEvent| {
            app.drag = Some((f64::from(event.offset_x()), f64::from(event.offset_y())));
            app.dragged = 0.0;
            app.canvas.set_pointer_capture(event.pointer_id())
        },
    )?;
    on(
        target,
        "pointermove",
        app,
        |app, event: web_sys::PointerEvent| {
            let (x, y) = (f64::from(event.offset_x()), f64::from(event.offset_y()));
            if let Some((last_x, last_y)) = app.drag {
                let from = app.to_world(last_x, last_y, app.wanted);
                let to = app.to_world(x, y, app.wanted);
                app.wanted.x += from.0 - to.0;
                app.wanted.y += from.1 - to.1;
                app.dragged += (x - last_x).hypot(y - last_y);
                app.drag = Some((x, y));
                return Ok(());
            }
            let hovered = app.pick(x, y);
            if hovered != app.hovered {
                app.hovered = hovered;
                app.update_labels()?;
            }
            Ok(())
        },
    )?;
    on(target, "pointerup", app, |app, _: web_sys::PointerEvent| {
        app.drag = None;
        Ok(())
    })?;
    on(target, "click", app, |app, event: web_sys::MouseEvent| {
        if app.dragged > DRAG_PIXELS {
            return Ok(());
        }
        let symbol = app.pick(f64::from(event.offset_x()), f64::from(event.offset_y()));
        app.choose(symbol, false)
    })?;
    let page: &web_sys::EventTarget = document.as_ref();
    on(page, "click", app, |app, event: web_sys::MouseEvent| {
        let row = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            .and_then(|element| element.closest("[data-symbol]").ok().flatten());
        if let Some(symbol) = row
            .and_then(|row| row.get_attribute("data-symbol"))
            .and_then(|symbol| symbol.parse::<u32>().ok())
        {
            app.ui.show_results(&app.model, &[])?;
            app.choose(Some(symbol), true)?;
        }
        Ok(())
    })?;
    on(
        page,
        "keydown",
        app,
        |app, event: web_sys::KeyboardEvent| {
            if event.key() == "Escape" {
                app.ui.show_results(&app.model, &[])?;
                app.choose(None, false)?;
            }
            Ok(())
        },
    )?;
    let search = document
        .get_element_by_id("search")
        .ok_or_else(|| JsValue::from_str("the page has no search"))?;
    let search_target: &web_sys::EventTarget = search.as_ref();
    on(search_target, "input", app, |app, event: web_sys::Event| {
        let text = event
            .target()
            .and_then(|target| target.dyn_into::<HtmlInputElement>().ok())
            .map(|input| input.value())
            .unwrap_or_default();
        let found = app.model.search(&text, MOST_RESULTS);
        app.ui.show_results(&app.model, &found)
    })
}
