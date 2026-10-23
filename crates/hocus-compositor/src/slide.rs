//! Switching desktops slides (#36): on the screen that switches only, a
//! strip of its desktops moves from the one shown to the next, the windows
//! of the desktops passing by drawn where they wait. A three-finger swipe
//! holds the strip under the fingers; let go, it springs to the nearest
//! desktop. Meanwhile the switcher shows the screen's desktops. The model
//! switches at once; only the picture moves, on the frame clock. Reduced
//! motion switches without a slide.

use std::{collections::BTreeMap, time::Duration};

use hocus_core::OutputName;
use hocus_shell::switcher::{Block, Switcher};
use hocus_ui::motion::{Animated, Curve, Easing, STANDARD, Spring};
use smithay::{
    desktop::Window,
    output::Output,
    utils::{Logical, Point, SERIAL_COUNTER},
};

use crate::{
    State, desktops::model_id, scene::SceneElement, shell::Panel, surface::title_and_app_id,
};

/// How long the switcher stays once the strip stopped.
const LINGER: Duration = Duration::from_millis(600);
/// Fingers that swipe desktops.
const FINGERS: u32 = 3;

#[derive(Debug)]
struct Slide {
    /// Where the strip is, in desktops from the first.
    position: Animated,
    /// A swipe holds it: the desktop it began on, and how far the fingers
    /// went, logical pixels.
    swipe: Option<(f32, f64)>,
    switcher: Option<(Point<i32, Logical>, Panel<Switcher>)>,
    /// When the switcher goes.
    until: Duration,
}

#[derive(Debug, Default)]
pub struct Slides {
    by_output: BTreeMap<String, Slide>,
    /// The screen a swipe moves.
    swiping: Option<String>,
}

impl crate::shell::Pixels for Switcher {
    fn pixels(&self) -> hocus_shell::column::View<'_> {
        self.view()
    }
}

/// What an output shows while its strip moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlideView {
    /// The strip's position, in desktops.
    pub position: f32,
    /// The desktop the model shows.
    pub shown: usize,
    /// The output's logical width.
    pub width: f64,
}

impl SlideView {
    /// How far a desktop's windows are drawn from their places.
    pub fn offset(&self, desktop: usize) -> i32 {
        ((desktop as f64 - f64::from(self.position)) * self.width).round() as i32
    }
}

impl State {
    fn moves(&self) -> bool {
        !self.instant_motion && !self.config.appearance.reduced_motion
    }

    /// `output` switched from desktop `from` to `to` (from 0): its strip
    /// slides there from where it is.
    pub fn slide_desktops(&mut self, output: &Output, from: usize, to: usize) {
        if !self.moves() {
            return;
        }
        let now = self.clock.now();
        // The Matrix theme's desktop rains away instead (#155); the
        // switcher shows the same.
        if self.matrix_moves() {
            self.matrix_switch_desktops(output, from, to);
            self.slides.by_output.insert(
                output.name(),
                Slide {
                    position: Animated::new(to as f32),
                    swipe: None,
                    switcher: None,
                    until: now,
                },
            );
            self.show_switcher(output);
            return;
        }
        let slide = self
            .slides
            .by_output
            .entry(output.name())
            .or_insert_with(|| Slide {
                position: Animated::new(from as f32),
                swipe: None,
                switcher: None,
                until: now,
            });
        slide.swipe = None;
        slide
            .position
            .set(to as f32, Curve::Ease(Easing::OutCubic, STANDARD), now);
        self.show_switcher(output);
    }

