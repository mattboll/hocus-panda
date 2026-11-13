//! The form that makes a desktop a project (docs/design/projects.md §2.1,
//! board 1), on the widget kit, the mockup's sizes.
//!
//! 400 px wide, 16 inside, its parts 12 apart, radius 14, the popovers'
//! glass. The title (15 px, 700) and its line (12 px, `text.faint`); "NOM"
//! (11 px, 700, capitals spaced 0.08 em) over the field (36 px, radius 8,
//! its rim in the accent at 60 %); "DOSSIER · DÉPÔTS GIT RÉCENTS" over the
//! repositories, a 42 px row each, 2 px apart (an 18 px folder, the name
//! 13 px 600, its path in Martian Mono 10.5 px, when on the right, 11 px),
//! the chosen one in the accent's tint and rim; "Choisir un autre
//! dossier…" (36 px, 12.5 px); "Annuler" and "Créer le projet" (34 px, 13
//! px) on the right. What is wrong shows under its part (12.5 px,
//! `#F7B98A`), the field's rim `#F2945B`.

use hocus_a11y::Role;
use iced::{
    Color, Element, Length, Padding, Point, Rectangle,
    alignment::Vertical,
    keyboard,
    widget::{
        Space, button,
        canvas::{Path, Stroke},
        column, container, row,
    },
    window,
};

use super::{Fault, Focus, Folder, Form, Other, Projects, Words};
use crate::theme::Ink;
use crate::{
    field::{
        Took,
        view::{Look, Style},
    },
    levels::draw::{LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
    popover::{Effect, Pressed, kit},
    surfaces::Lent,
    ui::{
        self, Around, control,
        field::Field,
        parts::{drawn, gap, spaced_by},
        style::{self, said},
    },
};

type El<'a> = Element<'a, Msg>;

const WIDTH: f32 = hocus_link::PROJECT_FORM_WIDTH as f32;
const BORDER: f32 = 1.0;
const PAD: f32 = 16.0;
const INNER: f32 = WIDTH - 2.0 * (BORDER + PAD);
const GAP: f32 = 12.0;
const RADIUS: f32 = hocus_link::PROJECT_FORM_RADIUS as f32;
const TITLE: f32 = 15.0;
const TITLE_GAP: f32 = 3.0;
const BODY: f32 = 12.0;
const BODY_LINE: f32 = 1.45;
const LABEL: f32 = 11.0;
const LABEL_GAP: f32 = 6.0;
const FIELD: f32 = 36.0;
const FIELD_RADIUS: f32 = 8.0;
const FIELD_PAD: f32 = 12.0;
const TYPED: f32 = 14.0;
const ROW: f32 = 42.0;
const ROW_GAP: f32 = 2.0;
const ROW_PAD: f32 = 10.0;
const ROW_RADIUS: f32 = 8.0;
const ICON: f32 = 18.0;
const ICON_GAP: f32 = 10.0;
const REPO: f32 = 13.0;
const PATH: f32 = 10.5;
const NOTE: f32 = 11.0;
const OTHER: f32 = 36.0;
const OTHER_TEXT: f32 = 12.5;
const BUTTON: f32 = 34.0;
const BUTTON_PAD: f32 = 14.0;
const BUTTON_TEXT: f32 = 13.0;
const BUTTON_GAP: f32 = 8.0;
const ERROR: f32 = 12.5;
const ERROR_GAP: f32 = 4.0;

const MARTIAN: iced::Font = iced::Font::with_name("Martian Mono");
const MANROPE_SEMIBOLD: iced::Font = kit::MANROPE_SEMIBOLD;

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

const GLASS_TOP: Ink = Ink::Dark(rgba(22, 27, 42, 0.97));
const GLASS_BOTTOM: Ink = Ink::Dark(rgba(14, 17, 28, 0.98));
const OPAQUE_TOP: Ink = Ink::Dark(rgb(0x161B2A));
const OPAQUE_BOTTOM: Ink = Ink::Dark(rgb(0x0E111C));
const RIM: Ink = Ink::Rim(rgba(255, 255, 255, 0.10));
const FAINT: Ink = Ink::Dark(rgb(0x8C96A8));
const QUIET: Ink = Ink::Dark(rgb(0x7C8698));
const ICON_INK: Ink = Ink::Dark(rgb(0xA9B2C2));
const FIELD_FILL: Ink = Ink::Dark(rgba(255, 255, 255, 0.06));
const FIELD_RIM: Ink = Ink::Accent(0.6);
const FIELD_RIM_OFF: Ink = Ink::Dark(rgba(255, 255, 255, 0.12));
const FIELD_REFUSED: Ink = Ink::Dark(rgb(0xF2945B));
const PEACH: Ink = Ink::Dark(rgb(0xF7B98A));
const CHOSEN_FILL: Ink = Ink::Accent(0.16);
const CHOSEN_RIM: Ink = Ink::Accent(0.6);
const HOVER: Ink = Ink::Dark(rgba(255, 255, 255, 0.06));
const CANCEL_FILL: Ink = Ink::Dark(rgba(255, 255, 255, 0.06));
const CANCEL_RIM: Ink = Ink::Dark(rgba(255, 255, 255, 0.12));
const ON_ACCENT: Ink = Ink::Fixed(rgb(0x0B1020));

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    Focus(Focus),
    /// A repository's row: it is chosen.
    Pick(usize),
    /// "Choisir un autre dossier…".
    Other,
    Cancel,
    Create,
    /// What the name's field took of an input method or the pointer.
    TookName(Took),
    TookPath(Took),
    Key(Pressed),
    Stale,
}

