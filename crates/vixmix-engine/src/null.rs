//! [`NullBackend`]: the engine with no audio device. It owns every buffer a tick borrows,
//! so a test, or a platform with no backend (Windows today), can pump frames through
//! [`Engine::process`] exactly as PipeWire's data thread will.

use std::sync::Arc;

use vixmix_model::MAX_CHANNELS;

use crate::engine::{ChannelInput, Engine, MAX_QUANTUM, Tick};

/// Pumps given inputs through an engine and keeps what it wrote.
pub struct NullBackend {
    engine: Arc<Engine>,
    max_frames: usize,
    inputs: [[Vec<f32>; 2]; MAX_CHANNELS],
    monitor: [Vec<f32>; 2],
    master: [Vec<f32>; 2],
    frames: usize,
}

impl NullBackend {
    /// A backend whose ticks are at most `max_frames` (clamped to 1..=[`MAX_QUANTUM`]), with
    /// silent inputs. Every buffer is allocated here, none in [`pump`](NullBackend::pump).
    pub fn new(engine: Arc<Engine>, max_frames: usize) -> NullBackend {
        let max_frames = max_frames.clamp(1, MAX_QUANTUM);
        let buffer = || vec![0.0f32; max_frames];
        NullBackend {
            engine,
            max_frames,
            inputs: std::array::from_fn(|_| [buffer(), buffer()]),
            monitor: [buffer(), buffer()],
            master: [buffer(), buffer()],
            frames: 0,
        }
    }

    /// The engine it drives.
    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    /// The longest tick it pumps.
    pub fn max_frames(&self) -> usize {
        self.max_frames
    }

    /// The left and right input of the channel at `index` in the snapshot, to fill before a
    /// pump; `None` past [`MAX_CHANNELS`]. Inputs keep their samples between pumps.
    pub fn input_mut(&mut self, index: usize) -> Option<(&mut [f32], &mut [f32])> {
        let [left, right] = self.inputs.get_mut(index)?;
        Some((left.as_mut_slice(), right.as_mut_slice()))
    }

    /// Silences every input.
    pub fn silence(&mut self) {
        for [left, right] in &mut self.inputs {
            left.fill(0.0);
            right.fill(0.0);
        }
    }

    /// One tick of `frames` (clamped to [`max_frames`](NullBackend::max_frames)) through
    /// [`Engine::process`]. Allocates nothing.
    pub fn pump(&mut self, frames: usize) {
        let n = frames.min(self.max_frames);
        let NullBackend {
            engine,
            inputs,
            monitor: [monitor_l, monitor_r],
            master: [master_l, master_r],
            ..
        } = self;
        let channel_inputs: [ChannelInput<'_>; MAX_CHANNELS] =
            std::array::from_fn(|i| ChannelInput {
                left: &inputs[i][0][..n],
                right: &inputs[i][1][..n],
            });
        engine.process(Tick {
            frames: n,
            channel_inputs: &channel_inputs,
            monitor_out: [&mut monitor_l[..n], &mut monitor_r[..n]],
            master_out: [&mut master_l[..n], &mut master_r[..n]],
        });
        self.frames = n;
    }

    /// What the last pump wrote to the Monitor output.
    pub fn monitor(&self) -> (&[f32], &[f32]) {
        (
            &self.monitor[0][..self.frames],
            &self.monitor[1][..self.frames],
        )
    }

    /// What the last pump wrote to the Master mix.
    pub fn master(&self) -> (&[f32], &[f32]) {
        (
            &self.master[0][..self.frames],
            &self.master[1][..self.frames],
        )
    }
}
