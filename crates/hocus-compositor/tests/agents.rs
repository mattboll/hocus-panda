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
//! streak, the rim's pulse, herdr's pane; the streak's start fading to
//! nothing at its row, and a second click replacing it (4.3b). The halo (4.4): its wave and its
//! glow, its reminder, its priorities, its end.
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

use common::{BTN_LEFT, WAIT, render_node, wait};
use hocus_agents::tools::Places;
use hocus_compositor::{
    Config, Headless, HeadlessConfig, RendererChoice,
    going::Streak,
    halo::{Concern, Shown},
    model::Rect,
};
use hocus_link::ToolState;

/// What the test's machine has.
#[derive(Default)]
struct Machine {
    claude: bool,
    codex: bool,
    herdr: bool,
    /// What its herdr holds (default: one agent, `refacto`, alone in its
    /// workspace).
    herdr_holds: Option<fn() -> FakeHerdr>,
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
    /// What it holds, and its subscribers.
    herdr: Herdr,
}

/// The fake herdr's requests.
type Heard = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// What the fake herdr holds: its workspaces, tabs and agents as its API
/// lists them (`herdr api schema`), and the connections subscribed to its
/// events.
#[derive(Default)]
struct FakeHerdr {
    workspaces: Vec<serde_json::Value>,
    tabs: Vec<serde_json::Value>,
    agents: Vec<serde_json::Value>,
    /// An older herdr: no `workspace.list`, no `tab.list`.
    no_labels: bool,
    subscribers: Vec<std::os::unix::net::UnixStream>,
}

type Herdr = std::sync::Arc<std::sync::Mutex<FakeHerdr>>;

