//! Quick settings as drawn, and what they ask for.

use std::time::Duration;

use hocus_shell::{KeyboardLayout, Lang, QuickAction, QuickState, Sheet, Tile};
use hocus_ui::{Input, Key, Point, Role, Theme};

mod common;

fn state(lang: Lang) -> QuickState {
    let works = |t: Tile| {
        matches!(
            t,
            Tile::Wifi | Tile::Bluetooth | Tile::DoNotDisturb | Tile::Airplane
        )
    };
    QuickState {
        profiles: [true; 3],
        tiles: hocus_shell::quick::tiles_in_order()
            .into_iter()
            .map(|t| {
                let on = matches!(t, Tile::Wifi);
                let detail = if works(t) {
                    lang.on_off(on)
                } else {
                    lang.coming()
                };
                (t, on, detail.to_owned(), works(t))
            })
            .collect(),
        profile: 1,
        volume: 40,
        output_device: String::new(),
        brightness: vec![("eDP-1".into(), 70), ("DP-1".into(), 85)],
        battery: Some(lang.battery_left(64, Some(230))),
        layouts: vec![
            KeyboardLayout {
                name: match lang {
                    Lang::En => "French (AZERTY)",
                    Lang::Fr => "Français (AZERTY)",
                }
                .into(),
                short: "FR".into(),
            },
            KeyboardLayout {
                name: match lang {
                    Lang::En => "English (US)",
                    Lang::Fr => "Anglais (US)",
                }
                .into(),
                short: "US".into(),
            },
        ]
        .into(),
        layout: 0,
        notifications: 2,
    }
}

fn sheet(lang: Lang) -> Sheet {
    let mut sheet = Sheet::new(Theme::default(), lang, 900.0, 1.0);
    sheet.set_state(&state(lang));
    sheet.open();
    sheet.draw(Duration::ZERO);
    sheet
}

fn find(sheet: &Sheet, role: Role, starts: &str) -> anyhow::Result<Point> {
    let access = sheet.access();
    let node = access
        .walk()
        .into_iter()
        .find(|n| n.role == role && n.label.as_deref().is_some_and(|l| l.starts_with(starts)))
        .ok_or_else(|| anyhow::anyhow!("no {role:?} {starts:?} in\n{}", access.dump()))?;
    let r = node.rect;
    Ok(Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0))
}

fn click(sheet: &mut Sheet, at: Point) -> Vec<QuickAction> {
    sheet.input(at, Input::PointerMove(at));
    let mut actions = sheet.input(at, Input::PointerButton { pressed: true });
    actions.extend(sheet.input(at, Input::PointerButton { pressed: false }));
    actions
}

#[test]
fn the_sheet_matches_its_reference() -> anyhow::Result<()> {
    common::expect_reference(&sheet(Lang::Fr).view(), "quick-fr.png")
}

#[test]
fn tiles_profiles_sliders_and_layouts_ask_for_their_change() -> anyhow::Result<()> {
    let mut sheet = sheet(Lang::En);
    let at = find(&sheet, Role::Button, "Bluetooth")?;
    assert_eq!(
        click(&mut sheet, at),
        vec![QuickAction::Toggle(Tile::Bluetooth, true)]
    );
    let at = find(&sheet, Role::Button, "Performance")?;
    assert_eq!(click(&mut sheet, at), vec![QuickAction::Profile(2)]);
    let at = find(&sheet, Role::Button, "Keyboard layout: English (US)")?;
    assert_eq!(click(&mut sheet, at), vec![QuickAction::Layout(1)]);
    let at = find(&sheet, Role::Button, "Clear all")?;
    assert_eq!(click(&mut sheet, at), vec![QuickAction::ClearNotifications]);
    // A tile that does not work yet is not a button.
    assert!(find(&sheet, Role::Button, "Night light").is_err());
    // The volume slider, dragged to its far end.
    let at = find(&sheet, Role::Slider, "Volume")?;
    sheet.input(at, Input::PointerMove(at));
    sheet.input(at, Input::PointerButton { pressed: true });
    let end = Point::new(at.x + 400.0, at.y);
    let actions = sheet.input(end, Input::PointerMove(end));
    sheet.input(end, Input::PointerButton { pressed: false });
    assert_eq!(actions.last(), Some(&QuickAction::Volume(100)));
    Ok(())
}

#[test]
fn escape_or_a_click_outside_closes_it() {
    let mut sheet = sheet(Lang::En);
    assert_eq!(sheet.key(Key::Escape), vec![QuickAction::Closed]);
    assert!(!sheet.is_open());
    sheet.open();
    let outside = Point::new(5.0, 5.0);
    assert_eq!(
        sheet.input(outside, Input::PointerButton { pressed: true }),
        vec![QuickAction::Closed]
    );
}
