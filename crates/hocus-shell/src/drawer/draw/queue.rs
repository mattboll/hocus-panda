//! Her triage queue laid out in place of the thread (#754, #758), as her
//! header's « 21 mails à trier › » unfolds it under its sub-view's bar
//! (« ‹ Retour à la discussion », « Le tri de tes mails »): a sentence
//! saying what the triage is; what she does, how far along, how long is
//! left, the mail read now; or why it waits, said once; then one card per
//! sender: ENVOYÉS PAR DES MACHINES, each with its action inside naming it
//! (« Ne pas trier les mails de GitHub »), DES PERSONNES, which she sorts
//! first and never offers to leave unsorted, the rest; PAS TRIÉS, À TA
//! DEMANDE with « Trier à nouveau les mails de Scaleway ».
//!
//! Everything here is what she sent ([`View`]); the drawer keeps only the
//! scroll. No model's name, no mail header's.

use hocus_secretary_link::queue::{Group, Sender, View};
use iced::{Point, Rectangle, Size, widget::text::Alignment};

use super::{
    super::{Open, Target, Words},
    BUTTON_X, Corner, DIM, FAINT, LABEL, Laid, Line, MINT, ORANGE, PAD, Shape, TEXT, THREAD_BOTTOM,
    THREAD_TOP, alpha, hex, lines_of, moved, push_lines, width_of,
};

const SECTION: f32 = 10.5;
const STATE: f32 = 15.0;
const NAME: f32 = 13.0;
const NOTE: f32 = 11.5;
const BODY: f32 = 12.5;
const BODY_LINE: f32 = BODY * 1.45;
const PILL: f32 = 11.5;
const PILL_H: f32 = PILL * 1.4 + 9.0;
const AVATAR: f32 = 28.0;

/// A section's title at `y`, its count on the right: its bottom.
fn section(shapes: &mut Vec<Shape>, title: &str, count: &str, right: f32, y: f32) -> f32 {
    let mid = y + SECTION * 0.7;
    shapes.push(Shape::Text(Line {
        text: title.to_owned(),
        at: Point::new(PAD, mid),
        size: SECTION,
        color: LABEL,
        bold: true,
        align: Alignment::Left,
    }));
    shapes.push(Shape::Text(Line {
        text: count.to_owned(),
        at: Point::new(right, mid),
        size: NOTE,
        color: FAINT,
        bold: false,
        align: Alignment::Right,
    }));
    y + SECTION * 1.4 + 6.0
}

/// `text` in lines from `y`, in `colour`: its bottom.
#[allow(clippy::too_many_arguments)]
fn words_at(
    shapes: &mut Vec<Shape>,
    text: &str,
    size: f32,
    colour: iced::Color,
    x: f32,
    room: f32,
    y: f32,
) -> f32 {
    let lines = lines_of(text, size, false, room);
    push_lines(shapes, &lines, x, y, size, size * 1.45, colour, false);
    y + lines.len() as f32 * size * 1.45
}

/// How many mails a count is.
fn mails(n: u64) -> String {
    match n {
        1 => "1 mail".to_owned(),
        n => format!("{n} mails"),
    }
}

/// How many mails, or items, a group holds.
fn count_words(group: &Group) -> String {
    if group.sender == Sender::Other {
        group.count.to_string()
    } else {
        mails(group.count)
    }
}

/// A sender's colour, the same each time.
fn colour_of(name: &str) -> u32 {
    const PALETTE: [u32; 6] = [0x8FB3FF, 0x7EE0A0, 0xF7A77A, 0xB39DFF, 0xC9D0DC, 0xF5C46E];
    let sum: u32 = name.chars().map(|c| c as u32).sum();
    PALETTE[(sum % PALETTE.len() as u32) as usize]
}

