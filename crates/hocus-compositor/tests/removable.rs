//! Removable drives (docs/design/bar.md "Removable drives", issue #898):
//! the real shell, against a fake UDisks2 (`levels/udisks.rs`) and a fake
//! NetworkManager on the fakes' private bus, never the machine's.
//!
//! One scenario, the spec's: a USB stick plugged in shows its bubble
//! ("Clé USB « KINGSTON » branchée", "Ouvrir" and "Éjecter"; its words
//! are the shell's unit tests'),
//! the computer's own disk and a loop device none; "Ouvrir" mounts it
//! (`Filesystem.Mount`) and opens its folder with `xdg-open` (the test's
//! own); its row comes in the connections, between the Wi-Fi's and the
//! keyboard layout's, with its free space, and the desktops move up for
//! it; ⏏ while it is in use says so with "Réessayer"; ⏏ again unmounts it
//! and powers it off, the row goes and "Tu peux retirer KINGSTON" shows.
//!
//! With `HOCUS_KIT_SHOTS=DIR`, the bubbles and the connections are saved
//! there as PNGs.
//!
//! Pixman here, so the shell draws on the CPU, without motion: as in CI.

use crate::common;
use crate::fakes;
use crate::nm;
use crate::udisks;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use common::WAIT;
use fakes::Fakes;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use udisks::{Call, Udisks};

/// What statvfs says the stick has free, for the shell (bytes).
const FREE: &str = "14200000000";

struct Bed {
    fakes: Fakes,
    udisks: Udisks,
    _nm: nm::Nm,
    hp: Headless,
    log: PathBuf,
}

impl Bed {
    fn start(shell: &Path) -> anyhow::Result<Self> {
        let fakes = Fakes::start()?;
        let setup = nm::Setup {
            aps: vec![nm::FakeAp::new("Maison", 80, nm::Security::Psk("t"))],
            saved: vec![("Maison", Some("t"))],
            current: Some("Maison"),
            ..nm::Setup::default()
        };
        let nm = nm::Nm::start(fakes.runtime(), &fakes.address, setup)?;
        let media = fakes.dir.path().join("media");
        let udisks = Udisks::start(fakes.runtime(), &fakes.address, media)?;
        let log = fakes.dir.path().join("shell.log");
        // The file manager: a script of the test's that says what it was
        // given.
        let bin = fakes.dir.path().join("bin");
        std::fs::create_dir_all(&bin)?;
        let opener = bin.join("xdg-open");
        std::fs::write(
            &opener,
            format!(
                "#!/bin/sh\necho \"$@\" >> '{}'\n",
                fakes.dir.path().join("opened").display()
            ),
        )?;
        std::fs::set_permissions(&opener, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
        let env = [
            (
                "RUST_LOG",
                "warn,hocuspanda_shell=debug,hocus_removable=debug".to_owned(),
            ),
            ("HOCUS_DRIVES_FREE", FREE.to_owned()),
            ("PATH", format!("{}:/usr/bin:/bin", bin.display())),
        ];
        let script = fakes.shell_with(shell, "fr_FR.UTF-8", &env, Some(&log))?;
        let mut config = Config::default();
        config.keyboard.layouts = vec!["fr".to_owned(), "us".to_owned()];
        let hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
            Some(script),
        )?;
        let mut bed = Self {
            fakes,
            udisks,
            _nm: nm,
            hp,
            log,
        };
        // The Wi-Fi's row and the layout's.
        bed.surface_is(194, 72)?;
        Ok(bed)
    }

    fn surface(&self) -> Rect {
        self.hp
            .connections_surfaces()
            .first()
            .map_or(Rect::new(0, 0, 0, 0), |(_, r)| *r)
    }

    fn desktops(&self) -> Rect {
        self.hp
            .desktops_surfaces()
            .first()
            .map_or(Rect::new(0, 0, 0, 0), |(_, r)| *r)
    }

