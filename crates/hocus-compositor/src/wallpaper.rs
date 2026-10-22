//! Wallpapers and the shell's theme. Pictures are made on a worker thread
//! (decoding, scaling, the glow) and come back through a channel; a screen
//! keeps its picture until the new one is ready. The theme follows the
//! configuration, its accent possibly taken from the primary screen's
//! wallpaper, and every part of the shell follows the theme.

use std::{collections::BTreeMap, path::PathBuf, sync::mpsc};

use hocus_core::config::WallpaperSpec;
use hocus_shell::wallpaper::{self, Mode, Picture, Request, Source};
use hocus_ui::{Accent, Color, Theme, ThemeName};
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            ImportMem, Renderer,
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            },
        },
    },
    output::Output,
    reexports::calloop::{LoopHandle, channel},
    utils::{Logical, Physical, Point, Rectangle, Size, Transform},
};

use crate::{App, State};

/// What a screen shows, as last asked: changing any of it asks again.
#[derive(Debug, Clone, PartialEq)]
struct Wanted {
    spec: WallpaperSpec,
    width: u32,
    height: u32,
    /// The ten minutes of the day, for the glow and folders.
    slot: u32,
    base: Color,
    grid: bool,
}

#[derive(Debug)]
struct Paper {
    wanted: Wanted,
    /// The request in flight, to drop results that came too late.
    generation: u64,
    buffer: Option<MemoryRenderBuffer>,
    size: (u32, u32),
    dominant: Option<Color>,
    /// A picture is being made.
    pending: bool,
}

#[derive(Debug)]
pub struct Wallpapers {
    papers: BTreeMap<String, Paper>,
    jobs: mpsc::Sender<(String, u64, Request)>,
    generation: u64,
}

struct Done {
    output: String,
    generation: u64,
    picture: Result<Picture, String>,
}

