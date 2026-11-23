//! The shell's layer surfaces opened, resized and closed through
//! iced_exwlshell's messages, in the shapes the shell says them: a size as
//! (width, height), 0 on an axis for "as long as its anchors make it", and
//! margins as (top, right, bottom, left).
//!
//! iced_exwlshell changes a surface's size only together with its anchor
//! (`LayoutChange`): each surface's anchor is kept from its opening for its
//! resizes, and forgotten as it closes.
//!
//! The text size (`[appearance] text-scale`, #928) is applied here alone.
//! The surfaces that follow it ([`hocus_link::follow_text_scale`]: the
//! bar's, the bubbles, the reminders, the launcher) say their sizes,
//! margins and input regions at 100 %, the mockups' pixels; here they are
//! asked of the compositor that many times larger, and iced draws them at
//! that scale ([`zoom`]), so that everything in them, laid out in the
//! mockups' pixels, grows alike. What they asked is kept, to be asked
//! again at a new text size ([`text_scale`]).

use std::{
    collections::HashMap,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU32, Ordering},
    },
};

use hocus_link::TextScale;
use iced::{Rectangle, window};
use iced_exwlshell::{
    InputRegion,
    actions::RegionActionCB,
    reexport::{Anchor, Extent, LayerSize, Margin, NewLayerShellSettings},
};

use crate::Message;

/// What a surface open was asked: its anchor, and, if it follows the
/// text size, its size, margins and input region at 100 %.
#[derive(Debug, Clone)]
struct Placed {
    anchor: Anchor,
    zoomed: Option<Asked>,
}

#[derive(Debug, Clone)]
struct Asked {
    size: LayerSize,
    margin: Option<Margin>,
    input: Option<Vec<Rectangle>>,
}

/// The surfaces open.
static PLACED: Mutex<Option<HashMap<window::Id, Placed>>> = Mutex::new(None);

/// The text size, in hundredths.
static TEXT: AtomicU32 = AtomicU32::new(100);

fn placed<T>(f: impl FnOnce(&mut HashMap<window::Id, Placed>) -> T) -> T {
    f(PLACED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(HashMap::new))
}

fn text() -> TextScale {
    TextScale(TEXT.load(Ordering::Relaxed))
}

/// A size as the shell says it: 0 on an axis leaves it to the anchors.
pub fn size((width, height): (u32, u32)) -> LayerSize {
    let extent = |px| Extent::try_px(px).unwrap_or(Extent::Fill);
    LayerSize {
        width: extent(width),
        height: extent(height),
    }
}

/// Margins as the shell says them: top, right, bottom, left.
pub fn margin((top, right, bottom, left): (i32, i32, i32, i32)) -> Margin {
    Margin {
        top,
        right,
        bottom,
        left,
    }
}

/// How many times larger than they say it the surfaces that follow the
/// text size are drawn.
pub fn text_factor() -> f32 {
    text().factor() as f32
}

/// How many times larger than it says it surface `id` is drawn: the text
/// size if it follows it, else 1. iced's scale factor for it.
pub fn zoom(id: window::Id) -> f32 {
    if placed(|placed| placed.get(&id).is_some_and(|p| p.zoomed.is_some())) {
        text().factor() as f32
    } else {
        1.0
    }
}

fn scaled_size(size: LayerSize, text: TextScale) -> LayerSize {
    let extent = |extent: Extent| match extent {
        Extent::Exact(px) => {
            let px = i32::try_from(px.get()).unwrap_or(i32::MAX);
            Extent::try_px(text.whole(px).max(1).unsigned_abs()).unwrap_or(Extent::Fill)
        }
        Extent::Fill => Extent::Fill,
    };
    LayerSize {
        width: extent(size.width),
        height: extent(size.height),
    }
}

fn scaled_margin(margin: Margin, text: TextScale) -> Margin {
    Margin {
        top: text.whole(margin.top),
        right: text.whole(margin.right),
        bottom: text.whole(margin.bottom),
        left: text.whole(margin.left),
    }
}

fn scaled_rect(r: Rectangle, text: TextScale) -> Rectangle {
    let k = text.factor() as f32;
    Rectangle {
        x: r.x * k,
        y: r.y * k,
        width: r.width * k,
        height: r.height * k,
    }
}

/// The room a bar in `form` takes from its screen's edge, in the
/// compositor's pixels at the text size: where a surface that keeps its
/// sizes (a capture's thumbnail) goes beside it.
pub fn bar_room(form: hocus_link::Form) -> i32 {
    let text = text();
    text.whole(form.width()) + text.whole(hocus_link::EDGE)
}

