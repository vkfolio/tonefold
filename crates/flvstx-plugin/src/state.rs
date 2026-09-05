//! Shared plugin state: the session store (behind a mutex, served to the agent and edited by the GUI),
//! the lock-free playback buffer consumed by the audio thread, chat transcript, and persistence.

use arc_swap::ArcSwap;
use flvstx_core::ops::Store;
use flvstx_core::{Note, Session, PPQ};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
#[allow(unused_imports)]
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

/// One scheduled MIDI event in song ticks.
#[derive(Debug, Clone, Copy)]
pub struct Event {
    pub tick: u32,
    pub on: bool,
    pub channel: u8,
    pub pitch: u8,
    pub vel: f32,
}

/// Pre-rendered, sorted event list. Replaced atomically whenever the session changes.
#[derive(Debug, Default)]
pub struct PlaybackBuffer {
    pub events: Vec<Event>,
    pub loop_start: u32,
    pub loop_end: u32,
    pub tempo: f32,
}

impl PlaybackBuffer {
    pub fn build(session: &Session, loop_section: Option<&str>, muted: &[String]) -> PlaybackBuffer {
        let mut events = Vec::new();
        let bar = session.bar_ticks();
        let (loop_start, loop_end) = match loop_section.and_then(|id| session.section(id).map(|s| (session.section_start(&s.id).unwrap_or(0), s.bars * bar))) {
            Some((start, len)) => (start, start + len),
            None => (0, session.total_ticks().max(bar)),
        };
        for t in &session.tracks {
            if muted.contains(&t.id) || t.muted {
                continue;
            }
            let ch = t.channel;
            for n in session.flatten(&t.id) {
                if n.start >= loop_end || n.end() <= loop_start {
                    continue;
                }
                events.push(Event { tick: n.start, on: true, channel: ch, pitch: n.pitch, vel: n.vel });
                events.push(Event { tick: n.end().min(loop_end.saturating_sub(1)).max(n.start + 1), on: false, channel: ch, pitch: n.pitch, vel: 0.0 });
            }
        }
        // Note-offs before note-ons at the same tick so retriggers work.
        events.sort_by_key(|e| (e.tick, e.on));
        PlaybackBuffer { events, loop_start, loop_end, tempo: session.tempo }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    User,
    Assistant,
    Tool,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatLine {
    pub role: ChatRole,
    pub text: String,
}

/// Everything persisted in the DAW project.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Persisted {
    pub session: Session,
    #[serde(default)]
    pub chat: Vec<ChatLine>,
    #[serde(default)]
    pub agent_session_id: Option<String>,
    #[serde(default)]
    pub selected_section: Option<String>,
    #[serde(default)]
    pub selected_track: Option<String>,
    #[serde(default)]
    pub ui_scale: Option<f32>,
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
    pub chat: Mutex<Vec<ChatLine>>,
    pub agent_session_id: Mutex<Option<String>>,
    pub ui: Mutex<UiState>,
}

#[derive(Debug, Clone)]
pub struct UiState {
    pub selected_section: Option<String>,
    /// Selected layer id.
    pub selected_track: String,
    pub loop_section: bool,
    pub muted: Vec<String>,
}

impl Default for UiState {
    fn default() -> Self {
        UiState { selected_section: None, selected_track: "melody".into(), loop_section: true, muted: Vec::new() }
    }
}

/// Every FLVSTX instance in the same host process shares one song, so one instance per FL instrument
/// (each with its own MIDI output channel) all show and play the same session.
pub fn global_shared() -> Arc<Shared> {
    static GLOBAL: std::sync::OnceLock<Arc<Shared>> = std::sync::OnceLock::new();
    GLOBAL.get_or_init(|| Shared::new(Session::default())).clone()
}

impl Shared {
    pub fn request_seek(&self, tick: u32) {
        let epoch = (self.seek.load(Ordering::Relaxed) >> 32) + 1;
        self.seek.store((epoch << 32) | tick as u64, Ordering::Release);
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
            chat: Mutex::new(Vec::new()),
            agent_session_id: Mutex::new(None),
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
        let buf = PlaybackBuffer::build(&session, loop_section.as_deref(), &ui.muted);
        self.playback.store(Arc::new(buf));
        self.built_revision.store(revision, Ordering::Release);
    }

    pub fn needs_rebuild(&self) -> bool {
        let rev = self.lock_store().revision;
        self.built_revision.load(Ordering::Acquire) != rev
    }

    pub fn push_chat(&self, role: ChatRole, text: impl Into<String>) {
        if let Ok(mut c) = self.chat.lock() {
            c.push(ChatLine { role, text: text.into() });
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
            agent_session_id: self.agent_session_id.lock().ok().and_then(|s| s.clone()),
            selected_section: ui.selected_section,
            selected_track: Some(ui.selected_track.clone()),
            ui_scale: None,
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
        if let Ok(mut s) = self.agent_session_id.lock() {
            *s = p.agent_session_id;
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
    (tempo as f64 / 60.0) * PPQ as f64 / sample_rate as f64
}
