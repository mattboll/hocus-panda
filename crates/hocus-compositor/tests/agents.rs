//! The agents' states and their tools (docs/design/bar.md §4 group 3, 4.0;
//! issue #383): the real shell, the compositor's agents' model, the `hocus`
//! command, and the tools' files in a home of the test's own.
//!
//! The card with two tools found; "Brancher" through its consent popover
//! writing the hooks and keeping a copy; "Débrancher" putting the file
//! back; "Ne rien brancher" hiding the group, remembered, and with herdr
//! running too; a tool installed later bringing the card back for it
//! alone; no tool, no group; `hocus agent` and `hocus agent-hook` (with
//! Claude Code's documented payloads) reaching the model and the shell;
//! the agent's window found above its process; nothing drawn at rest. The
//! agents' rows (4.1), and going to an agent from its row (4.3): the
//! streak, the rim's pulse, herdr's pane.
//!
//! Pixman here, so the shell draws on the CPU, opaque and without motion:
//! as in CI. The tests of motion and of the streak run on a GPU when the
//! machine has one, and are skipped otherwise. The shell runs away from the machine's services (no session
//! bus, no system bus, no PipeWire).

mod common;

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use common::{BTN_LEFT, WAIT, wait};
use hocus_agents::tools::Places;
use hocus_compositor::{
    Config, Headless, HeadlessConfig, RendererChoice, going::Streak, model::Rect,
};
use hocus_link::ToolState;

/// What the test's machine has.
#[derive(Default)]
struct Machine {
    claude: bool,
    codex: bool,
    herdr: bool,
    /// The time the shell's clock reads as it starts (`HOCUS_CLOCK_AT`).
    clock_at: Option<&'static str>,
    /// The compositor on this render node, the shell on its GPU (else
    /// Pixman, the shell on the CPU).
    gpu: Option<PathBuf>,
    reduced_motion: bool,
}

struct Bed {
    dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    hocus: PathBuf,
    /// The fake herdr's thread runs while the test does.
    _herdr: Option<std::thread::JoinHandle<()>>,
    /// What it was asked, method and parameters, past its listing.
    herdr_heard: Heard,
}

/// The fake herdr's requests.
type Heard = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// The programs this test needs, built beside it.
fn ready() -> Option<(PathBuf, PathBuf)> {
    let Some(shell) = common::shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return None;
    };
    let exe = std::env::current_exe().ok()?;
    let hocus = exe.parent()?.parent()?.join("hocus");
    if !hocus.exists() {
        eprintln!("hocus is not built: build the workspace to run this test");
        return None;
    }
    Some((shell, hocus))
}

fn places(home: &Path, hocus: &Path) -> Places {
    Places {
        home: home.to_path_buf(),
        config_home: home.join(".config"),
        codex_home: home.join(".codex"),
        path: vec![home.join("bin")],
        herdr_socket: home.join(".config/herdr/herdr.sock"),
        hocus: hocus.to_path_buf(),
        state_dir: Some(home.join("state")),
    }
}

/// A fake herdr on `socket`: one agent, `refacto`, working in pane w1:p1,
/// whose foreground process is `pid`. What it is asked besides listing and
/// subscribing goes into `heard`.
fn fake_herdr(
    socket: &Path,
    pid: u32,
    heard: Heard,
) -> anyhow::Result<std::thread::JoinHandle<()>> {
    std::fs::create_dir_all(socket.parent().unwrap_or(Path::new("/")))?;
    let listener = UnixListener::bind(socket)?;
    Ok(std::thread::spawn(move || {
        let mut kept = Vec::new();
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            let Ok(request) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            let id = request["id"].clone();
            let result = match request["method"].as_str() {
                Some("agent.list") => serde_json::json!({ "type": "agent_list", "agents": [
                    { "pane_id": "w1:p1", "agent_status": "working", "name": "refacto", "agent": "claude" }
                ] }),
                Some("pane.process_info") => {
                    serde_json::json!({ "shell_pid": 1, "foreground": [ { "pid": pid, "name": "claude" } ] })
                }
                Some("events.subscribe") => serde_json::json!({ "type": "subscription_started" }),
                method => {
                    if let Ok(mut heard) = heard.lock() {
                        heard.push(format!("{} {}", method.unwrap_or(""), request["params"]));
                    }
                    serde_json::json!({ "type": "ok" })
                }
            };
            let answer = serde_json::json!({ "id": id, "result": result });
            let _ = (&stream).write_all(format!("{answer}\n").as_bytes());
            // The subscription stays open.
            kept.push(stream);
        }
    }))
}

fn start(shell: &Path, hocus: &Path, machine: &Machine) -> anyhow::Result<Bed> {
    let dir = tempfile::tempdir()?;
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join("bin"))?;
    if machine.claude {
        std::fs::create_dir_all(home.join(".claude"))?;
        std::fs::write(home.join(".claude/settings.json"), ORIGINAL)?;
    }
    if machine.codex {
        std::fs::create_dir_all(home.join(".codex"))?;
    }
    let heard = Heard::default();
    let herdr = if machine.herdr {
        Some((
            fake_herdr(
                &home.join(".config/herdr/herdr.sock"),
                std::process::id(),
                heard.clone(),
            )?,
            heard,
        ))
    } else {
        None
    };
    start_in(dir, shell, hocus, herdr, machine)
}

fn start_in(
    dir: tempfile::TempDir,
    shell: &Path,
    hocus: &Path,
    herdr: Option<(std::thread::JoinHandle<()>, Heard)>,
    machine: &Machine,
) -> anyhow::Result<Bed> {
    let home = dir.path().join("home");
    let log = dir
        .path()
        .join(format!("shell-{}.log", Instant::now().elapsed().as_nanos()));
    let script = dir.path().join("shell");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' {}\
             RUST_LOG=warn,hocuspanda_shell=debug\nexec '{}' \"$@\" 2>>'{}'\n",
            home.display(),
            machine
                .clock_at
                .map_or(String::new(), |at| format!("HOCUS_CLOCK_AT='{at}' ")),
            shell.display(),
            log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let mut config = Config {
        agent_places: Some(places(&home, hocus)),
        ..Config::default()
    };
    config.appearance.reduced_motion = machine.reduced_motion;
    let hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: match machine.gpu {
                Some(_) => RendererChoice::Gles,
                None => RendererChoice::Pixman,
            },
            render_node: machine.gpu.clone(),
        },
        config,
        Some(script),
    )?;
    let (herdr, herdr_heard) =
        herdr.map_or((None, Heard::default()), |(h, heard)| (Some(h), heard));
    let mut bed = Bed {
        dir,
        hp,
        log,
        hocus: hocus.to_path_buf(),
        _herdr: herdr,
        herdr_heard,
    };
    let drawn = bed.hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.agents_surfaces().len() == 1 && hp.agents_commits() > 0
    })?;
    anyhow::ensure!(drawn, "no agents' group: {:?}", bed.hp.agents_surfaces());
    bed.logged("the agents group shows it")?;
    bed.hp.pointer_move(600.0, 500.0);
    bed.settle()?;
    Ok(bed)
}

