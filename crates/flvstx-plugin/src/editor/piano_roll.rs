//! Piano roll editor for the selected track/section: grid, notes, selection, move/resize/draw/delete,
//! velocity lane, chord labels, drum-lane mode, note preview.

use super::{track_color, theme};
use crate::state::Shared;
use flvstx_core::notation::{drum_pitch_lane, drum_lane_pitch};
use flvstx_core::theory::pitch_name;
use flvstx_core::{Note, TrackRole, PPQ};
use nih_plug_egui::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use std::collections::HashSet;
use std::sync::atomic::Ordering;

const KEYS_W: f32 = 62.0;
const HEADER_H: f32 = 44.0;
const VEL_H: f32 = 80.0;
const SCROLL_W: f32 = 16.0;
const DRUM_LANES: [&str; 13] = ["K", "S", "RS", "CL", "H", "PH", "OH", "T1", "T2", "T3", "RD", "CR", "P"];

#[derive(Debug, Clone)]
enum Drag {
    Move { origin: Pos2, orig: Vec<(usize, Note)>, moved: bool },
    Resize { origin: Pos2, orig: Vec<(usize, Note)> },
    Box { origin: Pos2, current: Pos2 },
    Velocity,
    /// Scrubbing the playhead on the ruler.
    Playhead,
    /// Playing the key column like a keyboard.
    Keys { last: u8 },
}

pub struct PianoRollView {
    /// Pixels per beat.
    pub zoom: f32,
    /// Leftmost visible tick.
    pub scroll_x: f32,
    /// Highest visible pitch (melodic mode).
    pub top_pitch: f32,
    pub row_h: f32,
    pub selection: HashSet<usize>,
    drag: Option<Drag>,
    pub snap_div: u32,
    last_track: Option<String>,
    last_section: Option<String>,
    pan_mode: bool,
    pan_origin: Option<(Pos2, f32, f32)>,
}

impl Default for PianoRollView {
    fn default() -> Self {
        PianoRollView { zoom: 60.0, scroll_x: 0.0, top_pitch: 84.0, row_h: 11.0, selection: HashSet::new(), drag: None, snap_div: 16, last_track: None, last_section: None, pan_mode: false, pan_origin: None }
    }
}

fn snap(tick: f32, div: u32) -> u32 {
    let step = (PPQ * 4 / div).max(1) as f32;
    ((tick / step).round() * step).max(0.0) as u32
}

