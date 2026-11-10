//! The scan with every bundle that must be loaded described in a child process: the test
//! child (`examples/scan_bundle.rs`, which prints noise around its answer), a plugin that
//! aborts while it loads (`examples/test_abort.rs`), one that never finishes loading
//! (`examples/test_hang.rs`), the test plugin, and a bundle described by its
//! `moduleinfo.json` alone.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use chimaeraproject_vst3_host::ClassId;
use common::*;
use vixmix_plugins::{ANSWER_MARK, Catalog, Describe, ScanCache, ScanCommand};

/// The class [`info_bundle`] declares.
const INFO_CLASS: ClassId = ClassId::new(0x1D4E7A20, 0x5B3C4F81, 0x9E62D0A7, 0x3F18C5B9);

/// A bundle with a `moduleinfo.json` and no binary: it can be described, never loaded.
fn info_bundle(dir: &Path, name: &str) -> PathBuf {
    let bundle = dir.join(name);
    let resources = bundle.join("Contents").join("Resources");
    std::fs::create_dir_all(&resources).unwrap();
    let info = format!(
        r#"{{
            "Name": "Vix Mix Info Echo",
            "Factory Info": {{ "Vendor": "chimaeraproject" }},
            "Classes": [{{
                "CID": "{}",
                "Category": "Audio Module Class",
                "Name": "Vix Mix Info Echo",
                "Vendor": "chimaeraproject",
                "Sub Categories": ["Fx", "Delay"]
            }}]
        }}"#,
        INFO_CLASS.to_hex()
    );
    std::fs::write(resources.join("moduleinfo.json"), info).unwrap();
    bundle
}

/// The test child, at the default timeout.
fn child() -> ScanCommand {
    ScanCommand::new(scan_child(), Vec::<String>::new())
}

/// A command that cannot start.
fn nowhere(dir: &Path) -> Describe {
    Describe::InChild(ScanCommand::new(
        dir.join("no-such-scanner"),
        Vec::<String>::new(),
    ))
}

fn assert_teaches(failure: &str, bundle: &str) {
    assert!(failure.contains(bundle), "{failure}");
    assert!(
        failure.contains("so it was skipped and the other plugins were scanned"),
        "{failure}"
    );
    assert!(failure.contains("fix or remove the plugin"), "{failure}");
    assert!(failure.ends_with("(menu, Rescan plugins)"), "{failure}");
}

#[test]
fn a_plugin_that_aborts_while_loading_fails_alone_and_the_scan_goes_on() {
    let dir = scratch("abort");
    info_bundle(&dir, "A Info.vst3");
    install_example(&dir, "B Abort.vst3", "test_abort");
    let gain = install_example(&dir, "C Gain.vst3", "test_gain");
    let host = host();
    let how = Describe::InChild(child());
    let dirs = std::slice::from_ref(&dir);

    // On Windows a crash dialog would hold the child until the timeout, and the failure
    // would say it took too long: it says it crashed.
    let first = Catalog::default().rescan_with(dirs, &host, &ScanCache::default(), &how);
    assert_eq!((first.described, first.reused), (3, 0));
    let [failure] = first.failures.as_slice() else {
        panic!("one failure: {:?}", first.failures);
    };
    assert_teaches(failure, "B Abort.vst3");
    assert!(
        failure.contains("the plugin crashed while it loaded"),
        "{failure}"
    );
    // The one before it, described from its moduleinfo.json, and the one after it, loaded in
    // a child of its own.
    let info = first
        .catalog
        .get(&INFO_CLASS.processor_id())
        .expect("the bundle before the crash");
    assert_eq!(info.name, "Vix Mix Info Echo");
    let after = first
        .catalog
        .get(&AUDIO_CLASS.processor_id())
        .expect("the bundle after the crash");
    assert_eq!(after.path, gain);
    assert_eq!(first.catalog.plugins.len(), 2);

    // A start with that outcome's record says the crash again and loads nothing: its command
    // here cannot even start.
    let start = first
        .catalog
        .rescan_with(dirs, &host, &first.cache, &nowhere(&dir));
    assert_eq!((start.described, start.reused), (0, 3));
    assert_eq!(start.failures, first.failures);
    assert_eq!(start.catalog, first.catalog);

    // Rescan plugins forgets it: loaded again in a child, it crashes again, alone.
    let mut cache = start.cache.clone();
    cache.forget_failures();
    assert_eq!(cache.len(), 2);
    let retried = start.catalog.rescan_with(dirs, &host, &cache, &how);
    assert_eq!((retried.described, retried.reused), (1, 2));
    assert_eq!(retried.failures, first.failures);
    assert_eq!(retried.catalog, first.catalog);
}

