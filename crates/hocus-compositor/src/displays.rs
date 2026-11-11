//! The screens' settings (6.9, docs/design/settings.md "Écrans",
//! docs/design/windows.md §12): each screen's mode, place, scale, VRR and
//! whether it is on, as `[outputs.NAME]` says, applied as the file changes;
//! the Settings' trials of a mode, which the compositor takes back by
//! itself after 15 s unless kept; and what the Settings are told of the
//! screens.
//!
//! - **The file, followed**: the configuration's one watcher
//!   ([`crate::reload`]) hands the loop what the file says; the screens
//!   take `[outputs]` and `[panel] output` from it. A file that does not
//!   read changes nothing.
//! - **The layout**, after the turn that asked for it, with the backend at
//!   hand: the modes asked (or each screen's preferred one at its highest
//!   rate), the scales, VRR; then the places: the file's when every screen
//!   on has one and they neither overlap nor leave one apart, else where
//!   the backend put them while nothing changed, else left to right in the
//!   connectors' order, tops aligned. Windows go along with their screen;
//!   a screen turned off sends its windows to another, as one unplugged.
//! - **Trials** (`try NAME mode=WxH@HZ` on the Settings' line): applied
//!   without the file; the Settings are told the seconds left each second;
//!   `keep ID` keeps it (the Settings then write it), `revert ID`, 15 s on
//!   the motions' clock or the Settings' line closing take it back.
//! - **The Settings' line**: `msg screens` on the control socket keeps the
//!   connection open; the screens are written on it as one JSON line each
//!   time they change, then the trials' news. `msg outputs` answers the
//!   same JSON once.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

use smithay::{
    desktop::layer_map_for_output,
    output::{Mode, Output},
    reexports::calloop::{
        RegistrationToken, channel,
        timer::{TimeoutAction, Timer},
    },
};

use crate::{
    Config, State,
    backend::Backend,
    config::ModeWanted,
    model::{Rect, WindowId},
    windows::rect_of,
};

/// A mode: width, height (pixels), refresh rate (mHz).
pub type Mode3 = (i32, i32, i32);

/// How long a trial lasts unless kept.
pub const TRIAL: Duration = Duration::from_secs(15);

/// What the backend knows of a screen, kept on its `Output` as it lights.
#[derive(Debug, Clone)]
pub struct Facts {
    /// Its modes, each once, the largest first.
    pub modes: Vec<Mode3>,
    pub preferred: Mode3,
    pub vrr_capable: bool,
    /// Its size, millimetres (0 when unknown).
    pub millimetres: (i32, i32),
    /// The scale it takes by itself when the file says none (a headless
    /// output's, given); `None`: by its density.
    pub fixed_scale: Option<f64>,
    /// Where the backend put it, and its logical size then.
    pub placed: (i32, i32),
    pub placed_size: (i32, i32),
}

impl Facts {
    /// The mode it takes without one asked: its preferred size at its
    /// highest rate.
    pub fn default_mode(&self) -> Mode3 {
        self.modes
            .iter()
            .copied()
            .filter(|m| (m.0, m.1) == (self.preferred.0, self.preferred.1))
            .max_by_key(|m| m.2)
            .unwrap_or(self.preferred)
    }
}

/// `modes` each once, the largest first, the fastest first.
pub fn sorted_modes(mut modes: Vec<Mode3>) -> Vec<Mode3> {
    modes.sort_by(|a, b| {
        (i64::from(b.0) * i64::from(b.1), b.0, b.2).cmp(&(
            i64::from(a.0) * i64::from(a.1),
            a.0,
            a.2,
        ))
    });
    modes.dedup();
    modes
}

pub fn mode3(mode: Mode) -> Mode3 {
    (mode.size.w, mode.size.h, mode.refresh)
}

pub fn mode_of(m: Mode3) -> Mode {
    Mode {
        size: (m.0, m.1).into(),
        refresh: m.2,
    }
}

/// A screen unplugged this session: the Settings draw its place.
#[derive(Debug, Clone)]
struct Gone {
    name: String,
    built_in: bool,
    millimetres: (i32, i32),
    rect: Rect,
    /// When, seconds since 1970 (what the user reads).
    at: i64,
}

/// A mode tried.
#[derive(Debug, Clone)]
struct Trial {
    id: u64,
    output: String,
    mode: Mode3,
    before: Mode3,
    until: Duration,
    session: u64,
    /// The whole seconds left last told.
    told: u64,
}

/// A trial kept, until the file says that screen's mode again.
#[derive(Debug, Clone)]
struct Kept {
    output: String,
    mode: Mode3,
    file_then: Option<ModeWanted>,
}

/// A Settings' line.
#[derive(Debug)]
pub(crate) struct Session {
    id: u64,
    to: mpsc::Sender<String>,
}

/// What the Settings' lines say, from their threads.
#[derive(Debug)]
pub enum FromSettings {
    Opened {
        session: u64,
        to: mpsc::Sender<String>,
    },
    Line {
        session: u64,
        line: String,
    },
    Closed {
        session: u64,
    },
}

