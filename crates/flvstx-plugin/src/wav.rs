//! Offline render of the session through the built-in soundfont synth to a 16-bit stereo WAV,
//! so a take can be auditioned (or bounced) without a DAW.

use crate::state::Shared;
use flvstx_core::render::{peak_and_gain, render_stereo, write_wav, SAMPLE_RATE};
use std::path::Path;

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

    let (left, right) = render_stereo(&session, section, &muted, &soundfont)?;
    // Keep the render honest: scale down rather than clip if the mix went over.
    let (peak, gain) = peak_and_gain(&left, &right);
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
        let Ok((l, r)) = render_stereo(&session, section, &others, &soundfont) else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use flvstx_core::{Clip, Note, Session, PPQ};

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

    /// Controllers have to reach the synth before the notes they shape: a pedal pressed after the
    /// chord it holds does nothing, and a bend applied late bends the wrong note.
    #[test]
    fn controllers_precede_notes_at_the_same_tick() {
        use flvstx_core::model::{AutoPoint, AutoTarget, Automation, Curve};
        use flvstx_core::playback::EventKind;

        let mut session = Session::default();
        session.sections.push(flvstx_core::Section::new("verse", "Verse", 1, 0.5));
        let section = session.sections[0].id.clone();
        let id = session.tracks[0].id.clone();
        let clip = Clip::with_automation(
            vec![Note::new(60, 0, PPQ, 0.9)],
            flvstx_core::ClipSource::Edited,
            vec![Automation::new(AutoTarget::Sustain, vec![AutoPoint { tick: 0, value: 1.0, curve: Curve::Step }])],
        );
        session.track_by_mut(&id).unwrap().clips.insert(section.clone(), clip);

        let buf = crate::state::PlaybackBuffer::build(&session, Some(&section), &[]);
        let first = buf.events.first().expect("events");
        assert!(matches!(first.kind, EventKind::Cc { cc: 64, .. }), "expected the pedal first, got {:?}", first.kind);
        assert!(buf.events.iter().any(|e| matches!(e.kind, EventKind::NoteOn { .. })));
    }
}
