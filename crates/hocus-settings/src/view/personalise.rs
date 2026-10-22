//! The Personalise pages after Appearance (#67): Wallpaper, Column and
//! dock, Pandas, Accessibility, and the clock of Language and region. No
//! board draws them yet: they keep board R2's template, captioned sections
//! of choices, cards and switch cards.

use std::collections::HashMap;

use hocus_core::config::{ClockFormat, Wallpaper, WallpaperSpec, parse_hex};
use hocus_shell::panda::{Character, Panda};

use super::*;

/// A text field of the Wallpaper page: what is typed is written with
/// Enter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Entry {
    Image,
    Folder,
    Colour,
}

impl Entry {
    fn key(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Folder => "folder",
            Self::Colour => "colour",
        }
    }
}

/// The Wallpaper page between builds.
#[derive(Debug, Default)]
pub(super) struct WallpaperPage {
    /// The screen whose wallpaper shows; every screen's with `None`.
    screen: Option<String>,
    /// Its fields, as built last.
    pub(super) fields: Vec<(Entry, NodeId)>,
    /// What was typed in them, until the file has it.
    drafts: HashMap<Entry, String>,
    /// The colour typed last was not one.
    bad_colour: bool,
}

impl WallpaperPage {
    pub(super) fn entry_of(&self, id: Option<NodeId>) -> Option<Entry> {
        let id = id?;
        self.fields.iter().find(|(_, n)| *n == id).map(|(e, _)| *e)
    }

    pub(super) fn node(&self, entry: Entry) -> Option<NodeId> {
        self.fields
            .iter()
            .find(|(e, _)| *e == entry)
            .map(|(_, n)| *n)
    }

    pub(super) fn type_into(&mut self, entry: Entry, text: String) {
        self.drafts.insert(entry, text);
    }

    pub(super) fn forget(&mut self, entry: Entry) {
        self.drafts.remove(&entry);
        if entry == Entry::Colour {
            self.bad_colour = false;
        }
    }

    pub(super) fn show(&mut self, screen: Option<String>) {
        self.screen = screen;
        self.leave();
    }

    /// Off the page, or the file changed: the fields show the file again.
    pub(super) fn leave(&mut self) {
        self.drafts.clear();
        self.bad_colour = false;
    }
}

/// Every screen's wallpaper, without one screen's over it.
fn general(w: &Wallpaper) -> WallpaperSpec {
    WallpaperSpec {
        kind: w.kind.clone(),
        image: w.image.clone(),
        folder: w.folder.clone(),
        mode: w.mode.clone(),
        colour: w.colour.clone(),
    }
}

/// `[dock] pinned` as `pinned`.
fn pinned_change(pinned: &[String]) -> Change {
    let list: toml_edit::Array = pinned.iter().map(String::as_str).collect();
    Change::new("dock", "pinned", list)
}

/// A wallpaper's miniature: what it shows behind a window and the column,
/// in the theme in effect.
struct Backdrop {
    kind: &'static str,
    /// The wallpaper's own colour, for a plain one.
    colour: Option<Color>,
}

