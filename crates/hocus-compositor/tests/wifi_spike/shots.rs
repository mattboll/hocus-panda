//! Spike #770: the Wi-Fi popover's states shot one after the other, with
//! what each costs in commits, for the canvas and for the widgets
//! (`HOCUS_WIFI_WIDGETS=1`), in a theme and at a scale:
//!
//! ```sh
//! HOCUS_SPIKE_SHOTS=/some/dir HOCUS_SPIKE_THEME=light HOCUS_SPIKE_SCALE=2 \
//!     cargo nextest run -p hocus-compositor --test wifi --run-ignored only spike_shots
//! ```
//!
//! Throwaway, as the spike is.

use std::io::Write as _;

use super::*;

const KEY_U: u32 = 22;
const KEY_I: u32 = 23;
const KEY_HOME: u32 = 102;
const KEY_END: u32 = 107;

/// As [`start`], at `scale` and in `theme`.
fn start_as(shell: &Path, setup: Setup, scale: i32, theme: &str) -> anyhow::Result<Bed> {
    let fakes = Fakes::start()?;
    let nm = Nm::start(fakes.runtime(), &fakes.address, setup)?;
    let bluez = bluez::Bluez::start(fakes.runtime(), &fakes.address, bluez::Setup::default())?;
    let log = Some(fakes.dir.path().join("shell.log"));
    let opener = opener(&fakes)?;
    let env = [("HOCUS_OPENER", opener.display().to_string())];
    let script = fakes.shell_with(shell, "fr_FR.UTF-8", &env, log.as_deref())?;
    let mut config = Config::default();
    config.keyboard.layouts = vec!["fr".to_owned()];
    if theme == "light" {
        config.appearance.theme = hocus_link::theme::Scheme::Light;
    }
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec![format!("{}x{}@{scale}", 1920 * scale, 1080 * scale).parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.connections_surfaces()
            .first()
            .is_some_and(|(_, r)| r.height == TWO_ROWS)
            && hp.connections_commits() > 0
            && hp.levels_commits() > 0
    })?;
    anyhow::ensure!(drawn, "no connections: {:?}", hp.connections_surfaces());
    let mut bed = Bed {
        fakes,
        nm,
        _bluez: bluez,
        hp,
        log,
    };
    bed.settle()?;
    Ok(bed)
}

struct Shots {
    dir: PathBuf,
    scale: i32,
    said: std::fs::File,
}

impl Shots {
    /// The popover as the screen shows it, 8 px around it too.
    fn shot(&mut self, bed: &mut Bed, name: &str) -> anyhow::Result<()> {
        wait(&mut bed.hp, Duration::from_millis(700))?;
        let rect = *bed
            .popovers()
            .first()
            .ok_or_else(|| anyhow::anyhow!("{name}: no popover"))?;
        let (width, height, bytes) = bed.hp.screenshot(0)?;
        let s = self.scale;
        let (x0, y0) = ((rect.x - 8) * s, (rect.y - 8).max(0) * s);
        let w = ((rect.width + 16) * s).min(width - x0);
        let h = ((rect.height + 16) * s).min(height - y0);
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
        out.save(self.dir.join(format!("{name}.png")))?;
        writeln!(self.said, "shot {name}: popover {rect:?}")?;
        Ok(())
    }

    /// The commits of the bar's, the levels', the connections' and the
    /// popover's surfaces over `during`, after `act`.
    fn cost(
        &mut self,
        bed: &mut Bed,
        name: &str,
        during: Duration,
        act: impl FnOnce(&mut Bed) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let before = bed.counts();
        let frames = bed.hp.frames();
        act(bed)?;
        wait(&mut bed.hp, during)?;
        let after = bed.counts();
        writeln!(
            self.said,
            "cost {name} ({} ms): bar {} levels {} connections {} popover {} frames {}",
            during.as_millis(),
            after.0 - before.0,
            after.1 - before.1,
            after.2 - before.2,
            after.3 - before.3,
            bed.hp.frames() - frames,
        )?;
        Ok(())
    }
}

