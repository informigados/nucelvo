use chrono::{DateTime, SecondsFormat, Utc};
use nucelvo_core::{Category, Field, Monitor, Sample, Snapshot, Value};
use serde::Serialize;

const HTML_STYLE: &str = r#"
    :root { color-scheme: light dark; accent-color: #087d68; }
    body { font: 1rem/1.5 system-ui, sans-serif; background: Canvas; color: CanvasText; }
    main { box-sizing: border-box; max-width: 70rem; padding: 0 1.25rem; margin: 2rem auto; }
    h1 { margin: 0 0 .4rem; font-size: clamp(1.6rem, 4vw, 2.4rem); }
    p { margin: .35rem 0; }
    table { width: 100%; border-collapse: collapse; margin-top: 1.5rem; overflow-wrap: anywhere; }
    th, td { border-bottom: 1px solid GrayText; padding: .7rem .55rem; text-align: left; }
    thead { border-top: 3px solid #087d68; }
    tbody th { font-weight: 600; }
    tbody tr:hover { background: #087d681a; }
"#;
use std::{
    io::{self, Write},
    path::Path,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub enum Format {
    Json,
    Text,
    Csv,
    Html,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Text => "txt",
            Self::Csv => "csv",
            Self::Html => "html",
        }
    }
}

#[derive(Clone, Copy)]
pub enum Scope {
    Inventory(Option<Category>),
    Sensors(Option<Duration>),
}

#[derive(Serialize)]
struct SensorRow {
    sensor: usize,
    #[serde(flatten)]
    sample: Sample,
}

#[derive(Serialize)]
pub struct Report {
    schema_version: u32,
    app_version: &'static str,
    captured_at: DateTime<Utc>,
    inventory_at: Option<DateTime<Utc>>,
    scope: &'static str,
    share_safe: bool,
    fields: Vec<Field>,
    samples: Vec<SensorRow>,
    #[serde(skip)]
    pt: bool,
}

impl Report {
    pub fn capture(
        snapshot: Option<&Snapshot>,
        monitor: &Monitor,
        scope: Scope,
        share_safe: bool,
        pt: bool,
    ) -> Self {
        let captured_at = Utc::now();
        let mut report = Self {
            schema_version: 1,
            app_version: env!("CARGO_PKG_VERSION"),
            captured_at,
            inventory_at: None,
            scope: "inventory",
            share_safe,
            fields: Vec::new(),
            samples: Vec::new(),
            pt,
        };
        match scope {
            Scope::Inventory(category) => {
                if let Some(snapshot) = snapshot {
                    report.inventory_at = Some(snapshot.captured_at);
                    report.fields.extend(
                        snapshot
                            .fields
                            .iter()
                            .filter(|field| {
                                category.is_none_or(|category| field.category == category)
                                    && (!share_safe || field.share_safe())
                            })
                            .cloned(),
                    );
                }
            }
            Scope::Sensors(interval) => {
                let now = Instant::now();
                report.scope = if interval.is_some() {
                    "sensor_history"
                } else {
                    "sensor_snapshot"
                };
                for (sensor, series) in monitor.series.values().enumerate() {
                    if let Some(interval) = interval {
                        report.samples.extend(
                            series
                                .points
                                .iter()
                                .filter(|sample| {
                                    now.saturating_duration_since(sample.seen) <= interval
                                })
                                .map(|sample| SensorRow {
                                    sensor,
                                    sample: sample.clone(),
                                }),
                        );
                    } else if let Some(sample) = series.points.back() {
                        report.samples.push(SensorRow {
                            sensor,
                            sample: sample.clone(),
                        });
                    }
                }
                report
                    .samples
                    .sort_by_key(|row| (row.sample.seen, row.sensor));
            }
        }
        report
    }

    pub fn len(&self) -> usize {
        self.fields.len() + self.samples.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn preview(&self, offset: usize, limit: usize) -> Vec<(String, String)> {
        let fields = self.fields.iter().skip(offset).take(limit);
        let samples = self
            .samples
            .iter()
            .skip(offset.saturating_sub(self.fields.len()))
            .take(limit.saturating_sub(fields.len()));
        fields
            .map(|field| {
                (
                    format!("{} · {}", field.group(self.pt), field.label(self.pt)),
                    field_display(field, self.pt),
                )
            })
            .chain(samples.map(|row| {
                (
                    format!(
                        "{} · {}",
                        preview_time(row.sample.at, self.pt),
                        sensor_name(&row.sample, self.pt)
                    ),
                    sample_display(&row.sample, self.pt),
                )
            }))
            .collect()
    }

    pub fn save(&self, path: &Path, format: Format) -> io::Result<()> {
        let directory = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        self.write(file.as_file_mut(), format)?;
        file.as_file().sync_all()?;
        file.persist_noclobber(path)?;
        Ok(())
    }

    fn write(&self, mut output: impl Write, format: Format) -> io::Result<()> {
        match format {
            Format::Json => serde_json::to_writer_pretty(output, self).map_err(io::Error::other),
            Format::Csv => self.csv(output),
            Format::Text | Format::Html => {
                let title = if self.pt {
                    "Relatório Nucelvo"
                } else {
                    "Nucelvo report"
                };
                let captured = if self.pt {
                    "Gerado em (UTC)"
                } else {
                    "Generated (UTC)"
                };
                let inventory_time = self.inventory_at.map(|time| {
                    let label = if self.pt {
                        "Inventário capturado em (UTC)"
                    } else {
                        "Inventory captured (UTC)"
                    };
                    format!("{label} {}", timestamp(time))
                });
                let state = if self.pt { "Estado" } else { "State" };
                let source = if self.pt { "Fonte" } else { "Source" };
                let privacy = match (self.pt, self.share_safe) {
                    (true, true) => "Compartilhamento seguro",
                    (true, false) => "Inclui identificadores",
                    (false, true) => "Share-safe",
                    (false, false) => "Includes identifiers",
                };
                let html = matches!(format, Format::Html);
                if html {
                    let language = if self.pt { "pt-BR" } else { "en-US" };
                    let field = if self.pt { "Campo" } else { "Field" };
                    let value = if self.pt { "Valor" } else { "Value" };
                    writeln!(
                        output,
                        r#"<!doctype html>
<html lang="{language}">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <style>{HTML_STYLE}</style>
</head>
<body><main>
  <h1>{title}</h1>
  <p>{captured} {} · {privacy}</p>"#,
                        timestamp(self.captured_at)
                    )?;
                    if let Some(time) = &inventory_time {
                        writeln!(output, "  <p>{time}</p>")?;
                    }
                    writeln!(
                        output,
                        r#"  <table>
    <thead><tr><th scope="col">{field}</th><th scope="col">{value}</th><th scope="col">{state}</th><th scope="col">{source}</th></tr></thead>
    <tbody>"#
                    )?;
                } else {
                    writeln!(
                        output,
                        "{title}\n{captured} {} · {privacy}",
                        timestamp(self.captured_at)
                    )?;
                    if let Some(time) = &inventory_time {
                        writeln!(output, "{time}")?;
                    }
                    writeln!(output)?;
                }
                let mut write_row = |label: &str, value: &str, status: &str, source: &str| {
                    if html {
                        writeln!(
                            output,
                            "<tr><th scope=\"row\">{}</th><td>{}</td><td>{}</td><td>{}</td></tr>",
                            escape(label),
                            escape(value),
                            escape(status),
                            escape(source)
                        )
                    } else {
                        writeln!(
                            output,
                            "{}: {} [{} · {}]",
                            one_line(label),
                            one_line(value),
                            status,
                            one_line(source)
                        )
                    }
                };
                for field in &self.fields {
                    write_row(
                        &format!("{} · {}", field.group(self.pt), field.label(self.pt)),
                        &field_display(field, self.pt),
                        field.state.label(self.pt),
                        source_label(field.source, self.pt),
                    )?;
                }
                for row in &self.samples {
                    let sample = &row.sample;
                    write_row(
                        &format!(
                            "{} · {}",
                            timestamp(sample.at),
                            sensor_name(sample, self.pt)
                        ),
                        &sample_display(sample, self.pt),
                        sample.state.label(self.pt),
                        source_label(sample.source, self.pt),
                    )?;
                }
                if html {
                    writeln!(output, "    </tbody>\n  </table>\n</main></body></html>")?;
                }
                Ok(())
            }
        }
    }

    fn csv(&self, output: impl Write) -> io::Result<()> {
        let mut writer = csv::Writer::from_writer(output);
        if self.scope == "inventory" {
            writer.write_record([
                "snapshot_at",
                "category",
                "device",
                "field",
                "value",
                "unit",
                "state",
                "source",
            ])?;
            for field in &self.fields {
                writer.write_record([
                    timestamp(self.inventory_at.unwrap_or(self.captured_at)),
                    code(field.category),
                    field.device.to_string(),
                    field.key.into(),
                    match &field.value {
                        Some(Value::Text(value)) => spreadsheet(value),
                        Some(Value::Number(value)) => value.to_string(),
                        None => String::new(),
                    },
                    field.unit.into(),
                    code(field.state),
                    spreadsheet(field.source),
                ])?;
            }
        } else {
            writer.write_record([
                "sampled_at",
                "sensor",
                "name",
                "value",
                "unit",
                "state",
                "source",
            ])?;
            for row in &self.samples {
                let sample = &row.sample;
                writer.write_record([
                    timestamp(sample.at),
                    row.sensor.to_string(),
                    spreadsheet(&sample.name),
                    sample
                        .value
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    sample.unit.into(),
                    code(sample.state),
                    spreadsheet(sample.source),
                ])?;
            }
        }
        writer.flush()
    }
}

pub fn field_display(field: &Field, pt: bool) -> String {
    match &field.value {
        Some(Value::Text(value)) if pt && field.key == "cache" => one_line(value)
            .replace(" · Unified · ", " · Unificado · ")
            .replace(" · Data · ", " · Dados · ")
            .replace(" · Instruction · ", " · Instruções · "),
        Some(Value::Text(value)) => one_line(value),
        Some(Value::Number(value)) if field.key == "memory_type" => match *value {
            0x12 => "DDR".into(),
            0x13 => "DDR2".into(),
            0x18 => "DDR3".into(),
            0x1a => "DDR4".into(),
            0x1b => "LPDDR".into(),
            0x1c => "LPDDR2".into(),
            0x1d => "LPDDR3".into(),
            0x1e => "LPDDR4".into(),
            0x22 => "DDR5".into(),
            0x23 => "LPDDR5".into(),
            _ if pt => "Tipo desconhecido".into(),
            _ => "Unknown type".into(),
        },
        Some(Value::Number(value)) if field.unit == "bytes" => {
            let (divisor, unit) = match *value {
                value if value >= 1 << 30 => (1 << 30, "GiB"),
                value if value >= 1 << 20 => (1 << 20, "MiB"),
                value if value >= 1 << 10 => (1 << 10, "KiB"),
                value => return format!("{value} B"),
            };
            format!("{} {unit}", decimal(*value as f64 / divisor as f64, pt))
        }
        Some(Value::Number(value)) if field.unit == "Hz" => {
            format!("{} MHz", decimal(*value as f64 / 1_000_000.0, pt))
        }
        Some(Value::Number(value)) if field.unit == "s" => format!(
            "{:02}:{:02}:{:02}",
            value / 3600,
            value / 60 % 60,
            value % 60
        ),
        Some(Value::Number(value)) if field.unit.is_empty() => value.to_string(),
        Some(Value::Number(value)) => format!("{value} {}", field.unit),
        None => field.state.label(pt).into(),
    }
}

pub fn source_label(source: &str, pt: bool) -> &'static str {
    let labels = match source {
        "Win32 · GetSystemTimes" | "Win32 PDH · % Processor Time" => {
            ("Windows CPU counters", "Contadores de CPU do Windows")
        }
        "Win32 · GlobalMemoryStatusEx" => (
            "Windows memory statistics",
            "Estatísticas de memória do Windows",
        ),
        "WMI · MSAcpi_ThermalZoneTemperature" => (
            "Windows ACPI thermal sensors",
            "Sensores térmicos ACPI do Windows",
        ),
        "DXGI · IDXGIAdapter1" => (
            "Windows graphics information",
            "Informações gráficas do Windows",
        ),
        "/proc/stat" => ("Linux CPU counters", "Contadores de CPU do Linux"),
        "/proc/meminfo" => (
            "Linux memory statistics",
            "Estatísticas de memória do Linux",
        ),
        "Linux · hwmon" => ("Linux hardware sensors", "Sensores de hardware do Linux"),
        "sysfs · DMI" | "sysfs · SMBIOS" => ("Firmware information", "Informações do firmware"),
        "/proc/self/mountinfo · statvfs" | "sysfs · block" => (
            "Linux storage information",
            "Informações de armazenamento do Linux",
        ),
        "sysfs · DRM / PCI" | "PCI IDs · local database" => (
            "Linux graphics information",
            "Informações gráficas do Linux",
        ),
        value
            if value.starts_with("Win32_")
                || value.starts_with("Windows Registry")
                || value.starts_with("Win32 · ") =>
        {
            (
                "Windows system information",
                "Informações do sistema do Windows",
            )
        }
        value
            if value.starts_with("/proc/")
                || value.starts_with("os-release")
                || value.starts_with("sysfs") =>
        {
            ("Linux system information", "Informações do sistema Linux")
        }
        _ => ("System information", "Informações do sistema"),
    };
    if pt { labels.1 } else { labels.0 }
}

pub fn sensor_name(sample: &Sample, pt: bool) -> &str {
    if pt {
        sample.name_pt.unwrap_or(&sample.name)
    } else {
        &sample.name
    }
}

pub fn sample_display(sample: &Sample, pt: bool) -> String {
    sample
        .value
        .map(|value| sensor_value(value, sample.unit, pt))
        .unwrap_or_else(|| sample.state.label(pt).into())
}

pub fn sensor_value(value: f64, unit: &str, pt: bool) -> String {
    let separator = if unit == "%" || unit.is_empty() {
        ""
    } else {
        " "
    };
    format!("{}{separator}{unit}", decimal(value, pt))
}

fn decimal(value: f64, pt: bool) -> String {
    let value = format!("{value:.1}");
    if pt { value.replace('.', ",") } else { value }
}

fn timestamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn preview_time(time: DateTime<Utc>, pt: bool) -> String {
    let layout = if pt {
        "%d/%m/%Y %H:%M:%S UTC"
    } else {
        "%m/%d/%Y %H:%M:%S UTC"
    };
    time.format(layout).to_string()
}

fn code(value: impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(value)) => value,
        _ => String::new(),
    }
}

