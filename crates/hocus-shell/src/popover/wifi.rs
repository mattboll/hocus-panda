//! The Wi-Fi's popover (bar.md §9, the 3.5 mockup): its title and its
//! switch; "Connecté par câble" when a cable is in; the network connected,
//! a button to leave it, and with a sign-in page waiting, a button to open
//! it; then the networks around, strongest first, one row each (its
//! signal, its name, "enregistré" when saved, "dans les Réglages" for those
//! the popover cannot join, a lock when protected), scrolling past six.
//!
//! A saved or open network joins with a click; its row turns "Connexion…"
//! until NetworkManager says it is done. A protected one unfolds a
//! password field under its row: `Enter` or "Se connecter" (from 8
//! characters) tries it, `Esc` or "Annuler" folds it; a wrong password
//! says so under the field, which keeps it. A saved network whose password
//! NetworkManager now refuses unfolds the field the same way.
//!
//! **The password** comes from the keys as the compositor's keymap types
//! them (the user's layout, AltGr, dead keys: xkbcommon's text, with the
//! keymap and the modifiers the compositor sends), is kept in a
//! [`Secret`], shown as dots unless the eye is pressed, and goes to
//! NetworkManager only. `Ctrl+V` or `Shift+Insert` pastes the clipboard's
//! text at its end ([`crate::paste`]), without its line breaks. It is
//! cleared when its row folds, when the connection succeeds, and when the
//! popover closes.
//!
//! **The order** of the networks holds while it is worked in (the pointer
//! over the list, a row unfolded or focused, a connection under way): rows
//! do not move under the pointer; newcomers join at the end. Otherwise it
//! follows their strength.
//!
//! Built with the shell's widget kit ([`crate::ui`]): its controls, the
//! keyboard's ring, screen readers' rows and the tests' dump come from one
//! tree; only its glyphs are canvases.

use std::{
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU32, Ordering},
    },
    time::Instant,
};

use hocus_a11y::Role;
use iced::{
    Color, Element, Length, Padding, Point, Radians, Rectangle, Size,
    advanced::widget::Id,
    alignment::Vertical,
    widget::{Space, button, canvas::Path, column, container, row, space, stack, text::Wrapping},
    window,
};

use super::{
    Event as Seen, Item, event,
    kit::{
        self, FILL, FILL_HOVER, HEADER, HEADER_TOP, ICON, ICON_GAP, LEFT, MANROPE_SEMIBOLD, NAME,
        NOTE, NOTE_HEIGHT, PEACH, RIGHT, ROW, ROW_GAP, ROW_OUT, ROW_PAD, ROW_RADIUS, ROW_TEXT,
        SCROLLBAR, SMALL, SMALL_PAD, SMALL_RADIUS, STATUS, SWITCH, Switch,
    },
};
use crate::{
    field::{
        self, Entry, Took,
        view::{Look, Style},
    },
    levels::{
        draw::{ACCENT, DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb, stroke},
        model::Words,
    },
    network::{
        self, Ask,
        draw::{ethernet, wifi as arcs},
        model::{Failure, Network, Progress, Security, Wifi},
        secret::{self, Secret},
    },
    theme::Ink,
    ui::{
        self, Around, Later, control,
        field::Field,
        parts::{self, Knob, drawn, gap, inset, note, spinner},
        style::{filled, hovered, lit, said},
    },
};

type El<'a> = Element<'a, Msg>;

// --- The mockup's sizes, at ×1 ------------------------------------------------

const GAP: f32 = 6.0;
const WIRED: f32 = 32.0;
const PORTAL: f32 = 34.0;
const PORTAL_TEXT: f32 = 13.5;
/// The divider, 1 px with 4 px around it.
const RULE: f32 = 4.0;
/// The label's line: its padding and the "Searching…" beside it.
const LABEL_ROW: f32 = 2.0 + SMALL * LINE + 2.0;
/// The rows end 2 px short of the list's right while it does not scroll.
const ROW_END: f32 = 2.0;
const LOCK: f32 = 14.0;
/// The list's height at most: six rows and a peek of the seventh.
const LIST: f32 = 236.0;
/// The password's form under its row.
const FORM_TOP: f32 = 2.0;
const FORM_INDENT: f32 = 34.0;
const FORM_GAP: f32 = 8.0;
const FORM_BOTTOM: f32 = 10.0;
const FIELD: f32 = 34.0;
const FIELD_TEXT: f32 = 14.0;
const FIELD_PAD: f32 = 10.0;
const FIELD_RIGHT: f32 = 36.0;
const EYE: f32 = 28.0;
const EYE_ICON: f32 = 16.0;
const ERROR: f32 = 12.5;
const BUTTON: f32 = 30.0;
const BUTTON_PAD: f32 = 12.0;
const BUTTON_TEXT: f32 = 13.0;
/// The Settings' link at the bottom (6.5): its divider and its row.
const LINK_BLOCK: f32 = RULE + 1.0 + RULE + 36.0;

const PORTAL_FILL: Ink = Ink::Dark(Color::from_rgba8(229, 121, 63, 0.18));
const PORTAL_HOVER: Ink = Ink::Dark(Color::from_rgba8(229, 121, 63, 0.26));
const ORANGE: Ink = Ink::Dark(rgb(0xF2945B));
const FIELD_BORDER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.16));
const INK: Ink = Ink::Dark(rgb(0x0B1020));

const LIST_ID: &str = "wifi:list";

// --- What the popover keeps while it is open -----------------------------------

/// What can hold the keyboard's ring, and be acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Switch,
    Disconnect,
    Portal,
    /// A network's row, by its place in the list.
    Row(usize),
    Field,
    Eye,
    Cancel,
    Join,
    /// "Réglages du Wi-Fi…" (6.6).
    Settings,
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("wifi:{self:?}")
    }
}

