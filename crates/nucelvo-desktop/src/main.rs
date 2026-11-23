#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod graph;
mod live;
mod preferences;
#[cfg(windows)]
mod window_windows;
mod workers;

use std::{
    cell::{Cell, RefCell},
    io,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex, atomic::Ordering, mpsc},
    thread,
    time::{Duration, Instant},
};

use chrono::{Local, Utc};
use nucelvo_core::{Category, Monitor, Snapshot, State};
use nucelvo_report::{
    Format, Report, Scope, field_display, sensor_name, sensor_value, source_label,
};
use preferences::Language;
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};

slint::include_modules!();

const SENSOR_PAGE: i32 = 7;
const EXPORT_PAGE: i32 = 8;
const PAGE_SIZE: usize = 100;
const RANGES: [u64; 3] = [60, 300, 900];

#[derive(Default)]
struct App {
    snapshot: Option<Snapshot>,
    pending_base: Option<Snapshot>,
    monitor: Monitor,
    inventory_query: String,
    metric_query: String,
    selected: String,
    metric_keys: Vec<String>,
    live_metrics: Rc<VecModel<LiveMetric>>,
    clipboard: Option<arboard::Clipboard>,
    report: Option<(Arc<Report>, Format)>,
    report_folder: Arc<Mutex<Option<PathBuf>>>,
}

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

fn category(page: i32) -> Option<Category> {
    usize::try_from(page - 1)
        .ok()
        .and_then(|index| Category::ALL.get(index).copied())
}

fn inventory(app: &App, window: &MainWindow) {
    let pt = window.get_portuguese();
    let selected = category(window.get_page());
    let query = app.inventory_query.trim().to_lowercase();
    let mut rows: Vec<Row> = app
        .snapshot
        .iter()
        .flat_map(|snapshot| snapshot.fields.iter())
        .filter(|field| {
            selected.is_none_or(|category| field.category == category)
                && (selected.is_some()
                    || !query.is_empty()
                    || matches!(
                        field.key,
                        "model" | "physical_memory" | "os" | "adapter" | "pci_id" | "disk"
                    ))
        })
        .map(|field| Row {
            label: field.label(pt).into(),
            value: field_display(field, pt).into(),
            detail: source_label(field.source, pt).into(),
            group: field.group(pt).into(),
            group_start: false,
            copyable: field.state == State::Ok,
        })
        .filter(|row| {
            [
                row.label.as_str(),
                row.value.as_str(),
                row.detail.as_str(),
                row.group.as_str(),
            ]
            .iter()
            .any(|value| value.to_lowercase().contains(&query))
        })
        .collect();
    let mut previous = SharedString::default();
    for row in &mut rows {
        if row.group != previous {
            row.group_start = true;
            previous = row.group.clone();
        }
    }
    window.set_rows(model(rows));
    if let Some(snapshot) = &app.snapshot {
        let format = if pt {
            "%d/%m · %H:%M:%S"
        } else {
            "%m/%d · %H:%M:%S"
        };
        window.set_capture_time(
            snapshot
                .captured_at
                .with_timezone(&Local)
                .format(format)
                .to_string()
                .into(),
        );
    }
}

