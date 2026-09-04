//! Voice leading: turns a chord sequence into concrete voicings with minimal voice motion,
//! common-tone retention, register limits and parallel fifth/octave avoidance against the bass.

use crate::model::{ChordEvent, Note};
use crate::theory::Chord;

#[derive(Debug, Clone)]
pub struct VoicingParams {
    pub lo: u8,
    pub hi: u8,
    /// Number of voices (3 or 4 typical).
    pub voices: usize,
    /// Prefer keeping the root out of the top voice.
    pub avoid_root_on_top: bool,
    /// Add the chord root an octave below the voicing (piano-style left hand).
    pub add_low_root: bool,
}

impl Default for VoicingParams {
    fn default() -> Self {
        VoicingParams { lo: 52, hi: 76, voices: 4, avoid_root_on_top: true, add_low_root: false }
    }
}

/// All voicings of `chord` inside [lo, hi] using `voices` distinct pitches (every chord tone at least once
/// when possible), sorted ascending.
fn candidate_voicings(chord: &Chord, p: &VoicingParams) -> Vec<Vec<u8>> {
    let tones = chord.tones_in_range(p.lo, p.hi);
    let pcs = chord.pitch_classes();
    let mut out = Vec::new();
    let k = p.voices.min(tones.len());
    if k == 0 {
        return out;
    }
    // Enumerate combinations (tones are few, so this is cheap: C(n, k) with n <= ~12).
    let mut idx: Vec<usize> = (0..k).collect();
    loop {
        let v: Vec<u8> = idx.iter().map(|&i| tones[i]).collect();
        // Coverage: prefer voicings that include root, third (if any) and at least min(pcs, k) distinct classes.
        let distinct: std::collections::BTreeSet<u8> = v.iter().map(|x| x % 12).collect();
        let need = pcs.len().min(k);
        let has_root = distinct.contains(&chord.root);
        if distinct.len() >= need.saturating_sub(1) && (has_root || k < 3) {
            // No adjacent semitone clusters at the bottom, spread limit in the low register.
            let ok_spacing = v.windows(2).all(|w| w[1] - w[0] >= 2 || w[0] > 60) && (v[1] - v[0] <= 12 || v[0] > 55);
            if ok_spacing {
                out.push(v);
            }
        }
        // Next combination.
        let mut i = k;
        loop {
            if i == 0 {
                return out;
            }
            i -= 1;
            if idx[i] < tones.len() - (k - i) {
                idx[i] += 1;
                for j in i + 1..k {
                    idx[j] = idx[j - 1] + 1;
                }
                break;
            }
        }
    }
}

fn motion_cost(prev: &[u8], next: &[u8], prev_bass: Option<u8>, next_bass: Option<u8>) -> i32 {
    // Pair voices by index (both sorted) and sum absolute motion, penalising large leaps and parallels.
    let mut cost = 0i32;
    let n = prev.len().min(next.len());
    let mut common = 0;
    for i in 0..n {
        let d = (next[i] as i32 - prev[i] as i32).abs();
        cost += d * d / 2 + d;
        if d == 0 {
            common += 1;
        }
    }
    cost -= common * 3;
    if let (Some(pb), Some(nb)) = (prev_bass, next_bass) {
        let bass_move = nb as i32 - pb as i32;
        if bass_move != 0 {
            for i in 0..n {
                let vm = next[i] as i32 - prev[i] as i32;
                let iv_prev = (prev[i] as i32 - pb as i32).rem_euclid(12);
                let iv_next = (next[i] as i32 - nb as i32).rem_euclid(12);
                if vm != 0 && vm.signum() == bass_move.signum() && iv_prev == iv_next && (iv_prev == 7 || iv_prev == 0) {
                    cost += 40; // parallel fifth/octave with the bass
                }
            }
        }
    }
    cost
}

/// Chooses a voicing per chord event with a greedy-with-lookahead search (beam of 4).
pub fn voice_lead(events: &[ChordEvent], p: &VoicingParams) -> Vec<Vec<u8>> {
    let mut beams: Vec<(i32, Vec<Vec<u8>>)> = vec![(0, Vec::new())];
    let center = (p.lo as i32 + p.hi as i32) / 2;
    for ev in events {
        let cands = candidate_voicings(&ev.chord, p);
        if cands.is_empty() {
            for b in &mut beams {
                b.1.push(vec![]);
            }
            continue;
        }
        let bass = Some(ev.chord.bass_pc());
        let mut next: Vec<(i32, Vec<Vec<u8>>)> = Vec::new();
        for (score, path) in &beams {
            for c in &cands {
                let mut cost = *score;
                match path.last().filter(|v| !v.is_empty()) {
                    Some(prev) => {
                        let prev_bass = events.get(path.len() - 1).map(|e| e.chord.bass_pc());
                        cost += motion_cost(prev, c, prev_bass, bass);
                    }
                    None => {
                        // First chord: prefer a centred, compact voicing.
                        let mid = c.iter().map(|&x| x as i32).sum::<i32>() / c.len() as i32;
                        cost += (mid - center).abs() * 2;
                    }
                }
                if p.avoid_root_on_top && c.last().map(|t| t % 12 == ev.chord.root).unwrap_or(false) {
                    cost += 4;
                }
                // Keep the top voice away from the extremes.
                let top = *c.last().unwrap() as i32;
                cost += ((top - (p.hi as i32 - 4)).max(0) + (p.lo as i32 + 4 - *c.first().unwrap() as i32).max(0)) * 2;
                let mut np = path.clone();
                np.push(c.clone());
                next.push((cost, np));
            }
        }
        next.sort_by_key(|(c, _)| *c);
        next.truncate(4);
        beams = next;
    }
    let mut best = beams.into_iter().min_by_key(|(c, _)| *c).map(|(_, p)| p).unwrap_or_default();
    if p.add_low_root {
        for (v, ev) in best.iter_mut().zip(events) {
            if let Some(&lowest) = v.first() {
                let root = ev.chord.bass_pc();
                let mut r = lowest.saturating_sub(12);
                while r % 12 != root {
                    r = r.saturating_sub(1);
                }
                if r >= 36 {
                    v.insert(0, r);
                }
            }
        }
    }
    best
}

/// Renders voiced chords into notes (block chords, one note per voice per event).
pub fn render_block_chords(events: &[ChordEvent], voicings: &[Vec<u8>], vel: f32) -> Vec<Note> {
    let mut notes = Vec::new();
    for (ev, v) in events.iter().zip(voicings) {
        for &pitch in v {
            notes.push(Note::new(pitch, ev.start, ev.len.saturating_sub(10).max(1), vel));
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PPQ;
    use crate::theory::{Key, ScaleKind};

    #[test]
    fn leads_smoothly() {
        let key = Key::new(0, ScaleKind::Major);
        let ev = crate::notation::parse_chords("| C | Am | F | G |", &key, 4, PPQ * 4).unwrap();
        let v = voice_lead(&ev, &VoicingParams::default());
        assert_eq!(v.len(), 4);
        for w in v.windows(2) {
            let total: i32 = w[0].iter().zip(&w[1]).map(|(a, b)| (*a as i32 - *b as i32).abs()).sum();
            assert!(total <= 12, "too much motion: {:?} -> {:?}", w[0], w[1]);
        }
        for (voicing, e) in v.iter().zip(&ev) {
            assert!(voicing.iter().all(|p| e.chord.contains(*p)));
        }
    }
}
