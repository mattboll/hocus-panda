//! A phone's or tablet's popover (bar.md §9, "The phone", #957, variant
//! B): its name and its kind; its battery as the battery's popover draws
//! it (3.7), without an estimate; "Faire sonner", "Envoyer un fichier…"
//! (the desktop portal's chooser) and "Envoyer le presse-papiers", each
//! there only while its plugin is loaded; a transfer under way, its ✕
//! stopping it; "Oublier ce téléphone", confirmed in place. Out of reach:
//! "Hors de portée." and "Oublier" alone.
//!
//! Built with the shell's widget kit ([`crate::ui`]), as Bluetooth's:
//! what changes draws this surface alone.
//!
//! The keyboard: `Tab` goes through the rows that act, `Enter` or `Space`
//! acts, `Esc` closes.

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_a11y::Role;
use hocus_link::CONNECTIONS_NAMESPACE;
use hocus_words::shell::phone as txt;
use iced::{
    Color, Element, Length, Padding, Point, Rectangle, Size,
    alignment::Vertical,
    widget::{Space, button, column, container, row, space, text::LineHeight},
    window,
};

use super::{
    Item,
    kit::{self, ASIDE, ICON, ICON_GAP, NOTE_HEIGHT, ROW, ROW_GAP, ROW_RADIUS, ROW_TEXT},
};
use crate::{
    clock::time::Lang,
    levels::{
        draw::{
            CRITICAL, DIM, FAINT, LINE, LOW, MANROPE, MANROPE_BOLD, Sign, TEXT, band_colour,
            battery_sign, stroke,
        },
        model::Band,
    },
    phone::{self, Ask, Device, Transfer},
    theme::Ink,
    ui::{
        self, Around, control,
        parts::{self, drawn, gap, moving},
        style::{self, said},
    },
};

type El<'a> = Element<'a, Msg>;

// --- The mockup's sizes, at ×1 ------------------------------------------------

const GAP: f32 = 6.0;
/// The divider, 1 px with 4 px around it.
const RULE: f32 = 4.0;
const RULE_HEIGHT: f32 = 2.0 * RULE + 1.0;
/// The battery: 42 × 24, its level 26 px over its line 13.5 px.
const BIG: Size = Size::new(42.0, 24.0);
const BIG_GAP: f32 = 12.0;
const LEVEL: f32 = 26.0;
const LEVEL_LINE: f32 = 1.1;
const STATE: f32 = 13.5;
const BLOCK: f32 = 2.0 + LEVEL * LEVEL_LINE + 1.0 + STATE * LINE + 4.0;
/// A transfer under its row: 34 px in; its name 13 px and its 22 px ✕,
/// its 4 px bar, its bytes 12 px.
const TRANSFER_IN: f32 = 34.0;
const TRANSFER_NAME: f32 = 13.0;
const TRANSFER_CLOSE: f32 = 22.0;
const TRANSFER_BAR: f32 = 4.0;
const TRANSFER_BYTES: f32 = 12.0;
const TRANSFER_FAILED: f32 = 12.5;
const TRANSFER: f32 = TRANSFER_CLOSE + 3.0 + TRANSFER_BAR + 3.0 + TRANSFER_BYTES * LINE + 4.0;
const BAR_FROM: Ink = Ink::Dark(crate::levels::draw::rgb(0x5B8DEF));
const BAR_TO: Ink = Ink::Dark(crate::levels::draw::rgb(0x8FB3FF));
const BAR_TRACK: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
/// "Envoyé", 2 s.
const SENT: Ink = Ink::Dark(crate::levels::draw::rgb(0x6CC287));
const SENT_FOR: Duration = Duration::from_secs(2);
/// The bell's swing: three of 0.5 s.
const SWING: Duration = Duration::from_millis(1500);
/// "Oublier", amber.
const FORGET_FILL: Ink = Ink::Dark(Color::from_rgba8(232, 161, 58, 0.22));
const FORGET_HOVER: Ink = Ink::Dark(Color::from_rgba8(232, 161, 58, 0.32));
const FORGET_TEXT: Ink = Ink::Dark(crate::levels::draw::rgb(0xF5C46E));
const SMALL_BUTTON: f32 = 26.0;

