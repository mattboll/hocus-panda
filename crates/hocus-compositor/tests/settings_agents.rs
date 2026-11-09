//! Agents (6.16, issue #450, variant A): the real `hocus-settings` window,
//! a client of a headless compositor (one 2560 × 1600 screen) whose
//! agents' tools are looked for in the test's own home, as the Settings
//! read them there too; its configuration file, state and runtime
//! directories the test's own, no bus, no GPU; herdr a fake on a socket of
//! the test's.
//!
//! One test per point of the spec: the three tools and their states;
//! "Brancher" writing nothing before the consent, then plugging in;
//! "Annuler" after "Débrancher" plugging it back; a declined tool plugged
//! in from the page and its refusal cleared; "Suivre herdr" off declining
//! it, its agents gone; the halo for a waiting agent only, the preview
//! saying so; the peek never; the rows' pandas as dots; no tool at all.
//! What the window shows is read from its log, never on a time.
//!
//! And the secretary's (secretary.md lot 7): in assistant mode only, the
//! applications that notified (the shell's history, the test's) listed
//! under her mode, Slack first; "Autoriser" writing nothing before its
//! sheet's "Autoriser", then a grant in her consent book (the test's data
//! folder), in the notebook chosen; "Ne plus lire", a revocation.

mod common;
mod settings_page;

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::{Path, PathBuf},
    time::Duration,
};

use common::{BTN_LEFT, WAIT, wait};
use hocus_agents::tools::Places;
use hocus_compositor::{
    Config, HaloRule, Headless, HeadlessConfig, PeekRule, RendererChoice, halo::Concern,
    model::Rect,
};
use serde_json::Value;

const APP_ID: &str = "hocus-settings";
const FILE: &str = "# By hand.\n[keyboard]\nlayouts = [\"us\"]\n";
/// Claude Code's settings before the page plugs it in.
const ORIGINAL: &str = "{\n  \"model\": \"opus\"\n}\n";

/// What the test's machine has.
#[derive(Default, Clone, Copy)]
struct Machine {
    claude: bool,
    codex: bool,
    /// herdr runs, with one agent working.
    herdr: bool,
    /// Codex declined with "Ne rien brancher" before.
    codex_declined: bool,
    /// The window moves (else reduced motion, as the file says).
    motion: bool,
    /// The secretary is the assistant, as the file says.
    assistant: bool,
    /// Slack and Signal notified (the shell's history), and an agents'
    /// tool.
    notified: bool,
}

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
    script: PathBuf,
    hocus: PathBuf,
    _herdr: Option<std::thread::JoinHandle<()>>,
}

/// A fake herdr on `socket`: one workspace, one tab, one Claude Code
/// working in it.
fn fake_herdr(socket: &Path) -> anyhow::Result<std::thread::JoinHandle<()>> {
    std::fs::create_dir_all(socket.parent().unwrap_or(Path::new("/")))?;
    let listener = UnixListener::bind(socket)?;
    let pid = std::process::id();
    Ok(std::thread::spawn(move || {
        let mut kept = Vec::new();
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            let Ok(request) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let result = match request["method"].as_str() {
                Some("agent.list") => serde_json::json!({ "type": "agent_list", "agents": [
                    { "pane_id": "w1:p1", "terminal_id": "w1:term1", "workspace_id": "w1",
                      "tab_id": "w1:t1", "focused": false, "revision": 1,
                      "agent_status": "working", "agent": "claude", "display_agent": null,
                      "name": null, "title": "✳ Claude Code", "cwd": null,
                      "foreground_cwd": null, "interactive_ready": true,
                      "launch_pending": false, "screen_detection_skipped": false,
                      "state_change_seq": 1, "state_labels": {}, "tokens": {} } ] }),
                Some("workspace.list") => serde_json::json!({ "type": "workspace_list",
                    "workspaces": [ { "workspace_id": "w1", "number": 1, "label": "refacto",
                      "focused": true, "pane_count": 1, "tab_count": 1,
                      "active_tab_id": "w1:t1", "agent_status": "working" } ] }),
                Some("tab.list") => serde_json::json!({ "type": "tab_list", "tabs": [
                    { "tab_id": "w1:t1", "workspace_id": "w1", "number": 1, "label": "1",
                      "focused": true, "pane_count": 1, "agent_status": "working" } ] }),
                Some("pane.process_info") => serde_json::json!({ "type": "pane_process_info",
                    "shell": { "pid": 1 }, "foreground": [ { "pid": pid, "name": "claude" } ] }),
                Some("events.subscribe") => serde_json::json!({ "type": "subscription_started" }),
                _ => serde_json::json!({ "type": "ok" }),
            };
            let answer = serde_json::json!({ "id": request["id"], "result": result });
            let _ = (&stream).write_all(format!("{answer}\n").as_bytes());
            kept.push(stream);
        }
    }))
}

