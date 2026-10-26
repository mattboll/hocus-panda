//! The settings window as a Wayland client: the view drawn into
//! shared-memory buffers, pointer and keyboard handed to it. What is in
//! effect comes from the control API on a thread of its own, which follows
//! the event stream; changes go to the configuration file.

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use hocus_api::{Client as Api, Event, OutputInfo, Reply, Request};
use hocus_shell::Lang;
use hocus_ui::{Accent, AccessNode, Color, Key, Point, Size, Theme, ThemeName};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::{
    zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1 as InhibitManager,
    zwp_keyboard_shortcuts_inhibitor_v1::{self, ZwpKeyboardShortcutsInhibitorV1 as Inhibitor},
};

use crate::{
    catalogue::{Context, Page, SystemZone},
    edit,
    view::{BluetoothAction, Listed, Outcome, Pairing, Screen, SettingsView, WifiAction},
};

/// How often the indexer is asked what it does while the Search page
/// shows.
const SEARCH_EVERY: Duration = Duration::from_secs(2);

/// The window's size before the compositor says otherwise.
const SIZE: (u32, u32) = (1200, 820);
const BTN_LEFT: u32 = 0x110;

#[derive(Debug, Clone)]
pub struct Options {
    /// The compositor's Wayland socket.
    pub wayland: PathBuf,
    /// Its control socket.
    pub api: PathBuf,
    pub page: Option<Page>,
    pub lang: Lang,
    pub app_id: String,
    /// Where timedated, AccountsService and fprintd are.
    pub system_bus: SystemBus,
    /// Where the default applications and autostart entries are read and
    /// written.
    pub xdg: Xdg,
    /// The time zone database, whose `zone1970.tab` places the cities of
    /// the map.
    pub zoneinfo: PathBuf,
    /// The user's home, for `~/` in the Search page's folders and the
    /// places never read under it; only its path is read, never what is in
    /// it.
    pub home: Option<PathBuf>,
}

/// The XDG directories the Applications page reads and writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Xdg {
    /// The user's and the machine's, from the environment.
    Machine,
    /// These (tests): the first configuration directory is the user's,
    /// written; the others are only read.
    At(hocus_platform::mime::Dirs),
}

impl Xdg {
    fn dirs(&self) -> hocus_platform::mime::Dirs {
        match self {
            Self::Machine => hocus_platform::mime::Dirs::machine(),
            Self::At(dirs) => dirs.clone(),
        }
    }
}

/// The default applications and autostart entries, read on this thread.
fn read_apps(xdg: &Xdg) -> crate::catalogue::AppsState {
    let dirs = xdg.dirs();
    let lang = std::env::var("LANG").ok();
    let entries = hocus_platform::apps::scan(&dirs.data, lang.as_deref());
    crate::catalogue::AppsState::Known(crate::catalogue::Apps {
        defaults: hocus_platform::mime::defaults(&dirs, &entries),
        autostart: hocus_platform::apps::autostart_entries(&dirs.config),
    })
}

/// Does `work` on a thread of its own, then reads the applications again
/// and tells the window.
fn apps_thread(xdg: Xdg, to_window: Sender<FromApi>, work: impl FnOnce(&Xdg) + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("applications".into())
        .spawn(move || {
            work(&xdg);
            let _ = to_window.send(FromApi::Apps(read_apps(&xdg)));
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "the applications will not be read");
    }
}

