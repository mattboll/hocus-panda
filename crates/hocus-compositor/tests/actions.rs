//! The actions palette (#736, docs/design/actions.md): the real shell, in
//! French, its state directory the test's own.
//!
//! One scenario per point of the approved spec: `Super+P` opens it on the
//! focused window's screen, 64 px from its top, centred in its work area,
//! the window ringed above the dim (and nothing on the other screen); the
//! empty field lists the categories, then the recent actions first;
//! typing filters, marks the letters, finds the English names and the
//! keywords; each row shows its shortcut as the configuration binds it,
//! followed live; `Enter` runs the action on the target; a step lists the
//! desktops, a digit picks one, "envoyer 3" does it in one go, `Backspace`
//! goes back; an impossible action stays inert; nothing found searches in
//! the launcher; a switch shows its state and turns over; log out asks
//! first; `Esc` and the target closing close it; at rest nothing is drawn;
//! screen readers read it as a dialog with its rows.
//!
//! Pixman here, so the shell draws on the CPU, as in CI.

use crate::common;

use std::{
    io::{Read as _, Write as _},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

use anyhow::Context as _;
use common::{
    ClientHandle, Command, KEY_ESC, KEY_LEFT, KEY_LEFTCTRL, KEY_LEFTMETA, KEY_LEFTSHIFT, Shows,
    WAIT, chord, open_showing, wait,
};
use hocus_compositor::{
    Config, Headless, HeadlessConfig, OutputSpec, RendererChoice,
    model::{Mode, Rect, WindowId, Zone},
};

const KEY_ENTER: u32 = 28;
const KEY_BACKSPACE: u32 = 14;
const KEY_DOWN: u32 = 108;
const KEY_P: u32 = 25;

/// A US keyboard's keys, by their evdev codes.
fn key_of(c: char) -> Option<u32> {
    Some(match c {
        'q' => 16,
        'w' => 17,
        'e' => 18,
        'r' => 19,
        't' => 20,
        'y' => 21,
        'u' => 22,
        'i' => 23,
        'o' => 24,
        'p' => 25,
        'a' => 30,
        's' => 31,
        'd' => 32,
        'f' => 33,
        'g' => 34,
        'h' => 35,
        'j' => 36,
        'k' => 37,
        'l' => 38,
        'z' => 44,
        'x' => 45,
        'c' => 46,
        'v' => 47,
        'b' => 48,
        'n' => 49,
        'm' => 50,
        ' ' => 57,
        '1'..='9' => 2 + u32::from(c) - u32::from('1'),
        _ => return None,
    })
}

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
    file: PathBuf,
    /// The test's windows' clients, kept running.
    clients: Vec<ClientHandle>,
}

