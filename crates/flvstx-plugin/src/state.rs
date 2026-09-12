//! Shared plugin state: the session store (behind a mutex, served to the agent and edited by the GUI),
//! the lock-free playback buffer consumed by the audio thread, chat transcript, and persistence.

use arc_swap::ArcSwap;
use flvstx_core::ops::Store;
use flvstx_core::{Note, Session};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
#[allow(unused_imports)]
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

pub use flvstx_core::playback::Event;

/// Pre-rendered, sorted event list. Replaced atomically whenever the session changes.
#[derive(Debug, Default)]
pub struct PlaybackBuffer {
    pub events: Vec<Event>,
    pub loop_start: u32,
    pub loop_end: u32,
    pub tempo: f32,
    /// General MIDI program per synth channel (128 = drums) at build time.
    pub programs: [u8; 16],
    /// Pitch-bend range in semitones per synth channel.
    pub bend_ranges: [u8; 16],
}

impl PlaybackBuffer {
    pub fn build(session: &Session, loop_section: Option<&str>, muted: &[String]) -> PlaybackBuffer {
        let tl = flvstx_core::playback::timeline(session, loop_section, muted);
        PlaybackBuffer { events: tl.events, loop_start: tl.loop_start, loop_end: tl.loop_end, tempo: tl.tempo, programs: tl.programs, bend_ranges: tl.bend_ranges }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    User,
    Assistant,
    Tool,
    System,
    /// The model's reasoning before an answer (a thinking model on Ollama; Claude's summaries).
    Thinking,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatLine {
    pub role: ChatRole,
    pub text: String,
    /// Which specialist said it, when the producer delegated. None = the producer, or the composer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

/// Everything persisted in the DAW project.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Persisted {
    pub session: Session,
    #[serde(default)]
    pub chat: Vec<ChatLine>,
    /// Pre-1.2 projects: the one composer conversation. Read on load, never written again.
    #[serde(default)]
    pub agent_session_id: Option<String>,
    /// The conversation to resume per mode ("composer", "producer").
    #[serde(default)]
    pub agent_sessions: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub selected_section: Option<String>,
    #[serde(default)]
    pub selected_track: Option<String>,
    #[serde(default)]
    pub ui_scale: Option<f32>,
    /// Which provider, server and model the chat header was set to, so a project reopens on the
    /// same backend it was written with.
    #[serde(default)]
    pub backend: Option<flvstx_ipc::Backend>,
}

/// Shared between GUI, IPC thread and (read-only, lock-free parts) the audio thread.
pub struct Shared {
    pub store: Arc<Mutex<Store>>,
    pub playback: Arc<ArcSwap<PlaybackBuffer>>,
    /// Transport commands from the GUI.
    pub playing: AtomicBool,
    pub sync_to_host: AtomicBool,
    pub loop_enabled: AtomicBool,
    /// Current playhead position in ticks (written by audio thread for the GUI).
    pub playhead_tick: AtomicU32,
    /// Host info mirrored for the GUI.
    pub host_tempo_bits: AtomicU32,
    pub host_playing: AtomicBool,
    /// Seek request: (epoch << 32) | tick. Every plugin instance tracks the last epoch it consumed,
    /// so all instances sharing this state seek together.
    pub seek: AtomicU64,
    /// Note preview from the piano roll: (epoch << 32) | (channel<<16 | pitch<<8 | vel).
    pub preview: AtomicU64,
    /// Session revision the playback buffer was built from.
    pub built_revision: AtomicU64,
    /// All-notes-off request epoch.
    pub panic: AtomicU64,
    /// Hash of the persisted JSON last loaded into this state (so several instances don't reload it).
    pub loaded_hash: AtomicU64,
    /// The loaded soundfont for the built-in synth (shared by all instances).
    pub soundfont: Mutex<Option<Arc<rustysynth::SoundFont>>>,
    /// Status text for the GUI ("loading…", "no soundfont", "GeneralUser GS").
    pub soundfont_status: Mutex<String>,
    /// Instance id that renders the built-in audio (0 = none yet); avoids doubled sound with several instances.
    pub audio_owner: AtomicU64,
    pub chat: Mutex<Vec<ChatLine>>,
    pub agent_sessions: Mutex<std::collections::HashMap<String, String>>,
    pub ui: Mutex<UiState>,
    /// The producer's current plan: proposed and awaiting an answer, or approved and running.
    pub plan: Mutex<Option<PlanState>>,
    /// The producer's checklist while a run is going. Not persisted: it belongs to a live turn.
    pub todos: Mutex<Vec<flvstx_ipc::TodoItem>>,
}

/// A proposed plan plus where it is. Kept on `Shared` (not the editor) so it survives a project
/// reload and every plugin instance sees the same one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanState {
    pub id: String,
    pub plan: flvstx_ipc::Plan,
    /// "awaiting_approval", "executing", "done", "sent_back", "cancelled".
    pub status: String,
    /// The session as it was when this plan was approved, so the whole run can be put back.
    #[serde(default)]
    pub checkpoint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UiState {
    pub selected_section: Option<String>,
    /// Selected layer id (the one shown in the piano roll).
    pub selected_track: String,
    /// All selected layer ids (Ctrl+click adds); playback solos these when `solo_selected` is on.
    pub selected_tracks: std::collections::BTreeSet<String>,
    /// Play only the selected layers. Off = play everything not muted.
    pub solo_selected: bool,
    pub loop_section: bool,
    pub muted: Vec<String>,
    /// Layer ids that were dragged into FL (shown with a tick).
    pub written: std::collections::BTreeSet<String>,
}

impl Default for UiState {
    fn default() -> Self {
        UiState { selected_section: None, selected_track: "melody".into(), selected_tracks: std::collections::BTreeSet::new(), solo_selected: false, loop_section: true, muted: Vec::new(), written: std::collections::BTreeSet::new() }
    }
}

/// Every FLVSTX instance in the same host process shares one song, so one instance per FL instrument
/// (each with its own MIDI output channel) all show and play the same session.
pub fn global_shared() -> Arc<Shared> {
    static GLOBAL: std::sync::OnceLock<Arc<Shared>> = std::sync::OnceLock::new();
    GLOBAL.get_or_init(|| Shared::new(Session::default())).clone()
}

/// Default soundfont location: `%LOCALAPPDATA%\FLVSTX\soundfont\GeneralUser-GS.sf2` (or `FLVSTX_SOUNDFONT`).
pub fn soundfont_path() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os("FLVSTX_SOUNDFONT") {
        return p.into();
    }
    flvstx_core::midi::default_export_dir().parent().map(|p| p.join("soundfont").join("GeneralUser-GS.sf2")).unwrap_or_default()
}

impl Shared {
    /// Loads the soundfont on a background thread (once).
    pub fn load_soundfont_async(self: &Arc<Self>) {
        if self.soundfont.lock().map(|s| s.is_some()).unwrap_or(false) {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            let path = soundfont_path();
            *me.soundfont_status.lock().unwrap() = "soundfont: loading…".into();
            match std::fs::File::open(&path).map_err(|e| e.to_string()).and_then(|mut f| rustysynth::SoundFont::new(&mut f).map_err(|e| e.to_string())) {
                Ok(sf) => {
                    let name = sf.get_info().get_bank_name().to_string();
                    *me.soundfont.lock().unwrap() = Some(Arc::new(sf));
                    *me.soundfont_status.lock().unwrap() = format!("sound: {}", if name.is_empty() { "soundfont".into() } else { name });
                }
                Err(e) => {
                    *me.soundfont_status.lock().unwrap() = format!("no soundfont ({}): run scripts\\install.ps1", e);
                }
            }
        });
    }

