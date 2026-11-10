//! The music player (s12): the real shell following MPRIS players on a
//! D-Bus daemon of the test's own (never the session's), the players
//! being this test's own, made with zbus. No real application.
//!
//! No player, no tile; a player playing shows its tile and its popover;
//! the tile's button sends `PlayPause`; two players, the last to play in
//! front, the other listed, a click bringing it forward. And the mockup's
//! states shot (`HOCUS_MUSIC_SHOTS=/some/dir`, ignored otherwise).
//!
//! Pixman here, so the shell draws on the CPU, as in CI.

use crate::common;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use common::{WAIT, wait};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use hocus_link::{MUSIC_NAMESPACE, POPOVER_NAMESPACE};
use zbus::{OwnedVariant, Variant, names::BusName};

const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

/// What a player says, as the test sets it.
#[derive(Debug, Clone)]
struct Said {
    identity: String,
    status: String,
    title: String,
    artist: String,
    album: String,
    art: String,
    length: i64,
    position: i64,
    shuffle: bool,
}

type Heard = Arc<Mutex<Vec<String>>>;

fn hear(heard: &Heard, what: &str) {
    heard
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(what.to_owned());
}

struct Root {
    said: Arc<Mutex<Said>>,
    heard: Heard,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        hear(&self.heard, "Raise");
    }

    #[zbus(property)]
    fn identity(&self) -> String {
        self.said
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .identity
            .clone()
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }
}

struct Player {
    said: Arc<Mutex<Said>>,
    heard: Heard,
}

