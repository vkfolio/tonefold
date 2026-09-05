//! egui editor: layout, top bar (key/tempo/style), arrangement strip, transport, layer list,
//! arrangement grid, per-layer suggestions with takes, agent connection management, persistence sync.

mod chat;
mod piano_roll;

use crate::state::{ChatRole, Persisted, Shared};
use crate::FlvstxParams;
use flvstx_core::generate::{generate_track, GenParams};
use flvstx_core::ops::dispatch;
use flvstx_core::theory::{ScaleKind, NOTE_NAMES_SHARP};
use flvstx_core::{ChordEvent, Clip, ClipSource, Note};
use flvstx_core::{Key, SectionRole, TrackRole};
use flvstx_ipc::{AgentClient, AgentEvent, DEFAULT_PORT};
use nih_plug::prelude::Editor;
use nih_plug_egui::create_egui_editor;
use nih_plug_egui::egui::{self, Color32, RichText};
use nih_plug_egui::resizable_window::ResizableWindow;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

pub fn track_color(kind: TrackRole) -> Color32 {
    match kind {
        TrackRole::Chords => Color32::from_rgb(120, 170, 255),
        TrackRole::Pad => Color32::from_rgb(150, 140, 255),
        TrackRole::Arpeggio => Color32::from_rgb(110, 210, 240),
        TrackRole::Pluck => Color32::from_rgb(90, 200, 200),
        TrackRole::Melody => Color32::from_rgb(255, 190, 90),
        TrackRole::CounterMelody => Color32::from_rgb(255, 160, 140),
        TrackRole::Harmony => Color32::from_rgb(240, 210, 140),
        TrackRole::Bass => Color32::from_rgb(150, 230, 140),
        TrackRole::Sub => Color32::from_rgb(110, 190, 110),
        TrackRole::Drums => Color32::from_rgb(240, 120, 140),
        TrackRole::Percussion => Color32::from_rgb(230, 150, 200),
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
    /// Text/widget scale (1.0 = egui default). Defaults from the Windows DPI.
    pub ui_scale: f32,
    applied_scale: f32,
    /// Alternative takes for the last "Suggest" (layered workflow).
    takes: Vec<Take>,
    take_track: Option<String>,
    take_section: String,
    take_idx: usize,
    take_seed: u64,
    /// Chat model: "default", "sonnet", "opus".
    pub model: String,
    show_arrangement: bool,
    add_kind: TrackRole,
    /// Output channel chosen in the top bar this frame (applied through the param setter).
    pending_output: Option<i32>,
    /// Edit buffers for single-line text fields (committed when the field loses focus).
    style_draft: String,
    style_focused: bool,
    name_draft: String,
    name_focused: bool,
    pub md_cache: egui_commonmark::CommonMarkCache,
}

/// One alternative produced by "Suggest": optional chords (for chord layers) plus the notes.
#[derive(Debug, Clone)]
struct Take {
    label: String,
    chords: Option<Vec<ChordEvent>>,
    notes: Vec<Note>,
}

#[cfg(windows)]
fn system_dpi_scale() -> f32 {
    #[link(name = "user32")]
    extern "system" {
        fn GetDpiForSystem() -> u32;
    }
    // SAFETY: plain Win32 call with no arguments.
    let dpi = unsafe { GetDpiForSystem() };
    // DPI-unaware hosts report 96; assume a typical 150% laptop display then (the picker persists the user's choice).
    if dpi <= 96 { 1.5 } else { (dpi as f32 / 96.0).clamp(1.0, 2.5) }
}
#[cfg(not(windows))]
fn system_dpi_scale() -> f32 {
    1.0
}

/// Scales fonts and spacing (instead of pixels_per_point, which the host wrapper owns).
pub fn apply_ui_scale(ctx: &egui::Context, scale: f32) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    let base: [(egui::TextStyle, f32); 5] = [
        (egui::TextStyle::Small, 10.0),
        (egui::TextStyle::Body, 13.0),
        (egui::TextStyle::Button, 13.0),
        (egui::TextStyle::Heading, 18.0),
        (egui::TextStyle::Monospace, 12.5),
    ];
    for (ts, size) in base {
        if let Some(f) = style.text_styles.get_mut(&ts) {
            f.size = size * scale;
        }
    }
    style.spacing.item_spacing = egui::vec2(6.0, 4.0) * scale;
    style.spacing.button_padding = egui::vec2(5.0, 2.0) * scale;
    style.spacing.interact_size = egui::vec2(40.0, 18.0) * scale;
    style.spacing.icon_width = 14.0 * scale;
    style.spacing.combo_width = 100.0 * scale;
    ctx.set_style(style);
}

