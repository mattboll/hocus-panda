//! What the editor draws, the 5.9 mockup's sizes × the launcher's zoom,
//! less on a screen too small for it: under its controls ([`Back`]) the
//! desktop darkened, the panel, the image with its marks, the separator
//! and the file's name; inside them (the kit's, [`super::view`]) each
//! tool, colour and button, the mark being drawn and the text's box.

use hocus_link::LAUNCHER_ZOOM;
use iced::advanced::{image::Renderer as _, svg};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, Vector, mouse,
    widget::{
        canvas::{self, Frame, Geometry, LineCap, LineDash, LineJoin, Path, Stroke, gradient},
        text::Alignment,
    },
    window,
};

use super::{COLOURS, Editor, Phase, Tool, marks::Mark};
use crate::theme::Ink;
use crate::{
    capture::{Words, marks},
    levels::draw::{MANROPE_BOLD, TEXT as TEXT_COLOUR, rgb},
    popover::kit::{self, MANROPE_SEMIBOLD},
};

/// The launcher's zoom: the editor at its largest.
pub const Z: f32 = LAUNCHER_ZOOM as f32;
/// The panel, the mockup's units: 696 wide, 12 in, its rows 10 apart.
const PANEL: (f32, f32) = (696.0, 530.0);
const PAD: f32 = 12.0;
const GAP: f32 = 10.0;
const TOOLBAR: f32 = 32.0;
const FOOTER: f32 = 34.0;
const IMAGE: (f32, f32) = (672.0, 420.0);
/// Its margin on a screen too small for it.
const MARGIN: f32 = 24.0;
/// The tools: 9 in, their icon 17, 6 before their label, 12 px 600; 2
/// apart; the separator 1 × 24, 6 from each side; the colours 22, 2 from
/// each side; undo 32 square.
const TOOL_PAD: f32 = 9.0;
const ICON: f32 = 17.0;
const ICON_GAP: f32 = 6.0;
const LABEL: f32 = 12.0;
const ITEM_GAP: f32 = 2.0;
const SEPARATOR: (f32, f32) = (1.0, 24.0);
const SEPARATOR_MARGIN: f32 = 6.0;
const SWATCH: f32 = 22.0;
const SWATCH_MARGIN: f32 = 2.0;
const SWATCH_RIM: f32 = 2.0;
pub const BUTTON_RADIUS: f32 = 9.0;
/// The footer: its name 11.5 px; its buttons 12 in (the green one 16),
/// 12.5 px, radius 10, 8 apart.
const NAME: f32 = 11.5;
const FOOT_PAD: f32 = 12.0;
const GREEN_PAD: f32 = 16.0;
const FOOT_TEXT: f32 = 12.5;
pub const FOOT_RADIUS: f32 = 10.0;
const FOOT_GAP: f32 = 8.0;
const PANEL_RADIUS: f32 = 16.0;
const IMAGE_RADIUS: f32 = 10.0;
/// The text's field: 220 × 30, radius 6.
pub const FIELD: (f32, f32) = (220.0, 30.0);
pub const FIELD_RADIUS: f32 = 6.0;
/// The icons' strokes: 1.9 of their 24.
const STROKE: f32 = 1.9;

const SHADE: Ink = Ink::Fixed(Color::from_rgba8(6, 8, 14, 0.6));
const PANEL_TOP: Ink = Ink::Dark(Color::from_rgba8(22, 27, 42, 0.97));
const PANEL_BOTTOM: Ink = Ink::Dark(Color::from_rgba8(14, 17, 28, 0.98));
const RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.12));
const HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const CHOSEN: Ink = Ink::Accent(0.22);
const CHOSEN_RING: Ink = Ink::Accent(0.6);
const SWATCH_OFF: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.15));
const IMAGE_BACK: Ink = Ink::Dark(Color::from_rgba8(0, 0, 0, 0.3));
const NAME_COLOUR: Ink = Ink::Dark(rgb(0x7C8698));
const DELETE: Ink = Ink::Dark(rgb(0xF7A77A));
const FOOT_RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.14));
const GREEN: Ink = Ink::Accent(1.0);
const ON_GREEN: Ink = Ink::Dark(rgb(0x0B1020));
const FIELD_BACK: Ink = Ink::Dark(Color::from_rgba8(0, 0, 0, 0.35));
pub const HINT: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.6));

