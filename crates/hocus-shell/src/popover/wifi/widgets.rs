//! The Wi-Fi's popover as a tree of iced's stock widgets (spike #770),
//! beside the canvas (`HOCUS_WIFI_WIDGETS=1` shows this one): the same
//! states, from the same model ([`super::Ui`], the network's snapshot),
//! at the mockup's sizes.
//!
//! Stock: `container`, `column`, `row`, `button`, `scrollable`,
//! `text_input`, `toggler`, `text`, `stack`, `mouse_area`. Ours
//! ([`super::custom`]): the surface around them and the keyboard's ring.
//! The glyphs stay the canvas's drawings, each in a small canvas of its
//! own: a canvas is a widget like another in the tree.
//!
//! What stays by hand, as iced 0.14 has it: which control holds the
//! keyboard (only text fields take iced's focus), and a name cut with "…"
//! (its text has no ellipsis).

use std::{
    sync::{
        Mutex, OnceLock, PoisonError,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::Instant,
};

use iced::{
    Background, Border, Color, Degrees, Element, Font, Length, Padding, Point, Rectangle, Renderer,
    Size, Theme,
    advanced::widget::{Id, Operation, operation::focusable},
    alignment::Vertical,
    gradient, keyboard, mouse,
    widget::{
        Space, button, canvas, column, container, mouse_area, row, scrollable, space, stack, text,
        text::{LineHeight, Shaping, Wrapping},
        text_input, toggler,
    },
};

use super::{
    Panel, Target, Ui,
    custom::{Around, reveal, ring},
    ui,
};
use crate::{
    levels::draw::{ACCENT, DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT},
    network::{
        self, Ask,
        draw::{ethernet, wifi as arcs},
        model::{Failure, Network, Security, Wifi},
        secret,
    },
    popover::{
        Effect, Event as Seen, Item, Pressed, event,
        kit::{
            self, FILL, FILL_HOVER, HEADER, HEADER_TOP, ICON, ICON_GAP, LABEL, MANROPE_SEMIBOLD,
            NAME, NOTE, NOTE_HEIGHT, NOTE_TOP, PAD_BOTTOM, PEACH, ROW, ROW_GAP, ROW_OUT, ROW_PAD,
            ROW_RADIUS, ROW_TEXT, SCROLLBAR, SMALL, SMALL_PAD, SMALL_RADIUS, STATUS, SWITCH, TITLE,
        },
    },
    theme::Ink,
};

/// The popover's widgets speak [`Msg`], which their surface handles.
type El<'a> = Element<'a, Msg>;

/// Whether the popover is the widgets' (`HOCUS_WIFI_WIDGETS=1`).
pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("HOCUS_WIFI_WIDGETS").is_ok_and(|v| v == "1"))
}

// --- The tokens the kit keeps to itself (a `style.rs` of the shell's, one day) -------

const BLOCK: f32 = 48.0;
const SMALL_BUTTON: f32 = 28.0;
const LABEL_SPACING: f32 = 0.08 * LABEL;
const BLOCK_FILL: Ink = Ink::Accent(0.10);
const RULE_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
const ROW_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.05));
const THUMB: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.16));
const SWITCH_OFF: Ink = Ink::Token(|t| t.switch_off);
const GLASS_TOP: Ink = Ink::Token(|t| t.popover_top);
const GLASS_BOTTOM: Ink = Ink::Token(|t| t.popover_bottom);
const OPAQUE_TOP: Ink = Ink::Token(|t| t.popover_opaque_top);
const OPAQUE_BOTTOM: Ink = Ink::Token(|t| t.popover_opaque_bottom);
const RIM: Ink = Ink::Token(|t| t.bar_border);
const SPINNER: f32 = 12.0;

/// The header's switch is iced's `toggler` (`HOCUS_WIFI_STOCK_SWITCH=1`),
/// to see what it lacks; else the kit's drawing in a stock button.
fn stock_switch() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("HOCUS_WIFI_STOCK_SWITCH").is_ok_and(|v| v == "1"))
}

const FIELD_ID: &str = "wifi:field";
const LIST_ID: &str = "wifi:list";

fn row_id(i: usize) -> Id {
    Id::from(format!("wifi:row:{i}"))
}

// --- What changes, and when the view is stale ------------------------------------------

/// What a widget asks.
#[derive(Clone)]
pub enum Msg {
    /// A control pressed, or `Enter` in the field.
    Act(Target),
    /// The field's text, as edited.
    Typed(String),
    /// The pointer came over the list, or left it.
    OverList(bool),
    /// A key the surface kept from the widgets.
    Key(Pressed),
    /// `Ctrl+V` or `Shift+Insert` in the field.
    Paste,
    /// A press anywhere on the popover; whether the field holds iced's
    /// focus after it.
    Pressed(bool),
    /// The network, or what a paste brought, changed under the view.
    Stale,
}

impl std::fmt::Debug for Msg {
    // What is typed is a password: never shown.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Act(_) => "Act",
            Self::Typed(_) => "Typed",
            Self::OverList(_) => "OverList",
            Self::Key(_) => "Key",
            Self::Paste => "Paste",
            Self::Pressed(_) => "Pressed",
            Self::Stale => "Stale",
        })
    }
}

