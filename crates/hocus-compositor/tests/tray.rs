//! The tray (5.14, issue #531): the real shell serving
//! `org.kde.StatusNotifierWatcher` on a D-Bus daemon of the test's own
//! (never the session's), and an item of the test's own on it: a
//! `StatusNotifierItem` with its `com.canonical.dbusmenu` menu, made here
//! with zbus. No real application.
//!
//! One scenario per point of the approved spec: an item registers and its
//! icon shows, in the full form then the mini; `NewIcon` draws it again,
//! the dot its pixmap holds with it; `Passive` hides it, `NeedsAttention`
//! shows its attention icon; a left click activates it, a middle click its
//! second action, the wheel scrolls it; a right click opens its menu, and a
//! row sends `Event("clicked")` then closes it; the item's owner going
//! takes its icon away; resting 400 ms on it shows its tooltip.
//!
//! Pixman here, so the shell draws on the CPU, as in CI.

mod common;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use common::{BTN_LEFT, KEY_LEFTMETA, WAIT, chord, wait};
use hocus_compositor::{Config, FormChoice, Headless, HeadlessConfig, RendererChoice, model::Rect};
use zbus::{
    Array, Dict, OwnedObjectPath, OwnedVariant, Signature, Structure, Variant, names::BusName,
};

const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;
const KEY_B: u32 = 48;

const ITEM: &str = "org.kde.StatusNotifierItem";
const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";

type Pixmaps = Vec<(i32, i32, Vec<u8>)>;

/// A 22 × 22 pixmap of `rgb`, ARGB32 in network order, with a 8 × 8 dot
/// of `dot` at its top right.
fn pixmap(rgb: [u8; 3], dot: Option<[u8; 3]>) -> Pixmaps {
    let mut bytes = Vec::with_capacity(22 * 22 * 4);
    for y in 0..22 {
        for x in 0..22 {
            let [r, g, b] = match dot {
                Some(dot) if x >= 14 && y < 8 => dot,
                _ => rgb,
            };
            bytes.extend([255, r, g, b]);
        }
    }
    vec![(22, 22, bytes)]
}

const MAGENTA: [u8; 3] = [255, 0, 255];
const GREEN: [u8; 3] = [0, 200, 80];
const RED: [u8; 3] = [240, 30, 40];
const BLUE: [u8; 3] = [30, 90, 250];

/// What the item says, as the test sets it.
struct Said {
    status: String,
    icon: Pixmaps,
    attention: Pixmaps,
}

/// What the item and its menu were asked, in order.
type Heard = Arc<Mutex<Vec<String>>>;

fn hear(heard: &Heard, what: String) {
    heard
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(what);
}

struct Item {
    said: Arc<Mutex<Said>>,
    heard: Heard,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    fn activate(&self, x: i32, y: i32) {
        hear(&self.heard, format!("Activate {x} {y}"));
    }

    fn secondary_activate(&self, x: i32, y: i32) {
        hear(&self.heard, format!("SecondaryActivate {x} {y}"));
    }

    fn scroll(&self, delta: i32, orientation: String) {
        hear(&self.heard, format!("Scroll {delta} {orientation}"));
    }

    fn context_menu(&self, x: i32, y: i32) {
        hear(&self.heard, format!("ContextMenu {x} {y}"));
    }

    #[zbus(property)]
    fn category(&self) -> String {
        "Communications".to_owned()
    }

    #[zbus(property)]
    fn id(&self) -> String {
        "slack".to_owned()
    }

    #[zbus(property)]
    fn title(&self) -> String {
        "Slack".to_owned()
    }