impl EditorState {
    fn setter_output(&mut self, setter: &nih_plug::prelude::ParamSetter) {
        if let Some(v) = self.pending_output.take() {
            setter.begin_set_parameter(&self.params.output);
            setter.set_parameter(&self.params.output, v);
            setter.end_set_parameter(&self.params.output);
        }
    }

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
                c.push_str(&format!("\nSelected section in the UI: {sec}\nSelected layer in the UI: {}\n", ui.selected_track));
            }
            c
        };
        let model = if self.model == "default" { None } else { Some(self.model.clone()) };
        if let Ok(g) = self.agent.lock() {
            if let Some(a) = g.as_ref() {
                a.send_user_message_with_model(text, &ctx, model.as_deref());
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

    /// Produces three alternatives for one layer, constrained by the layers that already exist
    /// (melody -> harmonize chords to it; chords -> melody over them; both -> everything else fits).
    fn suggest(&mut self, shared: &Shared, track: &str, section: &str) {
        let session = shared.lock_store().session.clone();
        let Some(sec) = session.section(section).cloned() else { return };
        let Some(t) = session.track_by(track).cloned() else { return };
        if t.locked {
            shared.push_chat(ChatRole::System, format!("{} is locked; unlock it (L) to get suggestions", t.name));
            return;
        }
        let tid = t.id.clone();
        self.take_seed = self.take_seed.wrapping_add(7);
        let base_seed = self.take_seed;
        let has_melody = !session.notes_of_kind(TrackRole::Melody, &sec.id).is_empty();
        let mut takes = Vec::new();
        let mut run = |label: String, chords: Option<Vec<ChordEvent>>, p: GenParams| {
            let mut clone = session.clone();
            if let Some(ch) = &chords {
                clone.section_mut(&sec.id).unwrap().chords = ch.clone();
            }
            clone.track_by_mut(&tid).unwrap().locked = false;
            clone.track_by_mut(&tid).unwrap().inactive.remove(&sec.id);
            if let Ok(clip) = generate_track(&mut clone, &tid, &sec.id, &p) {
                takes.push(Take { label, chords, notes: clip.notes });
            }
        };
        match t.kind {
            TrackRole::Chords => {
                let cands: Vec<(String, Vec<ChordEvent>)> = if has_melody {
                    let melody = session.notes_of_kind(TrackRole::Melody, &sec.id);
                    flvstx_core::generate::harmonize::harmonize(&session.key, &melody, sec.bars, session.bar_ticks(), None, 3)
                        .into_iter()
                        .map(|h| (format!("fits melody - {}", h.label), h.events))
                        .collect()
                } else {
                    flvstx_core::generate::chords::suggest_progressions(&session, None, 3, base_seed)
                        .into_iter()
                        .filter_map(|(sym, rom)| flvstx_core::notation::parse_chords(&sym, &session.key, sec.bars, session.bar_ticks()).ok().map(|ev| (rom, ev)))
                        .collect()
                };
                for (label, events) in cands {
                    run(label, Some(events), GenParams { seed: Some(base_seed), ..Default::default() });
                }
            }
            TrackRole::Melody => {
                for (i, contour) in ["arch", "rise", "wave"].iter().enumerate() {
                    run(format!("{contour} contour"), None, GenParams { seed: Some(base_seed + i as u64 * 13), contour: Some(contour.to_string()), ..Default::default() });
                }
            }
            TrackRole::Bass => {
                for (i, pat) in [None, Some("root5"), Some("push")].iter().enumerate() {
                    run(pat.map(|s| format!("{s} pattern")).unwrap_or_else(|| "style default".into()), None, GenParams { seed: Some(base_seed + i as u64 * 17), pattern: pat.map(|s| s.to_string()), ..Default::default() });
                }
            }
            TrackRole::Arpeggio => {
                for (i, (pat, rate)) in [("up", "16"), ("updown", "8"), ("chord", "16")].iter().enumerate() {
                    run(format!("{pat} 1/{rate}"), None, GenParams { seed: Some(base_seed + i as u64 * 23), pattern: Some(pat.to_string()), rate: Some(rate.to_string()), ..Default::default() });
                }
            }
            TrackRole::Percussion => {
                for (i, pat) in ["mixed", "shaker", "conga"].iter().enumerate() {
                    run(pat.to_string(), None, GenParams { seed: Some(base_seed + i as u64 * 29), pattern: Some(pat.to_string()), ..Default::default() });
                }
            }
            TrackRole::Harmony => {
                for (i, pat) in ["third", "sixth", "above"].iter().enumerate() {
                    run(pat.to_string(), None, GenParams { seed: Some(base_seed + i as u64 * 31), pattern: Some(pat.to_string()), ..Default::default() });
                }
            }
            _ => {
                for (i, (label, de)) in [("as is", 0.0f32), ("lighter", -0.25), ("busier", 0.25)].iter().enumerate() {
                    run(label.to_string(), None, GenParams { seed: Some(base_seed + i as u64 * 19), energy: Some((sec.energy + de).clamp(0.05, 1.0)), ..Default::default() });
                }
            }
        }
        if takes.is_empty() {
            shared.push_chat(ChatRole::System, format!("no {} suggestions could be made for {}", t.name, sec.name));
            return;
        }
        self.takes = takes;
        self.take_track = Some(tid);
        self.take_section = sec.id.clone();
        self.take_idx = 0;
        self.apply_take(shared, 0);
    }

    fn apply_take(&mut self, shared: &Shared, idx: usize) {
        let (Some(tid), Some(take)) = (self.take_track.clone(), self.takes.get(idx).cloned()) else { return };
        self.take_idx = idx;
        let section = self.take_section.clone();
        let mut g = shared.lock_store();
        let _ = g.mutate(|s| {
            if let Some(ch) = &take.chords {
                if let Some(sec) = s.section_mut(&section) {
                    sec.chords = ch.clone();
                }
            }
            if let Some(t) = s.track_by_mut(&tid) {
                t.inactive.remove(&section);
            }
            s.set_clip(&tid, &section, Clip::new(take.notes.clone(), ClipSource::Generated { seed: 0, params: serde_json::json!({ "take": take.label }) }))
        });
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
                    if let Some(sc) = p.ui_scale {
                        self.ui_scale = sc;
                    }
                    shared.load_persisted(p);
                }
            }
        }
        // Keep the selected layer valid.
        {
            let g = shared.lock_store();
            if let Ok(mut u) = shared.ui.lock() {
                if g.session.track_by(&u.selected_track).is_none() {
                    if let Some(t) = g.session.tracks.first() {
                        u.selected_track = t.id.clone();
                    }
                }
            }
        }
        let rev = shared.lock_store().revision;
        if shared.needs_rebuild() {
            shared.rebuild_playback();
        }
        if rev != self.last_persisted_revision {
            self.last_persisted_revision = rev;
            let mut p = shared.to_persisted();
            p.ui_scale = Some(self.ui_scale);
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
        piano: {
            let mut p = piano_roll::PianoRollView::default();
            p.row_h = 11.0 * system_dpi_scale();
            p
        },
        last_persisted_revision: u64::MAX,
        last_loaded_hash: 0,
        status: String::new(),
        spawn_attempted_at: None,
        ui_scale: system_dpi_scale(),
        applied_scale: 0.0,
        takes: Vec::new(),
        take_track: None,
        take_section: String::new(),
        take_idx: 0,
        take_seed: 100,
        model: "default".into(),
        show_arrangement: false,
        add_kind: TrackRole::Arpeggio,
        pending_output: None,
        style_draft: String::new(),
        style_focused: false,
        name_draft: String::new(),
        name_focused: false,
        md_cache: egui_commonmark::CommonMarkCache::default(),
    };
    create_egui_editor(
        params.editor_state.clone(),
        state,
        move |ctx, state| {
            apply_ui_scale(ctx, state.ui_scale);
            state.applied_scale = state.ui_scale;
        },
        move |ctx, setter, st| {
            st.setter_output(setter);
            if (st.applied_scale - st.ui_scale).abs() > 0.01 {
                apply_ui_scale(ctx, st.ui_scale);
                st.applied_scale = st.ui_scale;
                st.piano.row_h = 11.0 * st.ui_scale;
            }
            st.poll_agent(&shared);
            st.sync_state(&shared);
            draw(ctx, st, &shared);
            ctx.request_repaint_after(std::time::Duration::from_millis(if st.turn_active || shared.playing.load(Ordering::Relaxed) || shared.host_playing.load(Ordering::Relaxed) { 33 } else { 120 }));
        },
    )
}

