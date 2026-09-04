//! egui editor: layout, top bar (key/tempo/style), arrangement strip, transport, track list,
//! agent connection management, and persistence sync.

mod chat;
mod piano_roll;

use crate::state::{ChatRole, Persisted, Shared};
use crate::FlvstxParams;
use flvstx_core::ops::dispatch;
use flvstx_core::theory::{ScaleKind, NOTE_NAMES_SHARP};
use flvstx_core::{Key, TrackRole};
use flvstx_ipc::{AgentClient, AgentEvent, DEFAULT_PORT};
use nih_plug::prelude::Editor;
use nih_plug_egui::egui::{self, Color32, RichText};
use nih_plug_egui::create_egui_editor;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

pub fn track_color(role: TrackRole) -> Color32 {
    match role {
        TrackRole::Chords => Color32::from_rgb(120, 170, 255),
        TrackRole::Melody => Color32::from_rgb(255, 190, 90),
        TrackRole::Bass => Color32::from_rgb(150, 230, 140),
        TrackRole::Drums => Color32::from_rgb(240, 120, 140),
    }
}

pub struct EditorState {
    params: Arc<FlvstxParams>,
    agent: Mutex<Option<AgentClient>>,
    child: Mutex<Option<std::process::Child>>,
    port: u16,
    pub input: String,
    pub turn_active: bool,
    pub streaming: String,
    pub piano: piano_roll::PianoRollView,
    last_persisted_revision: u64,
    last_loaded_hash: u64,
    status: String,
    spawn_attempted_at: Option<std::time::Instant>,
}

impl EditorState {
    fn agent_connected(&self) -> bool {
        self.agent.lock().ok().and_then(|a| a.as_ref().map(|c| c.is_connected())).unwrap_or(false)
    }
    fn agent_spawned(&self) -> bool {
        self.child.lock().map(|c| c.is_some()).unwrap_or(false)
    }

    /// Starts the sidecar if needed and connects the client.
    fn ensure_agent(&mut self, shared: &Shared) {
        if !flvstx_ipc::port_open(self.port) {
            let recently = self.spawn_attempted_at.map(|t| t.elapsed().as_secs() < 5).unwrap_or(false);
            if !recently {
                self.spawn_attempted_at = Some(std::time::Instant::now());
                match flvstx_ipc::spawn_agent(self.port) {
                    Ok(c) => {
                        if let Ok(mut g) = self.child.lock() {
                            *g = Some(c);
                        }
                        shared.push_chat(ChatRole::System, format!("starting composer agent on port {}…", self.port));
                    }
                    Err(e) => shared.push_chat(ChatRole::System, format!("could not start agent (node + agent/dist needed): {e}")),
                }
            }
        }
        if let Ok(mut g) = self.agent.lock() {
            if g.is_none() {
                let client = AgentClient::connect(self.port, shared.store.clone());
                if let Ok(sid) = shared.agent_session_id.lock() {
                    if let Ok(mut s) = client.session_id.lock() {
                        *s = sid.clone();
                    }
                }
                *g = Some(client);
            }
        }
    }

    fn send_message(&mut self, shared: &Shared, text: &str) {
        self.ensure_agent(shared);
        shared.push_chat(ChatRole::User, text);
        let ctx = {
            let g = shared.lock_store();
            let ui = shared.ui.lock().map(|u| u.clone()).unwrap_or_default();
            let mut c = flvstx_ipc::context_for(&g);
            if let Some(sec) = ui.selected_section {
                c.push_str(&format!("\nSelected section in the UI: {sec}\nSelected track in the UI: {}\n", ui.selected_track.name()));
            }
            c
        };
        if let Ok(g) = self.agent.lock() {
            if let Some(a) = g.as_ref() {
                a.send_user_message(text, &ctx);
                self.turn_active = true;
                self.streaming.clear();
            }
        }
    }

