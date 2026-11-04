//! The graph as a listener hears it: chains, the MON/MST split, sends, mixes, mutes, the ear
//! button, missing and bypassed plugins, and the counters, through the null backend exactly
//! as PipeWire's data thread calls the tick.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use chimaeraproject_audio_processor::db_to_linear;
use common::{
    DataThread, Log, PLUGIN, Probe, TestRack, engine, feed, gate, insert, setup, sine, snap,
};
use vixmix_engine::{MAX_QUANTUM, NoPlugins, NullBackend};
use vixmix_model::{Command, InstanceId, MixId, Strip, apply, snapshot, taper};

#[test]
fn a_sine_through_a_chain_with_mon_on_and_mst_off_lands_in_monitor_at_half_and_master_dry() {
    let rack = TestRack::new();
    let half = db_to_linear(-6.02);
    let log = rack.add_probe(1, Probe::new(half).0);
    let mut snap = snap(1);
    snap.channels[0].chain = vec![insert(1)];
    snap.channels[0].chain_to_monitor = true;
    snap.channels[0].chain_to_master = false;
    let engine = engine();
    engine.prepare(&snap, &*rack);
    assert!(engine.problems().is_empty(), "{:?}", engine.problems());

    let mut null = NullBackend::new(Arc::clone(&engine), 256);
    let signal = sine(256, 0.8);
    feed(&mut null, 0, &signal);
    null.pump(256);

    // Monitor takes the chain's output: the sine at −6.02 dB, bit for bit what the plugin
    // made (a send and a mix at unity add without multiplying).
    let processed: Vec<f32> = signal.iter().map(|s| s * half).collect();
    assert_eq!(null.monitor().0, &processed[..]);
    assert_eq!(null.monitor().1, &processed[..]);
    let peak = null.monitor().0.iter().fold(0.0f32, |p, s| p.max(s.abs()));
    assert!((peak - 0.4).abs() < 1e-3, "half of 0.8, got {peak}");
    // Master takes the dry input: the processed sine does not land there.
    assert_eq!(null.master().0, &signal[..]);
    assert_eq!(null.master().1, &signal[..]);
    assert_eq!(log.blocks.load(Ordering::SeqCst), 1);
}

#[test]
fn a_send_at_minus_6_02_db_halves_the_sine_and_a_muted_send_carries_nothing() {
    let mut setup = setup(1);
    let ch = setup.channels[0].id;
    let _ = apply(
        &mut setup,
        Command::SetSend {
            ch,
            mix: MixId::Monitor,
            position: taper::db_to_position(-6.02),
        },
    )
    .unwrap();
    let _ = apply(
        &mut setup,
        Command::SetMute {
            ch,
            mix: MixId::Master,
            mute: true,
        },
    )
    .unwrap();
    let snap = snapshot(&setup);
    let gain = snap.channels[0].monitor.gain;
    assert!((gain - 0.5).abs() < 1e-3, "−6.02 dB is half: {gain}");
    let engine = engine();
    engine.prepare(&snap, &NoPlugins);

    let mut null = NullBackend::new(Arc::clone(&engine), 128);
    let signal = sine(128, 0.8);
    feed(&mut null, 0, &signal);
    null.pump(128);
    let expected: Vec<f32> = signal.iter().map(|s| s * gain).collect();
    assert_eq!(null.monitor().0, &expected[..]);
    assert!(null.master().0.iter().all(|s| *s == 0.0));
    assert!(null.master().1.iter().all(|s| *s == 0.0));
}

