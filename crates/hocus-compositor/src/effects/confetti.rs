//! Confetti for an agent's good news (#811, the board "Confettis des
//! bonnes nouvelles", its JS ported): a burst from the bar's side and a
//! shower from above flutter behind the windows, tumbling (each piece's
//! face turning, its drag with it), settle on the top edges of the
//! windows and of the bar, then melt. Once, for some seconds, then
//! nothing.
//!
//! Stepped at a fixed 60 steps a second from its start (the board's
//! frames), with a seeded generator; its places are global logical
//! pixels. A piece settled is kept on its edge (its holder's) and moves
//! with it; one whose window goes, goes. Drawn by
//! `crate::render::moments`: the pieces in flight on the wallpaper, under
//! the windows and the bar; those settled right in front of their edge,
//! never below its top few pixels.
//!
//! With a maximised window there is no wallpaper to see: they fall in the
//! bar's column instead (#800, the board "Fenêtre maximisée", its JS
//! ported), a shower from above its top, slower and lighter, that settles
//! nowhere and fades out as it ends.

use std::{cell::RefCell, collections::HashMap, time::Duration};

use smithay::backend::renderer::element::Id;

use super::{
    kept::Traced,
    mesh::{Mesh, Shape},
    moments::Place,
    particles::{Rng, STEP},
    snow::Holder,
};

/// Over by then whatever happens.
pub const LAST: Duration = Duration::from_secs(20);
/// At most this many steps at once, after a long pause.
const MOST_STEPS: u64 = 600;
/// The board's pieces: a burst from the bar, a shower from above (over
/// the board's 960 px left of its bar).
const BURST: usize = 240;
const SHOWER: f32 = 160.0;
const SHOWER_WIDTH: f32 = 960.0;
/// A piece's fall, px/s², and how long it lies before it melts, then
/// melts (s).
const FALL: f32 = 380.0;
const LIES: f32 = 2.4;
const MELTS: f32 = 0.9;
/// The board's colours (0 to 255).
const COLOURS: [[f32; 3]; 7] = [
    [255.0, 92.0, 92.0],
    [255.0, 206.0, 102.0],
    [94.0, 230.0, 168.0],
    [108.0, 184.0, 255.0],
    [182.0, 156.0, 255.0],
    [255.0, 150.0, 210.0],
    [255.0, 255.0, 255.0],
];

/// The bar's shower (#800): so many cards over its width but 10 px each
/// side, from up to 120 px above its top; their fall, px/s², their sway;
/// how long each lasts, fading out over its last 0.7 s.
const BAR_SHOWER: usize = 110;
const BAR_IN: f32 = 10.0;
const BAR_ABOVE: f32 = 120.0;
const BAR_FALL: f32 = 160.0;
const BAR_SWAY: f32 = 40.0;
const BAR_LASTS: f32 = 4.5;
const BAR_FADES: f32 = 0.7;
/// The board's colours there: [`COLOURS`] but white.
const BAR_COLOURS: usize = 6;

/// A piece of confetti: a little card, or a ribbon.
#[derive(Debug, Clone)]
pub struct Piece {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    rot: f32,
    vr: f32,
    /// Its face's turn: `cos` of it, how much of it faces the screen.
    phi: f32,
    vphi: f32,
    w: f32,
    h: f32,
    colour: usize,
    ribbon: bool,
    /// How long it has flown (s): the bar's last [`BAR_LASTS`].
    life: f32,
}

/// A piece settled on an edge: where on it (from its left end and its
/// top), and since when (s).
#[derive(Debug, Clone)]
pub struct Settled {
    pub holder: Holder,
    piece: Piece,
    life: f32,
}

impl Settled {
    /// How opaque it is: whole, then melting.
    fn alpha(&self) -> f32 {
        if self.life < LIES {
            1.0
        } else {
            (1.0 - (self.life - LIES) / MELTS).max(0.0)
        }
    }
}

