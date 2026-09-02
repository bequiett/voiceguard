use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::TensorRef,
};

const VAD_FRAME: usize = 512;
const CONTEXT: usize = 64;
const STATE_SIZE: usize = 2 * 1 * 128;

static MODEL: &[u8] = include_bytes!("../assets/silero_vad.onnx");

pub struct SpeechVad {
    session: Option<Session>,
    state: Vec<f32>,
    context: [f32; CONTEXT],
    frame: [f32; VAD_FRAME],
    frame_pos: usize,
    phase: f64,
    sample_rate: f64,
    probability: f32,
}

impl SpeechVad {
    pub fn new(sample_rate: f32) -> Self {
        let session = Session::builder().ok()
            .and_then(|b| b.with_optimization_level(GraphOptimizationLevel::Level3).ok())
            .and_then(|b| b.with_intra_threads(1).ok())
            .and_then(|b| b.with_inter_threads(1).ok())
            .and_then(|b| b.commit_from_memory(MODEL).ok());

        Self {
            session,
            state: vec![0.0; STATE_SIZE],
            context: [0.0; CONTEXT],
            frame: [0.0; VAD_FRAME],
            frame_pos: 0,
            phase: 0.0,
            sample_rate: sample_rate as f64,
            probability: 0.0,
        }
    }

    pub fn reset(&mut self, sample_rate: f32) {
        self.state.fill(0.0);
        self.context.fill(0.0);
        self.frame.fill(0.0);
        self.frame_pos = 0;
        self.phase = 0.0;
        self.sample_rate = sample_rate as f64;
        self.probability = 0.0;
    }

    #[inline]
    pub fn probability(&self) -> f32 {
        self.probability
    }

    pub fn push(&mut self, sample: f32) -> f32 {
        let ratio = 16_000.0 / self.sample_rate.max(16_000.0);
        self.phase += ratio;
        if self.phase < 1.0 {
            return self.probability;
        }
        self.phase -= 1.0;

        self.frame[self.frame_pos] = sample;
        self.frame_pos += 1;
        if self.frame_pos == VAD_FRAME {
            self.run_frame();
            self.context.copy_from_slice(&self.frame[VAD_FRAME - CONTEXT..]);
            self.frame_pos = 0;
        }
        self.probability
    }

    fn run_frame(&mut self) {
        let Some(session) = self.session.as_mut() else {
            self.fallback_probability();
            return;
        };

        let mut input = [0.0_f32; VAD_FRAME + CONTEXT];
        input[..CONTEXT].copy_from_slice(&self.context);
        input[CONTEXT..].copy_from_slice(&self.frame);
        let sr = [16_000_i64; 1];

        let Ok(input_tensor) = TensorRef::from_array_view(([1_usize, VAD_FRAME + CONTEXT], &input[..])) else {
            self.fallback_probability();
            return;
        };
        let Ok(state_tensor) = TensorRef::from_array_view(([2_usize, 1, 128], &self.state[..])) else {
            self.fallback_probability();
            return;
        };
        let Ok(sr_tensor) = TensorRef::from_array_view(([1_usize], &sr[..])) else {
            self.fallback_probability();
            return;
        };

        let Ok(outputs) = session.run(ort::inputs![input_tensor, state_tensor, sr_tensor]) else {
            self.fallback_probability();
            return;
        };
        let Ok((_, out)) = outputs[0].try_extract_tensor::<f32>() else {
            self.fallback_probability();
            return;
        };
        let Ok((_, state_out)) = outputs[1].try_extract_tensor::<f32>() else {
            self.fallback_probability();
            return;
        };
        if let Some(&p) = out.first() {
            self.probability = (self.probability * 0.30 + p.clamp(0.0, 1.0) * 0.70).clamp(0.0, 1.0);
        }
        if state_out.len() == STATE_SIZE {
            self.state.copy_from_slice(state_out);
        }
    }

    fn fallback_probability(&mut self) {
        let rms = (self.frame.iter().map(|x| x * x).sum::<f32>() / VAD_FRAME as f32).sqrt();
        let target = ((rms - 0.008) / 0.04).clamp(0.0, 1.0);
        self.probability = 0.7 * self.probability + 0.3 * target;
    }
}
