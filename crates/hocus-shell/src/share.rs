//! What to share (#116): when an application asks to see the screen (a
//! browser, a video call, through xdg-desktop-portal), the screens and the
//! windows show as pictures, taken when it opens; one is chosen, or none.
//! The rest of the screen dims.

use std::{sync::Arc, time::Duration};

use hocus_ui::{
    AccessNode, Align, Button, ButtonKind, Color, Direction, Event, Input, Insets, Justify, Key,
    Kind, Length, Look, NodeId, Paint, PixelRect, Point, Size, Style, Surface, Theme, Tree,
};

use crate::{column::View, i18n::Lang};

const MISSING: fn() -> NodeId = || unreachable!("adding to a node of this tree");
const DIM: Color = Color::rgba(0, 0, 0, 140);
/// A picture's width in the card, logical pixels.
const THUMB: f32 = 200.0;
/// Windows on a line.
const PER_LINE: usize = 4;

/// Something that can be shared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shareable {
    /// What the portal is told: `Monitor: DP-1`, `Window: <id>`.
    pub answer: String,
    pub name: String,
    pub detail: String,
    /// Width, height, premultiplied RGBA, already small.
    pub picture: Option<(u16, u16, Arc<[u8]>)>,
}

pub struct SharePicker {
    tree: Tree,
    surface: Surface,
    lang: Lang,
    screens: Vec<Shareable>,
    windows: Vec<Shareable>,
    targets: Vec<(NodeId, String)>,
    cancel: Option<NodeId>,
}

impl std::fmt::Debug for SharePicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharePicker")
            .field("screens", &self.screens.len())
            .field("windows", &self.windows.len())
            .finish_non_exhaustive()
    }
}

/// What the user did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareChoice {
    /// The portal's answer for what was chosen.
    Share(String),
    Cancel,
}

impl SharePicker {
    pub fn new(
        theme: Theme,
        lang: Lang,
        size: Size,
        scale: f32,
        screens: Vec<Shareable>,
        windows: Vec<Shareable>,
    ) -> Self {
        let mut picker = Self {
            tree: Tree::new(theme, size, scale),
            surface: Surface::new(size, scale),
            lang,
            screens,
            windows,
            targets: Vec::new(),
            cancel: None,
        };
        picker.build();
        picker
    }

