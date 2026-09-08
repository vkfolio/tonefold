//! Offline render of a session through a SoundFont, and a plain 16-bit WAV writer. Enabled by the
//! `render` feature so the plugin, the CLI and tests all bounce audio the same way.

use crate::playback::timeline;
use crate::{Session, PPQ};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

/// Silence rendered after the last note so releases and reverb tails are not cut off.
pub const TAIL_SECONDS: f32 = 2.0;
pub const SAMPLE_RATE: i32 = 44_100;

/// Renders `section` (or the whole song) to stereo buffers, skipping muted layers.
pub fn render_stereo(
    session: &Session,
    section: Option<&str>,
    muted: &[String],
    soundfont: &Arc<rustysynth::SoundFont>,
) -> Result<(Vec<f32>, Vec<f32>), String> {
    let tl = timeline(session, section, muted);
    if tl.events.is_empty() {
        return Err("nothing to render (no notes in this selection)".into());
    }

    let mut settings = rustysynth::SynthesizerSettings::new(SAMPLE_RATE);
    settings.enable_reverb_and_chorus = true;
    settings.maximum_polyphony = 96;
    let mut synth = rustysynth::Synthesizer::new(soundfont, &settings).map_err(|e| e.to_string())?;
    for (ch, program) in tl.programs.iter().enumerate() {
        if ch != 9 && *program < 128 {
            synth.process_midi_message(ch as i32, 0xC0, *program as i32, 0);
        }
    }

    // Ticks are laid out at the session tempo; the synth has no tempo of its own.
    let samples_per_tick = 60.0 / (tl.tempo.max(1.0) as f64 * PPQ as f64) * SAMPLE_RATE as f64;
    let total = ((tl.loop_end.saturating_sub(tl.loop_start)) as f64 * samples_per_tick) as usize + (TAIL_SECONDS * SAMPLE_RATE as f32) as usize;
    let mut left = vec![0.0f32; total];
    let mut right = vec![0.0f32; total];

    let mut pos = 0usize;
    for e in &tl.events {
        let at = (((e.tick.saturating_sub(tl.loop_start)) as f64 * samples_per_tick) as usize).min(total);
        if at > pos {
            synth.render(&mut left[pos..at], &mut right[pos..at]);
            pos = at;
        }
        if e.on {
            synth.note_on(e.synth_channel as i32, e.pitch as i32, ((e.vel * 127.0) as i32).clamp(1, 127));
        } else {
            synth.note_off(e.synth_channel as i32, e.pitch as i32);
        }
    }
    if pos < total {
        synth.render(&mut left[pos..], &mut right[pos..]);
    }
    Ok((left, right))
}

/// Where the installer puts the General MIDI soundfont (`FLVSTX_SOUNDFONT` overrides it).
pub fn default_soundfont_path() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os("FLVSTX_SOUNDFONT") {
        return p.into();
    }
    crate::midi::default_export_dir().parent().map(|p| p.join("soundfont").join("GeneralUser-GS.sf2")).unwrap_or_default()
}

/// Loads a SoundFont from disk.
pub fn load_soundfont(path: &Path) -> Result<Arc<rustysynth::SoundFont>, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    rustysynth::SoundFont::new(&mut file).map(Arc::new).map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes 16-bit PCM stereo at [`SAMPLE_RATE`], scaling by `gain`.
pub fn write_wav(path: &Path, left: &[f32], right: &[f32], gain: f32) -> Result<(), String> {
    let frames = left.len().min(right.len());
    let data_bytes = (frames * 4) as u32;
    let mut out = Vec::with_capacity(44 + data_bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&2u16.to_le_bytes()); // stereo
    out.extend_from_slice(&(SAMPLE_RATE as u32).to_le_bytes());
    out.extend_from_slice(&((SAMPLE_RATE as u32) * 4).to_le_bytes()); // byte rate
    out.extend_from_slice(&4u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for i in 0..frames {
        for s in [left[i], right[i]] {
            let v = (s * gain).clamp(-1.0, 1.0);
            out.extend_from_slice(&((v * 32767.0) as i16).to_le_bytes());
        }
    }
    let mut f = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(&out).map_err(|e| e.to_string())
}

/// Peak of a render, and the gain that keeps it just under full scale.
pub fn peak_and_gain(left: &[f32], right: &[f32]) -> (f32, f32) {
    let peak = left.iter().chain(right.iter()).fold(0.0f32, |m, s| m.max(s.abs()));
    (peak, if peak > 0.99 { 0.99 / peak } else { 1.0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Clip, ClipSource, Note, Section, Session};

    /// Renders a bar of C major through the installed soundfont. Skipped when it is not present
    /// (the installer downloads it), since there is nothing to render without one.
    #[test]
    fn renders_audible_samples() {
        let path = default_soundfont_path();
        let Ok(sf) = load_soundfont(&path) else {
            eprintln!("skipping: no soundfont at {}", path.display());
            return;
        };

        let mut session = Session::default();
        session.sections.push(Section::new("verse", "Verse", 1, 0.5));
        let section = session.sections[0].id.clone();
        let notes = vec![Note::new(60, 0, PPQ, 0.9), Note::new(64, PPQ, PPQ, 0.9), Note::new(67, PPQ * 2, PPQ * 2, 0.9)];
        let track = session.tracks[0].id.clone();
        session.track_by_mut(&track).unwrap().clips.insert(section.clone(), Clip::new(notes, ClipSource::Edited));

        let (left, right) = render_stereo(&session, Some(&section), &[], &sf).expect("render");
        assert_eq!(left.len(), right.len());
        let (peak, _) = peak_and_gain(&left, &right);
        assert!(peak > 0.01, "rendered audio is silent (peak {peak})");

        let out = std::env::temp_dir().join("flvstx-render-test.wav");
        write_wav(&out, &left, &right, 1.0).expect("write");
        let bytes = std::fs::read(&out).expect("read back");
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(bytes.len(), 44 + left.len() * 4);
        let _ = std::fs::remove_file(&out);
    }
}
