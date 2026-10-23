//! The Matrix theme's screens over the desktop (docs/design/matrix-theme.md,
//! §10, §11, §12 and §16): the switcher and the overview coming and going
//! in rain, the lock's rain cascading over the desktop and its padlock,
//! the unlock's flash, and the boot's wake-up, terminal and dissolve.
//!
//! Drawn with the shell's pictures, plain colours and glyphs from the
//! sheet, as `matrix_shell`; each a closed form of the time since it
//! began. The rain that stays behind the lock stands still once it has
//! fallen; the switcher's and the boot's fall while they show, as their
//! live previews and pandas already ask for frames.

use std::time::Duration;

use hocus_shell::matrix::{BLACK, CELL, GLOW, GREEN, HEAD, hash};
use hocus_ui::{Color, motion::Easing};
use smithay::{
    backend::renderer::{ImportAll, ImportMem, Renderer, Texture},
    output::Output,
    utils::{Logical, Point},
};

use crate::{
    State,
    alt_tab::{Closing, Shown, preview_elements},
    matrix::{Brush, SHEET_COLS, in_quad, millis, phase},
    matrix_shell::{Rain, Rect, rect, seed_of, ui_rect},
    matrix_windows::{damped, lerp, out_quad},
    scene::SceneElement,
};

/// The lock's rain has covered the desktop by then.
pub const LOCK_COVERED: Duration = Duration::from_millis(600);
/// The lock is in, its rain come to rest.
pub const LOCK_IN: Duration = Duration::from_millis(1000);
/// Unlocked: the rain speeds up, flashes, and the desktop's green fades.
pub const UNLOCK: Duration = Duration::from_millis(300);
/// A refused password shakes the lock 200 ms, its red fades over 300.
pub const REFUSED: Duration = Duration::from_millis(300);
/// Everything back: SYSTEM READY, the dissolve and the flash.
pub const BOOT_OUT: Duration = Duration::from_millis(1000);

/// The red of a refused password: the theme's error.
const RED: Color = Color::rgb(0xFF, 0x70, 0x70);
/// Behind Alt+Tab, black at 200 of 255.
const SWITCHER_DIM: f32 = 200.0 / 255.0;

/// How much our lock screen shows, `since` it began coming in the Matrix
/// theme: once the rain has covered the desktop, it fades in (out-cubic).
pub fn lock_shows(since: Duration, now: Duration) -> f32 {
    Easing::OutCubic.apply(phase(millis(since, now), 600.0, 800.0))
}

/// How far the padlock and the field are pushed sideways, a password
/// refused `since`: three swings of 8 px, dying out within 200 ms.
pub fn shake(since: Duration, now: Duration) -> f64 {
    let p = millis(since, now) / 200.0;
    if p >= 1.0 {
        return 0.0;
    }
    let swing = (6.0 * std::f32::consts::PI * p).sin() * (-2.0 * p).exp() * (1.0 - p);
    f64::from(8.0 * swing)
}

/// Where the lock's rain is, `t` milliseconds in: falling, then slowing
/// to a stop at 800 by `LOCK_IN`, where it stays.
fn lock_rain(t: f32) -> f32 {
    if t < 600.0 {
        t
    } else {
        600.0 + 200.0 * out_quad(phase(t, 600.0, 1000.0))
    }
}

/// Whether a closing switcher still goes.
pub(crate) fn closing_runs(closing: &Closing, now: Duration) -> bool {
    millis(closing.since, now) < closing_ms(closing.open.shown)
}

fn closing_ms(shown: Shown) -> f32 {
    match shown {
        Shown::Switcher => 200.0,
        Shown::Overview => 350.0,
    }
}

/// A switcher's moment on one screen.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Stage {
    Open(f32),
    Closing(f32, Option<usize>),
}

