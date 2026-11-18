//! The Hocuspanda camera (#619, docs/design/camera.md): a PipeWire video
//! source, "Hocuspanda", that applications in a call choose as their
//! camera. It reads the real camera only while an application reads it,
//! and draws the user's effects into its frames: Lumière ([`look`]), on
//! the GPU ([`gpu`]) or the CPU, and the banner ([`banner`]).
//!
//! The program, `hocus-camera`, is started by the compositor ([`serve`]);
//! the shell's camera window reads the source too, for its preview
//! ([`preview`]). Its frames go to a v4l2loopback device labelled
//! "Hocuspanda" too, when there is one ([`loopback`]), for the
//! applications that read only `/dev/video*`.

pub mod away;
pub mod backdrop;
pub mod banner;
pub mod convert;
pub mod face;
pub mod framing;
pub mod glsl;
pub mod gpu;
pub mod hands;
pub mod look;
pub mod loopback;
pub mod paint;
pub mod pods;
pub mod presenter;
pub mod preview;
pub mod react;
pub mod scenes;
pub mod segment;
pub mod serve;
pub mod sight;
pub mod track;
pub mod wear;
