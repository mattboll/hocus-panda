//! The pointer's cursor: the client's own surface when it sets one, else a
//! shape by its name (the client's, through cursor-shape-v1, or the
//! compositor's over a window's frame), drawn at each output's scale.
//!
//! Shapes come from the system's Xcursor theme, as applications' do:
//! `[appearance] cursor-theme` and `cursor-size`, else `XCURSOR_THEME` and
//! `XCURSOR_SIZE`, else `default` and the themes it inherits (often
//! Adwaita). The theme's files are read on a thread of its own: a shape is
//! asked for at a size the first time it shows (the compositor's own
//! shapes all at once with the first), and its images come back to the
//! loop. Until then, when the theme has no image for a shape, or when
//! there is no theme, the compositor draws its own ([`drawn`]). Each
//! output shows the image whose size is nearest the cursor's size times
//! its scale, pixel for pixel, so that it is sharp at 1, 1.5 or 2. An
//! animated cursor (`wait`, `progress`) shows its frames on the monotonic
//! clock: the loop wakes when the next one is due, and draws nothing in
//! between.
//!
//! The programs the desktop starts, XWayland's among them, are given the
//! theme and size (`XCURSOR_THEME`, `XCURSOR_SIZE`), so that theirs
//! match.

mod drawn;
pub mod theme;

use std::{cell::RefCell, collections::HashMap, path::PathBuf, sync::mpsc, time::Duration};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            ImportAll, ImportMem, Renderer, Texture,
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                render_elements,
                surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            },
        },
    },
    input::pointer::{CursorIcon, CursorImageStatus, CursorImageSurfaceData},
    output::Output,
    reexports::{
        calloop::{
            LoopHandle,
            channel::{self, Event},
        },
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::{Logical, Physical, Point, Rectangle, Size, Transform},
    wayland::compositor::with_states,
};

pub use theme::{Choice, CursorEnv};

use crate::{App, State};
use drawn::{Drawn, Shape};
use theme::Frame;

render_elements! {
    pub CursorElement<R> where R: ImportAll + ImportMem;
    Surface=WaylandSurfaceRenderElement<R>,
    Arrow=MemoryRenderBufferRenderElement<R>,
}

/// The shapes the compositor shows itself, asked of the theme together
/// the first time a size is: none is drawn before the theme's while the
/// pointer crosses a frame.
const OWN: [CursorIcon; 7] = [
    CursorIcon::Default,
    CursorIcon::EwResize,
    CursorIcon::NsResize,
    CursorIcon::NwseResize,
    CursorIcon::NeswResize,
    CursorIcon::Grab,
    CursorIcon::Grabbing,
];

/// The cursor's images: the theme's as they come, and the drawn ones.
#[derive(Debug)]
pub struct CursorImages {
    choice: Choice,
    theme: RefCell<Theme>,
    /// The theme's frames, by shape and size in pixels asked.
    frames: RefCell<HashMap<(CursorIcon, u32), Slot>>,
    /// Frames made into buffers for an output's scale: by shape, size
    /// asked, scale in thousandths, and frame.
    placed: RefCell<HashMap<(CursorIcon, u32, u32, usize), Placed>>,
    drawn: Drawn,
}

#[derive(Debug)]
enum Theme {
    /// None: the cursors are drawn.
    None,
    /// Its thread loads what it is asked.
    Loading(mpsc::Sender<(CursorIcon, u32)>),
}

#[derive(Debug)]
enum Slot {
    Asked,
    /// Not in the theme: drawn.
    Missing,
    Loaded(Vec<Frame>),
}

/// What the theme's thread sends the loop.
#[derive(Debug)]
enum Loaded {
    NoTheme,
    Frames {
        icon: CursorIcon,
        pixels: u32,
        frames: Option<Vec<Frame>>,
    },
}

/// A frame for one output's scale: its pixels on a canvas a whole number
/// of logical pixels wide, so that it is shown pixel for pixel.
#[derive(Debug, Clone)]
struct Placed {
    buffer: MemoryRenderBuffer,
    canvas: (i32, i32),
    size: Size<i32, Logical>,
    hotspot: (u32, u32),
}

