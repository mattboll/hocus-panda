//! The system's authentication dialog (7.5, issue #416): the real shell,
//! the session's polkit agent on a D-Bus daemon of the test's own (as the
//! system bus), against a fake polkit authority that records the agent's
//! registration and calls `BeginAuthentication` and
//! `CancelAuthentication` as polkitd does, and a fake helper (a script
//! speaking `polkit-agent-helper-1`'s protocol, which takes "panda"). The
//! accounts are the test's own (`HOCUS_AUTH_ETC`), the asking process the
//! test's (its windows are the asking application's). Never the machine's
//! polkit, its helper or its system bus.
//!
//! One test per point: the registration; the dialog says who asks, why and
//! for whom; the right password authorizes; a wrong one says so; three
//! close and refuse; `Esc` and Annuler cancel; polkit cancelling closes
//! it; two requests wait their turn; two accounts to choose from; every
//! screen dimmed and nothing else gets the pointer or the keys; at rest it
//! draws nothing; and polkit's socket (polkit 126 and later): the login,
//! then the cookie.
//!
//! Pixman here, the shell on the CPU, without motion: as in CI. Without
//! the shell built or dbus-daemon, the tests fail.

use crate::common;

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use common::{
    BTN_LEFT, Event, KEY_ESC, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_TAB, Pointed, Shows, WAIT,
};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use hocus_link::AUTH_NAMESPACE;
use zbus::{OwnedVariant, Variant, message::Header};

const KEY_ENTER: u32 = 28;
const KEY_B: u32 = 48;
const KEY_X: u32 = 45;
const APP: &str = "org.example.Updates";
const AGENT: &str = "org.freedesktop.PolicyKit1.AuthenticationAgent";
const MESSAGE: &str = "L'authentification est nécessaire pour installer des mises à jour.";
const ACTION: &str = "org.example.updates.install";

fn has(program: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn ready() -> Option<PathBuf> {
    let shell = common::shell_program();
    if !has("dbus-daemon") {
        common::missing("dbus-daemon");
    }
    Some(shell)
}

/// What the fake authority saw: the agents registered (their bus name,
/// subject's kind, session id, locale and path) and how each request was
/// answered, by cookie (`ok`, or the error's name).
#[derive(Debug, Default)]
struct Seen {
    registered: Vec<(String, String, String, String, String)>,
    answers: HashMap<String, String>,
}

type Shared = Arc<Mutex<Seen>>;

fn seen(shared: &Shared) -> std::sync::MutexGuard<'_, Seen> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// polkit's authority, as far as an agent sees it.
struct Authority(Shared);

#[zbus::interface(name = "org.freedesktop.PolicyKit1.Authority")]
impl Authority {
    fn register_authentication_agent(
        &self,
        subject: (String, HashMap<String, OwnedVariant>),
        locale: String,
        path: String,
        #[zbus(header)] header: Header<'_>,
    ) {
        let session = subject
            .1
            .get("session-id")
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
            .unwrap_or_default();
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        seen(&self.0)
            .registered
            .push((sender, subject.0, session, locale, path));
    }

    fn unregister_authentication_agent(
        &self,
        _subject: (String, HashMap<String, OwnedVariant>),
        _path: String,
    ) {
    }
}

/// How the test's accounts are said to polkit's agent.
enum Who {
    /// The test's own user.
    Me,
    /// The `sudo` group: the test's user and Sam.
    Admins,
}

struct Bed {
    hp: Headless,
    log: PathBuf,
    helper_log: PathBuf,
    runtime: tokio::runtime::Runtime,
    authority: zbus::Connection,
    seen: Shared,
    /// The test's process: the asking one.
    pid: u32,
    /// Last, so stopped once the compositor and its connections are.
    bus: Child,
    _dir: tempfile::TempDir,
}

impl Drop for Bed {
    fn drop(&mut self) {
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}

/// The fake helper: the login in its arguments, the cookie on its input,
/// then a password prompt; "panda" is the password.
const HELPER: &str = "#!/bin/sh\n\
    read -r cookie\n\
    printf '%s %s\\n' \"$1\" \"$cookie\" >> \"$HOCUS_TEST_HELPER_LOG\"\n\
    printf 'PAM_PROMPT_ECHO_OFF Password: \\n'\n\
    read -r password\n\
    if [ \"$password\" = panda ]; then echo SUCCESS; else echo FAILURE; fi\n";

fn uid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("Uid:"))
                .and_then(|l| l.split_whitespace().next())
                .and_then(|u| u.parse().ok())
        })
        .unwrap_or(1000)
}

