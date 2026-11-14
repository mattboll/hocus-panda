# Project desktops — specification (#789, draft for review)

Status: draft, not approved. The mockup is the "Bureaux projet" design
canvas (<https://claude.ai/artifact/8cocKvgYxdwBufi8qZK9yQ>; sources in
the maintainer's `hocus-panda-tools/mockups/s20-projects`), boards 1 to 5
and 3b. Its notes list the small choices made for it and the open
questions (§13 here); nothing is built before they are answered and the
canvas approved.

One space per project: a desktop that has a name and a folder. Its
windows stay open while the user is elsewhere; closing the project saves
them, with what makes them useful again (a terminal's folder, a
browser's tabs, an agent's conversation), and opening it brings them
back. It builds on the desktops (windows.md §12), the overview (§12b) and
the last session's restore (§14, `session/restore.rs`).

Decided with the maintainer (2026-11-13):

- A desktop becomes a project from the overview ("En faire un projet":
  a name, a folder) and from the launcher ("projet hocus" opens or
  creates the project; recent git folders are suggested).
- Leaving a project for another keeps its windows open: it is a live
  desktop to come back to at once. Saving and bringing back happen when
  the user closes the project, and at log out and log in.
- Coming back: the applications in their places, the terminals in the
  project's folder, the browsers' tabs (read from the Firefox and
  Chromium windows on the project's desktop, reopened as addresses in a
  new window, no extension), and the agents' sessions (a terminal in the
  folder resuming the agent's last conversation, `claude --resume ID`),
  each in its place. The agents' ids and folders come from the hooks
  Hocuspanda already receives (`hocus agent-hook`, docs/agents.md).

## 1. What a project is

- **A name and a folder**, given to one desktop of one screen. The name:
  1 to 40 characters, unique among the projects (case and accents
  ignored). The folder: an absolute path to a directory; it need not be
  a git repository.
- **Its monogram**: two capitals from the name, the first letters of its
  first two words (split at `-`, `_`, `.`, spaces: `hocus-site` is HS),
  else its first two letters (`blog` is BL). Two projects with the same
  monogram keep it; the name tells them apart where there is room.
- **Open**: the project is a desktop. Everything on that desktop belongs
  to it, and only that: a window sent there joins it, a window sent away
  leaves it, a window "on all desktops" (#723) never belongs to one.
  Switching desktops changes nothing; its windows go on running.
- **Closed**: its windows are closed and saved (§6); it has no desktop.
  It is listed in the launcher and comes back when opened.
- A desktop is one project at most, a project one desktop at most.
- **Its name is the desktop's** while it is open: the bar's tiles and
  their list, Alt+Tab's other desktops' suffix (`Firefox · hocus-site`),
  the overview's label and screen readers say it instead of
  `[desktops] names` (6.14). Closed, the desktop is again "Bureau N" or
  its name from the file.
- **Its screen**: a project lives on the screen its desktop is on. That
  screen unplugged, its windows move as windows.md §12 moves them and the
  project goes with them, onto the desktop they land on, unless that one
  is a project already (then the project stays waiting for its screen
  with its windows, as they do). The screen back, it goes back with
  them.
- **A desktop taken out** (`[desktops] count`, the Settings' `desktop-remove`):
  the project goes with its windows to the desktop they go to, unless
  that one is a project: then it is closed first, as by its menu (§5),
  the Settings' question saying so.

## 2. Making a project

### 2.1 From the overview (board 1)

- Under the thumbnail the pointer rests on, of a desktop that is not a
  project, a button "En faire un projet" / "Make it a project" (26 px
  high, the accent's rim and tint, a folder-plus icon), in place of
  nothing: the number's line stays. On a project's thumbnail the label
  says its monogram, "1 · hocus-panda", its folder, and what it holds
  ("3 fenêtres · 7 onglets · 1 agent"); a click on its name opens its
  card (§4).
- The button opens a form under the thumbnail (400 px, the popovers'
  glass): "Faire du bureau 2 un projet", one line saying its windows stay
  where they are, then:
  - **Nom**: the shell's text field (#763), filled with the chosen
    folder's last part and following the folder until the user types in
    it.
  - **Dossier**, its recent git repositories (§2.3), one 42 px row each
    (folder icon, name, path in Martian Mono, when on the right); the
    first is chosen. "Choisir un autre dossier…" asks the desktop
    portal's file chooser for a directory on the bus given with
    `--session-bus` (as the capture's editor does), else turns into a
    field for a path.
  - "Annuler" and "Créer le projet". `Enter` creates, `Esc` cancels.
- Errors under the field: "Un projet s'appelle déjà « hocus-site »." /
  "A project is already named "hocus-site"."; "Ce dossier n'existe
  pas." / "This folder does not exist.".
- Created: the label changes at once, the bar's dot gets the monogram,
  and a toast says "Le bureau 2 est le projet « hocus-site »" / "Desktop 2
  is the project "hocus-site"". The overview stays open.
- By the keyboard: `Tab` reaches the button of the shown desktop's
  thumbnail after the windows (§12b's selection), `Enter` opens the form.

### 2.2 From the launcher (board 2)

- The field's first word `projet` or `project` (accents and case
  ignored), then a space, lists only projects and folders (5.7's `:`
  does the same for emoji): no application, no web row.
- **PROJETS**: the projects whose name contains the rest (all of them
  when it is empty), the open ones first, then the closed by when they
  were closed, newest first. A row (81 px, 5.2's card): the monogram on
  a 51 px badge (the accent's gradient when open, grey when closed), the
  name, a pill "ouvert" / "fermé", and a line: its folder, "sur le
  bureau 1" or "fermé hier à 18:40", and what it holds ("Firefox (9
  onglets), 2 terminaux, Claude Code"). The hint: "Entrée y va" /
  "Enter goes there" (open), "Entrée le rouvre" / "Enter reopens it"
  (closed). `→` on a closed one shows its card (§4).
- **DÉPÔTS GIT RÉCENTS**: the recent repositories (§2.3) whose folder's
  name contains the rest and that no project has, three at most: "Créer
  le projet « hocus-cli »", the path and "dernier commit il y a 2 h";
  hint "Entrée le crée".
- Nothing matches: "Aucun projet ni dépôt git pour « … »." / "No project
  or git repository for "…"."
- `Enter` on an open project goes to its desktop (as a tile's click);
  on a closed one, reopens it (§7); on a folder, makes a project of it
  on the desktop §7 chooses, and goes there. The launcher closes.

### 2.3 Recent git repositories

Found off the loop, by a thread of the shell's, as the launcher or the
form opens, again only when the last look is a minute old:

1. the folders the terminals on that desktop are in (the form only),
   "ses terminaux y sont" / "its terminals are there";
2. the folders the agents' hooks named in the last 30 days (§8.1), kept
   in `$XDG_STATE_HOME/hocuspanda/folders` (one line each: the path, the
   last time, wall clock), 200 at most;
3. the directories holding a `.git` under `$HOME` two levels down, and
   three levels under `~/github`, `~/gitlab`, `~/src`, `~/code`,
   `~/projects`, `~/dev`, `~/work`, `~/git`: no symbolic link followed,
   no hidden directory, `node_modules` or `target` entered, 4000
   directories at most.

Each is a repository's top (the folder holding `.git`, a file for a
worktree); sorted by the newest of `.git/logs/HEAD` and `.git/index`
(a worktree's own in the main repository's `.git/worktrees/`), which is
"when" ("dernier commit il y a 2 h": from that time, wall clock). A
project's folder is not listed.

## 3. On a project (boards 3, 3b)

- **The mini form's dots** (§5 of bar.md): a project's dot carries its
  monogram, Manrope 800 at 7.5 px, `#0B1020` on the accent (the current
  desktop), `text` on the others. Nothing else changes in the mini form.
- **The full form**: over the tiles, a line for the project shown on
  that screen: a 14 px folder in the accent's text colour, its name (13
  px, 700), under it its folder (Martian Mono 10.5 px, `text.faint`, cut
  with "…" at its start). Its height is kept on a desktop that is not a
  project, so that nothing moves as desktops switch. A project's tile
  carries its monogram at its bottom right (7 px, 800, on the accent in
  the current tile, on `rgba(14,17,28,0.85)` in the others). With several
  screens' rows (8.4) only the bar's own screen has the line.
- **Switching**: the dots at the top of the screen (windows.md §12) have
  under them, 10 px down, the project arrived on: a pill (30 px,
  `rgba(14,17,28,0.88)`, the bar's rim), its monogram, its name (13.5 px,
  700), its folder (12 px, `text.faint`); it fades with the dots. None
  for a desktop that is not a project.
- **The list** of a project's tile or dot (3.9b) is 300 px wide (240 for
  the others): its monogram, its name and "3 fenêtres" on the first line,
  its folder under it, its rows (each window's icon and title, and on
  the right what it adds: "7 onglets", the terminal's folder, "agent au
  travail"), then a divider and "Fermer le projet…" (§5).
- **Screen readers**: "Bureau 2, projet hocus-site, 3 fenêtres" /
  "Desktop 2, project hocus-site, 3 windows".

## 4. What a project holds (board 5)

One card, the same in three places: under a project's thumbnail in the
overview (a click on its name), for a closed project in the launcher
(`→` on its row), and summed up in the closing's confirmation (§5). Open,
it says what is there now; closed, what will come back.

- Its monogram, name, a pill ("ouvert · bureau 1" / "fermé"), its folder,
  since when it is open or when it was closed.
- **FENÊTRES**: a sketch of the desktop (the bar's tiles' drawing, 160
  × 100), and each window: its icon, title, where it is ("moitié
  gauche", "plein écran", "flottante").
- **TERMINAUX**: each terminal window's folder, its program.
- **ONGLETS · FIREFOX · 7** (one section a browser window): each tab's
  title and host, four, then "3 de plus".
- **AGENTS**: each agent's panda in its state, its tool, its
  conversation's first prompt (the hook's `UserPromptSubmit`, the first
  of its session, cut at 60 characters), and, closed, the command that
  will resume it.
- "Les fenêtres privées ne sont jamais lues ni gardées." / "Private
  windows are never read nor kept."
- "Fermer le projet…" (open) or "Rouvrir" (closed).

The shell gets, for the card, the titles and the hosts of the tabs, never
their whole addresses (§10).

## 5. Closing a project (board 4)

- **Where**: "Fermer le projet…" in its list in the bar and on its card
  (§4); `project-close NAME` on the control socket (what the shell
  sends, and the tests).
- **The confirmation**: a dialog centred on the project's screen, as the
  session menu's (7.4), over a dim (`rgba(7,9,15,0.45)`): its monogram
  badge, "Fermer le projet hocus-site ?", its folder; "Ses fenêtres se
  ferment. Quand tu le rouvres, tout revient à sa place :" and what will
  come back, one row each (an application and its tabs' count, a
  terminal and its folder, an agent "qui reprendra sa conversation").
  Then, in the waiting state's amber, what closing interrupts, when
  something does: an agent at work or waiting ("Claude Code travaille
  encore : fermer interrompt sa réponse en cours."), a terminal running
  a program other than its shell ("npm run dev s'arrête ; son terminal
  reviendra dans le dossier, sans relancer la commande."). "Annuler" and
  "Fermer le projet"; `Enter` closes it, `Esc` cancels.
- **Then**, in order:
  1. the windows' snapshot is taken on the loop (§6), and the folders,
     tabs and agents are read by a worker (§8), which sends them back;
  2. each window is asked to close, as `Super+Q` does (its effects
     included); a window that asks something (an editor's "save?")
     keeps the project open until it closes; one still there 30 s after
     the request stays, on what is then a plain desktop, and the toast
     names it;
  3. the browsers' files are read again once their windows are gone
     (§8.3, a closed window's tabs are written then), and the project's
     file is written (§9);
  4. the desktop is a plain desktop again, its dot loses the monogram,
     and a toast says "hocus-site est fermé, ses 3 fenêtres gardées." and
     "Super+Espace, « projet hocus-site » : il revient." The screen stays
     on that desktop.
- An agent's terminal closing ends the agent; its conversation is
  already on disk (Claude Code and Codex write it as it goes), which is
  what resuming needs.

## 6. What is saved of a window

The session's own record of a window (windows.md §14, `SavedWindow`):
its application's desktop entry, its app id, its screen with that
screen's size and work area, its desktop, where it floats, its state,
kept above, its opacity. Not saved, as there: dialogs, the Settings, the
shell's surfaces, windows whose application has no desktop entry (the
confirmation lists those as "ne reviendra pas" / "will not come back").
A project adds, per window, what it is (§8):

- a **terminal**: its folder;
- a **browser** window: its tabs, in order, which one is selected, which
  are pinned;
- an **agent's** terminal: the agent's tool, session id, folder and
  transcript's path.

## 7. Reopening a project

- **From**: the launcher (`Enter` on its row, §2), its card's
  "Rouvrir"; `project-open NAME` on the control socket.
- **Which desktop**, on the screen the launcher is on: the current one
  when it is empty, else the first empty one, else one added after the
  last (`[desktops] count` is not changed: the desktop added is the
  session's, taken out when the project closes) up to nine; with nine
  full, the launcher's toast says "Plus de bureau libre : ferme un
  projet ou une fenêtre." / "No free desktop: close a project or a
  window." and nothing opens. The strip slides there first.
- **What starts**, as the last session's restore starts it (§14: off
  the loop, as session children, the windows matched by app id in order
  to the saved places, each taking its place before its first frame, on
  that desktop):
  - each **application** once, from its desktop entry;
  - each **terminal** window on its own, its folder given (§8.2);
  - each **browser** window with its tabs (§8.3);
  - each **agent** in a terminal of its own, resuming (§8.1).
- **Patience**: 30 s after the last start, the places not taken are
  dropped. The project's file is removed once everything came back or
  was dropped; until then a log out loses nothing.
- **Said**: when everything came back, nothing (the windows say it).
  Otherwise one notification (5.3), the project's monogram as its icon,
  "hocus-site est revenu, sauf …", naming what did not, three at most,
  then "et 2 autres": "Slack n'a pas pu être rouverte." (§14's
  sentence), "La conversation de Claude Code n'a pas pu reprendre. Son
  terminal est ouvert dans hocus-site.", "Les onglets d'une fenêtre de
  Firefox n'ont pas pu être lus." The project's folder gone: its
  terminals and agents open in `$HOME`, and the notification says "Le
  dossier ~/github/hocus-site n'existe plus."

## 8. How each kind of thing comes back

All of this is read by a worker thread, never on the loop: the loop
hands it the windows' snapshot (ids, app ids, titles, client pids) and
gets owned results back through a channel. Nothing is read at rest:
only when a project closes, when the session saves (its 3 s after a
change, windows.md §14; for windows on a project's desktop only, and
the tabs and agents only at log out), and when a card opens.

### 8.1 Agents (lot D)

- **What the hooks give**: every Claude Code and Codex hook's JSON has
  `session_id`, `cwd` and `transcript_path`. Today `hocus agent-hook`
  sends the session id as the agent's key and the folder's last part as
  its name. It will add two tab-separated parts to its line
  (docs/agents.md, "The line"): `cwd PATH` and `transcript PATH`. The
  compositor keeps them with the agent (the first `cwd`, as for its
  name), never logs them.
- **Which agents belong to a project**: those whose window (docs/agents.md:
  the first window above the agent's process) is on its desktop. An
  agent in one of herdr's panes is herdr's (§13, E).
- **Saved**: tool, session id, folder, transcript's path; and the
  terminal it ran in (that window's desktop entry), at that window's
  place. Its terminal window is saved as the agent's, not as a plain
  terminal.
- **Brought back**: if the transcript's file is gone, or the tool's
  program is not on the `PATH`, the terminal opens in the folder and the
  notification says the conversation could not resume. Else the terminal
  opens in the folder running the tool's command through the user's
  shell, so that leaving the agent leaves a shell there (`$SHELL -c
  'claude --resume ID; exec $SHELL'`; the id checked as letters, digits
  and `-` before):

  | Tool | Command | Its folder |
  |---|---|---|
  | Claude Code | `claude --resume SESSION_ID` | the session's `cwd` (Claude Code finds a session by its folder) |
  | Codex | `codex resume SESSION_ID` | the session's `cwd` |

  Lot D checks against the current Codex that its hooks' `session_id` is
  the id `codex resume` takes; if not, the id is the one in its
  transcript's file name (`rollout-…-ID.jsonl`).
- **Came back**: the agent's `SessionStart` hook with that session id
  (Claude Code's `source` is `resume`) within 30 s; its row and its
  panda come back in the bar. Not heard in 30 s: counted as not resumed,
  its terminal left as it is.

### 8.2 Terminals (lot B)

- **A terminal** is a window whose desktop entry has `Terminal` in its
  `Categories`, or whose app id is in the table below.
- **Its folder**: the client's process (the Wayland client's credentials;
  X11's `_NET_WM_PID`), its children that have a controlling terminal
  (the shells), and their `/proc/PID/cwd`. One shell: its folder. A
  terminal serving several windows from one process (`footclient`'s
  server, kitty's single instance, `gnome-terminal-server`, wezterm's
  mux) cannot tell which shell is whose: each of its windows gets the
  folder of the shell started last whose folder is inside the project's
  folder, else the project's folder. A window with tabs or splits comes
  back as one window, in its first shell's folder.
- **The program it runs**: the shell's foreground process group, from
  `/proc/PID/stat`'s `tpgid`: when it is not the shell's, the program's
  name is what the confirmation says will stop (§5). It is never
  started again.
- **Brought back**: the terminal's desktop entry, with its folder as the
  process's working directory, and its own option where it has one:

  | Terminal (app id) | Folder | Command (§8.1) |
  |---|---|---|
  | foot, footclient | `--working-directory=DIR` | `-- CMD…` |
  | kitty | `--directory DIR` | `CMD…` |
  | Alacritty | `--working-directory DIR` | `-e CMD…` |
  | org.wezfurlong.wezterm | `start --cwd DIR` | `-- CMD…` |
  | com.mitchellh.ghostty | `--working-directory=DIR` | `-e CMD…` |
  | org.gnome.Terminal | `--working-directory=DIR` | `-- CMD…` |
  | org.gnome.Console | `--working-directory=DIR` | `-- CMD…` |
  | org.gnome.Ptyxis | `--working-directory=DIR` | `-x CMD` |
  | org.kde.konsole | `--workdir DIR` | `-e CMD…` |
  | xfce4-terminal | `--working-directory=DIR` | `-x CMD…` |

  Another terminal gets the working directory alone; an agent needing a
  command in one it cannot give it to opens in the first of the table
  found on the `PATH`.

### 8.3 Browser tabs (lot C)

No extension, nothing in the browser changed: its own session files,
which it keeps for its crash recovery.

- **Which windows**: the windows on the project's desktop whose desktop
  entry is Firefox's (`firefox`, `org.mozilla.firefox`, `firefox-esr`,
  the Flatpak's) or a Chromium's (`chromium`, `google-chrome`,
  `brave-browser`, `vivaldi`, `microsoft-edge`).
- **Firefox**: its profiles (`profiles.ini`, `installs.ini`) in
  `~/.mozilla/firefox`, `$XDG_CONFIG_HOME/mozilla/firefox` (newer
  Firefox), the Flatpak's `~/.var/app/org.mozilla.firefox/.mozilla/firefox`
  and the Snap's `~/snap/firefox/common/.mozilla/firefox`; the profile
  running is the one whose `lock` names this machine. Its file is
  `sessionstore-backups/recovery.jsonlz4`: mozLz4 (the 8 bytes
  `mozLz40\0`, the size as 4 little-endian bytes, one LZ4 block), JSON
  inside: `windows[]`, each `tabs[]`, each `entries[]` and `index` (from
  1: the entry shown, its `url` and `title`), `pinned`; the window's
  `selected` (from 1). Firefox writes it at most every 15 s
  (`browser.sessionstore.interval`), and as a window closes, which then
  goes to `_closedWindows` with its tabs: after closing, that entry is
  read (§5, step 3).
- **Chromium**: the profile last used (`Local State`'s
  `profile.last_used`, else `Default`) in `~/.config/<browser>`; its
  newest `Sessions/Session_*` file: SNSS (the 4 bytes `SNSS`, a version,
  then commands, each a 16-bit size, an 8-bit id and a pickle), read for
  the tabs' windows and order, their navigation entries (address,
  title), the selected entry and tab, the tabs and windows closed, as
  Chromium's `components/sessions` (BSD) writes them. It writes them a
  few seconds after a change.
- **Matching a window to the file's**: the window's title is its shown
  tab's ("hocus-site · Accueil — Mozilla Firefox", "… - Chromium");
  without the browser's suffix it is matched with each saved window's
  selected tab's title; two alike are told apart by their order and
  size. A window not matched comes back without its tabs (a new window),
  and the notification says so.
- **What is not read**: private and incognito windows (neither browser
  writes them), a page's form data, history, cookies. A tab's history is
  not kept, only its address.
- **Brought back**: the browser's desktop entry with the addresses, one
  new window holding them as tabs in order: Chromium's `--new-window
  URL…` does; for Firefox, lot C finds the call that does on the
  current version (`--new-window` with several addresses), and if none
  does, the first address opens the window and the others follow it
  with `--new-tab`, one at a time, each once the window has the
  previous one. The selected tab is selected again where the browser
  allows it; pinned tabs come back as tabs.
- **The browser's own restore**: Firefox's "Ouvrir les fenêtres et
  onglets précédents" and Chromium's "Continuer là où vous en étiez"
  reopen every window themselves when they start. At log in, a browser
  set to (Firefox's `prefs.js` `browser.startup.page` = 3, Chromium's
  `Preferences` `session.restore_on_startup` = 1) is started once
  without addresses, as §14 does now, and its windows are matched to the
  saved places by their titles (§13, D). Reopening a closed project
  always gives the addresses: its windows are gone from the browser's
  session.

### 8.4 Applications

The last session's restore, unchanged: the desktop entry starts once
for all its windows, which take their places in order by app id.

## 9. Where it is kept

`$XDG_STATE_HOME/hocuspanda/`, written by a thread through a temporary
file, as `session` is; created `0700`, its files `0600`:

- `projects` (TOML), the list: for each project its `id`, `name`,
  `folder`, when it was made, opened and closed (RFC 3339, local time:
  the user reads them), and `open` with its screen's identity and desktop
  while it is open.
- `projects.d/ID.toml`, a closed project's contents. `ID` is its name in
  lower case, other characters `-` (`hocus-site`), `-2` and on when
  taken; it stays when the name changes.

  ```toml
  [[window]]
  entry = "org.mozilla.firefox"
  app-id = "org.mozilla.firefox"
  screen = "BOE NE160QDM 0"
  screen-size = [2560, 1600]
  area = [0, 0, 2364, 1600]
  place = [8, 8, 1180, 1584]
  state = "left"
  [window.browser]
  kind = "firefox"
  selected = 1
  [[window.browser.tab]]
  url = "http://localhost:4321/"
  title = "hocus-site · Accueil"

  [[window]]
  entry = "foot"
  app-id = "foot"
  # …its place…
  [window.terminal]
  folder = "/home/alex/github/hocus-site"

  [[window]]
  entry = "kitty"
  app-id = "kitty"
  # …its place…
  [window.agent]
  tool = "claude-code"
  session = "3f6c2a1e-9b0d-4c55-8e21-7d0a94c1b2e6"
  folder = "/home/alex/github/hocus-site"
  transcript = "/home/alex/.claude/projects/-home-alex-github-hocus-site/3f6c2a1e-9b0d-4c55-8e21-7d0a94c1b2e6.jsonl"
  first-prompt = "refais la section du haut de l'accueil, plus courte"
  ```

  `desktop` is not kept: a project comes back on the desktop §7 chooses.
- **Log out and log in**: the session's file (windows.md §14) gains the
  same parts for the windows on an open project's desktop, and a
  `[[project]]` per open project (its `id`, screen and desktop). At log
  in, each comes back on its desktop with the session's windows, its
  terminals in their folders, its tabs and its agents resuming. With
  `[session] restore = false`, nothing comes back by itself: each open
  project is written as closed at log out, to reopen from the launcher.

## 10. Privacy

- The tabs' addresses may hold secrets (a token in a query, a private
  document's link). They are kept only in these `0600` files, on this
  machine, read only for the windows on a project's desktop; the log
  says how many tabs, never an address or a title; the shell gets titles
  and hosts for the card, never a whole address; nothing leaves the
  machine.
- Private windows are never read: the browsers do not write them.
- The agents' transcripts are never read: only whether the file is
  there. Their first prompt (§4) is the hook's, kept in the project's
  file.
- Forgetting a project (§13, G) removes its file.

## 11. Accessibility and motion

- The form, the confirmation and the launcher's rows by the keyboard
  (§2, §5); screen readers' names (§3); the confirmation's dialog is
  modal, its focus on "Fermer le projet".
- Reduced motion: the name under the dots and the toasts fade; the
  windows coming back open as they do without effects.

## 12. Tests

Headless, one scenario per criterion (`crates/hocus-compositor/tests/projects.rs`,
two screens where it matters), in the session restore tests' way: the
test's own program as the applications started again (a desktop entry
of a fake XDG tree), a `HOME` and an `XDG_STATE_HOME` of its own.

1. A desktop made a project (the line `project-make SCREEN DESKTOP NAME
   FOLDER`, as the form sends it): the `desktops` line names it, the
   list's title, Alt+Tab's suffix and the screen readers' name say it;
   a name taken is refused.
2. A window sent to the project's desktop belongs to it, one sent away
   does not; a window on all desktops never does.
3. Switching away and back: its windows are the same (ids), never
   asked to close.
4. Closing: each window asked to close; one that does not close in 30 s
   stays and is named; `projects.d/ID.toml` holds every window's place
   and state; the desktop is plain again.
5. Terminals: a terminal stand-in (the test's program with app id
   `foot`, a shell child `sleep` in a folder of the test's) is saved
   with that folder and reopened with `--working-directory=` that folder
   and as its working directory.
6. Browsers: a fake Firefox profile (`profiles.ini`, a lock naming the
   host, a `recovery.jsonlz4` the test compresses) and a fake Chromium
   `Sessions/Session_*` (a recorded SNSS file, committed with the test):
   each window matched by title, its tabs saved in order with the
   selected one; a private window's tabs absent; a stand-in browser on
   the `PATH` records the addresses it is given, in one new window.
7. Agents: hooks' lines with `cwd` and `transcript`; closing saves the
   session; reopening starts the terminal stand-in with `$SHELL -c
   'claude --resume ID; exec $SHELL'` in that folder (a fake `claude` on
   the `PATH` records its arguments and folder, then says `SessionStart`
   with `source` `resume`); the transcript removed: no command, and the
   notification says so.
8. Reopening chooses the current empty desktop, else the first empty
   one, else one more; nine full: refused, said.
9. What did not come back is named in one notification; everything
   back: no notification.
10. Log out with two open projects, log in: both on their desktops, their
    windows in place; `[session] restore = false`: both closed, listed.
11. The launcher (`tests/launcher.rs`, the real shell): "projet hoc"
    lists the open project first, the closed one, then the git
    repositories of a fake `HOME` (one with a newer `.git/index` first,
    none inside `node_modules`); `Enter` on each does what §2 says.
12. Nothing at rest: a project open and shown, no frame and no wake-up
    in half a second; no file read while nothing closes or saves.

Unit tests: the monogram, the name's rules, mozLz4 and the session JSON,
SNSS's commands, the matching by title, the terminals' table, the
desktop chosen, the git scan's limits, the files' round trip.

On screenshots, by the maintainer (`HOCUS_SHOTS`), not by new headless
tests: the overview's button, form and labels, the bar's dots, line,
list and the name under the dots, the launcher's rows, the confirmation,
the card, the toasts and the notification, against the canvas.

## 13. Open questions

Decided by the maintainer (2026-11-13):

- B. A new project opens a terminal in its folder only when its desktop
  is empty (made from the launcher onto an empty desktop); a desktop
  that already has windows gets nothing new.
- C. Yes: every terminal, on any desktop, comes back in the folder it
  was in at log in (the same mechanism as projects', lot B).
- D. A browser set to restore its own windows keeps doing so at log in;
  Hocuspanda opens no tabs twice and only puts each browser window back
  on its project's desktop. Hocuspanda reopens tabs itself when a
  closed project is reopened, and at log in only for a browser that
  does not restore its own session.
- F. Yes: Settings › Desktops, « Retenir les onglets des projets », on
  by default. Private windows are never read either way.

With the canvas's approval, the maintainer took these defaults:

- A. The mini bar shows the monogram only; the full name shows as
  desktops switch.
- E. herdr: only its agents come back, not its session.
- G. Projects in the Settings (rename, change the folder, forget): not
  in this sprint.
- H. A tenth project when nine desktops are taken: a message says the
  nine desktops are taken and offers to close a project.
- I. The launcher finds a project by its name too ("hocus"), without the
  word "projet".

The small choices of the canvas's note "Petits choix faits pour la
maquette" are approved as drawn.

## 14. Lots

- **A, projects on desktops** (boards 1, 3, 3b): the model (a desktop's
  name and folder, `projects`), the overview's button and form, the git
  repositories (§2.3), the bar (dots, line, list, the name under the
  dots), names everywhere (§1, §3), the card while open. Tests 1 to 3,
  12.
- **B, closing and reopening, applications and terminals** (boards 2,
  4, 5): the confirmation, the save, the terminals (§8.2), the reopening
  (§7) with the session restore's starts, the notification, the
  launcher's `projet` rows, log out and log in. Tests 4, 5, 8 to 11.
- **C, browser tabs** (§8.3): Firefox's and Chromium's files, the
  matching, the addresses given back. Test 6.
- **D, agents' conversations** (§8.1): the hook line's `cwd` and
  `transcript` (docs/agents.md), the resume commands, their coming back
  heard. Test 7.

A comes first; B needs A; C and D need B and not each other.

## As built

### Lot A, projects on desktops (2026-11-13)

- **The model** (`crates/hocus-compositor/src/projects.rs`): a project is
  open on one desktop of one screen, by the screen's identity; its file,
  `$XDG_STATE_HOME/hocuspanda/projects`, is read once as the compositor
  starts and written by a thread of its own (the directory `0700`, the
  file `0600`), which also looks at a new project's folder. A project
  open at log in comes back on its desktop of its screen, the session's
  restore putting its windows back (the session file's `[[project]]` and
  `[session] restore = false` come with lot B). Desktops taken out or
  fewer in the file: a project goes with its desktop; landing on another
  project's, it is no longer open (lot B closes it first, with its
  confirmation). A screen unplugged: the project waits for its screen.
- **Made** by the overview's form (`project-make` on the link) or the
  Settings' line: `project-make SCREEN DESKTOP NAME FOLDER`, its parts a
  tab apart when the name or the folder holds spaces, answered
  `{"project-made": {"id", "name", "screen", "desktop"}}` or
  `{"refused": …}`.
- **Its name everywhere**: the bar's `desktops` line (`projects`, per
  screen), its tiles' and dots' monograms, the full form's line (its room
  kept while the bar's screen has a project, so that nothing moves as
  desktops switch; none on a screen without one), the list (300 px, the
  monogram, the folder), screen readers ("Bureau 2, projet hocus-site, 1
  fenêtre"), Alt+Tab's suffix, the overview's label (monogram, "1 ·
  name", folder, "3 fenêtres · 1 agent"), the radial menu, the agents'
  `list_desktops`, and the pill under the dots as desktops switch.
- **The overview**: "En faire un projet" under the thumbnail the pointer
  rests on (and `Tab` past the windows); the form and the card are the
  shell's surfaces (`crates/hocus-shell/src/project/`), drawn on the
  popovers' glass, holding the keyboard over the overview; a press beside
  them closes them, the overview closing too. The form's repositories
  (§2.3) are looked for on a thread of the shell's, four shown; the
  terminals' folders come from the windows whose app id is in §8.2's
  table; the agents' folders' file is read, which lot D writes. Made, the
  launcher's toast says so.
- **The card** while open: the header, the desktop's sketch and its
  windows with where each is, the private windows' line. Its terminals
  and "Fermer le projet…" came with lot B; its tabs and agents, and the
  list's details per window, come with lots C and D.
- **Tests** (`crates/hocus-compositor/tests/projects.rs`): 1, 2, 3 and
  12. Screenshots: `HOCUS_SHOTS=… cargo nextest run -p hocus-compositor
  projects_shots:: --run-ignored only`.

### Lot B, closing and reopening, applications and terminals (2026-11-14)

- **Closing** (`crates/hocus-compositor/src/project_life.rs`): "Fermer le
  projet…" in the list of a project's tile or dot (under a divider) and
  on its open card asks for the confirmation (`ToCompositor::ProjectCloseAsk`):
  the project's windows are read off the loop first (the projects'
  thread: each window's desktop entry, a terminal's folder and program,
  §8.2), then the shell's dialog (`hocus-project-close`, 480 px, centred
  on the project's screen) shows what comes back (an application and how
  many windows, "Un terminal dans hocus-site", "… ne reviendra pas" for a
  window without a desktop entry) and, in amber, an agent working or
  waiting on the desktop and each terminal's foreground program. The
  compositor draws its dim (`rgba(7,9,15,0.45)`) under it; from the
  overview, the overview closes first. `Enter` closes, `Esc` cancels, the
  keyboard rests on "Fermer le projet". Confirmed (`ProjectClose`, or
  `project-close NAME` on the Settings' line), the windows' places are
  taken on the loop, read off it, and each window is asked to close as
  `Super+Q` does; the project waits for them, 30 s at most; one still
  there stays, on what is then a plain desktop, and the toast names it.
  Those that closed are written to `projects.d/ID.toml` (`0600`), the
  project is closed, a desktop the session added for it taken out, and a
  toast says "hocus-site est fermé, ses 3 fenêtres gardées." then, fainter,
  "Super+Espace, « projet hocus-site » : il revient." (at the board's own
  size, not the launcher's × 1.5).
- **The saved format** (§9): a window is the session's `[[window]]` with a
  `[window.terminal]` table (`folder`); lots C and D add `[window.browser]`
  and `[window.agent]` beside it. The session's file gains `[[project]]`
  (`id`, `screen`, `desktop`) for each open project.
- **Terminals** (`crates/hocus-compositor/src/terminals.rs`): a window
  whose entry has `TerminalEmulator` in its categories, or whose app id is
  in §8.2's table. The client's children with a controlling terminal are
  its shells (`/proc/PID/stat`, `cwd`); one window of its process: its
  first shell; several (a terminal's server): the last started inside the
  project's folder, else that folder (outside a project, the last
  started). The foreground program is `tpgid`'s command line when it is
  not the shell's group. Every terminal on any desktop is saved with its
  folder by the session's thread (§13, C), read again as the session
  ends; started again with its folder as its working directory and the
  table's option.
- **Reopening** (`project-open NAME [SCREEN]`, the launcher, the closed
  card's "Rouvrir"): the current desktop when free (no window of its own,
  not a project, nothing coming back to it), else the first free one,
  else one more (the session's, `[desktops] count` in the file
  untouched), nine at most; none free, `ProjectNoRoom`: the launcher opens
  again saying "Plus de bureau libre : ferme un projet ou une fenêtre.
  « blog » attend un bureau libre." in amber over the open projects, whose
  `Enter` is then "Entrée le ferme…" (their confirmation) (§13, H). The
  screen switches there, then the windows come back through the session
  restore's machinery, generalised (`session/restore.rs`, `Comeback`):
  each application once, each terminal in its folder (its folder gone:
  `$HOME`), the windows matched by app id in order to their places on that
  desktop, 30 s of patience; the project's file goes once everything came
  back or was dropped. What did not come back is one notification, its
  image the project's monogram (`image-data`, drawn by the compositor):
  "blog est revenu, sauf Slack", its body the session's sentence ("Slack
  n'a pas pu être rouverte.") and "Le dossier ~/code/blog n'existe plus."
  when the folder went; three names at most, then "et 2 autres".
- **The launcher** (board 2): `projet` / `project` then a space lists
  "PROJETS" (open first by desktop, then closed newest first) and "DÉPÔTS
  GIT RÉCENTS" (three, the shell's thread, none a project's folder), no
  application nor web row; nothing found says so. A project's name alone
  finds it too, its row before the applications (§13, I). `Enter`: an
  open one's desktop (`ProjectGo`), a closed one reopens (`ProjectOpen`),
  a repository becomes a project (`ProjectNew`) on the desktop §7
  chooses, named after its folder, a terminal opening in it when that
  desktop is empty (§13, B). `→` on a closed one shows its card, centred.
  The compositor tells the shell the projects (`projects` line) as it
  connects and as they change: a closed one's windows are what will come
  back.
- **The card** (board 5), open and closed: the sketch draws every window
  at its place and size, back to front, each in its rim (the focused one
  in the accent) with its icon; "TERMINAUX" lists each terminal's folder
  and application; "Fermer le projet…" (open) or "Rouvrir" (closed, the
  accent). Closed: grey badge and pill "fermé", "fermé hier à 18:40" /
  "fermé le 2 novembre".
- **Log out and log in** (§9): `[session] restore = true`, the projects
  stay open in their file and their windows come back on their desktops
  with the session's (terminals in their folders). `false`: as the next
  session begins, each project still open is closed, its windows of the
  session's file (on its screen and desktop) its contents; done then
  rather than at log out, so that a session that ended without logging
  out (a crash) closes them too.
- **A screen unplugged** (§1): a project goes with its windows onto the
  same-numbered desktop of the screen they go to, unless that one is a
  project (it then waits for its screen); the screen back, it goes back
  (`away-from` in the projects' file).
- **Not in this lot**: the list's per-window details ("7 onglets", a
  terminal's folder, "agent au travail") — a terminal's folder there would
  be read as the list opens, which §8 does not allow; the tabs (C), the
  agents' rows, resume commands and the confirmation's "qui reprendra sa
  conversation" (D).
- **Tests** (`crates/hocus-compositor/tests/projects.rs`): 4
  (`closing_saves_the_windows_that_close_and_leaves_a_plain_desktop`), 5
  (`a_terminal_comes_back_in_its_shells_folder`: the test's own program
  as `foot`, its shell a `sleep` on a pty of its own), 8
  (`reopening_takes_the_current_empty_desktop_else_the_first_else_one_more`),
  9 (`what_did_not_come_back_is_named_in_one_notification`, a
  notifications server on a bus of the test's), 10
  (`log_out_and_in_brings_the_open_projects_back_or_closes_them`), 11
  (`the_launcher_lists_projects_then_repositories_and_enter_does_each`, the
  real shell: here rather than in `launcher.rs`, beside the projects'
  harness), and the screen's
  (`a_project_follows_its_windows_to_another_screen_and_back`).
  Screenshots: `HOCUS_SHOTS=… cargo nextest run -p hocus-compositor
  projects_shots::closing_and_reopening --run-ignored only`.

### Lot C, browser tabs (2026-11-14)

- **Read** (`crates/hocus-compositor/src/browsers.rs`), on the projects'
  thread as a card or the confirmation opens and as a project closes, on
  the session's thread as it ends (the windows on a project's desktop):
  Firefox's profile locked by a live process (`lock`'s `IP:+PID`), else
  the one whose session was written last; its
  `sessionstore-backups/recovery.jsonlz4` (else `sessionstore.jsonlz4`),
  mozLz4 decoded by an LZ4 block reader of our own (no dependency), its
  `windows` and `_closedWindows`, a window with `isPrivate` left out.
  Chromium's (`chromium`, `google-chrome`, `brave-browser`, `vivaldi`,
  `microsoft-edge` app ids) last used profile and newest
  `Sessions/Session_*`, its SNSS commands read for windows (normal ones),
  tabs, their navigations, the shown entry and tab, pinned, tabs and
  windows closed. A window's title without its browser's suffix is
  matched with each file window's shown tab's title, then the nearest
  size, then the order; a title saying private (Private Browsing,
  Navigation privée, Incognito, InPrivate) is never matched and its
  window keeps no tabs. Each browser's profile is remembered by the
  thread, read again once its windows closed (Firefox's lock goes as it
  quits). The log says how many tabs, never an address nor a title.
- **Closing** reads the tabs with the other windows, then, once the
  windows went, a second later the files again (§5 step 3), the closed
  windows first; what it finds replaces the first read.
- **Saved** as `[window.browser]` (`kind`, `selected` from 1, `[[window.browser.tab]]`
  `url`, `title`, `pinned`); a window not found keeps `kind` alone.
- **Brought back**: one start a window holding its addresses (those with
  a scheme, never an option, 200 at most): Chromium's `--new-window
  URL…`; Firefox's `--new-window URL` for one, the addresses alone for
  several, which Firefox's `nsDefaultCommandLineHandler` opens together
  in one new window when there is more than one (checked in
  `BrowserContentHandler.sys.mjs`). The selected tab is not selected
  again (no browser takes it on its command line). A window not found
  comes back as a new window and the notification says "Les onglets d’une
  fenêtre de Firefox n’ont pas pu être lus." At log in, a browser whose
  preferences reopen its own session (§13 D) starts once without
  addresses; a window that shows a saved tab's title as it maps takes that
  place, else the first of its application's.
- **The setting** `[desktops] remember-tabs` (Settings › Desktops,
  « Retenir les onglets des projets », on): off, nothing is read and
  browsers come back as plain applications.
- **The card** lists "ONGLETS · FIREFOX · 7" per browser window, each
  tab's title and host (the shell never gets an address), four, then "3
  de plus" / "Moins"; the confirmation says "Firefox, avec ses 4
  onglets".
- **Tests**: test 6
  (`projects::a_projects_browsers_come_back_with_their_tabs`: a fake
  Firefox profile and the committed SNSS file
  `tests/data/Session_13400000000000000`, built by
  `browsers::tests::snss_file`, `HOCUS_SNSS_WRITE=1` writes it again);
  unit tests for mozLz4, the session JSON, SNSS, the matching, the
  addresses given, the profile found.