/// The screens' settings and what goes on with them.
#[derive(Debug, Default)]
pub struct Displays {
    /// Every screen plugged, on or off, in the connectors' order.
    pub outputs: Vec<Output>,
    /// Those turned off by the file.
    off: BTreeSet<String>,
    /// Those with VRR on.
    vrr_on: BTreeSet<String>,
    /// An off screen's place, as the file or the layout last had it.
    off_places: BTreeMap<String, (i32, i32)>,
    gone: Vec<Gone>,
    trial: Option<Trial>,
    kept: Option<Kept>,
    next_trial: u64,
    pub(crate) sessions: Vec<Session>,
    timer: Option<RegistrationToken>,
    /// The file's places were refused (overlapping, or a screen apart).
    places_refused: bool,
    /// The layout is to be applied after this turn.
    pub(crate) wanted: bool,
    /// The screens changed: the Settings are told after this turn.
    pub(crate) dirty: bool,
    told: Option<String>,
    /// The desktops' windows as the Settings were last told (6.14).
    desktops_told: Option<crate::desktop_set::Counts>,
}

static SESSIONS: AtomicU64 = AtomicU64::new(1);

impl State {
    /// The file changed and reads (`reload.rs`): its screens' settings
    /// and the primary screen apply now. Whether they changed.
    pub(crate) fn displays_config_changed(&mut self, config: &Config) -> bool {
        // A screen's wallpaper is the wallpaper's (6.3, wallpaper.rs), not
        // the layout's.
        let screens =
            |outputs: &std::collections::BTreeMap<String, crate::config::OutputSettings>| {
                outputs
                    .iter()
                    .map(|(name, o)| (name.clone(), o.screen_part()))
                    .filter(|(_, o)| *o != crate::config::OutputSettings::default())
                    .collect::<Vec<_>>()
            };
        let sets_changed = config.screen_sets != self.config.screen_sets;
        let outputs_changed =
            screens(&config.outputs) != screens(&self.config.outputs) || sets_changed;
        let primary_before = self.primary_wanted();
        let primary_changed = config.panel.output != self.config.panel.output
            || config
                .screen_set(&self.current_set())
                .and_then(|s| s.primary.as_ref())
                != self
                    .config
                    .screen_set(&self.current_set())
                    .and_then(|s| s.primary.as_ref());
        if !outputs_changed && !primary_changed {
            return false;
        }
        tracing::info!(
            outputs_changed,
            primary_changed,
            "the configuration changed: the screens' settings apply"
        );
        // A screen's bar form chosen in the file is the bar's own (6.4).
        self.config.outputs = config.outputs.clone();
        self.config.screen_sets = config.screen_sets.clone();
        if let Some(kept) = &self.displays.kept {
            let now = self.screen_settings(&kept.output).and_then(|o| o.mode);
            if now != kept.file_then {
                self.displays.kept = None;
            }
        }
        if primary_changed {
            self.config.panel.output = config.panel.output.clone();
            if self.primary_wanted() != primary_before {
                self.panel.home = None;
            }
        }
        self.displays.wanted = true;
        self.displays.dirty = true;
        true
    }

    /// `output` joined the desktop (the backend made it): its settings
    /// apply after this turn.
    pub(crate) fn display_added(&mut self, output: &Output) {
        let name = output.name();
        self.displays.gone.retain(|g| g.name != name);
        if !self.displays.outputs.contains(output) {
            self.screen_joined(output);
            self.displays.outputs.push(output.clone());
            self.displays.outputs.sort_by_key(Output::name);
        }
        self.displays.wanted = true;
        self.displays.dirty = true;
    }

    /// `output` was unplugged: its place is remembered for the Settings.
    pub(crate) fn display_removed(&mut self, output: &Output) {
        let name = output.name();
        let rect = self.space.output_geometry(output).map(rect_of).or_else(|| {
            let (x, y) = *self.displays.off_places.get(&name)?;
            let size = logical_size(output)?;
            Some(Rect::new(x, y, size.0, size.1))
        });
        self.displays.outputs.retain(|o| o != output);
        self.graphics.backgrounds.get_mut().forget(&name);
        self.graphics.walls.get_mut().forget(&name);
        self.displays.off.remove(&name);
        self.displays.vrr_on.remove(&name);
        if let Some(rect) = rect {
            let facts = output.user_data().get::<Facts>();
            self.displays.gone.retain(|g| g.name != name);
            self.displays.gone.push(Gone {
                name: name.clone(),
                built_in: crate::away::is_internal(output),
                millimetres: facts.map_or((0, 0), |f| f.millimetres),
                rect,
                at: wall_now(),
            });
        }
        if self
            .displays
            .trial
            .as_ref()
            .is_some_and(|t| t.output == name)
        {
            self.trial_end(false, "its screen went");
        }
        self.displays.wanted = true;
        self.displays.dirty = true;
    }

    /// The lid took the laptop's screen away, or gave it back: at once,
    /// as an unplug or a screen plugged back, so that its windows are
    /// where they go before anything else happens; the layout follows
    /// after the turn.
    pub(crate) fn lid_screens(&mut self) {
        for output in self.displays.outputs.clone() {
            let name = output.name();
            let takes = self.lid_takes(&output);
            let off = self.displays.off.contains(&name);
            if takes && !off && self.is_plugged(&output) {
                tracing::info!(
                    output = name,
                    "the lid closed: the screen goes as unplugged"
                );
                if let Some(g) = self.space.output_geometry(&output) {
                    self.displays
                        .off_places
                        .insert(name.clone(), (g.loc.x, g.loc.y));
                }
                self.displays.off.insert(name);
                self.screen_turned_off(&output, true);
            } else if !takes && off && wanted_on(&self.config, &name) {
                tracing::info!(output = name, "the lid opened: the screen comes back");
                self.displays.off.remove(&name);
                let place = self.displays.off_places.remove(&name).unwrap_or_else(|| {
                    let right = self
                        .space
                        .outputs()
                        .filter_map(|o| self.space.output_geometry(o))
                        .map(|g| g.loc.x + g.size.w)
                        .max()
                        .unwrap_or(0);
                    (right, 0)
                });
                output.change_current_state(None, None, None, Some(place.into()));
                self.space.map_output(&output, place);
                layer_map_for_output(&output).arrange();
                self.panel_refresh();
                self.output_back(&output, true);
            }
        }
        self.displays.wanted = true;
        self.displays.dirty = true;
    }

