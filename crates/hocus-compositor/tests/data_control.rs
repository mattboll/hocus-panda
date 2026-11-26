//! The phone's clipboard to the desktop (#957, lot 2; docs/design/bar.md,
//! "The phone"): `ext-data-control-v1` and `zwlr_data_control_manager_v1`,
//! offered to KDE Connect's daemon alone. One scenario per criterion of
//! the approved spec (28 to 31), then the clipboard's history's rules for
//! what the phone copies (5.6).
//!
//! KDE Connect's daemon is stood in for by this test program, run again as
//! a process of its own ([`stand_in`]) through a hard link named
//! `kdeconnectd` in a folder of the test's: the compositor knows a client
//! by its executable (`/proc/PID/exe`), and the configuration allows that
//! one. The test's own clients, this process's, are any other program.

use crate::clipboard::{Bed, history};
use crate::common;

use std::{
    io::{BufRead, Read, Write},
    os::fd::AsFd,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::{Mutex, mpsc},
    time::Duration,
};

use anyhow::Context;
use common::{Clip, Command, Event, Shows, WAIT, wait};
use hocus_compositor::{Config, DataControlAllowed, Headless, HeadlessConfig, RendererChoice};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, event_created_child,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_registry, wl_seat::WlSeat},
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1},
    zwlr_data_control_source_v1::{self, ZwlrDataControlSourceV1},
};

const EXT: &str = "ext_data_control_manager_v1";
const WLR: &str = "zwlr_data_control_manager_v1";
const TEXT_TYPE: &str = "text/plain;charset=utf-8";
const SECRET_HINT: &str = "x-kde-passwordManagerHint";
/// The stand-in's application, its desktop entry's id.
const APP: &str = "test.kdeconnect";

// What the stand-in is told, in its environment.
const SOCKET: &str = "HOCUS_TEST_DC_SOCKET";
/// `ext` or `wlr`: the protocol it uses.
const PROTOCOL: &str = "HOCUS_TEST_DC_PROTOCOL";
/// A text it sets the clipboard to as it starts.
const SET: &str = "HOCUS_TEST_DC_SET";
/// Its text marked secret, as password managers mark theirs.
const SECRET: &str = "HOCUS_TEST_DC_SECRET";

/// The stand-in's line prefix on its standard output.
const SAYS: &str = "stand-in: ";

/// KDE Connect's daemon, standing in: run by [`StandIn::start`] as a
/// process of its own; nothing without its socket.
#[test]
fn stand_in() -> anyhow::Result<()> {
    let Some(socket) = std::env::var_os(SOCKET) else {
        return Ok(());
    };
    run_stand_in(Path::new(&socket))
}

/// The test program, hard linked as `kdeconnectd` in a folder of its own
/// beside it (the same file system), and the rule allowing it.
struct Linked {
    dir: tempfile::TempDir,
    program: PathBuf,
}

impl Linked {
    fn new() -> anyhow::Result<Self> {
        let exe = std::env::current_exe()?;
        let beside = exe.parent().context("the test's program has no folder")?;
        let dir = tempfile::Builder::new()
            .prefix("data-control-")
            .tempdir_in(beside)?;
        let program = dir.path().join("kdeconnectd");
        std::fs::hard_link(&exe, &program)
            .with_context(|| format!("could not link {} as kdeconnectd", exe.display()))?;
        Ok(Self { dir, program })
    }

    fn allowed(&self) -> DataControlAllowed {
        DataControlAllowed {
            name: "kdeconnectd".to_owned(),
            under: self.dir.path().to_owned(),
            app: APP.to_owned(),
        }
    }
}

/// The stand-in running, stopped by its own handle as the test ends.
struct StandIn {
    child: std::process::Child,
    lines: mpsc::Receiver<String>,
    /// What it said so far.
    said: Vec<String>,
    /// The first line [`StandIn::until`] has not matched past.
    next: usize,
}

