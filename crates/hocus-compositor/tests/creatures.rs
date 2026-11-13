//! The agents as creatures on their windows (#796, docs/design/effects.md),
//! on the held clock and on GLES (Mesa's software device, or the GPU
//! `HOCUS_TEST_GPU` names): an agent's panda sits on its window's top
//! edge, moving, and never over what the window shows; none on a
//! maximised window; none with the switch off, still with reduced motion.
//! The agents are told with `hocus agent`, run beside the test's program
//! (`cargo programs`): its window is the test's own, above it.

use crate::common;

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use common::{KEY_LEFTMETA, KEY_UP, WAIT, open_with};
use hocus_compositor::{
    Config, Headless, HeadlessConfig, OutputSpec,
    model::{Mode, Rect, WindowId, Zone},
};

const STEP: Duration = Duration::from_micros(16_667);
/// Where the agent's window is: room above it for its panda.
const AT: Rect = Rect {
    x: 300,
    y: 300,
    width: 600,
    height: 360,
};

fn creatures(on: bool, reduced_motion: bool) -> Config {
    let mut config = Config::default();
    config.effects.creatures = on;
    config.appearance.reduced_motion = reduced_motion;
    config
}

/// Steps the held clock by `by`, a frame at a time.
fn run(hp: &mut Headless, by: Duration) -> anyhow::Result<()> {
    let mut left = by;
    while left > Duration::ZERO {
        let step = left.min(STEP);
        hp.step(step)?;
        left -= step;
    }
    Ok(())
}

/// `hocus agent NAME STATE` on `hp`'s control socket, from a child of the
/// test's process (whose window is the agent's).
fn hocus(hp: &Headless, args: &[&str]) -> anyhow::Result<()> {
    let socket = hp
        .control_socket()
        .ok_or_else(|| anyhow::anyhow!("no control socket"))?;
    let status = Command::new(common::hocus_program())
        .arg("--socket")
        .arg(socket)
        .args(args)
        .status()?;
    anyhow::ensure!(status.success(), "hocus {args:?}: {status}");
    Ok(())
}

/// A GLES compositor, the test's window at [`AT`], an agent working in
/// it, then `config`.
fn bed(config: Config) -> anyhow::Result<(Headless, WindowId, common::ClientHandle)> {
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1280, 800)],
            ..common::gles()
        },
        creatures(false, false),
    )?;
    if hp.renderer() != "gles" {
        common::missing("GLES on Mesa's software device (libegl-mesa0, libgles2)");
    }
    common::at_rest(&mut hp)?;
    let (_, client) = open_with(
        &mut hp,
        "test.terminal",
        (AT.width as u32, AT.height as u32),
        false,
    )?;
    let id = hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some("test.terminal"))
        .map(|w| w.id)
        .ok_or_else(|| anyhow::anyhow!("no window"))?;
    hp.place_window(id, AT);
    common::at_rest(&mut hp)?;
    hocus(&hp, &["agent", "build", "working"])?;
    // Known once the creatures are on: it sits there.
    hp.reconfigure(creatures(true, false));
    let sits = hp.dispatch_until(WAIT, |hp| hp.creatures_sitting() == [(id, 1)])?;
    anyhow::ensure!(sits, "no creature: {:?}", hp.creatures_sitting());
    hp.hold_clock();
    hp.reconfigure(config);
    run(&mut hp, STEP * 2)?;
    Ok((hp, id, client))
}

/// Over `rect`, the pixels the same in `a` and `b`, of how many.
fn same(a: &(i32, i32, Vec<u8>), b: &(i32, i32, Vec<u8>), rect: Rect) -> (usize, usize) {
    let (mut same, mut all) = (0, 0);
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            let i = ((y * a.0 + x) * 4) as usize;
            all += 1;
            same += usize::from(a.2.get(i..i + 3) == b.2.get(i..i + 3));
        }
    }
    (same, all)
}

