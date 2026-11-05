//! The real host on the null backend, with a real VST3 plugin: a signal through a channel's
//! chain with MON on and MST off, meters that move, a save and a reload that give back the
//! same setup and the same sound, a plugin moved while it plays that is saved and sounds the
//! same after a relaunch, a copy that sounds like its original, and an instance's lifecycle
//! across a removal, its undo and the undo toast running out. On every platform.

mod common;

use std::time::{Duration, Instant};

use vixmix_host::{AudioBackend, Host, HostEvent, MeterFrame, RealHost, UNDO_TOAST};
use vixmix_model::{Command, InstanceId, MixId, Setup, SlotRef, Strip, apply, persist};

/// What the null backend feeds the first channel: a constant, so every peak is exact.
const LEVEL: f32 = 0.8;
/// The gain the test plugin is stored at.
const GAIN: f64 = 0.25;
const PROCESSED: f32 = LEVEL * GAIN as f32;
/// Where a test moves the plugin's gain (its parameter 0, normalized, is the gain) while it
/// plays, and where it moves it again just before quitting.
const MOVED: f64 = 0.5;
const AGAIN: f64 = 0.75;

fn close(found: f32, expected: f32) -> bool {
    (found - expected).abs() < 1e-5
}

/// Feeds [`LEVEL`] into the console's first channel, runs a few quanta and returns the
/// meters' peaks over them.
fn play(host: &mut RealHost) -> MeterFrame {
    let (left, right) = host.null_input(0).expect("on the null backend");
    left.fill(LEVEL);
    right.fill(LEVEL);
    let _ = host.poll();
    let _ = host.meters();
    for _ in 0..3 {
        let _ = host.poll();
    }
    host.meters()
}

/// A setup whose first channel runs the test plugin at [`GAIN`], MON on and MST off, written
/// to `config` after a first boot found the plugin; the insert's instance.
fn setup_with_the_plugin(name: &str) -> (std::path::PathBuf, std::path::PathBuf, InstanceId) {
    let plugins = common::plugin_dir(name);
    let config = common::config_dir(name);
    let host = common::boot(&config, &plugins, AudioBackend::Null);
    let plugin = common::test_plugin(&host);
    drop(host);
    let mut setup = Setup::default();
    let ch = setup.channels[0].id;
    let instance = apply(
        &mut setup,
        Command::AddInsert {
            strip: Strip::Channel(ch),
            plugin,
            at: 0,
        },
    )
    .unwrap()
    .created[0];
    for cmd in [
        Command::SetInsertState {
            instance,
            state: Some(common::gain_state(GAIN)),
        },
        Command::SetChainRouting {
            ch,
            mix: MixId::Master,
            on: false,
        },
    ] {
        let _ = apply(&mut setup, cmd).unwrap();
    }
    common::write_setup(&config, &setup);
    (config, plugins, instance)
}

#[test]
fn the_test_plugin_plays_through_a_channel_chain_and_comes_back_after_a_reload() {
    let (config, plugins, instance) = setup_with_the_plugin("chain");
    let mut host = common::boot(&config, &plugins, AudioBackend::Null);
    assert!(host.is_live(instance), "the insert's instance was made");
    assert!(host.problems().is_empty(), "{:?}", host.problems());
    let written = host.setup().clone();

    let frame = play(&mut host);
    let first = frame.channels[0];
    assert!(
        close(first.monitor.left, PROCESSED) && close(first.monitor.right, PROCESSED),
        "MON takes the chain's output: {first:?}"
    );
    assert!(
        close(first.master.left, LEVEL),
        "MST off takes the dry input: {first:?}"
    );
    assert!(close(frame.monitor.left, PROCESSED) && close(frame.master.left, LEVEL));

    // Saved (the plugin's state captured from the live instance) and booted again.
    host.save_now();
    let saved = host.setup().clone();
    assert_eq!(saved, written, "the captured state is the state it loaded");
    drop(host);
    let mut host = common::boot(&config, &plugins, AudioBackend::Null);
    assert_eq!(host.setup(), &saved);
    let again = play(&mut host);
    assert_eq!(
        again, frame,
        "the same setup sounds the same after a reload"
    );
}

/// The state the setup on disk holds for `instance`.
fn saved_state(config: &std::path::Path, instance: InstanceId) -> Option<Vec<u8>> {
    let (setup, problem) = persist::load(config).into_parts();
    assert!(problem.is_none(), "{problem:?}");
    let insert = setup.inserts().find(|i| i.instance == instance)?;
    insert.state.clone()
}

