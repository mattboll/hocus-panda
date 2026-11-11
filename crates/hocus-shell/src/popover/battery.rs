//! The battery's popover (bar.md §9, the 3.7 mockup): "Batterie" and
//! whether the machine is on battery or plugged in; a large battery filled
//! to its level in its band's colour (a bolt on AC), the level, and what
//! it does ("3 h 40 restantes", "En charge · plein dans 1 h 10"…); "MODE
//! D'ÉNERGIE" and three tiles, the power profiles, the active one lit, one
//! the machine does not have greyed with a line saying so; the battery's
//! health, when known. Without a battery but with power profiles, the
//! modes alone, titled "Énergie".
//!
//! Built with the shell's widget kit ([`crate::ui`]), as the sound's:
//! what changes draws this surface alone. A tile asks power-profiles-daemon for its profile and shows it
//! chosen until the daemon says it (3 s at most: it may refuse).
//!
//! The keyboard: `Tab` and `Shift+Tab` go through the tiles the machine
//! has; the arrows choose the next one; `Enter` or `Space` chooses; `Esc`
//! closes.

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_a11y::Role;
use iced::{
    Border, Color, Element, Length, Padding, Point, Rectangle, Size,
    alignment::{Horizontal, Vertical},
    keyboard,
    widget::{
        Space, button,
        canvas::{Frame, Path},
        column, container, row,
        text::LineHeight,
    },
    window,
};

use super::{
    Pressed,
    kit::{self, ASIDE, HEADER, HEADER_TOP, ICON, LABEL, LABEL_PAD, LEFT, MANROPE_SEMIBOLD, RIGHT},
};
use crate::levels::{
    self,
    draw::{
        ACCENT, DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, Sign, TEXT, band_colour, battery_sign,
        polygon, stroke,
    },
    model::{Band, Battery, Profile, Snapshot, Words},
    power::Ask,
};
use crate::theme::Ink;
use crate::ui::{
    self, Around, control,
    parts::{self, drawn, gap},
    style::{self, said},
};

type El<'a> = Element<'a, Msg>;

// --- The mockup's sizes, at ×1 ------------------------------------------------

const GAP: f32 = 8.0;
/// The large battery, 42 × 24, 12 px before its level (26 px on 1.1
/// lines) over its line (13.5 px), 1 px apart; 2 px above, 4 px under.
const BIG: Size = Size::new(42.0, 24.0);
const BIG_GAP: f32 = 12.0;
const LEVEL: f32 = 26.0;
const LEVEL_LINE: f32 = 1.1;
const STATE: f32 = 13.5;
const BLOCK: f32 = 2.0 + LEVEL * LEVEL_LINE + 1.0 + STATE * LINE + 4.0;
/// The room its line has, beside the large battery; a line too long
/// breaks between its parts (" · "), the block growing.
const STATE_ROOM: f32 = RIGHT - LEFT - BIG.width - BIG_GAP;
/// The divider: 1 px, 2 px around it.
const RULE: f32 = 2.0;
/// The modes' tiles: three in a row, 6 px apart, 58 px high.
const TILE: f32 = 58.0;
const TILE_GAP: f32 = 6.0;
const TILE_RADIUS: f32 = 10.0;
const ICON_GAP: f32 = 4.0;
const MODE: f32 = 12.5;
/// The lines under them: 12.5 px on 1.4 lines; the health's on normal
/// lines, 2 px down.
const NOTE: f32 = 12.5;
const NOTE_LINE: f32 = 1.4;
const HEALTH_PAD: f32 = 2.0;

const CHOSEN: Ink = Ink::Accent(0.16);
const TILE_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.04));
const TILE_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
const TILE_EDGE: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
const MISSING: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.28));

/// A tile shows the profile asked this long at most, until it is said.
const ASKED_FOR: Duration = Duration::from_secs(3);

const MODES: [Profile; 3] = [Profile::PowerSaver, Profile::Balanced, Profile::Performance];

