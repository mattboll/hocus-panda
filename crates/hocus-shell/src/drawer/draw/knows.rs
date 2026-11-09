//! "Ce qu'elle sait" laid out (docs/design/secretary.md §8, the s11
//! mockup's memory view, at its sizes ×1): under the tabs, the notebook
//! switch (Tout · Pro · Perso), the search field and the filter chips with
//! their counts; then the sections (APPRIS CETTE SEMAINE, PERSONNES…),
//! each item a line with its tags, its timeline, its smaller lines, "d'où
//! je le sais" and its buttons on the right; the footer (the count, "sur
//! cette machine, chiffrés", Exporter, Oublier tout le perso…, Tout
//! oublier…); the toast ("Oublié · Annuler") and the confirmation of a
//! whole forget over it.
//!
//! Everything here is what she sent ([`Knowledge`]); the drawer keeps only
//! the query, the scroll, the item unfolded and the confirmation.

use hocus_secretary_link::{
    Notebook,
    knows::{Knowledge, Known, StepTone, Tone},
};
use iced::{Point, Rectangle, Size, widget::text::Alignment};

use super::{
    super::{Open, Target, Words},
    Corner, DIM, FAINT, INK, LABEL, Laid, Line, MINT, PAD, Shape, TEXT, alpha, hex, lines_of,
    width_of,
};
use crate::secretary::Toast;

const SWITCH: f32 = 22.0;
const SWITCH_TEXT: f32 = 11.5;
const SEARCH: f32 = 34.0;
const SEARCH_TEXT: f32 = 13.0;
const CHIP: f32 = 12.0;
const CHIP_H: f32 = CHIP * 1.4 + 8.0;
const SECTION: f32 = 10.5;
const ITEM: f32 = 13.0;
const ITEM_LINE: f32 = ITEM * 1.4;
const META: f32 = 11.5;
const META_LINE: f32 = META * 1.4;
const TAG: f32 = 11.0;
const TAG_H: f32 = 15.0;
const ACT: f32 = 11.5;
const ACT_H: f32 = ACT * 1.4 + 6.0;
const FOOT: f32 = 12.5;
const GAP: f32 = 6.0;