    /// Waits until the connections' surface is `width` × `height`, drawn.
    fn surface_is(&mut self, width: i32, height: i32) -> anyhow::Result<()> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            hp.connections_surfaces()
                .first()
                .is_some_and(|(_, r)| (r.width, r.height) == (width, height))
                && hp.connections_commits() > 0
        })?;
        anyhow::ensure!(done, "the connections' surface: {:?}", self.surface());
        common::wait(&mut self.hp, Duration::from_millis(300))
    }

    /// Waits until the fake has been asked `call` `times` times.
    fn asked(&mut self, call: &Call, times: usize) -> anyhow::Result<()> {
        let udisks = &self.udisks;
        let done = self.hp.dispatch_until(WAIT, |_| {
            udisks.calls().iter().filter(|c| *c == call).count() >= times
        })?;
        anyhow::ensure!(done, "not asked {call:?}: {:?}", self.udisks.calls());
        Ok(())
    }

    /// Waits until one bubble shows, saying `summary`: its namespace,
    /// where it is, and what the kit's dump says of it.
    fn bubble(&mut self, summary: &str) -> anyhow::Result<(String, Rect, serde_json::Value)> {
        let log = self.log.clone();
        let mut seen = None;
        self.hp.dispatch_until(WAIT, |hp| {
            let dump = bubble_dump(&log, summary);
            let shown = hp.bubble_surfaces();
            if let (Some(dump), [(namespace, _, rect)]) = (dump, shown.as_slice()) {
                seen = Some((namespace.clone(), *rect, dump));
            }
            seen.is_some()
        })?;
        let seen = seen.ok_or_else(|| {
            let said = common::logged(&self.log);
            let drives: Vec<&str> = said
                .lines()
                .filter(|l| l.contains("removable") || l.contains("drive") || l.contains("bubble"))
                .collect();
            anyhow::anyhow!(
                "no bubble saying {summary:?}: {:?}\n{}",
                self.hp.bubble_surfaces(),
                drives.join("\n")
            )
        })?;
        common::drawn(&mut self.hp, &seen.0)?;
        Ok(seen)
    }

    /// A click on the bubble's control `id` (`bubble:Action(open)`).
    fn press(
        &mut self,
        bubble: &(String, Rect, serde_json::Value),
        id: &str,
    ) -> anyhow::Result<()> {
        let (namespace, rect, dump) = bubble;
        let at = dump[id]["at"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("no {id} in {dump}"))?;
        let n = |i: usize| at.get(i).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        let point = (
            f64::from(rect.x) + n(0) + n(2) / 2.0,
            f64::from(rect.y) + n(1) + n(3) / 2.0,
        );
        common::click_on(&mut self.hp, point, namespace)
    }

    /// The drive's row's ⏏: the middle row of three, its button on the
    /// row's right.
    fn eject(&mut self) -> anyhow::Result<()> {
        let s = self.surface();
        let lowest = f64::from(s.y + s.height) - 5.5 - 15.0;
        let at = (f64::from(s.x + s.width) - 8.0 - 11.0, lowest - 32.0);
        common::click_on(&mut self.hp, at, hocus_link::CONNECTIONS_NAMESPACE)
    }

    fn shot(&mut self, namespace: &str, name: &str) -> anyhow::Result<()> {
        common::layer_shot(&mut self.hp, namespace, name)
    }

    fn opened(&self) -> String {
        std::fs::read_to_string(self.fakes.dir.path().join("opened")).unwrap_or_default()
    }
}

/// The kit's dump of the bubble that shows saying `summary`, once it is
/// drawn: the stack says its title, the kit its controls after it.
fn bubble_dump(log: &Path, summary: &str) -> Option<serde_json::Value> {
    let text = common::logged(log);
    let title = format!("title=\"{summary}\"");
    let lines: Vec<&str> = text.lines().collect();
    let shows = lines
        .iter()
        .rposition(|l| l.contains("a bubble shows") && l.contains(&title))?;
    let line = lines[shows..]
        .iter()
        .rev()
        .find(|l| l.contains(" shows {") && l.contains("bubble:Close"))?;
    let at = line.find(" shows {")? + " shows ".len();
    serde_json::from_str(&line[at..]).ok()
}