#[test]
fn mute_beats_everything() {
    let rack = TestRack::new();
    // Each probe adds a tone, so it makes sound from silence.
    let _ = rack.add_probe(1, Probe::new(1.0).0.tone(0.25));
    let _ = rack.add_probe(2, Probe::new(1.0).0.tone(0.5));
    let _ = rack.add_probe(3, Probe::new(1.0).0.tone(0.125));
    let mut setup = setup(1);
    let ch = setup.channels[0].id;
    // The Master send at the top of its fader, and muted.
    let _ = apply(&mut setup, Command::SetLink { ch, link: false }).unwrap();
    for command in [
        Command::SetSend {
            ch,
            mix: MixId::Master,
            position: 1.0,
        },
        Command::SetMute {
            ch,
            mix: MixId::Master,
            mute: true,
        },
    ] {
        let _ = apply(&mut setup, command).unwrap();
    }
    let mut snap = snapshot(&setup);
    snap.channels[0].chain = vec![insert(1)];
    snap.monitor.chain = vec![insert(2)];
    snap.master.chain = vec![insert(3)];
    let engine = engine();
    engine.prepare(&snap, &*rack);
    let mut null = NullBackend::new(Arc::clone(&engine), 64);
    null.pump(64);

    // A muted send carries nothing, whatever its chain makes and however high its fader:
    // Master hears only its own chain's tone.
    assert!(null.master().0.iter().all(|s| *s == 0.125));
    assert_eq!(engine.meters().channel(0).master.take(), (0.0, 0.0));
    // The unmuted Monitor send carries the channel's tone, then Monitor's own.
    assert!(null.monitor().0.iter().all(|s| *s == 0.75));

    // A muted mix is silent, whatever its chain makes.
    snap.monitor.level.mute = true;
    engine.prepare(&snap, &*rack);
    null.pump(64);
    assert!(null.monitor().0.iter().all(|s| *s == 0.0));
    assert!(null.monitor().1.iter().all(|s| *s == 0.0));

    // The ear button plays a muted Master as what it is: silence.
    snap.monitor.level.mute = false;
    snap.master.level.mute = true;
    snap.preview_master = true;
    engine.prepare(&snap, &*rack);
    null.pump(64);
    assert!(null.master().0.iter().all(|s| *s == 0.0));
    assert!(null.monitor().0.iter().all(|s| *s == 0.0));
    let _ = engine.meters().monitor().take();
    null.pump(64);
    assert_eq!(engine.meters().monitor().take(), (0.0, 0.0));
}

#[test]
fn a_linked_send_change_arrives_once_in_one_tick_and_sets_no_plugin_up_again() {
    let rack = TestRack::new();
    let log = rack.add_probe(1, Probe::new(1.0).0);
    let mut setup = setup(1);
    let ch = setup.channels[0].id;
    let with_chain = |setup: &vixmix_model::Setup| {
        let mut snap = snapshot(setup);
        snap.channels[0].chain = vec![insert(1)];
        snap
    };
    let engine = engine();
    engine.prepare(&with_chain(&setup), &*rack);
    let mut null = NullBackend::new(Arc::clone(&engine), 128);
    let signal = sine(128, 0.5);
    feed(&mut null, 0, &signal);
    null.pump(128);
    assert_eq!(null.monitor().0, &signal[..]);
    assert_eq!(null.master().0, &signal[..]);

    // One fader move on a linked channel: both sends move, in one command.
    let applied = apply(
        &mut setup,
        Command::SetSend {
            ch,
            mix: MixId::Master,
            position: taper::db_to_position(-6.0),
        },
    )
    .unwrap();
    assert!(applied.engine);
    let snap = with_chain(&setup);
    let (monitor, master) = (snap.channels[0].monitor.gain, snap.channels[0].master.gain);
    assert!(monitor < 1.0 && master < 1.0, "the link moved both");
    let (before, _) = engine.generations();
    engine.prepare(&snap, &*rack);
    assert_eq!(engine.generations().0, before + 1, "one prepare, one graph");

    // The very next tick has both sends at their new gains: never one without the other.
    null.pump(128);
    let at = |gain: f32| signal.iter().map(|s| s * gain).collect::<Vec<f32>>();
    assert_eq!(null.monitor().0, &at(monitor)[..]);
    assert_eq!(null.master().0, &at(master)[..]);
    // And it arrived once: the next tick runs the same graph to the same result.
    let (monitor_out, master_out) = (null.monitor().0.to_vec(), null.master().0.to_vec());
    null.pump(128);
    assert_eq!(engine.generations().0, before + 1);
    assert_eq!(null.monitor().0, &monitor_out[..]);
    assert_eq!(null.master().0, &master_out[..]);

    // The plugin heard a graph swap, not a new life: set up and activated once, told nothing.
    assert_eq!(Log::get(&log.setups), 1);
    assert_eq!(Log::get(&log.activations), 1);
    assert_eq!(log.changes.load(Ordering::SeqCst), 0);
    assert_eq!(log.blocks.load(Ordering::SeqCst), 3);
}