/// Where the editor's pieces are on its surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Its zoom: the launcher's, less on a small screen.
    pub z: f32,
    pub panel: Rectangle,
    pub tools: Vec<(Tool, Rectangle)>,
    pub separator: Rectangle,
    pub colours: Vec<Rectangle>,
    pub undo: Rectangle,
    /// The image's place, and the image in it.
    pub room: Rectangle,
    pub image: Rectangle,
    pub name_at: Point,
    pub delete: Rectangle,
    pub save_as: Rectangle,
    pub copy: Rectangle,
}

impl Layout {
    /// On a surface `size`, for an image `logical` large (its pixels over
    /// the screen's scale).
    pub fn of(size: Size, words: Words, logical: (f32, f32)) -> Self {
        let z = Z
            .min((size.width - 2.0 * MARGIN) / PANEL.0)
            .min((size.height - 2.0 * MARGIN) / PANEL.1)
            .max(0.4);
        let (pw, ph) = (PANEL.0 * z, PANEL.1 * z);
        let panel = Rectangle::new(
            Point::new(
                ((size.width - pw) / 2.0).round(),
                ((size.height - ph) / 2.0).round(),
            ),
            Size::new(pw, ph),
        );
        let (left, top) = (panel.x + PAD * z, panel.y + PAD * z);
        let right = panel.x + pw - PAD * z;
        let bar = TOOLBAR * z;
        let middle = top + bar / 2.0;
        let mut x = left;
        let mut tools = Vec::new();
        for tool in Tool::ALL {
            let label = kit::measure(words.tool(tool), LABEL * z, MANROPE_SEMIBOLD);
            let width = ((2.0 * TOOL_PAD + ICON + ICON_GAP) * z + label).ceil();
            tools.push((
                tool,
                Rectangle::new(Point::new(x, top), Size::new(width, bar)),
            ));
            x += width + ITEM_GAP * z;
        }
        x += SEPARATOR_MARGIN * z;
        let separator = Rectangle::new(
            Point::new(x, middle - SEPARATOR.1 * z / 2.0),
            Size::new(SEPARATOR.0, SEPARATOR.1 * z),
        );
        x += SEPARATOR.0 + (SEPARATOR_MARGIN + ITEM_GAP) * z;
        let mut colours = Vec::new();
        for _ in COLOURS {
            x += SWATCH_MARGIN * z;
            let side = SWATCH * z;
            colours.push(Rectangle::new(
                Point::new(x, middle - side / 2.0),
                Size::new(side, side),
            ));
            x += side + (SWATCH_MARGIN + ITEM_GAP) * z;
        }
        let undo = Rectangle::new(Point::new(right - bar, top), Size::new(bar, bar));
        let room = Rectangle::new(
            Point::new(left, top + bar + GAP * z),
            Size::new(IMAGE.0 * z, IMAGE.1 * z),
        );
        let k = (room.width / logical.0.max(1.0))
            .min(room.height / logical.1.max(1.0))
            .min(1.0);
        let (iw, ih) = (logical.0 * k, logical.1 * k);
        let image = Rectangle::new(
            Point::new(
                room.x + (room.width - iw) / 2.0,
                room.y + (room.height - ih) / 2.0,
            ),
            Size::new(iw, ih),
        );
        let foot = room.y + room.height + GAP * z;
        let height = FOOTER * z;
        let button = |text: &str, pad: f32, font| {
            (kit::measure(text, FOOT_TEXT * z, font) + 2.0 * pad * z).ceil()
        };
        let copy_w = button(words.copy_and_close(), GREEN_PAD, MANROPE_BOLD);
        let save_w = button(words.save_as(), FOOT_PAD, MANROPE_SEMIBOLD);
        let delete_w = button(words.delete(), FOOT_PAD, MANROPE_SEMIBOLD);
        let copy = Rectangle::new(Point::new(right - copy_w, foot), Size::new(copy_w, height));
        let save_as = Rectangle::new(
            Point::new(copy.x - FOOT_GAP * z - save_w, foot),
            Size::new(save_w, height),
        );
        let delete = Rectangle::new(
            Point::new(save_as.x - FOOT_GAP * z - delete_w, foot),
            Size::new(delete_w, height),
        );
        Self {
            z,
            panel,
            tools,
            separator,
            colours,
            undo,
            room,
            image,
            name_at: Point::new(left, foot + height / 2.0),
            delete,
            save_as,
            copy,
        }
    }

