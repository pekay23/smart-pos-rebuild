// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::{
    commands::{active_session, add_audit_event},
    db::AppState,
    models::SessionUser,
};
use chrono::{
    DateTime, Datelike, Duration, LocalResult, NaiveDateTime, NaiveTime, SecondsFormat, TimeZone,
    Utc,
};
use chrono_tz::Tz;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

fn stamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSchedule {
    pub user_id: String,
    pub timezone: String,
    pub start_time: String,
    pub end_time: String,
    pub weekdays: Vec<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkPeriod {
    pub id: String,
    pub user_id: String,
    pub full_name: String,
    pub scheduled_start: String,
    pub started_at: String,
    pub due_at: String,
    pub ended_at: Option<String>,
    pub end_reason: Option<String>,
    pub elapsed_seconds: i64,
    pub late_seconds: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttendanceView {
    pub server_now: String,
    pub periods: Vec<WorkPeriod>,
    pub schedules: Vec<WorkSchedule>,
}

pub(crate) fn reconcile(db: &Connection, now: DateTime<Utc>) -> Result<(), String> {
    db.execute("UPDATE work_periods SET ended_at = due_at, end_reason = auto_end_reason WHERE ended_at IS NULL AND due_at <= ?1", [stamp(now)])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Clone, Copy)]
enum ScheduleBoundary {
    Start,
    End,
}

fn resolve_schedule_boundary(
    zone: Tz,
    local: NaiveDateTime,
    boundary: ScheduleBoundary,
) -> Result<DateTime<Tz>, String> {
    let mut candidate = local;
    for _ in 0..=24 * 60 {
        match zone.from_local_datetime(&candidate) {
            LocalResult::Single(value) => return Ok(value),
            LocalResult::Ambiguous(first, second) => {
                let first_utc = first.with_timezone(&Utc);
                let second_utc = second.with_timezone(&Utc);
                return Ok(match boundary {
                    ScheduleBoundary::Start if first_utc <= second_utc => first,
                    ScheduleBoundary::Start => second,
                    ScheduleBoundary::End if first_utc >= second_utc => first,
                    ScheduleBoundary::End => second,
                });
            }
            LocalResult::None => {
                candidate = candidate
                    .checked_add_signed(Duration::minutes(1))
                    .ok_or_else(|| {
                        "The scheduled time is outside the supported date range.".to_owned()
                    })?;
            }
        }
    }
    Err("This timezone has no valid local time after the scheduled boundary.".to_owned())
}

fn schedule_window(
    schedule: &WorkSchedule,
    now: DateTime<Utc>,
) -> Result<Option<(DateTime<Utc>, DateTime<Utc>)>, String> {
    let zone: Tz = schedule
        .timezone
        .parse()
        .map_err(|_| "Choose a valid IANA timezone, such as Africa/Accra.".to_owned())?;
    let start = NaiveTime::parse_from_str(&schedule.start_time, "%H:%M")
        .map_err(|_| "Enter a valid start time.".to_owned())?;
    let end = NaiveTime::parse_from_str(&schedule.end_time, "%H:%M")
        .map_err(|_| "Enter a valid end time.".to_owned())?;
    if start == end || schedule.weekdays.is_empty() || schedule.weekdays.iter().any(|day| *day > 6)
    {
        return Err("Choose working days and different start and end times.".to_owned());
    }
    let today = now.with_timezone(&zone).date_naive();
    for offset in [0, 1] {
        let date = today - Duration::days(offset);
        if !schedule
            .weekdays
            .contains(&date.weekday().num_days_from_monday())
        {
            continue;
        }
        let end_date = if end <= start {
            date + Duration::days(1)
        } else {
            date
        };
        // Fall-back ambiguity includes the first repeated instant at shift start and
        // the last repeated instant at shift end. Spring-forward gaps move forward
        // to the first valid local minute, so the scheduled period is never skipped.
        let from = resolve_schedule_boundary(zone, date.and_time(start), ScheduleBoundary::Start)?
            .with_timezone(&Utc);
        let mut to =
            resolve_schedule_boundary(zone, end_date.and_time(end), ScheduleBoundary::End)?
                .with_timezone(&Utc);
        if to <= from {
            // If both boundaries fall inside the same spring-forward gap, wall-time
            // adjustment can collapse a short shift to zero. Preserve its intended
            // scheduled duration from the first valid start instant.
            let nominal_duration = end_date
                .and_time(end)
                .signed_duration_since(date.and_time(start));
            to = from + nominal_duration;
        }
        if now >= from && now < to {
            return Ok(Some((from, to)));
        }
    }
    Ok(None)
}

fn read_schedule(db: &Connection, user: &SessionUser) -> Result<Option<WorkSchedule>, String> {
    let row = db.query_row("SELECT timezone,start_time,end_time,weekdays_json FROM work_schedules WHERE user_id=?1 AND store_id=?2",
        params![user.user_id,user.store_id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?)))
        .optional().map_err(|e| e.to_string())?;
    row.map(|(timezone, start_time, end_time, days)| {
        Ok(WorkSchedule {
            user_id: user.user_id.clone(),
            timezone,
            start_time,
            end_time,
            weekdays: serde_json::from_str(&days)
                .map_err(|_| "The saved work schedule is invalid.".to_owned())?,
        })
    })
    .transpose()
}