/// A piece lying: its holder, and its place from its edge's left end and
/// top (logical).
pub type Lying = (Holder, f32, f32);

/// An edge confetti may settle on: its holder, its left end and top, its
/// width (global, logical).
pub type Ledge = (Holder, [f32; 3]);

/// The confetti, while they fly or lie.
#[derive(Debug)]
pub struct Confetti {
    start: Duration,
    /// When they were last fired: over [`LAST`] after it at the latest.
    pub fired: Duration,
    steps: u64,
    rng: Rng,
    /// The screen they fly over.
    pub output: String,
    /// Where they fly (global, logical: `x, y, w, h`): its screen, or
    /// its bar's column.
    pub zone: [f32; 4],
    /// Over the wallpaper, or in the bar's column.
    pub place: Place,
    pieces: Vec<Piece>,
    pub settled: Vec<Settled>,
    /// The blank standing for them behind the glass.
    pub blank: Id,
    /// What they drew last in flight, and on each edge (#815).
    pub drawn: RefCell<Traced>,
    pub fronts: RefCell<HashMap<Holder, Traced>>,
}

/// Where a burst leaves from (global, logical), and whether it goes left
/// (a bar on the right) or right.
#[derive(Debug, Clone, Copy)]
pub struct From {
    pub x: f32,
    pub y: f32,
    pub leftward: bool,
}

impl Confetti {
    /// Confetti over `zone` (global, logical) of screen `output`, from
    /// `start`, none fired yet.
    pub fn new(start: Duration, seed: u64, output: String, zone: [f32; 4]) -> Self {
        Self {
            start,
            fired: start,
            steps: 0,
            rng: Rng::new(seed),
            output,
            zone,
            place: Place::Wall,
            pieces: Vec::new(),
            settled: Vec::new(),
            blank: Id::new(),
            drawn: RefCell::default(),
            fronts: RefCell::default(),
        }
    }

    /// Confetti in the bar's column, `zone` (global, logical) of screen
    /// `output`, from `start`, none fired yet (#800).
    pub fn in_bar(start: Duration, seed: u64, output: String, zone: [f32; 4]) -> Self {
        Self {
            place: Place::Bar,
            ..Self::new(start, seed, output, zone)
        }
    }

    /// The board "Fenêtre maximisée"'s `confetti()`: a shower over the
    /// bar's column, from above its top (#800).
    pub fn fire_in_bar(&mut self) {
        let [x, y, w, _] = self.zone;
        let rng = &mut self.rng;
        let mut rnd = |a: f32, b: f32| a + rng.next() * (b - a);
        for i in 0..BAR_SHOWER {
            self.pieces.push(Piece {
                x: x + rnd(BAR_IN, (w - BAR_IN).max(BAR_IN)),
                y: y + rnd(-BAR_ABOVE, -5.0),
                vx: rnd(-20.0, 20.0),
                vy: rnd(30.0, 90.0),
                rot: rnd(0.0, 6.0),
                vr: rnd(-5.0, 5.0),
                phi: rnd(0.0, 6.0),
                vphi: rnd(5.0, 12.0),
                w: rnd(4.0, 7.0),
                h: rnd(7.0, 11.0),
                colour: i % BAR_COLOURS,
                ribbon: false,
                life: 0.0,
            });
        }
    }

