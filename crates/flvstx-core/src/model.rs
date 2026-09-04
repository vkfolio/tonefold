//! Session model: the single source of truth shared by the plugin, the CLI and the agent tools.
//! All times are in ticks at [`PPQ`] pulses per quarter note.

use crate::theory::{Chord, Key, ScaleKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Ticks per quarter note used everywhere inside the engine.
pub const PPQ: u32 = 960;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackRole {
    Chords,
    Melody,
    Bass,
    Drums,
}

impl TrackRole {
    pub const ALL: [TrackRole; 4] = [TrackRole::Chords, TrackRole::Melody, TrackRole::Bass, TrackRole::Drums];

    /// Zero-based MIDI channel.
    pub fn midi_channel(self) -> u8 {
        match self {
            TrackRole::Chords => 0,
            TrackRole::Melody => 1,
            TrackRole::Bass => 2,
            TrackRole::Drums => 9,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            TrackRole::Chords => "chords",
            TrackRole::Melody => "melody",
            TrackRole::Bass => "bass",
            TrackRole::Drums => "drums",
        }
    }

    pub fn parse(s: &str) -> Option<TrackRole> {
        match s.trim().to_ascii_lowercase().as_str() {
            "chords" | "chord" | "harmony" | "keys" => Some(TrackRole::Chords),
            "melody" | "lead" | "vocal" | "topline" => Some(TrackRole::Melody),
            "bass" | "bassline" => Some(TrackRole::Bass),
            "drums" | "beat" | "percussion" => Some(TrackRole::Drums),
            _ => None,
        }
    }

    /// Reasonable pitch register for the role (inclusive MIDI note numbers).
    pub fn register(self) -> (u8, u8) {
        match self {
            TrackRole::Chords => (48, 76),
            TrackRole::Melody => (60, 84),
            TrackRole::Bass => (28, 52),
            TrackRole::Drums => (35, 59),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub pitch: u8,
    /// Start in ticks relative to the start of the section.
    pub start: u32,
    /// Length in ticks.
    pub len: u32,
    /// Velocity 0.0..=1.0.
    pub vel: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyric: Option<String>,
}

impl Note {
    pub fn new(pitch: u8, start: u32, len: u32, vel: f32) -> Self {
        Note { pitch, start, len, vel, lyric: None }
    }
    pub fn end(&self) -> u32 {
        self.start + self.len
    }
    pub fn vel_midi(&self) -> u8 {
        (self.vel.clamp(0.0, 1.0) * 127.0).round() as u8
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ClipSource {
    /// Produced by a rule generator with these parameters.
    Generated { seed: u64, params: serde_json::Value },
    /// Written from compact notation by the agent.
    Agent,
    /// Edited by hand in the piano roll.
    Edited,
    /// Imported from a MIDI file.
    Imported,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub notes: Vec<Note>,
    pub source: ClipSource,
}

impl Clip {
    pub fn new(notes: Vec<Note>, source: ClipSource) -> Self {
        let mut c = Clip { notes, source };
        c.sort();
        c
    }
    pub fn sort(&mut self) {
        self.notes.sort_by(|a, b| a.start.cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    }
}

/// A chord placed on the section timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChordEvent {
    pub start: u32,
    pub len: u32,
    pub chord: Chord,
    /// Symbol as written by the user/agent, e.g. "Am7" or "vi".
    pub symbol: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub id: String,
    pub name: String,
    pub bars: u32,
    /// 0.0 (sparse, quiet) ..= 1.0 (dense, loud).
    pub energy: f32,
    #[serde(default)]
    pub chords: Vec<ChordEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyrics: Option<String>,
}

impl Section {
    pub fn new(id: impl Into<String>, name: impl Into<String>, bars: u32, energy: f32) -> Self {
        Section { id: id.into(), name: name.into(), bars, energy, chords: Vec::new(), lyrics: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub role: TrackRole,
    /// Clips keyed by section id.
    #[serde(default)]
    pub clips: BTreeMap<String, Clip>,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub muted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSig {
    pub num: u32,
    pub den: u32,
}

impl Default for TimeSig {
    fn default() -> Self {
        TimeSig { num: 4, den: 4 }
    }
}

impl TimeSig {
    pub fn bar_ticks(&self) -> u32 {
        PPQ * 4 * self.num / self.den
    }
    pub fn beat_ticks(&self) -> u32 {
        PPQ * 4 / self.den
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub key: Key,
    pub tempo: f32,
    pub time_sig: TimeSig,
    pub sections: Vec<Section>,
    pub tracks: Vec<Track>,
    /// Free-text style/genre hint used by generators ("lofi", "kids", "cinematic", ...).
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub seed: u64,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            key: Key { root: 0, scale: ScaleKind::Major },
            tempo: 100.0,
            time_sig: TimeSig::default(),
            sections: Vec::new(),
            tracks: TrackRole::ALL.iter().map(|&role| Track { role, clips: BTreeMap::new(), locked: false, muted: false }).collect(),
            style: "pop".into(),
            seed: 1,
        }
    }
}

impl Session {
    pub fn bar_ticks(&self) -> u32 {
        self.time_sig.bar_ticks()
    }

    pub fn section(&self, id: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.id.eq_ignore_ascii_case(id) || s.name.eq_ignore_ascii_case(id))
    }

    pub fn section_mut(&mut self, id: &str) -> Option<&mut Section> {
        self.sections.iter_mut().find(|s| s.id.eq_ignore_ascii_case(id) || s.name.eq_ignore_ascii_case(id))
    }

    pub fn section_index(&self, id: &str) -> Option<usize> {
        self.sections.iter().position(|s| s.id.eq_ignore_ascii_case(id) || s.name.eq_ignore_ascii_case(id))
    }

    /// Absolute start tick of a section in the song.
    pub fn section_start(&self, id: &str) -> Option<u32> {
        let idx = self.section_index(id)?;
        let bar = self.bar_ticks();
        Some(self.sections[..idx].iter().map(|s| s.bars * bar).sum())
    }

    pub fn total_ticks(&self) -> u32 {
        let bar = self.bar_ticks();
        self.sections.iter().map(|s| s.bars * bar).sum()
    }

    pub fn track(&self, role: TrackRole) -> &Track {
        self.tracks.iter().find(|t| t.role == role).expect("all roles present")
    }

    pub fn track_mut(&mut self, role: TrackRole) -> &mut Track {
        self.tracks.iter_mut().find(|t| t.role == role).expect("all roles present")
    }

    pub fn clip(&self, role: TrackRole, section: &str) -> Option<&Clip> {
        let sec = self.section(section)?;
        self.track(role).clips.get(&sec.id)
    }

    pub fn set_clip(&mut self, role: TrackRole, section: &str, clip: Clip) -> crate::Result<()> {
        let id = self.section(section).ok_or_else(|| crate::Error::UnknownSection(section.into()))?.id.clone();
        self.track_mut(role).clips.insert(id, clip);
        Ok(())
    }

    /// Adds a section, generating a unique id from the name.
    pub fn add_section(&mut self, name: &str, bars: u32, energy: f32) -> String {
        let base: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        let base = if base.is_empty() { "section".to_string() } else { base };
        let mut id = base.clone();
        let mut n = 2;
        while self.sections.iter().any(|s| s.id == id) {
            id = format!("{base}{n}");
            n += 1;
        }
        self.sections.push(Section::new(id.clone(), name, bars, energy));
        id
    }

    /// Flattens a track into song-absolute notes (section clips offset by section start).
    pub fn flatten(&self, role: TrackRole) -> Vec<Note> {
        let bar = self.bar_ticks();
        let mut out = Vec::new();
        let mut offset = 0;
        let track = self.track(role);
        for s in &self.sections {
            let len = s.bars * bar;
            if let Some(clip) = track.clips.get(&s.id) {
                for n in &clip.notes {
                    if n.start < len {
                        let mut m = n.clone();
                        m.start += offset;
                        m.len = m.len.min(len.saturating_sub(n.start).max(1));
                        out.push(m);
                    }
                }
            }
            offset += len;
        }
        out
    }

    /// Chord sounding at `tick` inside a section (falls back to the last chord before the tick).
    pub fn chord_at(section: &Section, tick: u32) -> Option<&ChordEvent> {
        section.chords.iter().filter(|c| c.start <= tick).last().or_else(|| section.chords.first())
    }
}