impl Bed {
    /// The compositor and the Settings' script; `None` when the programs
    /// are not built.
    fn start(machine: Machine) -> anyhow::Result<Option<Self>> {
        let Some(program) = common::settings_program() else {
            eprintln!("hocus-settings is not built: build the workspace to run this test");
            return Ok(None);
        };
        let exe = std::env::current_exe()?;
        let Some(hocus) = exe
            .parent()
            .and_then(Path::parent)
            .map(|d| d.join("hocus"))
            .filter(|h| h.exists())
        else {
            eprintln!("hocus is not built: build the workspace to run this test");
            return Ok(None);
        };
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in [
            "home",
            "config/hocuspanda",
            "cache",
            "state/hocuspanda",
            "data",
            "bin",
        ] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let home = root.join("home");
        if machine.claude {
            std::fs::create_dir_all(home.join(".claude"))?;
            std::fs::write(home.join(".claude/settings.json"), ORIGINAL)?;
        }
        if machine.codex {
            std::fs::create_dir_all(home.join(".codex"))?;
        }
        if machine.codex_declined {
            std::fs::write(
                root.join("state/hocuspanda/agents.toml"),
                "declined = [\"codex\"]\n\n[declined-on]\ncodex = 1761822000\n",
            )?;
        }
        let herdr = if machine.herdr {
            std::fs::create_dir_all(root.join("config/herdr"))?;
            Some(fake_herdr(&root.join("config/herdr/herdr.sock"))?)
        } else {
            None
        };
        // This test's process in the test's own `/proc`: herdr's pane's.
        let mine = home.join("proc").join(std::process::id().to_string());
        std::fs::create_dir_all(&mine)?;
        std::os::unix::fs::symlink(std::env::current_dir()?, mine.join("cwd"))?;
        let file = root.join("config/hocuspanda/config.toml");
        let mut text = if machine.motion {
            FILE.to_owned()
        } else {
            format!("{FILE}\n[appearance]\nreduced-motion = true\n")
        };
        if machine.assistant {
            text.push_str("\n[secretary]\nmode = \"assistant\"\n");
        }
        if machine.notified {
            std::fs::write(
                root.join("state/hocuspanda/notifications"),
                [
                    r#"{"app":"Signal","summary":"Claire","body":"Dimanche ?","agent":false,"at":1794215520000,"unseen":true}"#,
                    r#"{"app":"Slack","entry":"slack","summary":"Karim (#prod-alertes)","body":"la prod est tombée","agent":false,"at":1794215500000,"unseen":true}"#,
                    r#"{"app":"Claude Code","summary":"waiting for your input","agent":true,"at":1794215400000,"unseen":false}"#,
                ]
                .join("\n"),
            )?;
        }
        std::fs::write(&file, &text)?;
        let log = root.join("settings.log");
        let script = root.join("bin/hocus-settings");
        let at = |sub: &str| root.join(sub).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS CODEX_HOME\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 HOCUS_POWER_SUPPLY_DIR=/nonexistent PIPEWIRE_REMOTE=/nonexistent ICED_BACKEND=tiny-skia \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 TZ=UTC HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 PATH='{}:/usr/bin:/bin' HOCUS_USER_NAME=Alex \
                 RUST_LOG=warn,hocus_settings=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("data"),
                at("bin"),
                program.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut config = Config::load(&file);
        config.keyboard.layouts = vec!["us".to_owned()];
        config.file = Some(file);
        config.state_dir = Some(root.join("state/hocuspanda"));
        config.appearance.reduced_motion = !machine.motion;
        config.agent_places = Some(Places {
            home: home.clone(),
            config_home: root.join("config"),
            codex_home: home.join(".codex"),
            path: vec![root.join("bin")],
            herdr_socket: root.join("config/herdr/herdr.sock"),
            processes: home.join("proc"),
            hocus: hocus.clone(),
            state_dir: Some(root.join("state/hocuspanda")),
        });
        let mut hp = Headless::start_with(
            HeadlessConfig {
                outputs: vec!["2560x1600@1".parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
        )?;
        hp.pointer_move(1280.0, 800.0);
        Ok(Some(Self {
            hp,
            dir,
            log,
            script,
            hocus,
            _herdr: herdr,
        }))
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn claude_file(&self) -> PathBuf {
        self.root().join("home/.claude/settings.json")
    }

    fn memory(&self) -> String {
        std::fs::read_to_string(self.root().join("state/hocuspanda/agents.toml"))
            .unwrap_or_default()
    }

    /// Launched on Agents, its window open with the keyboard, the tools
    /// known.
    fn open(&mut self) -> anyhow::Result<Value> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![self.script.display().to_string(), "agents".to_owned()],
        });
        let open = self.hp.dispatch_until(Duration::from_secs(20), |hp| {
            hp.windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some(APP_ID) && w.keyboard)
        })?;
        anyhow::ensure!(
            open,
            "the window never opened: {}",
            common::logged(&self.log)
        );
        self.shows(|s| {
            s["file"] == "saved" && s["page"] == "agents" && s["agents"]["known"] == true
        })
    }

    /// What the window shows, once `done` holds of it.
    fn shows(&mut self, done: impl Fn(&Value) -> bool) -> anyhow::Result<Value> {
        let log = self.log.clone();
        let read = |log: &Path| -> Option<Value> { common::last_json(log, " shows {") };
        let mut seen = None;
        self.hp.dispatch_until(WAIT, |_| {
            seen = read(&log).filter(|s| done(s));
            seen.is_some()
        })?;
        seen.ok_or_else(|| {
            let said = common::logged(&log);
            let trouble: Vec<&str> = said
                .lines()
                .filter(|l| l.contains("WARN") || l.contains("ERROR") || l.contains("panicked"))
                .collect();
            anyhow::anyhow!(
                "the window never showed it; it shows {}\n{}",
                read(&log).map_or_else(|| "nothing".to_owned(), |s| s.to_string()),
                trouble.join("\n")
            )
        })
    }

    /// The window's current places, once they hold `id`.
    fn places_with(&mut self, id: &str) -> anyhow::Result<Value> {
        settings_page::places_with(&mut self.hp, &self.log, id)
    }

    fn window(&self) -> anyhow::Result<Rect> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(APP_ID))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no settings window"))
    }

    fn place_of(&mut self, id: &str) -> anyhow::Result<[f64; 4]> {
        let places = self.places_with(id)?;
        let place = &places[id];
        let n = |i: usize| place[i].as_f64().unwrap_or_default();
        Ok([n(0), n(1), n(2), n(3)])
    }

    /// `id` scrolled into the window with the wheel over the page.
    fn reveal(&mut self, id: &str) -> anyhow::Result<()> {
        let mut last = self.place_of(id)?;
        let mut still = 0;
        for _ in 0..60 {
            let window = self.window()?;
            let (top, bottom) = (last[1], last[1] + last[3]);
            let notches = if bottom > f64::from(window.height) - 90.0 {
                2
            } else if top < 56.0 {
                -2
            } else {
                wait(&mut self.hp, Duration::from_millis(100))?;
                let now = self.place_of(id)?;
                if now == last {
                    return Ok(());
                }
                last = now;
                continue;
            };
            self.hp.pointer_move(
                f64::from(window.x) + 500.0,
                f64::from(window.y) + f64::from(window.height) / 2.0,
            );
            wait(&mut self.hp, Duration::from_millis(50))?;
            self.hp.wheel(notches);
            let before = last;
            let log = self.log.clone();
            let key = id.to_owned();
            let moved = self.hp.dispatch_until(Duration::from_secs(2), |_| {
                read_place(&log, &key).is_some_and(|p| p != before)
            })?;
            if !moved {
                still += 1;
                if still >= 3 {
                    return Ok(());
                }
                continue;
            }
            still = 0;
            last = self.place_of(id)?;
        }
        anyhow::bail!("{id} never came into view")
    }

    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        if !id.starts_with("option:") && !id.starts_with("sheet:") && id != "toast-action" {
            self.reveal(id)?;
        }
        let [x, y, w, h] = self.place_of(id)?;
        let window = self.window()?;
        let at = (
            f64::from(window.x) + x + w / 2.0,
            f64::from(window.y) + y + h / 2.0,
        );
        self.hp.pointer_move(at.0 - 1.0, at.1);
        self.hp.pointer_move(at.0, at.1);
        wait(&mut self.hp, Duration::from_millis(30))?;
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        Ok(())
    }

    fn choose(&mut self, setting: &str, n: usize) -> anyhow::Result<()> {
        self.click(&format!("control:{setting}"))?;
        self.shows(|s| s["menu"].as_str() == Some(setting))?;
        self.click(&format!("option:{n}"))?;
        Ok(())
    }

    fn written(&mut self, done: impl Fn(&str) -> bool) -> anyhow::Result<String> {
        let file = self.root().join("config/hocuspanda/config.toml");
        let seen = self.hp.dispatch_until(WAIT, |_| {
            std::fs::read_to_string(&file).is_ok_and(|t| done(&t))
        })?;
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        anyhow::ensure!(seen, "the file never held it:\n{text}");
        Ok(text)
    }

    fn key(&mut self, code: u32) -> anyhow::Result<()> {
        self.hp.key(code, true);
        self.hp.key(code, false);
        wait(&mut self.hp, Duration::from_millis(10))
    }

    /// `hocus agent NAME STATE`, once the compositor's model has it.
    fn agent_is(&mut self, name: &str, state: &str) -> anyhow::Result<()> {
        let socket = self
            .hp
            .control_socket()
            .ok_or_else(|| anyhow::anyhow!("no control socket"))?
            .to_path_buf();
        let mut child = std::process::Command::new(&self.hocus)
            .arg("--socket")
            .arg(socket)
            .args(["agent", name, state])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .spawn()?;
        let done = self
            .hp
            .dispatch_until(WAIT, |_| matches!(child.try_wait(), Ok(Some(_))))?;
        anyhow::ensure!(done, "hocus agent {name} {state} never ended");
        let known = self.agents_until(|a| a.iter().any(|a| a.name == name && a.state == state))?;
        anyhow::ensure!(known, "{name} not {state}");
        Ok(())
    }

    /// Waits until the compositor's agents hold `done`; whether they did.
    fn agents_until(
        &mut self,
        done: impl Fn(&[hocus_link::AgentShown]) -> bool,
    ) -> anyhow::Result<bool> {
        let start = std::time::Instant::now();
        while start.elapsed() < WAIT {
            if done(&self.hp.agents()) {
                return Ok(true);
            }
            self.hp.dispatch(Duration::from_millis(20))?;
        }
        Ok(done(&self.hp.agents()))
    }

    /// Waits until the compositor's `[agents]` holds `done`.
    fn compositor_follows(
        &mut self,
        done: impl Fn(&hocus_compositor::AgentsSettings) -> bool,
    ) -> anyhow::Result<()> {
        let followed = self
            .hp
            .dispatch_until(WAIT, |hp| done(&hp.state().config.agents))?;
        anyhow::ensure!(
            followed,
            "the compositor has {:?}",
            self.hp.state().config.agents
        );
        Ok(())
    }
}

