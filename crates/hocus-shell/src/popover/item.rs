//! What opens a popover: the bar's items, their sizes and surfaces.

use super::*;

/// What opens a popover, and the keyboard layout's item, which opens none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    Sound,
    Wifi,
    Bluetooth,
    Brightness,
    /// The battery and the power modes (the modes alone without a
    /// battery).
    Battery,
    /// The keyboard's layout: a click, or `Enter` while it holds the
    /// keyboard, asks for the next one (3.8).
    Keyboard,
    /// A desktop's tile (or dot) of the bar's row `row` (its screen's, 8.4:
    /// 0 the bar's own), desktop `index` from 0: a click, or `Enter` while
    /// it holds the keyboard, goes to that desktop of that screen (3.9);
    /// hovered, it shows the list of its windows (3.9b).
    Desktop {
        row: u8,
        index: u8,
    },
    /// The Sources popover (4.0): from the secretary's briefing, which it
    /// replaces (4.6), and the card's "Un autre outil ?".
    Sources,
    AnotherTool,
    /// A card's "Brancher": the consent popover for that tool.
    Plug(crate::agents::Tool),
    /// The card's "Ne rien brancher": a click declines the tools listed.
    Decline,
    /// An agent's row (4.1), by the compositor's id: a click, or `Enter`
    /// while it holds the keyboard, goes to the agent.
    Agent(u64),
    /// The secretary, at the head of the agents' group (4.6): her
    /// briefing, from which Sources opens.
    Secretary,
    /// The footer's bell (5.4, 5.5): the notifications' history.
    Notifications,
    /// The footer's clipboard (5.5): its history (5.6), which `Super+V`
    /// opens too.
    Clipboard,
    /// The footer's menu (5.5): a click, or `Enter` while it holds the
    /// keyboard, opens the launcher (5.1); hovering it opens nothing.
    Menu,
    /// The footer's user (5.5): their name and logging out (the session's
    /// menu, 7.4).
    User,
    /// The sign of what is in use, under the clock (5.11, 5.12): who uses
    /// the microphone, the camera or the screen; muting the microphone,
    /// stopping a share or the recording.
    InUse,
    /// A tray icon (5.14), by its item's key: its menu, read before it
    /// opens (a right click, or `Enter` while it holds the keyboard).
    Tray(u32),
    /// The next meeting's card (Day.2): a click, or `Enter`, opens the
    /// calendar (Day.5) on today, its meeting chosen.
    Meeting,
    /// The card's join button: a click, or `Enter`, joins its video call.
    Join,
    /// The mini form's meeting mark: its popover, by a hover or a click.
    MeetingMark,
    /// A line of Today (Day.3), by its row from the top: a click, or
    /// `Enter`, opens its call, else the calendar on its event.
    TodayLine(u8),
    /// Today's "+ N autres aujourd'hui": Day.5's calendar on today.
    TodayMore,
    /// The mini form's to-do item (Day.4): the tasks' popover.
    Tasks,
    /// The group's "+ N autres" (Day.4): the same popover, every task.
    TasksMore,
    /// The group's `+`: a click, or `Enter` while it holds the keyboard,
    /// opens the field to add a task.
    TaskAdd,
    /// The box of the group's line, from its top: a click, or `Space` or
    /// `Enter` while it holds the keyboard, ticks it (or unticks it).
    Task(u8),
    /// Day.1's first-run card's "Choisir…": the sources' popover, step by
    /// step.
    DayChoose,
    /// The card's "Plus tard": a click, or `Enter`, removes the card for
    /// good.
    DayLater,
    /// The clock's group, the date and the clock (Day.5): the calendar,
    /// by a click only, in both forms.
    Calendar,
    /// A window's line over the desktops (#604), by the compositor's id: a
    /// click, or `Enter`, focuses and raises it.
    Window(u64),
    /// Its ✕: a click, or `Enter`, asks it to close.
    WindowClose(u64),
    /// The windows' "+ N autres": the overview (8.5 A), by a click or
    /// `Enter`.
    WindowsMore,
    /// The mini form's windows' item: their popover, by a hover or a
    /// click.
    Windows,
    /// A window's menu (#723), by the compositor's id: opened by a right
    /// click on its line, once the compositor said what it shows.
    WindowMenu(u64),
    /// The music player's tile over the desktops (s12): its popover, by a
    /// click.
    Music,
}