/// Claude Code's settings before the test plugs it in.
const ORIGINAL: &str = "{\n  \"model\": \"opus\"\n}\n";

fn plain(log: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default()).into_owned();
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

impl Bed {
    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    /// The commits of every surface of the shell's but the clock's.
    fn counts(&self) -> [u64; 6] {
        [
            self.hp.bar_commits(),
            self.hp.levels_commits(),
            self.hp.connections_commits(),
            self.hp.desktops_commits(),
            self.hp.agents_commits(),
            self.hp.popover_commits(),
        ]
    }

    /// Waits until none of the shell's surfaces (the clock aside) commits
    /// for a second: a slow machine's shell may still be starting.
    fn settle(&mut self) -> anyhow::Result<()> {
        let start = Instant::now();
        loop {
            let before = self.counts();
            wait(&mut self.hp, Duration::from_secs(1))?;
            if self.counts() == before {
                return Ok(());
            }
            anyhow::ensure!(start.elapsed() < 3 * WAIT, "the shell never rests");
        }
    }

    fn mark(&self) -> usize {
        plain(&self.log).len()
    }

    /// Waits until the shell logs a line with all of `texts`, past `from`.
    fn logged_after(&mut self, from: usize, texts: &[&str]) -> anyhow::Result<()> {
        let log = self.log.clone();
        let done = self.hp.dispatch_until(WAIT, |_| {
            plain(&log)
                .get(from..)
                .is_some_and(|l| l.lines().any(|line| texts.iter().all(|t| line.contains(t))))
        })?;
        let lines: Vec<String> = plain(&log)
            .get(from..)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains("agent") || l.contains("popover"))
            .map(str::to_owned)
            .collect();
        anyhow::ensure!(
            done,
            "the shell never logged {texts:?}; it logged:\n{}",
            lines.join("\n")
        );
        Ok(())
    }

    fn logged(&mut self, text: &str) -> anyhow::Result<()> {
        self.logged_after(0, &[text])
    }

    fn surface(&self) -> Rect {
        self.hp
            .agents_surfaces()
            .first()
            .map_or(Rect::new(0, 0, 0, 0), |(_, r)| *r)
    }

    /// The group's top left, global: 11 px in from the surface's left
    /// (the bar's padding inside its rim), 5 px down (the ring's room).
    fn origin(&self) -> (f64, f64) {
        let s = self.surface();
        (f64::from(s.x) + 11.0, f64::from(s.y) + 5.0)
    }

    /// Where the group's `item` is (`Sources`, `Plug(ClaudeCode)`,
    /// `Decline`…), global, as the shell last logged its items' middles
    /// in the bar's frame (from its outer right edge, its top).
    fn item(&mut self, item: &str) -> anyhow::Result<(f64, f64)> {
        let wanted = format!("{item}@");
        let log = self.log.clone();
        let found = |text: &str| {
            text.lines()
                .rev()
                .find(|l| l.contains("the agents group's items"))
                .and_then(|l| {
                    let at = l.split(&wanted).nth(1)?;
                    let at = at.split_whitespace().next()?;
                    let (x, y) = at.split_once(',')?;
                    Some((x.parse::<f64>().ok()?, y.parse::<f64>().ok()?))
                })
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| found(&plain(&log)).is_some())?;
        anyhow::ensure!(done, "no {item} in the group");
        let (x, y) = found(&plain(&log)).unwrap_or_default();
        let right = f64::from(self.hp.outputs()[0].1.width) - 8.0;
        let at = (right + x, 8.0 + y);
        // The surface as the compositor has it reaches there: the shell
        // logs as it draws, before the compositor takes its commit.
        let inside = |r: &Rect| {
            at.0 >= f64::from(r.x)
                && at.1 >= f64::from(r.y)
                && at.0 < f64::from(r.x + r.width)
                && at.1 < f64::from(r.y + r.height)
        };
        let reached = self.hp.dispatch_until(WAIT, |hp| {
            hp.agents_surfaces().first().is_some_and(|(_, r)| inside(r))
        })?;
        anyhow::ensure!(reached, "{item} at {at:?} is off {:?}", self.surface());
        Ok(at)
    }

    /// Waits until the group's surface is one pixel high: it shows
    /// nothing.
    fn one_pixel(&mut self) -> anyhow::Result<()> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            hp.agents_surfaces()
                .first()
                .is_some_and(|(_, r)| r.height == 1)
        })?;
        anyhow::ensure!(done, "the group still shows: {:?}", self.surface());
        Ok(())
    }

    fn click(&mut self, (x, y): (f64, f64)) {
        self.hp.pointer_move(x - 1.0, y);
        self.hp.pointer_move(x, y);
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
    }

    /// The popover, once it shows.
    fn popover(&mut self) -> anyhow::Result<Rect> {
        let shown = self.hp.dispatch_until(WAIT, |hp| {
            hp.popover_surfaces()
                .first()
                .is_some_and(|(_, r)| r.height > 0)
        })?;
        anyhow::ensure!(shown, "no popover");
        Ok(self.hp.popover_surfaces()[0].1)
    }

    /// The tools as the shell is told, once `state` of `tool`.
    fn tool_becomes(&mut self, tool: &str, state: ToolState) -> anyhow::Result<()> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            hp.agent_tools()
                .iter()
                .any(|t| t.tool == tool && t.state == state)
        })?;
        anyhow::ensure!(done, "{tool} not {state:?}: {:?}", self.hp.agent_tools());
        Ok(())
    }

    /// Runs `hocus` with `args`, `stdin` on its input.
    fn hocus(&self, args: &[&str], stdin: Option<&str>) -> anyhow::Result<()> {
        let socket = self
            .hp
            .control_socket()
            .ok_or_else(|| anyhow::anyhow!("no control socket"))?
            .to_path_buf();
        let mut child = Command::new(&self.hocus)
            .arg("--socket")
            .arg(socket)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        if let Some(text) = stdin
            && let Some(mut input) = child.stdin.take()
        {
            input.write_all(text.as_bytes())?;
        }
        drop(child.stdin.take());
        let out = child.wait_with_output()?;
        anyhow::ensure!(out.status.success(), "hocus {args:?}: {}", out.status);
        anyhow::ensure!(out.stdout.is_empty(), "hocus printed {:?}", out.stdout);
        Ok(())
    }

    /// Waits until the model has an agent named `name` in `state` (none:
    /// no such agent).
    fn agent_becomes(&mut self, name: &str, state: Option<&str>) -> anyhow::Result<()> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            let agents = hp.agents_peek();
            match state {
                Some(state) => agents.iter().any(|a| a.name == name && a.state == state),
                None => !agents.iter().any(|a| a.name == name),
            }
        })?;
        anyhow::ensure!(done, "{name} not {state:?}: {:?}", self.hp.agents());
        Ok(())
    }
}

