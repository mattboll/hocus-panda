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
//! And a click on its icon brings its application forward (5.14b, issue
//! #537): the item and the application's window are this program's, behind
//! the window of another program, which has the keyboard. With an item
//! that takes the click's token (`ProvideXdgActivationToken`), and with one
//! that ignores it and makes its own; without a click, its rim only pulses;
//! the click lets it focus once, within 2 s; an application hidden in the
//! tray opens its window focused.
//!
//! Pixman here, so the shell draws on the CPU, as in CI.

use crate::common;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use common::{
    BTN_LEFT, ClientHandle, Command as Do, KEY_LEFTMETA, KEY_LEFTSHIFT, Shows, WAIT, chord, wait,
};
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
    app: Option<App>,
}

/// The application behind an item: its Wayland client, whether its item
/// takes the click's token, and what it does when activated.
struct App {
    commands: std::sync::mpsc::Sender<Do>,
    takes_token: bool,
    reacts: Reacts,
    token: Mutex<Option<String>>,
}

/// What an application does when its item is activated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reacts {
    /// Nothing yet: the test has it ask later.
    Waits,
    /// Its window asks for the focus, with the token it was given, else
    /// with one of its own.
    Activates,
    /// It opens its window, hidden until then.
    Opens,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    fn activate(&self, x: i32, y: i32) {
        hear(&self.heard, format!("Activate {x} {y}"));
        let Some(app) = &self.app else {
            return;
        };
        let _ = match app.reacts {
            Reacts::Waits => Ok(()),
            Reacts::Activates => {
                let token = app
                    .token
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take();
                app.commands.send(Do::Activate(token))
            }
            Reacts::Opens => app.commands.send(Do::Open {
                app_id: APP,
                size: (400, 300),
            }),
        };
    }

    /// Kept for its `Activate`; an item that ignores it has no such
    /// method, as Electron's.
    fn provide_xdg_activation_token(&self, token: String) -> zbus::fdo::Result<()> {
        match &self.app {
            Some(app) if app.takes_token => {
                hear(&self.heard, format!("ProvideXdgActivationToken {token}"));
                *app.token.lock().unwrap_or_else(PoisonError::into_inner) = Some(token);
                Ok(())
            }
            _ => Err(zbus::fdo::Error::UnknownMethod(
                "ProvideXdgActivationToken".to_owned(),
            )),
        }
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
        let shell = common::shell_program();
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
        let (bus, address) = common::private_bus(root)?;
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
        let up = bed.hp.dispatch_until(Duration::from_secs(20), |hp| {
            !hp.drawn_layers(hocus_link::BELL_NAMESPACE).is_empty()
        })?;
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
        self.item_of(status, icon, None)
    }

    /// The same, `app`'s.
    fn item_of(&mut self, status: &str, icon: Pixmaps, app: Option<App>) -> anyhow::Result<Client> {
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
            app,
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
        // Drawn at its place, and the frame shown at rest: else its
        // place and pixels are not its own yet.
        let tray = common::drawn(&mut self.hp, hocus_link::TRAY_NAMESPACE)?;
        common::at_rest(&mut self.hp)?;
        Ok(tray[0].1)
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
    let levels_before = common::drawn(&mut bed.hp, hocus_link::LEVELS_NAMESPACE)?[0].1;
    let _item = bed.item("Active", pixmap(MAGENTA, None))?;
    let tray = bed.tray()?;
    // Just above the footer, across the bar's inside, a row of one.
    let footer = bed.hp.bell_surfaces()[0].1;
    assert_eq!(tray.width, 194, "{tray:?}");
    assert!(tray.y + tray.height <= footer.y + 6, "{tray:?} {footer:?}");
    let icon = bed.icon(tray);
    bed.shows(icon, MAGENTA)?;
    bed.logged("the tray shows it")?;
    // The levels above it went up, taller by what it takes: once the
    // shell has drawn them again, which may come after the tray.
    let taller = bed.hp.dispatch_until(WAIT, |hp| {
        hp.drawn_layers(hocus_link::LEVELS_NAMESPACE)
            .first()
            .is_some_and(|(_, r)| r.height > levels_before.height)
    })?;
    assert!(taller, "{:?}", bed.hp.levels_surfaces());
    // The mini form: a column, the icon in it.
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_B]);
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
    // Where it is drawn: 0 × 0 at its anchor before its first buffer.
    let tip = common::drawn(&mut bed.hp, hocus_link::TIP_NAMESPACE)?[0].1;
    assert!(tip.x + tip.width <= tray.x - 6, "{tip:?} {tray:?}");
    // The pointer leaving the icon takes it away.
    bed.hp.pointer_move(400.0, 300.0);
    let gone = bed.hp.dispatch_until(WAIT, |hp| {
        hp.layer_rects(hocus_link::TIP_NAMESPACE).is_empty()
    })?;
    anyhow::ensure!(gone, "the tooltip stayed");
    Ok(())
}

