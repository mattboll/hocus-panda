//! The frames made on the GPU, with wgpu (Vulkan or GL, whatever the
//! vendor): Lumière's correction, its denoise and the banner in one pass
//! of a fragment shader, the same arithmetic as the CPU's
//! ([`crate::look`]), then read back for PipeWire.
//!
//! Each frame: its RGBA written into a texture, drawn into two targets
//! (the picture, and the picture without the banner, which the next
//! frame's denoise reads: two of them, taken in turn), the picture copied
//! into a buffer and read. An integrated GPU first (`LowPower`): the
//! camera's work is small, the battery's matters.

use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};

use crate::look::{BYTES, H, KEEP_NEW, MOVING, Making, Overlay, STILL, W};

/// Where the frames are made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wanted {
    /// A GPU if there is one; a CPU's Vulkan (lavapipe) is no faster
    /// than the CPU's own path: the CPU then.
    Auto,
    /// The CPU's own path.
    Cpu,
    /// Any adapter, a CPU's Vulkan too (tests).
    Any,
}

impl Wanted {
    pub fn of(said: &str) -> Self {
        match said {
            "cpu" => Self::Cpu,
            "any" => Self::Any,
            _ => Self::Auto,
        }
    }
}

/// wgpu's native futures are ready at once.
fn ready<F: Future>(future: F) -> Option<F::Output> {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => Some(output),
        Poll::Pending => None,
    }
}

const SHADER: &str = r"
struct Look {
    gain: vec4<f32>,
    flags: vec4<f32>,
};

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var last: texture_2d<f32>;
@group(0) @binding(2) var overlay: texture_2d<f32>;
@group(0) @binding(3) var<uniform> look: Look;

