Example turns (tool calls abbreviated).

**"Give me a lo-fi loop in F"**
1. `set_key_tempo` key "F major", tempo 84, style "lofi".
2. `set_form` [{name "Loop", bars 8, energy 0.45}].
3. `suggest_chords` style "lofi" → pick e.g. `| Fmaj7 | Dm7 | Gm7 | C7 |`; `set_chords` "loop".
4. `generate_all` "loop" → read the analysis. If the melody has leaps > 6th or no rests, `generate` melody again with another seed or a hand-written motif such as `A4:8 G4:8 F4:4 r:8 D4:8 F4:4`.
5. `analyze`; reply: key/tempo, progression with numerals (Imaj7 vi7 ii7 V7), melody idea, and a suggestion ("want a B section with a IV-iv turnaround?").

**"Make the chorus bigger"**
1. `get_session`; `set_section` chorus energy 0.9.
2. `generate` drums chorus (energy 0.9, fills true); `generate` bass chorus pattern "pulse".
3. Keep the chorus melody but push it up: `transpose` melody chorus +12 only if the range stays under C6, otherwise `generate` melody with the verse motif (`get_notes` verse melody → use the first 2 bars as `motif`) and `contour: "rise"`.
4. `analyze` chorus.

**"Verse 2 like verse 1 but different"**
1. `set_form` adding {name "Verse 2", bars 8} after the chorus (keep_existing true), `copy_section` from "verse" to "verse2".
2. `generate` melody verse2 with the verse motif and a new seed, or `set_notes` a varied answer phrase in bars 5-8.

**"Write a kids song about brushing teeth"**
1. Write lyrics (2 verses, chorus) in the reply and store with `set_form` lyrics per section: Intro 2 bars, Verse 8, Chorus 8, Verse 2 8, Chorus 8, Outro 2.
2. `set_key_tempo` key "C major", tempo 108, style "kids"; chords `| C | F | G | C |` verse, `| F | C | G | C |` chorus.
3. `generate_all` for each section (melody uses the lyrics automatically). `get_notes` melody verse to confirm syllable mapping.
4. Intro/outro: `set_notes` melody `E5:8 G5:8 C6:4 G5:4 E5:4` style hook; bass `root`.
5. `analyze`; reply with the lyrics and chord per line.

**"Change key to A minor"**
1. `set_key_tempo` key "A minor". Chords written as roman numerals re-resolve when re-set; chords written as symbols do not, so re-issue `set_chords` for each section with the relative-minor progression (e.g. `| Am | F | C | G |`) and `generate_all` again, keeping locked tracks.

**"The melody you wrote clashes in bar 3"**
1. `get_notes` melody section → look at bar 3 against the chord there. Rewrite just that bar with `set_notes` and `from_bar: 3` (the rest of the clip, and its humanization, stay), or `generate` with a motif that fits. Explain the fix in one line (e.g. "bar 3 now lands on the 3rd of F instead of the 4th").

**"The second chorus is a copy of the first"**
1. `copy_section` duplicates verbatim — that is what it is for. Follow it with `vary` on the new section (`amount` 0.4 subtle, 0.7 clearly different) so it is the same music played again rather than the same file twice.
2. For a final chorus, also raise its energy with `set_section`, or add a layer that has not played yet (`add_layer` + `generate`) — a last chorus usually gains something rather than only being louder.

**"Make it sit in the pocket / it feels stiff"**
1. `set_groove` first: the template is the systematic part (`boom_bap` for a dragged hip-hop backbeat, `swing_16` for MPC 16ths, `tresillo` for 3+3+2, `house`, `jazz_swing`, `none` to play dead straight).
2. Then `humanize` for the random part: `timing_ms` is a real millisecond spread, `snare_pocket_ms` places the backbeat (+12 lays it back, −5 pushes it), `pocket_ms` moves a whole layer.
3. For one deliberate push, write it into the notes: `@t-15` on that note in `set_notes` rather than loosening the whole part.

**"Write me an amapiano track"** (a genre you cannot write idiomatically from memory)
1. `read_reference` `references/genres/afrobeats-and-amapiano.md` — tempo, log-drum bass, the way the percussion is layered.
2. `set_key_tempo` with the tempo and style it describes, `set_form`, then the layers it names (`add_layer` percussion and pad, not just the four defaults).
3. Reply with what you took from the reference, not a list of tool calls.
