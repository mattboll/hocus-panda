//! The "La barre" page (6.4, variant A): the rows of the model's page
//! with their own lines, a row per screen for its form, the ↑ and ↓ of
//! the groups that move, and the preview beside them (the screens with
//! their bars, and the laptop's bar drawn as the file says it).
//!
//! The rows of one key each are the window's ([`crate::bar::settings`]);
//! a screen's form and the groups' order are keys the page writes itself,
//! with their toasts' words; "Revenir à la forme de sa largeur" asks the
//! compositor to forget the form chosen with a click (`bar-auto NAME` on
//! its line). The preview's clock ticks only while the page shows.

use std::time::{Duration, Instant};

use hocus_link::{BarGroup, Form};
use iced::{
    Alignment, Background, Color, Font, Length, Padding, Point, Rectangle, Renderer, Size, Theme,
    keyboard::key::Named,
    mouse,
    widget::{Space, button, canvas, column, container, pin, row},
};

use super::{
    App, Control, Doing, KeysSaid, Message, Page,
    style::*,
    view::{El, card_box, divided, icon, label, t},
};
use crate::{
    bar as model,
    lang::{Lang, w},
    model::{Icon, SectionId, Setting, Value},
    store::{KeyEdit, Slot},
};

/// The page's width at most: the rows, 20 px, the preview.
pub const PAGE_WIDTH: f32 = 788.0;
const PREVIEW: f32 = 214.0;
/// The laptop's bar in the preview: drawn at its logical sizes, 960 high,
/// then at 55 %.
const BAR_HIGH: f32 = 960.0;
const BAR_SCALE: f32 = 0.55;
/// The minutes' transition played once on the preview.
const PLAY: Duration = Duration::from_millis(700);

/// The page's state.
#[derive(Debug, Default)]
pub struct State {
    /// The preview's bar: the form it shows, and the switch under way
    /// (from that width, since then).
    shown: Option<Form>,
    switch: Option<(f64, Instant)>,
    /// The transition played on the preview's minutes, since then.
    played: Option<(&'static str, Instant)>,
    /// The preview's next tick is asked.
    ticking: bool,
    /// The ↑ or ↓ the keyboard follows after a move: its group, up, and
    /// the order it waits for the file to say.
    follow: Option<(BarGroup, bool, [BarGroup; 3])>,
}

/// The page's own messages.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A screen's form: its name, `"auto"`, `"full"` or `"mini"`.
    Form(String, &'static str),
    /// "Revenir à la forme de sa largeur".
    Auto(String),
    /// A group one place up (`true`) or down.
    Move(BarGroup, bool),
    /// A link to another page's row (or to a section by its slug).
    Link(&'static str),
    /// A group of the preview clicked: its row lights.
    Lit(&'static str),
    Tick,
}

/// A control of the page's own that takes the keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    Link(&'static str),
    Form(String),
    Auto(String),
    Move(BarGroup, bool),
}

/// A screen's row of the form.
#[derive(Debug, Clone, PartialEq)]
struct FormRow {
    name: String,
    title: String,
    short: String,
    plugged: bool,
    /// What the file asks: `"auto"`, `"full"` or `"mini"`.
    asked: &'static str,
    /// Its bar now, if it has one; the form chosen there with a click.
    bar: Option<&'static str>,
    chosen: Option<&'static str>,
    /// Logical pixels.
    width: i32,
    position: (i32, i32),
    height: i32,
    built_in: bool,
    primary: bool,
}

impl FormRow {
    /// The form it takes in automatic: the click's, else its width's.
    fn auto(&self) -> Form {
        match self.chosen {
            Some("mini") => Form::Mini,
            Some(_) => Form::Full,
            None => Form::default_for(self.width),
        }
    }

    /// The form its bar has, or would have.
    fn form(&self) -> Form {
        match (self.bar, self.asked) {
            (Some("mini"), _) => Form::Mini,
            (Some(_), _) => Form::Full,
            (None, "full") => Form::Full,
            (None, "mini") => Form::Mini,
            (None, _) => self.auto(),
        }
    }
}

fn form_word(form: Form, lang: Lang) -> &'static str {
    match (form, lang) {
        (Form::Full, Lang::French) => "pleine",
        (Form::Full, Lang::English) => "full",
        (Form::Mini, _) => "mini",
    }
}

impl App {
    fn bar_file(&self) -> model::File {
        self.snapshot
            .as_ref()
            .map(|s| s.bar.as_ref().clone())
            .unwrap_or_default()
    }

    fn bar_slot(&self, id: &str) -> Option<Slot> {
        crate::model::setting(id).map(|s| Slot::of(s, self.source))
    }

    fn bar_bool(&self, id: &str) -> bool {
        self.bar_slot(id).and_then(|s| self.value(s)) != Some(Value::Bool(false))
    }

    pub(super) fn bar_shows(&self) -> bool {
        self.page == Page::Section(SectionId::Bar)
    }

    /// The screens with a row of their own: those the compositor knows,
    /// then those the file gives a form, plugged first.
    fn form_rows(&self) -> Vec<FormRow> {
        let lang = self.lang;
        let file = self.bar_file();
        let mut rows: Vec<FormRow> = Vec::new();
        if let Some(known) = self.screens_list() {
            for s in &known.list {
                if !s.connected && s.bar_chosen.is_none() && file.named().all(|n| n != s.name) {
                    continue;
                }
                rows.push(FormRow {
                    name: s.name.clone(),
                    title: s.title(lang),
                    short: s.short(lang),
                    plugged: s.connected,
                    asked: file.form_of(&s.name),
                    bar: s.bar,
                    chosen: s.bar_chosen,
                    width: s.size.0,
                    position: s.position,
                    height: s.size.1,
                    built_in: s.built_in,
                    primary: s.primary,
                });
            }
        }
        for name in file.named() {
            if rows.iter().any(|r| r.name == name) {
                continue;
            }
            rows.push(FormRow {
                name: name.to_owned(),
                title: name.to_owned(),
                short: name.to_owned(),
                plugged: false,
                asked: file.form_of(name),
                bar: None,
                chosen: None,
                width: 0,
                position: (0, 0),
                height: 0,
                built_in: false,
                primary: false,
            });
        }
        rows.sort_by_key(|r| !r.plugged);
        rows
    }

    /// The laptop's screen in the preview: the built-in one plugged, else
    /// the primary, else the first with a bar.
    fn preview_screen(&self, rows: &[FormRow]) -> Option<FormRow> {
        rows.iter()
            .filter(|r| r.plugged)
            .find(|r| r.built_in)
            .or_else(|| rows.iter().find(|r| r.plugged && r.primary))
            .or_else(|| rows.iter().find(|r| r.bar.is_some()))
            .or_else(|| rows.iter().find(|r| r.plugged))
            .cloned()
    }

    pub(super) fn bar_controls(&self) -> Vec<Control> {
        let setting = |id: &str| self.bar_slot(id).map(Control::Setting);
        let mut out: Vec<Control> =
            vec![Control::Bar(Stop::Link("appearance.reduce-transparency"))];
        out.extend(setting(model::SCREENS));
        out.push(Control::Bar(Stop::Link(crate::screens::PRIMARY)));
        for r in self.form_rows().into_iter().filter(|r| r.plugged) {
            out.push(Control::Bar(Stop::Form(r.name.clone())));
            if r.asked == "auto" && r.chosen.is_some() {
                out.push(Control::Bar(Stop::Auto(r.name)));
            }
        }
        out.push(Control::Bar(Stop::Link("shortcuts.toggle-bar")));
        out.extend(setting(model::MEETING));
        for group in self.bar_file().order() {
            out.push(Control::Bar(Stop::Move(group, true)));
            out.push(Control::Bar(Stop::Move(group, false)));
            out.extend(setting(model::group_setting(group)));
        }
        for id in [
            model::WINDOWS,
            model::SECONDS,
            model::TRANSITION,
            model::FONT,
            model::COLOR,
        ] {
            out.extend(setting(id));
        }
        out
    }

    fn bar_focused(&self, stop: &Stop) -> bool {
        matches!(self.focus, super::Focus::Control(i)
            if self.controls().get(i) == Some(&Control::Bar(stop.clone())))
    }

    // --- What changes it --------------------------------------------------------------

    pub(super) fn bar_update(&mut self, msg: Msg) -> iced::Task<Message> {
        match msg {
            Msg::Form(name, form) => {
                self.bar_form(&name, form);
                iced::Task::none()
            }
            Msg::Auto(name) => {
                if self.writable() && self.screens_ask(format!("bar-auto {name}")) {
                    tracing::info!(
                        output = name,
                        "the bar's form chosen with a click, forgotten"
                    );
                }
                iced::Task::none()
            }
            Msg::Move(group, up) => {
                self.bar_move(group, up);
                iced::Task::none()
            }
            Msg::Link(target) => {
                if target == "agenda" {
                    self.go(Page::Section(SectionId::Agenda));
                    return iced::Task::none();
                }
                self.show_row(target)
            }
            Msg::Lit(id) => {
                self.scroll_to = Some(id);
                self.light = Some((id, Instant::now()));
                iced::Task::none()
            }
            Msg::Tick => {
                self.bar.ticking = false;
                iced::Task::none()
            }
        }
    }