#[test]
fn a_panda_sits_on_its_agents_window_and_never_over_what_it_shows() -> anyhow::Result<()> {
    let (mut hp, id, _client) = bed(creatures(false, false))?;
    let before = hp.screenshot(0)?;
    assert!(hp.creatures().is_empty());
    hp.reconfigure(creatures(true, false));
    run(&mut hp, Duration::from_millis(200))?;
    let drawn = hp.creatures();
    assert_eq!(drawn.len(), 1, "{drawn:?}");
    let (window, _, [x, y, w, h]) = drawn[0];
    assert_eq!(window, id);
    // Large, on the top edge's right corner, its feet on the edge: above
    // the window but for the top few pixels.
    assert_eq!(w, 92.0);
    assert_eq!(x + w, f64::from(AT.x + AT.width) - 12.0);
    let bottom = y + h;
    assert!(
        bottom > f64::from(AT.y) && bottom < f64::from(AT.y + 8),
        "its box ends at {bottom}"
    );
    let shot = hp.screenshot(0)?;
    // Drawn above the edge.
    let above = Rect {
        x: x as i32,
        y: y as i32,
        width: w as i32,
        height: AT.y - y as i32,
    };
    let (kept, all) = same(&before, &shot, above);
    assert!(
        all - kept > all / 5,
        "a panda above the edge: {kept} of {all} kept"
    );
    // What the window shows, below its top 8 pixels, as before.
    let inside = Rect {
        x: AT.x,
        y: AT.y + 8,
        width: AT.width,
        height: AT.height - 8,
    };
    let (kept, all) = same(&before, &shot, inside);
    assert_eq!(kept, all, "the window's pixels untouched");
    // It moves: a frame after another.
    let frames = hp.frames_on(0);
    run(&mut hp, Duration::from_millis(300))?;
    assert!(hp.frames_on(0) > frames + 5, "it moves");
    // Its agent gone, it goes.
    hocus(&hp, &["agent", "build", "ended"])?;
    let gone = hp.dispatch_until(WAIT, |hp| hp.creatures_sitting().is_empty())?;
    assert!(gone, "{:?}", hp.creatures_sitting());
    Ok(())
}

#[test]
fn none_on_a_maximized_window() -> anyhow::Result<()> {
    let (mut hp, id, _client) = bed(creatures(true, false))?;
    assert_eq!(hp.creatures().len(), 1);
    common::chord(&mut hp, &[KEY_LEFTMETA, KEY_UP]);
    let maximized = hp.dispatch_until(WAIT, |hp| hp.mode(id) == Mode::Snapped(Zone::Maximized))?;
    anyhow::ensure!(maximized, "{:?}", hp.mode(id));
    run(&mut hp, Duration::from_millis(400))?;
    assert!(hp.creatures_sitting().is_empty());
    assert!(hp.creatures().is_empty(), "{:?}", hp.creatures());
    Ok(())
}

#[test]
fn none_with_the_switch_off_and_still_with_reduced_motion() -> anyhow::Result<()> {
    for (config, case) in [
        (creatures(false, false), "off"),
        (creatures(true, true), "reduced motion"),
    ] {
        let (mut hp, _, _client) = bed(config)?;
        run(&mut hp, Duration::from_millis(100))?;
        let still = case == "reduced motion";
        assert_eq!(hp.creatures().len(), usize::from(still), "{case}");
        let last = hp.frames_on(0);
        run(&mut hp, Duration::from_millis(300))?;
        assert_eq!(hp.frames_on(0), last, "{case}: nothing drawn per frame");
    }
    Ok(())
}

// --- The screenshots ------------------------------------------------------------

/// A screenshot of output 0 saved as `name`.png in `out`.
fn save(hp: &mut Headless, out: &Path, name: &str) -> anyhow::Result<()> {
    let (w, h, px) = hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
        .ok_or_else(|| anyhow::anyhow!("no image"))?;
    let file = out.join(format!("{name}.png"));
    image.save(&file)?;
    eprintln!("{}", file.display());
    Ok(())
}

/// The terminals of the screenshots: foot, each a process of its own
/// (an agent's window is its process's), stopped by their own pids.
struct Terminals {
    children: Vec<std::process::Child>,
    env: Vec<(&'static str, String)>,
    dir: PathBuf,
}

impl Terminals {
    /// A terminal titled `title` showing `text`, at `rect`: its window's
    /// id and its process's.
    fn open(
        &mut self,
        hp: &mut Headless,
        title: &str,
        text: &str,
        rect: Rect,
    ) -> anyhow::Result<(WindowId, u32)> {
        let n = self.children.len();
        let file = self.dir.join(format!("text-{n}"));
        std::fs::write(&file, text)?;
        let app_id = format!("term-{n}");
        let child = Command::new("foot")
            .args(["--app-id", &app_id, "--title", title])
            .args(["-o", "pad=12x10", "-e", "sh", "-c"])
            .arg(format!(
                "printf '\\033]2;{title}\\007'; cat '{}'; sleep 600",
                file.display()
            ))
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (*k, v.as_str())))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let pid = child.id();
        self.children.push(child);
        let shown = hp.dispatch_until(WAIT, |hp| {
            hp.windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some(app_id.as_str()))
        })?;
        anyhow::ensure!(shown, "foot never showed");
        let id = hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(app_id.as_str()))
            .map(|w| w.id)
            .ok_or_else(|| anyhow::anyhow!("no terminal"))?;
        hp.place_window(id, rect);
        Ok((id, pid))
    }
}

