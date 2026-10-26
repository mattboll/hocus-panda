//! Leaving the session and sleeping (#72). Logging out, restarting and
//! shutting down show a card that counts ten seconds down; Escape or Cancel
//! stops it. Applications holding shutting down back (logind inhibitors)
//! stop the countdown: the card says who and why, and waits for a decision.
//! Then the session is saved, windows are asked to close, and once they are
//! gone (or the user goes on anyway) the session ends or logind restarts or
//! shuts the machine down. Suspending and hibernating go at once; the lock
//! before sleep is logind's PrepareForSleep (#30).
//!
//! Only the session itself asks logind; nested or headless, the event
//! stream says what would happen.

use std::time::Duration;

use hocus_api::Event;
use hocus_platform::power::{self, Capabilities, Inhibitor, Power};
use hocus_shell::leave::{Leave, LeaveChoice, Leaving, Stage};
use smithay::{
    reexports::calloop::{
        LoopHandle, RegistrationToken, channel,
        timer::{TimeoutAction, Timer},
    },
    utils::{Logical, Point},
};

use crate::{App, State, shell::Panel, surface};

/// How long the countdown lasts.
const COUNTDOWN: Duration = Duration::from_secs(10);
/// How long windows get to close before the card says which stay.
const CLOSING: Duration = Duration::from_secs(10);

/// What logind answered, back on the loop.
#[derive(Debug)]
enum News {
    Caps(Capabilities),
    Blockers(Vec<Inhibitor>),
    Done(Power, Result<(), String>),
}

/// Leaving, while its card shows.
#[derive(Debug)]
pub struct Card {
    output: String,
    panel: Panel<Leave>,
    /// When the countdown or the closing ends, on the compositor's clock.
    until: Option<Duration>,
    /// Wakes the loop once a second while the card shows.
    tick: Option<RegistrationToken>,
}

/// What leaving needs between turns of the loop.
#[derive(Debug, Default)]
pub struct Leaver {
    pub card: Option<Card>,
    pub caps: Capabilities,
    /// logind told them, for the test harness (#235).
    pub(crate) caps_known: bool,
    news: Option<channel::Sender<News>>,
}

/// Follows logind's answers on the loop, and asks what the machine can do.
pub fn start(handle: &LoopHandle<'static, App>, state: &mut State) {
    let (news, received) = channel::channel::<News>();
    let inserted = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg(news) = event {
            app.timed("leave news", |app| app.state.leave_news(news));
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "could not follow logind for leaving");
        return;
    }
    if state.is_session || state.system_bus.is_some() {
        let sender = news.clone();
        power::capabilities(state.system_bus.clone(), move |caps| {
            let _ = sender.send(News::Caps(caps));
        });
    }
    state.leaver.news = Some(news);
}

impl State {
    fn asks_logind(&self) -> bool {
        self.is_session || self.system_bus.is_some()
    }

    fn leave_news(&mut self, news: News) {
        match news {
            // The column shows hibernating only where it works.
            News::Caps(caps) => {
                self.leaver.caps = caps;
                self.leaver.caps_known = true;
            }
            News::Blockers(all) => {
                let held: Vec<(String, String)> = all
                    .into_iter()
                    .filter(Inhibitor::blocks_shutdown)
                    .map(|i| (i.who, i.why))
                    .collect();
                if held.is_empty() {
                    return;
                }
                if let Some(card) = &mut self.leaver.card
                    && matches!(card.panel.view.stage(), Stage::CountingDown(_))
                {
                    card.until = None;
                    card.panel.view.set_stage(Stage::HeldBack(held));
                    self.needs_redraw = true;
                }
            }
            News::Done(power, Err(err)) => {
                tracing::warn!(?power, err, "logind refused");
            }
            News::Done(..) => {}
        }
    }

    pub fn leave_open(&self) -> bool {
        self.leaver.card.is_some()
    }

