//! Alt+Tab's switcher and a window's radial menu, while they show and
//! as they go.

use smithay::{input::keyboard::Keycode, reexports::calloop::RegistrationToken};

use crate::{motion::Tween, switcher::Switcher};

#[derive(Debug, Default)]
pub struct Switching {
    /// Alt+Tab, while it is held, and its panel vanishing (§11).
    pub open: Option<Switcher>,
    pub out: Option<(Switcher, Tween)>,
    /// The switcher's key held down, stepping at the keyboard's repeat
    /// delay and rate: its keycode and its timer (§11).
    pub repeat: Option<(Keycode, RegistrationToken)>,
}

#[derive(Debug, Default)]
pub struct RadialMenu {
    /// A window's radial menu while it is open, and as it goes (#723);
    /// the right press that may open it; what draws it.
    pub open: Option<crate::radial::Radial>,
    pub out: Option<crate::radial::Going>,
    pub press: Option<crate::radial::Press>,
    pub paint: crate::radial_draw::RadialPaint,
}
