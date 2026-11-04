//! The test plugin this crate's integration tests load: "Vix Mix Test Gain".
//!
//! A copy of the kit's `crates/vst3-host/examples/test_gain.rs` (the chimaeraproject kit, MIT),
//! at the tag `kit-source.json` pins, which is itself derived from `examples/gain.rs` of the
//! `vst3` crate (coupler-rs/vst3-rs, Copyright (c) Micah Johnston, MIT OR Apache-2.0). It lives
//! here because `cargo test` builds the examples of the workspace's own packages only: a
//! dependency's example is built when asked for by name, which no gate does.
//!
//! What the kit's copy changed from the stock example, kept here:
//!
//! - an `IPluginFactory2` whose audio class carries sub-categories, and a vendor;
//! - `initialize` asks the host context for `IHostApplication::getName` and refuses to start
//!   without it, which proves the host's context reaches the plugin;
//! - the component's state is the gain as 8 little-endian bytes (`getState`, `setState`, and
//!   the controller's `setComponentState`);
//! - `process` applies **every** point of parameter 0 as a step at its sample offset (the stock
//!   example applied only a block's last point), so a test can predict each sample exactly;
//! - a second, read-only parameter 1 "Level" reports the block's output peak through
//!   `outputParameterChanges`;
//! - both halves implement `IConnectionPoint`, so the host's connect and disconnect run;
//! - `#[unsafe(no_mangle)]` exports, with lowercase `bundleEntry`/`bundleExit` on macOS.
//!
//! What this copy changes from the kit's: fresh class ids and the name "Vix Mix Test Gain",
//! so it is never mistaken for the kit's plugin in one scan, and the sub-categories `Fx|Tools`,
//! so the catalog's classification has something to read (it files the plugin under Utility).
//!
//! Built by `cargo test` as a `cdylib` example; `tests/common/mod.rs` lays it out as a bundle.

#![allow(non_snake_case)]
#![allow(unsafe_op_in_unsafe_fn)]
#![allow(clippy::missing_safety_doc)]
// The SDK's enum constants are `i32` on Windows and `u32` elsewhere.
#![allow(clippy::unnecessary_cast)]

use std::cell::Cell;
use std::ffi::{CString, c_char, c_void};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{ptr, slice};

use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComRef, ComWrapper, uid};

fn copy_cstring(src: &str, dst: &mut [c_char]) {
    let c_string = CString::new(src).unwrap_or_default();
    let bytes = c_string.as_bytes_with_nul();
    for (src, dst) in bytes.iter().zip(dst.iter_mut()) {
        *dst = *src as c_char;
    }
    if bytes.len() > dst.len()
        && let Some(last) = dst.last_mut()
    {
        *last = 0;
    }
}

fn copy_wstring(src: &str, dst: &mut [TChar]) {
    let mut len = 0;
    for (src, dst) in src.encode_utf16().zip(dst.iter_mut()) {
        *dst = src as TChar;
        len += 1;
    }
    if len < dst.len() {
        dst[len] = 0;
    } else if let Some(last) = dst.last_mut() {
        *last = 0;
    }
}

const PLUGIN_NAME: &str = "Vix Mix Test Gain";
const VENDOR: &str = "chimaeraproject";
const VERSION: &str = "1.0.0";
const GAIN: ParamID = 0;
const LEVEL: ParamID = 1;
/// Points of parameter 0 one block applies; more are ignored.
const MAX_POINTS: usize = 64;

/// The host's name, if the context answers `IHostApplication::getName`.
unsafe fn host_name(context: *mut FUnknown) -> Option<String> {
    let host = ComRef::from_raw(context)?.cast::<IHostApplication>()?;
    let mut name: String128 = [0; 128];
    if host.getName(&mut name) != kResultOk {
        return None;
    }
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    Some(String::from_utf16_lossy(&name[..end]))
}

unsafe fn read_gain(stream: *mut IBStream) -> Option<f64> {
    let stream = ComRef::from_raw(stream)?;
    let mut bytes = [0u8; 8];
    let mut read = 0;
    if stream.read(bytes.as_mut_ptr().cast(), 8, &mut read) != kResultOk || read != 8 {
        return None;
    }
    Some(f64::from_le_bytes(bytes))
}

struct TestGainProcessor {
    gain: AtomicU64,
}

impl Class for TestGainProcessor {
    type Interfaces = (
        IComponent,
        IAudioProcessor,
        IProcessContextRequirements,
        IConnectionPoint,
    );
}

impl TestGainProcessor {
    const CID: TUID = uid(0x7A1C5E93, 0x4F2B8D06, 0xB3E9017C, 0x5D84A2F1);

    fn new() -> TestGainProcessor {
        TestGainProcessor {
            gain: AtomicU64::new(1.0f64.to_bits()),
        }
    }
}

