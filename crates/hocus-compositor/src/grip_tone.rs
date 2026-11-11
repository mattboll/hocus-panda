//! The grip's pill stands out from what lies under it (docs/design/
//! windows.md §7, #719): light over a dark header, as it always was, dark
//! over a light one, decided from the pixels of the window under the pill.
//!
//! Read from the renderer's copy of the window's surface, only while its
//! grip shows and once per commit of that surface: the copy of the few
//! pixels around the pill is asked once a frame is drawn, and read at the
//! next one, so that the GPU has done it by then and the loop never waits
//! on it (GLES copies into a pixel buffer that is mapped later). A window
//! at rest, or whose grip is hidden, costs nothing.

use std::any::Any;

use smithay::{
    backend::{
        allocator::{Fourcc, format::has_alpha},
        renderer::{
            ContextId, ExportMem, Texture,
            utils::{CommitCounter, with_renderer_surface_state},
        },
    },
    utils::{Logical, Point, Rectangle, Size},
    wayland::seat::WaylandFocus,
};

use crate::{
    State,
    decoration::{GRIP_INSET, GRIP_SIZE},
    model::WindowId,
    windows::Grip,
};

/// How far around the pill what lies under it is read, logical pixels.
const AROUND: f64 = 2.0;
/// How much better the other tone must stand out before the pill changes:
/// a header near the middle does not make it flicker from one commit to
/// the next.
const HYSTERESIS: f64 = 1.15;

/// The pill's two looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Tone {
    /// Light (`#EEF1F6` at 75 %), over dark headers: the grip's look before
    /// #719, and before anything under it is read.
    #[default]
    Light,
    /// Dark (`#141926` at 75 %, the light theme's text), over light ones.
    Dark,
}

impl Tone {
    /// The pill's colour, straight (not premultiplied).
    pub const fn rgba(self) -> [u8; 4] {
        match self {
            Self::Light => [0xEE, 0xF1, 0xF6, 191],
            Self::Dark => [0x14, 0x19, 0x26, 191],
        }
    }

    /// The pill's pixels over `under` (sRGB), blended as the renderers
    /// blend.
    pub fn over(self, under: [f64; 3]) -> [f64; 3] {
        let [r, g, b, a] = self.rgba();
        let a = f64::from(a) / 255.0;
        let [ur, ug, ub] = under;
        let mix = |pill: u8, under: f64| f64::from(pill) * a + under * (1.0 - a);
        [mix(r, ur), mix(g, ug), mix(b, ub)]
    }

    /// The tone that stands out more from `under`, staying `self` unless
    /// the other does clearly better.
    pub fn for_under(self, under: [f64; 3]) -> Self {
        let other = match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        };
        if contrast(other.over(under), under) > contrast(self.over(under), under) * HYSTERESIS {
            other
        } else {
            self
        }
    }
}

/// WCAG's relative luminance of an sRGB colour, channels 0 to 255.
pub fn luminance(rgb: [f64; 3]) -> f64 {
    let lin = |v: f64| {
        let v = (v / 255.0).clamp(0.0, 1.0);
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2])
}

/// WCAG's contrast ratio between two sRGB colours.
pub fn contrast(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// A window's pill tone, and what was read under it.
#[derive(Default)]
pub struct GripTone {
    pub tone: Tone,
    /// The commit of the window's surface last read (or being read).
    read: Option<CommitCounter>,
    /// A copy asked after the last frame, read at the next one: the
    /// renderer's mapping and its context, and whether the texture has
    /// alpha. Boxed: its type is the renderer's.
    pending: Option<Box<dyn Any>>,
}

impl std::fmt::Debug for GripTone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GripTone")
            .field("tone", &self.tone)
            .field("read", &self.read)
            .field("pending", &self.pending.is_some())
            .finish()
    }
}

/// A copy under way on a renderer: its context, its textures' type `T`,
/// and its mapping `M`.
struct Reading<T: Texture, M> {
    context: ContextId<T>,
    mapping: M,
    alpha: bool,
}

/// The mean colour of `pixels` (RGBA, premultiplied), straight; none when
/// mostly transparent: then what shows there is not the window's.
fn mean(pixels: &[u8], alpha: bool) -> Option<[f64; 3]> {
    let mut sum = [0.0f64; 4];
    let mut n = 0.0;
    for px in pixels.as_chunks::<4>().0 {
        let a = if alpha { f64::from(px[3]) } else { 255.0 };
        sum[0] += f64::from(px[0]);
        sum[1] += f64::from(px[1]);
        sum[2] += f64::from(px[2]);
        sum[3] += a;
        n += 1.0;
    }
    if n == 0.0 || sum[3] < n * 255.0 / 2.0 {
        return None;
    }
    // Premultiplied: the sum of the colours over the sum of the alphas is
    // the straight mean colour of what covers.
    let k = 255.0 / sum[3];
    Some([sum[0] * k, sum[1] * k, sum[2] * k])
}

/// The pill's place around which the window is read, logical and local to
/// its surface, for a window `geometry` (local to its surface too).
fn under_pill(geometry: Rectangle<i32, Logical>) -> Rectangle<f64, Logical> {
    let geometry = geometry.to_f64();
    Rectangle::new(
        Point::from((
            geometry.loc.x + (geometry.size.w - GRIP_SIZE.0) / 2.0 - AROUND,
            geometry.loc.y + GRIP_INSET - AROUND,
        )),
        Size::from((GRIP_SIZE.0 + 2.0 * AROUND, GRIP_SIZE.1 + 2.0 * AROUND)),
    )
}

