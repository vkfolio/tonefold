//! Kids' rhyme melodies: lyrics → syllables → rhythm (stressed syllables on strong beats),
//! singable range (≤ an octave), stepwise motion, simple repetition, clear cadences.

use super::{clamp_to_section, GenParams, SongCtx};
use crate::model::{Note, Section, Session, PPQ};
use crate::theory::Key;
use rand::Rng;

/// Very small English syllabifier: splits on vowel groups, keeps consonant clusters attached.
pub fn syllables(word: &str) -> Vec<String> {
    let w: String = word.chars().filter(|c| c.is_alphanumeric() || *c == '\'' || *c == '-').collect();
    if w.is_empty() {
        return vec![];
    }
    if let Some(parts) = w.split('-').map(str::to_string).collect::<Vec<_>>().into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().into_iter().collect::<Vec<_>>().get(1..) {
        if !parts.is_empty() {
            let mut all = vec![w.split('-').next().unwrap().to_string()];
            all.extend(parts.iter().cloned());
            return all.into_iter().flat_map(|p| syllables(&p)).collect();
        }
    }
    let chars: Vec<char> = w.chars().collect();
    let is_vowel = |c: char| "aeiouyAEIOUY".contains(c);
    let mut groups: Vec<(usize, usize)> = Vec::new(); // vowel group ranges
    let mut i = 0;
    while i < chars.len() {
        if is_vowel(chars[i]) {
            let start = i;
            while i < chars.len() && is_vowel(chars[i]) {
                i += 1;
            }
            groups.push((start, i));
        } else {
            i += 1;
        }
    }
    // Silent trailing 'e' ("cake", "little" keeps 'le').
    if groups.len() > 1 {
        let (ls, le) = *groups.last().unwrap();
        let last_is_e = le == chars.len() && le - ls == 1 && (chars[ls] == 'e' || chars[ls] == 'E');
        let before_le = ls >= 1 && chars[ls - 1] == 'l' && ls >= 2 && !is_vowel(chars[ls - 2]);
        if last_is_e && !before_le && !(ls >= 1 && (chars[ls - 1] == 'l' || chars[ls - 1] == 'r') && false) {
            groups.pop();
        }
    }
    if groups.len() <= 1 {
        return vec![w];
    }
    let mut out = Vec::new();
    let mut start = 0;
    for gi in 0..groups.len() - 1 {
        let (_, ve) = groups[gi];
        let (ns, _) = groups[gi + 1];
        // Split consonants between vowel groups: one to the next syllable ("hap-py" splits doubles).
        let cons = ns - ve;
        let cut = if cons <= 1 { ve } else { ve + cons / 2 };
        out.push(chars[start..cut].iter().collect::<String>());
        start = cut;
    }
    out.push(chars[start..].iter().collect::<String>());
    out
}

/// Lines → syllables with a crude stress guess (first syllable of multi-syllable words stressed,
/// single-syllable function words unstressed).
fn syllabify_lines(lyrics: &str) -> Vec<Vec<(String, bool)>> {
    let function_words = ["a", "an", "the", "and", "or", "of", "to", "in", "on", "at", "is", "it", "its", "my", "your", "his", "her", "we", "you", "i", "me", "up", "so", "for", "with", "but", "as", "be", "are", "was"];
    lyrics
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|line| {
            let mut out = Vec::new();
            for word in line.split_whitespace() {
                let clean: String = word.chars().filter(|c| c.is_alphanumeric() || *c == '\'' || *c == '-').collect();
                if clean.is_empty() {
                    continue;
                }
                let syls = syllables(&clean);
                let n = syls.len();
                for (i, s) in syls.into_iter().enumerate() {
                    let stressed = if n == 1 { !function_words.contains(&clean.to_ascii_lowercase().as_str()) } else { i == 0 };
                    out.push((s, stressed));
                }
            }
            out
        })
        .collect()
}

/// Simple nursery-rhyme contour: degrees over an 8-note skeleton, repeated with variation.
fn skeleton(rng: &mut impl Rng) -> Vec<i32> {
    let options: [&[i32]; 6] = [
        &[0, 0, 4, 4, 5, 5, 4, 2, 2, 1, 1, 0], // Twinkle-like: sol-sol-la-la-sol / fa-fa-mi-mi-re-re-do
        &[2, 1, 0, 1, 2, 2, 2, 1, 1, 1, 2, 4], // Mary-had-a-little-lamb
        &[0, 1, 2, 0, 0, 1, 2, 0, 2, 3, 4, 2],
        &[4, 2, 4, 2, 0, 1, 2, 3, 4, 4, 2, 0],
        &[0, 2, 4, 4, 2, 0, 1, 1, 2, 2, 0, 0],
        &[0, 0, 1, 2, 2, 1, 0, 4, 4, 3, 2, 1],
    ];
    options[rng.random_range(0..options.len())].to_vec()
}

