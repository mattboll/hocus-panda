//! A capture taken by the backend, with its renderer: each screen it
//! needs drawn again into a buffer of its own at the screen's resolution
//! (or the window alone, at its screen's), as the screen shows it but for
//! the capture's own drawing, the toolbar and the toasts, the pointer only
//! if asked (on a window, only when it is over it), and read back. The headless backend's screenshots are read
//! the same way. What is drawn is handed to a thread ([`super::save`]).

use anyhow::Context;
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, ExportMem, ImportAll, ImportMem, Offscreen, Renderer, Texture,
            damage::OutputDamageTracker,
        },
    },
    output::Output,
    utils::{Buffer as BufferCoords, Physical, Rectangle, Size, Transform},
};

use super::{Shot, Target, geometry::Area, save};
use crate::{
    State,
    going::Shaders,
    render::{BACKGROUND, OutputElement, Purpose},
    wobble::Bend,
};

/// Takes the capture asked for, if one is: drawn now, made into an image
/// off the loop.
pub fn take<R, T>(renderer: &mut R, state: &mut State)
where
    R: Renderer
        + ImportAll
        + ImportMem
        + Offscreen<T>
        + Bind<T>
        + ExportMem
        + Bend
        + Shaders
        + crate::glass::Glass,
    R::TextureId: Send + Clone + Texture + 'static,
    R::Error: Send + Sync + 'static,
{
    let Some(shot) = state.capture.take_pending() else {
        return;
    };
    match pieces(renderer, state, &shot) {
        Ok((pieces, cut, flash)) => {
            let job = save::Job {
                pieces,
                cut,
                when: state.capture_when(),
                pictures: state.config.pictures.clone(),
                french: state.config.french,
            };
            state.capture_taken(shot, job, flash);
        }
        Err(err) => tracing::warn!("the capture could not be drawn: {err:#}"),
    }
}

/// What a capture's pieces are made of, the part of the only one an area
/// keeps, and what flashes (screens' names, and their parts).
type Drawn = (
    Vec<save::Piece>,
    Option<(u32, u32, u32, u32)>,
    Vec<(String, Area)>,
);

fn pieces<R, T>(renderer: &mut R, state: &State, shot: &Shot) -> anyhow::Result<Drawn>
where
    R: Renderer
        + ImportAll
        + ImportMem
        + Offscreen<T>
        + Bind<T>
        + ExportMem
        + Bend
        + Shaders
        + crate::glass::Glass,
    R::TextureId: Send + Clone + Texture + 'static,
    R::Error: Send + Sync + 'static,
{
    let output = |name: &str| -> anyhow::Result<Output> {
        state
            .space
            .outputs()
            .find(|o| o.name() == name)
            .cloned()
            .with_context(|| format!("no screen {name}"))
    };
    let whole = |state: &State, o: &Output| -> Option<(String, Area)> {
        let (w, h) = state.logical_size(&o.name())?;
        Some((
            o.name(),
            Area {
                x: 0.0,
                y: 0.0,
                w,
                h,
            },
        ))
    };
    match &shot.target {
        Target::Area { output: name, area } => {
            let o = output(name)?;
            let piece = screen(renderer, state, &o, shot.pointer)?;
            let s = piece.scale;
            let cut = (
                (area.x * s).round().max(0.0) as u32,
                (area.y * s).round().max(0.0) as u32,
                (area.w * s).round().max(1.0) as u32,
                (area.h * s).round().max(1.0) as u32,
            );
            Ok((vec![piece], Some(cut), vec![(name.clone(), *area)]))
        }
        Target::Screen(name) => {
            let o = output(name)?;
            let piece = screen(renderer, state, &o, shot.pointer)?;
            Ok((vec![piece], None, whole(state, &o).into_iter().collect()))
        }
        Target::All => {
            let outputs: Vec<Output> = state.space.outputs().cloned().collect();
            anyhow::ensure!(!outputs.is_empty(), "no screen");
            let mut pieces = Vec::with_capacity(outputs.len());
            for o in &outputs {
                pieces.push(screen(renderer, state, o, shot.pointer)?);
            }
            let flash = outputs.iter().filter_map(|o| whole(state, o)).collect();
            Ok((pieces, None, flash))
        }
        Target::Window(id) => {
            let o = state
                .window_output(*id)
                .context("the window is on no screen")?;
            let scale = o.current_scale().fractional_scale();
            let (mut elements, rect) = state
                .window_alone(renderer, *id, scale)
                .context("the window went")?;
            // The pointer in front, where it is on the window, when it is
            // over it (5.8b).
            if shot.pointer && state.pointer_over(*id) {
                let cursor = state.cursor_elements_in(renderer, rect, scale);
                elements.splice(0..0, cursor.into_iter().map(OutputElement::Cursor));
            }
            let size: Size<i32, Physical> = rect.size.to_physical_precise_round(scale);
            let pixels = draw::<R, T>(renderer, size, scale, &elements, [0.0; 4])?;
            let piece = save::Piece {
                pixels,
                size: (size.w as u32, size.h as u32),
                at: (rect.loc.x, rect.loc.y),
                scale,
            };
            // The flash on each screen the window is on, its part there.
            let flash = state
                .space
                .outputs()
                .filter_map(|o| {
                    let g = state.space.output_geometry(o)?.to_f64();
                    let inside = rect.intersection(g)?;
                    Some((
                        o.name(),
                        Area {
                            x: inside.loc.x - g.loc.x,
                            y: inside.loc.y - g.loc.y,
                            w: inside.size.w,
                            h: inside.size.h,
                        },
                    ))
                })
                .collect();
            Ok((vec![piece], None, flash))
        }
    }
}