impl CursorImages {
    /// The cursor in `choice`'s theme and size, its files looked for in
    /// `search` on a thread of its own; drawn with no search path.
    pub fn start(handle: &LoopHandle<'static, App>, choice: Choice, search: Vec<PathBuf>) -> Self {
        let theme = if search.is_empty() {
            Theme::None
        } else {
            match load_theme(handle, &choice, search) {
                Ok(asks) => Theme::Loading(asks),
                Err(err) => {
                    tracing::warn!(%err, "could not load the cursor theme; cursors drawn");
                    Theme::None
                }
            }
        };
        Self {
            choice,
            theme: RefCell::new(theme),
            frames: RefCell::default(),
            placed: RefCell::default(),
            drawn: Drawn::default(),
        }
    }

    pub fn choice(&self) -> &Choice {
        &self.choice
    }

    /// The theme's frame of `icon` at `now` for an output at `scale`, the
    /// image nearest `pixels` wide; `None` to draw it. The first time,
    /// asks the theme for it, and shows the same shape at another size
    /// meanwhile when there is one.
    fn themed(&self, icon: CursorIcon, pixels: u32, scale: f64, now: Duration) -> Option<Placed> {
        if matches!(*self.theme.borrow(), Theme::None) {
            return None;
        }
        let mut frames = self.frames.borrow_mut();
        let key = match frames.get(&(icon, pixels)) {
            Some(Slot::Loaded(_)) => pixels,
            Some(Slot::Missing) => return None,
            Some(Slot::Asked) => nearest_loaded(&frames, icon, pixels)?,
            None => {
                self.ask(&mut frames, icon, pixels);
                nearest_loaded(&frames, icon, pixels)?
            }
        };
        let Some(Slot::Loaded(loaded)) = frames.get(&(icon, key)) else {
            return None;
        };
        let (index, _) = theme::frame_at(loaded, now);
        let at_scale = (scale * 1000.0).round() as u32;
        let placed = self
            .placed
            .borrow_mut()
            .entry((icon, key, at_scale, index))
            .or_insert_with(|| place(&loaded[index], scale))
            .clone();
        Some(placed)
    }

    /// Asks the theme's thread for `icon` at `pixels`, and for the
    /// compositor's own shapes too when that size is new.
    fn ask(&self, frames: &mut HashMap<(CursorIcon, u32), Slot>, icon: CursorIcon, pixels: u32) {
        let theme = self.theme.borrow();
        let Theme::Loading(asks) = &*theme else {
            return;
        };
        let new_size = !frames.keys().any(|&(_, p)| p == pixels);
        let mut wanted = vec![icon];
        if new_size {
            wanted.extend(OWN.iter().filter(|&&own| own != icon));
        }
        for icon in wanted {
            let slot = match asks.send((icon, pixels)) {
                Ok(()) => Slot::Asked,
                // The thread is gone: drawn.
                Err(_) => Slot::Missing,
            };
            frames.insert((icon, pixels), slot);
        }
    }

    /// When the shown frame of an animated `icon` changes next.
    fn next_frame(&self, icon: CursorIcon, now: Duration) -> Option<Duration> {
        self.frames
            .borrow()
            .iter()
            .filter(|((i, _), _)| *i == icon)
            .filter_map(|(_, slot)| match slot {
                Slot::Loaded(frames) => theme::frame_at(frames, now).1,
                _ => None,
            })
            .min()
    }

    /// The theme's thread sent something: whether the cursor shown may
    /// look different.
    fn loaded(&self, loaded: Loaded, shown: Option<CursorIcon>) -> bool {
        match loaded {
            Loaded::NoTheme => {
                *self.theme.borrow_mut() = Theme::None;
                self.frames.borrow_mut().clear();
                false
            }
            Loaded::Frames {
                icon,
                pixels,
                frames,
            } => {
                let slot = frames.map_or(Slot::Missing, Slot::Loaded);
                self.frames.borrow_mut().insert((icon, pixels), slot);
                shown == Some(icon)
            }
        }
    }
}

/// The size nearest `pixels` that `icon` is loaded at.
fn nearest_loaded(
    frames: &HashMap<(CursorIcon, u32), Slot>,
    icon: CursorIcon,
    pixels: u32,
) -> Option<u32> {
    frames
        .iter()
        .filter(|((i, _), slot)| *i == icon && matches!(slot, Slot::Loaded(_)))
        .map(|((_, p), _)| *p)
        .min_by_key(|p| p.abs_diff(pixels))
}