impl Bed {
    fn start(shell: &Path) -> anyhow::Result<Self> {
        Self::start_on(shell, &[(1920, 1080)], &[], true)
    }

    /// The bed on screens of these sizes, with `env` for the shell too, on
    /// a plain background or the desktop's own.
    fn start_on(
        shell: &Path,
        screens: &[(u32, u32)],
        env: &[(&str, String)],
        plain: bool,
    ) -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        let state = dir.path().join("state");
        let etc = dir.path().join("etc");
        let apps = dir.path().join("share/applications");
        for sub in [&home, &state.join("hocuspanda"), &etc, &apps] {
            std::fs::create_dir_all(sub)?;
        }
        let me = uid();
        std::fs::write(
            etc.join("passwd"),
            format!(
                "alex:x:{me}:{me}:Alex Martin,,,:/home/alex:/bin/sh\n\
                 sam:x:{}:{}:Sam Leroy:/home/sam:/bin/sh\n",
                me + 1,
                me + 1
            ),
        )?;
        std::fs::write(etc.join("group"), "sudo:x:27:alex,sam\n")?;
        std::fs::write(
            apps.join(format!("{APP}.desktop")),
            "[Desktop Entry]\nType=Application\nName=Updates\nName[fr]=Mises à jour\n\
             Exec=true\n",
        )?;
        let helper = dir.path().join("helper");
        std::fs::write(&helper, HELPER)?;
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755))?;
        let helper_log = dir.path().join("helper.log");
        std::fs::write(&helper_log, "")?;
        let (bus, address) = common::private_bus(dir.path())?;
        let log = dir.path().join("shell.log");
        let extra: String = env
            .iter()
            .map(|(name, value)| format!("export {name}='{value}'\n"))
            .collect();
        let script = dir.path().join("shell");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='{address}' \
                 PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{home}' \
                 XDG_CONFIG_HOME='{home}/.config' XDG_CACHE_HOME='{home}/.cache' \
                 XDG_DATA_HOME='{home}/.local/share' XDG_DATA_DIRS='{share}' \
                 XDG_STATE_HOME='{state}' TZ=UTC ICED_BACKEND=tiny-skia \
                 HOCUS_AUTH_AGENT=1 XDG_SESSION_ID=7 HOCUS_AUTH_ETC='{etc}' \
                 HOCUS_POLKIT_HELPER='{helper}' HOCUS_TEST_HELPER_LOG='{helper_log}' \
                 RUST_LOG=warn,hocuspanda_shell=debug\n{extra}\
                 exec '{shell}' \"$@\" 2>>'{log}'\n",
                home = home.display(),
                share = dir.path().join("share").display(),
                state = state.display(),
                etc = etc.display(),
                helper = helper.display(),
                helper_log = helper_log.display(),
                shell = shell.display(),
                log = log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let seen = Shared::default();
        let authority = runtime.block_on(
            zbus::connection::Builder::address(address.as_str())
                .name("org.freedesktop.PolicyKit1")
                .serve_at(
                    "/org/freedesktop/PolicyKit1/Authority",
                    Authority(seen.clone()),
                )
                .build(),
        )?;
        let config = Config {
            plain_background: plain,
            ..Config::default()
        };
        let outputs = screens
            .iter()
            .map(|(w, h)| format!("{w}x{h}").parse())
            .collect::<Result<Vec<_>, _>>()?;
        let mut hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs,
                renderer: RendererChoice::Pixman,
                render_node: None,
            },
            config,
            Some(script),
        )?;
        let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
        anyhow::ensure!(drawn, "no bar");
        let mut bed = Self {
            hp,
            log,
            helper_log,
            runtime,
            authority,
            seen,
            pid: std::process::id(),
            bus,
            _dir: dir,
        };
        bed.registered()?;
        bed.hp.pointer_move(400.0, 600.0);
        Ok(bed)
    }

    fn idle(&mut self, time: Duration) -> anyhow::Result<()> {
        self.hp.dispatch_until(time, |_| false)?;
        Ok(())
    }

    /// Waits until the shell registered as the agent.
    fn registered(&mut self) -> anyhow::Result<(String, String, String, String, String)> {
        let seen = self.seen.clone();
        let done = self.hp.dispatch_until(Duration::from_secs(15), |_| {
            !self::seen(&seen).registered.is_empty()
        })?;
        anyhow::ensure!(done, "the shell never registered:\n{}", self.log());
        Ok(self::seen(&self.seen).registered[0].clone())
    }

    /// polkit asks for `cookie`: `BeginAuthentication`, its answer
    /// recorded once it comes.
    fn begin(&mut self, cookie: &str, who: Who, message: &str) -> anyhow::Result<()> {
        let (agent, _, _, _, path) = self.registered()?;
        let identities: Vec<(&str, HashMap<&str, Variant<'static>>)> = match who {
            Who::Me => vec![("unix-user", HashMap::from([("uid", Variant::from(uid()))]))],
            Who::Admins => vec![(
                "unix-group",
                HashMap::from([("gid", Variant::from(27_u32))]),
            )],
        };
        let details: HashMap<String, String> = HashMap::from([
            ("polkit.subject-pid".to_owned(), self.pid.to_string()),
            ("polkit.caller-pid".to_owned(), self.pid.to_string()),
            (
                "program".to_owned(),
                "/usr/libexec/updates-helper".to_owned(),
            ),
        ]);
        let connection = self.authority.clone();
        let seen = self.seen.clone();
        let (cookie, message) = (cookie.to_owned(), message.to_owned());
        self.runtime.spawn(async move {
            let reply = connection
                .call_method(
                    Some(agent.as_str()),
                    path.as_str(),
                    Some(AGENT),
                    "BeginAuthentication",
                    &(
                        ACTION,
                        message.as_str(),
                        "system-software-update",
                        details,
                        cookie.as_str(),
                        identities,
                    ),
                )
                .await;
            let answer = match reply {
                Ok(_) => "ok".to_owned(),
                Err(zbus::Error::MethodError(name, _, _)) => name.to_string(),
                Err(err) => format!("{err}"),
            };
            self::seen(&seen).answers.insert(cookie, answer);
        });
        Ok(())
    }

    /// polkit cancels `cookie`'s request.
    fn cancel(&mut self, cookie: &str) -> anyhow::Result<()> {
        let (agent, _, _, _, path) = self.registered()?;
        let connection = self.authority.clone();
        let cookie = cookie.to_owned();
        self.runtime.block_on(async move {
            connection
                .call_method(
                    Some(agent.as_str()),
                    path.as_str(),
                    Some(AGENT),
                    "CancelAuthentication",
                    &(cookie.as_str(),),
                )
                .await
        })?;
        Ok(())
    }

    /// How polkit's call for `cookie` was answered, once it is.
    fn answered(&mut self, cookie: &str) -> anyhow::Result<String> {
        let seen = self.seen.clone();
        let done = self
            .hp
            .dispatch_until(WAIT, |_| self::seen(&seen).answers.contains_key(cookie))?;
        anyhow::ensure!(done, "{cookie} never answered:\n{}", self.log());
        Ok(self::seen(&self.seen).answers[cookie].clone())
    }

    fn answer_now(&self, cookie: &str) -> Option<String> {
        seen(&self.seen).answers.get(cookie).cloned()
    }

    fn log(&self) -> String {
        let text =
            String::from_utf8_lossy(&std::fs::read(&self.log).unwrap_or_default()).into_owned();
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Waits until the shell logged `what` after `from` bytes of its log.
    fn logged_after(&mut self, from: usize, what: &str) -> anyhow::Result<()> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            if self.log().get(from..).is_some_and(|l| l.contains(what)) {
                return Ok(());
            }
            self.idle(Duration::from_millis(30))?;
        }
        anyhow::bail!("the shell did not log {what:?}:\n{}", self.log())
    }

    fn logged(&mut self, what: &str) -> anyhow::Result<()> {
        self.logged_after(0, what)
    }

    /// The lines the fake helper wrote: its login and the cookie it read.
    fn helper_lines(&self) -> Vec<String> {
        std::fs::read_to_string(&self.helper_log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// The dialog, once it shows and holds the keyboard and the pointer;
    /// where it is.
    fn dialog(&mut self) -> anyhow::Result<Rect> {
        let shown = self.hp.dispatch_until(WAIT, |hp| {
            !hp.drawn_layers(AUTH_NAMESPACE).is_empty()
                && hp.auth_holds()
                && hp.keyboard_layer().as_deref() == Some(AUTH_NAMESPACE)
        })?;
        anyhow::ensure!(
            shown,
            "no dialog: {:?}\n{}",
            self.hp.layer_rects(AUTH_NAMESPACE),
            self.log()
        );
        Ok(self.hp.drawn_layers(AUTH_NAMESPACE)[0].1)
    }

    /// Waits until the dialog is gone.
    fn gone(&mut self) -> anyhow::Result<()> {
        let gone = self
            .hp
            .dispatch_until(WAIT, |hp| hp.layer_rects(AUTH_NAMESPACE).is_empty())?;
        anyhow::ensure!(gone, "the dialog stays:\n{}", self.log());
        anyhow::ensure!(!self.hp.auth_holds(), "the dialog still holds the input");
        Ok(())
    }

    fn key(&mut self, code: u32) {
        self.hp.key(code, true);
        self.hp.key(code, false);
    }

    /// `word` typed, a key each (evdev codes, AZERTY or QWERTY alike for
    /// these letters but `a`, typed as QWERTY's `a`: the test keymap is
    /// the default).
    fn type_word(&mut self, word: &str) {
        for c in word.chars() {
            let code = match c {
                'p' => 25,
                'a' => 30,
                'n' => 49,
                'd' => 32,
                'o' => 24,
                'e' => 18,
                _ => continue,
            };
            self.key(code);
        }
    }

    /// `word` typed and sent, once the field shows it typed (its length).
    fn try_password(&mut self, word: &str) -> anyhow::Result<()> {
        let from = self.log().len();
        self.type_word(word);
        self.logged_after(from, &format!("Mot de passe d'Alex Martin: {}", word.len()))?;
        self.key(KEY_ENTER);
        Ok(())
    }

    /// A window of the asking application's, which takes the keyboard and
    /// says what its pointer gets.
    fn window(&mut self) -> anyhow::Result<common::ClientHandle> {
        let (_, handle) = common::open_showing(
            &mut self.hp,
            APP,
            Shows::Pointing {
                app_id: APP,
                size: (900, 600),
            },
        )?;
        self.idle(Duration::from_millis(200))?;
        Ok(handle)
    }
}

#[test]
fn the_shell_registers_as_the_sessions_agent() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    let (sender, kind, session, locale, path) = bed.registered()?;
    assert!(sender.starts_with(':'), "{sender}");
    assert_eq!(kind, "unix-session");
    assert_eq!(session, "7");
    assert_eq!(locale, "fr_FR.UTF-8");
    assert_eq!(path, "/org/hocuspanda/PolicyKit1/AuthenticationAgent");
    // Nothing shows until polkit asks.
    bed.idle(Duration::from_millis(300))?;
    assert!(bed.hp.layer_rects(AUTH_NAMESPACE).is_empty());
    Ok(())
}

