//! The screenshots of project desktops, lot A, for the maintainer
//! (docs/design/projects.md, boards 1, 3, 3b and 5's open card; #789):
//! the real shell on the CPU, away from the machine's services, on a
//! 1440 × 900 screen as the canvas's. The overview with a project and
//! "En faire un projet" under the thumbnail the pointer rests on; its form;
//! the project made; a project's card; the mini bar's dots as desktops
//! switch, the project's name under the dots; a project's list; the full
//! bar's line over its tiles. Written to `$HOCUS_SHOTS` when it names a
//! folder; run by hand (`--run-ignored only`).

use crate::common;

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use anyhow::Context;
use common::{
    Command, KEY_1, KEY_2, KEY_3, KEY_DOWN, KEY_ESC, KEY_LEFTMETA, KEY_RIGHT, WAIT, chord, wait,
};
use hocus_compositor::{Config, FormChoice, Headless, HeadlessConfig, RendererChoice};

const SCREEN: &str = "HEADLESS-1";
const KEY_W: u32 = 17;
const KEY_ENTER: u32 = 28;
const KEY_SPACE: u32 = 57;

/// The screen as `name.png`, and its right part ×2 as `name-zoom.png`
/// when `zoom` names a rectangle to look at.
fn shoot(
    hp: &mut Headless,
    out: &Path,
    name: &str,
    zoom: Option<(u32, u32, u32, u32)>,
) -> anyhow::Result<()> {
    let (w, h, px) = hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
    let file = out.join(format!("{name}.png"));
    image.save(&file)?;
    eprintln!("{}", file.display());
    if let Some((x, y, cw, ch)) = zoom {
        let cw = cw.min(w as u32 - x);
        let ch = ch.min(h as u32 - y);
        let crop = image::imageops::crop_imm(&image, x, y, cw, ch).to_image();
        let zoomed = image::imageops::resize(
            &crop,
            cw * 2,
            ch * 2,
            image::imageops::FilterType::CatmullRom,
        );
        let file = out.join(format!("{name}-zoom.png"));
        zoomed.save(&file)?;
        eprintln!("{}", file.display());
    }
    Ok(())
}

/// A git repository at `home/path`, last changed `ago` ago.
fn repo(home: &Path, path: &str, ago: Duration) -> anyhow::Result<PathBuf> {
    let top = home.join(path);
    std::fs::create_dir_all(top.join(".git/logs"))?;
    let head = top.join(".git/logs/HEAD");
    std::fs::write(&head, "x")?;
    std::fs::File::options()
        .write(true)
        .open(&head)?
        .set_modified(SystemTime::now() - ago)?;
    Ok(top)
}

/// A window titled `title`, its client kept.
fn window(
    hp: &mut Headless,
    app: &'static str,
    title: &'static str,
    size: (u32, u32),
) -> anyhow::Result<common::ClientHandle> {
    let (_, client) = common::open_with(hp, app, size, false)?;
    client.commands.send(Command::Title(title))?;
    Ok(client)
}

fn press(hp: &mut Headless, key: u32) {
    hp.key(key, true);
    hp.key(key, false);
}

/// Runs until a surface of `namespace` is drawn; where it is.
fn drawn(hp: &mut Headless, namespace: &str) -> anyhow::Result<hocus_compositor::model::Rect> {
    let up = hp.dispatch_until(WAIT, |hp| !hp.drawn_layers(namespace).is_empty())?;
    anyhow::ensure!(up, "no {namespace}");
    wait(hp, Duration::from_millis(400))?;
    Ok(hp.drawn_layers(namespace)[0].1)
}

