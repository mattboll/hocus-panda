//! The same widget column for every candidate renderer: what it contains,
//! and how the time per frame, memory and images are measured.
//!
//! Coordinates are logical pixels; each candidate draws at `scale`.

use std::time::{Duration, Instant};

/// Leaves text out, to see what it costs.
pub static SKIP_TEXT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn skip_text() -> bool {
    SKIP_TEXT.load(std::sync::atomic::Ordering::Relaxed)
}

/// The column's width, from decision 0007.
pub const WIDTH: f32 = 320.0;
pub const GAP: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

/// The bamboo theme of the identity, roughly.
pub const BACKGROUND: Rgba = Rgba(0x10, 0x14, 0x12, 0xff);
pub const CARD: Rgba = Rgba(0x1b, 0x22, 0x1e, 0xf0);
pub const BORDER: Rgba = Rgba(0xff, 0xff, 0xff, 0x14);
pub const TEXT: Rgba = Rgba(0xee, 0xf2, 0xee, 0xff);
pub const MUTED: Rgba = Rgba(0xa8, 0xb4, 0xab, 0xff);
pub const ACCENT: Rgba = Rgba(0x7b, 0xc4, 0x7f, 0xff);
pub const AGENT: Rgba = Rgba(0xb4, 0x8c, 0xff, 0xff);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    /// Manrope: the whole interface.
    Interface,
    /// Fraunces: large titles, the clock.
    Display,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    RoundRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        fill: Rgba,
        /// A 1 px border, inside.
        border: Option<Rgba>,
    },
    /// Wrapped at `width`; `y` is the top of the first line.
    Text {
        x: f32,
        y: f32,
        width: f32,
        size: f32,
        weight: u16,
        family: Family,
        color: Rgba,
        text: String,
    },
    /// A stroked arc, angles in radians from 3 o'clock, clockwise.
    Arc {
        cx: f32,
        cy: f32,
        r: f32,
        start: f32,
        sweep: f32,
        width: f32,
        color: Rgba,
    },
}

/// A widget: its place in the column and its shapes, in its own
/// coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Widget {
    pub name: &'static str,
    pub y: f32,
    pub h: f32,
    pub shapes: Vec<Shape>,
}

impl Widget {
    pub fn w(&self) -> f32 {
        WIDTH
    }
}

fn card(h: f32) -> Shape {
    Shape::RoundRect {
        x: 0.0,
        y: 0.0,
        w: WIDTH,
        h,
        radius: 16.0,
        fill: CARD,
        border: Some(BORDER),
    }
}

fn text(x: f32, y: f32, size: f32, weight: u16, color: Rgba, text: &str) -> Shape {
    Shape::Text {
        x,
        y,
        width: WIDTH - 2.0 * x,
        size,
        weight,
        family: Family::Interface,
        color,
        text: text.to_owned(),
    }
}

fn bar(x: f32, y: f32, w: f32, share: f32, color: Rgba) -> [Shape; 2] {
    [
        Shape::RoundRect {
            x,
            y,
            w,
            h: 6.0,
            radius: 3.0,
            fill: BORDER,
            border: None,
        },
        Shape::RoundRect {
            x,
            y,
            w: w * share,
            h: 6.0,
            radius: 3.0,
            fill: color,
            border: None,
        },
    ]
}

/// The agent card at `frame`: a spinning arc and a changing percentage, the
/// widget that changes every frame.
pub fn agent(frame: u32) -> Widget {
    let t = frame as f32 / 60.0;
    let percent = (frame / 3) % 101;
    Widget {
        name: "agent",
        y: 0.0,
        h: 84.0,
        shapes: vec![
            card(84.0),
            Shape::Arc {
                cx: 42.0,
                cy: 42.0,
                r: 18.0,
                start: t * std::f32::consts::TAU,
                sweep: 4.2,
                width: 4.0,
                color: AGENT,
            },
            text(76.0, 20.0, 15.0, 600, TEXT, "Agent\u{202f}: compilation…"),
            text(
                76.0,
                44.0,
                13.0,
                500,
                MUTED,
                &format!("{percent}\u{202f}% · étape 3 sur 7"),
            ),
        ],
    }
}