#[test]
fn the_dialog_says_who_asks_why_and_for_whom() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    let _window = bed.window()?;
    bed.begin("c1", Who::Me, MESSAGE)?;
    let rect = bed.dialog()?;
    // Centred across the screen, its top 220 px over its middle.
    assert_eq!(rect.width, 380);
    assert_eq!(rect.x, (1920 - 380) / 2);
    assert_eq!(rect.y, 1080 / 2 - 220);
    bed.logged("Autorisation nécessaire : Mises à jour")?;
    let log = bed.log();
    assert!(log.contains(MESSAGE), "{log}");
    assert!(log.contains("\"Alex Martin\", \"administrateur\""), "{log}");
    assert!(log.contains("Mot de passe d'Alex Martin: 0"), "{log}");
    assert!(log.contains("[> Détails]"), "{log}");
    assert!(log.contains("[Autoriser (inert)]"), "{log}");
    // One account: nothing to choose.
    assert!(!log.contains("[Changer]"), "{log}");
    Ok(())
}

#[test]
fn details_unfold_the_action_its_program_its_process_and_its_window() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    let _window = bed.window()?;
    bed.begin("c1", Who::Me, MESSAGE)?;
    let rect = bed.dialog()?;
    bed.logged("[> Détails]")?;
    let from = bed.log().len();
    // The field has the keyboard; Tab goes to Détails, Enter unfolds.
    bed.key(KEY_TAB);
    bed.key(KEY_ENTER);
    bed.logged_after(from, "[v Détails]")?;
    let log = bed.log();
    assert!(log.contains(&format!("Action: {ACTION}")), "{log}");
    assert!(
        log.contains("Programme: /usr/libexec/updates-helper"),
        "{log}"
    );
    assert!(
        log.contains(&format!("Processus: {}, lancé par Alex Martin", bed.pid)),
        "{log}"
    );
    assert!(log.contains("Fenêtre: "), "{log}");
    // It grew downward, its top where it was.
    let grown = bed.hp.dispatch_until(WAIT, |hp| {
        hp.drawn_layers(AUTH_NAMESPACE)
            .first()
            .is_some_and(|(_, r)| r.height > rect.height)
    })?;
    assert!(grown, "{:?}", bed.hp.layer_rects(AUTH_NAMESPACE));
    assert_eq!(bed.hp.layer_rects(AUTH_NAMESPACE)[0].1.y, rect.y);
    Ok(())
}

