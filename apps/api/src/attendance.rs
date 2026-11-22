// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::HashSet;
use uuid::Uuid;

const SAFETY_SHIFT: Duration = Duration::hours(16);
const MAX_PERIODS: i64 = 100;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ScheduleInput {
    pub weekdays: Vec<u8>,
    pub start_time: String,
    pub end_time: String,
    pub enabled: bool,
    pub expected_revision: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkSchedule {
    pub membership_id: String,
    pub branch_id: String,
    pub timezone: String,
    pub weekdays: Vec<i16>,
    pub start_time: String,
    pub end_time: String,
    pub enabled: bool,
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttendanceStaff {
    pub membership_id: String,
    pub display_name: Option<String>,
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkPeriod {
    pub id: String,
    pub branch_id: String,
    pub membership_id: String,
    pub schedule_revision: Option<i64>,
    pub timezone: String,
    pub scheduled_start_at: DateTime<Utc>,
    pub due_at: DateTime<Utc>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub end_reason: Option<String>,
    pub revision: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MyAttendance {
    pub server_now: DateTime<Utc>,
    pub schedule: Option<WorkSchedule>,
    pub current_period: Option<WorkPeriod>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchAttendanceSummary {
    pub active_staff_count: i64,
    pub started_today_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttendanceError {
    InvalidInput,
    NotFound,
    Conflict,
    IdempotencyConflict,
    Database,
}

pub(crate) async fn get_my(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    member: &str,
    branch: &str,
) -> Result<MyAttendance, AttendanceError> {
    let mut tx = pool.begin().await.map_err(|_| AttendanceError::Database)?;
    set_context(&mut tx, identity, tenant, member, branch).await?;
    let schedule = load_schedule(&mut tx, tenant, branch, member).await?;
    let current = sqlx::query_as::<_, WorkPeriod>(
        "SELECT id::text, branch_id::text, membership_id::text, schedule_revision, timezone,
                scheduled_start_at, due_at, started_at, ended_at, end_reason, revision
         FROM attendance_periods WHERE tenant_id=$1::uuid AND branch_id=$2::uuid
           AND membership_id=$3::uuid AND ended_at IS NULL ORDER BY started_at DESC LIMIT 1",
    )
    .bind(tenant)
    .bind(branch)
    .bind(member)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| AttendanceError::Database)?;
    let server_now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| AttendanceError::Database)?;
    tx.commit().await.map_err(|_| AttendanceError::Database)?;
    Ok(MyAttendance {
        server_now,
        schedule,
        current_period: current,
    })
}

pub(crate) async fn list_schedules(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    member: &str,
    branch: &str,
) -> Result<Vec<WorkSchedule>, AttendanceError> {
    let mut tx = pool.begin().await.map_err(|_| AttendanceError::Database)?;
    set_context(&mut tx, identity, tenant, member, branch).await?;
    let schedules = sqlx::query_as::<_, WorkSchedule>(
        "SELECT membership_id::text, branch_id::text,
                (SELECT timezone FROM branches b WHERE b.tenant_id=s.tenant_id AND b.id=s.branch_id) AS timezone,
                weekdays, to_char(start_time,'HH24:MI') AS start_time,
                to_char(end_time,'HH24:MI') AS end_time, enabled, revision
         FROM attendance_schedules s WHERE tenant_id=$1::uuid AND branch_id=$2::uuid
         ORDER BY membership_id",
    )
    .bind(tenant)
    .bind(branch)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| AttendanceError::Database)?;
    tx.commit().await.map_err(|_| AttendanceError::Database)?;
    Ok(schedules)
}

pub(crate) async fn list_staff(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    member: &str,
    branch: &str,
) -> Result<Vec<AttendanceStaff>, AttendanceError> {
    let mut tx = pool.begin().await.map_err(|_| AttendanceError::Database)?;
    set_context(&mut tx, identity, tenant, member, branch).await?;
    sqlx::query("SELECT set_config('pos.staff_roster_tenant_id',$1,true)")
        .bind(tenant)
        .execute(&mut *tx)
        .await
        .map_err(|_| AttendanceError::Database)?;
    let staff = sqlx::query_as::<_, AttendanceStaff>(
        "SELECT m.id::text AS membership_id, NULLIF(btrim(p.display_name),'') AS display_name,
                array_agg(DISTINCT g.role_key ORDER BY g.role_key) AS roles
         FROM memberships m
         JOIN person_identities p ON p.id=m.identity_id
         JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
         WHERE m.tenant_id=$1::uuid AND m.status='active'
           AND (g.branch_id IS NULL OR g.branch_id=$2::uuid)
         GROUP BY m.id,p.display_name
         ORDER BY lower(COALESCE(NULLIF(btrim(p.display_name),''),'Staff member')),m.id
         LIMIT 250",
    )
    .bind(tenant)
    .bind(branch)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| AttendanceError::Database)?;
    tx.commit().await.map_err(|_| AttendanceError::Database)?;
    Ok(staff)
}

pub(crate) async fn list_periods(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    member: &str,
    branch: &str,
    limit: Option<i64>,
) -> Result<Vec<WorkPeriod>, AttendanceError> {
    let limit = limit.unwrap_or(50);
    if !(1..=MAX_PERIODS).contains(&limit) {
        return Err(AttendanceError::InvalidInput);
    }
    let mut tx = pool.begin().await.map_err(|_| AttendanceError::Database)?;
    set_context(&mut tx, identity, tenant, member, branch).await?;
    sqlx::query("SELECT set_config('pos.attendance_manager_read','true',true)")
        .execute(&mut *tx)
        .await
        .map_err(|_| AttendanceError::Database)?;
    let periods = sqlx::query_as::<_, WorkPeriod>(
        "SELECT id::text, branch_id::text, membership_id::text, schedule_revision, timezone,
                scheduled_start_at, due_at, started_at, ended_at, end_reason, revision
         FROM attendance_periods WHERE tenant_id=$1::uuid AND branch_id=$2::uuid
         ORDER BY started_at DESC, id DESC LIMIT $3",
    )
    .bind(tenant)
    .bind(branch)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| AttendanceError::Database)?;
    tx.commit().await.map_err(|_| AttendanceError::Database)?;
    Ok(periods)
}

pub(crate) async fn branch_summary(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    member: &str,
    branch: &str,
) -> Result<BranchAttendanceSummary, AttendanceError> {
    let mut tx = pool.begin().await.map_err(|_| AttendanceError::Database)?;
    set_context(&mut tx, identity, tenant, member, branch).await?;
    sqlx::query("SELECT set_config('pos.attendance_manager_read','true',true)")
        .execute(&mut *tx)
        .await
        .map_err(|_| AttendanceError::Database)?;
    let summary = sqlx::query_as::<_, BranchAttendanceSummary>(
        "SELECT COUNT(*) FILTER (WHERE ended_at IS NULL)::bigint AS active_staff_count,
                COUNT(*) FILTER (
                    WHERE (started_at AT TIME ZONE timezone)::date =
                          (clock_timestamp() AT TIME ZONE timezone)::date
                )::bigint AS started_today_count
         FROM attendance_periods
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid",
    )
    .bind(tenant)
    .bind(branch)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| AttendanceError::Database)?;
    tx.commit().await.map_err(|_| AttendanceError::Database)?;
    Ok(summary)
}

