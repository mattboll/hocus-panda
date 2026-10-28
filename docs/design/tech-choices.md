# Technical choices — v2 (draft for review)

Status: approved 2026-10-27; spike done 2026-10-28 ([report](spike-iced.md)); the bar's renderer measured on a GPU 2026-10-28. It answers three questions: how the shell
is drawn, where it runs, and what we keep from the first build.

## Why these questions

The first build drew its whole shell with its own toolkit (`hocus-ui`, on
vello_cpu and parley), inside the compositor's process. Most of the
defects the user hit were the toolkit's or that choice's:

- menus that close when the pointer moves onto them;
- a text field whose caret sits in the placeholder's first letter;
- hover and focus that get lost on rebuilds;
- no way to restart the interface without losing every window, so each
  try meant logging out.

The compositor itself (smithay, DRM/KMS, input, XWayland, multi-GPU) was
not what failed.

## Decision 1 — the shell is a separate process

**Proposal:** the compositor stays one process (window management,
rendering, input, protocols, effects on windows). The shell (bar,
popovers, launcher, notifications, lock screen's UI, settings) becomes one
client process that draws layer-shell surfaces.

| For | Against |
|---|---|
| Restarting the shell keeps every window: the "hot reload in a real GDM session" the user asked for | A protocol between the two to design and keep stable |
| A crash in the shell no longer ends the session | Effects that need both (blur behind the bar, the streak across windows) need compositor support |
| The shell can use a mainstream toolkit and its widgets | Two processes to start, supervise and test |
| The same split as the field's working desktops (COSMIC's panel and applets, layer-shell bars) | Changes decision "one process" in `docs/architecture.md` |

What crosses the boundary:

- **Wayland:** wlr-layer-shell (bar, popovers, overlays such as the panda
  peek and the streak); a background effect request for blur behind the
  bar; foreign-toplevel for the windows list and focus.
- **The control socket** (already exists): desktops, outputs, events,
  "focus this window", agents' states relayed from herdr.
- **D-Bus** for system services, read by the shell directly
  (NetworkManager, BlueZ, UPower, PipeWire through its own API, KDE
  Connect).

The compositor supervises the shell: it starts it, restarts it on a crash
(with a back-off), and a `hocus shell restart` command restarts it on
purpose. While the shell is down, the windows keep their places and the
bar's space stays reserved, so nothing jumps.

## Decision 2 — the shell's toolkit: iced

| Toolkit | Licence | Fit |
|---|---|---|
| **iced 0.14** + iced_layershell | MIT / MIT | Animation API (new in 0.14), input methods, mature widgets (text input, scrollables), styles we fully control; COSMIC builds a whole desktop on it. Accessibility (AccessKit) to check in the spike |
| egui | MIT / Apache-2.0 | Immediate mode: fast to write, harder to make a polished, animated, distinctive look; AccessKit support |
| Slint | GPL-3.0 or a royalty-free proprietary licence | Strong animations, accessibility and input methods, but not a permissive licence: ruled out by decision 0002 |
| Freya, Xilem | MIT / Apache-2.0 | Promising, still young: too much risk for the base |
| `hocus-ui` (ours) | MIT | The source of most UI defects; rewriting it well is a project of its own |

**Proposal:** iced 0.14 with iced_layershell for the shell, our own theme
(the spec's tokens), our own few widgets where the design needs them (the
halo, the gauges, the pandas). The pandas stay vector drawings, rendered by
iced's canvas or as SVG.

## Decision 3 — what we keep

- **Kept, as a base:** smithay (MIT) and the compositor's approach.
- **Rewritten from a clean crate layout:** everything else, slice by
  slice. Code from the first build is reread and ported only when a slice
  needs it and only if it holds up: likely the DRM/KMS and multi-GPU
  backend, input, XWayland, session and lock handling. Each port is named
  in its pull request.
- **Dropped:** `hocus-ui`, the in-process shell, and the features out of
  the v1 scope (tiling, agents driving the desktop, the natural-language
  command, the Matrix theme, plugins, FreeBSD).
- The first build stays reachable under a git tag (`v0-first-build`). To
  decide with the user: the new work either replaces `main` at once, or
  grows on a `v2` branch merged when the first slice is accepted.

## The development loop

- The shell restarts in a few seconds after a build
  (`cargo watch -x 'run -p hocus-shell'` against the running session), in
  the user's real GDM session, keeping every window.
- The compositor is installed as a `.deb` for the real session and run
  nested for compositor work, with a different modifier so the host's
  shortcuts don't get in the way.
- Builds: 8 jobs, mold, sccache (in place since 2026-10-27).
- Acceptance is on the user's machine, against the approved mockups and
  this spec, not against references we recorded ourselves. Headless
  scenarios stay, to keep what works from breaking.

## The spike (to validate before deciding)

Two to three days, with a go/no-go per point.