impl<R> Brush<'_, R>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    /// A bright ring `width` wide around `rect`, its glow `glow` wider.
    fn glow(&mut self, rect: Rect, width: f64, glow: f64, alpha: f32) {
        self.outline(rect, width, GLOW, alpha);
        let wide = Rect::new(
            (rect.loc.x - width, rect.loc.y - width).into(),
            (rect.size.w + 2.0 * width, rect.size.h + 2.0 * width).into(),
        );
        self.outline(wide, glow, GLOW, 0.25 * alpha);
    }

    /// The columns of glyphs of `area` falling off it, `p` of the way
    /// (in-cubic), each at its own pace, fading.
    fn collapse(&mut self, area: Rect, seed: u64, p: f32) {
        if p >= 1.0 {
            return;
        }
        let cw = f64::from(CELL.0);
        let fall = f64::from(Easing::InCubic.apply(p));
        let cols = (area.size.w / cw).ceil() as u32;
        for i in 0..cols {
            let speed = 0.7 + 0.6 * (hash(&[seed, i.into()]) % 1000) as f64 / 1000.0;
            let dy = speed * fall * area.size.h;
            let height = area.size.h - dy;
            let x = area.loc.x + f64::from(i) * cw;
            if height <= 0.0 || x >= area.loc.x + area.size.w {
                continue;
            }
            self.column(
                Point::from((x, area.loc.y + dy)),
                height,
                (hash(&[seed, i.into(), 1]) % u64::from(SHEET_COLS)) as u32,
                i,
                1.0 - p,
            );
        }
    }

    /// Columns of glyphs falling into `area` over black, `p` of the way
    /// down, each at its own pace.
    fn fill_down(&mut self, area: Rect, seed: u64, p: f32) -> Vec<Rect> {
        let cw = f64::from(CELL.0);
        let cols = (area.size.w / cw).ceil() as u32;
        let mut behind = Vec::new();
        for i in 0..cols {
            let speed = 0.8 + 0.5 * (hash(&[seed, i.into()]) % 1000) as f64 / 1000.0;
            let h = (area.size.h * f64::from(p) * speed).min(area.size.h);
            let x = area.loc.x + f64::from(i) * cw;
            let w = cw.min(area.loc.x + area.size.w - x);
            if h <= 0.0 || w <= 0.0 {
                continue;
            }
            self.column(
                Point::from((x, area.loc.y)),
                h,
                (hash(&[seed, i.into(), 1]) % u64::from(SHEET_COLS)) as u32,
                i,
                1.0,
            );
            behind.push(rect(x, area.loc.y, w, h));
        }
        behind
    }
}