impl FakeHerdr {
    /// One workspace, `workspace`, with tabs labelled `tabs` (an unnamed
    /// one is labelled with its place), a Claude Code working in the
    /// first: pane w1:p1, its directory `cwd`.
    fn with(workspace: &str, tabs: &[&str], cwd: Option<&str>) -> Self {
        let tab = |i: usize, label: &str| {
            serde_json::json!({ "tab_id": format!("w1:t{}", i + 1), "workspace_id": "w1",
                "number": i + 1, "label": label, "focused": i == 0, "pane_count": 1,
                "agent_status": "idle" })
        };
        Self {
            workspaces: vec![serde_json::json!({ "workspace_id": "w1", "number": 1,
                "label": workspace, "focused": true, "pane_count": tabs.len(),
                "tab_count": tabs.len(), "active_tab_id": "w1:t1", "agent_status": "working" })],
            tabs: tabs.iter().enumerate().map(|(i, l)| tab(i, l)).collect(),
            agents: vec![
                serde_json::json!({ "pane_id": "w1:p1", "terminal_id": "w1:term1",
                "workspace_id": "w1", "tab_id": "w1:t1", "focused": false, "revision": 1,
                "agent_status": "working", "agent": "claude", "display_agent": null,
                "name": null, "title": "✳ Claude Code", "cwd": cwd, "foreground_cwd": null,
                "interactive_ready": true, "launch_pending": false,
                "screen_detection_skipped": false, "state_change_seq": 1,
                "state_labels": {}, "tokens": {} }),
            ],
            ..Self::default()
        }
    }
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

/// A fake herdr on `socket`, holding `herdr`; each pane's foreground
/// process is `pid`. What it is asked besides listing and subscribing
/// goes into `heard`.
fn fake_herdr(
    socket: &Path,
    pid: u32,
    herdr: Herdr,
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
            let Ok(mut held) = herdr.lock() else {
                return;
            };
            let method = request["method"].as_str();
            let result = match method {
                Some("agent.list") => {
                    serde_json::json!({ "type": "agent_list", "agents": held.agents })
                }
                Some("workspace.list" | "tab.list") if held.no_labels => serde_json::Value::Null,
                Some("workspace.list") => {
                    serde_json::json!({ "type": "workspace_list", "workspaces": held.workspaces })
                }
                Some("tab.list") => serde_json::json!({ "type": "tab_list", "tabs": held.tabs }),
                Some("pane.process_info") => {
                    serde_json::json!({ "type": "pane_process_info", "shell": { "pid": 1 },
                        "foreground": [ { "pid": pid, "name": "claude" } ] })
                }
                Some("events.subscribe") => serde_json::json!({ "type": "subscription_started" }),
                method => {
                    if let Ok(mut heard) = heard.lock() {
                        heard.push(format!("{} {}", method.unwrap_or(""), request["params"]));
                    }
                    serde_json::json!({ "type": "ok" })
                }
            };
            let answer = if result.is_null() {
                serde_json::json!({ "id": id, "error": { "code": "unknown_method",
                    "message": format!("unknown method {}", method.unwrap_or("")) } })
            } else {
                serde_json::json!({ "id": id, "result": result })
            };
            let _ = (&stream).write_all(format!("{answer}\n").as_bytes());
            if method == Some("events.subscribe")
                && let Ok(subscriber) = stream.try_clone()
            {
                held.subscribers.push(subscriber);
            }
            drop(held);
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
    let held: Herdr = std::sync::Arc::new(std::sync::Mutex::new(
        machine
            .herdr_holds
            .map_or_else(|| FakeHerdr::with("refacto", &["1"], None), |holds| holds()),
    ));
    let herdr = if machine.herdr {
        Some((
            fake_herdr(
                &home.join(".config/herdr/herdr.sock"),
                std::process::id(),
                held.clone(),
                heard.clone(),
            )?,
            heard,
        ))
    } else {
        None
    };
    let mut bed = start_in(dir, shell, hocus, herdr, machine)?;
    bed.herdr = held;
    Ok(bed)
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
        herdr: Herdr::default(),
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

    /// Where the briefing's `item` is (`Sources`, `Agent(3)`), global, as
    /// the shell last logged its items' middles in the popover `popover`.
    fn briefing_item(&mut self, popover: Rect, item: &str) -> anyhow::Result<(f64, f64)> {
        let wanted = format!("{item}@");
        let log = self.log.clone();
        let found = |text: &str| {
            text.lines()
                .rev()
                .find(|l| l.contains("the briefing's items"))
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
        anyhow::ensure!(done, "no {item} in the briefing");
        let (x, y) = found(&plain(&log)).unwrap_or_default();
        Ok((f64::from(popover.x) + x, f64::from(popover.y) + y))
    }

    /// The secretary's briefing, opened by a click on her, once it shows.
    fn open_briefing(&mut self) -> anyhow::Result<Rect> {
        let secretary = self.item("Secretary")?;
        self.click(secretary);
        let shown = self.hp.dispatch_until(WAIT, |hp| {
            hp.popover_surfaces()
                .iter()
                .any(|(_, r)| r.width == 320 && r.height > 0)
        })?;
        anyhow::ensure!(shown, "no briefing: {:?}", self.hp.popover_surfaces());
        self.logged("the briefing's items")?;
        let popover = self.hp.popover_surfaces();
        Ok(popover
            .iter()
            .find(|(_, r)| r.width == 320)
            .map_or(Rect::new(0, 0, 0, 0), |(_, r)| *r))
    }

    /// The Sources popover, opened from the briefing's "Sources", once it
    /// shows in the briefing's place.
    fn open_sources(&mut self) -> anyhow::Result<Rect> {
        let briefing = self.open_briefing()?;
        let sources = self.briefing_item(briefing, "Sources")?;
        self.click(sources);
        let shown = self.hp.dispatch_until(WAIT, |hp| {
            let popovers = hp.popover_surfaces();
            popovers.len() == 1 && popovers[0].1.width == 300 && popovers[0].1.height > 0
        })?;
        anyhow::ensure!(shown, "no Sources: {:?}", self.hp.popover_surfaces());
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
    // Sources, from the secretary's briefing, says what each is.
    let popover = bed.open_sources()?;
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
    let popover = bed.open_sources()?;
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
    let popover = bed.open_sources()?;
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
            .all(|p| (p[1].1 - p[0].1 - 34.0).abs() < 0.6),
        "34 px apart: {rows:?}"
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
    // Under the secretary, five rows and the line: 50 + 5 × 34 + 20, and
    // the ring's room.
    let tall = bed.hp.dispatch_until(WAIT, |hp| {
        hp.agents_surfaces()
            .first()
            .is_some_and(|(_, r)| r.height == 50 + 5 * 34 + 20 + 10)
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
        // c is stuck: to the top, a and b sliding down, c's panda hopping
        // (stuck, not waiting: the secretary holds still, 4.6).
        let from = bed.mark();
        bed.hocus(&["agent", "c", "failed"], None)?;
        let commits = bed.commits_for(Duration::from_secs(2))?;
        bed.logged_after(from, &["shows=AGENTS 3 · c bloqué · a 0\u{a0}min"])?;
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
                    landing: self.hp.pulse(app_id),
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

/// On a GPU: the streak starts from nothing at its row (issue #495). CSS's
/// shadow ignores the line's gradient: drawn as it is, its glow ringed the
/// line's first pixels, still transparent, a dark core. Read from the
/// screen against what it showed before the click: next to the row, and
/// behind it, the streak's first pixels leave what is under them; further
/// along its start, the line is no darker than its glow beside it.
#[test]
fn the_streaks_start_fades_to_nothing_at_its_row() -> anyhow::Result<()> {
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
    bed.hocus(&["agent", "build", "working"], None)?;
    bed.agent_becomes("build", Some("working"))?;
    let row = bed.row("build")?;
    // The pointer on the row first: its hover is in both pictures.
    bed.hp.pointer_move(row.0 - 1.0, row.1);
    bed.hp.pointer_move(row.0, row.1);
    bed.settle()?;
    let (width, _, before) = bed.hp.screenshot(0)?;
    bed.click(row);
    // A frame of the streak well on its way, at its full alpha.
    let mut seen = None;
    let found = bed.hp.dispatch_until(WAIT, |hp| {
        seen = hp
            .streak()
            .filter(|s| s.alpha == 1.0 && !near(s.from, s.head, 250.0));
        seen.is_some()
    })?;
    anyhow::ensure!(found, "no streak");
    let Streak { from, head, .. } = seen.unwrap_or_default();
    let (_, _, after) = bed.hp.screenshot(0)?;
    let len = ((head.0 - from.0).powi(2) + (head.1 - from.1).powi(2)).sqrt();
    let u = ((head.0 - from.0) / len, (head.1 - from.1) / len);
    // How much the streak changed the pixel `along` its line from its
    // start and `across` it: the most of its channels.
    let changed = |along: f64, across: f64| -> anyhow::Result<i32> {
        let x = from.0 + along * u.0 - across * u.1;
        let y = from.1 + along * u.1 + across * u.0;
        let at = ((y as i32 * width + x as i32) * 4) as usize;
        let (a, b) = (
            before.get(at..at + 3).unwrap_or_default(),
            after.get(at..at + 3).unwrap_or_default(),
        );
        anyhow::ensure!(a.len() == 3 && b.len() == 3, "({x}, {y}) is off the screen");
        Ok((0..3)
            .map(|i| (i32::from(a[i]) - i32::from(b[i])).abs())
            .max()
            .unwrap_or_default())
    };
    for along in [-8.0, -4.0, 0.0, 2.0, 4.0] {
        for across in [-8.0, -5.0, -3.0, 0.0, 3.0, 5.0, 8.0] {
            let d = changed(along, across)?;
            assert!(d <= 3, "{d} levels at {along} along, {across} across");
        }
    }
    for along in [8.0, 16.0, 32.0, 64.0] {
        let core = changed(along, 0.0)?;
        for across in [-8.0, -5.0, -3.0, 3.0, 5.0, 8.0] {
            let glow = changed(along, across)?;
            assert!(
                glow <= core + 2,
                "a dark core {along} along: {core} levels on the line, {glow} {across} across"
            );
        }
    }
    let over = bed.hp.dispatch_until(WAIT, |hp| hp.streak().is_none())?;
    anyhow::ensure!(over, "the streak goes on");
    Ok(())
}

/// On a GPU: a second row clicked while the first streak runs replaces it
/// at once: one streak, the second row's, from the second row.
#[test]
fn a_second_click_replaces_the_first_streak_at_once() -> anyhow::Result<()> {
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
    let (infra, build) = (bed.row("infra")?, bed.row("build")?);
    let amber = [0xF5, 0xB4, 0x4A];
    let accent = [0x6C, 0xC2, 0x87];
    bed.click(infra);
    let first = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.streak().is_some_and(|s| s.colour == amber))?;
    anyhow::ensure!(first, "no streak to infra");
    bed.click(build);
    bed.hp.dispatch(Duration::from_millis(2))?;
    let mut streaks = Vec::new();
    let start = Instant::now();
    let mut frames = bed.hp.frames();
    while start.elapsed() < Duration::from_millis(1200) {
        bed.hp.dispatch(Duration::from_millis(2))?;
        if bed.hp.frames() != frames {
            frames = bed.hp.frames();
            streaks.extend(bed.hp.streak());
        }
    }
    anyhow::ensure!(!streaks.is_empty(), "no streak to build");
    // From the second row's left edge, in its colour, from its first frame
    // on: the first streak is gone.
    assert!(
        streaks
            .iter()
            .all(|s| s.colour == accent && near(s.from, (build.0 - 92.0, build.1), 1.0)),
        "rows at {infra:?}, {build:?}: {streaks:?}"
    );
    assert!(bed.hp.streak().is_none(), "it ends");
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

// --- 4.1b: agents named as herdr names them (#497) ---------------------------------

impl Bed {
    /// The fake herdr changes what it holds, then tells its subscribers
    /// `event` with `data`.
    fn herdr_says(
        &mut self,
        change: impl FnOnce(&mut FakeHerdr),
        event: &str,
        data: serde_json::Value,
    ) -> anyhow::Result<()> {
        let mut held = self
            .herdr
            .lock()
            .map_err(|_| anyhow::anyhow!("the fake herdr panicked"))?;
        change(&mut held);
        let line = serde_json::json!({ "event": event, "data": data });
        anyhow::ensure!(!held.subscribers.is_empty(), "nobody follows herdr");
        for subscriber in &held.subscribers {
            (&*subscriber).write_all(format!("{line}\n").as_bytes())?;
        }
        Ok(())
    }

    /// No agent is named by its tool, or by its kind as herdr says it.
    fn no_tool_names(&self) -> anyhow::Result<()> {
        let agents = self.hp.agents_peek();
        let tools = ["claude", "Claude Code", "codex", "Codex", "claude-code"];
        anyhow::ensure!(
            !agents.iter().any(|a| tools.contains(&a.name.as_str())),
            "an agent named by its tool: {agents:?}"
        );
        Ok(())
    }
}

/// A Claude Code in herdr's first tab, named, of two, in workspace
/// "hocus-panda".
fn two_tabs() -> FakeHerdr {
    FakeHerdr::with(
        "hocus-panda",
        &["refacto", "2"],
        Some("/home/alex/hocus-panda"),
    )
}

/// The cause of #497: herdr's agent shows as herdr's sidebar names it, its
/// workspace's label and its tab's, not as its kind ("claude").
#[test]
fn a_herdr_agent_is_named_as_herdrs_sidebar_names_it() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            herdr: true,
            herdr_holds: Some(two_tabs),
            ..Machine::default()
        },
    )?;
    bed.tool_becomes("herdr", ToolState::Running)?;
    bed.agent_becomes("hocus-panda · refacto", Some("working"))?;
    // Followed as the shell started.
    bed.logged("shows=AGENTS 1 · hocus-panda · refacto 0\u{a0}min")?;
    bed.no_tool_names()?;
    Ok(())
}

/// A rename in herdr, its tab's then its workspace's, shows at once.
#[test]
fn a_rename_in_herdr_shows_at_once() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            herdr: true,
            herdr_holds: Some(two_tabs),
            ..Machine::default()
        },
    )?;
    bed.tool_becomes("herdr", ToolState::Running)?;
    bed.agent_becomes("hocus-panda · refacto", Some("working"))?;
    let from = bed.mark();
    bed.herdr_says(
        |h| h.tabs[0]["label"] = "revue".into(),
        "tab_renamed",
        serde_json::json!({ "type": "tab_renamed", "tab_id": "w1:t1",
            "workspace_id": "w1", "label": "revue" }),
    )?;
    bed.agent_becomes("hocus-panda · revue", Some("working"))?;
    bed.logged_after(from, &["shows=AGENTS 1 · hocus-panda · revue"])?;
    bed.herdr_says(
        |h| h.workspaces[0]["label"] = "panda".into(),
        "workspace_renamed",
        serde_json::json!({ "type": "workspace_renamed", "workspace_id": "w1",
            "label": "panda" }),
    )?;
    bed.agent_becomes("panda · revue", Some("working"))?;
    // Its other tab closed: an unnamed tab alone would not show, a named
    // one still does.
    bed.herdr_says(
        |h| {
            h.tabs.pop();
        },
        "tab_closed",
        serde_json::json!({ "type": "tab_closed", "tab_id": "w1:t2", "workspace_id": "w1" }),
    )?;
    bed.agent_becomes("panda · revue", Some("working"))?;
    bed.herdr_says(
        |h| h.tabs[0]["label"] = "1".into(),
        "tab_renamed",
        serde_json::json!({ "type": "tab_renamed", "tab_id": "w1:t1",
            "workspace_id": "w1", "label": "1" }),
    )?;
    bed.agent_becomes("panda", Some("working"))?;
    bed.no_tool_names()?;
    Ok(())
}

