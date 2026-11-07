//! The "Bureaux et fenêtres" page (6.14, variant A): the desktops' count
//! (a stepper) and names (chips that become a field), then the model's
//! rows: moving and snapping, Alt+Tab, the ink.
//!
//! A desktop taken out (its ✕, or − for the last) is asked of the
//! compositor first on the Settings' line, which moves its windows to the
//! one before it and says which; the file is written once it answered (or
//! at once when no compositor listens, or after a while without an
//! answer). "Annuler" puts it back with them the same way, before the
//! file is written back. Adding one is the count written: the compositor
//! appends an empty desktop on every screen.

use std::time::Duration;

use iced::{
    Alignment, Background, Color, Length, Padding, Theme,
    keyboard::key::Named,
    widget::{Space, button, column, container, operation, row, text_input, tooltip},
};

use super::{
    App, Change, Control, Doing, KeysSaid, Message,
    style::*,
    view::{El, card_box, divided, icon, label, t},
};
use crate::{
    desktops as model,
    lang::{Lang, w},
    model::{Icon, SectionId, Setting, Value},
    screens::Said,
    store::{KeyEdit, Raw, Slot},
};

/// How long a desktop taken out or put back waits for the compositor's
/// answer before the file is written anyway.
const ANSWER: Duration = Duration::from_secs(2);
/// The name's field.
pub const FIELD: &str = "desktops:name";

/// The page's state.
#[derive(Debug, Default)]
pub struct State {
    /// The windows on each desktop, every screen's together, as the
    /// compositor says them: none when it does not.
    windows: Option<Vec<usize>>,
    /// A name being changed: its desktop (from 0), the field's text.
    editing: Option<(usize, String)>,
    /// A desktop taken out or put back, waiting for the compositor.
    waiting: Option<Waiting>,
    serial: u64,
    /// The desktops taken out: the change written, which desktop, and the
    /// windows that moved, for "Annuler" and its redo.
    removals: Vec<Removal>,
    /// "ESSAYER ICI".
    pub trial: super::desktops_trial::TrialState,
    /// "RÈGLES PAR APPLICATION".
    pub rules: super::desktops_rules::RulesState,
}

#[derive(Debug, Clone)]
struct Removal {
    after: Vec<KeyEdit>,
    index: usize,
    windows: Vec<u64>,
}

/// What waits for the compositor's answer.
#[derive(Debug, Clone)]
struct Waiting {
    serial: u64,
    what: Asked,
}

#[derive(Debug, Clone)]
enum Asked {
    /// Desktop `index` taken out, anew.
    Remove { index: usize },
    /// A change undone or redone: its keys, written once answered.
    Again(Box<Again>),
}

#[derive(Debug, Clone)]
struct Again {
    edits: Vec<KeyEdit>,
    said: KeysSaid,
    doing: Doing,
    change: Change,
}

/// The page's own messages.
#[derive(Debug, Clone)]
pub enum Msg {
    /// One desktop more (`true`) or fewer, the last.
    Step(bool),
    /// Desktop `index`'s name opened as a field.
    Edit(usize),
    /// The field's text.
    Typed(String),
    /// The field's text written.
    Commit,
    /// Desktop `index` taken out.
    Remove(usize),
    /// A desktop added after the last.
    Add,
    /// A link to another page's row.
    Link(&'static str),
    /// The answer waited for `serial` did not come.
    Late(u64),
    /// The trial area: a press, the pointer moved (its coordinates), let
    /// go; the move key's button.
    TrialPress(f32, f32),
    TrialMove(f32, f32),
    TrialRelease(f32, f32),
    Hold,
    /// A rule opened or folded, taken out; a menu of the rules opened, a
    /// choice of it; its "above" switched; the menu closed.
    RuleEdit(usize),
    RuleRemove(usize),
    RuleMenu(super::desktops_rules::RuleMenu),
    RuleChoose(super::desktops_rules::RuleMenu, usize),
    RuleAbove(usize),
    CloseRuleMenu,
}

/// A control of the page's own that takes the keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    Link(&'static str),
    Minus,
    Plus,
    Name(usize),
    Remove(usize),
    Add,
    Hold,
    RuleEdit(usize),
    RuleRemove(usize),
    RuleMenu(super::desktops_rules::RuleMenu),
    RuleAbove(usize),
}