/// A sender's card at `y`: its letter, its name, its address and what it
/// is, its count; its action inside, under a rule. Its bottom.
#[allow(clippy::too_many_arguments)]
fn sender(
    shapes: &mut Vec<Shape>,
    targets: &mut Vec<(Target, Rectangle)>,
    open: &Open,
    group: &Group,
    action: Option<(Target, String)>,
    aside: bool,
    right: f32,
    y: f32,
) -> f32 {
    let x = PAD;
    let w = right - PAD;
    let mut parts = Vec::new();
    let top = y + 9.0;
    let name = if group.name.is_empty() {
        &group.label
    } else {
        &group.name
    };
    let letter = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    let colour = colour_of(name);
    parts.push(Shape::Box {
        rect: Rectangle::new(Point::new(x + 10.0, top + 1.0), Size::new(AVATAR, AVATAR)),
        radius: 8.0,
        fill: if aside {
            alpha(colour, 0.35)
        } else {
            hex(colour)
        },
        rim: None,
        dashed: false,
        sharp: Corner::None,
    });
    parts.push(Shape::Text(Line {
        text: letter,
        at: Point::new(x + 10.0 + AVATAR / 2.0, top + 1.0 + AVATAR / 2.0),
        size: 13.0,
        color: hex(0x0B1020),
        bold: true,
        align: Alignment::Center,
    }));
    let count = count_words(group);
    let cw = width_of(&count, NOTE, true) + 14.0;
    parts.push(Shape::Box {
        rect: Rectangle::new(
            Point::new(x + w - 10.0 - cw, top + 5.0),
            Size::new(cw, 20.0),
        ),
        radius: 6.0,
        fill: alpha(0xFFFFFF, 0.07),
        rim: None,
        dashed: false,
        sharp: Corner::None,
    });
    parts.push(Shape::Text(Line {
        text: count,
        at: Point::new(x + w - 10.0 - cw / 2.0, top + 15.0),
        size: NOTE,
        color: hex(0xD5DBE5),
        bold: true,
        align: Alignment::Center,
    }));
    let tx = x + 10.0 + AVATAR + 10.0;
    let room = x + w - 10.0 - cw - 8.0 - tx;
    let shown = crate::popover::kit::elide(name, room, |s| width_of(s, NAME, true));
    parts.push(Shape::Text(Line {
        text: shown,
        at: Point::new(tx, top + 8.0),
        size: NAME,
        color: if aside { DIM } else { TEXT },
        bold: true,
        align: Alignment::Left,
    }));
    if !group.note.is_empty() {
        let note = crate::popover::kit::elide(&group.note, room, |s| width_of(s, NOTE, false));
        parts.push(Shape::Text(Line {
            text: note,
            at: Point::new(tx, top + 24.0),
            size: NOTE,
            color: FAINT,
            bold: false,
            align: Alignment::Left,
        }));
    }
    let mut bottom = top + AVATAR + 4.0;
    if let Some((target, label)) = action {
        bottom += 4.0;
        parts.push(Shape::Box {
            rect: Rectangle::new(Point::new(x + 10.0, bottom), Size::new(w - 20.0, 1.0)),
            radius: 0.0,
            fill: alpha(0xFFFFFF, 0.07),
            rim: None,
            dashed: false,
            sharp: Corner::None,
        });
        bottom += 8.0;
        let bw = width_of(&label, PILL, true) + 2.0 * BUTTON_X;
        let rect = Rectangle::new(Point::new(x + 10.0, bottom), Size::new(bw, PILL_H));
        let hover = open.hover == Some(target);
        parts.push(Shape::Box {
            rect,
            radius: 8.0,
            fill: alpha(0xFFFFFF, if hover { 0.12 } else { 0.06 }),
            rim: Some(alpha(0xFFFFFF, if hover { 0.28 } else { 0.16 })),
            dashed: false,
            sharp: Corner::None,
        });
        parts.push(Shape::Text(Line {
            text: label,
            at: rect.center(),
            size: PILL,
            color: if hover { MINT } else { hex(0xD5DBE5) },
            bold: true,
            align: Alignment::Center,
        }));
        targets.push((target, rect));
        bottom += PILL_H;
    }
    bottom += 9.0;
    shapes.push(Shape::Box {
        rect: Rectangle::new(Point::new(x, y), Size::new(w, bottom - y)),
        radius: 10.0,
        fill: alpha(0xFFFFFF, if aside { 0.015 } else { 0.035 }),
        rim: Some(alpha(0xFFFFFF, if aside { 0.14 } else { 0.08 })),
        dashed: aside,
        sharp: Corner::None,
    });
    shapes.extend(parts);
    bottom
}

/// A sender as the user reads it: its name, else its address.
fn name_of(from: &str) -> String {
    match from.rsplit_once('<') {
        Some((name, _)) if !name.trim().trim_matches('"').trim().is_empty() => {
            name.trim().trim_matches('"').trim().to_owned()
        }
        _ => from.trim().to_owned(),
    }
}

