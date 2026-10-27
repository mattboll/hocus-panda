//! The panel's forms in the compositor (#263, decision 0011). The widget
//! column is always there in form `column`; in the others, what stays on
//! the primary screen is a strip on the right, a bar at the top, or a
//! faint hint at the right edge, and the whole column slides out over the
//! windows when asked for: the pointer resting on the strip or at the edge,
//! a click on the strip or the bar, or the shortcut (Super+C). It slides
//! back when the pointer leaves it, on Escape, on the shortcut again or a
//! click elsewhere. Windows leave room for what stays only.
//!
//! With `[panel] screens = "all"` (#282) every screen keeps its own strip,
//! bar or hint, and the one column comes out on the screen it was asked
//! on: the strip or the edge rested on or clicked, the bar clicked, the
//! screen under the pointer for the shortcut. Each top bar shows its
//! screen's active window, and its buttons act on it. The column form
//! stays on the primary screen.
//!
//! The slide is one value, evaluated in closed form at the clock's time
//! when the scene is drawn: frames are asked for while it moves, none at
//! rest. With reduced motion (or in tests that do not ask for motion) the
//! column is out or in at once.

use std::time::Duration;

use hocus_api::UiNode;
use hocus_core::{
    WindowId,
    config::{PanelForm, PanelScreens},
};
use hocus_shell::{
    Action as ShellAction, ActiveWindow, ColumnState, Lang,
    column::{MARGIN, View, WIDTH},
    strip::{self, EdgeHint, Here, Strip},
    topbar::{self, TopBar},
};
use hocus_ui::{
    Input, Point as UiPoint, Theme,
    motion::{Animated, Curve, Easing, SMALL, STANDARD},
};
use smithay::{
    backend::renderer::{ImportAll, ImportMem, Renderer, Texture},
    output::Output,
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
};

use crate::{
    State,
    alarm::Alarm,
    scene::SceneElement,
    shell::{Panel, Pixels, Shell, column_rect, flatten},
};

/// How long the pointer rests on the strip before the column comes out:
/// passing over it on the way to another screen does nothing.
const STRIP_REST: Duration = SMALL;
/// And at the right edge, for the hidden panel: as long as at the bottom
/// edge for the dock.
const EDGE_REST: Duration = Duration::from_millis(300);
/// The right edge that counts, logical pixels, as the dock's bottom one.
const EDGE: f64 = 2.0;

impl Pixels for Strip {
    fn pixels(&self) -> View<'_> {
        self.view()
    }
}

impl Pixels for TopBar {
    fn pixels(&self) -> View<'_> {
        self.view()
    }
}

impl Pixels for EdgeHint {
    fn pixels(&self) -> View<'_> {
        self.view()
    }
}

/// What stays on the screen of a form that keeps the column in.
#[derive(Debug)]
enum Bar {
    Strip(Panel<Strip>),
    Top(Panel<TopBar>),
    /// The hidden panel's hint at the edge, and the scale it is drawn at.
    Hint(Panel<EdgeHint>, f64),
}

/// What stays of the panel on one screen.
#[derive(Debug)]
struct OnScreen {
    output: String,
    /// That output's logical rectangle and its scale.
    area: Rectangle<i32, Logical>,
    scale: f64,
    bar: Option<Bar>,
    /// The window the top bar shows and its buttons act on, on every
    /// screen (#282): that screen's active one.
    window: Option<WindowId>,
    /// The pointer over the top bar.
    inside: bool,
}

impl OnScreen {
    fn new(output: &str, area: Rectangle<i32, Logical>, scale: f64) -> Self {
        Self {
            output: output.to_owned(),
            area,
            scale,
            bar: None,
            window: None,
            inside: false,
        }
    }
}

/// Whether the whole column is out over the windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Out {
    #[default]
    In,
    /// The pointer brought it, or came over it: it goes back when the
    /// pointer leaves it.
    Hovered,
    /// Asked for (the shortcut, a click on the top bar) with the pointer
    /// elsewhere: it stays until Escape, the shortcut again or a click
    /// elsewhere, or until the pointer came over it and left.
    Asked,
}