impl Drop for Terminals {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A fake herdr on `socket`: three panes side by side in its one tab, an
/// agent in each (`api` working, `site` waiting, `docs` resting).
fn fake_herdr(socket: &Path) -> anyhow::Result<()> {
    use std::io::{BufRead, BufReader, Write};
    std::fs::create_dir_all(socket.parent().unwrap_or(Path::new("/")))?;
    let listener = std::os::unix::net::UnixListener::bind(socket)?;
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            let Ok(request) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            let pane = |i: usize, status: &str, cwd: &str| {
                serde_json::json!({ "pane_id": format!("w1:p{i}"), "workspace_id": "w1",
                    "tab_id": "w1:t1", "focused": i == 1, "agent_status": status,
                    "agent": "claude", "cwd": cwd })
            };
            let rect = |i: u16| {
                serde_json::json!({ "pane_id": format!("w1:p{i}"), "focused": i == 1,
                    "rect": { "x": (i - 1) * 40, "y": 1, "width": 40, "height": 30 } })
            };
            let result = match request["method"].as_str() {
                Some("agent.list") => serde_json::json!({ "type": "agent_list", "agents": [
                    pane(1, "working", "/src/api"), pane(2, "blocked", "/src/site"),
                    pane(3, "idle", "/src/docs")] }),
                Some("workspace.list") => serde_json::json!({ "type": "workspace_list",
                    "workspaces": [{ "workspace_id": "w1", "label": "", "focused": true }] }),
                Some("tab.list") => serde_json::json!({ "type": "tab_list",
                    "tabs": [{ "tab_id": "w1:t1", "workspace_id": "w1", "label": "1",
                        "focused": true }] }),
                Some("pane.layout") => serde_json::json!({ "type": "pane_layout", "layout": {
                    "workspace_id": "w1", "tab_id": "w1:t1", "zoomed": false,
                    "focused_pane_id": "w1:p1", "splits": [],
                    "area": { "x": 0, "y": 1, "width": 120, "height": 30 },
                    "panes": [rect(1), rect(2), rect(3)] } }),
                Some("pane.process_info") => serde_json::json!({ "type": "pane_process_info" }),
                _ => serde_json::json!({ "type": "ok" }),
            };
            let answer = serde_json::json!({ "id": request["id"], "result": result });
            let _ = (&stream).write_all(format!("{answer}\n").as_bytes());
            kept.push(stream);
        }
    });
    Ok(())
}

