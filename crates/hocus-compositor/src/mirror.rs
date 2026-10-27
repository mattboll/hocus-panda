//! Mirroring (#125): a screen shows another one's picture, scaled to fit
//! with bars, instead of desktops of its own. It stays lit but leaves the
//! layout as an unplugged screen would: the pointer does not go there, and
//! the windows it showed wait on the primary screen until it shows its own
//! desktops again, where they were. Its cursor is the source's, drawn in
//! the source's picture.

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::utils::{Relocate, RelocateRenderElement, RescaleRenderElement},
    },
    output::Output,
    utils::{Physical, Point, Scale, Size},
};

use crate::{
    State,
    outputs::Head,
    scene::{OutputElement, SceneElement},
};

/// A screen showing another's picture.
#[derive(Debug, Clone)]
pub struct Mirror {
    pub output: Output,
    pub source: String,
    /// Its own desktops, for when it shows them again.
    desktops: usize,
}

impl State {
    /// The screen `output` shows instead of its own desktops, if any.
    pub fn mirror_source(&self, output: &Output) -> Option<Output> {
        let mirror = self.mirrors.iter().find(|m| m.output == *output)?;
        self.space
            .outputs()
            .find(|o| o.name() == mirror.source)
            .cloned()
    }

    pub fn is_mirroring(&self, name: &str) -> bool {
        self.mirrors.iter().any(|m| m.output.name() == name)
    }

    /// A lit screen by name: in the layout, or showing another's picture.
    pub fn lit_output(&self, name: &str) -> Option<Output> {
        self.find_output(name).or_else(|| {
            self.mirrors
                .iter()
                .find(|m| m.output.name() == name)
                .map(|m| m.output.clone())
        })
    }

    /// Screens start and stop mirroring as the configuration says: a lit
    /// screen shows its `mirror`'s picture when that one is lit and shows
    /// its own desktops.
    pub fn settle_mirrors(&mut self, heads: &[Head]) {
        let lit = |name: &str| heads.iter().any(|h| h.name == name && h.now.enabled);
        let wanted: Vec<(String, String)> = heads
            .iter()
            .filter(|h| h.now.enabled)
            .filter_map(|h| {
                let source = self.config.output(&h.name)?.mirror.clone()?;
                let shows_own = self
                    .config
                    .output(&source)
                    .is_none_or(|s| s.mirror.is_none());
                (lit(&source) && shows_own).then(|| (h.name.clone(), source))
            })
            .collect();
        // Those that stop: back in the layout, with their windows.
        let stopping: Vec<Mirror> = self
            .mirrors
            .iter()
            .filter(|m| !wanted.iter().any(|(name, _)| *name == m.output.name()))
            .cloned()
            .collect();
        for mirror in stopping {
            self.mirrors.retain(|m| m.output != mirror.output);
            let name = mirror.output.name();
            let Some(head) = heads.iter().find(|h| h.name == name && h.now.enabled) else {
                continue;
            };
            self.space.map_output(&mirror.output, head.now.position);
            self.plug_output(&mirror.output, mirror.desktops);
            self.emit(|s| hocus_api::Event::OutputAdded {
                output: s.output_info(&mirror.output),
            });
            tracing::info!(output = name, "showing its own desktops again");
        }
        for (name, source) in wanted {
            if let Some(mirror) = self.mirrors.iter_mut().find(|m| m.output.name() == name) {
                mirror.source = source;
                continue;
            }
            let Some(output) = self.find_output(&name) else {
                continue;
            };
            let desktops = self
                .model
                .outputs()
                .iter()
                .find(|o| o.name.0 == name)
                .map_or(1, |o| o.desktops.len());
            self.unplug_output(&output);
            tracing::info!(output = name, source, "showing another screen");
            self.mirrors.push(Mirror {
                output,
                source,
                desktops,
            });
        }
        self.redraw.all();
    }

    /// The mirrored screen's picture: the source's scene, scaled to fit
    /// `output` and centred.
    pub fn mirror_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        source: &Output,
        cursor: bool,
    ) -> Vec<OutputElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let (Some(to), Some(from)) = (physical_size(output), physical_size(source)) else {
            return Vec::new();
        };
        let k = (f64::from(to.w) / f64::from(from.w.max(1)))
            .min(f64::from(to.h) / f64::from(from.h.max(1)));
        let offset = Point::<i32, Physical>::from((
            ((f64::from(to.w) - f64::from(from.w) * k) / 2.0).round() as i32,
            ((f64::from(to.h) - f64::from(from.h) * k) / 2.0).round() as i32,
        ));
        // The source's magnifier is its own: the mirror shows the whole
        // picture.
        self.scene_parts(renderer, source, cursor)
            .into_iter()
            .map(|e: SceneElement<R>| {
                let scaled =
                    RescaleRenderElement::from_element(e, Point::default(), Scale::from(k));
                OutputElement::Mirrored(RelocateRenderElement::from_element(
                    scaled,
                    offset,
                    Relocate::Relative,
                ))
            })
            .collect()
    }
}

/// An output's size in pixels as it is turned.
fn physical_size(output: &Output) -> Option<Size<i32, Physical>> {
    let mode = output.current_mode()?;
    Some(output.current_transform().transform_size(mode.size))
}