/// A Claude Code plugged in, running in a herdr pane: one agent, seen by
/// both, named as herdr names it rather than by its folder.
#[test]
fn a_claude_code_agent_in_a_herdr_pane_takes_herdrs_name() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            herdr: true,
            herdr_holds: Some(|| FakeHerdr::with("panda", &["refacto", "2"], None)),
            ..Machine::default()
        },
    )?;
    bed.tool_becomes("herdr", ToolState::Running)?;
    bed.agent_becomes("panda · refacto", Some("working"))?;
    // Its hooks, as Claude Code sends them: `hocus agent-hook` runs as
    // this process's child, the pane's foreground process.
    let hook = |event: &str| {
        format!(
            "{{\"session_id\":\"abc123\",\"transcript_path\":\"/x.jsonl\",\
             \"cwd\":\"/home/alex/hocus-panda\",\"permission_mode\":\"default\",\
             \"hook_event_name\":\"{event}\",\"tool_name\":\"Bash\",\
             \"tool_input\":{{\"command\":\"cargo test\"}}}}"
        )
    };
    bed.hocus(
        &["agent-hook", "claude-code"],
        Some(&hook("PermissionRequest")),
    )?;
    bed.agent_becomes("panda · refacto", Some("waiting"))?;
    let agents = bed.hp.agents();
    assert_eq!(agents.len(), 1, "{agents:?}");
    assert_eq!(agents[0].source, "claude-code+herdr");
    // Renamed in herdr: the hook's agent too.
    bed.herdr_says(
        |h| h.tabs[0]["label"] = "revue".into(),
        "tab.renamed",
        serde_json::json!({ "type": "tab_renamed", "tab_id": "w1:t1",
            "workspace_id": "w1", "label": "revue" }),
    )?;
    bed.agent_becomes("panda · revue", Some("waiting"))?;
    bed.no_tool_names()?;
    Ok(())
}

