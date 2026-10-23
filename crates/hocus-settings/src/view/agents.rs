//! AI and agents (#67): the AI the command field and circles ask (#44),
//! herdr's coding agents and their desktops (#41, #192), the agents that
//! use the desktop and the key that stops them (#176), and circle to ask
//! (#46). No board draws it: it keeps board R2's template, captioned
//! sections of choices, switch cards and fields written with Enter. The
//! provider's key goes to the compositor's keyring, never to the file, and
//! is never shown back: only whether one is kept.

use hocus_core::{Action, Modifiers};

use super::*;
use crate::catalogue::{OPENAI_URL, PROVIDERS, openai_like};

/// The keys offered to hold while circling: Super alone draws (#86).
const CIRCLE_KEYS: [&str; 4] = ["Super+Shift", "Super+Ctrl", "Super+Alt", "Ctrl+Alt"];

/// Keys held, as the keyboard says them: "Super+Maj".
fn keys_name(m: Modifiers, lang: Lang) -> String {
    [
        (m.logo, "Super"),
        (m.ctrl, "Ctrl"),
        (m.alt, "Alt"),
        (m.shift, if lang == Lang::Fr { "Maj" } else { "Shift" }),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .map(|(_, name)| name)
    .collect::<Vec<_>>()
    .join("+")
}

/// A provider's usual address, as the hint says it.
fn usual_url(provider: &str) -> &'static str {
    hocus_platform::ai::Provider::from_name(provider).map_or("", |p| p.default_url())
}

impl SettingsView {
    /// AI and agents: the AI, herdr, the agents on the desktop, circling.
    pub(super) fn agents(&mut self, theme: &Theme, body: NodeId) {
        self.tree.set_style(body, fill_width(column(26.0)));
        self.ai(theme, body);
        self.herdr(theme, body);
        self.mcp_agents(theme, body);
        self.circle(theme, body);
    }

    /// Whether the questions go to a server speaking OpenAI's API that is
    /// not OpenAI's: as the file says, or chosen and its address not typed
    /// yet.
    fn openai_like_chosen(&self) -> bool {
        let ai = &self.ctx.config.ai;
        openai_like(&ai.provider, &ai.url) || (self.system.openai_like && ai.provider == "openai")
    }

    /// Another provider: its own address and model, not the last one's.
    fn provider_changes(&self, id: &str) -> Vec<Change> {
        let ai = &self.ctx.config.ai;
        if id == ai.provider && !self.openai_like_chosen() {
            return Vec::new();
        }
        let mut changes = vec![Change::new("ai", "provider", id)];
        for (key, set) in [("url", &ai.url), ("model", &ai.model)] {
            if !set.is_empty() {
                changes.push(Change {
                    value: None,
                    ..Change::new("ai", key, "")
                });
            }
        }
        changes
    }

    /// OpenAI's API on another server: the address stays, to be typed if
    /// there is none; a model of another provider's goes.
    pub(super) fn choose_openai_like(&mut self) -> Vec<Outcome> {
        self.system.openai_like = true;
        self.dirty = true;
        let ai = &self.ctx.config.ai;
        if ai.provider == "openai" {
            return Vec::new();
        }
        let mut out = vec![Outcome::Change(Box::new(Change::new(
            "ai", "provider", "openai",
        )))];
        if !ai.model.is_empty() {
            out.push(Outcome::Change(Box::new(Change {
                value: None,
                ..Change::new("ai", "model", "")
            })));
        }
        out
    }

