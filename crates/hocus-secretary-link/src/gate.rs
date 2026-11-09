//! The shell's speak gate (docs/design/secretary.md §6.3): she asks to
//! speak ([`Speak`]); the gate answers now, held or dropped from the
//! screen's state, which the shell already has ([`Screen`]), and her
//! restraint's settings ([`Restraint`]). Here, apart from the shell, so
//! that it is tested on a clock of its own: it owns no thread and reads
//! no clock; the shell's thread gives it the time and wakes it at
//! [`Gate::next`].
//!
//! | Condition | Effect |
//! |---|---|
//! | a meeting takes time now | held (VIPs too) |
//! | do not disturb | held (VIPs too) |
//! | the screen shared or recorded | held |
//! | the camera or the microphone in use | held |
//! | locked, or the idle curtain | held |
//! | a full-screen window on the screen | held |
//! | typing (a key within 20 s) | held, at most 10 min, then dropped |
//! | quiet hours of its notebook | dropped: the next briefing |
//! | a perso item in a meeting, a share, a camera call | held (VIPs too) |
//! | more than 6 today, or one less than 30 min ago | held (VIPs pass) |
//! | her peek already out | held: one at a time, never stacked |
//!
//! Past its deadline an item is dropped (it goes to the next briefing).
//! A held item is shown when what holds it ends, if it can still be
//! acted on. Her briefings ("Ton point est prêt", "Pendant ton absence")
//! come at the user's own first session: quiet hours do not drop them.

use jiff::{SignedDuration, Timestamp, civil};
use serde::{Deserialize, Serialize};

use crate::{Notebook, Speak, SpeakKind};

/// Typing: a key within this long.
pub const TYPING: SignedDuration = SignedDuration::from_secs(20);
/// Held for typing at most this long; then it goes to the next briefing.
pub const TYPING_MOST: SignedDuration = SignedDuration::from_mins(10);

/// Hours from `from` to `to` (local time), across midnight when `to` is
/// earlier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hours {
    pub from: civil::Time,
    pub to: civil::Time,
}

impl Hours {
    pub const fn new(from: (i8, i8), to: (i8, i8)) -> Self {
        Self {
            from: civil::Time::constant(from.0, from.1, 0, 0),
            to: civil::Time::constant(to.0, to.1, 0, 0),
        }
    }

    pub fn contains(&self, time: civil::Time) -> bool {
        if self.from <= self.to {
            self.from <= time && time < self.to
        } else {
            time >= self.from || time < self.to
        }
    }

    /// `from-to` as `HH:MM-HH:MM`.
    pub fn of(text: &str) -> Option<Self> {
        let (from, to) = text.split_once('-')?;
        Some(Self {
            from: from.trim().parse().ok()?,
            to: to.trim().parse().ok()?,
        })
    }
}

/// Her restraint (`[secretary.speak]`), as she sends it to the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Restraint {
    /// Peeks a day at most (VIPs aside).
    pub most: u32,
    /// Minutes between two at least (VIPs aside).
    pub apart: u32,
    /// Quiet hours of the pro notebook, and its weekends.
    pub pro: Hours,
    pub pro_weekends: bool,
    /// Quiet hours of the perso notebook.
    pub perso: Hours,
    /// How long her peek stays out, in seconds.
    pub stays: u32,
}

impl Default for Restraint {
    fn default() -> Self {
        Self {
            most: 6,
            apart: 30,
            pro: Hours::new((19, 0), (8, 0)),
            pro_weekends: true,
            perso: Hours::new((22, 0), (8, 0)),
            stays: 12,
        }
    }
}

impl Restraint {
    /// Quiet hours for `notebook` at local time `at`.
    pub fn quiet(&self, notebook: Notebook, at: civil::DateTime) -> bool {
        match notebook {
            Notebook::Pro => {
                let weekend = matches!(
                    at.date().weekday(),
                    civil::Weekday::Saturday | civil::Weekday::Sunday
                );
                (self.pro_weekends && weekend) || self.pro.contains(at.time())
            }
            Notebook::Perso => self.perso.contains(at.time()),
        }
    }
}

/// What the shell knows of the screen, as it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Screen {
    /// A meeting of the featured calendars takes time now, until then.
    pub meeting_until: Option<Timestamp>,
    pub dnd: bool,
    /// The screen shared or recorded.
    pub shared: bool,
    /// The camera or the microphone in use.
    pub camera: bool,
    /// Locked, or the idle curtain down.
    pub locked: bool,
    /// A full-screen window on the screen she would come out on.
    pub fullscreen: bool,
    /// The last key typed.
    pub typed: Option<Timestamp>,
}