    /// Whether the file turned `output` off.
    pub fn display_off(&self, output: &Output) -> bool {
        self.displays.off.contains(&output.name())
    }

    /// What a screen should be now: on, its mode, its scale, VRR.
    fn display_wanted(&self, output: &Output) -> Wanted {
        let name = output.name();
        let settings = self.screen_settings(&name);
        let settings = settings.as_ref();
        let facts = output.user_data().get::<Facts>();
        let current = output.current_mode().map(mode3);
        let mode = match (&self.displays.trial, &self.displays.kept, facts) {
            (Some(trial), _, _) if trial.output == name => Some(trial.mode),
            (_, Some(kept), _) if kept.output == name => Some(kept.mode),
            (_, _, Some(facts)) => Some(
                settings
                    .and_then(|s| s.mode)
                    .and_then(|m| m.pick(&facts.modes))
                    .unwrap_or_else(|| facts.default_mode()),
            ),
            _ => current,
        };
        let scale = match (settings.and_then(|s| s.scale), facts, mode) {
            (Some(scale), _, _) => scale,
            (None, Some(facts), Some(mode)) => facts.fixed_scale.unwrap_or_else(|| {
                crate::output_spec::default_scale((mode.0, mode.1), facts.millimetres)
            }),
            _ => output.current_scale().fractional_scale(),
        };
        Wanted {
            enabled: settings.is_none_or(|s| s.enabled) && !self.lid_takes(output),
            mode,
            scale,
            vrr: settings.is_some_and(|s| s.vrr) && facts.is_some_and(|f| f.vrr_capable),
        }
    }

    // --- Trials -------------------------------------------------------------------------

    /// `mode` tried on screen `name` for the Settings' line `session`.
    fn trial_start(&mut self, session: u64, name: &str, mode: &str) -> Result<u64, String> {
        let output = self
            .displays
            .outputs
            .iter()
            .find(|o| o.name() == name)
            .cloned()
            .ok_or_else(|| format!("no screen {name}"))?;
        if self.display_off(&output) {
            return Err(format!("{name} is off"));
        }
        let facts = output
            .user_data()
            .get::<Facts>()
            .ok_or_else(|| format!("{name}'s modes are not known"))?;
        let asked = ModeWanted::parse(mode).ok_or_else(|| format!("mode {mode:?}"))?;
        let picked = asked
            .pick(&facts.modes)
            .ok_or_else(|| format!("{name} does not offer {mode}"))?;
        if self.displays.trial.is_some() {
            self.trial_end(false, "another trial");
        }
        let before = output
            .current_mode()
            .map(mode3)
            .ok_or_else(|| format!("{name} has no mode"))?;
        self.displays.next_trial += 1;
        let id = self.displays.next_trial;
        let until = self.now() + TRIAL;
        tracing::info!(
            id,
            output = name,
            mode,
            "a mode tried: back in 15 s unless kept"
        );
        self.displays.trial = Some(Trial {
            id,
            output: name.to_owned(),
            mode: picked,
            before,
            until,
            session,
            told: TRIAL.as_secs(),
        });
        self.displays.wanted = true;
        self.displays.dirty = true;
        self.trial_tell();
        self.trial_arm(Duration::from_secs(1));
        Ok(id)
    }

    /// The trial's news to its line: the seconds left.
    fn trial_tell(&mut self) {
        let Some(trial) = &self.displays.trial else {
            return;
        };
        let line = serde_json::json!({
            "trial": {
                "id": trial.id,
                "output": trial.output,
                "mode": say_mode(trial.mode),
                "before": say_mode(trial.before),
                "left": trial.told,
            }
        })
        .to_string();
        let session = trial.session;
        self.displays_send(Some(session), &line);
    }

    /// The trial ends: kept, or its screen back to its mode.
    fn trial_end(&mut self, kept: bool, why: &str) {
        let Some(trial) = self.displays.trial.take() else {
            return;
        };
        if let Some(token) = self.displays.timer.take() {
            self.loop_handle.remove(token);
        }
        tracing::info!(
            id = trial.id,
            output = trial.output,
            kept,
            why,
            "a trial ends"
        );
        if kept {
            let file_then = self.screen_settings(&trial.output).and_then(|o| o.mode);
            self.displays.kept = Some(Kept {
                output: trial.output.clone(),
                mode: trial.mode,
                file_then,
            });
        }
        let line = serde_json::json!({
            "ended": { "id": trial.id, "output": trial.output, "kept": kept }
        })
        .to_string();
        self.displays_send(Some(trial.session), &line);
        self.displays.wanted = true;
        self.displays.dirty = true;
    }

    /// What falls due of the trial on the motions' clock: a second less,
    /// or its end.
    pub(crate) fn trial_tick(&mut self) {
        let now = self.now();
        let Some(trial) = &mut self.displays.trial else {
            return;
        };
        if now >= trial.until {
            self.trial_end(false, "15 s without an answer");
            return;
        }
        let left = (trial.until - now).as_secs_f64().ceil() as u64;
        let next = trial.until - now - Duration::from_secs(left.saturating_sub(1));
        if left != trial.told {
            trial.told = left;
            self.trial_tell();
        }
        self.trial_arm(next.max(Duration::from_millis(1)));
    }

