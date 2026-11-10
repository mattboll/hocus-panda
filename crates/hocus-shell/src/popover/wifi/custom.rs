//! The widgets of our own a popover made of iced's stock widgets needs
//! (spike #770). Nothing here is the Wi-Fi's: every popover would share
//! them.
//!
//! - [`Around`], a popover's whole surface: the pointer coming in and
//!   leaving (the controller's cone and its 300 ms), the keys before the
//!   widgets get them (iced 0.14 gives the keyboard's focus to text fields
//!   only: `Tab`, the arrows, `Enter` on a button are ours), the wake that
//!   builds the view again without the shell's other surfaces knowing, and
//!   the height its content takes by itself.
//! - [`Ring`], the keyboard's ring around a widget: 2 px, 3 px *outside*
//!   it, which a stock widget's border (inside its bounds) cannot draw.
//! - [`reveal`], an operation: a list scrolled as little as shows one of
//!   its rows, found by its id.

use iced::{
    Border, Color, Element, Event, Length, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{
            Id, Operation, Tree,
            operation::{self, Outcome, Scrollable, scrollable},
            tree,
        },
    },
    keyboard, window,
};

use crate::levels::draw::{RING, RING_OUT};
use crate::popover::{Event as Seen, Pressed, event, kit};

type El<'a, Message> = Element<'a, Message, iced::Theme, iced::Renderer>;

/// The log's target for what a surface's widgets show.
const SHOWS: &str = "hocuspanda_shell::shows";

// --- The surface -------------------------------------------------------------------

/// A popover's surface around its widgets, and their loop.
///
/// iced hands a widget's message to the application, and iced_layershell
/// then builds and draws again every surface of the shell (a bar's ten,
/// on each screen), a turn of its loop later. So the popover's widgets
/// speak a message type of their own (`Local`), which this surface takes
/// from them and has handled at once (`handle`): the model changes before
/// the next event is looked at, the view of this surface alone is built
/// again, and the compositor is asked to have it drawn (`redraw`), as the
/// canvases are woken. What only the tree built next can take (the field's
/// focus, keys typed for a field that is not there yet) waits for it.
pub struct Around<'a, Local, Message> {
    pub id: window::Id,
    /// Which build of the view this is.
    pub build: u64,
    pub content: El<'a, Local>,
    /// What the content was built from changed since, outside it.
    pub stale: Box<dyn Fn() -> bool + 'a>,
    pub on_stale: Box<dyn Fn() -> Local + 'a>,
    /// A key, before the content: `Some` takes it.
    pub on_key: Box<dyn Fn(Pressed) -> Option<Local> + 'a>,
    /// A press anywhere, once the content has had it, and the widget
    /// holding iced's focus then.
    pub on_press: Box<dyn Fn(Option<Id>) -> Local + 'a>,
    /// What a widget asked, handled at once.
    pub handle: Box<dyn Fn(Local) + 'a>,
    /// The operations that wait for the tree built after what was handled.
    pub pending: Box<dyn Fn() -> Vec<Box<dyn Operation>> + 'a>,
    /// The height the content takes, laid out with all the height it wants.
    pub natural: Box<dyn Fn(f32) + 'a>,
    /// Asks the compositor to have this surface drawn.
    pub redraw: Box<dyn Fn() + 'a>,
    /// A message of the shell's that does nothing: from a redraw, only a
    /// message has the view built again.
    pub wake: Message,
}

impl<Local, Message> std::fmt::Debug for Around<'_, Local, Message> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Around")
            .field("id", &self.id)
            .field("build", &self.build)
            .finish_non_exhaustive()
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
    /// What the tree showed when last said in the log.
    said: String,
}

fn child(layout: Layout<'_>) -> Layout<'_> {
    layout.children().next().unwrap_or(layout)
}

impl<Local, Message: Clone> Around<'_, Local, Message> {
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

    /// The widget of the content holding iced's focus.
    fn focused(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
    ) -> Option<Id> {
        let mut find = operation::focusable::find_focused();
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            child(layout),
            renderer,
            &mut operation::black_box(&mut find),
        );
        match find.finish() {
            Outcome::Some(id) => Some(id),
            _ => None,
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
        let pressed = match event {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => Some(Pressed {
                key: key.clone(),
                modifiers: *modifiers,
                text: text.as_deref().map(str::to_owned),
            }),
            _ => None,
        };
        // A key the surface takes, as the model is now.
        if let Some(local) = pressed.and_then(|pressed| (self.on_key)(pressed)) {
            (self.handle)(local);
            shell.capture_event();
            return true;
        }
        // The others wait for the tree to be built: this one's field may
        // be gone, or not there yet.
        let kept = tree.state.downcast_mut::<Kept>();
        if kept.outdated == Some(self.build) && matches!(event, Event::Keyboard(_)) {
            kept.held.push(event.clone());
            shell.capture_event();
            return false;
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
        ) {
            let focused = self.focused(tree, layout, renderer);
            (self.handle)((self.on_press)(focused));
            handled = true;
        }
        handled
    }
}

