//! The real host's plugin scan through a child process, as the app asks for it:
//! `BootOptions::plugin_scan` reaches the scan. The child is `vixmix-plugins`' example
//! `scan_bundle`, which `cargo test --workspace` builds beside the test plugin.

mod common;

use std::path::{Path, PathBuf};

use common::{BUNDLE_NAME, PLUGIN_NAME};
use vixmix_host::{AudioBackend, BootOptions, Describe, Host, HostEvent, RealHost, ScanCommand};

/// `vixmix-plugins`' example `scan_bundle`, built.
fn scan_child() -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary knows its own path");
    let path = exe
        .parent()
        .and_then(Path::parent)
        .expect("the test binary lives in target/<profile>/deps")
        .join("examples")
        .join(format!("scan_bundle{}", std::env::consts::EXE_SUFFIX));
    assert!(
        path.is_file(),
        "the scan's test child `{}` has not been built; run `cargo test --workspace`, or \
         `cargo build -p vixmix-plugins --example scan_bundle` before running the host's tests \
         alone",
        path.display()
    );
    path
}

fn boot(config: &Path, plugins: &Path, plugin_scan: Describe) -> RealHost {
    RealHost::boot(BootOptions {
        config_dir: Some(config.to_path_buf()),
        debug_build: true,
        audio: AudioBackend::Null,
        plugin_dirs: Some(vec![plugins.to_path_buf()]),
        plugin_scan,
    })
    .unwrap()
}

fn toasts(events: Vec<HostEvent>) -> Vec<String> {
    events
        .into_iter()
        .filter_map(|event| match event {
            HostEvent::Toast(toast) => Some(toast),
            _ => None,
        })
        .collect()
}

#[test]
fn the_boot_options_scan_loads_each_plugin_in_the_child_it_names() {
    let plugins = common::plugin_dir("scan-child");
    let config = common::config_dir("scan-child");

    // A child that cannot start: the test plugin has no moduleinfo.json and must be loaded,
    // so the scan says so, and keeps nothing of it.
    let nowhere = plugins.join("no-such-scanner");
    let mut host = boot(
        &config,
        &plugins,
        Describe::InChild(ScanCommand::new(&nowhere, ["--scan-bundle"])),
    );
    let said = toasts(host.poll());
    assert!(
        said.iter()
            .any(|toast| toast.contains(BUNDLE_NAME) && toast.contains("no-such-scanner")),
        "{said:?}"
    );
    assert!(host.catalog().plugins.is_empty());
    drop(host);

    // The next start, with a child that runs, loads it there and finds it.
    let host = boot(
        &config,
        &plugins,
        Describe::InChild(ScanCommand::new(scan_child(), Vec::<String>::new())),
    );
    assert!(
        host.catalog().plugins.iter().any(|p| p.name == PLUGIN_NAME),
        "{:?}",
        host.catalog()
    );
}