#[test]
#[ignore = "screenshots for the maintainer, by hand"]
fn the_overview_the_bar_and_the_card_of_project_desktops() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS") else {
        eprintln!("HOCUS_SHOTS names no folder");
        return Ok(());
    };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out)?;
    let shell = common::shell_program();
    let dir = tempfile::tempdir()?;
    let at = |sub: &str| -> anyhow::Result<PathBuf> {
        let path = dir.path().join(sub);
        std::fs::create_dir_all(&path)?;
        Ok(path)
    };
    let home = at("home")?;
    let hour = Duration::from_secs(3600);
    let panda = repo(&home, "github/hocus-panda", hour / 4)?;
    repo(&home, "github/hocus-site", hour / 2)?;
    repo(&home, "github/hocus-panda-tools", 2 * hour)?;
    repo(&home, "code/blog", 26 * hour)?;
    repo(&home, "code/api-meteo", 96 * hour)?;
    let env = [
        ("HOME", home.display().to_string()),
        ("XDG_CONFIG_HOME", at("config")?.display().to_string()),
        ("XDG_CACHE_HOME", at("cache")?.display().to_string()),
        ("XDG_STATE_HOME", at("state")?.display().to_string()),
        ("XDG_DATA_HOME", at("data")?.display().to_string()),
        ("LC_ALL", "fr_FR.UTF-8".to_owned()),
        ("LANG", "fr_FR.UTF-8".to_owned()),
        ("HOCUS_USER_NAME", "Alex".to_owned()),
    ];
    let script = common::shell_script(dir.path(), &shell, &env)?;
    let mut config = Config::default();
    config.panel.form = FormChoice::Mini;
    config.locale.language = hocus_link::locale::Language::Chosen(hocus_words::Lang::French);
    config.home = Some(home.clone());
    config.state_dir = Some(at("state/hocuspanda")?);
    config.desktops.names = vec![String::new(), String::new(), "Discussions".into()];
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1440x900".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config.clone(),
        Some(script),
    )?;
    let up = hp.dispatch_until(WAIT, |hp| {
        hp.drawn_layers(hocus_link::DESKTOPS_NAMESPACE).len() == 1
    })?;
    anyhow::ensure!(up, "no desktops' tiles");

    // Desktop 1, the project hocus-panda: a browser, Claude Code, a
    // terminal. Desktop 2, shown: a browser and a terminal. Desktop 3:
    // a chat. Desktop 4: empty.
    hp.pointer_move(700.0, 450.0);
    let mut clients = vec![
        window(
            &mut hp,
            "firefox",
            "#789 Project desktops — Mozilla Firefox",
            (690, 470),
        )?,
        window(&mut hp, "kitty", "Claude Code — hocus-panda", (620, 460))?,
        window(&mut hp, "foot", "~/github/hocus-panda/crates", (440, 300))?,
    ];
    chord(&mut hp, &[KEY_LEFTMETA, KEY_3]);
    clients.push(window(&mut hp, "slack", "Slack — L’équipe", (820, 600))?);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    clients.push(window(
        &mut hp,
        "firefox",
        "hocus-site · Accueil — Mozilla Firefox",
        (760, 520),
    )?);
    clients.push(window(
        &mut hp,
        "foot",
        "npm run dev — ~/github/hocus-site",
        (550, 340),
    )?);
    wait(&mut hp, Duration::from_millis(400))?;
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 1, "hocus-panda", &panda)?;

    // The overview, the pointer on desktop 2's thumbnail: its button.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_W]);
    wait(&mut hp, Duration::from_millis(400))?;
    let thumb = hp
        .overview_thumbs()
        .into_iter()
        .find(|(o, d, _)| o == SCREEN && *d == 1)
        .map(|(.., m)| m)
        .context("no thumbnail")?;
    hp.pointer_move(thumb.0, thumb.1);
    wait(&mut hp, Duration::from_millis(300))?;
    shoot(&mut hp, &out, "1-overview-make", Some((200, 0, 700, 260)))?;
    // Its form.
    let button = hp.overview_make(SCREEN, 1).context("no button")?;
    hp.pointer_move(button.0, button.1);
    wait(&mut hp, Duration::from_millis(100))?;
    hp.button(common::BTN_LEFT, true);
    hp.button(common::BTN_LEFT, false);
    let form = drawn(&mut hp, hocus_link::PROJECT_FORM_NAMESPACE)?;
    hp.pointer_move(1100.0, 800.0);
    wait(&mut hp, Duration::from_millis(300))?;
    shoot(
        &mut hp,
        &out,
        "2-overview-form",
        Some((
            form.x as u32,
            (form.y - 160).max(0) as u32,
            420,
            form.height as u32 + 170,
        )),
    )?;
    // Made: the label, the toast.
    press(&mut hp, KEY_ENTER);
    let made = hp.dispatch_until(WAIT, |hp| hp.projects().len() == 2)?;
    anyhow::ensure!(made, "no second project: {:?}", hp.projects());
    wait(&mut hp, Duration::from_millis(500))?;
    shoot(&mut hp, &out, "3-overview-made", Some((200, 0, 700, 260)))?;
    // Project 1's card.
    let name = hp.overview_project_name(SCREEN, 0).context("no name")?;
    hp.pointer_move(name.0, name.1);
    wait(&mut hp, Duration::from_millis(100))?;
    hp.button(common::BTN_LEFT, true);
    hp.button(common::BTN_LEFT, false);
    let card = drawn(&mut hp, hocus_link::PROJECT_CARD_NAMESPACE)?;
    hp.pointer_move(1100.0, 850.0);
    wait(&mut hp, Duration::from_millis(300))?;
    shoot(
        &mut hp,
        &out,
        "4-overview-card",
        Some((card.x as u32, card.y as u32, 620, card.height as u32 + 10)),
    )?;
    press(&mut hp, KEY_ESC);
    wait(&mut hp, Duration::from_millis(300))?;
    press(&mut hp, KEY_ESC);
    let gone = hp.dispatch_until(WAIT, |hp| hp.overview().is_none())?;
    anyhow::ensure!(gone, "the overview stays");
    wait(&mut hp, Duration::from_millis(400))?;

    // The mini bar: desktop 1 from desktop 2, the name under the dots.
    hp.pointer_move(700.0, 450.0);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
    wait(&mut hp, Duration::from_millis(160))?;
    shoot(&mut hp, &out, "5-switch-name", Some((470, 0, 500, 90)))?;
    wait(&mut hp, Duration::from_millis(1200))?;
    shoot(
        &mut hp,
        &out,
        "6-mini-bar",
        Some((1440 - 200, 900 - 420, 200, 420)),
    )?;
    // A project's list, resting on its dot.
    let dot = hp
        .desk_tile_middles()
        .into_iter()
        .find(|(_, _, d, _)| *d == 1)
        .map(|(.., m)| m)
        .context("no dot")?;
    hp.pointer_move(dot.0 - 1.0, dot.1);
    hp.pointer_move(dot.0, dot.1);
    let list = drawn(&mut hp, hocus_link::POPOVER_NAMESPACE)?;
    shoot(
        &mut hp,
        &out,
        "7-mini-list",
        Some((
            list.x as u32 - 10,
            list.y as u32 - 10,
            list.width as u32 + 90,
            list.height as u32 + 20,
        )),
    )?;
    hp.pointer_move(700.0, 450.0);
    wait(&mut hp, Duration::from_millis(600))?;

    // The full bar: the project's line over the tiles, their monograms.
    config.panel.form = FormChoice::Full;
    hp.reconfigure(config);
    wait(&mut hp, Duration::from_millis(1500))?;
    let (_, tiles) = hp
        .desktops_surfaces()
        .into_iter()
        .next()
        .context("no desktops' surface")?;
    shoot(
        &mut hp,
        &out,
        "8-full-bar",
        Some((
            (tiles.x - 20).max(0) as u32,
            (tiles.y - 60).max(0) as u32,
            240,
            tiles.height as u32 + 120,
        )),
    )?;
    // And the line on desktop 2, which is a project too now.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    wait(&mut hp, Duration::from_millis(800))?;
    shoot(
        &mut hp,
        &out,
        "9-full-bar-other",
        Some((
            (tiles.x - 20).max(0) as u32,
            (tiles.y - 60).max(0) as u32,
            240,
            tiles.height as u32 + 120,
        )),
    )?;
    drop(clients);
    Ok(())
}

