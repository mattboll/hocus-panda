# Features

An exhaustive, implementation-neutral list of what the desktop does.
Workarounds (restarting a crashed compositor, a crash-recovery runner) are
not features: their *expected result* is in the quality requirements at the
end.

`[linux]` marks what relies on a Linux-only service and needs a FreeBSD path.

Brainstormed features beyond this list live in [ideas.md](ideas.md).

## 1. Session

- Save and restore the session: applications relaunched in place, desktops and state; several windows of one application each back on its own desktop
- XDG autostart
- Log out, power off, reboot, suspend, hibernate `[linux]`
- Emergency logout, protected, that works even when locked
- Boot animation
- First-run assistant (keyboard, colours, clock)
- A single welcome path

## 2. Window management

- Floating windows, cascade placement on the screen under the pointer
- Work area that reserves the panel
- Move (Alt+drag, the top edge), resize (edges, Alt+right drag); over a window's edges the cursor shows the resize it starts (corners reach 16 px along each edge) or the move, and keeps it while the window moves or resizes
- Drag a maximized window out of maximize, with resistance
- Magnetic screen edges and window edges
- Drag-to-snap with preview: halves, top = maximize, quarters
- Keyboard snapping, re-applied when gaps or screens change
- Inner and outer gaps
- Maximize, minimize, fullscreen (including low-resolution games)
- Windows pinned on every desktop
- Dialogs follow their parent; dim behind modal dialogs
- Show desktop
- Per-desktop tiling: master and stack, swap, grow, float
- Window rules (by app or pattern): floating, desktop, size, maximized, opacity, pinned
- Remembered places per screen layout, re-applied when screens are plugged
- Per-window opacity (Super+wheel)
- Click to focus, recent order, focus-stealing prevention: a window takes the focus with an activation token made from your last key or click, or one the shell made for what you started from it (launcher, dock, notification, tray); otherwise it asks for attention
- Attention halo on windows that request attention
- Drop-down terminal
- Drop-down terminal that slides and has an adjustable size
- Default terminal setting
- Always on top
- A restored window always comes back on a visible screen

## 3. Virtual desktops

- 1 to 8 named desktops
- Direct shortcuts for every desktop
- Previous / next, send a window to a desktop
- Sliding transition; a sent window flies away
- Three-finger swipe, windows follow the fingers
- Desktop switcher with draggable previews
- Desktops named after their applications
- **Per-screen desktops**: each screen has its own and switches alone (keep one screen on a chat app all day, switch another by project)
- Number of desktops per screen; a screen with one desktop never switches (fixed screen)
- Shortcuts, gestures and switcher act on the screen under the pointer
- Per-screen switcher, in that screen's proportions (no more squashed previews of both screens side by side)
- Send a window to the other screen, on its shown desktop or a chosen one
- Unplugged screen: its desktops join a remaining screen and go back when it is plugged again (to try; if annoying, gather its windows on one desktop instead)
- Theme per desktop (colours, accent, wallpaper): you know at a glance where you are
- Named desktops: the switcher's strip, the column, Alt+Tab and the overview show their names
- Activity of the other desktops: a badge on a desktop with an agent waiting, an agent done since you last visited it, or a window asking for attention (strip, column, overview)
- No permanent desktop indicator taking room: the switcher appears when you switch

## 4. Overview, Alt+Tab, corners, gestures

