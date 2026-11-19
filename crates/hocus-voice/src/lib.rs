//! The Hocuspanda microphone (#619, lot 5, docs/design/voice.md): a
//! PipeWire audio source, "Hocuspanda", that every application lists as a
//! microphone. It reads the real microphone only while an application
//! reads it, and makes the voice through the effect chosen
//! ([`effect`]): as it is, without noise ([`denoise`], DeepFilterNet),
//! studio ([`studio`]), or transformed ([`shift`]: Panda, Robot, Grave).
//!
//! The program, `hocus-voice`, is started by the compositor ([`serve`]);
//! the camera and microphone's window reads the source too, for its level
//! meter ([`listen`]).

pub mod denoise;
pub mod effect;
pub mod listen;
pub mod serve;
pub mod shift;
pub mod studio;
pub mod wav;

/// Samples a second, everywhere in the program.
pub const RATE: usize = hocus_link::voice::VOICE_RATE as usize;

#[cfg(test)]
mod tests;
