//! Standard MIDI file I/O via `midly`, plus the JSON export consumed by the FL piano-roll script.

use crate::model::{AutoTarget, Note, Session, PPQ};
use crate::{Error, Result};
use midly::num::{u15, u24, u28, u4, u7};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, Track, TrackEvent, TrackEventKind};
use serde::{Deserialize, Serialize};

pub struct MidiTrack<'a> {
    pub name: &'a str,
    pub channel: u8,
    pub notes: &'a [Note],
    /// Controller lanes: (tick, target, value). Written as ordinary CC and pitch-bend messages, so
    /// any instrument reads them and one that ignores them still plays the notes.
    pub automation: &'a [(u32, AutoTarget, f32)],
    /// Declared through RPN 0 at the head of the track.
    pub bend_range: u8,
}

impl<'a> MidiTrack<'a> {
    pub fn new(name: &'a str, channel: u8, notes: &'a [Note]) -> Self {
        MidiTrack { name, channel, notes, automation: &[], bend_range: 2 }
    }
}

/// Writes a type-1 SMF with one track per entry. Notes are song-absolute ticks at [`PPQ`].
pub fn write_smf(path: &std::path::Path, tempo: f32, time_sig: (u32, u32), tracks: &[MidiTrack]) -> Result<()> {
    let bytes = smf_bytes(tempo, time_sig, tracks)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

pub fn smf_bytes(tempo: f32, time_sig: (u32, u32), tracks: &[MidiTrack]) -> Result<Vec<u8>> {
    let mut smf = Smf::new(Header::new(Format::Parallel, Timing::Metrical(u15::new(PPQ as u16))));
    // Conductor track.
    let mut cond: Track = Vec::new();
    let us_per_beat = (60_000_000.0 / tempo.max(1.0)) as u32;
    cond.push(TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(us_per_beat))) });
    let den_pow = (time_sig.1 as f32).log2() as u8;
    cond.push(TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::TimeSignature(time_sig.0 as u8, den_pow, 24, 8)) });
    cond.push(TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) });
    smf.tracks.push(cond);

    for t in tracks {
        let mut events: Vec<(u32, u8, TrackEventKind)> = Vec::new(); // (tick, order, kind)
        let ch = u4::new(t.channel.min(15));
        // Declare the bend range up front, or a 12-semitone slide is played as 2.
        if t.automation.iter().any(|(_, target, _)| *target == AutoTarget::PitchBend) {
            for (cc, value) in [(101u8, 0u8), (100, 0), (6, t.bend_range.clamp(1, 24)), (38, 0)] {
                events.push((0, 0, TrackEventKind::Midi { channel: ch, message: MidiMessage::Controller { controller: u7::new(cc), value: u7::new(value) } }));
            }
        }
        for (tick, target, value) in t.automation {
            let message = match target {
                AutoTarget::PitchBend => MidiMessage::PitchBend { bend: midly::PitchBend(midly::num::u14::new(((value.clamp(-1.0, 1.0) * 8191.0) as i32 + 8192).clamp(0, 16383) as u16)) },
                other => MidiMessage::Controller { controller: u7::new(other.cc().unwrap_or(11)), value: u7::new((value.clamp(0.0, 1.0) * 127.0) as u8) },
            };
            events.push((*tick, 0, TrackEventKind::Midi { channel: ch, message }));
        }
        for n in t.notes {
            if let Some(l) = &n.lyric {
                events.push((n.start, 0, TrackEventKind::Meta(MetaMessage::Lyric(l.as_bytes()))));
            }
            events.push((n.start, 1, TrackEventKind::Midi { channel: ch, message: MidiMessage::NoteOn { key: u7::new(n.pitch.min(127)), vel: u7::new(n.vel_midi().max(1)) } }));
            events.push((n.end(), 0, TrackEventKind::Midi { channel: ch, message: MidiMessage::NoteOff { key: u7::new(n.pitch.min(127)), vel: u7::new(0) } }));
        }
        events.sort_by_key(|(t, o, _)| (*t, *o));
        let mut track: Track = Vec::new();
        track.push(TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::TrackName(t.name.as_bytes())) });
        let mut last = 0u32;
        for (tick, _, kind) in events {
            track.push(TrackEvent { delta: u28::new(tick - last), kind });
            last = tick;
        }
        track.push(TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) });
        smf.tracks.push(track);
    }
    let mut out = Vec::new();
    smf.write(&mut out).map_err(|e| Error::Midi(e.to_string()))?;
    Ok(out)
}

/// A track read from a MIDI file, rescaled to [`PPQ`].
#[derive(Debug, Clone)]
pub struct ImportedTrack {
    pub name: String,
    pub channel: u8,
    pub notes: Vec<Note>,
    /// Controller lanes read back from the file: (tick, target, value).
    pub automation: Vec<(u32, AutoTarget, f32)>,
}

