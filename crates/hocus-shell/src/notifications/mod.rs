//! Notifications (5.3): the desktop's server, `org.freedesktop.Notifications`
//! on the session bus the compositor names ([`server`]), and their bubbles
//! beside the bar ([`stack`] decides, [`draw`] draws).
//!
//! The server runs on a thread of its own and hands each notification,
//! owned, to [`input`]; so do the bubbles' surfaces with what the pointer
//! does, and the shell with the bars and the focused window's screen. The
//! [`effects`] stream runs the stack on what comes and on its deadlines,
//! and carries out what it says: layer-shell actions (iced runs them
//! without drawing the other surfaces), lines to the compositor (a
//! bubble's `wake`, `closing`, `raise`), and the signals, back to the
//! server's thread. Nothing here goes through iced's messages, which would
//! draw every surface again.

pub mod draw;
pub mod note;
mod server;
pub mod stack;

use std::{
    sync::{Mutex, PoisonError},
    time::Instant,
};

use hocus_link::{Form, ToCompositor};
use iced::{Element, Length, Rectangle, widget::canvas, window};
use iced_layershell::reexport::{
    Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption,
};
pub use stack::Input;
use stack::{Effect, Stack};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::Message;

/// The stack, and how the shell draws.
#[derive(Debug)]
pub struct Center {
    pub stack: Stack,
    /// The shell draws on the CPU: the bubbles are their cards alone.
    pub card: bool,
}

static CENTER: Mutex<Option<Center>> = Mutex::new(None);
static INPUTS: Mutex<Option<UnboundedSender<Input>>> = Mutex::new(None);
static TAKEN: Mutex<Option<UnboundedReceiver<Input>>> = Mutex::new(None);

/// `with` on the stack.
pub fn with<T>(with: impl FnOnce(&mut Center) -> T) -> T {
    let mut center = CENTER.lock().unwrap_or_else(PoisonError::into_inner);
    with(center.get_or_insert_with(|| Center {
        stack: Stack::new(false),
        card: false,
    }))
}

/// Starts the server, if the compositor named a bus; `software`: the
/// shell draws on the CPU.
pub fn start(software: bool) {
    with(|c| {
        c.stack = Stack::new(software);
        c.card = software;
    });
    let (sender, receiver) = unbounded_channel();
    *INPUTS.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
    *TAKEN.lock().unwrap_or_else(PoisonError::into_inner) = Some(receiver);
    server::start();
}

/// What happened, for the stack.
pub fn input(input: Input) {
    if let Some(sender) = INPUTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        let _ = sender.send(input);
    }
}

/// The compositor says the focused window's screen.
pub fn focus(screen: String) {
    input(Input::Focus(screen));
}

/// The bars changed: the bubbles keep beside them, and the dots on them.
pub fn bars_changed() {
    let bars: Vec<(String, Form)> = crate::popover::with(|c| c.bar_forms());
    input(Input::Bars(bars));
}

/// Runs the stack on what comes and on its deadlines, and carries out what
/// it says.
pub fn effects() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::SinkExt;
    iced::stream::channel(16, async |mut output| {
        let Some(mut inputs) = TAKEN.lock().unwrap_or_else(PoisonError::into_inner).take() else {
            std::future::pending::<()>().await;
            return;
        };
        loop {
            let deadline = with(|c| c.stack.deadline());
            let effects = match deadline {
                Some(at) => {
                    use iced::futures::future::{Either, select};
                    let due = std::pin::pin!(tokio::time::sleep_until(at.into()));
                    let next = std::pin::pin!(inputs.recv());
                    match select(next, due).await {
                        Either::Left((Some(input), _)) => {
                            with(|c| c.stack.handle(input, Instant::now()))
                        }
                        Either::Left((None, _)) => return,
                        Either::Right(((), _)) => with(|c| c.stack.tick(Instant::now())),
                    }
                }
                None => match inputs.recv().await {
                    Some(input) => with(|c| c.stack.handle(input, Instant::now())),
                    None => return,
                },
            };
            for message in effects.into_iter().flat_map(carry_out) {
                if output.send(message).await.is_err() {
                    return;
                }
            }
        }
    })
}

/// Wider than any surface: a region as large as can be.
const EVERYWHERE: i32 = 1 << 20;

type WlRegion = wayland_client::protocol::wl_region::WlRegion;

