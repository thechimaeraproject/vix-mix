//! A plugin that never finishes loading, as one waiting on a dialog or a licence server that
//! does not answer.
//!
//! A VST3 module only as far as its entry points go: the hook the kit's `Module::load` calls
//! first on each platform (`InitDll`, `ModuleEntry`, `bundleEntry`) and `GetPluginFactory`,
//! which it calls next, each sleep forever. A scan that loads it in a child process stops the
//! child at its timeout (`tests/scan_child.rs`). Built by `cargo test` as a `cdylib` example;
//! `tests/common/mod.rs` lays it out as a bundle.

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::time::Duration;

fn forever() -> ! {
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
extern "system" fn InitDll() -> bool {
    forever()
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleEntry(_library_handle: *mut c_void) -> bool {
    forever()
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn bundleEntry(_bundle_ref: *mut c_void) -> bool {
    forever()
}

#[unsafe(no_mangle)]
extern "system" fn GetPluginFactory() -> *mut c_void {
    forever()
}