/// Starts the theme's thread: it finds the theme, then loads what the
/// loop asks for, sending it back through the loop's channel.
fn load_theme(
    handle: &LoopHandle<'static, App>,
    choice: &Choice,
    search: Vec<PathBuf>,
) -> anyhow::Result<mpsc::Sender<(CursorIcon, u32)>> {
    let (sender, receiver) = channel::channel::<Loaded>();
    handle
        .insert_source(receiver, |event, _, app| {
            if let Event::Msg(loaded) = event {
                app.state.cursor_loaded(loaded);
            }
        })
        .map_err(|err| anyhow::anyhow!("{}", err.error))?;
    let (asks, asked) = mpsc::channel::<(CursorIcon, u32)>();
    let name = choice.theme.clone();
    std::thread::Builder::new()
        .name("cursor theme".into())
        .spawn(move || {
            let dirs = theme::theme_dirs(&search, &name);
            if dirs.is_empty() {
                tracing::warn!(
                    theme = name,
                    ?search,
                    "no cursor theme found; cursors drawn"
                );
                let _ = sender.send(Loaded::NoTheme);
                return;
            }
            tracing::info!(theme = name, ?dirs, "cursor theme");
            for (icon, pixels) in asked {
                let mut names = theme::names(icon);
                // A shape not drawn otherwise than as the arrow: the
                // theme's arrow rather than the drawn one.
                if Shape::of(icon) == Shape::Arrow && icon != CursorIcon::Default {
                    names.extend(theme::names(CursorIcon::Default));
                }
                let frames = theme::find(&dirs, &names).and_then(|p| theme::load(&p, pixels));
                if frames.is_none() {
                    tracing::debug!(%icon, "not in the cursor theme; drawn");
                }
                let sent = sender.send(Loaded::Frames {
                    icon,
                    pixels,
                    frames,
                });
                if sent.is_err() {
                    return;
                }
            }
        })?;
    Ok(asks)
}

/// The logical size, from `pixels` up, that is a whole number of
/// pixels at `scale`; `pixels / scale` rounded up when none is near.
fn logical_fit(pixels: u32, scale: f64) -> i32 {
    let start = (f64::from(pixels) / scale).ceil().max(1.0) as i32;
    (start..start + 8)
        .find(|&l| {
            let p = f64::from(l) * scale;
            (p - p.round()).abs() < 1e-6
        })
        .unwrap_or(start)
}

/// `frame` on a transparent canvas whose size is a whole number of
/// logical pixels at `scale`, its image in the top left corner.
fn place(frame: &Frame, scale: f64) -> Placed {
    let size = Size::from((
        logical_fit(frame.width, scale),
        logical_fit(frame.height, scale),
    ));
    let canvas = |l: i32, px: u32| ((f64::from(l) * scale).round() as i32).max(px as i32);
    let (cw, ch) = (canvas(size.w, frame.width), canvas(size.h, frame.height));
    let mut pixels = vec![0u8; (cw * ch * 4) as usize];
    let row = (frame.width * 4) as usize;
    for (y, line) in frame.pixels.chunks_exact(row).enumerate() {
        let at = y * cw as usize * 4;
        pixels[at..at + row].copy_from_slice(line);
    }
    Placed {
        buffer: MemoryRenderBuffer::from_slice(
            &pixels,
            Fourcc::Argb8888,
            (cw, ch),
            1,
            Transform::Normal,
            None,
        ),
        canvas: (cw, ch),
        size,
        hotspot: frame.hotspot,
    }
}

/// What the pointer shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shown<'a> {
    Hidden,
    /// A client's own image.
    Surface(&'a WlSurface),
    /// A shape by its name, the client's or the compositor's.
    Named(CursorIcon),
}

