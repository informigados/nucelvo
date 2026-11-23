use std::{
    io::{self, ErrorKind},
    time::{SystemTime, UNIX_EPOCH},
};

use nucelvo_core::{CpuSample, Inventory, Reading};
mod details;
#[cfg(any(windows, target_os = "linux"))]
pub use details::NetworkSampler;
pub use details::{NativeSensors, base_snapshot, hardware_details, memory_sample};
#[cfg(windows)]
mod logical_windows;

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod platform;

#[derive(Clone, Copy)]
struct CpuTicks {
    busy: u64,
    idle: u64,
}

pub fn inventory() -> Inventory {
    Inventory {
        processor: Reading::from_result(platform::processor(), platform::CPU_SOURCE),
        logical_processors: Reading::from_result(
            std::thread::available_parallelism().map(|count| count.get()),
            "std::thread::available_parallelism",
        ),
        memory_bytes: Reading::from_result(platform::memory_bytes(), platform::MEMORY_SOURCE),
        operating_system: Reading::from_result(platform::operating_system(), platform::OS_SOURCE),
        architecture: std::env::consts::ARCH,
    }
}

#[derive(Default)]
pub struct CpuSampler {
    previous: Option<CpuTicks>,
    #[cfg(windows)]
    logical: logical_windows::LogicalSampler,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    previous_logical: std::collections::BTreeMap<u32, CpuTicks>,
}

impl CpuSampler {
    #[cfg(windows)]
    pub fn logical_samples(&mut self) -> Vec<nucelvo_core::Sample> {
        self.logical.sample()
    }

    pub fn sample(&mut self) -> CpuSample {
        let result = match platform::cpu_ticks() {
            Ok(current) => self.previous.replace(current).map_or_else(
                || Err(io::Error::from(ErrorKind::WouldBlock)),
                |previous| utilization(previous, current),
            ),
            Err(error) => {
                self.previous = None;
                Err(error)
            }
        };
        CpuSample {
            device_id: "cpu",
            metric_id: "utilization_total",
            unit: "percent",
            sampled_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            reading: Reading::from_result(result, platform::USAGE_SOURCE),
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn logical_samples(&mut self) -> Vec<nucelvo_core::Sample> {
        let previous = std::mem::take(&mut self.previous_logical);
        let emit = |logical, reading: Reading<f64>| nucelvo_core::Sample {
            seen: std::time::Instant::now(),
            at: chrono::Utc::now(),
            id: format!("cpu:logical:{logical}"),
            name: format!("CPU {logical}"),
            name_pt: None,
            source: reading.source,
            unit: "%",
            state: reading.state,
            value: reading.value,
        };
        let rows = match platform::logical_cpu_ticks() {
            Ok(rows) => rows,
            Err(error) => {
                let state = Reading::<()>::from_result(Err(error), platform::USAGE_SOURCE).state;
                return previous
                    .keys()
                    .map(|logical| {
                        emit(
                            *logical,
                            Reading {
                                source: platform::USAGE_SOURCE,
                                state,
                                value: None,
                            },
                        )
                    })
                    .collect();
            }
        };
        let mut samples = Vec::with_capacity(rows.len());
        for (logical, ticks) in rows {
            let reading = previous.get(&logical).map_or_else(
                || Err(ErrorKind::WouldBlock.into()),
                |previous| utilization(*previous, ticks),
            );
            self.previous_logical.insert(logical, ticks);
            samples.push(emit(
                logical,
                Reading::from_result(reading, platform::USAGE_SOURCE),
            ));
        }
        for logical in previous
            .keys()
            .filter(|logical| !self.previous_logical.contains_key(logical))
        {
            samples.push(emit(
                *logical,
                Reading {
                    source: platform::USAGE_SOURCE,
                    state: nucelvo_core::State::Stale,
                    value: None,
                },
            ));
        }
        samples
    }
}

fn utilization(before: CpuTicks, after: CpuTicks) -> io::Result<f64> {
    let busy = after.busy.checked_sub(before.busy);
    let idle = after.idle.checked_sub(before.idle);
    if let (Some(busy), Some(idle)) = (busy, idle) {
        let total = busy as f64 + idle as f64;
        if total > 0.0 {
            return Ok(100.0 * busy as f64 / total);
        }
    }
    Err(io::Error::from(ErrorKind::WouldBlock))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_usage_handles_idle_and_counter_resets() {
        let before = CpuTicks { busy: 10, idle: 20 };
        let idle = CpuTicks { busy: 10, idle: 30 };
        let mixed = CpuTicks { busy: 20, idle: 30 };
        assert_eq!(utilization(before, idle).unwrap(), 0.0);
        assert_eq!(utilization(before, mixed).unwrap(), 50.0);
        assert!(utilization(before, before).is_err());
        assert!(utilization(mixed, before).is_err());
    }
}