fn monitor(app: &mut App, window: &MainWindow, rebuild_choices: bool) {
    let pt = window.get_portuguese();
    if matches!(window.get_page(), 0 | 1 | 2 | 4) {
        let metrics = live::metrics(&app.monitor, window.get_page(), window.get_paused(), pt);
        if metrics.len() == app.live_metrics.row_count() {
            for (index, reading) in metrics.into_iter().enumerate() {
                if app.live_metrics.row_data(index).as_ref() != Some(&reading) {
                    app.live_metrics.set_row_data(index, reading);
                }
            }
        } else {
            app.live_metrics.set_vec(metrics);
        }
    }
    if window.get_page() != SENSOR_PAGE {
        return;
    }
    let query = app.metric_query.trim().to_lowercase();
    let mut options = app
        .monitor
        .series
        .iter()
        .filter_map(|(id, series)| {
            let sample = series.points.back()?;
            if !live::group_matches(window.get_sensor_group(), sample) {
                return None;
            }
            let name = sensor_name(sample, pt);
            let shown = if sample.unit.is_empty() {
                name.into()
            } else {
                format!("{name} · {}", sample.unit)
            };
            (shown.to_lowercase().contains(&query)
                || source_label(sample.source, pt)
                    .to_lowercase()
                    .contains(&query))
            .then_some((id.clone(), shown))
        })
        .collect::<Vec<_>>();
    options.sort_by(|a, b| {
        let priority = |id: &str| match id {
            "cpu:load" => 0,
            "memory:load" => 1,
            id if id.starts_with("gpu:") => 2,
            id if id.starts_with("cpu:logical:") => 4,
            _ => 3,
        };
        (priority(&a.0), &a.1, &a.0).cmp(&(priority(&b.0), &b.1, &b.0))
    });
    let (keys, names): (Vec<_>, Vec<_>) = options.into_iter().unzip();
    if rebuild_choices || keys != app.metric_keys {
        app.metric_keys = keys;
        window.set_metric_options(model(names.into_iter().map(SharedString::from).collect()));
        let index = app
            .metric_keys
            .iter()
            .position(|key| key == &app.selected)
            .unwrap_or(0);
        window.set_metric_index(if app.metric_keys.is_empty() {
            -1
        } else {
            index as i32
        });
    }

    let series = app
        .metric_keys
        .get(window.get_metric_index().max(0) as usize)
        .and_then(|key| app.monitor.series.get(key));
    if let Some((series, sample)) =
        series.and_then(|series| series.points.back().map(|sample| (series, sample)))
    {
        let now = if window.get_paused() {
            sample.seen
        } else {
            Instant::now()
        };
        let stat = |value| {
            if series.count > 0 {
                sensor_value(value, sample.unit, pt).into()
            } else {
                SharedString::from("—")
            }
        };
        window.set_current_value(live::current(sample, window.get_paused(), pt).into());
        window.set_metric_tone(live::tone(sample));
        window.set_low_value(stat(series.min));
        window.set_high_value(stat(series.max));
        window.set_average_value(stat(series.mean));
        window.set_metric_detail(
            format!(
                "{} · {}",
                source_label(sample.source, pt),
                sample.at.with_timezone(&Local).format("%H:%M:%S")
            )
            .into(),
        );
        let range = Duration::from_secs(RANGES[window.get_range_index() as usize]);
        let plot = graph::plot(series, now, range, pt);
        window.set_has_graph(!plot.path.is_empty());
        window.set_graph(plot.path.into());
        window.set_axis_low(plot.low.into());
        window.set_axis_high(plot.high.into());
    } else {
        for set in [
            MainWindow::set_current_value,
            MainWindow::set_low_value,
            MainWindow::set_high_value,
            MainWindow::set_average_value,
        ] {
            set(window, "—".into());
        }
        window.set_metric_detail("".into());
        window.set_metric_tone(0);
        window.set_axis_low("".into());
        window.set_axis_high("".into());
        window.set_has_graph(false);
    }
}

fn clear_preview(app: &mut App, window: &MainWindow) {
    app.report = None;
    window.set_export_prepared(false);
    window.set_preview_count(0);
    window.set_preview_page(0);
    window.set_preview(model(Vec::new()));
}

fn preview(app: &App, window: &MainWindow, page: i32) {
    if let Some((report, _)) = &app.report {
        let final_page = report.len().saturating_sub(1) / PAGE_SIZE;
        let page = page.clamp(0, final_page as i32);
        window.set_preview_page(page);
        window.set_preview(model(
            report
                .preview(page as usize * PAGE_SIZE, PAGE_SIZE)
                .into_iter()
                .map(|(label, value)| Row {
                    label: label.into(),
                    value: value.into(),
                    ..Default::default()
                })
                .collect(),
        ));
    }
}

fn preference(window: &MainWindow, language: Language) -> bool {
    let portuguese = language == Language::Portuguese;
    let result = slint::select_bundled_translation(if portuguese { "pt-BR" } else { "en" });
    if result.is_err() {
        let _ = slint::select_bundled_translation("en");
    }
    window.set_portuguese(portuguese && result.is_ok());
    result.is_ok()
}

fn open_folder(path: PathBuf, window: slint::Weak<MainWindow>) -> io::Result<()> {
    let command = if cfg!(windows) {
        "explorer.exe"
    } else {
        "xdg-open"
    };
    thread::Builder::new()
        .name("open-report-folder".into())
        .spawn(move || {
            if !std::process::Command::new(command)
                .arg(path)
                .status()
                .is_ok_and(|status| cfg!(windows) || status.success())
            {
                let _ = window.upgrade_in_event_loop(|window| window.set_notice(7));
            }
        })
        .map(|_| ())
}

