//! The camera and microphone's window, « CAMÉRA ET MICRO » (#619,
//! docs/design/camera.md), opened by a click on the camera's tile of the
//! in-use sign, as the bar's popovers (§9), 340 wide, 14 px in, its parts
//! 9 px apart, as the approved mockup draws it:
//!
//! - its label (11 px, 700, spaced 0.1 em, `text.faint`) and who reads the
//!   camera and the microphone ("Utilisée par Firefox", 11.5 px);
//! - the preview, 312 × 176, radius 10, on `#1A1F2C`: what the others
//!   see, read from the Hocuspanda source while the window shows
//!   (`crate::camera::preview`);
//! - the tabs, Image · Fond · Visage · Voix · Réactions (26 px, radius 7,
//!   in a 2 px track, radius 9), a violet dot (`#B69CFF`, 5 px) on a tab
//!   with an effect on;
//! - the tab's grid, from the one list of effects
//!   (`crate::camera::effects`): 4 a row, 6 px apart, each tile 62 tall,
//!   radius 9, its thumbnail 34 × 22 (radius 5) over its name (10.5 px,
//!   600); chosen, ringed blue (`rgba(143,179,255,0.65)` on `0.20`). Those
//!   this version does not make are dimmed and say "Bientôt"; then the
//!   dashed "+ add-ons" tile (inert until add-ons, #442), and a hint line
//!   under the grid (11 px);
//! - a rule, the screen light's switch ("Lumière d'écran", "auto, sur ton
//!   visage" or its strength), and the help's link (`#8FB3FF`), which
//!   shows how to choose « Hocuspanda » in a call (the InCall board),
//!   with its way back, and where it works: everywhere with the
//!   v4l2loopback device labelled "Hocuspanda" (looked for in sysfs as the
//!   page opens), else what is needed.
//!
//! Built with the shell's widget kit ([`crate::ui`]): stock widgets on the
//! kit's surface; canvases only where the look draws (the thumbnails, the
//! dashed tile, the preview's frames).

use std::{
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use hocus_a11y::Role;
use hocus_link::CameraShown;
use hocus_words::{Text, shell::camera as w};
use iced::{
    Background, Border, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme,
    alignment::{Horizontal, Vertical},
    mouse,
    widget::{
        Space, button,
        canvas::{self, Frame, Path, Stroke, gradient},
        column, container, row, stack,
    },
    window,
};

use super::{
    Item, Pressed,
    kit::{self, MANROPE_SEMIBOLD, SWITCH},
};
use crate::{
    camera::{
        self,
        effects::{self, Effect, Tab, Thumb},
    },
    clock::time::Lang,
    levels::draw::{FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
    theme::{DIM, Ink},
    ui::{
        Around, control,
        parts::{Knob, drawn, gap, spaced_by},
        style::{self, said},
    },
};

type El<'a> = Element<'a, Msg>;

pub const WIDTH: u32 = 340;
const BORDER: f32 = kit::BORDER;
const PAD: f32 = 14.0;
const GAP: f32 = 9.0;
const LABEL: f32 = 11.0;
const READERS: f32 = 11.5;
const PREVIEW: Size = Size::new(312.0, 176.0);
const PREVIEW_RADIUS: f32 = 10.0;
const TAB: f32 = 26.0;
const TAB_RADIUS: f32 = 7.0;
const TRACK_PAD: f32 = 2.0;
const TRACK_RADIUS: f32 = 9.0;
const TAB_TEXT: f32 = 11.5;
const DOT: f32 = 5.0;
const DOT_IN: f32 = 4.0;
const PER_ROW: usize = 4;
const TILE: f32 = 62.0;
const TILE_GAP: f32 = 6.0;
const TILE_RADIUS: f32 = 9.0;
const THUMB: Size = Size::new(34.0, 22.0);
const THUMB_RADIUS: f32 = 5.0;
const TILE_TEXT: f32 = 10.5;
const PLUS: f32 = 18.0;
const ADDONS: f32 = 10.0;
const HINT: f32 = 11.0;
const HINT_HEIGHT: f32 = 15.0;
const LIGHT: f32 = 12.5;
const LIGHT_DETAIL: f32 = 11.5;
const LINK: f32 = 12.0;
/// The help's measures (the InCall board's, at the window's width).
const TITLE: f32 = 15.0;
const BODY: f32 = 12.5;
const BODY_LINE: f32 = 1.5;
const SMALL: f32 = 12.0;
const ROW: f32 = 34.0;
/// Frames come at most this often.
const FRAME: Duration = Duration::from_millis(33);
/// Without a frame, the preview looks again this often.
const WAITING: Duration = Duration::from_millis(250);
/// Dimmed: what this version does not make.
const SOON: f32 = 0.4;

const SURFACE: Ink = Ink::Dark(rgb(0x1A1F2C));
const TRACK: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.05));
const TAB_ON: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.12));
const TAB_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
const VIOLET: Ink = Ink::Dark(rgb(0xB69CFF));
const CHOSEN: Ink = Ink::Dark(Color::from_rgba8(143, 179, 255, 0.20));
const CHOSEN_RIM: Ink = Ink::Dark(Color::from_rgba8(143, 179, 255, 0.65));
const TILE_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.04));
const TILE_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
const TILE_RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const DASHED: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.22));
const SEP: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
const SOFT_TEXT: Ink = Ink::Dark(rgb(0xC9D0DC));
const LINK_BLUE: Ink = Ink::Dark(rgb(0x8FB3FF));
const LIST: Ink = Ink::Dark(rgb(0x252C3D));
const LIST_RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const LIST_ON: Ink = Ink::Dark(Color::from_rgba8(143, 179, 255, 0.16));
const BOX: Ink = Ink::Dark(rgb(0x1B2130));
const BOX_RIM: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
const YES: Ink = Ink::Dark(rgb(0x7EE0A0));
const MAYBE: Ink = Ink::Dark(rgb(0xF5B44A));