fn read_place(log: &Path, id: &str) -> Option<[f64; 4]> {
    let places = common::last_json(log, " places {")?;
    let place = places.get(id)?;
    let n = |i: usize| place[i].as_f64();
    Some([n(0)?, n(1)?, n(2)?, n(3)?])
}

/// The tool `word`'s row as the window shows it.
fn tool<'a>(shown: &'a Value, word: &str) -> &'a Value {
    shown["agents"]["tools"]
        .as_array()
        .and_then(|t| t.iter().find(|t| t["tool"] == word))
        .unwrap_or(&Value::Null)
}

#[test]
fn the_page_lists_the_three_tools_with_their_states() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        claude: true,
        codex: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| tool(s, "codex")["row"] == "found")?;
    let rows: Vec<(&str, &str, &str)> = shown["agents"]["tools"]
        .as_array()
        .map(|t| {
            t.iter()
                .map(|t| {
                    (
                        t["tool"].as_str().unwrap_or(""),
                        t["state"].as_str().unwrap_or(""),
                        t["action"].as_str().unwrap_or(""),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        rows,
        [
            ("claude-code", "Trouvé, pas branché", "plug"),
            ("codex", "Trouvé, pas branché", "plug"),
            ("herdr", "Pas sur cet ordinateur", "none"),
        ]
    );
    assert_eq!(shown["agents"]["count"], "0 suivi · aucun agent en cours");
    assert_eq!(shown["agents"]["none"], false);
    // The halo's and the pandas' rows, the model's.
    for id in [
        "row:agents.halo",
        "row:agents.remind",
        "row:agents.preview",
        "row:agents.peek",
        "row:agents.pandas",
        "row:agents.rest",
        "row:agents.another",
        "agents:copy",
    ] {
        bed.place_of(id)?;
    }
    // An agent running is counted.
    bed.agent_is("build", "working")?;
    bed.shows(|s| s["agents"]["count"] == "0 suivi · 1 agent en cours")?;
    Ok(())
}

#[test]
fn brancher_writes_nothing_before_the_consent_then_plugs_in() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        claude: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| tool(s, "claude-code")["action"] == "plug")?;
    // The sheet, then Esc: nothing written.
    bed.click("tool:claude-code")?;
    bed.shows(|s| s["agents"]["consent"] == "claude-code" && s["agents"]["consent_on"] == "plug")?;
    bed.place_of("sheet:plug")?;
    assert_eq!(std::fs::read_to_string(bed.claude_file())?, ORIGINAL);
    bed.key(common::KEY_ESC)?;
    bed.shows(|s| s["agents"]["consent"].is_null())?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(std::fs::read_to_string(bed.claude_file())?, ORIGINAL);
    // Again, and its "Brancher": the hooks written, a copy kept.
    bed.click("tool:claude-code")?;
    bed.shows(|s| s["agents"]["consent"] == "claude-code")?;
    bed.click("sheet:plug")?;
    bed.shows(|s| {
        tool(s, "claude-code")["row"] == "plugged"
            && s["toast"] == "Claude Code branché"
            && s["agents"]["consent"].is_null()
    })?;
    let now = std::fs::read_to_string(bed.claude_file())?;
    assert!(now.contains("agent-hook claude-code"), "{now}");
    assert_eq!(
        std::fs::read_to_string(
            bed.root()
                .join("home/.claude/settings.json.before-hocuspanda")
        )?,
        ORIGINAL
    );
    bed.shows(|s| s["agents"]["count"] == "1 suivi · aucun agent en cours")?;
    Ok(())
}

#[test]
fn annuler_after_debrancher_plugs_it_back() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        claude: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.click("tool:claude-code")?;
    bed.shows(|s| s["agents"]["consent"] == "claude-code")?;
    bed.key(28)?; // Enter: "Brancher" holds the keyboard.
    bed.shows(|s| tool(s, "claude-code")["action"] == "unplug")?;
    // "Débrancher": no consent, the file as it was.
    bed.click("tool:claude-code")?;
    bed.shows(|s| {
        tool(s, "claude-code")["action"] == "plug"
            && s["toast"] == "Claude Code débranché"
            && s["agents"]["consent"].is_null()
    })?;
    assert_eq!(std::fs::read_to_string(bed.claude_file())?, ORIGINAL);
    // "Annuler": plugged back in, without asking.
    bed.click("toast-action")?;
    bed.shows(|s| {
        tool(s, "claude-code")["action"] == "unplug" && s["agents"]["consent"].is_null()
    })?;
    assert!(std::fs::read_to_string(bed.claude_file())?.contains("agent-hook claude-code"));
    Ok(())
}

