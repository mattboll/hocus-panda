//! "Identifier" (Settings › Écrans, #927): for 3 s, each screen shows its
//! number large in its middle, its name and resolution under it, in a
//! rounded dark glass box: the numbers of the Settings' drawing, which the
//! Settings send (`identify [{"output":…,"number":…,"words":…}]` on their
//! line), so that the two never disagree.
//!
//! Drawn by the compositor over the windows and the bar, on the screen
//! only (never in a capture, a recording or a share); it takes neither the
//! keyboard nor the pointer. A click let go anywhere, or `Esc`, sends it
//! away early, and still goes where it was going. It fades in and out in
//! [`FADE`], at once with reduced motion; asked again while it shows, it
//! counts its 3 s again from there.

use std::{
    cell::{OnceCell, RefCell},
    collections::HashMap,
    time::Duration,
};

use serde::Deserialize;
use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{Id, memory::MemoryRenderBuffer},
    },
    output::Output,
    utils::{Logical, Rectangle},
};
use tiny_skia::Pixmap;

use crate::{
    State,
    away::{Felt, Pressed},
    decoration::{FrameIds, ShadowStyle, buffer, shadow_elements},
    magnifier::draw::placed,
    radial_draw::{MANROPE_SEMIBOLD, advance, argb, paint, rounded, stroke, write},
    render::{OutputElement, OutputElements},
};

const MANROPE_REGULAR: &[u8] = include_bytes!("../../hocus-shell/fonts/Manrope-Regular.ttf");

/// How long the numbers show, their fades included.
pub const SHOWN_FOR: Duration = Duration::from_secs(3);
/// Their fade in and out.
pub const FADE: Duration = Duration::from_millis(200);
/// `Esc`'s XKB keycode (evdev's 1, plus 8).
const ESC: u32 = 9;

/// The box, logical pixels: the mockup's, at the size of a real screen.
/// Its fill is the magnifier's pill's 94 %, not the mockup's 78 %:
/// nothing blurs behind it,
/// and a window's text (the Settings' own drawing, enlarged) showed
/// through.
const NUMBER_PX: f32 = 168.0;
const WORDS_PX: f32 = 22.0;
const PAD_X: f64 = 52.0;
const PAD_Y: f64 = 30.0;
const GAP: f64 = 16.0;
const RADIUS: f64 = 36.0;
const FILL: [u8; 4] = [14, 17, 28, 240];
const RIM: [u8; 4] = [255, 255, 255, 36];
const NUMBER: [u8; 4] = [0xEE, 0xF1, 0xF6, 0xFF];
const WORDS: [u8; 4] = [0xA9, 0xB2, 0xC2, 0xFF];
const SHADOW: ShadowStyle = ShadowStyle {
    dx: 0.0,
    dy: 14.0,
    blur: 40.0,
    alpha: 0.45,
    radius: RADIUS,
};

/// What one screen shows, as the Settings say it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Badge {
    /// The screen, by its connector.
    pub output: String,
    pub number: u32,
    /// "Écran du portable · 2560 × 1600".
    pub words: String,
    /// What it says in place of its number: "1 · 2" on the screen shown
    /// while mirrored (#931).
    #[serde(default)]
    pub label: Option<String>,
}

impl Badge {
    /// What it says large.
    fn said(&self) -> String {
        self.label
            .clone()
            .unwrap_or_else(|| self.number.to_string())
    }
}

/// The numbers while they show.
#[derive(Debug, Clone)]
struct Shown {
    badges: Vec<Badge>,
    /// When it came in, and when it is gone (the motions' clock).
    since: Duration,
    until: Duration,
}

/// The fonts of the box: the number's and the words'.
struct Faces {
    number: fontdue::Font,
    words: fontdue::Font,
}

/// A box's picture: what it was drawn for (what it says, words, scale in
/// thousandths), its pixels, their size, its logical size and its id.
type Picture = (
    (String, String, u32),
    MemoryRenderBuffer,
    (i32, i32),
    (f64, f64),
    Id,
);

#[derive(Default)]
pub struct Identify {
    shown: Option<Shown>,
    faces: OnceCell<Option<Faces>>,
    /// Each screen's box, by its connector, while it shows.
    pictures: RefCell<HashMap<String, Picture>>,
    shadow: FrameIds,
}

impl std::fmt::Debug for Identify {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identify")
            .field("shown", &self.shown)
            .finish_non_exhaustive()
    }
}

fn faces() -> Option<Faces> {
    let read = |bytes: &'static [u8]| {
        fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|err| tracing::warn!(err, "the screens' numbers' font does not read"))
            .ok()
    };
    Some(Faces {
        number: read(MANROPE_SEMIBOLD)?,
        words: read(MANROPE_REGULAR)?,
    })
}

