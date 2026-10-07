//! Humanization: metric-aware, correlated timing drift; velocity contours; pocket offsets; swing;
//! gate-length variation. Everything is seeded and reproducible.

use crate::model::{Note, TrackRole, PPQ};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanizeParams {
    /// Overall timing looseness in milliseconds at the 16th-note level (downbeats get ~25% of this).
    pub timing_ms: f32,
    /// Static push (+) / laid-back (-) offset in ms for the whole part.
    pub pocket_ms: f32,
    /// Swing amount 0.5 (straight) ..= 0.75; applied to off-beat 8ths (or 16ths when `swing_16ths`).
    pub swing: f32,
    pub swing_16ths: bool,
    /// Velocity randomness as a fraction (0.05 = ±5%).
    pub vel_jitter: f32,
    /// Strength of metric accents (0..1).
    pub accent: f32,
    /// Phrase arc depth (0..1): crescendo into the last bar of each 4-bar phrase.
    pub phrase_arc: f32,
    /// Gate length variation fraction (0.1 = 90%..110%).
    pub gate_var: f32,
    /// How far the kit spreads around `pocket_ms` (kick behind, hats ahead). 0 = one offset for all.
    pub pocket_spread: f32,
    /// Backbeat placement on top of `pocket_ms`: negative pushes the snare, positive lays it back
    /// (hip-hop and R&B sit 10-20 ms behind, pop and rock a few ms ahead).
    pub snare_pocket_ms: f32,
    pub seed: u64,
}

impl Default for HumanizeParams {
    fn default() -> Self {
        HumanizeParams {
            timing_ms: 9.0,
            pocket_ms: 0.0,
            swing: 0.5,
            swing_16ths: false,
            vel_jitter: 0.05,
            accent: 0.6,
            phrase_arc: 0.45,
            gate_var: 0.08,
            pocket_spread: 1.0,
            snare_pocket_ms: 0.0,
            seed: 1,
        }
    }
}

impl HumanizeParams {
    /// Sensible defaults per role and style.
    pub fn preset(role: TrackRole, style: &str) -> Self {
        let s = style.to_ascii_lowercase();
        let mut p = HumanizeParams::default();
        match role.humanize_base() {
            TrackRole::Drums => {
                p.timing_ms = 6.0;
                p.accent = 0.7;
                p.gate_var = 0.0;
            }
            TrackRole::Bass => {
                p.timing_ms = 7.0;
                p.pocket_ms = -4.0;
                p.accent = 0.4;
            }
            TrackRole::Chords => {
                p.timing_ms = 10.0;
                p.accent = 0.3;
                p.gate_var = 0.1;
            }
            TrackRole::Melody => {
                p.timing_ms = 9.0;
                p.pocket_ms = 2.0;
                p.accent = 0.5;
                p.gate_var = 0.12;
            }
            _ => {}
        }
        let feel = StyleFeel::of(&s);
        p.swing = feel.swing;
        p.swing_16ths = feel.swing_16ths;
        p.timing_ms *= feel.timing_mul;
        p.snare_pocket_ms = feel.snare_pocket_ms;
        if role.humanize_base() == TrackRole::Drums {
            p.pocket_ms += feel.drum_pocket_ms;
        }
        if let Some(a) = feel.accent {
            p.accent = a;
        }
        if let Some(arc) = feel.phrase_arc {
            p.phrase_arc = arc;
        }
        p
    }
}

fn ms_to_ticks(ms: f32, tempo: f32) -> f32 {
    ms / 1000.0 * tempo / 60.0 * PPQ as f32
}

/// Metric weight of a tick position: 1.0 downbeat, 0.75 other beats, 0.5 off-beat 8ths, 0.25 16ths, 0.1 else.
pub fn metric_weight(tick: u32, bar_ticks: u32) -> f32 {
    let in_bar = tick % bar_ticks;
    if in_bar == 0 {
        1.0
    } else if in_bar % PPQ == 0 {
        0.75
    } else if in_bar % (PPQ / 2) == 0 {
        0.5
    } else if in_bar % (PPQ / 4) == 0 {
        0.25
    } else {
        0.1
    }
}

