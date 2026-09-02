use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::TensorRef,
};

pub const NUM_BINS: usize = 257;
const CONV_SIZE: usize = 2 * 1 * 16 * 16 * 33;
const TRA_SIZE: usize = 2 * 3 * 1 * 1 * 16;
const INTER_SIZE: usize = 2 * 1 * 33 * 16;
const INPUT_SIZE: usize = NUM_BINS * 2;

static MODEL: &[u8] = include_bytes!("../assets/gtcrn_simple.onnx");

pub struct Gtcrn {
    session: Session,
    conv: Vec<f32>,
    tra: Vec<f32>,
    inter: Vec<f32>,
    input: Vec<f32>,
    output: [(f32, f32); NUM_BINS],
}

impl Gtcrn {
    pub fn new() -> Option<Self> {
        let session = Session::builder().ok()?
            .with_optimization_level(GraphOptimizationLevel::Level3).ok()?
            .with_intra_threads(1).ok()?
            .with_inter_threads(1).ok()?
            .commit_from_memory(MODEL).ok()?;

        let mut model = Self {
            session,
            conv: vec![0.0; CONV_SIZE],
            tra: vec![0.0; TRA_SIZE],
            inter: vec![0.0; INTER_SIZE],
            input: vec![0.0; INPUT_SIZE],
            output: [(0.0, 0.0); NUM_BINS],
        };
        model.warm_up();
        Some(model)
    }

    pub fn reset(&mut self) {
        self.conv.fill(0.0);
        self.tra.fill(0.0);
        self.inter.fill(0.0);
        self.warm_up();
    }

    fn warm_up(&mut self) {
        let mut frame = [(0.0_f32, 0.0_f32); NUM_BINS];
        for (i, bin) in frame.iter_mut().enumerate() {
            let tilt = 1.0 - (i as f32 / NUM_BINS as f32) * 0.5;
            *bin = (0.0015 * tilt, 0.00035 * tilt);
        }
        for _ in 0..8 {
            let _ = self.process(&frame);
        }
    }

    pub fn process(&mut self, spectrum: &[(f32, f32); NUM_BINS]) -> Option<[(f32, f32); NUM_BINS]> {
        for (i, &(re, im)) in spectrum.iter().enumerate() {
            self.input[i * 2] = re;
            self.input[i * 2 + 1] = im;
        }

        let input = TensorRef::from_array_view(([1_usize, NUM_BINS, 1, 2], &self.input[..])).ok()?;
        let conv = TensorRef::from_array_view(([2_usize, 1, 16, 16, 33], &self.conv[..])).ok()?;
        let tra = TensorRef::from_array_view(([2_usize, 3, 1, 1, 16], &self.tra[..])).ok()?;
        let inter = TensorRef::from_array_view(([2_usize, 1, 33, 16], &self.inter[..])).ok()?;

        let outputs = self.session.run(ort::inputs![input, conv, tra, inter]).ok()?;
        let (_, enhanced) = outputs[0].try_extract_tensor::<f32>().ok()?;
        let (_, conv_out) = outputs[1].try_extract_tensor::<f32>().ok()?;
        let (_, tra_out) = outputs[2].try_extract_tensor::<f32>().ok()?;
        let (_, inter_out) = outputs[3].try_extract_tensor::<f32>().ok()?;

        if enhanced.len() < NUM_BINS * 2 || conv_out.len() != CONV_SIZE || tra_out.len() != TRA_SIZE || inter_out.len() != INTER_SIZE {
            return None;
        }

        self.conv.copy_from_slice(conv_out);
        self.tra.copy_from_slice(tra_out);
        self.inter.copy_from_slice(inter_out);
        for i in 0..NUM_BINS {
            self.output[i] = (enhanced[i * 2], enhanced[i * 2 + 1]);
        }
        Some(self.output)
    }
}