#[test]
fn two_tools_found_show_the_card() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            claude: true,
            codex: true,
            herdr: false,
            ..Machine::default()
        },
    )?;
    bed.logged("shows=Pour suivre tes agents, branche ton outil\u{a0}: Claude Code, Codex")?;
    let s = bed.surface();
    // Under the clock, 100 px under the bar's top; the full form's width.
    assert_eq!((s.y, s.width), (8 + 95, 194));
    // Sources says what each is.
    let sources = bed.item("Sources")?;
    bed.click(sources);
    let popover = bed.popover()?;
    assert_eq!(popover.width, 300);
    assert_eq!(popover.y, 8 + 62);
    bed.logged("Trouvé, pas branché")?;
    Ok(())
}

#[test]
fn plugging_in_writes_the_hooks_after_the_click_and_keeps_a_copy() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            claude: true,
            codex: true,
            herdr: false,
            ..Machine::default()
        },
    )?;
    let settings = bed.home().join(".claude/settings.json");
    // "Brancher" opens the consent popover; nothing is written yet.
    let from = bed.mark();
    let plug = bed.item("Plug(ClaudeCode)")?;
    bed.click(plug);
    let popover = bed.popover()?;
    bed.logged_after(from, &["the agents' popover shows it"])?;
    assert_eq!(std::fs::read_to_string(&settings)?, ORIGINAL);
    // Its "Brancher", at its bottom right.
    let confirm = (
        f64::from(popover.x + popover.width - 15 - 6),
        f64::from(popover.y + popover.height - 15 - 15),
    );
    bed.click(confirm);
    bed.tool_becomes("claude-code", ToolState::Plugged)?;
    let text = std::fs::read_to_string(&settings)?;
    assert!(
        text.contains(&format!("{} agent-hook claude-code", hocus.display())),
        "{text}"
    );
    assert!(text.contains("\"model\": \"opus\""));
    assert_eq!(
        std::fs::read_to_string(bed.home().join(".claude/settings.json.before-hocuspanda"))?,
        ORIGINAL
    );
    // The card is Codex's alone now.
    bed.logged("shows=Pour suivre tes agents, branche ton outil\u{a0}: Codex")?;
    Ok(())
}

#[test]
fn unplugging_puts_the_file_back_as_it_was() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            claude: true,
            codex: false,
            herdr: false,
            ..Machine::default()
        },
    )?;
    let settings = bed.home().join(".claude/settings.json");
    let plug = bed.item("Plug(ClaudeCode)")?;
    bed.click(plug);
    let popover = bed.popover()?;
    bed.click((
        f64::from(popover.x + popover.width - 21),
        f64::from(popover.y + popover.height - 30),
    ));
    bed.tool_becomes("claude-code", ToolState::Plugged)?;
    // Plugged, nothing running: "Aucun agent en cours."
    bed.logged("shows=Aucun agent en cours. Suivis\u{a0}: Claude Code.")?;
    bed.settle()?;
    // Sources' first row: "Débrancher".
    let sources = bed.item("Sources")?;
    bed.click(sources);
    let popover = bed.popover()?;
    bed.logged("Branché")?;
    bed.click((f64::from(popover.x + 270), f64::from(popover.y + 51 + 20)));
    bed.tool_becomes("claude-code", ToolState::Found)?;
    assert_eq!(std::fs::read_to_string(&settings)?, ORIGINAL);
    assert!(
        !bed.home()
            .join(".claude/settings.json.before-hocuspanda")
            .exists()
    );
    Ok(())
}

#[test]
fn plugging_nothing_in_hides_the_group_and_is_remembered() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            claude: true,
            codex: true,
            herdr: false,
            ..Machine::default()
        },
    )?;
    let from = bed.mark();
    let decline = bed.item("Decline")?;
    bed.click(decline);
    bed.tool_becomes("codex", ToolState::Declined)?;
    bed.logged_after(from, &["shows=nothing"])?;
    // One pixel, letting the pointer through.
    bed.one_pixel()?;
    // Nothing was written into the tools' settings.
    assert_eq!(
        std::fs::read_to_string(bed.home().join(".claude/settings.json"))?,
        ORIGINAL
    );
    // A new session: still hidden.
    let Bed { dir, hp, .. } = bed;
    drop(hp);
    let mut again = start_in(dir, &shell, &hocus, None, &Machine::default())?;
    again.tool_becomes("claude-code", ToolState::Declined)?;
    again.one_pixel()?;
    Ok(())
}

#[test]
fn plugging_nothing_in_with_herdr_running_hides_the_group_too() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            claude: true,
            codex: false,
            herdr: true,
            ..Machine::default()
        },
    )?;
    bed.tool_becomes("herdr", ToolState::Running)?;
    // herdr's agent is followed, and the card proposes Claude Code.
    let done = bed.hp.dispatch_until(WAIT, |hp| {
        hp.agents_peek()
            .iter()
            .any(|a| a.name == "refacto" && a.source == "herdr")
    })?;
    anyhow::ensure!(done, "{:?}", bed.hp.agents());
    let from = bed.mark();
    let decline = bed.item("Decline")?;
    bed.click(decline);
    bed.tool_becomes("herdr", ToolState::Ignored)?;
    bed.logged_after(from, &["shows=nothing"])?;
    assert!(bed.hp.agents().is_empty(), "{:?}", bed.hp.agents());
    bed.one_pixel()?;
    Ok(())
}