fn variant(v: Variant<'static>) -> OwnedVariant {
    OwnedVariant::try_from(v).unwrap_or_else(|_| unreachable!())
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn play_pause(&self) {
        hear(&self.heard, "PlayPause");
    }

    fn next(&self) {
        hear(&self.heard, "Next");
    }

    fn previous(&self) {
        hear(&self.heard, "Previous");
    }

    fn set_position(&self, _track: zbus::ObjectPath<'_>, position: i64) {
        hear(&self.heard, &format!("SetPosition {position}"));
    }

    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.said
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .status
            .clone()
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedVariant> {
        let said = self
            .said
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let mut meta = HashMap::new();
        meta.insert(
            "mpris:trackid".to_owned(),
            variant(Variant::from(
                zbus::ObjectPath::try_from("/org/test/track/1").unwrap_or_else(|_| unreachable!()),
            )),
        );
        meta.insert("xesam:title".to_owned(), variant(Variant::from(said.title)));
        let mut artists = zbus::Array::new(&zbus::Signature::Str);
        let _ = artists.append(Variant::from(said.artist));
        meta.insert("xesam:artist".to_owned(), variant(Variant::Array(artists)));
        meta.insert("xesam:album".to_owned(), variant(Variant::from(said.album)));
        meta.insert(
            "mpris:length".to_owned(),
            variant(Variant::I64(said.length)),
        );
        if !said.art.is_empty() {
            meta.insert("mpris:artUrl".to_owned(), variant(Variant::from(said.art)));
        }
        meta
    }

    #[zbus(property)]
    fn position(&self) -> i64 {
        self.said
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .position
    }

    #[zbus(property)]
    fn shuffle(&self) -> bool {
        self.said
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .shuffle
    }

    #[zbus(property)]
    fn set_shuffle(&mut self, on: bool) {
        hear(&self.heard, &format!("Shuffle {on}"));
        self.said
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .shuffle = on;
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

/// A player of the test's on the bus.
struct Client {
    connection: zbus::Connection,
    said: Arc<Mutex<Said>>,
    heard: Heard,
}

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
    bus: Child,
    address: String,
    runtime: tokio::runtime::Runtime,
}

impl Drop for Bed {
    fn drop(&mut self) {
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}

impl Bed {
    fn start() -> anyhow::Result<Option<Self>> {
        let shell = common::shell_program();
        let daemon = Command::new("sh")
            .args(["-c", "command -v dbus-daemon"])
            .stdout(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !daemon {
            eprintln!("no dbus-daemon: skipped");
            return Ok(None);
        }
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config", "cache", "state", "data"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let (bus, address) = common::private_bus(root)?;
        let log = root.join("shell.log");
        let at = |sub: &str| root.join(sub).display().to_string();
        let script = common::shell_script(
            root,
            &shell,
            &[
                ("LC_ALL", "fr_FR.UTF-8".to_owned()),
                ("LANG", "fr_FR.UTF-8".to_owned()),
                ("HOME", at("home")),
                ("XDG_CONFIG_HOME", at("config")),
                ("XDG_CACHE_HOME", at("cache")),
                ("XDG_STATE_HOME", at("state")),
                ("XDG_DATA_HOME", at("data")),
                ("XDG_DATA_DIRS", at("data")),
                ("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned()),
            ],
        )?;
        let text = std::fs::read_to_string(&script)?
            .replace("\"$@\"\n", &format!("\"$@\" 2>>'{}'\n", log.display()));
        std::fs::write(&script, text)?;
        let mut config = Config::default();
        config.keyboard.layouts = vec!["fr".to_owned()];
        let hp = Headless::start_on_bus(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Pixman,
                render_node: None,
            },
            config,
            Some(script),
            Some(address.clone()),
        )?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let mut bed = Self {
            hp,
            dir,
            log,
            bus,
            address,
            runtime,
        };
        let up = bed.hp.dispatch_until(Duration::from_secs(20), |hp| {
            !hp.drawn_layers(hocus_link::BELL_NAMESPACE).is_empty()
        })?;
        anyhow::ensure!(up, "no footer");
        bed.logged("the music players are followed")?;
        bed.hp.pointer_move(400.0, 300.0);
        wait(&mut bed.hp, Duration::from_millis(200))?;
        Ok(Some(bed))
    }

    fn plain(&self) -> String {
        common::logged(&self.log)
    }

    /// Waits for the shell to log `text` after `from` (an offset in the
    /// log): the last line saying it.
    fn logged_after(&mut self, from: usize, text: &str) -> anyhow::Result<String> {
        let log = self.log.clone();
        let done = self.hp.dispatch_until(WAIT, |_| {
            common::logged(&log)
                .get(from..)
                .is_some_and(|l| l.contains(text))
        })?;
        let after = self.plain().get(from..).unwrap_or_default().to_owned();
        anyhow::ensure!(done, "the shell never logged {text:?}; it logged:\n{after}");
        Ok(after
            .lines()
            .rfind(|l| l.contains(text))
            .unwrap_or_default()
            .to_owned())
    }

    fn logged(&mut self, text: &str) -> anyhow::Result<String> {
        self.logged_after(0, text)
    }

    /// A player named `org.mpris.MediaPlayer2.{name}` comes, saying `said`.
    fn player(&mut self, name: &str, said: Said) -> anyhow::Result<Client> {
        let said = Arc::new(Mutex::new(said));
        let heard = Heard::default();
        let address = self.address.clone();
        let name = format!("org.mpris.MediaPlayer2.{name}");
        let root = Root {
            said: said.clone(),
            heard: heard.clone(),
        };
        let player = Player {
            said: said.clone(),
            heard: heard.clone(),
        };
        let connection = self.runtime.block_on(async {
            zbus::connection::Builder::address(address.as_str())
                .name(name.as_str())
                .serve_at(PATH, root)
                .serve_at(PATH, player)
                .build()
                .await
        })?;
        Ok(Client {
            connection,
            said,
            heard,
        })
    }

    /// The player says its properties changed (after `change`).
    fn change(&self, client: &Client, change: impl FnOnce(&mut Said)) -> anyhow::Result<()> {
        change(&mut client.said.lock().unwrap_or_else(PoisonError::into_inner));
        self.runtime.block_on(client.connection.emit_signal(
            None::<BusName<'_>>,
            PATH,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            &(
                PLAYER,
                HashMap::<String, OwnedVariant>::new(),
                Vec::<String>::new(),
            ),
        ))?;
        Ok(())
    }

    /// The tile, once it shows: where it is on the screen.
    fn tile(&mut self) -> anyhow::Result<Rect> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            hp.drawn_layers(MUSIC_NAMESPACE)
                .iter()
                .any(|(_, r)| r.height > 20)
        })?;
        anyhow::ensure!(done, "no tile: {:?}", self.hp.layer_rects(MUSIC_NAMESPACE));
        common::at_rest(&mut self.hp)?;
        let surface = self.hp.drawn_layers(MUSIC_NAMESPACE)[0].1;
        Ok(Rect {
            x: surface.x + 11,
            y: surface.y + 5,
            width: 172,
            height: 50,
        })
    }

    fn no_tile(&mut self) -> anyhow::Result<()> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            hp.layer_rects(MUSIC_NAMESPACE)
                .iter()
                .all(|(_, r)| r.height <= 20)
        })?;
        anyhow::ensure!(done, "a tile: {:?}", self.hp.layer_rects(MUSIC_NAMESPACE));
        Ok(())
    }

    /// A click on the tile (its round button when `button`).
    fn click_tile(&mut self, button: bool) -> anyhow::Result<()> {
        let tile = self.tile()?;
        let x = if button {
            tile.x + tile.width - 8 - 14
        } else {
            tile.x + 30
        };
        let at = (f64::from(x), f64::from(tile.y + tile.height / 2));
        common::click_on(&mut self.hp, at, MUSIC_NAMESPACE)
    }

    /// The popover, once drawn.
    fn popover(&mut self) -> anyhow::Result<Rect> {
        let done = self
            .hp
            .dispatch_until(WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
        anyhow::ensure!(done, "no popover");
        common::at_rest(&mut self.hp)?;
        Ok(common::drawn_popovers(&self.hp)[0])
    }

    /// What the popover shows by id, once it says `text`, after `from`.
    fn shows_after(&mut self, from: usize, text: &str) -> anyhow::Result<serde_json::Value> {
        let last = |log: &str| {
            log.get(from..)
                .and_then(|l| {
                    l.lines()
                        .rfind(|l| l.contains("shows {") && l.contains("\"music:"))
                })
                .map(str::to_owned)
        };
        let log = self.log.clone();
        let done = self.hp.dispatch_until(WAIT, |_| {
            last(&common::logged(&log)).is_some_and(|l| l.contains(text))
        })?;
        let line = last(&self.plain()).unwrap_or_default();
        anyhow::ensure!(done, "the popover never showed {text:?}: {line}");
        let json = line
            .split_once("shows ")
            .map(|(_, j)| j)
            .unwrap_or_default();
        Ok(serde_json::from_str(json)?)
    }

    /// A click on the popover's control `id`.
    fn click_id(&mut self, shows: &serde_json::Value, id: &str) -> anyhow::Result<()> {
        let pop = self.popover()?;
        let at = shows[id]["at"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("no {id} in {shows}"))?;
        let n = |i: usize| at[i].as_f64().unwrap_or_default();
        let point = (
            f64::from(pop.x) + n(0) + n(2) / 2.0,
            f64::from(pop.y) + n(1) + n(3) / 2.0,
        );
        common::click_on(&mut self.hp, point, POPOVER_NAMESPACE)
    }

    fn heard(&mut self, client: &Client, what: &str) -> anyhow::Result<()> {
        let heard = client.heard.clone();
        let done = self.hp.dispatch_until(WAIT, |_| {
            heard
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .any(|h| h.starts_with(what))
        })?;
        anyhow::ensure!(
            done,
            "the player never heard {what:?}: {:?}",
            client.heard.lock().unwrap_or_else(PoisonError::into_inner)
        );
        Ok(())
    }
}

