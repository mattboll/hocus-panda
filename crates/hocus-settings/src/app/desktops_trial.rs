//! "ESSAYER ICI" on the Bureaux et fenêtres page (6.14): the trial area
//! ([`crate::trial`]) drawn, fed the pointer, and the button that holds
//! the move key for it (the desktop takes Alt+drag before the window sees
//! it; the real key works too when the host lets it through). Nothing in
//! it is written.

use std::time::{Duration, Instant};

use iced::{
    Alignment, Background, Color, Length, Point, Rectangle, Renderer, Size, Theme,
    keyboard::Modifiers,
    mouse,
    widget::{button, canvas, column, container, row},
};

use super::{
    App, Message,
    desktops_page::{Msg, Stop},
    style::*,
    view::{El, t},
};
use crate::{
    desktops as model,
    lang::{Lang, w},
    model::Value,
    trial::{self, Rules, Said, Trial},
};
use hocus_geometry::{Rect, Zone};

/// The trial's own state on the page.
#[derive(Debug, Default)]
pub struct TrialState {
    pub trial: Trial,
    /// "Tenir Alt" pressed.
    pub held: bool,
    /// The modifiers the window is told are held.
    pub mods: Modifiers,
    /// The trial's clock starts here.
    origin: Option<Instant>,
}

impl TrialState {
    fn since(&mut self, now: Instant) -> Duration {
        let origin = *self.origin.get_or_insert(now);
        now.saturating_duration_since(origin)
    }

    fn at(&self, now: Instant) -> Duration {
        self.origin
            .map_or(Duration::ZERO, |o| now.saturating_duration_since(o))
    }
}

/// The key that moves a window: `"alt"`, `"super"` or `"none"`.
fn key_name(word: &str) -> &'static str {
    match word {
        "super" => "Super",
        "none" => "",
        _ => "Alt",
    }
}

impl App {
    fn trial_bool(&self, id: &str) -> bool {
        crate::model::setting(id)
            .map(|s| crate::store::Slot::of(s, self.source))
            .and_then(|s| self.value(s))
            != Some(Value::Bool(false))
    }

