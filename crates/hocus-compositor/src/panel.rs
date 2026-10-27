//! The panel's forms in the compositor (#263, decision 0011). The widget
//! column is always there in form `column`; in the others, what stays on
//! the primary screen is a strip on the right, a bar at the top, or a
//! faint hint at the right edge, and the whole column slides out over the
//! windows when asked for: the pointer resting on the strip or at the edge,
//! a click on the strip or the bar, or the shortcut (Super+C). It slides
//! back when the pointer leaves it, on Escape, on the shortcut again or a
//! click elsewhere. Windows leave room for what stays only.
//!
//! The slide is one value, evaluated in closed form at the clock's time
//! when the scene is drawn: frames are asked for while it moves, none at
//! rest. With reduced motion (or in tests that do not ask for motion) the
//! column is out or in at once.

use std::time::Duration;

use hocus_api::UiNode;
use hocus_core::config::PanelForm;
use hocus_shell::{
    Action as ShellAction, ColumnState, Lang,
    column::{MARGIN, View, WIDTH},
    strip::{self, EdgeHint, Strip},
    topbar::{self, TopBar},
};
use hocus_ui::{
    Input, Point as UiPoint, Theme,
    motion::{Animated, Curve, Easing, SMALL, STANDARD},
};
use smithay::{
    backend::renderer::{ImportAll, ImportMem, Renderer, Texture},
    output::Output,
    utils::{Logical, Point, Rectangle},
};