impl Bed {
    /// The compositor on `outputs`, the real shell, the configuration in
    /// a file of the test's own (followed live).
    fn start(outputs: Vec<OutputSpec>, reduced_motion: bool) -> anyhow::Result<Option<Self>> {
        let shell = common::shell_program();
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in [
            "home",
            "config/hocuspanda",
            "cache",
            "state/hocuspanda",
            "bin",
        ] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&file, "[keyboard]\nlayouts = [\"us\"]\n")?;
        let log = root.join("shell.log");
        let script = root.join("shell");
        let home = |sub: &str| root.join(sub).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 XDG_DATA_DIRS='{}' PATH='{}' RUST_LOG=warn,hocuspanda_shell=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                home("home"),
                home("config"),
                home("cache"),
                home("state"),
                home("data"),
                home("system"),
                home("bin"),
                shell.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        config.appearance.reduced_motion = reduced_motion;
        config.file = Some(file.clone());
        config.home = Some(root.join("home"));
        config.state_dir = Some(root.join("state"));
        let hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs,
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
            Some(script),
        )?;
        let mut bed = Self {
            hp,
            dir,
            log,
            file,
            clients: Vec::new(),
        };
        let up = bed
            .hp
            .dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
        anyhow::ensure!(up, "no bar");
        Ok(Some(bed))
    }

    fn one_screen() -> anyhow::Result<Option<Self>> {
        Self::start(vec![OutputSpec::new(1920, 1080)], false)
    }

    /// A window `app_id`, titled `title`, focused.
    fn window(&mut self, app_id: &'static str, title: &'static str) -> anyhow::Result<WindowId> {
        let (_, handle) = open_showing(
            &mut self.hp,
            app_id,
            Shows::Window {
                app_id,
                size: (800, 600),
                popup: false,
            },
        )?;
        handle.commands.send(Command::Title(title))?;
        self.clients.push(handle);
        // The title comes from the client's thread: until the compositor
        // has it, the palette names the window by its application's id.
        let id = self.id_of(app_id)?;
        let titled = self
            .hp
            .dispatch_until(WAIT, |hp| hp.window_title(id) == title)?;
        anyhow::ensure!(titled, "{app_id} is titled {:?}", self.hp.window_title(id));
        Ok(id)
    }

    fn id_of(&self, app_id: &str) -> anyhow::Result<WindowId> {
        self.hp
            .all_windows()
            .into_iter()
            .find(|(_, app, _, _)| app.as_deref() == Some(app_id))
            .map(|(id, ..)| id)
            .ok_or_else(|| anyhow::anyhow!("no window {app_id}"))
    }

    fn desktop_of(&self, app_id: &str) -> Option<usize> {
        self.hp
            .all_windows()
            .into_iter()
            .find(|(_, app, _, _)| app.as_deref() == Some(app_id))
            .map(|(_, _, desktop, _)| desktop)
    }

    fn plain(&self) -> String {
        String::from_utf8_lossy(&std::fs::read(&self.log).unwrap_or_default())
            .chars()
            .filter(|c| *c != '\u{1b}')
            .collect()
    }

    fn mark(&self) -> usize {
        self.plain().len()
    }

    /// Waits for the shell to log a line holding `text` after `from`;
    /// that line.
    fn logged_after(&mut self, from: usize, text: &str) -> anyhow::Result<String> {
        let log = self.log.clone();
        let read = |log: &Path| {
            String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default())
                .chars()
                .filter(|c| *c != '\u{1b}')
                .collect::<String>()
        };
        let found = |log: &Path| {
            read(log)
                .get(from..)
                .and_then(|l| l.lines().rfind(|l| l.contains(text)).map(str::to_owned))
        };
        let done = self.hp.dispatch_until(WAIT, |_| found(&log).is_some())?;
        anyhow::ensure!(
            done,
            "the shell never logged {text:?}; it logged:\n{}",
            read(&log)
                .get(from..)
                .unwrap_or_default()
                .lines()
                .filter(|l| l.contains("action"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        Ok(found(&log).unwrap_or_default())
    }

    /// The last thing the palette showed after `from`, once it holds
    /// `text`.
    fn shows_after(&mut self, from: usize, text: &str) -> anyhow::Result<String> {
        let log = self.log.clone();
        let last = |log: &Path| {
            String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default())
                .chars()
                .filter(|c| *c != '\u{1b}')
                .collect::<String>()
                .get(from..)
                .and_then(|l| {
                    l.lines()
                        .rfind(|l| l.contains("the actions palette shows it"))
                        .map(str::to_owned)
                })
                .unwrap_or_default()
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| last(&log).contains(text))?;
        let shows = last(&log);
        anyhow::ensure!(
            done,
            "the palette never showed {text:?}; it showed:\n{shows}"
        );
        Ok(shows)
    }

    fn press(&mut self, key: u32) {
        self.hp.key(key, true);
        self.hp.key(key, false);
    }

    fn type_text(&mut self, text: &str) -> anyhow::Result<()> {
        for c in text.chars() {
            let key = key_of(c).ok_or_else(|| anyhow::anyhow!("no key for {c:?}"))?;
            self.press(key);
        }
        Ok(())
    }

    /// `Super+P`, and the palette drawn and still: what it shows.
    fn open(&mut self) -> anyhow::Result<String> {
        let from = self.mark();
        let commits = self.hp.actions_commits();
        chord(&mut self.hp, &[KEY_LEFTMETA, KEY_P]);
        let opened = self.hp.dispatch_until(WAIT, |hp| {
            hp.actions_commits() > commits && hp.actions_surfaces().len() == 1
        })?;
        anyhow::ensure!(opened, "no palette");
        let shows = self.shows_after(from, "query=\"\"")?;
        let output = self.hp.actions_surfaces()[0].0.clone();
        let still = self.hp.dispatch_until(WAIT, |hp| {
            hp.actions_backdrop(&output)
                .is_some_and(|(_, alpha)| alpha >= 1.0)
        })?;
        anyhow::ensure!(still, "the palette never came whole");
        Ok(shows)
    }

    /// Types `text` in the open palette: what it shows then.
    fn typed(&mut self, text: &str) -> anyhow::Result<String> {
        let from = self.mark();
        self.type_text(text)?;
        self.shows_after(from, &format!("query={text:?}"))
    }

    /// Waits for the palette's surface to go.
    fn closed(&mut self) -> anyhow::Result<()> {
        let gone = self
            .hp
            .dispatch_until(WAIT, |hp| hp.actions_surfaces().is_empty())?;
        anyhow::ensure!(gone, "the palette stays");
        Ok(())
    }

    /// `hocus msg a11y`'s answer, from a thread while the loop runs.
    fn a11y(&mut self) -> anyhow::Result<String> {
        let path = self
            .hp
            .control_socket()
            .context("no control socket")?
            .to_path_buf();
        let (said, heard) = mpsc::channel();
        std::thread::spawn(move || {
            let asked = (|| -> std::io::Result<String> {
                let mut stream = UnixStream::connect(&path)?;
                stream.set_read_timeout(Some(WAIT))?;
                stream.write_all(b"msg a11y\n")?;
                let mut answer = String::new();
                stream.read_to_string(&mut answer)?;
                Ok(answer)
            })();
            let _ = said.send(asked);
        });
        let mut answer = None;
        self.hp.dispatch_until(WAIT, |_| {
            if answer.is_none() {
                answer = heard.try_recv().ok();
            }
            answer.is_some()
        })?;
        Ok(answer.context("no answer")??)
    }
}

