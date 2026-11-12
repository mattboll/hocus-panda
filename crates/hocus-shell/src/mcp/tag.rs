//! An MCP agent's tag (docs/design/mcp.md, scene 1): before an action,
//! the agent's name on its colour over what it wants to do, in its words,
//! at the top left of the window it will touch (the compositor rings the
//! window), or at the top of the screen for a desktop, a setting or a
//! launch. A layer surface of its own ([`hocus_link::MCP_TAG_NAMESPACE`]),
//! in the overlay, taking neither the keyboard nor the pointer; drawn once.
//!
//! Beside an agent's own pointer (lot B, [`McpAt::Pointer`]), the tag
//! hangs off its arrow as the mockup's `.cursor` does: 24 px right of its
//! tip and 10 px under it; a pointer resting after its action shows its
//! name alone.
//!
//! One card, so that its corners are the compositor's to round on the
//! CPU: the name's band on the agent's violet (the mockup's `.tag`, 11.5
//! px bold, 3 × 8 px), the intent under it on the dark ink (`.intent`,
//! 11.5 px, 4 × 8 px, at most 240 px wide, wrapped), rimmed in the violet.

use hocus_link::{McpAt, McpShow};
use iced::{
    Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse,
    widget::{
        canvas,
        canvas::{Frame, Geometry, Stroke},
        text::Alignment,
    },
};

use crate::{
    Message,
    levels::draw::{LINE, TEXT},
    popover::kit::{self, MANROPE_SEMIBOLD},
    theme::Ink,
};

/// The agents' colour, the theme's `agent` token.
pub const AGENT: Ink = Ink::Token(|t| t.agent);
const NAME: f32 = 11.5;
const INTENT: f32 = 11.5;
const PAD: Size = Size::new(8.0, 3.0);
const INTENT_PAD: f32 = 4.0;
const MOST: f32 = 240.0;
const RADIUS: f32 = 7.0;
const ON_AGENT: Color = Color::from_rgb8(0x1B, 0x12, 0x30);
const INK: Color = Color::from_rgba8(20, 16, 36, 0.96);

/// Where a tag sits from its agent's pointer's tip (the mockup's arrow,
/// 20 px wide, its tip 2 px in, then 6 px; the tag 12 px down).
const BESIDE: (i32, i32) = (24, 10);

/// What the tag lays out: its name's band, its intent's lines (none for a
/// pointer's name alone).
fn layout(show: &McpShow) -> (Size, Vec<String>) {
    let name = kit::measure(&show.agent, NAME, crate::levels::draw::MANROPE_BOLD);
    let room = MOST - 2.0 * PAD.width;
    let lines = if show.intent.is_empty() {
        Vec::new()
    } else {
        kit::wrap(&show.intent, room, |t| {
            kit::measure(t, INTENT, kit::MANROPE_SEMIBOLD)
        })
    };
    let widest = lines
        .iter()
        .map(|l| kit::measure(l, INTENT, kit::MANROPE_SEMIBOLD))
        .fold(name, f32::max);
    let width = (widest + 2.0 * PAD.width + 2.0).ceil();
    let under = if lines.is_empty() {
        0.0
    } else {
        lines.len() as f32 * INTENT * LINE + 2.0 * INTENT_PAD
    };
    let height = (NAME * LINE + 2.0 * PAD.height + under + 2.0).ceil();
    (Size::new(width, height), lines)
}

/// Its size and its top left corner on its screen.
pub fn place(show: &McpShow) -> ((u32, u32), (i32, i32)) {
    let (size, _) = layout(show);
    let at = match show.at {
        McpAt::Window { x, y } => (x, y),
        McpAt::Pointer { x, y } => (x + BESIDE.0, y + BESIDE.1),
        McpAt::Desktop { x, y, .. } | McpAt::Screen { x, y } => {
            (x - (size.width / 2.0).round() as i32, y)
        }
    };
    ((size.width as u32, size.height as u32), at)
}

pub fn view(show: McpShow) -> Element<'static, Message> {
    canvas(Tag(show))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

struct Tag(McpShow);

impl<M> canvas::Program<M> for Tag {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let (_, lines) = layout(&self.0);
        let size = bounds.size();
        let agent = AGENT.get();
        let all = Rectangle::new(Point::ORIGIN, size);
        f.fill(&kit::rounded(all, RADIUS), INK);
        let band = NAME * LINE + 2.0 * PAD.height;
        // The name's band: the card's top corners rounded, its bottom
        // square under the intent; the whole card for a name alone.
        if lines.is_empty() {
            f.fill(&kit::rounded(all, RADIUS), agent);
        } else {
            f.with_clip(
                Rectangle::new(Point::ORIGIN, Size::new(size.width, band + 1.0)),
                |f| {
                    f.fill(
                        &kit::rounded(
                            Rectangle::new(Point::ORIGIN, Size::new(size.width, band + RADIUS)),
                            RADIUS,
                        ),
                        agent,
                    );
                },
            );
        }
        f.fill_text(kit::text_in(
            crate::levels::draw::MANROPE_BOLD,
            kit::text(
                self.0.agent.clone(),
                Point::new(1.0 + PAD.width, 1.0 + band / 2.0),
                NAME,
                ON_AGENT,
                Alignment::Left,
            ),
        ));
        let mut y = band + INTENT_PAD + 1.0;
        for line in lines {
            f.fill_text(kit::text_in(
                MANROPE_SEMIBOLD,
                kit::text(
                    line,
                    Point::new(1.0 + PAD.width, y + INTENT * LINE / 2.0),
                    INTENT,
                    TEXT.get(),
                    Alignment::Left,
                ),
            ));
            y += INTENT * LINE;
        }
        let rim = Rectangle::new(
            Point::new(0.5, 0.5),
            Size::new(size.width - 1.0, size.height - 1.0),
        );
        f.stroke(
            &kit::rounded(rim, RADIUS - 0.5),
            Stroke::default()
                .with_color(Color { a: 0.55, ..agent })
                .with_width(1.0),
        );
        vec![f.into_geometry()]
    }
}
