//! Chord progression suggestion (genre-weighted functional harmony) and chord-track rendering
//! (voice-led block chords, or rhythmic comping patterns by style/energy).

use super::{clamp_to_section, GenParams};
use crate::model::{ChordEvent, Note, Section, Session, PPQ};
use crate::theory::{Chord, ChordQuality, Extension, Key};
use crate::voicing::{voice_lead, VoicingParams};
use rand::Rng;

/// Roman-numeral progression templates by style. Each entry is one bar (a `.` splits bars).
fn templates(style: &str, minor: bool) -> Vec<&'static str> {
    let s = style.to_ascii_lowercase();
    let kids = s.contains("kid") || s.contains("nursery") || s.contains("rhyme") || s.contains("folk");
    let lofi = s.contains("lofi") || s.contains("lo-fi") || s.contains("jazz") || s.contains("neo");
    let hip = s.contains("hip") || s.contains("trap");
    let cine = s.contains("cinema") || s.contains("orchestra") || s.contains("epic") || s.contains("ambient");
    let edm = s.contains("edm") || s.contains("house") || s.contains("dance") || s.contains("pop");
    if minor {
        if cine {
            vec!["| i | VI | III | VII |", "| i | iv | VI | V |", "| i | VII | VI | VII |", "| i | i | VI | VII |", "| i | III | VII | iv |", "| i | VI | iv | V |"]
        } else if lofi {
            vec!["| i7 | iv7 | VII7 | III maj7 |", "| i7 | VI maj7 | ii° 7 | V7 |", "| i9 | iv9 | i9 | V7 |", "| i7 | VII7 | VI maj7 | V7 |"]
        } else if hip {
            vec!["| i | i | VI | VII |", "| i | VII | VI | VI |", "| i | iv | i | V |", "| i | VI | III | VII |"]
        } else {
            vec!["| i | VI | III | VII |", "| i | VII | VI | VII |", "| i | iv | VII | III |", "| i | VI | VII | i |", "| i | III | VII | VI |", "| i | v | VI | iv |"]
        }
    } else if kids {
        vec!["| I | IV | V | I |", "| I | V | I | V |", "| I | I | IV | V |", "| I | vi | IV | V |", "| I | IV | I | V |", "| I | V | vi | IV |"]
    } else if lofi {
        vec!["| Imaj7 | vi7 | ii7 | V7 |", "| ii7 | V7 | Imaj7 | vi7 |", "| Imaj7 | IVmaj7 | iii7 | vi7 |", "| IVmaj7 | iii7 | vi7 | ii7 |", "| Imaj7 | iii7 | IVmaj7 | V7 |"]
    } else if cine {
        vec!["| I | V | vi | IV |", "| I | iii | IV | I |", "| vi | IV | I | V |", "| I | IV | vi | V |", "| I | bVII | IV | I |", "| I | V | IV | IV |"]
    } else if hip {
        vec!["| I | vi | IV | V |", "| vi | IV | I | V |", "| I | IV | vi | V |", "| ii | V | I | I |"]
    } else if edm {
        vec!["| I | V | vi | IV |", "| vi | IV | I | V |", "| I | vi | IV | V |", "| IV | V | vi | I |", "| I | IV | vi | V |", "| vi | V | IV | V |", "| I | iii | vi | IV |"]
    } else {
        vec!["| I | V | vi | IV |", "| I | vi | IV | V |", "| vi | IV | I | V |", "| I | IV | V | IV |", "| I | iii | IV | V |", "| ii | V | I | vi |"]
    }
}

/// Suggests `count` progressions for the session key/style as chord-notation strings (with roman numerals).
pub fn suggest_progressions(session: &Session, style: Option<&str>, count: usize, seed: u64) -> Vec<(String, String)> {
    let style = style.unwrap_or(&session.style);
    let key: Key = session.key;
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(seed);
    use rand::SeedableRng;
    let mut pool = templates(style, key.scale.is_minor());
    // Shuffle deterministically.
    for i in (1..pool.len()).rev() {
        let j = rng.random_range(0..=i);
        pool.swap(i, j);
    }
    pool.iter()
        .take(count)
        .map(|t| {
            let romans = t.replace(" maj7", "maj7").replace("° 7", "°7");
            let symbols: Vec<String> = romans
                .split('|')
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(|c| Chord::parse_roman(c, &key).map(|ch| ch.symbol()).unwrap_or_else(|| c.to_string()))
                .collect();
            (format!("| {} |", symbols.join(" | ")), romans)
        })
        .collect()
}

/// Adds tasteful extensions to plain triads depending on style (7ths for lo-fi/jazz, add9 for cinematic, none for kids).
pub fn colorize(chords: &mut [ChordEvent], style: &str, rng: &mut impl Rng) {
    let s = style.to_ascii_lowercase();
    let lofi = s.contains("lofi") || s.contains("lo-fi") || s.contains("jazz") || s.contains("neo") || s.contains("rnb") || s.contains("r&b");
    let cine = s.contains("cinema") || s.contains("ambient") || s.contains("orchestra");
    for ev in chords.iter_mut() {
        if !ev.chord.extensions.is_empty() {
            continue;
        }
        if lofi {
            match ev.chord.quality {
                ChordQuality::Major => ev.chord.extensions.push(if rng.random_bool(0.7) { Extension::Maj7 } else { Extension::Add9 }),
                ChordQuality::Minor => ev.chord.extensions.push(if rng.random_bool(0.6) { Extension::Min7 } else { Extension::Nine }),
                ChordQuality::Diminished => ev.chord.extensions.push(Extension::Min7),
                _ => {}
            }
        } else if cine && rng.random_bool(0.4) && matches!(ev.chord.quality, ChordQuality::Major | ChordQuality::Minor) {
            ev.chord.extensions.push(if rng.random_bool(0.5) { Extension::Add9 } else { Extension::Six });
            if rng.random_bool(0.3) && ev.chord.quality == ChordQuality::Major {
                ev.chord.extensions.clear();
                ev.chord.quality = ChordQuality::Sus2;
            }
        }
        ev.symbol = ev.chord.symbol();
    }
}

