//! Themes and accents, as drawn.

use std::time::Duration;

use hocus_ui::{
    Accent, Align, ButtonKind, Color, Direction, Insets, Kind, Length, Look, NodeId, Paint, Size,
    Style, Surface, Theme, ThemeName, Tree,
};

mod common;

/// A card like the column's: a clock, a title, a line, a chip and a button.
fn sample(theme: Theme) -> anyhow::Result<(Tree, NodeId)> {
    let mut tree = Tree::new(theme.clone(), Size::new(300.0, 236.0), 1.0);
    let none = || anyhow::anyhow!("not added");
    let root = tree.root();
    tree.set_style(
        root,
        Style {
            padding: Insets::all(12.0),
            gap: 8.0,
            ..Style::default()
        },
    );
    tree.add(
        root,
        Style::default(),
        Kind::text("14:32", theme.text.display),
    )
    .ok_or_else(none)?;
    let card = tree
        .add(
            root,
            Style {
                padding: Insets::all(12.0),
                gap: 6.0,
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Surface),
                radius: theme.radii.large,
                border: Some(Paint::Border),
                shadow: 0.0,
            }),
        )
        .ok_or_else(none)?;
    tree.add(
        card,
        Style::default(),
        Kind::text(
            "PROCHAINE RÉUNION",
            theme.text.caption.with_color(Paint::Accent),
        ),
    )
    .ok_or_else(none)?;
    tree.add(
        card,
        Style::default(),
        Kind::text("Revue d'architecture, 3e étage", theme.text.body),
    )
    .ok_or_else(none)?;
    let row = tree
        .add(
            card,
            Style {
                direction: Direction::Row,
                gap: 8.0,
                align: Align::Center,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )
        .ok_or_else(none)?;
    let chip = tree
        .add(
            row,
            Style {
                width: Length::Px(24.0),
                height: Length::Px(24.0),
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Accent),
                radius: 6.0,
                ..Look::default()
            }),
        )
        .ok_or_else(none)?;
    tree.add(
        row,
        Style::default(),
        Kind::text("dans 12\u{a0}min", theme.text.body.with_color(Paint::Muted)),
    )
    .ok_or_else(none)?;
    tree.add(
        row,
        Style::default(),
        Kind::button("Rejoindre", ButtonKind::Primary, &theme),
    )
    .ok_or_else(none)?;
    Ok((tree, chip))
}

fn pixel(surface: &Surface, x: u32, y: u32) -> Color {
    let i = ((y * surface.width() + x) * 4) as usize;
    let p = &surface.pixels()[i..i + 4];
    Color::rgba(p[0], p[1], p[2], p[3])
}

#[test]
fn every_theme_draws_the_same_card_in_its_own_colours_and_faces() -> anyhow::Result<()> {
    for name in ThemeName::ALL {
        let (mut tree, _) = sample(Theme::builtin(name))?;
        let mut surface = Surface::new(tree.size(), tree.scale());
        let damage = tree.update(Duration::ZERO);
        surface.draw(&mut tree, &damage);
        common::expect_reference(&surface, &format!("theme-{}.png", name.name()))?;
    }
    Ok(())
}

#[test]
fn an_amber_accent_redraws_accent_elements_in_the_themes_amber() -> anyhow::Result<()> {
    for name in [ThemeName::Dark, ThemeName::Light] {
        let theme = Theme::builtin(name);
        let (mut tree, chip) = sample(theme.clone())?;
        let mut surface = Surface::new(tree.size(), tree.scale());
        let damage = tree.update(Duration::ZERO);
        surface.draw(&mut tree, &damage);
        let r = tree.rect(chip).ok_or_else(|| anyhow::anyhow!("no chip"))?;
        let (x, y) = ((r.x + r.w / 2.0) as u32, (r.y + r.h / 2.0) as u32);
        assert_eq!(pixel(&surface, x, y), theme.color(Paint::Accent));

        let amber = theme.with_accent(Accent::Amber);
        let expected = amber.accents.amber;
        tree.set_theme(amber);
        let damage = tree.update(Duration::from_millis(16));
        surface.draw(&mut tree, &damage);
        assert_eq!(pixel(&surface, x, y), expected, "{}", name.name());
    }
    Ok(())
}
