//! The meeting's transcription's asking popover (docs/design/meetings.md),
//! opened by the meeting card's Transcrire (or the mini popover's), as the
//! bar's others (§9), 280 wide, 14 px in, its parts 10 px apart:
//! "TRANSCRIRE" (11 px, 700, spaced 0.1 em, `text.faint`), the meeting's
//! title (15 px, 700, two lines at most), what happens (12.5 px,
//! `#C9D0DC`), the reminder in amber (`#F5B44A`), "Télécharger le modèle
//! (610 Mo)" when the models are not there yet, **Transcrire** (34 px,
//! radius 9, `#5B8DEF`), **Annuler** (30 px, radius 8,
//! `rgba(255,255,255,0.06)`, `#A9B2C2`), and "Ne plus demander" (12 px,
//! `text.faint`, a box to tick).

use hocus_words::shell::transcribe as txt;
use std::sync::{Mutex, PoisonError};

use hocus_a11y::Role;
use iced::{
    Color, Element, Length, Point, Rectangle, Size,
    alignment::Vertical,
    widget::{
        Space, button,
        canvas::{LineCap, LineJoin, Path, Stroke},
        column, container, row,
    },
    window,
};

use super::{
    Event as Seen, event,
    kit::{self, MANROPE_SEMIBOLD},
};
use crate::theme::Ink;
use crate::{
    clock::time::Lang,
    levels::draw::{FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
    meeting::draw::JOIN_BLUE,
    ui::{
        Around, control,
        parts::{drawn, gap, spaced_by},
        style::{self, said},
    },
};

type El<'a> = Element<'a, Msg>;

pub const WIDTH: u32 = 280;
const BORDER: f32 = 1.0;
const PAD: f32 = 14.0;
const GAP: f32 = 10.0;
const LABEL: f32 = 11.0;
const LABEL_SPACING: f32 = 0.1;
const TITLE: f32 = 15.0;
const TITLE_LINES: usize = 2;
const BODY: f32 = 12.5;
/// The body's lines: 1.5 of its size apart.
const BODY_LINE: f32 = 1.5;
const GO: f32 = 34.0;
const GO_RADIUS: f32 = 9.0;
const GO_TEXT: f32 = 13.5;
const CANCEL: f32 = 30.0;
const CANCEL_RADIUS: f32 = 8.0;
const CHECK: f32 = 13.0;
const CHECK_TEXT: f32 = 12.0;
const CHECK_GAP: f32 = 8.0;
const EXPLAIN: Ink = Ink::Dark(rgb(0xC9D0DC));
const WARN: Ink = Ink::Dark(rgb(0xF5B44A));
const CANCEL_INK: Ink = Ink::Dark(rgb(0xA9B2C2));
const CANCEL_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
const CANCEL_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const GO_HOVER: Ink = Ink::Dark(Color::from_rgb8(0x6E, 0x9B, 0xF2));
const TICK: Ink = Ink::Dark(rgb(0x6CC287));
const TICK_INK: Ink = Ink::Dark(rgb(0x0B1020));
const BOX_RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.30));

/// What a control does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Go,
    Cancel,
    NeverAsk,
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("transcribe:{self:?}")
    }
}

/// What it says, as laid out for the card now.
#[derive(Debug, Clone, PartialEq)]
struct Layout {
    title: Vec<String>,
    explain: Vec<String>,
    warn: Vec<String>,
    /// "Télécharger le modèle (610 Mo)", the models not there yet.
    download: Option<String>,
    height: f32,
}

fn layout(lang: Lang, title: &str) -> Layout {
    let room = WIDTH as f32 - 2.0 * (BORDER + PAD);
    let mut lines = kit::wrap(title, room, |t| kit::measure(t, TITLE, MANROPE_BOLD));
    if lines.len() > TITLE_LINES {
        let rest = lines[TITLE_LINES - 1..].join(" ");
        lines.truncate(TITLE_LINES - 1);
        lines.push(kit::elide(&rest, room, |t| {
            kit::measure(t, TITLE, MANROPE_BOLD)
        }));
    }
    let body = |text: &str| kit::wrap(text, room, |t| kit::measure(t, BODY, MANROPE));
    let explain = body(txt::EXPLAIN.get(lang));
    let warn = body(txt::WARN.get(lang));
    let missing = hocus_meetings::Places::from_env()
        .map(|p| hocus_meetings::models::missing(&p.models()))
        .unwrap_or_default();
    let download = (!missing.is_empty())
        .then(|| txt::download(lang, hocus_meetings::models::megabytes(&missing)));
    let body_lines = |n: usize| n as f32 * BODY * BODY_LINE;
    let mut y = BORDER + PAD + LABEL * LINE;
    y += GAP + lines.len() as f32 * TITLE * LINE;
    y += GAP + body_lines(explain.len());
    y += GAP + body_lines(warn.len());
    if download.is_some() {
        y += GAP + body_lines(1);
    }
    y += GAP + GO + GAP + CANCEL + GAP + CHECK_TEXT * LINE;
    Layout {
        title: lines,
        explain,
        warn,
        download,
        height: y + PAD + BORDER,
    }
}

