//! The page around the canvas: the panel, the search, the labels. Text goes in as text, never as
//! markup: names and source come from the project.

use crate::graph::{Model, Related};
use crate::highlight;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, HtmlElement};

/// How many labels the graph shows at once.
pub const LABELS: usize = 48;

pub struct Ui {
    document: Document,
    panel: HtmlElement,
    name: Element,
    meta: Element,
    lists: Element,
    source: Element,
    results: Element,
    counts: Element,
    labels: Vec<HtmlElement>,
}

fn by_id(document: &Document, id: &str) -> Result<Element, JsValue> {
    document
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str(&format!("the page has no #{id}")))
}

fn html(element: Element) -> Result<HtmlElement, JsValue> {
    element.dyn_into().map_err(JsValue::from)
}

impl Ui {
    pub fn new(document: Document) -> Result<Self, JsValue> {
        let holder = by_id(&document, "labels")?;
        let mut labels = Vec::with_capacity(LABELS);
        for _ in 0..LABELS {
            let label = document.create_element("div")?;
            holder.append_child(&label)?;
            labels.push(html(label)?);
        }
        Ok(Self {
            panel: html(by_id(&document, "panel")?)?,
            name: by_id(&document, "name")?,
            meta: by_id(&document, "meta")?,
            lists: by_id(&document, "lists")?,
            source: by_id(&document, "source")?,
            results: by_id(&document, "results")?,
            counts: by_id(&document, "counts")?,
            labels,
            document,
        })
    }

    pub fn show_counts(&self, model: &Model) {
        let snapshot = &model.snapshot;
        let state = if snapshot.provisional {
            "resolving"
        } else {
            "live"
        };
        self.counts.set_text_content(Some(&format!(
            "{} symbols, {} edges, {state}",
            snapshot.names.len(),
            snapshot.edges.len()
        )));
    }

    pub fn show_lost(&self) {
        self.counts
            .set_text_content(Some("Lost the connection; trying again"));
    }

    /// A clickable row for a symbol. The click is handled on the page as a whole.
    fn item(&self, model: &Model, symbol: u32) -> Result<Element, JsValue> {
        let item = self.document.create_element("div")?;
        item.set_class_name("item");
        item.set_attribute("data-symbol", &symbol.to_string())?;
        item.set_text_content(Some(model.name_of(symbol)));
        let file = self.document.create_element("small")?;
        file.set_text_content(Some(model.file_of(symbol)));
        item.append_child(&file)?;
        Ok(item)
    }

    pub fn show_results(&self, model: &Model, found: &[u32]) -> Result<(), JsValue> {
        self.results.set_text_content(None);
        for symbol in found {
            let item = self.item(model, *symbol)?;
            self.results.append_child(&item)?;
        }
        Ok(())
    }

    fn show_list(&self, model: &Model, heading: &str, symbols: &[u32]) -> Result<(), JsValue> {
        if symbols.is_empty() {
            return Ok(());
        }
        let title = self.document.create_element("div")?;
        title.set_class_name("heading");
        title.set_text_content(Some(&format!("{heading} ({})", symbols.len())));
        self.lists.append_child(&title)?;
        for symbol in symbols {
            let item = self.item(model, *symbol)?;
            self.lists.append_child(&item)?;
        }
        Ok(())
    }

    pub fn show_symbol(
        &self,
        model: &Model,
        symbol: u32,
        related: &Related,
    ) -> Result<(), JsValue> {
        self.panel.style().set_property("display", "block")?;
        self.name.set_text_content(Some(model.name_of(symbol)));
        let line = model
            .snapshot
            .lines
            .get(symbol as usize)
            .copied()
            .unwrap_or(0);
        self.meta
            .set_text_content(Some(&format!("{}:{line}", model.file_of(symbol))));
        self.source.set_text_content(None);
        self.lists.set_text_content(None);
        self.show_list(model, "Called by", &related.called_by)?;
        self.show_list(model, "Calls", &related.calls)?;
        self.show_list(model, "Possibly", &related.possibly)?;
        self.show_list(model, "Used by", &related.used_by)?;
        self.show_list(model, "Uses", &related.uses)
    }

    fn add_span(&self, class: &str, text: &str) -> Result<(), JsValue> {
        let span = self.document.create_element("span")?;
        span.set_class_name(class);
        span.set_text_content(Some(text));
        self.source.append_child(&span)?;
        Ok(())
    }

    fn add_line_number(&self, line: usize) -> Result<(), JsValue> {
        self.add_span("ln", &format!("{line:>4}  "))
    }

    /// The source, coloured, with its lines numbered.
    pub fn show_source(&self, first_line: usize, text: &str) -> Result<(), JsValue> {
        self.source.set_text_content(None);
        let mut line = first_line;
        self.add_line_number(line)?;
        for (class, piece) in highlight::pieces(text) {
            let style = class.style().unwrap_or("p");
            for (place, part) in piece.split('\n').enumerate() {
                if place > 0 {
                    self.source.append_with_str_1("\n")?;
                    line += 1;
                    self.add_line_number(line)?;
                }
                if !part.is_empty() {
                    self.add_span(style, part)?;
                }
            }
        }
        Ok(())
    }

    pub fn hide_symbol(&self) -> Result<(), JsValue> {
        self.panel.style().set_property("display", "none")
    }

    /// Put labels at these places on the screen, and hide the rest.
    pub fn show_labels(&self, shown: &[(f64, f64, &str)]) -> Result<(), JsValue> {
        for (place, label) in self.labels.iter().enumerate() {
            let style = label.style();
            match shown.get(place) {
                Some((x, y, text)) => {
                    label.set_text_content(Some(text));
                    style.set_property("left", &format!("{x}px"))?;
                    style.set_property("top", &format!("{y}px"))?;
                    style.set_property("display", "block")?;
                }
                None => style.set_property("display", "none")?,
            }
        }
        Ok(())
    }
}
