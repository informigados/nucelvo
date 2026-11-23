use libloading::os::windows::{
    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, Library,
};
use nucelvo_core::{Reading, Sample, State};
use std::{
    ffi::{CStr, c_char, c_void},
    path::Path,
    ptr,
};
use winreg::{
    RegKey,
    enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY},
};

const SOURCE: &str = "NVIDIA NVML";
const GRAPHICS_CLOCK: u32 = 0;
const MEMORY_CLOCK: u32 = 2;
type Device = *mut c_void;
type Status = i32;
type Init = unsafe extern "C" fn() -> Status;
type Count = unsafe extern "C" fn(*mut u32) -> Status;
type Handle = unsafe extern "C" fn(u32, *mut Device) -> Status;
type Name = unsafe extern "C" fn(Device, *mut c_char, u32) -> Status;
type IndexedGauge = unsafe extern "C" fn(Device, u32, *mut u32) -> Status;
type Utilization = unsafe extern "C" fn(Device, *mut Rates) -> Status;
type Memory = unsafe extern "C" fn(Device, *mut MemoryInfo) -> Status;
type Gauge = unsafe extern "C" fn(Device, *mut u32) -> Status;

#[derive(Default)]
#[repr(C)]
struct Rates {
    gpu: u32,
    memory: u32,
}

#[derive(Default)]
#[repr(C)]
struct MemoryInfo {
    total: u64,
    free: u64,
    used: u64,
}

pub(super) struct Nvml {
    _library: Library,
    initialized: bool,
    shutdown: Init,
    count: Count,
    handle: Handle,
    name: Name,
    temperature: IndexedGauge,
    clock: Option<IndexedGauge>,
    utilization: Utilization,
    memory: Memory,
    fan: Gauge,
    power: Gauge,
}

