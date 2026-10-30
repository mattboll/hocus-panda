//! The halo (docs/design/bar.md §6, 4.4): the whole bar lights up when
//! something concerns the user, one state at a time, by priority. A wave
//! runs down the bar once, then a glow stays: a border in its colour, a
//! glow outside the bar and one inside it. It ends as soon as the user
//! acts: goes to the agent, answers it, sees the finished one.
//!
//! The compositor draws it, over the bar's surfaces as the bar's glass is
//! under them: two GLES pixel shaders of the closed-form CSS the mockup
//! uses (its `box-shadow`s and its wave's `linear-gradient`), evaluated on
//! the frame clock. The shell's surfaces never draw for it. The glow's
//! element is kept while it does not change, so that at rest it leaves no
//! damage and costs no frame; the wave is a new element each frame while
//! it runs, as the streak is ([`crate::going`]). A Pixman compositor has
//! no shaders, and no halo.
//!
//! Its states come from the agents' model (4.0, 4.1) and, with the
//! calendar later, from the meetings ([`State::meeting_near`]). Each agent
//! that concerns the user is an episode, from its state's start: going to
//! it, or its window getting the keyboard, acknowledges it, and the halo
//! no longer counts it. The reminder of a waiting agent is a timer of the
//! loop at its fifth minute, never polled.

use std::{cell::RefCell, collections::BTreeMap, collections::HashMap, time::Duration};

use smithay::{
    backend::renderer::{
        element::Kind,
        gles::element::PixelShaderElement,
        gles::{GlesPixelProgram, GlesRenderer, Uniform, UniformName, UniformType},
    },
    output::Output,
    reexports::calloop::{
        RegistrationToken,
        timer::{TimeoutAction, Timer},
    },
    utils::{Logical, Rectangle},
};

use crate::{
    State,
    going::{ShaderElement, Shaders},
    motion::{Bezier, Tween},
};

/// The wave: a band 40 % of the bar's height runs from above its top to
/// below its bottom once, in this time, along this curve.
pub const WAVE: Duration = Duration::from_millis(600);
pub const WAVE_CURVE: Bezier = Bezier(0.3, 0.6, 0.3, 1.0);
pub const WAVE_BAND: f64 = 0.4;
/// The glow fades in, and out, in these times (CSS's `ease-out` in,
/// `ease-in` out); with reduced motion, both in this one, `ease-out`.
pub const FADE_IN: Duration = Duration::from_millis(600);
pub const FADE_OUT: Duration = Duration::from_millis(500);
pub const FADE_REDUCED: Duration = Duration::from_millis(200);
/// An agent still waiting after this long gets one more wave.
pub const REMINDER: Duration = Duration::from_secs(5 * 60);

/// What concerns the user, the most important first: their order is the
/// halo's priority (bar.md §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Concern {
    /// A meeting in 2 min or less.
    MeetingNow,
    /// An agent waits for the user.
    Waiting,
    /// A meeting in 10 min or less.
    MeetingSoon,
    /// An agent is stuck (failed).
    Stuck,
    /// A finished agent not seen yet (variant B).
    ToSee,
}

impl Concern {
    /// The agent's state as the shell is told it (`hocus-link`'s words).
    pub fn of_agent(state: &str) -> Option<Self> {
        match state {
            "waiting" => Some(Self::Waiting),
            "failed" => Some(Self::Stuck),
            "done" => Some(Self::ToSee),
            _ => None,
        }
    }

    pub fn colour(self) -> [u8; 3] {
        match self {
            Self::MeetingNow | Self::MeetingSoon => [0x8F, 0xB3, 0xFF],
            Self::Waiting => [0xF5, 0xB4, 0x4A],
            Self::Stuck => [0xF2, 0x94, 0x5B],
            Self::ToSee => [0x6C, 0xC2, 0x87],
        }
    }

    /// The glow it stays as.
    pub fn glow(self) -> Glow {
        let colour = self.colour().map(|c| f32::from(c) / 255.0);
        let (border, outer, inner) = match self {
            Self::MeetingNow => (0.85, (44.0, 0.60), (22.0, 0.18)),
            Self::Waiting | Self::MeetingSoon | Self::Stuck => (0.85, (34.0, 0.45), (22.0, 0.18)),
            Self::ToSee => (0.45, (20.0, 0.25), (14.0, 0.10)),
        };
        Glow {
            colour,
            border,
            outer: Shadow {
                blur: outer.0,
                alpha: outer.1,
            },
            inner: Shadow {
                blur: inner.0,
                alpha: inner.1,
            },
        }
    }

    /// Its wave's alpha; `None`: it has none.
    pub fn wave(self) -> Option<f32> {
        match self {
            Self::MeetingNow => Some(0.42),
            Self::Waiting => Some(0.38),
            Self::MeetingSoon => Some(0.22),
            Self::Stuck | Self::ToSee => None,
        }
    }
}

/// How near the next meeting is: the calendar's way into the halo, with
/// "Your day" (not built yet).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Meeting {
    /// In 10 min or less.
    InTen,
    /// In 2 min or less.
    InTwo,
}

impl Meeting {
    fn concern(self) -> Concern {
        match self {
            Self::InTen => Concern::MeetingSoon,
            Self::InTwo => Concern::MeetingNow,
        }
    }
}

/// A CSS shadow of the glow: its blur (twice the Gaussian's deviation),
/// logical pixels, and its colour's alpha.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Shadow {
    pub blur: f32,
    pub alpha: f32,
}

/// The glow as drawn: its colour (0 to 1), the bar's 1 px border in it,
/// and its glows outside and inside the bar.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Glow {
    pub colour: [f32; 3],
    pub border: f32,
    pub outer: Shadow,
    pub inner: Shadow,
}

