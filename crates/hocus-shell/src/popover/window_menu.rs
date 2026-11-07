//! The bar's menu of a window (#723, bar.md "The windows in the bar"): a
//! right click on a window's line (the full form's group, the mini form's
//! popover, a desktop's list) opens, beside the bar, the same actions as
//! the window's radial menu. 240 px wide: its application's icon and
//! title; Maximise (or Restore) and Minimise with their keys; Always on top
//! and On all desktops as switches; "ENVOYER SUR" with a row of tiles per
//! screen, shaped like it, the window's own desktop ringed and inert, a
//! click sending the window there; Close, in orange, with its key.
//!
//! The compositor knows the window's states and the screens: the shell
//! asks it as the right click comes (`window-menu ID`), and opens the menu
//! when it answers, at once. A switch flips at once and stays; the other
//! actions close the menu. `Tab` and the arrows go through it, `Enter` or
//! `Space` acts, `Esc` closes it.

use std::sync::{Mutex, PoisonError};

use hocus_link::{ToCompositor, WindowAction, WindowMenu};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke},
        text::Alignment,
    },
    window,
};

use super::{
    Effect, Event as Seen, Item, Pressed, event,
    kit::{self, Input, Pointer, text, text_in},
};
use crate::{
    clock::time::Lang,
    levels::draw::{ACCENT, DIM, FAINT, HOVER, MANROPE_BOLD, TEXT, ring},
    theme::Ink,
    windows::draw::{CLOSE_HOVER, CLOSE_INK},
};

pub const WIDTH: u32 = 240;
const BORDER: f32 = 1.0;
const PAD: f32 = 8.0;
/// The content's sides: rows run 8 px in, their words 10 px further.
const SIDE: f32 = BORDER + PAD;
const INNER: f32 = WIDTH as f32 - 2.0 * SIDE;
const ROW_IN: f32 = 10.0;
/// The header: the application's icon and its title.
const HEADER: f32 = 34.0;
const HEADER_ICON: f32 = 18.0;
const HEADER_TEXT: f32 = 13.5;
/// A row: 30 px, its icon 14 px, its words 13 px, its key 10.5 px.
const ROW: f32 = 30.0;
const ROW_RADIUS: f32 = 8.0;
const ROW_ICON: f32 = 14.0;
const ROW_TEXT: f32 = 13.0;
const KEY_TEXT: f32 = 10.5;
const ICON_GAP: f32 = 10.0;
/// Between the groups: a rule in 9 px.
const GAP: f32 = 9.0;
/// "ENVOYER SUR", a screen's name over its tiles, its tiles.
const SEND_LABEL: f32 = 22.0;
const CAPTION: f32 = 16.0;
const CAPTION_TEXT: f32 = 11.0;
const TILE_HEIGHT: f32 = 24.0;
const TILE_GAP: f32 = 6.0;
const TILE_RADIUS: f32 = 5.0;
const SCREEN_GAP: f32 = 6.0;
/// The switches: 30 × 17.
const SWITCH: Size = Size::new(30.0, 17.0);

const MONO: iced::Font = iced::Font::with_name("Martian Mono");
const TILE: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
const TILE_SHOWN: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.12));
const TILE_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.18));
const SWITCH_OFF: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.16));

/// What can be acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Maximize,
    Minimize,
    Above,
    AllDesktops,
    /// Desktop `desktop` of the menu's `screen`th screen.
    Tile {
        screen: usize,
        desktop: u8,
    },
    Close,
}

