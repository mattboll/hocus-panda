//! What the shell's surfaces report to the controller, and what it
//! answers.

use super::*;

/// What the shell's surfaces report.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The pointer is on `item` of `bar`, which is at `rect` (bar frame).
    ItemEnter {
        bar: String,
        item: Item,
        rect: Rectangle,
    },
    /// It left the item; last seen at `at`.
    ItemLeave {
        at: Point,
    },
    /// A press on `item` of `bar`, at `rect`.
    ItemPress {
        bar: String,
        item: Item,
        rect: Rectangle,
    },
    /// The pointer seen at `at` on a bar's surfaces, off its items.
    Pointer {
        at: Point,
    },
    PopoverEnter,
    PopoverLeave,
    /// A press on the bar's surfaces, off its items and the popover.
    PressElsewhere,
    /// A press on none of the shell's surfaces (the compositor's
    /// `outside`).
    Outside,
    /// `Esc` in the popover.
    Escape,
    /// The popover, opened by hovering, is now worked in (a password to
    /// type): it stays and takes the keyboard, as if a click had opened it.
    Hold,
    /// `Enter` or `Space` on the item holding the keyboard.
    ItemActivate,
    /// `Tab` (or down) on the item holding the keyboard: to the next item
    /// of its surface; `back`, `Shift+Tab` (or up): to the previous.
    ItemStep {
        back: bool,
    },
    /// `Home` (`last`: `End`) on the item holding the keyboard: to the
    /// bar's first item, or its last.
    ItemEdge {
        last: bool,
    },
    /// `←` or `→` (`right`) on the item holding the keyboard: along its
    /// row (the desktops' tiles, the footer, the tray); on a level, its
    /// value ∓5 % without opening it.
    ItemSide {
        right: bool,
    },
    /// `Esc` on the item holding the keyboard.
    ItemRelease,
    /// `bar` changed form, or went.
    BarChanged {
        bar: String,
    },
    /// What the popover offered was chosen (a window of a desktop's list):
    /// it closes.
    Chosen,
    /// The popover gives way to `item`'s, beside the same item (Sources'
    /// "Brancher": the consent popover; the briefing's "Sources").
    Replace(Item),
    /// `item`'s popover opens, from `rect` on `bar`, as a click opens it,
    /// what was open closing: a tray icon's menu, once read.
    Open {
        bar: String,
        item: Item,
        rect: Rectangle,
    },
    /// A line of the popover names agent `id` (the briefing's): it closes,
    /// and the compositor goes to the agent, the streak from the line, at
    /// `row` in the popover.
    GoTo {
        id: u64,
        row: Rectangle,
    },
}