pub fn generate_kids_melody(session: &Session, section: &Section, params: &GenParams, ctx: SongCtx) -> Vec<Note> {
    let key: Key = session.key;
    let mut rng = params.rng_in(session, 51, section, ctx);
    let bar = session.bar_ticks();
    let total = section.bars * bar;
    let q = PPQ;
    let e = q / 2;
    let lyrics = params.lyrics.clone().or_else(|| section.lyrics.clone());
    let tonic = key.degree_pitch(0, 4); // C4-ish
    let shape = skeleton(&mut rng);
    let mut notes: Vec<Note> = Vec::new();

    let lines: Vec<Vec<(String, bool)>> = match &lyrics {
        Some(l) => syllabify_lines(l),
        None => vec![],
    };

    if lines.is_empty() {
        // No lyrics: 2-bar phrases of quarter notes with the last note held, from the skeleton.
        let mut t = 0u32;
        let mut i = 0usize;
        while t < total {
            let bar_idx = t / bar;
            let in_phrase = bar_idx % 2;
            let last_beat = (t % bar) / q == session.time_sig.num - 1 && in_phrase == 1;
            let deg = shape[i % shape.len()];
            let pitch = key.degree_pitch(deg, 4);
            let len = if last_beat { q * 2 } else { q };
            notes.push(Note::new(pitch, t, len - len / 8, if t % bar == 0 { 0.85 } else { 0.72 }));
            t += len;
            i += 1;
            if last_beat {
                t = (bar_idx + 1) * bar; // rest for the remainder
            }
        }
    } else {
        // Each lyric line gets a phrase; try to fit lines into equal chunks of the section.
        let n_lines = lines.len() as u32;
        let bars_per_line = (section.bars / n_lines).max(1);
        let mut shape_i = 0usize;
        for (li, line) in lines.iter().enumerate() {
            let line_start = li as u32 * bars_per_line * bar;
            if line_start >= total {
                break;
            }
            let avail = bars_per_line * bar - q; // leave a beat of breath at the end of each line
            let n_syl = line.len().max(1) as u32;
            // Base unit: quarter notes if they fit, else eighths.
            let unit = if n_syl * q <= avail { q } else { e };
            let mut t = line_start;
            // Align the first stressed syllable to the downbeat: unstressed lead-ins become pickups before it.
            let first_stress = line.iter().position(|(_, s)| *s).unwrap_or(0);
            if first_stress > 0 {
                t = line_start; // pickups start on the bar; stressed syllable lands on a beat below
            }
            for (si, (syl, stressed)) in line.iter().enumerate() {
                if t >= line_start + bars_per_line * bar {
                    break;
                }
                let is_last = si + 1 == line.len();
                let mut len = if *stressed && unit == e && (t % q) == 0 && si + 1 < line.len() && !line[si + 1].1 { unit } else { unit };
                if is_last {
                    len = (line_start + bars_per_line * bar - t).min(q * 2).max(unit);
                }
                let deg = shape[shape_i % shape.len()];
                shape_i += 1;
                let mut pitch = key.degree_pitch(deg, 4);
                // Stressed syllables prefer chord tones.
                if let Some(ch) = Session::chord_at(section, t) {
                    if *stressed && !ch.chord.contains(pitch) {
                        pitch = key.step(pitch, if rng.random_bool(0.5) { 1 } else { -1 });
                        if !ch.chord.contains(pitch) {
                            pitch = key.step(pitch, -2);
                        }
                    }
                }
                // Last syllable of the last line resolves to the tonic; other lines end on a chord tone.
                if is_last {
                    pitch = if li + 1 == lines.len() { tonic } else { key.degree_pitch(if li % 2 == 0 { 4 } else { 2 }, 4) };
                }
                let mut n = Note::new(pitch, t, len - len / 8, if *stressed { 0.85 } else { 0.7 });
                n.lyric = Some(syl.clone());
                notes.push(n);
                t += len;
            }
        }
    }
    // Keep everything within an octave above the tonic and stepwise (leap cap = a 5th).
    let lo = tonic;
    let hi = tonic + 12;
    for i in 0..notes.len() {
        let p = notes[i].pitch.clamp(lo, hi);
        notes[i].pitch = key.snap(p);
        if i > 0 {
            let prev = notes[i - 1].pitch as i32;
            let d = notes[i].pitch as i32 - prev;
            if d.abs() > 7 {
                notes[i].pitch = key.step(notes[i - 1].pitch, if d > 0 { 2 } else { -2 }).clamp(lo, hi);
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
    fn syllabifies() {
        assert_eq!(syllables("twinkle"), vec!["twin", "kle"]);
        assert_eq!(syllables("little"), vec!["lit", "tle"]);
        assert_eq!(syllables("star"), vec!["star"]);
        assert_eq!(syllables("happy"), vec!["hap", "py"]);
        assert_eq!(syllables("cake"), vec!["cake"]);
        assert_eq!(syllables("banana"), vec!["ba", "na", "na"]);
    }

    #[test]
    fn kids_melody_with_lyrics() {
        let mut s = Session::default();
        s.style = "kids".into();
        let id = s.add_section("Verse", 8, 0.5);
        s.section_mut(&id).unwrap().chords = parse_chords("| C | F | G | C |", &s.key, 8, s.bar_ticks()).unwrap();
        let p = GenParams { lyrics: Some("Twinkle twinkle little star\nHow I wonder what you are".into()), ..Default::default() };
        let sec = s.section(&id).unwrap().clone();
        let notes = generate_kids_melody(&s, &sec, &p, SongCtx::of(&s, &id));
        assert_eq!(notes.iter().filter(|n| n.lyric.is_some()).count(), 14);
        assert!(notes.iter().all(|n| n.pitch >= 60 && n.pitch <= 72));
        assert_eq!(notes.last().unwrap().pitch, 60);
        for w in notes.windows(2) {
            assert!((w[1].pitch as i32 - w[0].pitch as i32).abs() <= 7);
        }
    }
}