/// A Claude Code outside herdr: its project's folder, from its first
/// event's `cwd`, kept when later events have none or another; a session
/// whose events never say: its process's working directory.
#[test]
fn a_hook_agent_outside_herdr_takes_its_folder() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, &hocus, &Machine::default())?;
    let hook = |session: &str, event: &str, cwd: Option<&str>| {
        let cwd = cwd.map_or(String::new(), |c| format!(",\"cwd\":\"{c}\""));
        format!(
            "{{\"session_id\":\"{session}\",\"transcript_path\":\"/x.jsonl\"{cwd},\
             \"hook_event_name\":\"{event}\",\"prompt\":\"go\"}}"
        )
    };
    let from = bed.mark();
    bed.hocus(
        &["agent-hook", "claude-code"],
        Some(&hook("s1", "SessionStart", Some("/home/alex/hocus-panda"))),
    )?;
    bed.agent_becomes("hocus-panda", Some("idle"))?;
    bed.hocus(
        &["agent-hook", "claude-code"],
        Some(&hook("s1", "UserPromptSubmit", None)),
    )?;
    bed.agent_becomes("hocus-panda", Some("working"))?;
    bed.hocus(
        &["agent-hook", "claude-code"],
        Some(&hook(
            "s1",
            "PermissionRequest",
            Some("/home/alex/hocus-panda/crates"),
        )),
    )?;
    bed.agent_becomes("hocus-panda", Some("waiting"))?;
    bed.logged_after(from, &["shows=AGENTS 1 · hocus-panda t'attend"])?;
    // Another session whose events have no `cwd`: where its process runs
    // (this test's directory, as `hocus agent-hook` runs under it).
    let here = std::env::current_dir()?;
    let folder = here
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    bed.hocus(
        &["agent-hook", "claude-code"],
        Some(&hook("s2", "UserPromptSubmit", None)),
    )?;
    bed.agent_becomes(&folder, Some("working"))?;
    assert_eq!(bed.hp.agents().len(), 2, "{:?}", bed.hp.agents());
    bed.no_tool_names()?;
    Ok(())
}

/// Never the tool's name: an older herdr without its labels names its
/// agent by its folder, then by its process's directory, not "claude".
#[test]
fn an_agent_is_never_named_by_its_tool() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            herdr: true,
            herdr_holds: Some(|| {
                let mut herdr = FakeHerdr::with("panda", &["1"], None);
                herdr.no_labels = true;
                herdr
            }),
            ..Machine::default()
        },
    )?;
    bed.tool_becomes("herdr", ToolState::Running)?;
    let here = std::env::current_dir()?;
    let folder = here
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    bed.agent_becomes(&folder, Some("working"))?;
    bed.no_tool_names()?;
    Ok(())
}

// --- 4.4: the halo ---------------------------------------------------------------

/// What the halo shows at a frame.
#[derive(Debug, Clone, Copy)]
struct Glimpse {
    /// Since the sampling started.
    at: Duration,
    shown: Option<Shown>,
}

impl Glimpse {
    fn wave(&self) -> Option<f64> {
        self.shown.and_then(|s| s.wave).map(|w| w.top)
    }

    fn border(&self) -> f32 {
        self.shown.map_or(0.0, |s| s.glow.border)
    }
}

impl Bed {
    /// The halo at each frame drawn for `time`.
    fn halo_for(&mut self, time: Duration) -> anyhow::Result<Vec<Glimpse>> {
        let start = Instant::now();
        let mut frames = self.hp.frames();
        let mut out = Vec::new();
        while start.elapsed() < time {
            self.hp.dispatch(Duration::from_millis(2))?;
            let now = self.hp.frames();
            if now != frames {
                frames = now;
                out.push(Glimpse {
                    at: start.elapsed(),
                    shown: self.hp.halo(),
                });
            }
        }
        Ok(out)
    }

    /// Nothing draws but the clock's seconds for 2 s.
    fn rests(&mut self) -> anyhow::Result<()> {
        let frames = self.hp.frames();
        wait(&mut self.hp, Duration::from_secs(2))?;
        let drawn = self.hp.frames() - frames;
        anyhow::ensure!(drawn <= 3, "{drawn} frames drawn at rest");
        Ok(())
    }

