//! The toast a copied result leaves (5.2): "Copié : 6,214" at the bottom
//! of the launcher's screen for about 1.8 s, once the launcher closed.
//!
//! It is a small layer surface of the shell's
//! ([`hocus_link::LAUNCHER_TOAST_NAMESPACE`]), in the overlay layer, anchored to the
//! bottom 40 px up and centred over the space the bar leaves, taking
//! neither the keyboard nor the pointer. The shell draws it once and
//! removes it after [`LAUNCHER_TOAST`]; the compositor fades it in, rising
//! 8 px, and out, as the mockup's `toast-in`.

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::LAUNCHER_TOAST;
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Stroke},
        text::Alignment,
    },
    window,
};

use crate::{
    levels::draw::{LINE, TEXT},
    popover::{
        Effect,
        kit::{self, MANROPE_SEMIBOLD},
    },
};

/// Its text, 13 px 600, 10 px from its top and bottom, 16 from its sides,
/// in a 1 px rim, radius 12, 40 px from the screen's bottom.
const SIZE: f32 = 13.0;
const PAD: Size = Size::new(16.0, 10.0);
const RADIUS: f32 = 12.0;
pub const BOTTOM: i32 = 40;
const FILL: Color = Color::from_rgba8(14, 17, 28, 0.94);
const RIM: Color = Color::from_rgba8(108, 194, 135, 0.45);
/// The compositor's fade out ends a little after the shell's clock says:
/// it counts from the toast's first frame.
const LINGER: Duration = Duration::from_millis(100);

/// The toast shown.
#[derive(Debug, Clone)]
struct Shown {
    id: window::Id,
    said: String,
    until: Instant,
}

static TOAST: Mutex<Option<Shown>> = Mutex::new(None);

fn with<T>(with: impl FnOnce(&mut Option<Shown>) -> T) -> T {
    with(&mut TOAST.lock().unwrap_or_else(PoisonError::into_inner))
}

/// `said` shows on `output` from `now`, in place of a toast shown.
pub fn show(output: String, said: String, now: Instant) -> Vec<Effect> {
    let width = kit::measure(&said, SIZE, MANROPE_SEMIBOLD) + 2.0 * PAD.width + 2.0;
    let height = SIZE * LINE + 2.0 * PAD.height + 2.0;
    let id = window::Id::unique();
    let mut effects = Vec::new();
    with(|toast| {
        if let Some(old) = toast.take() {
            effects.push(Effect::Remove(old.id));
        }
        tracing::debug!(said, "the launcher's toast shows it");
        *toast = Some(Shown {
            id,
            said,
            until: now + LAUNCHER_TOAST + LINGER,
        });
    });
    effects.push(Effect::Toast {
        id,
        output,
        size: (width.ceil() as u32, height.ceil() as u32),
    });
    effects
}

/// Whether surface `id` is the toast's.
pub fn is_surface(id: window::Id) -> bool {
    with(|toast| toast.as_ref().is_some_and(|t| t.id == id))
}

/// When it goes.
pub fn deadline() -> Option<Instant> {
    with(|toast| toast.as_ref().map(|t| t.until))
}

/// What is due at `now`: the toast goes once shown long enough.
pub fn tick(now: Instant) -> Vec<Effect> {
    with(|toast| match toast.take() {
        Some(shown) if shown.until <= now => vec![Effect::Remove(shown.id)],
        kept => {
            *toast = kept;
            Vec::new()
        }
    })
}

/// The toast on its surface.
#[derive(Debug, Clone)]
pub struct View {
    pub id: window::Id,
    /// On the CPU, whose surfaces have no transparency: opaque, the
    /// compositor rounds its corners.
    pub software: bool,
}

impl<Message> canvas::Program<Message> for View {
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
        let said = with(|toast| {
            toast
                .as_ref()
                .filter(|t| t.id == self.id)
                .map(|t| t.said.clone())
        });
        if let Some(said) = said {
            draw(&mut frame, &said, bounds.size(), self.software);
        }
        vec![frame.into_geometry()]
    }
}

fn draw(f: &mut Frame, said: &str, size: Size, software: bool) {
    let rect = Rectangle::new(Point::ORIGIN, size);
    let fill = if software {
        Color { a: 1.0, ..FILL }
    } else {
        FILL
    };
    f.fill(&kit::rounded(rect, RADIUS), fill);
    let rim = Rectangle::new(
        Point::new(0.5, 0.5),
        Size::new(size.width - 1.0, size.height - 1.0),
    );
    f.stroke(
        &kit::rounded(rim, RADIUS - 0.5),
        Stroke::default().with_color(RIM).with_width(1.0),
    );
    f.fill_text(kit::text_in(
        MANROPE_SEMIBOLD,
        kit::text(
            said.to_owned(),
            rect.center(),
            SIZE,
            TEXT,
            Alignment::Center,
        ),
    ));
}
