//! The pointer's shape, set again each time the pointer enters one of the
//! shell's surfaces (#553).
//!
//! iced_layershell sets a surface's shape only when the shape its widgets
//! ask for changes, and asks them once for all the events it read
//! together. When the pointer leaves a surface and comes back before the
//! shell has handled the leave (both read at once, the shell busy
//! meanwhile), the shape asked for is the one it set last, and it sets
//! nothing: the pointer keeps the shape the other client gave it, an
//! arrow or none over a button that wants a hand.
//!
//! So the shell has a pointer of its own on iced's connection, which the
//! compositor tells the same enters, with a cursor-shape device on it.
//! Every surface's widgets are wrapped in [`Entered`], which sees each
//! enter among their events and, as iced asks for the shape they make,
//! sets that shape with the enter's serial. iced_layershell reads the
//! socket for both pointers and hands the widgets their events after the
//! read: the shell's own enter is there by then.
//!
//! Nothing here reads the connection, nor wakes: the pointer's events
//! are handled as the widgets get theirs.

use std::{
    cell::Cell,
    sync::{Mutex, PoisonError},
};

use iced::{
    Element, Event, Length, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree, tree},
    },
};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_pointer::{self, WlPointer},
        wl_registry::WlRegistry,
        wl_seat::{self, WlSeat},
    },
};
use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1},
    wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
};

/// Makes the pointer on `connection`, the one iced draws on.
pub fn start(connection: &Connection) {
    match Pointer::new(connection) {
        Ok(pointer) => {
            *POINTER.lock().unwrap_or_else(PoisonError::into_inner) = Some(pointer);
        }
        Err(err) => tracing::warn!(%err, "the pointer's shape is set only as it changes"),
    }
}

static POINTER: Mutex<Option<Pointer>> = Mutex::new(None);

struct Pointer {
    connection: Connection,
    queue: EventQueue<Seat>,
    seat: Seat,
}

struct Seat {
    manager: WpCursorShapeManagerV1,
    /// The pointer and its shape's device, once the seat has a pointer.
    device: Option<(WlPointer, WpCursorShapeDeviceV1)>,
    /// The serial of the enter the pointer is on a surface since; `None`
    /// when it is on none of the shell's.
    entered: Option<u32>,
}

impl Pointer {
    fn new(connection: &Connection) -> Result<Self, String> {
        let (globals, mut queue) =
            registry_queue_init::<Seat>(connection).map_err(|e| e.to_string())?;
        let qh = queue.handle();
        let manager: WpCursorShapeManagerV1 = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| format!("wp_cursor_shape_manager_v1: {e}"))?;
        // The compositor has one seat; its capabilities give the pointer.
        let _: WlSeat = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| format!("wl_seat: {e}"))?;
        let mut seat = Seat {
            manager,
            device: None,
            entered: None,
        };
        // Before iced reads the connection.
        queue
            .roundtrip(&mut seat)
            .map_err(|e| format!("the seat: {e}"))?;
        Ok(Self {
            connection: connection.clone(),
            queue,
            seat,
        })
    }

    fn handle(&mut self) -> bool {
        match self.queue.dispatch_pending(&mut self.seat) {
            Ok(_) => true,
            Err(err) => {
                tracing::warn!(%err, "the pointer's events");
                false
            }
        }
    }

    fn set(&mut self, interaction: mouse::Interaction) {
        let (Some(serial), Some((_, device))) = (self.seat.entered, &self.seat.device) else {
            return;
        };
        device.set_shape(serial, shape(interaction));
        // iced's loop flushes what is left when the socket is full.
        match self.connection.flush() {
            Ok(()) => {}
            Err(wayland_client::backend::WaylandError::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(err) => tracing::warn!(%err, "the pointer's shape"),
        }
    }
}

/// Handles what iced's loop has read for the pointer: its events would
/// pile up otherwise, one for each move.
fn handle() {
    let mut pointer = POINTER.lock().unwrap_or_else(PoisonError::into_inner);
    if pointer.as_mut().is_some_and(|p| !p.handle()) {
        *pointer = None;
    }
}

