//! Mirroring (#931, docs/design/windows.md §12 "Mirroring"): one screen,
//! the *shown* one, has its picture on every other screen on, the
//! *mirrors*.
//!
//! - **A mirror is no screen for the desktop**: it leaves the desktop as
//!   an unplugged screen does (its windows to the shown screen, onto the
//!   same desktop number; its layer surfaces closed; its `wl_output`
//!   global removed), and keeps lighting, its picture the shown screen's.
//!   When mirroring stops it comes back as a screen of the same identity
//!   plugged back: at its place, its windows flying back, with a new
//!   global.
//! - **Its picture** ([`crate::magnifier::draw`]): the shown screen's
//!   scene drawn into a picture of that screen's size, kept between
//!   frames with damage tracking of its own, then that picture fitted on
//!   the mirror, its proportions kept, black bands around it. A mirror
//!   draws when the shown screen draws ([`crate::redraw::Commits`]), only
//!   what changed; nothing at rest.
//! - **Kept per set of screens**, in the compositor's state file
//!   ([`crate::screen_sets`]): the shown screen's identity, or nothing
//!   (extended). Plugged again, a set takes its choice back in the same
//!   turn as its screens light. The shown screen going (the lid, an
//!   unplug) stops it for now; the set coming back brings it back.
//! - **A set never seen** lights extended, and the winking panda offers
//!   « Recopier » (`State::screens_new_peek`).

use std::{
    collections::BTreeMap,
    sync::{Mutex, PoisonError},
};

use hocus_link::{MirrorScreen, MirrorShown, ScreensNew, ToShell};
use smithay::{
    desktop::layer_map_for_output,
    output::Output,
    reexports::wayland_server::backend::GlobalId,
    utils::{Physical, Rectangle, Size},
};

use crate::State;

/// A screen's `wl_output` global now, kept on its `Output`: mirroring
/// retires it, and makes another when the screen comes back.
#[derive(Debug, Default)]
struct GlobalSlot(Mutex<Option<GlobalId>>);

/// `global` is `output`'s `wl_output` global now (the backend made it).
pub fn keep_global(output: &Output, global: GlobalId) {
    output
        .user_data()
        .insert_if_missing_threadsafe(GlobalSlot::default);
    if let Some(slot) = output.user_data().get::<GlobalSlot>() {
        *slot.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(global);
    }
}

/// `output`'s global now, taken: `None` while it is a mirror (retired
/// already); `or` for an output that never kept one.
pub(crate) fn take_global(output: &Output, or: Option<GlobalId>) -> Option<GlobalId> {
    match output.user_data().get::<GlobalSlot>() {
        Some(slot) => slot.0.lock().unwrap_or_else(PoisonError::into_inner).take(),
        None => or,
    }
}

/// Mirroring now: the screen shown, and the mirrors.
#[derive(Debug, Clone)]
struct Now {
    shown: Output,
    mirrors: Vec<Output>,
}

/// Mirroring's state, on the loop.
#[derive(Debug, Default)]
pub struct Mirror {
    now: Option<Now>,
    /// Each mirror's place before it mirrored, by connector: where it
    /// comes back.
    places: BTreeMap<String, (i32, i32)>,
    /// The screen plugged last: the one a new set's offer names.
    joined: Option<String>,
}

/// A screen's diagonal in inches, from its millimetres; 0 unknown.
pub(crate) fn inches_of(output: &Output) -> u32 {
    let mm = output
        .user_data()
        .get::<crate::displays::Facts>()
        .map_or((0, 0), |f| f.millimetres);
    if mm.0 > 0 && mm.1 > 0 {
        ((f64::from(mm.0).powi(2) + f64::from(mm.1).powi(2)).sqrt() / 25.4).round() as u32
    } else {
        0
    }
}

fn named(output: &Output) -> MirrorScreen {
    MirrorScreen {
        output: output.name(),
        built_in: crate::away::is_internal(output),
        inches: inches_of(output),
    }
}

