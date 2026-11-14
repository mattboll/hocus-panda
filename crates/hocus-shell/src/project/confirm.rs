//! Closing a project's confirmation (docs/design/projects.md §5, board 4),
//! on the widget kit, the mockup's sizes.
//!
//! 480 px wide, 22 px inside (18 at the bottom), its parts 14 apart,
//! radius 16, `#141926` in a 1 px rim, centred on the project's screen
//! over the compositor's dim. Its monogram on a 40 px badge (radius 12,
//! the accent's gradient, 14 px 800), "Fermer le projet hocus-site ?" (17
//! px, 700) and its folder under it (Martian Mono 12 px, `text.faint`);
//! the line saying what comes back (13 px, `#A9B2C2`); a row each for what
//! comes back (34 px, 13 px, its application's 18 px icon) on a faint
//! ground; what closing interrupts in the waiting state's amber (12.5 px,
//! `#F5D29A` on `rgba(245,180,74,0.10)`, a warning sign); "Annuler" and
//! "Fermer le projet" on the right (36 px, 13.5 px). The keyboard rests on
//! "Fermer le projet" (§11): `Enter` closes, `Esc` cancels, `Tab` goes
//! between the two.

use hocus_a11y::Role;
use hocus_words::shell::project as txt;
use iced::{
    Color, Element, Length, Padding, Point, Rectangle, Size,
    alignment::Vertical,
    keyboard,
    widget::{
        Space, button,
        canvas::{self, Path, Stroke},
        column, container, row,
    },
    window,
};

use super::{Projects, Words, card::Tint};
use crate::theme::Ink;
use crate::{
    clock::time::Lang,
    levels::draw::{MANROPE, MANROPE_BOLD, TEXT, rgb},
    popover::{Effect, Pressed, kit},
    surfaces::Lent,
    ui::{
        self, Around, control,
        parts::{drawn, gap},
        style::{self, said},
    },
};

type El<'a> = Element<'a, Msg>;

const WIDTH: f32 = hocus_link::PROJECT_CLOSE_WIDTH as f32;
const BORDER: f32 = 1.0;
const PAD: f32 = 22.0;
const PAD_BOTTOM: f32 = 18.0;
const INNER: f32 = WIDTH - 2.0 * (BORDER + PAD);
const GAP: f32 = 14.0;
const RADIUS: f32 = hocus_link::PROJECT_CLOSE_RADIUS as f32;
const BADGE: f32 = 40.0;
const BADGE_RADIUS: f32 = 12.0;
const BADGE_TEXT: f32 = 14.0;
const TITLE: f32 = 17.0;
const FOLDER: f32 = 12.0;
const BODY: f32 = 13.0;
const BODY_LINE: f32 = 1.5;
const ROWS_PAD: f32 = 6.0;
const ROW: f32 = 34.0;
const ROW_GAP: f32 = 2.0;
const ROW_PAD: f32 = 6.0;
const ICON: f32 = 18.0;
const ROW_TEXT: f32 = 13.0;
const WARN: f32 = 12.5;
const WARN_PAD: (f32, f32) = (10.0, 12.0);
const WARN_ICON: f32 = 16.0;
const WARN_GAP: f32 = 10.0;
const BUTTON: f32 = 36.0;
const BUTTON_PAD: f32 = 16.0;
const BUTTON_TEXT: f32 = 13.5;
const BUTTON_GAP: f32 = 8.0;
const BUTTON_RADIUS: f32 = 9.0;
const BUTTONS_TOP: f32 = 2.0;

const MARTIAN: iced::Font = iced::Font::with_name("Martian Mono");
const MANROPE_SEMIBOLD: iced::Font = iced::Font {
    weight: iced::font::Weight::Semibold,
    ..MANROPE
};
const MANROPE_EXTRABOLD: iced::Font = iced::Font {
    weight: iced::font::Weight::ExtraBold,
    ..MANROPE
};

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

