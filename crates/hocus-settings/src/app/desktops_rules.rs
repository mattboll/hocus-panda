//! "RÈGLES PAR APPLICATION" on the Bureaux et fenêtres page (6.14): a row
//! per rule (`[[windows.rules]]`, in the file's order), its application's
//! tile, name and what it does, "Modifier" opening its three rows (its
//! desktop, its size as it opens, above the others), a ✕ taking it out;
//! then "+ Ajouter une règle", the applications installed without one,
//! those with a window open first. The rules are written whole, as an
//! array of tables, each change with its toast and "Annuler".

use hocus_words::settings::app_desktops_rules as words;
use std::time::Instant;

use iced::{
    Alignment, Background, Length, Padding, Shadow, Theme,
    widget::{Space, button, column, container, mouse_area, pin, row, stack},
};

use super::{
    App, Doing, KeysSaid, Message,
    desktops_page::{Msg, Stop},
    screens::{menu_button, switch},
    style::*,
    view::{El, icon, t},
};
use crate::{
    desktops::{self as model, Rule},
    lang::w,
    model::Icon,
    store::KeyEdit,
};

/// A menu of the rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleMenu {
    /// Rule `i`'s desktop.
    Desktop(usize),
    /// Rule `i`'s size as it opens.
    Size(usize),
    /// "+ Ajouter une règle".
    Add,
}

impl RuleMenu {
    fn id(self) -> String {
        match self {
            Self::Desktop(i) => format!("rule:{i}:desktop"),
            Self::Size(i) => format!("rule:{i}:size"),
            Self::Add => "rules:add".to_owned(),
        }
    }
}

/// The rules' state on the page.
#[derive(Debug, Default)]
pub struct RulesState {
    /// The rule open, by its application's id.
    open: Option<String>,
    /// A menu open, since then.
    menu: Option<(RuleMenu, Instant)>,
    /// The applications with a window open, as the compositor said them.
    open_apps: Vec<String>,
}

/// An application's tile's colour, the same for an id each time.
fn tile_colour(id: &str) -> iced::Color {
    const COLOURS: [u32; 6] = [0x6F9BFF, 0x6CC287, 0xF5C46E, 0xE58AC8, 0x8FD3D9, 0xF2945B];
    let sum = id
        .bytes()
        .fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
    rgb(COLOURS[(sum % COLOURS.len() as u32) as usize])
}

impl App {
    fn rules(&self) -> Vec<Rule> {
        self.snapshot
            .as_ref()
            .map(|s| s.rules.0.clone())
            .unwrap_or_default()
    }

    /// An application's name: its entry's, else its id.
    fn app_name(id: &str) -> String {
        crate::apps::named(id).map_or_else(|| id.to_owned(), |a| a.name.clone())
    }

    /// The rows' controls, for the keyboard.
    pub(super) fn rules_controls(&self) -> Vec<Stop> {
        let mut out = Vec::new();
        for (i, rule) in self.rules().iter().enumerate() {
            out.push(Stop::RuleEdit(i));
            out.push(Stop::RuleRemove(i));
            if self.desktops.rules.open.as_deref() == Some(rule.app_id.as_str()) {
                out.push(Stop::RuleMenu(RuleMenu::Desktop(i)));
                out.push(Stop::RuleMenu(RuleMenu::Size(i)));
                out.push(Stop::RuleAbove(i));
            }
        }
        out.push(Stop::RuleMenu(RuleMenu::Add));
        out
    }

    /// The choices of `menu`: what each says.
    fn rule_choices(&self, menu: RuleMenu) -> Vec<String> {
        let lang = self.lang;
        match menu {
            RuleMenu::Desktop(_) => {
                let names = self.desktop_names_now();
                let mut out = vec![w(words::WHERE_YOU_OPEN_IT).get(lang).to_owned()];
                for i in 0..self.desktop_count_now() {
                    out.push(format!("{} · {}", i + 1, model::name_of(&names, i, lang)));
                }
                out
            }
            RuleMenu::Size(_) => model::SIZES
                .iter()
                .map(|(_, said)| said.get(lang).to_owned())
                .collect(),
            RuleMenu::Add => self
                .addable()
                .into_iter()
                .map(|(id, name)| if name == id { id } else { name })
                .collect(),
        }
    }

