//! The session's side of the lock (#30): applications keep the screen
//! awake and ask to lock through the ScreenSaver service; logind asks to
//! lock, and the session locks before sleeping. Leaving (#72): an
//! application holding shutting down back is said and waited for, and
//! the machine offers hibernating only where it works. Needs
//! `dbus-daemon`; skipped without it.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use hocus_api::{Reply, Request};
use hocus_compositor::{FRAME, HarnessOptions, Headless, HeadlessConfig, RendererChoice};
use hocus_test::bus::{PrivateBus, installed};
use zbus::{OwnedObjectPath, Proxy, object_server::SignalEmitter};

fn compositor(
    session: Option<&PrivateBus>,
    system: Option<&PrivateBus>,
) -> anyhow::Result<Headless> {
    Headless::start_options(HarnessOptions {
        headless: HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..Default::default()
        },
        api_permission: hocus_api::Permission::Test,
        watchdog: None,
        xwayland: false,
        shell: true,
        session_bus: session.map(|b| b.address.clone()),
        system_bus: system.map(|b| b.address.clone()),
        data_dirs: None,
        child_env: None,
        polkit_helper: None,
    })
}

fn shows(compositor: &mut Headless, label: &str) -> anyhow::Result<bool> {
    let Reply::Ui(nodes) = compositor.request(Request::Ui)? else {
        anyhow::bail!("expected the interface");
    };
    Ok(nodes.iter().any(|n| n.label.as_deref() == Some(label)))
}

fn wait_for(compositor: &mut Headless, label: &str) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !shows(compositor, label)? {
        anyhow::ensure!(Instant::now() < deadline, "{label} never showed");
        compositor.dispatch(Duration::from_millis(10))?;
        compositor.advance(FRAME)?;
    }
    Ok(())
}

/// Lets a call made from a thread reach the compositor and come back.
fn call<T: Send + 'static>(
    compositor: &mut Headless,
    f: impl FnOnce() -> zbus::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    let (done, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(f());
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(r) = result.try_recv() {
            return Ok(r?);
        }
        anyhow::ensure!(Instant::now() < deadline, "the call never came back");
        compositor.dispatch(Duration::from_millis(5))?;
    }
}

fn screensaver(address: String) -> impl Fn(&'static str, u32) -> zbus::Result<u32> {
    move |method, cookie| {
        zbus::block_on(async {
            let conn = zbus::connection::Builder::address(address.as_str())
                .build()
                .await?;
            let proxy = Proxy::new(
                &conn,
                "org.freedesktop.ScreenSaver",
                "/org/freedesktop/ScreenSaver",
                "org.freedesktop.ScreenSaver",
            )
            .await?;
            match method {
                "Inhibit" => proxy.call("Inhibit", &("video", "playing")).await,
                "UnInhibit" => proxy
                    .call::<_, _, ()>("UnInhibit", &(cookie))
                    .await
                    .map(|()| 0),
                _ => proxy.call::<_, _, ()>(method, &()).await.map(|()| 0),
            }
        })
    }
}

#[test]
fn an_inhibitor_keeps_the_screen_awake_and_lock_locks() -> anyhow::Result<()> {
    if !installed("dbus-daemon") {
        eprintln!("skipped: no dbus-daemon");
        return Ok(());
    }
    let bus = PrivateBus::start()?;
    let mut compositor = compositor(Some(&bus), None)?;
    compositor.write_config("[idle]\ncurtain_after = 60\nlock_after = 30\n")?;
    let ss = screensaver(bus.address.clone());
    let cookie = call(&mut compositor, {
        let ss = screensaver(bus.address.clone());
        move || ss("Inhibit", 0)
    })?;
    compositor.advance(Duration::from_secs(120))?;
    assert!(
        !shows(&mut compositor, "Locking in 29\u{a0}s")?,
        "kept awake"
    );
    call(&mut compositor, move || ss("UnInhibit", cookie))?;
    compositor.advance(Duration::from_millis(61_500))?;
    wait_for(&mut compositor, "Locking in 29\u{a0}s")?;
    call(&mut compositor, {
        let ss = screensaver(bus.address.clone());
        move || ss("Lock", 0)
    })?;
    wait_for(&mut compositor, "Password")?;
    Ok(())
}