/// A control's id: what tests and screen readers know it by.
fn id_of(what: &str) -> String {
    format!("project:{what}")
}

/// The title's line, cut into the lines it takes.
fn body_lines(words: Words) -> Vec<String> {
    kit::wrap(words.form_line(), INNER, |t| kit::measure(t, BODY, MANROPE))
}

/// The lines what is wrong takes.
fn fault_lines(form: &Form, words: Words, of_name: bool) -> Vec<String> {
    form.fault
        .as_ref()
        .filter(|f| f.of_name() == of_name)
        .map(|f| kit::wrap(&words.fault(f), INNER, |t| kit::measure(t, ERROR, MANROPE)))
        .unwrap_or_default()
}

fn fault_height(lines: usize) -> f32 {
    if lines == 0 {
        0.0
    } else {
        ERROR_GAP + lines as f32 * ERROR * LINE
    }
}

/// Its size, logical pixels, for what it shows now.
pub fn size(form: &Form, words: Words) -> (u32, u32) {
    let body = body_lines(words).len() as f32 * BODY * BODY_LINE;
    let head = TITLE * LINE + TITLE_GAP + body;
    let name =
        LABEL * LINE + LABEL_GAP + FIELD + fault_height(fault_lines(form, words, true).len());
    let rows = form.shown().len();
    let list = if rows == 0 {
        OTHER
    } else {
        rows as f32 * ROW + (rows - 1) as f32 * ROW_GAP
    };
    let other = if form.other == Other::Field {
        FIELD
    } else {
        OTHER
    };
    let folder = LABEL * LINE
        + LABEL_GAP
        + list
        + ROW_GAP
        + other
        + fault_height(fault_lines(form, words, false).len());
    let height = 2.0 * (BORDER + PAD) + head + GAP + name + GAP + folder + GAP + BUTTON;
    (WIDTH as u32, height.ceil() as u32)
}

/// Its size again for what it shows now: a resize if it changed.
pub fn refit(form: &mut Form) -> Vec<Effect> {
    let size = size(form, super::words());
    if size == form.size {
        return super::woken();
    }
    form.size = size;
    let place = super::place(form.ask.at, size, form.ask.screen);
    let margin = (place != form.place).then_some((place.1, 0, 0, place.0));
    form.place = place;
    let mut effects = vec![Effect::Resize {
        id: form.surface,
        size,
        margin,
    }];
    effects.extend(super::woken());
    effects
}

/// The form on its surface `id`, on the glass (else opaque).
pub fn view<'a>(
    projects: &'a Lent<Projects>,
    id: window::Id,
    words: Words,
    glass: bool,
) -> Element<'a, crate::Message> {
    Around::new(
        id,
        build(projects, id, words),
        move |m| widget(projects, id, m),
        Msg::Stale,
    )
    .own(hocus_link::PROJECT_FORM_NAMESPACE, move || {
        projects.read(|p| p.form_on(id).is_some()).unwrap_or(false)
    })
    .rebuild(move || build(projects, id, words))
    .keys(move |pressed| Some(Msg::Key(pressed.clone())))
    .on(iced::widget::canvas(super::card::Tint {
        radius: RADIUS,
        top: if glass { GLASS_TOP } else { OPAQUE_TOP },
        bottom: if glass { GLASS_BOTTOM } else { OPAQUE_BOTTOM },
        rim: RIM,
    })
    .width(Length::Fill)
    .height(Length::Fill))
}

