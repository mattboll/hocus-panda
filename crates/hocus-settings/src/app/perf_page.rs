//! The Performances page (#655, docs/design/settings.md "Performances"):
//! a day of the desktop's CPU and memory as the compositor recorded it.
//! The day (‹ › through the 14 kept, "Jour | 7 jours"), four tiles, the
//! signals with "Copier le rapport", the day's chart in bands sharing one
//! time axis, the peaks' table with what was happening, and the switch.
//!
//! The day is read on a thread of the page's own as the page shows or
//! the day changes ([`crate::perf::read`]); nothing is read at rest. The
//! window also tells the compositor its page (and its process, measured
//! with the desktop) on the control socket, off its thread.

use hocus_words::settings::app_perf_page as words;
use std::{path::PathBuf, time::Instant};

use hocus_perf::{
    EventKind, Report, SignalKind,
    format::Who,
    report::Peak,
    signals::{self, Signal},
    text,
};
use iced::{
    Alignment, Background, Color, Length, Padding, Point, Rectangle, Renderer, Size, Theme,
    futures::channel::mpsc::UnboundedSender,
    mouse,
    widget::{Column, Space, button, canvas, column, container, row},
};
use jiff::{civil::Date, tz::TimeZone};

use super::{
    App, Message, Page,
    style::*,
    view::{El, card_box, divided, label, t},
};
use crate::{
    lang::{Lang, w},
    model::SectionId,
    perf::{self as model, Bucket, Read},
};

/// How long "Copié" stays.
const COPIED: std::time::Duration = std::time::Duration::from_secs(2);

/// The chart's colours: the compositor, the shell, a signal's threshold,
/// the frames, a moving wallpaper.
const COMPOSITOR: Color = rgb(0x5B8DEF);
const SHELL: Color = rgb(0x3FA66B);
const THRESHOLD: Color = rgb(0xF2945B);
const FRAMES: Color = rgb(0x8C96A8);
const WALLPAPER: Color = rgb(0x3FA66B);

#[derive(Debug, Clone)]
pub enum Msg {
    Read(Box<Read>),
    /// ‹ and ›.
    Earlier,
    Later,
    /// "Jour | 7 jours".
    Week(bool),
    /// A day of the week's list.
    Day(Date),
    /// "Copier le rapport" of the signal `n`.
    Copy(usize),
    Folder,
}

/// The page's state.
#[derive(Debug)]
pub struct State {
    to: Option<UnboundedSender<Message>>,
    dir: Option<PathBuf>,
    tz: TimeZone,
    /// The day shown; `None`: today.
    pub(super) date: Option<Date>,
    pub(super) week: bool,
    pub(super) read: Option<Read>,
    /// The read under way: which day, with its week or not.
    reading: Option<(Date, bool)>,
    /// The page the compositor was told last; `None` before the first.
    told: Option<Option<SectionId>>,
    /// A signal's report copied, when.
    copied: Option<(usize, Instant)>,
}

impl State {
    pub fn new(to: &UnboundedSender<Message>, state: Option<PathBuf>) -> Self {
        Self {
            to: Some(to.clone()),
            dir: state.as_deref().map(model::dir),
            tz: TimeZone::system(),
            date: None,
            week: false,
            read: None,
            reading: None,
            told: None,
            copied: None,
        }
    }
}

fn today(tz: &TimeZone) -> Date {
    jiff::Zoned::now().with_time_zone(tz.clone()).date()
}

impl App {
    fn perf_shows(&self) -> bool {
        self.page == Page::Section(SectionId::Performance)
    }

    /// The day shown.
    fn perf_date(&self) -> Date {
        self.perf.date.unwrap_or_else(|| today(&self.perf.tz))
    }

    /// After each message: the compositor told of a new page; the day read
    /// when the page shows it and it is not read yet.
    pub(super) fn perf_after(&mut self) {
        let page = match self.page {
            Page::Section(id) => Some(id),
            Page::Home => None,
        };
        if self.perf.told != Some(page) {
            self.perf.told = Some(page);
            let name = page.map(|id| id.name(self.lang)).unwrap_or_default();
            tell_page(crate::perf::settings_line(name));
        }
        if !self.perf_shows() {
            return;
        }
        let wanted = (self.perf_date(), self.perf.week);
        let read = self
            .perf
            .read
            .as_ref()
            .is_some_and(|r| r.date == wanted.0 && (!wanted.1 || !r.week.is_empty()));
        if read || self.perf.reading == Some(wanted) {
            return;
        }
        let (Some(dir), Some(to)) = (self.perf.dir.clone(), self.perf.to.clone()) else {
            return;
        };
        self.perf.reading = Some(wanted);
        let started = std::thread::Builder::new()
            .name("performance".to_owned())
            .spawn(move || {
                let read = model::read(&dir, wanted.0, wanted.1);
                let _ = to.unbounded_send(Message::Perf(Msg::Read(Box::new(read))));
            });
        if let Err(err) = started {
            tracing::warn!(%err, "the day is not read");
        }
    }

