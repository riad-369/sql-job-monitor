use crate::{
    domain::{
        category_summary, command_availability, filter_and_sort, format_duration, format_timestamp,
        group_runs, include_active_run, last_ran, local_epoch, next_run_in, relative_update,
        status_counts,
        timeline::{build_timeline, TimelineHistory, TimelineLane},
        ActiveRun, HistoryRun, Job, SortColumn,
    },
    platform,
    saved::{self, SavedConnection},
    settings::{self, UiSettings},
    sql::{self, Authentication, ConnectionOptions, SharedClient},
    CategoryChoice, Columns, HistoryRow, HistoryRunRow, JobRow, JobStepRow, MainWindow,
    SavedConnectionRow, StatusCountRow, Theme, TimelineBarRow, TimelineLaneRow,
};
use chrono::{Datelike, Local, NaiveDateTime, Timelike};
use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel, Weak};
use std::{
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::runtime::{Handle, Runtime};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

fn install_window_hooks(ui: &MainWindow) {
    use slint::winit_030::{EventResult, WinitWindowAccessor};
    use std::cell::Cell;
    use std::rc::Rc;

    let applied = Rc::new(Cell::new(false));
    let primary_held = Rc::new(Cell::new(false));
    let ui_weak = ui.as_weak();
    ui.window().on_winit_window_event(move |window, event| {
        if handle_shortcut(event, &ui_weak, &primary_held) {
            return EventResult::PreventDefault;
        }
        if let slint::winit_030::winit::event::WindowEvent::ThemeChanged(theme) = event {
            let light = matches!(theme, slint::winit_030::winit::window::Theme::Light);
            if let Some(ui) = ui_weak.upgrade() {
                Theme::get(&ui).set_system_light(light);
            }
        }
        if !applied.get()
            && matches!(
                event,
                slint::winit_030::winit::event::WindowEvent::RedrawRequested
            )
        {
            let done = window
                .with_winit_window(|winit_window| {
                    let titled = platform::apply_caption(winit_window);
                    let fitted = ui_weak.upgrade().is_some_and(|ui| {
                        let light = matches!(
                            winit_window.theme(),
                            Some(slint::winit_030::winit::window::Theme::Light)
                        );
                        Theme::get(&ui).set_system_light(light);
                        platform::fit_window_to_monitor(winit_window, |width, height| {
                            ui.set_window_min_width(width);
                            ui.set_window_min_height(height);
                        })
                    });
                    titled && fitted
                })
                .unwrap_or(false);
            applied.set(done);
        }
        EventResult::Propagate
    });
}

fn handle_shortcut(
    event: &slint::winit_030::winit::event::WindowEvent,
    ui_weak: &Weak<MainWindow>,
    primary_held: &std::cell::Cell<bool>,
) -> bool {
    use slint::winit_030::winit::event::{ElementState, WindowEvent};
    use slint::winit_030::winit::keyboard::{Key, NamedKey};

    match event {
        WindowEvent::ModifiersChanged(modifiers) => {
            primary_held.set(platform::primary_modifier(modifiers.state()));
            false
        }
        WindowEvent::KeyboardInput {
            event: key,
            is_synthetic: false,
            ..
        } if key.state == ElementState::Pressed && !key.repeat => {
            let Some(ui) = ui_weak.upgrade() else {
                return false;
            };
            match &key.logical_key {
                Key::Named(NamedKey::F5) if ui.get_connected() => {
                    ui.invoke_refresh_requested();
                    true
                }
                Key::Character(text)
                    if cfg!(target_os = "macos")
                        && primary_held.get()
                        && text.eq_ignore_ascii_case("r")
                        && ui.get_connected() =>
                {
                    ui.invoke_refresh_requested();
                    true
                }
                Key::Named(NamedKey::Escape) => {
                    ui.set_confirm_stop_visible(false);
                    ui.set_job_menu_open(false);
                    ui.set_steps_menu_open(false);
                    ui.set_category_menu_open(false);
                    ui.set_status_menu_open(false);
                    true
                }
                Key::Character(text)
                    if primary_held.get()
                        && text.eq_ignore_ascii_case("f")
                        && ui.get_connected() =>
                {
                    ui.set_search_focus_tick(ui.get_search_focus_tick().wrapping_add(1));
                    true
                }
                _ => false,
            }
        }
        _ => false,
    }
}

fn apply_settings(ui: &MainWindow, settings: &UiSettings, zoom_saved: bool) {
    Theme::get(ui).set_mode(settings.theme_mode);
    ui.set_pinned(settings.pinned);
    ui.set_history_pane_width(settings.history_width);
    ui.set_history_collapsed(settings.history_collapsed);
    ui.set_timeline_days(settings.timeline_days);
    ui.set_timeline_range(settings.timeline_range);
    ui.set_timeline_pph(settings.timeline_pph);
    ui.set_timeline_zoom_saved(zoom_saved);
    ui.set_auto_refresh(settings.auto_refresh);
    ui.set_refresh_seconds(settings.refresh_seconds);
    let columns = Columns::get(ui);
    let widths = settings.columns;
    columns.set_name(widths[0]);
    columns.set_status(widths[1]);
    columns.set_enabled(widths[2]);
    columns.set_step(widths[3]);
    columns.set_last_run(widths[4]);
    columns.set_duration(widths[5]);
    columns.set_next_run(widths[6]);
    columns.set_owner(widths[7]);
    columns.set_category(widths[8]);
}

fn persist_layout(ui: &MainWindow, last_connection_id: Option<String>) {
    let mut settings = settings::load();
    if let Some(id) = last_connection_id {
        settings.last_connection_id = id;
    }
    settings.theme_mode = Theme::get(ui).get_mode();
    settings.pinned = ui.get_pinned();
    settings.history_width = ui.get_history_pane_width();
    settings.history_collapsed = ui.get_history_collapsed();
    settings.timeline_days = ui.get_timeline_days();
    settings.timeline_range = ui.get_timeline_range();
    settings.timeline_pph = ui.get_timeline_pph();
    settings.timeline_zoom_saved = ui.get_timeline_zoom_saved();
    settings.auto_refresh = ui.get_auto_refresh();
    settings.refresh_seconds = ui.get_refresh_seconds();
    let columns = Columns::get(ui);
    settings.columns = [
        columns.get_name(),
        columns.get_status(),
        columns.get_enabled(),
        columns.get_step(),
        columns.get_last_run(),
        columns.get_duration(),
        columns.get_next_run(),
        columns.get_owner(),
        columns.get_category(),
    ];
    let _ = settings::store(&settings);
}

fn fill_saved_form(ui: &MainWindow, id: &str) {
    if id.is_empty() {
        return;
    }
    let Some(saved) = saved::load()
        .into_iter()
        .find(|connection| connection.id == id)
    else {
        return;
    };
    ui.set_form_server(saved.server.into());
    ui.set_form_port(
        saved
            .port
            .map(|port| port.to_string())
            .unwrap_or_default()
            .into(),
    );
    ui.set_form_username(saved.username.into());
    ui.set_form_windows_auth(saved.windows_auth);
    ui.set_form_trust_certificate(saved.trust_server_certificate);
}

fn move_job_selection(ui: &MainWindow, delta: i32) {
    use slint::Model;
    let jobs = ui.get_jobs();
    let count = jobs.row_count();
    if count == 0 {
        return;
    }
    let selected = ui.get_selected_job_id();
    let current =
        (0..count).find(|&index| jobs.row_data(index).is_some_and(|row| row.id == selected));
    let next = match current {
        Some(index) => {
            let Some(next) = i32::try_from(index)
                .ok()
                .and_then(|index| index.checked_add(delta))
            else {
                return;
            };
            if next < 0 || usize::try_from(next).is_ok_and(|next| next >= count) {
                return;
            }
            next as usize
        }
        None => {
            if delta < 0 {
                count - 1
            } else {
                0
            }
        }
    };
    let Some(row) = jobs.row_data(next) else {
        return;
    };
    ui.set_job_menu_open(false);
    ui.set_steps_menu_open(false);
    ui.set_time_tip_open(false);
    ui.set_selected_job_id(row.id.clone());
    ui.set_selected_job_name(row.name.clone());
    ui.set_selected_job_status(row.status.clone());
    ui.set_selected_job_step(row.current_step.clone());
    ui.invoke_job_selected(row.id.clone(), row.name.clone(), row.status);
    ui.set_selection_scroll_row(i32::try_from(next).unwrap_or(0));
    ui.set_selection_scroll_tick(ui.get_selection_scroll_tick().wrapping_add(1));
}

fn refreshed_ago(epoch: i32) -> String {
    if epoch <= 0 {
        return "Not updated yet".to_owned();
    }
    relative_update(Local::now().timestamp() - i64::from(epoch))
}

fn schedule_phrase(kind: &str, epoch: i32) -> String {
    if epoch <= 0 {
        return if kind == "next" {
            "No next run".to_owned()
        } else {
            "No last run".to_owned()
        };
    }
    let delta = Local::now().timestamp() - i64::from(epoch);
    if kind == "next" {
        next_run_in(-delta)
    } else {
        last_ran(delta)
    }
}

const MAX_HISTORY_RUNS: usize = 50;

pub struct Application {
    ui: MainWindow,
    runtime: Runtime,
    state: Arc<Mutex<State>>,
    auto_refresh: Arc<AtomicBool>,
    refresh_seconds: Arc<AtomicU64>,
    request_generation: Arc<AtomicU64>,
}

#[derive(Default)]
struct State {
    client: Option<SharedClient>,
    jobs: Vec<Job>,
    search: String,
    status_filter: String,
    selected_categories: Vec<String>,
    sort_column: SortColumn,
    sort_ascending: bool,
    selected_job_id: String,
    polling: Option<CancellationToken>,
    /// 0 while the grid is shown, otherwise how many days the timeline covers.
    timeline_days: u32,
    timeline_history: TimelineHistory,
}

struct PollingTask {
    client: SharedClient,
    state: Arc<Mutex<State>>,
    ui_weak: Weak<MainWindow>,
    token: CancellationToken,
    auto_refresh: Arc<AtomicBool>,
    refresh_seconds: Arc<AtomicU64>,
    generation: Arc<AtomicU64>,
}

impl Application {
    pub fn new() -> anyhow::Result<Self> {
        platform::select_backend();
        let ui = MainWindow::new()?;
        ui.set_unified_title_bar(platform::uses_unified_title_bar());
        ui.set_title_inset(if platform::uses_unified_title_bar() {
            platform::TRAFFIC_LIGHT_INSET
        } else {
            0.0
        });
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("sql-job-monitor")
            .build()?;

        Ok(Self {
            ui,
            runtime,
            state: Arc::new(Mutex::new(State {
                status_filter: "All".to_owned(),
                selected_categories: vec!["ETL".to_owned()],
                sort_ascending: true,
                ..State::default()
            })),
            auto_refresh: Arc::new(AtomicBool::new(false)),
            refresh_seconds: Arc::new(AtomicU64::new(5)),
            request_generation: Arc::new(AtomicU64::new(0)),
        })
    }

    pub fn run(self) -> anyhow::Result<()> {
        let settings = settings::load();
        apply_settings(&self.ui, &settings, settings.timeline_zoom_saved);
        self.auto_refresh
            .store(settings.auto_refresh, Ordering::Relaxed);
        self.refresh_seconds.store(
            u64::try_from(settings.refresh_seconds).unwrap_or(5).max(2),
            Ordering::Relaxed,
        );
        self.state.lock().expect("state poisoned").timeline_days =
            u32::try_from(settings.timeline_days).unwrap_or(0);
        publish_saved(&self.ui);
        fill_saved_form(&self.ui, &settings.last_connection_id);
        self.install_callbacks();
        if !settings.last_connection_id.is_empty() {
            self.ui
                .invoke_saved_login_requested(settings.last_connection_id.clone().into());
        }
        install_window_hooks(&self.ui);
        let ui_weak = self.ui.as_weak();
        self.runtime.spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let updated = ui_weak.upgrade_in_event_loop(|ui| {
                    ui.set_refreshed_ago(refreshed_ago(ui.get_last_refreshed_epoch()).into());
                    if ui.get_time_tip_open() {
                        ui.set_time_tip_text(
                            schedule_phrase(
                                ui.get_time_tip_kind().as_str(),
                                ui.get_time_tip_epoch(),
                            )
                            .into(),
                        );
                    }
                });
                if updated.is_err() {
                    break;
                }
            }
        });
        self.ui.run()?;
        if let Some(token) = self.state.lock().expect("state poisoned").polling.take() {
            token.cancel();
        }
        self.runtime.shutdown_timeout(Duration::from_secs(2));
        Ok(())
    }

    fn install_callbacks(&self) {
        self.install_connection_callbacks();
        self.install_grid_callbacks();
        self.install_command_callbacks();
        self.ui.on_open_coffee_requested(|| {
            platform::open_in_browser("https://buymeacoffee.com/keithr");
        });

        let ui_weak = self.ui.as_weak();
        self.ui.on_persist_requested(move || {
            if let Some(ui) = ui_weak.upgrade() {
                persist_layout(&ui, None);
            }
        });
        let ui_weak = self.ui.as_weak();
        self.ui.on_move_selection_requested(move |delta| {
            if let Some(ui) = ui_weak.upgrade() {
                move_job_selection(&ui, delta);
            }
        });
        let ui_weak = self.ui.as_weak();
        self.ui.on_time_tip_changed(move |kind, epoch| {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_time_tip_text(schedule_phrase(kind.as_str(), epoch).into());
            }
        });
        let ui_weak = self.ui.as_weak();
        Columns::get(&self.ui).on_resize_finished(move || {
            if let Some(ui) = ui_weak.upgrade() {
                persist_layout(&ui, None);
            }
        });
    }

    fn install_connection_callbacks(&self) {
        let ui_weak = self.ui.as_weak();
        let state = self.state.clone();
        let handle = self.runtime.handle().clone();
        let auto_refresh = self.auto_refresh.clone();
        let refresh_seconds = self.refresh_seconds.clone();
        let generation = self.request_generation.clone();

        self.ui.on_connect_requested(
            move |server, port, username, password, windows_auth, trust_certificate, remember| {
                let Some(options) = connection_options(
                    &ui_weak,
                    &server,
                    &port,
                    &username,
                    &password,
                    windows_auth,
                    trust_certificate,
                ) else {
                    return;
                };
                begin_connect(ConnectRequest {
                    options,
                    remember,
                    ui_weak: ui_weak.clone(),
                    state: state.clone(),
                    handle: handle.clone(),
                    auto_refresh: auto_refresh.clone(),
                    refresh_seconds: refresh_seconds.clone(),
                    generation: generation.clone(),
                });
            },
        );

        let ui_weak = self.ui.as_weak();
        let state = self.state.clone();
        let handle = self.runtime.handle().clone();
        let auto_refresh = self.auto_refresh.clone();
        let refresh_seconds = self.refresh_seconds.clone();
        let generation = self.request_generation.clone();
        self.ui.on_saved_login_requested(move |id| {
            let Some(saved) = saved::load()
                .into_iter()
                .find(|connection| connection.id == id.as_str())
            else {
                set_error(&ui_weak, "That saved connection is no longer available.");
                return;
            };
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_form_server(saved.server.clone().into());
                ui.set_form_port(
                    saved
                        .port
                        .map(|port| port.to_string())
                        .unwrap_or_default()
                        .into(),
                );
                ui.set_form_username(saved.username.clone().into());
                ui.set_form_windows_auth(saved.windows_auth);
                ui.set_form_trust_certificate(saved.trust_server_certificate);
            }
            let authentication = if saved.windows_auth {
                if cfg!(target_os = "windows") {
                    Authentication::Integrated
                } else {
                    set_error(
                        &ui_weak,
                        "Windows authentication is not available on macOS. Use a SQL Server login.",
                    );
                    return;
                }
            } else {
                match saved::load_password(&saved.id) {
                    Ok(password) => Authentication::SqlServer {
                        username: saved.username.clone(),
                        password,
                    },
                    Err(error) => {
                        set_error(&ui_weak, &error);
                        return;
                    }
                }
            };
            begin_connect(ConnectRequest {
                options: ConnectionOptions {
                    server: saved.server,
                    port: saved.port,
                    database: "msdb".to_owned(),
                    authentication,
                    trust_server_certificate: saved.trust_server_certificate,
                },
                remember: true,
                ui_weak: ui_weak.clone(),
                state: state.clone(),
                handle: handle.clone(),
                auto_refresh: auto_refresh.clone(),
                refresh_seconds: refresh_seconds.clone(),
                generation: generation.clone(),
            });
        });

        let ui_weak = self.ui.as_weak();
        self.ui.on_saved_delete_requested(move |id| {
            let id = id.to_string();
            let mut connections = saved::load();
            connections.retain(|connection| connection.id != id);
            if let Err(error) = saved::store(&connections) {
                set_error(&ui_weak, &error);
                return;
            }
            saved::delete_password(&id);
            if let Some(ui) = ui_weak.upgrade() {
                publish_saved(&ui);
            }
        });

        let ui_weak = self.ui.as_weak();
        let state = self.state.clone();
        let generation = self.request_generation.clone();
        self.ui.on_disconnect_requested(move || {
            generation.fetch_add(1, Ordering::SeqCst);
            let mut state = state.lock().expect("state poisoned");
            if let Some(token) = state.polling.take() {
                token.cancel();
            }
            state.client = None;
            state.jobs.clear();
            state.selected_job_id.clear();
            state.timeline_history.clear();
            drop(state);
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_connected(false);
                ui.set_busy(false);
                ui.set_error_message("".into());
                ui.set_jobs(empty_job_model());
                ui.set_status_counts(model(Vec::<StatusCountRow>::new()));
                ui.set_timeline_lanes(model(Vec::<TimelineLaneRow>::new()));
                ui.set_history(empty_history_model());
                ui.set_selected_job_id("".into());
                ui.set_selected_job_name("".into());
                ui.set_selected_job_status("".into());
                ui.set_selected_job_step("".into());
                ui.set_last_refreshed("Never".into());
                ui.set_last_refreshed_epoch(0);
                ui.set_refreshed_ago("Not updated yet".into());
                ui.set_updated_tip_open(false);
            }
        });

        let auto_refresh = self.auto_refresh.clone();
        let refresh_seconds = self.refresh_seconds.clone();
        let ui_weak = self.ui.as_weak();
        self.ui
            .on_refresh_settings_changed(move |enabled, seconds| {
                auto_refresh.store(enabled, Ordering::Relaxed);
                refresh_seconds.store(seconds.max(2) as u64, Ordering::Relaxed);
                if let Some(ui) = ui_weak.upgrade() {
                    persist_layout(&ui, None);
                }
            });
    }

    fn install_grid_callbacks(&self) {
        let state = self.state.clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_filters_changed(move |search, status| {
            let rows = {
                let mut state = state.lock().expect("state poisoned");
                state.search = search.to_string();
                state.status_filter = status.to_string();
                visible_jobs(&state)
            };
            set_jobs(&ui_weak, rows);
            republish_timeline(&state, &ui_weak);
        });

        let state = self.state.clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_category_toggled(move |name, checked| {
            let (rows, choices, summary) = {
                let mut state = state.lock().expect("state poisoned");
                let name = name.to_string();
                if checked {
                    if !state
                        .selected_categories
                        .iter()
                        .any(|category| category.eq_ignore_ascii_case(&name))
                    {
                        state.selected_categories.push(name);
                    }
                } else {
                    state
                        .selected_categories
                        .retain(|category| !category.eq_ignore_ascii_case(&name));
                }
                view_state(&state)
            };
            apply_view(&ui_weak, rows, choices, summary);
            republish_timeline(&state, &ui_weak);
        });

        let state = self.state.clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_categories_set_all(move |selected| {
            let (rows, choices, summary) = {
                let mut state = state.lock().expect("state poisoned");
                state.selected_categories = if selected {
                    category_names(&state.jobs)
                } else {
                    Vec::new()
                };
                view_state(&state)
            };
            apply_view(&ui_weak, rows, choices, summary);
            republish_timeline(&state, &ui_weak);
        });

        let state = self.state.clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_sort_requested(move |column| {
            let (rows, ascending) = {
                let mut state = state.lock().expect("state poisoned");
                let next = match column.as_str() {
                    "status" => SortColumn::Status,
                    "last-run" => SortColumn::LastRun,
                    _ => SortColumn::Name,
                };
                if state.sort_column == next {
                    state.sort_ascending = !state.sort_ascending;
                } else {
                    state.sort_column = next;
                    state.sort_ascending = true;
                }
                (visible_jobs(&state), state.sort_ascending)
            };
            let _ = ui_weak.upgrade_in_event_loop(move |ui| {
                ui.set_jobs(job_model(rows));
                ui.set_sort_column(column);
                ui.set_sort_ascending(ascending);
            });
            republish_timeline(&state, &ui_weak);
        });

        let state = self.state.clone();
        let ui_weak = self.ui.as_weak();
        let handle = self.runtime.handle().clone();
        self.ui.on_timeline_requested(move |days| {
            let days = u32::try_from(days).unwrap_or(0);
            let client = {
                let mut state = state.lock().expect("state poisoned");
                state.timeline_days = days;
                state.client.clone()
            };
            republish_timeline(&state, &ui_weak);
            if let Some(ui) = ui_weak.upgrade() {
                persist_layout(&ui, None);
            }
            if days == 0 {
                return;
            }
            let Some(client) = client else {
                return;
            };
            let state = state.clone();
            let ui_weak = ui_weak.clone();
            handle.spawn(async move {
                refresh_timeline(&client, &state, &ui_weak).await;
            });
        });

        let state = self.state.clone();
        let ui_weak = self.ui.as_weak();
        let handle = self.runtime.handle().clone();
        self.ui.on_job_selected(move |job_id, _job_name, _status| {
            let (client, active) = {
                let mut state = state.lock().expect("state poisoned");
                state.selected_job_id = job_id.to_string();
                let active = state
                    .jobs
                    .iter()
                    .find(|job| job.id == state.selected_job_id)
                    .and_then(Job::active_run);
                (state.client.clone(), active)
            };
            let Some(client) = client else { return };
            let selected_id = job_id.to_string();
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_history_loading(true);
                ui.set_history(empty_history_model());
            }
            let state = state.clone();
            let ui_weak = ui_weak.clone();
            handle.spawn(async move {
                publish_job_history(&client, &state, &ui_weak, &selected_id, active, true).await;
            });
        });

        let state = self.state.clone();
        let ui_weak = self.ui.as_weak();
        let handle = self.runtime.handle().clone();
        self.ui.on_steps_requested(move |job_id| {
            let client = state.lock().expect("state poisoned").client.clone();
            let Some(client) = client else { return };
            let job_id = job_id.to_string();
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_job_steps(model(Vec::new()));
                ui.set_steps_loading(true);
            }
            let ui_weak = ui_weak.clone();
            handle.spawn(async move {
                let result = sql::fetch_steps(&client, &job_id).await;
                let _ = ui_weak.upgrade_in_event_loop(move |ui| {
                    if ui.get_job_menu_id().as_str() != job_id {
                        return;
                    }
                    ui.set_steps_loading(false);
                    match result {
                        Ok(steps) => ui.set_job_steps(model(
                            steps
                                .into_iter()
                                .map(|step| JobStepRow {
                                    label: format!("Step {}: {}", step.step_id, step.name).into(),
                                    name: step.name.into(),
                                })
                                .collect(),
                        )),
                        Err(error) => {
                            ui.set_error_message(error.to_string().into());
                            ui.set_job_steps(model(Vec::new()));
                        }
                    }
                });
            });
        });

        let state = self.state.clone();
        let ui_weak = self.ui.as_weak();
        let generation = self.request_generation.clone();
        let handle = self.runtime.handle().clone();
        self.ui.on_refresh_requested(move || {
            let client = state.lock().expect("state poisoned").client.clone();
            if let Some(client) = client {
                let state = state.clone();
                let ui_weak = ui_weak.clone();
                let generation = generation.clone();
                set_busy(&ui_weak, true);
                handle.spawn(async move {
                    refresh_jobs(client, state, ui_weak, generation).await;
                });
            }
        });
    }

    fn install_command_callbacks(&self) {
        let state = self.state.clone();
        let generation = self.request_generation.clone();
        let handle = self.runtime.handle().clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_start_requested(move |job_id| {
            spawn_control(
                &state,
                &ui_weak,
                &generation,
                &handle,
                job_id.to_string(),
                ControlAction::Start,
            );
        });

        let state = self.state.clone();
        let generation = self.request_generation.clone();
        let handle = self.runtime.handle().clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_stop_requested(move |job_id| {
            spawn_control(
                &state,
                &ui_weak,
                &generation,
                &handle,
                job_id.to_string(),
                ControlAction::Stop,
            );
        });

        let state = self.state.clone();
        let generation = self.request_generation.clone();
        let handle = self.runtime.handle().clone();
        let ui_weak = self.ui.as_weak();
        self.ui
            .on_start_at_step_requested(move |job_id, step_name| {
                spawn_control(
                    &state,
                    &ui_weak,
                    &generation,
                    &handle,
                    job_id.to_string(),
                    ControlAction::StartAtStep(step_name.to_string()),
                );
            });

        let state = self.state.clone();
        let generation = self.request_generation.clone();
        let handle = self.runtime.handle().clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_set_enabled_requested(move |job_id, enabled| {
            spawn_control(
                &state,
                &ui_weak,
                &generation,
                &handle,
                job_id.to_string(),
                ControlAction::SetEnabled(enabled),
            );
        });

        let state = self.state.clone();
        let generation = self.request_generation.clone();
        let handle = self.runtime.handle().clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_refresh_job_requested(move |job_id| {
            spawn_control(
                &state,
                &ui_weak,
                &generation,
                &handle,
                job_id.to_string(),
                ControlAction::Refresh,
            );
        });
    }
}

