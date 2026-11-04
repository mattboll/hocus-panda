# Screenshots and screen recording — specification (5.8, 5.9, 5.10)

Status: approved 2026-11-02 (issue #398), variant A of the 5.8 mockup:
choose, then capture, with working handles. Built since 2026-11-03: see
[as built](#as-built). Sizes are logical pixels, the mockup's; like the
launcher's, they are drawn × 1.5 (`hocus_link::LAUNCHER_ZOOM`), as the
mockup showed them on the maintainer's 2560 × 1600 screen at scale 1.
What comes after the capture, its thumbnail and its annotator, is 5.9
([below](#59-the-thumbnail-and-the-annotator)); recording the screen from
the same toolbar is 5.10 ([below](#510-screen-recording)).

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
  *Since 5.9 the thumbnail replaces it; it is left for a capture that
  could not be saved.*

## 6. Shortcuts

`Print`: the capture. `Shift+Print`: the whole screen under the pointer,
at once. `Alt+Print`: the focused window, at once. All three are in
`[shortcuts]` (`capture`, `capture-screen`, `capture-window`).

## 7. Rules

- Encoding and writing happen off the loop.
- Protected content (windows that ask for it) shows black.
- Lock screens are never captured.

## As built

Drawing on the screen (issue #564, [ink.md](ink.md)): the strokes, the
arrows and the laser are in the scene a capture (5.8) and a recording's
frames (5.10) of a screen are drawn from, as on the screen; the capture's
own darkening, border and toolbar, and the recorded area's frame, are
over them and never in the picture.

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
- **The toast** (since 5.9, only for a capture not saved: the thumbnail
  shows instead): "Zone de 600 × 380 copiée · enregistrée dans
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
  screencopy protocols for other programs. (The pointer in a window's
  capture and the capture in the clipboard's history came with 5.8b,
  below.)

### 5.8b, the history and the pointer (2026-11-03)

Decided by the maintainer after 5.8 (issue #528).

- **Captures in the clipboard's history**: every capture (an area, a
  window, a screen, all of them, `Shift+Print` and `Alt+Print`) is an
  image of the history ([bar.md](bar.md), 5.6), as an image copied from
  an application is: its thumbnail in its tile, "Image · 640 × 360", and
  for its source "Capture d'écran" / "Screenshot" (the search finds it by
  that word too), first. The clipboard offering a capture is the
  compositor's own selection, which the history never reads back; so the
  capture's thread, once the PNG is made, also makes the history's line
  (`capture-copied CAPTURE png DATA`, the data in base64, off the loop as
  a copy's line is), and the loop tells it to the shell after offering
  the clipboard. Its application, kept with the item on disk, is
  `hocuspanda.capture` (`hocus_link::CAPTURE_APP`). A capture larger than
  the history keeps (8 MB of PNG) is copied and saved, not listed.
- **Replacing it (for 5.9)**: each capture has a number, the session's
  count of captures drawn (`Capture::numbered`, carried by the capture's
  thread). The same number told again with another PNG (the annotated
  image) replaces the item that capture made: the old item and its files
  go, the new one comes first, pinned if the old one was; if the user
  removed it meanwhile, the new one is only added. The compositor's way
  is `clipboard::capture_line(number, &png)` on the thread that made the
  PNG, then `State::capture_in_history(line)` on the loop; the shell's,
  `clipboard::captured(number, clip)`. The shell keeps which item each
  number made for the session only.
- **The pointer in a window's capture**: with "include the pointer" on,
  a window's capture shows the pointer when it is over that window (the
  window under it, as window mode's hover finds it), drawn where it is on
  the window, in front of its surfaces and popups; elsewhere, or with the
  option off, none. `Alt+Print`, like `Shift+Print`, never includes it.
- **Measured headless**: a window's capture with the option on has the
  pointer under its tip and the window's colour elsewhere, and with it
  off none (`tests/screenshots.rs`, pixman); with the real shell, a
  capture is the history's first row, "[image] «Image · 640 × 360»
  Capture d'écran · maintenant", above what was copied before, and
  `Enter` gives the window it as the clipboard (`Ctrl+V`), its bytes the
  saved PNG's, though a client had taken the clipboard since
  (`tests/clipboard.rs`). Unit tests for the line, the source's words and
  search, and an item replaced keeping its pin. On the Intel GPU, the
  screenshot of the pull request.

## 5.9: the thumbnail and the annotator

### The approved spec

Approved by the maintainer on 2026-11-03 (issue #399), variant **A** of
the 5.9 mockup: a thumbnail at the bottom, beside the bar. It follows
5.8's capture and replaces its placeholder toast.

**The thumbnail**

- **Where**: bottom right of the captured screen, against the bar (8 px
  from the bar and from the bottom).
- **Look** (mockup units × 1.5, the launcher's zoom): 220 wide, padding
  8, radius 14, the popovers' glass. It shows the image scaled to fit,
  128 tall, radius 10. Under it: "Copiée · enregistrée" / "Copied ·
  saved", an "Annoter" / "Annotate" button (green tint) and ✕.
- **Motion**: it slides up 14 px and fades in over 240 ms.
- **Time**: it stays 6 s, with a 2 px countdown line at its bottom;
  hovering pauses the countdown.
- **Actions**: a click on the image or on "Annoter" opens the editor; ✕
  deletes the capture: the file goes, and the clipboard empties if it
  still holds this image; dragging the thumbnail into an application
  drops the PNG file there (a `text/uri-list` drag, plus `image/png`).
- **When its time is up**, it goes, and the capture goes to the
  notifications' history (5.4) as an item with its image and actions:
  "Annoter" / "Annotate", "Ouvrir le dossier" / "Open folder", "Supprimer"
  / "Delete".

**The editor**

- **Where**: a panel centred on the screen over a darkened, blurred
  desktop, holding the image fitted at most 1.5× the launcher's zoom of
  the mockup (672 × 420 in mockup units).
- **Tools**: **Flèche** / **Arrow** (drag from tail to head, 4 px line
  with a 16 px head); **Cadre** / **Box** (drag; 3 px border, radius 4);
  **Texte** / **Text** (click, type, `Enter`; Manrope bold 18 px with a
  dark outline); **Flou** / **Blur** (drag a rectangle that blurs what is
  under it, strongly enough that text cannot be read; the saved image is
  blurred too, not just covered). The selected tool has the green tint.
- **Colours**: red `#F2545B` (the default), amber, green, blue, white.
- **Undo**: `Ctrl+Z` undoes and `Ctrl+Shift+Z` redoes, with the undo
  button in the toolbar. `Escape` while typing cancels the text.
- **Buttons**: **"Copier et fermer"** / **"Copy and close"** writes the
  annotated image over the saved file and the clipboard, then closes;
  **"Enregistrer sous…"** / **"Save as…"**: the desktop portal's file
  chooser; **"Supprimer"** / **"Delete"**: as ✕. Closing with no
  annotation changes nothing.
- **Sizes**: the drawing is done at the image's real resolution; marks
  scale with it.
- **Original**: the original is kept until the editor closes, so undoing
  back to nothing restores it.

All encoding and writing happen off the loop.

### As built (5.9, 2026-11-03)

- **Who does what.** The shell draws the thumbnail and the editor, each a
  layer surface of its own on the overlay layer
  (`hocus-capture-thumb`, taking the pointer, never the keyboard;
  `hocus-capture-editor`, all of the screen, holding the keyboard). The
  compositor moves them from their first frame (the thumbnail up 14 px
  and in over 240 ms, `cubic-bezier(.2, .8, .2, 1)`; the editor in over
  180 ms; at once with reduced motion), draws the thumbnail's shadow (the
  mockup's `0 16px 44px rgba(0,0,0,0.5)`, zoomed; on the CPU it cuts its
  corners) and blurs the whole screen behind the editor (the mockup's
  `blur(10px)`, zoomed); neither is ever in a capture. What is the
  compositor's stays its own: the clipboard, the file's deletion, the
  drag.
- **The lines.** `captured ID OUTPUT MODE WIDTH HEIGHT`, then the folder
  and the file after tabs (`-` for none): the id is the capture's number
  (5.8b's, `Capture::numbered`). The shell asks `capture forget ID` (✕,
  "Supprimer": the compositor deletes the file on a thread and empties
  the clipboard if the selection is still the bytes it offered for that
  capture, Wayland's and X11's), `capture annotated ID` (the shell wrote
  the annotated image over the file: a thread reads it back, the
  clipboard takes it, and the clipboard's history replaces the capture's
  item with it, 5.8b's `capture_line` and `capture_in_history`) and
  `capture drag ID`.
- **The thumbnail**: its card 330 × 267 (220 × 178 zoomed), its right 8 px
  from the bar (from the screen's edge on a screen without one), its
  bottom 8 px from the screen's. The image is read and made small on a
  thread of the shell's (contain-fitted in 306 × 192, never larger than a
  pixel a pixel), its corners cut at 15 px; "Copiée · enregistrée" 16.5 px
  `#A9B2C2`; "Annoter" 11.5 × 1.5 px bold on `rgba(108,194,135,0.22)`,
  radius 10.5; ✕ 39 px square, lit `rgba(255,255,255,0.10)` under the
  pointer, as "Annoter" is. The countdown: 3 px (2 zoomed),
  `rgba(108,194,135,0.7)`, 15 px from the sides, drawn again a pixel at a
  time and 30 times a second at most; paused, `rgba(255,255,255,0.35)`.
  The pointer anywhere on the card holds it; it goes on from where it
  was when the pointer leaves. A new capture while one shows sends that
  one to the history.
- **The drag**: a press on the image moved 6 px tells the compositor,
  which starts a drag of its own from the pointer's press (smithay's
  server-side drag): `text/uri-list` (`file://` and the path, its bytes
  escaped) and `image/png` (the file's bytes), written to the application
  on a thread. Nothing is drawn under the pointer while it drags (no drag
  icon).
- **To the history**: as its time is up, or as its editor closes but by
  "Supprimer", the capture is a notification of the shell's own in the
  history (an id past the server's), seen: "Capture d'écran" /
  "Screenshot", its file's name for title, "1300 × 800 ·
  Images/Captures d'écran" for body, its image cut square as its icon
  (28 px, its corners rounded as the letter tiles'), and its three
  actions; a click on it opens the editor too (no window is raised).
  "Annoter" and "Ouvrir le dossier" leave it there (`resident`);
  "Ouvrir le dossier" runs `xdg-open` on the folder (through the
  compositor's launch); "Supprimer" deletes it and takes it out. It is
  kept on disk with the history (its image found again from its file);
  read back after a restart, it has no actions, as any notification read
  back. Without a bus of its own (nested, headless), the history is not
  kept on disk, as in 5.4.
- **The editor's panel**: the mockup's sizes × 1.5 (less on a screen too
  small for it, 24 px from its edges at least): 1044 × 795, padding 18,
  radius 24, `rgba(22,27,42,0.97) → rgba(14,17,28,0.98)`, a 1 px rim, its
  shadow (`0 22px 60px rgba(0,0,0,0.5)`, zoomed) on a GPU; the screen
  darkened `rgba(6,8,14,0.6)` over the compositor's blur. The toolbar:
  the four tools (icon 25.5 px, label 18 px 600), a separator, the five
  colours (33 px, a 3 px ring `#EEF1F6` on the chosen one), undo on the
  right. The image's place 1008 × 630, radius 15, the image contain-fitted
  in it, never larger than a pixel a pixel. Under it the file's name and
  the image's size (17.25 px `#7C8698`), then "Supprimer" (`#F7A77A`),
  "Enregistrer sous…" and "Copier et fermer" (green).
- **The marks**: kept in the image's own pixels, each mark's sizes the
  mockup's × the editor's zoom × the image's pixels per logical pixel of
  its place, so that what shows is what is saved: an arrow 6 px wide with
  a 24 px head (the line running three quarters into it), a faint dark
  halo; a box 4.5 px, its outer corners 6; a text 27 px Manrope bold, its
  outline 2.25 px `rgba(0,0,0,0.9)`, its line's left 10.5 px right of the
  click; a blur three box blurs of 10.5 px (about a Gaussian of that
  deviation, reading around the region), with the mockup's faint
  `rgba(120,130,150,0.12)` tint. A drag under 12 px draws nothing. The
  marks are drawn in order: a blur blurs the marks before it. The image
  shown is drawn with its marks by the same code as the saved one (no 2D
  library: the shapes by their distance to each pixel, the text by
  fontdue), at the screen's pixels, again as marks come and go; the mark
  being dragged and the text being typed (in its dashed field, 330 × 45)
  are drawn over it as shapes, a blur being dragged from the image
  blurred once as the editor opens.
- **Keys**: while typing, `Enter` keeps the text, `Escape` drops it,
  `Backspace` takes a character back; a click elsewhere on the image, a
  tool or a button keeps it. Otherwise `Ctrl+Z`, `Ctrl+Shift+Z`, and
  `Escape` closes the editor without changing anything (the capture to
  the history).
- **"Copier et fermer"** with marks: a thread draws them on the original
  at its resolution, encodes the PNG and writes it over the file (a new
  file renamed over it), then tells the compositor (`capture annotated`).
  With no marks (none drawn, or all undone): nothing is written, the
  clipboard is left as it is.
- **"Enregistrer sous…"**: on its own thread, the portal
  (`org.freedesktop.portal.FileChooser.SaveFile`) on the session bus the
  compositor names (the notifications' bus: `--session-bus`, or the
  session's own on a virtual terminal), if `org.freedesktop.portal.Desktop`
  has an owner there or can be started; the bus and the call have 5 s,
  then the user as long as they like. The proposed name is "Capture du …
  (annotée).png" / "Screenshot from … (annotated).png" in the capture's
  folder. While the chooser shows, the editor steps aside (its surface
  goes, its marks kept) and comes back after. Without a bus, a portal, or
  a portal choosing files, the copy is saved beside the capture under
  that name (" (2)"… when taken). Either way, the file's line under the
  image says "Enregistrée : NAME" / "Saved: NAME"; the capture's own file
  and the clipboard are left as they are.
- **Choices made here** (to confirm on the machine): `Escape` closes the
  editor and keeps nothing of its marks; a closed editor (but by
  "Supprimer") sends the capture to the history, as the thumbnail's time
  up does, its item seen; "Enregistrer sous…" leaves the editor open; the
  thumbnail's motion out is none (it goes at once); the thumbnail and the
  history item for a capture that was not saved: none (5.8's toast);
  the 2 px of the countdown and the gaps are zoomed as the card is, but
  the 8 px from the bar and the bottom are not (the bubbles' 8 px); the
  history's icon is the image's middle, cut square.
- **Fixed on the way**: pressing to draw a new area and holding without
  moving drew a border of a negative size, which panicked the
  compositor's renderer (5.8); its sides are now at least nothing.
- **Measured headless** (`crates/hocus-compositor/tests/annotate.rs`), the
  real shell on the CPU and a D-Bus daemon of the test's own, an area
  over a window of 3 px black and white squares: the thumbnail 330 ×
  267, 8 px from the bar and the bottom, the file in `Captures d'écran`;
  the pointer on it holds it past 6 s, away it goes after what was left;
  its time up, the history lists it ("Capture d'écran «…» maintenant [3
  actions]") and its "Annoter" opens the editor; ✕ deletes the file and
  empties the clipboard; an arrow (red), a box (green), a text and a blur
  where they were drawn in the written file, the rest as it was; a blur's
  variance under 2 % of what it was, grey, not covered; `Ctrl+Z`,
  `Ctrl+Shift+Z`, the undo button and `Escape` on a text; "Copier et
  fermer" writing the file and the clipboard the same bytes; undone back
  to nothing, the file and the clipboard untouched. Unit tests for the
  marks' pixels (an arrow's line and head, a box's frame, a text's colour
  and outline, a blur's variance, a blur over the marks before it), the
  thumbnail's and the editor's layouts and fit, the countdown, the
  marks' sizes in the image's pixels, undo and redo, the names a copy
  takes, the portal's addresses and a bus without one, the drag's
  address, the lines. On the Intel GPU, the screenshots of the pull
  request.
- **Not built**: a drag icon; the keyboard on the thumbnail; a portal's
  file chooser tried by hand (no portal answers in the tests: the copy is
  saved beside, as without one).

## 5.10: screen recording

### The approved spec

Approved by the maintainer on 2026-11-03 (issue #400), variant **A** of
the 5.10 mockup: the indicator in the bar.

**Starting**

- **From the capture**: 5.8's toolbar gets a **Photo / Vidéo** switch at
  its left. Vidéo turns the area's frame red and the main button into
  **Démarrer** / **Start** (red, with a white dot).
- **Modes**: Zone / Fenêtre / Écran (no "all screens" for video).
- **Toggles**: **Son** / **Sound** (the computer's output, on by default)
  and **Micro** (off by default).
- **Countdown**: Démarrer runs a 3-2-1 countdown (as 5.8's delay, with a
  red ring) and the capture UI leaves.
- **Shortcut**: `Super+Shift+R` (`[shortcuts] record`) starts with the
  last settings and stops a running recording.

**While recording**

- **The bar**: under the clock, a 48-wide item (radius 12,
  `rgba(242,84,91,0.16)` with a 1 px `rgba(242,84,91,0.55)` ring) with a
  red dot that beats (1.4 s) and the elapsed time (Martian Mono 12 px,
  `#FFD9DB`).
  - Hovering it shows **pause** and **stop** under it.
  - Paused, the dot turns amber and stops beating, and the time reads
    "pause".
  - In the full form the same item sits under the clock, with the time
    and both buttons always shown.
- **The frame**: the recorded area keeps a 2 px red dashed frame, 3 px
  outside it, which is never in the video. It is dimmed while paused.
- **Never recorded**: the bar's item, the frame, the capture UI and the
  cursor's own indicator.

**The file**

- **Format**: MP4, H.264 + AAC, at 30 fps, at the area's real resolution
  (even sizes). WebM (VP9 + Opus) is a setting (`[recording] format`) for
  the settings sprint.
- **Encoding** runs off the loop: frames from the compositor's own
  rendering, read back without stalling the loop; audio from PipeWire (the
  default sink's monitor and/or the default source). Prefer the GPU's
  encoder through VA-API when there is one, else software.
- **Saved** in `$XDG_VIDEOS_DIR/Enregistrements d'écran/` (`Screen
  recordings/` in English) as "Enregistrement du 2026-11-03 à
  14-32-05.mp4".

**Stopping**

- Stop, or `Super+Shift+R`, ends the file.
- 5.9's thumbnail follows, with the first frame, the duration and
  "Enregistrée · MP4". Its buttons are **Ouvrir** / **Open** (the default
  player) and ✕ (delete), and its drag drops the file.
- After that it goes to the notifications' history as a capture does.

**Limits**

- One recording at a time.
- Locking the screen stops and saves it.
- A screen unplugged during a recording of that screen stops and saves
  it.

### As built (5.10, 2026-11-03)

- **Who does what.** The compositor records (`crates/hocus-compositor/src/record/`):
  what is chosen, the countdown, the frame on the screen, the frames, the
  encoder, the file. The shell draws the toolbar in Vidéo, the bar's item
  and the thumbnail. The lines (`crates/hocus-link/src/record.rs`):
  `capture … photo|video sound|nosound micro|nomicro` (the toolbar's
  state, 5.8's line with three more words), `capture photo|video|sound|micro`
  (its clicks); `recording ID running|paused ELAPSED` and `recording-end`
  (the bar's item: the shell counts the time on from what it was told);
  `recorded ID OUTPUT WIDTH HEIGHT DURATION FORMAT`, the folder, the file
  and the first frame's picture after tabs (the thumbnail; an empty file:
  it could not be saved, a toast says so); from the shell `record
  pause|resume|stop` (the item) and `record forget|drag ID` (the
  thumbnail).
- **The toolbar** (`crates/hocus-shell/src/capture/draw.rs`): the switch
  first, its tint `rgba(255,255,255,0.06)` 3 px around its tabs (30 tall,
  12 px in, 12.5 px bold, radius 8; Photo chosen `#EEF1F6` on `#0B1020`,
  Vidéo chosen white on `#F2545B`, the other `#C9D0DC`), 4 px before the
  modes. In Vidéo: Zone, Fenêtre, Écran (5.8's buttons, their icons
  kept), the separator, Son and Micro (the mockup's icons, 16 px; on, the
  green tint and ring; off, at 0.6 of their opacity), then **Démarrer**
  (14 px in, a 10 px white dot 7 px before the word, `#F2545B`). The
  toolbar's surface takes the size of what it shows: switching resizes it,
  still centred. The compositor turns the area's border and the window's
  ring `#F2545B` in Vidéo. "Tous les écrans" asked in Vidéo is the screen.
- **The countdown** is 5.8's disc and number, with a 2 px
  `rgba(242,84,91,0.7)` ring inside its edge, always 3 s (the toolbar's
  delay is Photo's), on the screen recorded. Cancelled by `Super+Shift+R`
  or the screen locking.
- **The frames** (`record/frames.rs`): as a share's (5.11, `share::frames`):
  the backend draws the recording's frame at each turn of its loop when
  something changed, at most 30 a second, into a buffer of its own, as the
  screen shows it but for what only the screen shows (`Purpose::Capture`:
  the capture's UI and toasts, the recording's frame, the share's ring; and
  the bar's item, `hocus-bar-in-use`, which no capture, share or recording
  shows any more), the pointer in it. An area: its screen drawn moved by
  the area's corner, into a buffer of its size; a window: the window alone
  (its surfaces and popups), the pointer when it is over it, into a buffer
  of the size it had as the recording started (cut, or black around it,
  when it changes). The loop never copies a frame: the CPU renderer's
  buffer is lent to the recorder's thread, the GPU's read back into a
  pixel buffer that the read-back thread maps and lends
  (`share::glread`, which now hands frames to a sink, `share::sink`: a
  share's stream or a recording). A recording's frames are counted apart
  from the shares' (a key of its own).
- **The recorder** (`record/encode.rs`), a thread of its own per
  recording: it names and makes the file, starts **ffmpeg** (a program of
  its own, `Config::ffmpeg`, `ffmpeg` on the `PATH`), copies each frame lent
  into a frame of its own and gives it back at once, and writes ffmpeg
  exactly 30 frames a second of the recording's time, its pauses left out
  (the last frame again while nothing changed): the file's pace is the
  recording's, whatever the loop's. Raw `bgr0` frames on ffmpeg's input;
  H.264 through VA-API (`h264_vaapi`) on the render node the compositor
  draws on when it draws on a GPU and ffmpeg encodes there (tried once, on
  a fifth of a second of black), else `libx264` (`veryfast`,
  `zerolatency`, CRF 23), else `libopenh264`; AAC at 160 kb/s; a keyframe
  every 2 s; MP4. WebM (`[recording] format = "webm"`): VP9 (`vp9_vaapi`,
  else `libvpx-vp9`, real time) and Opus. Once stopped, the last frames
  are written, ffmpeg's input closed, and it ends the file (2 min at
  most, then it is killed: its process is the recorder's own); a file
  ffmpeg could not end is removed. Then a picture of the first frame (640
  px wide at most, PNG, in the runtime directory) for the thumbnail.
- **The sound** (`record/audio.rs`): with Son, a PipeWire capture stream
  `hocuspanda-recording-sound` with `stream.capture.sink = true` (the
  session manager links it to the default sink's monitor); with Micro,
  `hocuspanda-recording-micro` (the default source); raw 32-bit floats, 48
  kHz, stereo (PipeWire converts), on a thread of their own with
  PipeWire's loop. A mixer thread sums them and writes them to a pipe
  ffmpeg reads (a FIFO in a directory of its own in the runtime
  directory, never waited on: opened to read and write, written without
  blocking), at the recording's pace: as many samples as it has lasted,
  silence where nothing came (nothing playing, no microphone, no
  PipeWire), none while paused. The sound is 100 ms late on its capture
  (PipeWire's buffers): the frames are put as late in the file
  (`-itsoffset`), so that they meet. Without Son nor Micro, no sound track.
- **The frame on the screen** (`record/draw.rs`): dashes of 8 every 16 (× 1.5,
  as the capture's sizes), 2 px (3), 3 px (4.5) outside the area or the
  window (which it follows), `rgba(242,84,91,0.9)`, half as opaque while
  paused; a whole screen's just inside its edges. Solid rectangles, on the
  screen only.
- **The bar's item** (since 5.12, issue #445: the recording's line, its
  pause and **Arrêter l'enregistrement** in the popover of the sign of
  what is in use; since 5.12b, issue #561, the item's red, its dot and
  its time are the sign's recording tile again, without its buttons;
  [bar.md](bar.md), as built, 5.12; what follows is 5.10's item as it
  was): one more use of
  5.11's sign, over the sharing one (10 px between them), on the same
  surface; the agents move under both. 48 wide, 6 px in, its 10 px dot 4
  px over its time's 16 px line (Martian Mono 12 px, `#FFD9DB`); the dot
  `#F2545B` beating from 1 to 0.35 of its opacity and back over 1.4 s
  (ease-in-out, drawn again every 50 ms; still with reduced motion or on
  the CPU, where it draws again every 250 ms for the time); paused, amber
  `#F5B44A` and still, the time "pause" ("paused" in English). Opened, 6
  px more, then pause (or ▶ to go on, `rgba(255,255,255,0.06)`) and stop
  (`rgba(242,84,91,0.85)`, a white square), 36 × 30, radius 8, 4 px apart,
  lit `rgba(255,255,255,0.10)` under the pointer. In the mini form it opens
  under the pointer (the surface grows, the agents move down) and closes
  as it leaves; in the full form it is always open. The time reads
  "0:12", "12:04", "1:02:03".
- **The thumbnail** is 5.9's card: the first frame fitted in its place, a
  40 px disc `rgba(255,255,255,0.18)` with a white play triangle in its
  middle, the length at its bottom right (Martian Mono 10.5 px on
  `rgba(0,0,0,0.6)`, radius 6; the disc, the triangle and the length's
  backing are drawn into the picture on the thread that makes it, since
  the shell's renderer draws pictures over a card's shapes), "Enregistrée
  · MP4" / "Saved · MP4",
  **Ouvrir** / **Open** (green tint) and ✕; its countdown and its hold as
  5.9's. Ouvrir, or a click on the picture, gives the file to `xdg-open`
  (found in the shell's `PATH`, started by the compositor) and sends it
  to the history; ✕ deletes the file (`record forget`, on a thread of the
  compositor's); dragged, its file drops (`text/uri-list`, the
  compositor's drag). Its time up, the history keeps it as a capture's:
  "Enregistrement d'écran" / "Screen recording", its file's name, its
  length and folder, its first frame as its icon, **Ouvrir**, **Ouvrir le
  dossier** and **Supprimer**.
- **Where it goes**: `$XDG_VIDEOS_DIR`, else `XDG_VIDEOS_DIR` in
  `user-dirs.dirs`, else `~/Videos`; in it `Enregistrements d'écran` or
  `Screen recordings` by the messages' locale; "Enregistrement du
  2026-11-03 à 14-32-05.mp4" / "Screen recording from 2026-11-03
  14-32-05.mp4" in local time, " (2)" when taken.
- **It stops** from the bar's item, `Super+Shift+R`, the screen locking
  (for now: the session leaving its virtual terminal, as the display
  manager's lock or another user's session shows: the tty backend's
  pause), its screen unplugged, its window closed, a frame that cannot be
  drawn, or ffmpeg failing (the toast "L'enregistrement n'a pas pu être
  enregistré"). Stopped before its first frame (#550), nothing is saved:
  ffmpeg, which has nothing to end, is stopped and its sound left (the
  same toast); and the sound's mixer, finishing, waits 2 s at most for
  ffmpeg to read what is left, since the pipe it writes is its own to read
  as well and so is never broken, only full. Once stopped, the sound is
  ended first, then the last frames are written: ffmpeg 6.1 (Ubuntu
  24.04's) reads no frame before it has the sound's first second or its
  end, so a recording stopped within its first second, its last frames
  written first, waited for ever (#550, v2's CI). The sound always comes
  (silence when PipeWire gives nothing), and ffmpeg is killed if the last
  frames are not written within 30 s. One at a time: Démarrer while one records does nothing
  (the capture stays open for a photo).
- **Choices made here** (to confirm on the machine): ffmpeg as a program
  rather than a library (below); the pointer is always in the video
  (the toolbar's "include the pointer" is Photo's); a window's recording
  keeps the window's first size; a whole screen's frame runs inside its
  edges; the countdown is 3 s whatever Photo's delay; `Super+Shift+R`
  during the countdown cancels it, and while the capture is open starts
  with what it shows; the mode is shared by Photo and Vidéo (all screens
  becomes the screen); Micro is "Mic" in English; paused reads "paused"
  in English; the time goes past an hour as "1:02:03"; the bar's item and
  the sharing sign are one surface, the item first; the thumbnail keeps
  5.9's card and sizes (the mockup drew the picture 120 tall); Ouvrir
  sends the recording to the history; the frame's corners are square;
  the sound is put 100 ms on the frames.
- **ffmpeg, not a library.** No crate that `cargo deny` accepts encodes
  H.264 and AAC (an AAC encoder in Rust is missing; `fdk-aac`'s licence is
  not permissive), let alone through VA-API. So the encoding is ffmpeg's,
  the system's own program, which the .deb recommends: never linked, its
  codecs' licences stay its own (Debian's is GPL with libx264), building
  needs nothing more, and a codec or a VA-API driver that crashes ends the
  recording, never the session (in the compositor's process a GPU
  driver's fault would end every window). Frames go to it on a pipe from
  the recorder's thread, written straight from the frame it copied.
- **Measured headless** (`crates/hocus-compositor/tests/recording.rs`),
  pixman, the encoder libx264 on the CPU, the files decoded by ffmpeg and
  read by ffprobe, each test's videos' folder, runtime directory and
  PipeWire its own: Vidéo turns the area's border red, Son on and Micro
  off by default, the modes without all screens; Démarrer counts 3, 2, 1
  with the red ring, nothing darkened, then records; `Super+Shift+R`
  stops, and again starts with the same area and settings, then stops; an
  area of 451 × 301 recorded at 450 × 300, H.264 at 30/1 in an MP4 in
  `Screen recordings`, named for when, no red pixel in any of its decoded
  frames (the frame is on the screen, outside the area), the window's
  colour in it, and none of its frames' bytes copied on the loop (a
  hand-over's time printed, never asserted); a window recorded alone at
  its even size, its frame never in it; a pause left out of the file's
  length, its frame dimmed on the screen, and no frame sent while paused;
  with Son, Micro or both an AAC track (a PipeWire of the test's own, no
  session manager: silence), without them none; WebM as VP9 and Opus;
  one recording at a time; the screen locking and the screen unplugged
  stop and save it; the thumbnail's delete removes the file. With the
  real shell: the toolbar's Vidéo tab, its new size still centred, Micro
  on and off, Démarrer and the countdown; the bar's item closed then open
  under the pointer (its surface's height), pause, go on and stop by its
  buttons, then the thumbnail in `Vidéos/Enregistrements d'écran`; in the
  full form its buttons without hovering; Ouvrir giving the file to
  `xdg-open` (the test's own) and sending it to the history, ✕ deleting
  the next one. Unit tests for the lines, the names and folders, ffmpeg's
  arguments and its encoders' list, the even sizes, the timeline, the
  mixer, the streams' properties, the frame's dashes, the toolbar's
  layouts, the item's time, beat and buttons.
- **Not built**: a lock screen of the desktop's own (the session leaving
  its terminal stands for it); the cursor's own indicator (none is drawn
  yet); the WebM setting in the settings' UI (the settings sprint);
  DMA-BUF frames (shared memory, copied off the loop); a recording's
  frame of its own size following a window that changes size; VA-API
  measured on a real GPU (left to the maintainer's machine: tests never
  draw on one).