/// A widget's message, handled at once.
fn widget(projects: &Lent<Projects>, id: window::Id, message: Msg) {
    let now = std::time::Instant::now();
    let done = projects.change(|p| {
        let effects = handle(p, id, message, now);
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

/// What `message` does to the form on `id`.
pub fn handle(
    p: &mut Projects,
    id: window::Id,
    message: Msg,
    now: std::time::Instant,
) -> Vec<Effect> {
    let Some(form) = p.form_mut(id) else {
        return Vec::new();
    };
    match message {
        Msg::Stale => Vec::new(),
        Msg::Focus(focus) => {
            form.focus = focus;
            Vec::new()
        }
        Msg::Pick(at) => {
            pick(form, at);
            refit(form)
        }
        Msg::Other => other(form),
        Msg::Cancel => {
            let mut effects = Vec::new();
            p.close(now, &mut effects);
            effects
        }
        Msg::Create => create(form),
        Msg::TookName(took) => {
            form.focus = Focus::Name;
            took_by(form, took, false)
        }
        Msg::TookPath(took) => {
            form.focus = Focus::Path;
            took_by(form, took, true)
        }
        Msg::Key(pressed) => key(p, id, &pressed, now),
    }
}

/// What a field took: an edit follows, a paste is asked for.
fn took_by(form: &mut Form, took: Took, path: bool) -> Vec<Effect> {
    match took {
        Took::Edited => {
            if path {
                form.folder = Folder::Typed;
            } else {
                form.named = true;
            }
            edited(form);
            refit(form)
        }
        Took::Paste => {
            super::paste();
            Vec::new()
        }
        Took::No | Took::Same | Took::Moved => Vec::new(),
    }
}

/// A field was edited: what was wrong with it goes, the name follows a
/// typed folder.
pub fn edited(form: &mut Form) {
    match form.focus {
        Focus::Path => {
            form.folder = Folder::Typed;
            if form.fault.as_ref().is_some_and(|f| !f.of_name()) {
                form.fault = None;
            }
            form.follow();
        }
        _ => {
            form.named = !form.name.is_empty();
            if form.fault.as_ref().is_some_and(Fault::of_name) {
                form.fault = None;
            }
        }
    }
}

/// Repository `at` is chosen: the name follows it.
fn pick(form: &mut Form, at: usize) {
    if at >= form.shown().len() {
        return;
    }
    form.folder = Folder::Repo(at);
    form.focus = Focus::Repo(at);
    if form.fault.as_ref().is_some_and(|f| !f.of_name()) {
        form.fault = None;
    }
    if form.other == Other::Field {
        form.other = Other::Button;
    }
    form.follow();
}

/// "Choisir un autre dossier…": the file chooser asked.
fn other(form: &mut Form) -> Vec<Effect> {
    if form.other != Other::Button {
        return Vec::new();
    }
    form.other = Other::Asking;
    form.focus = Focus::Other;
    super::choose_folder(form.serial);
    super::woken()
}

/// "Créer le projet": the compositor asked, unless the form sees already
/// what will not do.
fn create(form: &mut Form) -> Vec<Effect> {
    if form.waiting {
        return Vec::new();
    }
    let name = form.name.text().trim().to_owned();
    let fault = match (form.folder(), hocus_link::projects::name_fault(&name)) {
        (_, Some(fault)) => Some(Fault::Refused(hocus_link::ProjectRefusal::Name(fault))),
        (None, None) => Some(Fault::NoFolder),
        (Some(_), None) => {
            let key = hocus_link::projects::name_key(&name);
            form.ask
                .taken
                .iter()
                .find(|t| hocus_link::projects::name_key(t) == key)
                .map(|t| Fault::Refused(hocus_link::ProjectRefusal::NameTaken(t.clone())))
        }
    };
    if let Some(fault) = fault {
        form.fault = Some(fault);
        return refit(form);
    }
    let Some(folder) = form.folder() else {
        return Vec::new();
    };
    tracing::debug!(
        desktop = form.ask.desktop + 1,
        "the form asks for a project"
    );
    form.waiting = true;
    form.fault = None;
    let mut effects = vec![Effect::Tell(hocus_link::ToCompositor::ProjectMake {
        output: form.ask.output.clone(),
        desktop: form.ask.desktop,
        name,
        folder: folder.display().to_string(),
    })];
    effects.extend(refit(form));
    effects
}

/// A key on the form on `id`: `Esc` cancels, `Enter` creates (or does
/// what holds the keyboard: "Annuler", the chooser), `Tab` goes round, the
/// arrows go through the repositories, the rest types in the field holding
/// the keyboard.
pub fn key(
    p: &mut Projects,
    id: window::Id,
    pressed: &Pressed,
    now: std::time::Instant,
) -> Vec<Effect> {
    use keyboard::{Key, key::Named};
    let Some(form) = p.form_mut(id) else {
        return Vec::new();
    };
    let back = pressed.modifiers.shift();
    match &pressed.key {
        Key::Named(Named::Escape) => {
            let mut effects = Vec::new();
            p.close(now, &mut effects);
            effects
        }
        Key::Named(Named::Enter) => match form.focus {
            Focus::Cancel => {
                let mut effects = Vec::new();
                p.close(now, &mut effects);
                effects
            }
            Focus::Other => other(form),
            _ => create(form),
        },
        Key::Named(Named::Tab) => {
            let ring = form.ring();
            let at = ring.iter().position(|f| *f == form.focus).unwrap_or(0);
            let n = ring.len();
            let next = if back { (at + n - 1) % n } else { (at + 1) % n };
            form.focus = ring[next];
            super::woken()
        }
        Key::Named(Named::ArrowDown | Named::ArrowUp)
            if matches!(form.focus, Focus::Repo(_)) || form.focus == Focus::Name =>
        {
            let down = matches!(&pressed.key, Key::Named(Named::ArrowDown));
            let rows = form.shown().len();
            if rows == 0 {
                return Vec::new();
            }
            let at = match (form.focus, &form.folder) {
                (Focus::Repo(i), _) => i,
                (_, Folder::Repo(i)) => *i,
                _ => 0,
            };
            let to = if down {
                (at + 1).min(rows - 1)
            } else {
                at.saturating_sub(1)
            };
            pick(form, to);
            refit(form)
        }
        Key::Named(Named::Space) if !matches!(form.focus, Focus::Name | Focus::Path) => {
            match form.focus {
                Focus::Repo(i) => {
                    pick(form, i);
                    refit(form)
                }
                Focus::Other => other(form),
                Focus::Cancel => {
                    let mut effects = Vec::new();
                    p.close(now, &mut effects);
                    effects
                }
                Focus::Create => create(form),
                Focus::Name | Focus::Path => Vec::new(),
            }
        }
        _ => {
            let path = form.focus == Focus::Path;
            if !path && form.focus != Focus::Name {
                return Vec::new();
            }
            let entry = if path { &mut form.path } else { &mut form.name };
            let took = entry.key(&pressed.key, pressed.modifiers, pressed.text.as_deref());
            took_by(form, took, path)
        }
    }
}

/// Its content, as the form is now; said in the log when it changed.
fn build<'a>(projects: &'a Lent<Projects>, id: window::Id, words: Words) -> El<'a> {
    projects
        .change(|p| {
            let form = p.form_mut(id)?;
            let now = shows(form, words);
            if now != form.logged {
                tracing::debug!(said = %now, "the project's form shows it");
                form.logged = now;
            }
            Some(content(projects, form, words))
        })
        .flatten()
        .unwrap_or_else(|| Space::new().into())
}

/// What it shows, in words, for the log and the tests.
pub fn shows(form: &Form, words: Words) -> String {
    let repos: Vec<String> = form
        .shown()
        .iter()
        .map(|r| r.path.display().to_string())
        .collect();
    format!(
        "desktop={} name={:?} folder={:?} repos={repos:?} focus={:?} other={:?} fault={:?} waiting={}",
        form.ask.desktop + 1,
        form.name.text(),
        form.folder().map(|f| f.display().to_string()),
        form.focus,
        form.other,
        form.fault.as_ref().map(|f| words.fault(f)),
        form.waiting,
    )
}

fn content<'a>(projects: &'a Lent<Projects>, form: &Form, words: Words) -> El<'a> {
    let mut all = column![];
    // The title and its line.
    let title = words.form_title(usize::from(form.ask.desktop) + 1);
    all = all.push(ui::line(
        container(said(title.clone(), TITLE, MANROPE_BOLD, TEXT.get())).height(TITLE * LINE),
        title,
    ));
    all = all.push(Space::new().height(TITLE_GAP));
    for line in body_lines(words) {
        all = all.push(
            container(said(line, BODY, MANROPE, FAINT.get()))
                .height(BODY * BODY_LINE)
                .align_y(Vertical::Center),
        );
    }
    all = all.push(Space::new().height(GAP));
    // The name.
    all = all.push(label(words.name()));
    all = all.push(Space::new().height(LABEL_GAP));
    let refused = form.fault.as_ref().is_some_and(Fault::of_name);
    all = all.push(field(projects, form, false, words, refused));
    all = faults(all, form, words, true);
    all = all.push(Space::new().height(GAP));
    // The folders.
    all = all.push(label(words.folder_heading()));
    all = all.push(Space::new().height(LABEL_GAP));
    let shown = form.shown();
    if shown.is_empty() {
        let none = if form.repos.is_some() {
            words.no_repository()
        } else {
            ""
        };
        all = all.push(ui::line(
            container(said(none.to_owned(), OTHER_TEXT, MANROPE, FAINT.get()))
                .height(OTHER)
                .padding(Padding {
                    left: ROW_PAD,
                    ..Padding::ZERO
                })
                .align_y(Vertical::Center),
            none,
        ));
    }
    let mut rows = column![].spacing(ROW_GAP);
    for (i, repo) in shown.iter().enumerate() {
        rows = rows.push(repo_row(form, words, i, repo));
    }
    all = all.push(rows);
    all = all.push(Space::new().height(ROW_GAP));
    all = all.push(if form.other == Other::Field {
        let refused = form.fault.as_ref().is_some_and(|f| !f.of_name());
        field(projects, form, true, words, refused)
    } else {
        other_row(form, words)
    });
    all = faults(all, form, words, false);
    all = all.push(Space::new().height(GAP));
    all = all.push(buttons(form, words));
    container(all)
        .padding(Padding {
            top: BORDER + PAD,
            right: BORDER + PAD,
            bottom: 0.0,
            left: BORDER + PAD,
        })
        .into()
}