    fn trial_arm(&mut self, after: Duration) {
        if let Some(token) = self.displays.timer.take() {
            self.loop_handle.remove(token);
        }
        let timer = self
            .loop_handle
            .insert_source(Timer::from_duration(after), |_, _, app| {
                app.state.displays.timer = None;
                app.state.trial_tick();
                TimeoutAction::Drop
            });
        match timer {
            Ok(token) => self.displays.timer = Some(token),
            Err(err) => tracing::warn!(%err.error, "could not time the trial"),
        }
    }

    // --- The Settings' lines ------------------------------------------------------------

    pub(crate) fn displays_heard(&mut self, heard: FromSettings) {
        match heard {
            FromSettings::Opened { session, to } => {
                tracing::info!(session, "the Settings follow the screens");
                let line = self.displays_json().to_string();
                let _ = to.send(line);
                let counts = self.desktop_counts();
                let _ = to.send(Self::desktops_line(counts));
                self.displays.desktops_told = Some(counts);
                // The clipboard's history and the recording (6.17).
                if let Some((items, pinned)) = self.clipboard.count {
                    let _ = to.send(crate::clipboard::clipboard_count_line(items, pinned));
                }
                let _ = to.send(self.recording_line());
                // The agents' tools (6.16).
                let _ = to.send(self.agents_settings_opened());
                self.displays.sessions.push(Session { id: session, to });
            }
            FromSettings::Closed { session } => {
                tracing::info!(session, "the Settings' line closed");
                self.displays.sessions.retain(|s| s.id != session);
                if self
                    .displays
                    .trial
                    .as_ref()
                    .is_some_and(|t| t.session == session)
                {
                    self.trial_end(false, "the Settings went");
                }
            }
            FromSettings::Line { session, line } => self.displays_line(session, &line),
        }
    }

    fn displays_line(&mut self, session: u64, line: &str) {
        // "Oublier cet ensemble" (6.9): its screens, a tab before each
        // (their identities hold spaces).
        if let Some(screens) = line.strip_prefix("forget-set") {
            let keys: Vec<String> = screens
                .split('\t')
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(str::to_owned)
                .collect();
            if let Err(why) = self.forget_screen_set(&crate::screen_sets::set_name(&keys)) {
                tracing::warn!(session, line, why, "the Settings asked what cannot be");
                let refused = serde_json::json!({ "refused": why }).to_string();
                self.displays_send(Some(session), &refused);
            }
            return;
        }
        let mut words = line.split_whitespace();
        let answer = match (words.next(), words.next(), words.next()) {
            (Some("try"), Some(name), Some(what)) => match what.strip_prefix("mode=") {
                Some(mode) => self.trial_start(session, name, mode).err(),
                None => Some(format!("cannot try {what}")),
            },
            (Some(word @ ("keep" | "revert")), Some(id), None) => {
                let id: Option<u64> = id.parse().ok();
                if self
                    .displays
                    .trial
                    .as_ref()
                    .is_some_and(|t| Some(t.id) == id)
                {
                    self.trial_end(word == "keep", "the Settings said");
                    None
                } else {
                    Some("no such trial".to_owned())
                }
            }
            // "Revenir à la forme de sa largeur" (6.4): the click's choice
            // forgotten.
            (Some("bar-auto"), Some(name), None) => {
                self.bar_auto(name);
                None
            }
            // The Agents page's tools (6.16): plugged in or out, herdr
            // followed or not; the watcher's thread writes, and the line
            // hears the tools again.
            (Some(word @ ("agent-plug" | "agent-unplug")), Some(tool), None) => {
                match crate::agents::tool(tool) {
                    Some(tool) => {
                        let ask = if word == "agent-plug" {
                            hocus_agents::watch::Ask::Plug(tool)
                        } else {
                            hocus_agents::watch::Ask::Unplug(tool)
                        };
                        self.agent_tools_asked(ask);
                        None
                    }
                    None => Some(format!("no tool {tool}")),
                }
            }
            (Some("agent-follow"), Some("herdr"), None) => {
                self.agent_tools_asked(hocus_agents::watch::Ask::FollowHerdr);
                None
            }
            (Some("agent-unfollow"), Some("herdr"), None) => {
                self.agent_tools_asked(hocus_agents::watch::Ask::UnfollowHerdr);
                None
            }
            // "Effacer l'historique" (6.17).
            (Some("clipboard-clear"), None, None) => {
                self.clipboard_clear();
                None
            }
            // A desktop taken out, or put back with its windows (6.14,
            // desktop_set.rs): answered on this line.
            (Some("desktop-remove"), Some(number), None) => {
                let done = number
                    .parse()
                    .map_err(|_| format!("no desktop {number}"))
                    .and_then(|n| self.desktop_remove(n));
                self.desktops_answer(session, done)
            }
            (Some("desktop-restore"), Some(number), windows) => {
                let windows: Result<Vec<u64>, _> =
                    windows.into_iter().chain(words).map(str::parse).collect();
                let done = match (number.parse(), windows) {
                    (Ok(n), Ok(windows)) => self.desktop_restore(n, &windows),
                    _ => Err(format!("cannot read {line}")),
                };
                self.desktops_answer(session, done)
            }
            // The applications with a window open, for the rules' menu
            // (6.14): they come first there.
            (Some("windows-apps"), None, None) => {
                let mut apps: Vec<String> = self
                    .windows
                    .values()
                    .filter_map(|m| crate::windows::app_id(&m.window))
                    .filter(|a| !a.is_empty())
                    .collect();
                apps.sort();
                apps.dedup();
                let answer = serde_json::json!({ "apps": apps }).to_string();
                self.displays_send(Some(session), &answer);
                None
            }
            _ => Some(format!("not understood: {line}")),
        };
        if let Some(why) = answer {
            tracing::warn!(session, line, why, "the Settings asked what cannot be");
            let refused = serde_json::json!({ "refused": why }).to_string();
            self.displays_send(Some(session), &refused);
        }
    }