#[test]
fn the_right_password_authorizes() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    bed.begin("c-right", Who::Me, MESSAGE)?;
    bed.dialog()?;
    bed.try_password("panda")?;
    assert_eq!(bed.answered("c-right")?, "ok");
    bed.gone()?;
    // The helper had the login, then the cookie on its input.
    assert_eq!(bed.helper_lines(), ["alex c-right"]);
    Ok(())
}

#[test]
fn a_wrong_password_says_so_and_the_field_empties() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    bed.begin("c-wrong", Who::Me, MESSAGE)?;
    bed.dialog()?;
    let from = bed.log().len();
    bed.try_password("nope")?;
    bed.logged_after(from, "Mot de passe incorrect. Réessaie.")?;
    let log = bed.log();
    assert!(
        log[from..].contains("Mot de passe d'Alex Martin: 0"),
        "{log}"
    );
    // Still open, the keyboard its own; the helper started again.
    assert_eq!(bed.answer_now("c-wrong"), None);
    assert!(bed.hp.auth_holds());
    bed.try_password("panda")?;
    assert_eq!(bed.answered("c-wrong")?, "ok");
    assert_eq!(bed.helper_lines(), ["alex c-wrong", "alex c-wrong"]);
    Ok(())
}

#[test]
fn three_wrong_passwords_close_the_dialog_and_refuse() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    bed.begin("c-three", Who::Me, MESSAGE)?;
    bed.dialog()?;
    for _ in 0..2 {
        let from = bed.log().len();
        bed.try_password("nope")?;
        bed.logged_after(from, "Mot de passe incorrect. Réessaie.")?;
    }
    bed.try_password("nope")?;
    assert_eq!(
        bed.answered("c-three")?,
        "org.freedesktop.PolicyKit1.Error.Failed"
    );
    bed.gone()?;
    Ok(())
}

