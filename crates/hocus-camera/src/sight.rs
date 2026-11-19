//! The face's and the hands' models on threads of their own (lot 4), so
//! that the frames' loop never waits for them: each takes the latest
//! frame when it is free (frames meanwhile are not copied), and says what
//! it saw, which the loop takes at its next frame. Their models load as
//! the thread starts; the threads end when the loop lets them go (no
//! effect needs them, or the camera is no longer read).
//!
//! - **The face** ([`Faces`]): every frame it is free for.
//! - **The hands** ([`Hands`]): ten times a second at most; the palm's
//!   detector three times a second while no hand is followed, once a
//!   second while one is (a second hand coming).

use std::{
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    time::{Duration, Instant},
};

use hocus_link::{CameraLook, camera::Wear};

use crate::{
    face::{Face, FaceTracker, Pose},
    framing::Framing,
    hands::{self, Gesture, Hand, HandTracker},
    look::{BYTES, H, W},
    panda::{self, Drive, Light, Sim},
    react::{self, Confetti, Gestures, Label, POP},
    segment::Mask,
    track::Frame,
    wear,
};

/// A model's thread ended: its models do not load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ended;

/// The hands a thread saw, and the gesture each makes.
pub type SeenHands = Vec<(Hand, Option<Gesture>)>;

/// A frame for a thread: its pixels (RGBA), when it came.
struct Job {
    pixels: Vec<u8>,
    at: Instant,
    /// The face's expressions asked too (the panda).
    more: bool,
}

/// What a thread saw, and the frame's buffer back.
struct Seen<T> {
    what: T,
    at: Instant,
    pixels: Vec<u8>,
}

/// A thread running a model on the frames it is given.
struct Worker<T> {
    jobs: SyncSender<Job>,
    seen: Receiver<Seen<T>>,
    /// A buffer to copy the next frame into, while the thread is free.
    spare: Option<Vec<u8>>,
    busy: bool,
}

impl<T: Send + 'static> Worker<T> {
    fn start<S: 'static>(
        name: &str,
        state: impl FnOnce() -> Option<S> + Send + 'static,
        run: fn(&mut S, &[u8], Instant, bool) -> T,
    ) -> Option<Self> {
        let (jobs, jobs_in) = mpsc::sync_channel::<Job>(1);
        let (seen_out, seen) = mpsc::channel::<Seen<T>>();
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let Some(mut state) = state() else {
                    return;
                };
                for job in jobs_in {
                    let what = run(&mut state, &job.pixels, job.at, job.more);
                    if seen_out
                        .send(Seen {
                            what,
                            at: job.at,
                            pixels: job.pixels,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|err| tracing::warn!(%err, name, "no thread for a model"))
            .ok()?;
        Some(Self {
            jobs,
            seen,
            spare: Some(vec![0; BYTES]),
            busy: false,
        })
    }

    /// `frame` (RGBA) given to the thread if it is free.
    fn offer(&mut self, frame: &[u8], at: Instant, more: bool) -> bool {
        if self.busy {
            return false;
        }
        let Some(mut pixels) = self.spare.take() else {
            return false;
        };
        pixels.clear();
        pixels.extend_from_slice(frame);
        match self.jobs.try_send(Job { pixels, at, more }) {
            Ok(()) => {
                self.busy = true;
                true
            }
            Err(mpsc::TrySendError::Full(job) | mpsc::TrySendError::Disconnected(job)) => {
                self.spare = Some(job.pixels);
                false
            }
        }
    }

    /// What the thread saw last, if it said something new; `Err` when it
    /// ended (its models do not load).
    fn take(&mut self) -> Result<Option<(T, Instant)>, Ended> {
        let mut latest = None;
        loop {
            match self.seen.try_recv() {
                Ok(seen) => {
                    self.busy = false;
                    self.spare = Some(seen.pixels);
                    latest = Some((seen.what, seen.at));
                }
                Err(TryRecvError::Empty) => return Ok(latest),
                Err(TryRecvError::Disconnected) => {
                    return if latest.is_some() {
                        Ok(latest)
                    } else {
                        Err(Ended)
                    };
                }
            }
        }
    }
}

fn frame(pixels: &[u8]) -> Frame<'_> {
    Frame {
        pixels,
        width: W,
        height: H,
        order: [0, 1, 2],
    }
}

/// The face's thread.
pub struct Faces {
    worker: Worker<Option<Face>>,
}