struct Out {
    @location(0) picture: vec4<f32>,
    @location(1) plain: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn curve(v: f32, g: f32, e: f32) -> f32 {
    let x = min(v * g, 1.0);
    return round(clamp(e * x / (1.0 + (e - 1.0) * x), 0.0, 1.0) * 255.0) / 255.0;
}

@fragment
fn fs(@builtin(position) p: vec4<f32>) -> Out {
    let at = vec2<i32>(p.xy);
    let c = textureLoad(frame, at, 0);
    var rgb = c.rgb;
    if (look.flags.x > 0.5) {
        let e = look.gain.w;
        var fresh = vec3<f32>(curve(c.r, look.gain.r, e), curve(c.g, look.gain.g, e), curve(c.b, look.gain.b, e));
        if (look.flags.y < 0.5) {
            let old = textureLoad(last, at, 0).rgb;
            let d = round(max(max(abs(fresh.r - old.r), abs(fresh.g - old.g)), abs(fresh.b - old.b)) * 255.0);
            let t = clamp((d - STILL) / (MOVING - STILL), 0.0, 1.0);
            let k = KEEP + (1.0 - KEEP) * t * t * (3.0 - 2.0 * t);
            fresh = old + (fresh - old) * k;
        }
        rgb = fresh;
    }
    var out: Out;
    out.plain = vec4<f32>(rgb, 1.0);
    var shown = rgb;
    if (look.flags.w > 0.5) {
        let o = textureLoad(overlay, at, 0);
        shown = o.rgb + shown * (1.0 - o.a);
    }
    out.picture = vec4<f32>(shown, 1.0);
    return out;
}
";

/// The GPU, its pipeline and its textures, while the camera is read.
pub struct Gpu {
    name: String,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    frame: wgpu::Texture,
    overlay: wgpu::Texture,
    picture: wgpu::Texture,
    plains: [wgpu::Texture; 2],
    /// Reading plain 0 (writing 1), and the other way.
    binds: [wgpu::BindGroup; 2],
    uniforms: wgpu::Buffer,
    read: wgpu::Buffer,
    /// The plain texture written last.
    turn: usize,
    /// No last frame to denoise with.
    first: bool,
    /// The overlay in the texture, as last written.
    overlay_shown: Option<Overlay>,
}

impl std::fmt::Debug for Gpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gpu")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Gpu {
    /// The GPU `wanted` takes, ready; `None`: none, or the CPU's then.
    pub fn new(wanted: Wanted) -> Option<Self> {
        if wanted == Wanted::Cpu {
            return None;
        }
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
            ..wgpu::InstanceDescriptor::default()
        });
        let adapter = ready(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))?
        .map_err(|err| tracing::info!(%err, "no GPU: the frames are made on the CPU"))
        .ok()?;
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu && wanted == Wanted::Auto {
            tracing::info!(
                adapter = info.name,
                "a CPU's adapter: the frames are made on the CPU"
            );
            return None;
        }
        let (device, queue) = ready(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("hocus-camera"),
            required_limits: wgpu::Limits::downlevel_defaults(),
            ..wgpu::DeviceDescriptor::default()
        }))?
        .map_err(|err| tracing::warn!(%err, adapter = info.name, "the GPU does not start"))
        .ok()?;
        device.on_uncaptured_error(std::sync::Arc::new(|err| {
            tracing::warn!(%err, "the GPU said");
        }));
        Some(Self::with(info.name, device, queue))
    }

    fn with(name: String, device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let source = SHADER
            .replace("STILL", &format!("{STILL:.1}"))
            .replace("MOVING", &format!("{MOVING:.1}"))
            .replace("KEEP", &format!("{KEEP_NEW:.2}"));
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lumière"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let texture_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lumière"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                texture_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lumière"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let target = Some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::Bgra8Unorm,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lumière"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[target.clone(), target],
            }),
            multiview: None,
            cache: None,
        });
        let size = wgpu::Extent3d {
            width: W as u32,
            height: H as u32,
            depth_or_array_layers: 1,
        };
        let texture = |label: &str, format: wgpu::TextureFormat, usage: wgpu::TextureUsages| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        use wgpu::{TextureFormat as F, TextureUsages as U};
        let frame = texture("frame", F::Rgba8Unorm, U::TEXTURE_BINDING | U::COPY_DST);
        let overlay = texture("overlay", F::Rgba8Unorm, U::TEXTURE_BINDING | U::COPY_DST);
        let picture = texture("picture", F::Bgra8Unorm, U::RENDER_ATTACHMENT | U::COPY_SRC);
        let plains = [0, 1].map(|_| {
            texture(
                "plain",
                F::Bgra8Unorm,
                U::RENDER_ATTACHMENT | U::TEXTURE_BINDING,
            )
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("look"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("read back"),
            size: BYTES as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let (frame_view, overlay_view) = (view(&frame), view(&overlay));
        let binds = [0, 1].map(|last| {
            let last_view = plains.get(last).map(view);
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("lumière"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&frame_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(
                            last_view.as_ref().unwrap_or(&frame_view),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&overlay_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: uniforms.as_entire_binding(),
                    },
                ],
            })
        });
        tracing::info!(adapter = name, "the frames are made on the GPU");
        Self {
            name,
            device,
            queue,
            pipeline,
            frame,
            overlay,
            picture,
            plains,
            binds,
            uniforms,
            read,
            turn: 0,
            first: true,
            overlay_shown: None,
        }
    }

    /// The adapter's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    fn whole(&self) -> wgpu::Extent3d {
        wgpu::Extent3d {
            width: W as u32,
            height: H as u32,
            depth_or_array_layers: 1,
        }
    }

    fn write(&self, texture: &wgpu::Texture, pixels: &[u8]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(W as u32 * 4),
                rows_per_image: Some(H as u32),
            },
            self.whole(),
        );
    }

    /// RGBA `frame` (`W` × `H`) made into `out`, B, G, R, x: whether it
    /// was (else the GPU failed: the caller makes it on the CPU).
    pub fn make(&mut self, frame: &[u8], making: &Making<'_>, out: &mut [u8]) -> bool {
        let (Some(frame), Some(out)) = (frame.get(..BYTES), out.get_mut(..BYTES)) else {
            return false;
        };
        self.write(&self.frame, frame);
        if making.overlay != self.overlay_shown.as_ref() {
            if let Some(overlay) = making.overlay {
                let mut whole = vec![0u8; BYTES];
                if let Some(rows) = whole.get_mut(overlay.top * W * 4..) {
                    for (to, from) in rows.iter_mut().zip(&overlay.pixels) {
                        *to = *from;
                    }
                }
                self.write(&self.overlay, &whole);
            }
            self.overlay_shown = making.overlay.cloned();
        }
        if !making.lighting {
            self.first = true;
        }
        let c = making.correction;
        let look: [f32; 8] = [
            c.gain[0],
            c.gain[1],
            c.gain[2],
            c.exposure,
            if making.lighting { 1.0 } else { 0.0 },
            if self.first { 1.0 } else { 0.0 },
            0.0,
            if making.overlay.is_some() { 1.0 } else { 0.0 },
        ];
        let bytes: Vec<u8> = look.iter().flat_map(|v| v.to_ne_bytes()).collect();
        self.queue.write_buffer(&self.uniforms, 0, &bytes);
        // Write the plain texture the last frame did not; read that one.
        let (write, read) = (1 - self.turn, self.turn);
        let (Some(plain), Some(bind)) = (self.plains.get(write), self.binds.get(read)) else {
            return false;
        };
        let picture_view = self
            .picture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let plain_view = plain.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let attachment = |view| {
                Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumière"),
                color_attachments: &[attachment(&picture_view), attachment(&plain_view)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.picture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(W as u32 * 4),
                    rows_per_image: Some(H as u32),
                },
            },
            self.whole(),
        );
        self.queue.submit([encoder.finish()]);
        let slice = self.read.slice(..);
        let mapped = std::sync::Arc::new(std::sync::Mutex::new(None));
        let told = mapped.clone();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            if let Ok(mut told) = told.lock() {
                *told = Some(result.is_ok());
            }
        });
        if let Err(err) = self.device.poll(wgpu::PollType::wait_indefinitely()) {
            tracing::warn!(?err, "the GPU's frame is not read");
            return false;
        }
        let ok = mapped.lock().ok().and_then(|m| *m) == Some(true);
        if ok {
            out.copy_from_slice(&slice.get_mapped_range());
            self.read.unmap();
            self.turn = write;
            self.first = !making.lighting;
        }
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::look::{Correction, Cpu, Overlay};

    /// A frame of gradients, `shift` added.
    fn frame(shift: u8) -> Vec<u8> {
        let mut frame = Vec::with_capacity(BYTES);
        for y in 0..H {
            for x in 0..W {
                let r = (x * 255 / W) as u8;
                let g = (y * 255 / H) as u8;
                let b = ((x + y) % 97) as u8;
                frame.extend_from_slice(&[r.saturating_add(shift), g, b, 255]);
            }
        }
        frame
    }

    /// Lumière, its denoise and the banner on the GPU make what the CPU
    /// makes, within a level. On lavapipe (Mesa's Vulkan on the CPU), never
    /// a GPU: the test runs itself again with that driver alone shown to
    /// Vulkan's loader, which then opens no render node.
    #[test]
    fn the_gpu_makes_what_the_cpu_makes() {
        const CHILD: &str = "HOCUS_CAMERA_LAVAPIPE";
        if std::env::var_os(CHILD).is_none() {
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
                .args(["--exact", "gpu::tests::the_gpu_makes_what_the_cpu_makes"])
                .env(CHILD, "1")
                .env("VK_DRIVER_FILES", &lvp)
                .env("VK_ICD_FILENAMES", &lvp)
                .env("WGPU_BACKEND", "vulkan")
                .status()
                .expect("the test again");
            assert!(status.success());
            return;
        }
        let mut gpu = Gpu::new(Wanted::Any).expect("lavapipe");
        let mut cpu = Cpu::default();
        let overlay = Overlay {
            top: H - 100,
            pixels: (0..W * 100)
                .flat_map(|i| {
                    let a = (i % 256) as u8;
                    [a / 2, a / 3, a / 4, a]
                })
                .collect(),
        };
        let correction = Correction {
            gain: [1.1, 0.95, 0.8],
            exposure: 1.7,
        };
        for (shift, banner) in [(0, false), (3, true), (40, true)] {
            let making = Making {
                lighting: true,
                correction,
                overlay: banner.then_some(&overlay),
            };
            let input = frame(shift);
            let (mut on_gpu, mut on_cpu) = (vec![0; BYTES], vec![0; BYTES]);
            assert!(gpu.make(&input, &making, &mut on_gpu));
            cpu.make(&input, &making, &mut on_cpu);
            let worst = on_gpu
                .iter()
                .zip(&on_cpu)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            assert!(worst <= 2, "shift {shift}: {worst} levels apart");
        }
    }
}