impl State {
    /// Whether the screens mirror now.
    pub fn mirroring(&self) -> bool {
        self.mirror.now.is_some()
    }

    /// The screen shown, while mirroring.
    pub fn mirror_shown(&self) -> Option<&Output> {
        self.mirror.now.as_ref().map(|n| &n.shown)
    }

    /// The screen whose picture `output` shows, if it is a mirror.
    pub fn mirror_source(&self, output: &Output) -> Option<Output> {
        let now = self.mirror.now.as_ref()?;
        now.mirrors.contains(output).then(|| now.shown.clone())
    }

    /// Whether `output` is a mirror now.
    pub fn is_mirror(&self, output: &Output) -> bool {
        self.mirror
            .now
            .as_ref()
            .is_some_and(|n| n.mirrors.contains(output))
    }

    /// The mirror of that connector's name, if there is one.
    pub(crate) fn mirror_named(&self, name: &str) -> Option<&Output> {
        self.mirror
            .now
            .as_ref()?
            .mirrors
            .iter()
            .find(|o| o.name() == name)
    }

    /// Where mirror `name` comes back, as the Settings draw it.
    pub(crate) fn mirror_place(&self, name: &str) -> Option<(i32, i32)> {
        self.mirror.places.get(name).copied()
    }

    /// What the shell is told of mirroring now.
    pub(crate) fn mirror_said(&self) -> Option<MirrorShown> {
        let now = self.mirror.now.as_ref()?;
        Some(MirrorShown {
            shown: named(&now.shown),
            mirrors: now.mirrors.iter().map(named).collect(),
        })
    }

    /// The screen shown by default (spec: the built-in panel when it is
    /// on, else the primary screen).
    fn mirror_default_shown(&self) -> Option<Output> {
        self.space
            .outputs()
            .find(|o| crate::away::is_internal(o))
            .or_else(|| {
                let primary = self.panel_primary()?;
                self.space.outputs().find(|o| o.name() == primary)
            })
            .or_else(|| self.space.outputs().next())
            .cloned()
    }

    /// `Super+Shift+P`, the display key, the palette's action: the screens
    /// mirror, or extend again, at once; nothing with one screen.
    pub(crate) fn mirror_toggle(&mut self) {
        self.mirror_asked(!self.mirroring());
    }

    /// Mirroring asked for (`on`), or extended again: by the shortcut, the
    /// new screens' bubble, the sign. The set keeps that choice.
    pub(crate) fn mirror_asked(&mut self, on: bool) {
        if on == self.mirroring() {
            return;
        }
        if on {
            if self.space.outputs().count() < 2 {
                tracing::debug!("one screen: nothing to mirror");
                return;
            }
            let Some(shown) = self.mirror_default_shown() else {
                return;
            };
            let key = self.screen_key(&shown.name());
            self.peek_screens_new_answered();
            self.keep_mirror(Some(key));
        } else {
            self.keep_mirror(None);
        }
        self.mirror_follow_set();
    }

    /// The Settings' line (6.9): `mirror IDENTITY` shows that screen on
    /// the others; `None` (`extend`), each screen its own again.
    pub(crate) fn mirror_from_settings(&mut self, shown: Option<&str>) -> Result<(), String> {
        match shown {
            Some(key) => {
                let output = self
                    .connector_of(key)
                    .and_then(|name| {
                        self.displays
                            .outputs
                            .iter()
                            .find(|o| o.name() == name)
                            .cloned()
                    })
                    .ok_or_else(|| format!("no screen {key}"))?;
                if self.current_set().len() < 2 {
                    return Err("one screen".to_owned());
                }
                if !self
                    .current_set()
                    .contains(&self.screen_key(&output.name()))
                {
                    return Err(format!("{key} is off"));
                }
                self.keep_mirror(Some(key.to_owned()));
            }
            None => self.keep_mirror(None),
        }
        self.mirror_follow_set();
        Ok(())
    }