#[test]
fn a_declined_tool_is_plugged_in_from_the_page_and_its_refusal_cleared() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        codex: true,
        codex_declined: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| {
        tool(s, "codex")["row"] == "declined" && tool(s, "codex")["declined_on"].is_i64()
    })?;
    assert_eq!(tool(&shown, "codex")["state"], "Refusé");
    assert_eq!(tool(&shown, "codex")["action"], "plug");
    bed.click("tool:codex")?;
    bed.shows(|s| s["agents"]["consent"] == "codex")?;
    bed.click("sheet:plug")?;
    // Plugged, its hooks to approve in Codex; no longer declined.
    let shown = bed.shows(|s| tool(s, "codex")["row"] == "to-approve")?;
    assert_eq!(tool(&shown, "codex")["state"], "Branché · à approuver");
    let config = std::fs::read_to_string(bed.root().join("home/.codex/config.toml"))?;
    assert!(config.contains("agent-hook"), "{config}");
    let memory = bed.memory();
    assert!(memory.contains("declined = []"), "{memory}");
    assert!(!memory.contains("codex = "), "{memory}");
    Ok(())
}

#[test]
fn following_herdr_off_declines_it_and_its_agents_go() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        herdr: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| {
        tool(s, "herdr")["row"] == "running" && tool(s, "herdr")["action"] == "follow-on"
    })?;
    let followed = bed.agents_until(|a| !a.is_empty())?;
    anyhow::ensure!(followed, "herdr's agent never came");
    bed.shows(|s| s["agents"]["count"] == "1 suivi · 1 agent en cours")?;
    bed.click("tool:herdr")?;
    bed.shows(|s| {
        tool(s, "herdr")["row"] == "running-ignored"
            && tool(s, "herdr")["action"] == "follow-off"
            && s["toast"] == "herdr n’est plus suivi"
    })?;
    let gone = bed.agents_until(<[_]>::is_empty)?;
    anyhow::ensure!(gone, "herdr's agents stay: {:?}", bed.hp.agents());
    assert!(bed.memory().contains("\"herdr\""), "{}", bed.memory());
    // On again: followed, its agent back.
    bed.click("tool:herdr")?;
    bed.shows(|s| tool(s, "herdr")["action"] == "follow-on")?;
    let back = bed.agents_until(|a| !a.is_empty())?;
    anyhow::ensure!(back, "herdr's agent never came back");
    Ok(())
}