/// The password's field: the shell's text field (#771) as a secret,
/// masked unless the eye shows it, never copied out.
#[derive(Debug)]
struct Password(Entry);

impl Default for Password {
    fn default() -> Self {
        Self(Entry::new(field::Kind::Secret, secret::LONGEST))
    }
}

impl std::ops::Deref for Password {
    type Target = Entry;

    fn deref(&self) -> &Entry {
        &self.0
    }
}

impl std::ops::DerefMut for Password {
    fn deref_mut(&mut self) -> &mut Entry {
        &mut self.0
    }
}

impl Password {
    /// Long enough to try.
    fn complete(&self) -> bool {
        self.0.chars() >= secret::SHORTEST
    }

    /// A copy for NetworkManager, made on purpose.
    fn secret(&self) -> Secret {
        let mut secret = Secret::new();
        secret.push(self.0.text());
        secret
    }
}

/// What it keeps between views and between opening and closing: the
/// controller asks its height from it, and its rows' order holds.
#[derive(Debug, Default)]
struct Ui {
    /// The networks' SSIDs, in the order shown.
    order: Vec<Vec<u8>>,
    /// The row whose password field is unfolded.
    unfolded: Option<Vec<u8>>,
    password: Password,
    /// The password shown, not dots.
    shown: bool,
    /// Why the last try with the field failed.
    error: Option<Failure>,
    /// A row joined with a click whose connection failed.
    failed: Option<Vec<u8>>,
    /// The last attempt reacted to.
    seen: u64,
    focus: Option<Target>,
    /// The pointer is over the list.
    over_list: bool,
    /// The Wi-Fi's switch, until NetworkManager says it.
    switch: Switch,
    paste: Paste,
    /// Pastes asked, to tell a late one from the one awaited.
    pastes: u64,
}

/// A paste into the field: the clipboard's text, read off the loop, lands
/// only in the field that asked, still unfolded and holding the keyboard.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Paste {
    #[default]
    None,
    /// The text of paste `n` is on its way.
    Awaited(u64),
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

impl Ui {
    /// Whether the rows hold their order.
    fn holding(&self, network: &Network) -> bool {
        self.over_list
            || self.unfolded.is_some()
            || matches!(
                self.focus,
                Some(Target::Row(_) | Target::Field | Target::Eye | Target::Cancel | Target::Join)
            )
            || network.connecting_to().is_some()
    }

    /// The rows' order for `network`: as it is while held (gone ones out,
    /// new ones last), else by strength.
    fn relist(&mut self, network: &Network) {
        let others: Vec<&Wifi> = network.others().collect();
        if self.holding(network) {
            self.order.retain(|s| others.iter().any(|n| n.ssid == *s));
            for n in &others {
                if !self.order.contains(&n.ssid) {
                    self.order.push(n.ssid.clone());
                }
            }
        } else {
            self.order = others.iter().map(|n| n.ssid.clone()).collect();
        }
        if let Some(unfolded) = &self.unfolded
            && !self.order.contains(unfolded)
        {
            self.fold();
        }
        if !network.enabled {
            self.fold();
        }
    }

    /// The rows shown, in order.
    fn rows(&self, network: &Network) -> Vec<Wifi> {
        self.order
            .iter()
            .filter_map(|s| network.networks.iter().find(|n| n.ssid == *s).cloned())
            .collect()
    }

    /// The unfolded row's place.
    fn unfolded_row(&self, rows: &[Wifi]) -> Option<usize> {
        let ssid = self.unfolded.as_ref()?;
        rows.iter().position(|n| n.ssid == *ssid)
    }

    fn fold(&mut self) {
        self.unfolded = None;
        self.password.clear();
        self.shown = false;
        self.error = None;
        // A paste on its way lands nowhere.
        self.paste = Paste::None;
        if matches!(
            self.focus,
            Some(Target::Field | Target::Eye | Target::Cancel | Target::Join)
        ) {
            self.focus = None;
        }
    }

    /// Unfolds `ssid`'s field, empty, the keyboard in it.
    fn unfold(&mut self, ssid: Vec<u8>) {
        self.fold();
        self.unfolded = Some(ssid);
        self.focus = Some(Target::Field);
    }

    /// What the network's last attempt says, once.
    fn react(&mut self, network: &Network) -> bool {
        let Some(attempt) = &network.attempt else {
            return false;
        };
        if attempt.id <= self.seen || attempt.state == Progress::Connecting {
            return false;
        }
        self.seen = attempt.id;
        let ssid = attempt.ssid.clone();
        let mine = self.unfolded.as_ref() == Some(&ssid);
        match attempt.state {
            Progress::Connected => {
                if mine {
                    self.fold();
                }
                self.failed = None;
                false
            }
            Progress::Failed(Failure::Secret) => {
                if !mine {
                    self.unfold(ssid);
                }
                self.error = Some(Failure::Secret);
                self.focus = Some(Target::Field);
                true
            }
            Progress::Failed(Failure::Other) => {
                if mine {
                    self.error = Some(Failure::Other);
                } else {
                    self.failed = Some(ssid);
                }
                false
            }
            Progress::Connecting => false,
        }
    }

    /// The switch as shown: as asked, until NetworkManager says it.
    fn enabled(&self, network: &Network, now: Instant) -> bool {
        self.switch.shown(network.enabled, now)
    }
}

/// The popover opens: the networks in their order now, nothing unfolded;
/// a scan asked, whose results come while it is open.
pub fn opening() {
    let network = network::snapshot();
    ui(|ui| {
        *ui = Ui {
            seen: network.attempt.as_ref().map_or(0, |a| a.id),
            ..Ui::default()
        };
        ui.relist(&network);
    });
    if network.enabled && network.hardware {
        network::ask(Ask::Scan);
    }
}

