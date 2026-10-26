# Architecture

Status: initial design, to be refined issue by issue. Decisions with lasting
consequences get a record in [decisions/](decisions/).

## One process, one loop

Hocuspanda is a single Rust process: the Wayland compositor **and** the whole
shell (widget column, dock, launcher, notifications, quick settings, menus,
lock screen). It is built on [smithay](https://github.com/Smithay/smithay)
(MIT).

```
            ┌─────────────────────────── hocuspanda ───────────────────────────┐
 Wayland  ──┤  calloop event loop — owns ALL state (no locks, no sharing)      │
 clients    │   ├─ smithay: protocols, input, outputs, XWayland                │
            │   ├─ core model: windows, desktops per screen, layout, focus     │
            │   ├─ shell: widgets drawn into the scene, animations             │
            │   └─ control API socket (+ event stream)                         │
            │                ▲ channels (results only)                         │
            │  worker threads: D-Bus, network, audio, files, AI, search        │
            └──────────────────────────────────────────────────────────────────┘
```

Rules:
- **The loop owns the state.** Nothing outside the loop holds a reference to
  it. Worker threads receive owned requests and send owned results back
  through calloop channels. The type system enforces it.
- **Nothing slow on the loop.** No blocking I/O, no D-Bus calls, no process
  spawning that waits. Every loop handler is timed against a frame (16 ms);
  one over budget is logged with its name and duration, and kept in the
  `stats` of the control API with the frame-time histogram of each output.
- **A watchdog that does not bite a quiet screen.** A thread wakes the loop
  every second and checks it answered; after 10 s without an answer it
  writes every thread's state and backtrace (gdb when installed) to
  `$XDG_STATE_HOME/hocuspanda/stall-*.txt`, then logs the stall. It does not
  kill the session (yet): what to do with a stuck loop on a real seat is
  decided with the DRM backend.
- **Logs** go to the terminal and, for a session, to
  `$XDG_STATE_HOME/hocuspanda/hocuspanda.log` (the previous one kept as
  `.old`), written by a worker thread through a bounded queue.
- **Monotonic time everywhere.** Animations, timeouts, idle, caches. Wall
  clock only for what the user reads (the clock, the calendar).
  Compositor time is read from one clock, which tests replace with a
  virtual one that moves only when they advance it.
- **The shell is part of the scene.** Its surfaces are known by identity,
  output and layer; no title matching, no positioning over IPC.

## Crates

A Cargo workspace; names may move as the code lands.

| Crate | Role | Depends on smithay |
|---|---|---|
| `hocus-core` | Pure model: windows, desktops per screen, outputs layout, focus, snapping, placement, rules; the configuration, read and written one key at a time with the file's comments kept (the settings app and the AI's proposals share the writer). Deterministic and unit-tested | no |
| `hocus-compositor` | smithay glue: protocols, backends, input, rendering, XWayland | yes |
| `hocus-ui` | Small retained widget toolkit drawn into textures: layout, text, theme tokens, springs, hit testing, accessibility tree | no |
| `hocus-shell` | The desktop's own UI built on `hocus-ui`: widget column, dock, launcher, notifications, quick settings, menus, lock screen, pandas | no |
| `hocus-settings` | The settings app: a Wayland client of its own, drawn with `hocus-ui`, changing the configuration file ([decision 0010](decisions/0010-settings-as-a-client.md)) | no |
| `hocus-annotate` | The screenshot annotation window (#128): arrows, frames, texts and blurred areas drawn over a capture, a Wayland client drawn with `hocus-ui` as the settings are; copying goes through the compositor, which keeps the clipboard | no |
| `hocus-platform` | Services behind traits: power, battery, backlight, network, audio, session, notifications (D-Bus); Linux and FreeBSD implementations | no |
| `hocus-api` | Control protocol types, socket server, event stream; MCP bridge | no |
| `hocus-test` | Scenario runner, test clients, screenshot comparison | yes |
| `hocuspanda` | The binary | yes |
| `hocus` | Command-line tool: `hocus msg`, `hocus scenario`, `hocus mcp`… | no |

Keeping `hocus-core`, `hocus-ui` and `hocus-shell` free of smithay keeps them
fast to compile and test, and lets the UI be developed in a plain window.

## Backends

- **winit (nested)**: the compositor runs in a window of the current session.
  Default for development: no session restart, no VT switch.
- **headless**: no screen; outputs are offscreen buffers (GLES on a render
  node, or the pixman software renderer on CI). Used by every scenario test.
- **tty**: the session itself on a virtual terminal (`--backend tty`, the
  default when started outside a graphical session). libseat (logind or
  seatd) gives the seat, its devices and the switches to other terminals
  (Ctrl+Alt+F1…F12, which pause the session and draw everything again on
  return). The primary GPU's display engine is driven through DRM/KMS
  (smithay's `DrmOutputManager`), drawn with GLES on GBM (Mesa's software GL
  in a VM without 3D); screens plugged and unplugged are followed through
  udev; linux-dmabuf is offered for clients that draw on the GPU. libinput
  reads keyboards and mice. Screens of other GPUs stay dark for now (#123).
  Tried in a QEMU VM on virtio-gpu, then on hardware.

Screens are set the same way on every backend (`outputs.rs`): the backend
lists its screens, lit or off, with their modes, and carries a setting out
on the hardware (a mode on the CRTC, adaptive sync, a buffer size headless);
what follows on the desktop is common — the output's state, the model's
geometry, windows kept in place, relayout, events. Wanted settings come from
`[[outputs]]` and, for where screens are, the `[[layouts]]` entry for the set
connected, or, for the session, from wlr-output-management; they are
settled after the turn of the loop where something changed, and
configurations are answered then. Output globals are retired in two steps
(hidden, destroyed seconds later) so that a client binding one meanwhile is
not cut off.

The clipboard history (#71) reads every text copied, by Wayland and X11
clients, on a worker thread; a copy marked secret
(`x-kde-passwordManagerHint`) is never read. Copies are numbered as they are
made, so one read late goes to its place. Only the session keeps it on
disk, sealed with ChaCha20-Poly1305 under a key file of its own (a copy of
the file is unreadable; the account is not protected from itself). What the
history and the emoji picker choose is typed into the focused application
through text-input-v3, once it has the keyboard back; without an input
method, the compositor sends the text itself, which the protocol has the
client apply.

Window motion (#33) uses the shell's vocabulary (`hocus_ui::motion`:
durations, easings, springs, values retargeted from where they are), each
window's opacity and scale evaluated in closed form at the clock's time when
the scene is drawn. While a value moves the loop asks for frames, which each
output's clock paces; at rest it asks for none, and nothing is queued per
frame. Tests run on a virtual clock and sample motion at a given time;
those that do not ask for it (`motion = true`) see windows appear at once.
Switching desktops (#36) slides a strip of the screen's desktops on that
screen only: the model switches at once and only the picture moves, the
windows of the desktops passing by drawn where they wait. A three-finger
swipe holds the strip under the fingers and lets it spring to the nearest
desktop; the switcher shows the screen's desktops meanwhile, with their
names under them when they have some (#192).
The Matrix theme (#50) has windows of its own: one materializes from a
column of glyphs that fills its place row by row, one closing glitches,
turns to glyphs and drains off as rain. Like the magic lamp, these are
drawn with the elements every renderer has (rows and columns cut from a
sheet of glyphs drawn once per scale, plain colours, the window's
picture), evaluated in closed form at the clock's time, so that headless
scenarios sample them. The glyphs are strokes of our own after
half-width katakana: no bundled font has them. The theme's other window
motions (#154) are drawn the same way: a window minimizing switches off
as a CRT into its dock icon and comes back on from there, one maximizing
races circuit traces to its new place and springs there, one snapping
springs in under the grid line of its zone; a dragged window leaves a
phosphor trail of where it was drawn lately (positions kept as it moves,
not per frame), a resized one glows and reads out its size. What shines
over the dock (the CRT's line and flash, the traces) is drawn over the
panels, the window's own picture with the windows. Its shell (#155) is
drawn the same way, in `matrix_shell` and `matrix_screens`: the desktop
left behind breaks into glyphs that rain away, Alt+Tab and the overview
come over black and slow rain, the lock's rain cascades over the desktop
and comes to rest behind its padlock of glyphs, toasts are typed in and
scatter, the column comes and goes behind a band of rain, the launcher
jacks in and rains away, the boot wakes, lists the windows as a terminal
and says SYSTEM READY. The shell's views leave room for it (a variant
without background, the padlock's place, the progress bar's) and say
where their parts are; what is drawn is parts of their pictures, glyphs
and plain colours, each a closed form of the time since it began. Rain
that stays (behind the lock, the launcher) stands still; nothing is drawn
once a moment is over. What one brush draws in a scene (hundreds of
glyphs and colours) is one element (`matrix_layers`), kept from frame to
frame by where in the code it is taken: the same id, a commit that moves
only when its strokes change, and as damage only the strokes that came,
went or changed (of a colour that grew, what it gained), so a redraw
over still rain repaints nothing and moving rain only its drops (#221).
The theme's wallpaper is
its still rain, drawn with the other wallpapers on their worker thread.
Wallpapers that move (#108, `rain` and `stars`) are GLES shaders: before
a GLES backend draws an output, the wallpaper is painted into a texture
of the output's size at the clock's time (the rain looks its glyphs up in
an atlas of them), and the scene shows that texture in place of the still
picture. The still picture, made on the worker as any wallpaper, stays
whenever nothing should move: reduced motion, the power saver, a
fullscreen window or the lock over it, and the pixman renderer, which
keeps scenarios deterministic. Only while a wallpaper moves does the loop
ask for frames.
Frosted glass (#37) is GLES shaders too. The shell's panels (the column,
its popovers, quick settings, the launcher's card, toasts) are drawn on a
`Glass` paint that the theme makes see-through when glass is on, and their
trees say where those boxes are. Before a GLES backend draws an output,
each pane of glass, from the back, has the scene behind it drawn into a
texture of its own (the pane and the blur's reach around it) through a
damage tracker: when nothing behind changed, nothing is drawn, and the blur
of the last time stays. Only when something did is the dual Kawase blur run
on it (halved and doubled back a few times, a few samples per pixel), and
the scene shows the result under the pane, cut to its corners. Windows a
rule asks it for get the same behind them. The stats count each blur, per
output, so that scenarios check a still scene is never blurred again.
Without shaders (pixman), with transparency off, high contrast or the
power saver, the theme has no glass and the panels are solid. Windows'
shadows take their darkness from the theme.
The Matrix theme's refinements on the GPU (#169, `crt`) are painted the
same way, before a GLES backend draws an output: a closing window's
picture is flattened once into a texture, and on each frame of its glitch
a shader draws it shifted in bands, red and blue taken apart, into a
texture the scene shows instead of the bands (a new id each time: only
the window is damaged). The profile of the phosphor bloom, a screen's
scanlines and the vignette are painted once by another shader; the
brushes stretch the profile around bright lines (as strokes of their
layer), and fullscreen overlays show the scanlines and vignette with ids
kept from frame to frame, so that a still overlay repaints nothing. The
stats count the glitch's frames per output. Pixman, reduced motion and
instant motion keep the plain effects.
The window switcher (#39) shows each card's window live: the window's own
surfaces drawn again, scaled into the card (a rescale element: a surface
element's size follows the output's scale, not the one it was made at), so
there is no snapshot to keep up to date.

A mirroring screen (#125, `mirror` in its `[[outputs]]` entry) stays lit but
leaves the layout as an unplugged one would: its windows wait on the
primary screen and come back where they were when it shows its own desktops
again. It draws the source's scene, scaled to fit and centred, through the
same elements as the source (no copy of the source's frame), so it follows
the source's damage and shows its cursor.

Screen recording (#70) takes a frame after the turn of the loop, at most
`fps` a second and only when the screen drew something new (or a click's
ring is growing), with the red frame and the bar left out; the frame goes,
owned, to a worker thread that feeds an encoder process (ffmpeg, VA-API when
the card has it) at a steady rate and paints click rings into it. A frame
the worker has no room for is dropped; the loop never waits for it.

Its options (#131) stay off the loop too. Sound is recorded by the encoder,
one segment per stretch between pauses, and put with the picture once it is
done, cut or padded to its length. The webcam is read by an encoder process
of its own as small square frames that the worker cuts into a circle in each
frame; on the screen only a mark shows where it goes, and the pointer moves
it. Keys are drawn into a small picture on the loop when a combination is
pressed, and the worker lays it near the bottom of the frames that follow.
Only combinations with Super, Ctrl or Alt and keys that are not text (Enter,
arrows, F-keys…) are shown: plain typing, which may be a password, never is,
nor anything while locked or while polkit asks.

The recording editor (#132) is `hocus-trim`, a client of its own beside the
annotation window (they share the window code, each editor acting on what
its view asks). It asks everything of the encoder as processes on worker
threads: the recording's length, a strip of thumbnails, the frame where a
handle stops, the frames while it plays, and the export, cut in MP4, WebM
or GIF. A size limit is met by trying again smaller (a lower rate, a
narrower GIF) until the file fits. The compositor only offers it, from the
recording's toast.

System updates (#84) are looked for on a worker thread every few hours,
never while a window is fullscreen or on the power saver: apt, dnf, pacman
(`checkupdates`) or pkg, read without root; installing goes through
`pkexec`, so the session's own polkit dialog asks for the password. Only
the session asks the system's manager; nested or in tests, only commands
the configuration names run.

Calendars (#75) are fetched on worker threads every ten minutes (CalDAV:
PROPFIND to find the calendars, REPORT for the coming weeks; a
subscription: one GET), kept in a sealed cache in the state directory for
when the network is down, and expanded on the loop (recurrences, exceptions,
time zones) into the coming weeks' events. Reminders come ten minutes and
one minute before, with Join when the event has a call's link. Passwords go
from the settings to the compositor over the control socket and into the
keyring (the Secret Service; a sealed file without one), never into the
configuration; an account taken out of the configuration takes its cache
and its password with it. Accounts count only once the configuration is
read, so a session starting never forgets one.

Google calendars (#137) come from GNOME Online Accounts (GOA), never from
an OAuth client of Hocuspanda's own: a worker thread follows
`org.gnome.OnlineAccounts` on the session bus through its public D-Bus API
(the object manager at `/org/gnome/OnlineAccounts`, `InterfacesAdded`,
`InterfacesRemoved` and `PropertiesChanged`), and keeps the accounts whose
`org.gnome.OnlineAccounts.Account` says `ProviderType` `google` with
`CalendarDisabled` false. Each sync asks GOA for a token
(`org.gnome.OnlineAccounts.OAuth2Based.GetAccessToken`) and reads the
account's calendars through Google's CalDAV endpoint
(`https://apidata.googleusercontent.com/caldav/v2/`: principal, calendar
home, then each `<calendar id>/events/`) with the same CalDAV client, the
token as a bearer; a token Google refuses is renewed once
(`Account.EnsureCredentials`, then `GetAccessToken` again). Tokens stay
GOA's: asked for each sync, never stored, never logged, and sent to
Google's own server only. An account removed in GOA, or its calendar
switched off there, takes its events and its cache with it; until GOA
first answers, the cache of its accounts stays. Without GOA nothing shows
and nothing is logged above debug; the settings' Accounts and calendar
page lists GOA's Google accounts read-only and opens GNOME's own settings
for them (`gnome-control-center online-accounts`) when installed. Tests
run GOA's and Google's stand-ins on a private bus and 127.0.0.1; the
harness alone points the client at the stand-in, and on the virtual clock
without one Google accounts never sync.

The command field (#44) understands short requests in English and French
without a model (`hocus-shell::intents`: screenshots with a target and a
delay, opening an application or a settings page, desktops, recording,
focus mode, night light, notes) and shows each as a card to confirm before
anything runs. A question goes to the provider `[ai]` names (ollama, a
llama.cpp server, OpenAI's chat completions, Claude's Messages API) on a
worker thread of its own, which reads the provider's key from the keyring
(the Secret Service; a sealed file without one), asks with `ureq` and sends
the answer back piece by piece through a channel; the loop only appends the
pieces and lays the card out once per turn; a long answer scrolls inside
the card (the wheel over it, Page Up and Page Down). Without a model named,
the worker asks the provider which models it has at the first question
(Claude's newest Opus, ollama's first…) and the session keeps that one:
none is written in the code. A Claude model that refuses the request's
options (effort, fallbacks) is asked once more without them. The AI is off
until `[ai] enabled` turns it on. Typing again, closing the field
or Stop tells the worker to stop at the next piece, and what an older
question brings back is dropped. Nested or in tests, a provider's usual
address is never asked: only the one the configuration gives. The answer's
card (`hocus-shell::answer`) is shared with circling to ask (#46).

An answer may propose actions (#197), from a fixed set the shell knows
(`hocus-shell::proposals`): open a settings page, set one configuration key
among a list of plain looks and habits (nothing that runs a program, names
a file or an address, or weakens the lock), run a request the command field
understands offline, or hand the question and the answer (and what was
circled) to a herdr agent. The model is told the set in its instructions
and ends its answer with a fenced `hocus-actions` block, one JSON object per
line; the card never shows the block, and each line is read strictly: an
unknown action, page, key or value, a request not understood, a malformed
line are dropped, and three at most are kept. They come first under the
answer, the main one first; choosing one shows what it will do (a diff of
the configuration file as last read, the text an agent gets) and it runs
only on a second, explicit choice; Cancel goes back to the answer. A
configuration change goes through the settings app's own writer
(`hocus-core::config_edit`), on a worker thread, so the file keeps its
comments and the watcher applies it. A question handed to an agent goes to
its herdr as a prompt (`agent.prompt`) and into the agents' journal as that
agent's. While agents are stopped (#176), nothing is offered, and what was
offered before does not run. Only herdr's agents are offered for now, not
MCP agents.

Circling to ask (#46) is a left drag with `[circle] modifiers` held
(Super+Shift: Super alone draws on the screen, #86). One sheet the size of
the screen (`hocus-shell::circle`) holds the loop, drawn piece by piece as
the drawing pen's line is, the menu beside it and the answer's card. The
release asks for the loop's bounding box to be captured after the turn,
without the sheet; a worker encodes it, hands it to the program that reads
text (`[screenshots] ocr`, tesseract) through a file of the runtime
directory, removes the file and sends the text back. What is chosen before
the text is there waits for it. Explain, Translate and a question go
through the same worker as the command field's questions, told who asked,
and are offered only while the AI is on; Copy the text, Note and Search
(the launcher's search engine through `xdg-open`) act at once. The menu
holds the keyboard and the pointer: Escape, or a press anywhere else that
no client sees, closes it all.

The radial menu (#40) is a window's actions around the pointer, opened by
`[radial]`'s button with its keys held (Super and the right button: Super
and the left one draw, #86) over a window, never over the shell. Its
panel (`hocus-shell::radial`) is a square drawn at the pointer and moved
in to stay whole on that screen; it takes the pointer and the keys while
open, and no client sees its buttons. Held, `Gesture` keeps the last
moments of the pointer's way: let go at least 28 px away at 500 px/s or
more (over the last 100 ms) is a flick, which picks the direction it
points at; slower, the sector past 56 px lights up and the release picks
it; nearer, nothing. Close is never flicked: resting 300 ms on it arms it.
Moved less than 6 px, the press is a click and the menu stays for a click
or the arrows, Enter and Escape. What is picked is the compositor's own
action on that window (maximize, pin, send to the next screen or a desktop
of its screen, minimize, on top, snap, close), as the shortcuts and the
control API do it.

Document search (#45) lives in `hocus-platform::search`: an indexer thread
at nice 19 walks the configured folders a few entries at a time with pauses
between batches, reads what changed since the index was kept (by date and
size), and waits while on battery (the power source quick settings know,
from UPower); a searcher thread answers the command field's queries from
the index, the newest only. The text comes from readers of our own, small
and fuzzed: plain text, HTML without its markup, PDF's text layer (objects
found by scanning, object streams, Flate, ToUnicode maps), and office files
(a minimal ZIP reader and quick-xml); screenshots' text through the OCR
program. The index is an inverted index of folded words ranked by BM25,
the last word taken as a prefix; with a model that embeds (ollama's
`/api/embed`, `/v1/embeddings`), the start of each text has a few
quantized vectors, and both rankings are fused (reciprocal rank fusion).
What is never read (`search::deny`) is checked on an entry's name before
its type is even asked, and again by the one gate every open goes through;
symbolic links are never followed. The loop only sends the configuration,
the power source and the query, and hands what comes back to the launcher,
which shows it under "Documents".

The phone (#85) goes through KDE Connect's daemon on the session bus,
followed on a worker thread and never started by us: the session starts it
from its autostart entry, with the environment its clipboard needs to reach
the compositor (ext-data-control). Without it, nothing shows and nothing is
logged but at debug level; it is found when it appears. The worker reads
the phones paired and within reach and their battery again whenever the
daemon signals a change, and reads each notification a phone posts; the
loop shows the first phone in the column and the notifications as toasts.
A toast takes a reply typed into it (the keyboard goes to it while it is
typed, and it waits), which goes back through the daemon. KDE Connect's own
copy of a phone notification, sent to our notification server, is held
(its `x-kde-appname` and `x-kde-eventId` hints say what it is). Ringing,
sending the clipboard or files (`hocus msg send-to-phone`) and mounting
its files each run on a thread of their own, so a phone slow to answer
holds nothing.

Wireless adb (#153), once `[phone] adb` allows it, pairs a phone for
debugging and mirrors its screen. adb and scrcpy are looked for on the
`PATH` the session's programs get, on a worker; without adb nothing runs
and nothing shows. Pairing follows Android Studio's flow: the popover shows
a QR code `WIFI:T:ADB;S:<name>;P:<password>;;` (a fresh random name and
password each time, never logged nor named to screen readers); the phone
scans it and advertises a `_adb-tls-pairing._tcp` service under that name
over mDNS; a worker polls adb's own discovery (`adb mdns services`) for it,
runs `adb pair` with the password, then `adb connect` to the debugging port
the phone advertises next (`_adb-tls-connect._tcp`), unless adb joined it
by itself. Closing the popover before the phone scanned the code cancels
the pairing (the worker stops, the code is forgotten); once it scanned,
`adb pair` is under way and the phone ends paired whatever we do, so the
pairing goes on and a toast says how it ended, the popover being closed.
Asking adb rather than browsing mDNS ourselves keeps one view of
the network and needs no daemon or crate of our own. Every adb command runs
on that worker with a time limit. "Mirror its screen" starts scrcpy for the
device adb reaches (`scrcpy -s SERIAL`), as any program the user starts;
its window is an ordinary one. With adb but no KDE Connect phone, the
column shows a phone line for adb alone.

Coding agents (#41) come from herdr's API socket (JSON lines): a worker
thread lists the agents, follows each one's status through a subscription,
and lists them again when panes come or go; another carries out focusing
an agent and pressing keys in its terminal (Enter to accept what it asks,
Escape to refuse), each request on a connection of its own. The column
shows the active ones with a panda each and sums up the others; one
waiting for an answer asks in a toast as well. A rest shorter than two
seconds is held back on the loop's clock, so an agent flickering between
work and rest does not make the column flicker. Showing an agent also
brings forward the terminal herdr's client runs in, found by its
processes. Only the session looks for herdr's default socket; nested or in
tests, only a socket the configuration names is used.

Desktops per project (#192): several herdr can be followed, each through
its own client, each tied to a desktop (`[[agents.herdr]]`); an agent's key
on the board is its herdr's and its pane's. The column shows in full the
agents of the desktop its screen shows (and of a herdr tied to none), every
other desktop's in a line with its counts, which goes there when clicked;
short of room, those lines share one, then go; then the meeting card takes
one line (when · title · Join) and a waiting agent's row is said in short
(its name with Yes and No, its question on one line), before the
notifications go and the agents shown in full give any; room left over
goes back to the notifications. A 1280×800 laptop's column so keeps a
waiting agent, the meeting and the account card. The desktops not shown carry a badge wherever the
desktops are drawn (the column's dots and its desktops popover, the
switcher's strip, Alt+Tab and the overview): an agent waiting, a window
asking for attention, an agent done since a screen last showed its
desktop. A window asks for attention when it asks to come forward without
Alex having asked for it (#145): with an activation token made from no
input, or from an input older than their last key or click (the serial
of the last press is kept), or too old (tokens are timed on the
compositor's clock), or with any token while no screen shows its
desktop: Alex goes there by choice, from a badge, never pulled away
(#218). Tokens the shell makes for what Alex starts from it always may
take the focus: programs it launches get one (`XDG_ACTIVATION_TOKEN`,
`DESKTOP_STARTUP_ID`), a notification's action comes after its
`ActivationToken` signal, a tray item is handed one
(`ProvideXdgActivationToken`) before `Activate`; what the session starts
by itself (autostart, a restored session) gets none. An X11 window's
urgency hint asks the same way, and taken back asks no more. A window
asks until it has the keyboard; meanwhile its dock icon wiggles once when
the dock shows, and its dot, and the hidden dock's hint, take the
accent. A waiting agent's toast names its desktop when no screen
shows it.

Notes (#79) are one Markdown file, read and written whole on worker
threads (written to a new file, then renamed over the old one); lines the
notes do not understand are kept as they were. The file is read again each
time the notes open, so edits made elsewhere show.

Comfort over the whole screen (#78): focus mode puts a veil just under the
focused window; the magnifier wraps every element of an output's scene in a
rescale around the pointer, so the pointer's point stays put and input needs
no change; night light warms each screen's gamma ramp (the display
engine's colour pipeline, on a TTY), easing over three seconds, so pictures
taken of the screen keep their true colours. Its schedule is fixed hours or
sunset to sunrise (NOAA's approximation) where the user says they are.

## Rendering

- Compositing: smithay's GLES renderer with damage tracking.
- Effects (blur, shadows, rounded corners, wobbly mesh, genie, depth of
  field, Matrix): GLES shaders in the compositor, reloadable at runtime in
  development builds.
- Shell UI: widgets rasterised on the CPU into textures and cached, redrawn
  only when their content changes: vello_cpu for drawing, parley for text,
  behind `hocus-ui`'s own painting interface
  ([decision 0009](decisions/0009-ui-rendering-stack.md)).
- Frame pacing per output, driven by presentation feedback. Animations
  advance on that clock and never schedule work per tick.

## Protocols

Besides the core (compositor, xdg-shell, xdg-decoration, data device,
XWayland), the compositor serves the protocols applications and tools
expect: xdg-activation (a recent token brings its window forward),
idle-inhibit, cursor-shape, keyboard-shortcuts-inhibit (granted while the
window has the keyboard), xdg-foreign, presentation-time, fractional-scale,
viewporter, single-pixel-buffer, alpha-modifier, content-type, pointer
constraints and relative pointer, pointer gestures, the virtual keyboard,
the primary selection, ext-data-control for clipboard managers,
ext-foreign-toplevel-list, security-context, and wlr-layer-shell for
third-party bars, launchers and wallpaper tools: background and bottom
surfaces under the windows, top above them (under a fullscreen window),
overlay above everything but the cursor; exclusive zones come off the work
area, and a surface may take the keyboard always or when clicked. Clients
started through a security context (Flatpak and other sandboxes) do not see
layer-shell, the virtual keyboard, data control, the window list nor
security contexts.

Screenshot tools and screen sharing read the screen through
ext-image-copy-capture (output and window sources) and wlr-screencopy
(whole outputs and regions): requests are kept on the loop and served
after each dispatch, where the renderer is, by drawing the output or
window again offscreen into the client's shared-memory buffer; a copy
that waits for damage is served once the output drew a new frame.
xdg-desktop-portal-wlr works on them and streams through PipeWire; its
chooser is `hocus share-pick` (#116), which asks the compositor over the
control API (`pick_share`, answered once the user chose): the shell shows
the screens and the windows as pictures and answers `Monitor: NAME` or
`Window: IDENTIFIER`. The configuration files are in `data/portal/`.
While the screen is shared, the column says so with a stop that ends
every capture, toasts are held, and one summarises them afterwards (a
tool that goes while it waits for the screen to change no longer counts). Idle notification (#29) is served;
drm-lease, for VR headsets, waits for an issue of its own.

## Session

What is open (each window's application, output, desktop, place and
state) is saved every thirty seconds when it changed and when quitting, in
`$XDG_STATE_HOME/hocuspanda/session.toml`, written on a worker thread. At
the next start a card offers to put everything back, to choose which
applications, or to start fresh (`[session] restore`); the applications
start again and their windows go back where they were as they come; one
that cannot start, or whose window never comes, is reported in a toast.
Several windows of one application (three browser windows on three
project desktops, #192) are told apart by the title each had when saved:
a window coming back takes the place saved with its title, else the first
of its application still waiting, in the order they were opened, so a
title that changed meanwhile never keeps a window from coming back.
XDG autostart entries start too. Only the session itself saves, restores
and autostarts: nested or headless, the developer's session is never
touched.

## Idle and lock

Without input, the curtain dims every screen with a countdown, then the
session locks. Idle inhibitors (shown windows) and fullscreen windows hold
it off; the countdown starts again when they stop. ext-idle-notify tells
third-party tools. Locked, the screens show nothing but the lock: a lock
client's surfaces (ext-session-lock), or our own lock screen, whose
password goes to PAM on a worker thread. A lock client that goes away
without unlocking leaves our lock screen: a crash never unlocks.

## Control API

- UNIX socket next to the Wayland one (`hocus-0.api` beside `hocus-0`),
  JSON lines, versioned, typed in `hocus-api`. Requests (windows, desktops,
  outputs, focus, input injection, screenshots, scene dump) and an event
  stream (subscriptions). Commands the compositor starts get its path in
  `HOCUS_SOCKET`.
- Served on the loop with non-blocking sockets; a client that stops reading
  is dropped past a bound. Slow work (PNG encoding) runs on a worker thread
  and the reply follows when it is done.
- The same API serves the `hocus` tool, the MCP bridge for AI agents, and the
  test harness. Input injection and scene dumps require an explicit test or
  agent permission (`--api-permission test`; agents come with their seat).
- `hocus mcp` (#42, [mcp](mcp.md)) is a Model Context Protocol server on
  stdio, a program of its own: it blocks on its control client while it
  waits for a state, never on the loop. Started with `--agent NAME`, it sends
  `agent` first; the connection then makes only what agents may (windows,
  outputs, desktops, `ui`, screenshots, events, `act`), whatever the
  server's permission. `act` aims input at a window (a point relative to
  it) or at a labelled element of the shell; the compositor checks, at each
  action, the agent's rights in the configuration in effect
  (`[[agents.mcp]]`: read or act, which app ids, the shell or not), and
  that the session is not locked nor a password dialog open. Typed keys go
  straight to the window, past the shortcuts, through the layout in
  effect.
- Applications' own accessibility trees (#175, `a11y.rs`, read by
  `hocus_platform::atspi`): `window_tree` and an `act` aimed at an
  `app_element` hand the window's description (its client's pid, app id,
  title, size) to a reader thread started at the first request, and are
  answered later. The reader connects to the accessibility bus (given, or
  asked of the session bus's `org.a11y.Bus`), matches the window to an
  application of the registry by pid then app id, and to a top-level
  element by title, and reads its tree breadth first within limits of
  elements, depth and time, leaving out what is not showing and never
  reading a password field's text. Its result comes back through a
  calloop channel; an element found by label becomes an `act` at its
  centre, checked again, its rectangle kept for the ring. An agent reads
  the trees of its apps alone.
- Each agent connection gets a seat of its own (#43, `seat.rs`): a
  `wl_seat` made when it names itself and gone with it, whose pointer and
  keyboard have their own focus. Its input goes to its windows through
  smithay's pointer and keyboard handles, never the user's; what follows
  the user's keyboard (the clipboard history, shortcut inhibitors, the
  cursor a client sets) checks the seat is the user's, and an agent's
  activation token or move request takes nothing from the user. An `act`
  is answered later (the handler returns `None`, and the server's `reply`
  follows): it waits on the compositor's clock, checked after each
  dispatch, for the window's application to take the new seat, for its
  text inputs to say what the field is for once the agent's keyboard came
  in (the compositor tells the agent seat's text inputs itself, since no
  input method runs; it reads their content purpose through its own
  dispatch of text-input-v3 before smithay's), for the ring before a
  click, and for the user on the card of a sensitive action; then it
  checks everything again and lands, through the agent's seat. The
  server tells which agent connections went (`take_departed`). Each seat's
  pointer, ring and card are a small view of `hocus_shell::seat` placed
  where it points, made again only when what it shows changes; the
  column's journal leaves a slot for each agent's window, which the
  compositor draws there, live, as the switcher draws its previews.
- An agent acts on its windows wherever they are (#229). A window on a
  desktop no screen shows is unmapped from the space but keeps its place
  (`Managed::location`): the agent's point is taken from there, and its
  seat's pointer focus is set on the surface found there, which hit-testing
  in smithay's pointer does not tie to an output. While an agent drives
  such a window (an action waits, or it acted in the last ten seconds),
  `refresh_seats` sends it frame callbacks every 100 ms on the
  compositor's clock, for the output that hosts its desktop; one deadline,
  woken by the seats' timer, nothing queued per frame, and nothing once no
  agent drives one. Nothing is rendered for it; a capture of the window
  (`screenshot` of a window) renders its surfaces on demand, mapped or not.
  An agent connection's capture of such a window drives it too: it sends
  a frame callback at once when the application waits for one, and the
  reply is deferred until the window's next commit or 200 ms on the
  compositor's clock (`captures_due`, served after each dispatch).
  The seat remembers the window under its pointer: its pointer and ring
  are drawn only where a screen shows that window; a card waiting for the
  user shows alone, on the screen at its place. A minimized window an
  agent acts on is brought back by `Model::unminimize`, which touches
  neither focus nor the shown desktop.
- A card the user clicked holds their keyboard (`seat_card_keys`, one of
  the modal panels): Enter and Space press its focused button, Tab moves,
  Escape refuses; a press elsewhere lets go.
- The agents' journal (#176) keeps each `act` of an agent connection, carried
  out or refused, with the window as it was and the user's local time, and
  each question the user hands a coding agent from an AI's answer (#197); the
  control server holds whether agents are stopped (`stop-agents`), and the
  compositor then refuses every request of an agent connection, and new
  `agent` requests (kept in the journal too), before anything else; events
  stop reaching them too. An agent connection reads its own entries alone.
  The journal is a ring in memory: nothing per entry goes to disk or to the
  log above debug.
- The settings app ([decision 0010](decisions/0010-settings-as-a-client.md))
  reads what is in effect through it (`config`, `outputs`), follows the event
  stream, and is told which page to show by `open_settings`
  (`hocus settings keyboard`). To record a shortcut, it holds the
  compositor's own back through keyboard-shortcuts-inhibit, so that the
  keys reach it; nothing is written until the new keys are confirmed.
  An AI provider's key typed there goes to the compositor's keyring
  (`ai_secret`), never to the file; `ai` says which providers have one,
  never the keys, and the `ai` event when that changes. Its button stops
  or allows every MCP agent (`set_agents_stopped`) as the stop key does,
  and the `agents_stopped` event tells it. Its Search page reads what the
  document indexer does (`search`: counts and when it last looked, never a
  document) when the `search` event says its state changed, and every two
  seconds on a thread of its own while the page shows; Search again now
  is `search_again`. A folder typed there is checked against
  `search::deny` by its words alone (the settings never open it), and
  refused with the place never read named.
  What is not the compositor's it asks itself, on threads of its own: the
  system's time zone (timedated), the account's name, picture and
  fingerprints (AccountsService, fprintd) on the system bus; the default
  applications and autostart entries in the XDG files (`mimeapps.list`,
  the user's copy of an autostart entry), which every desktop reads.
  While the Network page shows, a worker lists the Wi-Fi networks in reach
  through NetworkManager (a scan at once and every 10 s), joins, leaves
  and forgets them; a password typed there goes to NetworkManager, which
  keeps it, never to the file nor the logs. While the Bluetooth page shows,
  another lists BlueZ's devices with discovery on, stopped as the page
  closes, and pairs, connects, disconnects and forgets them; a pairing
  registers an agent with BlueZ for that pairing only, which asks the page
  to confirm a passkey or shows the code to type on the device (a code
  typed here is not offered yet). The time zone's world map places the
  cities after the system's `zone1970.tab` (tzdata, public domain), read at
  start; without it only the list shows. Its land is Natural Earth's
  1:110m outline (public domain), simplified to half degrees by
  `scripts/land-outline.py` into `crates/hocus-settings/data/land.txt`.
  Tests give it a private system bus with stand-ins, private XDG
  directories and their own copy of `zone1970.tab`, never the machine's.

## Configuration

`~/.config/hocuspanda/config.toml` (or `--config FILE`), applied live on
change. Defaults live in code (`hocus_core::Config`); the file only holds
what the user changed. A thread looks at the file every half second, reads
and checks it, and hands the loop a configuration ready to apply; a file that
does not read is reported with its line and key (log, `config_error` event,
`hocus msg config`) and the previous configuration stays. Tests use a
configuration file of their own. Every key and the shortcut actions are in
[configuration](configuration.md). Themes in their own files.

## Plugins

Rust crates implementing traits from `hocus-shell` (status module, widget,
launcher provider, action, effect), compiled with the desktop and enabled by
Cargo features and configuration. No scripting language, no dynamic loading.
See [decision 0004](decisions/0004-plugins-compiled-in.md).

## Platform layer

Each service is a trait with a Linux and a FreeBSD implementation:

| Service | Linux | FreeBSD |
|---|---|---|
| Seat, session | libseat (logind or seatd) | libseat (seatd) |
| Device hot plug | udev | devd |
| Power, suspend | logind | acpiconf, `zzz` |
| Battery | UPower / sysfs | `hw.acpi.battery` sysctl |
| Backlight | logind / sysfs | backlight(8) |
| Network | NetworkManager, iwd | ifconfig, wpa_supplicant |
| Audio | PipeWire (wpctl) | OSS mixer(8), PipeWire when present |
| Notifications, portals, screensaver | D-Bus (zbus) | D-Bus (zbus) |

CI builds on Linux and FreeBSD from the start, even when FreeBSD services are
still stubs.

On Linux, quick settings and the status row read UPower (battery),
power-profiles-daemon, NetworkManager and BlueZ on the system bus, following
their property changes, and PipeWire through `wpctl` and the backlight
through sysfs every two seconds; changes go back the same ways (the
backlight through logind). logind asks to lock and unlock, and a delay
inhibitor holds sleep until the lock shows. The ScreenSaver service lets
applications keep the screen awake and ask to lock. Leaving the session
(#72) asks logind what the machine can do (hibernating is offered only
where it works) and which applications block shutting down: their reasons
stop the ten-second countdown until the user decides; the session is then
saved, windows are asked to close, and logind restarts or powers off. The
session is polkit's authentication agent (#83): a request shows a dialog
drawn by the compositor over every window, with the keyboard and the
system's label, so that no client can cover it or imitate it; the password
goes to polkit's own helper (its socket, or the setuid binary of older
versions), which tells polkit. The tray (#73) serves the
StatusNotifierWatcher on the session bus and reads each application's
title, status and icon (its pixels, or a PNG or SVG of the icon themes) when
it says they changed, and its dbusmenu menu when the user asks for it; an
application slow to answer holds only its own thread. Each source runs on a
thread of its own; a missing one leaves its part of the state as it was.
Only the session itself uses the machine's services: nested or headless,
quick settings show a stub and change nothing on the machine, and tests give
private buses with services of their own.

## Security

What the session holds (an unlocked desktop, the clipboard, the screen) is
guarded by these rules (#134):

- **Locked means nothing else.** While locked, each screen's scene is the
  lock alone: no window is drawn, on a screen plugged meanwhile either, and
  captures (screencopy, ext-image-copy-capture, the control API) go through
  the same scene. A lock client that dies leaves our own lock.
- **A crash never unlocks.** While locked, `locked` in the state directory
  says so; a session that starts and finds it (the compositor crashed or was
  killed while locked, and an autologin started a new one) starts locked.
- **Nothing listens on the network.** The Wayland and control sockets are
  Unix sockets in `XDG_RUNTIME_DIR`. The control socket is mode 0600, and a
  connection whose peer credentials (SO_PEERCRED, getpeereid) are not the
  compositor's own user is closed unread. Injection and scene dumps need the
  test permission, given only on the command line.
- **Agents and AI (#42)** speak over Unix sockets only, with the same peer
  check and permissions per connection; never TCP. An agent that names
  itself is held to its `[[agents.mcp]]` entry by the compositor, not by
  its own client: read-only unless it says `act`, only on the app ids it
  lists, never while locked. It acts through a seat of its own, never the
  user's keyboard; its actions in the applications of `[agents]
  confirm_apps` and its typing into a field said to be a password wait for
  the user, on a card the user's pointer answers (never a key, which a
  user typing elsewhere could press by mistake). The name scopes a cooperating agent; it is no
  secret, as any program of the user's could edit the configuration too.
- **The phone (#85)** goes through KDE Connect, which pairs (a code
  confirmed on both screens) and talks over TLS itself; the session only
  uses phones already paired there, on the session's own bus. Wireless
  adb (#153) is off by default (`[phone] adb`), since a paired phone can be
  debugged from this computer; it pairs the same way, with a password the
  QR code carries and both sides prove over SPAKE2 and TLS (adb's own),
  never sent in clear.
- **Calendar passwords (#75)** live in the keyring, never in the
  configuration, and travel only over https (plain http is refused but to
  this machine); the cache of events is sealed. Google's access tokens
  (#137) are GNOME Online Accounts': asked for each sync, never stored
  nor logged, and sent only to the origin of Google's CalDAV endpoint,
  whatever its answers point at.
- **Search never reads secrets (#45).** The document indexer never opens
  `~/.ssh`, `~/.gnupg`, keyrings, password stores, cloud credentials,
  browser profiles, hidden folders named for secrets, `.env*`, `id_*` or
  key files, whatever the configuration names; the list is in the code
  (`hocus-platform::search::deny`), not in the configuration. Its index is
  readable by the user only. Vectors of documents go to a model on this
  machine unless `[search] meaning = "on"`.
- **Parsers are fuzzed** on every build (`robustness` tests): the
  configuration, the session file, shortcuts and actions, the control API's
  lines through the real server, and the document search's readers (PDF,
  ZIP, office XML, HTML) and index file.

## Testing

A Debian testing VM (`scripts/vm/`) runs hocuspanda as the session on its
tty1 (virtio-gpu, Mesa's software GL), with keys and mouse motion sent
through QEMU and its screen read back, against real logind, PAM, UPower,
NetworkManager, power-profiles-daemon and PipeWire; it switches terminals,
suspends and wakes.

- **Unit tests** on `hocus-core` and `hocus-ui`: layout, snapping, desktops,
  springs, text layout.
- **Scenarios**: files that describe steps (spawn a test client, move the
  pointer, press keys, plug an output, advance virtual time) and
  expectations (scene state, focus, positions, screenshot within tolerance).
  They run headless on a virtual clock, so animations are deterministic.
  Nothing then depends on real time: the timers that wake the loop when
  something falls due are not set (what is due is seen as the clock
  advances), and a moment starts from the compositor's own events (a frame
  served, a key handled), not from when a client's thread got its request
  in (#212). Nor does a step wait a fixed amount of real time for a client
  or a worker: it waits for the event that says it is done (a client drew
  the size its new state asks, a test client's picture with its marks is
  in, an activation or a worker's answer reached the loop, logind's signals
  are followed), and the clock never moves past a wait that depends on an
  application before it answered: an agent's capture or typing pings the
  application (`xdg_wm_base.ping`), and the harness advances only once it
  answered, having drawn or said what its field is for (#235).
  Each runs on its own compositor in-process, through the control API;
  the format is described in `tests/scenarios/README.md`.
- **Test clients**: a small in-repo client that draws known colours and
  reports what it receives, plus real ones (foot, Firefox) for smoke tests.
- **Reference screenshots** reviewed in pull requests.
- **Performance**: frame-time histograms per output and slow handlers,
  exposed as `stats` over the API (`hocus msg stats`); scenarios can assert
  budgets. Timings in wall-clock milliseconds (the Matrix theme's GPU
  passes) measure the machine's load as much as the code alongside the
  rest of the suite: they are ignored there and run alone,
  `cargo nextest run -p hocus-compositor --run-ignored only the_refinements_cost`
  (`--release` for the release budget).
- **VMs**: QEMU scripts for the DRM session, suspend/resume, several outputs,
  and FreeBSD.

## Licenses

MIT for the code. Dependencies must be permissive (MIT, Apache-2.0, BSD,
ISC, Zlib, Unicode, CC0; OFL for fonts), checked by `cargo deny` in CI;
data shipped in the tree too (Natural Earth's land outline and tzdata's
`zone1970.tab` copy for the tests are in the public domain). No
code copied or translated from GPL projects: niri and cosmic-comp (both
smithay-based) are GPL-3.0, as are KWin and Mutter. Read their documentation
and design discussions, not their code. smithay and its `anvil` example are
MIT and can be used freely; sway and wlroots (MIT) and Hyprland (BSD-3) are
permissive but written in other languages.