    pub fn request_seek(&self, tick: u32) {
        let epoch = (self.seek.load(Ordering::Relaxed) >> 32) + 1;
        self.seek.store((epoch << 32) | tick as u64, Ordering::Release);
    }
    /// Moves the playhead to `tick` (both the marker the GUI draws and the audio thread's position),
    /// so playback continues — or starts — from there.
    pub fn seek_to(&self, tick: u32) {
        self.playhead_tick.store(tick, Ordering::Relaxed);
        self.request_seek(tick);
    }
    pub fn request_panic(&self) {
        self.panic.fetch_add(1, Ordering::AcqRel);
    }
    pub fn request_preview(&self, channel: u8, pitch: u8, vel_midi: u8) {
        let epoch = (self.preview.load(Ordering::Relaxed) >> 32) + 1;
        let v = ((channel as u64) << 16) | ((pitch as u64) << 8) | vel_midi.max(1) as u64;
        self.preview.store((epoch << 32) | v, Ordering::Release);
    }

    pub fn new(session: Session) -> Arc<Shared> {
        let store = Arc::new(Mutex::new(Store::new(session)));
        let shared = Shared {
            store,
            playback: Arc::new(ArcSwap::from_pointee(PlaybackBuffer::default())),
            playing: AtomicBool::new(false),
            sync_to_host: AtomicBool::new(false),
            loop_enabled: AtomicBool::new(true),
            playhead_tick: AtomicU32::new(0),
            host_tempo_bits: AtomicU32::new(120f32.to_bits()),
            host_playing: AtomicBool::new(false),
            seek: AtomicU64::new(0),
            preview: AtomicU64::new(0),
            built_revision: AtomicU64::new(u64::MAX),
            panic: AtomicU64::new(0),
            loaded_hash: AtomicU64::new(0),
            soundfont: Mutex::new(None),
            soundfont_status: Mutex::new("soundfont: not loaded".into()),
            audio_owner: AtomicU64::new(0),
            chat: Mutex::new(Vec::new()),
            agent_sessions: Mutex::new(Default::default()),
            plan: Mutex::new(None),
            todos: Mutex::new(Vec::new()),
            ui: Mutex::new(UiState::default()),
        };
        let shared = Arc::new(shared);
        shared.rebuild_playback();
        shared
    }