/// The pointer where the column comes out from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Rest {
    #[default]
    Away,
    /// Resting there since then, on that screen's strip or edge.
    Since(Duration, String),
    /// It brought the column out, or the column was sent back while it
    /// stayed: it must leave before resting there counts again.
    Spent,
}

/// The panel's form and what it shows besides the column.
#[derive(Debug)]
pub struct Forms {
    /// The form and the screens drawn, to see the configuration change
    /// them.
    form: Option<(PanelForm, PanelScreens)>,
    /// What stays on each screen that has it, the primary first.
    screens: Vec<OnScreen>,
    theme: Theme,
    lang: Lang,
    pub(crate) out: Out,
    /// 1 out, 0 in.
    slide: Animated,
    /// It moved at the last frame: one more shows where it ends.
    sliding: bool,
    rest: Rest,
    alarm: Alarm,
    /// The screen whose top bar a press began on.
    pressed: Option<String>,
    /// The screen the column was last asked on, on every screen (#282).
    asked_on: Option<String>,
}

impl Forms {
    pub fn new(theme: Theme, lang: Lang) -> Self {
        Self {
            form: None,
            screens: Vec::new(),
            theme,
            lang,
            out: Out::In,
            slide: Animated::new(0.0),
            sliding: false,
            rest: Rest::Away,
            alarm: Alarm::default(),
            pressed: None,
            asked_on: None,
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        if theme == self.theme {
            return;
        }
        self.theme = theme.clone();
        for screen in &mut self.screens {
            match &mut screen.bar {
                Some(Bar::Strip(p)) => p.view.set_theme(theme.clone()),
                Some(Bar::Top(p)) => p.view.set_theme(theme.clone()),
                // Made again with the new accent.
                Some(Bar::Hint(..)) => screen.bar = None,
                None => {}
            }
        }
    }

    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
        for screen in &mut self.screens {
            match &mut screen.bar {
                Some(Bar::Strip(p)) => p.view.set_lang(lang),
                Some(Bar::Top(p)) => p.view.set_lang(lang),
                _ => {}
            }
        }
    }

    /// The column slides out (or is out at once when nothing `moves`).
    fn open(&mut self, how: Out, now: Duration, moves: bool) {
        self.out = how;
        if moves {
            self.slide
                .set(1.0, Curve::Ease(Easing::OutCubic, STANDARD), now);
        } else {
            self.slide.jump(1.0);
        }
    }

    /// The column slides back.
    fn close(&mut self, now: Duration, moves: bool) {
        self.out = Out::In;
        if moves {
            self.slide
                .set(0.0, Curve::Ease(Easing::InCubic, STANDARD), now);
        } else {
            self.slide.jump(0.0);
        }
    }

    /// The screen whose strip, or right edge when hidden, is at `at`.
    fn trigger_at(&self, form: PanelForm, at: Point<f64, Logical>) -> Option<usize> {
        self.screens.iter().position(|s| match form {
            PanelForm::Strip => strip_rect(s.area).to_f64().contains(at),
            PanelForm::Hidden => at_edge(s.area, at),
            PanelForm::Top | PanelForm::Column => false,
        })
    }

    /// The screen whose top bar is at `at`.
    fn bar_at(&self, at: Point<f64, Logical>) -> Option<usize> {
        self.screens.iter().position(|s| {
            matches!(s.bar, Some(Bar::Top(_))) && top_rect(s.area).to_f64().contains(at)
        })
    }
}

/// The strip on an output of logical rectangle `area`.
pub fn strip_rect(area: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    let (w, m) = (strip::WIDTH as i32, MARGIN as i32);
    Rectangle::new(
        (area.loc.x + area.size.w - m - w, area.loc.y + m).into(),
        (w, (area.size.h - 2 * m).max(1)).into(),
    )
}