    /// Where the pieces are, as the editor's log says it (for tests):
    /// `name@x,y` (their middles), `image@x,y,w,h`.
    pub fn said(&self) -> String {
        let at =
            |name: &str, r: Rectangle| format!("{name}@{:.0},{:.0}", r.center_x(), r.center_y());
        let mut said: Vec<String> = self
            .tools
            .iter()
            .map(|(tool, r)| at(&format!("{tool:?}").to_lowercase(), *r))
            .collect();
        said.extend(
            self.colours
                .iter()
                .enumerate()
                .map(|(i, r)| at(&format!("colour{i}"), *r)),
        );
        said.push(at("undo", self.undo));
        said.push(at("delete", self.delete));
        said.push(at("save-as", self.save_as));
        said.push(at("copy", self.copy));
        let i = self.image;
        said.push(format!(
            "image@{:.1},{:.1},{:.1},{:.1}",
            i.x, i.y, i.width, i.height
        ));
        said.join(" ")
    }
}

/// What nothing holds, under the editor's controls: the desktop
/// darkened, the panel, the image with its marks, the separator and the
/// file's name.
#[derive(Debug, Clone)]
pub struct Back {
    pub id: window::Id,
    pub words: Words,
    /// The compositor blurs behind the shell: a shadow under the panel.
    pub glass: bool,
}

/// The editor's image's size, logical pixels of its screen.
pub fn logical(e: &Editor) -> (f32, f32) {
    let s = e.scale.max(0.1);
    (e.image.width() as f32 / s, e.image.height() as f32 / s)
}

impl<Message> canvas::Program<Message> for Back {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let shown = super::with(|e| {
            let e = e.as_mut().filter(|e| e.phase == Phase::Shown(self.id))?;
            hold(e, renderer);
            Some(e.clone())
        });
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), SHADE.get());
        if let Some(editor) = shown {
            let layout = Layout::of(bounds.size(), self.words, logical(&editor));
            self.draw_panel(&mut frame, &editor, &layout);
        }
        vec![frame.into_geometry()]
    }
}

/// The images the editor shows, in the renderer's memory before they are
/// drawn (#594). iced's GPU renderer uploads an image of more than 2 MiB
/// on a thread of its own and draws nothing for it until that is done; the
/// redraw it then asks for is lost (iced_layershell does nothing with
/// `RedrawAll`), so the picture shown again with a new mark stayed away
/// until something else drew the editor again. Loaded here, it is uploaded
/// before this frame, in the frame that shows it; held, it stays until a
/// new one replaces it. On the CPU this decodes nothing more than drawing
/// it would.
pub fn hold(e: &mut Editor, renderer: &Renderer) {
    let wanted = [e.shown.id(), e.blurred.id()];
    e.held.retain(|a| wanted.contains(&a.handle().id()));
    for handle in [e.shown.clone(), e.blurred.clone()] {
        if e.held.iter().any(|a| a.handle().id() == handle.id()) {
            continue;
        }
        match renderer.load_image(&handle) {
            Ok(allocation) => e.held.push(allocation),
            // Drawn as iced draws it, then: when it can.
            Err(err) => tracing::debug!(%err, "the editor's image is not loaded"),
        }
    }
}

/// The frame of the text being typed: from where it was clicked.
pub fn field_of(at: Point, z: f32) -> Rectangle {
    Rectangle::new(
        Point::new(at.x, at.y - FIELD.1 * z / 2.0),
        Size::new(FIELD.0 * z, FIELD.1 * z),
    )
}