/// The mockup's records, their covers drawn into `dir`.
fn brassens(dir: &Path, status: &str) -> Said {
    Said {
        identity: "Spotify".to_owned(),
        status: status.to_owned(),
        title: "Les Passantes".to_owned(),
        artist: "Georges Brassens".to_owned(),
        album: "Fernande · 1972".to_owned(),
        art: cover(dir, "c1"),
        length: 240_000_000,
        position: 91_000_000,
        shuffle: true,
    }
}

fn fireship(dir: &Path) -> Said {
    Said {
        identity: "Firefox".to_owned(),
        status: "Playing".to_owned(),
        title: "Rust in 100 seconds — the borrow checker explained".to_owned(),
        artist: "Fireship".to_owned(),
        album: "YouTube".to_owned(),
        art: cover(dir, "c3"),
        length: 151_000_000,
        position: 107_000_000,
        shuffle: true,
    }
}

fn aphex(dir: &Path) -> Said {
    Said {
        identity: "Spotify".to_owned(),
        status: "Paused".to_owned(),
        title: "Avril 14th".to_owned(),
        artist: "Aphex Twin".to_owned(),
        album: "Drukqs".to_owned(),
        art: cover(dir, "c2"),
        length: 125_000_000,
        position: 30_000_000,
        shuffle: true,
    }
}