/// What holds the keyboard's ring, and what a control does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Tab(Tab),
    Tile(&'static str),
    Light,
    Help,
    Back,
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        match self {
            Self::Tile(id) => format!("camera:tile:{id}"),
            other => format!("camera:{other:?}"),
        }
    }
}

/// Which page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Page {
    #[default]
    Effects,
    /// How to choose « Hocuspanda » in a call.
    Help,
}

#[derive(Debug)]
struct Ui {
    page: Page,
    tab: Tab,
    focus: Option<Target>,
    switch: kit::Switch,
    said: Option<String>,
    /// The v4l2loopback device is there, as the help page last opened.
    loopback: bool,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            page: Page::Effects,
            tab: Tab::Image,
            focus: None,
            switch: kit::Switch::default(),
            said: None,
            loopback: false,
        }
    }
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// The height it took when last laid out.
static NATURAL: AtomicU32 = AtomicU32::new(0);

/// The window opens: its first tab, nothing holds the ring, the preview
/// reads.
pub fn opening() {
    ui(|ui| *ui = Ui::default());
    camera::preview::start();
}

/// It closes: the preview no longer reads.
pub fn closing() {
    camera::preview::stop();
}

/// Its height, logical pixels: as the widgets took it, else as counted.
pub fn height() -> u32 {
    let natural = f32::from_bits(NATURAL.load(Ordering::Relaxed));
    if natural > 0.0 {
        return natural.round() as u32;
    }
    let rows = ui(|ui| ui.tab).effects().count().div_ceil(PER_ROW).max(1) as f32;
    (2.0 * (BORDER + PAD)
        + LABEL * LINE
        + PREVIEW.height
        + TAB
        + 2.0 * TRACK_PAD
        + rows * TILE
        + (rows - 1.0) * TILE_GAP
        + HINT_HEIGHT
        + 1.0
        + SWITCH.height
        + LINK * LINE
        + 7.0 * GAP)
        .round() as u32
}

/// The window on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub glass: bool,
    pub motion: bool,
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    Act(Target),
    Focus(Target),
    Pressed,
    Stale,
}

pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let motion = panel.motion;
    Around::new(panel.id, build(motion), handle, Msg::Stale)
        .rebuild(move || build(motion))
        .press(Msg::Pressed)
        .keys(taken)
        .natural(|height| {
            let was = NATURAL.swap(height.to_bits(), Ordering::Relaxed);
            if (f32::from_bits(was) - height).abs() > 0.5 {
                crate::popover::refit(Item::Camera);
            }
        })
        .on_glass(panel.glass)
}

fn build<'a>(motion: bool) -> El<'a> {
    let lang = Lang::of_messages();
    let shown = camera::shown();
    let readers = readers(lang);
    ui(|ui| {
        ui.switch.settle(shown.screen_light, Instant::now());
        say(lang, &shown, &readers, ui);
        let page = match ui.page {
            Page::Effects => effects_page(lang, &shown, &readers, ui, motion),
            Page::Help => help_page(lang, ui.focus, ui.loopback),
        };
        container(page)
            .padding(BORDER + PAD)
            .width(Length::Fill)
            .into()
    })
}

