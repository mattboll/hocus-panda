//! A koi pond (#803, the board "Bassin de koïs", its JS ported): the
//! wallpaper is a pond. Its water is a field of heights (the board's
//! 240 × 144 over its 1200 × 720: a cell every 5 logical pixels) rippling
//! as waves do; the koi swim with a spine of fourteen joints that bends
//! no more than 0.32 rad at each, their fins flapping and their patterns
//! on them; they come to the pointer when it rests on the water, circle
//! it, and scatter from a click; a drop falls now and then; lily pads
//! float, bobbing with the water. Drawn by `crate::render::koi`: the koi
//! and their blurred shadows over the pond's floor into a picture, seen
//! through the water (the board's `KOI_FS`), the pads over it.
//!
//! Stepped at a fixed 60 steps a second from its start (the board's
//! frames: two waves a step, as its two a frame), with a seeded
//! generator; as many koi and pads per pixel as the board's seven and six.
//! Its places are logical pixels from its screen's corner; the koi and
//! the pads keep to the screen's work area (beside the bar), as the
//! board's kept left of its bar. The floor, drawn once a screen, is drawn
//! by a worker. The pond itself, its koi and its drawing are
//! `hocus_scenes::koi`'s, which the Hocuspanda camera draws too.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::mpsc,
    time::Duration,
};

use hocus_scenes::koi as sim;
pub use hocus_scenes::koi::{CELL, Pointer, floor};
use smithay::{
    backend::renderer::{element::Id, utils::CommitCounter},
    utils::{Physical, Rectangle},
};

use super::{
    draw::Heights,
    kept::Traced,
    mesh::Mesh,
    particles::{Rng, STEP},
};
use crate::wobble::Kept;

/// The pointer rests once it has not moved more than this (logical
/// pixels) for this long (the board's 0.6 s).
const REST_MOVE: f64 = 3.0;
const REST_AFTER: Duration = Duration::from_millis(600);
/// At most this many steps at once (10 s).
const MOST_STEPS: u64 = 600;

/// The pond's floor: drawn by a worker, then kept until it is a texture.
#[derive(Debug)]
enum Floor {
    Drawing(mpsc::Receiver<Vec<u8>>),
    Drawn(Rc<Vec<u8>>),
    /// A texture now (`Pond::floor_texture`).
    Made,
}

/// The koi's water as last drawn on its screen (#815): what it was drawn
/// from, the screen's scale and size, the commit it was drawn at and what
/// changed since the one before.
#[derive(Debug, Default)]
pub struct Water {
    pub heights: Vec<f32>,
    pub under: Mesh,
    pub at: Option<(u64, (i32, i32))>,
    pub since: Option<(CommitCounter, Vec<Rectangle<i32, Physical>>)>,
}

/// The pond on one screen: the pond itself, and what drawing it keeps.
#[derive(Debug)]
pub struct Pond {
    pub sim: sim::Pond,
    /// Its water, the still water standing for it behind the glass of the
    /// bar, and the blank standing for its pads there.
    pub ids: [Id; 3],
    commit: Cell<CommitCounter>,
    drawn: Cell<u64>,
    /// Its water as last drawn, to tell what of it changed (#815).
    pub water: RefCell<Water>,
    /// Its pads as last drawn.
    pub pads_drawn: RefCell<Traced>,
    floor: RefCell<Option<Floor>>,
    /// The floor's texture, and the still water's commit.
    pub floor_texture: RefCell<Option<Rc<dyn std::any::Any>>>,
    pub floor_commit: Cell<CommitCounter>,
    /// What is under the water, and its heights, kept between frames.
    pub under: RefCell<Option<Kept>>,
    pub heights: RefCell<Option<Heights>>,
    /// The generation the textures show.
    pub shown: Cell<Option<u64>>,
}

impl std::ops::Deref for Pond {
    type Target = sim::Pond;

    fn deref(&self) -> &sim::Pond {
        &self.sim
    }
}

impl std::ops::DerefMut for Pond {
    fn deref_mut(&mut self) -> &mut sim::Pond {
        &mut self.sim
    }
}

impl Pond {
    fn new(size: (f32, f32), zone: [f32; 4], rng: &mut Rng, seed: u64) -> Self {
        let sim = sim::Pond::new(size, zone, rng);
        let (tx, rx) = mpsc::channel();
        let (w, h) = (size.0.ceil() as u32, size.1.ceil() as u32);
        let spawned = std::thread::Builder::new()
            .name("koi floor".into())
            .spawn(move || {
                // The receiver gone (the pond with it): nothing to tell.
                let _sent = tx.send(floor(w, h, seed));
            });
        if let Err(err) = &spawned {
            tracing::warn!(%err, "no worker for the koi's floor: no pond");
        }
        Self {
            sim,
            ids: std::array::from_fn(|_| Id::new()),
            commit: Cell::new(CommitCounter::default()),
            drawn: Cell::new(u64::MAX),
            water: RefCell::default(),
            pads_drawn: RefCell::default(),
            floor: RefCell::new(spawned.ok().map(|_| Floor::Drawing(rx))),
            floor_texture: RefCell::new(None),
            floor_commit: Cell::new(CommitCounter::default()),
            under: RefCell::new(None),
            heights: RefCell::new(None),
            shown: Cell::new(None),
        }
    }