pub(crate) async fn save_schedule(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    actor: &str,
    branch: &str,
    target_member: &str,
    input: ScheduleInput,
) -> Result<WorkSchedule, AttendanceError> {
    let input = validate_schedule(input)?;
    // PostgreSQL smallint[] is encoded as Vec<i16> by SQLx; Vec<u8> maps to bytea.
    let weekdays: Vec<i16> = input.weekdays.iter().copied().map(i16::from).collect();
    let target = Uuid::parse_str(target_member).map_err(|_| AttendanceError::InvalidInput)?;
    let mut tx = pool.begin().await.map_err(|_| AttendanceError::Database)?;
    set_context(&mut tx, identity, tenant, actor, branch).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("attendance-schedule:{tenant}:{branch}:{target}"))
        .execute(&mut *tx)
        .await
        .map_err(|_| AttendanceError::Database)?;
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM memberships WHERE tenant_id=$1::uuid AND id=$2 AND status='active')",
    )
    .bind(tenant)
    .bind(target)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| AttendanceError::Database)?;
    if !active {
        return Err(AttendanceError::NotFound);
    }
    let timezone: String = sqlx::query_scalar(
        "SELECT timezone FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid",
    )
    .bind(tenant)
    .bind(branch)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| AttendanceError::Database)?
    .ok_or(AttendanceError::NotFound)?;
    if timezone.parse::<Tz>().is_err() {
        return Err(AttendanceError::InvalidInput);
    }

    if input.expected_revision == 0 {
        let inserted = sqlx::query(
            "INSERT INTO attendance_schedules(tenant_id,branch_id,membership_id,weekdays,start_time,end_time,enabled,created_by_membership_id,updated_by_membership_id)
             VALUES($1::uuid,$2::uuid,$3,$4,$5::time,$6::time,$7,$8::uuid,$8::uuid)
             ON CONFLICT(tenant_id,branch_id,membership_id) DO NOTHING",
        )
        .bind(tenant).bind(branch).bind(target).bind(&weekdays).bind(&input.start_time)
        .bind(&input.end_time).bind(input.enabled).bind(actor)
        .execute(&mut *tx).await.map_err(|_| AttendanceError::Database)?;
        if inserted.rows_affected() != 1 {
            return Err(AttendanceError::Conflict);
        }
    } else {
        let updated = sqlx::query(
            "UPDATE attendance_schedules SET weekdays=$4,start_time=$5::time,end_time=$6::time,enabled=$7,
                    revision=revision+1,updated_by_membership_id=$8::uuid,updated_at=clock_timestamp()
             WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND membership_id=$3::uuid AND revision=$9",
        )
        .bind(tenant).bind(branch).bind(target).bind(&weekdays).bind(&input.start_time)
        .bind(&input.end_time).bind(input.enabled).bind(actor).bind(input.expected_revision)
        .execute(&mut *tx).await.map_err(|_| AttendanceError::Database)?;
        if updated.rows_affected() != 1 {
            return Err(AttendanceError::Conflict);
        }
    }
    audit(
        &mut tx,
        tenant,
        actor,
        "attendance.schedule.saved",
        target_member,
        input.expected_revision,
    )
    .await?;
    let schedule = load_schedule(&mut tx, tenant, branch, target_member)
        .await?
        .ok_or(AttendanceError::NotFound)?;
    tx.commit().await.map_err(|_| AttendanceError::Database)?;
    Ok(schedule)
}

