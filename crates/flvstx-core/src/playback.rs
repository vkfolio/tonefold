//! Flattening a session into a sorted event timeline — the one place that decides which layer plays
//! on which channel, so the plugin's audio thread, the offline WAV render and the CLI all hear the
//! same thing.

use crate::model::AutoTarget;
use crate::{Session, PPQ};

/// What an event does. Kept `Copy` and allocation-free: the audio thread walks these directly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EventKind {
    NoteOn { pitch: u8, vel: f32 },
    NoteOff { pitch: u8 },
    /// Control change (expression, sustain, modulation).
    Cc { cc: u8, value: u8 },
    /// Pitch bend, -8192..8191 around centre.
    Bend { value: i16 },
}

impl EventKind {
    /// Order within one tick: controllers first (a pedal must be down before the note it holds),
    /// then note-offs, then note-ons so a retrigger does not cut its own new note.
    fn order(&self) -> u8 {
        match self {
            EventKind::Cc { .. } | EventKind::Bend { .. } => 0,
            EventKind::NoteOff { .. } => 1,
            EventKind::NoteOn { .. } => 2,
        }
    }
}

/// One scheduled MIDI event in song ticks.
#[derive(Debug, Clone, Copy)]
pub struct Event {
    pub tick: u32,
    pub channel: u8,
    /// Channel for the built-in synth (drums/percussion always go to the GM percussion channel 9).
    pub synth_channel: u8,
    pub kind: EventKind,
}

/// A section (or whole song) laid out as events, with what each synth channel needs to play it.
#[derive(Debug, Default, Clone)]
pub struct Timeline {
    pub events: Vec<Event>,
    pub loop_start: u32,
    pub loop_end: u32,
    pub tempo: f32,
    /// General MIDI program per synth channel (128 = drums).
    pub programs: [u8; 16],
    /// Pitch-bend range in semitones per synth channel.
    pub bend_ranges: [u8; 16],
}

/// Synth channel for a layer: pitched layers keep their MIDI channel (channel 10 is reserved for
/// percussion in General MIDI, so a pitched layer sitting there is moved), drums always play on it.
fn synth_channel(pitched: bool, channel: u8) -> u8 {
    if pitched {
        if channel == 9 {
            15
        } else {
            channel
        }
    } else {
        9
    }
}

/// Builds the timeline for `section` (or the whole song when `None`), skipping muted layers.
pub fn timeline(session: &Session, section: Option<&str>, muted: &[String]) -> Timeline {
    let mut events = Vec::new();
    let bar = session.bar_ticks();
    let (loop_start, loop_end) = match section.and_then(|id| session.section(id).map(|s| (session.section_start(&s.id).unwrap_or(0), s.bars * bar))) {
        Some((start, len)) => (start, start + len),
        None => (0, session.total_ticks().max(bar)),
    };
    let mut programs = [0u8; 16];
    let mut bend_ranges = [2u8; 16];
    programs[9] = crate::gm::DRUM_KIT;
    for t in &session.tracks {
        let sch = synth_channel(t.kind.is_pitched(), t.channel);
        if t.kind.is_pitched() {
            programs[sch as usize] = t.program(&session.style);
            bend_ranges[sch as usize] = t.bend_range.clamp(1, 24);
        }
        if muted.contains(&t.id) || t.muted {
            continue;
        }
        for n in session.flatten(&t.id) {
            if n.start >= loop_end || n.end() <= loop_start {
                continue;
            }
            let off = n.end().min(loop_end.saturating_sub(1)).max(n.start + 1);
            events.push(Event { tick: n.start, channel: t.channel, synth_channel: sch, kind: EventKind::NoteOn { pitch: n.pitch, vel: n.vel } });
            events.push(Event { tick: off, channel: t.channel, synth_channel: sch, kind: EventKind::NoteOff { pitch: n.pitch } });
        }
        for (tick, target, value) in session.flatten_automation(&t.id) {
            if tick < loop_start || tick >= loop_end {
                continue;
            }
            let kind = match target {
                AutoTarget::PitchBend => EventKind::Bend { value: (value.clamp(-1.0, 1.0) * 8191.0) as i16 },
                other => EventKind::Cc { cc: other.cc().unwrap_or(11), value: (value.clamp(0.0, 1.0) * 127.0) as u8 },
            };
            events.push(Event { tick, channel: t.channel, synth_channel: sch, kind });
        }
    }
    events.sort_by_key(|e| (e.tick, e.kind.order()));
    Timeline { events, loop_start, loop_end, tempo: session.tempo, programs, bend_ranges }
}

/// Song ticks advanced per audio sample at this tempo.
pub fn ticks_per_sample(tempo: f32, sample_rate: f32) -> f64 {
    (tempo as f64 / 60.0) * PPQ as f64 / sample_rate as f64
}