    /// The applications a rule can be added for: installed, without one,
    /// those with a window open first; open ones without an entry by id.
    fn addable(&self) -> Vec<(String, String)> {
        let rules = self.rules();
        let has = |id: &str| rules.iter().any(|r| r.app_id.eq_ignore_ascii_case(id));
        let open = &self.desktops.rules.open_apps;
        let is_open = |id: &str| open.iter().any(|o| o.eq_ignore_ascii_case(id));
        let mut first: Vec<(String, String)> = Vec::new();
        let mut rest: Vec<(String, String)> = Vec::new();
        for app in crate::apps::list() {
            if has(&app.id) {
                continue;
            }
            let item = (app.id.clone(), app.name.clone());
            if is_open(&app.id) {
                first.push(item);
            } else {
                rest.push(item);
            }
        }
        for id in open {
            // The Settings' own window is not one to give a rule.
            if id == "hocus-settings" {
                continue;
            }
            if !has(id) && crate::apps::named(id).is_none() {
                first.push((id.clone(), id.clone()));
            }
        }
        first.extend(rest);
        first
    }

    /// The choice `menu` shows chosen.
    fn rule_chosen(&self, menu: RuleMenu) -> Option<usize> {
        let rules = self.rules();
        match menu {
            RuleMenu::Desktop(i) => {
                let d = rules.get(i)?.desktop;
                Some(d.and_then(|d| usize::try_from(d).ok()).unwrap_or(0))
            }
            RuleMenu::Size(i) => {
                let size = rules
                    .get(i)?
                    .size
                    .clone()
                    .unwrap_or_else(|| "last".to_owned());
                model::SIZES.iter().position(|(word, _)| *word == size)
            }
            RuleMenu::Add => None,
        }
    }

    // --- What changes them ------------------------------------------------------------

    pub(super) fn rules_update(&mut self, msg: Msg) -> iced::Task<Message> {
        match msg {
            Msg::RuleEdit(i) => {
                let id = self.rules().get(i).map(|r| r.app_id.clone());
                self.desktops.rules.open = if self.desktops.rules.open == id {
                    None
                } else {
                    id
                };
                self.desktops.rules.menu = None;
            }
            Msg::RuleRemove(i) => self.rule_remove(i),
            Msg::RuleMenu(menu) => {
                let same = self.desktops.rules.menu.is_some_and(|(m, _)| m == menu);
                self.desktops.rules.menu =
                    (!same && self.writable()).then(|| (menu, Instant::now()));
                // Which applications have a window open: they come first.
                if menu == RuleMenu::Add && !same {
                    self.screens_ask("windows-apps".to_owned());
                }
            }
            Msg::RuleChoose(menu, choice) => {
                self.desktops.rules.menu = None;
                self.rule_choose(menu, choice);
            }
            Msg::RuleAbove(i) => {
                let mut rules = self.rules();
                if let Some(rule) = rules.get_mut(i) {
                    rule.above = !rule.above;
                    self.rules_write(rules, i);
                }
            }
            Msg::CloseRuleMenu => self.desktops.rules.menu = None,
            _ => {}
        }
        iced::Task::none()
    }

    /// `Esc` with a menu of the rules open: it closes.
    pub(super) fn rules_escape(&mut self) -> bool {
        self.desktops.rules.menu.take().is_some()
    }

    pub(super) fn rules_apps(&mut self, apps: Vec<String>) {
        self.desktops.rules.open_apps = apps;
    }

    fn rule_choose(&mut self, menu: RuleMenu, choice: usize) {
        let mut rules = self.rules();
        match menu {
            RuleMenu::Desktop(i) => {
                if let Some(rule) = rules.get_mut(i) {
                    rule.desktop = (choice > 0).then(|| i64::try_from(choice).unwrap_or(1));
                    self.rules_write(rules, i);
                }
            }
            RuleMenu::Size(i) => {
                if let (Some(rule), Some((word, _))) = (rules.get_mut(i), model::SIZES.get(choice))
                {
                    rule.size = (*word != "last").then(|| (*word).to_owned());
                    self.rules_write(rules, i);
                }
            }
            RuleMenu::Add => {
                let Some((id, name)) = self.addable().into_iter().nth(choice) else {
                    return;
                };
                rules.push(Rule {
                    app_id: id.clone(),
                    ..Rule::default()
                });
                let lang = self.lang;
                let said = KeysSaid {
                    done: words::rule_added(lang, &name),
                    undone: words::undone_s_rule_is_taken_out(lang, &name),
                    redone: words::redone_s_rule(lang, &name),
                    row: model::RULES,
                };
                tracing::info!(app = id, "a rule added");
                self.desktops.rules.open = Some(id);
                self.rules_keys(&rules, said);
            }
        }
    }