/// The rows a line of the log shows, as the shell wrote them.
fn rows(shows: &str) -> Vec<String> {
    let Some(start) = shows.find("rows=[") else {
        return Vec::new();
    };
    let list = &shows[start + 6..shows.rfind(']').unwrap_or(shows.len())];
    list.split("\", \"")
        .map(|r| r.trim_matches('"').replace("\\u{a0}", "\u{a0}"))
        .collect()
}

#[test]
fn super_p_opens_on_the_focused_windows_screen_ringed_above_the_dim() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(
        vec![OutputSpec::new(1920, 1080), OutputSpec::new(2560, 1440)],
        false,
    )?
    else {
        return Ok(());
    };
    // The window on the second screen, the pointer back on the first.
    bed.hp.pointer_move(2900.0, 700.0);
    let kitty = bed.window("kitty", "kitty · herdr")?;
    bed.hp.pointer_move(600.0, 500.0);
    let shows = bed.open()?;
    assert!(shows.contains("target=\"kitty · herdr\""), "{shows}");
    let surfaces = bed.hp.actions_surfaces();
    let (output, rect) = surfaces[0].clone();
    assert_eq!(output, "HEADLESS-2");
    // 64 px under the screen's top, 720 wide, centred in its work area
    // (the bar on the right keeps its own room).
    assert_eq!(rect.y, 64);
    assert_eq!(rect.width, 720);
    let area = bed
        .hp
        .state()
        .work_area(
            &bed.hp
                .state()
                .space
                .outputs()
                .find(|o| o.name() == "HEADLESS-2")
                .cloned()
                .context("no second screen")?,
        )
        .context("no work area")?;
    let centre = rect.x + rect.width / 2;
    assert!(
        (centre - (area.x + area.width / 2)).abs() <= 1,
        "{rect:?} in {area:?}"
    );
    // The window ringed above the dim, there; nothing on the first screen.
    assert_eq!(
        bed.hp.actions_backdrop("HEADLESS-2").map(|(w, _)| w),
        Some(Some(kitty.0))
    );
    assert_eq!(bed.hp.actions_backdrop("HEADLESS-1"), None);
    assert_eq!(
        bed.hp.actions_target(),
        Some((Some(kitty.0), "HEADLESS-2".to_owned()))
    );
    // The empty field: the categories in their order, no recent ones.
    let heads: Vec<String> = rows(&shows)
        .into_iter()
        .filter(|r| r.starts_with('#'))
        .collect();
    assert_eq!(
        heads,
        [
            "# Fenêtre",
            "# Bureaux",
            "# Écrans",
            "# Agents",
            "# Barre",
            "# Captures",
            "# Accessibilité",
            "# Session",
            "# Réglages"
        ]
    );
    // Each row with its shortcut, as the configuration binds it.
    assert!(shows.contains("Réduire <Super H>"), "{shows}");
    assert!(shows.contains("Moitié droite <Super →>"), "{shows}");
    assert!(
        shows.contains("Envoyer au bureau… <Super Maj 1…9>"),
        "{shows}"
    );
    // Screen readers: a dialog with its field and its rows.
    let said = bed.a11y()?;
    assert!(said.contains("dialog \"Actions\""), "{said}");
    assert!(said.contains("text \"Chercher une action\""), "{said}");
    assert!(said.contains("list item \"Fermer\""), "{said}");
    // At rest, the palette draws nothing.
    wait(&mut bed.hp, Duration::from_millis(700))?;
    let commits = bed.hp.actions_commits();
    wait(&mut bed.hp, Duration::from_secs(1))?;
    assert_eq!(bed.hp.actions_commits(), commits, "drawn at rest");
    // Esc: it closes, the keyboard back on the window.
    bed.press(KEY_ESC);
    bed.closed()?;
    let back = bed.hp.dispatch_until(WAIT, |hp| {
        hp.windows().iter().any(|w| w.id == kitty && w.keyboard)
    })?;
    anyhow::ensure!(back, "the window never had the keyboard back");
    // A rebound shortcut shows rebound, without restarting.
    std::fs::write(
        &bed.file,
        "[keyboard]\nlayouts = [\"us\"]\n\n[shortcuts]\nminimize = \"Super+M\"\n",
    )?;
    let read = bed.hp.dispatch_until(WAIT, |hp| {
        hp.state().config.shortcuts.minimize == ["Super+M".parse().expect("a chord")]
    })?;
    anyhow::ensure!(read, "the shortcut was never read");
    let shows = bed.open()?;
    assert!(shows.contains("Réduire <Super M>"), "{shows}");
    Ok(())
}