use crate::{
    State,
    alarm::Alarm,
    scene::SceneElement,
    shell::{Panel, Pixels, column_rect, flatten},
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Rest {
    #[default]
    Away,
    /// Resting there since then.
    Since(Duration),
    /// It brought the column out, or the column was sent back while it
    /// stayed: it must leave before resting there counts again.
    Spent,
}

/// The panel's form and what it shows besides the column.
#[derive(Debug)]
pub struct Forms {
    /// The form drawn, to see the configuration change it.
    form: Option<PanelForm>,
    bar: Option<Bar>,
    theme: Theme,
    lang: Lang,
    pub(crate) out: Out,
    /// 1 out, 0 in.
    slide: Animated,
    /// It moved at the last frame: one more shows where it ends.
    sliding: bool,
    rest: Rest,
    alarm: Alarm,
    /// The pointer over the top bar, and a press begun on it.
    bar_inside: bool,
    bar_pressed: bool,
}

impl Forms {
    pub fn new(theme: Theme, lang: Lang) -> Self {
        Self {
            form: None,
            bar: None,
            theme,
            lang,
            out: Out::In,
            slide: Animated::new(0.0),
            sliding: false,
            rest: Rest::Away,
            alarm: Alarm::default(),
            bar_inside: false,
            bar_pressed: false,
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        if theme == self.theme {
            return;
        }
        self.theme = theme.clone();
        match &mut self.bar {
            Some(Bar::Strip(p)) => p.view.set_theme(theme),
            Some(Bar::Top(p)) => p.view.set_theme(theme),
            // Made again with the new accent.
            Some(Bar::Hint(..)) => self.bar = None,
            None => {}
        }
    }

    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
        match &mut self.bar {
            Some(Bar::Strip(p)) => p.view.set_lang(lang),
            Some(Bar::Top(p)) => p.view.set_lang(lang),
            _ => {}
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

impl State {
    /// The panel's form in effect.
    pub(crate) fn panel_form(&self) -> PanelForm {
        self.config.panel.form
    }

    /// Whether panels move as they come and go now.
    fn panel_moves(&self) -> bool {
        !self.instant_motion && !self.config.appearance.reduced_motion
    }

    /// What the panel keeps on `output` at rest, which windows leave room
    /// for: the column, the strip or the top bar; nothing when hidden.
    pub fn panel_rect(&self, output: &Output) -> Option<Rectangle<i32, Logical>> {
        let (name, area, _) = self.shell.as_ref()?.on.as_ref()?;
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

    /// After each dispatch, the column drawn: follows the form, shows the
    /// strip or the bar, brings the column out once the pointer rested
    /// long enough. Whether something to draw changed.
    pub(crate) fn refresh_panel(&mut self, state: &ColumnState) -> bool {
        let form = self.panel_form();
        let now = self.clock.now();
        let moves = self.panel_moves();
        let handle = self.waker().cloned();
        let fullscreen = self.column_output().is_some_and(|o| self.fullscreen_on(&o));
        let Some(shell) = self.shell.as_mut() else {
            return false;
        };
        let mut changed = false;
        let forms = &mut shell.panel;
        if forms.form != Some(form) {
            // Another form: the column is in, and what stays made anew.
            forms.form = Some(form);
            forms.bar = None;
            forms.out = Out::In;
            forms.slide.jump(0.0);
            forms.rest = Rest::Away;
            forms.alarm.set(handle.as_ref(), "panel rest", None, now);
            shell.column.close_popover();
            shell.column.pointer_left();
            changed = true;
        }
        // Nothing is made or copied while the form and the screen stay.
        let place = shell.on.as_ref().map(|(_, area, scale)| (*area, *scale));
        match (form, place) {
            (PanelForm::Strip, Some((area, scale))) => {
                let height = strip_rect(area).size.h as f32;
                match &mut forms.bar {
                    Some(Bar::Strip(p)) => p.view.resize(height, scale as f32),
                    _ => {
                        let strip =
                            Strip::new(forms.theme.clone(), forms.lang, height, scale as f32);
                        forms.bar = Some(Bar::Strip(Panel::new(strip)));
                    }
                }
            }
            (PanelForm::Top, Some((area, scale))) => {
                let width = top_rect(area).size.w as f32;
                match &mut forms.bar {
                    Some(Bar::Top(p)) => p.view.resize(width, scale as f32),
                    _ => {
                        let bar = TopBar::new(forms.theme.clone(), forms.lang, width, scale as f32);
                        forms.bar = Some(Bar::Top(Panel::new(bar)));
                    }
                }
            }
            (PanelForm::Hidden, Some((_, scale))) => {
                if !matches!(forms.bar, Some(Bar::Hint(_, s)) if s == scale) {
                    let mut hint = Panel::new(EdgeHint::new(forms.theme.clone(), scale as f32));
                    hint.upload(&[]);
                    forms.bar = Some(Bar::Hint(hint, scale));
                    changed = true;
                }
            }
            _ => forms.bar = None,
        }
        match &mut forms.bar {
            Some(Bar::Strip(p)) => {
                p.view.set_state(state);
                let drawn = p.view.draw(now);
                changed |= p.upload(&drawn) || p.view.is_animating(now);
            }
            Some(Bar::Top(p)) => {
                p.view.set_state(state);
                let drawn = p.view.draw(now);
                changed |= p.upload(&drawn) || p.view.is_animating(now);
            }
            Some(Bar::Hint(..)) | None => {}
        }
        // Rested long enough where the column comes out from.
        if let Rest::Since(since) = forms.rest {
            let rest = if form == PanelForm::Hidden {
                EDGE_REST
            } else {
                STRIP_REST
            };
            if now >= since + rest {
                forms.rest = Rest::Spent;
                forms.alarm.set(handle.as_ref(), "panel rest", None, now);
                if forms.out == Out::In && !fullscreen {
                    forms.open(Out::Hovered, now, moves);
                    changed = true;
                }
            }
        }
        // While it slides, and once more where it ends.
        let sliding = forms.slide.is_running(now);
        changed |= sliding || forms.sliding;
        forms.sliding = sliding;
        changed
    }

    /// The pointer moved to `location`, the column not taking it: the
    /// column out goes back when the pointer left its band; resting on the
    /// strip or at the edge counts; the top bar's buttons follow it.
    /// Whether the strip or the bar took it.
    pub(crate) fn panel_pointer_motion(&mut self, location: Point<f64, Logical>) -> bool {
        let form = self.panel_form();
        if form == PanelForm::Column {
            return false;
        }
        let now = self.clock.now();
        let moves = self.panel_moves();
        let handle = self.waker().cloned();
        let fullscreen = self.column_output().is_some_and(|o| self.fullscreen_on(&o));
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
        let a = area.to_f64();
        let on_strip = form == PanelForm::Strip && strip_rect(area).to_f64().contains(location);
        let trigger = !fullscreen
            && match form {
                PanelForm::Strip => on_strip,
                PanelForm::Hidden => {
                    location.x >= a.loc.x + a.size.w - EDGE
                        && location.x <= a.loc.x + a.size.w
                        && location.y >= a.loc.y
                        && location.y <= a.loc.y + a.size.h
                }
                PanelForm::Top | PanelForm::Column => false,
            };
        if !trigger {
            forms.rest = Rest::Away;
            forms.alarm.set(handle.as_ref(), "panel rest", None, now);
        } else if forms.rest == Rest::Away && forms.out == Out::In {
            forms.rest = Rest::Since(now);
            let rest = if form == PanelForm::Hidden {
                EDGE_REST
            } else {
                STRIP_REST
            };
            forms
                .alarm
                .set(handle.as_ref(), "panel rest", Some(now + rest), now);
        }
        // The top bar's buttons show the pointer over them.
        if let Some(Bar::Top(panel)) = &mut forms.bar {
            let bar = top_rect(area);
            let at = UiPoint::new(
                (location.x - f64::from(bar.loc.x)) as f32,
                (location.y - f64::from(bar.loc.y)) as f32,
            );
            if bar.to_f64().contains(location) || forms.bar_pressed {
                forms.bar_inside = true;
                panel.view.input(at, Input::PointerMove(at));
                self.needs_redraw = true;
                return true;
            }
            if forms.bar_inside {
                forms.bar_inside = false;
                panel.view.pointer_left();
                self.needs_redraw = true;
            }
        }
        self.needs_redraw |= forms.slide.is_running(now);
        on_strip
    }

    /// The primary button, the column not taking it: a press on the strip
    /// brings the column out, the top bar's buttons act, a press elsewhere
    /// sends the column back. Whether the strip or the bar took it.
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
        let Some(shell) = self.shell.as_mut() else {
            return false;
        };
        let Some(area) = shell.on.as_ref().map(|(_, area, _)| *area) else {
            return false;
        };
        let forms = &mut shell.panel;
        // The top bar: its buttons act; elsewhere on it, the column comes
        // out or goes back.
        if let Some(Bar::Top(panel)) = &mut forms.bar {
            let bar = top_rect(area);
            let on_bar = bar.to_f64().contains(location);
            if (pressed && on_bar) || (!pressed && forms.bar_pressed) {
                forms.bar_pressed = pressed;
                let at = UiPoint::new(
                    (location.x - f64::from(bar.loc.x)) as f32,
                    (location.y - f64::from(bar.loc.y)) as f32,
                );
                let actions = panel.view.input(at, Input::PointerButton { pressed });
                self.needs_redraw = true;
                for action in actions {
                    if action == ShellAction::OpenPanel {
                        self.toggle_panel_from(Out::Asked);
                    } else {
                        self.shell_action(action);
                    }
                }
                return true;
            }
        }
        let forms = &mut shell.panel;
        if !pressed {
            return false;
        }
        // A press elsewhere sends the column back.
        if forms.out != Out::In && !in_band(area, location) {
            forms.close(now, moves);
            forms.rest = Rest::Away;
            shell.column.close_popover();
            self.needs_redraw = true;
            return false;
        }
        if form == PanelForm::Strip
            && forms.out == Out::In
            && strip_rect(area).to_f64().contains(location)
        {
            forms.open(Out::Hovered, now, moves);
            forms.rest = Rest::Spent;
            self.needs_redraw = true;
            return true;
        }
        false
    }

    /// The column out, or back (Super+C): nothing in form `column`.
    pub fn toggle_panel(&mut self) {
        self.toggle_panel_from(Out::Asked);
    }

    fn toggle_panel_from(&mut self, how: Out) {
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
            forms.open(how, now, moves);
        } else {
            forms.close(now, moves);
            forms.rest = Rest::Spent;
            shell.column.close_popover();
            shell.column.pointer_left();
        }
        self.needs_redraw = true;
    }

    /// Escape: the column out goes back. Whether it was out.
    pub fn panel_escape(&mut self) -> bool {
        if !self.panel_out() {
            return false;
        }
        self.toggle_panel_from(Out::Asked);
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
        let Some((name, area, _)) = &shell.on else {
            return Vec::new();
        };
        if *name != output.name() || self.fullscreen_on(output) {
            return Vec::new();
        }
        let local = |r: Rectangle<i32, Logical>| r.loc - area.loc;
        let element = match &shell.panel.bar {
            Some(Bar::Strip(p)) => p.element(renderer, output, local(strip_rect(*area))),
            Some(Bar::Top(p)) => p.element(renderer, output, local(top_rect(*area))),
            // Only while the column is all in.
            Some(Bar::Hint(p, _)) if self.column_shift().is_none() => {
                p.element(renderer, output, hint_place(*area))
            }
            _ => None,
        };
        element.map(SceneElement::Frame).into_iter().collect()
    }

    /// The strip's or the bar's panel on glass, output-local.
    pub(crate) fn panel_glass(&self, output: &Output) -> Vec<crate::blur::Spot> {
        let Some(shell) = &self.shell else {
            return Vec::new();
        };
        let Some((name, area, _)) = &shell.on else {
            return Vec::new();
        };
        if *name != output.name() {
            return Vec::new();
        }
        let (rect, glass) = match &shell.panel.bar {
            Some(Bar::Strip(p)) => (strip_rect(*area), p.view.glass()),
            Some(Bar::Top(p)) => (top_rect(*area), p.view.glass()),
            _ => return Vec::new(),
        };
        let origin = (rect.loc - area.loc).to_f64();
        crate::blur::spots(crate::blur::Surface::Bar, origin, &glass)
    }

    /// The strip's or the bar's interface for the control API.
    pub(crate) fn panel_ui(&self, out: &mut Vec<UiNode>) {
        let Some(shell) = &self.shell else {
            return;
        };
        let Some((_, area, _)) = &shell.on else {
            return;
        };
        match &shell.panel.bar {
            Some(Bar::Strip(p)) => flatten(&p.view.access(), strip_rect(*area).loc, 0, out),
            Some(Bar::Top(p)) => flatten(&p.view.access(), top_rect(*area).loc, 0, out),
            _ => {}
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
    }
}
