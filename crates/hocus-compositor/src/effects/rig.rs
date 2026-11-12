//! A creature's drawing in pieces (#796): the validated drawing of its
//! mood (`hocus_pandas`) cut where the parts its motion moves begin and
//! end, each piece drawn once with resvg into a sheet (a texture), then
//! every frame each piece is laid on a quad of its own, moved as its
//! parts are. Nothing is drawn again on the CPU while it moves: a frame
//! is a few quads.
//!
//! The pieces, in the drawing's order: the runs of what does not move
//! between the moving parts, and inside each part what it holds, the
//! same way down. A piece is drawn as the drawing places it (its groups'
//! transforms), at rest; a frame moves it from there by its parts' poses,
//! the outer ones first, as the SVG would. At rest the pieces laid
//! together are the drawing (the bar's panda), pixel for pixel: each
//! piece's place in its sheet is on whole pixels as on the screen.

use hocus_pandas::{Mood, creature};
use resvg::{
    tiny_skia::{self, Transform},
    usvg,
};

/// The drawings' box, `-3 -3.5 126 120.5`: the canvas the pieces are
/// placed on, in its units.
pub const BOX: (f32, f32) = hocus_pandas::VIEW_BOX;

/// Pixels around each piece in its sheet, so that none bleeds into the
/// next as it is sampled.
const PAD: u32 = 2;

/// A mood's drawing in pieces.
pub struct Rig {
    tree: usvg::Tree,
    parts: Vec<Part>,
    pieces: Vec<Piece>,
}

impl std::fmt::Debug for Rig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rig")
            .field("parts", &self.parts.len())
            .field("pieces", &self.pieces.len())
            .finish_non_exhaustive()
    }
}

/// A moving part: its group's id, the part holding it, where its group's
/// space is on the canvas.
#[derive(Debug)]
struct Part {
    id: String,
    parent: Option<usize>,
    abs: Transform,
}

/// A piece: the part it is in (none: the drawing itself), where its
/// nodes' parent's space is on the canvas, its nodes (their places in the
/// tree), and its rectangle on the canvas (all it covers, and a unit).
#[derive(Debug)]
struct Piece {
    part: Option<usize>,
    parent_abs: Transform,
    nodes: Vec<Vec<usize>>,
    cell: tiny_skia::Rect,
}

/// The pieces drawn at a size: the pixels (premultiplied RGBA), their
/// size, and each piece's place there, `[x, y]` of the canvas's corner
/// (in sheet pixels; a canvas unit is `k` pixels).
#[derive(Debug)]
pub struct Sheet {
    pub rgba: Vec<u8>,
    pub size: (u32, u32),
    pub k: f32,
    origins: Vec<(f32, f32)>,
}

/// A piece laid this frame: its quad's corners on the canvas (top left,
/// top right, bottom left, bottom right), where they are in the sheet (0
/// to 1), how opaque.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Laid {
    pub corners: [(f32, f32); 4],
    pub uv: [(f32, f32); 4],
    pub alpha: f32,
}

impl Rig {
    /// `mood`'s drawing cut where its creature's moving parts are.
    pub fn new(mood: Mood) -> Option<Self> {
        let tree = usvg::Tree::from_str(&wrapped(mood.still()), &usvg::Options::default())
            .map_err(|err| tracing::warn!(%err, mood = mood.name(), "a creature's drawing"))
            .ok()?;
        let moving = creature::parts(mood);
        let mut parts = Vec::new();
        let mut pieces = Vec::new();
        cut(
            tree.root(),
            &mut Vec::new(),
            None,
            moving,
            &mut parts,
            &mut pieces,
        );
        Some(Self {
            tree,
            parts,
            pieces,
        })
    }