/// Sets the shape for `interaction` if the pointer is on one of the
/// shell's surfaces, with the serial of the enter that put it there.
fn set(interaction: mouse::Interaction) {
    let mut pointer = POINTER.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(p) = pointer.as_mut() else {
        return;
    };
    if p.handle() {
        p.set(interaction);
    } else {
        *pointer = None;
    }
}

/// The shape iced_layershell gives `interaction`, so that the two never
/// differ.
fn shape(interaction: mouse::Interaction) -> Shape {
    use mouse::Interaction;
    match interaction {
        Interaction::Idle | Interaction::Wait => Shape::Wait,
        Interaction::Pointer => Shape::Pointer,
        Interaction::Grab => Shape::Grab,
        Interaction::Text => Shape::Text,
        Interaction::ZoomIn => Shape::ZoomIn,
        Interaction::Grabbing => Shape::Grabbing,
        Interaction::Crosshair => Shape::Crosshair,
        Interaction::NotAllowed => Shape::NotAllowed,
        Interaction::ResizingVertically => Shape::NsResize,
        Interaction::ResizingHorizontally => Shape::EwResize,
        Interaction::Cell => Shape::Cell,
        Interaction::Move => Shape::Move,
        Interaction::Copy => Shape::Copy,
        Interaction::Help => Shape::Help,
        Interaction::ZoomOut => Shape::ZoomOut,
        Interaction::ResizingDiagonallyUp | Interaction::ResizingDiagonallyDown => {
            Shape::NwseResize
        }
        _ => Shape::Default,
    }
}

/// A surface's widgets, whose shape is set again at each enter.
pub struct Entered<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
}

impl<'a, Message, Theme, Renderer> Entered<'a, Message, Theme, Renderer> {
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
        }
    }
}

impl<Message, Theme, Renderer> std::fmt::Debug for Entered<'_, Message, Theme, Renderer> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entered").finish_non_exhaustive()
    }
}

/// Whether an enter came since the shape was last asked for.
#[derive(Default)]
struct Fresh(Cell<bool>);

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Entered<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Fresh>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Fresh::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Mouse(event) = event {
            if let mouse::Event::CursorEntered = event {
                tree.state.downcast_ref::<Fresh>().0.set(true);
            } else {
                handle();
            }
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let interaction = self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        );
        // Asked once for all the events read together, enter included:
        // the shape the pointer is to have now.
        if tree.state.downcast_ref::<Fresh>().0.take() {
            set(interaction);
        }
        interaction
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<Entered<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(entered: Entered<'a, Message, Theme, Renderer>) -> Self {
        Element::new(entered)
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for Seat {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for Seat {
    fn event(
        seat: &mut Self,
        wl_seat: &WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        else {
            return;
        };
        let has = capabilities.contains(wl_seat::Capability::Pointer);
        match (&seat.device, has) {
            (None, true) => {
                let pointer = wl_seat.get_pointer(qh, ());
                let device = seat.manager.get_pointer(&pointer, qh, ());
                seat.device = Some((pointer, device));
            }
            (Some(_), false) => {
                if let Some((pointer, device)) = seat.device.take() {
                    device.destroy();
                    if pointer.version() >= 3 {
                        pointer.release();
                    }
                }
                seat.entered = None;
            }
            _ => {}
        }
    }
}

impl Dispatch<WlPointer, ()> for Seat {
    fn event(
        seat: &mut Self,
        _: &WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter { serial, .. } => seat.entered = Some(serial),
            wl_pointer::Event::Leave { .. } => seat.entered = None,
            _ => {}
        }
    }
}

impl Dispatch<WpCursorShapeManagerV1, ()> for Seat {
    fn event(
        _: &mut Self,
        _: &WpCursorShapeManagerV1,
        _: <WpCursorShapeManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpCursorShapeDeviceV1, ()> for Seat {
    fn event(
        _: &mut Self,
        _: &WpCursorShapeDeviceV1,
        _: <WpCursorShapeDeviceV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