    /// 3 px left of the bar, at its middle: in its outer glow.
    fn beside_the_bar(&self) -> (f64, f64) {
        let width = self.hp.bars().first().map_or(0.0, |b| b.2);
        (1920.0 - 8.0 - width - 3.0, 540.0)
    }

    /// Agent `name` becomes `state`, and the shell shows it.
    fn agent_is(&mut self, name: &str, state: &str) -> anyhow::Result<()> {
        self.hocus(&["agent", name, state], None)?;
        self.agent_becomes(name, Some(state))
    }
}

/// A bed on the GPU, the compositor drawing with GLES (the halo is its
/// shaders'); `None`, and the test is skipped, without one.
fn halo_bed(reduced_motion: bool) -> anyhow::Result<Option<Bed>> {
    let Some((shell, hocus)) = ready() else {
        return Ok(None);
    };
    let Some(node) = render_node() else {
        return Ok(None);
    };
    let bed = start(
        &shell,
        &hocus,
        &Machine {
            gpu: Some(node),
            reduced_motion,
            ..Machine::default()
        },
    )?;
    if bed.hp.renderer() != "gles" {
        eprintln!("the compositor draws on the CPU here: no halo");
        return Ok(None);
    }
    Ok(Some(bed))
}

/// From the first glimpse to the last that `shows`, how long.
fn span(glimpses: &[Glimpse], shows: impl Fn(&Glimpse) -> bool) -> Duration {
    let shown: Vec<&Glimpse> = glimpses.iter().filter(|g| shows(g)).collect();
    match (shown.first(), shown.last()) {
        (Some(a), Some(b)) => b.at - a.at,
        _ => Duration::ZERO,
    }
}

/// On a GPU: an agent waiting runs one wave down the bar, in amber, for
/// some 600 ms, as the glow fades in; the glow stays, read from the screen
/// beside the bar, and nothing is drawn at rest.
#[test]
fn a_waiting_agent_runs_a_wave_then_glows_then_nothing_draws() -> anyhow::Result<()> {
    let Some(mut bed) = halo_bed(false)? else {
        return Ok(());
    };
    bed.agent_is("build", "working")?;
    bed.settle()?;
    assert!(bed.hp.halo().is_none());
    let beside = bed.beside_the_bar();
    let before = pixel(&mut bed.hp, beside)?;
    bed.hocus(&["agent", "build", "waiting"], None)?;
    let glimpses = bed.halo_for(Duration::from_millis(1500))?;
    let waving: Vec<(Duration, f64)> = glimpses
        .iter()
        .filter_map(|g| g.wave().map(|top| (g.at, top)))
        .collect();
    let (Some(first), Some(last)) = (waving.first(), waving.last()) else {
        anyhow::bail!("no wave: {glimpses:?}");
    };
    // Down the bar from above its top to near its bottom, once.
    assert!(first.1 < 0.3 && last.1 > 0.6, "{waving:?}");
    assert!(waving.windows(2).all(|p| p[1].1 > p[0].1), "{waving:?}");
    let wave = glimpses.iter().find_map(|g| g.shown.and_then(|s| s.wave));
    assert_eq!(
        wave.map(|w| (w.colour, w.alpha)),
        Some(([0xF5, 0xB4, 0x4A], 0.38))
    );
    let lasted = span(&glimpses, |g| g.wave().is_some());
    assert!(
        (Duration::from_millis(450)..Duration::from_millis(650)).contains(&lasted),
        "{lasted:?}: {waving:?}"
    );
    // The glow fades in with it, then stays.
    let full = Concern::Waiting.glow();
    let rising = span(&glimpses, |g| g.shown.is_some_and(|s| s.glow != full));
    assert!(
        (Duration::from_millis(450)..Duration::from_millis(700)).contains(&rising),
        "{rising:?}: {glimpses:?}"
    );
    // The secretary radiates for 3.2 s from the start (4.6): then rest.
    wait(&mut bed.hp, Duration::from_secs(2))?;
    bed.rests()?;
    let rest = bed.hp.halo().map(|s| (s.concern, s.glow, s.wave));
    assert_eq!(rest, Some((Some(Concern::Waiting), full, None)));
    assert_eq!(bed.hp.halo_waves(), 1);
    // Amber beside the bar.
    let after = pixel(&mut bed.hp, beside)?;
    assert!(
        after[0] > before[0] + 25 && after[1] > before[1] + 15 && after[0] > after[2] + 10,
        "{before:?} then {after:?}"
    );
    Ok(())
}

/// On a GPU: still waiting at its fifth minute (backdated to 4 min 59 s),
/// the agent's reminder runs one more wave, by the loop's timer a second
/// later; then never again.
#[test]
fn the_reminder_wave_comes_once_after_five_minutes() -> anyhow::Result<()> {
    let Some(mut bed) = halo_bed(false)? else {
        return Ok(());
    };
    bed.agent_is("build", "waiting")?;
    bed.settle()?;
    assert_eq!(bed.hp.halo_waves(), 1);
    // As if it had waited 4 min 59 s by now.
    let since = Duration::from_micros(bed.hp.agents_peek()[0].since);
    let waited = bed.hp.state().now().saturating_sub(since);
    bed.hp
        .backdate_agents(Duration::from_secs(299).saturating_sub(waited));
    let glimpses = bed.halo_for(Duration::from_millis(2500))?;
    let came = glimpses
        .iter()
        .find(|g| g.wave().is_some())
        .map(|g| g.at)
        .ok_or_else(|| anyhow::anyhow!("no reminder: {glimpses:?}"))?;
    assert!(
        (Duration::from_millis(800)..Duration::from_millis(1600)).contains(&came),
        "{came:?}"
    );
    assert_eq!(bed.hp.halo_waves(), 2);
    // An hour later, still waiting: no more.
    bed.hp.backdate_agents(Duration::from_secs(3600));
    wait(&mut bed.hp, Duration::from_millis(1500))?;
    assert_eq!(bed.hp.halo_waves(), 2);
    bed.rests()?;
    Ok(())
}