impl State {
    /// The switcher or the overview on `output` in the Matrix theme,
    /// coming, there, or going: front first.
    pub(crate) fn matrix_switcher_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let now = self.clock.now();
        let (open, stage) = match (&self.alt_tab.open, &self.alt_tab.closing) {
            (Some(open), _) => (open, Stage::Open(millis(open.since, now))),
            (None, Some(c)) => (&c.open, Stage::Closing(millis(c.since, now), c.picked)),
            (None, None) => return Vec::new(),
        };
        let Some(view) = open.views.iter().find(|v| v.output == output.name()) else {
            return Vec::new();
        };
        let Some(area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let scale = output.current_scale().fractional_scale();
        let screen = rect(0.0, 0.0, f64::from(area.size.w), f64::from(area.size.h));
        let moves = self.matrix_moves();
        let overview = open.shown == Shown::Overview;
        let seed = seed_of(&output.name());
        let place = view.place.to_f64();
        // Each card: its window, its card and its slot, and where the
        // window is on this screen.
        let cards: Vec<_> = (0..view.count)
            .filter_map(|i| {
                let (id, _) = open.entries.get(view.first + i)?;
                let window = self.windows.get(id)?.window.clone();
                let card = ui_rect(view.panel.view.card(i)?, place);
                let slot = ui_rect(view.panel.view.preview(i)?, place);
                let frame = (self.output_of(&window) == Some(output)
                    && !self.model.is_minimized(*id))
                .then(|| {
                    let at = self.space.element_location(&window)? - area.loc;
                    let size = window.geometry().size;
                    Some(rect(
                        f64::from(at.x),
                        f64::from(at.y),
                        f64::from(size.w),
                        f64::from(size.h),
                    ))
                })
                .flatten();
                Some((view.first + i, window, card, slot, frame))
            })
            .collect();
        // How much of it shows, its previews and its panel.
        let (shows, panel_alpha) = match stage {
            Stage::Open(t) if moves && overview => (phase(t, 0.0, 400.0), phase(t, 250.0, 400.0)),
            Stage::Open(t) if moves => {
                let a = Easing::OutCubic.apply(t / 150.0);
                (a, a)
            }
            Stage::Open(_) => (1.0, 1.0),
            Stage::Closing(t, _) if overview => (
                1.0 - Easing::OutCubic.apply(t / 350.0),
                1.0 - phase(t, 0.0, 150.0),
            ),
            Stage::Closing(t, _) => {
                let a = 1.0 - Easing::OutCubic.apply(t / 200.0);
                (a, a)
            }
        };
        let mut out = Vec::new();
        let mut brush = self.brush(renderer, scale);
        let mut behind: Vec<Rect> = Vec::new();
        let mut previews: Vec<(smithay::desktop::Window, Rect, f32)> = Vec::new();
        match stage {
            Stage::Open(t) => {
                let selected = cards.iter().find(|c| c.0 == open.selected);
                if moves
                    && let Some((from, since)) = open.moved
                    && let Some(new) = selected
                    && let Some(old) = cards.iter().find(|c| c.0 == from)
                {
                    let age = millis(since, now);
                    // A trace from the old card to the new, gone in 100 ms;
                    // sparks at the new one for 80.
                    if age < 100.0 {
                        let (a, b) = (centre(old.2), centre(new.2));
                        let y = b.y;
                        let (l, r) = (a.x.min(b.x), a.x.max(b.x));
                        brush.solid(
                            rect(l, y - 1.0, (r - l).max(1.0), 2.0),
                            HEAD,
                            1.0 - age / 100.0,
                        );
                        if (a.y - b.y).abs() > 1.0 {
                            let (top, bottom) = (a.y.min(b.y), a.y.max(b.y));
                            brush.solid(
                                rect(a.x - 1.0, top, 2.0, bottom - top),
                                HEAD,
                                1.0 - age / 100.0,
                            );
                        }
                    }
                    if age < 80.0 {
                        let mut rng =
                            hocus_shell::matrix::Rng::new(hash(&[seed, since.as_millis() as u64]));
                        let c = centre(new.2);
                        let k = f64::from(age) / 1000.0;
                        for _ in 0..6 {
                            let angle = f64::from(rng.range(0.0, std::f32::consts::TAU));
                            let speed = f64::from(rng.range(200.0, 500.0));
                            let edge = (new.2.size.w / 2.0) * angle.cos().abs().max(0.3);
                            let (x, y) = (
                                c.x + angle.cos() * (edge + speed * k),
                                c.y + angle.sin() * (new.2.size.h / 2.0 + speed * k),
                            );
                            brush.solid(rect(x, y, 2.0, 2.0), HEAD, 1.0 - age / 80.0);
                        }
                    }
                }
                // The glow follows the panel's ring as it glides (150 ms,
                // out-cubic).
                let ring = selected.map(|new| {
                    let old = open
                        .moved
                        .filter(|(_, since)| moves && millis(*since, now) < 150.0)
                        .and_then(|(from, since)| {
                            let old = cards.iter().find(|c| c.0 == from)?;
                            let p = Easing::OutCubic.apply(millis(since, now) / 150.0);
                            Some(between(old.2, new.2, p))
                        });
                    old.unwrap_or(new.2)
                });
                if let Some(ring) = ring {
                    brush.glow(ring, 2.0, 4.0, panel_alpha);
                }
                for (n, (_, window, _, slot, frame)) in cards.iter().enumerate() {
                    if moves && overview && t < 400.0 {
                        // Windows turn to rain where they are, then their
                        // glyphs gather into the grid, giving way to them.
                        let from = frame.unwrap_or(*slot);
                        let seed = hash(&[seed, n as u64]);
                        if t < 100.0 {
                            if frame.is_some() {
                                behind.extend(brush.fill_down(from, seed, t / 100.0));
                            }
                        } else {
                            let p = damped(phase(t, 100.0, 400.0), 0.3);
                            let r = between(from, *slot, p);
                            brush.typed(r, r.size.w, seed, 1.0 - phase(t, 300.0, 400.0), false);
                            previews.push((window.clone(), r, phase(t, 250.0, 400.0)));
                        }
                    } else {
                        previews.push((window.clone(), *slot, shows));
                    }
                }
            }
            Stage::Closing(t, picked) => {
                for (n, (i, window, card, slot, frame)) in cards.iter().enumerate() {
                    if !overview {
                        previews.push((window.clone(), *slot, shows));
                        if picked == Some(*i) {
                            brush.glow(*card, 2.0, 4.0, shows);
                        }
                        continue;
                    }
                    if picked == Some(*i) {
                        // The one picked goes back to its place on a
                        // spring, its border flashing and fading.
                        let to = frame.unwrap_or(*slot);
                        let r = between(*slot, to, damped(t / 350.0, 0.35));
                        let pulse = 1.0 - t / 350.0;
                        brush.glow(r, 2.0, 6.0, pulse);
                        previews.push((window.clone(), r, 1.0));
                    } else {
                        // The others fall to pieces.
                        brush.collapse(*slot, hash(&[seed, n as u64]), t / 350.0);
                    }
                }
            }
        }
        out.append(&mut brush.out);
        let mut back = self.brush(brush.renderer, scale);
        for r in behind {
            back.solid(r, BLACK, 1.0);
        }
        out.append(&mut back.out);
        for (window, r, alpha) in previews {
            if alpha > 0.0 {
                out.extend(preview_elements(renderer, &window, r, scale, alpha));
            }
        }
        out.extend(
            view.panel
                .element_with(
                    renderer,
                    output,
                    view.place,
                    Some(panel_alpha.clamp(0.0, 1.0)),
                )
                .map(SceneElement::Frame),
        );
        // Behind it all, the dark and the rain: pure black for the
        // overview, black at 200 of 255 for Alt+Tab.
        let mut brush = self.brush(renderer, scale);
        if self.matrix_rains() {
            let rain = Rain::sparse(seed, if overview { 0.5 } else { 0.3 });
            brush.rain(
                screen,
                &rain,
                millis(Duration::ZERO, now),
                0.7 * shows,
                |_| f64::MAX,
            );
        }
        let dim = if overview { 1.0 } else { SWITCHER_DIM };
        brush.solid(screen, BLACK, dim * shows);
        out.append(&mut brush.out);
        out
    }

