use crate::State;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::{
    collections::{BTreeMap, VecDeque},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Cpu,
    Memory,
    Board,
    Graphics,
    Storage,
    System,
}

impl Category {
    pub const ALL: [Self; 6] = [
        Self::Cpu,
        Self::Memory,
        Self::Board,
        Self::Graphics,
        Self::Storage,
        Self::System,
    ];
    pub fn label(self, pt: bool) -> &'static str {
        let labels = match self {
            Self::Cpu => ("CPU", "CPU"),
            Self::Memory => ("Memory", "Memória"),
            Self::Board => ("Motherboard", "Placa-mãe"),
            Self::Graphics => ("Graphics", "Vídeo"),
            Self::Storage => ("Storage", "Armazenamento"),
            Self::System => ("System", "Sistema"),
        };
        if pt { labels.1 } else { labels.0 }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum Value {
    Text(String),
    Number(u64),
}

#[derive(Clone, Debug, Serialize)]
pub struct Field {
    pub category: Category,
    pub device: usize,
    pub key: &'static str,
    #[serde(skip)]
    pub labels: (&'static str, &'static str),
    pub value: Option<Value>,
    pub unit: &'static str,
    pub source: &'static str,
    pub state: State,
    #[serde(skip)]
    pub private: bool,
}

impl Field {
    pub fn text(
        category: Category,
        device: usize,
        key: &'static str,
        labels: (&'static str, &'static str),
        value: Option<String>,
        source: &'static str,
    ) -> Self {
        let value = value
            .filter(|text| !text.trim().is_empty())
            .map(Value::Text);
        let state = if value.is_some() {
            State::Ok
        } else {
            State::Unsupported
        };
        Self {
            category,
            device,
            key,
            labels,
            value,
            unit: "",
            source,
            state,
            private: false,
        }
    }

    pub fn number(mut self, value: Option<u64>, unit: &'static str) -> Self {
        self.value = value.map(Value::Number);
        self.state = if value.is_some() {
            State::Ok
        } else {
            State::Unsupported
        };
        self.unit = unit;
        self
    }

    pub fn label(&self, pt: bool) -> &'static str {
        if pt { self.labels.1 } else { self.labels.0 }
    }

    pub fn share_safe(&self) -> bool {
        !self.private
            && !matches!(
                self.key,
                "serial"
                    | "uuid"
                    | "mac"
                    | "ip"
                    | "hostname"
                    | "username"
                    | "ssid"
                    | "volume_name"
                    | "mount_point"
                    | "path"
            )
    }

    pub fn group(&self, pt: bool) -> String {
        let label = self.category.label(pt);
        let (device, number) = match self.category {
            Category::Storage if self.device >= 10_000 => ("Volume", self.device - 9_999),
            Category::Storage => (if pt { "Unidade" } else { "Drive" }, self.device + 1),
            Category::Graphics => (if pt { "Adaptador" } else { "Adapter" }, self.device + 1),
            Category::Memory if self.device > 0 => {
                (if pt { "Módulo" } else { "Module" }, self.device)
            }
            _ => return label.to_owned(),
        };
        format!("{label} · {device} {number}")
    }
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub captured_at: DateTime<Utc>,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Sample {
    #[serde(skip)]
    pub seen: Instant,
    pub at: DateTime<Utc>,
    #[serde(skip)]
    pub id: String,
    pub name: String,
    #[serde(skip)]
    pub name_pt: Option<std::borrow::Cow<'static, str>>,
    pub unit: &'static str,
    pub source: &'static str,
    pub state: State,
    pub value: Option<f64>,
}

#[derive(Default)]
pub struct Series {
    pub points: VecDeque<Sample>,
    pub count: u64,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
}

pub const HISTORY: Duration = Duration::from_secs(900);
const MAX_SERIES: usize = 128;
const MAX_POINTS: usize = 901;

#[derive(Default)]
pub struct Monitor {
    pub series: BTreeMap<String, Series>,
}

impl Monitor {
    pub fn record(&mut self, mut sample: Sample) {
        if !self.series.contains_key(&sample.id) && self.series.len() >= MAX_SERIES {
            return;
        }
        sample.value = sample
            .value
            .filter(|value| sample.state == State::Ok && value.is_finite());
        let series = self.series.entry(sample.id.clone()).or_default();
        if let Some(value) = sample.value {
            if series.count == 0 {
                series.min = value;
                series.max = value;
            }
            series.count += 1;
            series.min = series.min.min(value);
            series.max = series.max.max(value);
            series.mean += (value - series.mean) / series.count as f64;
        }
        if series.points.len() == MAX_POINTS {
            series.points.pop_front();
        }
        let now = sample.seen;
        series.points.push_back(sample);
        self.expire(now);
    }

    pub fn expire(&mut self, now: Instant) {
        self.series.retain(|_, series| {
            while series
                .points
                .front()
                .is_some_and(|sample| now.saturating_duration_since(sample.seen) > HISTORY)
            {
                series.points.pop_front();
            }
            !series.points.is_empty()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn half_hour_keeps_only_last_fifteen_minutes_and_skips_gaps() {
        let base = Instant::now();
        let mut monitor = Monitor::default();
        for second in 0..=1800 {
            monitor.record(Sample {
                seen: base + Duration::from_secs(second),
                at: Utc::now(),
                id: "x".into(),
                name: "X".into(),
                name_pt: None,
                unit: "%",
                source: "test",
                state: if second % 2 == 0 {
                    State::Ok
                } else {
                    State::Stale
                },
                value: Some(if second % 2 == 0 { 20.0 } else { 99.0 }),
            });
        }
        let series = &monitor.series["x"];
        assert_eq!(series.points.len(), 901);
        assert_eq!(series.count, 901);
        assert_eq!((series.min, series.max, series.mean), (20.0, 20.0, 20.0));
        assert!(series.points[1].value.is_none());
    }
}
