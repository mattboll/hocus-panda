//! The Matrix theme's brush draws hundreds of glyphs and plain colours a
//! frame. Made into elements of their own, each with a new id every frame,
//! they made every redraw repaint the whole screen, the lock's still rain
//! included (#221).
//!
//! What a brush draws is one element instead, kept from frame to frame: the
//! same id, a commit that moves only when what it draws changes, and as its
//! damage only the glyphs and colours that came, went or changed. One
//! element also keeps the scene's order still while drops come and go, so
//! that what is behind the rain (its black, the desktop) is not repainted
//! for it.

use std::{collections::HashMap, panic::Location, sync::Mutex};

use smithay::{
    backend::renderer::{
        Color32F, ImportMem, Renderer,
        element::{
            Element, Id, Kind, RenderElement, memory::MemoryRenderBufferRenderElement,
            solid::SolidColorRenderElement,
        },
        utils::{CommitCounter, DamageBag, DamageSet, DamageSnapshot, OpaqueRegions},
    },
    utils::{Buffer, Physical, Rectangle, Scale},
};

/// A layer not seen for this many scenes is forgotten: back later, it is
/// new, and repainted whole.
const KEEP: u64 = 600;

/// One thing a brush draws: part of the sheet of glyphs, or a colour.
// Most are glyphs: boxed, each would cost an allocation.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub(crate) enum Stroke<R: Renderer> {
    Glyphs(MemoryRenderBufferRenderElement<R>),
    Solid(SolidColorRenderElement),
}

impl<R> Stroke<R>
where
    R: Renderer + ImportMem,
    R::TextureId: 'static,
{
    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        match self {
            Self::Glyphs(e) => e.geometry(scale),
            Self::Solid(e) => e.geometry(scale),
        }
    }

    /// What it paints where: two strokes with the same mark paint the
    /// same pixels.
    fn mark(&self, scale: Scale<f64>) -> Mark {
        let g = self.geometry(scale);
        let (paint, src) = match self {
            Self::Glyphs(e) => {
                let s = e.src();
                (
                    [e.alpha().to_bits(), 0, 0, 0],
                    [s.loc.x, s.loc.y, s.size.w, s.size.h].map(f64::to_bits),
                )
            }
            Self::Solid(e) => (e.color().components().map(f32::to_bits), NO_SRC),
        };
        Mark {
            at: [g.loc.x, g.loc.y, g.size.w, g.size.h],
            src,
            paint,
        }
    }
}

/// A stroke's place, its part of the sheet (none for a colour) and its
/// colour or strength, as bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Mark {
    at: [i32; 4],
    src: [u64; 4],
    paint: [u32; 4],
}

/// A plain colour's mark has no part of the sheet.
const NO_SRC: [u64; 4] = [u64::MAX; 4];

impl Mark {
    fn rect(&self) -> Rectangle<i32, Physical> {
        Rectangle::new(
            (self.at[0], self.at[1]).into(),
            (self.at[2], self.at[3]).into(),
        )
    }

    fn is_solid(&self) -> bool {
        self.src == NO_SRC
    }

    /// What it paints, wherever it is.
    fn paint(&self) -> Mark {
        Mark {
            at: [0; 4],
            ..*self
        }
    }
}

