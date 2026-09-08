//! Session model: the single source of truth shared by the plugin, the CLI and the agent tools.
//! All times are in ticks at [`PPQ`] pulses per quarter note.
//!
//! A song is a list of sections (with a role and an energy) and a list of layers (tracks). Each
//! layer has a kind that selects its generator, a MIDI channel, and a clip per section. The
//! arrangement is the layer × section presence matrix (`Track::active`).

use crate::theory::{Chord, Key, ScaleKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Ticks per quarter note used everywhere inside the engine.
pub const PPQ: u32 = 960;

/// What a layer is for; selects the generator, register, humanization preset and colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TrackRole {
    Chords,
    Pad,
    Arpeggio,
    Pluck,
    Melody,
    CounterMelody,
    Harmony,
    Bass,
    Sub,
    Drums,
    Percussion,
}

impl TrackRole {
    /// The four default layers of a new session.
    pub const DEFAULT: [TrackRole; 4] = [TrackRole::Chords, TrackRole::Melody, TrackRole::Bass, TrackRole::Drums];
    pub const ALL: [TrackRole; 11] = [
        TrackRole::Chords,
        TrackRole::Pad,
        TrackRole::Arpeggio,
        TrackRole::Pluck,
        TrackRole::Melody,
        TrackRole::CounterMelody,
        TrackRole::Harmony,
        TrackRole::Bass,
        TrackRole::Sub,
        TrackRole::Drums,
        TrackRole::Percussion,
    ];

    /// Zero-based default MIDI channel for the first layer of this kind.
    pub fn default_channel(self) -> u8 {
        match self {
            TrackRole::Chords => 0,
            TrackRole::Melody => 1,
            TrackRole::Bass => 2,
            TrackRole::Pad => 3,
            TrackRole::Arpeggio => 4,
            TrackRole::Pluck => 5,
            TrackRole::CounterMelody => 6,
            TrackRole::Harmony => 7,
            TrackRole::Sub => 8,
            TrackRole::Drums => 9,
            TrackRole::Percussion => 10,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            TrackRole::Chords => "chords",
            TrackRole::Pad => "pad",
            TrackRole::Arpeggio => "arpeggio",
            TrackRole::Pluck => "pluck",
            TrackRole::Melody => "melody",
            TrackRole::CounterMelody => "counter_melody",
            TrackRole::Harmony => "harmony",
            TrackRole::Bass => "bass",
            TrackRole::Sub => "sub",
            TrackRole::Drums => "drums",
            TrackRole::Percussion => "percussion",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TrackRole::Chords => "Chords",
            TrackRole::Pad => "Pad",
            TrackRole::Arpeggio => "Arp",
            TrackRole::Pluck => "Pluck",
            TrackRole::Melody => "Melody",
            TrackRole::CounterMelody => "Counter",
            TrackRole::Harmony => "Harmony",
            TrackRole::Bass => "Bass",
            TrackRole::Sub => "Sub",
            TrackRole::Drums => "Drums",
            TrackRole::Percussion => "Perc",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            TrackRole::Chords => "comped/block chords, voice-led",
            TrackRole::Pad => "long held voicings, slow strums, wide spacing",
            TrackRole::Arpeggio => "arpeggiates the chords (up/down/up-down/random, 8ths-16ths, 1-2 octaves)",
            TrackRole::Pluck => "short off-beat chord stabs",
            TrackRole::Melody => "lead line, motif-developed",
            TrackRole::CounterMelody => "second line that answers the lead where it rests, contrary motion, lower register",
            TrackRole::Harmony => "harmonizes the lead a 3rd/6th below, diatonic",
            TrackRole::Bass => "bass line following the chord roots",
            TrackRole::Sub => "sub bass: root only, long notes, low octave",
            TrackRole::Drums => "drum kit groove with fills",
            TrackRole::Percussion => "shaker/conga/tambourine layers (Euclidean)",
        }
    }