#[test]
fn escape_or_cancel_cancels() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    bed.begin("c-esc", Who::Me, MESSAGE)?;
    bed.dialog()?;
    bed.key(KEY_ESC);
    assert_eq!(
        bed.answered("c-esc")?,
        "org.freedesktop.PolicyKit1.Error.Cancelled"
    );
    bed.gone()?;
    // Annuler, clicked: its button left of Autoriser, in the footer.
    bed.begin("c-button", Who::Me, MESSAGE)?;
    let rect = bed.dialog()?;
    let at = (
        f64::from(rect.x + rect.width) - 21.0 - 100.0 - 8.0 - 40.0,
        f64::from(rect.y + rect.height) - 19.0 - 19.0,
    );
    common::click_on(&mut bed.hp, at, AUTH_NAMESPACE)?;
    assert_eq!(
        bed.answered("c-button")?,
        "org.freedesktop.PolicyKit1.Error.Cancelled"
    );
    bed.gone()?;
    Ok(())
}

#[test]
fn polkit_cancelling_closes_the_dialog() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    bed.begin("c-gone", Who::Me, MESSAGE)?;
    bed.dialog()?;
    bed.cancel("c-gone")?;
    bed.gone()?;
    assert_eq!(
        bed.answered("c-gone")?,
        "org.freedesktop.PolicyKit1.Error.Cancelled"
    );
    Ok(())
}

