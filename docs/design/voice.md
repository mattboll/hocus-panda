# The voice (#619, lot 5)

A microphone named "Hocuspanda" that calls choose instead of the
laptop's: the same voice, through the user's effect. It reads the real
microphone only while something reads it. Mockup: the design canvas
« #619 — effets webcam », the window's Voix tab (sources in the
maintainer's `hocus-panda-tools/mockups/s23-webcam`); the spec: the
issue's revised comment (2026-11-17), "The window (all lots)" and
"Lot 5". The camera's lots: [camera.md](camera.md).

## The program, `hocus-voice`

- **A program of its own** (crate `hocus-voice`), not a part of
  `hocus-camera`: a call's voice must not stop because the camera's GPU
  work fails or stalls (a driver's fault ends that program, and its
  restart takes 2 s), and a camera's frame (some milliseconds of work on
  its loop) must never hold a 5 ms quantum of sound. Started by the
  compositor as the camera is: from beside its binary on a virtual
  terminal (nested or headless with `--voice`; tests give
  `Config::voice_program`), started again when it ends (2 s, then twice
  as long, up to a minute); it ends with its input.
- **Its lines**: the compositor writes the voice on its standard input,
  one JSON object a line (`hocus_link::VoiceLook`), at once and as
  `[voice] effect` changes; it says on its standard output what happens
  to it (`hocus_link::FromVoice`: on the graph, read or not, the real
  microphone it reads or would read).
- **Its source**: a PipeWire output stream, `media.class =
  Audio/Source`, `node.name = hocuspanda-voice`, `node.description =
  Hocuspanda`, 48 kHz, mono, 32-bit floats, its port made by itself
  (`adapter.auto-port-config`). Every application lists it: PipeWire's,
  and PulseAudio's through pipewire-pulse (measured on a PipeWire of the
  test's own with pipewire-pulse: `ffmpeg -sources pulse` lists
  `hocuspanda-voice [Hocuspanda]`; `pactl list sources` shows it the
  same way). Never a driver: both its streams are of one `node.group`,
  so the real microphone's clock drives the source too. A
  `priority.session` of 100 (a microphone's is over 1000): a session
  manager never makes it the default by itself.
- **Read only while read**: while a link goes out of its node (the
  registry's links; not its stream's state, which the group keeps
  running while the microphone's stream runs). Then, and only then, it
  reads the real microphone: a stream of
  its own (`hocuspanda-voice-input`, not autoconnected, of the source's
  `node.link-group`, so that a session manager never links one to the
  other), which it links itself to the microphone's first output port
  (its first channel's, `port.id` 0). It links them again whenever the
  link goes: a session manager sets a node's ports up again once it
  appears (WirePlumber does, a few milliseconds after the stream is
  linked: its ports made again, a monitor port with them), and a link
  goes with the ports it joined (#911; the log says each link made at
  info). A link undone each time it is made is asked 16 times at most
  a reading, then a warning.
  The microphone: the session's default source (the `default`
  metadata) unless it is the Hocuspanda one, else the one chosen
  (`default.configured.audio.source`), else the source of highest
  priority. The last reader gone, the microphone's stream goes.
- **Each quantum** (256 samples asked, 5.3 ms) the microphone sends is
  made by the voice on PipeWire's real-time thread into a queue the
  source's quantum takes from. The two run in the same cycle in either
  order: when the source comes first, it takes the last cycle's, and the
  queue keeps that one quantum from then on; it never holds more than
  50 ms.
- **A change of voice** is made off the real-time thread (Sans bruit's
  models take some hundreds of milliseconds to read), then the voice
  playing fades out over 5 ms and the new one in from its start: no
  click.

## The voices (`[voice] effect`, one at a time)

| Tile | Word | What it does | Its delay | One core (measured) |
|---|---|---|---|---|
| Telle quelle | `none` | the samples as they are | 0 | 0 % |
| Sans bruit | `quiet` | DeepFilterNet3 | 40 ms | 4 % |
| Studio | `studio` | equaliser, compressor, de-esser | 0 | 0.2 % |
| Panda | `panda` | a fifth up, formants up 15 %, the highs softened | 21 ms | 0.6 % |
| Robot | `robot` | one note, 150 Hz | 21 ms | 0.1 % |
| Grave | `deep` | an octave down, formants down 12 % | 21 ms | 0.6 % |

One core: the share of one core of the laptop's processor each takes,
in a debug build, on the tests' reading under a noise (Sans bruit's models
then run on every frame), its models' reading included
(`each_voice_keeps_within_its_budget`).

- **Sans bruit** (`denoise.rs`): DeepFilterNet3 (Schröter et al., MIT or
  Apache-2.0), its three ONNX models as the project publishes them,
  unchanged, in the program (8.3 MB, no download, as the camera's
  models: `crates/hocus-voice/models/`, `LICENSE-DEEPFILTERNET`), run with
  tract on the CPU frame by frame (`tract-pulse` keeps their state).
  The signal path is DeepFilterNet's own library's (libDF, MIT or
  Apache-2.0), read and written again here for tract 0.23: a 960-sample
  Vorbis window every 480; 32 ERB bands and 96 bins, normalised with
  running means, into the encoder; the ERB decoder's gains on the
  spectrum two frames back (the models look two frames ahead); the deep
  filter's five-frame taps on the first 96 bins; frames under -10 dB of
  local SNR are silence, over 30 dB left alone, over 20 dB get the gains
  only. Its delay: the window's 10 ms, the look-ahead's 20 ms, and up to
  10 ms to meet the stream's quanta: 40 ms in all (DeepFilterNet's
  low-latency model, without look-ahead, is 38 MB: left out).