#[test]
fn a_stick_plugged_in_opens_shows_its_row_and_ejects() -> anyhow::Result<()> {
    let shell = common::shell_program();
    if !fakes::available() {
        common::missing("dbus-daemon, pipewire, pw-dump, wpctl and pw-metadata");
    }
    let mut bed = Bed::start(&shell)?;
    let desktops = bed.desktops();
    // The computer's own disk and the loop device show nothing.
    assert!(bed.hp.bubble_surfaces().is_empty());
    // Plugged in: its bubble, its size alone (not mounted yet).
    bed.udisks.plug()?;
    let bubble = bed.bubble("Clé USB « KINGSTON » branchée")?;
    let said = bubble.2.to_string();
    assert!(bubble.2["bubble:Action(eject)"].is_object(), "{said}");
    bed.shot(&bubble.0, "898-bubble")?;
    // "Ouvrir": mounted, opened in the file manager.
    bed.press(&bubble, "bubble:Action(open)")?;
    bed.asked(&Call::Mount("sdb1".to_owned()), 1)?;
    let fakes_dir = bed.fakes.dir.path().to_owned();
    let opened = bed.hp.dispatch_until(WAIT, |_| {
        std::fs::read_to_string(fakes_dir.join("opened"))
            .is_ok_and(|o| o.contains("media/KINGSTON"))
    })?;
    assert!(opened, "the file manager was given {:?}", bed.opened());
    // Its row: one more, and the desktops over the connections move up.
    bed.surface_is(194, 104)?;
    let moved = bed.hp.dispatch_until(WAIT, |hp| {
        hp.desktops_surfaces()
            .first()
            .is_some_and(|(_, r)| r.y + r.height == desktops.y + desktops.height - 32)
    })?;
    assert!(
        moved,
        "the desktops: {:?}, were {desktops:?}",
        bed.desktops()
    );
    let log = common::logged(&bed.log);
    assert!(log.contains("KINGSTON"), "the row never said its drive");
    bed.shot(hocus_link::CONNECTIONS_NAMESPACE, "898-connections")?;
    // ⏏ while something uses it: said, with "Réessayer"; still there.
    bed.udisks.hold(true);
    bed.eject()?;
    bed.asked(&Call::Unmount("sdb1".to_owned()), 1)?;
    let busy = bed.bubble("KINGSTON est encore utilisée")?;
    let said = busy.2.to_string();
    assert!(busy.2["bubble:Action(retry)"].is_object(), "{said}");
    bed.shot(&busy.0, "898-busy")?;
    assert!(bed.udisks.plugged());
    assert_eq!(bed.surface().height, 104);
    // Let go: "Réessayer" unmounts it and powers it off; its row goes.
    bed.udisks.hold(false);
    bed.press(&busy, "bubble:Action(retry)")?;
    bed.asked(&Call::Unmount("sdb1".to_owned()), 2)?;
    bed.asked(
        &Call::PowerOff("Kingston_DataTraveler_3_0_408D5C1A".to_owned()),
        1,
    )?;
    let removed = bed.bubble("Tu peux retirer KINGSTON")?;
    bed.shot(&removed.0, "898-removed")?;
    bed.surface_is(194, 72)?;
    // Its time up, it goes.
    let gone = bed.hp.dispatch_until(Duration::from_secs(12), |hp| {
        hp.bubble_surfaces().is_empty()
    })?;
    assert!(gone, "{:?}", bed.hp.bubble_surfaces());
    // Plugged again and opened from its bubble, its ⏏ ejects it at once.
    bed.udisks.plug()?;
    let again = bed.bubble("Clé USB « KINGSTON » branchée")?;
    bed.press(&again, "bubble:Action(open)")?;
    bed.surface_is(194, 104)?;
    bed.eject()?;
    bed.asked(
        &Call::PowerOff("Kingston_DataTraveler_3_0_408D5C1A".to_owned()),
        2,
    )?;
    bed.surface_is(194, 72)?;
    assert!(!bed.udisks.plugged());
    Ok(())
}
