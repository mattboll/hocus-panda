//! Text fields and sliders, from the keyboard and the pointer.

use std::time::Duration;

use hocus_ui::{Event, Input, Key, Kind, Length, Point, Size, Style, TextField, Theme, Tree};

#[test]
fn a_field_takes_text_and_edits_it() -> anyhow::Result<()> {
    let theme = Theme::default();
    let mut tree = Tree::new(theme.clone(), Size::new(300.0, 60.0), 1.0);
    let root = tree.root();
    let field = tree
        .add(
            root,
            Style::default(),
            Kind::TextField(Box::new(TextField::new(
                "Name",
                "Your name",
                theme.text.body,
            ))),
        )
        .ok_or_else(|| anyhow::anyhow!("no field"))?;
    tree.set_focus(Some(field));
    assert_eq!(tree.text_input("Zoé"), vec![Event::Changed(field)]);
    tree.input(Input::Key(Key::Left));
    tree.input(Input::Key(Key::Backspace));
    assert_eq!(tree.field_value(field), Some("Zé"));
    tree.input(Input::Key(Key::Home));
    tree.input(Input::Key(Key::Delete));
    tree.input(Input::Key(Key::End));
    tree.text_input("!");
    assert_eq!(tree.field_value(field), Some("é!"));
    Ok(())
}

#[test]
fn a_slider_follows_a_drag_and_the_arrows() -> anyhow::Result<()> {
    let mut tree = Tree::new(Theme::default(), Size::new(200.0, 40.0), 1.0);
    let root = tree.root();
    let slider = tree
        .add(
            root,
            Style {
                width: Length::Px(200.0),
                ..Style::default()
            },
            Kind::Slider {
                value: 0.4,
                step: 0.05,
                name: "Volume".into(),
            },
        )
        .ok_or_else(|| anyhow::anyhow!("no slider"))?;
    tree.update(Duration::ZERO);
    tree.input(Input::PointerMove(Point::new(100.0, 12.0)));
    assert_eq!(
        tree.input(Input::PointerButton { pressed: true }),
        vec![Event::Changed(slider)]
    );
    assert_eq!(tree.slider_value(slider), Some(0.5));
    // Dragging past the end stops at 1.
    tree.input(Input::PointerMove(Point::new(260.0, 30.0)));
    tree.input(Input::PointerButton { pressed: false });
    assert_eq!(tree.slider_value(slider), Some(1.0));
    tree.input(Input::Key(Key::Left));
    assert!(
        tree.slider_value(slider)
            .is_some_and(|v| (v - 0.95).abs() < 1e-6)
    );
    assert!(tree.access().dump().contains("slider \"Volume 95%\""));
    Ok(())
}
