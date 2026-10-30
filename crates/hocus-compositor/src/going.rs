//! Going to an agent (docs/design/bar.md §8, 4.3): from its row in the
//! bar, a streak of the row's colour runs to the centre of the agent's
//! window, over the windows and under the bar, and the window's rim pulses
//! once as it lands. When the window is on another desktop, the strip
//! slides there first and the streak starts once it has.
//!
//! Everything is a start time and closed-form curves on the frame clock,
//! as the rest of the motion ([`crate::motion`]): the go-to is set once,
//! and each frame draws what it is at that moment; while it runs, drawing
//! a frame asks for the next, and then nothing is drawn. With reduced
//! motion, or a compositor drawing on the CPU (which has no shaders for
//! the streak), there is no slide and no streak: the rim lights and fades
//! out.

use std::time::Duration;

use smithay::{
    backend::renderer::{
        Renderer,
        element::{Element, Id, Kind, RenderElement, UnderlyingStorage},
        gles::{
            GlesError, GlesFrame, GlesPixelProgram, GlesRenderer, Uniform, UniformName,
            UniformType, element::PixelShaderElement,
        },
        pixman::PixmanRenderer,
        utils::{CommitCounter, DamageSet, OpaqueRegions},
    },
    utils::{Buffer as BufferCoords, Logical, Physical, Point, Rectangle, Scale, Transform},
};

use crate::{
    State,
    model::WindowId,
    motion::{self, Bezier, Tween, lerp},
};

/// The streak grows to the window in this time, along this curve, then
/// fades out (CSS's `ease-in`).
pub const GROW: Duration = Duration::from_millis(300);
pub const GROW_CURVE: Bezier = Bezier(0.3, 0.7, 0.2, 1.0);
pub const FADE: Duration = Duration::from_millis(220);
/// Its line, logical pixels, with round ends; and its glow, CSS's blur
/// (twice the deviation).
pub const THICKNESS: f64 = 4.0;
pub const GLOW: f64 = 12.0;

/// The rim's pulse, as the mockup's keyframes: in a quarter of its time,
/// a 2 px rim at 95 % and a ring of 4 px at 55 %; then the ring grows to
/// 14 px as both fade out. Each step eases out, as CSS's keyframes do.
const PEAK: f64 = 0.25;
pub const RIM_WIDTH: f64 = 2.0;
const RIM_ALPHA: f64 = 0.95;
const RING_PEAK: f64 = 4.0;
const RING_END: f64 = 14.0;
const RING_ALPHA: f64 = 0.55;

/// The colour of a row by the state the shell is told (`hocus-link`'s
/// words): waiting amber, stuck orange, to see green, working the accent,
/// resting grey.
pub fn colour(state: &str) -> [u8; 3] {
    match state {
        "waiting" => [0xF5, 0xB4, 0x4A],
        "failed" => [0xF2, 0x94, 0x5B],
        "done" => [0x7E, 0xE0, 0xA0],
        "working" => [0x6C, 0xC2, 0x87],
        _ => [0xA9, 0xB2, 0xC2],
    }
}

/// A go-to under way, with its streak.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Going {
    pub window: WindowId,
    /// Where the streak starts, global and logical: its row's left edge,
    /// at its middle.
    pub from: (f64, f64),
    pub colour: [u8; 3],
    /// When the streak starts: once the strip has slid to the window.
    pub start: Duration,
}

/// The streak at a moment: from its start to its head (global, logical),
/// and how much of it shows.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Streak {
    pub from: (f64, f64),
    pub head: (f64, f64),
    pub alpha: f32,
    pub colour: [u8; 3],
}

impl Going {
    /// When the streak reaches the window, and the rim pulses.
    pub fn lands(&self) -> Duration {
        self.start + GROW
    }

    pub fn done(&self, now: Duration) -> bool {
        now >= self.lands() + FADE
    }

    /// The streak at `now` toward `to` (global): `None` before it starts
    /// and once it has faded.
    pub fn streak(&self, to: (f64, f64), now: Duration) -> Option<Streak> {
        if now < self.start || self.done(now) {
            return None;
        }
        let grow = Tween::new(self.start, GROW, GROW_CURVE).at(now);
        let fade = Tween::new(self.lands(), FADE, Bezier::EASE_IN).at(now);
        let head = (lerp(self.from.0, to.0, grow), lerp(self.from.1, to.1, grow));
        Some(Streak {
            from: self.from,
            head,
            alpha: (1.0 - fade) as f32,
            colour: self.colour,
        })
    }
}

/// The rim's pulse at a moment: the 2 px rim's alpha, and the ring's width
/// (logical pixels) and alpha.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pulse {
    pub rim: f32,
    pub ring: Option<(f64, f32)>,
}

