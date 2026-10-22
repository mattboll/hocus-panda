# Rendering stack prototypes

The measurements behind [decision 0009](../../docs/decisions/0009-ui-rendering-stack.md):
the same widget column (320 logical pixels wide, seven widgets, French text in
Manrope and Fraunces, an agent card that changes every frame) drawn by three
candidates.

| Binary | Shapes | Text |
|---|---|---|
| `tiny` | tiny-skia | cosmic-text (swash), glyphs blended by hand |
| `vello_cpu` | vello_cpu (sparse strips, CPU) | parley + vello_cpu glyphs |
| `vello_gpu` | vello (wgpu compute) | parley + vello glyphs |

Kept out of the workspace so the product never builds or depends on them.

```sh
./fetch-fonts.sh                      # Manrope and Fraunces (OFL) into fonts/
cargo build --release --bins
./target/release/tiny 2               # scale 2; also 1
./target/release/vello_cpu 2
./target/release/vello_gpu 2          # needs a GPU (Vulkan, Metal or DX12)
```

Each prints, after warm-up, the median, p95 and max time of:

- **full column**: every widget into one buffer, the agent card's text laid
  out again each frame (other text is laid out once and cached);
- **one widget**: the agent card alone into its own buffer, which is what a
  widget whose content changed costs;
- **shapes only**: the full column without text;
- for vello on the GPU, the time until the GPU is done, the CPU encoding
  alone, and reading the result back to memory;
- init time and peak resident memory.

Images land in `out/` for comparing text.
