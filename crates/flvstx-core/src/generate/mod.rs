//! Rule-based generators. Each takes a [`Session`] + section and returns notes relative to the section start.
//! The agent normally decides *what* (form, harmony, motif, style) and calls these to realise *how*.
//! Generators read the section's role/energy and its position in the song so verses, choruses,
//! builds and outros come out different, and derive from the lead/chords that already exist.

pub mod bass;
pub mod chords;
pub mod drums;
pub mod harmonize;
pub mod kids;
pub mod layers;
pub mod melody;

use crate::humanize::{humanize, HumanizeParams};
use crate::model::{Clip, ClipSource, Section, SectionRole, Session, TrackRole};
use crate::{Error, Result};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

/// Common generation parameters accepted by every generator (unknown fields ignored per generator).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GenParams {
    /// Style hint; falls back to the session style.
    #[serde(default)]
    pub style: Option<String>,
    /// Energy override 0..1; falls back to the section energy.
    #[serde(default)]
    pub energy: Option<f32>,
    #[serde(default)]
    pub seed: Option<u64>,
    /// Melody: motif in melodic notation to develop (else one is invented, or reused from another section).
    #[serde(default)]
    pub motif: Option<String>,
    /// Melody: contour hint "arch", "rise", "fall", "wave".
    #[serde(default)]
    pub contour: Option<String>,
    /// Melody/kids: lyrics to set (one syllable per note).
    #[serde(default)]
    pub lyrics: Option<String>,
    /// Bass: pattern ("root", "root5", "octave", "walking", "pedal", "push", "808", "pulse").
    /// Arpeggio: "up", "down", "updown", "random", "chord". Percussion: "shaker", "conga", "tambourine", "mixed".
    /// Harmony: "third", "sixth", "above".
    #[serde(default)]
    pub pattern: Option<String>,
    /// Arpeggio rate: "8", "16", "8t", "16t" (default by energy).
    #[serde(default)]
    pub rate: Option<String>,
    /// Arpeggio octaves (1..3).
    #[serde(default)]
    pub octaves: Option<u32>,
    /// Drums: fill on the last bar of each 4-bar phrase.
    #[serde(default)]
    pub fills: Option<bool>,
    /// Apply humanization after generating (default true).
    #[serde(default)]
    pub humanize: Option<bool>,
    /// Note density 0..1 (melody/bass/drums/percussion), overrides energy-derived default.
    #[serde(default)]
    pub density: Option<f32>,
    /// Melody: reuse the motif of this section (default: the first section with a melody).
    #[serde(default)]
    pub from_section: Option<String>,
}

impl GenParams {
    pub fn rng(&self, session: &Session, salt: u64) -> ChaCha8Rng {
        ChaCha8Rng::seed_from_u64(self.seed.unwrap_or(session.seed).wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(salt))
    }
    pub fn style<'a>(&'a self, session: &'a Session) -> &'a str {
        self.style.as_deref().unwrap_or(&session.style)
    }
    pub fn energy(&self, section: &Section) -> f32 {
        self.energy.unwrap_or(section.energy).clamp(0.0, 1.0)
    }
}

/// Song-position context handed to generators so sections differ meaningfully.
#[derive(Debug, Clone, Copy)]
pub struct SongCtx {
    pub role: SectionRole,
    /// 0-based index of the section among sections of the same role (second chorus = 1).
    pub occurrence: usize,
    /// True for the last section of its role (final chorus is biggest).
    pub last_of_role: bool,
    pub is_first_section: bool,
    pub is_last_section: bool,
    /// Role of the following section (builds lead into drops/choruses).
    pub next_role: Option<SectionRole>,
}

impl SongCtx {
    pub fn of(session: &Session, section_id: &str) -> SongCtx {
        let (idx, count, last_of_role, before) = session.section_position(section_id);
        let role = session.sections.get(idx).map(|s| s.role).unwrap_or(SectionRole::Other);
        SongCtx { role, occurrence: before, last_of_role, is_first_section: idx == 0, is_last_section: idx + 1 >= count, next_role: session.sections.get(idx + 1).map(|s| s.role) }
    }

    /// Energy adjusted by song position: final chorus/drop a little bigger, first intro smaller.
    pub fn effective_energy(&self, base: f32) -> f32 {
        let mut e = base;
        if matches!(self.role, SectionRole::Chorus | SectionRole::Drop) && self.last_of_role && self.occurrence > 0 {
            e += 0.1;
        }
        if matches!(self.role, SectionRole::Intro) {
            e -= 0.1;
        }
        e.clamp(0.0, 1.0)
    }
}