/// The tab laid out under `top`, from `view` as she last sent it.
#[allow(clippy::too_many_arguments)]
pub(super) fn lay_out(
    laid: &mut Laid,
    open: &Open,
    view: Option<&Knowledge>,
    toast: Option<&Toast>,
    words: Words,
    top: f32,
    right: f32,
    said: &mut Vec<String>,
) {
    let width = laid.size.width;
    let height = laid.size.height;
    let mut y = top;
    let mut fixed: Vec<Shape> = Vec::new();
    let mut targets: Vec<(Target, Rectangle)> = Vec::new();
    let confirming = open.confirm.is_some();
    // The notebook switch.
    let labels = words.notebooks();
    let mut x = PAD + 2.0;
    let widths: Vec<f32> = labels
        .iter()
        .map(|l| width_of(l, SWITCH_TEXT, true) + 24.0)
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + 2.0 * (labels.len() as f32 - 1.0) + 4.0;
    fixed.push(Shape::Box {
        rect: Rectangle::new(Point::new(PAD, y), Size::new(total, SWITCH + 4.0)),
        radius: 8.0,
        fill: alpha(0xFFFFFF, 0.035),
        rim: Some(alpha(0xFFFFFF, 0.07)),
        dashed: false,
        sharp: Corner::None,
    });
    let on = match open.book {
        None => 0,
        Some(Notebook::Pro) => 1,
        Some(Notebook::Perso) => 2,
    };
    for (n, (label, w)) in labels.iter().zip(&widths).enumerate() {
        let rect = Rectangle::new(Point::new(x, y + 2.0), Size::new(*w, SWITCH));
        let selected = n == on;
        if selected {
            fixed.push(Shape::Box {
                rect,
                radius: 6.0,
                fill: if n == 2 {
                    alpha(0xF28FB1, 0.22)
                } else {
                    alpha(0xFFFFFF, 0.12)
                },
                rim: None,
                dashed: false,
                sharp: Corner::None,
            });
        }
        fixed.push(text(
            label,
            Point::new(rect.center_x(), rect.center_y()),
            SWITCH_TEXT,
            match (selected, n) {
                (true, 2) => hex(0xFBD3E1),
                (true, _) => TEXT,
                _ => LABEL,
            },
            true,
            Alignment::Center,
        ));
        targets.push((Target::Notebook(n as u8), rect));
        x += w + 2.0;
    }
    y += SWITCH + 4.0 + 8.0;
    // The search field.
    let field = Rectangle::new(Point::new(PAD, y), Size::new(right - PAD, SEARCH));
    let focused = open.hover == Some(Target::Search) || !open.search.is_empty();
    fixed.push(Shape::Box {
        rect: field,
        radius: 9.0,
        fill: alpha(0xFFFFFF, 0.05),
        rim: Some(if focused {
            alpha(0x6CC287, 0.45)
        } else {
            alpha(0xFFFFFF, 0.10)
        }),
        dashed: false,
        sharp: Corner::None,
    });
    fixed.push(Shape::Lens(Point::new(field.x + 17.0, field.center_y())));
    let (shown, colour) = if open.search.is_empty() {
        (words.search().to_owned(), FAINT)
    } else {
        (format!("{}▏", open.search), TEXT)
    };
    fixed.push(text(
        &shown,
        Point::new(field.x + 32.0, field.center_y()),
        SEARCH_TEXT,
        colour,
        false,
        Alignment::Left,
    ));
    targets.push((Target::Search, field));
    y += SEARCH + 8.0;
    // The chips.
    if let Some(view) = view {
        let mut x = PAD;
        for (n, chip) in view.chips.iter().enumerate() {
            let count = if chip.kind.is_some() {
                format!("{}", chip.count)
            } else {
                String::new()
            };
            let lw = width_of(&chip.label, CHIP, true);
            let cw = if count.is_empty() {
                0.0
            } else {
                4.0 + width_of(&count, CHIP, false)
            };
            let w = lw + cw + 20.0;
            if x > PAD && x + w > right {
                x = PAD;
                y += CHIP_H + 6.0;
            }
            let rect = Rectangle::new(Point::new(x, y), Size::new(w, CHIP_H));
            let selected = chip.kind == open.kind;
            let hover = open.hover == Some(Target::Chip(n as u8));
            fixed.push(Shape::Box {
                rect,
                radius: CHIP_H / 2.0,
                fill: if selected {
                    alpha(0x7EE0A0, 0.14)
                } else if hover {
                    alpha(0xFFFFFF, 0.09)
                } else {
                    alpha(0xFFFFFF, 0.05)
                },
                rim: Some(if selected {
                    alpha(0x7EE0A0, 0.45)
                } else {
                    alpha(0xFFFFFF, 0.1)
                }),
                dashed: false,
                sharp: Corner::None,
            });
            fixed.push(text(
                &chip.label,
                Point::new(x + 10.0, rect.center_y()),
                CHIP,
                if selected {
                    hex(0x9BEBB8)
                } else {
                    hex(0xC9D0DC)
                },
                true,
                Alignment::Left,
            ));
            if !count.is_empty() {
                fixed.push(text(
                    &count,
                    Point::new(x + 10.0 + lw + 4.0, rect.center_y()),
                    CHIP,
                    LABEL,
                    false,
                    Alignment::Left,
                ));
            }
            targets.push((Target::Chip(n as u8), rect));
            x += w + 6.0;
        }
        y += CHIP_H + 8.0;
    }
    // The footer, from the bottom.
    let foot_y = height - 10.0 - FOOT * 1.5;
    let foot_rule = foot_y - 8.0;
    fixed.push(Shape::Box {
        rect: Rectangle::new(Point::new(1.0, foot_rule), Size::new(width - 2.0, 1.0)),
        radius: 0.0,
        fill: alpha(0xFFFFFF, 0.07),
        rim: None,
        dashed: false,
        sharp: Corner::None,
    });
    let middle = foot_y + FOOT * 0.75;
    let mut x = PAD;
    let total = view.map_or(0, |v| v.total);
    let parts: [(String, Option<Target>); 4] = [
        (words.memories(total), None),
        (words.export().to_owned(), Some(Target::Export)),
        (words.forget_perso().to_owned(), Some(Target::ForgetPerso)),
        (words.forget_all().to_owned(), Some(Target::ForgetAll)),
    ];
    for (n, (part, target)) in parts.iter().enumerate() {
        if n > 0 {
            fixed.push(text(
                " · ",
                Point::new(x, middle),
                FOOT,
                FAINT,
                false,
                Alignment::Left,
            ));
            x += width_of(" · ", FOOT, false);
        }
        let bold = target.is_some();
        let w = width_of(part, FOOT, bold);
        let hover = target.is_some_and(|t| open.hover == Some(t));
        fixed.push(text(
            part,
            Point::new(x, middle),
            FOOT,
            match (target, hover) {
                (Some(Target::ForgetPerso | Target::ForgetAll), true) => hex(0xF7A77A),
                (Some(_), true) => TEXT,
                (Some(_), false) => hex(0xC9D0DC),
                (None, _) => DIM,
            },
            bold,
            Alignment::Left,
        ));
        if let Some(target) = target {
            targets.push((
                *target,
                Rectangle::new(
                    Point::new(x - 2.0, foot_y - 2.0),
                    Size::new(w + 4.0, FOOT * 1.5 + 4.0),
                ),
            ));
        }
        x += w;
    }
    // The list, scrolled.
    let list = Rectangle::new(
        Point::new(0.0, y),
        Size::new(width, (foot_rule - 6.0 - y).max(0.0)),
    );
    laid.thread = list;
    let mut shapes: Vec<Shape> = Vec::new();
    let mut list_targets: Vec<(Target, Rectangle)> = Vec::new();
    // Each item's (and title's) shapes, and where it is: only those whole
    // in the list are drawn (the CPU renderer does not clip).
    let mut blocks: Vec<(usize, usize, f32, f32)> = Vec::new();
    let mut at = 2.0;
    match view {
        None => {
            shapes.push(text(
                words.connecting(),
                Point::new(width / 2.0, at + 30.0),
                ITEM,
                DIM,
                false,
                Alignment::Center,
            ));
            at += 60.0;
        }
        Some(view) if view.sections.is_empty() => {
            let empty = words.pick(
                "Rien à montrer ici pour l’instant.",
                "Nothing to show here yet.",
            );
            shapes.push(text(
                empty,
                Point::new(width / 2.0, at + 30.0),
                ITEM,
                DIM,
                false,
                Alignment::Center,
            ));
            at += 60.0;
        }
        Some(view) => {
            let mut index = 0u32;
            for section in &view.sections {
                at += 8.0;
                let first = shapes.len();
                let from = at;
                let mid = at + SECTION * 0.7;
                shapes.push(text(
                    &section.title,
                    Point::new(PAD + 2.0, mid),
                    SECTION,
                    LABEL,
                    true,
                    Alignment::Left,
                ));
                shapes.push(text(
                    &section.note,
                    Point::new(right - 2.0, mid),
                    SECTION,
                    LABEL,
                    false,
                    Alignment::Right,
                ));
                said.push(format!("section:{} ({})", section.title, section.note));
                at += SECTION * 1.4 + 4.0;
                blocks.push((first, shapes.len(), from, at));
                for known in &section.items {
                    let first = shapes.len();
                    let from = at;
                    at = item(
                        &mut shapes,
                        &mut list_targets,
                        open,
                        known,
                        index,
                        words,
                        right,
                        at,
                        said,
                    );
                    blocks.push((first, shapes.len(), from, at));
                    at += GAP;
                    index += 1;
                }
            }
        }
    }
    at += 8.0;
    laid.content = at;
    let room = list.height;
    let scroll = open.knows_scroll.clamp(0.0, (at - room).max(0.0));
    let shift = list.y - scroll;
    // The toast's band: what text the list has there would come over it
    // (a layer's text is drawn over the next layer's shapes).
    let band = toast.map(|_| (foot_rule - 14.0 - 38.0, foot_rule - 14.0));
    let whole = |top: f32, bottom: f32| {
        let (top, bottom) = (top + shift, bottom + shift);
        top >= list.y - 0.5
            && bottom <= list.y + room + 0.5
            && band.is_none_or(|(a, b)| bottom <= a || top >= b)
    };
    if confirming {
        // Under the confirmation, nothing of the list (see above).
    } else if blocks.is_empty() {
        for shape in shapes {
            laid.shapes.push(super::moved(shape, shift));
        }
    } else {
        for (first, last, top, bottom) in &blocks {
            if whole(*top, *bottom) {
                for shape in &shapes[*first..*last] {
                    laid.shapes.push(super::moved(shape.clone(), shift));
                }
            }
        }
    }
    if !confirming {
        for (target, rect) in list_targets {
            let rect = Rectangle::new(Point::new(rect.x, rect.y + shift), rect.size());
            if whole(rect.y - shift, rect.y + rect.height - shift) {
                laid.targets.push((target, rect));
            }
        }
        laid.targets.extend(targets);
    }
    laid.overlay.extend(fixed);
    // The toast, over the list's bottom.
    if let Some(toast) = toast {
        let undo = toast.undo.map(|_| words.undo());
        let tw = width_of(&toast.text, FOOT, false);
        let uw = undo.map_or(0.0, |u| 14.0 + width_of(u, FOOT, true));
        let w = tw + uw + 28.0;
        let rect = Rectangle::new(
            Point::new((width - w) / 2.0, foot_rule - 14.0 - 38.0),
            Size::new(w, 38.0),
        );
        laid.overlay.push(Shape::Box {
            rect,
            radius: 10.0,
            fill: hex(0x0B1A12),
            rim: Some(alpha(0x7EE0A0, 0.4)),
            dashed: false,
            sharp: Corner::None,
        });
        laid.overlay.push(text(
            &toast.text,
            Point::new(rect.x + 14.0, rect.center_y()),
            FOOT,
            TEXT,
            false,
            Alignment::Left,
        ));
        if let Some(undo) = undo {
            let ux = rect.x + 14.0 + tw + 14.0;
            laid.overlay.push(text(
                undo,
                Point::new(ux, rect.center_y()),
                FOOT,
                MINT,
                true,
                Alignment::Left,
            ));
            if !confirming {
                laid.targets.push((
                    Target::ToastUndo,
                    Rectangle::new(
                        Point::new(ux - 4.0, rect.y + 4.0),
                        Size::new(uw - 6.0, 30.0),
                    ),
                ));
            }
        }
        said.push(format!(
            "toast:{}{}",
            toast.text,
            if toast.undo.is_some() {
                " · Annuler"
            } else {
                ""
            }
        ));
    }
    // A whole forget, to confirm.
    if let Some(what) = open.confirm {
        let (title, body, yes) = words.confirm(what);
        laid.overlay.push(Shape::Box {
            rect: Rectangle::new(Point::ORIGIN, laid.size),
            radius: 14.0,
            fill: alpha(0x05070D, 0.62),
            rim: None,
            dashed: false,
            sharp: Corner::None,
        });
        let w = 440.0_f32.min(width - 2.0 * PAD);
        let inner = w - 40.0;
        let body_lines = lines_of(body, ITEM, false, inner);
        let button_h = 12.5 * 1.4 + 14.0;
        let h =
            20.0 + 15.0 * 1.4 + 8.0 + body_lines.len() as f32 * ITEM_LINE + 16.0 + button_h + 20.0;
        let card = Rectangle::new(
            Point::new((width - w) / 2.0, (height - h) / 2.0),
            Size::new(w, h),
        );
        laid.overlay.push(Shape::Box {
            rect: card,
            radius: 14.0,
            fill: hex(0x161B2A),
            rim: Some(alpha(0xF2945B, 0.5)),
            dashed: false,
            sharp: Corner::None,
        });
        let mut cy = card.y + 20.0;
        laid.overlay.push(text(
            title,
            Point::new(card.x + 20.0, cy + 10.0),
            15.0,
            TEXT,
            true,
            Alignment::Left,
        ));
        cy += 15.0 * 1.4 + 8.0;
        for line in &body_lines {
            laid.overlay.push(text(
                line,
                Point::new(card.x + 20.0, cy + ITEM_LINE / 2.0),
                ITEM,
                DIM,
                false,
                Alignment::Left,
            ));
            cy += ITEM_LINE;
        }
        cy += 16.0;
        let keep = words.keep_it();
        let yw = width_of(yes, 12.5, true) + 24.0;
        let kw = width_of(keep, 12.5, true) + 24.0;
        let yes_rect = Rectangle::new(Point::new(card.x + 20.0, cy), Size::new(yw, button_h));
        let keep_rect = Rectangle::new(
            Point::new(card.x + 20.0 + yw + 8.0, cy),
            Size::new(kw, button_h),
        );
        let hover_yes = open.hover == Some(Target::Confirm(true));
        laid.overlay.push(Shape::Box {
            rect: yes_rect,
            radius: 8.0,
            fill: if hover_yes {
                hex(0xF7A77A)
            } else {
                hex(0xF2945B)
            },
            rim: None,
            dashed: false,
            sharp: Corner::None,
        });
        laid.overlay.push(text(
            yes,
            yes_rect.center(),
            12.5,
            INK,
            true,
            Alignment::Center,
        ));
        laid.overlay.push(Shape::Box {
            rect: keep_rect,
            radius: 8.0,
            fill: alpha(
                0xFFFFFF,
                if open.hover == Some(Target::Confirm(false)) {
                    0.12
                } else {
                    0.07
                },
            ),
            rim: Some(alpha(0xFFFFFF, 0.14)),
            dashed: false,
            sharp: Corner::None,
        });
        laid.overlay.push(text(
            keep,
            keep_rect.center(),
            12.5,
            hex(0xD5DBE5),
            true,
            Alignment::Center,
        ));
        laid.targets.push((Target::Confirm(true), yes_rect));
        laid.targets.push((Target::Confirm(false), keep_rect));
        said.push(format!("confirm:{title}"));
    }
}