// --- What the popover keeps while it is open -----------------------------------

/// What can hold the keyboard's ring, and be acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Ring,
    SendFile,
    SendClipboard,
    /// The transfer's ✕.
    Stop,
    Forget,
    /// The confirmation's "Oublier", and "Annuler".
    ForgetYes,
    ForgetNo,
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("phone:{self:?}")
    }
}

#[derive(Debug, Default)]
struct Ui {
    focus: Option<Target>,
    /// "Oublier" asks to be sure.
    confirming: bool,
    rang: Option<Instant>,
    sent: Option<Instant>,
    /// What it showed, as last said in the log.
    said: String,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// Device `key`'s popover opens.
pub fn opening(_key: u32) {
    ui(|ui| {
        *ui = Ui::default();
    });
}

/// It closes: a stopped transfer it said is let go.
pub fn closing(key: u32) {
    ui(|ui| {
        ui.focus = None;
        ui.confirming = false;
    });
    if let Some(row) = phone::row(key)
        && row.device.transfer.as_ref().is_some_and(|t| t.failed)
    {
        phone::ask(Ask::Settle(row.device.id));
    }
}

/// The last text copied, never a secret (the compositor never hands one):
/// what "Envoyer le presse-papiers" sends.
fn last_text() -> Option<String> {
    let now = crate::notifications::wall_ms();
    crate::clipboard::with(|h| {
        h.shown(now)
            .into_iter()
            .filter(|i| i.kind != crate::clipboard::Kind::Image && !i.text.is_empty())
            .max_by_key(|i| i.at)
            .map(|i| i.text.to_string())
    })
}

// --- Its height ----------------------------------------------------------------------

/// What it shows, from the top.
#[derive(Debug, Clone, PartialEq)]
struct Layout {
    reachable: bool,
    battery: bool,
    ring: bool,
    file: bool,
    clipboard: bool,
    transfer: bool,
}

impl Layout {
    fn of(device: &Device) -> Self {
        Self {
            reachable: device.reachable,
            battery: device.reachable && device.battery.is_some(),
            ring: device.reachable && device.ring,
            file: device.reachable && device.share && phone::portal(),
            clipboard: device.reachable && device.share,
            transfer: device.reachable && device.transfer.is_some(),
        }
    }

    fn actions(&self) -> usize {
        [self.ring, self.file, self.clipboard]
            .iter()
            .filter(|b| **b)
            .count()
    }

    fn height(&self) -> u32 {
        let mut y = kit::HEADER_TOP + kit::HEADER;
        if !self.reachable {
            y += GAP + NOTE_HEIGHT;
        }
        if self.battery {
            y += GAP + BLOCK;
        }
        let actions = self.actions();
        if actions > 0 {
            y += GAP + RULE_HEIGHT;
            y += GAP + actions as f32 * ROW + (actions - 1) as f32 * ROW_GAP;
        }
        if self.transfer {
            y += ROW_GAP + TRANSFER;
        }
        y += GAP + RULE_HEIGHT + GAP + ROW;
        kit::height(y)
    }
}

/// The popover's height for what it shows now.
pub fn height(key: u32) -> u32 {
    phone::row(key).map_or(120, |row| Layout::of(&row.device).height())
}

// --- The view ------------------------------------------------------------------------

/// Device `key`'s popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    /// Its surface: it reacts only while open, not while it fades out.
    pub id: window::Id,
    pub key: u32,
    pub lang: Lang,
    /// On the compositor's glass (else opaque).
    pub glass: bool,
    /// The bell swings.
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
    let content = match phone::row(panel.key) {
        Some(row) => ui(|ui| content(panel, &row.device, ui)),
        None => Space::new().into(),
    };
    let key = panel.key;
    Around::new(panel.id, content, move |m| handle(key, m), Msg::Stale)
        .press(Msg::Pressed)
        .on_glass(panel.glass)
}