    /// Logging out, restarting or shutting down: the card, its countdown,
    /// and the question to logind.
    pub fn begin_leaving(&mut self, what: Leaving) {
        let Some(output) = self
            .model
            .primary()
            .and_then(|o| self.find_output(&o.name.0))
            .or_else(|| self.output_under_pointer())
        else {
            return;
        };
        let Some(g) = self.space.output_geometry(&output) else {
            return;
        };
        let seconds = COUNTDOWN.as_secs() as u32;
        let view = Leave::new(
            self.theme(),
            self.shell_lang(),
            hocus_ui::Size::new(g.size.w as f32, g.size.h as f32),
            output.current_scale().fractional_scale() as f32,
            what,
            Stage::CountingDown(seconds),
        );
        let tick = self.waker().and_then(|handle| {
            handle
                .insert_source(Timer::from_duration(Duration::from_secs(1)), |_, _, app| {
                    app.state.needs_redraw = true;
                    TimeoutAction::ToDuration(Duration::from_secs(1))
                })
                .ok()
        });
        if let Some(old) = self.leaver.card.take() {
            self.drop_card(old);
        }
        self.leaver.card = Some(Card {
            output: output.name(),
            panel: Panel::new(view),
            until: Some(self.clock.now() + COUNTDOWN),
            tick,
        });
        if self.asks_logind()
            && let Some(news) = self.leaver.news.clone()
        {
            power::inhibitors(self.system_bus.clone(), move |all| {
                let _ = news.send(News::Blockers(all));
            });
        }
        self.emit(|_| Event::Leaving {
            action: name(what).into(),
            stage: "countdown".into(),
        });
        self.needs_redraw = true;
    }

    /// Sleep now (locked first through logind's PrepareForSleep).
    pub fn sleep_now(&mut self, power: Power) {
        let action = match power {
            Power::Hibernate => "hibernate",
            _ => "suspend",
        };
        self.emit(|_| Event::Leaving {
            action: action.into(),
            stage: "now".into(),
        });
        if !self.asks_logind() {
            tracing::info!(action, "not the session: nothing sleeps");
            return;
        }
        self.ask_logind(power);
    }

    fn ask_logind(&self, power: Power) {
        let news = self.leaver.news.clone();
        power::act(self.system_bus.clone(), power, move |result| {
            if let Some(news) = news {
                let _ = news.send(News::Done(power, result));
            }
        });
    }

    fn drop_card(&mut self, card: Card) {
        if let (Some(token), Some(handle)) = (card.tick, &self.loop_handle) {
            handle.remove(token);
        }
        self.needs_redraw = true;
    }

    fn leave_chose(&mut self, choice: Option<LeaveChoice>) {
        match choice {
            Some(LeaveChoice::Cancel) => {
                if let Some(card) = self.leaver.card.take() {
                    let what = card.panel.view.what();
                    self.drop_card(card);
                    self.emit(|_| Event::Leaving {
                        action: name(what).into(),
                        stage: "cancelled".into(),
                    });
                }
            }
            Some(LeaveChoice::Go) => self.leave_go(),
            None => {}
        }
    }

    /// After the countdown, or "now", or "anyway".
    fn leave_go(&mut self) {
        let Some(card) = &mut self.leaver.card else {
            return;
        };
        let what = card.panel.view.what();
        let anyway = matches!(card.panel.view.stage(), Stage::Closing(open) if !open.is_empty());
        if !anyway && !matches!(card.panel.view.stage(), Stage::Closing(_)) {
            // Saved first: closing windows takes them out of the session.
            card.panel.view.set_stage(Stage::Closing(Vec::new()));
            card.until = Some(self.clock.now() + CLOSING);
            self.save_session_now();
            for managed in self.windows.values() {
                surface::close(&managed.window);
            }
            self.emit(|_| Event::Leaving {
                action: name(what).into(),
                stage: "closing".into(),
            });
            self.needs_redraw = true;
            if !self.windows.is_empty() {
                return;
            }
        }
        self.leave_finish(what);
    }

    fn leave_finish(&mut self, what: Leaving) {
        if let Some(card) = self.leaver.card.take() {
            self.drop_card(card);
        }
        self.emit(|_| Event::Leaving {
            action: name(what).into(),
            stage: "now".into(),
        });
        match what {
            Leaving::LogOut => {
                tracing::info!("logging out");
                self.loop_signal.stop();
            }
            Leaving::Restart | Leaving::ShutDown if self.asks_logind() => {
                self.ask_logind(if what == Leaving::Restart {
                    Power::Reboot
                } else {
                    Power::PowerOff
                });
            }
            Leaving::Restart | Leaving::ShutDown => {
                tracing::info!(action = name(what), "not the session: the machine stays on");
            }
        }
    }