/// How a style sits against the grid. A table rather than an if/else chain so every style — including
/// the ones we have never heard of — gets a real feel instead of falling through to a dead-straight
/// default (which is what "pop", the default style, used to do).
#[derive(Debug, Clone, Copy)]
pub struct StyleFeel {
    pub swing: f32,
    pub swing_16ths: bool,
    pub timing_mul: f32,
    /// Applied to drum tracks on top of the role preset.
    pub drum_pocket_ms: f32,
    /// Backbeat placement: negative pushes, positive lays back.
    pub snare_pocket_ms: f32,
    pub accent: Option<f32>,
    pub phrase_arc: Option<f32>,
    /// Groove template name, applied before the random humanization.
    pub groove: Option<&'static str>,
}

impl StyleFeel {
    /// Straight but not mechanical: a hair of 16th shuffle and a pushed backbeat, which is how a
    /// pop record is actually played.
    pub const DEFAULT: StyleFeel = StyleFeel {
        swing: 0.52,
        swing_16ths: true,
        timing_mul: 1.0,
        drum_pocket_ms: 0.0,
        snare_pocket_ms: -3.0,
        accent: None,
        phrase_arc: None,
        groove: Some("straight_pop"),
    };

    pub fn of(style: &str) -> StyleFeel {
        let s = style.to_ascii_lowercase();
        let has = |keys: &[&str]| keys.iter().any(|k| s.contains(k));
        if has(&["lofi", "lo-fi", "boom"]) {
            StyleFeel { swing: 0.60, timing_mul: 1.4, drum_pocket_ms: -3.0, snare_pocket_ms: 12.0, groove: Some("boom_bap"), ..StyleFeel::DEFAULT }
        } else if has(&["trap", "drill"]) {
            StyleFeel { swing: 0.54, timing_mul: 1.1, drum_pocket_ms: -2.0, snare_pocket_ms: 8.0, groove: Some("swing_16"), ..StyleFeel::DEFAULT }
        } else if has(&["hip", "r&b", "rnb", "neo-soul", "neosoul", "soul"]) {
            StyleFeel { swing: 0.58, timing_mul: 1.3, drum_pocket_ms: -4.0, snare_pocket_ms: 14.0, groove: Some("boom_bap"), ..StyleFeel::DEFAULT }
        } else if has(&["house", "edm", "techno", "dance", "trance"]) {
            StyleFeel { swing: if s.contains("house") { 0.55 } else { 0.50 }, timing_mul: 0.5, snare_pocket_ms: -2.0, groove: Some("house"), ..StyleFeel::DEFAULT }
        } else if has(&["jazz", "swing", "bebop"]) {
            StyleFeel { swing: 0.66, swing_16ths: false, timing_mul: 1.3, snare_pocket_ms: 4.0, groove: Some("jazz_swing"), ..StyleFeel::DEFAULT }
        } else if has(&["cinema", "orchestra", "ambient", "epic", "score"]) {
            StyleFeel { swing: 0.5, swing_16ths: false, timing_mul: 1.6, snare_pocket_ms: 0.0, accent: Some(0.3), phrase_arc: Some(0.6), groove: None, ..StyleFeel::DEFAULT }
        } else if has(&["latin", "reggaeton", "salsa", "bossa", "afro"]) {
            StyleFeel { swing: 0.5, swing_16ths: false, timing_mul: 1.1, snare_pocket_ms: 0.0, groove: Some("tresillo"), ..StyleFeel::DEFAULT }
        } else if has(&["kid", "nursery", "rhyme", "lullab"]) {
            StyleFeel { swing: 0.52, swing_16ths: false, timing_mul: 0.8, snare_pocket_ms: 0.0, accent: Some(0.6), groove: None, ..StyleFeel::DEFAULT }
        } else if has(&["rock", "metal", "punk", "country", "folk"]) {
            StyleFeel { swing: 0.5, swing_16ths: false, timing_mul: 1.1, snare_pocket_ms: -5.0, groove: Some("straight_pop"), ..StyleFeel::DEFAULT }
        } else {
            StyleFeel::DEFAULT
        }
    }
}

/// Random-walk coefficients and the stationary deviation they produce, so `timing_ms` can be
/// normalised back to real milliseconds: sqrt(AR_B^2 * (1/3) / (1 - AR_A^2)).
const AR_A: f32 = 0.7;
const AR_B: f32 = 0.3;
const AR_SIGMA: f32 = 0.2425;

/// Where this note sits against the beat. A kit is not one instrument: the kick leans back, the
/// backbeat carries the style's feel, and the hats push. Pitched parts just use `pocket_ms`.
fn pocket_ms_for(p: &HumanizeParams, role: TrackRole, pitch: u8) -> f32 {
    if role.humanize_base() != TrackRole::Drums {
        return p.pocket_ms;
    }
    match pitch {
        35 | 36 => p.pocket_ms + 4.0 * p.pocket_spread,          // kick behind
        37..=40 => p.pocket_ms + p.snare_pocket_ms,              // backbeat, per style
        42 | 44 | 46 | 51 | 59 => p.pocket_ms - 3.0 * p.pocket_spread, // hats and ride push
        _ => p.pocket_ms,
    }
}