/// The pulse of `tween` at `now`; `ring`: the ring grows (not with reduced
/// motion or on the CPU, where the rim only lights and fades out).
pub fn pulse(tween: &Tween, now: Duration, ring: bool) -> Pulse {
    let t = tween.linear(now);
    let ease = |x: f64| Bezier::EASE_OUT.at(x);
    let (rim, width, ring_alpha) = if t < PEAK {
        let e = ease(t / PEAK);
        (RIM_ALPHA * e, RING_PEAK * e, RING_ALPHA * e)
    } else {
        let e = ease((t - PEAK) / (1.0 - PEAK));
        (
            RIM_ALPHA * (1.0 - e),
            lerp(RING_PEAK, RING_END, e),
            RING_ALPHA * (1.0 - e),
        )
    };
    Pulse {
        rim: rim as f32,
        ring: (ring && width > 0.0 && ring_alpha > 0.0).then_some((width, ring_alpha as f32)),
    }
}

impl State {
    /// Whether going to an agent moves: neither reduced motion nor a
    /// compositor on the CPU.
    pub fn going_moves(&self) -> bool {
        !self.reduced_motion() && self.blur
    }

    /// Goes to window `id` from `from` (global, its row's left edge and
    /// middle; `None`: no streak), the streak in `colour`: its desktop
    /// first if it is another, sliding, then the streak; the window has the
    /// keyboard at once, and its rim pulses as the streak lands.
    pub fn go_to(&mut self, id: WindowId, from: Option<(f64, f64)>, colour: [u8; 3]) {
        let moves = self.going_moves();
        let now = self.now();
        let slides = moves && !self.on_current_desktop(id);
        self.bring_moving(id, moves);
        // Brought back from minimised, or not shown at all: no streak to a
        // place it is not.
        let shown = self.model.is_shown(id);
        let start = if slides {
            now + motion::DESKTOP_SLIDE
        } else {
            now
        };
        self.going = from.filter(|_| moves && shown).map(|from| Going {
            window: id,
            from,
            colour,
            start,
        });
        let lands = self.going.as_ref().map_or(now, Going::lands);
        if let Some(managed) = self.windows.get_mut(&id) {
            managed.landing = Some((Tween::new(lands, motion::PULSE, Bezier::LINEAR), moves));
        }
        self.redraw = true;
    }

    /// The streak at `now`, aimed at its window's centre as drawn.
    pub fn streak_at(&self, now: Duration) -> Option<Streak> {
        let going = self.going.as_ref()?;
        let rect = self.visual_rect(going.window, now)?;
        let (offset, _) = self.strip_offset(going.window, now)?;
        let to = (
            rect.loc.x + offset + rect.size.w / 2.0,
            rect.loc.y + rect.size.h / 2.0,
        );
        going.streak(to, now)
    }

    /// The go-to moves on; whether it still does.
    pub fn tick_going(&mut self, now: Duration) -> bool {
        match &self.going {
            Some(going) if going.done(now) || !self.windows.contains_key(&going.window) => {
                self.going = None;
                false
            }
            Some(_) => true,
            None => false,
        }
    }
}

// --- Drawing the streak -------------------------------------------------------

/// Renderers that can draw the compositor's own shaders, the streak and
/// the halo (4.4); the CPU's draws neither.
pub trait Shaders: Renderer + Sized {
    /// The streak's element on an output whose top left is `origin`
    /// (global, logical) at `scale`. `None`: not drawn.
    fn streak_element(
        &mut self,
        streak: &Streak,
        origin: Point<f64, Logical>,
        scale: f64,
    ) -> Option<ShaderElement>;

    /// The halo's program (`crate::halo`), compiled once; `None`: no halo.
    fn halo_program(&mut self) -> Option<GlesPixelProgram>;

    fn draw_shader(
        frame: &mut Self::Frame<'_, '_>,
        element: &ShaderElement,
        src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), Self::Error>;
}

/// One of the compositor's shaders drawn: the streak, a new one each frame
/// so that the damage is where it was and where it is; the halo's glow,
/// kept while it does not change, so that it leaves no damage at rest.
#[derive(Debug, Clone)]
pub struct ShaderElement(pub(crate) PixelShaderElement);

impl Shaders for PixmanRenderer {
    fn streak_element(
        &mut self,
        _streak: &Streak,
        _origin: Point<f64, Logical>,
        _scale: f64,
    ) -> Option<ShaderElement> {
        None
    }

    fn halo_program(&mut self) -> Option<GlesPixelProgram> {
        None
    }

