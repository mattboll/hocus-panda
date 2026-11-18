//! The desktop's living wallpapers drawn for the camera (lot 3): behind
//! the person (Fond) and under « Je reviens »'s card. The compositor's
//! own shaders (`hocus_scenes::shaders`, [`crate::glsl`]), drawn with
//! wgpu on a thread of their own, about 30 times a second while one is
//! wanted, and nothing while none is.
//!
//! - **Where**: a GPU first (an integrated one, as the frames'); else a
//!   CPU's Vulkan (lavapipe), at a quarter of the pixels; else none, and
//!   the frames make do ([`Scenes::latest`] says nothing).
//! - **Their time**: the scene's own clock from when it was first wanted,
//!   so that it moves on as it does on the screen; the aurora moves as it
//!   does with the music, its swell without the beat's ripple (no music
//!   plays in a call), so that it lives behind the person.
//! - **What it makes**: RGBA, opaque, the frame's size or a quarter of it
//!   (the frames scale it).

use std::{
    collections::HashMap,
    sync::{Arc, Condvar, Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::camera::Scene;
use hocus_scenes::shaders;

use crate::{
    glsl::{self, Adapted, Stage},
    gpu::ready,
    look::{H, W},
};

/// The camera's frames come this often at most.
const EVERY: Duration = Duration::from_millis(33);
/// The board's moment the aurora starts at (its still frame on the
/// screen).
const AURORA_START: f64 = 12.0;

/// A scene's picture: RGBA, `width` × `height`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub scene: Scene,
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Default)]
struct Wish {
    scene: Option<Scene>,
    quit: bool,
}

#[derive(Debug, Default)]
struct Shared {
    wish: Mutex<Wish>,
    woken: Condvar,
    latest: Mutex<Option<Arc<Picture>>>,
}

/// The scenes' thread: told which to draw, its latest picture.
#[derive(Debug)]
pub struct Scenes {
    shared: Arc<Shared>,
}

impl Drop for Scenes {
    fn drop(&mut self) {
        self.shared
            .wish
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .quit = true;
        self.shared.woken.notify_all();
    }
}

impl Scenes {
    /// The thread, waiting for a scene to draw. `cpu_too`: a CPU's Vulkan
    /// will do when there is no GPU.
    pub fn start(cpu_too: bool) -> Self {
        let shared = Arc::new(Shared::default());
        let theirs = shared.clone();
        let spawned = std::thread::Builder::new()
            .name("camera's scenes".to_owned())
            .spawn(move || run(&theirs, cpu_too));
        if let Err(err) = spawned {
            tracing::warn!(%err, "no thread for the living wallpapers");
        }
        Self { shared }
    }

    /// Draw `scene` from now on, or none.
    pub fn want(&self, scene: Option<Scene>) {
        let mut wish = self
            .shared
            .wish
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if wish.scene != scene {
            wish.scene = scene;
            self.shared.woken.notify_all();
        }
    }

    /// The latest picture of the scene wanted, if one was drawn.
    pub fn latest(&self, scene: Scene) -> Option<Arc<Picture>> {
        self.shared
            .latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .filter(|p| p.scene == scene)
    }
}

fn run(shared: &Shared, cpu_too: bool) {
    let mut painter: Option<Option<Painter>> = None;
    let mut clocks: HashMap<Scene, Instant> = HashMap::new();
    loop {
        let scene = {
            let mut wish = shared.wish.lock().unwrap_or_else(PoisonError::into_inner);
            while wish.scene.is_none() && !wish.quit {
                wish = shared
                    .woken
                    .wait(wish)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            if wish.quit {
                return;
            }
            wish.scene
        };
        let Some(scene) = scene else {
            continue;
        };
        let started = Instant::now();
        let painter = painter.get_or_insert_with(|| Painter::new(cpu_too));
        let Some(painter) = painter.as_mut() else {
            // Nothing to draw with: told once, then nothing is drawn.
            let mut wish = shared.wish.lock().unwrap_or_else(PoisonError::into_inner);
            while !wish.quit {
                wish = shared
                    .woken
                    .wait(wish)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            return;
        };
        let since = clocks.entry(scene).or_insert(started).elapsed();
        match painter.draw(scene, since.as_secs_f64()) {
            Some(picture) => {
                *shared.latest.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(Arc::new(picture));
            }
            None => tracing::debug!(?scene, "a scene's picture not drawn"),
        }
        if let Some(rest) = EVERY.checked_sub(started.elapsed()) {
            std::thread::sleep(rest);
        }
    }
}

/// A shader's pipeline and its uniforms' and textures' layout.
struct Pass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    vertex: Adapted,
    fragment: Adapted,
}

/// What a pass's vertices are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vertices {
    /// A picture's quad: its place and its pixels (the compositor's
    /// `matrix::VERTEX`).
    Quad,
    /// A mesh of shapes (`hocus_scenes::mesh`): place, place across,
    /// colour, shape (`SPARKS_VERTEX`).
    Mesh,
}