/// What the controller asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// A popover's surface for `item`, on `bar`'s output: `size`, and its
    /// layer-shell margins (top, right, bottom, left).
    Open {
        id: window::Id,
        bar: String,
        item: Item,
        size: (u32, u32),
        margin: (i32, i32, i32, i32),
        keyboard: KeyboardInteractivity,
    },
    Keyboard(window::Id, KeyboardInteractivity),
    /// The popover's surface to a new size, and its new margins if they
    /// change.
    Resize {
        id: window::Id,
        size: (u32, u32),
        margin: Option<(i32, i32, i32, i32)>,
    },
    /// The compositor fades it out (`closing`).
    Closing,
    /// Popover `id`, opened by the keyboard, puts its first control under
    /// the ring (as `Tab` would).
    FirstControl(window::Id),
    Remove(window::Id),
    /// The surfaces of this namespace show something new.
    Wake(&'static str),
    /// Every surface's view built again (a view of stock widgets reads
    /// what it shows as it is built, not as it draws).
    Rebuild,
    /// Surface `id` takes the pointer (`takes`), or lets it through.
    Input {
        id: window::Id,
        takes: bool,
    },
    /// The compositor is told where the keyboard's ring is on `bar` (its
    /// frame), for the magnifier.
    Ring {
        bar: String,
        rect: Rectangle,
    },
    /// The compositor is asked for the next keyboard layout.
    NextLayout,
    /// A level steps, as its key on the keyboard does (`←`/`→` on the
    /// sound's or the brightness's item).
    Level(hocus_link::LevelKey),
    /// The compositor is asked for a desktop of `bar`'s screen.
    Desktop {
        bar: String,
        to: DesktopTo,
    },
    /// The compositor is asked for a window: its desktop, and the focus.
    Bring(u64),
    /// The compositor is asked for an agent: its window, and it is seen;
    /// the streak runs from its row (`row`, in `bar`'s frame).
    GoToAgent {
        id: u64,
        bar: String,
        row: Rectangle,
    },
    /// The winking panda clicked: the compositor sends it back.
    PeekClose,
    /// The compositor plugs a tool in, or unplugs it (4.0).
    Plug(crate::agents::Tool),
    Unplug(crate::agents::Tool),
    /// The compositor declines the tools listed ("Ne rien brancher").
    Decline,
    /// The compositor follows herdr again.
    FollowHerdr,
    /// The secretary's briefing opened (`true`) or closed: the compositor
    /// sends a panda peeking out back, and keeps others in while it is
    /// open (4.7b).
    Briefing(bool),
    /// The clipboard takes this text.
    Copy(String),
    /// The launcher opens on `bar`'s screen, or closes (the footer's menu).
    ToggleLauncher(String),
    /// Tray icon `key`'s menu is read, then opens beside it at `rect`.
    TrayMenu {
        bar: String,
        key: u32,
        rect: Rectangle,
    },
    /// A layer surface of the shell's beside `bar`, of `namespace` (the
    /// peek, 4.5): `size`, its margins from the screen's top right (top,
    /// right, bottom, left); it takes no keyboard.
    Layer {
        id: window::Id,
        bar: String,
        namespace: &'static str,
        size: (u32, u32),
        margin: (i32, i32, i32, i32),
    },
    /// Surface `id` takes the pointer in `rects` only (its own pixels).
    Region {
        id: window::Id,
        rects: Vec<Rectangle>,
    },
    /// The launcher's surface on `output` (5.1): over all of it, the bar
    /// too, holding the keyboard.
    Launcher {
        id: window::Id,
        output: String,
    },
    /// The launcher's toast on `output` (5.2), `size` large: at its
    /// bottom, taking neither the keyboard nor the pointer.
    Toast {
        id: window::Id,
        output: String,
        size: (u32, u32),
    },
    /// An MCP agent's tag (docs/design/mcp.md) on `output`, `size`
    /// large, its top left at `at` (logical, from the screen's), taking
    /// neither the keyboard nor the pointer; or its toast (`toast`), which
    /// takes the pointer for its buttons.
    Agent {
        id: window::Id,
        output: String,
        size: (u32, u32),
        at: (i32, i32),
        kind: crate::mcp::Surface,
    },
    /// The actions palette's surface on `output` (#736): `size`, its
    /// margins from the screen's top left corner (top, right, bottom,
    /// left), holding the keyboard.
    Actions {
        id: window::Id,
        output: String,
        size: (u32, u32),
        margin: (i32, i32, i32, i32),
    },
    /// The emoji picker's surface on `output` (5.7): `size`, its margins
    /// from the screen's top left corner (top, right, bottom, left),
    /// holding the keyboard.
    Emoji {
        id: window::Id,
        output: String,
        size: (u32, u32),
        margin: (i32, i32, i32, i32),
    },
    /// A line to the compositor.
    Tell(ToCompositor),
    /// The calendar opens on `bar` on `day`, `event` chosen (Day.5): from
    /// what acts within the controller (the next meeting's card, Today's
    /// lines), carried out after it.
    OpenCalendar {
        bar: String,
        day: jiff::civil::Date,
        event: Option<hocus_day::model::EventId>,
    },
    /// The capture's toolbar on `output` (5.8), `size` large: at its
    /// bottom, taking the pointer, never the keyboard.
    CaptureBar {
        id: window::Id,
        output: String,
        size: (u32, u32),
    },
    /// The share's chooser on `output` (5.11), `size` large: centred on
    /// it, over everything, holding the keyboard.
    ShareChooser {
        id: window::Id,
        output: String,
        size: (u32, u32),
    },
    /// The authentication dialog on `output` (7.5), `size` large: centred
    /// across it, `top` from its top, over everything, holding the
    /// keyboard.
    Auth {
        id: window::Id,
        output: String,
        size: (u32, u32),
        top: i32,
    },
    /// The capture's thumbnail on `output` (5.9), `size` large, its
    /// margins from the screen's bottom right corner (top, right, bottom,
    /// left): taking the pointer, never the keyboard.
    CaptureThumb {
        id: window::Id,
        output: String,
        size: (u32, u32),
        margin: (i32, i32, i32, i32),
    },
    /// The capture's editor on `output` (5.9): all of it, over everything,
    /// holding the keyboard.
    CaptureEditor {
        id: window::Id,
        output: String,
    },
    /// What the to-do list's group does (Day.4): the field opens, a box
    /// ticks.
    Tasks(crate::tasks::Act),
    /// The lock's surface on `output` (7.1): all of it, over everything,
    /// never the keyboard (the compositor keeps it while locked).
    LockSurface {
        id: window::Id,
        output: String,
    },
    /// The curtain's pill on `output` (7.2): 300 × 64, centred across it,
    /// `top` from its top, over everything, taking neither the pointer nor
    /// the keyboard.
    CurtainSurface {
        id: window::Id,
        output: String,
        top: i32,
    },
}