#[test]
fn halo_waiting_only_shows_no_glow_for_a_finished_agent() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine::default())? else {
        return Ok(());
    };
    bed.open()?;
    bed.choose("agents.halo", 1)?;
    bed.written(|t| t.contains("[agents]\nhalo = \"waiting\""))?;
    bed.shows(|s| {
        s["toast"]
            .as_str()
            .is_some_and(|t| t.starts_with("Le halo"))
    })?;
    bed.compositor_follows(|a| a.halo == HaloRule::Waiting)?;
    bed.agent_is("api", "done")?;
    assert_eq!(bed.hp.halo_concern(), None);
    bed.agent_is("infra", "waiting")?;
    let lit = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.halo_concern() == Some(Concern::Waiting))?;
    anyhow::ensure!(lit, "no halo for a waiting agent");
    // The preview: no halo for a finished agent, one for a waiting one.
    bed.click("preview:done")?;
    bed.shows(|s| s["agents"]["played"] == "done" && s["agents"]["preview_halo"] == false)?;
    bed.click("preview:waiting")?;
    bed.shows(|s| s["agents"]["played"] == "waiting" && s["agents"]["preview_halo"] == true)?;
    // Never: the reminder's row is inert.
    bed.choose("agents.halo", 2)?;
    bed.shows(|s| s["agents"]["remind_inert"] == true)?;
    bed.compositor_follows(|a| a.halo == HaloRule::Off)?;
    Ok(())
}

