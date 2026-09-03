use std::{f32::consts::PI, path::PathBuf, thread};

use crossbeam_channel::{bounded, Receiver, Sender, TryRecvError};
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::TensorRef,
};
use realfft::{num_complex::Complex, ComplexToReal, RealFftPlanner, RealToComplex};

pub const HOP: usize = 480;
const FFT: usize = 960;
const BINS: usize = FFT / 2 + 1;

pub struct DpdfWorker {
    tx: Option<Sender<Vec<f32>>>,
    rx: Receiver<Vec<f32>>,
}

impl DpdfWorker {
    pub fn new(model: PathBuf) -> Self {
        let (tx_in, rx_in) = bounded::<Vec<f32>>(6);
        let (tx_out, rx_out) = bounded::<Vec<f32>>(6);
        let (ready_tx, ready_rx) = bounded::<bool>(1);

        thread::Builder::new()
            .name("voiceguard-dpdf".into())
            .spawn(move || {
                let mut processor = match DpdfProcessor::new(&model) {
                    Ok(p) => {
                        let _ = ready_tx.send(true);
                        p
                    }
                    Err(_) => {
                        let _ = ready_tx.send(false);
                        return;
                    }
                };
                while let Ok(input) = rx_in.recv() {
                    let output = processor.process_hop(&input).unwrap_or(input);
                    if tx_out.send(output).is_err() { break; }
                }
            })
            .ok();

        let available = ready_rx.recv().unwrap_or(false);
        Self { tx: available.then_some(tx_in), rx: rx_out }
    }

    pub fn submit(&self, frame: Vec<f32>) -> bool {
        let Some(tx) = &self.tx else { return false; };
        matches!(tx.try_send(frame), Ok(()))
    }

    pub fn take(&self) -> Option<Vec<f32>> {
        match self.rx.try_recv() {
            Ok(frame) => Some(frame),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}

struct DpdfProcessor {
    session: Session,
    state: Vec<f32>,
    input: Vec<f32>,
    window: Vec<f32>,
    fft_in: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    model_spec: Vec<f32>,
    ifft: Vec<f32>,
    overlap: Vec<f32>,
    r2c: std::sync::Arc<dyn RealToComplex<f32>>,
    c2r: std::sync::Arc<dyn ComplexToReal<f32>>,
}

impl DpdfProcessor {
    fn new(path: &std::path::Path) -> Result<Self, ()> {
        let session = Session::builder().map_err(|_| ())?
            .with_optimization_level(GraphOptimizationLevel::Level3).map_err(|_| ())?
            .with_intra_threads(2).map_err(|_| ())?
            .with_inter_threads(1).map_err(|_| ())?
            .commit_from_file(path).map_err(|_| ())?;

        let metadata = session.metadata().map_err(|_| ())?;
        let state_size = metadata.custom("state_size").ok_or(())?.parse::<usize>().map_err(|_| ())?;
        let erb_size = metadata.custom("erb_norm_state_size").ok_or(())?.parse::<usize>().map_err(|_| ())?;
        let spec_size = metadata.custom("spec_norm_state_size").ok_or(())?.parse::<usize>().map_err(|_| ())?;
        let erb = parse_list(metadata.custom("erb_norm_init").ok_or(())?);
        let spec = parse_list(metadata.custom("spec_norm_init").ok_or(())?);
        if erb.len() != erb_size || spec.len() != spec_size || erb_size + spec_size > state_size { return Err(()); }
        let mut state = vec![0.0; state_size];
        state[..erb_size].copy_from_slice(&erb);
        state[erb_size..erb_size + spec_size].copy_from_slice(&spec);
        drop(metadata);

        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(FFT);
        let c2r = planner.plan_fft_inverse(FFT);
        let window = (0..FFT).map(|i| {
            let s = (0.5 * PI * (i as f32 + 0.5) / (FFT as f32 / 2.0)).sin();
            (0.5 * PI * s * s).sin()
        }).collect();

        Ok(Self {
            session, state, input: vec![0.0; FFT], window,
            fft_in: vec![0.0; FFT], spectrum: vec![Complex::new(0.0, 0.0); BINS],
            model_spec: vec![0.0; BINS * 2], ifft: vec![0.0; FFT], overlap: vec![0.0; FFT],
            r2c, c2r,
        })
    }

    fn process_hop(&mut self, hop: &[f32]) -> Result<Vec<f32>, ()> {
        if hop.len() != HOP { return Err(()); }
        self.input.copy_within(HOP.., 0);
        self.input[FFT - HOP..].copy_from_slice(hop);
        for i in 0..FFT { self.fft_in[i] = self.input[i] * self.window[i]; }
        self.r2c.process(&mut self.fft_in, &mut self.spectrum).map_err(|_| ())?;
        for i in 0..BINS {
            self.model_spec[i * 2] = self.spectrum[i].re;
            self.model_spec[i * 2 + 1] = self.spectrum[i].im;
        }

        let spec = TensorRef::from_array_view(([1_usize, 1, BINS, 2], self.model_spec.as_slice())).map_err(|_| ())?;
        let state = TensorRef::from_array_view(([self.state.len()], self.state.as_slice())).map_err(|_| ())?;
        let outputs = self.session.run(ort::inputs![spec, state]).map_err(|_| ())?;
        let (_, enhanced) = outputs[0].try_extract_tensor::<f32>().map_err(|_| ())?;
        let (_, next_state) = outputs[1].try_extract_tensor::<f32>().map_err(|_| ())?;
        if enhanced.len() < BINS * 2 || next_state.len() != self.state.len() { return Err(()); }
        self.state.copy_from_slice(next_state);
        for i in 0..BINS { self.spectrum[i] = Complex::new(enhanced[i * 2], enhanced[i * 2 + 1]); }
        self.spectrum[0].im = 0.0;
        if let Some(x) = self.spectrum.last_mut() { x.im = 0.0; }
        self.c2r.process(&mut self.spectrum, &mut self.ifft).map_err(|_| ())?;
        for i in 0..FFT { self.overlap[i] += self.ifft[i] / FFT as f32 * self.window[i]; }
        let out = self.overlap[..HOP].to_vec();
        self.overlap.copy_within(HOP.., 0);
        self.overlap[FFT - HOP..].fill(0.0);
        Ok(out)
    }
}

fn parse_list(s: String) -> Vec<f32> {
    s.split(',').filter_map(|x| x.parse::<f32>().ok()).collect()
}