    fn build(&mut self) {
        let theme = self.tree.theme().clone();
        let lang = self.lang;
        let t = &mut self.tree;
        let root = t.root();
        for child in t.children(root) {
            t.remove(child);
        }
        self.targets.clear();
        t.set_look(
            root,
            Look {
                background: Some(Paint::Rgba(DIM)),
                ..Look::default()
            },
        );
        t.set_style(
            root,
            Style {
                align: Align::Center,
                justify: Justify::Center,
                ..Style::default()
            },
        );
        // Four pictures to a line, within the screen.
        let width = (PER_LINE as f32 * (THUMB + 16.0) + (PER_LINE - 1) as f32 * 12.0 + 56.0)
            .min(t.size().w - 64.0);
        let card = t
            .add(
                root,
                Style {
                    width: Length::Px(width),
                    padding: Insets::all(28.0),
                    gap: 16.0,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Glass),
                    radius: 24.0,
                    border: Some(Paint::Border),
                    shadow: 30.0,
                }),
            )
            .unwrap_or_else(MISSING);
        t.add(
            card,
            Style::default(),
            Kind::text(lang.share_title(), theme.text.headline.with_weight(700)),
        )
        .unwrap_or_else(MISSING);
        t.add(
            card,
            Style::default(),
            Kind::text(lang.share_note(), theme.text.body.with_color(Paint::Muted)),
        )
        .unwrap_or_else(MISSING);
        let mut targets = Vec::new();
        for (caption, items) in [
            (lang.share_screens(), &self.screens),
            (lang.share_windows(), &self.windows),
        ] {
            if items.is_empty() {
                continue;
            }
            t.add(
                card,
                Style {
                    padding: Insets {
                        top: 6.0,
                        ..Insets::default()
                    },
                    ..Style::default()
                },
                Kind::text(caption.to_uppercase(), theme.text.caption),
            )
            .unwrap_or_else(MISSING);
            for line in items.chunks(PER_LINE) {
                let row = t
                    .add(
                        card,
                        Style {
                            direction: Direction::Row,
                            gap: 12.0,
                            ..Style::default()
                        },
                        Kind::Box(Look::default()),
                    )
                    .unwrap_or_else(MISSING);
                for item in line {
                    let tile = t
                        .add(
                            row,
                            Style {
                                width: Length::Px(THUMB + 16.0),
                                padding: Insets::all(8.0),
                                gap: 6.0,
                                ..Style::default()
                            },
                            Kind::Box(Look {
                                background: Some(Paint::SurfaceHover),
                                radius: 14.0,
                                ..Look::default()
                            }),
                        )
                        .unwrap_or_else(MISSING);
                    let said = format!("{} · {}", item.name, item.detail);
                    t.set_clickable(tile, &said);
                    let height = item
                        .picture
                        .as_ref()
                        .map_or(THUMB * 9.0 / 16.0, |(w, h, _)| {
                            (THUMB * f32::from(*h) / f32::from((*w).max(1))).min(THUMB)
                        });
                    let picture = item
                        .picture
                        .as_ref()
                        .and_then(|(w, h, px)| Kind::image(*w, *h, px, &item.name));
                    let style = Style {
                        width: Length::Px(THUMB),
                        height: Length::Px(height),
                        ..Style::default()
                    };
                    match picture {
                        Some(image) => t.add(tile, style, image),
                        None => t.add(
                            tile,
                            style,
                            Kind::Box(Look {
                                background: Some(Paint::SurfacePressed),
                                radius: 8.0,
                                ..Look::default()
                            }),
                        ),
                    }
                    .unwrap_or_else(MISSING);
                    t.add(
                        tile,
                        Style {
                            width: Length::Px(THUMB),
                            ..Style::default()
                        },
                        Kind::text_line(item.name.clone(), theme.text.label.with_weight(600)),
                    )
                    .unwrap_or_else(MISSING);
                    t.add(
                        tile,
                        Style {
                            width: Length::Px(THUMB),
                            ..Style::default()
                        },
                        Kind::text_line(
                            item.detail.clone(),
                            theme.text.caption.with_color(Paint::Muted),
                        ),
                    )
                    .unwrap_or_else(MISSING);
                    targets.push((tile, item.answer.clone()));
                }
            }
        }
        let foot = t
            .add(
                card,
                Style {
                    direction: Direction::Row,
                    justify: Justify::End,
                    width: Length::Fill,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .unwrap_or_else(MISSING);
        let cancel = t
            .add(
                foot,
                Style {
                    height: Length::Px(40.0),
                    padding: Insets::symmetric(0.0, 18.0),
                    ..Style::default()
                },
                Kind::Button(Button::new(lang.share_cancel(), ButtonKind::Plain, &theme)),
            )
            .unwrap_or_else(MISSING);
        self.targets = targets;
        self.cancel = Some(cancel);
    }

    pub fn pointer_move(&mut self, at: Point) {
        self.tree.input(Input::PointerMove(at));
    }

    pub fn pointer_button(&mut self, pressed: bool) -> Option<ShareChoice> {
        let events = self.tree.input(Input::PointerButton { pressed });
        self.handle(events)
    }

    /// Escape cancels; Tab and Enter go through the choices.
    pub fn key(&mut self, key: Key) -> Option<ShareChoice> {
        if key == Key::Escape {
            return Some(ShareChoice::Cancel);
        }
        let events = self.tree.input(Input::Key(key));
        self.handle(events)
    }

    fn handle(&mut self, events: Vec<Event>) -> Option<ShareChoice> {
        events.into_iter().find_map(|event| match event {
            Event::Activated(id) if Some(id) == self.cancel => Some(ShareChoice::Cancel),
            Event::Activated(id) => self
                .targets
                .iter()
                .find(|(t, _)| *t == id)
                .map(|(_, answer)| ShareChoice::Share(answer.clone())),
            _ => None,
        })
    }

    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        let damage = self.tree.update(now);
        self.surface.draw(&mut self.tree, &damage)
    }

    /// Its see-through panels, from its corner, for the compositor to blur
    /// what is behind them (#37, #162).
    pub fn glass(&self) -> Vec<hocus_ui::Glass> {
        self.tree.glass()
    }

    /// Its panels' shadows, from its corner, for the compositor to draw
    /// under it (#281).
    pub fn shadows(&self) -> Vec<hocus_ui::Cast> {
        self.tree.shadows()
    }

    pub fn view(&self) -> View<'_> {
        View {
            pixels: self.surface.pixels(),
            width: self.surface.width(),
            height: self.surface.height(),
            at: Point::default(),
        }
    }

    pub fn access(&self) -> AccessNode {
        self.tree.access()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(answer: &str, name: &str) -> Shareable {
        Shareable {
            answer: answer.into(),
            name: name.into(),
            detail: "1920×1080".into(),
            picture: Some((2, 1, vec![255; 8].into())),
        }
    }

    #[test]
    fn a_click_shares_what_it_is_on_and_escape_nothing() {
        let mut p = SharePicker::new(
            Theme::default(),
            Lang::En,
            Size::new(1280.0, 800.0),
            1.0,
            vec![item("Monitor: DP-1", "DP-1")],
            vec![item("Window: 42", "notes.txt")],
        );
        p.draw(Duration::ZERO);
        let access = p.access();
        let node = access
            .walk()
            .into_iter()
            .find(|n| n.label.as_deref() == Some("notes.txt · 1920×1080"))
            .unwrap_or_else(|| panic!("no window:\n{}", access.dump()));
        let at = Point::new(node.rect.x + 10.0, node.rect.y + 10.0);
        p.pointer_move(at);
        p.pointer_button(true);
        assert_eq!(
            p.pointer_button(false),
            Some(ShareChoice::Share("Window: 42".into()))
        );
        assert_eq!(p.key(Key::Escape), Some(ShareChoice::Cancel));
    }
}
