What makes MIDI sound played rather than programmed, and the knobs Tonefold gives you. (For the
musical side — where a style sits against the grid, what a groove *is* — read
`references/rhythm-groove/groove-and-feel.md`.)

Every generated part is already humanized: the engine applies a **groove template** (the systematic
way a style leans against the grid) and then a **random walk** around it, per layer, seeded from the
section so a repeated chorus is a new performance rather than a copy.

- **Timing.** `timing_ms` is the standard deviation in real milliseconds, tightest on downbeats and
  loosest on 16ths, redrawn at each phrase, capped at three times the setting. Notes that start
  together (chords) move together. Presets: drums 6, bass 7, chords 10, melody 9 — multiplied by the
  style (EDM ×0.5, lo-fi/hip-hop ×1.4, jazz ×1.3, cinematic ×1.6).
- **Pocket.** `pocket_ms` moves a whole part: negative is behind the beat (laid back), positive ahead
  (urgent). Within a kit the engine spreads it — kick a few ms behind, hats ahead — and
  `snare_pocket_ms` places the backbeat: about −3 for pop and rock, −5 for rock and punk, +12 to +14
  for hip-hop, lo-fi and R&B, where the snare drags.
- **Swing.** 0.5 straight, 0.52 the default pop lilt, 0.55 house, 0.58-0.62 hip-hop and lo-fi, 0.66
  jazz triplets. `swing_16ths` swings 16ths (hip-hop, house hats) rather than 8ths (jazz, shuffle).
- **Groove templates.** `straight_pop`, `swing_16` (MPC), `boom_bap` (dragged backbeat), `house`,
  `jazz_swing`, `tresillo` (3+3+2 for latin, reggaeton, some hip-hop). Chosen from the style unless
  set per song or per layer; `"none"` plays dead straight.
- **Velocity.** Metric accents (`accent`), a phrase arch and a section-long rise (`phrase_arc`), and
  a little noise (`vel_jitter` 0.04-0.06 — noise is not expression). Ghost notes stay ghostly.
  Drum bands: accents 110-127, regular hats 60-90, ghosts 30-60, roughly 80% moderate to 20% accented.
- **Gate length.** `gate_var` scales lengths 85-115%, longer when louder. Drums are never gated.

Use `humanize` with overrides when the user asks for a different feel, or after `set_notes` with
humanize=false:
- "tighter" → `timing_ms` 3, `vel_jitter` 0.03
- "looser / more human" → `timing_ms` 14, `gate_var` 0.15
- "swing it" → `swing` 0.6, `swing_16ths` true
- "lazy / behind the beat" → `pocket_ms` −6 on drums and bass, or `snare_pocket_ms` +14
- "robotic on purpose" (some EDM) → `timing_ms` 0, `vel_jitter` 0, `accent` 0.2, groove `"none"`

**Expression is not velocity.** On a sustained patch — pad, strings, organ — velocity does nothing
once the note has started. The engine writes controller lanes instead: a CC11 arch per phrase on
pads and harmonies, sustain pedal lifted and re-pressed at each chord change on sustaining chord
parts, and pitch-bend glides on 808 basses. They play through the built-in sounds, ride along in
exported MIDI and follow a layer dragged into FL Studio.

Beyond every knob, feel comes from the writing: ghost notes and hat dynamics, rests between phrases,
longer notes at cadences, a motif that returns, and fewer notes when another part is busy.
