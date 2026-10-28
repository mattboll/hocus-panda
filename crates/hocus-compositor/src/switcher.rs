//! Alt+Tab (docs/design/windows.md §11): held, a panel of the windows
//! shows on the screen under the pointer, the current desktop's first,
//! the most recently used first, minimised ones included, then the other
//! desktops' windows, dimmed and numbered. `Tab` and `Shift+Tab` move the
//! selection, releasing `Alt` goes to it (and to its desktop), `Esc`
//! cancels, a click on one goes to it. A quick `Alt+Tab` goes back to the
//! previous window without showing anything.

use std::time::Duration;

use smithay::{
    input::keyboard::{Keysym, ModifiersState},
    output::Output,
};

use crate::{
    State,
    model::WindowId,
    motion::{self, Bezier, Tween},
};

/// A thumbnail fits in this box, its aspect kept (the prototype's 220 px
/// wide, at most 150 high).
pub const THUMB: (f64, f64) = (220.0, 150.0);
/// Around each thumbnail and its title, inside the selection's box.
pub const ITEM_PAD: f64 = 8.0;
/// Between a thumbnail and its title, and the title's line.
pub const LABEL_GAP: f64 = 8.0;
pub const LABEL_HEIGHT: f64 = 16.0;
/// The title's size, logical pixels.
pub const LABEL_SIZE: f64 = 12.0;
/// The panel's padding, the space between items, and how far it keeps
/// from the screen's sides.
pub const PANEL_PAD: f64 = 16.0;
pub const ITEM_GAP: f64 = 14.0;
pub const MARGIN: f64 = 32.0;
pub const THUMB_RADIUS: f64 = 6.0;
/// Windows of other desktops, and minimised ones, show this much.
pub const DIMMED: f32 = 0.6;

/// A rectangle in logical pixels, from the panel's screen's corner.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Area {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    /// Scaled by `k` around (`cx`, `cy`).
    pub fn scaled(self, k: f64, (cx, cy): (f64, f64)) -> Self {
        Self {
            x: cx + (self.x - cx) * k,
            y: cy + (self.y - cy) * k,
            w: self.w * k,
            h: self.h * k,
        }
    }

    pub fn lerp(self, to: Self, t: f64) -> Self {
        Self {
            x: motion::lerp(self.x, to.x, t),
            y: motion::lerp(self.y, to.y, t),
            w: motion::lerp(self.w, to.w, t),
            h: motion::lerp(self.h, to.h, t),
        }
    }

    pub fn centre(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// Where an item of the panel is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemPlace {
    /// The selection's box.
    pub item: Area,
    pub thumb: Area,
    /// The title's line, as wide as the item less its padding.
    pub label: Area,
}

/// The panel and its items for windows of these sizes, centred on a
/// screen `screen` large; smaller all together when they do not fit.
pub fn layout(sizes: &[(f64, f64)], screen: (f64, f64)) -> (Area, Vec<ItemPlace>) {
    let thumbs: Vec<(f64, f64)> = sizes
        .iter()
        .map(|&(w, h)| {
            let (w, h) = (w.max(1.0), h.max(1.0));
            if w * THUMB.1 >= h * THUMB.0 {
                (THUMB.0, h * THUMB.0 / w)
            } else {
                (w * THUMB.1 / h, THUMB.1)
            }
        })
        .collect();
    let n = thumbs.len() as f64;
    let item_w = THUMB.0 + 2.0 * ITEM_PAD;
    let tallest = thumbs.iter().map(|t| t.1).fold(0.0, f64::max);
    let item_h = |th: f64| th + 2.0 * ITEM_PAD + LABEL_GAP + LABEL_HEIGHT;
    let width = 2.0 * PANEL_PAD + n * item_w + (n - 1.0).max(0.0) * ITEM_GAP;
    let height = 2.0 * PANEL_PAD + item_h(tallest);
    let k = ((screen.0 - 2.0 * MARGIN) / width)
        .min((screen.1 - 2.0 * MARGIN) / height)
        .clamp(0.05, 1.0);
    let panel = Area {
        x: (screen.0 - width * k) / 2.0,
        y: (screen.1 - height * k) / 2.0,
        w: width * k,
        h: height * k,
    };
    let bottom = PANEL_PAD + item_h(tallest);
    let items = thumbs
        .iter()
        .enumerate()
        .map(|(i, &(tw, th))| {
            let x = PANEL_PAD + i as f64 * (item_w + ITEM_GAP);
            // The items stand on one line: their titles line up.
            let item = Area {
                x,
                y: bottom - item_h(th),
                w: item_w,
                h: item_h(th),
            };
            let thumb = Area {
                x: x + (item_w - tw) / 2.0,
                y: item.y + ITEM_PAD,
                w: tw,
                h: th,
            };
            let label = Area {
                x: x + ITEM_PAD,
                y: thumb.y + th + LABEL_GAP,
                w: THUMB.0,
                h: LABEL_HEIGHT,
            };
            let place = |a: Area| Area {
                x: panel.x + a.x * k,
                y: panel.y + a.y * k,
                w: a.w * k,
                h: a.h * k,
            };
            ItemPlace {
                item: place(item),
                thumb: place(thumb),
                label: place(label),
            }
        })
        .collect();
    (panel, items)
}

