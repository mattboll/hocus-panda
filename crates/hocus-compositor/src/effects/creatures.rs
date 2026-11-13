//! The agents as creatures on their windows (#796, docs/design/effects.md):
//! each agent's panda, the bar's own drawing, sits on the top edge of its
//! agent's window and plays its state, as the board "Les agents en
//! créatures" does (`hocus_pandas::creature` its motion, [`super::rig`]
//! its drawing in pieces, `crate::render::creatures` its drawing).
//!
//! Where they sit, decided as the agents and the windows change:
//!
//! - one agent in a window: its panda on the top edge's right corner,
//!   large;
//! - herdr's: one panda over each pane of the tab herdr shows that an
//!   agent runs in (herdr says where its panes are across its terminal);
//!   herdr's other tabs' agents stay in the bar;
//! - several agents in a window (a terminal's tabs: the desktop knows the
//!   window, not the tab): the shown tab's large on the left corner, the
//!   others small beside it with a name tag. The shown one is the one the
//!   window's title names; else the one that started working last (the
//!   user just wrote to it); else the one whose state changed last;
//! - a maximised or fullscreen window, or one with no room above it, has
//!   no free edge: none there, the pandas stay in the bar.
//!
//! Off by default (`[effects] creatures`). With reduced motion they sit
//! still; in the power saver, while a screen is shared or recorded and on
//! the CPU there are none; never in a capture.

use std::{cell::RefCell, collections::BTreeMap, collections::HashMap, rc::Rc, time::Duration};

use hocus_link::AgentShown;
use hocus_pandas::Mood;
use smithay::backend::{
    allocator::Fourcc,
    renderer::{element::Id, element::memory::MemoryRenderBuffer, utils::CommitCounter},
};

use super::{Still, rig::Rig};
use crate::{
    State,
    model::{Mode, WindowId},
    overview::AgentState,
};

/// A panda's width, logical: alone on its window, the shown tab's among
/// others, over a herdr pane, another tab's (the board's).
pub const LARGE: f64 = 92.0;
pub const SHOWN: f64 = 84.0;
pub const PANE: f64 = 72.0;
pub const SMALL: f64 = 52.0;
/// Alone, its right side this far in from the window's.
const RIGHT_IN: f64 = 12.0;
/// Among others, the shown one's left side this far in from the window's,
/// the others after it, this far apart.
const LEFT_IN: f64 = 18.0;
const AFTER_SHOWN: f64 = 28.0;
const BETWEEN: f64 = 12.0;
/// Another tab's panda, this opaque (the board's).
pub const SMALL_ALPHA: f32 = 0.6;
/// Its feet this far into the window's top: on its edge, within the top
/// pixels a decoration may cover (`snow_look::REACH_BELOW`).
pub const FEET_IN: f64 = 3.0;
/// The feet's bottom in the drawing's box, from its top (of 120.5).
const FEET: f64 = 118.5;
/// A name tag: its height, its words' size, its sides' room; its bottom
/// this far into the window's top.
pub const TAG_HIGH: f64 = 18.0;
pub const TAG_TEXT: f64 = 10.5;
pub const TAG_PAD: f64 = 7.0;
pub const TAG_IN: f64 = 4.0;
/// The narrowest panda drawn; a smaller room shows none.
const NARROWEST: f64 = 36.0;
/// A sheet not drawn with for this long is let go.
const KEPT: Duration = Duration::from_secs(30);

/// A creature, as decided for its window.
#[derive(Debug, Clone, PartialEq)]
pub struct Creature {
    pub agent: u64,
    pub mood: Mood,
    pub place: Place,
    /// Its name tag ("blog · autre onglet"): another tab's.
    pub tag: Option<String>,
}

/// Where on its window's edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Place {
    /// Alone: the right corner, large.
    Alone,
    /// The shown tab's among several: the left corner.
    Shown,
    /// Another tab's, the `n`th after the shown one, small.
    Beside(usize),
    /// Over its herdr pane: the pane's first column's share of the
    /// window's width, and its width's.
    Pane { from: f64, width: f64 },
}

/// A creature placed on its window's edge: its box's left side from the
/// window's (logical), its width, how opaque; its tag's width, when it
/// has one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub x: f64,
    pub width: f64,
    pub alpha: f32,
    pub tag: Option<f64>,
}

