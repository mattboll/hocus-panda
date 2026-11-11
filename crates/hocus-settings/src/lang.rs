//! The window's language, and the words it says in it: every string the
//! user reads is a [`Words`], a text of the catalogue.

/// The window's language: the workspace's one.
pub use hocus_words::Lang;

/// A text in every language: the catalogue's (`hocus_words::settings`).
pub use hocus_words::Text as Words;
