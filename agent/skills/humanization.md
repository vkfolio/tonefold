What makes MIDI sound played rather than programmed, and the knobs that control it.

The engine applies, per track, a style preset (chosen from the session style) with these components:

- **Metric-aware timing.** Downbeats stay tight (about 25% of `timing_ms`), other beats 40%, off-beat 8ths 70%, 16ths 100%. Offsets follow a random walk (each note's offset is 70% of the previous plus new noise) so the player drifts rather than stumbles, capped at ±20 ms. Notes that start together (chords) move together.
- **Pocket.** `pocket_ms` shifts a whole part: negative = behind the beat (laid back: hip-hop drums and bass -4 to -8), positive = ahead (urgent: pushy melodies +2 to +4).
- **Swing.** `swing` 0.5 = straight, 0.55 light house shuffle, 0.58-0.62 hip-hop/lo-fi, 0.66 jazz triplet feel. `swing_16ths` true swings 16ths (hip-hop, house hats), false swings 8ths (jazz, shuffle).
- **Velocity contours.** Accents from metric position (`accent`), a crescendo into the last bar of each 4-bar phrase and a slight rise across the section (`phrase_arc`), plus only a little noise (`vel_jitter` 0.04-0.08). Ghost notes (velocity < 0.45) are kept ghostly.
- **Gate length.** `gate_var` scales note lengths 85-115% correlated with velocity (louder = longer). Drums are never gated.

Presets: drums timing 6 ms, bass 7 ms with pocket -4, chords 10 ms, melody 9 ms with pocket +2. EDM halves the timing looseness; lo-fi/hip-hop multiplies it by 1.4 and adds swing; cinematic multiplies by 1.6 and deepens the phrase arc.

Use `humanize` with overrides when the user asks for tighter/looser/swung/laid-back results, or after `set_notes` if you passed humanize=false. Typical requests:
- "tighter" → timing_ms 3, vel_jitter 0.03.
- "more human / loose" → timing_ms 12, gate_var 0.15.
- "swing it" → swing 0.6, swing_16ths true.
- "lazy / behind the beat" → pocket_ms -6 on drums and bass.
- "robotic on purpose" (some EDM) → timing_ms 0, vel_jitter 0, accent 0.2.

Beyond the knobs, feel comes from writing: ghost notes and hat dynamics in drums, rests between phrases, longer notes at cadences, motif repetition, and fewer notes when other parts are busy.
