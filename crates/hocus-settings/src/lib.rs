//! The settings app ([decision 0010](../../docs/decisions/0010-settings-as-a-client.md)):
//! a Wayland client of its own, drawn with `hocus-ui`. It shows what is in
//! effect through the control API and changes the configuration file,
//! which the compositor applies live.

pub mod arrange;
pub mod catalogue;
pub mod client;
/// The configuration file's writer, shared with the shell's AI actions (#197).
pub use hocus_core::config_edit as edit;
pub mod view;
pub mod words;
