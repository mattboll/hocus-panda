//! The accessibility tree, and the input that goes with it.

use hocus_ui::{
    ButtonKind, Direction, Event, Input, Key, Kind, Point, Role, Size, Style, Theme, Tree,
};

const CHECK: &str = "M9 16.2 4.8 12l-1.4 1.4L9 19 21 7l-1.4-1.4z";

fn dialog() -> anyhow::Result<(Tree, hocus_ui::NodeId, hocus_ui::NodeId)> {
    let theme = Theme::default();
    let mut tree = Tree::new(theme, Size::new(320.0, 160.0), 1.0);
    let root = tree.root();
    let none = || anyhow::anyhow!("not added");
    tree.add(
        root,
        Style::default(),
        Kind::text("Quitter la session ?", theme.text.title),
    )
    .ok_or_else(none)?;
    let icon = Kind::icon(CHECK, 16.0, theme.colors.accent, None).ok_or_else(none)?;
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