impl Spot {
    /// Its box's height.
    pub fn height(&self) -> f64 {
        self.width * f64::from(super::rig::BOX.1) / f64::from(super::rig::BOX.0)
    }

    /// Its box's bottom from the window's top edge (negative: above it),
    /// its feet [`FEET_IN`] into the edge, or on its tag.
    pub fn bottom(&self) -> f64 {
        let below_feet = self.height() * (1.0 - FEET / f64::from(super::rig::BOX.1));
        match self.tag {
            Some(_) => TAG_IN - TAG_HIGH + below_feet,
            None => FEET_IN + below_feet,
        }
    }
}

/// Where `creatures` sit on a window `width` wide, in their order; one
/// that does not fit is left out (`None`).
pub fn spots(creatures: &[Creature], width: f64, tag_widths: &[f64]) -> Vec<Option<Spot>> {
    let mut next_beside = None;
    creatures
        .iter()
        .zip(tag_widths.iter().copied().chain(std::iter::repeat(0.0)))
        .map(|(c, tag_width)| match c.place {
            Place::Alone => {
                let w = LARGE.min(width - 2.0 * RIGHT_IN);
                (w >= NARROWEST).then_some(Spot {
                    x: width - RIGHT_IN - w,
                    width: w,
                    alpha: 1.0,
                    tag: None,
                })
            }
            Place::Shown => {
                let w = SHOWN.min(width - 2.0 * LEFT_IN);
                next_beside = Some(LEFT_IN + w + AFTER_SHOWN);
                (w >= NARROWEST).then_some(Spot {
                    x: LEFT_IN,
                    width: w,
                    alpha: 1.0,
                    tag: None,
                })
            }
            Place::Beside(_) => {
                let at = next_beside?;
                let room = SMALL.max(tag_width);
                if at + room > width - BETWEEN {
                    next_beside = Some(f64::INFINITY);
                    return None;
                }
                next_beside = Some(at + room + BETWEEN);
                Some(Spot {
                    x: at + (room - SMALL) / 2.0,
                    width: SMALL,
                    alpha: SMALL_ALPHA,
                    tag: (tag_width > 0.0).then_some(tag_width),
                })
            }
            Place::Pane { from, width: share } => {
                let pane = share * width;
                let w = PANE.min(pane - 8.0);
                (w >= NARROWEST).then(|| Spot {
                    x: (from + share / 2.0) * width - w / 2.0,
                    width: w,
                    alpha: 1.0,
                    tag: None,
                })
            }
        })
        .collect()
}

/// The creatures' state between frames.
#[derive(Default)]
pub struct Creatures {
    /// The agents as last listed.
    agents: Vec<AgentShown>,
    /// Each window's creatures, as decided.
    pub shown: BTreeMap<WindowId, Vec<Creature>>,
    /// Their motion's start.
    epoch: Option<Duration>,
    /// Each mood's drawing in pieces (none: it did not read).
    rigs: RefCell<HashMap<Mood, Option<Rc<Rig>>>>,
    /// Each mood's sheets, by its pixels a unit (in hundredths): the
    /// texture, the sheet's places, when last drawn with.
    pub(crate) sheets: RefCell<HashMap<(Mood, u32), Kept>>,
    /// Each creature drawn on a screen: its element's id, commit and last
    /// vertices; its tag's id.
    pub(crate) drawn: RefCell<HashMap<(WindowId, u64, String), Drawn>>,
    /// The tags' pictures, by their words and the scale (thousandths).
    pub(crate) tags: RefCell<HashMap<(String, u32), TagPicture>>,
    pub(crate) fonts: std::cell::OnceCell<Option<crate::radial_draw::Fonts>>,
    /// Where each creature was drawn in the last frame (its time), for
    /// the tests: its window, its agent, its box (global, logical).
    pub(crate) last: RefCell<(Duration, Vec<DrawnAt>)>,
}

impl std::fmt::Debug for Creatures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Creatures")
            .field("shown", &self.shown)
            .field("sheets", &self.sheets.borrow().len())
            .finish_non_exhaustive()
    }
}

