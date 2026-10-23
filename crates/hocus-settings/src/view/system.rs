//! The System pages (#67): Notifications, Applications, and Account and
//! updates; Accounts and calendar and About are beside them in the view.
//! No board draws them: they keep board R2's template, captioned sections
//! of choices and switch cards. What the compositor holds (do not
//! disturb) is changed through it; the default applications and the
//! autostart entries are the XDG files', which the settings write
//! themselves; the account is AccountsService's and fprintd's.

use hocus_core::config::AppNotifications;
use hocus_platform::account::{AccountInfo, Fingerprint, UserState};

use super::*;
use crate::catalogue::AppsState;

/// History lengths offered; 0 keeps none.
const HISTORY: [u32; 5] = [0, 10, 20, 50, 100];
/// Hours between looks for updates offered.
const EVERY: [u32; 4] = [1, 6, 24, 168];

/// The System pages between builds.
#[derive(Debug, Default)]
pub(super) struct SystemPage {
    /// The kind of file whose choices show.
    pub(super) open: Option<&'static str>,
    /// Why AccountsService did not take the last change.
    pub(super) refused: Option<String>,
}

impl SettingsView {
    /// What the XDG files say of the default applications and autostart.
    pub fn set_apps(&mut self, apps: AppsState) {
        if apps != self.ctx.apps {
            self.ctx.apps = apps;
            self.dirty = true;
        }
    }

    /// What AccountsService and fprintd say.
    pub fn set_account(&mut self, account: AccountInfo) {
        if account != self.ctx.account {
            // What was typed is in effect now.
            self.fields.forget(&Field::RealName);
            self.fields.forget(&Field::Picture);
            self.ctx.account = account;
            self.dirty = true;
        }
    }

    /// AccountsService refused a change, and why.
    pub fn account_refused(&mut self, why: String) {
        self.system.refused = Some(why);
        self.dirty = true;
    }

    /// A name on the left, as the choices' rows have it.
    fn row_name(&mut self, theme: &Theme, line: NodeId, name: &str) {
        self.add(
            line,
            Style {
                width: Length::Px(170.0),
                ..Style::default()
            },
            Kind::text_line(name, theme.text.body),
        );
    }

    /// Notifications: do not disturb, while sharing, the history, and
    /// what becomes of each application's.
    pub(super) fn notifications(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let n = self.ctx.config.notifications.clone();
        let dnd = self.ctx.notifications.dnd;

        let section = self.section(theme, body, words::DND);
        let sub = words::DND_SUB.get(lang);
        self.switch_card_to(
            theme,
            section,
            (words::DND, sub),
            dnd,
            Some(Target::DoNotDisturb(!dnd)),
            Some(&Setting::DoNotDisturb),
        );
        self.note(theme, section, words::DND_NOTE);

        let section = self.section(theme, body, words::SHARING);
        self.switch_card(
            theme,
            section,
            (words::HOLD_WHILE_SHARING, words::HOLD_WHILE_SHARING_SUB),
            n.hold_while_sharing,
            Some(Change::new(
                "notifications",
                "hold_while_sharing",
                !n.hold_while_sharing,
            )),
            Some(&Setting::HoldWhileSharing),
        );

        let section = self.section(theme, body, words::HISTORY);
        let mut offered = HISTORY.to_vec();
        if !offered.contains(&n.history) {
            offered.push(n.history);
            offered.sort_unstable();
        }
        let choices = offered
            .iter()
            .map(|h| {
                let label = if *h == 0 {
                    words::NONE.get(lang).to_owned()
                } else {
                    h.to_string()
                };
                let target = Target::Set(Change::new("notifications", "history", i64::from(*h)));
                (label, *h == n.history, target)
            })
            .collect();
        self.named_choices(theme, section, words::HISTORY_KEEPS, choices);
        self.note(theme, section, words::HISTORY_NOTE);
        self.highlighted(section, &Setting::NotificationHistory, Look::default());

        let section = self.section(theme, body, words::APPLICATIONS);
        // Those heard from this session, the latest first, then those the
        // file names.
        let mut apps = self.ctx.notifications.apps.clone();
        for app in n.apps.keys() {
            if !apps.contains(app) {
                apps.push(app.clone());
            }
        }
        if apps.is_empty() {
            self.note(theme, section, words::NO_APP_YET);
        }
        for app in &apps {
            let current = n.for_app(app);
            let choices = [
                AppNotifications::Show,
                AppNotifications::History,
                AppNotifications::Off,
            ]
            .into_iter()
            .map(|mode| {
                // Back to showing: the key goes, the file stays short.
                let change = match mode {
                    AppNotifications::Show => Change {
                        value: None,
                        ..Change::new("notifications.apps", app.as_str(), "")
                    },
                    mode => Change::new("notifications.apps", app.as_str(), mode.as_str()),
                };
                let label = words::app_mode(mode).get(lang).to_owned();
                (label, mode == current, Target::Set(change))
            })
            .collect();
            self.choices_named(theme, section, app, choices);
        }
        self.note(theme, section, words::APPS_NOTE);
        self.note(theme, section, words::NOTIFICATIONS_SOUND_NOTE);
        self.highlighted(section, &Setting::AppNotifications, Look::default());
    }

