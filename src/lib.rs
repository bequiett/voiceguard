mod df_worker;
mod event_guard;

use std::{collections::VecDeque, num::NonZeroU32, path::PathBuf, sync::Arc};

use df_worker::DfWorker;
use event_guard::{EventDecision, EventGuard};
use nih_plug::prelude::*;

const FRAME: usize = 480;
const EXTRA_LOOKAHEAD: usize = 2;
const REPORTED_LATENCY: u32 = 2880;

struct VoiceGuard {
    params: Arc<VoiceGuardParams>,
    channels: Vec<Channel>,
    sample_rate: f32,
}

struct PendingFrame {
    raw: Vec<f32>,
    enhanced: Option<Vec<f32>>,
    event: EventDecision,
}

struct Channel {
    worker: DfWorker,
    event: EventGuard,
    input: Vec<f32>,
    input_pos: usize,
    output: VecDeque<f32>,
    delayed: VecDeque<PendingFrame>,
    event_gain: f32,
}

#[derive(Params)]
struct VoiceGuardParams {
    #[id = "bypass"]
    bypass: BoolParam,
    #[id = "strength"]
    strength: FloatParam,
    #[id = "voice_protect"]
    voice_protect: FloatParam,
    #[id = "artifact"]
    artifact: FloatParam,
    #[id = "floor"]
    floor: FloatParam,
}

impl Default for VoiceGuardParams {
    fn default() -> Self {
        Self {
            bypass: BoolParam::new("Bypass", false),
            strength: FloatParam::new("Strength", 0.94, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            voice_protect: FloatParam::new("Voice Protect", 0.72, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            artifact: FloatParam::new("Artifact", 0.90, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            floor: FloatParam::new("Floor", -45.0, FloatRange::Linear { min: -72.0, max: -12.0 })
                .with_unit(" dB")
                .with_value_to_string(formatters::v2s_f32_rounded(1)),
        }
    }
}

impl Channel {
    fn new(model_dir: PathBuf) -> Self {
        Self {
            worker: DfWorker::new(model_dir),
            event: EventGuard::new(),
            input: vec![0.0; FRAME],
            input_pos: 0,
            output: VecDeque::with_capacity(FRAME * 8),
            delayed: VecDeque::with_capacity(8),
            event_gain: 1.0,
        }
    }

    fn push(&mut self, sample: f32, strength: f32, artifact: f32, floor: f32, protect: f32) -> f32 {
        self.input[self.input_pos] = sample;
        self.input_pos += 1;
        if self.input_pos == FRAME {
            let raw = self.input.clone();
            let decision = self.event.analyze(&raw);
            self.worker.submit(raw.clone());
            self.delayed.push_back(PendingFrame { raw, enhanced: None, event: decision });
            self.input_pos = 0;
        }

        while let Some(enhanced) = self.worker.take() {
            if let Some(slot) = self.delayed.iter_mut().find(|x| x.enhanced.is_none()) {
                slot.enhanced = Some(enhanced);
            }
        }

        while self.delayed.len() > EXTRA_LOOKAHEAD {
            let ready = self.delayed.front().map(|x| x.enhanced.is_some()).unwrap_or(false);
            if !ready { break; }
            if let Some(mut pending) = self.delayed.pop_front() {
                let enhanced = pending.enhanced.take().unwrap_or_else(|| pending.raw.clone());
                let mixed: Vec<f32> = enhanced.iter().zip(pending.raw.iter())
                    .map(|(wet, dry)| wet * strength + dry * (1.0 - strength))
                    .collect();
                self.render(mixed, pending.event, artifact, floor, protect);
            }
        }

        self.output.pop_front().unwrap_or(0.0)
    }

    fn render(&mut self, frame: Vec<f32>, d: EventDecision, artifact: f32, floor: f32, protect: f32) {
        if frame.len() != FRAME {
            self.output.extend(std::iter::repeat_n(0.0, FRAME));
            return;
        }

        let transient = d.transient * artifact;
        let breath = d.breath * artifact;
        let wind = d.wind * artifact;
        let mut target: f32 = 1.0;

        if transient > 0.42 {
            target = target.min(1.0 - transient * (0.92 - 0.25 * protect));
        }
        if breath > 0.28 {
            target = target.min(1.0 - breath * (0.88 - 0.30 * protect));
        }
        if wind > 0.30 {
            target = target.min(1.0 - wind * (0.94 - 0.24 * protect));
        }
        target = target.clamp(floor, 1.0);

        let attack = if transient > 0.42 { 0.12 } else { 0.55 };
        let release = if breath.max(wind) > 0.30 { 0.992 } else { 0.975 };
        for x in frame {
            let coeff = if target < self.event_gain { attack } else { release };
            self.event_gain = target + coeff * (self.event_gain - target);
            self.output.push_back((x * self.event_gain).clamp(-0.99, 0.99));
        }
    }
}

impl Default for VoiceGuard {
    fn default() -> Self {
        Self { params: Arc::new(VoiceGuardParams::default()), channels: Vec::new(), sample_rate: 48_000.0 }
    }
}

impl Plugin for VoiceGuard {
    const NAME: &'static str = "VoiceGuard";
    const VENDOR: &'static str = "bequiett";
    const URL: &'static str = "https://github.com/bequiett/voiceguard";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout { main_input_channels: NonZeroU32::new(1), main_output_channels: NonZeroU32::new(1), ..AudioIOLayout::const_default() },
        AudioIOLayout { main_input_channels: NonZeroU32::new(2), main_output_channels: NonZeroU32::new(2), ..AudioIOLayout::const_default() },
    ];
    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> { self.params.clone() }

    fn initialize(&mut self, layout: &AudioIOLayout, config: &BufferConfig, context: &mut impl InitContext<Self>) -> bool {
        if (config.sample_rate - 48_000.0).abs() > 1.0 { return false; }
        self.sample_rate = config.sample_rate;
        let count = layout.main_input_channels.map_or(1, |n| n.get()) as usize;
        let model_dir = model_dir();
        self.channels = (0..count).map(|_| Channel::new(model_dir.clone())).collect();
        context.set_latency_samples(REPORTED_LATENCY);
        true
    }

    fn reset(&mut self) {
        let model_dir = model_dir();
        for channel in &mut self.channels { *channel = Channel::new(model_dir.clone()); }
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, _context: &mut impl ProcessContext<Self>) -> ProcessStatus {
        if self.params.bypass.value() { return ProcessStatus::Normal; }
        let strength = self.params.strength.value();
        let artifact = self.params.artifact.value();
        let floor = util::db_to_gain(self.params.floor.value());
        let protect = self.params.voice_protect.value();

        for mut frame in buffer.iter_samples() {
            for (i, sample) in frame.iter_mut().enumerate() {
                if let Some(channel) = self.channels.get_mut(i) {
                    *sample = channel.push(*sample, strength, artifact, floor, protect);
                }
            }
        }
        ProcessStatus::Normal
    }
}

fn model_dir() -> PathBuf {
    std::env::current_exe().ok()
        .and_then(|p| p.parent().map(|x| x.to_path_buf()))
        .unwrap_or_default()
        .join("dfn3_h0")
}

impl ClapPlugin for VoiceGuard {
    const CLAP_ID: &'static str = "com.bequiett.voiceguard";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Real-time microphone cleanup");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Mono, ClapFeature::Stereo, ClapFeature::Utility];
}

impl Vst3Plugin for VoiceGuard {
    const VST3_CLASS_ID: [u8; 16] = *b"VoiceGuardBqt002";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Tools];
}

nih_export_clap!(VoiceGuard);
nih_export_vst3!(VoiceGuard);