impl Drop for StandIn {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl StandIn {
    /// Starts `linked` on `hp`'s socket with `protocol`, setting the
    /// clipboard to `set` (marked secret with `secret`), and waits for its
    /// globals.
    fn start(
        hp: &mut Headless,
        linked: &Linked,
        protocol: &str,
        set: Option<&str>,
        secret: bool,
    ) -> anyhow::Result<Self> {
        let mut command = std::process::Command::new(&linked.program);
        command
            .args(["--exact", "data_control::stand_in", "--nocapture"])
            .env(SOCKET, hp.socket())
            .env(PROTOCOL, protocol)
            .stdout(std::process::Stdio::piped());
        if let Some(set) = set {
            command.env(SET, set);
        }
        if secret {
            command.env(SECRET, "1");
        }
        let mut child = command.spawn()?;
        let stdout = child.stdout.take().context("no output")?;
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
            {
                if let Some(said) = line.strip_prefix(SAYS)
                    && sender.send(said.to_owned()).is_err()
                {
                    break;
                }
            }
        });
        let mut stand_in = Self {
            child,
            lines,
            said: Vec::new(),
            next: 0,
        };
        stand_in.until(hp, |l| l.starts_with("globals "))?;
        Ok(stand_in)
    }

    /// Runs the compositor until the stand-in says a line `matches`: that
    /// line.
    fn until(
        &mut self,
        hp: &mut Headless,
        matches: impl Fn(&str) -> bool,
    ) -> anyhow::Result<String> {
        let from = self.next;
        let lines = &self.lines;
        let said = &mut self.said;
        let mut at = None;
        hp.dispatch_until(WAIT, |_| {
            said.extend(lines.try_iter());
            at = said
                .get(from..)
                .and_then(|new| new.iter().position(|l| matches(l)))
                .map(|i| from + i);
            at.is_some()
        })?;
        let at =
            at.with_context(|| format!("the stand-in never said it; it said {:?}", self.said))?;
        self.next = at + 1;
        Ok(self.said[at].clone())
    }

    fn globals(&self) -> Vec<String> {
        self.said
            .iter()
            .find_map(|l| l.strip_prefix("globals "))
            .map(|g| g.split(' ').map(str::to_owned).collect())
            .unwrap_or_default()
    }
}

/// A window of the test's reads the clipboard until it reads `want`.
fn pastes(hp: &mut Headless, client: &common::ClientHandle, want: &str) -> anyhow::Result<()> {
    let mut last = String::new();
    let deadline = std::time::Instant::now() + WAIT;
    while std::time::Instant::now() < deadline {
        client.commands.send(Command::ReadClipboard)?;
        let mut read = None;
        hp.dispatch_until(Duration::from_secs(5), |_| {
            read = client.events.try_iter().find_map(|e| match e {
                Event::Pasted(text) => Some(text),
                _ => None,
            });
            read.is_some()
        })?;
        last = read.unwrap_or_default();
        if last == want {
            return Ok(());
        }
        wait(hp, Duration::from_millis(50))?;
    }
    anyhow::bail!("the window read {last:?}, not {want:?}")
}

/// The compositor alone, the stand-in allowed.
fn compositor(linked: &Linked) -> anyhow::Result<Headless> {
    let mut config = Config::default();
    config.keyboard.layouts = vec!["us".to_owned()];
    config.data_control = vec![linked.allowed()];
    Headless::start_with(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )
}

/// Only KDE Connect sees it (scenario 28): given the allowed client and another,
/// the first's registry has both managers, the other's neither.
#[test]
fn only_kde_connect_sees_the_data_control_globals() -> anyhow::Result<()> {
    let linked = Linked::new()?;
    let mut hp = compositor(&linked)?;
    let stand_in = StandIn::start(&mut hp, &linked, "ext", None, false)?;
    let globals = stand_in.globals();
    assert!(globals.iter().any(|g| g == EXT), "{globals:?}");
    assert!(globals.iter().any(|g| g == WLR), "{globals:?}");
    let other = common::client(hp.socket(), None)?;
    let mut names = None;
    hp.dispatch_until(WAIT, |_| {
        names = other.events.try_iter().find_map(|e| match e {
            Event::Globals(names) => Some(names),
            _ => None,
        });
        names.is_some()
    })?;
    let names = names.context("the other client never listed its globals")?;
    assert!(names.iter().any(|n| n.starts_with("wl_seat ")), "{names:?}");
    assert!(
        !names
            .iter()
            .any(|n| n.starts_with(EXT) || n.starts_with(WLR)),
        "another client sees data control: {names:?}"
    );
    Ok(())
}

