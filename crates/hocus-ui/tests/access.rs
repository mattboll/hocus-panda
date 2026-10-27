//! The accessibility tree, and the input that goes with it.

use hocus_ui::{
    ButtonKind, Direction, Event, Input, Key, Kind, Paint, Point, Role, Size, Style, Theme, Tree,
};

const CHECK: &str = "M9 16.2 4.8 12l-1.4 1.4L9 19 21 7l-1.4-1.4z";

fn dialog() -> anyhow::Result<(Tree, hocus_ui::NodeId, hocus_ui::NodeId)> {
    let theme = Theme::default();
    let mut tree = Tree::new(theme.clone(), Size::new(320.0, 160.0), 1.0);
    let root = tree.root();
    let none = || anyhow::anyhow!("not added");
    tree.add(
        root,
        Style::default(),
        Kind::text("Quitter la session ?", theme.text.title),
    )
    .ok_or_else(none)?;
    let icon = Kind::icon(CHECK, 16.0, Paint::Accent, None).ok_or_else(none)?;
    tree.add(root, Style::default(), icon).ok_or_else(none)?;
    let row = tree
        .add(
            root,
            Style {
                direction: Direction::Row,
                gap: 8.0,
                ..Style::default()
            },
            Kind::Box(hocus_ui::Look::default()),
        )
        .ok_or_else(none)?;
    let cancel = tree
        .add(
            row,
            Style::default(),
            Kind::button("Annuler", ButtonKind::Plain, &theme),
        )
        .ok_or_else(none)?;
    let quit = tree
        .add(
            row,
            Style::default(),
            Kind::button("Quitter", ButtonKind::Primary, &theme),
        )
        .ok_or_else(none)?;
    Ok((tree, cancel, quit))
}

#[test]
fn every_button_has_a_role_and_a_label() -> anyhow::Result<()> {
    let (mut tree, _, _) = dialog()?;
    tree.update(std::time::Duration::ZERO);
    let access = tree.access();
    let buttons: Vec<_> = access
        .walk()
        .into_iter()
        .filter(|n| n.role == Role::Button)
        .collect();
    assert_eq!(buttons.len(), 2);
    for button in buttons {
        assert!(
            button.label.as_deref().is_some_and(|l| !l.is_empty()),
            "{button:?}"
        );
    }
    // The decorative icon is left out.
    let dump = access.dump();
    assert!(!dump.contains("icon"), "{dump}");
    assert!(dump.contains("button \"Quitter\""), "{dump}");
    Ok(())
}

#[test]
fn tab_moves_focus_and_enter_activates() -> anyhow::Result<()> {
    let (mut tree, cancel, quit) = dialog()?;
    tree.update(std::time::Duration::ZERO);
    tree.input(Input::Key(Key::Tab));
    assert_eq!(tree.focused(), Some(cancel));
    tree.input(Input::Key(Key::Tab));
    assert_eq!(tree.focused(), Some(quit));
    assert_eq!(
        tree.input(Input::Key(Key::Enter)),
        vec![Event::Activated(quit)]
    );
    tree.input(Input::Key(Key::Tab));
    assert_eq!(tree.focused(), Some(cancel), "focus goes round");
    assert!(tree.access().dump().contains("button \"Annuler\""));
    Ok(())
}

#[test]
fn a_click_is_a_press_and_a_release_on_the_same_button() -> anyhow::Result<()> {
    let (mut tree, cancel, quit) = dialog()?;
    tree.update(std::time::Duration::ZERO);
    let center = |id| {
        tree.rect(id)
            .map(|r| Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0))
            .ok_or_else(|| anyhow::anyhow!("no rect"))
    };
    let (on_cancel, on_quit) = (center(cancel)?, center(quit)?);
    tree.input(Input::PointerMove(on_quit));
    tree.input(Input::PointerButton { pressed: true });
    assert_eq!(
        tree.input(Input::PointerButton { pressed: false }),
        vec![Event::Activated(quit)]
    );
    // Pressed on one, released on the other: nothing.
    tree.input(Input::PointerMove(on_cancel));
    tree.input(Input::PointerButton { pressed: true });
    tree.input(Input::PointerMove(on_quit));
    assert_eq!(tree.input(Input::PointerButton { pressed: false }), vec![]);
    assert_eq!(tree.hit(on_cancel, true), Some(cancel));
    Ok(())
}