impl Draw for Backdrop {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _now: Duration) {
        let c = &theme.colors;
        canvas.push_clip(rect, 12.0);
        // A spread that looks scattered and is the same every time.
        let spot = |i: u32, n: u32| {
            let mut x = i.wrapping_mul(0x9e37_79b1) ^ n.wrapping_mul(0x85eb_ca77);
            x ^= x >> 15;
            x = x.wrapping_mul(0x2c1b_3c6d);
            x ^= x >> 12;
            (x % 1_000) as f32 / 1_000.0
        };
        match self.kind {
            "image" => {
                canvas.fill_rect(rect, 0.0, c.accent.with_alpha(70).over(c.background));
                let sun = rect.h * 0.22;
                canvas.fill_rect(
                    Rect::new(rect.x + rect.w * 0.18, rect.y + rect.h * 0.18, sun, sun),
                    sun / 2.0,
                    c.on_accent.with_alpha(200).over(c.accent),
                );
                for (x, w, top, ink) in [(-0.2, 0.9, 0.55, 150), (0.35, 0.95, 0.68, 230)] {
                    let hill = Rect::new(
                        rect.x + rect.w * x,
                        rect.y + rect.h * top,
                        rect.w * w,
                        rect.h,
                    );
                    canvas.fill_rect(
                        hill,
                        rect.h * 0.45,
                        c.surface.with_alpha(ink).over(c.accent),
                    );
                }
            }
            "folder" => {
                canvas.fill_rect(rect, 0.0, c.background);
                for i in 0..3 {
                    let d = i as f32 * 9.0;
                    let card = Rect::new(
                        rect.x + 14.0 + d,
                        rect.y + 12.0 + d,
                        rect.w * 0.5,
                        rect.h * 0.48,
                    );
                    canvas.fill_rect(card, 6.0, c.surface_pressed.over(c.surface));
                    canvas.fill_rect(
                        Rect::new(card.x, card.y + card.h * 0.6, card.w, card.h * 0.4),
                        6.0,
                        c.accent.with_alpha(60 + 50 * i).over(c.surface),
                    );
                }
            }
            "colour" => {
                canvas.fill_rect(rect, 0.0, self.colour.unwrap_or(c.background));
            }
            "rain" => {
                canvas.fill_rect(rect, 0.0, Color::rgb(0x02, 0x04, 0x0a));
                let green = Color::rgb(0x00, 0xf0, 0x60);
                for i in 0..14 {
                    let x = rect.x + 4.0 + i as f32 * (rect.w - 8.0) / 14.0;
                    let top = rect.y + spot(i, 1) * rect.h * 0.6 - 10.0;
                    let len = 18.0 + spot(i, 2) * rect.h * 0.5;
                    canvas.fill_rect(Rect::new(x, top, 2.0, len), 1.0, green.with_alpha(70));
                    canvas.fill_rect(Rect::new(x, top + len - 3.0, 2.0, 3.0), 1.0, green);
                }
            }
            "stars" => {
                canvas.fill_rect(rect, 0.0, Color::rgb(0x07, 0x0a, 0x18));
                for i in 0..36 {
                    let d = 1.0 + spot(i, 3) * 2.0;
                    let star = Rect::new(
                        rect.x + spot(i, 4) * rect.w,
                        rect.y + spot(i, 5) * rect.h,
                        d,
                        d,
                    );
                    let alpha = 90 + (spot(i, 6) * 165.0) as u8;
                    canvas.fill_rect(
                        star,
                        d / 2.0,
                        Color::rgb(0xf2, 0xf1, 0xec).with_alpha(alpha),
                    );
                }
            }
            // The glow: the theme's soft light, high in a corner.
            _ => {
                canvas.fill_rect(rect, 0.0, c.background);
                let (cx, cy) = (rect.x + rect.w * 0.7, rect.y + rect.h * 0.15);
                // Many faint discs, one in another: a light that fades out.
                for step in 0..10 {
                    let d = rect.w * (1.1 - step as f32 * 0.09);
                    let alpha = 10;
                    canvas.fill_rect(
                        Rect::new(cx - d / 2.0, cy - d / 2.0, d, d),
                        d / 2.0,
                        c.accent.with_alpha(alpha),
                    );
                }
            }
        }
        // A window and the column, as on the theme cards.
        let window = Rect::new(
            rect.x + 12.0,
            rect.y + rect.h * 0.3,
            rect.w * 0.42,
            rect.h * 0.46,
        );
        canvas.fill_rect(window, 6.0, c.surface.with_alpha(230));
        canvas.fill_rect(
            Rect::new(rect.x + rect.w - 26.0, rect.y + 7.0, 19.0, rect.h - 14.0),
            6.0,
            c.surface.with_alpha(230),
        );
        canvas.pop();
    }

    fn size(&self) -> Size {
        Size::new(0.0, 80.0)
    }
}

