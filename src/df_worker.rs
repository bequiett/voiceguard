use std::{path::PathBuf, thread};

use crossbeam_channel::{bounded, Receiver, Sender, TryRecvError, TrySendError};
use deepfilter_rt::{DeepFilterProcessor, HOP_SIZE};

pub struct DfWorker {
    tx: Sender<Vec<f32>>,
    rx: Receiver<Vec<f32>>,
}

impl DfWorker {
    pub fn new(model_dir: PathBuf) -> Self {
        let (tx_in, rx_in) = bounded::<Vec<f32>>(8);
        let (tx_out, rx_out) = bounded::<Vec<f32>>(8);

        thread::Builder::new()
            .name("voiceguard-dfn".into())
            .spawn(move || {
                let mut processor = match DeepFilterProcessor::with_threads(&model_dir, 2) {
                    Ok(mut p) => {
                        let _ = p.warmup();
                        p
                    }
                    Err(_) => return,
                };

                while let Ok(input) = rx_in.recv() {
                    let mut output = vec![0.0; HOP_SIZE];
                    if processor.process_frame(&input, &mut output).is_err() {
                        output.copy_from_slice(&input);
                    }
                    if tx_out.send(output).is_err() {
                        break;
                    }
                }
            })
            .ok();

        Self { tx: tx_in, rx: rx_out }
    }

    pub fn submit(&self, frame: Vec<f32>) {
        match self.tx.try_send(frame) {
            Ok(_) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
        }
    }

    pub fn take(&self) -> Option<Vec<f32>> {
        match self.rx.try_recv() {
            Ok(frame) => Some(frame),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}