fn library() -> Option<Library> {
    // SAFETY: Search only the system directory for a DCH driver.
    let dch = unsafe { Library::load_with_flags("nvml.dll", LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if let Ok(dch) = dch {
        return Some(dch);
    }
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion",
            KEY_READ | KEY_WOW64_64KEY,
        )
        .ok()?;
    let program_files: String = key.get_value("ProgramFilesDir").ok()?;
    let path = Path::new(&program_files).join(r"NVIDIA Corporation\NVSMI\nvml.dll");
    if !path.is_absolute() {
        return None;
    }
    // SAFETY: Load the legacy driver from its installation directory discovered
    // through the machine registry. Dependencies are searched there or in System32.
    unsafe {
        Library::load_with_flags(
            path,
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    }
    .ok()
}

impl Nvml {
    pub(super) fn open() -> Option<Self> {
        let library = library()?;
        // SAFETY: These signatures match the NVML C ABI. The library stays
        // loaded for the entire lifetime of these function pointers.
        unsafe {
            let init: Init = *library.get(b"nvmlInit_v2\0").ok()?;
            let mut sampler = Self {
                initialized: false,
                shutdown: *library.get(b"nvmlShutdown\0").ok()?,
                count: *library.get(b"nvmlDeviceGetCount_v2\0").ok()?,
                handle: *library.get(b"nvmlDeviceGetHandleByIndex_v2\0").ok()?,
                name: *library.get(b"nvmlDeviceGetName\0").ok()?,
                temperature: *library.get(b"nvmlDeviceGetTemperature\0").ok()?,
                clock: library
                    .get(b"nvmlDeviceGetClockInfo\0")
                    .ok()
                    .map(|symbol| *symbol),
                utilization: *library.get(b"nvmlDeviceGetUtilizationRates\0").ok()?,
                memory: *library.get(b"nvmlDeviceGetMemoryInfo\0").ok()?,
                fan: *library.get(b"nvmlDeviceGetFanSpeed\0").ok()?,
                power: *library.get(b"nvmlDeviceGetPowerUsage\0").ok()?,
                _library: library,
            };
            sampler.initialized = init() == 0;
            sampler.initialized.then_some(sampler)
        }
    }

    pub(super) fn sample(&self) -> Vec<Sample> {
        let mut count = 0;
        // SAFETY: NVML was initialized and count points to a writable u32.
        if unsafe { (self.count)(&mut count) } != 0 {
            return Vec::new();
        }
        (0..count.min(16))
            .flat_map(|index| self.device(index))
            .collect()
    }

    fn device(&self, index: u32) -> Vec<Sample> {
        let mut device = ptr::null_mut();
        // SAFETY: Output pointers and buffers match NVML's types and sizes.
        // Successful handles are used only while this initialized library is alive.
        unsafe {
            if (self.handle)(index, &mut device) != 0 || device.is_null() {
                return Vec::new();
            }
            let mut name = [0u8; 128];
            let _ = (self.name)(device, name.as_mut_ptr().cast(), name.len() as u32);
            let model = CStr::from_bytes_until_nul(&name)
                .ok()
                .and_then(|value| value.to_str().ok())
                .filter(|value| !value.is_empty())
                .unwrap_or("NVIDIA");
            let label = format!("{model} #{}", index + 1);
            let mut temp = 0;
            let mut utilization = Rates::default();
            let mut memory = MemoryInfo::default();
            let mut fan = 0;
            let mut power = 0;
            let mut samples = vec![
                metric(
                    index,
                    &label,
                    "temperature",
                    ("GPU temperature", "Temperatura da GPU"),
                    "°C",
                    (self.temperature)(device, 0, &mut temp),
                    (temp <= 150).then_some(f64::from(temp)),
                ),
                metric(
                    index,
                    &label,
                    "load",
                    ("GPU usage", "Uso da GPU"),
                    "%",
                    (self.utilization)(device, &mut utilization),
                    (utilization.gpu <= 100).then_some(f64::from(utilization.gpu)),
                ),
                metric(
                    index,
                    &label,
                    "vram",
                    ("VRAM used", "VRAM em uso"),
                    "MiB",
                    (self.memory)(device, &mut memory),
                    (memory.total > 0 && memory.used <= memory.total)
                        .then_some(memory.used as f64 / 1_048_576.0),
                ),
                metric(
                    index,
                    &label,
                    "fan",
                    ("GPU fan speed", "Velocidade da ventoinha da GPU"),
                    "%",
                    (self.fan)(device, &mut fan),
                    Some(f64::from(fan)),
                ),
                metric(
                    index,
                    &label,
                    "power",
                    ("GPU power", "Potência da GPU"),
                    "W",
                    (self.power)(device, &mut power),
                    Some(f64::from(power) / 1000.0),
                ),
            ];
            if let Some(clock) = self.clock {
                for (domain, kind, title) in [
                    (
                        GRAPHICS_CLOCK,
                        "graphics_clock",
                        ("GPU graphics clock", "Clock gráfico da GPU"),
                    ),
                    (
                        MEMORY_CLOCK,
                        "memory_clock",
                        ("GPU memory clock", "Clock da memória da GPU"),
                    ),
                ] {
                    let mut mhz = 0;
                    let status = clock(device, domain, &mut mhz);
                    samples.push(metric(
                        index,
                        &label,
                        kind,
                        title,
                        "Hz",
                        status,
                        Some(f64::from(mhz) * 1e6),
                    ));
                }
            }
            samples
        }
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        // SAFETY: The matching shutdown runs before unloading the library.
        if self.initialized {
            unsafe { (self.shutdown)() };
        }
    }
}

fn metric(
    index: u32,
    device: &str,
    kind: &str,
    (en, pt): (&str, &str),
    unit: &'static str,
    status: Status,
    value: Option<f64>,
) -> Sample {
    let state = match status {
        0 if value.is_some() => State::Ok,
        3 | 13 => State::Unsupported,
        4 => State::PermissionDenied,
        9 | 10 | 15 => State::TemporarilyUnavailable,
        _ => State::Error,
    };
    let mut result = super::sample(
        format!("gpu:nvidia:{index}:{kind}"),
        format!("{en} · {device}"),
        None,
        unit,
        Reading {
            source: SOURCE,
            state,
            value: value.filter(|_| state == State::Ok),
        },
    );
    result.name_pt = Some(format!("{pt} · {device}").into());
    result
}
