#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod about;
mod graph;
mod inventory_view;
mod live;
mod preferences;
#[cfg(windows)]
mod window_windows;
mod workers;

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex, atomic::Ordering, mpsc},
    thread,
    time::{Duration, Instant},
};

use chrono::{Local, Utc};
use nucelvo_core::{Category, Monitor, Snapshot, State};
use nucelvo_report::{Format, Report, Scope, sensor_name, sensor_value, source_label};
use preferences::{Language, Preferences, Theme};
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};

slint::include_modules!();

const SENSOR_PAGE: i32 = 7;
const STORAGE_PAGE: i32 = 5;
const EXPORT_PAGE: i32 = 8;
const PAGE_SIZE: usize = 100;
const RANGES: [u64; 3] = [60, 300, 900];

#[derive(Default)]
struct App {
    preferences: Preferences,
    snapshot: Option<Snapshot>,
    pending_base: Option<Snapshot>,
    monitor: Monitor,
    inventory_query: String,
    metric_query: String,
    selected: String,
    metric_keys: Vec<String>,
    inline_open: [bool; 7],
    inline_selected: [String; 7],
    storage_choices: Vec<usize>,
    storage_selected: Option<usize>,
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

fn inventory(app: &mut App, window: &MainWindow) {
    let pt = window.get_portuguese();
    let selected = category(window.get_page());
    let query = app.inventory_query.trim().to_lowercase();
    let fields = app
        .snapshot
        .as_ref()
        .map_or(&[][..], |snapshot| snapshot.fields.as_slice());
    let storage = window.get_page() == STORAGE_PAGE;
    let drives = if storage {
        inventory_view::drives(fields, pt)
    } else {
        BTreeMap::new()
    };
    let models: BTreeMap<_, _> = if storage {
        fields
            .iter()
            .filter_map(|field| match &field.value {
                Some(nucelvo_core::Value::Text(name))
                    if field.category == Category::Storage && field.key == "model" =>
                {
                    Some((field.device, name.trim()))
                }
                _ => None,
            })
            .collect()
    } else {
        BTreeMap::new()
    };
    let mut volumes: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    if storage {
        let labels: Vec<SharedString> = std::iter::once(
            if pt {
                "Todas as unidades"
            } else {
                "All drives"
            }
            .into(),
        )
        .chain(drives.values().map(|label| label.as_str().into()))
        .collect();
        if !window
            .get_storage_options()
            .iter()
            .eq(labels.iter().cloned())
        {
            window.set_storage_options(model(labels));
        }
        app.storage_choices = drives.keys().copied().collect();
        let selected_index = app
            .storage_selected
            .and_then(|id| drives.keys().position(|key| *key == id))
            .map_or(0, |i| i as i32 + 1);
        if selected_index == 0 {
            app.storage_selected = None;
        }
        window.set_storage_index(selected_index);
        for field in fields
            .iter()
            .filter(|field| field.category == Category::Storage && field.device >= 10_000)
        {
            let entries = volumes.entry(inventory_view::drive(field)).or_default();
            if entries.last() != Some(&field.device) {
                entries.push(field.device);
            }
        }
    }
    let mut matching: Vec<_> = fields
        .iter()
        .filter(|field| {
            selected.is_none_or(|category| field.category == category)
                && (selected.is_some()
                    || !query.is_empty()
                    || matches!(
                        field.key,
                        "model" | "physical_memory" | "os" | "adapter" | "pci_id" | "disk"
                    ))
        })
        .filter(|field| {
            !storage
                || app
                    .storage_selected
                    .is_none_or(|id| inventory_view::drive(field) == id)
        })
        .collect();
    if storage {
        if query.is_empty() {
            matching.retain(|field| field.key != "model" || !models.contains_key(&field.device));
        }
        matching.sort_by_key(|field| {
            (
                inventory_view::drive(field),
                inventory_view::section(field),
                field.device,
            )
        });
    }
    let mut rows: Vec<Row> = matching
        .into_iter()
        .map(|field| {
            let mut row = inventory_view::row(field, pt);
            if storage {
                let drive = inventory_view::drive(field);
                row.group = drives[&drive].as_str().into();
                row.group_model = models.get(&drive).copied().unwrap_or("").into();
                row.subgroup = if inventory_view::section(field) == 2 {
                    if pt {
                        "Saúde e uso"
                    } else {
                        "Health and usage"
                    }
                    .into()
                } else {
                    inventory_view::subgroup(field, &volumes).into()
                };
            }
            row
        })
        .filter(|row| {
            [
                row.label.as_str(),
                row.value.as_str(),
                row.detail.as_str(),
                row.group.as_str(),
                row.note.as_str(),
            ]
            .iter()
            .any(|value| value.to_lowercase().contains(&query))
        })
        .collect();
    let mut previous = SharedString::default();
    let mut previous_subgroup = SharedString::default();
    for row in &mut rows {
        if row.group != previous {
            row.group_start = selected.is_none_or(|category| row.group != category.label(pt));
            previous = row.group.clone();
            previous_subgroup = "".into();
        }
        row.subgroup_start = !row.subgroup.is_empty() && row.subgroup != previous_subgroup;
        previous_subgroup = row.subgroup.clone();
    }
    window.set_expanded_note(-1);
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
    let page = window.get_page();
    if matches!(page, 0 | 1 | 2 | 4 | 6) {
        let metrics = live::metrics(&app.monitor, page, window.get_paused(), pt);
        if page == 4 {
            window.set_live_device(
                metrics
                    .iter()
                    .find(|metric| !metric.detail.is_empty())
                    .map(|metric| metric.detail.clone())
                    .unwrap_or_default(),
            );
        }
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
    let inline = page < SENSOR_PAGE;
    let available = inline
        && (matches!(page, 0..=2)
            || app.monitor.series.values().any(|series| {
                series
                    .points
                    .back()
                    .is_some_and(|sample| live::inline_matches(page, sample))
            }));
    window.set_inline_chart_available(available);
    let open = available && app.inline_open[page as usize];
    window.set_inline_chart_open(open);
    if page != SENSOR_PAGE && !open {
        return;
    }
    let query = if inline {
        String::new()
    } else {
        app.metric_query.trim().to_lowercase()
    };
    let mut options = app
        .monitor
        .series
        .iter()
        .filter_map(|(id, series)| {
            let sample = series.points.back()?;
            if !(if inline {
                live::inline_matches(page, sample)
            } else {
                live::group_matches(window.get_sensor_group(), sample)
            }) {
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
        let selected = if inline {
            &app.inline_selected[page as usize]
        } else {
            &app.selected
        };
        let index = app
            .metric_keys
            .iter()
            .position(|key| key == selected)
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
        window.set_metric_icon(live::icon(sample));
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
        window.set_graph_area(plot.area.into());
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
        window.set_metric_icon(9);
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
            inventory(&mut app, &window);
        }
        monitor(&mut app, &window, true);
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
        let mut app = data.borrow_mut();
        app.preferences.language = language;
        window.set_save_failed(
            !(preference(&window, language) && preferences::save(app.preferences).is_ok()),
        );
        inventory(&mut app, &window);
        monitor(&mut app, &window, true);
        clear_preview(&mut app, &window);
    });
    let weak = window.as_weak();
    let data = Rc::clone(app);
    window.on_theme_selected(move |choice| {
        let theme = match choice {
            1 => Theme::Light,
            2 => Theme::Dark,
            _ => return,
        };
        if let Some(window) = weak.upgrade() {
            let mut app = data.borrow_mut();
            app.preferences.theme = theme;
            window.invoke_apply_theme(theme.choice());
            window.set_save_failed(preferences::save(app.preferences).is_err());
        }
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
        inventory(&mut app, &window);
    });
    let weak = window.as_weak();
    let data = Rc::clone(app);
    window.on_storage_selected(move |index| {
        if let Some(window) = weak.upgrade() {
            let mut app = data.borrow_mut();
            let selected = usize::try_from(index - 1)
                .ok()
                .and_then(|index| app.storage_choices.get(index).copied());
            app.storage_selected = selected;
            inventory(&mut app, &window);
        }
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
        if let Some(window) = weak.upgrade()
            && window.get_inline_chart_available()
        {
            let mut app = data.borrow_mut();
            let page = window.get_page() as usize;
            app.inline_open[page] = if key.is_empty() {
                !app.inline_open[page]
            } else {
                true
            };
            if !key.is_empty() {
                app.inline_selected[page] = key.into();
            }
            monitor(&mut app, &window, true);
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
            if window.get_page() == SENSOR_PAGE {
                app.selected = key;
            } else {
                app.inline_selected[window.get_page() as usize] = key;
            }
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
    if let Some(samples) = updates.network {
        for sample in samples {
            app.monitor.record(sample);
        }
        changed = true;
    }
    if let Some(samples) = updates.native {
        let missing = app
            .monitor
            .series
            .iter()
            .filter_map(|(id, series)| {
                let last = series.points.back()?;
                (!id.starts_with("cpu:")
                    && !id.starts_with("network:")
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
    let mut require_gpu_clocks = false;
    let mut require_native_cache = false;
    let mut require_memory_diagnostics = false;
    let mut require_storage = false;
    let mut require_network = false;
    for argument in std::env::args().skip(1) {
        smoke |= argument == "--smoke-test";
        require_logical |= argument == "--require-logical-cpu";
        require_nvidia |= argument == "--require-nvidia";
        require_gpu_clocks |= argument == "--require-gpu-clocks";
        require_native_cache |= argument == "--require-native-cache";
        require_memory_diagnostics |= argument == "--require-memory-diagnostics";
        require_storage |= argument == "--require-storage";
        require_network |= argument == "--require-network";
    }
    let window = MainWindow::new()?;
    about::bind(&window);
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
    let settings = if smoke {
        Preferences::default()
    } else {
        preferences::load()
    };
    preference(&window, settings.language);
    if settings.theme != Theme::System {
        window.invoke_apply_theme(settings.theme.choice());
    }
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
    let app = Rc::new(RefCell::new(App {
        preferences: settings,
        ..App::default()
    }));
    window.set_live_metrics(ModelRc::from(Rc::clone(&app.borrow().live_metrics)));
    bind(&window, &app, &control, refresh);
    monitor(&mut app.borrow_mut(), &window, true);
    let timer = Timer::default();
    let weak = window.as_weak();
    let smoke_result = Rc::new(Cell::new(None));
    let result = Rc::clone(&smoke_result);
    let mut was_minimized = false;
    timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
        // Slint can service another timer tick before the backend exits the event loop.
        if result.get().is_some() {
            return;
        }
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
        let network_ready = !require_network || {
            let app = app.borrow();
            ["network:receive", "network:send"].iter().all(|id| {
                app.monitor.series.get(*id)
                    .and_then(|series| series.points.back())
                    .is_some_and(|point| point.state == State::Ok && point.value.is_some())
            })
        };
        let clock_id = |domain: &str| {
            app.borrow().monitor.series.iter().find_map(|(id, series)| {
                (id.starts_with("gpu:nvidia:") && id.ends_with(domain)
                    && series.points.back().is_some_and(|point| point.value.is_some()))
                    .then(|| id.clone())
            })
        };
        let graphics_clock = require_gpu_clocks.then(|| clock_id(":graphics_clock")).flatten();
        let clocks_ready = !require_gpu_clocks
            || (graphics_clock.is_some() && clock_id(":memory_clock").is_some());
        if smoke
            && window.get_ready()
            && (!require_logical || logical > 0)
            && nvidia_ready
            && network_ready
            && clocks_ready
            && app
                .borrow()
                .monitor
                .series
                .get("cpu:load")
                .is_some_and(|series| series.count > 0)
        {
            let english = window.get_overview_title() == "Overview";
            let pt = preference(&window, Language::Portuguese)
                && window.get_overview_title() == "Visão geral"
                && window.get_about_title() == "Sobre";
            let english_restored = preference(&window, Language::English)
                && window.get_overview_title() == "Overview"
                && window.get_about_title() == "About";
            window.invoke_apply_theme(Theme::Light.choice());
            let light = !window.get_dark_mode();
            window.invoke_apply_theme(Theme::Dark.choice());
            let theme_switch = light && window.get_dark_mode();
            window.invoke_open_sensor("cpu:load".into());
            let inline_cpu = window.get_page() == 0
                && window.get_inline_chart_open()
                && window.get_has_graph();
            let sensor_link = if require_nvidia {
                let key = app.borrow().live_metrics.row_data(3).unwrap_or_default().key;
                window.invoke_open_sensor(key.clone());
                key.starts_with("gpu:nvidia:")
                    && window.get_page() == 0
                    && window.get_inline_chart_open()
                    && window.get_metric_tone() == 3
            } else {
                true
            };
            let clock_chart = graphics_clock.is_none_or(|id| {
                window.invoke_navigate(4);
                window.invoke_open_sensor(id.into());
                window.get_inline_chart_open() && window.get_has_graph()
                    && window.get_metric_icon() == 16
            });
            let native_cache = !require_native_cache || {
                window.invoke_navigate(1);
                let rows = window.get_rows();
                ["Total L1 data cache", "Total L1 instruction cache", "Total L2 cache", "Total L3 cache"]
                    .iter().all(|label| rows.iter().any(|row| row.label == *label
                        && row.copyable && !row.note.is_empty()))
            };
            let memory_diagnostics = !require_memory_diagnostics || {
                window.invoke_navigate(2);
                window.get_rows().iter().any(|row| {
                    row.group == "Memory diagnostics" && row.label == "Latest Windows memory test"
                        && row.copyable && !row.note.is_empty()
                })
            };
            window.invoke_navigate(STORAGE_PAGE);
            let storage_layout = !require_storage || {
                let nested = window.get_rows().iter().any(|row| row.subgroup_start && !row.group_start
                    && row.group.starts_with("Drive ") && row.subgroup.starts_with("Volume"));
                window.invoke_storage_selected(1);
                let single = window.get_rows().iter().filter(|row| row.group_start).count() == 1;
                window.invoke_storage_selected(0);
                nested && single
            };
            window.invoke_navigate(6);
            let uptime_note = window.get_rows().iter().any(|row| row.label == "Uptime at capture" && !row.note.is_empty());
            let network_inventory = !require_network || {
                let rows = window.get_rows();
                ["Adapter", "Link status", "Data received by the adapter", "Data sent by the adapter"]
                    .iter().all(|label| rows.iter().any(|row| row.label == *label
                        && row.group.starts_with("Network adapter ") && row.copyable))
            };
            let network_chart = !require_network || {
                window.invoke_open_sensor("network:receive".into());
                window.get_inline_chart_open() && window.get_has_graph()
                    && window.get_metric_icon() == 14
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
            let storage_readings = state.snapshot.as_ref().is_some_and(|snapshot| {
                let valid = |key, source| snapshot.fields.iter().any(|field| {
                    field.key == key && field.source == source && field.state == State::Ok
                });
                valid("power_on_hours", "Windows · ATA SMART")
                    && valid("power_on_hours", "Windows · NVMe health log")
                    && valid("ssd_endurance_used", "Windows · NVMe health log")
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
            let checks = [
                ("language switching", english && pt && english_restored),
                ("theme switching", theme_switch),
                ("Overview CPU chart", inline_cpu),
                ("GPU sensor link", sensor_link),
                ("Storage layout", storage_layout),
                ("uptime explanation", uptime_note),
                ("network inventory", network_inventory),
                ("network rate chart", network_chart),
                ("GPU clock chart", clock_chart),
                ("native CPU cache inventory", native_cache),
                ("memory diagnostics", memory_diagnostics),
                ("hardware inventory", real),
                ("Sensors chart", chart_visible),
                ("report formats", saved && !report.is_empty()),
                ("drive health readings", !require_storage || storage_readings),
            ];
            let mut passed = true;
            for (name, ok) in checks {
                if !ok {
                    eprintln!("Desktop smoke check failed: {name}.");
                    passed = false;
                }
            }
            result.set(Some(passed));
            println!(
                "Inventory, chart, localization and exports: {} ms. Logical CPUs with a valid sample: {logical}.",
                started.elapsed().as_millis(),
            );
            let _ = slint::quit_event_loop();
        } else if smoke && started.elapsed() >= Duration::from_secs(25) {
            eprintln!("Desktop smoke test timed out. Logical CPUs with a valid sample: {logical}.");
            result.set(Some(false));
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
    if smoke && smoke_result.get() != Some(true) {
        return Err(io::Error::other("desktop smoke test failed").into());
    }
    Ok(())
}