/// Its height, for the card now.
pub fn height() -> u32 {
    let lang = Lang::of_messages();
    let title = crate::meeting::snapshot()
        .map(|c| c.title)
        .unwrap_or_default();
    layout(lang, &title).height.round() as u32
}

/// The popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub glass: bool,
}

#[derive(Debug, Default)]
struct Ui {
    focus: Option<Target>,
    said: Option<String>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// The popover opens: nothing holds the ring, nothing ticked.
pub fn opening() {
    ui(|ui| *ui = Ui::default());
    crate::transcribe::asking();
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    Act(Target),
    Focus(Target),
    Pressed,
    Stale,
}

/// The popover on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    Around::new(panel.id, build(), handle, Msg::Stale)
        .rebuild(build)
        .press(Msg::Pressed)
        .on_glass(panel.glass)
}

fn build<'a>() -> El<'a> {
    let lang = Lang::of_messages();
    let Some(card) = crate::meeting::snapshot() else {
        return Space::new().into();
    };
    let layout = layout(lang, &card.title);
    let ticked = crate::transcribe::ticked();
    ui(|ui| {
        say(lang, &layout, ticked, ui);
        content(lang, &layout, ticked, ui.focus)
    })
}

fn lines<'a>(lines: &[String], ink: Color) -> El<'a> {
    let mut all = column![];
    for line in lines {
        all = all.push(
            container(said(line.clone(), BODY, MANROPE, ink))
                .height(BODY * BODY_LINE)
                .align_y(Vertical::Center),
        );
    }
    all.into()
}

fn content<'a>(lang: Lang, layout: &Layout, ticked: bool, focus: Option<Target>) -> El<'a> {
    let label = container(spaced_by(
        txt::HEADING.get(lang),
        LABEL,
        MANROPE_BOLD,
        FAINT.get(),
        LABEL_SPACING,
    ))
    .height(LABEL * LINE)
    .align_y(Vertical::Center);
    let mut title = column![];
    for line in &layout.title {
        title = title.push(said(line.clone(), TITLE, MANROPE_BOLD, TEXT.get()));
    }
    let mut all = column![
        label,
        crate::ui::line(title, layout.title.join(" ")),
        crate::ui::line(
            lines(&layout.explain, EXPLAIN.get()),
            txt::EXPLAIN.get(lang)
        ),
        crate::ui::line(lines(&layout.warn, WARN.get()), txt::WARN.get(lang)),
    ]
    .spacing(GAP);
    if let Some(download) = &layout.download {
        all = all.push(crate::ui::line(
            lines(std::slice::from_ref(download), EXPLAIN.get()),
            download.clone(),
        ));
    }
    all = all
        .push(button_of(lang, Target::Go, focus))
        .push(button_of(lang, Target::Cancel, focus))
        .push(never_ask(lang, ticked, focus));
    container(all).padding(BORDER + PAD).into()
}

fn button_of<'a>(lang: Lang, target: Target, focus: Option<Target>) -> El<'a> {
    let (word, height, radius, font, ink) = match target {
        Target::Go => (
            txt::TRANSCRIBE.get(lang),
            GO,
            GO_RADIUS,
            MANROPE_BOLD,
            Color::WHITE,
        ),
        _ => (
            txt::CANCEL.get(lang),
            CANCEL,
            CANCEL_RADIUS,
            MANROPE_SEMIBOLD,
            CANCEL_INK.get(),
        ),
    };
    let size = if target == Target::Go { GO_TEXT } else { BODY };
    let pressed = button(container(said(word, size, font, ink)).center(Length::Fill))
        .padding(0)
        .width(Length::Fill)
        .height(height)
        .style(move |_, status| {
            let lit = style::lit(status);
            let fill = match (target, lit) {
                (Target::Go, false) => JOIN_BLUE,
                (Target::Go, true) => GO_HOVER,
                (_, false) => CANCEL_FILL,
                (_, true) => CANCEL_HOVER,
            };
            button::Style {
                background: Some(fill.get().into()),
                border: style::round(radius),
                ..button::Style::default()
            }
        })
        .on_press(Msg::Act(target));
    control(pressed, target.id(), Role::Button, word)
        .focus(focus == Some(target), Msg::Focus(target), radius)
        .activate(Msg::Act(target))
        .rows(0)
        .into()
}