impl<Local, Message: Clone> Widget<Message, iced::Theme, iced::Renderer>
    for Around<'_, Local, Message>
{
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
        theme: &iced::Theme,
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
        let open = crate::popover::with(|c| c.is_open(self.id));
        let drawing = matches!(event, Event::Window(window::Event::RedrawRequested(_)));
        let mut handled = false;
        // The tree built after what was handled: what waited for it.
        let kept = tree.state.downcast_mut::<Kept>();
        if kept.outdated.is_some_and(|build| build != self.build) {
            kept.outdated = None;
            let held = std::mem::take(&mut kept.held);
            for operation in (self.pending)() {
                self.run(tree, layout, renderer, operation);
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
                if (self.stale)() {
                    (self.handle)((self.on_stale)());
                    handled = true;
                }
                // The keys that reached another of the shell's surfaces, and
                // the `Tab` of a popover opened by the keyboard.
                for pressed in kit::keys(self.id) {
                    if let Some(local) = (self.on_key)(pressed) {
                        (self.handle)(local);
                        handled = true;
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorEntered)
                if open =>
            {
                let kept = tree.state.downcast_mut::<Kept>();
                if !kept.inside && cursor.position_over(layout.bounds()).is_some() {
                    kept.inside = true;
                    event_seen(Seen::PopoverEnter);
                }
            }
            Event::Mouse(mouse::Event::CursorLeft) if open => {
                tree.state.downcast_mut::<Kept>().inside = false;
                event_seen(Seen::PopoverLeave);
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
            // What it shows, said in the log when it changes: read from
            // the tree as it is about to be drawn.
            if drawing && open && tracing::enabled!(target: SHOWS, tracing::Level::DEBUG) {
                let mut shown = Shown::default();
                self.content.as_widget_mut().operate(
                    &mut tree.children[0],
                    child(layout),
                    renderer,
                    &mut shown,
                );
                let json = shown.json();
                let kept = tree.state.downcast_mut::<Kept>();
                if kept.said != json {
                    tracing::debug!(target: SHOWS, "shows {json}");
                    kept.said = json;
                }
            }
            return;
        }
        tree.state.downcast_mut::<Kept>().outdated = Some(self.build);
        if drawing {
            // From a redraw, iced_layershell builds no view again unless
            // a message asks: every surface's then.
            shell.publish(self.wake.clone());
        } else {
            shell.invalidate_widgets();
            (self.redraw)();
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

fn event_seen(seen: Seen) {
    event(seen);
}

impl<'a, Local: 'a, Message: Clone + 'a> From<Around<'a, Local, Message>> for El<'a, Message> {
    fn from(around: Around<'a, Local, Message>) -> Self {
        Element::new(around)
    }
}

// --- The keyboard's ring -----------------------------------------------------------

/// A widget, and the keyboard's ring around it while it holds it.
pub struct Ring<'a, Message> {
    content: El<'a, Message>,
    on: bool,
    radius: f32,
}

/// `content` under the keyboard's ring when `on`, its corners' `radius`.
pub fn ring<'a, Message: 'a>(
    content: impl Into<El<'a, Message>>,
    on: bool,
    radius: f32,
) -> El<'a, Message> {
    Element::new(Ring {
        content: content.into(),
        on,
        radius,
    })
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Ring<'_, Message> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        if !self.on {
            return;
        }
        // As `levels::draw::ring`: 2 px of the accent as text, 3 px out;
        // under high contrast 3 px, the background between. In a layer of
        // its own: a list's does not cut it at the list's sides.
        let colour = crate::theme::ACCENT_TEXT.get();
        let bounds = layout.bounds();
        let around = |out: f32, width: f32, colour: Color| renderer::Quad {
            bounds: bounds.expand(out + width),
            border: Border {
                color: colour,
                width,
                radius: (self.radius + out + width).into(),
            },
            ..renderer::Quad::default()
        };
        let reach = RING_OUT + RING;
        renderer.with_layer(bounds.expand(reach + 1.0), |renderer| {
            if crate::theme::contrast() {
                let gap = RING_OUT - 1.0;
                let back = crate::theme::Ink::Token(|t| t.popover_opaque_bottom).get();
                renderer.fill_quad(around(0.0, gap, back), Color::TRANSPARENT);
                renderer.fill_quad(around(gap, RING + 1.0, colour), Color::TRANSPARENT);
            } else {
                renderer.fill_quad(around(RING_OUT, RING, colour), Color::TRANSPARENT);
            }
        });
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
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
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
        renderer: &iced::Renderer,
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
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

// --- A row shown -------------------------------------------------------------------

/// The list `list` scrolled as little as shows the container `target` in
/// it, whole if it fits.
pub fn reveal<T: 'static>(list: Id, target: Id) -> impl Operation<T> {
    struct Reveal {
        list: Id,
        target: Id,
        /// The list's viewport, how far it is scrolled, its content's
        /// height.
        view: Option<(Rectangle, f32, f32)>,
        found: Option<Rectangle>,
    }

    impl<T: 'static> Operation<T> for Reveal {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
            operate(self);
        }

        fn scrollable(
            &mut self,
            id: Option<&Id>,
            bounds: Rectangle,
            content: Rectangle,
            translation: Vector,
            _: &mut dyn Scrollable,
        ) {
            if id == Some(&self.list) {
                self.view = Some((bounds, translation.y, content.height));
            }
        }

        fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
            if id == Some(&self.target) {
                self.found = Some(bounds);
            }
        }

        fn finish(&self) -> Outcome<T> {
            let (Some((view, scroll, content)), Some(row)) = (self.view, self.found) else {
                return Outcome::None;
            };
            let most = (content - view.height).max(0.0);
            let to = kit::reveal((row.y - view.y, row.height), view.height, scroll, most);
            if (to - scroll).abs() < 0.5 {
                return Outcome::None;
            }
            Outcome::Chain(Box::new(scrollable::scroll_to(
                self.list.clone(),
                operation::scrollable::AbsoluteOffset {
                    x: None,
                    y: Some(to),
                },
            )))
        }
    }

    Reveal {
        list,
        target,
        view: None,
        found: None,
    }
}

