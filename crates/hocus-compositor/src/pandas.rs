//! The pandas on the desktop (#47). While a video plays fullscreen on one
//! screen, a panda eats popcorn in a corner of the others, looking at it.
//! It moves a moment when it comes, then holds still: no frames at rest,
//! none at all on battery saver. Off in the configuration or from quick
//! settings, none is drawn.

use std::collections::BTreeMap;

use hocus_shell::panda::{Character, Mood, Spot};
use smithay::{
    output::Output,
    reexports::wayland_server::Resource,
    utils::{Logical, Point},
};

use crate::{State, scene::SceneElement, shell::Panel, window_mode::WindowMode};

/// The panda's side and its place from the screen's bottom right corner,
/// logical pixels, as the design has it.
const SIDE: i32 = 170;
const RIGHT: i32 = 18;
const BOTTOM: i32 = 70;

impl crate::shell::Pixels for Spot {
    fn pixels(&self) -> hocus_shell::column::View<'_> {
        self.view()
    }
}

#[derive(Debug, Default)]
pub struct Pandas {
    /// Turned off from quick settings, for this session.
    hidden: bool,
    /// The popcorn panda on each screen but the video's, by output name.
    watching: BTreeMap<String, Panel<Spot>>,
}

impl State {
    pub fn pandas_on(&self) -> bool {
        self.config.pandas.enabled && !self.pandas.hidden
    }

    /// From quick settings: off or back on for the session.
    pub fn toggle_pandas(&mut self) {
        self.pandas.hidden = !self.pandas.hidden;
        self.needs_redraw = true;
    }

    /// The screen a video plays on: a fullscreen window that keeps the
    /// session awake, as players do.
    fn video_output(&self) -> Option<String> {
        let inhibits = |m: &crate::desktops::Managed| {
            m.window.toplevel().is_some_and(|t| {
                let s = t.wl_surface();
                self.protocols
                    .inhibiting
                    .iter()
                    .any(|i| i.is_alive() && i == s)
            })
        };
        let window = self.windows.iter().find(|(id, m)| {
            matches!(m.mode, WindowMode::Fullscreen { .. })
                && self.model.is_visible(**id)
                && inhibits(m)
        })?;
        let at = self.space.element_location(&window.1.window)?;
        self.space
            .outputs()
            .find(|o| {
                self.space
                    .output_geometry(o)
                    .is_some_and(|g| g.contains(at))
            })
            .map(Output::name)
    }

    /// After each dispatch: popcorn pandas come and go with the video, and
    /// draw while they move.
    pub fn refresh_pandas(&mut self) {
        let video = if self.pandas_on() && !self.locked() {
            self.video_output()
        } else {
            None
        };
        let wanted: Vec<Output> = match &video {
            Some(name) => self
                .space
                .outputs()
                .filter(|o| o.name() != *name)
                .cloned()
                .collect(),
            None => Vec::new(),
        };
        let before = self.pandas.watching.len();
        self.pandas
            .watching
            .retain(|name, _| wanted.iter().any(|o| o.name() == *name));
        if self.pandas.watching.len() != before {
            self.needs_redraw = true;
        }
        let now = self.clock.now();
        for output in wanted {
            let name = output.name();
            if !self.pandas.watching.contains_key(&name) {
                // On battery saver it holds still from the start.
                let theme = self
                    .theme()
                    .with_reduced_motion(self.theme().reduced_motion || self.power_saver());
                let scale = output.current_scale().fractional_scale() as f32;
                let spot = Spot::new(
                    theme,
                    self.shell_lang(),
                    Character::Apprentice,
                    Mood::Popcorn,
                    SIDE as f32,
                    scale,
                    now,
                );
                self.pandas.watching.insert(name.clone(), Panel::new(spot));
            }
            if let Some(panel) = self.pandas.watching.get_mut(&name) {
                let drawn = panel.view.draw(now);
                if panel.upload(&drawn) || panel.view.is_moving(now) {
                    self.needs_redraw = true;
                }
            }
        }
    }

    /// Where the popcorn panda sits on `output`, from its corner.
    fn panda_place(&self, output: &Output) -> Option<Point<i32, Logical>> {
        let g = self.space.output_geometry(output)?;
        Some(Point::from((
            g.size.w - SIDE - RIGHT,
            g.size.h - SIDE - BOTTOM,
        )))
    }

    pub fn panda_elements<R>(&self, renderer: &mut R, output: &Output) -> Vec<SceneElement<R>>
    where
        R: smithay::backend::renderer::Renderer
            + smithay::backend::renderer::ImportAll
            + smithay::backend::renderer::ImportMem,
        R::TextureId: Send + Clone + smithay::backend::renderer::Texture + 'static,
    {
        let (Some(panel), Some(place)) = (
            self.pandas.watching.get(&output.name()),
            self.panda_place(output),
        ) else {
            return Vec::new();
        };
        panel
            .element(renderer, output, place)
            .map(SceneElement::Frame)
            .into_iter()
            .collect()
    }

    pub fn pandas_ui(&self, out: &mut Vec<hocus_api::UiNode>) {
        for (name, panel) in &self.pandas.watching {
            let Some(output) = self.find_output(name) else {
                continue;
            };
            let (Some(g), Some(place)) = (
                self.space.output_geometry(&output),
                self.panda_place(&output),
            ) else {
                continue;
            };
            crate::shell::flatten(&panel.view.access(), g.loc + place, 0, out);
        }
    }
}