/// The Settings' line, on which the first project is made.
struct Line {
    stream: std::os::unix::net::UnixStream,
    heard: std::sync::mpsc::Receiver<serde_json::Value>,
}

impl Line {
    fn open(hp: &mut Headless) -> anyhow::Result<Self> {
        use std::io::{BufRead, Write};
        let path = hp.control_socket().context("no control socket")?;
        let mut stream = std::os::unix::net::UnixStream::connect(path)?;
        stream.write_all(b"msg screens\n")?;
        let reader = stream.try_clone()?;
        let (said, heard) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(reader).lines() {
                let Ok(line) = line else {
                    return;
                };
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line)
                    && said.send(value).is_err()
                {
                    return;
                }
            }
        });
        let opened = hp.dispatch_until(WAIT, |_| heard.try_recv().is_ok())?;
        anyhow::ensure!(opened, "the line never opened");
        Ok(Self { stream, heard })
    }

    fn make(
        &mut self,
        hp: &mut Headless,
        desktop: usize,
        name: &str,
        folder: &Path,
    ) -> anyhow::Result<()> {
        use std::io::Write;
        let line = format!(
            "project-make\t{SCREEN}\t{desktop}\t{name}\t{}\n",
            folder.display()
        );
        self.stream.write_all(line.as_bytes())?;
        let mut made = false;
        hp.dispatch_until(WAIT, |_| {
            while let Ok(value) = self.heard.try_recv() {
                made |= value.get("project-made").is_some();
            }
            made
        })?;
        anyhow::ensure!(made, "no project made");
        Ok(())
    }
}