impl Screen {
    pub fn meeting(&self, now: Timestamp) -> bool {
        self.meeting_until.is_some_and(|end| end > now)
    }

    /// The moment it is (§7.4).
    pub fn moment(&self, now: Timestamp) -> crate::Moment {
        if self.shared {
            crate::Moment::Sharing
        } else if self.camera {
            crate::Moment::Camera
        } else if self.meeting(now) {
            crate::Moment::Meeting
        } else {
            crate::Moment::Quiet
        }
    }

    fn typing(&self, now: Timestamp) -> bool {
        self.typed
            .is_some_and(|at| at <= now && now.duration_since(at) < TYPING)
    }
}

/// Why an item is held or dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Why {
    Meeting,
    Dnd,
    Shared,
    Camera,
    Locked,
    Fullscreen,
    Typing,
    Quiet,
    /// A perso item in a public moment.
    Perso,
    /// Six today already.
    Count,
    /// One less than 30 min ago.
    Apart,
    /// Her peek is out.
    Out,
    /// Past its deadline.
    Late,
}

impl Why {
    pub const fn word(self) -> &'static str {
        match self {
            Self::Meeting => "meeting",
            Self::Dnd => "dnd",
            Self::Shared => "shared",
            Self::Camera => "camera",
            Self::Locked => "locked",
            Self::Fullscreen => "fullscreen",
            Self::Typing => "typing",
            Self::Quiet => "quiet",
            Self::Perso => "perso",
            Self::Count => "count",
            Self::Apart => "apart",
            Self::Out => "out",
            Self::Late => "late",
        }
    }
}

/// The gate's answer for an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Now,
    Held(Why),
    Dropped(Why),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Waiting {
    speak: Speak,
    /// Since when it is held for typing, if it is.
    typing_since: Option<Timestamp>,
}

/// The gate: what it holds, what came out today.
#[derive(Debug, Clone, Default)]
pub struct Gate {
    pub restraint: Restraint,
    pub screen: Screen,
    held: Vec<Waiting>,
    /// When her peeks came out, today (local day), VIPs' too.
    shown: Vec<(Timestamp, bool)>,
    day: Option<civil::Date>,
    /// Her peek out now.
    out: Option<u64>,
}

/// What happened to an item at a step: shown now, held, or dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decided {
    pub speak: Speak,
    pub verdict: Verdict,
}

impl Gate {
    pub fn new(restraint: Restraint) -> Self {
        Self {
            restraint,
            ..Self::default()
        }
    }

    /// Her peek out, if one is.
    pub fn out(&self) -> Option<u64> {
        self.out
    }

    /// The items held, oldest first.
    pub fn held(&self) -> impl Iterator<Item = &Speak> {
        self.held.iter().map(|w| &w.speak)
    }

    /// Peeks shown today.
    pub fn shown_today(&self) -> usize {
        self.shown.len()
    }

    fn new_day(&mut self, local: civil::DateTime) {
        if self.day != Some(local.date()) {
            self.day = Some(local.date());
            self.shown.clear();
        }
    }

    /// Why `speak` may not come out at `now` (`local` its local time),
    /// and whether that drops it; none: it may.
    fn why(&self, speak: &Speak, now: Timestamp, local: civil::DateTime) -> Option<(Why, bool)> {
        let deadline = speak
            .deadline
            .as_deref()
            .and_then(|d| d.parse::<Timestamp>().ok());
        if deadline.is_some_and(|d| d <= now) {
            return Some((Why::Late, true));
        }
        let briefing = matches!(speak.kind, SpeakKind::Briefing | SpeakKind::Absence);
        if !briefing && self.restraint.quiet(speak.notebook, local) {
            return Some((Why::Quiet, true));
        }
        let s = &self.screen;
        if s.locked {
            return Some((Why::Locked, false));
        }
        if s.meeting(now) {
            return Some((Why::Meeting, false));
        }
        if s.dnd {
            return Some((Why::Dnd, false));
        }
        if s.shared {
            return Some((Why::Shared, false));
        }
        if s.camera {
            return Some((Why::Camera, false));
        }
        if speak.notebook == Notebook::Perso && s.moment(now).public() {
            return Some((Why::Perso, false));
        }
        if s.fullscreen {
            return Some((Why::Fullscreen, false));
        }
        if s.typing(now) {
            return Some((Why::Typing, false));
        }
        if self.out.is_some() {
            return Some((Why::Out, false));
        }
        if !speak.vip {
            let counted = self.shown.iter().filter(|(_, vip)| !vip).count();
            if counted >= self.restraint.most as usize {
                return Some((Why::Count, false));
            }
            let apart = SignedDuration::from_mins(i64::from(self.restraint.apart));
            if self
                .shown
                .iter()
                .any(|(at, _)| *at <= now && now.duration_since(*at) < apart)
            {
                return Some((Why::Apart, false));
            }
        }
        None
    }

