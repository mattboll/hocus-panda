//! The launcher (5.1): the shell's full-screen layer surface, namespace
//! [`LAUNCHER_NAMESPACE`], over one screen while it is open.
//!
//! `Super+Space` tells the shell to open it on the screen under the
//! pointer, or to close it ([`ToShell::Launcher`]). The shell draws its
//! field and its grid and takes the keyboard; the compositor draws what is
//! under and around them, as it does for the bar and its popovers: the
//! desktop behind it blurred (with glass) and darkened, the bar too, and
//! its motion: it fades in over 160 ms from its first frame, its panel
//! growing from 0.97 of its size, and back out over 120 ms once the shell
//! says it closes (`closing`), the shell removing it after that. With
//! reduced motion it only fades. The shell draws it once for that motion.
//!
//! An application the shell asks for ([`ToCompositor::Launch`]) starts as
//! the compositor's other programs do, on a thread of its own, its first
//! window taking the focus on the current desktop.
//!
//! [`ToCompositor::Launch`]: hocus_link::ToCompositor::Launch

use std::{cell::RefCell, collections::HashMap, time::Duration};

use hocus_link::{
    LAUNCHER_CLOSE, LAUNCHER_NAMESPACE, LAUNCHER_OPEN, LAUNCHER_SCALE, Launch, ToShell,
};
use smithay::{
    backend::renderer::{element::solid::SolidColorBuffer, utils::with_renderer_surface_state},
    desktop::{LayerSurface, layer_map_for_output},
    output::Output,
    utils::{Logical, Size},
};

use crate::{
    State,
    motion::{Bezier, Tween},
    programs,
};

/// The mockup's `cubic-bezier(.2, .8, .2, 1)`, the panel growing in.
const GROW: Bezier = Bezier(0.2, 0.8, 0.2, 1.0);

/// The launchers shown (one, but one closing may still fade out as
/// another opens), with their motion.
#[derive(Debug, Default)]
pub struct Launchers {
    shown: Vec<Shown>,
    /// What darkens each screen behind it, by screen: the same pixels
    /// from one frame to the next, for the damage.
    dims: RefCell<HashMap<String, SolidColorBuffer>>,
}

#[derive(Debug)]
struct Shown {
    layer: LayerSurface,
    /// Its first frame, on the compositor's clock.
    opened: Duration,
    /// Since when it closes.
    closing: Option<Duration>,
}

/// How a launcher is drawn at a moment: how opaque it and what is behind
/// it are, and how large its panel is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub alpha: f32,
    pub scale: f64,
}

/// How a launcher opened at `opened` (and closing since `closing`) is drawn
/// at `now`; `motion`: its panel grows and shrinks, else it only fades.
fn look(opened: Duration, closing: Option<Duration>, now: Duration, motion: bool) -> Look {
    let (alpha, grown) = match closing {
        None => (
            Tween::new(opened, LAUNCHER_OPEN, Bezier::EASE_OUT).at(now),
            Tween::new(opened, LAUNCHER_OPEN, GROW).at(now),
        ),
        Some(since) => {
            let gone = Tween::new(since, LAUNCHER_CLOSE, Bezier::EASE_IN).at(now);
            (1.0 - gone, 1.0 - gone)
        }
    };
    let scale = if motion {
        LAUNCHER_SCALE + (1.0 - LAUNCHER_SCALE) * grown.clamp(0.0, 1.0)
    } else {
        1.0
    };
    Look {
        alpha: alpha.clamp(0.0, 1.0) as f32,
        scale,
    }
}

impl Shown {
    fn moving(&self, now: Duration) -> bool {
        match self.closing {
            Some(since) => now < since + LAUNCHER_CLOSE,
            None => now < self.opened + LAUNCHER_OPEN,
        }
    }
}

impl Launchers {
    /// The launcher `layer`, if it shows, and how.
    pub fn look(&self, layer: &LayerSurface, now: Duration, motion: bool) -> Option<Look> {
        let shown = self.shown.iter().find(|s| &s.layer == layer)?;
        Some(look(shown.opened, shown.closing, now, motion))
    }

    /// Whether one is still moving at `now`.
    pub fn tick(&self, now: Duration) -> bool {
        self.shown.iter().any(|s| s.moving(now))
    }

    /// Whether one shows and is not closing.
    pub fn open(&self) -> bool {
        self.shown.iter().any(|s| s.closing.is_none())
    }

    /// What darkens screen `output`, `size` large, behind it.
    pub fn dim(&self, output: &str, size: Size<i32, Logical>) -> SolidColorBuffer {
        let ([r, g, b], a) = hocus_link::LAUNCHER_DIM;
        let channel = |c: u8| f32::from(c) / 255.0 * a;
        // Premultiplied, as the renderers blend.
        let colour = [channel(r), channel(g), channel(b), a];
        let mut dims = self.dims.borrow_mut();
        let dim = dims
            .entry(output.to_owned())
            .or_insert_with(|| SolidColorBuffer::new(size, colour));
        dim.update(size, colour);
        dim.clone()
    }
}

