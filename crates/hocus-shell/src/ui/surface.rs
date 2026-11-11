//! A popover's surface around its widgets ([`Around`]), and its glass.

use std::sync::{
    Mutex, PoisonError,
    atomic::{AtomicU64, Ordering},
};

use iced::{
    Element, Event, Length, Rectangle, Renderer, Size, Theme,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, renderer,
        widget::{Operation, Tree, operation::Outcome, tree},
    },
    keyboard,
    widget::canvas,
    window,
};

use super::{
    control::Declared,
    keys::{self, Step},
    walk::Walk,
};
use crate::{
    Message,
    a11y::rows::{self, Rows},
    popover::{Effect, Event as Seen, Pressed, event, kit},
};

type El<'a, M> = Element<'a, M>;

/// The log's target for what a surface's widgets show.
const SHOWS: &str = "hocuspanda_shell::shows";

/// Counts what changed outside the surfaces' messages (a network, a
/// mixer: [`touch`]), and the views built.
static STAMP: AtomicU64 = AtomicU64::new(0);
static BUILDS: AtomicU64 = AtomicU64::new(0);

/// What a kit surface shows changed outside it: its view is built again as
/// it next draws.
pub fn touch() {
    STAMP.fetch_add(1, Ordering::Relaxed);
}

/// Operations that wait for the tree built after what a surface handled
/// (a list scrolled to the row the ring went to).
pub struct Later(Mutex<Vec<Box<dyn Operation>>>);

impl Later {
    pub const fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }

    pub fn push(&self, operation: impl Operation + 'static) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Box::new(operation));
    }

    fn take(&self) -> Vec<Box<dyn Operation>> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// A key, before the kit's: `Some` takes it.
type OnKey<'a, L> = Box<dyn Fn(&Pressed) -> Option<L> + 'a>;

/// A popover's surface around its widgets, and their loop.
///
/// iced hands a widget's message to the application, and iced_layershell
/// then builds and draws again every surface of the shell (a bar's ten,
/// on each screen), a turn of its loop later. So a popover's widgets speak
/// a message type of their own (`L`), which this surface takes from them
/// and has handled at once: the model changes before the next event is
/// looked at, the view of this surface alone is built again, and the
/// compositor is asked to have it drawn, as the canvases are woken.
///
/// Its controls' declarations ([`super::control`]) give it the rest: the
/// keyboard's ring goes through them ([`keys`]), screen readers read them
/// ([`crate::a11y::rows`]) and the log says them for tests (`shows {…}`,
/// with `RUST_LOG=hocuspanda_shell::shows=debug`). The pointer coming in
/// and leaving reaches the popovers' controller (its cone, its 300 ms).
pub struct Around<'a, L> {
    id: window::Id,
    /// Which build of the view this is, and of what outside it.
    build: u64,
    stamp: u64,
    content: El<'a, L>,
    handle: Box<dyn Fn(L) + 'a>,
    /// What the content was built from changed since, outside it, besides
    /// [`touch`].
    stale: Box<dyn Fn() -> bool + 'a>,
    on_stale: L,
    /// A key, before the kit's: `Some` takes it.
    on_key: OnKey<'a, L>,
    /// A press anywhere, once the content has had it.
    on_press: Option<L>,
    /// Opened by the keyboard: the ring's first place (else `Tab`'s).
    first: Option<L>,
    later: Option<&'static Later>,
    /// The height the content takes, laid out with all the height it wants.
    natural: Box<dyn Fn(f32) + 'a>,
    /// The line under its title, for screen readers.
    subtitle: Option<String>,
    /// When what it shows ends by itself (a value asked and not said).
    wake: Box<dyn Fn() -> Option<std::time::Instant> + 'a>,
    /// Its content built again, from the model as it is now.
    rebuild: Option<Box<dyn Fn() -> El<'a, L> + 'a>>,
    /// Built again, to be laid out.
    pending: Option<El<'a, L>>,
    /// A surface of its own, not a popover of the bar's.
    own: Option<Own<'a>>,
}

/// A surface of the shell's own (the actions palette, the emoji picker,
/// the launcher…), not one of the bar's popovers: open while `open` says,
/// drawn again by its `namespace`. It takes its keys itself, `Escape`
/// among them; the popovers' controller knows nothing of it.
struct Own<'a> {
    namespace: &'static str,
    open: Box<dyn Fn() -> bool + 'a>,
}

impl<L> std::fmt::Debug for Around<'_, L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Around")
            .field("id", &self.id)
            .field("build", &self.build)
            .finish_non_exhaustive()
    }
}

