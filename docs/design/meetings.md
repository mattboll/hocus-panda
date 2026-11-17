# Meetings: transcribing one (#618)

Status: the MVP's spec approved on 2026-11-17 (issue #618, "Spec — MVP:
transcribe a meeting", with its mockup, `s22-transcribe`); built as below.

A meeting of the agenda, in progress, its call joined: the meeting card
offers to transcribe it. The sound stays on the laptop; the text is written
at the end, one Markdown file. Who speaks does not matter; the call's voices
are told apart when they can be.

## As built (2026-11-17)

### The bar (`crates/hocus-shell/src/transcribe.rs`)

- **The button** (Day.2's card, bar.md): while the card's meeting is in
  progress and an application uses the microphone (5.12), the card's button
  is **Transcrire** / **Transcribe** in place of the join button: 30 × 30,
  radius 8, `rgba(255,255,255,0.10)` (`0.18` under the pointer and while
  its popover shows), the mockup's waveform (16 px, `text`). Before the call
  is joined, and for a meeting ahead, the join button as before. The mini
  form's meeting popover has **Transcrire** under **Rejoindre** (34 px,
  the same fill), which opens the asking popover beside the mark.
- **Asking** (`popover/transcribe.rs`): a popover as the bar's others (§9),
  280 wide, beside the bar, its top on the button's: "TRANSCRIRE", the
  meeting's title, "Le son reste sur ce portable. Le texte est écrit à la
  fin de la réunion, dans Documents › Réunions.", "Préviens les
  participants qu'ils sont transcrits." in amber `#F5B44A`, "Télécharger le
  modèle (610 Mo)" while the models are not there yet, **Transcrire**
  (`#5B8DEF`), **Annuler**, and "Ne plus demander" (a box to tick; kept as
  `$XDG_STATE_HOME/hocuspanda/transcribe-never-ask`, after which the button
  transcribes at once).
- **While it records**: the card's button is a stop (a red square on
  `rgba(242,84,91,0.22)`), a red dot `#F2545B` (6 px, 4 px before the time
  line) pulses from 0.45 to 1 and back over 1.6 s (still on the CPU and
  with reduced motion); the sign of what is in use (5.12) gets a tile after
  the camera's: the waveform in `#F2545B` on `rgba(242,84,91,0.16)` and the
  time in Martian Mono 12 px `#FFD9DB`, side by side in the full form's row
  (77 × 24), the waveform over the time in the mini form's column (36 × 44,
  as the recording's tile). Its popover has the transcription's line
  ("Point produit", "transcription · 18:42") and **Arrêter la
  transcription**. The time is counted by the shell from the compositor's
  `transcribing` line; the tile draws as its second changes.
- **Stopping**: the stop (card, mini popover, in-use popover); the meeting's
  end (the card no longer its meeting); the call's application away from
  the microphone for 2 min (its streams' key, a thread of its own timing
  it). The shell's own streams of the transcription are not a use of the
  microphone (`hocus_sound::TRANSCRIPTION_NODES`).

### The sound (`crates/hocus-compositor/src/transcribe/capture.rs`)

- Asked (`transcribe start`, `hocus_link::TranscribeMeeting`), the
  compositor makes `$XDG_STATE_HOME/hocuspanda/meetings/<id>/` with
  `meeting.json` (title, times, attendees, when it began, the language, the
  folder its text goes in) and two tracks, never mixed, 16 kHz mono 16-bit
  WAV: `me.wav`, the default source (as the recording's microphone, linked
  by the session manager); `call.wav`, the output streams of the
  application using the microphone, found by their streams' key
  (`hocus_sound::app_key`) and linked by the compositor itself to its own
  capture stream as they come (`link-factory`), so that music and
  notifications stay out. The tracks are written at the recording's pace,
  silence where nothing came, on threads of their own; stopping never
  waits.