impl Glow {
    pub fn shows(&self) -> bool {
        self.border > 0.0 || self.outer.alpha > 0.0 || self.inner.alpha > 0.0
    }

    /// Its colour, 0 to 255.
    pub fn rgb(&self) -> [u8; 3] {
        self.colour.map(|c| (c * 255.0).round() as u8)
    }

    /// Gone: its colour kept, nothing of it shown. CSS's transition to and
    /// from `0 0 0 transparent` shrinks the blur with the alpha.
    fn faded(self) -> Self {
        Self {
            colour: self.colour,
            ..Self::default()
        }
    }

    fn lerp(a: Self, b: Self, t: f64) -> Self {
        let t = t as f32;
        let mix = |x: f32, y: f32| x + (y - x) * t;
        let shadow = |x: Shadow, y: Shadow| Shadow {
            blur: mix(x.blur, y.blur).max(0.0),
            alpha: mix(x.alpha, y.alpha).clamp(0.0, 1.0),
        };
        Self {
            colour: [0, 1, 2].map(|i| mix(a.colour[i], b.colour[i]).clamp(0.0, 1.0)),
            border: mix(a.border, b.border).clamp(0.0, 1.0),
            outer: shadow(a.outer, b.outer),
            inner: shadow(a.inner, b.inner),
        }
    }
}

/// The wave at a moment: its band's top, as a part of the bar's height
/// inside its border (-0.4 above it to 1 below it), its colour and alpha.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaveAt {
    pub top: f64,
    pub alpha: f32,
    pub colour: [u8; 3],
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Wave {
    start: Duration,
    colour: [u8; 3],
    alpha: f32,
}

impl Wave {
    fn done(&self, now: Duration) -> bool {
        now >= self.start + WAVE
    }

    fn at(&self, now: Duration) -> Option<WaveAt> {
        if now < self.start || self.done(now) {
            return None;
        }
        let p = Tween::new(self.start, WAVE, WAVE_CURVE).at(now);
        Some(WaveAt {
            top: crate::motion::lerp(-WAVE_BAND, 1.0, p),
            alpha: self.alpha,
            colour: self.colour,
        })
    }
}

/// What the halo shows at a moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    /// What it shows, or fades out from (`None`).
    pub concern: Option<Concern>,
    pub glow: Glow,
    pub wave: Option<WaveAt>,
}

/// How the halo moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// Its waves, its fades.
    Full,
    /// Reduced motion: no wave, short fades.
    Reduced,
    /// Not drawn (the CPU, or no bar): it changes at once, with no wave.
    Still,
}

impl Motion {
    fn fade_in(self) -> (Duration, Bezier) {
        match self {
            Self::Full => (FADE_IN, Bezier::EASE_OUT),
            Self::Reduced => (FADE_REDUCED, Bezier::EASE_OUT),
            Self::Still => (Duration::ZERO, Bezier::LINEAR),
        }
    }

    fn fade_out(self) -> (Duration, Bezier) {
        match self {
            Self::Full => (FADE_OUT, Bezier::EASE_IN),
            Self::Reduced => (FADE_REDUCED, Bezier::EASE_OUT),
            Self::Still => (Duration::ZERO, Bezier::LINEAR),
        }
    }
}

/// An agent as the halo sees it: what of it concerns the user, since when
/// (its state's start, on the frame clock), and whether the user looks at
/// it (its own window has the keyboard).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seen {
    pub id: u64,
    pub concern: Option<Concern>,
    pub since: Duration,
    pub looked_at: bool,
}

/// An agent that concerns the user, from its state's start.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Episode {
    concern: Concern,
    since: Duration,
    /// The user went to it, or looked at it: it no longer counts.
    acknowledged: bool,
    /// Its reminder's wave ran.
    reminded: bool,
}

/// The glow going from what it was to what it becomes.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Change {
    from: Glow,
    to: Glow,
    tween: Tween,
}

impl Default for Change {
    fn default() -> Self {
        Self {
            from: Glow::default(),
            to: Glow::default(),
            tween: Tween::new(Duration::ZERO, Duration::ZERO, Bezier::LINEAR),
        }
    }
}

/// The halo's state, on the loop.
#[derive(Debug, Default)]
pub struct Halo {
    episodes: BTreeMap<u64, Episode>,
    meeting: Option<Concern>,
    /// What it shows (or `None`, fading out).
    shown: Option<Concern>,
    change: Change,
    wave: Option<Wave>,
    /// Waves started so far, for tests.
    pub waves: u64,
    /// The next reminder's timer, and when it fires.
    timer: Option<(Duration, RegistrationToken)>,
    /// Each screen's glow as last drawn, kept while it does not change.
    kept: RefCell<HashMap<String, (Key, ShaderElement)>>,
    /// What just began, for the peek (4.5): an agent that starts waiting
    /// or finishes, unless the user looks at it, and a waiting agent's
    /// reminder; taken by [`Halo::take_news`].
    news: Vec<(u64, Concern)>,
}

impl Halo {
    /// What concerns the user most, if anything.
    pub fn concern(&self) -> Option<Concern> {
        self.episodes
            .values()
            .filter(|e| !e.acknowledged)
            .map(|e| e.concern)
            .chain(self.meeting)
            .min()
    }

