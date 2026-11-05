//! The bar's column under the clock (bar.md §4, §5): the day's surfaces
//! and the agents', in their order, and the room they share.
//!
//! Each of them is a surface of its own; none places itself from the one
//! above it. Their order is [`order`], and every surface's top is the
//! column's start plus what the surfaces above it in that order take
//! ([`top`]), each with the 14 px after it, nothing for one that does not
//! show. When one of them changes height, comes or goes, [`moved`] puts
//! every one of them in its place again.
//!
//! On a short bar they give way in [`GIVES_WAY`]'s order: Today first
//! (its lines, then the group), then the windows over the desktops
//! (#604), then the to-do list's lines, then the agents' rows, which fold
//! into "+ N" ([`room`]).
//!
//! Which groups show and the order of Today, the to-do list and the
//! agents are the configuration's (`[panel]`, 6.4, [`set_groups`]): a
//! hidden group takes no room, as an empty one; the mini form follows the
//! same order, Today not in it.
//!
//! The windows' group is not in the column's order: it sits on the
//! desktops, from the bottom, in the room the column leaves free (the
//! spacer's), so that nothing in the column moves for it. Today, which
//! fills the spacer, ends over it; the to-do list and the agents' group
//! never give way to it.

use std::sync::{Mutex, PoisonError};

use hocus_link::{BarGroup, BarGroups, Form};

use crate::agents;

/// What the configuration says the column shows, as last told.
static GROUPS: Mutex<Option<BarGroups>> = Mutex::new(None);

/// The column's top under the bar's in the full form: 14 px of padding,
/// the clock's 72 px and the 14 px between groups.
pub const TOP: f32 = 100.0;
/// In the mini form: 12 px of padding, the clock's two lines of 19 px at
/// 1.1 (41.8 px), and the 14 px between the mini form's items.
pub const MINI_TOP: f32 = 68.0;
/// Between the column's groups (and the mini form's items), and over the
/// desktops' group under them.
pub const GAP: f32 = 14.0;

/// A surface of the column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// The next meeting's card, or its mark (Day.2).
    Meeting,
    /// The sign of what uses the microphone, the camera or the screen
    /// (5.12).
    InUse,
    /// Today's events (Day.3): the full form's alone.
    Today,
    /// The to-do list (Day.4), or its mini item.
    Tasks,
    /// The agents' group (4.0 to 4.7), or the mini form's agents' item.
    Agents,
    /// The windows over the desktops (#604): not in the column's order,
    /// placed from the bottom in the room it leaves.
    Windows,
}

impl Part {
    fn of(group: BarGroup) -> Self {
        match group {
            BarGroup::Today => Self::Today,
            BarGroup::Tasks => Self::Tasks,
            BarGroup::Agents => Self::Agents,
        }
    }
}

/// The order in which the full form's groups give way when the bar is
/// short: the first gives way first, whatever their order.
pub const GIVES_WAY: [Part; 4] = [Part::Today, Part::Windows, Part::Tasks, Part::Agents];

/// What the column shows, as the configuration last said.
pub fn groups() -> BarGroups {
    GROUPS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .unwrap_or_default()
}

/// The configuration says what the column shows (`[panel]`, 6.4): the
/// groups that come or go and those that move take their places, on
/// every bar.
pub fn set_groups(groups: BarGroups) {
    let before = GROUPS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .replace(groups)
        .unwrap_or_default();
    if before == groups {
        return;
    }
    tracing::info!(?groups, "the bar's groups");
    // Each part reads what shows as it lays out; the meeting's card and
    // Today's lines are made again, the to-do list and the agents laid
    // out again as they draw, then everyone's place.
    crate::meeting::refresh();
    crate::today::refresh();
    crate::tasks::shown_changed();
    agents::shown_changed();
    moved();
}

/// The column in `form`, top to bottom, the movable groups in `order`:
/// the meeting's card (or mark), the sign of what is in use, then Today
/// (in the full form only), the to-do list and the agents.
pub fn order_with(form: Form, order: [BarGroup; 3]) -> Vec<Part> {
    let mut parts = vec![Part::Meeting, Part::InUse];
    parts.extend(
        order
            .into_iter()
            .filter(|g| form == Form::Full || *g != BarGroup::Today)
            .map(Part::of),
    );
    parts
}

