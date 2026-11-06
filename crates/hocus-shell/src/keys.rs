//! The keys go to the shell's surface the compositor gave the keyboard to
//! (#623), never to one because the shell made it last.
//!
//! iced_layershell hands every key to the surface layershellev calls
//! current: the one the keyboard last entered, but also the one last
//! pressed, and the one last made (`push_window`). Nothing in either lets
//! the shell set it or correct it. So a surface the shell made while the
//! launcher, the emoji picker or a field held the keyboard (a bubble, a
//! reminder, the meeting card) took the typing: the field still looked
//! focused, and no key reached it.
//!
//! So the shell follows the compositor's `wl_keyboard.enter` itself, with
//! a keyboard of its own on iced's connection, which the compositor tells
//! the same enters and leaves. An enter names a Wayland surface, and
//! iced_layershell shows the shell no surface of its windows: which window
//! it is for, the shell learns from what follows. The enter makes the
//! entered surface layershellev's current one, so:
//!
//! - the modifiers the compositor sends right after each enter are handed
//!   to that window, if iced_layershell has it already;
//! - else (the compositor gives the keyboard to a surface that asks for it
//!   as soon as it knows it, before iced_layershell has it) that window
//!   opens in iced just after, and its first key is handed to it.
//!
//! A window made after the enter is none of these: the compositor could
//! not have entered it yet. The shell's windows are numbered in the order
//! they are made (`window::Id::unique`), and the enter takes a number
//! too, as the shell sees it: a window numbered before it may be the one
//! entered, one numbered after may not. The first keyboard event handed
//! to a window numbered before it settles which one holds the keyboard; a
//! window opening meanwhile is taken for it until then.
//!
//! Every surface's widgets are wrapped in [`Held`]. A keyboard event
//! handed to another window than the one holding the keyboard is kept for
//! that one, and the shell woken: its widgets get the event before their
//! next one, the redraw that follows. Until the shell knows which window
//! holds the keyboard (none of its own does, or which one is not known
//! yet), the keys go where iced_layershell hands them.
//!
//! iced_layershell reads the socket for both keyboards and hands the
//! widgets their events after the read: the shell's own enter is there by
//! then. A leave, layershellev tells its current window as modifiers
//! emptied then an unfocus: a window those modifiers would have taken for
//! an enter's is let go at that unfocus.
//!
//! A key typed while a surface just made has not yet opened in iced is
//! lost (iced_layershell hands it to that surface, which it does not have
//! yet): that lasts one round trip to the compositor.
//!
//! Nothing here reads the connection, nor wakes: the keyboard's events are
//! handled as the widgets get theirs.
//!
//! The log says where the keys went: each enter and leave and each change
//! of the holder (debug), each keyboard event's window, its kind and its
//! route (trace, `hocuspanda_shell::keys=trace`), never what was typed.

use std::sync::{Mutex, PoisonError};

use iced::{
    Element, Event, Length, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree},
    },
    keyboard, window,
};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_keyboard::{self, WlKeyboard},
        wl_registry::WlRegistry,
        wl_seat::{self, WlSeat},
    },
};

/// At most this many keyboard events wait for their window: a window
/// closed before it took them leaves them here, the oldest let go.
const KEPT: usize = 256;

/// Makes the shell's keyboard on `connection`, the one iced draws on.
pub fn start(connection: &Connection) {
    match Keyboard::new(connection) {
        Ok(keyboard) => {
            *KEYBOARD.lock().unwrap_or_else(PoisonError::into_inner) = Some(keyboard);
        }
        Err(err) => {
            tracing::warn!(%err, "the keys go where iced_layershell hands them");
        }
    }
}

static KEYBOARD: Mutex<Option<Keyboard>> = Mutex::new(None);

struct Keyboard {
    queue: EventQueue<Seat>,
    seat: Seat,
    keys: Keys,
}

struct Seat {
    keyboard: Option<WlKeyboard>,
    /// The enters and leaves come since the shell last looked, in order.
    told: Vec<Told>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Told {
    Enter,
    Leave,
}

impl Keyboard {
    fn new(connection: &Connection) -> Result<Self, String> {
        let (globals, mut queue) =
            registry_queue_init::<Seat>(connection).map_err(|e| e.to_string())?;
        let qh = queue.handle();
        // The compositor has one seat; its capabilities give the keyboard.
        let _: WlSeat = globals
            .bind(&qh, 1..=3, ())
            .map_err(|e| format!("wl_seat: {e}"))?;
        let mut seat = Seat {
            keyboard: None,
            told: Vec::new(),
        };
        // Before iced reads the connection, so that the first enter comes
        // to both keyboards.
        queue
            .roundtrip(&mut seat)
            .map_err(|e| format!("the seat: {e}"))?;
        Ok(Self {
            queue,
            seat,
            keys: Keys::default(),
        })
    }