    /// A desktop taken out or put back: its answer to the line `session`,
    /// and the desktops' windows to every line; or why it could not be.
    fn desktops_answer(
        &mut self,
        session: u64,
        done: Result<serde_json::Value, String>,
    ) -> Option<String> {
        let answer = match done {
            Ok(answer) => answer,
            Err(why) => return Some(why),
        };
        self.desktops_tell_settings();
        self.displays_send(Some(session), &answer.to_string());
        None
    }

    /// The desktops' windows to the Settings' lines, if they changed since
    /// they were told.
    fn desktops_tell_settings(&mut self) {
        let counts = self.desktop_counts();
        if self.displays.desktops_told == Some(counts) {
            return;
        }
        self.displays.desktops_told = Some(counts);
        self.displays_send(None, &Self::desktops_line(counts));
    }

    /// `line` to the Settings' line `session`, or to all of them.
    pub(crate) fn displays_send(&mut self, session: Option<u64>, line: &str) {
        self.displays.sessions.retain(|s| {
            if session.is_some_and(|id| id != s.id) {
                return true;
            }
            s.to.send(line.to_owned()).is_ok()
        });
    }

    /// After the turn: the layout applied when asked, then the Settings
    /// told what changed.
    pub(crate) fn displays_after_turn(&mut self, backend: &mut Backend) {
        if self.displays.wanted {
            self.displays.wanted = false;
            self.apply_displays(backend);
        }
        if !self.displays.sessions.is_empty() {
            self.desktops_tell_settings();
        }
        if !self.displays.dirty {
            return;
        }
        self.displays.dirty = false;
        if self.displays.sessions.is_empty() {
            return;
        }
        let line = self.displays_json().to_string();
        if self.displays.told.as_ref() == Some(&line) {
            return;
        }
        self.displays_send(None, &line);
        self.displays.told = Some(line);
    }

    /// The screens, as the Settings and `hocus msg outputs` read them.
    pub fn displays_json(&self) -> serde_json::Value {
        let primary = self.panel_primary();
        let mut list: Vec<serde_json::Value> = Vec::new();
        for output in &self.displays.outputs {
            let name = output.name();
            let facts = output.user_data().get::<Facts>();
            let settings = self.screen_settings(&name);
            let settings = settings.as_ref();
            let mode = output.current_mode().map(mode3);
            let position = self
                .space
                .output_geometry(output)
                .map(|g| (g.loc.x, g.loc.y))
                .or_else(|| self.displays.off_places.get(&name).copied())
                .unwrap_or_default();
            let size = logical_size(output).unwrap_or_default();
            let (scale, from) = match settings.and_then(|s| s.scale) {
                Some(scale) => (scale, "file"),
                None => (output.current_scale().fractional_scale(), "auto"),
            };
            let auto_scale = match (facts, mode) {
                (Some(f), Some(m)) => f.fixed_scale.unwrap_or_else(|| {
                    crate::output_spec::default_scale((m.0, m.1), f.millimetres)
                }),
                _ => output.current_scale().fractional_scale(),
            };
            let modes: Vec<serde_json::Value> = facts
                .map(|f| {
                    f.modes
                        .iter()
                        .map(|m| {
                            serde_json::json!([
                                m.0,
                                m.1,
                                m.2,
                                (m.0, m.1) == (f.preferred.0, f.preferred.1)
                            ])
                        })
                        .collect()
                })
                .unwrap_or_default();
            let file_mode = settings.and_then(|s| s.mode);
            let offered = match (file_mode, facts) {
                (Some(m), Some(f)) => m.pick(&f.modes).is_some(),
                _ => true,
            };
            let built_in = crate::away::is_internal(output);
            list.push(serde_json::json!({
                "name": name,
                "key": self.screen_key(&name),
                "built_in": built_in,
                "mm": [facts.map_or(0, |f| f.millimetres.0), facts.map_or(0, |f| f.millimetres.1)],
                "modes": modes,
                "mode": mode.map(|m| [m.0, m.1, m.2]),
                "scale": scale,
                "scale_from": from,
                "auto_scale": auto_scale,
                "position": [position.0, position.1],
                "size": [size.0, size.1],
                "vrr_capable": facts.is_some_and(|f| f.vrr_capable),
                "vrr": self.displays.vrr_on.contains(&name),
                "enabled": !self.display_off(output),
                "connected": true,
                "lid_closed": built_in && self.lid_closed(),
                "primary": primary.as_deref() == Some(name.as_str()),
                "file_mode": file_mode.map(|m| m.to_string()),
                "mode_offered": offered,
                // The bar on it now, and the form chosen there with a
                // click (6.4's page).
                "bar": self.panel.bars.get(&name).map(|b| b.form.name()),
                "bar_chosen": self.panel.chosen(&name).map(hocus_link::Form::name),
                // The magnifier on it (#424): its zoom and view, or null.
                "magnifier": self.magnifier_json(&name),
            }));
        }
        for gone in &self.displays.gone {
            list.push(serde_json::json!({
                "name": gone.name,
                "key": self.screen_key(&gone.name),
                "built_in": gone.built_in,
                "mm": [gone.millimetres.0, gone.millimetres.1],
                "modes": [],
                "mode": null,
                "position": [gone.rect.x, gone.rect.y],
                "size": [gone.rect.width, gone.rect.height],
                "enabled": true,
                "connected": false,
                "unplugged_at": gone.at,
                "bar_chosen": self.panel.chosen(&gone.name).map(hocus_link::Form::name),
            }));
        }
        serde_json::json!({
            "outputs": list,
            "places_refused": self.displays.places_refused,
            "trial": self.displays.trial.as_ref().map(|t| t.id),
            // The set in use and the sets known (windows.md §12, 6.9).
            "set": self.current_set(),
            "sets": self.sets_json(),
        })
    }

