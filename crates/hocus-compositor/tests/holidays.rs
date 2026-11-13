//! The garlands proposed in December (#813, docs/design/effects.md): on a
//! held date, a notification server of the test's own on a D-Bus daemon
//! of its own (never the session's). In December, both garlands off: one
//! notification, "Des guirlandes pour les fêtes ?", Oui / Non; Oui writes
//! both switches on into the configuration file; not asked again that
//! year, even by the next session; asked again the next December.

use crate::common;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use common::WAIT;
use hocus_compositor::{Config, Headless, HeadlessConfig, OutputSpec, RendererChoice};

/// What the server was told: each notification's summary and actions.
type Said = Arc<Mutex<Vec<(String, Vec<String>)>>>;

struct Server(Said);

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        _app: String,
        _replaces: u32,
        _icon: String,
        summary: String,
        _body: String,
        actions: Vec<String>,
        _hints: HashMap<String, zbus::OwnedVariant>,
        _timeout: i32,
    ) -> u32 {
        let mut said = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        said.push((summary, actions));
        said.len() as u32
    }
}

/// A notifications server on a bus of the test's own.
struct Notifications {
    address: String,
    said: Said,
    daemon: std::process::Child,
    connection: zbus::Connection,
    runtime: tokio::runtime::Runtime,
}

impl Drop for Notifications {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

impl Notifications {
    fn start(dir: &Path) -> anyhow::Result<Self> {
        if std::process::Command::new("dbus-daemon")
            .arg("--version")
            .output()
            .is_err()
        {
            common::missing("dbus-daemon");
        }
        let (daemon, address) = common::private_bus(dir)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let said = Said::default();
        let connection = runtime.block_on(
            zbus::connection::Builder::address(address.as_str())
                .name("org.freedesktop.Notifications")
                .serve_at("/org/freedesktop/Notifications", Server(said.clone()))
                .build(),
        )?;
        Ok(Self {
            address,
            said,
            daemon,
            connection,
            runtime,
        })
    }

    fn said(&self) -> Vec<(String, Vec<String>)> {
        self.said
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The user chooses `key` on notification `id`.
    fn choose(&self, id: u32, key: &str) -> anyhow::Result<()> {
        self.runtime.block_on(self.connection.emit_signal(
            None::<&str>,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "ActionInvoked",
            &(id, key),
        ))?;
        Ok(())
    }
}

/// A compositor on `bus`, its configuration file `file` and its state in
/// `state`, in French.
fn start(bus: &str, file: &Path, state: &Path) -> anyhow::Result<Headless> {
    let config = Config {
        lang: hocus_words::Lang::French,
        file: Some(file.to_path_buf()),
        state_dir: Some(state.to_path_buf()),
        ..Config::default()
    };
    Headless::start_on_bus(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1280, 800)],
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        config,
        None,
        Some(bus.to_owned()),
    )
}

fn idle(hp: &mut Headless, d: Duration) -> anyhow::Result<()> {
    hp.dispatch_until(d, |_| false)?;
    Ok(())
}