    /// The board's `fire()`: a burst `from` the bar, and a shower from
    /// above over `above` (`x, w`, and its top, global, logical).
    pub fn fire(&mut self, from: From, above: [f32; 3]) {
        let rng = &mut self.rng;
        let mut rnd = |a: f32, b: f32| a + rng.next() * (b - a);
        let pi = std::f32::consts::PI;
        for i in 0..BURST {
            let a = rnd(pi * 0.92, pi * 1.42);
            let a = if from.leftward { a } else { pi - a };
            let speed = rnd(650.0, 1500.0);
            let ribbon = rnd(0.0, 1.0) < 0.18;
            self.pieces.push(Piece {
                x: from.x + rnd(-6.0, 6.0),
                y: from.y + rnd(-10.0, 10.0),
                vx: a.cos() * speed,
                vy: a.sin() * speed,
                rot: rnd(0.0, std::f32::consts::TAU),
                vr: rnd(-8.0, 8.0),
                phi: rnd(0.0, std::f32::consts::TAU),
                vphi: rnd(6.0, 16.0),
                w: rnd(5.0, 9.0),
                h: rnd(9.0, 15.0),
                colour: i % COLOURS.len(),
                ribbon,
                life: 0.0,
            });
        }
        let [x, w, top] = above;
        let shower = (SHOWER * w / SHOWER_WIDTH).round() as usize;
        for i in 0..shower {
            let ribbon = rnd(0.0, 1.0) < 0.18;
            self.pieces.push(Piece {
                x: x + rnd(0.0, w),
                y: top + rnd(-260.0, -10.0),
                vx: rnd(-30.0, 30.0),
                vy: rnd(20.0, 120.0),
                rot: rnd(0.0, std::f32::consts::TAU),
                vr: rnd(-6.0, 6.0),
                phi: rnd(0.0, std::f32::consts::TAU),
                vphi: rnd(5.0, 13.0),
                w: rnd(5.0, 9.0),
                h: rnd(9.0, 15.0),
                colour: (i * 3) % COLOURS.len(),
                ribbon,
                life: 0.0,
            });
        }
    }

    /// Whether they are over at `now`: none flies nor lies, or their time
    /// since they were last fired is gone.
    pub fn over(&self, now: Duration) -> bool {
        (self.pieces.is_empty() && self.settled.is_empty()) || now.saturating_sub(self.start) > LAST
    }

    /// How many fly and how many lie, for the tests.
    pub fn counts(&self) -> (usize, usize) {
        (self.pieces.len(), self.settled.len())
    }

    /// Steps them to `now`, settling on `ledges` (front first); those
    /// lying on an edge no longer there go.
    pub fn advance(&mut self, now: Duration, ledges: &[Ledge]) {
        self.settled
            .retain(|s| ledges.iter().any(|(h, _)| *h == s.holder));
        let due = (now.saturating_sub(self.start).as_secs_f64() / f64::from(STEP)) as u64;
        let from = self.steps.max(due.saturating_sub(MOST_STEPS));
        for _ in from..due {
            self.step(STEP, ledges);
        }
        self.steps = self.steps.max(due);
        let fronts = self.fronts.get_mut();
        let settled = &self.settled;
        fronts.retain(|h, _| settled.iter().any(|s| s.holder == *h));
    }

    /// The board's frame, `dt` seconds on.
    fn step(&mut self, dt: f32, ledges: &[Ledge]) {
        if self.place == Place::Bar {
            self.step_in_bar(dt);
            return;
        }
        let [zx, zy, zw, zh] = self.zone;
        let rng = &mut self.rng;
        let mut landed = Vec::new();
        self.pieces.retain_mut(|p| {
            let flat = p.phi.cos().abs();
            let drag = 0.45 + 2.3 * flat;
            p.vx *= (-drag * dt).exp();
            p.vy = p.vy * (-drag * dt * 0.8).exp() + FALL * dt;
            p.vx += p.phi.sin() * 70.0 * dt * flat;
            let before = p.y;
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.rot += p.vr * dt;
            p.phi += p.vphi * dt;
            if p.vy > 0.0 {
                let on = ledges.iter().find(|(_, [x, y, w])| {
                    p.x > x + 4.0 && p.x < x + w - 4.0 && before <= *y && p.y >= *y
                });
                if let Some((holder, [x, _, _])) = on {
                    let mut rnd = |a: f32, b: f32| a + rng.next() * (b - a);
                    let lying = Piece {
                        x: p.x - x,
                        y: -2.0 - rnd(0.0, 3.0),
                        phi: rnd(-0.4, 0.4),
                        rot: rnd(-0.5, 0.5)
                            + if rnd(0.0, 1.0) < 0.5 {
                                0.0
                            } else {
                                std::f32::consts::FRAC_PI_2
                            },
                        ..p.clone()
                    };
                    landed.push(Settled {
                        holder: holder.clone(),
                        piece: lying,
                        life: 0.0,
                    });
                    return false;
                }
            }
            let gone = p.y > zy + zh + 30.0 || p.x < zx - 40.0 || p.x > zx + zw + 40.0;
            !gone
        });
        self.settled.append(&mut landed);
        for s in &mut self.settled {
            s.life += dt;
        }
        self.settled.retain(|s| s.alpha() > 0.0);
    }