/// What is wrong with the name (`of_name`) or the folder, under it.
fn faults<'a>(
    mut all: iced::widget::Column<'a, Msg>,
    form: &Form,
    words: Words,
    of_name: bool,
) -> iced::widget::Column<'a, Msg> {
    let lines = fault_lines(form, words, of_name);
    if lines.is_empty() {
        return all;
    }
    all = all.push(Space::new().height(ERROR_GAP));
    let whole = lines.join(" ");
    let mut text = column![];
    for line in lines {
        text = text.push(container(said(line, ERROR, MANROPE, PEACH.get())).height(ERROR * LINE));
    }
    all.push(ui::line(text, whole))
}

/// A part's label: 11 px, 700, capitals spaced 0.08 em.
fn label<'a>(word: &str) -> El<'a> {
    ui::line(
        container(spaced_by(
            &word.to_uppercase(),
            LABEL,
            MANROPE_BOLD,
            FAINT.get(),
            0.08,
        ))
        .height(LABEL * LINE)
        .align_y(Vertical::Center),
        word,
    )
    .into()
}

/// The name's field, or the path's (`path`).
fn field<'a>(
    projects: &'a Lent<Projects>,
    form: &Form,
    path: bool,
    words: Words,
    refused: bool,
) -> El<'a> {
    let focus = if path { Focus::Path } else { Focus::Name };
    let focused = form.focus == focus;
    let typed = Field {
        with: Box::new(move |with: &mut dyn FnMut(&mut crate::field::Entry)| {
            projects.change(|p| {
                if let Some(form) = &mut p.form {
                    if path {
                        with(&mut form.path);
                    } else {
                        with(&mut form.name);
                    }
                }
            });
        }),
        style: Style::one(TYPED, MANROPE),
        placeholder: if path {
            words.folder_path().to_owned()
        } else {
            String::new()
        },
        look: Look::of(TEXT.get(), QUIET.get()),
        focused,
        left: FIELD_PAD,
        right: FIELD_PAD,
        on_focus: Msg::Focus(focus),
        on_took: if path { Msg::TookPath } else { Msg::TookName },
    }
    .view(FIELD);
    let rim = if refused {
        FIELD_REFUSED.get()
    } else if focused {
        FIELD_RIM.get()
    } else {
        FIELD_RIM_OFF.get()
    };
    let boxed = container(typed)
        .width(INNER)
        .height(FIELD)
        .style(move |_| container::Style {
            background: Some(FIELD_FILL.get().into()),
            border: iced::Border {
                color: rim,
                width: 1.0,
                radius: FIELD_RADIUS.into(),
            },
            ..container::Style::default()
        });
    let (id, name, value) = if path {
        ("Path", words.folder_path(), form.path.text())
    } else {
        ("Name", words.name(), form.name.text())
    };
    control(boxed, id_of(id), Role::TextField, name)
        .node(|n| n.value(value.to_owned()))
        .typing()
        .into()
}