/// A tag's picture and its size in pixels.
pub(crate) type TagPicture = (MemoryRenderBuffer, (i32, i32));

/// Where a creature was drawn: its window, its agent, its box (global,
/// logical: x, y, width, height).
pub type DrawnAt = (WindowId, u64, [f64; 4]);

/// A sheet made a texture.
pub(crate) struct Kept {
    pub texture: Rc<dyn std::any::Any>,
    pub sheet: Rc<super::rig::Sheet>,
    pub used: std::cell::Cell<Duration>,
}

/// A creature's element as last drawn.
#[derive(Debug)]
pub(crate) struct Drawn {
    pub id: Id,
    pub tag: Id,
    pub commit: CommitCounter,
    pub vertices: Vec<f32>,
    pub used: Duration,
}

impl Creatures {
    /// `mood`'s drawing in pieces, cut once.
    pub(crate) fn rig(&self, mood: Mood) -> Option<Rc<Rig>> {
        self.rigs
            .borrow_mut()
            .entry(mood)
            .or_insert_with(|| Rig::new(mood).map(Rc::new))
            .clone()
    }

    /// A frame on the screen starts at `now`: what the last drew is
    /// forgotten (the first screen's frame of a turn).
    pub(crate) fn frame_starts(&self, now: Duration) {
        let mut last = self.last.borrow_mut();
        if last.0 != now {
            *last = (now, Vec::new());
        }
    }

    /// Where each creature was drawn in the last frame.
    pub fn drawn(&self) -> Vec<DrawnAt> {
        self.last.borrow().1.clone()
    }

    /// The motion's time at `now`, seconds (within an hour: precise as a
    /// float), for an agent: each its own phase, as the board's.
    pub(crate) fn time(&self, now: Duration, agent: u64) -> f32 {
        let since = now.saturating_sub(self.epoch.unwrap_or(now));
        (since.as_secs_f64() % 3600.0) as f32 + (agent % 7) as f32 * 1.7
    }
}

/// The mood of an agent's state.
pub fn mood_of(state: &str) -> Mood {
    crate::pandas::mood(AgentState::of(state))
}

impl State {
    /// How the creatures show now: `None`, not at all; `Some(true)`,
    /// moving; `Some(false)`, still (reduced motion).
    pub(crate) fn creatures_showing(&self) -> Option<bool> {
        if !self.config.effects.creatures {
            return None;
        }
        match self.effects_still() {
            None => Some(true),
            Some(Still::ReducedMotion) => Some(false),
            Some(Still::PowerSaver | Still::Shown | Still::Cpu) => None,
        }
    }

    /// The agents changed (`flush_agents`).
    pub(crate) fn creatures_agents(&mut self, agents: &[AgentShown]) {
        if self.fx.creatures.agents != agents {
            self.fx.creatures.agents = agents.to_vec();
        }
    }

    /// The creatures move on to `now`: where they sit decided again,
    /// what they no longer draw with let go. Whether they move (a frame
    /// is wanted).
    pub(crate) fn tick_creatures(&mut self, now: Duration) -> bool {
        let showing = self.creatures_showing();
        let decided = if showing.is_some() {
            self.creatures_decided()
        } else {
            BTreeMap::new()
        };
        let creatures = &mut self.fx.creatures;
        if decided != creatures.shown {
            tracing::debug!(windows = decided.len(), "the creatures sit");
            creatures.shown = decided;
            self.redraw = true;
        }
        if creatures.shown.is_empty() {
            creatures.epoch = None;
        } else if creatures.epoch.is_none() {
            creatures.epoch = Some(now);
        }
        creatures
            .sheets
            .borrow_mut()
            .retain(|_, kept| now.saturating_sub(kept.used.get()) < KEPT);
        creatures
            .drawn
            .borrow_mut()
            .retain(|_, drawn| now.saturating_sub(drawn.used) < KEPT);
        if showing != Some(true) {
            return false;
        }
        // Moving where one shows: its window on the desktop shown, on a
        // screen that is on and shows no fullscreen window.
        self.fx.creatures.shown.keys().any(|id| {
            self.strip_offset(*id, now).is_some()
                && self
                    .output_of(*id)
                    .is_some_and(|o| self.output_powered(&o) && !self.fullscreen_in_front(&o))
        })
    }