/// Where its parts are, for what it shows.
#[derive(Debug, Clone, PartialEq)]
struct Layout {
    /// The battery's block's top, and its line's lines.
    battery: Option<f32>,
    state: Vec<String>,
    /// The divider's and the label's tops, with a battery above them.
    label: Option<(f32, f32)>,
    /// The tiles' top.
    tiles: Option<f32>,
    /// The lines under them (no performance mode, performance held back),
    /// and the health's: each its top and its lines.
    notes: Vec<(f32, Vec<String>)>,
    health: Option<(f32, Vec<String>)>,
    end: f32,
}

impl Layout {
    fn of(levels: &Snapshot, words: Words) -> Self {
        let room = RIGHT - LEFT;
        let lines = |text: &str| kit::wrap(text, room, |t| kit::measure(t, NOTE, MANROPE));
        let mut end = HEADER_TOP + HEADER;
        let state = levels.battery.map_or_else(Vec::new, |b| {
            crate::levels::draw::lines(&words.battery_line(&b), STATE_ROOM, |t| {
                kit::measure(t, STATE, MANROPE)
            })
        });
        let battery = levels.battery.map(|_| {
            let top = end + GAP;
            end = top + BLOCK + (state.len().max(1) - 1) as f32 * STATE * LINE;
            top
        });
        let modes = levels.profiles.any();
        let label = (modes && battery.is_some()).then(|| {
            let rule = end + GAP + RULE;
            let label = rule + 1.0 + RULE + GAP;
            end = label + 2.0 * LABEL_PAD + LABEL * LINE;
            (rule, label)
        });
        let tiles = modes.then(|| {
            let top = end + GAP;
            end = top + TILE;
            top
        });
        let mut notes = Vec::new();
        if modes {
            let missing = (!levels.profiles.performance).then(|| words.no_performance());
            let degraded = levels
                .degraded
                .filter(|_| levels.profile == Some(Profile::Performance))
                .map(|why| words.degraded(why));
            for note in missing.into_iter().chain(degraded) {
                let top = end + GAP;
                let note = lines(note);
                end = top + note.len() as f32 * NOTE * NOTE_LINE;
                notes.push((top, note));
            }
        }
        let health = levels.battery.and_then(|b| b.health()).map(|percent| {
            let top = end + GAP + HEALTH_PAD;
            let health = kit::wrap(&words.health(percent), room, |t| {
                kit::measure(t, NOTE, MANROPE)
            });
            end = top + health.len() as f32 * NOTE * LINE;
            (top, health)
        });
        Self {
            battery,
            state,
            label,
            tiles,
            notes,
            health,
            end,
        }
    }

    fn height(&self) -> u32 {
        kit::height(self.end)
    }
}

/// The popover opens: nothing said yet, nothing asked.
pub fn opening() {
    ui(|ui| *ui = Ui::default());
}

/// Its height for what it shows now.
pub fn height() -> u32 {
    Layout::of(&levels::snapshot(), kit::words()).height()
}

// --- What it keeps while it is open ---------------------------------------------

/// What can hold the keyboard's ring, and be acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A mode's tile.
    Mode(Profile),
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("battery:{self:?}")
    }
}

#[derive(Debug, Default)]
struct Ui {
    focus: Option<Target>,
    /// The profile asked, and since when.
    asked: Option<(Profile, Instant)>,
    /// What was last said in the log.
    said: Option<Snapshot>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

impl Ui {
    /// The profile shown chosen: the one asked, else the daemon's.
    fn chosen(&self, levels: &Snapshot) -> Option<Profile> {
        self.asked.map(|(p, _)| p).or(levels.profile)
    }

