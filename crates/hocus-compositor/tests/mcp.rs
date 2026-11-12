//! Agents on the desktop through `hocus mcp` (docs/design/mcp.md, #440
//! lot A): each test talks MCP to a real `hocus mcp` process on its
//! standard input and output, against a headless compositor whose
//! configuration names two agents: `Claude`, who acts and changes
//! settings, and `Looker`, who only reads. The toast's test runs the real
//! shell on the CPU, in French, in a home of the test's own. Each test
//! waits for what it checks, never for a time.

use crate::common;

use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
};

use common::{KEY_ESC, KEY_LEFTMETA, KEY_LEFTSHIFT, WAIT};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice};
use serde_json::{Value, json};

const CONFIG: &str = "[[agents.mcp]]\nname = \"Claude\"\naccess = \"act\"\nsettings = true\n\n\
                      [[agents.mcp]]\nname = \"Looker\"\n";

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    file: PathBuf,
    log: PathBuf,
}

/// The compositor on a 1920 × 1080 screen, its configuration [`CONFIG`];
/// with the real shell when `shell`.
fn bed(shell: bool) -> anyhow::Result<Bed> {
    bed_with(shell, CONFIG)
}

/// The same, its configuration `text`.
fn bed_with(shell: bool, text: &str) -> anyhow::Result<Bed> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    for sub in ["home", "config", "state/hocuspanda", "cache"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    std::fs::write(root.join("state/hocuspanda/day-sources"), "later\n")?;
    let file = root.join("config/config.toml");
    std::fs::write(&file, text)?;
    let log = root.join("shell.log");
    let config = Config {
        file: Some(file.clone()),
        agents: toml::from_str::<Config>(text)?.agents,
        plain_background: true,
        lang: hocus_words::Lang::French,
        ..Config::default()
    };
    let headless = HeadlessConfig {
        outputs: vec!["1920x1080".parse()?],
        renderer: RendererChoice::Pixman,
        render_node: None,
    };
    let hp = if shell {
        let program = common::shell_program();
        let at = |p: &str| root.join(p).display().to_string();
        let script = common::shell_script(
            root,
            &program,
            &[
                ("HOME", at("home")),
                ("XDG_CONFIG_HOME", at("config")),
                ("XDG_STATE_HOME", at("state")),
                ("XDG_CACHE_HOME", at("cache")),
                ("LC_ALL", "fr_FR.UTF-8".to_owned()),
                ("LANG", "fr_FR.UTF-8".to_owned()),
                ("ICED_BACKEND", "tiny-skia".to_owned()),
                (
                    "RUST_LOG",
                    "warn,hocuspanda_shell=debug,hocuspanda_shell::shows=debug".to_owned(),
                ),
            ],
        )?;
        // Its log beside it.
        let wrapped = root.join("shell-logged");
        std::fs::write(
            &wrapped,
            format!(
                "#!/bin/sh\nexec '{}' \"$@\" 2>>'{}'\n",
                script.display(),
                log.display()
            ),
        )?;
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&wrapped, std::fs::Permissions::from_mode(0o755))?;
        }
        let mut hp = Headless::start_with_shell(headless, config, Some(wrapped))?;
        let up = hp.dispatch_until(WAIT, |hp| hp.bar_commits() > 0)?;
        anyhow::ensure!(up, "no bar");
        hp
    } else {
        Headless::start_with(headless, config)?
    };
    Ok(Bed {
        _dir: dir,
        hp,
        file,
        log,
    })
}

/// A `hocus mcp --agent NAME` process, and what it says, line by line.
struct Mcp {
    child: Child,
    input: ChildStdin,
    said: mpsc::Receiver<Value>,
    next: u64,
}

