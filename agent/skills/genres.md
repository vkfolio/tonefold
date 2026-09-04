Style cheat-sheet: tempo, harmony, rhythm, and generator params that fit. Pass the style word to `set_key_tempo` (style) so the engine picks matching grooves and humanization.

**Pop** (100-125 BPM). Diatonic loops: I-V-vi-IV, vi-IV-I-V, I-vi-IV-V. Verse 8 bars, pre-chorus 4, chorus 8. Melody: short repeated cells with a lifted chorus (start the chorus motif a 3rd or 4th higher, longer notes on the title line). Bass: root with pushes (pattern `push`) in verses, `pulse` 8ths in choruses. Drums: backbeat, 8th hats in verses, 16ths or open hats in choruses, fill every 8 bars.

**EDM / House / Dance** (120-128 BPM, techno 128-135). Minor keys common (A minor, F minor). Progressions: i-VI-III-VII, vi-IV-I-V, four chords looped for 8 bars. Chords: off-beat stabs at high energy, pads at low energy. Bass: `octave` or `pulse` (energy > 0.5). Drums: four-on-the-floor, open hat on every "and", claps layered with the snare, 16th hats when energy > 0.5. Arrangement: intro (drums thin) / build (energy 0.6, rising) / drop (energy 0.9-1.0, 16 bars) / break (energy 0.3) / drop. Timing tight: `humanize` with timing_ms 3, swing 0.5-0.55.

**Hip-hop / Boom-bap** (85-95 BPM). Sample-like 2- or 4-chord loops with 7ths (i7-iv7, ii7-V7-Imaj7). Bass: `push` or `root5`, sits with the kick. Drums: swung 16th hats (swing 0.58-0.62), kick patterns with a syncopated second kick, snare on 2 and 4 with ghosts, laid-back pocket (pocket_ms -4 to -6 on drums and bass).

**Trap / Drill** (130-150 BPM, half-time feel). Dark minor keys (C# minor, F minor, harmonic minor for drill). Sparse chords (pads or plucks holding), i-VI-VII-i or i-iv-VI-V. Bass: `808` (long sustained 808s with slides implied by octave drops). Drums: half-time snare on 3, rolling hats with 32nd bursts, occasional open hat. Melody: simple 4-note bell/pluck motif repeated with small variations; leave lots of space.

**Lo-fi** (70-90 BPM). Jazzy extended chords: Imaj7-vi7-ii7-V7, IVmaj7-iii7-vi7-ii7, borrow iv or bVII. Chord voicings close, played as held chords with a soft re-strike. Melody: short, lazy, pentatonic-ish phrases with rests; motif of 3-5 notes. Bass: `root5` or `push`, warm and simple. Drums: swing 0.6, soft velocities, ghosts, rim instead of snare when quiet, timing_ms 10-14 for a wobbly feel.

**R&B / Neo-soul** (65-95 BPM). Extended chords with 9ths and 13ths, ii-V movements, chromatic approach chords. Melody with passing tones and syncopation. Bass: `walking`-flavoured or `push`. Drums: swung, laid back.

**Cinematic / Orchestral / Ambient** (60-100 BPM). Modal colours: i-VI-III-VII (epic), I-bVII-IV-I (heroic), Lydian for wonder (I-II), Dorian for mystery. Chords: pads (energy < 0.5) or 4-voice sustained voicings with add9/sus2 colours, strummed slightly. Melody: long notes, arch contour, wider intervals allowed (5ths, octaves), phrases of 4 bars; use `contour: "arch"` or `"rise"`. Bass: `root` (whole notes) or `pedal` under changing chords for tension. Drums: sparse; toms and crashes only at phrase boundaries, low energy sections without drums (`clear` drums). Humanize loose: timing_ms 12-16, phrase_arc 0.6.

**Jazz** (swing 120-200, ballad 60-80). ii-V-I chains, secondary dominants (V/V), tritone subs. Bass: `walking`. Drums: ride pattern, swing 0.66. Melody: chord-tone arpeggios plus chromatic approach notes, syncopated.

**Folk / Acoustic** (90-120). I-IV-V, I-V-vi-IV, 3/4 or 6/8 options. Chords strummed on beats. Bass: `root5` (alternating bass). Drums: shaker and light kick, or none.

**Kids / Nursery rhymes** (95-115 BPM, sometimes 6/8 at 60-70). See the kids skill. Major keys (C, D, F, G), I-IV-V-I and I-V-I, plain triads. Melody within an octave, stepwise, repetitive, one syllable per note, clear cadences. Bass: `root` or `root5`. Drums: light (shaker, kick, soft snare or clap), no ghost notes, fills simple.

**Time signatures.** 3/4 waltz (kids, folk, cinematic), 6/8 (lullabies, folk, Irish), 5/4 for character. Set with `set_key_tempo` time_signature; drum patterns adapt to the beat count.