#[test]
fn typing_filters_enter_runs_on_the_target_and_the_recent_come_first() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::one_screen()? else {
        return Ok(());
    };
    let kitty = bed.window("kitty", "kitty")?;
    bed.open()?;
    // "bur": the desktops' actions first, the letters marked; the
    // overview by its keyword.
    let shows = bed.typed("bur")?;
    let listed = rows(&shows);
    // The first row that can run is selected, past an inert one.
    assert!(
        listed[0].starts_with("Bureau de gauche") && listed[0].contains("~ déjà le premier bureau"),
        "{listed:?}"
    );
    assert!(listed[1].starts_with("*Bureau de droite"), "{listed:?}");
    assert!(
        listed
            .iter()
            .take(5)
            .all(|r| r.to_lowercase().contains("bureau")),
        "{listed:?}"
    );
    assert!(listed.iter().any(|r| r.contains("[Bur]")), "{listed:?}");
    assert!(
        listed
            .iter()
            .any(|r| r.starts_with("Vue d’ensemble") && r.contains("(mot-clé\u{a0}: bureaux)")),
        "{listed:?}"
    );
    bed.press(KEY_ESC);
    bed.closed()?;
    // In English, the same actions.
    bed.open()?;
    let shows = bed.typed("desktop")?;
    assert!(shows.contains("Envoyer au bureau…"), "{shows}");
    assert!(shows.contains("Aller au bureau…"), "{shows}");
    bed.press(KEY_ESC);
    bed.closed()?;
    // "moitie droite", Enter: the window takes the right half, once the
    // palette is gone.
    bed.open()?;
    bed.typed("moitie droite")?;
    bed.press(KEY_ENTER);
    bed.closed()?;
    let snapped = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.mode(kitty) == Mode::Snapped(Zone::Right))?;
    anyhow::ensure!(snapped, "not snapped: {:?}", bed.hp.mode(kitty));
    // Then "Agrandir": the recent ones, the latest first.
    bed.open()?;
    bed.typed("agrandir")?;
    bed.press(KEY_ENTER);
    bed.closed()?;
    let maximized = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.mode(kitty) == Mode::Snapped(Zone::Maximized))?;
    anyhow::ensure!(maximized, "not maximised: {:?}", bed.hp.mode(kitty));
    let shows = bed.open()?;
    let listed = rows(&shows);
    assert_eq!(listed[0], "# Récentes", "{listed:?}");
    assert!(listed[1].starts_with("*Agrandir"), "{listed:?}");
    assert!(listed[2].starts_with("Moitié droite"), "{listed:?}");
    // Kept on disk, for the next session.
    let kept = std::fs::read_to_string(bed.dir.path().join("state/hocuspanda/actions"))?;
    assert_eq!(
        kept.lines().take(2).collect::<Vec<_>>(),
        ["maximize", "snap-right"]
    );
    // A switch shows its state, and Enter turns it over.
    bed.press(KEY_ESC);
    bed.closed()?;
    bed.open()?;
    let shows = bed.typed("toujours au premier")?;
    assert!(
        rows(&shows)[0].starts_with("*Toujours au premier plan") && shows.contains("= désactivé"),
        "{shows}"
    );
    bed.press(KEY_ENTER);
    bed.closed()?;
    let above = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.state().model.is_above(kitty))?;
    anyhow::ensure!(above, "not kept above");
    let shows = bed.open()?;
    assert!(
        shows.contains("Toujours au premier plan = activé"),
        "{shows}"
    );
    Ok(())
}