    /// The agents as they are now. An agent that starts waiting, unless
    /// the user looks at it, runs a wave. Whether what shows changed.
    pub fn agents(&mut self, agents: &[Seen], now: Duration, motion: Motion) -> bool {
        let before = std::mem::take(&mut self.episodes);
        let mut wave = None;
        for agent in agents {
            let Some(concern) = agent.concern else {
                continue;
            };
            let episode = match before.get(&agent.id) {
                Some(e) if e.concern == concern => Episode {
                    since: agent.since,
                    acknowledged: e.acknowledged || agent.looked_at,
                    ..*e
                },
                _ => {
                    if concern == Concern::Waiting && !agent.looked_at {
                        wave = Some(concern);
                    }
                    if matches!(concern, Concern::Waiting | Concern::ToSee) && !agent.looked_at {
                        self.news.push((agent.id, concern));
                    }
                    Episode {
                        concern,
                        since: agent.since,
                        acknowledged: agent.looked_at,
                        reminded: false,
                    }
                }
            };
            self.episodes.insert(agent.id, episode);
        }
        self.show(now, motion, wave)
    }

    /// What began since last taken, in order: the agents that started
    /// waiting or finished, and the reminders.
    pub fn take_news(&mut self) -> Vec<(u64, Concern)> {
        std::mem::take(&mut self.news)
    }

    /// Whether agent `id` still concerns the user as `concern`: in it, and
    /// not acted on.
    pub fn counts(&self, id: u64, concern: Concern) -> bool {
        self.episodes
            .get(&id)
            .is_some_and(|e| e.concern == concern && !e.acknowledged)
    }

    /// The user went to agent `id`: it no longer counts.
    pub fn acknowledge(&mut self, id: u64) {
        if let Some(episode) = self.episodes.get_mut(&id) {
            episode.acknowledged = true;
        }
    }

    /// The next meeting is `near`, or not. A meeting that comes nearer runs
    /// its wave. Whether what shows changed.
    pub fn meeting(&mut self, near: Option<Meeting>, now: Duration, motion: Motion) -> bool {
        let concern = near.map(Meeting::concern);
        if concern == self.meeting {
            return false;
        }
        self.meeting = concern;
        self.show(now, motion, concern)
    }

    /// When the next reminder is due: the fifth minute of an agent waiting,
    /// not reminded yet.
    pub fn next_reminder(&self) -> Option<Duration> {
        self.episodes
            .values()
            .filter(|e| e.concern == Concern::Waiting && !e.acknowledged && !e.reminded)
            .map(|e| e.since + REMINDER)
            .min()
    }

    /// The reminders due by `now` run their wave, once. Whether what shows
    /// changed.
    pub fn remind(&mut self, now: Duration, motion: Motion) -> bool {
        // A timer may fire a hair early.
        let by = now + Duration::from_millis(1);
        let mut due = false;
        for (id, episode) in &mut self.episodes {
            if episode.concern == Concern::Waiting
                && !episode.acknowledged
                && !episode.reminded
                && episode.since + REMINDER <= by
            {
                episode.reminded = true;
                due = true;
                self.news.push((*id, Concern::Waiting));
            }
        }
        due && self.show(now, motion, Some(Concern::Waiting))
    }

    /// Shows what concerns the user most: the glow goes to it from where it
    /// is; `wave`: what just happened runs its wave, when it is what shows.
    fn show(&mut self, now: Duration, motion: Motion, wave: Option<Concern>) -> bool {
        let target = self.concern();
        let mut changed = false;
        if target != self.shown {
            let current = self.glow_at(now);
            let (to, (duration, curve)) = match target {
                Some(concern) => (concern.glow(), motion.fade_in()),
                None => (current.faded(), motion.fade_out()),
            };
            let from = if current.shows() { current } else { to.faded() };
            self.change = Change {
                from,
                to,
                tween: Tween::new(now, duration, curve),
            };
            self.shown = target;
            changed = true;
        }
        if let Some(concern) = wave
            && target == Some(concern)
            && motion == Motion::Full
            && let Some(alpha) = concern.wave()
        {
            self.wave = Some(Wave {
                start: now,
                colour: concern.colour(),
                alpha,
            });
            self.waves += 1;
            changed = true;
        }
        changed
    }

    fn glow_at(&self, now: Duration) -> Glow {
        let Change { from, to, tween } = self.change;
        Glow::lerp(from, to, tween.at(now))
    }

    /// What it shows at `now`; `None`: nothing.
    pub fn at(&self, now: Duration) -> Option<Shown> {
        let glow = self.glow_at(now);
        let wave = self.wave.and_then(|w| w.at(now));
        (glow.shows() || wave.is_some()).then_some(Shown {
            concern: self.shown,
            glow,
            wave,
        })
    }

    /// Whether it moves at `now`: a fade, a wave.
    pub fn moving(&self, now: Duration) -> bool {
        !self.change.tween.done(now) || self.wave.is_some_and(|w| !w.done(now))
    }

    /// A wave run out goes.
    pub fn tick(&mut self, now: Duration) {
        if self.wave.is_some_and(|w| w.done(now)) {
            self.wave = None;
        }
    }
}

impl State {
    /// How the halo moves here: drawn by a shader on GLES under a shell's
    /// bar, with or without motion; not drawn otherwise.
    pub fn halo_motion(&self) -> Motion {
        if !self.blur || !self.shell_linked() {
            Motion::Still
        } else if self.reduced_motion() {
            Motion::Reduced
        } else {
            Motion::Full
        }
    }

    /// The agents as the shell is told them changed, or which window has
    /// the keyboard did.
    pub(crate) fn halo_agents(&mut self, agents: &[Seen]) {
        let (now, motion) = (self.now(), self.halo_motion());
        if self.halo.agents(agents, now, motion) {
            self.redraw = true;
        }
        self.arm_reminder();
    }

    /// The next meeting is `near`, or not (the calendar's, not built yet).
    pub fn meeting_near(&mut self, near: Option<Meeting>) {
        let (now, motion) = (self.now(), self.halo_motion());
        if self.halo.meeting(near, now, motion) {
            self.redraw = true;
        }
    }