pub(crate) async fn clock_in(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    member: &str,
    branch: &str,
    key: &str,
) -> Result<WorkPeriod, AttendanceError> {
    validate_key(key)?;
    let mut tx = pool.begin().await.map_err(|_| AttendanceError::Database)?;
    set_context(&mut tx, identity, tenant, member, branch).await?;
    lock_member(&mut tx, tenant, member).await?;
    if let Some(period) = replay(&mut tx, tenant, member, key, "clocked_in").await? {
        tx.commit().await.map_err(|_| AttendanceError::Database)?;
        return Ok(period);
    }
    let now = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| AttendanceError::Database)?;
    let timezone: String = sqlx::query_scalar(
        "SELECT timezone FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid",
    )
    .bind(tenant)
    .bind(branch)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| AttendanceError::Database)?
    .ok_or(AttendanceError::NotFound)?;
    let schedule = load_schedule(&mut tx, tenant, branch, member).await?;
    let (scheduled_start, due_at) = resolve_period(schedule.as_ref(), &timezone, now)?;
    let row = sqlx::query_as::<_, WorkPeriod>(
        "INSERT INTO attendance_periods(tenant_id,branch_id,membership_id,schedule_revision,timezone,scheduled_start_at,due_at,started_by_membership_id)
         VALUES($1::uuid,$2::uuid,$3::uuid,$4,$5,$6,$7,$3::uuid)
         RETURNING id::text,branch_id::text,membership_id::text,schedule_revision,timezone,scheduled_start_at,due_at,started_at,ended_at,end_reason,revision",
    )
    .bind(tenant).bind(branch).bind(member).bind(schedule.as_ref().map(|value| value.revision))
    .bind(&timezone).bind(scheduled_start).bind(due_at)
    .fetch_one(&mut *tx).await.map_err(|error| {
        if error.as_database_error().is_some_and(|db| db.is_unique_violation()) { AttendanceError::Conflict }
        else { AttendanceError::Database }
    })?;
    sqlx::query(
        "INSERT INTO attendance_events(tenant_id,period_id,branch_id,membership_id,actor_membership_id,event_type,idempotency_key)
         VALUES($1::uuid,$2::uuid,$3::uuid,$4::uuid,$4::uuid,'clocked_in',$5)",
    )
    .bind(tenant).bind(&row.id).bind(branch).bind(member).bind(key)
    .execute(&mut *tx).await.map_err(|error| {
        if error.as_database_error().is_some_and(|db| db.is_unique_violation()) { AttendanceError::IdempotencyConflict }
        else { AttendanceError::Database }
    })?;
    audit(
        &mut tx,
        tenant,
        member,
        "attendance.clocked_in",
        &row.id,
        row.revision,
    )
    .await?;
    tx.commit().await.map_err(|_| AttendanceError::Database)?;
    Ok(row)
}

