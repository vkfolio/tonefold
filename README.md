<p align="center"><img src="docs/images/logo.svg" width="72" alt=""></p>

<h1 align="center">Tonefold</h1>

<p align="center">
  An AI music composer app. Chat with Claude, get a whole song you can edit and hear, and take it anywhere: a DAW, MIDI or WAV.
</p>

<p align="center">
  <a href="https://vkfolio.github.io/tonefold/">Website</a> ·
  <a href="https://github.com/vkfolio/tonefold/releases/latest">Download</a> ·
  <a href="docs/tutorial-twinkle.md">Tutorial</a> ·
  <a href="docs/usage.md">Docs</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/licence-GPL--3.0-74DDC4" alt="GPL-3.0"></a>
  <a href="https://github.com/vkfolio/tonefold/actions/workflows/ci.yml"><img src="https://github.com/vkfolio/tonefold/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/vkfolio/tonefold/releases/latest"><img src="https://img.shields.io/github/v/release/vkfolio/tonefold?color=74DDC4" alt="release"></a>
</p>

Tonefold is a standalone app. It writes chords, melody, bass, drums and eight more layer kinds into
a piano roll you can edit, plays them through built-in sounds, and exports MIDI, a mix and one WAV
per layer. You don't need a DAW. If you use one, an optional CLAP/VST3 plugin puts the same
composer inside it.

**Platform:** Windows. The app is the main download; the CLAP and VST3 plugins are optional.
macOS is on the roadmap.

## What it does

- **Composes, layer by layer.** Melody first? It fits chords to it. Chords first? It writes a melody
  over them. Bass, drums, pads, arpeggios, plucks, counter-melodies, harmonies, sub and percussion
  follow what already exists, section by section, with the whole song in view.
- **Plays like a person, not a sequencer.** Repeats are new performances rather than copies, timing
  and dynamics land where players put them, each style has its own groove and pocket, and sustained
  parts carry real expression — swells, sustain pedal, 808 glides.
- **Sounds like something immediately.** A built-in General MIDI synth means you hear the song
  before choosing a single instrument. Export the mix and one WAV per layer, or the MIDI.
- **Goes wherever you produce.** Export MIDI, a stereo mix and per-layer stems from the app, or drag
  a layer straight from Tonefold onto a track in your DAW.
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

That installs the app under `%LOCALAPPDATA%\Tonefold` with no administrator prompt and downloads
the GeneralUser GS soundfont once (~31 MB). Start **Tonefold** from the Start Menu.

**Node.js 20+** is optional and only the chat composer needs it — everything else, including
generation, playback and export, works without it.

## In your DAW (optional)

To use Tonefold inside a DAW as well, install the plugins:

```powershell
scripts\install.ps1 -Plugins
```

This asks for one UAC prompt and puts `Tonefold.clap` and `Tonefold.vst3` in
`C:\Program Files\Common Files`, where DAWs look. Rescan plugins in your DAW. In the plugin, the
composer follows the host's transport and can send live MIDI to your instruments.

**FL Studio** is the tested host; prefer the CLAP build there (**Options → Manage plugins → Find
installed plugins**). The installer also adds a piano-roll import script when FL Studio is
present. Other CLAP/VST3 hosts should work but are not yet tested; reports are welcome.

## Using it

- `docs/tutorial-twinkle.md` — a complete song, start to finish, assuming nothing (uses FL Studio for the final step).
- `docs/usage.md` — the reference: workflow, layers, arrangement, export, the humanization knobs.
- `skill/tonefold-song/SKILL.md` — compose from an external AI (Claude Code, Claude Desktop, Codex)
  through the command line.

## Building from source

Rust stable and (for the composer) Node 20+:

```powershell
cargo build --release -p tonefold-plugin --features standalone --bin tonefold-standalone   # the app
cargo build --release -p tonefold-cli
cd agent; npm ci; npm run build; cd ..
cargo xtask bundle tonefold-plugin --release          # optional: the CLAP and VST3
scripts\install.ps1                                 # installs what you built (-Plugins for the CLAP/VST3)
scripts\package.ps1                                 # or make a release zip
```

Layout: `crates/tonefold-core` is the engine (theory, generators, humanization, MIDI, rendering) with
no plugin dependencies; `crates/tonefold-plugin` is the app and its egui editor, built as the
standalone app and, through nih-plug, as the optional CLAP/VST3; `crates/tonefold-cli` is the command line; `agent/` is the Claude Agent SDK sidecar (which can
also run on an Ollama model, local or remote — see `docs/usage.md`).

## Roadmap

- macOS (CLAP, VST3, AU) and testing in other DAWs: Ableton Live, Bitwig, Reaper
- Audio-to-MIDI so the composer can follow a recorded idea
- More specialists (orchestration, sound design) and a shared style memory per project

Ideas and votes are welcome in [Discussions](https://github.com/vkfolio/tonefold/discussions).

## Contributing

Bug reports, fixes, genre knowledge and new generators are all welcome. Start with
[CONTRIBUTING.md](CONTRIBUTING.md). Security issues go through [SECURITY.md](SECURITY.md).

## Licence

GPL-3.0-or-later (see `LICENSE`) — nih-plug's VST3 bindings require it.

The composer's reference library in `agent/reference/` is based on "Music Composition Agent Skill"
by SJY051, licensed under CC BY 4.0; see `agent/reference/NOTICE.md`. The built-in sounds come from
the GeneralUser GS soundfont by S. Christian Collins, downloaded at install time under its own
licence.

Tonefold was called FLVSTX before 1.4.0.

FL Studio is a trademark of Image-Line Software. Claude is a trademark of Anthropic. Tonefold is
an independent project, not affiliated with or endorsed by either.
