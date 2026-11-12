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
    bed_on(shell, text, None)
}

/// The same, applications' trees read on the accessibility bus `a11y`.
fn bed_on(shell: bool, text: &str, a11y: Option<String>) -> anyhow::Result<Bed> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    for sub in ["home", "config", "state/hocuspanda", "cache"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    std::fs::write(root.join("state/hocuspanda/day-sources"), "later\n")?;
    let file = root.join("config/config.toml");
    std::fs::write(&file, text)?;
    let log = root.join("shell.log");
    let read = toml::from_str::<Config>(text)?;
    let config = Config {
        file: Some(file.clone()),
        agents: read.agents,
        keyboard: read.keyboard,
        a11y_bus: a11y,
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
    mcp_with(hp, agent, &[])
}

/// The same, with these variables (the desktop entries it finds).
fn mcp_with(hp: &Headless, agent: &str, env: &[(&str, &Path)]) -> anyhow::Result<Mcp> {
    let socket = hp
        .control_socket()
        .ok_or_else(|| anyhow::anyhow!("no control socket"))?;
    let mut child = Command::new(common::hocus_program())
        .envs(env.iter().copied())
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
        "click",
        "scroll",
        "type",
        "press_keys",
        "close_window",
        "wait_for",
        "capture",
        "accessibility_tree",
    ] {
        assert!(names.contains(&name), "{name} not in {names:?}");
    }
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
    // The agent's pointer on the window's top, its middle, the tag
    // beside it.
    assert!(
        matches!(show.at, hocus_link::McpAt::Pointer { y, .. } if y == rect.y + 12),
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

// --- In applications (lot B) ------------------------------------------------

/// Claude acts in the windows of `acted` and `mail`, Typist on a US
/// keyboard, so that the keys' codes are known.
const IN_APPS: &str = "[keyboard]\nlayouts = [\"us\"]\n\n[agents]\nshow-before-ms = 0\n\n\
                       [[agents.mcp]]\nname = \"Claude\"\naccess = \"act\"\n\
                       apps = [\"acted*\", \"mail\"]\n";

const AGENT_SEAT: &str = "agent-Claude";

/// An acted window (`app_id`, 400 × 300), taking every seat.
fn acted(
    hp: &mut Headless,
    app_id: &'static str,
    secret: bool,
) -> anyhow::Result<(u64, common::ClientHandle)> {
    let (_, handle) = common::open_showing(
        hp,
        app_id,
        common::Shows::Acted {
            app_id,
            size: (400, 300),
            secret,
        },
    )?;
    let id = hp
        .windows()
        .iter()
        .find(|w| w.app_id.as_deref() == Some(app_id))
        .map(|w| w.id.0)
        .ok_or_else(|| anyhow::anyhow!("no {app_id}"))?;
    Ok((id, handle))
}

/// What seat `seat` gave the client so far.
fn seated(handle: &common::ClientHandle, seat: &str, got: &mut Vec<common::Seated>) {
    while let Ok(event) = handle.events.try_recv() {
        if let common::Event::OnSeat(name, what) = event
            && name == seat
        {
            got.push(what);
        }
    }
}

/// The focused window's app id.
fn focused(hp: &Headless) -> Option<String> {
    hp.windows()
        .into_iter()
        .find(|w| w.focused)
        .and_then(|w| w.app_id)
}

#[test]
fn a_click_lands_through_the_agents_seat_and_the_users_focus_stays() -> anyhow::Result<()> {
    let mut bed = bed_with(false, IN_APPS)?;
    let (id, handle) = acted(&mut bed.hp, "acted", false)?;
    // The user's window over it, with the keyboard.
    common::open(&mut bed.hp, "foot", (640, 400))?;
    assert_eq!(focused(&bed.hp).as_deref(), Some("foot"));
    let mut agent = mcp(&bed.hp, "Claude")?;
    let result = agent.tool(
        &mut bed.hp,
        "click",
        json!({ "window": id, "x": 40, "y": 30, "intent": "Clique dans la fenêtre" }),
    )?;
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(result["structuredContent"]["x"], 40.0, "{result}");
    let mut got = Vec::new();
    let pressed = bed.hp.dispatch_until(WAIT, |_| {
        seated(&handle, AGENT_SEAT, &mut got);
        got.contains(&common::Seated::Press(0x110))
    })?;
    anyhow::ensure!(pressed, "no press on the agent's seat: {got:?}");
    assert!(got.contains(&common::Seated::Enter), "{got:?}");
    // Its keyboard followed its click; the user's stayed.
    assert!(got.contains(&common::Seated::Focus(true)), "{got:?}");
    assert_eq!(focused(&bed.hp).as_deref(), Some("foot"));
    // The pointer shows, its name beside it.
    let pointers = bed.hp.mcp_shown().pointers;
    assert!(pointers.iter().any(|p| p.agent == "Claude"), "{pointers:?}");
    // Not an application of its: refused.
    let rect = common::rect_of(&bed.hp, "foot").ok_or_else(|| anyhow::anyhow!("no foot"))?;
    let other = bed
        .hp
        .windows()
        .iter()
        .find(|w| w.app_id.as_deref() == Some("foot"))
        .map(|w| w.id.0);
    let result = agent.tool(
        &mut bed.hp,
        "click",
        json!({ "window": other, "intent": "Clique" }),
    )?;
    assert_eq!(result["isError"], true, "{result}");
    assert!(text(&result).contains("apps"), "{result}");
    assert_eq!(common::rect_of(&bed.hp, "foot"), Some(rect));
    Ok(())
}

/// The desktops (from 1) of the `acted` windows, as list_windows says.
fn acted_desktops(agent: &mut Mcp, hp: &mut Headless) -> anyhow::Result<Vec<u64>> {
    let result = agent.tool(hp, "list_windows", json!({}))?;
    Ok(result["structuredContent"]["windows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|w| w["app_id"] == "acted")
        .filter_map(|w| w["desktop"].as_u64())
        .collect())
}

/// #790: an application an agent launches onto another desktop opens
/// there, and the desktop the user sees stays.
#[test]
fn a_launch_onto_another_desktop_opens_there_out_of_sight() -> anyhow::Result<()> {
    let mut bed = bed_with(false, IN_APPS)?;
    let data = bed._dir.path().join("data");
    std::fs::create_dir_all(data.join("applications"))?;
    std::fs::write(
        data.join("applications/acted.desktop"),
        "[Desktop Entry]\nType=Application\nName=Acted\nExec=true\n",
    )?;
    let env = [
        ("XDG_DATA_HOME", data.as_path()),
        ("XDG_DATA_DIRS", data.as_path()),
    ];
    let mut agent = mcp_with(&bed.hp, "Claude", &env)?;
    let result = agent.tool(
        &mut bed.hp,
        "launch",
        json!({ "app": "Acted", "desktop": 2, "intent": "Ouvre Acted sur le bureau 2" }),
    )?;
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(result["structuredContent"]["desktop"], 2, "{result}");
    // Out of sight, the harness lists it not: as the agent sees it.
    let shows = common::Shows::Window {
        app_id: "acted",
        size: (400, 300),
        popup: false,
    };
    let _client = common::client(bed.hp.socket(), Some(shows))?;
    let mut desktops = Vec::new();
    for _ in 0..200 {
        bed.hp.dispatch(std::time::Duration::from_millis(10))?;
        desktops = acted_desktops(&mut agent, &mut bed.hp)?;
        if !desktops.is_empty() {
            break;
        }
    }
    assert_eq!(desktops, [2]);
    assert_eq!(bed.hp.desktop().0, 0, "the user's desktop changed");
    // Its next window opens where the user is, as any.
    common::open(&mut bed.hp, "acted", (400, 300))?;
    let mut desktops = acted_desktops(&mut agent, &mut bed.hp)?;
    desktops.sort_unstable();
    assert_eq!(desktops, [1, 2]);
    // A desktop the screen has not: refused, nothing started.
    let result = agent.tool(
        &mut bed.hp,
        "launch",
        json!({ "app": "Acted", "desktop": 9, "intent": "Ouvre Acted" }),
    )?;
    assert_eq!(result["isError"], true, "{result}");
    Ok(())
}

/// The session ended each time an agent went: an application asking its
/// seat's pointer's cursor shape after that panicked the compositor. The
/// seat stays; the agent's next connection takes it again.
#[test]
fn an_agent_going_leaves_its_seat_for_the_next_time() -> anyhow::Result<()> {
    let mut bed = bed_with(false, IN_APPS)?;
    let (id, handle) = acted(&mut bed.hp, "acted", false)?;
    let click = json!({ "window": id, "x": 40, "y": 30, "intent": "Clique dans la fenêtre" });
    let mut agent = mcp(&bed.hp, "Claude")?;
    let result = agent.tool(&mut bed.hp, "click", click.clone())?;
    assert_eq!(result["isError"], false, "{result}");
    let mut got = Vec::new();
    let pressed = bed.hp.dispatch_until(WAIT, |_| {
        seated(&handle, AGENT_SEAT, &mut got);
        got.contains(&common::Seated::Press(0x110))
    })?;
    anyhow::ensure!(pressed, "no press on the agent's seat: {got:?}");
    drop(agent);
    let went = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.mcp_shown().pointers.is_empty())?;
    anyhow::ensure!(went, "the agent's pointer still shows");
    handle.commands.send(common::Command::CursorShapes)?;
    bed.hp
        .dispatch_until(std::time::Duration::from_millis(300), |_| false)?;
    // Again, on the same seat.
    let mut agent = mcp(&bed.hp, "Claude")?;
    let result = agent.tool(&mut bed.hp, "click", click)?;
    assert_eq!(result["isError"], false, "{result}");
    got.clear();
    let pressed = bed.hp.dispatch_until(WAIT, |_| {
        seated(&handle, AGENT_SEAT, &mut got);
        got.contains(&common::Seated::Press(0x110))
    })?;
    anyhow::ensure!(pressed, "no press on the seat taken again: {got:?}");
    Ok(())
}

#[test]
fn typing_reaches_the_window_alone_and_never_the_shortcuts() -> anyhow::Result<()> {
    let mut bed = bed_with(false, IN_APPS)?;
    let (id, handle) = acted(&mut bed.hp, "acted", false)?;
    common::open(&mut bed.hp, "foot", (640, 400))?;
    let mut agent = mcp(&bed.hp, "Claude")?;
    let result = agent.tool(
        &mut bed.hp,
        "type",
        json!({ "window": id, "text": "aB", "intent": "Tape un mot" }),
    )?;
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(result["structuredContent"]["typed"], 2);
    // The stop key, typed by the agent: the window's, never the desktop's.
    let result = agent.tool(
        &mut bed.hp,
        "press_keys",
        json!({ "window": id, "keys": "Super+Shift+Escape", "intent": "Essaie" }),
    )?;
    assert_eq!(result["isError"], false, "{result}");
    let mut got = Vec::new();
    let typed = bed.hp.dispatch_until(WAIT, |_| {
        seated(&handle, AGENT_SEAT, &mut got);
        got.contains(&common::Seated::Key(KEY_ESC, true))
    })?;
    anyhow::ensure!(typed, "the keys did not come: {got:?}");
    let pressed: Vec<u32> = got
        .iter()
        .filter_map(|g| match g {
            common::Seated::Key(code, true) => Some(*code),
            _ => None,
        })
        .collect();
    // a, then Shift and b, then Super, Shift, Escape.
    assert_eq!(
        pressed,
        [30, KEY_LEFTSHIFT, 48, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_ESC]
    );
    assert!(!bed.hp.mcp_shown().stopped, "the agent stopped itself");
    assert_eq!(focused(&bed.hp).as_deref(), Some("foot"));
    // Its journal counts what it typed, never keeps it.
    let journal = agent.tool(&mut bed.hp, "journal", json!({}))?;
    let said = journal.to_string();
    assert!(said.contains("type 2 characters"), "{said}");
    assert!(!said.contains("aB"), "{said}");
    Ok(())
}

#[test]
fn a_click_by_label_through_the_applications_tree() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let (mut bus, address) = common::private_bus(dir.path())?;
    let tree = common::atspi::FakeTree::start(
        &address,
        "mail",
        vec![common::atspi::Element::compose("Nouveau message")],
    )?;
    let run = || -> anyhow::Result<()> {
        let mut bed = bed_on(false, IN_APPS, Some(address.clone()))?;
        let (id, handle) = acted(&mut bed.hp, "mail", false)?;
        let mut agent = mcp(&bed.hp, "Claude")?;
        let read = agent.tool(&mut bed.hp, "accessibility_tree", json!({ "window": id }))?;
        assert_eq!(read["isError"], false, "{read}");
        let elements = read["structuredContent"]["elements"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let labelled = |label: &str| {
            elements
                .iter()
                .find(|e| e["label"] == label)
                .cloned()
                .unwrap_or_default()
        };
        assert_eq!(labelled("Envoyer")["role"], "push button", "{read}");
        assert_eq!(labelled("Sujet")["value"], "Déjeuner vendredi", "{read}");
        // A password's content is never read.
        assert!(labelled("Mot de passe").get("value").is_none(), "{read}");
        assert!(!tree.read().iter().any(|l| l == "Mot de passe"));
        let result = agent.tool(
            &mut bed.hp,
            "click",
            json!({ "window": id, "label": "envoyer", "intent": "Envoie le message" }),
        )?;
        assert_eq!(result["isError"], false, "{result}");
        assert_eq!(result["structuredContent"]["x"], 334.0, "{result}");
        assert_eq!(result["structuredContent"]["y"], 264.0, "{result}");
        let mut got = Vec::new();
        let pressed = bed.hp.dispatch_until(WAIT, |_| {
            seated(&handle, AGENT_SEAT, &mut got);
            got.contains(&common::Seated::Press(0x110))
        })?;
        anyhow::ensure!(pressed, "no press: {got:?}");
        // Several labels hold it: refused, listed.
        let result = agent.tool(
            &mut bed.hp,
            "click",
            json!({ "window": id, "label": "r", "role": "push button", "intent": "x" }),
        )?;
        assert_eq!(result["isError"], true, "{result}");
        assert!(text(&result).contains("2 elements"), "{result}");
        Ok(())
    };
    let ran = run();
    drop(tree);
    // Ours, by its own handle.
    let _ = bus.kill();
    let _ = bus.wait();
    ran
}

/// The card of the action shown now, once it shows.
fn card(bed: &mut Bed) -> anyhow::Result<hocus_link::McpCard> {
    let shown = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.mcp_shown().card.is_some())?;
    anyhow::ensure!(shown, "no card");
    bed.hp
        .mcp_shown()
        .card
        .ok_or_else(|| anyhow::anyhow!("gone"))
}

#[test]
fn a_password_field_asks_the_user_first_and_refuse_refuses() -> anyhow::Result<()> {
    let mut bed = bed_with(false, IN_APPS)?;
    let (id, handle) = acted(&mut bed.hp, "acted-bank", true)?;
    let mut agent = mcp(&bed.hp, "Claude")?;
    let asked = agent.send(
        "tools/call",
        json!({ "name": "type", "arguments": {
            "window": id, "text": "1234", "intent": "Tape le code"
        } }),
    )?;
    let card = card(&mut bed)?;
    assert_eq!(card.why, hocus_link::McpWhy::Password, "{card:?}");
    assert_eq!(card.intent, "Tape le code");
    assert!(
        matches!(card.ask, hocus_link::McpAsk::Type { .. }),
        "{card:?}"
    );
    let row = bed.hp.mcp_shown().agents;
    assert!(
        row.iter().any(|r| r.doing == hocus_link::McpDoing::Asking),
        "{row:?}"
    );
    bed.hp.mcp_reply(card.id, hocus_link::McpReply::Refuse);
    let answer = agent.answer(&mut bed.hp, asked)?;
    assert_eq!(answer["result"]["isError"], true, "{answer}");
    assert!(text(&answer["result"]).contains("refused it"), "{answer}");
    assert!(bed.hp.mcp_shown().card.is_none());
    let mut got = Vec::new();
    common::wait(&mut bed.hp, std::time::Duration::from_millis(200))?;
    seated(&handle, AGENT_SEAT, &mut got);
    assert!(
        !got.iter().any(|g| matches!(g, common::Seated::Key(..))),
        "typed anyway: {got:?}"
    );
    Ok(())
}

#[test]
fn taking_over_refuses_the_agent_until_handed_back() -> anyhow::Result<()> {
    let config = IN_APPS.replace(
        "show-before-ms = 0\n",
        "show-before-ms = 0\nconfirm-apps = [\"acted\"]\n",
    );
    let mut bed = bed_with(false, &config)?;
    let (id, _handle) = acted(&mut bed.hp, "acted", false)?;
    common::open(&mut bed.hp, "foot", (640, 400))?;
    let mut agent = mcp(&bed.hp, "Claude")?;
    let asked = agent.send(
        "tools/call",
        json!({ "name": "click", "arguments": { "window": id, "intent": "Valide" } }),
    )?;
    let card = card(&mut bed)?;
    assert!(
        matches!(&card.why, hocus_link::McpWhy::App { name } if name == "acted"),
        "{card:?}"
    );
    bed.hp.mcp_reply(card.id, hocus_link::McpReply::TakeOver);
    let answer = agent.answer(&mut bed.hp, asked)?;
    assert_eq!(answer["result"]["isError"], true, "{answer}");
    // The window comes forward with the user's keyboard.
    let forward = bed
        .hp
        .dispatch_until(WAIT, |hp| focused(hp).as_deref() == Some("acted"))?;
    anyhow::ensure!(forward, "the window did not come forward");
    assert_eq!(bed.hp.mcp_shown().taken, ["Claude"]);
    let result = agent.tool(
        &mut bed.hp,
        "move_window",
        json!({ "window": id, "x": 0, "intent": "À gauche" }),
    )?;
    assert_eq!(result["isError"], true, "{result}");
    assert!(text(&result).contains("took over"), "{result}");
    bed.hp.mcp_hand_back("Claude");
    let result = agent.tool(
        &mut bed.hp,
        "move_window",
        json!({ "window": id, "x": 0, "intent": "À gauche" }),
    )?;
    assert_eq!(result["isError"], false, "{result}");
    Ok(())
}

#[test]
fn a_window_on_a_desktop_no_screen_shows_is_captured() -> anyhow::Result<()> {
    let mut bed = bed_with(false, IN_APPS)?;
    let (id, _handle) = acted(&mut bed.hp, "acted", false)?;
    let mut agent = mcp(&bed.hp, "Claude")?;
    let sent = agent.tool(
        &mut bed.hp,
        "move_window",
        json!({ "window": id, "desktop": 2, "intent": "Sur le bureau 2" }),
    )?;
    assert_eq!(sent["isError"], false, "{sent}");
    let window = hocus_compositor::model::WindowId(id);
    let unseen = bed
        .hp
        .dispatch_until(WAIT, |hp| !hp.state().model.is_shown(window))?;
    anyhow::ensure!(unseen, "still shown");
    let shot = agent.tool(&mut bed.hp, "capture", json!({ "window": id }))?;
    assert_eq!(shot["isError"], false, "{shot}");
    assert_eq!(shot["content"][0]["type"], "image", "{shot}");
    assert_eq!(shot["structuredContent"]["width"], 400, "{shot}");
    use base64::Engine;
    let png = base64::engine::general_purpose::STANDARD
        .decode(shot["content"][0]["data"].as_str().unwrap_or_default())?;
    let image = image::load_from_memory(&png)?.to_rgba8();
    assert_eq!((image.width(), image.height()), (400, 300));
    // Its application's drawing, not nothing.
    assert!(image.pixels().any(|p| p.0[3] > 0), "an empty capture");
    Ok(())
}

/// The mockup's scenes 1 (the agent's pointer, its ring and its intent on
/// what it will touch) and 3 (the card), dark, ×1, for the maintainer's
/// eyes: `HOCUS_SHOTS=DIR cargo nextest run -p hocus-compositor
/// mcp::shots_in_apps --run-ignored only`.
#[test]
#[ignore = "screenshots, on demand"]
fn shots_in_apps() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        eprintln!("HOCUS_SHOTS is not set: no shots");
        return Ok(());
    };
    let config = "[keyboard]\nlayouts = [\"us\"]\n\n[agents]\nshow-before-ms = 4000\n\
                  confirm-apps = [\"notes\"]\n\n[[agents.mcp]]\nname = \"Claude\"\n\
                  access = \"act\"\napps = [\"kitty*\", \"notes\"]\n";
    let mut bed = bed_with(true, config)?;
    let (first, kitty) = acted(&mut bed.hp, "kitty", false)?;
    let (second, kitty_2) = acted(&mut bed.hp, "kitty-2", false)?;
    let _ = kitty
        .commands
        .send(common::Command::Title("kitty — ~/github/hocus-panda"));
    let _ = kitty_2.commands.send(common::Command::Title("kitty"));
    let browser = common::open(&mut bed.hp, "firefox", (700, 560)).map(|_| {
        bed.hp
            .windows()
            .iter()
            .find(|w| w.app_id.as_deref() == Some("firefox"))
            .map(|w| w.id.0)
            .unwrap_or_default()
    })?;
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
    // 1: its pointer, its ring and its intent, before it moves the window.
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
    common::wait(&mut bed.hp, std::time::Duration::from_millis(800))?;
    shoot(&mut bed.hp, &out, "scene-1-pointer")?;
    claude.answer(&mut bed.hp, asked)?;
    // 1 again: a click in the browser's window, its point ringed.
    let asked = claude.send(
        "tools/call",
        json!({ "name": "click", "arguments": {
            "window": second, "x": 300, "y": 200, "intent": "Clique dans le terminal du bas"
        } }),
    )?;
    common::wait(&mut bed.hp, std::time::Duration::from_millis(800))?;
    shoot(&mut bed.hp, &out, "scene-1-click")?;
    claude.answer(&mut bed.hp, asked)?;
    // 3: closing a window of an application to confirm.
    let (notes, notes_client) = acted(&mut bed.hp, "notes", false)?;
    let _ = notes_client
        .commands
        .send(common::Command::Title("Notes — Courses (modifié)"));
    bed.hp
        .place_window(WindowId(notes), Rect::new(120, 160, 540, 400));
    common::at_rest(&mut bed.hp)?;
    let asked = claude.send(
        "tools/call",
        json!({ "name": "close_window", "arguments": {
            "window": notes, "intent": "Ferme les fenêtres dont je n'ai plus besoin"
        } }),
    )?;
    let carded = bed
        .hp
        .dispatch_until(std::time::Duration::from_secs(15), |hp| {
            !hp.drawn_layers(hocus_link::MCP_CARD_NAMESPACE).is_empty()
        })?;
    anyhow::ensure!(carded, "no card");
    common::wait(&mut bed.hp, std::time::Duration::from_millis(800))?;
    shoot(&mut bed.hp, &out, "scene-3-card")?;
    // Its Refuser, clicked.
    let at = control_at(&mut bed, hocus_link::MCP_CARD_NAMESPACE, "mcp-card:Refuse")?;
    common::click_on(&mut bed.hp, at, hocus_link::MCP_CARD_NAMESPACE)?;
    let answer = claude.answer(&mut bed.hp, asked)?;
    anyhow::ensure!(answer["result"]["isError"] == true, "{answer}");
    Ok(())
}