    /// Behind our lock screen in the Matrix theme: its padlock, the red of
    /// a refused password, the rain and the black. Front first.
    pub(crate) fn matrix_lock_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        if !self.matrix_look() {
            return Vec::new();
        }
        let Some(panel) = self.lock_screen_on(output) else {
            return Vec::new();
        };
        let Some(area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let now = self.clock.now();
        let scale = output.current_scale().fractional_scale();
        let screen = rect(0.0, 0.0, f64::from(area.size.w), f64::from(area.size.h));
        let seed = seed_of(&output.name());
        let since = self.lock_matrix_since();
        let t = since.map_or(f32::MAX, |s| millis(s, now));
        let shows = since.map_or(1.0, |s| lock_shows(s, now));
        let refused = self.locking.refused.map(|s| millis(s, now));
        let dx = self.locking.refused.map_or(0.0, |s| shake(s, now));
        let mut brush = self.brush(renderer, scale);
        if let Some(badge) = panel.view.badge() {
            let at = ui_rect(badge, Point::default()).loc + Point::from((dx, 0.0));
            brush.padlock(at, seed, shows);
        }
        // Refused: red scanlines over the padlock and the field, and the
        // rain gone red a moment.
        let red = refused.map_or(0.0, |t| 1.0 - phase(t, 0.0, 300.0));
        if red > 0.0
            && let Some(band) = panel.view.band()
        {
            let band = ui_rect(band, Point::default());
            let mut y = band.loc.y;
            while y < band.loc.y + band.size.h {
                brush.solid(rect(band.loc.x, y, band.size.w, 1.0), RED, 0.35 * red);
                y += 3.0;
            }
            brush.solid(screen, RED, 0.18 * red);
        }
        if self.matrix_rains() {
            if t < 600.0 {
                // It cascades over the desktop, which shows below it.
                brush.cascade(screen, &Rain::dense(seed), 1.2 * t / 600.0, t, 1.0);
                return brush.out;
            }
            // Dimmer once it has covered the desktop, for the lock to
            // read over it.
            let dim = 1.0 - 0.6 * phase(t.min(1000.0), 600.0, 1000.0);
            brush.rain(screen, &Rain::dense(seed), lock_rain(t), dim, |_| f64::MAX);
        }
        brush.solid(screen, BLACK, 1.0);
        brush.out
    }

