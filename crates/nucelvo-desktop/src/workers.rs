use chrono::Utc;
use nucelvo_core::{Field, Sample, Snapshot};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Updates {
    pub base: Option<Snapshot>,
    pub details: Option<Vec<Field>>,
    pub fast: Option<[Sample; 2]>,
    pub logical: Option<Vec<Sample>>,
    pub network: Option<[Sample; 2]>,
    pub native: Option<Vec<Sample>>,
}

#[derive(Default)]
pub struct Control {
    pub paused: AtomicBool,
    pub stopping: AtomicBool,
}

pub fn fast(updates: Arc<Mutex<Updates>>, control: Arc<Control>) {
    let mut sampler = nucelvo_collect::CpuSampler::default();
    #[cfg(any(windows, target_os = "linux"))]
    let mut network_sampler = nucelvo_collect::NetworkSampler::default();
    while !control.stopping.load(Ordering::Relaxed) {
        if control.paused.load(Ordering::Relaxed) {
            sampler = nucelvo_collect::CpuSampler::default();
            #[cfg(any(windows, target_os = "linux"))]
            {
                network_sampler = nucelvo_collect::NetworkSampler::default();
            }
        } else {
            let raw = sampler.sample();
            let cpu = Sample {
                id: "cpu:load".into(),
                name: "CPU usage".into(),
                name_pt: Some("Uso da CPU".into()),
                at: Utc::now(),
                seen: Instant::now(),
                unit: "%",
                source: raw.reading.source,
                value: raw.reading.value,
                state: raw.reading.state,
            };
            let memory = nucelvo_collect::memory_sample();
            let logical = sampler.logical_samples();
            #[cfg(any(windows, target_os = "linux"))]
            let network = network_sampler.sample();
            let mut pending = updates.lock().unwrap();
            #[cfg(any(windows, target_os = "linux"))]
            {
                pending.network = Some(network);
            }
            pending.fast = Some([cpu, memory]);
            pending.logical = Some(logical);
        }
        thread::park_timeout(Duration::from_secs(1));
    }
}

pub fn hardware(updates: Arc<Mutex<Updates>>, control: Arc<Control>, requests: mpsc::Receiver<()>) {
    let sensors = nucelvo_collect::NativeSensors::default();
    let mut refresh = true;
    let mut next_sensors = Instant::now();
    while !control.stopping.load(Ordering::Relaxed) {
        if refresh {
            let base = nucelvo_collect::base_snapshot();
            updates.lock().unwrap().base = Some(base);
            let detail = nucelvo_collect::hardware_details();
            updates.lock().unwrap().details = Some(detail);
        }
        if Instant::now() >= next_sensors {
            if !control.paused.load(Ordering::Relaxed) {
                let native = sensors.sample();
                updates.lock().unwrap().native = Some(native);
            }
            next_sensors = Instant::now() + Duration::from_secs(5);
        }
        match requests.recv_timeout(next_sensors.saturating_duration_since(Instant::now())) {
            Ok(()) => refresh = true,
            Err(mpsc::RecvTimeoutError::Timeout) => refresh = false,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}