/// Which way a key moves the selection, or what else it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchKey {
    Next,
    Previous,
    Cancel,
    Commit,
    /// Any other key while the panel is up: kept from the application.
    Swallow,
}

/// The modifiers that keep Alt+Tab open while held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Hold {
    pub alt: bool,
    pub logo: bool,
    pub ctrl: bool,
}

impl Hold {
    pub fn held(self, mods: &ModifiersState) -> bool {
        (!self.alt || mods.alt) && (!self.logo || mods.logo) && (!self.ctrl || mods.ctrl)
    }

    pub fn any(self) -> bool {
        self.alt || self.logo || self.ctrl
    }
}

/// Alt+Tab while held.
#[derive(Debug, Clone)]
pub struct Switcher {
    pub output: Output,
    /// The windows, in the panel's order; whether each is on the current
    /// desktop, and its desktop (from 0).
    pub items: Vec<(WindowId, bool, usize)>,
    pub selected: usize,
    /// Alt+Tab was pressed then: the panel shows `SWITCHER_DELAY` after.
    pub opened: Duration,
    /// The selection's ring moves from this item, since then.
    pub ring_from: Option<(usize, Duration)>,
    pub hold: Hold,
    /// The key that moves the selection on (`Tab`).
    pub key: Keysym,
}

impl Switcher {
    pub fn shown(&self, now: Duration) -> bool {
        now >= self.opened + motion::SWITCHER_DELAY
    }

    /// How much the panel shows as it appears, 0 to 1.
    pub fn appear(&self, now: Duration) -> f64 {
        Tween::new(
            self.opened + motion::SWITCHER_DELAY,
            motion::SWITCHER_IN,
            Bezier::EASE_OUT,
        )
        .at(now)
    }

    /// Where the ring is between the item it leaves and the selected one,
    /// 0 to 1.
    pub fn ring(&self, now: Duration) -> (usize, f64) {
        match self.ring_from {
            Some((from, since)) => (
                from,
                Tween::new(since, motion::SWITCHER_RING, Bezier::EASE_OUT).at(now),
            ),
            None => (self.selected, 1.0),
        }
    }

    fn step(&mut self, by: isize, now: Duration) {
        let n = self.items.len() as isize;
        if n == 0 {
            return;
        }
        let from = self.selected;
        self.selected = ((self.selected as isize + by).rem_euclid(n)) as usize;
        self.ring_from = Some((from, now));
        // Tabbing again shows the panel at once.
        if !self.shown(now) {
            self.opened = now.saturating_sub(motion::SWITCHER_DELAY);
            self.ring_from = None;
        }
    }
}

/// What a modifier key is: those are passed on while the panel is up.
pub fn is_modifier(sym: Keysym) -> bool {
    matches!(
        sym,
        Keysym::Alt_L
            | Keysym::Alt_R
            | Keysym::Super_L
            | Keysym::Super_R
            | Keysym::Meta_L
            | Keysym::Meta_R
            | Keysym::Shift_L
            | Keysym::Shift_R
            | Keysym::Control_L
            | Keysym::Control_R
            | Keysym::ISO_Level3_Shift
            | Keysym::Hyper_L
            | Keysym::Hyper_R
    )
}

/// What a key pressed while Alt+Tab is held does: `sym` is its first
/// level's, `modified` what it types with the modifiers.
pub fn switch_key(switcher_key: Keysym, sym: Keysym, modified: Keysym, shift: bool) -> SwitchKey {
    if sym == switcher_key || modified == Keysym::ISO_Left_Tab {
        return if shift || modified == Keysym::ISO_Left_Tab {
            SwitchKey::Previous
        } else {
            SwitchKey::Next
        };
    }
    match sym {
        Keysym::Escape => SwitchKey::Cancel,
        Keysym::Return | Keysym::KP_Enter | Keysym::space => SwitchKey::Commit,
        Keysym::Right | Keysym::Down => SwitchKey::Next,
        Keysym::Left | Keysym::Up => SwitchKey::Previous,
        _ => SwitchKey::Swallow,
    }
}

