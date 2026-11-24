//! NetworkManager's form on the authentication dialog's surface (#930,
//! the s32 mockup): the connection's name over what it asks, a VPN
//! plugin's message, a labelled field per secret, "Mot de passe refusé"
//! when the secrets held were refused, "Retenir le mot de passe" where the
//! agent may keep one, Annuler and Se connecter. Its glass, rim, fields
//! and buttons are the system's authentication dialog's ([`super::view`]);
//! the compositor draws its shadow, the dim and its motion.

use hocus_a11y::Role;
use hocus_link::AUTH_NAMESPACE;
use hocus_network::agent::{Asked, Kind, Wanted, What};
use hocus_words::shell::{auth as auth_txt, secrets as txt};
use iced::{
    Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, Vector, mouse,
    widget::{
        Space, Stack,
        canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke, gradient},
        text::Alignment,
    },
    window,
};

use super::{
    Auth, Target,
    form::Form,
    view::{
        self, BORDER, BUTTON, BUTTON_GAP, BUTTON_PAD, BUTTON_RADIUS, BUTTON_TEXT, CAPS, FIELD,
        FIELD_GAP, FIELD_LEFT, FIELD_ON, FIELD_RADIUS, FIELD_RIGHT, FIELD_TEXT, FOOTER_GAP, GLASS,
        INK, LEFT, MESSAGE, MESSAGE_GAP, MESSAGE_LINE, Msg, NOTE, NOTE_GAP, NOTE_LINE, OPAQUE,
        PAD_BOTTOM, PAD_TOP, PEACH, RADIUS, RIGHT, RIM, TILE, TILE_GAP, TILE_RADIUS, TITLE,
        TITLE_GAP, WIDTH, at, draw_authorize, draw_cancel, draw_field_box, fill_rounded, id_of,
        pressed, rim, text, width,
    },
};
use crate::{
    Message,
    clock::time::Lang,
    field::{
        Entry,
        view::{Look, Style},
    },
    levels::draw::{ACCENT, DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
    popover::kit::{self, MANROPE_SEMIBOLD},
    surfaces::Lent,
    theme::Ink,
    ui::{Around, control, field::Field, parts::drawn_out},
};

type El<'a> = Element<'a, Msg>;

/// The header: the 44 px tile, its 22 px padlock; the connection's name
/// (17 px 700) over what it asks (13 px).
const ICON: f32 = 22.0;
const ASKS: f32 = 13.0;
/// A field's label, 6 px over it: 12 px 600.
const LABEL: f32 = 12.0;
const LABEL_GAP: f32 = 6.0;
/// "Retenir le mot de passe", 14 px under: a 16 px box (radius 4), 8 px
/// before its words (13 px), on a 22 px row.
const CHECK_GAP: f32 = 14.0;
const CHECK: f32 = 16.0;
const CHECK_RADIUS: f32 = 4.0;
const CHECK_TEXT_GAP: f32 = 8.0;
const CHECK_TEXT: f32 = 13.0;
const CHECK_ROW: f32 = 22.0;

const TINT: Ink = Ink::Dark(Color::from_rgba8(143, 179, 255, 0.14));
const BLUE: Ink = Ink::Dark(rgb(0x8FB3FF));
const FIELD_OFF: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.14));
const CHECK_INK: Ink = Ink::Dark(rgb(0xD5DBE5));
const CHECK_OFF: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.30));

/// The form's words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    lang: Lang,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    fn pick(self, text: hocus_words::Text) -> &'static str {
        text.get(self.lang)
    }

    /// What the connection asks, under its name.
    pub fn asks(self, asked: &Asked) -> &'static str {
        self.pick(match asked.kind {
            Kind::Vpn => txt::VPN_ASKS,
            Kind::Wifi => txt::WIFI_ASKS,
            Kind::Other => txt::NETWORK_ASKS,
        })
    }

    /// A secret's field's label: what it is, else its key.
    pub fn label(self, wanted: &Wanted) -> String {
        match wanted.what {
            What::User => self.pick(txt::USER_NAME).to_owned(),
            What::Password => self.pick(txt::PASSWORD).to_owned(),
            What::KeyPassword => self.pick(txt::KEY_PASSWORD).to_owned(),
            What::Code => self.pick(txt::ONE_TIME_CODE).to_owned(),
            What::Other => wanted.key.clone(),
        }
    }

    pub fn remember(self) -> &'static str {
        self.pick(txt::REMEMBER_THE_PASSWORD)
    }

    pub fn refused(self) -> &'static str {
        self.pick(txt::PASSWORD_REFUSED)
    }

    pub fn cancel(self) -> &'static str {
        self.pick(auth_txt::CANCEL)
    }

    pub fn connect(self) -> &'static str {
        self.pick(txt::CONNECT)
    }
}