/// On a GPU: a stuck agent lights the glow in orange, without a wave.
#[test]
fn a_stuck_agent_glows_without_a_wave() -> anyhow::Result<()> {
    let Some(mut bed) = halo_bed(false)? else {
        return Ok(());
    };
    bed.agent_is("build", "working")?;
    bed.settle()?;
    let beside = bed.beside_the_bar();
    let before = pixel(&mut bed.hp, beside)?;
    bed.hocus(&["agent", "build", "failed"], None)?;
    let glimpses = bed.halo_for(Duration::from_millis(1200))?;
    assert!(glimpses.iter().any(|g| g.shown.is_some()), "{glimpses:?}");
    assert!(glimpses.iter().all(|g| g.wave().is_none()), "{glimpses:?}");
    bed.rests()?;
    let rest = bed.hp.halo().map(|s| (s.concern, s.glow));
    assert_eq!(rest, Some((Some(Concern::Stuck), Concern::Stuck.glow())));
    assert_eq!(bed.hp.halo_waves(), 0);
    let after = pixel(&mut bed.hp, beside)?;
    let lift = [0, 1, 2].map(|i| after[i] - before[i]);
    assert!(
        lift[0] > 25 && lift[0] > lift[1] + 8,
        "{before:?} then {after:?}"
    );
    Ok(())
}

/// On a GPU: a finished agent not seen yet lights a soft green glow,
/// without a wave (variant B).
#[test]
fn a_finished_agent_not_seen_glows_softly_green() -> anyhow::Result<()> {
    let Some(mut bed) = halo_bed(false)? else {
        return Ok(());
    };
    bed.agent_is("build", "working")?;
    bed.settle()?;
    let beside = bed.beside_the_bar();
    let before = pixel(&mut bed.hp, beside)?;
    bed.hocus(&["agent", "build", "done"], None)?;
    let glimpses = bed.halo_for(Duration::from_millis(1200))?;
    assert!(glimpses.iter().all(|g| g.wave().is_none()), "{glimpses:?}");
    bed.rests()?;
    let soft = Concern::ToSee.glow();
    assert_eq!(soft.rgb(), [0x6C, 0xC2, 0x87]);
    assert_eq!(
        (soft.border, soft.outer.alpha, soft.inner.alpha),
        (0.45, 0.25, 0.10)
    );
    let rest = bed.hp.halo().map(|s| (s.concern, s.glow));
    assert_eq!(rest, Some((Some(Concern::ToSee), soft)));
    let after = pixel(&mut bed.hp, beside)?;
    let lift = [0, 1, 2].map(|i| after[i] - before[i]);
    assert!(
        lift[1] > 8 && lift[1] > lift[0],
        "{before:?} then {after:?}"
    );
    Ok(())
}

/// One state at a time: waiting over stuck over to see; as each is dealt
/// with, the next shows; the finished one seen from its row, nothing.
/// The model's, so on Pixman too, which draws no halo.
#[test]
fn waiting_comes_before_stuck_before_to_see() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, &hocus, &Machine::default())?;
    for (name, state, concern) in [
        ("api", "done", Concern::ToSee),
        ("site", "failed", Concern::Stuck),
        ("infra", "waiting", Concern::Waiting),
        ("build", "working", Concern::Waiting),
    ] {
        bed.agent_is(name, state)?;
        assert_eq!(bed.hp.halo_concern(), Some(concern), "{name} {state}");
    }
    // Answered, the stuck one shows; working again, the finished one.
    bed.agent_is("infra", "working")?;
    assert_eq!(bed.hp.halo_concern(), Some(Concern::Stuck));
    bed.agent_is("site", "working")?;
    assert_eq!(bed.hp.halo_concern(), Some(Concern::ToSee));
    bed.settle()?;
    let row = bed.row("api")?;
    bed.click(row);
    bed.agent_becomes("api", Some("seen"))?;
    assert_eq!(bed.hp.halo_concern(), None);
    assert_eq!(bed.hp.renderer(), "pixman");
    assert!(bed.hp.halo().is_none(), "drawn on the CPU");
    Ok(())
}

/// On a GPU: answering the agent ends its halo, which fades out in some
/// 500 ms; waiting again, going to it from its row does too, while it
/// still waits. Then nothing is drawn.
#[test]
fn acting_ends_the_halo_with_its_fade() -> anyhow::Result<()> {
    let Some(mut bed) = halo_bed(false)? else {
        return Ok(());
    };
    for way in ["answered", "gone to"] {
        bed.agent_is("build", "waiting")?;
        bed.settle()?;
        assert!(bed.hp.halo().is_some(), "{way}");
        if way == "answered" {
            bed.hocus(&["agent", "build", "working"], None)?;
        } else {
            let row = bed.row("build")?;
            bed.click(row);
        }
        let glimpses = bed.halo_for(Duration::from_millis(1200))?;
        let fading: Vec<f32> = glimpses
            .iter()
            .filter(|g| g.shown.is_some_and(|s| s.concern.is_none()))
            .map(Glimpse::border)
            .collect();
        assert!(fading.len() >= 10, "{way}: {glimpses:?}");
        assert!(fading.windows(2).all(|p| p[1] <= p[0]), "{way}: {fading:?}");
        let faded = span(&glimpses, |g| g.shown.is_some());
        assert!(
            (Duration::from_millis(350)..Duration::from_millis(650)).contains(&faded),
            "{way}: {faded:?}"
        );
        assert!(bed.hp.halo().is_none(), "{way}");
        assert_eq!(bed.hp.halo_concern(), None, "{way}");
        bed.rests()?;
        if way == "gone to" {
            // It still waits; only its halo ended.
            assert_eq!(bed.hp.agents_peek()[0].state, "waiting");
        } else {
            bed.agent_becomes("build", Some("working"))?;
        }
    }
    Ok(())
}

