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
//! (its lines, then the group), then the to-do list's lines, then the
//! agents' rows, which fold into "+ N" ([`room`]).

use hocus_link::Form;

use crate::agents;

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
}

/// The full form's column, top to bottom (§4, #599).
pub const FULL: [Part; 5] = [
    Part::Meeting,
    Part::InUse,
    Part::Today,
    Part::Tasks,
    Part::Agents,
];

/// The mini form's, top to bottom (§5): the mark, the sign, the to-do
/// item, the agents' item.
pub const MINI: [Part; 4] = [Part::Meeting, Part::InUse, Part::Tasks, Part::Agents];

/// The order in which the full form's groups give way when the bar is
/// short: the first gives way first.
pub const GIVES_WAY: [Part; 3] = [Part::Today, Part::Tasks, Part::Agents];

/// The column in `form`, top to bottom.
pub fn order(form: Form) -> &'static [Part] {
    match form {
        Form::Full => &FULL,
        Form::Mini => &MINI,
    }
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
    };
    if height > 0.0 { height + GAP } else { 0.0 }
}

/// `part`'s top under bar `bar`'s top in `form`: the column's start and
/// what the parts above it take.
pub fn top(bar: &str, form: Form, part: Part) -> f32 {
    top_with(form, part, |above| taken(bar, form, above))
}

/// `part`'s top in `form`, `taken` saying what each part above it takes.
fn top_with(form: Form, part: Part, mut taken: impl FnMut(Part) -> f32) -> f32 {
    start(form)
        + order(form)
            .iter()
            .take_while(|p| **p != part)
            .map(|p| taken(*p))
            .sum::<f32>()
}

/// The top of the meeting's card or of the sign of what is in use, which
/// are the same on every bar: nothing of a bar's own is above them.
pub fn shared_top(form: Form, part: Part) -> f32 {
    debug_assert!(matches!(part, Part::Meeting | Part::InUse));
    top_with(form, part, |above| taken("", form, above))
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
    let head = top_with(Form::Full, Part::Today, &mut taken);
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
/// included, as it gives way: Today none, the to-do list its header and
/// its last line ("+ N" or "Rien à faire."), the agents' group what it
/// takes (it gives way last).
fn least(bar: &str, part: Part) -> f32 {
    match part {
        Part::Today => 0.0,
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
/// to-do list's, Today's rows, then everyone's top).
pub fn moved() {
    agents::draw::fit_rows();
    crate::tasks::draw::fit_lines();
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
        move |p| match p {
            Part::Meeting => meeting,
            Part::InUse => in_use,
            Part::Today => today,
            Part::Tasks => tasks,
            Part::Agents => agents,
        }
    }

    #[test]
    fn the_full_forms_order_puts_the_agents_under_to_do() {
        assert_eq!(
            FULL,
            [
                Part::Meeting,
                Part::InUse,
                Part::Today,
                Part::Tasks,
                Part::Agents
            ]
        );
        assert_eq!(
            MINI,
            [Part::Meeting, Part::InUse, Part::Tasks, Part::Agents]
        );
    }

    #[test]
    fn each_top_is_the_start_and_what_the_parts_above_take() {
        // The card (44 + 14), no sign, Today (18 + 60 + 14), To do (18 + 66
        // + 20 + 14).
        let t = takes(58.0, 0.0, 92.0, 118.0, 200.0);
        assert_eq!(top_with(Form::Full, Part::Meeting, &t), 100.0);
        assert_eq!(top_with(Form::Full, Part::InUse, &t), 158.0);
        assert_eq!(top_with(Form::Full, Part::Today, &t), 158.0);
        assert_eq!(top_with(Form::Full, Part::Tasks, &t), 250.0);
        assert_eq!(top_with(Form::Full, Part::Agents, &t), 368.0);
        // Without the card, an empty Today and no task source: the agents
        // right under the clock.
        let t = takes(0.0, 0.0, 0.0, 0.0, 200.0);
        assert_eq!(top_with(Form::Full, Part::Agents, &t), 100.0);
        // The mini form: the mark's 16 + 14, the to-do item's 34 + 14;
        // Today is not in it.
        let t = takes(30.0, 0.0, 92.0, 48.0, 76.0);
        assert_eq!(top_with(Form::Mini, Part::Tasks, &t), 98.0);
        assert_eq!(top_with(Form::Mini, Part::Agents, &t), 146.0);
    }

    #[test]
    fn today_gives_way_first_then_to_dos_lines_then_the_agents_rows() {
        let t = takes(58.0, 0.0, 92.0, 118.0, 200.0);
        // To do without its lines: its header and "+ N", and 14 px.
        let least = |p: Part| match p {
            Part::Today => 0.0,
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
}