    /// Applications: the default one for each kind of file, and those
    /// started with the session.
    pub(super) fn apps(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let apps = match &self.ctx.apps {
            AppsState::Known(apps) => Some(apps.clone()),
            AppsState::Unknown => None,
        };

        let section = self.section(theme, body, words::DEFAULT_APPS);
        match &apps {
            None => self.note(theme, section, words::NOT_READ_YET),
            Some(apps) => {
                for default in &apps.defaults {
                    self.default_app(theme, section, apps, default);
                }
            }
        }
        self.note(theme, section, words::DEFAULT_APPS_NOTE);
        self.highlighted(section, &Setting::DefaultApps, Look::default());

        let section = self.section(theme, body, words::AT_START);
        let session = self.ctx.config.session.clone();
        let choices = [
            ("ask", words::ASK),
            ("always", words::ALWAYS),
            ("never", words::NEVER),
        ]
        .into_iter()
        .map(|(id, name)| {
            let target = Target::Set(Change::new("session", "restore", id));
            (name.get(lang).to_owned(), session.restore == id, target)
        })
        .collect();
        let restore = self.named_choices(theme, section, words::RESTORE, choices);
        if let Some(first) = restore.first()
            && self.highlight == Some(Setting::SessionRestore)
        {
            self.tree.set_focus(Some(*first));
        }
        self.switch_card(
            theme,
            section,
            (words::AUTOSTART, words::AUTOSTART_SUB),
            session.autostart,
            Some(Change::new("session", "autostart", !session.autostart)),
            Some(&Setting::Autostart),
        );
        if let Some(apps) = &apps {
            if apps.autostart.is_empty() {
                self.note(theme, section, words::NO_AUTOSTART);
            }
            for entry in &apps.autostart {
                let sub = if entry.enabled && session.autostart {
                    words::STARTS
                } else {
                    words::DOES_NOT_START
                };
                let target = Target::Autostart {
                    id: entry.id.clone(),
                    on: !entry.enabled,
                };
                self.switch_card_named(
                    theme,
                    section,
                    (&entry.name, sub.get(lang)),
                    entry.enabled,
                    Some(target),
                    None,
                );
            }
        }
        self.note(theme, section, words::AUTOSTART_NOTE);
    }