    /// She asks to speak: now, held (kept here) or dropped.
    pub fn ask(&mut self, speak: Speak, now: Timestamp, local: civil::DateTime) -> Verdict {
        self.new_day(local);
        self.held.retain(|w| w.speak.id != speak.id);
        // Those held before it go first.
        let first = self.held.is_empty() || speak.vip;
        match self.why(&speak, now, local) {
            None if first => {
                self.came_out(&speak, now);
                Verdict::Now
            }
            None => {
                self.held.push(Waiting {
                    speak,
                    typing_since: None,
                });
                Verdict::Held(Why::Out)
            }
            Some((why, true)) => Verdict::Dropped(why),
            Some((why, false)) => {
                let typing_since = (why == Why::Typing).then_some(now);
                self.held.push(Waiting {
                    speak,
                    typing_since,
                });
                Verdict::Held(why)
            }
        }
    }

    fn came_out(&mut self, speak: &Speak, now: Timestamp) {
        self.out = Some(speak.id);
        self.shown.push((now, speak.vip));
    }

    /// Her peek went back in.
    pub fn back(&mut self, id: u64) {
        if self.out == Some(id) {
            self.out = None;
        }
    }

    /// She no longer wants to say `id` (done meanwhile).
    pub fn withdraw(&mut self, id: u64) {
        self.held.retain(|w| w.speak.id != id);
    }

    /// What changed (the screen, the time): the held items looked at
    /// again, oldest first (VIPs before). At most one comes out; those
    /// that can no longer be acted on, or held for typing too long, are
    /// dropped.
    pub fn step(&mut self, now: Timestamp, local: civil::DateTime) -> Vec<Decided> {
        self.new_day(local);
        let mut out = Vec::new();
        let mut kept = Vec::new();
        let mut held = std::mem::take(&mut self.held);
        // VIPs first, then by age.
        held.sort_by_key(|w| !w.speak.vip);
        for mut waiting in held {
            match self.why(&waiting.speak, now, local) {
                None => {
                    self.came_out(&waiting.speak, now);
                    out.push(Decided {
                        speak: waiting.speak,
                        verdict: Verdict::Now,
                    });
                }
                Some((why, true)) => out.push(Decided {
                    speak: waiting.speak,
                    verdict: Verdict::Dropped(why),
                }),
                Some((Why::Typing, false)) => {
                    let since = *waiting.typing_since.get_or_insert(now);
                    if now.duration_since(since) >= TYPING_MOST {
                        out.push(Decided {
                            speak: waiting.speak,
                            verdict: Verdict::Dropped(Why::Typing),
                        });
                    } else {
                        kept.push(waiting);
                    }
                }
                Some(_) => {
                    waiting.typing_since = None;
                    kept.push(waiting);
                }
            }
        }
        self.held = kept;
        out
    }

