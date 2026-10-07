---
name: tonefold-song
description: Compose a song as a Tonefold project (.tonefold) by driving the tonefold-cli composition engine — chords, melody, bass, drums and more layers, humanized and rendered to audio. Use when the user asks for a song, a beat, a loop, a chord progression or a MIDI idea for FL Studio, or names a .tonefold file.
---

# Writing songs with Tonefold

Tonefold is a composition engine: you make the musical decisions, it performs them. It voices the
chords, develops the motif, places the notes against the grid with a real groove, shapes the
dynamics and writes expression (swells, sustain pedal, 808 slides). The result opens in the Tonefold
plugin or standalone app, exports MIDI for FL Studio, and renders to WAV.

## The one rule

**Never hand-write tick data.** Do not build JSON with note `start`/`len` numbers, and do not edit a
`.tonefold` file directly. Notes written that way land perfectly on the grid with flat velocities,
which is the exact sound this engine exists to avoid. Go through the commands below; the engine
humanizes everything it generates.

Write *decisions* — key, tempo, form, chord progressions, a motif, a groove — and let it play them.

## How to run it

Every command has the same shape, and each one saves the file:

```
tonefold-cli op SONG.tonefold METHOD '{"json":"params"}'
```

Use a `.tonefold` extension and the file opens in the app. On Windows PowerShell, wrap the JSON in
single quotes exactly as above. Check what you have at any point with:

```
tonefold-cli describe SONG.tonefold          # key, tempo, form, layers, chords, note counts
tonefold-cli wav SONG.tonefold out.wav       # render it and listen
tonefold-cli export SONG.tonefold DIR        # latest.mid + one .mid per layer for FL Studio
```

If `tonefold-cli` is not on PATH it is at `target/release/tonefold-cli.exe` in the Tonefold repo.

## A complete song, start to finish

```
tonefold-cli op song.tonefold set_key_tempo '{"key":"F major","tempo":96,"style":"lofi"}'
tonefold-cli op song.tonefold set_form '{"sections":[
  {"name":"Intro","bars":4,"role":"intro","chords":"| Fmaj7 | Dm7 |"},
  {"name":"Verse","bars":8,"role":"verse","chords":"| Fmaj7 | Dm7 | Gm7 | C7 |"},
  {"name":"Chorus","bars":8,"role":"chorus","chords":"| Bbmaj7 | C7 | Am7 | Dm7 |"},
  {"name":"Verse 2","bars":8,"role":"verse","chords":"| Fmaj7 | Dm7 | Gm7 | C7 |"},
  {"name":"Chorus 2","bars":8,"role":"chorus","chords":"| Bbmaj7 | C7 | Am7 | Dm7 |"},
  {"name":"Outro","bars":4,"role":"outro","chords":"| Fmaj7 . |"}]}'
tonefold-cli op song.tonefold add_layer '{"kind":"pad"}'
tonefold-cli op song.tonefold generate_song '{}'
tonefold-cli op song.tonefold vary '{"section":"chorus2","amount":0.5}'
tonefold-cli wav song.tonefold song.wav
```

That is a real arrangement in six commands. `generate_song` fills every layer of every section with
continuity between them; `vary` re-performs the repeat so it develops instead of repeating.

## What to call, and when

**Set up** — `set_key_tempo` {key, tempo, style, time_signature}, `set_form` {sections[]},
`set_section` {section, name, bars, energy, role}, `copy_section` {from, to}.

Section roles shape everything downstream: `intro verse pre_chorus chorus bridge break build drop
outro`. Energy defaults from the role and drives density, register and dynamics.

**Harmony** — `set_chords` {section, notation}, `suggest_chords` {style, count},
`harmonize` {section, complexity: simple|diatonic|rich, apply} to fit chords *to* an existing melody.

**Layers** — a new song has chords, melody, bass, drums. `add_layer` {kind} for `pad arpeggio pluck
counter_melody harmony sub percussion`. A thin, samey arrangement is usually a missing layer, not a
weak part. `set_arrangement` {track, section, active} drops a layer out of a section — silence is
arrangement. `set_instrument` {track, instrument} picks the General MIDI sound (`list_instruments`).

**Content** — `generate` {track, section, params}, `generate_all` {section, params},
`generate_song` {params}. Steer with params: `energy` 0-1, `density` 0-1, `seed` (change for another
take), `contour` arch|rise|fall|wave, `motif` (melody, your own idea in notation), `pattern`
(bass: root|root5|octave|walking|pedal|push|808|pulse; arp: up|down|updown|random|chord),
`rate`, `octaves`, `fills`.

**Feel** — `set_groove` {groove} picks the systematic lean: `straight_pop swing_16 boom_bap house
jazz_swing tresillo none`. `humanize` {track, params} then controls the random part: `timing_ms` is a
real millisecond spread (3 tight, 14 loose), `snare_pocket_ms` places the backbeat (+12 lays it back
for hip-hop, −5 pushes it), `pocket_ms` moves a whole layer, `swing` 0.5-0.66.

**Your own material** — `set_notes` {track, section, notation, from_bar, to_bar}. Use it for a hook
you actually hear, a fill, an answer phrase; use `generate` for everything else. `from_bar` rewrites
just those bars.

**Check and finish** — `analyze` {track, section} reports range, leaps, out-of-key notes, strong-beat
clashes, rests and velocity spread, and warns about the things that sound wrong. Call it before you
report back, and fix what it flags. `undo` steps back.

## Notation

**Chords** — `| C | Am | F | G |`, roman numerals `| I | vi | IV | V |`, two per bar
`| C . Am . |`, sevenths and extensions `Fmaj7 G7sus4`, slash bass `C/E`, borrowed `bVII`,
secondary `V/V`.

**Melody and bass** — `E4:8 G4:8 A4:4 r:8 G4:8~ G4:4 E4:2`, i.e. `pitch:duration` where the duration
is the denominator (4 = quarter, 8 = eighth, `.` dotted, `t` triplet), `r` rest, `~` tie,
`@v90` velocity, `@t+12` places a note 12 ms late (`@t-8` early) for a deliberate push or drag,
`/syl` a lyric syllable. Middle C is C4. Bar lines are optional.

**Drums** — one lane per line on a 16-step bar:
```
K:  x---x---x---x---
S:  ----X-------X-g-
H:  x-x-x-x-x-x-x-x-
OH: -------x--------
```
Lanes `K S CL H OH RS T1 T2 T3 RD CR P`; `x` hit, `X` accent, `g` ghost, `f` flam.

## Writing music that does not sound generated

- **One idea, developed.** State a motif, repeat it, vary it, sequence it, bring it back. Recurrence
  is what a listener hears as a hook. Pass `motif` to `generate` when you have a real idea.
- **Phrases breathe.** Think in 2- and 4-bar phrases: a question that ends away from the tonic, an
  answer that resolves. Leave rests at phrase ends.
- **Sections must contrast.** A chorus that is only louder is not a chorus — change the register, the
  rhythm, the density, or which layers play. Use `set_arrangement` to drop layers in a verse so the
  chorus has somewhere to arrive.
- **Repeats develop.** Follow `copy_section` with `vary`, and give the last chorus something new: a
  layer that has not played yet, a lifted melody, open hats.
- **Match the style word to the music.** It selects the groove, the pocket and the humanization, so
  `style` "lofi" genuinely swings and drags where "house" pushes.

## Reporting back

Say what you wrote in a few lines — key, tempo, form, the progression in roman numerals, the idea
behind the melody — and where the file is. Do not paste note lists. Offer one concrete next step.
