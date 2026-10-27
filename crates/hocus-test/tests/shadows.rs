//! Windows' shadows fall off smoothly at every scale (#261): drawn at the
//! output's pixels, they lighten steadily away from the window, as the
//! Gaussian of CSS's `box-shadow` does, in no steps wider than the screen's
//! 8-bit levels make them, and keep their shape from scale 1 to 2. Panels
//! cast the same shadow (#281): the launcher's is the boards', and the
//! column out over a window casts it on the window.

use std::time::Duration;

use anyhow::Context as _;
use hocus_api::{Reply, Request, Target};
use hocus_compositor::{
    FRAME, HarnessOptions, Headless, HeadlessConfig, OutputSpec, RendererChoice,
};
use hocus_test::{client::SolidWindow, image::Image};

/// The light of the scenarios' afternoon (docs/design/identity.md): the
/// shadow straight down, 36 px, blurred 70 px, half dark in the dark theme.
const DY: f64 = 36.0;
const BLUR: f64 = 70.0;
const DARKEST: f64 = 0.5;

/// The dark theme's base, and the light theme's.
const DARK: [u8; 3] = [0x0E, 0x0F, 0x12];
const LIGHT: [u8; 3] = [0xE9, 0xE6, 0xDF];

/// The focus ring around the window, logical pixels: the lines start past
/// it, however its edge falls on pixels.
const RING: f64 = 2.0;

/// How far out from the window's edge the lines go, logical pixels.
const OUT: f64 = 120.0;

fn compositor(scale: f64) -> anyhow::Result<Headless> {
    let (w, h) = ((1280.0 * scale) as i32, (800.0 * scale) as i32);
    Headless::start_options(HarnessOptions {
        headless: HeadlessConfig {
            outputs: vec![OutputSpec::new(w, h).scale(scale)],
            renderer: RendererChoice::Pixman,
        },
        api_permission: hocus_api::Permission::Test,
        watchdog: None,
        xwayland: false,
        // The wallpapers are the shell's.
        shell: true,
        session_bus: None,
        system_bus: None,
        data_dirs: None,
        child_env: None,
        polkit_helper: None,
        motion: false,
        first_run: false,
        google_caldav: None,
        google_tasks: None,
    })
}

fn wallpaper(compositor: &mut Headless, rgb: [u8; 3]) -> anyhow::Result<()> {
    let [r, g, b] = rgb;
    let refused = compositor.write_config(&format!(
        "[wallpaper]\nkind = \"colour\"\ncolour = \"#{r:02x}{g:02x}{b:02x}\"\n"
    ))?;
    anyhow::ensure!(refused.is_none(), "{refused:?}");
    compositor.dispatch_until(Duration::from_secs(10), Headless::wallpapers_ready)?;
    // Past any crossfade from the last one.
    compositor.advance(Duration::from_secs(2))?;
    compositor.advance(FRAME)
}

fn screenshot(compositor: &mut Headless) -> anyhow::Result<Image> {
    let Reply::Screenshot(shot) = compositor.request(Request::Screenshot {
        target: Target::Output("HEADLESS-1".into()),
        cursor: false,
    })?
    else {
        anyhow::bail!("no screenshot");
    };
    Image::from_png(&shot.png()?)
}

/// The window's rectangle, logical.
fn window(compositor: &mut Headless) -> anyhow::Result<(f64, f64, f64, f64)> {
    let Reply::Windows(windows) = compositor.request(Request::Windows)? else {
        anyhow::bail!("no windows");
    };
    let g = windows.first().map(|w| w.geometry).context("no window")?;
    Ok((
        f64::from(g.x),
        f64::from(g.y),
        f64::from(g.width),
        f64::from(g.height),
    ))
}

/// The standard normal distribution's cumulative function (Abramowitz and
/// Stegun 7.1.26).
fn normal_cdf(z: f64) -> f64 {
    let x = z.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - poly * (-x * x).exp();
    if z < 0.0 {
        0.5 * (1.0 - erf)
    } else {
        0.5 * (1.0 + erf)
    }
}