    /// Takes the floor the worker finished: whether it just came.
    fn poll_floor(&self) -> bool {
        let mut floor = self.floor.borrow_mut();
        let Some(Floor::Drawing(rx)) = floor.as_ref() else {
            return false;
        };
        match rx.try_recv() {
            Ok(pixels) => {
                *floor = Some(Floor::Drawn(Rc::new(pixels)));
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                tracing::warn!("the koi's floor was not drawn");
                *floor = None;
                false
            }
        }
    }

    /// Whether its floor is drawn.
    pub fn floor_ready(&self) -> bool {
        matches!(
            self.floor.borrow().as_ref(),
            Some(Floor::Drawn(_) | Floor::Made)
        )
    }

    /// The floor's pixels (premultiplied RGBA, its logical size), until
    /// they are a texture.
    pub fn floor_pixels(&self) -> Option<Rc<Vec<u8>>> {
        match self.floor.borrow().as_ref()? {
            Floor::Drawn(pixels) => Some(pixels.clone()),
            Floor::Drawing(_) | Floor::Made => None,
        }
    }

    /// Its floor is the texture `texture` now: its pixels go.
    pub fn floor_made(&self, texture: Rc<dyn std::any::Any>) {
        *self.floor_texture.borrow_mut() = Some(texture);
        *self.floor.borrow_mut() = Some(Floor::Made);
        let mut c = self.floor_commit.get();
        c.increment();
        self.floor_commit.set(c);
    }

    /// it was last drawn, with the one before then.
    pub fn commit(&self) -> (CommitCounter, Option<CommitCounter>) {
        let mut c = self.commit.get();
        if self.drawn.get() != self.sim.generation() {
            let prev = c;
            c.increment();
            self.commit.set(c);
            self.drawn.set(self.sim.generation());
            return (c, Some(prev));
        }
        (c, None)
    }
}

/// The pond on every screen it shows on.
#[derive(Debug)]
pub struct Ponds {
    start: Duration,
    steps: u64,
    rng: Rng,
    seed: u64,
    /// By screen name.
    pub panes: HashMap<String, Pond>,
    /// Where the pointer was last seen, and since when it is there.
    pointer: Option<((f64, f64), Duration)>,
    /// Clicks on the water since the last step: the screen, where (from
    /// its corner).
    clicks: Vec<(String, (f32, f32))>,
}

impl Ponds {
    pub fn new(start: Duration, seed: u64) -> Self {
        Self {
            start,
            steps: 0,
            rng: Rng::new(seed),
            seed,
            panes: HashMap::new(),
            pointer: None,
            clicks: Vec::new(),
        }
    }

    /// Screen `name` is `size` (logical), its work area `zone` (from its
    /// corner): its pond, dug when it is new or its size changed. Whether
    /// it was.
    pub fn pond(&mut self, name: &str, size: (f32, f32), zone: [f32; 4]) -> bool {
        if let Some(pond) = self.panes.get_mut(name)
            && pond.size == size
        {
            pond.zone = zone;
            return false;
        }
        self.seed = self.seed.wrapping_add(1);
        let pond = Pond::new(size, zone, &mut self.rng, self.seed);
        self.panes.insert(name.to_owned(), pond);
        true
    }

    /// Only the screens of `names` keep theirs.
    pub fn retain_panes(&mut self, names: &[&str]) {
        self.panes.retain(|n, _| names.contains(&n.as_str()));
    }

    /// Steps it to `now` on the screens of `on`, each with where the
    /// pointer rests on it (from its corner).
    pub fn advance(&mut self, now: Duration, on: &[(String, Option<(f32, f32)>)]) {
        for (name, (x, y)) in std::mem::take(&mut self.clicks) {
            if let Some(pond) = self.panes.get_mut(&name) {
                pond.clicked(x, y);
            }
        }
        let due = (now.saturating_sub(self.start).as_secs_f64() / f64::from(STEP)) as u64;
        let from = self.steps.max(due.saturating_sub(MOST_STEPS));
        for _ in from..due {
            for (name, resting) in on {
                if let Some(pond) = self.panes.get_mut(name) {
                    let pointer = Pointer { resting: *resting };
                    pond.step(STEP, pointer, &mut self.rng);
                }
            }
        }
        self.steps = self.steps.max(due);
    }
}

impl crate::State {
    /// How the koi's pond shows now (#803).
    pub(crate) fn koi_plays(&self) -> Option<super::Plays> {
        self.wallpaper_effect(self.config.effects.koi)
    }