impl Item {
    /// Whether a click (or `Enter`) opens its popover; else it acts.
    pub(super) fn clicks(self) -> bool {
        !matches!(
            self,
            Self::Keyboard
                | Self::Desktop { .. }
                | Self::Decline
                | Self::Agent(_)
                | Self::Menu
                | Self::Tray(_)
                | Self::Meeting
                | Self::Join
                | Self::TodayLine(_)
                | Self::TodayMore
                | Self::TaskAdd
                | Self::Task(_)
                | Self::DayLater
                | Self::Window(_)
                | Self::WindowClose(_)
                | Self::WindowsMore
        )
    }

    /// Whether it is the agents' group's.
    pub(super) fn agents(self) -> bool {
        matches!(
            self,
            Self::Sources | Self::AnotherTool | Self::Plug(_) | Self::Decline
        )
    }

    /// Whether hovering it opens its popover, in `form`: a desktop's list
    /// and the footer's clipboard, bell and user in both (5.5), the others
    /// in the mini form; the agents' mini item (4.7), her briefing or
    /// Sources on the card, as the others. The footer's menu never: a
    /// screen-wide launcher must not open on the way past.
    pub(super) fn hovers(self, form: Form) -> bool {
        match self {
            Self::Desktop { .. }
            | Self::Clipboard
            | Self::Notifications
            | Self::User
            | Self::InUse => true,
            Self::Keyboard
            | Self::Agent(_)
            | Self::Menu
            | Self::Tray(_)
            | Self::Meeting
            | Self::Join
            | Self::TodayLine(_)
            | Self::TodayMore => false,
            Self::TasksMore | Self::TaskAdd | Self::Task(_) => false,
            Self::Window(_) | Self::WindowClose(_) | Self::WindowsMore => false,
            Self::DayChoose | Self::DayLater => false,
            // Resting on the clock stays harmless (Day.5).
            Self::Calendar => false,
            Self::Secretary | Self::Sources => form == Form::Mini,
            item if item.agents() => false,
            _ => form == Form::Mini,
        }
    }

    /// Its popover's width, logical pixels.
    pub(super) fn width(self) -> u32 {
        match self {
            Self::Desktop { .. } => desktop::WIDTH,
            Self::Secretary => briefing::WIDTH,
            Self::Notifications => history::WIDTH,
            Self::Clipboard => clipboard::WIDTH,
            Self::User => session::WIDTH,
            Self::InUse => in_use::WIDTH,
            Self::Tray(_) => tray::WIDTH,
            Self::MeetingMark => meeting::WIDTH,
            Self::Tasks | Self::TasksMore => tasks::WIDTH,
            Self::DayChoose => setup::WIDTH,
            Self::Calendar => calendar::WIDTH,
            Self::Windows => windows::WIDTH,
            Self::WindowMenu(_) => window_menu::WIDTH,
            Self::WindowsMore => desktop::WIDTH,
            Self::Music => music::WIDTH,
            item if item.agents() => agents::WIDTH,
            _ => POPOVER_WIDTH.unsigned_abs(),
        }
    }

