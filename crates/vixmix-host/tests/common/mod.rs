//! What the host's integration tests share: the test plugin laid out as an installed bundle,
//! a state for it at a chosen gain, scratch directories, and a setup file written before a
//! boot.
//!
//! The test plugin is `vixmix-plugins`' example `test_gain` (a gain whose component state is
//! the gain as 8 bytes). `cargo test --workspace` (`dev check`, CI) builds every workspace
//! package's examples before it runs any test, into `target/<profile>/examples/`; this lays
//! that binary out as `VixMixTestGain.vst3` under `CARGO_TARGET_TMPDIR`, as the plugins
//! crate's own tests do.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chimaeraproject_vst3_host::{Arch, Os, PluginState, binary_path};
use vixmix_host::{AudioBackend, BootOptions, Host, RealHost};
use vixmix_model::{PluginRef, Setup, persist};

pub const BUNDLE_NAME: &str = "VixMixTestGain.vst3";
pub const PLUGIN_NAME: &str = "Vix Mix Test Gain";

/// The built example, or a panic that says how to build it.
fn built_example() -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary knows its own path");
    // target/<profile>/deps/<test> -> target/<profile>/examples/
    let profile_dir = exe
        .parent()
        .and_then(Path::parent)
        .expect("the test binary lives in target/<profile>/deps");
    let file = if cfg!(target_os = "windows") {
        "test_gain.dll"
    } else if cfg!(target_os = "macos") {
        "libtest_gain.dylib"
    } else {
        "libtest_gain.so"
    };
    let path = profile_dir.join("examples").join(file);
    assert!(
        path.is_file(),
        "the test plugin `{}` has not been built; run `cargo test --workspace` (which builds \
         every package's examples), or `cargo build -p vixmix-plugins --example test_gain` \
         before running the host's tests alone",
        path.display()
    );
    path
}

fn root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("vixmix-host-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    })
}

/// A directory of this process holding one freshly laid-out copy of the test plugin.
pub fn plugin_dir(name: &str) -> PathBuf {
    let dir = root().join("plugins").join(name);
    let bundle = dir.join(BUNDLE_NAME);
    let target = binary_path(&bundle, Os::current(), Arch::current());
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::copy(built_example(), &target).unwrap();
    dir
}

/// An empty configuration directory of this process.
pub fn config_dir(name: &str) -> PathBuf {
    let dir = root().join("config").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A scratch directory of this process, for files a test writes.
pub fn scratch(name: &str) -> PathBuf {
    let dir = root().join("scratch").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The state the test plugin saves at `gain`: its component state is the gain as 8 bytes,
/// its controller's is empty.
pub fn gain_state(gain: f64) -> Vec<u8> {
    PluginState {
        component: gain.to_le_bytes().to_vec(),
        controller: Vec::new(),
    }
    .encode()
}

/// Boots the real host on `config`, scanning only `plugins`, with `audio`.
pub fn boot(config: &Path, plugins: &Path, audio: AudioBackend) -> RealHost {
    RealHost::boot(BootOptions {
        config_dir: Some(config.to_path_buf()),
        debug_build: true,
        audio,
        plugin_dirs: Some(vec![plugins.to_path_buf()]),
        ..BootOptions::default()
    })
    .unwrap()
}

/// How a setup names the test plugin, as the scan found it.
pub fn test_plugin(host: &RealHost) -> PluginRef {
    let entry = host
        .catalog()
        .plugins
        .iter()
        .find(|p| p.name == PLUGIN_NAME)
        .unwrap_or_else(|| panic!("the scan did not find {PLUGIN_NAME}: {:?}", host.catalog()));
    PluginRef {
        id: entry.id.clone(),
        bundle: entry.bundle.clone(),
        name: entry.name.clone(),
    }
}

/// Writes `setup` as the configuration directory's `setup.ron`.
pub fn write_setup(config: &Path, setup: &Setup) {
    persist::save(config, setup).unwrap();
}