pub(crate) fn start_for_login(
    db: &Connection,
    user: &SessionUser,
    now: DateTime<Utc>,
) -> Result<(), String> {
    start_work_period(db, user, now, false)
}

pub(crate) fn start_for_manual_action(
    db: &Connection,
    user: &SessionUser,
    now: DateTime<Utc>,
) -> Result<(), String> {
    start_work_period(db, user, now, true)
}

fn start_work_period(
    db: &Connection,
    user: &SessionUser,
    now: DateTime<Utc>,
    explicit_start: bool,
) -> Result<(), String> {
    reconcile(db, now)?;
    let open: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM work_periods WHERE user_id=?1 AND store_id=?2 AND ended_at IS NULL)",
            params![user.user_id, user.store_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if open {
        return Ok(());
    }
    let (key, from, due, reason) = if let Some(schedule) = read_schedule(db, user)? {
        match schedule_window(&schedule, now)? {
            Some((from, to)) => (stamp(from), from, to, "schedule"),
            None if explicit_start => {
                let zone: Tz = schedule
                    .timezone
                    .parse()
                    .map_err(|_| "The saved work schedule timezone is invalid.".to_owned())?;
                (
                    now.with_timezone(&zone).date_naive().to_string(),
                    now,
                    now + Duration::hours(16),
                    "safety_limit",
                )
            }
            None => return Ok(()),
        }
    } else {
        if user.workspace_mode != "cashier" && !explicit_start {
            return Ok(());
        }
        let timezone: String = db
            .query_row(
                "SELECT timezone FROM businesses WHERE id=?1",
                [&user.business_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let zone: Tz = timezone
            .parse()
            .map_err(|_| "The business timezone is invalid.".to_owned())?;
        (
            now.with_timezone(&zone).date_naive().to_string(),
            now,
            now + Duration::hours(16),
            "safety_limit",
        )
    };
    // Login is idempotent even after a completed shift. A repeated start cannot reopen a closed period.
    db.execute("INSERT OR IGNORE INTO work_periods(id,user_id,store_id,period_key,scheduled_start,started_at,due_at,auto_end_reason) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![Uuid::new_v4().to_string(),user.user_id,user.store_id,key,stamp(from),stamp(now),stamp(due),reason]).map_err(|e|e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_attendance(state: State<'_, AppState>) -> Result<AttendanceView, String> {
    let user = active_session(&state, "")?;
    let db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let now = Utc::now();
    reconcile(&db, now)?;
    let manager = user
        .permissions
        .iter()
        .any(|p| p == "reports:read" || p == "users:write");
    let mut query=db.prepare("SELECT p.id,p.user_id,u.full_name,p.scheduled_start,p.started_at,p.due_at,p.ended_at,p.end_reason FROM work_periods p JOIN users u ON u.id=p.user_id WHERE p.store_id=?1 AND u.business_id=?2 AND (?3=1 OR p.user_id=?4) ORDER BY p.started_at DESC LIMIT 200").map_err(|e|e.to_string())?;
    let rows = query
        .query_map(
            params![user.store_id, user.business_id, manager, user.user_id],
            |r| {
                Ok(WorkPeriod {
                    id: r.get(0)?,
                    user_id: r.get(1)?,
                    full_name: r.get(2)?,
                    scheduled_start: r.get(3)?,
                    started_at: r.get(4)?,
                    due_at: r.get(5)?,
                    ended_at: r.get(6)?,
                    end_reason: r.get(7)?,
                    elapsed_seconds: 0,
                    late_seconds: 0,
                })
            },
        )
        .map_err(|e| e.to_string())?;
    let mut periods = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for period in &mut periods {
        let start = DateTime::parse_from_rfc3339(&period.started_at)
            .map_err(|e| e.to_string())?
            .with_timezone(&Utc);
        let scheduled = DateTime::parse_from_rfc3339(&period.scheduled_start)
            .map_err(|e| e.to_string())?
            .with_timezone(&Utc);
        let end = period
            .ended_at
            .as_ref()
            .map(|s| DateTime::parse_from_rfc3339(s))
            .transpose()
            .map_err(|e| e.to_string())?
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or(now);
        period.elapsed_seconds = (end - start).num_seconds().max(0);
        period.late_seconds = (start - scheduled).num_seconds().max(0);
    }
    let mut query=db.prepare("SELECT w.user_id,w.timezone,w.start_time,w.end_time,w.weekdays_json FROM work_schedules w JOIN users u ON u.id=w.user_id WHERE w.store_id=?1 AND u.business_id=?2 AND (?3=1 OR w.user_id=?4)").map_err(|e|e.to_string())?;
    let schedules = query
        .query_map(
            params![user.store_id, user.business_id, manager, user.user_id],
            |r| {
                let days: String = r.get(4)?;
                Ok(WorkSchedule {
                    user_id: r.get(0)?,
                    timezone: r.get(1)?,
                    start_time: r.get(2)?,
                    end_time: r.get(3)?,
                    weekdays: serde_json::from_str(&days).unwrap_or_default(),
                })
            },
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(AttendanceView {
        server_now: stamp(now),
        periods,
        schedules,
    })
}

#[tauri::command]
pub fn save_work_schedule(input: WorkSchedule, state: State<'_, AppState>) -> Result<(), String> {
    let user = active_session(&state, "users:write")?;
    schedule_window(&input, Utc::now())?;
    let mut db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id=?1 AND business_id=?2 AND is_active=1)",
            params![input.user_id, user.business_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Err("Choose an active staff member from this business.".to_owned());
    }
    tx.execute("INSERT INTO work_schedules(user_id,store_id,timezone,start_time,end_time,weekdays_json) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(user_id,store_id) DO UPDATE SET timezone=excluded.timezone,start_time=excluded.start_time,end_time=excluded.end_time,weekdays_json=excluded.weekdays_json",
        params![input.user_id,user.store_id,input.timezone,input.start_time,input.end_time,serde_json::to_string(&input.weekdays).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
    add_audit_event(
        &tx,
        &user,
        "work_schedule_saved",
        "user",
        Some(&input.user_id),
        serde_json::to_value(&input).map_err(|e| e.to_string())?,
    )?;
    tx.commit().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn start_work(state: State<'_, AppState>) -> Result<(), String> {
    let user = active_session(&state, "")?;
    let db = state.db.lock().map_err(|_| "Database unavailable.")?;
    start_for_manual_action(&db, &user, Utc::now())?;
    let open:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM work_periods WHERE user_id=?1 AND store_id=?2 AND ended_at IS NULL)",params![user.user_id,user.store_id],|r|r.get(0)).map_err(|e|e.to_string())?;
    if !open {
        return Err("There is no available work period now. It may be outside your schedule or already closed.".to_owned());
    }
    Ok(())
}

#[tauri::command]
pub fn end_work(state: State<'_, AppState>) -> Result<(), String> {
    let user = active_session(&state, "")?;
    let db = state.db.lock().map_err(|_| "Database unavailable.")?;
    let now = Utc::now();
    reconcile(&db, now)?;
    db.execute("UPDATE work_periods SET ended_at=MAX(started_at,?1),end_reason='staff' WHERE user_id=?2 AND store_id=?3 AND ended_at IS NULL",params![stamp(now),user.user_id,user.store_id]).map_err(|e|e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn time(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }
    fn schedule() -> WorkSchedule {
        WorkSchedule {
            user_id: "u".into(),
            timezone: "Africa/Accra".into(),
            start_time: "08:00".into(),
            end_time: "17:00".into(),
            weekdays: vec![0, 1, 2, 3, 4],
        }
    }
    #[test]
    fn late_login_preserves_scheduled_end() {
        let (start, end) = schedule_window(&schedule(), time("2026-10-29T10:30:00Z"))
            .unwrap()
            .unwrap();
        assert_eq!(start, time("2026-10-29T08:00:00Z"));
        assert_eq!(end, time("2026-10-29T17:00:00Z"));
    }
    #[test]
    fn does_not_start_before_shift_or_on_weekend() {
        assert!(schedule_window(&schedule(), time("2026-10-29T07:59:00Z"))
            .unwrap()
            .is_none());
        assert!(schedule_window(&schedule(), time("2026-10-31T10:00:00Z"))
            .unwrap()
            .is_none());
    }
    #[test]
    fn overnight_uses_previous_workday() {
        let mut s = schedule();
        s.start_time = "22:00".into();
        s.end_time = "06:00".into();
        let (start, end) = schedule_window(&s, time("2026-10-31T03:00:00Z"))
            .unwrap()
            .unwrap();
        assert_eq!(start, time("2026-10-30T22:00:00Z"));
        assert_eq!(end, time("2026-10-31T06:00:00Z"));
    }
    #[test]
    fn fall_back_uses_first_valid_start_and_last_valid_end() {
        let mut s = schedule();
        s.timezone = "America/New_York".into();
        s.start_time = "01:30".into();
        s.end_time = "01:45".into();
        s.weekdays = vec![6];

        let (start, end) = schedule_window(&s, time("2026-11-01T05:40:00Z"))
            .unwrap()
            .unwrap();

        assert_eq!(start, time("2026-11-01T05:30:00Z"));
        assert_eq!(end, time("2026-11-01T06:45:00Z"));
    }

    #[test]
    fn spring_forward_moves_a_nonexistent_boundary_to_the_first_valid_minute() {
        let mut s = schedule();
        s.timezone = "America/New_York".into();
        s.start_time = "02:30".into();
        s.end_time = "04:00".into();
        s.weekdays = vec![6];

        let (start, end) = schedule_window(&s, time("2026-03-08T07:15:00Z"))
            .unwrap()
            .unwrap();

        assert_eq!(start, time("2026-03-08T07:00:00Z"));
        assert_eq!(end, time("2026-03-08T08:00:00Z"));
    }

    #[test]
    fn spring_forward_preserves_a_short_shift_inside_the_gap() {
        let mut s = schedule();
        s.timezone = "America/New_York".into();
        s.start_time = "02:30".into();
        s.end_time = "02:45".into();
        s.weekdays = vec![6];

        let (start, end) = schedule_window(&s, time("2026-03-08T07:05:00Z"))
            .unwrap()
            .unwrap();

        assert_eq!(start, time("2026-03-08T07:00:00Z"));
        assert_eq!(end, time("2026-03-08T07:15:00Z"));
    }

    #[test]
    fn restart_closes_at_due_time_not_restart_time() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE work_periods (due_at TEXT,ended_at TEXT,end_reason TEXT,auto_end_reason TEXT); INSERT INTO work_periods VALUES ('2026-10-29T17:00:00.000Z',NULL,NULL,'schedule');").unwrap();
        reconcile(&db, time("2026-10-30T10:00:00Z")).unwrap();
        let end: String = db
            .query_row("SELECT ended_at FROM work_periods", [], |r| r.get(0))
            .unwrap();
        assert_eq!(end, "2026-10-29T17:00:00.000Z");
    }
    fn fixture() -> (Connection, SessionUser) {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../migrations/0001_initial.sql"))
            .unwrap();
        db.execute_batch(include_str!("../migrations/0007_identity_attendance.sql"))
            .unwrap();
        db.execute_batch("INSERT INTO businesses(id,name,created_at) VALUES('b','Business','2026-01-01'); INSERT INTO stores(id,business_id,name,created_at) VALUES('s','b','Store','2026-01-01'); INSERT INTO roles VALUES('r','b','cashier','Cashier','[]',1); INSERT INTO users(id,business_id,role_id,username,password_hash,full_name,created_at) VALUES('u','b','r','cashier','hash','Cashier','2026-01-01'); INSERT INTO work_schedules VALUES('u','s','Africa/Accra','08:00','17:00','[0,1,2,3,4]');").unwrap();
        (
            db,
            SessionUser {
                user_id: "u".into(),
                business_id: "b".into(),
                store_id: "s".into(),
                full_name: "Cashier".into(),
                username: "cashier".into(),
                role_key: "cashier".into(),
                role_name: "Cashier".into(),
                workspace_mode: "cashier".into(),
                is_owner: false,
                permissions: vec![],
                business_name: "Business".into(),
                store_name: "Store".into(),
                currency_code: "GHS".into(),
            },
        )
    }

    #[test]
    fn unscheduled_management_login_does_not_start_attendance() {
        let (db, mut user) = fixture();
        user.workspace_mode = "management".into();
        db.execute("DELETE FROM work_schedules WHERE user_id='u'", [])
            .unwrap();

        start_for_login(&db, &user, time("2026-10-29T09:00:00Z")).unwrap();

        let periods: i64 = db
            .query_row("SELECT count(*) FROM work_periods", [], |r| r.get(0))
            .unwrap();
        assert_eq!(periods, 0);
    }

    #[test]
    fn management_can_explicitly_start_unscheduled_attendance() {
        let (db, mut user) = fixture();
        user.workspace_mode = "management".into();
        db.execute("DELETE FROM work_schedules WHERE user_id='u'", [])
            .unwrap();

        start_for_manual_action(&db, &user, time("2026-10-29T09:00:00Z")).unwrap();

        let (count, reason): (i64, String) = db
            .query_row(
                "SELECT count(*),min(auto_end_reason) FROM work_periods",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(reason, "safety_limit");
    }

    #[test]
    fn management_manual_start_outside_schedule_records_actual_start_with_safety_end() {
        let (db, mut user) = fixture();
        user.workspace_mode = "management".into();
        let started = time("2026-10-29T20:00:00Z");

        start_for_manual_action(&db, &user, started).unwrap();

        let (count, start, due, reason): (i64, String, String, String) = db
            .query_row(
                "SELECT count(*),min(started_at),min(due_at),min(auto_end_reason) FROM work_periods",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(start, "2026-10-29T20:00:00.000Z");
        assert_eq!(due, "2026-10-30T12:00:00.000Z");
        assert_eq!(reason, "safety_limit");
    }

    #[test]
    fn manual_start_inside_schedule_uses_scheduled_end() {
        let (db, mut user) = fixture();
        user.workspace_mode = "management".into();

        start_for_manual_action(&db, &user, time("2026-10-29T10:00:00Z")).unwrap();

        let (due, reason): (String, String) = db
            .query_row(
                "SELECT due_at,auto_end_reason FROM work_periods",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(due, "2026-10-29T17:00:00.000Z");
        assert_eq!(reason, "schedule");
    }

    #[test]
    fn management_login_starts_a_period_when_inside_assigned_schedule() {
        let (db, mut user) = fixture();
        user.workspace_mode = "management".into();

        start_for_login(&db, &user, time("2026-10-29T09:00:00Z")).unwrap();

        let (count, reason): (i64, String) = db
            .query_row(
                "SELECT count(*),min(auto_end_reason) FROM work_periods",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(reason, "schedule");
    }

    #[test]
    fn unscheduled_cashier_login_keeps_safety_limited_auto_start() {
        let (db, user) = fixture();
        db.execute("DELETE FROM work_schedules WHERE user_id='u'", [])
            .unwrap();

        start_for_login(&db, &user, time("2026-10-29T09:00:00Z")).unwrap();

        let reason: String = db
            .query_row("SELECT auto_end_reason FROM work_periods", [], |r| r.get(0))
            .unwrap();
        assert_eq!(reason, "safety_limit");
    }

    #[test]
    fn login_after_logout_resumes_and_manual_end_does_not_restart() {
        let (db, user) = fixture();
        start_for_login(&db, &user, time("2026-10-29T09:00:00Z")).unwrap();
        start_for_login(&db, &user, time("2026-10-29T10:00:00Z")).unwrap();
        let (count, start): (i64, String) = db
            .query_row(
                "SELECT count(*),min(started_at) FROM work_periods",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(start, "2026-10-29T09:00:00.000Z");
        db.execute(
            "UPDATE work_periods SET ended_at='2026-10-29T12:00:00.000Z',end_reason='staff'",
            [],
        )
        .unwrap();
        start_for_login(&db, &user, time("2026-10-29T13:00:00Z")).unwrap();
        let open: i64 = db
            .query_row(
                "SELECT count(*) FROM work_periods WHERE ended_at IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(open, 0);
    }
    #[test]
    fn next_day_login_closes_old_period_and_starts_actual_arrival() {
        let (db, user) = fixture();
        start_for_login(&db, &user, time("2026-10-29T09:00:00Z")).unwrap();
        start_for_login(&db, &user, time("2026-10-30T09:15:00Z")).unwrap();
        let end: String = db
            .query_row(
                "SELECT ended_at FROM work_periods WHERE started_at='2026-10-29T09:00:00.000Z'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(end, "2026-10-29T17:00:00.000Z");
        let start: String = db
            .query_row(
                "SELECT started_at FROM work_periods WHERE ended_at IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(start, "2026-10-30T09:15:00.000Z");
    }
}