#[test]
fn a_declined_herdr_is_followed_again_from_sources() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            claude: true,
            codex: true,
            herdr: true,
            ..Machine::default()
        },
    )?;
    bed.tool_becomes("herdr", ToolState::Running)?;
    // Claude Code plugged in, then the rest declined, herdr with it.
    let plug = bed.item("Plug(ClaudeCode)")?;
    bed.click(plug);
    let popover = bed.popover()?;
    bed.click((
        f64::from(popover.x + popover.width - 21),
        f64::from(popover.y + popover.height - 30),
    ));
    bed.tool_becomes("claude-code", ToolState::Plugged)?;
    bed.settle()?;
    let decline = bed.item("Decline")?;
    bed.click(decline);
    bed.tool_becomes("herdr", ToolState::Ignored)?;
    bed.logged("shows=Aucun agent en cours. Suivis\u{a0}: Claude Code.")?;
    bed.settle()?;
    // Sources: herdr's row, the third, says so and offers "Suivre".
    let sources = bed.item("Sources")?;
    bed.click(sources);
    let popover = bed.popover()?;
    bed.logged("En marche · pas suivi")?;
    bed.click((
        f64::from(popover.x + 270),
        f64::from(popover.y + 51 + 2 * 46 + 20),
    ));
    bed.tool_becomes("herdr", ToolState::Running)?;
    bed.logged("En marche · suivi tout seul")?;
    let done = bed.hp.dispatch_until(WAIT, |hp| {
        hp.agents_peek()
            .iter()
            .any(|a| a.name == "refacto" && a.source == "herdr")
    })?;
    anyhow::ensure!(done, "herdr's agent not followed: {:?}", bed.hp.agents());
    Ok(())
}

#[test]
fn a_tool_installed_later_brings_the_card_back_for_it_alone() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            claude: true,
            codex: false,
            herdr: false,
            ..Machine::default()
        },
    )?;
    let decline = bed.item("Decline")?;
    bed.click(decline);
    bed.tool_becomes("claude-code", ToolState::Declined)?;
    let from = bed.mark();
    // Codex installed: its program appears in PATH.
    let program = bed.home().join("bin/codex");
    std::fs::write(&program, "#!/bin/sh\n")?;
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))?;
    bed.tool_becomes("codex", ToolState::Found)?;
    bed.logged_after(
        from,
        &["shows=Pour suivre tes agents, branche ton outil\u{a0}: Codex"],
    )?;
    Ok(())
}

#[test]
fn no_tool_no_group() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, &hocus, &Machine::default())?;
    bed.logged("shows=nothing")?;
    bed.one_pixel()?;
    // The pointer goes through to what is under it.
    let (x, y) = bed.origin();
    bed.hp.pointer_move(x + 20.0, y + 10.0);
    wait(&mut bed.hp, Duration::from_millis(100))?;
    assert_ne!(
        bed.hp.pointer_layer().as_deref(),
        Some(hocus_link::AGENTS_NAMESPACE)
    );
    Ok(())
}

#[test]
fn the_commands_states_reach_the_model_and_the_shell() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, &hocus, &Machine::default())?;
    let from = bed.mark();
    bed.hocus(&["agent", "build", "working"], None)?;
    bed.agent_becomes("build", Some("working"))?;
    // An agent runs: its row.
    bed.logged_after(from, &["shows=AGENTS 1 · build 0\u{a0}min", "agents=1"])?;
    bed.hocus(&["agent", "build", "waiting"], None)?;
    bed.agent_becomes("build", Some("waiting"))?;
    bed.hocus(&["agent", "build", "ended"], None)?;
    bed.agent_becomes("build", None)?;
    // A state that is not one is refused.
    let socket = bed
        .hp
        .control_socket()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let status = Command::new(&bed.hocus)
        .arg("--socket")
        .arg(socket)
        .args(["agent", "build", "sleeping"])
        .stderr(Stdio::null())
        .status()?;
    assert!(!status.success());
    Ok(())
}

#[test]
fn claude_codes_hooks_reach_the_model() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, &hocus, &Machine::default())?;
    // The payloads of code.claude.com/docs/en/hooks.
    let hook = |event: &str, extra: &str| {
        format!(
            "{{\"session_id\":\"abc123\",\"transcript_path\":\"/x.jsonl\",\
             \"cwd\":\"/home/alex/hocus-panda\",\"hook_event_name\":\"{event}\"{extra}}}"
        )
    };
    bed.hocus(
        &["agent-hook", "claude-code"],
        Some(&hook("SessionStart", ",\"source\":\"startup\"")),
    )?;
    bed.agent_becomes("hocus-panda", Some("idle"))?;
    bed.hocus(
        &["agent-hook", "claude-code"],
        Some(&hook("UserPromptSubmit", ",\"prompt\":\"go\"")),
    )?;
    bed.agent_becomes("hocus-panda", Some("working"))?;
    bed.hocus(
        &["agent-hook", "claude-code"],
        Some(&hook(
            "PermissionRequest",
            ",\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"rm -rf node_modules\"}",
        )),
    )?;
    bed.agent_becomes("hocus-panda", Some("waiting"))?;
    bed.hocus(
        &["agent-hook", "claude-code"],
        Some(&hook("Stop", ",\"stop_hook_active\":false")),
    )?;
    bed.agent_becomes("hocus-panda", Some("done"))?;
    let agents = bed.hp.agents();
    assert_eq!(agents.len(), 1, "{agents:?}");
    assert_eq!(agents[0].source, "claude-code");
    bed.hocus(
        &["agent-hook", "claude-code"],
        Some(&hook("SessionEnd", ",\"reason\":\"other\"")),
    )?;
    bed.agent_becomes("hocus-panda", None)?;
    // Codex's notify: its JSON as an argument.
    bed.hocus(
        &[
            "agent-hook",
            "codex",
            "{\"type\":\"agent-turn-complete\",\"thread-id\":\"t1\",\"cwd\":\"/w/infra\"}",
        ],
        None,
    )?;
    bed.agent_becomes("infra", Some("done"))?;
    // Not a payload: nothing, quietly.
    bed.hocus(&["agent-hook", "claude-code"], Some("hello"))?;
    Ok(())
}

#[test]
fn the_agents_window_is_found_above_its_process() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, &hocus, &Machine::default())?;
    // A window of this process's, as a terminal's is its agent's
    // ancestor: `hocus` runs as this process's child.
    let (_, _client) = common::open_with(&mut bed.hp, "test.terminal", (600, 400), false)?;
    let window = bed
        .hp
        .windows()
        .iter()
        .find(|w| w.app_id.as_deref() == Some("test.terminal"))
        .map(|w| w.id)
        .ok_or_else(|| anyhow::anyhow!("no window"))?;
    bed.hocus(&["agent", "refacto", "working"], None)?;
    bed.agent_becomes("refacto", Some("working"))?;
    let agents = bed.hp.agents();
    assert_eq!(agents[0].window, window.0, "{agents:?}");
    // Another process's: none.
    bed.hocus(&["agent", "elsewhere", "idle", "--pid", "1"], None)?;
    bed.agent_becomes("elsewhere", Some("idle"))?;
    let agents = bed.hp.agents();
    let elsewhere = agents.iter().find(|a| a.name == "elsewhere");
    assert_eq!(elsewhere.map(|a| a.window), Some(0), "{agents:?}");
    Ok(())
}