    /// The pieces drawn at `k` pixels a canvas unit.
    pub fn sheet(&self, k: f32) -> Option<Sheet> {
        let k = k.max(0.05);
        // Each piece's pixels: from the whole pixel at or before its
        // cell's corner, so that the canvas falls on whole pixels there as
        // on the screen.
        let spans: Vec<(i32, i32, u32, u32)> = self
            .pieces
            .iter()
            .map(|p| {
                let (x0, y0) = (
                    (p.cell.left() * k).floor() as i32,
                    (p.cell.top() * k).floor() as i32,
                );
                let (x1, y1) = (
                    (p.cell.right() * k).ceil() as i32,
                    (p.cell.bottom() * k).ceil() as i32,
                );
                (x0, y0, (x1 - x0).max(1) as u32, (y1 - y0).max(1) as u32)
            })
            .collect();
        let width = spans.iter().map(|s| s.2).max().unwrap_or(1) + 2 * PAD;
        let height: u32 = spans.iter().map(|s| s.3 + 2 * PAD).sum::<u32>().max(1);
        let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
        let mut origins = Vec::with_capacity(self.pieces.len());
        let mut top = 0u32;
        for (piece, (x0, y0, _, h)) in self.pieces.iter().zip(&spans) {
            // The canvas's corner in the sheet.
            let ox = PAD as f32 - *x0 as f32;
            let oy = (top + PAD) as f32 - *y0 as f32;
            origins.push((ox, oy));
            let place = Transform::from_row(k, 0.0, 0.0, k, ox, oy);
            for path in &piece.nodes {
                let Some(node) = node_at(self.tree.root(), path) else {
                    continue;
                };
                let Some(bounds) = node.abs_layer_bounding_box() else {
                    continue;
                };
                // resvg moves a node by its bounds' corner first: undone.
                let at = place
                    .pre_concat(piece.parent_abs)
                    .pre_translate(bounds.x(), bounds.y());
                resvg::render_node(node, at, &mut pixmap.as_mut());
            }
            top += h + 2 * PAD;
        }
        Some(Sheet {
            size: (width, height),
            rgba: pixmap.take(),
            k,
            origins,
        })
    }

    /// The pieces laid at `look` (none: at rest), in `sheet`.
    pub fn lay(&self, look: Option<&creature::Look>, sheet: &Sheet) -> Vec<Laid> {
        // Each part's move on the canvas, and its opacity, outer first.
        let mut moves: Vec<(Transform, f32)> = Vec::with_capacity(self.parts.len());
        for part in &self.parts {
            let (outer, outer_alpha) = part
                .parent
                .and_then(|p| moves.get(p).copied())
                .unwrap_or((Transform::identity(), 1.0));
            let moved = look.and_then(|l| l.parts.iter().find(|m| m.id == part.id));
            let (pose, alpha) = match moved {
                Some(m) => {
                    let [a, b, c, d, e, f] = m.pose.affine(m.origin);
                    (Transform::from_row(a, b, c, d, e, f), m.pose.opacity)
                }
                None => (Transform::identity(), 1.0),
            };
            let back = part.abs.invert().unwrap_or_default();
            moves.push((
                outer.pre_concat(part.abs).pre_concat(pose).pre_concat(back),
                outer_alpha * alpha.clamp(0.0, 1.0),
            ));
        }
        let (w, h) = (sheet.size.0 as f32, sheet.size.1 as f32);
        self.pieces
            .iter()
            .zip(&sheet.origins)
            .filter_map(|(piece, (ox, oy))| {
                let (moved, alpha) = piece
                    .part
                    .and_then(|p| moves.get(p).copied())
                    .unwrap_or((Transform::identity(), 1.0));
                if alpha <= 0.0 {
                    return None;
                }
                let c = piece.cell;
                let canvas = [
                    (c.left(), c.top()),
                    (c.right(), c.top()),
                    (c.left(), c.bottom()),
                    (c.right(), c.bottom()),
                ];
                let corners = canvas.map(|(x, y)| {
                    let mut p = tiny_skia::Point::from_xy(x, y);
                    moved.map_point(&mut p);
                    (p.x, p.y)
                });
                let uv = canvas.map(|(x, y)| ((ox + x * sheet.k) / w, (oy + y * sheet.k) / h));
                Some(Laid { corners, uv, alpha })
            })
            .collect()
    }
}

