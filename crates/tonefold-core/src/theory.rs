//! Music theory primitives: pitch classes, scales, chords, keys, roman numerals.

use serde::{Deserialize, Serialize};

pub const NOTE_NAMES_SHARP: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
pub const NOTE_NAMES_FLAT: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"];

/// Parses a pitch-class name like "C", "F#", "Bb", "e". Returns 0..=11.
pub fn parse_pitch_class(s: &str) -> Option<(u8, &str)> {
    let mut chars = s.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let mut pc: i32 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let mut rest = &s[1..];
    loop {
        match rest.chars().next() {
            Some('#') => {
                pc += 1;
                rest = &rest[1..];
            }
            Some('b') => {
                pc -= 1;
                rest = &rest[1..];
            }
            _ => break,
        }
    }
    Some((pc.rem_euclid(12) as u8, rest))
}

/// Parses "C4", "F#3", "Bb5" into a MIDI note number (C4 = 60). Returns (note, rest).
pub fn parse_pitch(s: &str) -> Option<(u8, &str)> {
    let (pc, rest) = parse_pitch_class(s)?;
    let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '-')).unwrap_or(rest.len());
    let oct: i32 = rest[..end].parse().ok()?;
    let n = (oct + 1) * 12 + pc as i32;
    if (0..=127).contains(&n) {
        Some((n as u8, &rest[end..]))
    } else {
        None
    }
}