#[test]
fn nothing_commits_at_rest() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            claude: true,
            codex: true,
            herdr: false,
            ..Machine::default()
        },
    )?;
    let from = bed.mark();
    bed.hocus(&["agent", "build", "working"], None)?;
    bed.agent_becomes("build", Some("working"))?;
    // The shell has it, and is done drawing it.
    bed.logged_after(from, &["shows=Pour suivre", "agents=1"])?;
    bed.settle()?;
    let before = bed.counts();
    // The same state again, and time passing: nothing.
    for _ in 0..5 {
        bed.hocus(&["agent", "build", "working"], None)?;
    }
    wait(&mut bed.hp, Duration::from_secs(2))?;
    assert_eq!(before, bed.counts());
    Ok(())
}

// --- 4.1: one row per agent ----------------------------------------------------

impl Bed {
    /// The id the compositor gives agent `name`.
    fn id_of(&self, name: &str) -> anyhow::Result<u64> {
        self.hp
            .agents_peek()
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.id)
            .ok_or_else(|| anyhow::anyhow!("no agent {name}: {:?}", self.hp.agents_peek()))
    }

    /// The middle of agent `name`'s row, global.
    fn row(&mut self, name: &str) -> anyhow::Result<(f64, f64)> {
        let id = self.id_of(name)?;
        self.item(&format!("Agent({id})"))
    }

    /// The window with the keyboard, by its app id.
    fn keyboard_on(&self) -> Option<String> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.focused && w.keyboard)
            .and_then(|w| w.app_id)
    }

    /// The agents' surface's commits, and when each came, for `time`.
    fn commits_for(&mut self, time: Duration) -> anyhow::Result<Vec<Duration>> {
        let start = Instant::now();
        let mut seen = self.hp.agents_commits();
        let mut at = Vec::new();
        while start.elapsed() < time {
            self.hp.dispatch(Duration::from_millis(2))?;
            let now = self.hp.agents_commits();
            if now != seen {
                seen = now;
                at.push(start.elapsed());
            }
        }
        Ok(at)
    }
}

/// The pixel at (`x`, `y`) of output 0, as R, G, B.
fn pixel(hp: &mut Headless, (x, y): (f64, f64)) -> anyhow::Result<[i32; 3]> {
    let (width, _, bytes) = hp.screenshot(0)?;
    let at = ((y as i32 * width + x as i32) * 4) as usize;
    let p = bytes
        .get(at..at + 4)
        .ok_or_else(|| anyhow::anyhow!("({x}, {y}) is off the screen"))?;
    Ok([i32::from(p[2]), i32::from(p[1]), i32::from(p[0])])
}

#[test]
fn the_rows_say_each_agent_in_order_with_its_colour() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, &hocus, &Machine::default())?;
    let from = bed.mark();
    for (name, state) in [
        ("build", "working"),
        ("notes", "idle"),
        ("site", "failed"),
        ("api", "done"),
        ("infra", "waiting"),
    ] {
        bed.hocus(&["agent", name, state], None)?;
        bed.agent_becomes(name, Some(state))?;
    }
    // What concerns the user first: waiting, stuck, to see, working,
    // resting.
    bed.logged_after(
        from,
        &["shows=AGENTS 5 · infra t'attend · site bloqué · api à voir · build 0\u{a0}min · notes repos"],
    )?;
    let rows: Vec<(f64, f64)> = ["infra", "site", "api", "build", "notes"]
        .into_iter()
        .map(|name| bed.row(name))
        .collect::<anyhow::Result<_>>()?;
    assert!(
        rows.windows(2)
            .all(|p| (p[1].1 - p[0].1 - 28.0).abs() < 0.6),
        "28 px apart: {rows:?}"
    );
    bed.settle()?;
    // Each row's tint, 3 px into it (left of its panda), against the
    // working one's, which has none.
    let tints: Vec<[i32; 3]> = rows
        .iter()
        .map(|&(x, y)| pixel(&mut bed.hp, (x - 92.0 + 3.0, y)))
        .collect::<anyhow::Result<_>>()?;
    let plain = tints[3];
    let lift = |t: [i32; 3]| [t[0] - plain[0], t[1] - plain[1], t[2] - plain[2]];
    let [waiting, stuck, to_see, resting] = [
        lift(tints[0]),
        lift(tints[1]),
        lift(tints[2]),
        lift(tints[4]),
    ];
    assert!(waiting[0] > 12 && waiting[1] > 8, "amber: {tints:?}");
    assert!(
        stuck[0] > 12 && stuck[0] > stuck[1] + 5,
        "orange: {tints:?}"
    );
    assert!(to_see[1] > 8 && to_see[1] > to_see[0], "green: {tints:?}");
    assert!(resting.iter().all(|d| d.abs() <= 2), "none: {tints:?}");
    Ok(())
}

#[test]
fn more_than_five_agents_fold_into_a_line() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, &hocus, &Machine::default())?;
    let from = bed.mark();
    for (name, state) in [
        ("w1", "working"),
        ("w2", "working"),
        ("w3", "working"),
        ("w4", "working"),
        ("w5", "working"),
        ("r1", "idle"),
        ("r2", "idle"),
    ] {
        bed.hocus(&["agent", name, state], None)?;
        bed.agent_becomes(name, Some(state))?;
    }
    // The two resting ones under the line.
    bed.logged_after(
        from,
        &[
            "shows=AGENTS 7 · w1 ",
            "· w5 0\u{a0}min · +\u{a0}2 au repos",
        ],
    )?;
    // Five rows and the line: 26 + 5 × 28 + 18, and the ring's room.
    let tall = bed.hp.dispatch_until(WAIT, |hp| {
        hp.agents_surfaces()
            .first()
            .is_some_and(|(_, r)| r.height == 26 + 5 * 28 + 18 + 10)
    })?;
    anyhow::ensure!(tall, "{:?}", bed.surface());
    // One of them works now: not all the hidden ones rest.
    let from = bed.mark();
    bed.hocus(&["agent", "r1", "working"], None)?;
    bed.logged_after(from, &["· +\u{a0}2 autres"])?;
    Ok(())
}