    fn rule_remove(&mut self, i: usize) {
        let mut rules = self.rules();
        if i >= rules.len() {
            return;
        }
        let gone = rules.remove(i);
        let name = Self::app_name(&gone.app_id);
        let lang = self.lang;
        let said = KeysSaid {
            done: words::s_rule_removed(lang, &name),
            undone: words::undone_s_rule_is_back(lang, &name),
            redone: words::redone_s_rule_removed(lang, &name),
            row: model::RULES,
        };
        if self.desktops.rules.open.as_deref() == Some(gone.app_id.as_str()) {
            self.desktops.rules.open = None;
        }
        self.rules_keys(&rules, said);
    }

    /// Rule `i` of `rules` changed: written, its toast saying what it does.
    fn rules_write(&mut self, rules: Vec<Rule>, i: usize) {
        let lang = self.lang;
        let names = self.desktop_names_now();
        let before = self.rules();
        let (Some(now), Some(then)) = (rules.get(i), before.get(i)) else {
            return;
        };
        let name = Self::app_name(&now.app_id);
        let said_now = model::rule_said(now, &names, lang);
        let said_then = model::rule_said(then, &names, lang);
        let said = KeysSaid {
            done: words::s_rule(lang, &name, &said_now),
            undone: words::undone(lang, &said_then),
            redone: words::redone(lang, &said_now),
            row: model::RULES,
        };
        self.rules_keys(&rules, said);
    }

    fn rules_keys(&mut self, rules: &[Rule], said: KeysSaid) {
        self.ask_keys(
            vec![KeyEdit {
                key: vec!["windows".into(), "rules".into()],
                value: model::rules_toml(rules),
            }],
            said,
            Doing::New,
            None,
        );
    }

    pub(super) fn rules_key(&mut self, stop: &Stop) -> iced::Task<Message> {
        let msg = match stop {
            Stop::RuleEdit(i) => Msg::RuleEdit(*i),
            Stop::RuleRemove(i) => Msg::RuleRemove(*i),
            Stop::RuleMenu(menu) => Msg::RuleMenu(*menu),
            Stop::RuleAbove(i) => Msg::RuleAbove(*i),
            _ => return iced::Task::none(),
        };
        self.rules_update(msg)
    }

    // --- What it draws ----------------------------------------------------------------

    fn rules_focused(&self, stop: &Stop) -> bool {
        matches!(self.focus, super::Focus::Control(i)
            if self.controls().get(i) == Some(&super::Control::Desktops(stop.clone())))
    }

