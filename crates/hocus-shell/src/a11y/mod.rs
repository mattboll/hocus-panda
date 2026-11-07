//! Screen readers (docs/design/screen-reader.md): what the shell shows,
//! as AT-SPI windows of named nodes, through `hocus-a11y` (AccessKit).
//!
//! A thread of its own owns the bridge to AT-SPI: the shell's loop never
//! waits on it. Until an assistive technology listens, the thread sleeps
//! on the bridge's events and nothing is described. While one listens, it
//! reads the shell's model (the bars' items and where they are, as the
//! popovers' controller keeps them for the keyboard; the levels, the
//! connections, the agents…) as it changes, a few times a second, builds
//! each window's tree, and hands over those that changed. What a screen
//! reader asks (a click, a step of a slider) is done as the pointer or
//! the keys would.
//!
//! Only on a session bus the compositor names (as the notifications are):
//! a nested shell beside a live session never registers on that session's
//! accessibility bus.

mod announce;
mod bar;
mod surfaces;
mod words;

use std::{
    collections::HashMap,
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant},
};

use hocus_a11y::{Act, Asked, Bridge, Event, Rect, Window};
use hocus_link::LevelKey;
use iced::{Rectangle, window};

pub use words::Words;

use crate::{clock::time::Lang, popover::Item};

/// How often the windows are described again while a reader listens:
/// what changes without a message (the levels, the clock, a bubble) is
/// heard within this.
const TICK: Duration = Duration::from_millis(250);

/// An announcement stays in the tree this long: a screen reader starting
/// later does not hear it again.
const SAID_FOR: Duration = Duration::from_secs(10);

/// Starts the thread, if the compositor named a session bus.
pub fn start() {
    let Some(bus) = crate::day::session_bus() else {
        tracing::info!("no session bus named: nothing for screen readers");
        return;
    };
    // AccessKit connects to the session's bus: it must be the one named.
    let session = std::env::var("DBUS_SESSION_BUS_ADDRESS").ok();
    if bus != "session" && session.as_deref() != Some(bus.as_str()) {
        tracing::info!("the session bus is not the one named: nothing for screen readers");
        return;
    }
    let started = std::thread::Builder::new()
        .name("a11y".to_owned())
        .spawn(run);
    if let Err(err) = started {
        tracing::warn!(%err, "no thread for screen readers");
    }
}

