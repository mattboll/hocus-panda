//! Panels over frosted glass (#37): a box on the glass paint is solid until
//! the theme turns glass on; then it is see-through, its shadow stays
//! outside it, and the tree says where it is for the compositor to blur
//! what is behind.

use std::time::Duration;

use hocus_ui::{Glass, Kind, Length, Look, Paint, Rect, Size, Style, Surface, Theme, Tree};

/// A transparent surface with one panel of 100×60 at (20, 20), and a
/// shadow under it.
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
    tree.add(
        root,
        Style {
            width: Length::Px(100.0),
            height: Length::Px(60.0),
            ..Style::default()
        },
        Kind::Box(Look {
            background: Some(Paint::Glass),
            radius: 12.0,
            border: None,
            shadow: 18.0,
        }),
    )
    .ok_or_else(|| anyhow::anyhow!("no panel"))?;
    Ok(tree)
}

/// The pixel at `(x, y)`, premultiplied RGBA.
fn pixel(surface: &Surface, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * surface.width() + x) * 4) as usize;
    let p = &surface.pixels()[i..i + 4];
    [p[0], p[1], p[2], p[3]]
}

fn drawn(tree: &mut Tree) -> Surface {
    let damage = tree.update(Duration::ZERO);
    let mut surface = Surface::new(tree.size(), tree.scale());
    surface.draw(tree, &damage);
    surface
}

#[test]
fn without_glass_the_panel_is_solid_and_not_listed() -> anyhow::Result<()> {
    let mut tree = panel(Theme::default())?;
    let surface = drawn(&mut tree);
    assert_eq!(pixel(&surface, 70, 50)[3], 255);
    assert_eq!(tree.glass(), vec![]);
    Ok(())
}

#[test]
fn with_glass_the_panel_is_see_through_without_its_shadow() -> anyhow::Result<()> {
    let theme = Theme::default().with_glass(true);
    let glass = theme.colors.glass;
    let mut tree = panel(theme)?;
    let surface = drawn(&mut tree);
    // The middle: the surface alone, its shadow not under it.
    let [r, _, _, a] = pixel(&surface, 70, 50);
    assert_eq!(a, glass.a, "only the surface's own opacity");
    let expected = u32::from(glass.r) * u32::from(glass.a) / 255;
    assert!(u32::from(r).abs_diff(expected) <= 1, "{r} vs {expected}");
    // Below the panel, the shadow.
    assert!(pixel(&surface, 70, 86)[3] > 0);
    assert_eq!(
        tree.glass(),
        vec![Glass {
            rect: Rect::new(20.0, 20.0, 100.0, 60.0),
            radius: 12.0,
            opacity: 1.0,
        }]
    );
    Ok(())
}
