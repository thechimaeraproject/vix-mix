//! The plugins crate against a real VST3 plugin: the test plugin (`examples/test_gain.rs`),
//! laid out as an installed bundle, scanned, made, lent to a chain filled as the engine fills
//! one, edited, saved, loaded into a second instance, retired and reaped.

mod common;

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chimaeraproject_vst3_host::{Arch, Os, binary_path};
use common::*;
use vixmix_engine::Lender;
use vixmix_model::{Insert, InstanceId};
use vixmix_plugins::{Catalog, InstanceEvent, Instances, PluginKind, Rack, ScanCache};

fn instances() -> (Arc<Rack>, Instances) {
    let rack = Rack::new();
    let instances = Instances::new(host(), Arc::clone(&rack));
    (rack, instances)
}

#[test]
fn the_scan_finds_the_test_plugin_classifies_it_and_skips_it_while_unchanged() {
    let dir = plugin_dir("scan");
    let host = host();
    let first = Catalog::scan(std::slice::from_ref(&dir), &host);
    assert!(first.failures.is_empty(), "{:?}", first.failures);
    assert_eq!((first.described, first.reused), (1, 0));
    let [entry] = first.catalog.plugins.as_slice() else {
        panic!("one effect: {:?}", first.catalog.plugins);
    };
    assert_eq!(entry.id, AUDIO_CLASS.processor_id());
    assert_eq!(entry.name, "Vix Mix Test Gain");
    assert_eq!(entry.vendor, "chimaeraproject");
    assert_eq!(entry.kind, PluginKind::Utility, "Fx|Tools is a utility");
    assert_eq!(entry.bundle, BUNDLE_NAME);
    assert_eq!(entry.path, dir.join(BUNDLE_NAME));

    // Through plugins.ron, as a restart sees it: unchanged, so not described again.
    let config = scratch("scan-config");
    first.save(&config).unwrap();
    let (loaded, problem) = Catalog::load(&config).into_parts();
    assert!(problem.is_none(), "{problem:?}");
    assert_eq!(loaded, first.catalog);
    let cache = ScanCache::load(&config);
    assert_eq!(cache.len(), 1);
    let again = loaded.rescan(std::slice::from_ref(&dir), &host, &cache);
    assert_eq!((again.described, again.reused), (0, 1));
    assert_eq!(again.catalog, first.catalog);

    // Forgetting failures keeps a bundle that worked: the menu's rescan still reuses it.
    let mut retry = cache.clone();
    retry.forget_failures();
    let retried = loaded.rescan(std::slice::from_ref(&dir), &host, &retry);
    assert_eq!((retried.described, retried.reused), (0, 1));
    assert_eq!(retried.catalog, first.catalog);

    // An update rewrites the binary inside the bundle: described again.
    let binary = binary_path(&dir.join(BUNDLE_NAME), Os::current(), Arch::current());
    std::fs::File::options()
        .write(true)
        .open(&binary)
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(3600))
        .unwrap();
    let updated = again
        .catalog
        .rescan(std::slice::from_ref(&dir), &host, &again.cache);
    assert_eq!((updated.described, updated.reused), (1, 0));
    assert_eq!(updated.catalog, first.catalog);
}

#[test]
fn rescan_plugins_retries_a_failure_whose_cause_the_bundle_time_does_not_show() {
    // A bundle that fails: its binary is not there.
    let dir = scratch("retry-plugins");
    let bundle = dir.join(BUNDLE_NAME);
    let binary = binary_path(&bundle, Os::current(), Arch::current());
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    let host = host();
    let first = Catalog::scan(std::slice::from_ref(&dir), &host);
    assert_eq!(first.failures.len(), 1, "{:?}", first.failures);
    assert!(
        first.failures[0].contains(BUNDLE_NAME),
        "{:?}",
        first.failures
    );
    assert!(first.catalog.plugins.is_empty());

    // Its cause is mended without the bundle's time moving, as when a runtime library it
    // needs is installed: here the binary arrives dated long before the scan.
    install_binary(&bundle);
    std::fs::File::options()
        .write(true)
        .open(&binary)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(1))
        .unwrap();
    let mut catalog = first.catalog.clone();
    catalog.set_favorite("vst3:kept", true);

    // A start's rescan keeps the failure, so a broken plugin is not loaded at every start.
    let start = catalog.rescan(std::slice::from_ref(&dir), &host, &first.cache);
    assert_eq!((start.described, start.reused), (0, 1));
    assert_eq!(start.failures, first.failures);

    // The menu's Rescan plugins forgets failures first: described again, found, and the
    // favourites carried over.
    let mut cache = start.cache.clone();
    cache.forget_failures();
    assert!(cache.is_empty());
    let retried = start
        .catalog
        .rescan(std::slice::from_ref(&dir), &host, &cache);
    assert_eq!((retried.described, retried.reused), (1, 0));
    assert!(retried.failures.is_empty(), "{:?}", retried.failures);
    assert!(retried.catalog.get(&AUDIO_CLASS.processor_id()).is_some());
    assert!(retried.catalog.favorites.contains("vst3:kept"));
}