/// What waits for the tree built after a message: iced's focus to the
/// field or away from it, the list scrolled to a row.
#[derive(Debug, Clone, Copy)]
enum Op {
    Focus,
    Unfocus,
    Reveal(usize),
}

static PENDING: Mutex<Vec<Op>> = Mutex::new(Vec::new());

fn later(op: Op) {
    PENDING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(op);
}

fn pending() -> Vec<Box<dyn Operation>> {
    let ops = std::mem::take(&mut *PENDING.lock().unwrap_or_else(PoisonError::into_inner));
    ops.into_iter()
        .map(|op| -> Box<dyn Operation> {
            match op {
                Op::Focus => Box::new(focusable::focus(Id::new(FIELD_ID))),
                Op::Unfocus => Box::new(focusable::unfocus()),
                Op::Reveal(i) => Box::new(reveal(Id::new(LIST_ID), row_id(i))),
            }
        })
        .collect()
}

/// Counts what changes outside a message: the network, a paste landed.
static STAMP: AtomicU64 = AtomicU64::new(0);
/// Counts the view's builds.
static BUILDS: AtomicU64 = AtomicU64::new(0);
/// The height the tree took when last laid out, px (bits of an `f32`).
static NATURAL: AtomicU32 = AtomicU32::new(0);

/// What the popover shows changed outside a message.
pub fn touch() {
    STAMP.fetch_add(1, Ordering::Relaxed);
}

/// The height the widgets take by themselves, once laid out.
pub fn natural() -> Option<u32> {
    let height = f32::from_bits(NATURAL.load(Ordering::Relaxed));
    (height > 0.0).then(|| height.round() as u32)
}

// --- The view ------------------------------------------------------------------------------

/// The popover on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let network = network::snapshot();
    let built = STAMP.load(Ordering::Relaxed);
    let content = ui(|ui| {
        let rows = ui.rows(&network);
        content(panel, &network, ui, &rows, Instant::now())
    });
    let motion = panel.motion;
    let around = Around {
        id: panel.id,
        build: BUILDS.fetch_add(1, Ordering::Relaxed),
        content,
        stale: Box::new(move || STAMP.load(Ordering::Relaxed) != built),
        on_stale: Box::new(|| Msg::Stale),
        on_key: Box::new(taken),
        on_press: Box::new(|focused| Msg::Pressed(focused == Some(Id::new(FIELD_ID)))),
        handle: Box::new(move |message| handle(message, motion)),
        pending: Box::new(pending),
        natural: Box::new(|height| NATURAL.store(height.to_bits(), Ordering::Relaxed)),
        redraw: Box::new(|| {
            crate::popover::send(vec![Effect::Wake(hocus_link::POPOVER_NAMESPACE)]);
        }),
        wake: crate::Message::Keys,
    };
    let glass = panel.glass;
    container(around)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| background(glass))
        .into()
}

/// The keys the surface keeps from its widgets, as the keyboard is held
/// now: those that move the ring or act on what holds it; in the field,
/// only what leaves it, and a paste (iced's clipboard is off in the
/// shell, [`crate::paste`] reads it).
fn taken(pressed: Pressed) -> Option<Msg> {
    use keyboard::{Key, key::Named};
    let typing = ui(|ui| ui.focus == Some(Target::Field));
    match &pressed.key {
        Key::Named(Named::Escape | Named::Tab) => Some(Msg::Key(pressed)),
        Key::Named(Named::ArrowUp | Named::ArrowDown | Named::Enter | Named::Space) if !typing => {
            Some(Msg::Key(pressed))
        }
        _ if typing && crate::paste::asks(&pressed.key, pressed.modifiers) => Some(Msg::Paste),
        _ => None,
    }
}

