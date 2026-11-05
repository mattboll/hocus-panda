//! The "Clavier et raccourcis" page and its "Raccourcis" sub-page (6.8,
//! variant A).
//!
//! - The page: DISPOSITIONS (the layouts in their order, each with ↑, ↓,
//!   ✕, and "+ Ajouter une disposition" with its search over XKB's
//!   registry), the layout switch, the keys' repeat with a field to try it,
//!   Caps Lock, the Compose key, Num Lock at login, and the row that opens
//!   the shortcuts.
//! - The sub-page: every shortcut of the desktop, by group, with its own
//!   search; a family of nine is one row that unfolds; a combination
//!   clicked records the next one pressed, the desktop's own shortcuts
//!   suspended meanwhile (keyboard-shortcuts-inhibit, [`crate::activate`]);
//!   a combination another action holds opens a box under the row,
//!   "Remplacer" or "Annuler"; "Rétablir" takes the line out of the file.

use std::{collections::BTreeSet, sync::mpsc};

use iced::{
    Alignment, Background, Color, Length, Padding, Task, Theme,
    keyboard::{Key, Modifiers, key::Named, key::Physical},
    widget::{Space, button, column, container, row, text, text_input},
};

use super::{
    App, Message, Page,
    style::*,
    view::{El, card_box, divided, icon, label, t},
};
use crate::{
    keyboard as model,
    keys::{self, Chord},
    lang::{Lang, w},
    model::{Icon, Kind, SectionId, Setting, Value},
    search::fold,
    store::{Reading, Slot},
    xkb::{self, Known},
};

/// The sub-page's search field, and the layouts' popover's.
pub const SUB_SEARCH: &str = "shortcuts-search";
pub const ADD_SEARCH: &str = "layout-search";

/// A shortcut being recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    pub setting: &'static str,
    /// The combination replaced; `None`: one more.
    pub index: Option<usize>,
    pub mods: Modifiers,
    /// Why the last keys were refused.
    pub refused: Option<String>,
}

/// A combination another action holds, waiting for "Remplacer".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub setting: &'static str,
    pub index: Option<usize>,
    pub chord: String,
    pub other: &'static str,
}

/// The pages' own state.
#[derive(Debug, Default)]
pub struct Keys {
    /// The "Raccourcis" sub-page shows, over the section's page.
    pub sub: bool,
    registry: Vec<Known>,
    /// The system's layouts, for a file that names none.
    system: Vec<String>,
    /// The layouts' popover, and its search.
    pub adding: Option<String>,
    pub query: String,
    unfolded: BTreeSet<&'static str>,
    pub recording: Option<Recording>,
    pub conflict: Option<Conflict>,
    inhibit: Option<mpsc::Sender<bool>>,
    try_text: String,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Registry(Vec<Known>, Vec<String>),
    Sub(bool),
    Query(String),
    Fold(&'static str),
    Record(&'static str, Option<usize>),
    Mods(Modifiers),
    Replace,
    Cancel,
    Reset(&'static str),
    ResetAll,
    Adding(bool),
    /// The popover's field takes the keyboard, once it is laid out.
    FocusAdd,
    AddQuery(String),
    Pick(String),
    Move(usize, bool),
    Remove(usize),
    Try(String),
}

fn setting(id: &str) -> Option<&'static Setting> {
    crate::model::setting(id)
}

fn slot(id: &str) -> Option<Slot> {
    setting(id).map(|s| Slot {
        setting: s.id,
        source: None,
    })
}

/// The registry and the system's layouts, read on a thread.
pub fn read_registry(lang: Lang, send: impl FnOnce(Vec<Known>, Vec<String>) + Send + 'static) {
    let started = std::thread::Builder::new()
        .name("settings' layouts".to_owned())
        .spawn(move || send(xkb::read(lang), xkb::system_layouts()));
    if let Err(err) = started {
        tracing::warn!(%err, "XKB's layouts are not read");
    }
}

impl App {
    /// An action's combinations now.
    pub(super) fn chords(&self, id: &str) -> Vec<String> {
        match slot(id).and_then(|s| self.value(s)) {
            Some(Value::List(list)) => list,
            _ => Vec::new(),
        }
    }

    fn defaults(id: &str) -> Vec<String> {
        match setting(id).and_then(|s| s.places.all().first().map(|p| p.default.clone())) {
            Some(Value::List(list)) => list,
            _ => Vec::new(),
        }
    }

    /// The file holds a line for it that is not its default.
    pub(super) fn changed(&self, id: &str) -> bool {
        let Some(slot) = slot(id) else {
            return false;
        };
        match self.reading(slot) {
            Reading::Absent => false,
            Reading::NotUnderstood(_) => true,
            Reading::Value(Value::List(list)) => {
                let defaults = Self::defaults(id);
                list.len() != defaults.len()
                    || list.iter().zip(&defaults).any(|(a, b)| {
                        match (Chord::parse(a), Chord::parse(b)) {
                            (Some(a), Some(b)) => !a.same(&b),
                            _ => a != b,
                        }
                    })
            }
            Reading::Value(_) => true,
        }
    }

    /// The layouts in force: the file's, else the system's.
    pub(super) fn layouts(&self) -> Vec<String> {
        match slot("keyboard.layouts").and_then(|s| self.value(s)) {
            Some(Value::List(list)) if !list.is_empty() => list,
            _ => self.keys.system.clone(),
        }
    }

    /// What applies without a key, when the system says it.
    pub(super) fn keyboard_default(&self, slot: Slot) -> Option<Value> {
        (slot.setting == "keyboard.layouts" && !self.keys.system.is_empty())
            .then(|| Value::List(self.keys.system.clone()))
    }

    fn option_on(&self, id: &str) -> Option<&'static str> {
        match slot(id).and_then(|s| self.value(s)) {
            Some(Value::Word(word)) => Some(word),
            _ => None,
        }
    }

    /// A row of the keyboard's page that does not apply now, and why.
    pub(super) fn keyboard_not_here(&self, setting: &Setting) -> Option<&'static str> {
        match setting.id {
            "keyboard.switch" if self.layouts().len() < 2 => Some(self.say(w(
                "Une seule disposition : rien à changer.",
                "One layout only: nothing to switch.",
            ))),
            "keyboard.caps" if self.option_on("keyboard.compose") == Some("compose:caps") => {
                Some(self.say(w(
                    "Verr. Maj. sert de touche Compose.",
                    "Caps Lock is the Compose key.",
                )))
            }
            _ => None,
        }
    }

