//! Givre (#740), for the maintainer's eyes: the bar with its agents'
//! pandas in their visor, the calendar open beside it, two bubbles (one
//! with its actions, one urgent) and the lock screen, all in Givre, on
//! the default wallpaper; on GLES (Mesa's software device, or the GPU
//! `HOCUS_TEST_GPU` names) for the glass, else on the CPU (the opaque
//! fallback). The real shell, its notifications on a D-Bus daemon of the
//! test's own, its home, state and runtime directories the test's own.
//!
//! `HOCUS_SHOTS=DIR cargo nextest run -p hocus-compositor --test
//! givre_shots --run-ignored only` (the shell and `hocus` built first).

mod common;

use std::{
    collections::HashMap,
    io::Write as _,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

use common::{BTN_LEFT, WAIT};
use hocus_compositor::{Authenticator, Config, Headless};
use zbus::zvariant::{OwnedVariant, Variant};

const SCREEN: (i32, i32) = (1920, 1080);
const BAR_LEFT: i32 = 1920 - 8 - 196;
/// The date under the clock: a click opens the calendar.
const DATE: (f64, f64) = (1814.0, 82.0);

const DAY: &str = "[[calendars]]\nid = \"work\"\nname = \"Travail\"\ncolor = \"#8FB3FF\"\n\n\
                   [[events]]\ntitle = \"Point produit\"\nstart = \"today 15:00\"\nend = \"today 15:30\"\n";
const FILE: &str = "[keyboard]\nlayouts = [\"us\"]\n\n[appearance]\ntheme = \"frost\"\naccent = \"ice\"\n\n\
                    [agents]\npeek = \"off\"\n\n\
                    [[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n";

struct Bed {
    hp: Headless,
    _dir: tempfile::TempDir,
    log: PathBuf,
    hocus: PathBuf,
    bus: Child,
    runtime: tokio::runtime::Runtime,
    client: zbus::Connection,
}

impl Drop for Bed {
    fn drop(&mut self) {
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}

fn start() -> anyhow::Result<Option<Bed>> {
    let Some(shell) = common::shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return Ok(None);
    };
    let exe = std::env::current_exe()?;
    let Some(hocus) = exe
        .parent()
        .and_then(Path::parent)
        .map(|d| d.join("hocus"))
        .filter(|h| h.exists())
    else {
        eprintln!("hocus is not built: build the workspace to run this test");
        return Ok(None);
    };
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    for sub in ["home/bin", "config/hocuspanda", "state/hocuspanda", "cache"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    let file = root.join("config/hocuspanda/config.toml");
    std::fs::write(&file, FILE)?;
    std::fs::write(root.join("config/hocuspanda/day.toml"), DAY)?;
    // The day's accounts chosen: no first-run card.
    std::fs::write(root.join("state/hocuspanda/day-sources"), "later\n")?;
    let (bus, address) = common::private_bus(root)?;
    let at = |sub: &str| root.join(sub).display().to_string();
    let log = root.join("shell.log");
    let script = root.join("shell");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             HOME='{}' XDG_CONFIG_HOME='{}' XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' \
             TZ=UTC HOCUS_CLOCK_AT='2026-11-07T14:32:07' HOCUS_USER_NAME=Alex \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 ICED_BACKEND=tiny-skia \
             PATH='{}:/usr/bin:/bin' RUST_LOG=warn,hocuspanda_shell=debug\n\
             exec '{}' \"$@\" 2>>'{}'\n",
            at("home"),
            at("config"),
            at("cache"),
            at("state"),
            at("home/bin"),
            shell.display(),
            log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let mut config = Config::load(&file);
    config.file = Some(file);
    config.french = true;
    config.lock_auth = Authenticator::test("not a password");
    let mut headless = common::gles();
    headless.outputs = vec![format!("{}x{}", SCREEN.0, SCREEN.1).parse()?];
    let mut hp = Headless::start_on_bus(headless, config, Some(script), Some(address.clone()))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let client = runtime.block_on(zbus::connection::Builder::address(address.as_str()).build())?;
    let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    hp.pointer_move(400.0, 600.0);
    let mut bed = Bed {
        hp,
        _dir: dir,
        log,
        hocus,
        bus,
        runtime,
        client,
    };
    bed.logged("the theme theme=frost")?;
    bed.logged("the notifications are served")?;
    Ok(Some(bed))
}

impl Bed {
    fn logged(&mut self, what: &str) -> anyhow::Result<()> {
        let log = self.log.clone();
        let seen = self
            .hp
            .dispatch_until(WAIT, |_| common::logged(&log).contains(what))?;
        anyhow::ensure!(seen, "the shell never said {what:?}");
        Ok(())
    }

    /// `hocus agent NAME STATE`, once the compositor's model has it.
    fn agent_is(&mut self, name: &str, state: &str) -> anyhow::Result<()> {
        let socket = self
            .hp
            .control_socket()
            .ok_or_else(|| anyhow::anyhow!("no control socket"))?
            .to_path_buf();
        let mut child = Command::new(&self.hocus)
            .arg("--socket")
            .arg(socket)
            .args(["agent", name, state])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        if let Some(mut input) = child.stdin.take() {
            input.flush()?;
        }
        let done = self
            .hp
            .dispatch_until(WAIT, |_| matches!(child.try_wait(), Ok(Some(_))))?;
        anyhow::ensure!(done, "hocus agent {name} {state} never ended");
        let known = self.hp.dispatch_until(WAIT, |hp| {
            hp.agents_peek()
                .iter()
                .any(|a| a.name == name && a.state == state)
        })?;
        anyhow::ensure!(known, "{name} not {state}");
        Ok(())
    }

    fn notify(
        &mut self,
        app: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        urgency: u8,
    ) -> anyhow::Result<()> {
        let mut hints: HashMap<&str, OwnedVariant> = HashMap::new();
        hints.insert("urgency", OwnedVariant::try_from(Variant::U8(urgency))?);
        let call = (
            app,
            0_u32,
            "",
            summary,
            body,
            actions.to_vec(),
            hints,
            -1_i32,
        );
        let client = self.client.clone();
        self.runtime.block_on(async move {
            client
                .call_method(
                    Some("org.freedesktop.Notifications"),
                    "/org/freedesktop/Notifications",
                    Some("org.freedesktop.Notifications"),
                    "Notify",
                    &call,
                )
                .await
        })?;
        Ok(())
    }

    /// The file holding `text`, renamed over it as the Settings write it.
    fn rewrite(&self, text: &str) -> anyhow::Result<()> {
        let folder = self._dir.path().join("config/hocuspanda");
        std::fs::write(folder.join(".config.toml.new"), text)?;
        std::fs::rename(folder.join(".config.toml.new"), folder.join("config.toml"))?;
        Ok(())
    }

    /// The calendar, opened from the date once the day is read.
    fn calendar(&mut self) -> anyhow::Result<()> {
        self.logged("the day holds")?;
        let log = self.log.clone();
        let lit = |text: &str, from: usize| {
            text.get(from..).is_some_and(|t| {
                t.lines()
                    .any(|l| l.contains("the clock's group is drawn") && l.contains("hover: true"))
            })
        };
        let mut hovered = false;
        for _ in 0..50 {
            let from = common::logged(&log).len();
            common::point(&mut self.hp, (DATE.0, 500.0))?;
            common::wait(&mut self.hp, Duration::from_millis(50))?;
            common::point(&mut self.hp, DATE)?;
            common::wait(&mut self.hp, Duration::from_millis(100))?;
            if lit(&common::logged(&log), from) {
                hovered = true;
                break;
            }
        }
        anyhow::ensure!(hovered, "the clock's group never lit under the pointer");
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        let open = self
            .hp
            .dispatch_until(WAIT, |hp| common::drawn_popovers(hp).len() == 1)?;
        anyhow::ensure!(open, "no calendar");
        // Off the bar: no hover in the picture.
        self.hp.pointer_move(400.0, 900.0);
        Ok(())
    }

    /// The screen's `(x, y, w, h)` saved as `DIR/NAME`, once at rest.
    fn save(
        &mut self,
        out: &Path,
        name: &str,
        (x, y, w, h): (i32, i32, i32, i32),
    ) -> anyhow::Result<()> {
        common::at_rest(&mut self.hp)?;
        common::wait(&mut self.hp, Duration::from_millis(300))?;
        let (width, height, px) = self.hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(width as u32, height as u32, rgba)
            .ok_or_else(|| anyhow::anyhow!("no image"))?;
        let image =
            image::imageops::crop_imm(&image, x as u32, y as u32, w as u32, h as u32).to_image();
        let path = out.join(name);
        image.save(&path)?;
        eprintln!("{} ({})", path.display(), self.hp.renderer());
        Ok(())
    }
}

#[test]
#[ignore = "screenshots for the maintainer: HOCUS_SHOTS=DIR"]
fn screenshots_of_givre() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let Some(mut bed) = start()? else {
        return Ok(());
    };
    for (name, state) in [
        ("docs", "working"),
        ("api-paiements", "working"),
        ("migration", "done"),
        ("infra", "failed"),
        ("site-web", "waiting"),
    ] {
        bed.agent_is(name, state)?;
    }
    bed.notify(
        "Slack",
        "Léa Martin",
        "Tu peux regarder la PR #812 avant le point de 15 h ?",
        &["reply", "Répondre", "read", "Marquer comme lu"],
        1,
    )?;
    bed.notify(
        "Batterie",
        "Batterie à 5 %",
        "Branche le chargeur : la machine se met en veille dans 4 min.",
        &[],
        2,
    )?;
    let shown = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.bubble_surfaces().len() >= 2)?;
    anyhow::ensure!(shown, "no bubbles");
    // Past the pandas' bursts and the bubbles' arrival.
    common::wait(&mut bed.hp, Duration::from_secs(4))?;
    let right = (
        BAR_LEFT - 8 - 420,
        0,
        SCREEN.0 - (BAR_LEFT - 8 - 420),
        SCREEN.1,
    );
    bed.save(&out, "givre-bar-bubbles.png", right)?;
    bed.calendar()?;
    common::wait(&mut bed.hp, Duration::from_secs(1))?;
    bed.save(&out, "givre-bar-calendar.png", right)?;
    // The same in the dark theme, to compare, then Givre again.
    bed.rewrite(&FILE.replace("theme = \"frost\"\naccent = \"ice\"", ""))?;
    bed.logged("the theme theme=dark")?;
    common::wait(&mut bed.hp, Duration::from_secs(1))?;
    bed.save(&out, "dark-bar-calendar.png", right)?;
    bed.rewrite(FILE)?;
    let log = bed.log.clone();
    let back = bed.hp.dispatch_until(WAIT, |_| {
        common::logged(&log)
            .lines()
            .rev()
            .find(|l| l.contains("the theme theme="))
            .is_some_and(|l| l.contains("theme=frost"))
    })?;
    anyhow::ensure!(back, "never Givre again");
    bed.hp.lock();
    bed.logged("the lock is drawn")?;
    common::wait(&mut bed.hp, Duration::from_secs(4))?;
    bed.save(&out, "givre-lock.png", (0, 0, SCREEN.0, SCREEN.1))?;
    Ok(())
}
