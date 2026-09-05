//! FLVSTX — Claude-powered MIDI composer plugin (CLAP + VST3) for FL Studio.
//!
//! Audio thread: reads the lock-free playback buffer and emits note events synced to the host
//! transport (or an internal clock). GUI thread: egui editor (chat, arrangement, piano roll).
//! IPC thread: WebSocket bridge to the agent sidecar, serving tool calls against the session store.

#[cfg(windows)]
pub mod dragout;
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
    seen_seek_epoch: u64,
    seen_panic_epoch: u64,
    seen_preview_epoch: u64,
    /// Built-in soundfont synth (only the audio-owner instance renders it).
    synth: Option<rustysynth::Synthesizer>,
    synth_programs: [u8; 16],
    synth_events: Vec<(u32, bool, u8, u8, u8)>,
    synth_l: Vec<f32>,
    synth_r: Vec<f32>,
    instance_id: u64,
}

static INSTANCE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Label for the MIDI output parameter: "All" or "ch N: Layer" looked up in the shared song.
fn output_label(v: i32) -> String {
    if v <= 0 {
        return "All layers".to_string();
    }
    let ch = (v - 1) as u8;
    let names: Vec<String> = state::global_shared().lock_store().session.tracks.iter().filter(|t| t.channel == ch).map(|t| t.name.clone()).collect();
    if names.is_empty() { format!("ch {v}: (no layer)") } else { format!("ch {v}: {}", names.join(" + ")) }
}

fn output_allows(v: i32, channel: u8) -> bool {
    v <= 0 || (v - 1) as u8 == channel
}

#[derive(Params)]
pub struct FlvstxParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,
    /// MIDI output filter: 0 = all layers, 1..16 = only the layer(s) on that MIDI channel.
    #[id = "output"]
    pub output: IntParam,
    /// The whole session + chat as JSON (kept in sync by the GUI thread).
    #[persist = "flvstx-state"]
    state_json: Arc<RwLock<String>>,
    /// Play the song through the built-in soundfont synth (no routing needed).
    #[id = "sound"]
    pub sound: BoolParam,
    /// Built-in synth gain.
    #[id = "sound_gain"]
    pub sound_gain: FloatParam,
}

impl Default for Flvstx {
    fn default() -> Self {
        Self {
            params: Arc::new(FlvstxParams {
                editor_state: EguiState::from_size(1500, 900),
                output: IntParam::new("MIDI output", 0, IntRange::Linear { min: 0, max: 16 })
                    .with_value_to_string(std::sync::Arc::new(output_label))
                    .with_string_to_value(std::sync::Arc::new(|s: &str| {
                        let t = s.trim().to_ascii_lowercase();
                        if t.starts_with("all") { return Some(0); }
                        t.trim_start_matches("ch").trim().split(':').next().and_then(|n| n.trim().parse::<i32>().ok())
                    })),
                state_json: Arc::new(RwLock::new(String::new())),
                sound: BoolParam::new("Built-in sound", true),
                sound_gain: FloatParam::new("Sound gain", util::db_to_gain(0.0), FloatRange::Skewed { min: util::db_to_gain(-30.0), max: util::db_to_gain(6.0), factor: FloatRange::gain_skew_factor(-30.0, 6.0) })
                    .with_unit(" dB")
                    .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
                    .with_string_to_value(formatters::s2v_f32_gain_to_db()),
            }),
            shared: state::global_shared(),
            sample_rate: 44100.0,
            pos_ticks: 0.0,
            next_event: 0,
            was_playing: false,
            sounding: Vec::with_capacity(64),
            last_loaded_state: 0,
            preview_off_at: None,
            seen_seek_epoch: 0,
            seen_panic_epoch: 0,
            seen_preview_epoch: 0,
            synth: None,
            synth_programs: [255; 16],
            synth_events: Vec::with_capacity(1024),
            synth_l: Vec::new(),
            synth_r: Vec::new(),
            instance_id: INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed),
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
        let max = buffer_config.max_buffer_size as usize;
        self.synth_l = vec![0.0; max.max(64)];
        self.synth_r = vec![0.0; max.max(64)];
        self.shared.load_soundfont_async();
        let _ = self.shared.audio_owner.compare_exchange(0, self.instance_id, Ordering::AcqRel, Ordering::Acquire);
        self.synth = None;
        self.synth_programs = [255; 16];
        self.try_create_synth();
        // Restore persisted state (project load). The GUI also does this, but the editor may not be open.
        let json = self.params.state_json.read().map(|s| s.clone()).unwrap_or_default();
        let h = hash_str(&json);
        // Several instances persist the same shared song; only the first to see a new blob loads it.
        if !json.is_empty() && h != self.last_loaded_state && h != self.shared.loaded_hash.load(Ordering::Acquire) {
            if let Ok(p) = serde_json::from_str::<Persisted>(&json) {
                self.shared.load_persisted(p);
                self.shared.loaded_hash.store(h, Ordering::Release);
            }
        }
        self.last_loaded_state = h;
        true
    }