impl std::fmt::Debug for Faces {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Faces").finish_non_exhaustive()
    }
}

struct FaceState {
    tracker: FaceTracker,
    last: Option<Instant>,
}

impl Faces {
    pub fn start() -> Option<Self> {
        let worker = Worker::start(
            "the face",
            || match FaceTracker::new() {
                Ok(tracker) => Some(FaceState {
                    tracker,
                    last: None,
                }),
                Err(err) => {
                    tracing::warn!(%err, "the face's models do not load");
                    None
                }
            },
            |state: &mut FaceState, pixels, at, expressions| {
                let dt = state
                    .last
                    .map_or(1.0 / 30.0, |l| at.duration_since(l).as_secs_f32());
                state.last = Some(at);
                match state.tracker.track(&frame(pixels), dt, expressions) {
                    Ok(face) => face,
                    Err(err) => {
                        tracing::warn!(%err, "the face's models failed");
                        None
                    }
                }
            },
        )?;
        Some(Self { worker })
    }

    /// The frame (RGBA) looked at, if the thread is free; its
    /// expressions too when `expressions`.
    pub fn offer(&mut self, frame: &[u8], at: Instant, expressions: bool) -> bool {
        self.worker.offer(frame, at, expressions)
    }

    /// The face it saw last (`Some(None)`: none in that frame), if new;
    /// `Err` once the thread ended.
    pub fn take(&mut self) -> Result<Option<(Option<Face>, Instant)>, Ended> {
        self.worker.take()
    }
}

/// The hands' thread.
pub struct Hands {
    worker: Worker<SeenHands>,
    offered: Option<Instant>,
}

impl std::fmt::Debug for Hands {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hands").finish_non_exhaustive()
    }
}

/// The hands looked at at most this often.
pub const HANDS_EVERY: Duration = Duration::from_millis(100);
/// The palm's detector, while no hand is followed, and while one is.
const DETECT_ALONE: Duration = Duration::from_millis(330);
const DETECT_MORE: Duration = Duration::from_secs(1);

struct HandState {
    tracker: HandTracker,
    detected: Option<Instant>,
}

impl Hands {
    pub fn start() -> Option<Self> {
        let worker = Worker::start(
            "the hands",
            || match HandTracker::new() {
                Ok(tracker) => Some(HandState {
                    tracker,
                    detected: None,
                }),
                Err(err) => {
                    tracing::warn!(%err, "the hands' models do not load");
                    None
                }
            },
            |state: &mut HandState, pixels, at, _| {
                let every = if state.tracker.following() {
                    DETECT_MORE
                } else {
                    DETECT_ALONE
                };
                let detect = state.detected.is_none_or(|d| at.duration_since(d) >= every);
                if detect {
                    state.detected = Some(at);
                }
                match state.tracker.track(&frame(pixels), detect) {
                    Ok(found) => found
                        .into_iter()
                        .map(|hand| {
                            let g = hands::gesture(&hand, H as f32);
                            (hand, g)
                        })
                        .collect(),
                    Err(err) => {
                        tracing::warn!(%err, "the hands' models failed");
                        Vec::new()
                    }
                }
            },
        )?;
        Some(Self {
            worker,
            offered: None,
        })
    }

    /// The frame (RGBA) looked at, if it is time and the thread is free.
    pub fn offer(&mut self, frame: &[u8], at: Instant) -> bool {
        if self
            .offered
            .is_some_and(|o| at.duration_since(o) < HANDS_EVERY)
        {
            return false;
        }
        let offered = self.worker.offer(frame, at, false);
        if offered {
            self.offered = Some(at);
        }
        offered
    }

    /// The hands it saw last and their gestures, if new; `Err` once the
    /// thread ended.
    pub fn take(&mut self) -> Result<Option<(SeenHands, Instant)>, Ended> {
        self.worker.take()
    }
}

/// The hair's top above the face at `pose` (head-local y), from the
/// person's `mask`: up from the forehead's top along the face's axis to
/// where the person ends.
pub fn hair_top(pose: &Pose, mask: &Mask) -> f32 {
    let (sr, cr) = pose.roll.sin_cos();
    let (ux, uy) = (sr, -cr);
    let (fx, fy) = pose.forehead;
    let mut d = 0.0;
    while d < 3.0 * pose.width {
        let (x, y) = (fx + ux * d, fy + uy * d);
        if y < 0.0 || mask.at(x / W as f32, y / H as f32) < 0.5 {
            break;
        }
        d += 2.0;
    }
    // A head's at least; above the frame's top, the hair goes on.
    let above = (d / pose.unit.max(1e-3)).max(10.0);
    pose.forehead_at.0 - above
}