#[test]
fn the_first_block_tells_a_lent_plugin_nothing_so_its_loaded_state_stands() {
    let rack = TestRack::new();
    let log = rack.add_probe(1, Probe::new(1.0).0);
    let mut snap = snap(1);
    snap.channels[0].chain = vec![insert(1)];
    let engine = engine();
    engine.prepare(&snap, &*rack);
    let mut null = NullBackend::new(Arc::clone(&engine), 64);
    null.pump(64);
    null.pump(64);
    // The chain's first block is its "every parameter" block, and an insert's parameters
    // are empty: the plugin hears an initial block with no changes, never a reset.
    assert_eq!(Log::get(&log.initial_blocks), 1);
    assert_eq!(log.changes.load(Ordering::SeqCst), 0);
    assert_eq!(log.blocks.load(Ordering::SeqCst), 2);
}

#[test]
fn a_missing_plugin_passes_audio_through_and_is_reported_until_it_turns_up() {
    let rack = TestRack::new();
    let mut snap = snap(1);
    snap.channels[0].chain = vec![insert(7)];
    let engine = engine();
    engine.prepare(&snap, &*rack);

    let problems = engine.problems();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].strip, Strip::Channel(snap.channels[0].id));
    assert_eq!(problems[0].instance, InstanceId(7));
    assert_eq!(problems[0].plugin_id, PLUGIN);
    assert!(
        problems[0].message.contains("not loaded"),
        "{}",
        problems[0].message
    );
    assert!(problems[0].message.contains("remove the insert"));

    let mut null = NullBackend::new(Arc::clone(&engine), 64);
    let signal = sine(64, 0.5);
    feed(&mut null, 0, &signal);
    null.pump(64);
    assert_eq!(null.monitor().0, &signal[..], "a pass-through is bit-exact");

    // A fader move prepares again: the failure is still reported once, and the rack is not
    // asked again while nothing it holds changed.
    let lends = rack.lends();
    engine.prepare(&snap, &*rack);
    assert_eq!(engine.problems().len(), 1);
    assert_eq!(rack.lends(), lends);

    // The instance turns up (the rack's generation moves): the next prepare lends it.
    let _ = rack.add_probe(7, Probe::new(0.5).0);
    engine.prepare(&snap, &*rack);
    assert!(engine.problems().is_empty());
    null.pump(64);
    let halved: Vec<f32> = signal.iter().map(|s| s * 0.5).collect();
    assert_eq!(null.monitor().0, &halved[..]);
}

#[test]
fn a_bypassed_insert_is_left_out_but_stays_lent_so_turning_it_on_is_immediate() {
    let rack = TestRack::new();
    let log = rack.add_probe(1, Probe::new(0.5).0);
    let mut snap = snap(1);
    snap.channels[0].chain = vec![insert(1)];
    snap.channels[0].chain[0].enabled = false;
    let engine = engine();
    engine.prepare(&snap, &*rack);
    assert!(engine.problems().is_empty(), "bypassed is not a problem");

    let mut null = NullBackend::new(Arc::clone(&engine), 64);
    let signal = sine(64, 0.5);
    feed(&mut null, 0, &signal);
    null.pump(64);
    assert_eq!(
        null.monitor().0,
        &signal[..],
        "bypassed: the dry signal, bit for bit"
    );
    assert_eq!(
        log.blocks.load(Ordering::SeqCst),
        0,
        "a bypassed plugin costs nothing"
    );
    assert!(!rack.is_home(1), "it stays lent, set up and active");
    assert!(log.active.load(Ordering::SeqCst));

    snap.channels[0].chain[0].enabled = true;
    engine.prepare(&snap, &*rack);
    null.pump(64);
    let halved: Vec<f32> = signal.iter().map(|s| s * 0.5).collect();
    assert_eq!(null.monitor().0, &halved[..]);
    assert_eq!(Log::get(&log.setups), 1, "turned on without a new setup");
}

