//! Wallpapers and the shell's theme. Pictures are made on a worker thread
//! (decoding, scaling, the glow) and come back through a channel; a screen
//! keeps its picture until the new one is ready. The theme follows the
//! configuration, its accent possibly taken from the primary screen's
//! wallpaper, and every part of the shell follows the theme.

use std::{collections::BTreeMap, path::PathBuf, sync::mpsc};

use hocus_core::config::WallpaperSpec;
use hocus_shell::{
    matrix,
    wallpaper::{self, Mode, Pattern, Picture, Request, Source},
};
use hocus_ui::{Accent, Color, Theme, ThemeName};
use smithay::{
    backend::renderer::{
        ImportMem, Renderer,
        element::{
            Kind,
            memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
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
    /// Physical pixels per logical one, for the size of a pattern.
    scale: f64,
    /// The ten minutes of the day, for the glow and folders.
    slot: u32,
    base: Color,
    pattern: Pattern,
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
    /// What the wallpapers and the theme were last worked out from,
    /// summed up: nothing is worked out again until it changes (#88).
    inputs: Option<u64>,
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
                    // In the order the compositor's buffers are, off the loop.
                    let picture = wallpaper::render(&request).map(|mut picture| {
                        crate::shell::into_order(&mut picture.pixels);
                        picture
                    });
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
        inputs: None,
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
    /// How the desktop shown on `output` looks, if it has a look of its
    /// own (#49).
    fn look_on(&self, output: &str) -> Option<&hocus_core::config::DesktopLook> {
        let o = self
            .model
            .output(&hocus_core::OutputName(output.to_owned()))?;
        let index = o.desktops.iter().position(|d| *d == o.shown)?;
        self.config.desktops.look(output, index)
    }

    /// The theme the configuration asks for on `output`, its shown
    /// desktop's own over `[appearance]`, before any wallpaper accent.
    fn configured_theme_on(&self, output: Option<&str>) -> Theme {
        let a = &self.config.appearance;
        let look = output.and_then(|o| self.look_on(o));
        let name = look.and_then(|l| l.theme.as_deref()).unwrap_or(&a.theme);
        let accent = look.and_then(|l| l.accent.as_deref()).unwrap_or(&a.accent);
        let hour = self.wall_time().hour;
        let mut theme = Theme::builtin(name.parse().unwrap_or(ThemeName::Dark))
            .with_daylight(hocus_ui::Daylight::at(hour));
        if let Ok(accent) = accent.parse::<Accent>() {
            theme = theme.with_accent(accent);
        }
        if a.high_contrast {
            theme = theme.with_high_contrast();
        }
        theme
            .with_text_size(a.text_size)
            .with_reduced_motion(a.reduced_motion)
            // See-through panels where what is behind them is blurred.
            .with_glass(self.glass_on())
            // Panels cast windows' shadow, drawn under them (#281).
            .with_casting(hocus_ui::Casting::Compositor)
    }

    /// After each dispatch: asks for the wallpapers that changed, and gives
    /// the shell the theme in effect.
    pub fn refresh_appearance(&mut self) {
        // Nothing it reads changed since the last turn: neither the
        // wallpapers nor the theme do.
        let inputs = self.appearance_inputs();
        match self.wallpapers.as_mut() {
            None => return,
            Some(papers) if papers.inputs == Some(inputs) => return,
            Some(papers) => papers.inputs = Some(inputs),
        }
        let slot = self.wall_minutes() / 10;
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        // Each output's look: its shown desktop's own, or the general one.
        let looks: Vec<(Theme, hocus_core::config::WallpaperSpec)> = outputs
            .iter()
            .map(|o| {
                let name = o.name();
                (
                    self.configured_theme_on(Some(&name)),
                    self.config
                        .wallpaper
                        .for_desktop(&name, self.look_on(&name)),
                )
            })
            .collect();
        let Some(papers) = self.wallpapers.as_mut() else {
            return;
        };
        papers
            .papers
            .retain(|name, _| outputs.iter().any(|o| o.name() == *name));
        for (output, (base, spec)) in outputs.iter().zip(looks) {
            let Some(mode) = output.current_mode() else {
                continue;
            };
            let name = output.name();
            let pattern = match base.name.parse::<ThemeName>() {
                Ok(ThemeName::Hud) => Pattern::Grid,
                Ok(ThemeName::Matrix) => Pattern::Rain,
                _ => Pattern::None,
            };
            let still = spec.kind == "image" || spec.kind == "colour";
            let wanted = Wanted {
                width: mode.size.w.max(1) as u32,
                height: mode.size.h.max(1) as u32,
                scale: output.current_scale().fractional_scale(),
                slot: if still { 0 } else { slot },
                base: base.colors.background,
                pattern,
                spec,
            };
            let current = papers.papers.get(&name).map(|p| &p.wanted);
            if current == Some(&wanted) {
                continue;
            }
            papers.generation += 1;
            let generation = papers.generation;
            let s = &wanted.spec;
            // The rain and the stars fall on their own night, whatever
            // the theme; still here, they move where the renderer can
            // (`backdrop`).
            let night = matches!(s.kind.as_str(), "rain" | "stars");
            let base = if night { matrix::BLACK } else { wanted.base };
            let colour = s
                .colour
                .as_deref()
                .and_then(hocus_core::config::parse_hex)
                .map_or(base, |[r, g, b]| Color::rgb(r, g, b));
            let source = match s.kind.as_str() {
                "image" => s.image.as_deref().map(|p| Source::Image(expand(p))),
                "folder" => s.folder.as_deref().map(|p| Source::Folder(expand(p))),
                "colour" => Some(Source::Colour),
                "rain" => Some(Source::Glow(Pattern::Rain)),
                "stars" => Some(Source::Glow(Pattern::Stars)),
                _ => None,
            }
            .unwrap_or(Source::Glow(pattern));
            let request = Request {
                source,
                mode: Mode::parse(&s.mode),
                colour,
                width: wanted.width,
                height: wanted.height,
                scale: wanted.scale as f32,
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

    /// What [`State::refresh_appearance`] reads, summed up without
    /// allocating: the configuration, the time of day, the screens and the
    /// desktops they show, glass.
    fn appearance_inputs(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        self.config_generation.hash(&mut hasher);
        let time = self.wall_time();
        (time.hour, time.minute / 10).hash(&mut hasher);
        self.glass_on().hash(&mut hasher);
        for output in self.space.outputs() {
            output
                .current_mode()
                .map(|m| (m.size.w, m.size.h))
                .hash(&mut hasher);
            output
                .current_scale()
                .fractional_scale()
                .to_bits()
                .hash(&mut hasher);
        }
        for output in self.model.outputs() {
            (&output.name.0, output.shown).hash(&mut hasher);
        }
        self.model.primary().map(|o| &o.name.0).hash(&mut hasher);
        hasher.finish()
    }

    /// Minutes since midnight, as the user reads the time.
    pub(crate) fn wall_minutes(&self) -> u32 {
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
                // In the screens' order: an element put in front of
                // everything (the launcher) moves it back in the stack,
                // which damages all of it, and pixman draws it again whole
                // without converting each pixel (#288). Said to be opaque,
                // it would be copied, not blended, and so would the edge of
                // a mirrored screen's picture, half covered, see-through.
                paper.buffer = Some(MemoryRenderBuffer::from_slice(
                    &picture.pixels,
                    crate::shell::ORDER,
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

    /// The theme in effect where the column is (the primary screen
    /// otherwise): the configured one or that desktop's own, its accent
    /// from that screen's wallpaper when asked.
    pub fn theme(&self) -> Theme {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        self.appearance_inputs().hash(&mut hasher);
        self.shell.is_some().hash(&mut hasher);
        if let Some(papers) = &self.wallpapers {
            for (name, paper) in &papers.papers {
                (name, paper.dominant.map(|c| (c.r, c.g, c.b, c.a))).hash(&mut hasher);
            }
        }
        let inputs = hasher.finish();
        if let Some((seen, theme)) = &*self.theme_memo.borrow()
            && *seen == inputs
        {
            return theme.clone();
        }
        let theme = self.work_out_theme();
        *self.theme_memo.borrow_mut() = Some((inputs, theme.clone()));
        theme
    }

    fn work_out_theme(&self) -> Theme {
        let column = self
            .column_output()
            .map(|o| o.name())
            .or_else(|| self.model.primary().map(|o| o.name.0.clone()));
        let theme = self.configured_theme_on(column.as_deref());
        let accent = column
            .as_deref()
            .and_then(|o| self.look_on(o))
            .and_then(|l| l.accent.as_deref())
            .unwrap_or(&self.config.appearance.accent);
        if accent != "wallpaper" {
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
        // Windows cast the shadow of the hour, as panels do (#161).
        let s = theme.shadow;
        let alpha = if self.config.decorations.shadow {
            f64::from(s.alpha)
        } else {
            0.0
        };
        let shape = (f64::from(s.dx), f64::from(s.dy), f64::from(s.blur), alpha);
        let style = &mut self.style;
        if (
            style.shadow_dx,
            style.shadow_dy,
            style.shadow_blur,
            style.shadow_alpha,
        ) != shape
        {
            (
                style.shadow_dx,
                style.shadow_dy,
                style.shadow_blur,
                style.shadow_alpha,
            ) = shape;
            self.textures.clear();
            self.needs_redraw = true;
        }
        // The focus ring in the accent, unless it has a colour of its own.
        let a = theme.colors.accent;
        if self.config.decorations.accent == "theme" && self.style.accent != [a.r, a.g, a.b] {
            self.style.accent = [a.r, a.g, a.b];
            self.textures.clear();
            self.needs_redraw = true;
        }
        // Windows' hairline in the theme's tint.
        let b = theme.colors.window_border;
        if self.style.border != [b.r, b.g, b.b, b.a] {
            self.style.border = [b.r, b.g, b.b, b.a];
            self.textures.clear();
            self.needs_redraw = true;
        }
        if self.applied_theme.as_ref() == Some(&theme) {
            return;
        }
        // A desktop's own theme comes with it: the column crossfades as
        // that desktop slides in (a configuration change applies at once).
        let desktop = self.column_output().and_then(|o| {
            let o = self.model.output(&hocus_core::OutputName(o.name()))?;
            o.desktops.iter().position(|d| *d == o.shown)
        });
        let moves = !self.instant_motion && !self.config.appearance.reduced_motion;
        let now = self.clock.now();
        if let Some(shell) = self.shell.as_mut() {
            let switched = shell.themed_desktop.is_some() && shell.themed_desktop != desktop;
            shell.themed_desktop = desktop;
            if switched && moves {
                shell.crossfade_theme(theme.clone(), now);
            } else {
                shell.set_theme(theme.clone());
            }
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
