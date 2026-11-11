//! The magnifier (#424, docs/design/accessibility.md): the screen under
//! the pointer drawn up to 16 times larger by the compositor, its view
//! following the pointer (at the edge, centred or proportionally), the
//! keyboard's focus and an application's text caret.
//!
//! One screen at a time: the one under the pointer; the others stay at
//! 1×. Everything keeps its real coordinates: the pointer moves on the
//! desktop as it always does, and what it shows is drawn where it is in
//! the view, so a click reaches what is under the pointer as drawn.
//! Screenshots, recordings and shared screens are drawn apart, at 1×.
//!
//! The view's motions (a jump to the focus, a zoom step) glide 120 ms on
//! the frame clock, or jump with reduced motion; a push of the pointer
//! moves it at once. A pill at the top shows the zoom for 2 s after each
//! change, its − and + taking clicks; a minimap at the bottom right shows
//! the view on the screen until 1.5 s after it stops. Nothing is drawn at
//! rest: the drawing ([`draw`]) keeps the screen's picture between frames
//! and only redraws what changed.

pub mod draw;

use std::time::Duration;

use smithay::{
    backend::input::ButtonState,
    desktop::{WindowSurfaceType, layer_map_for_output},
    output::Output,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle, Size},
    wayland::seat::WaylandFocus,
};

use crate::{
    State,
    config::FollowPointer,
    focus::KeyboardFocus,
    motion::{Bezier, Tween, lerp},
};

/// The zooms `Super+=` and `Super+-` step through.
pub const STEPS: [f64; 9] = [1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0];
pub const MAX: f64 = 16.0;
/// A notch of the wheel with `Super`: 10 %.
const WHEEL: f64 = 1.1;
/// A finger scroll's pixels that make a notch.
const NOTCH: f64 = 15.0;
/// How long the view glides to where it goes.
const GLIDE: Duration = Duration::from_millis(120);
/// The pill shows this long after a change of zoom; the minimap this
/// long after the view stops.
pub const HUD_FOR: Duration = Duration::from_secs(2);
pub const MINIMAP_FOR: Duration = Duration::from_millis(1500);
/// Both fade out in this time (at once with reduced motion).
pub const FADE: Duration = Duration::from_millis(120);
/// The text caret is kept this share of the view in from its edges.
const CARET_MARGIN: f64 = 0.1;

/// The magnifier's state.
#[derive(Debug, Default)]
pub struct Magnifier {
    /// On, and where; `None`: off.
    pub(crate) on: Option<On>,
    /// The zoom it comes back at, once turned off.
    last: Option<f64>,
    /// When the pill and the minimap go.
    pub(crate) hud_until: Option<Duration>,
    pub(crate) minimap_until: Option<Duration>,
    /// Where the pointer was when the view last followed it.
    pointer: Option<Point<f64, Logical>>,
    /// The motion under way comes from an absolute device (a tablet, a
    /// VM's pointer, the nested window's), which cannot push an edge.
    absolute: bool,
    /// The button pressed on the pill's − or +: its release is kept too.
    held: Option<u32>,
    /// What draws the pill and the minimap.
    pub(crate) paint: draw::Paint,
}

/// The magnifier on a screen.
#[derive(Debug, Clone)]
pub(crate) struct On {
    pub output: String,
    /// Where the view goes: its zoom, and its top left corner on the
    /// screen (logical, from the screen's corner).
    pub zoom: f64,
    pub view: Point<f64, Logical>,
    /// Where it glides from, when it does.
    glide: Option<Glide>,
}

#[derive(Debug, Clone, Copy)]
struct Glide {
    zoom: f64,
    view: Point<f64, Logical>,
    tween: Tween,
}

/// What a view shows of a screen of `size` at `zoom`.
fn view_size(size: Size<f64, Logical>, zoom: f64) -> Size<f64, Logical> {
    Size::from((size.w / zoom, size.h / zoom))
}