    /// The screen with the bar, as the rule says now.
    pub(crate) fn panel_primary(&self) -> Option<String> {
        let names: Vec<String> = self.space.outputs().map(Output::name).collect();
        crate::panel::primary(&names, &self.primary_wanted(), self.panel.home.as_deref())
    }

    // --- The layout ---------------------------------------------------------------------

    /// Every screen as it should be: on or off, its mode and VRR through the
    /// backend, its scale and place; windows go with their screen.
    fn apply_displays(&mut self, backend: &mut Backend) {
        let outputs = self.displays.outputs.clone();
        let wanted: Vec<(Output, Wanted)> = outputs
            .iter()
            .map(|o| (o.clone(), self.display_wanted(o)))
            .collect();
        // The last screen on stays on.
        let any_on = wanted.iter().any(|(_, w)| w.enabled);
        // Off first, from where they are: their windows go to another.
        for (output, w) in &wanted {
            let off = !w.enabled && any_on;
            let name = output.name();
            if off && !self.displays.off.contains(&name) {
                tracing::info!(output = name, "a screen turned off by the file");
                if let Some(g) = self.space.output_geometry(output) {
                    self.displays
                        .off_places
                        .insert(name.clone(), (g.loc.x, g.loc.y));
                }
                self.displays.off.insert(name);
                self.screen_turned_off(output, false);
            }
        }
        // Screens turned on again: laid out with the others, their
        // windows back once all are in place.
        let back: Vec<Output> = outputs
            .iter()
            .filter(|o| {
                let name = o.name();
                self.displays.off.contains(&name)
                    && wanted_on(&self.config, &name)
                    && !self.lid_takes(o)
            })
            .cloned()
            .collect();
        for output in &back {
            let name = output.name();
            tracing::info!(output = name, "a screen turned on by the file");
            self.displays.off.remove(&name);
            self.displays.off_places.remove(&name);
        }
        let on: Vec<(Output, Wanted)> = wanted
            .into_iter()
            .filter(|(o, _)| !self.display_off(o) || !any_on)
            .collect();
        // The windows still where a screen gone put them: still so once
        // the screens moved.
        let put = self.still_put();
        // Where each was, and its windows, before anything moves.
        let before: Vec<Before> = on
            .iter()
            .map(|(o, _)| {
                let was = self
                    .space
                    .output_geometry(o)
                    .map(rect_of)
                    .zip(self.work_area(o));
                (o.clone(), was, self.windows_on(o))
            })
            .collect();
        let mut scaled = Vec::new();
        for (output, w) in &on {
            let name = output.name();
            if let Some(mode) = w.mode
                && output.current_mode().map(mode3) != Some(mode)
            {
                match backend.set_mode(self, output, mode) {
                    Ok(()) => {
                        tracing::info!(output = name, mode = say_mode(mode), "a screen's mode")
                    }
                    Err(err) => tracing::warn!(output = name, "the mode is not taken: {err:#}"),
                }
            }
            let capable = output
                .user_data()
                .get::<Facts>()
                .is_some_and(|f| f.vrr_capable);
            if capable && w.vrr != self.displays.vrr_on.contains(&name) {
                match backend.set_vrr(output, w.vrr) {
                    Ok(()) => {
                        if w.vrr {
                            self.displays.vrr_on.insert(name.clone());
                        } else {
                            self.displays.vrr_on.remove(&name);
                        }
                    }
                    Err(err) => tracing::warn!(output = name, "VRR is not taken: {err:#}"),
                }
            }
            let now = output.current_scale().fractional_scale();
            if (now - w.scale).abs() > 1e-6 {
                tracing::info!(output = name, scale = w.scale, "a screen's scale");
                output.change_current_state(None, None, Some(crate::backend::scale(w.scale)), None);
                scaled.push(output.clone());
            }
        }
        // The places, with every size as it now is; a screen the lid took
        // away is out of them, as an unplugged one (windows.md §12).
        let outputs: Vec<Output> = outputs.into_iter().filter(|o| !self.lid_takes(o)).collect();
        let laid: Vec<Laid> = outputs
            .iter()
            .map(|o| {
                let name = o.name();
                let facts = o.user_data().get::<Facts>();
                Laid {
                    on: !self.display_off(o),
                    size: logical_size(o).unwrap_or((1, 1)),
                    file: self.screen_settings(&name).and_then(|s| s.position),
                    placed: facts.map_or_else(
                        || {
                            self.space
                                .output_geometry(o)
                                .map_or((0, 0), |g| (g.loc.x, g.loc.y))
                        },
                        |f| f.placed,
                    ),
                    placed_size: facts
                        .map_or_else(|| logical_size(o).unwrap_or((1, 1)), |f| f.placed_size),
                }
            })
            .collect();
        let (places, refused) = places(&laid);
        if refused && !self.displays.places_refused {
            tracing::warn!(
                "the file's places overlap or leave a screen apart: left to right instead"
            );
        }
        self.displays.places_refused = refused;
        for (output, place) in outputs.iter().zip(&places) {
            let name = output.name();
            if self.display_off(output) {
                self.displays.off_places.insert(name, *place);
                continue;
            }
            let mapped = self
                .space
                .output_geometry(output)
                .map(|g| (g.loc.x, g.loc.y));
            if mapped != Some(*place) || output.current_location() != (*place).into() {
                output.change_current_state(None, None, None, Some((*place).into()));
                self.space.map_output(output, *place);
            }
        }
        // Windows follow their screen, into its work area as its layers
        // leave it at its new size.
        for (output, _, _) in &before {
            layer_map_for_output(output).arrange();
        }
        // The bars at their screens' new scale and form first: the work
        // areas are theirs.
        for output in &scaled {
            self.bar_rescaled(&output.name());
        }
        self.panel_refresh();
        let mut moved = Vec::new();
        for (output, was, windows) in before {
            let now = self
                .space
                .output_geometry(&output)
                .map(rect_of)
                .zip(self.work_area(&output));
            if let (Some(was), Some(now)) = (was, now)
                && was != now
            {
                moved.push(crate::screens::Moved {
                    output: output.clone(),
                    was_area: was.1,
                    geometry: now.0,
                    area: now.1,
                    windows,
                });
            }
        }
        self.screens_moved(moved, &put);
        for output in &back {
            self.output_back(output, false);
        }
        self.panel_refresh();
        self.pointer_onto_primary();
        self.rearrange();
        self.note_screen_set();
        self.redraw = true;
        self.displays.dirty = true;
    }
}