    /// The screens as the set in use keeps them: mirrored on its shown
    /// screen while that one is on, else each its own. Called as the set
    /// changes (a screen plugged or unplugged, the lid) and as its choice
    /// does; nothing when they already are.
    pub(crate) fn mirror_follow_set(&mut self) {
        let set = self.current_set();
        let want = self
            .kept_mirror(&set)
            .filter(|key| set.len() >= 2 && set.contains(key))
            .and_then(|key| self.connector_of(&key))
            .and_then(|name| {
                self.displays
                    .outputs
                    .iter()
                    .find(|o| o.name() == name)
                    .cloned()
            });
        let now = self.mirror.now.as_ref().map(|n| n.shown.clone());
        match (now, want) {
            (None, None) => {}
            (Some(shown), Some(want)) if shown == want => {
                // A screen joined the set that mirrors: it mirrors too.
                if self.space.outputs().any(|o| *o != shown) {
                    self.mirror_start(&shown);
                }
            }
            (Some(_), want) => {
                self.mirror_stop();
                if let Some(want) = want {
                    self.mirror_start(&want);
                }
            }
            (None, Some(want)) => self.mirror_start(&want),
        }
    }

    /// `output` is unplugged: if it is the screen shown, mirroring stops
    /// for now, the others screens again to take its windows; the set
    /// keeps its choice for when it comes back.
    pub(crate) fn mirror_unplugged(&mut self, output: &Output) {
        if self.mirror_shown() == Some(output) {
            tracing::info!(
                output = output.name(),
                "the screen shown went: mirroring stops for now"
            );
            self.mirror_stop();
        }
    }

    /// The fitted picture as a test reads it: the mirror, the screen it
    /// shows, and where on the mirror that picture is (pixels).
    pub fn mirror_fitted(&self, mirror: &str) -> Option<(String, Rectangle<i32, Physical>)> {
        let now = self.mirror.now.as_ref()?;
        let output = now.mirrors.iter().find(|o| o.name() == mirror)?;
        let size = |o: &Output| {
            o.current_mode()
                .map(|m| o.current_transform().transform_size(m.size))
        };
        let (_, fitted) = fit(size(&now.shown)?, size(output)?);
        Some((now.shown.name(), fitted))
    }

    /// Every other screen on shows `shown`'s picture: each leaves the
    /// desktop as an unplugged screen does, its global removed.
    fn mirror_start(&mut self, shown: &Output) {
        let mirrors: Vec<Output> = self
            .space
            .outputs()
            .filter(|o| *o != shown)
            .cloned()
            .collect();
        if mirrors.is_empty() {
            return;
        }
        for output in &mirrors {
            let name = output.name();
            tracing::info!(
                shown = shown.name(),
                mirror = name,
                "mirroring: the screen shows another's picture"
            );
            if let Some(g) = self.space.output_geometry(output) {
                self.mirror.places.insert(name.clone(), (g.loc.x, g.loc.y));
            }
            // Nothing said: the user did it.
            self.screen_turned_off(output, false);
            if let Some(global) = take_global(output, None) {
                self.retire_output_global(global);
            }
        }
        match &mut self.mirror.now {
            Some(now) if now.shown == *shown => now.mirrors.extend(mirrors),
            now => {
                *now = Some(Now {
                    shown: shown.clone(),
                    mirrors,
                })
            }
        }
        self.mirror_changed();
    }

    /// Each mirror still plugged comes back as a screen of the same
    /// identity plugged back: at its place, its windows flying back.
    fn mirror_stop(&mut self) {
        let Some(now) = self.mirror.now.take() else {
            return;
        };
        tracing::info!(shown = now.shown.name(), "mirroring stops");
        for output in now.mirrors {
            let name = output.name();
            let place = self.mirror.places.remove(&name);
            if !self.displays.outputs.contains(&output) {
                // Unplugged meanwhile: nothing comes back.
                continue;
            }
            let global = output.create_global::<State>(&self.display_handle);
            keep_global(&output, global);
            let place = place.unwrap_or_else(|| {
                let right = self
                    .space
                    .outputs()
                    .filter_map(|o| self.space.output_geometry(o))
                    .map(|g| g.loc.x + g.size.w)
                    .max()
                    .unwrap_or(0);
                (right, 0)
            });
            output.change_current_state(None, None, None, Some(place.into()));
            self.space.map_output(&output, place);
            layer_map_for_output(&output).arrange();
            self.panel_refresh();
            self.output_back(&output, false);
        }
        self.mirror_changed();
    }

