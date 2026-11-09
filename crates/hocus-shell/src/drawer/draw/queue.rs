//! Her triage queue laid out in place of the thread (#754), as her status
//! line's click unfolds it: "‹ Discussion"; LE TRI SUR CET ORDINATEUR and
//! the model; what she does and the counts; the item read now (its
//! sender, subject, source and date, no more); the pace and the time
//! left; why it waits, and what slows it; EN ATTENTE by sender, the most
//! first, each with its count and, for a mail sender, « Ne pas trier,
//! chercher si besoin »; MIS DE CÔTÉ with « Trier quand même ».
//!
//! Everything here is what she sent ([`View`]); the drawer keeps only the
//! scroll.

use hocus_secretary_link::queue::View;
use iced::{Point, Rectangle, Size, widget::text::Alignment};

use super::{
    super::{Open, Target, Words},
    AMBER, BUTTON_X, Corner, DIM, FAINT, GAP, LABEL, Laid, Line, MINT, ORANGE, PAD, SMALL,
    SMALL_LINE, Shape, TEXT, THREAD_BOTTOM, THREAD_TOP, alpha, hex, lines_of, moved, width_of,
};

const SECTION: f32 = 10.5;
const STATE: f32 = 15.0;
const ROW: f32 = 13.0;
const ROW_LINE: f32 = ROW * 1.4;
const NOTE: f32 = 11.5;
const NOTE_LINE: f32 = NOTE * 1.4;
const PILL: f32 = 11.5;
const PILL_H: f32 = PILL * 1.4 + 8.0;

/// A section's title at `y`, with its count: its bottom.
fn section(shapes: &mut Vec<Shape>, title: &str, right: f32, y: f32) -> f32 {
    shapes.push(Shape::Text(Line {
        text: title.to_owned(),
        at: Point::new(PAD, y + SECTION * 0.7),
        size: SECTION,
        color: LABEL,
        bold: true,
        align: Alignment::Left,
    }));
    shapes.push(Shape::Rule(Rectangle::new(
        Point::new(
            PAD + width_of(title, SECTION, true) + 8.0,
            y + SECTION * 0.7,
        ),
        Size::new(right - PAD - width_of(title, SECTION, true) - 8.0, 0.0),
    )));
    y + SECTION * 1.4 + 8.0
}

/// A box of words at `y` in `colour` on its tint: its bottom.
fn band(
    shapes: &mut Vec<Shape>,
    text: &str,
    colour: u32,
    words: iced::Color,
    right: f32,
    y: f32,
) -> f32 {
    let inner = right - PAD - 20.0;
    let lines = lines_of(text, 12.0, false, inner);
    let h = lines.len() as f32 * 12.0 * 1.45 + 14.0;
    shapes.push(Shape::Box {
        rect: Rectangle::new(Point::new(PAD, y), Size::new(right - PAD, h)),
        radius: 9.0,
        fill: alpha(colour, 0.08),
        rim: Some(alpha(colour, 0.35)),
        dashed: false,
        sharp: Corner::None,
    });
    for (n, line) in lines.into_iter().enumerate() {
        shapes.push(Shape::Text(Line {
            text: line,
            at: Point::new(PAD + 10.0, y + 7.0 + n as f32 * 12.0 * 1.45 + 12.0 * 0.72),
            size: 12.0,
            color: words,
            bold: false,
            align: Alignment::Left,
        }));
    }
    y + h
}

/// A pill button at (`x`, `y`): its rect.
fn pill(
    shapes: &mut Vec<Shape>,
    open: &Open,
    target: Target,
    label: &str,
    x: f32,
    y: f32,
) -> Rectangle {
    let w = width_of(label, PILL, true) + 2.0 * BUTTON_X;
    let rect = Rectangle::new(Point::new(x, y), Size::new(w, PILL_H));
    let hover = open.hover == Some(target);
    shapes.push(Shape::Box {
        rect,
        radius: 8.0,
        fill: alpha(0xFFFFFF, if hover { 0.12 } else { 0.06 }),
        rim: Some(alpha(0x7EE0A0, if hover { 0.55 } else { 0.3 })),
        dashed: false,
        sharp: Corner::None,
    });
    shapes.push(Shape::Text(Line {
        text: label.to_owned(),
        at: Point::new(x + w / 2.0, y + PILL_H / 2.0),
        size: PILL,
        color: MINT,
        bold: true,
        align: Alignment::Center,
    }));
    rect
}