impl IPluginBaseTrait for TestGainProcessor {
    unsafe fn initialize(&self, context: *mut FUnknown) -> tresult {
        match host_name(context) {
            Some(name) if !name.is_empty() => kResultOk,
            _ => kResultFalse,
        }
    }

    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IConnectionPointTrait for TestGainProcessor {
    unsafe fn connect(&self, other: *mut IConnectionPoint) -> tresult {
        if other.is_null() {
            kInvalidArgument
        } else {
            kResultOk
        }
    }

    unsafe fn disconnect(&self, _other: *mut IConnectionPoint) -> tresult {
        kResultOk
    }

    unsafe fn notify(&self, _message: *mut IMessage) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for TestGainProcessor {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        *class_id = TestGainController::CID;
        kResultOk
    }

    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }

    unsafe fn getBusCount(&self, media_type: MediaType, dir: BusDirection) -> i32 {
        match media_type as MediaTypes {
            MediaTypes_::kAudio => match dir as BusDirections {
                BusDirections_::kInput | BusDirections_::kOutput => 1,
                _ => 0,
            },
            _ => 0,
        }
    }

    unsafe fn getBusInfo(
        &self,
        media_type: MediaType,
        dir: BusDirection,
        index: i32,
        bus: *mut BusInfo,
    ) -> tresult {
        if media_type as MediaTypes != MediaTypes_::kAudio || index != 0 {
            return kInvalidArgument;
        }
        let bus = &mut *bus;
        let name = match dir as BusDirections {
            BusDirections_::kInput => "Input",
            BusDirections_::kOutput => "Output",
            _ => return kInvalidArgument,
        };
        bus.mediaType = MediaTypes_::kAudio as MediaType;
        bus.direction = dir;
        bus.channelCount = 2;
        copy_wstring(name, &mut bus.name);
        bus.busType = BusTypes_::kMain as BusType;
        bus.flags = BusInfo_::BusFlags_::kDefaultActive as u32;
        kResultOk
    }

    unsafe fn getRoutingInfo(&self, _in: *mut RoutingInfo, _out: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }

    unsafe fn activateBus(
        &self,
        _media_type: MediaType,
        _dir: BusDirection,
        _index: i32,
        _state: TBool,
    ) -> tresult {
        kResultOk
    }

    unsafe fn setActive(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        match read_gain(state) {
            Some(gain) => {
                self.gain.store(gain.to_bits(), Ordering::Relaxed);
                kResultOk
            }
            None => kResultFalse,
        }
    }

    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        let Some(stream) = ComRef::from_raw(state) else {
            return kInvalidArgument;
        };
        let mut bytes = f64::from_bits(self.gain.load(Ordering::Relaxed)).to_le_bytes();
        let mut written = 0;
        stream.write(bytes.as_mut_ptr().cast(), 8, &mut written)
    }
}

impl IAudioProcessorTrait for TestGainProcessor {
    unsafe fn setBusArrangements(
        &self,
        inputs: *mut SpeakerArrangement,
        num_ins: i32,
        outputs: *mut SpeakerArrangement,
        num_outs: i32,
    ) -> tresult {
        if num_ins > 1 || num_outs != 1 {
            return kResultFalse;
        }
        if num_ins == 1 && *inputs != SpeakerArr::kStereo {
            return kResultFalse;
        }
        if *outputs != SpeakerArr::kStereo {
            return kResultFalse;
        }
        kResultTrue
    }

    unsafe fn getBusArrangement(
        &self,
        _dir: BusDirection,
        index: i32,
        arr: *mut SpeakerArrangement,
    ) -> tresult {
        if index != 0 {
            return kInvalidArgument;
        }
        *arr = SpeakerArr::kStereo;
        kResultOk
    }

    unsafe fn canProcessSampleSize(&self, symbolic_sample_size: i32) -> tresult {
        match symbolic_sample_size as SymbolicSampleSizes {
            SymbolicSampleSizes_::kSample32 => kResultOk,
            SymbolicSampleSizes_::kSample64 => kNotImplemented,
            _ => kInvalidArgument,
        }
    }

