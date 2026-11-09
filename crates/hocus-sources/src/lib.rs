//! The secretary's sources, read-only, and what code makes of them before
//! any model sees them (docs/design/secretary.md §10, §14, lot 3).
//!
//! - [`goa`]: every Google account of GNOME Online Accounts, on the bus
//!   the process is given, never another; each suggested a notebook.
//! - [`consent`]: nothing is read before its consent ("Autoriser"), one
//!   record per account and kind; a revocation purges.
//! - [`calendar`], [`contacts`], [`imap`]: the readers. Calendars come
//!   from `hocus-day`'s Google reader, hidden ones included; birthdays
//!   from CardDAV; Gmail over IMAP with `EXAMINE` and `BODY.PEEK` only,
//!   an allow-list checked before every byte leaves.
//! - [`redact`]: noise removed, then the secrets' first pass (code
//!   detectors), each secret replaced by `[secret masqué — voir …]`
//!   with a pointer to the original. The second pass (the local model)
//!   is lot 4's: [`redact::Check`] holds an item back until it is done.
//! - [`facts`]: public and school holidays, free slots and dates, computed
//!   in code and handed to the model as facts.
//! - [`cycle`]: one read of every consented source, as the secretary's
//!   source workers run it: items in, redacted items out.
//!
//! Everything here blocks (a bus, a socket, HTTPS): it runs on a source's
//! worker thread of the `hocus-secretary` process, never a loop.

pub mod calendar;
pub mod consent;
pub mod contacts;
pub mod cycle;
pub mod facts;
pub mod goa;
mod http;
pub mod imap;
pub mod item;
pub mod mail;
pub mod redact;

pub use crate::{
    consent::{Consent, ConsentBook, Grant},
    item::{Content, Item, Notebook, Pointer, SourceKey, SourceKind},
    redact::{Check, Outbound, Quoted, Redacted, Redactor, SecondPass},
};
pub use hocus_day::source::{Result, SourceError};
