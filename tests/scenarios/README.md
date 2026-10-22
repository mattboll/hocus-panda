# Scenarios

Each `.toml` file here is a scenario: steps and expectations run against its
own headless compositor (pixman renderer, virtual clock, private runtime
directory), through the control API. `cargo nextest run` runs them all, one
test per file; `hocus scenario run FILE…` runs some.

```toml
description = "A click on a window behind another focuses it and raises it"
# WIDTHxHEIGHT[@SCALE][+X+Y][/DESKTOPS]; default: one 1280x800 output with 4 desktops
outputs = ["2560x1600@2/1", "1920x1080"]
# Starts XWayland for `open_x11`; the scenario is skipped where Xwayland is missing
xwayland = false
# Shows the widget column on the primary output (in English, at a fixed
# Wednesday 21 October 14:37 plus the virtual time, for user "Alex")
shell = false
# A private D-Bus bus for the compositor (notifications); never the session's
dbus = false
# Windows move as in a session, to sample motion at a virtual time;
# otherwise they appear and go at once
motion = false
# Programs the steps run: the scenario is skipped where one is missing
needs = ["notify-send"]
# Where the launcher reads desktop entries, relative to the scenario
data_dirs = ["../fixtures"]
# What started applications inherit instead of the test's environment
# (PATH is added when missing; {scenario_dir} for stand-in programs)
child_env = { LD_PRELOAD = "/tmp/not-a-library.so" }

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
| `open = { app_id, size = [w, h], color = "#rrggbb", inhibit_shortcuts?, inhibit_idle? }` | starts a test client with a solid window, waits until it is shown; `inhibit_shortcuts` holds the compositor's shortcuts back while it has the keyboard; `inhibit_idle` keeps the session awake while it is shown, as a video player does |
| `open_layer = { namespace, layer, anchor?, size = [w, h], exclusive_zone?, keyboard?, color }` | starts a layer-shell client (`background`, `bottom`, `top`, `overlay`; anchors among `top`, `bottom`, `left`, `right`; 0 for a length anchored at both ends; keyboard `none`, `exclusive`, `on_demand`), waits until it is shown; `expect_received` reads it by its namespace |
| `close_layer = "namespace"` | closes that layer client |
| `session_file = """…"""` | writes the saved session file (TOML, `[[window]]` with `app_id`, `title`, `command`, `output`, `desktop`, `geometry`, `state`, `pinned`); `{test_client}` is the test client program |
| `save_session = true`, `offer_session = true` | saves the session now; offers to put the saved one back, as at start |
| `expect_session_file = { contains?, lacks? }` | the saved session file holds these strings, or lacks them |
| `lock_client = "#rrggbb"` | a lock client locks the session (ext-session-lock) with a surface of that colour on every output, waits until it is locked |
| `unlock_client = true`, `crash_lock_client = true` | the lock client unlocks, or goes away without unlocking as a crash would |
| `share = true`, `stop_sharing = true` | a screen sharing tool starts copying the first output again and again (wlr-screencopy with damage), then stops |
| `expect_sharing_ended = true` | the compositor ended the sharing: the tool got no more frames |
| `ask_share = true` | a screen-cast portal's chooser asks what to share (as `hocus share-pick`), and waits |
| `expect_shared = "Monitor: HEADLESS-1"` | it was answered this; `""` for nothing |
| `activate = "app_id"` | the client of that app id asks for its window to come forward, with an activation token of its own (xdg-activation) |
| `open_x11 = { app_id, size = [w, h], color = "#rrggbb" }` | the same with an X11 client through XWayland (WM_CLASS is the app id) |
| `copy = { app_id, text }` | the client of that app id, Wayland or X11, takes the clipboard with `text` |
| `expect_clipboard = { app_id, text }` | the client of that app id reads `text` from the clipboard (a Wayland client is offered it when it gets the keyboard) |
| `type = "fir"` | types text as on a US keyboard |
| `expect_runtime_file = { name, lines?, contains?, lacks? }` | a file a started program wrote in the compositor's runtime directory: so many lines, with or without these strings (the test client adds a line to `<app id>.starts` each time it starts) |
| `notify_send = ["--app-name=Mail", "Title", "Body"]` | runs `notify-send` on the private bus |
| `notify = { app, summary, body?, actions? = [["key", "Label"]], urgency?, timeout_ms?, replaces?, hints? = { "x-kde-appname" = "…" } }` | sends a notification on the private bus (urgency `low`, `normal`, `critical`; `timeout_ms = 0` never expires; `hints` of text) |
| `phone = { name, battery, charging? }` | KDE Connect, a stand-in on the private bus, with one phone paired and within reach (its files at `{runtime_dir}/phone-files`); again, its battery changes |
| `phone_notification = { app, title, text, reply? = true }` | the phone posts a notification; `reply` when its application takes replies (the reply ids are `r1`, `r2`… in order) |
| `phone_remove = 1` | the notification it posted first (from 1) goes on the phone |
| `expect_phone = "reply r1 On my way"` | the phone was asked this: `ring`, `reply ID TEXT`, `clipboard TEXT`, `share URL`, `browse` |
| `expect_phone_state = "absent"` | KDE Connect as the session found it, once so: `absent`, `stopped`, `running`, `off` |
| `click_ui = "Close"` | clicks the shell's button with this label or accessible name (`hocus msg ui` lists them) |
| `right_click_ui = "Chat"` | the same with the secondary button |
| `tray_app = { title, color = "#rrggbb", menu? = ["Open", "-", "Quit"], delay_ms? }` | an application registers in the tray on the private bus, with a disc of that colour as its icon and that menu (`"-"` a separator); it takes `delay_ms` to answer anything |
| `tray_attention = { title, attention }` | it asks for attention, or stops |
| `expect_tray = { title, asked = "clicked 2" }` | it was asked this: `activate`, `clicked N` (menu entry N, from 1) |
| `open_settings = { page? }` | starts the settings app in the test, on a page (`appearance`, `keyboard`, `screens`) or its home, and waits for its window |
| `expect_settings = { label, focused?, selected?, shown? }` | the settings window shows an element with this label: with the keyboard or not, on (a switch, a choice) or not; `shown = false` checks it does not |
| `click_settings = "Accent Amber"` | clicks the settings window's element with this label, through the compositor |
| `drag_settings = { label, by = [dx, dy] }` | presses on the settings window's element with this label, moves by so much in steps, and lets go |
| `expect_budget = { max_ms = 50 }` | every frame drawn so far took at most that long, and no loop handler went over its budget |
| `expect_state_file = { name, contains? = […], lacks? = […] }` | a file of the compositor's state directory (notes, the saved session) holds these strings, once written |
| `restart = true` | the compositor goes as a crash would, and a new one starts on the same configuration and state; clients go with it |
| `open_trim = "{last_recording}"` | opens the recording editor on a file (the last recording saved), in-process, and waits for it; `expect_trim`, `click_trim`, `drag_trim` drive it as the annotation window's steps do |
| `expect_export = { name_has, length? = [from, to], max_bytes? }` | a file the editor exported beside the last recording, once the editor names it: its length in seconds (ffprobe) and its size |
| `swipe = { fingers, by = [dx, dy], begin? = true, release? = true }` | a touchpad swipe on the screen under the pointer, moved in eight steps a frame apart; `begin = false` goes on with the one held |
| `expect_idle = { for_ms }` | no output draws a frame while this much virtual time passes: at rest, nothing is redrawn |
| `expect_recording = { audio?, keys_at_ms? }` | the last recording saved, read by ffprobe and ffmpeg once done: with a sound track as long as the picture (or none); keys drawn near the bottom from this time in the video, the top unchanged |
| `caldav = { dir, user, password }` | a CalDAV server on 127.0.0.1 serves the `.ics` files of `dir` (relative to the scenario); its calendar's address is `{caldav}` in later configuration, `type` and `calendar_password` steps |
| `caldav_stop = true` | the CalDAV server goes, as the network would |
| `calendar_password = { id, password }` | the settings keep this account's (`user@url`) password |
| `expect_calendars = { events?, secrets?, contains? = […], error? }` | the calendars, once so: events from now on, passwords kept, events by title, whether an account failed its last sync |
| `wait_wallpapers = true` | waits until the wallpapers are made and drawn, as pictures do: before a capture the compositor takes itself |
| `open_annotate = "{last_screenshot}"` | opens the annotation window on a capture (`{last_screenshot}` is the last one saved, `{runtime_dir}` the compositor's temporary directory), and waits for it |
| `expect_annotate`, `click_annotate`, `drag_annotate` | as the settings steps, in the annotation window (its capture is labelled `Capture`) |
| `expect_config_file = { contains?, lacks? }` | the configuration file holds these strings, or lacks them (the settings app writes it) |
| `expect_ui = { label, disabled?, selected?, shown?, inside? }` | the shell shows an element with this label (disabled or not, the current choice of a list or not, within `inside = [x, y, w, h]`); `shown = false` checks it does not |
| `close = "app_id"` | asks the window to close, waits until it is gone |
| `pointer_move = [x, y]` | moves the pointer, global logical coordinates |
| `click = "left"` | presses and releases a button (`left`, `right`, `middle`) |
| `press = "left"`, `release = "left"` | holds and lets go a button, for drags |
| `scroll = 15` | scrolls the wheel, logical pixels (15 per notch, down positive) |
| `key_down = "alt"`, `key_up = "alt"` | holds and lets go a key, for Alt+drag |
| `key = 30` | presses and releases a key, by evdev code (`KEY_A` is 30) |
| `keys = "super+shift+2"` | presses keys in order, releases them in reverse: `super`, `shift`, `ctrl`, `alt`, digits, letters (by position on a US keyboard), `enter`, `escape`, `tab`, `space`, `backspace`, `backtick`, `comma`, arrows, or evdev codes |
| `send = { app_id, output, desktop? }` | sends a window to an output: its desktop `desktop` (from 1), or the shown one |
| `plug = { spec = "1920x1080+1280+0", name? }` | plugs an output; `name` brings back one unplugged before |
| `unplug = "HEADLESS-2"` | unplugs an output |
| `randr = { output, enabled?, mode?, scale?, position? }` | sets a screen through wlr-output-management, as wlr-randr would (`mode` is `WIDTHxHEIGHT`), and checks it succeeded |
| `expect_output = { name, geometry? = [x, y, w, h], pixels? = [w, h], scale?, temperature? = [low, high], shown? }` | checks an output's logical geometry, pixel size, scale and night light (kelvin); `shown = false` checks it is off or gone |
| `stall_output = "HEADLESS-1"` | the output stops presenting its frames, as a stuck screen would, until the compositor recovers it |
| `set_state = { app_id, state = "maximized" }` | maximizes, makes fullscreen, minimizes or restores (`normal`) a window |
| `expect_accent = { hue = [15, 45] }` | the shell's accent in effect has a hue in that range (degrees) |
| `config = """…"""` | writes the configuration file (TOML) and waits until the compositor has read it; `{scenario_dir}` is the scenario's directory, for fixtures, `{runtime_dir}` the compositor's temporary directory, for files it writes, and `{test_client}` a test client program (`--app-id`, `--size`, `--color`) for what the compositor starts itself |
| `expect_config_error = "line 2"` | checks the last file was refused with that in the message; `""` checks it was applied |
| `expect_config_warning = "first one wins"` | checks what the configuration in effect left out |
| `expect_event = { type = "keyboard_layout", has? = "French" }` | checks the compositor said it in its event stream since the start |
| `advance_ms = 500` | moves the virtual clock forward |
| `frames = 1` | moves the virtual clock forward by whole frames |
| `expect_window = { app_id, output?, geometry? = [x, y, w, h], focused?, visible?, state?, opacity?, pinned? }` | checks a window, waiting a moment for its client to resize; `visible` is whether its desktop is shown and it is not minimized |
| `expect_no_window = "app_id"` | checks a window is gone |
| `expect_focus = "app_id"` | checks which window has keyboard focus |
| `expect_stack = ["below", "above"]` | checks the shown windows, bottom to top |
| `expect_shown = { output, desktop }` | checks which desktop an output shows, from 1 |
| `expect_received = { app_id, event = "key 30 pressed" }` | checks the client got an event: `configure WxH`, `pointer_enter`, `pointer_leave`, `button CODE pressed`, `keyboard_enter`, `keyboard_leave`, `key CODE pressed`, `close` (and `released`) |
| `expect_not_received = { app_id, event }` | checks the client never got an event |
| `expect_pixel = { output, at = [x, y], color = "#rrggbb", tolerance? = 24, is? = true }` | checks one pixel of an output, in its pixels; `is = false` checks it is not that colour |
| `expect_screenshot = { output, reference = "file.png", channel_tolerance? = 2, pixel_tolerance? = 0.0, cursor? = false }` | compares the output with a reference image next to the scenario; `cursor` draws the pointer's cursor too |

Reference images are recorded with `HOCUS_BLESS=1 cargo nextest run -p
hocus-test --test scenarios` (or `hocus scenario run --bless FILE`); look at
them before committing. When a comparison fails, the actual, expected and diff
images are left in `target/test-artifacts/<scenario>/`, which CI keeps as an
artifact.
