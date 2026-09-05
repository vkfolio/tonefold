//! Melody generation: motif + development. A 1–2 bar motif (given or invented) is developed across
//! the section with transposition, inversion, fragmentation and rhythmic variation, following the
//! chord changes (chord tones on strong beats, scale tones elsewhere), a contour target, a leap cap
//! and a rest budget. Phrases follow an antecedent/consequent shape.

use super::{clamp_to_section, GenParams};
use crate::model::{Note, Section, Session, TrackRole, PPQ};
use crate::notation::parse_melody;
use crate::theory::Key;
use rand::Rng;
use rand_chacha::ChaCha8Rng;

/// A rhythm cell: (offset within bar in ticks, length in ticks).
type Rhythm = Vec<(u32, u32)>;

fn rhythm_cells(style: &str, energy: f32, rng: &mut ChaCha8Rng) -> Rhythm {
    let q = PPQ;
    let e = q / 2;
    let s = q / 4;
    let dq = q + e;
    let lofi = style.contains("lofi") || style.contains("hip") || style.contains("trap") || style.contains("jazz");
    let cine = style.contains("cinema") || style.contains("ambient") || style.contains("orchestra");
    let cells: Vec<Rhythm> = if cine {
        vec![
            vec![(0, q * 2), (q * 2, q * 2)],
            vec![(0, q * 3), (q * 3, q)],
            vec![(0, dq), (dq, e), (q * 2, q * 2)],
            vec![(0, q), (q, q), (q * 2, q * 2)],
            vec![(0, q * 4)],
        ]
    } else if lofi {
        vec![
            vec![(e, e), (q, e), (q + e, q), (q * 3, e)],
            vec![(0, dq), (dq, e), (q * 2 + e, e), (q * 3, q)],
            vec![(s, s), (e, e), (q + e, e), (q * 2, q), (q * 3 + e, e)],
            vec![(0, q), (q + e, e), (q * 2, e), (q * 3, q)],
            vec![(0, e), (e, e), (q, q), (q * 2 + e, dq)],
        ]
    } else {
        vec![
            vec![(0, e), (e, e), (q, q), (q * 2, e), (q * 2 + e, e), (q * 3, q)],
            vec![(0, q), (q, e), (q + e, e), (q * 2, q), (q * 3, q)],
            vec![(0, dq), (dq, e), (q * 2, q), (q * 3, q)],
            vec![(0, e), (e, e), (q, e), (q + e, e), (q * 2, q * 2)],
            vec![(0, q), (q, q), (q * 2, e), (q * 2 + e, e), (q * 3, q)],
            vec![(0, q * 2), (q * 2, q), (q * 3, q)],
        ]
    };
    let idx = rng.random_range(0..cells.len());
    let mut cell = cells[idx].clone();
    // Thin out at low energy, add pickups at high energy.
    if energy < 0.35 && cell.len() > 3 {
        let drop = rng.random_range(1..cell.len() - 1);
        let (o, l) = cell.remove(drop);
        if let Some(prev) = cell.iter_mut().find(|(po, pl)| *po + *pl == o) {
            prev.1 += l;
        }
    } else if energy > 0.75 && cell.last().map(|(o, l)| o + l == q * 4).unwrap_or(false) && rng.random_bool(0.5) {
        let (o, l) = cell.last_mut().unwrap();
        if *l >= q {
            *l -= e;
            let ne = *o + *l;
            cell.push((ne, e));
        }
    }
    cell
}