enum ControlAction {
    Start,
    Stop,
    StartAtStep(String),
    SetEnabled(bool),
    Refresh,
}

fn spawn_control(
    state: &Arc<Mutex<State>>,
    ui_weak: &Weak<MainWindow>,
    generation: &Arc<AtomicU64>,
    handle: &Handle,
    job_id: String,
    action: ControlAction,
) {
    let client = state.lock().expect("state poisoned").client.clone();
    let Some(client) = client else { return };
    if let Err(message) = action_allowed(state, &job_id, &action) {
        set_error(ui_weak, &message);
        return;
    }
    let state = state.clone();
    let ui_weak = ui_weak.clone();
    let generation = generation.clone();
    set_busy(&ui_weak, true);
    set_error(&ui_weak, "");
    handle.spawn(async move {
        let result = match &action {
            ControlAction::Start => sql::start_job(&client, &job_id)
                .await
                .map(|()| "Start request accepted"),
            ControlAction::Stop => sql::stop_job(&client, &job_id)
                .await
                .map(|()| "Stop request accepted"),
            ControlAction::StartAtStep(step_name) => {
                sql::start_job_at_step(&client, &job_id, step_name)
                    .await
                    .map(|()| "Start request accepted")
            }
            ControlAction::SetEnabled(enabled) => sql::set_job_enabled(&client, &job_id, *enabled)
                .await
                .map(|()| {
                    if *enabled {
                        "Job enabled"
                    } else {
                        "Job disabled"
                    }
                }),
            ControlAction::Refresh => Ok("Job refreshed"),
        };
        match result {
            Ok(message) => {
                set_status(&ui_weak, message);
                if matches!(action, ControlAction::Refresh) {
                    refresh_one_job(client, state, ui_weak, generation, job_id).await;
                } else {
                    refresh_jobs(client, state, ui_weak, generation).await;
                }
            }
            Err(error) => {
                set_error(&ui_weak, &error.to_string());
                set_busy(&ui_weak, false);
            }
        }
    });
}

