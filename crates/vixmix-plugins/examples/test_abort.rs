//! A plugin that aborts while its module loads, as a bridged plugin does when its factory
//! cannot be made (issue 26: yabridge calls `std::terminate` inside `ModuleEntry`).
//!
//! A VST3 module only as far as its entry points go: the hook the kit's `Module::load` calls
//! first on each platform (`InitDll`, `ModuleEntry`, `bundleEntry`) and `GetPluginFactory`,
//! which it calls next, each call `std::process::abort()`. An abort cannot be caught, so only
//! a scan that loads it in a child process survives it (`tests/scan_child.rs`). Built by
//! `cargo test` as a `cdylib` example; `tests/common/mod.rs` lays it out as a bundle.

#![allow(non_snake_case)]

use std::ffi::c_void;

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
extern "system" fn InitDll() -> bool {
    std::process::abort()
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleEntry(_library_handle: *mut c_void) -> bool {
    std::process::abort()
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn bundleEntry(_bundle_ref: *mut c_void) -> bool {
    std::process::abort()
}

#[unsafe(no_mangle)]
extern "system" fn GetPluginFactory() -> *mut c_void {
    std::process::abort()
}