/// Applies humanization in place. `bar_ticks` is the bar length; `tempo` in BPM converts ms to ticks.
pub fn humanize(notes: &mut [Note], p: &HumanizeParams, role: TrackRole, tempo: f32, bar_ticks: u32) {
    if notes.is_empty() {
        return;
    }
    let mut rng = ChaCha8Rng::seed_from_u64(p.seed ^ (role as u64 + 1) * 0x9E37_79B9);
    notes.sort_by_key(|n| (n.start, n.pitch));
    let phrase = bar_ticks * 4;
    let total = notes.iter().map(|n| n.end()).max().unwrap_or(1).max(1);

    // Correlated timing drift per note (random walk pulled back to the grid).
    let mut drift = 0.0f32;
    let swing_unit = if p.swing_16ths { PPQ / 4 } else { PPQ / 2 };
    // A player is loosest where the grid is finest, but never four times tighter on a downbeat —
    // that reads as quantized. See `AR_SIGMA`: the walk is normalised so `timing_ms` is honest.
    let cap = ms_to_ticks((3.0 * p.timing_ms).min(45.0), tempo);
    let mut last_start = u32::MAX;
    let mut chord_shift = 0.0f32;
    let mut phrase_idx = u32::MAX;
    let mut phrase_wobble = 0.0f32;

    for n in notes.iter_mut() {
        let w = metric_weight(n.start, bar_ticks);
        let grid_start = n.start;

        // Swing: delay every other swing-unit.
        let mut shift = 0.0f32;
        if p.swing > 0.5 {
            let pos = grid_start % (swing_unit * 2);
            if pos == swing_unit {
                shift += (p.swing - 0.5) * 2.0 * swing_unit as f32;
            }
        }

        // Notes starting together (chords) move together, so decide the shift once per onset.
        if grid_start != last_start {
            // A new phrase resets most of the accumulated drift and picks up its own slight lean,
            // so phrase 2 does not simply continue phrase 1's wander.
            let this_phrase = grid_start / phrase.max(1);
            if this_phrase != phrase_idx {
                phrase_idx = this_phrase;
                drift *= 0.3;
                phrase_wobble = rng.random_range(-1.0..1.0) * 1.5;
            }
            let noise: f32 = rng.random_range(-1.0..1.0);
            drift = AR_A * drift + AR_B * noise;
            let scale = match w {
                x if x >= 1.0 => 0.55,
                x if x >= 0.75 => 0.7,
                x if x >= 0.5 => 0.9,
                _ => 1.0,
            };
            let t = (drift / AR_SIGMA) * ms_to_ticks(p.timing_ms, tempo) * scale;
            chord_shift = t.clamp(-cap, cap) + ms_to_ticks(pocket_ms_for(p, role, n.pitch) + phrase_wobble, tempo);
            last_start = grid_start;
        }
        shift += chord_shift;
        let new_start = (grid_start as f32 + shift).round().max(0.0) as u32;
        let delta = new_start as i64 - n.start as i64;
        n.start = new_start;
        // Keep the note end roughly where it was for positive shifts (avoid bleeding into the next note).
        if delta > 0 {
            n.len = (n.len as i64 - delta).max(PPQ as i64 / 16) as u32;
        }

        // Velocity: accent pattern + phrase arc + small jitter.
        let accent = (w - 0.5) * 0.3 * p.accent; // -0.12 .. +0.15
        let pos_in_phrase = (grid_start % phrase) as f32 / phrase as f32;
        let arc = if pos_in_phrase > 0.75 { (pos_in_phrase - 0.75) * 4.0 * 0.15 * p.phrase_arc } else { 0.0 };
        let song_pos = grid_start as f32 / total as f32;
        let song_arc = (song_pos - 0.5) * 0.06 * p.phrase_arc;
        let jitter: f32 = rng.random_range(-1.0..1.0) * p.vel_jitter;
        let is_ghost = n.vel < 0.45;
        let mut v = n.vel + if is_ghost { jitter * 1.5 } else { accent + arc + song_arc + jitter };
        if is_ghost {
            v = v.min(0.5);
        }
        n.vel = v.clamp(0.05, 1.0);

        // Gate length.
        if p.gate_var > 0.0 && role.humanize_base() != TrackRole::Drums {
            let g: f32 = 1.0 + rng.random_range(-1.0..1.0) * p.gate_var + (n.vel - 0.7) * 0.15;
            n.len = ((n.len as f32) * g.clamp(0.6, 1.15)).round().max(PPQ as f32 / 16.0) as u32;
        }
    }
    // Ensure legato parts don't overlap the next same-pitch note.
    let len = notes.len();
    for i in 0..len {
        for j in i + 1..len {
            if notes[j].start > notes[i].end() {
                break;
            }
            if notes[j].pitch == notes[i].pitch && notes[j].start < notes[i].end() {
                notes[i].len = notes[j].start.saturating_sub(notes[i].start).max(PPQ / 32);
            }
        }
    }
}