/// Does `work` on a thread of its own (a change through AccountsService),
/// then asks the services again and tells the window.
fn account_thread(
    bus: &SystemBus,
    user: String,
    to_window: Sender<FromApi>,
    work: impl FnOnce(&Option<String>, &str) -> Result<(), String> + Send + 'static,
) {
    use hocus_platform::account::{AccountInfo, Fingerprint, UserState};
    let Some(address) = bus.address() else {
        let _ = to_window.send(FromApi::Account(AccountInfo {
            user: UserState::Absent("no system bus".into()),
            fingerprint: Fingerprint::NoService,
        }));
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("account".into())
        .spawn(move || {
            if let Err(why) = work(&address, &user) {
                tracing::warn!(why, "the account stays as it was");
                let _ = to_window.send(FromApi::AccountRefused(why));
            }
            let info = hocus_platform::account::read(&address, &user);
            let _ = to_window.send(FromApi::Account(info));
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "the account will not be read");
    }
}

/// How often the networks in reach are looked for, and read, while the
/// Network page shows.
const SCAN_EVERY: Duration = Duration::from_secs(10);
const READ_EVERY: Duration = Duration::from_secs(2);
/// How often the Bluetooth devices are read while their page shows.
const DEVICES_EVERY: Duration = Duration::from_secs(1);

/// Lists the networks in reach while the Network page shows: scanned for
/// at once and every [`SCAN_EVERY`], read every [`READ_EVERY`]; what the
/// page asks is done here, in turn. Ends when `asked` is dropped.
fn network_worker(bus: Option<String>, to_window: Sender<FromApi>, asked: Receiver<WifiAction>) {
    use hocus_platform::wifi::{JoinError, NetworkManager};
    let nm = match NetworkManager::connect(&bus) {
        Ok(nm) => nm,
        Err(why) => {
            tracing::info!(why, "no NetworkManager");
            let _ = to_window.send(FromApi::Wifi(Listed::Absent(why)));
            // Waits for the page to close, answering what it asks.
            for _ in asked {
                let refused = Err(JoinError::Refused("no NetworkManager".into()));
                let _ = to_window.send(FromApi::WifiDone(refused));
            }
            return;
        }
    };
    let mut scanned: Option<Instant> = None;
    let mut last = None;
    loop {
        if scanned.is_none_or(|at| at.elapsed() >= SCAN_EVERY) {
            nm.scan();
            scanned = Some(Instant::now());
        }
        let now = match nm.read() {
            Ok(wifi) => Listed::Known(wifi),
            Err(why) => Listed::Absent(why),
        };
        if last.as_ref() != Some(&now) {
            if to_window.send(FromApi::Wifi(now.clone())).is_err() {
                return;
            }
            last = Some(now);
        }
        match asked.recv_timeout(READ_EVERY) {
            Ok(action) => {
                let done = match action {
                    WifiAction::Join { ssid, password } => {
                        nm.join(&ssid, password.as_ref().map(|p| p.0.as_str()))
                    }
                    WifiAction::Disconnect(_) => nm.disconnect().map_err(JoinError::Refused),
                    WifiAction::Forget(ssid) => nm.forget(&ssid).map_err(JoinError::Refused),
                };
                if let Err(err) = &done {
                    tracing::info!(?err, "the network did not do what was asked");
                }
                // Read again before it says so: the list shows the result.
                if let Ok(wifi) = nm.read() {
                    let now = Listed::Known(wifi);
                    let _ = to_window.send(FromApi::Wifi(now.clone()));
                    last = Some(now);
                }
                let _ = to_window.send(FromApi::WifiDone(done));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Lists the Bluetooth devices while the Bluetooth page shows, looking
/// for those in reach meanwhile; what the page asks is done here, in
/// turn. Stops looking and ends when `asked` is dropped.
fn bluetooth_worker(
    bus: Option<String>,
    to_window: Sender<FromApi>,
    asked: Receiver<BluetoothAction>,
) {
    use hocus_platform::bluetooth::{Bluetooth, Bluez};
    let bluez = match Bluez::connect(&bus) {
        Ok(bluez) => bluez,
        Err(why) => {
            tracing::info!(why, "no BlueZ");
            let _ = to_window.send(FromApi::Bluetooth(Listed::Absent(why)));
            for _ in asked {
                let _ = to_window.send(FromApi::BluetoothDone(Err("no BlueZ".into())));
            }
            return;
        }
    };
    let mut discovering = false;
    let mut last = None;
    loop {
        let now = match bluez.read() {
            Ok(bluetooth) => Listed::Known(bluetooth),
            Err(why) => Listed::Absent(why),
        };
        // Looked for once the radio is on, and again if it went off.
        let on = matches!(now, Listed::Known(Bluetooth::Devices(_)));
        if on && !discovering {
            match bluez.discover(true) {
                Ok(()) => discovering = true,
                Err(why) => tracing::debug!(why, "no discovery"),
            }
        } else if !on {
            discovering = false;
        }
        if last.as_ref() != Some(&now) {
            if to_window.send(FromApi::Bluetooth(now.clone())).is_err() {
                break;
            }
            last = Some(now);
        }
        match asked.recv_timeout(DEVICES_EVERY) {
            Ok(action) => {
                let done = match &action {
                    BluetoothAction::Pair(device) => {
                        let tx = Mutex::new(to_window.clone());
                        bluez.pair(device, move |ask| {
                            if let Ok(tx) = tx.lock() {
                                let _ = tx.send(FromApi::Ask(ask));
                            }
                        })
                    }
                    BluetoothAction::Connect(device) => bluez.connect_device(device, true),
                    BluetoothAction::Disconnect(device) => bluez.connect_device(device, false),
                    BluetoothAction::Forget(device) => bluez.forget(device),
                };
                if let Err(why) = &done {
                    tracing::info!(why, ?action, "the device did not do what was asked");
                }
                if let Ok(bluetooth) = bluez.read() {
                    let now = Listed::Known(bluetooth);
                    let _ = to_window.send(FromApi::Bluetooth(now.clone()));
                    last = Some(now);
                }
                let _ = to_window.send(FromApi::BluetoothDone(done));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    if discovering && let Err(why) = bluez.discover(false) {
        tracing::debug!(why, "discovery not stopped");
    }
}

/// A page's worker: what the page asks goes to it; it says it ended.
struct Worker<T> {
    asked: Sender<T>,
    ended: Receiver<()>,
}

impl<T: Send + 'static> Worker<T> {
    fn start(
        name: &str,
        to_window: Sender<FromApi>,
        bus: Option<String>,
        work: fn(Option<String>, Sender<FromApi>, Receiver<T>),
    ) -> Option<Self> {
        let (asked, receive) = channel();
        let (end, ended) = channel();
        let spawned = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                work(bus, to_window, receive);
                let _ = end.send(());
            });
        match spawned {
            Ok(_) => Some(Self { asked, ended }),
            Err(err) => {
                tracing::warn!(%err, name, "the list will not be read");
                None
            }
        }
    }

    /// Lets it end, and waits a moment for it: discovery is stopped
    /// before the app goes.
    fn stop(self, wait: Duration) {
        drop(self.asked);
        let _ = self.ended.recv_timeout(wait);
    }
}

/// Reads the time zones' places, on a thread of its own.
fn read_places(zoneinfo: PathBuf, to_window: Sender<FromApi>) {
    let spawned = std::thread::Builder::new()
        .name("time zones".into())
        .spawn(move || {
            let places = hocus_platform::timedate::places(&zoneinfo);
            if places.is_empty() {
                tracing::info!(dir = %zoneinfo.display(), "no zone1970.tab: no map");
            }
            let _ = to_window.send(FromApi::Places(places));
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "the time zones will not be placed");
    }
}

/// The system bus timedated is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemBus {
    /// The machine's.
    Machine,
    /// A private one (tests).
    At(String),
    /// None: the zone is not read nor set here (tests without one).
    Off,
}

impl SystemBus {
    /// The address to connect to, the machine's with `Some(None)`.
    fn address(&self) -> Option<Option<String>> {
        match self {
            Self::Machine => Some(None),
            Self::At(address) => Some(Some(address.clone())),
            Self::Off => None,
        }
    }
}

/// Reads the system's zone, then follows it, on threads of their own.
fn follow_zone(bus: &SystemBus, to_window: Sender<FromApi>) {
    let Some(address) = bus.address() else {
        let _ = to_window.send(FromApi::Zone(SystemZone::Absent("no system bus".into())));
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("time zone".into())
        .spawn(move || match hocus_platform::timedate::read(&address) {
            Ok(zone) => {
                let _ = to_window.send(FromApi::Zone(SystemZone::Known(zone)));
                let tx = Mutex::new(to_window);
                let followed = hocus_platform::timedate::follow(address, false, move |zone| {
                    if let Ok(tx) = tx.lock() {
                        let _ = tx.send(FromApi::Zone(SystemZone::Known(zone)));
                    }
                });
                if let Err(err) = followed {
                    tracing::warn!(%err, "the time zone will not be followed");
                }
            }
            Err(why) => {
                tracing::info!(why, "no timedated");
                let _ = to_window.send(FromApi::Zone(SystemZone::Absent(why)));
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "the time zone will not be read");
    }
}

/// The app running on a thread of its own (tests).
#[derive(Debug)]
pub struct Running {
    shown: Arc<Mutex<Option<Shown>>>,
    thread: JoinHandle<anyhow::Result<()>>,
}

/// What the window's last frame showed.
#[derive(Debug, Clone)]
pub struct Shown {
    /// As screen readers read it.
    pub access: AccessNode,
    /// The configuration and screens it was drawn from.
    pub config: hocus_core::Config,
    pub outputs: Vec<OutputInfo>,
}

impl Running {
    /// What the window shows, as screen readers read it, after its last
    /// frame.
    pub fn access(&self) -> Option<AccessNode> {
        self.shown().map(|s| s.access)
    }

    /// The window's last frame.
    pub fn shown(&self) -> Option<Shown> {
        self.shown.lock().ok().and_then(|s| s.clone())
    }

    /// Whether the last frame was drawn from this configuration (as the
    /// control API gives it) and these screens.
    pub fn shows(&self, config: &str, outputs: &[OutputInfo]) -> bool {
        let Ok(config) = hocus_core::Config::parse(config) else {
            return false;
        };
        self.shown()
            .is_some_and(|s| s.config == config && s.outputs == outputs)
    }

    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }

    pub fn join(self) -> anyhow::Result<()> {
        self.thread
            .join()
            .map_err(|_| anyhow::anyhow!("the settings app panicked"))?
    }
}

pub fn spawn(options: Options) -> anyhow::Result<Running> {
    let shown = Arc::new(Mutex::new(None));
    let thread = std::thread::Builder::new().name("settings".into()).spawn({
        let shown = shown.clone();
        move || run(options, Some(shown))
    })?;
    Ok(Running { shown, thread })
}

/// What the control API thread tells the window.
enum FromApi {
    /// What is in effect: the configuration, the screens, the accent
    /// (which may come from the wallpaper) and the file.
    Context(Box<(Context, Option<String>, Option<PathBuf>)>),
    Open(Option<String>),
    Gone(String),
    /// The system's time zone, as timedated says.
    Zone(SystemZone),
    /// Whether timedated took the zone asked for.
    ZoneSet(Result<String, String>),
    /// The default applications and autostart entries, as read.
    Apps(crate::catalogue::AppsState),
    /// What AccountsService and fprintd say.
    Account(hocus_platform::account::AccountInfo),
    /// AccountsService refused a change, and why.
    AccountRefused(String),
    /// The time zones' places, for the map.
    Places(Vec<hocus_platform::timedate::Place>),
    /// The networks in reach, as NetworkManager says.
    Wifi(Listed<hocus_platform::wifi::Wifi>),
    /// What was asked of a network is done, or why not.
    WifiDone(Result<(), hocus_platform::wifi::JoinError>),
    /// The Bluetooth devices, as BlueZ says.
    Bluetooth(Listed<hocus_platform::bluetooth::Bluetooth>),
    BluetoothDone(Result<(), String>),
    /// What a pairing asks.
    Ask(hocus_platform::bluetooth::Ask),
    /// What the document search's indexer does.
    Search(hocus_api::SearchInfo),
}

fn api_thread(path: PathBuf, home: Option<PathBuf>, to_window: Sender<FromApi>) {
    let result = (|| -> anyhow::Result<()> {
        let mut requests = Api::connect(&path)?;
        let mut events = Api::connect(&path)?;
        events.request(Request::Subscribe)?;
        let fetch = |requests: &mut Api| -> anyhow::Result<()> {
            let Reply::Config(info) = requests.request(Request::Config)? else {
                anyhow::bail!("no configuration");
            };
            let Reply::Outputs(outputs) = requests.request(Request::Outputs)? else {
                anyhow::bail!("no outputs");
            };
            let Reply::Screens(screens) = requests.request(Request::Screens)? else {
                anyhow::bail!("no screens");
            };
            let updates = match requests.request(Request::Updates)? {
                Reply::Updates(updates) => updates,
                _ => Default::default(),
            };
            let calendars = match requests.request(Request::Calendars)? {
                Reply::Calendars(calendars) => calendars,
                _ => Default::default(),
            };
            // A compositor without quick settings says nothing of them.
            let hardware = match requests.request(Request::Hardware) {
                Ok(Reply::Hardware(hardware)) => hardware,
                _ => Default::default(),
            };
            let notifications = match requests.request(Request::Notifications) {
                Ok(Reply::Notifications(notifications)) => notifications,
                _ => Default::default(),
            };
            let ai = match requests.request(Request::Ai) {
                Ok(Reply::Ai(ai)) => ai,
                _ => Default::default(),
            };
            let agents_stopped = match requests.request(Request::AgentsJournal) {
                Ok(Reply::AgentsJournal(journal)) => journal.stopped,
                _ => false,
            };
            let search = match requests.request(Request::Search) {
                Ok(Reply::Search(search)) => search,
                _ => Default::default(),
            };
            let config = hocus_core::Config::parse(&info.toml)
                .map_err(|e| anyhow::anyhow!("the configuration in effect: {e}"))?;
            let ctx = Context {
                config,
                outputs,
                screens,
                updates,
                calendars,
                hardware,
                user: info.user.clone(),
                // timedated says it, not the compositor.
                zone: SystemZone::Unknown,
                notifications,
                // The settings read these themselves.
                apps: Default::default(),
                account: Default::default(),
                ai,
                agents_stopped,
                search,
                home: home.clone(),
            };
            let path = info.path.map(PathBuf::from);
            let sent = to_window.send(FromApi::Context(Box::new((ctx, info.accent, path))));
            sent.map_err(|_| anyhow::anyhow!("the window is gone"))
        };
        fetch(&mut requests)?;
        loop {
            match events.next_event()? {
                Event::ConfigApplied
                | Event::Updates { .. }
                | Event::Calendars
                | Event::Hardware
                | Event::Notifications
                | Event::Ai
                | Event::AgentsStopped { .. }
                | Event::OutputAdded { .. }
                | Event::OutputRemoved { .. }
                | Event::OutputChanged { .. } => {
                    fetch(&mut requests)?;
                }
                // Only what the indexer does: its counts and its last look.
                Event::Search { .. } => {
                    if let Ok(Reply::Search(info)) = requests.request(Request::Search)
                        && to_window.send(FromApi::Search(info)).is_err()
                    {
                        return Ok(());
                    }
                }
                Event::OpenSettings { page } => {
                    if to_window.send(FromApi::Open(page)).is_err() {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
    })();
    if let Err(err) = result {
        let _ = to_window.send(FromApi::Gone(format!("{err:#}")));
    }
}

/// While the Search page shows, what the indexer does, asked every few
/// seconds on a thread of its own: it counts documents and says when it
/// last looked, which no event tells. The thread ends with the window.
fn follow_search(api: PathBuf, shown: Weak<AtomicBool>, to_window: Sender<FromApi>) {
    let spawned = std::thread::Builder::new()
        .name("settings search".into())
        .spawn(move || {
            let mut client: Option<Api> = None;
            loop {
                std::thread::sleep(SEARCH_EVERY);
                let Some(shown) = shown.upgrade() else {
                    return;
                };
                if !shown.load(Ordering::Relaxed) {
                    continue;
                }
                drop(shown);
                if client.is_none() {
                    client = Api::connect(&api).ok();
                }
                let Some(api) = client.as_mut() else {
                    continue;
                };
                match api.request(Request::Search) {
                    Ok(Reply::Search(info)) => {
                        if to_window.send(FromApi::Search(info)).is_err() {
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(err) => {
                        tracing::debug!(%err, "the indexer's state is asked again");
                        client = None;
                    }
                }
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "the indexer's state will not follow");
    }
}

/// The theme the configuration asks for, as the compositor makes it.
pub fn theme(ctx: &Context, accent: Option<&str>) -> Theme {
    let a = &ctx.config.appearance;
    let mut theme = Theme::builtin(a.theme.parse().unwrap_or(ThemeName::Dark));
    if let Ok(named) = a.accent.parse::<Accent>() {
        theme = theme.with_accent(named);
    } else if a.accent == "wallpaper"
        && let Some([r, g, b]) = accent.and_then(hocus_core::config::parse_hex)
    {
        theme = theme.with_custom_accent(Color::rgb(r, g, b));
    }
    if a.high_contrast {
        theme = theme.with_high_contrast();
    }
    theme
        .with_text_size(a.text_size)
        .with_reduced_motion(a.reduced_motion)
}

pub fn run(options: Options, shown: Option<Arc<Mutex<Option<Shown>>>>) -> anyhow::Result<()> {
    let stream = std::os::unix::net::UnixStream::connect(&options.wayland)
        .with_context(|| format!("could not connect to {}", options.wayland.display()))?;
    let conn = Connection::from_socket(stream)?;
    let (globals, mut queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).context("no wl_compositor")?;
    let xdg_shell = XdgShell::bind(&globals, &qh).context("no xdg_wm_base")?;
    let shm = Shm::bind(&globals, &qh).context("no wl_shm")?;

    // To record a shortcut, the compositor's own are held back.
    let inhibit = globals.bind::<InhibitManager, _, _>(&qh, 1..=1, ()).ok();
    let surface = compositor.create_surface(&qh);
    // The compositor draws the corners, the shadow and the ring.
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
    window.set_app_id(options.app_id.clone());
    window.set_title(crate::words::SETTINGS.get(options.lang));
    window.set_min_size(Some((720, 480)));
    window.commit();

    let (to_window, from_api) = channel();
    follow_zone(&options.system_bus, to_window.clone());
    let zone_news = to_window.clone();
    std::thread::Builder::new()
        .name("settings api".into())
        .spawn({
            let path = options.api.clone();
            let home = options.home.clone();
            move || api_thread(path, home, to_window)
        })?;
    // The first frame waits for what is in effect.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut early_zone = None;
    let (mut early_apps, mut early_account) = (None, None);
    apps_thread(options.xdg.clone(), zone_news.clone(), |_| {});
    read_places(options.zoneinfo.clone(), zone_news.clone());
    let mut early_places = None;
    let (ctx, accent, file) = loop {
        match from_api.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(FromApi::Context(what)) => break *what,
            Ok(FromApi::Zone(zone)) => early_zone = Some(zone),
            Ok(FromApi::ZoneSet(_) | FromApi::AccountRefused(_) | FromApi::Search(_)) => {}
            Ok(FromApi::Apps(apps)) => early_apps = Some(apps),
            Ok(FromApi::Account(account)) => early_account = Some(account),
            Ok(FromApi::Places(places)) => early_places = Some(places),
            Ok(FromApi::Gone(err)) => anyhow::bail!("the compositor did not answer: {err}"),
            // No page asks yet.
            Ok(
                FromApi::Open(_)
                | FromApi::Wifi(_)
                | FromApi::WifiDone(_)
                | FromApi::Bluetooth(_)
                | FromApi::BluetoothDone(_)
                | FromApi::Ask(_),
            ) => {}
            Err(_) => anyhow::bail!("the compositor did not answer in time"),
        }
    };
    let theme = theme(&ctx, accent.as_deref());
    let mut view = SettingsView::new(
        theme,
        options.lang,
        Size::new(SIZE.0 as f32, SIZE.1 as f32),
        1.0,
        ctx,
    );
    if let Some(zone) = early_zone {
        view.set_zone(zone);
    }
    if let Some(apps) = early_apps {
        view.set_apps(apps);
    }
    if let Some(account) = early_account {
        view.set_account(account);
    }
    if let Some(places) = early_places {
        view.set_places(places);
    }
    // The account is the compositor's user's.
    let user = view.context().user.clone();
    account_thread(&options.system_bus, user, zone_news.clone(), |_, _| Ok(()));
    view.open(options.page);
    // The language the file chose, once it is read.
    window.set_title(crate::words::SETTINGS.get(view.lang()));

    let search_shown = Arc::new(AtomicBool::new(false));
    follow_search(
        options.api.clone(),
        Arc::downgrade(&search_shown),
        zone_news.clone(),
    );

    let pool = SlotPool::new((SIZE.0 * SIZE.1 * 4) as usize, &shm)?;
    let mut app = App {
        registry: RegistryState::new(&globals),
        seat: SeatState::new(&globals, &qh),
        outputs: OutputState::new(&globals, &qh),
        shm,
        pool,
        window,
        view,
        accent,
        file,
        size: None,
        scale: 1,
        configured: false,
        frame_pending: false,
        needs_draw: true,
        committed: None,
        keyboard: None,
        pointer: None,
        seat_handle: None,
        modifiers: Modifiers::default(),
        inhibit,
        inhibitor: None,
        closed: false,
        started: Instant::now(),
        shown,
        from_api,
        api: options.api.clone(),
        system_bus: options.system_bus.clone(),
        xdg: options.xdg.clone(),
        zone_news,
        network: None,
        bluetooth: None,
        answer: None,
        search_shown,
    };
    while !app.closed {
        app.take_api_news();
        app.follow_page();
        app.search_shown
            .store(app.view.shows_search(), Ordering::Relaxed);
        if app.configured && app.needs_draw && !app.frame_pending {
            app.draw(&qh)?;
        }
        queue.flush()?;
        queue.dispatch_pending(&mut app)?;
        if let Some(guard) = queue.prepare_read() {
            let fd = guard.connection_fd();
            let mut fds = [rustix::event::PollFd::new(
                &fd,
                rustix::event::PollFlags::IN,
            )];
            let wait = if app.needs_draw {
                8_000_000
            } else {
                30_000_000
            };
            let timeout = rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: wait,
            };
            if rustix::event::poll(&mut fds, Some(&timeout))? > 0 {
                guard.read()?;
            }
        }
    }
    // Discovery stops with the window.
    app.answer = None;
    if let Some(worker) = app.bluetooth.take() {
        worker.stop(Duration::from_secs(2));
    }
    Ok(())
}

struct App {
    registry: RegistryState,
    seat: SeatState,
    outputs: OutputState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    view: SettingsView,
    accent: Option<String>,
    file: Option<PathBuf>,
    /// The size the compositor gave, logical pixels.
    size: Option<(u32, u32)>,
    scale: i32,
    configured: bool,
    frame_pending: bool,
    needs_draw: bool,
    /// The size in pixels of the last buffer given: one of the same size
    /// differs from it only where the view was drawn again.
    committed: Option<(u32, u32)>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    seat_handle: Option<wl_seat::WlSeat>,
    modifiers: Modifiers,
    inhibit: Option<InhibitManager>,
    /// Held while keys are recorded.
    inhibitor: Option<Inhibitor>,
    closed: bool,
    started: Instant,
    shown: Option<Arc<Mutex<Option<Shown>>>>,
    from_api: Receiver<FromApi>,
    /// The compositor's control socket, for one-off requests.
    api: PathBuf,
    system_bus: SystemBus,
    xdg: Xdg,
    /// Where the answers of timedated, AccountsService and the XDG files'
    /// threads come back.
    zone_news: Sender<FromApi>,
    /// The networks' worker, while the Network page shows.
    network: Option<Worker<WifiAction>>,
    /// The devices' worker, while the Bluetooth page shows.
    bluetooth: Option<Worker<BluetoothAction>>,
    /// The answer a pairing waits for.
    answer: Option<hocus_platform::bluetooth::Answer>,
    /// The Search page shows: the indexer's state is followed.
    search_shown: Arc<AtomicBool>,
}

impl App {
    /// The lists' workers run while their page shows.
    fn follow_page(&mut self) {
        let page = match self.view.screen() {
            Screen::Page(page) => Some(page),
            Screen::Home => None,
        };
        let bus = self.system_bus.address();
        let wanted = page == Some(Page::Network);
        if wanted && self.network.is_none() {
            match bus.clone() {
                Some(bus) => {
                    let tx = self.zone_news.clone();
                    self.network = Worker::start("networks", tx, bus, network_worker);
                }
                None if self.view.wifi_unknown() => {
                    self.view.set_wifi(Listed::Absent("no system bus".into()));
                }
                None => {}
            }
        } else if !wanted && let Some(worker) = self.network.take() {
            // It ends after what it is doing.
            drop(worker);
        }
        let wanted = page == Some(Page::Bluetooth);
        if wanted && self.bluetooth.is_none() {
            match bus {
                Some(bus) => {
                    let tx = self.zone_news.clone();
                    self.bluetooth = Worker::start("bluetooth", tx, bus, bluetooth_worker);
                }
                None if self.view.bluetooth_unknown() => {
                    self.view
                        .set_bluetooth(Listed::Absent("no system bus".into()));
                }
                None => {}
            }
        } else if !wanted && let Some(worker) = self.bluetooth.take() {
            // A pairing waiting for its answer is refused.
            self.answer = None;
            worker.stop(Duration::ZERO);
        }
        if self.view.is_animating() {
            self.needs_draw = true;
        }
    }

    fn take_api_news(&mut self) {
        while let Ok(message) = self.from_api.try_recv() {
            match message {
                FromApi::Context(what) => {
                    let (ctx, accent, file) = *what;
                    self.view.set_theme(theme(&ctx, accent.as_deref()));
                    let before = self.view.lang();
                    self.view.set_context(ctx);
                    // Its title in the language chosen, too.
                    if self.view.lang() != before {
                        self.window
                            .set_title(crate::words::SETTINGS.get(self.view.lang()));
                    }
                    self.accent = accent;
                    self.file = file;
                }
                FromApi::Open(page) => {
                    self.view.open(page.as_deref().and_then(Page::from_slug));
                }
                FromApi::Gone(err) => {
                    tracing::warn!(err, "the compositor went away");
                    self.closed = true;
                }
                FromApi::Zone(zone) => self.view.set_zone(zone),
                FromApi::ZoneSet(result) => self.view.zone_set(result),
                FromApi::Apps(apps) => self.view.set_apps(apps),
                FromApi::Account(account) => self.view.set_account(account),
                FromApi::AccountRefused(why) => self.view.account_refused(why),
                FromApi::Places(places) => self.view.set_places(places),
                FromApi::Wifi(wifi) => self.view.set_wifi(wifi),
                FromApi::WifiDone(result) => self.view.wifi_done(result),
                FromApi::Bluetooth(bluetooth) => self.view.set_bluetooth(bluetooth),
                FromApi::BluetoothDone(result) => self.view.bluetooth_done(result),
                FromApi::Ask(ask) => {
                    use hocus_platform::bluetooth::Ask;
                    let pairing = match ask {
                        Ask::Confirm { passkey, answer } => {
                            self.answer = Some(answer);
                            Some(Pairing::Confirm(passkey))
                        }
                        Ask::Show { code } => Some(Pairing::Show(code)),
                        Ask::Over => {
                            self.answer = None;
                            None
                        }
                    };
                    self.view.set_pairing(pairing);
                }
                FromApi::Search(info) => self.view.set_search(info),
            }
            self.needs_draw = true;
        }
    }

    fn outcomes(&mut self, qh: &QueueHandle<Self>, outcomes: Vec<Outcome>) {
        for outcome in outcomes {
            match outcome {
                Outcome::Close => self.closed = true,
                Outcome::Inhibit(true) => {
                    if self.inhibitor.is_none()
                        && let (Some(manager), Some(seat)) = (&self.inhibit, &self.seat_handle)
                    {
                        self.inhibitor =
                            Some(manager.inhibit_shortcuts(self.window.wl_surface(), seat, qh, ()));
                    }
                }
                Outcome::Inhibit(false) => {
                    if let Some(inhibitor) = self.inhibitor.take() {
                        inhibitor.destroy();
                    }
                }
                // Off the window's thread: the compositor answers in its time.
                Outcome::InstallUpdates => {
                    let api = self.api.clone();
                    let spawned =
                        std::thread::Builder::new()
                            .name("install".into())
                            .spawn(move || {
                                if let Err(err) = Api::connect(&api)
                                    .and_then(|mut c| c.request(Request::InstallUpdates).map(drop))
                                {
                                    tracing::warn!(%err, "could not install the updates");
                                }
                            });
                    if let Err(err) = spawned {
                        tracing::warn!(%err, "could not install the updates");
                    }
                }
                // The password goes straight to the compositor's keyring,
                // never into the configuration.
                Outcome::CalendarPassword { account, password } => {
                    let api = self.api.clone();
                    let spawned =
                        std::thread::Builder::new()
                            .name("calendar".into())
                            .spawn(move || {
                                let request = Request::CalendarSecret {
                                    account,
                                    secret: password,
                                };
                                if let Err(err) = Api::connect(&api)
                                    .and_then(|mut c| c.request(request).map(drop))
                                {
                                    tracing::warn!(%err, "could not keep the calendar password");
                                }
                            });
                    if let Err(err) = spawned {
                        tracing::warn!(%err, "could not keep the calendar password");
                    }
                }
                // GNOME's settings start from the compositor, with the
                // session's environment.
                Outcome::OpenOnlineAccounts => {
                    let api = self.api.clone();
                    let spawned = std::thread::Builder::new()
                        .name("online accounts".into())
                        .spawn(move || {
                            if let Err(err) = Api::connect(&api)
                                .and_then(|mut c| c.request(Request::OpenOnlineAccounts).map(drop))
                            {
                                tracing::warn!(%err, "could not open GNOME Online Accounts");
                            }
                        });
                    if let Err(err) = spawned {
                        tracing::warn!(%err, "could not open GNOME Online Accounts");
                    }
                }
                // Off the window's thread: polkit may ask for a password.
                Outcome::SetTimeZone(zone) => {
                    let tx = self.zone_news.clone();
                    let Some(address) = self.system_bus.address() else {
                        let _ = tx.send(FromApi::ZoneSet(Err("no system bus".into())));
                        continue;
                    };
                    let spawned = std::thread::Builder::new()
                        .name("set time zone".into())
                        .spawn(move || {
                            let result = hocus_platform::timedate::set(&address, &zone)
                                .map(|()| zone)
                                .inspect_err(|err| tracing::warn!(err, "the time zone stays"));
                            let _ = tx.send(FromApi::ZoneSet(result));
                        });
                    if let Err(err) = spawned {
                        tracing::warn!(%err, "could not set the time zone");
                    }
                }
                // Off the window's thread: the compositor hands it to the
                // service, which answers with the new state in its time.
                Outcome::Hardware(change) => {
                    let api = self.api.clone();
                    let spawned =
                        std::thread::Builder::new()
                            .name("hardware".into())
                            .spawn(move || {
                                let request = Request::SetHardware { change };
                                if let Err(err) = Api::connect(&api)
                                    .and_then(|mut c| c.request(request).map(drop))
                                {
                                    tracing::warn!(%err, "could not change the setting");
                                }
                            });
                    if let Err(err) = spawned {
                        tracing::warn!(%err, "could not change the setting");
                    }
                }
                // The key goes straight to the compositor's keyring, never
                // into the configuration; it says which providers have one.
                Outcome::AiKey { provider, key } => {
                    let api = self.api.clone();
                    let spawned =
                        std::thread::Builder::new()
                            .name("ai key".into())
                            .spawn(move || {
                                let request = Request::AiSecret {
                                    provider,
                                    secret: key,
                                };
                                if let Err(err) = Api::connect(&api)
                                    .and_then(|mut c| c.request(request).map(drop))
                                {
                                    tracing::warn!(%err, "could not keep the AI key");
                                }
                            });
                    if let Err(err) = spawned {
                        tracing::warn!(%err, "could not keep the AI key");
                    }
                }
                Outcome::StopAgents(stopped) => {
                    let api = self.api.clone();
                    let spawned = std::thread::Builder::new()
                        .name("stop agents".into())
                        .spawn(move || {
                            let request = Request::SetAgentsStopped { stopped };
                            if let Err(err) =
                                Api::connect(&api).and_then(|mut c| c.request(request).map(drop))
                            {
                                tracing::warn!(%err, "could not stop or allow the agents");
                            }
                        });
                    if let Err(err) = spawned {
                        tracing::warn!(%err, "could not stop or allow the agents");
                    }
                }
                // Off the window's thread; what the indexer does then
                // shows at once.
                Outcome::SearchAgain => {
                    let api = self.api.clone();
                    let tx = self.zone_news.clone();
                    let spawned = std::thread::Builder::new()
                        .name("search again".into())
                        .spawn(move || {
                            let asked = Api::connect(&api).and_then(|mut c| {
                                c.request(Request::SearchAgain)?;
                                c.request(Request::Search)
                            });
                            match asked {
                                Ok(Reply::Search(info)) => {
                                    let _ = tx.send(FromApi::Search(info));
                                }
                                Ok(_) => {}
                                Err(err) => {
                                    tracing::warn!(%err, "the folders were not looked at again");
                                }
                            }
                        });
                    if let Err(err) = spawned {
                        tracing::warn!(%err, "the folders were not looked at again");
                    }
                }
                Outcome::DoNotDisturb(on) => {
                    let api = self.api.clone();
                    let spawned = std::thread::Builder::new()
                        .name("do not disturb".into())
                        .spawn(move || {
                            let request = Request::SetDoNotDisturb { on };
                            if let Err(err) =
                                Api::connect(&api).and_then(|mut c| c.request(request).map(drop))
                            {
                                tracing::warn!(%err, "could not change do not disturb");
                            }
                        });
                    if let Err(err) = spawned {
                        tracing::warn!(%err, "could not change do not disturb");
                    }
                }
                // Files of the user's own: off the window's thread.
                Outcome::SetDefaultApp { kind, app } => {
                    apps_thread(self.xdg.clone(), self.zone_news.clone(), move |xdg| {
                        if let Err(err) =
                            hocus_platform::mime::set_default(&xdg.dirs(), &kind, &app)
                        {
                            tracing::warn!(%err, kind, app, "the default application stays");
                        }
                    });
                }
                Outcome::SetAutostart { id, on } => {
                    apps_thread(self.xdg.clone(), self.zone_news.clone(), move |xdg| {
                        let dirs = xdg.dirs().config;
                        if let Err(err) = hocus_platform::apps::set_autostart(&dirs, &id, on) {
                            tracing::warn!(%err, id, "the autostart entry stays");
                        }
                    });
                }
                // Off the window's thread: polkit may ask.
                Outcome::SetRealName(name) => {
                    let user = self.view.context().user.clone();
                    account_thread(
                        &self.system_bus,
                        user,
                        self.zone_news.clone(),
                        move |bus, user| hocus_platform::account::set_real_name(bus, user, &name),
                    );
                }
                Outcome::SetPicture(file) => {
                    let user = self.view.context().user.clone();
                    account_thread(
                        &self.system_bus,
                        user,
                        self.zone_news.clone(),
                        move |bus, user| {
                            hocus_platform::account::set_icon_file(
                                bus,
                                user,
                                std::path::Path::new(&file),
                            )
                        },
                    );
                }
                Outcome::Identify => {
                    let api = self.api.clone();
                    let spawned =
                        std::thread::Builder::new()
                            .name("identify".into())
                            .spawn(move || {
                                if let Err(err) = Api::connect(&api)
                                    .and_then(|mut c| c.request(Request::IdentifyOutputs).map(drop))
                                {
                                    tracing::warn!(%err, "could not identify the screens");
                                }
                            });
                    if let Err(err) = spawned {
                        tracing::warn!(%err, "could not identify the screens");
                    }
                }
                // To the page's worker, which does it in turn.
                Outcome::Wifi(action) => {
                    let sent = self.network.as_ref().map(|w| w.asked.send(action));
                    if !matches!(sent, Some(Ok(()))) {
                        let refused =
                            hocus_platform::wifi::JoinError::Refused("no NetworkManager".into());
                        self.view.wifi_done(Err(refused));
                    }
                }
                Outcome::Bluetooth(action) => {
                    let sent = self.bluetooth.as_ref().map(|w| w.asked.send(action));
                    if !matches!(sent, Some(Ok(()))) {
                        self.view.bluetooth_done(Err("no BlueZ".into()));
                    }
                }
                Outcome::PairingAnswer(yes) => {
                    if let Some(answer) = self.answer.take() {
                        answer.give(yes);
                    }
                }
                Outcome::Change(change) => {
                    let Some(file) = &self.file else {
                        tracing::warn!("no configuration file to change");
                        continue;
                    };
                    if let Err(err) = edit::write(file, &change) {
                        tracing::warn!(%err, key = change.key.as_str(), "could not change the setting");
                    }
                }
            }
        }
        self.needs_draw = true;
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) -> anyhow::Result<()> {
        let (w, h) = self.size.unwrap_or(SIZE);
        let scale = self.scale.max(1);
        self.view
            .resize(Size::new(w as f32, h as f32), scale as f32);
        let now = self.started.elapsed();
        let damage = self.view.draw(now);
        let (pw, ph) = self.view.pixel_size();
        let (buffer, canvas) = self.pool.create_buffer(
            pw as i32,
            ph as i32,
            pw as i32 * 4,
            wl_shm::Format::Argb8888,
        )?;
        // Premultiplied RGBA to ARGB8888, little endian.
        for (out, px) in canvas
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(self.view.pixels().as_chunks::<4>().0)
        {
            *out = [px[2], px[1], px[0], px[3]];
        }
        let surface = self.window.wl_surface();
        surface.set_buffer_scale(scale);
        // The compositor is told only what was drawn again: a pointer
        // moving over the window no longer has it draw the whole window
        // again (#88). A new size is new everywhere.
        if self.committed == Some((pw, ph)) {
            for r in &damage {
                surface.damage_buffer(r.x as i32, r.y as i32, r.w as i32, r.h as i32);
            }
        } else {
            surface.damage_buffer(0, 0, pw as i32, ph as i32);
            self.committed = Some((pw, ph));
        }
        buffer.attach_to(surface)?;
        self.needs_draw = self.view.is_animating();
        if self.needs_draw {
            surface.frame(qh, surface.clone());
            self.frame_pending = true;
        }
        self.window.commit();
        if let Some(shown) = &self.shown
            && let Ok(mut slot) = shown.lock()
        {
            let ctx = self.view.context();
            *slot = Some(Shown {
                access: self.view.access(),
                config: ctx.config.clone(),
                outputs: ctx.outputs.clone(),
            });
        }
        Ok(())
    }
}

/// A key pressed with modifiers, as shortcuts name it: digits of the top
/// row by place, other keys by their unshifted name; `None` for a modifier
/// alone.
fn combo_of(event: &KeyEvent, modifiers: &Modifiers) -> Option<hocus_core::shortcuts::Combo> {
    use hocus_core::shortcuts::{Combo, Key as K, Modifiers as M};
    const KEY_1: u32 = 2;
    const KEY_0: u32 = 11;
    const KEY_GRAVE: u32 = 41;
    let code = event.raw_code;
    let key = if (KEY_1..=KEY_0).contains(&code) {
        K::Digit(((code - KEY_1 + 1) % 10) as u8)
    } else if code == KEY_GRAVE {
        K::Named("grave".into())
    } else {
        let name = event
            .keysym
            .name()?
            .trim_start_matches("XK_")
            .to_ascii_lowercase();
        let modifier = [
            "shift",
            "control",
            "alt",
            "super",
            "meta",
            "hyper",
            "iso_level3",
            "caps_lock",
        ]
        .iter()
        .any(|m| name.starts_with(m));
        if modifier {
            return None;
        }
        K::Named(name)
    };
    Some(Combo {
        modifiers: M {
            logo: modifiers.logo,
            ctrl: modifiers.ctrl,
            alt: modifiers.alt,
            shift: modifiers.shift,
        },
        key,
    })
}

impl App {
    fn any_modifier(&self) -> bool {
        let m = &self.modifiers;
        m.logo || m.ctrl || m.alt || m.shift
    }
}

impl Dispatch<InhibitManager, ()> for App {
    fn event(
        _: &mut Self,
        _: &InhibitManager,
        _: <InhibitManager as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<Inhibitor, ()> for App {
    fn event(
        _: &mut Self,
        _: &Inhibitor,
        event: zwp_keyboard_shortcuts_inhibitor_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        tracing::debug!(?event, "shortcuts held back");
    }
}

fn key_of(keysym: Keysym) -> Option<Key> {
    Some(match keysym {
        Keysym::Escape => Key::Escape,
        Keysym::Return | Keysym::KP_Enter => Key::Enter,
        Keysym::Tab => Key::Tab,
        Keysym::ISO_Left_Tab => Key::BackTab,
        Keysym::Up => Key::Up,
        Keysym::Down => Key::Down,
        Keysym::Left => Key::Left,
        Keysym::Right => Key::Right,
        Keysym::Home => Key::Home,
        Keysym::End => Key::End,
        Keysym::BackSpace => Key::Backspace,
        Keysym::Delete => Key::Delete,
        _ => return None,
    })
}

impl WindowHandler for App {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.closed = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        let (w, h) = configure.new_size;
        self.size = match (w, h) {
            (Some(w), Some(h)) => Some((w.get(), h.get())),
            // Its own size, within what the screen leaves.
            _ => self.size.or_else(|| {
                let (bw, bh) = configure.suggested_bounds.unwrap_or(SIZE);
                let fit = |want: u32, room: u32| {
                    if room == 0 {
                        want
                    } else {
                        want.min(room.saturating_sub(48))
                    }
                };
                Some((fit(SIZE.0, bw).max(720), fit(SIZE.1, bh).max(480)))
            }),
        };
        self.configured = true;
        self.needs_draw = true;
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if factor != self.scale {
            self.scale = factor;
            self.needs_draw = true;
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        self.seat_handle = Some(seat.clone());
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            match self.seat.get_keyboard(qh, &seat, None) {
                Ok(k) => self.keyboard = Some(k),
                Err(err) => tracing::warn!(%err, "no keyboard"),
            }
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat.get_pointer(qh, &seat) {
                Ok(p) => self.pointer = Some(p),
                Err(err) => tracing::warn!(%err, "no pointer"),
            }
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard
            && let Some(k) = self.keyboard.take()
        {
            k.release();
        }
        if capability == Capability::Pointer
            && let Some(p) = self.pointer.take()
        {
            p.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if &event.surface != self.window.wl_surface() {
                continue;
            }
            let at = Point::new(event.position.0 as f32, event.position.1 as f32);
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.view.pointer_move(at);
                }
                PointerEventKind::Leave { .. } => self.view.pointer_leave(),
                PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                    self.view.pointer_move(at);
                    let out = self.view.pointer_button(true);
                    self.outcomes(qh, out);
                }
                PointerEventKind::Release { button, .. } if button == BTN_LEFT => {
                    let out = self.view.pointer_button(false);
                    self.outcomes(qh, out);
                }
                PointerEventKind::Axis { vertical, .. } => {
                    self.view.scroll(vertical.absolute as f32);
                }
                _ => {}
            }
            self.needs_draw = true;
        }
    }
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        // Recording a shortcut: keys are a combination, Escape alone cancels.
        if self.view.recording() && !(event.keysym == Keysym::Escape && !self.any_modifier()) {
            if let Some(combo) = combo_of(&event, &self.modifiers) {
                let out = self.view.record(combo);
                self.outcomes(qh, out);
            }
            return;
        }
        let out = match key_of(event.keysym) {
            Some(key) => self.view.key(key),
            None => match event.utf8.as_deref() {
                Some(text) if text.chars().all(|c| !c.is_control()) => self.view.text(text),
                _ => Vec::new(),
            },
        };
        self.outcomes(qh, out);
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: u32,
    ) {
        self.modifiers = modifiers;
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_keyboard!(App);
delegate_pointer!(App);
delegate_xdg_shell!(App);
delegate_xdg_window!(App);
delegate_registry!(App);