impl Back {
    fn draw_panel(&self, f: &mut Frame, e: &Editor, layout: &Layout) {
        let z = layout.z;
        let panel = layout.panel;
        if self.glass {
            let margin = 60.0 * z * 2.0;
            let around = Rectangle::new(
                Point::new(panel.x - margin, panel.y - margin),
                Size::new(panel.width + 2.0 * margin, panel.height + 2.0 * margin),
            );
            f.draw_svg(
                around,
                svg::Svg::new(shadow(around.size(), margin, panel.size(), z)),
            );
        }
        let tint = gradient::Linear::new(
            panel.position(),
            Point::new(panel.x, panel.y + panel.height),
        )
        .add_stop(0.0, PANEL_TOP.get())
        .add_stop(1.0, PANEL_BOTTOM.get());
        let radius = PANEL_RADIUS * z;
        f.fill(&kit::rounded(panel, radius), canvas::Fill::from(tint));
        f.stroke(
            &kit::rounded(inset(panel, 0.5), radius - 0.5),
            Stroke::default().with_color(RIM.get()).with_width(1.0),
        );
        f.fill_rectangle(
            layout.separator.position(),
            layout.separator.size(),
            RIM.get(),
        );
        // The image, and its marks over it.
        let room = layout.room;
        let image_radius = IMAGE_RADIUS * z;
        f.fill(&kit::rounded(room, image_radius), IMAGE_BACK.get());
        // Its corners rounded: the radius cuts the image's clip.
        let mut picture = canvas::Image::new(e.shown.clone());
        picture.border_radius = image_radius.into();
        f.with_clip(layout.image, |f| f.draw_image(layout.image, picture));
        f.stroke(
            &kit::rounded(inset(room, 0.5), image_radius - 0.5),
            Stroke::default().with_color(RIM.get()).with_width(1.0),
        );
        self.draw_name(f, e, layout);
    }

    fn draw_name(&self, f: &mut Frame, e: &Editor, layout: &Layout) {
        let z = layout.z;
        let said = match &e.saved_as {
            Some(name) => self.words.saved_as(name),
            None => format!(
                "{} · {} × {}",
                e.shot.name(),
                e.image.width(),
                e.image.height()
            ),
        };
        let room = (layout.delete.x - layout.name_at.x - FOOT_GAP * z).max(0.0);
        f.fill_text(kit::text(
            kit::elide(&said, room, |t| {
                kit::measure(t, NAME * z, crate::levels::draw::MANROPE)
            }),
            layout.name_at,
            NAME * z,
            NAME_COLOUR.get(),
            Alignment::Left,
        ));
    }
}

/// A tool's button at `rect` (the surface's frame): chosen, else lit
/// under the pointer; its icon and its label.
pub fn draw_tool(
    f: &mut Frame,
    rect: Rectangle,
    tool: Tool,
    label: &str,
    on: bool,
    lit: bool,
    z: f32,
) {
    let radius = BUTTON_RADIUS * z;
    let shape = kit::rounded(rect, radius);
    if on {
        f.fill(&shape, CHOSEN.get());
        f.stroke(
            &kit::rounded(inset(rect, 0.5), radius - 0.5),
            Stroke::default()
                .with_color(CHOSEN_RING.get())
                .with_width(1.0),
        );
    } else if lit {
        crate::theme::hover(f, &shape, HOVER.get());
    }
    let icon = Rectangle::new(
        Point::new(rect.x + TOOL_PAD * z, rect.center_y() - ICON * z / 2.0),
        Size::new(ICON * z, ICON * z),
    );
    draw_icon(f, Icon::Tool(tool), icon);
    f.fill_text(kit::text_in(
        MANROPE_SEMIBOLD,
        kit::text(
            label.to_owned(),
            Point::new(icon.x + icon.width + ICON_GAP * z, rect.center_y()),
            LABEL * z,
            TEXT_COLOUR.get(),
            Alignment::Left,
        ),
    ));
}

/// Colour `i`'s swatch at `rect`, ringed white when chosen.
pub fn draw_swatch(f: &mut Frame, rect: Rectangle, i: usize, on: bool, z: f32) {
    let [r, g, b] = COLOURS[i];
    let centre = rect.center();
    let side = rect.width / 2.0;
    f.fill(&Path::circle(centre, side), Color::from_rgb8(r, g, b));
    let rim = if on { TEXT_COLOUR } else { SWATCH_OFF };
    f.stroke(
        &Path::circle(centre, side - SWATCH_RIM * z / 2.0),
        Stroke::default()
            .with_color(rim.get())
            .with_width(SWATCH_RIM * z),
    );
}