    /// Screen `name`'s form written as `form`; the compositor forgets the
    /// one chosen there with a click as it reads it.
    fn bar_form(&mut self, name: &str, form: &'static str) {
        let rows = self.form_rows();
        let Some(row) = rows.iter().find(|r| r.name == name) else {
            return;
        };
        let file = self.bar_file();
        let own = file.outputs.iter().any(|(n, _)| n == name);
        if own && row.asked == form {
            return;
        }
        let lang = self.lang;
        let said = |f: &str| match (f, lang) {
            ("full", Lang::French) => "Pleine",
            ("full", Lang::English) => "Full",
            ("mini", _) => "Mini",
            _ => "Auto",
        };
        let whose = crate::screens::lower_first(&row.title);
        let (now, then) = (said(form), said(row.asked));
        let done = match lang {
            Lang::French => format!("Forme de la barre, {whose} : {now}"),
            Lang::English => format!("The bar’s form, {whose}: {now}"),
        };
        let said = KeysSaid {
            done,
            undone: match lang {
                Lang::French => format!("Annulé : la forme revient à {then}."),
                Lang::English => format!("Undone: the form is back to {then}."),
            },
            redone: match lang {
                Lang::French => format!("Rétabli : {now}."),
                Lang::English => format!("Redone: {now}."),
            },
            row: model::FORM_ROW,
        };
        self.ask_keys(
            vec![KeyEdit {
                key: vec!["outputs".into(), name.to_owned(), "panel-form".into()],
                value: Some(format!("\"{form}\"")),
            }],
            said,
            Doing::New,
            None,
        );
    }

    /// `group` one place up or down: `[panel] order` written whole.
    fn bar_move(&mut self, group: BarGroup, up: bool) {
        let before = self.bar_file().order();
        let Some(after) = model::moved(before, group, up) else {
            return;
        };
        let lang = self.lang;
        let now = model::say_order(after, lang);
        let then = model::say_order(before, lang);
        let said = KeysSaid {
            done: match lang {
                Lang::French => format!("Ordre de la barre : {now}"),
                Lang::English => format!("The bar’s order: {now}"),
            },
            undone: match lang {
                Lang::French => format!("Annulé : l’ordre revient à {then}."),
                Lang::English => format!("Undone: the order is back to {then}."),
            },
            redone: match lang {
                Lang::French => format!("Rétabli : {now}."),
                Lang::English => format!("Redone: {now}."),
            },
            row: model::group_setting(group),
        };
        let list = after
            .iter()
            .map(|g| format!("\"{}\"", g.name()))
            .collect::<Vec<_>>()
            .join(", ");
        self.bar.follow = Some((group, up, after));
        self.ask_keys(
            vec![KeyEdit {
                key: vec!["panel".into(), "order".into()],
                value: Some(format!("[{list}]")),
            }],
            said,
            Doing::New,
            None,
        );
    }

    /// After each message: the keyboard follows a moved group's button,
    /// the preview's form switch and transition start, its clock's next
    /// tick is asked while the page shows.
    pub(super) fn bar_after(&mut self) -> iced::Task<Message> {
        if !self.bar_shows() {
            self.bar.follow = None;
            return iced::Task::none();
        }
        // The keyboard on the button that moved with its row; at an end,
        // on the other one.
        if let Some((group, up, wanted)) = self.bar.follow
            && self.bar_file().order() == wanted
        {
            self.bar.follow = None;
            let order = wanted;
            let at = order.iter().position(|g| *g == group).unwrap_or(0);
            let up = if up && at == 0 {
                false
            } else if !up && at == order.len() - 1 {
                true
            } else {
                up
            };
            let controls = self.controls();
            if let Some(i) = controls
                .iter()
                .position(|c| *c == Control::Bar(Stop::Move(group, up)))
            {
                self.focus = super::Focus::Control(i);
            }
        }
        let rows = self.form_rows();
        if let Some(screen) = self.preview_screen(&rows) {
            let form = screen.form();
            match self.bar.shown {
                Some(before) if before != form => {
                    let from = self.preview_width();
                    self.bar.switch = (!self.reduced_motion).then_some((from, Instant::now()));
                    self.bar.shown = Some(form);
                }
                None => self.bar.shown = Some(form),
                _ => {}
            }
        }
        if self.bar.ticking {
            return iced::Task::none();
        }
        self.bar.ticking = true;
        let now = jiff::Zoned::now();
        let into = Duration::from_nanos(u64::try_from(now.subsec_nanosecond()).unwrap_or(0));
        let seconds = self.bar_bool(model::SECONDS);
        let wait = if seconds {
            Duration::from_secs(1).saturating_sub(into)
        } else {
            let second = u64::try_from(now.second()).unwrap_or(0);
            Duration::from_secs(60 - second).saturating_sub(into)
        };
        iced::Task::perform(tokio::time::sleep(wait + Duration::from_millis(5)), |()| {
            Message::Bar(Msg::Tick)
        })
    }

    /// A transition chosen: played once on the preview's minutes.
    pub(super) fn bar_chose(&mut self, slot: Slot, value: &Value) {
        if slot.setting == model::TRANSITION
            && let Value::Word(word) = value
        {
            self.bar.played = Some((word, Instant::now()));
        }
    }

    pub(super) fn bar_moving(&self, now: Instant) -> bool {
        self.bar_shows()
            && (self.bar.switch.is_some_and(|(_, since)| {
                now.saturating_duration_since(since) < hocus_link::SWITCH
            }) || self
                .bar
                .played
                .is_some_and(|(_, since)| now.saturating_duration_since(since) < PLAY))
    }

    /// The preview's bar's width now.
    fn preview_width(&self) -> f64 {
        let form = self.bar.shown.unwrap_or(Form::Full);
        match self.bar.switch {
            Some((from, since)) => {
                hocus_link::width_at(from, form, self.now.saturating_duration_since(since))
            }
            None => f64::from(form.width()),
        }
    }

    pub(super) fn bar_key(&mut self, stop: &Stop, named: Named) -> iced::Task<Message> {
        match (stop, named) {
            (Stop::Link(target), Named::Space | Named::Enter) => self.bar_update(Msg::Link(target)),
            (Stop::Auto(name), Named::Space | Named::Enter) => {
                self.bar_update(Msg::Auto(name.clone()))
            }
            (Stop::Move(group, up), Named::Space | Named::Enter) => {
                self.bar_update(Msg::Move(*group, *up))
            }
            (Stop::Form(name), Named::ArrowLeft | Named::ArrowRight) => {
                let words = ["auto", "full", "mini"];
                let now = self.bar_file().form_of(name);
                let at = words.iter().position(|w| *w == now).unwrap_or(0);
                let to = if named == Named::ArrowRight {
                    (at + 1).min(2)
                } else {
                    at.saturating_sub(1)
                };
                if to == at {
                    return iced::Task::none();
                }
                self.bar_update(Msg::Form(name.clone(), words[to]))
            }
            _ => iced::Task::none(),
        }
    }

    /// `Alt+↑` / `Alt+↓` on a movable group's switch moves it.
    pub(super) fn bar_alt_move(&mut self, slot: Slot, named: Named) -> Option<iced::Task<Message>> {
        let group = BarGroup::DEFAULT
            .into_iter()
            .find(|g| model::group_setting(*g) == slot.setting)?;
        let up = match named {
            Named::ArrowUp => true,
            Named::ArrowDown => false,
            _ => return None,
        };
        self.bar_move(group, up);
        self.bar.follow = None;
        Some(iced::Task::none())
    }

    /// A light asked for the order lights Today's row, where the order
    /// starts.
    pub(super) fn bar_lit_alias(id: &str) -> &str {
        if id == model::ORDER { model::TODAY } else { id }
    }

    // --- What it says -----------------------------------------------------------------

    /// The line under a row of the model's, when the page says its own.
    pub(super) fn bar_line(&self, id: &str) -> Option<String> {
        let lang = self.lang;
        match id {
            model::SCREENS => {
                let known = self.screens_list()?;
                let plugged: Vec<_> = known.plugged().collect();
                let all = self.bar_slot(model::SCREENS).and_then(|s| self.value(s))
                    == Some(Value::Word("all"));
                let primary = plugged
                    .iter()
                    .find(|s| s.primary)
                    .or_else(|| plugged.first())
                    .map(|s| crate::screens::lower_first(&s.title(lang)))?;
                Some(match (plugged.len(), all, lang) {
                    (1, _, Lang::French) => {
                        format!("Un seul écran branché : la barre est sur l’{primary}.")
                    }
                    (1, _, Lang::English) => {
                        format!("One screen plugged in: the bar is on the {primary}.")
                    }
                    (_, true, Lang::French) => "Une barre par écran, chacune avec les bureaux et les fenêtres de son écran.".to_owned(),
                    (_, true, Lang::English) => "A bar per screen, each with its screen’s desktops and windows.".to_owned(),
                    (_, false, Lang::French) => format!(
                        "Aujourd’hui sur l’{primary} ; elle passe sur l’autre s’il s’éteint."
                    ),
                    (_, false, Lang::English) => format!(
                        "On the {primary} now; it moves to the other if that one goes off."
                    ),
                })
            }
            model::COLOR => {
                let colour = self.clock_colour_word();
                let name = model::COLOURS
                    .iter()
                    .find(|(hex, _)| *hex == colour)
                    .map(|(_, name)| name.get(lang));
                Some(match name {
                    Some(_) => w("Les heures et les minutes.", "The hours and minutes.")
                        .get(lang)
                        .to_owned(),
                    None => match lang {
                        Lang::French => format!(
                            "Une couleur du fichier, {} ; elle reste jusqu’à ce qu’une autre soit choisie.",
                            colour.to_uppercase()
                        ),
                        Lang::English => format!(
                            "A colour from the file, {}; it stays until another is chosen.",
                            colour.to_uppercase()
                        ),
                    },
                })
            }
            _ => None,
        }
    }

