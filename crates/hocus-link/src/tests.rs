//! The messages on the line: every kind goes and comes back the same, a
//! side of another version is told apart at its first line, and what does
//! not read is skipped.

use std::{collections::BTreeSet, time::Duration};

use serde::de::DeserializeOwned;

use super::*;
use crate::wire::{hello, hello_of, said_version};

/// Messages to the shell, one of every kind at least: a kind without one
/// here fails [`every_kind_reads_back_as_written`].
fn to_shell() -> Vec<ToShell> {
    let rect = Rect {
        x: 120,
        y: -4,
        width: 2,
        height: 18,
    };
    let menu = WindowMenu {
        id: 12,
        maximized: true,
        above: false,
        all_desktops: true,
        screens: vec![
            MenuScreen {
                output: "eDP-1".into(),
                count: 4,
                current: 1,
                width: 1707,
                height: 1067,
                here: Some(0),
                internal: true,
                name: "eDP-1".into(),
            },
            MenuScreen {
                output: "DP-2".into(),
                count: 2,
                current: 0,
                width: 3440,
                height: 1440,
                here: None,
                internal: false,
                name: "Dell U3419W wide".into(),
            },
        ],
    };
    let mut all = vec![
        ToShell::Copied {
            app: "kitty".into(),
            clip: Clip {
                kind: ClipKind::Text,
                data: "cargo nextest run\nla suite".as_bytes().to_vec(),
            },
        },
        ToShell::Copied {
            app: String::new(),
            clip: Clip {
                kind: ClipKind::Png,
                data: (0..=255).collect(),
            },
        },
        ToShell::Copied {
            app: "org.gnome.Nautilus".into(),
            clip: Clip {
                kind: ClipKind::Text,
                data: Vec::new(),
            },
        },
        ToShell::CaptureCopied {
            capture: 3,
            clip: Clip {
                kind: ClipKind::Png,
                data: (0..=255).rev().collect(),
            },
        },
        ToShell::Clipboard {
            output: "eDP-1".into(),
        },
        ToShell::ClipboardRules(ClipRules::default()),
        ToShell::ClipboardRules(ClipRules {
            history: false,
            keep: ClipKeep::Session,
        }),
        ToShell::ClipboardClear,
        ToShell::SessionMenu {
            output: "eDP-1".into(),
        },
        ToShell::DropOver {
            bar: "eDP-1".into(),
            over: Some(("DP-1".into(), 3)),
        },
        ToShell::DropOver {
            bar: "eDP-1".into(),
            over: None,
        },
        ToShell::Emoji {
            output: "eDP-1".into(),
            scale: 180,
            width: 1707,
            height: 1067,
            at: EmojiAt::Caret(rect),
        },
        ToShell::Emoji {
            output: "HDMI-A-1".into(),
            scale: 120,
            width: 1920,
            height: 1080,
            at: EmojiAt::Window(rect),
        },
        ToShell::Emoji {
            output: "HDMI-A-1".into(),
            scale: 120,
            width: 1920,
            height: 1080,
            at: EmojiAt::Screen,
        },
        ToShell::EmojiInserted,
        ToShell::TrayToken {
            ask: 7,
            token: "Xq3vZk0aPAbtUJjJ2hF0cO1DzQ9m4wKe".into(),
        },
        ToShell::Exported {
            ask: 2,
            handle: Some("Rk2v8QwZp0aPAbtUJjJ2hF0cO1DzQ9m4".into()),
        },
        ToShell::Exported {
            ask: 3,
            handle: None,
        },
        ToShell::Look {
            glass: true,
            motion: false,
            lively: false,
        },
        ToShell::Look {
            glass: false,
            motion: true,
            lively: false,
        },
        ToShell::Theme(theme::Choice {
            scheme: theme::Scheme::Light,
            accent: theme::Accent::Violet,
            wallpaper: theme::AccentColours::lifted([0x3A, 0x2A, 0x26]),
            contrast: true,
        }),
        ToShell::Wallpaper(wallpaper::Wallpapers {
            lock: wallpaper::LockLook::Picture,
            lock_picture: wallpaper::Picture::File("~/a b.jpg".into()),
            picture: wallpaper::Picture::Builtin(backdrop::Backdrop::Forest),
            fit: wallpaper::Fit::Center,
            color: wallpaper::DEFAULT_COLOR,
            outputs: vec![("eDP-1".into(), wallpaper::Picture::Plain)],
        }),
        ToShell::Locale(locale::Locale {
            language: locale::Language::English,
            date: locale::DateFormat::Short,
            time: Some(locale::TimeFormat::H12),
        }),
        ToShell::Bar {
            output: "eDP-1".into(),
            form: Form::Mini,
            since: 123_456_789,
            scale: 180,
        },
        ToShell::NoBar {
            output: "DP-2".into(),
        },
        ToShell::Clock(ClockLook::default()),
        ToShell::Clock(ClockLook {
            font: "JetBrains Mono".into(),
            color: [0x3D, 0xFF, 0xA2],
            transition: Transition::Flip,
            seconds: false,
        }),
        ToShell::Day(DayAsk::Add("Acheter du pain, et du café".into())),
        ToShell::Day(DayAsk::Tick("18a2f-3".into())),
        ToShell::Day(DayAsk::Untick("c 2".into())),
        ToShell::Day(DayAsk::Refresh),
        ToShell::Day(DayAsk::Slept(900)),
        ToShell::Join,
        ToShell::FellBack {
            which: Supervised::Shell,
            program: "/home/alex/my builds/hocuspanda-shell".into(),
        },
        ToShell::Fullscreen(vec!["HDMI-A-1".to_owned(), "eDP-1".to_owned()]),
        ToShell::Fullscreen(Vec::new()),
        ToShell::SharedScreens(vec!["HEADLESS-1".to_owned()]),
        ToShell::SharedScreens(Vec::new()),
        ToShell::Day(DayAsk::Calendar {
            day: "2026-11-10".into(),
            event: Some("e 12".into()),
        }),
        ToShell::Day(DayAsk::Calendar {
            day: "2026-11-04".into(),
            event: None,
        }),
        ToShell::Screens(Vec::new()),
        ToShell::Screens(vec!["DELL U3419W".into(), "HDMI-A-1".into()]),
        ToShell::Layouts(Vec::new()),
        ToShell::Layouts(vec![
            KeyLayout {
                code: "fr".into(),
                name: "French".into(),
            },
            KeyLayout {
                code: "fr(bepo)".into(),
                name: "French (BEPO)".into(),
            },
            KeyLayout {
                code: String::new(),
                name: "English (US)".into(),
            },
        ]),
        ToShell::Layout(0),
        ToShell::Layout(2),
        ToShell::Lock(LockShown {
            phase: LockPhase::Waiting(27),
            chars: 0,
            caps: true,
            shake: 3,
            focus: LockFocus::Layout,
            agents: 2,
            wallpaper: true,
            motion: false,
            output: "eDP-1".into(),
        }),
        ToShell::Lock(LockShown {
            phase: LockPhase::Typing,
            chars: 7,
            caps: false,
            shake: 0,
            focus: LockFocus::Field,
            agents: 0,
            wallpaper: false,
            motion: true,
            output: "HDMI-A-1".into(),
        }),
        ToShell::Unlocked,
        ToShell::A11y,
        ToShell::Curtain(CurtainShown {
            elapsed: 0,
            length: 10_000,
            top: 1010,
            motion: true,
            output: "eDP-1".into(),
        }),
        ToShell::Curtain(CurtainShown {
            elapsed: 4_250,
            length: 10_000,
            top: 922,
            motion: false,
            output: "HDMI-A-1".into(),
        }),
        ToShell::CurtainEnd,
        ToShell::Desktops(Desktops {
            output: "eDP-1".into(),
            count: 4,
            current: 0,
            windows: Vec::new(),
        }),
        ToShell::Desktops(Desktops {
            output: "HDMI-A-1".into(),
            count: 9,
            current: 8,
            windows: vec![
                DeskWindow {
                    desktop: 0,
                    rect: [0, 0, SHARE, SHARE],
                    look: Look::Plain,
                    id: 3,
                    app: String::new(),
                },
                DeskWindow {
                    desktop: 8,
                    rect: [20, 30, 600, 940],
                    look: Look::Focused,
                    id: 12,
                    app: "com.example,odd".into(),
                },
                DeskWindow {
                    desktop: 8,
                    rect: [0, 0, 0, 0],
                    look: Look::Minimized,
                    id: 13,
                    app: "foot".into(),
                },
            ],
        }),
        ToShell::Titles(Titles {
            output: "eDP-1".into(),
            titles: Vec::new(),
        }),
        ToShell::Titles(Titles {
            output: "eDP-1".into(),
            titles: vec![(3, "~/hocus — fish".into()), (12, String::new())],
        }),
        ToShell::Recent(Recent {
            output: "eDP-1".into(),
            ids: vec![12, 3, 13],
        }),
        ToShell::Recent(Recent {
            output: "HDMI-A-1".into(),
            ids: Vec::new(),
        }),
        ToShell::BarWindows(true),
        ToShell::BarWindows(false),
        ToShell::DesktopNames(Vec::new()),
        ToShell::DesktopNames(vec!["Travail".into(), String::new(), "Mail · perso".into()]),
        ToShell::BarSide(Side::Left),
        ToShell::BarSide(Side::Right),
        ToShell::BarGroups(BarGroups::default()),
        ToShell::BarGroups(BarGroups {
            order: [BarGroup::Tasks, BarGroup::Today, BarGroup::Agents],
            meeting: false,
            today: true,
            tasks: false,
            agents: true,
        }),
        ToShell::Outside,
        ToShell::Focus("DP-2".into()),
        ToShell::BarKeyboard(Some("DP-2".into())),
        ToShell::BarKeyboard(None),
        ToShell::AgentTools(Vec::new()),
        ToShell::AgentTools(vec![
            AgentTool {
                tool: "claude-code".into(),
                state: ToolState::Found,
                file: "~/.claude/settings.json".into(),
            },
            AgentTool {
                tool: "codex".into(),
                state: ToolState::Plugged,
                file: "/opt/my codex/config.toml".into(),
            },
            AgentTool {
                tool: "herdr".into(),
                state: ToolState::Ignored,
                file: String::new(),
            },
        ]),
        ToShell::Agents(Vec::new()),
        ToShell::Agents(vec![AgentShown {
            id: 3,
            state: "waiting".into(),
            since: 123_456,
            source: "claude-code+herdr".into(),
            window: 0,
            name: "hocus panda".into(),
            note: None,
        }]),
        ToShell::Agents(vec![
            AgentShown {
                id: 3,
                state: "waiting".into(),
                since: 1,
                source: "claude-code".into(),
                window: 7,
                name: "site-web".into(),
                note: Some(Note::allow("Bash", "npm install")),
            },
            AgentShown {
                id: 4,
                state: "failed".into(),
                since: 2,
                source: "claude-code".into(),
                window: 0,
                name: "migration".into(),
                note: Note::says("API Error: Rate limit reached"),
            },
            AgentShown {
                id: 5,
                state: "waiting".into(),
                since: 2,
                source: "codex".into(),
                window: 0,
                name: "infra".into(),
                note: Some(Note::allow("apply_patch", "")),
            },
        ]),
        ToShell::Peek(Peek {
            id: 3,
            kind: PeekKind::Waiting,
            since: 123_456,
            name: "hocus panda · refacto".into(),
        }),
        ToShell::Peek(Peek {
            id: 4,
            kind: PeekKind::Done,
            since: 0,
            name: "api-paiements".into(),
        }),
        ToShell::NoPeek,
        ToShell::ScreensMoved(ScreensMoved {
            gone: true,
            built_in: false,
            inches: 34,
            desktop: 1,
            apps: vec!["firefox".into(), "foot".into(), String::new()],
        }),
        ToShell::ScreensMoved(ScreensMoved {
            gone: false,
            built_in: true,
            inches: 0,
            desktop: 0,
            apps: vec![],
        }),
        ToShell::AgentPandas(true),
        ToShell::AgentPandas(false),
        ToShell::Launcher {
            output: "eDP-1".into(),
            scale: 240,
        },
        ToShell::AuthAt(AuthAt {
            id: 3,
            output: "eDP-1".into(),
            scale: 150,
            height: 1067,
            app: "org.gnome.Software".into(),
            title: "Mises à jour".into(),
        }),
        ToShell::AuthAt(AuthAt {
            id: 4,
            output: "HEADLESS-1".into(),
            scale: 120,
            height: 1080,
            app: String::new(),
            title: String::new(),
        }),
        ToShell::CapsLock(true),
        ToShell::CapsLock(false),
        ToShell::Capture(CaptureBar {
            output: "eDP-1".into(),
            scale: 120,
            mode: CaptureMode::Area,
            delay: 0,
            pointer: false,
            video: false,
            sound: true,
            micro: false,
        }),
        ToShell::Capture(CaptureBar {
            output: "HDMI-A-1".into(),
            scale: 180,
            mode: CaptureMode::All,
            delay: 10,
            pointer: true,
            video: true,
            sound: false,
            micro: true,
        }),
        ToShell::CaptureEnd,
        ToShell::Captured(Captured {
            id: 1,
            output: "eDP-1".into(),
            mode: CaptureMode::Area,
            width: 600,
            height: 380,
            folder: "Images/Captures d'écran".into(),
            file: "/home/alex/Images/Captures d'écran/Capture du 2026-11-03 à 14-32-05.png"
                .into(),
        }),
        ToShell::Captured(Captured {
            id: 12,
            output: "DP-2".into(),
            mode: CaptureMode::Window,
            width: 1,
            height: 1,
            folder: String::new(),
            file: String::new(),
        }),
        ToShell::Recording(RecordingShown {
            id: 1,
            paused: false,
            elapsed: 0,
        }),
        ToShell::Recording(RecordingShown {
            id: 7,
            paused: true,
            elapsed: 754_321,
        }),
        ToShell::RecordingEnd,
        ToShell::Recorded(Recorded {
            id: 2,
            output: "eDP-1".into(),
            width: 900,
            height: 760,
            duration: 12_400,
            format: RecordFormat::Mp4,
            folder: "Vidéos/Enregistrements d'écran".into(),
            file: "/home/alex/Vidéos/Enregistrements d'écran/Enregistrement du 2026-11-03 à 14-32-05.mp4"
                .into(),
            poster: "/run/user/1000/hocuspanda-recording-2.png".into(),
        }),
        ToShell::Recorded(Recorded {
            id: 3,
            output: "DP-2".into(),
            width: 2,
            height: 2,
            duration: 1,
            format: RecordFormat::WebM,
            folder: String::new(),
            file: "/tmp/a.webm".into(),
            poster: String::new(),
        }),
        ToShell::WindowMenu(menu.clone()),
        ToShell::WindowMenu(WindowMenu {
            screens: Vec::new(),
            ..menu
        }),
        ToShell::WindowStates(WindowStates::default()),
        ToShell::WindowStates(WindowStates {
            above: vec![3, 9],
            all_desktops: vec![4],
        }),
        ToShell::ShareChoose(ShareChoose {
            request: 7,
            output: "eDP-1".into(),
            scale: 120,
            width: 2560,
            height: 1600,
            types: SHARE_MONITOR | SHARE_WINDOW,
            pointer: true,
            app: "org.mozilla.firefox".into(),
            detail: "Meet — Point produit".into(),
            screens: vec![
                ShareScreen {
                    name: "eDP-1".into(),
                    width: 2560,
                    height: 1600,
                    builtin: true,
                    model: String::new(),
                },
                ShareScreen {
                    name: "DP-2".into(),
                    width: 3440,
                    height: 1440,
                    builtin: false,
                    model: "DELL U3421WE".into(),
                },
            ],
            windows: vec![ShareWindow {
                id: 3,
                desktop: 2,
                here: false,
                minimized: true,
                app: String::new(),
                title: "kitty — cargo nextest".into(),
            }],
        }),
        ToShell::ShareChooseEnd(7),
        ToShell::Shares(Vec::new()),
        ToShell::Shares(vec![
            ShareShown {
                id: 1,
                app: "firefox".into(),
                thing: SharedThing::Window("hocus-panda — Visual Studio Code".into()),
            },
            ShareShown {
                id: 2,
                app: String::new(),
                thing: SharedThing::Screen("Écran du portable".into()),
            },
        ]),
        ToShell::LeaveCan(LeaveCan {
            suspend: Can::Yes,
            restart: Can::Challenge,
            shut_down: Can::No,
        }),
        ToShell::Leave(LeaveShown {
            output: "eDP-1".into(),
            action: LeaveAction::ShutDown,
            step: LeaveStep::Counting { left_ms: 9400 },
        }),
        ToShell::Leave(LeaveShown {
            output: "eDP-1".into(),
            action: LeaveAction::LogOut,
            step: LeaveStep::Held(vec![
                Holder {
                    kind: HolderKind::Inhibitor { pid: 4242 },
                    app: "Firefox".into(),
                    text: "Un téléchargement est en cours".into(),
                },
                Holder {
                    kind: HolderKind::Window { id: 7 },
                    app: "org.gnome.TextEditor".into(),
                    text: "Sans titre 1".into(),
                },
            ]),
        }),
        ToShell::Leave(LeaveShown {
            output: "DP-2".into(),
            action: LeaveAction::Restart,
            step: LeaveStep::Held(Vec::new()),
        }),
        ToShell::Leave(LeaveShown {
            output: "DP-2".into(),
            action: LeaveAction::Restart,
            step: LeaveStep::Authorizing,
        }),
        ToShell::Leave(LeaveShown {
            output: "DP-2".into(),
            action: LeaveAction::Restart,
            step: LeaveStep::Closing,
        }),
        ToShell::Leave(LeaveShown {
            output: "DP-2".into(),
            action: LeaveAction::Restart,
            step: LeaveStep::Acting,
        }),
        ToShell::LeaveEnd,
        ToShell::DeskScreens {
            bar: "eDP-1".into(),
            screens: vec![
                DeskScreen {
                    output: "eDP-1".into(),
                    width: 1707,
                    height: 1067,
                    built_in: true,
                    inches: 14,
                    number: 1,
                },
                DeskScreen {
                    output: "DP-1".into(),
                    width: 3440,
                    height: 1440,
                    built_in: false,
                    inches: 0,
                    number: 2,
                },
            ],
        },
        ToShell::Actions(ActionsShown {
            output: "DP-2".into(),
            scale: 120,
            window: Some(7),
            title: "kitty · herdr".into(),
            area: (8, 3360),
            on: vec!["above".into(), "screen-reader".into()],
            inert: vec![
                ("move-to-screen-left".into(), actions::Inert::OneScreen),
                ("magnifier-off".into(), actions::Inert::MagnifierOff),
            ],
            shortcuts: vec![
                ("close".into(), vec!["Super+Q".into(), "Alt+F4".into()]),
                ("focus-left".into(), Vec::new()),
                ("send-to-desktop-1".into(), vec!["Super+Shift+1".into()]),
            ],
        }),
        ToShell::Actions(ActionsShown {
            output: "HEADLESS-1".into(),
            scale: 240,
            ..ActionsShown::default()
        }),
        ToShell::ActionsEnd,
    ];
    all.extend(LevelKey::ALL.map(ToShell::Level));
    all
}

