//! Only what changed is drawn again, and drawing only that gives the same
//! pixels as drawing everything.

use std::time::Duration;

use hocus_ui::{
    Insets, Kind, Length, Look, NodeId, Paint, Rect, Size, Style, Surface, Theme, Tree,
    motion::{Curve, Easing},
    tree::Property,
};

const MS: fn(u64) -> Duration = Duration::from_millis;

/// A card with three lines of text.
fn card() -> anyhow::Result<(Tree, [NodeId; 3])> {
    let theme = Theme::default();
    let mut tree = Tree::new(theme.clone(), Size::new(320.0, 200.0), 2.0);
    let root = tree.root();
    let card = tree
        .add(
            root,
            Style {
                width: Length::Fill,
                padding: Insets::all(16.0),
                gap: 8.0,
                ..Style::default()
            },
            Kind::Box(Look {
                background: Some(Paint::Surface),
                radius: theme.radii.large,
                border: Some(Paint::Border),
                shadow: 0.0,
            }),
        )
        .ok_or_else(|| anyhow::anyhow!("no card"))?;
    let mut line = |text: &str| {
        tree.add(card, Style::default(), Kind::text(text, theme.text.body))
            .ok_or_else(|| anyhow::anyhow!("no line"))
    };
    let lines = [
        line("Processeur 23\u{202f}%")?,
        line("Mémoire 6,4\u{a0}Go sur 32\u{a0}Go")?,
        line("Disque 41\u{202f}%")?,
    ];
    Ok((tree, lines))
}

/// Draws everything at `now` on a fresh surface.
fn full_redraw(tree: &mut Tree, now: Duration) -> Surface {
    tree.resize(Size::new(tree.size().w + 1.0, tree.size().h), tree.scale());
    tree.resize(Size::new(tree.size().w - 1.0, tree.size().h), tree.scale());
    let damage = tree.update(now);
    let mut surface = Surface::new(tree.size(), tree.scale());
    surface.draw(tree, &damage);
    surface
}

#[test]
fn the_first_frame_draws_everything_and_a_still_one_nothing() -> anyhow::Result<()> {
    let (mut tree, _) = card()?;
    assert_eq!(tree.update(MS(0)), vec![Rect::new(0.0, 0.0, 320.0, 200.0)]);
    assert_eq!(tree.update(MS(16)), vec![]);
    Ok(())
}

#[test]
fn a_changed_text_damages_only_its_area() -> anyhow::Result<()> {
    let (mut tree, [cpu, memory, disk]) = card()?;
    let mut surface = Surface::new(tree.size(), tree.scale());
    let first = tree.update(MS(0));
    surface.draw(&mut tree, &first);

    let before = tree
        .rect(memory)
        .ok_or_else(|| anyhow::anyhow!("no rect"))?;
    tree.set_text(memory, "Mémoire 6,5\u{a0}Go sur 32\u{a0}Go");
    let damage = tree.update(MS(16));
    let after = tree
        .rect(memory)
        .ok_or_else(|| anyhow::anyhow!("no rect"))?;
    assert_eq!(damage, vec![before.union(&after)]);
    for other in [cpu, disk] {
        let r = tree.rect(other).ok_or_else(|| anyhow::anyhow!("no rect"))?;
        assert!(
            !damage[0].intersects(&r),
            "{other:?} at {r:?} is not damaged"
        );
    }
    // In physical pixels, what the compositor is told.
    let drawn = surface.draw(&mut tree, &damage);
    assert_eq!(drawn, vec![damage[0].to_physical(2.0)]);
    Ok(())
}

#[test]
fn drawing_the_damage_gives_the_pixels_of_a_full_redraw() -> anyhow::Result<()> {
    let (mut tree, [cpu, memory, _]) = card()?;
    let mut surface = Surface::new(tree.size(), tree.scale());
    let first = tree.update(MS(0));
    surface.draw(&mut tree, &first);

    tree.set_text(
        memory,
        "Mémoire 12\u{a0}Go sur 32\u{a0}Go, et une ligne qui passe à la suivante",
    );
    tree.animate(
        cpu,
        Property::OffsetX,
        24.0,
        Curve::Ease(Easing::OutCubic, MS(250)),
    );
    for ms in [16, 100, 200, 266, 300] {
        let damage = tree.update(MS(ms));
        surface.draw(&mut tree, &damage);
    }
    let expected = full_redraw(&mut tree, MS(300));
    // Missing damage leaves whole areas wrong; the renderer may round a
    // pixel by one level differently depending on where a region starts.
    let differ: Vec<usize> = surface
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(expected.pixels().as_chunks::<4>().0)
        .enumerate()
        .filter(|(_, (a, e))| a.iter().zip(*e).any(|(a, e)| a.abs_diff(*e) > 2))
        .map(|(i, _)| i)
        .collect();
    let w = surface.width() as usize;
    assert!(
        differ.is_empty(),
        "{} pixels differ, the first at {:?}",
        differ.len(),
        differ.first().map(|i| (i % w, i / w))
    );
    Ok(())
}
