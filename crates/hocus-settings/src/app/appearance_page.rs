//! The "Apparence et langue" page's own controls (6.2, variant A): the
//! segmented controls (the theme, the cursor's size, the time), the
//! accents' swatches, the cursor's preview and the link to La barre. The
//! rows, their saving and their search are the window's as on any page
//! ([`crate::appearance`]); the window itself is the page's preview.

use crate::lang::French as _;
use hocus_link::theme::{Accent, Scheme};
use hocus_words::settings::app_appearance_page as words;
use iced::{
    Alignment, Background, Color, Point, Rectangle, Renderer, Shadow, Theme, Vector, mouse,
    widget::{Space, button, canvas, container, row},
};

use super::{
    App, Doing, KeysSaid, Message,
    style::*,
    view::{El, t},
};
use crate::{
    appearance::{self, Before, CURSOR_SIZES, Said, today_as},
    lang::{Lang, w},
    model::{Choice, Kind, SectionId, Setting, Value},
    store::{KeyEdit, Reading, Slot},
    theme::Ink,
};

/// The swatches: 24 px, 9 px apart; the chosen one ringed, 2 px of the
/// window between, 2 px in its colour.
const SWATCH: f32 = 24.0;
/// 9 px between the colours: their rings' room taken off.
const SWATCH_GAP: f32 = 1.0;

impl App {
    /// The theme chosen now, as the file says it.
    fn scheme_now(&self) -> Scheme {
        match self.value(Slot {
            setting: appearance::THEME_ID,
            source: None,
        }) {
            Some(Value::Word(word)) => Scheme::of(word).unwrap_or_default(),
            _ => Scheme::default(),
        }
    }

    /// The word the file says at `setting`'s slot; `None` where it says
    /// nothing (or what it says is not understood).
    fn word_at(&self, setting: &'static str) -> Option<String> {
        match self.reading(Slot {
            setting,
            source: None,
        }) {
            Reading::Value(Value::Word(word)) => Some(word.to_owned()),
            _ => None,
        }
    }

    /// Choosing a theme that changes more than itself (#431, #740): the
    /// theme and what it changes (its accent, its windows' motion, its
    /// wallpaper; or, leaving it, what they were) written together, one
    /// toast saying them all, whose "Annuler" (or `Ctrl+Z`) takes them all
    /// back. `false`: a plain change, as any other setting's.
    pub(super) fn theme_chose(&mut self, slot: Slot, value: &Value) -> bool {
        let (appearance::THEME_ID, Value::Word(word)) = (slot.setting, value) else {
            return false;
        };
        let Some(to) = Scheme::of(word) else {
            return false;
        };
        let from = self.scheme_now();
        let now = Said {
            accent: self.word_at(appearance::ACCENT_ID),
            animation: self.word_at(crate::desktops::ANIMATION),
            picture: self
                .snapshot
                .as_ref()
                .and_then(|s| s.wallpaper.picture.clone()),
        };
        let state = hocus_config::state_dir();
        let before = Before::read(state.as_deref());
        let (proposed, remember) = appearance::theme_proposes(from, to, &now, &before);
        if proposed.is_empty() || !self.writable() {
            return false;
        }
        remember.write(state.as_deref());
        let lang = self.lang;
        let mut said = vec![self.said_change(slot, value)];
        let mut edits = vec![KeyEdit {
            key: vec!["appearance".into(), "theme".into()],
            value: Some(format!("\"{}\"", to.word())),
        }];
        for (key, word) in proposed {
            said.push(self.said_key(key, word.as_deref()));
            edits.push(KeyEdit {
                key: key.iter().map(|k| (*k).to_owned()).collect(),
                value: word.map(|w| format!("\"{w}\"")),
            });
        }
        let theme_name = slot.setting().map_or("", |s| s.name.get(lang));
        let then = self.said_value(slot);
        let done = said.join(" · ");
        let keys_said = KeysSaid {
            undone: words::undone_is_back_to(
                lang,
                words::UndoneIsBackTo {
                    theme_name,
                    then: &then,
                },
            ),
            redone: words::redone(lang, &done),
            done,
            row: appearance::THEME_ID,
        };
        self.ask_keys(edits, keys_said, Doing::New, None);
        true
    }