#[test]
fn a_step_lists_the_desktops_a_digit_picks_and_envoyer_3_goes_at_once() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::one_screen()? else {
        return Ok(());
    };
    bed.window("foot", "foot")?;
    bed.window("kitty", "kitty")?;
    bed.open()?;
    let shows = bed.typed("envoyer au bur")?;
    assert!(
        rows(&shows)[0].starts_with("*Envoyer au bureau…"),
        "{shows}"
    );
    // Enter: the desktops of the window's screen, its own inert.
    let from = bed.mark();
    bed.press(KEY_ENTER);
    let shows = bed.shows_after(from, "step=Some(\"send-to-desktop\")")?;
    let listed = rows(&shows);
    assert_eq!(listed[0], "# kitty part sur…", "{listed:?}");
    assert!(
        listed[1].starts_with("Bureau 1 · 2 fenêtres ~ actuel"),
        "{listed:?}"
    );
    assert!(listed[2].starts_with("*Bureau 2 · vide"), "{listed:?}");
    assert!(shows.contains("query=\"\""), "{shows}");
    // Backspace on the empty field: back to the actions, as typed.
    let from = bed.mark();
    bed.press(KEY_BACKSPACE);
    bed.shows_after(from, "step=None")?;
    let shows = bed.shows_after(from, "query=\"envoyer au bur\"")?;
    assert!(
        rows(&shows)[0].starts_with("*Envoyer au bureau…"),
        "{shows}"
    );
    // Again, and 3: the window on desktop 3, the screen on desktop 1.
    let from = bed.mark();
    bed.press(KEY_ENTER);
    bed.shows_after(from, "step=Some(\"send-to-desktop\")")?;
    bed.press(key_of('3').context("3")?);
    bed.closed()?;
    let sent = bed.hp.dispatch_until(WAIT, |hp| {
        hp.all_windows()
            .iter()
            .any(|(_, app, desktop, _)| app.as_deref() == Some("kitty") && *desktop == 2)
    })?;
    anyhow::ensure!(sent, "kitty is on {:?}", bed.desktop_of("kitty"));
    assert_eq!(bed.hp.desktop().0, 0, "the screen moved");
    // "envoyer 2", Enter: foot, now focused, goes to desktop 2 at once.
    let shows = bed.open()?;
    assert!(shows.contains("target=\"foot\""), "{shows}");
    let shows = bed.typed("envoyer 2")?;
    assert!(
        rows(&shows)[0].starts_with("*Envoyer au bureau 2"),
        "{shows}"
    );
    bed.press(KEY_ENTER);
    bed.closed()?;
    let sent = bed.hp.dispatch_until(WAIT, |hp| {
        hp.all_windows()
            .iter()
            .any(|(_, app, desktop, _)| app.as_deref() == Some("foot") && *desktop == 1)
    })?;
    anyhow::ensure!(sent, "foot is on {:?}", bed.desktop_of("foot"));
    assert_eq!(bed.hp.desktop().0, 0, "the screen moved");
    // In a step, Esc goes back first, then closes.
    bed.window("vim", "vim")?;
    bed.open()?;
    bed.typed("aller au bur")?;
    let from = bed.mark();
    bed.press(KEY_ENTER);
    bed.shows_after(from, "step=Some(\"desktop\")")?;
    let from = bed.mark();
    bed.press(KEY_ESC);
    bed.shows_after(from, "step=None")?;
    bed.press(KEY_ESC);
    bed.closed()?;
    Ok(())
}