pub(crate) async fn clock_out(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    member: &str,
    branch: &str,
    key: &str,
) -> Result<WorkPeriod, AttendanceError> {
    validate_key(key)?;
    let mut tx = pool.begin().await.map_err(|_| AttendanceError::Database)?;
    set_context(&mut tx, identity, tenant, member, branch).await?;
    lock_member(&mut tx, tenant, member).await?;
    if let Some(period) = replay(&mut tx, tenant, member, key, "clocked_out").await? {
        tx.commit().await.map_err(|_| AttendanceError::Database)?;
        return Ok(period);
    }
    let id: String = sqlx::query_scalar(
        "UPDATE attendance_periods SET ended_at=GREATEST(clock_timestamp(),started_at),end_reason='staff',
                ended_by_membership_id=$3::uuid,revision=revision+1
         WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND membership_id=$3::uuid AND ended_at IS NULL
         RETURNING id::text",
    )
    .bind(tenant).bind(branch).bind(member).fetch_optional(&mut *tx).await
    .map_err(|_| AttendanceError::Database)?.ok_or(AttendanceError::NotFound)?;
    let row = load_period(&mut tx, tenant, branch, &id)
        .await?
        .ok_or(AttendanceError::NotFound)?;
    sqlx::query(
        "INSERT INTO attendance_events(tenant_id,period_id,branch_id,membership_id,actor_membership_id,event_type,idempotency_key)
         VALUES($1::uuid,$2::uuid,$3::uuid,$4::uuid,$4::uuid,'clocked_out',$5)",
    )
    .bind(tenant).bind(&id).bind(branch).bind(member).bind(key)
    .execute(&mut *tx).await.map_err(|error| {
        if error.as_database_error().is_some_and(|db| db.is_unique_violation()) { AttendanceError::IdempotencyConflict }
        else { AttendanceError::Database }
    })?;
    audit(
        &mut tx,
        tenant,
        member,
        "attendance.clocked_out",
        &id,
        row.revision,
    )
    .await?;
    tx.commit().await.map_err(|_| AttendanceError::Database)?;
    Ok(row)
}

async fn replay(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    member: &str,
    key: &str,
    expected_type: &str,
) -> Result<Option<WorkPeriod>, AttendanceError> {
    let prior = sqlx::query_as::<_, (String, String)>(
        "SELECT event_type,period_id::text FROM attendance_events WHERE tenant_id=$1::uuid AND membership_id=$2::uuid AND idempotency_key=$3",
    ).bind(tenant).bind(member).bind(key).fetch_optional(&mut **tx).await.map_err(|_| AttendanceError::Database)?;
    let Some((event_type, period_id)) = prior else {
        return Ok(None);
    };
    if event_type != expected_type {
        return Err(AttendanceError::IdempotencyConflict);
    }
    load_period(tx, tenant, "", &period_id).await
}

async fn load_period(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    id: &str,
) -> Result<Option<WorkPeriod>, AttendanceError> {
    let query = if branch.is_empty() {
        "SELECT id::text,branch_id::text,membership_id::text,schedule_revision,timezone,scheduled_start_at,due_at,started_at,ended_at,end_reason,revision FROM attendance_periods WHERE tenant_id=$1::uuid AND id=$2::uuid"
    } else {
        "SELECT id::text,branch_id::text,membership_id::text,schedule_revision,timezone,scheduled_start_at,due_at,started_at,ended_at,end_reason,revision FROM attendance_periods WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3::uuid"
    };
    let row = if branch.is_empty() {
        sqlx::query_as::<_, WorkPeriod>(query)
            .bind(tenant)
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
    } else {
        sqlx::query_as::<_, WorkPeriod>(query)
            .bind(tenant)
            .bind(branch)
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
    }
    .map_err(|_| AttendanceError::Database)?;
    Ok(row)
}

