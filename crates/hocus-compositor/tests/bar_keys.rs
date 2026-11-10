//! The whole bar by the keyboard (docs/design/bar.md §11, issue #422): the
//! real shell on the CPU, headless, away from the machine's services, in
//! French. `Super+B` puts the keyboard into the bar of the focused
//! window's screen, on the agent waiting, else the one stuck, else the
//! clock; the keys walk the items (the shell's own tests check the order
//! item by item, `popover::tests`); `Enter` opens an item's popover,
//! `Esc` steps back to the item, then to the window. `Super+Shift+B`
//! switches the form. With a bar on each screen, `Super+B` again goes to
//! the next one, then back to the window.
//!
//! What the shell says of where the ring is ("the keyboard into the bar",
//! "the keyboard on an item") is read from its log; where the keyboard is,
//! from the compositor; what the window gets, from the test's client.

use crate::common;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use common::{
    Event, KEY_DOWN, KEY_ESC, KEY_LEFTMETA, KEY_LEFTSHIFT, Shows, WAIT, chord, shell_program, wait,
};
use hocus_compositor::{BarForm, Config, Headless, HeadlessConfig, PanelScreens, RendererChoice};
use hocus_link::{AgentShown, ToShell};

const KEY_B: u32 = 48;
const KEY_Q: u32 = 16;
const KEY_ENTER: u32 = 28;
const KEY_HOME: u32 = 102;
const KEY_END: u32 = 107;

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
}

/// The shell on `outputs` (a bar on each), its log in the test's
/// directory.
fn start(shell: &Path, outputs: &[&str]) -> anyhow::Result<Bed> {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let home = root.join("home");
    std::fs::create_dir_all(home.join(".local/state/hocuspanda"))?;
    // Day.1's first-run card set aside, as the agents' tests do.
    std::fs::write(home.join(".local/state/hocuspanda/day-sources"), "later\n")?;
    let log = root.join("shell.log");
    let script = root.join("shell");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             HOME='{home}' XDG_CONFIG_HOME='{home}/.config' XDG_CACHE_HOME='{home}/.cache' \
             XDG_STATE_HOME='{home}/.local/state' \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 ICED_BACKEND=tiny-skia \
             RUST_LOG=warn,hocuspanda_shell=debug\n\
             exec '{shell}' \"$@\" 2>>'{log}'\n",
            home = home.display(),
            shell = shell.display(),
            log = log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let mut config = Config {
        plain_background: true,
        french: true,
        state_dir: Some(root.join("state")),
        ..Config::default()
    };
    config.keyboard.layouts = vec!["us".to_owned()];
    config.panel.screens = PanelScreens::All;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: outputs
                .iter()
                .map(|o| o.parse())
                .collect::<Result<_, _>>()?,
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        config,
        Some(script),
    )?;
    let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.bar_commits() > 0 && hp.clock_surfaces().len() == outputs.len()
    })?;
    anyhow::ensure!(drawn, "no bar");
    let mut bed = Bed { _dir: dir, hp, log };
    // Every surface of the bar there and drawn: their items told.
    let footers = bed.hp.dispatch_until(WAIT, |hp| {
        hp.layer_rects(hocus_link::FOOTER_NAMESPACE).len() == outputs.len()
    })?;
    anyhow::ensure!(footers, "no footer");
    wait(&mut bed.hp, Duration::from_millis(1500))?;
    Ok(bed)
}

impl Bed {
    /// The shell's log, without its colours.
    fn log(&self) -> String {
        let text =
            String::from_utf8_lossy(&std::fs::read(&self.log).unwrap_or_default()).into_owned();
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn mark(&self) -> usize {
        self.log().len()
    }

    /// Waits until the shell logs a line holding every one of `texts`
    /// after `mark`.
    fn logged_since_all(&mut self, mark: usize, texts: &[&str]) -> anyhow::Result<()> {
        let deadline = std::time::Instant::now() + WAIT;
        let mut done = false;
        while std::time::Instant::now() < deadline {
            let text = self.log();
            if text
                .get(mark..)
                .is_some_and(|l| l.lines().any(|line| texts.iter().all(|t| line.contains(t))))
            {
                done = true;
                break;
            }
            wait(&mut self.hp, Duration::from_millis(20))?;
        }
        let tail: Vec<String> = self
            .log()
            .get(mark..)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains("keyboard") || l.contains("popover"))
            .map(str::to_owned)
            .collect();
        anyhow::ensure!(
            done,
            "the shell never logged {texts:?}; it logged:\n{}",
            tail.join("\n")
        );
        Ok(())
    }