    fn draw_shader(
        _frame: &mut Self::Frame<'_, '_>,
        _element: &ShaderElement,
        _src: Rectangle<f64, BufferCoords>,
        _dst: Rectangle<i32, Physical>,
        _damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// The streak's program, compiled once per GLES context; `None` when it
/// does not compile, and there is then no streak.
#[derive(Debug, Clone)]
struct Program(Option<GlesPixelProgram>);

/// For each pixel: the line, a 4 px box from the start to the head with
/// round ends, transparent at the start and full at the head (CSS's
/// `linear-gradient(90deg, transparent, colour)`); around it, outside
/// only, the glow of its box blurred (CSS's `box-shadow: 0 0 12px`), a
/// Gaussian written with the error function. Positions are logical pixels
/// from the element's corner.
const SHADER: &str = r"
precision highp float;
varying vec2 v_coords;
uniform vec2 size;
uniform float alpha;
#ifdef DEBUG_FLAGS
uniform float tint;
#endif
uniform vec2 from;
uniform vec2 head;
uniform vec3 colour;
uniform float half_width;
uniform float sigma;
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

// A box from lo to hi blurred, at x.
float blurred(float x, float lo, float hi) {
    float k = 1.0 / (sigma * 1.41421356);
    return 0.5 * (erf_((x - lo) * k) - erf_((x - hi) * k));
}

void main() {
    vec2 p = v_coords * size;
    vec2 d = head - from;
    float len = length(d);
    vec2 u = len > 0.0 ? d / len : vec2(1.0, 0.0);
    vec2 q = p - from;
    float along = dot(q, u);
    float across = dot(q, vec2(-u.y, u.x));
    float end = max(len - half_width, half_width);
    float edge = length(vec2(along - clamp(along, half_width, end), across)) - half_width;
    float cover = clamp(0.5 - edge / pixel, 0.0, 1.0);
    float grade = len > 0.0 ? clamp(along / len, 0.0, 1.0) : 0.0;
    float glow = blurred(across, -half_width, half_width) * blurred(along, 0.0, len);
    float a = cover * grade + glow * (1.0 - cover);
    gl_FragColor = vec4(colour, 1.0) * a * alpha;
#ifdef DEBUG_FLAGS
    if (tint == 1.0)
        gl_FragColor = vec4(0.0, 0.2, 0.0, 0.2) + gl_FragColor * 0.8;
#endif
}
";

fn compile(renderer: &mut GlesRenderer) -> Option<GlesPixelProgram> {
    let uniforms = [
        UniformName::new("from", UniformType::_2f),
        UniformName::new("head", UniformType::_2f),
        UniformName::new("colour", UniformType::_3f),
        UniformName::new("half_width", UniformType::_1f),
        UniformName::new("sigma", UniformType::_1f),
        UniformName::new("pixel", UniformType::_1f),
    ];
    renderer
        .compile_custom_pixel_shader(SHADER, &uniforms)
        .map_err(|err| tracing::warn!(%err, "the streak's shader does not compile: no streak"))
        .ok()
}

/// Where the streak is drawn, output-local and logical: around its line,
/// as far as its glow reaches.
pub fn streak_area(streak: &Streak, origin: Point<f64, Logical>) -> Rectangle<i32, Logical> {
    let reach = GLOW * 2.0 + THICKNESS;
    let (x0, y0) = (streak.from.0 - origin.x, streak.from.1 - origin.y);
    let (x1, y1) = (streak.head.0 - origin.x, streak.head.1 - origin.y);
    let left = (x0.min(x1) - reach).floor();
    let top = (y0.min(y1) - reach).floor();
    let right = (x0.max(x1) + reach).ceil();
    let bottom = (y0.max(y1) + reach).ceil();
    Rectangle::new(
        (left as i32, top as i32).into(),
        ((right - left) as i32, (bottom - top) as i32).into(),
    )
}

impl Shaders for GlesRenderer {
    fn streak_element(
        &mut self,
        streak: &Streak,
        origin: Point<f64, Logical>,
        scale: f64,
    ) -> Option<ShaderElement> {
        if self.egl_context().user_data().get::<Program>().is_none() {
            let program = compile(self);
            self.egl_context()
                .user_data()
                .insert_if_missing(|| Program(program));
        }
        let program = self
            .egl_context()
            .user_data()
            .get::<Program>()
            .and_then(|p| p.0.clone())?;
        if streak.alpha <= 0.0 {
            return None;
        }
        let area = streak_area(streak, origin);
        let corner = (
            origin.x + f64::from(area.loc.x),
            origin.y + f64::from(area.loc.y),
        );
        let local = |(x, y): (f64, f64)| ((x - corner.0) as f32, (y - corner.1) as f32);
        let [r, g, b] = streak.colour.map(|c| f32::from(c) / 255.0);
        let uniforms = vec![
            Uniform::new("from", local(streak.from)),
            Uniform::new("head", local(streak.head)),
            Uniform::new("colour", (r, g, b)),
            Uniform::new("half_width", (THICKNESS / 2.0) as f32),
            Uniform::new("sigma", (GLOW / 2.0) as f32),
            Uniform::new("pixel", (1.0 / scale) as f32),
        ];
        Some(ShaderElement(PixelShaderElement::new(
            program,
            area,
            None,
            streak.alpha,
            uniforms,
            Kind::Unspecified,
        )))
    }

    fn halo_program(&mut self) -> Option<GlesPixelProgram> {
        crate::halo::program(self)
    }

    fn draw_shader(
        frame: &mut GlesFrame<'_, '_>,
        element: &ShaderElement,
        src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        RenderElement::<GlesRenderer>::draw(&element.0, frame, src, dst, damage, opaque_regions)
    }
}

impl Element for ShaderElement {
    fn id(&self) -> &Id {
        self.0.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.0.current_commit()
    }

    fn src(&self) -> Rectangle<f64, BufferCoords> {
        self.0.src()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.0.geometry(scale)
    }

    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.0.location(scale)
    }

    fn transform(&self) -> Transform {
        Transform::Normal
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.0.damage_since(scale, commit)
    }

    fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        OpaqueRegions::default()
    }