pub fn words() -> Words {
    Words::new(Lang::of_messages())
}

/// Where the form's parts are, and its size.
#[derive(Debug, Clone)]
pub struct Layout {
    pub height: f32,
    pub tile: Rectangle,
    pub name_at: Point,
    pub asks_at: Point,
    pub name: String,
    pub message: Vec<String>,
    pub message_top: f32,
    /// Each field's label (its words, its middle's height) and box.
    pub labels: Vec<(String, f32)>,
    pub fields: Vec<Rectangle>,
    pub note_top: Option<f32>,
    pub check: Option<Rectangle>,
    pub footer: f32,
    pub cancel: Rectangle,
    pub connect: Rectangle,
}

impl Layout {
    pub fn of(form: &Form, w: Words) -> Self {
        let room = RIGHT - LEFT;
        let asked = &form.ask.asked;
        let mut y = BORDER + PAD_TOP;
        let tile = Rectangle::new(Point::new(LEFT, y), Size::new(TILE, TILE));
        let words_x = LEFT + TILE + TILE_GAP;
        let block = TITLE * LINE + TITLE_GAP + ASKS * LINE;
        let block_top = y + (TILE - block) / 2.0;
        let name_at = Point::new(words_x, block_top + TITLE * LINE / 2.0);
        let asks_at = Point::new(
            words_x,
            block_top + TITLE * LINE + TITLE_GAP + ASKS * LINE / 2.0,
        );
        let name = kit::elide(&asked.name, RIGHT - words_x, |t| {
            width(t, TITLE, MANROPE_BOLD)
        });
        y += TILE;
        let message = asked
            .message
            .as_deref()
            .map(|m| kit::wrap(m, room, |t| width(t, MESSAGE, MANROPE)))
            .unwrap_or_default();
        if !message.is_empty() {
            y += MESSAGE_GAP;
        }
        let message_top = y;
        y += message.len() as f32 * MESSAGE * MESSAGE_LINE;
        let mut labels = Vec::new();
        let mut fields = Vec::new();
        for wanted in &asked.wanted {
            y += FIELD_GAP;
            labels.push((w.label(wanted), y + LABEL * LINE / 2.0));
            y += LABEL * LINE + LABEL_GAP;
            fields.push(Rectangle::new(Point::new(LEFT, y), Size::new(room, FIELD)));
            y += FIELD;
        }
        let note_top = asked.new_ones().then(|| {
            y += NOTE_GAP;
            let top = y;
            y += NOTE * NOTE_LINE;
            top
        });
        let check = asked.rememberable().then(|| {
            y += CHECK_GAP;
            let row = Rectangle::new(
                Point::new(LEFT, y),
                Size::new(
                    CHECK + CHECK_TEXT_GAP + width(w.remember(), CHECK_TEXT, MANROPE),
                    CHECK_ROW,
                ),
            );
            y += CHECK_ROW;
            row
        });
        y += FOOTER_GAP;
        let footer = y;
        let connect_w = BUTTON_PAD + width(w.connect(), BUTTON_TEXT, MANROPE_BOLD) + BUTTON_PAD;
        let connect = Rectangle::new(
            Point::new(RIGHT - connect_w, y),
            Size::new(connect_w, BUTTON),
        );
        let cancel_w = BUTTON_PAD + width(w.cancel(), BUTTON_TEXT, MANROPE_SEMIBOLD) + BUTTON_PAD;
        let cancel = Rectangle::new(
            Point::new(connect.x - BUTTON_GAP - cancel_w, y),
            Size::new(cancel_w, BUTTON),
        );
        y += BUTTON + PAD_BOTTOM + BORDER;
        Self {
            height: y,
            tile,
            name_at,
            asks_at,
            name,
            message,
            message_top,
            labels,
            fields,
            note_top,
            check,
            footer,
            cancel,
            connect,
        }
    }

