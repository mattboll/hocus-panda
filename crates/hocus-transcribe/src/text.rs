//! The meeting's file: `# title`, the date and times, the calendar's
//! attendees, then one line a turn, `**14:01 · Voix 1** — text`, the
//! consecutive lines of one speaker together. The user's lines that repeat
//! what the call said at the same time are left out: without headphones,
//! the microphone hears the others too.

use hocus_meetings::{Heard as Who, Record};
use hocus_words::{Lang, transcribe as txt};
use jiff::{Timestamp, Zoned, tz::TimeZone};

/// Who speaks a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    /// The user's microphone.
    Me,
    /// A voice of the call, told apart: 1, 2…
    Voice(u32),
    /// The call, its voices not told apart.
    Others,
}

impl Speaker {
    fn name(self, lang: Lang) -> String {
        match self {
            Self::Me => txt::ME.get(lang).to_owned(),
            Self::Voice(n) => txt::voice(lang, n),
            Self::Others => txt::OTHERS.get(lang).to_owned(),
        }
    }
}

/// A line: from `start` to `end` (seconds from the tracks' start), who,
/// what.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub start: f64,
    pub end: f64,
    pub speaker: Speaker,
    pub text: String,
}

/// Lowercase words of `text`, letters and digits only.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// How much of `a`'s words `b` says too (0 to 1).
fn repeated(a: &str, b: &str) -> f32 {
    let (a, b) = (words(a), words(b));
    if a.is_empty() {
        return 1.0;
    }
    let common = a.iter().filter(|w| b.contains(w)).count();
    common as f32 / a.len() as f32
}

