//! What screen readers hear without going to the bar: an agent that starts
//! waiting or gets stuck (docs/design/screen-reader.md, "Announcements").
//! The two states that ask for the user, as the halo and the peek; never
//! working, finished or resting. Several within [`GROUP`] make one line;
//! with do not disturb on, none.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use hocus_link::AgentShown;

use super::words::Words;
use crate::agents::Look;

/// Agents that start waiting within this of the first are said together.
pub const GROUP: Duration = Duration::from_secs(2);

/// The agents' states as last seen, and what is to be said.
#[derive(Debug, Default)]
pub struct Announcer {
    /// Each agent's look, by its id; `None` before the first look, which
    /// says nothing (what already waits when a reader starts is in the
    /// bar).
    seen: Option<HashMap<u64, Look>>,
    /// Who started waiting or got stuck, in order, since `since`.
    pending: Vec<(String, Look)>,
    since: Option<Instant>,
}

impl Announcer {
    /// The agents now, at `now`; `quiet`: do not disturb is on.
    pub fn observe(&mut self, agents: &[AgentShown], now: Instant, quiet: bool) {
        let looks: HashMap<u64, Look> = agents.iter().map(|a| (a.id, Look::of(&a.state))).collect();
        if let Some(seen) = &self.seen
            && !quiet
        {
            for agent in agents {
                let look = looks[&agent.id];
                let came = matches!(look, Look::Waiting | Look::Stuck)
                    && seen.get(&agent.id) != Some(&look);
                let name = super::words::plain(&agent.name);
                if came && !self.pending.iter().any(|(n, l)| *n == name && *l == look) {
                    self.pending.push((name, look));
                    self.since.get_or_insert(now);
                }
            }
        }
        if quiet {
            self.pending.clear();
            self.since = None;
        }
        // One that no longer waits by the time it is said is not said.
        self.pending.retain(|(name, look)| {
            agents
                .iter()
                .any(|a| super::words::plain(&a.name) == *name && Look::of(&a.state) == *look)
        });
        if self.pending.is_empty() {
            self.since = None;
        }
        self.seen = Some(looks);
    }

    /// When something is to be said.
    pub fn deadline(&self) -> Option<Instant> {
        self.since.map(|since| since + GROUP)
    }

    /// What is to be said at `now`, if its time came.
    pub fn due(&mut self, now: Instant, words: Words) -> Option<String> {
        if self.deadline().is_none_or(|at| now < at) {
            return None;
        }
        let names = |want: Look| -> Vec<&str> {
            self.pending
                .iter()
                .filter(|(_, look)| *look == want)
                .map(|(name, _)| name.as_str())
                .collect()
        };
        let said = words.announce(&names(Look::Waiting), &names(Look::Stuck));
        self.pending.clear();
        self.since = None;
        (!said.is_empty()).then_some(said)
    }

    /// Nothing listens any more: the next look starts afresh.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(id: u64, name: &str, state: &str) -> AgentShown {
        AgentShown {
            id,
            state: state.to_owned(),
            since: 0,
            source: "test".to_owned(),
            window: 0,
            name: name.to_owned(),
            note: None,
            news: None,
        }
    }

    const FR: Words = Words {
        lang: hocus_words::Lang::French,
    };

    #[test]
    fn an_agent_that_starts_waiting_is_said_once() {
        let t = Instant::now();
        let mut a = Announcer::default();
        a.observe(&[agent(1, "site-web", "working")], t, false);
        assert_eq!(a.due(t + GROUP, FR), None);
        a.observe(&[agent(1, "site-web", "waiting")], t, false);
        assert_eq!(a.due(t, FR), None, "said after the group's time");
        // Seen again, still waiting: nothing more.
        a.observe(
            &[agent(1, "site-web", "waiting")],
            t + Duration::from_millis(500),
            false,
        );
        assert_eq!(a.due(t + GROUP, FR).as_deref(), Some("site-web t'attend."));
        assert_eq!(a.due(t + GROUP * 2, FR), None);
        a.observe(&[agent(1, "site-web", "waiting")], t + GROUP * 2, false);
        assert_eq!(a.due(t + GROUP * 4, FR), None);
    }

    #[test]
    fn finishing_working_or_resting_is_never_said() {
        let t = Instant::now();
        let mut a = Announcer::default();
        a.observe(&[agent(1, "site-web", "waiting")], t, false);
        for state in ["working", "done", "idle", "working"] {
            a.observe(&[agent(1, "site-web", state)], t, false);
            assert_eq!(a.due(t + GROUP, FR), None, "{state}");
        }
    }

    #[test]
    fn two_within_two_seconds_make_one_line() {
        let t = Instant::now();
        let mut a = Announcer::default();
        a.observe(
            &[
                agent(1, "site-web", "working"),
                agent(2, "docs", "working"),
                agent(3, "infra", "working"),
            ],
            t,
            false,
        );
        a.observe(
            &[
                agent(1, "site-web", "waiting"),
                agent(2, "docs", "working"),
                agent(3, "infra", "working"),
            ],
            t,
            false,
        );
        let later = t + Duration::from_millis(1500);
        a.observe(
            &[
                agent(1, "site-web", "waiting"),
                agent(2, "docs", "waiting"),
                agent(3, "infra", "failed"),
            ],
            later,
            false,
        );
        assert_eq!(a.deadline(), Some(t + GROUP));
        assert_eq!(
            a.due(t + GROUP, FR).as_deref(),
            Some("site-web et docs t'attendent. infra est bloqué.")
        );
    }

    #[test]
    fn the_first_look_and_do_not_disturb_say_nothing() {
        let t = Instant::now();
        let mut a = Announcer::default();
        a.observe(&[agent(1, "site-web", "waiting")], t, false);
        assert_eq!(a.deadline(), None);
        a.observe(&[agent(1, "site-web", "working")], t, false);
        a.observe(&[agent(1, "site-web", "waiting")], t, true);
        assert_eq!(a.due(t + GROUP, FR), None);
    }

    #[test]
    fn one_that_stops_waiting_before_it_is_said_is_not() {
        let t = Instant::now();
        let mut a = Announcer::default();
        a.observe(&[agent(1, "site-web", "working")], t, false);
        a.observe(&[agent(1, "site-web", "waiting")], t, false);
        a.observe(&[agent(1, "site-web", "working")], t + GROUP / 2, false);
        assert_eq!(a.due(t + GROUP, FR), None);
    }
}