/// Messages to the compositor, one of every kind at least.
fn to_compositor() -> Vec<ToCompositor> {
    let mut all = vec![
        ToCompositor::Emoji {
            keep: false,
            text: "🐱".into(),
        },
        ToCompositor::Emoji {
            keep: true,
            text: "🧑🏿‍🤝‍🧑🏿".into(),
        },
        ToCompositor::Software,
        ToCompositor::Wake(LEVELS_NAMESPACE.to_owned()),
        ToCompositor::Wake(CONNECTIONS_NAMESPACE.to_owned()),
        ToCompositor::Closing(POPOVER_NAMESPACE.to_owned()),
        ToCompositor::NextLayout,
        ToCompositor::Lock(LockAsk::Submit),
        ToCompositor::Lock(LockAsk::Focus(LockFocus::Button)),
        ToCompositor::Lock(LockAsk::Now),
        ToCompositor::Desktop {
            output: "eDP-1".into(),
            to: DesktopTo::At(3),
        },
        ToCompositor::Desktop {
            output: "eDP-1".into(),
            to: DesktopTo::Next,
        },
        ToCompositor::Desktop {
            output: "eDP-1".into(),
            to: DesktopTo::Previous,
        },
        ToCompositor::Bring(42),
        ToCompositor::Close(42),
        ToCompositor::Overview,
        ToCompositor::A11yLine("  button \"14 h 32\" <click>".into()),
        ToCompositor::A11yEnd,
        ToCompositor::GoToAgent { id: 7, row: None },
        ToCompositor::PeekClose,
        ToCompositor::Ring(Row {
            output: "eDP-1".into(),
            x: -190,
            y: 14,
            width: 172,
            height: 77,
        }),
        ToCompositor::GoToAgent {
            id: 7,
            row: Some(Row {
                output: "eDP-1".into(),
                x: -190,
                y: 126,
                width: 184,
                height: 28,
            }),
        },
        ToCompositor::Plug("claude-code".into()),
        ToCompositor::Unplug("codex".into()),
        ToCompositor::Decline,
        ToCompositor::LogOut,
        ToCompositor::Follow("herdr".into()),
        ToCompositor::Briefing { open: true },
        ToCompositor::Briefing { open: false },
        ToCompositor::Meeting(Some(MeetingNear::Ten)),
        ToCompositor::Meeting(Some(MeetingNear::Two)),
        ToCompositor::Meeting(None),
        ToCompositor::Copy("hocus agent my-agent waiting".into()),
        ToCompositor::Clip {
            paste: true,
            clip: Clip {
                kind: ClipKind::Text,
                data: "git log\n\tétoile ✦".as_bytes().to_vec(),
            },
        },
        ToCompositor::Clip {
            paste: false,
            clip: Clip {
                kind: ClipKind::Png,
                data: vec![0x89, b'P', b'N', b'G', 0, 255],
            },
        },
        ToCompositor::ClipboardCount {
            items: 143,
            pinned: 3,
        },
        ToCompositor::Raise {
            pid: 4242,
            app: "org.mozilla.firefox".into(),
        },
        ToCompositor::Raise {
            pid: 0,
            app: String::new(),
        },
        ToCompositor::TrayClick { ask: 3, pid: 4242 },
        ToCompositor::TrayClick { ask: 4, pid: 0 },
        ToCompositor::Export {
            ask: 5,
            namespace: "hocus-capture-editor".into(),
        },
        ToCompositor::Launch(Launch {
            id: "org.mozilla.firefox".into(),
            terminal: false,
            dir: String::new(),
            argv: vec!["firefox".into(), "--new-window".into()],
        }),
        ToCompositor::Launch(Launch {
            id: "htop".into(),
            terminal: true,
            dir: "/home/alex/my projects".into(),
            argv: vec!["htop".into(), String::new(), "a b".into()],
        }),
        ToCompositor::Auth(AuthAsk::Open {
            id: 3,
            pids: vec![4242, 4100, 1],
        }),
        ToCompositor::Auth(AuthAsk::Open {
            id: 4,
            pids: Vec::new(),
        }),
        ToCompositor::Auth(AuthAsk::Closing { id: 3 }),
        ToCompositor::SendTo {
            id: 7,
            output: "HDMI-A-1".into(),
            desktop: 2,
        },
        ToCompositor::WindowMenu(7),
        ToCompositor::DeskTiles {
            bar: "eDP-1".into(),
            tiles: vec![DeskTile {
                output: "DP-1".into(),
                desktop: 2,
                rect: [11, 51, 39, 20],
            }],
        },
        ToCompositor::Action(RunAction {
            id: "send-to-desktop".into(),
            arg: Some(2),
            window: Some(7),
        }),
        ToCompositor::Action(RunAction {
            id: "lock".into(),
            arg: None,
            window: None,
        }),
    ];
    all.extend(CaptureMode::ALL.map(|mode| ToCompositor::Capture(CaptureAsk::Mode(mode))));
    all.extend(
        [
            CaptureAsk::Delay,
            CaptureAsk::Pointer,
            CaptureAsk::Shoot,
            CaptureAsk::Video(true),
            CaptureAsk::Video(false),
            CaptureAsk::Sound,
            CaptureAsk::Micro,
            CaptureAsk::Forget(3),
            CaptureAsk::Annotated(4),
            CaptureAsk::Drag(u64::MAX),
        ]
        .map(ToCompositor::Capture),
    );
    all.extend(
        [
            RecordAsk::Pause,
            RecordAsk::Resume,
            RecordAsk::Stop,
            RecordAsk::Forget(4),
            RecordAsk::Drag(u64::MAX),
        ]
        .map(ToCompositor::Record),
    );
    all.extend(
        [
            LeaveAsk::Open,
            LeaveAsk::Ask {
                action: LeaveAction::LogOut,
                output: "HEADLESS-1".into(),
            },
            LeaveAsk::Suspend,
            LeaveAsk::Now,
            LeaveAsk::Cancel,
            LeaveAsk::Force(12),
        ]
        .map(ToCompositor::Leave),
    );
    all.extend(
        [
            ShareAsk::Thumbs {
                request: 3,
                rects: vec![
                    (
                        ShareSource::Screen("HEADLESS-1".into()),
                        Rect {
                            x: 31,
                            y: 140,
                            width: 236,
                            height: 129,
                        },
                    ),
                    (
                        ShareSource::Window(12),
                        Rect {
                            x: 290,
                            y: 140,
                            width: 236,
                            height: 129,
                        },
                    ),
                ],
            },
            ShareAsk::Thumbs {
                request: 3,
                rects: Vec::new(),
            },
            ShareAsk::Chosen {
                request: 3,
                source: ShareSource::Window(12),
                pointer: false,
            },
            ShareAsk::Cancel { request: 3 },
            ShareAsk::Stop { id: 9 },
        ]
        .map(ToCompositor::Share),
    );
    all.extend(
        WindowAction::ALL
            .into_iter()
            .map(|action| ToCompositor::WindowAction { id: 7, action }),
    );
    all
}

