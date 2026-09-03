mod dpdf_worker;
mod editor;
mod event_guard;

use std::{collections::VecDeque, num::NonZeroU32, path::PathBuf, sync::Arc};

use dpdf_worker::{DpdfWorker, HOP};
use event_guard::{EventDecision, EventGuard};
use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

const LOOKAHEAD: usize = 4;
const REPORTED_LATENCY: u32 = 2880;

#[cfg(windows)]
#[used]
static DLL_ANCHOR: u16 = 0;

struct VoiceGuard {
    params: Arc<VoiceGuardParams>,
    channels: Vec<Channel>,
    mono_source: bool,
}

struct PendingFrame {
    raw: Vec<f32>,
    enhanced: Option<Vec<f32>>,
    event: EventDecision,
}

struct Channel {
    worker: DpdfWorker,
    event: EventGuard,
    input: Vec<f32>,
    input_pos: usize,
    output: VecDeque<f32>,
    delayed: VecDeque<PendingFrame>,
    event_gain: f32,
    event_hold: usize,
}

#[derive(Params)]
pub(crate) struct VoiceGuardParams {
    #[persist = "editor-state"]
    pub(crate) editor_state: Arc<EguiState>,
    #[id = "bypass"] pub(crate) bypass: BoolParam,
    #[id = "strength"] pub(crate) strength: FloatParam,
    #[id = "voice_protect"] pub(crate) voice_protect: FloatParam,
    #[id = "artifact"] pub(crate) artifact: FloatParam,
    #[id = "floor"] pub(crate) floor: FloatParam,
    #[id = "output_gain"] pub(crate) output_gain: FloatParam,
}

impl Default for VoiceGuardParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(560, 300),
            bypass: BoolParam::new("Bypass", false),
            strength: FloatParam::new("Strength", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %").with_value_to_string(formatters::v2s_f32_percentage(0)).with_string_to_value(formatters::s2v_f32_percentage()),
            voice_protect: FloatParam::new("Voice Protect", 0.84, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %").with_value_to_string(formatters::v2s_f32_percentage(0)).with_string_to_value(formatters::s2v_f32_percentage()),
            artifact: FloatParam::new("Artifact", 0.72, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %").with_value_to_string(formatters::v2s_f32_percentage(0)).with_string_to_value(formatters::s2v_f32_percentage()),
            floor: FloatParam::new("Floor", -42.0, FloatRange::Linear { min: -72.0, max: -12.0 })
                .with_unit(" dB").with_value_to_string(formatters::v2s_f32_rounded(1)),
            output_gain: FloatParam::new("Output", util::db_to_gain(0.0), FloatRange::Skewed {
                    min: util::db_to_gain(-12.0), max: util::db_to_gain(18.0), factor: FloatRange::gain_skew_factor(-12.0, 18.0),
                })
                .with_smoother(SmoothingStyle::Logarithmic(30.0))
                .with_unit(" dB")
                .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
                .with_string_to_value(formatters::s2v_f32_gain_to_db()),
        }
    }
}

impl Channel {
    fn new(model: PathBuf) -> Self {
        Self {
            worker: DpdfWorker::new(model), event: EventGuard::new(), input: vec![0.0; HOP], input_pos: 0,
            output: VecDeque::with_capacity(HOP * 10), delayed: VecDeque::with_capacity(10),
            event_gain: 1.0, event_hold: 0,
        }
    }

    fn push(&mut self, sample: f32, strength: f32, artifact: f32, floor: f32, protect: f32) -> f32 {
        self.input[self.input_pos] = sample;
        self.input_pos += 1;
        if self.input_pos == HOP {
            let raw = self.input.clone();
            let event = self.event.analyze(&raw);
            let submitted = self.worker.submit(raw.clone());
            self.delayed.push_back(PendingFrame { raw: raw.clone(), enhanced: if submitted { None } else { Some(raw) }, event });
            self.input_pos = 0;
        }
        while let Some(enhanced) = self.worker.take() {
            if let Some(slot) = self.delayed.iter_mut().find(|x| x.enhanced.is_none()) { slot.enhanced = Some(enhanced); }
        }
        while self.delayed.len() > LOOKAHEAD {
            if !self.delayed.front().map(|x| x.enhanced.is_some()).unwrap_or(false) { break; }
            let mut pending = self.delayed.pop_front().unwrap();
            let enhanced = pending.enhanced.take().unwrap_or_else(|| pending.raw.clone());
            let mixed = enhanced.iter().zip(&pending.raw).map(|(wet, dry)| wet * strength + dry * (1.0 - strength)).collect();
            self.render(mixed, pending.event, artifact, floor, protect);
        }
        self.output.pop_front().unwrap_or(0.0)
    }

    fn render(&mut self, frame: Vec<f32>, d: EventDecision, artifact: f32, floor: f32, protect: f32) {
        if frame.len() != HOP { self.output.extend(std::iter::repeat_n(0.0, HOP)); return; }
        let transient = d.transient * artifact;
        let breath = d.breath * artifact;
        let wind = d.wind * artifact;
        let event = transient.max(breath).max(wind);
        if event > 0.52 { self.event_hold = if transient > breath.max(wind) { 2 } else { 8 }; }
        let active = self.event_hold > 0;
        if self.event_hold > 0 { self.event_hold -= 1; }

        let mut target = 1.0_f32;
        if active {
            let depth = if transient > 0.52 { 0.72 } else { 0.62 };
            target = (1.0 - event * depth * (1.0 - 0.42 * protect)).clamp(floor, 1.0);
        }
        let attack = if transient > 0.52 { 0.16 } else { 0.48 };
        let release = if breath.max(wind) > 0.42 { 0.998 } else { 0.994 };
        for x in frame {
            let coeff = if target < self.event_gain { attack } else { release };
            self.event_gain = target + coeff * (self.event_gain - target);
            self.output.push_back((x * self.event_gain).clamp(-0.99, 0.99));
        }
    }
}

