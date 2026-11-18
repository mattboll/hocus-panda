//! The desktop's living wallpapers (docs/design/effects.md), as the
//! compositor draws them on the screens and the Hocuspanda camera behind
//! the person (docs/design/camera.md, lot 3): their shaders
//! ([`shaders`]) and what moves them ([`aurora`], [`koi`], with the
//! shapes they are drawn of, [`mesh`], and [`particles`]), one source for
//! both. No
//! renderer, no smithay: the shell's side of the workspace (the camera
//! does not depend on smithay) can take them.

pub mod aurora;
pub mod koi;
pub mod mesh;
pub mod particles;
pub mod shaders;