    /// When the held items should be looked at again, on the clock alone
    /// (a meeting's end, the spacing, the typing's pause, a deadline,
    /// the end of quiet hours being a drop, not a wait); none: only when
    /// the screen changes. `local` is `now` in local time.
    pub fn next(&self, now: Timestamp) -> Option<Timestamp> {
        if self.held.is_empty() {
            return None;
        }
        let mut at: Vec<Timestamp> = Vec::new();
        if let Some(end) = self.screen.meeting_until.filter(|e| *e > now) {
            at.push(end);
        }
        if let Some(typed) = self.screen.typed {
            at.push(typed + TYPING);
        }
        let apart = SignedDuration::from_mins(i64::from(self.restraint.apart));
        if let Some(last) = self.shown.iter().map(|(t, _)| *t).max() {
            at.push(last + apart);
        }
        for waiting in &self.held {
            if let Some(d) = waiting
                .speak
                .deadline
                .as_deref()
                .and_then(|d| d.parse::<Timestamp>().ok())
            {
                at.push(d);
            }
            if let Some(since) = waiting.typing_since {
                at.push(since + TYPING_MOST);
            }
        }
        at.into_iter().filter(|t| *t > now).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: &str = "2026-11-10";

    fn at(hm: &str) -> (Timestamp, civil::DateTime) {
        let local: civil::DateTime = format!("{DAY}T{hm}:00").parse().expect("a time");
        let now = local
            .to_zoned(jiff::tz::TimeZone::UTC)
            .expect("UTC")
            .timestamp();
        (now, local)
    }

    fn speak(id: u64, notebook: Notebook) -> Speak {
        Speak {
            id,
            kind: SpeakKind::Reminder,
            text: format!("rappel {id}"),
            action: None,
            item: None,
            notebook,
            vip: false,
            deadline: None,
        }
    }

    /// AC12: no peek during each of the conditions; held items come out
    /// once the condition ends, if still actionable, else they go to the
    /// next briefing.
    #[test]
    fn nothing_comes_out_while_the_screen_says_no() {
        let (now, local) = at("10:00");
        type Hold = fn(&mut Screen, Timestamp);
        let holds: [(&str, Hold, Why); 6] = [
            (
                "a busy event",
                |s, now| s.meeting_until = Some(now + SignedDuration::from_mins(30)),
                Why::Meeting,
            ),
            ("do not disturb", |s, _| s.dnd = true, Why::Dnd),
            ("screen sharing", |s, _| s.shared = true, Why::Shared),
            ("the camera", |s, _| s.camera = true, Why::Camera),
            ("locked", |s, _| s.locked = true, Why::Locked),
            ("full screen", |s, _| s.fullscreen = true, Why::Fullscreen),
        ];
        for (what, hold, why) in holds {
            let mut gate = Gate::default();
            hold(&mut gate.screen, now);
            assert_eq!(
                gate.ask(speak(1, Notebook::Pro), now, local),
                Verdict::Held(why),
                "{what}"
            );
            assert!(gate.step(now, local).is_empty(), "{what}: still held");
            gate.screen = Screen::default();
            let later = now + SignedDuration::from_mins(31);
            let (_, later_local) = at("10:31");
            let came = gate.step(later, later_local);
            assert_eq!(came.len(), 1, "{what}");
            assert_eq!(came[0].verdict, Verdict::Now, "{what}: out once it ends");
        }
        // Quiet hours: dropped, for the next briefing.
        let mut gate = Gate::default();
        let (night, night_local) = at("21:00");
        assert_eq!(
            gate.ask(speak(2, Notebook::Pro), night, night_local),
            Verdict::Dropped(Why::Quiet)
        );
        // Held past its deadline: dropped, for the next briefing.
        let mut gate = Gate::default();
        gate.screen.dnd = true;
        let mut late = speak(3, Notebook::Pro);
        late.deadline = Some((now + SignedDuration::from_mins(20)).to_string());
        assert_eq!(gate.ask(late, now, local), Verdict::Held(Why::Dnd));
        assert_eq!(
            gate.next(now),
            Some(now + SignedDuration::from_mins(20)),
            "woken at its deadline"
        );
        let (later, later_local) = at("10:25");
        gate.screen.dnd = false;
        let came = gate.step(later, later_local);
        assert_eq!(came[0].verdict, Verdict::Dropped(Why::Late));
    }

    #[test]
    fn quiet_hours_are_each_notebooks() {
        let restraint = Restraint::default();
        let local = |t: &str| -> civil::DateTime { t.parse().expect("a time") };
        // Tuesday 20:00: pro quiet, perso not; 23:00 both.
        assert!(restraint.quiet(Notebook::Pro, local("2026-11-10T20:00")));
        assert!(!restraint.quiet(Notebook::Perso, local("2026-11-10T20:00")));
        assert!(restraint.quiet(Notebook::Perso, local("2026-11-10T23:00")));
        assert!(!restraint.quiet(Notebook::Pro, local("2026-11-10T08:00")));
        // Saturday noon: pro quiet (weekends), perso not.
        assert!(restraint.quiet(Notebook::Pro, local("2026-11-14T12:00")));
        assert!(!restraint.quiet(Notebook::Perso, local("2026-11-14T12:00")));
        // Her briefings are the user's own first session: never dropped.
        let mut gate = Gate::default();
        let (now, local) = at("07:30");
        let mut briefing = speak(1, Notebook::Pro);
        briefing.kind = SpeakKind::Briefing;
        assert_eq!(gate.ask(briefing, now, local), Verdict::Now);
    }

    /// AC13: of 10 candidates in a day, at most 6 come out, never two
    /// within 30 min; a VIP passes the count but not a meeting.
    #[test]
    fn six_a_day_thirty_minutes_apart() {
        let mut gate = Gate::default();
        let mut shown: Vec<Timestamp> = Vec::new();
        let (start, _) = at("08:00");
        for n in 0..10_i64 {
            // A candidate every 20 minutes from 9 h.
            let now = start + SignedDuration::from_mins(60 + 20 * n);
            let local = now.to_zoned(jiff::tz::TimeZone::UTC).datetime();
            let verdict = gate.ask(speak(n as u64 + 1, Notebook::Pro), now, local);
            if verdict == Verdict::Now {
                shown.push(now);
                gate.back(n as u64 + 1);
            }
            // What was held comes out when it may.
            for decided in gate.step(now, local) {
                if decided.verdict == Verdict::Now {
                    shown.push(now);
                    gate.back(decided.speak.id);
                }
            }
        }
        // The rest of the day, every five minutes.
        for m in 0..120_i64 {
            let now = start + SignedDuration::from_mins(60 + 200 + 5 * m);
            let local = now.to_zoned(jiff::tz::TimeZone::UTC).datetime();
            for decided in gate.step(now, local) {
                if decided.verdict == Verdict::Now {
                    shown.push(now);
                    gate.back(decided.speak.id);
                }
            }
        }
        assert!(shown.len() <= 6, "{} came out", shown.len());
        assert_eq!(shown.len(), 6, "six of ten came out over the day");
        for pair in shown.windows(2) {
            assert!(
                pair[1].duration_since(pair[0]) >= SignedDuration::from_mins(30),
                "two within 30 min: {pair:?}"
            );
        }
        // A VIP passes the count…
        let (now, local) = at("15:00");
        let mut vip = speak(99, Notebook::Pro);
        vip.vip = true;
        assert_eq!(gate.ask(vip.clone(), now, local), Verdict::Now);
        gate.back(99);
        // …but not a meeting.
        gate.screen.meeting_until = Some(now + SignedDuration::from_mins(60));
        vip.id = 100;
        assert_eq!(gate.ask(vip, now, local), Verdict::Held(Why::Meeting));
    }

    /// AC38, the peek's half: during a meeting, a share or a camera call,
    /// a perso item is held, a VIP's too; it comes out after.
    #[test]
    fn perso_waits_for_the_public_moment_to_end() {
        let (now, local) = at("11:00");
        let mut gate = Gate::default();
        gate.screen.camera = true;
        let mut perso = speak(1, Notebook::Perso);
        perso.vip = true;
        assert_eq!(gate.ask(perso, now, local), Verdict::Held(Why::Camera));
        gate.screen.camera = false;
        let came = gate.step(now, local);
        assert_eq!(came[0].verdict, Verdict::Now);
        // The moment itself: a meeting is public.
        gate.screen.meeting_until = Some(now + SignedDuration::from_mins(10));
        assert_eq!(gate.screen.moment(now), crate::Moment::Meeting);
    }

    #[test]
    fn one_at_a_time_and_typing_holds_at_most_ten_minutes() {
        let (now, local) = at("10:00");
        let mut gate = Gate::default();
        let mut vip = speak(1, Notebook::Pro);
        vip.vip = true;
        assert_eq!(gate.ask(vip, now, local), Verdict::Now);
        let mut other = speak(2, Notebook::Pro);
        other.vip = true;
        assert_eq!(gate.ask(other, now, local), Verdict::Held(Why::Out));
        gate.back(1);
        assert_eq!(gate.step(now, local)[0].verdict, Verdict::Now);
        gate.back(2);
        // Typing: held while keys come, then dropped after ten minutes.
        let mut gate = Gate::default();
        gate.screen.typed = Some(now);
        assert_eq!(
            gate.ask(speak(3, Notebook::Pro), now, local),
            Verdict::Held(Why::Typing)
        );
        let mut t = now;
        let mut dropped = false;
        for _ in 0..40 {
            t += SignedDuration::from_secs(15);
            gate.screen.typed = Some(t);
            let local = t.to_zoned(jiff::tz::TimeZone::UTC).datetime();
            if gate
                .step(t, local)
                .iter()
                .any(|d| d.verdict == Verdict::Dropped(Why::Typing))
            {
                dropped = true;
                assert!(t.duration_since(now) >= TYPING_MOST);
                break;
            }
        }
        assert!(dropped, "held for typing at most ten minutes");
    }

    #[test]
    fn hours_read_and_cross_midnight() {
        let hours = Hours::of("22:00-08:00").expect("hours");
        assert!(hours.contains(civil::time(23, 0, 0, 0)));
        assert!(hours.contains(civil::time(7, 59, 0, 0)));
        assert!(!hours.contains(civil::time(8, 0, 0, 0)));
        assert!(!hours.contains(civil::time(12, 0, 0, 0)));
        assert_eq!(Hours::of("nope"), None);
    }
}