/// Extracts a groove template (per-16th timing offsets in ticks and velocity multipliers) from notes,
/// e.g. an imported MIDI loop, averaged across bars.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Groove {
    pub offsets: Vec<f32>,
    pub vel_mul: Vec<f32>,
}

pub fn extract_groove(notes: &[Note], bar_ticks: u32) -> Groove {
    let step = PPQ / 4;
    let n = (bar_ticks / step) as usize;
    let mut off = vec![0.0f32; n];
    let mut vel = vec![0.0f32; n];
    let mut cnt = vec![0u32; n];
    for note in notes {
        let in_bar = note.start % bar_ticks;
        let slot = ((in_bar + step / 2) / step) as usize % n;
        let grid = slot as u32 * step;
        off[slot] += in_bar as f32 - grid as f32;
        vel[slot] += note.vel;
        cnt[slot] += 1;
    }
    let mean_vel = {
        let (s, c): (f32, u32) = notes.iter().fold((0.0, 0), |(s, c), n| (s + n.vel, c + 1));
        if c > 0 { s / c as f32 } else { 0.8 }
    };
    for i in 0..n {
        if cnt[i] > 0 {
            off[i] /= cnt[i] as f32;
            vel[i] = (vel[i] / cnt[i] as f32) / mean_vel;
        } else {
            vel[i] = 1.0;
        }
    }
    Groove { offsets: off, vel_mul: vel }
}

/// Built-in feels, as 16 per-16th (offset in ms, velocity multiplier) pairs. These are the
/// systematic part of a groove — where a style consistently sits against the grid — so they are
/// applied to quantized notes *before* [`humanize`] wanders around them.
pub fn groove_template(name: &str, role: TrackRole, tempo: f32) -> Option<Groove> {
    let drums = role.humanize_base() == TrackRole::Drums;
    // (ms offset, velocity multiplier) for the 16 sixteenths of a bar.
    let cells: [(f32, f32); 16] = match name {
        // Straight, but with the off-16ths a touch early and softer — the "80/20" hit distribution.
        "straight_pop" => [
            (0.0, 1.00), (-2.0, 0.82), (-1.0, 0.90), (-2.0, 0.80),
            (0.0, 0.96), (-2.0, 0.82), (-1.0, 0.88), (-2.0, 0.80),
            (0.0, 0.98), (-2.0, 0.82), (-1.0, 0.90), (-2.0, 0.80),
            (0.0, 0.94), (-2.0, 0.82), (-1.0, 0.88), (-2.0, 0.78),
        ],
        // MPC-style 16th swing: every other 16th late and quieter.
        "swing_16" => [
            (0.0, 1.00), (22.0, 0.72), (0.0, 0.88), (22.0, 0.74),
            (0.0, 0.96), (22.0, 0.72), (0.0, 0.86), (22.0, 0.74),
            (0.0, 0.98), (22.0, 0.72), (0.0, 0.88), (22.0, 0.74),
            (0.0, 0.94), (22.0, 0.70), (0.0, 0.86), (22.0, 0.72),
        ],
        // Behind-the-beat hip-hop/R&B: heavy swing plus a dragged backbeat.
        "boom_bap" => [
            (0.0, 1.00), (30.0, 0.68), (2.0, 0.85), (30.0, 0.70),
            (10.0, 0.98), (30.0, 0.68), (2.0, 0.84), (30.0, 0.70),
            (0.0, 0.98), (30.0, 0.68), (2.0, 0.85), (30.0, 0.70),
            (10.0, 0.94), (30.0, 0.66), (2.0, 0.84), (28.0, 0.70),
        ],
        // Four-to-the-floor: dead-on kicks, off-8ths pushed slightly early.
        "house" => [
            (0.0, 1.00), (-3.0, 0.86), (-3.0, 0.92), (-3.0, 0.86),
            (0.0, 1.00), (-3.0, 0.86), (-3.0, 0.90), (-3.0, 0.86),
            (0.0, 1.00), (-3.0, 0.86), (-3.0, 0.92), (-3.0, 0.86),
            (0.0, 1.00), (-3.0, 0.86), (-3.0, 0.90), (-3.0, 0.84),
        ],
        // Triplet feel: the second eighth two thirds of the way through the beat.
        "jazz_swing" => [
            (0.0, 1.00), (0.0, 0.70), (55.0, 0.80), (0.0, 0.70),
            (0.0, 0.92), (0.0, 0.70), (55.0, 0.86), (0.0, 0.70),
            (0.0, 0.96), (0.0, 0.70), (55.0, 0.80), (0.0, 0.70),
            (0.0, 0.92), (0.0, 0.70), (55.0, 0.86), (0.0, 0.70),
        ],
        // 3+3+2 accent grouping (tresillo): on the grid, but the weight moves.
        "tresillo" => [
            (0.0, 1.15), (0.0, 0.78), (0.0, 0.85), (0.0, 0.80),
            (0.0, 0.82), (0.0, 0.78), (0.0, 1.12), (0.0, 0.80),
            (0.0, 0.84), (0.0, 0.78), (0.0, 0.86), (0.0, 0.80),
            (0.0, 1.10), (0.0, 0.78), (0.0, 0.86), (0.0, 0.80),
        ],
        _ => return None,
    };
    // Pitched parts follow the feel's timing but keep their own dynamics; only a kit wants the
    // velocity mask, or a pad ends up pumping.
    let offsets = cells.iter().map(|(ms, _)| ms_to_ticks(*ms, tempo)).collect();
    let vel_mul = cells.iter().map(|(_, v)| if drums { *v } else { 1.0 + (*v - 1.0) * 0.3 }).collect();
    Some(Groove { offsets, vel_mul })
}

