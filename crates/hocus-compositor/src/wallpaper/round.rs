//! A folder's pictures in turn (#894): the round they come in, shuffled
//! or by name, the folder's changes taken in (a picture added joins the
//! round, one removed leaves it), what is kept of it between sessions,
//! and when the first change of a session comes.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::config::Order;

/// The pictures of a folder in the order they show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Round {
    order: Order,
    /// This round's pictures, in their order.
    list: Vec<PathBuf>,
    /// The one shown: its place in `list`.
    at: usize,
    /// xorshift64*'s state, never 0.
    rng: u64,
}

impl Round {
    /// A first round of `pictures`, the first showing.
    pub fn new(mut pictures: Vec<PathBuf>, order: Order, seed: u64) -> Self {
        pictures.sort();
        let mut round = Self {
            order,
            list: pictures,
            at: 0,
            rng: seed | 1,
        };
        if order == Order::Shuffle {
            round.shuffle();
        }
        round
    }

    /// The round kept from another session, the folder holding
    /// `pictures` now.
    pub fn again(kept: &Kept, order: Order, pictures: Vec<PathBuf>, seed: u64) -> Self {
        if kept.order != order {
            let mut round = Self::new(pictures, order, seed);
            if let Some(shown) = kept.list.get(kept.at) {
                round.show(shown);
            }
            return round;
        }
        let mut round = Self {
            order,
            list: kept.list.clone(),
            at: kept.at.min(kept.list.len().saturating_sub(1)),
            rng: seed | 1,
        };
        round.update(pictures);
        round
    }

