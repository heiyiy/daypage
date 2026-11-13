use crate::{Database, access, db, storage, write_access};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::sync::{Mutex, mpsc};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

type Reply = (String, Option<String>);
#[derive(Default)]
pub struct DataGate(Mutex<Option<(String, mpsc::Sender<Reply>)>>);
struct Prepared(tauri::AppHandle);
impl Drop for Prepared {
    fn drop(&mut self) {
        if let Ok(mut gate) = self.0.state::<DataGate>().0.lock() {
            *gate = None;
        }
        let _ = self.0.emit("finish-data-change", ());
    }
}
async fn prepare(app: &tauri::AppHandle) -> Result<Prepared, String> {
    let token = uuid::Uuid::new_v4().to_string();
    let (sender, receiver) = mpsc::channel();
    {
        let state = app.state::<DataGate>();
        let mut gate = state.0.lock().map_err(|e| e.to_string())?;
        if gate.is_some() {
            return Err("另一个数据操作正在进行".into());
        }
        *gate = Some((token.clone(), sender));
    }
    let guard = Prepared(app.clone());
    let mut expected = app
        .webview_windows()
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    app.emit("prepare-data-change", token)
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !expected.is_empty() {
            let (label, error) = receiver
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .map_err(|_| "窗口未就绪，请稍后重试".to_string())?;
            if let Some(error) = error {
                return Err(error);
            }
            expected.remove(&label);
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(guard)
}
#[tauri::command]
pub fn maintenance_ack(
    token: String,
    error: Option<String>,
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
) {
    if let Ok(gate) = app.state::<DataGate>().0.lock() {
        if let Some((expected, sender)) = gate.as_ref() {
            if *expected == token {
                let _ = sender.send((window.label().to_string(), error));
            }
        }
    }
}

fn directory(conn: &rusqlite::Connection) -> PathBuf {
    Path::new(conn.path().unwrap_or(""))
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf()
}
pub fn refresh_backup(conn: &rusqlite::Connection) {
    let error = storage::daily_backup(conn, &directory(conn))
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    let _ = db::set_setting(conn, "backup_error", &error);
}
#[tauri::command]
pub async fn check_backup(state: tauri::State<'_, Database>) -> Result<(), String> {
    access(&state, |conn| {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let directory = storage::backup_directory(conn, &directory(conn))?;
        if !directory.join(format!("daily-{today}.sqlite3")).exists() {
            refresh_backup(conn);
        }
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn get_backlog(
    date: String,
    state: tauri::State<'_, Database>,
) -> Result<Vec<db::DayTask>, String> {
    access(&state, move |conn| db::backlog(conn, &date)).await
}
#[tauri::command]
pub async fn carry_task(
    id: String,
    version: i64,
    state: tauri::State<'_, Database>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    write_access(&state, "结转待办", move |conn| {
        db::carry(
            conn,
            &id,
            version,
            &chrono::Local::now().date_naive().to_string(),
        )
    })
    .await?;
    let _ = app.emit("tasks-changed", ());
    Ok(())
}
#[tauri::command]
pub async fn undo_change(
    state: tauri::State<'_, Database>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let _prepared = prepare(&app).await?;
    access(&state, |conn| {
        storage::undo(conn)?;
        refresh_backup(conn);
        Ok(())
    })
    .await?;
    let _ = app.emit("data-replaced", ());
    Ok(())
}
fn registry() -> Command {
    let mut command = Command::new("reg.exe");
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    command
}
fn run_value() -> String {
    std::env::var("DAYPAGE_AUTOSTART_VALUE").unwrap_or_else(|_| "Daypage".into())
}
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
fn autostart() -> bool {
    registry()
        .args(["query", RUN_KEY, "/v", &run_value()])
        .output()
        .is_ok_and(|out| out.status.success())
}
#[tauri::command]
pub async fn set_autostart(enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let output = if enabled {
            let executable = std::env::current_exe().map_err(|e| e.to_string())?;
            registry()
                .args([
                    "add",
                    RUN_KEY,
                    "/v",
                    &run_value(),
                    "/t",
                    "REG_SZ",
                    "/d",
                    &format!("\"{}\"", executable.display()),
                    "/f",
                ])
                .output()
        } else if autostart() {
            registry()
                .args(["delete", RUN_KEY, "/v", &run_value(), "/f"])
                .output()
        } else {
            return Ok(());
        };
        let output = output.map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn get_task_meta(state: tauri::State<'_, Database>) -> Result<serde_json::Value, String> {
    access(&state,|conn|Ok(serde_json::json!({"undo":storage::undo_label(conn)?,"backup_error":db::get_setting(conn,"backup_error")?.unwrap_or_default()}))).await
}
#[tauri::command]
pub async fn get_maintenance(
    state: tauri::State<'_, Database>,
) -> Result<serde_json::Value, String> {
    access(&state, |conn| {
        Ok(serde_json::json!({
            "undo":storage::undo_label(conn)?,"autostart":autostart(),
            "backup_directory":storage::backup_directory(conn,&directory(conn))?,
            "backup_error":db::get_setting(conn,"backup_error")?.unwrap_or_default()
        }))
    })
    .await
}
fn protect_database(conn: &rusqlite::Connection, path: &Path) -> db::Result<()> {
    if let (Ok(current), Ok(target)) = (
        Path::new(conn.path().unwrap_or("")).canonicalize(),
        path.canonicalize(),
    ) {
        if current == target {
            return Err("不能覆盖或恢复正在使用的数据库，请选择备份文件".into());
        }
    }
    let same_parent =
        path.parent().and_then(|p| p.canonicalize().ok()) == directory(conn).canonicalize().ok();
    if same_parent
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.to_lowercase().starts_with("daypage.sqlite3"))
    {
        return Err("不能覆盖正在使用的数据库文件".into());
    }
    Ok(())
}
#[tauri::command]
pub async fn file_action(
    action: String,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Database>,
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    if window.label() != "journal" {
        return Err("请在日历设置中操作".into());
    }
    let dialog_app = app.clone();
    let choice = action.clone();
    let initial_directory = std::env::var_os("DAYPAGE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or(app.path().app_data_dir().map_err(|e| e.to_string())?);
    let dialog_window = window.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || {
        let file = dialog_app
            .dialog()
            .file()
            .set_parent(&dialog_window)
            .set_directory(initial_directory);
        if choice == "directory" {
            return file.blocking_pick_folder();
        }
        if choice == "restore" {
            return file
                .add_filter("日页备份或 JSON", &["sqlite3", "json"])
                .blocking_pick_file();
        }
        let extension = match choice.as_str() {
            "backup" => "sqlite3",
            "json" => "json",
            "markdown" => "md",
            _ => return None,
        };
        file.add_filter("日页文件", &[extension])
            .set_file_name(format!(
                "日页-{}.{}",
                chrono::Local::now().format("%Y%m%d-%H%M%S"),
                extension
            ))
            .blocking_save_file()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(chosen) = chosen else {
        return Ok(None);
    };
    let path = chosen.into_path().map_err(|e| e.to_string())?;
    if action == "restore" {
        let confirm_app = app.clone();
        let confirm_window = window.clone();
        let confirmed=tauri::async_runtime::spawn_blocking(move||confirm_app.dialog().message("将用所选文件完整替换待办、反思和日记。当前数据会先备份，设置保持不变。是否恢复？").parent(&confirm_window)
                .title("恢复日页数据").kind(MessageDialogKind::Warning).buttons(MessageDialogButtons::OkCancelCustom("恢复".into(),"取消".into())).blocking_show()).await.map_err(|e|e.to_string())?;
        if !confirmed {
            return Ok(None);
        }
    }
    let restored = action == "restore";
    let _prepared = if restored {
        Some(prepare(&app).await?)
    } else {
        None
    };
    let result = access(&state, move |conn| {
        protect_database(conn, &path)?;
        match action.as_str() {
            "directory" => {
                std::fs::create_dir_all(&path)?;
                // Verify a consistent backup succeeds before persisting the selected folder.
                let check = path.join(format!(
                    "manual-{}.sqlite3",
                    chrono::Utc::now().format("%Y%m%d-%H%M%S-%f")
                ));
                conn.backup("main", &check, None)?;
                db::set_setting(
                    conn,
                    "backup_directory",
                    path.to_str().ok_or("目录格式无效")?,
                )?;
                refresh_backup(conn);
            }
            "backup" => conn.backup("main", &path, None)?,
            "json" => std::fs::write(&path, storage::json(conn)?)?,
            "markdown" => std::fs::write(&path, storage::markdown(conn)?)?,
            "restore" => {
                let backups = storage::backup_directory(conn, &directory(conn))?;
                let before = storage::restore(conn, &path, &backups)?;
                refresh_backup(conn);
                return Ok(format!("恢复完成；恢复前备份：{}", before.display()));
            }
            _ => return Err("操作无效".into()),
        }
        Ok(path.display().to_string())
    })
    .await?;
    if restored {
        let _ = app.emit("data-replaced", ());
    }
    Ok(Some(result))
}

pub async fn request_quit(app: tauri::AppHandle) -> Result<(), String> {
    let _prepared = prepare(&app).await?;
    app.exit(0);
    Ok(())
}