/// The popover closes: what was typed goes.
pub fn closing() {
    ui(|ui| {
        ui.fold();
        ui.focus = None;
    });
}

/// Asks for the clipboard's text, for the field holding the keyboard: it
/// lands at the field's caret as if typed (whole if it fits, or not at all),
/// and clears the error; the popover then draws again. One at a time.
fn paste() {
    let n = ui(|ui| {
        if ui.paste != Paste::None {
            return None;
        }
        ui.pastes += 1;
        ui.paste = Paste::Awaited(ui.pastes);
        Some(ui.pastes)
    });
    let Some(n) = n else {
        return;
    };
    let asked = crate::paste::request(crate::paste::Clean::Dropped, move |text| {
        let landed = ui(|ui| {
            if ui.paste != Paste::Awaited(n) {
                return false;
            }
            ui.paste = Paste::None;
            let Some(text) = text.filter(|t| !t.is_empty()) else {
                return false;
            };
            if ui.unfolded.is_none() || ui.focus != Some(Target::Field) {
                return false;
            }
            let fits = ui.password.paste(text);
            if fits {
                ui.error = None;
                tracing::debug!("the clipboard's text pasted into the field");
            } else {
                tracing::debug!("the clipboard's text does not fit in the field");
            }
            fits
        });
        if landed {
            super::changed(Item::Wifi);
        }
    });
    if !asked {
        ui(|ui| {
            if ui.paste == Paste::Awaited(n) {
                ui.paste = Paste::None;
            }
        });
    }
}

// --- Its height ----------------------------------------------------------------------

/// The popover's height for what it shows now, which the controller asks
/// before its view is built: as its widgets lay it out (their natural
/// height, logged beside it at debug).
pub fn height() -> u32 {
    let network = network::snapshot();
    ui(|ui| {
        ui.relist(&network);
        Shape::of(&network, ui, &ui.rows(&network), Instant::now()).height()
    })
}

/// The height of the unfolded part under a row, with `error` lines.
fn form_height(error: usize) -> f32 {
    let error = if error > 0 {
        error as f32 * ERROR * LINE + FORM_GAP
    } else {
        0.0
    };
    FORM_TOP + FIELD + FORM_GAP + error + BUTTON + FORM_BOTTOM
}

/// The room the error has under the field: the field's width in a list
/// that scrolls.
const ERROR_ROOM: f32 = RIGHT - 2.0 - (LEFT - ROW_OUT) - FORM_INDENT - ROW_PAD;

/// What a failure says under the field, in lines.
fn error_lines(failure: Failure) -> Vec<String> {
    let words = kit::words();
    let said = match failure {
        Failure::Secret => words.wrong_password(),
        Failure::Other => words.could_not_connect(),
    };
    kit::wrapped(said, ERROR, ERROR_ROOM)
}

/// The network connected's state, under its name, in lines: "Connecté",
/// or "Page de connexion à ouvrir", which wraps beside Disconnect as the
/// mockup's does.
fn status_lines(portal: bool) -> Vec<String> {
    let words = kit::words();
    let said = if portal {
        words.sign_in_waits()
    } else {
        words.connected()
    };
    kit::wrapped(said, STATUS, kit::block_room(disconnect_width()))
}

fn disconnect_width() -> f32 {
    kit::measure(kit::words().disconnect(), STATUS, MANROPE) + 2.0 * SMALL_PAD
}

/// Where the popover ends and how tall its list is, as its blocks stack.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shape {
    end: f32,
    /// The list's view and its content's height.
    list: Option<(f32, f32)>,
}

impl Shape {
    fn of(network: &Network, ui: &Ui, rows: &[Wifi], now: Instant) -> Self {
        let mut y = HEADER_TOP + HEADER;
        let mut next = |height: f32| y += GAP + height;
        let enabled = ui.enabled(network, now) && network.hardware;
        if network.wired {
            next(WIRED);
        }
        if !enabled {
            next(NOTE_HEIGHT);
        }
        let current = network.current.is_some() && enabled;
        if current {
            next(kit::block_height(
                status_lines(network.portal).len().max(1) as f32
            ));
        }
        if current && network.portal {
            next(PORTAL);
        }
        let mut list = None;
        if enabled {
            next(2.0 * RULE + 1.0);
            next(LABEL_ROW);
            if rows.is_empty() {
                next(NOTE_HEIGHT);
            } else {
                let form = ui.unfolded_row(rows).map_or(0.0, |_| {
                    form_height(ui.error.map_or(0, |e| error_lines(e).len()))
                });
                let content = rows.len() as f32 * (ROW + ROW_GAP) - ROW_GAP + form;
                let view = content.min(LIST);
                next(view);
                list = Some((view, content));
            }
        }
        Self {
            end: y + LINK_BLOCK,
            list,
        }
    }

    fn height(self) -> u32 {
        kit::height(self.end)
    }
}

// --- The view ------------------------------------------------------------------------

/// The Wi-Fi's popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    /// Its surface: it reacts only while open, not while it fades out.
    pub id: window::Id,
    pub words: Words,
    /// On the compositor's glass (else opaque).
    pub glass: bool,
    /// Its spinners turn and its switch slides.
    pub motion: bool,
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A control pressed, or `Enter` on it.
    Act(Target),
    /// The ring comes to it.
    Focus(Target),
    /// What the field did with a key or the pointer.
    Took(Took),
    /// `Esc` folds the row unfolded.
    Fold,
    /// The pointer came over the list, or left it.
    OverList(bool),
    /// A press anywhere on the popover.
    Pressed,
    /// The network, or what a paste brought, changed under the view.
    Stale,
}

/// What waits for the tree built after a message: the list scrolled.
static LATER: Later = Later::new();
/// The height the tree took when last laid out, px (bits of an `f32`).
static NATURAL: AtomicU32 = AtomicU32::new(0);

