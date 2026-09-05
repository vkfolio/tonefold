//! Piano roll editor for the selected track/section: grid, notes, selection, move/resize/draw/delete,
//! velocity lane, chord labels, drum-lane mode, note preview.

use super::track_color;
use crate::state::Shared;
use flvstx_core::notation::{drum_pitch_lane, drum_lane_pitch};
use flvstx_core::theory::pitch_name;
use flvstx_core::{Note, TrackRole, PPQ};
use nih_plug_egui::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use std::collections::HashSet;
use std::sync::atomic::Ordering;

const KEYS_W: f32 = 44.0;
const HEADER_H: f32 = 22.0;
const VEL_H: f32 = 64.0;
const DRUM_LANES: [&str; 13] = ["K", "S", "RS", "CL", "H", "PH", "OH", "T1", "T2", "T3", "RD", "CR", "P"];

#[derive(Debug, Clone)]
enum Drag {
    Move { origin: Pos2, orig: Vec<(usize, Note)>, moved: bool },
    Resize { origin: Pos2, orig: Vec<(usize, Note)> },
    Box { origin: Pos2, current: Pos2 },
    Velocity,
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
}

impl Default for PianoRollView {
    fn default() -> Self {
        PianoRollView { zoom: 60.0, scroll_x: 0.0, top_pitch: 84.0, row_h: 11.0, selection: HashSet::new(), drag: None, snap_div: 16, last_track: None, last_section: None }
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
    if view.last_track.as_deref() != Some(track_id.as_str()) || view.last_section.as_deref() != Some(section.id.as_str()) {
        view.selection.clear();
        view.drag = None;
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

    // Toolbar.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{} · {}{}", track_name, section.name, if active { "" } else { " (silent here)" })).strong().color(color));
        ui.label("snap");
        egui::ComboBox::from_id_salt("snap").width(56.0).selected_text(format!("1/{}", view.snap_div)).show_ui(ui, |ui| {
            for d in [4u32, 8, 16, 32, 12, 24] {
                ui.selectable_value(&mut view.snap_div, d, format!("1/{d}"));
            }
        });
        if ui.small_button("−").clicked() {
            view.zoom = (view.zoom / 1.25).max(8.0);
        }
        if ui.small_button("+").clicked() {
            view.zoom = (view.zoom * 1.25).min(400.0);
        }
        if ui.small_button("fit").clicked() {
            view.zoom = ((ui.available_width() - KEYS_W - 40.0) / (total_ticks as f32 / PPQ as f32)).clamp(8.0, 400.0);
            view.scroll_x = 0.0;
        }
        ui.label(egui::RichText::new(format!("{} notes · double-click to add · drag to move · right edge to resize · right-click or Del to delete · ↑↓ transpose · Ctrl+A/D select all/duplicate", notes.len())).small().weak());
    });

    let avail = ui.available_size();
    let (rect, resp) = ui.allocate_exact_size(avail, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_rgb(24, 26, 30));
    let grid = Rect::from_min_max(Pos2::new(rect.left() + KEYS_W, rect.top() + HEADER_H), Pos2::new(rect.right(), rect.bottom() - VEL_H));
    let vel_rect = Rect::from_min_max(Pos2::new(grid.left(), grid.bottom()), rect.max);
    let row_h = if drums { (grid.height() / DRUM_LANES.len() as f32).clamp(12.0, 28.0) } else { view.row_h };