    pub(super) fn perf_update(&mut self, msg: Msg) -> iced::Task<Message> {
        match msg {
            Msg::Read(read) => {
                if self.perf.reading == Some((read.date, !read.week.is_empty())) {
                    self.perf.reading = None;
                }
                tracing::info!(
                    date = %read.date,
                    samples = read.day.samples.len(),
                    "a day of the performance read"
                );
                self.perf.read = Some(*read);
            }
            Msg::Earlier | Msg::Later => {
                let days = self
                    .perf
                    .read
                    .as_ref()
                    .map(|r| r.days.clone())
                    .unwrap_or_default();
                if let Some(date) = model::step(&days, self.perf_date(), msg_later(&msg)) {
                    self.perf.date = (date != today(&self.perf.tz)).then_some(date);
                }
            }
            Msg::Week(week) => self.perf.week = week,
            Msg::Day(date) => {
                self.perf.date = (date != today(&self.perf.tz)).then_some(date);
                self.perf.week = false;
            }
            Msg::Copy(n) => {
                let Some(read) = &self.perf.read else {
                    return iced::Task::none();
                };
                let Some(signal) = read.report.signals.get(n) else {
                    return iced::Task::none();
                };
                let report =
                    text::signal_report(&read.report, signal, read.date, self.lang, &self.perf.tz);
                self.perf.copied = Some((n, Instant::now()));
                tracing::info!(signal = ?signal.kind, "a signal's report copied");
                return iced::Task::batch([
                    iced::clipboard::write(report),
                    iced::Task::perform(tokio::time::sleep(COPIED), |()| Message::Nothing),
                ]);
            }
            Msg::Folder => {
                if let Some(dir) = self.perf.dir.clone() {
                    tracing::info!(folder = %dir.display(), "the days' folder is opened");
                    // Made first: no day may have been recorded yet.
                    super::open_folder(dir, true);
                }
            }
        }
        iced::Task::none()
    }

    /// What the page shows, for the log.
    pub(super) fn perf_shown(&self) -> serde_json::Value {
        let date = self.perf_date();
        let Some(read) = self.perf.read.as_ref().filter(|r| r.date == date) else {
            return serde_json::json!({ "date": date.to_string(), "read": false });
        };
        let lang = self.lang;
        let tz = &self.perf.tz;
        serde_json::json!({
            "date": date.to_string(),
            "read": true,
            "week": self.perf.week,
            "samples": read.day.samples.len(),
            "tiles": model::tiles(&read.report, self.lang, tz)
                .iter()
                .map(|t| serde_json::json!([t.label, t.value, t.line, t.sub]))
                .collect::<Vec<_>>(),
            "signals": read.report.signals.iter().map(|s| text::signal(s, lang, tz).0).collect::<Vec<_>>(),
            "peaks": read.report.peaks.iter().map(|p| text::peak(p, lang).0).collect::<Vec<_>>(),
        })
    }

    // --- Drawn ------------------------------------------------------------------------

    pub(super) fn perf_page<'a>(&'a self, mut page: Column<'a, Message>) -> El<'a> {
        let lang = self.lang;
        let controls = self.controls();
        let head = |words: &'static str, right: Option<String>| -> El<'a> {
            let mut head = row![label(words), Space::new().width(Length::Fill)]
                .align_y(Alignment::Center)
                .height(28);
            if let Some(right) = right {
                head = head.push(t(right, 12.0, FAINT.get()));
            }
            head.into()
        };
        let mut groups = column![].spacing(18).width(Length::Fill);
        groups = groups.push(self.perf_day_bar());
        let date = self.perf_date();
        match self.perf.read.as_ref().filter(|r| r.date == date) {
            None => {
                groups = groups.push(card_box(
                    container(t(w(words::READING_THE_DAY).get(lang), 13.0, DIM.get()))
                        .padding([14, 16])
                        .into(),
                ));
            }
            Some(read) if self.perf.week && !read.week.is_empty() => {
                groups = groups.push(
                    column![
                        head(w(words::THE_LAST_7_DAYS).get(lang), None),
                        card_box(self.perf_week(read)),
                    ]
                    .spacing(8),
                );
            }
            Some(read) if read.day.samples.is_empty() => {
                groups = groups.push(card_box(
                    container(
                        column![
                            t(
                                w(words::NOTHING_RECORDED_THAT_DAY).get(lang),
                                14.0,
                                TEXT.get()
                            )
                            .font(SEMIBOLD),
                            t(
                                w(words::THE_DAY_IS_MEASURED_WHILE_RECORD).get(lang),
                                12.5,
                                DIM.get(),
                            ),
                        ]
                        .spacing(4),
                    )
                    .padding([14, 16])
                    .into(),
                ));
            }
            Some(read) => {
                groups = groups.push(self.perf_tiles(&read.report));
                let count = read.report.signals.len();
                let today = read.date == today(&self.perf.tz);
                let right = match today {
                    true => words::today(lang, count),
                    false => words::that_day(lang, count),
                };
                groups = groups.push(
                    column![
                        head(w(words::SIGNALS).get(lang), Some(right)),
                        card_box(self.perf_signals(&read.report)),
                    ]
                    .spacing(8),
                );
                groups = groups.push(
                    column![
                        head(
                            w(words::THE_DAY).get(lang),
                            Some(w(words::A_SCALE_FOR_EACH_BAND).get(lang).to_owned())
                        ),
                        card_box(
                            container(
                                canvas(Chart::of(read, lang, &self.perf.tz))
                                    .width(Length::Fill)
                                    .height(CHART_HEIGHT),
                            )
                            .padding([14, 14])
                            .id("perf:chart".to_owned())
                            .into()
                        ),
                    ]
                    .spacing(8),
                );
                if !read.report.peaks.is_empty() {
                    groups = groups.push(
                        column![
                            head(w(words::THE_PEAKS).get(lang), None),
                            card_box(self.perf_peaks(&read.report)),
                        ]
                        .spacing(8),
                    );
                }
            }
        }
        // ENREGISTREMENT: the switch and the files.
        let mut rows: Vec<El<'a>> = Vec::new();
        if let Some(setting) = crate::model::setting(model::RECORD) {
            rows.push(self.row(setting, &controls));
        }
        rows.push(self.perf_files_row());
        let mut body = column![];
        for (i, r) in rows.into_iter().enumerate() {
            body = body.push(divided(i > 0, r));
        }
        groups = groups.push(
            column![
                head(w(words::RECORDING).get(lang), None),
                card_box(body.into()),
            ]
            .spacing(8),
        );
        page = page.push(groups);
        container(page).id("page:performance".to_owned()).into()
    }