    #[zbus(property)]
    fn status(&self) -> String {
        self.said
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .status
            .clone()
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> Pixmaps {
        self.said
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .icon
            .clone()
    }

    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> Pixmaps {
        self.said
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .attention
            .clone()
    }

    #[zbus(property)]
    fn tool_tip(&self) -> (String, Pixmaps, String, String) {
        (
            String::new(),
            Vec::new(),
            "Vous avez 2 notifications".to_owned(),
            String::new(),
        )
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(MENU_PATH).unwrap_or_else(|_| unreachable!())
    }
}

/// An entry of the menu: `(ia{sv}av)`.
fn entry(
    id: i32,
    props: &[(&str, Variant<'static>)],
    children: Vec<Variant<'static>>,
) -> Variant<'static> {
    let mut dict = Dict::new(&Signature::Str, &Signature::Variant);
    for (k, v) in props {
        let value = v.try_clone().unwrap_or_else(|_| Variant::from(""));
        let _ = dict.append(
            Variant::from((*k).to_owned()),
            Variant::Variant(Box::new(value)),
        );
    }
    let mut array = Array::new(&Signature::Variant);
    for child in children {
        let _ = array.append(Variant::Variant(Box::new(child)));
    }
    Variant::Structure(
        Structure::builder()
            .append_field(Variant::I32(id))
            .append_field(Variant::Dict(dict))
            .append_field(Variant::Array(array))
            .build()
            .unwrap_or_else(|_| unreachable!()),
    )
}

struct Menu {
    heard: Heard,
}

type Layout = (i32, HashMap<String, OwnedVariant>, Vec<OwnedVariant>);

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl Menu {
    fn get_layout(&self, _parent: i32, _depth: i32, _names: Vec<String>) -> (u32, Layout) {
        let label = |l: &str| ("label", Variant::from(l.to_owned()));
        let rows = vec![
            entry(1, &[label("_Ouvrir Slack")], vec![]),
            entry(
                2,
                &[("type", Variant::from("separator".to_owned()))],
                vec![],
            ),
            entry(
                3,
                &[
                    label("Ne pas déranger"),
                    ("toggle-type", Variant::from("checkmark".to_owned())),
                    ("toggle-state", Variant::I32(1)),
                ],
                vec![],
            ),
            entry(
                4,
                &[label("Préférences…"), ("enabled", Variant::Bool(false))],
                vec![],
            ),
            entry(5, &[label("Quitter Slack")], vec![]),
        ];
        let children = rows
            .into_iter()
            .filter_map(|r| OwnedVariant::try_from(r).ok())
            .collect();
        let mut props = HashMap::new();
        if let Ok(v) = OwnedVariant::try_from(Variant::from("submenu".to_owned())) {
            props.insert("children-display".to_owned(), v);
        }
        (1, (0, props, children))
    }

    fn event(&self, id: i32, event: String, _data: OwnedVariant, _stamp: u32) {
        hear(&self.heard, format!("Event {id} {event}"));
    }

    fn about_to_show(&self, _id: i32) -> bool {
        false
    }
}

struct Bed {
    hp: Headless,
    _dir: tempfile::TempDir,
    log: PathBuf,
    bus: Child,
    address: String,
    runtime: tokio::runtime::Runtime,
}

impl Drop for Bed {
    fn drop(&mut self) {
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}

/// The test's item on the bus, and what it was asked.
struct Client {
    connection: Option<zbus::Connection>,
    said: Arc<Mutex<Said>>,
    heard: Heard,
}

impl Bed {
    fn start(form: FormChoice) -> anyhow::Result<Option<Self>> {
        let Some(shell) = common::shell_program() else {
            eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
            return Ok(None);
        };
        let daemon = Command::new("sh")
            .args(["-c", "command -v dbus-daemon"])
            .stdout(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !daemon {
            eprintln!("no dbus-daemon: skipped");
            return Ok(None);
        }
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config", "cache", "state", "data"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let (bus, address) = private_bus(root)?;
        let log = root.join("shell.log");
        let at = |sub: &str| root.join(sub).display().to_string();
        let script = common::shell_script(
            root,
            &shell,
            &[
                ("LC_ALL", "fr_FR.UTF-8".to_owned()),
                ("LANG", "fr_FR.UTF-8".to_owned()),
                ("HOME", at("home")),
                ("XDG_CONFIG_HOME", at("config")),
                ("XDG_CACHE_HOME", at("cache")),
                ("XDG_STATE_HOME", at("state")),
                ("XDG_DATA_HOME", at("data")),
                ("XDG_DATA_DIRS", at("data")),
                ("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned()),
            ],
        )?;
        // Its errors to the log.
        let text = std::fs::read_to_string(&script)?
            .replace("\"$@\"\n", &format!("\"$@\" 2>>'{}'\n", log.display()));
        std::fs::write(&script, text)?;
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        config.panel.form = form;
        let hp = Headless::start_on_bus(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Pixman,
                render_node: None,
            },
            config,
            Some(script),
            Some(address.clone()),
        )?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let mut bed = Self {
            hp,
            _dir: dir,
            log,
            bus,
            address,
            runtime,
        };
        let up = bed
            .hp
            .dispatch_until(Duration::from_secs(20), |hp| !hp.bell_surfaces().is_empty())?;
        anyhow::ensure!(up, "no footer");
        bed.logged("the tray is served")?;
        bed.hp.pointer_move(400.0, 300.0);
        wait(&mut bed.hp, Duration::from_millis(200))?;
        Ok(Some(bed))
    }

    fn plain(&self) -> String {
        plain(&self.log)
    }

    /// Waits for the shell to log `text` after `from`: the line.
    fn logged_after(&mut self, from: usize, text: &str) -> anyhow::Result<String> {
        let log = self.log.clone();
        let done = self.hp.dispatch_until(WAIT, |_| {
            plain(&log).get(from..).is_some_and(|l| l.contains(text))
        })?;
        let after = plain(&log).get(from..).unwrap_or_default().to_owned();
        anyhow::ensure!(done, "the shell never logged {text:?}; it logged:\n{after}");
        Ok(after
            .lines()
            .rfind(|l| l.contains(text))
            .unwrap_or_default()
            .to_owned())
    }

    fn logged(&mut self, text: &str) -> anyhow::Result<String> {
        self.logged_after(0, text)
    }

    /// An item registers, `status` and showing `icon`.
    fn item(&mut self, status: &str, icon: Pixmaps) -> anyhow::Result<Client> {
        let said = Arc::new(Mutex::new(Said {
            status: status.to_owned(),
            icon,
            attention: pixmap(BLUE, None),
        }));
        let heard = Heard::default();
        let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
        let item = Item {
            said: said.clone(),
            heard: heard.clone(),
        };
        let menu = Menu {
            heard: heard.clone(),
        };
        let address = self.address.clone();
        let connection = self.runtime.block_on(async {
            let connection = zbus::connection::Builder::address(address.as_str())
                .name(name.as_str())
                .serve_at(ITEM_PATH, item)
                .serve_at(MENU_PATH, menu)
                .build()
                .await?;
            connection
                .call_method(
                    Some("org.kde.StatusNotifierWatcher"),
                    "/StatusNotifierWatcher",
                    Some("org.kde.StatusNotifierWatcher"),
                    "RegisterStatusNotifierItem",
                    &(name.as_str(),),
                )
                .await?;
            anyhow::Ok(connection)
        })?;
        Ok(Client {
            connection: Some(connection),
            said,
            heard,
        })
    }

    /// The item says `member` (`NewIcon`, `NewStatus`).
    fn signal(&self, client: &Client, member: &str) -> anyhow::Result<()> {
        let Some(connection) = &client.connection else {
            return Ok(());
        };
        let status = client
            .said
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .status
            .clone();
        self.runtime.block_on(async {
            if member == "NewStatus" {
                connection
                    .emit_signal(
                        None::<BusName<'_>>,
                        ITEM_PATH,
                        ITEM,
                        member,
                        &(status.as_str(),),
                    )
                    .await
            } else {
                connection
                    .emit_signal(None::<BusName<'_>>, ITEM_PATH, ITEM, member, &())
                    .await
            }
        })?;
        Ok(())
    }

    /// The tray's surface, once it shows.
    fn tray(&mut self) -> anyhow::Result<Rect> {
        let shows = self.hp.dispatch_until(WAIT, |hp| {
            !hp.layer_rects(hocus_link::TRAY_NAMESPACE).is_empty()
        })?;
        anyhow::ensure!(shows, "no tray");
        // Drawn.
        wait(&mut self.hp, Duration::from_millis(400))?;
        Ok(self.hp.layer_rects(hocus_link::TRAY_NAMESPACE)[0].1)
    }

    fn no_tray(&mut self) -> anyhow::Result<()> {
        let gone = self.hp.dispatch_until(WAIT, |hp| {
            hp.layer_rects(hocus_link::TRAY_NAMESPACE).is_empty()
        })?;
        anyhow::ensure!(gone, "the tray stayed");
        Ok(())
    }

    /// The first icon's centre on the screen, its tray at `tray`.
    fn icon(&self, tray: Rect) -> (i32, i32) {
        let full = tray.width > 100;
        if full {
            (tray.x + 11 + 17, tray.y + 13 + 17)
        } else {
            (tray.x + 7 + 20, tray.y + 13 + 16)
        }
    }

    /// The pixel at (`x`, `y`): red, green, blue.
    fn pixel(&mut self, (x, y): (i32, i32)) -> anyhow::Result<[u8; 3]> {
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let at = usize::try_from(y * width + x)? * 4;
        let p = bytes
            .get(at..at + 4)
            .ok_or_else(|| anyhow::anyhow!("({x}, {y}) off the screen"))?;
        Ok([p[2], p[1], p[0]])
    }

    /// Waits for the pixel at `at` to be near `rgb`.
    fn shows(&mut self, at: (i32, i32), rgb: [u8; 3]) -> anyhow::Result<()> {
        let start = Instant::now();
        loop {
            let p = self.pixel(at)?;
            if near(p, rgb) {
                return Ok(());
            }
            anyhow::ensure!(start.elapsed() < WAIT, "{at:?} shows {p:?}, not {rgb:?}");
            wait(&mut self.hp, Duration::from_millis(50))?;
        }
    }

    /// The pointer on the first icon, once the tray takes it there.
    fn over_icon(&mut self, tray: Rect) -> anyhow::Result<(f64, f64)> {
        let (x, y) = self.icon(tray);
        let at = (f64::from(x), f64::from(y));
        common::over(&mut self.hp, at, hocus_link::TRAY_NAMESPACE)?;
        Ok(at)
    }

    fn press(&mut self, button: u32) {
        self.hp.button(button, true);
        self.hp.button(button, false);
    }

    /// Waits for the item to have heard `what`.
    fn heard(&mut self, client: &Client, what: &str) -> anyhow::Result<()> {
        let heard = client.heard.clone();
        let done = self.hp.dispatch_until(WAIT, |_| {
            heard
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .any(|h| h.starts_with(what))
        })?;
        anyhow::ensure!(
            done,
            "the item never heard {what:?}: {:?}",
            client.heard.lock().unwrap_or_else(PoisonError::into_inner)
        );
        Ok(())
    }
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 24)
}

/// A D-Bus daemon of the test's own in `dir`, and its address.
fn private_bus(dir: &Path) -> anyhow::Result<(Child, String)> {
    let socket = dir.join("bus");
    let config = dir.join("bus.conf");
    std::fs::write(
        &config,
        format!(
            "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
             \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
             <busconfig><type>session</type><listen>unix:path={}</listen>\
             <policy context=\"default\"><allow send_destination=\"*\"/>\
             <allow receive_sender=\"*\"/><allow own=\"*\"/></policy></busconfig>\n",
            socket.display()
        ),
    )?;
    let bus = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", config.display()))
        .arg("--nofork")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let start = Instant::now();
    while !socket.exists() {
        anyhow::ensure!(start.elapsed() < Duration::from_secs(10), "no bus");
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok((bus, format!("unix:path={}", socket.display())))
}

/// The shell's log, without its colours' escape sequences.
fn plain(log: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default()).into_owned();
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn an_item_registers_and_its_icon_shows_in_both_forms() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FormChoice::Full)? else {
        return Ok(());
    };
    let levels_before = bed.hp.levels_surfaces()[0].1;
    let _item = bed.item("Active", pixmap(MAGENTA, None))?;
    let tray = bed.tray()?;
    // Just above the footer, across the bar's inside, a row of one.
    let footer = bed.hp.bell_surfaces()[0].1;
    assert_eq!(tray.width, 194, "{tray:?}");
    assert!(tray.y + tray.height <= footer.y + 6, "{tray:?} {footer:?}");
    let icon = bed.icon(tray);
    bed.shows(icon, MAGENTA)?;
    bed.logged("the tray shows it")?;
    // The levels above it went up, taller by what it takes.
    let levels = bed.hp.levels_surfaces()[0].1;
    assert!(levels.height > levels_before.height, "{levels:?}");
    // The mini form: a column, the icon in it.
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_B]);
    let mini = bed.hp.dispatch_until(WAIT, |hp| {
        hp.layer_rects(hocus_link::TRAY_NAMESPACE)
            .first()
            .is_some_and(|(_, r)| r.width == 54)
    })?;
    anyhow::ensure!(mini, "no mini tray");
    wait(&mut bed.hp, Duration::from_millis(500))?;
    let tray = bed.tray()?;
    let icon = bed.icon(tray);
    bed.shows(icon, MAGENTA)?;
    Ok(())
}

#[test]
fn a_new_icon_draws_it_again_its_dot_with_it() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FormChoice::Full)? else {
        return Ok(());
    };
    let item = bed.item("Active", pixmap(GREEN, None))?;
    let tray = bed.tray()?;
    let (x, y) = bed.icon(tray);
    // The icon spans the cell's middle ± 11 px: its dot's middle 7 px right
    // of the icon's and 7 px over it.
    let dot = (x + 7, y - 7);
    bed.shows((x, y), GREEN)?;
    bed.shows(dot, GREEN)?;
    item.said
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .icon = pixmap(GREEN, Some(RED));
    bed.signal(&item, "NewIcon")?;
    bed.shows(dot, RED)?;
    bed.shows((x - 5, y + 5), GREEN)?;
    Ok(())
}

