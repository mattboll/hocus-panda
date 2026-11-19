//! The weather's popover (#814, bar.md "Weather"), §9's mechanics: a click
//! on Today's weather (its header, the rain's line, tomorrow's), a hover
//! or a click on the mini form's item. 320 px wide, as the secretary's
//! briefing, 14 px in, its parts 14 px apart:
//!
//! - the place's name (15 px 700) and the date (12.5 px, `text.dim`);
//! - now: the icon (46 px), the temperature (40 px), what the weather is
//!   (14 px 700), "ressenti 6° · min 5° max 10°" and the wind (12.5 px,
//!   `text.dim`);
//! - "PLUIE, 3 PROCHAINES HEURES" and on its right when it comes ("pluie
//!   à 14 h 15", "jusqu'à 15 h 45", "aucune", in the rain's colour): 12
//!   bars by quarter hour (Open-Meteo's `minutely_15`), 34 px tall, the dry
//!   ones grey, "maint. +1 h +2 h +3 h" under them;
//! - "12 PROCHAINES HEURES": the temperature's curve (`#E8B44E`, a golden
//!   wash under it), every third hour its value and its hour, the chance
//!   of rain as bars at its foot (`#8FB3FF`, stronger from 50 %);
//! - "7 JOURS": a row a day, 32 px: its name ("Auj.", "Demain", "ven.
//!   21"), its icon, its rain if any ("6 mm"), its low, the range bar
//!   (from the week's lowest to its highest, cold to warm), its high;
//!   tomorrow lit;
//! - "Open-Meteo · à 13 h 45", the forecast's time, and "Réglages de la
//!   météo…", which opens Settings › Weather.
//!
//! Built with the shell's widget kit; the bars, the curve and the range
//! bars are drawn on canvases ([`crate::ui::parts::drawn`]).

use std::sync::{Mutex, PoisonError};

use hocus_a11y::Role;
use hocus_weather::{Day, Hour, Units};
use iced::{
    Color, Element, Length, Padding, Point, Rectangle, Size,
    alignment::{Horizontal, Vertical},
    widget::{
        Space, canvas, column, container, row, space,
        text::Alignment,
    },
    window,
};

use super::{Item, kit};
use crate::{
    clock::time::Lang,
    levels::draw::{DIM, FAINT, MANROPE, MANROPE_BOLD, TEXT},
    theme::Ink,
    ui::{
        Around, control,
        parts::{self, drawn},
        style,
    },
    weather::{self, Words, icon},
};

type El<'a> = Element<'a, Msg>;

/// Where its popover was opened from: Today's header, the rain's line,
/// tomorrow's, the mini form's item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Spot {
    Head,
    Rain,
    Tomorrow,
    Mini,
}

pub const WIDTH: u32 = 320;
const BORDER: f32 = 1.0;
const PAD: f32 = 14.0;
const GAP: f32 = 14.0;
/// The content's width.
const INNER: f32 = WIDTH as f32 - 2.0 * (BORDER + PAD);
const HEAD: f32 = 20.0;
const NOW: f32 = 54.0;
const NOW_ICON: f32 = 46.0;
const BIG: f32 = 38.0;
const LABEL: f32 = 18.0;
const LABEL_GAP: f32 = 4.0;
const BARS: f32 = 34.0;
const AXIS: f32 = 14.0;
const CURVE: f32 = 92.0;
const DAY: f32 = 32.0;
/// A day's range bar: what its row's cells and gaps leave of it.
const RANGE: f32 = INNER - 8.0 - (62.0 + 22.0 + 40.0 + 26.0 + 26.0) - 5.0 * 6.0;
const DAYS: usize = 7;
const SOURCE: f32 = 18.0;
/// The Settings' link: its divider (4 + 1 + 4) and its row.
const LINK: f32 = 45.0;

const RAIN_DRY: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const RAIN_DEEP: Color = Color::from_rgb8(0x5B, 0x8D, 0xEF);
const CURVE_LINE: Color = Color::from_rgb8(0xE8, 0xB4, 0x4E);
const COLD: Color = Color::from_rgb8(0x8F, 0xD3, 0xFF);
const TRACK: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
const TOMORROW: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.05));
const DIVIDER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));

