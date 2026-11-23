use std::time::{Duration, Instant};

use nucelvo_core::{Monitor, Sample, State};
use nucelvo_report::{sample_display, sensor_name};

use crate::LiveMetric;

pub fn current(sample: &Sample, paused: bool, pt: bool) -> String {
    if !paused && sample.seen.elapsed() > Duration::from_secs(12) {
        State::Stale.label(pt).into()
    } else {
        sample_display(sample, pt)
    }
}

pub fn group_matches(group: i32, sample: &Sample) -> bool {
    match group {
        1 => sample.id.starts_with("cpu:"),
        2 => sample.id.starts_with("memory:"),
        3 => sample.id.starts_with("gpu:"),
        4 => sample.unit == "°C",
        5 => sample.id.starts_with("network:"),
        _ => true,
    }
}

pub fn tone(sample: &Sample) -> i32 {
    if group_matches(5, sample) {
        if sample.id == "network:receive" { 4 } else { 5 }
    } else if group_matches(4, sample) {
        3
    } else if group_matches(3, sample) {
        2
    } else if group_matches(2, sample) {
        1
    } else {
        0
    }
}

pub fn icon(sample: &Sample) -> i32 {
    match sample.unit {
        "°C" => 10,
        "RPM" => 11,
        "W" => 12,
        "MiB" => 2,
        _ if sample.id == "network:receive" => 14,
        _ if sample.id == "network:send" => 15,
        _ if sample.id.ends_with(":fan") => 11,
        _ => 9,
    }
}

pub fn inline_matches(page: i32, sample: &Sample) -> bool {
    let chip = sample
        .id
        .strip_prefix("hwmon:")
        .and_then(|id| id.rsplit(':').nth(1));
    match page {
        0 => {
            matches!(sample.id.as_str(), "cpu:load" | "memory:load")
                || sample.id.starts_with("gpu:")
                || sample.unit == "°C"
        }
        1 => {
            sample.id.starts_with("cpu:") && sample.id != "cpu:logical:status"
                || sample.unit == "°C" && matches!(chip, Some("coretemp" | "k10temp" | "zenpower"))
        }
        2 => sample.id == "memory:load",
        4 => {
            sample.id.starts_with("gpu:")
                || matches!(chip, Some("amdgpu" | "nouveau" | "i915" | "xe"))
        }
        5 => matches!(chip, Some("nvme" | "drivetemp")) && sample.unit == "°C",
        6 => sample.id.starts_with("network:"),
        _ => false,
    }
}

pub fn metrics(monitor: &Monitor, page: i32, paused: bool, pt: bool) -> Vec<LiveMetric> {
    let latest = |id: &str| {
        monitor
            .series
            .get(id)
            .and_then(|series| series.points.back())
    };
    let item =
        |key: &str, (en, br): (&str, &str), tone, symbol, sample: Option<&Sample>| LiveMetric {
            title: (if pt { br } else { en }).into(),
            value: sample
                .map(|reading| current(reading, paused, pt))
                .unwrap_or_else(|| {
                    if key.is_empty() {
                        State::Unsupported.label(pt).into()
                    } else {
                        "—".into()
                    }
                })
                .into(),
            detail: sample
                .filter(|sample| sample.id.starts_with("gpu:") || sample.unit == "°C")
                .map(|sample| {
                    sensor_name(sample, pt)
                        .split_once(" · ")
                        .map_or(sensor_name(sample, pt), |(_, device)| device)
                })
                .unwrap_or_default()
                .into(),
            key: sample.map_or(key, |sample| sample.id.as_str()).into(),
            tone,
            glyph: sample.map_or(symbol, icon),
            available: sample.is_some_and(|reading| {
                reading.state == State::Ok
                    && (paused
                        || Instant::now().saturating_duration_since(reading.seen)
                            <= Duration::from_secs(12))
            }),
        };
    let cpu = || {
        item(
            "cpu:load",
            ("CPU usage", "Uso da CPU"),
            0,
            9,
            latest("cpu:load"),
        )
    };
    let ram = || {
        item(
            "memory:load",
            ("Memory usage", "Uso de memória"),
            1,
            9,
            latest("memory:load"),
        )
    };
    let gpu = |kind: &str| {
        monitor.series.iter().find_map(|(id, series)| {
            (id.starts_with("gpu:") && id.rsplit(':').next() == Some(kind))
                .then(|| series.points.back())
                .flatten()
        })
    };
    let gpu_load = || item("", ("GPU usage", "Uso da GPU"), 2, 9, gpu("load"));
    let gpu_temp = || {
        item(
            "",
            ("GPU temperature", "Temperatura da GPU"),
            3,
            10,
            gpu("temperature"),
        )
    };
    match page {
        0 => {
            let temperature = if gpu("temperature").is_some_and(|sample| sample.state == State::Ok)
            {
                gpu_temp()
            } else {
                let reading = monitor
                    .series
                    .values()
                    .filter_map(|series| series.points.back())
                    .find(|sample| sample.unit == "°C" && sample.state == State::Ok);
                item("", ("Temperature", "Temperatura"), 3, 10, reading)
            };
            vec![cpu(), ram(), gpu_load(), temperature]
        }
        1 => vec![cpu()],
        2 => vec![ram()],
        4 if gpu("load").is_some() => vec![
            item("", ("Usage", "Uso"), 2, 9, gpu("load")),
            item(
                "",
                ("Temperature", "Temperatura"),
                3,
                10,
                gpu("temperature"),
            ),
            item("", ("VRAM used", "VRAM em uso"), 2, 2, gpu("vram")),
            item("", ("Fan speed", "Ventoinha"), 2, 11, gpu("fan")),
            item("", ("Power", "Potência"), 2, 12, gpu("power")),
        ],
        6 => ["network:receive", "network:send"]
            .into_iter()
            .filter_map(latest)
            .map(|sample| {
                item(
                    &sample.id,
                    (&sample.name, sensor_name(sample, true)),
                    tone(sample),
                    icon(sample),
                    Some(sample),
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}