    /// Handles what iced's loop has read for the keyboard.
    fn handle(&mut self) -> bool {
        if let Err(err) = self.queue.dispatch_pending(&mut self.seat) {
            tracing::warn!(%err, "the keyboard's events: the keys go where iced_layershell hands them");
            return false;
        }
        for told in self.seat.told.drain(..) {
            self.keys.told(told);
        }
        true
    }
}

/// Which of the shell's windows holds the keyboard, as far as the shell
/// knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Holder {
    /// None of the shell's surfaces has the keyboard, or which one is not
    /// known: the keys go where iced_layershell hands them.
    Unknown,
    /// The compositor entered one of the shell's surfaces, made before
    /// window `before` (see the module's comment).
    Entering { before: window::Id },
    /// Window `id`, made before `before`, opened since the enter: taken
    /// for the one entered until a keyboard event says which.
    Opened { id: window::Id, before: window::Id },
    /// Window `0` holds it.
    Window(window::Id),
}

/// Where a keyboard event handed to a window goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    /// To that window's widgets.
    Here,
    /// To window `0`'s, which holds the keyboard.
    To(window::Id),
}

#[derive(Debug)]
struct Keys {
    holder: Holder,
    /// The holder before modifiers handed to window `0` settled it: those
    /// may be a leave's, which an unfocus of the same window follows at
    /// once.
    undo: Option<(window::Id, Holder)>,
    /// The keyboard events handed to other windows than the one holding
    /// the keyboard, for that one, in order.
    kept: Vec<(window::Id, Event)>,
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            holder: Holder::Unknown,
            undo: None,
            kept: Vec::new(),
        }
    }
}

impl Keys {
    /// The compositor told the shell's keyboard an enter or a leave.
    fn told(&mut self, told: Told) {
        self.undo = None;
        let was = self.holder;
        self.holder = match told {
            // Numbered after every window made so far.
            Told::Enter => Holder::Entering {
                before: window::Id::unique(),
            },
            Told::Leave => Holder::Unknown,
        };
        tracing::debug!(?told, ?was, now = ?self.holder, "the compositor moved the keyboard");
    }

    /// Where `event`, handed to window `here`, goes; what it tells of the
    /// holder taken in.
    fn route(&mut self, here: window::Id, event: &Event) -> Route {
        if let Some((id, holder)) = self.undo.take()
            && id == here
            && matches!(event, Event::Window(window::Event::Unfocused))
        {
            // Those modifiers were a leave's.
            tracing::debug!(window = ?here, ?holder, "the keyboard was not on this window: a leave's unfocus");
            self.holder = holder;
            return Route::Here;
        }
        let before = match self.holder {
            Holder::Entering { before } | Holder::Opened { before, .. } => Some(before),
            Holder::Unknown | Holder::Window(_) => None,
        };
        let entered_maybe = before.is_some_and(|before| here < before);
        if let Event::Window(window::Event::Opened { .. }) = event
            && let Holder::Entering { before } = self.holder
            && entered_maybe
        {
            tracing::debug!(window = ?here, "the keyboard is taken to be on this window, opened since its enter");
            self.holder = Holder::Opened { id: here, before };
            return Route::Here;
        }
        let Event::Keyboard(key) = event else {
            return Route::Here;
        };
        let was = self.holder;
        let route = self.route_key(here, key, entered_maybe);
        // Which window each key was handed to, and where it goes: what a
        // key lost to the wrong window leaves to read (#677). Never what
        // was typed.
        tracing::trace!(
            window = ?here,
            key = key_kind(key),
            ?was,
            now = ?self.holder,
            ?route,
            "a keyboard event"
        );
        route
    }

    /// Where keyboard event `key`, handed to window `here`, goes.
    fn route_key(&mut self, here: window::Id, key: &keyboard::Event, entered_maybe: bool) -> Route {
        match self.holder {
            Holder::Unknown => Route::Here,
            Holder::Entering { .. } | Holder::Opened { .. } if entered_maybe => {
                tracing::debug!(window = ?here, "the keyboard is on this window");
                if let keyboard::Event::ModifiersChanged(_) = key {
                    self.undo = Some((here, self.holder));
                }
                self.holder = Holder::Window(here);
                Route::Here
            }
            // Made since the enter: not the one entered.
            Holder::Entering { .. } => Route::Here,
            Holder::Opened { id, .. } | Holder::Window(id) if id != here => Route::To(id),
            Holder::Opened { .. } | Holder::Window(_) => Route::Here,
        }
    }