fn content<'a>(panel: &Panel, device: &Device, ui: &mut Ui) -> El<'a> {
    let lang = panel.lang;
    let layout = Layout::of(device);
    let now = Instant::now();
    say(device, &layout, ui, lang);
    let block = |content: El<'a>| parts::block(GAP, Some(content));
    let aside = said(
        phone::kind_title(device.kind, lang).to_owned(),
        ASIDE,
        MANROPE,
        FAINT.get(),
    )
    .into();
    let name = kit::elide(&device.name, 170.0, |t| {
        kit::measure(t, kit::TITLE, MANROPE_BOLD)
    });
    let mut all: Vec<El<'a>> = vec![parts::header(&name, Some(aside))];
    if !layout.reachable {
        all.push(block(parts::note(txt::OUT_OF_REACH_NOTE.get(lang))));
    }
    if let Some(battery) = device.battery.filter(|_| layout.battery) {
        all.push(block(battery_block(lang, battery)));
    }
    if layout.actions() > 0 {
        all.push(block(parts::rule()));
        let mut rows = column![].spacing(ROW_GAP);
        if layout.ring {
            let rang = ui.rang;
            let motion = panel.motion;
            let bell = moving(
                ICON,
                ICON,
                move |f, s, at| {
                    let swing = match rang {
                        Some(rang) if motion => swing_at(at.saturating_duration_since(rang)),
                        _ => 0.0,
                    };
                    bell_icon(f, Point::ORIGIN, s.width, swing, TEXT.get());
                },
                move |at| motion && rang.is_some_and(|r| at.saturating_duration_since(r) < SWING),
            );
            rows = rows.push(action_row(
                bell,
                said(
                    txt::RING.get(lang).to_owned(),
                    ROW_TEXT,
                    MANROPE,
                    TEXT.get(),
                )
                .into(),
                Target::Ring,
                txt::RING.get(lang),
                ui.focus,
                true,
            ));
        }
        if layout.file {
            rows = rows.push(action_row(
                drawn(ICON, ICON, |f, s| {
                    file_icon(f, Point::ORIGIN, s.width, TEXT.get())
                }),
                said(
                    txt::SEND_FILE.get(lang).to_owned(),
                    ROW_TEXT,
                    MANROPE,
                    TEXT.get(),
                )
                .into(),
                Target::SendFile,
                txt::SEND_FILE.get(lang),
                ui.focus,
                true,
            ));
        }
        if layout.clipboard {
            let sent = ui.sent.is_some_and(|s| now.duration_since(s) < SENT_FOR);
            let has = last_text().is_some();
            let (words, ink) = if sent {
                (txt::SENT.get(lang), SENT.get())
            } else if has {
                (txt::SEND_CLIPBOARD.get(lang), TEXT.get())
            } else {
                (txt::SEND_CLIPBOARD.get(lang), FAINT.get())
            };
            rows = rows.push(action_row(
                drawn(ICON, ICON, move |f, s| {
                    clipboard_icon(f, Point::ORIGIN, s.width, ink);
                }),
                said(words.to_owned(), ROW_TEXT, MANROPE, ink).into(),
                Target::SendClipboard,
                words,
                ui.focus,
                has && !sent,
            ));
        }
        all.push(block(rows.into()));
        if let Some(transfer) = device.transfer.as_ref().filter(|_| layout.transfer) {
            all.push(parts::block(
                ROW_GAP,
                Some(transfer_block(lang, transfer, ui.focus)),
            ));
        }
    }
    all.push(block(parts::rule()));
    all.push(block(forget_row(lang, device, ui)));
    parts::content(all)
}