/// A face last seen this long ago is no longer drawn on.
pub const FACE_STALE: Duration = Duration::from_millis(500);

/// What lot 4 needs of the person and draws: the threads, the face's
/// pose, Cadrage, the gestures and their reactions, the confetti.
#[derive(Debug, Default)]
pub struct Sight {
    faces: Option<Faces>,
    hands: Option<Hands>,
    /// The face's or the hands' models did not load: not tried again.
    faces_failed: bool,
    hands_failed: bool,
    /// The face's pose last seen, and when; its expressions, when asked.
    pose: Option<(Pose, Instant)>,
    shapes: Option<[f32; 52]>,
    /// The hair's top (head-local y), from the person's mask, kept
    /// smooth.
    hair: Option<f32>,
    /// The panda, while it takes the person's place.
    panda: Option<Sim>,
    framing: Framing,
    gestures: Gestures,
    /// When the last thumb's reaction was sent.
    thumb: Option<Instant>,
    label: Option<Label>,
    confetti: Confetti,
    /// The moments the compositor counted when this program heard first.
    moments: Option<u64>,
    frames: u64,
    last: Option<Instant>,
    panda_last: Option<Instant>,
}

impl Sight {
    /// What the face's and hands' models are wanted for, by `look`.
    fn wants(look: &CameraLook) -> (bool, bool) {
        (look.face.is_some() || look.framing, look.gestures)
    }

    /// A new look: the moments it counts (a new one, confetti, with Fêtes
    /// and the camera `read`).
    pub fn told(&mut self, look: &CameraLook, read: bool, now: Instant) {
        let before = self.moments.replace(look.moments);
        if let Some(before) = before
            && look.moments > before
            && look.celebrations
            && read
        {
            tracing::info!(moments = look.moments, "Fêtes: confetti in the picture");
            self.confetti.burst(look.moments, now);
        }
        if !look.gestures {
            self.gestures.reset();
            self.thumb = None;
        }
        if !look.celebrations {
            self.confetti = Confetti::default();
        }
    }

    /// The frame `rgba` at `now`: given to the models that are wanted
    /// and free, and what they saw taken.
    pub fn see(&mut self, look: &CameraLook, rgba: &[u8], now: Instant) {
        self.frames += 1;
        let (face, hands) = Self::wants(look);
        if !face {
            self.faces = None;
            self.pose = None;
        } else if self.faces.is_none() && !self.faces_failed {
            tracing::info!("the face's models start");
            self.faces = Faces::start();
            self.faces_failed = self.faces.is_none();
        }
        if !hands {
            self.hands = None;
        } else if self.hands.is_none() && !self.hands_failed {
            tracing::info!("the hands' models start");
            self.hands = Hands::start();
            self.hands_failed = self.hands.is_none();
        }
        if let Some(faces) = &mut self.faces {
            match faces.take() {
                Ok(Some((seen, at))) => match seen {
                    Some(face) => {
                        self.pose = Some((face.pose(), at));
                        self.shapes = face.expressions.as_deref().and_then(|e| e.try_into().ok());
                    }
                    None => {
                        if self
                            .pose
                            .is_some_and(|(_, at)| now.duration_since(at) > FACE_STALE)
                        {
                            self.pose = None;
                        }
                    }
                },
                Ok(None) => {}
                Err(Ended) => {
                    tracing::warn!("the face's thread ended");
                    self.faces = None;
                    self.faces_failed = true;
                }
            }
            // An accessory follows the head every frame; Cadrage alone,
            // which moves slowly, every other one.
            let panda = look.face == Some(Wear::Panda);
            if let Some(faces) = &mut self.faces
                && (look.face.is_some() || self.frames.is_multiple_of(2))
            {
                faces.offer(rgba, now, panda);
            }
        }
        if let Some(h) = &mut self.hands {
            match h.take() {
                Ok(Some((seen, at))) => {
                    let gestures: Vec<Gesture> = seen.iter().filter_map(|(_, g)| *g).collect();
                    if self.gestures.seen(&gestures, at) {
                        tracing::info!("Gestes: a thumb up");
                        self.thumb = Some(now);
                    }
                }
                Ok(None) => {}
                Err(Ended) => {
                    tracing::warn!("the hands' thread ended");
                    self.hands = None;
                    self.hands_failed = true;
                }
            }
            if let Some(h) = &mut self.hands {
                h.offer(rgba, now);
            }
        }
    }

