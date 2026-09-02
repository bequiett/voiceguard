use std::f32::consts::PI;

#[derive(Clone, Copy)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub fn highpass(cutoff_hz: f32, sample_rate: f32) -> Self {
        let q = 0.70710677_f32;
        let w0 = 2.0 * PI * cutoff_hz / sample_rate;
        let cos = w0.cos();
        let sin = w0.sin();
        let alpha = sin / (2.0 * q);

        let mut b0 = (1.0 + cos) * 0.5;
        let mut b1 = -(1.0 + cos);
        let mut b2 = (1.0 + cos) * 0.5;
        let a0 = 1.0 + alpha;
        let mut a1 = -2.0 * cos;
        let mut a2 = 1.0 - alpha;

        b0 /= a0;
        b1 /= a0;
        b2 /= a0;
        a1 /= a0;
        a2 /= a0;

        Self { b0, b1, b2, a1, a2, z1: 0.0, z2: 0.0 }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}