/// Generates a clip for the layer `track` (id/name/kind) in `section` and stores it (unless locked).
pub fn generate_track(session: &mut Session, track: &str, section_id: &str, params: &GenParams) -> Result<Clip> {
    let track_id = session.track_by(track).map(|t| t.id.clone()).ok_or_else(|| Error::UnknownTrack(track.into()))?;
    let kind = session.track_by(&track_id).unwrap().kind;
    let mut section = session.section(section_id).cloned().ok_or_else(|| Error::UnknownSection(section_id.into()))?;
    let needs_chords = !matches!(kind, TrackRole::Drums | TrackRole::Percussion);
    if section.chords.is_empty() && needs_chords {
        // No harmony yet: pick an idiomatic progression for the style so the first click always works.
        let style = params.style(session).to_string();
        let seed = params.seed.unwrap_or(session.seed);
        let sug = chords::suggest_progressions(session, Some(&style), 1, seed);
        let notation = sug.first().map(|(sym, _)| sym.clone()).ok_or_else(|| Error::Parse("no chord suggestion available".into()))?;
        let events = crate::notation::parse_chords(&notation, &session.key, section.bars, session.bar_ticks())?;
        section.chords = events.clone();
        // Melody-first: keep the section chord-less so `harmonize` can fit chords to the melody later.
        if !matches!(kind, TrackRole::Melody | TrackRole::CounterMelody | TrackRole::Harmony) {
            if let Some(sec) = session.section_mut(section_id) {
                sec.chords = events;
            }
        }
    }
    let ctx = SongCtx::of(session, &section.id);
    let mut notes = match kind {
        TrackRole::Chords => chords::render_chords(session, &section, params, ctx),
        TrackRole::Melody => {
            let s = params.style(session).to_ascii_lowercase();
            if s.contains("kid") || s.contains("nursery") || s.contains("rhyme") || params.lyrics.is_some() || section.lyrics.is_some() {
                kids::generate_kids_melody(session, &section, params)
            } else {
                melody::generate_melody(session, &section, params, ctx)
            }
        }
        TrackRole::Bass => bass::generate_bass(session, &section, params, ctx),
        TrackRole::Drums => drums::generate_drums(session, &section, params, ctx),
        TrackRole::Pad => layers::generate_pad(session, &section, params, ctx),
        TrackRole::Arpeggio => layers::generate_arp(session, &section, params, ctx),
        TrackRole::Pluck => layers::generate_pluck(session, &section, params, ctx),
        TrackRole::CounterMelody => layers::generate_counter(session, &section, params, ctx),
        TrackRole::Harmony => layers::generate_harmony(session, &section, params, ctx),
        TrackRole::Sub => layers::generate_sub(session, &section, params, ctx),
        TrackRole::Percussion => layers::generate_percussion(session, &section, params, ctx),
    };
    if params.humanize.unwrap_or(true) {
        let mut hp = HumanizeParams::preset(kind.humanize_base(), params.style(session));
        hp.seed = params.seed.unwrap_or(session.seed);
        humanize(&mut notes, &hp, kind.humanize_base(), session.tempo, session.bar_ticks());
    }
    clamp_to_section(&mut notes, section.bars * session.bar_ticks());
    let clip = Clip::new(notes, ClipSource::Generated { seed: params.seed.unwrap_or(session.seed), params: serde_json::to_value(params).unwrap_or_default() });
    if !session.track_by(&track_id).unwrap().locked {
        session.set_clip(&track_id, &section.id, clip.clone())?;
    }
    Ok(clip)
}

/// Generates every unlocked, active layer of a section in dependency order.
pub fn generate_section(session: &mut Session, section_id: &str, params: &GenParams) -> Result<Vec<(String, Clip)>> {
    let mut ids: Vec<(u8, String)> = session.tracks.iter().map(|t| (t.kind.order(), t.id.clone())).collect();
    ids.sort();
    let sec_id = session.section(section_id).map(|s| s.id.clone()).ok_or_else(|| Error::UnknownSection(section_id.into()))?;
    let mut out = Vec::new();
    for (_, id) in ids {
        let t = session.track_by(&id).unwrap();
        if t.locked || !t.active_in(&sec_id) {
            continue;
        }
        let clip = generate_track(session, &id, &sec_id, params)?;
        out.push((id, clip));
    }
    Ok(out)
}

/// Generates the whole song: every section, every unlocked active layer, with melodic continuity
/// (later sections reuse the first melody's motif) and section-role contrast.
pub fn generate_song(session: &mut Session, params: &GenParams) -> Result<Vec<(String, String)>> {
    let sections: Vec<String> = session.sections.iter().map(|s| s.id.clone()).collect();
    let mut done = Vec::new();
    for sid in sections {
        for (tid, _) in generate_section(session, &sid, params)? {
            done.push((sid.clone(), tid));
        }
    }
    Ok(done)
}

/// Removes notes that would sound past the end of the section and clamps lengths.
pub(crate) fn clamp_to_section(notes: &mut Vec<crate::model::Note>, total: u32) {
    notes.retain(|n| n.start < total);
    for n in notes.iter_mut() {
        n.len = n.len.min(total - n.start).max(1);
    }
}
