//! Compact text notation — the language the agent speaks.
//!
//! * Chords: `| C | Am . F | G7 |`  — one bar per `|…|` cell, `.` splits a bar evenly between chords,
//!   roman numerals (`| I | vi | IV | V |`) are resolved against the session key. A leading
//!   `Verse:` label is ignored.
//! * Melody / bass: whitespace-separated tokens `E4:8 F4:8 G4:4 r:8 G4:4. C5:2~ C5:4`
//!   — `pitch:denominator` (4 = quarter, 8 = eighth, 16, 2, 1; a trailing `.` dots the note,
//!   `t` makes a triplet), `r:` is a rest, `~` ties into the next note of the same pitch,
//!   `@v80` after a token sets velocity (0..127), `/syl-la-ble` attaches a lyric.
//!   `|` bar lines are optional and ignored (but validated against bar length when present).
//! * Drums: one lane per line, `K: x---x---x---x---` with lane names K/S/H/OH/CL/T1/T2/T3/RD/CR/P
//!   (kick, snare, closed hat, open hat, clap, toms, ride, crash, percussion). One step = a 16th
//!   note by default (`@8` or `@32` after the lane name changes resolution). Symbols: `x` hit,
//!   `X` accent, `g` ghost, `-`/`.` rest, `f` flam.

use crate::model::{ChordEvent, Note, PPQ};
use crate::theory::{parse_pitch, Chord, Key};
use crate::{Error, Result};

/// Parses a chord line into chord events for a section of `bars` bars.
pub fn parse_chords(text: &str, key: &Key, bars: u32, bar_ticks: u32) -> Result<Vec<ChordEvent>> {
    let text = text.trim();
    let text = match text.find(':') {
        Some(i) if !text[..i].contains('|') && text[..i].chars().all(|c| c.is_alphanumeric() || c == ' ' || c == '_') => &text[i + 1..],
        _ => text,
    };
    let cells: Vec<&str> = text.split('|').map(str::trim).filter(|c| !c.is_empty()).collect();
    if cells.is_empty() {
        return Err(Error::Parse("no chords found; expected e.g. `| C | Am | F | G |`".into()));
    }
    let mut events = Vec::new();
    let mut last: Option<Chord> = None;
    for (bar_idx, cell) in cells.iter().enumerate() {
        let toks: Vec<&str> = cell.split_whitespace().collect();
        let n = toks.len().max(1) as u32;
        let slot = bar_ticks / n;
        for (i, tok) in toks.iter().enumerate() {
            let start = (bar_idx as u32) * bar_ticks + i as u32 * slot;
            if *tok == "." || *tok == "%" || *tok == "-" {
                // Continue the previous chord: extend its length.
                if let Some(ev) = events.last_mut() {
                    let ev: &mut ChordEvent = ev;
                    ev.len = start + slot - ev.start;
                } else if let Some(c) = &last {
                    events.push(ChordEvent { start, len: slot, chord: c.clone(), symbol: c.symbol() });
                } else {
                    return Err(Error::Parse(format!("bar {}: continuation with no previous chord", bar_idx + 1)));
                }
                continue;
            }
            let chord = Chord::parse(tok, key).ok_or_else(|| Error::Parse(format!("bar {}: cannot parse chord '{}'", bar_idx + 1, tok)))?;
            last = Some(chord.clone());
            events.push(ChordEvent { start, len: slot, chord, symbol: tok.to_string() });
        }
    }
    // Repeat the pattern to fill the section if fewer bars were written.
    let written_bars = cells.len() as u32;
    if written_bars < bars && !events.is_empty() {
        let pattern = events.clone();
        let mut b = written_bars;
        while b < bars {
            for ev in &pattern {
                let start = ev.start + b * bar_ticks;
                if start < bars * bar_ticks {
                    events.push(ChordEvent { start, len: ev.len, ..ev.clone() });
                }
            }
            b += written_bars;
        }
    }
    events.retain(|e| e.start < bars * bar_ticks);
    for e in &mut events {
        let max = bars * bar_ticks - e.start;
        e.len = e.len.min(max);
    }
    Ok(events)
}