/// The koi's pond as the camera draws it: the compositor's own pond
/// (`hocus_scenes::koi`), its floor, what is under its water and its
/// heights, kept between frames.
struct Koi {
    pond: hocus_scenes::koi::Pond,
    rng: hocus_scenes::particles::Rng,
    steps: u64,
    floor: wgpu::Texture,
    under: wgpu::Texture,
    heights: wgpu::Texture,
    shapes: Pass,
    water: Pass,
}

/// The pond's size, logical: the frame's, as a screen of its size.
const POND: (f32, f32) = (W as f32, H as f32);
/// At most this many of its steps at once (10 s).
const MOST_STEPS: u64 = 600;

/// The device the scenes are drawn with, its target and what reads it
/// back.
pub struct Painter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    width: usize,
    height: usize,
    target: wgpu::Texture,
    read: wgpu::Buffer,
    sampler: wgpu::Sampler,
    passes: HashMap<Scene, Pass>,
    koi: Option<Koi>,
}

impl std::fmt::Debug for Painter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Painter")
            .field("size", &(self.width, self.height))
            .finish_non_exhaustive()
    }
}

/// The matrix taking a picture's pixels (`w` × `h`, from its upper left)
/// to the clip's space.
fn matrix(w: f32, h: f32) -> [f32; 9] {
    [2.0 / w, 0.0, 0.0, 0.0, -2.0 / h, 0.0, -1.0, 1.0, 1.0]
}

