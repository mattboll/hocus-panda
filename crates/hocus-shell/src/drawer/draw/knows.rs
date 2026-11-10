//! "Ce qu'elle sait" laid out (docs/design/secretary.md §8, the s11
//! mockup's memory view, corrected by #758): under the tabs, the search
//! field with the one notebook switch beside it (Travail et privé ·
//! Travail · Privé), the filter chips with their counts (none chosen:
//! everything); then the sections (À CONFIRMER, APPRIS CETTE SEMAINE,
//! PERSONNES…), each item a line with its tags, its timeline, its smaller
//! lines and "d'où je le sais". Only what is to confirm has buttons
//! (« C'est juste · Faux », « Oui · Non »); the rest has « ··· » on hover,
//! its menu (Modifier, Ce n'est plus vrai, Voir d'où je le sais, Oublier).
//! The footer: the count and Exporter, « Effacer… ▾ » apart, in orange,
//! its menu (Oublier tout le privé…, Tout oublier…); the toast ("Oublié ·
//! Annuler"), never with the confirmation of a whole forget.
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
    // The search field, the notebook switch beside it (#758).
    let labels = words.notebooks();
    let widths: Vec<f32> = labels
        .iter()
        .map(|l| width_of(l, SWITCH_TEXT, true) + 22.0)
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + 2.0 * (labels.len() as f32 - 1.0) + 4.0;
    let switch_x = right - total;
    let switch_y = y + (SEARCH - SWITCH - 4.0) / 2.0;
    fixed.push(Shape::Box {
        rect: Rectangle::new(
            Point::new(switch_x, switch_y),
            Size::new(total, SWITCH + 4.0),
        ),
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
    let mut x = switch_x + 2.0;
    for (n, (label, w)) in labels.iter().zip(&widths).enumerate() {
        let rect = Rectangle::new(Point::new(x, switch_y + 2.0), Size::new(*w, SWITCH));
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
    let field = Rectangle::new(
        Point::new(PAD, y),
        Size::new((switch_x - 8.0 - PAD).max(80.0), SEARCH),
    );
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
    let shown = crate::popover::kit::elide(&shown, field.width - 40.0, |s| {
        width_of(s, SEARCH_TEXT, false)
    });
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
    let total = view.map_or(0, |v| v.total);
    // The count and « Exporter »; « Effacer… ▾ » apart, in orange (#758).
    let count = words.memories(total);
    let cw = width_of(&count, FOOT, false);
    fixed.push(text(
        &count,
        Point::new(PAD, middle),
        FOOT,
        DIM,
        false,
        Alignment::Left,
    ));
    let sep = " · ";
    let sw = width_of(sep, FOOT, false);
    fixed.push(text(
        sep,
        Point::new(PAD + cw, middle),
        FOOT,
        FAINT,
        false,
        Alignment::Left,
    ));
    let export = words.export();
    let ew = width_of(export, FOOT, true);
    let ex = PAD + cw + sw;
    fixed.push(text(
        export,
        Point::new(ex, middle),
        FOOT,
        if open.hover == Some(Target::Export) {
            TEXT
        } else {
            hex(0xC9D0DC)
        },
        true,
        Alignment::Left,
    ));
    targets.push((
        Target::Export,
        Rectangle::new(
            Point::new(ex - 2.0, foot_y - 2.0),
            Size::new(ew + 4.0, FOOT * 1.5 + 4.0),
        ),
    ));
    let erase = words.erase();
    let rw = width_of(erase, FOOT, true) + 18.0;
    let erase_rect = Rectangle::new(Point::new(right - rw, middle - 12.0), Size::new(rw, 24.0));
    let hover = open.hover == Some(Target::Erase) || open.erase;
    fixed.push(Shape::Box {
        rect: erase_rect,
        radius: 7.0,
        fill: alpha(0xF2945B, if hover { 0.14 } else { 0.06 }),
        rim: Some(alpha(0xF2945B, 0.55)),
        dashed: false,
        sharp: Corner::None,
    });
    fixed.push(text(
        erase,
        erase_rect.center(),
        FOOT,
        hex(0xF7A77A),
        true,
        Alignment::Center,
    ));
    targets.push((Target::Erase, erase_rect));
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
            // Her memory's key beside it, no keyring having had it (#768):
            // said first, where her memory is shown.
            if view.key_beside {
                at += 8.0;
                let first = shapes.len();
                let from = at;
                let room = right - PAD - 4.0;
                for line in lines_of(
                    hocus_secretary_link::key_beside_said(words.fr),
                    META,
                    false,
                    room,
                ) {
                    shapes.push(text(
                        &line,
                        Point::new(PAD + 2.0, at + META * 0.7),
                        META,
                        hex(0xF7A77A),
                        false,
                        Alignment::Left,
                    ));
                    at += META_LINE;
                }
                said.push("key-beside".to_owned());
                blocks.push((first, shapes.len(), from, at));
            }
            // A public moment (§7.4): her perso items are not sent, only
            // how many.
            if view.hidden > 0 {
                at += 8.0;
                let n = view.hidden as usize;
                shapes.push(text(
                    &words.hidden(n),
                    Point::new(PAD + 2.0, at + ITEM * 0.7),
                    ITEM,
                    hex(0xF4B6C9),
                    false,
                    Alignment::Left,
                ));
                said.push(format!("hidden:{n}"));
                at += ITEM * 1.4;
            }
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
    // Never the toast with a confirmation (#758).
    let toast = toast.filter(|_| !confirming);
    let band = toast.map(|_| (foot_rule - 14.0 - 38.0, foot_rule - 14.0));
    // Under a confirmation's card, nothing of the list: its text would
    // come over it; the rest of the list stays.
    let card_band = open
        .confirm
        .map(|_| (height / 2.0 - 120.0, height / 2.0 + 120.0));
    let whole = |top: f32, bottom: f32| {
        let (top, bottom) = (top + shift, bottom + shift);
        top >= list.y - 0.5
            && bottom <= list.y + room + 0.5
            && band.is_none_or(|(a, b)| bottom <= a || top >= b)
            && card_band.is_none_or(|(a, b)| bottom <= a || top >= b)
    };
    // Its words selectable, as the thread's (#761): only those whole in
    // the list take the pointer; its buttons, links and « ··· » are not
    // text to select.
    let skip: Vec<Rectangle> = list_targets
        .iter()
        .filter(|(t, _)| !t.holds_text())
        .map(|(_, r)| *r)
        .collect();
    let shows = |n: usize, top: f32, bottom: f32| {
        let item = blocks
            .iter()
            .find(|(first, last, _, _)| (*first..*last).contains(&n));
        match item {
            Some((_, _, from, to)) => whole(*from, *to),
            None => blocks.is_empty() && whole(top - shift, bottom - shift),
        }
    };
    super::lines_as_texts(
        &mut laid.texts,
        &shapes,
        &skip,
        shift,
        &super::select::Source::Line,
        &shows,
    );
    let mut menu = None;
    if !confirming {
        for (target, rect) in list_targets {
            let rect = Rectangle::new(Point::new(rect.x, rect.y + shift), rect.size());
            if whole(rect.y - shift, rect.y + rect.height - shift) {
                if let Target::KnownMenu(index) = target
                    && open.known_menu == Some(index)
                {
                    menu = Some((index, rect));
                }
                laid.targets.push((target, rect));
            }
        }
        laid.targets.extend(targets);
    }
    // An item's « ··· » menu, over the list (#758).
    let menu = menu.and_then(|(index, dots)| {
        let known = view.and_then(|v| v.items().nth(index as usize))?;
        use hocus_secretary_link::knows::Act;
        let mut entries: Vec<(Target, String, bool)> = Vec::new();
        for act in known
            .actions
            .iter()
            .filter(|a| matches!(a, Act::Edit | Act::Close))
        {
            entries.push((
                Target::Known { index, act: *act },
                words.act(*act, &known.item).to_owned(),
                false,
            ));
        }
        entries.push((Target::Expand(index), words.whence().to_owned(), false));
        for act in known
            .actions
            .iter()
            .filter(|a| matches!(a, Act::Reject | Act::Later))
        {
            entries.push((
                Target::Known { index, act: *act },
                words.act(*act, &known.item).to_owned(),
                false,
            ));
        }
        if known.actions.contains(&Act::Forget) {
            entries.push((
                Target::Known {
                    index,
                    act: Act::Forget,
                },
                words.act(Act::Forget, &known.item).to_owned(),
                true,
            ));
        }
        let w = entries
            .iter()
            .map(|(_, l, _)| width_of(l, ACT + 1.0, false))
            .fold(150.0, f32::max)
            + 24.0;
        let row = 26.0;
        let h = entries.len() as f32 * row + 8.0;
        let below = dots.y + dots.height + 4.0;
        let top = if below + h > foot_rule {
            dots.y - h - 4.0
        } else {
            below
        };
        let rect = Rectangle::new(Point::new(dots.x + dots.width - w, top), Size::new(w, h));
        Some((index, rect, entries, row))
    });
    // « Effacer… ▾ »'s menu, over the footer (#758).
    let erase_entries = [
        (Target::ForgetPerso, words.forget_perso()),
        (Target::ForgetAll, words.forget_all()),
    ];
    let erase_menu = (open.erase && !confirming).then(|| {
        let w = erase_entries
            .iter()
            .map(|(_, l)| width_of(l, ACT + 1.0, false))
            .fold(0.0, f32::max)
            + 28.0;
        let h = erase_entries.len() as f32 * 26.0 + 8.0;
        Rectangle::new(Point::new(right - w, foot_rule - 6.0 - h), Size::new(w, h))
    });
    // What the menus cover, the list's text does not draw there (a
    // layer's text is drawn over the next layer's shapes).
    let covers: Vec<Rectangle> = menu
        .iter()
        .map(|(_, r, _, _)| *r)
        .chain(erase_menu)
        .collect();
    let covered = |shape: &Shape| match shape {
        Shape::Text(line) => {
            let w = width_of(&line.text, line.size, line.bold);
            let x = match line.align {
                Alignment::Center => line.at.x - w / 2.0,
                Alignment::Right => line.at.x - w,
                _ => line.at.x,
            };
            let h = line.size * 1.4;
            let r = Rectangle::new(Point::new(x, line.at.y - h / 2.0), Size::new(w, h));
            covers.iter().any(|c| c.intersects(&r))
        }
        _ => false,
    };
    if blocks.is_empty() {
        for shape in shapes {
            let shape = super::moved(shape, shift);
            if !covered(&shape) {
                laid.shapes.push(shape);
            }
        }
    } else {
        for (first, last, top, bottom) in &blocks {
            if whole(*top, *bottom) {
                for shape in &shapes[*first..*last] {
                    let shape = super::moved(shape.clone(), shift);
                    if !covered(&shape) {
                        laid.shapes.push(shape);
                    }
                }
            }
        }
    }
    laid.overlay.extend(fixed);
    if let Some((index, rect, entries, row)) = menu {
        let w = rect.width;
        laid.overlay.push(Shape::Box {
            rect,
            radius: 9.0,
            fill: hex(0x1F2536),
            rim: Some(alpha(0xFFFFFF, 0.16)),
            dashed: false,
            sharp: Corner::None,
        });
        for (n, (target, label, danger)) in entries.into_iter().enumerate() {
            let r = Rectangle::new(
                Point::new(rect.x + 4.0, rect.y + 4.0 + n as f32 * row),
                Size::new(w - 8.0, row),
            );
            let hover = open.hover == Some(target);
            if hover {
                laid.overlay.push(Shape::Box {
                    rect: r,
                    radius: 6.0,
                    fill: alpha(0xFFFFFF, 0.08),
                    rim: None,
                    dashed: false,
                    sharp: Corner::None,
                });
            }
            laid.overlay.push(text(
                &label,
                Point::new(r.x + 8.0, r.center_y()),
                ACT + 1.0,
                if danger { hex(0xF7A77A) } else { TEXT },
                false,
                Alignment::Left,
            ));
            laid.targets.push((target, r));
        }
        said.push(format!("menu:{index}"));
    }
    if let Some(rect) = erase_menu {
        laid.overlay.push(Shape::Box {
            rect,
            radius: 9.0,
            fill: hex(0x1F2536),
            rim: Some(alpha(0xF2945B, 0.4)),
            dashed: false,
            sharp: Corner::None,
        });
        for (n, (target, label)) in erase_entries.into_iter().enumerate() {
            let r = Rectangle::new(
                Point::new(rect.x + 4.0, rect.y + 4.0 + n as f32 * 26.0),
                Size::new(rect.width - 8.0, 26.0),
            );
            if open.hover == Some(target) {
                laid.overlay.push(Shape::Box {
                    rect: r,
                    radius: 6.0,
                    fill: alpha(0xF2945B, 0.12),
                    rim: None,
                    dashed: false,
                    sharp: Corner::None,
                });
            }
            laid.overlay.push(text(
                label,
                Point::new(r.x + 10.0, r.center_y()),
                ACT + 1.0,
                hex(0xF7A77A),
                false,
                Alignment::Left,
            ));
            laid.targets.push((target, r));
        }
        said.push("erase".to_owned());
    }
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
    use hocus_secretary_link::knows::Act;
    let x = PAD;
    let w = right - PAD;
    // Buttons only where the user decides (#758): what is to confirm.
    // The rest has « ··· » on hover, its menu.
    let buttons: Vec<Act> = if known.proposal {
        known
            .actions
            .iter()
            .copied()
            .filter(|a| matches!(a, Act::Accept | Act::Reject))
            .collect()
    } else {
        Vec::new()
    };
    let acts: Vec<(String, f32)> = buttons
        .iter()
        .map(|a| {
            let label = words.act(*a, &known.item).to_owned();
            let lw = width_of(&label, ACT, true) + 16.0;
            (label, lw)
        })
        .collect();
    let dots = !known.proposal && !known.actions.is_empty();
    let acts_w: f32 = if dots {
        26.0
    } else {
        acts.iter().map(|(_, w)| *w).sum::<f32>() + 4.0 * acts.len().saturating_sub(1) as f32
    };
    let room = w
        - 16.0
        - if acts.is_empty() && !dots {
            0.0
        } else {
            acts_w + 10.0
        };
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
    let open_word = words.pick("Ouvrir ↗", "Open ↗");
    let origin_lines = lines_of(
        &known.origin.text,
        META,
        false,
        room - if link {
            width_of(open_word, META, true) + 16.0
        } else {
            0.0
        },
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
        let sep = width_of(" · ", META, false);
        parts.push(text(
            " · ",
            Point::new(last_end, at - META_LINE / 2.0),
            META,
            LABEL,
            false,
            Alignment::Left,
        ));
        let ow = width_of(open_word, META, true);
        let rect = Rectangle::new(
            Point::new(last_end + sep - 2.0, at - META_LINE),
            Size::new(ow + 4.0, META_LINE),
        );
        parts.push(text(
            open_word,
            Point::new(last_end + sep, rect.center_y()),
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
    // Its buttons: the primary first, green.
    let mut bx = x + w - 8.0 - acts_w;
    let mut acts_bottom = y + 7.0;
    for (n, (act, (label, lw))) in buttons.iter().zip(&acts).enumerate() {
        let rect = Rectangle::new(Point::new(bx, y + 6.0), Size::new(*lw, ACT_H + 2.0));
        let target = Target::Known { index, act: *act };
        let hover = open.hover == Some(target);
        let primary = n == 0;
        parts.push(Shape::Box {
            rect,
            radius: 7.0,
            fill: match (primary, hover) {
                (true, false) => hex(0x6CC287),
                (true, true) => hex(0x7ED198),
                (false, false) => alpha(0xFFFFFF, 0.07),
                (false, true) => alpha(0xFFFFFF, 0.12),
            },
            rim: (!primary).then_some(alpha(0xFFFFFF, 0.14)),
            dashed: false,
            sharp: Corner::None,
        });
        parts.push(text(
            label,
            rect.center(),
            ACT,
            if primary { INK } else { hex(0xD5DBE5) },
            true,
            Alignment::Center,
        ));
        targets.push((target, rect));
        bx += lw + 4.0;
        acts_bottom = rect.y + rect.height;
    }
    // « ··· », shown on hover and while its menu is open.
    if dots {
        let target = Target::KnownMenu(index);
        let rect = Rectangle::new(
            Point::new(x + w - 8.0 - 26.0, y + 6.0),
            Size::new(26.0, 20.0),
        );
        let shown = matches!(open.hover, Some(Target::Expand(i) | Target::KnownMenu(i)) if i == index)
            || open.known_menu == Some(index);
        if shown {
            parts.push(Shape::Box {
                rect,
                radius: 6.0,
                fill: alpha(
                    0xFFFFFF,
                    if open.hover == Some(target) || open.known_menu == Some(index) {
                        0.14
                    } else {
                        0.07
                    },
                ),
                rim: None,
                dashed: false,
                sharp: Corner::None,
            });
            parts.push(text(
                "···",
                rect.center(),
                12.0,
                TEXT,
                true,
                Alignment::Center,
            ));
        }
        targets.push((target, rect));
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
            alpha(0x8FB3FF, 0.4)
        } else if known.left {
            alpha(0xFFFFFF, 0.14)
        } else {
            alpha(0xFFFFFF, 0.05)
        }),
        dashed: known.left,
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
