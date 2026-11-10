//! Slack through its notifications (secretary.md §20 lot 7, AC16): the
//! real shell serving the notifications on a D-Bus daemon of the test's
//! own (never the session's), the secretary's process beside it in
//! assistant mode, on a clock and a fake model of her own, her folder the
//! test's, its consent book granting Slack only.
//!
//! A Slack notification with actions reaches her intake; a Signal one
//! (not consented to) does not; and neither is ever acted on, closed nor
//! marked: the application hears no `ActionInvoked` and no
//! `NotificationClosed`, and both bubbles stay as they came.

use crate::common;

use std::{
    collections::HashMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Child,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, SecretaryMode};
use hocus_secretary_link::{
    Client, Connection, Notebook, Status,
    consent::{CONSENTS, ConsentBook, SourceKey},
};
use zbus::{MatchRule, MessageStream, OwnedVariant, Variant};

const WAIT: Duration = Duration::from_secs(20);

/// What the applications heard: `closed ID REASON`, `action ID KEY`.
type Heard = Arc<Mutex<Vec<String>>>;

struct Bed {
    dir: tempfile::TempDir,
    hp: Headless,
    bus: Child,
    runtime: tokio::runtime::Runtime,
    client: zbus::Connection,
    heard: Heard,
}

impl Drop for Bed {
    fn drop(&mut self) {
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}

/// The shell and her programs, and a D-Bus daemon; `None` to skip.
fn ready() -> Option<(PathBuf, PathBuf)> {
    let (shell, secretary) = (common::shell_program(), common::secretary_program());
    let daemon = std::process::Command::new("sh")
        .args(["-c", "command -v dbus-daemon"])
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !daemon {
        eprintln!("no dbus-daemon: skipped");
        return None;
    }
    Some((shell, secretary))
}

fn executable(path: &Path, text: &str) -> anyhow::Result<()> {
    std::fs::write(path, text)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn start(shell: &Path, secretary: &Path) -> anyhow::Result<Bed> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    for sub in ["home", "state/hocuspanda", "data/hocuspanda/secretary"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    // Slack consented to, in the pro notebook; nothing else.
    let mut book = ConsentBook::default();
    book.allow(
        SourceKey::notifications("Slack", Some("slack")),
        "Slack",
        Notebook::Pro,
        "2026-11-09T08:00:00Z".parse()?,
    );
    book.save(&root.join("data/hocuspanda/secretary").join(CONSENTS))?;
    let (bus, address) = common::private_bus(root)?;
    let at = |sub: &str| root.join(sub).display().to_string();
    executable(
        &root.join("shell"),
        &format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{home}' \
             XDG_CONFIG_HOME='{home}/.config' XDG_CACHE_HOME='{home}/.cache' \
             XDG_STATE_HOME='{state}' XDG_DATA_HOME='{data}' TZ=UTC \
             RUST_LOG=warn,hocuspanda_shell::secretary=debug\n\
             exec '{shell}' \"$@\" 2>>'{log}'\n",
            home = at("home"),
            state = at("state"),
            data = at("data"),
            shell = shell.display(),
            log = at("shell.log"),
        ),
    )?;
    std::fs::write(root.join("model.toml"), "[default]\ntext = \"D'accord.\"\n")?;
    // Her keyring's bus would be the notifications' one, which has none:
    // a key file in her folder instead. A fake model: no local one is
    // ever asked.
    executable(
        &root.join("secretary"),
        &format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS HOCUS_SECRETARY_BUS\n\
             export XDG_DATA_HOME='{data}' HOME='{home}'\n\
             exec '{secretary}' --virtual-clock 2026-11-09T08:00:00Z --fake-model '{model}' \
             --no-google \"$@\" 2>>'{log}'\n",
            data = at("data"),
            home = at("home"),
            secretary = secretary.display(),
            model = at("model.toml"),
            log = at("secretary.log"),
        ),
    )?;
    let mut config = Config {
        secretary_program: Some(root.join("secretary")),
        ..Config::default()
    };
    config.secretary.mode = SecretaryMode::Assistant;
    let mut hp = Headless::start_on_bus(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        config,
        Some(root.join("shell")),
        Some(address.clone()),
    )?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let client = runtime.block_on(zbus::connection::Builder::address(address.as_str()).build())?;
    let heard = Heard::default();
    runtime.block_on(listen(&client, heard.clone()))?;
    let drawn = hp.dispatch_until(WAIT, |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(drawn, "no bar");
    Ok(Bed {
        dir,
        hp,
        bus,
        runtime,
        client,
        heard,
    })
}

/// The notifications' signals, as `closed ID REASON` and `action ID KEY`.
async fn listen(client: &zbus::Connection, heard: Heard) -> anyhow::Result<()> {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.Notifications")
        .build()?;
    let stream = MessageStream::for_match_rule(rule, client, None).await?;
    tokio::spawn(async move {
        use zbus::export::futures_core::Stream;
        let mut stream = Box::pin(stream);
        while let Some(Ok(message)) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await
        {
            let header = message.header();
            let said = match header.member().map(|m| m.as_str().to_owned()).as_deref() {
                Some("NotificationClosed") => message
                    .body()
                    .deserialize::<(u32, u32)>()
                    .map(|(id, reason)| format!("closed {id} {reason}")),
                Some("ActionInvoked") => message
                    .body()
                    .deserialize::<(u32, String)>()
                    .map(|(id, key)| format!("action {id} {key}")),
                _ => continue,
            };
            if let Ok(said) = said {
                heard
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(said);
            }
        }
    });
    Ok(())
}

impl Bed {
    /// `app` notifies, with actions and a default, never expiring; its id.
    fn notify(
        &mut self,
        app: &str,
        entry: Option<&str>,
        summary: &str,
        body: &str,
    ) -> anyhow::Result<u32> {
        let mut hints: HashMap<&str, OwnedVariant> = HashMap::new();
        if let Some(entry) = entry {
            hints.insert(
                "desktop-entry",
                OwnedVariant::try_from(Variant::from(entry))?,
            );
        }
        let call = (
            app,
            0u32,
            "",
            summary,
            body,
            vec!["default", "Ouvrir", "reply", "Répondre"],
            hints,
            0i32,
        );
        let client = self.client.clone();
        self.runtime.block_on(async move {
            let reply = client
                .call_method(
                    Some("org.freedesktop.Notifications"),
                    "/org/freedesktop/Notifications",
                    Some("org.freedesktop.Notifications"),
                    "Notify",
                    &call,
                )
                .await?;
            anyhow::Ok(reply.body().deserialize::<u32>()?)
        })
    }