/// The popover on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let network = network::snapshot();
    let content = ui(|ui| {
        let rows = ui.rows(&network);
        content(panel, &network, ui, &rows, Instant::now())
    });
    let motion = panel.motion;
    Around::new(panel.id, content, move |m| handle(m, motion), Msg::Stale)
        .keys(|pressed| {
            use iced::keyboard::{Key, key::Named};
            let unfolded = ui(|ui| ui.unfolded.is_some());
            (pressed.key == Key::Named(Named::Escape) && unfolded).then_some(Msg::Fold)
        })
        .press(Msg::Pressed)
        .later(&LATER)
        .natural(|height| NATURAL.store(height.to_bits(), Ordering::Relaxed))
        .on_glass(panel.glass)
}

fn content<'a>(panel: &Panel, network: &Network, ui: &Ui, rows: &[Wifi], now: Instant) -> El<'a> {
    let words = panel.words;
    let on = ui.enabled(network, now);
    let enabled = on && network.hardware;
    let focus = ui.focus;
    // A widget keeps what it knows (how far the list is scrolled) by its
    // place in the tree, and the blocks above the list come and go: each
    // has its place, empty while it does not show.
    let mut all: Vec<El<'a>> = vec![header(panel, network, ui, now)];
    let block = |content: Option<El<'a>>| parts::block(GAP, content);
    all.push(block(network.wired.then(|| {
        let line = row![
            drawn(ICON, ICON, |f, s| ethernet(
                f,
                Point::ORIGIN,
                s.width,
                DIM.get()
            )),
            gap(ICON_GAP),
            said(words.by_cable(), NOTE, MANROPE, DIM.get()),
        ]
        .align_y(Vertical::Center);
        ui::line(
            container(line)
                .height(WIRED)
                .padding([0.0, ROW_OUT])
                .align_y(Vertical::Center),
            words.by_cable(),
        )
        .into()
    })));
    all.push(block((!enabled).then(|| {
        note(if network.airplane {
            words.airplane_is_on()
        } else if network.hardware {
            words.wifi_is_off()
        } else {
            words.no_adapter()
        })
    })));
    let current = network.current.as_ref().filter(|_| enabled);
    all.push(block(
        current.map(|current| connected(panel, current, network, focus)),
    ));
    all.push(block((current.is_some() && network.portal).then(|| {
        let can = network.check_uri.is_some();
        let open = button(
            container(said(
                words.open_sign_in(),
                PORTAL_TEXT,
                MANROPE_SEMIBOLD,
                PEACH.get(),
            ))
            .center(Length::Fill),
        )
        .padding(0)
        .width(Length::Fill)
        .height(PORTAL)
        .style(|_, status| {
            filled(
                if lit(status) {
                    PORTAL_HOVER
                } else {
                    PORTAL_FILL
                }
                .get(),
                ROW_RADIUS,
                PEACH.get(),
            )
        })
        .on_press_maybe(can.then_some(Msg::Act(Target::Portal)));
        let mut open = control(
            open,
            Target::Portal.id(),
            Role::Button,
            words.open_sign_in(),
        );
        if can {
            open = open
                .focus(
                    focus == Some(Target::Portal),
                    Msg::Focus(Target::Portal),
                    ROW_RADIUS,
                )
                .activate(Msg::Act(Target::Portal));
        }
        inset(ui::line(open, words.sign_in_waits()))
    })));
    all.push(block(enabled.then(parts::rule)));
    all.push(block(enabled.then(|| label(panel, network.scanning))));
    all.push(block(enabled.then(|| {
        if rows.is_empty() {
            note(if network.scanning {
                words.searching_networks()
            } else {
                words.no_network()
            })
        } else {
            list(panel, network, ui, rows)
        }
    })));
    let [rule, link] = parts::settings_link(
        words.wifi_settings(),
        &Target::Settings.id(),
        Msg::Act(Target::Settings),
        focus == Some(Target::Settings),
        Msg::Focus(Target::Settings),
    );
    all.push(rule);
    all.push(link);
    parts::content(all)
}

/// The title, and the switch.
fn header<'a>(panel: &Panel, network: &Network, ui: &Ui, now: Instant) -> El<'a> {
    let words = panel.words;
    let switch = network.hardware.then(|| {
        let knob = Knob {
            knob: |now, motion| {
                let network = network::snapshot();
                self::ui(|ui| {
                    let on = ui.enabled(&network, now);
                    ui.switch.knob_at(on, now, motion)
                })
            },
            sliding: |now| self::ui(|ui| ui.switch.sliding(now)),
            motion: panel.motion,
        };
        control(
            knob.view(Msg::Act(Target::Switch)),
            Target::Switch.id(),
            Role::ToggleButton,
            words.wifi(),
        )
        .node(|n| n.checked(ui.enabled(network, now)))
        .focus(
            ui.focus == Some(Target::Switch),
            Msg::Focus(Target::Switch),
            SWITCH.height / 2.0,
        )
        .activate(Msg::Act(Target::Switch))
        .into()
    });
    parts::header(words.wifi(), switch)
}