    fn trial_key(&self) -> &'static str {
        match crate::model::setting(model::MOVE_KEY)
            .map(|s| crate::store::Slot::of(s, self.source))
            .and_then(|s| self.value(s))
        {
            Some(Value::Word(word)) => word,
            _ => "alt",
        }
    }

    fn trial_rules(&self) -> Rules {
        Rules {
            magnet: self.trial_bool(model::MAGNET),
            snap: self.trial_bool(model::SNAP),
            wobble: self.trial_bool(model::WOBBLE),
            reduced: self.reduced_motion,
        }
    }

    /// Whether the move key is held for the trial: its button, or the key.
    fn trial_held(&self) -> bool {
        let mods = self.desktops.trial.mods;
        match self.trial_key() {
            "none" => false,
            "super" => self.desktops.trial.held || mods.logo(),
            _ => self.desktops.trial.held || mods.alt(),
        }
    }

    pub(super) fn trial_update(&mut self, msg: &Msg) {
        let now = Instant::now();
        let rules = self.trial_rules();
        let held = self.trial_held();
        let has_key = self.trial_key() != "none";
        let state = &mut self.desktops.trial;
        let at = state.since(now);
        match msg {
            Msg::TrialPress(x, y) => {
                state.trial.press((f64::from(*x), f64::from(*y)), held, at);
            }
            Msg::TrialMove(x, y) => {
                state
                    .trial
                    .motion((f64::from(*x), f64::from(*y)), rules, at);
            }
            Msg::TrialRelease(x, y) => {
                if x.is_finite() && y.is_finite() {
                    state
                        .trial
                        .motion((f64::from(*x), f64::from(*y)), rules, at);
                }
                state.trial.release(rules, at);
            }
            Msg::Hold if has_key => state.held = !state.held,
            _ => {}
        }
    }

    pub(super) fn trial_mods(&mut self, mods: Modifiers) {
        self.desktops.trial.mods = mods;
    }

    /// Whether the trial moves at `now`.
    pub(super) fn trial_moving(&self, now: Instant) -> bool {
        self.page == super::Page::Section(crate::model::SectionId::Desktops)
            && self
                .desktops
                .trial
                .trial
                .moving(self.desktops.trial.at(now))
    }

    /// What happened, as the first line under the screen says it.
    fn trial_said(&self) -> String {
        let lang = self.lang;
        let key = key_name(self.trial_key());
        let zone = |z: Zone| -> &'static str {
            match (z, lang) {
                (Zone::Left, Lang::French) => "la moitié gauche",
                (Zone::Right, Lang::French) => "la moitié droite",
                (Zone::Maximized, Lang::French) => "tout l’écran",
                (Zone::TopLeft, Lang::French) => "le quart en haut à gauche",
                (Zone::TopRight, Lang::French) => "le quart en haut à droite",
                (Zone::BottomLeft, Lang::French) => "le quart en bas à gauche",
                (Zone::BottomRight, Lang::French) => "le quart en bas à droite",
                (Zone::Left, Lang::English) => "the left half",
                (Zone::Right, Lang::English) => "the right half",
                (Zone::Maximized, Lang::English) => "the whole screen",
                (Zone::TopLeft, Lang::English) => "the top left quarter",
                (Zone::TopRight, Lang::English) => "the top right quarter",
                (Zone::BottomLeft, Lang::English) => "the bottom left quarter",
                (Zone::BottomRight, Lang::English) => "the bottom right quarter",
            }
        };
        match (self.desktops.trial.trial.said, lang) {
            (Said::Start, Lang::French) if key.is_empty() => {
                "Glisse « Notes » par sa barre de titre.".to_owned()
            }
            (Said::Start, Lang::English) if key.is_empty() => {
                "Drag “Notes” by its title bar.".to_owned()
            }
            (Said::Start, Lang::French) => {
                format!(
                    "Glisse « Notes » par sa barre de titre, ou par son contenu en tenant {key}."
                )
            }
            (Said::Start, Lang::English) => {
                format!("Drag “Notes” by its title bar, or by its content holding {key}.")
            }
            (Said::NeedsKey, Lang::French) if key.is_empty() => {
                "Sans touche choisie, seule sa barre de titre la déplace.".to_owned()
            }
            (Said::NeedsKey, Lang::English) if key.is_empty() => {
                "With no key chosen, only its title bar moves it.".to_owned()
            }
            (Said::NeedsKey, Lang::French) => format!(
                "Sans {key}, le clic irait à l’application. Tiens {key} (le bouton au-dessus) pour la saisir ici."
            ),
            (Said::NeedsKey, Lang::English) => format!(
                "Without {key}, the click would go to the application. Hold {key} (the button above) to grab it here."
            ),
            (Said::Stuck, Lang::French) => {
                "Collée au Terminal, à 8 px : glisse encore pour la décoller.".to_owned()
            }
            (Said::Stuck, Lang::English) => {
                "Stuck to the Terminal, 8 px away: drag on to free it.".to_owned()
            }
            (Said::Edge, Lang::French) => {
                "Collée au bord de l’écran, à 8 px : glisse encore pour la décoller.".to_owned()
            }
            (Said::Edge, Lang::English) => {
                "Stuck to the screen’s edge, 8 px in: drag on to free it.".to_owned()
            }
            (Said::Over(z), Lang::French) => format!("Lâche-la ici : {}.", zone(z)),
            (Said::Over(z), Lang::English) => format!("Let go here: {}.", zone(z)),
            (Said::Snapped(z), Lang::French) => {
                format!("Ancrée : {}. Glisse-la pour lui rendre sa taille.", zone(z))
            }
            (Said::Snapped(z), Lang::English) => {
                format!("Snapped: {}. Drag it to give it its size back.", zone(z))
            }
            (Said::Placed, Lang::French) => "Posée là où tu l’as lâchée.".to_owned(),
            (Said::Placed, Lang::English) => "Left where you let it go.".to_owned(),
        }
    }

    /// The motion's state, the second line.
    fn trial_motion(&self) -> &'static str {
        let rules = self.trial_rules();
        let words = if rules.reduced {
            w(
                "Mouvements réduits : elle se déplace d’un bloc, et prend sa place aussitôt.",
                "Reduced motion: it moves as one piece, and takes its place at once.",
            )
        } else if !rules.wobble {
            w(
                "Sans ondulation : elle se déplace d’un bloc.",
                "Without the wobble: it moves as one piece.",
            )
        } else {
            w(
                "Elle ondule pendant le geste et se pose en 400 ms.",
                "It wobbles as it moves and settles in 400 ms.",
            )
        };
        words.get(self.lang)
    }

    /// The trial's card: the key's button and its line, the screen, what
    /// happened and the motion.
    pub(super) fn trial_card(&self) -> El<'_> {
        let lang = self.lang;
        let key = key_name(self.trial_key());
        let none = key.is_empty();
        let held = self.desktops.trial.held && !none;
        let focused = matches!(self.focus, super::Focus::Control(i)
            if self.controls().get(i) == Some(&super::Control::Desktops(Stop::Hold)));
        let label = self.trial_hold_label();
        let alpha = if none { INERT } else { 1.0 };
        let mut hold = button(
            t(
                label,
                13.0,
                if held {
                    DEEP.get()
                } else {
                    fade(TEXT.get(), alpha)
                },
            )
            .font(BOLD),
        )
        .height(30)
        .padding([6, 14])
        .style(move |_, status| button::Style {
            background: Some(Background::Color(if held {
                ACCENT.get()
            } else {
                crate::theme::ink(rgba(
                    255,
                    255,
                    255,
                    if matches!(status, button::Status::Hovered) && !none {
                        0.10
                    } else {
                        0.06
                    },
                ))
            })),
            text_color: TEXT.get(),
            border: if focused {
                ring(8.0)
            } else {
                border(8.0, 1.0, fade(RIM_12.get(), alpha))
            },
            ..button::Style::default()
        });
        if !none {
            hold = hold.on_press(Message::Desktops(Msg::Hold));
        }
        let line = match (none, lang) {
            (true, _) => w(
                "Sans touche, « Notes » se déplace par sa barre de titre.",
                "With no key, “Notes” moves by its title bar.",
            )
            .get(lang)
            .to_owned(),
            (false, Lang::French) => {
                format!("Comme si tu tenais {key} : glisse « Notes » par son contenu.")
            }
            (false, Lang::English) => {
                format!("As if you held {key}: drag “Notes” by its content.")
            }
        };
        let now = self.desktops.trial.at(self.now);
        let trial = &self.desktops.trial.trial;
        let drawing = canvas(Drawing {
            notes: trial.drawn(now),
            shear: trial.shear_at(now),
            preview: trial.preview(),
            dragging: trial.dragging(),
            lang,
        })
        .width(trial::SCREEN.0 as f32)
        .height(trial::SCREEN.1 as f32);
        container(
            column![
                row![
                    container(hold).id("trial:hold".to_owned()),
                    t(line, 12.5, DIM.get())
                ]
                .spacing(12)
                .align_y(Alignment::Center),
                container(drawing).id("desktops:trial".to_owned()),
                column![
                    t(self.trial_said(), 12.5, TEXT.get()),
                    t(self.trial_motion(), 12.0, FAINT.get()),
                ]
                .spacing(4),
            ]
            .spacing(12),
        )
        .padding([14, 16])
        .width(Length::Fill)
        .into()
    }

    /// The move key's button: "Tenir Alt", "Alt tenue", "Aucune touche".
    fn trial_hold_label(&self) -> String {
        let lang = self.lang;
        let key = key_name(self.trial_key());
        let held = self.desktops.trial.held;
        match (key.is_empty(), held, lang) {
            (true, _, _) => w("Aucune touche", "No key").get(lang).to_owned(),
            (false, true, Lang::French) => format!("{key} tenue"),
            (false, true, Lang::English) => format!("{key} held"),
            (false, false, Lang::French) => format!("Tenir {key}"),
            (false, false, Lang::English) => format!("Hold {key}"),
        }
    }

    /// What the trial shows, for the log.
    pub(super) fn trial_shown(&self) -> serde_json::Value {
        let trial = &self.desktops.trial.trial;
        let n = trial.notes;
        serde_json::json!({
            "notes": [n.x, n.y, n.width, n.height],
            "snapped": trial.snapped().map(|z| format!("{z:?}")),
            "preview": trial.preview().map(|r| [r.x, r.y, r.width, r.height]),
            "said": self.trial_said(),
            "motion": self.trial_motion(),
            "held": self.desktops.trial.held,
            "hold": self.trial_hold_label(),
        })
    }
}