impl Default for VoiceGuard {
    fn default() -> Self { Self { params: Arc::new(VoiceGuardParams::default()), channels: Vec::new(), mono_source: false } }
}

impl Plugin for VoiceGuard {
    const NAME: &'static str = "VoiceGuard";
    const VENDOR: &'static str = "bequiett";
    const URL: &'static str = "https://github.com/bequiett/voiceguard";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout { main_input_channels: NonZeroU32::new(1), main_output_channels: NonZeroU32::new(2), ..AudioIOLayout::const_default() },
        AudioIOLayout { main_input_channels: NonZeroU32::new(2), main_output_channels: NonZeroU32::new(2), ..AudioIOLayout::const_default() },
        AudioIOLayout { main_input_channels: NonZeroU32::new(1), main_output_channels: NonZeroU32::new(1), ..AudioIOLayout::const_default() },
    ];
    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> { self.params.clone() }
    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> { editor::create(self.params.clone()) }

    fn initialize(&mut self, layout: &AudioIOLayout, config: &BufferConfig, context: &mut impl InitContext<Self>) -> bool {
        if (config.sample_rate - 48_000.0).abs() > 1.0 { return false; }
        let in_count = layout.main_input_channels.map_or(1, |n| n.get()) as usize;
        let out_count = layout.main_output_channels.map_or(in_count as u32, |n| n.get()) as usize;
        self.mono_source = in_count == 1 && out_count == 2;
        let base = plugin_dir(); preload_runtime(&base);
        let model = base.join("dpdfnet8").join("dpdfnet8_48khz_hr.onnx");
        let processors = if self.mono_source { 1 } else { in_count };
        self.channels = (0..processors).map(|_| Channel::new(model.clone())).collect();
        context.set_latency_samples(REPORTED_LATENCY); true
    }

    fn reset(&mut self) {
        let base = plugin_dir(); preload_runtime(&base);
        let model = base.join("dpdfnet8").join("dpdfnet8_48khz_hr.onnx");
        for channel in &mut self.channels { *channel = Channel::new(model.clone()); }
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, _context: &mut impl ProcessContext<Self>) -> ProcessStatus {
        if self.params.bypass.value() { return ProcessStatus::Normal; }
        let strength = self.params.strength.value();
        let artifact = self.params.artifact.value();
        let floor = util::db_to_gain(self.params.floor.value());
        let protect = self.params.voice_protect.value();

        for mut frame in buffer.iter_samples() {
            let gain = self.params.output_gain.smoothed.next();
            if self.mono_source {
                let mut samples = frame.iter_mut();
                if let Some(left) = samples.next() {
                    let input = *left;
                    let y = self.channels.get_mut(0).map(|ch| ch.push(input, strength, artifact, floor, protect)).unwrap_or(input);
                    let y = (y * gain).clamp(-0.99, 0.99);
                    *left = y;
                    if let Some(right) = samples.next() { *right = y; }
                }
            } else {
                for (i, sample) in frame.iter_mut().enumerate() {
                    if let Some(ch) = self.channels.get_mut(i) { *sample = (ch.push(*sample, strength, artifact, floor, protect) * gain).clamp(-0.99, 0.99); }
                }
            }
        }
        ProcessStatus::Normal
    }
}

#[cfg(windows)]
fn plugin_dir() -> PathBuf {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, ptr};
    use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT};
    let mut module = ptr::null_mut(); let flags = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    let address = &raw const DLL_ANCHOR;
    if unsafe { GetModuleHandleExW(flags, address, &mut module) } != 0 {
        let mut buf = vec![0_u16; 32768]; let len = unsafe { GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) };
        if len > 0 { let path = PathBuf::from(OsString::from_wide(&buf[..len as usize])); if let Some(parent) = path.parent() { return parent.to_path_buf(); } }
    }
    std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)).unwrap_or_default()
}
#[cfg(not(windows))] fn plugin_dir() -> PathBuf { std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)).unwrap_or_default() }
#[cfg(windows)] fn preload_runtime(base: &std::path::Path) { use std::{iter, os::windows::ffi::OsStrExt}; use windows_sys::Win32::System::LibraryLoader::LoadLibraryW; let dll = base.join("onnxruntime.dll"); let wide: Vec<u16> = dll.as_os_str().encode_wide().chain(iter::once(0)).collect(); unsafe { LoadLibraryW(wide.as_ptr()); } }
#[cfg(not(windows))] fn preload_runtime(_base: &std::path::Path) {}

impl ClapPlugin for VoiceGuard {
    const CLAP_ID: &'static str = "com.bequiett.voiceguard";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Real-time microphone cleanup");
    const CLAP_MANUAL_URL: Option<&'static str> = None; const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Mono, ClapFeature::Stereo, ClapFeature::Utility];
}
impl Vst3Plugin for VoiceGuard { const VST3_CLASS_ID: [u8; 16] = *b"VoiceGuardBqt003"; const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Tools]; }
nih_export_clap!(VoiceGuard); nih_export_vst3!(VoiceGuard);