#[test]
fn a_mix_runs_its_chain_on_the_summed_sends_before_its_level() {
    let rack = TestRack::new();
    let _ = rack.add_probe(1, Probe::new(1.0).0.tone(0.25));
    let mut setup = setup(2);
    let _ = apply(
        &mut setup,
        Command::SetMixLevel {
            mix: MixId::Master,
            position: taper::db_to_position(-6.0),
        },
    )
    .unwrap();
    let mut snap = snapshot(&setup);
    snap.master.chain = vec![insert(1)];
    let level = snap.master.level.gain;
    let engine = engine();
    engine.prepare(&snap, &*rack);
    let mut null = NullBackend::new(Arc::clone(&engine), 64);
    let signal = sine(64, 0.25);
    feed(&mut null, 0, &signal);
    feed(&mut null, 1, &signal);
    null.pump(64);
    // Two channels summed, the chain's tone added, then the level: (2s + 0.25) × level.
    let expected: Vec<f32> = signal.iter().map(|s| (s + s + 0.25) * level).collect();
    assert_eq!(null.master().0, &expected[..]);
    // Monitor has no chain and is at unity: the plain sum.
    let sum: Vec<f32> = signal.iter().map(|s| s + s).collect();
    assert_eq!(null.monitor().0, &sum[..]);
}

#[test]
fn a_tick_of_the_whole_quantum_limit_runs_every_frame_through_the_chain() {
    let rack = TestRack::new();
    let log = rack.add_probe(1, Probe::new(0.5).0);
    let mut snap = snap(1);
    snap.channels[0].chain = vec![insert(1)];
    let engine = engine();
    engine.prepare(&snap, &*rack);
    // Every processor is set up for the graph's quantum limit, since the quantum is
    // PipeWire's and varies from tick to tick.
    assert_eq!(Log::get(&log.max_block) as usize, MAX_QUANTUM);
    let mut null = NullBackend::new(Arc::clone(&engine), MAX_QUANTUM);
    feed(&mut null, 0, &vec![1.0; MAX_QUANTUM]);
    for frames in [MAX_QUANTUM, 64, 1000] {
        null.pump(frames);
        assert_eq!(null.monitor().0.len(), frames);
        assert!(
            null.monitor().0.iter().all(|s| *s == 0.5),
            "{frames} frames"
        );
    }
}

#[test]
fn dsp_load_reads_above_zero_under_the_null_backend() {
    let rack = TestRack::new();
    let _ = rack.add_probe(1, Probe::new(0.5).0);
    let mut snap = snap(1);
    snap.channels[0].chain = vec![insert(1)];
    let engine = engine();
    engine.prepare(&snap, &*rack);
    let mut null = NullBackend::new(Arc::clone(&engine), 256);
    feed(&mut null, 0, &sine(256, 0.5));
    null.pump(256);
    assert!(engine.stats().dsp_load_permille.load(Ordering::Relaxed) > 0);
}

#[test]
fn a_second_tick_at_the_same_time_plays_silence_instead_of_touching_the_graph() {
    let rack = TestRack::new();
    let (held, control) = gate();
    let _ = rack.add_probe(1, Probe::new(1.0).0.gated(held));
    let mut snap = snap(2);
    snap.channels[0].chain = vec![insert(1)];
    let engine = engine();
    engine.prepare(&snap, &*rack);

    let data = DataThread::spawn(Arc::clone(&engine), 2);
    let signal = sine(64, 0.5);
    data.pump(&signal);
    control.wait_entered();

    // A second caller, while the first is mid-tick: silence, and the graph untouched.
    let mut other = NullBackend::new(Arc::clone(&engine), 64);
    feed(&mut other, 1, &signal);
    other.pump(64);
    assert!(other.monitor().0.iter().all(|s| *s == 0.0));
    assert!(other.master().0.iter().all(|s| *s == 0.0));

    control.release();
    let pumped = data.result();
    let sum: Vec<f32> = signal.iter().map(|s| s + s).collect();
    assert_eq!(pumped.monitor, sum, "the first tick ran whole");
    data.stop();
}