    /// The face's pose to draw on at `now`, if seen lately.
    pub fn pose(&self, now: Instant) -> Option<Pose> {
        self.pose
            .filter(|(_, at)| now.duration_since(*at) <= FACE_STALE)
            .map(|(p, _)| p)
    }

    /// The hair's top above the face's forehead, from the person's
    /// `mask` (lot 3's): along the face's axis, up from the forehead's
    /// top to where the person ends; kept smooth.
    pub fn hair(&mut self, mask: &Mask) {
        let Some((pose, _)) = self.pose else {
            return;
        };
        let top = hair_top(&pose, mask);
        self.hair = Some(match self.hair {
            Some(h) => h + (top - h) * 0.25,
            None => top,
        });
    }

    /// Whether the hair's top is wanted (an accessory on it).
    pub fn wants_hair(look: &CameraLook) -> bool {
        matches!(look.face, Some(Wear::Ears | Wear::Beanie | Wear::Crown))
    }

    /// The panda in the person's place, over `made` (what is behind them
    /// drawn already), lit as `light`: its springs moved on to `now`.
    pub fn panda(&mut self, made: &mut [u8], look: &CameraLook, light: &Light, now: Instant) {
        let dt = self
            .panda_last
            .map_or(1.0 / 30.0, |l| now.duration_since(l).as_secs_f32());
        self.panda_last = Some(now);
        let drive = match (self.pose(now), self.shapes) {
            (Some(pose), Some(shapes)) => Some(Drive { pose, shapes }),
            _ => None,
        };
        let sim = self.panda.get_or_insert_with(Sim::default);
        sim.advance(dt, drive.as_ref());
        panda::draw(made, sim, look.panda_look, light);
    }

    /// Whether lot 4 draws in this frame (the banner then goes on last).
    pub fn draws(&self, look: &CameraLook) -> bool {
        look.face.is_some()
            || look.framing
            || self.framing.now.zoom > 1.0
            || self.gestures.raised
            || self.thumb.is_some()
            || self.confetti.falling()
    }

    /// On the person (after what is behind them): Visage's accessory
    /// (the panda is drawn apart, [`Self::panda`]).
    pub fn on_person(&self, made: &mut [u8], look: &CameraLook, now: Instant) {
        if let (Some(wear), Some(pose)) = (look.face, self.pose(now))
            && wear != Wear::Panda
        {
            wear::draw(made, wear, &pose, self.hair, 1.0);
        }
    }

    /// Over the picture: Cadrage's crop, then the reactions and the
    /// confetti.
    pub fn over(&mut self, made: &mut [u8], look: &CameraLook, now: Instant) {
        let dt = self
            .last
            .map_or(Duration::from_millis(33), |l| now.duration_since(l));
        self.last = Some(now);
        if look.framing && look.face != Some(Wear::Panda) {
            let pose = self.pose(now);
            self.framing.update(pose.as_ref(), dt);
        } else {
            self.framing.reset();
        }
        self.framing.apply(made);
        if let Some(sent) = self.thumb {
            let since = now.duration_since(sent);
            if since < POP {
                react::thumb(made, since);
            } else {
                self.thumb = None;
            }
        }
        if self.gestures.raised {
            let lang = hocus_words::Lang::of_code(&look.lang).unwrap_or(hocus_words::Lang::English);
            let said = hocus_words::camera::HAND_RAISED.get(lang);
            if self.label.as_ref().is_none_or(|l| l.said != said) {
                self.label = Some(Label::new(said));
            }
            if let Some(label) = &self.label {
                label.over(made);
            }
        }
        self.confetti.over(made, now);
    }

    /// The camera let go: the threads end, nothing is kept.
    pub fn reset(&mut self) {
        let moments = self.moments;
        *self = Self {
            moments,
            ..Self::default()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_link::camera::Wear;

    fn bgrx(rgba: &[u8]) -> Vec<u8> {
        rgba.as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, _]| [b, g, r, 255])
            .collect()
    }

