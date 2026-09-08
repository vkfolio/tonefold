//! Rule-based generators. Each takes a [`Session`] + section and returns notes relative to the section start.
//! The agent normally decides *what* (form, harmony, motif, style) and calls these to realise *how*.
//! Generators read the section's role/energy and its position in the song so verses, choruses,
//! builds and outros come out different, and derive from the lead/chords that already exist.

pub mod bass;
pub mod chords;
pub mod drums;
pub mod expression;
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

/// FNV-1a. Used instead of `DefaultHasher` because `RandomState` is seeded per process, which
/// would make the same seed produce a different song on every run.
pub fn hash_str(s: &str) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A deterministic roll in 0..1 for bar `bar` of a clip. Hashing rather than drawing from the
/// generator's `ChaCha8Rng` keeps variation order-independent: adding a new roll later does not
/// shift every draw that follows it and silently rewrite the rest of the part.
pub fn bar_roll(seed: u64, bar: u32, salt: u64) -> f32 {
    let h = splitmix64(seed ^ (bar as u64).wrapping_mul(0x2545_F491_4F6C_DD1D) ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    (h >> 11) as f32 / (1u64 << 53) as f32
}

impl GenParams {
    pub fn rng(&self, session: &Session, salt: u64) -> ChaCha8Rng {
        ChaCha8Rng::seed_from_u64(self.seed.unwrap_or(session.seed).wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(salt))
    }

    /// Per-section RNG: the same layer in two sections (or in the same section twice over) draws a
    /// different stream, so a repeated chorus is a new performance rather than a copy.
    pub fn rng_in(&self, session: &Session, salt: u64, section: &Section, ctx: SongCtx) -> ChaCha8Rng {
        ChaCha8Rng::seed_from_u64(splitmix64(self.stream(session, salt, section, ctx)))
    }

    /// The seed behind [`rng_in`], also used for per-bar rolls and for humanization.
    pub fn stream(&self, session: &Session, salt: u64, section: &Section, ctx: SongCtx) -> u64 {
        self.seed
            .unwrap_or(session.seed)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(salt)
            .wrapping_add(hash_str(&section.id).wrapping_mul(0x0000_0100_0000_01B3))
            .wrapping_add((ctx.occurrence as u64).wrapping_mul(0x2545_F491_4F6C_DD1D))
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
                kids::generate_kids_melody(session, &section, params, ctx)
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
        let style = params.style(session).to_string();
        // Groove first: it says where this style systematically sits against the grid, and it bins
        // notes by their nearest 16th — so it has to see them before humanization moves them.
        let groove_name = session.track_by(&track_id).and_then(|t| t.groove.clone()).or_else(|| session.groove.clone());
        let groove = match groove_name.as_deref() {
            Some("none") => None,
            Some(name) => crate::humanize::groove_template(name, kind, session.tempo),
            None => crate::humanize::StyleFeel::of(&style).groove.and_then(|n| crate::humanize::groove_template(n, kind, session.tempo)),
        };
        if let Some(g) = groove {
            crate::humanize::apply_groove(&mut notes, &g, 0.6, session.bar_ticks());
        }
        let mut hp = HumanizeParams::preset(kind.humanize_base(), params.style(session));
        // Section in the seed so repeats breathe differently; the real role (not the preset's base)
        // so a pad, an arp and the chords do not all move on the same random walk.
        hp.seed = params.seed.unwrap_or(session.seed) ^ hash_str(&section.id).wrapping_mul(0x9E37_79B9);
        humanize(&mut notes, &hp, kind, session.tempo, session.bar_ticks());
    }
    clamp_to_section(&mut notes, section.bars * session.bar_ticks());
    let energy = ctx.effective_energy(params.energy(&section));
    let lanes = expression::lanes_for(kind, &section, &notes, ctx, energy, session.bar_ticks());
    let clip = Clip::with_automation(notes, ClipSource::Generated { seed: params.seed.unwrap_or(session.seed), params: serde_json::to_value(params).unwrap_or_default() }, lanes);
    if clip.automation.iter().any(|a| a.target == crate::model::AutoTarget::PitchBend) {
        if let Some(t) = session.track_by_mut(&track_id) {
            t.bend_range = t.bend_range.max(12);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::analyze_clip;
    use crate::model::SectionRole;

    fn song(style: &str) -> Session {
        let mut s = Session::default();
        s.style = style.into();
        for (name, bars, role) in [("Verse", 8, SectionRole::Verse), ("Chorus", 8, SectionRole::Chorus), ("Verse 2", 8, SectionRole::Verse), ("Chorus 2", 8, SectionRole::Chorus)] {
            let id = s.add_section(name, bars, role.default_energy());
            s.section_mut(&id).unwrap().role = role;
        }
        generate_song(&mut s, &GenParams::default()).unwrap();
        s
    }

    /// The seeded contract: the same seed must still produce the same song, run after run.
    #[test]
    fn same_seed_same_song() {
        let a = serde_json::to_string(&song("pop")).unwrap();
        let b = serde_json::to_string(&song("pop")).unwrap();
        assert_eq!(a, b, "generation is not reproducible from the seed");
    }

    /// ...but a repeat is a new performance, not a photocopy: the second chorus must differ from
    /// the first while still being recognisably the same music.
    #[test]
    fn repeated_sections_differ() {
        let s = song("pop");
        let choruses: Vec<String> = s.sections.iter().filter(|x| x.role == SectionRole::Chorus).map(|x| x.id.clone()).collect();
        assert_eq!(choruses.len(), 2, "fixture should have two choruses");
        let (a, b) = (choruses[0].as_str(), choruses[1].as_str());
        let (mut compared, mut differing) = (0, 0);
        for t in &s.tracks {
            let (Some(ca), Some(cb)) = (s.clip(&t.id, a), s.clip(&t.id, b)) else { continue };
            if ca.notes.is_empty() || cb.notes.is_empty() {
                continue;
            }
            compared += 1;
            if ca.notes != cb.notes {
                differing += 1;
            }
            // Same music, though: the two takes should still share most of their pitch content.
            let pa: std::collections::HashSet<u8> = ca.notes.iter().map(|n| n.pitch).collect();
            let pb: std::collections::HashSet<u8> = cb.notes.iter().map(|n| n.pitch).collect();
            let shared = pa.intersection(&pb).count() as f32 / pa.union(&pb).count().max(1) as f32;
            assert!(shared > 0.3, "{} drifted into unrelated material (pitch overlap {shared:.2})", t.id);
        }
        assert!(compared >= 3, "expected several layers in both choruses, got {compared}");
        assert_eq!(differing, compared, "some layers are byte-identical between the two choruses");
    }

    /// The engine must not produce clips that its own analysis calls out as robotic.
    #[test]
    fn engine_never_trips_its_own_warnings() {
        for style in ["pop", "lofi", "cinematic"] {
            let s = song(style);
            for t in &s.tracks {
                for sec in &s.sections {
                    let Some(clip) = s.clip(&t.id, &sec.id) else { continue };
                    if clip.notes.is_empty() {
                        continue;
                    }
                    let a = analyze_clip(&s, t.kind, sec, &clip.notes);
                    for w in a.warnings.iter() {
                        assert!(!w.contains("nearly uniform"), "{style}/{}/{}: {w}", t.id, sec.id);
                    }
                }
            }
        }
    }
}