/// logind as far as the compositor uses it.
#[derive(Default)]
struct Manager {
    /// What ListInhibitors lists.
    inhibitors: Vec<(String, String, String, String, u32, u32)>,
    hibernate: bool,
    powered_off: Arc<AtomicBool>,
}

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl Manager {
    fn list_inhibitors(&self) -> Vec<(String, String, String, String, u32, u32)> {
        self.inhibitors.clone()
    }

    fn can_suspend(&self) -> &str {
        "yes"
    }

    fn can_hibernate(&self) -> &str {
        if self.hibernate { "yes" } else { "na" }
    }

    fn power_off(&self, _interactive: bool) {
        self.powered_off.store(true, Ordering::SeqCst);
    }

    #[zbus(name = "GetSessionByPID")]
    fn get_session_by_pid(&self, _pid: u32) -> OwnedObjectPath {
        OwnedObjectPath::try_from("/org/freedesktop/login1/session/_1").expect("a path")
    }

    fn inhibit(&self, _what: &str, _who: &str, _why: &str, _mode: &str) -> zbus::OwnedFd {
        let (read, write) = rustix::pipe::pipe().expect("a pipe");
        // The other end stays open as long as the test.
        std::mem::forget(write);
        zbus::OwnedFd::from(read)
    }

    #[zbus(signal)]
    async fn prepare_for_sleep(emitter: &SignalEmitter<'_>, start: bool) -> zbus::Result<()>;
}

struct Session;

#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl Session {
    #[zbus(signal)]
    async fn lock(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

fn logind(bus: &PrivateBus) -> anyhow::Result<zbus::Connection> {
    logind_with(bus, Manager::default())
}

fn logind_with(bus: &PrivateBus, manager: Manager) -> anyhow::Result<zbus::Connection> {
    Ok(zbus::block_on(
        zbus::connection::Builder::address(bus.address.as_str())
            .serve_at("/org/freedesktop/login1", manager)
            .serve_at("/org/freedesktop/login1/session/_1", Session)
            .name("org.freedesktop.login1")
            .build(),
    )?)
}

#[test]
fn logind_locks_on_request_and_before_sleep() -> anyhow::Result<()> {
    if !installed("dbus-daemon") {
        eprintln!("skipped: no dbus-daemon");
        return Ok(());
    }
    let bus = PrivateBus::start()?;
    let service = logind(&bus)?;
    let mut compositor = compositor(None, Some(&bus))?;
    // Its threads subscribe.
    let until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < until {
        compositor.dispatch(Duration::from_millis(10))?;
    }
    zbus::block_on(async {
        let iface = service
            .object_server()
            .interface::<_, Session>("/org/freedesktop/login1/session/_1")
            .await?;
        Session::lock(iface.signal_emitter()).await
    })?;
    wait_for(&mut compositor, "Password")?;
    // Unlocked with the password, then the machine goes to sleep.
    // "panda", then Enter.
    for code in [25, 30, 49, 32, 30, 28] {
        compositor.request(Request::Key {
            code,
            pressed: true,
        })?;
        compositor.request(Request::Key {
            code,
            pressed: false,
        })?;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while shows(&mut compositor, "Password")? {
        anyhow::ensure!(Instant::now() < deadline, "never unlocked");
        compositor.dispatch(Duration::from_millis(10))?;
        compositor.advance(FRAME)?;
    }
    zbus::block_on(async {
        let iface = service
            .object_server()
            .interface::<_, Manager>("/org/freedesktop/login1")
            .await?;
        Manager::prepare_for_sleep(iface.signal_emitter(), true).await
    })?;
    wait_for(&mut compositor, "Password")?;
    Ok(())
}

/// Clicks the shell's button with this label.
fn click(compositor: &mut Headless, label: &str) -> anyhow::Result<()> {
    let Reply::Ui(nodes) = compositor.request(Request::Ui)? else {
        anyhow::bail!("expected the interface");
    };
    let node = nodes
        .iter()
        .find(|n| n.role == "button" && n.label.as_deref() == Some(label))
        .ok_or_else(|| anyhow::anyhow!("no button {label:?}"))?;
    let r = node.rect;
    compositor.request(Request::PointerMove {
        x: f64::from(r.x) + f64::from(r.width) / 2.0,
        y: f64::from(r.y) + f64::from(r.height) / 2.0,
    })?;
    for pressed in [true, false] {
        compositor.request(Request::PointerButton {
            button: hocus_api::Button::Left,
            pressed,
        })?;
    }
    compositor.advance(FRAME)?;
    Ok(())
}

/// Lets the compositor's threads reach the bus.
fn settle(compositor: &mut Headless) -> anyhow::Result<()> {
    let until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < until {
        compositor.dispatch(Duration::from_millis(10))?;
    }
    Ok(())
}

#[test]
fn an_application_holding_shutdown_back_is_said_and_waited_for() -> anyhow::Result<()> {
    if !installed("dbus-daemon") {
        eprintln!("skipped: no dbus-daemon");
        return Ok(());
    }
    let bus = PrivateBus::start()?;
    let powered_off = Arc::new(AtomicBool::new(false));
    let _service = logind_with(
        &bus,
        Manager {
            inhibitors: vec![
                (
                    "shutdown".into(),
                    "Editor".into(),
                    "Unsaved document in notes.txt".into(),
                    "block".into(),
                    1000,
                    4242,
                ),
                // Only delays: it does not hold anything back.
                (
                    "sleep".into(),
                    "Hocuspanda".into(),
                    "Lock the screen before sleep".into(),
                    "delay".into(),
                    1000,
                    1,
                ),
            ],
            powered_off: powered_off.clone(),
            ..Default::default()
        },
    )?;
    let mut compositor = compositor(None, Some(&bus))?;
    settle(&mut compositor)?;
    click(&mut compositor, "Account: Alex")?;
    click(&mut compositor, "Shut down")?;
    wait_for(&mut compositor, "Editor — Unsaved document in notes.txt")?;
    // No countdown: it waits for a decision.
    compositor.advance(Duration::from_secs(15))?;
    settle(&mut compositor)?;
    assert!(shows(
        &mut compositor,
        "Editor — Unsaved document in notes.txt"
    )?);
    assert!(
        !powered_off.load(Ordering::SeqCst),
        "shut down without asking"
    );

    click(&mut compositor, "Shut down anyway")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !powered_off.load(Ordering::SeqCst) {
        anyhow::ensure!(Instant::now() < deadline, "logind was never asked");
        compositor.dispatch(Duration::from_millis(10))?;
        compositor.advance(FRAME)?;
    }
    Ok(())
}

#[test]
fn hibernating_is_offered_only_where_it_works() -> anyhow::Result<()> {
    if !installed("dbus-daemon") {
        eprintln!("skipped: no dbus-daemon");
        return Ok(());
    }
    for hibernate in [false, true] {
        let bus = PrivateBus::start()?;
        let _service = logind_with(
            &bus,
            Manager {
                hibernate,
                ..Default::default()
            },
        )?;
        let mut compositor = compositor(None, Some(&bus))?;
        settle(&mut compositor)?;
        compositor.advance(FRAME)?;
        click(&mut compositor, "Account: Alex")?;
        assert!(shows(&mut compositor, "Suspend")?);
        assert_eq!(shows(&mut compositor, "Hibernate")?, hibernate);
    }
    Ok(())
}

/// polkit's authority, as far as an agent uses it: it remembers where the
/// agent registered.
#[derive(Default)]
struct Authority {
    agent: Arc<std::sync::Mutex<Option<(String, String)>>>,
}

#[zbus::interface(name = "org.freedesktop.PolicyKit1.Authority")]
impl Authority {
    fn register_authentication_agent(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        _subject: (
            String,
            std::collections::HashMap<String, zbus::OwnedVariant>,
        ),
        _locale: String,
        path: String,
    ) {
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        if let Ok(mut agent) = self.agent.lock() {
            *agent = Some((sender, path));
        }
    }
}

fn type_keys(compositor: &mut Headless, codes: &[u32]) -> anyhow::Result<()> {
    for code in codes {
        for pressed in [true, false] {
            compositor.request(Request::Key {
                code: *code,
                pressed,
            })?;
        }
    }
    Ok(())
}

#[test]
fn the_system_asks_for_a_password_over_everything() -> anyhow::Result<()> {
    if !installed("dbus-daemon") {
        eprintln!("skipped: no dbus-daemon");
        return Ok(());
    }
    let bus = PrivateBus::start()?;
    let agent = Arc::new(std::sync::Mutex::new(None));
    let _service = zbus::block_on(
        zbus::connection::Builder::address(bus.address.as_str())
            .serve_at(
                "/org/freedesktop/PolicyKit1/Authority",
                Authority {
                    agent: agent.clone(),
                },
            )
            .name("org.freedesktop.PolicyKit1")
            .build(),
    )?;
    // polkit's helper, as far as the agent talks to it: "panda" passes.
    let dir = tempfile::tempdir()?;
    let helper = dir.path().join("helper");
    std::fs::write(
        &helper,
        "#!/bin/sh\nread cookie\necho 'PAM_PROMPT_ECHO_OFF Password: '\nread password\n\
         if [ \"$password\" = panda ]; then echo SUCCESS; else \
         echo 'PAM_ERROR_MSG Authentication failure'; echo FAILURE; fi\n",
    )?;
    std::fs::set_permissions(&helper, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    let mut compositor = Headless::start_options(HarnessOptions {
        headless: HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..Default::default()
        },
        api_permission: hocus_api::Permission::Test,
        watchdog: None,
        xwayland: false,
        shell: true,
        session_bus: None,
        system_bus: Some(bus.address.clone()),
        data_dirs: None,
        child_env: None,
        polkit_helper: Some(helper),
    })?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let (name, path) = loop {
        if let Some(found) = agent.lock().ok().and_then(|a| a.clone()) {
            break found;
        }
        anyhow::ensure!(Instant::now() < deadline, "the agent never registered");
        compositor.dispatch(Duration::from_millis(10))?;
    };

    // polkit asks, as for `pkexec true`; the call waits for the dialog.
    let address = bus.address.clone();
    let (done, answer) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result: zbus::Result<()> = zbus::block_on(async {
            let conn = zbus::connection::Builder::address(address.as_str())
                .build()
                .await?;
            let proxy = Proxy::new(
                &conn,
                name,
                path,
                "org.freedesktop.PolicyKit1.AuthenticationAgent",
            )
            .await?;
            let identities: Vec<(&str, std::collections::HashMap<&str, zbus::Variant<'_>>)> =
                vec![("unix-user", [("uid", zbus::Variant::from(0u32))].into())];
            proxy
                .call(
                    "BeginAuthentication",
                    &(
                        "org.freedesktop.policykit.exec",
                        "Authentication is needed to run `/usr/bin/true' as the super user",
                        "",
                        std::collections::HashMap::from([("command_line", "/usr/bin/true")]),
                        "cookie-1",
                        identities,
                    ),
                )
                .await
        });
        let _ = done.send(result);
    });
    wait_for(&mut compositor, "Authentication required")?;
    assert!(shows(&mut compositor, "/usr/bin/true")?);

    // A window covering the screen stays under it.
    let _window = hocus_test::client::SolidWindow {
        app_id: "cover".into(),
        width: 1920,
        height: 1080,
        argb: 0xff_ff_00_00,
    }
    .spawn(compositor.socket_path())?;
    compositor.dispatch_until(Duration::from_secs(10), |c| c.window_count() == 1)?;
    compositor.advance(FRAME * 2)?;
    let Reply::Screenshot(shot) = compositor.request(Request::Screenshot {
        target: hocus_api::Target::Output("HEADLESS-1".into()),
        cursor: false,
    })?
    else {
        anyhow::bail!("no screenshot");
    };
    let mut decoder = png::Decoder::new(std::io::Cursor::new(shot.png()?));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf)?;
    let channels = info.color_type.samples();
    let (w, h) = (info.width as usize, info.height as usize);
    let i = (h / 2 * w + w / 2) * channels;
    assert_ne!(&buf[i..i + 3], &[255, 0, 0], "the window covers the dialog");

    // The dialog has the keyboard: "bamboo", then Enter, is refused and
    // said.
    type_keys(&mut compositor, &[48, 30, 50, 48, 24, 24, 28])?;
    wait_for(&mut compositor, "Authentication failure")?;
    assert!(answer.try_recv().is_err(), "a wrong password let it run");
    // "panda", then Enter.
    type_keys(&mut compositor, &[25, 30, 49, 32, 30, 28])?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let result = loop {
        if let Ok(result) = answer.try_recv() {
            break result;
        }
        anyhow::ensure!(Instant::now() < deadline, "the call never came back");
        compositor.dispatch(Duration::from_millis(10))?;
        compositor.advance(FRAME)?;
    };
    result?;
    compositor.advance(FRAME)?;
    assert!(!shows(&mut compositor, "Authentication required")?);
    Ok(())
}