/// With reduced motion, on a GPU: no wave; the glow is there within some
/// 200 ms.
#[test]
fn with_reduced_motion_no_wave_and_the_glow_fades_in_quickly() -> anyhow::Result<()> {
    let Some(mut bed) = halo_bed(true)? else {
        return Ok(());
    };
    bed.agent_is("build", "working")?;
    bed.settle()?;
    bed.hocus(&["agent", "build", "waiting"], None)?;
    let glimpses = bed.halo_for(Duration::from_millis(1000))?;
    assert!(glimpses.iter().all(|g| g.wave().is_none()), "{glimpses:?}");
    let full = Concern::Waiting.glow();
    assert!(
        glimpses
            .iter()
            .any(|g| g.shown.is_some_and(|s| s.glow == full)),
        "{glimpses:?}"
    );
    let rising = span(&glimpses, |g| g.shown.is_some_and(|s| s.glow != full));
    assert!(
        rising < Duration::from_millis(260),
        "{rising:?}: {glimpses:?}"
    );
    assert_eq!(bed.hp.halo_waves(), 0);
    bed.rests()?;
    Ok(())
}

// --- 4.6: the secretary and her briefing -----------------------------------------

impl Bed {
    /// A Claude Code session `session` in project `project` says `event`,
    /// with `extra` fields: its hook, as Claude Code runs it.
    fn claude(&self, session: &str, project: &str, event: &str, extra: &str) -> anyhow::Result<()> {
        let payload = format!(
            "{{\"session_id\":\"{session}\",\"cwd\":\"/home/alex/{project}\",\
             \"hook_event_name\":\"{event}\"{extra}}}"
        );
        self.hocus(&["agent-hook", "claude-code"], Some(&payload))
    }

    /// The group's items as last logged.
    fn items_logged(&self) -> String {
        plain(&self.log)
            .lines()
            .rev()
            .find(|l| l.contains("the agents group's items"))
            .unwrap_or_default()
            .to_owned()
    }

    /// 3 px into the secretary's row, left of her head: its tint.
    fn secretary_tint(&mut self) -> anyhow::Result<[i32; 3]> {
        let (x, y) = self.origin();
        pixel(&mut self.hp, (x - 3.0, y + 22.0))
    }
}

#[test]
fn the_secretary_heads_the_group_with_her_line_in_the_moments_mood() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, &hocus, &Machine::default())?;
    let from = bed.mark();
    for (name, state) in [("build", "working"), ("docs", "working"), ("api", "done")] {
        bed.agent_is(name, state)?;
    }
    // Calm: how many work, how many are to see; her ear green.
    bed.logged_after(
        from,
        &[
            "secretary=2 au travail · 1 à voir",
            "mood=Calm",
            "shows=AGENTS 3",
        ],
    )?;
    bed.settle()?;
    // She heads the group; "Sources" is in her briefing, not beside her.
    let items = bed.items_logged();
    assert!(items.contains("Secretary@"), "{items}");
    assert!(!items.contains("Sources@"), "{items}");
    let calm = bed.secretary_tint()?;
    // Someone waits: amber, and who.
    let from = bed.mark();
    bed.agent_is("site-web", "waiting")?;
    bed.logged_after(
        from,
        &["secretary=Quelqu'un t'attend · site-web", "mood=Waiting"],
    )?;
    bed.settle()?;
    let waiting = bed.secretary_tint()?;
    let lift = |t: [i32; 3]| [t[0] - calm[0], t[1] - calm[1], t[2] - calm[2]];
    let amber = lift(waiting);
    assert!(
        amber[0] > 8 && amber[1] > 5 && amber[0] > amber[2] + 5,
        "amber: {calm:?} {waiting:?}"
    );
    // Answered, and another one stuck: orange, and who.
    let from = bed.mark();
    bed.agent_is("site-web", "working")?;
    bed.agent_is("migration", "failed")?;
    bed.logged_after(
        from,
        &["secretary=Un agent est bloqué · migration", "mood=Stuck"],
    )?;
    bed.settle()?;
    let stuck = bed.secretary_tint()?;
    let orange = lift(stuck);
    assert!(
        orange[0] > 8 && orange[0] > orange[1] + 3,
        "orange: {calm:?} {stuck:?}"
    );
    Ok(())
}

/// On a GPU: when someone starts waiting, her ear radiates twice and she
/// hops twice, 3.2 s, once; then nothing moves. With reduced motion, she
/// holds still. Skipped without a render node.
#[test]
fn the_secretary_radiates_once_when_someone_starts_waiting_then_rests() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let Some(node) = render_node() else {
        return Ok(());
    };
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
        bed.agent_is("build", "working")?;
        bed.settle()?;
        let from = bed.mark();
        bed.hocus(&["agent", "build", "waiting"], None)?;
        let commits = bed.commits_for(Duration::from_secs(5))?;
        bed.logged_after(from, &["mood=Waiting"])?;
        let radiated = plain(&bed.log)
            .get(from..)
            .unwrap_or_default()
            .matches("the secretary radiates")
            .count();
        let (frames, span) = motion_of(&commits);
        let last = commits.last().copied().unwrap_or_default();
        if moves {
            assert_eq!(radiated, 1, "{commits:?}");
            assert!(frames >= 60, "{frames} frames of motion: {commits:?}");
            assert!(
                (Duration::from_millis(2900)..Duration::from_millis(3700)).contains(&span),
                "{span:?}: {commits:?}"
            );
            assert!(last < Duration::from_millis(4000), "{commits:?}");
        } else {
            assert_eq!(radiated, 0);
            assert_eq!(frames, 0, "{commits:?}");
            assert!(last < Duration::from_millis(1200), "{commits:?}");
        }
        // Still waiting, the same report again: she does not radiate again.
        let from = bed.mark();
        bed.hocus(&["agent", "build", "waiting"], None)?;
        let commits = bed.commits_for(Duration::from_secs(1))?;
        assert!(commits.is_empty(), "{commits:?}");
        assert!(
            !plain(&bed.log)
                .get(from..)
                .unwrap_or_default()
                .contains("the secretary radiates")
        );
    }
    Ok(())
}

