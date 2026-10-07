//! Controller lanes for the generated parts. Notes alone cannot say "swell into the chorus" or
//! "hold the pedal through the change" — on a sustained patch, velocity does nothing once the note
//! has started, so these are what actually make a pad breathe or a piano sound played.

use super::SongCtx;
use crate::model::{AutoPoint, AutoTarget, Automation, Curve, Note, Section, SectionRole, TrackRole, PPQ};

/// Builds the lanes for a freshly generated clip.
pub fn lanes_for(kind: TrackRole, section: &Section, notes: &[Note], ctx: SongCtx, energy: f32, bar: u32) -> Vec<Automation> {
    if notes.is_empty() {
        return Vec::new();
    }
    let total = section.bars * bar;
    match kind {
        TrackRole::Pad | TrackRole::Harmony => vec![swell(section, ctx, energy, total, bar)],
        TrackRole::Chords => pedal(section, notes, total).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// CC11 shaped like a bow arm: in from soft, peak about two thirds through, ease off at the end.
/// A build ramps monotonically instead, so it keeps rising into whatever follows.
fn swell(section: &Section, ctx: SongCtx, energy: f32, total: u32, bar: u32) -> Automation {
    let mut points = Vec::new();
    // Expression scales a sounding note, and synths apply it steeply (roughly squared), so the
    // swell has to live near the top of the range or the part just goes quiet.
    let floor = 0.60 + 0.12 * energy;
    let ceil = 1.0;
    if matches!(ctx.role, SectionRole::Build) {
        let steps = section.bars.max(1);
        for i in 0..=steps {
            let f = i as f32 / steps as f32;
            points.push(AutoPoint { tick: (i * bar).min(total), value: floor + (1.0 - floor) * f, curve: Curve::Linear });
        }
        return Automation::new(AutoTarget::Expression, points);
    }
    // One arc per 4-bar phrase, so a long section keeps moving rather than swelling once.
    let phrase = (bar * 4).min(total.max(1));
    let mut start = 0;
    while start < total {
        let end = (start + phrase).min(total);
        let span = end.saturating_sub(start).max(1);
        points.push(AutoPoint { tick: start, value: floor, curve: Curve::Linear });
        points.push(AutoPoint { tick: start + span * 2 / 3, value: ceil, curve: Curve::Linear });
        points.push(AutoPoint { tick: end.saturating_sub(1), value: floor + (ceil - floor) * 0.35, curve: Curve::Linear });
        start = end;
    }
    Automation::new(AutoTarget::Expression, points)
}

/// CC64 lifted and re-pressed just after each chord change — the gesture that stops a piano part
/// sounding like separate stabs. Only for parts that are actually sustaining.
fn pedal(section: &Section, notes: &[Note], total: u32) -> Option<Automation> {
    let mean_len = notes.iter().map(|n| n.len as u64).sum::<u64>() / notes.len().max(1) as u64;
    if mean_len < (PPQ / 4) as u64 || section.chords.is_empty() {
        return None; // staccato stabs: pedalling them would just smear the part
    }
    let lift = PPQ / 32;
    let mut points = Vec::new();
    for ev in &section.chords {
        if ev.start >= total {
            break;
        }
        // Up an instant before the change, down again just after it.
        if ev.start > lift {
            points.push(AutoPoint { tick: ev.start - lift, value: 0.0, curve: Curve::Step });
        }
        points.push(AutoPoint { tick: ev.start + lift, value: 1.0, curve: Curve::Step });
    }
    points.push(AutoPoint { tick: total.saturating_sub(1), value: 0.0, curve: Curve::Step });
    Some(Automation::new(AutoTarget::Sustain, points))
}