/// The network connected: its signal, its name over its state, Disconnect.
fn connected<'a>(
    panel: &Panel,
    current: &Wifi,
    network: &Network,
    focus: Option<Target>,
) -> El<'a> {
    let words = panel.words;
    let name = kit::elide(&current.name, kit::block_room(disconnect_width()), |s| {
        kit::measure(s, NAME, MANROPE_SEMIBOLD)
    });
    let (state, colour) = if network.portal {
        (words.sign_in_waits(), PEACH)
    } else {
        (words.connected(), ACCENT)
    };
    // The state wraps by itself beside the button.
    let state = said(state, STATUS, MANROPE, colour.get())
        .wrapping(Wrapping::Word)
        .width(Length::Fill);
    let texts = column![
        said(name, NAME, MANROPE_SEMIBOLD, TEXT.get()),
        Space::new().height(1.0),
        state
    ]
    .width(Length::Fill);
    let bars = current.bars();
    let leave = control(
        parts::small_button(words.disconnect(), Msg::Act(Target::Disconnect)),
        Target::Disconnect.id(),
        Role::Button,
        format!("{} {}", words.disconnect(), current.name),
    )
    .focus(
        focus == Some(Target::Disconnect),
        Msg::Focus(Target::Disconnect),
        SMALL_RADIUS,
    )
    .activate(Msg::Act(Target::Disconnect));
    let line = row![
        drawn(ICON, ICON, move |f, s| arcs(
            f,
            Point::ORIGIN,
            s.width,
            bars,
            ACCENT.get(),
            false,
            1.0
        )),
        gap(ICON_GAP),
        texts,
        gap(ICON_GAP),
        leave,
    ]
    .align_y(Vertical::Center);
    ui::line(
        parts::connected(line),
        format!("{}, {}", current.name, words.connected()),
    )
    .into()
}

/// The section's label, its letters spaced, and "Searching…" on its right.
fn label<'a>(panel: &Panel, scanning: bool) -> El<'a> {
    let words = panel.words;
    let aside = scanning.then(|| {
        let mut aside = row![].align_y(Vertical::Center);
        if panel.motion {
            aside = aside.push(spinner(FAINT.get())).push(gap(6.0));
        }
        aside
            .push(said(words.searching(), SMALL, MANROPE, FAINT.get()))
            .into()
    });
    parts::label(words.networks(), LABEL_ROW, aside)
}

/// The networks around, scrolling past six.
fn list<'a>(panel: &Panel, network: &Network, ui: &Ui, rows: &[Wifi]) -> El<'a> {
    let words = panel.words;
    let busy = network.connecting_to();
    let unfolded = ui.unfolded_row(rows);
    // Whether it scrolls, for the names' room alone: the rows' own width
    // is the scrollable's business.
    let scrolls = Shape::of(network, ui, rows, Instant::now())
        .list
        .is_some_and(|(view, content)| content > view + 0.5);
    let width = RIGHT - LEFT + 2.0 * ROW_OUT
        - if scrolls {
            SCROLLBAR + ROW_END
        } else {
            ROW_END
        };
    let fr = words.wifi_settings().starts_with("Réglages");
    let mut all = column![].spacing(ROW_GAP);
    for (i, network) in rows.iter().enumerate() {
        let open = unfolded == Some(i);
        let aside = row_aside(words, network, open, busy, ui.failed.as_deref());
        let locked = network.security.locked();
        let room = kit::name_room(
            width,
            if locked { LOCK + ICON_GAP } else { 0.0 },
            aside.map(|(text, _, spins)| {
                (kit::measure(text, SMALL, MANROPE), spins && panel.motion)
            }),
        );
        let name = kit::elide(&network.name, room, |s| kit::measure(s, ROW_TEXT, MANROPE));
        let bars = network.bars();
        let mut line = row![
            drawn(ICON, ICON, move |f, s| arcs(
                f,
                Point::ORIGIN,
                s.width,
                bars,
                TEXT.get(),
                false,
                1.0
            )),
            gap(ICON_GAP),
            said(name, ROW_TEXT, MANROPE, TEXT.get()),
            space::horizontal(),
        ]
        .align_y(Vertical::Center);
        if let Some((text, colour, spins)) = aside {
            if spins && panel.motion {
                line = line.push(spinner(DIM.get())).push(gap(6.0));
            }
            line = line.push(said(text, SMALL, MANROPE, colour));
        }
        if locked {
            line = line
                .push(gap(ICON_GAP))
                .push(drawn(LOCK, LOCK, |f, _| lock(f, Point::ORIGIN)));
        }
        let press = busy.is_none().then_some(Msg::Act(Target::Row(i)));
        // Unfolded, it is lit as its form is, not more under the pointer.
        let idle = busy.is_none();
        let pressed = parts::row_button(line, ROW, press).style(move |_, status| {
            if idle && !open && lit(status) {
                hovered(ui::style::ROW_HOVER.get(), ROW_RADIUS)
            } else {
                ui::style::clear()
            }
        });
        let mut row_control = control(
            pressed,
            Target::Row(i).id(),
            Role::Button,
            network.name.clone(),
        )
        .node(|n| n.describe(described(words, network, ui, busy, fr)))
        .rows(0);
        if busy.is_none() {
            row_control = row_control
                .focus(
                    ui.focus == Some(Target::Row(i)),
                    Msg::Focus(Target::Row(i)),
                    ROW_RADIUS,
                )
                .activate(Msg::Act(Target::Row(i)));
        }
        let block: El<'a> = if open {
            container(column![row_control, form(panel, busy.is_some(), ui, fr)])
                .style(|_| {
                    let lit = hovered(ui::style::ROW_HOVER.get(), ROW_RADIUS);
                    container::Style {
                        background: lit.background,
                        border: lit.border,
                        ..container::Style::default()
                    }
                })
                .into()
        } else {
            row_control.into()
        };
        all = all.push(container(block).id(Id::from(format!("wifi:row:{i}"))));
    }
    let rows = container(all).padding(Padding {
        right: ROW_END,
        ..Padding::ZERO
    });
    parts::list(rows.into(), LIST_ID, LIST, Msg::OverList)
}