/// A line of pixels out from the window's edge: each one's value on the
/// wallpaper's brightest channel, and what the Gaussian says it should be.
struct Line {
    what: String,
    got: Vec<f64>,
    ideal: Vec<f64>,
}

impl Line {
    /// Out from the middle of the window's bottom edge, down.
    fn below(shot: &Image, scale: f64, rect: (f64, f64, f64, f64), bg: [u8; 3]) -> Self {
        let (x, y, w, h) = rect;
        let channel = brightest(bg);
        let column = ((x + w / 2.0) * scale) as u32;
        let edge = ((y + h + RING) * scale).ceil() as u32 + 1;
        let shadow_edge = (y + h + DY) * scale;
        Self::sample(
            shot,
            scale,
            bg,
            channel,
            "below",
            (0..(OUT * scale) as u32).map(|d| {
                let row = edge + d;
                let u = f64::from(row) + 0.5 - shadow_edge;
                ((column, row), 1.0 - normal_cdf(u / (BLUR * scale / 2.0)))
            }),
        )
    }

    /// Out from the middle of the window's left edge, to the left (the
    /// column is on the right), where the shadow is at its darkest.
    fn left(shot: &Image, scale: f64, rect: (f64, f64, f64, f64), bg: [u8; 3]) -> Self {
        let (x, y, _, h) = rect;
        let channel = brightest(bg);
        let row = ((y + h / 2.0 + DY) * scale) as u32;
        let edge = ((x - RING) * scale).floor() as u32 - 2;
        let shadow_edge = x * scale;
        Self::sample(
            shot,
            scale,
            bg,
            channel,
            "left",
            (0..(OUT * scale) as u32).map(|d| {
                let column = edge - d;
                let u = shadow_edge - (f64::from(column) + 0.5);
                ((column, row), 1.0 - normal_cdf(u / (BLUR * scale / 2.0)))
            }),
        )
    }

    fn sample(
        shot: &Image,
        scale: f64,
        bg: [u8; 3],
        channel: usize,
        side: &str,
        points: impl Iterator<Item = ((u32, u32), f64)>,
    ) -> Self {
        let base = f64::from(bg[channel]);
        let (got, ideal) = points
            .map(|((x, y), cover)| {
                let px = shot.pixel(x, y).unwrap_or_default();
                (f64::from(px[channel]), base * (1.0 - DARKEST * cover))
            })
            .unzip();
        Self {
            what: format!("{side} at scale {scale} on {bg:?}"),
            got,
            ideal,
        }
    }

    /// Lighter and lighter away from the window, never off the Gaussian by
    /// more than `tolerance` levels, and level only as long as the levels
    /// make it: a run of equal pixels wider than 2 px only where the
    /// Gaussian itself changes by less than a level and the tolerance across
    /// it.
    fn check(&self, tolerance: f64) {
        let Self { what, got, ideal } = self;
        for (i, pair) in got.windows(2).enumerate() {
            assert!(pair[0] <= pair[1], "{what}: darker again at {i}: {got:?}");
        }
        for (i, (g, want)) in got.iter().zip(ideal).enumerate() {
            assert!(
                (g - want).abs() <= tolerance,
                "{what}: {g} at {i}, not {want:.1}: {got:?}"
            );
        }
        let mut start = 0;
        for i in 1..=got.len() {
            if i == got.len() || got[i] != got[start] {
                if i - start > 2 {
                    let change = (ideal[i - 1] - ideal[start]).abs();
                    assert!(
                        change < 1.0 + tolerance,
                        "{what}: a step {} px wide at {start}, over {change:.1} levels: {got:?}",
                        i - start
                    );
                }
                start = i;
            }
        }
    }