    /// Its popover's height on `bar`, in `form`, logical pixels, for what
    /// it shows now: as it opens (`opening`), or after a change.
    pub(super) fn height(self, bar: &str, form: Form, opening: bool) -> u32 {
        match self {
            Self::Sound => sound::layout(opening).height(),
            Self::Wifi => wifi::height(),
            Self::Bluetooth => bluetooth::height(),
            Self::Brightness => brightness::height(),
            Self::Battery => battery::height(),
            Self::Desktop { row, index } => desktop::height(bar, row, index),
            Self::Sources | Self::AnotherTool => agents::sources_height(form),
            Self::Plug(tool) => agents::consent_height(tool),
            Self::Secretary => briefing::height(form),
            Self::Notifications => history::height(),
            Self::Clipboard => clipboard::height(),
            Self::User => session::height(),
            Self::InUse => in_use::height(),
            Self::Tray(key) => tray::height(key),
            Self::Calendar => calendar::height(),
            Self::MeetingMark => meeting::height(),
            Self::Tasks | Self::TasksMore => tasks::height(),
            Self::DayChoose => setup::height(),
            Self::Windows => windows::height(bar),
            Self::WindowMenu(id) => window_menu::height(id),
            Self::Music => music::height(),
            Self::WindowsMore => desktop::height(
                bar,
                0,
                u8::try_from(crate::desktops::current(bar)).unwrap_or(u8::MAX),
            ),
            Self::Keyboard
            | Self::Decline
            | Self::Agent(_)
            | Self::Menu
            | Self::Meeting
            | Self::Join
            | Self::TodayLine(_)
            | Self::TodayMore
            | Self::TaskAdd
            | Self::Task(_)
            | Self::DayLater
            | Self::Window(_)
            | Self::WindowClose(_) => 0,
        }
    }

    /// Whether its surface draws it lit while its popover shows: a tile's
    /// look does not change (it is lit by the pointer already); Sources
    /// lights the mini form's agents' item, which opens it on the card.
    pub(super) fn lit_while_open(self) -> bool {
        !matches!(self, Self::Desktop { .. }) && (!self.agents() || self == Self::Sources)
    }

    /// Its popover opens: what it shows starts afresh.
    pub(super) fn opening(self) {
        match self {
            Self::Sound => {}
            Self::Wifi => wifi::opening(),
            Self::Bluetooth => bluetooth::opening(),
            Self::Brightness => brightness::opening(),
            Self::Battery => battery::opening(),
            Self::Desktop { .. } | Self::WindowsMore => desktop::opening(),
            Self::Secretary => briefing::opening(),
            Self::Keyboard | Self::Agent(_) => {}
            Self::Sources | Self::AnotherTool | Self::Plug(_) => agents::opening(),
            Self::Decline => {}
            Self::Notifications => {
                history::opening();
                crate::notifications::history_open(true);
            }
            Self::Clipboard => clipboard::opening(),
            Self::User => session::opening(),
            Self::InUse => in_use::opening(),
            Self::Menu | Self::TaskAdd | Self::Task(_) => {}
            Self::MeetingMark => meeting::opening(),
            Self::Meeting | Self::Join => {}
            Self::TodayLine(_) | Self::TodayMore => {}
            Self::Tray(_) => tray::opening(),
            Self::Tasks | Self::TasksMore => tasks::opening(),
            Self::DayChoose => {
                setup::opening();
                crate::setup::opening();
            }
            Self::DayLater => {}
            Self::Calendar => calendar::opening(),
            Self::Music => music::opening(),
            Self::Windows => windows::opening(),
            Self::WindowMenu(_) => window_menu::opening(),
            Self::Window(_) | Self::WindowClose(_) => {}
        }
    }

    /// Its popover closes: what it kept goes (a password typed).
    pub(super) fn closing(self) {
        match self {
            Self::Sound
            | Self::Brightness
            | Self::Battery
            | Self::Keyboard
            | Self::Desktop { .. }
            | Self::Sources
            | Self::AnotherTool
            | Self::Plug(_)
            | Self::Decline
            | Self::Agent(_)
            | Self::Secretary
            | Self::Menu
            | Self::InUse
            | Self::Tray(_)
            | Self::Meeting
            | Self::Join
            | Self::MeetingMark
            | Self::TodayLine(_)
            | Self::TodayMore
            | Self::TaskAdd
            | Self::Task(_)
            | Self::DayLater
            | Self::Clipboard
            | Self::Window(_)
            | Self::WindowClose(_)
            | Self::WindowsMore
            | Self::Windows
            | Self::WindowMenu(_)
            | Self::Music => {}
            Self::Tasks | Self::TasksMore => tasks::closing(),
            Self::DayChoose => crate::setup::closing(),
            Self::Wifi => wifi::closing(),
            Self::Bluetooth => bluetooth::closing(),
            Self::Notifications => crate::notifications::history_open(false),
            Self::User => session::closing(),
            Self::Calendar => {}
        }
    }