    pub fn lock_store(&self) -> std::sync::MutexGuard<'_, Store> {
        match self.store.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }

    /// Rebuilds the playback buffer from the current session (call from GUI/IPC threads only).
    pub fn rebuild_playback(&self) {
        let (session, revision) = {
            let g = self.lock_store();
            (g.session.clone(), g.revision)
        };
        let ui = self.ui.lock().map(|u| u.clone()).unwrap_or_default();
        let loop_section = if ui.loop_section { ui.selected_section.clone() } else { None };
        let mut muted = ui.muted.clone();
        if ui.solo_selected && !ui.selected_tracks.is_empty() {
            for t in &session.tracks {
                if !ui.selected_tracks.contains(&t.id) {
                    muted.push(t.id.clone());
                }
            }
        }
        let buf = PlaybackBuffer::build(&session, loop_section.as_deref(), &muted);
        self.playback.store(Arc::new(buf));
        self.built_revision.store(revision, Ordering::Release);
    }

    pub fn needs_rebuild(&self) -> bool {
        let rev = self.lock_store().revision;
        self.built_revision.load(Ordering::Acquire) != rev
    }

    pub fn push_chat(&self, role: ChatRole, text: impl Into<String>) {
        self.push_chat_from(role, text, None);
    }

    /// Same, but attributed to a specialist.
    pub fn push_chat_from(&self, role: ChatRole, text: impl Into<String>, agent: Option<String>) {
        if let Ok(mut c) = self.chat.lock() {
            c.push(ChatLine { role, text: text.into(), agent });
            if c.len() > 400 {
                let drop = c.len() - 400;
                c.drain(0..drop);
            }
        }
    }

    pub fn to_persisted(&self) -> Persisted {
        let ui = self.ui.lock().map(|u| u.clone()).unwrap_or_default();
        Persisted {
            session: self.lock_store().session.clone(),
            chat: self.chat.lock().map(|c| c.clone()).unwrap_or_default(),
            agent_session_id: None,
            agent_sessions: self.agent_sessions.lock().map(|s| s.clone()).unwrap_or_default(),
            selected_section: ui.selected_section,
            selected_track: Some(ui.selected_track.clone()),
            ui_scale: None,
            backend: None,
        }
    }

    pub fn load_persisted(&self, p: Persisted) {
        {
            let mut g = self.lock_store();
            *g = Store::new(p.session);
            g.revision += 1;
        }
        if let Ok(mut c) = self.chat.lock() {
            *c = p.chat;
        }
        if let Ok(mut s) = self.agent_sessions.lock() {
            *s = p.agent_sessions;
            // A project saved before producer mode existed carries one id: it is the composer's.
            if let Some(old) = p.agent_session_id {
                s.entry("composer".into()).or_insert(old);
            }
        }
        if let Ok(mut u) = self.ui.lock() {
            u.selected_section = p.selected_section;
            if let Some(t) = p.selected_track {
                u.selected_track = t;
            }
        }
        self.rebuild_playback();
    }

    /// Marks a clip as hand-edited and bumps the revision (used by the piano roll).
    pub fn edit_notes(&self, track: &str, section: &str, f: impl FnOnce(&mut Vec<Note>)) {
        let mut g = self.lock_store();
        let _ = g.mutate(|s| {
            let id = s.section(section).map(|x| x.id.clone()).ok_or_else(|| flvstx_core::Error::UnknownSection(section.into()))?;
            let track = s.track_by_mut(track).ok_or_else(|| flvstx_core::Error::UnknownTrack(track.into()))?;
            let clip = track.clips.entry(id).or_insert_with(|| flvstx_core::Clip::new(Vec::new(), flvstx_core::ClipSource::Edited));
            f(&mut clip.notes);
            clip.source = flvstx_core::ClipSource::Edited;
            clip.sort();
            Ok(())
        });
    }
}

pub fn ticks_per_sample(tempo: f32, sample_rate: f32) -> f64 {
    flvstx_core::playback::ticks_per_sample(tempo, sample_rate)
}

#[cfg(test)]
mod compat_tests {
    use super::*;

    /// A project saved before producer mode must still open: every field added since is optional,
    /// and the one conversation such a project remembers is the composer's.
    #[test]
    fn a_pre_producer_project_still_loads() {
        let old = serde_json::json!({
            "session": flvstx_core::Session::default(),
            "chat": [
                { "role": "user", "text": "make it warmer" },
                { "role": "assistant", "text": "done" }
            ],
            "agent_session_id": "sess-old",
            "selected_section": "verse",
            "ui_scale": 1.5
        });
        let p: Persisted = serde_json::from_value(old).expect("an old project must still load");
        assert_eq!(p.chat.len(), 2);
        assert!(p.chat.iter().all(|l| l.agent.is_none()));
        assert!(p.agent_sessions.is_empty());

        let shared = Shared::new(flvstx_core::Session::default());
        shared.load_persisted(p);
        assert_eq!(shared.agent_sessions.lock().unwrap().get("composer").map(String::as_str), Some("sess-old"));
        assert!(shared.plan.lock().unwrap().is_none());
        assert!(shared.todos.lock().unwrap().is_empty());

        // ...and what it saves now carries the map instead.
        let round = shared.to_persisted();
        assert_eq!(round.agent_session_id, None);
        assert_eq!(round.agent_sessions.get("composer").map(String::as_str), Some("sess-old"));
    }
}