    unsafe fn getLatencySamples(&self) -> u32 {
        0
    }

    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        if (*setup).symbolicSampleSize != SymbolicSampleSizes_::kSample32 as i32 {
            return kResultFalse;
        }
        kResultOk
    }

    unsafe fn setProcessing(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let data = &*data;

        // Every point of parameter 0, in the order the queue holds them (by offset).
        let mut points = [(0i32, 0f64); MAX_POINTS];
        let mut count = 0;
        if let Some(changes) = ComRef::from_raw(data.inputParameterChanges) {
            for index in 0..changes.getParameterCount() {
                let Some(queue) = ComRef::from_raw(changes.getParameterData(index)) else {
                    continue;
                };
                if queue.getParameterId() != GAIN {
                    continue;
                }
                for point in 0..queue.getPointCount() {
                    let (mut offset, mut value) = (0, 0.0);
                    if count < MAX_POINTS
                        && queue.getPoint(point, &mut offset, &mut value) == kResultOk
                    {
                        points[count] = (offset, value);
                        count += 1;
                    }
                }
            }
        }

        let n = data.numSamples.max(0) as usize;
        let mut gain = f64::from_bits(self.gain.load(Ordering::Relaxed));
        let mut peak = 0f32;
        if data.numOutputs >= 1 && !data.outputs.is_null() {
            let out_bus = &*data.outputs;
            let out_channels = out_bus.numChannels.max(0) as usize;
            let outs: &[*mut f32] = if out_channels == 0 {
                &[]
            } else {
                slice::from_raw_parts(out_bus.__field0.channelBuffers32, out_channels)
            };
            let ins: &[*mut f32] = if data.numInputs >= 1 && !data.inputs.is_null() {
                let in_bus = &*data.inputs;
                let in_channels = in_bus.numChannels.max(0) as usize;
                if in_channels == 0 {
                    &[]
                } else {
                    slice::from_raw_parts(in_bus.__field0.channelBuffers32, in_channels)
                }
            } else {
                &[]
            };
            let mut next = 0;
            for i in 0..n {
                while next < count && points[next].0.max(0) as usize <= i {
                    gain = points[next].1;
                    next += 1;
                }
                for (c, &out) in outs.iter().enumerate() {
                    // In place when the host shares a bus's pointers, or has no input bus.
                    let input = ins.get(c).copied().unwrap_or(out);
                    let sample = *input.add(i) * gain as f32;
                    *out.add(i) = sample;
                    peak = peak.max(sample.abs());
                }
            }
            while next < count {
                gain = points[next].1;
                next += 1;
            }
        }
        self.gain.store(gain.to_bits(), Ordering::Relaxed);

        if let Some(changes) = ComRef::from_raw(data.outputParameterChanges) {
            let mut index = 0;
            if let Some(queue) = ComRef::from_raw(changes.addParameterData(&LEVEL, &mut index)) {
                let mut point = 0;
                queue.addPoint(0, f64::from(peak.min(1.0)), &mut point);
            }
        }
        kResultOk
    }

    unsafe fn getTailSamples(&self) -> u32 {
        0
    }
}

impl IProcessContextRequirementsTrait for TestGainProcessor {
    unsafe fn getProcessContextRequirements(&self) -> u32 {
        0
    }
}

struct TestGainController {
    gain: Cell<f64>,
    level: Cell<f64>,
    handler: Cell<*mut IComponentHandler>,
}

impl Class for TestGainController {
    type Interfaces = (IEditController, IConnectionPoint);
}

impl TestGainController {
    const CID: TUID = uid(0x2E6B9F14, 0xC0583A7D, 0x91F4E26B, 0x0A7D3C58);

    fn new() -> TestGainController {
        TestGainController {
            gain: Cell::new(1.0),
            level: Cell::new(0.0),
            handler: Cell::new(ptr::null_mut()),
        }
    }
}

impl IPluginBaseTrait for TestGainController {
    unsafe fn initialize(&self, context: *mut FUnknown) -> tresult {
        if host_name(context).is_some() {
            kResultOk
        } else {
            kResultFalse
        }
    }

    unsafe fn terminate(&self) -> tresult {
        self.handler.set(ptr::null_mut());
        kResultOk
    }
}

impl IConnectionPointTrait for TestGainController {
    unsafe fn connect(&self, other: *mut IConnectionPoint) -> tresult {
        if other.is_null() {
            kInvalidArgument
        } else {
            kResultOk
        }
    }

    unsafe fn disconnect(&self, _other: *mut IConnectionPoint) -> tresult {
        kResultOk
    }

    unsafe fn notify(&self, _message: *mut IMessage) -> tresult {
        kResultOk
    }
}

impl IEditControllerTrait for TestGainController {
    unsafe fn setComponentState(&self, state: *mut IBStream) -> tresult {
        match read_gain(state) {
            Some(gain) => {
                self.gain.set(gain);
                kResultOk
            }
            None => kResultFalse,
        }
    }

    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getParameterCount(&self) -> i32 {
        2
    }

    unsafe fn getParameterInfo(&self, param_index: i32, info: *mut ParameterInfo) -> tresult {
        let info = &mut *info;
        let (id, title, short, default, flags) = match param_index {
            0 => (
                GAIN,
                "Gain",
                "Gain",
                1.0,
                ParameterInfo_::ParameterFlags_::kCanAutomate,
            ),
            1 => (
                LEVEL,
                "Level",
                "Lvl",
                0.0,
                ParameterInfo_::ParameterFlags_::kIsReadOnly,
            ),
            _ => return kInvalidArgument,
        };
        info.id = id;
        copy_wstring(title, &mut info.title);
        copy_wstring(short, &mut info.shortTitle);
        copy_wstring("", &mut info.units);
        info.stepCount = 0;
        info.defaultNormalizedValue = default;
        info.unitId = 0;
        info.flags = flags;
        kResultOk
    }

