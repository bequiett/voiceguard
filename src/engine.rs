use std::{collections::VecDeque, f32::consts::PI, sync::Arc};

use realfft::{num_complex::Complex, ComplexToReal, RealFftPlanner, RealToComplex};

use crate::{
    artifact::{ArtifactDecision, ArtifactDetector},
    biquad::Biquad,
    gtcrn::{Gtcrn, NUM_BINS},
};

const MODEL_SR: f32 = 16_000.0;
const MODEL_NFFT: f32 = 512.0;

#[derive(Clone, Copy)]
pub struct Settings {
    pub strength: f32,
    pub voice_protect: f32,
    pub artifact: f32,
    pub floor_gain: f32,
    pub air: f32,
}

pub struct ChannelEngine {
    sample_rate: f32,
    nfft: usize,
    hop: usize,
    scale: f32,
    analysis_window: Vec<f32>,
    window: Vec<f32>,
    hop_input: Vec<f32>,
    hop_pos: usize,
    fft_in: Vec<f32>,
    fft_out: Vec<Complex<f32>>,
    ifft_out: Vec<f32>,
    overlap: Vec<f32>,
    output: VecDeque<f32>,
    r2c: Arc<dyn RealToComplex<f32>>,
    c2r: Arc<dyn ComplexToReal<f32>>,
    gtcrn: Option<Gtcrn>,
    artifact: ArtifactDetector,
    hp: Biquad,
    hf_noise: Vec<f32>,
    hf_ready: bool,
    gate_gain: f32,
    hangover: usize,
    provisional: usize,
    norm_rms: f32,
}

impl ChannelEngine {
    pub fn new(sample_rate: f32) -> Self {
        let raw = (MODEL_NFFT * sample_rate / MODEL_SR).round() as usize;
        let nfft = if raw % 2 == 0 { raw } else { raw + 1 };
        let hop = nfft / 2;
        let bins = nfft / 2 + 1;
        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(nfft);
        let c2r = planner.plan_fft_inverse(nfft);
        let window: Vec<f32> = (0..nfft)
            .map(|i| {
                let phase = 2.0 * PI * i as f32 / nfft as f32;
                (0.5 * (1.0 - phase.cos())).sqrt()
            })
            .collect();

        Self {
            sample_rate,
            nfft,
            hop,
            scale: MODEL_NFFT / nfft as f32,
            analysis_window: vec![0.0; nfft],
            window,
            hop_input: vec![0.0; hop],
            hop_pos: 0,
            fft_in: vec![0.0; nfft],
            fft_out: vec![Complex::new(0.0, 0.0); bins],
            ifft_out: vec![0.0; nfft],
            overlap: vec![0.0; nfft],
            output: VecDeque::with_capacity(nfft * 2),
            r2c,
            c2r,
            gtcrn: Gtcrn::new(),
            artifact: ArtifactDetector::new(),
            hp: Biquad::highpass(70.0, sample_rate),
            hf_noise: vec![1e-6; bins.saturating_sub(NUM_BINS)],
            hf_ready: false,
            gate_gain: 1.0,
            hangover: 0,
            provisional: 0,
            norm_rms: 0.0,
        }
    }

    pub fn reported_latency_samples(&self) -> u32 {
        self.hop as u32
    }

    pub fn reset(&mut self) {
        self.analysis_window.fill(0.0);
        self.hop_input.fill(0.0);
        self.hop_pos = 0;
        self.fft_in.fill(0.0);
        self.fft_out.fill(Complex::new(0.0, 0.0));
        self.ifft_out.fill(0.0);
        self.overlap.fill(0.0);
        self.output.clear();
        self.hf_noise.fill(1e-6);
        self.hf_ready = false;
        self.gate_gain = 1.0;
        self.hangover = 0;
        self.provisional = 0;
        self.norm_rms = 0.0;
        self.hp.reset();
        self.artifact.reset();
        if let Some(model) = self.gtcrn.as_mut() {
            model.reset();
        }
    }