/// What it shows, said in the log as it changes: the tests read it.
fn say(device: &Device, layout: &Layout, ui: &mut Ui, lang: Lang) {
    let mut rows = Vec::new();
    if !layout.reachable {
        rows.push(txt::OUT_OF_REACH_NOTE.get(lang).to_owned());
    }
    if let Some(b) = device.battery.filter(|_| layout.battery) {
        rows.push(format!("{} %", b.charge));
        rows.push(battery_line(lang, b).to_owned());
    }
    if layout.ring {
        rows.push(txt::RING.get(lang).to_owned());
    }
    if layout.file {
        rows.push(txt::SEND_FILE.get(lang).to_owned());
    }
    if layout.clipboard {
        let sent = ui.sent.is_some_and(|s| s.elapsed() < SENT_FOR);
        rows.push(
            if sent { txt::SENT } else { txt::SEND_CLIPBOARD }
                .get(lang)
                .to_owned(),
        );
    }
    if let Some(t) = device.transfer.as_ref().filter(|_| layout.transfer) {
        rows.push(if t.failed {
            txt::SENDING_STOPPED.get(lang).to_owned()
        } else {
            format!(
                "{} {:.0} %",
                phone::transfer_name(t, lang),
                t.fraction() * 100.0
            )
        });
    }
    if ui.confirming {
        rows.push(txt::forget_it(lang, &device.name));
    } else {
        rows.push(forget_words(lang, device).to_owned());
    }
    let said = format!("{}: {}", device.name, rows.join(" | "));
    if said != ui.said {
        tracing::debug!(rows = %said, "the phone's popover shows it");
        ui.said = said;
    }
}

/// "Sur batterie", "En charge", "Branché, plein".
fn battery_line(lang: Lang, battery: phone::Battery) -> &'static str {
    match (battery.charging, battery.charge) {
        (true, 100) => txt::PLUGGED_FULL,
        (true, _) => txt::CHARGING,
        (false, _) => txt::ON_BATTERY,
    }
    .get(lang)
}

/// The level's band: amber at 20 % or less, peach at 10 % or less, unless
/// charging.
fn band(battery: phone::Battery) -> Band {
    if battery.charging {
        return Band::Ok;
    }
    match battery.charge {
        0..=10 => Band::Critical,
        11..=20 => Band::Low,
        _ => Band::Ok,
    }
}

/// The large battery, its level and its line.
fn battery_block<'a>(lang: Lang, battery: phone::Battery) -> El<'a> {
    let band = band(battery);
    let (level, line) = match band {
        Band::Ok => (TEXT.get(), DIM.get()),
        Band::Low => (LOW.1, LOW.1),
        Band::Critical => (CRITICAL.1, CRITICAL.1),
    };
    let sign = Sign {
        percent: f32::from(battery.charge),
        fill: band_colour(band),
        bolt: battery.charging,
        outline: 1.4,
    };
    let big = drawn(BIG.width, BIG.height, move |f, s| {
        battery_sign(f, Rectangle::new(Point::ORIGIN, s), sign, 1.0);
    });
    let words = battery_line(lang, battery);
    let texts = column![
        said(
            format!("{}\u{a0}%", battery.charge),
            LEVEL,
            MANROPE_BOLD,
            level
        )
        .line_height(LineHeight::Relative(LEVEL_LINE)),
        Space::new().height(1.0),
        said(words.to_owned(), STATE, MANROPE, line),
    ];
    let block = container(row![big, gap(BIG_GAP), texts].align_y(Vertical::Center))
        .height(BLOCK)
        .padding(Padding {
            top: 2.0,
            bottom: 4.0,
            ..Padding::ZERO
        });
    control(
        parts::inset(block),
        "phone:Battery",
        Role::ProgressBar,
        txt::battery_of(lang, battery.charge),
    )
    .node(|n| {
        n.value(format!("{}\u{a0}%", battery.charge))
            .range(f64::from(battery.charge), 0.0, 100.0, None)
            .describe(words)
    })
    .into()
}

