//! Additional layer generators: pad, arpeggio, pluck, counter-melody, harmony, sub bass, percussion.
//! All derive from what exists (chords for the harmonic layers, the lead for counter/harmony).

use super::{clamp_to_section, GenParams, SongCtx};
use crate::model::{Note, Section, SectionRole, Session, TrackRole, PPQ};
use crate::theory::Key;
use crate::voicing::{voice_lead, VoicingParams};
use rand::Rng;

fn rate_ticks(rate: Option<&str>, energy: f32, style: &str) -> u32 {
    match rate.map(|r| r.to_ascii_lowercase()).as_deref() {
        Some("4") => PPQ,
        Some("8") => PPQ / 2,
        Some("16") => PPQ / 4,
        Some("32") => PPQ / 8,
        Some("8t") => PPQ / 3,
        Some("16t") => PPQ / 6,
        _ => {
            if style.contains("trance") || style.contains("edm") || energy > 0.6 {
                PPQ / 4
            } else {
                PPQ / 2
            }
        }
    }
}

/// Pad: sustained voicings, 4 voices, wide spacing, slow strum; thinner and lower in intros/breaks.
pub fn generate_pad(session: &Session, section: &Section, params: &GenParams, ctx: SongCtx) -> Vec<Note> {
    let energy = ctx.effective_energy(params.energy(section));
    let mut rng = params.rng(session, 61);
    let total = section.bars * session.bar_ticks();
    let voices = if energy < 0.4 { 3 } else { 4 };
    let vp = VoicingParams { lo: 50, hi: 79, voices, avoid_root_on_top: true, add_low_root: energy > 0.5 };
    let voicings = voice_lead(&section.chords, &vp);
    let mut notes = Vec::new();
    let base_vel = 0.4 + energy * 0.3;
    for (ev, v) in section.chords.iter().zip(&voicings) {
        let strum = rng.random_range(PPQ / 32..PPQ / 8);
        for (i, &p) in v.iter().enumerate() {
            let s = ev.start + strum * i as u32;
            if s < ev.start + ev.len {
                notes.push(Note::new(p, s, ev.len.saturating_sub(strum * i as u32).saturating_sub(PPQ / 16).max(PPQ / 4), (base_vel - i as f32 * 0.03).clamp(0.2, 1.0)));
            }
        }
    }
    // Swell into the next chorus/drop on builds.
    if matches!(ctx.role, SectionRole::Build) {
        for n in notes.iter_mut() {
            let pos = n.start as f32 / total.max(1) as f32;
            n.vel = (n.vel + pos * 0.3).min(1.0);
        }
    }
    clamp_to_section(&mut notes, total);
    notes
}