    /// The board "Fenêtre maximisée"'s frame, `dt` seconds on: the pieces
    /// fall through the bar's column, settling nowhere, and go as they
    /// end or leave its bottom.
    fn step_in_bar(&mut self, dt: f32) {
        let bottom = self.zone[1] + self.zone[3];
        self.pieces.retain_mut(|p| {
            p.life += dt;
            if p.life >= BAR_LASTS || p.y >= bottom + 20.0 {
                return false;
            }
            let flat = p.phi.cos().abs();
            p.vx *= (-(0.6 + 2.0 * flat) * dt).exp();
            p.vy = p.vy * (-(0.5 + 1.6 * flat) * dt).exp() + BAR_FALL * dt;
            p.vx += p.phi.sin() * BAR_SWAY * dt * flat;
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.rot += p.vr * dt;
            p.phi += p.vphi * dt;
            true
        });
    }

    /// The pieces in flight on a screen whose corner is `origin` (global,
    /// logical), at `scale`: output-local, physical.
    pub fn flying(&self, origin: (f64, f64), scale: f32) -> Mesh {
        let mut mesh = Mesh::default();
        let (ox, oy) = (origin.0 as f32, origin.1 as f32);
        if self.place == Place::Bar {
            for p in &self.pieces {
                draw_in_bar(
                    &mut mesh,
                    p,
                    [(p.x - ox) * scale, (p.y - oy) * scale],
                    scale,
                );
            }
            return mesh;
        }
        for p in &self.pieces {
            draw(
                &mut mesh,
                p,
                [(p.x - ox) * scale, (p.y - oy) * scale],
                scale,
                1.0,
            );
        }
        mesh
    }

    /// The pieces lying on `holder`'s edge, its left end and top at `edge`
    /// (output-local, physical), `alpha` as opaque as it is drawn; none
    /// when none lies there.
    pub fn lying(&self, holder: &Holder, edge: [f32; 2], scale: f32, alpha: f32) -> Option<Mesh> {
        let mut mesh = Mesh::default();
        for s in self.settled.iter().filter(|s| s.holder == *holder) {
            let p = &s.piece;
            draw(
                &mut mesh,
                p,
                [edge[0] + p.x * scale, edge[1] + p.y * scale],
                scale,
                s.alpha() * alpha,
            );
        }
        (!mesh.is_empty()).then_some(mesh)
    }

    /// Whether any lies on `holder`'s edge.
    pub fn lies_on(&self, holder: &Holder) -> bool {
        self.settled.iter().any(|s| s.holder == *holder)
    }

    /// Their places now (global, logical): in flight, and lying (each
    /// with its holder and its place on its edge from its left end and
    /// top), for the tests.
    pub fn places(&self) -> (Vec<(f32, f32)>, Vec<Lying>) {
        (
            self.pieces.iter().map(|p| (p.x, p.y)).collect(),
            self.settled
                .iter()
                .map(|s| (s.holder.clone(), s.piece.x, s.piece.y))
                .collect(),
        )
    }
}