- Window overview (animated grid, titles)
- Keyboard-navigable overview
- Alt+Tab with live previews, icon, title, recent order
- Alt+Tab and overview scoped to the screen under the pointer or to all screens (default: Alt+Tab = active screen, Super+Tab = all)
- Configurable hot corners (overview, launcher, desktop), set in settings
- Super pressed and released alone opens the overview
- Super + drag draws on the screen; the line fades when Super is released
- Super+Shift + drag circles anything to ask about it (keys set in `[circle]`): its text is read (tesseract), then copied, searched, noted, or explained, translated or asked about when the AI is on
- Super + right button over a window opens its radial menu at the pointer (keys and button set in `[radial]`): Maximize up, Pin, Next screen, Desktop… (the screen's desktops), Minimize down, Always on top, Snap (the snap zones) and Close at the top left; a flick picks on release, a slow move lights the action up, a click leaves it to the pointer and the keys (arrows, Enter, Escape); Close is never flicked, only rested on for 300 ms or clicked; the menu stays whole on the screen under the pointer
- Touchpad gestures: 3 fingers for desktops and overview, 4 fingers for the launcher
- Gestures passed to applications: pinch (browser zoom), hold
- Use the touchpad better: pinch in or out for the overview or the radial menu, and find other useful gestures. Must coexist with two-finger pinch in applications (browser zoom): 3 or 4 fingers, a modifier, or pinch on the desktop background
- Panel revealed at the screen edge when a window covers it

## 5. Rendering and effects

- **No title bars by default**: they take room for nothing (Firefox does without). Close, minimize, maximize live elsewhere: widget panel, radial menu at the pointer, gestures, shortcuts. Title bars as an option
- Client- or server-side decorations, depending on the application
- Title bar colours from the theme, fade on focus change
- Soft shadows, as dark as the theme makes them, cast by the light of the hour: from one side in the morning, straight down in the afternoon, from the other side in the evening, darker at night (windows and panels alike)
- Frosted blur behind the panel, menus, launcher, notifications, the dock, the switchers and the system dialogs, computed again only when what is behind changes; solid panels where it cannot blur, with transparency off, high contrast, reduced motion or the power saver
- Blur behind windows (a rule, for see-through ones), rounded corners everywhere
- Wobbly windows while moving
- "Magic lamp" minimize towards the dock
- Open, close (dissolve, Matrix rain) and resize animations
- Screenshot flash that flies away
- Focus mode: everything dims except the active window
- Magnifier up to 16×, following the pointer
- Night light
- Curtain that dims the screen before locking
- Cursor alert when something is waiting
- On-screen pen annotation
- Reduced motion; high contrast
- Wallpapers: image and fill, per screen, dynamic by time of day, animated (Matrix, stars)
- Accent colour taken from the wallpaper
- Complete Matrix theme
- Spring animations everywhere, shader effects reloadable while running
- HDR and colour management

## 6. Screens

- Hot plug without losing anything
- Relative layout (left, right, above, below), saved
- Primary screen; panel on each screen
- Mirroring
- Enable or disable a screen
- Resolution, fractional scale, cursor at the right scale per screen
- Adaptive sync (VRR)
- Per-screen panel margins
- Screens of every GPU (a laptop's second GPU, a DisplayLink dock), drawn by the primary GPU and plugged and unplugged with their GPU
- VR headsets never lit as screens: lent to VR runtimes (drm-lease), not to sandboxed applications

## 7. Input

- Keyboard layouts with their variants (AZERTY, BÉPO, US international…), every one XKB knows, searched in the settings by name in English or French; switched from the panel's keyboard menu
- Num Lock at start; lock keys shared between keyboards
- **Fully free shortcuts**: any combination for any action, no imposed modifier
- Actions parameterised by scope (screen under the pointer or all screens), each with its own shortcut: Alt+Tab for the active screen, Super+Tab for all by default
- Mouse and gestures configurable the same way (Alt+drag, Super+wheel, finger counts…)
- Natural scrolling
- Touch screens, touch drag and drop
- On-screen keyboard
- Input methods (IME)
- Touchpad and mouse settings: tap to click, disable while typing, acceleration, left-handed
- Shortcut to switch layout; XKB options
- Pointer constraints and relative pointer (games, Blender)
- Graphics tablets, gamepads, tablet mode

## 8. Applications and tools compatibility

- X11 applications through XWayland as plain compatibility: no part of the desktop depends on it
- Clipboard, primary selection, drag and drop
- Screenshots and screen or window sharing through the standard protocols (grim, OBS, browsers)
- Window-less clipboard managers
- Fractional scale, presentation-time
- External screen lockers (swaylock, waylock, gtklock)
- Activation and attention requests (xdg-activation, X11 urgency hints)
- Idle inhibit (idle-inhibit, D-Bus ScreenSaver); a fullscreen window keeps the screen awake
- Virtual keyboards and input tools (wtype)
- Sandboxed applications: sensitive protocols hidden (security-context)
- freedesktop notification server, with a per-app rate limit `[linux]` D-Bus
- Portal: screenshots, settings (colour scheme), screen picker
- Full portal: colour picker, file chooser
- Full D-Bus ScreenSaver (GetActive, Lock…)
- Third-party bars, notifications and wallpapers (layer-shell)
- Third-party screen tools (wlr-output-management, output-power, gamma-control)
- Third-party window lists (foreign-toplevel-management), third-party idle detection (ext-idle-notify)
- cursor-shape, keyboard-shortcuts-inhibit, xdg-foreign, drm-lease, tearing-control, content-type
- alpha-modifier, xdg-dialog, xdg-toplevel-icon, xdg-system-bell, linux-drm-syncobj, single-pixel-buffer, virtual-pointer

## 9. Dock

- **Discreet dock**: hidden while you work, shown on demand (screen edge, shortcut) or when the desktop is empty, after a restart for example
- Pinned icons, reordered by drag and drop
- An Applications button at its end, after a line (a square of three by three dots): it opens the launcher on every application
- Open applications, windows grouped by app (optional)
- Hover zoom with spring and bounce
- Icon animations: hover, launch bounce, "open" dot, shake and breathe when a window waits (its dot, and the hidden dock's hint, in the accent)
- Window previews on hover
- Icon menu: the app's windows, pin, unpin
- Updated when applications are installed (XDG, Flatpak, Snap)
- Frosted glass

## 10. Widget panel

A thin strip on the right of the screen by default, which opens into the
whole column over the windows; the column always there, a bar at the top or
nothing at all are options (see
[decision 0011](decisions/0011-panel-strip-by-default.md), which supersedes
[0007](decisions/0007-widget-column-right.md)).

- Four forms, `[panel] form`, on the settings' Appearance page and live:
  the strip (72 px: the time stacked, the next meeting's ring, a panda per
  active agent, the to-dos and notifications counted beside a tick and a
  bell, the desktops, the battery, the account), the column (320 px), a
  top bar (one line: the active window and its buttons, the time and the
  date, the agents, the tasks) and hidden (a faint hint at the right
  edge); windows leave room for the strip, the column or the bar only
- The whole column slides over the windows from the right edge: the pointer
  resting on the strip or at the right edge, a click on the strip or the
  bar, or Super+C; it slides back when the pointer leaves it, on Escape,
  Super+C again or a click elsewhere; at once with reduced motion

- Clock 12/24 h, seconds, follows the time zone
- Date; calendar on touch (month, events, today's agenda)
- Notifications with Clear all in their header, and a close button on the
  row under the pointer for one; each one cleared and still open is closed
  for its application as dismissed by the user (`NotificationClosed`,
  reason 2); animated bell with badge
- "Waiting for you": windows that request attention
- Command field ("Search, act, ask…")
- Status modules
- Account: avatar, menu (log out, lock, about, settings, recent apps)
- Power dialog
- Active window controls (minimize, maximize, close), since there are no title bars
- Desktops of each screen, compact
- Media player (MPRIS)
- Developer widgets

## 11. Quick settings (Super+A)

- Volume, audio outputs and inputs, per-app mixer `[linux]`
- Brightness `[linux]`
- Brightness of each screen, external monitors included (DDC/CI)
- Night light
- Wi-Fi: toggle, pick a network, password `[linux]`
- Bluetooth `[linux]`
- Do not disturb
- Screen recording and its options
- Notifications
- Power profile (power saver, balanced, performance) `[linux]`
- Focus mode, airplane mode, pandas shown or hidden

## 12. Launcher and command palette

- Launcher: fuzzy search, cascading results; at the pointer, centred or in the command field
- Every application, with the field empty: a grid in alphabetical order, each with its icon from the icon theme (or its glyph on its colour, as the dock draws it), hidden entries left out (`NoDisplay`, `Hidden`, other desktops'); the arrows, Tab, Page Up and Down and the wheel move, Enter or a click opens, typing searches; entries and icons are read off the loop, each icon once at a size
- Applications and their actions
- Intents in French and English, with confirmation ("screenshot of the browser in 3 seconds")
- Documents (words, then semantic search)
- Calculator, unit conversion, large text, QR code, URL, web search
- "Ask the AI"
- Keywords: volume, brightness, emoji
- Action palette (Super+P): every action by category with its shortcut and scope, searchable by name or by pressing keys; Enter runs it
- Desktop windows (settings, etc.) always open in front

## 13. Notifications

- Bubbles top right, newest first, slide in, countdown paused on hover
- Icon, click opens the app, action buttons, urgency, replacement
- History; transient notifications not kept; one held there and never
  shown (do not disturb) stays open for its application until cleared, or
  expires when the history lets it go
- Held during screen sharing, then summarised
- Matrix style

## 14. Screenshots, recording, sharing

- Screenshots: screen, all screens, window, area (or click a window), delayed; copied and saved
- Screenshot notification: preview, annotate, copy text, delete
- Annotator: arrow, box, text, pixelate, colours, undo
- Text from an area (OCR)
- Screen recording: area or window, microphone, sound, round webcam, clicks and keys, pause, GPU encoding, mp4/webm
- Recording editor: trim, export MP4/WebM/GIF, target size
- Share a window without the menus and notifications above it
- Remote desktop

## 15. Clipboard, emoji, notes

- Encrypted, persistent clipboard history with a manager
- Paste from history
- Copy files
- Emoji picker: recent, groups, search
- Emoji inserted directly into the application; recent Unicode emoji
- Notes: window, checkboxes, preview in the panel, quick edit

## 16. Status modules

- Sound, with OSD and media keys `[linux]`
- Brightness, with OSD `[linux]`
- Animated battery, critical-level action `[linux]`
- Network, airplane mode `[linux]`
- Keyboard layout
- Power profile `[linux]`
- Next meeting (filling ring, Join); today's agenda
- Phone: KDE Connect (battery, ring, notifications with reply, clipboard, send and browse files), wireless adb paired by a QR code, screen mirroring with scrcpy `[linux]`
- System updates `[linux]`
- Notification area (StatusNotifierItem) with menus, ring when the app waits
- Desktop icons (Home, Trash, ~/Desktop)

## 17. Assistant, AI, agents, search

- Chat with the AI: tabs, streamed Markdown, code blocks
- Providers: Claude, OpenAI, local models (ollama, LM Studio, llama.cpp…)
- Keys in the keyring
- The AI acts on the desktop, each action confirmed: an answer proposes, from a fixed set, opening a settings page, changing one configuration key (shown as a diff, the file's comments kept), a request of the command field or handing the question and the answer to a coding agent (journaled); nothing is offered while agents are stopped
- Desktop tools shared between the AI and MCP: windows, desktops, screenshot, launch, notify, file search, agenda
- MCP server (`hocus mcp`): windows, the shell's accessibility tree, window captures, clicks, typing and scrolls aimed at a window or a labelled element, waiting for a state; per-agent rights (read or act, which apps), enforced by the compositor
- Applications' accessibility trees for agents (AT-SPI): a window's roles, labels, places and states, huge trees cut and said partial, never a password field's content; clicks and scrolls aimed at an application's element by label (and role), the ring around the element
- Agent seat: each MCP agent has its own pointer and keyboard (a seat of its own, focus independent from the user's), its pointer drawn in its colour with its name; before a click a ring and its intent in one line; sensitive actions (password managers, the settings, `[agents] confirm_apps`, typing into a password field) wait for the user's confirmation on a card; the column shows its window live with Take over and Hand back; agents work on a desktop no screen shows (their windows keep drawing for their captures, a minimized one comes back there without the keyboard), the card takes the keyboard once clicked (Enter allows, Escape refuses), and the settings' AI and agents page sets what shows first and what asks first
- Agents' journal: what each MCP agent did (when, which action, on which window, refused or not; typed text only on request), in the column, `hocus msg agents-journal` and the `journal` tool; one key (Super+Shift+Escape) or the column's button stops every agent at once until allowed again
- Offline intents
- Document search: text, HTML, PDF, office files, OCR of screenshots; words + meaning; secrets excluded (never index `~/.ssh`, keyrings, password stores…); paused on battery; found under "Documents" in the command field (`[search]`, Settings → Search, `hocus msg search`, `search-again`)
- AI answers inside the command field rather than in a separate window
- Circle to ask: a loop drawn around anything on screen (Super+Shift + drag) offers Explain, Search, Translate, Copy the text, Note and a question of one's own; the answer streams into a card beside it
- Better semantic search in French
- Coding agents (agent managers such as herdr, any Claude Code session): list with a panda per agent, preview, notifications with reply buttons, halo, jump to the agent that waits
- Desktops per project: several herdr, each tied to a desktop; the column shows the current desktop's agents in full and every other desktop's in a line with its counts (working, waiting, done), which goes there

## 18. Calendar

- Multiple accounts, sync, local cache (CalDAV and ICS subscriptions)
- Google, through GNOME Online Accounts when installed `[linux]` (its accounts listed read-only in the settings); an OAuth client of our own later
- Reminders with "Join"
- Month view with week numbers from the clock, today's agenda
- Inline commands
- Microsoft

## 19. Settings

- Home as an icon grid
- Appearance: colour scheme, reduced motion, high contrast, clock, window buttons, font, language, icon theme, computer type
- Wallpaper (including per screen)
- Bundled themes (Nord, Gruvbox, Catppuccin, Tokyo Night, Matrix…), user themes, colours, automatic accent, export
- Dock, desktops, keyboard and shortcut editor, screenshots, window rules, modules, AI
- Account: avatar, password, fingerprint `[linux]`
- Screens: resolution, scale, position, primary, mirroring, enabled, VRR
- Network (the Wi-Fi networks in reach: join with a password, leave, forget) `[linux]`, Bluetooth (devices paired and in reach: pair, with a passkey to confirm, connect, forget) `[linux]`, date and time with world map `[linux]`
- Power: delays, suspend action, fullscreen keeps the screen awake
- Calendar, advanced, updates
- Different delays on battery and on AC
- Search in settings

## 20. Lock and power

- Lock screen: dimmed background, clock, avatar, Caps Lock warning, custom label
- Lock before suspend
- Locked: shortcuts and remote control restricted
- Lock, screen-off and suspend delays; curtain before
- Held by applications (D-Bus, Wayland) and by fullscreen; countdown resumes when the hold ends
- System action authentication (polkit) `[linux]`
- Session menu: lock, log out, suspend, hibernate, restart, shut down; applications can delay with a reason, a countdown can be cancelled

## 21. Themes, languages, accessibility

- Design tokens, named colours, glass, readable contrast
- French and English, switched live
- Screen reader (AT-SPI)
- Tooltips everywhere, polished empty and loading states

## 22. Control and automation

- Control API and event stream (windows, desktops, screens, session, screenshots, recording…)
- Command-line tool
- Control from launcher commands
- Readable configuration file, applied live
- Plugins written in Rust, compiled with the desktop

## 23. Fun

- Christmas lights, flames, sheep, sloth (off by default)
- The pandas (see [identity](design/identity.md))

## 24. Project

- MIT license, permissive dependencies only
- Portable to FreeBSD
- .deb and .rpm packages
- Other packages (FreeBSD, Flatpak for the tools…)

---

## Quality requirements

What the desktop guarantees by design (see [robustness](robustness.md)):

- Never crashes, never freezes, never shows a frozen frame; a slow part (widget, service, application) never slows the screen down
- No slowdown without a cause: every frame is measured, and a drift shows
- Survives suspend, screen hot plug, VT switches, GPU driver hiccups and GPU loss
- A crash never unlocks the session; a crashing lock never leaves a black screen with no way out
- Nothing is misplaced: menus, notifications and windows always on the right screen, at the right place
- Changing the primary screen, a mode or a scale restarts nothing and makes nothing disappear
- Desktop time never depends on wall-clock time (suspend, DST changes)
- Launched applications never inherit dangerous variables (LD_PRELOAD…)

## Development requirements

- Test without restarting the session: hot reload, desktop nested in a window
- Scenarios replayed automatically without a screen, with compared screenshots
- Drivable test VMs (Linux, FreeBSD) for hardware, suspend and multiple screens
- Continuous measurements: frame time, latencies
- Floating window model with snapping and remembered places