/// The queue laid out from `top`, scrolled by `open.scroll` (from its
/// top: `f32::MAX` shows its top).
#[allow(clippy::too_many_arguments)]
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
    let mut y = THREAD_TOP;
    let back = words.back();
    let bw = width_of(back, SMALL, true);
    shapes.push(Shape::Text(Line {
        text: back.to_owned(),
        at: Point::new(PAD, y + SMALL_LINE / 2.0),
        size: SMALL,
        color: MINT,
        bold: true,
        align: Alignment::Left,
    }));
    targets.push((
        Target::Back,
        Rectangle::new(
            Point::new(PAD - 4.0, y - 2.0),
            Size::new(bw + 8.0, SMALL_LINE + 4.0),
        ),
    ));
    y += SMALL_LINE + GAP;
    said.push("queue".to_owned());
    if let Some(view) = view {
        let title = words.pick("LE TRI SUR CET ORDINATEUR", "THE SORTING ON THIS COMPUTER");
        y = section(&mut shapes, title, right, y);
        // What she does, the counts, the model.
        shapes.push(Shape::Text(Line {
            text: view.state.clone(),
            at: Point::new(PAD, y + STATE * 0.72),
            size: STATE,
            color: TEXT,
            bold: true,
            align: Alignment::Left,
        }));
        if !view.model.is_empty() {
            shapes.push(Shape::Mono(Line {
                text: view.model.clone(),
                at: Point::new(right, y + STATE * 0.72),
                size: 10.5,
                color: FAINT,
                bold: false,
                align: Alignment::Right,
            }));
        }
        y += STATE * 1.4;
        for line in lines_of(&view.counts, 12.0, false, right - PAD) {
            shapes.push(Shape::Text(Line {
                text: line,
                at: Point::new(PAD, y + 12.0 * 0.72),
                size: 12.0,
                color: DIM,
                bold: false,
                align: Alignment::Left,
            }));
            y += 12.0 * 1.45;
        }
        said.push(format!("state:{} | {}", view.state, view.counts));
        y += 8.0;
        // The item read now.
        if let Some(now) = &view.now {
            let inner = right - PAD - 24.0;
            let x = PAD + 12.0;
            let mut parts: Vec<Shape> = Vec::new();
            let mut at = y + 9.0;
            parts.push(Shape::Text(Line {
                text: words.pick("EN COURS", "NOW").to_owned(),
                at: Point::new(x, at + 10.0 * 0.7),
                size: 10.0,
                color: MINT,
                bold: true,
                align: Alignment::Left,
            }));
            at += 10.0 * 1.4 + 4.0;
            let sender = crate::popover::kit::elide(&now.sender, inner, |s| width_of(s, ROW, true));
            parts.push(Shape::Text(Line {
                text: sender,
                at: Point::new(x, at + ROW_LINE / 2.0),
                size: ROW,
                color: TEXT,
                bold: true,
                align: Alignment::Left,
            }));
            at += ROW_LINE;
            for line in lines_of(&now.subject, ROW, false, inner)
                .into_iter()
                .take(2)
            {
                parts.push(Shape::Text(Line {
                    text: line,
                    at: Point::new(x, at + ROW_LINE / 2.0),
                    size: ROW,
                    color: hex(0xD5DBE5),
                    bold: false,
                    align: Alignment::Left,
                }));
                at += ROW_LINE;
            }
            parts.push(Shape::Text(Line {
                text: format!("{} · {}", now.source, now.date),
                at: Point::new(x, at + NOTE_LINE / 2.0),
                size: NOTE,
                color: FAINT,
                bold: false,
                align: Alignment::Left,
            }));
            at += NOTE_LINE + 9.0;
            shapes.push(Shape::Box {
                rect: Rectangle::new(Point::new(PAD, y), Size::new(right - PAD, at - y)),
                radius: 12.0,
                fill: hex(0x121827),
                rim: Some(alpha(0x7EE0A0, 0.35)),
                dashed: false,
                sharp: Corner::None,
            });
            shapes.extend(parts);
            said.push(format!(
                "now:{} | {} | {}",
                now.sender, now.subject, now.source
            ));
            y = at + 10.0;
        }
        // Its pace, the time left.
        let pace = match (view.rate.is_empty(), view.left.is_empty()) {
            (false, false) => format!("{} · {}", view.rate, view.left),
            (false, true) => view.rate.clone(),
            _ if view.waiting > 0 => words
                .pick(
                    "Le rythme se mesure au premier élément.",
                    "The pace is measured on the first item.",
                )
                .to_owned(),
            _ => String::new(),
        };
        if !pace.is_empty() {
            shapes.push(Shape::Text(Line {
                text: pace.clone(),
                at: Point::new(PAD, y + 12.5 * 0.72),
                size: 12.5,
                color: DIM,
                bold: false,
                align: Alignment::Left,
            }));
            y += 12.5 * 1.45 + 8.0;
            said.push(format!("pace:{pace}"));
        }
        if let Some(pause) = &view.pause {
            y = band(&mut shapes, pause, 0xF5B44A, hex(0xFFE2A8), right, y) + 8.0;
            said.push(format!("pause:{pause}"));
        }
        if let Some(slow) = &view.slow {
            y = band(&mut shapes, slow, 0xF2945B, ORANGE, right, y) + 8.0;
            said.push(format!("slow:{slow}"));
        }
        y += 6.0;
        // What waits, by sender.
        let waiting = if words.fr {
            format!("EN ATTENTE · {}", view.waiting)
        } else {
            format!("WAITING · {}", view.waiting)
        };
        y = section(&mut shapes, &waiting, right, y);
        let aside_label = words.pick(
            "Ne pas trier, chercher si besoin",
            "Don’t sort, search when needed",
        );
        for (n, group) in view.groups.iter().enumerate() {
            let count = group.count.to_string();
            let cw = width_of(&count, ROW, true) + 8.0;
            let label = crate::popover::kit::elide(&group.label, right - PAD - cw - 8.0, |s| {
                width_of(s, ROW, true)
            });
            shapes.push(Shape::Text(Line {
                text: label,
                at: Point::new(PAD, y + ROW_LINE / 2.0),
                size: ROW,
                color: TEXT,
                bold: true,
                align: Alignment::Left,
            }));
            shapes.push(Shape::Mono(Line {
                text: count,
                at: Point::new(right, y + ROW_LINE / 2.0),
                size: 12.0,
                color: if group.automated.is_some() {
                    AMBER
                } else {
                    DIM
                },
                bold: false,
                align: Alignment::Right,
            }));
            y += ROW_LINE;
            if !group.note.is_empty() {
                let note = crate::popover::kit::elide(&group.note, right - PAD, |s| {
                    width_of(s, NOTE, false)
                });
                shapes.push(Shape::Text(Line {
                    text: note,
                    at: Point::new(PAD, y + NOTE_LINE / 2.0),
                    size: NOTE,
                    color: FAINT,
                    bold: false,
                    align: Alignment::Left,
                }));
                y += NOTE_LINE;
            }
            if group.can_aside {
                y += 4.0;
                let target = Target::Aside(n as u32);
                let rect = pill(&mut shapes, open, target, aside_label, PAD, y);
                targets.push((target, rect));
                y += PILL_H;
            }
            y += 10.0;
            said.push(format!("group:{} · {}", group.label, group.count));
        }
        if view.groups.is_empty() {
            shapes.push(Shape::Text(Line {
                text: words.pick("Rien n’attend.", "Nothing waits.").to_owned(),
                at: Point::new(PAD, y + ROW_LINE / 2.0),
                size: ROW,
                color: DIM,
                bold: false,
                align: Alignment::Left,
            }));
            y += ROW_LINE + 10.0;
        }
        if !view.more.is_empty() {
            shapes.push(Shape::Text(Line {
                text: view.more.clone(),
                at: Point::new(PAD, y + NOTE_LINE / 2.0),
                size: NOTE,
                color: FAINT,
                bold: false,
                align: Alignment::Left,
            }));
            y += NOTE_LINE + 10.0;
        }
        // What is set aside.
        if !view.aside.is_empty() {
            y += 4.0;
            let title = if words.fr {
                format!("MIS DE CÔTÉ · {}", view.set_aside)
            } else {
                format!("SET ASIDE · {}", view.set_aside)
            };
            y = section(&mut shapes, &title, right, y);
            let back = words.pick("Trier quand même", "Sort them anyway");
            for (n, group) in view.aside.iter().enumerate() {
                let count = group.count.to_string();
                let bw = width_of(back, PILL, true) + 2.0 * BUTTON_X;
                let label =
                    crate::popover::kit::elide(&group.label, right - PAD - bw - 50.0, |s| {
                        width_of(s, ROW, true)
                    });
                shapes.push(Shape::Text(Line {
                    text: label,
                    at: Point::new(PAD, y + PILL_H / 2.0),
                    size: ROW,
                    color: DIM,
                    bold: true,
                    align: Alignment::Left,
                }));
                shapes.push(Shape::Mono(Line {
                    text: count,
                    at: Point::new(right - bw - 10.0, y + PILL_H / 2.0),
                    size: 12.0,
                    color: FAINT,
                    bold: false,
                    align: Alignment::Right,
                }));
                let target = Target::Unaside(n as u32);
                let rect = pill(&mut shapes, open, target, back, right - bw, y);
                targets.push((target, rect));
                y += PILL_H + 8.0;
                said.push(format!("aside:{} · {}", group.label, group.count));
            }
            for line in lines_of(
                words.pick(
                    "Je garde leur expéditeur, leur objet et leur date ; celui dont tu me parles, je le trie d’abord.",
                    "I keep their sender, subject and date; the one you ask me about, I sort first.",
                ),
                NOTE,
                false,
                right - PAD,
            ) {
                shapes.push(Shape::Text(Line {
                    text: line,
                    at: Point::new(PAD, y + NOTE_LINE / 2.0),
                    size: NOTE,
                    color: FAINT,
                    bold: false,
                    align: Alignment::Left,
                }));
                y += NOTE_LINE;
            }
        }
    } else {
        shapes.push(Shape::Text(Line {
            text: words
                .pick("Je regarde où en est le tri…", "Looking at the sorting…")
                .to_owned(),
            at: Point::new(PAD, y + ROW_LINE / 2.0),
            size: ROW,
            color: DIM,
            bold: false,
            align: Alignment::Left,
        }));
        y += ROW_LINE;
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