/// The top bar on an output of logical rectangle `area`.
pub fn top_rect(area: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    let m = MARGIN as i32;
    Rectangle::new(
        (area.loc.x + m, area.loc.y + topbar::TOP as i32).into(),
        ((area.size.w - 2 * m).max(1), topbar::HEIGHT as i32).into(),
    )
}

/// The hidden panel's hint, output-local.
fn hint_place(area: Rectangle<i32, Logical>) -> Point<i32, Logical> {
    let size = EdgeHint::SIZE;
    Point::from((
        area.size.w - (EdgeHint::RIGHT + size.w).round() as i32,
        (area.size.h - size.h.round() as i32) / 2,
    ))
}

/// Whether `at` is in the column's band: from its left edge to the
/// screen's right one, the whole height.
fn in_band(area: Rectangle<i32, Logical>, at: Point<f64, Logical>) -> bool {
    let column = column_rect(area).to_f64();
    let a = area.to_f64();
    at.x >= column.loc.x
        && at.x <= a.loc.x + a.size.w
        && at.y >= a.loc.y
        && at.y <= a.loc.y + a.size.h
}

/// Whether `at` is at the right edge of the output of rectangle `area`.
fn at_edge(area: Rectangle<i32, Logical>, at: Point<f64, Logical>) -> bool {
    let a = area.to_f64();
    at.x >= a.loc.x + a.size.w - EDGE
        && at.x <= a.loc.x + a.size.w
        && at.y >= a.loc.y
        && at.y <= a.loc.y + a.size.h
}

/// How long the pointer rests before the column comes out in `form`.
fn rest_for(form: PanelForm) -> Duration {
    if form == PanelForm::Hidden {
        EDGE_REST
    } else {
        STRIP_REST
    }
}

/// A screen with its place, its scale and its active window, for the
/// panel on every screen.
type Place = (
    String,
    Rectangle<i32, Logical>,
    f64,
    Option<(WindowId, ActiveWindow)>,
);

impl State {
    /// The panel's form in effect.
    pub(crate) fn panel_form(&self) -> PanelForm {
        self.config.panel.form
    }

    /// Whether every screen keeps its own strip, bar or hint, and the
    /// column comes out where it is asked (#282). The column form stays
    /// on the primary screen.
    pub(crate) fn panel_everywhere(&self) -> bool {
        self.config.panel.screens == PanelScreens::All && self.panel_form() != PanelForm::Column
    }

    /// Whether panels move as they come and go now.
    fn panel_moves(&self) -> bool {
        !self.instant_motion && !self.config.appearance.reduced_motion
    }

    /// The screen the column is on: the one it was last asked on when the
    /// panel is on every screen, the primary one otherwise.
    pub(crate) fn column_screen(&self) -> Option<Output> {
        if self.panel_everywhere()
            && let Some(output) = self
                .shell
                .as_ref()
                .and_then(|s| s.panel.asked_on.as_deref())
                .and_then(|name| self.find_output(name))
        {
            return Some(output);
        }
        self.column_output()
    }

    /// What the panel keeps on `output` at rest, which windows leave room
    /// for: the column, the strip or the top bar; nothing when hidden.
    pub fn panel_rect(&self, output: &Output) -> Option<Rectangle<i32, Logical>> {
        let (name, area, _) = self.shell.as_ref()?.on.as_ref()?;
        if self.panel_everywhere() {
            // Each screen its own, wherever the column is.
            let area = self.space.output_geometry(output)?;
            return match self.panel_form() {
                PanelForm::Strip => Some(strip_rect(area)),
                PanelForm::Top => Some(top_rect(area)),
                PanelForm::Column | PanelForm::Hidden => None,
            };
        }
        if *name != output.name() {
            return None;
        }
        match self.panel_form() {
            PanelForm::Column => Some(column_rect(*area)),
            PanelForm::Strip => Some(strip_rect(*area)),
            PanelForm::Top => Some(top_rect(*area)),
            PanelForm::Hidden => None,
        }
    }

