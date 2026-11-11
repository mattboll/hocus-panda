//! The Screens page drawn: its groups, rows, menus and dialog.

use super::*;
use crate::lang::French as _;
// The widget macros by name: through the glob they would be ambiguous.
use iced::widget::{column, row, stack};

impl App {
    /// The drawing's frame: its scale and where its screens' box starts.
    pub(super) fn view_of(&self) -> Option<View> {
        let known = self.screens_list()?;
        let width = self
            .places
            .at
            .get("drawing")
            .map_or(crate::app::view::PAGE_WIDTH - 16.0, |r| r.width);
        View::of(known, width)
    }

    pub(in crate::app) fn screens_page(&self) -> El<'_> {
        let lang = self.lang;
        let mut page = column![
            column![
                text(self.say(w("Écrans", "Screens"))).size(22).font(BOLD).color(TEXT),
                t(
                    self.say(w(
                        "La place de chaque écran, sa résolution, sa fréquence et son échelle ; l’écran où vont la barre et les nouvelles fenêtres.",
                        "Where each screen sits, its resolution, refresh rate and scale; the screen the bar and new windows go to.",
                    )),
                    13.5,
                    DIM.get(),
                )
                .line_height(1.5),
            ]
            .spacing(6),
        ]
        .spacing(18)
        .padding(Padding::default().top(14));
        // The arrangement.
        let identify_focus = self.focused(&Control::Identify);
        let identify = container(
            button(
                row![
                    icon(Icon::Screens, 13.0, DIM.get()),
                    t(self.say(w("Identifier", "Identify")), 12.0, TEXT.get()).font(SEMIBOLD),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .height(26)
            .padding([0, 10])
            .on_press_maybe((!self.chosen_inert_all()).then_some(Message::Screens(Msg::Identify)))
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(Background::Color(crate::theme::ink(rgba(
                        255,
                        255,
                        255,
                        if hovered { 0.09 } else { 0.06 },
                    )))),
                    text_color: TEXT.get(),
                    border: if identify_focus {
                        ring(7.0)
                    } else {
                        border(7.0, 1.0, RIM_12.get())
                    },
                    ..button::Style::default()
                }
            }),
        )
        .id(format!("control:{IDENTIFY}"));
        let head = row![
            label(self.say(w("DISPOSITION", "ARRANGEMENT"))),
            Space::new().width(Length::Fill),
            identify
        ]
        .align_y(Alignment::Center)
        .height(28);
        let drawing = container(
            canvas(Drawing {
                drawn: self.drawn(),
                landing: self.screens.held.as_ref().and_then(|h| {
                    let (moving, _, at) = self.landing(h)?;
                    h.dragging.then(|| {
                        let view = self.view_of()?;
                        Some(view.rect_of(Rect {
                            x: at.0,
                            y: at.1,
                            ..moving.rect()
                        }))
                    })?
                }),
                holding: self.screens.held.as_ref().map(|h| h.name.clone()),
                big: self.screens.identify.is_some(),
                empty: self.screens_empty(),
            })
            .width(Length::Fill)
            .height(DRAWING_HEIGHT),
        )
        .id("drawing".to_owned());
        let arrangement = column![
            container(drawing).style(boxed(crate::theme::ink(rgba(0, 0, 0, 0.20)), 8.0, None)),
            container(
                t(
                    self.say(w(
                        "Glisse un écran pour le placer contre l’autre ; un clic le choisit, ses réglages sont dessous. La bande claire est la barre.",
                        "Drag a screen against the other; click one to show its settings below. The light strip is the bar.",
                    )),
                    12.0,
                    FAINT.get(),
                ),
            )
            .padding([2, 6]),
        ]
        .spacing(8);
        page = page.push(
            container(column![head, card_box(container(arrangement).padding(8).into())].spacing(8))
                .id(format!("row:{ARRANGE}")),
        );
        for note in self.notes() {
            page = page.push(note);
        }
        // The sets of screens known, under the arrangement (windows.md
        // §12).
        if let Some(sets) = self.sets_group() {
            page = page.push(sets);
        }
        // The chosen screen.
        if let Some(screen) = self.chosen() {
            page = page.push(self.screen_group(screen));
        }
        // All screens.
        let primary_line = self.primary_line();
        let mut all = column![];
        all = all.push(self.screen_row(
            PRIMARY,
            self.say(w("Écran principal", "Primary screen")),
            Some(primary_line.0),
            primary_line.1,
            None,
            self.menu_control(PRIMARY),
        ));
        all = all.push(divided(
            true,
            self.screen_row(
                MIRROR,
                self.say(w("Recopie", "Mirroring")),
                Some(
                    self.say(w(
                        "Montrer la même image sur les deux écrans, pour un projecteur ou une présentation. Pas encore construite.",
                        "Show the same picture on both screens, for a projector or a talk. Not built yet.",
                    ))
                    .to_owned(),
                ),
                None,
                Some(later(lang)),
                switch(false, true, false, 1.0, None),
            ),
        ));
        page = page.push(
            column![
                row![label(self.say(w("TOUS LES ÉCRANS", "ALL SCREENS")))]
                    .height(28)
                    .align_y(Alignment::Center),
                card_box(all.into()),
            ]
            .spacing(8),
        );

        // The night light (#436): the model's rows, the file's keys.
        let controls = self.controls();
        let mut night = column![];
        for (i, id) in screens::NIGHT_ROWS.iter().enumerate() {
            if let Some(setting) = crate::model::setting(id) {
                night = night.push(divided(i > 0, self.row(setting, &controls)));
            }
        }
        page = page.push(
            column![
                row![label(self.say(w("ÉCLAIRAGE NOCTURNE", "NIGHT LIGHT")))]
                    .height(28)
                    .align_y(Alignment::Center),
                card_box(night.into()),
            ]
            .spacing(8),
        );
        container(page).id("page:screens".to_owned()).into()
    }

    /// ENSEMBLES D'ÉCRANS: a row for each set known, the one in use lit
    /// first; the others' menu forgets one. Nothing while only one set is
    /// known: there is nothing to choose or forget.
    pub(super) fn sets_group(&self) -> Option<El<'_>> {
        let known = self.screens_list()?;
        if known.sets.len() < 2 {
            return None;
        }
        let lang = self.lang;
        let now = jiff::Timestamp::now().as_second();
        let zone = jiff::tz::TimeZone::system();
        let mut list = column![].spacing(2);
        for (i, set) in known.sets.iter().enumerate() {
            let shapes = row(set
                .in_order()
                .into_iter()
                .map(|screen| {
                    let (w, h) = if screen.built_in {
                        (20.0, 13.0)
                    } else if screen.size.0 > 0 && screen.size.1 > 0 {
                        let w = (screen.size.0 as f32 / 110.0).clamp(22.0, 34.0);
                        (
                            w,
                            (w * screen.size.1 as f32 / screen.size.0 as f32).clamp(11.0, 20.0),
                        )
                    } else {
                        (26.0, 16.0)
                    };
                    container(Space::new().width(w).height(h))
                        .style(|_: &Theme| container::Style {
                            border: border(3.0, 1.5, DIM.get()),
                            ..container::Style::default()
                        })
                        .into()
                })
                .collect::<Vec<El<'_>>>())
            .spacing(3)
            .align_y(Alignment::End);
            let words = column![
                t(set.title(lang), 13.5, TEXT.get()).font(SEMIBOLD),
                t(set.line(lang, now, &zone), 12.0, DIM.get()),
            ]
            .spacing(2)
            .width(Length::Fill);
            let mut line = row![
                container(shapes)
                    .width(58)
                    .height(30)
                    .align_y(Alignment::End),
                words
            ]
            .spacing(12)
            .align_y(Alignment::Center);
            if !set.current {
                let open = self.screens.menu.is_some_and(|(r, _)| r == SETS)
                    && self.screens.set_menu.as_deref() == Some(set.name.as_str());
                let inert = !self.writable();
                let focused = self.focused(&Control::Set(set.name.clone()));
                let more = button(
                    container(t(
                        "⋯",
                        16.0,
                        fade(TEXT.get(), if inert { INERT } else { 1.0 }),
                    ))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
                )
                .width(32)
                .height(28)
                .padding(0)
                .on_press_maybe((!inert).then(|| Message::Screens(Msg::SetMenu(set.name.clone()))))
                .style(move |_, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: Some(Background::Color(crate::theme::ink(rgba(
                            255,
                            255,
                            255,
                            if hovered || open { 0.09 } else { 0.0 },
                        )))),
                        text_color: TEXT.get(),
                        border: if focused {
                            ring(7.0)
                        } else {
                            border(7.0, 1.0, Color::TRANSPARENT)
                        },
                        ..button::Style::default()
                    }
                });
                line = line.push(container(more).id(format!("control:set:{i}")));
            }
            let current = set.current;
            list = list.push(
                container(line)
                    .padding([8, 10])
                    .width(Length::Fill)
                    .style(move |_: &Theme| container::Style {
                        background: current
                            .then(|| Background::Color(crate::theme::Ink::Accent(0.12).get())),
                        border: border(
                            9.0,
                            1.0,
                            if current {
                                crate::theme::Ink::Accent(0.40).get()
                            } else {
                                Color::TRANSPARENT
                            },
                        ),
                        ..container::Style::default()
                    })
                    .id(format!("set:{i}")),
            );
        }
        let note = t(
            self.say(w(
                "Chaque ensemble garde la place des écrans, leurs résolutions, leurs échelles et l’écran principal.",
                "Each set keeps its screens' places, resolutions, scales and the primary screen.",
            )),
            12.0,
            FAINT.get(),
        );
        Some(
            container(
                column![
                    row![label(self.say(w("ENSEMBLES D’ÉCRANS", "SCREEN SETS")))]
                        .height(28)
                        .align_y(Alignment::Center),
                    card_box(
                        container(column![list, container(note).padding([4, 6])].spacing(6))
                            .padding(6)
                            .into()
                    ),
                ]
                .spacing(8),
            )
            .id(format!("row:{SETS}"))
            .into(),
        )
    }

    /// Nothing to draw yet, or nothing will come: the drawing says so.
    pub(super) fn screens_empty(&self) -> Option<String> {
        if self.screens.unavailable {
            return Some(
                self.say(w(
                    "Le bureau ne dit pas ses écrans ici : les Réglages ne tournent pas sous Hocuspanda.",
                    "The desktop does not tell its screens here: Settings is not running under Hocuspanda.",
                ))
                .to_owned(),
            );
        }
        self.screens
            .known
            .is_none()
            .then(|| self.say(w("Les écrans…", "The screens…")).to_owned())
    }

    /// The screens as drawn.
    pub(super) fn drawn(&self) -> Vec<Drawn> {
        let lang = self.lang;
        let (Some(known), Some(view)) = (self.screens_list(), self.view_of()) else {
            return Vec::new();
        };
        let chosen = self.chosen().map(|s| s.name.clone());
        let on = known.on().count();
        let now = self.now;
        let mut out = Vec::new();
        for s in &known.list {
            let mut rect = view.rect_of(s.rect());
            let mut lifted = false;
            if let Some(held) = &self.screens.held
                && held.dragging
                && held.name == s.name
            {
                rect.x += held.at.x - held.from.x;
                rect.y += held.at.y - held.from.y;
                lifted = true;
            } else if let Some(sliding) = &self.screens.sliding
                && sliding.name == s.name
                && !self.reduced_motion
            {
                let k =
                    crate::app::view::ease_out(now.saturating_duration_since(sliding.since), SLIDE);
                rect.x = sliding.from.x + (rect.x - sliding.from.x) * k;
                rect.y = sliding.from.y + (rect.y - sliding.from.y) * k;
            }
            let dashed = if !s.connected {
                Some(
                    self.say(w(
                        "Débranché · sa place est gardée",
                        "Unplugged · its place is kept",
                    ))
                    .to_owned(),
                )
            } else if s.lid_closed && on > 0 {
                Some(self.say(w("Couvercle fermé", "Lid closed")).to_owned())
            } else if !s.enabled {
                Some(self.say(w("Éteint", "Off")).to_owned())
            } else {
                None
            };
            let title = if rect.width < 150.0 {
                s.short(lang)
            } else {
                s.title(lang)
            };
            // A narrow one says its size alone.
            let mut line = match s.mode {
                Some(m) if rect.width < 190.0 => screens::say_size((m.0, m.1)),
                Some(m) => format!(
                    "{} · {}",
                    screens::say_size((m.0, m.1)),
                    screens::say_rate(m.2, lang)
                ),
                None => String::new(),
            };
            if !s.scale_auto && s.connected {
                line.push_str(&format!(" · {}", screens::say_scale(s.scale)));
            }
            let bar = (s.primary && dashed.is_none()).then(|| {
                let width = if s.size.0 >= FULL_FROM { 196.0 } else { 56.0 };
                let f = view.f;
                Rectangle {
                    x: rect.x + rect.width - (8.0 + width) * f,
                    y: rect.y + 8.0 * f,
                    width: width * f,
                    height: (rect.height - 16.0 * f).max(0.0),
                }
            });
            out.push(Drawn {
                name: s.name.clone(),
                number: s.number,
                rect,
                selected: chosen.as_deref() == Some(s.name.as_str()) && s.connected,
                dashed,
                title,
                line,
                bar,
                lifted,
            });
        }
        out
    }

    /// The notes for a screen missing.
    pub(super) fn notes(&self) -> Vec<El<'_>> {
        let lang = self.lang;
        let Some(known) = self.screens_list() else {
            return Vec::new();
        };
        let laptop = known
            .plugged()
            .find(|s| s.built_in)
            .or_else(|| known.plugged().next());
        let other_name = |s: &Screen| {
            known
                .plugged()
                .find(|o| o.name != s.name && o.enabled)
                .map(|o| screens::lower_first(&o.title(lang)))
                .unwrap_or_default()
        };
        let mut notes = Vec::new();
        for s in &known.list {
            let title = s.title(lang);
            let (head, body) = if !s.connected {
                let at = s.unplugged_at.and_then(local_time).unwrap_or_default();
                let place = laptop.map(|l| {
                    let side = screens::side_of(s.rect(), l.rect());
                    let align = screens::align_of(s.rect(), l.rect(), side);
                    screens::say_place(side, align, &l.title(lang), lang)
                });
                let other = laptop
                    .map(|l| screens::lower_first(&l.title(lang)))
                    .unwrap_or_default();
                match lang {
                    Lang::French => (
                        format!("{title} débranché à {at}"),
                        format!(
                            "Sa place ({}) et ses réglages sont gardés : il les reprend dès qu’il revient. Ses fenêtres sont passées sur l’{other}.",
                            place.unwrap_or_default()
                        ),
                    ),
                    Lang::English => (
                        format!("{title} unplugged at {at}"),
                        format!(
                            "Its place ({}) and settings are kept: it takes them back as soon as it returns. Its windows went to the {other}.",
                            place.unwrap_or_default()
                        ),
                    ),
                }
            } else if s.lid_closed && known.on().count() > 0 {
                let other = other_name(s);
                match lang {
                    Lang::French => (
                        "Couvercle fermé".to_owned(),
                        format!(
                            "L’écran du portable est éteint ; la session continue sur l’{other}, et la barre y est passée. Ses réglages reviennent quand tu rouvres le couvercle."
                        ),
                    ),
                    Lang::English => (
                        "Lid closed".to_owned(),
                        format!(
                            "The laptop screen is off; the session goes on on the {other}, and the bar moved there. Its settings come back when you open the lid."
                        ),
                    ),
                }
            } else if !s.enabled {
                let other = other_name(s);
                match lang {
                    Lang::French => (
                        format!("{title} éteint"),
                        format!(
                            "Il reste branché et garde sa place ; ses fenêtres sont passées sur l’{other}."
                        ),
                    ),
                    Lang::English => (
                        format!("{title} off"),
                        format!(
                            "It stays plugged in and keeps its place; its windows went to the {other}."
                        ),
                    ),
                }
            } else {
                continue;
            };
            notes.push(note(head, body, format!("note:{}", s.name)));
        }
        if known.places_refused {
            notes.push(note(
                self.say(w("Places du fichier refusées", "The file's places refused")).to_owned(),
                self.say(w(
                    "Dans config.toml, les écrans se chevauchent ou l’un est à l’écart : ils sont rangés de gauche à droite. Glisse-les pour les placer.",
                    "In config.toml the screens overlap or one stands apart: they are put left to right. Drag them into place.",
                ))
                .to_owned(),
                "note:places".to_owned(),
            ));
        }
        notes
    }

    /// The chosen screen's group: its head and its rows.
    pub(super) fn screen_group(&self, screen: &Screen) -> El<'_> {
        let lang = self.lang;
        let mut head = row![
            container(
                text(screen.number.to_string())
                    .size(10.5)
                    .font(BOLD)
                    .color(DEEP)
            )
            .width(20)
            .height(20)
            .center(20)
            .style(boxed(ACCENT.get(), 10.0, None)),
            label_owned(screen.title(lang).to_uppercase()),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .height(28);
        if screen.primary {
            head = head.push(
                container(t(self.say(w("Principal", "Primary")), 11.0, BLUE.get()).font(BOLD))
                    .padding([2, 8])
                    .style(boxed(
                        crate::theme::ink(rgba(143, 179, 255, 0.12)),
                        10.0,
                        None,
                    )),
            );
        }
        let mut facts = vec![screen.name.clone()];
        if let Some(i) = screen.inches() {
            facts.push(match lang {
                Lang::French => format!("{i} pouces"),
                Lang::English => format!("{i} in"),
            });
        }
        if let Some(d) = screen.density() {
            facts.push(match lang {
                Lang::French => format!("{d} ppp"),
                Lang::English => format!("{d} ppi"),
            });
        }
        head = head.push(Space::new().width(Length::Fill));
        head = head.push(text(facts.join(" · ")).size(11).font(MONO).color(FAINT));
        let only = self.screens_list().is_some_and(|k| k.on().count() <= 1) && screen.enabled;
        let lid = screen.lid_closed && self.screens_list().is_some_and(|k| k.on().count() > 0);
        let use_line = if lid {
            self.say(w("Le couvercle est fermé.", "The lid is closed."))
        } else if only {
            self.say(w(
                "C’est le seul écran allumé.",
                "It is the only screen on.",
            ))
        } else {
            self.say(w(
                "Éteint, il garde sa place et ses réglages.",
                "Off, it keeps its place and its settings.",
            ))
        };
        let native = screen.native();
        let size = screen.mode.map(|m| (m.0, m.1));
        let resolution_line = match (native, size) {
            (Some(n), Some(s)) if n == s => self
                .say(w("Sa définition native.", "Its native resolution."))
                .to_owned(),
            (Some(n), _) => match lang {
                Lang::French => format!(
                    "Hors de sa définition native ({}), l’image est moins nette.",
                    screens::say_size(n)
                ),
                Lang::English => format!(
                    "Not its native resolution ({}): the picture is less sharp.",
                    screens::say_size(n)
                ),
            },
            _ => String::new(),
        };
        let resolution_warn = screen
            .file_mode
            .as_ref()
            .filter(|_| !screen.mode_offered)
            .map(|m| {
                let applies = screen
                    .mode
                    .map(|m| say_mode3(m, lang))
                    .unwrap_or_default();
                match lang {
                    Lang::French => format!("Dans le fichier : « {m} », que l’écran ne propose pas ; {applies} s’applique."),
                    Lang::English => format!("In the file: “{m}”, which the screen does not offer; {applies} applies."),
                }
            });
        let rates = size.map(|s| screen.rates(s)).unwrap_or_default();
        let rate_line = (rates.len() <= 1).then(|| {
            self.say(w(
                "Une seule à cette résolution.",
                "Only one at this resolution.",
            ))
            .to_owned()
        });
        let logical_size = screen.size;
        let mut scale_line = match lang {
            Lang::French => format!("Comme {}", screens::say_size(logical_size)),
            Lang::English => format!("Like {}", screens::say_size(logical_size)),
        };
        if screen.scale_auto
            && let Some(d) = screen.density()
        {
            scale_line.push_str(&match lang {
                Lang::French => format!(", choisie par sa densité ({d} ppp)"),
                Lang::English => format!(", chosen by its density ({d} ppi)"),
            });
        }
        if logical_size.0 < FULL_FROM {
            scale_line.push_str(self.say(w(
                " ; la barre y prend sa forme mini.",
                "; the bar takes its mini form there.",
            )));
        }
        let vrr_line = if screen.vrr_capable {
            self.say(w(
                "L’image suit le rythme des jeux et des vidéos, sans saccades.",
                "The picture follows games and videos, without stutter.",
            ))
        } else {
            self.say(w(
                "Cet écran ne la propose pas.",
                "This screen does not offer it.",
            ))
        };
        let mut rows = column![];
        rows = rows.push(self.screen_row(
            USE,
            self.say(w("Utiliser cet écran", "Use this screen")),
            Some(use_line.to_owned()),
            None,
            None,
            self.switch_control(USE, screen.enabled),
        ));
        rows = rows.push(rule());
        let rows = rows
            .push(self.screen_row(
                RESOLUTION,
                self.say(w("Résolution", "Resolution")),
                Some(resolution_line),
                resolution_warn,
                None,
                self.menu_control(RESOLUTION),
            ))
            .push(rule())
            .push(self.screen_row(
                RATE,
                self.say(w("Fréquence", "Refresh rate")),
                rate_line,
                None,
                None,
                self.menu_control(RATE),
            ))
            .push(rule())
            .push(self.screen_row(
                SCALE,
                self.say(w("Échelle", "Scale")),
                Some(scale_line),
                None,
                None,
                self.menu_control(SCALE),
            ))
            .push(rule())
            .push(self.screen_row(
                VRR,
                self.say(w("Fréquence variable (VRR)", "Variable refresh rate (VRR)")),
                Some(vrr_line.to_owned()),
                None,
                None,
                self.switch_control(VRR, screen.vrr),
            ))
            .push(rule())
            .push(
                self.screen_row(
                    ROTATION,
                    self.say(w("Rotation", "Rotation")),
                    Some(
                        self.say(w(
                            "Prévue après la v1 (choix du 29 octobre) : à confirmer.",
                            "Planned after v1 (choice of 29 October): to be confirmed.",
                        ))
                        .to_owned(),
                    ),
                    None,
                    Some(later(lang)),
                    menu_button(
                        self.say(w("Normale", "Normal")).to_owned(),
                        true,
                        false,
                        false,
                        None,
                    ),
                ),
            );
        column![head, card_box(rows.into())].spacing(8).into()
    }

    /// The line under "Écran principal", and its warning.
    pub(super) fn primary_line(&self) -> (String, Option<String>) {
        let line = self
            .say(w(
                "La barre et les nouvelles fenêtres y vont ; automatique : l’écran du portable s’il est allumé, sinon l’autre. La forme de la barre, écran par écran, se règle dans La barre.",
                "The bar and new windows go there; automatic: the laptop screen when it is on, else the other. The bar's form, screen by screen, is set in The bar.",
            ))
            .to_owned();
        let warn = self.primary_named().and_then(|name| {
            let known = self.screens_list()?;
            if known.get(&name).is_some_and(|s| s.connected) {
                return None;
            }
            let there = known.plugged().find(|s| s.primary)?;
            let named = self.say_primary(Some(&name));
            let there = screens::lower_first(&there.title(self.lang));
            Some(match self.lang {
                Lang::French => {
                    format!("{named} n’est pas là : la barre est sur l’{there} en attendant.")
                }
                Lang::English => {
                    format!("{named} is not there: the bar is on the {there} meanwhile.")
                }
            })
        });
        (line, warn)
    }

    pub(super) fn switch_control(&self, row: &'static str, on: bool) -> El<'_> {
        let inert = self.screens_row_inert(row);
        let moved = self.knobs.get(row).map_or(1.0, |since| {
            crate::app::view::ease_out(self.now.saturating_duration_since(*since), crate::app::KNOB)
        });
        let focused = self.focused(&Control::Row(row));
        container(switch(
            on,
            inert,
            focused,
            moved,
            Some(Message::Screens(Msg::Toggle(row))),
        ))
        .id(format!("control:{row}"))
        .into()
    }

    pub(super) fn menu_control(&self, row: &'static str) -> El<'_> {
        let inert = self.screens_row_inert(row);
        let current = self.screens_current(row);
        let said = self
            .screens_choices(row)
            .into_iter()
            .find(|(c, _)| Some(c) == current.as_ref())
            .map(|(_, l)| l)
            .unwrap_or_default();
        let open = self.screens.menu.is_some_and(|(r, _)| r == row);
        let focused = self.focused(&Control::Row(row));
        container(menu_button(
            said,
            inert,
            focused,
            open,
            Some(Message::Screens(Msg::Menu(row))),
        ))
        .id(format!("control:{row}"))
        .into()
    }

    /// A row: its name, its line, its warning, a chip, its control.
    pub(super) fn screen_row<'a>(
        &self,
        id: &'static str,
        name: &'static str,
        line: Option<String>,
        warn: Option<String>,
        chip: Option<El<'a>>,
        control: El<'a>,
    ) -> El<'a> {
        let inert = self.screens_row_inert(id);
        let alpha = if inert && !matches!(id, PRIMARY) {
            INERT
        } else {
            1.0
        };
        let mut title = row![
            text(name)
                .size(14)
                .font(SEMIBOLD)
                .color(fade(TEXT.get(), alpha))
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if let Some(chip) = chip {
            title = title.push(chip);
        }
        let mut words = column![title].spacing(2).width(Length::Fill);
        if let Some(line) = line.filter(|l| !l.is_empty()) {
            words = words.push(t(line, 12.5, fade(DIM.get(), alpha)));
        }
        if let Some(warn) = warn {
            words = words.push(t(warn, 12.5, PEACH.get()));
        }
        let lit = self.lit(id);
        container(
            row![
                words,
                row![Space::new().height(36), control].align_y(Alignment::Center)
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
        .id(format!("row:{id}"))
        .into()
    }

    // --- Over the page ------------------------------------------------------------------

    /// The page's open menu, under its button.
    pub(in crate::app) fn screens_menu_list(&self) -> Option<El<'_>> {
        let (row, since) = self.screens.menu?;
        let anchor = if row == SETS {
            let index = self.screens_list().and_then(|k| {
                k.sets
                    .iter()
                    .position(|s| Some(&s.name) == self.screens.set_menu.as_ref())
            })?;
            format!("control:set:{index}")
        } else {
            format!("control:{row}")
        };
        let at = *self.places.at.get(&anchor)?;
        let current = self.screens_current(row);
        let opened = if self.reduced_motion {
            1.0
        } else {
            crate::app::view::ease_out(
                self.now.saturating_duration_since(since),
                crate::app::MENU_IN,
            )
        };
        let width = at.width.max(200.0);
        let mut list = column![];
        for (i, (choice, label)) in self.screens_choices(row).into_iter().enumerate() {
            let chosen = current.as_ref() == Some(&choice);
            let check: El<'_> = if chosen {
                icon(Icon::Check, 13.0, fade(ACCENT.get(), opened))
            } else {
                Space::new().width(13).into()
            };
            list = list.push(
                container(
                    button(
                        row![
                            container(check).width(18).center_x(18),
                            t(label, 13.5, fade(TEXT.get(), opened)).font(if chosen {
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
                    .on_press(Message::Screens(Msg::Choose(row, choice)))
                    .style(flat(
                        (i == self.screens.menu_index)
                            .then_some(crate::theme::ink(rgba(255, 255, 255, 0.07))),
                        crate::theme::ink(rgba(255, 255, 255, 0.07)),
                        7.0,
                        false,
                    )),
                )
                .id(format!("option:{i}")),
            );
        }
        let panel = container(list)
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
        // Under its button, or over it when the window's bottom is near.
        let height = 10.0 + 32.0 * self.screens_choices(row).len() as f32;
        let bottom = self
            .places
            .at
            .get(&format!(
                "sidebar:{}",
                if self.lang.fr() { "fr" } else { "en" }
            ))
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
                    .on_press(Message::Screens(Msg::CloseMenu)),
                pin(panel).x(x).y(y),
            ]
            .into(),
        )
    }

    /// "Garder ?", over the window, while a mode is tried.
    pub(in crate::app) fn screens_dialog(&self) -> Option<El<'_>> {
        let trial = self.screens.trial.as_ref()?;
        let lang = self.lang;
        let screen = self.screens_list().and_then(|k| k.get(&trial.output));
        let title = screen
            .map(|s| screens::lower_first(&s.title(lang)))
            .unwrap_or_default();
        let now = say_mode_text(&trial.mode, lang);
        let then = say_mode_text(&trial.before, lang);
        let left = trial.left;
        let question = match (trial.what, lang) {
            (RATE, Lang::French) => "Garder cette fréquence ?",
            (RATE, Lang::English) => "Keep this refresh rate?",
            (_, Lang::French) => "Garder cette résolution ?",
            (_, Lang::English) => "Keep this resolution?",
        };
        let body = match lang {
            Lang::French => {
                let mut s = format!(
                    "L’{title} affiche maintenant {now}. Sans réponse, il revient à {then} dans {left} s."
                );
                if let Some(first) = s.get(..1) {
                    s = first.to_uppercase() + &s[1..];
                }
                s
            }
            Lang::English => format!(
                "The {title} now shows {now}. Without an answer it goes back to {then} in {left} s."
            ),
        };
        // Sizes and rates kept on one line.
        let body = body
            .replace(" × ", "\u{a0}×\u{a0}")
            .replace(" Hz", "\u{a0}Hz")
            .replace(" s.", "\u{a0}s.");
        let on_keep = trial.on_keep;
        let ring = canvas(Countdown {
            left,
            of: TRIAL_SECONDS,
        })
        .width(44)
        .height(44);
        let buttons = row![
            Space::new().width(Length::Fill),
            container(
                button(t(self.say(w("Revenir", "Revert")), 13.0, TEXT.get()).font(SEMIBOLD))
                    .height(32)
                    .padding([7, 14])
                    .on_press(Message::Screens(Msg::Revert))
                    .style(move |_, status| {
                        let mut style = solid(
                            crate::theme::ink(rgba(255, 255, 255, 0.08)),
                            TEXT.get(),
                            status,
                        );
                        if !on_keep {
                            style.border = ring_border(8.0);
                        }
                        style
                    }),
            )
            .id("dialog:revert".to_owned()),
            container(
                button(t(self.say(w("Garder", "Keep")), 13.0, DEEP.get()).font(BOLD))
                    .height(32)
                    .padding([7, 16])
                    .on_press(Message::Screens(Msg::Keep))
                    .style(move |_, status| {
                        let mut style = solid(ACCENT.get(), DEEP.get(), status);
                        if on_keep {
                            style.border =
                                border(8.0, 2.0, crate::theme::ink(rgba(255, 255, 255, 0.85)));
                        }
                        style
                    }),
            )
            .id("dialog:keep".to_owned()),
        ]
        .spacing(8);
        let dialog = container(
            column![
                row![
                    ring,
                    column![
                        text(question).size(17).font(BOLD).color(TEXT),
                        t(body, 13.0, SOFT.get()).line_height(1.5),
                        t(
                            self.say(w(
                                "Échap revient aussitôt. Si la fenêtre se ferme, l’écran revient aussi.",
                                "Esc goes back at once. If the window closes, the screen goes back too.",
                            )),
                            12.0,
                            FAINT.get(),
                        ),
                    ]
                    .spacing(6)
                    .width(Length::Fill),
                ]
                .spacing(16),
                buttons,
            ]
            .spacing(18),
        )
        .width(440)
        .padding(20)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(MENU.get())),
            border: border(14.0, 1.0, RIM_12.get()),
            shadow: shadow(),
            ..container::Style::default()
        })
        .id("dialog".to_owned());
        Some(
            stack![
                mouse_area(
                    container(Space::new().width(Length::Fill).height(Length::Fill)).style(boxed(
                        crate::theme::ink(rgba(0, 0, 0, 0.58)),
                        0.0,
                        None
                    ))
                )
                .on_press(Message::Nothing),
                container(dialog)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center(Length::Fill),
            ]
            .into(),
        )
    }

    /// What the page shows, for the log.
    pub(in crate::app) fn screens_shown(&self) -> serde_json::Value {
        let lang = self.lang;
        let drawn: Vec<serde_json::Value> = self
            .drawn()
            .into_iter()
            .map(|d| {
                serde_json::json!({
                    "name": d.name,
                    "number": d.number,
                    "at": [d.rect.x.round(), d.rect.y.round(), d.rect.width.round(), d.rect.height.round()],
                    "selected": d.selected,
                    "state": d.dashed,
                    "title": d.title,
                    "line": d.line,
                    "bar": d.bar.is_some(),
                })
            })
            .collect();
        let rows: Vec<serde_json::Value> = [USE, RESOLUTION, RATE, SCALE, VRR, PRIMARY]
            .into_iter()
            .map(|row| {
                let value = match row {
                    USE => self.chosen().map(|s| s.enabled.to_string()),
                    VRR => self.chosen().map(|s| s.vrr.to_string()),
                    _ => {
                        let current = self.screens_current(row);
                        self.screens_choices(row)
                            .into_iter()
                            .find(|(c, _)| Some(c) == current.as_ref())
                            .map(|(_, l)| l)
                    }
                };
                serde_json::json!({ "id": row, "value": value, "inert": self.screens_row_inert(row) })
            })
            .collect();
        let choices: Option<Vec<String>> = self.screens.menu.map(|(row, _)| {
            self.screens_choices(row)
                .into_iter()
                .map(|(_, l)| l)
                .collect()
        });
        let chosen = self.chosen();
        serde_json::json!({
            "known": self.screens.known.is_some(),
            "unavailable": self.screens.unavailable,
            "drawn": drawn,
            "chosen": chosen.map(|s| s.name.clone()),
            "chosen_title": chosen.map(|s| s.title(lang)),
            "rows": rows,
            "menu": self.screens.menu.map(|(row, _)| row),
            "choices": choices,
            "dialog": self.screens.trial.as_ref().map(|t| serde_json::json!({
                "id": t.id, "output": t.output, "mode": t.mode, "left": t.left, "keep_focused": t.on_keep,
            })),
            "dragging": self.screens.held.as_ref().is_some_and(|h| h.dragging),
            "identify": self.screens.identify.is_some(),
            "notes": self.screens_list().map_or(0, |_| self.notes().len()),
        })
    }
}