/// A repository's row: its folder, name, path and when; the chosen one in
/// the accent's tint.
fn repo_row<'a>(form: &Form, words: Words, i: usize, repo: &super::repos::Repo) -> El<'a> {
    let chosen = form.folder == Folder::Repo(i);
    let name = repo.name();
    let path = crate::desktops::folder_shown(&repo.path.display().to_string());
    let note = if repo.terminals {
        words.terminals_there().to_owned()
    } else {
        repo.when.map(|w| words.when(w)).unwrap_or_default()
    };
    let note_w = kit::measure(&note, NOTE, MANROPE);
    let room = INNER - 2.0 * ROW_PAD - ICON - ICON_GAP - note_w - 8.0;
    let name_shown = kit::elide(&name, room, |t| kit::measure(t, REPO, MANROPE_SEMIBOLD));
    let path_shown = kit::elide_start(&path, room, |t| kit::measure(t, PATH, MARTIAN));
    let icon = drawn(ICON, ICON, |f, s| {
        folder(f, Rectangle::new(Point::ORIGIN, s), ICON_INK.get(), 1.7);
    });
    let texts = column![
        said(name_shown, REPO, MANROPE_SEMIBOLD, TEXT.get()),
        said(path_shown, PATH, MARTIAN, FAINT.get()),
    ]
    .spacing(1.0);
    let line = row![
        icon,
        gap(ICON_GAP),
        texts,
        iced::widget::space::horizontal(),
        said(note.clone(), NOTE, MANROPE, FAINT.get()),
    ]
    .align_y(Vertical::Center);
    let pressed = button(container(line).center_y(Length::Fill))
        .padding(Padding {
            left: ROW_PAD,
            right: ROW_PAD,
            ..Padding::ZERO
        })
        .width(INNER)
        .height(ROW)
        .style(move |_, status| {
            if chosen {
                button::Style {
                    background: Some(CHOSEN_FILL.get().into()),
                    border: iced::Border {
                        color: CHOSEN_RIM.get(),
                        width: 1.0,
                        radius: ROW_RADIUS.into(),
                    },
                    ..style::clear()
                }
            } else if style::lit(status) {
                style::hovered(HOVER.get(), ROW_RADIUS)
            } else {
                style::clear()
            }
        })
        .on_press(Msg::Pick(i));
    control(
        pressed,
        id_of(&format!("Repo({i})")),
        Role::RadioButton,
        format!("{name}, {path}"),
    )
    .node(|n| n.checked(chosen))
    .focus(
        form.focus == Focus::Repo(i),
        Msg::Focus(Focus::Repo(i)),
        ROW_RADIUS,
    )
    .activate(Msg::Pick(i))
    .into()
}