#[test]
#[ignore = "spike #770: writes screenshots and costs under HOCUS_SPIKE_SHOTS"]
fn spike_shots() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let Ok(dir) = std::env::var("HOCUS_SPIKE_SHOTS") else {
        eprintln!("HOCUS_SPIKE_SHOTS is not set: skipped");
        return Ok(());
    };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir)?;
    let scale: i32 = std::env::var("HOCUS_SPIKE_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let theme = std::env::var("HOCUS_SPIKE_THEME").unwrap_or_else(|_| "dark".to_owned());
    let mut shots = Shots {
        said: std::fs::File::create(dir.join("costs.txt"))?,
        dir,
        scale,
    };
    let setup = Setup {
        aps: around(),
        saved: vec![
            (LIVEBOX, Some("livebox-test")),
            ("Bbox-55D1", Some("bbox-test")),
        ],
        current: Some(LIVEBOX),
        ..Setup::default()
    };
    let mut bed = start_as(&shell, setup, scale, &theme)?;
    bed.hp.pointer_move(600.0, 500.0);
    let item = bed.item();
    bed.click(item)?;
    let rect = popover(height(&[CURRENT], 7, 0.0));
    bed.popover_is(Some(rect))?;
    bed.asked(&Call::Scan)?;
    wait(&mut bed.hp, Duration::from_secs(2))?;
    bed.settle()?;
    // 1. The list: one connected, saved, open, protected and 802.1X ones.
    shots.shot(&mut bed, "01-list")?;
    let still = Duration::from_secs(2);
    shots.cost(&mut bed, "at rest, open", still, |_| Ok(()))?;
    // 2. A row under the pointer.
    let blocks = [CURRENT];
    let (x, y) = row(rect, &blocks, 2);
    let brief = Duration::from_millis(400);
    shots.cost(&mut bed, "pointer onto a row", brief, |bed| {
        bed.hp.pointer_move(x, y);
        Ok(())
    })?;
    shots.shot(&mut bed, "02-hover-row")?;
    shots.cost(&mut bed, "pointer moved within the row", brief, |bed| {
        bed.hp.pointer_move(x + 12.0, y + 3.0);
        Ok(())
    })?;
    shots.cost(&mut bed, "pointer to the next row", brief, |bed| {
        bed.hp.pointer_move(x, y + ROW_STEP);
        Ok(())
    })?;
    shots.cost(&mut bed, "a wheel notch over the list", brief, |bed| {
        bed.hp.wheel(1);
        Ok(())
    })?;
    shots.shot(&mut bed, "03-scrolled")?;
    shots.cost(&mut bed, "the wheel back", brief, |bed| {
        bed.hp.wheel(-1);
        Ok(())
    })?;
    // Disconnect under the pointer.
    let leave = (
        f64::from(rect.x) + 230.0,
        f64::from(rect.y) + HEADER_END + GAP + CURRENT / 2.0,
    );
    bed.hp.pointer_move(leave.0, leave.1);
    shots.shot(&mut bed, "04-hover-button")?;
    // 3. The keyboard's ring: the switch, Disconnect, a row.
    bed.hp
        .pointer_move(f64::from(rect.x + 100), f64::from(rect.y + 28));
    wait(&mut bed.hp, brief)?;
    shots.cost(&mut bed, "Tab", brief, |bed| {
        bed.press(KEY_TAB);
        Ok(())
    })?;
    shots.shot(&mut bed, "05-focus-switch")?;
    bed.press(KEY_TAB);
    shots.shot(&mut bed, "06-focus-button")?;
    bed.press(KEY_TAB);
    bed.press(KEY_TAB);
    shots.shot(&mut bed, "07-focus-row")?;
    // 4. The Freebox's password: Enter on its row unfolds its field.
    shots.cost(&mut bed, "Enter unfolds the row", brief, |bed| {
        bed.press(KEY_ENTER);
        Ok(())
    })?;
    wait(&mut bed.hp, brief)?;
    shots.shot(&mut bed, "08-field-empty")?;
    for key in [KEY_Q, KEY_W, KEY_E, KEY_R, KEY_T] {
        bed.press(key);
    }
    shots.shot(&mut bed, "09-password")?;
    shots.cost(&mut bed, "the field holds the keyboard", still, |_| Ok(()))?;
    shots.cost(&mut bed, "a character typed", brief, |bed| {
        bed.press(KEY_Y);
        Ok(())
    })?;
    // Selected (the canvas's field has no selection).
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, KEY_HOME]);
    shots.shot(&mut bed, "10-password-selected")?;
    bed.press(KEY_END);
    // The eye: Tab goes to it, Space shows the password.
    bed.press(KEY_TAB);
    shots.shot(&mut bed, "11-focus-eye")?;
    bed.press(KEY_SPACE);
    shots.shot(&mut bed, "12-password-shown")?;
    bed.press(KEY_SPACE);
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, KEY_TAB]);
    for key in [KEY_U, KEY_I] {
        bed.press(key);
    }
    shots.shot(&mut bed, "13-password-complete")?;
    // 5. Connecting, then refused: "azertyui" is not its password.
    bed.nm.set_takes(Duration::from_secs(4));
    shots.cost(&mut bed, "Enter tries the password", brief, |bed| {
        bed.press(KEY_ENTER);
        Ok(())
    })?;
    shots.shot(&mut bed, "14-connecting")?;
    wait(&mut bed.hp, Duration::from_secs(6))?;
    shots.shot(&mut bed, "15-wrong-password")?;
    // 6. Folded, then the Wi-Fi off.
    bed.press(KEY_ESC);
    wait(&mut bed.hp, brief)?;
    shots.shot(&mut bed, "16-folded")?;
    let switch = (f64::from(rect.x) + 245.0, f64::from(rect.y) + 13.0 + 16.0);
    shots.cost(&mut bed, "a click on the switch", brief, |bed| {
        bed.click(switch)
    })?;
    wait(&mut bed.hp, Duration::from_secs(1))?;
    shots.shot(&mut bed, "17-off")?;
    if let Some(log) = &bed.log {
        let said: String = plain(log)
            .lines()
            .filter(|l| l.contains("widgets' height") || l.contains("popover's size"))
            .map(|l| format!("{l}\n"))
            .collect();
        std::fs::write(shots.dir.join("heights.txt"), said)?;
    }
    Ok(())
}