#[test]
fn a_plugin_moved_while_it_plays_is_saved_with_its_new_state_and_sounds_so_after_a_relaunch() {
    let (config, plugins, instance) = setup_with_the_plugin("moved");
    let mut host = common::boot(&config, &plugins, AudioBackend::Null);
    assert!(close(play(&mut host).channels[0].monitor.left, PROCESSED));

    // Moved as an edit in its editor would move it: heard from the next block, and on disk
    // once the setup has been quiet for the debounce, the state taken from the live plugin.
    host.set_param(instance, 0, MOVED).unwrap();
    let moved = LEVEL * MOVED as f32;
    let heard = play(&mut host).channels[0].monitor.left;
    assert!(close(heard, moved), "heard {heard}, wanted {moved}");
    assert_eq!(
        saved_state(&config, instance),
        Some(common::gain_state(GAIN))
    );
    let wait = host.next_poll().expect("the move waits for the debounce");
    std::thread::sleep(wait + Duration::from_millis(20));
    let _ = host.poll();
    assert_eq!(
        saved_state(&config, instance),
        Some(common::gain_state(MOVED)),
        "the autosave took the plugin's new state"
    );

    // Moved again and quit at once: the quit's save takes it.
    host.set_param(instance, 0, AGAIN).unwrap();
    let again = LEVEL * AGAIN as f32;
    assert!(close(play(&mut host).channels[0].monitor.left, again));
    drop(host);
    assert_eq!(
        saved_state(&config, instance),
        Some(common::gain_state(AGAIN))
    );

    // Relaunched: the same state, the same sound.
    let mut host = common::boot(&config, &plugins, AudioBackend::Null);
    let insert = host.setup().inserts().next().unwrap();
    assert_eq!(insert.state, Some(common::gain_state(AGAIN)));
    let heard = play(&mut host).channels[0].monitor.left;
    assert!(
        close(heard, again),
        "after the relaunch {heard}, wanted {again}"
    );
}

#[test]
fn a_removed_insert_keeps_its_instance_until_its_undo_toast_dies() {
    let (config, plugins, instance) = setup_with_the_plugin("undo");
    let mut host = common::boot(&config, &plugins, AudioBackend::Null);
    let _ = host.poll();

    let remove = Command::RemoveInsert { instance };
    let applied = host.apply(remove.clone()).unwrap();
    assert_eq!(applied.removed, vec![instance]);
    let undo = host
        .poll()
        .into_iter()
        .find_map(|e| match e {
            HostEvent::Removed { message, undo } => {
                assert_eq!(message, "Removed Vix Mix Test Gain from Mic");
                Some(undo)
            }
            _ => None,
        })
        .expect("the removal's undo");
    assert!(host.is_live(instance), "kept while the toast shows");
    assert!(
        close(play(&mut host).channels[0].monitor.left, LEVEL),
        "its slot is gone from the sound"
    );

    // Undone within the toast: the same instance plays again, with its state.
    let applied = host.apply(undo.clone()).unwrap();
    assert_eq!(applied.created, vec![instance]);
    assert!(host.is_live(instance));
    assert!(close(play(&mut host).channels[0].monitor.left, PROCESSED));

    // Removed again and left: once the toast has run out, the instance is retired. The
    // window sleeps as long as the host lets it, as the app does.
    let removed = Instant::now();
    let _ = host.apply(remove).unwrap();
    let deadline = removed + UNDO_TOAST + Duration::from_secs(3);
    while host.is_live(instance) && Instant::now() < deadline {
        let wait = host
            .next_poll()
            .expect("the kept instance waits for its toast");
        std::thread::sleep(wait);
        let _ = host.poll();
    }
    assert!(!host.is_live(instance), "retired once its toast died");
    assert!(
        removed.elapsed() >= UNDO_TOAST,
        "kept as long as the toast shows"
    );
    assert_eq!(host.next_poll(), None, "and nothing waits any more");

    // An undo after that makes it again from the state it was removed with.
    let _ = host.apply(undo).unwrap();
    assert!(host.is_live(instance));
    assert!(close(play(&mut host).channels[0].monitor.left, PROCESSED));
}

#[test]
fn a_copied_insert_sounds_like_its_original() {
    let (config, plugins, instance) = setup_with_the_plugin("copy");
    let mut host = common::boot(&config, &plugins, AudioBackend::Null);
    let ch = host.setup().channels[0].id;
    let applied = host
        .apply(Command::MoveInsert {
            from: SlotRef {
                strip: Strip::Channel(ch),
                index: 0,
            },
            to: SlotRef {
                strip: Strip::Mix(MixId::Master),
                index: 0,
            },
            copy: true,
        })
        .unwrap();
    let copy = applied.created[0];
    assert_ne!(copy, instance);
    assert!(host.is_live(copy) && host.is_live(instance));
    let frame = play(&mut host);
    // The channel's Master send is dry; the Master mix runs the copy at the same gain.
    assert!(close(frame.channels[0].master.left, LEVEL));
    assert!(close(frame.master.left, PROCESSED), "{frame:?}");
}
