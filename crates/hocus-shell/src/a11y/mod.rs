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
pub mod rows;
mod surfaces;
mod words;

use std::{
    collections::HashMap,
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant},
};

use hocus_a11y::{Act, Asked, Bridge, Event, Node, Rect, Role, Window};
use hocus_link::LevelKey;
use iced::{Rectangle, window};

pub use words::{Words, plain};

use crate::{clock::time::Lang, popover::Item};

/// How often the windows are described again while a reader listens:
/// what changes without a message (the levels, the clock, a bubble) is
/// heard within this.
const TICK: Duration = Duration::from_millis(250);

/// An announcement stays in the tree this long: a screen reader starting
/// later does not hear it again.
const SAID_FOR: Duration = Duration::from_secs(10);

/// Starts the thread, if the compositor named a session bus.
/// What the thread hears.
enum Heard {
    /// From AccessKit's thread.
    Bridge(Event),
    /// The compositor asks what the shell shows (`hocus msg a11y`).
    Dump,
}

/// The thread's channel, once it runs.
static HEARD: std::sync::Mutex<Option<mpsc::Sender<Heard>>> = std::sync::Mutex::new(None);

/// Starts the thread: always, for `hocus msg a11y`; its bridge to AT-SPI
/// only if the compositor named a session bus.
pub fn start() {
    let (tell, heard) = mpsc::channel();
    *HEARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(tell.clone());
    let bridged = bridged();
    let started = std::thread::Builder::new()
        .name("a11y".to_owned())
        .spawn(move || {
            let bridge = bridged.then(|| {
                Bridge::new(move |event| {
                    let _ = tell.send(Heard::Bridge(event));
                })
            });
            run(bridge, &heard);
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread for screen readers");
    }
}

/// The compositor asks what the shell shows: each window's tree, said
/// back a line at a time (`hocus msg a11y`).
pub fn dump() {
    let sent = HEARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .is_some_and(|tell| tell.send(Heard::Dump).is_ok());
    if !sent {
        crate::popover::tell(hocus_link::ToCompositor::A11yEnd);
    }
}

/// Whether the shell may register on the session's accessibility bus: on
/// the bus the compositor names only.
fn bridged() -> bool {
    let Some(bus) = crate::day::session_bus() else {
        tracing::info!("no session bus named: nothing for screen readers");
        return false;
    };
    // AccessKit connects to the session's bus: it must be the one named.
    let session = std::env::var("DBUS_SESSION_BUS_ADDRESS").ok();
    if bus != "session" && session.as_deref() != Some(bus.as_str()) {
        tracing::info!("the session bus is not the one named: nothing for screen readers");
        return false;
    }
    true
}

fn run(mut bridge: Option<Bridge>, heard: &mpsc::Receiver<Heard>) {
    let mut said = Said::default();
    loop {
        let listening = bridge.is_some() && hocus_a11y::listening();
        let heard = if listening {
            match heard.recv_timeout(said.wait(Instant::now())) {
                Ok(heard) => Some(heard),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match heard.recv() {
                Ok(heard) => Some(heard),
                Err(_) => return,
            }
        };
        match heard {
            Some(Heard::Bridge(Event::Listening(true))) => {
                if let Some(bridge) = &mut bridge {
                    bridge.forget();
                }
                // The open popover says its rows again.
                crate::popover::send(vec![crate::popover::Effect::Wake(
                    hocus_link::POPOVER_NAMESPACE,
                )]);
            }
            Some(Heard::Bridge(Event::Listening(false))) => {
                said.announcer.reset();
                if let Some(bridge) = &mut bridge {
                    bridge.keep([]);
                }
                continue;
            }
            Some(Heard::Bridge(Event::Asked(asked))) => said.carry_out(&asked),
            Some(Heard::Dump) => {
                for window in said.describe(Instant::now()) {
                    for line in window.dump().lines() {
                        crate::popover::tell(hocus_link::ToCompositor::A11yLine(line.to_owned()));
                    }
                }
                crate::popover::tell(hocus_link::ToCompositor::A11yEnd);
            }
            None => {}
        }
        if let Some(bridge) = &mut bridge
            && hocus_a11y::listening()
        {
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
    /// A row of the open popover, by its key.
    Row(String),
    /// A bubble's control by its key, or the bubble itself (`note`: its
    /// default).
    Bubble { serial: u64, key: String },
    /// A control of a lock, by its key.
    Lock(String),
    /// A control of the authentication dialog, by its key.
    Auth(String),
    /// A row of the actions palette, by its key (#736).
    Palette(String),
    /// A control of the emoji picker, by its key.
    Emoji(String),
    /// A control of the share's chooser, by its key.
    Share(String),
    /// A tile, card or emoji of the launcher, by its key.
    Launcher(String),
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
        if let Some(palette) = crate::actions::a11y() {
            for node in palette.all() {
                if node.key.starts_with("actions:Row(") {
                    self.targets.insert(
                        (palette.key.clone(), node.key.clone()),
                        Target::Palette(node.key.clone()),
                    );
                }
            }
            windows.push(palette);
        }
        if let Some(launcher) = crate::launcher::a11y() {
            for node in launcher.all() {
                if node.key.starts_with("launcher:Item(") {
                    self.targets.insert(
                        (launcher.key.clone(), node.key.clone()),
                        Target::Launcher(node.key.clone()),
                    );
                }
            }
            windows.push(launcher);
        }
        if let Some(chooser) = crate::share::a11y() {
            for node in chooser.all() {
                if node.key.starts_with("share:") {
                    self.targets.insert(
                        (chooser.key.clone(), node.key.clone()),
                        Target::Share(node.key.clone()),
                    );
                }
            }
            windows.push(chooser);
        }
        if let Some(picker) = crate::emoji::a11y() {
            for node in picker.all() {
                if node.key.starts_with("emoji:") {
                    self.targets.insert(
                        (picker.key.clone(), node.key.clone()),
                        Target::Emoji(node.key.clone()),
                    );
                }
            }
            windows.push(picker);
        }
        windows.extend(self.bubbles(words));
        if let Some(dialog) = crate::auth::a11y() {
            for node in dialog.all() {
                if node.key.starts_with("auth:") {
                    self.targets.insert(
                        (dialog.key.clone(), node.key.clone()),
                        Target::Auth(node.key.clone()),
                    );
                }
            }
            windows.push(dialog);
        }
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
        let surface = crate::popover::with(|c| c.open_surface())?;
        if let Some(rows) = rows::of(surface) {
            for node in &rows.order {
                self.targets.insert(
                    ("popover".to_owned(), node.clone()),
                    Target::Row(node.clone()),
                );
            }
            shown.rows = rows.nodes;
            shown.focus = rows.focus.filter(|_| active);
            shown.subtitle = rows.subtitle;
        }
        Some(surfaces::popover(&shown))
    }

    fn bubbles(&mut self, words: Words) -> Vec<Window> {
        let bubbles: Vec<(window::Id, surfaces::Bubble)> = crate::notifications::with(|c| {
            c.stack
                .bubbles()
                .iter()
                .map(|b| {
                    (
                        b.id,
                        surfaces::Bubble {
                            serial: b.serial,
                            app: b.note.app.clone(),
                            summary: b.note.summary.clone(),
                            body: b.note.body.clone(),
                            urgent: b.note.urgency == crate::notifications::note::Urgency::Critical,
                            default: b.note.default,
                            nodes: Vec::new(),
                        },
                    )
                })
                .collect()
        });
        bubbles
            .into_iter()
            .map(|(id, mut b)| {
                b.nodes = rows::of(id).map(|r| r.nodes).unwrap_or_default();
                let window = surfaces::bubble(&b, words);
                let keys = std::iter::once("note".to_owned())
                    .filter(|_| b.default)
                    .chain(
                        b.nodes
                            .iter()
                            .filter(|n| !n.acts.is_empty())
                            .map(|n| n.key.clone()),
                    );
                for key in keys {
                    self.targets.insert(
                        (window.key.clone(), key.clone()),
                        Target::Bubble {
                            serial: b.serial,
                            key,
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
        let mut screens: Vec<(&String, &window::Id)> = lock.surfaces.iter().collect();
        screens.sort();
        screens
            .into_iter()
            .map(|(output, surface)| {
                let clock = Node::new("clock", Role::Label, clock.clone());
                let window = crate::lock::view::a11y(*surface, output, words, vec![clock]);
                for node in window.all() {
                    if node.key.starts_with("lock:") {
                        self.targets.insert(
                            (window.key.clone(), node.key.clone()),
                            Target::Lock(node.key.clone()),
                        );
                    }
                }
                window
            })
            .collect()
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
            (Target::Row(row), act) => press_row(row, act),
            (Target::Palette(key), Act::Click) => crate::actions::a11y_click(key),
            (Target::Emoji(key), Act::Click) => crate::emoji::a11y_click(key),
            (Target::Share(key), Act::Click) => crate::share::a11y_click(key),
            (Target::Launcher(key), Act::Click) => crate::launcher::a11y_click(key),
            (Target::Bubble { serial, key }, Act::Click) => {
                crate::notifications::bubble::a11y_click(*serial, key);
            }
            (Target::Lock(key), Act::Click | Act::Focus) => crate::lock::view::a11y_click(key),
            (Target::Auth(key), Act::Click | Act::Focus) => crate::auth::a11y_click(key),
            (_, act) => tracing::debug!(?act, "not done from a screen reader"),
        }
    }
}

/// The keys that reach row `row` of the open popover and act on it: `Tab`
/// to it, then `Enter` (a click), or the arrows (a slider's step).
fn press_row(row: &str, act: Act) {
    use iced::keyboard::{Key, Modifiers, key::Named};
    let Some(surface) = crate::popover::with(|c| c.open_surface()) else {
        return;
    };
    let Some(rows) = rows::of(surface) else {
        return;
    };
    let (steps, step) = if rows.arrows {
        let Some((n, down)) = rows::arrows(&rows.order, rows.focus.as_deref(), row) else {
            return;
        };
        (
            n,
            if down {
                Named::ArrowDown
            } else {
                Named::ArrowUp
            },
        )
    } else {
        let Some(n) = rows::tabs(&rows.order, rows.focus.as_deref(), row) else {
            return;
        };
        (n, Named::Tab)
    };
    let then = match act {
        Act::Click => Named::Enter,
        Act::Increment => Named::ArrowRight,
        Act::Decrement => Named::ArrowLeft,
        Act::Focus => Named::Shift,
        Act::Menu => return,
    };
    for _ in 0..steps {
        crate::popover::key(&Key::Named(step), Modifiers::empty(), None);
    }
    if then != Named::Shift {
        crate::popover::key(&Key::Named(then), Modifiers::empty(), None);
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