pub struct Imported {
    pub tempo: Option<f32>,
    pub tracks: Vec<ImportedTrack>,
}

pub fn read_smf(bytes: &[u8]) -> Result<Imported> {
    let smf = Smf::parse(bytes).map_err(|e| Error::Midi(e.to_string()))?;
    let src_ppq = match smf.header.timing {
        Timing::Metrical(t) => t.as_int() as f64,
        Timing::Timecode(..) => return Err(Error::Midi("SMPTE timing not supported".into())),
    };
    let scale = PPQ as f64 / src_ppq;
    let mut tempo = None;
    let mut tracks = Vec::new();
    for track in &smf.tracks {
        let mut tick = 0u64;
        let mut name = String::new();
        let mut channel = 0u8;
        let mut open: std::collections::HashMap<(u8, u8), (u64, u8, Option<String>)> = Default::default();
        let mut pending_lyric: Option<String> = None;
        let mut notes = Vec::new();
        let mut automation: Vec<(u32, AutoTarget, f32)> = Vec::new();
        for ev in track {
            tick += ev.delta.as_int() as u64;
            match ev.kind {
                TrackEventKind::Meta(MetaMessage::Tempo(t)) => tempo = Some(60_000_000.0 / t.as_int() as f32),
                TrackEventKind::Meta(MetaMessage::TrackName(n)) => name = String::from_utf8_lossy(n).into_owned(),
                TrackEventKind::Meta(MetaMessage::Lyric(l)) => pending_lyric = Some(String::from_utf8_lossy(l).into_owned()),
                TrackEventKind::Midi { channel: ch, message } => {
                    channel = ch.as_int();
                    match message {
                        MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                            open.insert((ch.as_int(), key.as_int()), (tick, vel.as_int(), pending_lyric.take()));
                        }
                        MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                            if let Some((start, vel, lyric)) = open.remove(&(ch.as_int(), key.as_int())) {
                                let mut n = Note::new(key.as_int(), (start as f64 * scale).round() as u32, (((tick - start) as f64) * scale).round().max(1.0) as u32, vel as f32 / 127.0);
                                n.lyric = lyric;
                                notes.push(n);
                            }
                        }
                        MidiMessage::Controller { controller, value } => {
                            let target = match controller.as_int() {
                                1 => Some(AutoTarget::Modulation),
                                11 => Some(AutoTarget::Expression),
                                64 => Some(AutoTarget::Sustain),
                                _ => None,
                            };
                            if let Some(target) = target {
                                automation.push(((tick as f64 * scale).round() as u32, target, value.as_int() as f32 / 127.0));
                            }
                        }
                        MidiMessage::PitchBend { bend } => {
                            automation.push(((tick as f64 * scale).round() as u32, AutoTarget::PitchBend, (bend.0.as_int() as f32 - 8192.0) / 8191.0));
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        if !notes.is_empty() {
            notes.sort_by_key(|n| (n.start, n.pitch));
            tracks.push(ImportedTrack { name, channel, notes, automation });
        }
    }
    Ok(Imported { tempo, tracks })
}

/// JSON export format read by `flscript/FLVSTX Import.pyscript`.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExportFile {
    pub ppq: u32,
    pub tempo: f32,
    pub tracks: Vec<ExportTrack>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportTrack {
    pub name: String,
    pub notes: Vec<ExportNote>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportNote {
    pub pitch: u8,
    pub start: u32,
    pub len: u32,
    pub vel: f32,
    pub color: u8,
}

/// Builds the export for the whole song (or one section when `section` is given) for all tracks.
pub fn export_session(session: &Session, section: Option<&str>) -> Result<ExportFile> {
    if let Some(id) = section {
        if session.section(id).is_none() {
            return Err(Error::UnknownSection(id.into()));
        }
    }
    let mut tracks = Vec::new();
    for (i, t) in session.tracks.iter().enumerate() {
        let notes: Vec<Note> = match section {
            Some(id) => if t.active_in(&session.section(id).unwrap().id) { session.clip(&t.id, id).map(|c| c.notes.clone()).unwrap_or_default() } else { Vec::new() },
            None => session.flatten(&t.id),
        };
        tracks.push(ExportTrack {
            name: t.id.clone(),
            notes: notes.iter().map(|n| ExportNote { pitch: n.pitch, start: n.start, len: n.len, vel: n.vel, color: (i % 16) as u8 }).collect(),
        });
    }
    Ok(ExportFile { ppq: PPQ, tempo: session.tempo, tracks })
}

/// Writes both `latest.json` and `latest.mid` (plus per-track .mid files) into `dir`.
pub fn export_to_dir(session: &Session, section: Option<&str>, dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let export = export_session(session, section)?;
    let mut written = Vec::new();
    let json = dir.join("latest.json");
    std::fs::write(&json, serde_json::to_vec_pretty(&export).map_err(|e| Error::Midi(e.to_string()))?)?;
    written.push(json);
    let ts = (session.time_sig.num, session.time_sig.den);
    let all: Vec<(String, u8, Vec<Note>, Vec<(u32, AutoTarget, f32)>, u8)> = session
        .tracks
        .iter()
        .map(|t| {
            let (notes, auto) = match section {
                Some(id) if t.active_in(&session.section(id).unwrap().id) => (
                    session.clip(&t.id, id).map(|c| c.notes.clone()).unwrap_or_default(),
                    session.clip(&t.id, id).map(|c| c.automation.iter().flat_map(|a| a.points.iter().map(|p| (p.tick, a.target, p.value)).collect::<Vec<_>>()).collect()).unwrap_or_default(),
                ),
                Some(_) => (Vec::new(), Vec::new()),
                None => (session.flatten(&t.id), session.flatten_automation(&t.id)),
            };
            (t.id.clone(), t.channel, notes, auto, t.bend_range)
        })
        .collect();
    let tracks: Vec<MidiTrack> = all.iter().map(|(id, ch, n, a, br)| MidiTrack { name: id, channel: *ch, notes: n, automation: a, bend_range: *br }).collect();
    let mid = dir.join("latest.mid");
    write_smf(&mid, session.tempo, ts, &tracks)?;
    written.push(mid);
    for (id, ch, n, a, br) in &all {
        if !n.is_empty() {
            let p = dir.join(format!("{}.mid", id));
            write_smf(&p, session.tempo, ts, &[MidiTrack { name: id, channel: *ch, notes: n, automation: a, bend_range: *br }])?;
            written.push(p);
        }
    }
    Ok(written)
}

/// Default export directory: `%LOCALAPPDATA%\FLVSTX\export`.
pub fn default_export_dir() -> std::path::PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_else(|| std::env::temp_dir());
    base.join("FLVSTX").join("export")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expression has to survive the trip to disk, or none of it reaches FL Studio.
    #[test]
    fn expression_roundtrips() {
        let notes = vec![Note::new(48, 0, PPQ * 2, 0.9)];
        let automation = vec![
            (0u32, AutoTarget::Expression, 0.4),
            (PPQ, AutoTarget::Expression, 1.0),
            (0, AutoTarget::Sustain, 1.0),
            (PPQ * 2, AutoTarget::Sustain, 0.0),
            (0, AutoTarget::PitchBend, -0.5),
            (PPQ / 4, AutoTarget::PitchBend, 0.0),
        ];
        let track = MidiTrack { name: "bass", channel: 2, notes: &notes, automation: &automation, bend_range: 12 };
        let bytes = smf_bytes(120.0, (4, 4), &[track]).unwrap();
        let back = read_smf(&bytes).unwrap();
        let t = &back.tracks[0];
        let got = |target: AutoTarget| -> Vec<(u32, f32)> { t.automation.iter().filter(|(_, x, _)| *x == target).map(|(tick, _, v)| (*tick, *v)).collect() };

        let expr = got(AutoTarget::Expression);
        assert_eq!(expr.len(), 2, "expression lane lost: {expr:?}");
        assert!((expr[0].1 - 0.4).abs() < 0.02 && (expr[1].1 - 1.0).abs() < 0.02);

        let sus = got(AutoTarget::Sustain);
        assert_eq!(sus.len(), 2);
        assert!(sus[0].1 > 0.5 && sus[1].1 < 0.5, "pedal did not come back up");

        let bend = got(AutoTarget::PitchBend);
        assert_eq!(bend.len(), 2);
        assert!((bend[0].1 + 0.5).abs() < 0.02, "bend depth lost: {bend:?}");
        assert!(bend[1].1.abs() < 0.02, "bend did not return to centre");

        // The range has to be declared or a 12-semitone slide plays as 2.
        let rpn: Vec<u8> = bytes.windows(2).filter(|w| w[0] == 0x06).map(|w| w[1]).collect();
        assert!(rpn.contains(&12), "RPN bend-range preamble missing");
    }

    #[test]
    fn smf_roundtrip() {
        let notes = vec![Note::new(60, 0, PPQ, 0.8), Note::new(64, PPQ, PPQ / 2, 0.5), Note { pitch: 67, start: PPQ * 2, len: PPQ, vel: 1.0, lyric: Some("la".into()), slide: None }];
        let bytes = smf_bytes(120.0, (4, 4), &[MidiTrack::new("melody", 1, &notes)]).unwrap();
        let back = read_smf(&bytes).unwrap();
        assert_eq!(back.tempo.map(|t| t.round()), Some(120.0));
        assert_eq!(back.tracks.len(), 1);
        let key = |v: &Vec<Note>| v.iter().map(|n| (n.pitch, n.start, n.len, n.vel_midi(), n.lyric.clone())).collect::<Vec<_>>();
        assert_eq!(key(&back.tracks[0].notes), key(&notes));
    }
}