impl Drop for Mcp {
    fn drop(&mut self) {
        // Ours, by its own handle.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn mcp(hp: &Headless, agent: &str) -> anyhow::Result<Mcp> {
    let socket = hp
        .control_socket()
        .ok_or_else(|| anyhow::anyhow!("no control socket"))?;
    let mut child = Command::new(common::hocus_program())
        .arg("--socket")
        .arg(socket)
        .args(["mcp", "--agent", agent])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let input = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("no stdin"))?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("no stdout"))?;
    let (to, said) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else { return };
            if let Ok(value) = serde_json::from_str::<Value>(&line)
                && to.send(value).is_err()
            {
                return;
            }
        }
    });
    Ok(Mcp {
        child,
        input,
        said,
        next: 0,
    })
}

impl Mcp {
    /// Sends request `method`; its id.
    fn send(&mut self, method: &str, params: Value) -> anyhow::Result<u64> {
        self.next += 1;
        let line = json!({ "jsonrpc": "2.0", "id": self.next, "method": method, "params": params });
        writeln!(self.input, "{line}")?;
        self.input.flush()?;
        Ok(self.next)
    }

    /// The answer to request `id`, the compositor running meanwhile.
    fn answer(&mut self, hp: &mut Headless, id: u64) -> anyhow::Result<Value> {
        let mut found = None;
        let said = &self.said;
        let done = hp.dispatch_until(WAIT, |_| {
            while let Ok(value) = said.try_recv() {
                if value["id"] == id {
                    found = Some(value);
                }
            }
            found.is_some()
        })?;
        anyhow::ensure!(done, "no answer to request {id}");
        found.ok_or_else(|| anyhow::anyhow!("no answer"))
    }

    fn ask(&mut self, hp: &mut Headless, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.send(method, params)?;
        self.answer(hp, id)
    }

    /// Tool `name`'s result.
    fn tool(&mut self, hp: &mut Headless, name: &str, args: Value) -> anyhow::Result<Value> {
        let answer = self.ask(hp, "tools/call", json!({ "name": name, "arguments": args }))?;
        Ok(answer["result"].clone())
    }
}

/// A result's text.
fn text(result: &Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn initialize_then_the_tools() -> anyhow::Result<()> {
    let mut bed = bed(false)?;
    let mut agent = mcp(&bed.hp, "Claude")?;
    let init = agent.ask(
        &mut bed.hp,
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } }),
    )?;
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18", "{init}");
    assert_eq!(init["result"]["serverInfo"]["name"], "hocuspanda");
    let tools = agent.ask(&mut bed.hp, "tools/list", json!({}))?;
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .map(|t| t.iter().filter_map(|t| t["name"].as_str()).collect())
        .unwrap_or_default();
    for name in [
        "list_windows",
        "list_desktops",
        "list_outputs",
        "get_settings",
        "focus_window",
        "move_window",
        "switch_desktop",
        "launch",
        "set_setting",
    ] {
        assert!(names.contains(&name), "{name} not in {names:?}");
    }
    assert!(!names.contains(&"close_window"), "lot B's");
    Ok(())
}