    /// How far right of its place the column is drawn now; `None` while
    /// it does not show.
    pub(crate) fn column_shift(&self) -> Option<f64> {
        let shell = self.shell.as_ref()?;
        if self.panel_form() == PanelForm::Column {
            return Some(0.0);
        }
        let out = f64::from(shell.panel.slide.value(self.clock.now()));
        (out > 0.0).then(|| (1.0 - out) * f64::from(WIDTH + MARGIN))
    }

    /// Whether the column is drawn out over the windows, sliding or not.
    pub(crate) fn panel_out_drawn(&self) -> bool {
        self.panel_form() != PanelForm::Column && self.column_shift().is_some()
    }

    /// Whether the column takes the pointer: always in form `column`,
    /// otherwise while it is out.
    pub(crate) fn column_live(&self) -> bool {
        self.shell
            .as_ref()
            .is_some_and(|s| self.panel_form() == PanelForm::Column || s.panel.out != Out::In)
    }

    /// Whether the column is out over the windows: Escape sends it back.
    pub fn panel_out(&self) -> bool {
        self.panel_form() != PanelForm::Column
            && self.shell.as_ref().is_some_and(|s| s.panel.out != Out::In)
    }

    /// Every screen, the primary first, with its place and its active
    /// window: where the panel is on every screen.
    fn panel_places(&self) -> Vec<Place> {
        let primary = self.model.primary().map(|o| o.name.clone());
        let mut places: Vec<Place> = self
            .model
            .outputs()
            .iter()
            .filter_map(|o| {
                let output = self.find_output(&o.name.0)?;
                let area = self.space.output_geometry(&output)?;
                let active = self
                    .model
                    .active_on(&o.name)
                    .and_then(|id| Some((id, self.active_window(id)?)));
                Some((
                    o.name.0.clone(),
                    area,
                    output.current_scale().fractional_scale(),
                    active,
                ))
            })
            .collect();
        places.sort_by_key(|p| primary.as_ref().map(|n| &n.0) != Some(&p.0));
        places
    }

    /// Before the column is placed, after each dispatch: the pointer
    /// rested long enough where the column comes out from, which brings
    /// it out on that screen. Whether something to draw changed.
    pub(crate) fn panel_rested(&mut self) -> bool {
        let form = self.panel_form();
        let now = self.clock.now();
        let Some(Rest::Since(since, on)) = self.shell.as_ref().map(|s| &s.panel.rest) else {
            return false;
        };
        if now < *since + rest_for(form) {
            return false;
        }
        let fullscreen = self.find_output(on).is_some_and(|o| self.fullscreen_on(&o));
        let on = self.panel_everywhere().then(|| on.clone());
        let moves = self.panel_moves();
        let handle = self.waker().cloned();
        let Some(shell) = self.shell.as_mut() else {
            return false;
        };
        let forms = &mut shell.panel;
        forms.rest = Rest::Spent;
        forms.alarm.set(handle.as_ref(), "panel rest", None, now);
        if forms.out != Out::In || fullscreen {
            return false;
        }
        if on.is_some() {
            forms.asked_on = on;
        }
        forms.open(Out::Hovered, now, moves);
        true
    }