    /// Unlocked in the Matrix theme: the rain speeds up over the lock,
    /// flashes, and the desktop's green fades. Front first.
    pub(crate) fn matrix_unlock_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let Some((since, panel)) = self.unlock_matrix_on(output) else {
            return Vec::new();
        };
        let Some(area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let t = millis(since, self.clock.now());
        let scale = output.current_scale().fractional_scale();
        let screen = rect(0.0, 0.0, f64::from(area.size.w), f64::from(area.size.h));
        let seed = seed_of(&output.name());
        let mut out = Vec::new();
        if t < 150.0 {
            out.extend(
                panel
                    .element(renderer, output, Point::default())
                    .map(SceneElement::Frame),
            );
            let mut brush = self.brush(renderer, scale);
            if let Some(badge) = panel.view.badge() {
                brush.padlock(ui_rect(badge, Point::default()).loc, seed, 1.0);
            }
            // Three times as fast by the end, and bright again.
            let bright = 0.4 + 0.6 * t / 150.0;
            brush.rain(
                screen,
                &Rain::dense(seed),
                800.0 + t + t * t / 150.0,
                bright,
                |_| f64::MAX,
            );
            brush.solid(screen, BLACK, 1.0);
            out.append(&mut brush.out);
            return out;
        }
        let mut brush = self.brush(renderer, scale);
        if t < 200.0 {
            brush.solid(screen, HEAD, 1.0 - out_quad(phase(t, 150.0, 200.0)));
        } else {
            brush.solid(screen, GREEN, 0.25 * (1.0 - phase(t, 200.0, 300.0)));
        }
        brush.out
    }

