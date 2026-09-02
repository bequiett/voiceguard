use crate::gtcrn::NUM_BINS;

#[derive(Clone, Copy, Default)]
pub struct ArtifactDecision {
    pub confidence: f32,
    pub plosive: f32,
    pub burst: f32,
}

pub struct ArtifactDetector {
    prev_mag: [f32; NUM_BINS],
    noise_energy: f32,
    initialized: bool,
}

impl ArtifactDetector {
    pub fn new() -> Self {
        Self {
            prev_mag: [0.0; NUM_BINS],
            noise_energy: 1e-7,
            initialized: false,
        }
    }

    pub fn reset(&mut self) {
        self.prev_mag.fill(0.0);
        self.noise_energy = 1e-7;
        self.initialized = false;
    }

    pub fn analyze(&mut self, spectrum: &[(f32, f32); NUM_BINS], speech_prob: f32) -> ArtifactDecision {
        let mut total = 0.0_f32;
        let mut weighted = 0.0_f32;
        let mut log_sum = 0.0_f32;
        let mut mag_sum = 0.0_f32;
        let mut flux = 0.0_f32;
        let mut low = 0.0_f32;
        let mut high = 0.0_f32;

        for (i, &(re, im)) in spectrum.iter().enumerate() {
            let power = re * re + im * im + 1e-12;
            let mag = power.sqrt();
            total += power;
            mag_sum += mag;
            weighted += mag * i as f32;
            log_sum += mag.max(1e-9).ln();
            let d = mag - self.prev_mag[i];
            if d > 0.0 {
                flux += d;
            }
            self.prev_mag[i] = mag;
            if i <= 7 {
                low += power;
            }
            if i >= 64 {
                high += power;
            }
        }

        let mean_mag = mag_sum / NUM_BINS as f32;
        let flatness = (log_sum / NUM_BINS as f32).exp() / (mean_mag + 1e-9);
        let centroid = weighted / (mag_sum + 1e-9) / (NUM_BINS - 1) as f32;
        let flux_n = flux / (mag_sum + 1e-9);
        let low_ratio = low / (total + 1e-12);
        let high_ratio = high / (total + 1e-12);

        if !self.initialized {
            self.noise_energy = total.max(1e-8);
            self.initialized = true;
        }
        if speech_prob < 0.20 {
            let alpha = if total < self.noise_energy * 2.0 { 0.06 } else { 0.005 };
            self.noise_energy = self.noise_energy * (1.0 - alpha) + total * alpha;
        }
        let burst_ratio = total / (self.noise_energy + 1e-10);
        let non_voice = (1.0 - speech_prob).clamp(0.0, 1.0);

        let keyboard = ramp(flux_n, 0.10, 0.42)
            * ramp(centroid, 0.18, 0.50)
            * ramp(burst_ratio, 1.8, 8.0)
            * non_voice;

        let broad_burst = ramp(burst_ratio, 2.5, 14.0)
            * ramp(flux_n, 0.06, 0.34)
            * ramp(flatness, 0.16, 0.55)
            * non_voice;

        let breath_wind = ramp(flatness, 0.28, 0.68)
            * ramp(high_ratio, 0.10, 0.42)
            * ramp(burst_ratio, 1.2, 5.0)
            * non_voice
            * 0.80;

        let body_low = ramp(low_ratio, 0.18, 0.62)
            * ramp(burst_ratio, 2.0, 10.0)
            * non_voice
            * 0.78;

        let confidence = keyboard.max(broad_burst).max(breath_wind).max(body_low).clamp(0.0, 1.0);

        let plosive = (ramp(low_ratio, 0.20, 0.62)
            * ramp(burst_ratio, 2.2, 10.0)
            * (1.0 - ramp(centroid, 0.22, 0.48)))
            .clamp(0.0, 1.0);

        ArtifactDecision {
            confidence,
            plosive,
            burst: ramp(burst_ratio, 3.0, 14.0),
        }
    }
}

#[inline]
fn ramp(x: f32, lo: f32, hi: f32) -> f32 {
    ((x - lo) / (hi - lo)).clamp(0.0, 1.0)
}