async fn load_schedule(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    member: &str,
) -> Result<Option<WorkSchedule>, AttendanceError> {
    sqlx::query_as::<_, WorkSchedule>(
        "SELECT membership_id::text,branch_id::text,
                (SELECT timezone FROM branches b WHERE b.tenant_id=s.tenant_id AND b.id=s.branch_id) AS timezone,
                weekdays,to_char(start_time,'HH24:MI') AS start_time,to_char(end_time,'HH24:MI') AS end_time,enabled,revision
         FROM attendance_schedules s WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND membership_id=$3::uuid",
    )
    .bind(tenant).bind(branch).bind(member).fetch_optional(&mut **tx).await
    .map_err(|_| AttendanceError::Database)
}

fn validate_schedule(mut input: ScheduleInput) -> Result<ScheduleInput, AttendanceError> {
    let days: HashSet<u8> = input.weekdays.iter().copied().collect();
    if input.expected_revision < 0
        || input.weekdays.is_empty()
        || input.weekdays.len() > 7
        || days.len() != input.weekdays.len()
        || input.weekdays.iter().any(|day| *day > 6)
        || parse_time(&input.start_time).is_none()
        || parse_time(&input.end_time).is_none()
        || input.start_time == input.end_time
    {
        return Err(AttendanceError::InvalidInput);
    }
    input.weekdays.sort_unstable();
    Ok(input)
}

fn validate_key(key: &str) -> Result<(), AttendanceError> {
    if key.is_empty() || key.len() > 128 || !key.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) {
        Err(AttendanceError::InvalidInput)
    } else {
        Ok(())
    }
}

fn parse_time(value: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(value, "%H:%M").ok()
}

fn resolve_period(
    schedule: Option<&WorkSchedule>,
    timezone: &str,
    now: DateTime<Utc>,
) -> Result<(DateTime<Utc>, DateTime<Utc>), AttendanceError> {
    let Some(schedule) = schedule.filter(|schedule| schedule.enabled) else {
        return Ok((now, now + SAFETY_SHIFT));
    };
    let zone = timezone
        .parse::<Tz>()
        .map_err(|_| AttendanceError::InvalidInput)?;
    let start_time = parse_time(&schedule.start_time).ok_or(AttendanceError::InvalidInput)?;
    let end_time = parse_time(&schedule.end_time).ok_or(AttendanceError::InvalidInput)?;
    let today = now.with_timezone(&zone).date_naive();
    for days_ago in [1_i64, 0] {
        let date = today - Duration::days(days_ago);
        let weekday = date.weekday().num_days_from_monday() as i16;
        if !schedule.weekdays.contains(&weekday) {
            continue;
        }
        let start = NaiveDateTime::new(date, start_time);
        let mut end_date = date;
        if end_time <= start_time {
            end_date += Duration::days(1);
        }
        let end = NaiveDateTime::new(end_date, end_time);
        if let (Some(start_at), Some(end_at)) = (
            resolve_boundary(zone, start, true),
            resolve_boundary(zone, end, false),
        ) {
            if start_at <= now && now < end_at {
                return Ok((start_at, end_at));
            }
        }
    }
    Ok((now, now + SAFETY_SHIFT))
}

fn resolve_boundary(zone: Tz, local: NaiveDateTime, is_start: bool) -> Option<DateTime<Utc>> {
    let mut candidate = local;
    for _ in 0..=24 * 60 {
        match zone.from_local_datetime(&candidate) {
            LocalResult::Single(value) => return Some(value.with_timezone(&Utc)),
            LocalResult::Ambiguous(first, second) => {
                let first = first.with_timezone(&Utc);
                let second = second.with_timezone(&Utc);
                return Some(if is_start {
                    first.min(second)
                } else {
                    first.max(second)
                });
            }
            LocalResult::None => candidate = candidate.checked_add_signed(Duration::minutes(1))?,
        }
    }
    None
}