impl<'a, L: Clone + Send + 'static> Around<'a, L> {
    /// Popover `id`'s `content`, what its widgets ask `handle`d, `stale`
    /// told when what it shows changed outside it.
    pub fn new(
        id: window::Id,
        content: impl Into<El<'a, L>>,
        handle: impl Fn(L) + 'a,
        stale: L,
    ) -> Self {
        Self {
            id,
            build: BUILDS.fetch_add(1, Ordering::Relaxed),
            stamp: STAMP.load(Ordering::Relaxed),
            content: content.into(),
            handle: Box::new(handle),
            stale: Box::new(|| false),
            on_stale: stale,
            on_key: Box::new(|_| None),
            on_press: None,
            first: None,
            later: None,
            natural: Box::new(|_| {}),
            subtitle: None,
            wake: Box::new(|| None),
            rebuild: None,
            pending: None,
            own: None,
        }
    }

    /// A surface of its own, `namespace`'s, open while `open` says, not a
    /// popover of the bar's.
    pub fn own(mut self, namespace: &'static str, open: impl Fn() -> bool + 'a) -> Self {
        self.own = Some(Own {
            namespace,
            open: Box::new(open),
        });
        self
    }

    /// Stale too when `stale` says so.
    pub fn stale_when(mut self, stale: impl Fn() -> bool + 'a) -> Self {
        self.stale = Box::new(stale);
        self
    }

    /// The keys the surface takes before the kit.
    pub fn keys(mut self, on_key: impl Fn(&Pressed) -> Option<L> + 'a) -> Self {
        self.on_key = Box::new(on_key);
        self
    }

    /// Told of a press anywhere, after the widget pressed.
    pub fn press(mut self, message: L) -> Self {
        self.on_press = Some(message);
        self
    }

    /// Opened by the keyboard, the ring goes where `message` says.
    pub fn first(mut self, message: L) -> Self {
        self.first = Some(message);
        self
    }

    pub fn later(mut self, later: &'static Later) -> Self {
        self.later = Some(later);
        self
    }

    pub fn natural(mut self, natural: impl Fn(f32) + 'a) -> Self {
        self.natural = Box::new(natural);
        self
    }

    pub fn subtitle(mut self, subtitle: Option<String>) -> Self {
        self.subtitle = subtitle;
        self
    }

    /// How its content is built (as `content` was): what changes outside
    /// it as it draws (`stale_when`, [`touch`]) builds it again in place,
    /// this surface alone drawn again.
    pub fn rebuild(mut self, rebuild: impl Fn() -> El<'a, L> + 'a) -> Self {
        self.rebuild = Some(Box::new(rebuild));
        self
    }

    /// Drawn again when `at` says, if it does: what it shows ends then by
    /// itself, which `stale_when` then says.
    pub fn wake_at(mut self, at: impl Fn() -> Option<std::time::Instant> + 'a) -> Self {
        self.wake = Box::new(at);
        self
    }

    /// On the popovers' glass (else opaque), filling the surface.
    pub fn on_glass(self, glass: bool) -> El<'a, Message> {
        self.on(canvas(Glass(glass))
            .width(Length::Fill)
            .height(Length::Fill))
    }

    /// On a background of its own (the clipboard's tint), filling the
    /// surface.
    pub fn on(self, background: impl Into<El<'a, Message>>) -> El<'a, Message> {
        iced::widget::stack![background.into(), Element::new(self)].into()
    }
}

/// What the surface keeps from a build of its view to the next.
#[derive(Debug, Default)]
struct Kept {
    /// The pointer is on the surface.
    inside: bool,
    /// The build whose tree a handled message outdated.
    outdated: Option<u64>,
    /// The keys come for the tree to be built.
    held: Vec<Event>,
    /// The keys that reached another surface, still to go through.
    queued: Vec<Pressed>,
    /// What the tree showed when last said in the log.
    said: String,
}

