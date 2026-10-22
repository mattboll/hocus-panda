//! The recording editor's content (#132): the frame at the play head, a
//! strip of thumbnails with the cut's two handles, play and pause, and the
//! export: a format, a size limit for a chat that has one, Export. The view
//! only shows and asks; the editor runs the encoder on worker threads.

use std::{collections::HashMap, time::Duration};

use hocus_shell::Lang;
use hocus_ui::{
    AccessNode, Align, Button, ButtonKind, Direction, Event, Input, Insets, Justify, Key, Kind,
    Length, Look, NodeId, Paint, PixelRect, Point, Rect, Size, Style, Surface, TextField, Theme,
    Tree,
};

use crate::video::{Export, Format, Frames};

/// What the editor asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum TrimOutcome {
    Close,
    /// The frame at this time, for the preview.
    Seek(Duration),
    /// Playing the cut from the play head; stopping it.
    Play(Duration, Duration),
    Stop,
    Export(Export),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Format(Format),
    PlayPause,
    Export,
}

/// What a press on the strip took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    Start,
    End,
    Head,
}

const BAR: f32 = 64.0;
const MARGIN: f32 = 24.0;
const STRIP: f32 = 64.0;
/// The cut's handles, wide enough to take.
const HANDLE: f32 = 14.0;
/// Times move by a tenth of a second.
const STEP: f64 = 0.1;

pub struct TrimView {
    lang: Lang,
    tree: Tree,
    surface: Surface,
    length: Option<Duration>,
    thumbs: Option<Frames>,
    /// The frame shown: width, height, RGBA.
    preview: Option<(u32, u32, Vec<u8>)>,
    from: Duration,
    to: Duration,
    head: Duration,
    playing: bool,
    format: Format,
    limit: String,
    limit_field: Option<NodeId>,
    exporting: bool,
    note: Option<String>,
    /// Where the strip is.
    strip: Rect,
    held: Option<Held>,
    /// The handle the arrow keys move: the last one taken.
    selected: Held,
    targets: HashMap<NodeId, Target>,
    pointer: Option<Point>,
    dirty: bool,
    /// The frame and the play head moved: changed in place, so that a
    /// button being pressed keeps its node.
    moved: bool,
    preview_node: Option<NodeId>,
    head_node: Option<NodeId>,
    /// Where the frame goes.
    room: Rect,
}

impl std::fmt::Debug for TrimView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrimView")
            .field("from", &self.from)
            .field("to", &self.to)
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

fn snap(seconds: f64) -> Duration {
    Duration::from_secs_f64(((seconds / STEP).round() * STEP).max(0.0))
}

impl TrimView {
    pub fn new(theme: Theme, lang: Lang, size: Size, scale: f32) -> Self {
        Self {
            lang,
            tree: Tree::new(theme, size, scale),
            surface: Surface::new(size, scale),
            length: None,
            thumbs: None,
            preview: None,
            from: Duration::ZERO,
            to: Duration::ZERO,
            head: Duration::ZERO,
            playing: false,
            format: Format::Mp4,
            limit: String::new(),
            limit_field: None,
            exporting: false,
            note: None,
            strip: Rect::default(),
            held: None,
            selected: Held::Start,
            targets: HashMap::new(),
            pointer: None,
            dirty: true,
            moved: false,
            preview_node: None,
            head_node: None,
            room: Rect::default(),
        }
    }

    pub fn resize(&mut self, size: Size, scale: f32) {
        if size != self.tree.size() || scale != self.tree.scale() {
            self.tree.resize(size, scale);
            self.surface = Surface::new(size, scale);
            self.dirty = true;
        }
    }

    /// The recording's length is known: the cut is all of it.
    pub fn set_length(&mut self, length: Duration) {
        self.length = Some(length);
        self.from = Duration::ZERO;
        self.to = snap(length.as_secs_f64()).min(length);
        self.dirty = true;
    }