/// Whether the file has `name` on.
fn wanted_on(config: &Config, name: &str) -> bool {
    config.outputs.get(name).is_none_or(|s| s.enabled)
}

/// A screen before the layout moves it: its geometry and work area, its
/// windows.
type Before = (Output, Option<(Rect, Rect)>, Vec<WindowId>);

#[derive(Debug, Clone, Copy)]
struct Wanted {
    enabled: bool,
    mode: Option<Mode3>,
    scale: f64,
    vrr: bool,
}

/// A screen, as the places are chosen.
#[derive(Debug, Clone)]
struct Laid {
    on: bool,
    size: (i32, i32),
    file: Option<(i32, i32)>,
    placed: (i32, i32),
    placed_size: (i32, i32),
}

/// The screens' places (in `laid`'s order, the connectors'), and whether
/// the file's were refused: the file's when every screen on has one and
/// they neither overlap nor leave one apart (the smallest x and y made 0);
/// else the backend's while no size changed and none is off; else left to
/// right, tops aligned.
fn places(laid: &[Laid]) -> (Vec<(i32, i32)>, bool) {
    let on: Vec<&Laid> = laid.iter().filter(|l| l.on).collect();
    let files: Vec<Option<(i32, i32)>> = on.iter().map(|l| l.file).collect();
    let mut refused = false;
    if !on.is_empty() && files.iter().all(Option::is_some) {
        let rects: Vec<Rect> = on
            .iter()
            .map(|l| {
                let (x, y) = l.file.unwrap_or_default();
                Rect::new(x, y, l.size.0, l.size.1)
            })
            .collect();
        if arranged(&rects) {
            let x0 = rects.iter().map(|r| r.x).min().unwrap_or(0);
            let y0 = rects.iter().map(|r| r.y).min().unwrap_or(0);
            let places = laid
                .iter()
                .map(|l| match l.file {
                    Some((x, y)) => (x - x0, y - y0),
                    None => l.placed,
                })
                .collect();
            return (places, false);
        }
        refused = true;
    }
    let untouched = !refused
        && laid.iter().all(|l| l.on && l.size == l.placed_size)
        && files.iter().all(Option::is_none);
    if untouched {
        return (laid.iter().map(|l| l.placed).collect(), false);
    }
    let mut x = 0;
    let places = laid
        .iter()
        .map(|l| {
            if !l.on {
                return l.file.unwrap_or(l.placed);
            }
            let at = (x, 0);
            x += l.size.0;
            at
        })
        .collect();
    (places, refused)
}

/// Whether `rects` neither overlap nor leave one apart: each touches
/// another along an edge.
pub fn arranged(rects: &[Rect]) -> bool {
    let overlap = |a: &Rect, b: &Rect| {
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
    };
    let touch = |a: &Rect, b: &Rect| {
        let along_y = a.y.max(b.y) < (a.y + a.height).min(b.y + b.height);
        let along_x = a.x.max(b.x) < (a.x + a.width).min(b.x + b.width);
        ((a.x + a.width == b.x || b.x + b.width == a.x) && along_y)
            || ((a.y + a.height == b.y || b.y + b.height == a.y) && along_x)
    };
    for (i, a) in rects.iter().enumerate() {
        if rects.iter().skip(i + 1).any(|b| overlap(a, b)) {
            return false;
        }
    }
    let mut reached = vec![false; rects.len()];
    let mut next = vec![0];
    if let Some(first) = reached.first_mut() {
        *first = true;
    }
    while let Some(a) = next.pop().and_then(|i| rects.get(i)) {
        for (j, (b, reached)) in rects.iter().zip(&mut reached).enumerate() {
            if !*reached && touch(a, b) {
                *reached = true;
                next.push(j);
            }
        }
    }
    reached.iter().all(|r| *r)
}

/// A screen's logical size: its mode's pixels over its scale.
pub fn logical_size(output: &Output) -> Option<(i32, i32)> {
    let mode = output.current_mode()?;
    let scale = output.current_scale().fractional_scale();
    let size = mode.size.to_f64().to_logical(scale).to_i32_round::<i32>();
    Some((size.w, size.h))
}