/// The column in `form`, top to bottom, in the configuration's order.
pub fn order(form: Form) -> Vec<Part> {
    order_with(form, groups().order)
}

/// Where the column starts under the bar's top in `form`.
pub fn start(form: Form) -> f32 {
    match form {
        Form::Full => TOP,
        Form::Mini => MINI_TOP,
    }
}

/// What `part` takes of bar `bar`'s column in `form`: its height and the
/// 14 px after it, as it last laid out; none when it does not show.
///
/// It reads the parts' own state: never called under one of their locks.
pub fn taken(bar: &str, form: Form, part: Part) -> f32 {
    let height = match part {
        // Their room counts the 14 px after them already.
        Part::Meeting => return crate::meeting::room(form),
        Part::InUse => return crate::in_use::room(form),
        Part::Today => match form {
            Form::Full => crate::today::height(&crate::today::shown(bar).0),
            Form::Mini => 0.0,
        },
        Part::Tasks => crate::tasks::draw::height(bar, form),
        Part::Agents => agents::draw::height(form),
        Part::Windows => crate::windows::height(bar, form),
    };
    if height > 0.0 { height + GAP } else { 0.0 }
}

/// `part`'s top under bar `bar`'s top in `form`: the column's start and
/// what the parts above it take.
pub fn top(bar: &str, form: Form, part: Part) -> f32 {
    top_with(form, &order(form), part, |above| taken(bar, form, above))
}

/// `part`'s top in `form`, the column being `order`, `taken` saying what
/// each part above it takes.
fn top_with(form: Form, order: &[Part], part: Part, mut taken: impl FnMut(Part) -> f32) -> f32 {
    start(form)
        + order
            .iter()
            .take_while(|p| **p != part)
            .map(|p| taken(*p))
            .sum::<f32>()
}

/// Where bar `bar`'s column ends under its top in `form`: its start and
/// what every part of it takes, the 14 px after the last included.
pub fn end(bar: &str, form: Form) -> f32 {
    start(form)
        + order(form)
            .iter()
            .map(|p| taken(bar, form, *p))
            .sum::<f32>()
}

/// The top of the meeting's card or of the sign of what is in use, which
/// are the same on every bar: nothing of a bar's own is above them.
pub fn shared_top(form: Form, part: Part) -> f32 {
    debug_assert!(matches!(part, Part::Meeting | Part::InUse));
    top_with(form, &order(form), part, |above| taken("", form, above))
}

/// Where bar `bar`'s column ends in the full form, under the bar's top:
/// its desktops' group's top (the 14 px over it are the last group's);
/// not known before its desktops are placed.
pub fn bottom(bar: &str) -> Option<f32> {
    let under = crate::desktops::draw::top_above_bottom(bar)?;
    Some(crate::popover::with(|c| c.bar_height(bar)) - under)
}

/// The room bar `bar`'s full form has for `part`'s group (one of
/// [`GIVES_WAY`]), the 14 px after it left out: from its top down to the
/// column's bottom, less what the groups that give way after it take as
/// they are, and the least those that give way before it can take. Today
/// has what the spacer would have; the to-do list's room ignores Today;
/// the agents' ignores Today and counts the to-do list without its lines.
/// Until the bar's bottom is known, Today has none (it comes once it is)
/// and the others all they ask (none of their lines goes and comes back).
pub fn room(bar: &str, part: Part) -> f32 {
    let Some(bottom) = bottom(bar) else {
        return if part == Part::Today {
            0.0
        } else {
            f32::INFINITY
        };
    };
    room_with(
        part,
        bottom,
        |q| taken(bar, Form::Full, q),
        |q| least(bar, q),
    )
}