    /// The clock's colour, `#rrggbb`.
    fn clock_colour_word(&self) -> String {
        match self.bar_slot(model::COLOR).and_then(|s| self.value(s)) {
            Some(Value::Text(hex)) => hex,
            _ => model::COLOURS[0].0.to_owned(),
        }
    }

    /// The colour menu's button: "Personnalisée · #3DFFA2" for a colour of
    /// the file that is none of the four.
    pub(super) fn bar_menu_said(&self, setting: &Setting, value: &Value) -> Option<String> {
        if setting.id != model::COLOR {
            return None;
        }
        let Value::Text(hex) = value else {
            return None;
        };
        if model::COLOURS.iter().any(|(h, _)| h == hex) {
            return None;
        }
        Some(format!(
            "{} · {}",
            w("Personnalisée", "Custom").get(self.lang),
            hex.to_uppercase()
        ))
    }

    /// Under a row of the model's: a link (the primary screen's), or the
    /// amber line of a group with nothing to show.
    pub(super) fn bar_row_extra(&self, setting: &Setting, ink: f32) -> Option<El<'_>> {
        let lang = self.lang;
        match setting.id {
            model::SCREENS => Some(
                self.bar_link(
                    crate::screens::PRIMARY,
                    w(
                        "L’écran principal se choisit dans Écrans ›",
                        "The primary screen is chosen in Screens ›",
                    )
                    .get(lang),
                    ink,
                ),
            ),
            model::MEETING | model::TODAY | model::TASKS => {
                let file = self.bar_file();
                let none = if setting.id == model::TASKS {
                    !file.tasks
                } else {
                    !file.calendars
                };
                if !none {
                    return None;
                }
                Some(
                    row![
                        t(
                            w(
                                "Rien à montrer : aucun agenda ni tableau de tâches branché.",
                                "Nothing to show: no calendar or task board connected.",
                            )
                            .get(lang),
                            12.5,
                            fade(PEACH.get(), ink),
                        ),
                        self.bar_link(
                            "agenda",
                            w("Agenda et tâches ›", "Calendar and tasks ›").get(lang),
                            ink
                        ),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .wrap()
                    .into(),
                )
            }
            _ => None,
        }
    }

