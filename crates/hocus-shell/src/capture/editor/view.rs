//! The editor on the widget kit (#772): its tools, its colours, undo and
//! its three buttons are the kit's controls, placed where its layout puts
//! them and drawing their looks; the text being typed is the shared field
//! over its dashed box. The image takes the drags that draw a mark (a
//! canvas of its own, which draws the mark being drawn); what nothing
//! holds is drawn under them ([`Back`]).

use std::{
    cell::Cell,
    rc::Rc,
    sync::{Mutex, PoisonError},
};

use hocus_a11y::Role;
use hocus_link::CAPTURE_EDITOR_NAMESPACE;
use iced::{
    Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, Vector, mouse,
    widget::{
        Space, Stack, button,
        canvas::{self, Action, Frame, Geometry},
        pin,
    },
    window,
};

use super::{
    COLOURS, Control, Drag, Editor, Phase, TEXT, TEXT_IN, Tool, Typing,
    draw::{self, Back, Layout},
};
use crate::{
    Message,
    capture::Words,
    field::{
        Entry, Took,
        view::{Look, Style},
    },
    levels::draw::MANROPE_BOLD,
    popover::Pressed,
    ui::{self, Around, control, field::Field, parts::drawn_lit, sized::Sized, style},
};

type El<'a> = Element<'a, Msg>;

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A control acts: clicked, `Enter` or `Space` on it.
    Act(Control),
    /// The ring comes to a control.
    Focus(Control),
    /// What the text's field did with a key, a press or the input method.
    Took(Took),
    /// The image pressed, the pointer moved while it is, released (the
    /// surface's frame).
    Press(Point),
    Move(Point),
    Release,
    /// `Enter` in the text: it becomes a mark.
    Commit,
    /// `Escape`: the text goes, else the editor closes.
    Escape,
    Undo,
    Redo,
    /// What it shows changed under the view.
    Stale,
}

/// The editor's surface's size, as it was laid out: its controls are
/// placed from it.
static SIZE: Mutex<Size> = Mutex::new(Size::ZERO);