/// Its height: every part's, fixed.
pub fn height() -> u32 {
    let rain = LABEL + LABEL_GAP + BARS + LABEL_GAP + AXIS;
    let hours = LABEL + LABEL_GAP + CURVE;
    let week = LABEL + LABEL_GAP + DAY * DAYS as f32;
    let parts = HEAD + NOW + rain + hours + week + SOURCE + LINK;
    (2.0 * (BORDER + PAD) + parts + 4.0 * GAP + 10.0).ceil() as u32
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Settings,
}

impl Target {
    fn id(self) -> String {
        format!("weather:{self:?}")
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Act(Target),
    Focus(Target),
    Pressed,
    Stale,
}

#[derive(Debug, Default)]
struct Ui {
    focus: Option<Target>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

pub fn opening() {
    ui(|ui| ui.focus = None);
}

/// The forecast changed: an open popover shows it.
pub fn changed() {
    for spot in [Spot::Head, Spot::Rain, Spot::Tomorrow, Spot::Mini] {
        super::changed(Item::Weather(spot));
    }
}

#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub glass: bool,
}

pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let lang = Lang::of_messages();
    Around::new(panel.id, build(lang), handle, Msg::Stale)
        .rebuild(move || build(lang))
        .press(Msg::Pressed)
        .on_glass(panel.glass)
}

fn said<'a>(text: String, size: f32, font: iced::Font, colour: Color) -> El<'a> {
    style::said(text, size, font, colour).into()
}

/// A part's label, its capitals spaced, `aside` on its right.
fn label<'a>(word: &str, aside: Option<El<'a>>) -> El<'a> {
    parts::label(word, LABEL, aside)
}

/// `content` between the content's sides (the rows' and the labels' go
/// 6 px past them).
fn inset<'a>(content: impl Into<El<'a>>) -> El<'a> {
    parts::inset(content)
}

