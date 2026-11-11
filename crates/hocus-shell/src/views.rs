//! What each of the shell's surfaces shows: the bar's pieces, the
//! popovers, the launcher, the capture's surfaces.

use super::*;

impl Shell {
    /// The clock of the bar whose clock's surface is `id`.
    pub(super) fn clock_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.clock == id)?;
        let at = self.now + AHEAD;
        let forms = bar.forms(at);
        let (width, height) = bar.clock_surface;
        let backdrop = (!self.look.glass).then(|| {
            // The bar's gradient where the clock is, which changes by less
            // than a level over the clock's height.
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = clock::SURFACE_TOP as f32 + height as f32 / 2.0;
            mix(
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = clock::Clock {
            bar: bar.output.clone(),
            style: self.clock,
            motion: self.look.motion,
            wall: self.wall.clone(),
            lang: self.lang,
            forms,
            bar_width: bar.width(at),
            surface_width: width as f32,
            backdrop,
            locked: self.lock.read(lock::Locked::active).unwrap_or(false),
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The levels of the bar whose levels' surface is `id`.
    pub(super) fn levels_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.levels == id)?;
        let at = self.now + AHEAD;
        let (_, height) = bar.levels_surface;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = bar_height - levels::draw::BOTTOM as f32 - height as f32 / 2.0;
            mix(
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = levels::draw::Levels {
            forms: bar.forms(at),
            bar_width: bar.width(at),
            motion: self.look.motion,
            words: self.words,
            backdrop,
            bar: bar.output.clone(),
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The connections of the bar whose connections' surface is `id`.
    pub(super) fn connections_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.connections == id)?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = bar_height - levels::draw::BOTTOM as f32 - 160.0;
            mix(
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = network::draw::Connections {
            forms: bar.forms(at),
            bar_width: bar.width(at),
            words: self.words,
            backdrop,
            bar: bar.output.clone(),
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The desktops of the bar whose desktops' surface is `id`.
    pub(super) fn desktops_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.desktops == id)?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle =
                bar_height - levels::draw::BOTTOM as f32 - desktops::draw::middle(&bar.output);
            mix(
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = desktops::draw::Tiles {
            forms: bar.forms(at),
            bar_width: bar.width(at),
            motion: self.look.motion,
            words: self.words,
            backdrop,
            bar: bar.output.clone(),
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The agents' group of the bar whose agents' surface is `id`.
    pub(super) fn agents_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.agents == id)?;
        let at = self.now + AHEAD;
        // The bar's colour where the column puts the group, read as it draws.
        let backdrop = (!self.look.glass).then_some(agents::draw::Backdrop {
            top: OPAQUE_TOP.get(),
            bottom: OPAQUE_BOTTOM.get(),
        });
        let program = agents::draw::Group {
            forms: bar.forms(at),
            bar_width: bar.width(at),
            words: agents::Words::new(clock::time::Lang::of_messages()),
            backdrop,
            bar: bar.output.clone(),
            motion: self.look.motion,
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The to-do list of the bar whose to-do list's surface is `id`.
    pub(super) fn tasks_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.tasks == id)?;
        let at = self.now + AHEAD;
        // The bar's colour where the column puts the list, read as it draws.
        let backdrop = (!self.look.glass).then_some(agents::draw::Backdrop {
            top: OPAQUE_TOP.get(),
            bottom: OPAQUE_BOTTOM.get(),
        });
        let program = tasks::draw::Group {
            forms: bar.forms(at),
            bar_width: bar.width(at),
            words: tasks::Words::new(clock::time::Lang::of_messages()),
            backdrop,
            bar: bar.output.clone(),
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The footer of the bar whose footer's surface is `id`.
    pub(super) fn footer_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.footer == id)?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = bar_height - 40.0;
            mix(
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = footer::Footer {
            bar: bar.output.clone(),
            forms: bar.forms(at),
            bar_width: bar.width(at),
            words: footer::Words::new(clock::time::Lang::of_messages()),
            backdrop,
        };
        Some(in_bar(
            bar,
            at,
            canvas(ui::items::surface(program))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The sign of what is in use (5.12), if its surface is `id`.
    pub(super) fn in_use_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let output = in_use::bar_of(id)?;
        let bar = self.bars.values().find(|b| b.output == output)?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = column::TOP;
            mix(
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = in_use::draw::Sign {
            bar: bar.output.clone(),
            forms: bar.forms(at),
            bar_width: bar.width(at),
            backdrop,
            motion: self.look.motion,
            words: in_use::Words::new(clock::time::Lang::of_messages()),
        };
        Some(in_bar(
            bar,
            at,
            canvas(ui::items::surface(program))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The next meeting (Day.2), if its surface is `id`.
    pub(super) fn meeting_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let output = meeting::bar_of(id)?;
        let bar = self.bars.values().find(|b| b.output == output)?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = column::TOP;
            mix(
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = meeting::draw::View {
            bar: bar.output.clone(),
            forms: bar.forms(at),
            bar_width: bar.width(at),
            backdrop,
            motion: self.look.motion,
            words: meeting::Words::new(clock::time::Lang::of_messages()),
        };
        Some(in_bar(
            bar,
            at,
            canvas(ui::items::surface(program))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The windows over the desktops (#604), if their surface is `id`.
    pub(super) fn windows_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let output = windows::bar_of(id)?;
        let bar = self.bars.values().find(|b| b.output == output)?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then_some(agents::draw::Backdrop {
            top: OPAQUE_TOP.get(),
            bottom: OPAQUE_BOTTOM.get(),
        });
        let program = windows::draw::View {
            bar: bar.output.clone(),
            forms: bar.forms(at),
            bar_width: bar.width(at),
            words: windows::Words::new(clock::time::Lang::of_messages()),
            backdrop,
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The music player's tile (s12), if its surface is `id`.
    pub(super) fn music_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let output = music::bar_of(id)?;
        let bar = self.bars.values().find(|b| b.output == output)?;
        // The bar's colour behind it, where the column puts it.
        let backdrop = (!self.look.glass).then(|| {
            let middle = music::placed(&output).map_or(0.0, |(top, _)| top + music::TILE / 2.0);
            agents::draw::Backdrop {
                top: OPAQUE_TOP.get(),
                bottom: OPAQUE_BOTTOM.get(),
            }
            .at(middle, bar.height)
        });
        Some(music::tile::view(&output, self.look.motion, backdrop))
    }

    /// Today (Day.3), if its surface is `id`.
    pub(super) fn today_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let output = today::bar_of(id)?;
        let bar = self.bars.values().find(|b| b.output == output)?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = today::shown(&output).1 + 40.0;
            mix(
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = today::draw::View {
            bar: bar.output.clone(),
            forms: bar.forms(at),
            backdrop,
            motion: self.look.motion,
            words: today::Words::new(clock::time::Lang::of_messages()),
        };
        Some(in_bar(
            bar,
            at,
            canvas(ui::items::surface(program))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The tray of the bar whose tray's surface is `id` (5.14).
    pub(super) fn tray_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self
            .bars
            .values()
            .find(|b| tray::draw::surface(&b.output) == Some(id))?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = bar_height - 100.0;
            mix(
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = tray::draw::Tray {
            id,
            forms: bar.forms(at),
            bar_width: bar.width(at),
            motion: self.look.motion,
            backdrop,
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The authentication dialog told `told`: what it does, and its next
    /// tick (a surface faded out, the next request), if one is due.
    pub(super) fn auth_told(&mut self, told: auth::Told) -> Task<Message> {
        let effects = self.auth.update(told, self.now);
        let due = self.auth.get_mut().due();
        Self::with_tick(effects, due, || surfaces::Asked::Auth(auth::Come::Tick))
    }

    /// The panda peeking out (4.5), if its surface is `id`.
    pub(super) fn peek_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = agents::peek::bar_of(id)?;
        Some(agents::peek::view(id, bar))
    }

    /// The launcher, or its toast, if its surface is `id`.
    pub(super) fn launcher_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        if launcher::toast::is_surface(id) {
            return Some(
                canvas(launcher::toast::View {
                    id,
                    software: self.software,
                })
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            );
        }
        if !launcher::is_surface(id) {
            return None;
        }
        Some(launcher::view::view(
            id,
            launcher::Words::new(clock::time::Lang::of_messages()),
            self.software,
        ))
    }

    /// The actions palette told `come`: what it does, and its next tick
    /// (a surface faded out, what was chosen carried out), if one is due.
    pub(super) fn actions_told(&mut self, come: actions::Come) -> Task<Message> {
        let effects = self.actions.update(come, self.now);
        let due = self.actions.get_mut().due();
        Self::with_tick(effects, due, || {
            surfaces::Asked::Actions(actions::Come::Tick)
        })
    }

    /// `effects` carried out, and a tick asked for `due` if there is one.
    pub(super) fn with_tick(
        effects: Vec<popover::Effect>,
        due: Option<Instant>,
        tick: impl FnOnce() -> surfaces::Asked + Send + 'static,
    ) -> Task<Message> {
        let tick = due.map_or_else(Task::none, |at| {
            Task::perform(tokio::time::sleep_until(at.into()), move |()| {
                Message::Surface(tick())
            })
        });
        Task::batch(
            popover::messages(effects)
                .into_iter()
                .map(Task::done)
                .chain([tick]),
        )
    }

    /// The emoji picker told `come`: what it does, and its next tick (a
    /// surface faded out), if one is due.
    pub(super) fn emoji_told(&mut self, come: emoji::Come) -> Task<Message> {
        let effects = self.emoji.update(come, self.now);
        let due = self.emoji.get_mut().due();
        Self::with_tick(effects, due, || surfaces::Asked::Emoji(emoji::Come::Tick))
    }

    /// The capture's toolbar (5.8), its thumbnail or its editor (5.9), if
    /// its surface is `id`.
    pub(super) fn capture_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        if capture::thumb::is_surface(id) {
            return Some(capture::thumb::view(id, capture::words(), self.software));
        }
        if capture::editor::is_surface(id) {
            return Some(capture::editor::view::view(
                id,
                capture::words(),
                self.look.glass,
            ));
        }
        if !capture::is_surface(id) {
            return None;
        }
        Some(capture::toolbar::view(
            id,
            capture::Words::new(clock::time::Lang::of_messages()),
            self.look.glass,
        ))
    }

    /// The popover whose surface is `id`.
    pub(super) fn popover_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let item = popover::with(|c| c.item_of(id))?;
        let panel: Element<'_, Message> = match item {
            popover::Item::Sound => popover::sound::view(id, self.words, self.look.glass),
            popover::Item::Wifi => popover::wifi::view(&popover::wifi::Panel {
                id,
                words: self.words,
                glass: self.look.glass,
                motion: self.look.motion,
            }),
            popover::Item::Music => popover::music::view(id, self.look.glass),
            popover::Item::Bluetooth => popover::bluetooth::view(&popover::bluetooth::Panel {
                id,
                words: self.words,
                glass: self.look.glass,
                motion: self.look.motion,
            }),
            popover::Item::Brightness => popover::brightness::view(id, self.words, self.look.glass),
            popover::Item::Battery => popover::battery::view(id, self.words, self.look.glass),
            popover::Item::Windows => popover::windows::view(&popover::windows::Panel {
                id,
                words: windows::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
            }),
            popover::Item::WindowMenu(window) => {
                popover::window_menu::view(&popover::window_menu::Panel {
                    id,
                    window,
                    words: popover::window_menu::Words::new(clock::time::Lang::of_messages()),
                    glass: self.look.glass,
                })
            }
            popover::Item::Desktop { .. } | popover::Item::WindowsMore => {
                popover::desktop::view(&popover::desktop::Panel {
                    id,
                    words: self.words,
                    glass: self.look.glass,
                })
            }
            popover::Item::Sources | popover::Item::AnotherTool | popover::Item::Plug(_) => {
                popover::agents::view(&popover::agents::Panel {
                    id,
                    words: agents::Words::new(clock::time::Lang::of_messages()),
                    glass: self.look.glass,
                })
            }
            popover::Item::Secretary => popover::briefing::view(&popover::briefing::Panel {
                id,
                words: agents::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
            }),
            popover::Item::Notifications => popover::history::view(&popover::history::Panel {
                id,
                glass: self.look.glass,
                motion: self.look.motion,
            }),
            popover::Item::Clipboard => popover::clipboard::view(&popover::clipboard::Panel {
                id,
                words: popover::clipboard::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
            }),
            popover::Item::InUse => popover::in_use::view(&popover::in_use::Panel {
                id,
                words: in_use::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
            }),
            popover::Item::Tasks | popover::Item::TasksMore => {
                popover::tasks::view(&popover::tasks::Panel {
                    id,
                    words: tasks::Words::new(clock::time::Lang::of_messages()),
                    glass: self.look.glass,
                })
            }
            popover::Item::DayChoose => popover::setup::view(&popover::setup::Panel {
                id,
                words: setup::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
                motion: self.look.motion && !self.software,
            }),
            popover::Item::MeetingMark => popover::meeting::view(&popover::meeting::Panel {
                id,
                words: meeting::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
            }),
            popover::Item::User => popover::session::view(&popover::session::Panel {
                id,
                words: popover::session::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
                motion: self.look.motion && !self.software,
            }),
            popover::Item::Calendar => popover::calendar::view(&popover::calendar::Panel {
                id,
                glass: self.look.glass,
            }),
            // They open none.
            popover::Item::Tray(key) => popover::tray::view(&popover::tray::Panel {
                id,
                key,
                glass: self.look.glass,
            }),
            popover::Item::Keyboard
            | popover::Item::Decline
            | popover::Item::Agent(_)
            | popover::Item::Menu
            | popover::Item::Meeting
            | popover::Item::Join
            | popover::Item::TodayLine(_)
            | popover::Item::TodayMore
            | popover::Item::TaskAdd
            | popover::Item::Task(_)
            | popover::Item::DayLater
            | popover::Item::Window(_)
            | popover::Item::WindowClose(_) => {
                return None;
            }
        };
        Some(panel)
    }

    /// A surface's widgets, whose pointer's shape is set again each time
    /// the pointer enters it ([`pointer`]), which get the keys the
    /// compositor gives their surface ([`keys`]), and an input method while
    /// a field of theirs has the keyboard ([`field::ime`]).
    pub(super) fn view(&self, id: window::Id) -> Element<'_, Message> {
        ui::surface::viewing();
        keys::Held::new(
            id,
            field::ime::Asking::new(pointer::Entered::new(self.surface_view(id))),
            Message::Keys,
        )
        .into()
    }

    /// What every surface's view is built with.
    pub(super) fn context(&self) -> surfaces::Context {
        surfaces::Context {
            look: self.look,
            wall: self.wall.clone(),
            lang: self.lang,
        }
    }

    pub(super) fn surface_view(&self, id: window::Id) -> Element<'_, Message> {
        let Some(bar) = self.bars.get(&id) else {
            let context = self.context();
            return self
                .clock_view(id)
                .or_else(|| self.levels_view(id))
                .or_else(|| self.connections_view(id))
                .or_else(|| self.desktops_view(id))
                .or_else(|| self.agents_view(id))
                .or_else(|| self.tasks_view(id))
                .or_else(|| self.footer_view(id))
                .or_else(|| self.tray_view(id))
                .or_else(|| tray::tip::view(id))
                .or_else(|| self.peek_view(id))
                .or_else(|| self.launcher_view(id))
                .or_else(|| self.emoji.view(id, &context))
                .or_else(|| self.actions.view(id, &context))
                .or_else(|| self.capture_view(id))
                .or_else(|| self.auth.view(id, &context))
                .or_else(|| self.share.view(id, &context))
                .or_else(|| self.lock.view(id, &context))
                .or_else(|| self.curtain.view(id, &context))
                .or_else(|| self.in_use_view(id))
                .or_else(|| self.meeting_view(id))
                .or_else(|| self.today_view(id))
                .or_else(|| self.windows_view(id))
                .or_else(|| self.music_view(id))
                .or_else(|| self.popover_view(id))
                .or_else(|| notifications::view(id, self.look.glass))
                .or_else(|| reminder::view(id, self.look.glass))
                .unwrap_or_else(|| Space::new().into());
        };
        let width = bar.width(self.now + AHEAD);
        let output = bar.output.clone();
        let (top, bottom) = if self.look.glass {
            (GLASS_TOP, GLASS_BOTTOM)
        } else {
            (OPAQUE_TOP, OPAQUE_BOTTOM)
        };
        let marks = canvas(theme::BarMarks)
            .width(Length::Fill)
            .height(Length::Fill);
        let column = container(marks)
            .width(width)
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Gradient(
                    gradient::Linear::new(iced::Degrees(180.0))
                        .add_stop(0.0, top.get())
                        .add_stop(1.0, bottom.get())
                        .into(),
                )),
                border: Border {
                    color: RIM.get(),
                    width: theme::border(),
                    radius: (RADIUS as f32).into(),
                },
                ..container::Style::default()
            });
        let bar = container(if side::left() {
            row![column, space::horizontal()]
        } else {
            row![space::horizontal(), column]
        })
        .width(Length::Fill)
        .height(Length::Fill);
        let pointer = canvas(popover::BarPointer { bar: output })
            .width(Length::Fill)
            .height(Length::Fill);
        stack![bar, pointer].into()
    }
}