fn size() -> Size {
    *SIZE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A control's id: what tests and screen readers know it by.
fn id_of(control: Control) -> String {
    format!("capture:{control:?}")
}

/// The editor on its surface `id`, on the glass (else opaque).
pub fn view<'a>(id: window::Id, words: Words, glass: bool) -> Element<'a, Message> {
    let back = iced::widget::canvas(Back { id, words, glass })
        .width(Length::Fill)
        .height(Length::Fill);
    let seen: Rc<Cell<Size>> = Rc::default();
    let (built, rebuilt, stale) = (seen.clone(), seen.clone(), seen);
    Around::new(id, build(id, words, &built), handle, Msg::Stale)
        .own(CAPTURE_EDITOR_NAMESPACE, move || super::is_surface(id))
        .rebuild(move || build(id, words, &rebuilt))
        .stale_when(move || size() != stale.get())
        .keys(keys)
        .on(back)
}

fn canvas<P: canvas::Program<Msg>>(program: P) -> canvas::Canvas<P, Msg> {
    iced::widget::canvas(program)
}

/// The keys the editor takes before the kit: `Escape`, `Enter` in the
/// text, and `Ctrl+Z` and `Ctrl+Shift+Z` out of it.
fn keys(pressed: &Pressed) -> Option<Msg> {
    use iced::keyboard::{Key, key::Named};
    let typing = super::with(|e| e.as_ref().is_some_and(|e| e.typing.is_some()));
    let z = matches!(&pressed.key, Key::Character(c) if c.eq_ignore_ascii_case("z"));
    match &pressed.key {
        Key::Named(Named::Escape) => Some(Msg::Escape),
        Key::Named(Named::Enter) if typing => Some(Msg::Commit),
        _ if z && !typing && pressed.modifiers.control() => Some(if pressed.modifiers.shift() {
            Msg::Redo
        } else {
            Msg::Undo
        }),
        _ => None,
    }
}

/// The editor shown, and its layout on its surface as last laid out.
fn shown<T>(with: impl FnOnce(&mut Editor, &Layout) -> T) -> Option<T> {
    let size = size();
    let words = crate::capture::words();
    super::with(|e| {
        let e = e.as_mut().filter(|e| matches!(e.phase, Phase::Shown(_)))?;
        let layout = Layout::of(size, words, draw::logical(e));
        Some(with(e, &layout))
    })
}

fn clamp(p: Point, r: Rectangle) -> Point {
    Point::new(
        p.x.clamp(r.x, r.x + r.width),
        p.y.clamp(r.y, r.y + r.height),
    )
}

/// What a message asks that the editor does outside its lock.
enum Then {
    Paste,
    Close,
    Press(Control, Rectangle, f32),
}

fn handle(message: Msg) {
    let then = shown(|e, layout| {
        let (image, z) = (layout.image, layout.z);
        match message {
            Msg::Act(Control::Tool(tool)) => {
                e.commit_text(image, z);
                e.tool = tool;
            }
            Msg::Act(Control::Colour(i)) => e.colour = i,
            Msg::Act(Control::Undo) | Msg::Undo => {
                e.undo();
            }
            Msg::Redo => {
                e.redo();
            }
            Msg::Act(control @ (Control::Delete | Control::SaveAs | Control::Copy)) => {
                return Some(Then::Press(control, image, z));
            }
            Msg::Act(Control::Text) | Msg::Commit => {
                e.commit_text(image, z);
                e.focus = None;
            }
            Msg::Focus(Control::Text) => e.focus = Some(Control::Text),
            Msg::Focus(control) => {
                e.commit_text(image, z);
                e.focus = Some(control);
            }
            Msg::Took(Took::Paste) => return Some(Then::Paste),
            Msg::Press(p) if e.tool == Tool::Text => {
                // Elsewhere than in the text being typed (the field takes
                // its own presses): it becomes a mark, and another starts.
                e.commit_text(image, z);
                e.typing = Some(Typing::at(p));
                e.focus = Some(Control::Text);
            }
            Msg::Press(p) => e.drag = Some(Drag { from: p, to: p }),
            Msg::Move(p) => {
                if let Some(drag) = e.drag.as_mut() {
                    drag.to = clamp(p, image);
                }
            }
            Msg::Release => {
                if let Some(drag) = e.drag.take()
                    && let Some(mark) = e.mark_of(image, drag, z)
                {
                    e.add(mark);
                }
            }
            Msg::Escape if e.typing.is_some() => {
                e.typing = None;
                e.focus = None;
            }
            // Nothing changes.
            Msg::Escape => return Some(Then::Close),
            Msg::Took(_) | Msg::Stale => {}
        }
        None
    })
    .flatten();
    match then {
        Some(Then::Paste) => super::paste(),
        Some(Then::Close) => super::close(super::Closed::Kept),
        Some(Then::Press(control, image, z)) => super::press(control, image, z),
        None => {}
    }
}

/// `content` placed at `at`.
fn at<'a>(content: impl Into<El<'a>>, at: Point) -> El<'a> {
    pin(content).x(at.x).y(at.y).into()
}

/// A button `rect` large drawn by `draw` in the surface's frame (and
/// whether the pointer is over it), doing `control`.
fn pressed<'a>(
    rect: Rectangle,
    control: Control,
    draw: impl Fn(&mut Frame, bool) + 'a,
) -> button::Button<'a, Msg> {
    button(drawn_lit(rect.width, rect.height, 1.0, move |f, _, lit| {
        f.translate(Vector::new(-rect.x, -rect.y));
        draw(f, lit);
    }))
    .padding(0)
    .style(|_, _| style::clear())
    .on_press(Msg::Act(control))
}

/// The text being typed, for the field's widget.
fn entry(with: &mut dyn FnMut(&mut Entry)) {
    super::with(|e| {
        if let Some(typing) = e.as_mut().and_then(|e| e.typing.as_mut()) {
            with(&mut typing.text);
        }
    });
}