    /// The reminder's timer for the next one due, if it is not the one set.
    fn arm_reminder(&mut self) {
        let next = self.halo.next_reminder();
        if self.halo.timer.as_ref().map(|(at, _)| *at) == next {
            return;
        }
        if let Some((_, token)) = self.halo.timer.take() {
            self.loop_handle.remove(token);
        }
        let Some(at) = next else {
            return;
        };
        let timer = Timer::from_duration(at.saturating_sub(self.now()));
        let inserted = self.loop_handle.insert_source(timer, |_, _, app| {
            app.state.halo.timer = None;
            app.state.halo_remind();
            TimeoutAction::Drop
        });
        match inserted {
            Ok(token) => self.halo.timer = Some((at, token)),
            Err(err) => tracing::warn!(%err.error, "no timer for the halo's reminder"),
        }
    }

    fn halo_remind(&mut self) {
        let (now, motion) = (self.now(), self.halo_motion());
        if self.halo.remind(now, motion) {
            tracing::debug!("the halo's reminder");
            self.redraw = true;
        }
        self.arm_reminder();
        // Its panda peeks out once more (4.5).
        self.peek_news();
    }

    /// The halo moves on; whether it still does.
    pub fn tick_halo(&mut self, now: Duration) -> bool {
        self.halo.tick(now);
        self.halo_motion() != Motion::Still && self.halo.moving(now)
    }

    /// What the halo shows at `now`, where it is drawn.
    pub fn halo_at(&self, now: Duration) -> Option<Shown> {
        if self.halo_motion() == Motion::Still {
            return None;
        }
        self.halo.at(now)
    }

    /// The halo's elements around and over the bar on `output`, whose
    /// place at `now` is `bar` (output-local), front first: its wave, its
    /// glow.
    pub(crate) fn halo_elements<R: Shaders>(
        &self,
        renderer: &mut R,
        output: &Output,
        bar: Rectangle<f64, Logical>,
        now: Duration,
    ) -> Vec<ShaderElement> {
        let name = output.name();
        let shown = self.halo_at(now);
        let program = shown.and_then(|_| renderer.halo_program());
        let (Some(shown), Some(program)) = (shown, program) else {
            self.halo.kept.borrow_mut().remove(&name);
            return Vec::new();
        };
        let scale = output.current_scale().fractional_scale();
        let mut out = Vec::with_capacity(2);
        if let Some(wave) = shown.wave
            && let Some(element) = wave_element(&program, bar, &wave, scale)
        {
            out.push(element);
        }
        if shown.glow.shows() {
            let key = Key::new(bar, shown.glow, scale);
            let mut kept = self.halo.kept.borrow_mut();
            let element = match kept.get(&name) {
                Some((k, element)) if *k == key => element.clone(),
                _ => {
                    let element = glow_element(&program, &key);
                    kept.insert(name, (key, element.clone()));
                    element
                }
            };
            out.push(element);
        } else {
            self.halo.kept.borrow_mut().remove(&name);
        }
        out
    }
}

// --- Drawing it -------------------------------------------------------------

/// The halo's program, compiled once per GLES context; `None` when it does
/// not compile, and there is then no halo.
#[derive(Debug, Clone)]
struct Program(Option<GlesPixelProgram>);

/// For each pixel, logical pixels from the element's corner, what the
/// mockup's CSS draws as Chrome renders it: outside the bar, its
/// `box-shadow: 0 0 <blur>` (the bar's rounded box blurred, outside it
/// only); in its 1 px border, the border's colour; inside the border, the
/// `inset` shadow (what is outside the padding box, blurred, inside it)
/// and over it the wave's band, a `linear-gradient` transparent, the
/// colour, transparent, down its height. A blur is a Gaussian of deviation
/// half the CSS blur, written with the error function: exact along the
/// bar's sides, summed over rows near its rounded corners.
const SHADER: &str = r"
precision highp float;
varying vec2 v_coords;
uniform vec2 size;
uniform float alpha;
#ifdef DEBUG_FLAGS
uniform float tint;
#endif
uniform vec4 bar;
uniform float radius;
uniform vec3 colour;
uniform float border;
uniform vec2 outer;
uniform vec2 inner;
uniform vec3 wave;
uniform vec3 wave_colour;
uniform float pixel;

// Abramowitz and Stegun 7.1.26.
float erf_(float x) {
    float s = sign(x);
    float a = abs(x);
    float t = 1.0 / (1.0 + 0.3275911 * a);
    float y = 1.0 - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t
        - 0.284496736) * t + 0.254829592) * t * exp(-a * a);
    return s * y;
}

// From lo to hi, blurred, at x.
float across(float x, float lo, float hi, float sigma) {
    float k = 1.0 / (sigma * 1.41421356);
    return 0.5 * (erf_((x - lo) * k) - erf_((x - hi) * k));
}