/// The box's layout, logical: its size, and the number's and the words'
/// baselines from its top.
struct Layout {
    w: f64,
    h: f64,
    number_line: f64,
    words_line: f64,
}

fn layout(faces: &Faces, number: &str, words: &str) -> Layout {
    // The digits' height over the baseline: they stand on it.
    let top = number
        .chars()
        .map(|c| {
            let m = faces.number.metrics(c, NUMBER_PX);
            m.height as f32 + m.ymin as f32
        })
        .fold(0.0_f32, f32::max);
    let (ascent, descent) = faces
        .words
        .horizontal_line_metrics(WORDS_PX)
        .map_or((WORDS_PX * 0.8, -WORDS_PX * 0.2), |m| (m.ascent, m.descent));
    let inner = f64::from(advance(&faces.number, number, NUMBER_PX)).max(f64::from(advance(
        &faces.words,
        words,
        WORDS_PX,
    )));
    let number_line = PAD_Y + f64::from(top);
    let words_line = number_line + GAP + f64::from(ascent);
    Layout {
        w: (inner + 2.0 * PAD_X).ceil(),
        h: (words_line - f64::from(descent) + PAD_Y).ceil(),
        number_line,
        words_line,
    }
}

/// The box drawn at `scale`: its pixels and their size, and its logical
/// size.
fn draw(
    faces: &Faces,
    number: &str,
    words: &str,
    scale: f64,
) -> Option<(Vec<u8>, i32, i32, Layout)> {
    let l = layout(faces, number, words);
    let (pw, ph) = ((l.w * scale).ceil() as u32, (l.h * scale).ceil() as u32);
    let mut pixmap = Pixmap::new(pw, ph)?;
    let s = scale as f32;
    let t = tiny_skia::Transform::from_scale(s, s);
    let (fw, fh) = (l.w as f32, l.h as f32);
    if let Some(path) = rounded(0.5, 0.5, fw - 1.0, fh - 1.0, RADIUS as f32) {
        pixmap.fill_path(&path, &paint(FILL), tiny_skia::FillRule::Winding, t, None);
        pixmap.stroke_path(&path, &paint(RIM), &stroke(1.0), t, None);
    }
    let centred =
        |font: &fontdue::Font, text: &str, px: f32| (fw - advance(font, text, px)) / 2.0 * s;
    write(
        &mut pixmap,
        &faces.number,
        number,
        NUMBER_PX * s,
        (
            centred(&faces.number, number, NUMBER_PX),
            l.number_line as f32 * s,
        ),
        NUMBER,
        0.0,
    );
    write(
        &mut pixmap,
        &faces.words,
        words,
        WORDS_PX * s,
        (
            centred(&faces.words, words, WORDS_PX),
            l.words_line as f32 * s,
        ),
        WORDS,
        0.0,
    );
    Some((argb(pixmap), pw as i32, ph as i32, l))
}

impl State {
    /// The Settings' "Identifier": each screen of `badges` shows its
    /// number for [`SHOWN_FOR`] from now; showing already, it stays in
    /// and counts again.
    pub(crate) fn identify(&mut self, badges: Vec<Badge>) {
        let now = self.now();
        let alpha = self.identify_alpha(now);
        // Still fading in or out: on from where it is.
        let since = now.saturating_sub(FADE.mul_f32(alpha));
        tracing::info!(screens = badges.len(), "the screens identify themselves");
        self.identify.shown = Some(Shown {
            badges,
            since,
            until: now + SHOWN_FOR,
        });
        self.redraw = true;
    }

    /// The user's input: a click let go or `Esc` sends the numbers away,
    /// fading from where they are. Nothing is taken from what it reaches.
    pub(crate) fn identify_felt(&mut self, felt: Felt) {
        if !matches!(
            felt,
            Felt::Release(Pressed::Button(_)) | Felt::Press(Pressed::Key(ESC))
        ) {
            return;
        }
        let now = self.now();
        let alpha = self.identify_alpha(now);
        let reduced = self.reduced_motion();
        let Some(shown) = self.identify.shown.as_mut() else {
            return;
        };
        let until = if reduced {
            now
        } else {
            now + FADE.mul_f32(alpha)
        };
        if until < shown.until {
            tracing::debug!("the screens' numbers sent away");
            shown.until = until;
            self.redraw = true;
        }
    }

    /// How much of the numbers shows at `now`.
    fn identify_alpha(&self, now: Duration) -> f32 {
        let Some(shown) = &self.identify.shown else {
            return 0.0;
        };
        if now >= shown.until {
            return 0.0;
        }
        if self.reduced_motion() {
            return 1.0;
        }
        let fade = FADE.as_secs_f32();
        let coming = now.saturating_sub(shown.since).as_secs_f32() / fade;
        let going = (shown.until - now).as_secs_f32() / fade;
        coming.min(going).clamp(0.0, 1.0)
    }