    /// The AI: on or off, who answers, where, which model, and the key.
    fn ai(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        let ai = self.ctx.config.ai.clone();
        let section = self.section(theme, body, words::AI_CAPTION);
        self.switch_card(
            theme,
            section,
            (words::ASK_AI, words::ASK_AI_SUB),
            ai.enabled,
            Some(Change::new("ai", "enabled", !ai.enabled)),
            Some(&Setting::Ai),
        );

        let group = self.add(
            section,
            fill_width(column(14.0)),
            Kind::Box(Look::default()),
        );
        let like = self.openai_like_chosen();
        let holder = self.add(group, fill_width(column(8.0)), Kind::Box(Look::default()));
        self.field_name(theme, holder, words::PROVIDER);
        // On this machine or any server, then the services: two lines, so
        // that they fit beside the column.
        let mut lines = [Vec::new(), Vec::new()];
        for (id, name) in PROVIDERS {
            if id == "openai" {
                lines[0].push((
                    words::OPENAI_LIKE.get(lang).to_owned(),
                    like,
                    Target::OpenAiLike,
                ));
            }
            let chosen = ai.provider == id && !(id == "openai" && like);
            let target = Target::SetAll(self.provider_changes(id));
            let line = usize::from(matches!(id, "openai" | "claude"));
            lines[line].push((name.to_owned(), chosen, target));
        }
        for choices in lines {
            let line = self.add(holder, row(8.0), Kind::Box(Look::default()));
            for (label, chosen, target) in choices {
                let b = self.choice(theme, line, &label, chosen, target);
                let name = words::named_value(words::PROVIDER, &label, lang);
                self.tree.set_name(b, &name);
            }
        }

        let hint = if like {
            words::ADDRESS_OF_SERVER.get(lang).to_owned()
        } else if ai.provider == "openai" {
            words::empty_is(OPENAI_URL, lang)
        } else {
            words::empty_is(usual_url(&ai.provider), lang)
        };
        let holder = self.add(group, fill_width(column(8.0)), Kind::Box(Look::default()));
        self.field_name(theme, holder, words::ADDRESS);
        self.text_field(
            theme,
            holder,
            Field::AiUrl,
            (words::AI_ADDRESS.get(lang), &hint),
            &ai.url,
        );
        if self.system.bad_address {
            self.note_painted(theme, holder, words::BAD_ADDRESS, Paint::Waiting);
        } else {
            self.note(theme, holder, words::ADDRESS_NOTE);
        }

        let holder = self.add(group, fill_width(column(8.0)), Kind::Box(Look::default()));
        self.field_name(theme, holder, words::MODEL);
        self.text_field(
            theme,
            holder,
            Field::AiModel,
            (words::AI_MODEL.get(lang), words::MODEL_HINT.get(lang)),
            &ai.model,
        );
        self.note(theme, holder, words::MODEL_NOTE);
        self.highlighted(group, &Setting::AiProvider, Look::default());

        let holder = self.add(section, fill_width(column(8.0)), Kind::Box(Look::default()));
        self.ai_key(theme, holder, &ai.provider, like);
        self.highlighted(holder, &Setting::AiKey, Look::default());
    }

    /// A field's or choices' name above them, as the choices' have it.
    fn field_name(&mut self, theme: &Theme, parent: NodeId, name: words::Words) -> NodeId {
        let name = name.get(self.lang);
        self.line(parent, name, theme.text.body.with_weight(600))
    }

