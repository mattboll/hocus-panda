//! Where the window's pieces are: every container given an id, in the
//! window's coordinates, scrolling taken into account. The menus open
//! under their buttons from it, a found row is scrolled to from it, and
//! the log says it for tests.

use std::collections::BTreeMap;

use iced::{
    Rectangle, Vector,
    advanced::widget::{
        Id, Operation,
        operation::{Outcome, Scrollable},
    },
};

/// Each id's place on the window, and the page's scrolling: its viewport,
/// its content's height and how far it is scrolled.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Places {
    pub at: BTreeMap<String, Rectangle>,
    pub page: Option<Scroll>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Scroll {
    pub viewport: Rectangle,
    pub content: Rectangle,
    pub offset: f32,
}

/// The page's scrollable's id.
pub const PAGE: &str = "page";

/// The name an id was made from (`Id::from(String)`).
fn name(id: &Id) -> Option<String> {
    let said = format!("{id:?}");
    let inner = said.strip_prefix("Id(Custom(\"")?.strip_suffix("\"))")?;
    Some(inner.to_owned())
}

/// The operation collecting them.
#[derive(Debug, Default)]
pub struct Collect {
    /// The translation of the scrollables we are in.
    shift: Vec<Vector>,
    /// The next traversal is a scrollable's content, moved by this.
    next: Option<Vector>,
    found: Places,
}

impl Collect {
    fn shift(&self) -> Vector {
        self.shift.last().copied().unwrap_or(Vector::ZERO)
    }
}

impl Operation<Places> for Collect {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Places>)) {
        match self.next.take() {
            Some(by) => {
                let total = self.shift() + by;
                self.shift.push(total);
                operate(self);
                self.shift.pop();
            }
            None => operate(self),
        }
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        self.next = None;
        if let Some(name) = id.and_then(name) {
            let shift = self.shift();
            self.found.at.insert(
                name,
                Rectangle {
                    x: bounds.x - shift.x,
                    y: bounds.y - shift.y,
                    ..bounds
                },
            );
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        _: &mut dyn Scrollable,
    ) {
        if id.and_then(name).as_deref() == Some(PAGE) {
            self.found.page = Some(Scroll {
                viewport: bounds,
                content,
                offset: translation.y,
            });
        }
        self.next = Some(translation);
    }

    fn finish(&self) -> Outcome<Places> {
        Outcome::Some(self.found.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ids_name_is_read_back() {
        assert_eq!(
            name(&Id::from("row:power.lid".to_owned())).as_deref(),
            Some("row:power.lid")
        );
        assert_eq!(name(&Id::unique()), None);
    }
}