/// Arpeggio: cycles chord tones at a rate, pattern up/down/updown/random/chord (broken chord with
/// held root), 1-3 octaves; in builds the pattern rises and speeds up over the section.
pub fn generate_arp(session: &Session, section: &Section, params: &GenParams, ctx: SongCtx) -> Vec<Note> {
    let style = params.style(session).to_ascii_lowercase();
    let energy = ctx.effective_energy(params.energy(section));
    let mut rng = params.rng(session, 71);
    let total = section.bars * session.bar_ticks();
    let base_rate = rate_ticks(params.rate.as_deref(), energy, &style);
    let pattern = params.pattern.clone().unwrap_or_else(|| if rng.random_bool(0.6) { "up".into() } else { "updown".into() }).to_ascii_lowercase();
    let octaves = params.octaves.unwrap_or(if energy > 0.6 { 2 } else { 1 }).clamp(1, 3);
    let (lo, hi) = TrackRole::Arpeggio.register();
    let gate = if style.contains("pluck") || energy > 0.7 { 0.5 } else { 0.8 };
    let mut notes = Vec::new();
    for ev in &section.chords {
        let mut tones: Vec<u8> = ev.chord.tones_in_range(lo, lo + 12 * octaves as u8 - 1);
        // Start on the root.
        if let Some(pos) = tones.iter().position(|p| p % 12 == ev.chord.root) {
            tones.rotate_left(pos);
            let mut sorted = tones.clone();
            sorted.sort_unstable();
            let root = tones[0];
            tones = sorted.into_iter().filter(|p| *p >= root).collect();
            if tones.is_empty() {
                continue;
            }
        }
        if tones.is_empty() {
            continue;
        }
        let seq: Vec<u8> = match pattern.as_str() {
            "down" => tones.iter().rev().copied().collect(),
            "updown" | "up_down" | "pingpong" => {
                let mut v = tones.clone();
                if tones.len() > 2 {
                    v.extend(tones[1..tones.len() - 1].iter().rev());
                }
                v
            }
            "random" => (0..tones.len() * 2).map(|_| tones[rng.random_range(0..tones.len())]).collect(),
            "chord" | "broken" => {
                // Root, then upper tones alternating (Alberti-like).
                let mut v = Vec::new();
                for i in 1..tones.len() {
                    v.push(tones[0]);
                    v.push(tones[i]);
                }
                if v.is_empty() {
                    v = tones.clone();
                }
                v
            }
            _ => tones.clone(),
        };
        let mut t = ev.start;
        let mut i = 0usize;
        while t < ev.start + ev.len {
            // Builds: rate doubles halfway through the section.
            let rate = if matches!(ctx.role, SectionRole::Build) && t > total / 2 { (base_rate / 2).max(PPQ / 8) } else { base_rate };
            let p = seq[i % seq.len()].min(hi);
            let accent = (t % PPQ) == 0;
            let mut vel = 0.55 + energy * 0.25 + if accent { 0.12 } else { 0.0 };
            if matches!(ctx.role, SectionRole::Build) {
                vel += (t as f32 / total.max(1) as f32) * 0.2;
            }
            notes.push(Note::new(p, t, ((rate as f32) * gate) as u32, vel.min(1.0)));
            t += rate;
            i += 1;
        }
    }
    clamp_to_section(&mut notes, total);
    notes
}

/// Pluck: short chord stabs on off-beats (house/pop) or a syncopated pattern, 3 voices.
pub fn generate_pluck(session: &Session, section: &Section, params: &GenParams, ctx: SongCtx) -> Vec<Note> {
    let energy = ctx.effective_energy(params.energy(section));
    let mut rng = params.rng(session, 81);
    let total = section.bars * session.bar_ticks();
    let vp = VoicingParams { lo: 57, hi: 84, voices: 3, avoid_root_on_top: false, add_low_root: false };
    let voicings = voice_lead(&section.chords, &vp);
    let q = PPQ;
    let e = q / 2;
    let s16 = q / 4;
    // Offsets within a bar.
    let patterns: [&[u32]; 4] = [
        &[e, q + e, q * 2 + e, q * 3 + e],                  // off-beat 8ths
        &[0, q + e, q * 2 + s16 * 3, q * 3 + e],            // syncopated
        &[e, q * 2, q * 2 + e, q * 3 + s16 * 3],            // pushy
        &[s16 * 3, q + s16 * 3, q * 2 + s16 * 3, q * 3 + e], // 16th anticipations
    ];
    let pat = patterns[if energy > 0.6 { rng.random_range(0..patterns.len()) } else { 0 }];
    let bar = session.bar_ticks();
    let mut notes = Vec::new();
    for (ev, v) in section.chords.iter().zip(&voicings) {
        let first_bar = ev.start / bar;
        let last_bar = (ev.start + ev.len - 1) / bar;
        for b in first_bar..=last_bar {
            for &off in pat {
                let t = b * bar + off;
                if t < ev.start || t >= ev.start + ev.len {
                    continue;
                }
                for (i, &p) in v.iter().enumerate() {
                    notes.push(Note::new(p, t, s16 * 3 / 2, (0.6 + energy * 0.25 - i as f32 * 0.04).clamp(0.2, 1.0)));
                }
            }
        }
    }
    clamp_to_section(&mut notes, total);
    notes
}

