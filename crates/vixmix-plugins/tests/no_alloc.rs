//! The lent handle must never touch the allocator on the audio thread, edits or none: the
//! test plugin is lent to a chain filled as the engine fills one, and blocks run with an edit
//! waiting in the ring, with several, and with none.
//!
//! `assert_no_alloc`'s allocator aborts this test binary on the first allocation inside the
//! closure, so a regression cannot pass quietly. It proves this crate's handle and the kit's
//! adapter, not the plugin, whose allocator is its own (a `cdylib` carries its own).

mod common;

use std::sync::Arc;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use chimaeraproject_audio_processor::{BusBuffer, TransportInfo};
use common::*;
use vixmix_model::InstanceId;
use vixmix_plugins::{Instances, Rack};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn blocks_with_edits_merged_into_them_allocate_nothing() {
    let (catalog, entry) = catalog_of(&plugin_dir("no-alloc"));
    let rack = Rack::new();
    let mut instances = Instances::new(host(), Arc::clone(&rack));
    let instance = InstanceId(1);
    instances
        .create(instance, &plugin_ref(&entry), &catalog, None)
        .unwrap();
    let desired = [insert(instance, &entry.id)];
    let mut chain = stereo_chain();
    fill(&mut chain, &desired, &*rack);
    let mut bus = BusBuffer::new(2, FRAMES);
    let transport = TransportInfo::playing_at(0, ENGINE.sample_rate);
    let mut block = |bus: &mut BusBuffer| {
        for channel in 0..2 {
            bus.channel_mut(channel)[..FRAMES].fill(1.0);
        }
        chain.process(bus, FRAMES, &transport, &desired);
    };

    // The warm-up block, with the first-block flag.
    block(&mut bus);
    let mut gain = 1.0;
    for gains in [&[0.5][..], &[], &[0.25, 0.75, 0.125], &[]] {
        // The UI thread's side: the controller is told and the ring is fed.
        for &value in gains {
            instances.set_param(instance, GAIN, value).unwrap();
            gain = value as f32;
        }
        assert_no_alloc(|| block(&mut bus));
        for channel in 0..2 {
            let out = &bus.channel(channel)[..FRAMES];
            assert!(out.iter().all(|s| *s == gain), "{gain}: {out:?}");
        }
    }
    drop(chain);
    assert!(!rack.is_lent(instance), "home once the chain is gone");
}