    fn cancel_turn(&mut self) {
        if let Ok(g) = self.agent.lock() {
            if let Some(a) = g.as_ref() {
                a.cancel();
            }
        }
    }

    fn poll_agent(&mut self, shared: &Shared) {
        let mut events = Vec::new();
        if let Ok(g) = self.agent.lock() {
            if let Some(a) = g.as_ref() {
                while let Some(e) = a.try_recv() {
                    events.push(e);
                }
            }
        }
        for e in events {
            match e {
                AgentEvent::Connected => shared.push_chat(ChatRole::System, "composer connected"),
                AgentEvent::Disconnected { reason } => {
                    self.turn_active = false;
                    shared.push_chat(ChatRole::System, format!("composer disconnected ({reason})"));
                }
                AgentEvent::Ready { backend, .. } => self.status = format!("agent ready ({backend})"),
                AgentEvent::AssistantDelta { text } => self.streaming.push_str(&text),
                AgentEvent::AssistantMessage { text } => {
                    self.streaming.clear();
                    shared.push_chat(ChatRole::Assistant, text);
                }
                AgentEvent::ToolCall { name, input } => {
                    let short = summarize_input(&name, &input);
                    shared.push_chat(ChatRole::Tool, format!("{name} {short}"));
                }
                AgentEvent::ToolResult { .. } => {}
                AgentEvent::Rpc { .. } => {}
                AgentEvent::SessionChanged => {}
                AgentEvent::Done { session_id, .. } => {
                    self.turn_active = false;
                    self.streaming.clear();
                    if !session_id.is_empty() {
                        if let Ok(mut s) = shared.agent_session_id.lock() {
                            *s = Some(session_id);
                        }
                    }
                }
                AgentEvent::Error { message } => {
                    self.turn_active = false;
                    shared.push_chat(ChatRole::System, format!("error: {message}"));
                }
                AgentEvent::Pong => {}
            }
        }
    }

    /// Keeps the playback buffer and the persisted JSON in sync with the store.
    fn sync_state(&mut self, shared: &Shared) {
        // Project load detection: the host wrote a new state blob.
        let json = self.params.state_json.read().map(|s| s.clone()).unwrap_or_default();
        let h = crate::hash_str(&json);
        if !json.is_empty() && h != self.last_loaded_hash {
            self.last_loaded_hash = h;
            let current = serde_json::to_string(&shared.to_persisted()).unwrap_or_default();
            if crate::hash_str(&current) != h {
                if let Ok(p) = serde_json::from_str::<Persisted>(&json) {
                    shared.load_persisted(p);
                }
            }
        }
        let rev = shared.lock_store().revision;
        if shared.needs_rebuild() {
            shared.rebuild_playback();
        }
        if rev != self.last_persisted_revision {
            self.last_persisted_revision = rev;
            let p = shared.to_persisted();
            if let Ok(s) = serde_json::to_string(&p) {
                self.last_loaded_hash = crate::hash_str(&s);
                if let Ok(mut w) = self.params.state_json.write() {
                    *w = s;
                }
            }
        }
    }
}

fn summarize_input(name: &str, input: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    if let Some(o) = input.as_object() {
        for (k, v) in o {
            let s = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let s: String = s.chars().take(60).collect();
            parts.push(format!("{k}={s}"));
        }
    }
    let _ = name;
    parts.join(" ")
}

