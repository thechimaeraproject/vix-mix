//! The edit path (change request 1 in `docs/build-map.md`): how a value set in a plugin's own
//! editor, or one the mixer sets on the plugin's behalf, reaches the processor a chain is
//! borrowing.
//!
//! The kit never short-cuts an edit to the processor: `performEdit` becomes
//! `ControllerEvent::PerformEdit` on the UI thread, and the processor hears a value only in a
//! block's `ParamChanges`. The setup keeps no plugin parameters (a plugin's parameters live in
//! its own state), so nothing in the snapshot carries the value either. Each instance therefore
//! has a bounded single-producer, single-consumer ring: the [`EditSender`] stays with the
//! instance on the UI thread, the consumer end lives in the rack beside the processor and
//! travels with the forwarding handle the rack lends, and the handle merges what it pops into
//! the changes the chain hands the plugin, at offset 0 of the next block. Nothing on the audio
//! side allocates: the merged list is sized when the instance is lent, and the block's nested
//! bus slices are rebuilt on the stack.

use std::collections::VecDeque;

use chimaeraproject_audio_processor::{AudioProcessData, AudioProcessor, MAX_CHANNELS};
use chimaeraproject_param::{NormalizedValue, ParamChange, ParamChanges, ParamId};

/// How many edits one instance's ring holds between two blocks.
pub const EDIT_RING: usize = 256;

/// How many parameters' latest values wait on the UI side while the ring is full.
const WAITING_LIMIT: usize = 256;

/// Room the chain's own changes may take in the merged list: the kit's chain sizes its list
/// for every declared parameter plus this many keys.
const CHAIN_KEY_ROOM: usize = 64;

/// The most output buses a block can have for the handle to merge edits into it. A mixer's
/// insert has one; a block with more is processed with the chain's changes alone and the
/// edits wait.
const MAX_BUSES: usize = 8;

/// The consumer end, which lives in the rack and travels with the lent handle.
pub(crate) type EditReceiver = rtrb::Consumer<ParamChange>;

/// A new ring for one instance: the UI thread's end and the audio side's.
pub(crate) fn ring() -> (EditSender, EditReceiver) {
    let (producer, consumer) = rtrb::RingBuffer::new(EDIT_RING);
    (
        EditSender {
            producer,
            waiting: VecDeque::new(),
            dropped: 0,
        },
        consumer,
    )
}

/// The capacity a lent handle's merged list needs for a processor declaring `params`
/// parameters: the chain's room plus a full ring.
pub(crate) fn merged_capacity(params: usize) -> usize {
    params + CHAIN_KEY_ROOM + EDIT_RING
}

/// The UI thread's end of one instance's edit ring.
pub struct EditSender {
    producer: rtrb::Producer<ParamChange>,
    /// The latest value of each parameter that did not fit, oldest first.
    waiting: VecDeque<ParamChange>,
    dropped: u64,
}

impl EditSender {
    /// Queues `value` (clamped to 0..=1) for `param`, for the processor's next block. While the
    /// ring is full (the instance is in no running chain) the latest value of each parameter
    /// waits here instead and goes first once there is room, so the processor always ends up
    /// hearing the last value sent. `false` when that list is full too and the edit was
    /// dropped.
    pub fn send(&mut self, param: ParamId, value: NormalizedValue) -> bool {
        self.flush();
        let change = ParamChange {
            id: param,
            normalized: value.clamp(0.0, 1.0),
            sample_offset: 0,
        };
        // Straight into the ring only when nothing waits: an older value of this parameter
        // still waiting would otherwise arrive after it and undo it.
        if self.waiting.is_empty() && self.producer.push(change).is_ok() {
            return true;
        }
        if let Some(older) = self.waiting.iter_mut().find(|c| c.id == param) {
            older.normalized = change.normalized;
            return true;
        }
        if self.waiting.len() < WAITING_LIMIT {
            self.waiting.push_back(change);
            return true;
        }
        self.dropped += 1;
        false
    }

    /// Moves waiting values into the ring as far as it has room, oldest first.
    pub fn flush(&mut self) {
        while let Some(&change) = self.waiting.front() {
            if self.producer.push(change).is_err() {
                break;
            }
            self.waiting.pop_front();
        }
    }

