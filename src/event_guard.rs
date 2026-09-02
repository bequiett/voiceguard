use std::f32::consts::PI;

use realfft::{num_complex::Complex, RealFftPlanner, RealToComplex};

#[derive(Clone, Copy, Default)]
pub struct EventDecision {
    pub transient: f32,
    pub wind: f32,
    pub breath: f32,
}

pub struct EventGuard {
    fft: std::sync::Arc<dyn RealToComplex<f32>>,
    input: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    prev_mag: Vec<f32>,
    noise_energy: f32,
    breath_state: f32,
    wind_state: f32,
}

impl EventGuard {
    pub fn new() -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(480);
        Self {
            fft,
            input: vec![0.0; 480],
            spectrum: vec![Complex::new(0.0, 0.0); 241],
            prev_mag: vec![0.0; 241],
            noise_energy: 1e-7,
            breath_state: 0.0,
            wind_state: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.prev_mag.fill(0.0);
        self.noise_energy = 1e-7;
        self.breath_state = 0.0;
        self.wind_state = 0.0;
    }

    pub fn analyze(&mut self, frame: &[f32]) -> EventDecision {
        for (i, x) in frame.iter().take(480).enumerate() {
            let w = 0.5 - 0.5 * (2.0 * PI * i as f32 / 479.0).cos();
            self.input[i] = *x * w;
        }
        if self.fft.process(&mut self.input, &mut self.spectrum).is_err() {
            return EventDecision::default();
        }

        let mut total = 0.0_f32;
        let mut low = 0.0_f32;
        let mut voice = 0.0_f32;
        let mut high = 0.0_f32;
        let mut flux = 0.0_f32;
        let mut peak = 0.0_f32;
        let mut mag_sum = 0.0_f32;
        let mut log_sum = 0.0_f32;

        for (i, c) in self.spectrum.iter().enumerate() {
            let mag = c.norm() + 1e-10;
            let p = mag * mag;
            total += p;
            mag_sum += mag;
            log_sum += mag.ln();
            peak = peak.max(mag);
            flux += (mag - self.prev_mag[i]).max(0.0);
            self.prev_mag[i] = mag;
            let hz = i as f32 * 100.0;
            if hz < 300.0 { low += p; }
            if (300.0..=4000.0).contains(&hz) { voice += p; }
            if hz > 5000.0 { high += p; }
        }

        let flatness = (log_sum / self.spectrum.len() as f32).exp()
            / (mag_sum / self.spectrum.len() as f32 + 1e-9);
        let low_ratio = low / (total + 1e-10);
        let voice_ratio = voice / (total + 1e-10);
        let high_ratio = high / (total + 1e-10);
        let flux_n = flux / (mag_sum + 1e-9);
        let crest = peak / (mag_sum / self.spectrum.len() as f32 + 1e-9);
        let burst = total / (self.noise_energy + 1e-10);

        if burst < 2.0 {
            self.noise_energy = self.noise_energy * 0.96 + total * 0.04;
        } else {
            self.noise_energy = self.noise_energy * 0.997 + total * 0.003;
        }

        let transient = (ramp(flux_n, 0.08, 0.34)
            * ramp(crest, 3.0, 11.0)
            * ramp(burst, 1.7, 8.0))
            .clamp(0.0, 1.0);
        let wind_raw = (ramp(low_ratio, 0.34, 0.78)
            * ramp(flatness, 0.12, 0.48)
            * (1.0 - ramp(voice_ratio, 0.45, 0.82)))
            .clamp(0.0, 1.0);
        let breath_raw = (ramp(flatness, 0.24, 0.64)
            * ramp(high_ratio, 0.05, 0.28)
            * (1.0 - ramp(voice_ratio, 0.52, 0.86)))
            .clamp(0.0, 1.0);

        self.wind_state = smooth_state(self.wind_state, wind_raw, 0.55, 0.94);
        self.breath_state = smooth_state(self.breath_state, breath_raw, 0.48, 0.92);

        EventDecision { transient, wind: self.wind_state, breath: self.breath_state }
    }
}

fn smooth_state(old: f32, new: f32, attack: f32, release: f32) -> f32 {
    let a = if new > old { attack } else { release };
    old * a + new * (1.0 - a)
}

fn ramp(x: f32, lo: f32, hi: f32) -> f32 {
    ((x - lo) / (hi - lo)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impulse_is_transient() {
        let mut guard = EventGuard::new();
        for _ in 0..8 { let _ = guard.analyze(&vec![0.0001; 480]); }
        let mut frame = vec![0.0; 480];
        frame[100] = 1.0;
        let d = guard.analyze(&frame);
        assert!(d.transient > 0.35, "{}", d.transient);
    }

    #[test]
    fn tone_is_not_a_breath() {
        let mut guard = EventGuard::new();
        let frame: Vec<f32> = (0..480)
            .map(|i| (2.0 * PI * 180.0 * i as f32 / 48_000.0).sin() * 0.2)
            .collect();
        let d = guard.analyze(&frame);
        assert!(d.breath < 0.30, "{}", d.breath);
    }
}