// Signed distance to a box rounded by r: negative inside.
float rounded(vec2 p, vec4 box, float r) {
    vec2 centre = 0.5 * (box.xy + box.zw);
    vec2 half_ = 0.5 * (box.zw - box.xy);
    r = min(r, min(half_.x, half_.y));
    vec2 q = abs(p - centre) - (half_ - r);
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

// How far in from the box's sides its row at y starts, by its corners.
float inset(float y, vec4 box, float r) {
    float dy = max(max(box.y + r - y, y - (box.w - r)), 0.0);
    return r - sqrt(max(r * r - dy * dy, 0.0));
}

// The box rounded by r, blurred by a Gaussian of deviation sigma, at p.
float blurred(vec2 p, vec4 box, float r, float sigma) {
    float mass = across(p.y, box.y, box.w, sigma);
    float lo = max(box.y, p.y - 3.0 * sigma);
    float hi = min(box.w, p.y + 3.0 * sigma);
    if (hi <= lo)
        return 0.0;
    if (lo >= box.y + r && hi <= box.w - r)
        return mass * across(p.x, box.x, box.z, sigma);
    float step_ = (hi - lo) / 16.0;
    float sum = 0.0;
    float weights = 0.0;
    for (int i = 0; i < 16; i++) {
        float t = lo + (float(i) + 0.5) * step_;
        float d = (p.y - t) / sigma;
        float w = exp(-0.5 * d * d);
        float k = inset(t, box, r);
        sum += w * across(p.x, box.x + k, box.z - k, sigma);
        weights += w;
    }
    return mass * sum / max(weights, 1e-6);
}

void main() {
    vec2 p = v_coords * size;
    float d = rounded(p, bar, radius);
    float in_box = clamp(0.5 - d / pixel, 0.0, 1.0);
    vec4 pad = bar + vec4(1.0, 1.0, -1.0, -1.0);
    float pad_radius = max(radius - 1.0, 0.0);
    float pd = rounded(p, pad, pad_radius);
    float in_pad = clamp(0.5 - pd / pixel, 0.0, 1.0);
    float glow = border * max(in_box - in_pad, 0.0);
    float outer_sigma = max(outer.x, 0.01);
    if (outer.y > 0.0 && in_box < 1.0 && d < 3.0 * outer_sigma)
        glow += outer.y * blurred(p, bar, radius, outer_sigma) * (1.0 - in_box);
    float inner_sigma = max(inner.x, 0.01);
    if (inner.y > 0.0 && in_pad > 0.0 && pd > -3.0 * inner_sigma)
        glow += inner.y * (1.0 - blurred(p, pad, pad_radius, inner_sigma)) * in_pad;
    vec4 c = vec4(colour, 1.0) * glow;
    if (wave.z > 0.0) {
        float u = (p.y - wave.x) / wave.y;
        float a = wave.z * max(1.0 - abs(2.0 * u - 1.0), 0.0) * in_pad;
        c = vec4(wave_colour, 1.0) * a + c * (1.0 - a);
    }
    gl_FragColor = c * alpha;
#ifdef DEBUG_FLAGS
    if (tint == 1.0)
        gl_FragColor = vec4(0.0, 0.2, 0.0, 0.2) + gl_FragColor * 0.8;
#endif
}
";

/// The halo's program for `renderer`'s context, compiled the first time.
pub(crate) fn program(renderer: &mut GlesRenderer) -> Option<GlesPixelProgram> {
    if renderer
        .egl_context()
        .user_data()
        .get::<Program>()
        .is_none()
    {
        let uniforms = [
            UniformName::new("bar", UniformType::_4f),
            UniformName::new("radius", UniformType::_1f),
            UniformName::new("colour", UniformType::_3f),
            UniformName::new("border", UniformType::_1f),
            UniformName::new("outer", UniformType::_2f),
            UniformName::new("inner", UniformType::_2f),
            UniformName::new("wave", UniformType::_3f),
            UniformName::new("wave_colour", UniformType::_3f),
            UniformName::new("pixel", UniformType::_1f),
        ];
        let program = renderer
            .compile_custom_pixel_shader(SHADER, &uniforms)
            .map_err(|err| tracing::warn!(%err, "the halo's shader does not compile: no halo"))
            .ok();
        renderer
            .egl_context()
            .user_data()
            .insert_if_missing(|| Program(program));
    }
    renderer
        .egl_context()
        .user_data()
        .get::<Program>()
        .and_then(|p| p.0.clone())
}

/// The glow drawn: where, around which bar, how; the same key, the same
/// element.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Key {
    /// Output-local, logical.
    area: Rectangle<i32, Logical>,
    /// The bar from the area's corner: left, top, right, bottom.
    bar: [f32; 4],
    glow: Glow,
    scale: f64,
}

impl Key {
    fn new(bar: Rectangle<f64, Logical>, glow: Glow, scale: f64) -> Self {
        // A Gaussian's reach, three deviations: one and a half CSS blurs.
        let reach = (1.5 * f64::from(glow.outer.blur)).ceil() + 2.0;
        let left = (bar.loc.x - reach).floor();
        let top = (bar.loc.y - reach).floor();
        let right = (bar.loc.x + bar.size.w + reach).ceil();
        let bottom = (bar.loc.y + bar.size.h + reach).ceil();
        Self {
            area: Rectangle::new(
                (left as i32, top as i32).into(),
                ((right - left) as i32, (bottom - top) as i32).into(),
            ),
            bar: local(bar, left, top),
            glow,
            scale,
        }
    }
}

/// `bar` from (`left`, `top`): its left, top, right, bottom.
fn local(bar: Rectangle<f64, Logical>, left: f64, top: f64) -> [f32; 4] {
    [
        (bar.loc.x - left) as f32,
        (bar.loc.y - top) as f32,
        (bar.loc.x + bar.size.w - left) as f32,
        (bar.loc.y + bar.size.h - top) as f32,
    ]
}

fn uniforms(
    bar: [f32; 4],
    glow: &Glow,
    wave: (f32, f32, f32),
    wave_colour: [u8; 3],
    scale: f64,
) -> Vec<Uniform<'static>> {
    let [r, g, b] = glow.colour;
    let [wr, wg, wb] = wave_colour.map(|c| f32::from(c) / 255.0);
    vec![
        Uniform::new("bar", (bar[0], bar[1], bar[2], bar[3])),
        Uniform::new("radius", hocus_link::RADIUS as f32),
        Uniform::new("colour", (r, g, b)),
        Uniform::new("border", glow.border),
        Uniform::new("outer", (glow.outer.blur / 2.0, glow.outer.alpha)),
        Uniform::new("inner", (glow.inner.blur / 2.0, glow.inner.alpha)),
        Uniform::new("wave", wave),
        Uniform::new("wave_colour", (wr, wg, wb)),
        Uniform::new("pixel", (1.0 / scale) as f32),
    ]
}