pub fn apply_groove(notes: &mut [Note], groove: &Groove, strength: f32, bar_ticks: u32) {
    let step = PPQ / 4;
    let n = groove.offsets.len().max(1);
    for note in notes.iter_mut() {
        let in_bar = note.start % bar_ticks;
        let slot = ((in_bar + step / 2) / step) as usize % n;
        let shift = groove.offsets[slot] * strength;
        note.start = (note.start as f32 + shift).round().max(0.0) as u32;
        note.vel = (note.vel * (1.0 + (groove.vel_mul[slot] - 1.0) * strength)).clamp(0.05, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_is_bounded_and_deterministic() {
        let mk = || (0..32).map(|i| Note::new(60 + (i % 5) as u8, i * PPQ / 2, PPQ / 2, 0.8)).collect::<Vec<_>>();
        let mut a = mk();
        let mut b = mk();
        let p = HumanizeParams { timing_ms: 10.0, ..Default::default() };
        humanize(&mut a, &p, TrackRole::Melody, 120.0, PPQ * 4);
        humanize(&mut b, &p, TrackRole::Melody, 120.0, PPQ * 4);
        assert_eq!(a, b);
        let grid = mk();
        let max_ticks = ms_to_ticks(3.0 * p.timing_ms + p.pocket_ms.abs(), 120.0) as i64 + 1;
        for (h, g) in a.iter().zip(&grid) {
            assert!((h.start as i64 - g.start as i64).abs() <= max_ticks);
            assert!(h.vel > 0.5 && h.vel <= 1.0);
        }
        let distinct: std::collections::BTreeSet<u32> = a.iter().map(|n| n.start).collect();
        assert!(distinct.len() > 20, "timing should vary");
        let vels: std::collections::BTreeSet<u8> = a.iter().map(|n| n.vel_midi()).collect();
        assert!(vels.len() > 8, "velocity should vary");
    }

    #[test]
    fn swing_delays_offbeats() {
        let mut n = vec![Note::new(60, 0, PPQ / 2, 0.8), Note::new(60, PPQ / 2, PPQ / 2, 0.8)];
        let p = HumanizeParams { timing_ms: 0.0, swing: 0.66, vel_jitter: 0.0, gate_var: 0.0, ..Default::default() };
        humanize(&mut n, &p, TrackRole::Drums, 120.0, PPQ * 4);
        assert!(n[1].start > PPQ / 2 + 100);
    }
}