/// "Utilisée par Firefox": the applications using the camera or the
/// microphone now; "Pas utilisée".
fn readers(lang: Lang) -> String {
    let uses = crate::in_use::snapshot();
    let mut apps: Vec<String> = Vec::new();
    for seen in uses.camera.iter().chain(&uses.microphone) {
        if seen.live() && !apps.contains(&seen.what.app) {
            apps.push(seen.what.app.clone());
        }
    }
    if apps.is_empty() {
        return w::NOBODY_READS.get(lang).to_owned();
    }
    w::read_by(lang, apps.join(", "))
}

/// The label, and who reads.
fn head<'a>(lang: Lang, aside: El<'a>) -> El<'a> {
    row![
        spaced_by(w::HEADING.get(lang), LABEL, MANROPE_BOLD, FAINT.get(), 0.1),
        Space::new().width(Length::Fill),
        aside,
    ]
    .align_y(Vertical::Center)
    .into()
}

fn effects_page<'a>(
    lang: Lang,
    shown: &CameraShown,
    readers: &str,
    ui: &Ui,
    motion: bool,
) -> El<'a> {
    let reading = said(readers.to_owned(), READERS, MANROPE, FAINT.get());
    column![
        head(lang, reading.into()),
        preview(lang, shown),
        tabs(lang, shown, ui),
        grid(lang, shown, ui),
        container(said(
            effects::hint(ui.tab, shown, lang),
            HINT,
            MANROPE,
            FAINT.get()
        ))
        .height(HINT_HEIGHT)
        .align_y(Vertical::Center),
        rule(),
        light(lang, shown, ui, motion),
        help_link(lang, ui.focus),
    ]
    .spacing(GAP)
    .into()
}

/// The preview: the last frame, else what keeps it.
fn preview<'a>(lang: Lang, shown: &CameraShown) -> El<'a> {
    let waiting = if !shown.ready {
        Some(w::NOT_THERE)
    } else if shown.source.is_none() && camera::preview::frame().is_some() {
        Some(w::NO_CAMERA)
    } else if camera::preview::frame().is_none() {
        Some(w::STARTING)
    } else {
        None
    };
    let mut layers = stack![
        container(Space::new())
            .width(PREVIEW.width)
            .height(PREVIEW.height)
            .style(|_| style::boxed(SURFACE.get(), PREVIEW_RADIUS)),
        iced::widget::canvas(Frames)
            .width(PREVIEW.width)
            .height(PREVIEW.height),
    ];
    if let Some(waiting) = waiting {
        layers = layers.push(
            container(said(waiting.get(lang), SMALL, MANROPE, FAINT.get()))
                .width(PREVIEW.width)
                .height(PREVIEW.height)
                .center(Length::Fill),
        );
    }
    crate::ui::line(layers, w::PREVIEW.get(lang)).into()
}

/// The preview's frames, drawn as they come.
struct Frames;

impl<M> canvas::Program<M> for Frames {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        let iced::Event::Window(iced::window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        let next = match camera::preview::frame() {
            Some(_) => FRAME,
            None => WAITING,
        };
        Some(canvas::Action::request_redraw_at(*now + next))
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let Some((handle, _)) = camera::preview::frame() else {
            return Vec::new();
        };
        let mut f = Frame::new(renderer, bounds.size());
        let mut image = canvas::Image::new(handle);
        image.border_radius = PREVIEW_RADIUS.into();
        f.draw_image(Rectangle::new(Point::ORIGIN, bounds.size()), image);
        vec![f.into_geometry()]
    }
}