    #[inline]
    pub fn process_sample(&mut self, input: f32, speech_prob: f32, settings: Settings) -> f32 {
        let x = self.hp.process(input);
        self.hop_input[self.hop_pos] = x;
        self.hop_pos += 1;

        if self.hop_pos == self.hop {
            self.process_frame(speech_prob, settings);
            self.hop_pos = 0;
        }

        self.output.pop_front().unwrap_or(0.0)
    }

    fn process_frame(&mut self, speech_prob: f32, settings: Settings) {
        self.analysis_window.copy_within(self.hop.., 0);
        self.analysis_window[self.nfft - self.hop..].copy_from_slice(&self.hop_input);

        for i in 0..self.nfft {
            self.fft_in[i] = self.analysis_window[i] * self.window[i];
        }
        if self.r2c.process(&mut self.fft_in, &mut self.fft_out).is_err() {
            self.output.extend(self.hop_input.iter().copied());
            return;
        }

        let mut original = [(0.0_f32, 0.0_f32); NUM_BINS];
        for i in 0..NUM_BINS {
            original[i] = (self.fft_out[i].re * self.scale, self.fft_out[i].im * self.scale);
        }

        let decision = self.artifact.analyze(&original, speech_prob);
        let gate_target = self.gate_target(speech_prob, decision, settings);

        let norm_gain = self.normalization_gain(&original);
        let mut model_input = original;
        if norm_gain > 1.01 {
            for b in &mut model_input {
                b.0 *= norm_gain;
                b.1 *= norm_gain;
            }
        }

        let enhanced = self.gtcrn.as_mut()
            .and_then(|m| m.process(&model_input))
            .map(|mut y| {
                if norm_gain > 1.01 {
                    let inv = 1.0 / norm_gain;
                    for b in &mut y {
                        b.0 *= inv;
                        b.1 *= inv;
                    }
                }
                y
            })
            .unwrap_or(original);

        let speech_strength = settings.strength * (1.0 - 0.72 * settings.voice_protect);
        let mut nr_strength = if speech_prob > 0.55 { speech_strength } else { settings.strength };
        if decision.confidence > 0.65 && speech_prob < 0.45 {
            nr_strength = (nr_strength + decision.confidence * settings.artifact * 0.40).clamp(0.0, 1.0);
        }

        let inv_scale = 1.0 / self.scale;
        for i in 0..NUM_BINS {
            let mix = nr_strength.clamp(0.0, 1.0);
            let mut re = original[i].0 * (1.0 - mix) + enhanced[i].0 * mix;
            let mut im = original[i].1 * (1.0 - mix) + enhanced[i].1 * mix;

            if decision.plosive > 0.10 && i <= 10 {
                let hz = i as f32 * 31.25;
                let freq_mix = if hz <= 150.0 { 1.0 } else { ((250.0 - hz) / 100.0).clamp(0.0, 1.0) };
                let reduction = 1.0 - decision.plosive * settings.artifact * 0.70 * freq_mix;
                re *= reduction;
                im *= reduction;
            }

            self.fft_out[i] = Complex::new(re * inv_scale, im * inv_scale);
        }

        self.process_high_band(speech_prob, decision, gate_target, settings);

        self.fft_out[0].im = 0.0;
        if let Some(last) = self.fft_out.last_mut() {
            last.im = 0.0;
        }

        if self.c2r.process(&mut self.fft_out, &mut self.ifft_out).is_err() {
            self.output.extend(self.hop_input.iter().copied());
            return;
        }

        let scale = 1.0 / self.nfft as f32;
        for i in 0..self.nfft {
            self.overlap[i] += self.ifft_out[i] * scale * self.window[i];
        }

        let attack_coeff = coeff_ms(2.0, self.sample_rate);
        let release_coeff = coeff_ms(28.0, self.sample_rate);
        for i in 0..self.hop {
            let coeff = if gate_target > self.gate_gain { attack_coeff } else { release_coeff };
            self.gate_gain = gate_target + coeff * (self.gate_gain - gate_target);
            let y = (self.overlap[i] * self.gate_gain).clamp(-0.98, 0.98);
            self.output.push_back(y);
        }

        self.overlap.copy_within(self.hop.., 0);
        self.overlap[self.nfft - self.hop..].fill(0.0);
    }

