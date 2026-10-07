//! General MIDI program names and per-layer defaults for the built-in soundfont synth.

use crate::model::TrackRole;

/// Special program value meaning "drum kit" (percussion channel).
pub const DRUM_KIT: u8 = 128;

pub const GM_PROGRAMS: [&str; 128] = [
    "Acoustic Grand Piano", "Bright Acoustic Piano", "Electric Grand Piano", "Honky-tonk Piano", "Electric Piano 1", "Electric Piano 2", "Harpsichord", "Clavinet",
    "Celesta", "Glockenspiel", "Music Box", "Vibraphone", "Marimba", "Xylophone", "Tubular Bells", "Dulcimer",
    "Drawbar Organ", "Percussive Organ", "Rock Organ", "Church Organ", "Reed Organ", "Accordion", "Harmonica", "Tango Accordion",
    "Acoustic Guitar (nylon)", "Acoustic Guitar (steel)", "Electric Guitar (jazz)", "Electric Guitar (clean)", "Electric Guitar (muted)", "Overdriven Guitar", "Distortion Guitar", "Guitar Harmonics",
    "Acoustic Bass", "Electric Bass (finger)", "Electric Bass (pick)", "Fretless Bass", "Slap Bass 1", "Slap Bass 2", "Synth Bass 1", "Synth Bass 2",
    "Violin", "Viola", "Cello", "Contrabass", "Tremolo Strings", "Pizzicato Strings", "Orchestral Harp", "Timpani",
    "String Ensemble 1", "String Ensemble 2", "Synth Strings 1", "Synth Strings 2", "Choir Aahs", "Voice Oohs", "Synth Voice", "Orchestra Hit",
    "Trumpet", "Trombone", "Tuba", "Muted Trumpet", "French Horn", "Brass Section", "Synth Brass 1", "Synth Brass 2",
    "Soprano Sax", "Alto Sax", "Tenor Sax", "Baritone Sax", "Oboe", "English Horn", "Bassoon", "Clarinet",
    "Piccolo", "Flute", "Recorder", "Pan Flute", "Blown Bottle", "Shakuhachi", "Whistle", "Ocarina",
    "Lead 1 (square)", "Lead 2 (sawtooth)", "Lead 3 (calliope)", "Lead 4 (chiff)", "Lead 5 (charang)", "Lead 6 (voice)", "Lead 7 (fifths)", "Lead 8 (bass + lead)",
    "Pad 1 (new age)", "Pad 2 (warm)", "Pad 3 (polysynth)", "Pad 4 (choir)", "Pad 5 (bowed)", "Pad 6 (metallic)", "Pad 7 (halo)", "Pad 8 (sweep)",
    "FX 1 (rain)", "FX 2 (soundtrack)", "FX 3 (crystal)", "FX 4 (atmosphere)", "FX 5 (brightness)", "FX 6 (goblins)", "FX 7 (echoes)", "FX 8 (sci-fi)",
    "Sitar", "Banjo", "Shamisen", "Koto", "Kalimba", "Bag pipe", "Fiddle", "Shanai",
    "Tinkle Bell", "Agogo", "Steel Drums", "Woodblock", "Taiko Drum", "Melodic Tom", "Synth Drum", "Reverse Cymbal",
    "Guitar Fret Noise", "Breath Noise", "Seashore", "Bird Tweet", "Telephone Ring", "Helicopter", "Applause", "Gunshot",
];

pub fn program_name(program: u8) -> &'static str {
    if program >= DRUM_KIT {
        "Drum Kit"
    } else {
        GM_PROGRAMS[program as usize]
    }
}

/// Default General MIDI program for a layer kind (drums/percussion use the drum kit).
pub fn default_program(kind: TrackRole, style: &str) -> u8 {
    let s = style.to_ascii_lowercase();
    let kids = s.contains("kid") || s.contains("nursery") || s.contains("rhyme");
    let edm = s.contains("edm") || s.contains("house") || s.contains("techno") || s.contains("trance") || s.contains("trap");
    let cine = s.contains("cinema") || s.contains("orchestra") || s.contains("epic");
    match kind {
        TrackRole::Chords => if edm { 90 } else if cine { 48 } else { 4 },
        TrackRole::Pad => if cine { 49 } else { 89 },
        TrackRole::Arpeggio => if edm { 81 } else if kids { 10 } else { 11 },
        TrackRole::Pluck => if edm { 46 } else { 25 },
        TrackRole::Melody => if kids { 10 } else if edm { 80 } else if cine { 73 } else { 0 },
        TrackRole::CounterMelody => if kids { 11 } else { 71 },
        TrackRole::Harmony => 52,
        TrackRole::Bass => if edm { 38 } else { 33 },
        TrackRole::Sub => 38,
        TrackRole::Drums | TrackRole::Percussion => DRUM_KIT,
    }
}

/// Parses a program by number or by (partial, case-insensitive) name.
pub fn parse_program(s: &str) -> Option<u8> {
    let t = s.trim();
    if let Ok(n) = t.parse::<u32>() {
        return if n <= 128 { Some(n as u8) } else { None };
    }
    let l = t.to_ascii_lowercase();
    if l.contains("drum") || l.contains("kit") || l.contains("perc") {
        return Some(DRUM_KIT);
    }
    GM_PROGRAMS.iter().position(|n| n.to_ascii_lowercase() == l).or_else(|| GM_PROGRAMS.iter().position(|n| n.to_ascii_lowercase().contains(&l))).map(|i| i as u8)
}