    fn choose(&mut self, levels: &Snapshot, profile: Profile) {
        if !levels.profiles.has(profile) {
            return;
        }
        if self.chosen(levels) != Some(profile) {
            levels::ask_power(Ask::Profile(profile));
        }
        self.asked = Some((profile, Instant::now()));
    }
}

/// The tiles that can be chosen, in `Tab`'s order.
fn targets(levels: &Snapshot) -> Vec<Profile> {
    MODES
        .into_iter()
        .filter(|p| levels.profiles.has(*p))
        .collect()
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A tile pressed, or `Enter` on it.
    Act(Target),
    /// The ring comes to it.
    Focus(Target),
    /// An arrow on the tile holding the ring: the next, or the one before.
    Step(bool),
    /// A press anywhere on the popover.
    Pressed,
    /// The levels changed under the view.
    Stale,
}

/// The battery's popover on its surface.
pub fn view<'a>(id: window::Id, words: Words, glass: bool) -> Element<'a, crate::Message> {
    let levels = levels::snapshot();
    let subtitle = levels
        .battery
        .map(|b| words.plugged(b.charge.plugged()).to_owned());
    let (content, until) = ui(|ui| {
        let until = ui.asked.map(|(_, since)| since + ASKED_FOR);
        (content(words, &levels, ui), until)
    });
    // Asked and not said: the surface wakes when it stops showing.
    Around::new(id, content, handle, Msg::Stale)
        .stale_when(|| ui(|ui| expired(ui, Instant::now())))
        .wake_at(until)
        .keys(arrows)
        .press(Msg::Pressed)
        .subtitle(subtitle)
        .on_glass(glass)
}

/// Whether the profile asked showed long enough without the daemon
/// saying it.
fn expired(ui: &Ui, now: Instant) -> bool {
    ui.asked.is_some_and(|(_, since)| now >= since + ASKED_FOR)
}

/// An arrow on a tile holding the ring chooses the next one (a radio
/// group's way), stopping at the ends.
fn arrows(pressed: &Pressed) -> Option<Msg> {
    use keyboard::{Key, key::Named};
    let forward = match &pressed.key {
        Key::Named(Named::ArrowRight | Named::ArrowDown) => true,
        Key::Named(Named::ArrowLeft | Named::ArrowUp) => false,
        _ => return None,
    };
    ui(|ui| ui.focus.is_some()).then_some(Msg::Step(forward))
}

/// The level's and its line's colour: as the bar's (§4.2).
fn band(band: Band) -> (Color, Color) {
    match band {
        Band::Ok => (TEXT.get(), DIM.get()),
        _ => (band_colour(band), band_colour(band)),
    }
}

/// What it shows, said in the log as it changes.
fn say(words: Words, levels: &Snapshot, layout: &Layout, ui: &mut Ui) {
    if ui.said.as_ref() == Some(levels) {
        return;
    }
    tracing::debug!(
        "the battery's popover: title={} plugged={:?} line={:?} modes={:?} chosen={:?} \
         notes={:?} health={:?}",
        words.battery_title(levels.battery.is_some()),
        levels.battery.map(|b| words.plugged(b.charge.plugged())),
        levels.battery.map(|b| words.battery_line(&b)),
        levels.profiles,
        levels.profile,
        layout
            .notes
            .iter()
            .map(|(_, l)| l.join(" "))
            .collect::<Vec<_>>(),
        layout.health.as_ref().map(|(_, l)| l.join(" ")),
    );
    ui.said = Some(*levels);
}

fn content<'a>(words: Words, levels: &Snapshot, ui: &mut Ui) -> El<'a> {
    let layout = Layout::of(levels, words);
    say(words, levels, &layout, ui);
    let mut all: Vec<El<'a>> = vec![header(words, levels.battery)];
    if let Some(battery) = levels.battery.filter(|_| layout.battery.is_some()) {
        all.push(parts::block(
            GAP,
            Some(battery_block(words, &battery, &layout.state)),
        ));
    }
    if layout.label.is_some() {
        all.push(parts::block(GAP, Some(parts::rule_of(RULE))));
        all.push(parts::block(
            GAP,
            Some(parts::label_as(
                words.power_mode(),
                words.mode_label(),
                2.0 * LABEL_PAD + LABEL * LINE,
                None,
            )),
        ));
    }
    if layout.tiles.is_some() {
        all.push(parts::block(GAP, Some(tiles(words, levels, ui))));
    }
    for (_, lines) in &layout.notes {
        all.push(parts::block(GAP, Some(note(lines, NOTE_LINE))));
    }
    if let Some((_, lines)) = &layout.health {
        all.push(parts::block(GAP + HEALTH_PAD, Some(note(lines, LINE))));
    }
    parts::content(all)
}

/// "Batterie" ("Énergie" without one), and whether it is plugged in.
fn header<'a>(words: Words, battery: Option<Battery>) -> El<'a> {
    let aside = battery.map(|b| {
        said(
            words.plugged(b.charge.plugged()).to_owned(),
            ASIDE,
            MANROPE,
            FAINT.get(),
        )
        .into()
    });
    parts::header(words.battery_title(battery.is_some()), aside)
}

/// The large battery, its level and its line.
fn battery_block<'a>(words: Words, battery: &Battery, lines: &[String]) -> El<'a> {
    let (level, colour) = band(battery.band());
    let sign = Sign {
        percent: battery.percent,
        fill: band_colour(battery.band()),
        bolt: battery.bolt(),
        outline: 1.4,
    };
    let big = drawn(BIG.width, BIG.height, move |f, s| {
        battery_sign(f, Rectangle::new(Point::ORIGIN, s), sign, 1.0);
    });
    let mut texts = column![
        said(
            format!("{}\u{a0}%", battery.shown()),
            LEVEL,
            MANROPE_BOLD,
            level
        )
        .line_height(LineHeight::Relative(LEVEL_LINE)),
        Space::new().height(1.0),
    ];
    for line in lines {
        texts = texts.push(said(line.clone(), STATE, MANROPE, colour));
    }
    let block =
        container(row![big, gap(BIG_GAP), texts].align_y(Vertical::Center)).padding(Padding {
            top: 2.0,
            bottom: 4.0,
            ..Padding::ZERO
        });
    let line = lines.join(" ");
    control(
        parts::inset(block),
        "battery:Level",
        Role::ProgressBar,
        words.battery_title(true),
    )
    .node(|n| {
        n.value(format!("{}\u{a0}%", battery.shown()))
            .range(f64::from(battery.shown()), 0.0, 100.0, None)
            .describe(line)
    })
    .into()
}

/// The modes' tiles, the active one lit, one the machine has not greyed.
fn tiles<'a>(words: Words, levels: &Snapshot, ui: &Ui) -> El<'a> {
    let there = targets(levels);
    let chosen = ui.chosen(levels);
    let mut line = row![].spacing(TILE_GAP);
    for profile in MODES {
        let has = levels.profiles.has(profile);
        let lit = has && chosen == Some(profile);
        let ink = match (has, lit) {
            (false, _) => MISSING,
            (true, true) => TEXT,
            (true, false) => DIM,
        }
        .get();
        let face = column![
            drawn(ICON, ICON, move |f, _| mode_icon(
                f,
                Point::ORIGIN,
                profile,
                ink
            )),
            Space::new().height(ICON_GAP),
            said(words.mode(profile), MODE, MANROPE_SEMIBOLD, ink),
        ]
        .align_x(Horizontal::Center);
        let target = Target::Mode(profile);
        let tile = button(container(face).center(Length::Fill))
            .padding(0)
            .width(Length::Fill)
            .height(TILE)
            .style(move |_, status| {
                let (fill, edge) = match (has, lit) {
                    (false, _) => (TILE_FILL, TILE_EDGE),
                    (true, true) => (CHOSEN, ACCENT),
                    (true, false) if style::lit(status) => (TILE_HOVER, TILE_EDGE),
                    (true, false) => (TILE_FILL, TILE_EDGE),
                };
                button::Style {
                    background: Some(fill.get().into()),
                    text_color: ink,
                    border: Border {
                        color: edge.get(),
                        width: 1.0,
                        radius: TILE_RADIUS.into(),
                    },
                    ..button::Style::default()
                }
            })
            .on_press_maybe(has.then_some(Msg::Act(target)));
        let i = there.iter().position(|p| *p == profile);
        let mut tile =
            control(tile, target.id(), Role::RadioButton, words.mode(profile)).node(|n| match i {
                Some(i) => n.checked(lit).set(i + 1, there.len()),
                None => n.disabled(true),
            });
        if has {
            tile = tile
                .focus(ui.focus == Some(target), Msg::Focus(target), TILE_RADIUS)
                .activate(Msg::Act(target));
        }
        line = line.push(tile);
    }
    parts::inset(line)
}

/// Lines under the tiles: 12.5 px on lines `height` em apart, faint.
fn note<'a>(lines: &[String], height: f32) -> El<'a> {
    let mut said_lines = column![];
    for line in lines {
        said_lines = said_lines.push(
            said(line.clone(), NOTE, MANROPE, FAINT.get())
                .line_height(LineHeight::Relative(height)),
        );
    }
    ui::line(parts::inset(said_lines), lines.join(" ")).into()
}

// --- What a message does -------------------------------------------------------

fn handle(message: Msg) {
    let levels = levels::snapshot();
    match message {
        Msg::Stale => {}
        Msg::Pressed => ui(|ui| ui.focus = None),
        Msg::Focus(target) => ui(|ui| ui.focus = Some(target)),
        Msg::Act(Target::Mode(profile)) => ui(|ui| ui.choose(&levels, profile)),
        Msg::Step(forward) => ui(|ui| {
            let order = targets(&levels);
            let Some(i) = order
                .iter()
                .position(|p| ui.focus == Some(Target::Mode(*p)))
            else {
                return;
            };
            let next = if forward {
                (i + 1).min(order.len() - 1)
            } else {
                i.saturating_sub(1)
            };
            let profile = order[next];
            ui.focus = Some(Target::Mode(profile));
            ui.choose(&levels, profile);
        }),
    }
    // The profile asked stays shown until the daemon says it, or a while.
    ui(|ui| {
        let said = ui
            .asked
            .is_some_and(|(asked, _)| levels.profile == Some(asked));
        kit::awaited(&mut ui.asked, said, Instant::now(), ASKED_FOR);
    });
    super::refit(super::Item::Battery);
}

/// A mode's icon, 18 px, from the mockup's SVGs: a leaf, scales, a bolt.
fn mode_icon(f: &mut Frame, at: Point, profile: Profile, colour: Color) {
    let s = ICON / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, colour);
    match profile {
        Profile::PowerSaver => {
            // `M5 19c0-8 5-13 14-14 0 9-5 14-13 14` and `M5 19l7-7`.
            let leaf = Path::new(|b| {
                b.move_to(p(5.0, 19.0));
                b.bezier_curve_to(p(5.0, 11.0), p(10.0, 6.0), p(19.0, 5.0));
                b.bezier_curve_to(p(19.0, 14.0), p(14.0, 19.0), p(6.0, 19.0));
            });
            f.stroke(&leaf, line);
            f.stroke(&Path::line(p(5.0, 19.0), p(12.0, 12.0)), line);
        }
        Profile::Balanced => {
            // `M12 3v18M5 21h14M6 7h12`, and a pan under each end:
            // `M6 7l-3 6a3 3 0 0 0 6 0z`.
            for (from, to) in [((12.0, 3.0), (12.0, 21.0)), ((5.0, 21.0), (19.0, 21.0))] {
                f.stroke(&Path::line(p(from.0, from.1), p(to.0, to.1)), line);
            }
            f.stroke(&Path::line(p(6.0, 7.0), p(18.0, 7.0)), line);
            for x in [6.0, 18.0] {
                let pan = Path::new(|b| {
                    b.move_to(p(x, 7.0));
                    b.line_to(p(x - 3.0, 13.0));
                    b.arc(iced::widget::canvas::path::Arc {
                        center: p(x, 13.0),
                        radius: 3.0 * s,
                        start_angle: iced::Radians(std::f32::consts::PI),
                        end_angle: iced::Radians(0.0),
                    });
                    b.line_to(p(x, 7.0));
                });
                f.stroke(&pan, line);
            }
        }
        Profile::Performance => {
            // `M13 2 4 14h7l-1 8 9-12h-7z`.
            let bolt = polygon(&[
                p(13.0, 2.0),
                p(4.0, 14.0),
                p(11.0, 14.0),
                p(10.0, 22.0),
                p(19.0, 10.0),
                p(12.0, 10.0),
            ]);
            f.stroke(&bolt, line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levels::model::{Charge, Degraded, Profiles};

    fn levels(battery: Option<f32>, profiles: Profiles) -> Snapshot {
        Snapshot {
            battery: battery.map(|health| Battery {
                percent: 82.0,
                charge: Charge::Discharging,
                left: None,
                health: Some(health),
            }),
            profile: Some(Profile::Balanced),
            profiles,
            ..Snapshot::default()
        }
    }

    const ALL: Profiles = Profiles {
        power_saver: true,
        balanced: true,
        performance: true,
    };

    #[test]
    fn its_height_follows_what_it_shows() {
        let words = Words::new(crate::clock::time::Lang::French);
        // The header, the battery's block, the divider, the label, the
        // tiles, the health (one line wide enough, or two).
        let full = Layout::of(&levels(Some(91.0), ALL), words);
        assert_eq!(full.battery, Some(53.0));
        let tiles = full.tiles.expect("tiles");
        let (rule, label) = full.label.expect("a label");
        assert_eq!(rule, 53.0 + BLOCK + 8.0 + 2.0);
        assert!((tiles - (label + 4.0 + LABEL * LINE + 8.0)).abs() < 0.01);
        let (health, lines) = full.health.clone().expect("health");
        assert_eq!(health, tiles + TILE + 8.0 + 2.0);
        assert!(!lines.is_empty());
        // Health unknown: no line.
        let unknown = Layout::of(&levels(Some(0.0), ALL), words);
        assert_eq!(unknown.health, None);
        assert!(unknown.height() < full.height());
        // Critical: its line on two lines, the block taller.
        let mut low = levels(Some(91.0), ALL);
        if let Some(b) = low.battery.as_mut() {
            b.percent = 9.0;
            b.left = Some(std::time::Duration::from_secs(25 * 60));
        }
        let low = Layout::of(&low, words);
        assert_eq!(low.state, ["25 min restantes", "branche le chargeur"]);
        assert!((low.height() as f32 - full.height() as f32 - STATE * LINE).abs() <= 1.0);
        // No performance mode: a line under the tiles.
        let two = Profiles {
            performance: false,
            ..ALL
        };
        let missing = Layout::of(&levels(Some(0.0), two), words);
        assert_eq!(missing.notes.len(), 1);
        assert!(missing.height() > unknown.height());
        // Performance held back, while chosen.
        let mut held = levels(Some(0.0), ALL);
        held.degraded = Some(Degraded::Hot);
        assert!(Layout::of(&held, words).notes.is_empty());
        held.profile = Some(Profile::Performance);
        assert_eq!(Layout::of(&held, words).notes.len(), 1);
        // No battery: the modes alone, under the header, no label.
        let modes = Layout::of(&levels(None, ALL), words);
        assert_eq!((modes.battery, modes.label), (None, None));
        assert_eq!(modes.tiles, Some(HEADER_TOP + HEADER + GAP));
        assert_eq!(
            modes.height(),
            (HEADER_TOP + HEADER + GAP + TILE + 15.0) as u32
        );
        // No profiles: the battery alone.
        let alone = Layout::of(&levels(Some(0.0), Profiles::default()), words);
        assert_eq!((alone.label, alone.tiles), (None, None));
    }

    #[test]
    fn only_the_modes_there_can_be_chosen() {
        let two = Profiles {
            performance: false,
            ..ALL
        };
        let levels = levels(None, two);
        assert_eq!(targets(&levels), [Profile::PowerSaver, Profile::Balanced]);
        let mut ui = Ui::default();
        assert_eq!(ui.chosen(&levels), Some(Profile::Balanced));
        // The missing one is not asked.
        ui.choose(&levels, Profile::Performance);
        assert_eq!(ui.asked, None);
    }
}