/// The step after `zoom`, up or down.
pub fn step(zoom: f64, up: bool) -> f64 {
    const EPS: f64 = 1e-6;
    if up {
        STEPS
            .iter()
            .copied()
            .find(|s| *s > zoom + EPS)
            .unwrap_or(MAX)
    } else {
        STEPS
            .iter()
            .rev()
            .copied()
            .find(|s| *s < zoom - EPS)
            .unwrap_or(1.0)
    }
}

/// `view` kept on the screen.
pub fn clamp(
    view: Point<f64, Logical>,
    size: Size<f64, Logical>,
    zoom: f64,
) -> Point<f64, Logical> {
    let shown = view_size(size, zoom);
    Point::from((
        view.x.clamp(0.0, (size.w - shown.w).max(0.0)),
        view.y.clamp(0.0, (size.h - shown.h).max(0.0)),
    ))
}

/// The view at zoom `to` keeping `anchor` (on the screen) where it shows,
/// from `view` at zoom `from`.
pub fn zoom_about(
    view: Point<f64, Logical>,
    anchor: Point<f64, Logical>,
    from: f64,
    to: f64,
    size: Size<f64, Logical>,
) -> Point<f64, Logical> {
    let shown = (anchor - view).upscale(from);
    clamp(anchor - shown.downscale(to), size, to)
}

/// Where the view goes when the pointer is at `pointer` (on the screen).
pub fn follow(
    how: FollowPointer,
    view: Point<f64, Logical>,
    pointer: Point<f64, Logical>,
    size: Size<f64, Logical>,
    zoom: f64,
) -> Point<f64, Logical> {
    let shown = view_size(size, zoom);
    let to = match how {
        FollowPointer::Edge => {
            let along = |v: f64, p: f64, w: f64| {
                if p < v {
                    p
                } else if p > v + w {
                    p - w
                } else {
                    v
                }
            };
            Point::from((
                along(view.x, pointer.x, shown.w),
                along(view.y, pointer.y, shown.h),
            ))
        }
        FollowPointer::Centre => {
            Point::from((pointer.x - shown.w / 2.0, pointer.y - shown.h / 2.0))
        }
        // The pointer as far into the view as into the screen.
        FollowPointer::Proportional => Point::from((
            pointer.x - pointer.x / size.w * shown.w,
            pointer.y - pointer.y / size.h * shown.h,
        )),
    };
    clamp(to, size, zoom)
}

/// A view: x, y, width, height (global, logical).
pub type Seen = (f64, f64, f64, f64);

/// What the view jumps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Show {
    /// What took the keyboard: centred if it fits, else its top left.
    Focus,
    /// The text caret: kept 10 % in from the view's edges, moving as
    /// little as it can.
    Caret,
}

/// The view at `zoom` showing `rect` (on the screen) as `show` says, from
/// `view`; `None` when it shows already.
pub fn showing(
    show: Show,
    view: Point<f64, Logical>,
    rect: Rectangle<f64, Logical>,
    size: Size<f64, Logical>,
    zoom: f64,
) -> Option<Point<f64, Logical>> {
    let shown = view_size(size, zoom);
    let to = match show {
        Show::Focus => {
            let inside = rect.loc.x >= view.x
                && rect.loc.y >= view.y
                && rect.loc.x + rect.size.w <= view.x + shown.w
                && rect.loc.y + rect.size.h <= view.y + shown.h;
            if inside {
                return None;
            }
            let along = |r: f64, rw: f64, w: f64| {
                if rw <= w { r + rw / 2.0 - w / 2.0 } else { r }
            };
            Point::from((
                along(rect.loc.x, rect.size.w, shown.w),
                along(rect.loc.y, rect.size.h, shown.h),
            ))
        }
        Show::Caret => {
            let along = |v: f64, r: f64, rw: f64, w: f64| {
                let m = w * CARET_MARGIN;
                if r < v + m {
                    r - m
                } else if r + rw > v + w - m {
                    r + rw - w + m
                } else {
                    v
                }
            };
            Point::from((
                along(view.x, rect.loc.x, rect.size.w, shown.w),
                along(view.y, rect.loc.y, rect.size.h, shown.h),
            ))
        }
    };
    let to = clamp(to, size, zoom);
    (to != view).then_some(to)
}