pub fn show(ui: &mut egui::Ui, view: &mut PianoRollView, shared: &Shared) {
    let ui_state = shared.ui.lock().map(|u| u.clone()).unwrap_or_default();
    let track_id = ui_state.selected_track.clone();
    let (section, key, bar_ticks, notes, role, track_name, active) = {
        let g = shared.lock_store();
        let sec = ui_state.selected_section.as_ref().and_then(|id| g.session.section(id).cloned());
        let t = g.session.track_by(&track_id).cloned();
        let notes = sec.as_ref().and_then(|s| g.session.clip(&track_id, &s.id).map(|c| c.notes.clone())).unwrap_or_default();
        let role = t.as_ref().map(|t| t.kind).unwrap_or(TrackRole::Melody);
        let active = match (&t, &sec) { (Some(t), Some(s)) => t.active_in(&s.id), _ => true };
        (sec, g.session.key, g.session.bar_ticks(), notes, role, t.map(|t| t.name).unwrap_or_default(), active)
    };
    let Some(section) = section else {
        ui.centered_and_justified(|ui| ui.label("Add a section (top bar) or ask the composer for a song to get started."));
        return;
    };
    // Reset view when switching context.
    let context_changed = view.last_track.as_deref() != Some(track_id.as_str()) || view.last_section.as_deref() != Some(section.id.as_str());
    if context_changed {
        view.selection.clear();
        view.drag = None;
        view.pan_origin = None;
        view.last_track = Some(track_id.clone());
        view.last_section = Some(section.id.clone());
        let (lo, hi) = role.register();
        view.top_pitch = (hi + 5) as f32;
        let _ = lo;
        view.scroll_x = 0.0;
    }
    let drums = !role.is_pitched();
    let total_ticks = section.bars * bar_ticks;
    let color = track_color(role);
    let role = track_id.as_str();

    // Measure before toolbar controls consume the available width.
    let roll_width = ui.available_width();
    let mut fit = context_changed;
    ui.add_space(5.0);
    // Toolbar.
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(format!("{} · {}{}", track_name, section.name, if active { "" } else { " (silent here)" })).strong().color(color));
        // Built-in instrument for this layer.
        if !drums {
            let (program, style) = {
                let g = shared.lock_store();
                let t = g.session.track_by(&track_id).cloned();
                (t.map(|t| t.program(&g.session.style)).unwrap_or(0), g.session.style.clone())
            };
            let _ = style;
            let mut chosen = program;
            egui::ComboBox::from_id_salt("instrument").width(170.0).selected_text(flvstx_core::gm::program_name(program)).show_ui(ui, |ui| {
                for (i, name) in flvstx_core::gm::GM_PROGRAMS.iter().enumerate() {
                    ui.selectable_value(&mut chosen, i as u8, format!("{i:>3} {name}"));
                }
            });
            if chosen != program {
                let mut g = shared.lock_store();
                let tid = track_id.clone();
                let _ = g.mutate(|s| {
                    if let Some(t) = s.track_by_mut(&tid) {
                        t.instrument = Some(chosen);
                    }
                    Ok(())
                });
            }
        } else {
            ui.label(egui::RichText::new("Drum Kit").small().weak());
        }
        ui.label("snap");
        egui::ComboBox::from_id_salt("snap").width(56.0).selected_text(format!("1/{}", view.snap_div)).show_ui(ui, |ui| {
            for d in [4u32, 8, 16, 32, 12, 24] {
                ui.selectable_value(&mut view.snap_div, d, format!("1/{d}"));
            }
        });
        ui.label("Time");
        if ui.small_button("-").on_hover_text("Zoom out horizontally").clicked() {
            view.zoom = (view.zoom / 1.25).max(8.0);
        }
        if ui.small_button("+").on_hover_text("Zoom in horizontally").clicked() {
            view.zoom = (view.zoom * 1.25).min(400.0);
        }
        if !drums {
            ui.label("Pitch");
            if ui.small_button("-").on_hover_text("Show more pitches").clicked() { view.row_h = (view.row_h / 1.25).max(8.0); }
            if ui.small_button("+").on_hover_text("Make pitch rows taller").clicked() { view.row_h = (view.row_h * 1.25).min(48.0); }
        }
        if ui.toggle_value(&mut view.pan_mode, "Pan").on_hover_text("Drag to pan without editing notes. Middle-mouse drag also pans in Edit mode.").changed() {
            view.drag = None;
            view.pan_origin = None;
        }
        if ui.small_button("Fit notes").clicked() {
            fit = true;
            view.zoom = ((roll_width - KEYS_W) / (total_ticks as f32 / PPQ as f32)).clamp(8.0, 400.0);
            view.scroll_x = 0.0;
        }
        ui.label(egui::RichText::new(format!("{} notes", notes.len())).small().weak());
        ui.menu_button("Help", |ui| {
            ui.label("Double-click to add a note");
            ui.label("Drag to move; drag the right edge to resize");
            ui.label("Right-click or Delete to remove notes");
            ui.label("Up / Down: transpose selected notes");
            ui.label("Ctrl+A: select all / Ctrl+D: duplicate");
            ui.label("Scroll: pitch / Shift+scroll: time");
            ui.label("Scroll on the ruler: time");
            ui.label("Ctrl+scroll: time zoom / Ctrl+Shift+scroll: pitch zoom");
            ui.label("Pan button + drag, or middle-mouse drag: pan both axes");
            ui.label("Right scrollbar: pitch / bottom scrollbar: time");
            ui.label("Drag the ruler to seek; click keys to audition");
        });
    });

    let avail = ui.available_size();
    if avail.x <= KEYS_W + SCROLL_W + 20.0 || avail.y <= HEADER_H + VEL_H + SCROLL_W + 20.0 {
        ui.label("Enlarge the editor or hide Composer to see the piano roll.");
        return;
    }
    let (outer, _) = ui.allocate_exact_size(avail, Sense::hover());
    let rect = Rect::from_min_max(outer.min, outer.max - Vec2::splat(SCROLL_W));
    let resp = ui.interact(rect, ui.id().with("note-canvas"), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::CANVAS);
    let grid = Rect::from_min_max(Pos2::new(rect.left() + KEYS_W, rect.top() + HEADER_H), Pos2::new(rect.right(), rect.bottom() - VEL_H));
    let vel_rect = Rect::from_min_max(Pos2::new(grid.left(), grid.bottom()), rect.max);
    if fit {
        view.zoom = ((grid.width() - 12.0) / (total_ticks as f32 / PPQ as f32).max(1.0)).clamp(8.0, 400.0);
        view.scroll_x = 0.0;
        if !drums {
            let lo = notes.iter().map(|n| n.pitch).min().unwrap_or(48) as f32;
            let hi = notes.iter().map(|n| n.pitch).max().unwrap_or(72) as f32;
            let span = (hi - lo + 12.0).max(24.0);
            view.row_h = (grid.height() / span).clamp(14.0, 30.0);
            let visible = grid.height() / view.row_h;
            view.top_pitch = ((lo + hi + visible) / 2.0).ceil().clamp(24.0, 127.0);
        }
    }
    // Navigation is applied before drawing so keys, notes, and hit testing stay aligned.
    if resp.hovered() {
        let (scroll, zoom_delta, mods) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers));
        let p = resp.hover_pos().unwrap_or(grid.center());
        if mods.ctrl && (mods.shift || p.x < grid.left()) && !drums && scroll.y != 0.0 {
            let pitch = view.top_pitch - (p.y - grid.top()) / view.row_h;
            view.row_h = (view.row_h * (scroll.y * 0.002).exp()).clamp(8.0, 48.0);
            view.top_pitch = pitch + (p.y - grid.top()) / view.row_h;
        } else if zoom_delta != 1.0 || (mods.ctrl && scroll.y != 0.0) {
            let z = if zoom_delta != 1.0 { zoom_delta } else { (scroll.y * 0.002).exp() };
            let tick_at = view.scroll_x + (p.x - grid.left()) / view.zoom * PPQ as f32;
            view.zoom = (view.zoom * z).clamp(8.0, 400.0);
            view.scroll_x = tick_at - (p.x - grid.left()) / view.zoom * PPQ as f32;
        } else if mods.shift || p.y < grid.top() {
            view.scroll_x -= (scroll.x + scroll.y) / view.zoom * PPQ as f32;
        } else {
            view.scroll_x -= scroll.x / view.zoom * PPQ as f32;
            if !drums { view.top_pitch += scroll.y / view.row_h; }
        }
    }
    let (pointer_pos, middle, primary, pressed) = ui.input(|i| (
        i.pointer.interact_pos(), i.pointer.button_down(egui::PointerButton::Middle),
        i.pointer.primary_down(), i.pointer.button_pressed(egui::PointerButton::Middle) || (view.pan_mode && i.pointer.primary_pressed()),
    ));
    if pressed && pointer_pos.is_some_and(|p| rect.contains(p)) {
        view.pan_origin = pointer_pos.map(|p| (p, view.scroll_x, view.top_pitch));
        view.drag = None;
    }
    let navigation_active = view.pan_origin.is_some();
    if let (Some((origin, x, pitch)), Some(p)) = (view.pan_origin, pointer_pos) {
        if middle || (view.pan_mode && primary) {
            view.scroll_x = x - (p.x - origin.x) / view.zoom * PPQ as f32;
            if !drums { view.top_pitch = pitch + (p.y - origin.y) / view.row_h; }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else { view.pan_origin = None; }
    } else if view.pan_mode && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    let visible_ticks = grid.width() / view.zoom * PPQ as f32;
    let max_x = (total_ticks as f32 - visible_ticks).max(0.0);
    view.scroll_x = view.scroll_x.clamp(0.0, max_x);
    let visible_rows = grid.height() / view.row_h;
    let min_top = (visible_rows - 1.0).clamp(0.0, 127.0);
    view.top_pitch = view.top_pitch.clamp(min_top, 127.0);
    scrollbar(ui, "time-scroll", Rect::from_min_max(Pos2::new(grid.left(), rect.bottom() + 2.0), outer.max),
        false, visible_ticks / (total_ticks as f32).max(1.0), &mut view.scroll_x, max_x);
    if !drums {
        let mut from_top = 127.0 - view.top_pitch;
        scrollbar(ui, "pitch-scroll", Rect::from_min_max(Pos2::new(rect.right() + 2.0, grid.top()), Pos2::new(outer.right(), grid.bottom())),
            true, visible_rows / 128.0, &mut from_top, 127.0 - min_top);
        view.top_pitch = 127.0 - from_top;
    }
    let row_h = if drums { (grid.height() / DRUM_LANES.len() as f32).clamp(12.0, 28.0) } else { view.row_h };

    let tick_to_x = |t: f32| grid.left() + (t - view.scroll_x) / PPQ as f32 * view.zoom;
    let x_to_tick = |x: f32| view.scroll_x + (x - grid.left()) / view.zoom * PPQ as f32;
    let lane_of = |pitch: u8| -> usize { DRUM_LANES.iter().position(|l| *l == drum_pitch_lane(pitch)).unwrap_or(DRUM_LANES.len() - 1) };
    let pitch_to_y = |p: u8| -> f32 { if drums { grid.top() + lane_of(p) as f32 * row_h } else { grid.top() + (view.top_pitch - p as f32) * row_h } };
    let y_to_pitch = |y: f32| -> u8 {
        if drums {
            let lane = (((y - grid.top()) / row_h).floor().max(0.0) as usize).min(DRUM_LANES.len() - 1);
            drum_lane_pitch(DRUM_LANES[lane]).unwrap_or(36)
        } else {
            (view.top_pitch - (y - grid.top()) / row_h).floor().clamp(0.0, 127.0) as u8
        }
    };

    // Pitches this layer already uses, and the key held down in the key column.
    let used: HashSet<u8> = notes.iter().map(|n| n.pitch).collect();
    let pressed_key = match &view.drag {
        Some(Drag::Keys { last }) => Some(*last),
        _ => None,
    };

    // Clip rows to the keyboard/grid so panning cannot paint over the ruler or velocity lane.
    {
    let painter = painter.with_clip_rect(Rect::from_min_max(Pos2::new(rect.left(), grid.top()), grid.max));
    // Grid rows.
    if drums {
        for (i, lane) in DRUM_LANES.iter().enumerate() {
            let y = grid.top() + i as f32 * row_h;
            let r = Rect::from_min_size(Pos2::new(grid.left(), y), Vec2::new(grid.width(), row_h));
            painter.rect_filled(r, 0.0, if i % 2 == 0 { Color32::from_rgb(30, 32, 37) } else { Color32::from_rgb(27, 29, 33) });
            let lane_pitch = drum_lane_pitch(lane).unwrap_or(36);
            let held = pressed_key == Some(lane_pitch);
            let lr = Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(KEYS_W - 2.0, row_h - 1.0));
            painter.rect_filled(lr, 2.0, if held { color } else { Color32::from_rgb(38, 40, 46) });
            painter.text(Pos2::new(rect.left() + 4.0, y + row_h / 2.0), Align2::LEFT_CENTER, *lane, FontId::monospace(12.0), if held { Color32::from_rgb(20, 20, 20) } else { Color32::from_rgb(200, 200, 200) });
            if used.contains(&lane_pitch) {
                painter.circle_filled(Pos2::new(rect.left() + KEYS_W - 7.0, y + row_h / 2.0), 2.5, color);
            }
        }
    } else {
        let top_p = view.top_pitch.ceil() as i32;
        let rows = (grid.height() / row_h) as i32 + 2;
        for i in 0..rows {
            let p = top_p - i;
            if !(0..=127).contains(&p) {
                continue;
            }
            let y = grid.top() + (view.top_pitch - p as f32) * row_h;
            let r = Rect::from_min_size(Pos2::new(grid.left(), y), Vec2::new(grid.width(), row_h));
            let pc = (p % 12) as u8;
            let black = matches!(pc, 1 | 3 | 6 | 8 | 10);
            let in_key = key.contains(p as u8);
            let fill = if !in_key { Color32::from_rgb(20, 26, 34) } else if black { Color32::from_rgb(23, 30, 39) } else { Color32::from_rgb(28, 36, 46) };
            painter.rect_filled(r, 0.0, fill);
            if pc == key.root {
                painter.line_segment([Pos2::new(grid.left(), y + row_h), Pos2::new(grid.right(), y + row_h)], Stroke::new(1.0, Color32::from_rgb(60, 66, 80)));
            }
            // Keys column: click to hear the note; keys used by this layer are marked.
            let kr = Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(KEYS_W - 2.0, row_h - 1.0));
            let held = pressed_key == Some(p as u8);
            let fill = if held {
                color
            } else if black {
                Color32::from_rgb(40, 40, 44)
            } else {
                Color32::from_rgb(155, 174, 191)
            };
            painter.rect_filled(kr, 2.0, fill);
            let ink = if held { Color32::from_rgb(20, 20, 20) } else if black { Color32::from_rgb(190, 190, 195) } else { Color32::from_rgb(40, 40, 40) };
            if pc == 0 || used.contains(&(p as u8)) {
                painter.text(Pos2::new(rect.left() + 3.0, y + row_h / 2.0), Align2::LEFT_CENTER, pitch_name(p as u8), FontId::monospace(12.0), ink);
            }
            if used.contains(&(p as u8)) {
                painter.circle_filled(Pos2::new(rect.left() + KEYS_W - 7.0, y + row_h / 2.0), (row_h * 0.16).clamp(1.5, 3.0), color);
            }
        }
    }
    }
    let painter = painter.with_clip_rect(Rect::from_min_max(Pos2::new(grid.left(), rect.top()), rect.max));
    // Beat / bar lines + header.
    let first_beat = (view.scroll_x / PPQ as f32).floor() as u32;
    let beats_per_bar = bar_ticks / PPQ;
    let last_tick = x_to_tick(grid.right());
    let mut t = first_beat * PPQ;
    while (t as f32) <= last_tick && t <= total_ticks {
        let x = tick_to_x(t as f32);
        let is_bar = t % bar_ticks == 0;
        painter.line_segment([Pos2::new(x, grid.top()), Pos2::new(x, grid.bottom())], Stroke::new(if is_bar { 1.5 } else { 0.6 }, if is_bar { Color32::from_rgb(67, 85, 103) } else { Color32::from_rgb(52, 55, 62) }));
        if is_bar {
            painter.text(Pos2::new(x + 3.0, rect.top() + 3.0), Align2::LEFT_TOP, format!("{}", t / bar_ticks + 1), FontId::proportional(14.0), Color32::from_rgb(170, 175, 185));
        }
        // 16th subdivisions when zoomed in.
        if view.zoom > 90.0 {
            for k in 1..4 {
                let xs = tick_to_x((t + k * PPQ / 4) as f32);
                painter.line_segment([Pos2::new(xs, grid.top()), Pos2::new(xs, grid.bottom())], Stroke::new(0.4, Color32::from_rgb(40, 42, 48)));
            }
        }
        t += PPQ;
    }
    let _ = beats_per_bar;
    // Section end shade.
    let end_x = tick_to_x(total_ticks as f32);
    if end_x < grid.right() {
        painter.rect_filled(Rect::from_min_max(Pos2::new(end_x, grid.top()), grid.max), 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 90));
    }
    // Chord labels in the header.
    for ev in &section.chords {
        let x = tick_to_x(ev.start as f32);
        if x >= grid.left() - 40.0 && x <= grid.right() {
            painter.text(Pos2::new(x + 3.0, rect.top() + HEADER_H - 2.0), Align2::LEFT_BOTTOM, ev.chord.symbol(), FontId::proportional(14.0), track_color(TrackRole::Chords));
        }
    }
    // Playhead: always drawn, so it can be dragged on the ruler while stopped.
    let ph = shared.playhead_tick.load(Ordering::Relaxed);
    let sec_start = shared.lock_store().session.section_start(&section.id).unwrap_or(0);
    let rolling = shared.playing.load(Ordering::Relaxed) || shared.host_playing.load(Ordering::Relaxed);
    if ph >= sec_start && ph < sec_start + total_ticks {
        let x = tick_to_x((ph - sec_start) as f32);
        let c = if rolling { Color32::from_rgb(255, 230, 120) } else { Color32::from_rgb(150, 140, 90) };
        painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.5, c));
        // Handle in the ruler, so it reads as draggable.
        let h = 6.0;
        painter.add(egui::Shape::convex_polygon(
            vec![Pos2::new(x - h, rect.top()), Pos2::new(x + h, rect.top()), Pos2::new(x, rect.top() + h * 1.4)],
            c,
            Stroke::NONE,
        ));
    }

    // Working copy of notes with drag preview applied.
    let mut display: Vec<Note> = notes.clone();
    let mods = ui.input(|i| i.modifiers);
    let snap_div = if mods.alt { 0 } else { view.snap_div };
    let apply_snap = |t: f32| if snap_div == 0 { t.max(0.0) as u32 } else { snap(t, snap_div) };
    if let Some(Drag::Move { origin, orig, .. }) = &view.drag {
        if let Some(p) = resp.interact_pointer_pos() {
            let dt = (p.x - origin.x) / view.zoom * PPQ as f32;
            let dp = if drums { 0 } else { -((p.y - origin.y) / row_h).round() as i32 };
            for (i, n) in orig {
                if let Some(d) = display.get_mut(*i) {
                    d.start = apply_snap(n.start as f32 + dt);
                    d.pitch = (n.pitch as i32 + dp).clamp(0, 127) as u8;
                    if drums {
                        d.pitch = y_to_pitch(pitch_to_y(n.pitch) + (p.y - origin.y) + row_h / 2.0);
                    }
                }
            }
        }
    }
    if let Some(Drag::Resize { origin, orig }) = &view.drag {
        if let Some(p) = resp.interact_pointer_pos() {
            let dt = (p.x - origin.x) / view.zoom * PPQ as f32;
            for (i, n) in orig {
                if let Some(d) = display.get_mut(*i) {
                    let end = apply_snap(n.end() as f32 + dt).max(n.start + PPQ / 32);
                    d.len = end - n.start;
                }
            }
        }
    }

    // Draw notes.
    let hit_note = |pos: Pos2, list: &[Note]| -> Option<(usize, bool)> {
        for (i, n) in list.iter().enumerate().rev() {
            let x0 = tick_to_x(n.start as f32);
            let x1 = tick_to_x(n.end() as f32).max(x0 + 4.0);
            let y = pitch_to_y(n.pitch);
            let r = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + row_h));
            if r.contains(pos) {
                return Some((i, pos.x > x1 - 6.0 && x1 - x0 > 10.0));
            }
        }
        None
    };
    for (i, n) in display.iter().enumerate() {
        let x0 = tick_to_x(n.start as f32);
        let x1 = tick_to_x(n.end() as f32).max(x0 + 4.0);
        let y = pitch_to_y(n.pitch);
        if x1 < grid.left() || x0 > grid.right() || y + row_h < grid.top() || y > grid.bottom() {
            continue;
        }
        let r = Rect::from_min_max(Pos2::new(x0.max(grid.left()), y.max(grid.top())), Pos2::new(x1.min(grid.right()), (y + row_h - 1.0).min(grid.bottom())));
        let sel = view.selection.contains(&i);
        let a = (90.0 + n.vel * 165.0) as u8;
        let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a);
        painter.rect_filled(r, 4.0, fill);
        if r.width() > 42.0 && row_h >= 12.0 && n.lyric.is_none() {
            painter.text(egui::pos2(r.left() + 6.0, r.center().y), Align2::LEFT_CENTER, pitch_name(n.pitch), FontId::proportional(11.0), theme::CANVAS);
        }
        painter.rect_stroke(r, 2.0, Stroke::new(if sel { 2.0 } else { 1.0 }, if sel { Color32::WHITE } else { Color32::from_rgba_unmultiplied(0, 0, 0, 160) }), StrokeKind::Inside);
        if let Some(l) = &n.lyric {
            if r.width() > 14.0 {
                painter.text(Pos2::new(r.left() + 2.0, r.center().y), Align2::LEFT_CENTER, l, FontId::proportional(9.0), Color32::BLACK);
            }
        }
        // Velocity bar.
        let vx = tick_to_x(n.start as f32);
        if vx >= grid.left() && vx <= grid.right() {
            let h = n.vel * (VEL_H - 8.0);
            let vr = Rect::from_min_max(Pos2::new(vx, vel_rect.bottom() - 4.0 - h), Pos2::new(vx + 4.0, vel_rect.bottom() - 4.0));
            painter.rect_filled(vr, 1.0, if sel { Color32::WHITE } else { fill });
        }
    }
    painter.line_segment([Pos2::new(grid.left(), grid.bottom()), Pos2::new(grid.right(), grid.bottom())], Stroke::new(1.0, Color32::from_rgb(70, 74, 84)));
    ui.painter_at(rect).text(Pos2::new(rect.left() + 4.0, vel_rect.center().y), Align2::LEFT_CENTER, "VEL", FontId::proportional(14.0), Color32::from_rgb(140, 140, 150));

    // Box selection preview.
    if let Some(Drag::Box { origin, current }) = &view.drag {
        let r = Rect::from_two_pos(*origin, *current);
        painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(120, 160, 255, 30));
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(120, 160, 255)), StrokeKind::Inside);
    }

    // Pan gestures must never select, audition, move, or create notes.
    if !view.pan_mode && !navigation_active {
    // ---- Interaction ----
    let pointer = resp.interact_pointer_pos();
    let in_grid = pointer.map(|p| grid.contains(p)).unwrap_or(false);
    let in_vel = pointer.map(|p| vel_rect.contains(p)).unwrap_or(false);
    // The ruler (bar numbers / chord labels) scrubs the playhead; the key column plays notes.
    let ruler = Rect::from_min_max(Pos2::new(grid.left(), rect.top()), Pos2::new(grid.right(), grid.top()));
    let keys_rect = Rect::from_min_max(Pos2::new(rect.left(), grid.top()), Pos2::new(grid.left(), grid.bottom()));
    if let Some(p) = resp.hover_pos() {
        if ruler.contains(p) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        } else if keys_rect.contains(p) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    let seek_to_x = |x: f32| {
        let tick = x_to_tick(x).clamp(0.0, total_ticks.saturating_sub(1) as f32) as u32;
        shared.seek_to(sec_start + tick);
    };
    if ui.input(|i| i.pointer.primary_pressed()) {
        if let Some(p) = pointer {
            if ruler.contains(p) {
                view.drag = Some(Drag::Playhead);
                seek_to_x(p.x);
            } else if keys_rect.contains(p) {
                let pitch = y_to_pitch(p.y);
                preview(shared, role, pitch, 0.8);
                view.drag = Some(Drag::Keys { last: pitch });
            }
        }
    }
    let on_side = matches!(view.drag, Some(Drag::Playhead) | Some(Drag::Keys { .. }));
    if ui.input(|i| i.pointer.primary_released()) && on_side {
        // A click without movement never reports a drag stop, so let go here.
        view.drag = None;
    }

    if resp.drag_started() && !on_side {
        if let Some(p) = pointer {
            if in_vel {
                view.drag = Some(Drag::Velocity);
            } else if in_grid {
                match hit_note(p, &notes) {
                    Some((i, on_edge)) => {
                        if !view.selection.contains(&i) {
                            if !mods.ctrl {
                                view.selection.clear();
                            }
                            view.selection.insert(i);
                        }
                        let orig: Vec<(usize, Note)> = view.selection.iter().filter_map(|&j| notes.get(j).map(|n| (j, n.clone()))).collect();
                        view.drag = Some(if on_edge { Drag::Resize { origin: p, orig } } else { Drag::Move { origin: p, orig, moved: false } });
                        preview(shared, role, notes[i].pitch, notes[i].vel);
                    }
                    None => {
                        if !mods.ctrl {
                            view.selection.clear();
                        }
                        view.drag = Some(Drag::Box { origin: p, current: p });
                    }
                }
            }
        }
    }
    if resp.dragged() {
        if let (Some(p), Some(d)) = (pointer, view.drag.as_mut()) {
            match d {
                Drag::Playhead => seek_to_x(p.x),
                Drag::Keys { last } => {
                    let pitch = y_to_pitch(p.y);
                    if pitch != *last {
                        *last = pitch;
                        preview(shared, role, pitch, 0.8);
                    }
                }
                Drag::Box { current, .. } => *current = p,
                Drag::Move { moved, .. } => *moved = true,
                Drag::Velocity => {
                    // Set velocity of notes under the pointer's x (selected ones if any).
                    let v = ((vel_rect.bottom() - 4.0 - p.y) / (VEL_H - 8.0)).clamp(0.05, 1.0);
                    let tick = x_to_tick(p.x);
                    let targets: Vec<usize> = if !view.selection.is_empty() {
                        view.selection.iter().copied().collect()
                    } else {
                        notes.iter().enumerate().filter(|(_, n)| (tick_to_x(n.start as f32) - p.x).abs() < 5.0).map(|(i, _)| i).collect()
                    };
                    let _ = tick;
                    if !targets.is_empty() {
                        shared.edit_notes(role, &section.id, |ns| {
                            for i in &targets {
                                if let Some(n) = ns.get_mut(*i) {
                                    n.vel = v;
                                }
                            }
                        });
                    }
                }
                _ => {}
            }
        }
    }
    if resp.drag_stopped() {
        if let Some(d) = view.drag.take() {
            match d {
                Drag::Move { moved: true, .. } | Drag::Resize { .. } => {
                    let changed: Vec<(usize, Note)> = view.selection.iter().filter_map(|&i| display.get(i).map(|n| (i, n.clone()))).collect();
                    let keys: Vec<(u8, u32, u32)> = changed.iter().map(|(_, n)| (n.pitch, n.start, n.len)).collect();
                    shared.edit_notes(role, &section.id, |ns| {
                        for (i, n) in &changed {
                            if let Some(t) = ns.get_mut(*i) {
                                *t = n.clone();
                            }
                        }
                    });
                    // Re-select by identity after the clip was re-sorted.
                    let g = shared.lock_store();
                    if let Some(c) = g.session.clip(role, &section.id) {
                        view.selection = c.notes.iter().enumerate().filter(|(_, n)| keys.contains(&(n.pitch, n.start, n.len))).map(|(i, _)| i).collect();
                    }
                }
                Drag::Box { origin, current } => {
                    let r = Rect::from_two_pos(origin, current);
                    for (i, n) in notes.iter().enumerate() {
                        let x0 = tick_to_x(n.start as f32);
                        let x1 = tick_to_x(n.end() as f32);
                        let y = pitch_to_y(n.pitch);
                        if r.intersects(Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + row_h))) {
                            view.selection.insert(i);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    if resp.clicked() && in_grid && view.drag.is_none() {
        if let Some(p) = pointer {
            match hit_note(p, &notes) {
                Some((i, _)) => {
                    if mods.ctrl {
                        if !view.selection.remove(&i) {
                            view.selection.insert(i);
                        }
                    } else {
                        view.selection.clear();
                        view.selection.insert(i);
                    }
                    preview(shared, role, notes[i].pitch, notes[i].vel);
                }
                None => view.selection.clear(),
            }
        }
    }
    if resp.double_clicked() && in_grid {
        if let Some(p) = pointer {
            if hit_note(p, &notes).is_none() {
                let start = apply_snap(x_to_tick(p.x).max(0.0));
                let pitch = y_to_pitch(p.y);
                let len = if drums { PPQ / 4 } else { (PPQ * 4 / view.snap_div.max(1)).max(PPQ / 8) };
                if start < total_ticks {
                    shared.edit_notes(role, &section.id, |ns| ns.push(Note::new(pitch, start, len.min(total_ticks - start), 0.8)));
                    preview(shared, role, pitch, 0.8);
                    view.selection.clear();
                }
            }
        }
    }
    if resp.secondary_clicked() && in_grid {
        if let Some(p) = pointer {
            if let Some((i, _)) = hit_note(p, &notes) {
                let mut del: HashSet<usize> = if view.selection.contains(&i) { view.selection.clone() } else { HashSet::new() };
                del.insert(i);
                shared.edit_notes(role, &section.id, |ns| {
                    let mut idx = 0;
                    ns.retain(|_| {
                        let keep = !del.contains(&idx);
                        idx += 1;
                        keep
                    });
                });
                view.selection.clear();
            }
        }
    }
    }
    // Keyboard.
    if resp.hovered() || resp.has_focus() || !view.selection.is_empty() {
        let (del, up, down, sel_all, dup, ctrl, shift) = ui.input(|i| (i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace), i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown), i.modifiers.ctrl && i.key_pressed(egui::Key::A), i.modifiers.ctrl && i.key_pressed(egui::Key::D), i.modifiers.ctrl, i.modifiers.shift));
        let wants_text = ui.ctx().wants_keyboard_input() && !resp.hovered();
        if !wants_text {
            if del && !view.selection.is_empty() {
                let sel = view.selection.clone();
                shared.edit_notes(role, &section.id, |ns| {
                    let mut idx = 0;
                    ns.retain(|_| {
                        let keep = !sel.contains(&idx);
                        idx += 1;
                        keep
                    });
                });
                view.selection.clear();
            }
            if (up || down) && !view.selection.is_empty() && !drums {
                let delta: i32 = if up { 1 } else { -1 } * if shift { 12 } else { 1 };
                let sel = view.selection.clone();
                let mut keys = Vec::new();
                shared.edit_notes(role, &section.id, |ns| {
                    for i in &sel {
                        if let Some(n) = ns.get_mut(*i) {
                            n.pitch = (n.pitch as i32 + delta).clamp(0, 127) as u8;
                            keys.push((n.pitch, n.start, n.len));
                        }
                    }
                });
                let g = shared.lock_store();
                if let Some(c) = g.session.clip(role, &section.id) {
                    view.selection = c.notes.iter().enumerate().filter(|(_, n)| keys.contains(&(n.pitch, n.start, n.len))).map(|(i, _)| i).collect();
                    if let Some(&i) = view.selection.iter().next() {
                        preview(shared, role, c.notes[i].pitch, c.notes[i].vel);
                    }
                }
            }
            if sel_all && resp.hovered() {
                view.selection = (0..notes.len()).collect();
            }
            if dup && !view.selection.is_empty() {
                // Duplicate selection right after itself.
                let sel: Vec<Note> = view.selection.iter().filter_map(|&i| notes.get(i).cloned()).collect();
                let start = sel.iter().map(|n| n.start).min().unwrap_or(0);
                let end = sel.iter().map(|n| n.end()).max().unwrap_or(0);
                let span = snap((end - start) as f32, view.snap_div.max(4)).max(PPQ / 4);
                let mut keys = Vec::new();
                shared.edit_notes(role, &section.id, |ns| {
                    for n in &sel {
                        let mut m = n.clone();
                        m.start += span;
                        if m.start < total_ticks {
                            keys.push((m.pitch, m.start, m.len));
                            ns.push(m);
                        }
                    }
                });
                let g = shared.lock_store();
                if let Some(c) = g.session.clip(role, &section.id) {
                    view.selection = c.notes.iter().enumerate().filter(|(_, n)| keys.contains(&(n.pitch, n.start, n.len))).map(|(i, _)| i).collect();
                }
            }
            let _ = ctrl;
        }
    }
}

fn preview(shared: &Shared, track: &str, pitch: u8, vel: f32) {
    let v = ((vel.clamp(0.05, 1.0) * 127.0) as u8).max(1);
    let ch = shared.lock_store().session.track_by(track).map(|t| t.channel).unwrap_or(0);
    shared.request_preview(ch, pitch, v);
}

fn scrollbar(ui: &mut egui::Ui, salt: &str, rect: Rect, vertical: bool, visible: f32, value: &mut f32, max: f32) {
    let id = ui.id().with(salt);
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let start = if vertical { rect.top() } else { rect.left() };
    let length = if vertical { rect.height() } else { rect.width() };
    let thumb_len = (length * visible.clamp(0.0, 1.0)).max(24.0).min(length);
    let travel = (length - thumb_len).max(0.0);
    let thumb_start = start + if max > 0.0 { *value / max * travel } else { 0.0 };
    if let Some(p) = response.interact_pointer_pos() {
        let axis = if vertical { p.y } else { p.x };
        if ui.input(|i| i.pointer.primary_pressed()) {
            let grab = if (thumb_start..=thumb_start + thumb_len).contains(&axis) { axis - thumb_start } else { thumb_len / 2.0 };
            ui.data_mut(|d| d.insert_temp(id.with("grab"), grab));
        }
        if (response.dragged() || response.clicked()) && travel > 0.0 && max > 0.0 {
            let grab = ui.data(|d| d.get_temp::<f32>(id.with("grab"))).unwrap_or(thumb_len / 2.0);
            *value = ((axis - start - grab) / travel).clamp(0.0, 1.0) * max;
        }
    }
    ui.painter().rect_filled(rect, 5.0, theme::SURFACE);
    let offset = if max > 0.0 { *value / max * travel } else { 0.0 };
    let thumb = if vertical {
        Rect::from_min_size(Pos2::new(rect.left(), start + offset), Vec2::new(rect.width(), thumb_len))
    } else {
        Rect::from_min_size(Pos2::new(start + offset, rect.top()), Vec2::new(thumb_len, rect.height()))
    };
    ui.painter().rect_filled(thumb.shrink(2.0), 4.0, if response.dragged() { theme::ACCENT } else if response.hovered() { theme::MUTED } else { theme::BORDER });
    response.on_hover_text(if vertical { "Drag to scroll through pitches" } else { "Drag to scroll through time. Zoom in to reveal more scroll range." });
}

#[cfg(test)]
mod navigation_tests {
    use super::*;

    fn frame(ctx: &egui::Context, view: &mut PianoRollView, shared: &Shared, events: Vec<egui::Event>) {
        let _ = ctx.run(egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1100.0, 700.0))),
            events, ..Default::default()
        }, |ctx| { egui::CentralPanel::default().show(ctx, |ui| show(ui, view, shared)); });
    }

    #[test]
    fn pan_gestures_move_both_axes_without_editing_notes() {
        for button in [egui::PointerButton::Primary, egui::PointerButton::Middle] {
            let ctx = egui::Context::default();
            let mut session = flvstx_core::Session::default();
            let section = session.add_section("Verse", 16, 0.6);
            let shared = Shared::new(session);
            shared.ui.lock().unwrap().selected_section = Some(section.clone());
            shared.edit_notes("melody", &section, |notes| notes.push(Note::new(60, PPQ, PPQ, 0.8)));
            let mut view = PianoRollView::default();
            for _ in 0..3 { frame(&ctx, &mut view, &shared, vec![]); }
            view.zoom = 200.0;
            view.scroll_x = 2000.0;
            view.top_pitch = 90.0;
            view.row_h = 20.0;
            view.pan_mode = button == egui::PointerButton::Primary;
            let revision = shared.lock_store().revision;
            let before = shared.lock_store().session.clip("melody", &section).unwrap().notes.clone();
            let start = Pos2::new(500.0, 300.0);
            let end = Pos2::new(400.0, 360.0);
            frame(&ctx, &mut view, &shared, vec![egui::Event::PointerMoved(start), egui::Event::PointerButton { pos: start, button, pressed: true, modifiers: egui::Modifiers::NONE }]);
            frame(&ctx, &mut view, &shared, vec![egui::Event::PointerMoved(end)]);
            frame(&ctx, &mut view, &shared, vec![egui::Event::PointerButton { pos: end, button, pressed: false, modifiers: egui::Modifiers::NONE }]);
            assert!((view.scroll_x - (2000.0 + PPQ as f32 / 2.0)).abs() < 0.1);
            assert!((view.top_pitch - 93.0).abs() < 0.1);
            assert!(view.pan_origin.is_none());
            let g = shared.lock_store();
            assert_eq!(g.revision, revision);
            assert_eq!(serde_json::to_value(&before).unwrap(), serde_json::to_value(&g.session.clip("melody", &section).unwrap().notes).unwrap());
        }
    }

    #[test]
    fn navigation_clamps_to_song_and_pitch_limits() {
        let ctx = egui::Context::default();
        let mut session = flvstx_core::Session::default();
        let section = session.add_section("Verse", 4, 0.6);
        let shared = Shared::new(session);
        shared.ui.lock().unwrap().selected_section = Some(section);
        let mut view = PianoRollView::default();
        for _ in 0..3 { frame(&ctx, &mut view, &shared, vec![]); }
        view.scroll_x = -10000.0;
        view.top_pitch = 200.0;
        frame(&ctx, &mut view, &shared, vec![]);
        assert_eq!(view.scroll_x, 0.0);
        assert_eq!(view.top_pitch, 127.0);
        view.scroll_x = 1e9;
        view.top_pitch = -100.0;
        frame(&ctx, &mut view, &shared, vec![]);
        assert!(view.scroll_x < 4.0 * 4.0 * PPQ as f32);
        assert!(view.top_pitch >= 0.0);
    }
}