fn build<'a>(lang: Lang) -> El<'a> {
    let words = Words::new(lang);
    let now = crate::notifications::wall();
    let Some(shown) = weather::shown() else {
        return container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    };
    let forecast = shown.forecast.clone();
    let units = shown.units;
    let at = now.timestamp();
    let focus = ui(|ui| ui.focus);
    let mut parts: Vec<El<'a>> = Vec::new();
    // The place and the date.
    let date = crate::clock::time::date(lang, now.date(), hocus_link::locale::DateFormat::Short);
    parts.push(inset(
        row![
            said(shown.place.clone(), 15.0, MANROPE_BOLD, TEXT.get()),
            space::horizontal(),
            said(date, 12.5, MANROPE, DIM.get()),
        ]
        .align_y(Vertical::Center)
        .height(HEAD),
    ));
    // Now.
    let current = forecast.now(at);
    let today = forecast.day(now.date());
    if let Some(m) = current {
        let condition = words.condition(m.code, m.day);
        let mut feels = txt_feels(lang, &weather::degrees(m.feels, units));
        if let Some(day) = today {
            feels = format!(
                "{feels} · {}",
                hocus_words::shell::weather::low_high(
                    lang,
                    hocus_words::shell::weather::LowHigh {
                        low: &weather::degrees(day.min, units),
                        high: &weather::degrees(day.max, units),
                    }
                )
            );
        }
        let wind = hocus_words::shell::weather::wind(lang, units.wind(m.wind));
        let temp = weather::degrees(m.temperature, units);
        parts.push(inset(
            control(
                row![
                    icon::widget(m.icon(), NOW_ICON),
                    said(temp.clone(), BIG, MANROPE, TEXT.get()),
                    column![
                        said(condition.to_owned(), 14.0, MANROPE_BOLD, TEXT.get()),
                        said(feels.clone(), 12.5, MANROPE, DIM.get()),
                        said(wind.clone(), 12.5, MANROPE, DIM.get()),
                    ]
                    .spacing(0),
                ]
                .spacing(12)
                .align_y(Vertical::Center)
                .height(NOW),
                "weather:now",
                Role::Label,
                format!("{}, {feels}, {wind}", words.said_now(&temp, condition)),
            ),
        ));
    }
    // The rain of the next 3 hours.
    let quarters = forecast.next_quarters(at, 12);
    let rain = forecast.rain_today(&now);
    let rain_said = match rain {
        Some(r) if quarters.iter().any(|mm| *mm >= hocus_weather::WET) => match (r.start, r.end) {
            (Some(start), _) => hocus_words::shell::weather::rain_at(lang, words.time(start, &now)),
            (None, Some(end)) => hocus_words::shell::weather::until(lang, words.time(end, &now)),
            (None, None) => words.raining().to_owned(),
        },
        _ => hocus_words::shell::weather::NO_RAIN.get(lang).to_owned(),
    };
    parts.push(
        column![
            label(
                hocus_words::shell::weather::RAIN_NEXT_3_HOURS.get(lang),
                Some(said(
                    rain_said,
                    11.5,
                    kit::MANROPE_SEMIBOLD,
                    icon::RAIN.get()
                )),
            ),
            Space::new().height(LABEL_GAP),
            inset(rain_bars(quarters)),
            Space::new().height(LABEL_GAP),
            inset(axis(words)),
        ]
        .into(),
    );
    // The next 12 hours.
    let hours = forecast.next_hours(at, 12);
    let zone = now.time_zone().clone();
    parts.push(
        column![
            label(hocus_words::shell::weather::NEXT_12_HOURS.get(lang), None),
            Space::new().height(LABEL_GAP),
            inset(curve(hours, units, zone)),
        ]
        .into(),
    );
    // The week.
    let week = forecast.week(now.date(), DAYS);
    let mut days = column![
        label(hocus_words::shell::weather::SEVEN_DAYS.get(lang), None),
        Space::new().height(LABEL_GAP),
    ];
    let (low, high) = week.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(l, h), d| {
        (l.min(d.min), h.max(d.max))
    });
    for (i, day) in week.iter().enumerate() {
        days = days.push(inset(day_row(words, i, day, units, (low, high))));
    }
    parts.push(days.into());
    // The forecast's time, and the Settings.
    let fetched = hocus_words::shell::weather::source(lang, words.time(forecast.fetched, &now));
    let [rule, link] = parts::settings_link(
        hocus_words::shell::weather::SETTINGS.get(lang),
        &Target::Settings.id(),
        Msg::Act(Target::Settings),
        focus == Some(Target::Settings),
        Msg::Focus(Target::Settings),
    );
    let foot = column![
        inset(
            container(said(fetched, 12.0, MANROPE, FAINT.get()))
                .height(SOURCE)
                .align_y(Vertical::Center)
        ),
        rule,
        link,
    ];
    parts.push(foot.into());
    tracing::debug!(
        place = shown.place,
        days = week.len(),
        "the weather's popover shows it"
    );
    container(iced::widget::Column::with_children(parts).spacing(GAP))
        .padding(Padding {
            top: BORDER + PAD,
            right: BORDER + PAD - kit::ROW_OUT,
            bottom: BORDER + PAD,
            left: BORDER + PAD - kit::ROW_OUT,
        })
        .width(Length::Fill)
        .into()
}

fn txt_feels(lang: Lang, t: &str) -> String {
    hocus_words::shell::weather::feels_like(lang, t)
}

/// The 12 quarter hours' rain: bars from the bottom, the dry ones low and
/// grey.
fn rain_bars<'a>(quarters: Vec<f32>) -> El<'a> {
    drawn(INNER, BARS, move |f, size| {
        let most = quarters.iter().copied().fold(2.6f32, f32::max);
        let n = quarters.len().max(1) as f32;
        let gap = 2.0;
        let width = (size.width - gap * (n - 1.0)) / n;
        for (i, mm) in quarters.iter().enumerate() {
            let x = i as f32 * (width + gap);
            let (h, fill): (f32, canvas::Fill) = if *mm >= hocus_weather::WET {
                let h = (mm / most).max(0.14) * (size.height - 1.0);
                let gradient = canvas::gradient::Linear::new(
                    Point::new(0.0, size.height - 1.0 - h),
                    Point::new(0.0, size.height - 1.0),
                )
                .add_stop(0.0, icon::RAIN.get())
                .add_stop(1.0, RAIN_DEEP);
                (h, canvas::Fill::from(gradient))
            } else {
                (0.10 * (size.height - 1.0), canvas::Fill::from(RAIN_DRY.get()))
            };
            f.fill(
                &kit::rounded(
                    Rectangle::new(Point::new(x, size.height - 1.0 - h), Size::new(width, h)),
                    2.0_f32.min(h / 2.0),
                ),
                fill,
            );
        }
        f.fill_rectangle(
            Point::new(0.0, size.height - 1.0),
            Size::new(size.width, 1.0),
            DIVIDER.get(),
        );
    })
}