fn content<'a>(panel: &Panel, network: &Network, ui: &Ui, rows: &[Wifi], now: Instant) -> El<'a> {
    let words = panel.words;
    let on = ui.enabled(network, now);
    let enabled = on && network.hardware;
    let focus = ui.focus;
    // A widget keeps what it knows (the pointer over the list, how far it
    // is scrolled, the field's caret) by its place in the tree, and the
    // blocks above the list come and go: each has its place, empty while
    // it does not show. (iced 0.14's keyed column pairs a tree with
    // another widget's when more than one run of blocks changes at once,
    // and panics: not on the shell's loop.)
    let mut all: Vec<El<'a>> = vec![header(panel, network.hardware, on, focus)];
    let mut block = |content: Option<El<'a>>| {
        let spaced: El<'a> = match content {
            Some(content) => container(content)
                .padding(Padding {
                    top: super::GAP,
                    ..Padding::ZERO
                })
                .into(),
            None => Space::new().into(),
        };
        all.push(spaced);
    };
    block(network.wired.then(|| {
        let line = row![
            glyph(Glyph::Ethernet(DIM.get()), ICON),
            gap_x(ICON_GAP),
            said(words.by_cable(), NOTE, MANROPE, DIM.get()),
        ]
        .align_y(Vertical::Center);
        container(line)
            .height(super::WIRED)
            .padding([0.0, ROW_OUT])
            .align_y(Vertical::Center)
            .into()
    }));
    block((!enabled).then(|| {
        note(if network.airplane {
            words.airplane_is_on()
        } else if network.hardware {
            words.wifi_is_off()
        } else {
            words.no_adapter()
        })
    }));
    let current = network.current.as_ref().filter(|_| enabled);
    block(current.map(|current| connected(panel, current, network, focus)));
    block((current.is_some() && network.portal).then(|| {
        let open = button(
            container(said(
                words.open_sign_in(),
                super::PORTAL_TEXT,
                MANROPE_SEMIBOLD,
                PEACH.get(),
            ))
            .center(Length::Fill),
        )
        .padding(0)
        .width(Length::Fill)
        .height(super::PORTAL)
        .style(|_, status| {
            filled(
                if lit(status) {
                    super::PORTAL_HOVER
                } else {
                    super::PORTAL_FILL
                }
                .get(),
                ROW_RADIUS,
                PEACH.get(),
            )
        })
        .on_press_maybe(
            network
                .check_uri
                .is_some()
                .then_some(Msg::Act(Target::Portal)),
        );
        inset(control(open, Target::Portal, focus, ROW_RADIUS))
    }));
    block(enabled.then(rule));
    block(enabled.then(|| label(panel, network.scanning)));
    block(enabled.then(|| {
        if rows.is_empty() {
            note(if network.scanning {
                words.searching_networks()
            } else {
                words.no_network()
            })
        } else {
            list(panel, network, ui, rows)
        }
    }));
    all.push(rule());
    all.push(link(panel, focus));
    container(iced::widget::Column::with_children(all))
        .padding(Padding {
            top: HEADER_TOP,
            right: kit::LEFT - ROW_OUT,
            bottom: PAD_BOTTOM + kit::BORDER,
            left: kit::LEFT - ROW_OUT,
        })
        .into()
}

/// The title, and the switch.
fn header<'a>(panel: &Panel, hardware: bool, on: bool, focus: Option<Target>) -> El<'a> {
    let mut line = row![
        said(panel.words.wifi(), TITLE, MANROPE_BOLD, TEXT.get()),
        space::horizontal()
    ]
    .align_y(Vertical::Center);
    if hardware {
        let switch: El<'a> = if stock_switch() {
            // Stock: twice as wide as tall (44 × 22 for the mockup's
            // 40 × 22), no shadow under its knob, no slide.
            toggler(on)
                .on_toggle(|_| Msg::Act(Target::Switch))
                .size(SWITCH.height)
                .style(|_, status| {
                    let on = matches!(
                        status,
                        toggler::Status::Active { is_toggled: true }
                            | toggler::Status::Hovered { is_toggled: true }
                            | toggler::Status::Disabled { is_toggled: true }
                    );
                    toggler::Style {
                        background: if on { ACCENT } else { SWITCH_OFF }.get().into(),
                        background_border_width: 0.0,
                        background_border_color: Color::TRANSPARENT,
                        foreground: TEXT.get().into(),
                        foreground_border_width: 0.0,
                        foreground_border_color: Color::TRANSPARENT,
                        text_color: None,
                        border_radius: None,
                        padding_ratio: 2.0 / SWITCH.height,
                    }
                })
                .into()
        } else {
            // The kit's drawing (its knob's shadow, its slide) in a
            // stock button.
            button(
                canvas(Glyph::Switch(panel.motion))
                    .width(SWITCH.width)
                    .height(SWITCH.height),
            )
            .padding(0)
            .style(|_, _| clear())
            .on_press(Msg::Act(Target::Switch))
            .into()
        };
        line = line.push(control(switch, Target::Switch, focus, SWITCH.height / 2.0));
    }
    container(line)
        .height(HEADER)
        .padding([0.0, ROW_OUT])
        .align_y(Vertical::Center)
        .into()
}