const FILL: Ink = Ink::Dark(rgb(0x141926));
const RIM: Ink = Ink::Rim(rgba(255, 255, 255, 0.10));
const FAINT: Ink = Ink::Dark(rgb(0x8C96A8));
const SOFT: Ink = Ink::Dark(rgb(0xA9B2C2));
const ROWS_FILL: Ink = Ink::Dark(rgba(255, 255, 255, 0.035));
const ROWS_RIM: Ink = Ink::Dark(rgba(255, 255, 255, 0.07));
const AMBER_FILL: Ink = Ink::Dark(rgba(245, 180, 74, 0.10));
const AMBER_RIM: Ink = Ink::Dark(rgba(245, 180, 74, 0.28));
const AMBER_TEXT: Ink = Ink::Dark(rgb(0xF5D29A));
const AMBER: Ink = Ink::Dark(rgb(0xF5B44A));
const CANCEL_FILL: Ink = Ink::Dark(rgba(255, 255, 255, 0.06));
const CANCEL_RIM: Ink = Ink::Dark(rgba(255, 255, 255, 0.12));
const HOVER: Ink = Ink::Dark(rgba(255, 255, 255, 0.10));
const ON_ACCENT: Ink = Ink::Fixed(rgb(0x0B1020));

/// What holds the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Cancel,
    Close,
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    Focus(Focus),
    Cancel,
    Close,
    Key(Pressed),
    Stale,
}

/// A row of what comes back: its icon's app, and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Back {
    pub app: String,
    pub said: String,
}

/// What comes back, one row each, in words.
pub fn back_rows(shown: &hocus_link::ProjectConfirm, lang: Lang) -> Vec<Back> {
    shown
        .back
        .iter()
        .map(|b| match b {
            hocus_link::ComingBack::App { app, entry, count } => {
                let name = super::app_name(entry, app);
                Back {
                    app: app.clone(),
                    said: if *count > 1 {
                        txt::back_windows(
                            lang,
                            txt::BackWindows {
                                app: &name,
                                n: count,
                            },
                        )
                    } else {
                        name
                    },
                }
            }
            hocus_link::ComingBack::Terminal { app, folder, .. } => Back {
                app: app.clone(),
                said: txt::back_terminal(lang, last_part(folder)),
            },
            hocus_link::ComingBack::NotBack { app } => Back {
                app: app.clone(),
                said: txt::not_back(lang, app),
            },
        })
        .collect()
}

/// A folder's last part ("hocus-site").
fn last_part(folder: &str) -> &str {
    let folder = folder.trim_end_matches('/');
    folder
        .rsplit('/')
        .next()
        .filter(|p| !p.is_empty())
        .unwrap_or(folder)
}