    /// "Thème : Givre", as a theme's toast says each of its changes.
    fn said_change(&self, slot: Slot, value: &Value) -> String {
        let Some(setting) = slot.setting() else {
            return String::new();
        };
        let name = setting.name.get(self.lang);
        let now = setting.say(value, self.lang);
        words::text(self.lang, words::TextArgs { name, now: &now })
    }

    /// What a theme's toast says of `key` set to `word` (`None`: its
    /// default): "Couleur d'accent : Vert phosphore", "Fond d'écran : Pluie".
    fn said_key(&self, key: [&str; 2], word: Option<&str>) -> String {
        let lang = self.lang;
        if key == appearance::PICTURE_KEY {
            let picture = word
                .and_then(|w| hocus_link::wallpaper::Picture::of(w).ok())
                .unwrap_or_default();
            let name = crate::wallpaper::name_of(&picture, lang);
            return words::wallpaper(lang, &name);
        }
        let id = if key == appearance::ACCENT_KEY {
            appearance::ACCENT_ID
        } else {
            crate::desktops::ANIMATION
        };
        let Some(setting) = crate::model::setting(id) else {
            return String::new();
        };
        let default = setting.places.of(crate::model::Source::Ac).default.clone();
        // The word as one of the setting's own (they are `'static`).
        let value = word
            .and_then(|w| match &setting.kind {
                Kind::Segmented(choices) | Kind::Menu(choices) => choices
                    .iter()
                    .find(|c| matches!(c.value, Value::Word(v) if v == w))
                    .map(|c| c.value.clone()),
                Kind::Swatches => Accent::of(w).map(|a| Value::Word(a.word())),
                _ => None,
            })
            .unwrap_or(default);
        self.said_change(
            Slot {
                setting: id,
                source: None,
            },
            &value,
        )
    }

    /// The accents the swatches offer now (a themed theme's own first).
    pub(super) fn accents_shown(&self) -> Vec<Accent> {
        let chosen = crate::appearance::accent_of(
            self.value(Slot {
                setting: appearance::ACCENT_ID,
                source: None,
            })
            .as_ref(),
        );
        appearance::accents_shown(self.scheme_now(), chosen)
    }

