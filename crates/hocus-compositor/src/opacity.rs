//! A window's opacity (#735, docs/design/windows.md §3): `Super`+`Alt`
//! and the wheel over a window make it more or less transparent, by 10 %
//! a notch, from 100 % down to 20 %, so that it never vanishes. Kept per
//! window for its life, and in the saved session (§14). While it changes,
//! a pill in the window's middle says the percentage for 2 s, drawn as
//! the magnifier's pill (accessibility.md), then fades out.
//!
//! Nested, `Super` is `Alt+Super` already, and `Alt+Super` and the wheel
//! is the magnifier's: the opacity has no wheel there.

use hocus_words::compositor::opacity as txt;
use std::{
    cell::{OnceCell, RefCell},
    time::Duration,
};

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{Id, memory::MemoryRenderBuffer},
    },
    output::Output,
    utils::{Logical, Rectangle},
};
use tiny_skia::{PathBuilder, Pixmap};

use crate::{
    State,
    decoration::{FrameIds, buffer, shadow_elements},
    magnifier::draw::{
        ACCENT, ICON, ICON_GAP, PAD_LEFT, PILL_FILL, PILL_H, PILL_RIM, PILL_SHADOW, TEXT, ZOOM_PX,
        placed,
    },
    model::WindowId,
    radial_draw::{Fonts, advance, argb, paint, rounded, stroke, write},
    render::{OutputElement, OutputElements},
};

/// Opaque, in percent.
pub const FULL: u8 = 100;
/// The least a window can be: it stays visible.
pub const FLOOR: u8 = 20;
/// A notch of the wheel.
pub const STEP: u8 = 10;
/// A finger scroll's pixels that make a notch, as the magnifier's.
const NOTCH: f64 = 15.0;
/// The pill shows this long after the last change.
pub const PILL_FOR: Duration = Duration::from_secs(2);
const PAD_RIGHT: f64 = 14.0;

/// The opacity a step up (more opaque) or down from `percent`.
pub fn stepped(percent: u8, up: bool) -> u8 {
    let percent = percent.clamp(FLOOR, FULL);
    if up {
        percent.saturating_add(STEP).min(FULL)
    } else {
        percent.saturating_sub(STEP).max(FLOOR)
    }
}

/// `70 %` in French, `70%`: the pill's monospaced font has no narrow
/// no-break space.
pub fn percent_words(percent: u8, lang: hocus_words::Lang) -> String {
    txt::text(lang, percent)
}

/// The wheel's pixels not yet a notch, the window they are for, and the
/// pill.
#[derive(Default)]
pub struct Opacity {
    /// The window whose percentage shows, and until when.
    pub(crate) pill: Option<(WindowId, Duration)>,
    /// A finger's scroll, short of a notch, over that window.
    rest: Option<(WindowId, f64)>,
    fonts: OnceCell<Option<Fonts>>,
    /// The pill drawn for its words and scale (thousandths).
    picture: RefCell<Option<Picture>>,
    shadow: FrameIds,
}

/// A pill's picture: its key, its pixels, their size and its id.
type Picture = ((String, u32), MemoryRenderBuffer, (i32, i32), Id);

impl std::fmt::Debug for Opacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Opacity")
            .field("pill", &self.pill)
            .finish_non_exhaustive()
    }
}

/// The pill's width for `words`.
fn width(fonts: &Fonts, words: &str) -> f64 {
    PAD_LEFT + ICON + ICON_GAP + f64::from(advance(&fonts.mono, words, ZOOM_PX)) + PAD_RIGHT
}