    pub fn parse(s: &str) -> Option<TrackRole> {
        match s.trim().to_ascii_lowercase().replace('-', "_").replace(' ', "_").as_str() {
            "chords" | "chord" | "keys" | "piano" => Some(TrackRole::Chords),
            "pad" | "pads" | "strings" => Some(TrackRole::Pad),
            "arpeggio" | "arp" | "arpeggiator" | "arps" => Some(TrackRole::Arpeggio),
            "pluck" | "plucks" | "stab" | "stabs" => Some(TrackRole::Pluck),
            "melody" | "lead" | "vocal" | "topline" => Some(TrackRole::Melody),
            "counter_melody" | "counter" | "countermelody" | "counterpoint" => Some(TrackRole::CounterMelody),
            "harmony" | "harmonies" | "backing" => Some(TrackRole::Harmony),
            "bass" | "bassline" => Some(TrackRole::Bass),
            "sub" | "sub_bass" | "808" => Some(TrackRole::Sub),
            "drums" | "beat" | "kit" => Some(TrackRole::Drums),
            "percussion" | "perc" | "shaker" => Some(TrackRole::Percussion),
            _ => None,
        }
    }

    /// Reasonable pitch register for the kind (inclusive MIDI note numbers).
    pub fn register(self) -> (u8, u8) {
        match self {
            TrackRole::Chords => (48, 76),
            TrackRole::Pad => (48, 79),
            TrackRole::Arpeggio => (55, 88),
            TrackRole::Pluck => (55, 84),
            TrackRole::Melody => (60, 84),
            TrackRole::CounterMelody => (55, 79),
            TrackRole::Harmony => (55, 81),
            TrackRole::Bass => (28, 52),
            TrackRole::Sub => (24, 43),
            TrackRole::Drums => (35, 59),
            TrackRole::Percussion => (54, 82),
        }
    }

    pub fn is_pitched(self) -> bool {
        !matches!(self, TrackRole::Drums | TrackRole::Percussion)
    }

    /// Generation order: harmony sources first, then lines that depend on them.
    pub fn order(self) -> u8 {
        match self {
            TrackRole::Chords => 0,
            TrackRole::Melody => 1,
            TrackRole::Pad => 2,
            TrackRole::Arpeggio => 3,
            TrackRole::Pluck => 4,
            TrackRole::Bass => 5,
            TrackRole::Sub => 6,
            TrackRole::CounterMelody => 7,
            TrackRole::Harmony => 8,
            TrackRole::Drums => 9,
            TrackRole::Percussion => 10,
        }
    }

    /// The base kind whose humanization preset applies.
    pub fn humanize_base(self) -> TrackRole {
        match self {
            TrackRole::Pad | TrackRole::Arpeggio | TrackRole::Pluck => TrackRole::Chords,
            TrackRole::CounterMelody | TrackRole::Harmony => TrackRole::Melody,
            TrackRole::Sub => TrackRole::Bass,
            TrackRole::Percussion => TrackRole::Drums,
            r => r,
        }
    }
}

/// MIDI's own default, and what a synth assumes when nothing declares otherwise.
pub fn default_bend_range() -> u8 {
    2
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
    /// Glide into this note from `semitones` away over `ms` (an 808 slide, a guitar bend). Compiled
    /// into the clip's pitch-bend lane, which is what playback and export actually read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slide: Option<Slide>,
}

/// A pitch glide into a note.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Slide {
    /// Where the glide starts, relative to the note (negative = from below).
    pub semitones: f32,
    /// How long the glide takes.
    pub ms: u32,
}