fn read(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn in_december_once_a_year_and_yes_turns_both_on() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("config.toml");
    let state = dir.path().join("state");
    std::fs::write(
        &file,
        "# My desktop, by hand.\n[keyboard]\nrepeat-rate = 30\n",
    )?;
    let bus_dir = dir.path().join("bus");
    std::fs::create_dir_all(&bus_dir)?;
    let server = Notifications::start(&bus_dir)?;
    let mut hp = start(&server.address, &file, &state)?;
    // November: nothing.
    hp.holidays_date(2026, 11, 30)?;
    idle(&mut hp, Duration::from_millis(500))?;
    assert!(server.said().is_empty(), "{:?}", server.said());
    // The first of December: asked, once.
    hp.holidays_date(2026, 12, 1)?;
    let asked = hp.dispatch_until(WAIT, |_| !server.said().is_empty())?;
    anyhow::ensure!(asked, "not asked");
    let said = server.said();
    assert_eq!(
        said,
        [(
            "Des guirlandes pour les fêtes ?".to_owned(),
            ["yes", "Oui", "no", "Non"].map(str::to_owned).to_vec()
        )]
    );
    let asked = state.join("garlands-asked");
    let kept = hp.dispatch_until(WAIT, |_| read(&asked).trim() == "2026")?;
    anyhow::ensure!(kept, "the year not kept: {:?}", read(&asked));
    // Oui: both written on, the rest of the file kept.
    server.choose(1, "yes")?;
    let written = hp.dispatch_until(WAIT, |_| {
        let text = read(&file);
        text.contains("garland = true") && text.contains("garland-top = true")
    })?;
    anyhow::ensure!(written, "{}", read(&file));
    assert!(
        read(&file).starts_with("# My desktop, by hand.\n[keyboard]\nrepeat-rate = 30\n"),
        "{}",
        read(&file)
    );
    // The next day: not again.
    hp.holidays_date(2026, 12, 2)?;
    idle(&mut hp, Duration::from_millis(500))?;
    assert_eq!(server.said().len(), 1);
    drop(hp);
    // The next session, that December (its garlands off again): not
    // again; the next December, asked.
    let mut hp = start(&server.address, &file, &state)?;
    hp.holidays_date(2026, 12, 24)?;
    idle(&mut hp, Duration::from_millis(500))?;
    assert_eq!(server.said().len(), 1);
    hp.holidays_date(2027, 12, 1)?;
    let again = hp.dispatch_until(WAIT, |_| server.said().len() == 2)?;
    anyhow::ensure!(again, "not asked the next December");
    Ok(())
}

/// The proposal for the maintainer, into `$HOCUS_SHOTS`: the real shell
/// serving the notifications on the test's bus, its bubble on the first
/// of December. Run by hand (`--run-ignored only`).
#[test]
#[ignore = "screenshots, by hand"]
fn shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let dir = tempfile::tempdir()?;
    let at = |sub: &str| dir.path().join(sub).display().to_string();
    for sub in ["home", "config", "cache", "state", "data", "bus"] {
        std::fs::create_dir_all(dir.path().join(sub))?;
    }
    let (mut daemon, address) = common::private_bus(&dir.path().join("bus"))?;
    let script = common::shell_script(
        dir.path(),
        &common::shell_program(),
        &[
            ("LC_ALL", "fr_FR.UTF-8".to_owned()),
            ("LANG", "fr_FR.UTF-8".to_owned()),
            ("HOME", at("home")),
            ("XDG_CONFIG_HOME", at("config")),
            ("XDG_CACHE_HOME", at("cache")),
            ("XDG_STATE_HOME", at("state")),
            ("XDG_DATA_HOME", at("data")),
        ],
    )?;
    let file = dir.path().join("config.toml");
    std::fs::write(&file, "")?;
    let mut config = Config {
        lang: hocus_words::Lang::French,
        file: Some(file),
        state_dir: Some(dir.path().join("state/hocuspanda")),
        ..Config::default()
    };
    config.wallpaper.picture = hocus_link::wallpaper::Picture::of("night").unwrap_or_default();
    let shot = (|| {
        let mut hp = Headless::start_on_bus(
            HeadlessConfig {
                outputs: vec![OutputSpec::new(1440, 900)],
                ..common::gles()
            },
            config,
            Some(script),
            Some(address),
        )?;
        let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
        anyhow::ensure!(up, "no bar");
        common::wait(&mut hp, Duration::from_secs(2))?;
        hp.holidays_date(2026, 12, 1)?;
        common::wait(&mut hp, Duration::from_secs(2))?;
        let (w, h, px) = hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
            .ok_or_else(|| anyhow::anyhow!("no image"))?;
        let path = out.join("garlands-december.png");
        image.save(&path)?;
        eprintln!("{}", path.display());
        Ok(())
    })();
    let _ = daemon.kill();
    let _ = daemon.wait();
    shot
}