    pub fn set_thumbnails(&mut self, thumbs: Frames) {
        self.thumbs = Some(thumbs);
        self.dirty = true;
    }

    /// A frame to show, and the time it is at while playing.
    pub fn set_preview(&mut self, frame: (u32, u32, Vec<u8>), at: Option<Duration>) {
        self.preview = Some(frame);
        if let Some(at) = at {
            self.head = at;
        }
        self.moved = true;
    }

    /// Playing reached the end of the cut, or stopped.
    pub fn played(&mut self) {
        self.playing = false;
        self.dirty = true;
    }

    pub fn set_exporting(&mut self, exporting: bool) {
        self.exporting = exporting;
        self.dirty = true;
    }

    pub fn set_note(&mut self, note: Option<String>) {
        self.note = note;
        self.dirty = true;
    }

    /// The cut: its start and end.
    pub fn cut(&self) -> (Duration, Duration) {
        (self.from, self.to)
    }

    fn build(&mut self) {
        let theme = self.tree.theme().clone();
        let lang = self.lang;
        let size = self.tree.size();
        let missing = || unreachable!("adding to a node of this tree");
        let had_keys = self.limit_field.is_some() && self.tree.focused() == self.limit_field;
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
        let play = if self.playing {
            lang.pause()
        } else {
            lang.play()
        };
        let mut b = Button::new(play, ButtonKind::Plain, &theme);
        b.enabled = self.length.is_some();
        let id = t
            .add(
                bar,
                Style {
                    height: Length::Px(40.0),
                    padding: Insets::symmetric(0.0, 14.0),
                    ..Style::default()
                },
                Kind::Button(b),
            )
            .unwrap_or_else(missing);
        targets.push((id, Target::PlayPause));
        let said = self.length.map_or_else(String::new, |_| {
            lang.cut(self.from.as_secs_f64(), self.to.as_secs_f64())
        });
        t.add(
            bar,
            Style {
                padding: Insets::symmetric(0.0, 8.0),
                ..Style::default()
            },
            Kind::text_line(said, theme.text.body.with_weight(600)),
        )
        .unwrap_or_else(missing);
        t.add(
            bar,
            Style {
                width: Length::Fill,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        )
        .unwrap_or_else(missing);
        for (format, name) in [
            (Format::Mp4, "MP4"),
            (Format::WebM, "WebM"),
            (Format::Gif, "GIF"),
        ] {
            let mut b = Button::new(name, ButtonKind::Chip, &theme);
            b.selected = self.format == format;
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
            targets.push((id, Target::Format(format)));
        }
        let boxed = t
            .add(
                bar,
                Style {
                    width: Length::Px(170.0),
                    height: Length::Px(40.0),
                    padding: Insets::symmetric(0.0, 12.0),
                    align: Align::Center,
                    direction: Direction::Row,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::SurfaceHover),
                    border: Some(Paint::Border),
                    radius: 10.0,
                    ..Look::default()
                }),
            )
            .unwrap_or_else(missing);
        let field = t
            .add(
                boxed,
                Style {
                    width: Length::Fill,
                    height: Length::Px(26.0),
                    ..Style::default()
                },
                Kind::TextField(Box::new(TextField::new(
                    lang.size_limit(),
                    lang.size_limit(),
                    theme.text.body,
                ))),
            )
            .unwrap_or_else(missing);
        t.set_field_value(field, &self.limit);
        self.limit_field = Some(field);
        let label = if self.exporting {
            lang.exporting()
        } else {
            lang.export()
        };
        let mut b = Button::new(label, ButtonKind::Primary, &theme);
        b.enabled = self.length.is_some() && !self.exporting;
        let id = t
            .add(
                bar,
                Style {
                    height: Length::Px(40.0),
                    padding: Insets::symmetric(0.0, 14.0),
                    ..Style::default()
                },
                Kind::Button(b),
            )
            .unwrap_or_else(missing);
        targets.push((id, Target::Export));