/// The menu's words, French or English.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    french: bool,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self {
            french: lang == Lang::French,
        }
    }

    fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        if self.french { fr } else { en }
    }

    pub fn name(self, target: Target, maximized: bool) -> &'static str {
        match target {
            Target::Maximize if maximized => self.pick("Restaurer", "Restore"),
            Target::Maximize => self.pick("Agrandir", "Maximise"),
            Target::Minimize => self.pick("Réduire", "Minimise"),
            Target::Above => self.pick("Au premier plan", "Always on top"),
            Target::AllDesktops => self.pick("Sur tous les bureaux", "On all desktops"),
            Target::Tile { .. } => "",
            Target::Close => self.pick("Fermer", "Close"),
        }
    }

    fn key(target: Target, maximized: bool) -> &'static str {
        match target {
            Target::Maximize if maximized => "Super+↓",
            Target::Maximize => "Super+↑",
            Target::Minimize => "Super+H",
            Target::Close => "Super+Q",
            _ => "",
        }
    }

    pub fn send_to(self) -> &'static str {
        self.pick("ENVOYER SUR", "SEND TO")
    }

    /// A screen's caption: the built-in one is "this laptop".
    pub fn screen(self, screen: &hocus_link::MenuScreen) -> String {
        if screen.internal {
            self.pick("ce portable", "this laptop").to_owned()
        } else {
            screen.name.clone()
        }
    }
}

/// The menu asked for, and the one showing.
#[derive(Debug, Default)]
struct Menus {
    /// Asked of the compositor: its bar, its window, its line on the bar
    /// (none from a popover's line: the menu takes that popover's place),
    /// the window's title and application.
    asked: Option<Asked>,
    shown: Option<Shown>,
}

#[derive(Debug, Clone)]
struct Asked {
    bar: String,
    id: u64,
    from: Option<Rectangle>,
    title: String,
    app: String,
}

/// What the menu shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub menu: WindowMenu,
    pub title: String,
    pub app: String,
}

static MENUS: Mutex<Option<Menus>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut Menus) -> T) -> T {
    let mut menus = MENUS.lock().unwrap_or_else(PoisonError::into_inner);
    f(menus.get_or_insert_with(Menus::default))
}

/// What the menu of window `id` shows, if it is that one's.
pub fn shown(id: u64) -> Option<Shown> {
    with(|m| m.shown.clone().filter(|s| s.menu.id == id))
}

/// A right click on window `id`'s line on `bar` (at `from` in its frame;
/// none from a popover's line): the compositor is asked what its menu
/// shows, and it opens with the answer.
pub fn ask(bar: &str, id: u64, from: Option<Rectangle>, title: &str, app: &str) {
    tracing::debug!(window = id, bar, "the bar asks for a window's menu");
    with(|m| {
        m.asked = Some(Asked {
            bar: bar.to_owned(),
            id,
            from,
            title: title.to_owned(),
            app: app.to_owned(),
        });
    });
    super::send(vec![Effect::Tell(ToCompositor::WindowMenu(id))]);
}

/// The compositor's answer: the menu opens beside the line asked from
/// (or in the popover's place); an answer for the menu showing refreshes
/// it.
pub fn answer(menu: WindowMenu) {
    let id = menu.id;
    let asked = with(|m| {
        let asked = m.asked.take_if(|a| a.id == id);
        match &asked {
            Some(a) => {
                m.shown = Some(Shown {
                    menu: menu.clone(),
                    title: a.title.clone(),
                    app: a.app.clone(),
                });
            }
            None => {
                if let Some(shown) = m.shown.as_mut().filter(|s| s.menu.id == id) {
                    shown.menu = menu.clone();
                }
            }
        }
        asked
    });
    let Some(asked) = asked else {
        super::send(vec![Effect::Wake(hocus_link::POPOVER_NAMESPACE)]);
        return;
    };
    match asked.from {
        Some(rect) => event(Seen::Open {
            bar: asked.bar,
            item: Item::WindowMenu(id),
            rect,
        }),
        None => event(Seen::Replace(Item::WindowMenu(id))),
    }
}

/// Where its parts are.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub targets: Vec<(Target, Rectangle)>,
    pub rules: Vec<f32>,
    /// "ENVOYER SUR"'s middle; each screen's caption's middle.
    pub send_to: f32,
    pub captions: Vec<f32>,
    pub height: f32,
}

/// A row across the content from `top`.
fn row(top: f32) -> Rectangle {
    Rectangle::new(Point::new(SIDE, top), Size::new(INNER, ROW))
}