/// A row that acts: its icon, its words, lit under the pointer.
fn action_row<'a>(
    icon: El<'a>,
    words: El<'a>,
    target: Target,
    name: &str,
    focus: Option<Target>,
    acts: bool,
) -> El<'a> {
    let line = row![icon, gap(ICON_GAP), words, space::horizontal()].align_y(Vertical::Center);
    let pressed = parts::row_button(line, ROW, acts.then_some(Msg::Act(target)));
    let mut control = control(pressed, target.id(), Role::Button, name);
    if acts {
        control = control
            .focus(focus == Some(target), Msg::Focus(target), ROW_RADIUS)
            .activate(Msg::Act(target));
    }
    control.into()
}

/// A transfer: its name and its ✕, its bar, its bytes; stopped, "Envoi
/// interrompu".
fn transfer_block<'a>(lang: Lang, transfer: &Transfer, focus: Option<Target>) -> El<'a> {
    let name = kit::elide(
        &phone::transfer_name(transfer, lang),
        kit::RIGHT - kit::LEFT - TRANSFER_IN - TRANSFER_CLOSE - 8.0,
        |t| kit::measure(t, TRANSFER_NAME, MANROPE),
    );
    let target = Target::Stop;
    let close = button(drawn(TRANSFER_CLOSE, TRANSFER_CLOSE, |f, s| {
        cross(f, s, DIM.get());
    }))
    .padding(0)
    .style(|_, status| {
        if style::lit(status) {
            style::hovered(style::ROW_HOVER.get(), 6.0)
        } else {
            style::clear()
        }
    })
    .on_press(Msg::Act(target));
    let close = control(
        close,
        target.id(),
        Role::Button,
        txt::STOP_SENDING.get(lang),
    )
    .focus(focus == Some(target), Msg::Focus(target), 6.0)
    .activate(Msg::Act(target));
    let head = row![
        said(name.clone(), TRANSFER_NAME, MANROPE, TEXT.get()),
        space::horizontal(),
    ]
    .align_y(Vertical::Center)
    .height(TRANSFER_CLOSE);
    let head: El<'a> = if transfer.failed {
        head.into()
    } else {
        head.push(close).into()
    };
    let fraction = transfer.fraction();
    let bar = drawn(
        kit::RIGHT - kit::LEFT - TRANSFER_IN,
        TRANSFER_BAR,
        move |f, s| {
            let track = iced::widget::canvas::Path::rounded_rectangle(
                Point::ORIGIN,
                s,
                (TRANSFER_BAR / 2.0).into(),
            );
            f.fill(&track, BAR_TRACK.get());
            let done = s.width * fraction;
            if done > 0.0 {
                let fill = iced::widget::canvas::gradient::Linear::new(
                    Point::ORIGIN,
                    Point::new(done, 0.0),
                )
                .add_stop(0.0, BAR_FROM.get())
                .add_stop(1.0, BAR_TO.get());
                f.fill(
                    &iced::widget::canvas::Path::rounded_rectangle(
                        Point::ORIGIN,
                        Size::new(done, s.height),
                        (TRANSFER_BAR / 2.0).into(),
                    ),
                    iced::widget::canvas::Fill::from(fill),
                );
            }
        },
    );
    let under: El<'a> = if transfer.failed {
        said(
            txt::SENDING_STOPPED.get(lang).to_owned(),
            TRANSFER_FAILED,
            MANROPE,
            kit::PEACH.get(),
        )
        .into()
    } else {
        said(
            phone::done_of(lang, transfer.processed, transfer.total),
            TRANSFER_BYTES,
            MANROPE,
            FAINT.get(),
        )
        .into()
    };
    let block = column![
        head,
        Space::new().height(3.0),
        bar,
        Space::new().height(3.0),
        under,
    ]
    .width(Length::Fill);
    let said_as = if transfer.failed {
        format!("{name}, {}", txt::SENDING_STOPPED.get(lang))
    } else {
        format!(
            "{name}, {}",
            phone::done_of(lang, transfer.processed, transfer.total)
        )
    };
    ui::line(
        container(block).height(TRANSFER).padding(Padding {
            left: TRANSFER_IN,
            right: kit::ROW_PAD,
            ..Padding::ZERO
        }),
        said_as,
    )
    .into()
}

