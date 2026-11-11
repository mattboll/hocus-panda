//! The Settings' window on a headless compositor whose agents' tools are
//! the test's own: the bed of the Agents page's tests (6.16).

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::{Path, PathBuf},
    time::Duration,
};

use crate::common::{self, BTN_LEFT, WAIT, wait};
use crate::settings_page;
use hocus_agents::tools::Places;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use serde_json::Value;

pub const APP_ID: &str = "hocus-settings";
pub const FILE: &str = "# By hand.\n[keyboard]\nlayouts = [\"us\"]\n";
/// Claude Code's settings before the page plugs it in.
pub const ORIGINAL: &str = "{\n  \"model\": \"opus\"\n}\n";

/// What the test's machine has.
#[derive(Default, Clone, Copy)]
pub struct Machine {
    pub claude: bool,
    pub codex: bool,
    /// herdr runs, with one agent working.
    pub herdr: bool,
    /// Codex declined with "Ne rien brancher" before.
    pub codex_declined: bool,
    /// The window moves (else reduced motion, as the file says).
    pub motion: bool,
}

pub struct Bed {
    pub hp: Headless,
    pub dir: tempfile::TempDir,
    pub log: PathBuf,
    pub script: PathBuf,
    pub hocus: PathBuf,
    pub _herdr: Option<std::thread::JoinHandle<()>>,
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
    pub fn start(machine: Machine) -> anyhow::Result<Option<Self>> {
        let program = common::settings_program();
        let hocus = common::hocus_program();
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
        let text = if machine.motion {
            FILE.to_owned()
        } else {
            format!("{FILE}\n[appearance]\nreduced-motion = true\n")
        };
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

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn claude_file(&self) -> PathBuf {
        self.root().join("home/.claude/settings.json")
    }

    pub fn memory(&self) -> String {
        std::fs::read_to_string(self.root().join("state/hocuspanda/agents.toml"))
            .unwrap_or_default()
    }

    /// Launched on Agents, its window open with the keyboard, the tools
    /// known.
    pub fn open(&mut self) -> anyhow::Result<Value> {
        self.launch("agents")?;
        self.shows(|s| {
            s["file"] == "saved" && s["page"] == "agents" && s["agents"]["known"] == true
        })
    }

    /// Launched on `page`, its window open with the keyboard.
    fn launch(&mut self, page: &str) -> anyhow::Result<()> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![self.script.display().to_string(), page.to_owned()],
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
        Ok(())
    }

    /// What the window shows, once `done` holds of it.
    pub fn shows(&mut self, done: impl Fn(&Value) -> bool) -> anyhow::Result<Value> {
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
    pub fn places_with(&mut self, id: &str) -> anyhow::Result<Value> {
        settings_page::places_with(&mut self.hp, &self.log, id)
    }

    pub fn window(&self) -> anyhow::Result<Rect> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(APP_ID))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no settings window"))
    }

    pub fn place_of(&mut self, id: &str) -> anyhow::Result<[f64; 4]> {
        let places = self.places_with(id)?;
        let place = &places[id];
        let n = |i: usize| place[i].as_f64().unwrap_or_default();
        Ok([n(0), n(1), n(2), n(3)])
    }

    /// `id` scrolled into the window with the wheel over the page.
    pub fn reveal(&mut self, id: &str) -> anyhow::Result<()> {
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

    pub fn click(&mut self, id: &str) -> anyhow::Result<()> {
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

    pub fn choose(&mut self, setting: &str, n: usize) -> anyhow::Result<()> {
        self.click(&format!("control:{setting}"))?;
        self.shows(|s| s["menu"].as_str() == Some(setting))?;
        self.click(&format!("option:{n}"))?;
        Ok(())
    }

    pub fn written(&mut self, done: impl Fn(&str) -> bool) -> anyhow::Result<String> {
        let file = self.root().join("config/hocuspanda/config.toml");
        let seen = self.hp.dispatch_until(WAIT, |_| {
            std::fs::read_to_string(&file).is_ok_and(|t| done(&t))
        })?;
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        anyhow::ensure!(seen, "the file never held it:\n{text}");
        Ok(text)
    }

    pub fn key(&mut self, code: u32) -> anyhow::Result<()> {
        self.hp.key(code, true);
        self.hp.key(code, false);
        wait(&mut self.hp, Duration::from_millis(10))
    }

    /// `hocus agent NAME STATE`, once the compositor's model has it.
    pub fn agent_is(&mut self, name: &str, state: &str) -> anyhow::Result<()> {
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
    pub fn agents_until(
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
    pub fn compositor_follows(
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

pub fn read_place(log: &Path, id: &str) -> Option<[f64; 4]> {
    let places = common::last_json(log, " places {")?;
    let place = places.get(id)?;
    let n = |i: usize| place[i].as_f64();
    Some([n(0)?, n(1)?, n(2)?, n(3)?])
}

/// The window as drawn, once it rests, saved as `out/name`.
pub fn save(bed: &mut Bed, out: &Path, name: &str) -> anyhow::Result<()> {
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