async fn lock_member(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    member: &str,
) -> Result<(), AttendanceError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("attendance:{tenant}:{member}"))
        .execute(&mut **tx)
        .await
        .map_err(|_| AttendanceError::Database)?;
    Ok(())
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    tenant: &str,
    member: &str,
    branch: &str,
) -> Result<(), AttendanceError> {
    sqlx::query("SELECT set_config('pos.identity_id',$1,true),set_config('pos.tenant_id',$2,true),set_config('pos.membership_id',$3,true),set_config('pos.branch_id',$4,true)")
        .bind(identity).bind(tenant).bind(member).bind(branch).execute(&mut **tx).await
        .map_err(|_| AttendanceError::Database)?;
    Ok(())
}

async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    actor: &str,
    action: &str,
    entity: &str,
    revision: i64,
) -> Result<(), AttendanceError> {
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,$3,'attendance',$4::uuid,jsonb_build_object('revision',$5))")
        .bind(tenant).bind(actor).bind(action).bind(entity).bind(revision)
        .execute(&mut **tx).await.map_err(|_| AttendanceError::Database)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn schedule(days: Vec<i16>, start_time: &str, end_time: &str) -> WorkSchedule {
        WorkSchedule {
            membership_id: "m".into(),
            branch_id: "b".into(),
            timezone: "Africa/Accra".into(),
            weekdays: days,
            start_time: start_time.into(),
            end_time: end_time.into(),
            enabled: true,
            revision: 1,
        }
    }

    #[test]
    fn unscheduled_clock_in_uses_a_bounded_server_owned_safety_window() {
        let now = DateTime::parse_from_rfc3339("2026-11-04T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let (start, due) = resolve_period(None, "Africa/Accra", now).unwrap();
        assert_eq!(start, now);
        assert_eq!(due - start, SAFETY_SHIFT);
    }

    #[test]
    fn scheduled_late_clock_in_keeps_the_scheduled_end() {
        let now = DateTime::parse_from_rfc3339("2026-11-04T10:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let work = schedule(vec![2], "08:00", "17:00");
        let (start, due) = resolve_period(Some(&work), "Africa/Accra", now).unwrap();
        assert_eq!(start.to_rfc3339(), "2026-11-04T08:00:00+00:00");
        assert_eq!(due.to_rfc3339(), "2026-11-04T17:00:00+00:00");
    }

    #[test]
    fn overnight_schedule_uses_the_prior_workday() {
        let now = DateTime::parse_from_rfc3339("2026-11-05T03:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let work = schedule(vec![2], "22:00", "06:00");
        let (start, due) = resolve_period(Some(&work), "Africa/Accra", now).unwrap();
        assert_eq!(start.to_rfc3339(), "2026-11-04T22:00:00+00:00");
        assert_eq!(due.to_rfc3339(), "2026-11-05T06:00:00+00:00");
    }

    #[test]
    fn daylight_saving_ambiguity_uses_earlier_start_and_later_end() {
        let zone: Tz = "America/New_York".parse().unwrap();
        let start = resolve_boundary(
            zone,
            NaiveDate::from_ymd_opt(2026, 11, 1)
                .unwrap()
                .and_hms_opt(1, 30, 0)
                .unwrap(),
            true,
        )
        .unwrap();
        let end = resolve_boundary(
            zone,
            NaiveDate::from_ymd_opt(2026, 11, 1)
                .unwrap()
                .and_hms_opt(1, 45, 0)
                .unwrap(),
            false,
        )
        .unwrap();
        assert_eq!(start.to_rfc3339(), "2026-11-01T05:30:00+00:00");
        assert_eq!(end.to_rfc3339(), "2026-11-01T06:45:00+00:00");
    }

    #[test]
    fn missing_local_time_advances_to_first_valid_minute() {
        let zone: Tz = "America/New_York".parse().unwrap();
        let start = resolve_boundary(
            zone,
            NaiveDate::from_ymd_opt(2026, 3, 8)
                .unwrap()
                .and_hms_opt(2, 30, 0)
                .unwrap(),
            true,
        )
        .unwrap();
        assert_eq!(start.to_rfc3339(), "2026-03-08T07:00:00+00:00");
    }
}