    fn logged_since(&mut self, mark: usize, text: &str) -> anyhow::Result<()> {
        self.logged_since_all(mark, &[text])
    }

    /// Waits until the keyboard is on the shell's surface `namespace`
    /// (none: a window's, or nobody's).
    fn keyboard_on(&mut self, namespace: Option<&str>) -> anyhow::Result<()> {
        let on = self
            .hp
            .dispatch_until(WAIT, |hp| hp.keyboard_layer().as_deref() == namespace)?;
        anyhow::ensure!(
            on,
            "the keyboard is on {:?}, not {namespace:?}",
            self.hp.keyboard_layer()
        );
        Ok(())
    }

    fn press(&mut self, key: u32) {
        self.hp.key(key, true);
        self.hp.key(key, false);
    }

    fn super_b(&mut self) {
        chord(&mut self.hp, &[KEY_LEFTMETA, KEY_B]);
    }

    /// The window focused, by its application's id.
    fn focused(&self) -> Option<String> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.focused)
            .and_then(|w| w.app_id)
    }
}

/// An agent of the bar's in `state`, its window none.
fn agent(id: u64, name: &str, state: &str) -> AgentShown {
    AgentShown {
        id,
        state: state.to_owned(),
        since: id,
        source: "claude-code".to_owned(),
        window: 0,
        name: name.to_owned(),
        note: None,
    }
}

fn typed(events: &std::sync::mpsc::Receiver<Event>) -> Vec<u32> {
    events
        .try_iter()
        .filter_map(|e| match e {
            Event::Key(key, true) => Some(key),
            _ => None,
        })
        .collect()
}

#[test]
fn super_b_lands_on_the_clock_and_esc_gives_the_keyboard_back_to_the_window() -> anyhow::Result<()>
{
    let shell = shell_program();
    let mut bed = start(&shell, &["1920x1080"])?;
    let (_, typing) = common::open_showing(
        &mut bed.hp,
        "test.typing",
        Shows::Typing {
            app_id: "test.typing",
            size: (600, 400),
        },
    )?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(bed.focused().as_deref(), Some("test.typing"));
    let _ = typed(&typing.events);

    // Nobody waits: the ring on the clock, the bar's own surface holding
    // the keyboard.
    let mark = bed.mark();
    bed.super_b();
    bed.keyboard_on(Some(hocus_link::NAMESPACE))?;
    bed.logged_since_all(mark, &["the keyboard into the bar", "Calendar"])?;
    assert_eq!(bed.hp.bar_keyboard().as_deref(), Some("HEADLESS-1"));
    // The window keeps the focus, and gets no key (it saw `Super` go
    // down, before the shortcut was whole).
    let _ = typed(&typing.events);
    bed.press(KEY_Q);
    wait(&mut bed.hp, Duration::from_millis(200))?;
    // `Super` going down may reach the window only now, on a loaded
    // machine: that one is expected; Q is not.
    let mut got = typed(&typing.events);
    got.retain(|&key| key != KEY_LEFTMETA);
    assert_eq!(got, Vec::<u32>::new());
    assert_eq!(bed.focused().as_deref(), Some("test.typing"));

    // Down: the next item, another surface.
    let mark = bed.mark();
    bed.press(KEY_DOWN);
    bed.logged_since(mark, "the keyboard on an item")?;
    let off_clock = bed.hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer()
            .is_some_and(|ns| ns != hocus_link::NAMESPACE && ns.starts_with("hocus-bar"))
    })?;
    assert!(
        off_clock,
        "the keyboard is on {:?}",
        bed.hp.keyboard_layer()
    );
    // Home: the clock again; End: the footer's last button.
    let mark = bed.mark();
    bed.press(KEY_HOME);
    bed.logged_since_all(mark, &["the keyboard on an item", "Calendar"])?;
    bed.keyboard_on(Some(hocus_link::NAMESPACE))?;
    let mark = bed.mark();
    bed.press(KEY_END);
    bed.logged_since_all(mark, &["the keyboard on an item", "User"])?;
    bed.keyboard_on(Some(hocus_link::FOOTER_NAMESPACE))?;

    // Esc: back to the window, which types again.
    bed.press(KEY_ESC);
    bed.keyboard_on(None)?;
    assert_eq!(bed.focused().as_deref(), Some("test.typing"));
    bed.press(KEY_Q);
    let mut got = Vec::new();
    let reached = bed.hp.dispatch_until(WAIT, |_| {
        got.extend(typed(&typing.events));
        got.contains(&KEY_Q)
    })?;
    assert!(reached, "the window got {got:?}");
    Ok(())
}