impl State {
    /// What the pointer shows: what the client under it set.
    pub fn shown_cursor(&self) -> Shown<'_> {
        if let Some(icon) = self.frame_cursor {
            return Shown::Named(icon);
        }
        match &self.cursor {
            CursorImageStatus::Hidden => Shown::Hidden,
            CursorImageStatus::Surface(surface) => Shown::Surface(surface),
            CursorImageStatus::Named(icon) => Shown::Named(*icon),
        }
    }

    /// The theme's thread sent images: drawn when they are the shown
    /// cursor's, and the compositor draws it.
    fn cursor_loaded(&mut self, loaded: Loaded) {
        let shown = match self.shown_cursor() {
            Shown::Named(icon) => Some(icon),
            _ => None,
        };
        if self.cursor_images.loaded(loaded, shown) && !self.nested {
            self.redraw = true;
        }
    }

    /// When the cursor's next frame is due, when it is animated and the
    /// compositor draws it.
    pub fn cursor_next_frame(&self, now: Duration) -> Option<Duration> {
        match self.shown_cursor() {
            Shown::Named(icon) if !self.nested => self.cursor_images.next_frame(icon, now),
            _ => None,
        }
    }

    /// The cursor's elements on `output`, front first; none when the pointer
    /// is elsewhere or the client hid it.
    pub fn cursor_elements<R>(&self, renderer: &mut R, output: &Output) -> Vec<CursorElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let Some(pointer) = self.seat.get_pointer().map(|p| p.current_location()) else {
            return Vec::new();
        };
        let scale = output.current_scale().fractional_scale();
        let size = f64::from(self.cursor_images.choice.size);
        // A theme's image may be larger than its size says.
        let reach = size * 2.0;
        let near = Rectangle::<f64, Logical>::new(
            pointer - Point::from((reach, reach)),
            Size::from((reach * 2.0, reach * 2.0)),
        );
        if !near.overlaps(area.to_f64()) {
            return Vec::new();
        }
        let on_output = pointer - area.loc.to_f64();
        let element = match self.shown_cursor() {
            Shown::Hidden => return Vec::new(),
            Shown::Surface(surface) => {
                let hotspot = with_states(surface, |states| {
                    states
                        .data_map
                        .get::<CursorImageSurfaceData>()
                        .and_then(|data| data.lock().ok().map(|d| d.hotspot))
                        .unwrap_or_default()
                });
                let at: Point<i32, Physical> = (on_output - hotspot.to_f64())
                    .to_physical(scale)
                    .to_i32_round();
                return render_elements_from_surface_tree(
                    renderer,
                    surface,
                    at,
                    scale,
                    1.0,
                    Kind::Cursor,
                );
            }
            Shown::Named(icon) => {
                let pixels = (size * scale).round().max(1.0) as u32;
                match self.cursor_images.themed(icon, pixels, scale, self.now()) {
                    Some(placed) => {
                        let hotspot =
                            Point::from((f64::from(placed.hotspot.0), f64::from(placed.hotspot.1)));
                        // On whole pixels: the image is shown pixel for
                        // pixel.
                        let at = on_output.to_physical(scale) - hotspot;
                        let at = Point::<f64, Physical>::from((at.x.round(), at.y.round()));
                        let (w, h) = placed.canvas;
                        let all = Rectangle::from_size(Size::from((f64::from(w), f64::from(h))));
                        MemoryRenderBufferRenderElement::from_buffer(
                            renderer,
                            at,
                            &placed.buffer,
                            None,
                            Some(all),
                            Some(placed.size),
                            Kind::Cursor,
                        )
                    }
                    None => {
                        let shape = Shape::of(icon);
                        // The drawn shapes are in units of 24: at the
                        // cursor's size.
                        let unit = size / f64::from(drawn::SIZE);
                        let (buffer, px) = self.cursor_images.drawn.at_scale(shape, scale * unit);
                        let (x, y) = shape.hotspot();
                        let at = (on_output - Point::from((x * unit, y * unit))).to_physical(scale);
                        let all = Rectangle::from_size(Size::from((f64::from(px), f64::from(px))));
                        let logical = self.cursor_images.choice.size as i32;
                        MemoryRenderBufferRenderElement::from_buffer(
                            renderer,
                            at,
                            &buffer,
                            None,
                            Some(all),
                            Some(Size::from((logical, logical))),
                            Kind::Cursor,
                        )
                    }
                }
            }
        };
        element
            .inspect_err(|err| tracing::warn!(?err, "could not draw the cursor"))
            .map(|element| vec![CursorElement::Arrow(element)])
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_shows_pixel_for_pixel_at_any_scale() {
        // 24 px at 1, 36 at 1.5, 48 at 2: whole logical sizes.
        assert_eq!(logical_fit(24, 1.0), 24);
        assert_eq!(logical_fit(36, 1.5), 24);
        assert_eq!(logical_fit(48, 2.0), 24);
        // 32 px at 1.5: 22 logical pixels are 33, one more than the image.
        assert_eq!(logical_fit(32, 1.5), 22);
        assert_eq!(logical_fit(30, 1.25), 24);
        let frame = Frame {
            width: 32,
            height: 32,
            hotspot: (4, 4),
            delay: Duration::ZERO,
            pixels: vec![0xff; 32 * 32 * 4],
        };
        let placed = place(&frame, 1.5);
        assert_eq!(placed.canvas, (33, 33));
        assert_eq!(placed.size, Size::from((22, 22)));
    }
}