/// "maint. +1 h +2 h +3 h", spread under the bars.
fn axis<'a>(words: Words) -> El<'a> {
    let tick = |t: String| said(t, 11.0, MANROPE, FAINT.get());
    row![
        tick(hocus_words::shell::weather::NOW.get(words.lang).to_owned()),
        space::horizontal(),
        tick("+1 h".to_owned()),
        space::horizontal(),
        tick("+2 h".to_owned()),
        space::horizontal(),
        tick("+3 h".to_owned()),
    ]
    .height(AXIS)
    .align_y(Vertical::Center)
    .into()
}

/// The next hours' temperature as a curve, every third its value and its
/// hour, the chance of rain as bars at the foot.
fn curve<'a>(hours: Vec<Hour>, units: Units, zone: jiff::tz::TimeZone) -> El<'a> {
    drawn(INNER, CURVE, move |f, size| {
        if hours.is_empty() {
            return;
        }
        let temps: Vec<f32> = hours
            .iter()
            .map(|h| units.degrees(h.moment.temperature) as f32)
            .collect();
        let lo = temps.iter().copied().fold(f32::INFINITY, f32::min) - 1.0;
        let hi = temps.iter().copied().fold(f32::NEG_INFINITY, f32::max) + 1.0;
        let (top, foot) = (16.0, 62.0);
        let cell = size.width / hours.len() as f32;
        let y = |t: f32| top + (hi - t) / (hi - lo).max(1.0) * (foot - top - 10.0);
        let points: Vec<Point> = temps
            .iter()
            .enumerate()
            .map(|(i, t)| Point::new(cell * i as f32 + cell / 2.0, y(*t)))
            .collect();
        // The chance of rain, at the foot.
        let base = foot + 18.0;
        for (i, h) in hours.iter().enumerate() {
            if h.probability == 0 {
                continue;
            }
            let tall = f32::from(h.probability) / 100.0 * 16.0;
            let mut colour = icon::RAIN.get();
            colour.a = if h.probability >= 50 { 0.95 } else { 0.55 };
            f.fill(
                &kit::rounded(
                    Rectangle::new(
                        Point::new(cell * i as f32 + 3.0, base - tall),
                        Size::new(cell - 6.0, tall),
                    ),
                    2.0,
                ),
                colour,
            );
        }
        f.fill_rectangle(
            Point::new(0.0, base),
            Size::new(size.width, 1.0),
            TRACK.get(),
        );
        // The curve, smooth, and its wash.
        let path = |closed: bool| {
            canvas::Path::new(|b| {
                let Some((first, rest)) = points.split_first() else {
                    return;
                };
                b.move_to(*first);
                let mut last = *first;
                for p in rest {
                    let mid = (last.x + p.x) / 2.0;
                    b.bezier_curve_to(Point::new(mid, last.y), Point::new(mid, p.y), *p);
                    last = *p;
                }
                if closed {
                    b.line_to(Point::new(last.x, foot));
                    b.line_to(Point::new(first.x, foot));
                    b.close();
                }
            })
        };
        let mut wash_top = icon::SUN;
        wash_top.a = 0.28;
        let mut wash_foot = icon::SUN;
        wash_foot.a = 0.0;
        let wash = canvas::gradient::Linear::new(Point::new(0.0, top), Point::new(0.0, foot))
            .add_stop(0.0, wash_top)
            .add_stop(1.0, wash_foot);
        f.fill(&path(true), canvas::Fill::from(wash));
        f.stroke(
            &path(false),
            canvas::Stroke::default()
                .with_color(CURVE_LINE)
                .with_width(2.0)
                .with_line_cap(canvas::LineCap::Round),
        );
        for (i, (p, h)) in points.iter().zip(&hours).enumerate() {
            if i % 3 != 0 {
                continue;
            }
            f.fill(&canvas::Path::circle(*p, 2.6), icon::SUN);
            f.fill_text(kit::text_in(
                MANROPE_BOLD,
                kit::text(
                    format!("{}°", units.degrees(h.moment.temperature)),
                    Point::new(p.x, p.y - 9.0),
                    11.0,
                    TEXT.get(),
                    Alignment::Center,
                ),
            ));
            let hour = h.moment.at.to_zoned(zone.clone()).hour();
            f.fill_text(kit::text(
                format!("{hour} h"),
                Point::new(p.x, size.height - 6.0),
                10.5,
                FAINT.get(),
                Alignment::Center,
            ));
        }
    })
}