#[test]
fn super_b_lands_on_the_waiting_agent_then_on_the_stuck_one() -> anyhow::Result<()> {
    let shell = shell_program();
    let mut bed = start(&shell, &["1920x1080"])?;
    let mark = bed.mark();
    bed.hp.tell_shell(ToShell::Agents(vec![
        agent(1, "build", "working"),
        agent(2, "site-web", "waiting"),
        agent(3, "infra", "failed"),
    ]));
    bed.logged_since(mark, "the agents group shows it")?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let mark = bed.mark();
    bed.super_b();
    bed.keyboard_on(Some(hocus_link::AGENTS_NAMESPACE))?;
    bed.logged_since_all(mark, &["the keyboard into the bar", "Agent(2)"])?;
    bed.press(KEY_ESC);
    bed.keyboard_on(None)?;

    // Nobody waits any more: the stuck one.
    let mark = bed.mark();
    bed.hp.tell_shell(ToShell::Agents(vec![
        agent(1, "build", "working"),
        agent(3, "infra", "failed"),
    ]));
    bed.logged_since(mark, "the agents group shows it")?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let mark = bed.mark();
    bed.super_b();
    bed.logged_since_all(mark, &["the keyboard into the bar", "Agent(3)"])?;
    bed.keyboard_on(Some(hocus_link::AGENTS_NAMESPACE))?;
    Ok(())
}

#[test]
fn enter_opens_the_items_popover_and_esc_steps_back_twice() -> anyhow::Result<()> {
    let shell = shell_program();
    let mut bed = start(&shell, &["1920x1080"])?;
    common::open(&mut bed.hp, "test.window", (600, 400))?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    bed.super_b();
    bed.keyboard_on(Some(hocus_link::NAMESPACE))?;
    // Enter on the clock: the calendar, holding the keyboard.
    let mark = bed.mark();
    bed.press(KEY_ENTER);
    bed.logged_since(mark, "a popover opens")?;
    bed.keyboard_on(Some(hocus_link::POPOVER_NAMESPACE))?;
    // Esc: closed, the ring back on the clock.
    bed.press(KEY_ESC);
    bed.keyboard_on(Some(hocus_link::NAMESPACE))?;
    assert_eq!(bed.hp.bar_keyboard().as_deref(), Some("HEADLESS-1"));
    // Esc again: the window has it.
    bed.press(KEY_ESC);
    bed.keyboard_on(None)?;
    assert_eq!(bed.focused().as_deref(), Some("test.window"));
    Ok(())
}