/// What a row says to a screen reader beside its name.
fn described(words: Words, row: &Wifi, ui: &Ui, busy: Option<&[u8]>, fr: bool) -> String {
    let mut said = Vec::new();
    if busy == Some(row.ssid.as_slice()) {
        said.push(words.connecting().to_owned());
    }
    match row.security {
        Security::Open => {}
        Security::Psk { .. } | Security::Wep => said.push(
            if fr {
                "protégé par mot de passe"
            } else {
                "secured"
            }
            .to_owned(),
        ),
        Security::Enterprise => said.push(words.in_settings().to_owned()),
    }
    if row.saved {
        said.push(words.saved().to_owned());
    }
    let signal = match (row.bars(), fr) {
        (0 | 1, true) => "signal faible",
        (2, true) => "signal moyen",
        (3, true) => "bon signal",
        (_, true) => "signal fort",
        (0 | 1, false) => "weak signal",
        (2, false) => "fair signal",
        (3, false) => "good signal",
        (_, false) => "strong signal",
    };
    said.push(signal.to_owned());
    if ui.failed.as_deref() == Some(row.ssid.as_slice()) {
        said.push(words.could_not_connect().to_owned());
    }
    said.join(", ")
}

/// The password's form under its row: the field and its eye, what went
/// wrong, Cancel and Connect.
fn form<'a>(panel: &Panel, busy: bool, ui: &Ui, fr: bool) -> El<'a> {
    let words = panel.words;
    let error = ui.error;
    let focus = ui.focus;
    let typed = Field {
        with: |with| self::ui(|ui| with(&mut ui.password)),
        style: Style {
            reveal: ui.shown,
            ..Style::one(FIELD_TEXT, MANROPE)
        },
        placeholder: words.password(),
        look: Look::of(TEXT.get(), FAINT.get()),
        focused: focus == Some(Target::Field),
        left: FIELD_PAD,
        right: FIELD_RIGHT,
        on_focus: Msg::Focus(Target::Field),
        on_took: Msg::Took,
    }
    .view(FIELD);
    let typed = container(typed).style(move |_| container::Style {
        background: Some(FILL.get().into()),
        border: iced::Border {
            color: if error.is_some() {
                ORANGE
            } else {
                FIELD_BORDER
            }
            .get(),
            width: 1.0,
            radius: ROW_RADIUS.into(),
        },
        ..container::Style::default()
    });
    let mut typed = control(
        typed,
        Target::Field.id(),
        Role::PasswordField,
        words.password(),
    )
    .focus(
        focus == Some(Target::Field),
        Msg::Focus(Target::Field),
        ROW_RADIUS,
    )
    .activate(Msg::Act(Target::Join))
    .typing();
    if let Some(error) = error {
        typed = typed.node(|n| n.describe(error_lines(error).join(" ")));
    }
    let shown = ui.shown;
    let eye = button(drawn(EYE, EYE, move |f, s| {
        eye(f, Rectangle::new(Point::ORIGIN, s), shown);
    }))
    .padding(0)
    .width(EYE)
    .height(EYE)
    .style(|_, _| ui::style::clear())
    .on_press(Msg::Act(Target::Eye));
    let eye = control(
        eye,
        Target::Eye.id(),
        Role::ToggleButton,
        if fr {
            "Afficher le mot de passe"
        } else {
            "Show the password"
        },
    )
    .node(|n| n.checked(shown))
    .focus(focus == Some(Target::Eye), Msg::Focus(Target::Eye), 6.0)
    .activate(Msg::Act(Target::Eye));
    let field = stack![
        typed,
        container(eye)
            .align_right(Length::Fill)
            .center_y(Length::Fill)
            .padding(Padding {
                right: 3.0,
                ..Padding::ZERO
            }),
    ];
    let mut all = column![field, Space::new().height(FORM_GAP)];
    if let Some(error) = error {
        let lines = match error {
            Failure::Secret => words.wrong_password(),
            Failure::Other => words.could_not_connect(),
        };
        // It wraps by itself, in the room the canvas gave it.
        all = all
            .push(
                said(lines, ERROR, MANROPE, PEACH.get())
                    .wrapping(Wrapping::Word)
                    .width(ERROR_ROOM),
            )
            .push(Space::new().height(FORM_GAP));
    }
    let cancel = button(parts::centred(words.cancel(), BUTTON_TEXT, MANROPE))
        .padding([0.0, BUTTON_PAD])
        .height(BUTTON)
        .style(|_, status| {
            filled(
                if lit(status) { FILL_HOVER } else { FILL }.get(),
                ROW_RADIUS,
                if lit(status) { TEXT } else { DIM }.get(),
            )
        })
        .on_press(Msg::Act(Target::Cancel));
    let cancel = control(cancel, Target::Cancel.id(), Role::Button, words.cancel())
        .focus(
            focus == Some(Target::Cancel),
            Msg::Focus(Target::Cancel),
            ROW_RADIUS,
        )
        .activate(Msg::Act(Target::Cancel));
    let complete = ui.password.complete();
    let ready = complete && !busy;
    let join = button(parts::centred(
        if busy {
            words.connecting()
        } else {
            words.join()
        },
        BUTTON_TEXT,
        MANROPE_BOLD,
    ))
    .padding([0.0, BUTTON_PAD])
    .height(BUTTON)
    .style(move |_, _| {
        if ready {
            filled(ACCENT.get(), ROW_RADIUS, INK.get())
        } else {
            filled(FILL_HOVER.get(), ROW_RADIUS, FAINT.get())
        }
    })
    .on_press_maybe(ready.then_some(Msg::Act(Target::Join)));
    let mut join = control(join, Target::Join.id(), Role::Button, words.join())
        .node(|n| n.disabled(!complete));
    if ready {
        join = join
            .focus(
                focus == Some(Target::Join),
                Msg::Focus(Target::Join),
                ROW_RADIUS,
            )
            .activate(Msg::Act(Target::Join));
    }
    all = all.push(row![space::horizontal(), cancel, gap(6.0), join]);
    container(all)
        .padding(Padding {
            top: FORM_TOP,
            right: ROW_PAD,
            bottom: FORM_BOTTOM,
            left: FORM_INDENT,
        })
        .into()
}