fn glow_element(program: &GlesPixelProgram, key: &Key) -> ShaderElement {
    ShaderElement(PixelShaderElement::new(
        program.clone(),
        key.area,
        None,
        1.0,
        uniforms(key.bar, &key.glow, (0.0, 1.0, 0.0), [0; 3], key.scale),
        Kind::Unspecified,
    ))
}

/// Where the wave's band is now, inside the bar's border: its element's
/// area (output-local, as tall as the band and cut to the bar), the band's
/// top from that area's top, and its height.
fn wave_place(
    bar: Rectangle<f64, Logical>,
    wave: &WaveAt,
) -> Option<(Rectangle<i32, Logical>, f64, f64)> {
    let inside = bar.size.h - 2.0;
    let band = WAVE_BAND * inside;
    let band_top = bar.loc.y + 1.0 + wave.top * inside;
    let top = band_top.max(bar.loc.y + 1.0).floor();
    let bottom = (band_top + band).min(bar.loc.y + bar.size.h - 1.0).ceil();
    let left = bar.loc.x.floor();
    let right = (bar.loc.x + bar.size.w).ceil();
    if bottom <= top || right <= left {
        return None;
    }
    let area = Rectangle::new(
        (left as i32, top as i32).into(),
        ((right - left) as i32, (bottom - top) as i32).into(),
    );
    Some((area, band_top - top, band))
}