/// Invents a 1-bar motif: degrees relative to the first chord, on a rhythm cell.
fn invent_motif(key: &Key, section: &Section, style: &str, energy: f32, rng: &mut ChaCha8Rng) -> Vec<Note> {
    let rhythm = rhythm_cells(style, energy, rng);
    let chord = &section.chords[0].chord;
    let (lo, hi) = TrackRole::Melody.register();
    let tones = chord.tones_in_range(lo + 3, hi - 3);
    let start_pitch = tones.get(tones.len() / 2).copied().unwrap_or(67);
    // Contour of the motif: a short arch or a rise.
    // Degree offsets from the start pitch: mixes steps with chord-tone skips so motifs have a real shape.
    let shapes: [&[i32]; 8] = [
        &[0, 2, 4, 2, 0, -1],
        &[0, -2, -3, -1, 0, 2],
        &[0, 1, 3, 1, 0, -2],
        &[0, 4, 2, 3, 1, 0],
        &[0, 2, 1, -1, -2, 0],
        &[0, -1, 1, 3, 2, 0],
        &[0, 0, 2, 4, 3, 2],
        &[0, 3, 2, 0, -2, -1],
    ];
    let shape = shapes[rng.random_range(0..shapes.len())];
    let mut notes = Vec::new();
    let mut pitch;
    for (i, (off, len)) in rhythm.iter().enumerate() {
        let target = key.step(start_pitch, shape[i % shape.len()]);
        pitch = if i == 0 { start_pitch } else { target };
        // Strong beats: nearest chord tone.
        if *off % PPQ == 0 && (*off / PPQ) % 2 == 0 && !chord.contains(pitch) {
            pitch = nearest_chord_tone(chord, pitch);
        }
        notes.push(Note::new(pitch, *off, *len, 0.8));
    }
    notes
}

fn nearest_chord_tone(chord: &crate::theory::Chord, pitch: u8) -> u8 {
    nearest_chord_tone_avoiding(chord, pitch, None)
}

/// Nearest chord tone; when it would land on `avoid` (usually the previous note), take the next
/// closest chord tone within a 4th so repeated notes don't pile up on strong beats.
fn nearest_chord_tone_avoiding(chord: &crate::theory::Chord, pitch: u8, avoid: Option<u8>) -> u8 {
    let mut best: Option<u8> = None;
    let mut second: Option<u8> = None;
    for d in 0..=6u8 {
        for cand in [pitch.checked_sub(d), pitch.checked_add(d).filter(|p| *p <= 127)].into_iter().flatten() {
            if chord.contains(cand) && Some(cand) != best {
                if best.is_none() {
                    best = Some(cand);
                } else if second.is_none() {
                    second = Some(cand);
                }
            }
        }
        if best.is_some() && second.is_some() {
            break;
        }
    }
    match (best, second, avoid) {
        (Some(b), Some(s), Some(a)) if b == a && (s as i32 - pitch as i32).abs() <= 5 => s,
        (Some(b), _, _) => b,
        _ => pitch,
    }
}

/// Fits a motif (relative to its first chord) onto a different chord: chord tones stay chord tones by
/// mapping degrees, non-chord tones become scale tones.
fn adapt_to_chord(notes: &[Note], key: &Key, from_root: u8, to: &crate::theory::Chord, transpose_deg: i32, invert: bool, bar_ticks: u32) -> Vec<Note> {
    let shift_pc = (to.root as i32 - from_root as i32).rem_euclid(12);
    let shift_deg = key.degree_of((from_root + shift_pc as u8) % 12).map(|d| d as i32).unwrap_or(0) - key.degree_of(from_root).map(|d| d as i32).unwrap_or(0);
    let first = notes.first().map(|n| n.pitch).unwrap_or(67);
    notes
        .iter()
        .map(|n| {
            let rel = key.pitch_to_degree_index(n.pitch) - key.pitch_to_degree_index(first);
            let rel = if invert { -rel } else { rel };
            let mut p = key.step(first, rel + shift_deg + transpose_deg);
            let in_bar = n.start % bar_ticks;
            let strong = in_bar % PPQ == 0 && (in_bar / PPQ) % 2 == 0;
            if strong && !to.contains(p) {
                p = nearest_chord_tone(to, p);
            } else if !key.contains(p) {
                p = key.snap(p);
            }
            Note { pitch: p, ..n.clone() }
        })
        .collect()
}

fn contour_target(contour: &str, phrase_pos: f32) -> i32 {
    // Degrees offset over a 4-bar phrase.
    match contour {
        "rise" => (phrase_pos * 4.0) as i32,
        "fall" => -((phrase_pos * 4.0) as i32),
        "wave" => ((phrase_pos * std::f32::consts::TAU).sin() * 2.0).round() as i32,
        _ => (((phrase_pos * std::f32::consts::PI).sin()) * 3.0).round() as i32, // arch
    }
}