/// `r`, on a surface that follows the text size, in the compositor's
/// pixels: what the shell tells it of where things are on the bar.
pub fn told(r: Rectangle) -> [i32; 4] {
    let r = scaled_rect(r, text());
    [
        r.x.round() as i32,
        r.y.round() as i32,
        r.width.round() as i32,
        r.height.round() as i32,
    ]
}

/// The text size is now `to`: the surfaces that follow it, asked again at
/// it (their sizes, margins and input regions; iced draws them at it as
/// it takes them).
pub fn text_scale(to: TextScale) -> Vec<Message> {
    if TEXT.swap(to.0, Ordering::Relaxed) == to.0 {
        return Vec::new();
    }
    tracing::info!(text_scale = to.0, "the text size");
    let asked: Vec<(window::Id, Anchor, Asked)> = placed(|placed| {
        placed
            .iter()
            .filter_map(|(id, p)| Some((*id, p.anchor, p.zoomed.clone()?)))
            .collect()
    });
    let mut messages = Vec::new();
    for (id, anchor, asked) in asked {
        messages.push(Message::LayoutChange {
            id,
            anchor,
            size: scaled_size(asked.size, to),
        });
        if let Some(margin) = asked.margin {
            messages.push(Message::MarginChange {
                id,
                margin: scaled_margin(margin, to),
            });
        }
        if let Some(rects) = asked.input {
            messages.push(region(id, rects.into_iter().map(|r| scaled_rect(r, to))));
        }
    }
    messages
}

/// Opens surface `id` with `settings`, at the text size if its namespace
/// follows it.
pub fn open(id: window::Id, mut settings: NewLayerShellSettings) -> Message {
    let follows = settings
        .namespace
        .as_deref()
        .is_some_and(hocus_link::follow_text_scale);
    let zoomed = follows.then_some(Asked {
        size: settings.size,
        margin: settings.margin,
        input: None,
    });
    if follows {
        let text = text();
        settings.size = scaled_size(settings.size, text);
        settings.margin = settings.margin.map(|m| scaled_margin(m, text));
    }
    let anchor = settings.anchor;
    placed(|placed| placed.insert(id, Placed { anchor, zoomed }));
    Message::NewLayerShell { settings, id }
}

/// Surface `id` to `size`, against the anchor it opened with; nothing if
/// it is not open.
pub fn resize(id: window::Id, to: (u32, u32)) -> Option<Message> {
    let text = text();
    let asked = size(to);
    let Some((anchor, size)) = placed(|placed| {
        let p = placed.get_mut(&id)?;
        let size = match p.zoomed.as_mut() {
            Some(zoomed) => {
                zoomed.size = asked;
                scaled_size(asked, text)
            }
            None => asked,
        };
        Some((p.anchor, size))
    }) else {
        tracing::debug!(?id, "a size for a surface not open");
        return None;
    };
    Some(Message::LayoutChange { id, anchor, size })
}

/// Surface `id`'s new margins (top, right, bottom, left), at the text
/// size if it follows it.
pub fn moved(id: window::Id, to: (i32, i32, i32, i32)) -> Message {
    let text = text();
    let asked = margin(to);
    let margin = placed(
        |placed| match placed.get_mut(&id).and_then(|p| p.zoomed.as_mut()) {
            Some(zoomed) => {
                zoomed.margin = Some(asked);
                scaled_margin(asked, text)
            }
            None => asked,
        },
    );
    Message::MarginChange { id, margin }
}

/// Wider than any surface: a region as large as can be.
const EVERYWHERE: f32 = (1 << 20) as f32;

/// All of a surface, as an input region.
pub const ALL: Rectangle = Rectangle {
    x: 0.0,
    y: 0.0,
    width: EVERYWHERE,
    height: EVERYWHERE,
};

/// Surface `id` takes the pointer in `rects` only (each grown to whole
/// pixels), nowhere if there are none; at the text size if it follows it.
pub fn input(id: window::Id, rects: impl IntoIterator<Item = Rectangle>) -> Message {
    let rects: Vec<Rectangle> = rects.into_iter().collect();
    let text = text();
    let follows = placed(
        |placed| match placed.get_mut(&id).and_then(|p| p.zoomed.as_mut()) {
            Some(zoomed) => {
                zoomed.input = Some(rects.clone());
                true
            }
            None => false,
        },
    );
    if follows {
        region(id, rects.into_iter().map(|r| scaled_rect(r, text)))
    } else {
        region(id, rects)
    }
}