/// The wave's band now: an element of its own, over the glow.
fn wave_element(
    program: &GlesPixelProgram,
    bar: Rectangle<f64, Logical>,
    wave: &WaveAt,
    scale: f64,
) -> Option<ShaderElement> {
    let (area, top, band) = wave_place(bar, wave)?;
    let corner = (f64::from(area.loc.x), f64::from(area.loc.y));
    Some(ShaderElement(PixelShaderElement::new(
        program.clone(),
        area,
        None,
        1.0,
        uniforms(
            local(bar, corner.0, corner.1),
            &Glow::default(),
            (top as f32, band as f32, wave.alpha),
            wave.colour,
            scale,
        ),
        Kind::Unspecified,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn agent(id: u64, state: &str, since: Duration) -> Seen {
        Seen {
            id,
            concern: Concern::of_agent(state),
            since,
            looked_at: false,
        }
    }

    #[test]
    fn one_state_shows_the_most_important_first() {
        let mut halo = Halo::default();
        let now = ms(1000);
        let mut agents = vec![agent(1, "done", now)];
        halo.agents(&agents, now, Motion::Full);
        assert_eq!(halo.concern(), Some(Concern::ToSee));
        agents.push(agent(2, "failed", now));
        halo.agents(&agents, now, Motion::Full);
        assert_eq!(halo.concern(), Some(Concern::Stuck));
        halo.meeting(Some(Meeting::InTen), now, Motion::Full);
        assert_eq!(halo.concern(), Some(Concern::MeetingSoon));
        agents.push(agent(3, "waiting", now));
        halo.agents(&agents, now, Motion::Full);
        assert_eq!(halo.concern(), Some(Concern::Waiting));
        halo.meeting(Some(Meeting::InTwo), now, Motion::Full);
        assert_eq!(halo.concern(), Some(Concern::MeetingNow));
        halo.meeting(None, now, Motion::Full);
        // Working, idle and seen agents concern nobody.
        let calm = [
            agent(1, "working", now),
            agent(2, "idle", now),
            agent(3, "seen", now),
        ];
        halo.agents(&calm, now, Motion::Full);
        assert_eq!(halo.concern(), None);
    }

    #[test]
    fn the_looks_are_the_table_s() {
        let waiting = Concern::Waiting.glow();
        assert_eq!(waiting.rgb(), [0xF5, 0xB4, 0x4A]);
        assert_eq!(
            (waiting.border, waiting.outer, waiting.inner),
            (
                0.85,
                Shadow {
                    blur: 34.0,
                    alpha: 0.45
                },
                Shadow {
                    blur: 22.0,
                    alpha: 0.18
                }
            )
        );
        assert_eq!(Concern::MeetingNow.glow().outer.blur, 44.0);
        assert_eq!(Concern::MeetingNow.glow().outer.alpha, 0.60);
        let soft = Concern::ToSee.glow();
        assert_eq!(soft.rgb(), [0x6C, 0xC2, 0x87]);
        assert_eq!(
            (soft.border, soft.outer.alpha, soft.inner.alpha),
            (0.45, 0.25, 0.10)
        );
        assert_eq!((soft.outer.blur, soft.inner.blur), (20.0, 14.0));
        assert_eq!(Concern::Stuck.glow().rgb(), [0xF2, 0x94, 0x5B]);
        assert_eq!(Concern::MeetingSoon.glow().rgb(), [0x8F, 0xB3, 0xFF]);
        assert_eq!(
            [
                Concern::MeetingNow,
                Concern::Waiting,
                Concern::MeetingSoon,
                Concern::Stuck,
                Concern::ToSee
            ]
            .map(Concern::wave),
            [Some(0.42), Some(0.38), Some(0.22), None, None]
        );
    }

    #[test]
    fn a_waiting_agent_runs_a_wave_as_the_glow_fades_in() {
        let mut halo = Halo::default();
        let start = ms(1000);
        assert!(halo.agents(&[agent(1, "waiting", start)], start, Motion::Full));
        assert_eq!(halo.waves, 1);
        let at = |t| halo.at(ms(t)).expect("shown");
        // The band starts above the bar, quickly down, landing softly
        // below it.
        let first = at(1000).wave.expect("a wave");
        assert!((first.top + 0.4).abs() < 1e-9, "{first:?}");
        assert_eq!((first.alpha, first.colour), (0.38, [0xF5, 0xB4, 0x4A]));
        let middle = at(1150).wave.expect("a wave");
        assert!(middle.top > 0.3 && middle.top < 0.7, "{middle:?}");
        assert!(at(1599).wave.is_some());
        assert_eq!(at(1600).wave, None);
        // The glow, ease-out: nearly half there at a third of its time.
        let third = at(1200).glow;
        assert!(third.border > 0.38 && third.border < 0.85, "{third:?}");
        assert!(
            third.outer.blur > 15.0 && third.outer.blur < 34.0,
            "{third:?}"
        );
        assert_eq!(at(1600).glow, Concern::Waiting.glow());
        assert!(halo.moving(ms(1599)));
        assert!(!halo.moving(ms(1600)));
        // A second one waiting: its wave too; the glow stays.
        let both = [agent(1, "waiting", start), agent(2, "waiting", ms(2000))];
        assert!(halo.agents(&both, ms(2000), Motion::Full));
        assert_eq!(halo.waves, 2);
        assert_eq!(
            halo.at(ms(2000)).expect("shown").glow,
            Concern::Waiting.glow()
        );
        // The same again: nothing.
        assert!(!halo.agents(&both, ms(3000), Motion::Full));
        assert_eq!(halo.waves, 2);
    }

    #[test]
    fn stuck_and_to_see_glow_without_a_wave() {
        for (state, concern) in [("failed", Concern::Stuck), ("done", Concern::ToSee)] {
            let mut halo = Halo::default();
            halo.agents(&[agent(1, state, ms(0))], ms(0), Motion::Full);
            assert_eq!(halo.waves, 0);
            assert_eq!(halo.at(ms(300)).expect("shown").wave, None);
            assert_eq!(halo.at(ms(600)).expect("shown").glow, concern.glow());
            assert_eq!(halo.next_reminder(), None);
        }
    }

    #[test]
    fn a_wave_runs_only_for_what_shows() {
        let mut halo = Halo::default();
        halo.meeting(Some(Meeting::InTwo), ms(0), Motion::Full);
        assert_eq!(halo.waves, 1);
        assert_eq!(
            halo.at(ms(0)).and_then(|s| s.wave).map(|w| w.alpha),
            Some(0.42)
        );
        // An agent waiting under a meeting in 2 min: no wave of its own.
        halo.agents(&[agent(1, "waiting", ms(700))], ms(700), Motion::Full);
        assert_eq!(halo.waves, 1);
        // The meeting in 10 min, softer, under an agent waiting: none.
        let mut halo = Halo::default();
        halo.agents(&[agent(1, "waiting", ms(0))], ms(0), Motion::Full);
        halo.meeting(Some(Meeting::InTen), ms(700), Motion::Full);
        assert_eq!(halo.waves, 1);
        assert_eq!(halo.concern(), Some(Concern::Waiting));
        // Nearer, at 2 min: its wave, over the agent's.
        halo.meeting(Some(Meeting::InTwo), ms(800), Motion::Full);
        assert_eq!(halo.waves, 2);
    }

    #[test]
    fn the_reminder_comes_once_at_the_fifth_minute() {
        let mut halo = Halo::default();
        let since = ms(10_000);
        let waiting = [agent(1, "waiting", since)];
        halo.agents(&waiting, since, Motion::Full);
        assert_eq!(halo.next_reminder(), Some(since + REMINDER));
        let early = since + REMINDER - ms(5);
        assert!(!halo.remind(early, Motion::Full));
        assert_eq!(halo.waves, 1);
        let due = since + REMINDER;
        assert!(halo.remind(due, Motion::Full));
        assert_eq!(halo.waves, 2);
        assert!(halo.at(due).and_then(|s| s.wave).is_some());
        assert_eq!(halo.next_reminder(), None);
        // Once: still waiting later, the same agent, nothing more.
        halo.agents(&waiting, due + ms(1000), Motion::Full);
        assert!(!halo.remind(due + REMINDER, Motion::Full));
        assert_eq!(halo.waves, 2);
        // Waiting again after an answer: a new wave, a new reminder.
        halo.agents(&[agent(1, "working", due)], due, Motion::Full);
        let again = due + ms(60_000);
        halo.agents(&[agent(1, "waiting", again)], again, Motion::Full);
        assert_eq!(halo.waves, 3);
        assert_eq!(halo.next_reminder(), Some(again + REMINDER));
        // Backdated (its since moved): the same episode, its reminder too.
        halo.agents(
            &[agent(1, "waiting", again - ms(1000))],
            again,
            Motion::Full,
        );
        assert_eq!(halo.waves, 3);
        assert_eq!(halo.next_reminder(), Some(again - ms(1000) + REMINDER));
    }

    #[test]
    fn what_begins_is_news_for_the_peek() {
        let mut halo = Halo::default();
        let agents = [
            agent(1, "waiting", ms(0)),
            agent(2, "done", ms(0)),
            agent(3, "failed", ms(0)),
            Seen {
                looked_at: true,
                ..agent(4, "waiting", ms(0))
            },
        ];
        halo.agents(&agents, ms(0), Motion::Still);
        // Waiting and finished, not stuck, not looked at.
        assert_eq!(
            halo.take_news(),
            vec![(1, Concern::Waiting), (2, Concern::ToSee)]
        );
        assert!(halo.counts(1, Concern::Waiting) && halo.counts(2, Concern::ToSee));
        assert!(!halo.counts(4, Concern::Waiting) && !halo.counts(1, Concern::ToSee));
        // The same again: nothing new; the reminder is.
        halo.agents(&agents, ms(1000), Motion::Still);
        assert_eq!(halo.take_news(), vec![]);
        halo.remind(REMINDER, Motion::Still);
        assert_eq!(halo.take_news(), vec![(1, Concern::Waiting)]);
        // Gone to: it no longer counts.
        halo.acknowledge(1);
        assert!(!halo.counts(1, Concern::Waiting));
    }

    #[test]
    fn acting_ends_it_with_its_fade() {
        let mut halo = Halo::default();
        halo.agents(&[agent(1, "waiting", ms(0))], ms(0), Motion::Full);
        // Gone to: it fades out over 500 ms, ease-in, and nothing remains.
        halo.acknowledge(1);
        assert!(halo.agents(&[agent(1, "waiting", ms(0))], ms(2000), Motion::Full));
        assert_eq!(halo.concern(), None);
        assert_eq!(halo.next_reminder(), None);
        let fading = halo.at(ms(2250)).expect("fading");
        assert_eq!(fading.concern, None);
        assert_eq!(fading.glow.rgb(), [0xF5, 0xB4, 0x4A]);
        // Slowly first.
        assert!(fading.glow.border > 0.5, "{fading:?}");
        assert!(halo.at(ms(2499)).is_some());
        assert_eq!(halo.at(ms(2500)), None);
        assert!(!halo.moving(ms(2500)));
        // Looked at as it starts waiting (its window has the keyboard):
        // no halo at all.
        let mut halo = Halo::default();
        let looked = Seen {
            looked_at: true,
            ..agent(1, "waiting", ms(0))
        };
        assert!(!halo.agents(&[looked], ms(0), Motion::Full));
        assert_eq!((halo.waves, halo.at(ms(0))), (0, None));
        // Its window getting the keyboard later ends it too.
        let mut halo = Halo::default();
        halo.agents(&[agent(1, "failed", ms(0))], ms(0), Motion::Full);
        assert!(halo.agents(
            &[Seen {
                looked_at: true,
                ..agent(1, "failed", ms(0))
            }],
            ms(900),
            Motion::Full
        ));
        assert_eq!(halo.concern(), None);
    }

    #[test]
    fn the_glow_goes_from_one_state_to_the_next() {
        let mut halo = Halo::default();
        let both = [agent(1, "waiting", ms(0)), agent(2, "failed", ms(0))];
        halo.agents(&both, ms(0), Motion::Full);
        // The waiting one answered: from amber to orange, over 600 ms.
        halo.agents(
            &[agent(1, "working", ms(1000)), both[1]],
            ms(1000),
            Motion::Full,
        );
        let middle = halo.at(ms(1100)).expect("shown").glow;
        let [r, g, _] = middle.rgb();
        assert!(g < 0xB4 && g > 0x94 && r > 0xF2, "{middle:?}");
        assert_eq!(
            halo.at(ms(1600)).expect("shown").glow,
            Concern::Stuck.glow()
        );
        assert_eq!(halo.waves, 1);
    }

    #[test]
    fn reduced_motion_has_no_wave_and_short_fades() {
        let mut halo = Halo::default();
        halo.agents(&[agent(1, "waiting", ms(0))], ms(0), Motion::Reduced);
        assert_eq!(halo.waves, 0);
        let shown = halo.at(ms(100)).expect("shown");
        assert_eq!(shown.wave, None);
        assert!(shown.glow.border > 0.42 && shown.glow.border < 0.85);
        assert_eq!(
            halo.at(ms(200)).expect("shown").glow,
            Concern::Waiting.glow()
        );
        halo.remind(REMINDER, Motion::Reduced);
        assert_eq!(halo.waves, 0);
        halo.acknowledge(1);
        halo.agents(&[agent(1, "waiting", ms(0))], ms(1000), Motion::Reduced);
        assert!(halo.at(ms(1199)).is_some());
        assert_eq!(halo.at(ms(1200)), None);
        // Not drawn: at once, nothing moving.
        let mut halo = Halo::default();
        halo.agents(&[agent(1, "waiting", ms(0))], ms(0), Motion::Still);
        assert_eq!(halo.waves, 0);
        assert!(!halo.moving(ms(0)));
        assert_eq!(halo.at(ms(0)).expect("shown").glow, Concern::Waiting.glow());
    }

    #[test]
    fn the_glow_is_drawn_as_far_as_it_reaches() {
        let bar = Rectangle::<f64, Logical>::new((1716.0, 8.0).into(), (196.0, 1064.0).into());
        let key = Key::new(bar, Concern::Waiting.glow(), 1.0);
        // 34 px of blur reach 51 px, and 2 for the edges' pixels.
        assert_eq!(key.area.loc, (1716 - 53, 8 - 53).into());
        assert_eq!(key.area.size, (196 + 106, 1064 + 106).into());
        assert_eq!(key.bar, [53.0, 53.0, 249.0, 1117.0]);
        let wave = WaveAt {
            top: 0.5,
            alpha: 0.38,
            colour: [0; 3],
        };
        // The band, 40 % of the 1062 px inside the border, from its middle.
        let (area, top, band) = wave_place(bar, &wave).expect("shown");
        assert_eq!(area, Rectangle::new((1716, 540).into(), (196, 425).into()));
        assert!(
            (top - 0.0).abs() < 1e-9 && (band - 424.8).abs() < 1e-9,
            "{top} {band}"
        );
        // Above the bar, cut to it.
        let (area, top, _) = wave_place(bar, &WaveAt { top: -0.3, ..wave }).expect("shown");
        assert_eq!(area.loc.y, 9);
        assert!((top + 0.3 * 1062.0).abs() < 1e-9, "{top}");
        let gone = WaveAt { top: 1.0, ..wave };
        assert_eq!(wave_place(bar, &gone), None);
    }
}