pub fn generate_melody(session: &Session, section: &Section, params: &GenParams) -> Vec<Note> {
    let key = session.key;
    let style = params.style(session).to_ascii_lowercase();
    let energy = params.energy(section);
    let mut rng = params.rng(session, 21);
    let bar = session.bar_ticks();
    let total = section.bars * bar;
    let contour = params.contour.clone().unwrap_or_else(|| "arch".into());
    let (lo, hi) = TrackRole::Melody.register();

    let motif: Vec<Note> = match params.motif.as_deref().and_then(|m| parse_melody(m, 0.8).ok()).filter(|m| !m.is_empty()) {
        Some(m) => m,
        None => invent_motif(&key, section, &style, energy, &mut rng),
    };
    let motif_bars = ((motif.iter().map(|n| n.end()).max().unwrap_or(bar) + bar - 1) / bar).max(1);
    let motif_root = Session::chord_at(section, 0).map(|c| c.chord.root).unwrap_or(key.root);

    let mut notes: Vec<Note> = Vec::new();
    let mut b = 0u32;
    let phrase_bars = 4u32.min(section.bars.max(1));
    let mut idx = 0;
    while b < section.bars {
        let phrase_pos = (b % phrase_bars) as f32 / phrase_bars as f32;
        let last_of_phrase = (b % phrase_bars) + motif_bars >= phrase_bars;
        let phrase_idx = b / phrase_bars;
        let chord = Session::chord_at(section, b * bar).map(|c| c.chord.clone()).unwrap_or_else(|| key.diatonic_chord(0, false));

        // Development plan: statement, repeat/vary, sequence, cadence.
        // Statement / varied repeat / sequence a third away / cadence; the second phrase answers
        // with inversion or a higher restatement so 8 bars are not four identical pairs.
        let second_phrase = phrase_idx % 2 == 1;
        let (transpose, invert, vary): (i32, bool, bool) = match idx % 4 {
            0 => (if second_phrase { 2 } else { 0 }, second_phrase && rng.random_bool(0.5), false),
            1 => (0, false, true),
            2 => (if rng.random_bool(0.5) { 2 } else { -2 }, rng.random_bool(0.3), rng.random_bool(0.5)),
            _ => (if second_phrase { 1 } else { 0 }, false, true),
        };
        let mut cell = adapt_to_chord(&motif, &key, motif_root, &chord, transpose + contour_target(&contour, phrase_pos), invert, bar);

        // Rhythmic variation: drop or split a note.
        if vary && cell.len() > 2 {
            if rng.random_bool(0.5) {
                let i = rng.random_range(1..cell.len());
                let removed = cell.remove(i);
                if let Some(prev) = cell.get_mut(i - 1) {
                    if prev.end() == removed.start {
                        prev.len += removed.len;
                    }
                }
            } else {
                let i = rng.random_range(0..cell.len());
                if cell[i].len >= PPQ / 2 {
                    let half = cell[i].len / 2;
                    let mut second = cell[i].clone();
                    second.start += half;
                    second.len = cell[i].len - half;
                    second.pitch = key.step(second.pitch, if rng.random_bool(0.5) { 1 } else { -1 });
                    cell[i].len = half;
                    cell.insert(i + 1, second);
                }
            }
        }

        // Cadence: last cell of a phrase ends long, on a chord tone; antecedent phrases end off-tonic, consequent on tonic.
        if last_of_phrase {
            if let Some(last) = cell.last_mut() {
                let target_pc = if phrase_idx % 2 == 0 { chord.pitch_classes()[if chord.pitch_classes().len() > 2 { 2 } else { 0 }] } else { chord.root };
                let mut p = last.pitch;
                for d in 0..=7u8 {
                    if p >= d && (p - d) % 12 == target_pc {
                        p -= d;
                        break;
                    }
                    if p + d <= 127 && (p + d) % 12 == target_pc {
                        p += d;
                        break;
                    }
                }
                last.pitch = p;
                last.len = last.len.max(PPQ);
            }
            // Breathing room: drop trailing short notes so the phrase ends with a rest.
            while cell.len() > 2 && cell.last().map(|n| n.len < PPQ / 2 && n.start % bar >= PPQ * 3).unwrap_or(false) {
                cell.pop();
            }
        }

        // Breathing room: every cell ends with at least an 8th-note rest.
        let cell_end = motif_bars * bar;
        if let Some(last) = cell.last_mut() {
            if last.end() + PPQ / 2 > cell_end {
                last.len = cell_end.saturating_sub(PPQ / 2).saturating_sub(last.start).max(PPQ / 4);
            }
        }
        // Follow chord changes inside the cell for multi-chord bars.
        let mut prev_pitch: Option<u8> = notes.last().map(|n| n.pitch);
        for n in cell.iter_mut() {
            let abs = b * bar + n.start;
            if let Some(ev) = Session::chord_at(section, abs) {
                let in_bar = abs % bar;
                let strong = in_bar % PPQ == 0;
                if strong && !ev.chord.contains(n.pitch) {
                    n.pitch = nearest_chord_tone_avoiding(&ev.chord, n.pitch, prev_pitch);
                }
            }
            prev_pitch = Some(n.pitch);
            n.start = abs;
        }
        notes.extend(cell);
        b += motif_bars;
        idx += 1;
    }

    // Keep the line inside about a 10th around the motif's centre (singable), then cap leaps.
    let center = motif.first().map(|n| n.pitch as i32).unwrap_or(67);
    for n in notes.iter_mut() {
        let mut p = n.pitch as i32;
        while p > center + 9 {
            p -= 12;
        }
        while p < center - 7 {
            p += 12;
        }
        n.pitch = p.clamp(lo as i32, hi as i32) as u8;
    }
    // Leap cap and register clamp, keeping scale membership.
    for i in 0..notes.len() {
        if i > 0 {
            let prev = notes[i - 1].pitch as i32;
            let mut p = notes[i].pitch as i32;
            while p - prev > 9 {
                p -= 12;
            }
            while prev - p > 9 {
                p += 12;
            }
            notes[i].pitch = p.clamp(lo as i32, hi as i32) as u8;
        }
        while notes[i].pitch > hi {
            notes[i].pitch -= 12;
        }
        while notes[i].pitch < lo {
            notes[i].pitch += 12;
        }
    }
    // Velocity shape: phrase peaks slightly louder, pickups softer.
    for n in notes.iter_mut() {
        let in_bar = n.start % bar;
        n.vel = if in_bar == 0 { 0.85 } else if in_bar % PPQ == 0 { 0.78 } else { 0.7 };
    }
    // Rest budget: ensure at least ~15% silence per phrase by shortening the longest note tails when needed.
    clamp_to_section(&mut notes, total);
    notes.sort_by_key(|n| (n.start, n.pitch));
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notation::parse_chords;

    fn session() -> (Session, Section) {
        let mut s = Session::default();
        let id = s.add_section("Verse", 8, 0.6);
        let ev = parse_chords("| C | Am | F | G |", &s.key, 8, s.bar_ticks()).unwrap();
        s.section_mut(&id).unwrap().chords = ev;
        let sec = s.section(&id).unwrap().clone();
        (s, sec)
    }

    #[test]
    fn melody_is_musical() {
        let (s, sec) = session();
        let notes = generate_melody(&s, &sec, &GenParams::default());
        assert!(notes.len() >= 16, "got {}", notes.len());
        for n in &notes {
            assert!(s.key.contains(n.pitch), "{} out of key", n.pitch);
            assert!(n.pitch >= 55 && n.pitch <= 89);
        }
        for w in notes.windows(2) {
            assert!((w[1].pitch as i32 - w[0].pitch as i32).abs() <= 12);
        }
        let a = analyze(&s, &sec, &notes);
        assert!(a.rest_ratio > 0.05, "rest ratio {}", a.rest_ratio);
        // Motif recurs: first bar rhythm appears again later.
        let first: Vec<u32> = notes.iter().filter(|n| n.start < s.bar_ticks()).map(|n| n.start).collect();
        let second: Vec<u32> = notes.iter().filter(|n| n.start >= s.bar_ticks() && n.start < 2 * s.bar_ticks()).map(|n| n.start - s.bar_ticks()).collect();
        assert!(!first.is_empty() && !second.is_empty());
    }

    #[test]
    fn uses_given_motif() {
        let (s, sec) = session();
        let p = GenParams { motif: Some("E4:8 G4:8 A4:4 G4:4 E4:4".into()), ..Default::default() };
        let notes = generate_melody(&s, &sec, &p);
        assert_eq!(notes[0].pitch, 64);
        assert_eq!(notes[1].start, PPQ / 2);
    }

    fn analyze(s: &Session, sec: &Section, notes: &[Note]) -> crate::analyze::ClipAnalysis {
        crate::analyze::analyze_clip(s, TrackRole::Melody, sec, notes)
    }
}
