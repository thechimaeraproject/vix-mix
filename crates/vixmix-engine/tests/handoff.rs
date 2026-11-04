//! The handoff between the host thread and the tick, and the life of a lent instance across
//! graphs: what is dropped where and when, what is lent once and kept, what is set up again.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;

use common::{DataThread, Log, Probe, TestRack, engine, feed, gate, insert, sine, snap};
use vixmix_engine::{Lender, NullBackend};
use vixmix_model::InstanceId;

#[test]
fn the_previous_graph_drops_on_the_host_thread_only_after_the_tick_acknowledges_the_next() {
    let rack = TestRack::new();
    let (held_x, x) = gate();
    let (held_y, y) = gate();
    let _ = rack.add_probe(1, Probe::new(1.0).0.gated(held_x));
    let _ = rack.add_probe(2, Probe::new(1.0).0.gated(held_y));
    let engine = engine();
    let mut first = snap(2);
    first.channels[0].chain = vec![insert(1)];
    engine.prepare(&first, &*rack);
    let host = thread::current().id();

    let data = DataThread::spawn(Arc::clone(&engine), 2);
    let signal = sine(64, 0.5);
    data.pump(&signal);
    x.wait_entered();
    // The tick is inside generation 1, holding instance 1.
    assert_eq!(engine.generations(), (1, 1));

    // The host removes instance 1 and adds instance 2 elsewhere.
    let mut second = snap(2);
    second.channels[1].chain = vec![insert(2)];
    engine.prepare(&second, &*rack);
    assert_eq!(engine.reap(), 0, "generation 1 is in the tick's hands");
    assert_eq!(engine.reap(), 0);
    assert!(
        rack.homecomings().is_empty(),
        "instance 1 is still being processed"
    );
    assert!(!rack.is_home(1));
    assert_eq!(engine.generations(), (2, 1));

    // The tick finishes generation 1 and enters generation 2, where it stops again.
    x.release();
    let _ = data.result();
    data.pump(&signal);
    y.wait_entered();
    assert_eq!(engine.generations(), (2, 2), "the acknowledgement");

    // Now generation 1 goes, while the tick is still mid-cycle: on this thread.
    assert_eq!(engine.reap(), 1);
    assert_eq!(rack.homecomings(), vec![(InstanceId(1), host)]);
    assert!(rack.is_home(1));

    y.release();
    let pumped = data.result();
    assert_ne!(pumped.thread, host);
    data.stop();
    // Nothing ever came home on the data thread.
    assert!(rack.homecomings().iter().all(|(_, thread)| *thread == host));
}

#[test]
fn with_nothing_ticking_a_replaced_graph_and_its_instances_go_home_at_once() {
    let rack = TestRack::new();
    let _ = rack.add_probe(1, Probe::new(1.0).0);
    let engine = engine();
    let mut first = snap(1);
    first.channels[0].chain = vec![insert(1)];
    engine.prepare(&first, &*rack);
    assert!(!rack.is_home(1));
    // No backend is running: the graph that held instance 1 is dropped inside `prepare`,
    // so replaced graphs never pile up while nothing ticks.
    engine.prepare(&snap(1), &*rack);
    assert_eq!(
        rack.homecomings(),
        vec![(InstanceId(1), thread::current().id())]
    );
    assert_eq!(engine.reap(), 0, "nothing left to reap");
}

#[test]
fn an_instance_survives_a_new_insert_and_a_move_to_another_strip_without_a_new_setup() {
    let rack = TestRack::new();
    let first_log = rack.add_probe(1, Probe::new(0.5).0);
    let engine = engine();
    let mut graph = snap(2);
    graph.channels[0].chain = vec![insert(1)];
    engine.prepare(&graph, &*rack);
    let mut null = NullBackend::new(Arc::clone(&engine), 64);
    let signal = sine(64, 0.5);
    feed(&mut null, 0, &signal);
    null.pump(64);

    // A second instance arrives (the rack's generation moves) and the first moves to
    // channel 2, behind nothing; channel 1 gets the new one.
    let second_log = rack.add_probe(2, Probe::new(0.25).0);
    graph.channels[0].chain = vec![insert(2)];
    graph.channels[1].chain = vec![insert(1)];
    engine.prepare(&graph, &*rack);
    assert!(engine.problems().is_empty(), "{:?}", engine.problems());
    null.silence();
    feed(&mut null, 1, &signal);
    null.pump(64);
    let halved: Vec<f32> = signal.iter().map(|s| s * 0.5).collect();
    assert_eq!(
        null.monitor().0,
        &halved[..],
        "instance 1, now on channel 2"
    );

    // The same instance, carrying on: one setup, one activation, never home.
    assert_eq!(Log::get(&first_log.setups), 1);
    assert_eq!(Log::get(&first_log.activations), 1);
    assert_eq!(first_log.blocks.load(Ordering::SeqCst), 2);
    assert!(rack.homecomings().is_empty());
    assert_eq!(Log::get(&second_log.setups), 1);
}