    /// After each dispatch: the countdown, and windows closing.
    pub fn refresh_leaving(&mut self) {
        let now = self.clock.now();
        let open: Vec<String> = self
            .windows
            .values()
            .map(|m| surface::title_and_app_id(&m.window))
            .map(|(title, app)| title.or(app).unwrap_or_default())
            .collect();
        let Some(card) = &mut self.leaver.card else {
            return;
        };
        let what = card.panel.view.what();
        let mut finish = false;
        let mut go = false;
        match (card.panel.view.stage().clone(), card.until) {
            (Stage::CountingDown(_), Some(until)) => {
                let left = until.saturating_sub(now);
                if left.is_zero() {
                    go = true;
                } else {
                    card.panel
                        .view
                        .set_stage(Stage::CountingDown(left.as_secs_f64().ceil() as u32));
                }
            }
            (Stage::Closing(shown), until) if shown.is_empty() => {
                if open.is_empty() {
                    finish = true;
                } else if until.is_some_and(|u| now >= u) {
                    // Some stay: the card says which, and waits.
                    card.until = None;
                    card.panel.view.set_stage(Stage::Closing(open));
                }
            }
            (Stage::Closing(_), _) if open.is_empty() => finish = true,
            _ => {}
        }
        let drawn = card.panel.view.draw(now);
        if card.panel.upload(&drawn) {
            self.needs_redraw = true;
        }
        if go {
            self.leave_go();
        } else if finish {
            self.leave_finish(what);
        }
    }

    /// The card takes the pointer while it shows.
    pub fn leave_pointer(&mut self, at: Point<f64, Logical>, pressed: Option<bool>) -> bool {
        let Some(card) = self.leaver.card.as_ref() else {
            return false;
        };
        let Some(g) = self
            .find_output(&card.output)
            .and_then(|o| self.space.output_geometry(&o))
        else {
            return false;
        };
        let local = hocus_ui::Point::new(
            (at.x - f64::from(g.loc.x)) as f32,
            (at.y - f64::from(g.loc.y)) as f32,
        );
        let Some(card) = self.leaver.card.as_mut() else {
            return false;
        };
        card.panel.view.pointer_move(local);
        let choice = pressed.and_then(|p| card.panel.view.pointer_button(p));
        self.leave_chose(choice);
        self.needs_redraw = true;
        true
    }

    /// The card takes the keys while it shows.
    pub fn leave_key(&mut self, sym: smithay::input::keyboard::Keysym) -> bool {
        let Some(card) = self.leaver.card.as_mut() else {
            return false;
        };
        let choice = crate::launcher::key_of(sym).and_then(|k| card.panel.view.key(k));
        self.leave_chose(choice);
        self.needs_redraw = true;
        true
    }

    pub fn leave_elements<R>(
        &self,
        renderer: &mut R,
        output: &smithay::output::Output,
    ) -> Option<smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement<R>>
    where
        R: smithay::backend::renderer::Renderer + smithay::backend::renderer::ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let card = self.leaver.card.as_ref()?;
        if card.output != output.name() {
            return None;
        }
        card.panel.element(renderer, output, Point::default())
    }

    /// Its panel on glass, on the output it shows on.
    pub(crate) fn leave_glass(&self, output: &smithay::output::Output) -> Vec<crate::blur::Spot> {
        match self.leaver.card.as_ref() {
            Some(card) if card.output == output.name() => crate::blur::spots(
                crate::blur::Surface::Leave,
                Default::default(),
                &card.panel.view.glass(),
            ),
            _ => Vec::new(),
        }
    }

    pub fn leave_ui(&self, out: &mut Vec<hocus_api::UiNode>) {
        let Some(card) = &self.leaver.card else {
            return;
        };
        if let Some(g) = self
            .find_output(&card.output)
            .and_then(|o| self.space.output_geometry(&o))
        {
            crate::shell::flatten(&card.panel.view.access(), g.loc, 0, out);
        }
    }
}

fn name(what: Leaving) -> &'static str {
    match what {
        Leaving::LogOut => "log-out",
        Leaving::Restart => "restart",
        Leaving::ShutDown => "shut-down",
    }
}