fn never_ask<'a>(lang: Lang, ticked: bool, focus: Option<Target>) -> El<'a> {
    let word = txt::NEVER_ASK.get(lang);
    let check = button(
        row![
            drawn(CHECK, CHECK, move |f, size| {
                tick_box(f, Rectangle::new(Point::ORIGIN, size), ticked);
            }),
            gap(CHECK_GAP),
            said(word, CHECK_TEXT, MANROPE, FAINT.get()),
        ]
        .align_y(Vertical::Center),
    )
    .padding(0)
    .height(CHECK_TEXT * LINE)
    .style(|_, _| style::clear())
    .on_press(Msg::Act(Target::NeverAsk));
    control(check, Target::NeverAsk.id(), Role::CheckBox, word)
        .node(move |n| n.checked(ticked))
        .focus(
            focus == Some(Target::NeverAsk),
            Msg::Focus(Target::NeverAsk),
            3.0,
        )
        .activate(Msg::Act(Target::NeverAsk))
        .rows(0)
        .into()
}

/// A box to tick at `rect`: a green tick when `on`.
fn tick_box(f: &mut iced::widget::canvas::Frame, rect: Rectangle, on: bool) {
    let radius = 3.0;
    if on {
        f.fill(&kit::rounded(rect, radius), TICK.get());
        let k = rect.width / 16.0;
        let p = |x: f32, y: f32| Point::new(rect.x + x * k, rect.y + y * k);
        let tick = Path::new(|b| {
            b.move_to(p(4.0, 8.5));
            b.line_to(p(7.0, 11.5));
            b.line_to(p(12.5, 5.0));
        });
        f.stroke(
            &tick,
            Stroke::default()
                .with_color(TICK_INK.get())
                .with_width(2.0 * k)
                .with_line_cap(LineCap::Round)
                .with_line_join(LineJoin::Round),
        );
    } else {
        f.stroke(
            &kit::rounded(
                Rectangle::new(
                    Point::new(rect.x + 0.75, rect.y + 0.75),
                    Size::new(rect.width - 1.5, rect.height - 1.5),
                ),
                radius,
            ),
            Stroke::default().with_color(BOX_RIM.get()).with_width(1.5),
        );
    }
}

/// What it shows, for the log, when it changed.
fn say(lang: Lang, layout: &Layout, ticked: bool, ui: &mut Ui) {
    let mut said = vec![
        txt::HEADING.get(lang).to_owned(),
        layout.title.join(" / "),
        layout.explain.join(" "),
        layout.warn.join(" "),
    ];
    if let Some(download) = &layout.download {
        said.push(download.clone());
    }
    said.push(txt::TRANSCRIBE.get(lang).to_owned());
    said.push(txt::CANCEL.get(lang).to_owned());
    said.push(format!(
        "{} {}",
        if ticked { "[x]" } else { "[ ]" },
        txt::NEVER_ASK.get(lang)
    ));
    let said = said.join(" | ");
    if ui.said.as_deref() != Some(said.as_str()) {
        tracing::debug!(%said, "the transcription's popover shows it");
        ui.said = Some(said);
    }
}

fn handle(message: Msg) {
    match message {
        Msg::Stale => {}
        Msg::Pressed => ui(|ui| ui.focus = None),
        Msg::Focus(target) => ui(|ui| ui.focus = Some(target)),
        Msg::Act(Target::Go) => {
            super::send(crate::transcribe::start().into_iter().collect());
            event(Seen::Chosen);
        }
        Msg::Act(Target::Cancel) => event(Seen::Chosen),
        Msg::Act(Target::NeverAsk) => {
            crate::transcribe::tick(!crate::transcribe::ticked());
            super::send(vec![super::Effect::Rebuild]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_says_what_happens_in_both_languages() {
        assert_eq!(txt::HEADING.get(Lang::French), "TRANSCRIRE");
        assert_eq!(
            txt::EXPLAIN.get(Lang::French),
            "Le son reste sur ce portable. Le texte est écrit à la fin de la réunion, dans Documents › Réunions."
        );
        assert_eq!(
            txt::download(Lang::French, 610),
            "Télécharger le modèle (610 Mo)"
        );
        assert_eq!(
            txt::download(Lang::English, 610),
            "Download the model (610 MB)"
        );
        assert_eq!(
            (
                txt::NEVER_ASK.get(Lang::French),
                txt::CANCEL.get(Lang::French)
            ),
            ("Ne plus demander", "Annuler")
        );
    }
}