    /// A segmented control: its choices side by side, the current one lit;
    /// a value of the file that is none of them shows as one more (the
    /// cursor's size at 40: "40").
    pub(super) fn segmented_control(
        &self,
        setting: &'static Setting,
        slot: Slot,
        choices: &[Choice],
        inert: bool,
        focused: bool,
    ) -> El<'_> {
        let alpha = if inert { INERT } else { 1.0 };
        let current = self.value(slot);
        // Words in Manrope, numbers and times in Martian Mono.
        let mono = !matches!(setting.id, "appearance.theme" | crate::bar::SIDE);
        let mut segments = row![];
        let mut shown: Vec<(String, Value)> = choices
            .iter()
            .map(|c| (c.label.get(self.lang).to_owned(), c.value.clone()))
            .collect();
        if let Some(value) = &current
            && !choices.iter().any(|c| &c.value == value)
            && let Value::Int(n) = value
        {
            shown.push((n.to_string(), value.clone()));
        }
        for (i, (label, value)) in shown.into_iter().enumerate() {
            let chosen = current.as_ref() == Some(&value);
            let colour = if chosen { TEXT.get() } else { DIM.get() };
            let word = t(label, if mono { 11.5 } else { 12.5 }, fade(colour, alpha))
                .font(if mono { MONO } else { SEMIBOLD });
            let mut segment =
                button(word)
                    .height(26)
                    .padding([4, 12])
                    .style(move |_, _| button::Style {
                        background: chosen.then(|| {
                            Background::Color(fade(Ink::Token(|t| t.segment_chosen).get(), alpha))
                        }),
                        text_color: TEXT.get(),
                        border: border(7.0, 0.0, Color::TRANSPARENT),
                        shadow: if chosen && crate::theme::light() {
                            Shadow {
                                color: rgba(20, 25, 38, 0.12),
                                offset: Vector::new(0.0, 1.0),
                                blur_radius: 1.0,
                            }
                        } else {
                            Shadow::default()
                        },
                        ..button::Style::default()
                    });
            if !inert && !chosen {
                segment = segment.on_press(Message::Choose(slot, value.clone()));
            }
            segments = segments.push(container(segment).id(format!("segment:{}:{i}", setting.id)));
        }
        let track = container(segments)
            .padding(2)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(fade(
                    Ink::Token(|t| t.segmented).get(),
                    alpha,
                ))),
                border: if focused {
                    ring(9.0)
                } else {
                    border(9.0, 0.0, Color::TRANSPARENT)
                },
                ..container::Style::default()
            });
        if setting.id == "appearance.cursor-size" {
            let pixels = match current {
                Some(Value::Int(n)) => n,
                _ => CURSOR_SIZES[0],
            };
            return row![cursor_preview(pixels), track]
                .spacing(12)
                .align_y(Alignment::Center)
                .into();
        }
        track.into()
    }

    /// The accents' swatches: a radio group, the chosen one ringed, its
    /// name on the row's line.
    pub(super) fn swatches(&self, slot: Slot, inert: bool, focused: bool) -> El<'_> {
        let alpha = if inert { INERT } else { 1.0 };
        let chosen = crate::appearance::accent_of(self.value(slot).as_ref());
        let mut swatches = row![].spacing(SWATCH_GAP).align_y(Alignment::Center);
        for accent in self.accents_shown() {
            let [r, g, b] = accent.colours().base;
            let fill = Color::from_rgb8(r, g, b);
            let on = accent == chosen;
            let mut swatch = button(Space::new().width(SWATCH).height(SWATCH))
                .width(SWATCH)
                .height(SWATCH)
                .padding(0)
                .style(move |_, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: Some(Background::Color(fade(
                            if hovered && !on {
                                fill.scale_alpha(0.85)
                            } else {
                                fill
                            },
                            alpha,
                        ))),
                        border: border(SWATCH / 2.0, 0.0, Color::TRANSPARENT),
                        ..button::Style::default()
                    }
                });
            if !inert && !on {
                swatch = swatch.on_press(Message::Choose(slot, Value::Word(accent.word())));
            }
            // The ring: 2 px of the window, then 2 px in its colour (the
            // accent's while the group holds the keyboard).
            let ring_colour = if focused { ACCENT.get() } else { fill };
            let ringed = container(swatch)
                .padding(4)
                .style(move |_: &Theme| container::Style {
                    border: if on {
                        border(SWATCH / 2.0 + 4.0, 2.0, fade(ring_colour, alpha))
                    } else {
                        border(SWATCH / 2.0 + 4.0, 0.0, Color::TRANSPARENT)
                    },
                    ..container::Style::default()
                });
            swatches = swatches.push(container(ringed).id(format!("swatch:{}", accent.word())));
        }
        swatches.into()
    }

    /// "La barre ›": opens that section.
    pub(super) fn link_control(&self, to: SectionId) -> El<'_> {
        let said = format!("{} ›", to.name(self.lang));
        button(t(said, 13.5, ACCENT_TEXT_INK.get()).font(SEMIBOLD))
            .padding([4, 6])
            .on_press(Message::Section(to))
            .style(flat(
                None,
                crate::theme::ink(rgba(255, 255, 255, 0.05)),
                7.0,
                false,
            ))
            .into()
    }

    /// What a menu's choice says beside its label: today's date in that
    /// format, the system's language's name.
    pub(super) fn choice_aside(&self, setting: &Setting, choice: &Choice) -> Option<String> {
        match (setting.id, &choice.value) {
            ("locale.date", Value::Word(format)) => {
                Some(today_as(format, jiff::Zoned::now().date(), self.lang))
            }
            ("locale.language", Value::Word("system")) => Some(system_language(self.lang)),
            (id, value) => crate::agents::aside(id, value, self.lang).map(ToOwned::to_owned),
        }
    }

    /// A menu's value as its button says it: the system's language with
    /// its name, "Celle du système (français)".
    pub(super) fn menu_said(&self, setting: &Setting, value: &Value) -> String {
        if let Some(said) = self.bar_menu_said(setting, value) {
            return said;
        }
        if let Some(said) = crate::agents::button_said(setting.id, value, self.lang) {
            return said.to_owned();
        }
        let said = setting.say(value, self.lang);
        match (setting.id, value) {
            ("locale.language", Value::Word("system")) => {
                format!("{said} {}", system_language(self.lang))
            }
            _ => said,
        }
    }

    /// The accent's row says the chosen colour's name first.
    pub(super) fn accent_line(&self, slot: Slot) -> String {
        let (fr, en) = crate::appearance::accent_of(self.value(slot).as_ref()).names();
        let name = if self.lang.fr() { fr } else { en };
        let rest = w(words::SWITCHES_SELECTION_THE_KEYBOARD_S_RING).get(self.lang);
        format!("{name} · {rest}")
    }
}