/// From the phone (scenario 29): the allowed client sets "4821" through
/// `ext-data-control-v1`; another client's paste reads it, and the
/// shell's history lists it, KDE Connect's.
#[test]
fn a_copy_from_the_phone_is_pasted_and_in_the_history() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let linked = Linked::new()?;
    phone_entry(dir.path())?;
    let mut bed = Bed::start_with(dir.path(), |c| c.data_control = vec![linked.allowed()])?
        .context("no bed")?;
    let editor = bed.writing_window("test.editor", (640, 420))?;
    bed.focus(&editor)?;
    let mark = bed.mark();
    let _stand_in = StandIn::start(&mut bed.hp, &linked, "ext", Some("4821"), false)?;
    bed.logged_after(mark, "a copy for the history")?;
    pastes(&mut bed.hp, &editor.client, "4821")?;
    let (_, line) = bed.super_v()?;
    assert_eq!(
        history(&line),
        "RÉCENTS: [text] «4821» KDE Connect · maintenant (selected)"
    );
    Ok(())
}

/// To the phone, by KDE Connect (scenario 30): a client's copy is offered to the
/// allowed client, which reads it; a copy marked secret is offered too,
/// with its mark among its types (KDE Connect's own setting decides).
#[test]
fn a_copy_is_offered_to_kde_connect_a_secret_with_its_mark() -> anyhow::Result<()> {
    let linked = Linked::new()?;
    let mut hp = compositor(&linked)?;
    let mut stand_in = StandIn::start(&mut hp, &linked, "ext", None, false)?;
    let (_, editor) = common::open_showing(
        &mut hp,
        "test.editor",
        Shows::Typing {
            app_id: "test.editor",
            size: (640, 420),
        },
    )?;
    holds_keyboard(&mut hp, "test.editor")?;
    copy(&mut hp, &editor, &[TEXT_TYPE, "text/plain"], "bonjour")?;
    let offered = stand_in.until(&mut hp, |l| l.starts_with("offered "))?;
    assert_eq!(offered, format!("offered {TEXT_TYPE} text/plain"));
    stand_in.until(&mut hp, |l| l == "read bonjour")?;
    copy(&mut hp, &editor, &[TEXT_TYPE, SECRET_HINT], "hunter2")?;
    let offered = stand_in.until(&mut hp, |l| {
        l.starts_with("offered ") && l.contains(SECRET_HINT)
    })?;
    assert_eq!(offered, format!("offered {TEXT_TYPE} {SECRET_HINT}"));
    stand_in.until(&mut hp, |l| l == "read hunter2")?;
    Ok(())
}

/// The older protocol (scenario 31): the allowed client sets the selection through
/// `zwlr_data_control_manager_v1`; another client's paste reads it.
#[test]
fn the_older_protocol_sets_the_clipboard_too() -> anyhow::Result<()> {
    let linked = Linked::new()?;
    let mut hp = compositor(&linked)?;
    let (_, editor) = common::open_showing(
        &mut hp,
        "test.editor",
        Shows::Writing {
            app_id: "test.editor",
            size: (640, 420),
            caret: None,
        },
    )?;
    holds_keyboard(&mut hp, "test.editor")?;
    let _stand_in = StandIn::start(&mut hp, &linked, "wlr", Some("depuis le téléphone"), false)?;
    pastes(&mut hp, &editor, "depuis le téléphone")
}

/// The history's rules (5.6) hold for the phone's copies: one marked
/// secret is never kept.
#[test]
fn a_secret_from_the_phone_is_never_kept() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let linked = Linked::new()?;
    phone_entry(dir.path())?;
    let mut bed = Bed::start_with(dir.path(), |c| c.data_control = vec![linked.allowed()])?
        .context("no bed")?;
    let editor = bed.writing_window("test.editor", (640, 420))?;
    bed.focus(&editor)?;
    let secret = StandIn::start(&mut bed.hp, &linked, "ext", Some("motdepasse-du-tel"), true)?;
    // Pasted as any copy, but not kept.
    pastes(&mut bed.hp, &editor.client, "motdepasse-du-tel")?;
    drop(secret);
    wait(&mut bed.hp, Duration::from_millis(500))?;
    // Then a plain copy from the phone: the history holds it alone.
    let mark = bed.mark();
    let _plain = StandIn::start(&mut bed.hp, &linked, "ext", Some("une copie du tel"), false)?;
    bed.logged_after(mark, "a copy for the history")?;
    let (_, line) = bed.super_v()?;
    assert_eq!(
        history(&line),
        "RÉCENTS: [text] «une copie du tel» KDE Connect · maintenant (selected)"
    );
    Ok(())
}

