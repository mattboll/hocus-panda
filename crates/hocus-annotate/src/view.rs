//! The annotation window's content: the tools and colours above, the
//! capture below, fitted. A drag draws an arrow, a frame or a blurred area;
//! a click with the text tool starts a text, typed until Enter. Undo takes
//! back the last mark; Copy and Save make the picture.

use std::{collections::HashMap, time::Duration};

use hocus_shell::Lang;
use hocus_ui::{
    AccessNode, Align, Button, ButtonKind, Canvas, Color, Direction, Draw, Event, Input, Insets,
    Key, Kind, Length, Look, NodeId, Paint, PixelRect, Point, Rect, Size, Style, Surface, Theme,
    Tree,
};

use crate::scene::{self, COLORS, Picture, Shape, Tool};

/// What the window asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The picture, as PNG, to the clipboard.
    Copy(Vec<u8>),
    /// The picture, as PNG, to a file next to the capture.
    Save(Vec<u8>),
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Tool(Tool),
    Color(usize),
    Undo,
    Copy,
    Save,
}

const BAR: f32 = 64.0;
const MARGIN: f32 = 24.0;

/// A colour's swatch, ringed when chosen.
struct Swatch {
    color: Color,
    chosen: bool,
}

impl Draw for Swatch {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _: Duration) {
        let d = rect.w.min(rect.h);
        let r = Rect::new(
            rect.x + (rect.w - d) / 2.0,
            rect.y + (rect.h - d) / 2.0,
            d,
            d,
        );
        if self.chosen {
            canvas.fill_rect(r, d / 2.0, theme.colors.text);
        }
        let inner = Rect::new(r.x + 3.0, r.y + 3.0, d - 6.0, d - 6.0);
        canvas.fill_rect(inner, inner.w / 2.0, theme.colors.surface);
        let dot = Rect::new(r.x + 5.0, r.y + 5.0, d - 10.0, d - 10.0);
        canvas.fill_rect(dot, dot.w / 2.0, self.color);
    }

    fn size(&self) -> Size {
        Size::new(28.0, 28.0)
    }
}

pub struct AnnotateView {
    lang: Lang,
    tree: Tree,
    surface: Surface,
    picture: Picture,
    /// The capture with its blurred areas, premultiplied, and how many
    /// blurs it was made with.
    base: (Vec<u8>, usize),
    shapes: Vec<Shape>,
    /// A mark being drawn, from where.
    drawing: Option<(Point, Shape)>,
    /// The text being typed: its place in `shapes`.
    typing: Option<usize>,
    tool: Tool,
    color: usize,
    /// Where the capture shows.
    place: Rect,
    targets: HashMap<NodeId, Target>,
    note: Option<String>,
    pointer: Option<Point>,
    dirty: bool,
}

impl std::fmt::Debug for AnnotateView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnnotateView")
            .field("shapes", &self.shapes.len())
            .field("tool", &self.tool)
            .finish_non_exhaustive()
    }
}

impl AnnotateView {
    pub fn new(theme: Theme, lang: Lang, size: Size, scale: f32, picture: Picture) -> Self {
        let base = picture.base(&[]);
        Self {
            lang,
            tree: Tree::new(theme, size, scale),
            surface: Surface::new(size, scale),
            picture,
            base: (base, 0),
            shapes: Vec::new(),
            drawing: None,
            typing: None,
            tool: Tool::Arrow,
            color: 0,
            place: Rect::default(),
            targets: HashMap::new(),
            note: None,
            pointer: None,
            dirty: true,
        }
    }

    pub fn resize(&mut self, size: Size, scale: f32) {
        if size != self.tree.size() || scale != self.tree.scale() {
            self.tree.resize(size, scale);
            self.surface = Surface::new(size, scale);
            self.dirty = true;
        }
    }

    pub fn shapes(&self) -> &[Shape] {
        &self.shapes
    }