#[test]
fn list_windows_says_each_window() -> anyhow::Result<()> {
    let mut bed = bed(false)?;
    let rect = common::open(&mut bed.hp, "foot", (640, 400))?;
    let mut agent = mcp(&bed.hp, "Looker")?;
    let result = agent.tool(&mut bed.hp, "list_windows", json!({}))?;
    assert_eq!(result["isError"], false, "{result}");
    let windows = result["structuredContent"]["windows"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let foot = windows
        .iter()
        .find(|w| w["app_id"] == "foot")
        .ok_or_else(|| anyhow::anyhow!("no foot in {windows:?}"))?;
    assert_eq!(foot["desktop"], 1);
    assert_eq!(foot["focused"], true);
    assert_eq!(foot["state"], "floating");
    assert_eq!(foot["output"], "HEADLESS-1");
    assert_eq!(
        foot["geometry"],
        json!({ "x": rect.x, "y": rect.y, "width": rect.width, "height": rect.height })
    );
    Ok(())
}

#[test]
fn a_read_agent_and_an_unknown_one_are_refused() -> anyhow::Result<()> {
    let mut bed = bed(false)?;
    let rect = common::open(&mut bed.hp, "foot", (640, 400))?;
    let id = bed.hp.windows()[0].id.0;
    let mut looker = mcp(&bed.hp, "Looker")?;
    let result = looker.tool(
        &mut bed.hp,
        "move_window",
        json!({ "window": id, "x": 0, "y": 0, "intent": "À gauche" }),
    )?;
    assert_eq!(result["isError"], true, "{result}");
    assert!(text(&result).contains("may only read"), "{result}");
    assert_eq!(common::rect_of(&bed.hp, "foot"), Some(rect));
    let refused = bed.hp.mcp_shown().journal;
    assert!(
        matches!(
            refused.first().map(|l| &l.said),
            Some(hocus_link::McpSaid::Refused {
                why: hocus_link::McpRefusal::ReadOnly,
                ..
            })
        ),
        "{refused:?}"
    );
    let mut nobody = mcp(&bed.hp, "Nobody")?;
    let result = nobody.tool(&mut bed.hp, "list_windows", json!({}))?;
    assert_eq!(result["isError"], true, "{result}");
    assert!(text(&result).contains("[[agents.mcp]]"), "{result}");
    Ok(())
}

#[test]
fn a_move_shows_its_intent_first_then_the_window_moves() -> anyhow::Result<()> {
    let mut bed = bed(false)?;
    let rect = common::open(&mut bed.hp, "foot", (640, 400))?;
    let id = bed.hp.windows()[0].id.0;
    let mut agent = mcp(&bed.hp, "Claude")?;
    let asked = agent.send(
        "tools/call",
        json!({ "name": "move_window", "arguments": {
            "window": id, "x": 40, "y": 60, "intent": "Range le terminal à gauche"
        } }),
    )?;
    // Shown, not done yet.
    let shown = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.mcp_shown().show.is_some())?;
    anyhow::ensure!(shown, "the intent never showed");
    let show = bed
        .hp
        .mcp_shown()
        .show
        .ok_or_else(|| anyhow::anyhow!("gone"))?;
    assert_eq!(show.agent, "Claude");
    assert_eq!(show.intent, "Range le terminal à gauche");
    assert!(
        matches!(show.at, hocus_link::McpAt::Window { .. }),
        "{show:?}"
    );
    assert_eq!(
        common::rect_of(&bed.hp, "foot"),
        Some(rect),
        "moved before shown"
    );
    // Then done, and the tool answers.
    let answer = agent.answer(&mut bed.hp, asked)?;
    assert_eq!(answer["result"]["isError"], false, "{answer}");
    assert!(bed.hp.mcp_shown().show.is_none());
    let moved = hocus_compositor::model::Rect::new(40, 60, rect.width, rect.height);
    common::wait_for_rect(&mut bed.hp, "foot", moved)?;
    // The request settles: its toast.
    let toast = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.mcp_shown().toast.is_some())?;
    anyhow::ensure!(toast, "no toast");
    let toast = bed
        .hp
        .mcp_shown()
        .toast
        .ok_or_else(|| anyhow::anyhow!("gone"))?;
    assert_eq!(toast.summary.moved, 1);
    assert!(toast.undo);
    Ok(())
}

/// Where the kit's control `id` of surface `namespace` is on the screen,
/// as the shell's log last said it.
fn control_at(bed: &mut Bed, namespace: &str, id: &str) -> anyhow::Result<(f64, f64)> {
    let log = bed.log.clone();
    let place = |log: &Path| -> Option<(f64, f64)> {
        let at = common::last_json(log, " shows {")?
            .get(id)?
            .get("at")?
            .clone();
        let n = |i: usize| at.get(i).and_then(Value::as_f64);
        Some((n(0)? + n(2)? / 2.0, n(1)? + n(3)? / 2.0))
    };
    let found = bed.hp.dispatch_until(WAIT, |hp| {
        place(&log).is_some() && !hp.drawn_layers(namespace).is_empty()
    })?;
    anyhow::ensure!(found, "no {id} drawn");
    let (x, y) = place(&log).unwrap_or_default();
    let surface = bed.hp.drawn_layers(namespace)[0].1;
    Ok((f64::from(surface.x) + x, f64::from(surface.y) + y))
}

