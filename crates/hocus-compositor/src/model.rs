//! The windows as the desktop knows them, apart from smithay: their ids,
//! their stacking order, which one has the focus, how each is placed
//! (floating, snapped to a half or a quarter, maximised, fullscreen,
//! minimised), and the geometry of placing, snapping and resizing.
//! Plain data and decisions, tested without a compositor; `State` carries
//! them out on smithay's windows (docs/design/windows.md §2, §5, §8–§10).

use std::collections::{BTreeMap, HashMap};

/// Space around windows the desktop places, and between the work area and
/// the output's edges or a bar's reserved space (docs/design/windows.md §2).
pub const GAP: i32 = 8;
/// The bar's width in its two forms, and how far it keeps from the
/// output's edge (docs/design/bar.md §2, §3): its space is kept from this
/// slice on, so that nothing moves when it comes.
pub const BAR_FULL: i32 = 196;
pub const BAR_MINI: i32 = 56;
/// An output at least this wide without the bar has the full bar.
pub const BAR_FULL_FROM: i32 = 1600;
/// The pointer this close to a vertical or top edge of the work area
/// snaps to a half or maximises.
pub const SNAP_EDGE: i32 = 24;
/// And this close to a corner, to a quarter.
pub const SNAP_CORNER: i32 = 48;
/// The resize band outside each edge, and how far a corner reaches along
/// it (§8).
pub const FRAME: i32 = 8;
pub const CORNER: i32 = 16;
/// The top of a window with no header of its own resizes over its first
/// pixels (§7).
pub const TOP_INSIDE: i32 = 4;
/// Where a window of the same application already is, the next opens this
/// much down and right (§5).
pub const CASCADE: i32 = 32;
/// Windows are never resized below this, whatever their client says.
pub const MIN_SIZE: (i32, i32) = (120, 80);

/// A window, for as long as it is mapped. Ids are never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub u64);

/// A rectangle in the global logical space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Smaller by `by` on every side; never negative.
    pub fn inset(self, by: i32) -> Self {
        Self {
            x: self.x + by,
            y: self.y + by,
            width: (self.width - 2 * by).max(0),
            height: (self.height - 2 * by).max(0),
        }
    }

    pub const fn right(self) -> i32 {
        self.x + self.width
    }

    pub const fn bottom(self) -> i32 {
        self.y + self.height
    }

    pub fn contains(self, x: f64, y: f64) -> bool {
        x >= f64::from(self.x)
            && y >= f64::from(self.y)
            && x < f64::from(self.right())
            && y < f64::from(self.bottom())
    }

    pub fn centre(self) -> (f64, f64) {
        (
            f64::from(self.x) + f64::from(self.width) / 2.0,
            f64::from(self.y) + f64::from(self.height) / 2.0,
        )
    }
}

/// The bar's width on an output `width` logical pixels wide.
pub fn bar_width(width: i32) -> i32 {
    if width - BAR_FULL >= BAR_FULL_FROM {
        BAR_FULL
    } else {
        BAR_MINI
    }
}

/// Where the desktop places windows on an output: what layer surfaces
/// leave of it (`non_exclusive`, global) less the bar's space on the
/// output's right, and the gap.
pub fn work_area(non_exclusive: Rect, output: Rect) -> Rect {
    let bar_left = output.right() - GAP - bar_width(output.width);
    let mut zone = non_exclusive;
    zone.width = zone.width.min(bar_left - zone.x).max(0);
    zone.inset(GAP)
}

/// Where a window of `size` goes to sit at the centre of `area`: its
/// top-left corner. A window larger than the area keeps its top-left corner
/// in it, so its header stays reachable.
pub fn centred(area: Rect, size: (i32, i32)) -> (i32, i32) {
    let along = |start: i32, room: i32, length: i32| start + ((room - length) / 2).max(0);
    (
        along(area.x, area.width, size.0),
        along(area.y, area.height, size.1),
    )
}

/// Where a new window of `size` opens: at the centre of `area`, and down
/// and right of a window of the same application already there
/// (`same_app`: their top-left corners), as long as it stays in the area.
pub fn place(area: Rect, size: (i32, i32), same_app: &[(i32, i32)]) -> (i32, i32) {
    let mut at = centred(area, size);
    while same_app.contains(&at)
        && at.0 + CASCADE + size.0 <= area.right()
        && at.1 + CASCADE + size.1 <= area.bottom()
    {
        at = (at.0 + CASCADE, at.1 + CASCADE);
    }
    at
}

/// Where a floating window at `rect` in the work area `from` goes in
/// another screen's work area `to` (§12): its centre at the same share
/// across and down, its size no larger than `to`, and wholly inside it.
pub fn carried(rect: Rect, from: Rect, to: Rect) -> Rect {
    let width = rect.width.min(to.width).max(0);
    let height = rect.height.min(to.height).max(0);
    let (cx, cy) = rect.centre();
    let along = |centre: f64, start: i32, room: i32, to_start: i32, to_room: i32, size: i32| {
        let share = ((centre - f64::from(start)) / f64::from(room.max(1))).clamp(0.0, 1.0);
        let at = f64::from(to_start) + share * f64::from(to_room) - f64::from(size) / 2.0;
        (at.round() as i32).clamp(to_start, to_start + to_room - size)
    };
    Rect::new(
        along(cx, from.x, from.width, to.x, to.width, width),
        along(cy, from.y, from.height, to.y, to.height, height),
        width,
        height,
    )
}

/// Where a window snaps: a half, a quarter, or the whole work area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Zone {
    Left,
    Right,
    Maximized,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Which sides of the work area a zone touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Start,
    End,
    Both,
}