fn action_allowed(
    state: &Arc<Mutex<State>>,
    job_id: &str,
    action: &ControlAction,
) -> Result<(), String> {
    let state = state.lock().expect("state poisoned");
    let Some(job) = state.jobs.iter().find(|job| job.id == job_id) else {
        return Err("That job is no longer in the list.".to_owned());
    };
    let commands = command_availability(job.enabled, job.running, false);
    let allowed = match action {
        ControlAction::Start | ControlAction::StartAtStep(_) => commands.start,
        ControlAction::Stop => commands.stop,
        ControlAction::SetEnabled(true) => commands.enable,
        ControlAction::SetEnabled(false) => commands.disable,
        ControlAction::Refresh => commands.refresh,
    };
    if allowed {
        Ok(())
    } else {
        Err("That action is not available for this job right now.".to_owned())
    }
}

struct ConnectRequest {
    options: ConnectionOptions,
    remember: bool,
    ui_weak: Weak<MainWindow>,
    state: Arc<Mutex<State>>,
    handle: Handle,
    auto_refresh: Arc<AtomicBool>,
    refresh_seconds: Arc<AtomicU64>,
    generation: Arc<AtomicU64>,
}

fn connection_options(
    ui_weak: &Weak<MainWindow>,
    server: &str,
    port: &str,
    username: &str,
    password: &str,
    windows_auth: bool,
    trust_certificate: bool,
) -> Option<ConnectionOptions> {
    if windows_auth && !cfg!(target_os = "windows") {
        set_error(
            ui_weak,
            "Windows authentication is not available on macOS. Use a SQL Server login.",
        );
        return None;
    }
    let port = if port.trim().is_empty() {
        None
    } else {
        match port.trim().parse::<u16>() {
            Ok(port) => Some(port),
            Err(_) => {
                set_error(ui_weak, "Port must be a number from 1 to 65535.");
                return None;
            }
        }
    };
    Some(ConnectionOptions {
        server: server.to_string(),
        port,
        database: "msdb".to_owned(),
        authentication: if windows_auth {
            Authentication::Integrated
        } else {
            Authentication::SqlServer {
                username: if cfg!(target_os = "macos") {
                    username.trim().to_string()
                } else {
                    username.to_string()
                },
                password: Zeroizing::new(password.to_string()),
            }
        },
        trust_server_certificate: trust_certificate,
    })
}