/// The mockup's drawn covers, as small PNGs: its `.c1`, `.c2`, `.c3`.
fn cover(dir: &Path, which: &str) -> String {
    let path = dir.join(format!("{which}.png"));
    let mix = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    };
    let hex = |h: u32| [(h >> 16) as f32, ((h >> 8) & 255) as f32, (h & 255) as f32];
    let side = 144u32;
    let image = image::RgbaImage::from_fn(side, side, |x, y| {
        let (u, v) = (x as f32 / side as f32, y as f32 / side as f32);
        let rgb = match which {
            // conic-gradient(from 210deg at 30% 70%, #E46A4E, #F2B84B, #6B4FD8, #E46A4E)
            "c1" => {
                let angle = (u - 0.3).atan2(-(v - 0.7)).to_degrees();
                let t = ((angle - 210.0).rem_euclid(360.0)) / 360.0;
                let stops = [hex(0xE46A4E), hex(0xF2B84B), hex(0x6B4FD8), hex(0xE46A4E)];
                let i = ((t * 3.0) as usize).min(2);
                mix(stops[i], stops[i + 1], t * 3.0 - i as f32)
            }
            // linear-gradient(135deg, #2E6F8E, #9BD3C9 60%, #F4E9C8)
            "c2" => {
                let t = (u + v) / 2.0;
                if t < 0.6 {
                    mix(hex(0x2E6F8E), hex(0x9BD3C9), t / 0.6)
                } else {
                    mix(hex(0x9BD3C9), hex(0xF4E9C8), (t - 0.6) / 0.4)
                }
            }
            // radial-gradient(circle at 65% 35%, #F7D774 0 18%, #C2463B 19% 40%, #1E1A2E 41%)
            _ => {
                let d = ((u - 0.65).powi(2) + (v - 0.35).powi(2)).sqrt() / 0.919;
                if d < 0.18 {
                    hex(0xF7D774)
                } else if d < 0.40 {
                    hex(0xC2463B)
                } else {
                    hex(0x1E1A2E)
                }
            }
        };
        image::Rgba([rgb[0] as u8, rgb[1] as u8, rgb[2] as u8, 255])
    });
    let _ = image.save(&path);
    format!("file://{}", path.display())
}

#[test]
fn without_a_player_no_tile() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start()? else {
        return Ok(());
    };
    bed.logged("shows=no player")?;
    bed.no_tile()?;
    // A player comes and goes: its tile with it.
    let dir = bed.dir.path().to_owned();
    let client = bed.player("spotify", brassens(&dir, "Playing"))?;
    bed.tile()?;
    drop(client);
    bed.no_tile()?;
    Ok(())
}

#[test]
fn a_player_playing_shows_its_tile_and_its_popover() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start()? else {
        return Ok(());
    };
    let dir = bed.dir.path().to_owned();
    let _client = bed.player("spotify", brassens(&dir, "Playing"))?;
    bed.logged("shows=Les Passantes — Georges Brassens (Spotify, playing)")?;
    bed.tile()?;
    let from = bed.plain().len();
    bed.click_tile(false)?;
    let shows = bed.shows_after(from, "Les Passantes")?;
    let said = shows.to_string();
    for text in [
        "Musique",
        "Spotify",
        "Georges Brassens",
        "Fernande · 1972",
        "Afficher Spotify",
    ] {
        anyhow::ensure!(said.contains(text), "{text:?} not in {said}");
    }
    anyhow::ensure!(said.contains("−2:"), "no time left in {said}");
    Ok(())
}

#[test]
fn the_tiles_button_and_the_popovers_send_play_pause() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start()? else {
        return Ok(());
    };
    let dir = bed.dir.path().to_owned();
    let client = bed.player("spotify", brassens(&dir, "Playing"))?;
    bed.tile()?;
    bed.click_tile(true)?;
    bed.heard(&client, "PlayPause")?;
    // The tile's button opens no popover.
    wait(&mut bed.hp, Duration::from_millis(300))?;
    anyhow::ensure!(
        common::drawn_popovers(&bed.hp).is_empty(),
        "a popover opened"
    );
    bed.change(&client, |s| s.status = "Paused".to_owned())?;
    bed.logged("(Spotify, paused)")?;
    // In the popover, its big button.
    let from = bed.plain().len();
    bed.click_tile(false)?;
    let shows = bed.shows_after(from, "en pause")?;
    client
        .heard
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
    bed.click_id(&shows, "music:Play")?;
    bed.heard(&client, "PlayPause")?;
    Ok(())
}

