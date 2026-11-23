use nucelvo_core::Series;
use nucelvo_report::sensor_value;
use std::{
    fmt::Write,
    time::{Duration, Instant},
};

pub struct Plot {
    pub path: String,
    pub area: String,
    pub low: String,
    pub high: String,
}

pub fn plot(series: &Series, now: Instant, range: Duration, pt: bool) -> Plot {
    let mut result = Plot {
        path: String::new(),
        area: String::new(),
        low: String::new(),
        high: String::new(),
    };
    let Some(last) = series.points.back() else {
        return result;
    };
    let recent = series
        .points
        .iter()
        .filter(|point| now.saturating_duration_since(point.seen) <= range)
        .collect::<Vec<_>>();
    let span = recent.iter().filter_map(|sample| sample.value).fold(
        None,
        |span: Option<(f64, f64)>, value| {
            Some(span.map_or((value, value), |(min, max)| {
                (min.min(value), max.max(value))
            }))
        },
    );
    let Some((minimum, maximum)) = span else {
        return result;
    };
    let (low, high) = if last.unit == "%" {
        (0.0, maximum.max(100.0))
    } else {
        let pad = ((maximum - minimum) * 0.12).max(0.1);
        (minimum - pad, maximum + pad)
    };
    result.low = sensor_value(low, last.unit, pt);
    result.high = sensor_value(high, last.unit, pt);
    let cadence = if last.id.starts_with("cpu:") || last.id.starts_with("memory:") {
        1
    } else {
        5
    };
    let gap = Duration::from_millis(cadence * 1800);
    let stride = recent.len().div_ceil(250).max(1);
    let mut previous: Option<Instant> = None;
    let mut draw_line = false;
    let mut edge = None;
    for (index, sample) in recent.iter().enumerate() {
        let Some(value) = sample.value else {
            close_area(&mut result.area, edge.take());
            previous = None;
            draw_line = false;
            continue;
        };
        if previous.is_some_and(|seen| sample.seen.saturating_duration_since(seen) > gap) {
            close_area(&mut result.area, edge.take());
            draw_line = false;
        }
        previous = Some(sample.seen);
        if draw_line && index % stride != 0 && index + 1 < recent.len() {
            continue;
        }
        let age = now.saturating_duration_since(sample.seen).as_secs_f64();
        let x = 998.0 - 996.0 * age / range.as_secs_f64();
        let y = (98.0 - 96.0 * (value - low) / (high - low)).clamp(2.0, 98.0);
        if draw_line {
            let _ = write!(result.path, " L{x:.1},{y:.1}");
            let _ = write!(result.area, " L{x:.1},{y:.1}");
        } else {
            let _ = write!(result.path, " M{x:.1},{y:.1} h0.3");
            let _ = write!(result.area, " M{x:.1},100 L{x:.1},{y:.1}");
        }
        edge = Some(x);
        draw_line = true;
    }
    close_area(&mut result.area, edge);
    result
}

fn close_area(area: &mut String, edge: Option<f64>) {
    if let Some(edge) = edge {
        let _ = write!(area, " L{edge:.1},100 Z");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nucelvo_core::{Monitor, Sample, State};

    #[test]
    fn graph_breaks_lines_at_missing_values_and_delayed_samples() {
        let start = Instant::now();
        let mut monitor = Monitor::default();
        for (second, value) in [
            (0, Some(0.0)),
            (1, None),
            (2, Some(100.0)),
            (7, Some(50.0)),
            (8, Some(25.0)),
        ] {
            monitor.record(Sample {
                id: "cpu:load".into(),
                name: "CPU".into(),
                name_pt: None,
                unit: "%",
                source: "test",
                seen: start + Duration::from_secs(second),
                at: chrono::Utc::now(),
                state: if value.is_some() {
                    State::Ok
                } else {
                    State::TemporarilyUnavailable
                },
                value,
            });
        }
        let plot = plot(
            &monitor.series["cpu:load"],
            start + Duration::from_secs(8),
            Duration::from_secs(60),
            false,
        );
        assert_eq!(plot.path.matches('M').count(), 3);
        assert_eq!(plot.path.matches('L').count(), 1);
        assert_eq!(plot.area.matches('Z').count(), 3);
        assert_eq!((plot.low.as_str(), plot.high.as_str()), ("0.0%", "100.0%"));
    }
}