    /// The line at logical distance `d` from the edge.
    fn at(&self, scale: f64, d: f64) -> f64 {
        self.got[((d * scale) as usize).min(self.got.len() - 1)]
    }
}

/// The shadow the same on both sides of the window, left and right of its
/// middle (the light falls straight down): its corners and sides are drawn
/// from one quarter, mirrored.
fn symmetric(shot: &Image, scale: f64, rect: (f64, f64, f64, f64), bg: [u8; 3], tolerance: f64) {
    let (x, y, w, h) = rect;
    let channel = brightest(bg);
    let (left, right) = ((x * scale).round() as u32, ((x + w) * scale).round() as u32);
    let reach = (1.5 * BLUR * scale) as u32;
    // Through the window's middle, and below it in the shadow's corners.
    for row in [y + h / 2.0, y + h + DY, y + h + DY + BLUR / 2.0] {
        let row = (row * scale) as u32;
        // Past the focus ring.
        for i in ((RING * scale).ceil() as u32 + 2)..reach {
            let value = |x: u32| f64::from(shot.pixel(x, row).unwrap_or_default()[channel]);
            let (a, b) = (value(left - 1 - i), value(right + i));
            assert!(
                (a - b).abs() <= tolerance,
                "at scale {scale} on {bg:?}, row {row}: {a} left, {b} right, {i} px out"
            );
        }
    }
}

fn brightest(rgb: [u8; 3]) -> usize {
    (0..3).max_by_key(|&i| rgb[i]).unwrap_or(0)
}

#[test]
fn shadows_fall_off_smoothly_and_alike_at_every_scale() -> anyhow::Result<()> {
    let mut lines = Vec::new();
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let mut compositor = compositor(scale)?;
        wallpaper(&mut compositor, DARK)?;
        let _client = SolidWindow {
            app_id: "lit".into(),
            width: 400,
            height: 300,
            argb: 0xff_33_99_ff,
        }
        .spawn(compositor.socket_path())?;
        compositor.dispatch_until(Duration::from_secs(10), |c| c.window_count() == 1)?;
        compositor.advance(FRAME)?;
        let rect = window(&mut compositor)?;
        let dark = screenshot(&mut compositor)?;
        wallpaper(&mut compositor, LIGHT)?;
        let light = screenshot(&mut compositor)?;
        for (shot, bg, tolerance) in [(&dark, DARK, 1.0), (&light, LIGHT, 2.0)] {
            for line in [
                Line::below(shot, scale, rect, bg),
                Line::left(shot, scale, rect, bg),
            ] {
                line.check(tolerance);
                lines.push((scale, bg, line));
            }
            symmetric(shot, scale, rect, bg, tolerance);
        }
    }
    // The same shape at every scale: at the same logical distance, the
    // same shade.
    let scale_1: Vec<_> = lines.iter().filter(|(s, _, _)| *s == 1.0).collect();
    for (scale, bg, line) in &lines {
        let side = line.what.split(' ').next().unwrap_or_default();
        let Some((_, _, reference)) = scale_1
            .iter()
            .find(|(_, b, l)| b == bg && l.what.starts_with(side))
        else {
            continue;
        };
        let tolerance = if *bg == DARK { 1.0 } else { 3.0 };
        for d in 0..OUT as u32 {
            let d = f64::from(d);
            let (got, want) = (line.at(*scale, d), reference.at(1.0, d));
            assert!(
                (got - want).abs() <= tolerance,
                "{}: {got} at {d} logical px, {want} at scale 1",
                line.what
            );
        }
    }
    Ok(())
}

