//! What comes after a capture (5.9, docs/design/screenshots.md): the shell
//! shows its thumbnail, then its editor; the compositor keeps what is its
//! own of each capture taken and does what the shell asks of it.
//!
//! - **Kept**: each capture's file, and the bytes the clipboard was given
//!   for it (held weakly: the clipboard owns them), so that its deletion
//!   empties the clipboard only if the clipboard still holds this image.
//! - **Asked** (`capture forget|annotated|drag ID`): the file deleted on a
//!   thread and the clipboard emptied; the annotated file read back on a
//!   thread and offered on the clipboard; the file dragged into an
//!   application (a drag of the compositor's own, `text/uri-list` and
//!   `image/png`, written to the application on a thread).
//! - **Drawn**: the thumbnail slides up and fades in from its first frame,
//!   its shadow under it; the editor fades in over the desktop blurred.
//!   Neither is ever in a capture.

use std::{
    collections::HashMap,
    io::Write,
    os::fd::OwnedFd,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::Duration,
};

use hocus_link::{
    CAPTURE_EDITOR_IN, CAPTURE_EDITOR_NAMESPACE, CAPTURE_THUMB_IN, CAPTURE_THUMB_NAMESPACE,
    CAPTURE_THUMB_RADIUS, CAPTURE_THUMB_RISE, ClipKind, LAUNCHER_ZOOM,
};
use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{AsRenderElements, surface::WaylandSurfaceRenderElement},
    },
    desktop::{LayerSurface, layer_map_for_output},
    output::Output,
    reexports::wayland_server::protocol::wl_data_device_manager::DndAction,
    utils::{Logical, Physical, Point, Rectangle, Scale},
    wayland::selection::{
        SelectionTarget,
        data_device::{self, SourceMetadata},
    },
};

use super::Back;
use crate::{
    State,
    decoration::{FrameIds, Outside, Rounded, ShadowStyle, shadow_elements},
    going::Shaders,
    motion::{Bezier, Tween},
    programs,
    render::{OutputElement, OutputElements},
    wobble::Bend,
};

/// The captures kept, the newest: older ones are forgotten (their files
/// stay; only what the shell can still ask about goes).
const KEPT: usize = 64;
/// The thumbnail's shadow, the mockup's `0 16px 44px rgba(0, 0, 0, 0.5)`,
/// zoomed.
const THUMB_SHADOW: ShadowStyle = ShadowStyle {
    dy: 16.0 * LAUNCHER_ZOOM,
    blur: 44.0 * LAUNCHER_ZOOM,
    alpha: 0.5,
    radius: CAPTURE_THUMB_RADIUS,
};
/// Behind the editor: the mockup's `blur(10px)`, zoomed, twice its
/// deviation, without saturation.
const EDITOR_FROST: crate::glass::Frost = crate::glass::Frost {
    reach: 2.0 * 10.0 * LAUNCHER_ZOOM,
    saturation: 1.0,
};
/// The types a capture is dragged as.
const URIS: &str = "text/uri-list";

/// What is dragged: a capture (its address and its image), or a
/// recording's file (its address alone).
#[derive(Debug, Clone)]
enum Dragged {
    Capture(u64),
    File(PathBuf),
}

/// A capture taken, as the compositor keeps it.
#[derive(Debug, Clone)]
struct Taken {
    file: Option<PathBuf>,
    /// What the clipboard was given for it.
    offered: Weak<[u8]>,
}

/// What comes after the captures: what is kept of them, and their
/// surfaces since their first frame.
#[derive(Default)]
pub struct After {
    taken: HashMap<u64, Taken>,
    /// The capture being dragged into an application, or a recording's
    /// file (5.10).
    dragging: Option<Dragged>,
    thumb: Option<(LayerSurface, Duration)>,
    editor: Option<(LayerSurface, Duration)>,
    shadow: FrameIds,
    /// Commits of the editor's surface, for tests (#594).
    pub editor_commits: u64,
}

impl std::fmt::Debug for After {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("After")
            .field("taken", &self.taken.len())
            .field("dragging", &self.dragging)
            .finish_non_exhaustive()
    }
}

impl After {
    /// Capture `id` taken: its file, and the bytes the clipboard holds.
    pub fn keep(&mut self, id: u64, file: Option<PathBuf>, offered: &Arc<[u8]>) {
        self.taken.insert(
            id,
            Taken {
                file,
                offered: Arc::downgrade(offered),
            },
        );
        if self.taken.len() > KEPT
            && let Some(oldest) = self.taken.keys().min().copied()
        {
            self.taken.remove(&oldest);
        }
    }
}