    fn perf_day_bar(&self) -> El<'_> {
        let lang = self.lang;
        let date = self.perf_date();
        let days = self
            .perf
            .read
            .as_ref()
            .map(|r| r.days.as_slice())
            .unwrap_or_default();
        let arrow = |glyph: &'static str, msg: Msg, on: bool| -> El<'_> {
            button(
                container(t(glyph, 18.0, if on { TEXT.get() } else { FAINT.get() }))
                    .center_x(28)
                    .center_y(28),
            )
            .padding(0)
            .on_press_maybe(on.then_some(Message::Perf(msg)))
            .style(flat(None, HOVER.get(), 8.0, false))
            .into()
        };
        let session = self
            .perf
            .read
            .as_ref()
            .filter(|r| r.date == date)
            .map(|r| model::session(&r.report, lang, &self.perf.tz))
            .unwrap_or_default();
        let week = |week: bool, words: crate::lang::Words| -> El<'_> {
            let chosen = self.perf.week == week;
            let id = if week { "perf:week" } else { "perf:day" };
            container(
                button(
                    t(
                        words.get(lang),
                        12.5,
                        if chosen { TEXT.get() } else { DIM.get() },
                    )
                    .font(SEMIBOLD),
                )
                .height(24)
                .padding([3, 10])
                .on_press(Message::Perf(Msg::Week(week)))
                .style(move |_, _| button::Style {
                    background: chosen.then_some(Background::Color(crate::theme::ink(rgba(
                        255, 255, 255, 0.13,
                    )))),
                    text_color: TEXT.get(),
                    border: border(7.0, 0.0, Color::TRANSPARENT),
                    ..button::Style::default()
                }),
            )
            .id(id.to_owned())
            .into()
        };
        let segmented = container(row![
            week(false, w(words::DAY)),
            week(true, w(words::N_7_DAYS))
        ])
        .padding(2)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(crate::theme::ink(rgba(
                255, 255, 255, 0.05,
            )))),
            border: border(9.0, 0.0, Color::TRANSPARENT),
            ..container::Style::default()
        });
        row![
            arrow("‹", Msg::Earlier, model::step(days, date, false).is_some()),
            t(
                model::date_title(date, today(&self.perf.tz), lang),
                15.0,
                TEXT.get()
            )
            .font(BOLD),
            t(session, 12.5, DIM.get()),
            arrow("›", Msg::Later, model::step(days, date, true).is_some()),
            Space::new().width(Length::Fill),
            segmented,
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    }

    fn perf_tiles(&self, report: &Report) -> El<'_> {
        let tiles = model::tiles(report, self.lang, &self.perf.tz);
        // Two by two: the page is 720 px wide, a tile's lines stay whole.
        let mut grid = column![].spacing(10);
        for two in tiles.chunks(2) {
            let mut line = row![].spacing(10);
            for tile in two {
                let tile = tile.clone();
                line = line.push(
                    container(
                        column![
                            t(tile.label, 11.5, DIM.get()).font(BOLD),
                            t(tile.value, 22.0, TEXT.get()).font(BOLD),
                            t(tile.line, 11.5, DIM.get()),
                            t(tile.sub, 11.0, FAINT.get()),
                        ]
                        .spacing(3),
                    )
                    .padding([12, 14])
                    .width(Length::FillPortion(1))
                    .style(boxed(
                        CARD.get(),
                        12.0,
                        Some((1.0, CARD_RIM.get())),
                    )),
                );
            }
            grid = grid.push(line);
        }
        grid.into()
    }

    fn perf_signals(&self, report: &Report) -> El<'_> {
        let lang = self.lang;
        let said = lang;
        let mut rows: Vec<El<'_>> = Vec::new();
        for (n, signal) in report.signals.iter().enumerate() {
            let (title, line) = text::signal(signal, said, &self.perf.tz);
            let copied = self
                .perf
                .copied
                .is_some_and(|(c, at)| c == n && at.elapsed() < COPIED);
            let copy = button(t(
                if copied {
                    w(words::COPIED).get(lang)
                } else {
                    w(words::COPY_THE_REPORT).get(lang)
                },
                12.5,
                MINT.get(),
            ))
            .padding([4, 8])
            .on_press(Message::Perf(Msg::Copy(n)))
            .style(flat(None, HOVER.get(), 7.0, false));
            rows.push(
                container(
                    row![
                        column![
                            t(format!("⚠ {title}"), 13.5, ORANGE.get()).font(SEMIBOLD),
                            t(line, 12.0, DIM.get()),
                        ]
                        .spacing(3)
                        .width(Length::Fill),
                        copy,
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([12, 16])
                .into(),
            );
        }
        let calm: Vec<&str> = SignalKind::ALL
            .iter()
            .filter(|k| !report.signals.iter().any(|s| s.kind == **k))
            .map(|k| kind_name(*k, lang))
            .collect();
        rows.push(
            container(
                column![
                    t(
                        if report.signals.is_empty() {
                            w(words::NO_SIGNAL)
                        } else {
                            w(words::NOTHING_ELSE)
                        }
                        .get(lang),
                        13.5,
                        MINT.get()
                    )
                    .font(SEMIBOLD),
                    t(
                        words::under_their_thresholds(lang, calm.join(", ")),
                        12.0,
                        DIM.get()
                    ),
                ]
                .spacing(3),
            )
            .padding([12, 16])
            .into(),
        );
        let mut body = column![];
        for (i, r) in rows.into_iter().enumerate() {
            body = body.push(divided(i > 0, r));
        }
        body.into()
    }

    fn perf_peaks(&self, report: &Report) -> El<'_> {
        let lang = self.lang;
        let said = lang;
        let tz = &self.perf.tz;
        let mut rows: Vec<El<'_>> = Vec::new();
        let head = row![
            Space::new().width(26),
            t(w(words::WHEN).get(lang), 11.5, FAINT.get()).width(96),
            t(w(words::WHAT_WAS_HAPPENING).get(lang), 11.5, FAINT.get()).width(Length::Fill),
            t("CPU", 11.5, FAINT.get())
                .width(52)
                .align_x(Alignment::End),
            t(w(words::LENGTH).get(lang), 11.5, FAINT.get())
                .width(60)
                .align_x(Alignment::End),
        ]
        .spacing(10);
        rows.push(container(head).padding([8, 16]).into());
        for p in &report.peaks {
            let (what, why) = text::peak(p, said);
            let flagged = p.signal.is_some();
            let ink = if flagged { ORANGE.get() } else { TEXT.get() };
            let badge = container(t(p.n.to_string(), 11.0, ink).font(BOLD))
                .center_x(20)
                .center_y(20)
                .style(move |_: &Theme| container::Style {
                    border: border(10.0, 1.5, ink),
                    ..container::Style::default()
                });
            let line = row![
                container(badge).width(26),
                t(text::when(p.from, p.to, tz), 12.0, DIM.get())
                    .font(MONO)
                    .width(96),
                column![t(what, 13.0, ink).font(SEMIBOLD), t(why, 11.5, DIM.get())]
                    .spacing(2)
                    .width(Length::Fill),
                t(text::percent(p.cpu_percent, said), 12.0, TEXT.get())
                    .font(MONO)
                    .width(52)
                    .align_x(Alignment::End),
                t(text::duration(p.seconds()), 12.0, DIM.get())
                    .font(MONO)
                    .width(60)
                    .align_x(Alignment::End),
            ]
            .spacing(10)
            .align_y(Alignment::Center);
            let tint = if flagged {
                Some(fade(THRESHOLD, 0.08))
            } else if p.wallpaper {
                Some(fade(WALLPAPER, 0.07))
            } else {
                None
            };
            rows.push(
                container(line)
                    .padding([9, 16])
                    .width(Length::Fill)
                    .style(move |_: &Theme| container::Style {
                        background: tint.map(Background::Color),
                        ..container::Style::default()
                    })
                    .into(),
            );
        }
        let mut body = column![];
        for (i, r) in rows.into_iter().enumerate() {
            body = body.push(divided(i > 1, r));
        }
        body.into()
    }

    fn perf_week<'a>(&'a self, read: &'a Read) -> El<'a> {
        let lang = self.lang;
        let said = lang;
        let most = read
            .week
            .iter()
            .map(|(_, r)| r.cpu.average_percent)
            .fold(1.0_f64, f64::max);
        let mut body = column![];
        for (i, (date, r)) in read.week.iter().enumerate() {
            let recorded = r.from.is_some();
            let bar = container(Space::new().height(8))
                .width(Length::Fixed((120.0 * r.cpu.average_percent / most) as f32))
                .style(boxed(COMPOSITOR, 4.0, None));
            let said_cpu = if recorded {
                text::percent(r.cpu.average_percent, said)
            } else {
                "—".to_owned()
            };
            let memory = if recorded {
                text::megabytes(r.memory.end_kb[0] + r.memory.end_kb[1], said)
            } else {
                String::new()
            };
            let flags = match (r.signals.len(), lang) {
                (0, _) => String::new(),
                (n, Lang::French) => format!("⚠ {n} signa{}", if n > 1 { "ux" } else { "l" }),
                (n, Lang::English) => format!("⚠ {n} signal{}", if n > 1 { "s" } else { "" }),
            };
            let line = button(
                row![
                    t(text::date(*date, said), 13.0, TEXT.get())
                        .font(SEMIBOLD)
                        .width(170),
                    container(bar).width(130),
                    t(said_cpu, 12.0, TEXT.get()).font(MONO).width(64),
                    t(memory, 12.0, DIM.get()).font(MONO).width(80),
                    t(flags, 12.0, ORANGE.get()).width(Length::Fill),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([10, 16])
            .width(Length::Fill)
            .on_press_maybe(recorded.then_some(Message::Perf(Msg::Day(*date))))
            .style(flat(None, HOVER.get(), 0.0, false));
            body = body.push(divided(i > 0, line.into()));
        }
        body.into()
    }

    fn perf_files_row(&self) -> El<'_> {
        let lang = self.lang;
        let place = self
            .perf
            .dir
            .as_deref()
            .map(super::view::tilde)
            .unwrap_or_default();
        let line = words::n_14_days_under_1_mb_a(lang, &place);
        row![
            column![
                t(w(words::THE_FILES).get(lang), 14.0, TEXT.get()).font(SEMIBOLD),
                t(line, 12.5, DIM.get()),
            ]
            .spacing(3)
            .width(Length::Fill),
            button(t(w(words::OPEN_THE_FOLDER).get(lang), 12.5, MINT.get()))
                .padding([4, 8])
                .on_press_maybe(
                    self.perf
                        .dir
                        .is_some()
                        .then_some(Message::Perf(Msg::Folder))
                )
                .style(flat(None, HOVER.get(), 7.0, false)),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .padding(Padding::from([12, 16]))
        .into()
    }
}

fn msg_later(msg: &Msg) -> bool {
    matches!(msg, Msg::Later)
}

/// A signal's kind in two words, for "under their thresholds".
fn kind_name(kind: SignalKind, lang: Lang) -> &'static str {
    match kind {
        SignalKind::SustainedCpu => w(words::SUSTAINED_CPU),
        SignalKind::FramesAtRest => w(words::FRAMES_AT_REST),
        SignalKind::Wakeups => w(words::WAKEUPS),
        SignalKind::MemoryGrowing => w(words::MEMORY),
        SignalKind::SlowMoment => w(words::SLOW_MOMENTS),
    }
    .get(lang)
}

