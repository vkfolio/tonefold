//! Harmonize a melody: choose chord progressions that fit the notes already written.
//! Candidates are scored per segment (bar or half bar) by how well the melody's weighted pitches
//! sit inside each chord, then a Viterbi search picks the sequence with the best fit + functional
//! movement + cadences. Several variants (simple / full diatonic / rich extensions / half-bar) give
//! distinct alternatives.

use crate::humanize::metric_weight;
use crate::model::{ChordEvent, Note, PPQ};
use crate::theory::{Chord, ChordQuality, Extension, Key};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Complexity {
    /// I, IV, V, vi (and ii) triads.
    Simple,
    /// All diatonic triads.
    Diatonic,
    /// Diatonic with 7ths/9ths and a few borrowed colours.
    Rich,
}

impl Complexity {
    pub fn parse(s: &str) -> Complexity {
        match s.to_ascii_lowercase().as_str() {
            "simple" | "easy" | "kids" | "basic" => Complexity::Simple,
            "rich" | "complex" | "jazzy" | "extended" => Complexity::Rich,
            _ => Complexity::Diatonic,
        }
    }
}

/// A candidate chord with its degree (for transition scoring) and a display symbol.
#[derive(Debug, Clone)]
struct Cand {
    chord: Chord,
    degree: usize,
    borrowed: bool,
}

fn candidates(key: &Key, complexity: Complexity) -> Vec<Cand> {
    let mut out = Vec::new();
    let n = key.scale.intervals().len();
    if n != 7 {
        // Pentatonic/other: fall back to the parent major/minor triads.
        let parent = Key::new(key.root, if key.scale.is_minor() { crate::theory::ScaleKind::NaturalMinor } else { crate::theory::ScaleKind::Major });
        return candidates(&parent, complexity);
    }
    let minor = key.scale.is_minor();
    let simple_degrees: &[usize] = if minor { &[0, 5, 2, 6, 3, 4] } else { &[0, 3, 4, 5, 1] };
    for d in 0..7 {
        if complexity == Complexity::Simple && !simple_degrees.contains(&d) {
            continue;
        }
        let tri = key.diatonic_chord(d, false);
        // Skip the diminished vii° in simple/diatonic unless rich.
        if tri.quality == ChordQuality::Diminished && complexity != Complexity::Rich {
            continue;
        }
        out.push(Cand { chord: tri.clone(), degree: d, borrowed: false });
        if complexity == Complexity::Rich {
            out.push(Cand { chord: key.diatonic_chord(d, true), degree: d, borrowed: false });
            if matches!(tri.quality, ChordQuality::Major | ChordQuality::Minor) && d != 4 {
                let mut c = tri.clone();
                c.extensions.push(Extension::Add9);
                out.push(Cand { chord: c, degree: d, borrowed: false });
            }
        }
    }
    if minor && complexity != Complexity::Simple {
        // Harmonic-minor dominant.
        let mut v = Chord::new(key.root + 7, ChordQuality::Major);
        if complexity == Complexity::Rich {
            v.extensions.push(Extension::Min7);
        }
        out.push(Cand { chord: v, degree: 4, borrowed: true });
    }
    if !minor && complexity == Complexity::Rich {
        // Borrowed iv and bVII.
        out.push(Cand { chord: Chord::new(key.root + 5, ChordQuality::Minor), degree: 3, borrowed: true });
        out.push(Cand { chord: Chord::new(key.root + 10, ChordQuality::Major), degree: 6, borrowed: true });
    }
    out
}

/// Functional transition bonus between scale degrees (0-based).
fn transition(prev: usize, next: usize, minor: bool) -> f32 {
    let _ = minor;
    match (prev, next) {
        (a, b) if a == b => -0.35,
        (4, 0) => 0.9,  // V -> I
        (3, 0) => 0.5,  // IV -> I
        (3, 4) => 0.6,  // IV -> V
        (1, 4) => 0.7,  // ii -> V
        (5, 3) => 0.5,  // vi -> IV
        (5, 1) => 0.4,  // vi -> ii
        (0, 3) => 0.4,
        (0, 4) => 0.3,
        (0, 5) => 0.4,
        (2, 5) => 0.4,  // iii -> vi
        (2, 3) => 0.3,
        (6, 0) => 0.6,  // vii/bVII -> I
        (4, 5) => 0.5,  // deceptive
        (4, 3) => -0.3, // V -> IV weak
        _ => 0.0,
    }
}

