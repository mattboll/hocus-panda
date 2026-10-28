# softbuffer 0.4.8, patched

A copy of [softbuffer](https://github.com/rust-windowing/softbuffer) 0.4.8
(MIT OR Apache-2.0), used by the shell through `[patch.crates-io]` in the
workspace's `Cargo.toml`. Its only change: the Wayland backend creates its
`wl_shm` buffers as `Argb8888` instead of `Xrgb8888`
(`src/backends/wayland/buffer.rs`), so that what iced's CPU renderer
(tiny-skia) leaves transparent shows the desktop instead of black
(docs/design/tech-choices.md, "The bar's renderer (3.0)"). tiny-skia already
writes premultiplied alpha into that byte. The benches and dev-dependencies
are left out. Drop this copy once upstream offers transparent buffers.
