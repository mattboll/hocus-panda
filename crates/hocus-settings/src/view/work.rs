//! The Work pages (#67): Desktops, Windows, Mouse and touchpad,
//! Screenshots and recording, Clipboard and emoji. No board draws them:
//! they keep board R2's template, captioned sections of choices and switch
//! cards, with fields written with Enter.

use std::collections::HashMap;

use hocus_core::{
    Action, Scope, ScreenshotMode,
    config::{Rule, Trigger},
};

use super::*;

/// A text field of the Work pages: what is typed is written with Enter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Field {
    ScreenshotFolder,
    RecordingFolder,
    /// The app of a rule to add.
    NewRule,
    /// A desktop's name, by its screen and its place there (from 0).
    DesktopName {
        output: String,
        index: usize,
    },
    /// The account's full name, written through AccountsService.
    RealName,
    /// The account's picture's file, the same.
    Picture,
    /// The AI provider's address and model.
    AiUrl,
    AiModel,
    /// The AI provider's key: for the keyring, never shown back.
    AiKey,
    /// The socket of a herdr to add.
    NewHerdr,
    /// A folder for the document search to read, and a name for it to
    /// leave out.
    NewSearchFolder,
    NewExclude,
    /// The model making the documents' vectors.
    EmbeddingModel,
    /// The web search's address.
    SearchEngine,
    /// An application whose windows agents act in once confirmed.
    NewConfirmApp,
}

/// The Work pages' fields between builds.
#[derive(Debug, Default)]
pub(super) struct Fields {
    /// As built last.
    pub(super) nodes: Vec<(Field, NodeId)>,
    /// What was typed in them, until the file has it.
    drafts: HashMap<Field, String>,
}

impl Fields {
    pub(super) fn field_of(&self, id: Option<NodeId>) -> Option<Field> {
        let id = id?;
        self.nodes
            .iter()
            .find(|(_, n)| *n == id)
            .map(|(f, _)| f.clone())
    }

    pub(super) fn node(&self, field: &Field) -> Option<NodeId> {
        self.nodes.iter().find(|(f, _)| f == field).map(|(_, n)| *n)
    }

    pub(super) fn type_into(&mut self, field: Field, text: String) {
        self.drafts.insert(field, text);
    }

    pub(super) fn forget(&mut self, field: &Field) {
        self.drafts.remove(field);
    }

    /// Off the page, or the file changed: the fields show the file again.
    pub(super) fn leave(&mut self) {
        self.drafts.clear();
    }
}

/// `[agents] confirm_apps` as `apps`.
pub(super) fn confirm_apps_change(apps: &[String]) -> Change {
    let list: toml_edit::Array = apps.iter().map(String::as_str).collect();
    Change::new("agents", "confirm_apps", list)
}

/// Gaps offered, logical pixels.
const GAPS: [u32; 6] = [0, 4, 8, 12, 16, 24];
/// Window corners offered, logical pixels.
const RADII: [u32; 5] = [0, 6, 12, 18, 24];
const FPS: [u32; 3] = [15, 30, 60];
const COUNTDOWNS: [u32; 4] = [0, 3, 5, 10];
const ENTRIES: [u32; 5] = [25, 50, 100, 250, 500];

/// A rule's name: its app pattern, else its title's, else any app.
fn rule_pattern(rule: &Rule, lang: Lang) -> String {
    rule.app_id
        .clone()
        .or_else(|| rule.title.clone())
        .unwrap_or_else(|| words::ANY_APP.get(lang).to_owned())
}