/// Tells the compositor the window's page, off the window's thread.
fn tell_page(line: String) {
    let Some(socket) = crate::devices::socket() else {
        return;
    };
    let started = std::thread::Builder::new()
        .name("page told".to_owned())
        .spawn(move || {
            use std::io::Write as _;
            let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&socket) else {
                return;
            };
            let _ = stream.set_write_timeout(Some(std::time::Duration::from_millis(500)));
            let _ = writeln!(stream, "{line}");
        });
    if let Err(err) = started {
        tracing::debug!(%err, "the page is not told");
    }
}

// --- The chart ------------------------------------------------------------------------

/// The chart's height: the legend, five bands and the hours.
const CHART_HEIGHT: f32 = 452.0;
/// Room for the scales on the left.
const LEFT: f32 = 40.0;

/// A band of the chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Band {
    Cpu,
    Memory,
    Frames,
    Events,
    Power,
}

impl Band {
    const ALL: [Self; 5] = [
        Self::Cpu,
        Self::Memory,
        Self::Frames,
        Self::Events,
        Self::Power,
    ];

    fn height(self) -> f32 {
        match self {
            Self::Cpu => 108.0,
            Self::Memory => 76.0,
            Self::Frames => 46.0,
            Self::Events => 30.0,
            Self::Power => 42.0,
        }
    }