// --- A click brings its application forward (5.14b, issue #537) ------------------

/// The application's window.
const APP: &str = "test.app";
/// The other program's window, in front with the keyboard.
const FRONT: &str = "test.front";

/// Run by the tests below as a process of its own, given the compositor's
/// socket in `HOCUS_TEST_FRONT`: a window of another program, until the
/// compositor goes. Run alone, it does nothing.
#[test]
fn a_window_of_another_program() -> anyhow::Result<()> {
    let Some(socket) = std::env::var_os("HOCUS_TEST_FRONT") else {
        return Ok(());
    };
    let shows = Shows::Window {
        app_id: FRONT,
        size: (900, 700),
        popup: false,
    };
    let client = common::client(Path::new(&socket), Some(shows))?;
    // Its events end with its connection, as the compositor goes.
    while client.events.recv().is_ok() {}
    Ok(())
}

/// The other program, stopped by its own PID when the test ends.
struct Front(Child);

impl Drop for Front {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The scene: the other program's window with the keyboard; the
/// application's behind it (none, `hidden`), with its tray item, which
/// takes the click's token or not and `reacts` so; the tray's icon.
struct Scene {
    bed: Bed,
    _front: Front,
    app: ClientHandle,
    item: Client,
    tray: Rect,
}

impl Scene {
    fn start(takes_token: bool, reacts: Reacts, hidden: bool) -> anyhow::Result<Option<Self>> {
        let Some(mut bed) = Bed::start(FormChoice::Full)? else {
            return Ok(None);
        };
        let child = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "tray::a_window_of_another_program",
                "--nocapture",
            ])
            .env("HOCUS_TEST_FRONT", bed.hp.socket())
            .stdout(Stdio::null())
            .spawn()?;
        let front = Front(child);
        let shown = bed.hp.dispatch_until(WAIT, |hp| has_keyboard(hp, FRONT))?;
        anyhow::ensure!(shown, "the other program's window: {:?}", bed.hp.windows());
        let app = if hidden {
            common::client(bed.hp.socket(), None)?
        } else {
            let shows = Shows::Window {
                app_id: APP,
                size: (400, 300),
                popup: false,
            };
            let (_, app) = common::open_showing(&mut bed.hp, APP, shows)?;
            app
        };
        let item = bed.item_of(
            "Active",
            pixmap(MAGENTA, None),
            Some(App {
                commands: app.commands.clone(),
                takes_token,
                reacts,
                token: Mutex::new(None),
            }),
        )?;
        let tray = bed.tray()?;
        let scene = Self {
            bed,
            _front: front,
            app,
            item,
            tray,
        };
        if !hidden {
            scene.behind()?;
        }
        Ok(Some(scene))
    }

    /// A left click on the item's icon, once its item heard it: when.
    fn click(&mut self) -> anyhow::Result<Instant> {
        let before = self.heard("Activate ");
        self.bed.over_icon(self.tray)?;
        let at = Instant::now();
        self.bed.press(BTN_LEFT);
        let item = self.item.heard.clone();
        let heard = self
            .bed
            .hp
            .dispatch_until(WAIT, |_| count(&item, "Activate ") > before)?;
        anyhow::ensure!(heard, "the item never heard the click");
        Ok(at)
    }

    /// How many times the item heard `what`.
    fn heard(&self, what: &str) -> usize {
        count(&self.item.heard, what)
    }

    /// The application's window comes forward, with the keyboard.
    fn forward(&mut self) -> anyhow::Result<()> {
        let came = self.bed.hp.dispatch_until(WAIT, |hp| {
            has_keyboard(hp, APP)
                && hp
                    .windows()
                    .last()
                    .and_then(|w| w.app_id.clone())
                    .as_deref()
                    == Some(APP)
        })?;
        anyhow::ensure!(came, "it stayed behind: {:?}", self.bed.hp.windows());
        Ok(())
    }

    /// The application's window stays behind, its rim pulsing.
    fn pulses(&mut self) -> anyhow::Result<()> {
        let pulsed = self
            .bed
            .hp
            .dispatch_until(WAIT, |hp| hp.pulse(APP).is_some())?;
        anyhow::ensure!(pulsed, "no pulse: {:?}", self.bed.hp.windows());
        self.behind()
    }

    /// The application's window behind the other's, which has the
    /// keyboard.
    fn behind(&self) -> anyhow::Result<()> {
        let windows = self.bed.hp.windows();
        let at = |app: &str| {
            windows
                .iter()
                .position(|w| w.app_id.as_deref() == Some(app))
        };
        anyhow::ensure!(
            at(APP).is_some() && at(APP) < at(FRONT),
            "not behind: {windows:?}"
        );
        anyhow::ensure!(has_keyboard(&self.bed.hp, FRONT), "{windows:?}");
        Ok(())
    }

    /// The application asks for the focus (`token`, else one of its own).
    fn activate(&self, token: Option<String>) {
        let _ = self.app.commands.send(Do::Activate(token));
    }

    /// A click on the other program's window, where the application's is
    /// not: it takes the keyboard back.
    fn back_to_front(&mut self) -> anyhow::Result<()> {
        let windows = self.bed.hp.windows();
        let rect = |app: &str| {
            windows
                .iter()
                .find(|w| w.app_id.as_deref() == Some(app))
                .map(|w| w.rect)
        };
        let (Some(front), Some(app)) = (rect(FRONT), rect(APP)) else {
            anyhow::bail!("{windows:?}");
        };
        let inside = |r: Rect, (x, y): (i32, i32)| {
            x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
        };
        let at = [
            (front.x + 20, front.y + 20),
            (front.x + front.width - 20, front.y + 20),
            (front.x + 20, front.y + front.height - 20),
            (front.x + front.width - 20, front.y + front.height - 20),
        ]
        .into_iter()
        .find(|p| !inside(app, *p))
        .ok_or_else(|| anyhow::anyhow!("{front:?} under {app:?}"))?;
        self.bed.hp.pointer_move(f64::from(at.0), f64::from(at.1));
        self.bed.press(BTN_LEFT);
        let back = self
            .bed
            .hp
            .dispatch_until(WAIT, |hp| has_keyboard(hp, FRONT))?;
        anyhow::ensure!(back, "{:?}", self.bed.hp.windows());
        Ok(())
    }
}