fn bind(
    window: &MainWindow,
    app: &Rc<RefCell<App>>,
    control: &Arc<workers::Control>,
    refresh: mpsc::SyncSender<()>,
) {
    let weak = window.as_weak();
    let data = Rc::clone(app);
    window.on_navigate(move |page| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if page == window.get_page() {
            return;
        }
        if matches!(window.get_notice(), 1 | 2 | 6) {
            window.set_notice(0);
        }
        let mut app = data.borrow_mut();
        if page == EXPORT_PAGE && window.get_page() != EXPORT_PAGE {
            window.set_share_safe_category(true);
            let from = window.get_page();
            window.set_export_default_scope(if from == SENSOR_PAGE {
                2
            } else if category(from).is_some() {
                1
            } else {
                0
            });
            if category(from).is_some() {
                window.set_export_category(from - 1);
            }
        }
        window.set_page(page);
        if page < SENSOR_PAGE {
            inventory(&app, &window);
        }
        if matches!(page, 0 | 1 | 2 | 4 | SENSOR_PAGE) {
            monitor(&mut app, &window, true);
        }
        if page == EXPORT_PAGE {
            clear_preview(&mut app, &window);
        }
    });
    let weak = window.as_weak();
    let data = Rc::clone(app);
    window.on_language_selected(move |index| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let language = if index == 1 {
            Language::Portuguese
        } else {
            Language::English
        };
        window.set_save_failed(
            !(preference(&window, language) && preferences::save(language).is_ok()),
        );
        let mut app = data.borrow_mut();
        inventory(&app, &window);
        monitor(&mut app, &window, true);
        clear_preview(&mut app, &window);
    });
    let weak = window.as_weak();
    window.on_refresh(move || {
        if let Some(window) = weak.upgrade()
            && refresh.try_send(()).is_ok()
        {
            window.set_refreshing(true);
        }
    });
    let weak = window.as_weak();
    let data = Rc::clone(app);
    window.on_search(move |query| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let mut app = data.borrow_mut();
        app.inventory_query = query.into();
        inventory(&app, &window);
    });
    let weak = window.as_weak();
    let data = Rc::clone(app);
    window.on_copy(move |value| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let mut app = data.borrow_mut();
        if app.clipboard.is_none() {
            app.clipboard = arboard::Clipboard::new().ok();
        }
        let copied = app
            .clipboard
            .as_mut()
            .is_some_and(|clipboard| clipboard.set_text(value.to_string()).is_ok());
        window.set_notice(if copied { 1 } else { 2 });
    });
    let weak = window.as_weak();
    let data = Rc::clone(app);
    let state = Arc::clone(control);
    window.on_pause(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let paused = !window.get_paused();
        state.paused.store(paused, Ordering::Relaxed);
        window.set_paused(paused);
        monitor(&mut data.borrow_mut(), &window, false);
    });
    let weak = window.as_weak();
    let data = Rc::clone(app);
    window.on_metric_filter(move |query| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let mut app = data.borrow_mut();
        app.metric_query = query.into();
        monitor(&mut app, &window, true);
    });
    let weak = window.as_weak();
    let data = Rc::clone(app);
    window.on_sensor_group_changed(move |group| {
        if let Some(window) = weak.upgrade() {
            window.set_sensor_group(group);
            monitor(&mut data.borrow_mut(), &window, true);
        }
    });
    let weak = window.as_weak();
    let data = Rc::clone(app);
    window.on_open_sensor(move |key| {
        if let Some(window) = weak.upgrade() {
            {
                let mut app = data.borrow_mut();
                app.selected = key.into();
                app.metric_query.clear();
            }
            window.set_sensor_group(0);
            window.set_metric_search_text("".into());
            window.invoke_navigate(SENSOR_PAGE);
        }
    });
    let weak = window.as_weak();
    let data = Rc::clone(app);
    window.on_metric(move |index| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let mut app = data.borrow_mut();
        if let Some(key) = app.metric_keys.get(index.max(0) as usize).cloned() {
            app.selected = key;
            window.set_metric_index(index);
            monitor(&mut app, &window, false);
        }
    });
    let weak = window.as_weak();
    let data = Rc::clone(app);
    window.on_range_changed(move |index| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        window.set_range_index(index.clamp(0, 2));
        monitor(&mut data.borrow_mut(), &window, false);
    });
    bind_export(window, app);
}