    fn build(&mut self) {
        let theme = self.tree.theme().clone();
        let lang = self.lang;
        let size = self.tree.size();
        let missing = || unreachable!("adding to a node of this tree");
        let t = &mut self.tree;
        let root = t.root();
        for child in t.children(root) {
            t.remove(child);
        }
        t.set_look(
            root,
            Look {
                background: Some(Paint::Background),
                ..Look::default()
            },
        );
        let bar = t
            .add(
                root,
                Style {
                    direction: Direction::Row,
                    width: Length::Fill,
                    height: Length::Px(BAR),
                    padding: Insets::symmetric(0.0, 14.0),
                    gap: 6.0,
                    align: Align::Center,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Surface),
                    ..Look::default()
                }),
            )
            .unwrap_or_else(missing);
        let mut targets = Vec::new();
        for (tool, name) in [
            (Tool::Arrow, lang.annotate_arrow()),
            (Tool::Frame, lang.annotate_frame()),
            (Tool::Text, lang.annotate_text()),
            (Tool::Blur, lang.annotate_blur()),
        ] {
            let mut b = Button::new(name, ButtonKind::Chip, &theme);
            b.selected = self.tool == tool;
            let id = t
                .add(
                    bar,
                    Style {
                        height: Length::Px(40.0),
                        padding: Insets::symmetric(0.0, 10.0),
                        ..Style::default()
                    },
                    Kind::Button(b),
                )
                .unwrap_or_else(missing);
            targets.push((id, Target::Tool(tool)));
        }
        t.add(
            bar,
            Style {
                width: Length::Px(12.0),
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )
        .unwrap_or_else(missing);
        for (i, color) in COLORS.into_iter().enumerate() {
            let name = lang.annotate_colour(i);
            let id = t
                .add(
                    bar,
                    Style {
                        width: Length::Px(28.0),
                        height: Length::Px(28.0),
                        ..Style::default()
                    },
                    Kind::Custom {
                        draw: Box::new(Swatch {
                            color,
                            chosen: self.color == i,
                        }),
                        label: Some(name.to_owned()),
                    },
                )
                .unwrap_or_else(missing);
            t.set_clickable(id, name);
            targets.push((id, Target::Color(i)));
        }
        t.add(
            bar,
            Style {
                width: Length::Fill,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )
        .unwrap_or_else(missing);
        if let Some(note) = &self.note {
            t.add(
                bar,
                Style::default(),
                Kind::text_line(note.clone(), theme.text.label.with_color(Paint::Muted)),
            )
            .unwrap_or_else(missing);
        }
        for (target, name, kind) in [
            (Target::Undo, lang.annotate_undo(), ButtonKind::Plain),
            (Target::Copy, lang.annotate_copy(), ButtonKind::Plain),
            (Target::Save, lang.annotate_save(), ButtonKind::Primary),
        ] {
            let mut b = Button::new(name, kind, &theme);
            b.enabled = target != Target::Undo || !self.shapes.is_empty();
            let id = t
                .add(
                    bar,
                    Style {
                        height: Length::Px(40.0),
                        padding: Insets::symmetric(0.0, 12.0),
                        ..Style::default()
                    },
                    Kind::Button(b),
                )
                .unwrap_or_else(missing);
            targets.push((id, target));
        }
        // The capture, fitted below the bar, never larger than itself.
        let room = Rect::new(
            MARGIN,
            BAR + MARGIN,
            (size.w - 2.0 * MARGIN).max(1.0),
            (size.h - BAR - 2.0 * MARGIN).max(1.0),
        );
        let (pw, ph) = (self.picture.width as f32, self.picture.height as f32);
        let k = (room.w / pw).min(room.h / ph).min(1.0);
        let (w, h) = (pw * k, ph * k);
        self.place = Rect::new(
            room.x + (room.w - w) / 2.0,
            room.y + (room.h - h) / 2.0,
            w,
            h,
        );
        let shapes = {
            let mut all = self.shapes.clone();
            all.extend(self.drawing.as_ref().map(|(_, s)| s.clone()));
            all
        };
        let blurs = shapes
            .iter()
            .filter(|s| matches!(s, Shape::Blur { .. }))
            .count();
        let blurring = self
            .drawing
            .as_ref()
            .is_some_and(|(_, s)| matches!(s, Shape::Blur { .. }));
        // The blurred base is made again only when the blurs change.
        if blurs != self.base.1 || blurring {
            self.base = (self.picture.base(&shapes), blurs);
        }
        let area = t
            .add(
                root,
                Style {
                    at: Some(Point::new(0.0, 0.0)),
                    width: Length::Px(size.w),
                    height: Length::Px(size.h),
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(missing);
        scene::build(
            t,
            area,
            self.place,
            &self.picture,
            &self.base.0,
            &shapes,
            &theme,
        );
        self.targets = targets.into_iter().collect();
        self.dirty = false;
    }

    /// A view point in the capture's pixels, kept within it.
    fn to_picture(&self, p: Point) -> Point {
        let k = self.place.w / self.picture.width as f32;
        Point::new(
            ((p.x - self.place.x) / k).clamp(0.0, self.picture.width as f32),
            ((p.y - self.place.y) / k).clamp(0.0, self.picture.height as f32),
        )
    }

    fn shape_between(&self, from: Point, to: Point) -> Shape {
        let rect = Rect::new(
            from.x.min(to.x),
            from.y.min(to.y),
            (from.x - to.x).abs(),
            (from.y - to.y).abs(),
        );
        match self.tool {
            Tool::Arrow => Shape::Arrow {
                from,
                to,
                color: self.color,
            },
            Tool::Frame => Shape::Frame {
                rect,
                color: self.color,
            },
            Tool::Blur => Shape::Blur { rect },
            Tool::Text => Shape::Text {
                at: from,
                text: String::new(),
                color: self.color,
            },
        }
    }

    pub fn pointer_move(&mut self, at: Point) {
        self.pointer = Some(at);
        if let Some((from, _)) = self.drawing.clone() {
            let to = self.to_picture(at);
            self.drawing = Some((from, self.shape_between(from, to)));
            self.dirty = true;
            return;
        }
        self.tree.input(Input::PointerMove(at));
    }

    pub fn pointer_button(&mut self, pressed: bool) -> Vec<Outcome> {
        let at = self.pointer.unwrap_or_default();
        if pressed && self.place.contains(at) {
            self.end_typing();
            let from = self.to_picture(at);
            if self.tool == Tool::Text {
                self.shapes.push(self.shape_between(from, from));
                self.typing = Some(self.shapes.len() - 1);
            } else {
                self.drawing = Some((from, self.shape_between(from, from)));
            }
            self.dirty = true;
            return Vec::new();
        }
        if !pressed && let Some((from, shape)) = self.drawing.take() {
            let to = self.to_picture(at);
            // A click alone draws nothing.
            if (to.x - from.x).abs() + (to.y - from.y).abs() > 3.0 {
                self.shapes.push(shape);
            }
            self.dirty = true;
            return Vec::new();
        }
        let events = self.tree.input(Input::PointerButton { pressed });
        self.handle(events)
    }

    /// The text being typed ends; an empty one goes.
    fn end_typing(&mut self) {
        if let Some(i) = self.typing.take()
            && matches!(self.shapes.get(i), Some(Shape::Text { text, .. }) if text.trim().is_empty())
        {
            self.shapes.remove(i);
        }
        self.dirty = true;
    }

    /// Typed text goes to the text being written.
    pub fn text(&mut self, typed: &str) {
        if let Some(Shape::Text { text, .. }) = self.typing.and_then(|i| self.shapes.get_mut(i)) {
            text.push_str(typed);
            self.dirty = true;
        }
    }

    pub fn key(&mut self, key: Key) -> Vec<Outcome> {
        if self.typing.is_some() {
            match key {
                Key::Enter | Key::Escape => self.end_typing(),
                Key::Backspace => {
                    if let Some(Shape::Text { text, .. }) =
                        self.typing.and_then(|i| self.shapes.get_mut(i))
                    {
                        text.pop();
                        self.dirty = true;
                    }
                }
                _ => {}
            }
            return Vec::new();
        }
        if key == Key::Escape {
            return vec![Outcome::Close];
        }
        let events = self.tree.input(Input::Key(key));
        self.handle(events)
    }

    /// Ctrl and a letter: Z undoes, C copies, S saves.
    pub fn shortcut(&mut self, letter: char) -> Vec<Outcome> {
        match letter.to_ascii_lowercase() {
            'z' => {
                self.undo();
                Vec::new()
            }
            'c' => self.make(Outcome::Copy),
            's' => self.make(Outcome::Save),
            _ => Vec::new(),
        }
    }

    pub fn undo(&mut self) {
        self.typing = None;
        self.shapes.pop();
        self.dirty = true;
    }

    /// The picture with its marks, as PNG.
    pub fn picture_png(&self) -> anyhow::Result<Vec<u8>> {
        scene::export(&self.picture, &self.shapes, self.tree.theme())
            .ok_or_else(|| anyhow::anyhow!("the picture could not be made"))?
            .to_png()
    }

    fn make(&mut self, outcome: fn(Vec<u8>) -> Outcome) -> Vec<Outcome> {
        self.end_typing();
        match self.picture_png() {
            Ok(png) => vec![outcome(png)],
            Err(err) => {
                tracing::warn!("{err:#}");
                Vec::new()
            }
        }
    }

    /// Says what happened, in the bar: saved where, copied.
    pub fn set_note(&mut self, note: Option<String>) {
        self.note = note;
        self.dirty = true;
    }

    fn handle(&mut self, events: Vec<Event>) -> Vec<Outcome> {
        let mut out = Vec::new();
        for event in events {
            let Event::Activated(id) = event else {
                continue;
            };
            match self.targets.get(&id).copied() {
                Some(Target::Tool(tool)) => {
                    self.end_typing();
                    self.tool = tool;
                    self.dirty = true;
                }
                Some(Target::Color(i)) => {
                    self.color = i;
                    if let Some(Shape::Text { color, .. }) =
                        self.typing.and_then(|i| self.shapes.get_mut(i))
                    {
                        *color = i;
                    }
                    self.dirty = true;
                }
                Some(Target::Undo) => self.undo(),
                Some(Target::Copy) => out.extend(self.make(Outcome::Copy)),
                Some(Target::Save) => out.extend(self.make(Outcome::Save)),
                None => {}
            }
        }
        out
    }

    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        if self.dirty {
            self.build();
        }
        let damage = self.tree.update(now);
        self.surface.draw(&mut self.tree, &damage)
    }

    pub fn pixels(&self) -> &[u8] {
        self.surface.pixels()
    }

    pub fn pixel_size(&self) -> (u32, u32) {
        (self.surface.width(), self.surface.height())
    }

    pub fn access(&self) -> AccessNode {
        self.tree.access()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> AnnotateView {
        let picture = Picture {
            width: 400,
            height: 300,
            rgba: [40, 60, 80, 255].repeat(400 * 300),
        };
        let mut v = AnnotateView::new(
            Theme::default(),
            Lang::En,
            Size::new(900.0, 700.0),
            1.0,
            picture,
        );
        v.draw(Duration::ZERO);
        v
    }

    fn drag(v: &mut AnnotateView, from: (f32, f32), to: (f32, f32)) {
        let p = |(x, y): (f32, f32)| Point::new(v.place.x + x, v.place.y + y);
        let (a, b) = (p(from), p(to));
        v.pointer_move(a);
        v.pointer_button(true);
        v.pointer_move(b);
        v.pointer_button(false);
        v.draw(Duration::ZERO);
    }

    fn click(v: &mut AnnotateView, name: &str) -> Vec<Outcome> {
        let access = v.access();
        let node = access
            .walk()
            .into_iter()
            .find(|n| n.label.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no {name}:\n{}", access.dump()));
        let at = Point::new(
            node.rect.x + node.rect.w / 2.0,
            node.rect.y + node.rect.h / 2.0,
        );
        v.pointer_move(at);
        let mut out = v.pointer_button(true);
        out.extend(v.pointer_button(false));
        v.draw(Duration::ZERO);
        out
    }

    #[test]
    fn an_arrow_drawn_then_undone_saves_the_capture_as_taken() {
        let mut v = view();
        drag(&mut v, (20.0, 20.0), (200.0, 150.0));
        assert_eq!(v.shapes().len(), 1);
        click(&mut v, "Undo");
        let out = click(&mut v, "Save");
        let [Outcome::Save(png)] = out.as_slice() else {
            panic!("{out:?}");
        };
        let saved = Picture::from_png(png).expect("a PNG");
        assert_eq!(saved, v.picture);
    }

    #[test]
    fn a_text_is_typed_where_clicked_in_the_colour_chosen() {
        let mut v = view();
        click(&mut v, "Text");
        click(&mut v, "Yellow");
        drag(&mut v, (50.0, 50.0), (50.0, 50.0));
        v.text("Look");
        v.key(Key::Enter);
        assert!(matches!(
            v.shapes(),
            [Shape::Text { text, color: 1, .. }] if text == "Look"
        ));
        // Ctrl+Z takes it back.
        v.shortcut('z');
        assert!(v.shapes().is_empty());
    }
}
