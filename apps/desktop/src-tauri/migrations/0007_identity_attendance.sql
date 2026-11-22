CREATE TABLE IF NOT EXISTS personal_passkeys (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    credential_id TEXT NOT NULL UNIQUE,
    label TEXT NOT NULL,
    credential_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_used_at TEXT
);
CREATE TABLE IF NOT EXISTS work_schedules (
    user_id TEXT NOT NULL REFERENCES users(id),
    store_id TEXT NOT NULL REFERENCES stores(id),
    timezone TEXT NOT NULL,
    start_time TEXT NOT NULL,
    end_time TEXT NOT NULL,
    weekdays_json TEXT NOT NULL,
    PRIMARY KEY (user_id, store_id)
);
CREATE TABLE IF NOT EXISTS work_periods (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    store_id TEXT NOT NULL REFERENCES stores(id),
    period_key TEXT NOT NULL,
    scheduled_start TEXT NOT NULL,
    started_at TEXT NOT NULL,
    due_at TEXT NOT NULL,
    ended_at TEXT,
    end_reason TEXT CHECK(end_reason IN ('staff', 'schedule', 'safety_limit')),
    auto_end_reason TEXT NOT NULL CHECK(auto_end_reason IN ('schedule', 'safety_limit')),
    UNIQUE(user_id, store_id, period_key)
);
CREATE UNIQUE INDEX IF NOT EXISTS one_open_work_period ON work_periods(user_id) WHERE ended_at IS NULL;
CREATE TABLE IF NOT EXISTS account_password_attempts (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    failures INTEGER NOT NULL DEFAULT 0,
    locked_until TEXT
);