/// The tabs, in their track.
fn tabs<'a>(lang: Lang, shown: &CameraShown, ui: &Ui) -> El<'a> {
    let mut all = row![].spacing(TRACK_PAD);
    for tab in Tab::ALL {
        let chosen = ui.tab == tab;
        let word = tab.name().get(lang);
        let ink = if chosen { TEXT } else { FAINT };
        let mut face = stack![
            container(said(word, TAB_TEXT, MANROPE_BOLD, ink.get()))
                .width(Length::Fill)
                .height(TAB)
                .center(Length::Fill)
        ];
        if tab.on(shown) {
            face = face.push(
                container(
                    container(Space::new().width(DOT).height(DOT))
                        .style(|_| style::boxed(VIOLET.get(), DOT / 2.0)),
                )
                .width(Length::Fill)
                .height(TAB)
                .align_x(Horizontal::Right)
                .align_y(Vertical::Top)
                .padding([DOT_IN, DOT_IN]),
            );
        }
        let target = Target::Tab(tab);
        let pressed = button(face)
            .padding(0)
            .width(Length::Fill)
            .height(TAB)
            .style(move |_, status| {
                let fill = if chosen {
                    Some(TAB_ON.get().into())
                } else if style::lit(status) {
                    Some(TAB_HOVER.get().into())
                } else {
                    None
                };
                button::Style {
                    background: fill,
                    border: style::round(TAB_RADIUS),
                    ..button::Style::default()
                }
            })
            .on_press(Msg::Act(target));
        all = all.push(
            control(pressed, target.id(), Role::RadioButton, word)
                .node(move |n| n.checked(chosen))
                .focus(ui.focus == Some(target), Msg::Focus(target), TAB_RADIUS)
                .activate(Msg::Act(target)),
        );
    }
    container(all)
        .padding(TRACK_PAD)
        .width(Length::Fill)
        .style(|_| style::boxed(TRACK.get(), TRACK_RADIUS))
        .into()
}

/// The tab's tiles, 4 a row, then the add-ons' tile.
fn grid<'a>(lang: Lang, shown: &CameraShown, ui: &Ui) -> El<'a> {
    let mut cells: Vec<El<'a>> = ui
        .tab
        .effects()
        .map(|effect| tile(lang, shown, effect, ui.focus))
        .collect();
    cells.push(addons(lang));
    let mut rows = column![].spacing(TILE_GAP);
    let mut cells = cells.into_iter().peekable();
    while cells.peek().is_some() {
        let mut line = row![].spacing(TILE_GAP);
        for _ in 0..PER_ROW {
            line = line.push(match cells.next() {
                Some(cell) => cell,
                None => Space::new().width(Length::Fill).height(TILE).into(),
            });
        }
        rows = rows.push(line);
    }
    rows.into()
}

fn tile<'a>(
    lang: Lang,
    shown: &CameraShown,
    effect: &'static Effect,
    focus: Option<Target>,
) -> El<'a> {
    let on = effect.on(shown);
    let ready = effect.ready;
    let alpha = if ready { 1.0 } else { SOON };
    let thumb = effect.thumb;
    let name = effect.name.get(lang);
    let ink = if on && ready { TEXT } else { DIM };
    let face = column![
        drawn(THUMB.width, THUMB.height, move |f, size| {
            draw_thumb(f, size, thumb, alpha);
        }),
        said(
            name,
            TILE_TEXT,
            MANROPE_SEMIBOLD,
            ink.get().scale_alpha(alpha)
        ),
    ]
    .spacing(4.0)
    .align_x(Horizontal::Center);
    let target = Target::Tile(effect.id);
    let mut pressed = button(container(face).center(Length::Fill))
        .padding(0)
        .width(Length::Fill)
        .height(TILE)
        .style(move |_, status| {
            let (fill, rim) = match (on, ready && style::lit(status)) {
                (true, _) => (CHOSEN, CHOSEN_RIM),
                (false, true) => (TILE_HOVER, TILE_RIM),
                (false, false) => (TILE_FILL, TILE_RIM),
            };
            button::Style {
                background: Some(Background::Color(fill.get().scale_alpha(alpha))),
                border: Border {
                    radius: TILE_RADIUS.into(),
                    width: 1.0,
                    color: rim.get().scale_alpha(alpha),
                },
                ..button::Style::default()
            }
        });
    let said_name = if ready {
        name.to_owned()
    } else {
        format!("{name}, {}", w::SOON.get(lang))
    };
    if ready {
        pressed = pressed.on_press(Msg::Act(target));
    }
    let mut control =
        control(pressed, target.id(), Role::ToggleButton, said_name).node(move |n| n.checked(on));
    if ready {
        control = control
            .focus(focus == Some(target), Msg::Focus(target), TILE_RADIUS)
            .activate(Msg::Act(target));
    }
    control.into()
}