/// The tiles of a screen `width` × `height` with `count` desktops, from
/// `top`, across the content: shaped like it, no taller than
/// [`TILE_HEIGHT`].
fn tiles(count: u8, (width, height): (u32, u32), top: f32) -> Vec<Rectangle> {
    let n = f32::from(count.max(1));
    let aspect = if height == 0 {
        16.0 / 10.0
    } else {
        width as f32 / height as f32
    };
    let room = (INNER - 2.0 * 4.0 - TILE_GAP * (n - 1.0)) / n;
    let w = room.min(TILE_HEIGHT * aspect);
    let h = (w / aspect).min(TILE_HEIGHT).round();
    let w = w.round();
    (0..count)
        .map(|i| {
            Rectangle::new(
                Point::new(SIDE + 4.0 + f32::from(i) * (w + TILE_GAP), top),
                Size::new(w, h),
            )
        })
        .collect()
}

/// The menu's parts for `menu`.
pub fn layout(menu: &WindowMenu) -> Layout {
    let mut targets = Vec::new();
    let mut rules = Vec::new();
    let mut y = BORDER + PAD + HEADER;
    for target in [Target::Maximize, Target::Minimize] {
        targets.push((target, row(y)));
        y += ROW;
    }
    rules.push(y + GAP / 2.0);
    y += GAP;
    for target in [Target::Above, Target::AllDesktops] {
        targets.push((target, row(y)));
        y += ROW;
    }
    rules.push(y + GAP / 2.0);
    y += GAP;
    let send_to = y + SEND_LABEL / 2.0;
    y += SEND_LABEL;
    let mut captions = Vec::new();
    for (i, screen) in menu.screens.iter().enumerate() {
        captions.push(y + CAPTION / 2.0);
        y += CAPTION;
        let rects = tiles(screen.count, (screen.width, screen.height), y);
        let h = rects.first().map_or(TILE_HEIGHT, |r| r.height);
        for (d, rect) in rects.into_iter().enumerate() {
            targets.push((
                Target::Tile {
                    screen: i,
                    desktop: u8::try_from(d).unwrap_or(u8::MAX),
                },
                rect,
            ));
        }
        y += h + SCREEN_GAP;
    }
    rules.push(y + GAP / 2.0);
    y += GAP;
    targets.push((Target::Close, row(y)));
    y += ROW;
    Layout {
        targets,
        rules,
        send_to,
        captions,
        height: y + PAD + BORDER,
    }
}

/// Its height for what it shows now.
pub fn height(id: u64) -> u32 {
    shown(id).map_or(200, |s| layout(&s.menu).height.round() as u32)
}

/// Whether `target` acts: a tile of the window's own desktop does not.
fn acts(menu: &WindowMenu, target: Target) -> bool {
    match target {
        Target::Tile { screen, desktop } => menu
            .screens
            .get(screen)
            .is_some_and(|s| s.here != Some(desktop)),
        _ => true,
    }
}

/// What the menu shows, said for the log (and the tests).
pub fn said(words: Words, menu: &WindowMenu) -> String {
    let switch = |on: bool| if on { "on" } else { "off" };
    let screens: Vec<String> = menu
        .screens
        .iter()
        .map(|s| {
            let tiles: Vec<String> = (0..s.count)
                .map(|d| {
                    let mut tile = (d + 1).to_string();
                    if s.here == Some(d) {
                        tile.push('*');
                    }
                    tile
                })
                .collect();
            format!("{} [{}]", words.screen(s), tiles.join(" "))
        })
        .collect();
    format!(
        "{}; {}; {}={}; {}={}; {} {}; {}",
        words.name(Target::Maximize, menu.maximized),
        words.name(Target::Minimize, false),
        words.name(Target::Above, false),
        switch(menu.above),
        words.name(Target::AllDesktops, false),
        switch(menu.all_desktops),
        words.send_to(),
        screens.join(" | "),
        words.name(Target::Close, false),
    )
}

/// The menu on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub window: u64,
    pub words: Words,
    pub glass: bool,
}

#[derive(Debug, Default)]
pub struct State {
    pointer: Pointer<Target>,
    focus: Option<Target>,
    said: Option<String>,
}