impl Zone {
    /// The zone's rectangle in `area`: halves and quarters keep the gap
    /// between them.
    pub fn rect(self, area: Rect) -> Rect {
        let left_w = (area.width - GAP) / 2;
        let top_h = (area.height - GAP) / 2;
        let (x, w) = match self.horizontal() {
            Side::Start => (area.x, left_w),
            Side::End => (area.x + left_w + GAP, area.width - left_w - GAP),
            Side::Both => (area.x, area.width),
        };
        let (y, h) = match self.vertical() {
            Side::Start => (area.y, top_h),
            Side::End => (area.y + top_h + GAP, area.height - top_h - GAP),
            Side::Both => (area.y, area.height),
        };
        Rect::new(x, y, w.max(0), h.max(0))
    }

    pub fn horizontal(self) -> Side {
        match self {
            Self::Left | Self::TopLeft | Self::BottomLeft => Side::Start,
            Self::Right | Self::TopRight | Self::BottomRight => Side::End,
            Self::Maximized => Side::Both,
        }
    }

    pub fn vertical(self) -> Side {
        match self {
            Self::TopLeft | Self::TopRight => Side::Start,
            Self::BottomLeft | Self::BottomRight => Side::End,
            Self::Left | Self::Right | Self::Maximized => Side::Both,
        }
    }

    fn from_sides(horizontal: Side, vertical: Side) -> Self {
        match (horizontal, vertical) {
            (Side::Start, Side::Both) => Self::Left,
            (Side::End, Side::Both) => Self::Right,
            (Side::Start, Side::Start) => Self::TopLeft,
            (Side::End, Side::Start) => Self::TopRight,
            (Side::Start, Side::End) => Self::BottomLeft,
            (Side::End, Side::End) => Self::BottomRight,
            (Side::Both, _) => Self::Maximized,
        }
    }

    /// The zone the pointer at (`x`, `y`) is in while dragging a window:
    /// a corner (within [`SNAP_CORNER`] of it), the left or right edge (a
    /// half) or the top edge (maximised), from the work area's edges; the
    /// pointer may be past them, up to the output's edge.
    pub fn at(area: Rect, x: f64, y: f64) -> Option<Self> {
        let near = |d: f64, within: i32| d < f64::from(within);
        let left = x - f64::from(area.x);
        let right = f64::from(area.right()) - x;
        let top = y - f64::from(area.y);
        let bottom = f64::from(area.bottom()) - y;
        let at_left = near(left, SNAP_EDGE);
        let at_right = near(right, SNAP_EDGE);
        let at_top = near(top, SNAP_EDGE);
        let at_bottom = near(bottom, SNAP_EDGE);
        let corner_x = near(left, SNAP_CORNER) || near(right, SNAP_CORNER);
        let corner_y = near(top, SNAP_CORNER) || near(bottom, SNAP_CORNER);
        let side_x = if left < right { Side::Start } else { Side::End };
        let side_y = if top < bottom { Side::Start } else { Side::End };
        // A corner: at an edge, and within reach of the other one.
        if (at_left || at_right) && corner_y || (at_top || at_bottom) && corner_x {
            return Some(Self::from_sides(side_x, side_y));
        }
        if at_left || at_right {
            return Some(Self::from_sides(side_x, Side::Both));
        }
        at_top.then_some(Self::Maximized)
    }
}

/// How a window is placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Where the user put it.
    #[default]
    Floating,
    /// In a zone of the work area, maximised included.
    Snapped(Zone),
    /// Over its whole output, the bar's space included.
    Fullscreen,
}

/// Which edges a resize moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Edges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl Edges {
    pub const fn any(self) -> bool {
        self.left || self.right || self.top || self.bottom
    }

    /// Its CSS and cursor-shape name: `ew-resize`, `ns-resize`,
    /// `nwse-resize` or `nesw-resize` (§8).
    pub fn cursor_name(self) -> &'static str {
        match (self.left || self.right, self.top || self.bottom) {
            (true, false) => "ew-resize",
            (false, true) => "ns-resize",
            (true, true) if self.left == self.top => "nwse-resize",
            (true, true) => "nesw-resize",
            (false, false) => "default",
        }
    }

    /// The edges of the corner of `rect` nearest to (`x`, `y`): what
    /// `Super`+right-drag resizes.
    pub fn nearest_corner(rect: Rect, x: f64, y: f64) -> Self {
        let (cx, cy) = rect.centre();
        Self {
            left: x < cx,
            right: x >= cx,
            top: y < cy,
            bottom: y >= cy,
        }
    }
}

/// Where the pointer at (`x`, `y`) is on the frame of a window at `rect`:
/// the edges it resizes, in the band outside each edge (corners reach
/// [`CORNER`] along them), and over the first `top_inside` pixels of its
/// top. `None` inside the window or away from it.
pub fn frame_hit(rect: Rect, x: f64, y: f64, top_inside: i32) -> Option<Edges> {
    let outer = Rect::new(
        rect.x - FRAME,
        rect.y - FRAME,
        rect.width + 2 * FRAME,
        rect.height + 2 * FRAME,
    );
    if !outer.contains(x, y) {
        return None;
    }
    let inside = rect.contains(x, y);
    let top_band = inside && y < f64::from(rect.y + top_inside);
    if inside && !top_band {
        return None;
    }
    let (left, top) = (f64::from(rect.x), f64::from(rect.y));
    let (right, bottom) = (f64::from(rect.right()), f64::from(rect.bottom()));
    let corner = f64::from(CORNER);
    let near_left = x < left + corner;
    let near_right = x >= right - corner;
    let near_top = y < top + corner;
    let near_bottom = y >= bottom - corner;
    let out_left = x < left;
    let out_right = x >= right;
    let out_top = y < top || top_band;
    let out_bottom = y >= bottom;
    Some(Edges {
        left: out_left || ((out_top || out_bottom) && near_left),
        right: out_right || ((out_top || out_bottom) && near_right),
        top: out_top || ((out_left || out_right) && near_top),
        bottom: out_bottom || ((out_left || out_right) && near_bottom),
    })
}