#[test]
fn two_players_the_last_to_play_in_front_a_click_brings_the_other() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start()? else {
        return Ok(());
    };
    let dir = bed.dir.path().to_owned();
    let _spotify = bed.player("spotify", aphex(&dir))?;
    let _firefox = bed.player("firefox.instance_1_23", fireship(&dir))?;
    bed.logged("shows=Rust in 100 seconds — the borrow checker explained — Fireship (Firefox, playing); others: Avril 14th")?;
    let from = bed.plain().len();
    bed.click_tile(false)?;
    let shows = bed.shows_after(from, "Avril 14th")?;
    let said = shows.to_string();
    anyhow::ensure!(said.contains("Firefox"), "{said}");
    bed.click_id(&shows, "music:Other(0)")?;
    bed.logged("shows=Avril 14th — Aphex Twin (Spotify, paused); others: Rust in 100")?;
    Ok(())
}

/// A tiny HTTP server on 127.0.0.1 that answers every request with a
/// PNG of `rgb` alone: its address, and how many requests it had.
fn cover_server(rgb: [u8; 3]) -> anyhow::Result<(String, Arc<Mutex<u32>>)> {
    use std::io::{Read as _, Write as _};
    let mut png = Vec::new();
    image::RgbImage::from_pixel(32, 32, image::Rgb(rgb))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = format!("http://{}/cover.png", listener.local_addr()?);
    let asked = Arc::new(Mutex::new(0));
    let count = asked.clone();
    // It ends with the test's process.
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            *count.lock().unwrap_or_else(PoisonError::into_inner) += 1;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                png.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&png);
        }
    });
    Ok((address, asked))
}

#[test]
fn a_cover_on_an_http_url_is_fetched_and_shown() -> anyhow::Result<()> {
    const MAGENTA: [u8; 3] = [255, 0, 255];
    let Some(mut bed) = Bed::start()? else {
        return Ok(());
    };
    let (url, asked) = cover_server(MAGENTA)?;
    let dir = bed.dir.path().to_owned();
    let _client = bed.player(
        "spotify",
        Said {
            art: url,
            ..brassens(&dir, "Playing")
        },
    )?;
    bed.logged("a cover fetched")?;
    let from = bed.plain().len();
    bed.click_tile(false)?;
    let shows = bed.shows_after(from, "Les Passantes")?;
    let pop = bed.popover()?;
    // The cover: on the left of the title's block, as tall as it.
    let now = shows["music:now"]["at"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("no cover's block in {shows}"))?;
    let n = |i: usize| now[i].as_f64().unwrap_or_default() as i32;
    let (x, y) = (pop.x + n(0) + 36, pop.y + n(1) + n(3) / 2);
    let start = std::time::Instant::now();
    loop {
        let (width, _, bytes) = bed.hp.screenshot(0)?;
        let at = usize::try_from(y * width + x)? * 4;
        let p = bytes.get(at..at + 4).unwrap_or(&[0; 4]);
        let seen = [p[2], p[1], p[0]];
        if seen.iter().zip(MAGENTA).all(|(a, b)| a.abs_diff(b) <= 24) {
            break;
        }
        anyhow::ensure!(start.elapsed() < WAIT, "({x}, {y}) shows {seen:?}");
        wait(&mut bed.hp, Duration::from_millis(50))?;
    }
    // Fetched once: the player's later changes find it kept.
    anyhow::ensure!(*asked.lock().unwrap_or_else(PoisonError::into_inner) == 1);
    Ok(())
}

// --- The mockup's states, shot -------------------------------------------------------