/// The dashed "+ add-ons" tile: effects from add-ons (#442), later.
fn addons<'a>(lang: Lang) -> El<'a> {
    let face = column![
        said("+", PLUS, MANROPE, FAINT.get()),
        said(w::ADDONS.get(lang), ADDONS, MANROPE_SEMIBOLD, FAINT.get()),
    ]
    .spacing(2.0)
    .align_x(Horizontal::Center);
    let tile = stack![
        iced::widget::canvas(Dashed)
            .width(Length::Fill)
            .height(TILE),
        container(face)
            .width(Length::Fill)
            .height(TILE)
            .center(Length::Fill),
    ];
    crate::ui::line(
        container(tile).width(Length::Fill),
        w::ADDONS_NAME.get(lang),
    )
    .into()
}

/// The add-ons' tile's dashed edge.
struct Dashed;

impl<M> canvas::Program<M> for Dashed {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let rect = Rectangle::new(
            Point::new(0.5, 0.5),
            Size::new(bounds.width - 1.0, bounds.height - 1.0),
        );
        f.stroke(
            &kit::rounded(rect, TILE_RADIUS - 0.5),
            Stroke {
                line_dash: canvas::LineDash {
                    segments: &[4.0, 3.0],
                    offset: 0,
                },
                ..Stroke::default().with_color(DASHED.get()).with_width(1.0)
            },
        );
        vec![f.into_geometry()]
    }
}

