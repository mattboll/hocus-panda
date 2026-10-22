//! A spring sampled at fixed times draws the same frames every run.

use std::time::Duration;

use hocus_ui::{
    Kind, Length, Look, Size, Style, Surface, Theme, Tree,
    motion::{Curve, Spring},
    tree::Property,
};

mod common;

#[test]
fn a_spring_sampled_at_fixed_times_matches_the_references() -> anyhow::Result<()> {
    let theme = Theme::default();
    let mut tree = Tree::new(theme, Size::new(200.0, 64.0), 1.0);
    let root = tree.root();
    let chip = tree
        .add(
            root,
            Style {
                width: Length::Px(48.0),
                height: Length::Px(48.0),
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(theme.colors.accent),
                radius: 12.0,
                border: None,
                shadow: 0.0,
            }),
        )
        .ok_or_else(|| anyhow::anyhow!("no chip"))?;
    tree.set_style(
        root,
        Style {
            padding: hocus_ui::Insets::all(8.0),
            align: hocus_ui::Align::Start,
            ..Style::default()
        },
    );
    let mut surface = Surface::new(tree.size(), tree.scale());
    let first = tree.update(Duration::ZERO);
    surface.draw(&mut tree, &first);

    tree.animate(
        chip,
        Property::OffsetX,
        136.0,
        Curve::Spring(Spring::SETTLE),
    );
    for ms in [0_u64, 60, 120, 200, 320, 1500] {
        let damage = tree.update(Duration::from_millis(ms));
        surface.draw(&mut tree, &damage);
        common::expect_reference(&surface, &format!("spring-{ms:04}ms.png"))?;
    }
    assert!(!tree.is_animating(Duration::from_millis(1500)));
    Ok(())
}