#[test]
fn a_retired_instance_goes_home_and_its_insert_passes_through_reported() {
    let rack = TestRack::new();
    let _ = rack.add_probe(1, Probe::new(0.5).0);
    let engine = engine();
    let mut graph = snap(1);
    graph.channels[0].chain = vec![insert(1)];
    engine.prepare(&graph, &*rack);

    rack.retire(1);
    engine.prepare(&graph, &*rack);
    assert!(rack.is_home(1), "let go, and home since nothing ticks");
    let problems = engine.problems();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].instance, InstanceId(1));

    let mut null = NullBackend::new(Arc::clone(&engine), 64);
    let signal = sine(64, 0.5);
    feed(&mut null, 0, &signal);
    null.pump(64);
    assert_eq!(null.monitor().0, &signal[..]);
}

#[test]
fn a_new_instance_under_a_held_id_takes_over_and_the_old_one_goes_home() {
    let rack = TestRack::new();
    let old = rack.add_probe(1, Probe::new(0.5).0);
    let engine = engine();
    let mut graph = snap(1);
    graph.channels[0].chain = vec![insert(1)];
    engine.prepare(&graph, &*rack);

    // The host made the instance again (the plugins crate retires the old entry).
    let new = rack.add_probe(1, Probe::new(0.25).0);
    engine.prepare(&graph, &*rack);
    assert_eq!(Log::get(&new.setups), 1);
    assert!(
        !old.active.load(Ordering::SeqCst),
        "the old one went home inactive"
    );
    assert_eq!(rack.homecomings().len(), 1);

    let mut null = NullBackend::new(Arc::clone(&engine), 64);
    let signal = sine(64, 0.5);
    feed(&mut null, 0, &signal);
    null.pump(64);
    let quartered: Vec<f32> = signal.iter().map(|s| s * 0.25).collect();
    assert_eq!(null.monitor().0, &quartered[..]);
}

#[test]
fn a_new_rate_sets_every_instance_up_again_once_no_running_graph_holds_it() {
    let rack = TestRack::new();
    let log = rack.add_probe(1, Probe::new(0.5).0);
    let (held, control) = gate();
    let gated = rack.add_probe(2, Probe::new(1.0).0.gated(held));
    let engine = engine();
    let mut graph = snap(2);
    graph.channels[0].chain = vec![insert(1)];
    graph.channels[1].chain = vec![insert(2)];
    engine.prepare(&graph, &*rack);
    assert_eq!(Log::get(&log.rate), 48_000);

    let data = DataThread::spawn(Arc::clone(&engine), 2);
    let signal = sine(64, 0.5);
    data.pump(&signal);
    control.wait_entered();

    // The backend reports a new rate while a tick is running both instances: neither may be
    // set up again yet.
    assert!(engine.set_rate(44_100));
    assert_eq!(Log::get(&log.setups), 1);
    assert_eq!(engine.reap(), 0);
    assert_eq!(Log::get(&log.setups), 1);
    control.release();
    let _ = data.result();

    // Until the host reaps, the graph at the new rate plays both inserts as pass-throughs.
    data.pump(&signal);
    let dry: Vec<f32> = signal.iter().map(|s| s + s).collect();
    assert_eq!(data.result().monitor, dry);
    assert!(
        engine.problems().is_empty(),
        "setting up again is not a problem"
    );

    // The reap lets the old graphs go, sets both up at 44.1 kHz and swaps the graph in.
    assert!(engine.reap() >= 1);
    assert_eq!(Log::get(&log.setups), 2);
    assert_eq!(Log::get(&log.rate), 44_100);
    assert_eq!(Log::get(&gated.rate), 44_100);
    assert!(log.active.load(Ordering::SeqCst));
    data.pump(&signal);
    let processed: Vec<f32> = signal.iter().map(|s| s * 0.5 + s).collect();
    assert_eq!(data.result().monitor, processed);
    assert!(rack.homecomings().is_empty(), "the same instances, kept");
    data.stop();
}

#[test]
fn an_instance_that_asks_for_new_buses_is_set_up_again() {
    let rack = TestRack::new();
    let log = rack.add_probe(1, Probe::new(0.5).0);
    let engine = engine();
    let mut graph = snap(1);
    graph.channels[0].chain = vec![insert(1)];
    engine.prepare(&graph, &*rack);
    assert_eq!(Log::get(&log.setups), 1);

    assert!(engine.set_up_again(InstanceId(1)));
    // Nothing ticks, so it was set up again at once and is back in the graph.
    assert_eq!(Log::get(&log.setups), 2);
    assert_eq!(Log::get(&log.activations), 2);
    assert!(!engine.set_up_again(InstanceId(9)), "not held");

    // The plugins crate also moves its generation: the instance is still the engine's.
    rack.touch();
    engine.prepare(&graph, &*rack);
    assert_eq!(Log::get(&log.setups), 2);
    let mut null = NullBackend::new(Arc::clone(&engine), 64);
    let signal = sine(64, 0.5);
    feed(&mut null, 0, &signal);
    null.pump(64);
    let halved: Vec<f32> = signal.iter().map(|s| s * 0.5).collect();
    assert_eq!(null.monitor().0, &halved[..]);
}

