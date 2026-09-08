//! Bass lines: pattern library selected by style and energy, following the chord roots (slash bass
//! respected), with approach notes into chord changes and space where the melody is busy.

use super::{bar_roll, clamp_to_section, GenParams, SongCtx};
use crate::model::{Note, Section, SectionRole, Session, TrackRole, PPQ};
use rand::Rng;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Pattern {
    Root,
    RootFifth,
    Octave,
    Walking,
    Pedal,
    Push,
    EightOhEight,
    Pulse8,
}

fn pick_pattern(style: &str, energy: f32, explicit: Option<&str>, rng: &mut impl Rng) -> Pattern {
    if let Some(p) = explicit {
        return match p.to_ascii_lowercase().as_str() {
            "root" | "whole" => Pattern::Root,
            "root5" | "rootfifth" | "root-fifth" => Pattern::RootFifth,
            "octave" | "octaves" => Pattern::Octave,
            "walking" | "walk" => Pattern::Walking,
            "pedal" => Pattern::Pedal,
            "push" | "anticipation" | "syncopated" => Pattern::Push,
            "808" | "trap" => Pattern::EightOhEight,
            "pulse" | "8ths" | "eighths" => Pattern::Pulse8,
            _ => Pattern::Root,
        };
    }
    let s = style;
    if s.contains("trap") || s.contains("808") {
        Pattern::EightOhEight
    } else if s.contains("jazz") || s.contains("swing") {
        Pattern::Walking
    } else if s.contains("house") || s.contains("edm") || s.contains("techno") || s.contains("dance") {
        if energy > 0.5 { Pattern::Octave } else { Pattern::Pulse8 }
    } else if s.contains("cinema") || s.contains("ambient") || s.contains("orchestra") {
        if energy < 0.5 { Pattern::Root } else { Pattern::Pedal }
    } else if s.contains("lofi") || s.contains("lo-fi") || s.contains("hip") || s.contains("boom") {
        if rng.random_bool(0.5) { Pattern::Push } else { Pattern::RootFifth }
    } else if s.contains("kid") || s.contains("nursery") || s.contains("rhyme") || s.contains("folk") {
        if energy > 0.5 { Pattern::RootFifth } else { Pattern::Root }
    } else if energy > 0.7 {
        Pattern::Pulse8
    } else if energy > 0.4 {
        Pattern::Push
    } else {
        Pattern::Root
    }
}

fn root_in_register(pc: u8) -> u8 {
    let (lo, hi) = TrackRole::Bass.register();
    let mut p = lo + ((pc as i32 - lo as i32).rem_euclid(12)) as u8;
    if p > hi {
        p -= 12;
    }
    p
}

