//! Chat panel: transcript, tool-call chips, input box, quick actions, agent status.

use super::EditorState;
use crate::state::{ChatRole, Shared};
use nih_plug_egui::egui::{self, Color32, RichText};
use std::sync::atomic::Ordering;

pub const QUICK_ACTIONS: &[(&str, &str)] = &[
    ("Suggest chords", "Suggest three chord progressions that fit this session's key and style, then set the best one on the selected section and generate all parts."),
    ("New melody", "Write a fresh melody idea for the selected section: invent a 1-bar motif and develop it. Keep the other tracks."),
    ("Variation", "Make a variation of the selected section's melody and drums: same motif, different rhythm and a new seed. Keep the chords and bass."),
    ("More energy", "Raise the energy of the selected section: busier drums with 16th hats and a fill, a pulsing bass, and a melody that sits a bit higher."),
    ("Calmer", "Make the selected section calmer: sparser drums (or none), held chords, a simpler bass, and a melody with more rests."),
    ("Humanize more", "Loosen the feel on all tracks of the selected section: more timing variation, a laid-back pocket on drums and bass, and gentle swing."),
    ("Tighter", "Tighten the timing on all tracks (near-quantized, minimal jitter) but keep the velocity dynamics."),
    ("Full song", "Turn the current material into a full song: intro, verse, chorus, verse 2, chorus, bridge, final chorus, outro, with related melodies and an energy curve."),
];

pub fn show(ui: &mut egui::Ui, st: &mut EditorState, shared: &Shared) {
    ui.horizontal(|ui| {
        ui.heading("Composer");
        let (connected, spawned) = (st.agent_connected(), st.agent_spawned());
        let status = if st.turn_active {
            ("thinking…", Color32::from_rgb(240, 200, 80))
        } else if connected {
            ("connected", Color32::from_rgb(120, 220, 120))
        } else if spawned {
            ("starting…", Color32::from_rgb(200, 200, 120))
        } else {
            ("offline", Color32::from_rgb(200, 120, 120))
        };
        ui.label(RichText::new(status.0).color(status.1).small());
        if st.turn_active {
            ui.spinner();
            if ui.small_button("Cancel").clicked() {
                st.cancel_turn();
            }
        } else if !connected && ui.small_button("Connect").clicked() {
            st.ensure_agent(shared);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Clear chat").clicked() {
                if let Ok(mut c) = shared.chat.lock() {
                    c.clear();
                }
            }
            let models = [("default", "default"), ("sonnet", "sonnet"), ("opus", "opus")];
            let cur = st.model.clone();
            egui::ComboBox::from_id_salt("model").width(80.0 * st.ui_scale).selected_text(cur.as_str()).show_ui(ui, |ui| {
                for (label, value) in models {
                    if ui.selectable_label(st.model == value, label).clicked() {
                        st.model = value.to_string();
                    }
                }
            });
        });
    });
    ui.separator();

    // Transcript.
    let avail = ui.available_height() - 150.0;
    egui::ScrollArea::vertical().id_salt("chat-scroll").auto_shrink([false, false]).max_height(avail.max(80.0)).stick_to_bottom(true).show(ui, |ui| {
        let lines = shared.chat.lock().map(|c| c.clone()).unwrap_or_default();
        for line in &lines {
            match line.role {
                ChatRole::User => {
                    ui.add_space(4.0);
                    egui::Frame::new().fill(Color32::from_rgb(40, 52, 70)).corner_radius(6.0).inner_margin(6.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(&line.text).color(Color32::from_rgb(230, 235, 245)));
                    });
                }
                ChatRole::Assistant => {
                    ui.add_space(4.0);
                    egui::Frame::new().fill(Color32::from_rgb(34, 40, 46)).corner_radius(6.0).inner_margin(6.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui_commonmark::CommonMarkViewer::new().show(ui, &mut st.md_cache, &line.text);
                    });
                }
                ChatRole::Tool => {
                    ui.label(RichText::new(format!("  ▸ {}", line.text)).small().color(Color32::from_rgb(150, 170, 190)));
                }
                ChatRole::System => {
                    ui.label(RichText::new(&line.text).small().italics().color(Color32::from_rgb(170, 150, 150)));
                }
            }
        }
        if st.turn_active && !st.streaming.is_empty() {
            ui.add_space(4.0);
            egui::Frame::new().fill(Color32::from_rgb(34, 40, 46)).corner_radius(6.0).inner_margin(6.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let text = st.streaming.clone();
                egui_commonmark::CommonMarkViewer::new().show(ui, &mut st.md_cache, &text);
            });
        }
    });

    ui.separator();
    // Quick actions.
    ui.horizontal_wrapped(|ui| {
        for (label, prompt) in QUICK_ACTIONS {
            if ui.add_enabled(!st.turn_active, egui::Button::new(*label).small()).clicked() {
                st.send_message(shared, prompt);
            }
        }
    });
    // Input.
    let mut send = false;
    ui.horizontal(|ui| {
        let te = egui::TextEdit::multiline(&mut st.input).desired_rows(3).hint_text("Describe what you want… (Enter to send, Shift+Enter for a new line)").desired_width(ui.available_width() - 60.0);
        let resp = ui.add(te);
        if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift) {
            send = true;
        }
        if ui.add_enabled(!st.turn_active && !st.input.trim().is_empty(), egui::Button::new("Send")).clicked() {
            send = true;
        }
    });
    if send && !st.turn_active {
        let text = st.input.trim_end_matches('\n').trim().to_string();
        if !text.is_empty() {
            st.input.clear();
            st.send_message(shared, &text);
        }
    }
    let _ = shared.playing.load(Ordering::Relaxed);
}