/// `rect` resized by moving its `edges` by (`dx`, `dy`), between `min` and
/// `max` (0: no maximum); the other edges stay where they are.
pub fn resized(
    rect: Rect,
    edges: Edges,
    (dx, dy): (i32, i32),
    min: (i32, i32),
    max: (i32, i32),
) -> Rect {
    let clamp = |v: i32, lo: i32, hi: i32| {
        let lo = lo.max(1);
        if hi > 0 {
            v.clamp(lo, hi.max(lo))
        } else {
            v.max(lo)
        }
    };
    let mut out = rect;
    if edges.left || edges.right {
        let w = if edges.left {
            rect.width - dx
        } else {
            rect.width + dx
        };
        out.width = clamp(w, min.0, max.0);
        if edges.left {
            out.x = rect.right() - out.width;
        }
    }
    if edges.top || edges.bottom {
        let h = if edges.top {
            rect.height - dy
        } else {
            rect.height + dy
        };
        out.height = clamp(h, min.1, max.1);
        if edges.top {
            out.y = rect.bottom() - out.height;
        }
    }
    out
}

/// Where a window dragged out of a zone goes: its size before it snapped,
/// under the pointer at (`x`, `y`) at the same place across it (`along`,
/// from 0 at its left to 1 at its right), and as far down as it was held
/// (`held_down` of its `height` then): a header stays under the pointer,
/// and a point further down than the smaller window reaches keeps its
/// share of the height.
pub fn torn_off(
    restore: (i32, i32),
    along: f64,
    (held_down, height): (f64, i32),
    x: f64,
    y: f64,
) -> (i32, i32) {
    let left = x - along.clamp(0.0, 1.0) * f64::from(restore.0);
    let restored = f64::from(restore.1);
    let down = if held_down < restored {
        held_down.max(0.0)
    } else {
        held_down / f64::from(height.max(1)) * restored
    };
    (left.round() as i32, (y - down).round() as i32)
}

/// What the keyboard asks of the focused window (§9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Snap {
    /// `Super+←` / `Super+→`.
    Half(Side),
    /// `Super+↑`.
    Maximize,
    /// `Super+↓`: restore; when it is floating already, minimise.
    Restore,
    /// `Super+Shift+arrows`: to the quarter on that side, keeping the
    /// other way's side.
    QuarterTop,
    QuarterBottom,
    QuarterLeft,
    QuarterRight,
}

/// What a keyboard snap does to a window in `mode` whose geometry is
/// `rect`, in `area`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapResult {
    To(Zone),
    Restore,
    Minimize,
}

pub fn keyboard_snap(snap: Snap, mode: Mode, rect: Rect, area: Rect) -> SnapResult {
    let (cx, cy) = rect.centre();
    let (ax, ay) = area.centre();
    let current = match mode {
        Mode::Snapped(zone) => Some(zone),
        Mode::Floating | Mode::Fullscreen => None,
    };
    // The side the window is on each way: its zone's, or its centre's.
    let horizontal = current.map(Zone::horizontal).filter(|s| *s != Side::Both);
    let horizontal = horizontal.unwrap_or(if cx < ax { Side::Start } else { Side::End });
    let vertical = current.map(Zone::vertical).filter(|s| *s != Side::Both);
    let vertical = vertical.unwrap_or(if cy < ay { Side::Start } else { Side::End });
    match snap {
        Snap::Half(side) => {
            let half = Zone::from_sides(side, Side::Both);
            if current == Some(half) {
                // Again: to the other side.
                let other = if side == Side::Start {
                    Side::End
                } else {
                    Side::Start
                };
                SnapResult::To(Zone::from_sides(other, Side::Both))
            } else {
                SnapResult::To(half)
            }
        }
        Snap::Maximize => SnapResult::To(Zone::Maximized),
        Snap::Restore => match mode {
            Mode::Floating => SnapResult::Minimize,
            Mode::Snapped(_) | Mode::Fullscreen => SnapResult::Restore,
        },
        Snap::QuarterTop => SnapResult::To(Zone::from_sides(horizontal, Side::Start)),
        Snap::QuarterBottom => SnapResult::To(Zone::from_sides(horizontal, Side::End)),
        Snap::QuarterLeft => SnapResult::To(Zone::from_sides(Side::Start, vertical)),
        Snap::QuarterRight => SnapResult::To(Zone::from_sides(Side::End, vertical)),
    }
}

/// Snapped windows on the other side of the edges a resize moves: they
/// move their facing edge with it. `rect` is the window resized, `others`
/// the other snapped windows of its output; the result says, for each
/// that shares a moved edge, which of its edges follows.
pub fn partners(rect: Rect, edges: Edges, others: &[(WindowId, Rect)]) -> Vec<(WindowId, Edges)> {
    let overlaps = |a0: i32, a1: i32, b0: i32, b1: i32| a0 < b1 && b0 < a1;
    others
        .iter()
        .filter_map(|&(id, o)| {
            let across = overlaps(rect.y, rect.bottom(), o.y, o.bottom());
            let down = overlaps(rect.x, rect.right(), o.x, o.right());
            let follow = Edges {
                left: edges.right && across && o.x == rect.right() + GAP,
                right: edges.left && across && o.right() + GAP == rect.x,
                top: edges.bottom && down && o.y == rect.bottom() + GAP,
                bottom: edges.top && down && o.bottom() + GAP == rect.y,
            };
            follow.any().then_some((id, follow))
        })
        .collect()
}