fn save_error_notice(error: &io::Error) -> i32 {
    match error.kind() {
        io::ErrorKind::AlreadyExists => 4,
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => 9,
        io::ErrorKind::StorageFull => 10,
        io::ErrorKind::NotFound => 11,
        _ => 5,
    }
}

fn bind_export(window: &MainWindow, app: &Rc<RefCell<App>>) {
    let weak = window.as_weak();
    let data = Rc::clone(app);
    window.on_prepare_export(move |scope, format, safe, category_index| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let mut app = data.borrow_mut();
        let format =
            [Format::Json, Format::Text, Format::Csv, Format::Html][format.clamp(0, 3) as usize];
        let scope = match scope {
            1 => {
                let Some(category) = category(category_index + 1) else {
                    clear_preview(&mut app, &window);
                    window.set_notice(6);
                    return;
                };
                Scope::Inventory(Some(category))
            }
            2 => Scope::Sensors(None),
            3 => Scope::Sensors(Some(Duration::from_secs(300))),
            4 => Scope::Sensors(Some(Duration::from_secs(900))),
            _ => Scope::Inventory(None),
        };
        let report = Report::capture(
            app.snapshot.as_ref(),
            &app.monitor,
            scope,
            safe || !matches!(scope, Scope::Inventory(Some(_))),
            window.get_portuguese(),
        );
        clear_preview(&mut app, &window);
        window.set_notice(if report.is_empty() { 6 } else { 0 });
        if report.is_empty() {
            return;
        }
        window.set_preview_count(report.len() as i32);
        window.set_export_prepared(true);
        app.report = Some((Arc::new(report), format));
        preview(&app, &window, 0);
    });
    let weak = window.as_weak();
    let data = Rc::clone(app);
    window.on_invalidate_export(move || {
        if let Some(window) = weak.upgrade() {
            clear_preview(&mut data.borrow_mut(), &window);
        }
    });
    let weak = window.as_weak();
    let data = Rc::clone(app);
    window.on_change_preview_page(move |step| {
        if let Some(window) = weak.upgrade() {
            preview(&data.borrow(), &window, window.get_preview_page() + step);
        }
    });
    let weak = window.as_weak();
    let data = Rc::clone(app);
    window.on_open_folder(move || {
        let path = data.borrow().report_folder.lock().unwrap().clone();
        if let Some(path) = path
            && open_folder(path, weak.clone()).is_err()
            && let Some(window) = weak.upgrade()
        {
            window.set_notice(7);
        }
    });
    let weak = window.as_weak();
    let data = Rc::clone(app);
    window.on_save_export(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let Some((report, format)) = data.borrow().report.clone() else {
            return;
        };
        if window.get_export_busy() {
            return;
        }
        window.set_export_busy(true);
        window.set_folder_ready(false);
        window.set_notice(0);
        let title = if window.get_portuguese() {
            "Salvar relatório do Nucelvo"
        } else {
            "Save Nucelvo Report"
        };
        let saved_path = Arc::clone(&data.borrow().report_folder);
        let weak = window.as_weak();
        let result = thread::Builder::new()
            .name("report-export".into())
            .spawn(move || {
                let ext = format.extension();
                let name = format!("nucelvo-{}.{}", Local::now().format("%Y%m%d-%H%M%S"), ext);
                let chosen = rfd::FileDialog::new()
                    .set_title(title)
                    .set_file_name(name)
                    .add_filter(ext.to_uppercase(), &[ext])
                    .save_file();
                let (notice, detail) = match chosen {
                    Some(mut path) => {
                        if !path
                            .extension()
                            .is_some_and(|extension| extension.eq_ignore_ascii_case(ext))
                            && let Some(mut name) = path.file_name().map(|name| name.to_os_string())
                        {
                            name.push(format!(".{ext}"));
                            path.set_file_name(name);
                        }
                        match report.save(&path, format) {
                            Ok(()) => {
                                *saved_path.lock().unwrap() =
                                    path.parent().map(|path| path.to_owned());
                                (3, path.to_string_lossy().into_owned())
                            }
                            Err(error) => (save_error_notice(&error), String::new()),
                        }
                    }
                    None => (8, String::new()),
                };
                let _ = weak.upgrade_in_event_loop(move |window| {
                    window.set_notice_detail(detail.into());
                    window.set_notice(notice);
                    window.set_export_busy(false);
                    window.set_folder_ready(notice == 3);
                });
            });
        if result.is_err() {
            window.set_export_busy(false);
            window.set_notice(5);
        }
    });
}