/// The user's lines but those that repeat a line of the call said at the
/// same time (a second either way): the microphone hearing the speakers.
pub fn without_echo(me: Vec<Line>, call: &[Line]) -> Vec<Line> {
    me.into_iter()
        .filter(|line| {
            let said_by_call = call
                .iter()
                .filter(|c| c.start < line.end + 1.0 && line.start < c.end + 1.0)
                .map(|c| c.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            said_by_call.is_empty() || repeated(&line.text, &said_by_call) < 0.6
        })
        .collect()
}

/// Every line in order of time, a speaker's consecutive lines together.
pub fn turns(mut lines: Vec<Line>) -> Vec<Line> {
    lines.retain(|l| !l.text.trim().is_empty());
    lines.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut out: Vec<Line> = Vec::new();
    for line in lines {
        match out.last_mut() {
            Some(last) if last.speaker == line.speaker => {
                last.end = last.end.max(line.end);
                last.text.push(' ');
                last.text.push_str(line.text.trim());
            }
            _ => out.push(Line {
                text: line.text.trim().to_owned(),
                ..line
            }),
        }
    }
    out
}

/// Who the turns hear, for the notification.
pub fn who(turns: &[Line]) -> Who {
    let mut voices: Vec<u32> = turns
        .iter()
        .filter_map(|l| match l.speaker {
            Speaker::Voice(n) => Some(n),
            _ => None,
        })
        .collect();
    voices.sort_unstable();
    voices.dedup();
    Who {
        me: turns.iter().any(|l| l.speaker == Speaker::Me),
        voices: u32::try_from(voices.len()).unwrap_or(u32::MAX),
        others: turns.iter().any(|l| l.speaker == Speaker::Others),
    }
}

/// "14:00", in `zone`.
fn hhmm(t: Timestamp, zone: &TimeZone) -> String {
    t.to_zoned(zone.clone()).strftime("%H:%M").to_string()
}

/// The file's name: "2026-11-17 14-00 Point produit.md", from the
/// meeting's start; what a file name cannot hold left out.
pub fn file_name(start: &Zoned, title: &str) -> String {
    let title: String = title
        .chars()
        .map(|c| if c == '/' || c.is_control() { ' ' } else { c })
        .collect();
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let title: String = title.chars().take(120).collect();
    format!("{} {title}.md", start.strftime("%Y-%m-%d %H-%M"))
}

/// The meeting's Markdown: its head, then a paragraph a turn.
pub fn markdown(record: &Record, turns: &[Line], zone: &TimeZone) -> anyhow::Result<String> {
    let lang = Lang::of_code(&record.lang).unwrap_or_default();
    let (start, end): (Timestamp, Timestamp) = (record.start.parse()?, record.end.parse()?);
    let began: Timestamp = record.began.parse()?;
    let day = start.to_zoned(zone.clone());
    let weekday = hocus_words::date::WEEKDAYS
        .get(lang)
        .get(day.weekday().to_monday_zero_offset() as usize)
        .copied()
        .unwrap_or_default();
    let month = hocus_words::date::MONTHS
        .get(lang)
        .get(usize::try_from(day.month() - 1).unwrap_or(0))
        .copied()
        .unwrap_or_default();
    let mut out = format!("# {}\n\n", record.title.trim());
    out.push_str(&txt::date_line(
        lang,
        txt::DateLine {
            weekday: &weekday,
            day: &day.day(),
            month: &month,
            year: &day.year(),
            start: &hhmm(start, zone),
            end: &hhmm(end, zone),
        },
    ));
    out.push('\n');
    if !record.attendees.is_empty() {
        // Two spaces: the attendees on a line of their own, in the same
        // paragraph.
        out.insert_str(out.len() - 1, "  ");
        out.push_str(&txt::attendees(lang, record.attendees.join(", ")));
        out.push('\n');
    }
    for turn in turns {
        let at = began + jiff::SignedDuration::from_secs_f64(turn.start.max(0.0));
        out.push_str(&format!(
            "\n**{} · {}** — {}\n",
            hhmm(at, zone),
            turn.speaker.name(lang),
            turn.text
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(start: f64, speaker: Speaker, text: &str) -> Line {
        Line {
            start,
            end: start + 2.0,
            speaker,
            text: text.to_owned(),
        }
    }

    #[test]
    fn the_microphone_hearing_the_call_is_left_out() {
        let call = [line(10.0, Speaker::Voice(1), "On commence par la démo ?")];
        let me = vec![
            line(10.5, Speaker::Me, "on commence par la démo"),
            line(11.0, Speaker::Me, "Oui, vas-y."),
            line(30.0, Speaker::Me, "On commence par la démo ?"),
        ];
        let kept = without_echo(me, &call);
        assert_eq!(
            kept.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            ["Oui, vas-y.", "On commence par la démo ?"]
        );
    }

    #[test]
    fn a_speakers_consecutive_lines_are_one_turn_in_order() {
        let turns = turns(vec![
            line(5.0, Speaker::Voice(1), "Bonjour."),
            line(0.0, Speaker::Me, "Salut"),
            line(1.0, Speaker::Me, "tout le monde."),
            line(8.0, Speaker::Voice(2), "Hello."),
            line(9.0, Speaker::Voice(1), " "),
        ]);
        let said: Vec<(Speaker, &str)> =
            turns.iter().map(|t| (t.speaker, t.text.as_str())).collect();
        assert_eq!(
            said,
            [
                (Speaker::Me, "Salut tout le monde."),
                (Speaker::Voice(1), "Bonjour."),
                (Speaker::Voice(2), "Hello."),
            ]
        );
        assert_eq!(
            who(&turns),
            Who {
                me: true,
                voices: 2,
                others: false
            }
        );
    }

    #[test]
    fn the_file_in_french_and_its_name() -> anyhow::Result<()> {
        let record = Record {
            title: "Point produit".to_owned(),
            start: "2026-11-17T14:00:00Z".to_owned(),
            end: "2026-11-17T14:32:00Z".to_owned(),
            attendees: vec!["Alex Martin".to_owned(), "Sam".to_owned()],
            began: "2026-11-17T14:00:40Z".to_owned(),
            lang: "fr".to_owned(),
            folder: "/d".into(),
        };
        let turns = turns(vec![
            line(0.0, Speaker::Me, "Bonjour."),
            line(50.0, Speaker::Voice(1), "Salut."),
            line(200.0, Speaker::Others, "On y va."),
        ]);
        let text = markdown(&record, &turns, &TimeZone::UTC)?;
        assert_eq!(
            text,
            "# Point produit\n\nmardi 17 novembre 2026, 14:00 – 14:32  \n\
             Invités (agenda) : Alex Martin, Sam\n\n\
             **14:00 · Moi** — Bonjour.\n\n\
             **14:01 · Voix 1** — Salut.\n\n\
             **14:04 · Les autres** — On y va.\n"
        );
        let start: Timestamp = record.start.parse()?;
        assert_eq!(
            file_name(&start.to_zoned(TimeZone::UTC), "Point / produit"),
            "2026-11-17 14-00 Point produit.md"
        );
        Ok(())
    }
}