impl SettingsView {
    /// A field of a Work page, showing what was typed or the file's value.
    pub(super) fn text_field(
        &mut self,
        theme: &Theme,
        parent: NodeId,
        field: Field,
        (name, hint): (&str, &str),
        value: &str,
    ) -> NodeId {
        let boxed = self.add(
            parent,
            Style {
                height: Length::Px(44.0),
                padding: Insets::symmetric(0.0, 14.0),
                align: Align::Center,
                ..fill_width(row(0.0))
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                border: Some(Paint::Border),
                radius: 12.0,
                ..Look::default()
            }),
        );
        let node = self.add(
            boxed,
            Style {
                width: Length::Fill,
                height: Length::Px(28.0),
                ..Style::default()
            },
            Kind::TextField(Box::new(if field == Field::AiKey {
                TextField::new(name, hint, theme.text.body).secret()
            } else {
                TextField::new(name, hint, theme.text.body)
            })),
        );
        let shown = self
            .fields
            .drafts
            .get(&field)
            .cloned()
            .unwrap_or_else(|| value.to_owned());
        self.tree.set_field_value(node, &shown);
        self.fields.nodes.push((field, node));
        boxed
    }

    /// What was typed in a field, written to the file; empty, the key goes
    /// (a new rule needs an app).
    pub(super) fn commit_field(&mut self, field: Field) -> Vec<Outcome> {
        let typed = self
            .fields
            .drafts
            .get(&field)
            .cloned()
            .or_else(|| {
                let node = self.fields.node(&field)?;
                self.tree.field_value(node).map(str::to_owned)
            })
            .unwrap_or_default();
        let typed = typed.trim().to_owned();
        self.dirty = true;
        self.tree.set_focus(None);
        let value = (!typed.is_empty()).then(|| toml_edit::Value::from(typed.as_str()));
        let change = match &field {
            // Not the file's: AccountsService's, which says it back.
            // What was typed stays until it says it back.
            Field::RealName | Field::Picture => {
                self.system.refused = None;
                if typed.is_empty() {
                    self.fields.forget(&field);
                    return Vec::new();
                }
                self.fields.type_into(field.clone(), typed.clone());
                return vec![if field == Field::RealName {
                    Outcome::SetRealName(typed)
                } else {
                    Outcome::SetPicture(typed)
                }];
            }
            // The key goes to the keyring: nothing typed is kept here.
            Field::AiKey => {
                self.fields.forget(&field);
                if typed.is_empty() {
                    return Vec::new();
                }
                let provider = self.ctx.config.ai.provider.clone();
                return vec![Outcome::AiKey {
                    provider,
                    key: typed,
                }];
            }
            Field::AiUrl => {
                let ai = hocus_core::config::Ai {
                    url: typed.clone(),
                    ..Default::default()
                };
                // In the clear to another machine: not written, said why.
                self.system.bad_address = !ai.url_is_safe();
                if self.system.bad_address {
                    self.fields.type_into(field.clone(), typed);
                    return Vec::new();
                }
                Change {
                    value,
                    ..Change::new("ai", "url", "")
                }
            }
            Field::AiModel => Change {
                value,
                ..Change::new("ai", "model", "")
            },
            Field::NewSearchFolder | Field::NewExclude => {
                return self.commit_list_entry(&field, &typed);
            }
            Field::SearchEngine => return self.commit_engine(&typed),
            Field::EmbeddingModel => Change {
                value,
                ..Change::new("search", "embedding_model", "")
            },
            Field::NewHerdr => {
                self.fields.forget(&field);
                if value.is_none() {
                    return Vec::new();
                }
                let index = self.ctx.config.agents.herdr.len();
                return vec![Outcome::Change(Box::new(Change::herdr(
                    index, "socket", value,
                )))];
            }
            Field::NewConfirmApp => {
                self.fields.forget(&field);
                let mut apps = self.ctx.config.agents.confirm_apps.clone();
                if typed.is_empty() || apps.contains(&typed) {
                    return Vec::new();
                }
                apps.push(typed);
                return vec![Outcome::Change(Box::new(confirm_apps_change(&apps)))];
            }
            Field::ScreenshotFolder => Change {
                value,
                ..Change::new("screenshots", "folder", "")
            },
            Field::RecordingFolder => Change {
                value,
                ..Change::new("recording", "folder", "")
            },
            Field::NewRule => {
                self.fields.forget(&field);
                if value.is_none() {
                    return Vec::new();
                }
                let index = self.ctx.config.rules.len();
                return vec![Outcome::Change(Box::new(Change::rule(
                    index, "app_id", value,
                )))];
            }
            Field::DesktopName { output, index } => {
                let mut names = self
                    .ctx
                    .config
                    .output(output)
                    .map(|o| o.desktop_names.clone())
                    .unwrap_or_default();
                if names.len() <= *index {
                    names.resize(index + 1, String::new());
                }
                names[*index] = typed.clone();
                while names.last().is_some_and(String::is_empty) {
                    names.pop();
                }
                if names.is_empty() {
                    Change::output_unset(output, "desktop_names")
                } else {
                    let list: toml_edit::Array = names.iter().map(String::as_str).collect();
                    Change::output(output, "desktop_names", list)
                }
            }
        };
        // The typed text stays until the file has it.
        self.fields.type_into(field, typed);
        vec![Outcome::Change(Box::new(change))]
    }

    /// A name, then choices: beside it when the window is wide, under it
    /// otherwise, so that they fit. Each is named "Name: choice" for
    /// screen readers; the choices' buttons.
    pub(super) fn named_choices(
        &mut self,
        theme: &Theme,
        parent: NodeId,
        name: words::Words,
        choices: Vec<(String, bool, Target)>,
    ) -> Vec<NodeId> {
        let name = name.get(self.lang);
        self.choices_named(theme, parent, name, choices)
    }

    /// The same, its name said as it comes (an application's).
    pub(super) fn choices_named(
        &mut self,
        theme: &Theme,
        parent: NodeId,
        name: &str,
        choices: Vec<(String, bool, Target)>,
    ) -> Vec<NodeId> {
        let lang = self.lang;
        let wide = self.tree.size().w >= 1180.0;
        let line = if wide {
            let line = self.add(parent, fill_width(row(8.0)), Kind::Box(Look::default()));
            self.add(
                line,
                Style {
                    width: Length::Px(170.0),
                    ..Style::default()
                },
                Kind::text_line(name, theme.text.body),
            );
            line
        } else {
            let holder = self.add(parent, fill_width(column(8.0)), Kind::Box(Look::default()));
            self.line(holder, name, theme.text.body.with_weight(600));
            self.add(holder, row(8.0), Kind::Box(Look::default()))
        };
        choices
            .into_iter()
            .map(|(label, chosen, target)| {
                let mut button = Button::new(&label, ButtonKind::Plain, theme);
                button.selected = chosen;
                let b = self.add(
                    line,
                    Style {
                        height: Length::Px(44.0),
                        padding: Insets::symmetric(0.0, 12.0),
                        ..Style::default()
                    },
                    Kind::Button(button),
                );
                self.targets.insert(b, target);
                self.tree
                    .set_name(b, &words::name_value(name, &label, lang));
                b
            })
            .collect()
    }

    /// The keys of these actions, one a line, with a way to the keyboard
    /// page beside the first.
    pub(super) fn keys_line(&mut self, theme: &Theme, parent: NodeId, actions: &[Action]) {
        let lang = self.lang;
        let shortcuts = self.ctx.config.shortcuts();
        for (i, action) in actions.iter().enumerate() {
            let line = self.add(parent, fill_width(row(12.0)), Kind::Box(Look::default()));
            let (name, _, _) = words::action(action, lang);
            self.line(line, &name, theme.text.body.with_color(Paint::Muted));
            let keys = self.add(line, row(6.0), Kind::Box(Look::default()));
            match shortcuts.bindings.iter().find(|(_, a)| *a == action) {
                Some((combo, _)) => {
                    for cap in hocus_shell::actions::caps(combo, lang) {
                        self.keycap(theme, keys, &cap);
                    }
                }
                None => {
                    self.line(keys, words::NO_KEYS.get(lang), theme.text.body);
                }
            }
            if i > 0 {
                continue;
            }
            self.add(
                line,
                fill_width(Style::default()),
                Kind::Box(Look::default()),
            );
            let change = self.add(
                line,
                Style {
                    height: Length::Px(40.0),
                    padding: Insets::symmetric(0.0, 16.0),
                    ..Style::default()
                },
                Kind::Button(Button::new(
                    words::CHANGE_KEYS.get(lang),
                    ButtonKind::Plain,
                    theme,
                )),
            );
            self.targets.insert(change, Target::Page(Page::Keyboard));
        }
    }

    /// One fewer, how many, one more: from 1 to 8 desktops. Its buttons
    /// say what they are on when `tell` does.
    fn stepper(
        &mut self,
        theme: &Theme,
        parent: NodeId,
        n: u32,
        (fewer, more): (String, String),
        change: impl Fn(u32) -> Change,
    ) {
        let stepper = self.add(
            parent,
            Style {
                padding: Insets::all(4.0),
                ..row(4.0)
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 14.0,
                ..Look::default()
            }),
        );
        for (label, name, to) in [
            ("−", fewer, n.saturating_sub(1).max(1)),
            ("+", more, (n + 1).min(8)),
        ] {
            let mut b = Button::new(label, ButtonKind::Plain, theme).with_name(name);
            b.enabled = to != n;
            let id = self.add(
                stepper,
                Style {
                    width: Length::Px(44.0),
                    height: Length::Px(40.0),
                    ..Style::default()
                },
                Kind::Button(b),
            );
            self.targets.insert(id, Target::Set(change(to)));
            if label == "−" {
                self.add(
                    stepper,
                    Style {
                        width: Length::Px(36.0),
                        align: Align::Center,
                        ..Style::default()
                    },
                    Kind::text_line(n.to_string(), theme.text.title.with_weight(700)),
                );
            }
        }
    }

    /// Switch cards side by side when there is room, else one under
    /// another.
    pub(super) fn switches(&mut self, parent: NodeId) -> NodeId {
        let wide = self.tree.size().w >= 1180.0;
        self.add(
            parent,
            Style {
                direction: if wide {
                    Direction::Row
                } else {
                    Direction::Column
                },
                ..fill_width(row(12.0))
            },
            Kind::Box(Look::default()),
        )
    }

    /// Desktops: how many each screen has and their names, how many a new
    /// screen gets, how to switch.
    pub(super) fn desktops(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let screens = self.ctx.screens.clone();
        let several = screens.len() > 1;
        for s in &screens {
            let title = screen_title(s, lang);
            let tell = several.then_some(title.as_str());
            let section = self.add(body, fill_width(column(12.0)), Kind::Box(Look::default()));
            self.line(section, &title.to_uppercase(), theme.text.caption);

            // Its count, as on the Screens page.
            let line = self.add(section, fill_width(row(14.0)), Kind::Box(Look::default()));
            self.add(
                line,
                Style {
                    width: Length::Px(90.0),
                    ..Style::default()
                },
                Kind::text_line(words::DESKTOPS.get(lang), theme.text.body),
            );
            let n = s.desktops.clamp(1, 8);
            let output = s.name.clone();
            let names = (
                words::on_screen(words::FEWER_DESKTOPS, tell, lang),
                words::on_screen(words::MORE_DESKTOPS, tell, lang),
            );
            self.stepper(theme, line, n, names, move |to| {
                Change::output(&output, "desktops", i64::from(to))
            });
            if n == 1 {
                self.line(
                    line,
                    words::ONE_DESKTOP_NOTE.get(lang),
                    theme.text.label.with_color(Paint::Muted),
                );
            }
            self.highlighted(section, &Setting::DesktopCount, Look::default());

            // Their names, four to a line.
            let names = self
                .ctx
                .config
                .output(&s.name)
                .map(|o| o.desktop_names.clone())
                .unwrap_or_default();
            let all: Vec<usize> = (0..n as usize).collect();
            for some in all.chunks(4) {
                let line = self.add(section, fill_width(row(10.0)), Kind::Box(Look::default()));
                for i in some {
                    let field = Field::DesktopName {
                        output: s.name.clone(),
                        index: *i,
                    };
                    let name = words::desktop_name_field(i + 1, tell, lang);
                    let hint = words::desktop_hint(i + 1, lang);
                    let value = names.get(*i).cloned().unwrap_or_default();
                    self.text_field(theme, line, field, (&name, &hint), &value);
                }
                for _ in some.len()..4 {
                    self.add(
                        line,
                        fill_width(Style::default()),
                        Kind::Box(Look::default()),
                    );
                }
            }
            self.highlighted(section, &Setting::DesktopNames, Look::default());
        }
        let section = self.add(body, fill_width(column(8.0)), Kind::Box(Look::default()));
        self.note(theme, section, words::DESKTOP_NAMES_NOTE);

        // How many a screen gets the first time.
        let section = self.section(theme, body, words::NEW_SCREENS);
        let per_output = self.ctx.config.desktops.per_output.clamp(1, 8);
        let line = self.add(section, fill_width(row(14.0)), Kind::Box(Look::default()));
        self.add(
            line,
            Style {
                width: Length::Px(90.0),
                ..Style::default()
            },
            Kind::text_line(words::DESKTOPS.get(lang), theme.text.body),
        );
        let names = (
            words::FEWER_NEW.get(lang).to_owned(),
            words::MORE_NEW.get(lang).to_owned(),
        );
        self.stepper(theme, line, per_output, names, |to| {
            Change::new("desktops", "per_output", i64::from(to))
        });
        self.note(theme, section, words::NEW_SCREENS_NOTE);

        let section = self.section(theme, body, words::SWITCHING);
        self.note(theme, section, words::SWITCHING_NOTE);
        self.keys_line(
            theme,
            section,
            &[
                Action::NextDesktop(Scope::PointerOutput),
                Action::PreviousDesktop(Scope::PointerOutput),
            ],
        );
    }

    /// Windows: snapping, gaps, effects and rules.
    pub(super) fn windows(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let config = self.ctx.config.clone();
        let w = config.windows;

        let section = self.section(theme, body, words::SNAPPING);
        self.switch_card(
            theme,
            section,
            (words::SNAP_EDGES, words::SNAP_EDGES_SUB),
            w.snap,
            Some(Change::new("windows", "snap", !w.snap)),
            Some(&Setting::Snap),
        );
        self.note(theme, section, words::SNAP_NOTE);

        let section = self.section(theme, body, words::GAPS);
        for (key, name, now) in [
            ("inner", words::GAPS_INNER, w.gaps.inner),
            ("outer", words::GAPS_OUTER, w.gaps.outer),
        ] {
            let choices = GAPS
                .iter()
                .map(|n| {
                    let target = Target::Set(Change::new("windows.gaps", key, i64::from(*n)));
                    (words::pixels(*n), *n == now, target)
                })
                .collect();
            self.named_choices(theme, section, name, choices);
        }
        self.note(theme, section, words::GAPS_NOTE);
        self.highlighted(section, &Setting::Gaps, Look::default());

        let section = self.section(theme, body, words::EFFECTS);
        let switches = self.switches(section);
        let d = &config.decorations;
        for (setting, section_key, key, name, sub, on) in [
            (
                Some(Setting::Wobbly),
                "windows",
                "wobbly",
                words::WOBBLY,
                words::WOBBLY_SUB,
                w.wobbly,
            ),
            (
                None,
                "decorations",
                "shadow",
                words::SHADOWS,
                words::SHADOWS_SUB,
                d.shadow,
            ),
            (
                Some(Setting::TitleBars),
                "decorations",
                "title_bars",
                words::TITLE_BARS,
                words::TITLE_BARS_SUB,
                d.title_bars,
            ),
        ] {
            let change = Change::new(section_key, key, !on);
            self.switch_card(
                theme,
                switches,
                (name, sub),
                on,
                Some(change),
                setting.as_ref(),
            );
        }
        let radius = d.radius.round() as u32;
        let choices = RADII
            .iter()
            .map(|r| {
                let target = Target::Set(Change::new("decorations", "radius", f64::from(*r)));
                (words::pixels(*r), *r == radius, target)
            })
            .collect();
        self.named_choices(theme, section, words::CORNERS, choices);
        let line = self.add(section, fill_width(row(12.0)), Kind::Box(Look::default()));
        let text = self.add(
            line,
            fill_width(Style::default()),
            Kind::Box(Look::default()),
        );
        self.note(theme, text, words::EFFECTS_NOTE);
        let b = self.add(
            line,
            Style {
                height: Length::Px(40.0),
                padding: Insets::symmetric(0.0, 16.0),
                ..Style::default()
            },
            Kind::Button(Button::new(
                Page::Accessibility.label().get(lang),
                ButtonKind::Plain,
                theme,
            )),
        );
        self.targets.insert(b, Target::Page(Page::Accessibility));

        self.rules(theme, body);
    }

    /// The window rules, each with what it sets and Remove, and a field to
    /// add one.
    fn rules(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        let section = self.section(theme, body, words::RULES);
        self.note(theme, section, words::RULES_NOTE);
        let rules = self.ctx.config.rules.clone();
        if rules.is_empty() {
            self.line(
                section,
                words::NO_RULE.get(lang),
                theme.text.body.with_color(Paint::Muted),
            );
        }
        for (i, rule) in rules.iter().enumerate() {
            let pattern = rule_pattern(rule, lang);
            let card = self.add(
                section,
                Style {
                    padding: Insets::symmetric(12.0, 16.0),
                    ..fill_width(column(10.0))
                },
                Kind::Box(Look {
                    background: Some(Paint::SurfaceHover),
                    radius: 16.0,
                    ..Look::default()
                }),
            );
            let head = self.add(card, fill_width(row(12.0)), Kind::Box(Look::default()));
            let names = self.add(head, fill_width(column(2.0)), Kind::Box(Look::default()));
            let mono = hocus_ui::TextStyle {
                typeface: hocus_ui::Typeface::Mono,
                ..theme.text.body.with_weight(600)
            };
            self.line(names, &pattern, mono);
            let detail = words::rule_detail(rule, lang);
            if !detail.is_empty() {
                self.line(
                    names,
                    &detail,
                    theme.text.caption.with_weight(500).with_color(Paint::Muted),
                );
            }
            let remove = Button::new(words::REMOVE.get(lang), ButtonKind::Plain, theme)
                .with_name(words::remove_rule(&pattern, lang));
            let b = self.add(
                head,
                Style {
                    height: Length::Px(36.0),
                    padding: Insets::symmetric(0.0, 14.0),
                    ..Style::default()
                },
                Kind::Button(remove),
            );
            self.targets.insert(b, Target::Set(Change::forget_rule(i)));
            let chips = self.add(card, row(8.0), Kind::Box(Look::default()));
            for (key, name, on) in [
                ("maximized", words::RULE_MAXIMIZED, rule.maximized),
                ("pinned", words::RULE_PINNED, rule.pinned),
                ("on_top", words::RULE_ON_TOP, rule.on_top),
                ("blur", words::RULE_BLUR, rule.blur),
            ] {
                let on = on == Some(true);
                let mut b = Button::new(name.get(lang), ButtonKind::Chip, theme)
                    .with_name(words::of_rule(&pattern, name, lang));
                b.selected = on;
                let id = self.add(
                    chips,
                    Style {
                        height: Length::Px(36.0),
                        padding: Insets::symmetric(0.0, 12.0),
                        ..Style::default()
                    },
                    Kind::Button(b),
                );
                // On is written; off takes the key out, back to the
                // window's own.
                let value = (!on).then(|| toml_edit::Value::from(true));
                self.targets
                    .insert(id, Target::Set(Change::rule(i, key, value)));
            }
        }
        let line = self.add(section, fill_width(row(10.0)), Kind::Box(Look::default()));
        self.text_field(
            theme,
            line,
            Field::NewRule,
            (words::NEW_RULE.get(lang), words::NEW_RULE_HINT.get(lang)),
            "",
        );
        let add = self.add(
            line,
            Style {
                height: Length::Px(44.0),
                padding: Insets::symmetric(0.0, 18.0),
                ..Style::default()
            },
            Kind::Button(
                Button::new(words::ADD.get(lang), ButtonKind::Primary, theme)
                    .with_name(words::ADD_RULE.get(lang)),
            ),
        );
        self.targets.insert(add, Target::Commit(Field::NewRule));
        self.highlighted(section, &Setting::Rules, Look::default());
    }

    /// Mouse and touchpad: how they scroll, click and go; gestures; hot
    /// corners.
    pub(super) fn mouse(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let config = self.ctx.config.clone();
        let (t, m) = (config.touchpad, config.mouse);

        let section = self.section(theme, body, words::TOUCHPAD);
        let switches = self.switches(section);
        for (setting, key, name, sub, on) in [
            (
                Some(Setting::NaturalScroll),
                "natural_scroll",
                words::NATURAL_SCROLL,
                words::NATURAL_SCROLL_SUB,
                t.natural_scroll,
            ),
            (
                Some(Setting::TapToClick),
                "tap_to_click",
                words::TAP_TO_CLICK,
                words::TAP_TO_CLICK_SUB,
                t.tap_to_click,
            ),
            (
                None,
                "disable_while_typing",
                words::WHILE_TYPING,
                words::WHILE_TYPING_SUB,
                t.disable_while_typing,
            ),
        ] {
            let change = Change::new("touchpad", key, !on);
            let card = self.switch_card(
                theme,
                switches,
                (name, sub),
                on,
                Some(change),
                setting.as_ref(),
            );
            self.tree
                .set_name(card, &words::of_device(words::TOUCHPAD, name, lang));
        }
        self.speed(theme, section, "touchpad", t.speed);

        let section = self.section(theme, body, words::MOUSE);
        let switches = self.switches(section);
        for (key, name, sub, on) in [
            (
                "natural_scroll",
                words::NATURAL_SCROLL,
                words::NATURAL_WHEEL_SUB,
                m.natural_scroll,
            ),
            (
                "left_handed",
                words::LEFT_HANDED,
                words::LEFT_HANDED_SUB,
                m.left_handed,
            ),
        ] {
            let change = Change::new("mouse", key, !on);
            let card = self.switch_card(theme, switches, (name, sub), on, Some(change), None);
            self.tree
                .set_name(card, &words::of_device(words::MOUSE, name, lang));
        }
        self.speed(theme, section, "mouse", m.speed);
        self.note(theme, section, words::POINTING_NOTE);

        // Gestures, and Super alone.
        let section = self.section(theme, body, words::GESTURES);
        let g = config.gestures.clone();
        self.switch_card(
            theme,
            section,
            (words::THREE_SIDEWAYS, words::THREE_SIDEWAYS_SUB),
            g.three_sideways,
            Some(Change::new("gestures", "three_sideways", !g.three_sideways)),
            None,
        );
        for (key, name, current) in [
            ("three_up", words::THREE_UP, g.three_up),
            ("three_down", words::THREE_DOWN, g.three_down),
            ("four_up", words::FOUR_UP, g.four_up),
            ("four_down", words::FOUR_DOWN, g.four_down),
            ("super_alone", words::SUPER_ALONE, g.super_alone),
        ] {
            let choices = Trigger::ALL
                .into_iter()
                .map(|trigger| {
                    let target = Target::Set(Change::new("gestures", key, trigger.as_str()));
                    (
                        words::trigger(trigger).get(lang).to_owned(),
                        trigger == current,
                        target,
                    )
                })
                .collect();
            self.named_choices(theme, section, name, choices);
        }
        self.note(theme, section, words::GESTURES_NOTE);
        self.highlighted(section, &Setting::Gestures, Look::default());

        self.hot_corners(theme, body);
    }

    /// A pointing device's speed, as choices.
    fn speed(&mut self, theme: &Theme, parent: NodeId, section: &'static str, now: f64) {
        let lang = self.lang;
        let device = if section == "mouse" {
            words::MOUSE
        } else {
            words::TOUCHPAD
        };
        let choices = words::SPEEDS
            .iter()
            .map(|(value, name)| {
                let target = Target::Set(Change::new(section, "speed", *value));
                (
                    name.get(lang).to_owned(),
                    (now - value).abs() < 0.05,
                    target,
                )
            })
            .collect();
        let buttons = self.named_choices(theme, parent, words::SPEED, choices);
        // Told apart from the other device's.
        for (b, (_, name)) in buttons.into_iter().zip(words::SPEEDS) {
            let speed = words::named_value(words::SPEED, name.get(lang), lang);
            self.tree
                .set_name(b, &words::named_value(device, &speed, lang));
        }
    }

    /// What each corner of a screen does (#86).
    fn hot_corners(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        let c = self.ctx.config.hot_corners.clone();
        let section = self.section(theme, body, words::HOT_CORNERS);
        self.note(theme, section, words::HOT_CORNERS_NOTE);
        for (key, name, current) in [
            ("top_left", words::TOP_LEFT, c.top_left),
            ("top_right", words::TOP_RIGHT, c.top_right),
            ("bottom_left", words::BOTTOM_LEFT, c.bottom_left),
            ("bottom_right", words::BOTTOM_RIGHT, c.bottom_right),
        ] {
            let choices = Trigger::ALL
                .into_iter()
                .map(|trigger| {
                    let target = Target::Set(Change::new("hot_corners", key, trigger.as_str()));
                    (
                        words::trigger(trigger).get(lang).to_owned(),
                        trigger == current,
                        target,
                    )
                })
                .collect();
            self.named_choices(theme, section, name, choices);
        }
        self.highlighted(section, &Setting::HotCorners, Look::default());
    }

    /// Screenshots and recording: where they go, and how recordings start.
    pub(super) fn captures(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let config = self.ctx.config.clone();
        let s = config.screenshots.clone();

        let section = self.section(theme, body, words::SCREENSHOTS);
        let switches = self.switches(section);
        for (key, name, sub, on) in [
            ("save", words::SAVE_SHOTS, words::SAVE_SHOTS_SUB, s.save),
            ("copy", words::COPY_SHOTS, words::COPY_SHOTS_SUB, s.copy),
        ] {
            let change = Change::new("screenshots", key, !on);
            self.switch_card(theme, switches, (name, sub), on, Some(change), None);
        }
        self.line(
            section,
            words::FOLDER.get(lang),
            theme.text.body.with_weight(600),
        );
        self.text_field(
            theme,
            section,
            Field::ScreenshotFolder,
            (
                words::SHOTS_FOLDER.get(lang),
                words::SHOTS_FOLDER_HINT.get(lang),
            ),
            &s.folder,
        );
        self.note(theme, section, words::FOLDER_FIELD_NOTE);
        self.keys_line(
            theme,
            section,
            &[
                Action::screenshot(ScreenshotMode::Screen),
                Action::screenshot(ScreenshotMode::Region),
                Action::screenshot(ScreenshotMode::Window),
            ],
        );
        self.note(theme, section, words::SHOTS_NOTE);
        self.highlighted(section, &Setting::Screenshots, Look::default());

        let r = config.recording.clone();
        let section = self.section(theme, body, words::RECORDING);
        let fps = FPS
            .iter()
            .map(|n| {
                let target = Target::Set(Change::new("recording", "fps", i64::from(*n)));
                (n.to_string(), *n == r.fps, target)
            })
            .collect();
        self.named_choices(theme, section, words::FPS, fps);
        let countdowns = COUNTDOWNS
            .iter()
            .map(|n| {
                let target = Target::Set(Change::new("recording", "countdown", i64::from(*n)));
                (words::seconds(*n, lang), *n == r.countdown, target)
            })
            .collect();
        self.named_choices(theme, section, words::COUNTDOWN, countdowns);
        let cards = [
            (
                "show_clicks",
                words::SHOW_CLICKS,
                words::SHOW_CLICKS_SUB,
                r.show_clicks,
            ),
            (
                "show_keys",
                words::SHOW_KEYS,
                words::SHOW_KEYS_SUB,
                r.show_keys,
            ),
            (
                "microphone",
                words::MICROPHONE,
                words::MICROPHONE_SUB,
                r.microphone,
            ),
            (
                "system_sound",
                words::SYSTEM_SOUND,
                words::SYSTEM_SOUND_SUB,
                r.system_sound,
            ),
            ("camera", words::WEBCAM, words::WEBCAM_SUB, r.camera),
            ("hardware", words::HARDWARE, words::HARDWARE_SUB, r.hardware),
        ];
        for three in cards.chunks(3) {
            let switches = self.switches(section);
            for &(key, name, sub, on) in three {
                let change = Change::new("recording", key, !on);
                self.switch_card(theme, switches, (name, sub), on, Some(change), None);
            }
        }
        self.line(
            section,
            words::FOLDER.get(lang),
            theme.text.body.with_weight(600),
        );
        self.text_field(
            theme,
            section,
            Field::RecordingFolder,
            (
                words::RECORDING_FOLDER.get(lang),
                words::RECORDING_FOLDER_HINT.get(lang),
            ),
            &r.folder,
        );
        self.note(theme, section, words::FOLDER_FIELD_NOTE);
        self.keys_line(theme, section, &[Action::Record]);
        self.note(theme, section, words::RECORDING_NOTE);
        self.highlighted(section, &Setting::Recording, Look::default());
    }

    /// Clipboard and emoji: the history and the picker.
    pub(super) fn clipboard(&mut self, theme: &Theme, body: NodeId) {
        self.tree.set_style(body, fill_width(column(26.0)));
        let c = self.ctx.config.clipboard;

        let section = self.section(theme, body, words::CLIPBOARD_HISTORY);
        let switches = self.switches(section);
        self.switch_card(
            theme,
            switches,
            (words::KEEP_HISTORY, words::KEEP_HISTORY_SUB),
            c.history,
            Some(Change::new("clipboard", "history", !c.history)),
            Some(&Setting::ClipboardHistory),
        );
        let across = c.history.then(|| Change::new("clipboard", "keep", !c.keep));
        self.switch_card(
            theme,
            switches,
            (words::ACROSS_SESSIONS, words::ACROSS_SESSIONS_SUB),
            c.keep && c.history,
            across,
            None,
        );
        let entries = ENTRIES
            .iter()
            .map(|n| {
                let target = Target::Set(Change::new("clipboard", "entries", i64::from(*n)));
                (n.to_string(), *n == c.entries, target)
            })
            .collect();
        self.named_choices(theme, section, words::ENTRIES, entries);
        self.keys_line(theme, section, &[Action::ClipboardHistory]);
        self.note(theme, section, words::CLIPBOARD_NOTE);

        let section = self.section(theme, body, words::EMOJI);
        self.keys_line(theme, section, &[Action::Emoji]);
        self.note(theme, section, words::EMOJI_NOTE);
        self.highlighted(section, &Setting::Emoji, Look::default());
    }
}
