//! Toasts as drawn, their time, and what clicks on them do.

use std::time::Duration;

use hocus_shell::{Lang, Toast, ToastEvent, Toasts, toasts::REPLY};
use hocus_ui::{Input, Key, Point, Role, Theme};

mod common;

const MS: fn(u64) -> Duration = Duration::from_millis;

fn toast(id: u32, summary: &str) -> Toast {
    Toast {
        id,
        app: "Mail".into(),
        summary: summary.into(),
        body: "Camille: the dock mockups are in the thread.".into(),
        actions: vec![
            ("reply".into(), "Reply".into()),
            ("open".into(), "Open".into()),
        ],
        critical: false,
        timeout: Some(Duration::from_secs(6)),
    }
}

fn button(toasts: &Toasts, name: &str) -> anyhow::Result<Point> {
    let access = toasts.access();
    let node = access
        .walk()
        .into_iter()
        .find(|n| n.role == Role::Button && n.label.as_deref() == Some(name))
        .ok_or_else(|| anyhow::anyhow!("no button {name:?} in\n{}", access.dump()))?;
    let r = node.rect;
    Ok(Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0))
}

fn click(toasts: &mut Toasts, at: Point) -> Vec<ToastEvent> {
    toasts.input(at, Input::PointerMove(at));
    toasts.input(at, Input::PointerButton { pressed: true });
    toasts.input(at, Input::PointerButton { pressed: false })
}

#[test]
fn three_toasts_stack_newest_on_top_and_reflow_when_one_goes() -> anyhow::Result<()> {
    let mut toasts = Toasts::new(Theme::default(), Lang::En, 700.0, 1.0);
    for (i, s) in ["first", "second", "third"].iter().enumerate() {
        // Without a countdown, so that only the reflow moves.
        let still = Toast {
            timeout: None,
            ..toast(i as u32 + 1, s)
        };
        toasts.show(still, MS(i as u64 * 100));
    }
    assert_eq!(toasts.shown(), vec![3, 2, 1]);
    toasts.draw(MS(1000));
    common::expect_reference(&toasts.view(), "toasts-three.png")?;

    let at = button(&toasts, "Dismiss: second")?;
    assert_eq!(click(&mut toasts, at), vec![ToastEvent::Dismissed(2)]);
    assert_eq!(toasts.shown(), vec![3, 1]);
    // The lower one springs up into the gap, then rests.
    assert!(toasts.is_animating(MS(1050)));
    toasts.draw(MS(3000));
    assert!(!toasts.is_animating(MS(3000)));
    Ok(())
}

#[test]
fn time_runs_out_unless_the_pointer_is_over_it() -> anyhow::Result<()> {
    let mut toasts = Toasts::new(Theme::default(), Lang::En, 700.0, 1.0);
    toasts.show(toast(1, "kept"), MS(0));
    toasts.show(
        Toast {
            id: 2,
            ..toast(2, "gone")
        },
        MS(0),
    );
    toasts.draw(MS(300));
    // Over the lower toast, "kept".
    let r = button(&toasts, "Dismiss: kept")?;
    let over = Point::new(r.x - 100.0, r.y + 40.0);
    toasts.input(over, Input::PointerMove(over));
    assert_eq!(toasts.update(MS(7000)), vec![ToastEvent::Expired(2)]);
    assert_eq!(toasts.shown(), vec![1]);
    // The pointer leaves: its 6 s go on from where they stopped.
    toasts.input(Point::new(-50.0, -50.0), Input::PointerLeave);
    assert_eq!(toasts.update(MS(12000)), vec![]);
    assert_eq!(toasts.update(MS(13000)), vec![ToastEvent::Expired(1)]);
    Ok(())
}

#[test]
fn an_action_is_reported_and_closes_the_toast() -> anyhow::Result<()> {
    let mut toasts = Toasts::new(Theme::default(), Lang::En, 700.0, 1.0);
    toasts.show(toast(7, "review"), MS(0));
    toasts.draw(MS(300));
    let at = button(&toasts, "Reply")?;
    assert_eq!(
        click(&mut toasts, at),
        vec![ToastEvent::Action(7, "reply".into())]
    );
    assert!(toasts.is_empty());
    Ok(())
}

#[test]
fn a_replacement_keeps_its_place() {
    let mut toasts = Toasts::new(Theme::default(), Lang::En, 700.0, 1.0);
    toasts.show(toast(1, "a"), MS(0));
    toasts.show(toast(2, "b"), MS(0));
    toasts.show(toast(1, "a, updated"), MS(10));
    assert_eq!(toasts.shown(), vec![2, 1]);
    assert_eq!(toasts.history().count(), 2, "a replacement is not new");
}

#[test]
fn a_reply_is_typed_into_the_toast_which_waits_for_it() -> anyhow::Result<()> {
    let mut toasts = Toasts::new(Theme::default(), Lang::En, 700.0, 1.0);
    toasts.show(
        Toast {
            actions: vec![(REPLY.into(), "Reply".into())],
            ..toast(3, "Sam")
        },
        MS(0),
    );
    toasts.draw(MS(300));
    assert!(!toasts.replying());
    let at = button(&toasts, "Reply")?;
    assert_eq!(click(&mut toasts, at), vec![]);
    assert!(toasts.replying(), "the field has the keys");
    toasts.draw(MS(400));
    // The pointer leaves; the toast still waits.
    toasts.input(Point::new(-50.0, -50.0), Input::PointerLeave);
    assert_eq!(toasts.update(MS(60_000)), vec![]);
    // Nothing typed: nothing sent.
    assert_eq!(toasts.key(Key::Enter), vec![]);
    toasts.text("On my");
    toasts.text(" way ");
    assert_eq!(
        toasts.key(Key::Enter),
        vec![ToastEvent::Replied(3, "On my way".into())]
    );
    assert!(toasts.is_empty());
    assert!(!toasts.replying());
    Ok(())
}

#[test]
fn escape_dismisses_a_reply_and_send_sends_it() -> anyhow::Result<()> {
    let mut toasts = Toasts::new(Theme::default(), Lang::En, 700.0, 1.0);
    let with_reply = |id| Toast {
        actions: vec![(REPLY.into(), "Reply".into())],
        ..toast(id, "Sam")
    };
    toasts.show(with_reply(4), MS(0));
    toasts.draw(MS(300));
    let at = button(&toasts, "Reply")?;
    click(&mut toasts, at);
    assert_eq!(toasts.key(Key::Escape), vec![ToastEvent::Dismissed(4)]);
    assert!(toasts.is_empty());

    toasts.show(with_reply(5), MS(400));
    toasts.draw(MS(700));
    let at = button(&toasts, "Reply")?;
    click(&mut toasts, at);
    toasts.text("Yes");
    toasts.draw(MS(800));
    let at = button(&toasts, "Send")?;
    assert_eq!(
        click(&mut toasts, at),
        vec![ToastEvent::Replied(5, "Yes".into())]
    );
    Ok(())
}
