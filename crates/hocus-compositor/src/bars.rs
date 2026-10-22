//! Title bars, as an option (decision 0006: off by default). A bar sits
//! above the client inside the window's frame: placement, snapping,
//! maximizing and tiling give the frame its rectangle, the client gets
//! what is left under the bar. Turning bars on or off keeps every frame
//! where it is. A press on a bar's buttons acts on release; elsewhere on
//! the bar it moves the window, and two presses close together maximize.

use std::{collections::BTreeMap, time::Duration};

use hocus_api::UiNode;
use hocus_core::WindowId;
use hocus_shell::{
    Lang,
    titlebar::{self, BarAction, Press, Shown, TitleBar},
};
use hocus_ui::Point as UiPoint;
use smithay::{
    backend::renderer::{
        ImportMem, Renderer,
        element::{
            Kind,
            memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
        },
    },
    desktop::Window,
    utils::{Logical, Physical, Point, Rectangle, SERIAL_COUNTER, Size},
};

use crate::{
    State,
    desktops::{model_id, title_and_app_id},
    grabs::BTN_LEFT,
    window_mode::WindowMode,
};

/// The bar's height, logical pixels.
pub const BAR: i32 = titlebar::HEIGHT as i32;
/// Two presses on a bar this close together maximize.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

struct Bar {
    bar: TitleBar,
    buffer: Option<MemoryRenderBuffer>,
    /// What it was built for: another width or scale builds it again.
    width: i32,
    scale: f64,
}

#[derive(Default)]
pub struct Bars {
    bars: BTreeMap<WindowId, Bar>,
    /// Whether windows are laid out with bars.
    on: bool,
    /// The bar under the pointer, and the one a press began on.
    hovered: Option<WindowId>,
    pressed: Option<WindowId>,
    last_press: Option<(WindowId, Duration)>,
}