/// The screen's part from `rect` grown by `by`, saved as `name`.png.
fn shoot(bed: &mut Bed, dir: &Path, name: &str, rect: Rect, by: i32) -> anyhow::Result<()> {
    wait(&mut bed.hp, Duration::from_millis(500))?;
    common::at_rest(&mut bed.hp)?;
    let (width, height, bytes) = bed.hp.screenshot(0)?;
    let x0 = (rect.x - by).max(0);
    let y0 = (rect.y - by).max(0);
    let w = (rect.width + 2 * by).min(width - x0);
    let h = (rect.height + 2 * by).min(height - y0);
    let mut out = image::RgbaImage::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let at = usize::try_from((y0 + y) * width + x0 + x)? * 4;
            if let Some([b, g, r, _]) = bytes
                .get(at..at + 4)
                .and_then(|p| <[u8; 4]>::try_from(p).ok())
            {
                out.put_pixel(x as u32, y as u32, image::Rgba([r, g, b, 255]));
            }
        }
    }
    out.save(dir.join(format!("{name}.png")))?;
    Ok(())
}

/// The popover and the bar's column beside it.
fn with_column(pop: Rect) -> Rect {
    Rect {
        x: pop.x,
        y: pop.y,
        width: 1920 - pop.x,
        height: 1080 - pop.y,
    }
}

#[test]
#[ignore = "writes the mockup's states under HOCUS_MUSIC_SHOTS"]
fn music_shots() -> anyhow::Result<()> {
    let Ok(out) = std::env::var("HOCUS_MUSIC_SHOTS") else {
        eprintln!("HOCUS_MUSIC_SHOTS is not set: skipped");
        return Ok(());
    };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out)?;
    let only = std::env::var("HOCUS_MUSIC_ONLY").ok();
    let Some(mut bed) = Bed::start()? else {
        return Ok(());
    };
    let dir = bed.dir.path().to_owned();
    // The two players' state alone, as `HOCUS_MUSIC_ONLY=two:NAME` names it.
    if let Some(name) = only.as_deref().and_then(|o| o.strip_prefix("two:")) {
        let _spotify = bed.player("spotify", aphex(&dir))?;
        let _firefox = bed.player("firefox.instance_1_23", fireship(&dir))?;
        bed.logged("(Firefox, playing); others")?;
        let from = bed.plain().len();
        bed.click_tile(false)?;
        bed.shows_after(from, "Avril 14th")?;
        let pop = bed.popover()?;
        bed.hp.pointer_move(1000.0, 500.0);
        return shoot(&mut bed, &out, name, with_column(pop), 12);
    }
    let spotify = bed.player("spotify", brassens(&dir, "Playing"))?;
    bed.logged("(Spotify, playing)")?;
    let tile = bed.tile()?;
    if only.is_none() {
        shoot(&mut bed, &out, "tile-playing", tile, 12)?;
    }
    let from = bed.plain().len();
    bed.click_tile(false)?;
    bed.shows_after(from, "Les Passantes")?;
    let pop = bed.popover()?;
    bed.hp.pointer_move(1000.0, 500.0);
    shoot(&mut bed, &out, "playing", with_column(pop), 12)?;
    if std::env::var("HOCUS_MUSIC_FULL").is_ok() {
        let all = Rect {
            x: 1400,
            y: 0,
            width: 520,
            height: 1080,
        };
        shoot(&mut bed, &out, "full", all, 0)?;
    }
    if only.is_some() {
        return Ok(());
    }
    // Paused.
    bed.change(&spotify, |s| s.status = "Paused".to_owned())?;
    bed.shows_after(from, "en pause")?;
    let pop = bed.popover()?;
    shoot(&mut bed, &out, "paused", with_column(pop), 12)?;
    // Esc closes it.
    common::chord(&mut bed.hp, &[1]);
    wait(&mut bed.hp, Duration::from_millis(500))?;
    shoot(&mut bed, &out, "tile-paused", tile, 12)?;
    // Two players: Spotify paused on Aphex Twin, Firefox playing.
    bed.change(&spotify, |s| *s = aphex(&dir))?;
    let _firefox = bed.player("firefox.instance_1_23", fireship(&dir))?;
    bed.logged("(Firefox, playing); others")?;
    let from = bed.plain().len();
    bed.click_tile(false)?;
    bed.shows_after(from, "Avril 14th")?;
    let pop = bed.popover()?;
    bed.hp.pointer_move(1000.0, 500.0);
    shoot(&mut bed, &out, "two-players", with_column(pop), 12)?;
    Ok(())
}