/// What closing interrupts, in one paragraph.
pub fn interrupted(shown: &hocus_link::ProjectConfirm, lang: Lang) -> String {
    shown
        .interrupted
        .iter()
        .map(|i| match i {
            hocus_link::Interrupted::Agent { tool, waiting } => {
                if *waiting {
                    txt::agent_waiting(lang, tool)
                } else {
                    txt::agent_working(lang, tool)
                }
            }
            hocus_link::Interrupted::Program(program) => txt::program_stops(lang, program),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The closing's toast (§5): "hocus-site est fermé, ses 3 fenêtres
/// gardées." (and the windows that stayed), then how it comes back.
pub fn closed_said(words: Words, name: &str, kept: u32, stayed: &[String]) -> (String, String) {
    let lang = words.lang;
    let mut said = if kept == 0 {
        txt::closed_empty(lang, name)
    } else {
        txt::closed_toast(lang, txt::ClosedToast { n: kept, name })
    };
    if !stayed.is_empty() {
        let names = stayed.join(", ");
        let n = stayed.len() as u64;
        said.push(' ');
        said.push_str(&txt::stayed(lang, txt::Stayed { n, names: &names }));
    }
    (said, txt::closed_then(lang, name))
}

/// The paragraph's lines, as they break in the warning.
fn warning_lines(text: &str) -> Vec<String> {
    let room = INNER - 2.0 * WARN_PAD.1 - 2.0 * BORDER - WARN_ICON - WARN_GAP;
    kit::wrap(text, room, |t| kit::measure(t, WARN, MANROPE))
}

fn body_lines(text: &str) -> Vec<String> {
    kit::wrap(text, INNER, |t| kit::measure(t, BODY, MANROPE))
}

/// Its size, logical pixels.
pub fn size(shown: &hocus_link::ProjectConfirm, words: Words) -> (u32, u32) {
    let lang = words.lang;
    let rows = back_rows(shown, lang).len() as f32;
    let body = body_lines(txt::CONFIRM_LINE.get(lang)).len() as f32 * BODY * BODY_LINE;
    let rows_h = if rows > 0.0 {
        2.0 * (ROWS_PAD + BORDER) + rows * ROW + (rows - 1.0) * ROW_GAP + GAP
    } else {
        0.0
    };
    let warning = interrupted(shown, lang);
    let warning_h = if warning.is_empty() {
        0.0
    } else {
        warning_lines(&warning).len() as f32 * WARN * BODY_LINE + 2.0 * (WARN_PAD.0 + BORDER) + GAP
    };
    let height = BORDER
        + PAD
        + BADGE
        + GAP
        + body
        + GAP
        + rows_h
        + warning_h
        + BUTTONS_TOP
        + BUTTON
        + PAD_BOTTOM
        + BORDER;
    (WIDTH as u32, height.ceil() as u32)
}

/// The confirmation on its surface `id`.
pub fn view<'a>(
    projects: &'a Lent<Projects>,
    id: window::Id,
    words: Words,
    _glass: bool,
) -> Element<'a, crate::Message> {
    Around::new(
        id,
        build(projects, id, words),
        move |m| widget(projects, m),
        Msg::Stale,
    )
    .own(hocus_link::PROJECT_CLOSE_NAMESPACE, move || {
        projects
            .read(|p| p.confirm_on(id).is_some())
            .unwrap_or(false)
    })
    .rebuild(move || build(projects, id, words))
    .keys(move |pressed| Some(Msg::Key(pressed.clone())))
    .on(iced::widget::canvas(Tint {
        radius: RADIUS,
        top: FILL,
        bottom: FILL,
        rim: RIM,
    })
    .width(Length::Fill)
    .height(Length::Fill))
}

fn widget(projects: &Lent<Projects>, message: Msg) {
    let now = std::time::Instant::now();
    let done = projects.change(|p| {
        let effects = handle(p, message, now);
        (effects, p.due().is_some())
    });
    let Some((effects, due)) = done else {
        return;
    };
    if !effects.is_empty() {
        crate::popover::send(effects);
    }
    if due {
        crate::surfaces::ask(crate::surfaces::Asked::Project(super::Come::Tick));
    }
}

fn handle(p: &mut Projects, message: Msg, now: std::time::Instant) -> Vec<Effect> {
    let Some(confirm) = p.confirm.as_deref_mut() else {
        return Vec::new();
    };
    match message {
        Msg::Stale => Vec::new(),
        Msg::Focus(focus) => {
            confirm.focus = focus;
            Vec::new()
        }
        Msg::Cancel => {
            tracing::info!("a project's closing cancelled");
            let mut effects = Vec::new();
            p.close(now, &mut effects);
            effects
        }
        Msg::Close => {
            let name = confirm.shown.name.clone();
            tracing::info!(name, "a project's closing confirmed");
            let mut effects = vec![Effect::Tell(hocus_link::ToCompositor::ProjectClose(name))];
            p.close(now, &mut effects);
            effects
        }
        Msg::Key(pressed) => key(p, &pressed, now),
    }
}

/// A key on the confirmation.
pub fn key(p: &mut Projects, pressed: &Pressed, now: std::time::Instant) -> Vec<Effect> {
    use keyboard::{Key, key::Named};
    let Some(confirm) = p.confirm.as_deref_mut() else {
        return Vec::new();
    };
    match &pressed.key {
        Key::Named(Named::Escape) => handle(p, Msg::Cancel, now),
        Key::Named(Named::Enter | Named::Space) => {
            let message = match confirm.focus {
                Focus::Cancel => Msg::Cancel,
                Focus::Close => Msg::Close,
            };
            handle(p, message, now)
        }
        Key::Named(Named::Tab | Named::ArrowLeft | Named::ArrowRight) => {
            confirm.focus = match confirm.focus {
                Focus::Cancel => Focus::Close,
                Focus::Close => Focus::Cancel,
            };
            super::woken()
        }
        _ => Vec::new(),
    }
}

fn build<'a>(projects: &'a Lent<Projects>, id: window::Id, words: Words) -> El<'a> {
    projects
        .change(|p| {
            let confirm = p.confirm.as_deref_mut().filter(|c| c.surface == id)?;
            let lang = words.lang;
            let rows = back_rows(&confirm.shown, lang);
            let warning = interrupted(&confirm.shown, lang);
            let now = format!(
                "name={:?} rows={:?} interrupted={warning:?}",
                confirm.shown.name,
                rows.iter().map(|r| r.said.as_str()).collect::<Vec<_>>()
            );
            if now != confirm.logged {
                tracing::debug!(said = %now, "the project's confirmation shows it");
                confirm.logged.clone_from(&now);
            }
            Some(content(confirm, &rows, &warning, words))
        })
        .flatten()
        .unwrap_or_else(|| Space::new().into())
}