    fn title(self, lang: Lang) -> &'static str {
        match self {
            Self::Cpu => w(words::CPU_OF_A_CORE),
            Self::Memory => w(words::RESIDENT_MEMORY_MB),
            Self::Frames => w(words::FRAMES_DRAWN_A_SECOND),
            Self::Events => w(words::WHAT_WAS_HAPPENING),
            Self::Power => w(words::THE_MACHINE_S_POWER_W_ON),
        }
        .get(lang)
    }
}

/// The smallest of `steps` at or above `x`.
fn nice(x: f32, steps: &[f32]) -> f32 {
    steps
        .iter()
        .copied()
        .find(|s| *s >= x)
        .unwrap_or_else(|| steps.last().copied().unwrap_or(1.0))
}

/// The day's chart: its data, drawn in bands sharing one time axis.
#[derive(Debug)]
struct Chart {
    lang: Lang,
    from: i64,
    to: i64,
    buckets: Vec<Bucket>,
    locked: Vec<(i64, i64)>,
    wallpaper: Vec<(i64, i64)>,
    /// Each peak: its number, its middle, its CPU, whether it is a signal.
    peaks: Vec<(usize, i64, f32, bool)>,
    /// A memory that only grows: from, to, kilobytes then and now, whose.
    growth: Option<(i64, i64, [u32; 2], Who)>,
    /// What to write in the events' band, where.
    labels: Vec<(i64, String)>,
    /// The hours on the axis, and their words.
    hours: Vec<(i64, String)>,
    /// The memory's last values, for its labels.
    memory_end: [f32; 2],
    memory_start: [f32; 2],
}

/// The buckets drawn: one per 4 px or so of a 690 px chart.
const BUCKETS: usize = 160;

