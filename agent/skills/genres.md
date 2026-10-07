What the Tonefold engine does with a style word, so you can pick the generator params. Pass the style
to `set_key_tempo`; the substrings below are what it actually matches.

For the *music* — how a genre is written, what its harmony and groove really are, what a producer in
that scene would expect — read the genre file from the reference library instead of guessing
(`references/genres/…`, listed in the index). This table is only the engine's side.

| style word | tempo | feel the engine picks | bass `pattern` | drums |
|---|---|---|---|---|
| `pop` (default) | 100-125 | `straight_pop`, swing 0.52, snare −3 ms | `push` verses, `pulse` choruses | backbeat, 8th hats, fill every 4 bars |
| `house` `edm` `techno` `dance` `trance` | 120-135 | `house`, timing ×0.5, swing 0.55 (house) | `pulse` / `octave` | four-on-the-floor, open hat on the "and" |
| `lofi` `lo-fi` `boom` | 70-95 | `boom_bap`, swing 0.60, snare +12 ms | `push` / `root5` | swung 16th hats, ghost snares |
| `hip` `r&b` `rnb` `neo-soul` `soul` | 65-95 | `boom_bap`, swing 0.58, snare +14 ms | `walking`-ish / `push` | dragged backbeat, ghosts |
| `trap` `drill` | 130-150 (half-time) | `swing_16`, swing 0.54, snare +8 ms | `808` (slides are generated) | half-time snare, rolling hats |
| `jazz` `swing` `bebop` | 60-200 | `jazz_swing`, 8th swing 0.66 | `walking` | ride pattern, brushed backbeat |
| `cinema` `orchestra` `ambient` `epic` | 60-100 | no groove, timing ×1.6, deep phrase arc | `root` / `pedal` | sparse or none; builds get a snare roll |
| `latin` `reggaeton` `salsa` `bossa` `afro` | 90-130 | `tresillo` (3+3+2 accents) | `root5` / `push` | percussion layers earn their place here |
| `rock` `metal` `punk` `country` `folk` | 90-160 | `straight_pop`, straight 8ths, snare −5 ms | `root5` / `octave` | backbeat, crash on section starts |
| `kid` `nursery` `rhyme` `lullab` | 95-115 (or 6/8 at 60-70) | tight, no groove, strong accents | `root` / `root5` | shaker, soft kick and clap |

Anything else falls back to the pop feel — which is a real feel, not dead straight.

Notes that matter in practice:
- **Layers.** Only chords/melody/bass/drums exist by default. `add_layer` for pad, arpeggio, pluck,
  counter_melody, harmony, sub, percussion — a thin, same-y arrangement is usually a missing layer,
  not a weak part. `references/orchestration/arrangement-density.md` if you want the reasoning.
- **Time signatures.** 3/4 waltz (kids, folk, cinematic), 6/8 (lullabies, folk, Irish), 5/4 for
  character. Set with `set_key_tempo` (time_signature).
- **Instrument.** `set_instrument` per layer picks the built-in General MIDI sound the user hears;
  it does not change what is written.