/// What `row` says on its right, before its lock: "Connexion…" while it
/// joins, "dans les Réglages" when the popover cannot join it, "échec" when
/// it failed, "enregistré" when saved; its colour, and whether a spinner
/// turns before it.
fn row_aside(
    words: Words,
    row: &Wifi,
    unfolded: bool,
    busy: Option<&[u8]>,
    failed: Option<&[u8]>,
) -> Option<(&'static str, Color, bool)> {
    let this = Some(row.ssid.as_slice());
    let (text, colour, spinner) = if busy == this && !unfolded {
        (words.connecting(), DIM, true)
    } else if !row.security.joinable() {
        (words.in_settings(), FAINT, false)
    } else if failed == this {
        (words.failed(), PEACH, false)
    } else if row.saved && busy != this {
        (words.saved(), FAINT, false)
    } else {
        return None;
    };
    Some((text, colour.get(), spinner))
}

/// The lock, 14 px: `rect x5 y11 w14 h10 rx2` and `M8 11V8a4 4 0 0 1 8 0v3`.
fn lock(f: &mut iced::widget::canvas::Frame, at: Point) {
    let s = LOCK / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.2 * s, FAINT.get());
    f.stroke(
        &Path::rounded_rectangle(
            p(5.0, 11.0),
            Size::new(14.0 * s, 10.0 * s),
            (2.0 * s).into(),
        ),
        line,
    );
    let shackle = Path::new(|b| {
        b.move_to(p(8.0, 11.0));
        b.line_to(p(8.0, 8.0));
        b.arc(iced::widget::canvas::path::Arc {
            center: p(12.0, 8.0),
            radius: 4.0 * s,
            start_angle: Radians(std::f32::consts::PI),
            end_angle: Radians(std::f32::consts::TAU),
        });
        b.line_to(p(16.0, 11.0));
    });
    f.stroke(&shackle, line);
}

/// The eye in its 28 px button, 16 px: the almond `M2 12s3.6-7 10-7 10 7
/// 10 7-3.6 7-10 7S2 12 2 12z`, its pupil, and a slash while the password
/// shows.
fn eye(f: &mut iced::widget::canvas::Frame, button: Rectangle, shown: bool) {
    let s = EYE_ICON / 24.0;
    let at = Point::new(
        button.center_x() - EYE_ICON / 2.0,
        button.center_y() - EYE_ICON / 2.0,
    );
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, DIM.get());
    let almond = Path::new(|b| {
        b.move_to(p(2.0, 12.0));
        b.bezier_curve_to(p(2.0, 12.0), p(5.6, 5.0), p(12.0, 5.0));
        b.bezier_curve_to(p(18.4, 5.0), p(22.0, 12.0), p(22.0, 12.0));
        b.bezier_curve_to(p(22.0, 12.0), p(18.4, 19.0), p(12.0, 19.0));
        b.bezier_curve_to(p(5.6, 19.0), p(2.0, 12.0), p(2.0, 12.0));
        b.close();
    });
    f.stroke(&almond, line);
    f.stroke(&Path::circle(p(12.0, 12.0), 3.0 * s), line);
    if shown {
        f.stroke(&Path::line(p(3.0, 3.0), p(21.0, 21.0)), line);
    }
}

// --- What a message does -------------------------------------------------------

/// `message`, handled at once; then what the network says, the rows'
/// order, the ring off what is gone, and the list scrolled to what the
/// ring went to.
fn handle(message: Msg, motion: bool) {
    let network = network::snapshot();
    let before = ui(|ui| (ui.focus, ui.unfolded.clone()));
    match message {
        Msg::Stale => {}
        Msg::Act(target) => act(target, &network, motion),
        Msg::Focus(target) => ui(|ui| ui.focus = Some(target)),
        Msg::Took(took) => match took {
            Took::Edited => ui(|ui| ui.error = None),
            Took::Paste => paste(),
            Took::No | Took::Same | Took::Moved => {}
        },
        Msg::Fold => ui(|ui| {
            let rows = ui.rows(&network);
            let row = ui.unfolded_row(&rows);
            if network.connecting_to() == ui.unfolded.as_deref() {
                network::ask(Ask::Abandon);
            }
            ui.fold();
            ui.focus = row.map(Target::Row);
        }),
        Msg::OverList(over) => ui(|ui| ui.over_list = over),
        // As the canvas: a press takes the ring away, but the keyboard
        // stays in an unfolded field (the field's own press gave it).
        Msg::Pressed => ui(|ui| {
            if ui.unfolded.is_none() || ui.focus != Some(Target::Field) {
                ui.focus = None;
            }
        }),
    }
    let now = Instant::now();
    let (hold, reveal) = ui(|ui| {
        let hold = ui.react(&network);
        ui.relist(&network);
        ui.switch.settle(network.enabled, now);
        let rows = ui.rows(&network);
        let reveal = if (ui.focus, ui.unfolded.clone()) == before {
            None
        } else {
            match ui.focus {
                Some(Target::Row(i)) => Some(i),
                Some(Target::Field | Target::Eye | Target::Cancel | Target::Join) => {
                    ui.unfolded_row(&rows)
                }
                _ => None,
            }
        };
        (hold, reveal)
    });
    if hold {
        event(Seen::Hold);
    }
    if let Some(i) = reveal {
        LATER.push(ui::reveal(
            Id::new(LIST_ID),
            Id::from(format!("wifi:row:{i}")),
        ));
    }
    super::refit(Item::Wifi);
    tracing::debug!(
        natural = f32::from_bits(NATURAL.load(Ordering::Relaxed)),
        computed = height(),
        "the Wi-Fi popover's height"
    );
}