/// A thumbnail at `alpha`, its corners rounded.
fn draw_thumb(f: &mut Frame, size: Size, thumb: Thumb, alpha: f32) {
    let rect = Rectangle::new(Point::ORIGIN, size);
    let shape = kit::rounded(rect, THUMB_RADIUS);
    let colour = |c: u32| {
        let [r, g, b, a] = c.to_be_bytes();
        Color::from_rgba8(r, g, b, f32::from(a) / 255.0 * alpha)
    };
    let corner = (size.width / 2.0).hypot(size.height / 2.0);
    match thumb {
        Thumb::Flat(c) => f.fill(&shape, colour(c)),
        Thumb::Linear(angle, stops) => {
            // CSS's gradient line: through the middle, as long as the box
            // reaches along it.
            let a = angle.to_radians();
            let (dx, dy) = (a.sin(), -a.cos());
            let half = (size.width * dx.abs() + size.height * dy.abs()) / 2.0;
            let mid = Point::new(size.width / 2.0, size.height / 2.0);
            let mut linear = gradient::Linear::new(
                Point::new(mid.x - dx * half, mid.y - dy * half),
                Point::new(mid.x + dx * half, mid.y + dy * half),
            );
            for &(at, c) in stops {
                linear = linear.add_stop(at, colour(c));
            }
            f.fill(&shape, canvas::Fill::from(linear));
        }
        Thumb::Radial(inner, outer) => {
            f.fill(&shape, colour(outer));
            let mid = Point::new(size.width / 2.0, size.height / 2.0);
            // Rings from the corners in: the gradient by steps.
            let steps = 12;
            for i in (1..=steps).rev() {
                let t = i as f32 / steps as f32;
                let mix = mix(colour(inner), colour(outer), t);
                f.with_save(|f| {
                    f.fill(&Path::circle(mid, corner * t), mix);
                });
            }
        }
        Thumb::Disc {
            at,
            r,
            inner,
            outer,
        } => {
            f.fill(&shape, colour(outer));
            let centre = Point::new(size.width * at.0, size.height * at.1);
            let far = [
                (0.0, 0.0),
                (size.width, 0.0),
                (0.0, size.height),
                (size.width, size.height),
            ]
            .iter()
            .map(|(x, y)| (x - centre.x).hypot(y - centre.y))
            .fold(0.0, f32::max);
            f.fill(&Path::circle(centre, far * r), colour(inner));
        }
        Thumb::Ears => {
            f.fill(&shape, colour(0x3A3D_46FF));
            for x in [0.3, 0.7] {
                let centre = Point::new(size.width * x, size.height * 0.4);
                f.fill(
                    &Path::circle(centre, size.height * 0.37),
                    colour(0x1A1C_22FF),
                );
            }
        }
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

fn rule<'a>() -> El<'a> {
    container(Space::new())
        .width(Length::Fill)
        .height(1.0)
        .style(|_| style::boxed(SEP.get(), 0.0))
        .into()
}

/// "Lumière d'écran", how it shines, its switch.
fn light<'a>(lang: Lang, shown: &CameraShown, ui: &Ui, motion: bool) -> El<'a> {
    let detail = match shown.strength.as_str() {
        "soft" => w::SOFT,
        "medium" => w::MEDIUM,
        "strong" => w::STRONG,
        _ => w::AUTO,
    };
    let on = ui.switch.shown(shown.screen_light, Instant::now());
    let knob = Knob {
        knob: |now, motion| {
            let said = camera::shown().screen_light;
            ui_switch(|switch| {
                let on = switch.shown(said, now);
                switch.knob_at(on, now, motion)
            })
        },
        sliding: |now| ui_switch(|switch| switch.sliding(now)),
        motion,
    };
    let switch = control(
        knob.view(Msg::Act(Target::Light)),
        Target::Light.id(),
        Role::ToggleButton,
        w::SCREEN_LIGHT.get(lang),
    )
    .node(move |n| n.checked(on))
    .focus(
        ui.focus == Some(Target::Light),
        Msg::Focus(Target::Light),
        SWITCH.height / 2.0,
    )
    .activate(Msg::Act(Target::Light));
    row![
        said(w::SCREEN_LIGHT.get(lang), LIGHT, MANROPE, SOFT_TEXT.get()),
        gap(8.0),
        container(said(detail.get(lang), LIGHT_DETAIL, MANROPE, FAINT.get())).width(Length::Fill),
        gap(8.0),
        switch,
    ]
    .align_y(Vertical::Center)
    .into()
}

/// The switch's knob, from its own lock: the view's builder holds the
/// window's state while it builds.
fn ui_switch<T>(with: impl FnOnce(&mut kit::Switch) -> T) -> T {
    ui(|ui| with(&mut ui.switch))
}

fn help_link<'a>(lang: Lang, focus: Option<Target>) -> El<'a> {
    link(w::HELP.get(lang), Target::Help, focus)
}

/// A link's words, in blue, doing `target`.
fn link<'a>(word: &str, target: Target, focus: Option<Target>) -> El<'a> {
    let pressed = button(said(word.to_owned(), LINK, MANROPE, LINK_BLUE.get()))
        .padding(0)
        .style(|_, _| style::clear())
        .on_press(Msg::Act(target));
    control(pressed, target.id(), Role::Button, word)
        .focus(focus == Some(target), Msg::Focus(target), 3.0)
        .activate(Msg::Act(target))
        .into()
}

/// Where choosing « Hocuspanda » works: each kind of application, what
/// it takes, and whether that is a plain yes. Everywhere with the
/// v4l2loopback device; without it, Firefox behind its setting, Chrome
/// behind its experimental one, the applications not at all.
fn where_it_works(loopback: bool) -> [(Text, Text, bool); 3] {
    if loopback {
        [
            (w::WORKS_WEB, w::YES, true),
            (w::WORKS_CHROME, w::YES, true),
            (w::WORKS_APPS, w::YES, true),
        ]
    } else {
        [
            (w::WORKS_WEB, w::SETTING_TO_TURN_ON, false),
            (w::WORKS_CHROME, w::EXPERIMENTAL, false),
            (w::WORKS_APPS, w::NOT_WITHOUT_PACKAGE, false),
        ]
    }
}

/// What the help says is needed, under the table: nothing with the
/// device; else the package, and Firefox's setting.
fn what_it_needs(loopback: bool) -> Vec<Text> {
    if loopback {
        Vec::new()
    } else {
        vec![w::PACKAGE_NEEDED, w::FIREFOX_SETTING]
    }
}

/// `text` wrapped at the window's width, each line `size` tall times
/// `line`.
fn wrapped<'a>(text: &str, size: f32, line: f32, color: Color) -> El<'a> {
    let room = WIDTH as f32 - 2.0 * (BORDER + PAD);
    let mut lines = column![];
    for one in kit::wrap(text, room, |t| kit::measure(t, size, MANROPE)) {
        lines = lines.push(
            container(said(one, size, MANROPE, color))
                .height(size * line)
                .align_y(Vertical::Center),
        );
    }
    crate::ui::line(lines, text).into()
}

/// How to choose « Hocuspanda » in a call: the InCall board, at the
/// window's width.
fn help_page<'a>(lang: Lang, focus: Option<Target>, loopback: bool) -> El<'a> {
    let list_row = |left: El<'a>, right: Option<El<'a>>, chosen: bool| -> El<'a> {
        let mut line = row![left].align_y(Vertical::Center);
        if let Some(right) = right {
            line = line.push(Space::new().width(Length::Fill)).push(right);
        }
        container(line)
            .padding([0.0, 12.0])
            .width(Length::Fill)
            .height(ROW)
            .align_y(Vertical::Center)
            .style(move |_| {
                if chosen {
                    style::boxed(LIST_ON.get(), 0.0)
                } else {
                    container::Style::default()
                }
            })
            .into()
    };
    let choices = container(column![
        list_row(
            said(w::BUILT_IN.get(lang), BODY, MANROPE, DIM.get()).into(),
            None,
            false
        ),
        list_row(
            said("Hocuspanda", BODY, MANROPE_BOLD, TEXT.get()).into(),
            Some(
                said(
                    w::WITH_YOUR_EFFECTS.get(lang),
                    SMALL,
                    MANROPE_SEMIBOLD,
                    LINK_BLUE.get()
                )
                .into()
            ),
            true
        ),
    ])
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(LIST.get().into()),
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: LIST_RIM.get(),
        },
        ..container::Style::default()
    });
    let settings = container(
        column![
            said(w::CALL_SETTINGS.get(lang), SMALL, MANROPE, FAINT.get()),
            choices
        ]
        .spacing(8.0),
    )
    .padding(12.0)
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(BOX.get().into()),
        border: Border {
            radius: 12.0.into(),
            width: 1.0,
            color: BOX_RIM.get(),
        },
        ..container::Style::default()
    });
    let works = |(app, answer, yes): (Text, Text, bool)| -> El<'a> {
        row![
            said(app.get(lang), SMALL, MANROPE, TEXT.get()),
            Space::new().width(Length::Fill),
            said(
                answer.get(lang),
                SMALL,
                MANROPE_SEMIBOLD,
                if yes { YES.get() } else { MAYBE.get() }
            ),
        ]
        .align_y(Vertical::Center)
        .height(SMALL * LINE + 6.0)
        .into()
    };
    let back = link(&format!("‹ {}", w::BACK.get(lang)), Target::Back, focus);
    let mut table = column![];
    for answer in where_it_works(loopback) {
        table = table.push(works(answer));
    }
    let mut page = column![
        head(lang, back),
        said(w::IN_THE_CALL.get(lang), TITLE, MANROPE_BOLD, TEXT.get()),
        wrapped(w::HELP_BODY.get(lang), BODY, BODY_LINE, SOFT_TEXT.get()),
        settings,
        spaced_by(
            w::WHERE_IT_WORKS.get(lang),
            LABEL,
            MANROPE_BOLD,
            FAINT.get(),
            0.1
        ),
        table,
    ];
    for need in what_it_needs(loopback) {
        page = page.push(wrapped(need.get(lang), SMALL, LINE, SOFT_TEXT.get()));
    }
    page.push(said(
        w::SCREEN_LIGHT_EVERYWHERE.get(lang),
        SMALL,
        MANROPE,
        FAINT.get(),
    ))
    .spacing(GAP)
    .into()
}