#[test]
fn a_setting_changed_goes_back_with_the_toasts_annuler() -> anyhow::Result<()> {
    let mut bed = bed(true)?;
    let mut agent = mcp(&bed.hp, "Claude")?;
    let result = agent.tool(
        &mut bed.hp,
        "set_setting",
        json!({ "key": "idle.delay", "value": 600, "intent": "Écran allumé plus longtemps" }),
    )?;
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(result["structuredContent"]["before"], Value::Null);
    assert_eq!(result["structuredContent"]["after"], 600);
    let written = std::fs::read_to_string(&bed.file)?;
    assert!(written.contains("[idle]\ndelay = 600"), "{written}");
    assert!(written.starts_with(CONFIG), "the rest kept: {written}");
    // A key of the wrong type, one that is not there, one kept from
    // agents: refused, the file as it was.
    for (key, value) in [
        ("idle.delay", json!("long")),
        ("idle.nope", json!(1)),
        ("agents.halo", json!("off")),
    ] {
        let refused = agent.tool(
            &mut bed.hp,
            "set_setting",
            json!({ "key": key, "value": value, "intent": "x" }),
        )?;
        assert_eq!(refused["isError"], true, "{key}: {refused}");
    }
    assert_eq!(std::fs::read_to_string(&bed.file)?, written);
    // The toast, then its Annuler.
    let at = control_at(&mut bed, hocus_link::MCP_TOAST_NAMESPACE, "mcp-toast:Undo")?;
    common::click_on(&mut bed.hp, at, hocus_link::MCP_TOAST_NAMESPACE)?;
    let file = bed.file.clone();
    let back = bed.hp.dispatch_until(WAIT, |_| {
        std::fs::read_to_string(&file).is_ok_and(|t| !t.contains("delay"))
    })?;
    anyhow::ensure!(
        back,
        "not put back: {}",
        std::fs::read_to_string(&bed.file)?
    );
    let gone = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.mcp_shown().toast.is_none())?;
    anyhow::ensure!(gone, "the toast stayed");
    Ok(())
}

#[test]
fn the_stop_key_refuses_every_call_until_pressed_again() -> anyhow::Result<()> {
    let mut bed = bed(false)?;
    let mut agent = mcp(&bed.hp, "Claude")?;
    let chord = |hp: &mut Headless| common::chord(hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_ESC]);
    chord(&mut bed.hp);
    let stopped = bed.hp.dispatch_until(WAIT, |hp| hp.mcp_shown().stopped)?;
    anyhow::ensure!(stopped, "not stopped");
    let result = agent.tool(&mut bed.hp, "list_windows", json!({}))?;
    assert_eq!(result["isError"], true, "{result}");
    assert!(text(&result).contains("stopped every agent"), "{result}");
    chord(&mut bed.hp);
    let going = bed.hp.dispatch_until(WAIT, |hp| !hp.mcp_shown().stopped)?;
    anyhow::ensure!(going, "still stopped");
    let result = agent.tool(&mut bed.hp, "list_windows", json!({}))?;
    assert_eq!(result["isError"], false, "{result}");
    Ok(())
}

/// The screen as it shows now, saved as `name.png` in `out`.
fn shoot(hp: &mut Headless, out: &Path, name: &str) -> anyhow::Result<()> {
    common::at_rest(hp)?;
    let (w, h, px) = hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(u32::try_from(w)?, u32::try_from(h)?, rgba)
        .ok_or_else(|| anyhow::anyhow!("no image"))?;
    std::fs::create_dir_all(out)?;
    let file = out.join(format!("{name}.png"));
    image.save(&file)?;
    eprintln!("{}", file.display());
    Ok(())
}