/// Where the pixels of `new` may differ from those of `old`, both front
/// first: the strokes that went or came.
///
/// A colour that only grew or shrank (the black behind a cascading rain,
/// a bar filling) damages what it gained or lost, when the colours that
/// changed are the same ones, in the same order among what stayed: then
/// each still paints, where it was and is, what it painted. Strokes in
/// both must keep their order too, or overlapping ones would paint over
/// each other the other way round; when they do not, all of both.
fn changed(old: &[Mark], new: &[Mark]) -> Vec<Rectangle<i32, Physical>> {
    let mut count: HashMap<Mark, (usize, usize)> = HashMap::with_capacity(old.len());
    for mark in old {
        count.entry(*mark).or_default().0 += 1;
    }
    for mark in new {
        count.entry(*mark).or_default().1 += 1;
    }
    // Which strokes are in both, the first ones of a kind first.
    let stayed = |marks: &[Mark]| {
        let mut left: HashMap<Mark, usize> =
            count.iter().map(|(m, (o, n))| (*m, *o.min(n))).collect();
        marks
            .iter()
            .map(|m| match left.get_mut(m) {
                Some(n) if *n > 0 => {
                    *n -= 1;
                    true
                }
                _ => false,
            })
            .collect::<Vec<bool>>()
    };
    let (old_stayed, new_stayed) = (stayed(old), stayed(new));
    let order = |marks: &[Mark], stayed: &[bool]| {
        marks
            .iter()
            .zip(stayed)
            .filter_map(|(m, stayed)| match (stayed, m.is_solid()) {
                (true, _) => Some((true, *m)),
                (false, true) => Some((false, m.paint())),
                (false, false) => None,
            })
            .collect::<Vec<_>>()
    };
    let moved = |marks: &[Mark], stayed: &[bool], solid: bool| {
        marks
            .iter()
            .zip(stayed)
            .filter(move |(m, stayed)| !**stayed && m.is_solid() == solid)
            .map(|(m, _)| m.rect())
            .collect::<Vec<_>>()
    };
    if order(old, &old_stayed) == order(new, &new_stayed) {
        let mut damage = moved(old, &old_stayed, false);
        damage.extend(moved(new, &new_stayed, false));
        let colours = moved(old, &old_stayed, true)
            .into_iter()
            .zip(moved(new, &new_stayed, true));
        for (was, is) in colours {
            damage.extend(Rectangle::subtract_rects_many([was], [is]));
            damage.extend(Rectangle::subtract_rects_many([is], [was]));
        }
        return damage;
    }
    let kept = |marks: &[Mark], stayed: &[bool]| {
        marks
            .iter()
            .zip(stayed)
            .filter(|(_, stayed)| **stayed)
            .map(|(m, _)| *m)
            .collect::<Vec<_>>()
    };
    if kept(old, &old_stayed) != kept(new, &new_stayed) {
        return old.iter().chain(new).map(Mark::rect).collect();
    }
    let mut damage = moved(old, &old_stayed, false);
    damage.extend(moved(new, &new_stayed, false));
    damage.extend(moved(old, &old_stayed, true));
    damage.extend(moved(new, &new_stayed, true));
    damage
}

/// What one brush drew in the scenes built before.
#[derive(Debug)]
struct Kept {
    id: Id,
    damage: DamageBag<i32, Physical>,
    marks: Vec<Mark>,
    /// The scene it was last in.
    seen: u64,
}

/// A layer's place in the scenes: the output, its scale, where in the code
/// it is taken, and how many were taken there before it in this scene.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    output: String,
    scale: u32,
    at: &'static Location<'static>,
    nth: u32,
}

#[derive(Debug, Default)]
struct Store {
    /// The output whose scene is being built.
    output: String,
    /// Scenes built so far.
    built: u64,
    /// Layers taken in this scene, by scale and place in the code.
    taken: HashMap<(u32, &'static Location<'static>), u32>,
    kept: HashMap<Key, Kept>,
}

/// The brushes' layers, kept from frame to frame.
#[derive(Debug)]
pub struct Layers {
    store: Mutex<Store>,
    /// The strokes' own id: never seen by a renderer, their layer's is.
    stroke: Id,
}

impl Default for Layers {
    fn default() -> Self {
        Self {
            store: Mutex::default(),
            stroke: Id::new(),
        }
    }
}

impl Layers {
    /// A scene of `output` is being built: its layers count from the first.
    pub(crate) fn begin(&self, output: &str) {
        let Ok(mut store) = self.store.lock() else {
            return;
        };
        store.built += 1;
        if store.output != output {
            store.output = output.to_string();
        }
        store.taken.clear();
        let built = store.built;
        store.kept.retain(|_, kept| built - kept.seen <= KEEP);
    }

    pub(crate) fn stroke_id(&self) -> Id {
        self.stroke.clone()
    }