/// The board's `draw()`: piece `p` at `at` (physical pixels), `alpha`
/// opaque. Its face's turn shades it and narrows it; a card facing the
/// screen has a sheen down its left; a ribbon curls one way, then the
/// other.
fn draw(mesh: &mut Mesh, p: &Piece, at: [f32; 2], scale: f32, alpha: f32) {
    let flip = p.phi.cos();
    let k = if flip >= 0.0 {
        0.75 + 0.25 * flip
    } else {
        0.5 + 0.2 * -flip
    };
    let c = COLOURS.get(p.colour).copied().unwrap_or([255.0; 3]);
    let colour = [
        c[0] / 255.0 * k * alpha,
        c[1] / 255.0 * k * alpha,
        c[2] / 255.0 * k * alpha,
        alpha,
    ];
    let (sin, cos) = p.rot.sin_cos();
    // From the piece's own frame to the screen's.
    let turn = |x: f32, y: f32| {
        [
            at[0] + (x * cos - y * sin) * scale,
            at[1] + (x * sin + y * cos) * scale,
        ]
    };
    if p.ribbon {
        // The board's curve, from (-10, 0) to (10, 0) through (∓4, ∓8 flip).
        let width = 3.0 * flip.abs().max(0.25);
        let point = |t: f32| {
            let u = 1.0 - t;
            let x = u * u * u * -10.0
                + 3.0 * u * u * t * -4.0
                + 3.0 * u * t * t * 4.0
                + t * t * t * 10.0;
            let y = 3.0 * u * u * t * (-8.0 * flip) + 3.0 * u * t * t * (8.0 * flip);
            (x, y)
        };
        const PIECES: usize = 6;
        let mut last = point(0.0);
        for i in 1..=PIECES {
            let next = point(i as f32 / PIECES as f32);
            let (mx, my) = ((last.0 + next.0) / 2.0, (last.1 + next.1) / 2.0);
            let (dx, dy) = (next.0 - last.0, next.1 - last.1);
            let len = dx.hypot(dy);
            card(
                mesh,
                turn(mx, my),
                p.rot + dy.atan2(dx),
                [(len + width) / 2.0 * scale, width / 2.0 * scale],
                width / 2.0 * scale,
                colour,
            );
            last = next;
        }
        return;
    }
    let tall = (p.h * flip.abs()).max(0.8);
    card(
        mesh,
        at,
        p.rot,
        [p.w / 2.0 * scale, tall / 2.0 * scale],
        0.0,
        colour,
    );
    if flip > 0.6 {
        // Its sheen: the left 35 % of it, white at 35 %.
        let sheen = 0.35 * alpha;
        card(
            mesh,
            turn(-p.w / 2.0 + p.w * 0.175, 0.0),
            p.rot,
            [p.w * 0.175 * scale, p.h * flip / 2.0 * scale],
            0.0,
            [sheen; 4],
        );
    }
}

/// The board "Fenêtre maximisée"'s card: piece `p` at `at` (physical
/// pixels), its face's turn shading it and narrowing it, fading out over
/// its last [`BAR_FADES`] seconds (#800).
fn draw_in_bar(mesh: &mut Mesh, p: &Piece, at: [f32; 2], scale: f32) {
    let flat = p.phi.cos().abs();
    let k = if p.phi.cos() >= 0.0 {
        0.75 + 0.25 * flat
    } else {
        0.55
    };
    let alpha = ((BAR_LASTS - p.life) / BAR_FADES).clamp(0.0, 1.0);
    let c = COLOURS.get(p.colour).copied().unwrap_or([255.0; 3]);
    let colour = [
        c[0] / 255.0 * k * alpha,
        c[1] / 255.0 * k * alpha,
        c[2] / 255.0 * k * alpha,
        alpha,
    ];
    let tall = (p.h * flat).max(0.8);
    card(
        mesh,
        at,
        p.rot,
        [p.w / 2.0 * scale, tall / 2.0 * scale],
        0.0,
        colour,
    );
}