/// How much of the shadow of a box rounded to `radius` there is at pixel
/// centre `(x, y)`, the box's corners at `(x0, y0)` and `(x1, y1)`, all in
/// physical pixels: CSS's Gaussian over the box, its straight sides
/// exactly, less what its rounded corners leave out, summed in quarter
/// pixels.
fn box_cover(
    (x, y): (f64, f64),
    (x0, y0, x1, y1): (f64, f64, f64, f64),
    radius: f64,
    scale: f64,
) -> f64 {
    let sigma = BLUR * scale / 2.0;
    let side = |z: f64, a: f64, b: f64| normal_cdf((z - a) / sigma) - normal_cdf((z - b) / sigma);
    let gauss = |d: f64| {
        (-d * d / (2.0 * sigma * sigma)).exp() / (sigma * (2.0 * std::f64::consts::PI).sqrt())
    };
    let step = 0.25;
    let n = (radius / step).ceil() as i32;
    let mut missing = 0.0;
    for (cx, cy, sx, sy) in [
        (x0 + radius, y0 + radius, -1.0, -1.0),
        (x1 - radius, y0 + radius, 1.0, -1.0),
        (x0 + radius, y1 - radius, -1.0, 1.0),
        (x1 - radius, y1 - radius, 1.0, 1.0),
    ] {
        for i in 0..n {
            for j in 0..n {
                let (u, v) = ((f64::from(i) + 0.5) * step, (f64::from(j) + 0.5) * step);
                if u.hypot(v) > radius {
                    missing += gauss(x - (cx + sx * u)) * gauss(y - (cy + sy * v)) * step * step;
                }
            }
        }
    }
    (side(x, x0, x1) * side(y, y0, y1) - missing).max(0.0)
}

impl Line {
    /// Out from the middle of a panel's bottom edge, down: the shadow of a
    /// box its size, as the boards draw it.
    fn below_panel(
        shot: &Image,
        scale: f64,
        (rect, radius): ((f64, f64, f64, f64), f64),
        bg: [u8; 3],
    ) -> Self {
        let (x, y, w, h) = rect;
        let shadow = (
            x * scale,
            (y + DY) * scale,
            (x + w) * scale,
            (y + h + DY) * scale,
        );
        let column = ((x + w / 2.0) * scale) as u32;
        let edge = ((y + h) * scale).ceil() as u32 + 2;
        Self::sample(
            shot,
            scale,
            bg,
            brightest(bg),
            "below the panel",
            (0..(OUT * scale) as u32).map(|d| {
                let row = edge + d;
                let at = (f64::from(column) + 0.5, f64::from(row) + 0.5);
                ((column, row), box_cover(at, shadow, radius * scale, scale))
            }),
        )
    }

    /// Out from a panel's left edge, to the left, through the middle of its
    /// shadow.
    fn left_of_panel(
        shot: &Image,
        scale: f64,
        (rect, radius): ((f64, f64, f64, f64), f64),
        bg: [u8; 3],
        out: f64,
    ) -> Self {
        let (x, y, w, h) = rect;
        let shadow = (
            x * scale,
            (y + DY) * scale,
            (x + w) * scale,
            (y + h + DY) * scale,
        );
        let row = ((y + h / 2.0 + DY) * scale) as u32;
        let edge = (x * scale).floor() as u32 - 2;
        Self::sample(
            shot,
            scale,
            bg,
            brightest(bg),
            "left of the panel",
            (0..(out * scale) as u32).map(|d| {
                let column = edge - d;
                let at = (f64::from(column) + 0.5, f64::from(row) + 0.5);
                ((column, row), box_cover(at, shadow, radius * scale, scale))
            }),
        )
    }
}

/// The shell's interface, as the control API lists it.
fn ui(compositor: &mut Headless) -> anyhow::Result<Vec<hocus_api::UiNode>> {
    let Reply::Ui(nodes) = compositor.request(Request::Ui)? else {
        anyhow::bail!("no interface");
    };
    Ok(nodes)
}

fn logical(r: hocus_api::Rect) -> (f64, f64, f64, f64) {
    (
        f64::from(r.x),
        f64::from(r.y),
        f64::from(r.width),
        f64::from(r.height),
    )
}