impl State {
    /// Alt+Tab, or Alt+Shift+Tab (`backward`): the switcher opens on the
    /// screen under the pointer, the previous window (or the last one)
    /// selected.
    pub fn open_switcher(&mut self, backward: bool, hold: Hold, key: Keysym) {
        let now = self.now();
        if let Some(switcher) = &mut self.switcher {
            switcher.step(if backward { -1 } else { 1 }, now);
            self.redraw = true;
            return;
        }
        let Some(output) = self.output_under_pointer() else {
            return;
        };
        let current = self.strip(&output).current();
        let order = self.model.switcher_order(|id| {
            self.model.desktop(id) == current && self.output_of(id).as_ref() == Some(&output)
        });
        if order.is_empty() {
            return;
        }
        let items: Vec<(WindowId, bool, usize)> = order
            .iter()
            .map(|&id| {
                let here = self.model.desktop(id) == current
                    && self.output_of(id).as_ref() == Some(&output);
                (id, here, self.model.desktop(id))
            })
            .collect();
        let focused = self.model.focused();
        let first = usize::from(items.len() > 1 && Some(items[0].0) == focused);
        let selected = if backward { items.len() - 1 } else { first };
        let now = self.now();
        tracing::debug!(windows = items.len(), selected, "switcher");
        self.switcher_out = None;
        self.switcher = Some(Switcher {
            output,
            items,
            selected,
            opened: now,
            ring_from: None,
            hold,
            key,
        });
        if !hold.any() {
            // Nothing to hold: it goes at once.
            self.end_switcher(true);
        }
    }

    /// The switcher's key, while it is held.
    pub fn switcher_key(&mut self, key: SwitchKey) {
        let now = self.now();
        let by = match key {
            SwitchKey::Next => 1,
            SwitchKey::Previous => -1,
            SwitchKey::Cancel => return self.end_switcher(false),
            SwitchKey::Commit => return self.end_switcher(true),
            SwitchKey::Swallow => return,
        };
        if let Some(switcher) = &mut self.switcher {
            switcher.step(by, now);
            self.redraw = true;
        }
    }

    /// The modifiers changed: once the held ones are let go, the selected
    /// window comes forward.
    pub fn switcher_modifiers(&mut self, mods: &ModifiersState) {
        if self.switcher.as_ref().is_some_and(|s| !s.hold.held(mods)) {
            self.end_switcher(true);
        }
    }

    /// Closes the switcher; with `go`, the selected window comes forward,
    /// on its desktop, back from minimised.
    pub fn end_switcher(&mut self, go: bool) {
        let Some(switcher) = self.switcher.take() else {
            return;
        };
        let now = self.now();
        let target = switcher.items.get(switcher.selected).map(|i| i.0);
        if switcher.shown(now) {
            self.switcher_out = Some((
                switcher,
                Tween::new(now, motion::SWITCHER_OUT, Bezier::EASE_IN),
            ));
        }
        self.redraw = true;
        if go && let Some(id) = target {
            tracing::debug!(window = id.0, "switched");
            self.bring(id);
        }
    }

    /// A window went: it leaves the switcher's list.
    pub fn switcher_forget(&mut self, id: WindowId) {
        let mut empty = false;
        if let Some(switcher) = &mut self.switcher
            && let Some(at) = switcher.items.iter().position(|i| i.0 == id)
        {
            switcher.items.remove(at);
            switcher.ring_from = None;
            empty = switcher.items.is_empty();
            if switcher.selected > at || switcher.selected == switcher.items.len() {
                switcher.selected = switcher.selected.saturating_sub(1);
            }
        }
        if empty {
            self.end_switcher(false);
        }
        if let Some((switcher, _)) = &mut self.switcher_out {
            switcher.items.retain(|i| i.0 != id);
        }
    }