pub fn format_chords(events: &[ChordEvent], key: &Key, bars: u32, bar_ticks: u32) -> String {
    let mut out = String::from("|");
    for b in 0..bars {
        let lo = b * bar_ticks;
        let hi = lo + bar_ticks;
        let in_bar: Vec<&ChordEvent> = events.iter().filter(|e| e.start >= lo && e.start < hi).collect();
        if in_bar.is_empty() {
            out.push_str(" . |");
            continue;
        }
        let parts: Vec<String> = in_bar.iter().map(|e| format!("{} ({})", e.chord.symbol(), e.chord.roman(key))).collect();
        out.push(' ');
        out.push_str(&parts.join(" "));
        out.push_str(" |");
    }
    out
}

struct Tok<'a> {
    text: &'a str,
}

/// Parses melodic notation into notes. `default_vel` is 0..1.
pub fn parse_melody(text: &str, default_vel: f32) -> Result<Vec<Note>> {
    let mut notes: Vec<Note> = Vec::new();
    let mut cursor: u32 = 0;
    let mut pending_tie: Option<usize> = None;
    let quarter = PPQ;
    for (i, raw) in text.split_whitespace().enumerate() {
        let tok = Tok { text: raw.trim_matches(',') };
        if tok.text == "|" || tok.text.is_empty() {
            continue;
        }
        let t = tok.text.trim_matches('|');
        if t.is_empty() {
            continue;
        }
        // Split off lyric.
        let (t, lyric) = match t.split_once('/') {
            Some((a, l)) => (a, Some(l.to_string())),
            None => (t, None),
        };
        // Split off velocity.
        let (t, vel) = match t.split_once("@v") {
            Some((a, v)) => (a, v.parse::<f32>().map(|x| x / 127.0).ok()),
            None => (t, None),
        };
        let tie = t.ends_with('~');
        let t = t.trim_end_matches('~');
        let (pitch_part, dur_part) = t.split_once(':').ok_or_else(|| Error::Parse(format!("token {} '{}': expected pitch:duration", i + 1, raw)))?;
        let (mut dur_str, mut dots, mut triplet) = (dur_part, 0u32, false);
        while dur_str.ends_with('.') {
            dots += 1;
            dur_str = &dur_str[..dur_str.len() - 1];
        }
        if dur_str.ends_with('t') {
            triplet = true;
            dur_str = &dur_str[..dur_str.len() - 1];
        }
        let denom: u32 = dur_str.parse().map_err(|_| Error::Parse(format!("token {} '{}': bad duration", i + 1, raw)))?;
        if denom == 0 || denom > 128 {
            return Err(Error::Parse(format!("token {} '{}': duration must be 1..128", i + 1, raw)));
        }
        let mut len = quarter * 4 / denom;
        let mut extra = len / 2;
        for _ in 0..dots {
            len += extra;
            extra /= 2;
        }
        if triplet {
            len = len * 2 / 3;
        }
        if pitch_part.eq_ignore_ascii_case("r") {
            cursor += len;
            pending_tie = None;
            continue;
        }
        let (pitch, _) = parse_pitch(pitch_part).ok_or_else(|| Error::Parse(format!("token {} '{}': bad pitch", i + 1, raw)))?;
        if let Some(idx) = pending_tie.take() {
            if notes[idx].pitch == pitch && notes[idx].end() == cursor {
                notes[idx].len += len;
                cursor += len;
                if tie {
                    pending_tie = Some(idx);
                }
                continue;
            }
        }
        let mut n = Note::new(pitch, cursor, len, vel.unwrap_or(default_vel));
        n.lyric = lyric;
        notes.push(n);
        if tie {
            pending_tie = Some(notes.len() - 1);
        }
        cursor += len;
    }
    Ok(notes)
}

/// Renders notes back into melodic notation (monophonic; overlapping notes are emitted in order).
pub fn format_melody(notes: &[Note]) -> String {
    let mut out = Vec::new();
    let mut cursor = 0u32;
    let mut sorted: Vec<&Note> = notes.iter().collect();
    sorted.sort_by_key(|n| (n.start, n.pitch));
    for n in sorted {
        if n.start > cursor {
            out.extend(dur_tokens("r", n.start - cursor, None));
        }
        let mut tail = format!("@v{}", n.vel_midi());
        if let Some(l) = &n.lyric {
            tail.push('/');
            tail.push_str(l);
        }
        out.extend(dur_tokens(&crate::theory::pitch_name(n.pitch), n.len, Some(&tail)));
        cursor = n.end().max(cursor);
    }
    out.join(" ")
}