fn child(layout: Layout<'_>) -> Layout<'_> {
    layout.children().next().unwrap_or(layout)
}

impl<L: Clone + Send + 'static> Around<'_, L> {
    /// `operation` on the content, and those it chains.
    fn run(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: Box<dyn Operation>,
    ) {
        let mut next = Some(operation);
        while let Some(mut operation) = next.take() {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                child(layout),
                renderer,
                operation.as_mut(),
            );
            if let Outcome::Chain(chained) = operation.finish() {
                next = Some(chained);
            }
        }
    }

    /// The tree as laid out: its controls, its places, its texts.
    fn walk(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer) -> Walk<L> {
        let mut walk = Walk::default();
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            child(layout),
            renderer,
            &mut walk,
        );
        walk
    }

    /// A key, as the surface, then the kit, take it: whether it was.
    fn key(
        &mut self,
        pressed: &Pressed,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
    ) -> Option<bool> {
        if let Some(local) = (self.on_key)(pressed) {
            (self.handle)(local);
            return Some(true);
        }
        let controls: Vec<Declared<L>> = self
            .walk(tree, layout, renderer)
            .controls
            .into_iter()
            .map(|(_, d)| d)
            .collect();
        match keys::step(&controls, pressed) {
            Step::Tell(local) => {
                (self.handle)(local);
                Some(true)
            }
            Step::Escape => {
                if self.own.is_none() {
                    event(Seen::Escape);
                }
                Some(false)
            }
            Step::Kept => Some(false),
            Step::Pass => None,
        }
    }

    /// `event` for the content: what its widgets ask is handled here.
    /// Whether anything was.
    #[allow(clippy::too_many_arguments)]
    fn dispatch(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) -> bool {
        // The keys wait for the tree built after what was handled: the
        // ring's order, a field, are as it shows them.
        let kept = tree.state.downcast_mut::<Kept>();
        if kept.outdated == Some(self.build) && matches!(event, Event::Keyboard(_)) {
            kept.held.push(event.clone());
            shell.capture_event();
            return false;
        }
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            text,
            ..
        }) = event
        {
            let pressed = Pressed {
                key: key.clone(),
                modifiers: *modifiers,
                text: text.as_deref().map(str::to_owned),
            };
            if let Some(handled) = self.key(&pressed, tree, layout, renderer) {
                shell.capture_event();
                return handled;
            }
        }
        let mut asked = Vec::new();
        let mut inner = Shell::new(&mut asked);
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            child(layout),
            cursor,
            renderer,
            clipboard,
            &mut inner,
            viewport,
        );
        if inner.is_event_captured() {
            shell.capture_event();
        }
        shell.request_redraw_at(inner.redraw_request());
        shell.request_input_method(inner.input_method());
        if inner.is_layout_invalid() {
            shell.invalidate_layout();
        }
        if inner.are_widgets_invalid() {
            shell.invalidate_widgets();
        }
        let mut handled = !asked.is_empty();
        for local in asked {
            (self.handle)(local);
        }
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) && let Some(pressed) = self.on_press.clone()
        {
            (self.handle)(pressed);
            handled = true;
        }
        handled
    }

    /// The keys that reached another surface, one at a time: each after
    /// the tree the one before built.
    fn queued(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer) -> bool {
        loop {
            let kept = tree.state.downcast_mut::<Kept>();
            if kept.queued.is_empty() {
                return false;
            }
            let pressed = kept.queued.remove(0);
            if self.key(&pressed, tree, layout, renderer) == Some(true) {
                return true;
            }
        }
    }

    /// What the tree shows, for screen readers, and said in the log when
    /// it changes.
    fn tell(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer) {
        let walk = self.walk(tree, layout, renderer);
        if !walk.controls.is_empty() {
            let order = walk
                .controls
                .iter()
                .filter(|(_, d)| d.on_focus.is_some())
                .map(|(_, d)| d.node.key.clone())
                .collect();
            let nodes = walk
                .controls
                .iter()
                .map(|(r, d)| d.node.clone().bounds(rows::at(*r)))
                .collect();
            rows::publish(
                self.id,
                Rows {
                    subtitle: self.subtitle.clone(),
                    nodes,
                    order,
                    focus: walk.focused().map(|d| d.node.key.clone()),
                    arrows: false,
                },
            );
        }
        if tracing::enabled!(target: SHOWS, tracing::Level::DEBUG) {
            let json = walk.json();
            let kept = tree.state.downcast_mut::<Kept>();
            if kept.said != json {
                tracing::debug!(target: SHOWS, "shows {json}");
                kept.said = json;
            }
        }
    }
}