impl Chart {
    fn of(read: &Read, lang: Lang, tz: &TimeZone) -> Self {
        let r = &read.report;
        let (from, to) = (
            r.from.unwrap_or(0),
            r.to.unwrap_or(1).max(r.from.unwrap_or(0) + 1),
        );
        let buckets = model::buckets(&read.day, from, to, BUCKETS);
        // Each peak's marker over its highest bar.
        let index = |t: i64| {
            ((t - from) as f32 / (to - from) as f32 * BUCKETS as f32)
                .clamp(0.0, BUCKETS as f32 - 1.0) as usize
        };
        let peaks = r
            .peaks
            .iter()
            .map(|p: &Peak| {
                let highest = buckets[index(p.from)..=index(p.to).max(index(p.from))]
                    .iter()
                    .map(|b| b.highest)
                    .fold(0.0, f32::max);
                (p.n, (p.from + p.to) / 2, highest, p.signal.is_some())
            })
            .collect();
        let growth = r
            .signals
            .iter()
            .find(|s: &&Signal| s.kind == SignalKind::MemoryGrowing)
            .and_then(|s| Some((s.from, s.to, s.kb?, s.who?)));
        let mut labels = Vec::new();
        for s in &read.day.samples {
            for e in &s.e {
                let said = match e.kind {
                    EventKind::ScreenPlugged => words::screen(lang, &e.what),
                    EventKind::SettingsPage => w(words::SETTINGS).get(lang).to_owned(),
                    EventKind::Locked => w(words::LOCKED).get(lang).to_owned(),
                    EventKind::Resume => w(words::WOKE).get(lang).to_owned(),
                    EventKind::WallpaperMoving => w(words::MOVING_WALLPAPER).get(lang).to_owned(),
                    _ => continue,
                };
                labels.push((e.t, said));
            }
        }
        // Many windows at once, said once.
        for p in &r.peaks {
            let opened = p
                .events
                .iter()
                .filter(|e| e.kind == EventKind::WindowOpened)
                .count();
            if opened > 4 {
                labels.push(((p.from + p.to) / 2, words::windows(lang, opened)));
            }
        }
        labels.sort_by_key(|l| l.0);
        // An hour a tick, every two past twelve hours.
        let step = if to - from > 12 * 3600 { 7200 } else { 3600 };
        let mut hours = Vec::new();
        let mut at = (from / step + 1) * step;
        while at < to {
            // Whole local hours: the zone's offset taken in.
            let local = jiff::Timestamp::from_second(at)
                .map(|ts| ts.to_zoned(tz.clone()))
                .ok();
            if let Some(z) = local {
                let back = i64::from(z.minute()) * 60 + i64::from(z.second());
                let tick = at - back;
                if tick > from && hours.last().is_none_or(|h: &(i64, String)| h.0 != tick) {
                    hours.push((tick, format!("{} h", z.hour())));
                }
            }
            at += step;
        }
        let mb = |kb: [u32; 2]| [kb[0] as f32 / 1024.0, kb[1] as f32 / 1024.0];
        Self {
            lang,
            from,
            to,
            buckets,
            locked: r.locked.clone(),
            wallpaper: r.wallpaper.clone(),
            peaks,
            growth,
            labels,
            hours,
            memory_end: mb(r.memory.end_kb),
            memory_start: mb(r.memory.start_kb),
        }
    }

    fn x(&self, t: i64, width: f32) -> f32 {
        LEFT + (t - self.from) as f32 / (self.to - self.from).max(1) as f32 * (width - LEFT)
    }
}

