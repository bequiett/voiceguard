mod artifact;
mod biquad;
mod engine;
mod gtcrn;
mod silero;

use std::{num::NonZeroU32, sync::Arc};

use engine::{ChannelEngine, Settings};
use nih_plug::prelude::*;
use silero::SpeechVad;

struct VoiceGuard {
    params: Arc<VoiceGuardParams>,
    engines: Vec<ChannelEngine>,
    vad: SpeechVad,
    sample_rate: f32,
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

    #[id = "air"]
    air: FloatParam,
}

impl Default for VoiceGuardParams {
    fn default() -> Self {
        Self {
            bypass: BoolParam::new("Bypass", false),
            strength: FloatParam::new("Strength", 0.72, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            voice_protect: FloatParam::new("Voice Protect", 0.82, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            artifact: FloatParam::new("Artifact", 0.70, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            floor: FloatParam::new("Floor", -32.0, FloatRange::Linear { min: -60.0, max: -12.0 })
                .with_unit(" dB")
                .with_value_to_string(formatters::v2s_f32_rounded(1)),
            air: FloatParam::new("Air", 0.75, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
        }
    }
}

impl Default for VoiceGuard {
    fn default() -> Self {
        let sample_rate = 48_000.0;
        Self {
            params: Arc::new(VoiceGuardParams::default()),
            engines: vec![ChannelEngine::new(sample_rate), ChannelEngine::new(sample_rate)],
            vad: SpeechVad::new(sample_rate),
            sample_rate,
        }
    }
}

impl Plugin for VoiceGuard {
    const NAME: &'static str = "VoiceGuard";
    const VENDOR: &'static str = "bequiett";
    const URL: &'static str = "https://github.com/bequiett/voiceguard";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        let channels = audio_io_layout.main_input_channels.map_or(1, |n| n.get()) as usize;
        self.engines = (0..channels).map(|_| ChannelEngine::new(self.sample_rate)).collect();
        self.vad = SpeechVad::new(self.sample_rate);
        if let Some(engine) = self.engines.first() {
            context.set_latency_samples(engine.reported_latency_samples());
        }
        true
    }

    fn reset(&mut self) {
        for engine in &mut self.engines {
            engine.reset();
        }
        self.vad.reset(self.sample_rate);
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        if self.params.bypass.value() {
            return ProcessStatus::Normal;
        }

        let settings = Settings {
            strength: self.params.strength.value().clamp(0.0, 1.0),
            voice_protect: self.params.voice_protect.value().clamp(0.0, 1.0),
            artifact: self.params.artifact.value().clamp(0.0, 1.0),
            floor_gain: util::db_to_gain(self.params.floor.value()),
            air: self.params.air.value().clamp(0.0, 1.0),
        };

        for mut frame in buffer.iter_samples() {
            let channels = frame.len().max(1);
            let mut mono = 0.0_f32;
            for sample in frame.iter_mut() {
                mono += *sample;
            }
            mono /= channels as f32;
            let speech = self.vad.push(mono);
            for (channel, sample) in frame.iter_mut().enumerate() {
                if let Some(engine) = self.engines.get_mut(channel) {
                    *sample = engine.process_sample(*sample, speech, settings);
                }
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for VoiceGuard {
    const CLAP_ID: &'static str = "com.bequiett.voiceguard";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Low-latency speech-first microphone cleanup");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Mono,
        ClapFeature::Stereo,
        ClapFeature::Utility,
    ];
}

impl Vst3Plugin for VoiceGuard {
    const VST3_CLASS_ID: [u8; 16] = *b"VoiceGuardBqt001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Tools,
    ];
}

nih_export_clap!(VoiceGuard);
nih_export_vst3!(VoiceGuard);