    /// The switcher above `output`'s bottom edge, as its desktops are now.
    fn show_switcher(&mut self, output: &Output) {
        let name = OutputName(output.name());
        let Some(out) = self.model.output(&name).cloned() else {
            return;
        };
        let Some(area) = self.space.output_geometry(output) else {
            return;
        };
        let (w, h) = (f64::from(area.size.w), f64::from(area.size.h.max(1)));
        let blocks: Vec<Vec<Block>> = out
            .desktops
            .iter()
            .map(|d| {
                self.model
                    .desktop(*d)
                    .map(|desk| desk.windows.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|id| !self.model.is_minimized(*id))
                    .filter_map(|id| {
                        let managed = self.windows.get(&id)?;
                        let size = managed.window.geometry().size;
                        let at = self
                            .space
                            .element_location(&managed.window)
                            .unwrap_or(managed.location)
                            - area.loc;
                        let (_, app_id) = title_and_app_id(&managed.window);
                        Some(Block {
                            x: (f64::from(at.x) / w) as f32,
                            y: (f64::from(at.y) / h) as f32,
                            w: (f64::from(size.w) / w) as f32,
                            h: (f64::from(size.h) / h) as f32,
                            color: hocus_shell::toasts::app_color(&app_id.unwrap_or_default()),
                        })
                    })
                    .collect()
            })
            .collect();
        let shown = out
            .desktops
            .iter()
            .position(|d| *d == out.shown)
            .unwrap_or(0);
        let aspect = (w / h) as f32;
        let switcher = Switcher::new(
            self.theme(),
            self.shell_lang(),
            output.current_scale().fractional_scale() as f32,
            aspect,
            &blocks,
            shown,
        );
        let size = Switcher::size(blocks.len(), aspect);
        let mut panel = Panel::new(switcher);
        panel.upload(&[]);
        let place = Point::from((
            ((w - f64::from(size.w)) / 2.0).round() as i32,
            (h - f64::from(size.h) - 40.0).round() as i32,
        ));
        let now = self.clock.now();
        if let Some(slide) = self.slides.by_output.get_mut(&output.name()) {
            slide.switcher = Some((place, panel));
            slide.until = slide.until.max(now + STANDARD + LINGER);
        }
        self.needs_redraw = true;
    }

    /// Where `output`'s strip is, while it moves or a swipe holds it.
    pub fn slide_view(&self, output: &Output) -> Option<SlideView> {
        let slide = self.slides.by_output.get(&output.name())?;
        let now = self.clock.now();
        if !slide.position.is_running(now) && slide.swipe.is_none() {
            return None;
        }
        let out = self.model.output(&OutputName(output.name()))?;
        let shown = out.desktops.iter().position(|d| *d == out.shown)?;
        let width = f64::from(self.space.output_geometry(output)?.size.w);
        Some(SlideView {
            position: slide.position.value(now),
            shown,
            width,
        })
    }

    /// The windows of the desktops passing by, not the one shown: where
    /// they wait, moved along the strip. Pinned ones never move.
    pub fn slide_extras(
        &self,
        output: &Output,
        view: SlideView,
    ) -> Vec<(Window, Point<i32, Logical>)> {
        let Some(out) = self.model.output(&OutputName(output.name())) else {
            return Vec::new();
        };
        let p = view.position;
        let near = [p.floor() as i64, p.ceil() as i64];
        let mut extras = Vec::new();
        for k in near {
            let Ok(k) = usize::try_from(k) else {
                continue;
            };
            if k == view.shown || (k as f32 - p).abs() >= 1.0 {
                continue;
            }
            let Some(desk) = out.desktops.get(k).and_then(|d| self.model.desktop(*d)) else {
                continue;
            };
            for id in &desk.windows {
                if self.model.is_minimized(*id) || self.model.is_pinned(*id) {
                    continue;
                }
                if let Some(managed) = self.windows.get(id) {
                    let at = managed.location + Point::from((view.offset(k), 0));
                    extras.push((managed.window.clone(), at));
                }
            }
        }
        extras
    }

    /// Whether a window of the shown desktop slides with the strip: on
    /// `output` and not pinned.
    pub fn slides_with(&self, window: &Window, output: &Output) -> bool {
        model_id(window).is_some_and(|id| !self.model.is_pinned(id))
            && self.output_of(window) == Some(output)
    }

    /// The switcher on `output`, while it shows.
    /// The desktop switcher on glass, while it shows on `output`.
    pub(crate) fn switcher_glass(&self, output: &Output) -> Vec<crate::blur::Spot> {
        let Some(slide) = self.slides.by_output.get(&output.name()) else {
            return Vec::new();
        };
        if self.clock.now() >= slide.until && slide.swipe.is_none() {
            return Vec::new();
        }
        slide
            .switcher
            .as_ref()
            .map_or_else(Vec::new, |(place, panel)| {
                crate::blur::spots(
                    crate::blur::Surface::DesktopSwitcher,
                    place.to_f64(),
                    &panel.view.glass(),
                )
            })
    }