/// The project's monogram on its badge.
fn badge<'a>(mono: String, side: f32, radius: f32, size: f32) -> El<'a> {
    drawn(side, side, move |f, s| {
        let rect = Rectangle::new(Point::ORIGIN, s);
        let fill = canvas::gradient::Linear::new(Point::ORIGIN, Point::new(s.width, s.height))
            .add_stop(0.0, rgb(0x3FA66B))
            .add_stop(1.0, rgb(0x7EE0A0));
        f.fill(
            &Path::rounded_rectangle(rect.position(), rect.size(), radius.into()),
            canvas::Fill::from(fill),
        );
        f.fill_text(canvas::Text {
            content: mono.clone(),
            position: rect.center(),
            max_width: f32::INFINITY,
            color: ON_ACCENT.get(),
            size: iced::Pixels(size),
            line_height: iced::widget::text::LineHeight::Relative(1.0),
            font: MANROPE_EXTRABOLD,
            align_x: iced::widget::text::Alignment::Center,
            align_y: Vertical::Center,
            shaping: iced::widget::text::Shaping::Basic,
        });
    })
}

fn content<'a>(confirm: &super::Confirm, rows: &[Back], warning: &str, words: Words) -> El<'a> {
    let lang = words.lang;
    let shown = &confirm.shown;
    let title = txt::confirm_title(lang, &shown.name);
    let title_room = INNER - BADGE - 12.0;
    let title = kit::elide(&title, title_room, |t| kit::measure(t, TITLE, MANROPE_BOLD));
    let folder = kit::elide_start(
        &crate::desktops::folder_shown(&shown.folder),
        title_room,
        |t| kit::measure(t, FOLDER, MARTIAN),
    );
    let head = row![
        badge(
            hocus_link::projects::monogram(&shown.name),
            BADGE,
            BADGE_RADIUS,
            BADGE_TEXT
        ),
        gap(12.0),
        column![
            ui::line(said(title.clone(), TITLE, MANROPE_BOLD, TEXT.get()), title),
            ui::line(said(folder.clone(), FOLDER, MARTIAN, FAINT.get()), folder),
        ]
        .spacing(2.0),
    ]
    .height(BADGE)
    .align_y(Vertical::Center);
    let mut all = column![head, Space::new().height(GAP)];
    let line = txt::CONFIRM_LINE.get(lang);
    let mut body = column![];
    for l in body_lines(line) {
        body = body.push(
            said(l, BODY, MANROPE, SOFT.get())
                .line_height(iced::widget::text::LineHeight::Relative(BODY_LINE)),
        );
    }
    all = all
        .push(ui::line(body, line))
        .push(Space::new().height(GAP));
    if !rows.is_empty() {
        let mut list = column![].spacing(ROW_GAP);
        for back in rows {
            let found = crate::desktops::icons::of(&back.app);
            let letter = back
                .app
                .rsplit('.')
                .next()
                .filter(|n| !n.is_empty())
                .unwrap_or(&back.app)
                .to_owned();
            let icon = drawn(ICON, ICON, move |f, s| {
                kit::app_tile(
                    f,
                    Rectangle::new(Point::ORIGIN, s),
                    (found.as_ref(), &letter),
                    (5.0, 10.0),
                    1.0,
                );
            });
            let room = INNER - 2.0 * (ROWS_PAD + BORDER) - 2.0 * ROW_PAD - ICON - 10.0;
            let text = kit::elide(&back.said, room, |t| kit::measure(t, ROW_TEXT, MANROPE));
            let line = row![icon, gap(10.0), said(text, ROW_TEXT, MANROPE, TEXT.get())]
                .align_y(Vertical::Center);
            list = list.push(ui::line(
                container(line)
                    .padding(Padding {
                        left: ROW_PAD,
                        right: ROW_PAD,
                        ..Padding::ZERO
                    })
                    .height(ROW)
                    .align_y(Vertical::Center),
                back.said.clone(),
            ));
        }
        let boxed = container(list)
            .width(INNER)
            .padding(ROWS_PAD + BORDER)
            .style(|_| container::Style {
                background: Some(ROWS_FILL.get().into()),
                border: iced::Border {
                    color: ROWS_RIM.get(),
                    width: BORDER,
                    radius: 10.0.into(),
                },
                ..container::Style::default()
            });
        all = all.push(boxed).push(Space::new().height(GAP));
    }
    if !warning.is_empty() {
        let mut lines = column![];
        for l in warning_lines(warning) {
            lines = lines.push(
                said(l, WARN, MANROPE, AMBER_TEXT.get())
                    .line_height(iced::widget::text::LineHeight::Relative(BODY_LINE)),
            );
        }
        let sign = drawn(WARN_ICON, WARN_ICON, |f, s| warning_sign(f, s, AMBER.get()));
        let boxed = container(
            row![
                column![Space::new().height(2.0), sign],
                gap(WARN_GAP),
                ui::line(lines, warning.to_owned())
            ]
            .align_y(Vertical::Top),
        )
        .width(INNER)
        .padding(Padding {
            top: WARN_PAD.0 + BORDER,
            bottom: WARN_PAD.0 + BORDER,
            left: WARN_PAD.1 + BORDER,
            right: WARN_PAD.1 + BORDER,
        })
        .style(|_| container::Style {
            background: Some(AMBER_FILL.get().into()),
            border: iced::Border {
                color: AMBER_RIM.get(),
                width: BORDER,
                radius: 10.0.into(),
            },
            ..container::Style::default()
        });
        all = all.push(boxed).push(Space::new().height(GAP));
    }
    all = all
        .push(Space::new().height(BUTTONS_TOP))
        .push(buttons(confirm.focus, words));
    container(all)
        .padding(Padding {
            top: BORDER + PAD,
            right: BORDER + PAD,
            bottom: 0.0,
            left: BORDER + PAD,
        })
        .into()
}