- **Studio** (`studio.rs`): biquads (Robert Bristow-Johnson's cookbook):
  the rumble under 80 Hz out, +3 dB of body under 180 Hz, -2 dB around
  350 Hz, +3 dB of presence around 3.2 kHz, +2 dB of air over 9 kHz; a
  gentle de-esser (while the band around 7 kHz stands out of the voice,
  what is over 5 kHz goes down, 6 dB at most); a compressor over -26
  dBFS, 3 to 1, a soft knee of 8 dB, 5 ms in and 150 ms out, 7 dB of
  make-up, with a gentle expander under -55 dBFS so that the room's hiss
  is not raised with the voice; a limiter at -1 dBFS.
- **Panda, Robot, Grave** (`shift.rs`): a phase vocoder written here (no
  pitch shifter's code taken; never Rubber Band): 1024-sample Hann
  frames, every 256 samples; each bin's true frequency from its phase's
  advance; the spectrum divided by its envelope (its cepstrum kept under
  1.25 ms) into the excitation, which moves to the pitch's ratio, phases
  carried on at the new frequencies; then the envelope again, read at the
  formants' ratio, so that the voice stays a voice. Robot keeps each
  frame's magnitudes and sets its phases to zero, every 320 samples: the
  voice on one note, 150 Hz, metallic. The output's level follows the
  input's over a second.

## The window's Voix tab

The camera and microphone's window ([camera.md](camera.md), "The window
« CAMÉRA ET MICRO »") holds the voices as one more tab of the one list of
effects (`crates/hocus-shell/src/camera/effects.rs`): Telle quelle, Sans
bruit, Studio, Panda, Robot, Grave, the mockup's thumbnails and words,
one of them ringed, a violet dot on the tab while it is not Telle quelle,
the chosen one's words under the grid ("Sans bruit : clavier,
ventilateur, rue : retirés"). A tile writes `[voice] effect` at once
(the file's own section, as the spec names it), shown before the
compositor says it back.

- **The microphone's tile** of the in-use sign (bar.md 5.12): a click
  opens the window on Voix (the camera's opens it on Image); a hover
  still opens the sign's popover, so that its mute stays at hand (in a
  call without a camera, that tile is most of the sign); a violet dot on it
  while the Hocuspanda microphone is read with a voice other than Telle
  quelle.
- **The meter** in the preview's lower right, as the mockup draws it: a
  pill 18 px tall, radius 6, `rgba(10,14,24,0.6)`; five bars 2 px wide,
  2 apart, 10, 12, 8, 12 and 9 px tall at most, in the voice's colour
  (`#C9D0DC`, `#6CC287`, `#F5B44A`, `#F4F1EA`, `#8FB3FF`, `#B69CFF`); the
  voice's name (9.5 px, `#EEF1F6`: "ta voix", "sans bruit", "studio",
  "panda", "robot", "grave"). Each bar is the level heard 70 ms after its
  left one's, from a third of its height to all of it: the Hocuspanda
  microphone read by the window (`hocus_voice::listen`, its stream
  `hocuspanda-voice-meter`) while an application uses the microphone,
  and only then; otherwise the bars rest.
- **Settings › Caméra**: a group "MICRO HOCUSPANDA", its row "Voix" (a
  menu of the six).

**Choices made here** (to confirm on the machine):

- Sans bruit is the default: the mockup's state (Lumière was on by
  default for the same reason). Choosing Hocuspanda in a call is already
  asking for something better than the bare microphone.
- A program apart from the camera's (above), and `[voice]` a section of
  its own, as the spec's "(`[camera]`, `[voice]`)" says.
- The microphone's tile opens the window by a click only; its hover is
  the sign's popover's (the camera's tile, lot 2, took both).
- Opened from the microphone's tile with no application using the
  camera, the preview does not read the camera: it says "Caméra
  éteinte". Looking at the voices never turns the camera's light on.
- The meter reads the microphone only while an application uses one;
  its reading, and the program's own, are left out of the in-use sign as
  the camera's are; a call reading the Hocuspanda microphone is a use of
  the microphone, its application's name shown.
- Panda is a fifth up (×1.5), its formants up 15 % and its highs over
  4 kHz softened (rounder); Grave exactly an octave down, its formants
  12 % down; Robot's note 150 Hz.

## A meeting's transcription

A meeting's transcription ([meetings.md](meetings.md)) records the real
microphone, never the Hocuspanda one: its "Moi" stream names the
microphone `hocus-voice` reads as its target (`target.object`), so that
even with "Hocuspanda" made the session's default microphone, the text is
made from the voice as it is (`transcribe::capture::me_target`); without
`hocus-voice`, the default source as before.

## Measured

Headless (`crates/hocus-compositor/tests/voice.rs`, a PipeWire of the
test's own, a fake microphone node of the test's playing a tape): the
microphone it would read said at once; unread, the microphone is not
read; a call reading "Hocuspanda" reads it, and hears Telle quelle's
samples exactly (each within 10⁻⁵); a click from the microphone heard by
the call 16 ms later at most (5 clicks); Sans bruit chosen
live: a reading under a white noise 5 dB down, 4.6 dB of
SI-SNR in, 14.9 dB heard; Panda and Grave: a vowel at 140 Hz
heard at 210 and 70 Hz; the call gone, the microphone is let go. With the
real shell: the microphone's tile opens the window on Voix, Sans bruit
ringed, and Robot's tile writes `[voice] effect = "robot"`.

`hocus-voice`'s unit tests, on the tests' reading (`data/speech.wav`:
three seconds of a LibriVox reading in French, public domain): Telle
quelle gives it back exactly; Sans bruit takes a white noise 5 dB under
it from 4.6 to 14.8 dB of SI-SNR, its delay the 1920 samples it says;
Panda, Grave and Robot sing a vowel at 210, 70 and 150 Hz, as loud as it
came; each voice's time on one core, under half of it. The studio
evens a loud and a quiet tone (20 dB apart, under 12 dB out). The lines:
`hocus-link`'s `the_voice_s_lines_read_back`; the in-use sign leaving
the program's streams out: `hocus-sound`'s
`a_call_reading_the_hocuspanda_microphone_uses_the_microphone`; the
transcription's target: `transcribe::capture::tests::moi_records_the_real_microphone`.

The listening samples and the window's screenshots:

```sh
HOCUS_VOICE_SAMPLES=/tmp/voices cargo nextest run -p hocus-voice --run-ignored only listening_samples
HOCUS_SHOTS=/tmp/shots cargo nextest run -p hocus-compositor --run-ignored only voice::screenshots
```