/// Presses `keys` together and lets them go, the last first.
fn press(compositor: &mut Headless, keys: &[u32]) -> anyhow::Result<()> {
    let presses = keys.iter().map(|c| (*c, true));
    let releases = keys.iter().rev().map(|c| (*c, false));
    for (code, pressed) in presses.chain(releases) {
        compositor.request(Request::Key { code, pressed })?;
    }
    Ok(())
}

const KEY_LEFTMETA: u32 = 125;
const KEY_SPACE: u32 = 57;
const KEY_C: u32 = 46;

/// Given the launcher open in the afternoon, when its shadow is looked at
/// below it and beside it, then it is the boards' (Bureau, D07): `0 36px
/// 70px` half dark, as windows' is, within 2 levels at scales 1 and 2,
/// on a dark wallpaper and a light one (#281). It no longer shrinks to its
/// surface.
#[test]
fn the_launcher_casts_the_boards_shadow() -> anyhow::Result<()> {
    for scale in [1.0, 2.0] {
        let mut compositor = compositor(scale)?;
        wallpaper(&mut compositor, DARK)?;
        press(&mut compositor, &[KEY_LEFTMETA, KEY_SPACE])?;
        compositor.advance(Duration::from_secs(1))?;
        // Its card: the widest box of the shell's, its surface aside.
        let card = ui(&mut compositor)?
            .into_iter()
            .filter(|n| n.depth == 1 && n.role == "group")
            .max_by_key(|n| n.rect.width)
            .map(|n| logical(n.rect))
            .context("no launcher")?;
        let dark = screenshot(&mut compositor)?;
        wallpaper(&mut compositor, LIGHT)?;
        let light = screenshot(&mut compositor)?;
        // Its corners, as the shell rounds them.
        let card = (card, 24.0);
        for (shot, bg) in [(&dark, DARK), (&light, LIGHT)] {
            Line::below_panel(shot, scale, card, bg).check(2.0);
            Line::left_of_panel(shot, scale, card, bg, OUT).check(2.0);
        }
    }
    Ok(())
}

/// Given a window under where the column comes out (the strip, by
/// default), when Super+C brings the column out over it (#263), then the
/// column casts the boards' shadow on the window, as windows do (#281).
#[test]
fn the_column_out_over_a_window_casts_a_shadow_on_it() -> anyhow::Result<()> {
    let scale = 1.0;
    let mut compositor = compositor(scale)?;
    wallpaper(&mut compositor, DARK)?;
    let blue = [0x33, 0x99, 0xff];
    let _client = SolidWindow {
        app_id: "under".into(),
        width: 1100,
        height: 700,
        argb: 0xff_33_99_ff,
    }
    .spawn(compositor.socket_path())?;
    compositor.dispatch_until(Duration::from_secs(10), |c| c.window_count() == 1)?;
    compositor.advance(FRAME)?;
    let window = window(&mut compositor)?;
    press(&mut compositor, &[KEY_LEFTMETA, KEY_C])?;
    compositor.advance(Duration::from_secs(1))?;
    let nodes = ui(&mut compositor)?;
    let field = nodes
        .iter()
        .find(|n| n.label.as_deref() == Some("Command field"))
        .map(|n| n.rect)
        .context("the column is not out")?;
    let column = nodes
        .iter()
        .filter(|n| n.depth == 0)
        .map(|n| n.rect)
        .find(|r| r.x <= field.x && r.x + r.width >= field.x + field.width)
        .map(logical)
        .context("no column")?;
    // The line, all over the window.
    let out = 80.0;
    anyhow::ensure!(
        window.0 < column.0 - out - 2.0 && window.0 + window.2 > column.0,
        "the window {window:?} is not under the column's shadow {column:?}"
    );
    let shot = screenshot(&mut compositor)?;
    let line = Line::left_of_panel(&shot, scale, (column, 24.0), blue, out);
    line.check(2.0);
    // Clearly darker at the column's edge.
    assert!(line.got[0] < 255.0 - 40.0, "no shadow: {:?}", line.got);
    Ok(())
}