    /// The boot screen in the Matrix theme: darkness and a cursor, the rain
    /// waking, the terminal over slow rain with its strip filling; once
    /// all is back, SYSTEM READY, the dissolve and a flash. Front first.
    pub(crate) fn matrix_boot_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        if !self.matrix_look() {
            return Vec::new();
        }
        let Some((since, leaving, panel)) = self.boot_matrix_on(output) else {
            return Vec::new();
        };
        let Some(area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let now = self.clock.now();
        let moves = self.matrix_moves();
        let t = if moves { millis(since, now) } else { f32::MAX };
        let l = leaving.map(|s| millis(s, now));
        let scale = output.current_scale().fractional_scale();
        let screen = rect(0.0, 0.0, f64::from(area.size.w), f64::from(area.size.h));
        let middle = Point::<f64, Logical>::from((screen.size.w / 2.0, screen.size.h / 2.0));
        let seed = seed_of(&output.name());
        let mut brush = self.brush(renderer, scale);
        // The flash, then the desktop.
        if let Some(l) = l
            && l >= 850.0
        {
            let a = if l < 900.0 {
                1.0
            } else {
                1.0 - out_quad(phase(l, 900.0, 1000.0))
            };
            brush.solid(screen, HEAD, a);
            return brush.out;
        }
        if let Some(l) = l {
            let a = phase(l, 0.0, 100.0) * (1.0 - phase(l, 400.0, 850.0));
            brush.letters(
                self.shell_lang().system_ready(),
                middle,
                screen.size.w,
                seed,
                a,
            );
            if l < 400.0 {
                brush.ring(
                    middle,
                    2.0 * f64::from(l),
                    8.0,
                    HEAD,
                    0.9 * (1.0 - l / 400.0).sqrt(),
                );
            }
        }
        if (50.0..250.0).contains(&t) {
            brush.solid(rect(middle.x - 6.0, middle.y - 11.0, 12.0, 22.0), HEAD, 1.0);
        }
        let panel_alpha = if moves {
            Easing::OutCubic.apply(phase(t, 800.0, 1000.0))
                * (1.0 - l.map_or(0.0, |l| phase(l, 0.0, 200.0)))
        } else {
            1.0
        };
        if let Some(bar) = panel.view.bar() {
            let bar = ui_rect(bar, Point::default());
            let strip = rect(
                bar.loc.x,
                bar.loc.y + bar.size.h / 2.0 - 7.0,
                bar.size.w,
                14.0,
            );
            let filled = bar.size.w * f64::from(panel.view.progress());
            // A strip of glyphs filling left to right, its end bright.
            brush.typed(
                strip,
                filled.max(f64::from(CELL.0)),
                seed,
                panel_alpha,
                true,
            );
        }
        let mut out = std::mem::take(&mut brush.out);
        out.extend(
            panel
                .element_with(renderer, output, Point::default(), Some(panel_alpha))
                .map(SceneElement::Frame),
        );
        let mut brush = self.brush(renderer, scale);
        if self.matrix_rains() && t >= 300.0 {
            if t < 800.0 {
                // It wakes: full, bright and fast, from the top.
                let rain = Rain {
                    speed: 1040.0,
                    ..Rain::dense(seed)
                };
                brush.cascade(screen, &rain, 1.2 * (t - 300.0) / 500.0, t - 300.0, 1.0);
            } else {
                // Slower behind the terminal; going, it speeds up and
                // brightens (in-quad).
                let speed_up = l.map_or(0.0, |l| {
                    3.0 * in_quad(phase(l, 400.0, 850.0)) * (l - 400.0).max(0.0)
                });
                let bright = l.map_or(0.0, |l| phase(l, 400.0, 850.0));
                let tau = if moves {
                    500.0 + 0.5 * (t - 800.0) + speed_up
                } else {
                    500.0
                };
                brush.rain(
                    screen,
                    &Rain::dense(seed),
                    tau,
                    0.45 + 0.55 * bright,
                    |_| f64::MAX,
                );
            }
        }
        brush.solid(screen, BLACK, 1.0);
        out.append(&mut brush.out);
        out
    }
}

fn centre(r: Rect) -> Point<f64, Logical> {
    Point::from((r.loc.x + r.size.w / 2.0, r.loc.y + r.size.h / 2.0))
}

/// Between `a` and `b`, `p` of the way (beyond, overshooting).
fn between(a: Rect, b: Rect, p: f32) -> Rect {
    rect(
        lerp(a.loc.x, b.loc.x, p),
        lerp(a.loc.y, b.loc.y, p),
        lerp(a.size.w, b.size.w, p).max(1.0),
        lerp(a.size.h, b.size.h, p).max(1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn the_lock_shows_once_the_rain_has_covered_the_desktop() {
        assert_eq!(lock_shows(ms(0), ms(500)), 0.0);
        assert!(lock_shows(ms(0), ms(700)) > 0.5);
        assert_eq!(lock_shows(ms(0), ms(800)), 1.0);
        assert_eq!(lock_rain(300.0), 300.0);
        assert_eq!(lock_rain(1000.0), 800.0);
        assert_eq!(lock_rain(5000.0), 800.0, "then it stands still");
    }

    #[test]
    fn a_refused_password_shakes_three_times_and_stops() {
        let swings: Vec<f64> = (0..200).map(|t| shake(ms(0), ms(t))).collect();
        let peak = swings.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        assert!(peak > 4.0 && peak <= 8.0, "{peak}");
        let crossings = swings.windows(2).filter(|w| w[0] * w[1] < 0.0).count();
        assert!((4..=6).contains(&crossings), "{crossings}");
        assert_eq!(shake(ms(0), ms(200)), 0.0);
    }

    #[test]
    fn halfway_is_between() {
        let r = between(
            rect(0.0, 0.0, 100.0, 100.0),
            rect(100.0, 50.0, 50.0, 50.0),
            0.5,
        );
        assert_eq!(r, rect(50.0, 25.0, 75.0, 75.0));
    }
}
