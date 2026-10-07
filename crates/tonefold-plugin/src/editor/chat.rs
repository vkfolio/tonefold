//! Chat panel: transcript, tool-call chips, input box, quick actions, agent status.

use super::{theme, EditorState};
use crate::state::{ChatRole, PlanState, Shared};
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

/// The colour a specialist signs its messages with — the same family as the layers it owns, so a
/// glance at the transcript says who was working.
fn agent_color(agent: &str) -> Color32 {
    match agent {
        "harmony-form" => Color32::from_rgb(126, 168, 232),   // chords blue
        "melody-topline" => Color32::from_rgb(232, 186, 116), // melody amber
        "rhythm-section" => Color32::from_rgb(228, 138, 170),  // drums pink
        "arrangement-mix" => Color32::from_rgb(140, 208, 170), // arrangement green
        _ => Color32::from_rgb(143, 200, 218),
    }
}

pub fn show(ui: &mut egui::Ui, st: &mut EditorState, shared: &Shared) {
    // At large UI scales in a narrow rack the header alone can eat the panel, and the input has to
    // stay reachable — so the title shrinks before the writing surface does.
    let roomy = ui.available_height() > 300.0 * st.ui_scale;
    if roomy {
        theme::eyebrow(ui, "CREATIVE PARTNER");
    }
    let connected = st.agent_connected();
    let producer = st.mode == "producer";
    let waiting = producer && st.phase == "awaiting_approval";
    ui.horizontal_wrapped(|ui| {
        // The toggle is the title. A mode change restarts the sidecar session (different tools and
        // persona), so it is only offered between turns.
        for (mode, hint) in [
            ("Composer", "Answers straight away - best for one quick change"),
            ("Producer", "Plans the work and waits for your approval before writing anything"),
        ] {
            let selected = st.mode.eq_ignore_ascii_case(mode);
            let text = RichText::new(mode).color(if selected { theme::TEXT } else { theme::MUTED });
            let text = if roomy { text.heading() } else { text.strong() };
            if ui.add_enabled(!st.turn_active, egui::SelectableLabel::new(selected, text)).on_hover_text(hint).clicked() {
                st.mode = mode.to_lowercase();
            }
        }
        let (label, color) = if waiting {
            ("Waiting for you", Color32::from_rgb(143, 200, 218))
        } else if producer && st.turn_active && st.phase == "planning" {
            ("Planning", Color32::from_rgb(240, 200, 100))
        } else if producer && st.turn_active {
            ("Building", Color32::from_rgb(240, 200, 100))
        } else if st.turn_active {
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
            // Nothing is running while the plan sits with the user, so no spinner.
            if !waiting { ui.spinner(); }
            if ui.small_button("Cancel").clicked() { st.cancel_turn(); }
        } else if !connected && ui.small_button("Connect").clicked() {
            st.ensure_agent(shared);
        }
        // A long delegated step can be quiet for a while; a very long silence usually is not work.
        if st.turn_active && !waiting {
            if let Some(quiet) = st.last_event.map(|t| t.elapsed().as_secs()).filter(|s| *s >= 120) {
                ui.label(RichText::new(format!("quiet for {}m", quiet / 60)).small().color(Color32::from_rgb(240, 170, 120)))
                    .on_hover_text("No word from the agent for a while. Cancel if it looks stuck.");
            }
        }
        // Cost is shown, never capped — the model picker is the only thing that governs spend.
        if let Some(c) = st.last_cost {
            let total = st.session_cost;
            ui.label(RichText::new(format!("${c:.2}")).small().color(theme::MUTED))
                .on_hover_text(format!("last turn ${c:.2} · this session ${total:.2}"));
        }
    });
    ui.horizontal_wrapped(|ui| {
        // Where the model runs. Switching mid-conversation is fine: the sidecar keeps one
        // conversation per provider, so coming back to Claude picks up where it left off.
        let ollama = st.provider == "ollama";
        for (value, label, hint) in [
            ("claude", "Claude", "Anthropic, through your Claude Code login"),
            ("ollama", "Ollama", "A local or remote Ollama server — free, private, and only as capable as the model you run"),
        ] {
            let selected = st.provider == value;
            if ui.add_enabled(!st.turn_active, egui::SelectableLabel::new(selected, RichText::new(label).small())).on_hover_text(hint).clicked() && !selected {
                st.provider = value.into();
                if value == "ollama" { st.refresh_models(shared); }
            }
        }
        if ollama {
            let url = ui.add(egui::TextEdit::singleline(&mut st.ollama_url_draft).desired_width(150.0 * st.ui_scale).hint_text("http://localhost:11434"))
                .on_hover_text("Ollama server: localhost, or another machine on your network (set OLLAMA_HOST=0.0.0.0 there)");
            let entered = url.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (url.lost_focus() || entered) && st.ollama_url_draft.trim() != st.ollama_url {
                st.ollama_url = st.ollama_url_draft.trim().to_string();
                st.ollama_url_draft = st.ollama_url.clone();
                st.refresh_models(shared);
            }
            let shown = if st.ollama_model.is_empty() { "pick a model" } else { st.ollama_model.as_str() };
            egui::ComboBox::from_id_salt("ollama-model").width(150.0 * st.ui_scale).selected_text(shown).show_ui(ui, |ui| {
                if st.ollama_models.is_empty() {
                    ui.label(RichText::new("nothing listed yet").small().color(theme::MUTED));
                }
                for m in st.ollama_models.clone() {
                    ui.selectable_value(&mut st.ollama_model, m.clone(), m);
                }
            });
            if ui.small_button("↻").on_hover_text("Ask the server for its models again").clicked() {
                st.refresh_models(shared);
            }
            if !st.models_status.is_empty() {
                let warn = !st.models_status.starts_with("asking");
                ui.label(RichText::new(&st.models_status).small().color(if warn { Color32::from_rgb(240, 170, 120) } else { theme::MUTED }));
            }
        } else {
            ui.label(RichText::new("Model").small().weak());
            egui::ComboBox::from_id_salt("model").width(90.0 * st.ui_scale).selected_text(&st.model).show_ui(ui, |ui| {
                for value in ["default", "sonnet", "opus"] {
                    ui.selectable_value(&mut st.model, value.to_string(), value);
                }
            });
        }
        // One switch, remembered per provider: Claude thinks by default, a local model does not.
        let think = if ollama { &mut st.ollama_think } else { &mut st.claude_think };
        ui.add_enabled(!st.turn_active, egui::Checkbox::new(think, RichText::new("Think").small())).on_hover_text(if ollama {
            "Let the model reason before it answers. Better on hard requests, much slower on a small GPU; the reasoning shows in the transcript."
        } else {
            "Let Claude think before it answers (adaptive: it decides how much). Off is faster and cheaper on simple requests."
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
        let plan = shared.plan.lock().ok().and_then(|p| p.clone());
        if let Some(plan) = &plan {
            plan_bar(ui, st, shared, plan, compact);
        } else if st.turn_active {
            // No plan on screen (composer mode, or a producer turn still planning) — but a running
            // checklist is still worth showing.
            todo_list(ui, shared, compact);
        }
        let compact = compact || plan.is_some();
        if !st.turn_active {
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
        }
        let response = egui::ScrollArea::vertical().id_salt("draft-scroll").max_height(if compact { 84.0 } else { 120.0 * st.ui_scale }).show(ui, |ui| {
        ui.add(egui::TextEdit::multiline(&mut st.input)
            .desired_rows(if compact { 2 } else { 3 }).desired_width(f32::INFINITY).margin(egui::vec2(12.0, 12.0))
            .return_key(Some(egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::Enter)))
            .hint_text(if waiting { "Type what to change, then Revise above..." } else { "Describe a melody, mood, or change..." }))
        }).inner;
        let enter = response.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        // Ollama has no default model, so a turn without one would only come back as an error.
        let ready = !st.turn_active && !st.input.trim().is_empty() && !(st.provider == "ollama" && st.ollama_model.is_empty());
        let busy_label = if producer { "Working..." } else { "Composing..." };
        let send = !waiting
            && ui.add_enabled(ready, egui::Button::new(RichText::new(if st.turn_active { busy_label } else if producer { "Plan this" } else { "Send request" }).strong().color(theme::CANVAS))
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
                let by = lines[start..index].iter().filter_map(|l| l.agent.clone()).next();
                let title = match &by {
                    Some(a) => format!("{} composition steps · {}", index - start, a.replace('-', " ")),
                    None => format!("{} composition steps", index - start),
                };
                let tint = by.as_deref().map(agent_color).unwrap_or(theme::MUTED);
                ui.push_id(("activity", start), |ui| {
                    egui::CollapsingHeader::new(RichText::new(title).small().color(tint)).show(ui, |ui| {
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
                                let (author, tint) = match (&line.agent, user) {
                                    (_, true) => ("YOU".to_string(), Color32::from_rgb(143, 200, 218)),
                                    (Some(a), _) => (a.replace('-', " ").to_uppercase(), agent_color(a)),
                                    (None, _) => ((if producer { "PRODUCER" } else { "COMPOSER" }).to_string(), Color32::from_rgb(143, 200, 218)),
                                };
                                ui.label(RichText::new(author).small().strong().color(tint));
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
                    ChatRole::Thinking => {
                        let by = line.agent.as_deref().map(|a| format!("Thinking · {}", a.replace('-', " "))).unwrap_or_else(|| "Thinking".into());
                        egui::CollapsingHeader::new(RichText::new(by).small().weak()).show(ui, |ui| {
                            ui.label(RichText::new(&line.text).small().italics().color(theme::MUTED));
                        });
                    }
                }
            });
            index += 1;
        }
        if st.turn_active && !st.thinking.is_empty() {
            // Live reasoning: the tail of it, so a long deliberation reads as progress, not a wall.
            ui.add_space(6.0);
            let tail: String = {
                let n = st.thinking.chars().count();
                if n > 700 { format!("…{}", st.thinking.chars().skip(n - 700).collect::<String>()) } else { st.thinking.clone() }
            };
            egui::CollapsingHeader::new(RichText::new("Thinking…").small().weak()).default_open(true).show(ui, |ui| {
                ui.label(RichText::new(tail).small().italics().color(theme::MUTED));
            });
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

/// The pinned answer to a proposed plan. The plan itself goes into the transcript, which scrolls —
/// this bar does not, so a twelve-step plan can still be answered on a small panel.
fn plan_bar(ui: &mut egui::Ui, st: &mut EditorState, shared: &Shared, plan: &PlanState, compact: bool) {
    let awaiting = plan.status == "awaiting_approval";
    let (title, tint) = match plan.status.as_str() {
        "awaiting_approval" => ("Plan — your call", Color32::from_rgb(143, 200, 218)),
        "executing" => ("Plan approved — building", Color32::from_rgb(240, 200, 100)),
        "sent_back" => ("Sent back — replanning", Color32::from_rgb(240, 200, 100)),
        "done" => ("Plan done", Color32::from_rgb(120, 220, 170)),
        _ => ("Plan", theme::MUTED),
    };
    let finished = plan.status == "done";
    egui::Frame::new().fill(theme::SURFACE).corner_radius(10.0).inner_margin(10.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(title).small().strong().color(tint));
            // The subtitle is the first thing to go when the panel is tight: the checklist below it
            // says more, and the input matters more than either.
            if !compact {
                let steps = plan.plan.steps.len();
                let rev = if plan.plan.revision > 1 { format!(", revision {}", plan.plan.revision) } else { String::new() };
                ui.label(RichText::new(format!("{steps} steps{rev} — read it above")).small().color(theme::MUTED));
            }
        });
        if finished {
            ui.horizontal_wrapped(|ui| {
                // One step back to before the whole run — undo cannot reach that far.
                if plan.checkpoint.is_some() && ui.small_button("Revert this run").on_hover_text("Put the song back to how it was before the plan ran").clicked() {
                    st.revert_run(shared);
                }
                if ui.small_button("Dismiss").clicked() {
                    if let Ok(mut p) = shared.plan.lock() {
                        *p = None;
                    }
                }
            });
        }
        if !awaiting && !finished {
            // Bounded: while it runs there is nothing here to click, and the input must survive a
            // checklist of any length at any UI scale.
            let budget = (ui.available_height() * 0.4).clamp(60.0, 260.0);
            egui::ScrollArea::vertical().id_salt("run-todos").max_height(budget).show(ui, |ui| {
                ui.set_width(ui.available_width());
                todo_list(ui, shared, compact);
            });
        }
        if awaiting {
            ui.add_space(4.0);
            // Columns, not a wrapped row: at large UI scales a wrapped button falls off the panel,
            // and an unanswerable plan wedges the turn.
            let note = st.input.trim().to_string();
            let mut answer: Option<(&str, Option<String>)> = None;
            ui.columns(3, |c| {
                let w = c[0].available_width();
                if c[0].add(egui::Button::new(RichText::new("Approve").strong().color(theme::CANVAS)).fill(theme::ACCENT).min_size(egui::vec2(w, 0.0)))
                    .on_hover_text("Build it, in this order").clicked() {
                    answer = Some(("approve", None));
                }
                let w = c[1].available_width();
                if c[1].add(egui::Button::new("Revise").min_size(egui::vec2(w, 0.0)))
                    .on_hover_text(if note.is_empty() { "Type what to change below, then click this" } else { note.as_str() }).clicked() {
                    answer = Some(("reject", if note.is_empty() { None } else { Some(note.clone()) }));
                }
                let w = c[2].available_width();
                if c[2].add(egui::Button::new("Stop").min_size(egui::vec2(w, 0.0)))
                    .on_hover_text("Cancel this plan and write nothing").clicked() {
                    answer = Some(("cancel", None));
                }
            });
            if let Some((decision, notes)) = answer {
                st.answer_plan(shared, decision, notes.as_deref());
                if decision == "reject" { st.input.clear(); }
            }
        }
    });
    ui.add_space(8.0);
}

/// The producer's checklist while the run goes: what is done, what it is doing now. Mirrored from
/// its own TodoWrite calls, so it cannot drift from what the model believes.
fn todo_list(ui: &mut egui::Ui, shared: &Shared, compact: bool) {
    let todos = shared.todos.lock().map(|t| t.clone()).unwrap_or_default();
    if todos.is_empty() {
        return;
    }
    let done = todos.iter().filter(|t| t.status == "completed").count();
    ui.add_space(4.0);
    ui.label(RichText::new(format!("{done}/{} steps", todos.len())).small().color(theme::MUTED));
    // A tight panel gets only what is happening now; the rest is a count. The input matters more.
    let current = todos.iter().position(|t| t.status == "in_progress").unwrap_or(done);
    let (first, take) = if compact { (current, 1) } else { (0, 8) };
    for t in todos.iter().skip(first).take(take) {
        let (mark, color) = match t.status.as_str() {
            "completed" => ("done", theme::MUTED),
            "in_progress" => ("now", Color32::from_rgb(240, 200, 100)),
            _ => ("next", theme::MUTED),
        };
        let content = match (compact, t.content.char_indices().nth(26)) {
            (true, Some((cut, _))) => format!("{}…", &t.content[..cut]),
            _ => t.content.clone(),
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(mark).small().strong().color(color));
            ui.label(RichText::new(content).small().color(if t.status == "completed" { theme::MUTED } else { theme::TEXT }));
        });
    }
}