pub fn create(params: Arc<FlvstxParams>, shared: Arc<Shared>) -> Option<Box<dyn Editor>> {
    let state = EditorState {
        params: params.clone(),
        agent: Mutex::new(None),
        child: Mutex::new(None),
        port: std::env::var("FLVSTX_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT),
        input: String::new(),
        turn_active: false,
        streaming: String::new(),
        piano: piano_roll::PianoRollView::default(),
        last_persisted_revision: u64::MAX,
        last_loaded_hash: 0,
        status: String::new(),
        spawn_attempted_at: None,
    };
    let shared_for_persist = shared.clone();
    create_egui_editor(
        params.editor_state.clone(),
        state,
        move |ctx, _state| {
            let mut style = (*ctx.style()).clone();
            style.visuals = egui::Visuals::dark();
            style.spacing.item_spacing = egui::vec2(6.0, 4.0);
            ctx.set_style(style);
            let _ = &shared_for_persist;
        },
        move |ctx, _setter, st| {
            st.poll_agent(&shared);
            st.sync_state(&shared);
            draw(ctx, st, &shared);
            ctx.request_repaint_after(std::time::Duration::from_millis(if st.turn_active || shared.playing.load(Ordering::Relaxed) || shared.host_playing.load(Ordering::Relaxed) { 33 } else { 120 }));
        },
    )
}

fn draw(ctx: &egui::Context, st: &mut EditorState, shared: &Shared) {
    egui::TopBottomPanel::top("top").show(ctx, |ui| {
        top_bar(ui, st, shared);
        sections_strip(ui, shared);
    });
    egui::SidePanel::left("chat").default_width(360.0).min_width(260.0).show(ctx, |ui| {
        chat::show(ui, st, shared);
    });
    egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
        transport(ui, st, shared);
    });
    egui::CentralPanel::default().show(ctx, |ui| {
        piano_roll::show(ui, &mut st.piano, shared);
    });
}

fn top_bar(ui: &mut egui::Ui, st: &mut EditorState, shared: &Shared) {
    let (key, tempo, style, ts) = {
        let g = shared.lock_store();
        (g.session.key, g.session.tempo, g.session.style.clone(), g.session.time_sig)
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new("FLVSTX").strong().size(18.0));
        ui.separator();
        // Key root.
        let mut root = key.root as usize;
        let mut scale = key.scale;
        egui::ComboBox::from_id_salt("root").width(50.0).selected_text(NOTE_NAMES_SHARP[root]).show_ui(ui, |ui| {
            for (i, n) in NOTE_NAMES_SHARP.iter().enumerate() {
                ui.selectable_value(&mut root, i, *n);
            }
        });
        egui::ComboBox::from_id_salt("scale").width(130.0).selected_text(scale.name()).show_ui(ui, |ui| {
            for s in ScaleKind::ALL {
                ui.selectable_value(&mut scale, s, s.name());
            }
        });
        if root != key.root as usize || scale != key.scale {
            let mut g = shared.lock_store();
            let _ = g.mutate(|s| {
                s.key = Key::new(root as u8, scale);
                Ok(())
            });
        }
        ui.separator();
        let mut t = tempo;
        ui.label("BPM");
        if ui.add(egui::DragValue::new(&mut t).range(30.0..=300.0).speed(0.5).fixed_decimals(0)).changed() {
            let mut g = shared.lock_store();
            let _ = g.mutate(|s| {
                s.tempo = t;
                Ok(())
            });
        }
        let mut num = ts.num;
        ui.label(format!("{}/{}", ts.num, ts.den));
        if ui.add(egui::DragValue::new(&mut num).range(2..=12)).changed() {
            let mut g = shared.lock_store();
            let _ = g.mutate(|s| {
                s.time_sig.num = num;
                Ok(())
            });
        }
        ui.separator();
        ui.label("Style");
        let mut style_edit = style.clone();
        let resp = ui.add(egui::TextEdit::singleline(&mut style_edit).desired_width(110.0));
        if resp.lost_focus() && style_edit != style {
            let mut g = shared.lock_store();
            let _ = g.mutate(|s| {
                s.style = style_edit.clone();
                Ok(())
            });
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let host_tempo = f32::from_bits(shared.host_tempo_bits.load(Ordering::Relaxed));
            ui.label(RichText::new(format!("host {:.0} BPM {}", host_tempo, if shared.host_playing.load(Ordering::Relaxed) { "▶" } else { "■" })).small().weak());
            if !st.status.is_empty() {
                ui.label(RichText::new(&st.status).small().weak());
            }
        });
    });
}