fn spreadsheet(value: &str) -> String {
    let first = value
        .trim_start_matches(|character: char| character.is_whitespace() || character.is_control())
        .chars()
        .next();
    if matches!(
        first,
        Some('=' | '+' | '-' | '@' | '＝' | '＋' | '－' | '＠')
    ) || value.starts_with(['\r', '\n', '\t'])
    {
        format!("'{value}")
    } else {
        value.into()
    }
}

fn one_line(value: &str) -> String {
    value
        .chars()
        .map(|char| if char.is_control() { ' ' } else { char })
        .collect()
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn privacy_is_applied_before_every_format_and_html_csv_are_safe() {
        let mut private = Field::text(
            Category::Storage,
            0,
            "serial",
            ("Serial", "Série"),
            Some("secret-serial".into()),
            "test",
        );
        private.private = true;
        let public = Field::text(
            Category::Storage,
            0,
            "model",
            ("Model", "Modelo"),
            Some(" =<script>".into()),
            "Win32_DiskDrive",
        );
        let snapshot = Snapshot {
            captured_at: Utc::now(),
            fields: vec![private, public],
        };
        let monitor = Monitor::default();
        let safe = Report::capture(
            Some(&snapshot),
            &monitor,
            Scope::Inventory(None),
            true,
            false,
        );
        assert_eq!(safe.preview(0, 100).len(), 1);
        for format in [Format::Json, Format::Text, Format::Csv, Format::Html] {
            let mut bytes = Vec::new();
            safe.write(&mut bytes, format).unwrap();
            let result = String::from_utf8(bytes).unwrap();
            assert!(!result.contains("secret-serial"));
            if matches!(format, Format::Json | Format::Csv) {
                assert!(result.contains("Win32_DiskDrive"));
            } else {
                assert!(result.contains("Windows system information"));
                assert!(!result.contains("Win32_DiskDrive"));
            }
            if matches!(format, Format::Html) {
                assert!(result.contains("&lt;script&gt;"));
            }
            if matches!(format, Format::Csv) {
                assert!(result.contains("' =<script>"));
            }
        }
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("report.json");
        safe.save(&path, Format::Json).unwrap();
        let full = Report::capture(
            Some(&snapshot),
            &monitor,
            Scope::Inventory(None),
            false,
            false,
        );
        assert_eq!(full.preview(0, 100).len(), 2);
        assert_eq!(
            full.save(&path, Format::Json).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
    }
}