/// Undo's button at `rect`, lit under the pointer.
pub fn draw_undo(f: &mut Frame, rect: Rectangle, lit: bool, z: f32) {
    if lit {
        crate::theme::hover(f, &kit::rounded(rect, BUTTON_RADIUS * z), HOVER.get());
    }
    let icon = Rectangle::new(
        Point::new(
            rect.center_x() - ICON * z / 2.0,
            rect.center_y() - ICON * z / 2.0,
        ),
        Size::new(ICON * z, ICON * z),
    );
    draw_icon(f, Icon::Undo, icon);
}

/// "Supprimer" (`delete`, in its orange) or "Enregistrer sous…" at
/// `rect`: its rim, lit under the pointer.
pub fn draw_foot(f: &mut Frame, rect: Rectangle, word: &str, delete: bool, lit: bool, z: f32) {
    let radius = FOOT_RADIUS * z;
    if lit {
        crate::theme::hover(f, &kit::rounded(rect, radius), HOVER.get());
    }
    f.stroke(
        &kit::rounded(inset(rect, 0.5), radius - 0.5),
        Stroke::default().with_color(FOOT_RIM.get()).with_width(1.0),
    );
    let colour = if delete { DELETE } else { TEXT_COLOUR };
    f.fill_text(kit::text_in(
        MANROPE_SEMIBOLD,
        kit::text(
            word.to_owned(),
            rect.center(),
            FOOT_TEXT * z,
            colour.get(),
            Alignment::Center,
        ),
    ));
}

/// "Copier et fermer" at `rect`, green.
pub fn draw_copy(f: &mut Frame, rect: Rectangle, word: &str, z: f32) {
    f.fill(&kit::rounded(rect, FOOT_RADIUS * z), GREEN.get());
    f.fill_text(kit::text_in(
        MANROPE_BOLD,
        kit::text(
            word.to_owned(),
            rect.center(),
            FOOT_TEXT * z,
            ON_GREEN.get(),
            Alignment::Center,
        ),
    ));
}

/// The text's box at `field`, dashed in the mark's `colour`.
pub fn draw_field_box(f: &mut Frame, field: Rectangle, colour: Color, z: f32) {
    let shape = kit::rounded(field, FIELD_RADIUS * z);
    f.fill(&shape, FIELD_BACK.get());
    f.stroke(
        &kit::rounded(inset(field, 0.5), FIELD_RADIUS * z - 0.5),
        Stroke {
            line_dash: LineDash {
                segments: &[4.0, 3.0],
                offset: 0,
            },
            ..Stroke::default().with_color(colour).with_width(1.0)
        },
    );
}

pub fn inset(r: Rectangle, by: f32) -> Rectangle {
    Rectangle::new(
        Point::new(r.x + by, r.y + by),
        Size::new(
            (r.width - 2.0 * by).max(0.0),
            (r.height - 2.0 * by).max(0.0),
        ),
    )
}

