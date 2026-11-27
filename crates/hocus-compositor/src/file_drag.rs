//! A file dragged over the bar's connections, for a phone's row (bar.md
//! "The phone", #957).
//!
//! An application's drag carrying files (`text/uri-list`) goes to the
//! surface under the pointer as any drag, the shell's too; but the shell
//! cannot tell which of its layer surfaces a drag's `enter` names (its
//! layer-shell crate keeps them to itself). The compositor knows: while
//! such a drag is over a bar's connections' surface, it tells the shell
//! where (`file-drag`), as it tells a window moved over the tiles
//! (`tile_drop.rs`), and brings that bar in front of the windows until the
//! drag leaves it. The shell takes the drag's offer there, through its own
//! data device, when the row under it is a phone's that takes files; let
//! go with the offer taken, the compositor says so (`file-dropped`) and the
//! shell reads the files from the offer and finishes it. Nothing runs but
//! on the pointer's moves during such a drag.

use hocus_link::{CONNECTIONS_NAMESPACE, ToShell};
use smithay::{
    desktop::layer_map_for_output,
    reexports::wayland_server::protocol::{wl_data_source::WlDataSource, wl_surface::WlSurface},
    utils::{Logical, Point, Rectangle},
    wayland::selection::data_device::with_source_metadata,
};

use crate::State;

/// The type a drag of files offers.
pub const URIS: &str = "text/uri-list";

/// A drag carrying files, and where it is over a bar's connections.
#[derive(Debug, Default)]
pub struct FileDrag {
    /// The bar whose connections it is over, and where in their surface,
    /// as last told.
    over: Option<(String, (i32, i32))>,
}

/// Whether `source` offers files.
pub fn offers_files(source: &WlDataSource) -> bool {
    with_source_metadata(source, |m| m.mime_types.iter().any(|t| t == URIS)).unwrap_or(false)
}

impl State {
    /// A client's drag began from `source`: one of files is followed.
    pub fn file_drag_started(&mut self, source: Option<&WlDataSource>) {
        self.file_drag = source.is_some_and(offers_files).then(FileDrag::default);
        if self.file_drag.is_some() {
            tracing::debug!("a drag of files");
        }
    }

    /// The pointer moved to `at` (global) during the drag: over a bar's
    /// connections, the shell is told where, and that bar comes in front;
    /// out of them, that it left.
    pub fn file_drag_motion(&mut self, at: Point<f64, Logical>) {
        let Some(drag) = &self.file_drag else {
            return;
        };
        let now = self.connections_under(at).map(|(bar, rect)| {
            let local = at - rect.loc;
            (bar, (local.x.floor() as i32, local.y.floor() as i32))
        });
        if now == drag.over {
            return;
        }
        let before = self.file_drag.as_mut().and_then(|d| d.over.take());
        if let Some((was, _)) = &before
            && now.as_ref().is_none_or(|(bar, _)| bar != was)
        {
            self.tell_shell(ToShell::FileDrag {
                bar: was.clone(),
                at: None,
            });
            self.bar_front_dropped();
        }
        if let Some((bar, point)) = &now {
            self.tell_shell(ToShell::FileDrag {
                bar: bar.clone(),
                at: Some(*point),
            });
            self.bar_front_dropping(bar);
        }
        if let Some(drag) = self.file_drag.as_mut() {
            drag.over = now;
        }
    }

    /// The drag ended on `target`, `validated` when its target took the
    /// offer: over a bar's connections, the shell is told it was let go
    /// there, or that it left.
    pub fn file_drag_ended(&mut self, target: Option<&WlSurface>, validated: bool) {
        let Some(drag) = self.file_drag.take() else {
            return;
        };
        let Some((bar, _)) = drag.over else {
            return;
        };
        let on_it = validated && target.is_some_and(|t| self.is_connections_of(&bar, t));
        tracing::debug!(bar, dropped = on_it, "a drag of files ends over the bar");
        if on_it {
            self.tell_shell(ToShell::FileDropped { bar });
        } else {
            self.tell_shell(ToShell::FileDrag { bar, at: None });
        }
        self.bar_front_dropped();
    }

    /// The bar whose connections' surface is under `at` (global), and
    /// where that surface is.
    fn connections_under(
        &self,
        at: Point<f64, Logical>,
    ) -> Option<(String, Rectangle<f64, Logical>)> {
        self.space.outputs().find_map(|output| {
            let name = output.name();
            if !self.panel.bars.contains_key(&name) {
                return None;
            }
            let screen = self.space.output_geometry(output)?;
            let map = layer_map_for_output(output);
            let layer = map
                .layers()
                .find(|l| l.namespace() == CONNECTIONS_NAMESPACE)?;
            let geometry = map.layer_geometry(layer)?;
            let rect = Rectangle::new(screen.loc + geometry.loc, geometry.size).to_f64();
            rect.contains(at).then_some((name, rect))
        })
    }

    /// Whether `surface` is bar `bar`'s connections' surface.
    fn is_connections_of(&self, bar: &str, surface: &WlSurface) -> bool {
        self.space
            .outputs()
            .find(|o| o.name() == bar)
            .is_some_and(|output| {
                layer_map_for_output(output)
                    .layers()
                    .any(|l| l.namespace() == CONNECTIONS_NAMESPACE && l.wl_surface() == surface)
            })
    }
}
