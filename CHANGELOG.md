# Changelog

## 1.3.1 — 2026-09-12

- The **Think** box works for Claude too: on by default (adaptive), off for faster, cheaper
  simple requests. Each provider remembers its own setting.

## 1.3.0 — 2026-09-12

Compose on an Ollama model — the one on this PC or one across the room — instead of Claude.

### Ollama

- The composer and the producer can run on an **Ollama** model — the Ollama on this PC or one on
  another machine — instead of Claude. The chat header has a Claude / Ollama switch, a server URL
  field and a model picker filled from what that server has pulled; the choice is saved with the
  project. Terminal chat: `/ollama [URL]`, `/models`, `/model NAME`, `/claude`.
- Claude and Ollama keep separate conversations per mode, so switching back to Claude resumes
  where it left off. No cost is shown for local turns.
- Under the hood the sidecar hosts a small bridge onto Ollama's native chat API rather than using
  its Anthropic-compatible endpoint: that endpoint cannot set the context window and truncated the
  composer's brief to 2048 tokens, after which the model invented tools. `FLVSTX_OLLAMA_NUM_CTX`
  (default 16384, capped at the model's limit) is the window the bridge asks for.
- **Think** checkbox for Ollama (off by default: on a small GPU it is minutes of deliberation
  before the first tool call), and the model's reasoning now shows in the transcript as it
  streams, collapsed once the answer arrives — for Claude's summaries too.

## 1.2.0 — 2026-09-09

A producer that plans the work, asks before it writes, and hands each part to a specialist.

### Producer mode

- A second mode beside the composer: it reads the session, **proposes a plan and waits**. The plan is
  a list of changes — key, tempo, form, and per step the `layer@section` it will write — so
  approving it is approving what gets touched, not a paragraph of prose. **Approve**, **Revise**
  (the note you type is fed straight back and it re-plans in the same turn) or **Stop**.
- The approval is enforced by the host, not asked for in a prompt: nothing that writes can run until
  you approve.
- **Four specialists.** Once approved, each step goes to an agent whose whole context is that craft —
  harmony and form, melody and topline, rhythm section, arrangement and mix — and each is fenced to
  its own layers. A specialist asked for something outside its territory is refused and hands the
  musical intent back to the producer instead of faking it.
- The producer checks the host's ledger of what actually landed rather than trusting a report, and a
  live checklist shows which step is running.
- **Revert this run**: a producer run makes far more changes than undo can hold, so approving a plan
  marks the session first and one button puts it all back.
- Cost per turn and for the session is shown in the panel. Nothing is capped — the model picker is
  the only thing that governs spend.

### Composer

- **Ask AI** beside *Suggest*: hands one layer to the composer, which reads the section and
  chooses the approach (or writes the notes itself) instead of reseeding the rule engine.

## 1.1.0 — 2026-09-09

A redesigned editor, and a piano roll you can actually navigate.

### Look and layout

- A shared navy and mint theme across the editor, a compact header, section cards, and instrument
  cards that preview their notes.
- Composition and playback controls are separated rather than sharing one bar; toolbars wrap and the
  sections and arrangement views scroll, so nothing is cut off at small window sizes.
- Larger controls throughout, and the restored UI scale now also applies to the piano-roll rows at
  startup rather than only after a change.

### Piano roll

- Pan mode, middle-mouse panning, visible time and pitch scrollbars, and independent zoom for each
  axis. Navigation is bounded, and pitch rows stay aligned while scrolling.
- Notes are framed automatically when you switch section or layer, and are labelled; **Fit notes**
  resets both axes and uses the full grid width.
- Compact inline help instead of a wall of text.

### Composer panel

- Collapsible, with a pinned message input, starter ideas, and quick actions you can edit before
  sending.
- Consecutive tool calls collapse into one expandable activity entry, and message roles and session
  updates read more clearly.

### Fixes

- The standalone's delayed autosave could lose the final edit after a burst of changes.
- Stop now requests all-notes-off; Undo and Redo reflect the history that actually exists;
  alternative takes only appear for the layer they belong to.

### Development

- A reproducible render of the real egui geometry, for visual QA of the editor.

## 1.0.0 — 2026-09-09

First release: a complete composer for FL Studio, installable without a build toolchain.

### Composing

- Eleven layer kinds — chords, pad, arpeggio, pluck, melody, counter-melody, harmony, bass, sub,
  drums, percussion — generated in dependency order, each following what already exists.
- Song form with roles (intro, verse, pre-chorus, chorus, bridge, break, build, drop, outro) that
  drive energy, density, register and arrangement; layers can be silent per section.
- Harmony that fits chords to an existing melody (Viterbi search over functional progressions), and
  progressions suggested per style.
- Melodies built from a motif and developed — statement, variation, sequence, cadence — with
  contour targets, singable leaps, and phrases that breathe.
- A chat composer powered by the Claude Agent SDK, with a vendored 100-file composition reference
  library it reads on demand.

### Sounding played rather than programmed

- Generation and humanization are seeded per section and occurrence, so a repeated chorus is a new
  performance rather than a copy.
- `timing_ms` delivers a real millisecond spread, tightest on downbeats, redrawn each phrase.
- Six groove templates (straight pop, MPC 16ths, boom bap, house, jazz swing, tresillo) applied
  before the random walk, chosen by style — including a default style that is not dead straight.
- Per-instrument drum pocket: kick behind, hats ahead, backbeat placed by style.
- Dynamics that follow metre, phrase arch and melodic height; bar-level variation in hats, fills,
  kicks, arps, plucks, percussion and bass.
- Expression lanes: CC11 swells on pads, sustain pedal at chord changes, pitch-bend glides on 808
  basses — through the built-in synth, the WAV render, exported MIDI and drag-out.

### Editing and hearing

- Piano roll with draw, move, resize, velocity lane, drum lanes, scale highlighting, a scrubbable
  playhead and a playable key column.
- Built-in General MIDI synth (GeneralUser GS), per-layer instrument selection, solo and mute.
- Audio export: a stereo mix plus one WAV per layer. MIDI export, drag-out to an FL channel, and an
  FL piano-roll import script.
- Songs save as `.flvstx` files (New / Open / Save); the standalone autosaves and reopens where you
  left off, and the plugin stores its session in the FL project.

### Outside the plugin

- `flvstx-cli` for scripted composition, rendering and export.
- `skill/flvstx-song/SKILL.md` so an external AI can compose through the CLI.