pub fn start(handle: &LoopHandle<'static, App>, state: &mut State) {
    let (jobs, queue) = mpsc::channel::<(String, u64, Request)>();
    let (done, results) = channel::channel::<Done>();
    let inserted = handle.insert_source(results, |event, _, app| {
        if let channel::Event::Msg(done) = event {
            app.timed("wallpaper", |app| app.state.wallpaper_ready(done));
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "no wallpapers");
        return;
    }
    let worker = std::thread::Builder::new()
        .name("wallpapers".into())
        .spawn(move || {
            while let Ok(job) = queue.recv() {
                // Only the latest request of each screen is worth drawing.
                let mut latest = BTreeMap::new();
                latest.insert(job.0.clone(), job);
                while let Ok(more) = queue.try_recv() {
                    latest.insert(more.0.clone(), more);
                }
                for (output, generation, request) in latest.into_values() {
                    let picture = wallpaper::render(&request);
                    if done
                        .send(Done {
                            output,
                            generation,
                            picture,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            }
        });
    if let Err(err) = worker {
        tracing::warn!(%err, "no wallpapers");
        return;
    }
    state.wallpapers = Some(Wallpapers {
        papers: BTreeMap::new(),
        jobs,
        generation: 0,
    });
}

/// `~` at the start is the home directory.
fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(rest))
            .unwrap_or_else(|| PathBuf::from(path)),
        None => PathBuf::from(path),
    }
}

impl State {
    /// The theme the configuration asks for, before any wallpaper accent.
    fn configured_theme(&self) -> Theme {
        let a = &self.config.appearance;
        let mut theme = Theme::builtin(a.theme.parse().unwrap_or(ThemeName::Dark));
        if let Ok(accent) = a.accent.parse::<Accent>() {
            theme = theme.with_accent(accent);
        }
        theme
    }

    /// After each dispatch: asks for the wallpapers that changed, and gives
    /// the shell the theme in effect.
    pub fn refresh_appearance(&mut self) {
        let base = self.configured_theme();
        let grid = base.name == "hud";
        let slot = self.wall_minutes() / 10;
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        let Some(papers) = self.wallpapers.as_mut() else {
            return;
        };
        papers
            .papers
            .retain(|name, _| outputs.iter().any(|o| o.name() == *name));
        for output in &outputs {
            let Some(mode) = output.current_mode() else {
                continue;
            };
            let name = output.name();
            let spec = self.config.wallpaper.for_output(&name);
            let still = spec.kind == "image" || spec.kind == "colour";
            let wanted = Wanted {
                width: mode.size.w.max(1) as u32,
                height: mode.size.h.max(1) as u32,
                slot: if still { 0 } else { slot },
                base: base.colors.background,
                grid,
                spec,
            };
            let current = papers.papers.get(&name).map(|p| &p.wanted);
            if current == Some(&wanted) {
                continue;
            }
            papers.generation += 1;
            let generation = papers.generation;
            let s = &wanted.spec;
            let colour = s
                .colour
                .as_deref()
                .and_then(hocus_core::config::parse_hex)
                .map_or(wanted.base, |[r, g, b]| Color::rgb(r, g, b));
            let source = match s.kind.as_str() {
                "image" => s.image.as_deref().map(|p| Source::Image(expand(p))),
                "folder" => s.folder.as_deref().map(|p| Source::Folder(expand(p))),
                "colour" => Some(Source::Colour),
                _ => None,
            }
            .unwrap_or(Source::Glow { grid });
            let request = Request {
                source,
                mode: Mode::parse(&s.mode),
                colour,
                width: wanted.width,
                height: wanted.height,
                hour: (wanted.slot * 10) as f32 / 60.0,
            };
            let paper = papers.papers.entry(name.clone()).or_insert_with(|| Paper {
                wanted: wanted.clone(),
                generation,
                buffer: None,
                size: (0, 0),
                dominant: None,
                pending: true,
            });
            paper.wanted = wanted;
            paper.generation = generation;
            paper.pending = true;
            let _ = papers.jobs.send((name, generation, request));
        }
        self.apply_theme();
    }

    /// Minutes since midnight, as the user reads the time.
    fn wall_minutes(&self) -> u32 {
        let t = self.wall_time();
        u32::from(t.hour) * 60 + u32::from(t.minute)
    }

    fn wallpaper_ready(&mut self, done: Done) {
        let Some(papers) = self.wallpapers.as_mut() else {
            return;
        };
        let Some(paper) = papers.papers.get_mut(&done.output) else {
            return;
        };
        if paper.generation != done.generation {
            return;
        }
        paper.pending = false;
        match done.picture {
            Ok(picture) => {
                paper.buffer = Some(MemoryRenderBuffer::from_slice(
                    &picture.pixels,
                    Fourcc::Abgr8888,
                    (picture.width as i32, picture.height as i32),
                    1,
                    Transform::Normal,
                    None,
                ));
                paper.size = (picture.width, picture.height);
                paper.dominant = picture.dominant;
            }
            // The previous picture stays; the configuration says why.
            Err(err) => {
                tracing::warn!(output = done.output, %err, "wallpaper not shown");
                self.config_warnings.push(format!("wallpaper: {err}"));
            }
        }
        self.needs_redraw = true;
        self.apply_theme();
    }

    /// The theme in effect: the configured one, its accent from the
    /// primary screen's wallpaper when asked.
    pub fn theme(&self) -> Theme {
        let theme = self.configured_theme();
        if self.config.appearance.accent != "wallpaper" {
            return theme;
        }
        let primary = self.model.primary().map(|o| o.name.0.clone());
        let dominant = self.wallpapers.as_ref().and_then(|w| {
            primary
                .and_then(|p| w.papers.get(&p))
                .or_else(|| w.papers.values().next())
                .and_then(|p| p.dominant)
        });
        match dominant {
            Some(c) => theme.with_custom_accent(c),
            None => theme,
        }
    }

    /// Gives every part of the shell the theme in effect, when it changed.
    fn apply_theme(&mut self) {
        let theme = self.theme();
        if self.applied_theme.as_ref() == Some(&theme) {
            return;
        }
        if let Some(shell) = self.shell.as_mut() {
            shell.set_theme(theme.clone());
        }
        if let Some(toasts) = self.toasts.as_mut() {
            toasts.set_theme(theme.clone());
        }
        if let Some(launcher) = self.launcher.as_mut() {
            launcher.set_theme(theme.clone());
        }
        if let Some(quick) = self.quick.as_mut() {
            quick.set_theme(theme.clone());
        }
        self.applied_theme = Some(theme);
        self.needs_redraw = true;
    }

    /// The wallpaper of `output`, at the very back.
    pub fn wallpaper_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Vec<MemoryRenderBufferRenderElement<R>>
    where
        R: Renderer + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let Some(paper) = self
            .wallpapers
            .as_ref()
            .and_then(|w| w.papers.get(&output.name()))
        else {
            return Vec::new();
        };
        let (Some(buffer), Some(area)) = (&paper.buffer, self.space.output_geometry(output)) else {
            return Vec::new();
        };
        let (w, h) = paper.size;
        let all = Rectangle::from_size(Size::from((f64::from(w), f64::from(h))));
        let size: Size<i32, Logical> = area.size;
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            Point::<f64, Physical>::from((0.0, 0.0)),
            buffer,
            None,
            Some(all),
            Some(size),
            Kind::Unspecified,
        ) {
            Ok(element) => vec![element],
            Err(err) => {
                tracing::warn!(?err, "could not draw the wallpaper");
                Vec::new()
            }
        }
    }

    /// Whether every screen has the picture it asked for (tests wait for
    /// it before looking).
    pub fn wallpapers_ready(&self) -> bool {
        self.wallpapers
            .as_ref()
            .is_none_or(|w| w.papers.values().all(|p| !p.pending))
    }

    /// The accent in effect, as `#rrggbb`, for the control API.
    pub fn accent_hex(&self) -> String {
        let c = self.theme().colors.accent;
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    }
}