#[test]
fn an_impossible_action_is_inert_and_nothing_found_searches_the_launcher() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::one_screen()? else {
        return Ok(());
    };
    let kitty = bed.window("kitty", "kitty")?;
    let rect: Rect = common::rect_of(&bed.hp, "kitty").context("kitty")?;
    bed.open()?;
    // One screen: inert, with its reason; Enter changes nothing.
    let shows = bed.typed("ecran de droite")?;
    assert!(
        rows(&shows)[0].contains("Envoyer sur l’écran de droite")
            && rows(&shows)[0].contains("~ un seul écran"),
        "{shows}"
    );
    bed.press(KEY_ENTER);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(bed.hp.actions_surfaces().len(), 1, "it closed");
    assert_eq!(common::rect_of(&bed.hp, "kitty"), Some(rect));
    // Nothing found: the line, then the launcher's and the Settings' rows;
    // Enter opens the launcher searching for it.
    for _ in 0.."ecran de droite".len() {
        bed.press(KEY_BACKSPACE);
    }
    let shows = bed.typed("pizza")?;
    let listed = rows(&shows);
    assert_eq!(
        listed,
        [
            "Aucune action pour «\u{a0}pizza\u{a0}».",
            "*Chercher «\u{a0}pizza\u{a0}» dans le lanceur",
            "Chercher «\u{a0}pizza\u{a0}» dans les Réglages"
        ]
    );
    let from = bed.mark();
    bed.press(KEY_ENTER);
    bed.closed()?;
    let line = bed.logged_after(from, "the launcher opens searching")?;
    assert!(line.contains("pizza"), "{line}");
    let opened = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.launcher_surfaces().len() == 1)?;
    anyhow::ensure!(opened, "no launcher");
    bed.press(KEY_ESC);
    let gone = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.launcher_surfaces().is_empty())?;
    anyhow::ensure!(gone, "the launcher stays");
    // Log out asks first.
    bed.open()?;
    bed.typed("se deconnecter")?;
    bed.press(KEY_ENTER);
    bed.closed()?;
    let asked = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.leave_shown().is_some())?;
    anyhow::ensure!(asked, "no confirmation");
    assert!(bed.hp.state().ending.is_none(), "the session ended at once");
    assert!(bed.hp.windows().iter().any(|w| w.id == kitty));
    Ok(())
}

