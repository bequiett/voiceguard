mod dpdf_worker;
mod event_guard;

use std::{collections::VecDeque, num::NonZeroU32, path::PathBuf, sync::Arc};

use dpdf_worker::{DpdfWorker, HOP};
use event_guard::{EventDecision, EventGuard};
use nih_plug::prelude::*;

const LOOKAHEAD: usize = 4;
const REPORTED_LATENCY: u32 = 2880;

#[cfg(windows)]
#[used]
static DLL_ANCHOR: u16 = 0;

struct VoiceGuard {
    params: Arc<VoiceGuardParams>,
    channels: Vec<Channel>,
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
struct VoiceGuardParams {
    #[id = "bypass"] bypass: BoolParam,
    #[id = "strength"] strength: FloatParam,
    #[id = "voice_protect"] voice_protect: FloatParam,
    #[id = "artifact"] artifact: FloatParam,
    #[id = "floor"] floor: FloatParam,
}

impl Default for VoiceGuardParams {
    fn default() -> Self {
        Self {
            bypass: BoolParam::new("Bypass", false),
            strength: FloatParam::new("Strength", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %").with_value_to_string(formatters::v2s_f32_percentage(0)).with_string_to_value(formatters::s2v_f32_percentage()),
            voice_protect: FloatParam::new("Voice Protect", 0.80, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %").with_value_to_string(formatters::v2s_f32_percentage(0)).with_string_to_value(formatters::s2v_f32_percentage()),
            artifact: FloatParam::new("Artifact", 0.82, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit(" %").with_value_to_string(formatters::v2s_f32_percentage(0)).with_string_to_value(formatters::s2v_f32_percentage()),
            floor: FloatParam::new("Floor", -48.0, FloatRange::Linear { min: -72.0, max: -12.0 })
                .with_unit(" dB").with_value_to_string(formatters::v2s_f32_rounded(1)),
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
        if event > 0.48 { self.event_hold = if transient > breath.max(wind) { 2 } else { 10 }; }
        let active = self.event_hold > 0;
        if self.event_hold > 0 { self.event_hold -= 1; }

        let mut target = 1.0_f32;
        if active {
            let depth = if transient > 0.48 { 0.82 } else { 0.72 };
            target = (1.0 - event * depth * (1.0 - 0.35 * protect)).clamp(floor, 1.0);
        }
        let attack = if transient > 0.48 { 0.05 } else { 0.25 };
        let release = if breath.max(wind) > 0.40 { 0.996 } else { 0.985 };
        for x in frame {
            let coeff = if target < self.event_gain { attack } else { release };
            self.event_gain = target + coeff * (self.event_gain - target);
            self.output.push_back((x * self.event_gain).clamp(-0.99, 0.99));
        }
    }
}

impl Default for VoiceGuard { fn default() -> Self { Self { params: Arc::new(VoiceGuardParams::default()), channels: Vec::new() } } }

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
        let count = layout.main_input_channels.map_or(1, |n| n.get()) as usize;
        let base = plugin_dir(); preload_runtime(&base);
        let model = base.join("dpdfnet8").join("dpdfnet8_48khz_hr.onnx");
        self.channels = (0..count).map(|_| Channel::new(model.clone())).collect();
        context.set_latency_samples(REPORTED_LATENCY); true
    }
    fn reset(&mut self) {
        let base = plugin_dir(); preload_runtime(&base);
        let model = base.join("dpdfnet8").join("dpdfnet8_48khz_hr.onnx");
        for channel in &mut self.channels { *channel = Channel::new(model.clone()); }
    }
    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, _context: &mut impl ProcessContext<Self>) -> ProcessStatus {
        if self.params.bypass.value() { return ProcessStatus::Normal; }
        let strength = self.params.strength.value(); let artifact = self.params.artifact.value();
        let floor = util::db_to_gain(self.params.floor.value()); let protect = self.params.voice_protect.value();
        for mut frame in buffer.iter_samples() { for (i, sample) in frame.iter_mut().enumerate() { if let Some(ch) = self.channels.get_mut(i) { *sample = ch.push(*sample, strength, artifact, floor, protect); } } }
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