/// The zoom as the pill writes it: `×8`, `×1,5` in French.
pub fn zoom_words(zoom: f64, lang: hocus_words::Lang) -> String {
    let tenths = (zoom * 10.0).round() as i64;
    let text = if tenths % 10 == 0 {
        format!("×{}", tenths / 10)
    } else {
        format!("×{}.{}", tenths / 10, tenths % 10)
    };
    text.replace('.', hocus_words::number::DECIMAL.get(lang))
}

impl Magnifier {
    /// Its zoom and view at `now` on the screen it is on.
    fn shown(&self, now: Duration) -> Option<(&str, f64, Point<f64, Logical>)> {
        let on = self.on.as_ref()?;
        Some(match on.glide {
            Some(g) => {
                let t = g.tween.at(now);
                let view =
                    Point::from((lerp(g.view.x, on.view.x, t), lerp(g.view.y, on.view.y, t)));
                (on.output.as_str(), lerp(g.zoom, on.zoom, t), view)
            }
            None => (on.output.as_str(), on.zoom, on.view),
        })
    }
}

impl State {
    /// The magnifier's zoom and view (from the screen's corner, logical)
    /// on `output` at `now`, when it is on there.
    pub(crate) fn magnified(
        &self,
        output: &Output,
        now: Duration,
    ) -> Option<(f64, Point<f64, Logical>)> {
        let (name, zoom, view) = self.magnifier.shown(now)?;
        (name == output.name()).then_some((zoom, view))
    }

    /// The screen of `name` and where it is.
    fn screen_named(&self, name: &str) -> Option<(Output, Rectangle<f64, Logical>)> {
        let output = self.space.outputs().find(|o| o.name() == name)?.clone();
        let area = self.space.output_geometry(&output)?.to_f64();
        Some((output, area))
    }

    /// The screen under `at`, else the first.
    fn screen_at(&self, at: Point<f64, Logical>) -> Option<(Output, Rectangle<f64, Logical>)> {
        let output = self
            .space
            .output_under(at)
            .next()
            .or_else(|| self.space.outputs().next())?
            .clone();
        let area = self.space.output_geometry(&output)?.to_f64();
        Some((output, area))
    }

    fn pointer_location(&self) -> Point<f64, Logical> {
        self.seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default()
    }

    /// The view goes to `zoom` and `view`, gliding from where it shows
    /// unless motion is reduced.
    fn magnifier_to(&mut self, zoom: f64, view: Point<f64, Logical>) {
        let now = self.now();
        let reduced = self.reduced_motion();
        let shown = self.magnifier.shown(now).map(|(_, z, v)| (z, v));
        let Some(on) = self.magnifier.on.as_mut() else {
            return;
        };
        on.glide = match shown {
            Some((z, v)) if !reduced && ((z - zoom).abs() > 1e-9 || v != view) => Some(Glide {
                zoom: z,
                view: v,
                tween: Tween::new(now, GLIDE, Bezier::EASE_OUT),
            }),
            _ => None,
        };
        on.zoom = zoom;
        on.view = view;
        self.magnifier.minimap_until = Some(now + MINIMAP_FOR);
        self.redraw = true;
    }

    /// On at `zoom`, on the screen under the pointer, the pointer staying
    /// where it shows.
    fn magnifier_on(&mut self, zoom: f64) {
        let pointer = self.pointer_location();
        let Some((output, area)) = self.screen_at(pointer) else {
            return;
        };
        let zoom = zoom.clamp(1.0, MAX);
        let anchor = pointer - area.loc;
        let view = zoom_about(Point::default(), anchor, 1.0, zoom, area.size);
        tracing::info!(output = output.name(), zoom, "magnifier on");
        self.magnifier.on = Some(On {
            output: output.name(),
            zoom: 1.0,
            view: Point::default(),
            glide: None,
        });
        self.magnifier.pointer = Some(pointer);
        self.magnifier_to(zoom, view);
    }