/// Screen `output` drawn for a capture, at its resolution.
pub(crate) fn screen<R, T>(
    renderer: &mut R,
    state: &State,
    output: &Output,
    pointer: bool,
) -> anyhow::Result<save::Piece>
where
    R: Renderer
        + ImportAll
        + ImportMem
        + Offscreen<T>
        + Bind<T>
        + ExportMem
        + Bend
        + Shaders
        + crate::glass::Glass,
    R::TextureId: Send + Clone + Texture + 'static,
    R::Error: Send + Sync + 'static,
{
    let mode = output.current_mode().context("a screen without a mode")?;
    let size = output.current_transform().transform_size(mode.size);
    let scale = output.current_scale().fractional_scale();
    let at = state
        .space
        .output_geometry(output)
        .context("a screen out of the desktop")?
        .loc;
    let elements = state.elements_for(renderer, output, Purpose::Capture { cursor: pointer });
    let pixels = draw::<R, T>(renderer, size, scale, &elements, BACKGROUND)?;
    Ok(save::Piece {
        pixels,
        size: (size.w as u32, size.h as u32),
        at: (f64::from(at.x), f64::from(at.y)),
        scale,
    })
}

/// `elements` drawn into a buffer `size` large, upright, on `clear`; its
/// pixels read back as ARGB8888 (B, G, R, A in memory).
pub(crate) fn draw<R, T>(
    renderer: &mut R,
    size: Size<i32, Physical>,
    scale: f64,
    elements: &[OutputElement<R>],
    clear: [f32; 4],
) -> anyhow::Result<Vec<u8>>
where
    R: Renderer + ImportAll + ImportMem + Offscreen<T> + Bind<T> + ExportMem + Bend + Shaders,
    R::TextureId: Send + Clone + Texture + 'static,
    R::Error: Send + Sync + 'static,
{
    anyhow::ensure!(size.w > 0 && size.h > 0, "nothing to draw");
    let buffer: Size<i32, BufferCoords> = (size.w, size.h).into();
    let mut target: T = renderer
        .create_buffer(Fourcc::Abgr8888, buffer)
        .or_else(|_| renderer.create_buffer(Fourcc::Argb8888, buffer))
        .context("no buffer to draw the capture into")?;
    let mut framebuffer = renderer.bind(&mut target).context("could not bind")?;
    let mut damage = OutputDamageTracker::new(size, scale, Transform::Normal);
    damage
        .render_output(renderer, &mut framebuffer, 0, elements, clear)
        .map_err(|err| anyhow::anyhow!("{err:?}"))
        .context("could not draw the capture")?;
    let region = Rectangle::from_size(buffer);
    let mapping = renderer
        .copy_framebuffer(&framebuffer, region, Fourcc::Argb8888)
        .context("could not read the capture")?;
    let bytes = renderer
        .map_texture(&mapping)
        .context("could not map the capture")?;
    Ok(bytes.to_vec())
}