fn sections_strip(ui: &mut egui::Ui, shared: &Shared) {
    let (sections, selected) = {
        let g = shared.lock_store();
        let ui_state = shared.ui.lock().map(|u| u.clone()).unwrap_or_default();
        (g.session.sections.clone(), ui_state.selected_section)
    };
    // Auto-select the first section.
    if selected.is_none() && !sections.is_empty() {
        if let Ok(mut u) = shared.ui.lock() {
            u.selected_section = Some(sections[0].id.clone());
        }
        shared.rebuild_playback();
    }
    let selected = selected.or_else(|| sections.first().map(|s| s.id.clone()));
    let total_bars: u32 = sections.iter().map(|s| s.bars).sum::<u32>().max(1);
    let playhead = shared.playhead_tick.load(Ordering::Relaxed);
    let bar_ticks = shared.lock_store().session.bar_ticks();
    ui.horizontal(|ui| {
        let avail = ui.available_width() - 160.0;
        let mut new_selection = None;
        for sec in &sections {
            let w = (avail * sec.bars as f32 / total_bars as f32).max(48.0);
            let is_sel = selected.as_deref() == Some(sec.id.as_str());
            let start = shared.lock_store().session.section_start(&sec.id).unwrap_or(0);
            let in_play = playhead >= start && playhead < start + sec.bars * bar_ticks && (shared.playing.load(Ordering::Relaxed) || shared.host_playing.load(Ordering::Relaxed));
            let fill = if is_sel { Color32::from_rgb(70, 90, 130) } else { Color32::from_rgb(45, 48, 55) };
            let label = format!("{}\n{} bars · e{:.0}%", sec.name, sec.bars, sec.energy * 100.0);
            let btn = egui::Button::new(RichText::new(label).small()).fill(fill).min_size(egui::vec2(w, 34.0));
            let resp = ui.add(btn);
            if in_play {
                let r = resp.rect;
                let frac = (playhead - start) as f32 / (sec.bars * bar_ticks) as f32;
                let x = r.left() + r.width() * frac;
                ui.painter().line_segment([egui::pos2(x, r.top()), egui::pos2(x, r.bottom())], egui::Stroke::new(2.0, Color32::from_rgb(255, 230, 120)));
            }
            if resp.clicked() {
                new_selection = Some(sec.id.clone());
            }
        }
        if let Some(id) = new_selection {
            if let Ok(mut u) = shared.ui.lock() {
                u.selected_section = Some(id);
            }
            shared.rebuild_playback();
        }
        if ui.button("+ Section").clicked() {
            let mut g = shared.lock_store();
            let n = g.session.sections.len();
            let name = match n {
                0 => "Verse".to_string(),
                1 => "Chorus".to_string(),
                _ => format!("Section {}", n + 1),
            };
            let _ = g.mutate(|s| {
                s.add_section(&name, 8, 0.6);
                Ok(())
            });
        }
    });
    // Selected section editor.
    if let Some(id) = selected {
        if let Some(sec) = sections.iter().find(|s| s.id == id) {
            let (mut name, mut bars, mut energy) = (sec.name.clone(), sec.bars, sec.energy);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Section").weak());
                let r1 = ui.add(egui::TextEdit::singleline(&mut name).desired_width(120.0));
                ui.label("bars");
                let r2 = ui.add(egui::DragValue::new(&mut bars).range(1..=128));
                ui.label("energy");
                let r3 = ui.add(egui::Slider::new(&mut energy, 0.0..=1.0).show_value(false));
                if (r1.lost_focus() && name != sec.name) || r2.changed() || r3.drag_stopped() || (r3.changed() && !r3.dragged()) {
                    let mut g = shared.lock_store();
                    let _ = dispatch(&mut g, "set_section", &serde_json::json!({ "section": id, "name": name, "bars": bars, "energy": energy }));
                }
                let chords_text = {
                    let g = shared.lock_store();
                    flvstx_core::notation::format_chords(&sec.chords, &g.session.key, sec.bars, g.session.bar_ticks())
                };
                ui.label(RichText::new(chords_text).small().color(track_color(TrackRole::Chords)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕ delete").clicked() {
                        let mut g = shared.lock_store();
                        let _ = g.mutate(|s| {
                            s.sections.retain(|x| x.id != id);
                            for t in s.tracks.iter_mut() {
                                t.clips.remove(&id);
                            }
                            Ok(())
                        });
                        if let Ok(mut u) = shared.ui.lock() {
                            u.selected_section = None;
                        }
                    }
                    if ui.small_button("▶ move").clicked() {
                        let mut g = shared.lock_store();
                        let _ = g.mutate(|s| {
                            if let Some(i) = s.sections.iter().position(|x| x.id == id) {
                                if i + 1 < s.sections.len() {
                                    s.sections.swap(i, i + 1);
                                }
                            }
                            Ok(())
                        });
                    }
                    if ui.small_button("◀ move").clicked() {
                        let mut g = shared.lock_store();
                        let _ = g.mutate(|s| {
                            if let Some(i) = s.sections.iter().position(|x| x.id == id) {
                                if i > 0 {
                                    s.sections.swap(i, i - 1);
                                }
                            }
                            Ok(())
                        });
                    }
                    if ui.small_button("duplicate").clicked() {
                        let mut g = shared.lock_store();
                        let new_name = format!("{} 2", sec.name);
                        let _ = g.mutate(|s| {
                            let new_id = s.add_section(&new_name, sec.bars, sec.energy);
                            let src = s.section(&id).cloned().unwrap();
                            s.section_mut(&new_id).unwrap().chords = src.chords.clone();
                            s.section_mut(&new_id).unwrap().lyrics = src.lyrics.clone();
                            for t in s.tracks.iter_mut() {
                                if let Some(c) = t.clips.get(&id).cloned() {
                                    t.clips.insert(new_id.clone(), c);
                                }
                            }
                            Ok(())
                        });
                    }
                });
            });
        }
    }
}

