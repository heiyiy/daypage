use crate::db::{self, Result};
use chrono::{Local, Utc};
use rusqlite::{Connection, OptionalExtension, params, types::ValueRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

type Rows = BTreeMap<String, Value>;
type Snapshot = BTreeMap<String, Rows>;
const TABLES: [&str; 3] = ["tasks", "task_days", "daily_notes"];
#[derive(Serialize, Deserialize)]
pub struct Archive {
    format: String,
    version: u32,
    data: Snapshot,
}
#[derive(Serialize, Deserialize)]
struct Change {
    table: String,
    key: String,
    before: Option<Value>,
    after: Option<Value>,
}

fn key_column(table: &str) -> &str {
    if table == "daily_notes" { "date" } else { "id" }
}
fn snapshot(conn: &Connection) -> Result<Snapshot> {
    let mut data = Snapshot::new();
    for table in TABLES {
        let mut stmt = conn.prepare(&format!("SELECT * FROM {table}"))?;
        let columns = stmt
            .column_names()
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let mut rows = Rows::new();
        let mut query = stmt.query([])?;
        while let Some(row) = query.next()? {
            let mut object = serde_json::Map::new();
            for (i, column) in columns.iter().enumerate() {
                let value = match row.get_ref(i)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(n) => Value::from(n),
                    ValueRef::Text(text) => Value::String(std::str::from_utf8(text)?.to_string()),
                    _ => return Err("数据字段格式无效".into()),
                };
                object.insert(column.clone(), value);
            }
            let value = Value::Object(object);
            let key = value[key_column(table)]
                .as_str()
                .ok_or("记录缺少标识")?
                .to_string();
            rows.insert(key, value);
        }
        data.insert(table.to_string(), rows);
    }
    Ok(data)
}
fn semantic(mut row: Value) -> Value {
    if let Some(map) = row.as_object_mut() {
        map.remove("version");
        map.remove("updated_at");
    }
    row
}
fn init_capture(conn: &Connection) -> Result<()> {
    let initialized: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_temp_master WHERE name='undo_capture')",
        [],
        |r| r.get(0),
    )?;
    if initialized {
        return Ok(());
    }
    conn.execute_batch("CREATE TEMP TABLE undo_capture(table_name TEXT,row_key TEXT,before_row TEXT,after_row TEXT,PRIMARY KEY(table_name,row_key)); CREATE TEMP TABLE undo_capture_enabled(enabled INTEGER);INSERT INTO undo_capture_enabled VALUES(0);")?;
    for table in TABLES {
        let stmt = conn.prepare(&format!("SELECT * FROM {table} LIMIT 0"))?;
        let columns = stmt.column_names();
        let object = |prefix: &str| {
            format!(
                "json_object({})",
                columns
                    .iter()
                    .map(|column| format!("'{column}',{prefix}.{column}"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        for (event, key, before, after) in [
            (
                "INSERT",
                format!("NEW.{}", key_column(table)),
                "NULL".to_string(),
                object("NEW"),
            ),
            (
                "UPDATE",
                format!("NEW.{}", key_column(table)),
                object("OLD"),
                object("NEW"),
            ),
            (
                "DELETE",
                format!("OLD.{}", key_column(table)),
                object("OLD"),
                "NULL".to_string(),
            ),
        ] {
            conn.execute_batch(&format!("CREATE TEMP TRIGGER capture_{table}_{event} AFTER {event} ON main.{table} WHEN (SELECT enabled FROM undo_capture_enabled)=1 BEGIN INSERT INTO undo_capture(table_name,row_key,before_row,after_row) VALUES('{table}',{key},{before},{after}) ON CONFLICT(table_name,row_key) DO UPDATE SET after_row=excluded.after_row; END;"))?;
        }
    }
    Ok(())
}
// Capture only changed rows; routine edits never scan the full journal history.
pub fn tracked<T>(
    conn: &mut Connection,
    label: &str,
    operation: impl FnOnce(&mut Connection) -> Result<T>,
) -> Result<T> {
    init_capture(conn)?;
    conn.execute_batch("SAVEPOINT tracked_write; DELETE FROM undo_capture; UPDATE undo_capture_enabled SET enabled=1;")?;
    let result = (|| {
        let value = operation(conn)?;
        conn.execute("UPDATE undo_capture_enabled SET enabled=0", [])?;
        let changes = {
            let mut stmt=conn.prepare("SELECT table_name,row_key,before_row,after_row FROM undo_capture ORDER BY table_name,row_key")?;
            let raw = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let mut changes = Vec::new();
            for (table, key, before, after) in raw {
                let before = before
                    .map(|value| serde_json::from_str::<Value>(&value))
                    .transpose()?;
                let after = after
                    .map(|value| serde_json::from_str::<Value>(&value))
                    .transpose()?;
                if before != after {
                    changes.push(Change {
                        table,
                        key,
                        before,
                        after,
                    });
                }
            }
            changes
        };
        if !changes.is_empty() {
            conn.execute(
                "INSERT INTO undo_log(label,changes,created_at) VALUES(?1,?2,?3)",
                params![
                    label,
                    serde_json::to_string(&changes)?,
                    Utc::now().to_rfc3339()
                ],
            )?;
            conn.execute("DELETE FROM undo_log WHERE id NOT IN (SELECT id FROM undo_log ORDER BY id DESC LIMIT 50)",[])?;
        }
        Ok(value)
    })();
    match result {
        Ok(value) => {
            conn.execute_batch("RELEASE tracked_write")?;
            Ok(value)
        }
        Err(error) => {
            conn.execute_batch("ROLLBACK TO tracked_write; RELEASE tracked_write")?;
            Err(error)
        }
    }
}
fn write_row(conn: &Connection, table: &str, row: &Value) -> Result<()> {
    if !TABLES.contains(&table) {
        return Err("数据表无效".into());
    }
    let object = row.as_object().ok_or("记录格式无效")?;
    // Column names come from the destination schema, never from an imported SQL identifier.
    let stmt = conn.prepare(&format!("SELECT * FROM {table} LIMIT 0"))?;
    let columns = stmt
        .column_names()
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let mut values = Vec::new();
    for column in &columns {
        let value = object.get(column).ok_or("导入记录缺少字段")?;
        values.push(match value {
            Value::Null => rusqlite::types::Value::Null,
            Value::String(s) => rusqlite::types::Value::Text(s.clone()),
            Value::Number(n) => rusqlite::types::Value::Integer(n.as_i64().ok_or("整数格式无效")?),
            _ => return Err("导入字段格式无效".into()),
        });
    }
    let names = columns.join(",");
    let placeholders = vec!["?"; columns.len()].join(",");
    let updates = columns
        .iter()
        .map(|name| format!("{name}=excluded.{name}"))
        .collect::<Vec<_>>()
        .join(",");
    conn.execute(&format!("INSERT INTO {table}({names}) VALUES({placeholders}) ON CONFLICT({}) DO UPDATE SET {updates}",key_column(table)),rusqlite::params_from_iter(values))?;
    Ok(())
}
pub fn undo_label(conn: &Connection) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT label FROM undo_log ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?)
}
pub fn undo(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction()?;
    let (id, json): (i64, String) = tx.query_row(
        "SELECT id,changes FROM undo_log ORDER BY id DESC LIMIT 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let changes: Vec<Change> = serde_json::from_str(&json)?;
    let current = snapshot(&tx)?;
    for change in &changes {
        if current[&change.table]
            .get(&change.key)
            .cloned()
            .map(semantic)
            != change.after.clone().map(semantic)
        {
            return Err("记录已有后续修改，不能覆盖，请刷新后重试".into());
        }
    }
    tx.execute_batch("PRAGMA defer_foreign_keys=ON")?;
    for change in changes.iter().rev() {
        if let Some(mut row) = change.before.clone() {
            if row.get("version").is_some() {
                let version = current[&change.table]
                    .get(&change.key)
                    .and_then(|r| r["version"].as_i64())
                    .unwrap_or(0)
                    .max(row["version"].as_i64().unwrap_or(0))
                    + 1;
                row["version"] = version.into();
                row["updated_at"] = Utc::now().to_rfc3339().into();
            }
            write_row(&tx, &change.table, &row)?;
        } else {
            tx.execute(
                &format!(
                    "DELETE FROM {} WHERE {}=?1",
                    change.table,
                    key_column(&change.table)
                ),
                [&change.key],
            )?;
        }
    }
    tx.execute("DELETE FROM undo_log WHERE id=?1", [id])?;
    tx.commit()?;
    Ok(())
}
pub fn archive(conn: &Connection) -> Result<Archive> {
    Ok(Archive {
        format: "daypage".into(),
        version: 3,
        data: snapshot(conn)?,
    })
}
pub fn json(conn: &Connection) -> Result<String> {
    Ok(serde_json::to_string_pretty(&archive(conn)?)?)
}
pub fn markdown(conn: &Connection) -> Result<String> {
    let data = snapshot(conn)?;
    let mut dates = std::collections::BTreeSet::new();
    for table in ["task_days", "daily_notes"] {
        for row in data[table].values() {
            dates.insert(row["date"].as_str().ok_or("日期格式无效")?.to_string());
        }
    }
    let mut text = String::from("# 日页\n\n");
    for date in dates {
        text.push_str(&format!("## {date}\n\n"));
        let mut rows = data["task_days"]
            .values()
            .filter(|r| r["date"] == date)
            .collect::<Vec<_>>();
        rows.sort_by_key(|r| {
            (
                r["sort_order"].as_i64().unwrap_or(0),
                r["id"].as_str().unwrap_or(""),
            )
        });
        for row in rows {
            let title = row["title_snapshot"]
                .as_str()
                .unwrap_or("")
                .replace(['\n', '\r'], " ");
            text.push_str(&format!(
                "- [{}] {}{}\n",
                if row["result"] == "completed" {
                    "x"
                } else {
                    " "
                },
                title,
                if row["deleted_at"].is_null() {
                    ""
                } else {
                    "（已删除）"
                }
            ));
            if let Some(link) = row["carried_to_id"].as_str() {
                if let Some(target) = data["task_days"].get(link) {
                    text.push_str(&format!(
                        "  结转至 {}\n",
                        target["date"].as_str().unwrap_or("")
                    ));
                }
            }
            let reflection = row["reflection"].as_str().unwrap_or("");
            if !reflection.is_empty() {
                text.push_str(&format!(
                    "\n  事项反思：{}\n",
                    reflection.replace('\n', "\n  ")
                ));
            }
        }
        if let Some(note) = data["daily_notes"].get(&date) {
            text.push_str(&format!("\n{}\n", note["body"].as_str().unwrap_or("")));
        }
        text.push('\n');
    }
    Ok(text)
}
fn validate(conn: &Connection) -> Result<()> {
    let check: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if check != "ok" {
        return Err("数据库完整性检查未通过".into());
    }
    if conn.prepare("PRAGMA foreign_key_check")?.exists([])? {
        return Err("数据关联检查未通过".into());
    }
    let data = snapshot(conn)?;
    for table in ["task_days", "daily_notes"] {
        for row in data[table].values() {
            db::validate_date(row["date"].as_str().ok_or("日期无效")?)?;
            if !row["version"]
                .as_i64()
                .is_some_and(|v| v > 0 && v < i64::MAX / 4)
            {
                return Err("记录版本无效".into());
            }
        }
    }
    for row in data["task_days"].values() {
        let title = row["title_snapshot"].as_str().ok_or("标题无效")?;
        if title.trim().is_empty()
            || title.chars().count() > 500
            || row["reflection"]
                .as_str()
                .ok_or("反思无效")?
                .chars()
                .count()
                > 10_000
        {
            return Err("待办内容格式无效".into());
        }
        if let Some(link) = row["carried_to_id"].as_str() {
            let target = data["task_days"].get(link).ok_or("结转关联缺失")?;
            if target["task_id"] != row["task_id"]
                || target["date"].as_str() <= row["date"].as_str()
            {
                return Err("结转关联无效".into());
            }
        }
    }
    for row in data["daily_notes"].values() {
        if row["body"].as_str().ok_or("日记无效")?.chars().count() > 100_000 {
            return Err("日记过长".into());
        }
    }
    Ok(())
}
fn replace(conn: &mut Connection, data: &Snapshot, invalidate: bool) -> Result<()> {
    if data.len() != 3 || TABLES.iter().any(|table| !data.contains_key(*table)) {
        return Err("数据表集合无效".into());
    }
    let baseline:i64=conn.query_row("SELECT MAX(v) FROM (SELECT COALESCE(MAX(version),0) v FROM task_days UNION ALL SELECT COALESCE(MAX(version),0) FROM daily_notes)",[],|r|r.get(0))?;
    let tx = conn.transaction()?;
    tx.execute_batch("PRAGMA defer_foreign_keys=ON; DELETE FROM task_days; DELETE FROM tasks; DELETE FROM daily_notes;")?;
    for table in TABLES {
        for (key, row) in &data[table] {
            if row[key_column(table)].as_str() != Some(key) {
                return Err("数据标识不一致".into());
            }
            write_row(&tx, table, row)?;
        }
    }
    validate(&tx)?;
    if invalidate {
        for table in ["task_days", "daily_notes"] {
            tx.execute(
                &format!("UPDATE {table} SET version=MAX(version,?1)+1"),
                [baseline],
            )?;
        }
    }
    tx.execute("DELETE FROM undo_log", [])?;
    tx.commit()?;
    Ok(())
}
pub fn restore(conn: &mut Connection, path: &Path, backups: &Path) -> Result<PathBuf> {
    let mut candidate = db::open(Path::new(":memory:"))?;
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("json"))
    {
        if std::fs::metadata(path)?.len() > 128 * 1024 * 1024 {
            return Err("导入文件超过 128 MiB".into());
        }
        let imported: Archive = serde_json::from_slice(&std::fs::read(path)?)?;
        if imported.format != "daypage" || imported.version != 3 {
            return Err("不支持的导出文件版本".into());
        }
        replace(&mut candidate, &imported.data, false)?;
    } else {
        let source = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let version: i64 = source.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if !(1..=3).contains(&version) {
            return Err("不支持的备份版本".into());
        }
        // Copy into memory, then migrate the isolated candidate; never alter the chosen file.
        let backup = rusqlite::backup::Backup::new(&source, &mut candidate)?;
        backup.run_to_completion(128, std::time::Duration::from_millis(5), None)?;
        drop(backup);
        candidate.execute_batch("PRAGMA foreign_keys=ON")?;
        if version == 1 {
            candidate.execute_batch(include_str!("../migrations/002_journal.sql"))?;
        }
        if version < 3 {
            candidate.execute_batch(include_str!("../migrations/003_complete.sql"))?;
        }
        validate(&candidate)?;
    }
    let data = snapshot(&candidate)?;
    std::fs::create_dir_all(backups)?;
    let before = backups.join(format!(
        "before-restore-{}.sqlite3",
        Utc::now().format("%Y%m%d-%H%M%S-%f")
    ));
    conn.backup("main", &before, None)?;
    replace(conn, &data, true)?;

    Ok(before)
}
pub fn backup_directory(conn: &Connection, default: &Path) -> Result<PathBuf> {
    Ok(db::get_setting(conn, "backup_directory")?
        .map(PathBuf::from)
        .unwrap_or_else(|| default.join("backups")))
}
pub fn daily_backup(conn: &Connection, default: &Path) -> Result<()> {
    let directory = backup_directory(conn, default)?;
    std::fs::create_dir_all(&directory)?;
    let path = directory.join(format!("daily-{}.sqlite3", Local::now().format("%Y-%m-%d")));
    conn.backup("main", &path, None)?;
    let mut files = std::fs::read_dir(&directory)?
        .filter_map(|entry| entry.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| name.starts_with("daily-") && name.ends_with(".sqlite3"))
        })
        .collect::<Vec<_>>();
    files.sort();
    let excess = files.len().saturating_sub(30);
    for path in files.into_iter().take(excess) {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn memory() -> Connection {
        db::open(Path::new(":memory:")).unwrap()
    }
    fn temp() -> PathBuf {
        let path = std::env::temp_dir().join(format!("daypage-storage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }
    #[test]
    fn carry_is_atomic_idempotent_and_keeps_daily_reflections_separate() {
        let mut conn = memory();
        let original = db::create_task(&mut conn, "2026-11-12", "报告").unwrap();
        db::edit_entry(&mut conn, &original.id, 1, "reflection", Some("昨天的反思")).unwrap();
        tracked(&mut conn, "结转", |c| {
            db::carry(c, &original.id, 2, "2026-11-13")
        })
        .unwrap();
        db::carry(&mut conn, &original.id, 2, "2026-11-13").unwrap();
        assert_eq!(db::get_day(&conn, "2026-11-13").unwrap().len(), 1);
        let target = db::get_day(&conn, "2026-11-13").unwrap().remove(0);
        assert_eq!(target.reflection, "");
        assert!(db::backlog(&conn, "2026-11-13").unwrap().is_empty());
        db::set_completed(&conn, &target.id, true, 1).unwrap();
        let source = db::get_day(&conn, "2026-11-12").unwrap().remove(0);
        assert!(!source.completed);
        assert_eq!(source.reflection, "昨天的反思");
        assert_eq!(source.carried_to.as_deref(), Some("2026-11-13"));
        // Undo must not discard a target modified after carry (even by another writer).
        assert!(undo(&mut conn).is_err());
        assert!(db::get_day(&conn, "2026-11-13").unwrap()[0].completed);
    }
    #[test]
    fn undo_stack_handles_add_rename_reflection_order_delete_and_carry() {
        let mut conn = memory();
        let first = tracked(&mut conn, "添加", |c| {
            db::create_task(c, "2026-11-12", "甲")
        })
        .unwrap();
        let second = tracked(&mut conn, "添加", |c| {
            db::create_task(c, "2026-11-12", "乙")
        })
        .unwrap();
        tracked(&mut conn, "排序", |c| {
            db::edit_entry(c, &second.id, 1, "up", None)
        })
        .unwrap();
        undo(&mut conn).unwrap();
        assert_eq!(db::get_day(&conn, "2026-11-12").unwrap()[0].title, "甲");
        undo(&mut conn).unwrap();
        assert_eq!(db::get_day(&conn, "2026-11-12").unwrap().len(), 1);
        let current = db::get_day(&conn, "2026-11-12").unwrap().remove(0);
        tracked(&mut conn, "反思", |c| {
            db::edit_entry(c, &first.id, current.version, "reflection", Some("内容"))
        })
        .unwrap();
        undo(&mut conn).unwrap();
        assert_eq!(db::get_day(&conn, "2026-11-12").unwrap()[0].reflection, "");
        let current = db::get_day(&conn, "2026-11-12").unwrap().remove(0);
        tracked(&mut conn, "结转", |c| {
            db::carry(c, &first.id, current.version, "2026-11-13")
        })
        .unwrap();
        undo(&mut conn).unwrap();
        assert!(db::get_day(&conn, "2026-11-13").unwrap().is_empty());
        assert!(
            db::get_day(&conn, "2026-11-12").unwrap()[0]
                .carried_to
                .is_none()
        );
        undo(&mut conn).unwrap();
        assert!(db::get_day(&conn, "2026-11-12").unwrap().is_empty());
    }
    #[test]
    fn failed_tracked_write_rolls_back_every_change_and_history() {
        let mut conn = memory();
        let result: Result<()> = tracked(&mut conn, "失败", |c| {
            db::create_task(c, "2026-11-13", "不应保存")?;
            Err("模拟失败".into())
        });
        assert!(result.is_err());
        assert!(db::get_day(&conn, "2026-11-13").unwrap().is_empty());
        assert!(undo_label(&conn).unwrap().is_none());
    }
    #[test]
    fn json_and_sqlite_restore_preserve_data_and_pre_restore_snapshot() {
        let directory = temp();
        let mut conn = memory();
        let task = db::create_task(&mut conn, "2026-11-12", "测试").unwrap();
        db::edit_entry(&mut conn, &task.id, 1, "reflection", Some("反思\n第二行")).unwrap();
        db::carry(&mut conn, &task.id, 2, "2026-11-13").unwrap();
        db::save_note(&conn, "2026-11-12", "日记", 0).unwrap();
        let export = directory.join("export.json");
        std::fs::write(&export, json(&conn).unwrap()).unwrap();
        let backup = directory.join("backup.sqlite3");
        conn.backup("main", &backup, None).unwrap();
        assert!(markdown(&conn).unwrap().contains("反思\n  第二行"));
        assert!(markdown(&conn).unwrap().contains("结转至 2026-11-13"));
        db::create_task(&mut conn, "2026-11-13", "恢复前独有").unwrap();
        let before = restore(&mut conn, &export, &directory).unwrap();
        assert_eq!(db::get_day(&conn, "2026-11-13").unwrap().len(), 1);
        assert_eq!(
            db::get_day(&conn, "2026-11-12").unwrap()[0].reflection,
            "反思\n第二行"
        );
        assert!(db::get_day(&conn, "2026-11-12").unwrap()[0].version > 3);
        let saved = Connection::open(before).unwrap();
        assert_eq!(db::get_day(&saved, "2026-11-13").unwrap().len(), 2);
        drop(saved);
        restore(&mut conn, &backup, &directory).unwrap();
        assert_eq!(db::get_note(&conn, "2026-11-12").unwrap().body, "日记");
        drop(conn);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn corrupt_or_invalid_restore_never_changes_current_data() {
        let directory = temp();
        let mut conn = memory();
        db::create_task(&mut conn, "2026-11-13", "保留").unwrap();
        let invalid = directory.join("invalid.json");
        let mut data = archive(&conn).unwrap();
        data.data
            .get_mut("task_days")
            .unwrap()
            .values_mut()
            .next()
            .unwrap()["carried_to_id"] = "missing".into();
        std::fs::write(&invalid, serde_json::to_string(&data).unwrap()).unwrap();
        assert!(restore(&mut conn, &invalid, &directory).is_err());
        assert_eq!(db::get_day(&conn, "2026-11-13").unwrap()[0].title, "保留");
        let invalid = directory.join("invalid.sqlite3");
        std::fs::write(&invalid, b"invalid").unwrap();
        assert!(restore(&mut conn, &invalid, &directory).is_err());
        assert_eq!(db::get_day(&conn, "2026-11-13").unwrap().len(), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn daily_backup_refreshes_same_day_and_retains_thirty_files() {
        let directory = temp();
        let mut conn = memory();
        let backups = directory.join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        for n in 1..=31 {
            std::fs::write(
                backups.join(format!("daily-2000-01-{n:02}.sqlite3")),
                b"old",
            )
            .unwrap();
        }
        daily_backup(&conn, &directory).unwrap();
        assert_eq!(std::fs::read_dir(&backups).unwrap().count(), 30);
        db::create_task(&mut conn, "2026-11-13", "新增").unwrap();
        daily_backup(&conn, &directory).unwrap();
        let latest = Connection::open(
            backups.join(format!("daily-{}.sqlite3", Local::now().format("%Y-%m-%d"))),
        )
        .unwrap();
        assert_eq!(db::get_day(&latest, "2026-11-13").unwrap().len(), 1);
        drop(latest);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