    /// Whether the provider's key is kept, a field to keep another, and
    /// Remove; never the key itself.
    fn ai_key(&mut self, theme: &Theme, holder: NodeId, provider: &str, like: bool) {
        let lang = self.lang;
        let head = self.add(holder, fill_width(row(12.0)), Kind::Box(Look::default()));
        let name = self.add(
            head,
            fill_width(Style::default()),
            Kind::Box(Look::default()),
        );
        self.field_name(theme, name, words::KEY);
        if provider == "ollama" {
            self.note(theme, holder, words::NO_KEY_NEEDED);
            return;
        }
        let kept = self
            .ctx
            .ai
            .keys
            .as_ref()
            .map(|keys| keys.iter().any(|k| k == provider));
        let (said, paint) = match kept {
            Some(true) => (words::KEY_KEPT, Paint::Text),
            Some(false) => (words::NO_KEY, Paint::Muted),
            None => (words::KEY_NOT_READ, Paint::Muted),
        };
        self.line(
            head,
            said.get(lang),
            theme.text.body.with_weight(600).with_color(paint),
        );
        if kept == Some(true) {
            let remove = Button::new(words::REMOVE.get(lang), ButtonKind::Plain, theme)
                .with_name(words::REMOVE_KEY.get(lang));
            let b = self.add(
                head,
                Style {
                    height: Length::Px(36.0),
                    padding: Insets::symmetric(0.0, 14.0),
                    ..Style::default()
                },
                Kind::Button(remove),
            );
            self.targets
                .insert(b, Target::ForgetKey(provider.to_owned()));
        }
        let line = self.add(holder, fill_width(row(10.0)), Kind::Box(Look::default()));
        let hint = if kept == Some(true) {
            words::KEY_HINT_REPLACE
        } else {
            words::KEY_HINT
        };
        self.text_field(
            theme,
            line,
            Field::AiKey,
            (words::AI_KEY.get(lang), hint.get(lang)),
            "",
        );
        let keep = Button::new(words::KEEP.get(lang), ButtonKind::Primary, theme)
            .with_name(words::KEEP_KEY.get(lang));
        let b = self.add(
            line,
            Style {
                height: Length::Px(44.0),
                padding: Insets::symmetric(0.0, 18.0),
                ..Style::default()
            },
            Kind::Button(keep),
        );
        self.targets.insert(b, Target::Commit(Field::AiKey));
        let note = if provider == "llamacpp" || like {
            words::KEY_IF_ASKED
        } else {
            words::KEY_NOTE
        };
        self.note(theme, holder, note);
    }

    /// The desktops of the screen a herdr's agents go to: its own, else
    /// the primary one's.
    fn desktops_of(&self, output: Option<&str>) -> u32 {
        let screens = &self.ctx.screens;
        let screen = match output {
            Some(name) => screens.iter().find(|s| s.name == name),
            None => screens
                .iter()
                .find(|s| s.primary)
                .or_else(|| screens.first()),
        };
        screen.map_or(self.ctx.config.desktops.per_output, |s| s.desktops)
    }