    /// The numbers fade and go; whether they move. `wake`: when to draw
    /// again after a pause.
    pub(crate) fn tick_identify(&mut self, now: Duration, wake: &mut impl FnMut(Duration)) -> bool {
        let Some(shown) = &self.identify.shown else {
            return false;
        };
        let (since, until) = (shown.since, shown.until);
        if now >= until {
            self.identify.shown = None;
            self.identify.pictures.borrow_mut().clear();
            self.redraw = true;
            return false;
        }
        if self.reduced_motion() {
            wake(until);
            false
        } else if now < since + FADE || now + FADE >= until {
            true
        } else {
            wake(until - FADE);
            false
        }
    }

    /// What each screen shows now, for tests: its connector, number and
    /// words.
    pub(crate) fn identified(&self) -> Vec<Badge> {
        let Some(shown) = &self.identify.shown else {
            return Vec::new();
        };
        if self.identify_alpha(self.now()) <= 0.0 {
            return Vec::new();
        }
        shown
            .badges
            .iter()
            .filter(|b| self.space.outputs().any(|o| o.name() == b.output))
            .cloned()
            .collect()
    }

    /// How long the numbers still show, for tests.
    pub(crate) fn identify_left(&self) -> Option<Duration> {
        let shown = self.identify.shown.as_ref()?;
        let now = self.now();
        (now < shown.until).then(|| shown.until - now)
    }

    /// The box on `output`, front first: on the screen only.
    pub(crate) fn identify_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        out: &mut Vec<OutputElement<R>>,
    ) where
        R: Renderer + ImportAll + ImportMem + crate::wobble::Bend + crate::going::Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(shown) = &self.identify.shown else {
            return;
        };
        let name = output.name();
        let Some(badge) = shown.badges.iter().find(|b| b.output == name) else {
            return;
        };
        let alpha = self.identify_alpha(self.now());
        if alpha <= 0.0 {
            return;
        }
        let (Some(area), Some(faces)) = (
            self.space.output_geometry(output),
            self.identify.faces.get_or_init(faces).as_ref(),
        ) else {
            return;
        };
        let scale = output.current_scale().fractional_scale();
        let key = (
            badge.said(),
            badge.words.clone(),
            (scale * 1000.0).round() as u32,
        );
        let mut pictures = self.identify.pictures.borrow_mut();
        if pictures.get(&name).is_none_or(|(k, ..)| *k != key)
            && let Some((pixels, pw, ph, l)) = draw(faces, &badge.said(), &badge.words, scale)
        {
            pictures.insert(
                name.clone(),
                (
                    key,
                    buffer(&pixels, pw, ph),
                    (pw, ph),
                    (l.w, l.h),
                    Id::new(),
                ),
            );
        }
        let Some((_, texture, px, (w, h), id)) = pictures.get(&name) else {
            return;
        };
        let rect: Rectangle<f64, Logical> = Rectangle::new(
            (
                ((f64::from(area.size.w) - w) / 2.0).round(),
                ((f64::from(area.size.h) - h) / 2.0).round(),
            )
                .into(),
            (*w, *h).into(),
        );
        out.extend(placed(renderer, texture, *px, rect, scale, alpha, id));
        out.extend(
            shadow_elements(
                renderer,
                &self.textures,
                &self.identify.shadow,
                SHADOW,
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
    fn the_box_draws_at_two_scales_around_its_words() {
        let Some(faces) = faces() else {
            panic!("the fonts read");
        };
        let words = "Écran du portable · 2560 × 1600";
        let l = layout(&faces, "1", words);
        assert!(l.w >= f64::from(advance(&faces.words, words, WORDS_PX)) + 2.0 * PAD_X);
        assert!(l.number_line < l.words_line && l.words_line < l.h);
        for scale in [1.0, 1.5, 2.0] {
            let (pixels, w, h, _) = draw(&faces, "1", words, scale).expect("drawn");
            assert_eq!(pixels.len(), (w * h * 4) as usize);
            assert_eq!(h, (l.h * scale).ceil() as i32);
        }
    }

    #[test]
    fn the_settings_line_reads() {
        let badges: Vec<Badge> = serde_json::from_str(
            r#"[{"output":"eDP-1","number":1,"words":"Écran du portable · 2560 × 1600"}]"#,
        )
        .expect("read");
        assert_eq!(badges[0].output, "eDP-1");
        assert_eq!(badges[0].number, 1);
    }
}