/// Surface `id` takes the pointer in `region` only, or nowhere.
fn region(id: window::Id, region: Option<Rectangle>) -> Message {
    let rect = region.map(|r| {
        let (x, y) = (r.x.floor() as i32, r.y.floor() as i32);
        let right = (r.x + r.width).ceil() as i32;
        let bottom = (r.y + r.height).ceil() as i32;
        (x, y, right - x, bottom - y)
    });
    let callback = iced_layershell::actions::ActionCallback::new(move |wl: &WlRegion| {
        wl.subtract(-EVERYWHERE, -EVERYWHERE, 2 * EVERYWHERE, 2 * EVERYWHERE);
        if let Some((x, y, w, h)) = rect {
            wl.add(x, y, w, h);
        }
    });
    Message::SetInputRegion { id, callback }
}

/// Tells the compositor; else (a shell alone) wakes every surface.
fn tell_or_wake(message: ToCompositor) -> Vec<Message> {
    if crate::popover::tell(message) {
        Vec::new()
    } else {
        vec![Message::Levels]
    }
}

/// An effect as messages for iced, a line to the compositor, or a signal.
fn carry_out(effect: Effect) -> Vec<Message> {
    match effect {
        Effect::Open {
            id,
            screen,
            namespace,
            size,
            margin,
            bottom,
            region: taken,
        } => {
            let pointer = taken.is_some();
            vec![
                Message::NewLayerShell {
                    settings: NewLayerShellSettings {
                        size: Some(size),
                        layer: Layer::Top,
                        anchor: if bottom {
                            Anchor::Right | Anchor::Bottom
                        } else {
                            Anchor::Right | Anchor::Top
                        },
                        exclusive_zone: Some(0),
                        margin: Some(margin),
                        keyboard_interactivity: KeyboardInteractivity::None,
                        output_option: if screen == crate::ALONE {
                            OutputOption::Active
                        } else {
                            OutputOption::OutputName(screen)
                        },
                        events_transparent: !pointer,
                        namespace: Some(namespace),
                    },
                    id,
                },
                region(id, taken),
            ]
        }
        Effect::Place {
            id,
            size,
            margin,
            region: taken,
        } => {
            let mut messages = vec![
                Message::SizeChange { id, size },
                Message::MarginChange { id, margin },
            ];
            if taken.is_some() {
                messages.push(region(id, taken));
            }
            messages
        }
        Effect::Wake(namespace) => tell_or_wake(ToCompositor::Wake(namespace)),
        Effect::Closing(namespace) => {
            crate::popover::tell(ToCompositor::Closing(namespace));
            Vec::new()
        }
        Effect::Remove(id) => vec![Message::RemoveWindow(id)],
        Effect::Signal(signal) => {
            server::signal(signal);
            Vec::new()
        }
        Effect::Raise { pid, app } => {
            crate::popover::tell(ToCompositor::Raise { pid, app });
            Vec::new()
        }
    }
}

/// What surface `id` shows, if it is one of the notifications': a bubble,
/// the chip, a dot. `glass`: the compositor blurs behind the shell.
pub fn view<'a>(id: window::Id, glass: bool) -> Option<Element<'a, Message>> {
    enum Is {
        Bubble,
        Chip,
        Dot(String),
    }
    let (is, card) = with(|c| {
        let is = if c.stack.bubble(id).is_some() {
            Some(Is::Bubble)
        } else if c.stack.chip(id).is_some() {
            Some(Is::Chip)
        } else {
            c.stack.dot(id).map(|(_, bar)| Is::Dot(bar))
        };
        (is, c.card)
    });
    let element: Element<'a, Message> = match is? {
        Is::Bubble => canvas(draw::Bubble { id, glass, card })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        Is::Chip => canvas(draw::Chip { id, card })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        Is::Dot(bar) => {
            // On the CPU, the bar's colour at the dot's height under it.
            let backdrop = card.then(|| {
                let height = crate::popover::with(|c| c.bar_height(&bar));
                let middle = (height - 30.0) / height.max(1.0);
                crate::mix(
                    crate::OPAQUE_TOP,
                    crate::OPAQUE_BOTTOM,
                    middle.clamp(0.0, 1.0),
                )
            });
            canvas(draw::Dot { id, backdrop })
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
    };
    Some(element)
}