impl Painter {
    /// A GPU, else (`cpu_too`) a CPU's Vulkan at a quarter of the pixels.
    pub fn new(cpu_too: bool) -> Option<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
            ..wgpu::InstanceDescriptor::default()
        });
        let adapter = ready(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))?
        .map_err(|err| tracing::info!(%err, "no GPU for the living wallpapers"))
        .ok()?;
        let info = adapter.get_info();
        let cpu = info.device_type == wgpu::DeviceType::Cpu;
        if cpu && !cpu_too {
            tracing::info!(adapter = info.name, "no GPU for the living wallpapers");
            return None;
        }
        let (device, queue) = ready(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("camera's scenes"),
            required_limits: wgpu::Limits::downlevel_defaults(),
            ..wgpu::DeviceDescriptor::default()
        }))?
        .map_err(|err| tracing::warn!(%err, "the GPU does not start for the scenes"))
        .ok()?;
        device.on_uncaptured_error(Arc::new(|err| {
            tracing::warn!(%err, "the GPU said, of the scenes");
        }));
        let (width, height) = if cpu { (W / 2, H / 2) } else { (W, H) };
        tracing::info!(adapter = info.name, width, height, "the living wallpapers' GPU");
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("scene"),
            size: extent(width, height),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene read back"),
            size: (width * height * 4) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("scene"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        Some(Self {
            device,
            queue,
            width,
            height,
            target,
            read,
            sampler,
            passes: HashMap::new(),
            koi: None,
        })
    }

    /// `scene` at `t` seconds of its own clock.
    pub fn draw(&mut self, scene: Scene, t: f64) -> Option<Picture> {
        let encoder = match scene {
            Scene::Aurora => self.aurora(t)?,
            Scene::Koi => self.koi(t)?,
        };
        self.read_back(encoder, scene)
    }

    /// The aurora: the compositor's shader over the frame.
    fn aurora(&mut self, t: f64) -> Option<wgpu::CommandEncoder> {
        if !self.passes.contains_key(&Scene::Aurora) {
            let pass = make_pass(
                &self.device,
                shaders::VERTEX,
                shaders::AURORA,
                Vertices::Quad,
                None,
            )?;
            self.passes.insert(Scene::Aurora, pass);
        }
        let pass = self.passes.get(&Scene::Aurora)?;
        let (w, h) = (self.width as f32, self.height as f32);
        let time = AURORA_START + t;
        let (energy, _) = hocus_scenes::aurora::drive(time);
        let bind = self.bind(
            pass,
            &[("matrix", &matrix(w, h))],
            &[
                ("size", &[W as f32, H as f32]),
                ("res", &[w, h]),
                ("time", &[time as f32]),
                ("amp", &[energy as f32]),
                ("beat", &[0.0]),
            ],
            &[],
        );
        let quad = self.quad(w, h);
        let target = self.target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.encoder();
        draw(&mut encoder, &target, true, pass, &bind, &quad, None, 6);
        Some(encoder)
    }

    /// The koi's pond: stepped to `t`, the koi and their shadows drawn
    /// over its floor, seen through its water, the pads over it; as the
    /// compositor draws it on a screen of the frame's size.
    fn koi(&mut self, t: f64) -> Option<wgpu::CommandEncoder> {
        if self.koi.is_none() {
            self.koi = Some(self.dig()?);
        }
        let koi = self.koi.as_mut()?;
        let due = (t / f64::from(hocus_scenes::particles::STEP)) as u64;
        let from = koi.steps.max(due.saturating_sub(MOST_STEPS));
        for _ in from..due {
            koi.pond.step(
                hocus_scenes::particles::STEP,
                hocus_scenes::koi::Pointer::default(),
                &mut koi.rng,
            );
        }
        koi.steps = koi.steps.max(due);
        let koi = self.koi.as_ref()?;
        let (w, h) = (self.width as f32, self.height as f32);
        let scale = w / POND.0;
        let (gw, gh) = koi.pond.grid;
        // The heights, the board's `cur × 28 / 255` in half floats.
        let halves: Vec<u8> = koi
            .pond
            .cur
            .iter()
            .flat_map(|v| tract_onnx::prelude::f16::from_f32(v * 28.0 / 255.0).to_le_bytes())
            .collect();
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &koi.heights,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &halves,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(gw as u32 * 2),
                rows_per_image: Some(gh as u32),
            },
            extent(gw, gh),
        );
        let mut encoder = self.encoder();
        // Under the water: the floor, then the koi and their shadows.
        encoder.copy_texture_to_texture(
            koi.floor.as_image_copy(),
            koi.under.as_image_copy(),
            extent(POND.0 as usize, POND.1 as usize),
        );
        let under_view = koi.under.create_view(&wgpu::TextureViewDescriptor::default());
        let clip: [(&str, &[f32]); 3] = [
            ("keep", &[-1.0e7, -1.0e7, 1.0e7, 1.0e7]),
            ("count", &[0.0]),
            ("corner", &[0.0]),
        ];
        let fish = koi.pond.under_mesh();
        if let Some((vertices, indices, n)) = self.mesh(&fish) {
            let bind = self.bind(&koi.shapes, &[("matrix", &matrix(POND.0, POND.1))], &clip, &[]);
            draw(&mut encoder, &under_view, false, &koi.shapes, &bind, &vertices, Some(&indices), n);
        }
        // The water, over the frame.
        let look: [(&str, &[f32]); 6] = [
            ("screen", &[w, h]),
            ("grid", &[POND.0 / (gw as f32 * hocus_scenes::koi::CELL), POND.1 / (gh as f32 * hocus_scenes::koi::CELL)]),
            ("texel", &[1.0 / gw as f32, 1.0 / gh as f32]),
            ("bend", &[1200.0 / POND.0, 720.0 / POND.1]),
            ("zero", &[0.0]),
            ("still", &[0.0]),
        ];
        let heights_view = koi.heights.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = self.bind(
            &koi.water,
            &[("matrix", &matrix(w, h))],
            &look,
            &[&under_view, &heights_view],
        );
        let quad = self.quad(w, h);
        let target = self.target.create_view(&wgpu::TextureViewDescriptor::default());
        draw(&mut encoder, &target, true, &koi.water, &bind, &quad, None, 6);
        // The pads, bobbing on it.
        let pads = koi.pond.pads_mesh(scale);
        if let Some((vertices, indices, n)) = self.mesh(&pads) {
            let bind = self.bind(&koi.shapes, &[("matrix", &matrix(w, h))], &clip, &[]);
            draw(&mut encoder, &target, false, &koi.shapes, &bind, &vertices, Some(&indices), n);
        }
        Some(encoder)
    }

    /// The pond dug: its floor drawn (once), its textures and passes made.
    fn dig(&self) -> Option<Koi> {
        let mut rng = hocus_scenes::particles::Rng::new(SEED);
        let pond = hocus_scenes::koi::Pond::new(POND, [0.0, 0.0, POND.0, POND.1], &mut rng);
        let (pw, ph) = (POND.0 as usize, POND.1 as usize);
        let floor_pixels = hocus_scenes::koi::floor(pw as u32, ph as u32, SEED);
        let texture = |label: &str, (w, h): (usize, usize), format, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent(w, h),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        use wgpu::{TextureFormat as F, TextureUsages as U};
        let floor = texture("koi floor", (pw, ph), F::Rgba8Unorm, U::COPY_SRC | U::COPY_DST);
        self.queue.write_texture(
            floor.as_image_copy(),
            &floor_pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pw as u32 * 4),
                rows_per_image: Some(ph as u32),
            },
            extent(pw, ph),
        );
        let under = texture(
            "koi under",
            (pw, ph),
            F::Rgba8Unorm,
            U::RENDER_ATTACHMENT | U::TEXTURE_BINDING | U::COPY_DST,
        );
        let heights = texture(
            "koi heights",
            pond.grid,
            F::R16Float,
            U::TEXTURE_BINDING | U::COPY_DST,
        );
        let with_clip = shaders::SHAPES.replacen(
            "varying vec2 v_at;\n",
            &format!("varying vec2 v_at;\n{}", shaders::CLIP),
            1,
        );
        let over = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;
        let shapes = make_pass(
            &self.device,
            shaders::SPARKS_VERTEX,
            &with_clip,
            Vertices::Mesh,
            Some(over),
        )?;
        let water = make_pass(
            &self.device,
            shaders::VERTEX,
            shaders::KOI,
            Vertices::Quad,
            None,
        )?;
        tracing::info!(koi = pond.koi.len(), "the camera's koi pond dug");
        Some(Koi {
            pond,
            rng,
            steps: 0,
            floor,
            under,
            heights,
            shapes,
            water,
        })
    }

    fn encoder(&self) -> wgpu::CommandEncoder {
        self.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("scene"),
            })
    }

    /// A pass's uniforms and textures bound.
    fn bind(
        &self,
        pass: &Pass,
        vertex: &[(&str, &[f32])],
        fragment: &[(&str, &[f32])],
        textures: &[&wgpu::TextureView],
    ) -> wgpu::BindGroup {
        let uniform = |bytes: &[u8], label: &str| {
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes.len() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&buffer, 0, bytes);
            buffer
        };
        let vb = uniform(&pass.vertex.block.bytes(vertex), "vertex look");
        let fb = uniform(&pass.fragment.block.bytes(fragment), "fragment look");
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: vb.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: fb.as_entire_binding(),
            },
        ];
        for (i, view) in textures.iter().enumerate() {
            let at = 2 + 2 * i as u32;
            entries.push(wgpu::BindGroupEntry {
                binding: at,
                resource: wgpu::BindingResource::TextureView(view),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: at + 1,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            });
        }
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene"),
            layout: &pass.layout,
            entries: &entries,
        })
    }

    /// A picture's two triangles, `w` × `h`: their places, and their
    /// pixels.
    fn quad(&self, w: f32, h: f32) -> wgpu::Buffer {
        let quad: Vec<f32> = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, 0.0), (w, h), (0.0, h)]
            .iter()
            .flat_map(|&(x, y)| [x, y, x, y])
            .collect();
        self.buffer(&quad.iter().flat_map(|v| v.to_ne_bytes()).collect::<Vec<u8>>(), wgpu::BufferUsages::VERTEX)
    }

    /// A mesh's vertices and indices, and how many; none when empty.
    fn mesh(&self, mesh: &hocus_scenes::mesh::Mesh) -> Option<(wgpu::Buffer, wgpu::Buffer, u32)> {
        if mesh.is_empty() {
            return None;
        }
        let vertices: Vec<u8> = mesh.vertices.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let mut indices: Vec<u8> = mesh.indices.iter().flat_map(|i| i.to_ne_bytes()).collect();
        // Buffers are written four bytes at a time.
        indices.resize(indices.len().div_ceil(4) * 4, 0);
        Some((
            self.buffer(&vertices, wgpu::BufferUsages::VERTEX),
            self.buffer(&indices, wgpu::BufferUsages::INDEX),
            mesh.indices.len() as u32,
        ))
    }

    fn buffer(&self, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene"),
            size: bytes.len().max(4) as u64,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&buffer, 0, bytes);
        buffer
    }

    fn read_back(&self, mut encoder: wgpu::CommandEncoder, scene: Scene) -> Option<Picture> {
        let (width, height) = (self.width, self.height);
        encoder.copy_texture_to_buffer(
            self.target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width as u32 * 4),
                    rows_per_image: Some(height as u32),
                },
            },
            extent(width, height),
        );
        self.queue.submit([encoder.finish()]);
        let slice = self.read.slice(..);
        let mapped = Arc::new(Mutex::new(None));
        let told = mapped.clone();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            if let Ok(mut told) = told.lock() {
                *told = Some(result.is_ok());
            }
        });
        if let Err(err) = self.device.poll(wgpu::PollType::wait_indefinitely()) {
            tracing::warn!(?err, "a scene's picture is not read");
            return None;
        }
        if mapped.lock().ok().and_then(|m| *m) != Some(true) {
            return None;
        }
        let rgba = slice.get_mapped_range().to_vec();
        self.read.unmap();
        Some(Picture {
            scene,
            width,
            height,
            rgba,
        })
    }
}

