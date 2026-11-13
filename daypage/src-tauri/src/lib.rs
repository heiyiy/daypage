mod db;
mod desktop;
mod geometry;
mod maintenance;
mod storage;

use std::sync::{Arc, Mutex};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};

struct Database(Arc<Mutex<rusqlite::Connection>>);

// Run disk access off the UI thread. All windows share this connection and lock.
async fn access<T: Send + 'static>(
    state: &Database,
    operation: impl FnOnce(&mut rusqlite::Connection) -> db::Result<T> + Send + 'static,
) -> Result<T, String> {
    let db = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = db
            .lock()
            .map_err(|_| "数据库不可用，请重新启动".to_string())?;
        operation(&mut conn).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn write_access<T: Send + 'static>(
    state: &Database,
    label: &'static str,
    operation: impl FnOnce(&mut rusqlite::Connection) -> db::Result<T> + Send + 'static,
) -> Result<T, String> {
    access(state, move |conn| {
        let value = storage::tracked(conn, label, operation)?;
        maintenance::refresh_backup(conn);
        Ok(value)
    })
    .await
}

#[tauri::command]
async fn get_day(
    date: String,
    state: tauri::State<'_, Database>,
) -> Result<Vec<db::DayTask>, String> {
    access(&state, move |conn| db::get_day(conn, &date)).await
}
#[tauri::command]
async fn create_task(
    date: String,
    title: String,
    state: tauri::State<'_, Database>,
    app: tauri::AppHandle,
) -> Result<db::DayTask, String> {
    let task = write_access(&state, "添加待办", move |conn| {
        db::create_task(conn, &date, &title)
    })
    .await?;
    let _ = app.emit("tasks-changed", ());
    Ok(task)
}
#[tauri::command]
async fn get_deleted(
    date: String,
    state: tauri::State<'_, Database>,
) -> Result<Vec<db::DayTask>, String> {
    access(&state, move |conn| db::get_deleted(conn, &date)).await
}
#[tauri::command]
async fn edit_entry(
    id: String,
    version: i64,
    action: String,
    title: Option<String>,
    state: tauri::State<'_, Database>,
    app: tauri::AppHandle,
) -> Result<Vec<db::DayTask>, String> {
    let rows = write_access(&state, "修改待办", move |conn| {
        db::edit_entry(conn, &id, version, &action, title.as_deref())
    })
    .await?;
    let _ = app.emit("tasks-changed", ());
    Ok(rows)
}
#[tauri::command]
async fn set_completed(
    id: String,
    completed: bool,
    version: i64,
    state: tauri::State<'_, Database>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    write_access(&state, "勾选待办", move |conn| {
        db::set_completed(conn, &id, completed, version)
    })
    .await?;
    let _ = app.emit("tasks-changed", ());
    Ok(())
}
#[tauri::command]
async fn get_note(date: String, state: tauri::State<'_, Database>) -> Result<db::Note, String> {
    access(&state, move |conn| db::get_note(conn, &date)).await
}
#[tauri::command]
async fn save_note(
    date: String,
    body: String,
    version: i64,
    state: tauri::State<'_, Database>,
    app: tauri::AppHandle,
) -> Result<i64, String> {
    let version = write_access(&state, "日记修改", move |conn| {
        db::save_note(conn, &date, &body, version)
    })
    .await?;
    let _ = app.emit("notes-changed", ());
    Ok(version)
}
#[tauri::command]
async fn get_month(
    year: i32,
    month: u32,
    state: tauri::State<'_, Database>,
) -> Result<Vec<db::DaySummary>, String> {
    access(&state, move |conn| db::get_month(conn, year, month)).await
}
#[tauri::command]
fn hide_widget(app: tauri::AppHandle) {
    desktop::set_hidden(&app, true);
}
fn show(app: &tauri::AppHandle, label: &str) -> Result<(), String> {
    if label == "main" {
        desktop::set_hidden(app, false);
        return Ok(());
    }
    let window = if let Some(window) = app.get_webview_window(label) {
        window
    } else if label == "journal" {
        let config = app
            .config()
            .app
            .windows
            .iter()
            .find(|config| config.label == "journal")
            .ok_or("找不到日记窗口配置")?;
        let window = tauri::WebviewWindowBuilder::from_config(app, config)
            .map_err(|e| e.to_string())?
            .visible(false)
            .build()
            .map_err(|e| e.to_string())?;
        app.state::<geometry::Geometry>().restore(&window);
        window
    } else {
        return Err("找不到窗口".into());
    };
    window.unminimize().map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}
#[tauri::command]
async fn open_journal(app: tauri::AppHandle) -> Result<(), String> {
    show(&app, "journal")
}
#[tauri::command]
fn show_widget(app: tauri::AppHandle) {
    let _ = show(&app, "main");
}
#[tauri::command]
async fn quit_app(app: tauri::AppHandle) -> Result<(), String> {
    maintenance::request_quit(app).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(maintenance::DataGate::default())
        .manage(desktop::Desktop::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            let _ = show(app, "main");
        }))
        .setup(|app| {
            let directory = std::env::var_os("DAYPAGE_DATA_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or(app.path().app_data_dir()?);
            std::fs::create_dir_all(&directory)?;
            let conn = db::open(&directory.join("daypage.sqlite3"))?;
            maintenance::refresh_backup(&conn);
            app.manage(Database(Arc::new(Mutex::new(conn))));
            app.manage(geometry::Geometry::open(
                directory.join("window-layout.json"),
            ));
            if let Some(window) = app.get_webview_window("main") {
                app.state::<geometry::Geometry>().restore(&window);
            }
            let widget = MenuItem::with_id(app, "widget", "待办", true, None::<&str>)?;
            let journal = MenuItem::with_id(app, "journal", "日记 / 日历", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&widget, &journal, &quit])?;
            let mut tray = TrayIconBuilder::new()
                .tooltip("日页")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "widget" => {
                        let _ = show(app, "main");
                    }
                    "journal" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = show(&app, "journal");
                        });
                    }
                    "quit" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            if maintenance::request_quit(app.clone()).await.is_err() {
                                let _ = show(&app, "main");
                            }
                        });
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let _ = show(tray.app_handle(), "main");
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            desktop::start(app.handle())?;
            Ok(())
        })
        .on_window_event(geometry::event)
        .invoke_handler(tauri::generate_handler![
            maintenance::maintenance_ack,
            maintenance::get_task_meta,
            maintenance::get_backlog,
            maintenance::carry_task,
            maintenance::undo_change,
            maintenance::get_maintenance,
            maintenance::file_action,
            maintenance::set_autostart,
            maintenance::check_backup,
            get_day,
            create_task,
            get_deleted,
            edit_entry,
            set_completed,
            get_note,
            save_note,
            get_month,
            hide_widget,
            open_journal,
            show_widget,
            quit_app
        ])
        .build(tauri::generate_context!())
        .expect("日页启动失败")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                let state = app.state::<geometry::Geometry>();
                for label in ["main", "journal"] {
                    if let Some(window) = app.get_webview_window(label) {
                        state.capture(&window.as_ref().window());
                    }
                }
                state.save();
            }
        });
}
