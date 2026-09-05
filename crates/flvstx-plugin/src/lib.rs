//! FLVSTX — Claude-powered MIDI composer plugin (CLAP + VST3) for FL Studio.
//!
//! Audio thread: reads the lock-free playback buffer and emits note events synced to the host
//! transport (or an internal clock). GUI thread: egui editor (chat, arrangement, piano roll).
//! IPC thread: WebSocket bridge to the agent sidecar, serving tool calls against the session store.

pub mod editor;
pub mod state;

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use state::{Persisted, Shared};
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};

pub struct Flvstx {
    params: Arc<FlvstxParams>,
    shared: Arc<Shared>,
    sample_rate: f32,
    /// Internal clock position in ticks (fractional) when not synced to the host.
    pos_ticks: f64,
    /// Index into the playback buffer's events for the next event to emit.
    next_event: usize,
    was_playing: bool,
    /// Notes currently sounding (channel, pitch) so we can send note-offs on stop/loop.
    sounding: Vec<(u8, u8)>,
    last_loaded_state: u64,
    preview_off_at: Option<(u32, u8, u8)>,
}

/// Which MIDI channel this instance sends. Every layer has its own channel (shown in the layer list),
/// so run one instance per FL instrument and pick that layer's channel here; "All" sends everything.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputSelect {
    #[id = "all"]
    All,
    #[id = "ch1"] Ch1, #[id = "ch2"] Ch2, #[id = "ch3"] Ch3, #[id = "ch4"] Ch4,
    #[id = "ch5"] Ch5, #[id = "ch6"] Ch6, #[id = "ch7"] Ch7, #[id = "ch8"] Ch8,
    #[id = "ch9"] Ch9, #[id = "ch10"] Ch10, #[id = "ch11"] Ch11, #[id = "ch12"] Ch12,
    #[id = "ch13"] Ch13, #[id = "ch14"] Ch14, #[id = "ch15"] Ch15, #[id = "ch16"] Ch16,
}

impl OutputSelect {
    fn allows(self, channel: u8) -> bool {
        match self {
            OutputSelect::All => true,
            other => (other as usize as u8) == channel + 1,
        }
    }
}

#[derive(Params)]
pub struct FlvstxParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,
    /// MIDI output track filter (see [`OutputSelect`]).
    #[id = "output"]
    pub output: EnumParam<OutputSelect>,
    /// The whole session + chat as JSON (kept in sync by the GUI thread).
    #[persist = "flvstx-state"]
    state_json: Arc<RwLock<String>>,
}

impl Default for Flvstx {
    fn default() -> Self {
        Self {
            params: Arc::new(FlvstxParams {
                editor_state: EguiState::from_size(1500, 900),
                output: EnumParam::new("MIDI output", OutputSelect::All),
                state_json: Arc::new(RwLock::new(String::new())),
            }),
            shared: Shared::new(flvstx_core::Session::default()),
            sample_rate: 44100.0,
            pos_ticks: 0.0,
            next_event: 0,
            was_playing: false,
            sounding: Vec::with_capacity(64),
            last_loaded_state: 0,
            preview_off_at: None,
        }
    }
}