#[test]
fn two_requests_wait_their_turn() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    bed.begin("c-first", Who::Me, MESSAGE)?;
    bed.dialog()?;
    bed.logged(MESSAGE)?;
    let second = "L'authentification est nécessaire pour formater « Clé USB 32 Go ».";
    bed.begin("c-second", Who::Me, second)?;
    bed.logged("1 autre demande en attente")?;
    assert!(!bed.log().contains(second), "{}", bed.log());
    bed.try_password("panda")?;
    assert_eq!(bed.answered("c-first")?, "ok");
    let from = bed.log().len();
    // The second opens after the first closed, alone.
    bed.logged_after(from, second)?;
    bed.dialog()?;
    assert_eq!(bed.hp.layer_rects(AUTH_NAMESPACE).len(), 1);
    assert!(
        !bed.log()[from..].contains("autre demande"),
        "{}",
        bed.log()
    );
    assert_eq!(bed.answer_now("c-second"), None);
    bed.key(KEY_ESC);
    assert_eq!(
        bed.answered("c-second")?,
        "org.freedesktop.PolicyKit1.Error.Cancelled"
    );
    Ok(())
}

#[test]
fn two_accounts_to_choose_from() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    bed.begin("c-two", Who::Admins, MESSAGE)?;
    bed.dialog()?;
    // The session's user first, and Changer.
    bed.logged("\"Alex Martin\", \"administrateur\", \"[Changer]\"")?;
    // Shift+Tab from the field: the accounts are folded, Changer.
    let from = bed.log().len();
    bed.hp.key(42, true);
    bed.key(KEY_TAB);
    bed.hp.key(42, false);
    bed.key(KEY_ENTER);
    bed.logged_after(from, "( ) Sam Leroy")?;
    assert!(
        bed.log()[from..].contains("(x) Alex Martin"),
        "{}",
        bed.log()
    );
    // Down the list to Sam: chosen, the field empty, the helper for him.
    let from = bed.log().len();
    bed.key(KEY_TAB);
    bed.key(KEY_TAB);
    bed.key(KEY_ENTER);
    bed.logged_after(from, "\"Sam Leroy\", \"administrateur\"")?;
    bed.type_word("panda");
    bed.logged_after(from, "Mot de passe de Sam Leroy: 5")?;
    bed.key(KEY_ENTER);
    assert_eq!(bed.answered("c-two")?, "ok");
    assert_eq!(
        bed.helper_lines().last().map(String::as_str),
        Some("sam c-two")
    );
    Ok(())
}