/// Renders the section's chord events into notes with a comping pattern chosen by style and energy.
pub fn render_chords(session: &Session, section: &Section, params: &GenParams) -> Vec<Note> {
    let style = params.style(session).to_ascii_lowercase();
    let energy = params.energy(section);
    let mut rng = params.rng(session, 11);
    let bar = session.bar_ticks();
    let total = section.bars * bar;
    let kids = style.contains("kid") || style.contains("nursery") || style.contains("rhyme");
    let cine = style.contains("cinema") || style.contains("ambient") || style.contains("orchestra");
    let edm = style.contains("edm") || style.contains("house") || style.contains("dance") || style.contains("techno");
    let hip = style.contains("hip") || style.contains("trap") || style.contains("lofi") || style.contains("lo-fi");

    let vp = VoicingParams { lo: 52, hi: 76, voices: if cine { 4 } else { 3 + (energy > 0.4) as usize }, avoid_root_on_top: true, add_low_root: cine || kids };
    let voicings = voice_lead(&section.chords, &vp);
    let base_vel = 0.55 + energy * 0.3;
    let mut notes = Vec::new();

    for (ev, v) in section.chords.iter().zip(&voicings) {
        if v.is_empty() {
            continue;
        }
        // Choose a pattern: pads hold; kids/pop play on beats; edm stabs off-beats; hip-hop lets chords ring with a pickup.
        let pattern: Vec<(u32, u32, f32)> = if cine || energy < 0.25 {
            vec![(0, ev.len, 1.0)]
        } else if edm && energy > 0.5 {
            // Off-beat 8th stabs.
            let mut p = Vec::new();
            let mut t = PPQ / 2;
            while t < ev.len {
                p.push((t, PPQ / 2 - PPQ / 8, 0.9));
                t += PPQ;
            }
            p
        } else if kids || (style.contains("pop") && energy > 0.5) {
            // Beat 1 long, beats 2-4 short pulses; every other bar add an "and" pickup.
            let beats = ev.len / PPQ;
            let mut p = Vec::new();
            for b in 0..beats.max(1) {
                let start = b * PPQ;
                p.push((start, if b == 0 { PPQ - PPQ / 8 } else { PPQ / 2 }, if b == 0 { 1.0 } else { 0.8 }));
            }
            if beats >= 4 && (ev.start / bar) % 2 == 1 && energy > 0.4 {
                p.push((3 * PPQ + PPQ / 2, PPQ / 2, 0.75));
            }
            p
        } else if hip {
            // Hold, with a soft re-strike on the "and of 2" for longer chords.
            let mut p = vec![(0, ev.len.min(PPQ * 2 + PPQ / 2), 1.0)];
            if ev.len > PPQ * 2 && rng.random_bool(0.6) {
                p.push((PPQ * 2 + PPQ / 2, ev.len - (PPQ * 2 + PPQ / 2), 0.8));
            }
            p
        } else {
            // Generic: half-note pulses.
            let mut p = Vec::new();
            let mut t = 0;
            while t < ev.len {
                p.push((t, (PPQ * 2).min(ev.len - t) - PPQ / 16, if t == 0 { 1.0 } else { 0.85 }));
                t += PPQ * 2;
            }
            p
        };

        for (off, len, vmul) in pattern {
            let strum: u32 = if cine || kids { rng.random_range(0..PPQ / 24) } else { 0 };
            for (vi, &pitch) in v.iter().enumerate() {
                let s = ev.start + off + strum * vi as u32;
                if s >= total {
                    continue;
                }
                let vel = (base_vel * vmul - vi as f32 * 0.03).clamp(0.2, 1.0);
                notes.push(Note::new(pitch, s, len.saturating_sub(strum * vi as u32).max(PPQ / 8), vel));
            }
        }
    }
    clamp_to_section(&mut notes, total);
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notation::parse_chords;

    #[test]
    fn suggestions_are_in_key() {
        let mut s = Session::default();
        s.key = Key::parse("G major").unwrap();
        let sug = suggest_progressions(&s, Some("pop"), 3, 7);
        assert_eq!(sug.len(), 3);
        for (sym, _) in &sug {
            let ev = parse_chords(sym, &s.key, 4, s.bar_ticks()).unwrap();
            assert_eq!(ev.len(), 4);
            for e in ev {
                assert!(s.key.contains(e.chord.root), "{sym}");
            }
        }
    }

    #[test]
    fn renders_chords() {
        let mut s = Session::default();
        let id = s.add_section("Verse", 4, 0.6);
        let ev = parse_chords("| C | Am | F | G |", &s.key, 4, s.bar_ticks()).unwrap();
        s.section_mut(&id).unwrap().chords = ev;
        let sec = s.section(&id).unwrap().clone();
        let notes = render_chords(&s, &sec, &GenParams::default());
        assert!(notes.len() >= 12);
        assert!(notes.iter().all(|n| n.end() <= s.bar_ticks() * 4));
    }
}