#[test]
fn peek_off_never_shows_the_panda() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine::default())? else {
        return Ok(());
    };
    bed.open()?;
    bed.choose("agents.peek", 2)?;
    bed.written(|t| t.contains("peek = \"off\""))?;
    bed.compositor_follows(|a| a.peek == PeekRule::Off)?;
    // The reminder's menu, written as the compositor reads it.
    bed.choose("agents.remind", 0)?;
    bed.written(|t| t.contains("remind = \"2m\""))?;
    bed.compositor_follows(|a| a.remind == hocus_compositor::Remind::Two)?;
    Ok(())
}

#[test]
fn pandas_off_draws_dots_in_the_rows() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine::default())? else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(shown["agents"]["pandas"], true);
    bed.click("control:agents.pandas")?;
    bed.written(|t| t.contains("pandas = false"))?;
    bed.shows(|s| s["agents"]["pandas"] == false)?;
    bed.compositor_follows(|a| !a.pandas)?;
    Ok(())
}

/// Her consent book, in the test's data folder.
fn consents(bed: &Bed) -> String {
    std::fs::read_to_string(bed.root().join("data/hocuspanda/secretary/consents.json"))
        .unwrap_or_default()
}

fn notifier<'a>(shown: &'a Value, key: &str) -> &'a Value {
    shown["agents"]["notifiers"]
        .as_array()
        .and_then(|n| n.iter().find(|n| n["key"] == key))
        .unwrap_or(&Value::Null)
}