fn begin_connect(request: ConnectRequest) {
    let ConnectRequest {
        options,
        remember,
        ui_weak,
        state,
        handle,
        auto_refresh,
        refresh_seconds,
        generation,
    } = request;
    set_busy(&ui_weak, true);
    set_error(&ui_weak, "");
    let label = options.server.clone();
    handle.clone().spawn(async move {
        match sql::connect(&options).await {
            Ok(client) => {
                let remembered = remember.then(|| remember_connection(&options));
                let save_error = remembered
                    .as_ref()
                    .and_then(|result| result.as_ref().err().cloned());
                let last_id = remembered
                    .and_then(Result::ok)
                    .or_else(|| saved_connection_id(&options));
                let token = CancellationToken::new();
                {
                    let mut state = state.lock().expect("state poisoned");
                    if let Some(previous) = state.polling.replace(token.clone()) {
                        previous.cancel();
                    }
                    state.client = Some(client.clone());
                    state.jobs.clear();
                }
                let weak = ui_weak.clone();
                let save_error_for_ui = save_error.clone();
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.set_connected(true);
                    ui.set_busy(false);
                    ui.set_error_message(save_error_for_ui.unwrap_or_default().into());
                    ui.set_server_label(label.into());
                    ui.set_status_message("Connected".into());
                    ui.set_last_refreshed("Never".into());
                    ui.set_last_refreshed_epoch(0);
                    ui.set_refreshed_ago("Not updated yet".into());
                    if save_error.is_none() {
                        publish_saved(&ui);
                    }
                    persist_layout(&ui, last_id);
                });

                refresh_jobs(
                    client.clone(),
                    state.clone(),
                    ui_weak.clone(),
                    generation.clone(),
                )
                .await;
                spawn_polling(
                    &handle,
                    PollingTask {
                        client,
                        state,
                        ui_weak,
                        token,
                        auto_refresh,
                        refresh_seconds,
                        generation,
                    },
                );
            }
            Err(error) => {
                set_error(&ui_weak, &error.to_string());
                set_busy(&ui_weak, false);
            }
        }
    });
}