    /// The pull request's pictures of lot 4, the camera's frames made
    /// from the test pictures: each accessory on the portrait, Cadrage
    /// (a person further away, before and after), a thumb's reaction,
    /// « main levée », the confetti. Written to `$HOCUS_SHOTS` when it names a folder.
    /// Run by hand (`--run-ignored only`).
    #[test]
    #[ignore = "writes the pull request's screenshots"]
    fn screenshots_of_the_camera_s_pictures_lot_4() -> anyhow::Result<()> {
        let Ok(dir) = std::env::var("HOCUS_SHOTS") else {
            return Ok(());
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir)?;
        let save = |name: &str, bgrx: &[u8]| -> anyhow::Result<()> {
            let rgba: Vec<u8> = bgrx
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|&[b, g, r, _]| [r, g, b, 255])
                .collect();
            image::save_buffer(
                dir.join(name),
                &rgba,
                W as u32,
                H as u32,
                image::ColorType::Rgba8,
            )?;
            Ok(())
        };
        let person = crate::face::test_face();
        let mut tracker = FaceTracker::new()?;
        let pose = tracker
            .track(&frame(&person), 1.0 / 30.0, false)?
            .expect("a face")
            .pose();
        let mut mask = Mask::default();
        crate::segment::Segmenter::new()?.run(&person, W, H, [0, 1, 2], &mut mask)?;
        let top = hair_top(&pose, &mask);
        for (wear, name) in [
            (Wear::Ears, "camera-oreilles.png"),
            (Wear::Beanie, "camera-bonnet.png"),
            (Wear::Crown, "camera-couronne.png"),
            (Wear::Glasses, "camera-lunettes.png"),
        ] {
            let mut made = bgrx(&person);
            wear::draw(&mut made, wear, &pose, Some(top), 1.0);
            save(name, &made)?;
        }
        // Heads turned: the accessories turn with them.
        for (picture, wear, name) in [
            (
                crate::hands::test_raised_hand(),
                Wear::Beanie,
                "camera-bonnet-tourne.png",
            ),
            (
                crate::hands::test_raised_hand(),
                Wear::Glasses,
                "camera-lunettes-tourne.png",
            ),
            (
                crate::hands::test_thumb_up(),
                Wear::Crown,
                "camera-couronne-wakata.png",
            ),
            (
                crate::hands::test_thumb_up(),
                Wear::Ears,
                "camera-oreilles-wakata.png",
            ),
        ] {
            let mut tracker = FaceTracker::new()?;
            let pose = tracker
                .track(&frame(&picture), 1.0 / 30.0, false)?
                .expect("a face")
                .pose();
            let mut mask = Mask::default();
            crate::segment::Segmenter::new()?.run(&picture, W, H, [0, 1, 2], &mut mask)?;
            let mut made = bgrx(&picture);
            wear::draw(&mut made, wear, &pose, Some(hair_top(&pose, &mask)), 1.0);
            save(name, &made)?;
        }
        // Cadrage: a person further away, off the middle, before and
        // after.
        let far = crate::track::test_picture(include_bytes!("../data/far.jpg"));
        save("camera-sans-cadrage.png", &bgrx(&far))?;
        let mut tracker = FaceTracker::new()?;
        let pose = tracker
            .track(&frame(&far), 1.0 / 30.0, false)?
            .expect("a face")
            .pose();
        let mut framing = Framing::default();
        for _ in 0..90 {
            framing.update(Some(&pose), Duration::from_millis(33));
        }
        let mut made = bgrx(&far);
        framing.apply(&mut made);
        save("camera-cadrage.png", &made)?;
        // A thumb up, its reaction at the top of its rise.
        let mut made = bgrx(&crate::hands::test_thumb_up());
        react::thumb(&mut made, Duration::from_millis(500));
        save("camera-pouce-leve.png", &made)?;
        let mut made = bgrx(&crate::hands::test_raised_hand());
        Label::new(hocus_words::camera::HAND_RAISED.get(hocus_words::Lang::French)).over(&mut made);
        save("camera-main-levee.png", &made)?;
        let mut made = bgrx(&person);
        let mut confetti = Confetti::default();
        let t0 = Instant::now();
        confetti.burst(7, t0);
        confetti.over(&mut made, t0 + Duration::from_millis(1700));
        save("camera-fetes.png", &made)?;
        Ok(())
    }
}