pub fn pitch_name(n: u8) -> String {
    format!("{}{}", NOTE_NAMES_SHARP[(n % 12) as usize], (n as i32 / 12) - 1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleKind {
    Major,
    NaturalMinor,
    HarmonicMinor,
    MelodicMinor,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Locrian,
    MajorPentatonic,
    MinorPentatonic,
    Blues,
    WholeTone,
    Chromatic,
}

impl ScaleKind {
    pub const ALL: [ScaleKind; 14] = [
        ScaleKind::Major,
        ScaleKind::NaturalMinor,
        ScaleKind::HarmonicMinor,
        ScaleKind::MelodicMinor,
        ScaleKind::Dorian,
        ScaleKind::Phrygian,
        ScaleKind::Lydian,
        ScaleKind::Mixolydian,
        ScaleKind::Locrian,
        ScaleKind::MajorPentatonic,
        ScaleKind::MinorPentatonic,
        ScaleKind::Blues,
        ScaleKind::WholeTone,
        ScaleKind::Chromatic,
    ];

    pub fn intervals(self) -> &'static [u8] {
        match self {
            ScaleKind::Major => &[0, 2, 4, 5, 7, 9, 11],
            ScaleKind::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
            ScaleKind::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            ScaleKind::MelodicMinor => &[0, 2, 3, 5, 7, 9, 11],
            ScaleKind::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            ScaleKind::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            ScaleKind::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            ScaleKind::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            ScaleKind::Locrian => &[0, 1, 3, 5, 6, 8, 10],
            ScaleKind::MajorPentatonic => &[0, 2, 4, 7, 9],
            ScaleKind::MinorPentatonic => &[0, 3, 5, 7, 10],
            ScaleKind::Blues => &[0, 3, 5, 6, 7, 10],
            ScaleKind::WholeTone => &[0, 2, 4, 6, 8, 10],
            ScaleKind::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ScaleKind::Major => "major",
            ScaleKind::NaturalMinor => "minor",
            ScaleKind::HarmonicMinor => "harmonic minor",
            ScaleKind::MelodicMinor => "melodic minor",
            ScaleKind::Dorian => "dorian",
            ScaleKind::Phrygian => "phrygian",
            ScaleKind::Lydian => "lydian",
            ScaleKind::Mixolydian => "mixolydian",
            ScaleKind::Locrian => "locrian",
            ScaleKind::MajorPentatonic => "major pentatonic",
            ScaleKind::MinorPentatonic => "minor pentatonic",
            ScaleKind::Blues => "blues",
            ScaleKind::WholeTone => "whole tone",
            ScaleKind::Chromatic => "chromatic",
        }
    }

    pub fn parse(s: &str) -> Option<ScaleKind> {
        let s = s.trim().to_ascii_lowercase().replace('_', " ").replace('-', " ");
        Some(match s.as_str() {
            "major" | "ionian" | "maj" | "" => ScaleKind::Major,
            "minor" | "natural minor" | "aeolian" | "min" | "m" => ScaleKind::NaturalMinor,
            "harmonic minor" => ScaleKind::HarmonicMinor,
            "melodic minor" => ScaleKind::MelodicMinor,
            "dorian" => ScaleKind::Dorian,
            "phrygian" => ScaleKind::Phrygian,
            "lydian" => ScaleKind::Lydian,
            "mixolydian" => ScaleKind::Mixolydian,
            "locrian" => ScaleKind::Locrian,
            "major pentatonic" | "pentatonic" => ScaleKind::MajorPentatonic,
            "minor pentatonic" => ScaleKind::MinorPentatonic,
            "blues" => ScaleKind::Blues,
            "whole tone" => ScaleKind::WholeTone,
            "chromatic" => ScaleKind::Chromatic,
            _ => return None,
        })
    }

    pub fn is_minor(self) -> bool {
        matches!(
            self,
            ScaleKind::NaturalMinor
                | ScaleKind::HarmonicMinor
                | ScaleKind::MelodicMinor
                | ScaleKind::Dorian
                | ScaleKind::Phrygian
                | ScaleKind::MinorPentatonic
                | ScaleKind::Blues
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Key {
    /// Pitch class of the tonic, 0..=11.
    pub root: u8,
    pub scale: ScaleKind,
}

impl Key {
    pub fn new(root: u8, scale: ScaleKind) -> Self {
        Key { root: root % 12, scale }
    }

    /// Parses "C major", "A minor", "F# dorian", "Bb", "Am", "Dm".
    pub fn parse(s: &str) -> Option<Key> {
        let s = s.trim();
        let (root, rest) = parse_pitch_class(s)?;
        let rest = rest.trim();
        if rest == "m" {
            return Some(Key::new(root, ScaleKind::NaturalMinor));
        }
        let scale = ScaleKind::parse(rest)?;
        Some(Key::new(root, scale))
    }

    pub fn name(&self) -> String {
        format!("{} {}", NOTE_NAMES_SHARP[self.root as usize], self.scale.name())
    }

    pub fn pitch_classes(&self) -> Vec<u8> {
        self.scale.intervals().iter().map(|i| (self.root + i) % 12).collect()
    }

    pub fn contains(&self, pitch: u8) -> bool {
        let pc = pitch % 12;
        self.scale.intervals().iter().any(|i| (self.root + i) % 12 == pc)
    }

    /// Scale degree (0-based) of a pitch class if diatonic.
    pub fn degree_of(&self, pitch: u8) -> Option<usize> {
        let pc = pitch % 12;
        self.scale.intervals().iter().position(|i| (self.root + i) % 12 == pc)
    }

    /// Pitch for a 0-based scale degree in a given octave (degree may exceed the scale length or be negative).
    pub fn degree_pitch(&self, degree: i32, octave: i32) -> u8 {
        let n = self.scale.intervals().len() as i32;
        let oct_shift = degree.div_euclid(n);
        let d = degree.rem_euclid(n) as usize;
        let p = (octave + 1 + oct_shift) * 12 + self.root as i32 + self.scale.intervals()[d] as i32;
        p.clamp(0, 127) as u8
    }

    /// Degree index of a pitch on an unbounded diatonic ladder (…, -1, 0, 1, …), snapping non-diatonic pitches down.
    pub fn pitch_to_degree_index(&self, pitch: u8) -> i32 {
        let n = self.scale.intervals().len() as i32;
        let rel = (pitch as i32 - self.root as i32).rem_euclid(12);
        let oct = (pitch as i32 - self.root as i32).div_euclid(12);
        let ints = self.scale.intervals();
        let mut d = 0;
        for (i, &iv) in ints.iter().enumerate() {
            if iv as i32 <= rel {
                d = i as i32;
            }
        }
        oct * n + d
    }

    /// Moves a pitch by `steps` scale degrees.
    pub fn step(&self, pitch: u8, steps: i32) -> u8 {
        let idx = self.pitch_to_degree_index(pitch) + steps;
        let n = self.scale.intervals().len() as i32;
        let oct = idx.div_euclid(n);
        let d = idx.rem_euclid(n) as usize;
        let p = self.root as i32 + oct * 12 + self.scale.intervals()[d] as i32;
        p.clamp(0, 127) as u8
    }

    /// Nearest diatonic pitch (ties go downward).
    pub fn snap(&self, pitch: u8) -> u8 {
        if self.contains(pitch) {
            return pitch;
        }
        for d in 1..=6u8 {
            if pitch >= d && self.contains(pitch - d) {
                return pitch - d;
            }
            if pitch + d <= 127 && self.contains(pitch + d) {
                return pitch + d;
            }
        }
        pitch
    }

    /// Diatonic triad quality for a 0-based degree (only meaningful for 7-note scales).
    pub fn diatonic_chord(&self, degree: usize, seventh: bool) -> Chord {
        let ints = self.scale.intervals();
        let n = ints.len();
        let get = |k: usize| (self.root + ints[(degree + k) % n] + if (degree + k) >= n { 12 } else { 0 }) % 12;
        let root = get(0);
        let third = (get(2) as i32 - root as i32).rem_euclid(12);
        let fifth = (get(4) as i32 - root as i32).rem_euclid(12);
        let quality = match (third, fifth) {
            (4, 7) => ChordQuality::Major,
            (3, 7) => ChordQuality::Minor,
            (3, 6) => ChordQuality::Diminished,
            (4, 8) => ChordQuality::Augmented,
            _ => ChordQuality::Major,
        };
        let mut chord = Chord::new(root, quality);
        if seventh {
            let sev = (get(6) as i32 - root as i32).rem_euclid(12);
            chord.extensions.push(if sev == 11 { Extension::Maj7 } else { Extension::Min7 });
        }
        chord
    }

    /// Relative key (major <-> minor) sharing the same pitch classes.
    pub fn relative(&self) -> Key {
        match self.scale {
            ScaleKind::Major => Key::new(self.root + 9, ScaleKind::NaturalMinor),
            ScaleKind::NaturalMinor => Key::new(self.root + 3, ScaleKind::Major),
            _ => *self,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChordQuality {
    Major,
    Minor,
    Diminished,
    Augmented,
    Sus2,
    Sus4,
    Power,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Extension {
    Min7,
    Maj7,
    Dim7,
    Add9,
    Nine,
    Add11,
    Eleven,
    Six,
    FlatFive,
    SharpFive,
    FlatNine,
    SharpNine,
    SharpEleven,
    Thirteen,
}

impl Extension {
    fn intervals(self) -> &'static [u8] {
        match self {
            Extension::Min7 => &[10],
            Extension::Maj7 => &[11],
            Extension::Dim7 => &[9],
            Extension::Add9 => &[14],
            Extension::Nine => &[10, 14],
            Extension::Add11 => &[17],
            Extension::Eleven => &[10, 14, 17],
            Extension::Six => &[9],
            Extension::FlatFive => &[],
            Extension::SharpFive => &[],
            Extension::FlatNine => &[10, 13],
            Extension::SharpNine => &[10, 15],
            Extension::SharpEleven => &[18],
            Extension::Thirteen => &[10, 14, 21],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Chord {
    /// Root pitch class 0..=11.
    pub root: u8,
    pub quality: ChordQuality,
    #[serde(default)]
    pub extensions: Vec<Extension>,
    /// Bass pitch class for slash chords.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bass: Option<u8>,
}

impl Chord {
    pub fn new(root: u8, quality: ChordQuality) -> Self {
        Chord { root: root % 12, quality, extensions: Vec::new(), bass: None }
    }

    /// Intervals above the root in semitones (may exceed 12 for extensions), sorted, deduplicated.
    pub fn intervals(&self) -> Vec<u8> {
        let mut v: Vec<u8> = match self.quality {
            ChordQuality::Major => vec![0, 4, 7],
            ChordQuality::Minor => vec![0, 3, 7],
            ChordQuality::Diminished => vec![0, 3, 6],
            ChordQuality::Augmented => vec![0, 4, 8],
            ChordQuality::Sus2 => vec![0, 2, 7],
            ChordQuality::Sus4 => vec![0, 5, 7],
            ChordQuality::Power => vec![0, 7],
        };
        for e in &self.extensions {
            match e {
                Extension::FlatFive => {
                    v.retain(|&i| i != 7);
                    v.push(6);
                }
                Extension::SharpFive => {
                    v.retain(|&i| i != 7);
                    v.push(8);
                }
                _ => v.extend_from_slice(e.intervals()),
            }
        }
        v.sort_unstable();
        v.dedup();
        v
    }

    pub fn pitch_classes(&self) -> Vec<u8> {
        let mut v: Vec<u8> = self.intervals().iter().map(|i| (self.root + i) % 12).collect();
        v.dedup();
        v
    }

    pub fn contains(&self, pitch: u8) -> bool {
        self.pitch_classes().contains(&(pitch % 12))
    }

    pub fn bass_pc(&self) -> u8 {
        self.bass.unwrap_or(self.root)
    }

    pub fn third_pc(&self) -> Option<u8> {
        match self.quality {
            ChordQuality::Major | ChordQuality::Augmented => Some((self.root + 4) % 12),
            ChordQuality::Minor | ChordQuality::Diminished => Some((self.root + 3) % 12),
            _ => None,
        }
    }

    pub fn is_minor_like(&self) -> bool {
        matches!(self.quality, ChordQuality::Minor | ChordQuality::Diminished)
    }

    /// Chord tones in ascending order in the register [lo, hi] (inclusive).
    pub fn tones_in_range(&self, lo: u8, hi: u8) -> Vec<u8> {
        let pcs = self.pitch_classes();
        (lo..=hi).filter(|p| pcs.contains(&(p % 12))).collect()
    }

    pub fn symbol(&self) -> String {
        let mut s = NOTE_NAMES_SHARP[self.root as usize].to_string();
        s.push_str(match self.quality {
            ChordQuality::Major => "",
            ChordQuality::Minor => "m",
            ChordQuality::Diminished => "dim",
            ChordQuality::Augmented => "aug",
            ChordQuality::Sus2 => "sus2",
            ChordQuality::Sus4 => "sus4",
            ChordQuality::Power => "5",
        });
        for e in &self.extensions {
            s.push_str(match e {
                Extension::Min7 => "7",
                Extension::Maj7 => "maj7",
                Extension::Dim7 => "dim7",
                Extension::Add9 => "add9",
                Extension::Nine => "9",
                Extension::Add11 => "add11",
                Extension::Eleven => "11",
                Extension::Six => "6",
                Extension::FlatFive => "b5",
                Extension::SharpFive => "#5",
                Extension::FlatNine => "b9",
                Extension::SharpNine => "#9",
                Extension::SharpEleven => "#11",
                Extension::Thirteen => "13",
            });
        }
        if let Some(b) = self.bass {
            s.push('/');
            s.push_str(NOTE_NAMES_SHARP[b as usize]);
        }
        s
    }

    /// Parses a chord symbol such as "C", "Am", "F#m7b5", "Gsus4", "Bbmaj7", "Dm7/F", "E5", "Caug", "Cdim7".
    pub fn parse_symbol(s: &str) -> Option<Chord> {
        let s = s.trim();
        let (main, bass) = match s.split_once('/') {
            Some((m, b)) => (m, Some(b)),
            None => (s, None),
        };
        let (root, rest) = parse_pitch_class(main)?;
        let mut rest = rest.to_string();
        let mut chord = Chord::new(root, ChordQuality::Major);

        // Quality prefixes, longest first.
        let quals: [(&str, ChordQuality); 12] = [
            ("maj", ChordQuality::Major),
            ("min", ChordQuality::Minor),
            ("dim", ChordQuality::Diminished),
            ("aug", ChordQuality::Augmented),
            ("sus2", ChordQuality::Sus2),
            ("sus4", ChordQuality::Sus4),
            ("sus", ChordQuality::Sus4),
            ("m", ChordQuality::Minor),
            ("M", ChordQuality::Major),
            ("-", ChordQuality::Minor),
            ("°", ChordQuality::Diminished),
            ("+", ChordQuality::Augmented),
        ];
        // "maj7" must map to Major + Maj7, "m7" to Minor + Min7 — handle "maj" specially below.
        if rest.starts_with("maj") {
            chord.quality = ChordQuality::Major;
            rest = rest[3..].to_string();
            if rest.starts_with('7') {
                chord.extensions.push(Extension::Maj7);
                rest = rest[1..].to_string();
            } else if rest.starts_with('9') {
                chord.extensions.push(Extension::Maj7);
                chord.extensions.push(Extension::Add9);
                rest = rest[1..].to_string();
            }
        } else {
            for (tok, q) in quals.iter() {
                if rest.starts_with(tok) {
                    chord.quality = *q;
                    rest = rest[tok.len()..].to_string();
                    break;
                }
            }
        }

        // Extensions, greedy.
        let ext_toks: [(&str, &[Extension]); 14] = [
            ("dim7", &[Extension::Dim7]),
            ("add9", &[Extension::Add9]),
            ("add11", &[Extension::Add11]),
            ("#11", &[Extension::SharpEleven]),
            ("b9", &[Extension::FlatNine]),
            ("#9", &[Extension::SharpNine]),
            ("b5", &[Extension::FlatFive]),
            ("#5", &[Extension::SharpFive]),
            ("13", &[Extension::Thirteen]),
            ("11", &[Extension::Eleven]),
            ("9", &[Extension::Nine]),
            ("7", &[Extension::Min7]),
            ("6", &[Extension::Six]),
            ("5", &[]),
        ];
        let mut guard = 0;
        while !rest.is_empty() && guard < 8 {
            guard += 1;
            let mut matched = false;
            for (tok, exts) in ext_toks.iter() {
                if rest.starts_with(tok) {
                    if *tok == "5" && chord.quality == ChordQuality::Major && chord.extensions.is_empty() {
                        chord.quality = ChordQuality::Power;
                    } else if *tok == "7" && chord.quality == ChordQuality::Diminished {
                        chord.extensions.push(Extension::Dim7);
                    } else {
                        chord.extensions.extend_from_slice(exts);
                    }
                    rest = rest[tok.len()..].to_string();
                    matched = true;
                    break;
                }
            }
            if !matched {
                // Tolerate parentheses/spaces, reject anything else.
                let c = rest.chars().next().unwrap();
                if c == '(' || c == ')' || c == ' ' {
                    rest = rest[c.len_utf8()..].to_string();
                } else {
                    return None;
                }
            }
        }
        if let Some(b) = bass {
            chord.bass = Some(parse_pitch_class(b)?.0);
        }
        Some(chord)
    }

    /// Parses a roman numeral relative to a key: "I", "ii", "V7", "bVII", "iv", "vi7", "IVmaj7", "V/V" (secondary dominant), "ii°".
    pub fn parse_roman(s: &str, key: &Key) -> Option<Chord> {
        let s = s.trim();
        // Secondary dominant: X/Y = dominant of Y.
        if let Some((a, b)) = s.split_once('/') {
            if a.eq_ignore_ascii_case("V") || a.eq_ignore_ascii_case("V7") {
                let target = Chord::parse_roman(b, key)?;
                let mut c = Chord::new(target.root + 7, ChordQuality::Major);
                if a.ends_with('7') {
                    c.extensions.push(Extension::Min7);
                }
                return Some(c);
            }
        }
        let mut rest = s;
        let mut accidental: i32 = 0;
        while let Some(c) = rest.chars().next() {
            match c {
                'b' if rest.len() > 1 && rest[1..].starts_with(|x: char| x == 'I' || x == 'V' || x == 'i' || x == 'v') => {
                    accidental -= 1;
                    rest = &rest[1..];
                }
                '#' => {
                    accidental += 1;
                    rest = &rest[1..];
                }
                _ => break,
            }
        }
        let numerals: [(&str, usize); 7] = [("VII", 6), ("VI", 5), ("IV", 3), ("V", 4), ("III", 2), ("II", 1), ("I", 0)];
        let upper = rest.to_ascii_uppercase();
        let (tok, degree) = numerals.iter().find(|(t, _)| upper.starts_with(t))?;
        let is_upper = rest.starts_with(tok);
        let suffix = &rest[tok.len()..];

        // Root from the major-scale degree (roman numerals are conventionally relative to the major scale
        // when accidentals are written, otherwise relative to the key's own scale).
        let root = if accidental != 0 || key.scale.intervals().len() != 7 {
            let major = [0, 2, 4, 5, 7, 9, 11];
            (key.root as i32 + major[*degree] + accidental).rem_euclid(12) as u8
        } else {
            (key.root + key.scale.intervals()[*degree]) % 12
        };
        let mut chord = Chord::new(root, if is_upper { ChordQuality::Major } else { ChordQuality::Minor });
        let mut suffix = suffix.to_string();
        if suffix.starts_with('°') || suffix.starts_with("dim") || suffix.starts_with('o') {
            chord.quality = ChordQuality::Diminished;
            suffix = suffix.trim_start_matches('°').trim_start_matches("dim").trim_start_matches('o').to_string();
        } else if suffix.starts_with('+') || suffix.starts_with("aug") {
            chord.quality = ChordQuality::Augmented;
            suffix = suffix.trim_start_matches('+').trim_start_matches("aug").to_string();
        } else if suffix.starts_with("sus4") || suffix.starts_with("sus") {
            chord.quality = ChordQuality::Sus4;
            suffix = suffix.trim_start_matches("sus4").trim_start_matches("sus").to_string();
        } else if suffix.starts_with("sus2") {
            chord.quality = ChordQuality::Sus2;
            suffix = suffix[4..].to_string();
        }
        if suffix.starts_with("maj7") {
            chord.extensions.push(Extension::Maj7);
        } else if suffix.starts_with('7') {
            chord.extensions.push(if chord.quality == ChordQuality::Diminished { Extension::Dim7 } else { Extension::Min7 });
        } else if suffix.starts_with('9') {
            chord.extensions.push(Extension::Nine);
        } else if suffix.starts_with("add9") {
            chord.extensions.push(Extension::Add9);
        } else if suffix.starts_with('6') {
            chord.extensions.push(Extension::Six);
        }
        Some(chord)
    }

    /// Parses either a chord symbol or a roman numeral.
    pub fn parse(s: &str, key: &Key) -> Option<Chord> {
        let t = s.trim();
        let first = t.chars().next()?;
        let looks_roman = t.trim_start_matches(['b', '#']).starts_with(['I', 'V', 'i', 'v']);
        if looks_roman && (first == 'I' || first == 'V' || first == 'i' || first == 'v' || first == 'b' || first == '#') {
            if let Some(c) = Chord::parse_roman(t, key) {
                return Some(c);
            }
        }
        Chord::parse_symbol(t)
    }

    /// Roman numeral of this chord in a key, if the root is diatonic.
    pub fn roman(&self, key: &Key) -> String {
        let names = ["I", "II", "III", "IV", "V", "VI", "VII"];
        let base = match key.degree_of(self.root) {
            Some(d) if key.scale.intervals().len() == 7 => names[d].to_string(),
            _ => {
                let major = [0, 2, 4, 5, 7, 9, 11];
                let rel = (self.root as i32 - key.root as i32).rem_euclid(12);
                match major.iter().position(|&m| m == rel) {
                    Some(d) => names[d].to_string(),
                    None => {
                        let d = major.iter().position(|&m| m == rel + 1).unwrap_or(0);
                        format!("b{}", names[d])
                    }
                }
            }
        };
        let mut s = if self.is_minor_like() { base.to_ascii_lowercase() } else { base };
        match self.quality {
            ChordQuality::Diminished => s.push('°'),
            ChordQuality::Augmented => s.push('+'),
            ChordQuality::Sus2 => s.push_str("sus2"),
            ChordQuality::Sus4 => s.push_str("sus4"),
            _ => {}
        }
        for e in &self.extensions {
            s.push_str(match e {
                Extension::Min7 => "7",
                Extension::Maj7 => "maj7",
                Extension::Nine => "9",
                Extension::Add9 => "add9",
                Extension::Six => "6",
                _ => "",
            });
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pitches() {
        assert_eq!(parse_pitch("C4").unwrap().0, 60);
        assert_eq!(parse_pitch("A3").unwrap().0, 57);
        assert_eq!(parse_pitch("F#5").unwrap().0, 78);
        assert_eq!(parse_pitch("Bb2").unwrap().0, 46);
        assert_eq!(pitch_name(61), "C#4");
    }

    #[test]
    fn parses_chord_symbols() {
        assert_eq!(Chord::parse_symbol("Am7").unwrap().pitch_classes(), vec![9, 0, 4, 7]);
        assert_eq!(Chord::parse_symbol("Cmaj7").unwrap().pitch_classes(), vec![0, 4, 7, 11]);
        assert_eq!(Chord::parse_symbol("F#m7b5").unwrap().pitch_classes(), vec![6, 9, 0, 4]);
        assert_eq!(Chord::parse_symbol("G7/B").unwrap().bass, Some(11));
        assert_eq!(Chord::parse_symbol("E5").unwrap().quality, ChordQuality::Power);
        assert_eq!(Chord::parse_symbol("Dsus4").unwrap().pitch_classes(), vec![2, 7, 9]);
        assert!(Chord::parse_symbol("H").is_none());
    }

    #[test]
    fn parses_roman() {
        let c = Key::parse("C major").unwrap();
        assert_eq!(Chord::parse_roman("vi", &c).unwrap().symbol(), "Am");
        assert_eq!(Chord::parse_roman("V7", &c).unwrap().symbol(), "G7");
        assert_eq!(Chord::parse_roman("bVII", &c).unwrap().symbol(), "A#");
        assert_eq!(Chord::parse_roman("ii°", &c).unwrap().symbol(), "Ddim");
        assert_eq!(Chord::parse_roman("V/V", &c).unwrap().symbol(), "D");
        let am = Key::parse("A minor").unwrap();
        assert_eq!(Chord::parse_roman("iv", &am).unwrap().symbol(), "Dm");
        assert_eq!(Chord::parse_roman("VI", &am).unwrap().symbol(), "F");
        assert_eq!(Chord::parse_symbol("Am").unwrap().roman(&c), "vi");
    }

    #[test]
    fn key_ops() {
        let k = Key::parse("D major").unwrap();
        assert!(k.contains(66));
        assert!(!k.contains(65));
        assert_eq!(k.snap(65), 64);
        assert_eq!(k.step(62, 1), 64);
        assert_eq!(k.step(62, -1), 61);
        assert_eq!(k.step(62, 7), 74);
        assert_eq!(k.diatonic_chord(0, false).symbol(), "D");
        assert_eq!(k.diatonic_chord(1, true).symbol(), "Em7");
        assert_eq!(k.diatonic_chord(6, false).symbol(), "C#dim");
    }
}