    /// The panel's place and its items' on its screen, logical pixels from
    /// the screen's corner.
    pub fn switcher_layout(&self, switcher: &Switcher) -> Option<(Area, Vec<ItemPlace>)> {
        let geometry = self.space.output_geometry(&switcher.output)?;
        let sizes: Vec<(f64, f64)> = switcher
            .items
            .iter()
            .map(|(id, _, _)| {
                self.home_rect(*id)
                    .map_or((4.0, 3.0), |r| (f64::from(r.width), f64::from(r.height)))
            })
            .collect();
        Some(layout(
            &sizes,
            (f64::from(geometry.size.w), f64::from(geometry.size.h)),
        ))
    }

    /// A press while the panel shows: on an item, it goes there; elsewhere,
    /// the switcher closes. Whether the press was the switcher's.
    pub fn switcher_click(&mut self, x: f64, y: f64) -> bool {
        let now = self.now();
        let Some(switcher) = self.switcher.as_ref().filter(|s| s.shown(now)) else {
            return false;
        };
        let Some(geometry) = self.space.output_geometry(&switcher.output) else {
            return false;
        };
        let (lx, ly) = (x - f64::from(geometry.loc.x), y - f64::from(geometry.loc.y));
        let hit = self
            .switcher_layout(switcher)
            .and_then(|(_, items)| items.iter().position(|p| p.item.contains(lx, ly)));
        match hit {
            Some(at) => {
                if let Some(switcher) = &mut self.switcher {
                    switcher.selected = at;
                }
                self.end_switcher(true);
            }
            None => self.end_switcher(false),
        }
        true
    }

    /// The switcher's motions; whether something moves now. `wake` asks
    /// for a frame when the panel is due to show.
    pub fn tick_switcher(&mut self, now: Duration, wake: &mut impl FnMut(Duration)) -> bool {
        let mut moving = false;
        if let Some(switcher) = &mut self.switcher {
            if switcher.shown(now) {
                moving |= switcher.appear(now) < 1.0;
                if let Some((_, since)) = switcher.ring_from {
                    if now >= since + motion::SWITCHER_RING {
                        switcher.ring_from = None;
                    } else {
                        moving = true;
                    }
                }
            } else {
                wake(switcher.opened + motion::SWITCHER_DELAY);
            }
        }
        if let Some((_, tween)) = &self.switcher_out {
            if tween.done(now) {
                self.switcher_out = None;
            } else {
                moving = true;
            }
        }
        moving
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_keep_their_aspect_in_220_by_150_and_titles_line_up() {
        let (panel, items) = layout(&[(1600.0, 1000.0), (800.0, 1200.0)], (1920.0, 1080.0));
        assert_eq!(items.len(), 2);
        let (a, b) = (items[0].thumb, items[1].thumb);
        assert_eq!((a.w, a.h), (220.0, 137.5));
        assert_eq!((b.w, b.h), (100.0, 150.0));
        assert_eq!(items[0].label.y, items[1].label.y);
        // 16 + 236 + 14 + 236 + 16 wide, centred.
        assert_eq!(panel.w, 518.0);
        assert_eq!(panel.x, (1920.0 - 518.0) / 2.0);
        assert_eq!(items[1].item.x - items[0].item.x, 236.0 + ITEM_GAP);
        // The tallest item fills the panel's height less its padding.
        assert_eq!(panel.h, 2.0 * PANEL_PAD + 150.0 + 40.0);
    }

    #[test]
    fn many_windows_shrink_to_fit_the_screen() {
        let sizes = vec![(800.0, 600.0); 12];
        let (panel, items) = layout(&sizes, (1920.0, 1080.0));
        assert!(panel.w <= 1920.0 - 2.0 * MARGIN + 1e-6);
        assert!(panel.x >= MARGIN - 1e-6);
        let last = items[11].item;
        assert!(last.x + last.w <= panel.x + panel.w);
    }

    #[test]
    fn tab_moves_on_shift_tab_back_and_escape_cancels() {
        let tab = Keysym::Tab;
        assert_eq!(switch_key(tab, tab, tab, false), SwitchKey::Next);
        assert_eq!(
            switch_key(tab, tab, Keysym::ISO_Left_Tab, true),
            SwitchKey::Previous
        );
        assert_eq!(
            switch_key(tab, Keysym::Escape, Keysym::Escape, false),
            SwitchKey::Cancel
        );
        assert_eq!(
            switch_key(tab, Keysym::a, Keysym::a, false),
            SwitchKey::Swallow
        );
        let hold = Hold {
            alt: true,
            ..Hold::default()
        };
        let mut mods = ModifiersState {
            alt: true,
            ..ModifiersState::default()
        };
        assert!(hold.held(&mods));
        mods.alt = false;
        assert!(!hold.held(&mods));
    }
}