/// Runs the compositor until `app_id`'s window holds the keyboard: a
/// client without it may neither take the clipboard nor read it.
fn holds_keyboard(hp: &mut Headless, app_id: &str) -> anyhow::Result<()> {
    let holds = hp.dispatch_until(WAIT, |hp| {
        hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some(app_id) && w.keyboard)
    })?;
    anyhow::ensure!(holds, "{app_id} never held the keyboard");
    Ok(())
}

/// The stand-in's desktop entry, among the bed's applications (as KDE
/// Connect's window's, which the launcher shows: the daemon's is hidden).
fn phone_entry(root: &Path) -> anyhow::Result<()> {
    let applications = root.join("data/applications");
    std::fs::create_dir_all(&applications)?;
    std::fs::write(
        applications.join(format!("{APP}.desktop")),
        "[Desktop Entry]\nType=Application\nName=KDE Connect\nExec=true\n",
    )?;
    Ok(())
}

/// `client`'s window copies `text` as `types`, once the compositor has it.
fn copy(
    hp: &mut Headless,
    client: &common::ClientHandle,
    types: &'static [&'static str],
    text: &'static str,
) -> anyhow::Result<()> {
    client.commands.send(Command::Copy(Clip {
        types,
        text,
        hang: false,
    }))?;
    let copied = hp.dispatch_until(WAIT, |_| {
        client.events.try_iter().any(|e| e == Event::Copied)
    })?;
    anyhow::ensure!(copied, "the clipboard was never taken");
    Ok(())
}

// The stand-in itself: a data control client as KDE Connect's daemon is
// (KGuiAddons' `KSystemClipboard`), saying what it sees on its standard
// output.

/// Its state: the connection, to send what it asks for at once.
struct Daemon {
    conn: Connection,
}

fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{SAYS}{line}");
    let _ = out.flush();
}

fn run_stand_in(socket: &Path) -> anyhow::Result<()> {
    let conn = Connection::from_socket(UnixStream::connect(socket)?)?;
    let (globals, mut queue) = registry_queue_init::<Daemon>(&conn)?;
    let qh = queue.handle();
    let names: Vec<String> = globals
        .contents()
        .with_list(|list| list.iter().map(|g| g.interface.clone()).collect());
    let seat: WlSeat = globals.bind(&qh, 1..=1, ())?;
    let set = std::env::var(SET).ok();
    let mut types = vec![TEXT_TYPE, "text/plain"];
    if std::env::var_os(SECRET).is_some() {
        types.push(SECRET_HINT);
    }
    let text = set.clone().unwrap_or_default();
    // Its objects live as long as it runs.
    let _objects: Box<dyn std::any::Any> = match std::env::var(PROTOCOL).as_deref() {
        Ok("wlr") => {
            let manager: ZwlrDataControlManagerV1 = globals.bind(&qh, 1..=2, ())?;
            let device = manager.get_data_device(&seat, &qh, ());
            let source = set.is_some().then(|| {
                let source = manager.create_data_source(&qh, text.clone());
                for t in &types {
                    source.offer((*t).to_owned());
                }
                device.set_selection(Some(&source));
                source
            });
            Box::new((manager, device, source))
        }
        _ => {
            let manager: ExtDataControlManagerV1 = globals.bind(&qh, 1..=1, ())?;
            let device = manager.get_data_device(&seat, &qh, ());
            let source = set.is_some().then(|| {
                let source = manager.create_data_source(&qh, text.clone());
                for t in &types {
                    source.offer((*t).to_owned());
                }
                device.set_selection(Some(&source));
                source
            });
            Box::new((manager, device, source))
        }
    };
    let mut daemon = Daemon { conn: conn.clone() };
    queue.roundtrip(&mut daemon)?;
    say(&format!("globals {}", names.join(" ")));
    loop {
        queue.blocking_dispatch(&mut daemon)?;
    }
}