/// `path` as a `file://` address, its bytes outside RFC 3986's unreserved
/// ones and `/` escaped.
pub fn file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            uri.push(char::from(b));
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

impl State {
    /// `capture forget ID`: its file goes (on a thread), and the clipboard
    /// empties if it still holds this capture.
    pub(super) fn capture_forget(&mut self, id: u64) {
        let Some(taken) = self.capture.after.taken.remove(&id) else {
            tracing::debug!(id, "no such capture to forget");
            return;
        };
        if self.clipboard_holds(&taken.offered) {
            tracing::info!(id, "the clipboard lets the capture go");
            data_device::clear_data_device_selection(&self.display_handle, &self.seat);
            if let Some(xwm) = self.xwm.as_mut()
                && let Err(err) = xwm.new_selection(SelectionTarget::Clipboard, None)
            {
                tracing::warn!(?err, "could not empty the X11 clipboard");
            }
        }
        if let Some(file) = taken.file {
            tracing::info!(id, file = %file.display(), "the capture is deleted");
            programs::on_a_thread("delete the capture".to_owned(), move || {
                match std::fs::remove_file(&file) {
                    Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                        tracing::warn!(file = %file.display(), %err, "could not delete the capture");
                    }
                    _ => {}
                }
            });
        }
    }

    /// Whether the clipboard holds what was `offered` (the compositor's own
    /// selection, those very bytes).
    fn clipboard_holds(&self, offered: &Weak<[u8]>) -> bool {
        let Some(offered) = offered.upgrade() else {
            return false;
        };
        data_device::current_data_device_selection_userdata(&self.seat)
            .is_some_and(|held| held.as_ref().is_some_and(|b| Arc::ptr_eq(b, &offered)))
    }

    /// `capture annotated ID`: the shell wrote the annotated image over the
    /// capture's file; a thread reads it back for the clipboard.
    pub(super) fn capture_annotated(&mut self, id: u64) {
        let Some(file) = self
            .capture
            .after
            .taken
            .get(&id)
            .and_then(|t| t.file.clone())
        else {
            tracing::debug!(id, "no such capture annotated");
            return;
        };
        let Some(back) = self.capture_done() else {
            return;
        };
        programs::on_a_thread("read the annotated capture".to_owned(), move || {
            let png = std::fs::read(&file).map_err(|err| format!("{}: {err}", file.display()));
            // Its line for the clipboard's history, which replaces the item
            // the capture made (5.8b).
            let history = png
                .as_ref()
                .ok()
                .and_then(|png| crate::clipboard::capture_line(id, png));
            let _ = back.send(Back::Annotated { id, png, history });
        });
    }

    /// The annotated capture `id` read back: the clipboard takes it.
    pub(super) fn capture_reread(
        &mut self,
        id: u64,
        png: Result<Vec<u8>, String>,
        history: Option<String>,
    ) {
        let png = match png {
            Ok(png) => png,
            Err(err) => {
                tracing::warn!(id, err, "the annotated capture could not be read");
                return;
            }
        };
        let Some(taken) = self.capture.after.taken.get(&id).cloned() else {
            return;
        };
        tracing::info!(
            id,
            bytes = png.len(),
            "the clipboard takes the annotated capture"
        );
        let offered = self.offer_clipboard(ClipKind::Png.types(), png);
        self.capture.after.keep(id, taken.file, &offered);
        if let Some(line) = history {
            self.capture_in_history(line);
        }
    }

    /// `capture drag ID`: the left button held on the thumbnail, the
    /// capture's file is dragged from there as the compositor's own.
    pub(super) fn capture_drag(&mut self, id: u64) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let has_file = self
            .capture
            .after
            .taken
            .get(&id)
            .is_some_and(|t| t.file.is_some());
        let (Some(start), Some(serial)) = (
            pointer.grab_start_data(),
            pointer.with_grab(|serial, _| serial),
        ) else {
            tracing::debug!(id, "no button held: no drag");
            return;
        };
        if !has_file || start.button != crate::grabs::BTN_LEFT {
            return;
        }
        tracing::info!(id, "the capture is dragged");
        self.capture.after.dragging = Some(Dragged::Capture(id));
        let metadata = SourceMetadata {
            mime_types: vec![URIS.to_owned(), crate::clipboard::PNG.to_owned()],
            dnd_action: DndAction::Copy,
        };
        let dh = self.display_handle.clone();
        let seat = self.seat.clone();
        data_device::start_dnd(&dh, &seat, self, serial, Some(start), None, metadata);
    }

    /// A recording's thumbnail dragged (5.10), the left button held: its
    /// file is dragged from there, as its address.
    pub(crate) fn drag_file(&mut self, file: PathBuf) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let (Some(start), Some(serial)) = (
            pointer.grab_start_data(),
            pointer.with_grab(|serial, _| serial),
        ) else {
            tracing::debug!("no button held: no drag");
            return;
        };
        if start.button != crate::grabs::BTN_LEFT {
            return;
        }
        tracing::info!(file = %file.display(), "a file is dragged");
        self.capture.after.dragging = Some(Dragged::File(file));
        let metadata = SourceMetadata {
            mime_types: vec![URIS.to_owned()],
            dnd_action: DndAction::Copy,
        };
        let dh = self.display_handle.clone();
        let seat = self.seat.clone();
        data_device::start_dnd(&dh, &seat, self, serial, Some(start), None, metadata);
    }

    /// The application the capture was dropped on reads it as `mime`: its
    /// address, or its image, written on a thread.
    pub fn capture_drag_send(&mut self, mime: &str, fd: OwnedFd) {
        let file = match &self.capture.after.dragging {
            Some(Dragged::Capture(id)) => self
                .capture
                .after
                .taken
                .get(id)
                .and_then(|t| t.file.clone()),
            Some(Dragged::File(file)) if mime == URIS => Some(file.clone()),
            _ => None,
        };
        let Some(file) = file else {
            return;
        };
        let mime = mime.to_owned();
        programs::on_a_thread("drop the capture".to_owned(), move || {
            let mut out = std::fs::File::from(fd);
            let written = if mime == URIS {
                out.write_all(format!("{}\r\n", file_uri(&file)).as_bytes())
            } else {
                std::fs::File::open(&file)
                    .and_then(|mut png| std::io::copy(&mut png, &mut out).map(drop))
            };
            if let Err(err) = written {
                tracing::debug!(mime, %err, "could not drop the capture");
            }
        });
    }

    /// The drag ended, dropped or not.
    pub fn capture_drag_ended(&mut self) {
        self.capture.after.dragging = None;
    }

    /// A commit of the thumbnail's or the editor's `layer`: its first frame
    /// starts its motion.
    pub fn capture_after_committed(&mut self, layer: &LayerSurface) {
        let now = self.now();
        if layer.namespace() == CAPTURE_EDITOR_NAMESPACE {
            self.capture.after.editor_commits += 1;
        }
        let slot = match layer.namespace() {
            CAPTURE_THUMB_NAMESPACE => &mut self.capture.after.thumb,
            CAPTURE_EDITOR_NAMESPACE => &mut self.capture.after.editor,
            _ => return,
        };
        if slot.as_ref().is_some_and(|(l, _)| l == layer) {
            return;
        }
        let drawn = smithay::backend::renderer::utils::with_renderer_surface_state(
            layer.wl_surface(),
            |s| s.buffer().is_some(),
        )
        .unwrap_or(false);
        if drawn {
            *slot = Some((layer.clone(), now));
            self.redraw = true;
        }
    }

    /// A layer surface went: if it was the thumbnail or the editor, its
    /// motion goes too.
    pub fn capture_after_gone(&mut self, layer: &LayerSurface) {
        let after = &mut self.capture.after;
        for slot in [&mut after.thumb, &mut after.editor] {
            if slot.as_ref().is_some_and(|(l, _)| l == layer) {
                *slot = None;
            }
        }
    }

    /// Whether the thumbnail or the editor moves at `now`.
    pub(super) fn capture_after_moving(&self, now: Duration) -> bool {
        let after = &self.capture.after;
        after
            .thumb
            .as_ref()
            .is_some_and(|(_, since)| now < *since + CAPTURE_THUMB_IN)
            || after
                .editor
                .as_ref()
                .is_some_and(|(_, since)| now < *since + CAPTURE_EDITOR_IN)
    }

    /// The editor and the thumbnail on `output` at `now`, front first;
    /// where the editor's glass goes, and how opaque it is, when it shows.
    pub(crate) fn capture_after_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        now: Duration,
        out: &mut Vec<OutputElement<R>>,
    ) -> Option<(usize, f32)>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let scale = output.current_scale().fractional_scale();
        let still = self.reduced_motion();
        let shown = |slot: &Option<(LayerSurface, Duration)>| {
            let (layer, since) = slot.as_ref()?;
            let map = layer_map_for_output(output);
            map.layers()
                .any(|l| l == layer)
                .then(|| map.layer_geometry(layer))
                .flatten()
                .map(|g| (layer.clone(), *since, g))
        };
        let mut glass = None;
        if let Some((layer, since, geometry)) = shown(&self.capture.after.editor) {
            let alpha = if still {
                1.0
            } else {
                Tween::new(since, CAPTURE_EDITOR_IN, Bezier::EASE_OUT)
                    .at(now)
                    .clamp(0.0, 1.0) as f32
            };
            if alpha > 0.0 {
                let loc = geometry.loc.to_physical_precise_round(scale);
                let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
                    AsRenderElements::<R>::render_elements(
                        &layer,
                        renderer,
                        loc,
                        Scale::from(scale),
                        alpha,
                    );
                out.extend(surfaces.into_iter().map(OutputElements::Surface));
                glass = Some((out.len(), alpha));
            }
        }
        if let Some((layer, since, geometry)) = shown(&self.capture.after.thumb) {
            let p = if still {
                1.0
            } else {
                Tween::new(since, CAPTURE_THUMB_IN, Bezier(0.2, 0.8, 0.2, 1.0))
                    .at(now)
                    .clamp(0.0, 1.0)
            };
            let alpha = p as f32;
            if alpha > 0.0 {
                let mut at = geometry.to_f64();
                at.loc.y += CAPTURE_THUMB_RISE * (1.0 - p);
                let loc: Point<i32, Physical> = at.loc.to_physical_precise_round(scale);
                let surfaces: Vec<WaylandSurfaceRenderElement<R>> =
                    AsRenderElements::<R>::render_elements(
                        &layer,
                        renderer,
                        loc,
                        Scale::from(scale),
                        alpha,
                    );
                if self.shell.software {
                    let clip = Rectangle::new(loc, geometry.size.to_physical_precise_round(scale));
                    let radius = (CAPTURE_THUMB_RADIUS * scale).round() as i32;
                    out.extend(surfaces.into_iter().map(|e| {
                        OutputElements::Bar(Rounded::new(e, clip, radius, Outside::Cropped, scale))
                    }));
                } else {
                    out.extend(surfaces.into_iter().map(OutputElements::Surface));
                }
                out.extend(
                    shadow_elements(
                        renderer,
                        &self.textures,
                        &self.capture.after.shadow,
                        THUMB_SHADOW,
                        at,
                        scale,
                        alpha,
                    )
                    .into_iter()
                    .map(OutputElements::Shadow),
                );
            }
        }
        glass
    }

    /// The glass under the editor on `output`, `alpha` opaque: what is
    /// `behind` it, the whole screen, blurred. None without glass.
    pub(crate) fn capture_editor_glass<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        alpha: f32,
        behind: &[OutputElement<R>],
    ) -> Option<OutputElement<R>>
    where
        R: Renderer + ImportAll + ImportMem + Bend + Shaders + crate::glass::Glass,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        if !self.glass_on() {
            return None;
        }
        let size = self.space.output_geometry(output)?.size;
        let scale = output.current_scale().fractional_scale();
        let region: Rectangle<i32, Physical> =
            Rectangle::<f64, Logical>::from_size(size.to_f64()).to_physical_precise_round(scale);
        let blurred = renderer.frost(
            &mut self.panes.borrow_mut(),
            &crate::glass::editor_pane(&output.name()),
            region,
            scale,
            EDITOR_FROST,
            behind,
        )?;
        crate::render::glass_piece(renderer, blurred, region, scale, alpha, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_dragged_as_its_address() {
        assert_eq!(
            file_uri(Path::new(
                "/home/alex/Images/Captures d'écran/Capture du 2026-11-03 à 14-32-05.png"
            )),
            "file:///home/alex/Images/Captures%20d%27%C3%A9cran/\
             Capture%20du%202026-11-03%20%C3%A0%2014-32-05.png"
        );
    }

    #[test]
    fn only_the_newest_captures_are_kept() {
        let mut after = After::default();
        let bytes: Arc<[u8]> = Arc::from(vec![1, 2, 3]);
        for id in 1..=(KEPT as u64 + 3) {
            after.keep(id, None, &bytes);
        }
        assert_eq!(after.taken.len(), KEPT);
        assert!(!after.taken.contains_key(&3));
        assert!(after.taken.contains_key(&4));
    }
}