/// The kinds serde reads for `T`, from what it says of one it does not
/// know (`probe`): the list its derive made, which no one keeps by hand.
fn kinds<T: DeserializeOwned + std::fmt::Debug>(probe: &str) -> BTreeSet<String> {
    let said = serde_json::from_str::<T>(probe)
        .expect_err("a kind of no message")
        .to_string();
    // "unknown variant `?`, expected one of `a`, `b`" (or "`a` or `b`").
    let (_, listed) = said
        .split_once(", expected ")
        .unwrap_or_else(|| panic!("{said}"));
    listed
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// A line's kind, and the kind of what it carries when that is itself one
/// of several (an ask: `"shoot"`, or `{"mode":"area"}`).
fn kind_and_inner(line: &str) -> (String, Option<String>) {
    assert!(!line.contains('\n'), "on one line: {line}");
    let value: serde_json::Value = serde_json::from_str(line).expect(line);
    let object = value.as_object().expect("one object a line");
    let kind = object["kind"].as_str().expect("its kind").to_owned();
    let inner = match object.get("data") {
        Some(serde_json::Value::String(inner)) => Some(inner.clone()),
        Some(serde_json::Value::Object(data)) if data.len() == 1 => data.keys().next().cloned(),
        _ => None,
    };
    (kind, inner)
}

/// The inner kinds seen in the lines of kind `kind`.
fn inner_of(seen: &[(String, Option<String>)], kind: &str) -> BTreeSet<String> {
    seen.iter()
        .filter(|(k, _)| k == kind)
        .filter_map(|(_, inner)| inner.clone())
        .collect()
}

#[test]
fn every_kind_reads_back_as_written() {
    let mut seen = Vec::new();
    for message in to_shell() {
        let line = message.line();
        seen.push(kind_and_inner(&line));
        assert_eq!(ToShell::read(&line), Ok(message), "{line}");
    }
    let kinds_seen: BTreeSet<String> = seen.iter().map(|(kind, _)| kind.clone()).collect();
    assert_eq!(
        kinds_seen,
        kinds::<ToShell>(r#"{"kind":"?"}"#),
        "a kind to the shell has no message in `to_shell`"
    );
    assert_eq!(inner_of(&seen, "day"), kinds::<DayAsk>(r#""?""#));

    let mut seen = Vec::new();
    for message in to_compositor() {
        let line = message.line();
        seen.push(kind_and_inner(&line));
        assert_eq!(ToCompositor::read(&line), Ok(message), "{line}");
    }
    let kinds_seen: BTreeSet<String> = seen.iter().map(|(kind, _)| kind.clone()).collect();
    assert_eq!(
        kinds_seen,
        kinds::<ToCompositor>(r#"{"kind":"?"}"#),
        "a kind to the compositor has no message in `to_compositor`"
    );
    assert_eq!(inner_of(&seen, "capture"), kinds::<CaptureAsk>(r#""?""#));
    assert_eq!(inner_of(&seen, "record"), kinds::<RecordAsk>(r#""?""#));
    assert_eq!(inner_of(&seen, "leave"), kinds::<LeaveAsk>(r#""?""#));
    assert_eq!(inner_of(&seen, "share"), kinds::<ShareAsk>(r#""?""#));
    assert_eq!(inner_of(&seen, "lock"), kinds::<LockAsk>(r#""?""#));
    assert_eq!(inner_of(&seen, "auth"), kinds::<AuthAsk>(r#""?""#));
}

#[test]
fn a_message_is_one_json_object_on_a_line() {
    let bar = ToShell::Bar {
        output: "eDP-1".into(),
        form: Form::Full,
        since: 0,
        scale: 120,
    };
    assert_eq!(
        bar.line(),
        r#"{"kind":"bar","data":{"output":"eDP-1","form":"full","since":0,"scale":120}}"#
    );
    assert_eq!(ToShell::Outside.line(), r#"{"kind":"outside"}"#);
    assert_eq!(
        ToCompositor::Capture(CaptureAsk::Mode(CaptureMode::Window)).line(),
        r#"{"kind":"capture","data":{"mode":"window"}}"#
    );
    // A clip's bytes in base64, its line breaks with them.
    let clip = ToCompositor::Clip {
        paste: true,
        clip: Clip {
            kind: ClipKind::Text,
            data: b"a\nb".to_vec(),
        },
    };
    assert_eq!(
        clip.line(),
        r#"{"kind":"clip","data":{"paste":true,"clip":{"kind":"text","data":"YQpi"}}}"#
    );
}

#[test]
fn the_hello_says_the_version_and_another_one_is_told_apart() {
    assert_eq!(hello(), r#"{"kind":"hello","data":{"version":1}}"#);
    assert_eq!(said_version(&hello()), Some(VERSION));
    // A side upgraded, or not yet: its version is read all the same.
    assert_eq!(said_version(&hello_of(VERSION + 1)), Some(VERSION + 1));
    assert_eq!(said_version(&hello_of(0)), Some(0));
    // The words of before version 1, and a message in place of the hello:
    // no hello.
    for first in [
        "software",
        "look glass motion lively",
        "",
        &ToCompositor::Software.line(),
        &ToShell::Outside.line(),
    ] {
        assert_eq!(said_version(first), None, "{first:?}");
    }
    // A hello is no message: one that comes later is skipped.
    assert!(ToShell::read(&hello()).is_err());
    assert!(ToCompositor::read(&hello()).is_err());
}

#[test]
fn a_kind_one_side_does_not_know_is_said_and_skipped() {
    let Err(BadLine(why)) = ToShell::read(r#"{"kind":"teleport","data":{"to":"Mars"}}"#) else {
        panic!("a kind of no message read");
    };
    assert_eq!(why, r#"a kind this side does not know: "teleport""#);
    let Err(BadLine(why)) = ToCompositor::read(r#"{"kind":"teleport"}"#) else {
        panic!("a kind of no message read");
    };
    assert!(why.contains("teleport"), "{why}");
    // The line after it reads.
    assert_eq!(
        ToShell::read(&ToShell::Outside.line()),
        Ok(ToShell::Outside)
    );
    // A field missing is named; what a line carries is never said.
    let Err(BadLine(why)) = ToShell::read(r#"{"kind":"launcher","data":{"output":"eDP-1"}}"#)
    else {
        panic!("a launcher without its scale read");
    };
    assert_eq!(why, r#""launcher": missing field `scale`"#);
    for bad in [
        r#"{"kind":"focus","data":{"hunter2":1}}"#,
        r#"{"kind":"layout","data":"hunter2"}"#,
        r#"{"kind":"copied","data":{"app":"kitty","clip":{"kind":"text","data":"hunter2*"}}}"#,
        r#"{"kind":"outside","data":"hunter2""#,
        "hunter2",
        "",
    ] {
        let Err(BadLine(why)) = ToShell::read(bad) else {
            panic!("{bad} read");
        };
        assert!(!why.contains("hunter2"), "{why}");
    }
}

#[test]
fn what_reads_and_cannot_be_is_refused() {
    // Nothing to run; an emoji that is none.
    let nothing = ToCompositor::Launch(Launch {
        id: "x".into(),
        terminal: false,
        dir: String::new(),
        argv: Vec::new(),
    });
    assert!(ToCompositor::read(&nothing.line()).is_err());
    for text in ["🐱 🐶", "", "a\u{7}"] {
        let emoji = ToCompositor::Emoji {
            keep: false,
            text: text.into(),
        };
        assert!(ToCompositor::read(&emoji.line()).is_err(), "{text:?}");
    }
    let long = "😀".repeat(20);
    assert!(!emoji_text(&long));
    assert!(!emoji_text("a\u{7}"));
    // A clip larger than the history takes, or of no kind.
    let large = ToCompositor::Clip {
        paste: true,
        clip: Clip {
            kind: ClipKind::Png,
            data: vec![0; CLIP_MAX + 1],
        },
    };
    assert!(ToCompositor::read(&large.line()).is_err());
    assert!(
        ToShell::read(r#"{"kind":"copied","data":{"app":"","clip":{"kind":"html","data":""}}}"#)
            .is_err()
    );
    // A window outside its screen.
    let outside = ToShell::Desktops(Desktops {
        output: "eDP-1".into(),
        count: 1,
        current: 0,
        windows: vec![DeskWindow {
            desktop: 0,
            rect: [0, 0, SHARE + 1, 1],
            look: Look::Plain,
            id: 1,
            app: String::new(),
        }],
    });
    assert!(ToShell::read(&outside.line()).is_err());
    // A group twice in the bar's order.
    let twice = ToShell::BarGroups(BarGroups {
        order: [BarGroup::Today, BarGroup::Today, BarGroup::Agents],
        ..BarGroups::default()
    });
    assert!(ToShell::read(&twice.line()).is_err());
}

#[test]
fn a_clip_is_never_logged() {
    let clip = Clip {
        kind: ClipKind::Text,
        data: "hunter2".as_bytes().to_vec(),
    };
    assert_eq!(format!("{clip:?}"), "Clip(text, 7 bytes)");
}

#[test]
fn the_texts_a_line_carries_are_tidied() {
    // A title: on one line, no longer than its most.
    let line = ToShell::Titles(Titles {
        output: "eDP-1".into(),
        titles: vec![(1, " a\tb\nc ".into()), (2, "é".repeat(TITLE_MAX + 5))],
    })
    .line();
    let Ok(ToShell::Titles(read)) = ToShell::read(&line) else {
        panic!("{line:?}");
    };
    assert_eq!(read.titles[0], (1, "a b c".to_owned()));
    assert_eq!(read.titles[1].1.chars().count(), TITLE_MAX);
    // A name.
    assert_eq!(screen_name(" LG\tULTRA\nGEAR "), "LG ULTRA GEAR");
    assert_eq!(
        ToShell::read(&ToShell::Screens(vec![" LG\tULTRA ".into()]).line()),
        Ok(ToShell::Screens(vec!["LG ULTRA".into()]))
    );
    assert_eq!(
        ToShell::read(&ToShell::Day(DayAsk::Add("deux\nlignes".into())).line()),
        Ok(ToShell::Day(DayAsk::Add("deux lignes".into())))
    );
    // What is shared, and why the session is held.
    let shares = ToShell::Shares(vec![ShareShown {
        id: 1,
        app: "a b".into(),
        thing: SharedThing::Window("one\ttwo\nthree".into()),
    }]);
    let Ok(ToShell::Shares(read)) = ToShell::read(&shares.line()) else {
        panic!("the shares do not read");
    };
    assert_eq!(read[0].thing, SharedThing::Window("one two three".into()));
    // A line of the screen readers' tree keeps its indent.
    assert_eq!(
        ToCompositor::read(&ToCompositor::A11yLine("  button\tOK \n".into()).line()),
        Ok(ToCompositor::A11yLine("  button OK".into()))
    );
}

#[test]
fn a_note_is_one_short_line_and_never_logged() {
    // Its line breaks, tabs and runs of spaces made one space.
    assert_eq!(
        note_text("  npm\tinstall \n --save\r\n"),
        "npm install --save"
    );
    // Cut to its most, the last character an ellipsis.
    let long = note_text(&"é".repeat(NOTE_MAX + 30));
    assert_eq!(long.chars().count(), NOTE_MAX);
    assert!(long.ends_with('…'));
    assert_eq!(note_text(&"a".repeat(NOTE_MAX)), "a".repeat(NOTE_MAX));
    // Nothing said: no note.
    assert_eq!(Note::says(" \n "), None);
    // A request's tool is one word.
    assert_eq!(
        Note::allow("my tool", "x"),
        Note::Allow {
            tool: "my_tool".into(),
            what: "x".into()
        }
    );
    // Read back as written, on the line and in an agent's own words.
    let note = Note::says("a\tb");
    assert_eq!(note, Some(Note::Says("a b".into())));
    assert_eq!("says a b".parse::<Note>().ok(), note);
    let agents = ToShell::Agents(vec![AgentShown {
        id: 1,
        state: "failed".into(),
        since: 0,
        source: "command".into(),
        window: 0,
        name: "x".into(),
        note,
    }]);
    let Ok(ToShell::Agents(read)) = ToShell::read(&agents.line()) else {
        panic!("the agents do not read");
    };
    assert_eq!(read[0].note, Some(Note::Says("a b".into())));
    // Its length only, in what is logged.
    let secret = Note::allow("Bash", "export TOKEN=hunter2");
    assert_eq!(format!("{secret:?}"), "allow (24 characters)");
    assert!(!format!("{:?}", read[0]).contains("a b"));
}

#[test]
fn the_bar_order_takes_known_names_once_then_the_missing_ones() {
    use BarGroup::{Agents, Tasks, Today};
    assert_eq!(BarGroup::order_of::<&str>(&[]), [Today, Tasks, Agents]);
    assert_eq!(
        BarGroup::order_of(&["tasks", "today", "agents"]),
        [Tasks, Today, Agents]
    );
    // Unknown and repeated names skipped, the missing appended.
    assert_eq!(
        BarGroup::order_of(&["agents", "windows", "agents"]),
        [Agents, Today, Tasks]
    );
    assert!("meeting".parse::<BarGroup>().is_err());
}

#[test]
fn a_bubbles_namespace_holds_its_number() {
    assert_eq!(bubble_namespace(7), "hocus-bubble-7");
    assert!(is_bubble(&bubble_namespace(12)));
    for not in [
        "hocus-bubble-",
        "hocus-bubble-x",
        BUBBLES_WAITING_NAMESPACE,
        NAMESPACE,
    ] {
        assert!(!is_bubble(not), "{not}");
    }
}

#[test]
fn a_reminders_namespace_holds_its_number_and_slides_as_a_bubble() {
    assert_eq!(reminder_namespace(3), "hocus-reminder-3");
    assert!(is_reminder(&reminder_namespace(12)));
    assert!(slides_in(&reminder_namespace(1)) && slides_in(&bubble_namespace(1)));
    for not in ["hocus-reminder-", "hocus-reminder-x", MEETING_NAMESPACE] {
        assert!(!is_reminder(not) && !slides_in(not), "{not}");
    }
}

#[test]
fn colours_read_in_both_lengths() {
    assert_eq!(parse_color("#EEF1F6"), Ok([0xEE, 0xF1, 0xF6]));
    assert_eq!(parse_color("#3dffa2"), Ok([0x3D, 0xFF, 0xA2]));
    assert_eq!(parse_color("#fff"), Ok([255, 255, 255]));
    for bad in ["EEF1F6", "#EEF1F", "#GGGGGG", "#", "#+1+2+3"] {
        assert!(parse_color(bad).is_err(), "{bad}");
    }
}

#[test]
fn the_default_form_follows_the_width_left_beside_the_bar() {
    // bar.md §3's table, in logical pixels.
    assert_eq!(Form::default_for(1366), Form::Mini);
    assert_eq!(Form::default_for(1920), Form::Full);
    assert_eq!(Form::default_for(1280), Form::Mini);
    assert_eq!(Form::default_for(2560), Form::Full);
    assert_eq!(Form::default_for(3440), Form::Full);
    assert_eq!(Form::default_for(1796), Form::Full);
    assert_eq!(Form::default_for(1795), Form::Mini);
}

#[test]
fn the_switch_overshoots_a_little_and_lands() {
    assert_eq!(width_at(196.0, Form::Mini, Duration::ZERO), 196.0);
    assert_eq!(width_at(196.0, Form::Mini, SWITCH), 56.0);
    let narrowest = (1..240)
        .map(|ms| width_at(196.0, Form::Mini, Duration::from_millis(ms)))
        .fold(f64::MAX, f64::min);
    assert!((44.0..56.0).contains(&narrowest), "{narrowest}");
    assert!(ease(0.3) > 0.7, "quick to start");
}