/// Removes everything under the root and makes it again, as a view built
/// anew when something outside changed: another title, and the buttons new
/// nodes, a box deeper in the tree, where they showed.
fn dialog_again(tree: &mut Tree, with_quit: bool) -> anyhow::Result<hocus_ui::NodeId> {
    let theme = Theme::default();
    let root = tree.root();
    for child in tree.children(root) {
        tree.remove(child);
    }
    let none = || anyhow::anyhow!("not added");
    tree.add(
        root,
        Style::default(),
        Kind::text("Fermer la session ?", theme.text.title),
    )
    .ok_or_else(none)?;
    let icon = Kind::icon(CHECK, 16.0, Paint::Accent, None).ok_or_else(none)?;
    tree.add(root, Style::default(), icon).ok_or_else(none)?;
    let holder = tree
        .add(root, Style::default(), Kind::Box(hocus_ui::Look::default()))
        .ok_or_else(none)?;
    let row = tree
        .add(
            holder,
            Style {
                direction: Direction::Row,
                gap: 8.0,
                ..Style::default()
            },
            Kind::Box(hocus_ui::Look::default()),
        )
        .ok_or_else(none)?;
    let cancel = tree
        .add(
            row,
            Style::default(),
            Kind::button("Annuler", ButtonKind::Plain, &theme),
        )
        .ok_or_else(none)?;
    if with_quit {
        tree.add(
            row,
            Style::default(),
            Kind::button("Quitter", ButtonKind::Primary, &theme),
        )
        .ok_or_else(none)?;
    }
    Ok(cancel)
}

fn find(tree: &Tree, label: &str) -> Option<hocus_ui::NodeId> {
    let access = tree.access();
    let center = access
        .walk()
        .into_iter()
        .find(|n| n.role == Role::Button && n.label.as_deref() == Some(label))
        .map(|n| Point::new(n.rect.x + n.rect.w / 2.0, n.rect.y + n.rect.h / 2.0))?;
    tree.hit(center, true)
}

#[test]
fn a_press_survives_its_view_built_anew_before_the_release() -> anyhow::Result<()> {
    let (mut tree, cancel, quit) = dialog()?;
    tree.update(std::time::Duration::ZERO);
    let on_cancel = tree
        .rect(cancel)
        .map(|r| Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0))
        .ok_or_else(|| anyhow::anyhow!("no rect"))?;
    tree.input(Input::PointerMove(on_cancel));
    tree.input(Input::PointerButton { pressed: true });

    // Built anew between the press and the release, with nothing told of
    // the pointer since: the new "Annuler" is under it, and takes the click.
    let again = dialog_again(&mut tree, true)?;
    assert!(tree.rect(quit).is_none(), "the old nodes are gone");
    assert_eq!(
        tree.input(Input::PointerButton { pressed: false }),
        vec![Event::Activated(again)]
    );

    // Built anew and told of the pointer, as views do to show what is
    // under it: the press shows on the new node, and the release clicks
    // it.
    tree.input(Input::PointerButton { pressed: true });
    let anew = dialog_again(&mut tree, false)?;
    tree.layout();
    tree.input(Input::PointerMove(on_cancel));
    assert_eq!(find(&tree, "Annuler"), Some(anew));
    assert_eq!(tree.focused(), Some(anew), "the press focused it");
    assert_eq!(
        tree.input(Input::PointerButton { pressed: false }),
        vec![Event::Activated(anew)]
    );
    Ok(())
}

#[test]
fn a_press_on_a_node_gone_with_the_rebuild_clicks_nothing() -> anyhow::Result<()> {
    let (mut tree, _, quit) = dialog()?;
    tree.update(std::time::Duration::ZERO);
    let on_quit = tree
        .rect(quit)
        .map(|r| Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0))
        .ok_or_else(|| anyhow::anyhow!("no rect"))?;
    tree.input(Input::PointerMove(on_quit));
    tree.input(Input::PointerButton { pressed: true });
    dialog_again(&mut tree, false)?;
    tree.layout();
    tree.input(Input::PointerMove(on_quit));
    assert_eq!(find(&tree, "Quitter"), None);
    assert_eq!(tree.input(Input::PointerButton { pressed: false }), vec![]);

    // Nor does the one that is there, pressed on before a rebuild with
    // the pointer elsewhere.
    let cancel = dialog_again(&mut tree, true)?;
    tree.update(std::time::Duration::ZERO);
    let on_cancel = tree
        .rect(cancel)
        .map(|r| Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0))
        .ok_or_else(|| anyhow::anyhow!("no rect"))?;
    tree.input(Input::PointerMove(on_cancel));
    tree.input(Input::PointerButton { pressed: true });
    dialog_again(&mut tree, true)?;
    tree.layout();
    let quit = find(&tree, "Quitter").ok_or_else(|| anyhow::anyhow!("no Quitter"))?;
    let on_quit = tree
        .rect(quit)
        .map(|r| Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0))
        .ok_or_else(|| anyhow::anyhow!("no rect"))?;
    tree.input(Input::PointerMove(on_quit));
    assert_eq!(tree.input(Input::PointerButton { pressed: false }), vec![]);
    Ok(())
}