#[test]
fn a_finished_agent_stays_to_see_until_its_window_gets_the_keyboard() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, &hocus, &Machine::default())?;
    // Its terminal: a window of this process's, which runs `hocus`.
    let (_, _client) = common::open_with(&mut bed.hp, "test.terminal", (600, 400), false)?;
    // On another desktop, empty: no window has the keyboard.
    common::chord(&mut bed.hp, &[common::KEY_LEFTMETA, common::KEY_2]);
    let away = bed.hp.dispatch_until(WAIT, |hp| hp.desktop() == (1, 1.0))?;
    anyhow::ensure!(away, "{:?}", bed.hp.desktop());
    bed.hocus(&["agent", "build", "working"], None)?;
    bed.hocus(&["agent", "build", "done"], None)?;
    bed.agent_becomes("build", Some("done"))?;
    assert_ne!(bed.hp.agents_peek()[0].window, 0);
    bed.logged("build à voir")?;
    // It stays so.
    wait(&mut bed.hp, Duration::from_secs(1))?;
    assert_eq!(bed.hp.agents_peek()[0].state, "done");
    // Back on its desktop, its window has the keyboard: seen, it rests.
    let from = bed.mark();
    common::chord(&mut bed.hp, &[common::KEY_LEFTMETA, common::KEY_1]);
    bed.agent_becomes("build", Some("seen"))?;
    assert_eq!(bed.keyboard_on().as_deref(), Some("test.terminal"));
    bed.logged_after(from, &["shows=AGENTS 1 · build repos"])?;
    // Finishing again while its window has the keyboard: seen at once.
    bed.hocus(&["agent", "build", "working"], None)?;
    bed.agent_becomes("build", Some("working"))?;
    bed.hocus(&["agent", "build", "done"], None)?;
    bed.agent_becomes("build", Some("seen"))?;
    Ok(())
}

#[test]
fn a_rows_click_goes_to_the_agents_window_on_its_desktop() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, &hocus, &Machine::default())?;
    let (_, _client) = common::open_with(&mut bed.hp, "test.terminal", (600, 400), false)?;
    common::chord(&mut bed.hp, &[common::KEY_LEFTMETA, common::KEY_2]);
    let away = bed.hp.dispatch_until(WAIT, |hp| hp.desktop() == (1, 1.0))?;
    anyhow::ensure!(away, "{:?}", bed.hp.desktop());
    bed.hocus(&["agent", "build", "done"], None)?;
    bed.agent_becomes("build", Some("done"))?;
    bed.settle()?;
    let row = bed.row("build")?;
    bed.click(row);
    // Its desktop, its window with the keyboard, and it is seen.
    let there = bed.hp.dispatch_until(WAIT, |hp| hp.desktop() == (0, 0.0))?;
    anyhow::ensure!(there, "{:?}", bed.hp.desktop());
    let focused = bed.hp.dispatch_until(WAIT, |hp| {
        hp.windows()
            .iter()
            .any(|w| w.focused && w.keyboard && w.app_id.as_deref() == Some("test.terminal"))
    })?;
    anyhow::ensure!(focused, "{:?}", bed.keyboard_on());
    bed.agent_becomes("build", Some("seen"))?;
    Ok(())
}

#[test]
fn a_working_agents_minutes_go_on_with_the_clock_and_nothing_draws_in_between() -> anyhow::Result<()>
{
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    // The shell's clock reaches 10:00 some 15 s after it starts.
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            clock_at: Some("09:59:45"),
            ..Machine::default()
        },
    )?;
    bed.hocus(&["agent", "build", "working"], None)?;
    bed.agent_becomes("build", Some("working"))?;
    // As if it had worked 57 s: "0 min" until the clock's next minute,
    // when it has worked more than a minute.
    let from = bed.mark();
    bed.hp.backdate_agents(Duration::from_secs(57));
    bed.logged_after(from, &["shows=AGENTS 1 · build 0\u{a0}min"])?;
    bed.settle()?;
    let (agents, clock) = (bed.hp.agents_commits(), bed.hp.clock_commits());
    let from = bed.mark();
    let log = bed.log.clone();
    let went_on = bed.hp.dispatch_until(Duration::from_secs(25), |_| {
        plain(&log).get(from..).is_some_and(|l| {
            l.lines()
                .any(|l| l.contains("the agents' minutes go on") && l.contains("build 1\u{a0}min"))
        })
    })?;
    anyhow::ensure!(went_on, "the minutes never went on");
    wait(&mut bed.hp, Duration::from_secs(1))?;
    // The clock drew every second; the row once, for its new minute.
    let ticks = bed.hp.clock_commits() - clock;
    let drawn = bed.hp.agents_commits() - agents;
    assert!(ticks >= 3, "{ticks} clock commits");
    assert!(
        (1..=2).contains(&drawn),
        "{drawn} commits of the agents for {ticks} seconds"
    );
    Ok(())
}

/// The commits closer together than a second's ticks: the motion's
/// frames, and how long they last.
fn motion_of(commits: &[Duration]) -> (usize, Duration) {
    let close: Vec<Duration> = commits
        .windows(2)
        .filter(|p| p[1] - p[0] < Duration::from_millis(100))
        .map(|p| p[1])
        .collect();
    let span = match (close.first(), close.last()) {
        (Some(a), Some(b)) => *b - *a,
        _ => Duration::ZERO,
    };
    (close.len(), span)
}

/// On a GPU: a change slides the rows and hops the changed agent's panda,
/// for 650 ms, then nothing moves; with reduced motion, it changes at
/// once. Skipped without a render node.
#[test]
fn a_change_slides_and_hops_then_rests() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let node = PathBuf::from("/dev/dri/renderD128");
    if !node.exists() {
        eprintln!("no render node: the shell would draw on the CPU, without motion");
        return Ok(());
    }
    for (reduced_motion, moves) in [(false, true), (true, false)] {
        let mut bed = start(
            &shell,
            &hocus,
            &Machine {
                gpu: Some(node.clone()),
                reduced_motion,
                ..Machine::default()
            },
        )?;
        if bed.hp.shell_on_cpu() {
            eprintln!("the shell draws on the CPU here: no motion to see");
            return Ok(());
        }
        for name in ["a", "b", "c"] {
            bed.hocus(&["agent", name, "working"], None)?;
            bed.agent_becomes(name, Some("working"))?;
        }
        bed.settle()?;
        // c waits: to the top, a and b sliding down, c's panda hopping.
        let from = bed.mark();
        bed.hocus(&["agent", "c", "waiting"], None)?;
        let commits = bed.commits_for(Duration::from_secs(2))?;
        bed.logged_after(from, &["shows=AGENTS 3 · c t'attend · a 0\u{a0}min"])?;
        let (frames, span) = motion_of(&commits);
        if moves {
            bed.logged_after(from, &["the agents' rows move"])?;
            assert!(frames >= 10, "{frames} frames of motion: {commits:?}");
            assert!(
                (Duration::from_millis(500)..Duration::from_millis(900)).contains(&span),
                "{span:?}: {commits:?}"
            );
        } else {
            assert_eq!(frames, 0, "{commits:?}");
        }
        // Then it rests.
        let last = commits.last().copied().unwrap_or_default();
        assert!(last < Duration::from_millis(1200), "{commits:?}");
    }
    Ok(())
}

