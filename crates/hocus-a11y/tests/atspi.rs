//! The tree read back as a screen reader reads it: a session bus of the
//! test's own, its accessibility bus (`at-spi-bus-launcher`, which starts
//! `at-spi2-registryd`), a program showing a window through the bridge,
//! and AT-SPI's own library (its Python binding) walking it and pressing a
//! button. Never the session's buses: everything has a runtime directory,
//! a home and a bus of its own. Skipped without dbus-daemon, the launcher
//! or the binding.

use std::{
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

use hocus_a11y::{Act, Bridge, Event, Node, Rect, Role, Window};

const CHILD_ENV: &str = "HOCUS_A11Y_TEST_CHILD";
const LAUNCHER: &str = "/usr/libexec/at-spi-bus-launcher";
const WAIT: Duration = Duration::from_secs(15);

/// A bar of a few items, a popover's rows and a bubble, as the shell
/// describes them.
fn windows() -> Vec<Window> {
    let mut bar = Window::new("bar", "Barre — eDP-1");
    bar.size = Some((256.0, 800.0));
    bar.nodes = vec![
        Node::new("clock", Role::Button, "14 h 32, jeudi 6 novembre")
            .bounds(Some(Rect::new(8.0, 8.0, 240.0, 64.0)))
            .act(Act::Click),
        Node::new("desktops", Role::RadioGroup, "Bureaux").children([
            Node::new(
                "desktop:0",
                Role::RadioButton,
                "Bureau 1, actuel, 2 fenêtres",
            )
            .checked(true)
            .set(1, 2)
            .act(Act::Click),
            Node::new(
                "desktop:1",
                Role::RadioButton,
                "Bureau 2, 1 fenêtre : Firefox",
            )
            .checked(false)
            .set(2, 2)
            .act(Act::Click),
        ]),
        Node::new("volume", Role::Slider, "Volume")
            .value("62 %")
            .range(62.0, 0.0, 100.0, Some(5.0))
            .act(Act::Increment)
            .act(Act::Decrement),
        Node::new("battery", Role::ProgressBar, "Batterie")
            .value("82 %, 3 h 10 restantes, équilibré")
            .range(82.0, 0.0, 100.0, None),
    ];
    let mut popover = Window::new("popover", "Son");
    popover.active = true;
    popover.nodes = vec![
        Node::new("dialog", Role::Dialog, "Son")
            .describe("Haut-parleurs du portable")
            .children([
                Node::new("mute", Role::ToggleButton, "Couper le son")
                    .checked(false)
                    .act(Act::Click),
                Node::new("output:0", Role::RadioButton, "Haut-parleurs")
                    .checked(true)
                    .set(1, 2),
            ]),
    ];
    popover.focus = Some("mute".to_owned());
    let mut lock = Window::new("lock", "Verrouillage");
    lock.nodes = vec![
        Node::new("password", Role::PasswordField, "Mot de passe"),
        Node::new("unlock", Role::Button, "Déverrouiller").act(Act::Click),
    ];
    let mut bubble = Window::new("bubble:1", "Slack : Léa Martin");
    bubble.nodes = vec![
        Node::new("note", Role::Notification, "Slack : Léa Martin")
            .describe("On se voit à 15 h ?")
            .polite()
            .child(Node::new("action:reply", Role::Button, "Répondre").act(Act::Click)),
    ];
    vec![bar, popover, lock, bubble]
}

/// The program the screen reader reads: the bridge, its windows shown as
/// soon as something listens, the actions asked printed, until its input
/// closes.
fn child() {
    let (tell, events) = mpsc::channel();
    let mut bridge = Bridge::new(move |event| {
        let _ = tell.send(event);
    });
    bridge.keep(windows().iter().map(|w| w.key.as_str()).collect::<Vec<_>>());
    let (ended, end) = mpsc::channel();
    std::thread::spawn(move || {
        let mut rest = Vec::new();
        let _ = std::io::stdin().read_to_end(&mut rest);
        let _ = ended.send(());
    });
    loop {
        if end.try_recv().is_ok() {
            return;
        }
        match events.recv_timeout(Duration::from_millis(50)) {
            Ok(Event::Listening(true)) => {
                bridge.forget();
                bridge.show(windows());
                println!("shown {}", hocus_a11y::built());
            }
            Ok(Event::Listening(false)) => println!("not listened"),
            Ok(Event::Asked(asked)) => {
                println!("asked {} {} {:?}", asked.window, asked.node, asked.act)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Reads the tree of the application whose name starts with `app`, and
/// presses the button named "Déverrouiller".
const READER: &str = r#"
import sys, time
import gi
gi.require_version('Atspi', '2.0')
from gi.repository import Atspi

app_name = sys.argv[1]
deadline = time.time() + 10
app = None
while app is None and time.time() < deadline:
    desktop = Atspi.get_desktop(0)
    for i in range(desktop.get_child_count()):
        a = desktop.get_child_at_index(i)
        if a is not None and (a.get_name() or '') == app_name and a.get_child_count() >= 4:
            app = a
    if app is None:
        time.sleep(0.1)
if app is None:
    print('no application')
    sys.exit(1)

def states(acc):
    s = acc.get_state_set()
    out = []
    for name in ('checked', 'pressed', 'focused', 'active', 'focusable'):
        if s.contains(getattr(Atspi.StateType, name.upper())):
            out.append(name)
    return out

def walk(acc, depth, press):
    line = '  ' * depth + acc.get_role_name() + ' | ' + (acc.get_name() or '')
    desc = acc.get_description()
    if desc:
        line += ' | desc=' + desc
    value = acc.get_value_iface()
    if value is not None:
        line += ' | value=%g/%g..%g' % (value.get_current_value(), value.get_minimum_value(), value.get_maximum_value())
        text = value.get_text() if hasattr(value, 'get_text') else None
        if text:
            line += ' "' + text + '"'
    line += ' | ' + ','.join(states(acc))
    action = acc.get_action_iface()
    if action is not None:
        names = [action.get_action_name(i) for i in range(action.get_n_actions())]
        line += ' | actions=' + ','.join(names)
        if acc.get_name() == press:
            action.do_action(0)
    print(line, flush=True)
    for i in range(acc.get_child_count()):
        walk(acc.get_child_at_index(i), depth + 1, press)

walk(app, 0, 'Déverrouiller')
"#;

/// Processes the test started, stopped by their own ids as it ends: the
/// program, the session bus, then the launcher, which stops its
/// accessibility bus (and so the registry) as the session bus goes.
#[derive(Default)]
struct Started {
    program: Option<Child>,
    daemon: Option<Child>,
    launcher: Option<Child>,
}

impl Drop for Started {
    fn drop(&mut self) {
        for child in [&mut self.program, &mut self.daemon].into_iter().flatten() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(launcher) = &mut self.launcher {
            let until = Instant::now() + Duration::from_secs(5);
            while launcher.try_wait().ok().flatten().is_none() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = launcher.kill();
            let _ = launcher.wait();
        }
    }
}

fn found(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn has_binding() -> bool {
    Command::new("python3")
        .args([
            "-I",
            "-c",
            "import gi; gi.require_version('Atspi', '2.0'); from gi.repository import Atspi",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// A command with nothing of the test's environment but what is given:
/// never the session's buses.
fn private(program: impl AsRef<std::ffi::OsStr>, dir: &Path, bus: &str) -> Command {
    let mut command = Command::new(program);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", dir)
        .env("XDG_RUNTIME_DIR", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("DBUS_SESSION_BUS_ADDRESS", bus)
        .env("GSETTINGS_BACKEND", "memory")
        .env("NO_AT_BRIDGE", "1");
    command
}

fn lines(out: impl Read + Send + 'static) -> mpsc::Receiver<String> {
    let (said, heard) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines() {
            let Ok(line) = line else { return };
            if said.send(line).is_err() {
                return;
            }
        }
    });
    heard
}

fn wait_for(heard: &mpsc::Receiver<String>, start: &str) -> Option<String> {
    let until = Instant::now() + WAIT;
    while let Some(left) = until.checked_duration_since(Instant::now()) {
        match heard.recv_timeout(left) {
            Ok(line) if line.starts_with(start) => return Some(line),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    None
}

#[test]
fn a_screen_reader_reads_the_tree_and_presses_a_button() {
    if std::env::var_os(CHILD_ENV).is_some() {
        child();
        return;
    }
    if !found("dbus-daemon") || !Path::new(LAUNCHER).exists() || !has_binding() {
        eprintln!("skipped: needs dbus-daemon, {LAUNCHER} and AT-SPI's Python binding");
        return;
    }
    // Short: a socket's path has a limit.
    let dir = tempfile::Builder::new()
        .prefix("hp-a11y")
        .tempdir_in("/tmp")
        .expect("a directory");
    let dir = dir.path().to_owned();
    std::fs::create_dir_all(dir.join("config")).expect("config");
    let mut started = Started::default();

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
    )
    .expect("the bus's configuration");
    let mut daemon = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", config.display()))
        .args(["--nofork", "--print-address=1"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("dbus-daemon");
    let address = daemon.stdout.take().map(lines);
    started.daemon = Some(daemon);
    let bus = address
        .and_then(|heard| heard.recv_timeout(WAIT).ok())
        .expect("the bus's address");
    assert!(bus.contains(&socket.display().to_string()), "{bus}");

    // The accessibility bus, turned on as a screen reader turns it on.
    let launcher = private(LAUNCHER, &dir, &bus)
        .arg("--launch-immediately")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("at-spi-bus-launcher");
    started.launcher = Some(launcher);
    let call = |args: &[&str]| {
        private("gdbus", &dir, &bus)
            .args([
                "call",
                "--session",
                "-d",
                "org.a11y.Bus",
                "-o",
                "/org/a11y/bus",
                "-m",
            ])
            .args(args)
            .output()
            .is_ok_and(|o| o.status.success())
    };
    let until = Instant::now() + WAIT;
    while !call(&["org.a11y.Bus.GetAddress"]) {
        assert!(Instant::now() < until, "the accessibility bus did not come");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(call(&[
        "org.freedesktop.DBus.Properties.Set",
        "org.a11y.Status",
        "IsEnabled",
        "<true>",
    ]));

    // The program, this test again.
    let exe: PathBuf = std::env::current_exe().expect("the test's program");
    let mut program = private(&exe, &dir, &bus)
        .env(CHILD_ENV, "1")
        .args([
            "--exact",
            "atspi::a_screen_reader_reads_the_tree_and_presses_a_button",
            "--nocapture",
            "--test-threads=1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the program");
    let said = program.stdout.take().map(lines).expect("its output");
    let input = program.stdin.take();
    started.program = Some(program);
    let shown = wait_for(&said, "shown").expect("the program heard nobody listen");
    assert_ne!(shown, "shown 0", "no tree built");

    // Named as the bridge names it, not after its program.
    let read = private("python3", &dir, &bus)
        .args(["-I", "-c", READER, "Hocuspanda"])
        .output()
        .expect("the reader");
    let tree = String::from_utf8_lossy(&read.stdout).into_owned();
    assert!(
        read.status.success(),
        "{tree}\n{}",
        String::from_utf8_lossy(&read.stderr)
    );
    let has = |line: &str| {
        assert!(
            tree.lines().any(|l| l.trim_start().starts_with(line)),
            "no line starting {line:?} in:\n{tree}"
        );
    };
    has("application | Hocuspanda");
    has("frame | Barre — eDP-1");
    has("panel | Bureaux");
    has("button | 14 h 32, jeudi 6 novembre |");
    has("radio button | Bureau 1, actuel, 2 fenêtres | checked");
    has("radio button | Bureau 2, 1 fenêtre : Firefox |");
    has("slider | Volume | value=62/0..100");
    has("progress bar | Batterie | value=82/0..100");
    has("dialog | Son | desc=Haut-parleurs du portable");
    has("toggle button | Couper le son |");
    has("password text | Mot de passe |");
    has("button | Déverrouiller |");
    has("notification | Slack : Léa Martin | desc=On se voit à 15 h ?");
    has("button | Répondre |");
    // The unpressed toggle is not pressed; the focused row is focused in
    // the active window.
    let mute = tree
        .lines()
        .find(|l| l.contains("Couper le son"))
        .unwrap_or_default();
    assert!(!mute.contains("pressed"), "{mute}");
    assert!(mute.contains("focused"), "{mute}");

    let asked = wait_for(&said, "asked").expect("the press did not come back");
    assert_eq!(asked, "asked lock unlock Click");

    // Its input closed, it ends on its own.
    drop(input);
    if let Some(program) = &mut started.program {
        let until = Instant::now() + WAIT;
        while program.try_wait().ok().flatten().is_none() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