/// What it shows, for the log, when it changed.
fn say(lang: Lang, shown: &CameraShown, readers: &str, ui: &mut Ui) {
    let said = match ui.page {
        Page::Effects => {
            let tiles: Vec<String> = ui
                .tab
                .effects()
                .map(|e| {
                    let mark = match (e.ready, e.on(shown)) {
                        (false, _) => "~",
                        (true, true) => "x",
                        (true, false) => " ",
                    };
                    format!("[{mark}] {}", e.name.get(lang))
                })
                .collect();
            let dots: Vec<&str> = Tab::ALL
                .iter()
                .filter(|t| t.on(shown))
                .map(|t| t.name().get(lang))
                .collect();
            format!(
                "{} | {readers} | {} (dots: {}) | {} | {} | {}: {}",
                w::HEADING.get(lang),
                ui.tab.name().get(lang),
                dots.join(", "),
                tiles.join(", "),
                effects::hint(ui.tab, shown, lang),
                w::SCREEN_LIGHT.get(lang),
                if shown.screen_light { "on" } else { "off" },
            )
        }
        Page::Help => {
            let answers: Vec<String> = where_it_works(ui.loopback)
                .iter()
                .map(|(app, answer, _)| format!("{}: {}", app.get(lang), answer.get(lang)))
                .collect();
            format!(
                "{} | {} | {}",
                w::HEADING.get(lang),
                w::IN_THE_CALL.get(lang),
                answers.join(", ")
            )
        }
    };
    if ui.said.as_deref() != Some(said.as_str()) {
        tracing::debug!(%said, "the camera's window shows it");
        ui.said = Some(said);
    }
}