/// Text in the accent.
const ACCENT_TEXT_INK: Ink = Ink::AccentText;

/// "(français)": the session's language, as the window would speak it
/// without the file's choice.
fn system_language(lang: Lang) -> String {
    // A language the catalogue has no name for in `lang`: its own name.
    let name = match Lang::of_messages() {
        Lang::French => words::FRENCH_NAME.get(lang),
        Lang::English => words::ENGLISH_NAME.get(lang),
        #[allow(unreachable_patterns, reason = "a language added by its file alone")]
        other => other.name(),
    };
    words::in_parentheses(lang, name)
}

/// The cursor at `pixels`: a 72 × 72 tile with the arrow drawn at that
/// size (at most the tile's).
fn cursor_preview<'a>(pixels: i64) -> El<'a> {
    let tile = container(
        canvas(Arrow {
            size: (pixels as f32).clamp(8.0, 64.0),
        })
        .width(72)
        .height(72),
    )
    .width(72)
    .height(72)
    .style(|_: &Theme| container::Style {
        background: Some(Background::Color(CARD.get())),
        border: border(9.0, 1.0, CARD_RIM.get()),
        ..container::Style::default()
    });
    container(tile).id("cursor-preview".to_owned()).into()
}

struct Arrow {
    size: f32,
}

impl<M> canvas::Program<M> for Arrow {
    type State = ();

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        // The usual arrow, its tip at the top left, in a box `size` high.
        let s = self.size / 24.0;
        let at = Point::new(
            (bounds.width - 16.0 * s) / 2.0,
            (bounds.height - 22.0 * s) / 2.0,
        );
        let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
        let arrow = canvas::Path::new(|b| {
            b.move_to(p(0.0, 0.0));
            b.line_to(p(0.0, 18.0));
            b.line_to(p(4.5, 14.0));
            b.line_to(p(7.5, 21.0));
            b.line_to(p(10.5, 19.8));
            b.line_to(p(7.5, 13.0));
            b.line_to(p(13.5, 13.0));
            b.close();
        });
        frame.fill(&arrow, Color::BLACK);
        frame.stroke(
            &arrow,
            canvas::Stroke::default()
                .with_color(Color::WHITE)
                .with_width((1.4 * s).max(1.0)),
        );
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_systems_language_is_named_in_the_windows() {
        assert!(system_language(Lang::French).starts_with('('));
    }
}