/// Surface `id`'s input region, `rects` in its own pixels.
fn region(id: window::Id, rects: impl IntoIterator<Item = Rectangle>) -> Message {
    let rects: Vec<(i32, i32, i32, i32)> = rects
        .into_iter()
        .map(|r| {
            let (x, y) = (r.x.floor() as i32, r.y.floor() as i32);
            let right = (r.x + r.width).ceil() as i32;
            let bottom = (r.y + r.height).ceil() as i32;
            (x, y, right - x, bottom - y)
        })
        .collect();
    // Each region iced_exwlshell hands is new, empty.
    let callback = RegionActionCB::new(move |region: &mut InputRegion| {
        for &(x, y, width, height) in &rects {
            if let Err(err) = region.add(x, y, width, height) {
                tracing::warn!(%err, "a surface's input region");
            }
        }
    });
    Message::SetInputRegion { id, callback }
}

/// Closes surface `id`.
pub fn close(id: window::Id) -> Message {
    closed(id);
    Message::RemoveWindow(id)
}

/// Surface `id` is gone, closed by the shell or by the compositor.
pub fn closed(id: window::Id) {
    placed(|placed| placed.remove(&id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_leaves_its_axis_to_the_anchors() {
        assert_eq!(size((0, 0)), LayerSize::FILL);
        assert_eq!(size((300, 0)), LayerSize::fill_height(300));
        assert_eq!(size((300, 40)), LayerSize::px(300, 40));
    }

    #[test]
    fn a_surface_resizes_against_its_anchor_until_it_closes() {
        let id = window::Id::unique();
        let anchor = Anchor::Right | Anchor::Bottom;
        let settings = NewLayerShellSettings {
            anchor,
            ..NewLayerShellSettings::default()
        };
        assert!(matches!(open(id, settings), Message::NewLayerShell { .. }));
        assert!(matches!(
            resize(id, (10, 20)),
            Some(Message::LayoutChange { anchor: a, size: s, .. }) if a == anchor && s == LayerSize::px(10, 20)
        ));
        assert!(matches!(close(id), Message::RemoveWindow(i) if i == id));
        assert!(resize(id, (10, 20)).is_none());
    }

    /// #928: a surface that follows the text size is asked of the
    /// compositor at it, opened and moved, and again when it changes; iced
    /// draws it at it. One that keeps its sizes is asked as it says.
    #[test]
    fn the_text_size_scales_the_surfaces_that_follow_it_only() {
        let open_as = |namespace: &str| {
            let id = window::Id::unique();
            let settings = NewLayerShellSettings {
                size: size((280, 0)),
                margin: Some(margin((8, 212, 0, 0))),
                namespace: Some(namespace.to_owned()),
                ..NewLayerShellSettings::default()
            };
            let opened = match open(id, settings) {
                Message::NewLayerShell { settings, .. } => (settings.size, settings.margin),
                _ => panic!("not an opening"),
            };
            (id, opened)
        };
        let _ = text_scale(TextScale(130));
        let (bubble, opened) = open_as(&hocus_link::bubble_namespace(4));
        assert_eq!(opened, (size((364, 0)), Some(margin((10, 276, 0, 0)))));
        assert!((zoom(bubble) - 1.3).abs() < 1e-6);
        let (lock, opened) = open_as(hocus_link::LOCK_NAMESPACE);
        assert_eq!(opened, (size((280, 0)), Some(margin((8, 212, 0, 0)))));
        assert!((zoom(lock) - 1.0).abs() < f32::EPSILON);
        assert!(matches!(
            resize(bubble, (100, 40)),
            Some(Message::LayoutChange { size: s, .. }) if s == size((130, 52))
        ));
        assert!(matches!(
            moved(bubble, (20, 0, 0, 0)),
            Message::MarginChange { margin: m, .. } if m == margin((26, 0, 0, 0))
        ));
        let _ = input(
            bubble,
            [Rectangle::new(
                iced::Point::ORIGIN,
                iced::Size::new(10.0, 10.0),
            )],
        );
        // Back to 100 %: the bubble is asked again as it said, the lock
        // not at all.
        let again = text_scale(TextScale::NORMAL);
        assert_eq!(again.len(), 3, "its size, its margins, its input region");
        assert!(again.iter().all(|m| match m {
            Message::LayoutChange { id, size: s, .. } => *id == bubble && *s == size((100, 40)),
            Message::MarginChange { id, margin: m } => *id == bubble && *m == margin((20, 0, 0, 0)),
            Message::SetInputRegion { id, .. } => *id == bubble,
            _ => false,
        }));
        assert!((zoom(bubble) - 1.0).abs() < f32::EPSILON);
        assert!(text_scale(TextScale::NORMAL).is_empty(), "nothing changed");
        assert_eq!(
            told(Rectangle::new(
                iced::Point::new(-10.0, 4.0),
                iced::Size::new(2.0, 2.0)
            )),
            [-10, 4, 2, 2]
        );
    }
}