/// The keys the surface keeps from the kit: none of its own yet.
fn taken(_pressed: &Pressed) -> Option<Msg> {
    None
}

fn handle(message: Msg) {
    match message {
        Msg::Stale => {}
        Msg::Pressed => ui(|ui| ui.focus = None),
        Msg::Focus(target) => ui(|ui| ui.focus = Some(target)),
        Msg::Act(Target::Tab(tab)) => {
            ui(|ui| ui.tab = tab);
            rebuild();
        }
        Msg::Act(Target::Tile(id)) => {
            let Some(effect) = effects::EFFECTS.iter().find(|e| e.id == id && e.ready) else {
                return;
            };
            match effect.pick {
                effects::Pick::Toggle(key) => {
                    let on = effect.on(&camera::shown());
                    tracing::info!(effect = id, on = !on, "the camera's effect chosen");
                    camera::choose(key, !on);
                }
                effects::Pick::One { key, value, .. } => {
                    tracing::info!(effect = id, key, "the camera's effect chosen");
                    camera::choose_word(key, value);
                }
            }
            rebuild();
        }
        Msg::Act(Target::Light) => {
            let said = camera::shown().screen_light;
            let on = ui(|ui| ui.switch.flip(said, Instant::now(), true));
            camera::choose("screen-light", on);
            rebuild();
        }
        Msg::Act(Target::Help) => {
            // From sysfs alone: no device is opened.
            let loopback = hocus_camera::loopback::present();
            tracing::debug!(loopback, "the camera's help: the v4l2loopback device");
            ui(|ui| {
                ui.page = Page::Help;
                ui.focus = None;
                ui.loopback = loopback;
            });
            rebuild();
        }
        Msg::Act(Target::Back) => {
            ui(|ui| {
                ui.page = Page::Effects;
                ui.focus = None;
            });
            rebuild();
        }
    }
}

fn rebuild() {
    super::send(vec![super::Effect::Rebuild]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_words_are_the_mockup_s() {
        let fr = Lang::French;
        assert_eq!(w::HEADING.get(fr), "CAMÉRA ET MICRO");
        assert_eq!(w::read_by(fr, "Firefox"), "Utilisée par Firefox");
        assert_eq!(w::AUTO.get(fr), "auto, sur ton visage");
        assert_eq!(w::HELP.get(fr), "Choisir « Hocuspanda » dans l’appel…");
        let tabs: Vec<&str> = Tab::ALL.iter().map(|t| t.name().get(fr)).collect();
        assert_eq!(tabs, ["Image", "Fond", "Visage", "Voix", "Réactions"]);
    }

    /// « OÙ ÇA MARCHE »: everywhere with the v4l2loopback device; without
    /// it, what each kind of application takes, and the package named.
    #[test]
    fn the_help_says_where_it_works_with_the_device_or_without() {
        let fr = Lang::French;
        let said = |loopback| -> Vec<(&str, &str, bool)> {
            where_it_works(loopback)
                .iter()
                .map(|(app, answer, yes)| (app.get(fr), answer.get(fr), *yes))
                .collect()
        };
        assert_eq!(
            said(true),
            [
                ("Firefox (visio web)", "oui", true),
                ("Chrome, Chromium", "oui", true),
                ("Slack, Teams, Discord (applis)", "oui", true),
            ]
        );
        assert!(what_it_needs(true).is_empty());
        assert_eq!(
            said(false),
            [
                ("Firefox (visio web)", "réglage à activer", false),
                ("Chrome, Chromium", "réglage expérimental", false),
                (
                    "Slack, Teams, Discord (applis)",
                    "pas sans ce paquet",
                    false
                ),
            ]
        );
        let needs: Vec<&str> = what_it_needs(false).iter().map(|t| t.get(fr)).collect();
        assert_eq!(needs.len(), 2);
        assert!(needs[0].contains("v4l2loopback-dkms"), "{}", needs[0]);
        assert!(needs[0].contains("redémarre"), "{}", needs[0]);
        assert!(
            needs[0].contains("sudo modprobe v4l2loopback"),
            "{}",
            needs[0]
        );
        assert!(
            needs[1].contains("about:config")
                && needs[1].contains("media.webrtc.camera.allow-pipewire"),
            "{}",
            needs[1]
        );
        let en: Vec<&str> = what_it_needs(false)
            .iter()
            .map(|t| t.get(Lang::English))
            .collect();
        assert!(en[0].contains("v4l2loopback-dkms"), "{}", en[0]);
    }
}