impl Note {
    pub fn new(pitch: u8, start: u32, len: u32, vel: f32) -> Self {
        Note { pitch, start, len, vel, lyric: None, slide: None }
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

/// What a controller lane drives. Expression and sustain are how a real player shapes a held
/// sound; notes alone cannot say "swell here" or "hold the pedal through the change".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoTarget {
    /// CC11 — loudness of a sounding note (a string or pad swell).
    Expression,
    /// CC64 — sustain pedal, on above 0.5.
    Sustain,
    /// Pitch bend, -1..1 across the track's bend range.
    PitchBend,
    /// CC1 — modulation / vibrato depth.
    Modulation,
}

impl AutoTarget {
    /// Controller number, or None for pitch bend (its own MIDI message).
    pub fn cc(&self) -> Option<u8> {
        match self {
            AutoTarget::Expression => Some(11),
            AutoTarget::Sustain => Some(64),
            AutoTarget::Modulation => Some(1),
            AutoTarget::PitchBend => None,
        }
    }
    /// Value a lane rests at when nothing is written (also what a reset restores).
    pub fn neutral(&self) -> f32 {
        match self {
            AutoTarget::Expression => 1.0,
            AutoTarget::Sustain | AutoTarget::Modulation => 0.0,
            AutoTarget::PitchBend => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Curve {
    /// Ramp to the next point.
    Linear,
    /// Hold until the next point (pedals, switches).
    Step,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutoPoint {
    /// Ticks from the start of the section.
    pub tick: u32,
    /// 0..1, or -1..1 for pitch bend.
    pub value: f32,
    #[serde(default = "linear_curve")]
    pub curve: Curve,
}

fn linear_curve() -> Curve {
    Curve::Linear
}

/// One controller lane over a clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Automation {
    pub target: AutoTarget,
    pub points: Vec<AutoPoint>,
}

impl Automation {
    pub fn new(target: AutoTarget, points: Vec<AutoPoint>) -> Self {
        let mut a = Automation { target, points };
        a.points.sort_by_key(|p| p.tick);
        a
    }

    /// Value at `tick`, interpolating between points.
    pub fn value_at(&self, tick: u32) -> f32 {
        if self.points.is_empty() {
            return self.target.neutral();
        }
        match self.points.binary_search_by_key(&tick, |p| p.tick) {
            Ok(i) => self.points[i].value,
            Err(0) => self.points[0].value,
            Err(i) if i >= self.points.len() => self.points[self.points.len() - 1].value,
            Err(i) => {
                let (a, b) = (self.points[i - 1], self.points[i]);
                match a.curve {
                    Curve::Step => a.value,
                    Curve::Linear => {
                        let span = (b.tick - a.tick).max(1) as f32;
                        a.value + (b.value - a.value) * ((tick - a.tick) as f32 / span)
                    }
                }
            }
        }
    }

    pub fn end(&self) -> u32 {
        self.points.last().map(|p| p.tick).unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub notes: Vec<Note>,
    pub source: ClipSource,
    /// Controller lanes (expression, sustain, bend) for this clip.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automation: Vec<Automation>,
}

impl Clip {
    pub fn new(notes: Vec<Note>, source: ClipSource) -> Self {
        let mut c = Clip { notes, source, automation: Vec::new() };
        c.sort();
        c.compile();
        c
    }

    pub fn with_automation(notes: Vec<Note>, source: ClipSource, automation: Vec<Automation>) -> Self {
        let mut c = Clip { notes, source, automation };
        c.sort();
        c.compile();
        c
    }

    /// Expands per-note sugar (currently [`Slide`]) into the lanes, so everything downstream reads
    /// exactly one representation of expression.
    pub fn compile(&mut self) {
        let slides: Vec<(u32, f32, u32)> = self.notes.iter().filter_map(|n| n.slide.map(|s| (n.start, s.semitones, s.ms))).collect();
        if slides.is_empty() {
            return;
        }
        let lane = match self.automation.iter().position(|a| a.target == AutoTarget::PitchBend) {
            Some(i) => &mut self.automation[i],
            None => {
                self.automation.push(Automation::new(AutoTarget::PitchBend, Vec::new()));
                self.automation.last_mut().unwrap()
            }
        };
        // Ticks per ms is tempo-dependent; slides are short, so assume the common 120 BPM here and
        // let the caller re-compile if it cares. PPQ/2 per beat at 120 BPM = 4 ticks per ms.
        for (start, semis, ms) in slides {
            let ticks = (ms * PPQ / 500).max(PPQ / 32);
            let from = start.saturating_sub(ticks);
            lane.points.push(AutoPoint { tick: from, value: (semis / 12.0).clamp(-1.0, 1.0), curve: Curve::Linear });
            lane.points.push(AutoPoint { tick: start, value: 0.0, curve: Curve::Linear });
        }
        lane.points.sort_by_key(|p| p.tick);
        lane.points.dedup_by_key(|p| p.tick);
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

/// Structural role of a section; generators use it together with `energy` and the section's
/// position in the song.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionRole {
    Intro,
    Verse,
    PreChorus,
    Chorus,
    Bridge,
    Break,
    Build,
    Drop,
    Outro,
    Other,
}

impl SectionRole {
    pub fn from_name(name: &str) -> SectionRole {
        let n = name.to_ascii_lowercase();
        if n.contains("intro") {
            SectionRole::Intro
        } else if n.contains("pre") {
            SectionRole::PreChorus
        } else if n.contains("chorus") || n.contains("hook") || n.contains("refrain") {
            SectionRole::Chorus
        } else if n.contains("drop") {
            SectionRole::Drop
        } else if n.contains("build") || n.contains("rise") {
            SectionRole::Build
        } else if n.contains("break") {
            SectionRole::Break
        } else if n.contains("bridge") || n.contains("middle") {
            SectionRole::Bridge
        } else if n.contains("outro") || n.contains("end") {
            SectionRole::Outro
        } else if n.contains("verse") || n.starts_with('a') || n.starts_with('b') {
            SectionRole::Verse
        } else {
            SectionRole::Other
        }
    }

    pub fn default_energy(self) -> f32 {
        match self {
            SectionRole::Intro => 0.3,
            SectionRole::Verse => 0.5,
            SectionRole::PreChorus => 0.65,
            SectionRole::Chorus => 0.85,
            SectionRole::Bridge => 0.55,
            SectionRole::Break => 0.3,
            SectionRole::Build => 0.7,
            SectionRole::Drop => 0.95,
            SectionRole::Outro => 0.3,
            SectionRole::Other => 0.6,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            SectionRole::Intro => "intro",
            SectionRole::Verse => "verse",
            SectionRole::PreChorus => "pre_chorus",
            SectionRole::Chorus => "chorus",
            SectionRole::Bridge => "bridge",
            SectionRole::Break => "break",
            SectionRole::Build => "build",
            SectionRole::Drop => "drop",
            SectionRole::Outro => "outro",
            SectionRole::Other => "section",
        }
    }

    pub fn parse(s: &str) -> Option<SectionRole> {
        Some(match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "intro" => SectionRole::Intro,
            "verse" => SectionRole::Verse,
            "pre_chorus" | "prechorus" | "pre" => SectionRole::PreChorus,
            "chorus" | "hook" => SectionRole::Chorus,
            "bridge" => SectionRole::Bridge,
            "break" | "breakdown" => SectionRole::Break,
            "build" | "buildup" | "riser" => SectionRole::Build,
            "drop" => SectionRole::Drop,
            "outro" => SectionRole::Outro,
            "other" | "section" => SectionRole::Other,
            _ => return None,
        })
    }

    /// Which default layers play in a section of this role (arrangement defaults).
    pub fn default_layers_active(self, kind: TrackRole) -> bool {
        use SectionRole::*;
        use TrackRole::*;
        match self {
            Intro => matches!(kind, Chords | Pad | Arpeggio | Percussion | Melody),
            Break => matches!(kind, Chords | Pad | Arpeggio | Melody | Sub),
            Build => !matches!(kind, Melody | Harmony | Sub),
            Outro => matches!(kind, Chords | Pad | Melody | Arpeggio),
            _ => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub id: String,
    pub name: String,
    pub bars: u32,
    /// 0.0 (sparse, quiet) ..= 1.0 (dense, loud).
    pub energy: f32,
    #[serde(default = "default_section_role")]
    pub role: SectionRole,
    #[serde(default)]
    pub chords: Vec<ChordEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyrics: Option<String>,
}

fn default_section_role() -> SectionRole {
    SectionRole::Other
}

impl Section {
    pub fn new(id: impl Into<String>, name: impl Into<String>, bars: u32, energy: f32) -> Self {
        let name = name.into();
        let role = SectionRole::from_name(&name);
        Section { id: id.into(), name, bars, energy, role, chords: Vec::new(), lyrics: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// Stable id used by tools and the UI (e.g. "melody", "arp2").
    pub id: String,
    pub name: String,
    #[serde(alias = "role")]
    pub kind: TrackRole,
    /// Zero-based MIDI channel this layer is sent on.
    pub channel: u8,
    /// Clips keyed by section id.
    #[serde(default)]
    pub clips: BTreeMap<String, Clip>,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub muted: bool,
    /// Sections in which this layer is silent (arrangement). Absent = plays.
    #[serde(default)]
    pub inactive: BTreeSet<String>,
    /// General MIDI program for the built-in synth (128 = drum kit). None = default for the kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument: Option<u8>,
    /// Groove template for this layer ("none" to play straight). None = the song's, else the style's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groove: Option<String>,
    /// Pitch-bend range in semitones, declared to the synth and to exported MIDI. 808 slides want 12.
    #[serde(default = "default_bend_range")]
    pub bend_range: u8,
}

impl Track {
    pub fn new(id: impl Into<String>, name: impl Into<String>, kind: TrackRole, channel: u8) -> Self {
        Track { id: id.into(), name: name.into(), kind, channel, clips: BTreeMap::new(), locked: false, muted: false, inactive: BTreeSet::new(), instrument: None, groove: None, bend_range: default_bend_range() }
    }
    pub fn active_in(&self, section_id: &str) -> bool {
        !self.inactive.contains(section_id)
    }
    /// Effective General MIDI program (explicit or the kind's default for the style).
    pub fn program(&self, style: &str) -> u8 {
        self.instrument.unwrap_or_else(|| crate::gm::default_program(self.kind, style))
    }
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
    /// Groove template for the whole song ("none" to play straight). None = pick from the style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groove: Option<String>,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            key: Key { root: 0, scale: ScaleKind::Major },
            tempo: 100.0,
            time_sig: TimeSig::default(),
            sections: Vec::new(),
            tracks: TrackRole::DEFAULT.iter().map(|&k| Track::new(k.name(), k.label(), k, k.default_channel())).collect(),
            style: "pop".into(),
            seed: 1,
            groove: None,
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

    /// Finds a layer by id, name, or kind name (first layer of that kind).
    pub fn track_by(&self, key: &str) -> Option<&Track> {
        let k = key.trim();
        self.tracks
            .iter()
            .find(|t| t.id.eq_ignore_ascii_case(k) || t.name.eq_ignore_ascii_case(k))
            .or_else(|| TrackRole::parse(k).and_then(|kind| self.tracks.iter().find(|t| t.kind == kind)))
    }

    pub fn track_by_mut(&mut self, key: &str) -> Option<&mut Track> {
        let idx = self.track_index(key)?;
        self.tracks.get_mut(idx)
    }

    pub fn track_index(&self, key: &str) -> Option<usize> {
        let k = key.trim();
        self.tracks
            .iter()
            .position(|t| t.id.eq_ignore_ascii_case(k) || t.name.eq_ignore_ascii_case(k))
            .or_else(|| TrackRole::parse(k).and_then(|kind| self.tracks.iter().position(|t| t.kind == kind)))
    }

    /// First layer of a kind (convenience for generators that read the lead or the chords).
    pub fn track_of_kind(&self, kind: TrackRole) -> Option<&Track> {
        self.tracks.iter().find(|t| t.kind == kind)
    }

    /// Notes of the first layer of a kind in a section (empty if none / inactive there).
    pub fn notes_of_kind(&self, kind: TrackRole, section_id: &str) -> Vec<Note> {
        self.track_of_kind(kind).and_then(|t| if t.active_in(section_id) { t.clips.get(section_id).map(|c| c.notes.clone()) } else { None }).unwrap_or_default()
    }

    pub fn clip(&self, track: &str, section: &str) -> Option<&Clip> {
        let sec = self.section(section)?;
        self.track_by(track)?.clips.get(&sec.id)
    }

    pub fn set_clip(&mut self, track: &str, section: &str, clip: Clip) -> crate::Result<()> {
        let id = self.section(section).ok_or_else(|| crate::Error::UnknownSection(section.into()))?.id.clone();
        let t = self.track_by_mut(track).ok_or_else(|| crate::Error::UnknownTrack(track.into()))?;
        t.clips.insert(id, clip);
        Ok(())
    }

    /// Adds a layer of a kind with a unique id and a free MIDI channel; returns the id.
    pub fn add_track(&mut self, kind: TrackRole, name: Option<&str>) -> String {
        let base = kind.name().to_string();
        let mut id = base.clone();
        let mut n = 2;
        while self.tracks.iter().any(|t| t.id == id) {
            id = format!("{base}{n}");
            n += 1;
        }
        let name = name.map(str::to_string).unwrap_or_else(|| if n > 2 { format!("{} {}", kind.label(), n - 1) } else { kind.label().to_string() });
        let used: BTreeSet<u8> = self.tracks.iter().map(|t| t.channel).collect();
        let mut channel = kind.default_channel();
        if used.contains(&channel) {
            channel = (0..16u8).find(|c| !used.contains(c) && (*c != 9 || !kind.is_pitched())).unwrap_or(channel);
        }
        let mut track = Track::new(id.clone(), name, kind, channel);
        for s in &self.sections {
            if !s.role.default_layers_active(kind) {
                track.inactive.insert(s.id.clone());
            }
        }
        self.tracks.push(track);
        id
    }

    pub fn remove_track(&mut self, key: &str) -> bool {
        match self.track_index(key) {
            Some(i) => {
                self.tracks.remove(i);
                true
            }
            None => false,
        }
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
        let sec = Section::new(id.clone(), name, bars, energy);
        let role = sec.role;
        for t in self.tracks.iter_mut() {
            if !role.default_layers_active(t.kind) {
                t.inactive.insert(id.clone());
            }
        }
        self.sections.push(sec);
        id
    }

    /// Flattens a layer into song-absolute notes (section clips offset by section start; inactive
    /// sections skipped).
    /// Every controller lane of a layer laid out along the song, with a reset at each section
    /// boundary so a pedal held at the end of a verse cannot smear into the chorus.
    pub fn flatten_automation(&self, track: &str) -> Vec<(u32, AutoTarget, f32)> {
        let bar = self.bar_ticks();
        let mut out: Vec<(u32, AutoTarget, f32)> = Vec::new();
        let Some(track) = self.track_by(track) else { return out };
        let step = PPQ / 16;
        let mut offset = 0;
        let mut touched: Vec<AutoTarget> = Vec::new();
        for s in &self.sections {
            let len = s.bars * bar;
            let clip = if track.active_in(&s.id) { track.clips.get(&s.id) } else { None };
            for target in [AutoTarget::Expression, AutoTarget::Sustain, AutoTarget::PitchBend, AutoTarget::Modulation] {
                let lane = clip.and_then(|c| c.automation.iter().find(|a| a.target == target));
                match lane {
                    Some(lane) if !lane.points.is_empty() => {
                        if !touched.contains(&target) {
                            touched.push(target);
                        }
                        let mut t = 0;
                        let mut last = f32::NAN;
                        while t < len {
                            let v = lane.value_at(t);
                            // Only emit where the lane actually moves; a flat lane costs one event.
                            if !(v - last).abs().lt(&0.004) {
                                out.push((offset + t, target, v));
                                last = v;
                            }
                            t += step;
                        }
                        // Leave the lane where it belongs at the boundary.
                        out.push((offset + len.saturating_sub(1), target, target.neutral()));
                    }
                    _ => {
                        if touched.contains(&target) {
                            out.push((offset, target, target.neutral()));
                        }
                    }
                }
            }
            offset += len;
        }
        out.sort_by_key(|(t, _, _)| *t);
        out
    }

    pub fn flatten(&self, track: &str) -> Vec<Note> {
        let bar = self.bar_ticks();
        let mut out = Vec::new();
        let Some(track) = self.track_by(track) else { return out };
        let mut offset = 0;
        for s in &self.sections {
            let len = s.bars * bar;
            if track.active_in(&s.id) {
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
            }
            offset += len;
        }
        out
    }

    /// Chord sounding at `tick` inside a section (falls back to the last chord before the tick).
    pub fn chord_at(section: &Section, tick: u32) -> Option<&ChordEvent> {
        section.chords.iter().filter(|c| c.start <= tick).last().or_else(|| section.chords.first())
    }

    /// Position hints for a section: (index, count, is_last_of_its_role, occurrences_of_role_before).
    pub fn section_position(&self, id: &str) -> (usize, usize, bool, usize) {
        let idx = self.section_index(id).unwrap_or(0);
        let role = self.sections.get(idx).map(|s| s.role).unwrap_or(SectionRole::Other);
        let before = self.sections[..idx].iter().filter(|s| s.role == role).count();
        let after = self.sections[idx + 1..].iter().filter(|s| s.role == role).count();
        (idx, self.sections.len(), after == 0, before)
    }
}