/// "Oublier ce téléphone" ("cet appareil" for a tablet).
fn forget_words(lang: Lang, device: &Device) -> &'static str {
    match device.kind {
        phone::Kind::Phone => txt::FORGET_PHONE,
        phone::Kind::Tablet => txt::FORGET_DEVICE,
    }
    .get(lang)
}

/// "Oublier ce téléphone", or, asked, "Oublier Pixel 8 ?" with
/// "Oublier" and "Annuler".
fn forget_row<'a>(lang: Lang, device: &Device, ui: &Ui) -> El<'a> {
    if !ui.confirming {
        let words = forget_words(lang, device);
        return action_row(
            drawn(ICON, ICON, |f, s| {
                unlink_icon(f, Point::ORIGIN, s.width, FAINT.get())
            }),
            said(words.to_owned(), ROW_TEXT, MANROPE, FAINT.get()).into(),
            Target::Forget,
            words,
            ui.focus,
            true,
        );
    }
    let small = |words: &str, target: Target, amber: bool| -> El<'a> {
        let pressed = button(
            container(said(
                words.to_owned(),
                kit::STATUS,
                kit::MANROPE_SEMIBOLD,
                if amber { FORGET_TEXT.get() } else { TEXT.get() },
            ))
            .center_y(Length::Fill),
        )
        .padding([0.0, kit::SMALL_PAD])
        .height(SMALL_BUTTON)
        .style(move |_, status| {
            let fill = match (amber, style::lit(status)) {
                (true, false) => FORGET_FILL.get(),
                (true, true) => FORGET_HOVER.get(),
                (false, false) => kit::FILL.get(),
                (false, true) => kit::FILL_HOVER.get(),
            };
            style::filled(fill, kit::SMALL_RADIUS, TEXT.get())
        })
        .on_press(Msg::Act(target));
        control(pressed, target.id(), Role::Button, words)
            .focus(
                ui.focus == Some(target),
                Msg::Focus(target),
                kit::SMALL_RADIUS,
            )
            .activate(Msg::Act(target))
            .into()
    };
    let question = txt::forget_it(lang, &device.name);
    let room = kit::RIGHT - kit::LEFT - 150.0;
    let question = kit::elide(&question, room, |t| kit::measure(t, ROW_TEXT, MANROPE));
    let line = row![
        said(question, ROW_TEXT, MANROPE, TEXT.get()),
        space::horizontal(),
        small(txt::FORGET.get(lang), Target::ForgetYes, true),
        gap(6.0),
        small(txt::CANCEL.get(lang), Target::ForgetNo, false),
    ]
    .align_y(Vertical::Center);
    container(line)
        .height(ROW)
        .padding([0.0, kit::ROW_PAD])
        .center_y(ROW)
        .into()
}

// --- What a message does -------------------------------------------------------

fn handle(key: u32, message: Msg) {
    match message {
        Msg::Stale => {}
        Msg::Focus(target) => ui(|ui| ui.focus = Some(target)),
        Msg::Pressed => ui(|ui| ui.focus = None),
        Msg::Act(target) => act(key, target),
    }
    super::refit(Item::Phone(key));
}