/// The network connected: its signal, its name over its state, Disconnect.
fn connected<'a>(
    panel: &Panel,
    current: &Wifi,
    network: &Network,
    focus: Option<Target>,
) -> El<'a> {
    let words = panel.words;
    let disconnect = kit::measure(words.disconnect(), STATUS, MANROPE) + 2.0 * SMALL_PAD;
    let name = kit::elide(&current.name, kit::block_room(disconnect), |s| {
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
    let leave = button(centred(words.disconnect(), STATUS, MANROPE))
        .padding([0.0, SMALL_PAD])
        .height(SMALL_BUTTON)
        .style(|_, status| {
            filled(
                if lit(status) { FILL_HOVER } else { FILL }.get(),
                SMALL_RADIUS,
                if lit(status) { TEXT } else { DIM }.get(),
            )
        })
        .on_press(Msg::Act(Target::Disconnect));
    let line = row![
        glyph(Glyph::Wifi(current.bars(), ACCENT.get()), ICON),
        gap_x(ICON_GAP),
        texts,
        gap_x(ICON_GAP),
        control(leave, Target::Disconnect, focus, SMALL_RADIUS),
        // 48 px at least.
        Space::new().height(BLOCK),
    ]
    .align_y(Vertical::Center);
    container(line)
        .padding([0.0, ROW_OUT])
        .style(|_| boxed(BLOCK_FILL.get(), ROW_RADIUS))
        .into()
}

/// The section's label, its letters spaced, and "Searching…" on its right.
fn label<'a>(panel: &Panel, scanning: bool) -> El<'a> {
    let words = panel.words;
    // No letter-spacing in iced's text: a letter a widget.
    let mut letters = row![].spacing(LABEL_SPACING);
    for ch in words.networks().chars() {
        letters = letters.push(said(ch.to_string(), LABEL, MANROPE_BOLD, DIM.get()));
    }
    let mut line = row![letters, space::horizontal()].align_y(Vertical::Center);
    if scanning {
        if panel.motion {
            line = line
                .push(glyph(Glyph::Spinner(FAINT.get()), SPINNER))
                .push(gap_x(6.0));
        }
        line = line.push(said(words.searching(), SMALL, MANROPE, FAINT.get()));
    }
    container(line)
        .height(super::LABEL_ROW)
        .padding([0.0, ROW_OUT])
        .align_y(Vertical::Center)
        .into()
}

/// The networks around, scrolling past six.
fn list<'a>(panel: &Panel, network: &Network, ui: &Ui, rows: &[Wifi]) -> El<'a> {
    let words = panel.words;
    let busy = network.connecting_to();
    let unfolded = |row: &Wifi| ui.unfolded.as_ref() == Some(&row.ssid);
    // Whether it scrolls, for the names' room alone: the rows' own width
    // is the scrollable's business.
    let form = rows.iter().any(unfolded).then(|| {
        let error = ui.error.map_or(0, |e| super::error_lines(e).len());
        super::form_height(error)
    });
    let content = rows.len() as f32 * (ROW + ROW_GAP) - ROW_GAP + form.unwrap_or(0.0);
    let width = kit::RIGHT - kit::LEFT + 2.0 * ROW_OUT
        - if content > super::LIST + 0.5 {
            SCROLLBAR + super::ROW_END
        } else {
            super::ROW_END
        };
    let mut all = column![].spacing(ROW_GAP);
    for (i, network) in rows.iter().enumerate() {
        let unfolded = unfolded(network);
        let aside = super::row_aside(words, network, unfolded, busy, ui.failed.as_deref());
        let locked = network.security.locked();
        let room = kit::name_room(
            width,
            if locked { super::LOCK + ICON_GAP } else { 0.0 },
            aside.map(|a| {
                (
                    kit::measure(a.text, SMALL, MANROPE),
                    a.spinner && panel.motion,
                )
            }),
        );
        let name = kit::elide(&network.name, room, |s| kit::measure(s, ROW_TEXT, MANROPE));
        let mut line = row![
            glyph(Glyph::Wifi(network.bars(), TEXT.get()), ICON),
            gap_x(ICON_GAP),
            said(name, ROW_TEXT, MANROPE, TEXT.get()),
            space::horizontal(),
        ]
        .align_y(Vertical::Center);
        if let Some(aside) = aside {
            if aside.spinner && panel.motion {
                line = line
                    .push(glyph(Glyph::Spinner(DIM.get()), SPINNER))
                    .push(gap_x(6.0));
            }
            line = line.push(said(aside.text, SMALL, MANROPE, aside.colour));
        }
        if locked {
            line = line
                .push(gap_x(ICON_GAP))
                .push(glyph(Glyph::Lock, super::LOCK));
        }
        let pressed = button(container(line).center_y(Length::Fill))
            .padding([0.0, ROW_PAD])
            .width(Length::Fill)
            .height(ROW)
            .style(move |_, status| {
                if !unfolded && lit(status) {
                    hovered(ROW_HOVER.get(), ROW_RADIUS)
                } else {
                    clear()
                }
            })
            .on_press_maybe(busy.is_none().then_some(Msg::Act(Target::Row(i))));
        let pressed = control(pressed, Target::Row(i), ui.focus, ROW_RADIUS);
        let block: El<'a> = if unfolded {
            container(column![pressed, self::form(panel, network_busy(busy), ui)])
                .style(|_| {
                    let lit = hovered(ROW_HOVER.get(), ROW_RADIUS);
                    container::Style {
                        background: lit.background,
                        border: lit.border,
                        ..container::Style::default()
                    }
                })
                .into()
        } else {
            pressed
        };
        all = all.push(container(block).id(row_id(i)));
    }
    let scrolled = scrollable(container(all).padding(Padding {
        right: super::ROW_END,
        ..Padding::ZERO
    }))
    .id(LIST_ID)
    .direction(scrollable::Direction::Vertical(
        scrollable::Scrollbar::new()
            .width(SCROLLBAR)
            .scroller_width(SCROLLBAR)
            .margin(0)
            .spacing(0),
    ))
    .style(|_, _| {
        let rail = scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller {
                background: THUMB.get().into(),
                border: round(SCROLLBAR / 2.0),
            },
        };
        scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll: scrollable::AutoScroll {
                background: Color::TRANSPARENT.into(),
                border: Border::default(),
                shadow: iced::Shadow::default(),
                icon: Color::TRANSPARENT,
            },
        }
    });
    container(
        mouse_area(scrolled)
            .on_enter(Msg::OverList(true))
            .on_exit(Msg::OverList(false)),
    )
    .max_height(super::LIST)
    .into()
}

