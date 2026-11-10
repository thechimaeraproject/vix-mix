//! The tick must never touch the allocator once its thread has run one: eight channels,
//! chains on three of them and on both mixes, at varying cycle lengths within one prepared
//! graph and across a graph swapped in mid-run, through the null backend exactly as the
//! PipeWire data thread will call it, its meters, counters, busy time and worst cycle written
//! and read as it goes.
//!
//! `assert_no_alloc`'s allocator aborts this test binary on the first allocation inside the
//! closure (`memory allocation of N bytes failed`), so a regression cannot pass quietly. The
//! first tick runs outside it: `arc_swap` registers the thread's debt slot on first use.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use common::{Probe, TestRack, engine, insert, setup};
use vixmix_engine::{MASTER, MONITOR, NullBackend};
use vixmix_model::{Command, MixId, apply, snapshot, taper};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn eight_channels_with_chains_tick_without_allocating_from_the_second_block() {
    let mut setup = setup(8);
    let ch = setup.channels[3].id;
    let _ = apply(
        &mut setup,
        Command::SetMute {
            ch,
            mix: MixId::Monitor,
            mute: true,
        },
    )
    .unwrap();
    let ch = setup.channels[5].id;
    let _ = apply(
        &mut setup,
        Command::SetChainRouting {
            ch,
            mix: MixId::Master,
            on: false,
        },
    )
    .unwrap();
    let _ = apply(&mut setup, Command::SetPreview(true)).unwrap();

    let rack = TestRack::new();
    let mut logs = Vec::new();
    for instance in 1..=7 {
        logs.push(rack.add_probe(instance, Probe::new(0.5).0));
    }
    let with_chains = |setup: &vixmix_model::Setup| {
        let mut snap = snapshot(setup);
        snap.channels[0].chain = vec![insert(1), insert(2)];
        snap.channels[2].chain = vec![insert(3)];
        snap.channels[5].chain = vec![insert(4)];
        snap.monitor.chain = vec![insert(5)];
        snap.master.chain = vec![insert(6), insert(7)];
        snap
    };

    let engine = engine();
    engine.prepare(&with_chains(&setup), &*rack);
    assert!(engine.problems().is_empty(), "{:?}", engine.problems());
    let mut null = NullBackend::new(Arc::clone(&engine), 1024);
    for index in 0..8 {
        let (left, right) = null.input_mut(index).unwrap();
        for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            *l = ((i + index) as f32 * 0.01).sin() * 0.25;
            *r = -*l;
        }
    }

    // The warm-up tick.
    null.pump(256);
    let busy = engine.dsp().busy_ns();
    let mut worst = 0;
    for frames in [64, 256, 1024, 256, 64] {
        assert_no_alloc(|| {
            null.pump(frames);
            let _ = engine.meters().tap(0, MONITOR).take();
            let _ = engine.meters().bus(MASTER).take();
            let _ = engine.stats().dsp_load_permille.load(Ordering::Relaxed);
            worst = worst.max(engine.dsp().take_peak());
        });
    }
    // The busy time and the worst cycle were written inside, not merely allowed.
    assert!(
        engine.dsp().busy_ns() > busy,
        "each tick adds its busy time"
    );
    assert!(
        worst > 0,
        "and keeps the worst cycle's load until it is taken"
    );

    // A fader move swaps a new graph in (built on this, the host, thread). Its chains are
    // new, and their first block (the kit's "every parameter" block) allocates nothing.
    let ch = setup.channels[0].id;
    let _ = apply(
        &mut setup,
        Command::SetSend {
            ch,
            mix: MixId::Master,
            position: taper::db_to_position(-12.0),
        },
    )
    .unwrap();
    let (before, _) = engine.generations();
    engine.prepare(&with_chains(&setup), &*rack);
    for frames in [1024, 64, 256] {
        assert_no_alloc(|| {
            null.pump(frames);
            let _ = engine.meters().bus(MONITOR).take();
            let _ = engine.dsp().take_peak();
        });
    }
    assert_eq!(engine.generations(), (before + 1, before + 1));
    assert!(null.master().0.iter().any(|s| *s != 0.0));
    assert!(
        logs.iter()
            .all(|log| log.blocks.load(Ordering::SeqCst) == 9),
        "every insert ran every block"
    );
}
