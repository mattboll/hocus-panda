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
//! the agent's window found above its process; nothing drawn at rest.
//!
//! Pixman here, so the shell draws on the CPU, opaque and without motion:
//! as in CI. The shell runs away from the machine's services (no session
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
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use hocus_link::ToolState;

/// What the test's machine has.
#[derive(Default)]
struct Machine {
    claude: bool,
    codex: bool,
    herdr: bool,
}

struct Bed {
    dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    hocus: PathBuf,
    /// The fake herdr's thread runs while the test does.
    _herdr: Option<std::thread::JoinHandle<()>>,
}

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
/// whose foreground process is `pid`.
fn fake_herdr(socket: &Path, pid: u32) -> anyhow::Result<std::thread::JoinHandle<()>> {
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
                _ => serde_json::json!({ "type": "subscription_started" }),
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
    let herdr = if machine.herdr {
        Some(fake_herdr(
            &home.join(".config/herdr/herdr.sock"),
            std::process::id(),
        )?)
    } else {
        None
    };
    start_in(dir, shell, hocus, herdr)
}

fn start_in(
    dir: tempfile::TempDir,
    shell: &Path,
    hocus: &Path,
    herdr: Option<std::thread::JoinHandle<()>>,
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
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' \
             RUST_LOG=warn,hocuspanda_shell=debug\nexec '{}' \"$@\" 2>>'{}'\n",
            home.display(),
            shell.display(),
            log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let config = Config {
        agent_places: Some(places(&home, hocus)),
        ..Config::default()
    };
    let hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    let mut bed = Bed {
        dir,
        hp,
        log,
        hocus: hocus.to_path_buf(),
        _herdr: herdr,
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

    fn settle(&mut self) -> anyhow::Result<()> {
        let start = Instant::now();
        loop {
            let before = (self.hp.agents_commits(), self.hp.popover_commits());
            wait(&mut self.hp, Duration::from_millis(600))?;
            if (self.hp.agents_commits(), self.hp.popover_commits()) == before
                || start.elapsed() > WAIT
            {
                return Ok(());
            }
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
        Ok((right + x, 8.0 + y))
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
    let mut again = start_in(dir, &shell, &hocus, None)?;
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
    // An agent runs: the group's header, until 4.1's tiles.
    bed.logged_after(from, &["shows=AGENTS", "agents=1"])?;
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
        },
    )?;
    bed.hocus(&["agent", "build", "working"], None)?;
    bed.agent_becomes("build", Some("working"))?;
    bed.settle()?;
    let before = (
        bed.hp.agents_commits(),
        bed.hp.bar_commits(),
        bed.hp.levels_commits(),
    );
    // The same state again, and time passing: nothing.
    for _ in 0..5 {
        bed.hocus(&["agent", "build", "working"], None)?;
    }
    wait(&mut bed.hp, Duration::from_secs(2))?;
    let after = (
        bed.hp.agents_commits(),
        bed.hp.bar_commits(),
        bed.hp.levels_commits(),
    );
    assert_eq!(before, after);
    Ok(())
}