/// `target` pressed, or `Enter` on it.
fn act(target: Target, network: &Network, motion: bool) {
    let busy = network.connecting_to().is_some();
    match target {
        Target::Switch => {
            let on = ui(|ui| ui.switch.flip(network.enabled, Instant::now(), motion));
            // In airplane mode, the Wi-Fi alone comes back (6.6).
            if on {
                network::unblock_wifi();
            }
            network::ask(Ask::Enable(on));
        }
        Target::Settings => super::session::open_settings(Some("network")),
        Target::Disconnect => network::ask(Ask::Disconnect),
        Target::Portal => {
            if let Some(uri) = network.check_uri.clone() {
                tracing::info!("the sign-in page opens in the browser");
                network::open_in_browser(uri);
            }
        }
        Target::Row(i) => {
            let Some(row) = ui(|ui| ui.rows(network).get(i).cloned()) else {
                return;
            };
            if busy {
                return;
            }
            // 802.1X and WEP: their sheet in the Settings (6.6).
            if !row.security.joinable() {
                super::session::open_settings_join(&row.name);
                return;
            }
            let unfold = matches!(row.security, Security::Psk { .. }) && !row.saved;
            let hold = ui(|ui| {
                ui.failed = None;
                if !unfold {
                    ui.fold();
                    network::ask(Ask::Connect {
                        ssid: row.ssid.clone(),
                    });
                    return false;
                }
                if ui.unfolded.as_ref() == Some(&row.ssid) {
                    ui.fold();
                    ui.focus = Some(Target::Row(i));
                    false
                } else {
                    ui.unfold(row.ssid.clone());
                    true
                }
            });
            // A popover opened by hovering stays, for the typing.
            if hold {
                event(Seen::Hold);
            }
        }
        Target::Field => {}
        Target::Eye => ui(|ui| ui.shown = !ui.shown),
        Target::Cancel => ui(|ui| {
            if ui.unfolded.is_some() && busy {
                network::ask(Ask::Abandon);
            }
            let rows = ui.rows(network);
            let row = ui.unfolded_row(&rows);
            ui.fold();
            ui.focus = row.map(Target::Row);
        }),
        Target::Join => submit(network),
    }
}

/// The password tried, if it is long enough and nothing is under way.
fn submit(network: &Network) {
    if network.connecting_to().is_some() {
        return;
    }
    ui(|ui| {
        let Some(ssid) = ui.unfolded.clone() else {
            return;
        };
        if !ui.password.complete() {
            return;
        }
        ui.error = None;
        network::ask(Ask::ConnectWith {
            ssid,
            secret: ui.password.secret(),
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wifi(name: &str, strength: u8, security: Security) -> Wifi {
        Wifi {
            ssid: name.as_bytes().to_vec(),
            name: name.to_owned(),
            strength,
            security,
            saved: false,
            ap: format!("/{name}"),
        }
    }

    fn network(names: &[(&str, u8)]) -> Network {
        Network {
            manager: true,
            hardware: true,
            enabled: true,
            networks: names
                .iter()
                .map(|(n, s)| wifi(n, *s, Security::Psk { sae: false }))
                .collect(),
            ..Network::default()
        }
    }

    #[test]
    fn the_rows_hold_their_order_while_worked_in() {
        let mut ui = Ui::default();
        ui.relist(&network(&[("a", 80), ("b", 60), ("c", 40)]));
        assert_eq!(ui.order, [b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);
        // The pointer over the list: c grows stronger, d comes; neither
        // moves a row.
        ui.over_list = true;
        let changed = network(&[("c", 90), ("a", 80), ("d", 70), ("b", 60)]);
        ui.relist(&changed);
        assert_eq!(
            ui.order,
            [b"a".to_vec(), b"b".to_vec(), b"c".to_vec(), b"d".to_vec()]
        );
        // Gone, it goes; the pointer away, strength again.
        ui.relist(&network(&[("c", 90), ("a", 80), ("b", 60)]));
        assert_eq!(ui.order.len(), 3);
        ui.over_list = false;
        ui.relist(&network(&[("c", 90), ("a", 80), ("b", 60)]));
        assert_eq!(ui.order, [b"c".to_vec(), b"a".to_vec(), b"b".to_vec()]);
    }

    #[test]
    fn its_height_follows_what_it_shows() {
        let now = Instant::now();
        let mut ui = Ui::default();
        // Off: the header and its line, then the Settings' link (45 px).
        let mut off = network(&[]);
        off.enabled = false;
        assert_eq!(Shape::of(&off, &ui, &[], now).height(), 90 + 45);
        // Eight networks: six and a peek, the list at its most.
        let eight: Vec<(&str, u8)> = ["a", "b", "c", "d", "e", "f", "g", "h"]
            .iter()
            .map(|n| (*n, 50))
            .collect();
        let around = network(&eight);
        ui.relist(&around);
        let rows = ui.rows(&around);
        let shape = Shape::of(&around, &ui, &rows, now);
        assert_eq!(shape.list.map(|(view, _)| view), Some(LIST));
        // Two: no scrolling, and a row unfolded grows it.
        let two = network(&[("a", 80), ("b", 60)]);
        ui.relist(&two);
        let rows = ui.rows(&two);
        let small = Shape::of(&two, &ui, &rows, now).height();
        ui.unfold(b"b".to_vec());
        let unfolded = Shape::of(&two, &ui, &rows, now);
        assert_eq!(unfolded.height(), small + form_height(0).round() as u32);
        assert!(unfolded.list.is_some_and(|(view, content)| view >= content));
        // Folding clears what was typed.
        assert!(ui.password.paste("azerty&1@ê"));
        ui.fold();
        assert!(ui.password.is_empty() && ui.unfolded.is_none());
    }
}