fn draw(ctx: &egui::Context, st: &mut EditorState, shared: &Shared) {
    let scale = st.ui_scale;
    egui::TopBottomPanel::top("top").show(ctx, |ui| {
        top_bar(ui, st, shared);
        sections_strip(ui, st, shared);
        if st.show_arrangement {
            arrangement_grid(ui, st, shared);
        }
    });
    egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
        transport(ui, st, shared);
    });
    egui::SidePanel::left("chat").resizable(true).default_width(360.0 * scale).min_width(240.0 * scale).max_width(560.0 * scale).show(ctx, |ui| {
        chat::show(ui, st, shared);
    });
    egui::SidePanel::right("layers").resizable(true).default_width(150.0 * scale).min_width(110.0 * scale).max_width(260.0 * scale).show(ctx, |ui| {
        layers_panel(ui, st, shared);
    });
    // Central area: piano roll, with nih-plug's resize corner (the host owns the window size; the
    // corner asks the host to resize).
    let egui_state = st.params.editor_state.clone();
    let min = egui::vec2(900.0, 560.0);
    ResizableWindow::new("flvstx-window").min_size(min).show(ctx, &egui_state, |ui| {
        piano_roll::show(ui, &mut st.piano, shared);
    });
}

fn top_bar(ui: &mut egui::Ui, st: &mut EditorState, shared: &Shared) {
    let (key, tempo, style, ts) = {
        let g = shared.lock_store();
        (g.session.key, g.session.tempo, g.session.style.clone(), g.session.time_sig)
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new("FLVSTX").strong().size(18.0 * st.ui_scale));
        ui.separator();
        let mut root = key.root as usize;
        let mut scale = key.scale;
        egui::ComboBox::from_id_salt("root").width(50.0 * st.ui_scale).selected_text(NOTE_NAMES_SHARP[root]).show_ui(ui, |ui| {
            for (i, n) in NOTE_NAMES_SHARP.iter().enumerate() {
                ui.selectable_value(&mut root, i, *n);
            }
        });
        egui::ComboBox::from_id_salt("scale").width(130.0 * st.ui_scale).selected_text(scale.name()).show_ui(ui, |ui| {
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
        if !st.style_focused {
            st.style_draft = style.clone();
        }
        let resp = ui.add(egui::TextEdit::singleline(&mut st.style_draft).desired_width(110.0 * st.ui_scale).hint_text("pop, lofi, kids…"));
        st.style_focused = resp.has_focus();
        if resp.lost_focus() && st.style_draft.trim() != style {
            let new_style = st.style_draft.trim().to_string();
            let mut g = shared.lock_store();
            let _ = g.mutate(|s| {
                s.style = new_style.clone();
                Ok(())
            });
        }
        ui.separator();
        // Which layer this plugin instance sends to its MIDI output (one instance per FL instrument).
        let cur = st.params.output.value();
        let (label, choices): (String, Vec<(i32, String)>) = {
            let g = shared.lock_store();
            let mut v: Vec<(i32, String)> = vec![(0, "All layers".into())];
            let mut seen = std::collections::BTreeSet::new();
            for t in &g.session.tracks {
                if seen.insert(t.channel) {
                    let names: Vec<&str> = g.session.tracks.iter().filter(|x| x.channel == t.channel).map(|x| x.name.as_str()).collect();
                    v.push((t.channel as i32 + 1, format!("{} (ch {})", names.join(" + "), t.channel + 1)));
                }
            }
            let label = v.iter().find(|(c, _)| *c == cur).map(|(_, l)| l.clone()).unwrap_or_else(|| format!("ch {cur}"));
            (label, v)
        };
        ui.label("Send");
        egui::ComboBox::from_id_salt("send-layer").width(150.0 * st.ui_scale).selected_text(label).show_ui(ui, |ui| {
            for (c, l) in choices {
                if ui.selectable_label(cur == c, l).clicked() {
                    st.pending_output = Some(c);
                }
            }
        }).response.on_hover_text("Which layer this plugin instance plays through its MIDI output. Add one FLVSTX per instrument and pick that instrument's layer here.");
        ui.separator();
        ui.checkbox(&mut st.show_arrangement, "Arrangement").on_hover_text("Show the layers × sections grid");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let sizes = [("S", 1.0f32), ("M", 1.25), ("L", 1.5), ("XL", 1.8), ("XXL", 2.2)];
            let cur = sizes.iter().min_by(|a, b| (a.1 - st.ui_scale).abs().partial_cmp(&(b.1 - st.ui_scale).abs()).unwrap()).map(|s| s.0).unwrap_or("M");
            egui::ComboBox::from_id_salt("ui-size").width(48.0 * st.ui_scale).selected_text(format!("A {cur}")).show_ui(ui, |ui| {
                for (label, v) in sizes {
                    if ui.selectable_label((v - st.ui_scale).abs() < 0.01, label).clicked() {
                        st.ui_scale = v;
                    }
                }
            });
            let host_tempo = f32::from_bits(shared.host_tempo_bits.load(Ordering::Relaxed));
            ui.label(RichText::new(format!("host {:.0} BPM {}", host_tempo, if shared.host_playing.load(Ordering::Relaxed) { "▶" } else { "■" })).small().weak());
            if !st.status.is_empty() {
                ui.label(RichText::new(&st.status).small().weak());
            }
        });
    });
}