/// A partner's new geometry once the window it faces is at `moved`.
pub fn follow(partner: Rect, follow: Edges, moved: Rect) -> Rect {
    let mut out = partner;
    if follow.left {
        out.x = moved.right() + GAP;
        out.width = (partner.right() - out.x).max(1);
    }
    if follow.right {
        out.width = (moved.x - GAP - partner.x).max(1);
    }
    if follow.top {
        out.y = moved.bottom() + GAP;
        out.height = (partner.bottom() - out.y).max(1);
    }
    if follow.bottom {
        out.height = (moved.y - GAP - partner.y).max(1);
    }
    out
}

/// Where two snapped windows face each other across the gap (§8): its
/// handle moves the boundary and both follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seam {
    /// The window on the left, or above.
    pub first: WindowId,
    pub second: WindowId,
    /// Side by side: the seam runs down; otherwise across.
    pub vertical: bool,
    /// Where the gap starts across the seam: the first window's right or
    /// bottom edge.
    pub across: i32,
    /// The span both windows share along the seam.
    pub from: i32,
    pub to: i32,
    /// The window a drag of the handle resizes, and from which edge: the
    /// one whose side spans the seam's whole length, so that every
    /// neighbour along it follows.
    pub grab: WindowId,
    pub edges: Edges,
}

impl Seam {
    /// The middle of the gap across, and of the shared span along.
    pub fn centre(&self) -> (f64, f64) {
        let across = f64::from(self.across) + f64::from(GAP) / 2.0;
        let along = f64::from(self.from + self.to) / 2.0;
        if self.vertical {
            (across, along)
        } else {
            (along, across)
        }
    }

    /// The point (`x`, `y`) as (across, along) the seam: from its centre
    /// line, and from where its span starts.
    fn local(&self, x: f64, y: f64) -> (f64, f64) {
        let (cx, cy) = self.centre();
        if self.vertical {
            (x - cx, y - f64::from(self.from))
        } else {
            (y - cy, x - f64::from(self.from))
        }
    }

    /// Whether (`x`, `y`) is within `reach` of the seam's centre line,
    /// along its span.
    pub fn near(&self, x: f64, y: f64, reach: f64) -> bool {
        let (across, along) = self.local(x, y);
        across.abs() <= reach && along >= 0.0 && along < f64::from(self.to - self.from)
    }

    /// Whether (`x`, `y`) is on a box `length` long and `thickness` thick
    /// at the seam's centre.
    pub fn on_handle(&self, x: f64, y: f64, length: f64, thickness: f64) -> bool {
        let (across, along) = self.local(x, y);
        let middle = f64::from(self.to - self.from) / 2.0;
        across.abs() <= thickness / 2.0 && (along - middle).abs() <= length / 2.0
    }

    /// In the gap, away from the span's ends (a corner's reach there): the
    /// window whose edge is nearer, and that edge.
    pub fn edge_at(&self, x: f64, y: f64) -> Option<(WindowId, Edges)> {
        let (across, along) = self.local(x, y);
        let corner = f64::from(CORNER);
        let outside = across.abs() > f64::from(GAP) / 2.0
            || along < corner
            || along >= f64::from(self.to - self.from) - corner;
        if outside {
            None
        } else if across < 0.0 {
            Some((self.first, self.first_edges()))
        } else {
            Some((self.second, self.second_edges()))
        }
    }

    /// The first window's edge on the seam: right or bottom.
    fn first_edges(&self) -> Edges {
        Edges {
            right: self.vertical,
            bottom: !self.vertical,
            ..Edges::default()
        }
    }

    /// The second window's: left or top.
    fn second_edges(&self) -> Edges {
        Edges {
            left: self.vertical,
            top: !self.vertical,
            ..Edges::default()
        }
    }
}

/// Where `a` and `b` meet across the gap, if they do.
pub fn seam((a, ra): (WindowId, Rect), (b, rb): (WindowId, Rect)) -> Option<Seam> {
    let make = |(first, rf): (WindowId, Rect), (second, rs): (WindowId, Rect), vertical: bool| {
        let (from, to, across, first_longer) = if vertical {
            let span = (rf.y.max(rs.y), rf.bottom().min(rs.bottom()));
            (span.0, span.1, rf.right(), rf.height >= rs.height)
        } else {
            let span = (rf.x.max(rs.x), rf.right().min(rs.right()));
            (span.0, span.1, rf.bottom(), rf.width >= rs.width)
        };
        if from >= to {
            return None;
        }
        let mut seam = Seam {
            first,
            second,
            vertical,
            across,
            from,
            to,
            grab: first,
            edges: Edges::default(),
        };
        (seam.grab, seam.edges) = if first_longer {
            (first, seam.first_edges())
        } else {
            (second, seam.second_edges())
        };
        Some(seam)
    };
    if ra.right() + GAP == rb.x {
        make((a, ra), (b, rb), true)
    } else if rb.right() + GAP == ra.x {
        make((b, rb), (a, ra), true)
    } else if ra.bottom() + GAP == rb.y {
        make((a, ra), (b, rb), false)
    } else if rb.bottom() + GAP == ra.y {
        make((b, rb), (a, ra), false)
    } else {
        None
    }
}

