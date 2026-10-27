# Scenarios

Each `.toml` file here is a scenario: steps and expectations run against its
own headless compositor (pixman renderer, virtual clock, private runtime
directory), through the control API. `cargo nextest run` runs them all, one
test per file; `hocus scenario run FILE…` runs some. Keyboard layouts are
named, in the shell and the settings, after `xkb/` here (a part of XKB's
registry and its French catalogue, made by `scripts/xkb-excerpt.py`), never
the machine's; their keymaps are XKB's own.

```toml
description = "A click on a window behind another focuses it and raises it"
# WIDTHxHEIGHT[@SCALE][+X+Y][/DESKTOPS][:WIDTHxHEIGHTmm]; default: one 1280x800
# output with 4 desktops. A size in millimetres, as a screen's EDID gives it,
# makes the scale the one of its density unless @SCALE is given; without one,
# the scale is @SCALE or 1
outputs = ["2560x1600@2/1", "1920x1080"]
# Starts XWayland for `open_x11`; the scenario is skipped where Xwayland is missing
xwayland = false
# Shows the shell's panel on the primary output (in English, at a fixed
# Wednesday 21 October 14:37 UTC plus the virtual time, for user "Alex";
# UTC is the system's time zone there, unless a timedated stand-in says another):
# the strip by default; a scenario about the column itself writes
# [panel] form = "column" in its first step and in each configuration it writes
shell = false
# A private D-Bus bus for the compositor (notifications); never the session's
dbus = false
# Stand-ins for UPower (a battery at 80 % on its charger), NetworkManager
# (Wi-Fi on, joined to its saved "Bamboo grove"; "Panda den" and "Tea house"
# in reach with the password "bamboo shoots", "Café du Parc" open, "Office"
# an account's, one hidden), BlueZ (on; "Bamboo headphones" paired and
# connected, "Panda mouse" paired; while discovering, "Panda keyboard",
# which shows the passkey 123456 to confirm, and "Speaker by the window",
# which just pairs) and power profiles (balanced), on a private system bus
# the compositor and the settings follow; never the machine's. The sound is
# kept in memory then, never the machine's
devices = false
# Stand-ins for AccountsService ("Alex Panda", no picture) and fprintd (a
# reader, one finger enrolled), on a private system bus the settings ask;
# never the machine's
accounts = false
# A stand-in for timedated in this time zone, on a private system bus the
# compositor and the settings follow; never the machine's. Without it the
# settings cannot change the time zone. Their world map places the cities
# after zoneinfo/zone1970.tab here (tzdata's, public domain), never the
# machine's: it draws the same everywhere
timedated = "UTC"
# A stand-in for GNOME Online Accounts on the scenario's private session
# bus (dbus implied; never the session's) with a Google account signed in,
# its calendar on, and one for Google's CalDAV on 127.0.0.1, at Google's
# paths, serving the .ics files of dir as its calendar; it takes only the
# token GOA handed out last. With running = false, GOA's name is taken only
# at a `goa = "run"` step: until then, it is not installed
goa = { identity = "alex.panda@example.com", dir = "calendar/google", running = true }
# Windows move as in a session, to sample motion at a virtual time;
# otherwise they appear and go at once
motion = false
# pixman (the same pixels everywhere), or gles on a render node, for
# shaders: skipped where there is none, and no screenshots (the GPU's pixels)
renderer = "pixman"
# Programs the steps run: the scenario is skipped where one is missing
needs = ["notify-send"]
# Where the launcher reads desktop entries, relative to the scenario
data_dirs = ["../fixtures"]
# The system's configuration directories the settings read (autostart
# entries, mimeapps.list), relative to the scenario, after the user's own:
# a private one, which they write
config_dirs = ["../fixtures/system/config"]
# What started applications inherit instead of the test's environment
# (PATH is added when missing; {scenario_dir} for stand-in programs)
child_env = { LD_PRELOAD = "/tmp/not-a-library.so" }
# A home of its own: this folder (relative to the scenario) copied into a
# fresh temporary directory, {home} in configuration, `type` and `unreadable`
# steps; documents are searched there, never in the developer's, and it is
# the settings app's home
home = "../fixtures/search-home"

[[step]]
open = { app_id = "back", size = [400, 300], color = "#3399ff" }
[[step]]
pointer_move = [450, 260]
[[step]]
click = "left"
[[step]]
expect_focus = "back"
```