    /// A link in the accent's text colour.
    fn bar_link<'a>(&self, target: &'static str, said: &str, ink: f32) -> El<'a> {
        let focused = self.bar_focused(&Stop::Link(target));
        container(
            button(t(said.to_owned(), 12.5, fade(MINT.get(), ink)).font(SEMIBOLD))
                .padding([1, 2])
                .on_press(Message::Bar(Msg::Link(target)))
                .style(move |_, status| button::Style {
                    background: matches!(status, button::Status::Hovered)
                        .then(|| Background::Color(crate::theme::ink(rgba(255, 255, 255, 0.05)))),
                    text_color: TEXT.get(),
                    border: if focused {
                        ring(5.0)
                    } else {
                        border(5.0, 0.0, Color::TRANSPARENT)
                    },
                    ..button::Style::default()
                }),
        )
        .id(format!("link:{target}"))
        .into()
    }

    /// A movable group's switch, the ↑ and ↓ before it.
    pub(super) fn bar_arrows(&self, setting: &Setting) -> Option<El<'_>> {
        let group = BarGroup::DEFAULT
            .into_iter()
            .find(|g| model::group_setting(*g) == setting.id)?;
        let order = self.bar_file().order();
        let at = order.iter().position(|g| *g == group)?;
        let arrow = |up: bool| -> El<'_> {
            let end = if up { at == 0 } else { at == order.len() - 1 };
            let inert = end || !self.writable();
            let alpha = if inert { 0.3 } else { 1.0 };
            let focused = self.bar_focused(&Stop::Move(group, up));
            let glyph = icon(
                if up { Icon::ChevronUp } else { Icon::Chevron },
                13.0,
                fade(DIM.get(), alpha),
            );
            let mut b = button(container(glyph).center(26))
                .width(28)
                .height(28)
                .padding(0)
                .style(move |_, status| button::Style {
                    background: (matches!(status, button::Status::Hovered) && !inert)
                        .then(|| Background::Color(crate::theme::ink(rgba(255, 255, 255, 0.10)))),
                    text_color: TEXT.get(),
                    border: if focused {
                        ring(7.0)
                    } else {
                        border(
                            7.0,
                            1.0,
                            fade(crate::theme::ink(rgba(255, 255, 255, 0.12)), alpha),
                        )
                    },
                    ..button::Style::default()
                });
            if !inert {
                b = b.on_press(Message::Bar(Msg::Move(group, up)));
            }
            let id = format!("move:{}:{}", group.name(), if up { "up" } else { "down" });
            container(b).id(id).into()
        };
        Some(row![arrow(true), arrow(false)].spacing(4).into())
    }

    // --- What it draws ----------------------------------------------------------------

    pub(super) fn bar_page<'a>(&'a self, mut page: iced::widget::Column<'a, Message>) -> El<'a> {
        let lang = self.lang;
        page = page.push(
            row![
                icon(Icon::Appearance, 14.0, FAINT.get()),
                t(
                    w(
                        "La transparence de la barre et les mouvements réduits se règlent dans",
                        "The bar’s transparency and reduced motion are set in",
                    )
                    .get(lang),
                    13.0,
                    FAINT.get()
                ),
                self.bar_link(
                    "appearance.reduce-transparency",
                    w("Apparence et langue ›", "Appearance and language ›").get(lang),
                    1.0
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .wrap(),
        );
        let controls = self.controls();
        let model_row = |id: &str| -> El<'_> {
            match crate::model::setting(id) {
                Some(s) => self.row(s, &controls),
                None => Space::new().into(),
            }
        };
        let head = |words: &'static str, note: Option<String>| -> El<'_> {
            let mut head = row![label(words), Space::new().width(Length::Fill)]
                .align_y(Alignment::Center)
                .height(28);
            if let Some(note) = note {
                head = head.push(t(note, 12.0, FAINT.get()));
            }
            head.into()
        };
        let group = |head: El<'a>, rows: Vec<El<'a>>| -> El<'a> {
            let mut body = column![];
            for (i, r) in rows.into_iter().enumerate() {
                body = body.push(divided(i > 0, r));
            }
            column![head, card_box(body.into())].spacing(8).into()
        };
        let mut rows = column![].spacing(18).width(Length::Fill);
        // EMPLACEMENT.
        rows = rows.push(group(
            head(model::PLACE.label.get(lang), None),
            vec![model_row(model::SCREENS)],
        ));
        // FORME: a row per screen, then by hand.
        let mut forms: Vec<El<'_>> = self
            .form_rows()
            .into_iter()
            .enumerate()
            .map(|(i, r)| self.form_row(r, i == 0))
            .collect();
        forms.push(self.by_hand_row());
        rows = rows.push(group(
            head(
                model::FORM.label.get(lang),
                Some(
                    w("Pleine 196 px · mini 56 px", "Full 196 px · mini 56 px")
                        .get(lang)
                        .to_owned(),
                ),
            ),
            forms,
        ));
        // CE QU'ELLE MONTRE, in the bar's order.
        let mut shows: Vec<El<'_>> = vec![
            self.always_row(w("L’horloge et la date", "The clock and the date"), None),
            model_row(model::MEETING),
            self.always_row(
                w(
                    "Micro, caméra ou écran en cours",
                    "Microphone, camera or screen in use",
                ),
                Some(w(
                    "Le signe de ce qui t’écoute, te filme ou partage l’écran ne se masque pas.",
                    "The sign of what listens, films or shares the screen is never hidden.",
                )),
            ),
        ];
        for (i, g) in self.bar_file().order().into_iter().enumerate() {
            let r = model_row(model::group_setting(g));
            shows.push(if i == 0 {
                container(r).id(format!("row:{}", model::ORDER)).into()
            } else {
                r
            });
        }
        shows.push(model_row(model::WINDOWS));
        shows.push(self.always_row(
            w(
                "Bureaux, connexions, niveaux, boutons du bas",
                "Desktops, connections, levels, bottom buttons",
            ),
            None,
        ));
        rows = rows.push(group(
            head(
                model::SHOWS.label.get(lang),
                Some(
                    w(
                        "Un groupe vide ne prend pas de place",
                        "An empty group takes no room",
                    )
                    .get(lang)
                    .to_owned(),
                ),
            ),
            shows,
        ));
        // L'HORLOGE.
        rows = rows.push(group(
            head(model::CLOCK.label.get(lang), None),
            [model::SECONDS, model::TRANSITION, model::FONT, model::COLOR]
                .into_iter()
                .map(model_row)
                .collect(),
        ));
        page = page.push(
            row![
                rows,
                // The preview's place: it is drawn over the page there,
                // held at the page's top as it scrolls.
                container(Space::new().width(PREVIEW).height(self.preview_height()))
                    .id("bar:preview-slot".to_owned()),
            ]
            .spacing(20),
        );
        container(page).id("page:bar".to_owned()).into()
    }

    /// A row of what always shows: its name, its line, "Toujours".
    fn always_row<'a>(&self, name: crate::lang::Words, line: Option<crate::lang::Words>) -> El<'a> {
        let lang = self.lang;
        let mut words = column![t(name.get(lang).to_owned(), 14.0, TEXT.get()).font(SEMIBOLD)]
            .spacing(2)
            .width(Length::Fill);
        if let Some(line) = line {
            words = words.push(t(line.get(lang).to_owned(), 12.5, DIM.get()));
        }
        container(
            row![
                words,
                t(
                    w("Toujours", "Always").get(lang).to_owned(),
                    13.0,
                    FAINT.get()
                ),
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 9.0,
            right: 14.0,
            bottom: 9.0,
            left: 16.0,
        })
        .center_y(Length::Shrink)
        .height(Length::Shrink)
        .into()
    }

    /// The line under a screen's form: what applies now.
    fn form_line(&self, r: &FormRow) -> String {
        let lang = self.lang;
        if !r.plugged {
            return w(
                "Débranché ; sa forme est gardée pour son retour.",
                "Unplugged; its form is kept for its return.",
            )
            .get(lang)
            .to_owned();
        }
        let mut line = match (r.asked, r.chosen, lang) {
            ("auto", Some(c), Lang::French) => {
                format!(
                    "Automatique : {}, choisie d’un clic sur son bord.",
                    if c == "mini" { "mini" } else { "pleine" }
                )
            }
            ("auto", Some(c), Lang::English) => {
                format!("Automatic: {c}, chosen with a click on its edge.")
            }
            ("auto", None, _) => {
                let form = form_word(Form::default_for(r.width), lang);
                let from = hocus_link::FULL_FROM + Form::Full.width();
                match lang {
                    Lang::French => format!(
                        "Automatique : {form}, l’écran fait {} px de large (pleine dès {from}).",
                        r.width
                    ),
                    Lang::English => format!(
                        "Automatic: {form}, the screen is {} px wide (full from {from}).",
                        r.width
                    ),
                }
            }
            ("full", _, Lang::French) => {
                "Toujours pleine ; un clic sur son bord change pour la session.".to_owned()
            }
            ("full", _, Lang::English) => {
                "Always full; a click on its edge changes it for the session.".to_owned()
            }
            (_, _, Lang::French) => {
                "Toujours mini ; un clic sur son bord change pour la session.".to_owned()
            }
            (_, _, Lang::English) => {
                "Always mini; a click on its edge changes it for the session.".to_owned()
            }
        };
        let screens_all =
            self.bar_slot(model::SCREENS).and_then(|s| self.value(s)) == Some(Value::Word("all"));
        if r.bar.is_none() && !screens_all {
            line.push(' ');
            line.push_str(
                w(
                    "La barre n’y vient que si l’écran principal s’éteint.",
                    "The bar only comes here if the primary screen goes off.",
                )
                .get(lang),
            );
        }
        line
    }

    fn form_row(&self, r: FormRow, first: bool) -> El<'_> {
        let lang = self.lang;
        let inert = !r.plugged || !self.writable();
        let ink = if inert { INERT } else { 1.0 };
        let mut words = column![t(r.title.clone(), 14.0, fade(TEXT.get(), ink)).font(SEMIBOLD)]
            .spacing(2)
            .width(Length::Fill);
        words = words.push(t(self.form_line(&r), 12.5, fade(DIM.get(), ink)));
        if r.plugged && r.asked == "auto" && r.chosen.is_some() {
            let focused = self.bar_focused(&Stop::Auto(r.name.clone()));
            let mut back = button(
                t(
                    w(
                        "Revenir à la forme de sa largeur",
                        "Back to the form of its width",
                    )
                    .get(lang)
                    .to_owned(),
                    12.5,
                    MINT.get(),
                )
                .font(SEMIBOLD),
            )
            .padding([1, 2])
            .style(move |_, _| button::Style {
                border: if focused {
                    ring(5.0)
                } else {
                    border(5.0, 0.0, Color::TRANSPARENT)
                },
                ..button::Style::default()
            });
            if !inert {
                back = back.on_press(Message::Bar(Msg::Auto(r.name.clone())));
            }
            words = words.push(container(back).id(format!("auto:{}", r.name)));
        }
        let focused = self.bar_focused(&Stop::Form(r.name.clone()));
        let choices = [
            ("auto", w("Auto", "Auto")),
            ("full", w("Pleine", "Full")),
            ("mini", w("Mini", "Mini")),
        ];
        let mut segments = row![];
        for (word, said) in choices {
            let chosen = r.asked == word;
            let colour = if chosen { TEXT.get() } else { DIM.get() };
            let mut segment =
                button(t(said.get(lang).to_owned(), 12.5, fade(colour, ink)).font(SEMIBOLD))
                    .height(26)
                    .padding([4, 12])
                    .style(move |_, _| button::Style {
                        background: chosen.then(|| {
                            Background::Color(fade(
                                crate::theme::Ink::Token(|t| t.segment_chosen).get(),
                                ink,
                            ))
                        }),
                        text_color: TEXT.get(),
                        border: border(7.0, 0.0, Color::TRANSPARENT),
                        ..button::Style::default()
                    });
            if !inert && !chosen {
                segment = segment.on_press(Message::Bar(Msg::Form(r.name.clone(), word)));
            }
            segments = segments.push(container(segment).id(format!("form:{}:{word}", r.name)));
        }
        let track = container(segments)
            .padding(2)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(fade(
                    crate::theme::Ink::Token(|t| t.segmented).get(),
                    ink,
                ))),
                border: if focused {
                    ring(9.0)
                } else {
                    border(9.0, 0.0, Color::TRANSPARENT)
                },
                ..container::Style::default()
            });
        let lit = if first {
            self.lit(model::FORM_ROW)
        } else {
            0.0
        };
        let mut out = container(row![words, track].spacing(16).align_y(Alignment::Center))
            .padding(Padding {
                top: 9.0,
                right: 14.0,
                bottom: 9.0,
                left: 16.0,
            })
            .center_y(Length::Shrink)
            .height(Length::Shrink)
            .style(move |_: &Theme| container::Style {
                background: (lit > 0.0).then_some(Background::Color(
                    crate::theme::Ink::Accent(0.20 * lit).get(),
                )),
                border: border(11.0, 1.5, crate::theme::Ink::Accent(0.85 * lit).get()),
                ..container::Style::default()
            });
        if first {
            out = out.id(format!("row:{}", model::FORM_ROW));
        }
        container(out).id(format!("screen:{}", r.name)).into()
    }

    /// "Changer de forme à la main": the shortcut as bound now.
    fn by_hand_row(&self) -> El<'_> {
        let lang = self.lang;
        let chords = match self
            .bar_slot("shortcuts.toggle-bar")
            .and_then(|s| self.value(s))
        {
            Some(Value::List(list)) => list,
            _ => vec!["Super+B".to_owned()],
        };
        let said = if chords.is_empty() {
            w("aucun", "none").get(lang).to_owned()
        } else {
            chords
                .iter()
                .map(|c| crate::keys::shown(c, lang))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let words = column![
            t(
                w("Changer de forme à la main", "Change form by hand").get(lang).to_owned(),
                14.0,
                TEXT.get()
            )
            .font(SEMIBOLD),
            t(
                w(
                    "Un clic sur le bord extérieur de la barre, ou Super+B. En automatique, l’écran retient ton choix.",
                    "A click on the bar’s outer edge, or Super+B. In automatic, the screen remembers your choice.",
                )
                .get(lang)
                .replace("Super+B", &said),
                12.5,
                DIM.get()
            ),
            self.bar_link(
                "shortcuts.toggle-bar",
                w(
                    "Changer le raccourci dans Clavier et raccourcis ›",
                    "Change the shortcut in Keyboard and shortcuts ›",
                )
                .get(lang),
                1.0
            ),
        ]
        .spacing(2)
        .width(Length::Fill);
        let lit = self.lit(model::BY_HAND);
        container(
            row![
                words,
                container(t(said, 12.0, TEXT.get()).font(MONO))
                    .padding([3, 8])
                    .style(|_: &Theme| container::Style {
                        background: Some(Background::Color(crate::theme::ink(rgba(
                            255, 255, 255, 0.06
                        )))),
                        border: border(6.0, 1.0, RIM_12.get()),
                        ..container::Style::default()
                    }),
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 9.0,
            right: 14.0,
            bottom: 9.0,
            left: 16.0,
        })
        .style(move |_: &Theme| container::Style {
            background: (lit > 0.0).then_some(Background::Color(
                crate::theme::Ink::Accent(0.20 * lit).get(),
            )),
            border: border(11.0, 1.5, crate::theme::Ink::Accent(0.85 * lit).get()),
            ..container::Style::default()
        })
        .id(format!("row:{}", model::BY_HAND))
        .into()
    }

    // --- The preview ------------------------------------------------------------------

    /// What the preview draws now.
    fn preview(&self) -> Preview {
        let lang = self.lang;
        let rows = self.form_rows();
        let laptop = self.preview_screen(&rows);
        // The screens at 1/30 (less when they do not fit), as 6.9 places
        // them.
        let placed: Vec<&FormRow> = rows.iter().filter(|r| r.width > 0).collect();
        let left = placed.iter().map(|r| r.position.0).min().unwrap_or(0);
        let right = placed
            .iter()
            .map(|r| r.position.0 + r.width)
            .max()
            .unwrap_or(1);
        let top = placed.iter().map(|r| r.position.1).min().unwrap_or(0);
        let scale = (1.0 / 30.0_f32).min((PREVIEW - 2.0) / (right - left).max(1) as f32);
        let screens: Vec<Mini> = placed
            .iter()
            .map(|r| {
                let rect = Rectangle::new(
                    Point::new(
                        1.0 + (r.position.0 - left) as f32 * scale,
                        (r.position.1 - top) as f32 * scale,
                    ),
                    Size::new(r.width as f32 * scale, r.height as f32 * scale),
                );
                let form = r.bar.map(|_| r.form());
                let state = match (r.plugged, form, lang) {
                    (false, _, Lang::French) => "débranché".to_owned(),
                    (false, _, Lang::English) => "unplugged".to_owned(),
                    (true, None, Lang::French) => "sans barre".to_owned(),
                    (true, None, Lang::English) => "no bar".to_owned(),
                    (true, Some(f), _) => form_word(f, lang).to_owned(),
                };
                Mini {
                    rect,
                    bar: form.map(|f| f.width() as f32 * scale),
                    plugged: r.plugged,
                    caption: format!("{} · {state}", r.short),
                }
            })
            .collect();
        let screens_high = screens
            .iter()
            .map(|s| s.rect.y + s.rect.height)
            .fold(0.0, f32::max);
        let form = self.bar.shown.unwrap_or(Form::Full);
        let caption = laptop.as_ref().map_or_else(String::new, |l| {
            format!("{} · {}", l.short, form_word(form, lang))
        });
        let file = self.bar_file();
        let mut groups = Vec::new();
        for g in file.order() {
            if self.bar_bool(model::group_setting(g)) {
                groups.push(g);
            }
        }
        let twelve = matches!(
            self.bar_slot("locale.time").and_then(|s| self.value(s)),
            Some(Value::Word("12h"))
        );
        let now = jiff::Zoned::now();
        let (hour, minute, second) = (now.hour(), now.minute(), now.second());
        let (hh, half) = if twelve {
            let h = match hour % 12 {
                0 => 12,
                h => h,
            };
            (h.to_string(), Some(if hour < 12 { "AM" } else { "PM" }))
        } else {
            (format!("{hour:02}"), None)
        };
        let font_name = match self.bar_slot(model::FONT).and_then(|s| self.value(s)) {
            Some(Value::Text(name)) if name != model::BUNDLED && !name.is_empty() => {
                Some(&*Box::leak(name.into_boxed_str()))
            }
            _ => None,
        };
        let colour = hocus_link::wallpaper::colour_of(&self.clock_colour_word())
            .map_or(Color::from_rgb8(0xEE, 0xF1, 0xF6), |[r, g, b]| {
                Color::from_rgb8(r, g, b)
            });
        let played = self.bar.played.and_then(|(word, since)| {
            let t = self.now.saturating_duration_since(since);
            (t < PLAY && !self.reduced_motion).then(|| (word, t.as_secs_f32() / PLAY.as_secs_f32()))
        });
        Preview {
            label: w("APERÇU", "PREVIEW").get(lang).to_owned(),
            caption,
            screens,
            screens_high,
            width: self.preview_width() as f32,
            form,
            meeting: self.bar_bool(model::MEETING),
            windows: self.bar_bool(model::WINDOWS),
            groups,
            hh,
            mm: format!("{minute:02}"),
            ss: self
                .bar_bool(model::SECONDS)
                .then(|| format!("{second:02}")),
            half,
            date: say_date(&now, lang),
            font: font_name.map_or(MONO, Font::with_name),
            colour,
            played,
            lang,
            hint: w(
                "La barre de l’écran du portable, à 55 %. Un clic sur un groupe allume sa ligne.",
                "The laptop screen’s bar, at 55 %. A click on a group lights its row.",
            )
            .get(lang)
            .to_owned(),
        }
    }

    fn preview_height(&self) -> f32 {
        // The label, the screens (54 at most) and their captions, the bar.
        22.0 + 70.0 + 24.0 + BAR_HIGH * BAR_SCALE + 14.0 + 34.0
    }

    /// The preview, over the page at its place, held at the page's top as
    /// it scrolls (as CSS's `sticky`).
    pub(super) fn bar_preview_layer(&self) -> Option<El<'_>> {
        if !self.bar_shows() {
            return None;
        }
        let slot = *self.places.at.get("bar:preview-slot")?;
        let page = self.places.page?;
        let top = page.viewport.y + 6.0;
        let y = slot.y.max(top);
        let drawing = canvas(self.preview())
            .width(PREVIEW)
            .height(self.preview_height());
        Some(
            pin(container(drawing).id("bar:preview".to_owned()))
                .x(slot.x)
                .y(y)
                .into(),
        )
    }

    /// What the log says of the page, for tests.
    pub(super) fn bar_shown(&self) -> serde_json::Value {
        let rows: Vec<serde_json::Value> = self
            .form_rows()
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "title": r.title,
                    "form": r.asked,
                    "line": self.form_line(&r),
                    "back": r.plugged && r.asked == "auto" && r.chosen.is_some(),
                    "inert": !r.plugged,
                })
            })
            .collect();
        let preview = self.preview();
        let groups: Vec<&str> = preview.layout().into_iter().map(|(id, _)| id).collect();
        let file = self.bar_file();
        let nothing: Vec<&str> = [model::MEETING, model::TODAY, model::TASKS]
            .into_iter()
            .filter(|id| {
                if *id == model::TASKS {
                    !file.tasks
                } else {
                    !file.calendars
                }
            })
            .collect();
        let colour = crate::model::setting(model::COLOR).and_then(|s| {
            let value = self.value(Slot::of(s, self.source))?;
            Some(
                self.bar_menu_said(s, &value)
                    .unwrap_or_else(|| s.say(&value, self.lang)),
            )
        });
        let order = file.order().map(BarGroup::name);
        let screens: Vec<String> = preview.screens.iter().map(|s| s.caption.clone()).collect();
        serde_json::json!({
            "forms": rows,
            "order": order,
            "screens_line": self.bar_line(model::SCREENS),
            "nothing": nothing,
            "colour": colour,
            "preview": {
                "caption": preview.caption,
                "form": preview.form.name(),
                "groups": groups,
                "screens": screens,
                "seconds": preview.ss.is_some(),
                "played": preview.played.map(|(word, _)| word),
            },
        })
    }
}

