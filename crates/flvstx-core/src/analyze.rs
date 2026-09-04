//! Readable analysis of clips so the agent can self-correct ("largest leap is a 9th", "4 notes out of key").

use crate::model::{Note, Section, Session, TrackRole, PPQ};
use crate::theory::pitch_name;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipAnalysis {
    pub track: String,
    pub section: String,
    pub note_count: usize,
    pub bars: u32,
    pub range: Option<(String, String)>,
    pub largest_leap: u8,
    pub leaps_over_6th: usize,
    pub out_of_key: Vec<String>,
    pub non_chord_on_strong_beats: usize,
    pub density_per_bar: Vec<usize>,
    pub rest_ratio: f32,
    pub velocity_range: (u8, u8),
    pub distinct_start_offsets: usize,
    pub warnings: Vec<String>,
}

pub fn analyze_clip(session: &Session, role: TrackRole, section: &Section, notes: &[Note]) -> ClipAnalysis {
    let bar = session.bar_ticks();
    let total = section.bars * bar;
    let mut sorted: Vec<&Note> = notes.iter().collect();
    sorted.sort_by_key(|n| (n.start, n.pitch));

    let range = if sorted.is_empty() {
        None
    } else {
        let lo = sorted.iter().map(|n| n.pitch).min().unwrap();
        let hi = sorted.iter().map(|n| n.pitch).max().unwrap();
        Some((pitch_name(lo), pitch_name(hi)))
    };

    // Leaps: consecutive melodic notes (monophonic reading: highest note per onset).
    let mut mono: Vec<&Note> = Vec::new();
    for n in &sorted {
        match mono.last() {
            Some(l) if l.start == n.start => {
                if n.pitch > l.pitch {
                    *mono.last_mut().unwrap() = n;
                }
            }
            _ => mono.push(n),
        }
    }
    let mut largest = 0u8;
    let mut over6 = 0usize;
    for w in mono.windows(2) {
        let d = (w[1].pitch as i32 - w[0].pitch as i32).unsigned_abs() as u8;
        largest = largest.max(d);
        if d > 9 {
            over6 += 1;
        }
    }

    let out_of_key: Vec<String> = if role == TrackRole::Drums {
        vec![]
    } else {
        let mut v: Vec<String> = sorted.iter().filter(|n| !session.key.contains(n.pitch)).map(|n| format!("{}@bar{}", pitch_name(n.pitch), n.start / bar + 1)).collect();
        v.dedup();
        v.truncate(12);
        v
    };

    let mut nct_strong = 0usize;
    if role == TrackRole::Melody || role == TrackRole::Bass {
        for n in &sorted {
            let in_bar = n.start % bar;
            let strong = in_bar % PPQ == 0 && (in_bar / PPQ) % 2 == 0;
            if strong {
                if let Some(ch) = Session::chord_at(section, n.start) {
                    if !ch.chord.contains(n.pitch) {
                        nct_strong += 1;
                    }
                }
            }
        }
    }

    let mut density = vec![0usize; section.bars as usize];
    for n in &sorted {
        let b = (n.start / bar) as usize;
        if b < density.len() {
            density[b] += 1;
        }
    }

    // Rest ratio: fraction of the section with nothing sounding (monophonic union).
    let mut covered = 0u64;
    let mut cur_end = 0u32;
    for n in &sorted {
        let s = n.start.min(total);
        let e = n.end().min(total);
        if e > cur_end {
            covered += (e - s.max(cur_end)) as u64;
            cur_end = e;
        }
    }
    let rest_ratio = if total > 0 { 1.0 - covered as f32 / total as f32 } else { 0.0 };

    let vmin = sorted.iter().map(|n| n.vel_midi()).min().unwrap_or(0);
    let vmax = sorted.iter().map(|n| n.vel_midi()).max().unwrap_or(0);
    let offsets: std::collections::BTreeSet<u32> = sorted.iter().map(|n| n.start % (PPQ / 4)).collect();

    let mut warnings = Vec::new();
    if sorted.is_empty() {
        warnings.push("clip is empty".into());
    }
    if over6 > 0 && role == TrackRole::Melody {
        warnings.push(format!("{over6} leap(s) larger than a 6th; singers/players find these hard"));
    }
    if !out_of_key.is_empty() {
        warnings.push(format!("{} note(s) outside {}", out_of_key.len(), session.key.name()));
    }
    if role == TrackRole::Melody && rest_ratio < 0.1 && section.bars >= 4 {
        warnings.push("melody has almost no rests; phrases need breathing room".into());
    }
    if role != TrackRole::Drums && vmax.saturating_sub(vmin) < 8 && sorted.len() > 8 {
        warnings.push("velocities are nearly uniform; humanize the part".into());
    }
    if role == TrackRole::Melody && nct_strong > sorted.len() / 3 && sorted.len() > 4 {
        warnings.push(format!("{nct_strong} strong-beat notes are not chord tones; melody may clash with the harmony"));
    }
    if let Some(n) = sorted.iter().find(|n| n.end() > total) {
        warnings.push(format!("a note at bar {} extends past the end of the section", n.start / bar + 1));
    }
    let (lo, hi) = role.register();
    let out_reg = sorted.iter().filter(|n| n.pitch < lo.saturating_sub(5) || n.pitch > hi + 5).count();
    if out_reg > 0 && role != TrackRole::Drums {
        warnings.push(format!("{out_reg} note(s) far outside the usual {} register ({}..{})", role.name(), pitch_name(lo), pitch_name(hi)));
    }

    ClipAnalysis {
        track: role.name().into(),
        section: section.id.clone(),
        note_count: sorted.len(),
        bars: section.bars,
        range,
        largest_leap: largest,
        leaps_over_6th: over6,
        out_of_key,
        non_chord_on_strong_beats: nct_strong,
        density_per_bar: density,
        rest_ratio,
        velocity_range: (vmin, vmax),
        distinct_start_offsets: offsets.len(),
        warnings,
    }
}

impl ClipAnalysis {
    pub fn summary(&self) -> String {
        let mut s = format!(
            "{} / {}: {} notes over {} bars",
            self.track, self.section, self.note_count, self.bars
        );
        if let Some((lo, hi)) = &self.range {
            s.push_str(&format!(", range {lo}-{hi}"));
        }
        s.push_str(&format!(
            ", largest leap {} st, rests {:.0}%, vel {}-{}, density/bar {:?}",
            self.largest_leap,
            self.rest_ratio * 100.0,
            self.velocity_range.0,
            self.velocity_range.1,
            self.density_per_bar
        ));
        if !self.out_of_key.is_empty() {
            s.push_str(&format!(", out of key: {}", self.out_of_key.join(" ")));
        }
        for w in &self.warnings {
            s.push_str("\n  ! ");
            s.push_str(w);
        }
        s
    }
}