    /// The action holding `chord`, other than `but`.
    fn holder(&self, chord: &Chord, but: &str) -> Option<&'static str> {
        model::shortcuts()
            .filter(|s| s.id != but)
            .find(|s| {
                self.chords(s.id)
                    .iter()
                    .filter_map(|c| Chord::parse(c))
                    .any(|c| c.same(chord))
            })
            .map(|s| s.id)
    }

    fn name_of(&self, id: &str) -> &'static str {
        setting(id).map_or("", |s| s.name.get(self.lang))
    }

    pub(super) fn keys_update(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Registry(registry, system) => {
                tracing::info!(
                    layouts = registry.len(),
                    system = system.len(),
                    "XKB's layouts"
                );
                self.keys.registry = registry;
                self.keys.system = system;
            }
            Msg::Sub(open) => {
                self.stop_recording();
                self.keys.sub = open;
                self.keys.conflict = None;
                self.go(Page::Section(SectionId::Keyboard));
                self.keys.sub = open;
                return iced::widget::operation::snap_to(
                    super::places::PAGE,
                    iced::widget::scrollable::RelativeOffset::START,
                );
            }
            Msg::Query(query) => self.keys.query = query,
            Msg::Fold(prefix) => {
                if !self.keys.unfolded.remove(prefix) {
                    self.keys.unfolded.insert(prefix);
                }
            }
            Msg::Record(id, index) => {
                if !self.writable() {
                    return Task::none();
                }
                self.keys.conflict = None;
                self.keys.recording = Some(Recording {
                    setting: id,
                    index,
                    mods: Modifiers::empty(),
                    refused: None,
                });
                tracing::info!(setting = id, "recording a shortcut");
                return self.inhibit(true);
            }
            Msg::Mods(mods) => {
                if let Some(recording) = &mut self.keys.recording {
                    recording.mods = mods;
                }
            }
            Msg::Replace => {
                let Some(conflict) = self.keys.conflict.take() else {
                    return Task::none();
                };
                let other = conflict.other;
                let chord = Chord::parse(&conflict.chord);
                let left: Vec<String> = self
                    .chords(other)
                    .into_iter()
                    .filter(|c| {
                        !Chord::parse(c)
                            .zip(chord.as_ref())
                            .is_some_and(|(c, n)| c.same(n))
                    })
                    .collect();
                let mine = self.with_chord(conflict.setting, conflict.index, &conflict.chord);
                self.serial += 1;
                let batch = self.serial;
                let said = match self.lang {
                    Lang::French => format!(
                        "{} : {} · « {} » n’en a plus",
                        self.name_of(conflict.setting),
                        keys::shown(&conflict.chord, self.lang),
                        self.name_of(other)
                    ),
                    Lang::English => format!(
                        "{}: {} · “{}” has none left",
                        self.name_of(conflict.setting),
                        keys::shown(&conflict.chord, self.lang),
                        self.name_of(other)
                    ),
                };
                if let (Some(theirs), Some(ours)) = (slot(other), slot(conflict.setting)) {
                    // Its toast is ours: one for both.
                    let other_said = Some(String::new());
                    self.write_batch(theirs, Value::List(left), batch, other_said);
                    self.write_batch(ours, Value::List(mine), batch, Some(said));
                }
            }
            Msg::Cancel => {
                self.keys.conflict = None;
                self.stop_recording();
                return self.inhibit(false);
            }
            Msg::Reset(id) => {
                if let Some(slot) = slot(id) {
                    self.remove(slot, None);
                }
            }
            Msg::ResetAll => {
                self.serial += 1;
                let batch = self.serial;
                let changed: Vec<&'static str> = model::shortcuts()
                    .filter(|s| self.changed(s.id))
                    .map(|s| s.id)
                    .collect();
                for id in changed {
                    if let Some(slot) = slot(id) {
                        self.remove(slot, Some(batch));
                    }
                }
            }
            Msg::Adding(open) => {
                self.keys.adding = open.then(String::new);
                if open {
                    return Task::done(Message::Keys(Msg::FocusAdd));
                }
            }
            Msg::FocusAdd => return iced::widget::operation::focus(ADD_SEARCH),
            Msg::AddQuery(query) => self.keys.adding = Some(query),
            Msg::Pick(code) => {
                self.keys.adding = None;
                let mut list = self.layouts();
                if !list.contains(&code) {
                    list.push(code);
                    return self.set_layouts(list);
                }
            }
            Msg::Move(i, up) => {
                let mut list = self.layouts();
                let j = if up { i.checked_sub(1) } else { Some(i + 1) };
                if let Some(j) = j.filter(|j| *j < list.len()) {
                    list.swap(i, j);
                    return self.set_layouts(list);
                }
            }
            Msg::Remove(i) => {
                let mut list = self.layouts();
                if list.len() > 1 && i < list.len() {
                    list.remove(i);
                    return self.set_layouts(list);
                }
            }
            Msg::Try(text) => self.keys.try_text = text,
        }
        Task::none()
    }

    fn set_layouts(&mut self, list: Vec<String>) -> Task<Message> {
        match slot("keyboard.layouts") {
            Some(slot) => self.change(slot, Value::List(list)),
            None => Task::none(),
        }
    }

    /// `id`'s combinations with `chord` in place of the one at `index`, or
    /// added.
    fn with_chord(&self, id: &str, index: Option<usize>, chord: &str) -> Vec<String> {
        let mut list = self.chords(id);
        match index.filter(|i| *i < list.len()) {
            Some(i) => list[i] = chord.to_owned(),
            None => list.push(chord.to_owned()),
        }
        list
    }

    /// Asks the compositor for every key (`true`) or gives them back,
    /// from the window's connection, on a thread of its own.
    fn inhibit(&mut self, take: bool) -> Task<Message> {
        if let Some(asks) = &self.keys.inhibit {
            if asks.send(take).is_ok() {
                return Task::none();
            }
            self.keys.inhibit = None;
        }
        if !take {
            return Task::none();
        }
        let Some(id) = self.window else {
            return Task::none();
        };
        let (asks, taken) = mpsc::channel();
        let _ = asks.send(true);
        self.keys.inhibit = Some(asks);
        iced::window::run(id, move |window| {
            if let Some(surface) = crate::activate::Surface::of(window) {
                surface.inhibit_shortcuts(taken);
            }
        })
        .map(|()| Message::Nothing)
    }

    pub(super) fn stop_recording(&mut self) {
        if self.keys.recording.take().is_some()
            && let Some(asks) = &self.keys.inhibit
        {
            let _ = asks.send(false);
        }
    }

    /// A key while recording: it is the shortcut, unless refused.
    pub(super) fn record_key(
        &mut self,
        key: &Key,
        physical: &Physical,
        modifiers: Modifiers,
    ) -> Task<Message> {
        let Some(recording) = self.keys.recording.clone() else {
            return Task::none();
        };
        match key.as_ref() {
            Key::Named(Named::Escape) => {
                self.stop_recording();
                return Task::none();
            }
            Key::Named(Named::Backspace) => {
                self.stop_recording();
                let mut list = self.chords(recording.setting);
                if let Some(i) = recording.index.filter(|i| *i < list.len()) {
                    list.remove(i);
                    if let Some(slot) = slot(recording.setting) {
                        return self.change(slot, Value::List(list));
                    }
                }
                return Task::none();
            }
            Key::Named(Named::Shift | Named::Control | Named::Alt | Named::Super | Named::Meta) => {
                if let Some(r) = &mut self.keys.recording {
                    r.mods = modifiers;
                }
                return Task::none();
            }
            _ => {}
        }
        let Some(name) = keys::xkb_name(key, physical) else {
            return Task::none();
        };
        let chord = Chord {
            logo: modifiers.logo(),
            ctrl: modifiers.control(),
            alt: modifiers.alt(),
            shift: modifiers.shift(),
            key: name,
        };
        let lang = self.lang;
        let shown = chord.caps(lang).join(" + ");
        let refuse = |this: &mut Self, why: String| {
            if let Some(r) = &mut this.keys.recording {
                r.refused = Some(why);
            }
            Task::none()
        };
        if chord.fixed() {
            return refuse(
                self,
                match lang {
                    Lang::French => {
                        format!("« {shown} » est fixe : il passe à un autre terminal virtuel.")
                    }
                    Lang::English => {
                        format!("“{shown}” is fixed: it switches to another virtual terminal.")
                    }
                },
            );
        }
        if chord.types() {
            return refuse(
                self,
                match lang {
                    Lang::French => {
                        format!("« {shown} » irait aux applications : ajoute Super, Ctrl ou Alt.")
                    }
                    Lang::English => {
                        format!("“{shown}” would go to applications: add Super, Ctrl or Alt.")
                    }
                },
            );
        }
        let own = self.chords(recording.setting);
        if own.iter().enumerate().any(|(i, c)| {
            Some(i) != recording.index && Chord::parse(c).is_some_and(|c| c.same(&chord))
        }) {
            return refuse(
                self,
                match lang {
                    Lang::French => format!("« {shown} » est déjà là."),
                    Lang::English => format!("“{shown}” is already there."),
                },
            );
        }
        let written = chord.written();
        self.stop_recording();
        // Super+Space while it switches the layouts (grp:win_space_toggle).
        let grp_space = chord.logo
            && !chord.ctrl
            && !chord.alt
            && !chord.shift
            && chord.key.eq_ignore_ascii_case("space")
            && self.option_on("keyboard.switch") == Some("grp:win_space_toggle");
        if let Some(other) = self.holder(&chord, recording.setting) {
            tracing::info!(
                setting = recording.setting,
                other,
                "a shortcut another action holds"
            );
            self.keys.conflict = Some(Conflict {
                setting: recording.setting,
                index: recording.index,
                chord: written,
                other,
            });
            return Task::none();
        }
        if grp_space {
            return refuse(
                self,
                match lang {
                    Lang::French => format!("« {shown} » change de disposition."),
                    Lang::English => format!("“{shown}” switches the layout."),
                },
            );
        }
        let list = self.with_chord(recording.setting, recording.index, &written);
        match slot(recording.setting) {
            Some(slot) => self.change(slot, Value::List(list)),
            None => Task::none(),
        }
    }

    // --- The keyboard's page --------------------------------------------------------

    pub(super) fn keyboard_page<'a>(
        &'a self,
        mut page: iced::widget::Column<'a, Message>,
    ) -> El<'a> {
        if self.keys.sub {
            return self.shortcuts_page();
        }
        let lang = self.lang;
        let controls = self.controls();
        let row_of = |id: &str| setting(id).map(|s| self.row(s, &controls));
        // Dispositions.
        let layouts = self.layouts();
        let mut list = column![];
        for (i, code) in layouts.iter().enumerate() {
            list = list.push(divided(i > 0, self.layout_row(i, code, &layouts)));
        }
        let add = container(
            button(
                row![
                    text("+").size(16).font(BOLD).color(MINT),
                    t(
                        w("Ajouter une disposition", "Add a layout").get(lang),
                        13.5,
                        MINT.get()
                    )
                    .font(BOLD),
                ]
                .spacing(8)
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            .height(46)
            .width(Length::Fill)
            .padding([0, 16])
            .on_press(Message::Keys(Msg::Adding(self.keys.adding.is_none())))
            .style(flat(None, HOVER.get(), 0.0, false)),
        )
        .id("layouts:add".to_owned());
        list = list.push(divided(true, add.into()));
        if let Some(query) = &self.keys.adding {
            list = list.push(self.add_popover(query, &layouts));
        }
        if let Some(switch) = row_of("keyboard.switch") {
            list = list.push(divided(true, switch));
        }
        page = page.push(group(model::LAYOUTS.label.get(lang), card_box(list.into())));
        // Répétition.
        let mut repeat = column![];
        for (i, id) in ["keyboard.repeat-delay", "keyboard.repeat-rate"]
            .iter()
            .enumerate()
        {
            if let Some(r) = row_of(id) {
                repeat = repeat.push(divided(i > 0, r));
            }
        }
        let field = text_input("", &self.keys.try_text)
            .on_input(|s| Message::Keys(Msg::Try(s)))
            .size(12.0)
            .font(MONO)
            .width(288)
            .padding([7, 10])
            .style(super::style::search);
        repeat = repeat.push(divided(
            true,
            container(
                row![
                    t(w("Essayer ici", "Try it here").get(lang), 14.0, TEXT.get())
                        .font(SEMIBOLD)
                        .width(Length::Fill),
                    container(field).style(boxed(
                        crate::theme::ink(rgba(255, 255, 255, 0.06)),
                        8.0,
                        Some((1.0, RIM_12.get()))
                    )),
                ]
                .align_y(Alignment::Center),
            )
            .padding([11, 16])
            .id("keyboard:try".to_owned())
            .into(),
        ));
        page = page.push(group(
            model::REPEAT.label.get(lang),
            card_box(repeat.into()),
        ));
        // Touches spéciales.
        let mut special = column![];
        for (i, id) in ["keyboard.caps", "keyboard.compose", "keyboard.numlock"]
            .iter()
            .enumerate()
        {
            if let Some(r) = row_of(id) {
                special = special.push(divided(i > 0, r));
            }
        }
        page = page.push(group(
            model::SPECIAL.label.get(lang),
            card_box(special.into()),
        ));
        // Raccourcis.
        let changed = model::shortcuts().filter(|s| self.changed(s.id)).count();
        let words = column![
            t(
                w("Raccourcis du bureau", "Desktop shortcuts").get(lang),
                14.0,
                TEXT.get()
            )
            .font(SEMIBOLD),
            t(
                w(
                    "64 raccourcis : fenêtres, bureaux, captures…",
                    "64 shortcuts: windows, desktops, captures…"
                )
                .get(lang),
                12.5,
                DIM.get()
            ),
        ]
        .spacing(2)
        .width(Length::Fill);
        let mut line = row![words].spacing(10).align_y(Alignment::Center);
        if changed > 0 {
            line = line.push(chip(match (lang, changed) {
                (Lang::French, 1) => "1 modifié".to_owned(),
                (Lang::French, n) => format!("{n} modifiés"),
                (Lang::English, n) => format!("{n} changed"),
            }));
        }
        line = line.push(
            container(icon(Icon::Chevron, 14.0, DIM.get()))
                .style(|_: &Theme| container::Style::default()),
        );
        // Lit when another page's link opens it (6.14's).
        let lit = self.lit(crate::desktops::SHORTCUTS);
        let open = container(
            container(
                button(line)
                    .width(Length::Fill)
                    .padding([10, 16])
                    .on_press(Message::Keys(Msg::Sub(true)))
                    .style(flat(None, HOVER.get(), 12.0, false)),
            )
            .style(move |_: &Theme| container::Style {
                background: (lit > 0.0).then_some(iced::Background::Color(
                    crate::theme::Ink::Accent(0.20 * lit).get(),
                )),
                border: border(11.0, 1.5, crate::theme::Ink::Accent(0.85 * lit).get()),
                ..container::Style::default()
            })
            .id(format!("row:{}", crate::desktops::SHORTCUTS)),
        )
        .id("keyboard:shortcuts".to_owned());
        page = page.push(group(
            w("RACCOURCIS", "SHORTCUTS").get(lang),
            card_box(open.into()),
        ));
        container(page).id("page:keyboard".to_owned()).into()
    }

    fn layout_name(&self, code: &str) -> String {
        self.keys
            .registry
            .iter()
            .find(|k| k.code == code)
            .map_or_else(|| code.to_owned(), |k| k.name.clone())
    }

    fn layout_row(&self, i: usize, code: &str, list: &[String]) -> El<'_> {
        let lang = self.lang;
        let alone = list.len() < 2;
        let small = |glyph: &'static str, on: Option<Message>, id: String| -> El<'_> {
            let inert = on.is_none();
            let mut b = button(
                container(
                    text(glyph)
                        .size(13)
                        .font(BOLD)
                        .color(fade(DIM.get(), if inert { 0.3 } else { 1.0 })),
                )
                .center(26),
            )
            .width(26)
            .height(26)
            .padding(0)
            .style(flat(None, HOVER.get(), 6.0, false));
            if let Some(on) = on {
                b = b.on_press(on);
            }
            container(b).id(id).into()
        };
        let line = if i == 0 {
            match lang {
                Lang::French => format!("{code} · active à l’ouverture de la session"),
                Lang::English => format!("{code} · active as the session opens"),
            }
        } else {
            code.to_owned()
        };
        container(
            row![
                text("⋮⋮").size(10).color(FAINT),
                container(
                    text(xkb::chip(code, list))
                        .size(10.5)
                        .font(MONO)
                        .color(BLUE)
                )
                .width(Length::Shrink)
                .padding([3, 8])
                .style(boxed(
                    crate::theme::ink(rgba(143, 179, 255, 0.14)),
                    6.0,
                    None
                )),
                column![
                    t(self.layout_name(code), 14.0, TEXT.get()).font(SEMIBOLD),
                    t(line, 12.5, DIM.get()),
                ]
                .spacing(2)
                .width(Length::Fill),
                small(
                    "↑",
                    (i > 0).then_some(Message::Keys(Msg::Move(i, true))),
                    format!("layout:{i}:up")
                ),
                small(
                    "↓",
                    (i + 1 < list.len()).then_some(Message::Keys(Msg::Move(i, false))),
                    format!("layout:{i}:down")
                ),
                small(
                    "✕",
                    (!alone).then_some(Message::Keys(Msg::Remove(i))),
                    format!("layout:{i}:remove")
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([10, 16])
        .id(format!("layout:{i}"))
        .into()
    }

    /// What the popover's search finds: accents and case aside, across the
    /// name, the XKB id and the layout it is a variant of.
    fn found_layouts(&self, query: &str) -> Vec<&Known> {
        let words: Vec<String> = fold(query)
            .split_whitespace()
            .map(ToOwned::to_owned)
            .collect();
        let mut found: Vec<(u8, &Known)> = self
            .keys
            .registry
            .iter()
            .filter_map(|k| {
                let hay = fold(&format!(
                    "{} {} {}",
                    k.name,
                    k.code,
                    k.of.as_deref().unwrap_or("")
                ));
                if !words.iter().all(|w| hay.contains(w.as_str())) {
                    return None;
                }
                // Its code first, then a code or a name starting so.
                let first = words.first().map_or("", String::as_str);
                let code = fold(&k.code);
                let name = fold(&k.name);
                let rank = if code == first {
                    0
                } else if code.starts_with(first) {
                    1
                } else if name.starts_with(first) || name.contains(&format!(" {first}")) {
                    2
                } else {
                    3
                };
                Some((rank, k))
            })
            .collect();
        found.sort_by_key(|(rank, _)| *rank);
        found.into_iter().map(|(_, k)| k).take(40).collect()
    }

    fn add_popover(&self, query: &str, have: &[String]) -> El<'_> {
        let lang = self.lang;
        let field = text_input(
            w("Français, bépo, us, dvorak…", "French, bepo, us, dvorak…").get(lang),
            query,
        )
        .id(ADD_SEARCH)
        .on_input(|s| Message::Keys(Msg::AddQuery(s)))
        .on_submit(
            match self
                .found_layouts(query)
                .into_iter()
                .find(|k| !have.contains(&k.code))
            {
                Some(k) => Message::Keys(Msg::Pick(k.code.clone())),
                None => Message::Nothing,
            },
        )
        .size(13.0)
        .font(MANROPE)
        .padding([8, 10])
        .style(super::style::search);
        let mut list = column![];
        for (i, known) in self.found_layouts(query).into_iter().enumerate() {
            let there = have.contains(&known.code);
            let ink = if there { 0.55 } else { 1.0 };
            let kind = match (&known.of, lang) {
                (None, Lang::French) => "disposition".to_owned(),
                (None, Lang::English) => "layout".to_owned(),
                (Some(of), Lang::French) => format!("variante · {of}"),
                (Some(of), Lang::English) => format!("variant · {of}"),
            };
            let mut line = row![
                column![
                    t(known.name.clone(), 13.5, fade(TEXT.get(), ink)).font(SEMIBOLD),
                    t(
                        if there {
                            w("déjà là", "already there").get(lang).to_owned()
                        } else {
                            kind
                        },
                        11.5,
                        fade(DIM.get(), ink)
                    ),
                ]
                .width(Length::Fill),
                text(known.code.clone())
                    .size(10.5)
                    .font(MONO)
                    .color(fade(FAINT.get(), ink)),
            ]
            .align_y(Alignment::Center)
            .spacing(10);
            line = line.height(42);
            let mut b = button(line)
                .width(Length::Fill)
                .padding([0, 10])
                .style(flat(None, HOVER.get(), 7.0, false));
            if !there {
                b = b.on_press(Message::Keys(Msg::Pick(known.code.clone())));
            }
            list = list.push(container(b).id(format!("pick:{i}")));
        }
        container(
            column![
                container(field).style(boxed(crate::theme::ink(rgba(255, 255, 255, 0.06)), 9.0, Some((1.0, RIM_12.get())))),
                iced::widget::scrollable(list).height(Length::Shrink).style(super::view::thin),
                t(
                    w(
                        "Les dispositions et variantes de XKB. Entrée ajoute la première, Échap ferme.",
                        "XKB's layouts and variants. Enter adds the first, Esc closes."
                    )
                    .get(lang),
                    11.5,
                    FAINT.get()
                ),
            ]
            .spacing(8),
        )
        .padding(10)
        .max_height(340)
        .style(boxed(MENU.get(), 12.0, Some((1.0, RIM_12.get()))))
        .id("layouts:popover".to_owned())
        .into()
    }

    // --- The shortcuts' sub-page ----------------------------------------------------

    /// The shortcuts the search shows, by group (all without a search).
    pub(super) fn shown_shortcuts(&self) -> Vec<&'static Setting> {
        let query = fold(&self.keys.query);
        let words: Vec<&str> = query.split_whitespace().collect();
        model::shortcuts()
            .filter(|s| {
                if words.is_empty() {
                    return true;
                }
                let mut hay = format!(
                    "{} {} {} {}",
                    s.name.fr,
                    s.name.en,
                    s.id.trim_start_matches("shortcuts."),
                    s.keywords
                        .0
                        .iter()
                        .chain(s.keywords.1)
                        .copied()
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                for chord in self.chords(s.id) {
                    hay.push(' ');
                    hay.push_str(&chord);
                    hay.push(' ');
                    hay.push_str(&keys::shown(&chord, Lang::French));
                    hay.push(' ');
                    hay.push_str(&keys::shown(&chord, Lang::English));
                }
                let hay = fold(&hay);
                words.iter().all(|w| hay.contains(w))
            })
            .collect()
    }

    fn shortcuts_page(&self) -> El<'_> {
        let lang = self.lang;
        let back = button(
            t(
                w("‹ Clavier et raccourcis", "‹ Keyboard and shortcuts").get(lang),
                13.0,
                MINT.get(),
            )
            .font(SEMIBOLD),
        )
        .padding(0)
        .on_press(Message::Keys(Msg::Sub(false)))
        .style(|_, _| button::Style::default());
        let mut page = column![
            container(back).id("shortcuts:back".to_owned()),
            column![
                text(w("Raccourcis", "Shortcuts").get(lang)).size(22).font(BOLD).color(TEXT),
                t(
                    w(
                        "Les touches sont lues sur la première disposition latine, quelle que soit celle qui est active.",
                        "Keys are read on the first Latin layout, whatever the active one.",
                    )
                    .get(lang),
                    13.5,
                    DIM.get()
                ),
            ]
            .spacing(6),
        ]
        .spacing(10)
        .padding(Padding::default().top(14));
        let shown = self.shown_shortcuts();
        let changed = model::shortcuts().filter(|s| self.changed(s.id)).count();
        let count = if self.keys.query.trim().is_empty() {
            match (lang, changed) {
                (Lang::French, 0) => "64 raccourcis".to_owned(),
                (Lang::French, 1) => "64 raccourcis · 1 modifié".to_owned(),
                (Lang::French, n) => format!("64 raccourcis · {n} modifiés"),
                (Lang::English, 0) => "64 shortcuts".to_owned(),
                (Lang::English, n) => format!("64 shortcuts · {n} changed"),
            }
        } else {
            match (lang, shown.len()) {
                (Lang::French, 1) => "1 raccourci".to_owned(),
                (Lang::French, n) => format!("{n} raccourcis"),
                (Lang::English, 1) => "1 shortcut".to_owned(),
                (Lang::English, n) => format!("{n} shortcuts"),
            }
        };
        let field = text_input(
            w(
                "Rechercher une action ou des touches",
                "Search actions or keys",
            )
            .get(lang),
            &self.keys.query,
        )
        .id(SUB_SEARCH)
        .on_input(|s| Message::Keys(Msg::Query(s)))
        .size(13.0)
        .font(MANROPE)
        .padding([8, 10])
        .style(super::style::search);
        let mut reset_all = button(
            t(
                w("Tout rétablir", "Reset all").get(lang),
                12.5,
                fade(TEXT.get(), if changed > 0 { 1.0 } else { INERT }),
            )
            .font(SEMIBOLD),
        )
        .height(32)
        .padding([7, 12])
        .style(flat(
            Some(crate::theme::ink(rgba(255, 255, 255, 0.06))),
            HOVER.get(),
            8.0,
            false,
        ));
        if changed > 0 && self.writable() {
            reset_all = reset_all.on_press(Message::Keys(Msg::ResetAll));
        }
        page = page.push(
            row![
                container(field)
                    .width(Length::Fill)
                    .style(boxed(
                        crate::theme::ink(rgba(255, 255, 255, 0.06)),
                        9.0,
                        Some((1.0, RIM_12.get()))
                    ))
                    .id("shortcuts:search".to_owned()),
                t(count, 12.5, FAINT.get()),
                container(reset_all).id("shortcuts:reset-all".to_owned()),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        );
        let searching = !self.keys.query.trim().is_empty();
        for group_label in model::SHORTCUT_GROUPS {
            let members: Vec<&'static Setting> = shown
                .iter()
                .copied()
                .filter(|s| s.group == group_label)
                .collect();
            if members.is_empty() {
                continue;
            }
            let mut rows = column![];
            let mut seen_families: Vec<&'static str> = Vec::new();
            let mut n = 0;
            for setting in members {
                if let (Some(family), false) = (model::family_of(setting.id), searching) {
                    if seen_families.contains(&family.prefix) {
                        continue;
                    }
                    seen_families.push(family.prefix);
                    rows = rows.push(divided(n > 0, self.family_row(family)));
                    n += 1;
                    if self.keys.unfolded.contains(family.prefix) {
                        for member in
                            model::shortcuts().filter(|s| model::family_of(s.id) == Some(family))
                        {
                            rows = rows.push(divided(
                                true,
                                container(self.shortcut_row(member))
                                    .padding(Padding::default().left(22))
                                    .into(),
                            ));
                        }
                    }
                    continue;
                }
                rows = rows.push(divided(n > 0, self.shortcut_row(setting)));
                n += 1;
            }
            page = page.push(group(group_label.label.get(lang), card_box(rows.into())));
        }
        if !searching {
            page = page.push(self.fixed_group());
        }
        container(page).id("page:shortcuts".to_owned()).into()
    }

    /// A combination's keycaps, as one button.
    fn combination(&self, id: &'static str, index: usize, chord: &str) -> El<'_> {
        let caps = keys::shown(chord, self.lang);
        let mut line = row![].spacing(4).align_y(Alignment::Center);
        for (i, cap) in caps.split(" + ").enumerate() {
            if i > 0 {
                line = line.push(text("+").size(11).color(FAINT));
            }
            line = line.push(keycap(cap.to_owned(), 1.0));
        }
        let mut b =
            button(line)
                .height(30)
                .padding([0, 4])
                .style(flat(None, HOVER.get(), 7.0, false));
        if self.writable() {
            b = b.on_press(Message::Keys(Msg::Record(id, Some(index))));
        }
        container(b).id(format!("chord:{id}:{index}")).into()
    }

    /// The dashed green button while recording.
    fn recording_button(&self, recording: &Recording) -> El<'_> {
        let lang = self.lang;
        let mut held = Vec::new();
        let m = recording.mods;
        for (on, name) in [
            (m.logo(), "Super"),
            (m.control(), "Ctrl"),
            (m.alt(), "Alt"),
            (m.shift(), if lang.fr() { "Maj" } else { "Shift" }),
        ] {
            if on {
                held.push(name);
            }
        }
        let said = if held.is_empty() {
            w("Appuie sur les touches…", "Press the keys…")
                .get(lang)
                .to_owned()
        } else {
            format!("{} + …", held.join(" + "))
        };
        container(t(said, 12.5, MINT.get()).font(SEMIBOLD))
            .width(Length::Shrink)
            .height(30)
            .padding([0, 12])
            .center_y(30)
            .style(boxed(
                crate::theme::Ink::Accent(0.10).get(),
                7.0,
                Some((1.0, crate::theme::Ink::Accent(0.8).get())),
            ))
            .id("recording".to_owned())
            .into()
    }

    fn shortcut_row(&self, setting: &'static Setting) -> El<'_> {
        let lang = self.lang;
        let id = setting.id;
        let changed = self.changed(id);
        let recording = self.keys.recording.as_ref().filter(|r| r.setting == id);
        let mut name = row![].spacing(8).align_y(Alignment::Center);
        if changed {
            name = name.push(container(Space::new().width(6).height(6)).style(boxed(
                ACCENT.get(),
                3.0,
                None,
            )));
        }
        name = name.push(t(setting.name.get(lang), 13.5, TEXT.get()).font(SEMIBOLD));
        let mut words = column![name].spacing(2).width(Length::Fill);
        if let Some(recording) = recording {
            let line = match recording.index {
                Some(_) => w(
                    "Échap annule, Retour arrière enlève ce raccourci.",
                    "Esc cancels, Backspace removes this shortcut.",
                ),
                None => w(
                    "Un raccourci de plus. Échap annule.",
                    "One more shortcut. Esc cancels.",
                ),
            };
            words = words.push(t(line.get(lang), 12.0, DIM.get()));
            if let Some(refused) = &recording.refused {
                words = words.push(t(refused.clone(), 12.0, PEACH.get()));
            }
        } else if changed {
            let defaults = Self::defaults(id);
            let said = if defaults.is_empty() {
                w("aucun", "none").get(lang).to_owned()
            } else {
                defaults
                    .iter()
                    .map(|c| keys::shown(c, lang))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            words = words.push(t(
                match lang {
                    Lang::French => format!("Par défaut : {said}"),
                    Lang::English => format!("Default: {said}"),
                },
                12.0,
                FAINT.get(),
            ));
        }
        let chords = self.chords(id);
        let mut right = row![].spacing(6).align_y(Alignment::Center);
        if chords.is_empty() && recording.is_none() {
            right = right.push(t(w("Aucun", "None").get(lang), 12.5, FAINT.get()));
        }
        for (i, chord) in chords.iter().enumerate() {
            match recording {
                Some(r) if r.index == Some(i) => right = right.push(self.recording_button(r)),
                _ => right = right.push(self.combination(id, i, chord)),
            }
        }
        match recording {
            Some(r) if r.index.is_none() => right = right.push(self.recording_button(r)),
            _ => {
                let mut plus =
                    button(container(text("+").size(14).font(BOLD).color(DIM)).center(26))
                        .width(26)
                        .height(26)
                        .padding(0)
                        .style(flat(None, HOVER.get(), 6.0, false));
                if self.writable() {
                    plus = plus.on_press(Message::Keys(Msg::Record(id, None)));
                }
                right = right.push(container(plus).id(format!("add:{id}")));
            }
        }
        if changed {
            let mut reset =
                button(t(w("↺ Rétablir", "↺ Reset").get(lang), 12.5, MINT.get()).font(SEMIBOLD))
                    .padding([4, 6])
                    .style(flat(None, HOVER.get(), 6.0, false));
            if self.writable() {
                reset = reset.on_press(Message::Keys(Msg::Reset(id)));
            }
            right = right.push(container(reset).id(format!("reset:{id}")));
        }
        let lit = self.lit(id);
        let line = container(row![words, right].spacing(12).align_y(Alignment::Center))
            .padding([8, 16])
            .style(move |_: &Theme| container::Style {
                background: (lit > 0.0).then_some(Background::Color(
                    crate::theme::Ink::Accent(0.20 * lit).get(),
                )),
                border: border(11.0, 1.5, crate::theme::Ink::Accent(0.85 * lit).get()),
                ..container::Style::default()
            })
            .id(format!("row:{id}"));
        let mut out = column![line];
        if let Some(conflict) = self.keys.conflict.as_ref().filter(|c| c.setting == id) {
            out = out.push(self.conflict_box(conflict));
        }
        out.into()
    }

    fn conflict_box(&self, conflict: &Conflict) -> El<'_> {
        let lang = self.lang;
        let shown = keys::shown(&conflict.chord, lang);
        let other = self.name_of(conflict.other);
        let said = match lang {
            Lang::French => format!("{shown} sert déjà à « {other} ». Le remplacer le lui enlève."),
            Lang::English => {
                format!("{shown} is already used by “{other}”. Replacing it takes it away.")
            }
        };
        let amber = crate::theme::ink(rgb(0xF5B44A));
        container(
            row![
                container(t(said, 12.5, TEXT.get()))
                    .width(Length::Fill)
                    .id("conflict:text".to_owned()),
                container(
                    button(t(w("Remplacer", "Replace").get(lang), 12.5, DEEP.get()).font(BOLD))
                        .padding([6, 12])
                        .on_press(Message::Keys(Msg::Replace))
                        .style(move |_, _| button::Style {
                            background: Some(Background::Color(amber)),
                            text_color: DEEP.get(),
                            border: border(8.0, 0.0, Color::TRANSPARENT),
                            ..button::Style::default()
                        }),
                )
                .id("conflict:replace".to_owned()),
                container(
                    button(t(w("Annuler", "Cancel").get(lang), 12.5, TEXT.get()).font(SEMIBOLD))
                        .padding([6, 12])
                        .on_press(Message::Keys(Msg::Cancel))
                        .style(flat(
                            Some(crate::theme::ink(rgba(255, 255, 255, 0.06))),
                            HOVER.get(),
                            8.0,
                            false
                        )),
                )
                .id("conflict:cancel".to_owned()),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([10, 14])
        .style(boxed(
            crate::theme::ink(rgba(245, 180, 74, 0.10)),
            9.0,
            Some((1.0, crate::theme::ink(rgba(245, 180, 74, 0.40)))),
        ))
        .id("conflict".to_owned())
        .into()
    }

    /// What a family's folded row says of its nine: "Super + 1 … 9" while
    /// they keep one pattern, else "Propres à chaque bureau".
    pub(super) fn family_keys(&self, family: &model::Family) -> Option<String> {
        let members: Vec<&'static Setting> = model::shortcuts()
            .filter(|s| model::family_of(s.id) == Some(family))
            .collect();
        let mut pattern: Option<Chord> = None;
        for (n, member) in members.iter().enumerate() {
            let chords = self.chords(member.id);
            let [one] = chords.as_slice() else {
                return None;
            };
            let chord = Chord::parse(one)?;
            if chord.key != (n + 1).to_string() {
                return None;
            }
            match &pattern {
                None => pattern = Some(chord),
                Some(p)
                    if (p.logo, p.ctrl, p.alt, p.shift)
                        == (chord.logo, chord.ctrl, chord.alt, chord.shift) => {}
                Some(_) => return None,
            }
        }
        let mut caps = pattern?.caps(self.lang);
        caps.pop();
        Some(format!("{} + 1 … 9", caps.join(" + ")))
    }

    fn family_row(&self, family: &'static model::Family) -> El<'_> {
        let lang = self.lang;
        let members: Vec<&'static Setting> = model::shortcuts()
            .filter(|s| model::family_of(s.id) == Some(family))
            .collect();
        let changed = members.iter().filter(|s| self.changed(s.id)).count();
        let mut words = column![t(family.name.get(lang), 13.5, TEXT.get()).font(SEMIBOLD)]
            .spacing(2)
            .width(Length::Fill);
        if changed > 0 {
            words = words.push(t(
                match lang {
                    Lang::French => format!("{changed} modifiés sur 9"),
                    Lang::English => format!("{changed} of 9 changed"),
                },
                12.0,
                FAINT.get(),
            ));
        }
        let keys_said: El<'_> = match self.family_keys(family) {
            Some(said) => {
                let mut line = row![].spacing(4).align_y(Alignment::Center);
                for (i, cap) in said.split(" + ").enumerate() {
                    if i > 0 {
                        line = line.push(text("+").size(11).color(FAINT));
                    }
                    line = line.push(keycap(cap.to_owned(), 1.0));
                }
                line.into()
            }
            None => t(
                w("Propres à chaque bureau", "One for each desktop").get(lang),
                12.5,
                DIM.get(),
            )
            .into(),
        };
        let open = self.keys.unfolded.contains(family.prefix);
        let fold = container(
            button(
                t(
                    if open {
                        w("Replier", "Fold")
                    } else {
                        w("Les 9", "All 9")
                    }
                    .get(lang),
                    12.5,
                    MINT.get(),
                )
                .font(SEMIBOLD),
            )
            .padding([4, 8])
            .on_press(Message::Keys(Msg::Fold(family.prefix)))
            .style(flat(None, HOVER.get(), 6.0, false)),
        )
        .id(format!("fold:{}", family.prefix));
        container(
            row![words, keys_said, fold]
                .spacing(12)
                .align_y(Alignment::Center),
        )
        .padding([8, 16])
        .id(format!("row:family:{}", family.prefix))
        .into()
    }

    fn fixed_group(&self) -> El<'_> {
        let lang = self.lang;
        let fixed = [
            (
                w("Déplacer une fenêtre", "Move a window"),
                w("Alt + glisser", "Alt + drag"),
            ),
            (
                w("Redimensionner une fenêtre", "Resize a window"),
                w("Super + clic droit glissé", "Super + right-drag"),
            ),
            (
                w("Changer de terminal virtuel", "Switch virtual terminal"),
                w("Ctrl + Alt + F1 … F12", "Ctrl + Alt + F1 … F12"),
            ),
        ];
        let mut rows = column![];
        for (i, (name, said)) in fixed.iter().enumerate() {
            let mut caps = row![].spacing(4).align_y(Alignment::Center);
            for (j, cap) in said.get(lang).split(" + ").enumerate() {
                if j > 0 {
                    caps = caps.push(text("+").size(11).color(FAINT));
                }
                caps = caps.push(keycap(cap.to_owned(), 0.6));
            }
            rows = rows.push(divided(
                i > 0,
                container(
                    row![
                        t(name.get(lang), 13.5, DIM.get())
                            .font(SEMIBOLD)
                            .width(Length::Fill),
                        caps,
                    ]
                    .align_y(Alignment::Center)
                    .spacing(12),
                )
                .padding([12, 16])
                .into(),
            ));
        }
        let head = row![
            label(w("FIXES", "FIXED").get(lang)),
            Space::new().width(Length::Fill),
            t(
                w("Ne se changent pas", "Cannot be changed").get(lang),
                12.0,
                FAINT.get()
            ),
        ]
        .align_y(Alignment::Center)
        .height(28);
        column![head, card_box(rows.into())].spacing(8).into()
    }

    /// What the pages show, for the log.
    pub(super) fn keys_shown(&self) -> serde_json::Value {
        let lang = self.lang;
        let layouts: Vec<serde_json::Value> = {
            let list = self.layouts();
            list.iter()
                .map(|c| serde_json::json!({ "code": c, "chip": xkb::chip(c, &list), "name": self.layout_name(c) }))
                .collect()
        };
        let shortcuts: Vec<serde_json::Value> = if self.keys.sub {
            let searching = !self.keys.query.trim().is_empty();
            let mut out = Vec::new();
            let mut families = Vec::new();
            for s in self.shown_shortcuts() {
                if let (Some(family), false) = (model::family_of(s.id), searching) {
                    if !families.contains(&family.prefix) {
                        families.push(family.prefix);
                        out.push(serde_json::json!({
                            "id": format!("family:{}", family.prefix),
                            "keys": self.family_keys(family),
                            "unfolded": self.keys.unfolded.contains(family.prefix),
                        }));
                    }
                    if !self.keys.unfolded.contains(family.prefix) {
                        continue;
                    }
                }
                out.push(serde_json::json!({
                    "id": s.id,
                    "keys": self.chords(s.id).iter().map(|c| keys::shown(c, lang)).collect::<Vec<_>>(),
                    "changed": self.changed(s.id),
                }));
            }
            out
        } else {
            Vec::new()
        };
        serde_json::json!({
            "sub": self.keys.sub,
            "layouts": layouts,
            "adding": self.keys.adding,
            "found": self.keys.adding.as_deref().map(|q| self.found_layouts(q).iter().map(|k| k.code.clone()).collect::<Vec<_>>()),
            "query": self.keys.query,
            "shortcuts": shortcuts,
            "recording": self.keys.recording.as_ref().map(|r| serde_json::json!({
                "setting": r.setting,
                "index": r.index,
                "refused": r.refused,
            })),
            "conflict": self.keys.conflict.as_ref().map(|c| serde_json::json!({
                "setting": c.setting,
                "chord": c.chord,
                "other": c.other,
            })),
            "changed": model::shortcuts().filter(|s| self.changed(s.id)).count(),
        })
    }
}

fn group<'a>(name: &'static str, body: El<'a>) -> El<'a> {
    column![
        row![label(name), Space::new().width(Length::Fill)]
            .align_y(Alignment::Center)
            .height(28),
        body
    ]
    .spacing(8)
    .into()
}

fn chip<'a>(said: String) -> El<'a> {
    container(text(said).size(10.5).font(BOLD).color(MINT))
        .padding([1, 6])
        .style(boxed(crate::theme::Ink::Accent(0.16).get(), 5.0, None))
        .into()
}

/// One key: 22 high, a 1 px rim and a 2 px bottom.
fn keycap<'a>(said: String, alpha: f32) -> El<'a> {
    container(
        text(said)
            .size(11)
            .font(MONO)
            .color(fade(TEXT.get(), alpha)),
    )
    .height(22)
    .padding([0, 7])
    .center_y(22)
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(fade(
            crate::theme::ink(rgba(255, 255, 255, 0.06)),
            alpha,
        ))),
        border: border(
            5.0,
            1.0,
            fade(crate::theme::ink(rgba(255, 255, 255, 0.14)), alpha),
        ),
        shadow: iced::Shadow {
            color: fade(crate::theme::ink(rgba(0, 0, 0, 0.45)), alpha),
            offset: iced::Vector::new(0.0, 2.0),
            blur_radius: 0.0,
        },
        ..container::Style::default()
    })
    .into()
}

/// Whether `kind` is one this module draws itself.
pub fn own_kind(kind: &Kind) -> bool {
    matches!(kind, Kind::Shortcut | Kind::Layouts)
}
