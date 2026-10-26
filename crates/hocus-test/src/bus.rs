//! A private D-Bus bus for tests: never the session's. It activates no
//! service, so nothing installed on the machine starts on it.

use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
};

use anyhow::Context;

const CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path=SOCKET</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#;

/// A `dbus-daemon` of our own; stopped (by its pid) when dropped.
#[derive(Debug)]
pub struct PrivateBus {
    child: Child,
    pub address: String,
    _dir: tempfile::TempDir,
}

impl PrivateBus {
    pub fn start() -> anyhow::Result<Self> {
        // A short path: sockets have a length limit.
        let dir = tempfile::Builder::new().prefix("hbus").tempdir_in("/tmp")?;
        let config = dir.path().join("bus.conf");
        let socket = dir.path().join("bus");
        std::fs::write(&config, CONFIG.replace("SOCKET", &socket.to_string_lossy()))?;
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("could not start dbus-daemon")?;
        let mut line = String::new();
        let stdout = child.stdout.take().context("no output from dbus-daemon")?;
        BufReader::new(stdout).read_line(&mut line)?;
        let address = line.trim().to_owned();
        if address.is_empty() {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("dbus-daemon gave no address");
        }
        Ok(Self {
            child,
            address,
            _dir: dir,
        })
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Whether `program` is on the `PATH`.
pub fn installed(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// A notification, as `Notify` takes it.
#[derive(Debug, Clone, Copy)]
pub struct Note<'a> {
    pub app: &'a str,
    pub replaces: u32,
    pub summary: &'a str,
    pub body: &'a str,
    pub actions: &'a [&'a str],
    /// 0 low, 1 normal, 2 critical.
    pub urgency: u8,
    pub timeout_ms: i32,
    /// Hints of text, besides the urgency (`x-kde-appname`…).
    pub hints: &'a [(&'a str, &'a str)],
}

/// Sends `note` to the notification server on the bus at `address`, and
/// returns its id.
pub fn notify(address: &str, note: &Note<'_>) -> anyhow::Result<u32> {
    let connection = zbus::block_on(zbus::connection::Builder::address(address).build())?;
    notify_on(&connection, note)
}

/// The same on an open connection, for many in a row.
pub fn notify_on(connection: &zbus::Connection, note: &Note<'_>) -> anyhow::Result<u32> {
    let mut hints: std::collections::HashMap<&str, zbus::Variant<'_>> = Default::default();
    hints.insert("urgency", note.urgency.into());
    for (key, value) in note.hints {
        hints.insert(key, (*value).into());
    }
    let reply = zbus::block_on(connection.call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "Notify",
        &(
            note.app,
            note.replaces,
            "",
            note.summary,
            note.body,
            note.actions,
            hints,
            note.timeout_ms,
        ),
    ))?;
    Ok(reply.body().deserialize::<u32>()?)
}

/// The notification server's signals, as they come: `ActivationToken ID`
/// (`ActivationToken ID empty` for an empty token), `ActionInvoked ID
/// KEY`, `NotificationClosed ID REASON`.
pub type Signals = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// Listens to the notification server's signals on the bus at `address`,
/// from a thread of its own until the bus goes; returns once listening.
pub fn listen_notifications(address: &str) -> anyhow::Result<Signals> {
    use futures_core::Stream;
    let signals: Signals = Default::default();
    let (ready, listening) = std::sync::mpsc::channel();
    let address = address.to_owned();
    let log = signals.clone();
    std::thread::Builder::new()
        .name("notification signals".into())
        .spawn(move || {
            zbus::block_on(async move {
                let stream = async {
                    let connection = zbus::connection::Builder::address(address.as_str())
                        .build()
                        .await?;
                    let rule = zbus::MatchRule::builder()
                        .msg_type(zbus::message::Type::Signal)
                        .interface("org.freedesktop.Notifications")
                        .build()?;
                    zbus::MessageStream::for_match_rule(rule, &connection, None).await
                };
                let mut stream = match stream.await {
                    Ok(s) => {
                        let _ = ready.send(Ok(()));
                        s
                    }
                    Err(err) => {
                        let _ = ready.send(Err(err.to_string()));
                        return;
                    }
                };
                while let Some(Ok(message)) =
                    std::future::poll_fn(|cx| std::pin::Pin::new(&mut stream).poll_next(cx)).await
                {
                    let header = message.header();
                    let body = message.body();
                    let line = match header.member().map(|m| m.as_str()) {
                        Some("ActivationToken") => {
                            body.deserialize::<(u32, String)>().ok().map(|(id, token)| {
                                if token.is_empty() {
                                    format!("ActivationToken {id} empty")
                                } else {
                                    format!("ActivationToken {id}")
                                }
                            })
                        }
                        Some("ActionInvoked") => body
                            .deserialize::<(u32, String)>()
                            .ok()
                            .map(|(id, key)| format!("ActionInvoked {id} {key}")),
                        Some("NotificationClosed") => body
                            .deserialize::<(u32, u32)>()
                            .ok()
                            .map(|(id, reason)| format!("NotificationClosed {id} {reason}")),
                        _ => None,
                    };
                    if let (Some(line), Ok(mut log)) = (line, log.lock()) {
                        log.push(line);
                    }
                }
            });
        })?;
    match listening.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(Ok(())) => Ok(signals),
        Ok(Err(err)) => anyhow::bail!("could not listen to the notifications: {err}"),
        Err(_) => anyhow::bail!("the bus did not answer"),
    }
}