/// `2560x1600@60`, `1920x1080@59.94`: as the file writes a mode.
pub fn say_mode(m: Mode3) -> String {
    if m.2 % 1000 == 0 {
        format!("{}x{}@{}", m.0, m.1, m.2 / 1000)
    } else {
        let hz = format!("{:.3}", f64::from(m.2) / 1000.0);
        let hz = hz.trim_end_matches('0').trim_end_matches('.');
        format!("{}x{}@{hz}", m.0, m.1)
    }
}

pub(crate) fn wall_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// A Settings' line (`msg screens`), on threads of its own: one writes
/// what the loop sends, this one reads what the Settings ask until the
/// connection closes.
pub fn serve(stream: UnixStream, loop_: channel::Sender<crate::agents::Heard>) {
    let session = SESSIONS.fetch_add(1, Ordering::Relaxed);
    let (to, lines) = mpsc::channel::<String>();
    let _ = stream.set_read_timeout(None);
    let Ok(writer) = stream.try_clone() else {
        return;
    };
    let started = std::thread::Builder::new()
        .name("settings' line out".to_owned())
        .spawn(move || {
            let mut writer = writer;
            let _ = writer.set_write_timeout(Some(Duration::from_secs(5)));
            for line in lines {
                if writer.write_all(line.as_bytes()).is_err() || writer.write_all(b"\n").is_err() {
                    let _ = writer.shutdown(std::net::Shutdown::Both);
                    return;
                }
            }
        });
    if started.is_err() {
        return;
    }
    if loop_
        .send(crate::agents::Heard::Screens(FromSettings::Opened {
            session,
            to,
        }))
        .is_err()
    {
        return;
    }
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(line) = line else {
            break;
        };
        if loop_
            .send(crate::agents::Heard::Screens(FromSettings::Line {
                session,
                line,
            }))
            .is_err()
        {
            return;
        }
    }
    let _ = loop_.send(crate::agents::Heard::Screens(FromSettings::Closed {
        session,
    }));
}

/// The `Facts` of a screen made by hand (headless): its one mode, or the
/// usual sizes under it too.
pub fn headless_modes(width: i32, height: i32, refresh: i32) -> Vec<Mode3> {
    const SIZES: [(i32, i32); 8] = [
        (2560, 1600),
        (2560, 1440),
        (2560, 1080),
        (1920, 1200),
        (1920, 1080),
        (1680, 1050),
        (1280, 800),
        (1280, 720),
    ];
    let mut modes = vec![(width, height, refresh)];
    modes.extend(
        SIZES
            .iter()
            .filter(|(w, h)| *w <= width && *h <= height && (*w, *h) != (width, height))
            .map(|(w, h)| (*w, *h, refresh)),
    );
    sorted_modes(modes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laid(_name: &str, size: (i32, i32), file: Option<(i32, i32)>, placed: (i32, i32)) -> Laid {
        Laid {
            on: true,
            size,
            file,
            placed,
            placed_size: size,
        }
    }

    #[test]
    fn the_files_places_apply_when_they_touch_without_overlapping() {
        let a = laid("A", (2560, 1600), Some((3440, 0)), (0, 0));
        let b = laid("B", (3440, 1440), Some((0, 80)), (2560, 0));
        let (got, refused) = places(&[a.clone(), b.clone()]);
        assert_eq!(got, vec![(3440, 0), (0, 80)]);
        assert!(!refused);
        // Overlapping: left to right, tops aligned.
        let b2 = laid("B", (3440, 1440), Some((100, 0)), (2560, 0));
        let (got, refused) = places(&[a.clone(), b2]);
        assert_eq!(got, vec![(0, 0), (2560, 0)]);
        assert!(refused);
        // Apart.
        let b3 = laid("B", (3440, 1440), Some((9000, 0)), (2560, 0));
        assert!(places(&[a.clone(), b3]).1);
        // Normalised.
        let a4 = laid("A", (2560, 1600), Some((-2560, -100)), (0, 0));
        let b4 = laid("B", (3440, 1440), Some((0, 0)), (2560, 0));
        assert_eq!(places(&[a4, b4]).0, vec![(0, 0), (2560, 100)]);
        // Nothing in the file: where the backend put them.
        let a5 = laid("A", (2560, 1600), None, (0, 0));
        let b5 = laid("B", (3440, 1440), None, (2560, 300));
        assert_eq!(places(&[a5, b5]).0, vec![(0, 0), (2560, 300)]);
    }

    #[test]
    fn modes_are_said_and_picked_as_the_file_writes_them() {
        assert_eq!(say_mode((2560, 1600, 60_000)), "2560x1600@60");
        assert_eq!(say_mode((1920, 1080, 59_940)), "1920x1080@59.94");
        let modes = [
            (1920, 1080, 60_000),
            (1920, 1080, 59_940),
            (1920, 1080, 120_000),
        ];
        let pick = |t: &str| ModeWanted::parse(t).and_then(|m| m.pick(&modes));
        assert_eq!(pick("1920x1080@60"), Some((1920, 1080, 60_000)));
        assert_eq!(pick("1920x1080@59.94"), Some((1920, 1080, 59_940)));
        assert_eq!(pick("1920x1080"), Some((1920, 1080, 120_000)));
        assert_eq!(pick("1920x1080@144"), None);
        assert_eq!(pick("2560x1440@60"), None);
        assert_eq!(ModeWanted::parse("big"), None);
    }
}