/// [`room`], with the column's bottom, what each part takes and the least
/// each takes.
fn room_with(
    part: Part,
    bottom: f32,
    mut taken: impl FnMut(Part) -> f32,
    mut least: impl FnMut(Part) -> f32,
) -> f32 {
    // Under the meeting's card and the sign, whatever the order: the
    // groups share the rest.
    let head = start(Form::Full) + taken(Part::Meeting) + taken(Part::InUse);
    let mut room = bottom - head - GAP;
    let mut before = true;
    for q in GIVES_WAY {
        if q == part {
            before = false;
            continue;
        }
        room -= if before { least(q) } else { taken(q) };
    }
    room
}

/// The least `part` takes of bar `bar`'s full form, the 14 px after it
/// included, as it gives way: Today and the windows none, the to-do list
/// its header and its last line ("+ N" or "Rien à faire."), the agents'
/// group what it takes (it gives way last).
fn least(bar: &str, part: Part) -> f32 {
    match part {
        Part::Today | Part::Windows => 0.0,
        Part::Tasks => {
            let least = crate::tasks::draw::least(bar);
            if least > 0.0 { least + GAP } else { 0.0 }
        }
        _ => taken(bar, Form::Full, part),
    }
}

/// One of the column's surfaces came, went or changed height, a bar
/// changed height or form: every one of them in its place again, in an
/// order where each reads what it depends on (the agents' rows' room, the
/// to-do list's, the windows', Today's rows, then everyone's top).
pub fn moved() {
    agents::draw::fit_rows();
    crate::tasks::draw::fit_lines();
    crate::windows::fit_all();
    crate::today::moved();
    crate::tasks::draw::moved();
    agents::draw::moved();
    agents::peek::moved();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What each part takes in a test's column.
    fn takes(
        meeting: f32,
        in_use: f32,
        today: f32,
        tasks: f32,
        agents: f32,
    ) -> impl Fn(Part) -> f32 {
        with_windows(meeting, in_use, today, tasks, agents, 0.0)
    }

    /// The same, the windows over the desktops taking `windows`.
    fn with_windows(
        meeting: f32,
        in_use: f32,
        today: f32,
        tasks: f32,
        agents: f32,
        windows: f32,
    ) -> impl Fn(Part) -> f32 {
        move |p| match p {
            Part::Meeting => meeting,
            Part::InUse => in_use,
            Part::Today => today,
            Part::Tasks => tasks,
            Part::Agents => agents,
            Part::Windows => windows,
        }
    }

    const FULL: [Part; 5] = [
        Part::Meeting,
        Part::InUse,
        Part::Today,
        Part::Tasks,
        Part::Agents,
    ];
    const MINI: [Part; 4] = [Part::Meeting, Part::InUse, Part::Tasks, Part::Agents];

    #[test]
    fn the_full_forms_order_puts_the_agents_under_to_do() {
        assert_eq!(order_with(Form::Full, BarGroup::DEFAULT), FULL);
        assert_eq!(order_with(Form::Mini, BarGroup::DEFAULT), MINI);
    }

    #[test]
    fn the_configurations_order_moves_today_to_do_and_the_agents() {
        let order = [BarGroup::Agents, BarGroup::Today, BarGroup::Tasks];
        assert_eq!(
            order_with(Form::Full, order),
            [
                Part::Meeting,
                Part::InUse,
                Part::Agents,
                Part::Today,
                Part::Tasks
            ]
        );
        // The mini form the same, without Today.
        assert_eq!(
            order_with(Form::Mini, order),
            [Part::Meeting, Part::InUse, Part::Agents, Part::Tasks]
        );
        // The card (44 + 14), the agents (200 + 14), Today (92), To do.
        let t = takes(58.0, 0.0, 92.0, 118.0, 200.0);
        let full = order_with(Form::Full, order);
        assert_eq!(top_with(Form::Full, &full, Part::Agents, &t), 158.0);
        assert_eq!(top_with(Form::Full, &full, Part::Today, &t), 358.0);
        assert_eq!(top_with(Form::Full, &full, Part::Tasks, &t), 450.0);
        // The room each has does not depend on the order.
        let least = |p: Part| match p {
            Part::Today | Part::Windows => 0.0,
            Part::Tasks => 52.0,
            _ => t(p),
        };
        assert_eq!(
            room_with(Part::Today, 700.0, &t, least),
            700.0 - 158.0 - 14.0 - 118.0 - 200.0
        );
    }

    #[test]
    fn each_top_is_the_start_and_what_the_parts_above_take() {
        // The card (44 + 14), no sign, Today (18 + 60 + 14), To do (18 + 66
        // + 20 + 14).
        let t = takes(58.0, 0.0, 92.0, 118.0, 200.0);
        assert_eq!(top_with(Form::Full, &FULL, Part::Meeting, &t), 100.0);
        assert_eq!(top_with(Form::Full, &FULL, Part::InUse, &t), 158.0);
        assert_eq!(top_with(Form::Full, &FULL, Part::Today, &t), 158.0);
        assert_eq!(top_with(Form::Full, &FULL, Part::Tasks, &t), 250.0);
        assert_eq!(top_with(Form::Full, &FULL, Part::Agents, &t), 368.0);
        // Without the card, an empty Today and no task source: the agents
        // right under the clock.
        let t = takes(0.0, 0.0, 0.0, 0.0, 200.0);
        assert_eq!(top_with(Form::Full, &FULL, Part::Agents, &t), 100.0);
        // The mini form: the mark's 16 + 14, the to-do item's 34 + 14;
        // Today is not in it.
        let t = takes(30.0, 0.0, 92.0, 48.0, 76.0);
        assert_eq!(top_with(Form::Mini, &MINI, Part::Tasks, &t), 98.0);
        assert_eq!(top_with(Form::Mini, &MINI, Part::Agents, &t), 146.0);
    }

    #[test]
    fn today_gives_way_first_then_to_dos_lines_then_the_agents_rows() {
        let t = takes(58.0, 0.0, 92.0, 118.0, 200.0);
        // To do without its lines: its header and "+ N", and 14 px.
        let least = |p: Part| match p {
            Part::Today | Part::Windows => 0.0,
            Part::Tasks => 18.0 + 20.0 + 14.0,
            _ => t(p),
        };
        let bottom = 700.0;
        // Today: what is left under the agents and To do as they are.
        assert_eq!(
            room_with(Part::Today, bottom, &t, least),
            bottom - 158.0 - 14.0 - 118.0 - 200.0
        );
        // To do: Today counts for nothing, the agents as they are.
        assert_eq!(
            room_with(Part::Tasks, bottom, &t, least),
            bottom - 158.0 - 14.0 - 200.0
        );
        // The agents: Today nothing, To do at its least.
        assert_eq!(
            room_with(Part::Agents, bottom, &t, least),
            bottom - 158.0 - 14.0 - 52.0
        );
    }

    #[test]
    fn the_windows_take_what_to_do_and_the_agents_leave_and_today_ends_over_them() {
        // Three windows over the desktops: 90 px and 14.
        let t = with_windows(58.0, 0.0, 92.0, 118.0, 200.0, 104.0);
        let least = |p: Part| match p {
            Part::Today | Part::Windows => 0.0,
            Part::Tasks => 18.0 + 20.0 + 14.0,
            _ => t(p),
        };
        let bottom = 700.0;
        // The windows: what To do and the agents leave, Today counting for
        // nothing; they never make those give way.
        assert_eq!(
            room_with(Part::Windows, bottom, &t, least),
            bottom - 158.0 - 14.0 - 118.0 - 200.0
        );
        assert_eq!(
            room_with(Part::Tasks, bottom, &t, least),
            bottom - 158.0 - 14.0 - 200.0
        );
        assert_eq!(
            room_with(Part::Agents, bottom, &t, least),
            bottom - 158.0 - 14.0 - 52.0
        );
        // Today ends 14 px over the windows.
        assert_eq!(
            room_with(Part::Today, bottom, &t, least),
            bottom - 158.0 - 14.0 - 104.0 - 118.0 - 200.0
        );
        // They are not in the column's order: no part's top counts them.
        assert!(!FULL.contains(&Part::Windows) && !MINI.contains(&Part::Windows));
        assert_eq!(top_with(Form::Full, &FULL, Part::Agents, &t), 368.0);
    }
}