#[test]
fn create_lend_process_save_and_a_second_instance_sounds_the_same() {
    let (catalog, entry) = catalog_of(&plugin_dir("state"));
    let plugin = plugin_ref(&entry);
    let (rack, mut instances) = instances();
    let first = InstanceId(1);
    instances.create(first, &plugin, &catalog, None).unwrap();
    assert!(instances.is_live(first) && rack.holds(first));
    let at_defaults = instances.save_state(first).unwrap();

    // A fresh instance plays at unity.
    let desired = [insert(first, &entry.id)];
    let mut chain = stereo_chain();
    fill(&mut chain, &desired, &*rack);
    let errors: Vec<&str> = chain.errors().collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(run(&mut chain, &desired), ramp(1.0));

    // The mixer moves its gain: the next block hears it, and the plugin keeps it.
    instances.set_param(first, GAIN, 0.5).unwrap();
    let heard = run(&mut chain, &desired);
    assert_eq!(heard, ramp(0.5));
    assert_eq!(run(&mut chain, &desired), heard);
    let controller = instances.controller(first).unwrap();
    assert_eq!(controller.get_param_normalized(GAIN), 0.5);

    // Home, saved, and loaded into a second instance: it sounds the same from its first block.
    chain.clear();
    let state = instances.save_state(first).unwrap();
    assert_ne!(
        state, at_defaults,
        "the state carries the gain the processor heard"
    );
    let second = InstanceId(2);
    instances
        .create(second, &plugin, &catalog, Some(&state))
        .unwrap();
    let desired = [insert(second, &entry.id)];
    let mut chain = stereo_chain();
    fill(&mut chain, &desired, &*rack);
    assert_eq!(run(&mut chain, &desired), heard);
    let controller = instances.controller(second).unwrap();
    assert_eq!(controller.get_param_normalized(GAIN), 0.5, "told too");
    chain.clear();

    // Bytes that are not a state: the plugin starts from its defaults and a notice says so.
    let third = InstanceId(3);
    instances
        .create(third, &plugin, &catalog, Some(b"not a state"))
        .unwrap();
    let notices = instances.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(
        notices[0].contains("starts from its defaults"),
        "{}",
        notices[0]
    );
}

#[test]
fn a_retired_instance_comes_home_and_drops_on_the_ui_thread() {
    let (catalog, entry) = catalog_of(&plugin_dir("retire"));
    let (rack, mut instances) = instances();
    let instance = InstanceId(7);
    instances
        .create(instance, &plugin_ref(&entry), &catalog, None)
        .unwrap();

    // The chain-preparing thread borrows it and runs a block.
    let (clear, cleared) = std::sync::mpsc::sync_channel::<()>(1);
    let lender = Arc::clone(&rack);
    let plugin_id = entry.id.clone();
    let preparing = std::thread::spawn(move || {
        let desired = [insert(instance, &plugin_id)];
        let mut chain = stereo_chain();
        fill(&mut chain, &desired, &*lender);
        let out = run(&mut chain, &desired);
        let _ = cleared.recv();
        chain.clear();
        out
    });
    while !rack.is_lent(instance) {
        std::thread::yield_now();
    }

    // The UI thread retires it: no chain may lend it, but its processor is still out.
    let generation = rack.generation();
    instances.retire(instance);
    assert!(
        rack.generation() > generation,
        "the engine must prepare again"
    );
    assert!(!instances.is_live(instance));
    assert!(matches!(rack.lend(instance, &entry.id), Ok(None)));
    assert!(instances.reap().is_empty(), "lent: nothing to drop yet");
    assert_eq!(instances.retiring(), 1);

    // The chain clears and the processor comes home; the UI thread drops it, then the
    // controller.
    clear.send(()).unwrap();
    assert_eq!(preparing.join().unwrap(), ramp(1.0));
    assert_eq!(instances.reap(), [instance]);
    assert_eq!(instances.retiring(), 0);
    assert!(rack.is_empty());
    assert!(instances.reap().is_empty());
}

#[test]
fn sync_makes_the_setups_instances_and_retires_what_it_no_longer_names() {
    let (catalog, entry) = catalog_of(&plugin_dir("sync"));
    let (rack, mut instances) = instances();
    let insert_of = |instance: u32| Insert {
        instance: InstanceId(instance),
        plugin: plugin_ref(&entry),
        enabled: true,
        state: None,
    };
    instances.sync(&[insert_of(1), insert_of(2)], &catalog);
    let live: Vec<InstanceId> = instances.live().collect();
    assert_eq!(live, [InstanceId(1), InstanceId(2)]);
    assert!(instances.take_notices().is_empty());
    let generation = rack.generation();
    instances.sync(&[insert_of(2)], &catalog);
    let live: Vec<InstanceId> = instances.live().collect();
    assert_eq!(live, [InstanceId(2)]);
    assert!(rack.generation() > generation);
    assert_eq!(
        rack.len(),
        1,
        "the retired one was home, so the sync reaped it"
    );
}

#[test]
fn the_test_plugin_has_no_editor_and_its_events_are_drained() {
    let (catalog, entry) = catalog_of(&plugin_dir("editor"));
    let (_rack, mut instances) = instances();
    let instance = InstanceId(4);
    instances
        .create(instance, &plugin_ref(&entry), &catalog, None)
        .unwrap();
    assert!(instances.editor(instance).unwrap().is_none());
    let events: Vec<InstanceEvent> = instances.tick();
    assert!(events.is_empty(), "{events:?}");
}