    /// The surface's size.
    pub fn surface(&self) -> (u32, u32) {
        (WIDTH as u32, self.height.ceil() as u32)
    }
}

/// The form on its surface `id`, on the glass (else opaque).
pub fn view<'a>(
    auth: &'a Lent<Auth>,
    id: window::Id,
    words: Words,
    glass: bool,
) -> Element<'a, Message> {
    Around::new(
        id,
        build(auth, id, words),
        move |m| view::handle(auth, id, m),
        Msg::Stale,
    )
    .own(AUTH_NAMESPACE, move || !view::closing(auth, id))
    .rebuild(move || build(auth, id, words))
    .keys(move |pressed| view::keys(auth, id, pressed))
    .on(iced::widget::canvas(Back {
        auth,
        id,
        words,
        glass,
    })
    .width(Length::Fill)
    .height(Length::Fill))
}

/// Its content, as the form is now; said in the log when it changed.
fn build<'a>(auth: &'a Lent<Auth>, id: window::Id, words: Words) -> El<'a> {
    let built = auth.read(|a| a.drawn_form(id, |form, caps| content(auth, id, form, caps, words)));
    let Some(Some((content, said))) = built else {
        return Space::new().into();
    };
    if !view::closing(auth, id) {
        auth.change(|a| {
            if a.said.as_ref() != Some(&said) {
                tracing::debug!(?said, "the network's dialog shows it");
                a.said = Some(said);
            }
        });
    }
    content
}