#[test]
fn every_screen_dims_and_nothing_else_gets_the_pointer_or_the_keys() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start_on(&shell, &[(1920, 1080), (1920, 1080)], &[], true)?;
    // A window that says what its pointer gets on the first screen; one
    // that takes the keyboard on the second, which has it.
    let pointing = bed.window()?;
    let rect = common::rect_of(&bed.hp, APP).expect("the window");
    let inside = (f64::from(rect.x) + 40.0, f64::from(rect.y) + 40.0);
    bed.hp.pointer_move(1920.0 + 400.0, 600.0);
    let (_, typing) = common::open_showing(
        &mut bed.hp,
        "test.typing",
        Shows::Typing {
            app_id: "test.typing",
            size: (600, 400),
        },
    )?;
    bed.idle(Duration::from_millis(200))?;
    // Each screen's background, before.
    let shade = |hp: &mut Headless, screen: usize| -> anyhow::Result<u32> {
        let (w, _, pixels) = hp.screenshot(screen)?;
        let i = (1040 * w as usize + 60) * 4;
        Ok(u32::from(pixels[i]) + u32::from(pixels[i + 1]) + u32::from(pixels[i + 2]))
    };
    let before = [shade(&mut bed.hp, 0)?, shade(&mut bed.hp, 1)?];
    let forms = bed.hp.bars();
    bed.begin("c-dim", Who::Me, MESSAGE)?;
    let dialog = bed.dialog()?;
    // On the screen of the asking process's window (the last one).
    let screens = bed.hp.layer_rects(AUTH_NAMESPACE);
    assert_eq!(screens.len(), 1);
    assert_eq!(screens[0].0, "HEADLESS-2", "{screens:?} {dialog:?}");
    common::at_rest(&mut bed.hp)?;
    let look = bed.hp.auth_look().expect("shown");
    assert_eq!((look.alpha, look.dim), (1.0, 1.0));
    for (screen, before) in before.into_iter().enumerate() {
        // `rgba(5,7,12,0.58)` over it.
        let after = f64::from(shade(&mut bed.hp, screen)?);
        let dimmed = f64::from(before) * 0.42 + f64::from(5 + 7 + 12) * 0.58;
        assert!(
            (after - dimmed).abs() <= 4.0,
            "screen {screen}: {before} → {after}, not {dimmed}"
        );
    }
    // Neither window sees a click nor a key, and a shortcut (Super+Shift+B,
    // the bar's other form) does nothing.
    while pointing.events.try_recv().is_ok() {}
    while typing.events.try_recv().is_ok() {}
    bed.hp.pointer_move(inside.0, inside.1);
    bed.hp.button(BTN_LEFT, true);
    bed.hp.button(BTN_LEFT, false);
    bed.hp.key(KEY_LEFTMETA, true);
    bed.hp.key(KEY_LEFTSHIFT, true);
    bed.key(KEY_B);
    bed.hp.key(KEY_LEFTSHIFT, false);
    bed.hp.key(KEY_LEFTMETA, false);
    bed.key(KEY_X);
    bed.idle(Duration::from_millis(300))?;
    let got: Vec<Event> = pointing.events.try_iter().collect();
    assert!(
        !got.iter()
            .any(|e| matches!(e, Event::Pointer(Pointed::Press | Pointed::Enter))),
        "{got:?}"
    );
    let got: Vec<Event> = typing.events.try_iter().collect();
    assert!(!got.iter().any(|e| matches!(e, Event::Key(..))), "{got:?}");
    assert_eq!(bed.hp.bars(), forms);
    assert_eq!(bed.hp.keyboard_layer().as_deref(), Some(AUTH_NAMESPACE));
    // The key went to the dialog's field.
    bed.logged("Mot de passe d'Alex Martin: 1")?;
    // Cancelled, the window has its clicks again.
    bed.key(KEY_ESC);
    bed.gone()?;
    bed.hp.pointer_move(inside.0 + 1.0, inside.1);
    bed.hp.button(BTN_LEFT, true);
    bed.hp.button(BTN_LEFT, false);
    let pressed = bed.hp.dispatch_until(WAIT, |_| {
        pointing
            .events
            .try_iter()
            .any(|e| e == Event::Pointer(Pointed::Press))
    })?;
    assert!(pressed, "the window never got its click back");
    Ok(())
}

#[test]
fn at_rest_the_dialog_draws_nothing() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start(&shell)?;
    bed.begin("c-rest", Who::Me, MESSAGE)?;
    bed.dialog()?;
    common::at_rest(&mut bed.hp)?;
    bed.idle(Duration::from_millis(300))?;
    let commits = bed.hp.auth_commits();
    bed.idle(Duration::from_secs(2))?;
    assert_eq!(bed.hp.auth_commits(), commits, "the dialog drew at rest");
    assert!(bed.hp.still(), "something moves");
    Ok(())
}

