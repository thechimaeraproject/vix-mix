//! The tick must never touch the allocator once its thread has run one: eight channels at
//! varying cycle lengths within one prepared graph, through the null backend exactly as the
//! PipeWire data thread will call it.
//!
//! `assert_no_alloc`'s allocator aborts this test binary on the first allocation inside the
//! closure (`memory allocation of N bytes failed`), so a regression cannot pass quietly. The
//! first tick runs outside it: `arc_swap` registers the thread's debt slot on first use.

use std::sync::Arc;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use vixmix_engine::{Engine, EngineConfig, MAX_QUANTUM, NoPlugins, NullBackend};
use vixmix_model::{ChannelKind, Command, MixId, Setup, apply, snapshot};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn eight_channels_tick_without_allocating_from_the_second_block() {
    let mut setup = Setup::default();
    while setup.channels.len() < 8 {
        let _ = apply(
            &mut setup,
            Command::AddChannel {
                kind: ChannelKind::Apps,
            },
        )
        .unwrap();
    }
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
    let _ = apply(&mut setup, Command::SetPreview(true)).unwrap();

    let engine = Arc::new(Engine::new(EngineConfig::new(48_000, MAX_QUANTUM, 2)));
    engine.prepare(&snapshot(&setup), &NoPlugins);
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
    for frames in [64, 256, 1024, 256, 64] {
        assert_no_alloc(|| {
            null.pump(frames);
            let _ = engine.meters().channel(0).monitor.take();
            let _ = engine.meters().master().take();
        });
    }
    assert!(null.master().0.iter().any(|s| *s != 0.0));
}
