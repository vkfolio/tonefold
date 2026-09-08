//! Offline render of the session through the built-in soundfont synth to a 16-bit stereo WAV,
//! so a take can be auditioned (or bounced) without a DAW.

use crate::state::{PlaybackBuffer, Shared};
use flvstx_core::model::PPQ;
use std::io::Write;
use std::path::Path;

/// Silence rendered after the last note so releases and reverb tails are not cut off.
const TAIL_SECONDS: f32 = 2.0;
const SAMPLE_RATE: i32 = 44_100;

/// What a render produced: the mix file first, then one file per layer.
pub struct Render {
    pub files: Vec<std::path::PathBuf>,
    pub seconds: f32,
    /// Peak of the mix before it was turned down (above 1.0 means it was).
    pub peak: f32,
}

/// Renders `section` (or the whole song) into `dir`: the mix as `<name>.wav`, plus one
/// `<name>-<layer>.wav` per layer that has notes. Every layer is rendered at the mix's gain, so the
/// stems add back up to the mix.
pub fn render_to_dir(shared: &Shared, section: Option<&str>, dir: &Path) -> Result<Render, String> {
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
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;

    let base = section
        .and_then(|id| session.section(id).map(|s| sanitize(&s.name)))
        .unwrap_or_else(|| "song".to_string());

    let (left, right) = render(&session, section, &muted, &soundfont)?;
    // Keep the render honest: scale down rather than clip if the mix went over.
    let peak = left.iter().chain(right.iter()).fold(0.0f32, |m, s| m.max(s.abs()));
    let gain = if peak > 0.99 { 0.99 / peak } else { 1.0 };
    let seconds = left.len() as f32 / SAMPLE_RATE as f32;

    let mix = dir.join(format!("{base}.wav"));
    write_wav(&mix, &left, &right, gain)?;
    let mut files = vec![mix];

    // One stem per layer: render with every other layer muted.
    for track in &session.tracks {
        if track.muted || muted.contains(&track.id) {
            continue;
        }
        let others: Vec<String> = session.tracks.iter().filter(|t| t.id != track.id).map(|t| t.id.clone()).collect();
        let Ok((l, r)) = render(&session, section, &others, &soundfont) else {
            continue; // no notes for this layer in this range
        };
        let path = dir.join(format!("{base}-{}.wav", sanitize(&track.id)));
        write_wav(&path, &l, &r, gain)?;
        files.push(path);
    }
    Ok(Render { files, seconds, peak })
}

/// File-name safe version of a section or layer name.
fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let s = s.trim_matches('_').to_string();
    if s.is_empty() { "layer".into() } else { s }
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

    /// A mix plus one stem per layer that has notes, all the same length.
    #[test]
    fn writes_mix_and_stems() {
        let path = crate::state::soundfont_path();
        let Ok(mut file) = std::fs::File::open(&path) else {
            eprintln!("skipping: no soundfont at {}", path.display());
            return;
        };
        let sf = std::sync::Arc::new(rustysynth::SoundFont::new(&mut file).expect("soundfont"));

        let mut session = Session::default();
        session.sections.push(flvstx_core::Section::new("verse", "Verse", 1, 0.5));
        let section = session.sections[0].id.clone();
        // Two layers with notes, the rest empty: only those two should get a stem.
        for (idx, pitch) in [(0usize, 48u8), (1usize, 72u8)] {
            let id = session.tracks[idx].id.clone();
            let notes = vec![Note::new(pitch, 0, PPQ * 2, 0.9)];
            session.track_by_mut(&id).unwrap().clips.insert(section.clone(), Clip::new(notes, flvstx_core::ClipSource::Edited));
        }

        let shared = crate::state::Shared::new(session);
        *shared.soundfont.lock().unwrap() = Some(sf);

        let dir = std::env::temp_dir().join("flvstx-stems-test");
        let _ = std::fs::remove_dir_all(&dir);
        let r = render_to_dir(&shared, Some(&section), &dir).expect("render");

        assert_eq!(r.files.len(), 3, "expected a mix and two stems, got {:?}", r.files);
        assert!(r.files[0].file_name().unwrap().to_str().unwrap().starts_with("Verse."));
        let mix_len = std::fs::metadata(&r.files[0]).unwrap().len();
        for stem in &r.files[1..] {
            let len = std::fs::metadata(stem).unwrap().len();
            assert_eq!(len, mix_len, "{} is a different length to the mix", stem.display());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