// --- 4.3: going to an agent -----------------------------------------------------

/// The render node for the tests that need the compositor on a GPU (the
/// streak is a shader's); `None`, and the test is skipped, without one.
fn render_node() -> Option<PathBuf> {
    let node = PathBuf::from("/dev/dri/renderD128");
    if node.exists() {
        Some(node)
    } else {
        eprintln!("no render node: the compositor would draw on the CPU, without the streak");
        None
    }
}

/// What going to an agent shows at a frame.
#[derive(Debug, Clone, Copy)]
struct Sample {
    /// Since the click.
    at: Duration,
    streak: Option<Streak>,
    /// The window's rim and ring, as they pulse.
    landing: Option<(f32, Option<f64>)>,
    /// The desktop shown, and where the strip is.
    strip: (usize, f64),
}

impl Bed {
    /// Clicks agent `name`'s row, then samples each frame for `time`, the
    /// pulse being `app_id`'s.
    fn go_to(&mut self, name: &str, app_id: &str, time: Duration) -> anyhow::Result<Vec<Sample>> {
        let row = self.row(name)?;
        let start = Instant::now();
        self.click(row);
        let mut frames = self.hp.frames();
        let mut out = Vec::new();
        while start.elapsed() < time {
            self.hp.dispatch(Duration::from_millis(2))?;
            let now = self.hp.frames();
            if now != frames {
                frames = now;
                out.push(Sample {
                    at: start.elapsed(),
                    streak: self.hp.streak(),
                    landing: self.hp.landing(app_id),
                    strip: self.hp.desktop(),
                });
            }
        }
        Ok(out)
    }

    /// The centre of the window of `app_id`, global.
    fn centre(&self, app_id: &str) -> anyhow::Result<(f64, f64)> {
        let rect = self
            .hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(app_id))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no {app_id}"))?;
        Ok((
            f64::from(rect.x) + f64::from(rect.width) / 2.0,
            f64::from(rect.y) + f64::from(rect.height) / 2.0,
        ))
    }

    /// Waits until the window of `app_id` has the keyboard.
    fn keyboard_goes_to(&mut self, app_id: &str) -> anyhow::Result<()> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            hp.windows()
                .iter()
                .any(|w| w.focused && w.keyboard && w.app_id.as_deref() == Some(app_id))
        })?;
        anyhow::ensure!(done, "the keyboard is on {:?}", self.keyboard_on());
        Ok(())
    }
}

/// The streak's samples, the time it showed (from its first frame to its
/// last), and the pulse's.
fn streaked(samples: &[Sample]) -> (Vec<Sample>, Duration) {
    let shown: Vec<Sample> = samples
        .iter()
        .filter(|s| s.streak.is_some())
        .copied()
        .collect();
    let span = match (shown.first(), shown.last()) {
        (Some(a), Some(b)) => b.at - a.at,
        _ => Duration::ZERO,
    };
    (shown, span)
}

fn near((x0, y0): (f64, f64), (x1, y1): (f64, f64), by: f64) -> bool {
    (x0 - x1).abs() <= by && (y0 - y1).abs() <= by
}

/// On a GPU: a row's click on the same desktop runs the streak from the
/// row's left edge to the window's centre in the row's colour, for some
/// 520 ms; the window has the keyboard, and its rim pulses with the ring
/// as the streak lands; then nothing is drawn.
#[test]
fn going_to_an_agent_runs_the_streak_then_pulses_its_rim_then_rests() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let Some(node) = render_node() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            gpu: Some(node),
            ..Machine::default()
        },
    )?;
    if bed.hp.renderer() != "gles" {
        eprintln!("the compositor draws on the CPU here: no streak");
        return Ok(());
    }
    let (_, _client) = common::open_with(&mut bed.hp, "test.terminal", (600, 400), false)?;
    bed.hocus(&["agent", "build", "working"], None)?;
    bed.agent_becomes("build", Some("working"))?;
    bed.settle()?;
    let row = bed.row("build")?;
    let centre = bed.centre("test.terminal")?;
    let samples = bed.go_to("build", "test.terminal", Duration::from_millis(1600))?;
    let (shown, span) = streaked(&samples);
    let (Some(first), Some(last)) = (shown.first(), shown.last()) else {
        anyhow::bail!("no streak: {samples:?}");
    };
    let (first_streak, last_streak) = (
        first.streak.unwrap_or_default(),
        last.streak.unwrap_or_default(),
    );
    let (from, head, colour) = (first_streak.from, first_streak.head, first_streak.colour);
    let landed = last_streak.head;
    // From the row's left edge (184 px wide), at its middle; the working
    // row's colour, the accent; to the window's centre.
    assert!(
        near(from, (row.0 - 92.0, row.1), 1.0),
        "{from:?} for {row:?}"
    );
    assert_eq!(colour, [0x6C, 0xC2, 0x87]);
    assert!(
        near(head, from, 300.0) && !near(head, centre, 10.0),
        "{head:?}"
    );
    assert!(near(landed, centre, 0.5), "{landed:?} for {centre:?}");
    assert!(
        (Duration::from_millis(420)..Duration::from_millis(650)).contains(&span),
        "{span:?}: {shown:?}"
    );
    // The rim lights once the streak is there, the ring growing.
    let pulsing: Vec<&Sample> = samples
        .iter()
        .filter(|s| s.landing.is_some_and(|(rim, _)| rim > 0.0))
        .collect();
    let lit = pulsing.first().map_or(Duration::MAX, |s| s.at);
    assert!(lit >= first.at + Duration::from_millis(250), "{samples:?}");
    let widest = pulsing
        .iter()
        .filter_map(|s| s.landing.and_then(|(_, ring)| ring))
        .fold(0.0, f64::max);
    assert!(widest > 8.0 && widest <= 14.0, "{widest}");
    bed.keyboard_goes_to("test.terminal")?;
    // Then nothing moves by the pulse's end; what draws after is the
    // clock's second, once a second.
    let end = samples
        .iter()
        .rfind(|s| s.streak.is_some() || s.landing.is_some())
        .map_or(Duration::ZERO, |s| s.at);
    assert!(
        end < first.at + Duration::from_millis(1100),
        "moving until {end:?}: {samples:?}"
    );
    let frames = bed.hp.frames();
    wait(&mut bed.hp, Duration::from_secs(2))?;
    assert!(bed.hp.frames() - frames <= 3, "drawn at rest");
    assert!(bed.hp.streak().is_none());
    Ok(())
}