    /// Each window's creatures, from the agents as last listed.
    fn creatures_decided(&self) -> BTreeMap<WindowId, Vec<Creature>> {
        let mut by_window: BTreeMap<WindowId, Vec<&AgentShown>> = BTreeMap::new();
        for agent in self.fx.creatures.agents.iter().filter(|a| a.window != 0) {
            by_window
                .entry(WindowId(agent.window))
                .or_default()
                .push(agent);
        }
        let lang = self.radial_lang();
        let mut out = BTreeMap::new();
        for (id, agents) in by_window {
            if !self.windows.contains_key(&id)
                || self.model.is_minimized(id)
                || matches!(
                    self.model.mode(id),
                    Mode::Fullscreen | Mode::Snapped(crate::model::Zone::Maximized)
                )
            {
                continue;
            }
            let creature = |agent: &AgentShown, place, tag| Creature {
                agent: agent.id,
                mood: mood_of(&agent.state),
                place,
                tag,
            };
            // herdr's panes shown.
            let panes: Vec<Creature> = agents
                .iter()
                .filter_map(|a| {
                    let (from, width) = self.herdr_pane_shown(a.id)?;
                    Some(creature(a, Place::Pane { from, width }, None))
                })
                .collect();
            if !panes.is_empty() {
                out.insert(id, panes);
                continue;
            }
            // herdr's agents of a tab it does not show stay in the bar.
            let agents: Vec<&AgentShown> = agents
                .into_iter()
                .filter(|a| !self.in_herdr(a.id))
                .collect();
            let creatures = match agents.as_slice() {
                [] => continue,
                [one] => vec![creature(one, Place::Alone, None)],
                several => {
                    let title = crate::windows::title_of(self, id);
                    let shown = shown_of(several, &title);
                    let mut list = Vec::with_capacity(several.len());
                    let mut n = 0;
                    for (i, agent) in several.iter().enumerate() {
                        if i == shown {
                            list.insert(0, creature(agent, Place::Shown, None));
                        } else {
                            let tag =
                                hocus_words::compositor::creatures::other_tab(lang, &agent.name);
                            list.push(creature(agent, Place::Beside(n), Some(tag)));
                            n += 1;
                        }
                    }
                    list
                }
            };
            out.insert(id, creatures);
        }
        out
    }
}

/// Which of `agents` (several in one window) its tab shows: the one the
/// window's `title` names, else the one that started working last, else
/// the one whose state changed last.
fn shown_of(agents: &[&AgentShown], title: &str) -> usize {
    let title = title.to_lowercase();
    let named = agents.iter().position(|a| {
        let name = a.name.to_lowercase();
        !name.is_empty() && title.contains(&name)
    });
    let latest = |working: bool| {
        agents
            .iter()
            .enumerate()
            .filter(|(_, a)| !working || a.state == "working")
            .max_by_key(|(_, a)| a.since)
            .map(|(i, _)| i)
    };
    named
        .or_else(|| latest(true))
        .or_else(|| latest(false))
        .unwrap_or(0)
}