/// "Annuler" and "Fermer le projet", on the right.
fn buttons<'a>(focus: Focus, words: Words) -> El<'a> {
    let lang = words.lang;
    let face = |word: &str, font, ink: Color| {
        container(said(word.to_owned(), BUTTON_TEXT, font, ink))
            .center_y(Length::Fill)
            .padding(Padding {
                left: BUTTON_PAD,
                right: BUTTON_PAD,
                ..Padding::ZERO
            })
    };
    let cancel_word = txt::CANCEL.get(lang);
    let cancel = button(face(cancel_word, MANROPE_SEMIBOLD, TEXT.get()))
        .padding(0)
        .height(BUTTON)
        .style(|_, status| button::Style {
            background: Some(
                if style::lit(status) {
                    HOVER.get()
                } else {
                    CANCEL_FILL.get()
                }
                .into(),
            ),
            border: iced::Border {
                color: CANCEL_RIM.get(),
                width: 1.0,
                radius: BUTTON_RADIUS.into(),
            },
            ..style::clear()
        })
        .on_press(Msg::Cancel);
    let cancel = control(cancel, "project:Cancel", Role::Button, cancel_word)
        .focus(
            focus == Focus::Cancel,
            Msg::Focus(Focus::Cancel),
            BUTTON_RADIUS,
        )
        .activate(Msg::Cancel);
    let close_word = txt::CLOSE_CONFIRM.get(lang);
    let close = button(face(close_word, MANROPE_BOLD, ON_ACCENT.get()))
        .padding(0)
        .height(BUTTON)
        .style(|_, status| {
            let a = if style::lit(status) { 0.9 } else { 1.0 };
            style::filled(
                crate::theme::ACCENT.get().scale_alpha(a),
                BUTTON_RADIUS,
                ON_ACCENT.get(),
            )
        })
        .on_press(Msg::Close);
    let close = control(close, "project:Close", Role::Button, close_word)
        .focus(
            focus == Focus::Close,
            Msg::Focus(Focus::Close),
            BUTTON_RADIUS,
        )
        .activate(Msg::Close);
    row![
        iced::widget::space::horizontal(),
        cancel,
        gap(BUTTON_GAP),
        close
    ]
    .width(INNER)
    .height(BUTTON)
    .into()
}