/// Mark `mark` (the image's pixels) drawn over the image shown at `image`,
/// as [`marks::render`] draws it.
pub fn draw_mark(f: &mut Frame, e: &Editor, image: Rectangle, mark: &Mark) {
    let k = e.pixels_per(image).max(1e-3);
    let at = |p: (f32, f32)| Point::new(image.x + p.0 / k, image.y + p.1 / k);
    let colour = |c: marks::Rgb| Color::from_rgb8(c[0], c[1], c[2]);
    let shade = |(c, a): (marks::Rgb, f32)| Color::from_rgba8(c[0], c[1], c[2], a);
    match mark {
        Mark::Arrow {
            from,
            to,
            width,
            head,
            colour: c,
        } => {
            let Some((end, corners)) = marks::arrow(*from, *to, *head) else {
                return;
            };
            let (w, halo) = (width / k, (width * 0.5).max(1.0) / k);
            let line = Path::line(at(*from), at(end));
            let tip = Path::new(|b| {
                b.move_to(at(corners[0]));
                b.line_to(at(corners[1]));
                b.line_to(at(corners[2]));
                b.close();
            });
            let round = |colour: Color, width: f32| {
                Stroke::default()
                    .with_color(colour)
                    .with_width(width)
                    .with_line_cap(LineCap::Round)
                    .with_line_join(LineJoin::Round)
            };
            f.stroke(&line, round(shade(marks::HALO), w + 2.0 * halo));
            f.stroke(&tip, round(shade(marks::HALO), 2.0 * halo));
            f.fill(&tip, shade(marks::HALO));
            f.stroke(&line, round(colour(*c), w));
            f.fill(&tip, colour(*c));
        }
        Mark::Box {
            rect,
            width,
            radius,
            colour: c,
        } => {
            let w = width / k;
            let r = Rectangle::new(at((rect.x, rect.y)), Size::new(rect.w / k, rect.h / k));
            f.stroke(
                &kit::rounded(inset(r, w / 2.0), (radius / k - w / 2.0).max(0.0)),
                Stroke::default().with_color(colour(*c)).with_width(w),
            );
        }
        Mark::Text {
            at: p,
            text,
            size,
            outline,
            colour: c,
        } => {
            let (p, size, o) = (at(*p), size / k, outline / k);
            let said = |colour: Color, by: Vector| {
                kit::text_in(
                    MANROPE_BOLD,
                    kit::text(text.clone(), p + by, size, colour, Alignment::Left),
                )
            };
            let dark = shade(marks::OUTLINE);
            for i in 0..12 {
                let angle = i as f32 * std::f32::consts::TAU / 12.0;
                f.fill_text(said(dark, Vector::new(o * angle.cos(), o * angle.sin())));
            }
            f.fill_text(said(colour(*c), Vector::new(0.0, 0.0)));
        }
        Mark::Blur { rect, .. } => {
            let r = Rectangle::new(at((rect.x, rect.y)), Size::new(rect.w / k, rect.h / k));
            if r.width < 1.0 || r.height < 1.0 {
                return;
            }
            f.with_clip(r, |f| {
                f.draw_image(image, canvas::Image::new(e.blurred.clone()));
            });
            f.fill_rectangle(r.position(), r.size(), shade(marks::BLUR_TINT));
        }
    }
}

/// An icon of the toolbar's.
#[derive(Debug, Clone, Copy)]
enum Icon {
    Tool(Tool),
    Undo,
}

/// The mockup's icon (its 24 px drawing) in `rect`.
fn draw_icon(f: &mut Frame, icon: Icon, rect: Rectangle) {
    let k = rect.width / 24.0;
    let p = |x: f32, y: f32| Point::new(rect.x + x * k, rect.y + y * k);
    let path = Path::new(|b| match icon {
        Icon::Tool(Tool::Arrow) => {
            b.move_to(p(5.0, 19.0));
            b.line_to(p(18.0, 6.0));
            b.move_to(p(10.0, 6.0));
            b.line_to(p(18.0, 6.0));
            b.line_to(p(18.0, 14.0));
        }
        Icon::Tool(Tool::Box) => {
            b.move_to(p(4.5, 6.5));
            b.line_to(p(19.5, 6.5));
            b.line_to(p(19.5, 17.5));
            b.line_to(p(4.5, 17.5));
            b.close();
        }
        Icon::Tool(Tool::Text) => {
            b.move_to(p(6.0, 6.0));
            b.line_to(p(18.0, 6.0));
            b.move_to(p(12.0, 6.0));
            b.line_to(p(12.0, 19.0));
            b.move_to(p(9.0, 19.0));
            b.line_to(p(15.0, 19.0));
        }
        Icon::Tool(Tool::Blur) => {
            for (x, y) in [
                (4.0, 4.0),
                (10.0, 4.0),
                (16.0, 4.0),
                (4.0, 10.0),
                (16.0, 10.0),
                (4.0, 16.0),
                (10.0, 16.0),
                (16.0, 16.0),
            ] {
                b.move_to(p(x, y));
                b.line_to(p(x + 4.0, y));
                b.line_to(p(x + 4.0, y + 4.0));
                b.line_to(p(x, y + 4.0));
                b.close();
            }
        }
        Icon::Undo => {
            b.move_to(p(9.0, 7.0));
            b.line_to(p(4.5, 11.5));
            b.line_to(p(9.0, 16.0));
            b.move_to(p(5.0, 11.5));
            b.line_to(p(14.0, 11.5));
            // A half circle of 5 on the right, as two curves.
            let c = 5.0 * 0.5523;
            b.bezier_curve_to(p(14.0 + c, 11.5), p(19.0, 16.5 - c), p(19.0, 16.5));
            b.bezier_curve_to(p(19.0, 16.5 + c), p(14.0 + c, 21.5), p(14.0, 21.5));
            b.line_to(p(11.0, 21.5));
        }
    });
    f.stroke(
        &path,
        Stroke::default()
            .with_color(TEXT_COLOUR.get())
            .with_width(STROKE * k)
            .with_line_join(LineJoin::Round)
            .with_line_cap(LineCap::Round),
    );
}