    /// Whether the koi's pond is `output`'s wallpaper now: its floor
    /// drawn, no fullscreen window in front.
    pub(crate) fn koi_shows_on(&self, output: &smithay::output::Output) -> bool {
        self.koi_plays().is_some()
            && self
                .fx
                .koi
                .as_ref()
                .and_then(|k| k.panes.get(&output.name()))
                .is_some_and(Pond::floor_ready)
            && !self.wall_hidden(output)
    }

    /// A click at `at` (global, logical) on the wallpaper: on a pond, its
    /// water rings and its koi scatter.
    pub(crate) fn koi_click(&mut self, at: smithay::utils::Point<f64, smithay::utils::Logical>) {
        if self.koi_plays() != Some(super::Plays::Moving) {
            return;
        }
        let Some(output) = self.space.output_under(at).next().cloned() else {
            return;
        };
        if !self.koi_shows_on(&output) {
            return;
        }
        let Some(area) = self.space.output_geometry(&output) else {
            return;
        };
        if let Some(ponds) = self.fx.koi.as_mut() {
            let local = (
                (at.x - f64::from(area.loc.x)) as f32,
                (at.y - f64::from(area.loc.y)) as f32,
            );
            ponds.clicks.push((output.name(), local));
        }
    }

    /// The koi's pond moves on to `now` (#803): dug as it is turned on, on
    /// each screen; its koi drawn to where the pointer rests on the water;
    /// stepped on the screens that are on and show no fullscreen window.
    /// Whether it moves (a frame is wanted).
    pub(crate) fn tick_koi(&mut self, now: Duration) -> bool {
        let Some(plays) = self.koi_plays() else {
            if self.config.effects.koi && self.fx.koi.is_some() {
                // Waiting (the power saver, a share): kept, not drawn.
                return false;
            }
            if self.fx.koi.take().is_some() {
                tracing::debug!("the koi's pond goes");
                self.redraw = true;
            }
            return false;
        };
        if self.fx.koi.is_none() {
            self.fx.ponds += 1;
            tracing::debug!(?plays, "a koi pond");
            self.fx.koi = Some(Ponds::new(now, self.fx.ponds));
            self.redraw = true;
        }
        let pointer = self
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        let on_water = !self.locked() && self.surface_under(pointer).is_none();
        let pointer_screen = self.space.output_under(pointer).next().map(|o| o.name());
        let mut screens = Vec::new();
        for output in self.space.outputs() {
            let (Some(area), Some(zone)) =
                (self.space.output_geometry(output), self.work_area(output))
            else {
                continue;
            };
            let name = output.name();
            let on = self.output_powered(output) && !self.wall_unseen(output);
            let size = (area.size.w as f32, area.size.h as f32);
            let zone = [
                (zone.x - area.loc.x) as f32,
                (zone.y - area.loc.y) as f32,
                zone.width as f32,
                zone.height as f32,
            ];
            let corner = (f64::from(area.loc.x), f64::from(area.loc.y));
            screens.push((name, size, zone, on, corner));
        }
        let Some(ponds) = self.fx.koi.as_mut() else {
            return false;
        };
        let mut dug = false;
        for (name, size, zone, _, _) in &screens {
            dug |= ponds.pond(name, *size, *zone);
        }
        let names: Vec<&str> = screens.iter().map(|s| s.0.as_str()).collect();
        ponds.retain_panes(&names);
        // At rest: not moved more than a few pixels for a moment, over the
        // water.
        let at = (pointer.x, pointer.y);
        let since = match ponds.pointer {
            Some((last, since)) if (last.0 - at.0).hypot(last.1 - at.1) <= REST_MOVE => since,
            _ => now,
        };
        ponds.pointer = Some((at, since));
        let resting = on_water && now.saturating_sub(since) > REST_AFTER;
        // A floor drawn since: a frame to show it.
        let mut fresh = false;
        for pond in ponds.panes.values() {
            fresh |= pond.poll_floor();
        }
        let on: Vec<(String, Option<(f32, f32)>)> = screens
            .into_iter()
            .filter(|s| s.3)
            .map(|(name, _, _, _, corner)| {
                let rest = (resting && pointer_screen.as_deref() == Some(name.as_str()))
                    .then_some(((at.0 - corner.0) as f32, (at.1 - corner.1) as f32));
                (name, rest)
            })
            .collect();
        let moving = plays == super::Plays::Moving && !on.is_empty();
        // Waiting, its clock goes on all the same: back, it does not
        // catch up on what nobody saw, all at once (#815).
        ponds.advance(now, if moving { &on } else { &[] });
        if dug || fresh {
            self.redraw = true;
        }
        // While a floor is being drawn, frames come to look for it.
        let drawing = ponds
            .panes
            .values()
            .any(|p| matches!(p.floor.borrow().as_ref(), Some(Floor::Drawing(_))));
        moving || drawing
    }
}