fn run() {
    let (tell, heard) = mpsc::channel();
    let mut bridge = Bridge::new(move |event| {
        let _ = tell.send(event);
    });
    let mut said = Said::default();
    loop {
        let event = if hocus_a11y::listening() {
            match heard.recv_timeout(said.wait(Instant::now())) {
                Ok(event) => Some(event),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match heard.recv() {
                Ok(event) => Some(event),
                Err(_) => return,
            }
        };
        match event {
            Some(Event::Listening(true)) => bridge.forget(),
            Some(Event::Listening(false)) => {
                said.announcer.reset();
                bridge.keep([]);
                continue;
            }
            Some(Event::Asked(asked)) => said.carry_out(&asked),
            None => {}
        }
        if hocus_a11y::listening() {
            let windows = said.describe(Instant::now());
            bridge.show(windows);
        }
    }
}

/// What a node a screen reader acts on stands for.
#[derive(Debug, Clone)]
enum Target {
    /// An item of a bar: what a click on it does.
    Item {
        bar: String,
        item: Item,
        rect: Rectangle,
    },
    /// A row of the sound's popover, by its key.
    Sound(String),
    /// A bubble's action, or the bubble itself (`None`: its default).
    Bubble { serial: u64, action: Option<String> },
    Lock {
        surface: window::Id,
        target: crate::lock::Target,
    },
    Auth {
        surface: window::Id,
        target: crate::auth::Target,
    },
}

/// The thread's state: the announcer, the last announcement, and what each
/// node shown stands for.
#[derive(Debug, Default)]
struct Said {
    announcer: announce::Announcer,
    announced: u64,
    announce: Option<(u64, String, Instant)>,
    targets: HashMap<(String, String), Target>,
}

impl Said {
    /// How long to wait for an event: the next tick, or the announcement's
    /// time if sooner.
    fn wait(&self, now: Instant) -> Duration {
        self.announcer
            .deadline()
            .map_or(TICK, |at| at.saturating_duration_since(now).min(TICK))
    }

    /// Every window the shell shows now. While the session is locked, the
    /// lock alone: nothing behind it is read.
    fn describe(&mut self, now: Instant) -> Vec<Window> {
        let lang = Lang::of_messages();
        let words = Words::new(lang);
        let agents = crate::agents::said().agents;
        let quiet = crate::notifications::with(|c| c.stack.dnd.is_on());
        self.announcer.observe(&agents, now, quiet);
        if let Some(text) = self.announcer.due(now, words) {
            self.announced += 1;
            tracing::info!(said = %text, "said to screen readers");
            self.announce = Some((self.announced, text, now));
        }
        if self
            .announce
            .as_ref()
            .is_some_and(|(_, _, at)| now.saturating_duration_since(*at) > SAID_FOR)
        {
            self.announce = None;
        }
        self.targets.clear();
        let mut windows = Vec::new();
        if crate::lock::active() {
            windows.extend(self.locks(words));
            return windows;
        }
        let mut bars = crate::popover::with(|c| c.bar_forms());
        bars.sort_by(|a, b| a.0.cmp(&b.0));
        for (i, (bar, form)) in bars.iter().enumerate() {
            let mut facts = gather(bar, *form, lang, &agents);
            // Said once, from the first bar.
            if i == 0 {
                facts.announce = self
                    .announce
                    .as_ref()
                    .map(|(n, text, _)| (*n, text.clone()));
            }
            let window = bar::window(&facts, words);
            for (item, rect) in &facts.frame {
                self.targets.insert(
                    (window.key.clone(), bar::key(*item)),
                    Target::Item {
                        bar: bar.clone(),
                        item: *item,
                        rect: *rect,
                    },
                );
            }
            windows.push(window);
        }
        windows.extend(self.popover(words));
        windows.extend(self.bubbles(words));
        windows.extend(self.auth(words));
        windows
    }

    fn popover(&mut self, words: Words) -> Option<Window> {
        let (item, active) = crate::popover::with(|c| {
            let item = c.open_on().map(|(_, item)| item)?;
            let active = c.open_surface().is_some_and(|id| c.holds_keyboard(id));
            Some((item, active))
        })?;
        let mut shown = surfaces::Popover {
            title: surfaces::popover_title(item, words),
            active,
            ..surfaces::Popover::default()
        };
        if item == Item::Sound {
            let (rows, focus) = crate::popover::sound::a11y(words);
            for row in &rows {
                self.targets.insert(
                    ("popover".to_owned(), row.key.clone()),
                    Target::Sound(row.key.clone()),
                );
            }
            shown.rows = rows;
            shown.focus = focus.filter(|_| active);
            shown.subtitle = crate::popover::sound::a11y_subtitle();
        }
        Some(surfaces::popover(&shown))
    }

    fn bubbles(&mut self, words: Words) -> Vec<Window> {
        let bubbles: Vec<surfaces::Bubble> = crate::notifications::with(|c| {
            c.stack
                .bubbles()
                .iter()
                .map(|b| surfaces::Bubble {
                    serial: b.serial,
                    app: b.note.app.clone(),
                    summary: b.note.summary.clone(),
                    body: b.note.body.clone(),
                    urgent: b.note.urgency == crate::notifications::note::Urgency::Critical,
                    actions: b.note.actions.clone(),
                    default: b.note.default,
                    progress: b.note.progress,
                })
                .collect()
        });
        bubbles
            .iter()
            .map(|b| {
                let window = surfaces::bubble(b, words);
                if b.default {
                    self.targets.insert(
                        (window.key.clone(), "note".to_owned()),
                        Target::Bubble {
                            serial: b.serial,
                            action: None,
                        },
                    );
                }
                for (action, _) in &b.actions {
                    self.targets.insert(
                        (window.key.clone(), format!("action:{action}")),
                        Target::Bubble {
                            serial: b.serial,
                            action: Some(action.clone()),
                        },
                    );
                }
                window
            })
            .collect()
    }

    fn locks(&mut self, words: Words) -> Vec<Window> {
        let Some(lock) = crate::lock::with(|l| l.clone()) else {
            return Vec::new();
        };
        let lang = words.lang();
        let clock = words.clock(&crate::clock::time::Shown::at(
            &crate::notifications::wall(),
            lang,
        ));
        let layout = crate::layouts::snapshot().shown().map(|l| l.name.clone());
        let mut screens: Vec<(&String, &window::Id)> = lock.surfaces.iter().collect();
        screens.sort();
        screens
            .into_iter()
            .map(|(output, surface)| {
                let window = surfaces::lock(
                    &surfaces::Lock {
                        output: output.clone(),
                        shown: lock.shown.clone(),
                        clock: Some(clock.clone()),
                        layout: layout.clone(),
                    },
                    words,
                );
                for (key, target) in [
                    ("field", crate::lock::Target::Field),
                    ("unlock", crate::lock::Target::Button),
                    ("layout", crate::lock::Target::Layout),
                ] {
                    self.targets.insert(
                        (window.key.clone(), key.to_owned()),
                        Target::Lock {
                            surface: *surface,
                            target,
                        },
                    );
                }
                window
            })
            .collect()
    }

    fn auth(&mut self, words: Words) -> Option<Window> {
        use crate::auth::Target as T;
        let auth_words = crate::auth::words();
        let (shown, surface) = crate::auth::with_shown(|dialog, caps, waiting| {
            let laid = crate::auth::draw::Layout::of(dialog, auth_words, waiting, caps);
            let rect = |r: Rectangle| Rect::new(r.x, r.y, r.width, r.height);
            let (who, _) = crate::auth::draw::who(dialog);
            let account = dialog.request.account();
            let shown = surfaces::Auth {
                name: auth_words.name(&who),
                message: dialog.request.message.trim().to_owned(),
                accounts: dialog
                    .request
                    .accounts
                    .iter()
                    .map(|a| a.name.clone())
                    .collect(),
                chosen: dialog.request.chosen,
                chooser: dialog.chooser,
                field: account.map_or_else(
                    || auth_words.password().to_owned(),
                    |a| auth_words.password_of(&a.name),
                ),
                note: laid.note.join(" "),
                checking: dialog.phase == crate::auth::Phase::Checking,
                details: dialog.details,
                waiting: laid.waiting.clone(),
                cancel: auth_words.cancel().to_owned(),
                authorize: auth_words.authorize().to_owned(),
                change: auth_words.change().to_owned(),
                focus: Some(dialog.focus),
                places: surfaces::AuthPlaces {
                    change: laid.change.map(rect),
                    people: laid.people.iter().copied().map(rect).collect(),
                    field: Some(rect(laid.field)),
                    details: Some(rect(laid.details)),
                    cancel: Some(rect(laid.cancel)),
                    authorize: Some(rect(laid.authorize)),
                },
                size: {
                    let (w, h) = laid.surface();
                    (w as f32, h as f32)
                },
            };
            (shown, dialog.surface)
        })?;
        let surface = surface?;
        let window = surfaces::auth(&shown, words);
        let mut targets = vec![T::Change, T::Field, T::Details, T::Cancel, T::Authorize];
        targets.extend((0..shown.accounts.len()).map(T::Account));
        for target in targets {
            self.targets.insert(
                (window.key.clone(), surfaces::auth_key(target)),
                Target::Auth { surface, target },
            );
        }
        Some(window)
    }

    /// Does what a screen reader asked, as the pointer or the keys would.
    fn carry_out(&self, asked: &Asked) {
        let Some(target) = self
            .targets
            .get(&(asked.window.clone(), asked.node.clone()))
        else {
            tracing::debug!(?asked, "a screen reader's action on what no longer shows");
            return;
        };
        tracing::debug!(?asked, "a screen reader's action");
        match (target, asked.act) {
            (
                Target::Item {
                    item: Item::Sound, ..
                },
                Act::Increment,
            ) => crate::levels::key(LevelKey::VolumeUp),
            (
                Target::Item {
                    item: Item::Sound, ..
                },
                Act::Decrement,
            ) => crate::levels::key(LevelKey::VolumeDown),
            (
                Target::Item {
                    item: Item::Brightness,
                    ..
                },
                Act::Increment,
            ) => crate::levels::key(LevelKey::BrightnessUp),
            (
                Target::Item {
                    item: Item::Brightness,
                    ..
                },
                Act::Decrement,
            ) => crate::levels::key(LevelKey::BrightnessDown),
            (Target::Item { bar, item, rect }, Act::Click) => {
                crate::popover::event(crate::popover::Event::ItemPress {
                    bar: bar.clone(),
                    item: *item,
                    rect: *rect,
                });
            }
            (Target::Sound(key), act) => match (key.as_str(), act) {
                ("Mute", Act::Click) => crate::levels::key(LevelKey::Mute),
                ("Slider", Act::Increment) => crate::levels::key(LevelKey::VolumeUp),
                ("Slider", Act::Decrement) => crate::levels::key(LevelKey::VolumeDown),
                _ => tracing::debug!(key, ?act, "not done from a screen reader yet"),
            },
            (Target::Bubble { serial, action }, Act::Click) => {
                crate::notifications::input(match action {
                    Some(key) => crate::notifications::Input::Action {
                        serial: *serial,
                        key: key.clone(),
                    },
                    None => crate::notifications::Input::Card(*serial),
                });
            }
            (Target::Lock { surface, target }, Act::Click | Act::Focus) => {
                crate::lock::pointer(*surface, crate::lock::Pointing::Press(Some(*target)));
                crate::lock::pointer(*surface, crate::lock::Pointing::Release(Some(*target)));
            }
            (Target::Auth { surface, target }, Act::Click | Act::Focus) => {
                let now = Instant::now();
                crate::auth::pointer(*surface, crate::auth::Pointing::Press(Some(*target)), now);
                crate::auth::pointer(*surface, crate::auth::Pointing::Release(Some(*target)), now);
                crate::auth::wake();
            }
            (_, act) => tracing::debug!(?act, "not done from a screen reader"),
        }
    }
}

/// What bar `bar` shows now, from the shell's model.
fn gather(
    bar: &str,
    form: hocus_link::Form,
    lang: Lang,
    agents: &[hocus_link::AgentShown],
) -> bar::Facts {
    use crate::popover::Item as I;
    let (shown, focused, open, height) = crate::popover::with(|c| {
        (
            c.items_of(bar),
            c.focused_on().filter(|(b, _)| *b == bar).map(|(_, i)| i),
            c.open_on().filter(|(b, _)| *b == bar).map(|(_, i)| i),
            c.bar_height(bar),
        )
    });
    // The bar's frame is from its right edge (its rim, 1 px in): its
    // surface's is from its left.
    let width = form.width().unsigned_abs() as f32;
    let items: Vec<(&'static str, Item, Rect)> = shown
        .iter()
        .map(|(namespace, item, r)| {
            (
                *namespace,
                *item,
                Rect::new(r.x + width + 1.0, r.y, r.width, r.height),
            )
        })
        .collect();
    let frame = shown.iter().map(|(_, item, r)| (*item, *r)).collect();
    let levels_words = crate::levels::model::Words::new(lang);
    let screen = crate::desktops::screen_of(bar, 0).map_or_else(
        || bar.to_owned(),
        |s| levels_words.screen(s.built_in, s.inches, s.number),
    );
    let uses = crate::in_use::snapshot();
    let mut used = Vec::new();
    let apps = |media: &[crate::in_use::Seen<crate::in_use::Media>]| -> Vec<String> {
        media
            .iter()
            .filter(|m| m.live())
            .map(|m| m.what.app.clone())
            .collect()
    };
    for (kind, apps) in [
        (crate::in_use::Kind::Microphone, apps(&uses.microphone)),
        (crate::in_use::Kind::Camera, apps(&uses.camera)),
        (
            crate::in_use::Kind::Screen,
            uses.shares
                .iter()
                .filter(|s| s.live())
                .map(|s| s.what.app.clone())
                .collect(),
        ),
    ] {
        if !apps.is_empty() {
            used.push((kind, apps));
        }
    }
    if uses.recording.as_ref().is_some_and(|r| r.live()) {
        used.push((crate::in_use::Kind::Recording, Vec::new()));
    }
    let lines = crate::tasks::lines();
    let tasks: Vec<bar::Task> = lines
        .lines
        .iter()
        .filter(|l| l.going.is_none() && l.look != crate::tasks::Look::Adding)
        .map(|l| bar::Task {
            title: l.title.clone(),
            ticked: l.look.ticked(),
        })
        .collect();
    let shown_boxes = items
        .iter()
        .filter(|(_, item, _)| matches!(item, I::Task(_)))
        .count();
    let windows = crate::windows::shown(bar);
    let window_lines: Vec<crate::windows::Line> = windows
        .rows
        .iter()
        .filter_map(|r| match r {
            crate::windows::Row::Window(line) => Some(line.clone()),
            crate::windows::Row::More(_) => None,
        })
        .collect();
    let more_windows = windows.count.saturating_sub(window_lines.len());
    let mut desks = Vec::new();
    for (_, item, _) in &items {
        let I::Desktop { row, index } = *item else {
            continue;
        };
        let output = crate::desktops::output_of(bar, row);
        let Some(said) = crate::desktops::snapshot(&output) else {
            continue;
        };
        let titles = crate::desktops::titles(&output).unwrap_or_default();
        let i = usize::from(index);
        let name = crate::desktops::name(i);
        let screen = (row > 0)
            .then(|| crate::desktops::screen_of(bar, row))
            .flatten()
            .map(|s| levels_words.screen(s.built_in, s.inches, s.number));
        desks.push((
            (row, index),
            bar::Desk {
                title: levels_words.desktop_title(i + 1, name.as_deref()),
                current: usize::from(said.current) == i,
                windows: said
                    .windows
                    .iter()
                    .filter(|w| {
                        usize::from(w.desktop) == i && w.look != hocus_link::Look::Minimized
                    })
                    .filter_map(|w| {
                        titles
                            .titles
                            .iter()
                            .find(|(id, _)| *id == w.id)
                            .map(|(_, t)| t.clone())
                    })
                    .collect(),
                count: usize::from(said.count),
                screen,
            },
        ));
    }
    let link = crate::network::snapshot().link();
    let bluetooth = crate::bluetooth::snapshot().row();
    bar::Facts {
        bar: bar.to_owned(),
        screen,
        size: Some((width, height)),
        items,
        frame,
        focused,
        open,
        clock: Some(crate::clock::time::Shown::at(
            &crate::notifications::wall(),
            lang,
        )),
        meeting: crate::meeting::snapshot(),
        uses: used,
        microphone_muted: uses.muted,
        today: crate::today::shown(bar).0,
        open_tasks: lines.open(),
        more_tasks: lines.open().saturating_sub(shown_boxes),
        tasks,
        agents: agents.to_vec(),
        now_us: crate::agents::now_us(),
        rows_shown: crate::agents::rows_shown(),
        windows: window_lines,
        more_windows,
        desks,
        link: (link != hocus_network::model::Link::Hidden).then_some(link),
        bluetooth: (bluetooth != hocus_bluetooth::model::Row::Hidden).then_some(bluetooth),
        layout: crate::layouts::snapshot().shown().map(|l| l.name.clone()),
        levels: crate::levels::snapshot(),
        unseen: crate::notifications::with(|c| c.stack.badge()),
        user: crate::user::name(),
        tray: crate::tray::shown()
            .into_iter()
            .map(|s| (s.key, s.name))
            .collect(),
        announce: None,
    }
}