impl<Message> canvas::Program<Message> for Panel {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match kit::input(self.id, event, bounds, cursor, &mut state.pointer) {
            Input::Redraw(_) => {
                for pressed in kit::keys(self.id) {
                    let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
                }
                self.say(state);
                None
            }
            Input::Mouse(seen, at) => self.mouse(state, seen, at),
            Input::Key(pressed) => self.key(state, &pressed),
            Input::Nothing => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        kit::background(&mut frame, bounds.size(), self.glass);
        if let Some(shown) = shown(self.window) {
            self.draw_menu(&mut frame, state, &shown);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        state.pointer.interaction(false)
    }
}

impl Panel {
    fn draw_menu(&self, f: &mut Frame, state: &State, shown: &Shown) {
        let menu = &shown.menu;
        let layout = layout(menu);
        // The header: the application's icon and the window's title.
        let middle = BORDER + PAD + HEADER / 2.0;
        let icon = Rectangle::new(
            Point::new(SIDE + ROW_IN - 2.0, middle - HEADER_ICON / 2.0),
            Size::new(HEADER_ICON, HEADER_ICON),
        );
        let found = crate::desktops::icons::of(&shown.app);
        let name = shown
            .app
            .rsplit('.')
            .next()
            .filter(|n| !n.is_empty())
            .unwrap_or(&shown.title);
        kit::app_tile(f, icon, (found.as_ref(), name), (4.0, 10.0), 1.0);
        let x = icon.x + HEADER_ICON + ICON_GAP;
        let room = WIDTH as f32 - SIDE - ROW_IN - x;
        let title = kit::elide(&shown.title, room, |t| {
            kit::measure(t, HEADER_TEXT, MANROPE_BOLD)
        });
        f.fill_text(text_in(
            MANROPE_BOLD,
            text(
                title,
                Point::new(x, middle),
                HEADER_TEXT,
                TEXT.get(),
                Alignment::Left,
            ),
        ));
        for y in &layout.rules {
            f.fill_rectangle(
                Point::new(SIDE + ROW_IN, y.round()),
                Size::new(INNER - 2.0 * ROW_IN, 1.0),
                Color::from_rgba8(255, 255, 255, 0.08),
            );
        }
        // "ENVOYER SUR", spaced capitals, and each screen's caption.
        let mut x = SIDE + ROW_IN;
        for ch in self.words.send_to().chars() {
            let s = ch.to_string();
            f.fill_text(text_in(
                MANROPE_BOLD,
                text(
                    s.clone(),
                    Point::new(x, layout.send_to),
                    CAPTION_TEXT,
                    DIM.get(),
                    Alignment::Left,
                ),
            ));
            x += kit::measure(&s, CAPTION_TEXT, MANROPE_BOLD) + 0.9;
        }
        for (screen, y) in menu.screens.iter().zip(&layout.captions) {
            f.fill_text(text(
                self.words.screen(screen),
                Point::new(SIDE + 4.0, *y),
                CAPTION_TEXT,
                FAINT.get(),
                Alignment::Left,
            ));
        }
        for (target, rect) in &layout.targets {
            let hover = state.pointer.hover == Some(*target) && acts(menu, *target);
            match target {
                Target::Tile { screen, desktop } => {
                    let Some(s) = menu.screens.get(*screen) else {
                        continue;
                    };
                    let fill = if hover {
                        TILE_HOVER
                    } else if s.current == *desktop {
                        TILE_SHOWN
                    } else {
                        TILE
                    };
                    f.fill(&kit::rounded(*rect, TILE_RADIUS), fill.get());
                    if s.here == Some(*desktop) {
                        f.stroke(
                            &kit::rounded(*rect, TILE_RADIUS),
                            Stroke::default().with_color(ACCENT.get()).with_width(1.5),
                        );
                    }
                    f.fill_text(text_in(
                        MONO,
                        text(
                            (desktop + 1).to_string(),
                            rect.center(),
                            KEY_TEXT,
                            DIM.get(),
                            Alignment::Center,
                        ),
                    ));
                }
                _ => self.draw_row(f, *target, *rect, menu, hover),
            }
        }
        if let Some((target, rect)) = state
            .focus
            .and_then(|focus| layout.targets.iter().find(|(t, _)| *t == focus))
        {
            let radius = if matches!(target, Target::Tile { .. }) {
                TILE_RADIUS
            } else {
                ROW_RADIUS
            };
            ring(f, *rect, radius);
        }
    }

