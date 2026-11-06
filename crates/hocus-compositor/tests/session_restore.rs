//! The last session's applications come back at log in, each window in
//! its place (docs/design/windows.md §14, issue #725), headless: a first
//! compositor saves its windows into its state directory, a second one,
//! on the same directory, starts their applications again (this test's
//! own program, [`restored_program`], through a desktop entry of a fake
//! XDG tree) and puts each window back: screen, desktop, place, state.
//! The autostart's applications are not started twice; one that does
//! not come back is named; `[session] restore = false` restores nothing.

mod common;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use common::{Shows, WAIT, client, open, wait};
use hocus_compositor::{
    Config, Headless, HeadlessConfig, OutputSpec, RendererChoice,
    model::{Mode, Rect, WindowId, Zone},
};
use hocus_link::WindowAction;

const APP: &str = "test.restored";
const RESTORED_APP: &str = "HOCUS_TEST_RESTORED_APP";
const RESTORED_COUNT: &str = "HOCUS_TEST_RESTORED_COUNT";

/// Started again by the compositor from a desktop entry: as many windows
/// of [`RESTORED_APP`] as [`RESTORED_COUNT`] says, each on a connection of
/// its own, until the compositor goes. Run alone, it does nothing.
#[test]
fn restored_program() -> anyhow::Result<()> {
    let Ok(app) = std::env::var(RESTORED_APP) else {
        return Ok(());
    };
    let app: &'static str = app.leak();
    let count: usize = std::env::var(RESTORED_COUNT)
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1);
    let display = PathBuf::from(std::env::var_os("WAYLAND_DISPLAY").unwrap_or_default());
    let socket = if display.is_absolute() {
        display
    } else {
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default()).join(display)
    };
    let clients = (0..count)
        .map(|_| {
            client(
                &socket,
                Some(Shows::Window {
                    app_id: app,
                    size: (500, 400),
                    popup: false,
                }),
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    // Its events end with its connection, as the compositor goes.
    for client in clients {
        while client.events.recv().is_ok() {}
    }
    Ok(())
}

/// A fake XDG tree: the state directory, the applications' entries and
/// the autostart's.
struct Tree {
    dir: tempfile::TempDir,
}

impl Tree {
    fn new() -> anyhow::Result<Self> {
        let tree = Self {
            dir: tempfile::tempdir()?,
        };
        for sub in ["state", "applications", "autostart", "marks"] {
            std::fs::create_dir_all(tree.path(sub))?;
        }
        Ok(tree)
    }

    fn path(&self, sub: &str) -> PathBuf {
        self.dir.path().join(sub)
    }

    /// The entry `id`, named `name`, running `exec`.
    fn entry(&self, dir: &str, id: &str, name: &str, exec: &str) -> anyhow::Result<()> {
        std::fs::write(
            self.path(dir).join(format!("{id}.desktop")),
            format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n"),
        )?;
        Ok(())
    }

    /// The entry that starts [`restored_program`] with `count` windows of
    /// `app`.
    fn restored_entry(&self, app: &str, count: usize) -> anyhow::Result<()> {
        let exe = std::env::current_exe()?;
        self.entry(
            "applications",
            app,
            "Restored",
            &format!(
                "env {RESTORED_APP}={app} {RESTORED_COUNT}={count} \"{}\" --exact restored_program --nocapture",
                exe.display()
            ),
        )
    }

    /// A command that adds a line to `marks/name`.
    fn marking(&self, name: &str) -> String {
        format!(
            "sh -c \"echo x >> '{}'\"",
            self.path("marks").join(name).display()
        )
    }

    fn marks(&self, name: &str) -> usize {
        std::fs::read_to_string(self.path("marks").join(name))
            .map(|t| t.lines().count())
            .unwrap_or(0)
    }

    fn config(&self) -> Config {
        Config {
            state_dir: Some(self.path("state")),
            applications: vec![self.path("applications")],
            ..Config::default()
        }
    }

    fn saved(&self) -> String {
        std::fs::read_to_string(self.path("state").join("session")).unwrap_or_default()
    }

    fn save(&self, text: &str) -> anyhow::Result<()> {
        std::fs::write(self.path("state").join("session"), text)?;
        Ok(())
    }
}

fn headless(outputs: &[OutputSpec]) -> HeadlessConfig {
    HeadlessConfig {
        outputs: outputs.to_vec(),
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    }
}

fn two() -> Vec<OutputSpec> {
    vec![OutputSpec::new(1920, 1080), OutputSpec::new(2560, 1440)]
}

/// A compositor on `tree` as a session's: saving, restoring once the
/// bar shows (a clear bar, here, as it shows now).
fn session(
    tree: &Tree,
    outputs: &[OutputSpec],
    config: Config,
    autostart: bool,
) -> anyhow::Result<Headless> {
    let mut hp = Headless::start_with(headless(outputs), config)?;
    if autostart {
        hp.autostart(vec![tree.path("autostart")]);
    }
    hp.restore_session();
    Ok(hp)
}

fn bar(hp: &mut Headless) -> anyhow::Result<()> {
    let bar = client(hp.socket(), Some(Shows::ClearBar))?;
    std::mem::forget(bar);
    Ok(())
}

/// The newest window of `app`.
fn newest(hp: &Headless, app: &str) -> anyhow::Result<WindowId> {
    hp.placements()
        .into_iter()
        .filter(|p| p.app_id.as_deref() == Some(app))
        .map(|p| p.id)
        .max()
        .ok_or_else(|| anyhow::anyhow!("no window {app}"))
}

/// Where a window is, as the session keeps it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Kept {
    output: Option<String>,
    desktop: usize,
    mode: String,
    rect: (i32, i32, i32, i32),
    above: bool,
    everywhere: bool,
}

fn kept(hp: &Headless, app: &str) -> Vec<Kept> {
    let everywhere = hp.everywhere();
    let mut out: Vec<Kept> = hp
        .placements()
        .into_iter()
        .filter(|p| p.app_id.as_deref() == Some(app))
        .map(|p| Kept {
            output: p.output.clone(),
            desktop: p.desktop,
            mode: format!("{:?}", p.mode),
            rect: (p.rect.x, p.rect.y, p.rect.width, p.rect.height),
            above: hp.above(p.id),
            everywhere: everywhere.contains(&p.id),
        })
        .collect();
    out.sort();
    out
}

/// Three windows of [`APP`] and one of an application without an entry,
/// arranged on two screens: one floating on desktop 2 of the first,
/// kept above; one maximised on the second; one floating on all
/// desktops. Saved.
fn first_session(tree: &Tree) -> anyhow::Result<Vec<Kept>> {
    let mut hp = session(tree, &two(), tree.config(), false)?;
    bar(&mut hp)?;
    hp.pointer_move(400.0, 400.0);
    open(&mut hp, APP, (500, 400))?;
    let a = newest(&hp, APP)?;
    hp.place_window(a, Rect::new(120, 140, 700, 500));
    hp.window_action(a, WindowAction::Above);
    hp.window_action(a, WindowAction::NextDesktop);
    open(&mut hp, APP, (500, 400))?;
    let b = newest(&hp, APP)?;
    hp.window_action(b, WindowAction::OtherScreen);
    hp.window_action(b, WindowAction::Maximize);
    open(&mut hp, APP, (500, 400))?;
    let c = newest(&hp, APP)?;
    hp.place_window(c, Rect::new(900, 300, 640, 480));
    hp.window_action(c, WindowAction::AllDesktops);
    open(&mut hp, "test.no-entry", (400, 300))?;
    wait(&mut hp, Duration::from_millis(300))?;
    let before = kept(&hp, APP);
    hp.save_session()?;
    let saved = tree.saved();
    assert_eq!(saved.matches("[[window]]").count(), 3, "{saved}");
    assert!(saved.contains("entry = \"test.restored\""), "{saved}");
    assert!(!saved.contains("test.no-entry"), "{saved}");
    assert!(saved.contains("state = \"maximized\""), "{saved}");
    assert!(saved.contains("screen = \"HEADLESS-2\""), "{saved}");
    Ok(before)
}

#[test]
fn the_windows_come_back_in_their_places_on_their_screens_and_desktops() -> anyhow::Result<()> {
    let tree = Tree::new()?;
    tree.restored_entry(APP, 3)?;
    let before = first_session(&tree)?;
    assert_eq!(before.len(), 3);
    assert!(before.iter().any(|k| k.desktop == 1 && k.above));
    assert!(before.iter().any(|k| k.everywhere));
    assert!(
        before
            .iter()
            .any(|k| k.output.as_deref() == Some("HEADLESS-2")
                && k.mode == format!("{:?}", Mode::Snapped(Zone::Maximized)))
    );

    let mut hp = session(&tree, &two(), tree.config(), false)?;
    // Nothing starts before the bar shows.
    wait(&mut hp, Duration::from_millis(300))?;
    assert!(hp.placements().is_empty());
    assert!(hp.session_restoring());
    bar(&mut hp)?;
    let back = hp.dispatch_until(WAIT, |hp| {
        !hp.session_restoring() && kept(hp, APP).len() == 3
    })?;
    anyhow::ensure!(back, "not back: {:?}", hp.placements());
    wait(&mut hp, Duration::from_millis(300))?;
    assert_eq!(kept(&hp, APP), before);
    assert!(hp.session_not_back().is_empty());
    Ok(())
}

/// A screen not there at log in: the window waits on the screen still
/// there, on its desktop, and goes back once its screen comes.
#[test]
fn a_window_of_a_screen_not_there_goes_back_once_it_comes() -> anyhow::Result<()> {
    let tree = Tree::new()?;
    tree.restored_entry(APP, 3)?;
    let before = first_session(&tree)?;

    let mut hp = session(&tree, &two()[..1], tree.config(), false)?;
    bar(&mut hp)?;
    let back = hp.dispatch_until(WAIT, |hp| {
        !hp.session_restoring() && kept(hp, APP).len() == 3
    })?;
    anyhow::ensure!(back, "not back: {:?}", hp.placements());
    let now = kept(&hp, APP);
    assert!(
        now.iter()
            .all(|k| k.output.as_deref() == Some("HEADLESS-1"))
    );
    assert!(
        now.iter()
            .any(|k| k.mode == format!("{:?}", Mode::Snapped(Zone::Maximized)) && k.desktop == 0)
    );
    hp.plug_screen("HEADLESS-2", OutputSpec::new(2560, 1440), None)?;
    let returned = hp.dispatch_until(WAIT, |hp| kept(hp, APP) == before)?;
    anyhow::ensure!(returned, "{:?}\nnot\n{before:?}", kept(&hp, APP));
    Ok(())
}

/// An application the autostart starts is not started again; its window
/// still goes back.
#[test]
fn an_application_the_autostart_starts_is_not_started_twice() -> anyhow::Result<()> {
    let tree = Tree::new()?;
    tree.entry("applications", "test.both", "Both", &tree.marking("both"))?;
    tree.entry("autostart", "test.both", "Both", &tree.marking("both"))?;
    tree.entry("applications", "test.once", "Once", &tree.marking("once"))?;
    tree.save(
        "[[window]]\nentry = \"test.both\"\napp-id = \"test.both\"\nscreen = \"HEADLESS-1\"\n\
         desktop = 1\nplace = [10, 10, 400, 300]\n\
         [[window]]\nentry = \"test.once\"\napp-id = \"test.once\"\nscreen = \"HEADLESS-1\"\n\
         desktop = 1\nplace = [10, 10, 400, 300]\n",
    )?;
    let mut hp = session(&tree, &two(), tree.config(), true)?;
    bar(&mut hp)?;
    let started = hp.dispatch_until(WAIT, |_| tree.marks("both") > 0 && tree.marks("once") > 0)?;
    anyhow::ensure!(started, "nothing started");
    wait(&mut hp, Duration::from_millis(800))?;
    assert_eq!(tree.marks("both"), 1, "started twice");
    assert_eq!(tree.marks("once"), 1);
    // Its window would still go back: the restore waits for it.
    assert!(hp.session_restoring());
    Ok(())
}

/// The applications that do not come back (no longer installed, a
/// program that does not run, one that ends in error) are named once,
/// in one notification; the others come back.
#[test]
fn those_that_do_not_come_back_are_named_in_one_notification() -> anyhow::Result<()> {
    let tree = Tree::new()?;
    tree.restored_entry(APP, 1)?;
    tree.entry(
        "applications",
        "test.missing",
        "Missing",
        "/nonexistent/program",
    )?;
    tree.entry("applications", "test.fails", "Fails", "false")?;
    let window = |id: &str| {
        format!(
            "[[window]]\nentry = \"{id}\"\napp-id = \"{id}\"\nscreen = \"HEADLESS-1\"\n\
             desktop = 1\nplace = [100, 100, 500, 400]\n"
        )
    };
    tree.save(&format!(
        "{}{}{}{}",
        window(APP),
        window("test.missing"),
        window("test.fails"),
        window("test.uninstalled")
    ))?;
    let notifications = Notifications::start(&tree)?;
    let mut config = tree.config();
    config.french = true;
    let mut hp = match &notifications {
        Some(n) => {
            let mut hp =
                Headless::start_on_bus(headless(&two()), config, None, Some(n.address.clone()))?;
            hp.restore_session();
            hp
        }
        None => session(&tree, &two(), config, false)?,
    };
    bar(&mut hp)?;
    let said = hp.dispatch_until(WAIT, |hp| !hp.session_not_back().is_empty())?;
    anyhow::ensure!(said, "nothing said");
    let mut not_back = hp.session_not_back();
    not_back.sort();
    assert_eq!(not_back, ["Fails", "Missing", "test.uninstalled"]);
    let back = hp.dispatch_until(WAIT, |hp| !hp.session_restoring())?;
    anyhow::ensure!(back, "{:?}", hp.placements());
    assert_eq!(kept(&hp, APP).len(), 1);
    if let Some(n) = notifications {
        let told = hp.dispatch_until(WAIT, |_| !n.said().is_empty())?;
        anyhow::ensure!(told, "no notification");
        let said = n.said();
        assert_eq!(said.len(), 1, "{said:?}");
        for name in ["Fails", "Missing", "test.uninstalled"] {
            assert!(said[0].contains(name), "{said:?}");
        }
        assert!(
            said[0].ends_with("n’ont pas pu être rouvertes."),
            "{said:?}"
        );
    }
    // The session saved now has the one that came back only.
    hp.end_session();
    let saved = tree.saved();
    assert_eq!(saved.matches("[[window]]").count(), 1, "{saved}");
    Ok(())
}

/// `[session] restore = false`: nothing starts, nothing is put back.
#[test]
fn the_setting_off_restores_nothing() -> anyhow::Result<()> {
    let tree = Tree::new()?;
    tree.entry(
        "applications",
        "test.marked",
        "Marked",
        &tree.marking("marked"),
    )?;
    tree.save(
        "[[window]]\nentry = \"test.marked\"\napp-id = \"test.marked\"\n\
         screen = \"HEADLESS-1\"\ndesktop = 1\nplace = [10, 10, 400, 300]\n",
    )?;
    let mut config = tree.config();
    config.session.restore = false;
    let mut hp = session(&tree, &two(), config, false)?;
    bar(&mut hp)?;
    wait(&mut hp, Duration::from_millis(1500))?;
    assert_eq!(tree.marks("marked"), 0);
    assert!(!hp.session_restoring());
    Ok(())
}

/// A notifications server of the test's own, on a session bus of its
/// own; none without `dbus-daemon`.
struct Notifications {
    address: String,
    said: Arc<Mutex<Vec<String>>>,
    daemon: std::process::Child,
    _connection: zbus::Connection,
    _runtime: tokio::runtime::Runtime,
}

impl Drop for Notifications {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

struct Server(Arc<Mutex<Vec<String>>>);

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
        _actions: Vec<String>,
        _hints: HashMap<String, zbus::OwnedVariant>,
        _timeout: i32,
    ) -> u32 {
        let mut said = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        said.push(summary);
        said.len() as u32
    }
}

impl Notifications {
    fn start(tree: &Tree) -> anyhow::Result<Option<Self>> {
        if std::process::Command::new("dbus-daemon")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("no dbus-daemon: the notification is not looked at");
            return Ok(None);
        }
        let dir: &Path = &tree.path("bus");
        std::fs::create_dir_all(dir)?;
        let (daemon, address) = common::private_bus(dir)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let said = Arc::new(Mutex::new(Vec::new()));
        let connection = runtime.block_on(
            zbus::connection::Builder::address(address.as_str())
                .name("org.freedesktop.Notifications")
                .serve_at("/org/freedesktop/Notifications", Server(said.clone()))
                .build(),
        )?;
        Ok(Some(Self {
            address,
            said,
            daemon,
            _connection: connection,
            _runtime: runtime,
        }))
    }

    fn said(&self) -> Vec<String> {
        self.said
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}