fn act(key: u32, target: Target) {
    let Some(row) = phone::row(key) else {
        return;
    };
    let id = row.device.id.clone();
    match target {
        Target::Ring => {
            tracing::debug!("a phone asked to ring");
            ui(|ui| ui.rang = Some(Instant::now()));
            phone::ask(Ask::Ring(id));
        }
        Target::SendFile => {
            tracing::debug!("a file to send to a phone, chosen");
            let title = txt::send_to(Lang::of_messages(), &row.device.name);
            // The popover closes as the chooser opens.
            super::event(super::Event::Chosen);
            let chose = std::thread::Builder::new()
                .name("phone: file chooser".to_owned())
                .spawn(move || {
                    match crate::capture::portal::open_files(&title, CONNECTIONS_NAMESPACE) {
                        crate::capture::portal::Chosen::Paths(paths) => phone::send(&id, &paths),
                        other => tracing::debug!(?other, "nothing sent to the phone"),
                    }
                });
            if let Err(err) = chose {
                tracing::warn!(%err, "no file chooser");
            }
        }
        Target::SendClipboard => {
            let Some(text) = last_text() else {
                return;
            };
            tracing::debug!("the clipboard sent to a phone");
            ui(|ui| ui.sent = Some(Instant::now()));
            phone::ask(Ask::Text { device: id, text });
            // "Envoyé" for 2 s, then the row again.
            let _ = std::thread::Builder::new()
                .name("phone: sent".to_owned())
                .spawn(move || {
                    std::thread::sleep(SENT_FOR);
                    super::changed(Item::Phone(key));
                });
        }
        Target::Stop => {
            if let Some(transfer) = &row.device.transfer {
                tracing::debug!("a transfer to a phone stopped");
                phone::ask(Ask::Cancel(transfer.view));
            }
        }
        Target::Forget => ui(|ui| {
            ui.confirming = true;
            ui.focus = ui.focus.is_some().then_some(Target::ForgetNo);
        }),
        Target::ForgetNo => ui(|ui| {
            ui.confirming = false;
            ui.focus = ui.focus.is_some().then_some(Target::Forget);
        }),
        Target::ForgetYes => {
            tracing::debug!("a phone forgotten");
            phone::ask(Ask::Forget(id));
            super::event(super::Event::Chosen);
        }
    }
}

// --- Icons, from the mockup's SVGs (a 24-unit box, 2 units of stroke) ----------

/// Where the bell is in its swing at `since` its ring: three 0.5 s swings,
/// radians.
fn swing_at(since: Duration) -> f32 {
    if since >= SWING {
        return 0.0;
    }
    let t = since.as_secs_f32();
    let fade = 1.0 - t / SWING.as_secs_f32();
    0.35 * fade * (t * std::f32::consts::TAU / 0.5).sin()
}

/// A bell, `size` px at `at`, swung by `swing` radians about its top.
fn bell_icon(f: &mut iced::widget::canvas::Frame, at: Point, size: f32, swing: f32, colour: Color) {
    let s = size / 24.0;
    f.with_save(|f| {
        f.translate(iced::Vector::new(at.x + 12.0 * s, at.y + 3.0 * s));
        f.rotate(swing);
        let p = |x: f32, y: f32| Point::new((x - 12.0) * s, (y - 3.0) * s);
        let body = iced::widget::canvas::Path::new(|b| {
            b.move_to(p(6.0, 16.0));
            b.line_to(p(6.0, 11.0));
            b.bezier_curve_to(p(6.0, 7.0), p(8.5, 4.5), p(12.0, 4.5));
            b.bezier_curve_to(p(15.5, 4.5), p(18.0, 7.0), p(18.0, 11.0));
            b.line_to(p(18.0, 16.0));
            b.line_to(p(20.0, 18.0));
            b.line_to(p(4.0, 18.0));
            b.close();
        });
        f.stroke(&body, stroke(2.0 * s, colour));
        f.stroke(
            &iced::widget::canvas::Path::line(p(10.0, 21.0), p(14.0, 21.0)),
            stroke(2.0 * s, colour),
        );
    });
}

/// A sheet with its corner folded, `size` px at `at`.
fn file_icon(f: &mut iced::widget::canvas::Frame, at: Point, size: f32, colour: Color) {
    let s = size / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let sheet = iced::widget::canvas::Path::new(|b| {
        b.move_to(p(14.0, 3.0));
        b.line_to(p(6.0, 3.0));
        b.line_to(p(6.0, 21.0));
        b.line_to(p(18.0, 21.0));
        b.line_to(p(18.0, 7.0));
        b.close();
        b.move_to(p(14.0, 3.0));
        b.line_to(p(14.0, 7.0));
        b.line_to(p(18.0, 7.0));
    });
    f.stroke(&sheet, stroke(2.0 * s, colour));
}