/// The pill drawn at `scale`: its pixels, width and height.
fn draw_pill(fonts: &Fonts, words: &str, scale: f64) -> Option<(Vec<u8>, i32, i32)> {
    let w = width(fonts, words);
    let (pw, ph) = ((w * scale).ceil() as u32, (PILL_H * scale).ceil() as u32);
    let mut pixmap = Pixmap::new(pw, ph)?;
    let s = scale as f32;
    let t = tiny_skia::Transform::from_scale(s, s);
    let (fw, fh) = (w as f32, PILL_H as f32);
    if let Some(path) = rounded(0.5, 0.5, fw - 1.0, fh - 1.0, fh / 2.0) {
        pixmap.fill_path(
            &path,
            &paint(PILL_FILL),
            tiny_skia::FillRule::Winding,
            t,
            None,
        );
        pixmap.stroke_path(&path, &paint(PILL_RIM), &stroke(1.0), t, None);
    }
    // A circle half filled, a 24-unit drawing at 15 px.
    let k = (ICON / 24.0) as f32;
    let icon = t
        .pre_translate(PAD_LEFT as f32, ((PILL_H - ICON) / 2.0) as f32)
        .pre_scale(k, k);
    let mut pb = PathBuilder::new();
    pb.push_circle(12.0, 12.0, 9.0);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &paint(TEXT), &stroke(1.8), icon, None);
    }
    let mut pb = PathBuilder::new();
    pb.move_to(12.0, 3.0);
    pb.cubic_to(7.03, 3.0, 3.0, 7.03, 3.0, 12.0);
    pb.cubic_to(3.0, 16.97, 7.03, 21.0, 12.0, 21.0);
    pb.close();
    if let Some(path) = pb.finish() {
        pixmap.fill_path(
            &path,
            &paint(TEXT),
            tiny_skia::FillRule::Winding,
            icon,
            None,
        );
    }
    let baseline = (fh / 2.0 + ZOOM_PX * 0.36) * s;
    write(
        &mut pixmap,
        &fonts.mono,
        words,
        ZOOM_PX * s,
        ((PAD_LEFT + ICON + ICON_GAP) as f32 * s, baseline),
        ACCENT,
        0.0,
    );
    Some((argb(pixmap), pw as i32, ph as i32))
}

impl State {
    /// Window `id`'s opacity, in percent.
    pub(crate) fn opacity_of(&self, id: WindowId) -> u8 {
        self.windows.get(&id).map_or(FULL, |m| m.opacity)
    }

    /// Sets window `id`'s opacity (from the saved session): kept between
    /// the floor and opaque.
    pub(crate) fn set_opacity(&mut self, id: WindowId, percent: u8) {
        if let Some(managed) = self.windows.get_mut(&id) {
            let percent = percent.clamp(FLOOR, FULL);
            if managed.opacity != percent {
                managed.opacity = percent;
                self.redraw = true;
            }
        }
    }