/// On a GPU: the window on another desktop, the strip slides there first;
/// the streak runs once it has.
#[test]
fn going_to_an_agent_on_another_desktop_slides_there_first() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let Some(node) = render_node() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            gpu: Some(node),
            ..Machine::default()
        },
    )?;
    if bed.hp.renderer() != "gles" {
        eprintln!("the compositor draws on the CPU here: no streak");
        return Ok(());
    }
    let (_, _client) = common::open_with(&mut bed.hp, "test.terminal", (600, 400), false)?;
    common::chord(&mut bed.hp, &[common::KEY_LEFTMETA, common::KEY_2]);
    let away = bed.hp.dispatch_until(WAIT, |hp| hp.desktop() == (1, 1.0))?;
    anyhow::ensure!(away, "{:?}", bed.hp.desktop());
    bed.hocus(&["agent", "build", "working"], None)?;
    bed.agent_becomes("build", Some("working"))?;
    bed.settle()?;
    let samples = bed.go_to("build", "test.terminal", Duration::from_millis(1600))?;
    let sliding: Vec<&Sample> = samples
        .iter()
        .filter(|s| s.strip.1 > 0.0 && s.strip.1 < 1.0)
        .collect();
    anyhow::ensure!(sliding.len() >= 5, "no slide: {samples:?}");
    let (shown, span) = streaked(&samples);
    anyhow::ensure!(!shown.is_empty(), "no streak: {samples:?}");
    // Never while the strip moves; once it is still.
    assert!(shown.iter().all(|s| s.strip == (0, 0.0)), "{shown:?}");
    let slid = sliding.first().map_or(Duration::ZERO, |s| s.at);
    assert!(
        shown[0].at >= slid + Duration::from_millis(250),
        "{samples:?}"
    );
    assert!(span >= Duration::from_millis(420), "{span:?}");
    bed.keyboard_goes_to("test.terminal")?;
    Ok(())
}

/// With reduced motion (on a GPU), or a compositor on the CPU: no slide
/// and no streak; the rim lights and fades, without its ring.
#[test]
fn with_reduced_motion_or_on_the_cpu_only_the_rim_lights() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let machines = [
        render_node().map(|node| Machine {
            gpu: Some(node),
            reduced_motion: true,
            ..Machine::default()
        }),
        Some(Machine::default()),
    ];
    for machine in machines.into_iter().flatten() {
        let mut bed = start(&shell, &hocus, &machine)?;
        let (_, _client) = common::open_with(&mut bed.hp, "test.terminal", (600, 400), false)?;
        common::chord(&mut bed.hp, &[common::KEY_LEFTMETA, common::KEY_2]);
        let away = bed.hp.dispatch_until(WAIT, |hp| hp.desktop().0 == 1)?;
        anyhow::ensure!(away, "{:?}", bed.hp.desktop());
        bed.hocus(&["agent", "build", "waiting"], None)?;
        bed.agent_becomes("build", Some("waiting"))?;
        bed.settle()?;
        let samples = bed.go_to("build", "test.terminal", Duration::from_millis(1000))?;
        let renderer = bed.hp.renderer();
        assert!(
            samples.iter().all(|s| s.streak.is_none()),
            "{renderer}: {samples:?}"
        );
        assert!(
            samples.iter().all(|s| s.strip.1 == 0.0 || s.strip.1 == 1.0),
            "{renderer}, slid: {samples:?}"
        );
        let lit: Vec<(f32, Option<f64>)> = samples.iter().filter_map(|s| s.landing).collect();
        assert!(
            lit.iter().any(|(rim, _)| *rim > 0.5),
            "{renderer}: {samples:?}"
        );
        assert!(lit.iter().all(|(_, ring)| ring.is_none()), "{lit:?}");
        bed.keyboard_goes_to("test.terminal")?;
    }
    Ok(())
}

/// On a GPU: the streak takes its row's colour, read from the screen:
/// amber to an agent that waits, the accent to one that works.
#[test]
fn the_streak_takes_the_rows_colour() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let Some(node) = render_node() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            gpu: Some(node),
            ..Machine::default()
        },
    )?;
    if bed.hp.renderer() != "gles" {
        eprintln!("the compositor draws on the CPU here: no streak");
        return Ok(());
    }
    let (_, _client) = common::open_with(&mut bed.hp, "test.terminal", (500, 300), false)?;
    bed.hocus(&["agent", "infra", "waiting"], None)?;
    bed.hocus(&["agent", "build", "working"], None)?;
    bed.agent_becomes("infra", Some("waiting"))?;
    bed.agent_becomes("build", Some("working"))?;
    bed.settle()?;
    for (name, colour) in [("infra", [0xF5, 0xB4, 0x4A]), ("build", [0x6C, 0xC2, 0x87])] {
        let row = bed.row(name)?;
        bed.click(row);
        // A frame of the streak well on its way: three quarters along it,
        // the line is that much of its colour.
        let mut seen = None;
        let found = bed.hp.dispatch_until(WAIT, |hp| {
            seen = hp
                .streak()
                .filter(|s| s.alpha == 1.0 && !near(s.from, s.head, 250.0));
            seen.is_some()
        })?;
        anyhow::ensure!(found, "no streak to {name}");
        let Streak {
            from,
            head,
            colour: told,
            ..
        } = seen.unwrap_or_default();
        assert_eq!(told, colour, "{name}");
        let at = (
            from.0 + 0.7 * (head.0 - from.0),
            from.1 + 0.7 * (head.1 - from.1),
        );
        let [r, g, b] = pixel(&mut bed.hp, at)?;
        if name == "infra" {
            assert!(
                r > 110 && r > g + 15 && g > b + 15,
                "amber: {:?}",
                [r, g, b]
            );
        } else {
            assert!(
                g > 100 && g > r + 20 && g > b + 10,
                "green: {:?}",
                [r, g, b]
            );
        }
        let over = bed.hp.dispatch_until(WAIT, |hp| hp.streak().is_none())?;
        anyhow::ensure!(over, "the streak goes on");
        bed.settle()?;
    }
    Ok(())
}

/// A herdr agent's row selects its pane in herdr (a fake one).
#[test]
fn going_to_a_herdr_agent_selects_its_pane() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            herdr: true,
            ..Machine::default()
        },
    )?;
    bed.tool_becomes("herdr", ToolState::Running)?;
    bed.agent_becomes("refacto", Some("working"))?;
    bed.settle()?;
    let row = bed.row("refacto")?;
    bed.click(row);
    let heard = bed.herdr_heard.clone();
    let selected = bed.hp.dispatch_until(WAIT, |_| {
        heard
            .lock()
            .is_ok_and(|h| h.iter().any(|r| r == r#"pane.focus {"pane_id":"w1:p1"}"#))
    })?;
    anyhow::ensure!(
        selected,
        "herdr was asked {:?}",
        bed.herdr_heard.lock().map(|h| h.clone())
    );
    Ok(())
}