impl App {
    fn desktops_slot(&self, id: &str) -> Option<Slot> {
        crate::model::setting(id).map(|s| Slot::of(s, self.source))
    }

    /// How many desktops the file says.
    fn desktop_count(&self) -> usize {
        match self.desktops_slot(model::COUNT).and_then(|s| self.value(s)) {
            Some(Value::Int(n)) => usize::try_from(n).unwrap_or(model::DEFAULT),
            _ => model::DEFAULT,
        }
        .clamp(1, model::MOST)
    }

    /// The names the file says, by position.
    fn desktop_names(&self) -> Vec<String> {
        match self.desktops_slot(model::NAMES).and_then(|s| self.value(s)) {
            Some(Value::List(names)) => names,
            _ => Vec::new(),
        }
    }

    fn desktops_word(&self, id: &str) -> Option<&'static str> {
        match self.desktops_slot(id).and_then(|s| self.value(s)) {
            Some(Value::Word(word)) => Some(word),
            _ => None,
        }
    }

    pub(super) fn desktops_controls(&self) -> Vec<Control> {
        let count = self.desktop_count();
        let mut out = vec![
            Control::Desktops(Stop::Link(model::SHORTCUTS)),
            Control::Desktops(Stop::Minus),
            Control::Desktops(Stop::Plus),
        ];
        for i in 0..count {
            out.push(Control::Desktops(Stop::Name(i)));
            if count > 1 {
                out.push(Control::Desktops(Stop::Remove(i)));
            }
        }
        if count < model::MOST {
            out.push(Control::Desktops(Stop::Add));
        }
        for setting in crate::desktops::settings() {
            if setting.id == model::COUNT || setting.id == model::NAMES {
                continue;
            }
            out.push(Control::Setting(Slot::of(setting, self.source)));
            if setting.id == model::WOBBLE {
                if self.reduced_motion {
                    out.push(Control::Desktops(Stop::Link(REDUCED)));
                }
                out.push(Control::Desktops(Stop::Hold));
            }
        }
        out.extend(self.rules_controls().into_iter().map(Control::Desktops));
        out
    }

    fn desktops_focused(&self, stop: &Stop) -> bool {
        matches!(self.focus, super::Focus::Control(i)
            if self.controls().get(i) == Some(&Control::Desktops(stop.clone())))
    }

    // --- What changes it --------------------------------------------------------------

    pub(super) fn desktops_update(&mut self, msg: Msg) -> iced::Task<Message> {
        if !matches!(msg, Msg::Typed(_) | Msg::Commit | Msg::Late(_)) {
            self.desktops_commit();
        }
        match msg {
            Msg::TrialPress(..) | Msg::TrialMove(..) | Msg::TrialRelease(..) | Msg::Hold => {
                self.trial_update(&msg);
                iced::Task::none()
            }
            Msg::RuleEdit(_)
            | Msg::RuleRemove(_)
            | Msg::RuleMenu(_)
            | Msg::RuleChoose(..)
            | Msg::RuleAbove(_)
            | Msg::CloseRuleMenu => self.rules_update(msg),
            Msg::Step(true) | Msg::Add => {
                self.desktop_add();
                iced::Task::none()
            }
            Msg::Step(false) => {
                let count = self.desktop_count();
                self.desktop_remove(count - 1)
            }
            Msg::Remove(index) => self.desktop_remove(index),
            Msg::Edit(index) => {
                if !self.writable() || index >= self.desktop_count() {
                    return iced::Task::none();
                }
                let names = self.desktop_names();
                let now = names
                    .get(index)
                    .map(|n| n.trim().to_owned())
                    .unwrap_or_default();
                self.desktops.editing = Some((index, now));
                iced::Task::batch([operation::focus(FIELD), operation::select_all(FIELD)])
            }
            Msg::Typed(text) => {
                if let Some((_, now)) = &mut self.desktops.editing {
                    *now = text.chars().take(model::NAME_MOST).collect();
                }
                iced::Task::none()
            }
            Msg::Commit => {
                self.desktops_commit();
                operation::focus("nothing")
            }
            Msg::Link(target) => self.show_row(target),
            Msg::Late(serial) => {
                if self
                    .desktops
                    .waiting
                    .as_ref()
                    .is_some_and(|w| w.serial == serial)
                {
                    tracing::warn!("the compositor did not answer: the file is written anyway");
                    self.desktops_answered(None);
                }
                iced::Task::none()
            }
        }
    }

    /// The name being changed, written (if it changed).
    pub(super) fn desktops_commit(&mut self) {
        let Some((index, text)) = self.desktops.editing.take() else {
            return;
        };
        let names = self.desktop_names();
        let before = names
            .get(index)
            .map(|n| n.trim().to_owned())
            .unwrap_or_default();
        let text = text.trim().to_owned();
        if text == before {
            return;
        }
        let Some(slot) = self.desktops_slot(model::NAMES) else {
            return;
        };
        let count = self.desktop_count();
        let after = model::trimmed(model::renamed(&names, index, &text), count);
        let lang = self.lang;
        let said = match (lang, text.is_empty()) {
            (Lang::French, false) => format!("Nom du bureau {} : {text}", index + 1),
            (Lang::French, true) => format!("Nom du bureau {} : aucun", index + 1),
            (Lang::English, false) => format!("Desktop {}’s name: {text}", index + 1),
            (Lang::English, true) => format!("Desktop {}’s name: none", index + 1),
        };
        if self.writable() {
            self.ask(
                slot,
                Raw::Set(Value::List(after)),
                Doing::New,
                None,
                None,
                Some(said),
            );
        }
    }

    /// `Esc` in the name's field: the name stays.
    pub(super) fn desktops_cancel(&mut self) -> bool {
        self.desktops.editing.take().is_some()
    }

    pub(super) fn desktops_editing(&self) -> bool {
        self.desktops.editing.is_some()
    }

    /// One desktop after the last, on every screen.
    fn desktop_add(&mut self) {
        let count = self.desktop_count();
        if count >= model::MOST || !self.writable() {
            return;
        }
        let Some(slot) = self.desktops_slot(model::COUNT) else {
            return;
        };
        let said = match self.lang {
            Lang::French => format!("Bureau {} ajouté, sur chaque écran", count + 1),
            Lang::English => format!("Desktop {} added, on every screen", count + 1),
        };
        self.ask(
            slot,
            Raw::Set(Value::Int(i64::try_from(count + 1).unwrap_or(4))),
            Doing::New,
            None,
            None,
            Some(said),
        );
    }

    /// Desktop `index` taken out: asked of the compositor first, if one
    /// listens.
    fn desktop_remove(&mut self, index: usize) -> iced::Task<Message> {
        let count = self.desktop_count();
        if count <= 1 || index >= count || !self.writable() || self.desktops.waiting.is_some() {
            return iced::Task::none();
        }
        if self.desktops.windows.is_some()
            && self.screens_ask(format!("desktop-remove {}", index + 1))
        {
            return self.desktops_wait(Asked::Remove { index });
        }
        // No compositor: the file alone, nothing to say of windows.
        self.desktops_removed(index, &[]);
        iced::Task::none()
    }

    fn desktops_wait(&mut self, what: Asked) -> iced::Task<Message> {
        self.desktops.serial += 1;
        let serial = self.desktops.serial;
        self.desktops.waiting = Some(Waiting { serial, what });
        iced::Task::perform(tokio::time::sleep(ANSWER), move |()| {
            Message::Desktops(Msg::Late(serial))
        })
    }

    /// Desktop `index` taken out, `moved` its windows: the count and the
    /// names written, with its toast and its "Annuler".
    fn desktops_removed(&mut self, index: usize, moved: &[u64]) {
        let lang = self.lang;
        let count = self.desktop_count();
        let names = self.desktop_names();
        let name = model::name_of(&names, index, lang);
        let left = model::without(&names, index);
        let to = model::name_of(&left, index.saturating_sub(1), lang);
        let after_names = model::trimmed(left, count - 1);
        let mut edits = vec![KeyEdit {
            key: vec!["desktops".into(), "count".into()],
            value: Some((count - 1).to_string()),
        }];
        edits.push(KeyEdit {
            key: vec!["desktops".into(), "names".into()],
            value: (!after_names.is_empty() || !names.is_empty())
                .then(|| model::toml_list(&after_names)),
        });
        // The rules naming it go with its windows, the later ones down one.
        let rules = self
            .snapshot
            .as_ref()
            .map(|s| s.rules.0.clone())
            .unwrap_or_default();
        let moved_rules = model::rules_without(&rules, index);
        if moved_rules != rules {
            edits.push(KeyEdit {
                key: vec!["windows".into(), "rules".into()],
                value: model::rules_toml(&moved_rules),
            });
        }
        let said = KeysSaid {
            done: model::removed_said(&name, moved.len(), &to, lang),
            undone: match lang {
                Lang::French => format!("Annulé : « {name} » revient."),
                Lang::English => format!("Undone: “{name}” is back."),
            },
            redone: match lang {
                Lang::French => format!("Rétabli : « {name} » retiré."),
                Lang::English => format!("Redone: “{name}” removed."),
            },
            row: model::COUNT,
        };
        tracing::info!(
            desktop = index + 1,
            windows = moved.len(),
            "a desktop taken out"
        );
        self.desktops.removals.push(Removal {
            after: edits.clone(),
            index,
            windows: moved.to_vec(),
        });
        self.ask_keys(edits, said, Doing::New, None);
    }

    /// The line to ask before a change of keys is undone (`undo`) or
    /// redone: a desktop taken out is put back with its windows, or taken
    /// out again. Asked, the keys are written once answered; `None`: not
    /// one of the page's.
    pub(super) fn desktops_again(
        &mut self,
        change: &Change,
        undo: bool,
    ) -> Option<iced::Task<Message>> {
        let Change::Keys {
            before,
            after,
            said,
        } = change
        else {
            return None;
        };
        let removal = self
            .desktops
            .removals
            .iter()
            .find(|r| r.after == *after)?
            .clone();
        let (line, edits, doing) = if undo {
            let ids: Vec<String> = removal.windows.iter().map(u64::to_string).collect();
            (
                format!("desktop-restore {} {}", removal.index + 1, ids.join(" ")),
                before.clone(),
                Doing::Undo,
            )
        } else {
            (
                format!("desktop-remove {}", removal.index + 1),
                after.clone(),
                Doing::Redo,
            )
        };
        let what = Asked::Again(Box::new(Again {
            edits: edits.clone(),
            said: said.clone(),
            doing,
            change: change.clone(),
        }));
        if self.desktops.windows.is_some() && self.screens_ask(line.trim().to_owned()) {
            return Some(self.desktops_wait(what));
        }
        self.ask_keys(edits, said.clone(), doing, Some(change.clone()));
        Some(iced::Task::none())
    }

    /// The compositor answered what waited (`None`: it did not, or
    /// refused): the file written.
    fn desktops_answered(&mut self, moved: Option<Vec<u64>>) {
        let Some(waiting) = self.desktops.waiting.take() else {
            return;
        };
        match waiting.what {
            Asked::Remove { index } => {
                self.desktops_removed(index, &moved.unwrap_or_default());
            }
            Asked::Again(again) => {
                let Again {
                    edits,
                    said,
                    doing,
                    change,
                } = *again;
                // Taken out again: the windows that moved this time.
                if doing == Doing::Redo
                    && let (Some(moved), Change::Keys { after, .. }) = (moved, &change)
                    && let Some(r) = self
                        .desktops
                        .removals
                        .iter_mut()
                        .find(|r| r.after == *after)
                {
                    r.windows = moved;
                }
                self.ask_keys(edits, said, doing, Some(change));
            }
        }
    }

    /// What the compositor's line says of the desktops; `false` when it is
    /// not theirs.
    pub(super) fn desktops_said(&mut self, said: &Said) -> bool {
        match said {
            Said::Desktops { count, windows } => {
                let mut windows = windows.clone();
                windows.resize(*count, 0);
                self.desktops.windows = Some(windows);
                true
            }
            Said::DesktopRemoved { windows, .. } => {
                self.desktops_answered(Some(windows.clone()));
                true
            }
            Said::DesktopRestored { .. } => {
                self.desktops_answered(None);
                true
            }
            Said::Refused(why) if self.desktops.waiting.is_some() => {
                tracing::warn!(why, "the compositor refused: the file is written anyway");
                self.desktops_answered(None);
                true
            }
            Said::Apps(apps) => {
                self.rules_apps(apps.clone());
                true
            }
            Said::Gone => {
                self.desktops.windows = None;
                if self.desktops.waiting.is_some() {
                    self.desktops_answered(None);
                }
                false
            }
            _ => false,
        }
    }

    pub(super) fn desktops_key(&mut self, stop: &Stop, named: Named) -> iced::Task<Message> {
        if !matches!(named, Named::Space | Named::Enter) {
            return iced::Task::none();
        }
        let msg = match stop {
            Stop::Link(target) => Msg::Link(target),
            Stop::Minus => Msg::Step(false),
            Stop::Plus => Msg::Step(true),
            Stop::Name(i) => Msg::Edit(*i),
            Stop::Remove(i) => Msg::Remove(*i),
            Stop::Add => Msg::Add,
            Stop::Hold => Msg::Hold,
            Stop::RuleEdit(_) | Stop::RuleRemove(_) | Stop::RuleMenu(_) | Stop::RuleAbove(_) => {
                return self.rules_key(stop);
            }
        };
        self.desktops_update(msg)
    }

    // --- What it says -----------------------------------------------------------------

    /// A row of the model's that does not apply now, and why: the wobble
    /// with reduced motion, the ink while Super moves windows.
    pub(super) fn desktops_why_not(&self, id: &str) -> Option<String> {
        let lang = self.lang;
        match id {
            model::WOBBLE if self.reduced_motion => Some(
                w(
                    "Coupées tant que Mouvements réduits est activé.",
                    "Off while Reduced motion is on.",
                )
                .get(lang)
                .to_owned(),
            ),
            model::INK if self.desktops_word(model::MOVE_KEY) == Some("super") => Some(
                w(
                    "Super + glisser déplace les fenêtres : l’encre ne se lance plus. Choisis Alt pour la retrouver.",
                    "Super+drag moves windows: the ink no longer starts. Choose Alt to get it back.",
                )
                .get(lang)
                .to_owned(),
            ),
            _ => None,
        }
    }

    pub(super) fn desktops_inert(&self, setting: &Setting) -> bool {
        setting.section == SectionId::Desktops && self.desktops_why_not(setting.id).is_some()
    }

    /// Under the wobble's row with reduced motion: the link to it.
    pub(super) fn desktops_row_extra(&self, setting: &Setting) -> Option<El<'_>> {
        (setting.id == model::WOBBLE && self.reduced_motion).then(|| {
            self.desktops_link(
                REDUCED,
                w("Apparence et langue ›", "Appearance and language ›").get(self.lang),
            )
        })
    }

    /// A choice's toast, when it says more than its value: Super to move
    /// says the ink no longer starts.
    pub(super) fn desktops_toast(&self, slot: Slot, value: &Value) -> Option<String> {
        (slot.setting == model::MOVE_KEY && *value == Value::Word("super")).then(|| {
            w(
                "Déplacer : Super + glisser · l’encre ne se lance plus",
                "Move: Super + drag · the ink no longer starts",
            )
            .get(self.lang)
            .to_owned()
        })
    }

    // --- What it draws ----------------------------------------------------------------

    fn desktops_link<'a>(&self, target: &'static str, said: &str) -> El<'a> {
        let focused = self.desktops_focused(&Stop::Link(target));
        container(
            button(t(said.to_owned(), 12.5, MINT.get()).font(SEMIBOLD))
                .padding([1, 2])
                .on_press(Message::Desktops(Msg::Link(target)))
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

    pub(super) fn desktops_page<'a>(
        &'a self,
        mut page: iced::widget::Column<'a, Message>,
    ) -> El<'a> {
        let lang = self.lang;
        page = page.push(
            row![
                icon(Icon::Keyboard, 15.0, FAINT.get()),
                t(
                    w(
                        "Les raccourcis des fenêtres et des bureaux se règlent dans",
                        "The windows’ and desktops’ shortcuts are set in",
                    )
                    .get(lang),
                    13.0,
                    FAINT.get()
                ),
                self.desktops_link(
                    model::SHORTCUTS,
                    w("Clavier et raccourcis ›", "Keyboard and shortcuts ›").get(lang),
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
        let count = self.desktop_count();
        let per_screen = match (lang, count) {
            (Lang::French, 1) => "1 bureau par écran".to_owned(),
            (Lang::French, n) => format!("{n} bureaux par écran"),
            (Lang::English, 1) => "1 desktop per screen".to_owned(),
            (Lang::English, n) => format!("{n} desktops per screen"),
        };
        page = page.push(group(
            head(model::DESKTOPS.label.get(lang), Some(per_screen)),
            vec![self.count_row(), self.names_row()],
        ));
        page = page.push(group(
            head(model::MOVE.label.get(lang), None),
            [
                model::MOVE_KEY,
                model::RESIZE_KEY,
                model::MAGNET,
                model::SNAP,
                model::WOBBLE,
            ]
            .into_iter()
            .map(model_row)
            .collect(),
        ));
        // ESSAYER ICI, under what it lets one feel.
        page = page.push(
            column![
                head(
                    w("ESSAYER ICI", "TRY IT HERE").get(lang),
                    Some(
                        w(
                            "Ce que tu fais ici n’est pas écrit",
                            "Nothing done here is written",
                        )
                        .get(lang)
                        .to_owned(),
                    ),
                ),
                card_box(self.trial_card()),
            ]
            .spacing(8),
        );
        page = page.push(group(
            head(model::ALT_TAB.label.get(lang), None),
            vec![model_row(model::ALT_TAB_ROW)],
        ));
        page = page.push(group(
            head(model::OVERVIEW.label.get(lang), None),
            vec![model_row(model::HOT_CORNER)],
        ));
        page = page.push(group(
            head(model::DRAW.label.get(lang), None),
            vec![model_row(model::INK)],
        ));
        page = page.push(group(
            head(model::RULES_GROUP.label.get(lang), None),
            self.rules_rows(),
        ));
        container(page).id("page:desktops".to_owned()).into()
    }

    pub(super) fn desktop_names_now(&self) -> Vec<String> {
        self.desktop_names()
    }

    pub(super) fn desktop_count_now(&self) -> usize {
        self.desktop_count()
    }

    pub(super) fn desktops_row_frame<'a>(&self, id: &'static str, body: El<'a>) -> El<'a> {
        self.desktops_frame(id, body)
    }

    /// A row's frame, lit when found: its words, then its control.
    fn desktops_frame<'a>(&self, id: &'static str, body: El<'a>) -> El<'a> {
        let lit = self.lit(id);
        container(body)
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
            })
            .id(format!("row:{id}"))
            .into()
    }

    fn words<'a>(&self, setting: &str) -> El<'a> {
        let lang = self.lang;
        let Some(s) = crate::model::setting(setting) else {
            return Space::new().into();
        };
        let mut words = column![t(s.name.get(lang).to_owned(), 14.0, TEXT.get()).font(SEMIBOLD)]
            .spacing(2)
            .width(Length::Fill);
        if let Some(line) = s.line {
            words = words.push(t(line.get(lang).to_owned(), 12.5, DIM.get()));
        }
        words.into()
    }

    /// "Nombre de bureaux": − the number +.
    fn count_row(&self) -> El<'_> {
        let count = self.desktop_count();
        let step = |more: bool| -> El<'_> {
            let inert = !self.writable()
                || self.desktops.waiting.is_some()
                || if more {
                    count >= model::MOST
                } else {
                    count <= 1
                };
            let alpha = if inert { INERT } else { 1.0 };
            let stop = if more { Stop::Plus } else { Stop::Minus };
            let focused = self.desktops_focused(&stop);
            let glyph =
                t(if more { "+" } else { "−" }, 16.0, fade(TEXT.get(), alpha)).font(SEMIBOLD);
            let mut b = button(container(glyph).center(30))
                .width(32)
                .height(32)
                .padding(0)
                .style(move |_, status| button::Style {
                    background: Some(Background::Color(crate::theme::ink(rgba(
                        255,
                        255,
                        255,
                        if matches!(status, button::Status::Hovered) && !inert {
                            0.10
                        } else {
                            0.06
                        },
                    )))),
                    text_color: TEXT.get(),
                    border: if focused {
                        ring(8.0)
                    } else {
                        border(8.0, 1.0, fade(RIM_12.get(), alpha))
                    },
                    ..button::Style::default()
                });
            if !inert {
                b = b.on_press(Message::Desktops(Msg::Step(more)));
            }
            container(b)
                .id(if more { "count:plus" } else { "count:minus" }.to_owned())
                .into()
        };
        let number = container(t(count.to_string(), 14.0, TEXT.get()).font(MONO))
            .width(30)
            .center_x(30)
            .id("count:number".to_owned());
        self.desktops_frame(
            model::COUNT,
            row![
                self.words(model::COUNT),
                row![step(false), number, step(true)]
                    .spacing(6)
                    .align_y(Alignment::Center),
            ]
            .spacing(16)
            .align_y(Alignment::Center)
            .into(),
        )
    }

    /// "Leurs noms": a chip per desktop, then "+ Ajouter un bureau".
    fn names_row(&self) -> El<'_> {
        let lang = self.lang;
        let count = self.desktop_count();
        let names = self.desktop_names();
        let inert = !self.writable() || self.desktops.waiting.is_some();
        let mut chips = row![].spacing(8).align_y(Alignment::Center);
        for i in 0..count {
            chips = chips.push(self.chip(i, &names, count, inert));
        }
        if count < model::MOST {
            let focused = self.desktops_focused(&Stop::Add);
            let mut add = button(
                row![
                    t("+", 14.0, MINT.get()).font(SEMIBOLD),
                    t(
                        w("Ajouter un bureau", "Add a desktop").get(lang).to_owned(),
                        13.0,
                        DIM.get()
                    )
                    .font(SEMIBOLD),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .height(32)
            .padding([6, 12])
            .style(move |_, status| button::Style {
                background: None,
                text_color: TEXT.get(),
                border: if focused {
                    ring(8.0)
                } else {
                    border(
                        8.0,
                        1.0,
                        crate::theme::ink(rgba(
                            255,
                            255,
                            255,
                            if matches!(status, button::Status::Hovered) {
                                0.32
                            } else {
                                0.20
                            },
                        )),
                    )
                },
                ..button::Style::default()
            });
            if !inert {
                add = add.on_press(Message::Desktops(Msg::Add));
            }
            chips = chips.push(container(add).id("desktops:add".to_owned()));
        }
        self.desktops_frame(
            model::NAMES,
            column![self.words(model::NAMES), chips.wrap().vertical_spacing(8)]
                .spacing(10)
                .into(),
        )
    }

    /// Desktop `i`'s chip: its number, its name (or its field), its ✕.
    fn chip<'a>(&'a self, i: usize, names: &[String], count: usize, inert: bool) -> El<'a> {
        let lang = self.lang;
        let named = names
            .get(i)
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty());
        let number = t((i + 1).to_string(), 11.0, FAINT.get()).font(MONO);
        let editing = self
            .desktops
            .editing
            .as_ref()
            .filter(|(at, _)| *at == i)
            .map(|(_, now)| now.clone());
        let name: El<'a> = match editing {
            Some(now) => container(
                text_input(&model::placeholder(i, lang), &now)
                    .id(FIELD)
                    .on_input(|s| Message::Desktops(Msg::Typed(s)))
                    .on_submit(Message::Desktops(Msg::Commit))
                    .size(13.5)
                    .font(SEMIBOLD)
                    .width(128)
                    .padding([3, 6])
                    .style(|_: &Theme, status| text_input::Style {
                        background: Background::Color(crate::theme::ink(rgba(255, 255, 255, 0.06))),
                        border: border(6.0, 1.0, rgba(108, 194, 135, 0.65)),
                        icon: DIM.get(),
                        placeholder: FAINT.get(),
                        value: TEXT.get(),
                        selection: crate::theme::Ink::Accent(
                            if matches!(status, text_input::Status::Focused { .. }) {
                                0.35
                            } else {
                                0.2
                            },
                        )
                        .get(),
                    }),
            )
            .id("desktops:field".to_owned())
            .into(),
            None => {
                let focused = self.desktops_focused(&Stop::Name(i));
                let said = named.clone().unwrap_or_else(|| model::placeholder(i, lang));
                let colour = if named.is_some() {
                    TEXT.get()
                } else {
                    FAINT.get()
                };
                let mut b = button(t(said, 13.5, colour).font(SEMIBOLD))
                    .padding([3, 4])
                    .style(flat(None, HOVER.get(), 6.0, focused));
                if !inert {
                    b = b.on_press(Message::Desktops(Msg::Edit(i)));
                }
                container(b).id(format!("chip:{i}")).into()
            }
        };
        let mut inside = row![number, name].spacing(6).align_y(Alignment::Center);
        if count > 1 {
            let focused = self.desktops_focused(&Stop::Remove(i));
            let mut close = button(container(icon(Icon::Close, 12.0, DIM.get())).center(22))
                .width(24)
                .height(24)
                .padding(0)
                .style(flat(None, HOVER.get(), 6.0, focused));
            if !inert {
                close = close.on_press(Message::Desktops(Msg::Remove(i)));
            }
            inside = inside.push(container(close).id(format!("remove:{i}")));
        }
        let chip = container(inside)
            .height(32)
            .padding(Padding {
                top: 0.0,
                right: 4.0,
                bottom: 0.0,
                left: 10.0,
            })
            .center_y(32)
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(crate::theme::ink(rgba(
                    255, 255, 255, 0.05,
                )))),
                border: border(8.0, 1.0, crate::theme::ink(rgba(255, 255, 255, 0.10))),
                ..container::Style::default()
            });
        // "Travail · 3 fenêtres", when the compositor says them.
        let windows = self
            .desktops
            .windows
            .as_ref()
            .and_then(|w| w.get(i).copied());
        match windows {
            Some(n) => {
                let tip = format!(
                    "{} · {}",
                    model::name_of(names, i, lang),
                    model::windows_said(n, lang)
                );
                tooltip(
                    chip,
                    container(t(tip, 12.0, TEXT.get()))
                        .padding([4, 8])
                        .style(|_: &Theme| container::Style {
                            background: Some(Background::Color(MENU.get())),
                            border: border(6.0, 1.0, RIM_12.get()),
                            ..container::Style::default()
                        }),
                    tooltip::Position::Top,
                )
                .into()
            }
            None => chip.into(),
        }
    }

    /// What the page shows, for the log.
    pub(super) fn desktops_shown(&self) -> serde_json::Value {
        let lang = self.lang;
        let count = self.desktop_count();
        let names = self.desktop_names();
        let chips: Vec<String> = (0..count)
            .map(|i| model::name_of(&names, i, lang))
            .collect();
        let why = |id| self.desktops_why_not(id);
        serde_json::json!({
            "count": count,
            "chips": chips,
            "windows": self.desktops.windows,
            "editing": self.desktops.editing.as_ref().map(|(i, now)| serde_json::json!([i, now])),
            "waiting": self.desktops.waiting.is_some(),
            "trial": self.trial_shown(),
            "rules": self.rules_shown(),
            "wobble_off": why(model::WOBBLE),
            "ink_off": why(model::INK),
        })
    }
}

/// Appearance's reduced motion, which the wobble's link lights.
const REDUCED: &str = "appearance.reduced-motion";