    /// A scroll with `Super`+`Alt` held (not nested), over a window: its
    /// opacity, a step a notch, up more opaque. Whether it took it.
    pub(crate) fn opacity_scroll(&mut self, vertical: f64) -> bool {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return false;
        };
        let mods = keyboard.modifier_state();
        if self.nested || !(mods.logo && mods.alt && !mods.ctrl && !mods.shift) {
            return false;
        }
        let Some(pointer) = self.seat.get_pointer() else {
            return false;
        };
        // Over no window (the bar, the desktop), or with the overview, the
        // radial menu or Alt+Tab open: taken, and nothing changes, so that
        // nothing under scrolls by surprise.
        let busy = self.overview.is_some()
            || self.radial_menu.open.is_some()
            || self.switching.open.is_some();
        let Some(id) = self
            .window_under(pointer.current_location())
            .filter(|_| !busy)
        else {
            return true;
        };
        if vertical == 0.0 {
            return true;
        }
        // The laser goes until the keys are let go, as after a shortcut.
        if self.ink.other_key() {
            self.redraw = true;
        }
        let rest = match self.opacity.rest {
            Some((w, rest)) if w == id => rest,
            _ => 0.0,
        };
        let mut rest = rest + vertical;
        let mut percent = self.opacity_of(id);
        // A wheel's notch is one step, a finger's every 15 px.
        while rest.abs() >= NOTCH - 1e-9 {
            percent = stepped(percent, rest < 0.0);
            rest -= NOTCH.copysign(rest);
        }
        self.opacity.rest = Some((id, rest));
        if percent != self.opacity_of(id) {
            tracing::debug!(window = id.0, percent, "a window's opacity");
            self.set_opacity(id, percent);
            self.session_changed();
        }
        self.opacity.pill = Some((id, self.now() + PILL_FOR));
        self.redraw = true;
        true
    }

    /// The pill fades and goes; whether it moves. `wake`: when to draw
    /// again after a pause.
    pub(crate) fn tick_opacity(&mut self, now: Duration, wake: &mut impl FnMut(Duration)) -> bool {
        let Some((id, at)) = self.opacity.pill else {
            return false;
        };
        if now >= at || !self.windows.contains_key(&id) {
            self.opacity.pill = None;
            self.opacity.rest = None;
            self.redraw = true;
            return false;
        }
        let fade = crate::magnifier::FADE;
        if self.reduced_motion() {
            wake(at);
            false
        } else if now + fade >= at {
            true
        } else {
            wake(at - fade);
            false
        }
    }

    /// The window and the percentage the pill shows, for tests.
    pub(crate) fn opacity_pill(&self) -> Option<(WindowId, u8)> {
        let (id, at) = self.opacity.pill?;
        (self.now() < at).then(|| (id, self.opacity_of(id)))
    }

    /// The pill over its window on `output`, front first: on the screen
    /// only, never in a capture.
    pub(crate) fn opacity_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + crate::wobble::Bend + crate::going::Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some((id, until)) = self.opacity.pill else {
            return;
        };
        let now = self.now();
        let alpha = self.magnifier_fade(Some(until), now);
        if alpha <= 0.0 {
            return;
        }
        let (Some(area), Some(visual), Some(fonts)) = (
            self.space.output_geometry(output),
            self.visual_rect(id, now),
            self.opacity
                .fonts
                .get_or_init(crate::radial_draw::fonts)
                .as_ref(),
        ) else {
            return;
        };
        let scale = output.current_scale().fractional_scale();
        let words = percent_words(self.opacity_of(id), self.radial_lang());
        let w = width(fonts, &words);
        let centre = visual.loc + visual.size.downscale(2.0).to_point();
        let rect: Rectangle<f64, Logical> = Rectangle::new(
            (
                (centre.x - w / 2.0).round() - f64::from(area.loc.x),
                (centre.y - PILL_H / 2.0).round() - f64::from(area.loc.y),
            )
                .into(),
            (w, PILL_H).into(),
        );
        if !rect.overlaps(Rectangle::from_size(area.size.to_f64())) {
            return;
        }
        let key = (words.clone(), (scale * 1000.0).round() as u32);
        let mut cache = self.opacity.picture.borrow_mut();
        if cache.as_ref().is_none_or(|(k, ..)| *k != key)
            && let Some((pixels, pw, ph)) = draw_pill(fonts, &words, scale)
        {
            *cache = Some((key, buffer(&pixels, pw, ph), (pw, ph), Id::new()));
        }
        let Some((_, texture, px, picture_id)) = cache.as_ref() else {
            return;
        };
        out.extend(placed(
            renderer, texture, *px, rect, scale, alpha, picture_id,
        ));
        out.extend(
            shadow_elements(
                renderer,
                &self.textures,
                &self.opacity.shadow,
                PILL_SHADOW,
                rect,
                scale,
                alpha,
            )
            .into_iter()
            .map(OutputElements::Shadow),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_of_ten_between_the_floor_and_opaque() {
        assert_eq!(stepped(100, true), 100);
        assert_eq!(stepped(100, false), 90);
        assert_eq!(stepped(30, false), 20);
        assert_eq!(stepped(20, false), 20, "never under the floor");
        assert_eq!(stepped(20, true), 30);
        assert_eq!(percent_words(70, hocus_words::Lang::French), "70 %");
        assert_eq!(percent_words(70, hocus_words::Lang::English), "70%");
    }

    #[test]
    fn the_pill_draws_at_two_scales() {
        let Some(fonts) = crate::radial_draw::fonts() else {
            return;
        };
        for scale in [1.0, 2.0] {
            let (pixels, w, h) = draw_pill(&fonts, "40 %", scale).expect("drawn");
            assert_eq!(pixels.len(), (w * h * 4) as usize);
            assert_eq!(h, (PILL_H * scale).ceil() as i32);
        }
    }
}