impl canvas::Program<Message> for Chart {
    type State = ();

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let width = bounds.width;
        let lang = self.lang;
        let text_ink = DIM.get();
        let faint = FAINT.get();
        let grid = fade(TEXT.get(), 0.07);
        let write =
            |f: &mut canvas::Frame, s: String, at: Point, size: f32, color: Color, right: bool| {
                f.fill_text(canvas::Text {
                    content: s,
                    position: at,
                    color,
                    size: size.into(),
                    font: MANROPE,
                    align_x: if right {
                        iced::widget::text::Alignment::Right
                    } else {
                        iced::widget::text::Alignment::Left
                    },
                    ..canvas::Text::default()
                });
            };
        // The legend.
        let mut lx = LEFT;
        for (swatch, words) in [
            (COMPOSITOR, w(words::COMPOSITOR)),
            (SHELL, w(words::SHELL)),
            (fade(WALLPAPER, 0.5), w(words::MOVING_WALLPAPER)),
            (fade(TEXT.get(), 0.25), w(words::LOCKED)),
            (THRESHOLD, w(words::A_SIGNAL_S_THRESHOLD)),
        ] {
            f.fill_rectangle(Point::new(lx, 2.0), Size::new(10.0, 10.0), swatch);
            let said = words.get(lang);
            write(
                &mut f,
                said.to_owned(),
                Point::new(lx + 14.0, 0.0),
                11.0,
                text_ink,
                false,
            );
            lx += 14.0 + measure(said, 11.0, MANROPE) + 18.0;
        }
        let bw = (width - LEFT) / self.buckets.len().max(1) as f32;
        let mut y = 24.0;
        let bottom_of_bands = 24.0
            + Band::ALL
                .iter()
                .map(|b| b.height() + 16.0 + 10.0)
                .sum::<f32>();
        // The hours' lines, through every band.
        for (at, _) in &self.hours {
            let x = self.x(*at, width);
            f.fill_rectangle(
                Point::new(x, 24.0),
                Size::new(1.0, bottom_of_bands - 24.0 - 10.0),
                grid,
            );
        }
        for band in Band::ALL {
            write(
                &mut f,
                band.title(lang).to_owned(),
                Point::new(LEFT, y),
                11.5,
                text_ink,
                false,
            );
            let top = y + 16.0;
            let h = band.height();
            let bottom = top + h;
            // Locked, hatched grey; the moving wallpaper, green.
            let hatch = |f: &mut canvas::Frame, spans: &[(i64, i64)], color: Color| {
                for &(a, b) in spans {
                    let (xa, xb) = (self.x(a, width), self.x(b, width));
                    if xb <= xa {
                        continue;
                    }
                    // Lines at 45°, each cut to the span: from its foot at
                    // (x0, bottom) up to (x0 + h, top).
                    let mut x0 = xa - h;
                    while x0 < xb {
                        let enter = ((xa - x0) / h).clamp(0.0, 1.0);
                        let leave = ((xb - x0) / h).clamp(0.0, 1.0);
                        if leave > enter {
                            let at = |t: f32| Point::new(x0 + t * h, bottom - t * h);
                            f.stroke(
                                &canvas::Path::line(at(enter), at(leave)),
                                canvas::Stroke::default().with_color(color).with_width(1.0),
                            );
                        }
                        x0 += 6.0;
                    }
                }
            };
            hatch(&mut f, &self.locked, fade(TEXT.get(), 0.16));
            if matches!(band, Band::Cpu | Band::Events) {
                hatch(&mut f, &self.wallpaper, fade(WALLPAPER, 0.35));
            }
            let scale = |f: &mut canvas::Frame, max: f32, marks: &[f32]| {
                for v in marks {
                    let yy = bottom - v / max * h;
                    f.fill_rectangle(
                        Point::new(LEFT, yy),
                        Size::new(width - LEFT, 1.0),
                        if *v == 0.0 {
                            fade(TEXT.get(), 0.14)
                        } else {
                            grid
                        },
                    );
                    write(
                        f,
                        format!("{v}"),
                        Point::new(LEFT - 6.0, yy - 7.0),
                        10.5,
                        faint,
                        true,
                    );
                }
            };
            match band {
                Band::Cpu => {
                    let most = self.buckets.iter().map(|b| b.highest).fold(0.0, f32::max);
                    let max = nice(most, &[30.0, 50.0, 100.0, 200.0, 400.0, 800.0]);
                    scale(&mut f, max, &[0.0, max / 2.0, max]);
                    for (i, b) in self.buckets.iter().enumerate() {
                        let x = LEFT + i as f32 * bw;
                        let comp = (b.compositor / max * h).min(h);
                        let shell = (b.shell / max * h).min(h - comp);
                        // Its highest sample, fainter, over its average.
                        let highest = (b.highest / max * h).min(h);
                        if highest > comp + shell {
                            f.fill_rectangle(
                                Point::new(x, bottom - highest),
                                Size::new(bw, highest - comp - shell),
                                fade(SHELL, 0.3),
                            );
                        }
                        f.fill_rectangle(
                            Point::new(x, bottom - comp),
                            Size::new(bw, comp),
                            fade(COMPOSITOR, 0.9),
                        );
                        f.fill_rectangle(
                            Point::new(x, bottom - comp - shell),
                            Size::new(bw, shell),
                            fade(SHELL, 0.85),
                        );
                    }
                    // The sustained CPU's threshold.
                    let ty = bottom - signals::CPU_PERCENT as f32 / max * h;
                    let dashed = canvas::Stroke {
                        line_dash: canvas::LineDash {
                            segments: &[5.0, 4.0],
                            offset: 0,
                        },
                        ..canvas::Stroke::default()
                            .with_color(THRESHOLD)
                            .with_width(1.5)
                    };
                    f.stroke(
                        &canvas::Path::line(Point::new(LEFT, ty), Point::new(width, ty)),
                        dashed,
                    );
                    write(
                        &mut f,
                        w(words::N_10_FOR_5_MIN).get(lang).to_owned(),
                        Point::new(width, ty - 15.0),
                        10.5,
                        THRESHOLD,
                        true,
                    );
                    // The peaks, numbered.
                    for &(n, at, cpu, flagged) in &self.peaks {
                        let x = self.x(at, width);
                        let cpu = cpu.min(max);
                        let vy = (bottom - cpu / max * h).max(top + 22.0);
                        let ink = if flagged { THRESHOLD } else { TEXT.get() };
                        let cy = (vy - 16.0).max(top + 9.0);
                        f.stroke(
                            &canvas::Path::line(Point::new(x, vy - 2.0), Point::new(x, cy + 9.0)),
                            canvas::Stroke::default().with_color(ink).with_width(1.2),
                        );
                        f.fill(&canvas::Path::circle(Point::new(x, cy), 8.5), CARD.get());
                        f.stroke(
                            &canvas::Path::circle(Point::new(x, cy), 8.5),
                            canvas::Stroke::default().with_color(ink).with_width(1.5),
                        );
                        f.fill_text(canvas::Text {
                            content: n.to_string(),
                            position: Point::new(x, cy - 6.5),
                            color: ink,
                            size: 10.5.into(),
                            font: BOLD,
                            align_x: iced::widget::text::Alignment::Center,
                            ..canvas::Text::default()
                        });
                    }
                }
                Band::Memory => {
                    let most = self
                        .buckets
                        .iter()
                        .map(|b| b.compositor_mb.max(b.shell_mb))
                        .fold(0.0, f32::max);
                    let max = nice(
                        most * 1.1,
                        &[100.0, 150.0, 200.0, 300.0, 500.0, 1000.0, 2000.0, 4000.0],
                    );
                    scale(&mut f, max, &[0.0, max / 2.0, max]);
                    for (pick, colour) in [(0, COMPOSITOR), (1, SHELL)] {
                        let path = canvas::Path::new(|p| {
                            let mut drawing = false;
                            for (i, b) in self.buckets.iter().enumerate() {
                                if !b.measured {
                                    drawing = false;
                                    continue;
                                }
                                let v = if pick == 0 {
                                    b.compositor_mb
                                } else {
                                    b.shell_mb
                                };
                                let at =
                                    Point::new(LEFT + (i as f32 + 0.5) * bw, bottom - v / max * h);
                                if drawing {
                                    p.line_to(at);
                                } else {
                                    p.move_to(at);
                                    drawing = true;
                                }
                            }
                        });
                        f.stroke(
                            &path,
                            canvas::Stroke::default().with_color(colour).with_width(2.0),
                        );
                    }
                    let [c1, s1] = self.memory_end;
                    let [_, s0] = self.memory_start;
                    write(
                        &mut f,
                        words::compositor_mb(lang, c1.round()),
                        Point::new(LEFT + 6.0, bottom - c1 / max * h - 16.0),
                        10.5,
                        COMPOSITOR,
                        false,
                    );
                    write(
                        &mut f,
                        words::shell_mb(lang, s0.round(), s1.round()),
                        Point::new(LEFT + 6.0, (bottom - s0 / max * h + 3.0).min(bottom - 13.0)),
                        10.5,
                        SHELL,
                        false,
                    );
                    if let Some((a, b, [k0, k1], who)) = self.growth {
                        let offset = 7.0;
                        let (ya, yb) = (
                            bottom - k0 as f32 / 1024.0 / max * h - offset,
                            bottom - k1 as f32 / 1024.0 / max * h - offset,
                        );
                        let dashed = canvas::Stroke {
                            line_dash: canvas::LineDash {
                                segments: &[5.0, 4.0],
                                offset: 0,
                            },
                            ..canvas::Stroke::default()
                                .with_color(THRESHOLD)
                                .with_width(1.5)
                        };
                        f.stroke(
                            &canvas::Path::line(
                                Point::new(self.x(a, width), ya),
                                Point::new(self.x(b, width), yb),
                            ),
                            dashed,
                        );
                        let whose = text::who(who, lang);
                        write(
                            &mut f,
                            words::only_grows(lang, whose),
                            Point::new(width, yb + offset + 4.0),
                            10.5,
                            THRESHOLD,
                            true,
                        );
                    }
                }
                Band::Frames => {
                    let most = self.buckets.iter().map(|b| b.fps).fold(0.0, f32::max);
                    let max = nice(most, &[10.0, 20.0, 30.0, 60.0, 120.0, 240.0]);
                    scale(&mut f, max, &[0.0, max]);
                    for (i, b) in self.buckets.iter().enumerate() {
                        let v = (b.fps / max * h).min(h);
                        if v > 0.0 {
                            f.fill_rectangle(
                                Point::new(LEFT + i as f32 * bw + 0.5, bottom - v),
                                Size::new((bw - 1.0).max(1.0), v),
                                FRAMES,
                            );
                        }
                    }
                }
                Band::Events => {
                    let mid = top + h / 2.0 + 6.0;
                    for (i, b) in self.buckets.iter().enumerate() {
                        if b.events > 0 {
                            let x = LEFT + (i as f32 + 0.5) * bw;
                            f.fill_rectangle(
                                Point::new(x - 0.75, mid - 5.0),
                                Size::new(1.5, 10.0),
                                fade(TEXT.get(), 0.4),
                            );
                        }
                    }
                    // Labels that do not run into each other.
                    let mut last_end = f32::MIN;
                    for (at, said) in &self.labels {
                        let size = 10.5;
                        let wide = measure(said, size, MANROPE);
                        let x = (self.x(*at, width) - wide / 2.0)
                            .max(LEFT)
                            .min(width - wide);
                        if x < last_end + 6.0 {
                            continue;
                        }
                        last_end = x + wide;
                        write(
                            &mut f,
                            said.clone(),
                            Point::new(x, top - 1.0),
                            size,
                            TEXT.get(),
                            false,
                        );
                    }
                }
                Band::Power => {
                    let any = self.buckets.iter().any(|b| b.watts.is_some());
                    if any {
                        let most = self
                            .buckets
                            .iter()
                            .filter_map(|b| b.watts)
                            .fold(0.0, f32::max);
                        let max = nice(most * 1.1, &[5.0, 10.0, 20.0, 40.0, 80.0, 160.0]);
                        scale(&mut f, max, &[0.0, max]);
                        let path = canvas::Path::new(|p| {
                            let mut drawing = false;
                            for (i, b) in self.buckets.iter().enumerate() {
                                let Some(watts) = b.watts else {
                                    drawing = false;
                                    continue;
                                };
                                let at = Point::new(
                                    LEFT + (i as f32 + 0.5) * bw,
                                    bottom - watts / max * h,
                                );
                                if drawing {
                                    p.line_to(at);
                                } else {
                                    p.move_to(at);
                                    drawing = true;
                                }
                            }
                        });
                        f.stroke(
                            &path,
                            canvas::Stroke::default()
                                .with_color(SOFT.get())
                                .with_width(2.0),
                        );
                    } else {
                        f.fill_rectangle(
                            Point::new(LEFT, bottom),
                            Size::new(width - LEFT, 1.0),
                            fade(TEXT.get(), 0.14),
                        );
                    }
                    let gaps = self.buckets.iter().any(|b| b.measured && b.watts.is_none());
                    if gaps {
                        write(
                            &mut f,
                            w(words::ON_AC_NOT_MEASURED).get(lang).to_owned(),
                            Point::new(LEFT + (width - LEFT) / 2.0, top + h / 2.0 - 7.0),
                            10.5,
                            faint,
                            false,
                        );
                    }
                }
            }
            y = bottom + 10.0;
        }
        for (at, said) in &self.hours {
            f.fill_text(canvas::Text {
                content: said.clone(),
                position: Point::new(self.x(*at, width), y),
                color: faint,
                size: 10.5.into(),
                font: MANROPE,
                align_x: iced::widget::text::Alignment::Center,
                ..canvas::Text::default()
            });
        }
        vec![f.into_geometry()]
    }
}