    /// A row: lit under the pointer (Close in orange), its icon, its
    /// words, its key or its switch.
    fn draw_row(
        &self,
        f: &mut Frame,
        target: Target,
        rect: Rectangle,
        menu: &WindowMenu,
        hover: bool,
    ) {
        let close = target == Target::Close;
        if hover {
            let ground = if close { CLOSE_HOVER } else { HOVER };
            f.fill(&kit::rounded(rect, ROW_RADIUS), ground.get());
        }
        let ink = if close { CLOSE_INK.get() } else { TEXT.get() };
        let icon = Rectangle::new(
            Point::new(rect.x + ROW_IN, rect.center_y() - ROW_ICON / 2.0),
            Size::new(ROW_ICON, ROW_ICON),
        );
        row_icon(f, target, icon, ink);
        f.fill_text(text(
            self.words.name(target, menu.maximized).to_owned(),
            Point::new(icon.x + ROW_ICON + ICON_GAP, rect.center_y()),
            ROW_TEXT,
            ink,
            Alignment::Left,
        ));
        let right = rect.x + rect.width - ROW_IN;
        match target {
            Target::Above | Target::AllDesktops => {
                let on = if target == Target::Above {
                    menu.above
                } else {
                    menu.all_desktops
                };
                let at = Rectangle::new(
                    Point::new(right - SWITCH.width, rect.center_y() - SWITCH.height / 2.0),
                    SWITCH,
                );
                let track = if on { ACCENT.get() } else { SWITCH_OFF.get() };
                f.fill(&kit::rounded(at, SWITCH.height / 2.0), track);
                let knob = SWITCH.height - 4.0;
                let x = if on {
                    at.x + at.width - 2.0 - knob / 2.0
                } else {
                    at.x + 2.0 + knob / 2.0
                };
                f.fill(
                    &Path::circle(Point::new(x, at.center_y()), knob / 2.0),
                    TEXT.get(),
                );
            }
            _ => {
                f.fill_text(text_in(
                    MONO,
                    text(
                        Words::key(target, menu.maximized).to_owned(),
                        Point::new(right, rect.center_y()),
                        KEY_TEXT,
                        FAINT.get(),
                        Alignment::Right,
                    ),
                ));
            }
        }
    }

    /// What it shows, said once a change: its words, and where its
    /// targets' middles are in its frame (tests click them).
    fn say(&self, state: &mut State) {
        let Some(shown) = shown(self.window) else {
            return;
        };
        let layout = layout(&shown.menu);
        let at: Vec<String> = layout
            .targets
            .iter()
            .map(|(t, r)| {
                let c = r.center();
                format!("{t:?}@{:.0},{:.0}", c.x, c.y).replace(' ', "")
            })
            .collect();
        let said = format!(
            "window={} shows={} targets={}",
            shown.menu.id,
            said(self.words, &shown.menu),
            at.join(" ")
        );
        if state.said.as_ref() != Some(&said) {
            tracing::debug!("the window's menu shows it: {said}");
            state.said = Some(said);
        }
    }

