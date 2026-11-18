//! The Hocuspanda camera (#619, docs/design/camera.md): a PipeWire video
//! source, "Hocuspanda", that applications in a call choose as their
//! camera. It reads the real camera only while an application reads it,
//! and draws the user's effects into its frames: Lumière ([`look`]), on
//! the GPU ([`gpu`]) or the CPU, and the banner ([`banner`]).
//!
//! The program, `hocus-camera`, is started by the compositor ([`serve`]);
//! the shell's camera window reads the source too, for its preview
//! ([`preview`]).

pub mod banner;
pub mod convert;
pub mod gpu;
pub mod look;
pub mod pods;
pub mod preview;
pub mod serve;