#[test]
fn the_target_going_or_reduced_motion_and_the_down_arrow() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(vec![OutputSpec::new(1920, 1080)], true)? else {
        return Ok(());
    };
    bed.window("kitty", "kitty")?;
    bed.open()?;
    // Reduced motion: the palette fades in place.
    let surfaces = bed.hp.actions_surfaces();
    assert_eq!(surfaces[0].1.y, 64);
    // The arrows move the selection past the headings.
    let from = bed.mark();
    bed.press(KEY_DOWN);
    let shows = bed.shows_after(from, "*Réduire")?;
    assert!(!shows.contains("*Fermer"), "{shows}");
    // The target window closing closes it, the keyboard nowhere stuck.
    for client in &bed.clients {
        client.commands.send(Command::Close)?;
    }
    bed.closed()?;
    assert_eq!(bed.hp.actions_target(), None);
    // With Shift held a letter types too (accents aside).
    bed.window("foot", "foot")?;
    bed.open()?;
    let from = bed.mark();
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, key_of('f').context("f")?]);
    bed.shows_after(from, "query=\"F\"")?;
    Ok(())
}

/// The filter is the shell's text field (#771): the caret moves in it,
/// what is typed goes in there, a selection is replaced, and the list
/// follows each time.
#[test]
fn the_caret_moves_in_the_field_and_the_list_follows_what_is_typed_there() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::one_screen()? else {
        return Ok(());
    };
    bed.window("kitty", "kitty")?;
    bed.open()?;
    let shows = bed.typed("brea")?;
    assert!(
        !rows(&shows).iter().any(|r| r.contains("Bureau de droite")),
        "{shows}"
    );
    // Three to the left, between « b » and « rea »: the letter goes there.
    let from = bed.mark();
    for _ in 0..3 {
        bed.press(KEY_LEFT);
    }
    bed.type_text("u")?;
    let shows = bed.shows_after(from, "query=\"burea\"")?;
    assert!(
        rows(&shows).iter().any(|r| r.contains("Bureau de droite")),
        "{shows}"
    );
    // It all selected: Ctrl+C copies it, what is typed replaces it, and
    // Ctrl+Z brings it back.
    let from = bed.mark();
    chord(&mut bed.hp, &[KEY_LEFTCTRL, key_of('a').context("a")?]);
    chord(&mut bed.hp, &[KEY_LEFTCTRL, key_of('c').context("c")?]);
    let copied = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.clipboard().as_deref() == Some("burea"))?;
    anyhow::ensure!(copied, "the clipboard holds {:?}", bed.hp.clipboard());
    bed.type_text("f")?;
    bed.shows_after(from, "query=\"f\"")?;
    let from = bed.mark();
    chord(&mut bed.hp, &[KEY_LEFTCTRL, key_of('z').context("z")?]);
    bed.shows_after(from, "query=\"burea\"")?;
    // The field asks for an input method: what one commits goes in too, in
    // place of what is selected again.
    let asks = bed.hp.dispatch_until(WAIT, |hp| hp.keyboard_takes_text())?;
    anyhow::ensure!(asks, "the field asks for no input method");
    let from = bed.mark();
    anyhow::ensure!(
        bed.hp.input_method(None, Some("é")),
        "the commit was not taken"
    );
    bed.shows_after(from, "query=\"é\"")?;
    Ok(())
}

