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
    pub seed: u64,
}

impl Default for HumanizeParams {
    fn default() -> Self {
        HumanizeParams {
            timing_ms: 8.0,
            pocket_ms: 0.0,
            swing: 0.5,
            swing_16ths: false,
            vel_jitter: 0.06,
            accent: 0.5,
            phrase_arc: 0.3,
            gate_var: 0.08,
            seed: 1,
        }
    }
}

impl HumanizeParams {
    /// Sensible defaults per role and style.
    pub fn preset(role: TrackRole, style: &str) -> Self {
        let s = style.to_ascii_lowercase();
        let mut p = HumanizeParams::default();
        match role {
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
        }
        if s.contains("lofi") || s.contains("lo-fi") || s.contains("hip") || s.contains("trap") || s.contains("boom") {
            p.swing = if s.contains("trap") { 0.54 } else { 0.6 };
            p.swing_16ths = true;
            p.timing_ms *= 1.4;
            if role == TrackRole::Drums {
                p.pocket_ms = -3.0;
            }
        } else if s.contains("house") || s.contains("edm") || s.contains("techno") || s.contains("dance") {
            p.timing_ms *= 0.5;
            p.swing = if s.contains("house") { 0.55 } else { 0.5 };
            p.swing_16ths = true;
        } else if s.contains("jazz") || s.contains("swing") {
            p.swing = 0.66;
            p.timing_ms *= 1.3;
        } else if s.contains("cinema") || s.contains("orchestra") || s.contains("ambient") {
            p.timing_ms *= 1.6;
            p.accent = 0.3;
            p.phrase_arc = 0.6;
        } else if s.contains("kid") || s.contains("nursery") || s.contains("rhyme") {
            p.timing_ms *= 0.8;
            p.swing = 0.52;
            p.accent = 0.6;
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
    let max_shift = ms_to_ticks(20.0, tempo);
    let mut last_start = u32::MAX;
    let mut chord_shift = 0.0f32;

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
            let noise: f32 = rng.random_range(-1.0..1.0);
            drift = 0.7 * drift + 0.3 * noise;
            let scale = match w {
                x if x >= 1.0 => 0.25,
                x if x >= 0.75 => 0.4,
                x if x >= 0.5 => 0.7,
                _ => 1.0,
            };
            let t = drift * ms_to_ticks(p.timing_ms, tempo) * scale;
            chord_shift = t.clamp(-max_shift, max_shift) + ms_to_ticks(p.pocket_ms, tempo);
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
        if p.gate_var > 0.0 && role != TrackRole::Drums {
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
        let max_ticks = ms_to_ticks(20.0, 120.0) as i64 + 1;
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
