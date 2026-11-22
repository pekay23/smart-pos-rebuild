// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use std::{env, error::Error, io};

use sqlx::{postgres::PgPoolOptions, PgPool};
use uuid::Uuid;

use crate::operator::{contains_sensitive_marker, parse_database_options};

pub async fn provision_from_environment(
    issuer: &str,
    subject: &str,
    display_name: &str,
    reason: &str,
) -> Result<String, Box<dyn Error>> {
    let (pool, actor) = connect_provisioner().await?;
    let issuer = normalize_issuer(issuer)?;
    let subject = validate_subject(subject)?;
    let display_name = validate_display_name(display_name)?;
    let reason = validate_reason(reason)?;
    let mut tx = pool.begin().await?;
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT id::text FROM operator_accounts WHERE issuer=$1 AND subject=$2 FOR UPDATE",
    )
    .bind(&issuer)
    .bind(&subject)
    .fetch_optional(&mut *tx)
    .await?;
    if existing.is_some() {
        return Err(input_error(
            "This issuer and subject already has an operator account; use set-status to reactivate or deactivate it.",
        ));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO operator_accounts(id,issuer,subject,role,status) VALUES($1,$2,$3,'support','active')",
    )
    .bind(id)
    .bind(&issuer)
    .bind(&subject)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE operator_directory_profiles SET display_name=$2,updated_at=now() WHERE operator_id=$1")
        .bind(id)
        .bind(&display_name)
        .execute(&mut *tx)
        .await?;
    record_event(
        &mut tx,
        &actor,
        "account_provisioned",
        id,
        &issuer,
        &subject,
        "support",
        "active",
        Some(&display_name),
        &reason,
    )
    .await?;
    tx.commit().await?;
    pool.close().await;
    Ok(id.to_string())
}

pub async fn set_status_from_environment(
    operator_id: &str,
    status: &str,
    reason: &str,
) -> Result<String, Box<dyn Error>> {
    let id =
        Uuid::parse_str(operator_id).map_err(|_| input_error("Operator ID must be a UUID."))?;
    if !matches!(status, "active" | "suspended" | "deactivated") {
        return Err(input_error(
            "Status must be active, suspended or deactivated.",
        ));
    }
    let (pool, actor) = connect_provisioner().await?;
    let reason = validate_reason(reason)?;
    let mut tx = pool.begin().await?;
    let row = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT issuer,subject,role,status FROM operator_accounts WHERE id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| input_error("Operator account was not found."))?;
    let (issuer, subject, role, current_status) = row;
    if current_status == status {
        tx.rollback().await?;
        pool.close().await;
        return Ok(format!(
            "Operator account {id} is already {status}; no change was made."
        ));
    }
    sqlx::query(
        "UPDATE operator_accounts SET status=$2,deactivated_at=CASE WHEN $2='deactivated' THEN now() ELSE NULL END WHERE id=$1",
    )
    .bind(id)
    .bind(status)
    .execute(&mut *tx)
    .await?;
    record_event(
        &mut tx,
        &actor,
        "account_status_changed",
        id,
        &issuer,
        &subject,
        &role,
        status,
        None,
        &reason,
    )
    .await?;
    tx.commit().await?;
    pool.close().await;
    Ok(format!("Operator account {id} is now {status}."))
}

pub async fn set_display_name_from_environment(
    operator_id: &str,
    display_name: &str,
    reason: &str,
) -> Result<String, Box<dyn Error>> {
    let id =
        Uuid::parse_str(operator_id).map_err(|_| input_error("Operator ID must be a UUID."))?;
    let (pool, actor) = connect_provisioner().await?;
    let display_name = validate_display_name(display_name)?;
    let reason = validate_reason(reason)?;
    let mut tx = pool.begin().await?;
    let (issuer, subject, role, status) = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT issuer,subject,role,status FROM operator_accounts WHERE id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| input_error("Operator account was not found."))?;
    sqlx::query("UPDATE operator_directory_profiles SET display_name=$2,updated_at=now() WHERE operator_id=$1")
        .bind(id)
        .bind(&display_name)
        .execute(&mut *tx)
        .await?;
    record_event(
        &mut tx,
        &actor,
        "display_name_changed",
        id,
        &issuer,
        &subject,
        &role,
        &status,
        Some(&display_name),
        &reason,
    )
    .await?;
    tx.commit().await?;
    pool.close().await;
    Ok(format!("Operator display name for {id} was updated."))
}