impl SettingsView {
    /// A card with a name, a line under it and a switch: `change` when it
    /// is clicked, greyed without one.
    pub(super) fn switch_card(
        &mut self,
        theme: &Theme,
        parent: NodeId,
        (name, sub): (words::Words, words::Words),
        on: bool,
        change: Option<Change>,
        setting: Option<&Setting>,
    ) -> NodeId {
        let lang = self.lang;
        let look = Look {
            background: Some(Paint::SurfaceHover),
            radius: 16.0,
            ..Look::default()
        };
        let card = self.add(
            parent,
            Style {
                height: Length::Px(68.0),
                padding: Insets::symmetric(10.0, 16.0),
                ..fill_width(row(14.0))
            },
            Kind::Box(look),
        );
        let works = change.is_some();
        let ink = if works { Paint::Text } else { Paint::Muted };
        if let Some(change) = change {
            self.tree.set_clickable(card, name.get(lang));
            self.tree.set_selected_box(card, on);
            self.targets.insert(card, Target::Set(change));
        }
        let words_box = self.add(card, fill_width(column(1.0)), Kind::Box(Look::default()));
        self.line(
            words_box,
            name.get(lang),
            theme.text.title.with_weight(700).with_color(ink),
        );
        self.line(
            words_box,
            sub.get(lang),
            theme.text.caption.with_weight(500).with_color(Paint::Muted),
        );
        self.add(
            card,
            Style {
                width: Length::Px(40.0),
                height: Length::Px(24.0),
                ..Style::default()
            },
            Kind::Custom {
                draw: Box::new(Switch(on)),
                label: None,
            },
        );
        if let Some(setting) = setting {
            self.highlighted(card, setting, look);
        }
        card
    }

    /// A muted line under a control.
    fn note(&mut self, theme: &Theme, parent: NodeId, words: words::Words) {
        let paint = Paint::Muted;
        self.note_painted(theme, parent, words, paint);
    }

    fn note_painted(&mut self, theme: &Theme, parent: NodeId, words: words::Words, paint: Paint) {
        let text = words.get(self.lang);
        self.add(
            parent,
            fill_width(Style::default()),
            Kind::text(text, theme.text.label.with_weight(500).with_color(paint)),
        );
    }