/// Cuts `group`'s children (at `path` in the tree, in part `part`) into
/// pieces, going into the moving parts and into the groups holding one.
fn cut(
    group: &usvg::Group,
    path: &mut Vec<usize>,
    part: Option<usize>,
    moving: &[&str],
    parts: &mut Vec<Part>,
    pieces: &mut Vec<Piece>,
) {
    let mut run: Vec<Vec<usize>> = Vec::new();
    let mut cell: Option<tiny_skia::Rect> = None;
    let flush =
        |run: &mut Vec<Vec<usize>>, cell: &mut Option<tiny_skia::Rect>, pieces: &mut Vec<Piece>| {
            if let Some(c) = cell.take()
                && !run.is_empty()
            {
                pieces.push(Piece {
                    part,
                    parent_abs: group.abs_transform(),
                    nodes: std::mem::take(run),
                    cell: tiny_skia::Rect::from_ltrb(
                        c.left() - 1.0,
                        c.top() - 1.0,
                        c.right() + 1.0,
                        c.bottom() + 1.0,
                    )
                    .unwrap_or(c),
                });
            }
            run.clear();
        };
    for (i, child) in group.children().iter().enumerate() {
        path.push(i);
        match child {
            usvg::Node::Group(g) if moving.contains(&g.id()) => {
                flush(&mut run, &mut cell, pieces);
                parts.push(Part {
                    id: g.id().to_owned(),
                    parent: part,
                    abs: g.abs_transform(),
                });
                let index = parts.len() - 1;
                cut(g, path, Some(index), moving, parts, pieces);
            }
            usvg::Node::Group(g) if holds(g, moving) => {
                flush(&mut run, &mut cell, pieces);
                cut(g, path, part, moving, parts, pieces);
            }
            node => {
                if let Some(b) = node.abs_layer_bounding_box() {
                    let b = b.to_rect();
                    cell = Some(match cell {
                        Some(c) => tiny_skia::Rect::from_ltrb(
                            c.left().min(b.left()),
                            c.top().min(b.top()),
                            c.right().max(b.right()),
                            c.bottom().max(b.bottom()),
                        )
                        .unwrap_or(c),
                        None => b,
                    });
                    run.push(path.clone());
                }
            }
        }
        path.pop();
    }
    flush(&mut run, &mut cell, pieces);
}