#[test]
fn through_polkits_socket_the_login_then_the_cookie() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // polkit 126's socket, a listener of the test's: it reads the login
    // and the cookie, asks the password and takes "panda".
    let dir = tempfile::Builder::new().prefix("hps-").tempdir_in("/tmp")?;
    let path = dir.path().join("agent-helper.socket");
    let listener = UnixListener::bind(&path)?;
    let heard = Arc::new(Mutex::new(Vec::<String>::new()));
    let kept = heard.clone();
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let mut lines = BufReader::new(stream.try_clone().expect("a stream"));
        let mut out = stream;
        let mut read = || {
            let mut line = String::new();
            let _ = lines.read_line(&mut line);
            line.trim_end().to_owned()
        };
        let login = read();
        let cookie = read();
        let _ = out.write_all(b"PAM_TEXT_INFO Bonjour\nPAM_PROMPT_ECHO_OFF Password: \n");
        let password = read();
        let _ = out.write_all(if password == "panda" {
            b"SUCCESS\n"
        } else {
            b"FAILURE\n"
        });
        kept.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend([login, cookie]);
    });
    let mut bed = Bed::start_on(
        &shell,
        &[(1920, 1080)],
        &[("HOCUS_POLKIT_SOCKET", path.display().to_string())],
        true,
    )?;
    bed.begin("c-socket", Who::Me, MESSAGE)?;
    bed.dialog()?;
    // What PAM says shows under the field.
    bed.logged("Bonjour")?;
    bed.try_password("panda")?;
    assert_eq!(bed.answered("c-socket")?, "ok");
    assert_eq!(
        *heard.lock().unwrap_or_else(PoisonError::into_inner),
        ["alex", "c-socket"]
    );
    // The program was not started.
    assert!(bed.helper_lines().is_empty());
    Ok(())
}

/// The pull request's screenshots, at 1920 × 1080 at scale 1, the shell
/// on the CPU: the dialog, a wrong password, the details unfolded, two
/// accounts to choose from; written to `$HOCUS_SHOTS` when it names a
/// folder. Run by hand (`--run-ignored only`).
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    use anyhow::Context;
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    let Some(shell) = ready() else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let save = |bed: &mut Bed, name: &str| -> anyhow::Result<()> {
        common::at_rest(&mut bed.hp)?;
        bed.idle(Duration::from_millis(300))?;
        common::at_rest(&mut bed.hp)?;
        let (w, h, bytes) = bed.hp.screenshot(0)?;
        let rgba: Vec<u8> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(u32::try_from(w)?, u32::try_from(h)?, rgba)
            .context("a screenshot of the wrong size")?;
        image.save(out.join(format!("{name}.png")))?;
        Ok(())
    };
    let mut bed = Bed::start_on(&shell, &[(1920, 1080)], &[], false)?;
    let _window = bed.window()?;
    bed.begin("c-shot", Who::Me, MESSAGE)?;
    bed.dialog()?;
    bed.logged("[> Détails]")?;
    save(&mut bed, "dialog")?;
    let from = bed.log().len();
    bed.try_password("nope")?;
    bed.logged_after(from, "Mot de passe incorrect. Réessaie.")?;
    save(&mut bed, "wrong")?;
    let from = bed.log().len();
    bed.key(KEY_TAB);
    bed.key(KEY_ENTER);
    bed.logged_after(from, "[v Détails]")?;
    save(&mut bed, "details")?;
    bed.key(KEY_ESC);
    bed.gone()?;
    bed.begin("c-shot-2", Who::Admins, MESSAGE)?;
    bed.dialog()?;
    bed.logged("[Changer]")?;
    let from = bed.log().len();
    bed.hp.key(42, true);
    bed.key(KEY_TAB);
    bed.hp.key(42, false);
    bed.key(KEY_ENTER);
    bed.logged_after(from, "( ) Sam Leroy")?;
    save(&mut bed, "accounts")?;
    Ok(())
}

/// The dialog in its main state (one account, the field empty), for the
/// widget kit's side-by-sides (#772).
#[test]
#[ignore = "writes screenshots under HOCUS_KIT_SHOTS"]
fn kit_shot() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = Bed::start_on(&shell, &[(1920, 1080)], &[], false)?;
    let _window = bed.window()?;
    bed.begin("c-kit", Who::Me, MESSAGE)?;
    bed.dialog()?;
    bed.logged("[> Détails]")?;
    common::at_rest(&mut bed.hp)?;
    common::layer_shot(&mut bed.hp, AUTH_NAMESPACE, "auth")
}
