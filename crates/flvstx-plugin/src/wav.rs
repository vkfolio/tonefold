//! Offline render of the session through the built-in soundfont synth to a 16-bit stereo WAV,
//! so a take can be auditioned (or bounced) without a DAW.

use crate::state::{PlaybackBuffer, Shared};
use flvstx_core::model::PPQ;
use std::io::Write;
use std::path::Path;

/// Silence rendered after the last note so releases and reverb tails are not cut off.
const TAIL_SECONDS: f32 = 2.0;
const SAMPLE_RATE: i32 = 44_100;

/// Renders `section` (or the whole song) and writes it to `path`.
/// Returns the length in seconds and the peak level before any limiting.
pub fn render_to_file(shared: &Shared, section: Option<&str>, path: &Path) -> Result<(f32, f32), String> {
    let (session, muted) = {
        let g = shared.lock_store();
        let ui = shared.ui.lock().map(|u| u.clone()).unwrap_or_default();
        (g.session.clone(), ui.muted.clone())
    };
    let soundfont = shared
        .soundfont
        .lock()
        .map_err(|_| "soundfont unavailable".to_string())?
        .clone()
        .ok_or_else(|| "no soundfont loaded yet — see the status next to the volume slider".to_string())?;

    let (left, right) = render(&session, section, &muted, &soundfont)?;

    // Keep the render honest: scale down rather than clip if the mix went over.
    let peak = left.iter().chain(right.iter()).fold(0.0f32, |m, s| m.max(s.abs()));
    let gain = if peak > 0.99 { 0.99 / peak } else { 1.0 };

    write_wav(path, &left, &right, gain)?;
    Ok((left.len() as f32 / SAMPLE_RATE as f32, peak))
}

/// Renders the selection to interleaved-free stereo buffers.
fn render(
    session: &flvstx_core::Session,
    section: Option<&str>,
    muted: &[String],
    soundfont: &std::sync::Arc<rustysynth::SoundFont>,
) -> Result<(Vec<f32>, Vec<f32>), String> {
    let buf = PlaybackBuffer::build(session, section, muted);
    if buf.events.is_empty() {
        return Err("nothing to render (no notes in this selection)".into());
    }

    let mut settings = rustysynth::SynthesizerSettings::new(SAMPLE_RATE);
    settings.enable_reverb_and_chorus = true;
    settings.maximum_polyphony = 96;
    let mut synth = rustysynth::Synthesizer::new(soundfont, &settings).map_err(|e| e.to_string())?;
    for (ch, program) in buf.programs.iter().enumerate() {
        if ch != 9 && *program < 128 {
            synth.process_midi_message(ch as i32, 0xC0, *program as i32, 0);
        }
    }

    // Ticks are laid out at the session tempo; the built-in synth has no tempo of its own.
    let samples_per_tick = 60.0 / (buf.tempo.max(1.0) as f64 * PPQ as f64) * SAMPLE_RATE as f64;
    let total = ((buf.loop_end.saturating_sub(buf.loop_start)) as f64 * samples_per_tick) as usize
        + (TAIL_SECONDS * SAMPLE_RATE as f32) as usize;
    let mut left = vec![0.0f32; total];
    let mut right = vec![0.0f32; total];

    let mut pos = 0usize;
    for e in &buf.events {
        let at = (((e.tick.saturating_sub(buf.loop_start)) as f64 * samples_per_tick) as usize).min(total);
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

/// 16-bit PCM stereo WAV.
fn write_wav(path: &Path, left: &[f32], right: &[f32], gain: f32) -> Result<(), String> {
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
    f.write_all(&out).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flvstx_core::{Clip, Note, Session};

    /// Renders a bar of C major through the installed soundfont. Skipped when it is not present
    /// (the installer downloads it), since there is nothing to render without one.
    #[test]
    fn renders_audible_samples() {
        let path = crate::state::soundfont_path();
        let Ok(mut file) = std::fs::File::open(&path) else {
            eprintln!("skipping: no soundfont at {}", path.display());
            return;
        };
        let sf = std::sync::Arc::new(rustysynth::SoundFont::new(&mut file).expect("soundfont"));

        let mut session = Session::default();
        session.sections.push(flvstx_core::Section::new("verse", "Verse", 1, 0.5));
        let section = session.sections[0].id.clone();
        let notes = vec![Note::new(60, 0, PPQ, 0.9), Note::new(64, PPQ, PPQ, 0.9), Note::new(67, PPQ * 2, PPQ * 2, 0.9)];
        let track = session.tracks[0].id.clone();
        session.track_by_mut(&track).unwrap().clips.insert(section.clone(), Clip::new(notes, flvstx_core::ClipSource::Edited));

        let (left, right) = render(&session, Some(&section), &[], &sf).expect("render");
        assert_eq!(left.len(), right.len());
        let peak = left.iter().chain(right.iter()).fold(0.0f32, |m, s| m.max(s.abs()));
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