fn update(window: &MainWindow, app: &mut App, updates: workers::Updates) {
    let inventory_changed = updates.base.is_some() || updates.details.is_some();
    if let Some(base) = updates.base {
        if app.snapshot.is_none() {
            app.snapshot = Some(base.clone());
        }
        app.pending_base = Some(base);
    }
    if let Some(details) = updates.details
        && let Some(mut snapshot) = app.pending_base.take()
    {
        snapshot.fields.extend(details);
        snapshot
            .fields
            .sort_by_key(|field| (field.category, field.device));
        snapshot.captured_at = Utc::now();
        app.snapshot = Some(snapshot);
        window.set_ready(true);
        window.set_refreshing(false);
    }
    if inventory_changed && window.get_page() < SENSOR_PAGE {
        inventory(app, window);
    }
    if window.get_paused() {
        return;
    }
    let mut changed = false;
    if let Some(samples) = updates.fast {
        for sample in samples {
            app.monitor.record(sample);
        }
        changed = true;
    }
    if let Some(samples) = updates.logical {
        for sample in samples {
            app.monitor.record(sample);
        }
    }
    if let Some(samples) = updates.native {
        let missing = app
            .monitor
            .series
            .iter()
            .filter_map(|(id, series)| {
                let last = series.points.back()?;
                (!id.starts_with("cpu:")
                    && id != "memory:load"
                    && last.state != State::Stale
                    && !samples.iter().any(|sample| sample.id == *id))
                .then_some(last.clone())
            })
            .collect::<Vec<_>>();
        for mut sample in missing {
            sample.seen = Instant::now();
            sample.at = Utc::now();
            sample.state = State::Stale;
            sample.value = None;
            app.monitor.record(sample);
        }
        for sample in samples {
            app.monitor.record(sample);
        }
        changed = true;
    }
    if changed && !window.window().is_minimized() {
        monitor(app, window, false);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    let mut smoke = false;
    let mut require_logical = false;
    let mut require_nvidia = false;
    for argument in std::env::args().skip(1) {
        smoke |= argument == "--smoke-test";
        require_logical |= argument == "--require-logical-cpu";
        require_nvidia |= argument == "--require-nvidia";
    }
    let window = MainWindow::new()?;
    let closing_window = window.as_weak();
    window.window().on_close_requested(move || {
        if closing_window
            .upgrade()
            .is_some_and(|window| window.get_export_busy())
        {
            slint::CloseRequestResponse::KeepWindowShown
        } else {
            slint::CloseRequestResponse::HideWindow
        }
    });
    preference(
        &window,
        if smoke {
            Language::English
        } else {
            preferences::load()
        },
    );
    window.set_refreshing(true);
    let updates = Arc::new(Mutex::new(workers::Updates::default()));
    let control = Arc::new(workers::Control::default());
    let (refresh, requests) = mpsc::sync_channel(1);
    let options = (Arc::clone(&updates), Arc::clone(&control));
    let fast = thread::Builder::new()
        .name("live-sampling".into())
        .spawn(move || workers::fast(options.0, options.1))?;
    let options = (Arc::clone(&updates), Arc::clone(&control));
    let hardware = thread::Builder::new()
        .name("hardware-inventory".into())
        .spawn(move || workers::hardware(options.0, options.1, requests))?;
    let app = Rc::new(RefCell::new(App::default()));
    window.set_live_metrics(ModelRc::from(Rc::clone(&app.borrow().live_metrics)));
    bind(&window, &app, &control, refresh);
    monitor(&mut app.borrow_mut(), &window, true);
    let timer = Timer::default();
    let weak = window.as_weak();
    let passed = Rc::new(Cell::new(false));
    let result = Rc::clone(&passed);
    let mut was_minimized = false;
    timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let native = window.window();
        let minimized = native.is_minimized();
        let restored = std::mem::replace(&mut was_minimized, minimized) && !minimized;
        if !minimized {
            window.set_narrow((native.size().width as f32 / native.scale_factor()) < 810.0);
        }
        let received = std::mem::take(&mut *updates.lock().unwrap());
        update(&window, &mut app.borrow_mut(), received);
        if restored {
            monitor(&mut app.borrow_mut(), &window, false);
            #[cfg(windows)]
            window_windows::repaint_restored_window(native);
        }
        let logical = if smoke {
            app.borrow().monitor.series.iter().filter(|(id, series)| {
                id.starts_with("cpu:logical:")
                    && series.points.back().is_some_and(|point| point.state == State::Ok && point.value.is_some())
            }).count()
        } else {
            0
        };
        let nvidia_ready = !require_nvidia || {
            let app = app.borrow();
            let has_reading = |kind| {
                app.monitor.series.iter().any(|(id, series)| {
                    id.starts_with("gpu:nvidia:")
                        && id.rsplit(':').next() == Some(kind)
                        && series.points.back().is_some_and(|point| point.state == State::Ok && point.value.is_some())
                })
            };
            ["temperature", "load", "vram", "fan", "power"].into_iter().all(has_reading)
        };
        if smoke
            && window.get_ready()
            && (!require_logical || logical > 0)
            && nvidia_ready
            && app
                .borrow()
                .monitor
                .series
                .get("cpu:load")
                .is_some_and(|series| series.count > 0)
        {
            let english = window.get_overview_title() == "Overview";
            let pt = preference(&window, Language::Portuguese)
                && window.get_overview_title() == "Visão geral";
            let english_restored = preference(&window, Language::English)
                && window.get_overview_title() == "Overview";
            let sensor_link = if require_nvidia {
                let key = app.borrow().live_metrics.row_data(3).unwrap_or_default().key;
                window.invoke_open_sensor(key.clone());
                key.starts_with("gpu:nvidia:")
                    && window.get_page() == SENSOR_PAGE
                    && window.get_metric_tone() == 3
            } else {
                true
            };
            window.invoke_navigate(SENSOR_PAGE);
            let chart_visible = window.get_has_graph();
            let state = app.borrow();
            let real = state.snapshot.as_ref().is_some_and(|snapshot| {
                ["model", "physical_memory", "os"].iter().all(|key| {
                    snapshot
                        .fields
                        .iter()
                        .any(|field| field.key == *key && field.state == State::Ok)
                }) && ["power_source", "battery_detected"].iter().all(|key| {
                    snapshot.fields.iter().any(|field| field.key == *key && field.category == Category::System)
                }) && Category::ALL.iter().all(|category| {
                    snapshot
                        .fields
                        .iter()
                        .any(|field| field.category == *category)
                })
            });
            let report = Report::capture(
                state.snapshot.as_ref(),
                &state.monitor,
                Scope::Inventory(None),
                true,
                false,
            );
            let saved = tempfile::tempdir().ok().is_some_and(|dir| {
                let sensors = Report::capture(
                    None,
                    &state.monitor,
                    Scope::Sensors(Some(Duration::from_secs(60))),
                    true,
                    false,
                );
                [Format::Json, Format::Text, Format::Csv, Format::Html]
                    .iter()
                    .all(|format| {
                        report
                            .save(&dir.path().join(format.extension()), *format)
                            .is_ok()
                    })
                    && !sensors.is_empty()
                    && [Format::Csv, Format::Html].iter().all(|format| {
                        sensors.save(&dir.path().join(format!("sensors.{}", format.extension())), *format).is_ok()
                    })
            });
            result.set(english && pt && english_restored && sensor_link && real && chart_visible && saved && !report.is_empty());
            println!(
                "Inventory, chart, localization and exports: {} ms. Logical CPUs with a valid sample: {logical}.",
                started.elapsed().as_millis(),
            );
            let _ = slint::quit_event_loop();
        } else if smoke && started.elapsed() >= Duration::from_secs(25) {
            eprintln!("Desktop smoke test timed out. Logical CPUs with a valid sample: {logical}.");
            let _ = slint::quit_event_loop();
        }
    });
    let run = window.run();
    timer.stop();
    control.stopping.store(true, Ordering::Relaxed);
    fast.thread().unpark();
    drop(window);
    fast.join()
        .map_err(|_| io::Error::other("live sampling failed"))?;
    if hardware.is_finished() {
        hardware
            .join()
            .map_err(|_| io::Error::other("hardware worker failed"))?;
    }
    run?;
    if smoke && !passed.get() {
        return Err(io::Error::other("desktop smoke test failed").into());
    }
    Ok(())
}
