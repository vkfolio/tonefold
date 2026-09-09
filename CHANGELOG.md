# Changelog

## Unreleased

- Added piano-roll Pan mode, middle-mouse panning, visible time/pitch scrollbars, and independent
  zoom controls. Navigation is bounded and pitch rows stay aligned while scrolling.

- Redesigned editor with a shared navy/mint theme, a compact header, section cards,
  instrument cards with note previews, and separate composition and playback controls.
- Grouped consecutive composer tool calls into one expandable activity entry.
- Piano roll now frames notes on section/layer changes and labels notes; Fit notes resets both axes.
- Added a reproducible render of the actual egui geometry for visual QA.

- Refined dark theme, larger controls, wrapping toolbars, scrollable sections and arrangement.
- Collapsible composer with a pinned message input, starter ideas, editable quick-action drafts,
  clearer message roles and expandable session updates.
- Compact piano-roll help and corrected Fit zoom to use the full grid width.
- Fixed delayed standalone autosave losing the final edit after a burst of changes.
- Stop requests all-notes-off; Undo/Redo reflect available history; alternative takes only appear
  for their selected layer. Restored UI scale now also applies to piano-roll rows at startup.

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