#[test]
fn a_plugin_that_never_finishes_loading_is_stopped_at_the_timeout() {
    let dir = scratch("hang");
    install_example(&dir, "A Hang.vst3", "test_hang");
    install_example(&dir, "B Gain.vst3", "test_gain");
    let how = Describe::InChild(child().with_timeout(Duration::from_secs(2)));
    let started = Instant::now();
    let outcome = Catalog::default().rescan_with(
        std::slice::from_ref(&dir),
        &host(),
        &ScanCache::default(),
        &how,
    );
    let took = started.elapsed();
    let [failure] = outcome.failures.as_slice() else {
        panic!("one failure: {:?}", outcome.failures);
    };
    assert_teaches(failure, "A Hang.vst3");
    assert!(
        failure.contains("the plugin took longer than 2 seconds to load"),
        "{failure}"
    );
    assert!(outcome.catalog.get(&AUDIO_CLASS.processor_id()).is_some());
    assert!(took >= Duration::from_secs(2), "{took:?}");
    assert!(
        took < Duration::from_secs(30),
        "stopped at the timeout, not waited for: {took:?}"
    );
    assert_eq!(outcome.cache.len(), 2, "the timeout is kept like a crash");
}

#[test]
fn a_bundle_with_a_moduleinfo_needs_no_child_and_one_whose_child_cannot_start_is_not_kept() {
    let dir = scratch("no-child");
    info_bundle(&dir, "A Info.vst3");
    install_example(&dir, "B Gain.vst3", "test_gain");
    let host = host();
    let dirs = std::slice::from_ref(&dir);

    let outcome =
        Catalog::default().rescan_with(dirs, &host, &ScanCache::default(), &nowhere(&dir));
    assert_eq!(outcome.described, 2);
    assert!(
        outcome.catalog.get(&INFO_CLASS.processor_id()).is_some(),
        "described here, running none of its code"
    );
    let [failure] = outcome.failures.as_slice() else {
        panic!("one failure: {:?}", outcome.failures);
    };
    assert!(failure.contains("B Gain.vst3"), "{failure}");
    assert!(
        failure.contains("could not run") && failure.contains("no-such-scanner"),
        "{failure}"
    );
    assert!(failure.ends_with("(menu, Rescan plugins)"), "{failure}");
    assert_eq!(
        outcome.cache.len(),
        1,
        "a cause not the bundle's is not kept"
    );

    // So the next scan, with a child that runs, describes it.
    let next =
        outcome
            .catalog
            .rescan_with(dirs, &host, &outcome.cache, &Describe::InChild(child()));
    assert_eq!((next.described, next.reused), (1, 1));
    assert!(next.failures.is_empty(), "{:?}", next.failures);
    assert!(next.catalog.get(&AUDIO_CLASS.processor_id()).is_some());
}

#[test]
fn the_child_answers_on_one_marked_line_among_noise_and_exits_0() {
    let dir = scratch("noise");
    let bundle = install_example(&dir, "Gain.vst3", "test_gain");
    let out = Command::new(scan_child()).arg(&bundle).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("noise before the answer\n"), "{stdout}");
    assert!(
        stdout.trim_end().ends_with("noise after the answer"),
        "{stdout}"
    );
    let answers: Vec<&str> = stdout
        .lines()
        .filter_map(|line| line.strip_prefix(ANSWER_MARK))
        .collect();
    let [answer] = answers.as_slice() else {
        panic!("one answer: {stdout}");
    };
    assert!(answer.contains("Vix Mix Test Gain"), "{answer}");
    assert!(answer.contains(&AUDIO_CLASS.processor_id()), "{answer}");
}