fn segment_fit(chord: &Chord, notes: &[(u8, f32)]) -> f32 {
    // notes: (pitch, weight). Chord tones score +w, non-chord tones -0.8w, root/third bonuses.
    let mut score = 0.0;
    let mut total = 0.0;
    let pcs = chord.pitch_classes();
    for (p, w) in notes {
        total += w;
        if pcs.contains(&(p % 12)) {
            score += w;
            if p % 12 == chord.root {
                score += 0.15 * w;
            }
            if chord.third_pc() == Some(p % 12) {
                score += 0.2 * w;
            }
        } else {
            score -= 0.8 * w;
        }
    }
    if total > 0.0 { score / total } else { 0.0 }
}

/// Weighted melody pitches per segment.
fn segments(notes: &[Note], bars: u32, bar_ticks: u32, per_bar: u32) -> Vec<Vec<(u8, f32)>> {
    let seg_len = bar_ticks / per_bar.max(1);
    let count = (bars * per_bar) as usize;
    let mut segs = vec![Vec::new(); count];
    for n in notes {
        let mut t = n.start;
        let end = n.end();
        while t < end {
            let seg = (t / seg_len) as usize;
            if seg >= count {
                break;
            }
            let seg_end = (seg as u32 + 1) * seg_len;
            let dur = end.min(seg_end) - t;
            let w = (dur as f32 / PPQ as f32) * (0.5 + metric_weight(t, bar_ticks)) * (if t == n.start { 1.0 } else { 0.6 });
            segs[seg].push((n.pitch, w));
            t = seg_end;
        }
    }
    segs
}

pub struct Harmonized {
    pub events: Vec<ChordEvent>,
    pub score: f32,
    pub label: String,
}

/// Runs the Viterbi search for one configuration.
fn solve(key: &Key, notes: &[Note], bars: u32, bar_ticks: u32, per_bar: u32, complexity: Complexity, avoid: Option<&[Vec<u8>]>) -> Option<Harmonized> {
    let cands = candidates(key, complexity);
    if cands.is_empty() || bars == 0 {
        return None;
    }
    let segs = segments(notes, bars, bar_ticks, per_bar);
    let n = segs.len();
    let k = cands.len();
    let minor = key.scale.is_minor();
    let seg_len = bar_ticks / per_bar.max(1);
    let segs_per_phrase = (4 * per_bar) as usize;
    let mut dp = vec![vec![f32::NEG_INFINITY; k]; n];
    let mut back = vec![vec![0usize; k]; n];
    for (i, seg) in segs.iter().enumerate() {
        let is_first = i == 0;
        let is_last = i + 1 == n;
        let phrase_end = (i + 1) % segs_per_phrase == 0;
        let phrase_pos = i % segs_per_phrase;
        for (c, cand) in cands.iter().enumerate() {
            let mut local = if seg.is_empty() { 0.0 } else { segment_fit(&cand.chord, seg) * 2.0 };
            if cand.borrowed {
                local -= 0.45;
            }
            if !cand.chord.extensions.is_empty() {
                local -= 0.08;
            }
            if is_first && cand.degree != 0 {
                local -= 0.35;
            }
            if is_last {
                local += if cand.degree == 0 { 0.8 } else { -0.3 };
            } else if phrase_end {
                local += if cand.degree == 4 { 0.5 } else if cand.degree == 0 { 0.2 } else { 0.0 };
            }
            if phrase_pos == 0 && !is_first && cand.degree == 0 {
                local += 0.15;
            }
            if let Some(av) = avoid {
                if av.iter().any(|seq| seq.get(i).map(|d| *d as usize == cand.degree).unwrap_or(false)) {
                    local -= 0.25;
                }
            }
            if is_first {
                dp[i][c] = local;
            } else {
                let mut best = f32::NEG_INFINITY;
                let mut arg = 0;
                for (p, pc) in cands.iter().enumerate() {
                    let s = dp[i - 1][p] + transition(pc.degree, cand.degree, minor) + local;
                    if s > best {
                        best = s;
                        arg = p;
                    }
                }
                dp[i][c] = best;
                back[i][c] = arg;
            }
        }
    }
    let (mut c, score) = dp[n - 1].iter().enumerate().fold((0, f32::NEG_INFINITY), |(bi, bs), (i, &s)| if s > bs { (i, s) } else { (bi, bs) });
    let mut seq = vec![0usize; n];
    for i in (0..n).rev() {
        seq[i] = c;
        c = back[i][c];
    }
    // Merge consecutive identical chords in a bar into one event.
    let mut events: Vec<ChordEvent> = Vec::new();
    for (i, &ci) in seq.iter().enumerate() {
        let chord = cands[ci].chord.clone();
        let start = i as u32 * seg_len;
        match events.last_mut() {
            Some(last) if last.chord == chord && last.start / bar_ticks == start / bar_ticks => last.len += seg_len,
            _ => events.push(ChordEvent { start, len: seg_len, symbol: chord.symbol(), chord }),
        }
    }
    let label = format!("{:?}{}", complexity, if per_bar > 1 { " · 2 chords/bar" } else { "" }).to_ascii_lowercase();
    Some(Harmonized { events, score: score / n as f32, label })
}

