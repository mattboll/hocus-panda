# iced: a way to make the CPU renderer draw whole frames

For [iced-rs/iced](https://github.com/iced-rs/iced), crate `iced_tiny_skia`
(`tiny_skia/` in the repository). Written against 0.14.1 (commit
`0ecf6066`). Not sent yet. iced asks that changes be discussed before a
pull request: this is the text of that discussion; the patch follows it.

---

**Title:** tiny-skia: stale pixels outside the damaged regions; an option to draw whole frames?

### What happens

With the `tiny-skia` renderer, `window::compositor::present` redraws only
the regions `damage::diff` finds changed between this frame's layers and
those of the buffer's last frame. The regions come from the primitives'
bounds, and some primitives draw past their bounds, or change without
their bounds saying so. What we met in an application of about twenty
pages (iced 0.14, Wayland, softbuffer):

- a container's **shadow**: when the container goes (a toast), its shadow
  outside its bounds stays on screen;
- **anti-aliased strokes** and rounded borders: a one-pixel fringe stays
  after the widget moves;
- a `canvas`'s **text** inside a `scrollable`: after a scroll, glyphs of
  the previous position stay;
- a **gradient** background drawn across several damaged regions: seams
  between the pieces;
- an **overlay** closed (a `pick_list`'s menu): the menu stays until
  something else damages its place.

The wgpu renderer draws every frame whole and shows none of these.

### What we do today

We carry an 18-line patch: a process-wide switch that makes `present` skip
the diff and damage the whole viewport.

```rust
// tiny_skia/src/window/compositor.rs
static WHOLE_FRAMES: AtomicBool = AtomicBool::new(false);

/// Makes every frame presented from now on draw the whole window, instead
/// of the regions found changed since the buffer's last frame.
pub fn draw_whole_frames(whole: bool) {
    WHOLE_FRAMES.store(whole, Ordering::Relaxed);
}

// in `present`
let damage = last_layers
    .filter(|_| !WHOLE_FRAMES.load(Ordering::Relaxed))
    .and_then(|last_layers| { /* damage::diff, as today */ })
    .unwrap_or_else(|| vec![Rectangle::with_size(viewport.logical_size())]);
```

It costs what one expects: about 40 ms for a 1100 × 760 window at scale 2
on a 2023 laptop's CPU. For an application that draws only when something
changes, that is one whole frame per change, and after a scroll or an
overlay the damaged regions already covered most of the window.

### What we would like

Either, whichever fits iced better:

1. **The damage fixed**, so that a primitive's damage covers what it
   draws (shadows' blur and offset, strokes' half width, a layer's
   translation). We can send the cases above as small reproductions, one
   per issue, if that is the way.
2. **An option to turn partial redraws off**, as a setting rather than a
   global: a field of `iced_tiny_skia::Settings` (and of
   `iced::Settings`/`graphics::Settings` if it should reach there), say
   `partial_redraws: bool`, default `true`, read by `present`. We would
   write that pull request.

A static switch as in our patch is the smallest change, but not what we
would propose to merge.

### Environment

iced 0.14.0, iced_tiny_skia 0.14.1, Linux, Wayland (layer-shell and
xdg-shell windows), softbuffer; Rust 1.99.
