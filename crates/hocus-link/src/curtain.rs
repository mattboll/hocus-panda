//! The curtain before the lock (7.2, variant A, docs/design/idle.md): when
//! the session has been idle for its delay, every screen dims over
//! [`CURTAIN`]; on the screen under the pointer, a pill says the session
//! is about to lock, a panda falling asleep in it. Any input lifts it.
//!
//! The compositor draws the dim, the pill's shadow and its motion (it
//! fades and rises in, fades out with the dim when lifted); the shell
//! draws the pill itself, on a layer surface of [`CURTAIN_NAMESPACE`] on
//! the screen it is told.
//!
//! - `curtain ELAPSED LENGTH TOP motion|still OUTPUT` ([`CurtainShown`]):
//!   the curtain is up since `ELAPSED` ms (0 as it starts, more when the
//!   shell connects late), for `LENGTH` ms; the pill's top is `TOP`
//!   logical pixels from the top of screen `OUTPUT`.
//! - `curtain-end`: it was lifted, or it ended (locked, or the screens
//!   went off): the pill's surface goes.

use std::{fmt, time::Duration};

use crate::{BadLine, flag, word};

/// The pill's surface: the pill alone, 300 × 64, centred across its
/// screen, in the overlay layer, taking neither the pointer nor the keys.
pub const CURTAIN_NAMESPACE: &str = "hocus-curtain";
/// How long the curtain dims before the session locks.
pub const CURTAIN: Duration = Duration::from_secs(10);
/// What dims the screens, at the curtain's end: `rgba(4, 6, 10, 0.74)`,
/// reached along `p^1.15`.
pub const CURTAIN_DIM: [u8; 3] = [4, 6, 10];
pub const CURTAIN_DIM_ALPHA: f32 = 0.74;
pub const CURTAIN_DIM_POWER: f64 = 1.15;
/// Lifted, the dim and the pill fade out over this (`ease-in`).
pub const CURTAIN_LIFT: Duration = Duration::from_millis(150);
/// The curtain draws at most once in this: the dim and the countdown.
pub const CURTAIN_FRAME: Duration = Duration::from_millis(100);
/// The pill, logical pixels; its top at [`PILL_TOP`] of its screen's
/// height.
pub const PILL_WIDTH: f64 = 300.0;
pub const PILL_HEIGHT: f64 = 80.0;
pub const PILL_RADIUS: f64 = 20.0;
pub const PILL_TOP: f64 = 0.64;
/// It comes in fading and rising [`PILL_RISE`] px over [`PILL_IN`]
/// (out-cubic); with reduced motion, it shows at once.
pub const PILL_IN: Duration = Duration::from_millis(250);
pub const PILL_RISE: f64 = 10.0;

/// The curtain as the shell draws its pill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurtainShown {
    /// How long it has been up, in milliseconds.
    pub elapsed: u32,
    /// How long it lasts, in milliseconds.
    pub length: u32,
    /// The pill's top from its screen's top, logical pixels.
    pub top: i32,
    /// Things move (no reduced motion): the panda's head tilts.
    pub motion: bool,
    /// The screen the pill shows on.
    pub output: String,
}

impl CurtainShown {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "curtain {} {} {} {} {}",
            self.elapsed,
            self.length,
            self.top,
            if self.motion { "motion" } else { "still" },
            crate::screen_name(&self.output),
        )
    }

    pub(crate) fn read<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<Self, BadLine> {
        let number = |w: &str, what: &str| {
            w.parse::<u32>()
                .map_err(|e| BadLine(format!("{what}: {e}")))
        };
        let elapsed = number(word(words, "elapsed")?, "elapsed")?;
        let length = number(word(words, "length")?, "length")?;
        let top = word(words, "top")?
            .parse::<i32>()
            .map_err(|e| BadLine(format!("top: {e}")))?;
        let motion = flag(words.next(), "motion", "still")?;
        let output = word(words, "output")?.to_owned();
        if words.next().is_some() {
            return Err(BadLine("a curtain line too long".to_owned()));
        }
        Ok(Self {
            elapsed,
            length,
            top,
            motion,
            output,
        })
    }
}