    fn alpha(&self) -> f32 {
        self.0.alpha()
    }

    fn kind(&self) -> Kind {
        Kind::Unspecified
    }
}

impl<R: Shaders> RenderElement<R> for ShaderElement {
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        R::draw_shader(frame, self, src, dst, damage, opaque_regions)
    }

    fn underlying_storage(&self, _renderer: &mut R) -> Option<UnderlyingStorage<'_>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn going(start: Duration) -> Going {
        Going {
            window: WindowId(1),
            from: (1700.0, 200.0),
            colour: colour("waiting"),
            start,
        }
    }

    #[test]
    fn the_streak_grows_to_the_window_then_fades_there() {
        let going = going(ms(1000));
        let to = (500.0, 600.0);
        assert_eq!(going.streak(to, ms(999)), None, "not before the slide");
        let at = |t| going.streak(to, ms(t)).expect("shown");
        let start = at(1000);
        assert_eq!(start.head, going.from);
        // Fast away, landing softly: past half the way at a third of the
        // time.
        let third = at(1100);
        assert!(third.head.0 < 1100.0 && third.head.1 > 400.0, "{third:?}");
        assert_eq!(third.alpha, 1.0);
        let landed = at(1300);
        assert_eq!(landed.head, to);
        assert_eq!(landed.alpha, 1.0);
        // It fades where it landed, slowly first (ease-in).
        let fading = at(1410);
        assert_eq!(fading.head, to);
        assert!(fading.alpha > 0.5 && fading.alpha < 0.95, "{fading:?}");
        assert_eq!(going.lands(), ms(1300));
        assert!(!going.done(ms(1519)));
        assert!(going.done(ms(1520)));
        assert_eq!(going.streak(to, ms(1520)), None);
    }

    #[test]
    fn the_rim_peaks_at_a_quarter_and_the_ring_grows_as_both_fade() {
        let tween = Tween::new(ms(0), motion::PULSE, Bezier::LINEAR);
        assert_eq!(pulse(&tween, ms(0), true), Pulse::default());
        let peak = pulse(&tween, ms(150), true);
        assert!((peak.rim - 0.95).abs() < 1e-6, "{peak:?}");
        let (width, alpha) = peak.ring.expect("a ring");
        assert!((width - 4.0).abs() < 1e-6 && (alpha - 0.55).abs() < 1e-6);
        let later = pulse(&tween, ms(400), true);
        let (wider, fainter) = later.ring.expect("a ring");
        assert!(wider > 10.0 && wider < 14.0, "{later:?}");
        assert!(fainter < 0.2 && later.rim < 0.4, "{later:?}");
        assert_eq!(pulse(&tween, ms(600), true).rim, 0.0);
        // Without motion: the rim alone.
        assert_eq!(pulse(&tween, ms(150), false).ring, None);
        // Not before it starts (a streak still on its way).
        let ahead = Tween::new(ms(500), motion::PULSE, Bezier::LINEAR);
        assert_eq!(pulse(&ahead, ms(200), true), Pulse::default());
    }

    #[test]
    fn the_streak_is_drawn_around_its_line_and_glow() {
        let streak = Streak {
            from: (1700.0, 200.0),
            head: (500.0, 600.0),
            alpha: 1.0,
            colour: colour("done"),
        };
        // On a second screen right of a first 1920 wide.
        let area = streak_area(&streak, Point::from((1920.0, 0.0)));
        assert_eq!(area.loc, (-1420 - 28, 200 - 28).into());
        assert_eq!(area.size, (1200 + 56, 400 + 56).into());
    }
}