    /// Coding agents: in the column or not, each herdr with its desktop,
    /// and a field to add one.
    fn herdr(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        let agents = self.ctx.config.agents.clone();
        let section = self.section(theme, body, words::AGENTS);
        self.switch_card(
            theme,
            section,
            (words::AGENTS_IN_COLUMN, words::AGENTS_SUB),
            agents.enabled,
            Some(Change::new("agents", "enabled", !agents.enabled)),
            None,
        );
        self.note(theme, section, words::HERDR_NOTE);
        for (i, herdr) in agents.herdr.iter().enumerate() {
            self.herdr_card(theme, section, i, herdr);
        }
        if !agents.herdr_socket.trim().is_empty() {
            let gone = Change {
                value: None,
                ..Change::new("agents", "herdr_socket", "")
            };
            // The older key: its agents belong to no desktop.
            self.herdr_head(
                theme,
                section,
                &agents.herdr_socket,
                words::OLD_SOCKET.get(lang),
                gone,
            );
        }
        if agents.herdr_sockets().is_empty() {
            self.note(theme, section, words::NO_HERDR);
        }
        let line = self.add(section, fill_width(row(10.0)), Kind::Box(Look::default()));
        self.text_field(
            theme,
            line,
            Field::NewHerdr,
            (words::NEW_HERDR.get(lang), words::NEW_HERDR_HINT.get(lang)),
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
                    .with_name(words::ADD_HERDR.get(lang)),
            ),
        );
        self.targets.insert(add, Target::Commit(Field::NewHerdr));
        self.highlighted(section, &Setting::Herdr, Look::default());
    }

    /// A herdr's card: its socket, where its agents go, and Remove.
    fn herdr_head(
        &mut self,
        theme: &Theme,
        parent: NodeId,
        socket: &str,
        detail: &str,
        remove: Change,
    ) -> NodeId {
        let lang = self.lang;
        let card = self.add(
            parent,
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
        self.line(names, socket, mono);
        self.line(
            names,
            detail,
            theme.text.caption.with_weight(500).with_color(Paint::Muted),
        );
        let button = Button::new(words::REMOVE.get(lang), ButtonKind::Plain, theme)
            .with_name(words::remove_herdr(socket, lang));
        let b = self.add(
            head,
            Style {
                height: Length::Px(36.0),
                padding: Insets::symmetric(0.0, 14.0),
                ..Style::default()
            },
            Kind::Button(button),
        );
        self.targets.insert(b, Target::Set(remove));
        card
    }

    /// One of `[[agents.herdr]]`: its desktop and, with several screens,
    /// that desktop's screen.
    fn herdr_card(
        &mut self,
        theme: &Theme,
        parent: NodeId,
        i: usize,
        herdr: &hocus_core::config::HerdrSocket,
    ) {
        let lang = self.lang;
        let detail = match herdr.desktop {
            Some(d) => words::on_desktop(d, herdr.output.as_deref(), lang),
            None => words::ANY_DESKTOP.get(lang).to_owned(),
        };
        let card = self.herdr_head(
            theme,
            parent,
            &herdr.socket,
            &detail,
            Change::forget_herdr(i),
        );
        let count = self.desktops_of(herdr.output.as_deref());
        let mut offered: Vec<Option<u32>> =
            std::iter::once(None).chain((1..=count).map(Some)).collect();
        if !offered.contains(&herdr.desktop) {
            offered.push(herdr.desktop);
        }
        let choices = offered
            .iter()
            .map(|d| {
                let label = match d {
                    Some(d) => d.to_string(),
                    None => words::NO_DESKTOP.get(lang).to_owned(),
                };
                let value = d.map(|d| toml_edit::Value::from(i64::from(d)));
                let target = Target::Set(Change::herdr(i, "desktop", value));
                (label, *d == herdr.desktop, target)
            })
            .collect();
        let name = words::of_herdr(words::DESKTOP, &herdr.socket, lang);
        let buttons = self.named_choices(theme, card, words::DESKTOP, choices);
        self.name_choices(&buttons, &name, &offered_labels(&offered, lang));

        let screens: Vec<String> = self.ctx.screens.iter().map(|s| s.name.clone()).collect();
        if screens.len() < 2 {
            return;
        }
        let mut offered: Vec<Option<String>> = std::iter::once(None)
            .chain(screens.into_iter().map(Some))
            .collect();
        if !offered.contains(&herdr.output) {
            offered.push(herdr.output.clone());
        }
        let labels: Vec<String> = offered
            .iter()
            .map(|o| {
                o.clone()
                    .unwrap_or_else(|| words::PRIMARY.get(lang).to_owned())
            })
            .collect();
        let choices = offered
            .iter()
            .zip(&labels)
            .map(|(o, label)| {
                let value = o.as_deref().map(toml_edit::Value::from);
                let target = Target::Set(Change::herdr(i, "output", value));
                (label.clone(), *o == herdr.output, target)
            })
            .collect();
        let name = words::of_herdr(words::SCREEN, &herdr.socket, lang);
        let buttons = self.named_choices(theme, card, words::SCREEN, choices);
        self.name_choices(&buttons, &name, &labels);
    }

    /// Choices shown under a short name, said with a longer one: each
    /// herdr's are told apart.
    fn name_choices(&mut self, buttons: &[NodeId], name: &str, labels: &[String]) {
        for (b, label) in buttons.iter().zip(labels) {
            self.tree
                .set_name(*b, &words::name_value(name, label, self.lang));
        }
    }

    /// The agents that use the desktop (#42): what the journal keeps, the
    /// key that stops them, and the same as a button.
    fn mcp_agents(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        let agents = self.ctx.config.agents.clone();
        let stopped = self.ctx.agents_stopped;
        let section = self.section(theme, body, words::MCP_AGENTS);
        self.switch_card(
            theme,
            section,
            (words::JOURNAL_TEXT, words::JOURNAL_TEXT_SUB),
            agents.journal_text,
            Some(Change::new("agents", "journal_text", !agents.journal_text)),
            Some(&Setting::JournalText),
        );
        let line = self.add(
            section,
            Style {
                padding: Insets::symmetric(12.0, 16.0),
                ..fill_width(row(12.0))
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 16.0,
                ..Look::default()
            }),
        );
        let (said, paint, button, kind) = if stopped {
            (
                words::AGENTS_STOPPED,
                Paint::Waiting,
                words::ALLOW_AGENTS,
                ButtonKind::Primary,
            )
        } else {
            (
                words::AGENTS_ALLOWED,
                Paint::Muted,
                words::STOP_ALL,
                ButtonKind::Plain,
            )
        };
        let text = self.add(
            line,
            fill_width(Style::default()),
            Kind::Box(Look::default()),
        );
        self.add(
            text,
            fill_width(Style::default()),
            Kind::text(
                said.get(lang),
                theme.text.body.with_weight(600).with_color(paint),
            ),
        );
        let b = self.add(
            line,
            Style {
                height: Length::Px(40.0),
                padding: Insets::symmetric(0.0, 18.0),
                ..Style::default()
            },
            Kind::Button(Button::new(button.get(lang), kind, theme)),
        );
        self.targets.insert(b, Target::StopAgents(!stopped));
        self.highlighted(
            line,
            &Setting::StopAgents,
            Look {
                background: Some(Paint::SurfaceHover),
                radius: 16.0,
                ..Look::default()
            },
        );
        self.stop_key(theme, section);
        self.note(theme, section, words::STOP_NOTE);
    }

    /// The stop key, as the shortcuts have it, and a way to change it.
    fn stop_key(&mut self, theme: &Theme, parent: NodeId) {
        let lang = self.lang;
        let line = self.add(parent, fill_width(row(12.0)), Kind::Box(Look::default()));
        self.line(
            line,
            words::STOP_KEY.get(lang),
            theme.text.body.with_color(Paint::Muted),
        );
        let caps = self.add(line, row(6.0), Kind::Box(Look::default()));
        let shortcuts = self.ctx.config.shortcuts();
        match shortcuts
            .bindings
            .iter()
            .find(|(_, a)| **a == Action::StopAgents)
        {
            Some((combo, _)) => {
                for cap in hocus_shell::actions::caps(combo, lang) {
                    self.keycap(theme, caps, &cap);
                }
            }
            None => {
                self.line(caps, words::NO_KEYS.get(lang), theme.text.body);
            }
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
            Kind::Button(
                Button::new(words::CHANGE_KEYS.get(lang), ButtonKind::Plain, theme)
                    .with_name(words::CHANGE_STOP_KEY.get(lang)),
            ),
        );
        self.targets.insert(change, Target::Page(Page::Keyboard));
    }

    /// Circle to ask: on or off, and the keys held.
    fn circle(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        let circle = self.ctx.config.circle.clone();
        let held = circle.modifiers.parse::<Modifiers>().ok();
        let section = self.section(theme, body, words::CIRCLE);
        let keys = held.map_or_else(|| circle.modifiers.clone(), |m| keys_name(m, lang));
        let sub = words::circle_how(&keys, lang);
        self.switch_card_to(
            theme,
            section,
            (words::CIRCLE, &sub),
            circle.enabled,
            Some(Target::Set(Change::new(
                "circle",
                "enabled",
                !circle.enabled,
            ))),
            Some(&Setting::Circle),
        );
        if circle.enabled {
            let mut offered: Vec<Modifiers> =
                CIRCLE_KEYS.iter().filter_map(|k| k.parse().ok()).collect();
            if let Some(m) = held
                && !offered.contains(&m)
            {
                offered.push(m);
            }
            let choices = offered
                .iter()
                .map(|m| {
                    let written = keys_name(*m, Lang::En);
                    let target = Target::Set(Change::new("circle", "modifiers", written));
                    (keys_name(*m, lang), Some(*m) == held, target)
                })
                .collect();
            self.named_choices(theme, section, words::KEYS_HELD, choices);
        }
        self.note(theme, section, words::CIRCLE_NOTE);
    }
}

/// The desktops offered, as their buttons say them.
fn offered_labels(offered: &[Option<u32>], lang: Lang) -> Vec<String> {
    offered
        .iter()
        .map(|d| match d {
            Some(d) => d.to_string(),
            None => words::NO_DESKTOP.get(lang).to_owned(),
        })
        .collect()
}