/// Lot 7: the applications that notified show under her mode in
/// assistant mode only (an agents' tool's own left out), Slack first.
#[test]
fn the_notifying_applications_show_under_the_secretary_in_assistant_mode_only() -> anyhow::Result<()>
{
    let Some(mut bed) = Bed::start(Machine {
        notified: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| {
        s["agents"]["notifiers"]
            .as_array()
            .is_some_and(|n| n.len() == 2)
    })?;
    assert_eq!(shown["agents"]["assistant"], false);
    let places = bed.places_with("control:secretary.mode")?;
    assert!(places.get("notifier:slack").is_none(), "classic: no row");
    bed.click("control:secretary.mode")?;
    let shown = bed.shows(|s| s["agents"]["assistant"] == true)?;
    let keys: Vec<&str> = shown["agents"]["notifiers"]
        .as_array()
        .map(|n| n.iter().filter_map(|n| n["key"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(keys, ["slack", "signal"]);
    bed.place_of("notifier:slack")?;
    assert!(consents(&bed).is_empty(), "nothing consented by itself");
    Ok(())
}

/// Lot 7: "Autoriser" writes nothing before its sheet's "Autoriser",
/// then a grant in her consent book, in the notebook chosen; "Pas
/// maintenant" nothing; "Ne plus lire" a revocation.
#[test]
fn autoriser_writes_a_grant_only_from_its_sheet_and_ne_plus_lire_a_revocation() -> anyhow::Result<()>
{
    let Some(mut bed) = Bed::start(Machine {
        assistant: true,
        notified: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| notifier(s, "slack")["granted"].is_null() && notifier(s, "signal").is_object())?;
    bed.click("notifier:signal")?;
    bed.shows(|s| s["agents"]["notify"] == "Signal")?;
    bed.click("sheet:not-now")?;
    bed.shows(|s| s["agents"]["notify"].is_null())?;
    assert!(
        consents(&bed).is_empty(),
        "nothing before the sheet's Autoriser"
    );

    bed.click("notifier:slack")?;
    let shown = bed.shows(|s| s["agents"]["notify"] == "Slack")?;
    assert_eq!(shown["agents"]["notify_notebook"], "pro", "Slack's is pro");
    assert!(consents(&bed).is_empty());
    bed.click("sheet:allow")?;
    bed.shows(|s| notifier(s, "slack")["granted"] == "pro")?;
    let book: hocus_secretary_link::consent::ConsentBook = serde_json::from_str(&consents(&bed))?;
    let grant = book.notifications("Slack", Some("slack")).expect("granted");
    assert_eq!(grant.notebook(), hocus_secretary_link::Notebook::Pro);
    assert!(book.notifications("Signal", None).is_none());

    bed.click("notifier:signal")?;
    bed.shows(|s| s["agents"]["notify"] == "Signal")?;
    bed.click("sheet:perso")?;
    bed.shows(|s| s["agents"]["notify_notebook"] == "perso")?;
    bed.click("sheet:allow")?;
    bed.shows(|s| notifier(s, "signal")["granted"] == "perso")?;

    bed.click("notifier:slack")?;
    bed.shows(|s| notifier(s, "slack")["granted"].is_null())?;
    let book: hocus_secretary_link::consent::ConsentBook = serde_json::from_str(&consents(&bed))?;
    assert!(book.notifications("Slack", None).is_none(), "revoked");
    assert_eq!(
        book.notifications("Signal", None).map(|g| g.notebook()),
        Some(hocus_secretary_link::Notebook::Perso)
    );
    assert_eq!(book.records().len(), 3, "a revocation is a later record");
    Ok(())
}

/// Lot 7's screenshots, into `HOCUS_SHOTS`.
#[test]
#[ignore = "screenshots for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_of_the_secretary_s_notifications() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let Some(mut bed) = Bed::start(Machine {
        claude: true,
        assistant: true,
        notified: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| notifier(s, "slack").is_object())?;
    bed.reveal("row:agents.notifier.signal")?;
    save(&mut bed, &out, "secretary-notifications.png")?;
    bed.click("notifier:slack")?;
    bed.shows(|s| s["agents"]["notify"] == "Slack")?;
    save(&mut bed, &out, "secretary-notifications-sheet.png")?;
    bed.click("sheet:allow")?;
    bed.shows(|s| notifier(s, "slack")["granted"] == "pro")?;
    bed.reveal("row:agents.notifier.signal")?;
    save(&mut bed, &out, "secretary-notifications-allowed.png")?;
    Ok(())
}

/// The secretary's switch (secretary.md §21): `[secretary] mode` written
/// "assistant", then "classic" again, the compositor following.
#[test]
fn the_secretary_s_switch_writes_her_mode() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine::default())? else {
        return Ok(());
    };
    bed.open()?;
    bed.click("control:secretary.mode")?;
    bed.written(|t| t.contains("[secretary]") && t.contains("mode = \"assistant\""))?;
    let followed = bed.hp.dispatch_until(WAIT, |hp| {
        hp.state().config.secretary.mode == hocus_compositor::SecretaryMode::Assistant
    })?;
    assert!(followed, "the compositor follows");
    bed.click("control:secretary.mode")?;
    bed.written(|t| t.contains("mode = \"classic\""))?;
    Ok(())
}

#[test]
fn no_tool_shows_the_dashed_line_and_the_command() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine::default())? else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| s["agents"]["none"] == true)?;
    assert_eq!(shown["agents"]["count"], "0 suivi · aucun agent en cours");
    bed.place_of("agents:none")?;
    bed.click("agents:copy")?;
    bed.shows(|s| s["agents"]["copied"] == true)?;
    Ok(())
}