fn sections_strip(ui: &mut egui::Ui, st: &mut EditorState, shared: &Shared) {
    let (sections, selected) = {
        let g = shared.lock_store();
        let ui_state = shared.ui.lock().map(|u| u.clone()).unwrap_or_default();
        (g.session.sections.clone(), ui_state.selected_section)
    };
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
        let avail = ui.available_width() - 160.0 * st.ui_scale;
        let mut new_selection = None;
        for sec in &sections {
            let w = (avail * sec.bars as f32 / total_bars as f32).max(48.0 * st.ui_scale);
            let is_sel = selected.as_deref() == Some(sec.id.as_str());
            let start = shared.lock_store().session.section_start(&sec.id).unwrap_or(0);
            let in_play = playhead >= start && playhead < start + sec.bars * bar_ticks && (shared.playing.load(Ordering::Relaxed) || shared.host_playing.load(Ordering::Relaxed));
            let fill = if is_sel { Color32::from_rgb(70, 90, 130) } else { Color32::from_rgb(45, 48, 55) };
            let label = format!("{}\n{} bars · {} · e{:.0}%", sec.name, sec.bars, sec.role.name(), sec.energy * 100.0);
            let btn = egui::Button::new(RichText::new(label).small()).fill(fill).min_size(egui::vec2(w, 34.0 * st.ui_scale));
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
                let role = SectionRole::from_name(&name);
                s.add_section(&name, 8, role.default_energy());
                Ok(())
            });
        }
    });
    if let Some(id) = selected {
        if let Some(sec) = sections.iter().find(|s| s.id == id) {
            let (mut bars, mut energy, mut role) = (sec.bars, sec.energy, sec.role);
            if !st.name_focused {
                st.name_draft = sec.name.clone();
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Section").weak());
                let r1 = ui.add(egui::TextEdit::singleline(&mut st.name_draft).desired_width(120.0 * st.ui_scale));
                st.name_focused = r1.has_focus();
                let name = st.name_draft.trim().to_string();
                let roles = [SectionRole::Intro, SectionRole::Verse, SectionRole::PreChorus, SectionRole::Chorus, SectionRole::Bridge, SectionRole::Break, SectionRole::Build, SectionRole::Drop, SectionRole::Outro, SectionRole::Other];
                let mut role_changed = false;
                egui::ComboBox::from_id_salt("sec-role").width(90.0 * st.ui_scale).selected_text(role.name()).show_ui(ui, |ui| {
                    for r in roles {
                        if ui.selectable_value(&mut role, r, r.name()).changed() {
                            role_changed = true;
                        }
                    }
                });
                ui.label("bars");
                let r2 = ui.add(egui::DragValue::new(&mut bars).range(1..=128));
                ui.label("energy");
                let r3 = ui.add(egui::Slider::new(&mut energy, 0.0..=1.0).show_value(false));
                if (r1.lost_focus() && name != sec.name) || r2.changed() || role_changed || r3.drag_stopped() || (r3.changed() && !r3.dragged()) {
                    let mut g = shared.lock_store();
                    let _ = dispatch(&mut g, "set_section", &serde_json::json!({ "section": id, "name": name, "bars": bars, "energy": energy, "role": role.name() }));
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
                                t.inactive.remove(&id);
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
                            s.section_mut(&new_id).unwrap().role = src.role;
                            for t in s.tracks.iter_mut() {
                                if let Some(c) = t.clips.get(&id).cloned() {
                                    t.clips.insert(new_id.clone(), c);
                                }
                                if t.inactive.contains(&id) {
                                    t.inactive.insert(new_id.clone());
                                } else {
                                    t.inactive.remove(&new_id);
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

/// Layers × sections presence grid.
fn arrangement_grid(ui: &mut egui::Ui, st: &mut EditorState, shared: &Shared) {
    let session = shared.lock_store().session.clone();
    if session.sections.is_empty() {
        return;
    }
    let cell = 22.0 * st.ui_scale;
    egui::Grid::new("arr-grid").spacing(egui::vec2(2.0, 2.0)).show(ui, |ui| {
        ui.label(RichText::new("layer \\ section").small().weak());
        for sec in &session.sections {
            ui.label(RichText::new(&sec.name).small());
        }
        ui.end_row();
        for t in &session.tracks {
            ui.label(RichText::new(&t.name).small().color(track_color(t.kind)));
            for sec in &session.sections {
                let active = t.active_in(&sec.id);
                let has_notes = t.clips.get(&sec.id).map(|c| !c.notes.is_empty()).unwrap_or(false);
                let fill = if !active { Color32::from_rgb(40, 40, 44) } else if has_notes { track_color(t.kind) } else { Color32::from_rgb(70, 74, 82) };
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(cell * 2.0, cell * 0.8), egui::Sense::click());
                ui.painter().rect_filled(rect, 3.0, fill);
                if resp.on_hover_text(if active { "playing (click to silence)" } else { "silent (click to enable)" }).clicked() {
                    let mut g = shared.lock_store();
                    let _ = dispatch(&mut g, "set_arrangement", &serde_json::json!({ "track": t.id, "section": sec.id, "active": !active }));
                }
            }
            ui.end_row();
        }
    });
}

fn layers_panel(ui: &mut egui::Ui, st: &mut EditorState, shared: &Shared) {
    ui.heading("Layers");
    ui.separator();
    let ui_state = shared.ui.lock().map(|u| u.clone()).unwrap_or_default();
    let tracks = shared.lock_store().session.tracks.clone();
    let mut select: Option<(String, bool)> = None; // (id, additive)
    let mut solo_changed = false;
    ui.horizontal(|ui| {
        let mut solo = ui_state.solo_selected;
        if ui.checkbox(&mut solo, "Solo selected").on_hover_text("Play only the selected layers (click = select one, Ctrl+click = add more). Off = play everything.").changed() {
            if let Ok(mut u) = shared.ui.lock() {
                u.solo_selected = solo;
            }
            solo_changed = true;
        }
        if ui.small_button("All").on_hover_text("select every layer").clicked() {
            if let Ok(mut u) = shared.ui.lock() {
                u.selected_tracks = tracks.iter().map(|t| t.id.clone()).collect();
            }
            solo_changed = true;
        }
    });
    egui::ScrollArea::vertical().id_salt("layers-scroll").auto_shrink([false, false]).max_height(ui.available_height() - 70.0 * st.ui_scale).show(ui, |ui| {
        for t in &tracks {
            let primary = ui_state.selected_track == t.id;
            let sel = primary || ui_state.selected_tracks.contains(&t.id);
            ui.horizontal(|ui| {
                let fill = if primary { track_color(t.kind) } else if sel { Color32::from_rgb(70, 76, 90) } else { Color32::from_rgb(40, 42, 48) };
                let btn = egui::Button::new(RichText::new(&t.name).color(if primary { Color32::BLACK } else { track_color(t.kind) })).fill(fill).min_size(egui::vec2(70.0 * st.ui_scale, 0.0));
                let resp = ui.add(btn).on_hover_text(format!("{} · MIDI ch {}\n{}\nclick: select, Ctrl+click: add to selection", t.kind.name(), t.channel + 1, t.kind.description()));
                if resp.clicked() {
                    let additive = ui.input(|i| i.modifiers.ctrl || i.modifiers.shift);
                    select = Some((t.id.clone(), additive));
                }
                if ui.add(egui::SelectableLabel::new(t.muted, "M")).on_hover_text("mute").clicked() {
                    let mut g = shared.lock_store();
                    let id = t.id.clone();
                    let _ = g.mutate(|s| {
                        let tr = s.track_by_mut(&id).unwrap();
                        tr.muted = !tr.muted;
                        Ok(())
                    });
                }
                if ui.add(egui::SelectableLabel::new(t.locked, "L")).on_hover_text("lock (generators skip it)").clicked() {
                    let mut g = shared.lock_store();
                    let id = t.id.clone();
                    let _ = g.mutate(|s| {
                        let tr = s.track_by_mut(&id).unwrap();
                        tr.locked = !tr.locked;
                        Ok(())
                    });
                }
            });
            ui.label(RichText::new(format!("ch {}", t.channel + 1)).small().weak());
        }
    });
    if let Some((id, additive)) = select {
        if let Ok(mut u) = shared.ui.lock() {
            if additive {
                if u.selected_tracks.contains(&id) && u.selected_tracks.len() > 1 && u.selected_track != id {
                    u.selected_tracks.remove(&id);
                } else {
                    u.selected_tracks.insert(id.clone());
                    u.selected_track = id;
                }
            } else {
                u.selected_tracks.clear();
                u.selected_tracks.insert(id.clone());
                u.selected_track = id;
            }
        }
        st.piano.selection.clear();
        solo_changed = true;
    }
    if solo_changed {
        shared.rebuild_playback();
    }
    ui.separator();
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("add-kind").width(90.0 * st.ui_scale).selected_text(st.add_kind.label()).show_ui(ui, |ui| {
            for k in TrackRole::ALL {
                ui.selectable_value(&mut st.add_kind, k, k.label()).on_hover_text(k.description());
            }
        });
        if ui.button("+ Layer").clicked() {
            let mut g = shared.lock_store();
            if let Ok(v) = dispatch(&mut g, "add_layer", &serde_json::json!({ "kind": st.add_kind.name() })) {
                if let Some(id) = v["track"].as_str() {
                    if let Ok(mut u) = shared.ui.lock() {
                        u.selected_track = id.to_string();
                        u.selected_tracks.clear();
                        u.selected_tracks.insert(id.to_string());
                    }
                    shared.rebuild_playback();
                }
            }
        }
    });
    if tracks.len() > 1 && ui.small_button("✕ remove selected layer").clicked() {
        let mut g = shared.lock_store();
        let _ = dispatch(&mut g, "remove_layer", &serde_json::json!({ "track": ui_state.selected_track }));
    }
}

fn transport(ui: &mut egui::Ui, st: &mut EditorState, shared: &Shared) {
    let ui_state = shared.ui.lock().map(|u| u.clone()).unwrap_or_default();
    ui.horizontal(|ui| {
        let playing = shared.playing.load(Ordering::Relaxed);
        let sync = shared.sync_to_host.load(Ordering::Relaxed);
        if ui.add_enabled(!sync, egui::Button::new(if playing { "■ Stop" } else { "▶ Play" }).min_size(egui::vec2(70.0 * st.ui_scale, 24.0 * st.ui_scale))).clicked() {
            if playing {
                shared.playing.store(false, Ordering::Relaxed);
            } else {
                shared.request_seek(shared.playback.load().loop_start);
                shared.playing.store(true, Ordering::Relaxed);
            }
        }
        let mut s = sync;
        if ui.checkbox(&mut s, "Sync to host").on_hover_text("Follow FL Studio's transport and position").changed() {
            shared.sync_to_host.store(s, Ordering::Relaxed);
            shared.playing.store(false, Ordering::Relaxed);
            shared.request_panic();
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
            shared.request_panic();
        }
        ui.separator();
        if ui.button("Undo").clicked() {
            shared.lock_store().undo();
        }
        if ui.button("Redo").clicked() {
            shared.lock_store().redo();
        }
        ui.separator();
        let sec = ui_state.selected_section.clone();
        if ui.button("Export section").on_hover_text("Writes latest.json/.mid (one per layer) for the FLVSTX Import piano-roll script").clicked() {
            let mut g = shared.lock_store();
            match dispatch(&mut g, "export", &serde_json::json!({ "section": sec })) {
                Ok(v) => shared.push_chat(ChatRole::System, format!("exported {} files. In FL: piano roll > Tools > Scripts > FLVSTX Import", v["files"].as_array().map(|a| a.len()).unwrap_or(0))),
                Err(e) => shared.push_chat(ChatRole::System, format!("export failed: {e}")),
            }
        }
        if ui.button("Export song").clicked() {
            let mut g = shared.lock_store();
            match dispatch(&mut g, "export", &serde_json::json!({})) {
                Ok(_) => shared.push_chat(ChatRole::System, "exported whole song to %LOCALAPPDATA%\\FLVSTX\\export (latest.mid + one .mid per layer)"),
                Err(e) => shared.push_chat(ChatRole::System, format!("export failed: {e}")),
            }
        }
        if ui.button("Open folder").clicked() {
            let dir = flvstx_core::midi::default_export_dir();
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::process::Command::new("explorer").arg(&dir).spawn();
        }
    });
    ui.horizontal(|ui| {
        let sec = ui_state.selected_section.clone();
        let (track_name, locked, kind) = {
            let g = shared.lock_store();
            g.session.track_by(&ui_state.selected_track).map(|t| (t.name.clone(), t.locked, t.kind)).unwrap_or(("?".into(), true, TrackRole::Melody))
        };
        let hint = match kind {
            TrackRole::Melody => "3 melody takes over the current chords (or free if none)",
            TrackRole::Chords => "3 progressions fitted to the melody (or idiomatic ones if no melody)",
            TrackRole::Bass => "3 bass lines following the chords and leaving room for the melody",
            TrackRole::Arpeggio => "3 arp patterns over the chords",
            TrackRole::Harmony => "3 harmony intervals under the lead",
            TrackRole::Percussion => "3 percussion setups",
            _ => "3 takes at different densities",
        };
        let btn = egui::Button::new(RichText::new(format!("Suggest {track_name}")).color(if locked { Color32::GRAY } else { track_color(kind) }));
        if ui.add_enabled(sec.is_some() && !locked, btn).on_hover_text(hint).clicked() {
            let tid = ui_state.selected_track.clone();
            st.suggest(shared, &tid, sec.as_deref().unwrap());
        }
        if !st.takes.is_empty() && st.take_track.is_some() && sec.as_deref() == Some(st.take_section.as_str()) {
            ui.label(RichText::new("takes:").weak());
            for i in 0..st.takes.len() {
                let label = format!("{} - {}", i + 1, st.takes[i].label);
                if ui.add(egui::SelectableLabel::new(st.take_idx == i, label)).clicked() && st.take_idx != i {
                    st.apply_take(shared, i);
                }
            }
            if ui.small_button("More").on_hover_text("three new takes").clicked() {
                let tid = st.take_track.clone().unwrap();
                st.suggest(shared, &tid, sec.as_deref().unwrap());
            }
            if ui.small_button("Keep").on_hover_text("lock this layer so later suggestions leave it alone").clicked() {
                let tid = st.take_track.clone().unwrap();
                let mut g = shared.lock_store();
                let _ = g.mutate(|s| {
                    if let Some(t) = s.track_by_mut(&tid) {
                        t.locked = true;
                    }
                    Ok(())
                });
                st.takes.clear();
                st.take_track = None;
            }
        }
        ui.separator();
        if ui.add_enabled(sec.is_some(), egui::Button::new("Generate section")).on_hover_text("Rule engine: every unlocked, active layer of the selected section").clicked() {
            let mut g = shared.lock_store();
            let seed = g.session.seed.wrapping_add(g.revision);
            if let Err(e) = dispatch(&mut g, "generate_all", &serde_json::json!({ "section": sec.clone().unwrap(), "params": { "seed": seed } })) {
                shared.push_chat(ChatRole::System, format!("generate: {e}"));
            }
        }
        if ui.button("Generate song").on_hover_text("Every section, every unlocked layer, with continuity and section contrast").clicked() {
            let mut g = shared.lock_store();
            let seed = g.session.seed.wrapping_add(g.revision);
            if let Err(e) = dispatch(&mut g, "generate_song", &serde_json::json!({ "params": { "seed": seed } })) {
                shared.push_chat(ChatRole::System, format!("generate song: {e}"));
            }
        }
    });
}