fn saved_connection_id(options: &ConnectionOptions) -> Option<String> {
    let (windows_auth, username) = match &options.authentication {
        Authentication::Integrated => (true, String::new()),
        Authentication::SqlServer { username, .. } => (false, username.clone()),
    };
    saved::load()
        .into_iter()
        .find(|saved| {
            saved.server.eq_ignore_ascii_case(&options.server)
                && saved.port == options.port
                && saved.windows_auth == windows_auth
                && saved.username.eq_ignore_ascii_case(&username)
        })
        .map(|saved| saved.id)
}

fn remember_connection(options: &ConnectionOptions) -> Result<String, String> {
    let (windows_auth, username, password) = match &options.authentication {
        Authentication::Integrated => (true, String::new(), None),
        Authentication::SqlServer { username, password } => {
            (false, username.clone(), Some(password.clone()))
        }
    };
    let mut connections = saved::load();
    let id = saved::upsert(
        &mut connections,
        SavedConnection {
            id: String::new(),
            server: options.server.clone(),
            port: options.port,
            windows_auth,
            username,
            trust_server_certificate: options.trust_server_certificate,
        },
    );
    saved::store(&connections)?;
    if let Some(password) = password {
        saved::store_password(&id, password.as_str())?;
    }
    Ok(id)
}