    /// After each dispatch, the column drawn: follows the form, shows the
    /// strip or the bar on each screen that has one. A strip or a bar that
    /// changed redraws its own screen (#244); whether the column's screen
    /// has more to draw (the column sliding).
    pub(crate) fn refresh_panel(&mut self, state: &ColumnState) -> bool {
        let form = self.panel_form();
        let screens = self.config.panel.screens;
        let everywhere = self.panel_everywhere();
        let now = self.clock.now();
        let handle = self.waker().cloned();
        // Nothing is looked up or copied for the primary screen alone.
        let all = if everywhere {
            self.panel_places()
        } else {
            Vec::new()
        };
        let Self {
            shell,
            space,
            redraw,
            ..
        } = self;
        let Some(shell) = shell.as_mut() else {
            return false;
        };
        let Shell {
            on, column, panel, ..
        } = shell;
        let forms = panel;
        let mut changed = false;
        if forms.form != Some((form, screens)) {
            redraw.all();
            // Another form: the column is in, and what stays made anew.
            forms.form = Some((form, screens));
            forms.screens.clear();
            forms.out = Out::In;
            forms.slide.jump(0.0);
            forms.rest = Rest::Away;
            forms.pressed = None;
            forms.alarm.set(handle.as_ref(), "panel rest", None, now);
            column.close_popover();
            column.pointer_left();
            changed = true;
        }
        // The screens that keep something: the column's alone, or every
        // one once the column has a screen.
        let one = on
            .as_ref()
            .map(|(name, area, scale)| (name.as_str(), *area, *scale));
        let every: Vec<(&str, Rectangle<i32, Logical>, f64)> = all
            .iter()
            .filter(|_| on.is_some())
            .map(|(name, area, scale, _)| (name.as_str(), *area, *scale))
            .collect();
        let places = match form {
            PanelForm::Column => &[][..],
            _ if everywhere => &every[..],
            _ => one.as_slice(),
        };
        // Nothing is made or copied while the form and the screens stay.
        forms
            .screens
            .retain(|s| places.iter().any(|(name, ..)| *name == s.output));
        for (i, &(name, area, scale)) in places.iter().enumerate() {
            match forms.screens.iter().position(|s| s.output == name) {
                Some(j) if j == i => {}
                Some(j) => {
                    let screen = forms.screens.remove(j);
                    forms.screens.insert(i, screen);
                }
                None => forms.screens.insert(i, OnScreen::new(name, area, scale)),
            }
            let Some(screen) = forms.screens.get_mut(i) else {
                continue;
            };
            screen.area = area;
            screen.scale = scale;
            match form {
                PanelForm::Strip => {
                    let height = strip_rect(area).size.h as f32;
                    match &mut screen.bar {
                        Some(Bar::Strip(p)) => p.view.resize(height, scale as f32),
                        _ => {
                            let strip =
                                Strip::new(forms.theme.clone(), forms.lang, height, scale as f32);
                            screen.bar = Some(Bar::Strip(Panel::new(strip)));
                        }
                    }
                }
                PanelForm::Top => {
                    let width = top_rect(area).size.w as f32;
                    match &mut screen.bar {
                        Some(Bar::Top(p)) => p.view.resize(width, scale as f32),
                        _ => {
                            let bar =
                                TopBar::new(forms.theme.clone(), forms.lang, width, scale as f32);
                            screen.bar = Some(Bar::Top(Panel::new(bar)));
                        }
                    }
                }
                PanelForm::Hidden => {
                    if !matches!(screen.bar, Some(Bar::Hint(_, s)) if s == scale) {
                        let mut hint = Panel::new(EdgeHint::new(forms.theme.clone(), scale as f32));
                        hint.upload(&[]);
                        screen.bar = Some(Bar::Hint(hint, scale));
                        redraw.named(space, name);
                    }
                }
                PanelForm::Column => screen.bar = None,
            }
        }
        for screen in &mut forms.screens {
            // Its own screen's desktops and active window on every screen;
            // the column's and the focused one otherwise.
            let here = if everywhere {
                let active = all
                    .iter()
                    .find(|(name, ..)| *name == screen.output)
                    .and_then(|(.., active)| active.as_ref());
                screen.window = active.map(|(id, _)| *id);
                Here {
                    screen: state.screens.iter().find(|s| s.output == screen.output),
                    active: active.map(|(_, window)| window),
                }
            } else {
                Here::of(state)
            };
            let shows = match &mut screen.bar {
                Some(Bar::Strip(p)) => {
                    p.view.set_state_here(state, here);
                    let drawn = p.view.draw(now);
                    p.upload(&drawn) || p.view.is_animating(now)
                }
                Some(Bar::Top(p)) => {
                    p.view.set_state_here(state, here);
                    let drawn = p.view.draw(now);
                    p.upload(&drawn) || p.view.is_animating(now)
                }
                Some(Bar::Hint(..)) | None => false,
            };
            if shows {
                redraw.named(space, &screen.output);
            }
        }
        // While it slides, and once more where it ends.
        let sliding = forms.slide.is_running(now);
        changed |= sliding || forms.sliding;
        forms.sliding = sliding;
        changed
    }

