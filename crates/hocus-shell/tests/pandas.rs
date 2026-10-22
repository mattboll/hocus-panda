//! The pandas as drawn (#47): both characters in every mood and the logo,
//! still and part way through their motion.

use std::time::Duration;

use hocus_shell::column::View;
use hocus_shell::panda::{Character, Mood, Panda};
use hocus_ui::{Kind, Length, Look, Paint, Point, Size, Style, Surface, Theme, ThemeName, Tree};

mod common;

const CELL: f32 = 120.0;

/// Every mood of both characters, a row each, and the logo; all moving
/// since `since`, drawn at `now`.
fn sheet(theme: &Theme, since: Duration, now: Duration) -> anyhow::Result<Surface> {
    let size = Size::new(CELL * 9.0, CELL * 3.0);
    let mut tree = Tree::new(theme.clone(), size, 1.0);
    let root = tree.root();
    let back = tree
        .add(
            root,
            Style {
                width: Length::Px(size.w),
                height: Length::Px(size.h),
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Background),
                radius: 0.0,
                border: None,
                shadow: 0.0,
            }),
        )
        .ok_or_else(|| anyhow::anyhow!("no background"))?;
    let mut place = |at: Point, panda: Panda| {
        tree.add(
            back,
            Style {
                at: Some(at),
                width: Length::Px(CELL),
                height: Length::Px(CELL),
                ..Style::default()
            },
            Kind::Custom {
                draw: Box::new(panda.moving_from(since)),
                label: None,
            },
        )
        .ok_or_else(|| anyhow::anyhow!("no panda"))
    };
    for (row, character) in [Character::Apprentice, Character::Watcher]
        .into_iter()
        .enumerate()
    {
        for (i, mood) in Mood::ALL.into_iter().enumerate() {
            place(
                Point::new(CELL * i as f32, CELL * row as f32),
                Panda::themed(character, mood, theme),
            )?;
        }
    }
    place(Point::new(0.0, CELL * 2.0), Panda::logo(theme))?;
    let damage = tree.update(now);
    let mut surface = Surface::new(tree.size(), tree.scale());
    surface.draw(&mut tree, &damage);
    Ok(surface)
}

fn expect(surface: &Surface, name: &str) -> anyhow::Result<()> {
    common::expect_reference(
        &View {
            pixels: surface.pixels(),
            width: surface.width(),
            height: surface.height(),
            at: Point::new(0.0, 0.0),
        },
        name,
    )
}

#[test]
fn both_characters_in_every_mood_and_the_logo() -> anyhow::Result<()> {
    let theme = Theme::default();
    // Before they were asked to move: the still poses.
    let still = sheet(&theme, Duration::from_secs(10), Duration::ZERO)?;
    expect(&still, "pandas.png")?;
    // Part way through: the juggled balls fly, the hat is thrown, smoke rises.
    let moving = sheet(&theme, Duration::ZERO, Duration::from_millis(1700))?;
    expect(&moving, "pandas-moving.png")
}

#[test]
fn on_a_light_theme_too() -> anyhow::Result<()> {
    let theme = Theme::builtin(ThemeName::Light);
    expect(
        &sheet(&theme, Duration::from_secs(10), Duration::ZERO)?,
        "pandas-light.png",
    )
}