fn publish_saved(ui: &MainWindow) {
    let rows = saved::load()
        .into_iter()
        .map(|connection| SavedConnectionRow {
            id: connection.id.clone().into(),
            label: connection.label().into(),
            detail: connection.detail().into(),
        })
        .collect();
    ui.set_saved_connections(model(rows));
}

fn spawn_polling(handle: &Handle, task: PollingTask) {
    handle.spawn(async move {
        loop {
            let delay = Duration::from_secs(task.refresh_seconds.load(Ordering::Relaxed).max(2));
            tokio::select! {
                _ = task.token.cancelled() => break,
                _ = tokio::time::sleep(delay) => {
                    if task.auto_refresh.load(Ordering::Relaxed) {
                        refresh_jobs(
                            task.client.clone(),
                            task.state.clone(),
                            task.ui_weak.clone(),
                            task.generation.clone(),
                        ).await;
                    }
                }
            }
        }
    });
}

async fn refresh_jobs(
    client: SharedClient,
    state: Arc<Mutex<State>>,
    ui_weak: Weak<MainWindow>,
    generation: Arc<AtomicU64>,
) {
    let request = generation.fetch_add(1, Ordering::SeqCst) + 1;
    match sql::fetch_jobs(&client).await {
        Ok(jobs) => {
            if request != generation.load(Ordering::SeqCst) {
                return;
            }
            let total = jobs.len();
            let (rows, choices, summary, selected, counts) = {
                let mut state = state.lock().expect("state poisoned");
                state.jobs = jobs;
                let counts = status_counts(&state.jobs);
                let (rows, choices, summary) = view_state(&state);
                let selected = selected_snapshot(&state);
                (rows, choices, summary, selected, counts)
            };
            let now = Local::now();
            let refreshed = now.format("%Y-%m-%d %-I:%M:%S %p").to_string();
            let epoch = i32::try_from(now.timestamp()).unwrap_or(0);
            let status = format!("Connected - {total} jobs ({} shown)", rows.len());
            let history_target = selected.clone();
            let _ = ui_weak.upgrade_in_event_loop(move |ui| {
                ui.set_jobs(job_model(rows));
                ui.set_status_counts(status_count_model(counts));
                set_category_view(&ui, choices, summary);
                apply_selected(&ui, selected.as_ref());
                ui.set_last_refreshed(refreshed.into());
                ui.set_last_refreshed_epoch(epoch);
                ui.set_refreshed_ago(relative_update(0).into());
                ui.set_status_message(status.into());
                ui.set_error_message("".into());
                ui.set_busy(false);
            });
            if let Some(selected) = history_target {
                let job_id = selected.id.clone();
                publish_job_history(&client, &state, &ui_weak, &job_id, selected.active, false)
                    .await;
            }
            refresh_timeline(&client, &state, &ui_weak).await;
        }
        Err(error) => {
            if request == generation.load(Ordering::SeqCst) {
                set_error(&ui_weak, &error.to_string());
                set_status(&ui_weak, "Refresh failed — showing last good data");
                set_busy(&ui_weak, false);
            }
        }
    }
}

fn visible_jobs(state: &State) -> Vec<Job> {
    filter_and_sort(
        &state.jobs,
        &state.search,
        &state.status_filter,
        &state.selected_categories,
        state.sort_column,
        state.sort_ascending,
    )
}

fn view_state(state: &State) -> (Vec<Job>, Vec<CategoryChoice>, String) {
    (
        visible_jobs(state),
        category_choices(state),
        category_summary(&state.selected_categories),
    )
}

fn category_names(jobs: &[Job]) -> Vec<String> {
    let mut names: Vec<String> = jobs
        .iter()
        .map(|job| job.category.clone())
        .filter(|category| !category.is_empty())
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    if !names.iter().any(|name| name.eq_ignore_ascii_case("ETL")) {
        names.push("ETL".to_owned());
        names.sort_by_key(|name| name.to_lowercase());
    }
    names
}

fn category_choices(state: &State) -> Vec<CategoryChoice> {
    category_names(&state.jobs)
        .into_iter()
        .map(|name| {
            let checked = state
                .selected_categories
                .iter()
                .any(|category| category.eq_ignore_ascii_case(&name));
            CategoryChoice {
                name: name.into(),
                checked,
            }
        })
        .collect()
}

fn apply_view(
    ui_weak: &Weak<MainWindow>,
    jobs: Vec<Job>,
    choices: Vec<CategoryChoice>,
    summary: String,
) {
    let _ = ui_weak.clone().upgrade_in_event_loop(move |ui| {
        ui.set_jobs(job_model(jobs));
        set_category_view(&ui, choices, summary);
    });
}

fn set_category_view(ui: &MainWindow, choices: Vec<CategoryChoice>, summary: String) {
    ui.set_category_filter_active(choices.iter().any(|choice| !choice.checked));
    ui.set_categories(model(choices));
    ui.set_category_label(summary.into());
}

#[derive(Clone)]
struct SelectedSnapshot {
    id: String,
    name: String,
    status: String,
    step: String,
    enabled: bool,
    running: bool,
    active: Option<ActiveRun>,
}

