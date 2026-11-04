//! The test plugin as an installed bundle, and a chain filled from the rack as the engine
//! fills one.
//!
//! `cargo test` builds `examples/test_gain.rs` as a `cdylib` and uplifts it to
//! `target/<profile>/examples/`; this lays it out the way an installed plugin is laid out
//! (`<dir>/VixMixTestGain.vst3/Contents/<arch>-<os>/VixMixTestGain.<ext>`) under
//! `CARGO_TARGET_TMPDIR`, so the source tree stays clean, as the kit's own
//! `crates/vst3-host/tests/common/mod.rs` does.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chimaeraproject_audio_processor::{
    AudioError, BusBuffer, BusLayout, EngineConfig, ProcessSetup, ProcessorChain,
    ProcessorInstanceId, ProcessorParams, TransportInfo,
};
use chimaeraproject_vst3_host::{Arch, ClassId, HostContext, Os, binary_path};
use vixmix_engine::Lender;
use vixmix_model::{InstanceId, PluginRef};
use vixmix_plugins::{Catalog, PluginEntry};

/// The test plugin's audio class (`examples/test_gain.rs`).
pub const AUDIO_CLASS: ClassId = ClassId::new(0x7A1C5E93, 0x4F2B8D06, 0xB3E9017C, 0x5D84A2F1);
/// Its gain parameter: the normalized value is the linear gain.
pub const GAIN: u32 = 0;
/// Its read-only meter.
pub const LEVEL: u32 = 1;

pub const BUNDLE_NAME: &str = "VixMixTestGain.vst3";

/// The engine's shape for these tests: blocks of at most 64 frames, stereo.
pub const ENGINE: EngineConfig = EngineConfig::new(48_000, 64, 2);
pub const FRAMES: usize = 64;

/// The built example, or a panic that says how to build it.
fn built_example() -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary knows its own path");
    // target/<profile>/deps/<test>.exe -> target/<profile>/examples/
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
        "the test plugin `{}` has not been built; run `cargo test -p vixmix-plugins` (which \
         builds examples), or `cargo build -p vixmix-plugins --example test_gain` before \
         running one test target alone",
        path.display()
    );
    path
}

fn root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("vixmix-plugins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    })
}

/// A directory of this process holding one freshly laid-out copy of the test plugin, for a
/// scan to find: each test gets its own, so none of them sees another's changes.
pub fn plugin_dir(name: &str) -> PathBuf {
    let dir = root().join(name);
    let bundle = dir.join(BUNDLE_NAME);
    let target = binary_path(&bundle, Os::current(), Arch::current());
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    install_binary(&bundle);
    dir
}

/// Copies the test plugin's binary into `bundle`, whose `Contents/<arch>-<os>/` must exist,
/// and returns where it went.
pub fn install_binary(bundle: &Path) -> PathBuf {
    let target = binary_path(bundle, Os::current(), Arch::current());
    std::fs::copy(built_example(), &target).unwrap();
    target
}

/// A scratch directory of this process, for files a test writes.
pub fn scratch(name: &str) -> PathBuf {
    let dir = root().join("scratch").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The host every test runs as.
pub fn host() -> HostContext {
    HostContext::new("Vix Mix Tests")
}

/// A catalog of a scan of `dir`, which must have found the test plugin.
pub fn catalog_of(dir: &Path) -> (Catalog, PluginEntry) {
    let outcome = Catalog::scan(&[dir.to_path_buf()], &host());
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    let entry = outcome
        .catalog
        .get(&AUDIO_CLASS.processor_id())
        .expect("the scan found the test plugin")
        .clone();
    (outcome.catalog, entry)
}

/// How a setup's insert names the test plugin.
pub fn plugin_ref(entry: &PluginEntry) -> PluginRef {
    PluginRef {
        id: entry.id.clone(),
        bundle: entry.bundle.clone(),
        name: entry.name.clone(),
    }
}

/// What the engine's chain asks for one insert of `plugin_id`: no parameter values, since a
/// VST3 plugin's parameters live in its own state.
pub fn insert(instance: InstanceId, plugin_id: &str) -> ProcessorParams {
    ProcessorParams::new(ProcessorInstanceId(u64::from(instance.0)), plugin_id)
}

/// An empty stereo insert chain at [`ENGINE`]'s setup.
pub fn stereo_chain() -> ProcessorChain {
    ProcessorChain::new(ProcessSetup::realtime(&ENGINE), BusLayout::stereo_insert())
}

/// Fills `chain` from `lender` as the engine does: a held instance is lent; one the lender
/// does not hold, or cannot lend, is a reported pass-through.
pub fn fill(chain: &mut ProcessorChain, desired: &[ProcessorParams], lender: &dyn Lender) {
    chain.fill_with(desired, |id, key| {
        let instance = InstanceId(u32::try_from(id.0).expect("instance ids are u32"));
        match lender.lend(instance, key) {
            Ok(Some(processor)) => Ok(processor),
            Ok(None) => Err(AudioError::UnknownProcessor(key.to_string())),
            Err(error) => Err(AudioError::SetupFailed {
                processor: key.to_string(),
                reason: error.to_string(),
            }),
        }
    });
}

/// One block of a 0.5 ramp through `chain`; the left channel that came out.
pub fn run(chain: &mut ProcessorChain, desired: &[ProcessorParams]) -> Vec<f32> {
    let mut bus = BusBuffer::new(2, FRAMES);
    for channel in 0..2 {
        for (i, sample) in bus.channel_mut(channel)[..FRAMES].iter_mut().enumerate() {
            *sample = 0.5 * (i as f32 / FRAMES as f32);
        }
    }
    let transport = TransportInfo::playing_at(0, ENGINE.sample_rate);
    chain.process(&mut bus, FRAMES, &transport, desired);
    bus.channel(0)[..FRAMES].to_vec()
}

/// The ramp [`run`] feeds, scaled by `gain`.
pub fn ramp(gain: f32) -> Vec<f32> {
    (0..FRAMES)
        .map(|i| 0.5 * (i as f32 / FRAMES as f32) * gain)
        .collect()
}