/// The whole column at `frame`, widgets placed from the top.
pub fn column(frame: u32) -> Vec<Widget> {
    let mut widgets = vec![
        Widget {
            name: "clock",
            y: 0.0,
            h: 112.0,
            shapes: vec![
                Shape::Text {
                    x: 16.0,
                    y: 4.0,
                    width: WIDTH - 32.0,
                    size: 64.0,
                    weight: 400,
                    family: Family::Display,
                    color: TEXT,
                    text: "14:32".into(),
                },
                text(18.0, 84.0, 15.0, 500, MUTED, "mercredi 22 octobre"),
            ],
        },
        Widget {
            name: "agenda",
            y: 0.0,
            h: 96.0,
            shapes: vec![
                card(96.0),
                text(
                    16.0,
                    14.0,
                    12.0,
                    700,
                    ACCENT,
                    "PROCHAINE RÉUNION · DANS 12 MIN",
                ),
                text(16.0, 36.0, 16.0, 600, TEXT, "Revue d'architecture"),
                text(
                    16.0,
                    62.0,
                    13.0,
                    500,
                    MUTED,
                    "Salle Œuvre, 3e étage — avec Zoé et Gaël",
                ),
            ],
        },
        Widget {
            name: "weather",
            y: 0.0,
            h: 80.0,
            shapes: vec![
                card(80.0),
                text(16.0, 16.0, 16.0, 600, TEXT, "Brume légère, 11\u{a0}°C"),
                text(
                    16.0,
                    44.0,
                    13.0,
                    500,
                    MUTED,
                    "Ressenti 9\u{a0}°C · vent 14\u{a0}km/h · «\u{202f}éclaircies\u{202f}»",
                ),
            ],
        },
    ];
    let mut system = vec![
        card(112.0),
        text(16.0, 14.0, 13.0, 600, TEXT, "Processeur 23\u{202f}%"),
        text(
            16.0,
            62.0,
            13.0,
            600,
            TEXT,
            "Mémoire 6,4\u{a0}Go sur 32\u{a0}Go",
        ),
    ];
    system.extend(bar(16.0, 38.0, WIDTH - 32.0, 0.23, ACCENT));
    system.extend(bar(16.0, 86.0, WIDTH - 32.0, 0.2, ACCENT));
    widgets.push(Widget {
        name: "system",
        y: 0.0,
        h: 112.0,
        shapes: system,
    });
    let mut media = vec![
        card(96.0),
        text(16.0, 14.0, 16.0, 600, TEXT, "Ça plane pour moi"),
        text(16.0, 40.0, 13.0, 500, MUTED, "Plastic Bertrand — 1977"),
    ];
    media.extend(bar(16.0, 72.0, WIDTH - 32.0, 0.41, ACCENT));
    widgets.push(Widget {
        name: "media",
        y: 0.0,
        h: 96.0,
        shapes: media,
    });
    widgets.push(agent(frame));
    widgets.push(Widget {
        name: "notes",
        y: 0.0,
        h: 168.0,
        shapes: vec![
            card(168.0),
            text(16.0, 14.0, 12.0, 700, ACCENT, "NOTES"),
            text(
                16.0,
                36.0,
                14.0,
                500,
                TEXT,
                "Penser à vérifier l'œil-de-bœuf\u{202f}; déjà fait à côté. Ajouter «\u{202f}Paramètres → Écrans\u{202f}» \
                 dans la barre, puis relire les accents\u{202f}: ÀÉÈÊËÎÏÔÙÛÜŸÇ àéèêëîïôùûüÿç, fi fl ffi — 1\u{202f}234,56\u{a0}€.",
            ),
        ],
    });
    let mut y = 0.0;
    for w in &mut widgets {
        w.y = y;
        y += w.h + GAP;
    }
    widgets
}

/// The column's height.
pub fn height(widgets: &[Widget]) -> f32 {
    widgets.last().map_or(0.0, |w| w.y + w.h)
}

/// Times of `n` runs of `f` after `warmup` ones.
pub fn measure(warmup: u32, n: u32, mut f: impl FnMut(u32)) -> Stats {
    for i in 0..warmup {
        f(i);
    }
    let mut times: Vec<Duration> = (0..n)
        .map(|i| {
            let start = Instant::now();
            f(warmup + i);
            start.elapsed()
        })
        .collect();
    times.sort();
    Stats {
        median: times[times.len() / 2],
        p95: times[times.len() * 95 / 100],
        max: times[times.len() - 1],
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Stats {
    pub median: Duration,
    pub p95: Duration,
    pub max: Duration,
}

impl std::fmt::Display for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        write!(
            f,
            "median {:.3} ms, p95 {:.3} ms, max {:.3} ms",
            ms(self.median),
            ms(self.p95),
            ms(self.max)
        )
    }
}

/// Peak resident memory of this process, in KiB (Linux).
pub fn peak_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
}

/// The fonts, read once.
pub struct Fonts {
    pub interface: Vec<u8>,
    pub display: Vec<u8>,
}

impl Fonts {
    pub fn load() -> Self {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts");
        let read = |name: &str| {
            std::fs::read(dir.join(name))
                .unwrap_or_else(|e| panic!("{name}: {e}; run ./fetch-fonts.sh first"))
        };
        Self {
            interface: read("Manrope.ttf"),
            display: read("Fraunces.ttf"),
        }
    }
}

/// Writes premultiplied RGBA pixels as a PNG.
pub fn write_png(path: &std::path::Path, width: u32, height: u32, premultiplied: &[u8]) {
    let mut straight = premultiplied.to_vec();
    for px in straight.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let file = std::fs::File::create(path).expect("create png");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&straight))
        .expect("write png");
}

/// What every candidate reports, one line each, for the decision record.
pub fn report(name: &str, scale: f32, full: Stats, one: Stats, extra: &[(&str, String)]) {
    println!("{name} @{scale}x");
    println!("  full column        {full}");
    println!("  one widget (agent) {one}");
    for (k, v) in extra {
        println!("  {k:<18} {v}");
    }
}