/// "jeudi 5 novembre", as the bar's date line.
fn say_date(now: &jiff::Zoned, lang: Lang) -> String {
    const DAYS_FR: [&str; 7] = [
        "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
    ];
    const DAYS_EN: [&str; 7] = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    const MONTHS_FR: [&str; 12] = [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ];
    const MONTHS_EN: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let day = usize::from(now.weekday().to_monday_zero_offset().unsigned_abs());
    let month = usize::try_from(now.month() - 1).unwrap_or(0);
    match lang {
        Lang::French => format!(
            "{} {} {}",
            DAYS_FR[day % 7],
            now.day(),
            MONTHS_FR[month % 12]
        ),
        Lang::English => format!(
            "{} {} {}",
            DAYS_EN[day % 7],
            now.day(),
            MONTHS_EN[month % 12]
        ),
    }
}

/// A screen in the preview's row.
#[derive(Debug, Clone)]
struct Mini {
    rect: Rectangle,
    /// Its bar's width there, if it has one.
    bar: Option<f32>,
    plugged: bool,
    caption: String,
}

/// The preview, as drawn.
struct Preview {
    label: String,
    caption: String,
    screens: Vec<Mini>,
    screens_high: f32,
    /// The laptop's bar's width now, logical pixels.
    width: f32,
    form: Form,
    meeting: bool,
    windows: bool,
    groups: Vec<BarGroup>,
    hh: String,
    mm: String,
    ss: Option<String>,
    half: Option<&'static str>,
    date: String,
    font: Font,
    colour: Color,
    played: Option<(&'static str, f32)>,
    lang: Lang,
    /// Under it: what it is, and that a click lights a row.
    hint: String,
}

/// The sample content's heights, logical pixels (the mockup's).
const CARD: f32 = 44.0;
const HEAD: f32 = 18.0;
const TODAY: f32 = HEAD + 2.0 * 20.0;
const TASKS: f32 = HEAD + 3.0 * 22.0;
const AGENTS: f32 = HEAD + 3.0 * 34.0;
const WINDOWS: f32 = 3.0 * 30.0;
const GAP: f32 = 14.0;
/// The full form's lower groups, from the bar's bottom up: its 12 px of
/// padding, the footer, the levels, the connections, the desktops.
const FOOTER_TOP: f32 = BAR_HIGH - 12.0 - 34.0;
const LEVELS_TOP: f32 = FOOTER_TOP - GAP - 74.0;
const CONNS_TOP: f32 = LEVELS_TOP - GAP - 94.0;
const DESKTOPS_TOP: f32 = CONNS_TOP - GAP - 30.0;

/// The mockup's colours.
const INK: Color = Color::from_rgb(
    0xEE as f32 / 255.0,
    0xF1 as f32 / 255.0,
    0xF6 as f32 / 255.0,
);
const INK_DIM: Color = Color::from_rgb(
    0xA9 as f32 / 255.0,
    0xB2 as f32 / 255.0,
    0xC2 as f32 / 255.0,
);
const INK_FAINT: Color = Color::from_rgb(
    0x8C as f32 / 255.0,
    0x96 as f32 / 255.0,
    0xA8 as f32 / 255.0,
);
const BLUE_SOFT: Color = Color::from_rgb(
    0x8F as f32 / 255.0,
    0xB3 as f32 / 255.0,
    0xFF as f32 / 255.0,
);
const AMBER: Color = Color::from_rgb(
    0xF5 as f32 / 255.0,
    0xB4 as f32 / 255.0,
    0x4A as f32 / 255.0,
);
const GREEN: Color = Color::from_rgb(
    0x6C as f32 / 255.0,
    0xC2 as f32 / 255.0,
    0x87 as f32 / 255.0,
);

impl Preview {
    /// Where the bar starts in the canvas: under the label and the
    /// screens.
    fn bar_top(&self) -> f32 {
        22.0 + self.screens_high.min(70.0) + 24.0
    }