    /// The id and damage of the layer taken at `at`, now drawing `marks`.
    fn keep(
        &self,
        scale: Scale<f64>,
        at: &'static Location<'static>,
        marks: Vec<Mark>,
    ) -> (Id, DamageSnapshot<i32, Physical>) {
        let Ok(mut store) = self.store.lock() else {
            // Unknown, as a new layer: repainted whole.
            return (Id::new(), DamageSnapshot::empty());
        };
        let scale = (scale.x * 1000.0).round() as u32;
        let nth = {
            let n = store.taken.entry((scale, at)).or_default();
            *n += 1;
            *n - 1
        };
        let key = Key {
            output: store.output.clone(),
            scale,
            at,
            nth,
        };
        let built = store.built;
        let kept = store.kept.entry(key).or_insert_with(|| Kept {
            id: Id::new(),
            damage: DamageBag::default(),
            marks: Vec::new(),
            seen: built,
        });
        kept.seen = built;
        if kept.marks != marks {
            kept.damage.add(changed(&kept.marks, &marks));
            kept.marks = marks;
        }
        (kept.id.clone(), kept.damage.snapshot())
    }

    /// A plain colour of its own, `at` in the code: the same id from frame
    /// to frame, its commit moving when its colour does.
    pub(crate) fn solid(
        &self,
        at: &'static Location<'static>,
        geometry: Rectangle<i32, Physical>,
        color: Color32F,
        scale: f64,
    ) -> SolidColorRenderElement {
        let mark = Mark {
            at: [0, 0, 0, 0],
            src: NO_SRC,
            paint: color.components().map(f32::to_bits),
        };
        let (id, damage) = self.keep(Scale::from(scale), at, vec![mark]);
        SolidColorRenderElement::new(
            id,
            geometry,
            damage.current_commit(),
            color,
            Kind::Unspecified,
        )
    }

    /// One element for `strokes` (front first), drawn at `scale` in
    /// `extent` at least, taken `at` in the code; none for no strokes.
    pub(crate) fn layer<R>(
        &self,
        at: &'static Location<'static>,
        strokes: Vec<Stroke<R>>,
        extent: Option<Rectangle<i32, Physical>>,
        scale: f64,
    ) -> Option<LayerElement<R>>
    where
        R: Renderer + ImportMem,
        R::TextureId: 'static,
    {
        let scale = Scale::from(scale);
        let marks: Vec<Mark> = strokes.iter().map(|s| s.mark(scale)).collect();
        let (id, damage) = self.keep(scale, at, marks);
        let geometry = strokes
            .iter()
            .map(|s| s.geometry(scale))
            .reduce(|a, b| a.merge(b))?;
        // Where the effect draws, so that the element keeps its place as
        // what it covers grows (a cascade, rows rushing out).
        let geometry = extent.map_or(geometry, |e| geometry.merge(e));
        let opaque = strokes
            .iter()
            .flat_map(|s| match s {
                Stroke::Solid(e) => {
                    let g = e.geometry(scale);
                    e.opaque_regions(scale)
                        .iter()
                        .map(|r| {
                            let mut r = *r;
                            r.loc += g.loc - geometry.loc;
                            r
                        })
                        .collect::<Vec<_>>()
                }
                Stroke::Glyphs(_) => Vec::new(),
            })
            .collect();
        Some(LayerElement {
            id,
            damage,
            strokes,
            geometry,
            opaque,
            scale,
        })
    }
}

/// What one brush drew, as one element.
#[derive(Debug)]
pub struct LayerElement<R: Renderer> {
    id: Id,
    damage: DamageSnapshot<i32, Physical>,
    /// Front first.
    strokes: Vec<Stroke<R>>,
    /// Around all of them, output-local.
    geometry: Rectangle<i32, Physical>,
    /// Where it hides what is behind, relative to `geometry`.
    opaque: Vec<Rectangle<i32, Physical>>,
    scale: Scale<f64>,
}

impl<R: Renderer> LayerElement<R> {
    /// Where a stroke at `natural` goes when the layer is drawn in `dst`
    /// (larger or smaller when magnified).
    fn place(
        &self,
        natural: Rectangle<i32, Physical>,
        dst: Rectangle<i32, Physical>,
    ) -> Rectangle<i32, Physical> {
        if dst == self.geometry {
            return natural;
        }
        let g = self.geometry;
        let kx = f64::from(dst.size.w) / f64::from(g.size.w.max(1));
        let ky = f64::from(dst.size.h) / f64::from(g.size.h.max(1));
        let x = |v: i32| dst.loc.x + (f64::from(v - g.loc.x) * kx).round() as i32;
        let y = |v: i32| dst.loc.y + (f64::from(v - g.loc.y) * ky).round() as i32;
        let (x0, y0) = (x(natural.loc.x), y(natural.loc.y));
        let (x1, y1) = (
            x(natural.loc.x + natural.size.w),
            y(natural.loc.y + natural.size.h),
        );
        Rectangle::new((x0, y0).into(), (x1 - x0, y1 - y0).into())
    }
}

