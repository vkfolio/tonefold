# FLVSTX

A composer that lives inside FL Studio. Chat with Claude, get chords, melody, bass, drums and eight
more layer kinds written into a piano roll you can edit, hear through built-in sounds, and drag
straight onto your instruments.

CLAP and VST3 for Windows, plus a standalone app that needs no DAW.

![the plugin](docs/Screenshot%202026-09-05%20063230.png)

## What it does

- **Composes, layer by layer.** Melody first? It fits chords to it. Chords first? It writes a melody
  over them. Bass, drums, pads, arpeggios, plucks, counter-melodies, harmonies, sub and percussion
  follow what already exists, section by section, with the whole song in view.
- **Plays like a person, not a sequencer.** Repeats are new performances rather than copies, timing
  and dynamics land where players put them, each style has its own groove and pocket, and sustained
  parts carry real expression — swells, sustain pedal, 808 glides.
- **Sounds like something immediately.** A built-in General MIDI synth means you hear the song
  before choosing a single instrument. Export the mix and one WAV per layer, or the MIDI.
- **Gets into FL Studio three ways.** Drag a layer onto a channel, run the piano-roll import script,
  or route live MIDI out to your instruments.
- **Plans before it writes.** *Producer* mode reads the session, proposes a plan you can see — the
  key, the form, the steps and exactly which `layer@section` each one touches — and changes nothing
  until you approve it. Then it hands each step to a specialist: harmony and form, melody and
  topline, rhythm section, arrangement and mix. Nobody writes outside their own layers, and the
  whole run can be reverted in one step. *Composer* mode is still there for a quick change.
- **Edits by hand.** A full piano roll: draw, move, resize, velocity lane, drum lanes, scrub the
  playhead, click the keys to hear them. The composer sees your edits on its next turn.

## Install

Download the release zip, unzip it anywhere, and run:

```powershell
scripts\install.ps1
```

It asks for one UAC prompt (the CLAP and VST3 go into `C:\Program Files\Common Files`, where FL
Studio looks) and puts everything else under `%LOCALAPPDATA%\FLVSTX`. It also downloads the
GeneralUser GS soundfont once, ~31 MB.

Then in FL Studio: **Options → Manage plugins → Find installed plugins**, and search for FLVSTX. Or
start the standalone app from the Start Menu.

**Node.js 20+** is optional and only the chat composer needs it — everything else, including
generation, playback and export, works without it.

## Using it

- `docs/tutorial-twinkle.md` — a complete song, start to finish, assuming nothing.
- `docs/usage.md` — the reference: workflow, layers, arrangement, export, the humanization knobs.
- `skill/flvstx-song/SKILL.md` — compose from an external AI (Claude Code, Claude Desktop, Codex)
  through the command line.

## Building from source

Rust stable and (for the composer) Node 20+:

```powershell
cargo xtask bundle flvstx-plugin --release          # the CLAP and VST3
cargo build --release -p flvstx-plugin --features standalone --bin flvstx-standalone
cargo build --release -p flvstx-cli
cd agent; npm ci; npm run build; cd ..
scripts\install.ps1                                 # installs what you just built
scripts\package.ps1                                 # or make a release zip
```

Layout: `crates/flvstx-core` is the engine (theory, generators, humanization, MIDI, rendering) with
no plugin dependencies; `crates/flvstx-plugin` is the nih-plug CLAP/VST3/standalone and its egui
editor; `crates/flvstx-cli` is the command line; `agent/` is the Claude Agent SDK sidecar.

## Licence

GPL-3.0-or-later (see `LICENSE`) — nih-plug's VST3 bindings require it.

The composer's reference library in `agent/reference/` is based on "Music Composition Agent Skill"
by SJY051, licensed under CC BY 4.0; see `agent/reference/NOTICE.md`. The built-in sounds come from
the GeneralUser GS soundfont by S. Christian Collins, downloaded at install time under its own
licence.