| # | What | Pass when |
|---|---|---|
| 1 | The bar's full form, partly (clock with seconds, agents tiles with the pandas, the meeting card, one gauge), as an iced_layershell client | It looks like the canvas board side by side at 100 %; it draws once a second at rest |
| 2 | A popover that stays open while the pointer moves toward it, and closes 300 ms after it left | It never closes on the way; Esc and focus work |
| 3 | A text field (add a task) | Caret, selection, placeholder, input method: right |
| 4 | The halo's wave and glow, and the form switch | 600 ms and 240 ms, smooth at the screen's refresh rate, no frame over budget |
| 5 | Restart the shell while windows are open | Windows stay; the bar is back in under 2 s |
| 6 | Accessibility: a screen reader reads the bar | Names and states read; if iced lacks it, the cost of adding it is known |
| 7 | Blur behind the bar from the compositor (smithay) | Works on the user's GPU and in the VM |

Where it runs: the bar as a client of the first build's compositor (which
has layer-shell) in the VM and, if it holds, in the user's session.

If iced fails points 2, 3 or 6 badly, the fallback is egui with our own
styling, measured on the same points.

## Outcome of the spike (2026-10-28)

The [spike's report](spike-iced.md) has the measurements and screenshots.
Decisions taken with the maintainer:

- **iced is kept** for the shell, on three conditions: the animations'
  smoothness is measured on a GPU before the bar's slice (the first
  build's compositor offers no linux-dmabuf, and the maintainer's session
  has no layer-shell, so every spike measurement ran in software; done
  since, see [the bar's renderer](#the-bars-renderer-30));
  iced_layershell is pinned and watched (a winit-core beta broke it once);
  the accessibility work below is planned.
- **Accessibility**: iced 0.14 has no AccessKit. Our own tree with
  `accesskit_unix` (about 3 to 5 days for the bar and its popovers) is a
  slice of its own, done before the v1 release.
- **`cargo deny`**: `ttf-parser` (unmaintained, RUSTSEC-2026-0192), the core
  of iced's text, is accepted with a note in `deny.toml`, reviewed on every
  iced update.
- Traps to keep in mind (each made the bar look clearly worse until fixed):
  iced's `web-colors` feature is required to blend like the mockups;
  images are pre-scaled per output scale; variable fonts are not matched,
  static instances are.
- The compositor's first slice must offer linux-dmabuf and wlr-layer-shell,
  keep the bar's reserved space while the shell restarts (a maximised
  window jumped in the spike), and give the keyboard back to the bar when
  one of its popovers closes.

## The bar's renderer (3.0)

Measured on 2026-10-28 (issue #357) with the spike's bar against v2's
compositor on an Intel laptop GPU; the numbers are in the
[report](spike-iced.md#measured-on-a-gpu-30). On the GPU, the wave and the
form switch keep the output's pace with no frame over budget, at 0.7–0.9 ms
of the bar's CPU per frame and 0.13–0.17 % at rest. The 2 cores of the
spike were lavapipe's, and lavapipe still costs that today.

**Recommendation: wgpu, with tiny-skia as the software fallback**, and four
things the bar's slice must do:

1. **Draw on the compositor's GPU, and wake no other.**
   - iced asks wgpu for the high-performance GPU, so on a laptop with two
     GPUs the bar lands on the discrete one and keeps it awake.
   - Creating a Vulkan instance loads every installed driver, which wakes a
     sleeping discrete GPU (2.2 s before the first draw) and costs about
     100 MB.
   - The shell is started by the compositor, which knows its render node. It
     starts the shell with `WGPU_POWER_PREF=low` and only that GPU's drivers
     (`VK_LOADER_DRIVERS_SELECT` or `VK_DRIVER_FILES`, and the EGL vendor for
     wgpu's GL backend).
   - Measured: first draw in 59 ms, 118 MB.
2. **An 8-bit-alpha surface.**
   - With `web-colors`, iced takes the first non-sRGB format, which is
     `Rgb10a2Unorm` (2-bit alpha) on Mesa.
   - A small patch to `iced_wgpu`'s format choice makes it prefer
     `Rgba8Unorm`/`Bgra8Unorm`. Offer it upstream, and carry it meanwhile.
3. **Fall back to tiny-skia explicitly.**
   - iced only falls back when wgpu fails, and with lavapipe installed wgpu
     never fails: the bar would then spend 1.2–2.5 cores per wave and still
     miss frames.
   - The shell checks the adapter wgpu offers. When it is a CPU (lavapipe,
     llvmpipe), or when there is no GPU, it starts with
     `ICED_BACKEND=tiny-skia`.
   - In that mode the bar plays no wave and shows the glow without its
     fade, as reduced motion does: tiny-skia does not hold the pace either
     (31 ms between frames). It costs 17 MB and starts in 5–15 ms.
4. **Fix tiny-skia's transparency before relying on it.**
   - softbuffer 0.4.8 presents `Xrgb8888` buffers on Wayland, so everything
     transparent turns black. Presenting `Argb8888` is a two-line change,
     carried as a patched softbuffer until upstream offers it.
   - With it, tiny-skia's outer shadow shows much stronger than wgpu's. The
     glow then needs drawing in our own canvas (the inner glow already is),
     or a fix in `iced_tiny_skia`.
   - Until then, the software mode draws the bar opaque with no outer
     glow: the "reduce transparency" look the spec already defines.