fn network_busy(busy: Option<&[u8]>) -> bool {
    busy.is_some()
}

/// The password's form under its row: the field and its eye, what went
/// wrong, Cancel and Connect.
fn form<'a>(panel: &Panel, busy: bool, ui: &Ui) -> El<'a> {
    let words = panel.words;
    let error = ui.error;
    let focus = ui.focus;
    let pad = (super::FIELD - super::FIELD_TEXT * LINE) / 2.0;
    let field = text_input(words.password(), ui.password.text())
        .id(FIELD_ID)
        .secure(!ui.shown)
        .on_input(Msg::Typed)
        .on_submit(Msg::Act(Target::Join))
        .size(super::FIELD_TEXT)
        .font(MANROPE)
        .line_height(LineHeight::Relative(LINE))
        .padding(Padding {
            top: pad,
            right: super::FIELD_RIGHT,
            bottom: pad,
            left: super::FIELD_PAD,
        })
        .width(Length::Fill)
        .style(move |_, status| text_input::Style {
            background: FILL.get().into(),
            border: Border {
                color: if error.is_some() {
                    super::ORANGE
                } else {
                    super::FIELD_BORDER
                }
                .get(),
                width: 1.0,
                radius: ROW_RADIUS.into(),
            },
            icon: DIM.get(),
            placeholder: FAINT.get(),
            value: TEXT.get(),
            selection: Ink::Accent(if matches!(status, text_input::Status::Focused { .. }) {
                0.35
            } else {
                0.2
            })
            .get(),
        });
    let eye = button(glyph(Glyph::Eye(ui.shown), super::EYE))
        .padding(0)
        .width(super::EYE)
        .height(super::EYE)
        .style(|_, _| clear())
        .on_press(Msg::Act(Target::Eye));
    let field = stack![
        field,
        container(control(eye, Target::Eye, focus, 6.0))
            .align_right(Length::Fill)
            .center_y(Length::Fill)
            .padding(Padding {
                right: 3.0,
                ..Padding::ZERO
            }),
    ];
    let mut all = column![field, Space::new().height(super::FORM_GAP)];
    if let Some(error) = error {
        let lines = match error {
            Failure::Secret => words.wrong_password(),
            Failure::Other => words.could_not_connect(),
        };
        // It wraps by itself, in the room the canvas gives it.
        all = all
            .push(
                said(lines, super::ERROR, MANROPE, PEACH.get())
                    .wrapping(Wrapping::Word)
                    .width(super::ERROR_ROOM),
            )
            .push(Space::new().height(super::FORM_GAP));
    }
    let cancel = button(centred(words.cancel(), super::BUTTON_TEXT, MANROPE))
        .padding([0.0, super::BUTTON_PAD])
        .height(super::BUTTON)
        .style(|_, status| {
            filled(
                if lit(status) { FILL_HOVER } else { FILL }.get(),
                ROW_RADIUS,
                if lit(status) { TEXT } else { DIM }.get(),
            )
        })
        .on_press(Msg::Act(Target::Cancel));
    let ready = ui.password.complete() && !busy;
    let join = button(centred(
        if busy {
            words.connecting()
        } else {
            words.join()
        },
        super::BUTTON_TEXT,
        MANROPE_BOLD,
    ))
    .padding([0.0, super::BUTTON_PAD])
    .height(super::BUTTON)
    .style(move |_, _| {
        if ready {
            filled(ACCENT.get(), ROW_RADIUS, super::INK.get())
        } else {
            filled(FILL_HOVER.get(), ROW_RADIUS, FAINT.get())
        }
    })
    .on_press_maybe(ready.then_some(Msg::Act(Target::Join)));
    all = all.push(row![
        space::horizontal(),
        control(cancel, Target::Cancel, focus, ROW_RADIUS),
        gap_x(6.0),
        control(join, Target::Join, focus, ROW_RADIUS),
    ]);
    container(all)
        .padding(Padding {
            top: super::FORM_TOP,
            right: ROW_PAD,
            bottom: super::FORM_BOTTOM,
            left: super::FORM_INDENT,
        })
        .into()
}

/// "Wi-Fi settings…": the sliders in their tile, the words, a chevron.
fn link<'a>(panel: &Panel, focus: Option<Target>) -> El<'a> {
    let tile = container(glyph(Glyph::Sliders, super::LINK_TILE))
        .style(|_| boxed(super::LINK_TILE_FILL, super::LINK_TILE_RADIUS));
    let line = row![
        tile,
        gap_x(ICON_GAP),
        said(
            panel.words.wifi_settings(),
            super::LINK_TEXT,
            MANROPE_SEMIBOLD,
            TEXT.get()
        ),
        space::horizontal(),
        glyph(Glyph::Chevron, 14.0),
    ]
    .align_y(Vertical::Center);
    let link = button(container(line).center_y(Length::Fill))
        .padding([0.0, ROW_OUT])
        .width(Length::Fill)
        .height(super::LINK)
        .style(|_, status| {
            if lit(status) {
                hovered(super::LINK_HOVER, super::LINK_RADIUS)
            } else {
                clear()
            }
        })
        .on_press(Msg::Act(Target::Settings));
    control(link, Target::Settings, focus, ROW_RADIUS)
}