    /// The group's rows.
    pub(super) fn rules_rows(&self) -> Vec<El<'_>> {
        let rules = self.rules();
        let names = self.desktop_names_now();
        let mut out: Vec<El<'_>> = Vec::new();
        for (i, rule) in rules.iter().enumerate() {
            let open = self.desktops.rules.open.as_deref() == Some(rule.app_id.as_str());
            out.push(self.rule_row(i, rule, &names, open));
            if open {
                out.push(self.rule_open(i, rule));
            }
        }
        out.push(self.rule_add_row());
        out
    }

    fn rule_row<'a>(&'a self, i: usize, rule: &Rule, names: &[String], open: bool) -> El<'a> {
        let lang = self.lang;
        let inert = !self.writable();
        let name = Self::app_name(&rule.app_id);
        let initial: String = name
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        let colour = tile_colour(&rule.app_id);
        let tile = container(t(initial, 13.0, rgb(0x0b1020)).font(BOLD))
            .width(28)
            .height(28)
            .center(28)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(colour)),
                border: border(7.0, 0.0, colour),
                ..container::Style::default()
            });
        let words = column![
            t(name.clone(), 14.0, TEXT.get()).font(SEMIBOLD),
            t(model::rule_said(rule, names, lang), 12.5, DIM.get()),
        ]
        .spacing(2)
        .width(Length::Fill);
        let edit_focused = self.rules_focused(&Stop::RuleEdit(i));
        let mut edit = button(
            t(
                if open { w(words::FOLD) } else { w(words::EDIT) }
                    .get(lang)
                    .to_owned(),
                13.0,
                TEXT.get(),
            )
            .font(SEMIBOLD),
        )
        .height(30)
        .padding([5, 12])
        .style(move |_, status| button::Style {
            background: Some(Background::Color(crate::theme::ink(rgba(
                255,
                255,
                255,
                if matches!(status, button::Status::Hovered) {
                    0.10
                } else {
                    0.06
                },
            )))),
            text_color: TEXT.get(),
            border: if edit_focused {
                ring(8.0)
            } else {
                border(8.0, 1.0, RIM_12.get())
            },
            ..button::Style::default()
        });
        edit = edit.on_press(Message::Desktops(Msg::RuleEdit(i)));
        let remove_focused = self.rules_focused(&Stop::RuleRemove(i));
        let mut remove = button(container(icon(Icon::Close, 12.0, DIM.get())).center(26))
            .width(28)
            .height(28)
            .padding(0)
            .style(flat(None, HOVER.get(), 7.0, remove_focused));
        if !inert {
            remove = remove.on_press(Message::Desktops(Msg::RuleRemove(i)));
        }
        container(
            row![
                tile,
                words,
                container(edit).id(format!("rule:{i}:edit")),
                container(remove).id(format!("rule:{i}:remove")),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding(Padding {
            top: 9.0,
            right: 14.0,
            bottom: 9.0,
            left: 16.0,
        })
        .id(format!("rule:{i}"))
        .into()
    }

    /// An open rule's three rows, 58 px in.
    fn rule_open<'a>(&'a self, i: usize, rule: &Rule) -> El<'a> {
        let lang = self.lang;
        let inert = !self.writable();
        let menu_row = |label: &'static str, menu: RuleMenu| -> El<'a> {
            let chosen = self.rule_chosen(menu).unwrap_or(0);
            let said = self
                .rule_choices(menu)
                .get(chosen)
                .cloned()
                .unwrap_or_default();
            let open = self.desktops.rules.menu.is_some_and(|(m, _)| m == menu);
            let focused = self.rules_focused(&Stop::RuleMenu(menu));
            row![
                t(label, 13.5, TEXT.get()).width(Length::Fill),
                container(menu_button(
                    said,
                    inert,
                    focused,
                    open,
                    Some(Message::Desktops(Msg::RuleMenu(menu))),
                ))
                .id(menu.id()),
            ]
            .align_y(Alignment::Center)
            .height(48)
            .into()
        };
        let above_focused = self.rules_focused(&Stop::RuleAbove(i));
        column![
            menu_row(w(words::DESKTOP).get(lang), RuleMenu::Desktop(i)),
            menu_row(w(words::WHEN_IT_OPENS).get(lang), RuleMenu::Size(i)),
            row![
                column![
                    t(w(words::ABOVE_THE_OTHERS).get(lang), 13.5, TEXT.get()),
                    t(
                        w(words::EVEN_WITHOUT_THE_FOCUS_A_FULL).get(lang),
                        12.0,
                        DIM.get()
                    ),
                ]
                .spacing(1)
                .width(Length::Fill),
                container(switch(
                    rule.above,
                    inert,
                    above_focused,
                    1.0,
                    Some(Message::Desktops(Msg::RuleAbove(i))),
                ))
                .id(format!("rule:{i}:above")),
            ]
            .align_y(Alignment::Center)
            .height(48),
        ]
        .padding(Padding {
            top: 0.0,
            right: 14.0,
            bottom: 6.0,
            left: 58.0,
        })
        .into()
    }

    /// "Une autre application", "+ Ajouter une règle".
    fn rule_add_row(&self) -> El<'_> {
        let lang = self.lang;
        let open = self
            .desktops
            .rules
            .menu
            .is_some_and(|(m, _)| m == RuleMenu::Add);
        let focused = self.rules_focused(&Stop::RuleMenu(RuleMenu::Add));
        let words = column![
            t(w(words::ANOTHER_APPLICATION).get(lang), 14.0, TEXT.get()).font(SEMIBOLD),
            t(
                w(words::ALWAYS_ON_A_DESKTOP_AT_A).get(lang),
                12.5,
                DIM.get()
            ),
        ]
        .spacing(2)
        .width(Length::Fill);
        self.desktops_row_frame(
            model::RULES,
            row![
                words,
                container(menu_button(
                    w(words::ADD_A_RULE).get(lang).to_owned(),
                    !self.writable(),
                    focused,
                    open,
                    Some(Message::Desktops(Msg::RuleMenu(RuleMenu::Add))),
                ))
                .id(RuleMenu::Add.id()),
            ]
            .spacing(16)
            .align_y(Alignment::Center)
            .into(),
        )
    }

    /// The open menu of the rules, under its button.
    pub(super) fn rules_menu_list(&self) -> Option<El<'_>> {
        let (menu, since) = self.desktops.rules.menu?;
        let at = *self.places.at.get(&menu.id())?;
        let opened = if self.reduced_motion {
            1.0
        } else {
            super::view::ease_out(self.now.saturating_duration_since(since), super::MENU_IN)
        };
        let choices = self.rule_choices(menu);
        let chosen = self.rule_chosen(menu);
        let width = at.width.max(220.0);
        let mut list = column![];
        let empty = choices.is_empty();
        for (i, label) in choices.iter().enumerate() {
            let is = chosen == Some(i);
            let check: El<'_> = if is {
                icon(Icon::Check, 13.0, fade(ACCENT.get(), opened))
            } else {
                Space::new().width(13).into()
            };
            list = list.push(
                container(
                    button(
                        row![
                            container(check).width(18).center_x(18),
                            t(label.clone(), 13.5, fade(TEXT.get(), opened)).font(if is {
                                SEMIBOLD
                            } else {
                                MANROPE
                            }),
                        ]
                        .spacing(6)
                        .height(Length::Fill)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .height(32)
                    .padding([0, 8])
                    .on_press(Message::Desktops(Msg::RuleChoose(menu, i)))
                    .style(flat(
                        None,
                        crate::theme::ink(rgba(255, 255, 255, 0.07)),
                        7.0,
                        false,
                    )),
                )
                .id(format!("option:{i}")),
            );
        }
        if empty {
            list = list.push(
                container(t(
                    w(words::EVERY_INSTALLED_APPLICATION_HAS_ITS_RULE).get(self.lang),
                    13.0,
                    FAINT.get(),
                ))
                .padding([8, 10]),
            );
        }
        // Long lists scroll: at most ten rows show.
        let rows = choices.len().clamp(1, 10) as f32;
        let height = 10.0 + 32.0 * rows;
        let panel = container(iced::widget::scrollable(list).height(height - 10.0))
            .width(width)
            .padding(5)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(fade(MENU.get(), opened))),
                border: border(10.0, 1.0, fade(RIM_12.get(), opened)),
                shadow: Shadow {
                    color: rgba(0, 0, 0, 0.45 * opened),
                    ..shadow()
                },
                ..container::Style::default()
            })
            .id("menu".to_owned());
        let x = (at.x + at.width - width).max(0.0);
        let bottom = self
            .places
            .at
            .get(&format!("sidebar:{}", words::EN.get(self.lang)))
            .map_or(f32::MAX, |s| s.y + s.height);
        let under = at.y + 38.0;
        let y = if under + height > bottom - 8.0 {
            (at.y - 6.0 - height).max(8.0) + 4.0 * (1.0 - opened)
        } else {
            under - 4.0 * (1.0 - opened)
        };
        Some(
            stack![
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .on_press(Message::Desktops(Msg::CloseRuleMenu)),
                pin(panel).x(x).y(y),
            ]
            .into(),
        )
    }

    pub(super) fn rules_moving(&self, now: Instant) -> bool {
        !self.reduced_motion
            && self
                .desktops
                .rules
                .menu
                .is_some_and(|(_, since)| now.saturating_duration_since(since) < super::MENU_IN)
    }

    /// What the rules show, for the log.
    pub(super) fn rules_shown(&self) -> serde_json::Value {
        let names = self.desktop_names_now();
        let rules: Vec<serde_json::Value> = self
            .rules()
            .iter()
            .map(|r| {
                serde_json::json!({
                    "app": r.app_id,
                    "name": Self::app_name(&r.app_id),
                    "said": model::rule_said(r, &names, self.lang),
                })
            })
            .collect();
        serde_json::json!({
            "rules": rules,
            "open": self.desktops.rules.open,
            "menu": self.desktops.rules.menu.map(|(m, _)| m.id()),
            "choices": self.desktops.rules.menu.map(|(m, _)| self.rule_choices(m)),
        })
    }
}