#[test]
fn passive_hides_it_and_needing_attention_shows_its_attention_icon() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FormChoice::Full)? else {
        return Ok(());
    };
    let item = bed.item("Active", pixmap(MAGENTA, None))?;
    bed.tray()?;
    item.said
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .status = "Passive".to_owned();
    bed.signal(&item, "NewStatus")?;
    bed.no_tray()?;
    item.said
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .status = "NeedsAttention".to_owned();
    bed.signal(&item, "NewStatus")?;
    let tray = bed.tray()?;
    let icon = bed.icon(tray);
    bed.shows(icon, BLUE)?;
    Ok(())
}

#[test]
fn clicks_and_the_wheel_go_to_the_item() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FormChoice::Full)? else {
        return Ok(());
    };
    let item = bed.item("Active", pixmap(MAGENTA, None))?;
    let tray = bed.tray()?;
    bed.over_icon(tray)?;
    bed.press(BTN_LEFT);
    bed.heard(&item, "Activate")?;
    bed.press(BTN_MIDDLE);
    bed.heard(&item, "SecondaryActivate")?;
    bed.hp.wheel(1);
    bed.heard(&item, "Scroll ")?;
    let heard = item
        .heard
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let scroll = heard
        .iter()
        .find(|h| h.starts_with("Scroll"))
        .cloned()
        .unwrap_or_default();
    assert!(scroll.ends_with(" vertical"), "{scroll}");
    let delta: i32 = scroll.split(' ').nth(1).unwrap_or("0").parse()?;
    assert_eq!(delta.abs(), 120, "{scroll}");
    // Its menu opened by none of these.
    assert!(bed.hp.popover_surfaces().is_empty());
    Ok(())
}