    /// Keeps `event` for window `to`.
    fn keep(&mut self, to: window::Id, event: Event) {
        if self.kept.len() >= KEPT {
            self.kept.remove(0);
        }
        self.kept.push((to, event));
    }

    /// The events kept for window `id`, in order.
    fn take(&mut self, id: window::Id) -> Vec<Event> {
        if self.kept.is_empty() {
            return Vec::new();
        }
        tracing::trace!(window = ?id, kept = self.kept.len(), "the keyboard events kept, for their windows");
        let mut taken = Vec::new();
        self.kept.retain(|(to, event)| {
            if *to == id {
                taken.push(event.clone());
                false
            } else {
                true
            }
        });
        taken
    }
}

/// What a keyboard event is, for the log: never its key nor its text.
fn key_kind(key: &keyboard::Event) -> &'static str {
    match key {
        keyboard::Event::KeyPressed { .. } => "pressed",
        keyboard::Event::KeyReleased { .. } => "released",
        keyboard::Event::ModifiersChanged(_) => "modifiers",
    }
}

/// What window `here`'s widgets get: the events kept for them, then
/// `event` unless it goes to the window holding the keyboard (kept for
/// it: `true`).
fn arrived(here: window::Id, event: &Event) -> (Vec<Event>, bool) {
    let mut keyboard = KEYBOARD.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(k) = keyboard.as_mut() else {
        return (Vec::new(), false);
    };
    if !k.handle() {
        // The keys kept are let go with it: their window never gets them.
        *keyboard = None;
        return (Vec::new(), false);
    }
    let kept = k.keys.take(here);
    match k.keys.route(here, event) {
        Route::Here => (kept, false),
        Route::To(to) => {
            tracing::trace!(from = ?here, ?to, "a key handed to another window");
            k.keys.keep(to, event.clone());
            (kept, true)
        }
    }
}

/// A window's widgets, which get the keys the compositor gives that
/// window, whichever window iced_layershell hands them to.
pub struct Held<'a, Message, Theme, Renderer> {
    id: window::Id,
    content: Element<'a, Message, Theme, Renderer>,
    /// Published when a key is kept for another window: the shell's
    /// update wakes every surface, that one's among them.
    wake: Message,
}

impl<'a, Message, Theme, Renderer> Held<'a, Message, Theme, Renderer> {
    pub fn new(
        id: window::Id,
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        wake: Message,
    ) -> Self {
        Self {
            id,
            content: content.into(),
            wake,
        }
    }
}

