//! Rule-based generators. Each takes a [`Session`] + section and returns notes relative to the section start.
//! The agent normally decides *what* (form, harmony, motif, style) and calls these to realise *how*.

pub mod bass;
pub mod chords;
pub mod drums;
pub mod harmonize;
pub mod kids;
pub mod melody;

use crate::model::{Clip, ClipSource, Section, Session, TrackRole};
use crate::humanize::{humanize, HumanizeParams};
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
    /// Melody: motif in melodic notation to develop (else one is invented).
    #[serde(default)]
    pub motif: Option<String>,
    /// Melody: contour hint "arch", "rise", "fall", "wave".
    #[serde(default)]
    pub contour: Option<String>,
    /// Melody/kids: lyrics to set (one syllable per note).
    #[serde(default)]
    pub lyrics: Option<String>,
    /// Bass: pattern name ("root", "root5", "octave", "walking", "pedal", "push", "808").
    #[serde(default)]
    pub pattern: Option<String>,
    /// Drums: fill on the last bar of each 4-bar phrase.
    #[serde(default)]
    pub fills: Option<bool>,
    /// Apply humanization after generating (default true).
    #[serde(default)]
    pub humanize: Option<bool>,
    /// Note density 0..1 (melody/bass/drums), overrides energy-derived default.
    #[serde(default)]
    pub density: Option<f32>,
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

/// Generates a clip for `role` in `section` and stores it in the session (unless the track is locked).
pub fn generate_track(session: &mut Session, role: TrackRole, section_id: &str, params: &GenParams) -> Result<Clip> {
    let mut section = session.section(section_id).cloned().ok_or_else(|| Error::UnknownSection(section_id.into()))?;
    if section.chords.is_empty() && role != TrackRole::Drums {
        // No harmony yet: pick an idiomatic progression for the style so the first click always works.
        let style = params.style(session).to_string();
        let seed = params.seed.unwrap_or(session.seed);
        let sug = chords::suggest_progressions(session, Some(&style), 1, seed);
        let notation = sug.first().map(|(sym, _)| sym.clone()).ok_or_else(|| Error::Parse("no chord suggestion available".into()))?;
        let events = crate::notation::parse_chords(&notation, &session.key, section.bars, session.bar_ticks())?;
        section.chords = events.clone();
        // Melody-first: keep the section chord-less so `harmonize` can fit chords to the melody later.
        // Chords/bass need real harmony, so those store the suggestion.
        if role != TrackRole::Melody {
            if let Some(sec) = session.section_mut(section_id) {
                sec.chords = events;
            }
        }
    }
    let mut notes = match role {
        TrackRole::Chords => chords::render_chords(session, &section, params),
        TrackRole::Melody => {
            let s = params.style(session).to_ascii_lowercase();
            if s.contains("kid") || s.contains("nursery") || s.contains("rhyme") || params.lyrics.is_some() || section.lyrics.is_some() {
                kids::generate_kids_melody(session, &section, params)
            } else {
                melody::generate_melody(session, &section, params)
            }
        }
        TrackRole::Bass => bass::generate_bass(session, &section, params),
        TrackRole::Drums => drums::generate_drums(session, &section, params),
    };
    if params.humanize.unwrap_or(true) {
        let mut hp = HumanizeParams::preset(role, params.style(session));
        hp.seed = params.seed.unwrap_or(session.seed);
        humanize(&mut notes, &hp, role, session.tempo, session.bar_ticks());
    }
    clamp_to_section(&mut notes, section.bars * session.bar_ticks());
    let clip = Clip::new(notes, ClipSource::Generated { seed: params.seed.unwrap_or(session.seed), params: serde_json::to_value(params).unwrap_or_default() });
    if !session.track(role).locked {
        session.set_clip(role, &section.id, clip.clone())?;
    }
    Ok(clip)
}

/// Removes notes that would sound past the end of the section and clamps lengths.
pub(crate) fn clamp_to_section(notes: &mut Vec<crate::model::Note>, total: u32) {
    notes.retain(|n| n.start < total);
    for n in notes.iter_mut() {
        n.len = n.len.min(total - n.start).max(1);
    }
}
