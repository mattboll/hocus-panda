//! `hocus-transcribe` (docs/design/meetings.md): a meeting's text from the
//! sound the compositor recorded, on the laptop, at a low priority, never
//! on the compositor's loop. Its two tracks are heard apart: the user's
//! microphone is "Moi", the call's voices "Voix 1", "Voix 2"… when they
//! can be told apart, else "Les autres".
//!
//! - [`device`]: where it hears: a GPU through Vulkan, else the CPU;
//! - [`engine`]: what hears, behind a trait; [`real`] (whisper.cpp and
//!   sherpa-onnx), or a fake one with fixed lines for tests;
//! - [`speech`]: where a track speaks;
//! - [`text`]: the turns and the Markdown file;
//! - [`fetch`]: the models, downloaded on first use and checked;
//! - [`run`]: a meeting's folder, from its sound to its file.

pub mod device;
pub mod engine;
pub mod fetch;
pub mod real;
pub mod run;
pub mod speech;
pub mod text;