/// The mockup's scenes 1, 2 and 4 (tools' `mockups/s14-mcp`), dark, ×1,
/// for the maintainer's eyes: `HOCUS_SHOTS=DIR cargo nextest run -p
/// hocus-compositor mcp::shots --run-ignored only`.
#[test]
#[ignore = "screenshots, on demand"]
fn shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        eprintln!("HOCUS_SHOTS is not set: no shots");
        return Ok(());
    };
    // Long enough to be caught on screen.
    let config = format!("[agents]\nshow-before-ms = 3000\n{CONFIG}");
    let mut bed = bed_with(true, &config)?;
    common::open(&mut bed.hp, "kitty", (520, 320))?;
    common::open(&mut bed.hp, "firefox", (700, 560))?;
    common::open(&mut bed.hp, "kitty-2", (520, 320))?;
    let id_of = |hp: &Headless, app: &str| {
        hp.windows()
            .iter()
            .find(|w| w.app_id.as_deref() == Some(app))
            .map(|w| w.id.0)
            .unwrap_or_default()
    };
    let (first, second) = (id_of(&bed.hp, "kitty"), id_of(&bed.hp, "kitty-2"));
    let browser = id_of(&bed.hp, "firefox");
    use hocus_compositor::model::{Rect, WindowId};
    for (id, rect) in [
        (first, Rect::new(380, 120, 620, 420)),
        (second, Rect::new(380, 600, 620, 400)),
        (browser, Rect::new(1040, 200, 600, 640)),
    ] {
        bed.hp.place_window(WindowId(id), rect);
    }
    common::at_rest(&mut bed.hp)?;
    let mut claude = mcp(&bed.hp, "Claude")?;
    // 1: shown before it acts.
    let asked = claude.send(
        "tools/call",
        json!({ "name": "move_window", "arguments": {
            "window": first, "x": 24, "y": 24, "width": 620, "height": 480,
            "intent": "Range les deux terminaux à gauche, l'un sous l'autre"
        } }),
    )?;
    let tagged = bed.hp.dispatch_until(WAIT, |hp| {
        !hp.drawn_layers(hocus_link::MCP_TAG_NAMESPACE).is_empty()
    })?;
    anyhow::ensure!(tagged, "no tag");
    common::wait(&mut bed.hp, std::time::Duration::from_millis(600))?;
    shoot(&mut bed.hp, &out, "scene-1-shown-before")?;
    claude.answer(&mut bed.hp, asked)?;
    let asked = claude.send(
        "tools/call",
        json!({ "name": "move_window", "arguments": {
            "window": second, "x": 24, "y": 528, "width": 620, "height": 500,
            "intent": "Range les deux terminaux à gauche, l'un sous l'autre"
        } }),
    )?;
    claude.answer(&mut bed.hp, asked)?;
    let asked = claude.send(
        "tools/call",
        json!({ "name": "set_setting", "arguments": {
            "key": "appearance.theme", "value": "dark", "intent": "Passe le thème en sombre"
        } }),
    )?;
    claude.answer(&mut bed.hp, asked)?;
    // A read agent's refusal, for the journal.
    let mut looker = mcp(&bed.hp, "Looker")?;
    looker.tool(
        &mut bed.hp,
        "launch",
        json!({ "app": "nope", "intent": "Ouvre Firefox" }),
    )?;
    looker.tool(
        &mut bed.hp,
        "switch_desktop",
        json!({ "desktop": 2, "intent": "Va sur le bureau 2" }),
    )?;
    // 2: the toast.
    let toasted = bed.hp.dispatch_until(WAIT, |hp| {
        !hp.drawn_layers(hocus_link::MCP_TOAST_NAMESPACE).is_empty()
    })?;
    anyhow::ensure!(toasted, "no toast");
    common::wait(&mut bed.hp, std::time::Duration::from_millis(600))?;
    shoot(&mut bed.hp, &out, "scene-2-toast")?;
    // 4: the journal, from the toast's Voir.
    let at = control_at(&mut bed, hocus_link::MCP_TOAST_NAMESPACE, "mcp-toast:See")?;
    common::click_on(&mut bed.hp, at, hocus_link::MCP_TOAST_NAMESPACE)?;
    let open = bed
        .hp
        .dispatch_until(WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
    anyhow::ensure!(open, "no journal");
    common::wait(&mut bed.hp, std::time::Duration::from_millis(600))?;
    shoot(&mut bed.hp, &out, "scene-4-journal")?;
    Ok(())
}