    fn full(&self) -> bool {
        self.form == Form::Full && self.width > 120.0
    }

    /// The groups of the bar, by the row each lights, at their logical
    /// places in the bar: the clock, then the column in its order, the
    /// windows over the desktops.
    fn layout(&self) -> Vec<(&'static str, Rectangle)> {
        let full = self.full();
        let (x, width) = if full { (12.0, 172.0) } else { (6.0, 44.0) };
        let clock = if full {
            42.0 * 1.05 + 6.0 + 21.0
        } else {
            19.0 * 1.1 * 2.0
        };
        let mut y = if full { 14.0 } else { 12.0 };
        let mut out = vec![(
            model::SECONDS,
            Rectangle::new(Point::new(x, y), Size::new(width, clock)),
        )];
        y += clock + GAP;
        let mut put = |id: &'static str, high: f32, out: &mut Vec<(&'static str, Rectangle)>| {
            out.push((id, Rectangle::new(Point::new(x, y), Size::new(width, high))));
            y += high + GAP;
        };
        if self.meeting {
            put(model::MEETING, if full { CARD } else { 4.0 }, &mut out);
        }
        for g in &self.groups {
            let high = match (g, full) {
                (BarGroup::Today, true) => TODAY,
                (BarGroup::Today, false) => continue,
                (BarGroup::Tasks, true) => TASKS,
                (BarGroup::Tasks, false) => 34.0,
                (BarGroup::Agents, true) => AGENTS,
                (BarGroup::Agents, false) => 62.0,
            };
            put(model::group_setting(*g), high, &mut out);
        }
        if self.windows {
            let (top, high) = if full {
                (DESKTOPS_TOP - GAP - WINDOWS, WINDOWS)
            } else {
                (BAR_HIGH - 200.0 - GAP - 34.0, 34.0)
            };
            out.push((
                model::WINDOWS,
                Rectangle::new(Point::new(x, top), Size::new(width, high)),
            ));
        }
        out
    }

    /// The bar's top left in the canvas, and the screen's edge's x.
    fn bar_origin(&self, bounds: Rectangle) -> (Point, f32) {
        let edge_x = bounds.width - 2.0;
        let x = edge_x - 8.0 * BAR_SCALE - self.width * BAR_SCALE;
        (Point::new(x, self.bar_top()), edge_x)
    }
}

/// `s` at `at` (its top left; centred on `at.x` when `centred`).
fn write(
    f: &mut canvas::Frame,
    s: &str,
    at: Point,
    size: f32,
    font: Font,
    color: Color,
    centred: bool,
) {
    f.fill_text(canvas::Text {
        content: s.to_owned(),
        position: at,
        color,
        size: size.into(),
        font,
        align_x: if centred {
            iced::alignment::Horizontal::Center.into()
        } else {
            iced::alignment::Horizontal::Left.into()
        },
        ..canvas::Text::default()
    });
}

/// `s` cut with "…" to fit `room`.
fn elided(s: &str, size: f32, font: Font, room: f32) -> String {
    let measure = crate::app::style::measure;
    if measure(s, size, font) <= room {
        return s.to_owned();
    }
    let mut chars: Vec<char> = s.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let cut = format!("{}…", chars.iter().collect::<String>().trim_end());
        if measure(&cut, size, font) <= room {
            return cut;
        }
    }
    "…".to_owned()
}

/// `s` with its right at `right`.
fn write_right(
    f: &mut canvas::Frame,
    s: &str,
    right: f32,
    y: f32,
    size: f32,
    font: Font,
    color: Color,
) {
    let w = crate::app::style::measure(s, size, font);
    write(f, s, Point::new(right - w, y), size, font, color, false);
}

fn rounded(f: &mut canvas::Frame, at: Point, size: Size, radius: f32, fill: Color) {
    f.fill(
        &canvas::Path::rounded_rectangle(at, size, radius.into()),
        fill,
    );
}

/// An agent's panda's head: white, its ring in its state's colour.
fn head(f: &mut canvas::Frame, centre: Point, radius: f32, ring: Color) {
    f.fill(
        &canvas::Path::circle(centre, radius),
        Color::from_rgb8(0x1c, 0x21, 0x30),
    );
    f.fill(
        &canvas::Path::circle(Point::new(centre.x, centre.y + radius * 0.1), radius * 0.48),
        Color::from_rgb8(0xF4, 0xF6, 0xFA),
    );
    f.stroke(
        &canvas::Path::circle(centre, radius),
        canvas::Stroke::default().with_color(ring).with_width(1.5),
    );
}

impl canvas::Program<Message> for Preview {
    type State = ();

    fn update(
        &self,
        (): &mut (),
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event else {
            return None;
        };
        let at = cursor.position_in(bounds)?;
        let (origin, _) = self.bar_origin(bounds);
        let local = Point::new((at.x - origin.x) / BAR_SCALE, (at.y - origin.y) / BAR_SCALE);
        let (id, _) = self
            .layout()
            .into_iter()
            .rev()
            .find(|(_, r)| r.contains(local))?;
        Some(canvas::Action::publish(Message::Bar(Msg::Lit(id))).and_capture())
    }

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // The label line.
        write(
            &mut frame,
            &self.label,
            Point::new(0.0, 2.0),
            11.5,
            BOLD,
            DIM.get(),
            false,
        );
        write_right(
            &mut frame,
            &self.caption,
            bounds.width,
            2.0,
            11.5,
            MANROPE,
            FAINT.get(),
        );
        // The screens, as Écrans places them, each its caption under it.
        for s in &self.screens {
            let rect = Rectangle::new(Point::new(s.rect.x, 22.0 + s.rect.y), s.rect.size());
            let path = canvas::Path::rounded_rectangle(rect.position(), rect.size(), 4.0.into());
            if s.plugged {
                frame.fill(
                    &path,
                    canvas::Gradient::Linear(
                        canvas::gradient::Linear::new(
                            rect.position(),
                            Point::new(rect.x + rect.width * 0.35, rect.y + rect.height),
                        )
                        .add_stop(0.0, Color::from_rgb8(0x1d, 0x2a, 0x44))
                        .add_stop(1.0, Color::from_rgb8(0x12, 0x1a, 0x2b)),
                    ),
                );
                frame.stroke(
                    &path,
                    canvas::Stroke::default()
                        .with_color(rgba(255, 255, 255, 0.18))
                        .with_width(1.0),
                );
                if let Some(bar) = s.bar {
                    let w = bar.max(2.0);
                    rounded(
                        &mut frame,
                        Point::new(rect.x + rect.width - 1.0 - w, rect.y + 1.0),
                        Size::new(w, (rect.height - 2.0).max(1.0)),
                        2.0,
                        rgba(238, 241, 246, 0.55),
                    );
                }
            } else {
                frame.stroke(
                    &path,
                    canvas::Stroke {
                        line_dash: canvas::LineDash {
                            segments: &[3.0, 3.0],
                            offset: 0,
                        },
                        ..canvas::Stroke::default()
                            .with_color(rgba(255, 255, 255, 0.24))
                            .with_width(1.0)
                    },
                );
            }
            write(
                &mut frame,
                &s.caption,
                Point::new(rect.x + rect.width / 2.0, rect.y + rect.height + 4.0),
                10.5,
                MANROPE,
                INK_DIM,
                true,
            );
        }
        // The laptop's bar at 55 %, 8 px (at 55 %) from its screen's edge.
        let (origin, edge_x) = self.bar_origin(bounds);
        let top = self.bar_top();
        rounded(
            &mut frame,
            Point::new(edge_x, top - 6.0),
            Size::new(2.0, BAR_HIGH * BAR_SCALE + 12.0),
            1.0,
            rgba(255, 255, 255, 0.22),
        );
        frame.with_save(|f| {
            f.translate(iced::Vector::new(origin.x, origin.y));
            f.scale(BAR_SCALE);
            self.draw_bar(f);
        });
        frame.fill_text(canvas::Text {
            content: self.hint.clone(),
            position: Point::new(0.0, top + BAR_HIGH * BAR_SCALE + 14.0),
            color: FAINT.get(),
            size: 12.0.into(),
            font: MANROPE,
            max_width: bounds.width,
            ..canvas::Text::default()
        });
        vec![frame.into_geometry()]
    }
}