#[test]
fn a_right_click_opens_its_menu_and_a_row_sends_clicked() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FormChoice::Full)? else {
        return Ok(());
    };
    let item = bed.item("Active", pixmap(MAGENTA, None))?;
    let tray = bed.tray()?;
    bed.over_icon(tray)?;
    let from = bed.plain().len();
    bed.press(BTN_RIGHT);
    let open = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_surfaces().len() == 1)?;
    anyhow::ensure!(open, "no menu");
    let line = bed.logged_after(from, "the tray's menu shows it")?;
    // Its rows, the separator, the one off, the mnemonic's mark gone.
    for row in [
        "Ouvrir Slack@",
        "—",
        "Ne pas déranger@",
        "Préférences…@",
        "Quitter Slack@",
    ] {
        assert!(line.contains(row), "{row} in {line}");
    }
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let popover = bed.hp.popover_surfaces()[0].1;
    // Beside the bar, its bottom on the icon's.
    assert!(
        popover.x + popover.width <= tray.x - 6,
        "{popover:?} {tray:?}"
    );
    let icon_bottom = bed.icon(tray).1 + 17;
    assert!(
        (popover.y + popover.height - icon_bottom).abs() <= 2,
        "{popover:?} {icon_bottom}"
    );
    let y: f64 = line
        .split("Quitter Slack@")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .unwrap_or("0")
        .parse()?;
    let at = (f64::from(popover.x + 60), f64::from(popover.y) + y);
    common::over(&mut bed.hp, at, hocus_link::POPOVER_NAMESPACE)?;
    bed.press(BTN_LEFT);
    bed.heard(&item, "Event 5 clicked")?;
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    anyhow::ensure!(closed, "the menu stayed");
    Ok(())
}