/// The editor's controls on its surface `id`, from the size it was laid
/// out at (none before it is known); told the size it is laid out at.
fn build<'a>(id: window::Id, words: Words, seen: &Cell<Size>) -> El<'a> {
    let size = size();
    seen.set(size);
    let content = super::with(|e| {
        let e = e.as_mut().filter(|e| e.phase == Phase::Shown(id))?;
        let layout = Layout::of(size, words, draw::logical(e));
        let said = layout.said();
        if size.width > 0.0 && said != e.laid {
            tracing::debug!(places = %said, "the capture's editor is laid out");
            e.laid = said;
        }
        (size.width > 0.0).then(|| controls(e, &layout, words))
    })
    .unwrap_or_else(|| Space::new().into());
    Sized::new(content, |room| {
        *SIZE.lock().unwrap_or_else(PoisonError::into_inner) = room;
    })
    .into()
}

/// The image, the tools, the colours, undo and the buttons where `layout`
/// puts them, in the keyboard's order; the text being typed over them.
fn controls<'a>(e: &Editor, layout: &Layout, w: Words) -> El<'a> {
    let z = layout.z;
    let ring = |control: Control| e.focus == Some(control);
    let mut all: Vec<El<'a>> = Vec::new();
    // The image: it takes the drags, and draws the mark being drawn.
    let image = layout.image;
    all.push(at(
        control(
            canvas(Image { tool: e.tool, z })
                .width(image.width)
                .height(image.height),
            "capture:Image",
            Role::Label,
            e.shot.name(),
        ),
        image.position(),
    ));
    let n = layout.tools.len();
    for (i, (tool, rect)) in layout.tools.iter().copied().enumerate() {
        let on = e.tool == tool;
        let label = w.tool(tool);
        let c = Control::Tool(tool);
        let button = pressed(rect, c, move |f, lit| {
            draw::draw_tool(f, rect, tool, label, on, lit, z);
        });
        all.push(at(
            control(button, id_of(c), Role::RadioButton, label)
                .node(|node| node.checked(on).set(i + 1, n))
                .focus(ring(c), Msg::Focus(c), draw::BUTTON_RADIUS * z)
                .activate(Msg::Act(c))
                .rows(0),
            rect.position(),
        ));
    }
    let n = layout.colours.len();
    for (i, rect) in layout.colours.iter().copied().enumerate() {
        let on = e.colour == i;
        let c = Control::Colour(i);
        let button = pressed(rect, c, move |f, _| draw::draw_swatch(f, rect, i, on, z));
        all.push(at(
            control(button, id_of(c), Role::RadioButton, w.colour(i))
                .node(|node| node.checked(on).set(i + 1, n))
                .focus(ring(c), Msg::Focus(c), rect.width / 2.0)
                .activate(Msg::Act(c))
                .rows(1),
            rect.position(),
        ));
    }
    let undo = layout.undo;
    let button = pressed(undo, Control::Undo, move |f, lit| {
        draw::draw_undo(f, undo, lit, z);
    });
    all.push(at(
        control(button, id_of(Control::Undo), Role::Button, w.undo())
            .node(|node| node.disabled(e.marks.is_empty()))
            .focus(
                ring(Control::Undo),
                Msg::Focus(Control::Undo),
                draw::BUTTON_RADIUS * z,
            )
            .activate(Msg::Act(Control::Undo)),
        undo.position(),
    ));
    for (c, rect, word) in [
        (Control::Delete, layout.delete, w.delete()),
        (Control::SaveAs, layout.save_as, w.save_as()),
    ] {
        let button = pressed(rect, c, move |f, lit| {
            draw::draw_foot(f, rect, word, c == Control::Delete, lit, z);
        });
        all.push(at(
            control(button, id_of(c), Role::Button, word)
                .focus(ring(c), Msg::Focus(c), draw::FOOT_RADIUS * z)
                .activate(Msg::Act(c)),
            rect.position(),
        ));
    }
    let copy = layout.copy;
    let word = w.copy_and_close();
    let button = pressed(copy, Control::Copy, move |f, _| {
        draw::draw_copy(f, copy, word, z);
    });
    all.push(at(
        control(button, id_of(Control::Copy), Role::Button, word)
            .focus(
                ring(Control::Copy),
                Msg::Focus(Control::Copy),
                draw::FOOT_RADIUS * z,
            )
            .activate(Msg::Act(Control::Copy)),
        copy.position(),
    ));
    // The text being typed, in the mark's colour and where the mark will
    // be, in its dashed box.
    if let Some(typing) = &e.typing {
        let [r, g, b] = COLOURS[e.colour];
        let colour = Color::from_rgb8(r, g, b);
        let field = draw::field_of(typing.at, z);
        let look = ui::parts::drawn_out(field.width, field.height, 1.0, move |f, _| {
            f.translate(Vector::new(-field.x, -field.y));
            draw::draw_field_box(f, field, colour, z);
        });
        let typed = Field {
            with: Box::new(entry),
            style: Style::one(TEXT.0 * z, MANROPE_BOLD),
            placeholder: w.text_hint().to_owned(),
            look: Look::of(colour, draw::HINT.get()),
            focused: ring(Control::Text),
            left: TEXT_IN * z,
            right: 0.0,
            on_focus: Msg::Focus(Control::Text),
            on_took: Msg::Took,
        }
        .view(field.height);
        all.push(at(
            control(
                Stack::with_children([look, typed])
                    .width(field.width)
                    .height(field.height),
                id_of(Control::Text),
                Role::TextField,
                w.tool(Tool::Text),
            )
            .node(|node| node.describe(typing.text.text().to_owned()))
            .focus(
                ring(Control::Text),
                Msg::Focus(Control::Text),
                draw::FIELD_RADIUS * z,
            )
            .activate(Msg::Commit)
            .typing(),
            field.position(),
        ));
    }
    Stack::with_children(all)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// The image's place: it takes the presses and drags that draw on it,
/// and draws the mark being drawn (the image and its marks are under it,
/// [`Back`]), cut to the image as the mark will be.
struct Image {
    tool: Tool,
    /// The editor's zoom.
    z: f32,
}

impl canvas::Program<Msg> for Image {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Msg>> {
        let iced::Event::Mouse(seen) = event else {
            return None;
        };
        let dragging = super::with(|e| e.as_ref().is_some_and(|e| e.drag.is_some()));
        match seen {
            mouse::Event::CursorMoved { position } if dragging => {
                Some(Action::publish(Msg::Move(*position)).and_capture())
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                // Not under what is over it (the text's field).
                cursor.position_in(bounds)?;
                let p = cursor.position()?;
                Some(Action::publish(Msg::Press(p)).and_capture())
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) if dragging => {
                Some(Action::publish(Msg::Release).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        super::with(|e| {
            let Some(e) = e.as_ref() else {
                return;
            };
            let Some(drag) = e.drag else {
                return;
            };
            // The image's place on the surface: the canvas's own.
            if let Some(mark) = e.mark_of(bounds, drag, self.z) {
                f.translate(Vector::new(-bounds.x, -bounds.y));
                draw::draw_mark(&mut f, e, bounds, &mark);
            }
        });
        vec![f.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if !cursor.is_over(bounds) {
            mouse::Interaction::default()
        } else if self.tool == Tool::Text {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::Crosshair
        }
    }
}

/// The editor for screen readers: what its controls declared.
pub fn a11y() -> Option<hocus_a11y::Window> {
    let words = crate::capture::words();
    let id = super::with(|e| match e.as_ref()?.phase {
        Phase::Shown(id) => Some(id),
        Phase::Choosing => None,
    })?;
    let rows = crate::a11y::rows::of(id)?;
    let size = size();
    let mut window = hocus_a11y::Window::new("capture-editor", words.editor());
    window.nodes = rows.nodes;
    window.focus = rows.focus;
    window.active = true;
    window.size = Some((size.width, size.height));
    Some(window)
}

/// A screen reader acts on control `key` of the editor.
pub fn a11y_click(key: &str) {
    let control = match key {
        "capture:Undo" => Control::Undo,
        "capture:Delete" => Control::Delete,
        "capture:SaveAs" => Control::SaveAs,
        "capture:Copy" => Control::Copy,
        _ => {
            let tool = Tool::ALL
                .into_iter()
                .find(|t| key == id_of(Control::Tool(*t)));
            let colour = (0..COLOURS.len()).find(|i| key == id_of(Control::Colour(*i)));
            match (tool, colour) {
                (Some(tool), _) => Control::Tool(tool),
                (_, Some(i)) => Control::Colour(i),
                _ => return,
            }
        }
    };
    handle(Msg::Act(control));
    crate::popover::send(vec![crate::popover::Effect::Wake(CAPTURE_EDITOR_NAMESPACE)]);
}
