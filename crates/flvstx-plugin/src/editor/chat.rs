//! Chat panel: transcript, tool-call chips, input box, quick actions, agent status.

use super::{theme, EditorState};
use crate::state::{ChatRole, Shared};
use nih_plug_egui::egui::{self, Color32, RichText};

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
    theme::eyebrow(ui, "CREATIVE PARTNER");
    let connected = st.agent_connected();
    ui.horizontal_wrapped(|ui| {
        ui.heading("Composer");
        let (label, color) = if st.turn_active {
            ("Composing", Color32::from_rgb(240, 200, 100))
        } else if connected {
            ("Ready", Color32::from_rgb(120, 220, 170))
        } else if st.agent_spawned() {
            ("Connecting", Color32::from_rgb(220, 200, 130))
        } else {
            ("Offline", Color32::from_rgb(175, 185, 200))
        };
        ui.label(RichText::new(label).color(color).small());
        if st.turn_active {
            ui.spinner();
            if ui.small_button("Cancel").clicked() { st.cancel_turn(); }
        } else if !connected && ui.small_button("Connect").clicked() {
            st.ensure_agent(shared);
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Model").small().weak());
        egui::ComboBox::from_id_salt("model").width(90.0 * st.ui_scale).selected_text(&st.model).show_ui(ui, |ui| {
            for value in ["default", "sonnet", "opus"] {
                ui.selectable_value(&mut st.model, value.to_string(), value);
            }
        });
        ui.menu_button("Chat options", |ui| {
            if ui.add_enabled(!st.turn_active, egui::Button::new("Clear transcript")).on_hover_text("Keeps the song and composer session").clicked() {
                if let Ok(mut c) = shared.chat.lock() { c.clear(); }
                ui.close_menu();
            }
        });
    });
    ui.separator();

    let compact = ui.available_height() < 450.0;
    // Lay out the input first so a long transcript cannot push it off screen.
    egui::TopBottomPanel::bottom("composer-input").frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(0, 10))).show_inside(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.menu_button("Quick ideas", |ui| {
                for (label, prompt) in QUICK_ACTIONS {
                    if ui.add_enabled(!st.turn_active, egui::Button::new(*label)).clicked() {
                        // Draft first: users can refine the request before sending it.
                        st.input = prompt.to_string();
                        ui.close_menu();
                    }
                }
            });
            if !compact { ui.label(RichText::new("Shift+Enter for a new line").small().color(theme::MUTED)); }
        });
        let response = egui::ScrollArea::vertical().id_salt("draft-scroll").max_height(if compact { 84.0 } else { 120.0 * st.ui_scale }).show(ui, |ui| {
        ui.add(egui::TextEdit::multiline(&mut st.input)
            .desired_rows(if compact { 2 } else { 3 }).desired_width(f32::INFINITY).margin(egui::vec2(12.0, 12.0))
            .return_key(Some(egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::Enter)))
            .hint_text("Describe a melody, mood, or change..."))
        }).inner;
        let enter = response.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let ready = !st.turn_active && !st.input.trim().is_empty();
        let send = ui.add_enabled(ready, egui::Button::new(RichText::new(if st.turn_active { "Composing..." } else { "Send request" }).strong().color(theme::CANVAS))
            .fill(theme::ACCENT).min_size(egui::vec2(ui.available_width(), 32.0 * st.ui_scale))).clicked();
        if ready && (enter || send) {
            let text = st.input.trim().to_string();
            st.input.clear();
            st.send_message(shared, &text);
            response.request_focus();
        }
    });

    egui::ScrollArea::vertical().id_salt("chat-scroll").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
        let lines = shared.chat.lock().map(|c| c.clone()).unwrap_or_default();
        if lines.is_empty() {
            ui.add_space(16.0);
            ui.heading("Start with an idea");
            ui.label("Describe what you want to hear, then refine it one layer at a time.");
            ui.add_space(8.0);
            for (label, prompt) in [
                ("Warm lo-fi groove", "Create an 8-bar warm lo-fi groove with mellow chords, a simple melody, bass and relaxed drums."),
                ("Bright pop chorus", "Create an uplifting 8-bar pop chorus with a memorable melody and energetic rhythm."),
                ("Develop my song", "Listen to the current session context and suggest how to develop the selected section while keeping my existing ideas."),
            ] {
                if ui.add_enabled(!st.turn_active, egui::Button::new(label)).clicked() {
                    st.input = prompt.to_string();
                }
            }
            ui.add_space(12.0);
            ui.label(RichText::new("Prefer to start without chat? Add a section, choose a layer, then use Suggest or Generate section below.").small().weak());
        }
        let mut index = 0;
        while index < lines.len() {
            // Consecutive tool calls are one activity group, not a wall of identical rows.
            if lines[index].role == ChatRole::Tool {
                let start = index;
                while index < lines.len() && lines[index].role == ChatRole::Tool { index += 1; }
                ui.push_id(("activity", start), |ui| {
                    egui::CollapsingHeader::new(RichText::new(format!("{} composition steps", index - start)).small().color(theme::MUTED)).show(ui, |ui| {
                        for line in &lines[start..index] { ui.label(RichText::new(&line.text).small()); }
                    });
                });
                continue;
            }
            let line = &lines[index];
            ui.push_id(index, |ui| {
                match line.role {
                    ChatRole::User | ChatRole::Assistant => {
                        let user = matches!(line.role, ChatRole::User);
                        ui.add_space(6.0);
                        egui::Frame::new().fill(if user { Color32::from_rgb(33, 57, 64) } else { theme::SURFACE })
                            .corner_radius(10.0).inner_margin(14.0).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new(if user { "YOU" } else { "COMPOSER" }).small().strong().color(Color32::from_rgb(143, 200, 218)));
                                if user { ui.label(&line.text); }
                                else { egui_commonmark::CommonMarkViewer::new().show(ui, &mut st.md_cache, &line.text); }
                            });
                    }
                    ChatRole::Tool => {
                        egui::CollapsingHeader::new(RichText::new("Session update").small().weak()).show(ui, |ui| {
                            ui.label(&line.text);
                        });
                    }
                    ChatRole::System => {
                        ui.label(RichText::new(&line.text).small().color(Color32::from_rgb(185, 190, 200)));
                    }
                }
            });
            index += 1;
        }
        if st.turn_active && !st.streaming.is_empty() {
            ui.add_space(6.0);
            egui::Frame::new().fill(theme::SURFACE).corner_radius(10.0).inner_margin(14.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui_commonmark::CommonMarkViewer::new().show(ui, &mut st.md_cache, &st.streaming);
            });
        }
    });
}