/// The screen as drawn, to `out/NAME.png`, and the palette's corner of it
/// beside it (`NAME-palette.png`).
fn shoot(bed: &mut Bed, out: &Path, name: &str) -> anyhow::Result<()> {
    wait(&mut bed.hp, Duration::from_millis(400))?;
    let (w, h, px) = bed.hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
    image.save(out.join(format!("{name}.png")))?;
    if let Some((_, rect)) = bed.hp.actions_surfaces().into_iter().next() {
        let x = (rect.x - 40).max(0) as u32;
        let y = (rect.y - 40).max(0) as u32;
        let cw = ((rect.width + 80) as u32).min(w as u32 - x);
        let ch = ((rect.height + 80) as u32).min(h as u32 - y);
        image::imageops::crop_imm(&image, x, y, cw, ch)
            .to_image()
            .save(out.join(format!("{name}-palette.png")))?;
    }
    eprintln!("{}", out.join(name).display());
    Ok(())
}

/// The screenshots for the maintainer: the empty field, "bur", and the
/// desktops' step. Written to `$HOCUS_SHOTS` when it names a folder; run
/// by hand (`--run-ignored only`).
#[test]
#[ignore = "screenshots, by hand"]
fn shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        eprintln!("HOCUS_SHOTS names no folder");
        return Ok(());
    };
    let Some(mut bed) = Bed::start(vec![OutputSpec::new(2560, 1440)], false)? else {
        return Ok(());
    };
    std::fs::write(
        &bed.file,
        "[keyboard]\nlayouts = [\"us\"]\n\n[desktops]\nnames = [\"Code\", \"Web\", \"Notes\"]\n",
    )?;
    let named = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.state().config.desktops.names.len() == 3)?;
    anyhow::ensure!(named, "the names were never read");
    bed.hp.pointer_move(600.0, 700.0);
    bed.window("firefox", "Firefox")?;
    bed.hp.pointer_move(1700.0, 700.0);
    bed.window("kitty", "kitty · herdr")?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, 106]);
    // Something recent.
    bed.open()?;
    bed.typed("moitie droite")?;
    bed.press(KEY_ENTER);
    bed.closed()?;
    bed.open()?;
    shoot(&mut bed, &out, "empty")?;
    bed.typed("bur")?;
    shoot(&mut bed, &out, "bur")?;
    for _ in 0..3 {
        bed.press(KEY_BACKSPACE);
    }
    bed.typed("envoyer au bur")?;
    let from = bed.mark();
    bed.press(KEY_ENTER);
    bed.shows_after(from, "step=Some(\"send-to-desktop\")")?;
    shoot(&mut bed, &out, "step")?;
    bed.press(KEY_BACKSPACE);
    bed.press(KEY_ESC);
    bed.closed()?;
    bed.open()?;
    bed.typed("pizza")?;
    shoot(&mut bed, &out, "nothing")?;
    Ok(())
}

/// The palette in its main state (the empty field, a window targeted),
/// for the widget kit's side-by-sides (#772).
#[test]
#[ignore = "writes screenshots under HOCUS_KIT_SHOTS"]
fn kit_shot() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::one_screen()? else {
        return Ok(());
    };
    bed.window("kitty", "kitty · herdr")?;
    bed.open()?;
    bed.hp.pointer_move(1800.0, 900.0);
    common::layer_shot(&mut bed.hp, hocus_link::ACTIONS_NAMESPACE, "actions")
}
