//! The screen capture's toolbar (5.8, docs/design/screenshots.md): the
//! compositor owns the capture and draws the rest of it (the darkening,
//! the area and its handles, the countdown, the flash); the shell draws
//! the toolbar at the bottom of the screen under the pointer, as the
//! compositor says what it shows (`capture`), and tells it what is
//! clicked (`capture mode|delay|pointer|shoot`). Then the toast, the
//! launcher's, says what was taken (`captured`), a placeholder until the
//! capture's own preview (5.9).
//!
//! As for the launcher, nothing goes through iced's messages: the
//! toolbar's state is here, the pointer changes it and draws its surface
//! alone.

pub mod draw;

use std::{
    sync::{Mutex, PoisonError},
    time::Instant,
};

use hocus_link::{CAPTURE_TOOLBAR_NAMESPACE, CaptureAsk, CaptureBar, CaptureMode, Captured};
use iced::window;

use crate::{clock::time::Lang, popover::Effect};

/// The toolbar's words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self {
            fr: lang == Lang::French,
        }
    }

    fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        if self.fr { fr } else { en }
    }

    pub fn mode(self, mode: CaptureMode) -> &'static str {
        match mode {
            CaptureMode::Area => self.pick("Zone", "Area"),
            CaptureMode::Window => self.pick("Fenêtre", "Window"),
            CaptureMode::Screen => self.pick("Écran", "Screen"),
            CaptureMode::All => self.pick("Tous les écrans", "All screens"),
        }
    }

    pub fn delay(self, seconds: u8) -> String {
        if seconds == 0 {
            self.pick("Tout de suite", "Now").to_owned()
        } else {
            format!("{seconds}\u{a0}s")
        }
    }

    pub fn shoot(self) -> &'static str {
        self.pick("Capturer", "Capture")
    }

    /// The toast once taken: what, copied, and where it was saved.
    pub fn captured(self, captured: &Captured) -> String {
        let saved = !captured.folder.is_empty();
        let folder = &captured.folder;
        if self.fr {
            // Each its agreement: une zone, une fenêtre, un écran.
            let (what, copied, kept) = match captured.mode {
                CaptureMode::Area => (
                    format!("Zone de {} × {}", captured.width, captured.height),
                    "copiée",
                    "enregistrée",
                ),
                CaptureMode::Window => ("Fenêtre".to_owned(), "copiée", "enregistrée"),
                CaptureMode::Screen => ("Écran".to_owned(), "copié", "enregistré"),
                CaptureMode::All => ("Tous les écrans".to_owned(), "copiés", "enregistrés"),
            };
            if saved {
                format!("{what} {copied} · {kept} dans {folder}")
            } else {
                format!("{what} {copied}")
            }
        } else {
            let what = match captured.mode {
                CaptureMode::Area => format!("Area of {} × {}", captured.width, captured.height),
                CaptureMode::Window => "Window".to_owned(),
                CaptureMode::Screen => "Screen".to_owned(),
                CaptureMode::All => "All screens".to_owned(),
            };
            if saved {
                format!("{what} copied · saved to {folder}")
            } else {
                format!("{what} copied")
            }
        }
    }
}

fn words() -> Words {
    Words::new(Lang::of_messages())
}

/// A button of the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Mode(CaptureMode),
    Delay,
    Pointer,
    Shoot,
}

impl Button {
    fn ask(self) -> CaptureAsk {
        match self {
            Self::Mode(mode) => CaptureAsk::Mode(mode),
            Self::Delay => CaptureAsk::Delay,
            Self::Pointer => CaptureAsk::Pointer,
            Self::Shoot => CaptureAsk::Shoot,
        }
    }
}

/// The toolbar shown.
#[derive(Debug, Clone)]
pub struct Toolbar {
    pub id: window::Id,
    pub bar: CaptureBar,
    /// The button under the pointer, and the one pressed.
    pub hover: Option<Button>,
    pub pressed: Option<Button>,
}

static TOOLBAR: Mutex<Option<Toolbar>> = Mutex::new(None);

/// `with` on the toolbar, if one shows.
pub fn with<T>(with: impl FnOnce(&mut Option<Toolbar>) -> T) -> T {
    with(&mut TOOLBAR.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Whether surface `id` is the toolbar's.
pub fn is_surface(id: window::Id) -> bool {
    with(|t| t.as_ref().is_some_and(|t| t.id == id))
}

/// The compositor's `capture` line: the toolbar opens on its screen, or
/// shows what changed.
pub fn set(bar: CaptureBar) {
    let effects = with(|toolbar| match toolbar {
        Some(shown) if shown.bar.output == bar.output => {
            shown.bar = bar;
            vec![Effect::Wake(CAPTURE_TOOLBAR_NAMESPACE)]
        }
        _ => {
            let mut effects = Vec::new();
            if let Some(old) = toolbar.take() {
                effects.push(Effect::Remove(old.id));
            }
            let id = window::Id::unique();
            let size = draw::Layout::of(words()).size;
            tracing::debug!(output = bar.output, "the capture's toolbar opens");
            effects.push(Effect::CaptureBar {
                id,
                output: bar.output.clone(),
                size: (size.width.ceil() as u32, size.height.ceil() as u32),
            });
            *toolbar = Some(Toolbar {
                id,
                bar,
                hover: None,
                pressed: None,
            });
            effects
        }
    });
    crate::popover::send(effects);
}

/// `capture-end`: the toolbar goes.
pub fn end() {
    let effects = with(|toolbar| {
        toolbar
            .take()
            .map(|t| vec![Effect::Remove(t.id)])
            .unwrap_or_default()
    });
    if !effects.is_empty() {
        tracing::debug!("the capture's toolbar goes");
        crate::popover::send(effects);
    }
}

/// `captured`: the launcher's toast says what was taken, on its screen.
pub fn captured(captured: Captured) {
    let said = words().captured(&captured);
    crate::popover::send(crate::launcher::toast::show(
        captured.output,
        said,
        Instant::now(),
    ));
}

/// `button` clicked on the toolbar on `id`: the compositor is told.
fn clicked(id: window::Id, button: Button) {
    if !is_surface(id) {
        return;
    }
    tracing::debug!(?button, "the capture's toolbar is clicked");
    crate::popover::send(vec![Effect::Tell(hocus_link::ToCompositor::Capture(
        button.ask(),
    ))]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured(mode: CaptureMode, folder: &str) -> Captured {
        Captured {
            output: "eDP-1".into(),
            mode,
            width: 1200,
            height: 760,
            folder: folder.into(),
        }
    }

    #[test]
    fn the_toast_says_what_was_taken_and_where() {
        let fr = Words::new(Lang::French);
        let en = Words::new(Lang::English);
        let folder = "Images/Captures d'écran";
        assert_eq!(
            fr.captured(&captured(CaptureMode::Area, folder)),
            "Zone de 1200 × 760 copiée · enregistrée dans Images/Captures d'écran"
        );
        assert_eq!(
            fr.captured(&captured(CaptureMode::Screen, folder)),
            "Écran copié · enregistré dans Images/Captures d'écran"
        );
        assert_eq!(
            en.captured(&captured(CaptureMode::All, "Pictures/Screenshots")),
            "All screens copied · saved to Pictures/Screenshots"
        );
        // Not saved: only copied.
        assert_eq!(
            en.captured(&captured(CaptureMode::Window, "")),
            "Window copied"
        );
        assert_eq!(fr.delay(0), "Tout de suite");
        assert_eq!(en.delay(10), "10\u{a0}s");
    }
}
