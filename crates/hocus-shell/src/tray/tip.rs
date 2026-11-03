//! A tray icon's tooltip (5.14): after 400 ms on an icon, "Name · what it
//! says" (the name in bold, 12 px) beside the bar, 8 px from it as the
//! popovers are, centred on the icon: 7 px by 10 inside, radius 9,
//! `rgba(14,17,28,0.96)` in a 1 px `rgba(255,255,255,0.12)` rim, the
//! mockup's. A surface of its own that takes no pointer, gone as the
//! pointer leaves the icon or presses it.

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{EDGE, TIP_NAMESPACE, TIP_RADIUS, ToCompositor};
use iced::{
    Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke},
        text::Alignment,
    },
    window,
};

use crate::{
    Message,
    levels::draw::{MANROPE, MANROPE_BOLD, TEXT},
    popover::{self, Effect, kit},
};

/// Resting this long on an icon shows its tooltip.
pub const DELAY: Duration = Duration::from_millis(400);
const PAD: Size = Size::new(10.0, 7.0);
const BORDER: f32 = 1.0;
const SIZE: f32 = 12.0;
const FILL: Color = Color::from_rgba8(14, 17, 28, 0.96);
const RIM: Color = Color::from_rgba8(255, 255, 255, 0.12);
/// What comes between the name and what the item says.
const BETWEEN: &str = " · ";

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Idle,
    /// The pointer rests on icon `key` of `bar` (at `rect`, the bar's
    /// frame), since then.
    Waiting {
        bar: String,
        key: u32,
        rect: Rectangle,
        since: Instant,
    },
    Shown {
        id: window::Id,
        key: u32,
    },
}

static PHASE: Mutex<Phase> = Mutex::new(Phase::Idle);

fn phase<T>(with: impl FnOnce(&mut Phase) -> T) -> T {
    with(&mut PHASE.lock().unwrap_or_else(PoisonError::into_inner))
}

/// The pointer is on icon `key` of `bar`, at `rect`: its tooltip shows in
/// 400 ms.
pub fn hover(bar: &str, key: u32, rect: Rectangle) {
    let gone = phase(|p| {
        let gone = match p {
            Phase::Shown { id, .. } => Some(*id),
            _ => None,
        };
        *p = Phase::Waiting {
            bar: bar.to_owned(),
            key,
            rect,
            since: Instant::now(),
        };
        gone
    });
    // The effects' stream waits for the new deadline.
    popover::send(gone.map(Effect::Remove).into_iter().collect());
}

/// The pointer left the icons, or pressed one: no tooltip.
pub fn leave() {
    let gone = phase(|p| match std::mem::replace(p, Phase::Idle) {
        Phase::Shown { id, .. } => Some(id),
        _ => None,
    });
    popover::send(gone.map(Effect::Remove).into_iter().collect());
}

/// When the tooltip is due.
pub fn deadline() -> Option<Instant> {
    phase(|p| match p {
        Phase::Waiting { since, .. } => Some(*since + DELAY),
        _ => None,
    })
}

/// What its icon's tooltip says: its name, and what it says after.
fn said(key: u32) -> Option<(String, String)> {
    super::shown()
        .into_iter()
        .find(|s| s.key == key)
        .map(|s| (s.name, s.tip))
}

/// The tooltip's size for `name` and `tip`.
fn size(name: &str, tip: &str) -> (u32, u32) {
    let mut width = kit::measure(name, SIZE, MANROPE_BOLD);
    if !tip.is_empty() {
        width += kit::measure(&format!("{BETWEEN}{tip}"), SIZE, MANROPE);
    }
    let w = width + 2.0 * (PAD.width + BORDER);
    let h = SIZE * crate::levels::draw::LINE + 2.0 * (PAD.height + BORDER);
    (w.ceil() as u32, h.round() as u32)
}

/// What is due at `now`: the tooltip opens.
pub fn tick(now: Instant) -> Vec<Effect> {
    let due = phase(|p| match p {
        Phase::Waiting {
            bar,
            key,
            rect,
            since,
        } if *since + DELAY <= now => Some((bar.clone(), *key, *rect)),
        _ => None,
    });
    let Some((bar, key, rect)) = due else {
        return Vec::new();
    };
    let Some((name, tip)) = said(key) else {
        phase(|p| *p = Phase::Idle);
        return Vec::new();
    };
    let form = popover::with(|c| c.bar_forms())
        .into_iter()
        .find(|(b, _)| *b == bar)
        .map(|(_, f)| f);
    let Some(form) = form else {
        phase(|p| *p = Phase::Idle);
        return Vec::new();
    };
    let (w, h) = size(&name, &tip);
    let top = (EDGE as f32 + rect.center_y() - h as f32 / 2.0).round() as i32;
    let margin = (top.max(EDGE), EDGE + form.width() + EDGE, 0, 0);
    let id = window::Id::unique();
    phase(|p| *p = Phase::Shown { id, key });
    tracing::debug!(name, tip, "the tray's tooltip shows");
    vec![
        Effect::Layer {
            id,
            bar,
            namespace: TIP_NAMESPACE,
            size: (w, h),
            margin,
        },
        Effect::Region {
            id,
            rects: Vec::new(),
        },
    ]
}

/// What the icons say changed: the tooltip shown says it.
pub fn changed() {
    if phase(|p| matches!(p, Phase::Shown { .. })) {
        popover::tell(ToCompositor::Wake(TIP_NAMESPACE.to_owned()));
    }
}

/// The tooltip, if its surface is `id`.
pub fn view<'a>(id: window::Id) -> Option<Element<'a, Message>> {
    let key = phase(|p| match p {
        Phase::Shown { id: shown, key } if *shown == id => Some(*key),
        _ => None,
    })?;
    Some(
        iced::widget::canvas(Tip { key })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    )
}

struct Tip {
    key: u32,
}

impl canvas::Program<Message> for Tip {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.size();
        let radius = TIP_RADIUS as f32;
        frame.fill(
            &Path::rounded_rectangle(Point::ORIGIN, size, radius.into()),
            FILL,
        );
        frame.stroke(
            &Path::rounded_rectangle(
                Point::new(BORDER / 2.0, BORDER / 2.0),
                Size::new(size.width - BORDER, size.height - BORDER),
                (radius - BORDER / 2.0).into(),
            ),
            Stroke::default().with_color(RIM).with_width(BORDER),
        );
        let Some((name, tip)) = said(self.key) else {
            return vec![frame.into_geometry()];
        };
        let middle = size.height / 2.0;
        let x = BORDER + PAD.width;
        let name_width = kit::measure(&name, SIZE, MANROPE_BOLD);
        frame.fill_text(kit::text_in(
            MANROPE_BOLD,
            kit::text(name, Point::new(x, middle), SIZE, TEXT, Alignment::Left),
        ));
        if !tip.is_empty() {
            frame.fill_text(kit::text(
                format!("{BETWEEN}{tip}"),
                Point::new(x + name_width, middle),
                SIZE,
                TEXT,
                Alignment::Left,
            ));
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_due_400_ms_after_the_pointer_came_to_rest() {
        let rect = Rectangle::new(Point::new(-50.0, 900.0), Size::new(34.0, 34.0));
        hover("DP-1", 3, rect);
        let at = deadline().expect("due");
        let since = at - DELAY;
        assert!(since.elapsed() < Duration::from_secs(5));
        // Not yet: nothing; the icon gone: nothing either.
        assert!(tick(since + Duration::from_millis(399)).is_empty());
        assert!(tick(at).is_empty());
        assert_eq!(deadline(), None);
        leave();
        assert_eq!(deadline(), None);
    }
}