/// Counter-melody: a second line in a lower register that fills the lead's rests and moves in
/// contrary motion where they overlap.
pub fn generate_counter(session: &Session, section: &Section, params: &GenParams, ctx: SongCtx) -> Vec<Note> {
    let key: Key = session.key;
    let lead = session.notes_of_kind(TrackRole::Melody, &section.id);
    let mut p = params.clone();
    p.seed = Some(params.seed.unwrap_or(session.seed).wrapping_add(991));
    if p.contour.is_none() {
        p.contour = Some("wave".into());
    }
    let mut line = super::melody::generate_melody(session, section, &p, ctx);
    let (lo, hi) = TrackRole::CounterMelody.register();
    // Lower register.
    for n in line.iter_mut() {
        n.pitch = n.pitch.saturating_sub(7);
        while n.pitch < lo {
            n.pitch += 12;
        }
        while n.pitch > hi {
            n.pitch -= 12;
        }
        n.pitch = key.snap(n.pitch);
        n.vel *= 0.85;
    }
    if lead.is_empty() {
        return line;
    }
    // Where the lead plays, thin the counter (keep notes that start when the lead is silent or on
    // long lead notes), and move it against the lead's direction.
    let lead_busy = |t: u32| lead.iter().any(|l| t >= l.start && t < l.end() && l.len < PPQ);
    let mut out: Vec<Note> = Vec::new();
    for n in line {
        if lead_busy(n.start) && n.len < PPQ / 2 {
            continue;
        }
        out.push(n);
    }
    // Contrary motion: if the lead rises across two counter notes, make the counter fall by step.
    for i in 1..out.len() {
        let l0 = lead.iter().filter(|l| l.start <= out[i - 1].start).last().map(|l| l.pitch as i32);
        let l1 = lead.iter().filter(|l| l.start <= out[i].start).last().map(|l| l.pitch as i32);
        if let (Some(a), Some(b)) = (l0, l1) {
            let lead_dir = (b - a).signum();
            let cdir = (out[i].pitch as i32 - out[i - 1].pitch as i32).signum();
            if lead_dir != 0 && cdir == lead_dir {
                let np = key.step(out[i - 1].pitch, -lead_dir);
                out[i].pitch = np.clamp(lo, hi);
            }
        }
    }
    // Avoid unisons/seconds with the lead on strong beats.
    for n in out.iter_mut() {
        if let Some(l) = lead.iter().find(|l| l.start == n.start) {
            let iv = (l.pitch as i32 - n.pitch as i32).rem_euclid(12);
            if iv == 0 || iv == 1 || iv == 2 || iv == 11 {
                n.pitch = key.step(n.pitch, -2).clamp(lo, hi);
            }
        }
    }
    out
}

/// Harmony: the lead a diatonic 3rd (or 6th for lower energy) below, skipping very short notes.
pub fn generate_harmony(session: &Session, section: &Section, params: &GenParams, ctx: SongCtx) -> Vec<Note> {
    let key: Key = session.key;
    let lead = session.notes_of_kind(TrackRole::Melody, &section.id);
    if lead.is_empty() {
        return Vec::new();
    }
    let energy = ctx.effective_energy(params.energy(section));
    let interval = match params.pattern.as_deref() {
        Some("third") | Some("3rd") => -2,
        Some("sixth") | Some("6th") => -5,
        Some("above") => 2,
        _ => if energy > 0.6 { -2 } else { -5 },
    };
    let (lo, hi) = TrackRole::Harmony.register();
    let mut out = Vec::new();
    for n in &lead {
        if n.len < PPQ / 4 {
            continue;
        }
        let mut p = key.step(n.pitch, interval);
        // Keep it a chord tone on strong beats when possible.
        if let Some(ev) = Session::chord_at(section, n.start) {
            if n.start % PPQ == 0 && !ev.chord.contains(p) {
                let alt = key.step(n.pitch, interval - 1);
                if ev.chord.contains(alt) {
                    p = alt;
                }
            }
        }
        let mut m = n.clone();
        m.pitch = p.clamp(lo, hi);
        m.vel = (n.vel * 0.8).max(0.2);
        m.lyric = None;
        out.push(m);
    }
    out
}

