// Generated from the Settings' rows: see `labels.rs`. Do not edit.

/// Every key a row writes, in the dotted keys' order.
pub const ALL: &[Label] = &[
    Label {
        key: "accessibility.screen-reader",
        name: hocus_words::settings::accessibility::SCREEN_READER_2,
        said: Said::Switch,
    },
    Label {
        key: "agents.halo",
        name: hocus_words::settings::agents::THE_HALO_2,
        said: Said::Choices(&[("all", hocus_words::settings::agents::FOR_EVERYTHING), ("waiting", hocus_words::settings::agents::WHEN_AN_AGENT_WAITS_FOR_YOU), ("off", hocus_words::settings::agents::NEVER)]),
    },
    Label {
        key: "agents.pandas",
        name: hocus_words::settings::agents::PANDAS_IN_THE_AGENTS_ROWS,
        said: Said::Switch,
    },
    Label {
        key: "agents.peek",
        name: hocus_words::settings::agents::THE_PANDA_PEEKING_OUT_OF_THE,
        said: Said::Choices(&[("waiting-and-done", hocus_words::settings::agents::WHEN_AN_AGENT_WAITS_FOR_YOU_2), ("waiting", hocus_words::settings::agents::WHEN_AN_AGENT_WAITS_FOR_YOU), ("off", hocus_words::settings::agents::NEVER)]),
    },
    Label {
        key: "agents.remind",
        name: hocus_words::settings::agents::REMIND_ME_OF_A_WAITING_AGENT,
        said: Said::Choices(&[("2m", hocus_words::settings::agents::AFTER_2_MIN), ("5m", hocus_words::settings::agents::AFTER_5_MIN), ("10m", hocus_words::settings::agents::AFTER_10_MIN), ("never", hocus_words::settings::agents::NEVER)]),
    },
    Label {
        key: "appearance.accent",
        name: hocus_words::settings::appearance::ACCENT_COLOUR,
        said: Said::Other,
    },
    Label {
        key: "appearance.cursor-size",
        name: hocus_words::settings::appearance::CURSOR_SIZE,
        said: Said::Choices(&[("24", hocus_words::settings::appearance::N_24), ("32", hocus_words::settings::appearance::N_32), ("48", hocus_words::settings::appearance::N_48), ("64", hocus_words::settings::appearance::N_64)]),
    },
    Label {
        key: "appearance.cursor-theme",
        name: hocus_words::settings::appearance::CURSOR_THEME,
        said: Said::Choices(&[("", hocus_words::settings::appearance::THE_SYSTEM_S)]),
    },
    Label {
        key: "appearance.high-contrast",
        name: hocus_words::settings::accessibility::HIGH_CONTRAST,
        said: Said::Switch,
    },
    Label {
        key: "appearance.reduce-transparency",
        name: hocus_words::settings::accessibility::REDUCED_TRANSPARENCY,
        said: Said::Switch,
    },
    Label {
        key: "appearance.reduced-motion",
        name: hocus_words::settings::accessibility::REDUCED_MOTION,
        said: Said::Switch,
    },
    Label {
        key: "appearance.theme",
        name: hocus_words::settings::appearance::THEME,
        said: Said::Choices(&[("dark", hocus_words::settings::appearance::DARK), ("light", hocus_words::settings::appearance::LIGHT), ("frost", hocus_words::settings::appearance::FROST), ("matrix", hocus_words::settings::appearance::MATRIX)]),
    },
    Label {
        key: "capture.folder",
        name: hocus_words::settings::app_captures_page::FOLDER,
        said: Said::Other,
    },
    Label {
        key: "capture.format",
        name: hocus_words::settings::captures::FORMAT,
        said: Said::Choices(&[("png", hocus_words::settings::captures::PNG), ("jpeg", hocus_words::settings::captures::JPEG)]),
    },
    Label {
        key: "capture.sound",
        name: hocus_words::settings::captures::SHUTTER_SOUND,
        said: Said::Switch,
    },
    Label {
        key: "clipboard.history",
        name: hocus_words::settings::captures::KEEP_WHAT_I_COPY,
        said: Said::Switch,
    },
    Label {
        key: "clipboard.ignore",
        name: hocus_words::settings::captures::APPLICATIONS_LEFT_OUT,
        said: Said::Other,
    },
    Label {
        key: "clipboard.keep",
        name: hocus_words::settings::captures::KEEP_FOR,
        said: Said::Choices(&[("session", hocus_words::settings::captures::UNTIL_THE_SESSION_ENDS), ("1d", hocus_words::settings::captures::N_1_DAY), ("7d", hocus_words::settings::app_perf_page::N_7_DAYS), ("30d", hocus_words::settings::captures::N_30_DAYS)]),
    },
    Label {
        key: "clipboard.skip-secrets",
        name: hocus_words::settings::captures::LEAVE_PASSWORDS_OUT,
        said: Said::Switch,
    },
    Label {
        key: "desktops.count",
        name: hocus_words::settings::desktops::NUMBER_OF_DESKTOPS,
        said: Said::Number(""),
    },
    Label {
        key: "desktops.names",
        name: hocus_words::settings::desktops::THEIR_NAMES,
        said: Said::Other,
    },
    Label {
        key: "effects.aurora",
        name: hocus_words::settings::effects::AURORA,
        said: Said::Switch,
    },
    Label {
        key: "effects.confetti",
        name: hocus_words::settings::effects::CONFETTI,
        said: Said::Switch,
    },
    Label {
        key: "effects.creatures",
        name: hocus_words::settings::effects::CREATURES,
        said: Said::Switch,
    },
    Label {
        key: "effects.daylight",
        name: hocus_words::settings::effects::DAYLIGHT,
        said: Said::Switch,
    },
    Label {
        key: "effects.depth",
        name: hocus_words::settings::effects::DEPTH,
        said: Said::Switch,
    },
    Label {
        key: "effects.fireflies",
        name: hocus_words::settings::effects::FIREFLIES,
        said: Said::Switch,
    },
    Label {
        key: "effects.fireworks",
        name: hocus_words::settings::effects::FIREWORKS,
        said: Said::Switch,
    },
    Label {
        key: "effects.flames",
        name: hocus_words::settings::effects::FLAMES,
        said: Said::Switch,
    },
    Label {
        key: "effects.garland",
        name: hocus_words::settings::effects::GARLAND,
        said: Said::Switch,
    },
    Label {
        key: "effects.garland-top",
        name: hocus_words::settings::effects::GARLAND_TOP,
        said: Said::Switch,
    },
    Label {
        key: "effects.jelly",
        name: hocus_words::settings::effects::JELLY,
        said: Said::Switch,
    },
    Label {
        key: "effects.koi",
        name: hocus_words::settings::effects::KOI,
        said: Said::Switch,
    },
    Label {
        key: "effects.rain-on-glass",
        name: hocus_words::settings::effects::RAIN_ON_GLASS,
        said: Said::Switch,
    },
    Label {
        key: "effects.shatter",
        name: hocus_words::settings::effects::SHATTER,
        said: Said::Switch,
    },
    Label {
        key: "effects.snow",
        name: hocus_words::settings::effects::SNOW,
        said: Said::Switch,
    },
    Label {
        key: "idle.delay",
        name: hocus_words::settings::power::DIM_THE_SCREEN_AFTER,
        said: Said::Choices(&[("60", hocus_words::settings::power::N_1_MIN), ("120", hocus_words::settings::power::N_2_MIN), ("180", hocus_words::settings::power::N_3_MIN), ("300", hocus_words::settings::power::N_5_MIN), ("600", hocus_words::settings::power::N_10_MIN), ("900", hocus_words::settings::power::N_15_MIN), ("1800", hocus_words::settings::power::N_30_MIN), ("off", hocus_words::settings::agents::NEVER)]),
    },
    Label {
        key: "ink.enabled",
        name: hocus_words::settings::desktops::DRAW_ON_THE_SCREEN_2,
        said: Said::Switch,
    },
    Label {
        key: "keyboard.layouts",
        name: hocus_words::settings::keyboard::LAYOUTS_2,
        said: Said::Other,
    },
    Label {
        key: "keyboard.numlock",
        name: hocus_words::settings::keyboard::NUM_LOCK_ON_AT_LOGIN,
        said: Said::Switch,
    },
    Label {
        key: "keyboard.repeat_delay",
        name: hocus_words::settings::keyboard::REPEAT_DELAY,
        said: Said::Number(" ms"),
    },
    Label {
        key: "keyboard.repeat_rate",
        name: hocus_words::settings::keyboard::REPEAT_RATE,
        said: Said::Number(" /s"),
    },
    Label {
        key: "locale.date",
        name: hocus_words::settings::appearance::DATE,
        said: Said::Choices(&[("long", hocus_words::settings::appearance::IN_WORDS), ("short", hocus_words::settings::appearance::SHORT), ("numeric", hocus_words::settings::appearance::IN_NUMBERS), ("iso", hocus_words::settings::appearance::YEAR_FIRST)]),
    },
    Label {
        key: "locale.language",
        name: hocus_words::settings::appearance::LANGUAGE,
        said: Said::Choices(&[("system", hocus_words::settings::appearance::THE_SYSTEM_S_2), ("fr", hocus_words::settings::appearance::FRAN_AIS), ("en", hocus_words::settings::appearance::ENGLISH)]),
    },
    Label {
        key: "locale.time",
        name: hocus_words::settings::appearance::TIME,
        said: Said::Choices(&[("24h", hocus_words::settings::appearance::N_14_05), ("12h", hocus_words::settings::appearance::N_2_05_PM)]),
    },
    Label {
        key: "magnifier.enabled",
        name: hocus_words::compositor::magnifier_draw::MAGNIFIER,
        said: Said::Switch,
    },
    Label {
        key: "magnifier.follow-keyboard",
        name: hocus_words::settings::accessibility::FOLLOW_THE_KEYBOARD,
        said: Said::Switch,
    },
    Label {
        key: "magnifier.follow-pointer",
        name: hocus_words::settings::accessibility::THE_VIEW_FOLLOWS_THE_POINTER,
        said: Said::Choices(&[("edge", hocus_words::settings::accessibility::AT_THE_EDGE), ("centre", hocus_words::settings::accessibility::CENTRED), ("proportional", hocus_words::settings::accessibility::PROPORTIONALLY)]),
    },
    Label {
        key: "magnifier.smooth",
        name: hocus_words::settings::accessibility::SMOOTHING,
        said: Said::Switch,
    },
    Label {
        key: "magnifier.zoom",
        name: hocus_words::settings::accessibility::ZOOM,
        said: Said::Choices(&[("2.0", Text::same("×2")), ("3.0", Text::same("×3")), ("4.0", Text::same("×4")), ("6.0", Text::same("×6")), ("8.0", Text::same("×8")), ("12.0", Text::same("×12")), ("16.0", Text::same("×16"))]),
    },
    Label {
        key: "night-light.enabled",
        name: hocus_words::settings::screens::NIGHT_LIGHT_2,
        said: Said::Switch,
    },
    Label {
        key: "night-light.from",
        name: hocus_words::settings::screens::FROM,
        said: Said::Choices(&[("18:00", Text::same("18:00")), ("18:30", Text::same("18:30")), ("19:00", Text::same("19:00")), ("19:30", Text::same("19:30")), ("20:00", Text::same("20:00")), ("20:30", Text::same("20:30")), ("21:00", Text::same("21:00")), ("21:30", Text::same("21:30")), ("22:00", Text::same("22:00")), ("22:30", Text::same("22:30")), ("23:00", Text::same("23:00")), ("23:30", Text::same("23:30"))]),
    },
    Label {
        key: "night-light.temperature",
        name: hocus_words::settings::screens::TEMPERATURE,
        said: Said::Number(" K"),
    },
    Label {
        key: "night-light.to",
        name: hocus_words::settings::screens::UNTIL,
        said: Said::Choices(&[("05:00", Text::same("05:00")), ("05:30", Text::same("05:30")), ("06:00", Text::same("06:00")), ("06:30", Text::same("06:30")), ("07:00", Text::same("07:00")), ("07:30", Text::same("07:30")), ("08:00", Text::same("08:00")), ("08:30", Text::same("08:30")), ("09:00", Text::same("09:00")), ("09:30", Text::same("09:30"))]),
    },
    Label {
        key: "overview.hot-corner",
        name: hocus_words::settings::desktops::TOP_LEFT_CORNER_OVERVIEW,
        said: Said::Switch,
    },
    Label {
        key: "panel.agents",
        name: hocus_words::settings::bar::THE_AGENTS,
        said: Said::Switch,
    },
    Label {
        key: "panel.clock.color",
        name: hocus_words::settings::bar::CLOCK_COLOUR,
        said: Said::Choices(&[("#eef1f6", hocus_words::settings::bar::SOFT_WHITE), ("#7ee0a0", hocus_words::settings::bar::GREEN), ("#8fb3ff", hocus_words::link::theme::BLUE), ("#f5c46e", hocus_words::settings::bar::AMBER)]),
    },
    Label {
        key: "panel.clock.font",
        name: hocus_words::settings::bar::CLOCK_FONT,
        said: Said::Choices(&[("Martian Mono", hocus_words::settings::bar::MARTIAN_MONO_BUNDLED)]),
    },
    Label {
        key: "panel.clock.seconds",
        name: hocus_words::settings::bar::SECONDS,
        said: Said::Switch,
    },
    Label {
        key: "panel.clock.transition",
        name: hocus_words::settings::bar::WHEN_THE_MINUTE_CHANGES,
        said: Said::Choices(&[("odometer", hocus_words::settings::bar::ODOMETER), ("roll", hocus_words::settings::bar::ROLL), ("flip", hocus_words::settings::bar::FLIP), ("neon", hocus_words::settings::bar::NEON), ("none", hocus_words::settings::bar::NO_MOTION)]),
    },
    Label {
        key: "panel.meeting",
        name: hocus_words::settings::bar::THE_NEXT_MEETING,
        said: Said::Switch,
    },
    Label {
        key: "panel.screens",
        name: hocus_words::settings::bar::ON_WHICH_SCREENS,
        said: Said::Choices(&[("primary", hocus_words::settings::bar::THE_PRIMARY_SCREEN), ("all", hocus_words::settings::bar::EVERY_SCREEN)]),
    },
    Label {
        key: "panel.side",
        name: hocus_words::settings::bar::SIDE_OF_THE_SCREEN,
        said: Said::Choices(&[("right", hocus_words::settings::bar::RIGHT), ("left", hocus_words::settings::bar::LEFT)]),
    },
    Label {
        key: "panel.tasks",
        name: hocus_words::settings::bar::TO_DO,
        said: Said::Switch,
    },
    Label {
        key: "panel.today",
        name: hocus_words::settings::bar::TODAY,
        said: Said::Switch,
    },
    Label {
        key: "panel.windows",
        name: hocus_words::settings::bar::THE_WINDOWS,
        said: Said::Switch,
    },
    Label {
        key: "perf.record",
        name: hocus_words::settings::perf::RECORD_THE_DAY,
        said: Said::Switch,
    },
    Label {
        key: "pointer.acceleration",
        name: hocus_words::settings::mouse::ACCELERATION,
        said: Said::Choices(&[("adaptive", hocus_words::settings::mouse::ADAPTIVE), ("flat", hocus_words::settings::app_applications_view::NONE)]),
    },
    Label {
        key: "pointer.left-handed",
        name: hocus_words::settings::mouse::LEFT_HANDED,
        said: Said::Switch,
    },
    Label {
        key: "pointer.natural-scroll",
        name: hocus_words::settings::mouse::NATURAL_SCROLLING,
        said: Said::Switch,
    },
    Label {
        key: "pointer.speed",
        name: hocus_words::settings::mouse::POINTER_SPEED,
        said: Said::Other,
    },
    Label {
        key: "power.ac.screen-off-after",
        name: hocus_words::settings::power::TURN_THE_SCREEN_OFF,
        said: Said::Choices(&[("0s", hocus_words::settings::model::AT_ONCE), ("10s", hocus_words::settings::power::N_10_S_AFTER), ("30s", hocus_words::settings::power::N_30_S_AFTER), ("1m", hocus_words::settings::power::N_1_MIN_AFTER), ("5m", hocus_words::settings::power::N_5_MIN_AFTER)]),
    },
    Label {
        key: "power.ac.suspend-after",
        name: hocus_words::settings::power::SUSPEND_AFTER,
        said: Said::Choices(&[("5m", hocus_words::settings::power::N_5_MIN), ("15m", hocus_words::settings::power::N_15_MIN), ("30m", hocus_words::settings::power::N_30_MIN), ("1h", hocus_words::settings::power::N_1_H), ("2h", hocus_words::settings::power::N_2_H), ("never", hocus_words::settings::agents::NEVER)]),
    },
    Label {
        key: "power.auto-saver",
        name: hocus_words::settings::power::AUTOMATIC_POWER_SAVER,
        said: Said::Switch,
    },
    Label {
        key: "power.auto-saver-below",
        name: hocus_words::settings::power::UNDER_THIS_BATTERY_LEVEL,
        said: Said::Number(" %"),
    },
    Label {
        key: "power.battery.idle",
        name: hocus_words::settings::power::DIM_THE_SCREEN_AFTER,
        said: Said::Choices(&[("1m", hocus_words::settings::power::N_1_MIN), ("2m", hocus_words::settings::power::N_2_MIN), ("3m", hocus_words::settings::power::N_3_MIN), ("5m", hocus_words::settings::power::N_5_MIN), ("10m", hocus_words::settings::power::N_10_MIN), ("15m", hocus_words::settings::power::N_15_MIN), ("30m", hocus_words::settings::power::N_30_MIN), ("never", hocus_words::settings::agents::NEVER)]),
    },
    Label {
        key: "power.battery.screen-off-after",
        name: hocus_words::settings::power::TURN_THE_SCREEN_OFF,
        said: Said::Choices(&[("0s", hocus_words::settings::model::AT_ONCE), ("10s", hocus_words::settings::power::N_10_S_AFTER), ("30s", hocus_words::settings::power::N_30_S_AFTER), ("1m", hocus_words::settings::power::N_1_MIN_AFTER), ("5m", hocus_words::settings::power::N_5_MIN_AFTER)]),
    },
    Label {
        key: "power.battery.suspend-after",
        name: hocus_words::settings::power::SUSPEND_AFTER,
        said: Said::Choices(&[("5m", hocus_words::settings::power::N_5_MIN), ("15m", hocus_words::settings::power::N_15_MIN), ("30m", hocus_words::settings::power::N_30_MIN), ("1h", hocus_words::settings::power::N_1_H), ("2h", hocus_words::settings::power::N_2_H), ("never", hocus_words::settings::agents::NEVER)]),
    },
    Label {
        key: "power.lid",
        name: hocus_words::settings::app_screens_view::LID_CLOSED,
        said: Said::Choices(&[("suspend", hocus_words::settings::power::SUSPEND), ("lock", hocus_words::settings::power::LOCK), ("screen-off", hocus_words::settings::power::TURN_THE_SCREEN_OFF), ("nothing", hocus_words::settings::power::DO_NOTHING)]),
    },
    Label {
        key: "power.lid-ignored-when-docked",
        name: hocus_words::settings::power::STAY_ON_WITH_A_SCREEN_PLUGGED,
        said: Said::Switch,
    },
    Label {
        key: "power.lock-before-sleep",
        name: hocus_words::settings::power::LOCK_BEFORE_SLEEPING,
        said: Said::Switch,
    },
    Label {
        key: "power.lock-on-idle",
        name: hocus_words::settings::power::LOCK_AFTER_DIMMING,
        said: Said::Switch,
    },
    Label {
        key: "power.power-button",
        name: hocus_words::settings::power::POWER_BUTTON,
        said: Said::Choices(&[("ask", hocus_words::settings::power::ASK_WHAT_TO_DO), ("suspend", hocus_words::settings::power::SUSPEND), ("power-off", hocus_words::settings::power::POWER_OFF), ("nothing", hocus_words::settings::power::DO_NOTHING)]),
    },
    Label {
        key: "programs.terminal",
        name: hocus_words::settings::app_applications_view::TERMINAL,
        said: Said::Other,
    },
    Label {
        key: "recording.folder",
        name: hocus_words::settings::app_captures_page::FOLDER,
        said: Said::Other,
    },
    Label {
        key: "recording.format",
        name: hocus_words::settings::captures::FORMAT,
        said: Said::Choices(&[("mp4", hocus_words::settings::captures::MP4), ("webm", hocus_words::settings::captures::WEBM)]),
    },
    Label {
        key: "session.restore",
        name: hocus_words::settings::applications::REOPEN_THE_APPLICATIONS_OF_THE_LAST,
        said: Said::Switch,
    },
    Label {
        key: "shortcuts.actions",
        name: hocus_words::settings::keyboard::ACTIONS,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.brightness-down",
        name: hocus_words::settings::keyboard::BRIGHTNESS_DOWN,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.brightness-up",
        name: hocus_words::settings::keyboard::BRIGHTNESS_UP,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.capture",
        name: hocus_words::settings::keyboard::SCREENSHOT,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.capture-screen",
        name: hocus_words::settings::keyboard::CAPTURE_THE_SCREEN,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.capture-window",
        name: hocus_words::settings::keyboard::CAPTURE_THE_WINDOW,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.clipboard",
        name: hocus_words::settings::keyboard::CLIPBOARD_HISTORY,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.close",
        name: hocus_words::settings::keyboard::CLOSE_THE_WINDOW,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.desktop-left",
        name: hocus_words::settings::keyboard::DESKTOP_ON_THE_LEFT,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.desktop-right",
        name: hocus_words::settings::keyboard::DESKTOP_ON_THE_RIGHT,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.emoji",
        name: hocus_words::settings::keyboard::EMOJI,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.focus-down",
        name: hocus_words::settings::keyboard::WINDOW_BELOW,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.focus-left",
        name: hocus_words::settings::keyboard::WINDOW_ON_THE_LEFT,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.focus-right",
        name: hocus_words::settings::keyboard::WINDOW_ON_THE_RIGHT,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.focus-up",
        name: hocus_words::settings::keyboard::WINDOW_ABOVE,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.join",
        name: hocus_words::settings::keyboard::JOIN_THE_MEETING,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.launcher",
        name: hocus_words::settings::keyboard::OPEN_THE_LAUNCHER,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.lock",
        name: hocus_words::settings::keyboard::LOCK_THE_SESSION,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.log-out",
        name: hocus_words::settings::keyboard::LOG_OUT,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.magnifier-in",
        name: hocus_words::settings::keyboard::ZOOM_IN,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.magnifier-off",
        name: hocus_words::settings::keyboard::TURN_THE_MAGNIFIER_OFF,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.magnifier-out",
        name: hocus_words::settings::keyboard::ZOOM_OUT,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.maximize",
        name: hocus_words::settings::keyboard::MAXIMISE,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.minimize",
        name: hocus_words::settings::keyboard::MINIMISE_THE_WINDOW,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.move-to-screen-left",
        name: hocus_words::settings::keyboard::MOVE_TO_THE_SCREEN_ON_THE,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.move-to-screen-right",
        name: hocus_words::settings::keyboard::MOVE_TO_THE_SCREEN_ON_THE_2,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.mute",
        name: hocus_words::settings::keyboard::MUTE,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.overview",
        name: hocus_words::settings::keyboard::OVERVIEW,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.quarter-down",
        name: hocus_words::settings::keyboard::BOTTOM_QUARTER,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.quarter-left",
        name: hocus_words::settings::keyboard::LEFT_QUARTER,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.quarter-right",
        name: hocus_words::settings::keyboard::RIGHT_QUARTER,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.quarter-up",
        name: hocus_words::settings::keyboard::TOP_QUARTER,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.record",
        name: hocus_words::settings::keyboard::RECORD_THE_SCREEN,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.restore",
        name: hocus_words::settings::keyboard::RESTORE,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.screen-reader",
        name: hocus_words::settings::keyboard::START_OR_STOP_THE_SCREEN_READER,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.snap-left",
        name: hocus_words::settings::desktops::LEFT_HALF,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.snap-right",
        name: hocus_words::settings::desktops::RIGHT_HALF,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.switch-windows",
        name: hocus_words::settings::keyboard::SWITCH_WINDOWS,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.switch-windows-backward",
        name: hocus_words::settings::keyboard::SWITCH_WINDOWS_BACKWARD,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.take-to-desktop-left",
        name: hocus_words::settings::keyboard::TAKE_TO_THE_DESKTOP_ON_THE,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.take-to-desktop-right",
        name: hocus_words::settings::keyboard::TAKE_TO_THE_DESKTOP_ON_THE_2,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.terminal",
        name: hocus_words::settings::keyboard::OPEN_A_TERMINAL,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.toggle-bar",
        name: hocus_words::settings::keyboard::CHANGE_THE_BAR_S_FORM,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.volume-down",
        name: hocus_words::settings::keyboard::VOLUME_DOWN,
        said: Said::Other,
    },
    Label {
        key: "shortcuts.volume-up",
        name: hocus_words::settings::keyboard::VOLUME_UP,
        said: Said::Other,
    },
    Label {
        key: "touchpad.disable-while-typing",
        name: hocus_words::settings::mouse::OFF_WHILE_TYPING,
        said: Said::Switch,
    },
    Label {
        key: "touchpad.natural-scroll",
        name: hocus_words::settings::mouse::NATURAL_SCROLLING,
        said: Said::Switch,
    },
    Label {
        key: "touchpad.off-with-mouse",
        name: hocus_words::settings::mouse::OFF_WHILE_A_MOUSE_IS_PLUGGED,
        said: Said::Switch,
    },
    Label {
        key: "touchpad.right-click",
        name: hocus_words::settings::mouse::RIGHT_CLICK_BY_PRESSING,
        said: Said::Choices(&[("two-fingers", hocus_words::settings::mouse::WITH_TWO_FINGERS), ("corner", hocus_words::settings::mouse::BOTTOM_RIGHT_CORNER)]),
    },
    Label {
        key: "touchpad.scroll",
        name: hocus_words::settings::mouse::SCROLL,
        said: Said::Choices(&[("two-fingers", hocus_words::settings::mouse::WITH_TWO_FINGERS), ("edge", hocus_words::settings::mouse::ON_THE_RIGHT_EDGE)]),
    },
    Label {
        key: "touchpad.speed",
        name: hocus_words::settings::mouse::POINTER_SPEED,
        said: Said::Other,
    },
    Label {
        key: "touchpad.swipe-launcher",
        name: hocus_words::settings::mouse::FOUR_FINGERS_UP_OR_DOWN,
        said: Said::Switch,
    },
    Label {
        key: "touchpad.tap-and-drag",
        name: hocus_words::settings::mouse::TAP_AND_DRAG,
        said: Said::Switch,
    },
    Label {
        key: "touchpad.tap-to-click",
        name: hocus_words::settings::mouse::TAP_TO_CLICK,
        said: Said::Switch,
    },
    Label {
        key: "wallpaper.animate",
        name: hocus_words::settings::wallpaper::ANIMATE_THE_BACKGROUND,
        said: Said::Switch,
    },
    Label {
        key: "wallpaper.color",
        name: hocus_words::settings::wallpaper::SOLID_COLOUR,
        said: Said::Choices(&[("#0b1020", hocus_words::link::backdrop::NIGHT), ("#2a2a30", Text::same("Graphite"))]),
    },
    Label {
        key: "wallpaper.fit",
        name: hocus_words::settings::wallpaper::FIT,
        said: Said::Choices(&[("cover", hocus_words::settings::wallpaper::COVER_THE_SCREEN), ("contain", hocus_words::settings::wallpaper::THE_WHOLE_PICTURE), ("center", hocus_words::settings::wallpaper::CENTRE_UNSCALED)]),
    },
    Label {
        key: "wallpaper.lock",
        name: hocus_words::settings::wallpaper::LOCK_SCREEN_2,
        said: Said::Choices(&[("dimmed", hocus_words::settings::app_wallpaper_page::THE_DESKTOP_S_BACKGROUND_DIMMED), ("blurred", hocus_words::settings::app_wallpaper_page::THE_DESKTOP_S_BACKGROUND_BLURRED), ("picture", hocus_words::settings::app_wallpaper_page::ANOTHER_PICTURE)]),
    },
    Label {
        key: "wallpaper.on-battery",
        name: hocus_words::settings::model::ON_BATTERY,
        said: Said::Choices(&[("half", hocus_words::settings::wallpaper::SLOWED_HALF), ("still", hocus_words::settings::wallpaper::STOPPED), ("full", hocus_words::settings::wallpaper::AS_ON_MAINS_POWER)]),
    },
    Label {
        key: "windows.alt-tab",
        name: hocus_words::settings::desktops::ALT_TAB_SHOWS,
        said: Said::Choices(&[("all", hocus_words::settings::desktops::ALL_DESKTOPS), ("desktop", hocus_words::settings::desktops::THIS_DESKTOP_ONLY)]),
    },
    Label {
        key: "windows.animation",
        name: hocus_words::settings::desktops::OPEN_AND_CLOSE,
        said: Said::Choices(&[("standard", hocus_words::settings::app_account_view::STANDARD), ("rain", hocus_words::link::moving::RAIN), ("frost", hocus_words::settings::appearance::FROST)]),
    },
    Label {
        key: "windows.magnet",
        name: hocus_words::settings::desktops::MAGNETIC_EDGES,
        said: Said::Switch,
    },
    Label {
        key: "windows.move-modifier",
        name: hocus_words::settings::desktops::MOVE_FROM_ANYWHERE,
        said: Said::Choices(&[("alt", hocus_words::settings::app_keyboard_page::ALT_DRAG), ("super", hocus_words::settings::desktops::SUPER_DRAG), ("none", hocus_words::settings::app_desktops_trial::NO_KEY)]),
    },
    Label {
        key: "windows.resize-modifier",
        name: hocus_words::settings::desktops::RESIZE_FROM_ANYWHERE,
        said: Said::Choices(&[("super", hocus_words::settings::app_keyboard_page::SUPER_RIGHT_DRAG), ("alt", hocus_words::settings::desktops::ALT_RIGHT_DRAG), ("none", hocus_words::settings::app_desktops_trial::NO_KEY)]),
    },
    Label {
        key: "windows.snap-by-drag",
        name: hocus_words::settings::desktops::SNAP_BY_DRAGGING_TO_AN_EDGE,
        said: Said::Switch,
    },
    Label {
        key: "windows.wobble",
        name: hocus_words::settings::desktops::WOBBLY_WINDOWS,
        said: Said::Switch,
    },
];