impl State {
    /// Whether the launcher's panel grows and shrinks: not with reduced
    /// motion, where it only fades.
    pub fn launcher_motion(&self) -> bool {
        !self.reduced_motion()
    }

    /// `Super+Space`: the shell opens the launcher on the screen under the
    /// pointer, or closes it.
    pub fn toggle_launcher(&mut self) {
        let Some(output) = self.output_under_pointer() else {
            return;
        };
        if !self.shell_linked() {
            tracing::info!("no shell: no launcher");
            return;
        }
        let scale = self.bar_scale(&output.name());
        self.tell_shell(ToShell::Launcher {
            output: output.name(),
            scale,
        });
    }

    /// A commit of the shell's launcher `layer`: its first frame starts its
    /// motion.
    pub fn launcher_committed(&mut self, layer: &LayerSurface) {
        let launchers = &mut self.panel.launchers;
        if launchers.shown.iter().any(|s| &s.layer == layer) {
            return;
        }
        let drawn = with_renderer_surface_state(layer.wl_surface(), |s| s.buffer().is_some())
            .unwrap_or(false);
        if !drawn {
            return;
        }
        let opened = self.now();
        self.panel.launchers.shown.push(Shown {
            layer: layer.clone(),
            opened,
            closing: None,
        });
        self.redraw = true;
    }

    /// The shell says its launcher closes: it fades out.
    pub fn launcher_closing(&mut self, namespace: &str) {
        if namespace != LAUNCHER_NAMESPACE {
            return;
        }
        let now = self.now();
        for shown in &mut self.panel.launchers.shown {
            shown.closing.get_or_insert(now);
        }
        self.redraw = true;
    }

    /// A layer surface went: if it was a launcher, its motion goes too.
    pub fn launcher_gone(&mut self, layer: &LayerSurface) {
        self.panel.launchers.shown.retain(|s| &s.layer != layer);
    }

    /// The launcher's surface on `output`, open or closing, if one shows.
    pub fn launcher_on(&self, output: &Output) -> Option<LayerSurface> {
        layer_map_for_output(output)
            .layers()
            .find(|l| l.namespace() == LAUNCHER_NAMESPACE)
            .cloned()
    }

    /// The shell asks for an application: started off the loop, its
    /// first window taking the focus.
    pub fn launch(&mut self, launch: Launch) {
        tracing::info!(
            app = launch.id,
            terminal = launch.terminal,
            "the launcher starts"
        );
        self.spawned_at = Some(self.now());
        let env = programs::Env::of(self).with_activation(self.activation_token());
        let terminal = launch
            .terminal
            .then(|| self.config.programs.terminal.clone());
        programs::launch(launch, terminal, env);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(opened: u64, closing: Option<u64>, now: u64, motion: bool) -> Look {
        let ms = Duration::from_millis;
        look(ms(opened), closing.map(ms), ms(now), motion)
    }

    #[test]
    fn it_fades_and_grows_in_then_fades_and_shrinks_out() {
        // At its first frame: transparent, at 0.97.
        let start = at(1000, None, 1000, true);
        assert_eq!(start.alpha, 0.0);
        assert!((start.scale - LAUNCHER_SCALE).abs() < 1e-9);
        // Halfway, mostly there; at 160 ms, whole.
        let mid = at(1000, None, 1080, true);
        assert!(mid.alpha > 0.6 && mid.alpha < 1.0, "{mid:?}");
        assert!(mid.scale > 0.99 && mid.scale < 1.0, "{mid:?}");
        assert_eq!(
            at(1000, None, 1160, true),
            Look {
                alpha: 1.0,
                scale: 1.0
            }
        );
        // Closing, ease-in: little gone at first, gone at 120 ms.
        let early = at(1000, Some(2000), 2030, true);
        assert!(early.alpha > 0.8 && early.scale > 0.99, "{early:?}");
        let gone = at(1000, Some(2000), 2120, true);
        assert_eq!(gone.alpha, 0.0);
        assert!((gone.scale - LAUNCHER_SCALE).abs() < 1e-9);
    }

    #[test]
    fn with_reduced_motion_it_only_fades() {
        let mid = at(1000, None, 1080, false);
        assert!(mid.alpha > 0.6 && mid.alpha < 1.0, "{mid:?}");
        assert_eq!(mid.scale, 1.0);
        let out = at(1000, Some(2000), 2060, false);
        assert!(out.alpha > 0.0 && out.alpha < 1.0, "{out:?}");
        assert_eq!(out.scale, 1.0);
    }
}