    /// `target` acted on: a switch flips and stays, the rest closes the
    /// menu.
    fn act(&self, state: &mut State, target: Target) {
        let Some(shown) = shown(self.window) else {
            return;
        };
        let menu = &shown.menu;
        if !acts(menu, target) {
            return;
        }
        let id = menu.id;
        let tell = |message| super::send(vec![Effect::Tell(message)]);
        let action = |action| ToCompositor::WindowAction { id, action };
        match target {
            Target::Maximize => tell(action(WindowAction::Maximize)),
            Target::Minimize => tell(action(WindowAction::Minimize)),
            Target::Close => tell(action(WindowAction::Close)),
            Target::Above | Target::AllDesktops => {
                with(|m| {
                    if let Some(s) = m.shown.as_mut().filter(|s| s.menu.id == id) {
                        if target == Target::Above {
                            s.menu.above = !s.menu.above;
                        } else {
                            s.menu.all_desktops = !s.menu.all_desktops;
                        }
                    }
                });
                tell(action(if target == Target::Above {
                    WindowAction::Above
                } else {
                    WindowAction::AllDesktops
                }));
                self.say(state);
                return;
            }
            Target::Tile { screen, desktop } => {
                let Some(s) = menu.screens.get(screen) else {
                    return;
                };
                tell(ToCompositor::SendTo {
                    id,
                    output: s.output.clone(),
                    desktop,
                });
            }
        }
        tracing::debug!(window = id, ?target, "the window's menu acts");
        event(Seen::Chosen);
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let shown = shown(self.window)?;
        let layout = layout(&shown.menu);
        let target = at.and_then(|at| {
            layout
                .targets
                .iter()
                .find(|(t, r)| r.contains(at) && acts(&shown.menu, *t))
                .map(|(t, _)| *t)
        });
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                state.pointer.moved();
                state.pointer.over(target).then_some(redraw).flatten()
            }
            mouse::Event::CursorLeft => {
                state.pointer.left();
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                state.focus = None;
                state.pointer.pressed = target;
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if let Some(target) = state.pointer.release(target) {
                    self.act(state, target);
                }
                redraw
            }
            _ => None,
        }
    }

    fn key<Message>(
        &self,
        state: &mut State,
        pressed: &Pressed,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        let shown = shown(self.window)?;
        let order: Vec<Target> = layout(&shown.menu)
            .targets
            .into_iter()
            .map(|(t, _)| t)
            .filter(|t| acts(&shown.menu, *t))
            .collect();
        match &pressed.key {
            Key::Named(Named::Escape) => {
                event(Seen::Escape);
                return Some(canvas::Action::capture());
            }
            Key::Named(Named::Tab) => {
                state.focus = kit::step(&order, state.focus, pressed.modifiers.shift());
            }
            Key::Named(
                arrow @ (Named::ArrowDown | Named::ArrowUp | Named::ArrowLeft | Named::ArrowRight),
            ) => {
                let forward = matches!(arrow, Named::ArrowDown | Named::ArrowRight);
                state.focus = match state.focus {
                    Some(focus) => kit::cycle(&order, focus, forward),
                    None => kit::step(&order, None, !forward),
                };
            }
            Key::Named(Named::Enter | Named::Space) => {
                if let Some(target) = state.focus {
                    self.act(state, target);
                }
            }
            _ => return None,
        }
        Some(canvas::Action::request_redraw().and_capture())
    }
}