pub fn generate_bass(session: &Session, section: &Section, params: &GenParams, ctx: SongCtx) -> Vec<Note> {
    let style = params.style(session).to_ascii_lowercase();
    let energy = ctx.effective_energy(params.energy(section));
    let mut rng = params.rng_in(session, 31, section, ctx);
    let mut pattern = pick_pattern(&style, energy, params.pattern.as_deref(), &mut rng);
    if params.pattern.is_none() && matches!(ctx.role, SectionRole::Intro | SectionRole::Break | SectionRole::Outro) {
        pattern = Pattern::Root;
    } else if params.pattern.is_none() && matches!(ctx.role, SectionRole::Chorus | SectionRole::Drop) && energy > 0.7 && !style.contains("trap") {
        pattern = if style.contains("house") || style.contains("edm") { Pattern::Octave } else { Pattern::Pulse8 };
    }
    let bar = session.bar_ticks();
    let total = section.bars * bar;
    let key = session.key;
    let q = PPQ;
    let e = q / 2;
    let mut notes = Vec::new();
    let base_vel = 0.7 + energy * 0.2;

    // Melody density per beat, to leave space when the melody is busy (call and response).
    let melody = session.notes_of_kind(TrackRole::Melody, &section.id);
    let beat_busy = |t: u32| melody.iter().filter(|n| n.start >= t && n.start < t + q).count() >= 2;
    let stream = params.stream(session, 31, section, ctx);

    for (ci, ev) in section.chords.iter().enumerate() {
        let root = root_in_register(ev.chord.bass_pc());
        let fifth = root + 7;
        let next_root = section.chords.get(ci + 1).map(|n| root_in_register(n.chord.bass_pc()));
        match pattern {
            Pattern::Root => {
                notes.push(Note::new(root, ev.start, ev.len - ev.len / 16, base_vel));
                // A pickup into the next chord on some bars, so a held root does not sit dead.
                let b = ev.start / bar;
                if ev.len >= q * 2 && energy > 0.35 && bar_roll(stream, b, 0x524F) < 0.35 {
                    if let Some(nr) = next_root {
                        let approach = if nr > root { nr - 2 } else { nr + 2 };
                        notes.push(Note::new(key.snap(approach), ev.start + ev.len - e, e - e / 4, base_vel - 0.12));
                    }
                }
            }
            Pattern::Pedal => {
                let pedal = root_in_register(section.chords[0].chord.bass_pc());
                let mut t = ev.start;
                while t < ev.start + ev.len {
                    let len = (q * 2).min(ev.start + ev.len - t);
                    notes.push(Note::new(pedal, t, len - len / 16, base_vel));
                    t += q * 2;
                }
            }
            Pattern::RootFifth => {
                let mut t = ev.start;
                let mut i = 0;
                while t < ev.start + ev.len {
                    let p = if i % 2 == 0 { root } else { fifth.min(root + 7) };
                    let len = (q).min(ev.start + ev.len - t);
                    if i % 2 == 0 || !beat_busy(t) || energy > 0.6 {
                        notes.push(Note::new(p, t, len - len / 8, if i % 2 == 0 { base_vel } else { base_vel - 0.1 }));
                    }
                    t += q;
                    i += 1;
                }
            }
            Pattern::Octave => {
                let mut t = ev.start;
                let mut i = 0;
                while t < ev.start + ev.len {
                    let b = t / bar;
                    let p = if i % 2 == 0 { root } else { root + 12 };
                    // Every other bar, answer the octave with the fifth instead.
                    let p = if i % 2 == 1 && bar_roll(stream, b, 0x5045) < 0.4 { fifth.min(root + 7) } else { p };
                    notes.push(Note::new(p, t, e - e / 4, if i % 2 == 0 { base_vel } else { base_vel - 0.12 }));
                    t += e;
                    i += 1;
                }
            }
            Pattern::Pulse8 => {
                let mut t = ev.start;
                let mut i = 0;
                while t < ev.start + ev.len {
                    let b = t / bar;
                    let accent = i % 2 == 0;
                    // Leave a hole for the melody, and jump the octave once or twice a bar.
                    let hole = !accent && beat_busy(t) && energy < 0.7 && bar_roll(stream, b * 16 + i, 0x5042) < 0.35;
                    if !hole {
                        let jump = !accent && bar_roll(stream, b * 16 + i, 0x5043) < 0.18;
                        let p = if jump { root + 12 } else { root };
                        let v = if accent { base_vel } else { base_vel - 0.15 };
                        notes.push(Note::new(p, t, e - e / 3, v + (bar_roll(stream, b * 16 + i, 0x5044) - 0.5) * 0.08));
                    }
                    t += e;
                    i += 1;
                }
            }
            Pattern::Push => {
                // Root on 1, rest, "and of 2" root, then push into the next chord an 8th early.
                notes.push(Note::new(root, ev.start, q + e, base_vel));
                if ev.len >= q * 3 {
                    notes.push(Note::new(root, ev.start + q * 2 + e, e, base_vel - 0.1));
                }
                if ev.len >= q * 4 {
                    let approach = next_root.map(|nr| if nr > root { nr - 1 } else { nr + 1 }).unwrap_or(fifth);
                    let approach = if key.contains(approach) || rng.random_bool(0.5) { approach } else { key.snap(approach) };
                    notes.push(Note::new(approach, ev.start + ev.len - e, e - e / 4, base_vel - 0.05));
                }
            }
            Pattern::Walking => {
                let beats = ev.len / q;
                let tones = ev.chord.tones_in_range(root, root + 12);
                for b in 0..beats {
                    let p = if b == 0 {
                        root
                    } else if b + 1 == beats {
                        // Approach the next root chromatically or diatonically.
                        match next_root {
                            Some(nr) if nr > root => nr - if rng.random_bool(0.5) { 1 } else { 2 },
                            Some(nr) => nr + if rng.random_bool(0.5) { 1 } else { 2 },
                            None => fifth,
                        }
                    } else {
                        let i = (b as usize) % tones.len().max(1);
                        tones.get(i).copied().unwrap_or(root)
                    };
                    notes.push(Note::new(p, ev.start + b * q, q - q / 8, base_vel - if b == 0 { 0.0 } else { 0.08 }));
                }
            }
            Pattern::EightOhEight => {
                // Long 808 on the downbeat, a syncopated hit later in the bar, occasional octave drop.
                notes.push(Note::new(root, ev.start, q * 2 + e, base_vel + 0.05));
                if ev.len >= q * 4 {
                    let t = ev.start + q * 2 + e + if rng.random_bool(0.5) { 0 } else { q / 4 };
                    notes.push(Note::new(root, t, q - q / 4, base_vel - 0.05));
                    if energy > 0.6 && rng.random_bool(0.5) {
                        notes.push(Note::new(root.saturating_sub(12).max(24), ev.start + q * 3 + e, e, base_vel));
                    }
                }
            }
        }
    }
    // Register clamp.
    let (lo, hi) = TrackRole::Bass.register();
    for n in notes.iter_mut() {
        while n.pitch > hi + 2 {
            n.pitch -= 12;
        }
        while n.pitch < lo.saturating_sub(2) {
            n.pitch += 12;
        }
    }
    clamp_to_section(&mut notes, total);
    notes.sort_by_key(|n| (n.start, n.pitch));
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notation::parse_chords;

    #[test]
    fn bass_follows_roots() {
        let mut s = Session::default();
        let id = s.add_section("A", 4, 0.5);
        s.section_mut(&id).unwrap().chords = parse_chords("| C | Am | F/A | G |", &s.key, 4, s.bar_ticks()).unwrap();
        let sec = s.section(&id).unwrap().clone();
        for pat in ["root", "root5", "octave", "walking", "pedal", "push", "808", "pulse"] {
            let p = GenParams { pattern: Some(pat.into()), ..Default::default() };
            let notes = generate_bass(&s, &sec, &p, super::SongCtx::of(&s, &sec.id));
            assert!(!notes.is_empty(), "{pat}");
            if pat != "pedal" {
                let first_of_bar3 = notes.iter().find(|n| n.start == s.bar_ticks() * 2).unwrap();
                assert_eq!(first_of_bar3.pitch % 12, 9, "{pat}: slash bass should be A");
            }
            assert!(notes.iter().all(|n| n.pitch >= 26 && n.pitch <= 54), "{pat}");
        }
    }
}