    fn normalization_gain(&mut self, spectrum: &[(f32, f32); NUM_BINS]) -> f32 {
        let rms = (spectrum.iter().map(|&(re, im)| re * re + im * im).sum::<f32>() / NUM_BINS as f32).sqrt();
        self.norm_rms = if self.norm_rms < 1e-6 { rms } else { 0.88 * self.norm_rms + 0.12 * rms };
        if self.norm_rms > 0.015 && self.norm_rms < 0.35 {
            (0.35 / self.norm_rms).min(2.5)
        } else {
            1.0
        }
    }

    fn gate_target(&mut self, speech: f32, decision: ArtifactDecision, s: Settings) -> f32 {
        let threshold = 0.50 - 0.20 * s.voice_protect;
        let speech_now = speech >= threshold;
        let strong_onset = decision.burst > 0.28;
        let hard_artifact = decision.confidence * s.artifact > (0.76 + 0.12 * s.voice_protect);

        if speech_now {
            self.hangover = 9;
            self.provisional = 0;
            return 1.0;
        }

        if strong_onset && !hard_artifact {
            self.provisional = 3;
        }

        if self.provisional > 0 {
            self.provisional -= 1;
            if hard_artifact && speech < 0.18 {
                return s.floor_gain;
            }
            return 1.0;
        }

        if self.hangover > 0 {
            self.hangover -= 1;
            if hard_artifact && speech < 0.12 {
                return (0.25 + 0.75 * s.floor_gain).clamp(s.floor_gain, 1.0);
            }
            return 1.0;
        }

        if hard_artifact || speech < threshold * 0.65 {
            s.floor_gain
        } else {
            let t = ((speech - threshold * 0.65) / (threshold * 0.35)).clamp(0.0, 1.0);
            s.floor_gain + (1.0 - s.floor_gain) * t
        }
    }

    fn process_high_band(&mut self, speech: f32, decision: ArtifactDecision, gate_target: f32, s: Settings) {
        if self.fft_out.len() <= NUM_BINS {
            return;
        }
        let hf_count = self.fft_out.len() - NUM_BINS;
        if self.hf_noise.len() != hf_count {
            self.hf_noise.resize(hf_count, 1e-6);
        }

        if speech < 0.18 {
            for i in 0..hf_count {
                let mag = self.fft_out[NUM_BINS + i].norm();
                let alpha = if !self.hf_ready || mag < self.hf_noise[i] * 2.0 { 0.035 } else { 0.002 };
                self.hf_noise[i] = self.hf_noise[i] * (1.0 - alpha) + mag * alpha;
            }
            self.hf_ready = true;
        }

        let speech_air = (0.35 + 0.65 * s.air).clamp(0.0, 1.0);
        let artifact_cut = 1.0 - decision.confidence * s.artifact * (1.0 - speech).clamp(0.0, 1.0);
        for i in 0..hf_count {
            let idx = NUM_BINS + i;
            let mag = self.fft_out[idx].norm();
            let snr = if self.hf_ready { mag / (self.hf_noise[i] + 1e-8) } else { 4.0 };
            let snr_gain = ((snr - 1.0) / 3.0).clamp(0.0, 1.0);
            let base = if speech > 0.45 {
                speech_air * (0.45 + 0.55 * snr_gain)
            } else {
                gate_target * snr_gain
            };
            let gain = (base * artifact_cut).clamp(s.floor_gain, 1.0);
            self.fft_out[idx].re *= gain;
            self.fft_out[idx].im *= gain;
        }
    }
}

#[inline]
fn coeff_ms(ms: f32, sample_rate: f32) -> f32 {
    (-1.0 / (0.001 * ms * sample_rate).max(1.0)).exp()
}