    /// The pointer moved to `location`, the column not taking it: the
    /// column out goes back when the pointer left its band; resting on a
    /// strip or at an edge counts; the top bars' buttons follow it.
    /// Whether a strip or a bar took it.
    pub(crate) fn panel_pointer_motion(&mut self, location: Point<f64, Logical>) -> bool {
        let form = self.panel_form();
        if form == PanelForm::Column {
            return false;
        }
        let now = self.clock.now();
        let moves = self.panel_moves();
        let handle = self.waker().cloned();
        let Some(shell) = self.shell.as_ref() else {
            return false;
        };
        let hit = shell.panel.trigger_at(form, location);
        let fullscreen = hit
            .and_then(|i| shell.panel.screens.get(i))
            .and_then(|s| self.find_output(&s.output))
            .is_some_and(|o| self.fullscreen_on(&o));
        let Some(shell) = self.shell.as_mut() else {
            return false;
        };
        let Some(area) = shell.on.as_ref().map(|(_, area, _)| *area) else {
            return false;
        };
        let forms = &mut shell.panel;
        match forms.out {
            Out::In => {}
            _ if in_band(area, location) => forms.out = Out::Hovered,
            Out::Hovered => {
                forms.close(now, moves);
                // The pointer is out of the strip too: resting there
                // counts again once it comes back.
                shell.column.close_popover();
                shell.column.pointer_left();
            }
            Out::Asked => {}
        }
        let forms = &mut shell.panel;
        let on_strip = form == PanelForm::Strip && hit.is_some();
        match hit
            .filter(|_| !fullscreen)
            .and_then(|i| forms.screens.get(i))
        {
            None => {
                forms.rest = Rest::Away;
                forms.alarm.set(handle.as_ref(), "panel rest", None, now);
            }
            Some(screen) => {
                let elsewhere = matches!(&forms.rest, Rest::Since(_, on) if *on != screen.output);
                if (forms.rest == Rest::Away || elsewhere) && forms.out == Out::In {
                    forms.rest = Rest::Since(now, screen.output.clone());
                    forms.alarm.set(
                        handle.as_ref(),
                        "panel rest",
                        Some(now + rest_for(form)),
                        now,
                    );
                }
            }
        }
        // The top bars' buttons show the pointer over them; a press begun
        // on one keeps the pointer there.
        if form == PanelForm::Top {
            let taker = match &forms.pressed {
                Some(output) => forms.screens.iter().position(|s| s.output == *output),
                None => forms.bar_at(location),
            };
            for (i, screen) in forms.screens.iter_mut().enumerate() {
                let Some(Bar::Top(panel)) = &mut screen.bar else {
                    continue;
                };
                if Some(i) == taker {
                    let bar = top_rect(screen.area);
                    let at = UiPoint::new(
                        (location.x - f64::from(bar.loc.x)) as f32,
                        (location.y - f64::from(bar.loc.y)) as f32,
                    );
                    screen.inside = true;
                    panel.view.input(at, Input::PointerMove(at));
                    self.redraw.all();
                } else if screen.inside {
                    screen.inside = false;
                    panel.view.pointer_left();
                    self.redraw.all();
                }
            }
            if taker.is_some() {
                return true;
            }
        }
        if forms.slide.is_running(now) {
            self.redraw.all();
        }
        on_strip
    }

