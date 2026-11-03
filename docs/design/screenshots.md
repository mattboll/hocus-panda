# Screenshots — specification (5.8)

Status: approved 2026-11-02 (issue #398), variant A of the 5.8 mockup:
choose, then capture, with working handles. Built since 2026-11-03: see
[as built](#as-built). Sizes are logical pixels, the mockup's; like the
launcher's, they are drawn × 1.5 (`hocus_link::LAUNCHER_ZOOM`), as the
mockup showed them on the maintainer's 2560 × 1600 screen at scale 1.
What comes after the capture (a preview, annotating) is 5.9.

## 1. Opening

- `Print` opens the capture on every screen: the screens darken
  (`rgba(6,8,14,0.55)` outside the chosen part), and a toolbar sits at
  the bottom centre of the screen under the pointer.
- **The toolbar**, the popovers' glass, radius 14:
  - the modes **Zone / Fenêtre / Écran / Tous les écrans** (Area / Window
    / Screen / All screens), the chosen one with the green tint and ring;
  - a separator;
  - the delay ("Tout de suite" / "Now", 3 s, 10 s; a click cycles);
  - "include the pointer" (off by default);
  - **Capturer** (Capture), the green button.
- `Escape` cancels. `Enter` or Capturer captures.

## 2. Area

- The last area is kept (in the state directory) and shown when the
  capture opens. A drag outside it draws a new one.
- The area has a 1.5 px white border, with its size in real pixels
  ("600 × 380", Martian Mono 10.5 px) above it.
- **Handles**: four corner dots of 8 px, plus the four edges' middles.
  Dragging a corner resizes both of its sides; dragging an edge resizes
  that side; dragging inside the area moves it. The pointer changes to the
  matching resize or move cursor. The area is held inside its screen. The
  arrow keys move it by 1 px; `Shift` plus the arrows resize it.

## 3. The other modes

- **Fenêtre**: the window under the pointer gets a green ring, and a
  click captures it, with its shadow left out.
- **Écran**: the screen under the pointer.
- **Tous les écrans**: one image of all the screens, as they are laid out.

## 4. Delay

A countdown (110 px round, Martian Mono 48 px) in the screen's centre. The
darkening and the toolbar leave during the countdown, so they are not in
the image.

## 5. Capture

- A white flash over what was taken (opacity 0.85 to 0 in 260 ms; with
  reduced motion, none).
- The image goes to the clipboard (`image/png`) and to
  `$XDG_PICTURES_DIR/Captures d'écran/` (`Screenshots/` in English),
  named "Capture du 2026-11-03 à 14-32-05.png" / "Screenshot from
  2026-11-03 14-32-05.png".
- It is taken at the screens' real resolution.
- The toast is a placeholder until 5.9: "… copiée · enregistrée dans
  Images/Captures d'écran" / "… copied · saved to Pictures/Screenshots".

## 6. Shortcuts

`Print`: the capture. `Shift+Print`: the whole screen under the pointer,
at once. `Alt+Print`: the focused window, at once. All three are in
`[shortcuts]` (`capture`, `capture-screen`, `capture-window`).

## 7. Rules

- Encoding and writing happen off the loop.
- Protected content (windows that ask for it) shows black.
- Lock screens are never captured.

## As built

### 5.8, choose then capture (2026-11-03)

- **Who draws what.** The compositor owns the capture and draws all of
  it but the toolbar: the darkening (four solid rectangles around what is
  chosen, the whole screen where nothing is), the area's border (four
  solid sides, 2 px at scale 1), its eight handles and its size, the
  window's green ring, the countdown and the flash
  (`crates/hocus-compositor/src/capture/`). An area dragged is drawn in
  the frame that follows the pointer's motion, with no line to the shell
  and no pixels drawn again but its size's few glyphs (Martian Mono, the
  shell's own file, read by the compositor with fontdue): dragging never
  waits for another process. The shell draws only the toolbar, a layer
  surface of its own (`hocus-capture-toolbar`, overlay layer, anchored to
  the bottom 33 px up, taking the pointer, never the keyboard): the
  compositor says what it shows (`capture OUTPUT SCALE MODE DELAY
  pointer|nopointer`) and closes it (`capture-end`), the shell says what
  is clicked (`capture mode MODE`, `capture delay`, `capture pointer`,
  `capture shoot`). The compositor blurs behind it as behind the
  popovers, draws its shadow (`0 14px 40px rgba(0,0,0,0.45)`, zoomed) and
  fades it in over 180 ms from its first frame (the mockup's `rise`).
- **Input while it is open.** The compositor keeps the keys (the
  modifiers go on, as during Alt+Tab): `Escape` cancels, `Enter` captures,
  the arrows move the area by a pixel, `Shift` and the arrows move its
  right or bottom edge; other keys do nothing, the desktop's shortcuts
  included. The pointer is the capture's but over the toolbar: no client
  sees it, and the cursor says what a press does (`crosshair` outside the
  area, `move` inside, the resize cursors within 9 px of an edge or a
  handle; a hand over a window in window mode). A press outside the area
  draws a new one from there (one smaller than 6 px leaves the area there
  was); a release in window mode captures the window under it. The area
  is held inside its screen, whichever screen the drag began on.
- **The area kept** is written to `$XDG_STATE_HOME/hocuspanda/capture-area`
  (`OUTPUT X Y W H`, logical pixels) on a thread as the capture closes,
  when it changed, and read as the compositor starts. On a screen that is
  not there any more, or the first time: a third of the screen under the
  pointer, in its middle. The mode, the delay and the pointer are kept
  for the session.
- **Taking it.** At its next turn the backend draws each screen needed
  again into a buffer of its own, at the screen's resolution and upright,
  as the screen shows it but without the capture's drawing, its toolbar
  and the toasts, the cursor only with "include the pointer", and reads
  it back (as the headless backend's screenshots are read). A window is
  drawn alone, its surfaces and its popups without its frame or its
  shadow, its corners rounded off (transparent). Then a thread cuts the
  area out, or lays all the screens out at the largest of their scales (a
  screen of a smaller one stretched to it, transparent where none is),
  encodes the PNG (`image`, PNG only) and writes it, never over another
  file (" (2)" for two in the same second); back on the loop the
  clipboard offers it as `image/png` (to Wayland and X11 clients), and
  the shell shows the launcher's toast on the capture's screen. A
  password manager's window (Bitwarden, KeePassXC, 1Password: the
  clipboard's history's list) is black in every capture. On a virtual
  terminal in the background nothing is captured.
- **Where it goes**: `$XDG_PICTURES_DIR`, else `XDG_PICTURES_DIR` in
  `$XDG_CONFIG_HOME/user-dirs.dirs`, else `~/Pictures`; in it
  `Captures d'écran` or `Screenshots`, by the messages' locale
  (`LC_ALL`, `LC_MESSAGES`, `LANG`), the file named in local time.
- **The delay** counts down with a timer of the loop each second: the
  toolbar and the darkening go at once; the disc and its number show in
  the middle of the screen taken (the pointer's for all of them), the
  number coming in over 300 ms from 1.25 times its size and 0.3 of its
  opacity (the mockup's `tick`; at once with reduced motion). Input goes
  to the windows meanwhile, so that a menu can be opened for the image.
- **The toast**: "Zone de 600 × 380 copiée · enregistrée dans
  Images/Captures d'écran", "Fenêtre copiée · …", "Écran copié · enregistré
  dans …", "Tous les écrans copiés · enregistrés dans …"; "Area of 600 ×
  380 copied · saved to Pictures/Screenshots", "Window copied · …". The
  folder is the pictures' folder's own name and the captures'. Not saved
  (the folder could not be written): "… copiée" alone.
- **Choices made here** (to confirm on the machine): the toolbar and the
  countdown are centred on the whole screen, as the launcher's toast is
  (the bar reserves no space), where the mockup centred them in the space
  left of the bar; the delay's button is as wide as its widest label, so
  that the toolbar keeps its size; the edges resize all along their length,
  not only at their middles' handles; the size shows over the area's top
  left corner, inside it when there is no room above; in window mode it
  shows the window's size, and the click captures on its release; a click
  in screen mode does not capture (`Enter` or Capturer does); the handles
  hide while a new area is drawn; the countdown cannot be cancelled; the
  French toast agrees with what was taken ("Écran copié").
- **Measured headless** (`crates/hocus-compositor/tests/screenshots.rs`),
  pixman, a flat background, the pictures' and state folders the test's
  own: an area drawn and taken (darkened outside, not inside; the image
  600 × 380); moved from inside and held in its screen, nudged and resized
  by the arrows; resized by a corner and by an edge, the cursor saying
  which; kept for the next capture and the next session; window mode
  taking the window under the pointer alone, at its size, its corners
  transparent; the screen under the pointer and all the screens (4480 ×
  1440, transparent under the smaller one); a 3 s delay counting down
  with nothing darkened and the window's own colour in the image; the PNG
  in `Screenshots`, named for when, and the same bytes on the clipboard;
  `Shift+Print` and `Alt+Print` at once; `Escape` cancelling, nothing
  saved. With the real shell: the toolbar at the bottom centre, its modes
  and its green button clicked, the toast's words and the French folder
  and name. Unit tests for the area's geometry, the image cut and laid
  out, the premultiplied pixels, the names, `user-dirs.dirs`, the area
  file, the keys, the lines and the toast's words. On the Intel GPU, the
  screenshots of the pull request.
- **Not built**: a lock screen to keep out (the session locks with the
  display manager's until one is built; then nothing is captured while
  it shows); a protocol for windows to ask for protection (none is
  common to Wayland compositors: the password managers stand for it); the
  pointer in a window's capture; the capture in the clipboard's history;
  the screencopy protocols for other programs.
