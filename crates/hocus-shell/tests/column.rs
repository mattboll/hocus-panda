//! The column as drawn, and what its buttons do.

use std::time::Duration;

use hocus_shell::{
    Action, ActiveWindow, Column, ColumnState, Lang, ScreenDesktops, TrayApp, TrayMenuEntry,
    WallTime,
};
use hocus_ui::{Input, Point, Role, Theme};

mod common;

fn state() -> ColumnState {
    ColumnState {
        can_hibernate: false,
        status: Default::default(),
        tray: Vec::new(),
        sharing: false,
        todos: Vec::new(),
        updates: None,
        time: WallTime {
            hour: 14,
            minute: 37,
            weekday: 2,
            day: 21,
            month: 10,
        },
        active: Some(ActiveWindow {
            title: "foot — ~/github/hocus-panda: cargo nextest run --workspace".into(),
            maximized: false,
        }),
        screens: vec![
            ScreenDesktops {
                output: "DP-1".into(),
                count: 4,
                shown: 1,
            },
            ScreenDesktops {
                output: "eDP-1".into(),
                count: 1,
                shown: 1,
            },
        ],
        layout: "FR".into(),
        layout_name: "fr".into(),
        user: "Camille".into(),
        notifications: vec![
            ("Mail".into(), "Revue de la MR #42".into()),
            ("Chat".into(), "Les maquettes du dock sont prêtes".into()),
        ],
    }
}

fn column(lang: Lang) -> Column {
    let mut column = Column::new(Theme::default(), lang, 760.0, 1.0);
    column.set_state(&state());
    column.draw(Duration::ZERO);
    column
}

/// The centre of the button named `name`, relative to the column.
fn button(column: &Column, name: &str) -> anyhow::Result<Point> {
    let access = column.access();
    let node = access
        .walk()
        .into_iter()
        .find(|n| n.role == Role::Button && n.label.as_deref() == Some(name))
        .ok_or_else(|| anyhow::anyhow!("no button {name:?} in\n{}", access.dump()))?;
    let r = node.rect;
    Ok(Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0))
}

fn click(column: &mut Column, at: Point) -> Vec<Action> {
    column.input(at, Input::PointerMove(at));
    column.input(at, Input::PointerButton { pressed: true });
    column.input(at, Input::PointerButton { pressed: false })
}

#[test]
fn the_column_matches_its_references() -> anyhow::Result<()> {
    let column = column(Lang::Fr);
    common::expect_reference(&column.view(), "column-fr.png")?;
    let column = self::column(Lang::En);
    common::expect_reference(&column.view(), "column-en.png")?;
    Ok(())
}

#[test]
fn the_window_buttons_ask_for_their_actions() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    for (name, action) in [
        ("Minimize", Action::Minimize),
        ("Maximize", Action::ToggleMaximize),
        ("Close", Action::Close),
    ] {
        let at = button(&column, name)?;
        assert_eq!(click(&mut column, at), vec![action]);
    }
    Ok(())
}

#[test]
fn the_desktops_line_follows_the_screen_and_opens_its_popover() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    let mut s = state();
    s.screens[0].shown = 3;
    column.set_state(&s);
    column.draw(Duration::from_millis(16));
    let line = "Desktop 3 of 4 on this screen";
    let at = button(&column, line)?;
    assert_eq!(click(&mut column, at), vec![]);
    column.draw(Duration::from_millis(32));
    let popover = column
        .popover_view()
        .ok_or_else(|| anyhow::anyhow!("no popover"))?;
    common::expect_reference(&popover, "column-desktops-popover.png")?;
    let at = button(&column, "eDP-1 · Desktop 1")?;
    assert_eq!(
        click(&mut column, at),
        vec![Action::ShowDesktop {
            output: "eDP-1".into(),
            desktop: 1
        }]
    );
    assert!(column.popover_view().is_none(), "an action closes it");
    Ok(())
}

#[test]
fn the_account_menu_logs_out() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    let at = button(&column, "Account: Camille")?;
    click(&mut column, at);
    column.draw(Duration::from_millis(16));
    let popover = column
        .popover_view()
        .ok_or_else(|| anyhow::anyhow!("no menu"))?;
    common::expect_reference(&popover, "column-account-menu.png")?;
    let dump = column.access().dump();
    assert!(dump.contains("button \"Lock\""), "{dump}");
    assert!(dump.contains("button \"Suspend\""), "{dump}");
    // Not offered where the machine does not hibernate.
    assert!(!dump.contains("Hibernate"), "{dump}");
    let at = button(&column, "Log out")?;
    assert_eq!(click(&mut column, at), vec![Action::LogOut]);
    Ok(())
}

#[test]
fn the_tray_shows_its_applications_and_their_menus() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    assert!(
        button(&column, "Tray: 1 application").is_err(),
        "no chip when empty"
    );
    let app = |key: &str, title: &str, attention| TrayApp {
        key: key.into(),
        title: title.into(),
        attention,
        icon: Some((1, 1, vec![0x80, 0, 0, 0x80].into())),
    };
    column.set_state(&ColumnState {
        tray: vec![app(":1.4/A", "Chat", true), app(":1.5/B", "Sync", false)],
        ..state()
    });
    column.draw(Duration::ZERO);
    let at = button(&column, "Tray: 2 applications, 1 waiting")?;
    click(&mut column, at);
    column.draw(Duration::ZERO);
    let at = button(&column, "Chat")?;
    assert_eq!(
        click(&mut column, at),
        vec![Action::TrayActivate(":1.4/A".into())]
    );
    // A right click on a line, or its menu button, asks for the menu.
    let at = button(&column, "Tray: 2 applications, 1 waiting")?;
    click(&mut column, at);
    column.draw(Duration::ZERO);
    let at = button(&column, "Sync")?;
    assert_eq!(
        column.secondary_click(at),
        vec![Action::TrayMenu(":1.5/B".into())]
    );
    let at = button(&column, "Chat’s menu")?;
    assert_eq!(
        click(&mut column, at),
        vec![Action::TrayMenu(":1.4/A".into())]
    );
    let entry = |id, label: &str| TrayMenuEntry {
        id,
        label: label.into(),
        enabled: true,
        separator: false,
        toggle: None,
        children: Vec::new(),
    };
    column.show_tray_menu(":1.4/A".into(), vec![entry(1, "Open"), entry(2, "Quit")]);
    column.draw(Duration::ZERO);
    let at = button(&column, "Quit")?;
    assert_eq!(
        click(&mut column, at),
        vec![Action::TrayChosen {
            key: ":1.4/A".into(),
            id: 2
        }]
    );
    // The chip goes with the last application.
    column.set_state(&state());
    column.draw(Duration::ZERO);
    assert!(button(&column, "Tray: 2 applications").is_err());
    assert!(button(&column, "Keyboard layout: fr").is_ok());
    Ok(())
}

#[test]
fn while_sharing_the_column_says_so_and_stops_it() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    assert!(
        button(&column, "Stop sharing").is_err(),
        "hidden when not sharing"
    );
    column.set_state(&ColumnState {
        sharing: true,
        ..state()
    });
    column.draw(Duration::ZERO);
    let at = button(&column, "Stop sharing")?;
    assert_eq!(click(&mut column, at), vec![Action::StopSharing]);
    Ok(())
}

#[test]
fn without_a_window_the_controls_are_disabled() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    column.set_state(&ColumnState {
        active: None,
        ..state()
    });
    column.draw(Duration::from_millis(16));
    let at = button(&column, "Close")?;
    assert_eq!(click(&mut column, at), vec![]);
    assert!(column.access().dump().contains("No window"));
    Ok(())
}
