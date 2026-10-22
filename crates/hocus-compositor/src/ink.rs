//! Drawing on the screen (#86): Super and a left drag leave a felt-tip
//! line over everything, each screen on a sheet of its own; no client sees
//! the button. When Super is let go the drawing fades, then the sheets go.

use std::collections::BTreeMap;

use hocus_shell::ink::Ink;
use hocus_ui::motion::{Animated, Curve, Easing, LARGE};
use smithay::{
    output::Output,
    utils::{Logical, Point},
};

use crate::{State, grabs::BTN_LEFT, scene::SceneElement, shell::Panel};

impl crate::shell::Pixels for Ink {
    fn pixels(&self) -> hocus_shell::column::View<'_> {
        self.view()
    }
}

#[derive(Debug, Default)]
pub struct Inking {
    /// By output name.
    sheets: BTreeMap<String, Panel<Ink>>,
    /// The line being drawn: its screen and where it is, from the
    /// screen's corner.
    drawing: Option<(String, Point<f64, Logical>)>,
    /// Once Super is let go: the drawing's opacity.
    fade: Option<Animated>,
}

impl State {
    /// A button: Super and the left button start a line, which the
    /// release ends; neither reaches a client.
    pub fn ink_button(&mut self, at: Point<f64, Logical>, button: u32, pressed: bool) -> bool {
        if button != BTN_LEFT {
            return false;
        }
        if !pressed {
            return self.ink.drawing.take().is_some();
        }
        let logo = self
            .seat
            .get_keyboard()
            .is_some_and(|k| k.modifier_state().logo);
        if !logo || self.locked() || self.modal_open() {
            return false;
        }
        let Some(output) = self.output_at(at) else {
            return false;
        };
        let Some(area) = self.space.output_geometry(&output) else {
            return false;
        };
        // A new drawing once the last one faded, or while it fades.
        if self.ink.fade.take().is_some() {
            self.ink.sheets.clear();
        }
        let name = output.name();
        if !self.ink.sheets.contains_key(&name) {
            let scale = output.current_scale().fractional_scale() as f32;
            let size = hocus_ui::Size::new(area.size.w as f32, area.size.h as f32);
            let mut sheet = Panel::new(Ink::new(self.theme(), size, scale));
            sheet.upload(&[]);
            self.ink.sheets.insert(name.clone(), sheet);
        }
        let local = at - area.loc.to_f64();
        self.ink.drawing = Some((name, local));
        // A dot where it starts.
        self.ink_to(at);
        true
    }

    /// The pointer moved: the line follows.
    pub fn ink_motion(&mut self, at: Point<f64, Logical>) {
        if self.ink.drawing.is_some() {
            self.ink_to(at);
        }
    }

    fn ink_to(&mut self, at: Point<f64, Logical>) {
        let Some((name, from)) = self.ink.drawing.clone() else {
            return;
        };
        let Some(origin) = self
            .find_output(&name)
            .and_then(|o| self.space.output_geometry(&o))
            .map(|g| g.loc.to_f64())
        else {
            return;
        };
        let to = at - origin;
        if let Some(sheet) = self.ink.sheets.get_mut(&name) {
            let p = |p: Point<f64, Logical>| hocus_ui::Point::new(p.x as f32, p.y as f32);
            sheet.view.line(p(from), p(to));
        }
        self.ink.drawing = Some((name, to));
        self.needs_redraw = true;
    }

    /// Super let go: the drawing fades.
    pub fn ink_lift(&mut self) {
        if self.ink.sheets.is_empty() || self.ink.fade.is_some() {
            return;
        }
        let mut fade = Animated::new(1.0);
        fade.set(0.0, Curve::Ease(Easing::InCubic, LARGE), self.clock.now());
        self.ink.fade = Some(fade);
        self.needs_redraw = true;
    }

    /// After each dispatch: new pieces uploaded, frames while it fades,
    /// and the sheets gone once it has.
    pub fn refresh_ink(&mut self) {
        let now = self.clock.now();
        if let Some(fade) = &self.ink.fade {
            if fade.is_running(now) {
                self.needs_redraw = true;
            } else {
                self.ink = Inking::default();
                self.needs_redraw = true;
                return;
            }
        }
        for sheet in self.ink.sheets.values_mut() {
            let drawn = sheet.view.draw(now);
            if sheet.upload(&drawn) {
                self.needs_redraw = true;
            }
        }
    }

    /// The drawing on `output`, over everything but the cursor.
    pub fn ink_elements<R>(&self, renderer: &mut R, output: &Output) -> Vec<SceneElement<R>>
    where
        R: smithay::backend::renderer::Renderer
            + smithay::backend::renderer::ImportAll
            + smithay::backend::renderer::ImportMem,
        R::TextureId: Send + Clone + smithay::backend::renderer::Texture + 'static,
    {
        if self.locked() {
            return Vec::new();
        }
        let Some(sheet) = self.ink.sheets.get(&output.name()) else {
            return Vec::new();
        };
        let alpha = self
            .ink
            .fade
            .as_ref()
            .map_or(1.0, |f| f.value(self.clock.now()));
        sheet
            .element_with(renderer, output, Point::default(), Some(alpha))
            .map(SceneElement::Frame)
            .into_iter()
            .collect()
    }

    /// The output that `at` is on.
    fn output_at(&self, at: Point<f64, Logical>) -> Option<Output> {
        self.space
            .outputs()
            .find(|o| {
                self.space
                    .output_geometry(o)
                    .is_some_and(|g| g.to_f64().contains(at))
            })
            .cloned()
    }
}