/// The panel's shadow, the mockup's `0 22px 60px rgba(0, 0, 0, 0.5)`, in a
/// picture `size` large, the panel `margin` in, outside it only.
fn shadow(size: Size, margin: f32, panel: Size, z: f32) -> svg::Handle {
    let (w, h) = (size.width, size.height);
    let (pw, ph) = (panel.width, panel.height);
    let r = PANEL_RADIUS * z;
    let dy = 22.0 * z;
    let deviation = 30.0 * z;
    svg::Handle::from_memory(
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" \
             height=\"{h}\"><defs><filter id=\"s\" filterUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" \
             width=\"{w}\" height=\"{h}\"><feGaussianBlur stdDeviation=\"{deviation}\"/></filter>\
             <mask id=\"o\" maskUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" width=\"{w}\" \
             height=\"{h}\"><rect width=\"{w}\" height=\"{h}\" fill=\"#fff\"/><rect x=\"{margin}\" \
             y=\"{margin}\" width=\"{pw}\" height=\"{ph}\" rx=\"{r}\" fill=\"#000\"/></mask></defs>\
             <g mask=\"url(#o)\"><rect x=\"{margin}\" y=\"{}\" width=\"{pw}\" height=\"{ph}\" \
             rx=\"{r}\" fill=\"#000\" fill-opacity=\"0.5\" filter=\"url(#s)\"/></g></svg>",
            margin + dy
        )
        .into_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{capture::editor::BLUR, clock::time::Lang};

    #[test]
    fn on_a_large_screen_it_is_the_mockup_zoomed_its_image_fitted() {
        let words = Words::new(Lang::French);
        let layout = Layout::of(Size::new(2560.0, 1600.0), words, (2560.0, 1600.0));
        assert_eq!(layout.z, Z);
        assert_eq!(layout.panel.size(), Size::new(1044.0, 795.0));
        assert_eq!(layout.room.size(), Size::new(1008.0, 630.0));
        assert_eq!(layout.image, layout.room);
        // Centred.
        assert_eq!(layout.panel.x, ((2560.0 - 1044.0) / 2.0_f32).round());
        // A small image is not made larger; a tall one fits its height.
        let small = Layout::of(Size::new(2560.0, 1600.0), words, (300.0, 200.0));
        assert_eq!(small.image.size(), Size::new(300.0, 200.0));
        let tall = Layout::of(Size::new(2560.0, 1600.0), words, (800.0, 1600.0));
        assert_eq!(tall.image.height, 630.0);
        assert_eq!(tall.image.width, 315.0);
        // The buttons in their row, the green one last.
        assert!(layout.delete.x + layout.delete.width < layout.save_as.x);
        assert!(layout.save_as.x + layout.save_as.width < layout.copy.x);
        assert_eq!(
            layout.copy.x + layout.copy.width,
            layout.panel.x + layout.panel.width - PAD * Z
        );
        assert!(layout.colours[4].x + layout.colours[4].width < layout.undo.x);
    }

    #[test]
    fn on_a_small_screen_it_is_smaller() {
        let layout = Layout::of(
            Size::new(1366.0, 768.0),
            Words::new(Lang::English),
            (1366.0, 768.0),
        );
        assert!(layout.z < Z);
        assert!(layout.panel.y >= MARGIN - 1.0);
        assert!(layout.panel.y + layout.panel.height <= 768.0 - MARGIN + 1.0);
    }

    #[test]
    fn its_blurs_shown_reach_as_far_as_saved() {
        // The blur shown is the image blurred as far as a blur mark goes at
        // the editor's zoom, on the screen's pixels.
        assert_eq!(BLUR * Z, 10.5);
    }
}