/// The pond's seed: the same pond each time.
const SEED: u64 = 803;

/// `pass` drawn into `target` (cleared first, or over what it holds).
#[expect(clippy::too_many_arguments, reason = "a draw's parts")]
fn draw(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    clear: bool,
    pass: &Pass,
    bind: &wgpu::BindGroup,
    vertices: &wgpu::Buffer,
    indices: Option<&wgpu::Buffer>,
    count: u32,
) {
    let load = if clear {
        wgpu::LoadOp::Clear(wgpu::Color::BLACK)
    } else {
        wgpu::LoadOp::Load
    };
    let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("scene"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    rp.set_pipeline(&pass.pipeline);
    rp.set_bind_group(0, bind, &[]);
    rp.set_vertex_buffer(0, vertices.slice(..));
    match indices {
        Some(indices) => {
            rp.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint16);
            rp.draw_indexed(0..count, 0, 0..1);
        }
        None => rp.draw(0..count, 0..1),
    }
}

fn extent(width: usize, height: usize) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: width as u32,
        height: height as u32,
        depth_or_array_layers: 1,
    }
}

/// A pass of the compositor's shaders, adapted: `vertex` and `fragment`,
/// its vertices `vertices`, blended by `blend` (else written).
fn make_pass(
    device: &wgpu::Device,
    vertex: &str,
    fragment: &str,
    vertices: Vertices,
    blend: Option<wgpu::BlendState>,
) -> Option<Pass> {
    let vertex = glsl::adapt(vertex, Stage::Vertex, 0);
    let fragment = glsl::adapt(fragment, Stage::Fragment, 1);
    let module = |adapted: &Adapted, stage: wgpu::naga::ShaderStage, label: &str| {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Glsl {
                shader: adapted.source.clone().into(),
                stage,
                defines: &[],
            },
        })
    };
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let vs = module(&vertex, wgpu::naga::ShaderStage::Vertex, "scene's vertices");
    let fs = module(&fragment, wgpu::naga::ShaderStage::Fragment, "scene");
    let uniform = |binding: u32, visibility: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let mut entries = vec![
        uniform(0, wgpu::ShaderStages::VERTEX),
        uniform(1, wgpu::ShaderStages::FRAGMENT),
    ];
    for i in 0..fragment.textures.len() as u32 {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 2 + 2 * i,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 3 + 2 * i,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
    }
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("scene"),
        entries: &entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("scene"),
        bind_group_layouts: &[&layout],
        push_constant_ranges: &[],
    });
    let attribute = |format, offset, shader_location| wgpu::VertexAttribute {
        format,
        offset,
        shader_location,
    };
    use wgpu::VertexFormat::{Float32x2 as V2, Float32x4 as V4};
    let (stride, attributes) = match vertices {
        Vertices::Quad => (16, vec![attribute(V2, 0, 0), attribute(V2, 8, 1)]),
        Vertices::Mesh => (
            (hocus_scenes::particles::FLOATS * 4) as u64,
            vec![
                attribute(V2, 0, 0),
                attribute(V2, 8, 1),
                attribute(V4, 16, 2),
                attribute(V4, 32, 3),
            ],
        ),
    };
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("scene"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &vs,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: stride,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            }],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &fs,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    });
    if let Some(err) = ready(device.pop_error_scope()).flatten() {
        tracing::warn!(%err, "a living wallpaper's shader does not build");
        return None;
    }
    Some(Pass {
        pipeline,
        layout,
        vertex,
        fragment,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Runs `test` again in a process of its own with lavapipe (Mesa's
    /// Vulkan on the CPU) alone shown to Vulkan's loader, which then opens
    /// no render node; `true` in that process.
    pub(crate) fn on_lavapipe(test: &str) -> bool {
        const CHILD: &str = "HOCUS_CAMERA_LAVAPIPE";
        if std::env::var_os(CHILD).is_some() {
            return true;
        }
        let lvp = std::fs::read_dir("/usr/share/vulkan/icd.d")
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("lvp_icd"))
            })
            .expect("lavapipe (mesa-vulkan-drivers) is needed");
        let status = std::process::Command::new(std::env::current_exe().expect("the test"))
            .args(["--exact", test])
            .env(CHILD, "1")
            .env("VK_DRIVER_FILES", &lvp)
            .env("VK_ICD_FILENAMES", &lvp)
            .env("WGPU_BACKEND", "vulkan")
            .status()
            .expect("the test again");
        assert!(status.success());
        false
    }

    /// The koi's pond, the compositor's own: its water over its floor,
    /// koi swimming in it.
    #[test]
    fn the_koi_pond_draws_and_its_koi_swim() {
        if !on_lavapipe("scenes::tests::the_koi_pond_draws_and_its_koi_swim") {
            return;
        }
        let mut painter = Painter::new(true).expect("lavapipe");
        let a = painter.draw(Scene::Koi, 0.0).expect("a picture");
        let b = painter.draw(Scene::Koi, 1.0).expect("another");
        let mean = |p: &Picture| {
            let mut sum = [0f64; 3];
            for px in p.rgba.as_chunks::<4>().0 {
                for c in 0..3 {
                    sum[c] += f64::from(px[c]);
                }
            }
            sum.map(|s| s / (p.width * p.height) as f64)
        };
        // The pond's teal: green and blue over red.
        let [r, g, b_] = mean(&a);
        assert!(g > r && b_ > r && g > 20.0, "{r} {g} {b_}");
        // Some bright koi in it.
        assert!(
            a.rgba.as_chunks::<4>().0.iter().filter(|p| p[0] > 180).count() > 500,
            "no koi"
        );
        assert_ne!(a.rgba, b.rgba, "they stand still");
        if let Ok(dir) = std::env::var("HOCUS_SHOTS") {
            let _ = image::save_buffer(
                std::path::Path::new(&dir).join("koi.png"),
                &a.rgba,
                a.width as u32,
                a.height as u32,
                image::ColorType::Rgba8,
            );
        }
    }

    /// The aurora, the compositor's own shader: curtains of green over a
    /// dark sky, moving.
    #[test]
    fn the_aurora_draws_and_moves() {
        if !on_lavapipe("scenes::tests::the_aurora_draws_and_moves") {
            return;
        }
        let mut painter = Painter::new(true).expect("lavapipe");
        let a = painter.draw(Scene::Aurora, 0.0).expect("a picture");
        let b = painter.draw(Scene::Aurora, 2.0).expect("another");
        assert_eq!((a.width, a.height), (W / 2, H / 2));
        let mean = |p: &Picture| {
            let mut sum = [0f64; 3];
            for px in p.rgba.as_chunks::<4>().0 {
                for c in 0..3 {
                    sum[c] += f64::from(px[c]);
                }
            }
            sum.map(|s| s / (p.width * p.height) as f64)
        };
        let [r, g, b_] = mean(&a);
        assert!(g > r && g > 8.0 && b_ < 120.0, "{r} {g} {b_}");
        assert_ne!(a.rgba, b.rgba, "it stands still");
        if let Ok(dir) = std::env::var("HOCUS_SHOTS") {
            let _ = image::save_buffer(
                std::path::Path::new(&dir).join("aurora.png"),
                &a.rgba,
                a.width as u32,
                a.height as u32,
                image::ColorType::Rgba8,
            );
        }
    }
}