/// What the desktop keeps of a window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Placement {
    pub mode: Mode,
    /// Where it was floating before it snapped, maximised or went
    /// fullscreen: where it goes back.
    pub restore: Option<Rect>,
    pub minimized: bool,
    /// The desktop it is on, from 0, on its screen (§12).
    pub desktop: usize,
    /// Its desktop is not the one its screen shows.
    pub hidden: bool,
}

/// The mapped windows, bottom first, the focus, and how each is placed.
#[derive(Debug, Default)]
pub struct Model {
    stack: Vec<WindowId>,
    focused: Option<WindowId>,
    next: u64,
    placements: BTreeMap<WindowId, Placement>,
    /// Every window, the most recently focused first: Alt+Tab's order
    /// (§11).
    recent: Vec<WindowId>,
    /// Each application's last size, by app id (§5).
    sizes: HashMap<String, (i32, i32)>,
}

impl Model {
    /// A new window, on top. It does not take the focus by itself: the
    /// policy for that is the caller's (slice 1 §4).
    pub fn add(&mut self) -> WindowId {
        self.next += 1;
        let id = WindowId(self.next);
        self.stack.push(id);
        self.placements.insert(id, Placement::default());
        self.recent.push(id);
        id
    }

    /// A new window just under the focused one: it opens behind it, and
    /// comes next to it in Alt+Tab's order.
    pub fn add_below_focus(&mut self) -> WindowId {
        let id = self.add();
        if let Some(focused) = self.focused
            && let Some(at) = self.stack.iter().position(|w| *w == focused)
        {
            self.stack.pop();
            self.stack.insert(at, id);
            if let Some(at) = self.recent.iter().position(|w| *w == focused) {
                self.recent.pop();
                self.recent.insert(at + 1, id);
            }
        }
        id
    }

    /// A window went; the focus goes to the one now on top that shows.
    pub fn remove(&mut self, id: WindowId) {
        self.stack.retain(|w| *w != id);
        self.placements.remove(&id);
        self.recent.retain(|w| *w != id);
        if self.focused == Some(id) {
            self.focused = self.top_shown();
        }
    }

    /// The window on top that shows: not minimised, on its screen's
    /// desktop.
    pub fn top_shown(&self) -> Option<WindowId> {
        self.stack
            .iter()
            .rev()
            .find(|w| self.is_shown(**w))
            .copied()
    }

    /// Whether `id` shows: not minimised, and its desktop is the one its
    /// screen shows.
    pub fn is_shown(&self, id: WindowId) -> bool {
        self.placements
            .get(&id)
            .is_some_and(|p| !p.minimized && !p.hidden)
    }

    pub fn desktop(&self, id: WindowId) -> usize {
        self.placement(id).desktop
    }

    /// Puts `id` on `desktop` of its screen; whether it shows is the
    /// caller's to say ([`Model::set_hidden`]).
    pub fn set_desktop(&mut self, id: WindowId, desktop: usize) {
        if let Some(p) = self.placements.get_mut(&id) {
            p.desktop = desktop;
        }
    }

    /// Whether `id`'s desktop is out of sight. A hidden window loses the
    /// focus to the window on top that shows.
    pub fn set_hidden(&mut self, id: WindowId, hidden: bool) {
        if let Some(p) = self.placements.get_mut(&id) {
            p.hidden = hidden;
        }
        if hidden && self.focused == Some(id) {
            self.focused = self.top_shown();
        }
    }

    /// Every window, the most recently focused first.
    pub fn recent(&self) -> &[WindowId] {
        &self.recent
    }

    /// Alt+Tab's list (§11): the windows of the current desktop
    /// (`on_current`), minimised ones included, the most recently used
    /// first; then the other desktops' windows in the same order.
    pub fn switcher_order(&self, on_current: impl Fn(WindowId) -> bool) -> Vec<WindowId> {
        let (mut here, elsewhere): (Vec<WindowId>, Vec<WindowId>) =
            self.recent.iter().copied().partition(|w| on_current(*w));
        here.extend(elsewhere);
        here
    }

    /// Focuses and raises `id`; `None` takes the focus from every window.
    pub fn focus(&mut self, id: Option<WindowId>) {
        if let Some(id) = id {
            if !self.stack.contains(&id) {
                return;
            }
            self.raise(id);
            self.recent.retain(|w| *w != id);
            self.recent.insert(0, id);
        }
        self.focused = id;
    }

    /// Puts `id` on top without focusing it.
    pub fn raise(&mut self, id: WindowId) {
        if self.stack.contains(&id) {
            self.stack.retain(|w| *w != id);
            self.stack.push(id);
        }
    }

    pub fn focused(&self) -> Option<WindowId> {
        self.focused
    }

    /// Bottom first.
    pub fn stack(&self) -> &[WindowId] {
        &self.stack
    }

    pub fn placement(&self, id: WindowId) -> Placement {
        self.placements.get(&id).copied().unwrap_or_default()
    }

    pub fn mode(&self, id: WindowId) -> Mode {
        self.placement(id).mode
    }

    /// `id` goes to `mode`. Leaving floating, its floating geometry
    /// `current` is kept to come back to.
    pub fn set_mode(&mut self, id: WindowId, mode: Mode, current: Rect) {
        if let Some(p) = self.placements.get_mut(&id) {
            if p.mode == Mode::Floating && mode != Mode::Floating {
                p.restore = Some(current);
            }
            p.mode = mode;
        }
    }

    /// Where `id` goes back to floating: its screen changed (§12).
    pub fn set_restore(&mut self, id: WindowId, restore: Option<Rect>) {
        if let Some(p) = self.placements.get_mut(&id) {
            p.restore = restore;
        }
    }