async fn connect_provisioner() -> Result<(PgPool, String), Box<dyn Error>> {
    let url = env::var("POS_OPERATOR_PROVISIONING_DATABASE_URL")
        .map_err(|_| input_error("POS_OPERATOR_PROVISIONING_DATABASE_URL is required."))?;
    if url.trim().is_empty() {
        return Err(input_error(
            "POS_OPERATOR_PROVISIONING_DATABASE_URL is required.",
        ));
    }
    if env::var("POS_OPERATOR_DATABASE_URL")
        .is_ok_and(|runtime_url| runtime_url.trim() == url.trim())
    {
        return Err(input_error(
            "The provisioning database credential must be separate from the operator runtime credential.",
        ));
    }
    let actor = env::var("POS_OPERATOR_PROVISIONING_ACTOR").map_err(|_| {
        input_error("POS_OPERATOR_PROVISIONING_ACTOR is required for the audit record.")
    })?;
    let actor = actor.trim();
    if actor.is_empty() || actor.chars().count() > 128 || actor.chars().any(char::is_control) {
        return Err(input_error(
            "POS_OPERATOR_PROVISIONING_ACTOR must be 1 to 128 printable characters.",
        ));
    }
    let options = parse_database_options(&url, false)
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(3))
        .connect_with(options)
        .await
        .map_err(|_| input_error("The operator provisioning database could not connect."))?;
    Ok((pool, actor.to_owned()))
}

async fn record_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &str,
    action: &str,
    operator_id: Uuid,
    issuer: &str,
    subject: &str,
    role: &str,
    status: &str,
    display_name: Option<&str>,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO operator_provisioning_events(actor_label,action,target_operator_id,issuer,subject,role,status,display_name,reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(actor)
    .bind(action)
    .bind(operator_id)
    .bind(issuer)
    .bind(subject)
    .bind(role)
    .bind(status)
    .bind(display_name)
    .bind(reason)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn validate_display_name(value: &str) -> Result<String, Box<dyn Error>> {
    let value = value.trim();
    if !(1..=80).contains(&value.chars().count()) || value.chars().any(char::is_control) {
        return Err(input_error(
            "Display name must be 1 to 80 printable characters.",
        ));
    }
    Ok(value.to_owned())
}

fn normalize_issuer(issuer: &str) -> Result<String, Box<dyn Error>> {
    let url = reqwest::Url::parse(issuer.trim())
        .map_err(|_| input_error("Issuer must be a valid HTTPS origin."))?;
    let local_http = url.scheme() == "http"
        && url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
    if (url.scheme() != "https" && !local_http)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(input_error("Issuer must be an HTTPS origin without credentials, path, query or fragment; HTTP is allowed only for loopback development."));
    }
    Ok(url.origin().ascii_serialization())
}

fn validate_subject(subject: &str) -> Result<String, Box<dyn Error>> {
    if subject.is_empty() || subject.chars().count() > 512 || subject.chars().any(char::is_control)
    {
        return Err(input_error(
            "OIDC subject must be 1 to 512 printable characters.",
        ));
    }
    Ok(subject.to_owned())
}

fn validate_reason(reason: &str) -> Result<String, Box<dyn Error>> {
    let reason = reason.trim();
    if reason.is_empty()
        || reason.chars().count() > 500
        || reason.chars().any(char::is_control)
        || contains_sensitive_marker(reason)
    {
        return Err(input_error(
            "A reason of 1 to 500 printable characters is required.",
        ));
    }
    Ok(reason.to_owned())
}

fn input_error(message: &'static str) -> Box<dyn Error> {
    io::Error::new(io::ErrorKind::InvalidInput, message).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issuer_is_normalized_to_the_same_origin_used_by_token_validation() {
        assert_eq!(
            normalize_issuer("https://identity.example/").unwrap(),
            "https://identity.example"
        );
        assert_eq!(
            normalize_issuer("http://127.0.0.1:9000/").unwrap(),
            "http://127.0.0.1:9000"
        );
        assert!(normalize_issuer("https://identity.example/path").is_err());
        assert!(normalize_issuer("http://identity.example/").is_err());
        assert!(normalize_issuer("https://user:pass@identity.example/").is_err());
    }

    #[test]
    fn account_inputs_reject_blank_or_unbounded_values_without_mutating_subjects() {
        assert_eq!(validate_subject("subject-42").unwrap(), "subject-42");
        assert!(validate_subject("").is_err());
        assert!(validate_subject("a\nb").is_err());
        assert!(validate_reason("Add the support operator").is_ok());
        assert!(validate_reason(" \t ").is_err());
        assert!(validate_reason("The operator's password was reset").is_err());
    }
}