impl<R: Renderer> Element for LayerElement<R> {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.damage.current_commit()
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        Rectangle::from_size(
            (
                f64::from(self.geometry.size.w),
                f64::from(self.geometry.size.h),
            )
                .into(),
        )
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.geometry
    }

    fn damage_since(
        &self,
        _scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        match self.damage.damage_since(commit) {
            Some(damage) => damage
                .into_iter()
                .map(|mut r| {
                    r.loc -= self.geometry.loc;
                    r
                })
                .collect(),
            None => DamageSet::from_slice(&[Rectangle::from_size(self.geometry.size)]),
        }
    }

    fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        OpaqueRegions::from_slice(&self.opaque)
    }

    fn kind(&self) -> Kind {
        Kind::Unspecified
    }
}

impl<R> RenderElement<R> for LayerElement<R>
where
    R: Renderer + ImportMem,
    R::TextureId: 'static,
{
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        _src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        // Back to front, each as it would be drawn alone: its damage
        // relative to itself, nothing of it opaque (a colour ignores it).
        let mut local = Vec::new();
        for stroke in self.strokes.iter().rev() {
            let at = self.place(stroke.geometry(self.scale), dst);
            local.clear();
            local.extend(damage.iter().filter_map(|d| {
                let d = Rectangle::new(d.loc + dst.loc, d.size);
                d.intersection(at).map(|mut d| {
                    d.loc -= at.loc;
                    d
                })
            }));
            if local.is_empty() {
                continue;
            }
            match stroke {
                Stroke::Glyphs(e) => RenderElement::<R>::draw(e, frame, e.src(), at, &local, &[])?,
                Stroke::Solid(e) => RenderElement::<R>::draw(e, frame, e.src(), at, &local, &[])?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(x: i32, alpha: f32) -> Mark {
        Mark {
            at: [x, 0, 8, 14],
            src: [0; 4],
            paint: [alpha.to_bits(), 0, 0, 0],
        }
    }

    #[test]
    fn the_same_strokes_damage_nothing() {
        let marks = [mark(0, 1.0), mark(16, 0.5)];
        assert!(changed(&marks, &marks).is_empty());
    }

    #[test]
    fn only_strokes_that_came_went_or_changed_are_damaged() {
        let old = [mark(0, 1.0), mark(16, 0.5), mark(32, 1.0)];
        let new = [mark(0, 1.0), mark(16, 0.4), mark(48, 1.0)];
        let mut damage: Vec<i32> = changed(&old, &new).iter().map(|r| r.loc.x).collect();
        damage.sort();
        assert_eq!(damage, [16, 16, 32, 48]);
    }

    fn colour(y: i32, h: i32) -> Mark {
        Mark {
            at: [0, y, 16, h],
            src: NO_SRC,
            paint: [0, 0, 0, 1.0f32.to_bits()],
        }
    }

    #[test]
    fn a_colour_that_grows_damages_what_it_gained() {
        let old = [mark(0, 1.0), colour(0, 100)];
        let new = [mark(0, 1.0), colour(0, 110)];
        let damage = changed(&old, &new);
        assert_eq!(damage, [Rectangle::new((0, 100).into(), (16, 10).into())]);
    }

    #[test]
    fn a_colour_that_moves_past_a_glyph_damages_both_whole() {
        let old = [mark(0, 1.0), colour(0, 100)];
        let new = [colour(0, 110), mark(0, 1.0)];
        assert_eq!(
            changed(&old, &new),
            [colour(0, 100).rect(), colour(0, 110).rect()]
        );
    }

    #[test]
    fn strokes_in_a_new_order_damage_all() {
        let old = [mark(0, 1.0), mark(4, 0.5)];
        let new = [mark(4, 0.5), mark(0, 1.0)];
        assert_eq!(changed(&old, &new).len(), 4);
    }
}