#[test]
fn the_items_owner_going_takes_its_icon_away() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FormChoice::Full)? else {
        return Ok(());
    };
    let mut item = bed.item("Active", pixmap(MAGENTA, None))?;
    bed.tray()?;
    let connection = item.connection.take();
    bed.runtime.block_on(async move {
        if let Some(connection) = connection {
            let _ = connection.graceful_shutdown().await;
        }
    });
    bed.no_tray()?;
    bed.logged("a tray item went")?;
    Ok(())
}

#[test]
fn resting_on_an_icon_400_ms_shows_its_tooltip() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(FormChoice::Full)? else {
        return Ok(());
    };
    let _item = bed.item("Active", pixmap(MAGENTA, None))?;
    let tray = bed.tray()?;
    bed.over_icon(tray)?;
    let rested = Instant::now();
    let shows = bed.hp.dispatch_until(WAIT, |hp| {
        !hp.layer_rects(hocus_link::TIP_NAMESPACE).is_empty()
    })?;
    anyhow::ensure!(shows, "no tooltip");
    // The pointer came to rest at most 30 ms before `over` saw it there.
    assert!(
        rested.elapsed() >= Duration::from_millis(340),
        "{:?}",
        rested.elapsed()
    );
    let line = bed.logged("the tray's tooltip shows")?;
    assert!(
        line.contains("Slack") && line.contains("Vous avez 2 notifications"),
        "{line}"
    );
    let tip = bed.hp.layer_rects(hocus_link::TIP_NAMESPACE)[0].1;
    assert!(tip.x + tip.width <= tray.x - 6, "{tip:?} {tray:?}");
    // The pointer leaving the icon takes it away.
    bed.hp.pointer_move(400.0, 300.0);
    let gone = bed.hp.dispatch_until(WAIT, |hp| {
        hp.layer_rects(hocus_link::TIP_NAMESPACE).is_empty()
    })?;
    anyhow::ensure!(gone, "the tooltip stayed");
    Ok(())
}