/// The warning sign (the mockup's 24 px triangle), `size` large.
fn warning_sign(f: &mut canvas::Frame, size: Size, ink: Color) {
    let k = size.width / 24.0;
    let p = |x: f32, y: f32| Point::new(x * k, y * k);
    let stroke = Stroke {
        line_cap: canvas::LineCap::Round,
        line_join: canvas::LineJoin::Round,
        ..Stroke::default().with_color(ink).with_width(2.0 * k)
    };
    f.stroke(
        &Path::new(|b| {
            b.move_to(p(12.0, 3.0));
            b.line_to(p(2.0, 20.0));
            b.line_to(p(22.0, 20.0));
            b.close();
        }),
        stroke,
    );
    f.stroke(
        &Path::new(|b| {
            b.move_to(p(12.0, 10.0));
            b.line_to(p(12.0, 14.0));
            b.move_to(p(12.0, 17.0));
            b.line_to(p(12.01, 17.0));
        }),
        stroke,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn confirm() -> hocus_link::ProjectConfirm {
        hocus_link::ProjectConfirm {
            output: "eDP-1".into(),
            screen: (1440, 900),
            name: "hocus-site".into(),
            folder: "/home/alex/github/hocus-site".into(),
            back: vec![hocus_link::ComingBack::Terminal {
                app: "foot".into(),
                entry: "foot".into(),
                folder: "/home/alex/github/hocus-site/".into(),
            }],
            interrupted: vec![
                hocus_link::Interrupted::Agent {
                    tool: "Claude Code".into(),
                    waiting: false,
                },
                hocus_link::Interrupted::Program("npm run dev".into()),
            ],
        }
    }

    #[test]
    fn it_says_what_comes_back_and_what_stops() {
        let shown = confirm();
        let rows = back_rows(&shown, Lang::French);
        assert_eq!(rows[0].said, "Un terminal dans hocus-site");
        assert_eq!(
            interrupted(&shown, Lang::French),
            "Claude Code travaille encore : fermer interrompt sa réponse en cours. \
             npm run dev s’arrête ; son terminal reviendra dans le dossier, sans relancer la commande."
        );
        let (said, then) = closed_said(Words::new(Lang::French), "hocus-site", 3, &[]);
        assert_eq!(said, "hocus-site est fermé, ses 3 fenêtres gardées.");
        assert_eq!(then, "Super+Espace, « projet hocus-site » : il revient.");
        let (said, _) = closed_said(Words::new(Lang::English), "blog", 1, &["Gedit".to_owned()]);
        assert_eq!(said, "blog is closed, its window kept. Gedit stays open.");
    }
}