/// A clipboard, `size` px at `at`.
fn clipboard_icon(f: &mut iced::widget::canvas::Frame, at: Point, size: f32, colour: Color) {
    let s = size / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, colour);
    f.stroke(
        &iced::widget::canvas::Path::rounded_rectangle(
            p(5.0, 4.0),
            Size::new(14.0 * s, 17.0 * s),
            (2.0 * s).into(),
        ),
        line,
    );
    f.stroke(
        &iced::widget::canvas::Path::rounded_rectangle(
            p(9.0, 2.0),
            Size::new(6.0 * s, 4.0 * s),
            (1.0 * s).into(),
        ),
        line,
    );
}

/// Two links apart: forgetting.
fn unlink_icon(f: &mut iced::widget::canvas::Frame, at: Point, size: f32, colour: Color) {
    let s = size / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, colour);
    for (from, to) in [
        ((9.0, 15.0), (6.5, 17.5)),
        ((15.0, 9.0), (17.5, 6.5)),
        ((4.0, 8.0), (7.0, 8.0)),
        ((8.0, 4.0), (8.0, 7.0)),
        ((17.0, 20.0), (17.0, 17.0)),
        ((20.0, 16.0), (17.0, 16.0)),
    ] {
        f.stroke(
            &iced::widget::canvas::Path::line(p(from.0, from.1), p(to.0, to.1)),
            line,
        );
    }
    for (x, y) in [(5.0, 19.0), (19.0, 5.0)] {
        f.stroke(&iced::widget::canvas::Path::circle(p(x, y), 3.0 * s), line);
    }
}

/// The transfer's ✕, in a `size` square.
fn cross(f: &mut iced::widget::canvas::Frame, size: Size, colour: Color) {
    let c = Point::new(size.width / 2.0, size.height / 2.0);
    let r = 4.5;
    let line = stroke(1.8, colour);
    f.stroke(
        &iced::widget::canvas::Path::line(
            Point::new(c.x - r, c.y - r),
            Point::new(c.x + r, c.y + r),
        ),
        line,
    );
    f.stroke(
        &iced::widget::canvas::Path::line(
            Point::new(c.x + r, c.y - r),
            Point::new(c.x - r, c.y + r),
        ),
        line,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone::{Battery, Kind};

    fn device(reachable: bool) -> Device {
        Device {
            id: "a1".to_owned(),
            name: "Pixel 8".to_owned(),
            kind: Kind::Phone,
            reachable,
            battery: reachable.then_some(Battery {
                charge: 82,
                charging: false,
            }),
            ring: reachable,
            share: reachable,
            came: 1,
            transfer: None,
        }
    }

    #[test]
    fn out_of_reach_it_says_so_and_offers_to_forget_only() {
        let away = Layout::of(&device(false));
        assert_eq!(away.actions(), 0);
        assert!(!away.battery && !away.transfer);
        let near = Layout::of(&device(true));
        assert!(near.battery && near.ring && near.clipboard);
        assert!(near.height() > away.height());
    }

    #[test]
    fn the_battery_says_charging_full_and_its_band() {
        let fr = Lang::French;
        let b = |charge, charging| Battery { charge, charging };
        assert_eq!(battery_line(fr, b(67, true)), "En charge");
        assert_eq!(battery_line(fr, b(100, true)), "Branché, plein");
        assert_eq!(battery_line(fr, b(40, false)), "Sur batterie");
        assert_eq!(band(b(20, false)), Band::Low);
        assert_eq!(band(b(10, false)), Band::Critical);
        assert_eq!(band(b(10, true)), Band::Ok);
    }

    #[test]
    fn the_bell_swings_three_times_then_rests() {
        assert!(swing_at(Duration::from_millis(125)).abs() > 0.1);
        assert!(swing_at(SWING).abs() < f32::EPSILON);
    }
}
