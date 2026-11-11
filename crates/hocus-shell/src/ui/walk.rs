//! What a laid-out tree shows, read by operations: its controls in order
//! with their bounds ([`Walk`]), what tests read of it ([`Walk::json`]),
//! and a list scrolled to one of its rows ([`reveal`]).

use std::collections::BTreeMap;

use iced::{
    Rectangle, Vector,
    advanced::widget::{
        Id, Operation,
        operation::{self, Outcome, Scrollable, scrollable},
    },
};

use super::control::Declared;

/// A tree walked: each container given an id, where it is on the surface
/// (its scrolling taken into account) and the texts in it; each control's
/// declaration and its bounds, in the tree's order.
#[derive(Debug)]
pub struct Walk<M> {
    /// The translation of the scrollables we are in.
    shift: Vec<Vector>,
    next_shift: Option<Vector>,
    /// The ids we are in.
    within: Vec<String>,
    next_id: Option<String>,
    pub places: BTreeMap<String, Rectangle>,
    pub texts: BTreeMap<String, Vec<String>>,
    pub controls: Vec<(Rectangle, Declared<M>)>,
}

impl<M> Default for Walk<M> {
    fn default() -> Self {
        Self {
            shift: Vec::new(),
            next_shift: None,
            within: Vec::new(),
            next_id: None,
            places: BTreeMap::new(),
            texts: BTreeMap::new(),
            controls: Vec::new(),
        }
    }
}

/// The name an id was made from.
fn name(id: &Id) -> Option<String> {
    let said = format!("{id:?}");
    let inner = said.strip_prefix("Id(Custom(\"")?.strip_suffix("\"))")?;
    Some(inner.to_owned())
}

impl<M> Walk<M> {
    fn shift(&self) -> Vector {
        self.shift.last().copied().unwrap_or(Vector::ZERO)
    }

    /// The control holding the ring.
    pub fn focused(&self) -> Option<&Declared<M>> {
        self.controls.iter().map(|(_, d)| d).find(|d| d.focused)
    }

    /// As one line of JSON: `{"id": {"at": [x, y, w, h], "says": […]}}`,
    /// and `"focus"`: the control holding the ring.
    pub fn json(&self) -> String {
        let mut all = serde_json::Map::new();
        for (id, r) in &self.places {
            let says = self.texts.get(id).cloned().unwrap_or_default();
            all.insert(
                id.clone(),
                serde_json::json!({
                    "at": [r.x.round(), r.y.round(), r.width.round(), r.height.round()],
                    "says": says,
                }),
            );
        }
        if let Some(loose) = self.texts.get("") {
            all.insert(String::new(), serde_json::json!({ "says": loose }));
        }
        if let Some(focused) = self.focused() {
            all.insert("focus".to_owned(), focused.node.key.clone().into());
        }
        serde_json::Value::Object(all).to_string()
    }
}

impl<M: Clone + Send + 'static> Operation for Walk<M> {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        let shift = self.next_shift.take();
        let id = self.next_id.take();
        if let Some(by) = shift {
            let total = self.shift() + by;
            self.shift.push(total);
        }
        if let Some(id) = &id {
            self.within.push(id.clone());
        }
        operate(self);
        if id.is_some() {
            self.within.pop();
        }
        if shift.is_some() {
            self.shift.pop();
        }
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        self.next_shift = None;
        self.next_id = id.and_then(name);
        if let Some(name) = &self.next_id {
            let shift = self.shift();
            self.places.insert(name.clone(), bounds - shift);
        }
    }

    fn scrollable(
        &mut self,
        _: Option<&Id>,
        _: Rectangle,
        _: Rectangle,
        translation: Vector,
        _: &mut dyn Scrollable,
    ) {
        self.next_id = None;
        self.next_shift = Some(translation);
    }

    fn custom(&mut self, _: Option<&Id>, bounds: Rectangle, state: &mut dyn std::any::Any) {
        if let Some(declared) = state.downcast_ref::<Declared<M>>() {
            self.controls
                .push((bounds - self.shift(), declared.clone()));
        }
    }

    fn text(&mut self, _: Option<&Id>, _: Rectangle, text: &str) {
        let within = self.within.last().cloned().unwrap_or_default();
        self.texts.entry(within).or_default().push(text.to_owned());
    }
}

/// The list `list` scrolled as little as shows the container `target` in
/// it, whole if it fits.
pub fn reveal<T: 'static>(list: Id, target: Id) -> impl Operation<T> {
    struct Reveal {
        list: Id,
        target: Id,
        /// The list's viewport, how far it is scrolled, its content's
        /// height.
        view: Option<(Rectangle, f32, f32)>,
        found: Option<Rectangle>,
    }

    impl<T: 'static> Operation<T> for Reveal {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
            operate(self);
        }

        fn scrollable(
            &mut self,
            id: Option<&Id>,
            bounds: Rectangle,
            content: Rectangle,
            translation: Vector,
            _: &mut dyn Scrollable,
        ) {
            if id == Some(&self.list) {
                self.view = Some((bounds, translation.y, content.height));
            }
        }

        fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
            if id == Some(&self.target) {
                self.found = Some(bounds);
            }
        }

        fn finish(&self) -> Outcome<T> {
            let (Some((view, scroll, content)), Some(row)) = (self.view, self.found) else {
                return Outcome::None;
            };
            let most = (content - view.height).max(0.0);
            let to = crate::popover::kit::reveal(
                (row.y - view.y, row.height),
                view.height,
                scroll,
                most,
            );
            if (to - scroll).abs() < 0.5 {
                return Outcome::None;
            }
            Outcome::Chain(Box::new(scrollable::scroll_to(
                self.list.clone(),
                operation::scrollable::AbsoluteOffset {
                    x: None,
                    y: Some(to),
                },
            )))
        }
    }

    Reveal {
        list,
        target,
        view: None,
        found: None,
    }
}

/// The controls `element` declares, laid out `width` wide, with their
/// bounds: what a surface's tests read of its declarations. Laid out on
/// the CPU's renderer, never a GPU.
#[cfg(test)]
pub fn declared<M: Clone + Send + 'static>(
    element: iced::Element<'_, M>,
    width: f32,
) -> Vec<(Rectangle, Declared<M>)> {
    use iced::{
        Font, Pixels, Size,
        advanced::{Layout, layout::Limits, renderer::Headless, widget::Tree},
    };
    let renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
        Font::default(),
        Pixels(16.0),
        Some("tiny-skia"),
    ))
    .expect("the CPU's renderer");
    let mut element = element;
    let mut tree = Tree::new(&element);
    let node = element.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &Limits::new(Size::ZERO, Size::new(width, f32::INFINITY)),
    );
    let mut walk = Walk::default();
    element
        .as_widget_mut()
        .operate(&mut tree, Layout::new(&node), &renderer, &mut walk);
    walk.controls
}