fn dur_tokens(name: &str, mut ticks: u32, tail: Option<&str>) -> Vec<String> {
    // Greedy decomposition into 1,2,4,8,16,32 (+dotted) note values; ties between parts.
    let table: [(u32, &str); 11] = [
        (PPQ * 6, "1."),
        (PPQ * 4, "1"),
        (PPQ * 3, "2."),
        (PPQ * 2, "2"),
        (PPQ * 3 / 2, "4."),
        (PPQ, "4"),
        (PPQ * 3 / 4, "8."),
        (PPQ / 2, "8"),
        (PPQ / 4, "16"),
        (PPQ / 6, "8t"),
        (PPQ / 8, "32"),
    ];
    let mut parts = Vec::new();
    let mut first = true;
    while ticks >= PPQ / 8 {
        let (t, d) = table.iter().find(|(t, _)| *t <= ticks).copied().unwrap();
        ticks -= t;
        let mut s = format!("{name}:{d}");
        if first {
            if let Some(tail) = tail {
                s.push_str(tail);
            }
            first = false;
        }
        parts.push(s);
    }
    if parts.is_empty() {
        return vec![];
    }
    if name != "r" {
        let n = parts.len();
        for p in parts.iter_mut().take(n - 1) {
            // Insert tie marker before the @v suffix if present.
            if let Some(i) = p.find('@') {
                p.insert(i, '~');
            } else {
                p.push('~');
            }
        }
    }
    parts
}

/// General MIDI drum map used by the drum lanes.
pub fn drum_lane_pitch(lane: &str) -> Option<u8> {
    Some(match lane.to_ascii_uppercase().as_str() {
        "K" | "KICK" | "BD" => 36,
        "S" | "SN" | "SNARE" | "SD" => 38,
        "RS" | "RIM" => 37,
        "CL" | "CLAP" | "CP" => 39,
        "H" | "HH" | "CH" | "HAT" => 42,
        "PH" => 44,
        "OH" | "OPEN" => 46,
        "T1" | "HT" => 50,
        "T2" | "MT" => 47,
        "T3" | "LT" | "FT" => 43,
        "RD" | "RIDE" => 51,
        "CR" | "CRASH" => 49,
        "P" | "PERC" | "SH" | "SHAKER" => 70,
        "TB" | "TAMB" => 54,
        "CB" | "COWBELL" => 56,
        _ => return None,
    })
}

pub fn drum_pitch_lane(pitch: u8) -> &'static str {
    match pitch {
        35 | 36 => "K",
        38 | 40 => "S",
        37 => "RS",
        39 => "CL",
        42 => "H",
        44 => "PH",
        46 => "OH",
        50 | 48 => "T1",
        47 | 45 => "T2",
        43 | 41 => "T3",
        51 | 59 => "RD",
        49 | 57 | 52 | 55 => "CR",
        54 => "TB",
        56 => "CB",
        _ => "P",
    }
}

/// Parses drum lanes. Each line: `LANE[@res]: pattern` where pattern chars are steps.
pub fn parse_drums(text: &str) -> Result<Vec<Note>> {
    let mut notes = Vec::new();
    for (li, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (head, pat) = line.split_once(':').ok_or_else(|| Error::Parse(format!("drum line {}: expected `LANE: pattern`", li + 1)))?;
        let (lane, res) = match head.trim().split_once('@') {
            Some((l, r)) => (l.trim(), r.trim().parse::<u32>().map_err(|_| Error::Parse(format!("drum line {}: bad resolution", li + 1)))?),
            None => (head.trim(), 16),
        };
        let pitch = drum_lane_pitch(lane).ok_or_else(|| Error::Parse(format!("drum line {}: unknown lane '{}'", li + 1, lane)))?;
        let step = PPQ * 4 / res;
        let mut idx = 0u32;
        for ch in pat.chars() {
            match ch {
                ' ' | '|' | '\t' => continue,
                '-' | '.' | '_' => {}
                'x' => notes.push(Note::new(pitch, idx * step, step * 3 / 4, 0.8)),
                'X' => notes.push(Note::new(pitch, idx * step, step * 3 / 4, 1.0)),
                'g' => notes.push(Note::new(pitch, idx * step, step / 2, 0.3)),
                'f' => {
                    notes.push(Note::new(pitch, (idx * step).saturating_sub(PPQ / 24), step / 4, 0.45));
                    notes.push(Note::new(pitch, idx * step, step * 3 / 4, 0.9));
                }
                other => return Err(Error::Parse(format!("drum line {}: unknown step symbol '{}'", li + 1, other))),
            }
            idx += 1;
        }
    }
    notes.sort_by_key(|n| (n.start, n.pitch));
    Ok(notes)
}