/// A day of the week: its name, its icon, its rain, its low, the range
/// bar on the week's (`low`, `high`), its high; tomorrow lit.
fn day_row<'a>(
    words: Words,
    i: usize,
    day: &Day,
    units: Units,
    (low, high): (f32, f32),
) -> El<'a> {
    let name = match i {
        0 => words.today_short().to_owned(),
        1 => words.tomorrow().to_owned(),
        _ => {
            let weekday = usize::from(day.date.weekday().to_monday_zero_offset().unsigned_abs());
            let short = hocus_words::date::SHORT_WEEKDAYS
                .get(words.lang)
                .get(weekday)
                .copied()
                .unwrap_or_default();
            format!("{short} {}", day.date.day())
        }
    };
    let (lo, hi) = (weather::degrees(day.min, units), weather::degrees(day.max, units));
    let amount = units.amount(day.mm).unwrap_or_default();
    let (from, to) = (day.min, day.max);
    let span = (high - low).max(1.0);
    let range = drawn(RANGE, 5.0, move |f, size| {
        f.fill(
            &kit::rounded(Rectangle::new(Point::ORIGIN, size), 2.5),
            TRACK.get(),
        );
        let x = (from - low) / span * size.width;
        let w = ((to - from) / span * size.width).max(4.0);
        let gradient = canvas::gradient::Linear::new(Point::new(x, 0.0), Point::new(x + w, 0.0))
            .add_stop(0.0, COLD)
            .add_stop(1.0, CURVE_LINE);
        f.fill(
            &kit::rounded(Rectangle::new(Point::new(x, 0.0), Size::new(w, size.height)), 2.5),
            canvas::Fill::from(gradient),
        );
    });
    let cell = |content: El<'a>, width: f32, align: Horizontal| -> El<'a> {
        container(content).width(width).align_x(align).into()
    };
    let line = row![
        cell(said(name.clone(), 13.0, kit::MANROPE_SEMIBOLD, TEXT.get()), 62.0, Horizontal::Left),
        cell(icon::widget(day.icon(), 18.0), 22.0, Horizontal::Center),
        cell(
            said(amount.clone(), 11.5, MANROPE, icon::RAIN.get()),
            40.0,
            Horizontal::Left
        ),
        cell(said(lo.clone(), 13.0, MANROPE, DIM.get()), 26.0, Horizontal::Right),
        container(range).width(RANGE).center_y(DAY),
        cell(said(hi.clone(), 13.0, MANROPE, TEXT.get()), 26.0, Horizontal::Right),
    ]
    .spacing(6)
    .align_y(Vertical::Center)
    .height(DAY);
    let lit = i == 1;
    let boxed = container(line)
        .padding([0.0, 4.0])
        .width(Length::Fill)
        .style(move |_| {
            if lit {
                style::boxed(TOMORROW.get(), 8.0)
            } else {
                container::Style::default()
            }
        });
    control(
        boxed,
        format!("weather:day:{i}"),
        Role::ListItem,
        format!("{name}, {lo} – {hi}{}", if amount.is_empty() { String::new() } else { format!(", {amount}") }),
    )
    .into()
}

fn handle(message: Msg) {
    match message {
        Msg::Stale => {}
        Msg::Pressed => ui(|ui| ui.focus = None),
        Msg::Focus(target) => ui(|ui| ui.focus = Some(target)),
        Msg::Act(Target::Settings) => {
            tracing::info!("the weather's popover opens its settings");
            super::session::open_settings(Some("weather"));
        }
    }
    for spot in [Spot::Head, Spot::Rain, Spot::Tomorrow, Spot::Mini] {
        super::changed(Item::Weather(spot));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_height_holds_its_parts() {
        // The mockup's parts, 14 px apart, in 14 px: about 660 px.
        let h = height();
        assert!((640..=700).contains(&h), "{h}");
    }
}