    /// A kind of file: the application that opens it, and Change, which
    /// shows the others.
    fn default_app(
        &mut self,
        theme: &Theme,
        parent: NodeId,
        apps: &crate::catalogue::Apps,
        default: &hocus_platform::mime::DefaultApp,
    ) {
        let lang = self.lang;
        let kind = words::app_kind(default.kind);
        let line = self.add(
            parent,
            Style {
                height: Length::Px(44.0),
                ..fill_width(row(12.0))
            },
            Kind::Box(Look::default()),
        );
        self.row_name(theme, line, kind.get(lang));
        let current = default.current.as_deref().and_then(|id| apps.name(id));
        let (said, style) = match (&current, default.choices.is_empty()) {
            (Some(name), _) => (name.clone(), theme.text.body.with_weight(700)),
            (None, true) => (
                words::NO_APP_FOR.get(lang).to_owned(),
                theme.text.body.with_color(Paint::Muted),
            ),
            (None, false) => (
                words::NONE_SET.get(lang).to_owned(),
                theme.text.body.with_color(Paint::Muted),
            ),
        };
        self.add(
            line,
            fill_width(Style::default()),
            Kind::text_line(said, style),
        );
        if default.choices.is_empty() {
            return;
        }
        let open = self.system.open == Some(default.kind);
        let mut button = Button::new(words::CHANGE.get(lang), ButtonKind::Plain, theme)
            .with_name(words::named_value(words::CHANGE, kind.get(lang), lang));
        button.selected = open;
        let b = self.add(
            line,
            Style {
                height: Length::Px(40.0),
                padding: Insets::symmetric(0.0, 16.0),
                ..Style::default()
            },
            Kind::Button(button),
        );
        self.targets.insert(b, Target::OpenKind(default.kind));
        if !open {
            return;
        }
        let list = self.add(
            parent,
            Style {
                padding: Insets::all(8.0),
                ..fill_width(column(4.0))
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 16.0,
                ..Look::default()
            }),
        );
        for (id, name) in &default.choices {
            let chosen = default.current.as_deref() == Some(id.as_str());
            let mut button = Button::new(name, ButtonKind::Plain, theme)
                .with_name(words::name_value(kind.get(lang), name, lang));
            button.selected = chosen;
            let b = self.add(
                list,
                Style {
                    height: Length::Px(40.0),
                    padding: Insets::symmetric(0.0, 14.0),
                    ..Style::default()
                },
                Kind::Button(button),
            );
            self.targets.insert(
                b,
                Target::DefaultApp {
                    kind: default.kind,
                    app: id.clone(),
                },
            );
        }
    }

    /// Board R1's "Account and updates": who, the name and picture, the
    /// password and the fingerprint, and the system's updates.
    pub(super) fn account(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let account = self.ctx.account.clone();
        let user = match &account.user {
            UserState::Known(user) => Some(user.clone()),
            UserState::Unknown | UserState::Absent(_) => None,
        };

        let who = self.section(theme, body, words::ACCOUNT);
        let line = self.add(who, fill_width(row(14.0)), Kind::Box(Look::default()));
        let shown = user
            .as_ref()
            .map(|u| u.real_name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| self.ctx.user.clone());
        self.avatar(theme, line, &shown, user.as_ref());
        let names = self.add(line, column(2.0), Kind::Box(Look::default()));
        self.line(names, &shown, theme.text.title.with_weight(700));
        let version = words::version(env!("CARGO_PKG_VERSION"), lang);
        let under = if shown == self.ctx.user {
            version
        } else {
            format!("{} · {version}", self.ctx.user)
        };
        self.line(names, &under, theme.text.body.with_color(Paint::Muted));
        match (&account.user, &user) {
            (_, Some(user)) => {
                for (field, name, hint, value) in [
                    (
                        Field::RealName,
                        words::FULL_NAME,
                        self.ctx.user.clone(),
                        user.real_name.clone(),
                    ),
                    (
                        Field::Picture,
                        words::AVATAR,
                        words::AVATAR_HINT.get(lang).to_owned(),
                        user.icon_file
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default(),
                    ),
                ] {
                    // Captioned as the wallpaper's fields are.
                    let holder = self.add(who, fill_width(column(8.0)), Kind::Box(Look::default()));
                    self.caption(theme, holder, name);
                    self.text_field(theme, holder, field, (name.get(lang), &hint), &value);
                }
                match self.system.refused.clone() {
                    Some(why) => {
                        self.add(
                            who,
                            fill_width(Style::default()),
                            Kind::text(why, theme.text.label.with_color(Paint::Waiting)),
                        );
                    }
                    None => self.note(theme, who, words::ACCOUNT_NOTE),
                }
            }
            (UserState::Absent(_), None) => self.note(theme, who, words::NO_ACCOUNTS_SERVICE),
            (UserState::Unknown | UserState::Known(_), None) => {}
        }
        self.highlighted(who, &Setting::AccountName, Look::default());

        let section = self.section(theme, body, words::PASSWORD);
        self.note(theme, section, words::PASSWORD_NOTE);

        if account.fingerprint != Fingerprint::Unknown {
            let section = self.section(theme, body, words::FINGERPRINT);
            match &account.fingerprint {
                Fingerprint::Reader { name, fingers } => {
                    let said = format!("{name} · {}", words::fingers(fingers.len(), lang));
                    self.line(section, &said, theme.text.body.with_weight(600));
                    self.note(theme, section, words::FINGERPRINT_NOTE);
                }
                Fingerprint::NoReader => {
                    self.line(section, words::NO_READER.get(lang), theme.text.body);
                }
                Fingerprint::NoService | Fingerprint::Unknown => {
                    self.note(theme, section, words::NO_FPRINTD);
                }
            }
            self.highlighted(section, &Setting::Fingerprint, Look::default());
        }

        self.updates(theme, body);
    }

    /// The account's picture, or its initial on the accent.
    fn avatar(
        &mut self,
        theme: &Theme,
        parent: NodeId,
        name: &str,
        user: Option<&hocus_platform::account::User>,
    ) {
        let lang = self.lang;
        let frame = self.add(
            parent,
            Style {
                width: Length::Px(56.0),
                height: Length::Px(56.0),
                align: Align::Center,
                justify: Justify::Center,
                clip: true,
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Accent),
                radius: 28.0,
                ..Look::default()
            }),
        );
        if let Some((w, h, px)) = user.and_then(|u| u.picture.as_ref())
            && let Some(image) = Kind::image(*w, *h, px, words::AVATAR.get(lang))
        {
            self.add(
                frame,
                Style {
                    width: Length::Px(56.0),
                    height: Length::Px(56.0),
                    ..Style::default()
                },
                image,
            );
            return;
        }
        let initial: String = name.chars().take(1).flat_map(char::to_uppercase).collect();
        self.line(
            frame,
            &initial,
            theme
                .text
                .title
                .with_weight(700)
                .with_color(Paint::OnAccent),
        );
    }

    /// Updates: looked for or not, how often, what is ready, Install.
    fn updates(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        let config = self.ctx.config.updates.clone();
        let section = self.section(theme, body, words::UPDATES);
        self.switch_card(
            theme,
            section,
            (words::LOOK_FOR_UPDATES, words::LOOK_FOR_UPDATES_SUB),
            config.enabled,
            Some(Change::new("updates", "enabled", !config.enabled)),
            Some(&Setting::Updates),
        );
        if config.enabled {
            let mut offered = EVERY.to_vec();
            if !offered.contains(&config.every_hours) {
                offered.push(config.every_hours);
                offered.sort_unstable();
            }
            let choices = offered
                .iter()
                .map(|h| {
                    let target = Target::Set(Change::new("updates", "every_hours", i64::from(*h)));
                    (words::every(*h, lang), *h == config.every_hours, target)
                })
                .collect();
            self.named_choices(theme, section, words::LOOK_EVERY, choices);
        }
        let updates = self.ctx.updates.clone();
        let head = self.add(section, fill_width(row(12.0)), Kind::Box(Look::default()));
        let (said, paint) = match (
            updates.state.as_str(),
            &updates.error,
            updates.updates.len(),
        ) {
            ("unsupported", _, _) => (words::NO_MANAGER.get(lang).to_owned(), Paint::Muted),
            ("checking", _, _) => (words::CHECKING.get(lang).to_owned(), Paint::Muted),
            ("installing", _, n) => (words::installing(n, lang), Paint::Muted),
            (_, Some(err), _) => (err.clone(), Paint::Waiting),
            (_, None, 0) => (words::UP_TO_DATE.get(lang).to_owned(), Paint::Muted),
            (_, None, n) => (words::updates_ready(n, lang), Paint::Text),
        };
        let text = self.add(
            head,
            Style {
                width: Length::Fill,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        );
        self.line(
            text,
            &said,
            theme.text.body.with_weight(600).with_color(paint),
        );
        if updates.state == "idle" && !updates.updates.is_empty() {
            let install = self.add(
                head,
                Style {
                    height: Length::Px(40.0),
                    padding: Insets::symmetric(0.0, 18.0),
                    ..Style::default()
                },
                Kind::Button(Button::new(
                    words::INSTALL_ALL.get(lang),
                    ButtonKind::Primary,
                    theme,
                )),
            );
            self.targets.insert(install, Target::InstallUpdates);
        }
        if updates.updates.is_empty() {
            return;
        }
        let list = self.add(
            section,
            Style {
                padding: Insets::symmetric(6.0, 18.0),
                ..fill_width(column(0.0))
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 16.0,
                ..Look::default()
            }),
        );
        for u in &updates.updates {
            let line = self.add(
                list,
                Style {
                    height: Length::Px(40.0),
                    ..fill_width(row(16.0))
                },
                Kind::Box(Look::default()),
            );
            let name = self.add(
                line,
                Style {
                    width: Length::Fill,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            );
            self.line(name, &u.name, theme.text.body.with_weight(600));
            let version = match &u.from {
                Some(from) => format!("{from} → {}", u.to),
                None => u.to.clone(),
            };
            self.line(line, &version, theme.text.label.with_color(Paint::Muted));
        }
    }
}