/// The creatures for the maintainer, written to `$HOCUS_SHOTS` when it
/// names a folder, on GLES, with the shell's bar, in foot terminals (each
/// a process of its own, as an agent's terminal is): the board's three
/// cases (one agent working; herdr's three panes; two tabs, the shown
/// one done, the other resting, tagged), the four states side by side,
/// and a maximised window, its panda in the bar alone. Run by hand
/// (`--run-ignored only`), with foot installed.
#[test]
#[ignore = "screenshots, by hand"]
fn shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let home = tempfile::tempdir()?;
    let at = |sub: &str| home.path().join(sub).display().to_string();
    for sub in [
        "home",
        "config",
        "cache",
        "state/hocuspanda",
        "data",
        "proc",
    ] {
        std::fs::create_dir_all(home.path().join(sub))?;
    }
    // Day.1's first-run card set aside: the agents' group shows.
    std::fs::write(home.path().join("state/hocuspanda/day-sources"), "later\n")?;
    let script = common::shell_script(
        home.path(),
        &common::shell_program(),
        &[
            ("LC_ALL", "fr_FR.UTF-8".to_owned()),
            ("LANG", "fr_FR.UTF-8".to_owned()),
            ("HOME", at("home")),
            ("XDG_CONFIG_HOME", at("config")),
            ("XDG_CACHE_HOME", at("cache")),
            ("XDG_STATE_HOME", at("state")),
            ("XDG_DATA_HOME", at("data")),
        ],
    )?;
    let herdr = home.path().join("home/.config/herdr/herdr.sock");
    let mut config = Config::default();
    config.wallpaper.picture = hocus_link::wallpaper::Picture::of("night").unwrap_or_default();
    config.locale.language = hocus_link::locale::Language::Chosen(hocus_words::Lang::French);
    config.agent_places = Some(hocus_agents::tools::Places {
        home: home.path().join("home"),
        config_home: home.path().join("home/.config"),
        codex_home: home.path().join("home/.codex"),
        path: vec![],
        herdr_socket: herdr.clone(),
        processes: home.path().join("proc"),
        hocus: common::hocus_program(),
        state_dir: Some(home.path().join("state")),
    });
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1440, 900)],
            ..common::gles()
        },
        config.clone(),
        Some(script),
    )?;
    anyhow::ensure!(hp.renderer() == "gles", "no GLES here");
    let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    let socket = hp.socket().to_path_buf();
    let runtime = socket.parent().map(PathBuf::from).unwrap_or_default();
    let terms_env = vec![
        ("PATH", "/usr/bin:/bin".to_owned()),
        ("WAYLAND_DISPLAY", socket.display().to_string()),
        ("XDG_RUNTIME_DIR", runtime.display().to_string()),
        ("HOME", at("home")),
        ("XDG_CONFIG_HOME", at("config")),
        ("LANG", "C.UTF-8".to_owned()),
    ];
    let mut terms = Terminals {
        children: Vec::new(),
        env: terms_env.clone(),
        dir: home.path().to_path_buf(),
    };
    let agent = |hp: &Headless, name: &str, state: &str, pid: u32| {
        hocus(hp, &["agent", name, state, "--pid", &pid.to_string()])
    };
    // The board: one agent working.
    let (_, one) = terms.open(
        &mut hp,
        "kitty — hocus-panda",
        "● Editing tasks/draw.rs\n● Running clippy…\n",
        Rect::new(60, 300, 330, 260),
    )?;
    agent(&hp, "hocus-panda", "working", one)?;
    // herdr's three panes, in a terminal running its client (in the
    // test's own `/proc`: above the kernel's largest pid, none is the
    // machine's).
    let (_, panes) = terms.open(
        &mut hp,
        "herdr · 3 panneaux",
        "api                         site                        docs\n\
         ● cargo test…               ? Overwrite?                > _\n",
        Rect::new(430, 300, 520, 300),
    )?;
    let proc = home.path().join("proc");
    for (pid, name, parent, argv) in [
        (panes, "foot", 1, vec!["foot"]),
        (4_200_005, "herdr", panes, vec!["herdr"]),
    ] {
        let dir = proc.join(pid.to_string());
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("stat"),
            format!("{pid} ({name}) S {parent} {pid} {pid} 0 -1"),
        )?;
        std::fs::write(dir.join("cmdline"), argv.join("\0") + "\0")?;
    }
    fake_herdr(&herdr)?;
    // Two tabs: the shown one done, the other resting.
    let (_, tabs) = terms.open(
        &mut hp,
        "Terminal",
        "✓ 3 brouillons prêts\n",
        Rect::new(990, 360, 300, 220),
    )?;
    hocus(&hp, &["agent", "blog", "idle", "--pid", &tabs.to_string()])?;
    hocus(
        &hp,
        &["agent", "secretaire", "done", "--pid", &tabs.to_string()],
    )?;
    config.effects.creatures = true;
    hp.reconfigure(config.clone());
    let sitting = hp.dispatch_until(WAIT, |hp| {
        hp.creatures_sitting().iter().map(|(_, n)| n).sum::<usize>() == 6
    })?;
    anyhow::ensure!(sitting, "the creatures: {:?}", hp.creatures_sitting());
    common::wait(&mut hp, Duration::from_secs(2))?;
    hp.hold_clock();
    run(&mut hp, Duration::from_millis(500))?;
    save(&mut hp, &out, "creatures-board")?;
    run(&mut hp, Duration::from_millis(330))?;
    save(&mut hp, &out, "creatures-board-later")?;
    // The four states side by side, each its terminal.
    drop(terms);
    let mut terms = Terminals {
        children: Vec::new(),
        env: terms_env.clone(),
        dir: home.path().join("states"),
    };
    std::fs::create_dir_all(&terms.dir)?;
    // Their terminals closed (herdr's too), their creatures go.
    let gone = hp.dispatch_until(WAIT, |hp| hp.creatures_sitting().is_empty())?;
    anyhow::ensure!(gone, "{:?}", hp.creatures_sitting());
    for (i, (name, state, text)) in [
        ("refacto", "working", "● Running clippy…"),
        ("site", "waiting", "? Overwrite index.html?"),
        ("docs", "idle", "> _"),
        ("release", "done", "✓ v2.3 publiée"),
    ]
    .into_iter()
    .enumerate()
    {
        let (_, pid) = terms.open(
            &mut hp,
            name,
            &format!("{text}\n"),
            Rect::new(40 + i as i32 * 290, 380, 260, 220),
        )?;
        hocus(&hp, &["agent", name, state, "--pid", &pid.to_string()])?;
    }
    let sitting = hp.dispatch_until(WAIT, |hp| {
        hp.creatures_sitting().iter().map(|(_, n)| n).sum::<usize>() == 4
    })?;
    anyhow::ensure!(sitting, "the creatures: {:?}", hp.creatures_sitting());
    run(&mut hp, Duration::from_millis(400))?;
    save(&mut hp, &out, "creatures-states")?;
    // A maximised window: no free edge, its panda in the bar alone.
    common::click(&mut hp, (150.0, 500.0))?;
    common::chord(&mut hp, &[KEY_LEFTMETA, KEY_UP]);
    run(&mut hp, Duration::from_millis(800))?;
    save(&mut hp, &out, "creatures-maximized")?;
    drop(terms);
    Ok(())
}