    /// Back to floating; where it was before, if known.
    pub fn unsnap(&mut self, id: WindowId) -> Option<Rect> {
        let p = self.placements.get_mut(&id)?;
        p.mode = Mode::Floating;
        p.restore.take()
    }

    pub fn is_minimized(&self, id: WindowId) -> bool {
        self.placements.get(&id).is_some_and(|p| p.minimized)
    }

    /// Minimises `id`: the focus goes to the window on top that shows.
    pub fn minimize(&mut self, id: WindowId) {
        let Some(p) = self.placements.get_mut(&id) else {
            return;
        };
        p.minimized = true;
        if self.focused == Some(id) {
            self.focused = self.top_shown();
        }
    }

    /// Brings `id` back (not focused: the caller focuses it).
    pub fn unminimize(&mut self, id: WindowId) {
        if let Some(p) = self.placements.get_mut(&id) {
            p.minimized = false;
        }
    }

    /// Remembers an application's size.
    pub fn remember_size(&mut self, app_id: &str, size: (i32, i32)) {
        if !app_id.is_empty() && size.0 > 0 && size.1 > 0 {
            self.sizes.insert(app_id.to_string(), size);
        }
    }

    pub fn remembered_size(&self, app_id: &str) -> Option<(i32, i32)> {
        self.sizes.get(app_id).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: Rect = Rect::new(0, 0, 1920, 1080);

    #[test]
    fn the_work_area_keeps_a_gap_and_the_bars_space() {
        // 1920 wide: the full bar, 196 px and 8 px from the edge.
        assert_eq!(
            work_area(OUTPUT, OUTPUT),
            Rect::new(8, 8, 1920 - 8 - 196 - 8 - 8, 1064)
        );
        // A 40 px bar along the top (a layer's exclusive zone).
        let zone = Rect::new(0, 40, 1920, 1040);
        assert_eq!(work_area(zone, OUTPUT), Rect::new(8, 48, 1700, 1024));
        // 1366 wide: the mini bar.
        let small = Rect::new(0, 0, 1366, 768);
        assert_eq!(work_area(small, small).right(), 1366 - 8 - 56 - 8);
        assert_eq!(Rect::new(0, 0, 10, 10).inset(8), Rect::new(8, 8, 0, 0));
    }

    #[test]
    fn a_new_window_sits_at_the_centre_and_cascades_from_its_own() {
        let area = Rect::new(8, 48, 1904, 1024);
        assert_eq!(centred(area, (800, 600)), (8 + 552, 48 + 212));
        // Too large: its top-left corner stays in the area.
        assert_eq!(centred(area, (3000, 2000)), (8, 48));
        let first = place(area, (800, 600), &[]);
        let second = place(area, (800, 600), &[first]);
        assert_eq!(second, (first.0 + CASCADE, first.1 + CASCADE));
        assert_eq!(
            place(area, (800, 600), &[first, second]),
            (first.0 + 2 * CASCADE, first.1 + 2 * CASCADE)
        );
    }

    #[test]
    fn a_window_carried_to_another_screen_keeps_its_place_and_fits() {
        let big = Rect::new(1928, 8, 2340, 1424);
        let small = Rect::new(8, 8, 1700, 1064);
        // Centred: centred.
        let centred = Rect::new(1928 + 870, 8 + 512, 600, 400);
        assert_eq!(carried(centred, big, small), Rect::new(558, 340, 600, 400));
        // In the bottom right quarter: there too, the same size.
        let r = carried(Rect::new(3398, 820, 600, 400), big, small);
        assert_eq!((r.width, r.height), (600, 400));
        let (cx, cy) = r.centre();
        assert!(cx > 8.0 + 1700.0 * 0.5 && cy > 8.0 + 1064.0 * 0.5, "{r:?}");
        // Against the left edge: against it still.
        assert_eq!(carried(Rect::new(1928, 100, 800, 300), big, small).x, 8);
        // Partly off its screen, or larger than the new area: inside it.
        let r = carried(Rect::new(4000, 1300, 2400, 600), big, small);
        assert_eq!(r, Rect::new(8, 472, 1700, 600));
    }

    #[test]
    fn halves_and_quarters_keep_the_gap_between_them() {
        let area = Rect::new(8, 8, 1700, 1064);
        let left = Zone::Left.rect(area);
        let right = Zone::Right.rect(area);
        assert_eq!(left, Rect::new(8, 8, 846, 1064));
        assert_eq!(right.x, left.right() + GAP);
        assert_eq!(right.right(), area.right());
        let tl = Zone::TopLeft.rect(area);
        let bl = Zone::BottomLeft.rect(area);
        assert_eq!(bl.y, tl.bottom() + GAP);
        assert_eq!(bl.bottom(), area.bottom());
        assert_eq!(Zone::Maximized.rect(area), area);
    }

    #[test]
    fn the_pointer_at_an_edge_or_a_corner_picks_a_zone() {
        let area = Rect::new(8, 8, 1700, 1064);
        let at = |x: f64, y: f64| Zone::at(area, x, y);
        assert_eq!(at(0.0, 500.0), Some(Zone::Left));
        assert_eq!(at(1705.0, 500.0), Some(Zone::Right));
        assert_eq!(at(800.0, 0.0), Some(Zone::Maximized));
        assert_eq!(at(800.0, 500.0), None);
        // Within 48 px of a corner, along an edge.
        assert_eq!(at(0.0, 40.0), Some(Zone::TopLeft));
        assert_eq!(at(40.0, 0.0), Some(Zone::TopLeft));
        assert_eq!(at(1700.0, 1060.0), Some(Zone::BottomRight));
        assert_eq!(at(0.0, 1050.0), Some(Zone::BottomLeft));
        assert_eq!(at(1690.0, 2.0), Some(Zone::TopRight));
        // Along the bottom edge, away from the corners: nothing.
        assert_eq!(at(800.0, 1070.0), None);
    }

    #[test]
    fn the_frame_resizes_from_every_edge_and_corner() {
        let rect = Rect::new(100, 100, 400, 300);
        let hit = |x: f64, y: f64| frame_hit(rect, x, y, 0).map(Edges::cursor_name);
        assert_eq!(hit(95.0, 250.0), Some("ew-resize"));
        assert_eq!(hit(505.0, 250.0), Some("ew-resize"));
        assert_eq!(hit(300.0, 95.0), Some("ns-resize"));
        assert_eq!(hit(300.0, 405.0), Some("ns-resize"));
        assert_eq!(hit(95.0, 95.0), Some("nwse-resize"));
        assert_eq!(hit(505.0, 405.0), Some("nwse-resize"));
        assert_eq!(hit(505.0, 95.0), Some("nesw-resize"));
        assert_eq!(hit(95.0, 405.0), Some("nesw-resize"));
        // A corner reaches 16 px along its edges.
        assert_eq!(hit(110.0, 95.0), Some("nwse-resize"));
        assert_eq!(hit(95.0, 112.0), Some("nwse-resize"));
        // Inside, and away: nothing.
        assert_eq!(hit(101.0, 101.0), None);
        assert_eq!(hit(300.0, 250.0), None);
        assert_eq!(hit(80.0, 250.0), None);
        // A window with no header: its first 4 px resize its top.
        let top = |x: f64, y: f64| frame_hit(rect, x, y, TOP_INSIDE);
        assert_eq!(
            top(300.0, 102.0),
            Some(Edges {
                top: true,
                ..Edges::default()
            })
        );
        assert_eq!(top(300.0, 105.0), None);
    }

    #[test]
    fn resizing_keeps_the_far_edges_and_the_limits() {
        let rect = Rect::new(100, 100, 400, 300);
        let nw = Edges {
            left: true,
            top: true,
            ..Edges::default()
        };
        let out = resized(rect, nw, (-50, 20), (0, 0), (0, 0));
        assert_eq!(out, Rect::new(50, 120, 450, 280));
        assert_eq!(out.right(), rect.right());
        // Never below the minimum, the far edge still in place.
        let out = resized(rect, nw, (390, 0), (200, 100), (0, 0));
        assert_eq!(out, Rect::new(300, 100, 200, 300));
        // Nor above the maximum.
        let se = Edges {
            right: true,
            bottom: true,
            ..Edges::default()
        };
        assert_eq!(
            resized(rect, se, (500, 500), (0, 0), (600, 0)),
            Rect::new(100, 100, 600, 800)
        );
        assert_eq!(
            Edges::nearest_corner(rect, 480.0, 120.0),
            Edges {
                right: true,
                top: true,
                ..Edges::default()
            }
        );
    }

    #[test]
    fn a_window_torn_off_a_zone_keeps_the_place_it_was_held_at() {
        // Held at a quarter of its width, 10 px down; 800 wide before.
        assert_eq!(
            torn_off((800, 600), 0.25, (10.0, 1000), 1000.0, 300.0),
            (800, 290)
        );
        // Held further down than it will reach: the same share of it.
        assert_eq!(
            torn_off((800, 600), 0.5, (900.0, 1000), 1000.0, 1000.0),
            (600, 460)
        );
    }

    #[test]
    fn the_keys_snap_to_halves_quarters_and_back() {
        let area = Rect::new(8, 8, 1700, 1064);
        let floating = Rect::new(100, 100, 400, 300);
        let snap = |s, mode| keyboard_snap(s, mode, floating, area);
        assert_eq!(
            snap(Snap::Half(Side::Start), Mode::Floating),
            SnapResult::To(Zone::Left)
        );
        // Again: to the other side.
        assert_eq!(
            snap(Snap::Half(Side::Start), Mode::Snapped(Zone::Left)),
            SnapResult::To(Zone::Right)
        );
        assert_eq!(
            snap(Snap::Maximize, Mode::Floating),
            SnapResult::To(Zone::Maximized)
        );
        assert_eq!(
            snap(Snap::Restore, Mode::Snapped(Zone::Maximized)),
            SnapResult::Restore
        );
        assert_eq!(snap(Snap::Restore, Mode::Floating), SnapResult::Minimize);
        // Quarters keep the other way's side: a left half goes up to the
        // top-left quarter, which goes right to the top-right one.
        assert_eq!(
            snap(Snap::QuarterTop, Mode::Snapped(Zone::Left)),
            SnapResult::To(Zone::TopLeft)
        );
        assert_eq!(
            snap(Snap::QuarterRight, Mode::Snapped(Zone::TopLeft)),
            SnapResult::To(Zone::TopRight)
        );
        // A floating window in the top left of the area.
        assert_eq!(
            snap(Snap::QuarterBottom, Mode::Floating),
            SnapResult::To(Zone::BottomLeft)
        );
    }

    #[test]
    fn a_snapped_pair_resizes_together() {
        let area = Rect::new(8, 8, 1700, 1064);
        let left = Zone::Left.rect(area);
        let right = Zone::Right.rect(area);
        let others = [(WindowId(2), right)];
        let east = Edges {
            right: true,
            ..Edges::default()
        };
        let found = partners(left, east, &others);
        assert_eq!(found.len(), 1);
        let moved = resized(left, east, (100, 0), (0, 0), (0, 0));
        let after = follow(right, found[0].1, moved);
        assert_eq!(after.x, moved.right() + GAP);
        assert_eq!(after.right(), right.right());
        // Its other edges do not pull the neighbour.
        let west = Edges {
            left: true,
            ..Edges::default()
        };
        assert!(partners(left, west, &others).is_empty());
    }

    #[test]
    fn a_seam_is_where_two_windows_face_across_the_gap() {
        let area = Rect::new(8, 8, 1700, 1064);
        let (l, tr, br) = (WindowId(1), WindowId(2), WindowId(3));
        let left = (l, Zone::Left.rect(area));
        let top_right = (tr, Zone::TopRight.rect(area));
        let bottom_right = (br, Zone::BottomRight.rect(area));
        // Given either way, the left one comes first; the half spans the
        // whole seam, so a drag of its handle resizes the half and both
        // quarters follow.
        let s = seam(top_right, left).expect("a seam");
        assert_eq!((s.first, s.second, s.vertical), (l, tr, true));
        assert_eq!((s.grab, s.edges.right), (l, true));
        assert_eq!((s.from, s.to), (top_right.1.y, top_right.1.bottom()));
        assert_eq!(s.centre().0, f64::from(left.1.right() + GAP / 2));
        // The quarters, one above the other: a seam across.
        let s = seam(bottom_right, top_right).expect("a seam");
        assert_eq!((s.first, s.vertical, s.edges.bottom), (tr, false, true));
        // Apart: none.
        assert!(seam(left, (WindowId(4), Rect::new(1000, 100, 200, 200))).is_none());
        // In the gap, the nearer window's edge; at the span's ends, none.
        let s = seam(left, (tr, Zone::Right.rect(area))).expect("a seam");
        let x = f64::from(s.across);
        assert_eq!(s.edge_at(x + 1.0, 500.0).map(|(id, _)| id), Some(l));
        assert_eq!(s.edge_at(x + 7.0, 500.0).map(|(id, _)| id), Some(tr));
        assert!(s.edge_at(x + 1.0, f64::from(s.from + 2)).is_none());
        let (cx, cy) = s.centre();
        assert!(s.on_handle(cx + 5.0, cy + 25.0, 52.0, 12.0));
        assert!(!s.on_handle(cx, cy + 40.0, 52.0, 12.0));
    }

    #[test]
    fn focus_raises_and_follows_the_top_when_its_window_goes() {
        let mut model = Model::default();
        let a = model.add();
        let b = model.add();
        assert_eq!(model.focused(), None);
        model.focus(Some(a));
        assert_eq!(model.stack(), &[b, a]);
        assert_eq!(model.focused(), Some(a));
        model.remove(a);
        assert_eq!(model.focused(), Some(b));
        // A window not in the model is never focused.
        model.focus(Some(WindowId(99)));
        assert_eq!(model.focused(), Some(b));
        model.focus(None);
        assert_eq!(model.focused(), None);
        assert_eq!(model.stack(), &[b]);
    }

    #[test]
    fn a_window_that_does_not_take_the_focus_opens_behind() {
        let mut model = Model::default();
        let a = model.add();
        model.focus(Some(a));
        let b = model.add_below_focus();
        assert_eq!(model.stack(), &[b, a]);
        assert_eq!(model.focused(), Some(a));
    }

    #[test]
    fn minimised_windows_leave_the_focus() {
        let mut model = Model::default();
        let a = model.add();
        let b = model.add();
        model.focus(Some(b));
        model.minimize(b);
        assert_eq!(model.focused(), Some(a));
        model.minimize(a);
        assert_eq!(model.focused(), None);
        model.unminimize(a);
        assert!(!model.is_minimized(a) && model.is_minimized(b));
    }

    #[test]
    fn alt_tab_lists_the_current_desktop_first_then_the_others_most_recent_first() {
        let mut model = Model::default();
        let a = model.add();
        let b = model.add();
        let c = model.add();
        let d = model.add();
        model.set_desktop(c, 1);
        model.set_hidden(c, true);
        model.focus(Some(a));
        model.focus(Some(d));
        model.focus(Some(b));
        model.minimize(d);
        // b was used last, then d (minimised now), a; c is on desktop 2.
        let on_first = |w: WindowId| model.desktop(w) == 0;
        assert_eq!(model.switcher_order(on_first), [b, d, a, c]);
        assert_eq!(model.recent(), [b, d, a, c]);
        // A window opened behind the focused one comes right after it.
        let e = model.add_below_focus();
        assert_eq!(model.recent()[..2], [b, e]);
    }

    #[test]
    fn a_hidden_window_gives_the_focus_to_the_top_window_that_shows() {
        let mut model = Model::default();
        let a = model.add();
        let b = model.add();
        model.focus(Some(b));
        model.set_desktop(b, 2);
        model.set_hidden(b, true);
        assert!(!model.is_shown(b));
        assert_eq!(model.focused(), Some(a));
        assert_eq!(model.top_shown(), Some(a));
        model.set_hidden(a, true);
        assert_eq!(model.focused(), None);
        model.set_hidden(b, false);
        assert_eq!(model.top_shown(), Some(b));
    }

    #[test]
    fn leaving_floating_keeps_where_to_come_back() {
        let mut model = Model::default();
        let a = model.add();
        let floating = Rect::new(10, 10, 300, 200);
        model.set_mode(a, Mode::Snapped(Zone::Left), floating);
        model.set_mode(a, Mode::Snapped(Zone::Maximized), Rect::default());
        assert_eq!(model.unsnap(a), Some(floating));
        assert_eq!(model.mode(a), Mode::Floating);
    }
}
