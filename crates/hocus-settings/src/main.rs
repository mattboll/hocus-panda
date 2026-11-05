//! `hocus-settings [SECTION] [--pair ADDRESS]`: the Settings' window, on
//! its section (`power`), or brought forward on it when it is open
//! already; `--pair` pairs a Bluetooth device on its page.

use std::{borrow::Cow, path::PathBuf, process::ExitCode};

use hocus_settings::{
    app::{self, App},
    lang::Lang,
    model::SectionId,
    single::{self, Again, Start},
    worker,
};
use iced::window;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("hocus_settings=info,hocus_bluetooth=info")
            }),
        )
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_writer(std::io::stderr)
        .init();
    let arg = std::env::args().nth(1);
    if arg.as_deref().is_some_and(|a| a == "--help" || a == "-h") {
        let slugs: Vec<&str> = SectionId::ALL.iter().map(|s| s.section().slug).collect();
        println!(
            "hocus-settings [SECTION] [--pair ADDRESS]\n\nSections: {}",
            slugs.join(", ")
        );
        return ExitCode::SUCCESS;
    }
    let section = arg.as_deref().and_then(SectionId::from_slug);
    if arg.is_some() && section.is_none() {
        tracing::warn!(section = arg, "no such section: the home opens");
    }
    // `--pair ADDRESS`: a Bluetooth device to pair (the bar's popover).
    let pair = std::env::args()
        .skip_while(|a| a != "--pair")
        .nth(1)
        .filter(|a| !a.is_empty() && !a.contains(char::is_whitespace));
    let again = Again {
        pair: pair.clone(),
        section: section.map(|s| s.section().slug.to_owned()),
        token: std::env::var("XDG_ACTIVATION_TOKEN")
            .ok()
            .filter(|t| !t.is_empty()),
    };
    let listener = match single::start(&again) {
        Start::Told => return ExitCode::SUCCESS,
        Start::First(listener) => listener,
    };
    hocus_settings::renderer::choose();
    // Before the window: it reads folders.
    hocus_settings::bar::find_fonts();
    hocus_settings::appearance::find_cursor_themes();
    let start = app::Start {
        lang: Lang::of_messages(),
        places: worker::Places {
            file: hocus_config::file(),
            state: hocus_config::state_dir(),
            supplies: std::env::var_os("HOCUS_POWER_SUPPLY_DIR")
                .map_or_else(|| PathBuf::from("/sys/class/power_supply"), PathBuf::from),
        },
        section,
        pair,
        user: user_name(),
        listener: std::sync::Arc::new(std::sync::Mutex::new(listener)),
    };
    tracing::info!(
        file = ?start.places.file,
        section = ?section,
        "the Settings open"
    );
    let mut settings = iced::Settings {
        antialiasing: true,
        default_font: app::default_font(),
        ..iced::Settings::default()
    };
    settings.fonts = app::fonts().into_iter().map(Cow::Borrowed).collect();
    let ran = iced::application(move || App::new(start.clone()), App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .scale_factor(App::text_scale)
        .theme(|_: &App| {
            if hocus_settings::theme::now().scheme == hocus_link::theme::Scheme::Light {
                iced::Theme::Light
            } else {
                iced::Theme::Dark
            }
        })
        .style(|app: &App, _: &iced::Theme| iced::theme::Style {
            background_color: app::background_of(app),
            text_color: app::text(),
        })
        .settings(settings)
        .window(window::Settings {
            size: iced::Size::new(app::SIZE.0, app::SIZE.1),
            min_size: Some(iced::Size::new(app::LEAST.0, app::LEAST.1)),
            // The compositor's decorations (xdg-decoration): under
            // Hocuspanda, no title bar but its grip at the top edge
            // (windows.md §7); the header is still the window's own.
            decorations: true,
            // ✕, Esc, Ctrl+W and the compositor's close all go through the
            // window, which writes what is pending first.
            exit_on_close_request: false,
            platform_specific: window::settings::PlatformSpecific {
                application_id: "hocus-settings".to_owned(),
                ..Default::default()
            },
            ..window::Settings::default()
        })
        .run();
    single::stop();
    // The computer hidden again and the search stopped before the program
    // goes (BlueZ keeps it visible otherwise until its timeout).
    app::stop_bluetooth();
    tracing::info!("the Settings closed");
    match ran {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(%err, "the Settings stopped");
            ExitCode::FAILURE
        }
    }
}

/// The user's name, as the bar says it: `HOCUS_USER_NAME`, else their
/// account entry's full name, else their login.
fn user_name() -> String {
    if let Ok(name) = std::env::var("HOCUS_USER_NAME")
        && !name.trim().is_empty()
    {
        return name.trim().to_owned();
    }
    let login = std::env::var("USER").unwrap_or_default();
    let uid = rustix::process::getuid().as_raw().to_string();
    let entry = std::fs::read_to_string("/etc/passwd")
        .ok()
        .and_then(|passwd| {
            passwd.lines().find_map(|line| {
                let fields: Vec<&str> = line.split(':').collect();
                (fields.get(2) == Some(&uid.as_str()))
                    .then(|| {
                        fields
                            .get(4)
                            .map(|g| g.split(',').next().unwrap_or("").to_owned())
                    })
                    .flatten()
            })
        })
        .filter(|name| !name.trim().is_empty());
    entry.unwrap_or(login)
}