    /// Values waiting for room in the ring.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Edits dropped because the ring and the waiting list were both full.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

/// Audio thread: fills `merged` with the chain's `changes` (and their initial flag), then as
/// many edits from `edits` as fit, oldest first, so an edit wins over the chain's value at the
/// same offset. Edits that do not fit stay in the ring for the next block. Allocates nothing:
/// `merged` was sized off the audio thread and is only ever `try_push`ed.
pub(crate) fn merge(changes: &ParamChanges, edits: &mut EditReceiver, merged: &mut ParamChanges) {
    merged.clear();
    merged.set_initial(changes.is_initial());
    for change in changes.iter() {
        let _ = merged.try_push(*change);
    }
    while merged.len() < merged.capacity() {
        let Ok(edit) = edits.pop() else {
            break;
        };
        let _ = merged.try_push(edit);
    }
}

/// Whether [`process_with`] can rebuild `data`'s block.
pub(crate) fn fits(data: &AudioProcessData<'_>) -> bool {
    data.outputs.len() <= MAX_BUSES && data.outputs.iter().all(|bus| bus.len() <= MAX_CHANNELS)
}

/// Calls `processor` on `data`'s buffers with `changes` in place of `data`'s own. The nested
/// output slices cannot be re-pointed (`&mut` is invariant, so every level's lifetime would
/// have to be the caller's), so they are rebuilt here from reborrows, on the stack, as the
/// kit's `BusBuffer::with_process_data` builds them. The caller checks [`fits`] first; a bus
/// or channel past the limits would be left out.
pub(crate) fn process_with(
    processor: &mut dyn AudioProcessor,
    data: &mut AudioProcessData<'_>,
    changes: &ParamChanges,
) {
    let inputs = data.inputs;
    let num_samples = data.num_samples;
    let transport = data.transport;
    let buses = data.outputs.len().min(MAX_BUSES);
    let mut counts = [0usize; MAX_BUSES];
    let mut lanes: [[&mut [f32]; MAX_CHANNELS]; MAX_BUSES] = Default::default();
    for ((bus, row), count) in data
        .outputs
        .iter_mut()
        .zip(lanes.iter_mut())
        .zip(counts.iter_mut())
    {
        for (channel, lane) in bus.iter_mut().zip(row.iter_mut()) {
            *lane = &mut **channel;
            *count += 1;
        }
    }
    let mut outputs: [&mut [&mut [f32]]; MAX_BUSES] = Default::default();
    for ((slot, row), &count) in outputs.iter_mut().zip(lanes.iter_mut()).zip(&counts) {
        *slot = &mut row[..count];
    }
    let mut rebuilt = AudioProcessData {
        inputs,
        outputs: &mut outputs[..buses],
        num_samples,
        param_changes: changes,
        transport,
    };
    processor.process(&mut rebuilt);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimaeraproject_audio_processor::{BusBuffer, Gain, TransportInfo};

    fn drain(edits: &mut EditReceiver) -> Vec<(ParamId, NormalizedValue)> {
        let mut out = Vec::new();
        while let Ok(change) = edits.pop() {
            out.push((change.id, change.normalized));
        }
        out
    }

    #[test]
    fn edits_arrive_in_order_and_a_full_ring_keeps_each_parameters_latest_value() {
        let (mut sender, mut edits) = ring();
        assert!(sender.send(3, 0.25));
        assert!(sender.send(4, 1.5), "clamped, not refused");
        assert_eq!(drain(&mut edits), [(3, 0.25), (4, 1.0)]);

        // Nobody drains: the ring fills, then each parameter's latest value waits.
        for i in 0..EDIT_RING {
            assert!(sender.send(1, i as f64 / EDIT_RING as f64));
        }
        assert!(sender.send(7, 0.1));
        assert!(sender.send(8, 0.2));
        assert!(sender.send(7, 0.9));
        assert_eq!(sender.waiting(), 2, "one per parameter");

        // The audio side drains; the next send flushes the waiting values first, in order.
        assert_eq!(drain(&mut edits).len(), EDIT_RING);
        assert!(sender.send(8, 0.3));
        assert_eq!(drain(&mut edits), [(7, 0.9), (8, 0.2), (8, 0.3)]);
        assert_eq!(sender.waiting(), 0);
        assert_eq!(sender.dropped(), 0);
    }

    #[test]
    fn the_waiting_list_is_bounded_and_counts_what_it_drops() {
        let (mut sender, _edits) = ring();
        for i in 0..EDIT_RING as ParamId {
            assert!(sender.send(i, 0.5));
        }
        for i in 0..WAITING_LIMIT as ParamId {
            assert!(sender.send(1000 + i, 0.5));
        }
        assert!(!sender.send(5000, 0.5));
        assert!(
            sender.send(1000, 0.75),
            "a waiting parameter still takes its new value"
        );
        assert_eq!(sender.dropped(), 1);
        assert_eq!(sender.waiting(), WAITING_LIMIT);
    }

    #[test]
    fn a_merge_keeps_the_chains_changes_first_and_leaves_what_does_not_fit() {
        let (mut sender, mut edits) = ring();
        for (param, value) in [(1, 0.1), (2, 0.2), (3, 0.3)] {
            sender.send(param, value);
        }
        let mut chain = ParamChanges::initial();
        chain.push_value(9, 0.9);
        let mut merged = ParamChanges::with_capacity(3);
        let room = merged.capacity();
        merge(&chain, &mut edits, &mut merged);
        assert!(merged.is_initial(), "the first-block flag survives");
        let got: Vec<_> = merged.iter().map(|c| (c.id, c.normalized)).collect();
        assert_eq!(got[0], (9, 0.9));
        assert_eq!(got.len(), room.min(4), "only what fits");
        assert_eq!(merged.capacity(), room, "never grown");
        // What did not fit is still in the ring, in order, for the next block.
        let left = drain(&mut edits);
        assert_eq!(left.len(), 4 - got.len());
        if let Some(last) = left.last() {
            assert_eq!(*last, (3, 0.3));
        }
    }

    #[test]
    fn a_rebuilt_block_runs_on_the_same_samples_with_the_merged_changes() {
        let mut gain = Gain::new("test.gain");
        let mut bus = BusBuffer::new(2, 8);
        for channel in 0..2 {
            bus.channel_mut(channel)[..8].fill(1.0);
        }
        let transport = TransportInfo::stopped();
        let none = ParamChanges::new();
        // The gain runs linearly in decibels from MIN_DB to MAX_DB; ask for 6 dB down.
        let mut merged = ParamChanges::new();
        merged.push_value(
            Gain::GAIN_DB_ID,
            (-6.0 - Gain::MIN_DB) / (Gain::MAX_DB - Gain::MIN_DB),
        );
        bus.with_process_data(8, &none, &transport, |data| {
            assert!(fits(data));
            process_with(&mut gain, data, &merged);
        });
        let expected = 10f32.powf(-6.0 / 20.0);
        for channel in 0..2 {
            assert!(
                bus.channel(channel)[..8]
                    .iter()
                    .all(|s| (s - expected).abs() < 1e-6),
                "{:?}",
                bus.channel(channel)
            );
        }
    }
}