    /// The namespace of the surface it is on in `form`, drawn again as
    /// its popover opens and closes: the keyboard layout's is the
    /// connections' row in the full form, the levels' button in the mini.
    pub(super) fn namespace(self, form: Form) -> &'static str {
        match (self, form) {
            (Self::Sound | Self::Brightness | Self::Battery, _) | (Self::Keyboard, Form::Mini) => {
                LEVELS_NAMESPACE
            }
            (Self::Wifi | Self::Bluetooth, _) | (Self::Keyboard, Form::Full) => {
                CONNECTIONS_NAMESPACE
            }
            (Self::Desktop { .. }, _) => DESKTOPS_NAMESPACE,
            (Self::Notifications | Self::Clipboard | Self::Menu | Self::User, _) => {
                FOOTER_NAMESPACE
            }
            (Self::InUse, _) => IN_USE_NAMESPACE,
            (Self::Tray(_), _) => TRAY_NAMESPACE,
            (Self::Meeting | Self::Join | Self::MeetingMark, _) => MEETING_NAMESPACE,
            (Self::TodayLine(_) | Self::TodayMore, _) => TODAY_NAMESPACE,
            (
                Self::Tasks
                | Self::TasksMore
                | Self::TaskAdd
                | Self::Task(_)
                | Self::DayChoose
                | Self::DayLater,
                _,
            ) => TASKS_NAMESPACE,
            (Self::Calendar, _) => CLOCK_NAMESPACE,
            (Self::Music, _) => MUSIC_NAMESPACE,
            (
                Self::Window(_)
                | Self::WindowClose(_)
                | Self::WindowsMore
                | Self::Windows
                | Self::WindowMenu(_),
                _,
            ) => WINDOWS_NAMESPACE,
            (
                Self::Sources
                | Self::AnotherTool
                | Self::Plug(_)
                | Self::Decline
                | Self::Agent(_)
                | Self::Secretary,
                _,
            ) => AGENTS_NAMESPACE,
        }
    }

    /// Whether its popover's bottom sits on the bar's whatever its height,
    /// as the 3.5, 3.6 and 3.7 mockups draw the Wi-Fi's, Bluetooth's, the
    /// brightness's and the battery's (else its top is on the item's,
    /// pushed up to stay inside the bar: a desktop's list's on the tiles').
    pub(super) fn on_bottom(self) -> bool {
        // The calendar's top on the clock's group's (Day.5).
        if self == Self::Calendar {
            return false;
        }
        !matches!(
            self,
            Self::Sound
                | Self::Desktop { .. }
                | Self::Secretary
                | Self::InUse
                | Self::Tray(_)
                | Self::MeetingMark
                | Self::Tasks
                | Self::TasksMore
                | Self::DayChoose
                | Self::Windows
                | Self::WindowsMore
                | Self::WindowMenu(_)
        ) && !self.agents()
    }

    /// Its surface on the bar at `place`, which holds the keyboard after
    /// `Esc` in its popover.
    pub(super) fn surface(self, place: &Place) -> window::Id {
        match self.namespace(place.form) {
            LEVELS_NAMESPACE => place.levels,
            DESKTOPS_NAMESPACE => place.desktops,
            AGENTS_NAMESPACE => place.agents,
            FOOTER_NAMESPACE => place.footer.unwrap_or(place.levels),
            IN_USE_NAMESPACE => place.in_use.unwrap_or(place.levels),
            MEETING_NAMESPACE => place.meeting.unwrap_or(place.levels),
            TODAY_NAMESPACE => place.today.unwrap_or(place.levels),
            TRAY_NAMESPACE => place.tray.unwrap_or(place.levels),
            TASKS_NAMESPACE => place.tasks.unwrap_or(place.levels),
            WINDOWS_NAMESPACE => place.windows.unwrap_or(place.levels),
            // The bar's own surface hears the clock's group's keys.
            CLOCK_NAMESPACE => place.surface,
            _ => place.connections,
        }
    }
}