fn transport(ui: &mut egui::Ui, st: &mut EditorState, shared: &Shared) {
    let ui_state = shared.ui.lock().map(|u| u.clone()).unwrap_or_default();
    ui.horizontal(|ui| {
        let playing = shared.playing.load(Ordering::Relaxed);
        let sync = shared.sync_to_host.load(Ordering::Relaxed);
        if ui.add_enabled(!sync, egui::Button::new(if playing { "■ Stop" } else { "▶ Play" }).min_size(egui::vec2(70.0, 24.0))).clicked() {
            if playing {
                shared.playing.store(false, Ordering::Relaxed);
            } else {
                shared.seek_tick.store(shared.playback.load().loop_start, Ordering::Release);
                shared.playing.store(true, Ordering::Relaxed);
            }
        }
        let mut s = sync;
        if ui.checkbox(&mut s, "Sync to host").on_hover_text("Follow FL Studio's transport and position").changed() {
            shared.sync_to_host.store(s, Ordering::Relaxed);
            shared.playing.store(false, Ordering::Relaxed);
            shared.panic.store(true, Ordering::Relaxed);
        }
        let mut loop_sec = ui_state.loop_section;
        if ui.checkbox(&mut loop_sec, "Loop section").on_hover_text("Loop only the selected section (else the whole song)").changed() {
            if let Ok(mut u) = shared.ui.lock() {
                u.loop_section = loop_sec;
            }
            shared.rebuild_playback();
        }
        let ph = shared.playhead_tick.load(Ordering::Relaxed);
        let bar = shared.lock_store().session.bar_ticks().max(1);
        ui.label(RichText::new(format!("{}.{}", ph / bar + 1, (ph % bar) / flvstx_core::PPQ + 1)).monospace());
        if ui.small_button("Panic").clicked() {
            shared.panic.store(true, Ordering::Relaxed);
        }
        ui.separator();
        // Track list.
        for role in TrackRole::ALL {
            let (locked, muted) = {
                let g = shared.lock_store();
                let t = g.session.track(role);
                (t.locked, t.muted)
            };
            let sel = ui_state.selected_track == role;
            let btn = egui::Button::new(RichText::new(role.name()).color(if sel { Color32::BLACK } else { track_color(role) })).fill(if sel { track_color(role) } else { Color32::from_rgb(40, 42, 48) });
            if ui.add(btn).clicked() {
                if let Ok(mut u) = shared.ui.lock() {
                    u.selected_track = role;
                }
                st.piano.selection.clear();
            }
            let mut m = muted;
            if ui.add(egui::SelectableLabel::new(m, "M")).on_hover_text("mute").clicked() {
                m = !m;
                let mut g = shared.lock_store();
                let _ = g.mutate(|s| {
                    s.track_mut(role).muted = m;
                    Ok(())
                });
            }
            let mut l = locked;
            if ui.add(egui::SelectableLabel::new(l, "L")).on_hover_text("lock (generators skip it)").clicked() {
                l = !l;
                let mut g = shared.lock_store();
                let _ = g.mutate(|s| {
                    s.track_mut(role).locked = l;
                    Ok(())
                });
            }
            ui.add_space(4.0);
        }
        ui.separator();
        let sec = ui_state.selected_section.clone();
        if ui.add_enabled(sec.is_some(), egui::Button::new("Generate all")).on_hover_text("Rule engine: chords, melody, bass, drums for the selected section").clicked() {
            let mut g = shared.lock_store();
            let seed = g.session.seed.wrapping_add(g.revision);
            match dispatch(&mut g, "generate_all", &serde_json::json!({ "section": sec.clone().unwrap(), "params": { "seed": seed } })) {
                Ok(_) => {}
                Err(e) => shared.push_chat(ChatRole::System, format!("generate: {e}")),
            }
        }
        if ui.add_enabled(sec.is_some(), egui::Button::new(format!("Regen {}", ui_state.selected_track.name()))).clicked() {
            let mut g = shared.lock_store();
            let seed = g.session.seed.wrapping_add(g.revision);
            match dispatch(&mut g, "generate", &serde_json::json!({ "track": ui_state.selected_track.name(), "section": sec.clone().unwrap(), "params": { "seed": seed } })) {
                Ok(_) => {}
                Err(e) => shared.push_chat(ChatRole::System, format!("generate: {e}")),
            }
        }
        if ui.button("Undo").clicked() {
            shared.lock_store().undo();
        }
        if ui.button("Redo").clicked() {
            shared.lock_store().redo();
        }
        ui.separator();
        if ui.button("Export section").on_hover_text("Writes latest.json/.mid for the FLVSTX Import piano-roll script").clicked() {
            let mut g = shared.lock_store();
            match dispatch(&mut g, "export", &serde_json::json!({ "section": sec })) {
                Ok(v) => shared.push_chat(ChatRole::System, format!("exported: {}", v["files"].as_array().map(|a| a.len()).unwrap_or(0).to_string() + " files. In FL: piano roll > Tools > Scripts > FLVSTX Import")),
                Err(e) => shared.push_chat(ChatRole::System, format!("export failed: {e}")),
            }
        }
        if ui.button("Export song").clicked() {
            let mut g = shared.lock_store();
            match dispatch(&mut g, "export", &serde_json::json!({})) {
                Ok(_) => shared.push_chat(ChatRole::System, "exported whole song to %LOCALAPPDATA%\\FLVSTX\\export (latest.mid + per-track .mid)"),
                Err(e) => shared.push_chat(ChatRole::System, format!("export failed: {e}")),
            }
        }
        if ui.button("Open folder").clicked() {
            let dir = flvstx_core::midi::default_export_dir();
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::process::Command::new("explorer").arg(&dir).spawn();
        }
    });
}