fn has_keyboard(hp: &Headless, app: &str) -> bool {
    hp.windows()
        .iter()
        .any(|w| w.app_id.as_deref() == Some(app) && w.focused && w.keyboard)
}

fn count(heard: &Heard, what: &str) -> usize {
    heard
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter(|h| h.starts_with(what))
        .count()
}

/// Longer than a click's 2 s.
const PAST_THE_CLICK: Duration = Duration::from_millis(2300);

#[test]
fn a_click_gives_its_token_to_an_item_that_takes_it() -> anyhow::Result<()> {
    let Some(mut scene) = Scene::start(true, Reacts::Waits, false)? else {
        return Ok(());
    };
    scene.click()?;
    // The token first, then `Activate`.
    let heard = scene
        .item
        .heard
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let given = heard
        .iter()
        .position(|h| h.starts_with("ProvideXdgActivationToken "));
    let activated = heard.iter().position(|h| h.starts_with("Activate "));
    assert!(given.is_some() && given < activated, "{heard:?}");
    let token = heard[given.unwrap_or_default()]
        .trim_start_matches("ProvideXdgActivationToken ")
        .to_owned();
    // The token alone is enough: used past the click's 2 s, it still
    // brings the window forward.
    wait(&mut scene.bed.hp, PAST_THE_CLICK)?;
    scene.behind()?;
    scene.activate(Some(token));
    scene.forward()
}

#[test]
fn a_click_brings_forward_an_application_that_makes_its_own_token() -> anyhow::Result<()> {
    let Some(mut scene) = Scene::start(false, Reacts::Activates, false)? else {
        return Ok(());
    };
    scene.click()?;
    scene.forward()?;
    assert_eq!(scene.heard("ProvideXdgActivationToken"), 0);
    Ok(())
}

#[test]
fn without_a_click_its_rim_only_pulses() -> anyhow::Result<()> {
    let Some(mut scene) = Scene::start(false, Reacts::Activates, false)? else {
        return Ok(());
    };
    scene.activate(None);
    scene.pulses()
}

#[test]
fn a_click_lets_it_take_the_focus_once_within_2_s() -> anyhow::Result<()> {
    let Some(mut scene) = Scene::start(false, Reacts::Waits, false)? else {
        return Ok(());
    };
    // Asked 2 s after the click: too late.
    scene.click()?;
    wait(&mut scene.bed.hp, PAST_THE_CLICK)?;
    scene.activate(None);
    scene.pulses()?;
    // Asked at once: it comes; asked again, from behind, within the 2 s:
    // only its rim.
    let clicked = scene.click()?;
    scene.activate(None);
    scene.forward()?;
    scene.back_to_front()?;
    scene.activate(None);
    scene.pulses()?;
    assert!(
        clicked.elapsed() < Duration::from_secs(2),
        "too slow to tell: {:?}",
        clicked.elapsed()
    );
    Ok(())
}

#[test]
fn an_application_hidden_in_the_tray_opens_its_window_focused() -> anyhow::Result<()> {
    let Some(mut scene) = Scene::start(false, Reacts::Opens, true)? else {
        return Ok(());
    };
    assert!(common::rect_of(&scene.bed.hp, APP).is_none());
    scene.click()?;
    scene.forward()
}
