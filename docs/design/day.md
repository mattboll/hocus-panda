# Your day: the data

The data under "Your day" in the bar (Day.1 to Day.6, issues #425 to
#430, their specs in their comments): calendars, events and tasks, from
sources behind adapters, read off the shell's loop. This page is the data
layer (Day.0, #563): the crate `hocus-day` and the shell's `day` module.
What the bar draws of it is each view's own spec.

## The pieces

```
config.toml [day] ──► hocus_day::Workers::start ──► one worker thread per account
                                                      │  (its source: fixture, local list,
                                                      │   later Google, Planka)
                                                      ▼
                          mpsc::Sender<Snapshot>  (owned, only when it changed)
                                                      │
              shell: day thread ──► Day (Arc, behind a mutex) ──► views: day::snapshot()
                                     │ changed?
                                     └─► log "the day holds …" (debug), wakers (day::on_change)
   views ──► day::change(Change) ──► Workers::change ──► the tasks' account's worker
```

- **`hocus-day`** (no smithay, no iced): the model, the two adapter traits,
  the fixture and the local list, the workers, the configuration's
  `[day]`, and the accounts' secrets.
- **The shell's `day` module** (`crates/hocus-shell/src/day`): starts the
  workers as the shell starts, from the configuration file the compositor
  read (`HOCUS_CONFIG`, set by the compositor; empty: none), and keeps the
  latest day for the views.

## The model (`hocus_day::model`)

| Type | Holds |
|---|---|
| `Calendar` | `id`, `account`, `name`, `colour`, `read_only`, `shown` |
| `Event` | `id`, `calendar`, `title`, `when`, `status` (`Confirmed`, `Tentative`, `Cancelled`), `answer` (`Accepted`, `Declined`, `Unanswered`), `free_busy` (`Busy`, `Free`), `place`, `video`, `page`, `description`, `alarms` (minutes before its start, earliest first) |
| `When` | `Timed { start, end }` (instants, shown in the system's zone) or `AllDay { first, last }` (dates, `last` included) |
| `TaskBoard` | `id`, `name`, `columns` (each `Column { id, name }`, in their order) |
| `Task` | `id`, `title`, `due`, `position` (its source's number: the column's order is by it, smallest first) |
| `Place` | where a card goes: a column, and a position in it or its top |

Ids are newtypes over strings (`AccountId`, `CalendarId`, `EventId`,
`BoardId`, `ColumnId`, `TaskId`). `Colour` reads and writes `#RRGGBB`.

- **Order**: `sort_events` sorts by start; on the same start, all-day
  events first, then the shorter, then by title and id. `sort_tasks` sorts
  by position, then id.
- `Event::takes_time()`: timed, not cancelled, not declined, not free:
  what takes the meeting card (Day.2), Today (Day.3) and the reminders
  (Day.6).
- Recurring events are expanded by the source, each occurrence its own
  event with its own id, exceptions honoured.
- The video link is the source's to find: the event's conference data,
  else the first meeting address in its place, then its description.

## The adapters (`hocus_day::source`)

Two traits, called by an account's worker on its own thread, one call at
a time: they block, and need only be `Send`. Neither knows how its source
signs in: an adapter gets what it needs (a token provider, a file) when it
is made.

```rust
pub trait CalendarSource: Send {
    fn calendars(&mut self) -> Result<Vec<Calendar>>;
    fn events(&mut self, calendar: &CalendarId, range: Range) -> Result<Listing>;
    // incremental sync; Err(Expired) makes the worker list again
    fn changes(&mut self, calendar: &CalendarId, token: &SyncToken) -> Result<Changes>;
}

pub trait KanbanSource: Send {
    fn board(&mut self) -> Result<TaskBoard>;                     // its columns, in order
    fn cards(&mut self, column: &ColumnId) -> Result<Vec<Task>>;  // in the column's order
    fn create(&mut self, column: &ColumnId, title: &str) -> Result<Task>;  // at its top
    fn move_card(&mut self, card: &TaskId, place: &Place) -> Result<()>;   // to a top, or back
}
```

- `Listing { events, token }`: a source without tokens gives `None`, and
  is never asked for `changes`. `Changes { changed, removed, token }`.
- `Range::window(now)`: the events the bar keeps, 2 months back to 12
  months ahead (Day.5).
- `SourceError`: `Offline`, `SignIn` (a token refused or missing),
  `Refused` (a conflict), `Gone` (the card is no more), `Expired` (the
  sync token), `Invalid` (as set it cannot work: a file that does not
  read, a column that is not there, a kind not built yet).
- `ColumnNames::find(board)`: the tasks' column is the one named as set,
  else TODO, else the first; the done column, as set, else DONE, else the
  last; `+` adds to the add column as set, else the tasks'. By name,
  case-insensitively; a name set that no column has is `Invalid`.

### Built in this ticket

- **The local list** (`local::LocalList`), a `KanbanSource` with the
  columns `todo` and `done`, in `$XDG_STATE_HOME/hocuspanda/tasks.json`
  (`{"todo": [Task…], "done": [Task…]}`, mode 0600), read as its worker
  starts and written by its worker after each change, whole or not at all.
  A file that does not read is left as it is, and the list refuses changes.
  It is always there: the bar's tasks until a board is set.
- **The fixture** (`fixture::FixtureCalendars`, `fixture::FixtureTasks`):
  a day from a file, below.

## The fixture

For tests and screenshots: a file named in the configuration, read again
each time it changes (its worker watches its folder with inotify, and
sleeps meanwhile). Its times are read against the shell's clock as the
file is read, so `HOCUS_CLOCK_AT` gives the day it says. What the bar
changes in its tasks is kept in memory, never written to the file; a
change of the file starts its tasks again from what it says.

```toml
[[calendars]]
id = "work"               # default: its name
name = "Travail"
color = "#8FB3FF"         # default: the calendars' blue
read-only = false
shown = true              # as the account would mark it

[[events]]
calendar = "work"         # by id or name; default: the first calendar
title = "Point produit"
start = "now+25m"
end = "now+55m"           # default: an hour after its start
# or a whole day, or several: date = "today", days = 1
status = "confirmed"      # "tentative", "cancelled"
answer = "accepted"       # "declined", "unanswered" (default)
free = false              # true: shown free
place = "Salle Jupiter"
video = "https://meet.example.org/abc-defg"
page = "https://calendar.example.org/event/1"
description = "…"
alarms = [10]             # minutes before its start
id = "e1"                 # default: e and its rank

[tasks]
board = "Sprint"
columns = ["TODO", "DONE"]   # the default
refuse = false               # true: every change refused, as offline

[[tasks.cards]]
title = "Relire la spec"
column = "TODO"           # default: the first column
due = "tomorrow 18:00"
id = "spec"               # default: c and its rank
```

A time is `now`, `now+1h30m`, `now-15m` (units `d`, `h`, `m`, `s`); or a
day, `today`, `tomorrow`, `yesterday`, `today+3`, `today-2`, then `HH:MM`
(or `HH:MM:SS`); or a local `2026-11-04 14:00` (or `2026-11-04T14:00`).
A day alone (`date`, a date-only `due`) is a whole day. All in the
shell's time zone.

## The worker (`hocus_day::Workers`)

One thread per account (`day: <account id>`), never the shell's loop.

- **When it reads**: at start (its cache first, if it has one), then on
  its schedule (`Kind::every`: Google 5 min, Planka 2 min; the fixture and
  the local list none), on request and on resume (`Workers::refresh`), when
  a fixture's file changes, and at once after a change went through.
  Between, it sleeps on its channel: nothing wakes until its schedule or a
  command.
- **What it sends**: an owned `Snapshot` of its account, only when it
  differs from the last it sent: `account`, `state` (`Loading`, `Cached`,
  `Fresh { fetched }`, `Failed { error, fetched }`, the times on the wall
  clock, for the user to read), `calendars`, `events` (the window's, of
  the shown calendars, in order), `board`, `tasks` (the tasks' column's
  cards, in order), `pending` (the changes not carried out yet).
- **Calendars**: listed whole once a day (the window moves), else synced
  from each calendar's token; the configuration's `shown` and `colors`
  apply over what the account says.
- **Changes** (`Change::Add { title }`, `Tick { task }`, `Untick { task }`)
  are queued per account, sent at once in their order. A tick goes to the
  top of the done column, and its place is remembered for its untick, which
  puts it back there (after a restart, to the top of the tasks' column).
  A tick and its untick both still queued undo each other. A change the
  source refuses stays queued with why (`Pending::refused`), the ones
  after it too when the source does not answer, and all are tried again
  each time the source answers again. A change whose card is gone is
  dropped. The queue is not kept across a restart of the shell.
- **The cache**: for the network's accounts (Google, Planka),
  `$XDG_CACHE_HOME/hocuspanda/day/<account id>.json`: their calendars,
  events, board, tasks, sync tokens and the window's day, with the time
  they were read. Read as the worker starts, so that the bar shows the
  last day at once (`State::Cached`) and while offline; written when what
  it holds changes.
- `Workers::flush(within)` waits for each worker to be done with what it
  was asked: the shell calls it before it ends (SIGTERM included), so the
  local list's last change is written.

## The day (`hocus_day::Day`)

The latest snapshot of each account, in the configuration's order, the
local list last.

- `Day::events(from, to, zone) -> Vec<Shown>`: the events of shown
  calendars overlapping `[from, to)`, every account's, in order, each with
  its calendar (`Shown { event, calendar }`: its colour, its name).
  `model::today(now)` gives today's bounds.
- `Day::tasks() -> Option<&Snapshot>`: the bar's tasks' account, the first
  board set, else the local list. Whether To do shows when only the local
  list is there and it is empty is Day.4's to say.
- `Day::accounts()`: every account's snapshot, its state for Settings and
  Day.5.
- `Day::summary(now)`: one line, for the log and tests.

## In the shell (`crates/hocus-shell/src/day`)

- `day::start()`, as the shell starts, after the notifications' wall
  clock: on a thread of its own, reads `[day]`, starts the workers, and
  keeps the day from their snapshots. A `[day]` that does not read is said in the
  log, and only the local list runs.
- `day::snapshot() -> Arc<Day>`: what a view reads, as often as it draws;
  cheap (an `Arc`).
- `day::on_change(wake)`: `wake` is called (on the day's thread) each time
  the day changes, until it returns `false`, and once at once. A view
  wakes its own surface from it, as the connections do; the shell's loop is
  never woken otherwise.
- `day::change(Change)`: a change of the bar's tasks, to their account,
  at once. The view keeps its own 1.5 s ticked state (Day.4) and reads the
  outcome in the next snapshots: the task gone from `tasks`, or its change
  in `pending` with `refused` set ("Pas enregistré").
- `day::refresh()`: every account read again. The shell asks it when
  logind says the machine resumed (`PrepareForSleep(false)` on the system
  bus, watched on a thread of its own; without a system bus, not at all).
- Each change of the day is logged at debug level, as `the day holds
  accounts=[ID STATE; …] today=N=[HH:MM «TITLE» (CALENDAR); …]
  tasks=N=[ID «TITLE»; …] pending=N`, which the headless tests read
  (`RUST_LOG=hocuspanda_shell=debug`).
- The compositor's `day` line (`hocus_link::DayAsk`: `day add TITLE`,
  `day tick ID`, `day untick ID`, `day refresh`) asks the same as the
  views, for tests (`Headless::tell_shell`).

## The configuration

`[day]` in `config.toml`, read by the shell as it starts (the rest of the
file is the compositor's). No secret goes in it.

```toml
[[day.calendars]]
kind = "google"                 # or "fixture"
account = "alex@example.org"    # google: the Online Accounts' account
file = "day.toml"               # fixture: its file, relative to config.toml's folder
shown = ["Travail", "Perso"]    # by name or id; none: as the account marks them
colors = { "Perso" = "#F28FB1" }

[[day.tasks]]
kind = "planka"                 # or "fixture"
address = "https://planka.example.org"
board = "https://planka.example.org/boards/1234"   # or its id
tasks = "TODO"
done = "DONE"
add = ""
file = "day.toml"               # fixture
```

Only the first `[[day.tasks]]` is used (the bar's tasks are one board's).
An account's id is made from its kind, what it holds and its name
(`fixture-calendars-day`, `planka-tasks-planka.example.org-1234`), unique;
`local` is the local list's. Its cache and its secret are named by it.

Google calendars come through GNOME Online Accounts (#425's amendment):
its access token from `OAuth2Based.GetAccessToken`, nothing kept by us.
Until their adapters land, a `google` or `planka` account shows as
`Failed (invalid: … not supported yet)`, and no worker runs for it.

## Secrets (`hocus_day::secrets`)

The kanban accounts' tokens (Planka's pasted access token), opaque:
`Secrets::get(account) -> Option<Token>`, `keep(account, &token) ->
Kept` (`Keyring` or `File`), `forget(account)`. They are kept in the
session's Secret Service on the bus the compositor names
(`--session-bus`, the session's own on a virtual terminal), never another,
as the attributes `application=hocuspanda`, `hocuspanda=day`,
`account=<id>`; else (no bus, no answer) in
`$XDG_STATE_HOME/hocuspanda/accounts/<id>`, mode 0600 in a 0700 folder.
Every call blocks, up to 15 s with a keyring's prompt: from a worker,
never the loop. `Token`'s `Debug` never shows it.

## For the next tickets

- **An adapter** implements `CalendarSource` or `KanbanSource` in a module
  of `hocus-day`, and replaces `not_yet` for its kind in
  `Workers::start`: the worker, its schedule, its cache, its queue and its
  retries come with it. Tested against a fake server, never the network.
- **A view** reads `day::snapshot()`, wakes its surface from
  `day::on_change`, and sends `day::change`; screenshots and tests feed it
  a fixture (`[[day.calendars]] kind = "fixture"`) and `HOCUS_CLOCK_AT`.
- Day.5's months outside the window, and Day.6's reminders computed off
  the loop, will need a request of their own to the workers; not built
  yet.

## Tests

- `hocus-day`'s unit tests: the model's order (`events_are_by_start_…`,
  `tasks_are_in_their_columns_order`), the fixture's relative times
  (`times_are_relative_to_the_clock`, `times_follow_the_clocks_zone`), the
  local list (`add_tick_untick_written_and_read_back`), the worker's queue,
  sync and cache, the configuration, the secrets' file.
- `crates/hocus-compositor/tests/day.rs`, the real shell, headless: a
  fixture's day reaches the shell; the local list survives a restart of
  the shell; changing the fixture's file shows the change; at rest nothing
  wakes, and a change of the day draws nothing.
