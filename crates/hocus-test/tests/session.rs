//! The session's side of the lock (#30): applications keep the screen
//! awake and ask to lock through the ScreenSaver service; logind asks to
//! lock, and the session locks before sleeping. Needs `dbus-daemon`;
//! skipped without it.

use std::time::{Duration, Instant};

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
struct Manager;

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl Manager {
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
    Ok(zbus::block_on(
        zbus::connection::Builder::address(bus.address.as_str())
            .serve_at("/org/freedesktop/login1", Manager)
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