/// A tag's picture: its words on a dark pill, ringed, `scale` pixels a
/// logical one; premultiplied ARGB and its size.
pub(crate) fn draw_tag(
    fonts: &crate::radial_draw::Fonts,
    words: &str,
    scale: f64,
) -> Option<(Vec<u8>, (i32, i32))> {
    use crate::radial_draw::{advance, paint, rounded, stroke, write};
    use tiny_skia::{FillRule, Pixmap, Transform};
    let k = scale as f32;
    let px = TAG_TEXT as f32 * k;
    let text_w = advance(&fonts.semibold, words, px);
    let w = text_w + 2.0 * TAG_PAD as f32 * k;
    let h = TAG_HIGH as f32 * k;
    let mut pixmap = Pixmap::new(w.ceil() as u32 + 1, h.ceil() as u32 + 1)?;
    if let Some(path) = rounded(0.5 * k, 0.5 * k, w - k, h - k, (h - k) / 2.0) {
        pixmap.fill_path(
            &path,
            &paint([14, 17, 28, 230]),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
        pixmap.stroke_path(
            &path,
            &paint([255, 255, 255, 26]),
            &stroke(k),
            Transform::identity(),
            None,
        );
    }
    write(
        &mut pixmap,
        &fonts.semibold,
        words,
        px,
        (TAG_PAD as f32 * k, h / 2.0 + px * 0.36),
        [0xA9, 0xB2, 0xC2, 0xFF],
        0.0,
    );
    let size = (pixmap.width() as i32, pixmap.height() as i32);
    Some((crate::radial_draw::argb(pixmap), size))
}

/// A tag's width, logical.
pub(crate) fn tag_width(fonts: &crate::radial_draw::Fonts, words: &str) -> f64 {
    let px = TAG_TEXT as f32;
    f64::from(crate::radial_draw::advance(&fonts.semibold, words, px)) + 2.0 * TAG_PAD
}

/// A tag's picture as the renderer takes it.
pub(crate) fn tag_buffer(pixels: &[u8], (w, h): (i32, i32)) -> MemoryRenderBuffer {
    MemoryRenderBuffer::from_slice(
        pixels,
        Fourcc::Argb8888,
        (w, h),
        1,
        smithay::utils::Transform::Normal,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(id: u64, name: &str, state: &str, since: u64) -> AgentShown {
        AgentShown {
            id,
            state: state.into(),
            since,
            source: "claude-code".into(),
            window: 7,
            name: name.into(),
            note: None,
            news: None,
        }
    }

    fn creature(agent: u64, place: Place) -> Creature {
        Creature {
            agent,
            mood: Mood::Working,
            place,
            tag: None,
        }
    }

    #[test]
    fn alone_on_the_right_corner_others_beside_the_shown_one() {
        // The board's window: 250 wide, its panda's right side 12 in.
        let alone = spots(&[creature(1, Place::Alone)], 250.0, &[]);
        assert_eq!(alone[0].map(|s| (s.x, s.width)), Some((146.0, 92.0)));
        // The board's tabs: the shown one 18 in, another 28 after it.
        let tabs = spots(
            &[creature(1, Place::Shown), creature(2, Place::Beside(0))],
            214.0,
            &[0.0, 0.0],
        );
        assert_eq!(tabs[0].map(|s| (s.x, s.width)), Some((18.0, 84.0)));
        assert_eq!(
            tabs[1].map(|s| (s.x, s.width, s.alpha)),
            Some((130.0, 52.0, 0.6))
        );
        // No room for a third.
        let three = spots(
            &[
                creature(1, Place::Shown),
                creature(2, Place::Beside(0)),
                creature(3, Place::Beside(1)),
            ],
            214.0,
            &[0.0, 0.0, 0.0],
        );
        assert_eq!(three[2], None);
    }

    #[test]
    fn a_panda_over_each_pane() {
        // The board's herdr: three panes across 400 px, 72 px pandas.
        let panes: Vec<Creature> = (0..3)
            .map(|i| {
                creature(
                    i,
                    Place::Pane {
                        from: f64::from(i as u32) / 3.0,
                        width: 1.0 / 3.0,
                    },
                )
            })
            .collect();
        let at: Vec<f64> = spots(&panes, 400.0, &[])
            .into_iter()
            .map(|s| s.map_or(0.0, |s| (s.x + s.width / 2.0).round()))
            .collect();
        assert_eq!(at, [67.0, 200.0, 333.0]);
    }

    #[test]
    fn its_feet_rest_on_the_edge() {
        let spot = spots(&[creature(1, Place::Alone)], 600.0, &[])[0].expect("fits");
        // The feet 3 px into the window's top, never past its top 8.
        let feet = spot.bottom() - spot.height() * (1.0 - FEET / 120.5);
        assert!((feet - FEET_IN).abs() < 1e-9 && feet < 8.0);
    }

    #[test]
    fn the_shown_tab_is_named_by_the_title_else_the_last_to_work() {
        let a = agent(1, "blog", "idle", 10);
        let b = agent(2, "secretaire", "done", 30);
        let c = agent(3, "api", "working", 20);
        assert_eq!(shown_of(&[&a, &b, &c], "~/github/blog — zsh"), 0);
        assert_eq!(shown_of(&[&a, &b, &c], "✳ Claude Code"), 2);
        assert_eq!(shown_of(&[&a, &b], "Terminal"), 1);
    }
}