fn selected_snapshot(state: &State) -> Option<SelectedSnapshot> {
    let job = state
        .jobs
        .iter()
        .find(|job| job.id == state.selected_job_id)?;
    Some(SelectedSnapshot {
        id: job.id.clone(),
        name: job.name.clone(),
        status: job.status.label().to_owned(),
        step: job.current_step.clone(),
        enabled: job.enabled,
        running: job.running,
        active: job.active_run(),
    })
}

fn apply_selected(ui: &MainWindow, selected: Option<&SelectedSnapshot>) {
    if let Some(selected) = selected {
        ui.set_selected_job_name(selected.name.clone().into());
        ui.set_selected_job_status(selected.status.clone().into());
        ui.set_selected_job_step(selected.step.clone().into());
        ui.set_job_menu_enabled(selected.enabled);
        ui.set_job_menu_running(selected.running);
    }
}

async fn publish_job_history(
    client: &SharedClient,
    state: &Arc<Mutex<State>>,
    ui_weak: &Weak<MainWindow>,
    job_id: &str,
    active: Option<ActiveRun>,
    report_errors: bool,
) {
    match sql::fetch_history(client, job_id).await {
        Ok(entries) => {
            let still_selected = state.lock().expect("state poisoned").selected_job_id == job_id;
            if still_selected {
                let runs = include_active_run(group_runs(entries, MAX_HISTORY_RUNS), active);
                let _ = ui_weak.upgrade_in_event_loop(move |ui| {
                    ui.set_history(history_model(runs));
                    ui.set_history_loading(false);
                });
            }
        }
        Err(error) => {
            if report_errors {
                set_error(ui_weak, &error.to_string());
            }
            let _ = ui_weak.upgrade_in_event_loop(|ui| ui.set_history_loading(false));
        }
    }
}

async fn refresh_one_job(
    client: SharedClient,
    state: Arc<Mutex<State>>,
    ui_weak: Weak<MainWindow>,
    generation: Arc<AtomicU64>,
    job_id: String,
) {
    let request = generation.fetch_add(1, Ordering::SeqCst) + 1;
    match sql::fetch_job(&client, &job_id).await {
        Ok(job) => {
            if request != generation.load(Ordering::SeqCst) {
                return;
            }
            let (rows, choices, summary, selected, gone, counts) = {
                let mut state = state.lock().expect("state poisoned");
                match job {
                    Some(job) => {
                        if let Some(existing) = state.jobs.iter_mut().find(|item| item.id == job.id)
                        {
                            *existing = job;
                        } else {
                            state.jobs.push(job);
                        }
                    }
                    None => state.jobs.retain(|item| item.id != job_id),
                }
                let gone = !state.jobs.iter().any(|item| item.id == job_id);
                if gone && state.selected_job_id == job_id {
                    state.selected_job_id.clear();
                }
                let selected = selected_snapshot(&state);
                let counts = status_counts(&state.jobs);
                let (rows, choices, summary) = view_state(&state);
                (rows, choices, summary, selected, gone, counts)
            };
            let history_target = selected.clone();
            let _ = ui_weak.upgrade_in_event_loop(move |ui| {
                ui.set_jobs(job_model(rows));
                ui.set_status_counts(status_count_model(counts));
                set_category_view(&ui, choices, summary);
                if gone {
                    ui.set_selected_job_id("".into());
                    ui.set_selected_job_name("".into());
                    ui.set_selected_job_status("".into());
                    ui.set_selected_job_step("".into());
                    ui.set_history(empty_history_model());
                } else {
                    apply_selected(&ui, selected.as_ref());
                }
                ui.set_busy(false);
            });
            republish_timeline(&state, &ui_weak);
            if let Some(selected) = history_target.filter(|selected| selected.id == job_id) {
                let job_id = selected.id.clone();
                publish_job_history(&client, &state, &ui_weak, &job_id, selected.active, true)
                    .await;
            }
        }
        Err(error) => {
            if request == generation.load(Ordering::SeqCst) {
                set_error(&ui_weak, &error.to_string());
                set_busy(&ui_weak, false);
            }
        }
    }
}

fn job_model(jobs: Vec<Job>) -> ModelRc<JobRow> {
    model(
        jobs.into_iter()
            .map(|job| JobRow {
                id: job.id.into(),
                name: job.name.into(),
                enabled: job.enabled,
                running: job.running,
                status: job.status.label().into(),
                current_step: job.current_step.into(),
                last_run: format_timestamp(&job.last_run).into(),
                last_run_epoch: local_epoch(&job.last_run),
                duration: format_duration(job.last_duration_seconds).into(),
                next_run: format_timestamp(&job.next_run).into(),
                next_run_epoch: local_epoch(&job.next_run),
                owner: job.owner.into(),
                category: job.category.into(),
            })
            .collect(),
    )
}

fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(rows)).into()
}

fn empty_job_model() -> ModelRc<JobRow> {
    model(Vec::new())
}

fn empty_history_model() -> ModelRc<HistoryRunRow> {
    model(Vec::new())
}

fn history_model(runs: Vec<HistoryRun>) -> ModelRc<HistoryRunRow> {
    model(
        runs.into_iter()
            .map(|run| {
                let running = run.outcome == "Running";
                HistoryRunRow {
                    run_at: format_timestamp(&run.run_at).into(),
                    outcome: run.outcome.into(),
                    duration: format_duration(run.duration_seconds).into(),
                    message: run.message.into(),
                    expanded: running,
                    description_open: false,
                    steps: model(
                        run.steps
                            .into_iter()
                            .map(|step| HistoryRow {
                                run_at: format_timestamp(&step.run_at).into(),
                                step: step.step.into(),
                                outcome: step.outcome.clone().into(),
                                duration: if step.outcome == "Running" {
                                    SharedString::default()
                                } else {
                                    format_duration(step.duration_seconds).into()
                                },
                                message: step.message.into(),
                                details_open: false,
                            })
                            .collect(),
                    ),
                }
            })
            .collect(),
    )
}

struct TimelineWindow {
    start: NaiveDateTime,
    end: NaiveDateTime,
    now: NaiveDateTime,
}

impl TimelineWindow {
    fn for_days(days: u32) -> Self {
        let now = Local::now().naive_local();
        let back = now - chrono::Duration::hours(i64::from(days) * 24);
        let start = back
            .with_minute(0)
            .and_then(|value| value.with_second(0))
            .and_then(|value| value.with_nanosecond(0))
            .unwrap_or(back);
        let end = start + chrono::Duration::hours(i64::from(days) * 24 + 2);
        Self { start, end, now }
    }

    fn since_date(&self) -> i32 {
        let date = self.start.date();
        date.year() * 10_000
            + i32::try_from(date.month()).unwrap_or(0) * 100
            + i32::try_from(date.day()).unwrap_or(0)
    }