- One recording at a time. Known limit (the spec's): without headphones
  the microphone hears the others too; the text leaves out a line of "Moi"
  that repeats what the call said at the same time.

### The text (`crates/hocus-transcribe`)

- At the stop, the compositor runs `hocus-transcribe run DIR` (beside its
  own program; `Config::transcriber`), which lowers its own priority to
  the lowest (nice 19) and says what it does on its output, a JSON line at
  a time (`hocus_meetings::Said`).
- **Where it hears** (`device.rs`): on a GPU through whisper.cpp's Vulkan
  backend, whatever its vendor: a discrete GPU with memory enough for the
  model (1.6 GB) first, the largest (a hybrid laptop's NVIDIA or AMD),
  else an integrated one, else the CPU (whisper.cpp's own, every core at
  nice 19, with the medium model, quantised q5_0, MIT, 539 MB, downloaded
  only then). The devices are read from the Vulkan loader; the program
  then starts again as itself (`exec`) with whisper.cpp shown that device
  alone (`GGML_VK_VISIBLE_DEVICES`). One line of its log says which and
  why; the progress notification's details end with its name ("·
  processeur" on the CPU). A GPU that would not take the model: the CPU.
  `[meetings] device`: `auto`, `cpu`, or a part of a device's name.
- **The engine**, behind a trait (`engine::Engine`): whisper.cpp
  (`whisper-rs`, large-v3-turbo quantised q5_0 on a GPU, each track's
  language found on its first window) hears each track's speech (where it speaks, by its loudness),
  packed in windows of 28 s at most, the silences between left out (it
  hears 30 s at once whatever they hold); sherpa-onnx tells the call's
  voices apart first (pyannote's segmentation 3.0 and 3D-Speaker's CAM++,
  clustered), "Voix 1", "Voix 2"… by their first turn, and each of the
  call's lines is the voice it overlaps most; with one voice or none told
  apart, "Les autres". A fake engine
  (`HOCUS_TRANSCRIBE_FAKE=FILE`, fixed lines) stands for it in tests.
- **The models** (`hocus_meetings::models`), 610 MB together (575 MB on
  the CPU, its medium model instead of the large one), downloaded
  on first use into `$XDG_DATA_HOME/hocuspanda/models`, each checked
  against its SHA-256 (written beside its place, renamed once checked),
  the download's progress in the notification.
- **The file**: in `[meetings] folder`, else the documents' folder's
  "Réunions" ("Meetings"); named `2026-11-17 14-00 Point produit.md`
  (another `(2)` beside one there): `# title`, the date and times
  ("mardi 17 novembre 2026, 14:00 – 14:32"), "Invités (agenda) : …" (the
  calendar's attendees, Google's and the fixture's), then a paragraph a
  turn, `**14:01 · Voix 1** — text`, a speaker's consecutive lines
  together. Written, the meeting's folder and its sound go; on failure
  they are kept.

### The notifications (`crates/hocus-compositor/src/transcribe/notes.rs`)

One notification a job, replaced as it goes, from a thread that holds its
connection while its buttons may be pressed: "Transcription de « Point
produit »", "32 min de son · environ 3 min encore" and its progress bar
(`value`; staying until replaced; closed by the user, no more of it);
then "Transcription prête", "Point produit · 32 min · Moi et 2 voix",
**Ouvrir** (the file, through `xdg-open`, as the launcher opens) and
**Dossier**; or "La transcription n'a pas pu se faire", "« Point produit »
· le son est gardé", **Réessayer** (the same folder, made again).

### Choices made here (to confirm on the machine)

- The asking popover is beside the bar, its top on the button's (the bar's
  popover rules), where the mockup draws it over the bar under the card.
- The download's line is a line of the popover above its buttons
  ("Télécharger le modèle (610 Mo)": the three models together, rounded to
  the ten); **Transcrire** agrees to it. The download starts with the
  text's making, at the stop.
- The full form's tile is the mockup's (waveform beside time); the mini
  form's stacks them as the recording's tile does, 36 × 44.
- While recording, the mini popover's button is **Arrêter la
  transcription**, and the in-use popover has the transcription's line and
  stop, as the recording's.
- The progress notification stays until replaced (`expire_timeout` 0).

### Measured headless

`crates/hocus-compositor/tests/transcribe.rs`: the real shell against a
PipeWire and a D-Bus of the test's own, its day a fixture's: the button
only during a meeting in progress whose call uses the microphone (the join
button before, and once the call leaves); asking (its words, the model to
download), then the tile, the stop and the call's own output stream
linked to its track; the stop making the text and the sound gone. The
compositor alone with the fake engine and a notifications server of the
test's: the stop writes the file with "Moi" and the voices in order (the
echo left out) and deletes the sound; a failure keeps it and "Réessayer"
makes the text. Unit tests: the tracks' files, where speech is, the turns
and the echo, the Markdown in French, the lengths and who was heard in both
languages, the links wanted. The real engine on whisper.cpp's sample,
ignored (it downloads the models): `HOCUS_TEST_MODELS=DIR cargo nextest run
-p hocus-transcribe --run-ignored only real::`. The device's choice is
unit-tested on lists of devices (a hybrid laptop's, lavapipe alone, none).

Measured by hand on 4 cores of the development machine at nice 19, its
GPUs out of reach (an empty `/dev/dri`, the NVIDIA devices `/dev/null`,
Vulkan's drivers lavapipe's alone): the Vulkan path works on lavapipe
(whisper.cpp's sample heard right with the tiny model, 177 s for 11 s:
lavapipe is far slower than whisper.cpp's own CPU path, so `auto` never
takes it); the CPU fallback, medium model, 88 s for a 34 s two-voice
sample (the large one took 145 s). **A real GPU's speed is not
measured.**

### Not built

Live text, a summary, names on voices, a Settings page (only `[meetings]
folder`), calls outside the agenda.