    /// A field of the Wallpaper page, showing what was typed or the file's
    /// value.
    fn entry_field(
        &mut self,
        theme: &Theme,
        parent: NodeId,
        entry: Entry,
        (name, hint): (words::Words, &str),
        value: Option<&str>,
    ) {
        let lang = self.lang;
        let boxed = self.add(
            parent,
            Style {
                height: Length::Px(44.0),
                padding: Insets::symmetric(0.0, 14.0),
                align: Align::Center,
                ..fill_width(row(0.0))
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                border: Some(Paint::Border),
                radius: 12.0,
                ..Look::default()
            }),
        );
        let field = self.add(
            boxed,
            Style {
                width: Length::Fill,
                height: Length::Px(28.0),
                ..Style::default()
            },
            Kind::TextField(Box::new(TextField::new(
                name.get(lang),
                hint,
                theme.text.body,
            ))),
        );
        let shown = self
            .wallpaper
            .drafts
            .get(&entry)
            .cloned()
            .unwrap_or_else(|| value.unwrap_or_default().to_owned());
        self.tree.set_field_value(field, &shown);
        self.wallpaper.fields.push((entry, field));
    }

    /// What was typed in a field written to the file: a path, or a colour
    /// once it reads as one; empty, the key goes.
    pub(super) fn commit(&mut self, entry: Entry) -> Vec<Outcome> {
        let typed = self
            .wallpaper
            .drafts
            .get(&entry)
            .cloned()
            .or_else(|| {
                let node = self.wallpaper.node(entry)?;
                self.tree.field_value(node).map(str::to_owned)
            })
            .unwrap_or_default();
        let typed = typed.trim();
        self.dirty = true;
        let value = match entry {
            _ if typed.is_empty() => None,
            Entry::Colour => {
                let hex = format!("#{}", typed.trim_start_matches('#').to_lowercase());
                if parse_hex(&hex).is_none() {
                    self.wallpaper.bad_colour = true;
                    return Vec::new();
                }
                Some(hex)
            }
            Entry::Image | Entry::Folder => Some(typed.to_owned()),
        };
        self.wallpaper.bad_colour = false;
        // The typed text stays until the file has it.
        if let Some(value) = &value {
            self.wallpaper.drafts.insert(entry, value.clone());
        }
        self.tree.set_focus(None);
        let screen = self.wallpaper.screen.as_deref();
        let change = Change::wallpaper(screen, entry.key(), value.map(Into::into));
        vec![Outcome::Change(Box::new(change))]
    }

    /// Wallpaper: every screen's or one screen's, its style, and what the
    /// style needs (a picture, a folder, a colour, how it covers).
    pub(super) fn wallpaper_page(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let outputs: Vec<String> = self.ctx.outputs.iter().map(|o| o.name.clone()).collect();
        if self
            .wallpaper
            .screen
            .as_ref()
            .is_some_and(|s| !outputs.contains(s))
        {
            self.wallpaper.show(None);
        }
        let screen = self.wallpaper.screen.clone();
        let w = self.ctx.config.wallpaper.clone();
        let spec = match &screen {
            Some(name) => w.for_output(name),
            None => general(&w),
        };
        let set = |key: &'static str, value: Option<&str>| {
            Target::Set(Change::wallpaper(
                screen.as_deref(),
                key,
                value.map(Into::into),
            ))
        };

        // Which screen, when there are several.
        if outputs.len() > 1 {
            let section = self.section(theme, body, words::SCREEN);
            let line = self.add(section, row(8.0), Kind::Box(Look::default()));
            self.choice(
                theme,
                line,
                words::EVERY_SCREEN.get(lang),
                screen.is_none(),
                Target::WallpaperScreen(None),
            );
            for name in &outputs {
                let chosen = screen.as_ref() == Some(name);
                self.choice(
                    theme,
                    line,
                    name,
                    chosen,
                    Target::WallpaperScreen(Some(name.clone())),
                );
            }
            if let Some(name) = &screen {
                let own = w.screens.iter().any(|s| &s.output == name);
                let line = self.add(section, fill_width(row(12.0)), Kind::Box(Look::default()));
                let said = if own {
                    words::OWN_WALLPAPER
                } else {
                    words::LIKE_EVERY_SCREEN
                };
                let text = self.add(
                    line,
                    fill_width(Style::default()),
                    Kind::Box(Look::default()),
                );
                self.note(theme, text, said);
                if own {
                    let b = self.add(
                        line,
                        Style {
                            height: Length::Px(40.0),
                            padding: Insets::symmetric(0.0, 16.0),
                            ..Style::default()
                        },
                        Kind::Button(Button::new(
                            words::LIKE_THE_OTHERS.get(lang),
                            ButtonKind::Plain,
                            theme,
                        )),
                    );
                    self.targets
                        .insert(b, Target::Set(Change::forget_wallpaper(name)));
                }
            }
        }

        // The style, as cards with their miniatures.
        let section = self.section(theme, body, words::STYLE);
        let per_line = if self.tree.size().w >= 1180.0 { 6 } else { 3 };
        let colour = spec
            .colour
            .as_deref()
            .and_then(parse_hex)
            .map(|[r, g, b]| Color::rgb(r, g, b));
        for line in words::WALLPAPERS.chunks(per_line) {
            let grid = self.add(section, fill_width(row(12.0)), Kind::Box(Look::default()));
            for (id, name, hint) in line {
                let chosen = spec.kind == *id;
                let card = self.add(
                    grid,
                    Style {
                        padding: Insets {
                            top: 8.0,
                            right: 8.0,
                            bottom: 12.0,
                            left: 8.0,
                        },
                        ..fill_width(column(10.0))
                    },
                    Kind::Box(Look {
                        background: Some(Paint::SurfaceHover),
                        radius: 18.0,
                        border: chosen.then_some(Paint::Accent),
                        ..Look::default()
                    }),
                );
                self.tree.set_clickable(card, name.get(lang));
                self.tree.set_selected_box(card, chosen);
                self.targets.insert(card, set("kind", Some(id)));
                self.add(
                    card,
                    Style {
                        width: Length::Fill,
                        height: Length::Px(80.0),
                        ..Style::default()
                    },
                    Kind::Custom {
                        draw: Box::new(Backdrop { kind: id, colour }),
                        label: None,
                    },
                );
                let words_box = self.add(
                    card,
                    Style {
                        padding: Insets::symmetric(0.0, 4.0),
                        ..column(1.0)
                    },
                    Kind::Box(Look::default()),
                );
                self.line(words_box, name.get(lang), theme.text.title.with_weight(700));
                self.line(
                    words_box,
                    hint.get(lang),
                    theme.text.caption.with_weight(500).with_color(Paint::Muted),
                );
            }
        }
        self.note(theme, section, words::MOVES_NOTE);

        // What the style needs.
        let kind = spec.kind.as_str();
        let picture = match kind {
            "image" => Some((
                words::PICTURE,
                Entry::Image,
                words::PICTURE_FILE,
                words::PICTURE_HINT,
                spec.image.clone(),
                words::PATH_NOTE,
            )),
            "folder" => Some((
                words::FOLDER,
                Entry::Folder,
                words::FOLDER_FIELD,
                words::FOLDER_HINT,
                spec.folder.clone(),
                words::FOLDER_NOTE,
            )),
            _ => None,
        };
        if let Some((title, entry, name, hint, value, note)) = picture {
            let section = self.section(theme, body, title);
            self.entry_field(
                theme,
                section,
                entry,
                (name, hint.get(lang)),
                value.as_deref(),
            );
            self.note(theme, section, note);
            if entry == Entry::Folder {
                self.note(theme, section, words::PATH_NOTE);
            }
            let section = self.section(theme, body, words::FIT);
            let line = self.add(section, row(8.0), Kind::Box(Look::default()));
            for (id, name) in words::MODES {
                self.choice(
                    theme,
                    line,
                    name.get(lang),
                    spec.mode == id,
                    set("mode", Some(id)),
                );
            }
        }
        if matches!(kind, "image" | "folder" | "colour") {
            let section = self.section(theme, body, words::COLOUR);
            let swatches = self.add(section, row(12.0), Kind::Box(Look::default()));
            let current = spec.colour.as_deref().map(str::to_lowercase);
            for (hex, name) in words::COLOURS {
                let chosen = current.as_deref() == Some(hex);
                let swatch = self.add(
                    swatches,
                    Style {
                        width: Length::Px(48.0),
                        height: Length::Px(48.0),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        radius: 24.0,
                        ..Look::default()
                    }),
                );
                self.tree
                    .set_clickable(swatch, &words::colour_named(name, lang));
                self.tree.set_selected_box(swatch, chosen);
                self.targets.insert(swatch, set("colour", Some(hex)));
                let color =
                    parse_hex(hex).map_or(theme.colors.background, |[r, g, b]| Color::rgb(r, g, b));
                self.add(
                    swatch,
                    Style {
                        width: Length::Fill,
                        height: Length::Fill,
                        ..Style::default()
                    },
                    Kind::Custom {
                        draw: Box::new(Swatch { color, chosen }),
                        label: None,
                    },
                );
            }
            self.choice(
                theme,
                swatches,
                words::THE_THEMES.get(lang),
                spec.colour.is_none(),
                set("colour", None),
            );
            self.entry_field(
                theme,
                section,
                Entry::Colour,
                (words::COLOUR_CODE, "#0e0f12"),
                spec.colour.as_deref(),
            );
            if self.wallpaper.bad_colour {
                self.note_painted(theme, section, words::NOT_A_COLOUR, Paint::Waiting);
            } else {
                self.note(theme, section, words::COLOUR_NOTE);
            }
        }
    }

    /// Column and dock: the column's optional lines, the dock and what it
    /// keeps.
    pub(super) fn column_dock(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let config = self.ctx.config.clone();

        let section = self.section(theme, body, words::COLUMN);
        self.note(theme, section, words::COLUMN_NOTE);
        let wide = self.tree.size().w >= 1180.0;
        let switches = self.add(
            section,
            Style {
                direction: if wide {
                    Direction::Row
                } else {
                    Direction::Column
                },
                ..fill_width(row(12.0))
            },
            Kind::Box(Look::default()),
        );
        for (section_key, name, sub, on) in [
            (
                "agents",
                words::AGENTS,
                words::AGENTS_SUB,
                config.agents.enabled,
            ),
            (
                "phone",
                words::PHONE,
                words::PHONE_SUB,
                config.phone.enabled,
            ),
            (
                "updates",
                words::SYSTEM_UPDATES,
                words::SYSTEM_UPDATES_SUB,
                config.updates.enabled,
            ),
        ] {
            let change = Change::new(section_key, "enabled", !on);
            self.switch_card(theme, switches, (name, sub), on, Some(change), None);
        }

        let section = self.section(theme, body, words::DOCK);
        let dock = config.dock.clone();
        self.switch_card(
            theme,
            section,
            (words::DOCK, words::DOCK_SUB),
            dock.enabled,
            Some(Change::new("dock", "enabled", !dock.enabled)),
            Some(&Setting::Dock),
        );

        // What it keeps, in order: each can go, or come before the one
        // ahead of it.
        let section = self.section(theme, body, words::PINNED);
        let pinned = dock.pinned;
        if pinned.is_empty() {
            self.note(theme, section, words::NO_PINNED);
        }
        // So many to a line: the page does not wrap.
        let per_line = if self.tree.size().w >= 1180.0 { 4 } else { 3 };
        for (n, some) in pinned.chunks(per_line).enumerate() {
            let chips = self.add(section, fill_width(row(10.0)), Kind::Box(Look::default()));
            for (j, entry) in some.iter().enumerate() {
                let i = n * per_line + j;
                let name = words::app_name(entry);
                let chip = self.add(
                    chips,
                    Style {
                        height: Length::Px(44.0),
                        padding: Insets {
                            left: 14.0,
                            right: 6.0,
                            ..Insets::default()
                        },
                        ..row(2.0)
                    },
                    Kind::Box(Look {
                        background: Some(Paint::SurfaceHover),
                        radius: 12.0,
                        ..Look::default()
                    }),
                );
                self.line(chip, &name, theme.text.label.with_weight(600));
                if i > 0 {
                    let mut order = pinned.clone();
                    order.swap(i - 1, i);
                    let before =
                        self.small_button(theme, chip, LEFT, &words::layout_before(&name, lang));
                    self.targets
                        .insert(before, Target::Set(pinned_change(&order)));
                }
                let mut rest = pinned.clone();
                rest.remove(i);
                let remove = self.small_button(
                    theme,
                    chip,
                    icons::CLOSE,
                    &words::remove_layout(&name, lang),
                );
                self.targets
                    .insert(remove, Target::Set(pinned_change(&rest)));
            }
        }
        self.note(theme, section, words::PINNED_NOTE);
    }

    /// Pandas: shown or not, what their ear says, where they show.
    pub(super) fn pandas(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let on = self.ctx.config.pandas.enabled;
        let section = self.add(body, fill_width(column(12.0)), Kind::Box(Look::default()));
        self.switch_card(
            theme,
            section,
            (words::SHOW_PANDAS, words::SHOW_PANDAS_SUB),
            on,
            Some(Change::new("pandas", "enabled", !on)),
            Some(&Setting::Pandas),
        );
        self.note(theme, section, words::PANDAS_NOTE);

        let section = self.section(theme, body, words::EAR);
        let per_line = if self.tree.size().w >= 1180.0 { 5 } else { 3 };
        let mut line = section;
        for (i, (mood, meaning)) in words::MOODS.into_iter().enumerate() {
            if i % per_line == 0 {
                line = self.add(section, fill_width(row(12.0)), Kind::Box(Look::default()));
            }
            let card = self.add(
                line,
                Style {
                    padding: Insets {
                        top: 8.0,
                        right: 8.0,
                        bottom: 12.0,
                        left: 8.0,
                    },
                    align: Align::Center,
                    ..fill_width(column(6.0))
                },
                Kind::Box(Look {
                    background: Some(Paint::SurfaceHover),
                    radius: 18.0,
                    ..Look::default()
                }),
            );
            // The two characters in turn, as agents get them.
            let panda = Panda::themed(Character::nth(i), mood, theme);
            self.add(
                card,
                Style {
                    width: Length::Px(88.0),
                    height: Length::Px(88.0),
                    ..Style::default()
                },
                Kind::Custom {
                    draw: Box::new(panda),
                    label: Some(mood.label(lang).to_owned()),
                },
            );
            self.line(card, meaning.get(lang), theme.text.body.with_weight(600));
        }
        // The last line's cards as wide as the others.
        let left = (per_line - words::MOODS.len() % per_line) % per_line;
        for _ in 0..left {
            self.add(
                line,
                fill_width(Style::default()),
                Kind::Box(Look::default()),
            );
        }

        let section = self.section(theme, body, words::WHERE_PANDAS);
        for place in words::PANDAS_WHERE {
            self.add(
                section,
                fill_width(Style::default()),
                Kind::text(place.get(lang), theme.text.body),
            );
        }
    }

    /// Accessibility: text size and contrast, motion, the magnifier.
    pub(super) fn accessibility(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let a = self.ctx.config.appearance.clone();

        let section = self.section(theme, body, words::SEEING);
        let size_row = self.add(section, fill_width(row(12.0)), Kind::Box(Look::default()));
        self.add(
            size_row,
            Style {
                width: Length::Px(140.0),
                ..Style::default()
            },
            Kind::text_line(words::TEXT_SIZE.get(lang), theme.text.body),
        );
        let slider = self.add(
            size_row,
            Style {
                width: Length::Fill,
                height: Length::Px(32.0),
                ..Style::default()
            },
            Kind::Slider {
                value: (a.text_size.clamp(80, 150) - 80) as f32 / 70.0,
                step: 5.0 / 70.0,
                name: words::TEXT_SIZE.get(lang).to_owned(),
            },
        );
        self.text_size = Some((slider, a.text_size));
        self.add(
            size_row,
            Style {
                width: Length::Px(56.0),
                ..Style::default()
            },
            Kind::text_line(
                words::percent(a.text_size, lang),
                theme.text.body.with_color(Paint::Muted),
            ),
        );
        self.switch_card(
            theme,
            section,
            (words::HIGH_CONTRAST, words::HIGH_CONTRAST_SUB),
            a.high_contrast,
            Some(Change::new("appearance", "high_contrast", !a.high_contrast)),
            None,
        );

        let section = self.section(theme, body, words::MOTION);
        self.switch_card(
            theme,
            section,
            (words::REDUCED_MOTION, words::REDUCED_MOTION_SUB),
            a.reduced_motion,
            Some(Change::new(
                "appearance",
                "reduced_motion",
                !a.reduced_motion,
            )),
            None,
        );

        // The magnifier's keys, changed on the keyboard page.
        let section = self.section(theme, body, words::MAGNIFIER);
        let shortcuts = self.ctx.config.shortcuts();
        let line = self.add(section, fill_width(row(12.0)), Kind::Box(Look::default()));
        for (action, name) in [
            (hocus_core::Action::ZoomIn, words::ZOOM_IN),
            (hocus_core::Action::ZoomOut, words::ZOOM_OUT),
        ] {
            self.line(
                line,
                name.get(lang),
                theme.text.body.with_color(Paint::Muted),
            );
            let keys = self.add(line, row(6.0), Kind::Box(Look::default()));
            match shortcuts.bindings.iter().find(|(_, a)| **a == action) {
                Some((combo, _)) => {
                    for cap in hocus_shell::actions::caps(combo, lang) {
                        self.keycap(theme, keys, &cap);
                    }
                }
                None => {
                    self.line(keys, words::NO_KEYS.get(lang), theme.text.body);
                }
            }
        }
        self.add(
            line,
            fill_width(Style::default()),
            Kind::Box(Look::default()),
        );
        let change = self.add(
            line,
            Style {
                height: Length::Px(40.0),
                padding: Insets::symmetric(0.0, 16.0),
                ..Style::default()
            },
            Kind::Button(Button::new(
                words::CHANGE_KEYS.get(lang),
                ButtonKind::Plain,
                theme,
            )),
        );
        self.targets.insert(change, Target::Page(Page::Keyboard));
        self.note(theme, section, words::MAGNIFIER_NOTE);
        self.highlighted(section, &Setting::Magnifier, Look::default());
    }

    /// Language and region's clock: 24 or 12 hours, and its time zone.
    pub(super) fn clock(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        let clock = self.ctx.config.clock.clone();
        let section = self.section(theme, body, words::CLOCK);
        let line = self.add(section, row(8.0), Kind::Box(Look::default()));
        for (format, name, id) in [
            (ClockFormat::TwentyFour, words::HOURS_24, "24h"),
            (ClockFormat::Twelve, words::HOURS_12, "12h"),
        ] {
            let target = Target::Set(Change::new("clock", "format", id));
            self.choice(theme, line, name.get(lang), clock.format == format, target);
        }
        self.highlighted(section, &Setting::Clock, Look::default());

        // A list of cities, by region; the map comes later.
        let section = self.section(theme, body, words::TIME_ZONE);
        let zone = clock.time_zone.as_str();
        let line = self.add(section, row(8.0), Kind::Box(Look::default()));
        let system = Target::Set(Change {
            value: None,
            ..Change::new("clock", "time_zone", "")
        });
        let b = self.choice(
            theme,
            line,
            words::LIKE_THE_SYSTEM.get(lang),
            zone.is_empty(),
            system,
        );
        // Told apart from the interface language's.
        let name = labelled(words::TIME_ZONE, words::LIKE_THE_SYSTEM.get(lang), lang);
        self.tree.set_name(b, &name);
        self.zone_chip(theme, line, "UTC", "UTC", zone == "UTC");
        let offered = words::ZONES
            .iter()
            .flat_map(|(_, cities)| cities.iter())
            .any(|(id, _)| *id == zone);
        // One set in the file and not offered shows too, chosen.
        if !zone.is_empty() && zone != "UTC" && !offered {
            let name = words::zone_name(zone, lang);
            self.zone_chip(theme, line, zone, &name, true);
        }
        let per_line = if self.tree.size().w >= 1180.0 { 7 } else { 4 };
        for (region, cities) in words::ZONES {
            let group = self.add(section, fill_width(column(8.0)), Kind::Box(Look::default()));
            self.line(
                group,
                region.get(lang),
                theme.text.label.with_weight(600).with_color(Paint::Muted),
            );
            // So many to a line: the page does not wrap.
            for some in cities.chunks(per_line) {
                let chips = self.add(group, fill_width(row(6.0)), Kind::Box(Look::default()));
                for (id, city) in some {
                    self.zone_chip(theme, chips, id, city.get(lang), zone == *id);
                }
            }
        }
        self.note(theme, section, words::TIME_ZONE_NOTE);
        self.highlighted(section, &Setting::TimeZone, Look::default());
    }

    fn zone_chip(&mut self, theme: &Theme, parent: NodeId, id: &str, name: &str, chosen: bool) {
        let mut b = Button::new(name, ButtonKind::Chip, theme);
        b.selected = chosen;
        let chip = self.add(
            parent,
            Style {
                height: Length::Px(36.0),
                padding: Insets::symmetric(0.0, 12.0),
                ..Style::default()
            },
            Kind::Button(b),
        );
        let change = Change::new("clock", "time_zone", id);
        self.targets.insert(chip, Target::Set(change));
    }
}