/// The screen as drawn.
#[derive(Debug)]
struct Drawing {
    notes: (f64, f64, f64, f64),
    shear: f64,
    preview: Option<Rect>,
    dragging: bool,
    lang: Lang,
}

fn point(x: f64, y: f64) -> Point {
    Point::new(x as f32, y as f32)
}

impl Drawing {
    /// A window: its body and title bar, the top lagging by `shear`.
    #[allow(clippy::too_many_arguments)]
    fn window(
        f: &mut canvas::Frame,
        (x, y, w, h): (f64, f64, f64, f64),
        shear: f64,
        title: &str,
        rim: Color,
        fill: Color,
        bar: Color,
    ) {
        let at = |down: f64| shear * (1.0 - down / h.max(1.0));
        if shear.abs() < 0.25 {
            let path = canvas::Path::rounded_rectangle(
                point(x, y),
                Size::new(w as f32, h as f32),
                6.0.into(),
            );
            f.fill(&path, fill);
            let top = canvas::Path::rounded_rectangle(
                point(x, y),
                Size::new(w as f32, f64::from(trial::TITLE) as f32),
                6.0.into(),
            );
            f.fill(&top, bar);
            f.stroke(
                &path,
                canvas::Stroke::default().with_color(rim).with_width(1.0),
            );
        } else {
            let quad = |y0: f64, y1: f64| {
                canvas::Path::new(|p| {
                    p.move_to(point(x - at(y0 - y), y0));
                    p.line_to(point(x + w - at(y0 - y), y0));
                    p.line_to(point(x + w - at(y1 - y), y1));
                    p.line_to(point(x - at(y1 - y), y1));
                    p.close();
                })
            };
            let body = quad(y, y + h);
            f.fill(&body, fill);
            f.fill(&quad(y, y + f64::from(trial::TITLE)), bar);
            f.stroke(
                &body,
                canvas::Stroke::default().with_color(rim).with_width(1.0),
            );
        }
        // On the CPU a text is not cut at the screen's edge: one that
        // would cross it is left out.
        let (tx, ty) = (x + 10.0 - at(11.0), y + 5.0);
        let room = f64::from(trial::SCREEN.0);
        if tx >= 2.0 && tx + 56.0 <= room && ty >= 0.0 && ty + 14.0 <= f64::from(trial::SCREEN.1) {
            f.fill_text(canvas::Text {
                content: title.to_owned(),
                position: point(tx, ty),
                color: rgba(238, 241, 246, 0.85),
                size: 11.0.into(),
                font: SEMIBOLD,
                ..canvas::Text::default()
            });
        }
        // A few lines of content.
        for (i, share) in [0.7, 0.5, 0.62].into_iter().enumerate() {
            let line_y = y + f64::from(trial::TITLE) + 14.0 + 14.0 * i as f64;
            if line_y + 6.0 > y + h - 6.0 {
                break;
            }
            let path = canvas::Path::rounded_rectangle(
                point(x + 10.0 - at(line_y - y), line_y),
                Size::new(((w - 20.0) * share) as f32, 5.0),
                2.5.into(),
            );
            f.fill(&path, rgba(238, 241, 246, 0.14));
        }
    }
}