    pub fn switcher_elements<R>(&self, renderer: &mut R, output: &Output) -> Vec<SceneElement<R>>
    where
        R: smithay::backend::renderer::Renderer
            + smithay::backend::renderer::ImportAll
            + smithay::backend::renderer::ImportMem,
        R::TextureId: Send + Clone + smithay::backend::renderer::Texture + 'static,
    {
        let Some(slide) = self.slides.by_output.get(&output.name()) else {
            return Vec::new();
        };
        if self.clock.now() >= slide.until && slide.swipe.is_none() {
            return Vec::new();
        }
        slide
            .switcher
            .as_ref()
            .and_then(|(place, panel)| panel.element(renderer, output, *place))
            .map(SceneElement::Frame)
            .into_iter()
            .collect()
    }

    /// After each dispatch: frames while a strip moves or the switcher
    /// shows; a strip at rest is forgotten.
    pub fn refresh_slides(&mut self) {
        let now = self.clock.now();
        self.slides
            .by_output
            .retain(|_, s| s.swipe.is_some() || s.position.is_running(now) || now < s.until);
        if !self.slides.by_output.is_empty() {
            self.needs_redraw = true;
        }
    }

    // Swipes.

    /// Fingers start a swipe on the screen under the pointer; whether the
    /// compositor takes it.
    pub fn swipe_begin(&mut self, fingers: u32) -> bool {
        if fingers != FINGERS || !self.moves() || self.locked() {
            return false;
        }
        let Some(output) = self.output_under_pointer() else {
            return false;
        };
        let Some(out) = self.model.output(&OutputName(output.name())) else {
            return false;
        };
        let Some(shown) = out.desktops.iter().position(|d| *d == out.shown) else {
            return false;
        };
        let now = self.clock.now();
        let slide = self
            .slides
            .by_output
            .entry(output.name())
            .or_insert_with(|| Slide {
                position: Animated::new(shown as f32),
                swipe: None,
                switcher: None,
                until: now,
            });
        // A strip still moving is caught where it is.
        let at = slide.position.value(now);
        slide.position.jump(at);
        slide.swipe = Some((at, 0.0));
        self.slides.swiping = Some(output.name());
        self.show_switcher(&output);
        true
    }

    /// The fingers moved by `dx` logical pixels: the strip follows, a
    /// desktop past the ends at most.
    pub fn swipe_update(&mut self, dx: f64) -> bool {
        let Some(name) = self.slides.swiping.clone() else {
            return false;
        };
        let Some(output) = self.find_output(&name) else {
            return false;
        };
        let width = self
            .space
            .output_geometry(&output)
            .map_or(1.0, |a| f64::from(a.size.w.max(1)));
        let count = self
            .model
            .output(&OutputName(name.clone()))
            .map_or(1, |o| o.desktops.len());
        if let Some(slide) = self.slides.by_output.get_mut(&name)
            && let Some((origin, moved)) = &mut slide.swipe
        {
            *moved += dx;
            // Fingers to the left bring the next desktop.
            let at = (f64::from(*origin) - *moved / width)
                .clamp(f64::from(*origin) - 1.0, f64::from(*origin) + 1.0)
                .clamp(0.0, (count - 1) as f64);
            slide.position.jump(at as f32);
        }
        self.needs_redraw = true;
        true
    }

    /// The fingers left: the strip springs to the nearest desktop, which
    /// is shown.
    pub fn swipe_end(&mut self, cancelled: bool) -> bool {
        let Some(name) = self.slides.swiping.take() else {
            return false;
        };
        let now = self.clock.now();
        let Some(slide) = self.slides.by_output.get_mut(&name) else {
            return true;
        };
        let Some((origin, _)) = slide.swipe.take() else {
            return true;
        };
        let at = slide.position.value(now);
        let target = if cancelled { origin } else { at.round() };
        slide
            .position
            .set(target, Curve::Spring(Spring::SMOOTH), now);
        slide.until = now + STANDARD + LINGER;
        if target != origin
            && let Some(output) = self.find_output(&name)
        {
            // The model switches; the strip goes on from where it is, on
            // its spring.
            let _ = self.show_desktop(&output, target as usize, SERIAL_COUNTER.next_serial());
            if let Some(slide) = self.slides.by_output.get_mut(&name) {
                slide
                    .position
                    .set(target, Curve::Spring(Spring::SMOOTH), now);
            }
        }
        self.needs_redraw = true;
        true
    }
}