    fn heard(&self) -> Vec<String> {
        self.heard
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// A connection to her once she listens.
    fn her(&mut self) -> anyhow::Result<Connection> {
        let socket = self
            .hp
            .secretary_socket()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow::anyhow!("no socket"))?;
        let start = Instant::now();
        loop {
            if let Ok(connection) = Connection::open(&socket, Client::Test, Duration::from_secs(5))
            {
                return Ok(connection);
            }
            anyhow::ensure!(start.elapsed() < WAIT, "she never listened");
            self.hp.dispatch(Duration::from_millis(20))?;
        }
    }

    /// Her status once `done` holds of it.
    fn until(
        &mut self,
        her: &mut Connection,
        done: impl Fn(&Status) -> bool,
    ) -> anyhow::Result<Status> {
        let start = Instant::now();
        loop {
            let status = her.status().map_err(anyhow::Error::msg)?;
            if done(&status) {
                return Ok(status);
            }
            anyhow::ensure!(
                start.elapsed() < WAIT,
                "never: {status:?}\n{}",
                common::logged(&self.dir.path().join("secretary.log"))
            );
            self.hp.dispatch(Duration::from_millis(20))?;
        }
    }
}

/// The notifications' source's items read so far.
fn read(status: &Status) -> u64 {
    status
        .sources
        .iter()
        .find(|s| s.name == "notifications")
        .map_or(0, |s| s.items)
}

/// AC16: a consented Slack's notification reaches her intake, an
/// unconsented application's does not, and no notification is ever acted
/// on, closed or marked because of her.
#[test]
fn slack_s_notifications_reach_her_and_none_is_ever_acted_on() -> anyhow::Result<()> {
    let Some((shell, secretary)) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, &secretary)?;
    let mut her = bed.her()?;
    bed.until(&mut her, |s| {
        s.connections
            .iter()
            .any(|c| c.client == Some(Client::Shell))
            && s.sources.iter().all(|s| !s.busy)
    })?;

    let slack = bed.notify(
        "Slack",
        Some("slack"),
        "Karim (#prod-alertes)",
        "la prod est tombée, tu regardes ?",
    )?;
    let signal = bed.notify("Signal", None, "Claire", "Dimanche chez maman ?")?;
    let status = bed.until(&mut her, |s| read(s) == 1 && s.counts.episodes == 1)?;
    assert_eq!(status.counts.episodes, 1, "only Slack's");

    // Both bubbles stay as they came, and the applications hear nothing:
    // no action invoked, no default, nothing closed.
    let shown = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.bubble_surfaces().len() == 2)?;
    assert!(shown, "both bubbles show");
    bed.hp.dispatch_until(Duration::from_secs(2), |_| false)?;
    assert_eq!(
        bed.heard(),
        Vec::<String>::new(),
        "{slack} and {signal} untouched"
    );
    assert_eq!(bed.hp.bubble_surfaces().len(), 2, "neither closed");
    let status = her.status().map_err(anyhow::Error::msg)?;
    assert_eq!(read(&status), 1, "Signal's never forwarded");

    let log = common::logged(&bed.dir.path().join("shell.log"));
    assert_eq!(
        log.matches("a notification forwarded to the secretary")
            .count(),
        1,
        "{log}"
    );
    Ok(())
}
