//! The settings app ([decision 0010](../../docs/decisions/0010-settings-as-a-client.md)):
//! a Wayland client of its own, drawn with `hocus-ui`. It shows what is in
//! effect through the control API and changes the configuration file,
//! which the compositor applies live.

pub mod catalogue;
pub mod client;
pub mod edit;
pub mod view;
pub mod words;