/// The queue laid out from `top`, scrolled by `open.scroll` (from its
/// top: `f32::MAX` shows its top).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) fn lay_out(
    laid: &mut Laid,
    open: &Open,
    view: Option<&View>,
    words: Words,
    top: f32,
    right: f32,
    said: &mut Vec<String>,
) {
    let mut shapes: Vec<Shape> = Vec::new();
    let mut targets: Vec<(Target, Rectangle)> = Vec::new();
    let mut y = THREAD_TOP + 2.0;
    let room = right - PAD;
    said.push("queue".to_owned());
    if let Some(view) = view {
        let paused = view.pause.is_some();
        // What the triage is, in a sentence: shortened while it waits.
        let lead = words.pick(
            "Avant de lire un mail pour toi, je le trie ici, sur ton ordinateur : je masque les mots de passe et les codes, et je regarde s’il attend une réponse de ta part. Tant qu’un mail n’est pas trié, je ne connais que son expéditeur, son objet et sa date.",
            "Before I read a mail for you, I sort it here, on your computer: I mask passwords and codes, and look whether it waits for your answer. Until a mail is sorted, I know only its sender, subject and date.",
        );
        let lead_lines = if paused {
            vec![crate::popover::kit::elide(lead, room - 20.0, |s| {
                width_of(s, BODY, false)
            })]
        } else {
            lines_of(lead, BODY, false, room - 20.0)
        };
        let h = lead_lines.len() as f32 * BODY_LINE + 16.0;
        shapes.push(Shape::Box {
            rect: Rectangle::new(Point::new(PAD, y), Size::new(room, h)),
            radius: 10.0,
            fill: alpha(0x8FB3FF, 0.06),
            rim: Some(alpha(0x8FB3FF, 0.3)),
            dashed: false,
            sharp: Corner::None,
        });
        push_lines(
            &mut shapes,
            &lead_lines,
            PAD + 10.0,
            y + 8.0,
            BODY,
            BODY_LINE,
            hex(0xD5DBE5),
            false,
        );
        y += h + 8.0;
        // What she does now, or why it waits: one reason, said once.
        let box_top = y;
        let mut at = y + 10.0;
        let mut parts = Vec::new();
        let sw = width_of(&view.state, STATE, true);
        parts.push(Shape::Text(Line {
            text: view.state.clone(),
            at: Point::new(PAD + 12.0, at + STATE * 0.7),
            size: STATE,
            color: TEXT,
            bold: true,
            align: Alignment::Left,
        }));
        parts.push(Shape::Text(Line {
            text: view.counts.clone(),
            at: Point::new(PAD + 12.0 + sw + 8.0, at + STATE * 0.7),
            size: 12.0,
            color: DIM,
            bold: false,
            align: Alignment::Left,
        }));
        at += STATE * 1.4 + 4.0;
        said.push(format!("state:{} | {}", view.state, view.counts));
        let inner = room - 24.0;
        if let Some(pause) = &view.pause {
            at = words_at(
                &mut parts,
                pause,
                12.0,
                hex(0xF5D9A6),
                PAD + 12.0,
                inner,
                at,
            );
            said.push(format!("pause:{pause}"));
        } else {
            let total = view.waiting + view.done;
            if total > 0 {
                let bar = Rectangle::new(Point::new(PAD + 12.0, at + 2.0), Size::new(inner, 4.0));
                parts.push(Shape::Box {
                    rect: bar,
                    radius: 2.0,
                    fill: alpha(0xFFFFFF, 0.08),
                    rim: None,
                    dashed: false,
                    sharp: Corner::None,
                });
                let done = view.done as f32 / total as f32;
                if done > 0.0 {
                    parts.push(Shape::Box {
                        rect: Rectangle::new(
                            bar.position(),
                            Size::new((bar.width * done).max(4.0), 4.0),
                        ),
                        radius: 2.0,
                        fill: hex(0x6CC287),
                        rim: None,
                        dashed: false,
                        sharp: Corner::None,
                    });
                }
                at += 12.0;
            }
            let pace = if view.pace.is_empty() && view.waiting > 0 {
                words
                    .pick(
                        "Le rythme se mesure au premier mail.",
                        "The pace is measured on the first mail.",
                    )
                    .to_owned()
            } else {
                view.pace.clone()
            };
            if !pace.is_empty() {
                at = words_at(&mut parts, &pace, 12.0, DIM, PAD + 12.0, inner, at);
                said.push(format!("pace:{pace}"));
            }
            if let Some(now) = &view.now {
                let name = name_of(&now.sender);
                let line = if words.fr {
                    format!("En ce moment : {name} · « {} »", now.subject)
                } else {
                    format!("Now: {name} · “{}”", now.subject)
                };
                at = words_at(&mut parts, &line, 12.0, DIM, PAD + 12.0, inner, at);
                said.push(format!(
                    "now:{} | {} | {}",
                    now.sender, now.subject, now.source
                ));
            }
        }
        if let Some(slow) = &view.slow {
            at = words_at(&mut parts, slow, 12.0, ORANGE, PAD + 12.0, inner, at);
            said.push(format!("slow:{slow}"));
        }
        at += 8.0;
        shapes.push(Shape::Box {
            rect: Rectangle::new(Point::new(PAD, box_top), Size::new(room, at - box_top)),
            radius: 10.0,
            fill: if paused {
                alpha(0xF5B44A, 0.06)
            } else {
                alpha(0xFFFFFF, 0.03)
            },
            rim: Some(if paused {
                alpha(0xF5B44A, 0.45)
            } else {
                alpha(0xFFFFFF, 0.08)
            }),
            dashed: false,
            sharp: Corner::None,
        });
        shapes.extend(parts);
        y = at + 14.0;
        // One card per sender, grouped by what sends them.
        let of = |kinds: &[Sender]| -> Vec<(usize, &Group)> {
            view.groups
                .iter()
                .enumerate()
                .filter(|(_, g)| kinds.contains(&g.sender))
                .collect()
        };
        for (title, hint, list, counted) in [
            (
                words.pick("ENVOYÉS PAR DES MACHINES", "SENT BY MACHINES"),
                words.pick(
                    "Des notifications et des lettres d’information. Inutile de les trier : sans tri, je les retrouve quand même si tu me les demandes.",
                    "Notifications and newsletters. No need to sort them: unsorted, I still find them if you ask me.",
                ),
                of(&[Sender::Machine, Sender::Letter]),
                true,
            ),
            (
                words.pick("DES PERSONNES", "PEOPLE"),
                words.pick(
                    "Je les trie en premier : ils attendent peut-être une réponse.",
                    "I sort them first: they may be waiting for an answer.",
                ),
                of(&[Sender::Person]),
                true,
            ),
            (
                words.pick("NOTIFICATIONS ET AGENDA", "NOTIFICATIONS AND CALENDAR"),
                "",
                of(&[Sender::Other]),
                false,
            ),
        ] {
            if list.is_empty() {
                continue;
            }
            let total: u64 = list.iter().map(|(_, g)| g.count).sum();
            let count = if counted {
                mails(total)
            } else {
                total.to_string()
            };
            y = section(&mut shapes, title, &count, right, y);
            if !hint.is_empty() {
                y = words_at(&mut shapes, hint, NOTE, LABEL, PAD, room, y) + 6.0;
            }
            for (n, group) in list {
                // The action only for a machine's or a newsletter's: a
                // person's mails left unsorted would hide a wait (#758).
                let action = group.can_aside.then(|| {
                    let label = match (group.sender, words.fr) {
                        (Sender::Letter, true) => "Ne pas trier cette lettre".to_owned(),
                        (Sender::Letter, false) => "Don’t sort this newsletter".to_owned(),
                        (_, true) => format!("Ne pas trier les mails de {}", group.name),
                        (_, false) => format!("Don’t sort {}’s mails", group.name),
                    };
                    (Target::Aside(n as u32), label)
                });
                let aside = action.is_some();
                y = sender(
                    &mut shapes,
                    &mut targets,
                    open,
                    group,
                    action,
                    false,
                    right,
                    y,
                ) + 6.0;
                said.push(format!(
                    "group:{} · {} · {:?}{}",
                    group.name,
                    group.count,
                    group.sender,
                    if aside { " · aside" } else { "" }
                ));
            }
            y += 8.0;
        }
        if view.groups.is_empty() {
            y = words_at(
                &mut shapes,
                words.pick("Rien n’attend.", "Nothing waits."),
                NAME,
                DIM,
                PAD,
                room,
                y,
            ) + 10.0;
        }
        if !view.more.is_empty() {
            y = words_at(&mut shapes, &view.more, NOTE, FAINT, PAD, room, y) + 10.0;
        }
        // Left unsorted at the user's asking: each brought back by name.
        if !view.aside.is_empty() {
            let count = mails(view.aside.iter().map(|g| g.count).sum());
            y = section(
                &mut shapes,
                words.pick("PAS TRIÉS, À TA DEMANDE", "UNSORTED, AS YOU ASKED"),
                &count,
                right,
                y,
            );
            for (n, group) in view.aside.iter().enumerate() {
                let label = if words.fr {
                    format!("Trier à nouveau les mails de {}", group.name)
                } else {
                    format!("Sort {}’s mails again", group.name)
                };
                y = sender(
                    &mut shapes,
                    &mut targets,
                    open,
                    group,
                    Some((Target::Unaside(n as u32), label)),
                    true,
                    right,
                    y,
                ) + 6.0;
                said.push(format!("aside:{} · {}", group.name, group.count));
            }
        }
    } else {
        y = words_at(
            &mut shapes,
            words.pick("Je regarde où en est le tri…", "Looking at the sorting…"),
            NAME,
            DIM,
            PAD,
            room,
            y,
        );
    }
    y += THREAD_BOTTOM;
    laid.content = y;
    let shown = laid.thread.height;
    let most = (y - shown).max(0.0);
    let scroll = open.scroll.clamp(0.0, most);
    let shift = top - (most - scroll);
    for shape in shapes {
        laid.shapes.push(moved(shape, shift));
    }
    for (target, rect) in targets {
        let rect = Rectangle::new(Point::new(rect.x, rect.y + shift), rect.size());
        if rect.y + rect.height > top && rect.y < top + shown {
            laid.targets.push((target, rect));
        }
    }
}