fn hash_str(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

impl Plugin for Flvstx {
    const NAME: &'static str = "FLVSTX";
    const VENDOR: &'static str = "FLVSTX";
    const URL: &'static str = "https://github.com/";
    const EMAIL: &'static str = "vigneshaiml@gmail.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];
    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.shared.clone())
    }

    fn initialize(&mut self, _layout: &AudioIOLayout, buffer_config: &BufferConfig, _context: &mut impl InitContext<Self>) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        // Restore persisted state (project load). The GUI also does this, but the editor may not be open.
        let json = self.params.state_json.read().map(|s| s.clone()).unwrap_or_default();
        let h = hash_str(&json);
        if !json.is_empty() && h != self.last_loaded_state {
            if let Ok(p) = serde_json::from_str::<Persisted>(&json) {
                self.shared.load_persisted(p);
            }
            self.last_loaded_state = h;
        }
        true
    }

    fn reset(&mut self) {
        self.next_event = 0;
        self.sounding.clear();
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, context: &mut impl ProcessContext<Self>) -> ProcessStatus {
        let (host_playing, host_tempo, host_pos_beats) = {
            let t = context.transport();
            (t.playing, t.tempo, t.pos_beats())
        };
        let shared = &self.shared;
        shared.host_playing.store(host_playing, Ordering::Relaxed);
        if let Some(t) = host_tempo {
            shared.host_tempo_bits.store((t as f32).to_bits(), Ordering::Relaxed);
        }
        while context.next_event().is_some() {}

        let n = buffer.samples() as u32;
        let output = self.params.output.value();
        let buf = shared.playback.load();
        let sync = shared.sync_to_host.load(Ordering::Relaxed);
        let want_play = if sync { host_playing } else { shared.playing.load(Ordering::Relaxed) };
        let looping = shared.loop_enabled.load(Ordering::Relaxed);
        let tempo = if sync { host_tempo.map(|t| t as f32).unwrap_or(buf.tempo) } else { buf.tempo };
        let tps = state::ticks_per_sample(tempo, self.sample_rate);

        // Panic / stop: release sounding notes.
        if shared.panic.swap(false, Ordering::AcqRel) || (!want_play && self.was_playing) {
            for &(ch, p) in &self.sounding {
                context.send_event(NoteEvent::NoteOff { timing: 0, voice_id: None, channel: ch, note: p, velocity: 0.0 });
            }
            self.sounding.clear();
        }

        // Seek request.
        let seek = shared.seek_tick.swap(u32::MAX, Ordering::AcqRel);
        if seek != u32::MAX {
            self.pos_ticks = seek as f64;
            self.next_event = buf.events.partition_point(|e| e.tick < seek);
        }

        if want_play {
            // Determine block start position in ticks.
            let mut start = if sync {
                match host_pos_beats {
                    Some(b) => {
                        let song = (b.max(0.0) * flvstx_core::PPQ as f64) as u32;
                        // Host position is song-absolute; map into the loop region.
                        let len = buf.loop_end.saturating_sub(buf.loop_start).max(1);
                        (buf.loop_start + (song.saturating_sub(buf.loop_start)) % len) as f64
                    }
                    None => self.pos_ticks,
                }
            } else {
                self.pos_ticks
            };
            if !self.was_playing || (sync && (start - self.pos_ticks).abs() > tps * n as f64 * 2.0) {
                // (Re)started or jumped: reposition the event cursor.
                if !sync && !self.was_playing && (start < buf.loop_start as f64 || start >= buf.loop_end as f64) {
                    start = buf.loop_start as f64;
                }
                self.next_event = buf.events.partition_point(|e| (e.tick as f64) < start);
            }
            let end = start + tps * n as f64;
            // Emit events in [start, end).
            let mut cursor = start;
            let mut emitted_guard = 0;
            loop {
                let ev = buf.events.get(self.next_event);
                match ev {
                    Some(e) if (e.tick as f64) < end && e.tick >= buf.loop_start && e.tick < buf.loop_end => {
                        let timing = (((e.tick as f64 - start) / tps).floor().max(0.0) as u32).min(n.saturating_sub(1));
                        if e.on {
                            if output.allows(e.channel) {
                                context.send_event(NoteEvent::NoteOn { timing, voice_id: None, channel: e.channel, note: e.pitch, velocity: e.vel });
                                if self.sounding.len() < 64 {
                                    self.sounding.push((e.channel, e.pitch));
                                }
                            }
                        } else {
                            context.send_event(NoteEvent::NoteOff { timing, voice_id: None, channel: e.channel, note: e.pitch, velocity: 0.0 });
                            if let Some(i) = self.sounding.iter().position(|&(c, p)| c == e.channel && p == e.pitch) {
                                self.sounding.swap_remove(i);
                            }
                        }
                        self.next_event += 1;
                        cursor = e.tick as f64;
                    }
                    Some(e) if e.tick >= buf.loop_end || e.tick < buf.loop_start => {
                        self.next_event += 1;
                    }
                    _ => break,
                }
                emitted_guard += 1;
                if emitted_guard > 4096 {
                    break;
                }
            }
            let _ = cursor;
            // Advance / loop.
            let mut new_pos = end;
            if new_pos >= buf.loop_end as f64 {
                if looping || sync {
                    // Release everything at the loop point and wrap.
                    let timing = (((buf.loop_end as f64 - start) / tps).floor().max(0.0) as u32).min(n.saturating_sub(1));
                    for &(ch, p) in &self.sounding {
                        context.send_event(NoteEvent::NoteOff { timing, voice_id: None, channel: ch, note: p, velocity: 0.0 });
                    }
                    self.sounding.clear();
                    let len = buf.loop_end.saturating_sub(buf.loop_start).max(1) as f64;
                    new_pos = buf.loop_start as f64 + (new_pos - buf.loop_start as f64) % len;
                    self.next_event = buf.events.partition_point(|e| (e.tick as f64) < new_pos);
                } else {
                    for &(ch, p) in &self.sounding {
                        context.send_event(NoteEvent::NoteOff { timing: n.saturating_sub(1), voice_id: None, channel: ch, note: p, velocity: 0.0 });
                    }
                    self.sounding.clear();
                    shared.playing.store(false, Ordering::Relaxed);
                    new_pos = buf.loop_start as f64;
                    self.next_event = buf.events.partition_point(|e| e.tick < buf.loop_start);
                }
            }
            self.pos_ticks = new_pos;
            shared.playhead_tick.store(new_pos as u32, Ordering::Relaxed);
        }
        self.was_playing = want_play;

        // Piano-roll note preview (short blip).
        let preview = shared.preview.swap(0, Ordering::AcqRel);
        if preview != 0 {
            let ch = ((preview >> 16) & 0xF) as u8;
            let pitch = ((preview >> 8) & 0x7F) as u8;
            let vel = (preview & 0x7F) as f32 / 127.0;
            if let Some((_, c, p)) = self.preview_off_at.take() {
                context.send_event(NoteEvent::NoteOff { timing: 0, voice_id: None, channel: c, note: p, velocity: 0.0 });
            }
            if output.allows(ch) {
                context.send_event(NoteEvent::NoteOn { timing: 0, voice_id: None, channel: ch, note: pitch, velocity: vel });
                self.preview_off_at = Some(((self.sample_rate * 0.25) as u32, ch, pitch));
            }
        }
        if let Some((remaining, ch, p)) = self.preview_off_at {
            if remaining <= n {
                context.send_event(NoteEvent::NoteOff { timing: remaining.saturating_sub(1).min(n - 1), voice_id: None, channel: ch, note: p, velocity: 0.0 });
                self.preview_off_at = None;
            } else {
                self.preview_off_at = Some((remaining - n, ch, p));
            }
        }

        for ch in buffer.as_slice() {
            ch.fill(0.0);
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for Flvstx {
    const CLAP_ID: &'static str = "com.flvstx.flvstx";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Claude-powered MIDI composer");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument, ClapFeature::NoteEffect, ClapFeature::Utility];
}

impl Vst3Plugin for Flvstx {
    const VST3_CLASS_ID: [u8; 16] = *b"FLVSTXcomposer01";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument, Vst3SubCategory::Tools];
}

nih_export_clap!(Flvstx);
nih_export_vst3!(Flvstx);