    /// The primary button, the column not taking it: a press on a strip
    /// brings the column out there, the top bars' buttons act, a press
    /// elsewhere sends the column back. Whether a strip or a bar took it.
    pub(crate) fn panel_pointer_button(
        &mut self,
        location: Point<f64, Logical>,
        pressed: bool,
    ) -> bool {
        let form = self.panel_form();
        if form == PanelForm::Column {
            return false;
        }
        let now = self.clock.now();
        let moves = self.panel_moves();
        let everywhere = self.panel_everywhere();
        let Some(shell) = self.shell.as_mut() else {
            return false;
        };
        let Some(area) = shell.on.as_ref().map(|(_, area, _)| *area) else {
            return false;
        };
        let forms = &mut shell.panel;
        // A top bar: its buttons act; elsewhere on it, the column comes
        // out on its screen or goes back.
        let taker = if pressed {
            forms.bar_at(location)
        } else {
            forms
                .pressed
                .take()
                .and_then(|output| forms.screens.iter().position(|s| s.output == output))
        };
        if let Some(screen) = taker.and_then(|i| forms.screens.get_mut(i))
            && let Some(Bar::Top(panel)) = &mut screen.bar
        {
            if pressed {
                forms.pressed = Some(screen.output.clone());
            }
            let bar = top_rect(screen.area);
            let at = UiPoint::new(
                (location.x - f64::from(bar.loc.x)) as f32,
                (location.y - f64::from(bar.loc.y)) as f32,
            );
            let actions = panel.view.input(at, Input::PointerButton { pressed });
            let window = screen.window;
            let output = everywhere.then(|| screen.output.clone());
            self.redraw.all();
            for action in actions {
                match action {
                    ShellAction::OpenPanel => self.toggle_panel_from(Out::Asked, output.clone()),
                    // On every screen, on that screen's active window.
                    ShellAction::Minimize | ShellAction::ToggleMaximize | ShellAction::Close
                        if everywhere =>
                    {
                        self.act_on_window(&action, window);
                    }
                    action => self.shell_action(action),
                }
            }
            return true;
        }
        if !pressed {
            return false;
        }
        // A press elsewhere sends the column back.
        if forms.out != Out::In && !in_band(area, location) {
            forms.close(now, moves);
            forms.rest = Rest::Away;
            shell.column.close_popover();
            self.redraw.all();
            return false;
        }
        if form == PanelForm::Strip
            && forms.out == Out::In
            && let Some(i) = forms.trigger_at(form, location)
        {
            if everywhere {
                forms.asked_on = forms.screens.get(i).map(|s| s.output.clone());
            }
            forms.open(Out::Hovered, now, moves);
            forms.rest = Rest::Spent;
            self.redraw.all();
            return true;
        }
        false
    }

    /// A top bar's button on the window it shows (#282).
    fn act_on_window(&mut self, action: &ShellAction, window: Option<WindowId>) {
        let Some(id) = window else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        match action {
            ShellAction::Minimize => self.minimize(id, serial),
            ShellAction::ToggleMaximize => self.toggle_maximized(id, serial),
            ShellAction::Close => {
                if let Some(managed) = self.windows.get(&id) {
                    crate::surface::close(&managed.window);
                }
            }
            _ => {}
        }
    }

    /// The column out, or back (Super+C): nothing in form `column`. On
    /// every screen, it comes out on the screen under the pointer.
    pub fn toggle_panel(&mut self) {
        let on = if self.panel_everywhere() {
            self.output_under_pointer().map(|o| o.name())
        } else {
            None
        };
        self.toggle_panel_from(Out::Asked, on);
    }

    /// The column out, on screen `on` when given, or back.
    fn toggle_panel_from(&mut self, how: Out, on: Option<String>) {
        if self.panel_form() == PanelForm::Column {
            return;
        }
        let now = self.clock.now();
        let moves = self.panel_moves();
        let Some(shell) = self.shell.as_mut() else {
            return;
        };
        if shell.on.is_none() {
            return;
        }
        let forms = &mut shell.panel;
        if forms.out == Out::In {
            if on.is_some() {
                forms.asked_on = on;
            }
            forms.open(how, now, moves);
        } else {
            forms.close(now, moves);
            forms.rest = Rest::Spent;
            shell.column.close_popover();
            shell.column.pointer_left();
        }
        self.redraw.all();
    }

