//! The secretary's process (docs/design/secretary.md §16): started and
//! supervised by the compositor in `[secretary] mode = "assistant"` only,
//! never on its loop nor the shell's. She keeps her memory (a temporal
//! knowledge graph in an encrypted SQLite, [`store`] and [`graph`]), her
//! audit log ([`audit`]), her sources' bookkeeping ([`sources`], [`brain`]),
//! and answers on her private socket ([`server`], the protocol in
//! `hocus-secretary-link`).
//!
//! Her models (lot 4): the providers behind one request shape
//! ([`model`], [`providers`]: the Anthropic Messages API, OpenAI-compatible
//! local servers, each role as [`settings`] say), the local triage queue
//! and the secrets' second pass ([`triage`]), context assembly within a
//! budget ([`context`]) from hybrid retrieval over her graph
//! ([`retrieve`]), and her read-only tools ([`tools`]). Her Google
//! accounts are one source ([`google`]); the applications' notifications
//! the shell forwards, another ([`notifications`], lot 7).
//!
//! What she learns (lot 6): the sentences that teach her and the rules
//! they compile to, what her sources say of the team, what she notices
//! ([`learn`]); "Ce qu'elle sait" ([`knows`]); the night's compaction
//! ([`compaction`]).
//!
//! The harness the next lots test with is here too: her own clock
//! ([`clock`]), the scripted fake model and the recorder ([`model`]), and
//! fixture sources ([`sources::Fixture`]).

pub mod audit;
pub mod brain;
pub mod clock;
pub mod compaction;
pub mod context;
pub mod fold;
pub mod google;
pub mod graph;
mod http;
pub mod key;
pub mod knows;
pub mod learn;
pub mod model;
pub mod notifications;
pub mod power;
pub mod providers;
pub mod retrieve;
pub mod server;
pub mod settings;
pub mod sources;
pub mod store;
pub mod tools;
pub mod triage;

/// Her memory's file, in her data folder.
pub const MEMORY: &str = "memory.db";

/// Her data folder: `$XDG_DATA_HOME/hocuspanda/secretary`, or under
/// `~/.local/share`; `None` without an absolute one.
pub use hocus_secretary_link::data_dir;
