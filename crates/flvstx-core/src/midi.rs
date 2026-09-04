//! Standard MIDI file I/O via `midly`, plus the JSON export consumed by the FL piano-roll script.

use crate::model::{Note, Session, TrackRole, PPQ};
use crate::{Error, Result};
use midly::num::{u15, u24, u28, u4, u7};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, Track, TrackEvent, TrackEventKind};
use serde::{Deserialize, Serialize};

pub struct MidiTrack<'a> {
    pub name: &'a str,
    pub channel: u8,
    pub notes: &'a [Note],
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
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        if !notes.is_empty() {
            notes.sort_by_key(|n| (n.start, n.pitch));
            tracks.push(ImportedTrack { name, channel, notes });
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
    let mut tracks = Vec::new();
    for role in TrackRole::ALL {
        let notes: Vec<Note> = match section {
            Some(id) => session.clip(role, id).map(|c| c.notes.clone()).unwrap_or_default(),
            None => session.flatten(role),
        };
        if section.is_some() && session.section(section.unwrap()).is_none() {
            return Err(Error::UnknownSection(section.unwrap().into()));
        }
        tracks.push(ExportTrack {
            name: role.name().to_string(),
            notes: notes.iter().map(|n| ExportNote { pitch: n.pitch, start: n.start, len: n.len, vel: n.vel, color: role as u8 }).collect(),
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
    let all: Vec<(TrackRole, Vec<Note>)> = TrackRole::ALL
        .iter()
        .map(|&r| (r, match section { Some(id) => session.clip(r, id).map(|c| c.notes.clone()).unwrap_or_default(), None => session.flatten(r) }))
        .collect();
    let tracks: Vec<MidiTrack> = all.iter().map(|(r, n)| MidiTrack { name: r.name(), channel: r.midi_channel(), notes: n }).collect();
    let mid = dir.join("latest.mid");
    write_smf(&mid, session.tempo, ts, &tracks)?;
    written.push(mid);
    for (r, n) in &all {
        if !n.is_empty() {
            let p = dir.join(format!("{}.mid", r.name()));
            write_smf(&p, session.tempo, ts, &[MidiTrack { name: r.name(), channel: r.midi_channel(), notes: n }])?;
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

    #[test]
    fn smf_roundtrip() {
        let notes = vec![Note::new(60, 0, PPQ, 0.8), Note::new(64, PPQ, PPQ / 2, 0.5), Note { pitch: 67, start: PPQ * 2, len: PPQ, vel: 1.0, lyric: Some("la".into()) }];
        let bytes = smf_bytes(120.0, (4, 4), &[MidiTrack { name: "melody", channel: 1, notes: &notes }]).unwrap();
        let back = read_smf(&bytes).unwrap();
        assert_eq!(back.tempo.map(|t| t.round()), Some(120.0));
        assert_eq!(back.tracks.len(), 1);
        let key = |v: &Vec<Note>| v.iter().map(|n| (n.pitch, n.start, n.len, n.vel_midi(), n.lyric.clone())).collect::<Vec<_>>();
        assert_eq!(key(&back.tracks[0].notes), key(&notes));
    }
}