// --- The pieces ----------------------------------------------------------------------------

/// `content` in `font` at `size`, on the mockups' line.
pub(crate) fn said<'a>(
    content: impl text::IntoFragment<'a>,
    size: f32,
    font: Font,
    colour: Color,
) -> iced::widget::Text<'a> {
    text(content)
        .size(size)
        .font(font)
        .line_height(LineHeight::Relative(LINE))
        .shaping(Shaping::Advanced)
        .wrapping(Wrapping::None)
        .color(colour)
}

/// A button's word, in the middle of its height, in the button's colour.
fn centred<'a>(word: &'static str, size: f32, font: Font) -> El<'a> {
    container(
        text(word)
            .size(size)
            .font(font)
            .line_height(LineHeight::Relative(LINE))
            .shaping(Shaping::Advanced)
            .wrapping(Wrapping::None),
    )
    .center_y(Length::Fill)
    .into()
}

pub(crate) fn gap_x<'a>(width: f32) -> El<'a> {
    Space::new().width(width).into()
}

/// A control: under the keyboard's ring while it holds it, in a container
/// named after it, which tests (and a screen reader's nodes) find it by.
fn control<'a>(
    content: impl Into<El<'a>>,
    target: Target,
    focus: Option<Target>,
    radius: f32,
) -> El<'a> {
    container(ring(content, focus == Some(target), radius))
        .id(Id::from(format!("wifi:{target:?}")))
        .into()
}

/// `content` between the content's sides, 6 px in from the rows'.
fn inset<'a>(content: El<'a>) -> El<'a> {
    container(content).padding([0.0, ROW_OUT]).into()
}

/// What is said in place of a list.
fn note<'a>(note: &'static str) -> El<'a> {
    container(said(note, NOTE, MANROPE, FAINT.get()))
        .height(NOTE_HEIGHT)
        .padding(Padding {
            top: NOTE_TOP,
            right: ROW_OUT,
            bottom: 0.0,
            left: ROW_OUT,
        })
        .into()
}

/// A divider: 1 px across the content, 4 px around it.
fn rule<'a>() -> El<'a> {
    container(
        container(Space::new().width(Length::Fill).height(1.0))
            .style(|_| boxed(RULE_FILL.get(), 0.0)),
    )
    .padding(Padding {
        top: super::RULE,
        right: ROW_OUT,
        bottom: super::RULE,
        left: ROW_OUT,
    })
    .into()
}

// --- The styles ------------------------------------------------------------------------------

pub(crate) fn lit(status: button::Status) -> bool {
    matches!(status, button::Status::Hovered | button::Status::Pressed)
}

pub(crate) fn round(radius: f32) -> Border {
    Border {
        radius: radius.into(),
        ..Border::default()
    }
}

pub(crate) fn boxed(fill: Color, radius: f32) -> container::Style {
    container::Style {
        background: Some(fill.into()),
        border: round(radius),
        ..container::Style::default()
    }
}

/// A button filled, its words in `ink`.
pub(crate) fn filled(fill: Color, radius: f32, ink: Color) -> button::Style {
    button::Style {
        background: Some(fill.into()),
        text_color: ink,
        border: round(radius),
        ..button::Style::default()
    }
}

pub(crate) fn clear() -> button::Style {
    button::Style {
        background: None,
        text_color: TEXT.get(),
        ..button::Style::default()
    }
}

/// Lit under the pointer: its fill; under high contrast, which has no
/// fills, a 1 px border in `text.dim` (as [`crate::theme::hover`]).
pub(crate) fn hovered(fill: Color, radius: f32) -> button::Style {
    if crate::theme::contrast() {
        button::Style {
            background: None,
            text_color: TEXT.get(),
            border: Border {
                color: DIM.get(),
                width: 1.0,
                radius: radius.into(),
            },
            ..button::Style::default()
        }
    } else {
        filled(fill, radius, TEXT.get())
    }
}

/// The popover's glass (or its opaque colour) and its rim.
pub(crate) fn background(glass: bool) -> container::Style {
    let (top, bottom) = if glass {
        (GLASS_TOP, GLASS_BOTTOM)
    } else {
        (OPAQUE_TOP, OPAQUE_BOTTOM)
    };
    container::Style {
        background: Some(Background::Gradient(
            gradient::Linear::new(Degrees(180.0))
                .add_stop(0.0, top.get())
                .add_stop(1.0, bottom.get())
                .into(),
        )),
        border: Border {
            color: RIM.get(),
            width: crate::theme::border(),
            radius: (hocus_link::RADIUS as f32).into(),
        },
        ..container::Style::default()
    }
}