/// "Choisir un autre dossier…", or the folder the chooser gave.
fn other_row<'a>(form: &Form, words: Words) -> El<'a> {
    let chosen = matches!(form.folder, Folder::Chosen(_));
    let text = match &form.folder {
        Folder::Chosen(path) => crate::desktops::folder_shown(&path.display().to_string()),
        _ => words.other_folder().to_owned(),
    };
    let dots = drawn(ICON, ICON, |f, s| {
        let y = s.height / 2.0;
        for x in [5.0, 12.0, 19.0] {
            f.stroke(
                &Path::circle(Point::new(x * s.width / 24.0, y), 1.0),
                Stroke::default().with_color(ICON_INK.get()).with_width(1.7),
            );
        }
    });
    let icon: El<'a> = if chosen {
        drawn(ICON, ICON, |f, s| {
            folder(f, Rectangle::new(Point::ORIGIN, s), ICON_INK.get(), 1.7);
        })
    } else {
        dots
    };
    let line = row![
        icon,
        gap(ICON_GAP),
        said(
            text.clone(),
            OTHER_TEXT,
            MANROPE,
            if chosen { TEXT.get() } else { ICON_INK.get() }
        ),
    ]
    .align_y(Vertical::Center);
    let pressed = button(container(line).center_y(Length::Fill))
        .padding(Padding {
            left: ROW_PAD,
            ..Padding::ZERO
        })
        .width(INNER)
        .height(OTHER)
        .style(move |_, status| {
            if chosen {
                button::Style {
                    background: Some(CHOSEN_FILL.get().into()),
                    border: iced::Border {
                        color: CHOSEN_RIM.get(),
                        width: 1.0,
                        radius: ROW_RADIUS.into(),
                    },
                    ..style::clear()
                }
            } else if style::lit(status) {
                style::hovered(HOVER.get(), ROW_RADIUS)
            } else {
                style::clear()
            }
        })
        .on_press(Msg::Other);
    control(pressed, id_of("Other"), Role::Button, text)
        .focus(
            form.focus == Focus::Other,
            Msg::Focus(Focus::Other),
            ROW_RADIUS,
        )
        .activate(Msg::Other)
        .into()
}

