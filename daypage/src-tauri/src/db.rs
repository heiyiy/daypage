use chrono::{NaiveDate, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::{error::Error, path::Path};
use uuid::Uuid;
pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug, Serialize)]
pub struct DayTask {
    pub id: String,
    pub title: String,
    pub completed: bool,
    pub version: i64,
    pub reflection: String,
    pub carried_to: Option<String>,
    pub date: String,
}
pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
    )?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > 3 {
        return Err("数据库版本高于当前程序支持的版本".into());
    }
    if version == 0 {
        conn.execute_batch(include_str!("../migrations/001_initial.sql"))?;
    }
    if version == 1 {
        // Make a consistent snapshot before upgrading an existing user's database.
        let backup = path.with_extension("pre-v2.sqlite3");
        if !backup.exists() {
            conn.backup("main", &backup, None)?;
        }
    }
    if version < 2 {
        conn.execute_batch(include_str!("../migrations/002_journal.sql"))?;
    }
    if version > 0 && version < 3 {
        let backup = path.with_extension("pre-v3.sqlite3");
        if !backup.exists() {
            conn.backup("main", &backup, None)?;
        }
    }
    if version < 3 {
        conn.execute_batch(include_str!("../migrations/003_complete.sql"))?;
    }
    Ok(conn)
}

#[derive(Serialize)]
pub struct Note {
    pub body: String,
    pub version: i64,
}

pub fn get_note(conn: &Connection, date: &str) -> Result<Note> {
    validate_date(date)?;
    Ok(conn
        .query_row(
            "SELECT body,version FROM daily_notes WHERE date=?1",
            [date],
            |r| {
                Ok(Note {
                    body: r.get(0)?,
                    version: r.get(1)?,
                })
            },
        )
        .optional()?
        .unwrap_or(Note {
            body: String::new(),
            version: 0,
        }))
}

pub fn save_note(conn: &Connection, date: &str, body: &str, version: i64) -> Result<i64> {
    validate_date(date)?;
    if body.chars().count() > 100_000 {
        return Err("日记不能超过 10 万字".into());
    }
    let now = Utc::now().to_rfc3339();
    let count = if version == 0 {
        conn.execute("INSERT INTO daily_notes(date,body,updated_at) VALUES(?1,?2,?3) ON CONFLICT(date) DO NOTHING",params![date,body,now])?
    } else {
        conn.execute("UPDATE daily_notes SET body=?1,updated_at=?2,version=version+1 WHERE date=?3 AND version=?4",params![body,now,date,version])?
    };
    if count != 1 {
        return Err("日记已被修改，当前文字已保留，请复制后重新打开这一天".into());
    }
    Ok(version + 1)
}

#[derive(Default, Serialize)]
pub struct DaySummary {
    pub date: String,
    pub total: i64,
    pub completed: i64,
    pub has_note: bool,
    pub tasks: Vec<TaskPreview>,
}

#[derive(Serialize)]
pub struct TaskPreview {
    pub title: String,
    pub completed: bool,
}

