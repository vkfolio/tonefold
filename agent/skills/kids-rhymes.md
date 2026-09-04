Writing a complete kids' rhyme song.

**Lyrics first.** Write short lines with a steady meter (mostly 7-8 syllables, trochaic or iambic), simple rhymes (AABB or ABAB), concrete images (animals, colours, counting, daily routines), and a repeating title line. Example structure: Verse (4 lines) / Chorus (4 lines, the title repeated) / Verse 2 / Chorus. Keep the vocabulary at ages 2-6. Actions and sounds ("clap clap", "moo") are welcome. Store them with `set_lyrics` per section or via `set_form` (lyrics field) so the melody generator maps one syllable per note.

**Music.** Key C, D, F or G major. Tempo 95-115 (or 6/8 at 60-70 for lullabies). Chords: I, IV, V (and vi sparingly): `| I | IV | V | I |`, `| I | I | V | V |`, `| I | IV | I | V |`, `| C | F | G | C |`. One chord per bar, plain triads, no 7ths. Form: Intro 2-4 bars (chords + light drums), Verse 8 bars, Chorus 8 bars, Verse 2, Chorus, Outro 2-4 bars (last line slowed in feel: long notes).

**Melody rules.** Range within one octave from the tonic (C4-C5). Mostly steps and small skips; no leap bigger than a 5th. Stressed syllables on beats 1 and 3, quarter notes with occasional eighth-note pairs for two-syllable words. Each line is a 2-bar phrase ending on a longer note; lines 2 and 4 end on the tonic, lines 1 and 3 on the 5th or 3rd. Repeat the first line's melody for the third line (AABA or ABAB melody with identical rhythm). Chorus: same rhythm as the verse, melody slightly higher and simpler.

**Use `generate` with track melody and the lyrics** (params.lyrics or section lyrics). Check the result with `get_notes`: one lyric per note, the last note on the tonic, phrases ending with rests. If the syllable count of a line does not fit the bars, shorten the line or give the section more bars.

**Accompaniment.** Chords: beat pulses (the engine does this for style "kids"). Bass: `root` or `root5`, quarter notes. Drums: shaker on 8ths, kick on 1 and 3, soft clap on 2 and 4, no ghost notes, a simple snare fill every 8 bars. Add a small instrumental hook (2 bars, xylophone-like melody outlining I-V) for intro/outro with `set_notes`.

**Report** the lyrics in the chat (the user needs them for singing), with the chord per line.
