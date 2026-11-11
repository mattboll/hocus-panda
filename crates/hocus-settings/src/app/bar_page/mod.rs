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

mod preview;

use hocus_words::settings::app_bar_page as words;
use preview::*;

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
    match form {
        Form::Full => words::FULL_2.get(lang),
        Form::Mini => "mini",
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
        out.extend(setting(model::SIDE));
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
        let said = |f: &str| match f {
            "full" => words::FULL.get(lang),
            "mini" => "Mini",
            _ => "Auto",
        };
        let whose = crate::screens::lower_first(&row.title);
        let (now, then) = (said(form), said(row.asked));
        let done = words::the_bar_s_form(lang, words::TheBarSForm { now, whose: &whose });
        let said = KeysSaid {
            done,
            undone: words::undone_the_form_is_back_to(lang, then),
            redone: words::redone(lang, now),
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
            done: words::the_bar_s_order(lang, &now),
            undone: words::undone_the_order_is_back_to(lang, &then),
            redone: words::redone(lang, &now),
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
                Some(match (plugged.len(), all) {
                    (1, _) => words::one_screen_plugged_in_the_bar(lang, &primary),
                    (_, true) => words::A_BAR_PER_SCREEN_EACH_WITH.get(lang).to_owned(),
                    (_, false) => words::on_the_now_it_moves_to(lang, &primary),
                })
            }
            model::COLOR => {
                let colour = self.clock_colour_word();
                let name = model::COLOURS
                    .iter()
                    .find(|(hex, _)| *hex == colour)
                    .map(|(_, name)| name.get(lang));
                Some(match name {
                    Some(_) => w(words::THE_HOURS_AND_MINUTES).get(lang).to_owned(),
                    None => words::a_colour_from_the_file_it(lang, colour.to_uppercase()),
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
            w(words::CUSTOM).get(self.lang),
            hex.to_uppercase()
        ))
    }

    /// Under a row of the model's: a link (the primary screen's), or the
    /// amber line of a group with nothing to show.
    pub(super) fn bar_row_extra(&self, setting: &Setting, ink: f32) -> Option<El<'_>> {
        let lang = self.lang;
        match setting.id {
            model::SCREENS => Some(self.bar_link(
                crate::screens::PRIMARY,
                w(words::THE_PRIMARY_SCREEN_IS_CHOSEN_IN).get(lang),
                ink,
            )),
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
                            w(words::NOTHING_TO_SHOW_NO_CALENDAR_OR).get(lang),
                            12.5,
                            fade(PEACH.get(), ink),
                        ),
                        self.bar_link("agenda", w(words::CALENDAR_AND_TASKS).get(lang), ink),
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

    /// "Nouveau" beside a key new with the page: the side (10.5/700,
    /// `#8FB3FF` on its 16 %, radius 5).
    pub(super) fn bar_chip(&self, setting: &Setting) -> Option<El<'_>> {
        if setting.id != model::SIDE {
            return None;
        }
        Some(
            container(t(w(words::NEW).get(self.lang).to_owned(), 10.5, BLUE.get()).font(BOLD))
                .padding([1, 6])
                .style(|_: &Theme| container::Style {
                    background: Some(Background::Color(crate::theme::ink(rgba(
                        143, 179, 255, 0.16,
                    )))),
                    border: border(5.0, 0.0, Color::TRANSPARENT),
                    ..container::Style::default()
                })
                .into(),
        )
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
                icon(Icon::Accessibility, 14.0, FAINT.get()),
                t(
                    w(words::THE_BAR_S_TRANSPARENCY_AND_REDUCED).get(lang),
                    13.0,
                    FAINT.get()
                ),
                self.bar_link(
                    "appearance.reduce-transparency",
                    w(words::ACCESSIBILITY).get(lang),
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
            vec![model_row(model::SIDE), model_row(model::SCREENS)],
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
                Some(w(words::FULL_196_PX_MINI_56_PX).get(lang).to_owned()),
            ),
            forms,
        ));
        // CE QU'ELLE MONTRE, in the bar's order.
        let mut shows: Vec<El<'_>> = vec![
            self.always_row(w(words::THE_CLOCK_AND_THE_DATE), None),
            model_row(model::MEETING),
            self.always_row(
                w(words::MICROPHONE_CAMERA_OR_SCREEN_IN_USE),
                Some(w(words::THE_SIGN_OF_WHAT_LISTENS_FILMS)),
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
        shows.push(self.always_row(w(words::DESKTOPS_CONNECTIONS_LEVELS_BOTTOM_BUTTONS), None));
        rows = rows.push(group(
            head(
                model::SHOWS.label.get(lang),
                Some(w(words::AN_EMPTY_GROUP_TAKES_NO_ROOM).get(lang).to_owned()),
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
                t(w(words::ALWAYS).get(lang).to_owned(), 13.0, FAINT.get()),
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
            return w(words::UNPLUGGED_ITS_FORM_IS_KEPT_FOR)
                .get(lang)
                .to_owned();
        }
        let mut line = match (r.asked, r.chosen, lang) {
            ("auto", Some(c), _) => {
                let form = if c == "mini" {
                    words::FORM_MINI
                } else {
                    words::FORM_FULL_CHOSEN
                };
                words::automatic_chosen(lang, form.get(lang))
            }
            ("auto", None, _) => {
                let form = form_word(Form::default_for(r.width), lang);
                let from = hocus_link::FULL_FROM + Form::Full.width();
                words::automatic_the_screen_is_px_wide(
                    lang,
                    words::AutomaticTheScreenIsPxWide {
                        form,
                        from,
                        r_width: r.width,
                    },
                )
            }
            ("full", _, _) => words::ALWAYS_FULL.get(lang).to_owned(),
            (_, _, _) => words::ALWAYS_MINI.get(lang).to_owned(),
        };
        let screens_all =
            self.bar_slot(model::SCREENS).and_then(|s| self.value(s)) == Some(Value::Word("all"));
        if r.bar.is_none() && !screens_all {
            line.push(' ');
            line.push_str(w(words::THE_BAR_ONLY_COMES_HERE_IF).get(lang));
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
                    w(words::BACK_TO_THE_FORM_OF_ITS).get(lang).to_owned(),
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
            ("auto", w(words::AUTO)),
            ("full", w(words::FULL)),
            ("mini", w(words::MINI)),
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
            _ => vec!["Super+Shift+B".to_owned()],
        };
        let said = if chords.is_empty() {
            w(words::NONE).get(lang).to_owned()
        } else {
            chords
                .iter()
                .map(|c| crate::keys::shown(c, lang))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let words = column![
            t(
                w(words::CHANGE_FORM_BY_HAND).get(lang).to_owned(),
                14.0,
                TEXT.get()
            )
            .font(SEMIBOLD),
            t(
                w(words::A_CLICK_ON_THE_BAR_S)
                    .get(lang)
                    .replace("SHORTCUT", &said),
                12.5,
                DIM.get()
            ),
            self.bar_link(
                "shortcuts.toggle-bar",
                w(words::CHANGE_THE_SHORTCUT_IN_KEYBOARD_AND).get(lang),
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
                let state = match (r.plugged, form) {
                    (false, _) => words::UNPLUGGED.get(lang).to_owned(),
                    (true, None) => words::NO_BAR.get(lang).to_owned(),
                    (true, Some(f)) => form_word(f, lang).to_owned(),
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
            label: w(words::PREVIEW).get(lang).to_owned(),
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
            left: self.bar_slot(model::SIDE).and_then(|s| self.value(s))
                == Some(Value::Word("left")),
            hint: w(words::THE_LAPTOP_SCREEN_S_BAR_AT).get(lang).to_owned(),
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
                "side": if preview.left { "left" } else { "right" },
                "played": preview.played.map(|(word, _)| word),
            },
        })
    }
}

/// "jeudi 5 novembre", as the bar's date line.
fn say_date(now: &jiff::Zoned, lang: Lang) -> String {
    let day = usize::from(now.weekday().to_monday_zero_offset().unsigned_abs());
    let month = usize::try_from(now.month() - 1).unwrap_or(0);
    let name =
        |list: hocus_words::List, i: usize| list.get(lang).get(i).copied().unwrap_or_default();
    words::weekday_day_month(
        lang,
        words::WeekdayDayMonth {
            weekday: name(hocus_words::date::WEEKDAYS, day % 7),
            day: now.day(),
            month: name(hocus_words::date::MONTHS, month % 12),
        },
    )
}
