//! Drum patterns: backbeat grammar per style, hat subdivision by energy, ghost notes, open-hat
//! accents, Euclidean percussion layers and fills at phrase boundaries.

use super::{clamp_to_section, GenParams, SongCtx};
use crate::model::{Note, Section, SectionRole, Session, PPQ};
use rand::Rng;

pub const KICK: u8 = 36;
pub const SNARE: u8 = 38;
pub const RIM: u8 = 37;
pub const CLAP: u8 = 39;
pub const HAT: u8 = 42;
pub const OPEN_HAT: u8 = 46;
pub const TOM_HI: u8 = 50;
pub const TOM_MID: u8 = 47;
pub const TOM_LO: u8 = 43;
pub const RIDE: u8 = 51;
pub const CRASH: u8 = 49;
pub const SHAKER: u8 = 70;

/// Bjorklund's algorithm: `k` onsets spread as evenly as possible over `n` steps.
pub fn euclid(k: usize, n: usize, rotate: usize) -> Vec<bool> {
    if n == 0 {
        return vec![];
    }
    let k = k.min(n);
    let mut pattern = vec![false; n];
    if k == 0 {
        return pattern;
    }
    let mut bucket = 0usize;
    for slot in pattern.iter_mut() {
        bucket += k;
        if bucket >= n {
            bucket -= n;
            *slot = true;
        }
    }
    pattern.rotate_right(rotate % n);
    // Ensure the pattern starts on a hit for rotation 0.
    if rotate == 0 && !pattern[0] {
        if let Some(first) = pattern.iter().position(|&b| b) {
            pattern.rotate_left(first);
        }
    }
    pattern
}

#[derive(Clone, Copy)]
struct Style {
    four_on_floor: bool,
    hats_16ths: bool,
    swing_hats: bool,
    clap_layer: bool,
    trap_hats: bool,
    ride: bool,
    half_time: bool,
    ghosts: bool,
    shaker: bool,
}

fn style_of(s: &str, energy: f32) -> Style {
    let four = s.contains("house") || s.contains("edm") || s.contains("techno") || s.contains("dance") || s.contains("disco");
    let trap = s.contains("trap") || s.contains("drill");
    let lofi = s.contains("lofi") || s.contains("lo-fi") || s.contains("boom") || s.contains("hip");
    let kids = s.contains("kid") || s.contains("nursery") || s.contains("rhyme") || s.contains("folk");
    let cine = s.contains("cinema") || s.contains("orchestra") || s.contains("epic");
    Style {
        four_on_floor: four,
        hats_16ths: (four && energy > 0.5) || (lofi && energy > 0.6) || (!four && !lofi && !kids && energy > 0.7),
        swing_hats: lofi,
        clap_layer: four || trap || (s.contains("pop") && energy > 0.6),
        trap_hats: trap,
        ride: s.contains("jazz") || (cine && energy > 0.6),
        half_time: trap || (lofi && energy < 0.5) || (cine && energy < 0.5),
        ghosts: !four && !kids && !cine,
        shaker: kids || s.contains("folk") || s.contains("acoustic") || (s.contains("pop") && energy < 0.5),
    }
}