/// Produces up to `count` distinct progressions that fit `notes`. `complexity` steers the palette;
/// `None` mixes simple, diatonic and rich variants.
pub fn harmonize(key: &Key, notes: &[Note], bars: u32, bar_ticks: u32, complexity: Option<Complexity>, count: usize) -> Vec<Harmonized> {
    let configs: Vec<(Complexity, u32)> = match complexity {
        Some(c) => vec![(c, 1), (c, 2)],
        None => vec![(Complexity::Simple, 1), (Complexity::Diatonic, 1), (Complexity::Rich, 1), (Complexity::Diatonic, 2), (Complexity::Rich, 2)],
    };
    let mut out: Vec<Harmonized> = Vec::new();
    let mut seen: Vec<Vec<u8>> = Vec::new();
    for (c, per_bar) in configs {
        // Ask for something different from what we already have.
        let avoid: Vec<Vec<u8>> = seen.iter().map(|s| if per_bar == 1 { s.iter().step_by(2).copied().collect() } else { s.iter().flat_map(|d| [*d, *d]).collect() }).collect();
        if let Some(h) = solve(key, notes, bars, bar_ticks, per_bar, c, if avoid.is_empty() { None } else { Some(&avoid) }) {
            let sig: Vec<u8> = (0..bars * 2).map(|half| {
                let t = half * bar_ticks / 2;
                h.events.iter().filter(|e| e.start <= t).last().and_then(|e| key.degree_of(e.chord.root)).unwrap_or(0) as u8
            }).collect();
            if !seen.contains(&sig) {
                seen.push(sig);
                out.push(h);
            }
        }
        if out.len() >= count {
            break;
        }
    }
    // Second pass: more seeds with the same configs if still short.
    let mut extra = 0;
    while out.len() < count && extra < 3 {
        extra += 1;
        let avoid: Vec<Vec<u8>> = seen.iter().map(|s| s.iter().step_by(2).copied().collect()).collect();
        if let Some(h) = solve(key, notes, bars, bar_ticks, 1, Complexity::Diatonic, Some(&avoid)) {
            let sig: Vec<u8> = (0..bars * 2).map(|half| {
                let t = half * bar_ticks / 2;
                h.events.iter().filter(|e| e.start <= t).last().and_then(|e| key.degree_of(e.chord.root)).unwrap_or(0) as u8
            }).collect();
            if seen.contains(&sig) {
                break;
            }
            seen.push(sig);
            out.push(h);
        } else {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notation::parse_melody;
    use crate::theory::ScaleKind;

    #[test]
    fn twinkle_gets_i_iv_v() {
        let key = Key::new(0, ScaleKind::Major);
        // Twinkle twinkle little star, how I wonder what you are (4 bars).
        let m = parse_melody("C4:4 C4:4 G4:4 G4:4 | A4:4 A4:4 G4:2 | F4:4 F4:4 E4:4 E4:4 | D4:4 D4:4 C4:2", 0.8).unwrap();
        let res = harmonize(&key, &m, 4, PPQ * 4, Some(Complexity::Simple), 2);
        assert!(!res.is_empty());
        let first = &res[0].events;
        assert_eq!(first[0].chord.root, 0, "starts on I");
        assert_eq!(first.last().unwrap().chord.root, 0, "ends on I");
        // Bar 2 (A A G) should be F or C, not G7-ish; bar 4 (D D C) should include G or Dm then C.
        let bar2 = first.iter().find(|e| e.start == PPQ * 4).unwrap();
        assert!(bar2.chord.root == 5 || bar2.chord.root == 0 || bar2.chord.root == 9, "{}", bar2.chord.symbol());
        let all = harmonize(&key, &m, 4, PPQ * 4, None, 3);
        assert!(all.len() >= 2);
        for h in &all {
            assert!(h.events.iter().all(|e| e.start < PPQ * 16));
        }
    }
}
