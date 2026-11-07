//! The shell's windows on AT-SPI, through AccessKit's adapter.
//!
//! One adapter per window, made as the window shows and dropped as it
//! goes; AccessKit puts them all under one application. The adapters ask
//! for a tree only once the session's accessibility bus says a screen
//! reader (or any assistive technology) is on: until then [`listening`]
//! is false and the shell builds nothing.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use accesskit::{
    ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, NodeId, TreeUpdate,
};

use crate::tree::{Act, Window};

/// An assistive technology listens: trees are wanted.
static LISTENING: AtomicBool = AtomicBool::new(false);
/// Trees built for AccessKit so far.
static BUILT: AtomicU64 = AtomicU64::new(0);

/// Whether an assistive technology listens, so that describing what shows
/// is worth it.
pub fn listening() -> bool {
    LISTENING.load(Ordering::Acquire)
}

/// How many trees were built for AccessKit: none while nothing listens.
pub fn built() -> u64 {
    BUILT.load(Ordering::Relaxed)
}

/// What a screen reader asked of node `node` of window `window`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    pub window: String,
    pub node: String,
    pub act: Act,
}

/// What comes back from AccessKit's thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// An assistive technology started (`true`) or stopped listening: the
    /// shell describes its windows again, whole, or stops.
    Listening(bool),
    Asked(Asked),
}

type Tell = Arc<dyn Fn(Event) + Send + Sync>;

/// The ids of a window's nodes, for its actions.
type Keys = Arc<Mutex<HashMap<NodeId, String>>>;

struct Held {
    adapter: accesskit_unix::Adapter,
    keys: Keys,
    /// What was last handed over, so that only a change is.
    last: Option<Window>,
    active: bool,
    /// Its first tree went over: what comes now is an update, whose new
    /// polite nodes are said (those of a first tree are not).
    primed: bool,
}

/// The shell's windows, each with its adapter.
pub struct Bridge {
    tell: Tell,
    windows: BTreeMap<String, Held>,
    /// An adapter given no tree, ever: it hears an assistive technology
    /// start while the shell shows no window yet (or keeps none), and
    /// stays out of AT-SPI's tree.
    _probe: accesskit_unix::Adapter,
}

impl std::fmt::Debug for Bridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bridge")
            .field("windows", &self.windows.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl Bridge {
    /// `tell` is called on AccessKit's thread: it should only send the
    /// event on to the shell's loop.
    pub fn new(tell: impl Fn(Event) + Send + Sync + 'static) -> Self {
        let tell: Tell = Arc::new(tell);
        let probe = accesskit_unix::Adapter::new(
            Activation { tell: tell.clone() },
            Actions {
                tell: tell.clone(),
                window: String::new(),
                keys: Keys::default(),
            },
            Deactivation { tell: tell.clone() },
        );
        Self {
            tell,
            windows: BTreeMap::new(),
            _probe: probe,
        }
    }

    /// The windows showing now, by their keys: an adapter for each new
    /// one, and those gone dropped. Cheap, and needs no tree.
    pub fn keep<'a>(&mut self, keys: impl IntoIterator<Item = &'a str>) {
        let keys: Vec<&str> = keys.into_iter().collect();
        self.windows.retain(|key, _| keys.contains(&key.as_str()));
        for key in keys {
            if !self.windows.contains_key(key) {
                let held = self.adapter(key);
                self.windows.insert(key.to_owned(), held);
            }
        }
    }

    /// Hands `windows` over, those that changed since last time; the
    /// others go (see [`Self::keep`]). Call it only while [`listening`].
    pub fn show(&mut self, windows: Vec<Window>) {
        self.keep(windows.iter().map(|w| w.key.as_str()));
        for window in windows {
            let Some(held) = self.windows.get_mut(&window.key) else {
                continue;
            };
            if held.last.as_ref() != Some(&window) {
                {
                    let mut keys = held.keys.lock().unwrap_or_else(PoisonError::into_inner);
                    keys.clear();
                    keys.extend(window.keys());
                }
                // A window's first tree says nothing on its own: it goes
                // without its polite nodes, which come in an update at once.
                let first = (!held.primed).then(|| window.quiet());
                let mut applied = false;
                held.adapter.update_if_active(|| {
                    applied = true;
                    BUILT.fetch_add(1, Ordering::Relaxed);
                    let update: TreeUpdate = first.as_ref().unwrap_or(&window).update();
                    update
                });
                if applied && let Some(first) = first {
                    held.primed = true;
                    if first != window {
                        held.adapter.update_if_active(|| {
                            BUILT.fetch_add(1, Ordering::Relaxed);
                            window.update()
                        });
                    }
                }
                tracing::debug!(window = %window.key, tree = %window.dump(), "the accessibility tree");
            }
            if held.active != window.active {
                held.adapter.update_window_focus_state(window.active);
                held.active = window.active;
            }
            held.last = Some(window);
        }
    }

    /// What was handed over is forgotten: the next [`Self::show`] hands
    /// every window over whole (an assistive technology started).
    pub fn forget(&mut self) {
        for held in self.windows.values_mut() {
            held.last = None;
            held.primed = false;
        }
    }

    fn adapter(&self, window: &str) -> Held {
        let keys: Keys = Arc::default();
        let adapter = accesskit_unix::Adapter::new(
            Activation {
                tell: self.tell.clone(),
            },
            Actions {
                tell: self.tell.clone(),
                window: window.to_owned(),
                keys: keys.clone(),
            },
            Deactivation {
                tell: self.tell.clone(),
            },
        );
        Held {
            adapter,
            keys,
            last: None,
            active: false,
            primed: false,
        }
    }
}

struct Activation {
    tell: Tell,
}

impl ActivationHandler for Activation {
    /// The tree comes from the shell's loop, as soon as it hears.
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        if !LISTENING.swap(true, Ordering::AcqRel) {
            tracing::info!("an assistive technology listens: the shell describes what it shows");
        }
        (self.tell)(Event::Listening(true));
        None
    }
}

struct Deactivation {
    tell: Tell,
}

impl DeactivationHandler for Deactivation {
    fn deactivate_accessibility(&mut self) {
        if LISTENING.swap(false, Ordering::AcqRel) {
            tracing::info!("no assistive technology listens any more");
        }
        (self.tell)(Event::Listening(false));
    }
}

struct Actions {
    tell: Tell,
    window: String,
    keys: Keys,
}

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        let Some(act) = Act::of(request.action) else {
            return;
        };
        let node = self
            .keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&request.target_node)
            .cloned();
        if let Some(node) = node {
            (self.tell)(Event::Asked(Asked {
                window: self.window.clone(),
                node,
                act,
            }));
        }
    }
}