impl State {
    /// The tone of window `id`'s pill as it is drawn.
    pub(crate) fn grip_tone(&self, id: WindowId) -> Tone {
        self.windows
            .get(&id)
            .map_or(Tone::Light, |m| m.grip_tone.tone)
    }

    /// Once a frame is drawn with `renderer`: reads the copies asked after
    /// the last one, and asks one under each grip that shows on a window
    /// committed since it was last read. A pill whose tone changes is
    /// drawn again.
    pub(crate) fn read_under_grips<R>(&mut self, renderer: &mut R)
    where
        R: ExportMem,
        R::TextureId: Clone + 'static,
        R::TextureMapping: 'static,
    {
        let showing: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, m)| m.grip != Grip::Hidden || m.grip_tone.pending.is_some())
            .map(|(id, _)| *id)
            .collect();
        let context = renderer.context_id();
        for id in showing {
            let headerless = !self.has_own_header(id);
            let Some(managed) = self.windows.get_mut(&id) else {
                continue;
            };
            let tone = &mut managed.grip_tone;
            // Another renderer's copy (another GPU's) waits for its own.
            if let Some(pending) = tone.pending.take() {
                match pending.downcast::<Reading<R::TextureId, R::TextureMapping>>() {
                    Ok(copy) if copy.context == context => {
                        match renderer.map_texture(&copy.mapping) {
                            Ok(pixels) => {
                                if let Some(under) = mean(pixels, copy.alpha) {
                                    let new = tone.tone.for_under(under);
                                    if new != tone.tone {
                                        tone.tone = new;
                                        self.redraw = true;
                                    }
                                }
                            }
                            Err(err) => {
                                tracing::debug!(?err, "could not read under a grip");
                            }
                        }
                    }
                    Ok(copy) => tone.pending = Some(copy),
                    Err(other) => tone.pending = Some(other),
                }
            }
            if tone.pending.is_some() || !headerless || managed.grip == Grip::Hidden {
                continue;
            }
            let Some(surface) = managed.window.wl_surface() else {
                continue;
            };
            let region = under_pill(managed.window.geometry());
            let copy = with_renderer_surface_state(&surface, |state| {
                let commit = state.current_commit();
                if tone.read == Some(commit) {
                    return None;
                }
                // Not drawn by this renderer (yet): read once it is.
                let texture = state.texture(context.clone())?.clone();
                let view = state.view()?;
                let area = state.buffer_size()?;
                tone.read = Some(commit);
                // From the surface to the part of the buffer it shows
                // (a viewport crops and scales), then to the buffer's
                // pixels.
                let k = (
                    view.src.size.w / f64::from(view.dst.w.max(1)),
                    view.src.size.h / f64::from(view.dst.h.max(1)),
                );
                let src = Rectangle::<f64, Logical>::new(
                    (
                        view.src.loc.x + region.loc.x * k.0,
                        view.src.loc.y + region.loc.y * k.1,
                    )
                        .into(),
                    (region.size.w * k.0, region.size.h * k.1).into(),
                );
                let pixels = src
                    .to_buffer(
                        f64::from(state.buffer_scale()),
                        state.buffer_transform(),
                        &area.to_f64(),
                    )
                    .to_i32_round()
                    .intersection(Rectangle::from_size(texture.size()))?;
                let alpha = Texture::format(&texture).is_none_or(has_alpha);
                Some((texture, pixels, alpha))
            })
            .flatten();
            let Some((texture, pixels, alpha)) = copy else {
                continue;
            };
            match renderer.copy_texture(&texture, pixels, Fourcc::Abgr8888) {
                Ok(mapping) => {
                    tone.pending = Some(Box::new(Reading::<R::TextureId, R::TextureMapping> {
                        context: context.clone(),
                        mapping,
                        alpha,
                    }));
                    // Read at the next frame, which this asks for.
                    self.redraw = true;
                }
                Err(err) => {
                    tracing::debug!(?err, "could not copy under a grip");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// KDE Connect's Kirigami header (#716, #719).
    const KIRIGAMI: [f64; 3] = [222.0, 224.0, 226.0];
    /// A dark header (Breeze Dark's).
    const DARK: [f64; 3] = [0x31 as f64, 0x36 as f64, 0x3b as f64];

    #[test]
    fn a_light_header_gets_the_dark_pill_and_a_dark_one_keeps_the_light_pill() {
        let light = Tone::Light.for_under(KIRIGAMI);
        assert_eq!(light, Tone::Dark);
        assert!(contrast(light.over(KIRIGAMI), KIRIGAMI) >= 3.0);
        let dark = Tone::Light.for_under(DARK);
        assert_eq!(dark, Tone::Light);
        assert!(contrast(dark.over(DARK), DARK) >= 3.0);
        // Back from a light header to a dark one.
        assert_eq!(Tone::Dark.for_under(DARK), Tone::Light);
    }

    #[test]
    fn the_mean_is_of_what_covers_and_none_when_mostly_clear() {
        let px = [222, 224, 226, 255, 111, 112, 113, 128];
        let m = mean(&px, true).expect("covered");
        assert!((m[0] - 222.0).abs() < 1.0, "{m:?}");
        assert_eq!(mean(&[0, 0, 0, 0, 0, 0, 0, 0], true), None);
        assert_eq!(mean(&[10, 10, 10, 0], false), Some([10.0, 10.0, 10.0]));
    }
}