    /// What is kept of it.
    pub fn kept(&self, changed: u64) -> Kept {
        Kept {
            order: self.order,
            list: self.list.clone(),
            at: self.at,
            changed,
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.list.len()
    }

    /// The picture shown; none in an empty folder.
    pub fn current(&self) -> Option<&Path> {
        self.list.get(self.at).map(PathBuf::as_path)
    }

    /// The next picture: the round's next, or past its end a new round's
    /// first (shuffled again, never the one just shown).
    pub fn advance(&mut self) -> Option<&Path> {
        if self.at + 1 < self.list.len() {
            self.at += 1;
        } else {
            let last = self.list.get(self.at).cloned();
            if self.order == Order::Shuffle && self.list.len() > 1 {
                self.shuffle();
                if self.list.first() == last.as_ref() {
                    let end = self.list.len() - 1;
                    self.list.swap(0, end);
                }
            }
            self.at = 0;
        }
        self.current()
    }

    /// What [`Self::advance`] would show, without moving: the picture
    /// decoded ahead.
    pub fn upcoming(&self) -> Option<PathBuf> {
        let mut ahead = self.clone();
        ahead.advance().map(Path::to_path_buf)
    }

    /// The folder holds `pictures` now: those added join the round (by
    /// name in their place; shuffled, somewhere among those still to
    /// come), those removed leave it. Whether the one shown left: the
    /// next one shows in its place.
    pub fn update(&mut self, pictures: Vec<PathBuf>) -> bool {
        let there: HashSet<&PathBuf> = pictures.iter().collect();
        let shown = self.current().map(Path::to_path_buf);
        let left = shown.as_ref().is_some_and(|s| !there.contains(s));
        let before = self
            .list
            .get(..self.at)
            .unwrap_or_default()
            .iter()
            .filter(|p| there.contains(p))
            .count();
        self.list.retain(|p| there.contains(p));
        let known: HashSet<PathBuf> = self.list.iter().cloned().collect();
        let added: Vec<PathBuf> = pictures
            .iter()
            .filter(|p| !known.contains(*p))
            .cloned()
            .collect();
        match self.order {
            Order::Name => {
                self.list.extend(added);
                self.list.sort();
                self.at = match &shown {
                    Some(shown) if !left => self.list.binary_search(shown).unwrap_or(0),
                    Some(shown) => self.list.partition_point(|p| p < shown),
                    None => 0,
                };
            }
            Order::Shuffle => {
                self.at = before;
                for picture in added {
                    // After the one shown: it shows in this round.
                    let from = if self.list.is_empty() || left {
                        self.at
                    } else {
                        self.at + 1
                    };
                    let span = (self.list.len() + 1).saturating_sub(from).max(1);
                    let place = from + (self.random() % span as u64) as usize;
                    self.list.insert(place.min(self.list.len()), picture);
                }
            }
        }
        if self.at >= self.list.len() {
            self.at = 0;
        }
        left
    }

    /// `picture` shows now, if the round has it.
    pub fn show(&mut self, picture: &Path) -> bool {
        match self.list.iter().position(|p| p == picture) {
            Some(at) => {
                self.at = at;
                true
            }
            None => false,
        }
    }

    /// The order changed: the round starts again in it from the one
    /// shown.
    pub fn reorder(&mut self, order: Order) {
        if order == self.order {
            return;
        }
        let shown = self.current().map(Path::to_path_buf);
        self.order = order;
        self.list.sort();
        if order == Order::Shuffle {
            self.shuffle();
        }
        self.at = 0;
        if let Some(shown) = shown {
            self.show(&shown);
            if order == Order::Shuffle {
                // The one shown first: the others all to come.
                let at = self.at;
                self.list.swap(0, at);
                self.at = 0;
            }
        }
    }

    fn shuffle(&mut self) {
        for i in (1..self.list.len()).rev() {
            let j = (self.random() % (i as u64 + 1)) as usize;
            self.list.swap(i, j);
        }
    }

    /// xorshift64*: enough to shuffle pictures.
    fn random(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// A round as kept between sessions, in
/// `$XDG_STATE_HOME/hocuspanda/wallpaper.json` by folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    #[serde(with = "order_word")]
    pub order: Order,
    pub list: Vec<PathBuf>,
    pub at: usize,
    /// When its picture last changed: seconds since 1970 (the wall clock:
    /// a session's monotonic clock does not outlive it).
    pub changed: u64,
}

mod order_word {
    use crate::config::Order;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(order: &Order, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(match order {
            Order::Shuffle => "shuffle",
            Order::Name => "name",
        })
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Order, D::Error> {
        Ok(match String::deserialize(d)?.as_str() {
            "name" => Order::Name,
            _ => Order::Shuffle,
        })
    }
}

/// When a folder's picture first changes in a session: whether at once,
/// and in how long next. `every` is the interval (none: once as each
/// session opens); `since` how long ago it last changed in a session
/// before, if one kept its round.
pub fn first_change(every: Option<Duration>, since: Option<Duration>) -> (bool, Option<Duration>) {
    match (every, since) {
        // A new session: the next picture, then none until the next.
        (None, Some(_)) => (true, None),
        (None, None) => (false, None),
        (Some(every), Some(since)) if since >= every => (true, Some(every)),
        (Some(every), Some(since)) => (false, Some(every - since)),
        (Some(every), None) => (false, Some(every)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pictures(names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|n| PathBuf::from(format!("/f/{n}.png")))
            .collect()
    }

    fn name(path: Option<&Path>) -> String {
        path.and_then(Path::file_stem)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The names shown by `n` changes from the one shown now.
    fn shown(round: &mut Round, n: usize) -> Vec<String> {
        let mut out = vec![name(round.current())];
        for _ in 0..n {
            out.push(name(round.advance()));
        }
        out
    }

    #[test]
    fn by_name_the_pictures_come_in_their_order_then_again() {
        let mut round = Round::new(pictures(&["c", "a", "b"]), Order::Name, 7);
        assert_eq!(shown(&mut round, 4), ["a", "b", "c", "a", "b"]);
        assert_eq!(name(round.upcoming().as_deref()), "c");
    }

    #[test]
    fn by_name_one_added_joins_in_its_place_and_one_removed_leaves() {
        let mut round = Round::new(pictures(&["a", "c", "e"]), Order::Name, 7);
        round.advance();
        assert_eq!(name(round.current()), "c");
        assert!(!round.update(pictures(&["a", "c", "d", "e"])));
        assert_eq!(shown(&mut round, 3), ["c", "d", "e", "a"]);
        // The one shown removed: the next in its place.
        round.advance();
        round.advance();
        assert_eq!(name(round.current()), "d");
        assert!(round.update(pictures(&["a", "c", "e"])));
        assert_eq!(shown(&mut round, 2), ["e", "a", "c"]);
    }

    #[test]
    fn shuffled_each_round_shows_every_picture_once_never_twice_in_a_row() {
        let names = ["a", "b", "c", "d", "e", "f"];
        let mut round = Round::new(pictures(&names), Order::Shuffle, 42);
        for _ in 0..5 {
            let mut seen: Vec<String> = vec![name(round.current())];
            for _ in 1..names.len() {
                seen.push(name(round.advance()));
            }
            let mut sorted = seen.clone();
            sorted.sort();
            assert_eq!(sorted, names, "{seen:?}");
            // The next round's first is not the last one shown.
            let next = name(round.advance());
            assert_ne!(Some(&next), seen.last(), "twice in a row: {seen:?}");
        }
    }

    #[test]
    fn shuffled_one_added_shows_in_this_round_and_one_removed_never() {
        let mut round = Round::new(pictures(&["a", "b", "c", "d"]), Order::Shuffle, 3);
        let first = name(round.current());
        assert!(!round.update(pictures(&["a", "b", "c", "d", "new"])));
        assert_eq!(name(round.current()), first);
        let rest: Vec<String> = (0..4).map(|_| name(round.advance())).collect();
        assert!(rest.contains(&"new".to_owned()), "{rest:?}");
        // "b" removed: never again, however long.
        let was_b = name(round.current()) == "b";
        assert_eq!(round.update(pictures(&["a", "c", "d", "new"])), was_b);
        for _ in 0..20 {
            assert_ne!(name(round.advance()), "b");
        }
        assert_eq!(round.len(), 4);
    }

    #[test]
    fn a_kept_round_goes_on_where_it_was() {
        let mut round = Round::new(pictures(&["a", "b", "c", "d"]), Order::Shuffle, 9);
        round.advance();
        let kept = round.kept(1_700_000_000);
        let shown_then = name(round.current());
        let again = Round::again(&kept, Order::Shuffle, pictures(&["a", "b", "c", "d"]), 1);
        assert_eq!(name(again.current()), shown_then);
        // In another order: it starts again from the one shown.
        let by_name = Round::again(&kept, Order::Name, pictures(&["a", "b", "c", "d"]), 1);
        assert_eq!(name(by_name.current()), shown_then);
        let text = serde_json::to_string(&kept).unwrap_or_default();
        assert_eq!(serde_json::from_str::<Kept>(&text).ok(), Some(kept));
    }

    #[test]
    fn an_empty_folder_shows_nothing_until_a_picture_comes() {
        let mut round = Round::new(Vec::new(), Order::Shuffle, 1);
        assert_eq!(round.current(), None);
        assert_eq!(round.advance(), None);
        round.update(pictures(&["a"]));
        assert_eq!(name(round.current()), "a");
    }

    #[test]
    fn the_first_change_of_a_session() {
        let hour = Some(Duration::from_secs(3600));
        // Nothing kept: after a whole interval.
        assert_eq!(first_change(hour, None), (false, hour));
        // Changed 20 min ago: in 40.
        assert_eq!(
            first_change(hour, Some(Duration::from_secs(1200))),
            (false, Some(Duration::from_secs(2400)))
        );
        // Two hours ago: at once, then every hour.
        assert_eq!(
            first_change(hour, Some(Duration::from_secs(7200))),
            (true, hour)
        );
        // Each session: at once when one came before, never after.
        assert_eq!(
            first_change(None, Some(Duration::from_secs(5))),
            (true, None)
        );
        assert_eq!(first_change(None, None), (false, None));
    }
}