#[test]
fn her_briefing_lists_who_waits_and_asks_what_who_is_stuck_and_why_and_who_to_see()
-> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, &hocus, &Machine::default())?;
    bed.claude(
        "s1",
        "site-web",
        "PermissionRequest",
        ",\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"npm install\",\
         \"description\":\"Install the dependencies\"}",
    )?;
    bed.agent_becomes("site-web", Some("waiting"))?;
    // Its permission's notification, 6 s later, says less: the request
    // is kept.
    bed.claude(
        "s1",
        "site-web",
        "Notification",
        ",\"message\":\"Claude needs your permission to use Bash\",\
         \"notification_type\":\"permission_prompt\"",
    )?;
    bed.claude(
        "s2",
        "migration",
        "StopFailure",
        ",\"error\":\"rate_limit\",\"error_details\":\"429 Too Many Requests\",\
         \"last_assistant_message\":\"API Error: Rate limit reached\"",
    )?;
    bed.agent_becomes("migration", Some("failed"))?;
    bed.claude("s3", "api", "Stop", ",\"stop_hook_active\":false")?;
    bed.agent_becomes("api", Some("done"))?;
    // The model has what each tool said.
    let agents = bed.hp.agents();
    let note = |name: &str| {
        agents
            .iter()
            .find(|a| a.name == name)
            .and_then(|a| a.note.clone())
    };
    assert_eq!(
        note("site-web"),
        Some(hocus_link::Note::allow("Bash", "npm install"))
    );
    assert_eq!(
        note("migration"),
        hocus_link::Note::says("API Error: Rate limit reached")
    );
    assert_eq!(note("api"), None);
    bed.settle()?;
    let from = bed.mark();
    let briefing = bed.open_briefing()?;
    // 320 px, beside the bar, its top on her row's (100 px under the bar's).
    assert_eq!((briefing.width, briefing.y), (320, 8 + 100));
    // Who waits and what it asks (15 characters: "Bash", "npm install"),
    // who is stuck and why (the failure's 29), who is to see; the footer.
    bed.logged_after(
        from,
        &[
            "briefing=T'ATTEND site-web · à l'instant (asks, 15 characters) \
           · BLOQUÉ migration · à l'instant (says, 29 characters) \
           · À VOIR api · à l'instant · 0 au travail · 0 au repos",
        ],
    )?;
    // What the tools said is drawn, never logged.
    let log = plain(&bed.log);
    assert!(!log.contains("npm install"));
    assert!(!log.contains("Rate limit"));
    Ok(())
}

#[test]
fn a_line_of_her_briefing_goes_to_its_agent() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, &hocus, &Machine::default())?;
    // Its terminal, on the first desktop; the user on the second.
    let (_, _client) = common::open_with(&mut bed.hp, "test.terminal", (600, 400), false)?;
    common::chord(&mut bed.hp, &[common::KEY_LEFTMETA, common::KEY_2]);
    let away = bed.hp.dispatch_until(WAIT, |hp| hp.desktop() == (1, 1.0))?;
    anyhow::ensure!(away, "{:?}", bed.hp.desktop());
    bed.agent_is("build", "waiting")?;
    bed.settle()?;
    let id = bed.id_of("build")?;
    let briefing = bed.open_briefing()?;
    let line = bed.briefing_item(briefing, &format!("Agent({id})"))?;
    bed.click(line);
    // The briefing closes; its desktop, its window with the keyboard.
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    anyhow::ensure!(closed, "{:?}", bed.hp.popover_surfaces());
    let there = bed.hp.dispatch_until(WAIT, |hp| hp.desktop() == (0, 0.0))?;
    anyhow::ensure!(there, "{:?}", bed.hp.desktop());
    let focused = bed.hp.dispatch_until(WAIT, |hp| {
        hp.windows()
            .iter()
            .any(|w| w.focused && w.keyboard && w.app_id.as_deref() == Some("test.terminal"))
    })?;
    anyhow::ensure!(focused, "{:?}", bed.keyboard_on());
    Ok(())
}

#[test]
fn sources_opens_from_her_briefing() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            claude: true,
            ..Machine::default()
        },
    )?;
    // The card under her, as 4.0 built it; she is calm.
    bed.logged("shows=Pour suivre tes agents, branche ton outil\u{a0}: Claude Code")?;
    bed.logged("secretary=0 au travail")?;
    let from = bed.mark();
    let sources = bed.open_sources()?;
    // In the briefing's place, where 4.0 opened it.
    assert_eq!((sources.width, sources.y), (300, 8 + 62));
    bed.logged_after(from, &["Trouvé, pas branché"])?;
    Ok(())
}

#[test]
fn nothing_commits_at_rest_with_her_briefing_open() -> anyhow::Result<()> {
    let Some((shell, hocus)) = ready() else {
        return Ok(());
    };
    // No minute changes for the test's while.
    let mut bed = start(
        &shell,
        &hocus,
        &Machine {
            clock_at: Some("10:00:05"),
            ..Machine::default()
        },
    )?;
    bed.claude(
        "s1",
        "site-web",
        "PermissionRequest",
        ",\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"npm install\"}",
    )?;
    bed.agent_becomes("site-web", Some("waiting"))?;
    bed.agent_is("migration", "failed")?;
    bed.open_briefing()?;
    bed.settle()?;
    let before = bed.counts();
    // The same states again, and time passing: nothing.
    for _ in 0..5 {
        bed.hocus(&["agent", "migration", "failed"], None)?;
    }
    wait(&mut bed.hp, Duration::from_secs(2))?;
    assert_eq!(before, bed.counts());
    Ok(())
}