    /// The zoom to `zoom` about the pointer (about the view's middle when
    /// the pointer is elsewhere); 1 or less turns it off.
    fn magnifier_zoom(&mut self, zoom: f64) {
        let Some(on) = self.magnifier.on.clone() else {
            if zoom > 1.0 + 1e-6 {
                self.magnifier_on(zoom);
            }
            return;
        };
        if zoom <= 1.0 + 1e-6 {
            // Stepped out: the next step in starts from 1 again.
            self.magnifier_gone(None);
            return;
        }
        let Some((_, area)) = self.screen_named(&on.output) else {
            return;
        };
        let zoom = zoom.min(MAX);
        let pointer = self.pointer_location() - area.loc;
        let shown = view_size(area.size, on.zoom);
        let inside = pointer.x >= on.view.x
            && pointer.y >= on.view.y
            && pointer.x <= on.view.x + shown.w
            && pointer.y <= on.view.y + shown.h;
        let anchor = if inside {
            pointer
        } else {
            on.view + Point::from((shown.w / 2.0, shown.h / 2.0))
        };
        let view = zoom_about(on.view, anchor, on.zoom, zoom, area.size);
        self.magnifier_to(zoom, view);
    }

    /// Off, coming back at `last` next time.
    fn magnifier_gone(&mut self, last: Option<f64>) {
        if let Some(on) = self.magnifier.on.take() {
            tracing::info!(output = on.output, "magnifier off");
        }
        self.magnifier.last = last;
        self.magnifier.hud_until = None;
        self.magnifier.minimap_until = None;
        self.redraw = true;
    }

    /// The pill shows the zoom for a while.
    fn magnifier_told(&mut self) {
        if self.magnifier.on.is_some() {
            self.magnifier.hud_until = Some(self.now() + HUD_FOR);
            self.redraw = true;
        }
    }

    /// `Super+=` (`up`) and `Super+-`: a step in, turning it on at its
    /// last zoom; a step out, off at 1×.
    pub fn magnifier_step(&mut self, up: bool) {
        match self.magnifier.on.as_ref().map(|on| on.zoom) {
            None if up => {
                let zoom = self.magnifier.last.unwrap_or(STEPS[1]);
                self.magnifier_on(zoom);
            }
            None => return,
            Some(zoom) => self.magnifier_zoom(step(zoom, up)),
        }
        self.magnifier_told();
    }

    /// `Super+Escape`: off, back at this zoom next time.
    pub fn magnifier_off(&mut self) {
        if let Some(zoom) = self.magnifier.on.as_ref().map(|on| on.zoom) {
            self.magnifier_gone(Some(zoom));
        }
    }