    fn reset(&mut self) {
        self.next_event = 0;
        self.sounding.clear();
        if let Some(s) = self.synth.as_mut() {
            s.note_off_all(true);
        }
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
        let panic_epoch = shared.panic.load(Ordering::Acquire);
        let panic_now = panic_epoch != self.seen_panic_epoch;
        self.seen_panic_epoch = panic_epoch;
        self.synth_events.clear();
        if panic_now || (!want_play && self.was_playing) {
            for &(ch, p) in &self.sounding {
                context.send_event(NoteEvent::NoteOff { timing: 0, voice_id: None, channel: ch, note: p, velocity: 0.0 });
            }
            self.sounding.clear();
            if let Some(s) = self.synth.as_mut() {
                s.note_off_all(false);
            }
        }

        // Seek request.
        let seek_packed = shared.seek.load(Ordering::Acquire);
        if (seek_packed >> 32) != self.seen_seek_epoch {
            self.seen_seek_epoch = seek_packed >> 32;
            let seek = (seek_packed & 0xFFFF_FFFF) as u32;
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
                            if output_allows(output, e.channel) {
                                context.send_event(NoteEvent::NoteOn { timing, voice_id: None, channel: e.channel, note: e.pitch, velocity: e.vel });
                                if self.sounding.len() < 64 {
                                    self.sounding.push((e.channel, e.pitch));
                                }
                            }
                            if self.synth_events.len() < self.synth_events.capacity() {
                                self.synth_events.push((timing, true, e.synth_channel, e.pitch, (e.vel * 127.0) as u8));
                            }
                        } else {
                            context.send_event(NoteEvent::NoteOff { timing, voice_id: None, channel: e.channel, note: e.pitch, velocity: 0.0 });
                            if let Some(i) = self.sounding.iter().position(|&(c, p)| c == e.channel && p == e.pitch) {
                                self.sounding.swap_remove(i);
                            }
                            if self.synth_events.len() < self.synth_events.capacity() {
                                self.synth_events.push((timing, false, e.synth_channel, e.pitch, 0));
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
        let preview_packed = shared.preview.load(Ordering::Acquire);
        let preview = if (preview_packed >> 32) != self.seen_preview_epoch { (preview_packed & 0xFFFF_FFFF) as u32 } else { 0 };
        self.seen_preview_epoch = preview_packed >> 32;
        if preview != 0 {
            let ch = ((preview >> 16) & 0xF) as u8;
            let pitch = ((preview >> 8) & 0x7F) as u8;
            let vel = (preview & 0x7F) as f32 / 127.0;
            if let Some((_, c, p)) = self.preview_off_at.take() {
                context.send_event(NoteEvent::NoteOff { timing: 0, voice_id: None, channel: c, note: p, velocity: 0.0 });
            }
            if output_allows(output, ch) {
                context.send_event(NoteEvent::NoteOn { timing: 0, voice_id: None, channel: ch, note: pitch, velocity: vel });
            }
            self.preview_off_at = Some(((self.sample_rate * 0.25) as u32, ch, pitch));
            let sch = if ch == 9 { 9 } else { ch };
            self.synth_events.push((0, true, sch, pitch, (vel * 127.0) as u8));
        }
        if let Some((remaining, ch, p)) = self.preview_off_at {
            if remaining <= n {
                context.send_event(NoteEvent::NoteOff { timing: remaining.saturating_sub(1).min(n - 1), voice_id: None, channel: ch, note: p, velocity: 0.0 });
                self.preview_off_at = None;
                self.synth_events.push((remaining.saturating_sub(1).min(n - 1), false, if ch == 9 { 9 } else { ch }, p, 0));
            } else {
                self.preview_off_at = Some((remaining - n, ch, p));
            }
        }

        for ch in buffer.as_slice() {
            ch.fill(0.0);
        }
        self.render_synth(buffer, &buf.programs, n as usize);
        ProcessStatus::Normal
    }
}

impl Flvstx {
    /// Creates the synthesizer once the shared soundfont is loaded (cheap once the font is in memory).
    fn try_create_synth(&mut self) {
        if self.synth.is_some() {
            return;
        }
        let sf = match self.shared.soundfont.try_lock() {
            Ok(g) => g.clone(),
            Err(_) => None,
        };
        if let Some(sf) = sf {
            let mut settings = rustysynth::SynthesizerSettings::new(self.sample_rate as i32);
            settings.enable_reverb_and_chorus = true;
            settings.maximum_polyphony = 96;
            if let Ok(s) = rustysynth::Synthesizer::new(&sf, &settings) {
                self.synth = Some(s);
                self.synth_programs = [255; 16];
            }
        }
    }

    fn render_synth(&mut self, buffer: &mut Buffer, programs: &[u8; 16], n: usize) {
        let enabled = self.params.sound.value();
        let owner = self.shared.audio_owner.load(Ordering::Acquire);
        if owner == 0 {
            let _ = self.shared.audio_owner.compare_exchange(0, self.instance_id, Ordering::AcqRel, Ordering::Acquire);
        }
        let is_owner = self.shared.audio_owner.load(Ordering::Acquire) == self.instance_id;
        if !enabled || !is_owner {
            return;
        }
        if self.synth.is_none() {
            self.try_create_synth();
        }
        let Some(synth) = self.synth.as_mut() else { return };
        if n > self.synth_l.len() {
            return;
        }
        // Program changes.
        for ch in 0..16 {
            let want = programs[ch];
            if want != self.synth_programs[ch] {
                self.synth_programs[ch] = want;
                if ch != 9 && want < 128 {
                    synth.process_midi_message(ch as i32, 0xC0, want as i32, 0);
                }
            }
        }
        // Render in segments between events for sample-accurate timing.
        self.synth_events.sort_by_key(|e| e.0);
        let mut pos = 0usize;
        let gain = self.params.sound_gain.value();
        let (l, r) = self.synth_l.split_at_mut(0);
        let _ = (l, r);
        let mut idx = 0;
        while pos < n {
            let next = self.synth_events.get(idx).map(|e| (e.0 as usize).min(n)).unwrap_or(n);
            if next > pos {
                let (ls, rs) = (&mut self.synth_l[pos..next], &mut self.synth_r[pos..next]);
                synth.render(ls, rs);
                pos = next;
            }
            while let Some(e) = self.synth_events.get(idx) {
                if (e.0 as usize).min(n) > pos {
                    break;
                }
                if e.1 {
                    synth.note_on(e.2 as i32, e.3 as i32, e.4.max(1) as i32);
                } else {
                    synth.note_off(e.2 as i32, e.3 as i32);
                }
                idx += 1;
            }
        }
        let out = buffer.as_slice();
        if out.len() >= 2 {
            for i in 0..n {
                out[0][i] += self.synth_l[i] * gain;
                out[1][i] += self.synth_r[i] * gain;
            }
        } else if let Some(ch) = out.first_mut() {
            for i in 0..n {
                ch[i] += (self.synth_l[i] + self.synth_r[i]) * 0.5 * gain;
            }
        }
    }
}

impl Drop for Flvstx {
    fn drop(&mut self) {
        let _ = self.shared.audio_owner.compare_exchange(self.instance_id, 0, Ordering::AcqRel, Ordering::Acquire);
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