#[test]
fn super_shift_b_switches_the_form_and_super_b_no_longer_does() -> anyhow::Result<()> {
    let shell = shell_program();
    let mut bed = start(&shell, &["1920x1080"])?;
    let form = |hp: &Headless| hp.bars().first().map(|b| b.1);
    assert_eq!(form(&bed.hp), Some(BarForm::Full));
    bed.hp.pointer_move(600.0, 500.0);
    bed.super_b();
    bed.keyboard_on(Some(hocus_link::NAMESPACE))?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(form(&bed.hp), Some(BarForm::Full));
    bed.press(KEY_ESC);
    bed.keyboard_on(None)?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_B]);
    let mini = bed
        .hp
        .dispatch_until(WAIT, |hp| form(hp) == Some(BarForm::Mini))?;
    assert!(mini, "{:?}", bed.hp.bars());
    // In the mini form too: on its clock.
    let mark = bed.mark();
    bed.super_b();
    bed.logged_since_all(mark, &["the keyboard into the bar", "Calendar"])?;
    bed.keyboard_on(Some(hocus_link::NAMESPACE))?;
    Ok(())
}

#[test]
fn with_a_bar_on_each_screen_super_b_goes_on_to_the_next_then_back() -> anyhow::Result<()> {
    let shell = shell_program();
    let mut bed = start(&shell, &["1920x1080", "1920x1080"])?;
    common::open(&mut bed.hp, "test.window", (600, 400))?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(bed.focused().as_deref(), Some("test.window"));
    let screen = |hp: &Headless| hp.bar_keyboard();
    // The window's screen's bar first.
    bed.super_b();
    let first = bed
        .hp
        .dispatch_until(WAIT, |hp| screen(hp).as_deref() == Some("HEADLESS-1"))?;
    assert!(first, "{:?}", screen(&bed.hp));
    // Again: the other screen's.
    bed.super_b();
    let second = bed
        .hp
        .dispatch_until(WAIT, |hp| screen(hp).as_deref() == Some("HEADLESS-2"))?;
    assert!(second, "{:?}", screen(&bed.hp));
    // And again: back to the window.
    bed.super_b();
    bed.keyboard_on(None)?;
    assert_eq!(screen(&bed.hp), None);
    assert_eq!(bed.focused().as_deref(), Some("test.window"));
    Ok(())
}

/// The magnifier on (docs/design/accessibility.md): its view follows the
/// ring, to the clock as `Super+B` lands there, to the footer on `End`.
#[test]
fn the_magnifiers_view_follows_the_ring() -> anyhow::Result<()> {
    const KEY_EQUAL: u32 = 13;
    let shell = shell_program();
    let mut bed = start(&shell, &["1920x1080"])?;
    bed.hp.pointer_move(200.0, 500.0);
    for _ in 0..4 {
        chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_EQUAL]);
    }
    let view = |hp: &Headless| hp.state().magnifier_seen().map(|(_, _, v)| v);
    let shows = |hp: &Headless, (x, y): (f64, f64)| {
        view(hp).is_some_and(|(vx, vy, w, h)| vx <= x && x <= vx + w && vy <= y && y <= vy + h)
    };
    assert!(view(&bed.hp).is_some(), "the magnifier is off");
    // The clock's group, near the bar's top right.
    let clock = (1920.0 - 8.0 - 100.0, 8.0 + 50.0);
    assert!(!shows(&bed.hp, clock));
    bed.super_b();
    let followed = bed.hp.dispatch_until(WAIT, |hp| shows(hp, clock))?;
    assert!(followed, "the view stays at {:?}", view(&bed.hp));
    // End: the footer's last button, at the bar's foot.
    bed.keyboard_on(Some(hocus_link::NAMESPACE))?;
    wait(&mut bed.hp, Duration::from_millis(200))?;
    bed.press(KEY_END);
    let foot = bed.hp.dispatch_until(WAIT, |hp| {
        view(hp).is_some_and(|(_, y, _, h)| y + h > 1000.0)
    })?;
    let said: Vec<String> = bed
        .log()
        .lines()
        .filter(|l| l.contains("keyboard"))
        .map(str::to_owned)
        .collect();
    assert!(
        foot,
        "the view stays at {:?}; the shell said:\n{}",
        view(&bed.hp),
        said.join("\n")
    );
    Ok(())
}