/// The form's controls where its layout places them, over what [`Back`]
/// draws, in the keyboard's order; and what it says (never a secret, nor
/// what a field holds: its length only).
pub(super) fn content<'a>(
    auth: &'a Lent<Auth>,
    id: window::Id,
    form: &Form,
    caps: bool,
    w: Words,
) -> (El<'a>, Vec<String>) {
    let layout = Layout::of(form, w);
    let asked = &form.ask.asked;
    let mut said = vec![layout.name.clone(), w.asks(asked).to_owned()];
    if !layout.message.is_empty() {
        said.push(layout.message.join(" "));
    }
    let mut all: Vec<El<'a>> = Vec::new();
    let ring = |target: Target| form.focus == target;
    for (i, ((field, (label, _)), wanted)) in layout
        .fields
        .iter()
        .copied()
        .zip(&layout.labels)
        .zip(&asked.wanted)
        .enumerate()
    {
        let target = Target::Entry(i);
        let focused = ring(target);
        let masked = wanted.what.masked();
        let caps_shown = caps && focused && masked;
        let border = if focused { FIELD_ON } else { FIELD_OFF };
        let look = drawn_out(field.width, field.height, 1.0, move |f, _| {
            f.translate(Vector::new(-field.x, -field.y));
            draw_field_box(f, field, border.get(), caps_shown);
        });
        let typed = Field {
            with: Box::new(move |with: &mut dyn FnMut(&mut Entry)| {
                auth.change(|a| a.entry_at(id, i, with));
            }),
            style: Style {
                hint: (FIELD_TEXT, MANROPE),
                ..Style::one(FIELD_TEXT, if masked { MANROPE_BOLD } else { MANROPE })
            },
            placeholder: String::new(),
            look: Look::of(TEXT.get(), FAINT.get()),
            focused,
            left: FIELD_LEFT + 2.0,
            right: FIELD_RIGHT + if caps_shown { CAPS + 6.0 } else { 0.0 },
            on_focus: Msg::Focus(target),
            on_took: Msg::Took,
        }
        .view(field.height);
        let stacked = Stack::with_children([look, typed])
            .width(field.width)
            .height(field.height);
        let role = if masked {
            Role::PasswordField
        } else {
            Role::TextField
        };
        all.push(at(
            control(stacked, id_of(target), role, label.clone())
                .focus(focused, Msg::Focus(target), FIELD_RADIUS)
                .activate(Msg::Act(Target::Connect))
                .typing(),
            field.position(),
        ));
        let chars = form.entries.get(i).map_or(0, Entry::chars);
        said.push(format!("{label}: {chars}"));
    }
    if layout.note_top.is_some() {
        said.push(w.refused().to_owned());
    }
    if let Some(check) = layout.check {
        let on = form.remember;
        let button = pressed(check, Target::Remember, move |f, _| {
            draw_check(f, check, w, on);
        });
        all.push(at(
            control(
                button,
                id_of(Target::Remember),
                Role::CheckBox,
                w.remember(),
            )
            .node(|n| n.checked(on))
            .focus(
                ring(Target::Remember),
                Msg::Focus(Target::Remember),
                CHECK_RADIUS,
            )
            .activate(Msg::Act(Target::Remember)),
            check.position(),
        ));
        said.push(format!(
            "{} {}",
            if on { "[x]" } else { "[ ]" },
            w.remember()
        ));
    }
    let cancel = layout.cancel;
    let button = pressed(cancel, Target::Cancel, move |f, lit| {
        draw_cancel(f, cancel, super::words(), lit);
    });
    all.push(at(
        control(button, id_of(Target::Cancel), Role::Button, w.cancel())
            .focus(
                ring(Target::Cancel),
                Msg::Focus(Target::Cancel),
                BUTTON_RADIUS,
            )
            .activate(Msg::Act(Target::Cancel)),
        cancel.position(),
    ));
    let ready = form.ready();
    let connect = layout.connect;
    let middle = layout.footer + BUTTON / 2.0;
    let button = pressed(connect, Target::Connect, move |f, _| {
        draw_authorize(f, connect, middle, w.connect(), false, ready);
    });
    all.push(at(
        control(button, id_of(Target::Connect), Role::Button, w.connect())
            .node(|n| n.disabled(!ready))
            .focus(
                ring(Target::Connect),
                Msg::Focus(Target::Connect),
                BUTTON_RADIUS,
            )
            .activate(Msg::Act(Target::Connect)),
        connect.position(),
    ));
    said.push(format!("[{}]", w.cancel()));
    said.push(format!(
        "[{}{}]",
        w.connect(),
        if ready { "" } else { " (inert)" }
    ));
    let content = Stack::with_children(all)
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    (content, said)
}

/// What the form draws under its controls: its glass (else opaque) and
/// rim, the connection and what it asks, the message, the fields' labels
/// and the note. Nothing once it faded out.
#[derive(Debug, Clone, Copy)]
struct Back<'a> {
    auth: &'a Lent<Auth>,
    id: window::Id,
    words: Words,
    glass: bool,
}

impl<M> canvas::Program<M> for Back<'_> {
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
        self.auth.read(|a| {
            a.drawn_form(self.id, |form, _| {
                draw_back(&mut frame, form, self.words, self.glass);
            })
        });
        vec![frame.into_geometry()]
    }
}

fn draw_back(f: &mut Frame, form: &Form, w: Words, glass: bool) {
    let layout = Layout::of(form, w);
    let size = f.size();
    let (top, bottom) = if glass { GLASS } else { OPAQUE };
    let tint = gradient::Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
        .add_stop(0.0, top.get())
        .add_stop(1.0, bottom.get());
    f.fill(
        &Path::rounded_rectangle(Point::ORIGIN, size, RADIUS.into()),
        canvas::Fill::from(tint),
    );
    rim(
        f,
        Rectangle::new(Point::ORIGIN, size),
        RADIUS,
        RIM.get(),
        BORDER,
    );
    // The header.
    fill_rounded(f, layout.tile, TILE_RADIUS, TINT.get());
    padlock(f, layout.tile);
    text(
        f,
        &layout.name,
        layout.name_at,
        TITLE,
        TEXT.get(),
        MANROPE_BOLD,
    );
    text(
        f,
        w.asks(&form.ask.asked),
        layout.asks_at,
        ASKS,
        DIM.get(),
        MANROPE,
    );
    for (i, line) in layout.message.iter().enumerate() {
        let middle = layout.message_top + (i as f32 + 0.5) * MESSAGE * MESSAGE_LINE;
        f.fill_text(canvas::Text {
            line_height: iced::widget::text::LineHeight::Relative(MESSAGE_LINE),
            ..kit::text(
                line.clone(),
                Point::new(LEFT, middle),
                MESSAGE,
                TEXT.get(),
                Alignment::Left,
            )
        });
    }
    for (label, middle) in &layout.labels {
        text(
            f,
            label,
            Point::new(LEFT, *middle),
            LABEL,
            DIM.get(),
            MANROPE_SEMIBOLD,
        );
    }
    if let Some(top) = layout.note_top {
        text(
            f,
            w.refused(),
            Point::new(LEFT, top + NOTE * NOTE_LINE / 2.0),
            NOTE,
            PEACH.get(),
            MANROPE,
        );
    }
}

