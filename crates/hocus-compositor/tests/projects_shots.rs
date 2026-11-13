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
use common::{Command, KEY_1, KEY_2, KEY_3, KEY_ESC, KEY_LEFTMETA, WAIT, chord, wait};
use hocus_compositor::{Config, FormChoice, Headless, HeadlessConfig, RendererChoice};

const SCREEN: &str = "HEADLESS-1";
const KEY_W: u32 = 17;
const KEY_ENTER: u32 = 28;

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
