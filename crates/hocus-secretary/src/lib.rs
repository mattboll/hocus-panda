//! The secretary's process (docs/design/secretary.md §16): started and
//! supervised by the compositor in `[secretary] mode = "assistant"` only,
//! never on its loop nor the shell's. She keeps her memory (a temporal
//! knowledge graph in an encrypted SQLite, [`store`] and [`graph`]), her
//! audit log ([`audit`]), her sources' bookkeeping ([`sources`], [`brain`]),
//! and answers on her private socket ([`server`], the protocol in
//! `hocus-secretary-link`).
//!
//! The harness the next lots test with is here too: her own clock
//! ([`clock`]), the scripted fake model and the recorder ([`model`]), and
//! fixture sources ([`sources::Fixture`]).

pub mod audit;
pub mod brain;
pub mod clock;
pub mod fold;
pub mod graph;
pub mod key;
pub mod model;
pub mod server;
pub mod sources;
pub mod store;

/// Her memory's file, in her data folder.
pub const MEMORY: &str = "memory.db";

/// Her data folder: `$XDG_DATA_HOME/hocuspanda/secretary`, or under
/// `~/.local/share`; `None` without an absolute one.
pub fn data_dir() -> Option<std::path::PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let data = var("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| var("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))?;
    data.is_absolute()
        .then(|| data.join("hocuspanda/secretary"))
}