    /// Escape: the column out goes back. Whether it was out.
    pub fn panel_escape(&mut self) -> bool {
        if !self.panel_out() {
            return false;
        }
        self.toggle_panel_from(Out::Asked, None);
        true
    }

    /// What stays of the panel on `output`, front first: the strip, the
    /// bar, or the hidden panel's hint; none under a fullscreen window.
    pub fn panel_elements<R>(&self, renderer: &mut R, output: &Output) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let Some(shell) = &self.shell else {
            return Vec::new();
        };
        let name = output.name();
        let Some(screen) = shell.panel.screens.iter().find(|s| s.output == name) else {
            return Vec::new();
        };
        if self.fullscreen_on(output) {
            return Vec::new();
        }
        let area = screen.area;
        let local = |r: Rectangle<i32, Logical>| r.loc - area.loc;
        // The column shows on this screen.
        let column_here = || {
            shell.on.as_ref().is_some_and(|(on, ..)| *on == name) && self.column_shift().is_some()
        };
        let element = match &screen.bar {
            Some(Bar::Strip(p)) => p.element(renderer, output, local(strip_rect(area))),
            Some(Bar::Top(p)) => p.element(renderer, output, local(top_rect(area))),
            // Only while the column is all in.
            Some(Bar::Hint(p, _)) if !column_here() => {
                p.element(renderer, output, hint_place(area))
            }
            _ => None,
        };
        element.map(SceneElement::Frame).into_iter().collect()
    }

    /// The strip's or the bar's panel on glass and its shadows on
    /// `output`, output-local.
    pub(crate) fn panel_panes(&self, output: &Output) -> crate::scene::Panes {
        let Some(shell) = &self.shell else {
            return crate::scene::Panes::default();
        };
        let name = output.name();
        let Some(screen) = shell.panel.screens.iter().find(|s| s.output == name) else {
            return crate::scene::Panes::default();
        };
        let area = screen.area;
        let (rect, glass, shadows) = match &screen.bar {
            Some(Bar::Strip(p)) => (strip_rect(area), p.view.glass(), p.view.shadows()),
            Some(Bar::Top(p)) => (top_rect(area), p.view.glass(), p.view.shadows()),
            _ => return crate::scene::Panes::default(),
        };
        let origin = (rect.loc - area.loc).to_f64();
        crate::scene::Panes::of(crate::blur::Surface::Bar, origin, &glass, &shadows)
    }

    /// The strips' or the bars' interface for the control API, the primary
    /// screen's first.
    pub(crate) fn panel_ui(&self, out: &mut Vec<UiNode>) {
        let Some(shell) = &self.shell else {
            return;
        };
        for screen in &shell.panel.screens {
            match &screen.bar {
                Some(Bar::Strip(p)) => {
                    flatten(&p.view.access(), strip_rect(screen.area).loc, 0, out);
                }
                Some(Bar::Top(p)) => flatten(&p.view.access(), top_rect(screen.area).loc, 0, out),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_and_the_bar_sit_where_board_d12_has_them() {
        let area = Rectangle::new((0, 0).into(), (3440, 1440).into());
        assert_eq!(
            strip_rect(area),
            Rectangle::new((3352, 16).into(), (72, 1408).into())
        );
        assert_eq!(
            top_rect(area),
            Rectangle::new((16, 12).into(), (3408, 52).into())
        );
        assert_eq!(hint_place(area), Point::from((3432, 560)));
        assert!(in_band(area, Point::from((3439.0, 700.0))));
        assert!(!in_band(area, Point::from((3000.0, 700.0))));
        // A second screen's own, from its corner.
        let second = Rectangle::new((3440, 0).into(), (1600, 1000).into());
        assert_eq!(
            strip_rect(second),
            Rectangle::new((4952, 16).into(), (72, 968).into())
        );
        assert!(at_edge(second, Point::from((5039.0, 500.0))));
        assert!(!at_edge(area, Point::from((5039.0, 500.0))));
    }
}
