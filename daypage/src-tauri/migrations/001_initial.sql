BEGIN IMMEDIATE;
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE task_days (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    date TEXT NOT NULL,
    title_snapshot TEXT NOT NULL,
    result TEXT NOT NULL DEFAULT 'pending' CHECK(result IN ('pending','completed','cancelled')),
    sort_order INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    carried_to_id TEXT REFERENCES task_days(id),
    deleted_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(task_id,date)
);
CREATE INDEX task_days_date ON task_days(date,sort_order);
PRAGMA user_version=1;
COMMIT;