/// Renders drum notes to lanes at 16th resolution (finer notes get rounded).
pub fn format_drums(notes: &[Note], bars: u32, bar_ticks: u32) -> String {
    let step = PPQ / 4;
    let steps = (bars * bar_ticks / step).max(1) as usize;
    let mut lanes: std::collections::BTreeMap<&'static str, Vec<char>> = Default::default();
    for n in notes {
        let lane = drum_pitch_lane(n.pitch);
        let row = lanes.entry(lane).or_insert_with(|| vec!['-'; steps]);
        let i = ((n.start + step / 2) / step) as usize;
        if i < steps {
            row[i] = if n.vel < 0.5 { 'g' } else if n.vel > 0.95 { 'X' } else { 'x' };
        }
    }
    let order = ["K", "S", "RS", "CL", "H", "PH", "OH", "T1", "T2", "T3", "RD", "CR", "TB", "CB", "P"];
    let mut out = String::new();
    for lane in order {
        if let Some(row) = lanes.get(lane) {
            let mut s = String::new();
            for (i, c) in row.iter().enumerate() {
                if i > 0 && i % 16 == 0 {
                    s.push_str(" | ");
                } else if i > 0 && i % 4 == 0 {
                    s.push(' ');
                }
                s.push(*c);
            }
            out.push_str(&format!("{lane:>2}: {s}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theory::ScaleKind;

    #[test]
    fn chords_roundtrip() {
        let key = Key::new(0, ScaleKind::Major);
        let ev = parse_chords("Verse: | C | Am . F | G7 |", &key, 4, PPQ * 4).unwrap();
        assert_eq!(ev.len(), 5, "3 written bars repeat to fill 4");
        assert_eq!(ev[1].len, PPQ * 8 / 3);
        assert_eq!(ev[2].start, PPQ * 4 + PPQ * 8 / 3);
        assert_eq!(ev[3].start, PPQ * 8);
        assert_eq!(ev[4].start, PPQ * 12);
        assert_eq!(ev[4].chord.symbol(), "C");
        let ev = parse_chords("| I | vi | IV | V |", &key, 8, PPQ * 4).unwrap();
        assert_eq!(ev.len(), 8);
        assert_eq!(ev[7].chord.symbol(), "G");
        assert!(parse_chords("| C | H |", &key, 2, PPQ * 4).is_err());
    }

    #[test]
    fn melody_parse() {
        let n = parse_melody("E4:8 E4:8 F4:8 G4:4. r:8 C5:4~ C5:4@v100/la", 0.8).unwrap();
        assert_eq!(n.len(), 5);
        assert_eq!(n[3].start, PPQ / 2 * 3);
        assert_eq!(n[4].len, PPQ * 2);
        assert_eq!(n[4].lyric.as_deref(), None); // lyric belongs to the second half which merged
        let m = parse_melody("C4:8t C4:8t C4:8t", 0.8).unwrap();
        assert_eq!(m[2].end(), PPQ);
        let f = format_melody(&n);
        let back = parse_melody(&f, 0.8).unwrap();
        assert_eq!(back.iter().map(|x| (x.pitch, x.start, x.len)).collect::<Vec<_>>(), n.iter().map(|x| (x.pitch, x.start, x.len)).collect::<Vec<_>>());
    }

    #[test]
    fn drums_parse() {
        let d = parse_drums("K: x---x---x---x---\nS: ----X-------X-g-\nH@8: x-x-x-x-").unwrap();
        assert_eq!(d.iter().filter(|n| n.pitch == 36).count(), 4);
        assert_eq!(d.iter().filter(|n| n.pitch == 38).count(), 3);
        assert_eq!(d.iter().filter(|n| n.pitch == 42).count(), 4);
        let s = format_drums(&d, 1, PPQ * 4);
        assert!(s.contains(" K: x--- x--- x--- x---"));
    }
}
