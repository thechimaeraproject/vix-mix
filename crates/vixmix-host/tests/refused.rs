//! The real host on Linux when PipeWire refuses: the null backend with PipeWire's refusal in
//! the banner, PipeWire asked again within five seconds, and the backend taken as soon as the
//! daemon answers. The refusal is made by pointing `PIPEWIRE_REMOTE` at a socket that does not
//! exist; the daemon of `scripts/ci/headless-pipewire.sh` answers once it is put back. It is
//! alone in this binary because it changes the process's environment.
//!
//! Without the script's marker the test says how to start the daemon and passes without
//! touching anything, except under `CI`, where a missing daemon fails.

#![cfg(target_os = "linux")]

use std::time::{Duration, Instant};

use vixmix_host::{AudioBackend, BackendState, BootOptions, Host, RealHost};

const HOW: &str = "the host's fallback test needs the headless daemon: run `source scripts/ci/headless-pipewire.sh` in this shell first (docs/cookbook/run-headless-pipewire.md); it never runs against a desktop session";

/// How long after a refusal the host asks PipeWire again (`RETRY_CONNECT`).
const RETRY: Duration = Duration::from_secs(5);

#[test]
fn a_pipewire_that_refuses_is_the_null_backend_with_its_reason_and_is_asked_again() {
    if std::env::var_os("VIXMIX_HEADLESS_PIPEWIRE").is_none() {
        assert!(std::env::var_os("CI").is_none(), "{HOW}");
        eprintln!("skipped: {HOW}");
        return;
    }
    let config = std::env::temp_dir().join(format!("vixmix-host-refused-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&config);
    let options = BootOptions {
        config_dir: Some(config.clone()),
        debug_build: true,
        audio: AudioBackend::System {
            app_name: "vixmix-host-refused",
        },
        plugin_dirs: Some(Vec::new()),
        ..BootOptions::default()
    };
    let remote = std::env::var_os("PIPEWIRE_REMOTE");
    // SAFETY: this binary has one test and no thread of its own reads the environment now:
    // the host has not started yet, and a refused connect has joined its control thread
    // before `boot` returns.
    unsafe { std::env::set_var("PIPEWIRE_REMOTE", "vixmix-no-such-daemon") };
    let booted = Instant::now();
    let mut host = RealHost::boot(options).unwrap();
    let banner = match host.backend() {
        BackendState::Absent { reason } => reason.clone(),
        other => panic!("PipeWire refused, and the host says {other:?}"),
    };
    assert!(
        banner.starts_with("No audio backend: PipeWire refused to connect to the daemon")
            && banner.contains("systemctl --user status pipewire"),
        "the banner says what failed and what to check: {banner}"
    );
    let wait = host.next_poll().expect("the host asks PipeWire again");
    assert!(wait <= RETRY, "asked again after {wait:?}");
    // Silent but alive: each poll pumps the null backend.
    let _ = host.poll();
    assert_eq!(host.stats().rate, 48_000);

    // The daemon answers again: the next ask finds it.
    // SAFETY: as above; the null backend runs no thread.
    unsafe {
        match &remote {
            Some(value) => std::env::set_var("PIPEWIRE_REMOTE", value),
            None => std::env::remove_var("PIPEWIRE_REMOTE"),
        }
    }
    let deadline = booted + RETRY + Duration::from_secs(5);
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        let wait = host
            .next_poll()
            .unwrap_or(Duration::from_millis(50))
            .min(Duration::from_millis(250));
        std::thread::sleep(wait);
        seen.extend(host.poll());
        if !matches!(host.backend(), BackendState::Absent { .. }) {
            break;
        }
    }
    assert!(
        matches!(
            host.backend(),
            BackendState::Connecting | BackendState::Running { .. } | BackendState::Idle { .. }
        ),
        "the host did not take PipeWire once it answered: {:?}, after {:?}; events {seen:?}",
        host.backend(),
        booted.elapsed()
    );
    assert!(
        booted.elapsed() >= RETRY - Duration::from_millis(100),
        "asked again before its time"
    );
    drop(host);
    let _ = std::fs::remove_dir_all(&config);
}