/// Sub bass: root notes only, long, an octave below the bass register.
pub fn generate_sub(session: &Session, section: &Section, params: &GenParams, ctx: SongCtx) -> Vec<Note> {
    let energy = ctx.effective_energy(params.energy(section));
    let total = section.bars * session.bar_ticks();
    let (lo, hi) = TrackRole::Sub.register();
    let mut notes = Vec::new();
    for ev in &section.chords {
        let pc = ev.chord.bass_pc();
        let mut p = lo + ((pc as i32 - lo as i32).rem_euclid(12)) as u8;
        if p > hi {
            p -= 12;
        }
        // Trap/808 style: retrigger halfway on long chords when energetic.
        if energy > 0.6 && ev.len >= PPQ * 4 {
            notes.push(Note::new(p, ev.start, ev.len / 2 - PPQ / 16, 0.9));
            notes.push(Note::new(p, ev.start + ev.len / 2, ev.len / 2 - PPQ / 16, 0.85));
        } else {
            notes.push(Note::new(p, ev.start, ev.len - PPQ / 16, 0.9));
        }
    }
    clamp_to_section(&mut notes, total);
    notes
}

/// Percussion: Euclidean layers (shaker, conga, tambourine, cowbell) chosen by pattern/style/energy.
pub fn generate_percussion(session: &Session, section: &Section, params: &GenParams, ctx: SongCtx) -> Vec<Note> {
    let energy = ctx.effective_energy(params.energy(section));
    let density = params.density.unwrap_or(energy);
    let mut rng = params.rng(session, 91);
    let bar = session.bar_ticks();
    let total = section.bars * bar;
    let s16 = PPQ / 4;
    let steps = (bar / s16) as usize;
    let pattern = params.pattern.clone().unwrap_or_else(|| "mixed".into()).to_ascii_lowercase();
    let mut lanes: Vec<(u8, usize, usize, f32)> = Vec::new(); // (pitch, hits, rotation, vel)
    let shaker = (70u8, (steps as f32 * (0.4 + density * 0.4)) as usize, 0usize, 0.55f32);
    let conga_hi = (63u8, 3 + (density * 3.0) as usize, 2usize, 0.7f32);
    let conga_lo = (64u8, 2 + (density * 2.0) as usize, 5usize, 0.75f32);
    let tamb = (54u8, 4usize, 2usize, 0.6f32);
    let cowbell = (56u8, 2usize, 0usize, 0.7f32);
    match pattern.as_str() {
        "shaker" => lanes.push(shaker),
        "conga" | "congas" => {
            lanes.push(conga_hi);
            lanes.push(conga_lo);
        }
        "tambourine" | "tamb" => lanes.push(tamb),
        "cowbell" => lanes.push(cowbell),
        _ => {
            lanes.push(shaker);
            if density > 0.35 {
                lanes.push(conga_hi);
            }
            if density > 0.55 {
                lanes.push(conga_lo);
            }
            if density > 0.7 && rng.random_bool(0.5) {
                lanes.push(tamb);
            }
        }
    }
    let mut notes = Vec::new();
    for b in 0..section.bars {
        for (pitch, hits, rot, vel) in &lanes {
            let pat = super::drums::euclid((*hits).min(steps), steps, *rot);
            for (i, on) in pat.iter().enumerate() {
                if *on {
                    let accent = i % 4 == 0;
                    notes.push(Note::new(*pitch, b * bar + i as u32 * s16, s16 * 3 / 4, if accent { vel + 0.15 } else { *vel }));
                }
            }
        }
        // Builds: add a rising 16th tambourine roll in the last bar.
        if matches!(ctx.role, SectionRole::Build) && b + 1 == section.bars {
            for i in 0..steps {
                notes.push(Note::new(54, b * bar + i as u32 * s16, s16 / 2, 0.4 + 0.6 * i as f32 / steps as f32));
            }
        }
    }
    clamp_to_section(&mut notes, total);
    notes.sort_by_key(|n| (n.start, n.pitch));
    notes.dedup_by(|a, b| a.start == b.start && a.pitch == b.pitch);
    notes
}