/// A row's icon, the radial menu's, in `rect`.
fn row_icon(f: &mut Frame, target: Target, rect: Rectangle, ink: Color) {
    let k = rect.width / 24.0;
    let p = |x: f32, y: f32| Point::new(rect.x + x * k, rect.y + y * k);
    let r = |x: f32, y: f32, w: f32, h: f32, radius: f32| {
        kit::rounded(Rectangle::new(p(x, y), Size::new(w * k, h * k)), radius * k)
    };
    let stroke = |width: f32, colour: Color| {
        Stroke::default()
            .with_color(colour)
            .with_width(width * k)
            .with_line_cap(canvas::LineCap::Round)
            .with_line_join(canvas::LineJoin::Round)
    };
    let back = Color {
        a: ink.a * 0.55,
        ..ink
    };
    let line = |f: &mut Frame, points: &[(f32, f32)], width: f32| {
        let path = Path::new(|b| {
            for (i, (x, y)) in points.iter().enumerate() {
                if i == 0 {
                    b.move_to(p(*x, *y));
                } else {
                    b.line_to(p(*x, *y));
                }
            }
        });
        f.stroke(&path, stroke(width, ink));
    };
    match target {
        Target::Maximize => f.stroke(&r(5.0, 5.0, 14.0, 14.0, 2.2), stroke(1.9, ink)),
        Target::Minimize => line(f, &[(6.0, 17.0), (18.0, 17.0)], 2.1),
        Target::Above => {
            f.stroke(&r(4.0, 10.0, 11.0, 9.0, 2.0), stroke(1.7, back));
            f.fill(
                &r(9.0, 5.0, 11.0, 9.0, 2.0),
                Color {
                    a: ink.a * 0.25,
                    ..ink
                },
            );
            f.stroke(&r(9.0, 5.0, 11.0, 9.0, 2.0), stroke(1.9, ink));
        }
        Target::AllDesktops => {
            for x in [3.0, 9.5, 16.0] {
                f.stroke(&r(x, 6.0, 5.0, 12.0, 1.5), stroke(1.7, ink));
            }
            line(f, &[(5.5, 12.0), (18.5, 12.0)], 2.2);
        }
        Target::Close => {
            line(f, &[(7.0, 7.0), (17.0, 17.0)], 2.1);
            line(f, &[(17.0, 7.0), (7.0, 17.0)], 2.1);
        }
        Target::Tile { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use hocus_link::MenuScreen;

    use super::*;

    fn screen(output: &str, count: u8, size: (u32, u32), here: Option<u8>) -> MenuScreen {
        MenuScreen {
            output: output.into(),
            count,
            current: 0,
            width: size.0,
            height: size.1,
            here,
            internal: output.starts_with("eDP"),
            name: output.into(),
        }
    }

    fn menu(screens: Vec<MenuScreen>) -> WindowMenu {
        WindowMenu {
            id: 7,
            maximized: false,
            above: true,
            all_desktops: false,
            screens,
        }
    }

    #[test]
    fn its_rows_its_tiles_per_screen_and_close_last() {
        let one = layout(&menu(vec![screen("eDP-1", 4, (1707, 1067), Some(0))]));
        let two = layout(&menu(vec![
            screen("eDP-1", 4, (1707, 1067), Some(0)),
            screen("DP-2", 3, (3440, 1440), None),
        ]));
        let kinds: Vec<Target> = one.targets.iter().map(|(t, _)| *t).collect();
        assert_eq!(
            kinds[..4],
            [
                Target::Maximize,
                Target::Minimize,
                Target::Above,
                Target::AllDesktops
            ]
        );
        assert_eq!(kinds.last(), Some(&Target::Close));
        assert_eq!(kinds.len(), 4 + 4 + 1);
        assert_eq!(two.targets.len(), 4 + 4 + 3 + 1);
        // A second screen's caption and row make it taller.
        assert!(two.height > one.height + CAPTION);
        // Rows are 30 px; the tiles fit across, shaped like their screen.
        for (t, r) in &two.targets {
            assert!(
                r.x >= SIDE && r.x + r.width <= WIDTH as f32 - SIDE,
                "{t:?} {r:?}"
            );
            if let Target::Tile { screen, .. } = t {
                let aspect = r.width / r.height;
                let want = if *screen == 0 {
                    1707.0 / 1067.0
                } else {
                    3440.0 / 1440.0
                };
                assert!((aspect - want).abs() < 0.2, "{t:?}: {aspect}");
            } else {
                assert_eq!(r.height, ROW);
            }
        }
    }

    #[test]
    fn the_windows_own_desktop_is_inert_and_said_ringed() {
        let m = menu(vec![
            screen("eDP-1", 4, (1707, 1067), Some(1)),
            screen("DP-2", 2, (2560, 1440), None),
        ]);
        assert!(!acts(
            &m,
            Target::Tile {
                screen: 0,
                desktop: 1
            }
        ));
        assert!(acts(
            &m,
            Target::Tile {
                screen: 0,
                desktop: 0
            }
        ));
        assert!(acts(
            &m,
            Target::Tile {
                screen: 1,
                desktop: 1
            }
        ));
        let fr = Words { french: true };
        assert_eq!(
            said(fr, &m),
            "Agrandir; Réduire; Au premier plan=on; Sur tous les bureaux=off; \
             ENVOYER SUR ce portable [1 2* 3 4] | DP-2 [1 2]; Fermer"
        );
        let en = Words { french: false };
        let maximized = WindowMenu {
            maximized: true,
            ..m
        };
        assert!(said(en, &maximized).starts_with("Restore; Minimise; Always on top=on"));
    }
}
