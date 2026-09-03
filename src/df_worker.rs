use std::{path::PathBuf, thread};

use crossbeam_channel::{bounded, Receiver, Sender, TryRecvError, TrySendError};
use deepfilter_rt::{DeepFilterProcessor, HOP_SIZE};

pub struct DfWorker {
    tx: Option<Sender<Vec<f32>>>,
    rx: Option<Receiver<Vec<f32>>>,
}

impl DfWorker {
    pub fn new(model_dir: PathBuf) -> Self {
        let mut processor = match DeepFilterProcessor::with_threads(&model_dir, 2) {
            Ok(mut p) => {
                if p.warmup().is_err() {
                    return Self { tx: None, rx: None };
                }
                p
            }
            Err(_) => return Self { tx: None, rx: None },
        };

        let (tx_in, rx_in) = bounded::<Vec<f32>>(8);
        let (tx_out, rx_out) = bounded::<Vec<f32>>(8);

        if thread::Builder::new()
            .name("voiceguard-dfn".into())
            .spawn(move || {
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
            .is_err()
        {
            return Self { tx: None, rx: None };
        }

        Self { tx: Some(tx_in), rx: Some(rx_out) }
    }

    pub fn submit(&self, frame: Vec<f32>) -> bool {
        let Some(tx) = &self.tx else { return false; };
        match tx.try_send(frame) {
            Ok(_) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
        }
    }

    pub fn take(&self) -> Option<Vec<f32>> {
        let Some(rx) = &self.rx else { return None; };
        match rx.try_recv() {
            Ok(frame) => Some(frame),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}