impl<L: Clone + Send + 'static> Widget<Message, Theme, Renderer> for Around<'_, L> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Kept>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Kept::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        if let Some(content) = self.pending.take() {
            self.content = content;
            tree.children[0].diff(self.content.as_widget());
        }
        let max = limits.max();
        // All the height it wants: what the surface should be.
        let free = layout::Limits::new(Size::ZERO, Size::new(max.width, f32::INFINITY));
        let content = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &free);
        (self.natural)(content.size().height);
        layout::Node::with_children(max, vec![content])
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
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
            child(layout),
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
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            child(layout),
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let open = match &self.own {
            Some(own) => (own.open)(),
            None => crate::popover::with(|c| c.is_open(self.id)),
        };
        let popover = self.own.is_none();
        let drawing = matches!(event, Event::Window(window::Event::RedrawRequested(_)));
        // Asked on every event: a tree built again from one is drawn
        // without a request of its own.
        if let Some(at) = (self.wake)().filter(|at| open && *at > std::time::Instant::now()) {
            shell.request_redraw_at(window::RedrawRequest::At(at));
        }
        let mut handled = false;
        // The tree built after what was handled: what waited for it.
        let kept = tree.state.downcast_mut::<Kept>();
        if kept.outdated.is_some_and(|build| build != self.build) {
            kept.outdated = None;
            let held = std::mem::take(&mut kept.held);
            for operation in self.later.map(Later::take).unwrap_or_default() {
                self.run(tree, layout, renderer, operation);
            }
            if self.queued(tree, layout, renderer) {
                handled = true;
                tree.state.downcast_mut::<Kept>().outdated = Some(self.build);
            }
            for event in &held {
                handled |= self.dispatch(
                    tree, event, layout, cursor, renderer, clipboard, shell, viewport,
                );
                if handled {
                    tree.state.downcast_mut::<Kept>().outdated = Some(self.build);
                }
            }
        }
        match event {
            Event::Window(window::Event::RedrawRequested(_)) if open => {
                // What was handled outdated this tree, and none was built
                // since (a message from a resize, which iced builds no
                // view for): built now, else the keys held wait for good.
                let waits = tree.state.downcast_mut::<Kept>().outdated == Some(self.build);
                if STAMP.load(Ordering::Relaxed) != self.stamp || (self.stale)() || waits {
                    (self.handle)(self.on_stale.clone());
                    handled = true;
                }
                // Opened by the keyboard: the ring's first place.
                if let Some(first) = self.first.clone()
                    && popover
                    && kit::first_control(self.id)
                {
                    (self.handle)(first);
                    handled = true;
                }
                // The keys that reached another of the shell's surfaces,
                // and the `Tab` of a popover opened by the keyboard.
                let keys = if popover {
                    kit::keys(self.id)
                } else {
                    Vec::new()
                };
                if !keys.is_empty() {
                    tree.state.downcast_mut::<Kept>().queued.extend(keys);
                    if tree.state.downcast_mut::<Kept>().outdated != Some(self.build) {
                        handled |= self.queued(tree, layout, renderer);
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorEntered)
                if open && popover =>
            {
                let kept = tree.state.downcast_mut::<Kept>();
                if !kept.inside && cursor.position_over(layout.bounds()).is_some() {
                    kept.inside = true;
                    crate::popover::event(Seen::PopoverEnter);
                }
            }
            Event::Mouse(mouse::Event::CursorLeft) if open && popover => {
                tree.state.downcast_mut::<Kept>().inside = false;
                crate::popover::event(Seen::PopoverLeave);
            }
            _ => {}
        }
        // Fading out, it only draws.
        if open || drawing {
            handled |= self.dispatch(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }
        if !handled {
            if drawing && open {
                self.tell(tree, layout, renderer);
            }
            return;
        }
        tree.state.downcast_mut::<Kept>().outdated = Some(self.build);
        if drawing {
            if let Some(rebuild) = &self.rebuild {
                // Built again here, laid out again before it draws: no
                // message, which would draw every surface of the shell
                // again. What waits for the new tree runs as it next
                // draws, a frame later.
                // Swapped in as it is laid out again: until then the
                // tree and its layout are the old content's (a stack
                // around asks the pointer's look of them).
                self.pending = Some(rebuild());
                self.build = BUILDS.fetch_add(1, Ordering::Relaxed);
                self.stamp = STAMP.load(Ordering::Relaxed);
                shell.invalidate_layout();
                shell.request_redraw();
            } else {
                // From a redraw, iced_layershell builds no view again
                // unless a message asks: every surface's then.
                shell.publish(Message::Keys);
            }
        } else {
            shell.invalidate_widgets();
            let namespace = self
                .own
                .as_ref()
                .map_or(hocus_link::POPOVER_NAMESPACE, |own| own.namespace);
            crate::popover::send(vec![Effect::Wake(namespace)]);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            child(layout),
            cursor,
            viewport,
            renderer,
        )
    }
}

/// A popover's glass (or its opaque colour), its rim and its corners'
/// marks, as the canvases draw it ([`kit::background`]).
#[derive(Debug, Clone, Copy)]
struct Glass(bool);

impl<M> canvas::Program<M> for Glass {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        kit::background(&mut f, bounds.size(), self.0);
        vec![f.into_geometry()]
    }
}