    /// `Super` and the wheel or two fingers: 10 % a notch, up zooming in.
    /// Whether the magnifier took it.
    pub(crate) fn magnifier_scroll(&mut self, vertical: f64) -> bool {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return false;
        };
        let mods = keyboard.modifier_state();
        // Nested, `Super` is `Alt+Super`, as for the shortcuts.
        let held = mods.logo && !mods.ctrl && !mods.shift && (mods.alt == self.nested);
        if !held || vertical == 0.0 {
            return false;
        }
        let zoom = self.magnifier.on.as_ref().map_or(1.0, |on| on.zoom);
        let to = (zoom * WHEEL.powf(-vertical / NOTCH)).min(MAX);
        if (to - zoom).abs() > 1e-9 {
            if self.magnifier.on.is_some() || to > 1.0 + 1e-6 {
                self.magnifier_zoom(to);
                // The wheel's steps follow each other: no glide.
                if let Some(on) = self.magnifier.on.as_mut() {
                    on.glide = None;
                }
            }
            self.magnifier_told();
        }
        true
    }

    /// The configuration's `[magnifier]` as the session starts: on, if it
    /// says so.
    pub(crate) fn magnifier_start(&mut self) {
        if self.config.magnifier.enabled {
            let zoom = self.config.magnifier.zoom;
            self.magnifier_on(zoom);
        }
    }

    /// `[magnifier]` changed on disk (the Settings): on or off, its zoom,
    /// the rest read where used. Whether it changed.
    pub(crate) fn magnifier_config_changed(&mut self, new: &crate::config::Magnifier) -> bool {
        if *new == self.config.magnifier {
            return false;
        }
        let old = std::mem::replace(&mut self.config.magnifier, new.clone());
        if new.enabled != old.enabled {
            if new.enabled {
                if self.magnifier.on.is_none() {
                    self.magnifier_on(new.zoom);
                }
            } else {
                self.magnifier_off();
            }
        } else if (new.zoom - old.zoom).abs() > 1e-9 && self.magnifier.on.is_some() {
            self.magnifier_zoom(new.zoom);
            self.magnifier_told();
        }
        self.redraw = true;
        true
    }

    /// The motion under way comes from an absolute device.
    pub(crate) fn magnifier_absolute(&mut self, absolute: bool) {
        self.magnifier.absolute = absolute;
    }

    /// The pointer moved to `location` (global): the view follows it, and
    /// goes to the screen it crosses to.
    pub(crate) fn magnifier_pointer(&mut self, location: Point<f64, Logical>) {
        let Some(on) = self.magnifier.on.clone() else {
            return;
        };
        if self.magnifier.pointer == Some(location) {
            return;
        }
        self.magnifier.pointer = Some(location);
        // The cursor is drawn magnified, over the screen's picture.
        self.redraw = true;
        let Some((output, area)) = self.screen_at(location) else {
            return;
        };
        let local = location - area.loc;
        if output.name() != on.output {
            // The magnifier goes with the pointer; the screen it leaves is
            // back at 1×.
            let view = follow(
                FollowPointer::Proportional,
                Point::default(),
                local,
                area.size,
                on.zoom,
            );
            self.magnifier.on = Some(On {
                output: output.name(),
                zoom: on.zoom,
                view,
                glide: None,
            });
            self.magnifier.minimap_until = Some(self.now() + MINIMAP_FOR);
            return;
        }
        // An absolute device cannot push an edge: proportional.
        let how = if self.magnifier.absolute {
            FollowPointer::Proportional
        } else {
            self.config.magnifier.follow_pointer
        };
        let view = follow(how, on.view, local, area.size, on.zoom);
        if view != on.view {
            let now = self.now();
            if let Some(on) = self.magnifier.on.as_mut() {
                // The pointer takes over from a glide under way.
                on.glide = None;
                on.view = view;
            }
            self.magnifier.minimap_until = Some(now + MINIMAP_FOR);
        }
    }

    /// The view jumps to show `rect` (global) as `how` says, when it is on
    /// the magnified screen and the keyboard is followed.
    pub(crate) fn magnifier_show(&mut self, rect: Rectangle<f64, Logical>, how: Show) {
        if !self.config.magnifier.follow_keyboard {
            return;
        }
        let Some(on) = self.magnifier.on.clone() else {
            return;
        };
        let Some((_, area)) = self.screen_named(&on.output) else {
            return;
        };
        if !area.overlaps(rect) {
            return;
        }
        // What the pointer just clicked is where the user looks already.
        if how == Show::Focus && rect.contains(self.pointer_location()) {
            return;
        }
        let local = Rectangle::new(rect.loc - area.loc, rect.size);
        if let Some(view) = showing(how, on.view, local, area.size, on.zoom) {
            self.magnifier_to(on.zoom, view);
        }
    }

    /// The keyboard went to `focused`: the view jumps to it (a window, a
    /// popover or another of the shell's surfaces).
    pub(crate) fn magnifier_focus_changed(&mut self, focused: Option<&KeyboardFocus>) {
        if self.magnifier.on.is_none() {
            return;
        }
        let Some(surface) = focused.and_then(|f| f.wl_surface()).map(|s| s.into_owned()) else {
            return;
        };
        if let Some(rect) = self.surface_rect(&surface) {
            self.magnifier_show(rect, Show::Focus);
        }
    }

    /// Where `surface` is drawn (global): a window's geometry, or a layer
    /// surface's place.
    fn surface_rect(&self, surface: &WlSurface) -> Option<Rectangle<f64, Logical>> {
        if let Some(window) = self
            .space
            .elements()
            .find(|w| w.wl_surface().is_some_and(|s| *s == *surface))
        {
            let mut rect = self.space.element_geometry(window)?;
            rect.size = window.geometry().size;
            return Some(rect.to_f64());
        }
        self.space.outputs().find_map(|output| {
            let map = layer_map_for_output(output);
            let layer = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)?;
            let mut rect = map.layer_geometry(layer)?;
            rect.loc += self.space.output_geometry(output)?.loc;
            Some(rect.to_f64())
        })
    }

    /// `surface`'s text input says where its caret is now: the view keeps
    /// it inside.
    pub(crate) fn magnifier_caret(&mut self, surface: &WlSurface) {
        if self.magnifier.on.is_none() {
            return;
        }
        let Some(mut caret) = self.emoji.inputs.caret(surface) else {
            return;
        };
        let Some(window) = self
            .space
            .elements()
            .find(|w| w.wl_surface().is_some_and(|s| *s == *surface))
        else {
            return;
        };
        let Some(at) = self.space.element_location(window) else {
            return;
        };
        // The surface's origin: its geometry's corner less what of it is
        // outside (a client-side shadow), as the emoji picker places it.
        caret.loc += at - window.geometry().loc;
        caret.size.w = caret.size.w.max(1);
        caret.size.h = caret.size.h.max(1);
        self.magnifier_show(caret.to_f64(), Show::Caret);
    }

    /// The glide, the pill and the minimap move on; whether something
    /// still moves. `wake`: when to draw again after a pause.
    pub(crate) fn tick_magnifier(
        &mut self,
        now: Duration,
        wake: &mut impl FnMut(Duration),
    ) -> bool {
        let reduced = self.reduced_motion();
        let magnifier = &mut self.magnifier;
        let mut moving = false;
        match magnifier.on.as_mut() {
            Some(on) => {
                if let Some(glide) = on.glide {
                    if glide.tween.done(now) {
                        on.glide = None;
                    } else {
                        moving = true;
                    }
                }
            }
            None => {
                magnifier.hud_until = None;
                magnifier.minimap_until = None;
            }
        }
        let mut gone = false;
        for until in [&mut magnifier.hud_until, &mut magnifier.minimap_until] {
            let Some(at) = *until else {
                continue;
            };
            if now >= at {
                *until = None;
                gone = true;
            } else if reduced {
                wake(at);
            } else if now + FADE >= at {
                moving = true;
            } else {
                wake(at - FADE);
            }
        }
        if gone {
            self.redraw = true;
        }
        moving
    }

    /// How much of the pill or the minimap shows at `now`, going at
    /// `until`.
    pub(crate) fn magnifier_fade(&self, until: Option<Duration>, now: Duration) -> f32 {
        match until {
            Some(at) if now < at => {
                if self.reduced_motion() {
                    1.0
                } else {
                    ((at - now).as_secs_f64() / FADE.as_secs_f64()).min(1.0) as f32
                }
            }
            _ => 0.0,
        }
    }

    /// Where the pointer shows on the magnified screen (logical, from its
    /// corner), and that screen.
    fn pointer_on_view(&self, now: Duration) -> Option<(Output, Point<f64, Logical>)> {
        let (name, zoom, view) = self.magnifier.shown(now)?;
        let (output, area) = self.screen_named(name)?;
        let local = self.pointer_location() - area.loc;
        Some((output, (local - view).upscale(zoom)))
    }

    /// A press on the pill's − or +: kept from clients, its release too.
    /// Whether the magnifier took the button.
    pub(crate) fn magnifier_button(&mut self, button: u32, state: ButtonState) -> bool {
        if state == ButtonState::Released {
            if self.magnifier.held == Some(button) {
                self.magnifier.held = None;
                self.pointer.kept_buttons.remove(&button);
                return true;
            }
            return false;
        }
        let now = self.now();
        if button != crate::grabs::BTN_LEFT
            || self.magnifier_fade(self.magnifier.hud_until, now) <= 0.0
        {
            return false;
        }
        let Some((output, at)) = self.pointer_on_view(now) else {
            return false;
        };
        let Some((_, area)) = self.screen_named(&output.name()) else {
            return false;
        };
        let Some(zoom) = self.magnifier.on.as_ref().map(|on| on.zoom) else {
            return false;
        };
        let words = draw::Words::of(zoom, self.radial_lang());
        let Some(pill) = self.magnifier.paint.layout(&words, area.size) else {
            return false;
        };
        let up = if pill.plus.contains(at) {
            true
        } else if pill.minus.contains(at) {
            false
        } else {
            return pill.rect.contains(at) && {
                // The pill itself takes the press, doing nothing.
                self.magnifier.held = Some(button);
                self.pointer.kept_buttons.insert(button);
                true
            };
        };
        self.magnifier.held = Some(button);
        self.pointer.kept_buttons.insert(button);
        self.magnifier_step(up);
        true
    }

    /// What `hocus msg outputs` says of the magnifier on screen `name`:
    /// its zoom and view (global, logical), or null.
    pub(crate) fn magnifier_json(&self, name: &str) -> serde_json::Value {
        let Some(on) = self.magnifier.on.as_ref().filter(|on| on.output == name) else {
            return serde_json::Value::Null;
        };
        let Some((_, area)) = self.screen_named(name) else {
            return serde_json::Value::Null;
        };
        let shown = view_size(area.size, on.zoom);
        serde_json::json!({
            "zoom": on.zoom,
            "view": [area.loc.x + on.view.x, area.loc.y + on.view.y, shown.w, shown.h],
        })
    }

    /// The magnifier's screen, zoom and view (global: x, y, width,
    /// height) as it goes, for tests.
    pub fn magnifier_seen(&self) -> Option<(String, f64, Seen)> {
        let on = self.magnifier.on.as_ref()?;
        let (_, area) = self.screen_named(&on.output)?;
        let shown = view_size(area.size, on.zoom);
        Some((
            on.output.clone(),
            on.zoom,
            (
                area.loc.x + on.view.x,
                area.loc.y + on.view.y,
                shown.w,
                shown.h,
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (f64, f64) = (1920.0, 1080.0);

    fn size() -> Size<f64, Logical> {
        Size::from(SCREEN)
    }

    fn p(x: f64, y: f64) -> Point<f64, Logical> {
        Point::from((x, y))
    }

    #[test]
    fn the_keys_step_through_the_zooms_and_out_at_one() {
        assert_eq!(step(1.0, true), 1.5);
        assert_eq!(step(1.5, true), 2.0);
        assert_eq!(step(12.0, true), 16.0);
        assert_eq!(step(16.0, true), 16.0);
        assert_eq!(step(2.0, false), 1.5);
        assert_eq!(step(1.5, false), 1.0);
        // From between two steps (the wheel's), the next one each way.
        assert_eq!(step(2.4, true), 3.0);
        assert_eq!(step(2.4, false), 2.0);
    }

    #[test]
    fn zooming_keeps_the_anchor_where_it_shows_and_the_view_on_the_screen() {
        // At 2× about the middle, the middle quarter.
        let view = zoom_about(p(0.0, 0.0), p(960.0, 540.0), 1.0, 2.0, size());
        assert_eq!(view, p(480.0, 270.0));
        // About the top left corner: it stays there.
        assert_eq!(
            zoom_about(p(0.0, 0.0), p(0.0, 0.0), 1.0, 8.0, size()),
            p(0.0, 0.0)
        );
        // About the bottom right corner: the view ends there.
        let view = zoom_about(p(0.0, 0.0), p(1920.0, 1080.0), 1.0, 16.0, size());
        assert_eq!(view, p(1920.0 - 120.0, 1080.0 - 67.5));
        // Never off the screen.
        let view = zoom_about(p(1800.0, 0.0), p(1900.0, 10.0), 16.0, 2.0, size());
        assert_eq!(view, p(960.0, 0.0));
    }

    #[test]
    fn at_the_edge_the_view_moves_only_when_pushed_and_by_as_much() {
        let view = p(480.0, 270.0);
        // 2×: 960 × 540 from (480, 270).
        assert_eq!(
            follow(FollowPointer::Edge, view, p(900.0, 500.0), size(), 2.0),
            view
        );
        assert_eq!(
            follow(
                FollowPointer::Edge,
                view,
                p(1440.0 + 50.0, 500.0),
                size(),
                2.0
            ),
            p(530.0, 270.0)
        );
        assert_eq!(
            follow(FollowPointer::Edge, view, p(400.0, 200.0), size(), 2.0),
            p(400.0, 200.0)
        );
        // Centred: the pointer in its middle; proportional: as far in.
        assert_eq!(
            follow(FollowPointer::Centre, view, p(1000.0, 600.0), size(), 2.0),
            p(520.0, 330.0)
        );
        assert_eq!(
            follow(
                FollowPointer::Proportional,
                view,
                p(1920.0, 0.0),
                size(),
                2.0
            ),
            p(960.0, 0.0)
        );
        // Never off the screen.
        assert_eq!(
            follow(FollowPointer::Centre, view, p(1910.0, 1070.0), size(), 2.0),
            p(960.0, 540.0)
        );
    }

    #[test]
    fn the_focus_is_centred_when_it_fits_and_the_caret_kept_in() {
        let view = p(0.0, 0.0);
        // 8×: 240 × 135. A 100 × 50 rectangle far away: centred on it.
        let rect = Rectangle::new(p(1000.0, 500.0), Size::from((100.0, 50.0)));
        assert_eq!(
            showing(Show::Focus, view, rect, size(), 8.0),
            Some(p(1050.0 - 120.0, 525.0 - 67.5))
        );
        // Inside already: nothing.
        let inside = Rectangle::new(p(10.0, 10.0), Size::from((20.0, 20.0)));
        assert_eq!(showing(Show::Focus, view, inside, size(), 8.0), None);
        // Too large to fit: its top left corner.
        let window = Rectangle::new(p(600.0, 300.0), Size::from((800.0, 600.0)));
        assert_eq!(
            showing(Show::Focus, view, window, size(), 8.0),
            Some(p(600.0, 300.0))
        );
        // A caret past the right edge: moved just enough to be 10 % in.
        let caret = Rectangle::new(p(250.0, 60.0), Size::from((2.0, 16.0)));
        assert_eq!(
            showing(Show::Caret, view, caret, size(), 8.0),
            Some(p(252.0 - 240.0 + 24.0, 0.0))
        );
        let caret = Rectangle::new(p(100.0, 60.0), Size::from((2.0, 16.0)));
        assert_eq!(showing(Show::Caret, view, caret, size(), 8.0), None);
    }

    #[test]
    fn the_zoom_reads_in_either_language() {
        assert_eq!(zoom_words(8.0, hocus_words::Lang::French), "×8");
        assert_eq!(zoom_words(1.5, hocus_words::Lang::French), "×1,5");
        assert_eq!(zoom_words(1.5, hocus_words::Lang::English), "×1.5");
        assert_eq!(zoom_words(2.31, hocus_words::Lang::English), "×2.3");
    }
}