// --- The glyphs: the canvas's drawings, each a small canvas -----------------------------------

#[derive(Debug, Clone, Copy)]
enum Glyph {
    Wifi(u8, Color),
    Ethernet(Color),
    Lock,
    Eye(bool),
    Sliders,
    Chevron,
    Spinner(Color),
    /// The header's switch, as the network and what was asked say; it
    /// slides (with motion).
    Switch(bool),
}

fn glyph<'a>(glyph: Glyph, size: f32) -> El<'a> {
    canvas(glyph).width(size).height(size).into()
}

/// A spinner's turn.
#[derive(Debug, Default)]
struct Turning {
    spin: kit::Spin,
    now: Option<Instant>,
}

impl<Message> canvas::Program<Message> for Glyph {
    type State = Turning;

    fn update(
        &self,
        state: &mut Turning,
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let iced::Event::Window(iced::window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        state.now = Some(*now);
        match self {
            // A spinner alone draws again each frame, and its surface alone.
            Self::Spinner(_) => {
                state.spin.run(true, *now);
                Some(canvas::Action::request_redraw())
            }
            Self::Switch(true) if ui(|ui| ui.switch.sliding(*now)) => {
                Some(canvas::Action::request_redraw())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Turning,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        match *self {
            Self::Wifi(bars, colour) => {
                arcs(
                    &mut f,
                    Point::ORIGIN,
                    bounds.width,
                    bars,
                    colour,
                    false,
                    1.0,
                );
            }
            Self::Ethernet(colour) => ethernet(&mut f, Point::ORIGIN, bounds.width, colour),
            Self::Lock => super::lock(&mut f, Point::ORIGIN),
            Self::Eye(shown) => super::eye(
                &mut f,
                Rectangle::new(Point::ORIGIN, Size::new(super::EYE, super::EYE)),
                shown,
            ),
            Self::Sliders => {
                for segment in [
                    [(4.0, 7.0), (13.0, 7.0)],
                    [(17.0, 7.0), (20.0, 7.0)],
                    [(15.0, 4.5), (15.0, 9.5)],
                    [(4.0, 17.0), (7.0, 17.0)],
                    [(11.0, 17.0), (20.0, 17.0)],
                    [(9.0, 14.5), (9.0, 19.5)],
                ] {
                    let at = Point::new(4.5, 4.5);
                    super::polyline(&mut f, at, 15.0 / 24.0, 2.2, DIM.get(), &segment);
                }
            }
            Self::Chevron => super::polyline(
                &mut f,
                Point::ORIGIN,
                14.0 / 24.0,
                2.4,
                FAINT.get(),
                &[(9.0, 6.0), (15.0, 12.0), (9.0, 18.0)],
            ),
            Self::Switch(motion) => {
                let now = state.now.unwrap_or_else(Instant::now);
                let network = network::snapshot();
                let at = ui(|ui| {
                    let on = ui.enabled(&network, now);
                    ui.switch.knob_at(on, now, motion)
                });
                let rect = kit::Switch::rect();
                f.translate(iced::Vector::new(-rect.x, -rect.y));
                kit::Switch::draw(&mut f, at);
            }
            Self::Spinner(colour) => {
                let turn = state.now.map_or(0.0, |now| state.spin.turn(now));
                kit::spinner(&mut f, Point::ORIGIN, colour, turn);
            }
        }
        vec![f.into_geometry()]
    }
}

// --- What a message does ---------------------------------------------------------------------

/// What can hold the keyboard, in `Tab`'s order (as [`super::targets`],
/// from what is shown, not from where it is).
fn order(network: &Network, ui: &Ui, rows: &[Wifi], now: Instant) -> Vec<Target> {
    let enabled = ui.enabled(network, now) && network.hardware;
    let current = enabled && network.current.is_some();
    let mut order = Vec::new();
    if network.hardware {
        order.push(Target::Switch);
    }
    if current {
        order.push(Target::Disconnect);
    }
    if current && network.portal && network.check_uri.is_some() {
        order.push(Target::Portal);
    }
    if enabled {
        let busy = network.connecting_to().is_some();
        for (i, row) in rows.iter().enumerate() {
            if !busy {
                order.push(Target::Row(i));
            }
            if ui.unfolded.as_ref() == Some(&row.ssid) {
                order.extend([Target::Field, Target::Eye, Target::Cancel]);
                if ui.password.complete() && !busy {
                    order.push(Target::Join);
                }
            }
        }
    }
    order.push(Target::Settings);
    order
}

/// `message`, handled at once; what the tree built next is then asked:
/// iced's focus to the field or away from it, the list scrolled to what
/// holds the keyboard.
fn handle(message: Msg, motion: bool) {
    let network = network::snapshot();
    let before = ui(|ui| ui.focus);
    // The field holds iced's focus already: not asked again (it would
    // send its caret to its end).
    let mut focused = false;
    let mut moved = false;
    let said = format!("{message:?}");
    match message {
        Msg::Stale => {}
        Msg::Act(target) => act(target, &network, motion),
        Msg::Typed(typed) => ui(|ui| {
            if typed.chars().count() <= secret::LONGEST {
                ui.password.clear();
                ui.password.set(&typed);
                ui.error = None;
            }
        }),
        Msg::OverList(over) => ui(|ui| ui.over_list = over),
        Msg::Key(pressed) => moved = key(&pressed, &network, motion),
        Msg::Paste => super::paste(),
        // As the canvas: a press takes the ring away, but the keyboard
        // stays in an unfolded field (which a press elsewhere made let
        // go of iced's focus: it is given back).
        Msg::Pressed(on_field) => ui(|ui| {
            focused = on_field;
            if ui.unfolded.is_none() {
                ui.focus = None;
            } else if on_field {
                ui.focus = Some(Target::Field);
            } else if ui.focus == Some(Target::Field) {
                later(Op::Focus);
            } else {
                ui.focus = None;
            }
        }),
    }
    // What the network's last attempt says, the rows' order, a ring on
    // what is gone.
    let now = Instant::now();
    let hold = ui(|ui| {
        let hold = ui.react(&network);
        ui.relist(&network);
        ui.switch.settle(network.enabled, now);
        let rows = ui.rows(&network);
        if ui
            .focus
            .is_some_and(|f| !order(&network, ui, &rows, now).contains(&f))
        {
            ui.focus = None;
        }
        hold
    });
    if hold {
        event(Seen::Hold);
    }
    crate::popover::refit(Item::Wifi);
    // What the tree took by itself when last laid out, beside what the
    // canvas's layout computes: the spike's check that the first can
    // replace the second.
    tracing::debug!(
        natural = natural(),
        computed = super::height(),
        "the widgets' height"
    );
    let after = ui(|ui| ui.focus);
    tracing::debug!(?said, ?before, ?after, "the widgets' message");
    match (before == Some(Target::Field), after == Some(Target::Field)) {
        (false, true) if !focused => later(Op::Focus),
        (true, false) => later(Op::Unfocus),
        _ => {}
    }
    let shown = match after {
        Some(Target::Row(i)) if moved => Some(i),
        Some(Target::Field | Target::Eye | Target::Cancel | Target::Join) if moved => ui(|ui| {
            let rows = ui.rows(&network);
            ui.unfolded
                .as_ref()
                .and_then(|s| rows.iter().position(|n| n.ssid == *s))
        }),
        _ => None,
    };
    if let Some(i) = shown {
        later(Op::Reveal(i));
    }
}

/// `target` pressed, or `Enter` on it (as the canvas's `act`).
fn act(target: Target, network: &Network, motion: bool) {
    let busy = network.connecting_to().is_some();
    match target {
        Target::Switch => {
            let on = ui(|ui| ui.switch.flip(network.enabled, Instant::now(), motion));
            if on {
                network::unblock_wifi();
            }
            network::ask(Ask::Enable(on));
        }
        Target::Settings => crate::popover::session::open_settings(Some("network")),
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
            if !row.security.joinable() {
                crate::popover::session::open_settings_join(&row.name);
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
            let row = ui
                .unfolded
                .as_ref()
                .and_then(|s| rows.iter().position(|n| n.ssid == *s));
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

/// A key the surface kept: whether the ring moved.
fn key(pressed: &Pressed, network: &Network, motion: bool) -> bool {
    use keyboard::{Key, key::Named};
    let (rows, order, focus) = ui(|ui| {
        let rows = ui.rows(network);
        let order = order(network, ui, &rows, Instant::now());
        (rows, order, ui.focus)
    });
    match &pressed.key {
        Key::Named(Named::Escape) => {
            let folded = ui(|ui| {
                let row = ui
                    .unfolded
                    .as_ref()
                    .and_then(|s| rows.iter().position(|n| n.ssid == *s))?;
                if network.connecting_to() == ui.unfolded.as_deref() {
                    network::ask(Ask::Abandon);
                }
                ui.fold();
                ui.focus = Some(Target::Row(row));
                Some(())
            });
            if folded.is_none() {
                event(Seen::Escape);
            }
        }
        Key::Named(Named::Tab) => {
            let next = kit::step(&order, focus, pressed.modifiers.shift());
            ui(|ui| ui.focus = next);
        }
        Key::Named(Named::ArrowUp | Named::ArrowDown) => {
            let Some(row @ Target::Row(_)) = focus else {
                return false;
            };
            let down = matches!(pressed.key, Key::Named(Named::ArrowDown));
            let rows: Vec<Target> = order
                .iter()
                .copied()
                .filter(|t| matches!(t, Target::Row(_)))
                .collect();
            let Some(next) = kit::cycle(&rows, row, down) else {
                return false;
            };
            ui(|ui| ui.focus = Some(next));
        }
        Key::Named(Named::Enter) => match focus {
            Some(Target::Field | Target::Join) => submit(network),
            Some(other) => act(other, network, motion),
            None => {}
        },
        Key::Named(Named::Space) => {
            if let Some(focus) = focus {
                act(focus, network, motion);
            }
        }
        _ => return false,
    }
    true
}