#[test]
fn an_insert_put_back_before_its_instance_went_home_is_taken_back_not_lent_again() {
    let rack = TestRack::new();
    let (held, control) = gate();
    let log = rack.add_probe(1, Probe::new(0.5).0.gated(held));
    let engine = engine();
    let mut with = snap(1);
    with.channels[0].chain = vec![insert(1)];
    engine.prepare(&with, &*rack);

    let data = DataThread::spawn(Arc::clone(&engine), 1);
    let signal = sine(64, 0.5);
    data.pump(&signal);
    control.wait_entered();

    // Removed and put back (an undo) while the tick still holds it: lending it again would
    // find it busy, so the engine takes back the one it has.
    engine.prepare(&snap(1), &*rack);
    engine.prepare(&with, &*rack);
    assert!(engine.problems().is_empty(), "{:?}", engine.problems());
    let lends = rack.lends();
    assert_eq!(lends, 1, "lent once, ever");

    control.release();
    let _ = data.result();
    data.pump(&signal);
    let halved: Vec<f32> = signal.iter().map(|s| s * 0.5).collect();
    assert_eq!(data.result().monitor, halved);
    assert_eq!(Log::get(&log.setups), 1);
    assert!(rack.homecomings().is_empty());
    data.stop();
}

#[test]
fn an_instance_another_borrower_holds_is_reported_and_lent_once_the_rack_moves() {
    let rack = TestRack::new();
    let _ = rack.add_probe(1, Probe::new(0.5).0);
    let elsewhere = rack.lend(InstanceId(1), common::PLUGIN).unwrap().unwrap();
    let engine = engine();
    let mut graph = snap(1);
    graph.channels[0].chain = vec![insert(1)];
    engine.prepare(&graph, &*rack);
    let problems = engine.problems();
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0].message.contains("still in another chain"),
        "{}",
        problems[0].message
    );

    drop(elsewhere);
    rack.touch();
    engine.prepare(&graph, &*rack);
    assert!(engine.problems().is_empty());
}

#[test]
fn one_instance_in_two_slots_plays_in_the_first_and_the_second_is_reported() {
    let rack = TestRack::new();
    let log = rack.add_probe(1, Probe::new(0.5).0);
    let engine = engine();
    let mut graph = snap(2);
    graph.channels[0].chain = vec![insert(1)];
    graph.channels[1].chain = vec![insert(1)];
    engine.prepare(&graph, &*rack);
    let problems = engine.problems();
    assert_eq!(problems.len(), 1);
    assert_eq!(
        problems[0].strip,
        vixmix_model::Strip::Channel(graph.channels[1].id)
    );
    assert!(problems[0].message.contains("already in another slot"));

    let mut null = NullBackend::new(Arc::clone(&engine), 64);
    let signal = sine(64, 0.5);
    feed(&mut null, 0, &signal);
    feed(&mut null, 1, &signal);
    null.pump(64);
    assert_eq!(
        log.blocks.load(Ordering::SeqCst),
        1,
        "processed once a tick"
    );
    let expected: Vec<f32> = signal.iter().map(|s| s * 0.5 + s).collect();
    assert_eq!(null.monitor().0, &expected[..]);
}

#[test]
fn dropping_the_engine_sends_every_instance_home_on_the_dropping_thread() {
    let rack = TestRack::new();
    let _ = rack.add_probe(1, Probe::new(0.5).0);
    let _ = rack.add_probe(2, Probe::new(0.5).0);
    let engine = engine();
    let mut graph = snap(1);
    graph.channels[0].chain = vec![insert(1)];
    graph.master.chain = vec![insert(2)];
    graph.master.chain[0].enabled = false;
    engine.prepare(&graph, &*rack);
    let mut null = NullBackend::new(Arc::clone(&engine), 64);
    null.pump(64);
    drop(null);
    drop(engine);
    let host = thread::current().id();
    let mut home = rack.homecomings();
    home.sort_by_key(|(instance, _)| *instance);
    assert_eq!(home, vec![(InstanceId(1), host), (InstanceId(2), host)]);
    let lent = rack.lend(InstanceId(1), common::PLUGIN).unwrap().unwrap();
    assert_eq!(lent.descriptor().id, common::PLUGIN);
}