/// The mockup's padlock, `ICON` px in the tile's middle: a 14 × 10 body
/// (radius 2.5) under `M8.5 10.5 V8 A3.5 3.5 0 0 1 15.5 8 V10.5`.
fn padlock(f: &mut Frame, tile: Rectangle) {
    let k = ICON / 24.0;
    let o = Point::new(tile.center_x() - ICON / 2.0, tile.center_y() - ICON / 2.0);
    let p = |x: f32, y: f32| Point::new(o.x + x * k, o.y + y * k);
    let line = Stroke::default()
        .with_color(BLUE.get())
        .with_width(1.8 * k)
        .with_line_cap(LineCap::Round)
        .with_line_join(LineJoin::Round);
    f.stroke(
        &kit::rounded(
            Rectangle::new(p(5.0, 10.5), Size::new(14.0 * k, 10.0 * k)),
            2.5 * k,
        ),
        line,
    );
    let shackle = Path::new(|b| {
        b.move_to(p(8.5, 10.5));
        b.line_to(p(8.5, 8.0));
        b.arc(canvas::path::Arc {
            center: p(12.0, 8.0),
            radius: 3.5 * k,
            start_angle: iced::Radians(std::f32::consts::PI),
            end_angle: iced::Radians(std::f32::consts::TAU),
        });
        b.line_to(p(15.5, 10.5));
    });
    f.stroke(&shackle, line);
}

/// "Retenir le mot de passe" at `row`: its box green and ticked when on,
/// a rim when off.
fn draw_check(f: &mut Frame, row: Rectangle, w: Words, on: bool) {
    let rect = Rectangle::new(
        Point::new(row.x, row.center_y() - CHECK / 2.0),
        Size::new(CHECK, CHECK),
    );
    if on {
        fill_rounded(f, rect, CHECK_RADIUS, ACCENT.get());
        // The mockup's `M2 5.2 L4 7.2 L8 3`, 10 px in the box's middle.
        let p = |x: f32, y: f32| Point::new(rect.x + 3.0 + x, rect.y + 3.0 + y);
        let tick = Path::new(|b| {
            b.move_to(p(2.0, 5.2));
            b.line_to(p(4.0, 7.2));
            b.line_to(p(8.0, 3.0));
        });
        f.stroke(
            &tick,
            Stroke::default()
                .with_color(INK.get())
                .with_width(1.8)
                .with_line_cap(LineCap::Round)
                .with_line_join(LineJoin::Round),
        );
    } else {
        rim(f, rect, CHECK_RADIUS, CHECK_OFF.get(), 1.5);
    }
    text(
        f,
        w.remember(),
        Point::new(rect.x + CHECK + CHECK_TEXT_GAP, row.center_y()),
        CHECK_TEXT,
        CHECK_INK.get(),
        MANROPE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_words() {
        let fr = Words::new(Lang::French);
        let en = Words::new(Lang::English);
        assert_eq!(fr.remember(), "Retenir le mot de passe");
        assert_eq!(en.connect(), "Connect");
        assert_eq!(fr.cancel(), "Annuler");
        let code = Wanted {
            key: "challenge-response".into(),
            what: What::Code,
            keep: hocus_network::agent::Keep::Never,
            known: None,
        };
        assert_eq!(fr.label(&code), "Code à usage unique");
    }
}