    fn day_labels(&self) -> Vec<SharedString> {
        let mut labels = Vec::new();
        let mut day = self.start.date();
        while day <= self.end.date() {
            labels.push(day.format("%a %b %-d").to_string().into());
            day = day.succ_opt().unwrap_or(day);
            if labels.len() > 400 {
                break;
            }
        }
        labels
    }
}

fn timeline_lanes(state: &State, window: &TimelineWindow) -> Vec<TimelineLane> {
    build_timeline(
        &visible_jobs(state),
        &state.timeline_history,
        window.start,
        window.end,
        window.now,
    )
}

/// Rebuilds the timeline from history already loaded, for filter and sort changes.
fn republish_timeline(state: &Arc<Mutex<State>>, ui_weak: &Weak<MainWindow>) {
    let (days, lanes, window) = {
        let state = state.lock().expect("state poisoned");
        if state.timeline_days == 0 {
            return;
        }
        let window = TimelineWindow::for_days(state.timeline_days);
        (state.timeline_days, timeline_lanes(&state, &window), window)
    };
    publish_timeline(ui_weak, days, lanes, window);
}

async fn refresh_timeline(
    client: &SharedClient,
    state: &Arc<Mutex<State>>,
    ui_weak: &Weak<MainWindow>,
) {
    let days = state.lock().expect("state poisoned").timeline_days;
    if days == 0 {
        return;
    }
    let _ = ui_weak.upgrade_in_event_loop(|ui| ui.set_timeline_loading(true));
    let window = TimelineWindow::for_days(days);
    match sql::fetch_timeline(client, window.since_date()).await {
        Ok(history) => {
            let lanes = {
                let mut state = state.lock().expect("state poisoned");
                if state.timeline_days != days {
                    return;
                }
                state.timeline_history = history;
                timeline_lanes(&state, &window)
            };
            publish_timeline(ui_weak, days, lanes, window);
        }
        Err(error) => {
            set_error(ui_weak, &error.to_string());
            let _ = ui_weak.upgrade_in_event_loop(|ui| ui.set_timeline_loading(false));
        }
    }
}

fn publish_timeline(
    ui_weak: &Weak<MainWindow>,
    days: u32,
    lanes: Vec<TimelineLane>,
    window: TimelineWindow,
) {
    let span = (window.end - window.start).num_seconds() as f32;
    let now = (window.now - window.start).num_seconds() as f32;
    let start_hour = i32::try_from(window.start.hour()).unwrap_or(0);
    let labels = window.day_labels();
    let _ = ui_weak.upgrade_in_event_loop(move |ui| {
        if ui.get_timeline_days() != i32::try_from(days).unwrap_or(0) {
            return;
        }
        ui.set_timeline_span(span);
        ui.set_timeline_now(now);
        ui.set_timeline_start_hour(start_hour);
        ui.set_timeline_day_labels(model(labels));
        ui.set_timeline_lanes(timeline_model(lanes));
        ui.set_timeline_loading(false);
    });
}

fn timeline_model(lanes: Vec<TimelineLane>) -> ModelRc<TimelineLaneRow> {
    model(
        lanes
            .into_iter()
            .map(|lane| TimelineLaneRow {
                id: lane.job_id.into(),
                name: lane.name.into(),
                status: lane.status.into(),
                current_step: lane.current_step.into(),
                bars: model(
                    lane.bars
                        .into_iter()
                        .map(|bar| TimelineBarRow {
                            start: bar.start,
                            length: bar.length,
                            status: bar.status.into(),
                            label: bar.label.into(),
                            detail: bar.detail.into(),
                            is_step: bar.is_step,
                        })
                        .collect(),
                ),
            })
            .collect(),
    )
}

fn status_count_model(counts: Vec<(String, i32)>) -> ModelRc<StatusCountRow> {
    model(
        counts
            .into_iter()
            .map(|(label, count)| StatusCountRow {
                label: label.into(),
                count,
            })
            .collect(),
    )
}

fn set_jobs(ui_weak: &Weak<MainWindow>, jobs: Vec<Job>) {
    let _ = ui_weak
        .clone()
        .upgrade_in_event_loop(move |ui| ui.set_jobs(job_model(jobs)));
}

fn set_error(ui_weak: &Weak<MainWindow>, message: &str) {
    let message: SharedString = message.into();
    let _ = ui_weak
        .clone()
        .upgrade_in_event_loop(move |ui| ui.set_error_message(message));
}

fn set_status(ui_weak: &Weak<MainWindow>, message: &str) {
    let message: SharedString = message.into();
    let _ = ui_weak
        .clone()
        .upgrade_in_event_loop(move |ui| ui.set_status_message(message));
}

fn set_busy(ui_weak: &Weak<MainWindow>, busy: bool) {
    let _ = ui_weak
        .clone()
        .upgrade_in_event_loop(move |ui| ui.set_busy(busy));
}

#[cfg(test)]
mod selection_tests {
    use super::move_job_selection;
    use crate::{JobRow, MainWindow};
    use slint::{ModelRc, VecModel};

    fn job_row(id: &str, name: &str) -> JobRow {
        JobRow {
            id: id.into(),
            name: name.into(),
            enabled: true,
            running: false,
            status: "Succeeded".into(),
            current_step: String::new().into(),
            last_run: String::new().into(),
            last_run_epoch: 0,
            duration: String::new().into(),
            next_run: String::new().into(),
            next_run_epoch: 0,
            owner: String::new().into(),
            category: "ETL".into(),
        }
    }

    #[test]
    fn arrows_move_the_highlighted_job() {
        let ui = MainWindow::new().expect("window");
        ui.set_connected(true);
        ui.set_timeline_days(0);
        ui.set_jobs(ModelRc::new(VecModel::from(vec![
            job_row("a", "Alpha"),
            job_row("b", "Beta"),
            job_row("c", "Gamma"),
        ])));

        move_job_selection(&ui, 1);
        assert_eq!(ui.get_selected_job_id().as_str(), "a");
        move_job_selection(&ui, 1);
        assert_eq!(ui.get_selected_job_id().as_str(), "b");
        move_job_selection(&ui, -1);
        assert_eq!(ui.get_selected_job_id().as_str(), "a");
        move_job_selection(&ui, -1);
        assert_eq!(ui.get_selected_job_id().as_str(), "a");
        move_job_selection(&ui, 1);
        move_job_selection(&ui, 1);
        move_job_selection(&ui, 1);
        assert_eq!(ui.get_selected_job_id().as_str(), "c");
    }
}