impl Preview {
    /// The bar at its logical sizes, `self.width` wide, 960 high, with the
    /// mockup's sample content.
    fn draw_bar(&self, f: &mut canvas::Frame) {
        let bw = self.width;
        let full = self.full();
        let body =
            canvas::Path::rounded_rectangle(Point::ORIGIN, Size::new(bw, BAR_HIGH), 14.0.into());
        f.fill(
            &body,
            canvas::Gradient::Linear(
                canvas::gradient::Linear::new(Point::ORIGIN, Point::new(0.0, BAR_HIGH))
                    .add_stop(0.0, Color::from_rgba8(22, 27, 42, 0.95))
                    .add_stop(1.0, Color::from_rgba8(14, 17, 28, 0.98)),
            ),
        );
        f.stroke(
            &body,
            canvas::Stroke::default()
                .with_color(rgba(255, 255, 255, 0.10))
                .with_width(1.0),
        );
        let soft = |a: f32| rgba(255, 255, 255, a);
        let lang = self.lang;
        // The minutes' transition, played once.
        let (lift, alpha) = match self.played {
            Some(("odometer", p)) => ((1.0 - ease(p)) * 30.0, ease(p)),
            Some(("roll", p)) => ((1.0 - ease(p)) * 18.0, p),
            Some(("flip", p)) => (0.0, ease(p)),
            Some(("neon", p)) => (
                0.0,
                if (0.12..0.22).contains(&p) || (0.36..0.5).contains(&p) {
                    0.2
                } else {
                    1.0
                },
            ),
            _ => (0.0, 1.0),
        };
        let minutes = Color {
            a: alpha,
            ..self.colour
        };
        let measure = crate::app::style::measure;
        if full {
            // HH:MM at 42 px, the colon pulled in, the seconds (or AM/PM)
            // after on the same baseline; the date under them.
            let hh = measure(&self.hh, 42.0, self.font);
            let colon = measure(":", 42.0, self.font) - 2.0 * 0.12 * 42.0;
            let mm = measure(&self.mm, 42.0, self.font);
            let aside = self.half.map(str::to_owned).or_else(|| self.ss.clone());
            let (aside_font, aside_size) = match self.half {
                Some(_) => (MANROPE, 13.0),
                None => (MONO, 18.0),
            };
            let aside_w = aside
                .as_ref()
                .map_or(0.0, |a| 4.0 + measure(a, aside_size, aside_font));
            let mut x = (bw - (hh + colon + mm + aside_w)) / 2.0;
            write(
                f,
                &self.hh,
                Point::new(x, 14.0),
                42.0,
                self.font,
                self.colour,
                false,
            );
            x += hh;
            write(
                f,
                ":",
                Point::new(x - 0.12 * 42.0, 14.0),
                42.0,
                self.font,
                self.colour,
                false,
            );
            x += colon;
            write(
                f,
                &self.mm,
                Point::new(x, 14.0 + lift),
                42.0,
                self.font,
                minutes,
                false,
            );
            x += mm;
            if let Some(aside) = aside {
                write(
                    f,
                    &aside,
                    Point::new(x + 4.0, 14.0 + 42.0 - aside_size - 4.0),
                    aside_size,
                    aside_font,
                    INK_FAINT,
                    false,
                );
            }
            write(
                f,
                &self.date,
                Point::new(bw / 2.0, 14.0 + 42.0 * 1.05 + 6.0),
                15.5,
                MANROPE,
                INK_DIM,
                true,
            );
        } else {
            let centre = bw / 2.0;
            write(
                f,
                &self.hh,
                Point::new(centre, 12.0),
                19.0,
                self.font,
                self.colour,
                true,
            );
            write(
                f,
                &self.mm,
                Point::new(centre, 12.0 + 20.9 + lift * 0.5),
                19.0,
                self.font,
                minutes,
                true,
            );
        }
        for (id, r) in self.layout() {
            if !full {
                let c = Point::new(bw / 2.0, r.y + r.height / 2.0);
                match id {
                    model::MEETING => rounded(
                        f,
                        Point::new(c.x - 15.0, r.y),
                        Size::new(30.0, 4.0),
                        2.0,
                        BLUE_SOFT,
                    ),
                    model::TASKS => {
                        f.stroke(
                            &canvas::Path::rounded_rectangle(
                                Point::new(c.x - 13.0, c.y - 6.0),
                                Size::new(12.0, 12.0),
                                3.0.into(),
                            ),
                            canvas::Stroke::default()
                                .with_color(INK_DIM)
                                .with_width(1.5),
                        );
                        write(
                            f,
                            "3",
                            Point::new(c.x + 7.0, c.y - 8.0),
                            11.0,
                            MONO,
                            INK_DIM,
                            true,
                        );
                    }
                    model::AGENTS => {
                        head(f, Point::new(c.x, r.y + 20.0), 20.0, AMBER);
                        f.fill(
                            &canvas::Path::circle(Point::new(c.x + 14.0, r.y + 34.0), 8.0),
                            AMBER,
                        );
                        write(
                            f,
                            "1",
                            Point::new(c.x + 14.0, r.y + 27.0),
                            10.0,
                            BOLD,
                            Color::from_rgb8(0x1C, 0x14, 0x06),
                            true,
                        );
                        write(
                            f,
                            "1",
                            Point::new(c.x, r.y + 44.0),
                            11.0,
                            SEMIBOLD,
                            INK_DIM,
                            true,
                        );
                    }
                    model::WINDOWS => rounded(
                        f,
                        Point::new(c.x - 22.0, r.y),
                        Size::new(44.0, 34.0),
                        10.0,
                        soft(0.05),
                    ),
                    _ => {}
                }
                continue;
            }
            match id {
                model::MEETING => {
                    rounded(
                        f,
                        r.position(),
                        r.size(),
                        10.0,
                        Color::from_rgba8(143, 179, 255, 0.10),
                    );
                    f.with_save(|g| {
                        g.fill_rectangle(
                            Point::new(r.x, r.y + 2.0),
                            Size::new(6.0, r.height - 4.0),
                            BLUE_SOFT,
                        );
                    });
                    write(
                        f,
                        w("Point d’équipe", "Team check-in").get(lang),
                        Point::new(r.x + 14.0, r.y + 5.0),
                        13.5,
                        BOLD,
                        INK,
                        false,
                    );
                    write(
                        f,
                        w("14:30 · dans 48 min", "14:30 · in 48 min").get(lang),
                        Point::new(r.x + 14.0, r.y + 24.0),
                        12.0,
                        MANROPE,
                        INK_DIM,
                        false,
                    );
                    f.stroke(
                        &canvas::Path::new(|b| {
                            let (x, y) = (r.x + r.width - 24.0, r.y + 14.0);
                            b.move_to(Point::new(x + 2.3, y + 4.7));
                            b.line_to(Point::new(x + 10.0, y + 4.7));
                            b.line_to(Point::new(x + 10.0, y + 11.3));
                            b.line_to(Point::new(x + 2.3, y + 11.3));
                            b.close();
                            b.move_to(Point::new(x + 10.0, y + 7.0));
                            b.line_to(Point::new(x + 13.7, y + 5.0));
                            b.line_to(Point::new(x + 13.7, y + 11.0));
                            b.line_to(Point::new(x + 10.0, y + 9.0));
                        }),
                        canvas::Stroke::default()
                            .with_color(Color::from_rgb8(0xA9, 0xC4, 0xFF))
                            .with_width(1.2),
                    );
                }
                model::TODAY => {
                    write(
                        f,
                        w("AUJOURD’HUI", "TODAY").get(lang),
                        Point::new(r.x, r.y + 3.0),
                        11.5,
                        BOLD,
                        INK_DIM,
                        false,
                    );
                    for (i, (at, what, dot)) in [
                        ("16:00", w("Revue du budget", "Budget review"), GREEN),
                        ("18:30", w("Cours de yoga", "Yoga class"), AMBER),
                    ]
                    .iter()
                    .enumerate()
                    {
                        let y = r.y + HEAD + i as f32 * 20.0;
                        write(f, at, Point::new(r.x, y + 3.0), 12.0, MONO, INK_DIM, false);
                        f.fill(
                            &canvas::Path::circle(Point::new(r.x + 52.0, y + 10.0), 3.5),
                            *dot,
                        );
                        write(
                            f,
                            what.get(lang),
                            Point::new(r.x + 62.0, y + 2.0),
                            13.0,
                            MANROPE,
                            INK,
                            false,
                        );
                    }
                }
                model::TASKS => {
                    write(
                        f,
                        w("À FAIRE", "TO DO").get(lang),
                        Point::new(r.x, r.y + 3.0),
                        11.5,
                        BOLD,
                        INK_DIM,
                        false,
                    );
                    write_right(f, "+", r.x + r.width, r.y, 14.0, MANROPE, INK_DIM);
                    for (i, what) in [
                        w("Relire la page de la barre", "Review the bar's page"),
                        w("Rappeler le garage", "Call the garage back"),
                        w("Acheter du café", "Buy coffee"),
                    ]
                    .iter()
                    .enumerate()
                    {
                        let y = r.y + HEAD + i as f32 * 22.0;
                        f.stroke(
                            &canvas::Path::rounded_rectangle(
                                Point::new(r.x + 4.0, y + 4.0),
                                Size::new(14.0, 14.0),
                                4.0.into(),
                            ),
                            canvas::Stroke::default()
                                .with_color(soft(0.38))
                                .with_width(1.5),
                        );
                        write(
                            f,
                            what.get(lang),
                            Point::new(r.x + 28.0, y + 3.0),
                            13.0,
                            MANROPE,
                            INK,
                            false,
                        );
                    }
                }
                model::AGENTS => {
                    write(
                        f,
                        "AGENTS",
                        Point::new(r.x, r.y + 3.0),
                        11.5,
                        BOLD,
                        INK_DIM,
                        false,
                    );
                    write_right(f, "3", r.x + r.width, r.y + 2.0, 11.0, MANROPE, INK_DIM);
                    for (i, (name, says, ring, lit)) in [
                        ("hocus-panda", w("t’attend", "waiting"), AMBER, true),
                        ("site-web", w("12 min", "12 min"), GREEN, false),
                        ("carnet", w("repos", "resting"), soft(0.20), false),
                    ]
                    .iter()
                    .enumerate()
                    {
                        let y = r.y + HEAD + i as f32 * 34.0;
                        if *lit {
                            rounded(
                                f,
                                Point::new(r.x - 6.0, y),
                                Size::new(r.width + 12.0, 34.0),
                                8.0,
                                Color::from_rgba8(245, 180, 74, 0.10),
                            );
                        }
                        head(f, Point::new(r.x + 13.0, y + 17.0), 13.0, *ring);
                        write(
                            f,
                            name,
                            Point::new(r.x + 34.0, y + 8.0),
                            13.5,
                            SEMIBOLD,
                            INK,
                            false,
                        );
                        let (colour, font) = match i {
                            0 => (AMBER, BOLD),
                            1 => (INK_DIM, MANROPE),
                            _ => (INK_FAINT, MANROPE),
                        };
                        write_right(
                            f,
                            says.get(lang),
                            r.x + r.width,
                            y + 10.0,
                            12.0,
                            font,
                            colour,
                        );
                    }
                }
                model::WINDOWS => {
                    for (i, (name, colour)) in [
                        (
                            w("Terminal — hocus-panda", "Terminal — hocus-panda"),
                            Color::from_rgb8(0x5B, 0x8D, 0xEF),
                        ),
                        (
                            w("Navigateur — Agenda", "Browser — Calendar"),
                            Color::from_rgb8(0xF2, 0x94, 0x5B),
                        ),
                        (w("Fichiers — Images", "Files — Pictures"), GREEN),
                    ]
                    .iter()
                    .enumerate()
                    {
                        let y = r.y + i as f32 * 30.0;
                        if i == 0 {
                            rounded(
                                f,
                                Point::new(r.x - 6.0, y),
                                Size::new(r.width + 12.0, 30.0),
                                8.0,
                                soft(0.08),
                            );
                        }
                        rounded(
                            f,
                            Point::new(r.x, y + 7.0),
                            Size::new(16.0, 16.0),
                            4.0,
                            *colour,
                        );
                        let name = elided(name.get(lang), 13.0, MANROPE, r.width - 24.0 - 18.0);
                        write(
                            f,
                            &name,
                            Point::new(r.x + 24.0, y + 7.0),
                            13.0,
                            MANROPE,
                            INK,
                            false,
                        );
                        write_right(f, "✕", r.x + r.width, y + 8.0, 11.0, MANROPE, INK_FAINT);
                    }
                }
                _ => {}
            }
        }
        if full {
            // The desktops, the connections, the levels, the footer.
            let tile = (172.0 - 3.0 * 6.0) / 4.0;
            for i in 0..4 {
                let at = Point::new(12.0 + i as f32 * (tile + 6.0), DESKTOPS_TOP);
                rounded(f, at, Size::new(tile, 30.0), 6.0, soft(0.05));
                f.stroke(
                    &canvas::Path::rounded_rectangle(at, Size::new(tile, 30.0), 6.0.into()),
                    canvas::Stroke::default()
                        .with_color(if i == 0 { GREEN } else { soft(0.10) })
                        .with_width(if i == 0 { 2.0 } else { 1.0 }),
                );
                if i < 2 {
                    rounded(
                        f,
                        Point::new(at.x + tile * 0.12, at.y + 7.5),
                        Size::new(tile * 0.6, 15.0),
                        2.0,
                        Color::from_rgba8(236, 239, 244, 0.30),
                    );
                } else {
                    write(
                        f,
                        &(i + 1).to_string(),
                        Point::new(at.x + tile / 2.0, at.y + 8.0),
                        11.0,
                        SEMIBOLD,
                        INK_FAINT,
                        true,
                    );
                }
            }
            for (i, (name, right)) in [
                ("Maison", ""),
                (w("Casque", "Headset").get(lang), "82 %"),
                (w("Français", "French").get(lang), "FR"),
            ]
            .iter()
            .enumerate()
            {
                let y = CONNS_TOP + i as f32 * 32.0;
                f.stroke(
                    &canvas::Path::circle(Point::new(21.0, y + 15.0), 7.0),
                    canvas::Stroke::default().with_color(INK).with_width(1.6),
                );
                write(
                    f,
                    name,
                    Point::new(38.0, y + 6.0),
                    14.5,
                    MANROPE,
                    INK,
                    false,
                );
                if i == 2 {
                    rounded(
                        f,
                        Point::new(184.0 - 26.0, y + 6.0),
                        Size::new(26.0, 18.0),
                        6.0,
                        soft(0.08),
                    );
                    write(
                        f,
                        right,
                        Point::new(184.0 - 13.0, y + 7.0),
                        12.0,
                        BOLD,
                        INK_DIM,
                        true,
                    );
                } else if !right.is_empty() {
                    write_right(f, right, 184.0, y + 8.0, 13.0, MANROPE, INK_DIM);
                }
            }
            for (i, (pc, a, b)) in [
                (0.60, (0x5B, 0x8D, 0xEF), (0x8F, 0xB3, 0xFF)),
                (0.70, (0xE8, 0xB4, 0x4E), (0xFF, 0xE0, 0x8A)),
                (0.82, (0x3F, 0xA6, 0x6B), (0x7E, 0xE0, 0xA0)),
            ]
            .iter()
            .enumerate()
            {
                let y = LEVELS_TOP + i as f32 * 28.0;
                f.stroke(
                    &canvas::Path::circle(Point::new(21.0, y + 9.0), 6.0),
                    canvas::Stroke::default().with_color(INK).with_width(1.6),
                );
                let (x, w) = (38.0, 108.0);
                rounded(
                    f,
                    Point::new(x, y + 5.0),
                    Size::new(w, 8.0),
                    4.0,
                    soft(0.10),
                );
                f.fill(
                    &canvas::Path::rounded_rectangle(
                        Point::new(x, y + 5.0),
                        Size::new(w * pc, 8.0),
                        4.0.into(),
                    ),
                    canvas::Gradient::Linear(
                        canvas::gradient::Linear::new(
                            Point::new(x, 0.0),
                            Point::new(x + w * pc, 0.0),
                        )
                        .add_stop(0.0, Color::from_rgb8(a.0, a.1, a.2))
                        .add_stop(1.0, Color::from_rgb8(b.0, b.1, b.2)),
                    ),
                );
                write_right(
                    f,
                    &format!("{}", (pc * 100.0).round()),
                    184.0,
                    y + 1.0,
                    14.5,
                    MANROPE,
                    INK_DIM,
                );
            }
            for i in 0..3 {
                rounded(
                    f,
                    Point::new(12.0 + i as f32 * 44.0, FOOTER_TOP),
                    Size::new(40.0, 34.0),
                    10.0,
                    soft(0.05),
                );
            }
            f.fill(
                &canvas::Path::circle(Point::new(184.0 - 20.0, FOOTER_TOP + 17.0), 12.0),
                GREEN,
            );
            write(
                f,
                "A",
                Point::new(184.0 - 20.0, FOOTER_TOP + 9.0),
                11.0,
                BOLD,
                Color::from_rgb8(0x0b, 0x10, 0x20),
                true,
            );
        } else {
            for i in 0..4 {
                let at = Point::new(
                    bw / 2.0 - 20.0 + (i % 2) as f32 * 22.0,
                    BAR_HIGH - 200.0 + (i / 2) as f32 * 17.0,
                );
                rounded(
                    f,
                    at,
                    Size::new(18.0, 13.0),
                    4.0,
                    if i == 0 { GREEN } else { soft(0.10) },
                );
            }
            for i in 0..3 {
                rounded(
                    f,
                    Point::new(bw / 2.0 - 20.0, BAR_HIGH - 150.0 + i as f32 * 44.0),
                    Size::new(40.0, 34.0),
                    10.0,
                    soft(0.05),
                );
            }
        }
    }
}

/// Ease-out, cubic.
fn ease(p: f32) -> f32 {
    1.0 - (1.0 - p.clamp(0.0, 1.0)).powi(3)
}
