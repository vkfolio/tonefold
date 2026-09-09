## Notation quick reference

- Chords: `| C | Am | F | G |`, `| I | vi | IV | V |`, `| C . Am . |` (half bars), `| Dm7 G7 | Cmaj7 . |`, slash `C/E`, `bVII`, `V/V`.
- Melody/bass: `E4:8 G4:8 A4:4 r:8 G4:8~ G4:4 E4:2` (pitch:duration; 4 = quarter, 8 = eighth, `.` dotted, `t` triplet, `r` rest, `~` tie, `@v90` velocity, `/la` lyric). Bar lines `|` are optional. Middle C is C4.
- Drums (16th steps, 16 per 4/4 bar): `K: x---x---x---x---`, `S: ----X-------X-g-`, `H: x-x-x-x-x-x-x-x-`, `OH: -------x--------`, `CL:`, `RS:`, `T1/T2/T3:`, `RD:`, `CR:`, `P:`. `X` accent, `g` ghost, `f` flam.

**Melodic notation is one voice.** `set_notes` writes a sequence, not a stack: brackets, commas and
`+` are not chord syntax, and a second line replaces rather than layers. Simultaneous notes come
from `set_chords` plus `generate` on a chord or pad layer — that is what voice-leads — or from
putting the parts on separate layers. Do not spend turns trying to hand-write a voicing.

Sections are addressed by id or name (case-insensitive). Layers are addressed by id, name or kind:
chords, pad, arpeggio, pluck, melody, counter_melody, harmony, bass, sub, drums, percussion — a new
session starts with only chords, melody, bass and drums, so `add_layer` when the arrangement needs
more (`list_layer_kinds` describes each one).