    // Input: scroll / zoom.
    if resp.hovered() {
        let (scroll, zoom_delta, mods) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers));
        if zoom_delta != 1.0 || (mods.ctrl && scroll.y != 0.0) {
            let z = if zoom_delta != 1.0 { zoom_delta } else { 1.0 + scroll.y * 0.002 };
            let old = view.zoom;
            view.zoom = (view.zoom * z).clamp(8.0, 400.0);
            if let Some(p) = resp.hover_pos() {
                let tick_at = view.scroll_x + (p.x - grid.left()) / old * PPQ as f32;
                view.scroll_x = (tick_at - (p.x - grid.left()) / view.zoom * PPQ as f32).max(0.0);
            }
        } else if mods.shift && scroll.y != 0.0 {
            view.scroll_x = (view.scroll_x - scroll.y / view.zoom * PPQ as f32).max(0.0);
        } else {
            if scroll.x != 0.0 {
                view.scroll_x = (view.scroll_x - scroll.x / view.zoom * PPQ as f32).max(0.0);
            }
            if scroll.y != 0.0 && !drums {
                view.top_pitch = (view.top_pitch + scroll.y / row_h).clamp(24.0, 127.0);
            }
        }
    }

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

    // Grid rows.
    if drums {
        for (i, lane) in DRUM_LANES.iter().enumerate() {
            let y = grid.top() + i as f32 * row_h;
            let r = Rect::from_min_size(Pos2::new(grid.left(), y), Vec2::new(grid.width(), row_h));
            painter.rect_filled(r, 0.0, if i % 2 == 0 { Color32::from_rgb(30, 32, 37) } else { Color32::from_rgb(27, 29, 33) });
            painter.text(Pos2::new(rect.left() + 4.0, y + row_h / 2.0), Align2::LEFT_CENTER, *lane, FontId::monospace(11.0), Color32::from_rgb(200, 200, 200));
        }
    } else {
        let top_p = view.top_pitch as i32;
        let rows = (grid.height() / row_h) as i32 + 2;
        for i in 0..rows {
            let p = top_p - i;
            if !(0..=127).contains(&p) {
                continue;
            }
            let y = grid.top() + i as f32 * row_h;
            let r = Rect::from_min_size(Pos2::new(grid.left(), y), Vec2::new(grid.width(), row_h));
            let pc = (p % 12) as u8;
            let black = matches!(pc, 1 | 3 | 6 | 8 | 10);
            let in_key = key.contains(p as u8);
            let fill = if !in_key { Color32::from_rgb(26, 27, 31) } else if black { Color32::from_rgb(31, 33, 38) } else { Color32::from_rgb(36, 39, 45) };
            painter.rect_filled(r, 0.0, fill);
            if pc == key.root {
                painter.line_segment([Pos2::new(grid.left(), y + row_h), Pos2::new(grid.right(), y + row_h)], Stroke::new(1.0, Color32::from_rgb(60, 66, 80)));
            }
            // Keys column.
            let kr = Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(KEYS_W - 2.0, row_h - 1.0));
            painter.rect_filled(kr, 2.0, if black { Color32::from_rgb(40, 40, 44) } else { Color32::from_rgb(210, 210, 214) });
            if pc == 0 {
                painter.text(Pos2::new(rect.left() + 3.0, y + row_h / 2.0), Align2::LEFT_CENTER, pitch_name(p as u8), FontId::monospace(9.0), Color32::from_rgb(40, 40, 40));
            }
        }
    }
    // Beat / bar lines + header.
    let first_beat = (view.scroll_x / PPQ as f32).floor() as u32;
    let beats_per_bar = bar_ticks / PPQ;
    let last_tick = x_to_tick(grid.right());
    let mut t = first_beat * PPQ;
    while (t as f32) <= last_tick && t <= total_ticks {
        let x = tick_to_x(t as f32);
        let is_bar = t % bar_ticks == 0;
        painter.line_segment([Pos2::new(x, grid.top()), Pos2::new(x, grid.bottom())], Stroke::new(if is_bar { 1.5 } else { 0.6 }, if is_bar { Color32::from_rgb(90, 95, 110) } else { Color32::from_rgb(52, 55, 62) }));
        if is_bar {
            painter.text(Pos2::new(x + 3.0, rect.top() + 3.0), Align2::LEFT_TOP, format!("{}", t / bar_ticks + 1), FontId::proportional(10.0), Color32::from_rgb(170, 175, 185));
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
            painter.text(Pos2::new(x + 3.0, rect.top() + HEADER_H - 2.0), Align2::LEFT_BOTTOM, ev.chord.symbol(), FontId::proportional(10.0), track_color(TrackRole::Chords));
        }
    }
    // Playhead.
    let ph = shared.playhead_tick.load(Ordering::Relaxed);
    let sec_start = shared.lock_store().session.section_start(&section.id).unwrap_or(0);
    if ph >= sec_start && ph < sec_start + total_ticks && (shared.playing.load(Ordering::Relaxed) || shared.host_playing.load(Ordering::Relaxed)) {
        let x = tick_to_x((ph - sec_start) as f32);
        painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.5, Color32::from_rgb(255, 230, 120)));
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
        painter.rect_filled(r, 2.0, fill);
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
    painter.text(Pos2::new(rect.left() + 4.0, vel_rect.center().y), Align2::LEFT_CENTER, "vel", FontId::proportional(10.0), Color32::from_rgb(140, 140, 150));

    // Box selection preview.
    if let Some(Drag::Box { origin, current }) = &view.drag {
        let r = Rect::from_two_pos(*origin, *current);
        painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(120, 160, 255, 30));
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(120, 160, 255)), StrokeKind::Inside);
    }

    // ---- Interaction ----
    let pointer = resp.interact_pointer_pos();
    let in_grid = pointer.map(|p| grid.contains(p)).unwrap_or(false);
    let in_vel = pointer.map(|p| vel_rect.contains(p)).unwrap_or(false);

    if resp.drag_started() {
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
    let v = ((vel.clamp(0.05, 1.0) * 127.0) as u32).max(1);
    let ch = shared.lock_store().session.track_by(track).map(|t| t.channel).unwrap_or(0);
    shared.preview.store(((ch as u32) << 16) | ((pitch as u32) << 8) | v, Ordering::Release);
}