/// A box at `centre` turned by `rot`, its half sizes `half` and corners'
/// radius (pixels), its edges smoothed over a pixel.
fn card(
    mesh: &mut Mesh,
    centre: [f32; 2],
    rot: f32,
    half: [f32; 2],
    radius: f32,
    colour: [f32; 4],
) {
    let (sin, cos) = rot.sin_cos();
    let (hx, hy) = (half[0] + 1.0, half[1] + 1.0);
    let corner = |x: f32, y: f32| [centre[0] + x * cos - y * sin, centre[1] + x * sin + y * cos];
    mesh.quad(
        [
            corner(-hx, -hy),
            corner(hx, -hy),
            corner(hx, hy),
            corner(-hx, hy),
        ],
        [[-hx, -hy], [hx, -hy], [hx, hy], [-hx, hy]],
        colour,
        Shape::Rounded,
        [half[0], half[1], radius.min(half[0]).min(half[1])],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::WindowId;

    #[test]
    fn they_burst_from_the_bar_settle_on_an_edge_and_melt() {
        let start = Duration::ZERO;
        let mut confetti = Confetti::new(start, 3, "out".into(), [0.0, 0.0, 1200.0, 720.0]);
        confetti.fire(
            From {
                x: 978.0,
                y: 70.0,
                leftward: true,
            },
            [0.0, 960.0, 0.0],
        );
        assert_eq!(confetti.counts().0, BURST + 160);
        let window = Holder::Window(WindowId(1));
        let ledges = [(window.clone(), [90.0, 260.0, 420.0])];
        let mut most = 0;
        for tenth in 1..=200 {
            confetti.advance(start + Duration::from_millis(100 * tenth), &ledges);
            most = most.max(confetti.counts().1);
            if confetti.over(start + Duration::from_millis(100 * tenth)) {
                break;
            }
        }
        assert!(most > 20, "{most} settled at most");
        assert_eq!(confetti.counts(), (0, 0), "all gone");
        // A piece lying sits on the edge: its middle a few pixels above it.
        let mut confetti = Confetti::new(start, 3, "out".into(), [0.0, 0.0, 1200.0, 720.0]);
        confetti.fire(
            From {
                x: 978.0,
                y: 70.0,
                leftward: true,
            },
            [0.0, 960.0, 0.0],
        );
        confetti.advance(Duration::from_secs(2), &ledges);
        let (_, lying) = confetti.places();
        assert!(!lying.is_empty());
        assert!(lying.iter().all(|(h, x, y)| *h == window
            && (4.0..416.0).contains(x)
            && (-5.0..=-2.0).contains(y)));
        // Its window gone, they go.
        confetti.advance(Duration::from_millis(2100), &[]);
        assert!(confetti.places().1.is_empty());
    }

    #[test]
    fn in_the_bar_they_fall_through_its_column_settling_nowhere_and_go() {
        let start = Duration::ZERO;
        let zone = [1060.0, 8.0, 212.0, 784.0];
        let mut confetti = Confetti::in_bar(start, 3, "out".into(), zone);
        confetti.fire_in_bar();
        assert_eq!(confetti.counts().0, BAR_SHOWER);
        // A ledge right under them: the bar's pieces never settle.
        let ledges = [(Holder::Window(WindowId(1)), [1000.0, 200.0, 400.0])];
        for tenth in 1..=60 {
            let now = start + Duration::from_millis(100 * tenth);
            confetti.advance(now, &ledges);
            let (flying, lying) = confetti.places();
            assert!(lying.is_empty());
            for (x, y) in flying {
                assert!((1000.0..1340.0).contains(&x), "{x}");
                assert!(
                    y > zone[1] - BAR_ABOVE && y < zone[1] + zone[3] + 30.0,
                    "{y}"
                );
            }
            if confetti.over(now) {
                // All gone as they end, some 4.5 s on.
                assert!((44..=46).contains(&tenth), "over at {tenth}");
                return;
            }
        }
        panic!("not over");
    }
}
