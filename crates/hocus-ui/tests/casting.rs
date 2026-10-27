//! Panels' shadows (#281): alone in a window, the toolkit draws them in the
//! surface; under the compositor, the tree lists them for it to draw under
//! the surface, as deep as windows', and draws none.

use std::time::Duration;

use hocus_ui::{
    Cast, Casting, Kind, Length, Look, Paint, Property, Rect, Size, Style, Surface, Theme, Tree,
};

/// A transparent surface with one panel of 100×60 at (20, 20) casting a
/// shadow, faded halfway.
fn panel(theme: Theme) -> anyhow::Result<Tree> {
    let mut tree = Tree::new(theme, Size::new(140.0, 120.0), 1.0);
    let root = tree.root();
    tree.set_look(root, Look::default());
    tree.set_style(
        root,
        Style {
            padding: hocus_ui::Insets::all(20.0),
            ..Style::default()
        },
    );
    let card = tree
        .add(
            root,
            Style {
                width: Length::Px(100.0),
                height: Length::Px(60.0),
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Surface),
                radius: 12.0,
                border: None,
                shadow: 18.0,
            }),
        )
        .ok_or_else(|| anyhow::anyhow!("no panel"))?;
    tree.jump(card, Property::Opacity, 0.5);
    Ok(tree)
}

fn drawn(tree: &mut Tree) -> Surface {
    let damage = tree.update(Duration::ZERO);
    let mut surface = Surface::new(tree.size(), tree.scale());
    surface.draw(tree, &damage);
    surface
}

/// The pixels outside the panel that anything was drawn on.
fn outside(surface: &Surface) -> usize {
    let w = surface.width() as usize;
    surface
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
            !Rect::new(20.0, 20.0, 100.0, 60.0).contains(hocus_ui::Point::new(x, y)) && p[3] > 0
        })
        .count()
}

#[test]
fn in_its_surface_the_shadow_is_drawn_and_not_listed() -> anyhow::Result<()> {
    let mut tree = panel(Theme::default())?;
    let surface = drawn(&mut tree);
    assert!(outside(&surface) > 0, "the shadow around the panel");
    assert_eq!(tree.shadows(), vec![]);
    Ok(())
}

#[test]
fn under_the_compositor_the_shadow_is_listed_and_not_drawn() -> anyhow::Result<()> {
    let theme = Theme::default().with_casting(Casting::Compositor);
    let shadow = theme.shadow;
    let mut tree = panel(theme)?;
    let surface = drawn(&mut tree);
    assert_eq!(outside(&surface), 0, "nothing around the panel");
    assert_eq!(
        tree.shadows(),
        vec![Cast {
            rect: Rect::new(20.0, 20.0, 100.0, 60.0),
            radius: 12.0,
            opacity: 0.5,
            shadow,
        }]
    );
    Ok(())
}
