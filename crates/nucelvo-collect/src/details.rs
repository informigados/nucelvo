use chrono::Utc;
use nucelvo_core::{Category as C, Field, Reading, Sample, Snapshot, State};
use std::time::Instant;

#[cfg(target_os = "macos")]
use super::platform as native;
#[cfg(windows)]
#[path = "displays_windows.rs"]
mod displays;
#[cfg(target_os = "linux")]
#[path = "details_linux.rs"]
mod native;
#[cfg(windows)]
#[path = "details_windows.rs"]
mod native;
#[cfg(any(windows, target_os = "linux"))]
#[path = "network.rs"]
mod network;
#[path = "power.rs"]
mod power;
#[cfg(any(windows, target_os = "linux"))]
#[path = "storage_health.rs"]
mod storage_health;

pub fn base_snapshot() -> Snapshot {
    let base = super::inventory();
    let text = |cat, key, labels, reading: Reading<String>| {
        let mut field = Field::text(cat, 0, key, labels, reading.value, reading.source);
        field.state = reading.state;
        field
    };
    let number = |cat, key, labels, reading: Reading<u64>, unit| {
        let mut field =
            Field::text(cat, 0, key, labels, None, reading.source).number(reading.value, unit);
        field.state = reading.state;
        field
    };
    let threads = Reading {
        value: base.logical_processors.value.map(|value| value as u64),
        source: base.logical_processors.source,
        state: base.logical_processors.state,
    };
    Snapshot {
        captured_at: Utc::now(),
        fields: vec![
            text(
                C::Cpu,
                "model",
                ("Processor", "Processador"),
                base.processor,
            ),
            number(
                C::Cpu,
                "logical_processors",
                ("Logical processors", "Processadores lógicos"),
                threads,
                "",
            ),
            number(
                C::Memory,
                "physical_memory",
                ("Physical memory", "Memória física"),
                base.memory_bytes,
                "bytes",
            ),
            text(
                C::System,
                "os",
                ("Operating system", "Sistema operacional"),
                base.operating_system,
            ),
            Field::text(
                C::System,
                0,
                "architecture",
                ("Architecture", "Arquitetura"),
                Some(base.architecture.into()),
                "Rust · target architecture",
            ),
        ],
    }
}

pub fn hardware_details() -> Vec<Field> {
    let mut fields = native::hardware_details();
    fields.extend(power::fields());
    #[cfg(any(windows, target_os = "linux"))]
    fields.extend(network::fields());
    #[cfg(windows)]
    fields.extend(displays::fields());
    fields
}

#[cfg(windows)]
#[path = "nvidia_windows.rs"]
mod nvidia;

#[cfg_attr(not(windows), derive(Default))]
pub struct NativeSensors {
    #[cfg(windows)]
    nvidia: Option<nvidia::Nvml>,
}

impl NativeSensors {
    pub fn sample(&self) -> Vec<Sample> {
        #[cfg(target_os = "macos")]
        let samples = unavailable_sensor("macOS", State::Unsupported);
        #[cfg(any(windows, target_os = "linux"))]
        let samples = native::sensors();
        #[cfg(windows)]
        if let Some(nvidia) = &self.nvidia {
            return samples.into_iter().chain(nvidia.sample()).collect();
        }
        samples
    }
}

#[cfg(windows)]
impl Default for NativeSensors {
    fn default() -> Self {
        Self {
            nvidia: nvidia::Nvml::open(),
        }
    }
}

pub fn memory_sample() -> Sample {
    #[cfg(target_os = "macos")]
    let source = native::MEMORY_LOAD_SOURCE;
    #[cfg(not(target_os = "macos"))]
    let source = super::platform::MEMORY_SOURCE;
    let reading = Reading::from_result(native::memory_load(), source);
    sample(
        "memory:load".into(),
        "Memory usage".into(),
        Some("Uso de memória"),
        "%",
        reading,
    )
}

fn sample(
    id: String,
    name: String,
    name_pt: Option<&'static str>,
    unit: &'static str,
    reading: Reading<f64>,
) -> Sample {
    Sample {
        id,
        name,
        name_pt: name_pt.map(Into::into),
        unit,
        source: reading.source,
        at: Utc::now(),
        seen: Instant::now(),
        state: reading.state,
        value: reading.value,
    }
}

fn unavailable_sensor(source: &'static str, state: State) -> Vec<Sample> {
    vec![sample(
        "native:status".into(),
        "Hardware sensors".into(),
        Some("Sensores de hardware"),
        "",
        Reading {
            value: None,
            state,
            source,
        },
    )]
}
