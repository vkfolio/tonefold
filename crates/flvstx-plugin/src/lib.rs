//! FLVSTX plugin — Phase 0 spike: MIDI-out instrument with an egui button that plays a chord.
use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, EguiState};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

pub struct Flvstx {
    params: Arc<FlvstxParams>,
    /// Set by GUI, consumed by audio thread.
    trigger: Arc<AtomicBool>,
    /// Sample countdown for the currently sounding chord (0 = silent).
    remaining: u32,
    sample_rate: f32,
    /// Last observed transport values for GUI display (f32 bits).
    tempo_bits: Arc<AtomicU32>,
    pos_beats_bits: Arc<AtomicU32>,
    playing: Arc<AtomicBool>,
}

#[derive(Params)]
pub struct FlvstxParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,
}

const CHORD: [u8; 3] = [60, 64, 67];

impl Default for Flvstx {
    fn default() -> Self {
        Self {
            params: Arc::new(FlvstxParams { editor_state: EguiState::from_size(420, 240) }),
            trigger: Arc::new(AtomicBool::new(false)),
            remaining: 0,
            sample_rate: 44100.0,
            tempo_bits: Arc::new(AtomicU32::new(0)),
            pos_beats_bits: Arc::new(AtomicU32::new(0)),
            playing: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Plugin for Flvstx {
    const NAME: &'static str = "FLVSTX";
    const VENDOR: &'static str = "FLVSTX";
    const URL: &'static str = "https://github.com/";
    const EMAIL: &'static str = "vigneshaiml@gmail.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // Silent stereo output so hosts that disable audio-less plugins keep us alive.
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
        let trigger = self.trigger.clone();
        let tempo_bits = self.tempo_bits.clone();
        let pos_bits = self.pos_beats_bits.clone();
        let playing = self.playing.clone();
        create_egui_editor(
            self.params.editor_state.clone(),
            (),
            |_, _| {},
            move |ctx, _setter, _state| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("FLVSTX spike");
                    if ui.button("Play C major chord (1 beat)").clicked() {
                        trigger.store(true, Ordering::Release);
                    }
                    let tempo = f32::from_bits(tempo_bits.load(Ordering::Relaxed));
                    let pos = f32::from_bits(pos_bits.load(Ordering::Relaxed));
                    ui.label(format!(
                        "host: playing={} tempo={:.1} pos_beats={:.2}",
                        playing.load(Ordering::Relaxed),
                        tempo,
                        pos
                    ));
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                });
            },
        )
    }

    fn initialize(
        &mut self,
        _layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let (playing, tempo, pos_beats) = {
            let t = context.transport();
            (t.playing, t.tempo, t.pos_beats())
        };
        self.playing.store(playing, Ordering::Relaxed);
        if let Some(tempo) = tempo {
            self.tempo_bits.store((tempo as f32).to_bits(), Ordering::Relaxed);
        }
        if let Some(pos) = pos_beats {
            self.pos_beats_bits.store((pos as f32).to_bits(), Ordering::Relaxed);
        }

        // Drain incoming events (unused in the spike).
        while context.next_event().is_some() {}

        let n = buffer.samples() as u32;
        if self.trigger.swap(false, Ordering::AcqRel) && self.remaining == 0 {
            let beat_len = (60.0 / tempo.unwrap_or(120.0) as f32 * self.sample_rate) as u32;
            self.remaining = beat_len.max(1);
            for &note in &CHORD {
                context.send_event(NoteEvent::NoteOn { timing: 0, voice_id: None, channel: 0, note, velocity: 0.8 });
            }
        }
        if self.remaining > 0 {
            if self.remaining <= n {
                let timing = self.remaining - 1;
                for &note in &CHORD {
                    context.send_event(NoteEvent::NoteOff { timing, voice_id: None, channel: 0, note, velocity: 0.0 });
                }
                self.remaining = 0;
            } else {
                self.remaining -= n;
            }
        }

        // Silence.
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
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::Instrument, ClapFeature::NoteEffect, ClapFeature::Utility];
}

impl Vst3Plugin for Flvstx {
    const VST3_CLASS_ID: [u8; 16] = *b"FLVSTXcomposer01";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Tools];
}

nih_export_clap!(Flvstx);
nih_export_vst3!(Flvstx);