impl<Message, Theme, Renderer> std::fmt::Debug for Held<'_, Message, Theme, Renderer> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Held")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Held<'_, Message, Theme, Renderer>
where
    Message: Clone,
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
        let (kept, elsewhere) = arrived(self.id, event);
        for kept in &kept {
            self.content.as_widget_mut().update(
                &mut tree.children[0],
                kept,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
        if elsewhere {
            shell.capture_event();
            shell.publish(self.wake.clone());
            return;
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
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
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

impl<'a, Message, Theme, Renderer> From<Held<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(held: Held<'a, Message, Theme, Renderer>) -> Self {
        Element::new(held)
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
        let has = capabilities.contains(wl_seat::Capability::Keyboard);
        match (&seat.keyboard, has) {
            (None, true) => seat.keyboard = Some(wl_seat.get_keyboard(qh, ())),
            (Some(_), false) => {
                if let Some(keyboard) = seat.keyboard.take()
                    && keyboard.version() >= 3
                {
                    keyboard.release();
                }
                seat.told.push(Told::Leave);
            }
            _ => {}
        }
    }
}

/// Only the enters and leaves count: the keymap's descriptor is closed as
/// it is dropped, and the keys come to iced_layershell's keyboard too.
impl Dispatch<WlKeyboard, ()> for Seat {
    fn event(
        seat: &mut Self,
        _: &WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Enter { .. } => seat.told.push(Told::Enter),
            wl_keyboard::Event::Leave { .. } => seat.told.push(Told::Leave),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modifiers() -> Event {
        Event::Keyboard(keyboard::Event::ModifiersChanged(
            keyboard::Modifiers::empty(),
        ))
    }

    fn key() -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Character("a".into()),
            modified_key: keyboard::Key::Character("a".into()),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyA),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: Some("a".into()),
            repeat: false,
        })
    }

    fn unfocused() -> Event {
        Event::Window(window::Event::Unfocused)
    }

    fn opened() -> Event {
        Event::Window(window::Event::Opened {
            position: None,
            size: Size::new(10.0, 10.0),
        })
    }

    #[test]
    fn the_window_given_the_enters_modifiers_takes_the_keys_handed_elsewhere() {
        let launcher = window::Id::unique();
        let mut keys = Keys::default();
        // Before any enter: where iced_layershell hands them.
        assert_eq!(keys.route(launcher, &key()), Route::Here);
        keys.told(Told::Enter);
        assert_eq!(keys.route(launcher, &modifiers()), Route::Here);
        assert_eq!(keys.route(launcher, &key()), Route::Here);
        // A surface made since: iced_layershell hands it the keys.
        let bubble = window::Id::unique();
        assert_eq!(keys.route(bubble, &opened()), Route::Here);
        assert_eq!(keys.route(bubble, &key()), Route::To(launcher));
        assert_eq!(keys.route(bubble, &modifiers()), Route::To(launcher));
        // A leave: back to iced_layershell's choice.
        keys.told(Told::Leave);
        assert_eq!(keys.route(bubble, &key()), Route::Here);
    }

    #[test]
    fn a_window_entered_before_iced_has_it_takes_the_keys_from_its_opening() {
        let picker = window::Id::unique();
        let mut keys = Keys::default();
        // Entered as it is made: its modifiers went to a window iced did
        // not have.
        keys.told(Told::Enter);
        assert_eq!(keys.route(picker, &opened()), Route::Here);
        // A bubble made before the first key.
        let bubble = window::Id::unique();
        assert_eq!(keys.route(bubble, &opened()), Route::Here);
        assert_eq!(keys.route(bubble, &key()), Route::To(picker));
        assert_eq!(keys.route(picker, &key()), Route::Here);
        assert_eq!(keys.route(bubble, &key()), Route::To(picker));
    }

    #[test]
    fn a_key_settles_which_window_made_before_the_enter_holds_it() {
        let (toast, field) = (window::Id::unique(), window::Id::unique());
        let mut keys = Keys::default();
        keys.told(Told::Enter);
        // Another window made before the enter opens first: taken for the
        // one entered until a key says otherwise.
        assert_eq!(keys.route(toast, &opened()), Route::Here);
        assert_eq!(keys.route(field, &opened()), Route::Here);
        assert_eq!(keys.route(field, &key()), Route::Here);
        assert_eq!(keys.route(toast, &key()), Route::To(field));
        // A window made after the enter says nothing of it.
        keys.told(Told::Enter);
        let bubble = window::Id::unique();
        assert_eq!(keys.route(bubble, &opened()), Route::Here);
        assert_eq!(keys.route(bubble, &key()), Route::Here);
        assert_eq!(keys.route(field, &key()), Route::Here);
        assert_eq!(keys.route(bubble, &key()), Route::To(field));
    }

    #[test]
    fn a_leaves_modifiers_and_unfocus_do_not_take_the_keyboard() {
        let (launcher, picker) = (window::Id::unique(), window::Id::unique());
        let mut keys = Keys::default();
        keys.told(Told::Enter);
        keys.route(launcher, &modifiers());
        // The keyboard moves from the launcher to the picker, read at once:
        // the launcher's leave (modifiers emptied, unfocus) comes first.
        keys.told(Told::Leave);
        keys.told(Told::Enter);
        assert_eq!(keys.route(launcher, &modifiers()), Route::Here);
        assert_eq!(keys.route(launcher, &unfocused()), Route::Here);
        assert_eq!(keys.route(picker, &modifiers()), Route::Here);
        assert_eq!(keys.route(launcher, &key()), Route::To(picker));
        // Back to the launcher, its events first: the picker's leave then
        // goes to it, and lets nothing go.
        keys.told(Told::Leave);
        keys.told(Told::Enter);
        assert_eq!(keys.route(launcher, &modifiers()), Route::Here);
        assert_eq!(keys.route(picker, &modifiers()), Route::To(launcher));
        assert_eq!(keys.route(picker, &unfocused()), Route::Here);
        assert_eq!(keys.route(picker, &key()), Route::To(launcher));
    }

    #[test]
    fn kept_events_go_to_their_window_in_order() {
        let (launcher, bubble) = (window::Id::unique(), window::Id::unique());
        let mut keys = Keys::default();
        keys.keep(launcher, key());
        keys.keep(bubble, modifiers());
        keys.keep(launcher, modifiers());
        assert_eq!(keys.take(launcher), vec![key(), modifiers()]);
        assert!(keys.take(launcher).is_empty());
        assert_eq!(keys.take(bubble), vec![modifiers()]);
    }
}