    unsafe fn getParamStringByValue(
        &self,
        id: u32,
        value_normalized: f64,
        string: *mut String128,
    ) -> tresult {
        match id {
            GAIN | LEVEL => {
                copy_wstring(&format!("{value_normalized:.2}"), &mut *string);
                kResultOk
            }
            _ => kInvalidArgument,
        }
    }

    unsafe fn getParamValueByString(
        &self,
        _id: u32,
        _string: *mut TChar,
        _value_normalized: *mut f64,
    ) -> tresult {
        kNotImplemented
    }

    unsafe fn normalizedParamToPlain(&self, _id: u32, value_normalized: f64) -> f64 {
        value_normalized
    }

    unsafe fn plainParamToNormalized(&self, _id: u32, plain_value: f64) -> f64 {
        plain_value
    }

    unsafe fn getParamNormalized(&self, id: u32) -> f64 {
        match id {
            GAIN => self.gain.get(),
            LEVEL => self.level.get(),
            _ => 0.0,
        }
    }

    unsafe fn setParamNormalized(&self, id: u32, value: f64) -> tresult {
        match id {
            GAIN => self.gain.set(value),
            LEVEL => self.level.set(value),
            _ => return kInvalidArgument,
        }
        kResultOk
    }

    unsafe fn setComponentHandler(&self, handler: *mut IComponentHandler) -> tresult {
        self.handler.set(handler);
        kResultOk
    }

    unsafe fn createView(&self, _name: *const c_char) -> *mut IPlugView {
        ptr::null_mut()
    }
}

struct Factory {}

impl Class for Factory {
    type Interfaces = (IPluginFactory2,);
}

impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        let info = &mut *info;
        copy_cstring(VENDOR, &mut info.vendor);
        copy_cstring(
            "https://github.com/thechimaeraproject/chimaeraproject-kit",
            &mut info.url,
        );
        copy_cstring("", &mut info.email);
        info.flags = PFactoryInfo_::FactoryFlags_::kUnicode as int32;
        kResultOk
    }

    unsafe fn countClasses(&self) -> i32 {
        2
    }

    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        let info = &mut *info;
        let (cid, category) = match index {
            0 => (TestGainProcessor::CID, "Audio Module Class"),
            1 => (TestGainController::CID, "Component Controller Class"),
            _ => return kInvalidArgument,
        };
        info.cid = cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring(category, &mut info.category);
        copy_cstring(PLUGIN_NAME, &mut info.name);
        kResultOk
    }

    unsafe fn createInstance(
        &self,
        cid: FIDString,
        iid: FIDString,
        obj: *mut *mut c_void,
    ) -> tresult {
        let instance: Option<ComPtr<FUnknown>> = match *(cid as *const TUID) {
            TestGainProcessor::CID => ComWrapper::new(TestGainProcessor::new()).to_com_ptr(),
            TestGainController::CID => ComWrapper::new(TestGainController::new()).to_com_ptr(),
            _ => None,
        };
        if let Some(instance) = instance {
            let ptr = instance.as_ptr();
            ((*(*ptr).vtbl).queryInterface)(ptr, iid as *mut TUID, obj)
        } else {
            kInvalidArgument
        }
    }
}

impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: i32, info: *mut PClassInfo2) -> tresult {
        let info = &mut *info;
        let (cid, category, sub) = match index {
            0 => (TestGainProcessor::CID, "Audio Module Class", "Fx|Tools"),
            1 => (TestGainController::CID, "Component Controller Class", ""),
            _ => return kInvalidArgument,
        };
        info.cid = cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring(category, &mut info.category);
        copy_cstring(PLUGIN_NAME, &mut info.name);
        info.classFlags = 0;
        copy_cstring(sub, &mut info.subCategories);
        copy_cstring(VENDOR, &mut info.vendor);
        copy_cstring(VERSION, &mut info.version);
        copy_cstring("VST 3.7.9", &mut info.sdkVersion);
        kResultOk
    }
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
extern "system" fn InitDll() -> bool {
    true
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
extern "system" fn ExitDll() -> bool {
    true
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn bundleEntry(_bundle_ref: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn bundleExit() -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleEntry(_library_handle: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleExit() -> bool {
    true
}

#[unsafe(no_mangle)]
extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    ComWrapper::new(Factory {})
        .to_com_ptr::<IPluginFactory>()
        .map_or(ptr::null_mut(), ComPtr::into_raw)
}