pub fn generate_drums(session: &Session, section: &Section, params: &GenParams, ctx: SongCtx) -> Vec<Note> {
    let style_str = params.style(session).to_ascii_lowercase();
    let energy = ctx.effective_energy(params.energy(section));
    let density = params.density.unwrap_or(energy);
    let mut rng = params.rng(session, 41);
    let st = style_of(&style_str, energy);
    let bar = session.bar_ticks();
    let total = section.bars * bar;
    let q = PPQ;
    let e = q / 2;
    let s16 = q / 4;
    let fills = params.fills.unwrap_or(true);
    let mut notes: Vec<Note> = Vec::new();
    let beats = session.time_sig.num.max(1);

    // Per-bar kick variations chosen once so the groove is consistent, with a variation every other bar.
    let kick_a: Vec<u32> = if st.four_on_floor {
        (0..beats).map(|b| b * q).collect()
    } else if st.half_time {
        vec![0, q * 2 + e + if rng.random_bool(0.5) { 0 } else { s16 }]
    } else {
        let mut v = vec![0, q * 2 + e];
        if energy > 0.5 || rng.random_bool(0.5) {
            v.push(q + e + s16);
        }
        v
    };
    let kick_b: Vec<u32> = {
        let mut v = kick_a.clone();
        if !st.four_on_floor {
            let extra = [q * 3 + e, q + s16 * 3, q * 3 + s16 * 3, q * 2 + s16 * 2];
            v.push(extra[rng.random_range(0..extra.len())]);
            v.sort_unstable();
            v.dedup();
        }
        v
    };
    let snare_beats: Vec<u32> = if st.half_time { vec![q * 2] } else if beats >= 4 { vec![q, q * 3] } else { vec![q] };

    for b in 0..section.bars {
        let bar_start = b * bar;
        let phrase_end = fills && (b + 1) % 4 == 0 && section.bars >= 4;
        let kicks = if b % 2 == 1 { &kick_b } else { &kick_a };

        // Crash on the first downbeat of the section (when energetic) and after fills.
        if b == 0 && energy > 0.5 {
            notes.push(Note::new(CRASH, bar_start, q, 0.9));
        }

        for &k in kicks {
            if k < bar {
                notes.push(Note::new(KICK, bar_start + k, e, if k == 0 { 1.0 } else { 0.9 }));
            }
        }
        for &sb in &snare_beats {
            if sb < bar {
                let vel = if st.half_time { 1.0 } else { 0.95 };
                notes.push(Note::new(SNARE, bar_start + sb, e, vel));
                if st.clap_layer {
                    notes.push(Note::new(CLAP, bar_start + sb, e, 0.8));
                }
            }
        }
        // Ghost notes around the backbeat.
        if st.ghosts && density > 0.3 {
            let candidates = [q + s16 * 3, q * 2 + s16, q * 3 + s16 * 3, e + s16, q * 2 + s16 * 3];
            let count = 1 + (density * 2.5) as usize;
            for i in 0..count.min(candidates.len()) {
                let t = candidates[(i + b as usize) % candidates.len()];
                if t < bar && rng.random_bool(0.7) {
                    notes.push(Note::new(SNARE, bar_start + t, s16, 0.2 + rng.random_range(0.0..0.2)));
                }
            }
        }

        // Hats / ride.
        let hat_pitch = if st.ride { RIDE } else { HAT };
        if st.trap_hats {
            // 8ths with 16th/32nd rolls sprinkled in.
            let mut t = 0;
            while t < bar {
                notes.push(Note::new(HAT, bar_start + t, s16, if t % q == 0 { 0.85 } else { 0.65 }));
                if density > 0.5 && rng.random_bool(0.25) {
                    let sub = if rng.random_bool(0.6) { s16 } else { s16 / 2 };
                    let mut r = t + sub;
                    while r < t + e {
                        notes.push(Note::new(HAT, bar_start + r, sub, 0.5));
                        r += sub;
                    }
                }
                t += e;
            }
        } else if st.hats_16ths {
            let mut t = 0;
            let mut i = 0;
            while t < bar {
                let vel = match i % 4 {
                    0 => 0.85,
                    2 => 0.7,
                    _ => 0.5,
                };
                notes.push(Note::new(hat_pitch, bar_start + t, s16, vel));
                t += s16;
                i += 1;
            }
        } else if density > 0.15 {
            let mut t = 0;
            let mut i = 0;
            while t < bar {
                notes.push(Note::new(hat_pitch, bar_start + t, e, if i % 2 == 0 { 0.8 } else { 0.6 }));
                t += e;
                i += 1;
            }
        }
        // Open hat on the "and" of the last beat (four-on-floor) or occasionally elsewhere.
        if st.four_on_floor {
            for bt in 0..beats {
                let t = bt * q + e;
                notes.retain(|n| !(n.pitch == HAT && n.start == bar_start + t));
                notes.push(Note::new(OPEN_HAT, bar_start + t, e - s16 / 2, 0.75));
            }
        } else if energy > 0.4 && rng.random_bool(0.5) {
            let t = (beats - 1) * q + e;
            notes.retain(|n| !(n.pitch == HAT && n.start == bar_start + t));
            notes.push(Note::new(OPEN_HAT, bar_start + t, e, 0.7));
        }
        // Shaker / percussion via Euclidean rhythm.
        if st.shaker || (density > 0.6 && !st.trap_hats) {
            let steps = (bar / s16) as usize;
            let k = if st.shaker { steps / 2 } else { 5 + (density * 4.0) as usize };
            let pat = euclid(k.min(steps), steps, if st.shaker { 0 } else { 2 });
            for (i, on) in pat.iter().enumerate() {
                if *on {
                    notes.push(Note::new(SHAKER, bar_start + i as u32 * s16, s16, if i % 4 == 0 { 0.7 } else { 0.5 }));
                }
            }
        }
        // Fill at the end of each 4-bar phrase: replace the last beat(s) with a tom/snare run.
        if phrase_end {
            let fill_start = bar_start + (beats - 1) * q - if energy > 0.6 { q } else { 0 };
            notes.retain(|n| !(n.start >= fill_start && n.start < bar_start + bar && (n.pitch == SNARE || n.pitch == HAT || n.pitch == SHAKER)));
            let drums = [SNARE, SNARE, TOM_HI, TOM_MID, TOM_LO, SNARE];
            let mut t = fill_start;
            let mut i = 0;
            let step = if energy > 0.5 { s16 } else { e };
            while t < bar_start + bar {
                let p = if energy > 0.5 { drums[i % drums.len()] } else { SNARE };
                notes.push(Note::new(p, t, step, 0.6 + (i as f32 * 0.06).min(0.35)));
                t += step;
                i += 1;
            }
            if b + 1 < section.bars {
                notes.push(Note::new(CRASH, bar_start + bar, q, 0.95));
            }
        }
    }
    // Build: snare roll rising into the next section over the last bar (or two).
    if matches!(ctx.role, SectionRole::Build) && section.bars >= 2 {
        let roll_bars = if section.bars >= 4 { 2 } else { 1 };
        let roll_start = (section.bars - roll_bars) * bar;
        notes.retain(|n| n.start < roll_start || n.pitch == KICK);
        let mut t = roll_start;
        let mut i = 0u32;
        while t < total {
            let frac = (t - roll_start) as f32 / (total - roll_start) as f32;
            let step = if frac < 0.5 { s16 } else { s16 / 2 };
            notes.push(Note::new(SNARE, t, step, 0.35 + 0.65 * frac));
            if i % 4 == 0 {
                notes.push(Note::new(KICK, t, e, 0.8 + 0.2 * frac));
            }
            t += step;
            i += 1;
        }
        notes.push(Note::new(CRASH, total.saturating_sub(s16), s16, 0.9));
    }
    if matches!(ctx.role, SectionRole::Intro) {
        notes.retain(|n| n.pitch != CRASH);
    }
    // Rim instead of snare for very quiet sections.
    if energy < 0.25 {
        for n in notes.iter_mut() {
            if n.pitch == SNARE && n.vel > 0.5 {
                n.pitch = RIM;
            }
        }
    }
    let _ = st.swing_hats; // swing is applied by the humanizer preset
    clamp_to_section(&mut notes, total);
    notes.sort_by_key(|n| (n.start, n.pitch));
    notes.dedup_by(|a, b| a.start == b.start && a.pitch == b.pitch);
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn euclid_patterns() {
        let p = euclid(3, 8, 0);
        assert_eq!(p.iter().filter(|&&b| b).count(), 3);
        assert!(p[0]);
        let p = euclid(5, 8, 0);
        assert_eq!(p.iter().filter(|&&b| b).count(), 5);
    }

    #[test]
    fn drums_have_backbeat_and_fill() {
        let mut s = Session::default();
        s.style = "pop".into();
        let id = s.add_section("A", 8, 0.7);
        let sec = s.section(&id).unwrap().clone();
        let notes = generate_drums(&s, &sec, &GenParams::default(), super::SongCtx::of(&s, &sec.id));
        let bar = s.bar_ticks();
        assert!(notes.iter().any(|n| n.pitch == SNARE && n.start == PPQ));
        assert!(notes.iter().any(|n| n.pitch == KICK && n.start == 0));
        // Fill in bar 4: toms present.
        assert!(notes.iter().any(|n| (n.pitch == TOM_HI || n.pitch == TOM_LO) && n.start >= bar * 3 && n.start < bar * 4));
        assert!(notes.iter().any(|n| n.pitch == SNARE && n.vel < 0.45), "ghost notes expected");
        assert!(notes.iter().all(|n| n.end() <= bar * 8));
    }
}