// --- What the tree shows, by id -----------------------------------------------------

/// What a surface's widgets show, read from the tree once laid out: each
/// container given an id, where it is on the surface (its scrolling taken
/// into account) and the texts in it; the field holding iced's focus. What
/// tests click and read, and what a screen reader's nodes need beyond
/// their role and state (as the Settings' `places`, #660).
#[derive(Debug, Default)]
pub struct Shown {
    /// The translation of the scrollables we are in.
    shift: Vec<Vector>,
    next_shift: Option<Vector>,
    /// The ids we are in.
    within: Vec<String>,
    next_id: Option<String>,
    pub places: std::collections::BTreeMap<String, Rectangle>,
    pub texts: std::collections::BTreeMap<String, Vec<String>>,
    pub focused: Option<String>,
}

/// The name an id was made from.
fn name(id: &Id) -> Option<String> {
    let said = format!("{id:?}");
    let inner = said.strip_prefix("Id(Custom(\"")?.strip_suffix("\"))")?;
    Some(inner.to_owned())
}

impl Shown {
    fn shift(&self) -> Vector {
        self.shift.last().copied().unwrap_or(Vector::ZERO)
    }

    /// As one line of JSON: `{"id": {"at": [x, y, w, h], "says": […]}}`.
    pub fn json(&self) -> String {
        let mut all = serde_json::Map::new();
        for (id, r) in &self.places {
            let says = self.texts.get(id).cloned().unwrap_or_default();
            all.insert(
                id.clone(),
                serde_json::json!({
                    "at": [r.x.round(), r.y.round(), r.width.round(), r.height.round()],
                    "says": says,
                }),
            );
        }
        if let Some(loose) = self.texts.get("") {
            all.insert("".to_owned(), serde_json::json!({ "says": loose }));
        }
        if let Some(focused) = &self.focused {
            all.insert("focus".to_owned(), focused.clone().into());
        }
        serde_json::Value::Object(all).to_string()
    }
}

impl Operation for Shown {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        let shift = self.next_shift.take();
        let id = self.next_id.take();
        if let Some(by) = shift {
            let total = self.shift() + by;
            self.shift.push(total);
        }
        if let Some(id) = &id {
            self.within.push(id.clone());
        }
        operate(self);
        if id.is_some() {
            self.within.pop();
        }
        if shift.is_some() {
            self.shift.pop();
        }
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        self.next_shift = None;
        self.next_id = id.and_then(name);
        if let Some(name) = &self.next_id {
            let shift = self.shift();
            self.places.insert(
                name.clone(),
                Rectangle {
                    x: bounds.x - shift.x,
                    y: bounds.y - shift.y,
                    ..bounds
                },
            );
        }
    }

    fn scrollable(
        &mut self,
        _: Option<&Id>,
        _: Rectangle,
        _: Rectangle,
        translation: Vector,
        _: &mut dyn Scrollable,
    ) {
        self.next_id = None;
        self.next_shift = Some(translation);
    }

    fn focusable(&mut self, id: Option<&Id>, _: Rectangle, state: &mut dyn operation::Focusable) {
        if state.is_focused() {
            self.focused = id.and_then(name);
        }
    }

    fn text(&mut self, _: Option<&Id>, _: Rectangle, text: &str) {
        let within = self.within.last().cloned().unwrap_or_default();
        self.texts.entry(within).or_default().push(text.to_owned());
    }
}