// --- Lot B -----------------------------------------------------------------------

/// A desktop entry `id` named `name` in `dir`, running `exec`.
fn entry(dir: &Path, id: &str, name: &str, exec: &str, extra: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join(format!("{id}.desktop")),
        format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n{extra}"),
    )?;
    Ok(())
}

/// The process whose parent is `pid`, as `/proc` says.
fn child_of(pid: u32) -> Option<u32> {
    std::fs::read_dir("/proc").ok()?.find_map(|e| {
        let e = e.ok()?;
        let stat = std::fs::read_to_string(e.path().join("stat")).ok()?;
        let (head, rest) = stat.rsplit_once(')')?;
        let ppid: u32 = rest.split_whitespace().nth(1)?.parse().ok()?;
        (ppid == pid)
            .then(|| head.split_whitespace().next()?.parse().ok())
            .flatten()
    })
}

/// A line to the control socket, answered or not.
fn control(hp: &Headless, line: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let path = hp.control_socket().context("no control socket")?;
    let mut stream = std::os::unix::net::UnixStream::connect(path)?;
    stream.write_all(format!("{line}\n").as_bytes())?;
    Ok(())
}

#[test]
#[ignore = "screenshots for the maintainer, by hand"]
fn closing_and_reopening_project_desktops() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS") else {
        eprintln!("HOCUS_SHOTS names no folder");
        return Ok(());
    };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out)?;
    let shell = common::shell_program();
    let dir = tempfile::tempdir()?;
    let at = |sub: &str| -> anyhow::Result<PathBuf> {
        let path = dir.path().join(sub);
        std::fs::create_dir_all(&path)?;
        Ok(path)
    };
    let home = at("home")?;
    let hour = Duration::from_secs(3600);
    let panda = repo(&home, "github/hocus-panda", hour / 4)?;
    let site = repo(&home, "github/hocus-site", hour / 2)?;
    repo(&home, "github/hocus-panda-tools", 2 * hour)?;
    repo(&home, "github/hocus-icons", 6 * 24 * hour)?;
    repo(&home, "code/api-meteo", 96 * hour)?;
    std::fs::create_dir_all(panda.join("crates"))?;
    let exe = std::env::current_exe()?;
    // The applications, the compositor's and the shell's.
    let share = at("share")?;
    let apps = share.join("applications");
    let app = |id: &str| {
        format!(
            "env HOCUS_TEST_PROJECT_APP={id} \"{}\" --exact projects::app_program --nocapture",
            exe.display()
        )
    };
    entry(
        &apps,
        "firefox",
        "Firefox",
        &app("firefox"),
        "Icon=firefox\n",
    )?;
    entry(
        &apps,
        "kitty",
        "kitty",
        &app("kitty"),
        "Categories=TerminalEmulator;\n",
    )?;
    entry(&apps, "slack", "Slack", "/nonexistent/slack", "")?;
    // The terminal: a wrapper starting the test's stand-in.
    let npm = dir.path().join("npm");
    std::fs::write(&npm, "#!/usr/bin/perl\n$0 = \"npm run dev\"; sleep 600;\n")?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&npm, std::fs::Permissions::from_mode(0o755))?;
    }
    let wrapper = |name: &str, run: Option<&Path>, title: &str| -> anyhow::Result<PathBuf> {
        use std::os::unix::fs::PermissionsExt;
        let file = dir.path().join(name);
        let run = run.map_or_else(String::new, |r| {
            format!(" HOCUS_TEST_PROJECT_RUN='{}'", r.display())
        });
        std::fs::write(
            &file,
            format!(
                "#!/bin/sh\nexec env HOCUS_TEST_PROJECT_TERMINAL=foot \
                 HOCUS_TEST_PROJECT_TITLE='{title}'{run} '{}' --exact projects::terminal_program --nocapture\n",
                exe.display()
            ),
        )?;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))?;
        Ok(file)
    };
    let plain = wrapper("foot-plain", None, "~/github/hocus-site")?;
    let crates = wrapper("foot-crates", None, "~/github/hocus-panda/crates")?;
    let dev = wrapper("foot-dev", Some(&npm), "npm run dev — ~/github/hocus-site")?;
    entry(
        &apps,
        "foot",
        "Foot",
        &format!("sh {}", plain.display()),
        "Categories=System;TerminalEmulator;\n",
    )?;
    // Closed projects: blog (Slack is no longer installed), api-meteo.
    let state = at("state/hocuspanda")?;
    let yesterday = jiff::Zoned::now()
        .yesterday()?
        .with()
        .hour(18)
        .minute(40)
        .second(0)
        .build()?;
    let saved_window = |app: &str, place: [i32; 4], state: &str, terminal: Option<&Path>| {
        let terminal = terminal.map_or_else(String::new, |f| {
            format!("[window.terminal]\nfolder = \"{}\"\n", f.display())
        });
        format!(
            "[[window]]\nentry = \"{app}\"\napp-id = \"{app}\"\nscreen = \"{SCREEN}\"\n\
             screen-size = [1440, 900]\narea = [8, 8, 1368, 884]\ndesktop = 1\n\
             place = {place:?}\nstate = \"{state}\"\n{terminal}"
        )
    };
    let blog = home.join("code/blog");
    std::fs::create_dir_all(&blog)?;
    std::fs::create_dir_all(state.join("projects.d"))?;
    std::fs::write(
        state.join("projects"),
        format!(
            "[[project]]\nid = \"blog\"\nname = \"blog\"\nfolder = \"{}\"\n\
             made = \"2026-11-01T10:00:00+01:00\"\nclosed = \"{}\"\n\n\
             [[project]]\nid = \"api-meteo\"\nname = \"api-meteo\"\nfolder = \"{}\"\n\
             made = \"2026-10-01T10:00:00+01:00\"\nclosed = \"2026-11-02T18:40:00+01:00\"\n",
            blog.display(),
            yesterday
                .timestamp()
                .display_with_offset(yesterday.offset()),
            home.join("code/api-meteo").display(),
        ),
    )?;
    std::fs::write(
        state.join("projects.d/blog.toml"),
        format!(
            "{}{}{}",
            saved_window("firefox", [8, 8, 700, 860], "left", None),
            saved_window("foot", [760, 120, 600, 400], "floating", Some(&blog)),
            saved_window("slack", [200, 200, 800, 600], "floating", None),
        ),
    )?;
    std::fs::write(
        state.join("projects.d/api-meteo.toml"),
        saved_window(
            "foot",
            [300, 200, 600, 400],
            "floating",
            Some(&home.join("code/api-meteo")),
        ),
    )?;

    let (bus, address) = common::private_bus(dir.path())?;
    let mut bus = bus;
    let env = [
        ("HOME", home.display().to_string()),
        ("XDG_CONFIG_HOME", at("config")?.display().to_string()),
        ("XDG_CACHE_HOME", at("cache")?.display().to_string()),
        ("XDG_STATE_HOME", at("state")?.display().to_string()),
        ("XDG_DATA_HOME", at("data")?.display().to_string()),
        ("XDG_DATA_DIRS", share.display().to_string()),
        ("LC_ALL", "fr_FR.UTF-8".to_owned()),
        ("LANG", "fr_FR.UTF-8".to_owned()),
        ("HOCUS_USER_NAME", "Alex".to_owned()),
    ];
    let script = common::shell_script(dir.path(), &shell, &env)?;
    let mut config = Config::default();
    config.panel.form = FormChoice::Mini;
    config.locale.language = hocus_link::locale::Language::Chosen(hocus_words::Lang::French);
    config.lang = hocus_words::Lang::French;
    config.home = Some(home.clone());
    config.state_dir = Some(state.clone());
    config.applications = vec![apps.clone()];
    config.desktops.names = vec![String::new(), String::new(), "Discussions".into()];
    let mut hp = Headless::start_on_bus(
        HeadlessConfig {
            outputs: vec!["1440x900".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config.clone(),
        Some(script),
        Some(address),
    )?;
    let up = hp.dispatch_until(WAIT, |hp| {
        hp.drawn_layers(hocus_link::DESKTOPS_NAMESPACE).len() == 1
    })?;
    anyhow::ensure!(up, "no desktops' tiles");
    hp.pointer_move(700.0, 450.0);

    // Desktop 1, hocus-panda: Firefox on the left, Claude Code, a
    // terminal in its crates.
    let mut clients = vec![window(
        &mut hp,
        "firefox",
        "#789 Project desktops — Mozilla Firefox",
        (690, 470),
    )?];
    chord(&mut hp, &[KEY_LEFTMETA, common::KEY_LEFT]);
    clients.push(window(
        &mut hp,
        "kitty",
        "Claude Code — hocus-panda",
        (620, 460),
    )?);
    let kitty = hp
        .all_windows()
        .into_iter()
        .find(|w| w.1.as_deref() == Some("kitty"))
        .map(|w| w.0)
        .context("no kitty")?;
    hp.place_window(kitty, hocus_compositor::model::Rect::new(740, 60, 620, 460));
    let mut children = Vec::new();
    let start =
        |hp: &Headless, wrapper: &Path, folder: &Path| -> anyhow::Result<std::process::Child> {
            Ok(std::process::Command::new("sh")
                .arg(wrapper)
                .current_dir(folder)
                .env("WAYLAND_DISPLAY", hp.socket())
                .spawn()?)
        };
    children.push(start(&hp, &crates, &panda.join("crates"))?);
    let ok = hp.dispatch_until(WAIT, |hp| {
        hp.all_windows()
            .iter()
            .any(|w| w.1.as_deref() == Some("foot"))
    })?;
    anyhow::ensure!(ok, "no terminal");
    let foot = hp
        .all_windows()
        .into_iter()
        .find(|w| w.1.as_deref() == Some("foot"))
        .map(|w| w.0)
        .context("no foot")?;
    hp.place_window(foot, hocus_compositor::model::Rect::new(780, 560, 520, 300));
    // Desktop 3: a chat.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_3]);
    clients.push(window(&mut hp, "slack", "Slack — L’équipe", (820, 600))?);
    // Desktop 2, hocus-site: Firefox, a terminal for Claude Code, npm run
    // dev.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    let site_browser = window(
        &mut hp,
        "firefox",
        "hocus-site · Accueil — Mozilla Firefox",
        (700, 500),
    )?;
    chord(&mut hp, &[KEY_LEFTMETA, common::KEY_LEFT]);
    let agent_terminal = start(&hp, &plain, &site)?;
    let agent_pid = agent_terminal.id();
    children.push(agent_terminal);
    let both = |hp: &Headless| {
        hp.all_windows()
            .iter()
            .filter(|w| w.1.as_deref() == Some("foot"))
            .count()
    };
    let ok = hp.dispatch_until(WAIT, |hp| both(hp) == 2)?;
    anyhow::ensure!(ok, "no second terminal");
    children.push(start(&hp, &dev, &site)?);
    let ok = hp.dispatch_until(WAIT, |hp| both(hp) == 3)?;
    anyhow::ensure!(ok, "no third terminal");
    wait(&mut hp, Duration::from_millis(400))?;
    let mut feet: Vec<_> = hp
        .all_windows()
        .into_iter()
        .filter(|w| w.1.as_deref() == Some("foot") && w.2 == 1)
        .map(|w| w.0)
        .collect();
    feet.sort();
    if let [a, b] = feet[..] {
        hp.place_window(a, hocus_compositor::model::Rect::new(744, 40, 610, 380));
        hp.place_window(b, hocus_compositor::model::Rect::new(744, 440, 610, 300));
    }
    let mut line = Line::open(&mut hp)?;
    line.make(&mut hp, 1, "hocus-panda", &panda)?;
    line.make(&mut hp, 2, "hocus-site", &site)?;
    // Claude Code at work in the first terminal of hocus-site.
    let shell_pid = {
        let mut found = None;
        hp.dispatch_until(WAIT, |_| {
            found = child_of(agent_pid);
            found.is_some()
        })?;
        found.context("no shell")?
    };
    control(
        &hp,
        &format!("agent claude-code site working {shell_pid} hocus-site"),
    )?;
    wait(&mut hp, Duration::from_millis(800))?;

    // 1. The list of hocus-site's dot: "Fermer le projet…" under its rows.
    let dot = hp
        .desk_tile_middles()
        .into_iter()
        .find(|(_, _, d, _)| *d == 1)
        .map(|(.., m)| m)
        .context("no dot")?;
    hp.pointer_move(dot.0 - 1.0, dot.1);
    hp.pointer_move(dot.0, dot.1);
    let list = drawn(&mut hp, hocus_link::POPOVER_NAMESPACE)?;
    shoot(
        &mut hp,
        &out,
        "1-list-close",
        Some((
            list.x as u32 - 10,
            list.y as u32 - 10,
            list.width as u32 + 90,
            list.height as u32 + 20,
        )),
    )?;
    hp.pointer_move(700.0, 450.0);
    wait(&mut hp, Duration::from_millis(600))?;

    // 2. hocus-panda's card in the overview: every window in its sketch,
    // its terminal's folder.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_W]);
    wait(&mut hp, Duration::from_millis(500))?;
    let name = hp.overview_project_name(SCREEN, 0).context("no name")?;
    hp.pointer_move(name.0, name.1);
    wait(&mut hp, Duration::from_millis(100))?;
    hp.button(common::BTN_LEFT, true);
    hp.button(common::BTN_LEFT, false);
    let card = drawn(&mut hp, hocus_link::PROJECT_CARD_NAMESPACE)?;
    hp.pointer_move(1100.0, 850.0);
    wait(&mut hp, Duration::from_millis(300))?;
    shoot(
        &mut hp,
        &out,
        "2-open-card",
        Some((card.x as u32, card.y as u32, 620, card.height as u32 + 10)),
    )?;
    press(&mut hp, KEY_ESC);
    wait(&mut hp, Duration::from_millis(300))?;
    press(&mut hp, KEY_ESC);
    let gone = hp.dispatch_until(WAIT, |hp| hp.overview().is_none())?;
    anyhow::ensure!(gone, "the overview stays");
    wait(&mut hp, Duration::from_millis(400))?;

    // 3. Closing hocus-site: its confirmation, what comes back and what
    // stops.
    hp.project_close_ask("hocus-site");
    let confirm = drawn(&mut hp, hocus_link::PROJECT_CLOSE_NAMESPACE)?;
    wait(&mut hp, Duration::from_millis(300))?;
    shoot(
        &mut hp,
        &out,
        "3-confirm",
        Some((
            (confirm.x - 20).max(0) as u32,
            (confirm.y - 20).max(0) as u32,
            confirm.width as u32 + 40,
            confirm.height as u32 + 40,
        )),
    )?;
    // 4. Closed: the toast. The browser closes when asked, as Firefox.
    press(&mut hp, KEY_ENTER);
    let asked = hp.dispatch_until(WAIT, |_| {
        site_browser
            .events
            .try_iter()
            .any(|e| e == common::Event::CloseRequested)
    })?;
    anyhow::ensure!(asked, "the browser was never asked to close");
    site_browser.commands.send(Command::Close)?;
    let closed = hp.dispatch_until(Duration::from_secs(40), |hp| {
        hp.projects()
            .iter()
            .any(|p| p.0 == "hocus-site" && p.2.is_none())
    })?;
    anyhow::ensure!(closed, "hocus-site never closed");
    drawn(&mut hp, hocus_link::LAUNCHER_TOAST_NAMESPACE)?;
    shoot(&mut hp, &out, "4-closed-toast", Some((200, 700, 1040, 200)))?;
    wait(&mut hp, Duration::from_millis(2500))?;

    // 5. The launcher, "projet hoc": the open one, the closed one, the
    // repositories.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
    wait(&mut hp, Duration::from_millis(500))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_SPACE]);
    drawn(&mut hp, hocus_link::LAUNCHER_NAMESPACE)?;
    for key in [25, 19, 24, 36, 18, 20, KEY_SPACE, 35, 24, 46] {
        press(&mut hp, key);
    }
    wait(&mut hp, Duration::from_millis(1500))?;
    shoot(&mut hp, &out, "5-launcher-projet", None)?;
    // 6. → on the closed one: its card.
    press(&mut hp, KEY_DOWN);
    wait(&mut hp, Duration::from_millis(200))?;
    shoot(&mut hp, &out, "5b-launcher-closed-selected", None)?;
    press(&mut hp, KEY_RIGHT);
    let card = drawn(&mut hp, hocus_link::PROJECT_CARD_NAMESPACE)?;
    wait(&mut hp, Duration::from_millis(300))?;
    shoot(
        &mut hp,
        &out,
        "6-closed-card",
        Some((
            (card.x - 20).max(0) as u32,
            (card.y - 20).max(0) as u32,
            card.width as u32 + 40,
            card.height as u32 + 40,
        )),
    )?;
    // 7. "Rouvrir": its windows come back on the empty desktop.
    press(&mut hp, KEY_ENTER);
    let back = hp.dispatch_until(Duration::from_secs(40), |hp| {
        hp.projects()
            .iter()
            .any(|p| p.0 == "hocus-site" && p.2.is_some())
            && !hp.project_coming_back("hocus-site")
    })?;
    anyhow::ensure!(back, "hocus-site never came back");
    wait(&mut hp, Duration::from_millis(1500))?;
    shoot(&mut hp, &out, "7-reopened", None)?;

    // 8. blog: Slack is not installed any more: the notification.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_SPACE]);
    drawn(&mut hp, hocus_link::LAUNCHER_NAMESPACE)?;
    for key in [25, 19, 24, 36, 18, 20, KEY_SPACE, 48, 38, 24, 34] {
        press(&mut hp, key);
    }
    wait(&mut hp, Duration::from_millis(500))?;
    press(&mut hp, KEY_ENTER);
    let told = hp.dispatch_until(Duration::from_secs(40), |hp| {
        hp.project_not_back().is_some()
    })?;
    anyhow::ensure!(told, "no notification");
    wait(&mut hp, Duration::from_millis(2500))?;
    shoot(
        &mut hp,
        &out,
        "8-notification",
        Some((1440 - 520, 0, 520, 260)),
    )?;
    shoot(&mut hp, &out, "8b-blog-back", None)?;

    // 9. Nine desktops taken: the launcher says so, offers to close one.
    config.desktops.count = 9;
    hp.reconfigure(config);
    wait(&mut hp, Duration::from_millis(500))?;
    for key in 2..=10 {
        let desktop = key - 2;
        let free = hp.all_windows().iter().all(|w| w.2 != desktop as usize);
        if free {
            chord(&mut hp, &[KEY_LEFTMETA, key]);
            wait(&mut hp, Duration::from_millis(400))?;
            clients.push(window(&mut hp, "slack", "Slack", (500, 400))?);
        }
    }
    chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
    wait(&mut hp, Duration::from_millis(500))?;
    chord(&mut hp, &[KEY_LEFTMETA, KEY_SPACE]);
    drawn(&mut hp, hocus_link::LAUNCHER_NAMESPACE)?;
    for key in [25, 19, 24, 36, 18, 20, KEY_SPACE, 30, 25, 23] {
        press(&mut hp, key);
    }
    wait(&mut hp, Duration::from_millis(500))?;
    press(&mut hp, KEY_ENTER);
    let refused = hp.dispatch_until(WAIT, |hp| hp.project_no_room().is_some())?;
    anyhow::ensure!(refused, "api-meteo opened");
    wait(&mut hp, Duration::from_millis(1500))?;
    shoot(&mut hp, &out, "9-no-room", None)?;

    drop(clients);
    drop(hp);
    for mut child in children {
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = bus.kill();
    let _ = bus.wait();
    Ok(())
}