impl std::fmt::Debug for Bars {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bars")
            .field("on", &self.on)
            .field("windows", &self.bars.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl State {
    /// Whether `window` gets a bar when bars are on: windows whose frame
    /// the compositor draws, but not the drop-down terminal.
    pub fn wants_bar(&self, window: &Window) -> bool {
        self.bars.on
            && crate::surface::server_side(window)
            && model_id(window).is_none_or(|id| self.dropdown_offset(id).is_none())
    }

    /// The bar above a window's client, logical pixels: 0 without one.
    pub fn bar_height(&self, id: WindowId) -> i32 {
        match self.windows.get(&id) {
            Some(m)
                if !matches!(m.mode, WindowMode::Fullscreen { .. })
                    && self.wants_bar(&m.window) =>
            {
                BAR
            }
            _ => 0,
        }
    }

    /// The client's part of a frame.
    pub fn client_rect(
        &self,
        id: WindowId,
        frame: Rectangle<i32, Logical>,
    ) -> Rectangle<i32, Logical> {
        let bar = self.bar_height(id);
        Rectangle::new(
            frame.loc + Point::from((0, bar)),
            (frame.size.w, (frame.size.h - bar).max(1)).into(),
        )
    }

    fn lang(&self) -> Lang {
        match self.clock {
            crate::clock::Clock::Monotonic { .. } => Lang::from_env(),
            crate::clock::Clock::Virtual { .. } => Lang::En,
        }
    }

    /// After each dispatch: windows follow the option, bars what their
    /// window shows.
    pub fn refresh_bars(&mut self) {
        let wanted = self.config.decorations.title_bars;
        if wanted != self.bars.on {
            self.toggle_bars(wanted);
        }
        if !wanted {
            return;
        }
        let theme = self.theme();
        let lang = self.lang();
        let left = self.config.decorations.buttons == "left";
        let radius = self.style.radius as f32;
        let focused = self.model.focused();
        let now = self.clock.now();
        let ids: Vec<WindowId> = self.windows.keys().copied().collect();
        let before = self.bars.bars.len();
        self.bars.bars.retain(|id, _| ids.contains(id));
        let mut changed = before != self.bars.bars.len();
        for id in ids {
            if self.bar_height(id) == 0 {
                changed |= self.bars.bars.remove(&id).is_some();
                continue;
            }
            let Some(managed) = self.windows.get(&id) else {
                continue;
            };
            let (title, app_id) = title_and_app_id(&managed.window);
            let scale = self
                .output_of(&managed.window)
                .map_or(1.0, |o| o.current_scale().fractional_scale());
            let width = managed.window.geometry().size.w;
            let shown = Shown {
                title: title
                    .filter(|t| !t.is_empty())
                    .or(app_id)
                    .unwrap_or_default(),
                focused: focused == Some(id),
                maximized: matches!(managed.mode, WindowMode::Maximized { .. }),
            };
            let stale = self
                .bars
                .bars
                .get(&id)
                .is_none_or(|b| b.width != width || b.scale != scale);
            if stale {
                let bar = TitleBar::new(
                    theme.clone(),
                    lang,
                    width as f32,
                    scale as f32,
                    radius,
                    left,
                );
                self.bars.bars.insert(
                    id,
                    Bar {
                        bar,
                        buffer: None,
                        width,
                        scale,
                    },
                );
            }
            let Some(bar) = self.bars.bars.get_mut(&id) else {
                continue;
            };
            if bar.bar.theme() != &theme {
                bar.bar.set_theme(theme.clone());
            }
            bar.bar.show(shown);
            let drawn = bar.bar.draw(now);
            match &mut bar.buffer {
                Some(buffer) => {
                    if !drawn.is_empty() {
                        crate::shell::upload(buffer, &bar.bar.view(), &drawn);
                        changed = true;
                    }
                }
                None => {
                    bar.buffer = Some(crate::shell::buffer_for(&bar.bar.view()));
                    changed = true;
                }
            }
        }
        if changed {
            self.needs_redraw = true;
        }
    }

    /// Every frame stays where it is: turned on, the bar takes the top of
    /// the client; turned off, the client takes the bar's place.
    fn toggle_bars(&mut self, on: bool) {
        let frames: Vec<(WindowId, Rectangle<i32, Logical>)> = self
            .windows
            .iter()
            .filter(|(_, m)| matches!(m.mode, WindowMode::Normal))
            .filter_map(|(id, _)| self.current_rect(*id).map(|r| (*id, r)))
            .collect();
        self.bars.on = on;
        if !on {
            self.bars.bars.clear();
            self.bars.hovered = None;
            self.bars.pressed = None;
        }
        for (id, frame) in frames {
            self.resize_to(id, frame, false);
        }
        // Maximized, snapped and tiled windows are laid out again.
        self.relayout();
        self.layout_tiles();
        self.needs_redraw = true;
    }

    /// The bar under `p`, if a window's bar is the topmost thing there, and
    /// `p` in the bar's coordinates.
    fn bar_at(&self, p: Point<f64, Logical>) -> Option<(WindowId, UiPoint)> {
        if self.bars.bars.is_empty() {
            return None;
        }
        for window in self.space.elements().rev() {
            let Some(location) = self.space.element_location(window) else {
                continue;
            };
            let size = window.geometry().size;
            if Rectangle::<i32, Logical>::new(location, size)
                .to_f64()
                .contains(p)
            {
                return None;
            }
            let Some(id) = model_id(window).filter(|id| self.bars.bars.contains_key(id)) else {
                continue;
            };
            let bar = Rectangle::<i32, Logical>::new(
                location - Point::from((0, BAR)),
                (size.w, BAR).into(),
            )
            .to_f64();
            if bar.contains(p) {
                let at = UiPoint::new((p.x - bar.loc.x) as f32, (p.y - bar.loc.y) as f32);
                return Some((id, at));
            }
        }
        None
    }

    /// The pointer moved: whether a bar has it.
    pub fn bars_pointer_motion(&mut self, location: Point<f64, Logical>) -> bool {
        let hit = self.bar_at(location);
        let over = hit.map(|(id, _)| id);
        if over != self.bars.hovered {
            if let Some(old) = self.bars.hovered.and_then(|id| self.bars.bars.get_mut(&id)) {
                old.bar.pointer_leave();
            }
            self.bars.hovered = over;
        }
        match hit {
            Some((id, at)) => {
                if let Some(bar) = self.bars.bars.get_mut(&id) {
                    bar.bar.pointer_move(at);
                }
                true
            }
            None => false,
        }
    }

    /// A button pressed or released: whether a bar took it.
    pub fn bars_pointer_button(
        &mut self,
        location: Point<f64, Logical>,
        button: u32,
        pressed: bool,
    ) -> bool {
        let serial = SERIAL_COUNTER.next_serial();
        if !pressed {
            let Some(id) = self.bars.pressed.take() else {
                return false;
            };
            let action = self.bars.bars.get_mut(&id).and_then(|b| b.bar.release());
            let window = self.windows.get(&id).map(|m| m.window.clone());
            match (action, window) {
                (Some(BarAction::Minimize), _) => self.minimize(id, serial),
                (Some(BarAction::ToggleMaximized), _) => self.toggle_maximized(id, serial),
                (Some(BarAction::Close), Some(window)) => crate::surface::close(&window),
                _ => {}
            }
            self.needs_redraw = true;
            return true;
        }
        let Some((id, at)) = self.bar_at(location) else {
            return false;
        };
        let Some(window) = self.windows.get(&id).map(|m| m.window.clone()) else {
            return false;
        };
        self.focus(Some(&window), serial);
        // The window menu comes with #40; until then other buttons only
        // focus.
        if button != BTN_LEFT {
            return true;
        }
        let Some(bar) = self.bars.bars.get_mut(&id) else {
            return false;
        };
        bar.bar.pointer_move(at);
        let press = bar.bar.press(at);
        self.bars.pressed = Some(id);
        if press == Press::Bar {
            let now = self.clock.now();
            let double = self
                .bars
                .last_press
                .is_some_and(|(w, t)| w == id && now.saturating_sub(t) <= DOUBLE_CLICK);
            if double {
                self.bars.last_press = None;
                self.toggle_maximized(id, serial);
            } else {
                self.bars.last_press = Some((id, now));
                // The move grab takes the release.
                self.bars.pressed = None;
                let _ = self.bars.bars.get_mut(&id).map(|b| b.bar.release());
                self.start_move(&window, button, None, serial);
            }
        }
        self.needs_redraw = true;
        true
    }

    /// A window's bar, at the top of its frame; `client` is where its
    /// client is drawn, relative to the output.
    pub fn bar_element<R>(
        &self,
        renderer: &mut R,
        id: WindowId,
        client: Point<i32, Logical>,
        scale: f64,
    ) -> Option<MemoryRenderBufferRenderElement<R>>
    where
        R: Renderer + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let bar = self.bars.bars.get(&id)?;
        let buffer = bar.buffer.as_ref()?;
        let v = bar.bar.view();
        let all = Rectangle::from_size(Size::from((f64::from(v.width), f64::from(v.height))));
        let size =
            Size::<f64, Logical>::from((f64::from(v.width) / scale, f64::from(v.height) / scale))
                .to_i32_round();
        let at = Point::<i32, Logical>::from((client.x, client.y - BAR));
        let location: Point<f64, Physical> = at.to_f64().to_physical(scale);
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            location,
            buffer,
            None,
            Some(all),
            Some(size),
            Kind::Unspecified,
        )
        .inspect_err(|err| tracing::warn!(?err, "could not draw a title bar"))
        .ok()
    }

    /// The bars for the control API, in global coordinates.
    pub fn bars_ui(&self, out: &mut Vec<UiNode>) {
        for (id, bar) in &self.bars.bars {
            let Some(location) = self
                .windows
                .get(id)
                .and_then(|m| self.space.element_location(&m.window))
            else {
                continue;
            };
            let origin = Point::from((location.x, location.y - BAR));
            crate::shell::flatten(&bar.bar.access(), origin, 0, out);
        }
    }
}