impl Daemon {
    /// A selection offered as `types`: said, then its text read on a
    /// thread, as KDE Connect reads each copy.
    fn offered(&self, types: &[String], receive: impl FnOnce(std::os::fd::BorrowedFd<'_>)) {
        say(&format!("offered {}", types.join(" ")));
        if !types.iter().any(|t| t == TEXT_TYPE) {
            return;
        }
        let Ok((mut reader, writer)) = std::io::pipe() else {
            return;
        };
        receive(writer.as_fd());
        let _ = self.conn.flush();
        drop(writer);
        std::thread::spawn(move || {
            let mut text = String::new();
            if reader.read_to_string(&mut text).is_ok() {
                say(&format!("read {text}"));
            }
        });
    }
}

/// What a source writes for `mime`.
fn write_source(text: &str, mime: &str, fd: std::os::fd::OwnedFd) {
    let mut file = std::fs::File::from(fd);
    let bytes = if mime == SECRET_HINT { "secret" } else { text };
    let _ = file.write_all(bytes.as_bytes());
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Daemon {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for Daemon {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: <WlSeat as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtDataControlManagerV1, ()> for Daemon {
    fn event(
        _: &mut Self,
        _: &ExtDataControlManagerV1,
        _: <ExtDataControlManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for Daemon {
    fn event(
        daemon: &mut Self,
        _: &ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_data_control_device_v1::Event::Selection { id: Some(offer) } = event {
            let types = offer
                .data::<Mutex<Vec<String>>>()
                .and_then(|t| t.lock().ok().map(|t| t.clone()))
                .unwrap_or_default();
            daemon.offered(&types, |fd| offer.receive(TEXT_TYPE.to_owned(), fd));
        }
    }

    event_created_child!(Daemon, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, Mutex::new(Vec::new()))
    ]);
}

impl Dispatch<ExtDataControlOfferV1, Mutex<Vec<String>>> for Daemon {
    fn event(
        _: &mut Self,
        _: &ExtDataControlOfferV1,
        event: ext_data_control_offer_v1::Event,
        types: &Mutex<Vec<String>>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event
            && let Ok(mut types) = types.lock()
        {
            types.push(mime_type);
        }
    }
}

impl Dispatch<ExtDataControlSourceV1, String> for Daemon {
    fn event(
        _: &mut Self,
        _: &ExtDataControlSourceV1,
        event: ext_data_control_source_v1::Event,
        text: &String,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_data_control_source_v1::Event::Send { mime_type, fd } = event {
            write_source(text, &mime_type, fd);
        }
    }
}

impl Dispatch<ZwlrDataControlManagerV1, ()> for Daemon {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlManagerV1,
        _: <ZwlrDataControlManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for Daemon {
    fn event(
        daemon: &mut Self,
        _: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_device_v1::Event::Selection { id: Some(offer) } = event {
            let types = offer
                .data::<Mutex<Vec<String>>>()
                .and_then(|t| t.lock().ok().map(|t| t.clone()))
                .unwrap_or_default();
            daemon.offered(&types, |fd| offer.receive(TEXT_TYPE.to_owned(), fd));
        }
    }

    event_created_child!(Daemon, ZwlrDataControlDeviceV1, [
        zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, Mutex::new(Vec::new()))
    ]);
}

impl Dispatch<ZwlrDataControlOfferV1, Mutex<Vec<String>>> for Daemon {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlOfferV1,
        event: zwlr_data_control_offer_v1::Event,
        types: &Mutex<Vec<String>>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event
            && let Ok(mut types) = types.lock()
        {
            types.push(mime_type);
        }
    }
}

impl Dispatch<ZwlrDataControlSourceV1, String> for Daemon {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlSourceV1,
        event: zwlr_data_control_source_v1::Event,
        text: &String,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_source_v1::Event::Send { mime_type, fd } = event {
            write_source(text, &mime_type, fd);
        }
    }
}