pub fn get_month(conn: &Connection, year: i32, month: u32) -> Result<Vec<DaySummary>> {
    if !(1..=9998).contains(&year) {
        return Err("年份无效".into());
    }
    let start = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or("月份无效")?
        .to_string();
    let end = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .ok_or("月份无效")?
    .to_string();
    let mut days = std::collections::BTreeMap::<String, DaySummary>::new();
    let mut stmt = conn.prepare("SELECT date,COUNT(*),SUM(result='completed') FROM task_days WHERE date>=?1 AND date<?2 AND deleted_at IS NULL AND result!='cancelled' GROUP BY date")?;
    for row in stmt.query_map(params![start, end], |r| {
        Ok(DaySummary {
            date: r.get(0)?,
            total: r.get(1)?,
            completed: r.get(2)?,
            has_note: false,
            tasks: Vec::new(),
        })
    })? {
        let day = row?;
        days.insert(day.date.clone(), day);
    }
    // Return at most three titles per day, while keeping the full completion counts.
    let mut stmt = conn.prepare("SELECT date,title_snapshot,result FROM (SELECT date,title_snapshot,result,sort_order,id,ROW_NUMBER() OVER (PARTITION BY date ORDER BY sort_order,id) AS row_number FROM task_days WHERE date>=?1 AND date<?2 AND deleted_at IS NULL AND result!='cancelled') WHERE row_number<=3 ORDER BY date,sort_order,id")?;
    for row in stmt.query_map(params![start, end], |r| {
        Ok((
            r.get::<_, String>(0)?,
            TaskPreview {
                title: r.get(1)?,
                completed: r.get::<_, String>(2)? == "completed",
            },
        ))
    })? {
        let (date, task) = row?;
        if let Some(day) = days.get_mut(&date) {
            day.tasks.push(task);
        }
    }
    let mut stmt = conn.prepare(
        "SELECT date FROM daily_notes WHERE date>=?1 AND date<?2 AND length(trim(body))>0",
    )?;
    for row in stmt.query_map(params![start, end], |r| r.get::<_, String>(0))? {
        let date = row?;
        days.entry(date.clone())
            .or_insert_with(|| DaySummary {
                date,
                ..Default::default()
            })
            .has_note = true;
    }
    Ok(days.into_values().collect())
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}
pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value])?;
    Ok(())
}
pub fn validate_date(date: &str) -> Result<()> {
    let parsed = NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
    if parsed.format("%Y-%m-%d").to_string() != date {
        return Err("日期格式应为 YYYY-MM-DD".into());
    }
    Ok(())
}
pub fn get_day(conn: &Connection, date: &str) -> Result<Vec<DayTask>> {
    validate_date(date)?;
    let mut stmt = conn.prepare("SELECT id,title_snapshot,result,version,reflection,(SELECT date FROM task_days target WHERE target.id=task_days.carried_to_id),date FROM task_days WHERE date=?1 AND deleted_at IS NULL ORDER BY sort_order,id")?;
    let rows = stmt.query_map([date], |r| {
        Ok(DayTask {
            id: r.get(0)?,
            title: r.get(1)?,
            completed: r.get::<_, String>(2)? == "completed",
            version: r.get(3)?,
            reflection: r.get(4)?,
            carried_to: r.get(5)?,
            date: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}
pub fn create_task(conn: &mut Connection, date: &str, title: &str) -> Result<DayTask> {
    validate_date(date)?;
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 500 {
        return Err("任务标题需为 1—500 个字符".into());
    }
    let task_id = Uuid::new_v4().to_string();
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let tx = conn.savepoint()?;
    tx.execute(
        "INSERT INTO tasks(id,title,created_at,updated_at) VALUES(?1,?2,?3,?3)",
        params![task_id, title, now],
    )?;
    tx.execute("INSERT INTO task_days(id,task_id,date,title_snapshot,sort_order,created_at,updated_at) SELECT ?1,?2,?3,?4,COALESCE(MAX(sort_order),-1)+1,?5,?5 FROM task_days WHERE date=?3", params![id,task_id,date,title,now])?;
    tx.commit()?;
    Ok(DayTask {
        id,
        title: title.to_string(),
        completed: false,
        version: 1,
        reflection: String::new(),
        carried_to: None,
        date: date.to_string(),
    })
}
pub fn set_completed(conn: &Connection, id: &str, completed: bool, version: i64) -> Result<()> {
    let count = conn.execute("UPDATE task_days SET result=?1,version=version+1,updated_at=?2 WHERE id=?3 AND version=?4 AND deleted_at IS NULL",
        params![if completed { "completed" } else { "pending" },Utc::now().to_rfc3339(),id,version])?;
    if count != 1 {
        return Err("记录已变化，请重新载入后再操作".into());
    }
    Ok(())
}

pub fn get_deleted(conn: &Connection, date: &str) -> Result<Vec<DayTask>> {
    validate_date(date)?;
    let mut stmt=conn.prepare("SELECT id,title_snapshot,result,version,reflection,(SELECT date FROM task_days target WHERE target.id=task_days.carried_to_id),date FROM task_days WHERE date=?1 AND deleted_at IS NOT NULL ORDER BY deleted_at DESC,id")?;
    let rows = stmt.query_map([date], |r| {
        Ok(DayTask {
            id: r.get(0)?,
            title: r.get(1)?,
            completed: r.get::<_, String>(2)? == "completed",
            version: r.get(3)?,
            reflection: r.get(4)?,
            carried_to: r.get(5)?,
            date: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn edit_entry(
    conn: &mut Connection,
    id: &str,
    version: i64,
    action: &str,
    title: Option<&str>,
) -> Result<Vec<DayTask>> {
    let tx = conn.savepoint()?;
    let (task_id, date, current_version, deleted): (String, String, i64, bool) = tx.query_row(
        "SELECT task_id,date,version,deleted_at IS NOT NULL FROM task_days WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    if current_version != version {
        return Err("记录已变化，请重新载入后再操作；未保存标题已保留".into());
    }
    let now = Utc::now().to_rfc3339();
    match action {
        "restore" if deleted => {
            tx.execute(
                "UPDATE task_days SET deleted_at=NULL,version=version+1,updated_at=?1 WHERE id=?2",
                params![now, id],
            )?;
        }
        "delete" if !deleted => {
            tx.execute(
                "UPDATE task_days SET deleted_at=?1,version=version+1,updated_at=?1 WHERE id=?2",
                params![now, id],
            )?;
        }
        "reflection" if !deleted => {
            let body = title.unwrap_or("");
            if body.chars().count() > 10_000 {
                return Err("事项反思不能超过 1 万字".into());
            }
            tx.execute(
                "UPDATE task_days SET reflection=?1,version=version+1,updated_at=?2 WHERE id=?3",
                params![body, now, id],
            )?;
        }
        "rename" if !deleted => {
            let title = title.unwrap_or("").trim();
            if title.is_empty() || title.chars().count() > 500 {
                return Err("任务标题需为 1—500 个字符".into());
            }
            tx.execute("UPDATE task_days SET title_snapshot=?1,version=version+1,updated_at=?2 WHERE id=?3",params![title,now,id])?;
            // A historical correction never changes the current title of a later arrangement.
            tx.execute("UPDATE tasks SET title=?1,updated_at=?2 WHERE id=?3 AND NOT EXISTS(SELECT 1 FROM task_days WHERE task_id=?3 AND date>?4 AND deleted_at IS NULL)",params![title,now,task_id,date])?;
        }
        "up" | "down" if !deleted => {
            let rows = {
                let mut stmt=tx.prepare("SELECT id,sort_order FROM task_days WHERE date=?1 AND deleted_at IS NULL ORDER BY sort_order,id")?;
                stmt.query_map([&date], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?
            };
            let index = rows
                .iter()
                .position(|row| row.0 == id)
                .ok_or("找不到任务")?;
            let neighbor = if action == "up" {
                index.checked_sub(1)
            } else {
                index.checked_add(1).filter(|&next| next < rows.len())
            };
            if let Some(next) = neighbor {
                tx.execute("UPDATE task_days SET sort_order=?1,version=version+1,updated_at=?2 WHERE id=?3",params![rows[next].1,now,id])?;
                tx.execute("UPDATE task_days SET sort_order=?1,version=version+1,updated_at=?2 WHERE id=?3",params![rows[index].1,now,rows[next].0])?;
            }
        }
        _ => return Err("操作无效，或该条目已删除 / 恢复".into()),
    }
    let rows = get_day(&tx, &date)?;
    tx.commit()?;
    Ok(rows)
}
pub fn backlog(conn: &Connection, date: &str) -> Result<Vec<DayTask>> {
    validate_date(date)?;
    let mut stmt=conn.prepare("SELECT id,date FROM task_days d WHERE date<?1 AND result='pending' AND deleted_at IS NULL AND carried_to_id IS NULL AND NOT EXISTS(SELECT 1 FROM task_days newer WHERE newer.task_id=d.task_id AND newer.date>d.date AND newer.deleted_at IS NULL) ORDER BY date DESC,sort_order,id")?;
    let ids = stmt
        .query_map([date], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut rows = Vec::new();
    for (id, day) in ids {
        if let Some(row) = get_day(conn, &day)?.into_iter().find(|r| r.id == id) {
            rows.push(row);
        }
    }
    Ok(rows)
}

pub fn carry(conn: &mut Connection, id: &str, version: i64, target: &str) -> Result<()> {
    validate_date(target)?;
    let tx = conn.savepoint()?;
    let (task_id,date,title,result,current,deleted,link):(String,String,String,String,i64,bool,Option<String>)=tx.query_row("SELECT task_id,date,title_snapshot,result,version,deleted_at IS NOT NULL,carried_to_id FROM task_days WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)))?;
    if let Some(link) = link {
        let existing: String =
            tx.query_row("SELECT date FROM task_days WHERE id=?1", [link], |r| {
                r.get(0)
            })?;
        if existing == target {
            return Ok(());
        }
        return Err("该事项已结转到其他日期".into());
    }
    if deleted || result != "pending" || current != version || target <= date.as_str() {
        return Err("只可将未完成且未结转的事项安排到后续日期，请刷新后重试".into());
    }
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM task_days WHERE task_id=?1 AND date>=?2)",
        params![task_id, target],
        |r| r.get(0),
    )?;
    if exists {
        return Err("目标日期或之后已有该任务安排（含已删除记录），请查看或恢复原安排".into());
    }
    let next = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    tx.execute("INSERT INTO task_days(id,task_id,date,title_snapshot,sort_order,created_at,updated_at) SELECT ?1,?2,?3,?4,COALESCE(MAX(sort_order),-1)+1,?5,?5 FROM task_days WHERE date=?3",params![next,task_id,target,title,now])?;
    tx.execute(
        "UPDATE task_days SET carried_to_id=?1,version=version+1,updated_at=?2 WHERE id=?3",
        params![next, now, id],
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editing_order_and_soft_delete_are_versioned_and_restorable() {
        let mut conn = open(Path::new(":memory:")).unwrap();
        let first = create_task(&mut conn, "2026-11-13", "甲").unwrap();
        let second = create_task(&mut conn, "2026-11-13", "乙").unwrap();
        let third = create_task(&mut conn, "2026-11-13", "丙").unwrap();
        let rows = edit_entry(&mut conn, &second.id, 1, "up", None).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            vec!["乙", "甲", "丙"]
        );
        assert!(edit_entry(&mut conn, &first.id, 1, "delete", None).is_err());
        let rows = edit_entry(&mut conn, &second.id, 2, "rename", Some("改名")).unwrap();
        assert_eq!(rows[0].title, "改名");
        assert!(edit_entry(&mut conn, &second.id, 3, "rename", Some("   ")).is_err());
        edit_entry(&mut conn, &second.id, 3, "delete", None).unwrap();
        assert_eq!(get_month(&conn, 2026, 11).unwrap()[0].total, 2);
        let deleted = get_deleted(&conn, "2026-11-13").unwrap();
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].title, "改名");
        edit_entry(&mut conn, &third.id, 1, "up", None).unwrap();
        let rows = edit_entry(&mut conn, &second.id, deleted[0].version, "restore", None).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            vec!["改名", "丙", "甲"]
        );
        assert!(get_deleted(&conn, "2026-11-13").unwrap().is_empty());
    }

    #[test]
    fn historical_title_edits_do_not_rewrite_later_snapshots() {
        let mut conn = open(Path::new(":memory:")).unwrap();
        let task = create_task(&mut conn, "2026-11-12", "原名").unwrap();
        conn.execute("INSERT INTO task_days(id,task_id,date,title_snapshot,sort_order,created_at,updated_at) SELECT 'later',task_id,'2026-11-13',title_snapshot,0,created_at,updated_at FROM task_days WHERE id=?1",[&task.id]).unwrap();
        edit_entry(&mut conn, &task.id, 1, "rename", Some("昨天的标题")).unwrap();
        assert_eq!(get_day(&conn, "2026-11-13").unwrap()[0].title, "原名");
        assert_eq!(
            conn.query_row("SELECT title FROM tasks", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "原名"
        );
        edit_entry(&mut conn, "later", 1, "rename", Some("现在的标题")).unwrap();
        assert_eq!(get_day(&conn, "2026-11-12").unwrap()[0].title, "昨天的标题");
    }
    #[test]
    fn month_previews_are_bounded_ordered_and_exclude_deleted_tasks() {
        let mut conn = open(Path::new(":memory:")).unwrap();
        let hidden = create_task(&mut conn, "2026-11-13", "已删除，不应出现在月历").unwrap();
        conn.execute(
            "UPDATE task_days SET deleted_at='deleted' WHERE id=?1",
            [hidden.id],
        )
        .unwrap();
        for index in 1..=5 {
            let task = create_task(&mut conn, "2026-11-13", &format!("待办 {index}")).unwrap();
            if index == 1 {
                set_completed(&conn, &task.id, true, 1).unwrap();
            }
        }
        create_task(&mut conn, "2026-12-01", "别的月份").unwrap();
        let month = get_month(&conn, 2026, 11).unwrap();
        assert_eq!(month.len(), 1);
        assert_eq!(month[0].total, 5);
        assert_eq!(month[0].completed, 1);
        assert_eq!(month[0].tasks.len(), 3);
        assert_eq!(month[0].tasks[0].title, "待办 1");
        assert!(month[0].tasks[0].completed);
        assert_eq!(month[0].tasks[2].title, "待办 3");
    }
    #[test]
    fn journal_versions_and_month_summary_preserve_each_date() {
        let mut conn = open(Path::new(":memory:")).unwrap();
        assert_eq!(get_note(&conn, "2026-11-11").unwrap().version, 0);
        assert_eq!(save_note(&conn, "2026-11-11", "昨天的日记", 0).unwrap(), 1);
        assert!(save_note(&conn, "2026-11-11", "旧写入", 0).is_err());
        assert_eq!(save_note(&conn, "2026-11-12", "今天的日记", 0).unwrap(), 1);
        let task = create_task(&mut conn, "2026-11-12", "任务").unwrap();
        set_completed(&conn, &task.id, true, task.version).unwrap();
        let month = get_month(&conn, 2026, 11).unwrap();
        assert_eq!(month.len(), 2);
        assert_eq!(month[0].total, 0);
        assert!(month[0].has_note);
        assert_eq!(month[1].completed, 1);
        assert_eq!(get_note(&conn, "2026-11-11").unwrap().body, "昨天的日记");
        save_note(&conn, "2026-11-11", "", 1).unwrap();
        assert_eq!(get_month(&conn, 2026, 11).unwrap().len(), 1);
        assert!(get_month(&conn, 2026, 13).is_err());
    }

    #[test]
    fn upgrading_v1_creates_consistent_backup_and_keeps_tasks() {
        let path = std::env::temp_dir().join(format!("daypage-upgrade-{}.sqlite3", Uuid::new_v4()));
        {
            let mut conn = Connection::open(&path).unwrap();
            conn.execute_batch(include_str!("../migrations/001_initial.sql"))
                .unwrap();
            create_task(&mut conn, "2026-11-12", "升级前的任务").unwrap();
        }
        let backup = path.with_extension("pre-v2.sqlite3");
        {
            let conn = open(&path).unwrap();
            assert_eq!(
                get_day(&conn, "2026-11-12").unwrap()[0].title,
                "升级前的任务"
            );
            assert_eq!(get_note(&conn, "2026-11-12").unwrap().version, 0);
            let saved = Connection::open(&backup).unwrap();
            assert_eq!(
                saved
                    .query_row("SELECT COUNT(*) FROM task_days", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
            assert_eq!(
                saved
                    .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(backup).unwrap();
        std::fs::remove_file(path.with_extension("pre-v3.sqlite3")).unwrap();
    }
    #[test]
    fn persists_across_reopen_and_keeps_dates_separate() {
        let path = std::env::temp_dir().join(format!("daypage-{}.sqlite3", Uuid::new_v4()));
        {
            let mut conn = open(&path).unwrap();
            let task = create_task(&mut conn, "2026-11-12", "  中文任务  ").unwrap();
            set_completed(&conn, &task.id, true, task.version).unwrap();
            assert!(set_completed(&conn, &task.id, false, task.version).is_err());
        }
        {
            let conn = open(&path).unwrap();
            let rows = get_day(&conn, "2026-11-12").unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].title, "中文任务");
            assert!(rows[0].completed);
            assert_eq!(rows[0].version, 2);
            assert!(get_day(&conn, "2026-11-13").unwrap().is_empty());
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn rejects_invalid_input_without_creating_partial_tasks() {
        let mut conn = open(Path::new(":memory:")).unwrap();
        assert!(create_task(&mut conn, "2026-02-30", "任务").is_err());
        assert!(create_task(&mut conn, "2026-11-12", "  ").is_err());
        assert!(create_task(&mut conn, "2026-11-12", &"字".repeat(501)).is_err());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