fn text(
    content: &str,
    at: Point,
    size: f32,
    color: iced::Color,
    bold: bool,
    align: Alignment,
) -> Shape {
    Shape::Text(Line {
        text: content.to_owned(),
        at,
        size,
        color,
        bold,
        align,
    })
}

/// One item at `y`: its bottom.
#[allow(clippy::too_many_arguments)]
fn item(
    shapes: &mut Vec<Shape>,
    targets: &mut Vec<(Target, Rectangle)>,
    open: &Open,
    known: &Known,
    index: u32,
    words: Words,
    right: f32,
    y: f32,
    said: &mut Vec<String>,
) -> f32 {
    let x = PAD;
    let w = right - PAD;
    // Its buttons, on the right.
    let acts: Vec<(String, f32)> = known
        .actions
        .iter()
        .map(|a| {
            let label = words.act(*a, &known.item).to_owned();
            let lw = width_of(&label, ACT, true) + 14.0;
            (label, lw)
        })
        .collect();
    let acts_w: f32 =
        acts.iter().map(|(_, w)| *w).sum::<f32>() + 4.0 * acts.len().saturating_sub(1) as f32;
    let room = w - 16.0 - if acts.is_empty() { 0.0 } else { acts_w + 10.0 };
    let mut parts: Vec<Shape> = Vec::new();
    let mut at = y + 7.0;
    let tx = x + 8.0;
    // Its line: the title bold, the rest after it.
    let whole = format!("{}{}", known.title, known.text);
    let lines = lines_of(&whole, ITEM, true, room);
    let gone = known.left;
    for (n, line) in lines.iter().enumerate() {
        let mid = at + ITEM_LINE / 2.0;
        if n == 0 && !known.title.is_empty() && line.starts_with(&known.title) {
            let tw = width_of(&known.title, ITEM, true);
            parts.push(text(
                &known.title,
                Point::new(tx, mid),
                ITEM,
                if gone { DIM } else { TEXT },
                true,
                Alignment::Left,
            ));
            parts.push(text(
                &line[known.title.len()..],
                Point::new(tx + tw, mid),
                ITEM,
                hex(0xC9D0DC),
                false,
                Alignment::Left,
            ));
        } else {
            parts.push(text(
                line,
                Point::new(tx, mid),
                ITEM,
                if gone { DIM } else { TEXT },
                false,
                Alignment::Left,
            ));
        }
        at += ITEM_LINE;
    }
    // Its tags.
    if !known.tags.is_empty() {
        let mut tagx = tx;
        at += 2.0;
        for tag in &known.tags {
            let tw = width_of(&tag.text, TAG, true) + 12.0;
            if tagx > tx && tagx + tw > tx + room {
                tagx = tx;
                at += TAG_H + 3.0;
            }
            let (fill, colour) = match tag.tone {
                Tone::Plain => (alpha(0xFFFFFF, 0.07), hex(0xD5DBE5)),
                Tone::New => (alpha(0x8FB3FF, 0.16), hex(0xB9CEFF)),
                Tone::Warn => (alpha(0xF5B44A, 0.14), hex(0xF5C46E)),
                Tone::Perso => (alpha(0xF28FB1, 0.18), hex(0xFBD3E1)),
            };
            let rect = Rectangle::new(Point::new(tagx, at), Size::new(tw, TAG_H));
            parts.push(Shape::Box {
                rect,
                radius: 5.0,
                fill,
                rim: None,
                dashed: false,
                sharp: Corner::None,
            });
            parts.push(text(
                &tag.text,
                rect.center(),
                TAG,
                colour,
                true,
                Alignment::Center,
            ));
            tagx += tw + 3.0;
        }
        at += TAG_H;
    }
    // Its timeline.
    if !known.timeline.is_empty() {
        at += 3.0;
        let mid = at + 11.0 * 0.7;
        let mut sx = tx;
        for (n, step) in known.timeline.iter().enumerate() {
            if n > 0 {
                parts.push(Shape::Box {
                    rect: Rectangle::new(Point::new(sx, mid), Size::new(16.0, 1.0)),
                    radius: 0.0,
                    fill: alpha(0xFFFFFF, 0.2),
                    rim: None,
                    dashed: false,
                    sharp: Corner::None,
                });
                sx += 20.0;
            }
            parts.push(Shape::Box {
                rect: Rectangle::new(Point::new(sx, mid - 3.0), Size::new(6.0, 6.0)),
                radius: 3.0,
                fill: match step.tone {
                    StepTone::Done => hex(0x7EE0A0),
                    StepTone::New => hex(0xF5B44A),
                    StepTone::Ended => hex(0x5A6274),
                },
                rim: None,
                dashed: false,
                sharp: Corner::None,
            });
            sx += 10.0;
            parts.push(text(
                &step.text,
                Point::new(sx, mid),
                11.0,
                LABEL,
                false,
                Alignment::Left,
            ));
            sx += width_of(&step.text, 11.0, false) + 4.0;
        }
        at += 11.0 * 1.4;
    }
    // Its smaller lines.
    for line in &known.lines {
        for l in lines_of(line, META, false, room) {
            parts.push(text(
                &l,
                Point::new(tx, at + META_LINE / 2.0),
                META,
                DIM,
                false,
                Alignment::Left,
            ));
            at += META_LINE;
        }
    }
    // Where she has it from, and its link.
    at += 1.0;
    let link = known.origin.open.is_some();
    let origin_lines = lines_of(
        &known.origin.text,
        META,
        false,
        room - if link { 20.0 } else { 0.0 },
    );
    let mut last_end = tx;
    for l in &origin_lines {
        parts.push(text(
            l,
            Point::new(tx, at + META_LINE / 2.0),
            META,
            LABEL,
            false,
            Alignment::Left,
        ));
        last_end = tx + width_of(l, META, false);
        at += META_LINE;
    }
    if link {
        let rect = Rectangle::new(
            Point::new(last_end + 2.0, at - META_LINE),
            Size::new(16.0, META_LINE),
        );
        parts.push(text(
            "↗",
            Point::new(rect.x + 3.0, rect.center_y()),
            META,
            MINT,
            true,
            Alignment::Left,
        ));
        targets.push((Target::Open(index), rect));
    }
    // Unfolded: the words quoted, and its source.
    let expanded = open.expanded.as_deref() == Some(super::super::unfolded(known).as_str());
    if expanded {
        at += 4.0;
        if let Some(quote) = &known.origin.quote {
            let quoted = if words.fr {
                format!("«\u{a0}{quote}\u{a0}»")
            } else {
                format!("“{quote}”")
            };
            let ql = lines_of(&quoted, 12.0, false, room - 12.0);
            parts.push(Shape::Quote(Rectangle::new(
                Point::new(tx, at + 2.0),
                Size::new(2.0, ql.len() as f32 * 12.0 * 1.45),
            )));
            for l in &ql {
                parts.push(text(
                    l,
                    Point::new(tx + 10.0, at + 12.0 * 0.725),
                    12.0,
                    hex(0xE6EAF1),
                    false,
                    Alignment::Left,
                ));
                at += 12.0 * 1.45;
            }
        }
        if let Some(word) = known.origin.provenance.map(|p| match (p, words.fr) {
            (hocus_secretary_link::Provenance::Told, true) => "tu me l’as dit",
            (hocus_secretary_link::Provenance::Extracted, true) => {
                "lu noir sur blanc dans une source"
            }
            (hocus_secretary_link::Provenance::Inferred, true) => "déduit par elle",
            (hocus_secretary_link::Provenance::Told, false) => "you told me",
            (hocus_secretary_link::Provenance::Extracted, false) => "read in a source",
            (hocus_secretary_link::Provenance::Inferred, false) => "inferred by her",
        }) {
            let line = format!(
                "{} · {}",
                words.pick("d’où je le sais", "where I have it from"),
                word
            );
            parts.push(text(
                &line,
                Point::new(tx, at + META_LINE / 2.0 + 2.0),
                META,
                DIM,
                false,
                Alignment::Left,
            ));
            at += META_LINE + 2.0;
        }
        if link {
            let label = words.open_source();
            let lw = width_of(label, META, true);
            let rect = Rectangle::new(
                Point::new(tx - 2.0, at),
                Size::new(lw + 4.0, META_LINE + 2.0),
            );
            parts.push(text(
                label,
                Point::new(tx, at + META_LINE / 2.0 + 1.0),
                META,
                MINT,
                true,
                Alignment::Left,
            ));
            targets.push((Target::Open(index), rect));
            at += META_LINE + 2.0;
        }
    }
    // Its buttons.
    let mut bx = x + w - 8.0 - acts_w;
    let mut acts_bottom = y + 7.0;
    for (act, (label, lw)) in known.actions.iter().zip(&acts) {
        let rect = Rectangle::new(Point::new(bx, y + 6.0), Size::new(*lw, ACT_H));
        let target = Target::Known { index, act: *act };
        let hover = open.hover == Some(target);
        parts.push(Shape::Box {
            rect,
            radius: 6.0,
            fill: alpha(0xFFFFFF, if hover { 0.1 } else { 0.05 }),
            rim: None,
            dashed: false,
            sharp: Corner::None,
        });
        let forget = matches!(act, hocus_secretary_link::knows::Act::Forget)
            || (matches!(act, hocus_secretary_link::knows::Act::Reject)
                && !known.item.starts_with("seen:"));
        parts.push(text(
            label,
            rect.center(),
            ACT,
            if hover && forget {
                hex(0xF7A77A)
            } else if hover {
                TEXT
            } else {
                DIM
            },
            true,
            Alignment::Center,
        ));
        targets.push((target, rect));
        bx += lw + 4.0;
        acts_bottom = rect.y + rect.height;
    }
    let bottom = at.max(acts_bottom) + 7.0;
    let rect = Rectangle::new(Point::new(x, y), Size::new(w, bottom - y));
    shapes.push(Shape::Box {
        rect,
        radius: 9.0,
        fill: if known.left {
            alpha(0xFFFFFF, 0.0)
        } else if expanded {
            alpha(0xFFFFFF, 0.05)
        } else {
            alpha(0xFFFFFF, 0.025)
        },
        rim: Some(if known.proposal {
            alpha(0xF5C46E, 0.28)
        } else if known.left {
            alpha(0xFFFFFF, 0.14)
        } else {
            alpha(0xFFFFFF, 0.05)
        }),
        dashed: known.left || known.proposal,
        sharp: Corner::None,
    });
    shapes.extend(parts);
    // Its line unfolds its provenance: the whole item but its buttons
    // (they come after, and win).
    targets.insert(0, (Target::Expand(index), rect));
    said.push(format!(
        "{index}:{}:{}{} | {} | {}{}",
        known.item,
        known.title,
        known.text,
        known.origin.text,
        known
            .actions
            .iter()
            .map(|a| words.act(*a, &known.item))
            .collect::<Vec<_>>()
            .join("/"),
        if expanded { " | unfolded" } else { "" }
    ));
    bottom
}