    /// After mirroring started or stopped: the screens laid out again, the
    /// mirrors drawn as their shown screen is, the shell and the Settings
    /// told.
    fn mirror_changed(&mut self) {
        let follows = self
            .mirror
            .now
            .as_ref()
            .map(|n| {
                n.mirrors
                    .iter()
                    .map(|m| (n.shown.clone(), m.clone()))
                    .collect()
            })
            .unwrap_or_default();
        self.commits.set_follows(follows);
        self.share_sources_changed();
        self.tell_shell(ToShell::Mirror(self.mirror_said()));
        // The bubbles' screen right after: a notification coming at once
        // shows on a screen of the desktop, never on a mirror.
        self.flush_bubbles_screen();
        self.displays.wanted = true;
        self.displays.dirty = true;
        self.redraw = true;
    }

    /// The screens make a set never seen, of two screens at least: the
    /// winking panda offers to mirror them, naming the one plugged last
    /// (a screen not the laptop's own, else any).
    pub(crate) fn mirror_offer(&mut self) {
        if self.mirroring() {
            return;
        }
        let set = self.current_set();
        let on: Vec<Output> = self
            .displays
            .outputs
            .iter()
            .filter(|o| set.contains(&self.screen_key(&o.name())))
            .cloned()
            .collect();
        let joined = self.mirror.joined.clone();
        let Some(named) = on
            .iter()
            .find(|o| joined.as_deref() == Some(o.name().as_str()) && !crate::away::is_internal(o))
            .or_else(|| on.iter().rev().find(|o| !crate::away::is_internal(o)))
            .or_else(|| on.last())
        else {
            return;
        };
        self.screens_new_peek(ScreensNew {
            built_in: crate::away::is_internal(named),
            inches: inches_of(named),
        });
    }

    /// Output `name` was plugged: the one a new set's offer names.
    pub(crate) fn mirror_joined(&mut self, name: String) {
        self.mirror.joined = Some(name);
    }
}

/// `shown`'s picture fitted on a mirror of `target` (pixels): how much it
/// is scaled, and where it is, centred, its proportions kept.
pub fn fit(
    shown: Size<i32, Physical>,
    target: Size<i32, Physical>,
) -> (f64, Rectangle<i32, Physical>) {
    if shown.w <= 0 || shown.h <= 0 {
        return (1.0, Rectangle::from_size(target));
    }
    let factor =
        (f64::from(target.w) / f64::from(shown.w)).min(f64::from(target.h) / f64::from(shown.h));
    let w = ((f64::from(shown.w) * factor).round() as i32).min(target.w);
    let h = ((f64::from(shown.h) * factor).round() as i32).min(target.h);
    let x = (target.w - w) / 2;
    let y = (target.h - h) / 2;
    (factor, Rectangle::new((x, y).into(), (w, h).into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_fitted_with_bands_its_proportions_kept() {
        // The laptop (2560 × 1600) on a TV (1920 × 1080): bands left and
        // right; the TV on the laptop: above and below.
        let (factor, at) = fit((2560, 1600).into(), (1920, 1080).into());
        assert!((factor - 0.675).abs() < 1e-9);
        assert_eq!(at, Rectangle::new((96, 0).into(), (1728, 1080).into()));
        let (_, at) = fit((1920, 1080).into(), (2560, 1600).into());
        assert_eq!(at, Rectangle::new((0, 80).into(), (2560, 1440).into()));
        let (_, at) = fit((1920, 1080).into(), (1920, 1080).into());
        assert_eq!(at, Rectangle::new((0, 0).into(), (1920, 1080).into()));
    }
}