/// "Annuler" and "Créer le projet", on the right.
fn buttons<'a>(form: &Form, words: Words) -> El<'a> {
    let face = |word: &str, size: f32, font, ink: Color| {
        container(said(word.to_owned(), size, font, ink))
            .center_y(Length::Fill)
            .padding(Padding {
                left: BUTTON_PAD,
                right: BUTTON_PAD,
                ..Padding::ZERO
            })
    };
    let cancel = button(face(
        words.cancel(),
        BUTTON_TEXT,
        MANROPE_SEMIBOLD,
        TEXT.get(),
    ))
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
            radius: FIELD_RADIUS.into(),
        },
        ..style::clear()
    })
    .on_press(Msg::Cancel);
    let cancel = control(cancel, id_of("Cancel"), Role::Button, words.cancel())
        .focus(
            form.focus == Focus::Cancel,
            Msg::Focus(Focus::Cancel),
            FIELD_RADIUS,
        )
        .activate(Msg::Cancel);
    let waiting = form.waiting;
    let create = button(face(
        words.create(),
        BUTTON_TEXT,
        MANROPE_BOLD,
        ON_ACCENT.get(),
    ))
    .padding(0)
    .height(BUTTON)
    .style(move |_, status| {
        let a = if waiting {
            0.6
        } else if style::lit(status) {
            0.9
        } else {
            1.0
        };
        style::filled(
            crate::theme::ACCENT.get().scale_alpha(a),
            FIELD_RADIUS,
            ON_ACCENT.get(),
        )
    })
    .on_press(Msg::Create);
    let create = control(create, id_of("Create"), Role::Button, words.create())
        .focus(
            form.focus == Focus::Create,
            Msg::Focus(Focus::Create),
            FIELD_RADIUS,
        )
        .activate(Msg::Create);
    row![
        iced::widget::space::horizontal(),
        cancel,
        gap(BUTTON_GAP),
        create
    ]
    .width(INNER)
    .height(BUTTON)
    .into()
}

/// A folder (the mockup's 24 px path) in `rect`, stroked `width` wide on
/// its scale.
pub fn folder(f: &mut iced::widget::canvas::Frame, rect: Rectangle, ink: Color, width: f32) {
    let side = rect.width.min(rect.height);
    crate::desktops::draw::folder_icon_stroked(
        f,
        Point::new(rect.x, rect.y),
        side,
        ink,
        width * side / 24.0,
    );
}