Steps, one per `[[step]]` table:

| Step | Does |
|---|---|
| `open = { app_id, size = [w, h], color = "#rrggbb", title?, min_size?, inhibit_shortcuts?, inhibit_idle?, password?, marks?, cursor?, accessible? }` | starts a test client with a solid window (titled `title`, untitled by default), waits until it is shown; it says it takes no less than `min_size = [w, h]` (its size by default), and the compositor never asks it less; `inhibit_shortcuts` holds the compositor's shortcuts back while it has the keyboard; `inhibit_idle` keeps the session awake while it is shown, as a video player does; `password` makes its field a password one, as its text inputs say (text-input-v3) when a keyboard comes in; `marks` draws a white square for each key and button pressed, at once when its last frame was shown, else at its next frame callback, as applications pace their drawing; `cursor = "text"` sets that cursor shape (cursor-shape-v1: `default`, `text`, `pointer`, `crosshair`, `wait`, `help`) each time the pointer comes onto its window; `accessible = "compose"` puts its application on a private accessibility bus the compositor reads (with a stand-in registry, made at the first), with a compose window's tree (a subject, a password, Send, Send later, a disabled Delete, its frame named as the title), its elements drawn where the tree says, and a button pressed over one is received as `hit LABEL`. It takes every seat, an agent's too |
| `open_layer = { namespace, layer, anchor?, size = [w, h], exclusive_zone?, keyboard?, color }` | starts a layer-shell client (`background`, `bottom`, `top`, `overlay`; anchors among `top`, `bottom`, `left`, `right`; 0 for a length anchored at both ends; keyboard `none`, `exclusive`, `on_demand`), waits until it is shown; `expect_received` reads it by its namespace |
| `close_layer = "namespace"` | closes that layer client |
| `session_file = """…"""` | writes the saved session file (TOML, `[[window]]` with `app_id`, `title`, `command`, `output`, `desktop`, `geometry`, `state`, `pinned`); `{test_client}` is the test client program (`--app-id`, `--size`, `--color`, `--title`) |
| `save_session = true`, `offer_session = true` | saves the session now, waiting for the file to be written; reads the saved one again and offers to put it back, as at start, waiting for the read |
| `expect_session_file = { contains?, lacks? }` | the saved session file holds these strings, or lacks them |
| `lock_client = "#rrggbb"` | a lock client locks the session (ext-session-lock) with a surface of that colour on every output, waits until it is locked |
| `unlock_client = true`, `crash_lock_client = true` | the lock client unlocks, or goes away without unlocking as a crash would |
| `share = true`, `stop_sharing = true` | a screen sharing tool starts copying the first output again and again (wlr-screencopy with damage), then stops |
| `expect_sharing_ended = true` | the compositor ended the sharing: the tool got no more frames |
| `ask_share = true` | a screen-cast portal's chooser asks what to share (as `hocus share-pick`), and waits |
| `expect_shared = "Monitor: HEADLESS-1"` | it was answered this; `""` for nothing |
| `activate = "app_id"` | the client of that app id asks for its window to come forward, with an activation token of its own (xdg-activation) |
| `ask_attention = "app_id"` | the same with a token it took eleven seconds of virtual time before: too old to bring its window forward, the window asks for attention instead |
| `activate_without_serial = "app_id"` | the same with a token made without a serial, from no input of the user's: the window asks for attention instead |
| `activate_with = { app_id, token_from = "launched-env" }` | the same with the token a program the compositor started was given (an `XDG_ACTIVATION_TOKEN=` line of that runtime file), as a new instance hands it to the one running |
| `open_x11 = { app_id, size = [w, h], color = "#rrggbb" }` | the same with an X11 client through XWayland (WM_CLASS is the app id) |
| `x11_urgent = { app_id, urgent = true }` | that X11 client sets its urgency hint (WM_HINTS), or takes it back |
| `copy = { app_id, text }` | the client of that app id, Wayland or X11, takes the clipboard with `text` |
| `expect_clipboard = { app_id, text }` | the client of that app id reads `text` from the clipboard (a Wayland client is offered it when it gets the keyboard) |
| `type = "fir"` | types text as on a US keyboard |
| `expect_runtime_file = { name, lines?, contains?, lacks? }` | a file a started program wrote in the compositor's runtime directory: so many lines, with or without these strings (the test client adds a line to `<app id>.starts` each time it starts) |
| `notify_send = ["--app-name=Mail", "Title", "Body"]` | runs `notify-send` on the private bus |
| `notify = { app, summary, body?, actions? = [["key", "Label"]], urgency?, timeout_ms?, replaces?, hints? = { "x-kde-appname" = "…" } }` | sends a notification on the private bus (urgency `low`, `normal`, `critical`; `timeout_ms = 0` never expires; `hints` of text) |
| `expect_notification_signals = ["ActivationToken 1", "ActionInvoked 1 default"]` | the notification server sent these signals, in this order (others may come between), since the first `notify`: `ActivationToken ID` (`… empty` for an empty token), `ActionInvoked ID KEY`, `NotificationClosed ID REASON` |
| `phone = { name, battery, charging? }` | KDE Connect, a stand-in on the private bus, with one phone paired and within reach (its files at `{runtime_dir}/phone-files`); again, its battery changes |
| `phone_notification = { app, title, text, reply? = true }` | the phone posts a notification; `reply` when its application takes replies (the reply ids are `r1`, `r2`… in order) |
| `phone_remove = 1` | the notification it posted first (from 1) goes on the phone |
| `expect_phone = "reply r1 On my way"` | the phone was asked this: `ring`, `reply ID TEXT`, `clipboard TEXT`, `share URL`, `browse` |
| `expect_phone_state = "absent"` | KDE Connect as the session found it, once so: `absent`, `stopped`, `running`, `off` |
| `scan_pairing_code = true` | the phone scans the pairing QR code the popover shows (wireless adb): its name and password go to the adb stand-in, as `NAME PASSWORD` in `{runtime_dir}/adb-phone` |
| `expect_adb = "ready"` | wireless adb as the session has it, once so, within seconds: `off`, `absent`, `ready`, `pairing` |
| `mcp = { agent?, tool, window?, arguments? = { … }, contains? = […], error?, wait?, image? }` | an MCP session (`hocus mcp`, #42) of its own, as agent `agent` of `[[agents.mcp]]` (or none), or that agent's lasting one when `mcp_session` opened it, calls `tool` with `arguments`; `window` is an app id whose window's id goes in as the `window` argument; the result's text holds `contains`, or the tool fails with `error` in its message. While it waits, an action shown before it lands (#43) moves the clock to when it does; one waiting for the user's confirmation needs `wait = false`: the step goes on once the action is in, and `expect_mcp` checks its result. `image` compares the result's image (a capture) with a reference next to the scenario, taken again for a moment until it matches, as the application draws on its own time. A capture that waits for its window to draw (one no screen shows) moves the clock to the end of that wait once the application answered a ping sent after its frame callback; so does any clock step while an agent's typing waits for the application to say what its field is for |
| `mcp_session = "helper"`, `mcp_end = "helper"` | an MCP session as that agent that lasts until `mcp_end` or the end (its seat stays meanwhile), named at once; the step goes on once every window's application took its seat |
| `expect_mcp = { agent?, contains? = […], error? }` | the result of the oldest `wait = false` call (that agent's), once it comes |
| `expect_journal = { has? = […], lacks? = […], stopped? }` | the agents' journal (#176), as `hocus msg agents-journal` prints it, holds the strings of `has` in that order (each after the one before, on its line or a later one) and none of `lacks`; agents are stopped or not |
| `click_ui = "Close"` | clicks the shell's button with this label or accessible name (`hocus msg ui` lists them); without one, the middle of any element so labelled (a card's text) |
| `right_click_ui = "Chat"` | the same with the secondary button |
| `tray_app = { title, color = "#rrggbb", menu? = ["Open", "-", "Quit"], delay_ms? }` | an application registers in the tray on the private bus, with a disc of that colour as its icon and that menu (`"-"` a separator); it takes `delay_ms` to answer anything |
| `tray_attention = { title, attention }` | it asks for attention, or stops |
| `expect_tray = { title, asked = "clicked 2" }` | it was asked this: `token` (an activation token given), `activate`, `clicked N` (menu entry N, from 1) |
| `open_settings = { page? }` | starts the settings app in the test, on a page (`appearance`, `wallpaper`, `column`, `pandas`, `accessibility`, `language`, `desktops`, `windows`, `keyboard`, `mouse`, `captures`, `clipboard`, `screens`, `sound`, `network`, `bluetooth`, `power`, `lock`, `notifications`, `apps`, `accounts`, `account`…) or its home, and waits for its window |
| `expect_settings = { label, focused?, selected?, shown? }` | the settings window shows an element with this label: with the keyboard or not, on (a switch, a choice) or not; `shown = false` checks it does not |
| `click_settings = "Accent Amber"` | clicks the settings window's element with this label, through the compositor |
| `click_settings_at = { label = "World map of the time zones", at = [0.89, 0.34] }` | clicks it elsewhere than its middle: `at` in fractions of its width and height from its corner |
| `drag_settings = { label, by = [dx, dy] }` | presses on the settings window's element with this label, moves by so much in steps, and lets go |
| `expect_budget = { max_ms = 50 }` | every frame drawn so far took at most that long, and no loop handler went over its budget |
| `measure = true` | starts counting what the compositor does (#88): frames drawn and scenes built per output, allocations on its loop, memory it holds |
| `expect_cost = { frames? = [n, …], renders? = [n, …], allocations?, allocated_mb?, held_mb? }` | what it did since `measure` or the last `expect_cost` is within these budgets, each at most: frames drawn per output, scenes built per output (drawn or not), allocations on its loop, megabytes asked for, megabytes more held; counting starts again. Counts, not times: the same on every machine, so a regression fails. Allocations are counted where the counting allocator is installed (the scenario tests and `hocus scenario`); what was counted is printed, to set budgets from ([performance](../../docs/performance.md)) |
| `expect_state_file = { name, contains? = […], lacks? = […] }` | a file of the compositor's state directory (notes, the saved session) holds these strings, once written; for a folder (`tasks`), one of its files (or its folders') holds `contains` and none holds `lacks` |
| `state_file = { name, text }` | writes a file of the compositor's state directory (`tasks/quote.ics`, `notes.md`), its folders made; the compositor reads it as it reads that file again, or after `restart` |
| `restart = true` | the compositor goes as a crash would, and a new one starts on the same configuration and state; clients go with it |
| `open_trim = "{last_recording}"` | opens the recording editor on a file (the last recording saved), in-process, and waits for it; `expect_trim`, `click_trim`, `drag_trim` drive it as the annotation window's steps do |
| `expect_export = { name_has, length? = [from, to], max_bytes? }` | a file the editor exported beside the last recording, once the editor names it: its length in seconds (ffprobe) and its size |
| `swipe = { fingers, by = [dx, dy], begin? = true, release? = true }` | a touchpad swipe on the screen under the pointer, moved in eight steps a frame apart; `begin = false` goes on with the one held |
| `expect_idle = { for_ms }` | no output draws a frame while this much virtual time passes: at rest, nothing is redrawn |
| `expect_frames = { for_ms, at_least }` | every output draws at least that many frames while this much virtual time passes: something moves |
| `expect_blur = { at_least?, at_most? }` | the blur behind panels and windows (#37) was computed so many times since the last `expect_blur`, or the start: only when what is behind changes (with `renderer = "gles"`; never with pixman) |
| `expect_glitch = { at_least?, at_most? }` | frames of a closing window's glitch in the Matrix theme painted with its colours apart on the GPU (#169), so many since the last `expect_glitch`, or the start (with `renderer = "gles"`; never with pixman or reduced motion) |
| `expect_recording = { audio?, keys_at_ms? }` | the last recording saved, read by ffprobe and ffmpeg once done: with a sound track as long as the picture (or none); keys drawn near the bottom from this time in the video, the top unchanged |
| `caldav = { dir, user, password }` | a CalDAV server on 127.0.0.1 serves the `.ics` files of `dir` (relative to the scenario); its calendar's address is `{caldav}` in later configuration, `type` and `calendar_password` steps |
| `caldav_stop = true` | the CalDAV server goes, as the network would |
| `herdr = { name?, agents = [{ name, status }] }` | a herdr of the test's own (never the user's) serves these agents (`working`, `blocked`, `done`, `idle`) on a socket in the compositor's runtime directory, `{herdr}` in later configuration; several have names, `{herdr:NAME}` |
| `herdr_status = { agent, status, label? }` | an agent's status changes in the herdr serving it (agents' names are unique in a scenario) once the compositor follows that agent, with what its integration says (the question, while blocked) |
| `expect_herdr = "agent.send_keys refacto enter"` | a herdr was asked this (`agent.focus NAME`, `agent.send_keys NAME KEYS`, `agent.prompt NAME TEXT`) |
| `ai_server = { pieces = […], delay_ms?, first_ms? }` | an AI provider on 127.0.0.1 (ollama's API, OpenAI's chat completions, Claude's Messages API, which refuses without a key; `/v1/models` lists `claude-sonnet-9`, `claude-opus-9-1`, `claude-opus-9`, `claude-haiku-9`, newest first) answers every question with these pieces, `delay_ms` before each and `first_ms` more before the first; it makes embeddings too (`/api/embed`, listing `test-embed`, and `/v1/embeddings`): a word counts in one of 64 dimensions, and "invoice", "facture", "bill"… share one, as "holiday" and "vacances", "car" and "voiture" do; its address is `{ai}` in later configuration and `type` steps |
| `ai_key = { provider, key }` | an AI provider's key is kept, as `hocus msg ai-key` does (in a sealed file of the state directory: never the developer's keyring) |
| `expect_search = { state?, documents?, embedded?, never_read?, unreadable?, meaning? }` | the document search's indexer (#45), once so, within 30 s of real time (it works on threads of its own): `off`, `waiting`, `paused` (on battery), `indexing` or `ready`; documents indexed, those with vectors, entries left out as never read (not opened) in its last look, entries that failed to read, whether search by meaning works |
| `unreadable = "{home}/Documents/.ssh"` | makes a file or folder of the scenario's home unreadable (mode 000) until the end: reading it would fail, and count as `unreadable` |
| `expect_ai = { path?, contains? = […], header? }` | the AI provider was asked, once so: on this path, its body holding these strings, with this header (`name: value`, the name in lower case) |
| `calendar_password = { id, password }` | the settings keep this account's (`user@url`) password |
| `expect_calendars = { events?, secrets?, contains? = […], error?, online?, online_accounts? }` | the calendars, once so: events from now on, passwords kept, events by title, whether an account failed its last sync, GNOME Online Accounts (`unknown`, `absent` or `installed`) and its Google accounts with their calendar on |
| `expect_calendar_cache = { contains? = […], lacks? = […] }` | the sealed calendar cache of the state directory, opened, holds these strings and not those (accounts' ids, events' texts) |
| `goa = "remove"` | something changes in GNOME Online Accounts' stand-in: `run` (its name taken), `sign_in`, `remove` (the account), `calendar_off`, `calendar_on`, `revoke` (Google refuses the token GOA holds until GOA renews it), `break_tokens` (Google refuses every token) |
| `expect_goa = ["GetAccessToken", "REPORT /caldav/v2/"]` | GOA's and Google's stand-ins were asked these, in this order, each the start of a line of their log: `GetAccessToken`, `EnsureCredentials`, and Google's `METHOD PATH ok` or `METHOD PATH refused` |
| `wait_wallpapers = true` | waits until the wallpapers are made and drawn, as pictures do: before a capture the compositor takes itself |
| `open_annotate = "{last_screenshot}"` | opens the annotation window on a capture (`{last_screenshot}` is the last one saved, `{runtime_dir}` the compositor's temporary directory), and waits for it |
| `expect_annotate`, `click_annotate`, `drag_annotate` | as the settings steps, in the annotation window (its capture is labelled `Capture`) |
| `expect_timedated = "SetTimezone Asia/Tokyo interactive"` | the timedated stand-in was asked this (`interactive`: polkit may ask) |
| `timedated_zone = "America/New_York"` | the system's zone changes in the stand-in, as if set elsewhere |
| `timedated_refuses = true` | the stand-in refuses to set the zone from now on, as polkit would (`false`: no longer) |
| `charger = false` | the charger is pulled out (`true`: plugged in), as the UPower stand-in says; the step ends once the compositor took it in, with the clock still |
| `expect_devices = "WirelessEnabled false"` | the devices' stand-ins were asked this: `WirelessEnabled BOOL`, `Powered BOOL` (Bluetooth), `ActiveProfile NAME`; NetworkManager's `RequestScan`, `ActivateConnection SSID`, `AddAndActivateConnection SSID KEY-MGMT` (`open`; never the password), `Disconnect`, `Delete SSID`; BlueZ's `StartDiscovery`, `StopDiscovery`, `RegisterAgent CAPABILITY`, `UnregisterAgent`, `Pair NAME` (`Pair NAME rejected`), `Trusted NAME BOOL`, `Connect NAME`, `Disconnect NAME`, `RemoveDevice NAME` |
| `expect_accounts = "SetRealName Alex P."` | the account's stand-ins were asked this: `SetRealName NAME`, `SetIconFile PATH` |
| `expect_xdg_file = { name, contains?, lacks? }` | a file of the user's private configuration directory the settings write (`mimeapps.list`, `autostart/NAME.desktop`) holds these strings, or lacks them |
| `expect_config_file = { contains?, lacks? }` | the configuration file holds these strings, or lacks them (the settings app writes it) |
| `expect_ui = { label, disabled?, selected?, shown?, inside? }` | the shell shows an element with this label (disabled or not, the current choice of a list or not, within `inside = [x, y, w, h]`); `shown = false` checks it does not |
| `close = "app_id"` | asks the window to close, waits until it is gone |
| `pointer_move = [x, y]` | moves the pointer, global logical coordinates |
| `pointer_path = [[x, y], …]` | moves the pointer through these points in turn, as a hand drawing a line |
| `click = "left"` | presses and releases a button (`left`, `right`, `middle`) |
| `press = "left"`, `release = "left"` | holds and lets go a button, for drags |
| `scroll = 15` | scrolls the wheel, logical pixels (15 per notch, down positive) |
| `key_down = "alt"`, `key_up = "alt"` | holds and lets go a key, for Alt+drag |
| `key = 30` | presses and releases a key, by evdev code (`KEY_A` is 30) |
| `keys = "super+shift+2"` | presses keys in order, releases them in reverse: `super`, `shift`, `ctrl`, `alt`, digits, letters (by position on a US keyboard), `enter`, `escape`, `tab`, `space`, `backspace`, `backtick`, `comma`, arrows, or evdev codes |
| `send = { app_id, output, desktop? }` | sends a window to an output: its desktop `desktop` (from 1), or the shown one |
| `plug = { spec = "1920x1080+1280+0", name? }` | plugs an output; `name` brings back one unplugged before |
| `unplug = "HEADLESS-2"` | unplugs an output |
| `prefer_mode = { output, mode = "1920x1200" }` | the output's connector, still connected, now prefers that mode (`WIDTHxHEIGHT`), as a monitor whose EDID changed or a VM's resized window would; the output follows unless a mode chosen for it is still offered |
| `randr = { output, enabled?, mode?, scale?, position? }` | sets a screen through wlr-output-management, as wlr-randr would (`mode` is `WIDTHxHEIGHT`), and checks it succeeded |
| `expect_output = { name, geometry? = [x, y, w, h], pixels? = [w, h], scale?, temperature? = [low, high], shown? }` | checks an output's logical geometry, pixel size, scale and night light (kelvin); `shown = false` checks it is off or gone |
| `stall_output = "HEADLESS-1"` | the output stops presenting its frames, as a stuck screen would, until the compositor recovers it |
| `set_state = { app_id, state = "maximized" }` | maximizes, makes fullscreen, minimizes or restores (`normal`) a window; the step ends once its client drew the size its new state asks, the clock still |
| `expect_accent = { hue = [15, 45] }` | the shell's accent in effect has a hue in that range (degrees) |
| `config = """…"""` | writes the configuration file (TOML) and waits until the compositor has read it; `{scenario_dir}` is the scenario's directory, for fixtures, `{runtime_dir}` the compositor's temporary directory, for files it writes, and `{test_client}` a test client program (`--app-id`, `--size`, `--color`) for what the compositor starts itself |
| `expect_config_error = "line 2"` | checks the last file was refused with that in the message; `""` checks it was applied |
| `expect_config_warning = "first one wins"` | checks what the configuration in effect left out |
| `expect_event = { type = "keyboard_layout", has? = "French" }` | checks the compositor said it in its event stream since the start |
| `advance_ms = 500` | moves the virtual clock forward |
| `frames = 1` | moves the virtual clock forward by whole frames |
| `expect_window = { app_id, title?, output?, geometry? = [x, y, w, h], focused?, visible?, state?, opacity?, pinned?, attention? }` | checks a window (among several of one app id, the one with this title), waiting a moment for its client to resize; `visible` is whether its desktop is shown and it is not minimized; `attention` whether it asks for attention |
| `expect_no_window = "app_id"` | checks a window is gone |
| `expect_focus = "app_id"` | checks which window has keyboard focus |
| `expect_cursor = "e-resize"` | checks the cursor the pointer shows, waiting a moment for a client to set its own: a shape's name as cursor-shape-v1 and CSS say it (`default`, `text`, `move`, `n-resize`, `ne-resize`, `e-resize`, `se-resize`, `s-resize`, `sw-resize`, `w-resize`, `nw-resize`…), `surface` for a client's own image, `hidden` |
| `expect_stack = ["below", "above"]` | checks the shown windows, bottom to top |
| `expect_shown = { output, desktop }` | checks which desktop an output shows, from 1 |
| `expect_received = { app_id, event = "key 30 pressed" }` | checks the client got an event: `configure WxH`, `pointer_enter`, `pointer_leave`, `button CODE pressed`, `keyboard_enter`, `keyboard_leave`, `key CODE pressed`, `close` (and `released`), `keymap LAYOUT, LAYOUT` (the names of the keymap's layouts, as XKB says them: `keymap French (BEPO), English (US)`), `typed TEXT` (what a key pressed types in that keymap, with the modifiers and layout in effect); an MCP agent's seat's events end with its name, `key 30 pressed on agent-helper`, and a `password = true` window says `password_field on agent-helper` once its field told that seat it is a password one; a `marks = true` window says `drew N marks` once the compositor has its picture of N marks, which a step waits for before a capture rather than a moment of real time |
| `expect_not_received = { app_id, event }` | checks the client never got an event |
| `expect_configures = { app_id, at_least = [w, h] }` | checks every size the client was asked, since it started, is at least so large (a 0×0 configure, which leaves the size to it, aside) |
| `expect_pixel = { output, at = [x, y], color = "#rrggbb", tolerance? = 24, is? = true }` | checks one pixel of an output, in its pixels; `is = false` checks it is not that colour |
| `expect_screenshot = { output, reference = "file.png", channel_tolerance? = 2, pixel_tolerance? = 0.0, cursor? = false }` | compares the output with a reference image next to the scenario; `cursor` draws the pointer's cursor too |

Reference images are recorded with `HOCUS_BLESS=1 cargo nextest run -p
hocus-test --test scenarios` (or `hocus scenario run --bless FILE`); look at
them before committing. When a comparison fails, the actual, expected and diff
images are left in `target/test-artifacts/<scenario>/`, which CI keeps as an
artifact.