        // The frame, fitted above what was said and the strip.
        let strip_y = size.h - MARGIN - STRIP;
        let note_y = strip_y - 34.0;
        if let Some(note) = &self.note {
            t.add(
                root,
                Style {
                    at: Some(Point::new(MARGIN, note_y)),
                    width: Length::Px((size.w - 2.0 * MARGIN).max(1.0)),
                    ..Style::default()
                },
                Kind::text_line(note.clone(), theme.text.label.with_color(Paint::Muted)),
            )
            .unwrap_or_else(missing);
        }
        let room = Rect::new(
            MARGIN,
            BAR + MARGIN,
            (size.w - 2.0 * MARGIN).max(1.0),
            (note_y - BAR - MARGIN - 8.0).max(1.0),
        );
        self.room = room;
        self.preview_node = None;
        self.head_node = None;

        // The strip: thumbnails side by side, dimmed outside the cut.
        self.strip = Rect::new(MARGIN, strip_y, (size.w - 2.0 * MARGIN).max(1.0), STRIP);
        let s = self.strip;
        let strip = t
            .add(
                root,
                Style {
                    at: Some(Point::new(s.x, s.y)),
                    width: Length::Px(s.w),
                    height: Length::Px(s.h),
                    direction: Direction::Row,
                    justify: Justify::Start,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Surface),
                    radius: 8.0,
                    ..Look::default()
                }),
            )
            .unwrap_or_else(missing);
        if let Some(thumbs) = &self.thumbs {
            let n = thumbs.rgba.len().max(1) as f32;
            for rgba in &thumbs.rgba {
                let Some(image) = Kind::image(thumbs.width as u16, thumbs.height as u16, rgba, "")
                else {
                    continue;
                };
                t.add(
                    strip,
                    Style {
                        width: Length::Px(s.w / n),
                        height: Length::Px(s.h),
                        ..Style::default()
                    },
                    image,
                )
                .unwrap_or_else(missing);
            }
        }
        if let Some(length) = self.length {
            let x_of =
                |d: Duration| s.x + s.w * (d.as_secs_f32() / length.as_secs_f32().max(0.001));
            let (a, b) = (x_of(self.from), x_of(self.to));
            let dim = Look {
                background: Some(Paint::Rgba(hocus_ui::Color::rgba(0, 0, 0, 150))),
                ..Look::default()
            };
            for (x0, x1) in [(s.x, a), (b, s.x + s.w)] {
                if x1 > x0 {
                    t.add(
                        root,
                        Style {
                            at: Some(Point::new(x0, s.y)),
                            width: Length::Px(x1 - x0),
                            height: Length::Px(s.h),
                            ..Style::default()
                        },
                        Kind::Box(dim),
                    )
                    .unwrap_or_else(missing);
                }
            }
            // The play head: a thin line.
            let head = t
                .add(
                    root,
                    Style {
                        at: Some(Point::new(x_of(self.head) - 1.0, s.y - 6.0)),
                        width: Length::Px(2.0),
                        height: Length::Px(s.h + 12.0),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Text),
                        ..Look::default()
                    }),
                )
                .unwrap_or_else(missing);
            self.head_node = Some(head);
            for (x, name, which) in [
                (a, lang.trim_start(), Held::Start),
                (b, lang.trim_end(), Held::End),
            ] {
                let id = t
                    .add(
                        root,
                        Style {
                            at: Some(Point::new(x - HANDLE / 2.0, s.y - 4.0)),
                            width: Length::Px(HANDLE),
                            height: Length::Px(s.h + 8.0),
                            ..Style::default()
                        },
                        Kind::Box(Look {
                            background: Some(Paint::Accent),
                            // The one the arrow keys move.
                            border: (self.selected == which).then_some(Paint::Text),
                            radius: 4.0,
                            ..Look::default()
                        }),
                    )
                    .unwrap_or_else(missing);
                t.set_clickable(id, name);
            }
        }
        self.targets = targets.into_iter().collect();
        if had_keys {
            self.tree.set_focus(self.limit_field);
        }
        self.dirty = false;
        self.moved = true;
    }

    /// The frame and the play head where they are now, the rest left as
    /// it is.
    fn place_moving(&mut self) {
        let room = self.room;
        let t = &mut self.tree;
        if let Some(old) = self.preview_node.take() {
            t.remove(old);
        }
        if let Some((w, h, rgba)) = &self.preview {
            let k = (room.w / *w as f32).min(room.h / *h as f32);
            let (pw, ph) = (*w as f32 * k, *h as f32 * k);
            if let Some(image) = Kind::image(*w as u16, *h as u16, rgba, self.lang.trim()) {
                self.preview_node = t.add(
                    t.root(),
                    Style {
                        at: Some(Point::new(
                            room.x + (room.w - pw) / 2.0,
                            room.y + (room.h - ph) / 2.0,
                        )),
                        width: Length::Px(pw),
                        height: Length::Px(ph),
                        ..Style::default()
                    },
                    image,
                );
            }
        }
        if let Some(head) = self.head_node {
            let s = self.strip;
            let x = self.x_of(self.head);
            self.tree.set_style(
                head,
                Style {
                    at: Some(Point::new(x - 1.0, s.y - 6.0)),
                    width: Length::Px(2.0),
                    height: Length::Px(s.h + 12.0),
                    ..Style::default()
                },
            );
        }
        self.moved = false;
    }

    /// The time at `x` on the strip, within the recording.
    fn time_at(&self, x: f32) -> Duration {
        let Some(length) = self.length else {
            return Duration::ZERO;
        };
        let f = ((x - self.strip.x) / self.strip.w.max(1.0)).clamp(0.0, 1.0);
        snap(f64::from(f) * length.as_secs_f64()).min(length)
    }

    fn x_of(&self, d: Duration) -> f32 {
        let length = self.length.map_or(1.0, |l| l.as_secs_f32().max(0.001));
        self.strip.x + self.strip.w * (d.as_secs_f32() / length)
    }

    /// Where the pointer was last, and whether a handle is held: for
    /// tests, which wait for a drag to be taken.
    pub fn pointer(&self) -> (Option<Point>, bool) {
        (self.pointer, self.held.is_some())
    }

    pub fn pointer_move(&mut self, at: Point) -> Vec<TrimOutcome> {
        self.pointer = Some(at);
        let Some(held) = self.held else {
            self.tree.input(Input::PointerMove(at));
            return Vec::new();
        };
        let t = self.time_at(at.x);
        let step = Duration::from_secs_f64(STEP);
        match held {
            Held::Start => self.from = t.min(self.to.saturating_sub(step)),
            Held::End => self.to = t.max(self.from + step),
            Held::Head => {
                self.head = t;
                self.moved = true;
                return Vec::new();
            }
        }
        self.dirty = true;
        Vec::new()
    }

    pub fn pointer_button(&mut self, pressed: bool) -> Vec<TrimOutcome> {
        let at = self.pointer.unwrap_or_default();
        let s = self.strip;
        let on_strip = self.length.is_some()
            && at.y >= s.y - 8.0
            && at.y <= s.y + s.h + 8.0
            && at.x >= s.x - HANDLE
            && at.x <= s.x + s.w + HANDLE;
        if pressed && on_strip {
            let near = |d: Duration| (self.x_of(d) - at.x).abs() <= HANDLE;
            self.held = Some(if near(self.to) && !near(self.from) {
                Held::End
            } else if near(self.from) {
                Held::Start
            } else {
                Held::Head
            });
            let mut out = Vec::new();
            if self.playing {
                self.playing = false;
                out.push(TrimOutcome::Stop);
            }
            out.extend(self.pointer_move(at));
            return out;
        }
        if !pressed && let Some(held) = self.held.take() {
            if held != Held::Head {
                self.selected = held;
            }
            // The frame where it was let go.
            let at = match held {
                Held::Start => self.from,
                Held::End => self.to,
                Held::Head => self.head,
            };
            self.head = at;
            self.dirty = true;
            return vec![TrimOutcome::Seek(at)];
        }
        let events = self.tree.input(Input::PointerButton { pressed });
        self.handle(events)
    }

    pub fn key(&mut self, key: Key) -> Vec<TrimOutcome> {
        let typing = self.limit_field.is_some() && self.tree.focused() == self.limit_field;
        if key == Key::Escape && !typing {
            return vec![TrimOutcome::Close];
        }
        // The arrows move the handle last taken: left and right by a tenth
        // of a second, down and up by a second.
        let tenths: i64 = match key {
            Key::Left => -1,
            Key::Right => 1,
            Key::Down => -10,
            Key::Up => 10,
            _ => 0,
        };
        if tenths != 0
            && !typing
            && let Some(length) = self.length
        {
            let step = Duration::from_secs_f64(STEP);
            let moved = |d: Duration| snap(d.as_secs_f64() + tenths as f64 * STEP).min(length);
            let at = match self.selected {
                Held::End => {
                    self.to = moved(self.to).max(self.from + step);
                    self.to
                }
                _ => {
                    self.from = moved(self.from).min(self.to.saturating_sub(step));
                    self.from
                }
            };
            self.head = at;
            self.dirty = true;
            return vec![TrimOutcome::Seek(at)];
        }
        let events = self.tree.input(Input::Key(key));
        self.handle(events)
    }

    /// Typed text goes to the size limit when it has the keyboard; a
    /// space elsewhere plays or pauses.
    pub fn text(&mut self, typed: &str) -> Vec<TrimOutcome> {
        if self.limit_field.is_some() && self.tree.focused() == self.limit_field {
            let events = self.tree.text_input(typed);
            return self.handle(events);
        }
        if typed == " " {
            return self.play_pause();
        }
        Vec::new()
    }

    fn play_pause(&mut self) -> Vec<TrimOutcome> {
        if self.length.is_none() {
            return Vec::new();
        }
        self.playing = !self.playing;
        self.dirty = true;
        if self.playing {
            // From the play head when it is in the cut, else its start.
            let from = if self.head >= self.from && self.head < self.to {
                self.head
            } else {
                self.from
            };
            vec![TrimOutcome::Play(from, self.to)]
        } else {
            vec![TrimOutcome::Stop]
        }
    }

    /// The limit typed, in megabytes, as bytes; none when empty or not a
    /// number.
    fn limit_bytes(&self) -> Option<u64> {
        let text = self.limit.trim().replace(',', ".");
        let mb: f64 = text.parse().ok().filter(|v: &f64| *v > 0.0)?;
        Some((mb * 1_000_000.0) as u64)
    }

    fn handle(&mut self, events: Vec<Event>) -> Vec<TrimOutcome> {
        let mut out = Vec::new();
        for event in events {
            match event {
                Event::Changed(id) if Some(id) == self.limit_field => {
                    self.limit = self.tree.field_value(id).unwrap_or_default().to_owned();
                }
                // A field is activated by Enter or a click: not an export.
                Event::Activated(id) if Some(id) == self.limit_field => {}
                Event::Activated(id) => match self.targets.get(&id).copied() {
                    Some(Target::Format(format)) => {
                        self.format = format;
                        self.dirty = true;
                    }
                    Some(Target::PlayPause) => out.extend(self.play_pause()),
                    Some(Target::Export) if !self.exporting && self.length.is_some() => {
                        if self.playing {
                            self.playing = false;
                            out.push(TrimOutcome::Stop);
                        }
                        out.push(TrimOutcome::Export(Export {
                            from: self.from,
                            to: self.to,
                            format: self.format,
                            limit: self.limit_bytes(),
                        }));
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        out
    }

    pub fn draw(&mut self, now: Duration) -> Vec<PixelRect> {
        if self.dirty {
            self.build();
        }
        if self.moved {
            self.place_moving();
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

    fn view() -> TrimView {
        let mut v = TrimView::new(Theme::default(), Lang::En, Size::new(1000.0, 640.0), 1.0);
        v.set_length(Duration::from_secs(10));
        v.draw(Duration::ZERO);
        v
    }

    fn drag(v: &mut TrimView, from_x: f32, to_x: f32) -> Vec<TrimOutcome> {
        let y = v.strip.y + v.strip.h / 2.0;
        v.pointer_move(Point::new(from_x, y));
        v.pointer_button(true);
        v.pointer_move(Point::new(to_x, y));
        let out = v.pointer_button(false);
        v.draw(Duration::ZERO);
        out
    }

    #[test]
    fn the_handles_cut_to_the_tenth_of_a_second() {
        let mut v = view();
        let per_second = v.strip.w / 10.0;
        let start = v.x_of(Duration::ZERO);
        let out = drag(&mut v, start, start + 2.0 * per_second + 1.0);
        assert_eq!(out, vec![TrimOutcome::Seek(Duration::from_secs(2))]);
        let end = v.x_of(Duration::from_secs(10));
        drag(&mut v, end, end - 4.0 * per_second);
        assert_eq!(v.cut(), (Duration::from_secs(2), Duration::from_secs(6)));
        assert!(v.access().dump().contains("0:02.0 – 0:06.0 · 4.0 s"));
    }

    #[test]
    fn the_frame_shows_and_stays_through_a_rebuild() {
        let mut v = view();
        v.set_preview((4, 2, [9, 9, 9, 255].repeat(8)), None);
        v.draw(Duration::from_millis(16));
        let shows = |v: &TrimView| v.access().dump().contains("image \"Trim the recording\"");
        assert!(shows(&v), "{}", v.access().dump());
        v.set_note(Some("hello".into()));
        v.draw(Duration::from_millis(32));
        assert!(shows(&v), "{}", v.access().dump());
    }

    #[test]
    fn a_click_on_export_exports() {
        let mut v = view();
        let access = v.access();
        let node = access
            .walk()
            .into_iter()
            .find(|n| n.label.as_deref() == Some("Export"))
            .expect("an export button")
            .clone();
        let at = Point::new(
            node.rect.x + node.rect.w / 2.0,
            node.rect.y + node.rect.h / 2.0,
        );
        v.pointer_move(at);
        let mut out = v.pointer_button(true);
        // A frame comes in between.
        v.set_preview((4, 2, vec![0; 32]), None);
        v.draw(Duration::from_millis(16));
        out.extend(v.pointer_button(false));
        assert!(
            matches!(out.as_slice(), [TrimOutcome::Export(_)]),
            "{out:?}"
        );
    }

    #[test]
    fn the_arrows_move_the_handle_last_taken() {
        let mut v = view();
        for _ in 0..2 {
            v.key(Key::Up);
        }
        let end = v.x_of(Duration::from_secs(10));
        drag(&mut v, end, end);
        for _ in 0..4 {
            v.key(Key::Down);
        }
        v.key(Key::Right);
        v.key(Key::Left);
        assert_eq!(v.cut(), (Duration::from_secs(2), Duration::from_secs(6)));
    }

    #[test]
    fn export_asks_for_the_cut_its_format_and_limit() {
        let mut v = view();
        v.format = Format::Gif;
        v.limit = "5".into();
        let out = v.handle(vec![Event::Activated(
            *v.targets
                .iter()
                .find(|(_, t)| **t == Target::Export)
                .map(|(id, _)| id)
                .expect("an export button"),
        )]);
        assert_eq!(
            out,
            vec![TrimOutcome::Export(Export {
                from: Duration::ZERO,
                to: Duration::from_secs(10),
                format: Format::Gif,
                limit: Some(5_000_000),
            })]
        );
    }
}