impl canvas::Program<Message> for Drawing {
    type State = ();

    fn update(
        &self,
        (): &mut (),
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let local = |p: Point| (p.x - bounds.x, p.y - bounds.y);
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let at = cursor.position_in(bounds)?;
                Some(
                    canvas::Action::publish(Message::Desktops(Msg::TrialPress(at.x, at.y)))
                        .and_capture(),
                )
            }
            // The cursor as the page places it: scrolled with it.
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) if self.dragging => {
                let (x, y) = local(cursor.position()?);
                Some(canvas::Action::publish(Message::Desktops(Msg::TrialMove(
                    x, y,
                ))))
            }
            // Let go where the pointer is: a last motion not seen yet counts.
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if self.dragging =>
            {
                let (x, y) = cursor.position().map_or((f32::NAN, f32::NAN), local);
                Some(canvas::Action::publish(Message::Desktops(
                    Msg::TrialRelease(x, y),
                )))
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let screen = canvas::Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 9.0.into());
        f.fill(
            &screen,
            canvas::Gradient::Linear(
                canvas::gradient::Linear::new(
                    Point::ORIGIN,
                    Point::new(bounds.width * 0.4, bounds.height),
                )
                .add_stop(0.0, Color::from_rgb8(0x16, 0x1e, 0x31))
                .add_stop(1.0, Color::from_rgb8(0x0c, 0x11, 0x1d)),
            ),
        );
        f.stroke(
            &screen,
            canvas::Stroke::default()
                .with_color(rgba(255, 255, 255, 0.06))
                .with_width(1.0),
        );
        // A window past the screen's edge is cut there, its title too.
        f.with_clip(Rectangle::with_size(bounds.size()), |f| {
            let t = trial::TERMINAL;
            Drawing::window(
                f,
                (
                    f64::from(t.x),
                    f64::from(t.y),
                    f64::from(t.width),
                    f64::from(t.height),
                ),
                0.0,
                "Terminal",
                rgba(255, 255, 255, 0.18),
                Color::from_rgb8(0x1a, 0x21, 0x2f),
                Color::from_rgb8(0x22, 0x2a, 0x3a),
            );
            if let Some(p) = self.preview {
                let path = canvas::Path::rounded_rectangle(
                    Point::new(p.x as f32, p.y as f32),
                    Size::new(p.width as f32, p.height as f32),
                    8.0.into(),
                );
                f.fill(&path, crate::theme::Ink::Accent(0.12).get());
                f.stroke(
                    &path,
                    canvas::Stroke::default()
                        .with_color(ACCENT.get())
                        .with_width(1.0),
                );
            }
            Drawing::window(
                f,
                self.notes,
                self.shear,
                match self.lang {
                    Lang::French | Lang::English => "Notes",
                },
                ACCENT.get(),
                Color::from_rgb8(0x24, 0x2d, 0x3e),
                Color::from_rgb8(0x2c, 0x36, 0x49),
            );
        });
        vec![f.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        (): &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.dragging {
            return mouse::Interaction::Grabbing;
        }
        let Some(at) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        let (x, y, w, h) = self.notes;
        let over = f64::from(at.x) >= x
            && f64::from(at.x) < x + w
            && f64::from(at.y) >= y
            && f64::from(at.y) < y + f64::from(trial::TITLE).min(h);
        if over {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}