/// `drawing` with each shape in a group of its own: resvg draws a node
/// alone through its bounds, which a group's include its stroke (a line
/// has none of its own). Nothing it shows changes.
fn wrapped(drawing: &str) -> String {
    const SHAPES: [&str; 6] = ["<path", "<circle", "<ellipse", "<rect", "<line", "<polygon"];
    let mut out = String::with_capacity(drawing.len() * 2);
    let mut rest = drawing;
    while let Some(at) = rest.find('<') {
        let (before, from) = rest.split_at(at);
        out.push_str(before);
        let shape = SHAPES.iter().any(|s| {
            from.starts_with(s)
                && from
                    .as_bytes()
                    .get(s.len())
                    .is_some_and(|c| c.is_ascii_whitespace())
        });
        let end = from.find('>').map_or(from.len(), |e| e + 1);
        let (tag, after) = from.split_at(end);
        if shape && tag.ends_with("/>") {
            out.push_str("<g>");
            out.push_str(tag);
            out.push_str("</g>");
        } else {
            out.push_str(tag);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Whether `group` holds one of the `moving` parts.
fn holds(group: &usvg::Group, moving: &[&str]) -> bool {
    group.children().iter().any(|child| match child {
        usvg::Node::Group(g) => moving.contains(&g.id()) || holds(g, moving),
        _ => false,
    })
}

/// The node at `path` (each child's index, from the root).
fn node_at<'a>(root: &'a usvg::Group, path: &[usize]) -> Option<&'a usvg::Node> {
    let (last, above) = path.split_last()?;
    let mut group = root;
    for i in above {
        match group.children().get(*i)? {
            usvg::Node::Group(g) => group = g,
            _ => return None,
        }
    }
    group.children().get(*last)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pieces laid at rest, drawn back to back as the screen would
    /// (nearest pixel, as they fall on whole pixels): the drawing.
    fn at_rest(mood: Mood, k: f32) -> (Vec<u8>, Vec<u8>, u32, u32) {
        let rig = Rig::new(mood).expect("the drawing reads");
        let sheet = rig.sheet(k).expect("drawn");
        let laid = rig.lay(None, &sheet);
        let (w, h) = ((BOX.0 * k).ceil() as u32, (BOX.1 * k).ceil() as u32);
        let mut out = vec![0u8; (w * h * 4) as usize];
        for piece in &laid {
            let [(x0, y0), _, _, (x1, y1)] = piece.corners;
            let [(u0, v0), _, _, (u1, v1)] = piece.uv;
            for y in (y0 * k).round() as i32..(y1 * k).round() as i32 {
                for x in (x0 * k).round() as i32..(x1 * k).round() as i32 {
                    if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                        continue;
                    }
                    let su = u0 + (u1 - u0) * ((x as f32 + 0.5) / k - x0) / (x1 - x0);
                    let sv = v0 + (v1 - v0) * ((y as f32 + 0.5) / k - y0) / (y1 - y0);
                    let sx = (su * sheet.size.0 as f32).floor() as usize;
                    let sy = (sv * sheet.size.1 as f32).floor() as usize;
                    let from = (sy * sheet.size.0 as usize + sx) * 4;
                    let to = ((y as u32 * w + x as u32) * 4) as usize;
                    let src = &sheet.rgba[from..from + 4];
                    let a = u32::from(src[3]);
                    for c in 0..4 {
                        let dst = u32::from(out[to + c]);
                        out[to + c] = (u32::from(src[c]) + dst * (255 - a) / 255).min(255) as u8;
                    }
                }
            }
        }
        let tree = usvg::Tree::from_str(mood.still(), &usvg::Options::default()).expect("reads");
        let mut whole = tiny_skia::Pixmap::new(w, h).expect("a pixmap");
        resvg::render(&tree, Transform::from_scale(k, k), &mut whole.as_mut());
        (out, whole.take(), w, h)
    }

    #[test]
    fn at_rest_the_pieces_are_the_drawing() {
        for mood in [Mood::Working, Mood::Waiting, Mood::Resting, Mood::Done] {
            let (pieces, whole, w, h) = at_rest(mood, 184.0 / BOX.0);
            let far = pieces
                .iter()
                .zip(&whole)
                .filter(|(a, b)| a.abs_diff(**b) > 3)
                .count();
            // Edges blended in two steps where pieces overlap may round a
            // little apart: a few values in a thousand pixels, no more.
            assert!(
                far * 2000 < (w * h) as usize,
                "{mood:?}: {far} of {} values apart",
                w * h * 4
            );
        }
    }

    #[test]
    fn each_shape_goes_in_a_group_of_its_own() {
        assert_eq!(
            wrapped("<svg><!-- a --><g id=\"x\"><path d=\"M0 0\" /></g><rect x=\"1\"/></svg>"),
            "<svg><!-- a --><g id=\"x\"><g><path d=\"M0 0\" /></g></g><g><rect x=\"1\"/></g></svg>"
        );
    }

    #[test]
    fn a_moved_part_moves_its_pieces_alone() {
        let rig = Rig::new(Mood::Working).expect("reads");
        let sheet = rig.sheet(1.0).expect("drawn");
        let rest = rig.lay(None, &sheet);
        let look = creature::look(Mood::Working, 0.1);
        let moved = rig.lay(Some(&look), &sheet);
        assert_eq!(rest.len(), moved.len());
        let changed = rest
            .iter()
            .zip(&moved)
            .filter(|(a, b)| a.corners != b.corners)
            .count();
        // The left paw is up (and the hat's star turns), not the laptop.
        assert!(
            changed >= 2 && changed < rest.len(),
            "{changed} of {}",
            rest.len()
        );
    }
}