/// At rest on the page, the window draws nothing: its pandas hopped as it
/// opened, the preview played its wave, then nothing moves.
#[test]
fn at_rest_the_page_commits_nothing() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        claude: true,
        motion: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.click("preview:waiting")?;
    bed.shows(|s| s["agents"]["played"] == "waiting")?;
    let mut last = bed.hp.window_commits();
    let mut still = 0;
    for _ in 0..200 {
        wait(&mut bed.hp, Duration::from_millis(100))?;
        let now = bed.hp.window_commits();
        still = if now == last { still + 1 } else { 0 };
        last = now;
        if still >= 10 {
            break;
        }
    }
    anyhow::ensure!(still >= 10, "the window never came to rest");
    wait(&mut bed.hp, Duration::from_secs(3))?;
    assert_eq!(bed.hp.window_commits(), last, "the window drew at rest");
    Ok(())
}

/// The window as drawn, once it rests, saved as `out/name`.
fn save(bed: &mut Bed, out: &Path, name: &str) -> anyhow::Result<()> {
    bed.hp.pointer_move(5.0, 5.0);
    let mut last = bed.hp.window_commits();
    let mut still = 0;
    for _ in 0..60 {
        if still >= 5 {
            break;
        }
        wait(&mut bed.hp, Duration::from_millis(100))?;
        let now = bed.hp.window_commits();
        still = if now == last { still + 1 } else { 0 };
        last = now;
    }
    let window = bed.window()?;
    let (w, h, bytes) = bed.hp.screenshot(0)?;
    let rgba: Vec<u8> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
        .ok_or_else(|| anyhow::anyhow!("no image"))?;
    let image = image::imageops::crop_imm(
        &image,
        window.x.max(0) as u32,
        window.y.max(0) as u32,
        window.width as u32,
        window.height as u32,
    )
    .to_image();
    image.save(out.join(name))?;
    Ok(())
}

/// The pull request's screenshots, into `HOCUS_SHOTS`.
#[test]
#[ignore = "screenshots for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let Some(mut bed) = Bed::start(Machine {
        claude: true,
        codex: true,
        codex_declined: true,
        herdr: true,
        motion: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| {
        tool(s, "codex")["declined_on"].is_i64() && tool(s, "herdr")["row"] == "running"
    })?;
    save(&mut bed, &out, "agents-page.png")?;
    bed.click("tool:claude-code")?;
    bed.shows(|s| s["agents"]["consent"] == "claude-code")?;
    save(&mut bed, &out, "agents-sheet.png")?;
    bed.click("sheet:plug")?;
    bed.shows(|s| tool(s, "claude-code")["row"] == "plugged")?;
    save(&mut bed, &out, "agents-plugged.png")?;
    bed.click("preview:waiting")?;
    bed.shows(|s| s["agents"]["played"] == "waiting")?;
    save(&mut bed, &out, "agents-halo.png")?;
    bed.choose("agents.halo", 1)?;
    bed.written(|t| t.contains("halo = \"waiting\""))?;
    bed.click("preview:done")?;
    bed.shows(|s| s["agents"]["preview_halo"] == false)?;
    save(&mut bed, &out, "agents-halo-waiting-only.png")?;
    bed.click("control:agents.pandas")?;
    bed.shows(|s| s["agents"]["pandas"] == false)?;
    bed.reveal("row:agents.rest")?;
    save(&mut bed, &out, "agents-pandas-dots.png")?;
    bed.click("control:agents.pandas")?;
    bed.shows(|s| s["agents"]["pandas"] == true)?;
    save(&mut bed, &out, "agents-pandas.png")?;
    let Some(mut none) = Bed::start(Machine {
        motion: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    none.open()?;
    none.shows(|s| s["agents"]["none"] == true)?;
    save(&mut none, &out, "agents-none.png")?;
    Ok(())
}
