// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CustomerError {
    InvalidInput,
    Conflict,
    Database,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CustomerInput {
    pub name: String,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostedCustomer {
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub status: String,
    pub revision: i32,
    pub merged_into_customer_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CustomerListQuery {
    pub query: Option<String>,
    pub offset: Option<i64>,
    pub page_size: Option<i64>,
    pub include_archived: Option<bool>,
}

impl CustomerListQuery {
    pub(crate) fn validated(self) -> Result<(Option<String>, i64, i64, bool), CustomerError> {
        let query = self
            .query
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        if query
            .as_ref()
            .is_some_and(|value| value.chars().count() > 100)
        {
            return Err(CustomerError::InvalidInput);
        }
        let offset = self.offset.unwrap_or(0);
        let page_size = self.page_size.unwrap_or(100);
        if !(0..=100_000).contains(&offset) || !(1..=100).contains(&page_size) {
            return Err(CustomerError::InvalidInput);
        }
        Ok((
            query,
            offset,
            page_size,
            self.include_archived.unwrap_or(false),
        ))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CustomerUpdateInput {
    pub name: String,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub notes: Option<String>,
    pub expected_revision: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CustomerStatusInput {
    pub status: String,
    pub expected_revision: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CustomerMergeInput {
    pub target_customer_id: String,
    pub expected_source_revision: i32,
    pub expected_target_revision: i32,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CustomerMergeResult {
    pub source: HostedCustomer,
    pub target: HostedCustomer,
    pub merged_at: String,
    pub replayed: bool,
}

fn normalize(input: CustomerInput) -> Result<CustomerInput, CustomerError> {
    let name = input.name.trim().to_owned();
    let phone = input
        .phone
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let email = input
        .email
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let notes = input
        .notes
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if name.is_empty()
        || name.chars().count() > 160
        || phone
            .as_ref()
            .is_some_and(|value| value.chars().count() > 40)
        || email
            .as_ref()
            .is_some_and(|value| value.chars().count() > 254 || !value.contains('@'))
        || notes
            .as_ref()
            .is_some_and(|value| value.chars().count() > 1000)
    {
        return Err(CustomerError::InvalidInput);
    }
    Ok(CustomerInput {
        name,
        phone,
        email,
        notes,
    })
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
) -> Result<(), CustomerError> {
    for (key, value) in [
        ("pos.identity_id", identity_id),
        ("pos.tenant_id", tenant_id),
        ("pos.membership_id", membership_id),
        ("pos.branch_id", branch_id),
        ("pos.station_id", station_id.unwrap_or("")),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut **tx)
            .await
            .map_err(|_| CustomerError::Database)?;
    }
    Ok(())
}

pub(crate) async fn list(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
    query: CustomerListQuery,
) -> Result<Vec<HostedCustomer>, CustomerError> {
    let (query, offset, page_size, include_archived) = query.validated()?;
    let mut tx = pool.begin().await.map_err(|_| CustomerError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    let pattern = query.map(|value| {
        let escaped = value
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        format!("%{escaped}%")
    });
    let rows = sqlx::query_as::<_, HostedCustomer>("SELECT id::text, name, phone, email, notes, created_at::text, status, revision, merged_into_customer_id::text FROM hosted_customers WHERE tenant_id=$1::uuid AND (status='active' OR ($5 AND status IN ('archived','merged'))) AND ($2::text IS NULL OR name ILIKE $2 ESCAPE E'\\\\' OR COALESCE(phone,'') ILIKE $2 ESCAPE E'\\\\' OR COALESCE(email,'') ILIKE $2 ESCAPE E'\\\\') ORDER BY lower(name), id LIMIT $3 OFFSET $4")
        .bind(tenant_id).bind(pattern).bind(page_size).bind(offset).bind(include_archived).fetch_all(&mut *tx).await.map_err(|_| CustomerError::Database)?;
    tx.commit().await.map_err(|_| CustomerError::Database)?;
    Ok(rows)
}

pub(crate) async fn update(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
    customer_id: &str,
    input: CustomerUpdateInput,
) -> Result<HostedCustomer, CustomerError> {
    let expected_revision = input.expected_revision;
    if expected_revision < 1 {
        return Err(CustomerError::InvalidInput);
    }
    let input = normalize(CustomerInput {
        name: input.name,
        phone: input.phone,
        email: input.email,
        notes: input.notes,
    })?;
    let mut tx = pool.begin().await.map_err(|_| CustomerError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    let row = sqlx::query_as::<_, HostedCustomer>(
        "UPDATE hosted_customers SET name=$4, phone=$5, email=$6, notes=$7, revision=revision+1, updated_at=now() \
         WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active' AND revision=$3 \
         RETURNING id::text,name,phone,email,notes,created_at::text,status,revision,merged_into_customer_id::text",
    ).bind(tenant_id).bind(customer_id).bind(expected_revision).bind(&input.name).bind(&input.phone).bind(&input.email).bind(&input.notes)
        .fetch_optional(&mut *tx).await.map_err(|_| CustomerError::Database)?.ok_or(CustomerError::Conflict)?;
    sqlx::query("INSERT INTO audit_events (tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES ($1::uuid,$2::uuid,'customer_updated','customer',$3::uuid,$4)")
        .bind(tenant_id).bind(membership_id).bind(customer_id).bind(serde_json::json!({"branchId":branch_id,"stationId":station_id,"revision":expected_revision+1}))
        .execute(&mut *tx).await.map_err(|_| CustomerError::Database)?;
    tx.commit().await.map_err(|_| CustomerError::Database)?;
    Ok(row)
}

pub(crate) async fn set_status(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
    customer_id: &str,
    input: CustomerStatusInput,
) -> Result<HostedCustomer, CustomerError> {
    if input.expected_revision < 1 || !matches!(input.status.as_str(), "active" | "archived") {
        return Err(CustomerError::InvalidInput);
    }
    let mut tx = pool.begin().await.map_err(|_| CustomerError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    let row = sqlx::query_as::<_, HostedCustomer>(
        "UPDATE hosted_customers SET status=$4, revision=revision+1, updated_at=now() \
         WHERE tenant_id=$1::uuid AND id=$2::uuid AND revision=$3 AND status IN ('active','archived') AND status<>$4 \
           AND ($4 <> 'archived' OR NOT EXISTS (SELECT 1 FROM hosted_customer_merges m WHERE m.tenant_id=$1::uuid AND m.target_customer_id=$2::uuid)) \
         RETURNING id::text,name,phone,email,notes,created_at::text,status,revision,merged_into_customer_id::text",
    )
    .bind(tenant_id)
    .bind(customer_id)
    .bind(input.expected_revision)
    .bind(&input.status)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| CustomerError::Database)?
    .ok_or(CustomerError::Conflict)?;
    sqlx::query("INSERT INTO audit_events (tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES ($1::uuid,$2::uuid,$3,'customer',$4::uuid,$5)")
        .bind(tenant_id).bind(membership_id).bind(if input.status == "archived" { "customer_archived" } else { "customer_restored" })
        .bind(customer_id).bind(serde_json::json!({"branchId":branch_id,"stationId":station_id,"revision":input.expected_revision+1}))
        .execute(&mut *tx).await.map_err(|_| CustomerError::Database)?;
    tx.commit().await.map_err(|_| CustomerError::Database)?;
    Ok(row)
}

pub(crate) async fn create(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
    idempotency_key: &str,
    raw_body: &[u8],
    input: CustomerInput,
) -> Result<HostedCustomer, CustomerError> {
    let input = normalize(input)?;
    if idempotency_key.is_empty()
        || idempotency_key.len() > 128
        || !idempotency_key
            .bytes()
            .all(|byte| (0x21..=0x7e).contains(&byte))
    {
        return Err(CustomerError::InvalidInput);
    }
    let request_hash = Sha256::digest(raw_body).to_vec();
    let mut tx = pool.begin().await.map_err(|_| CustomerError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    let lock_key = format!("{tenant_id}:{membership_id}:{idempotency_key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(|_| CustomerError::Database)?;
    if let Some((customer_id, old_hash)) = sqlx::query_as::<_, (String, Vec<u8>)>("SELECT id::text,creation_request_hash FROM hosted_customers WHERE tenant_id=$1::uuid AND created_by_membership_id=$2::uuid AND creation_idempotency_key=$3")
        .bind(tenant_id).bind(membership_id).bind(idempotency_key).fetch_optional(&mut *tx).await.map_err(|_| CustomerError::Database)? {
        if old_hash != request_hash { return Err(CustomerError::Conflict); }
    let customer = sqlx::query_as::<_, HostedCustomer>("SELECT id::text,name,phone,email,notes,created_at::text,status,revision,merged_into_customer_id::text FROM hosted_customers WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active'")
            .bind(tenant_id).bind(customer_id).fetch_one(&mut *tx).await.map_err(|_| CustomerError::Database)?;
        tx.commit().await.map_err(|_| CustomerError::Database)?;
        return Ok(customer);
    }
    let customer_id = Uuid::new_v4();
    let row = sqlx::query_as::<_, HostedCustomer>("INSERT INTO hosted_customers (id,tenant_id,name,phone,email,notes,status,created_by_membership_id,creation_idempotency_key,creation_request_hash) VALUES ($1,$2::uuid,$3,$4,$5,$6,'active',$7::uuid,$8,$9) RETURNING id::text,name,phone,email,notes,created_at::text,status,revision,merged_into_customer_id::text")
        .bind(customer_id).bind(tenant_id).bind(&input.name).bind(&input.phone).bind(&input.email).bind(&input.notes).bind(membership_id).bind(idempotency_key).bind(&request_hash)
        .fetch_one(&mut *tx).await.map_err(|error| if error.as_database_error().is_some_and(|database| database.is_unique_violation()) { CustomerError::Conflict } else { CustomerError::Database })?;
    sqlx::query("INSERT INTO audit_events (tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES ($1::uuid,$2::uuid,'customer_created','customer',$3::uuid,$4)")
        .bind(tenant_id).bind(membership_id).bind(&row.id).bind(serde_json::json!({"branchId":branch_id,"stationId":station_id})).execute(&mut *tx).await.map_err(|_| CustomerError::Database)?;
    tx.commit().await.map_err(|_| CustomerError::Database)?;
    Ok(row)
}

pub(crate) async fn merge(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    source_customer_id: &str,
    raw_body: &[u8],
    input: CustomerMergeInput,
) -> Result<CustomerMergeResult, CustomerError> {
    let source_id = Uuid::parse_str(source_customer_id).map_err(|_| CustomerError::InvalidInput)?;
    let target_id =
        Uuid::parse_str(&input.target_customer_id).map_err(|_| CustomerError::InvalidInput)?;
    if source_id == target_id
        || input.expected_source_revision < 1
        || input.expected_target_revision < 1
        || input.idempotency_key.is_empty()
        || input.idempotency_key.len() > 128
        || !input
            .idempotency_key
            .bytes()
            .all(|byte| (0x21..=0x7e).contains(&byte))
    {
        return Err(CustomerError::InvalidInput);
    }
    let request_hash = Sha256::digest(raw_body).to_vec();
    let mut tx = pool.begin().await.map_err(|_| CustomerError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        None,
    )
    .await?;
    let lock_key = format!("{tenant_id}:customer-merge:{source_id}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(|_| CustomerError::Database)?;

    if let Some((stored_hash,)) = sqlx::query_as::<_, (Vec<u8>,)>(
        "SELECT request_hash FROM hosted_customer_merges WHERE tenant_id=$1::uuid AND source_customer_id=$2",
    ).bind(tenant_id).bind(source_id).fetch_optional(&mut *tx).await.map_err(|_| CustomerError::Database)? {
        if stored_hash != request_hash { return Err(CustomerError::Conflict); }
        let source = sqlx::query_as::<_, HostedCustomer>("SELECT id::text,name,phone,email,notes,created_at::text,status,revision,merged_into_customer_id::text FROM hosted_customers WHERE tenant_id=$1::uuid AND id=$2")
            .bind(tenant_id).bind(source_id).fetch_optional(&mut *tx).await.map_err(|_| CustomerError::Database)?.ok_or(CustomerError::Conflict)?;
        let target = sqlx::query_as::<_, HostedCustomer>("SELECT id::text,name,phone,email,notes,created_at::text,status,revision,merged_into_customer_id::text FROM hosted_customers WHERE tenant_id=$1::uuid AND id=$2")
            .bind(tenant_id).bind(target_id).fetch_optional(&mut *tx).await.map_err(|_| CustomerError::Database)?.ok_or(CustomerError::Conflict)?;
        let (merged_at,) = sqlx::query_as::<_, (String,)>("SELECT merged_at::text FROM hosted_customer_merges WHERE tenant_id=$1::uuid AND source_customer_id=$2")
            .bind(tenant_id).bind(source_id).fetch_one(&mut *tx).await.map_err(|_| CustomerError::Database)?;
        tx.commit().await.map_err(|_| CustomerError::Database)?;
        return Ok(CustomerMergeResult { source, target, merged_at, replayed: true });
    }

    // A deterministic row-lock order prevents simultaneous reverse merges deadlocking.
    sqlx::query("SELECT id FROM hosted_customers WHERE tenant_id=$1::uuid AND id=ANY($2) ORDER BY id FOR UPDATE")
        .bind(tenant_id).bind(vec![source_id, target_id]).fetch_all(&mut *tx).await.map_err(|_| CustomerError::Database)?;
    let source = sqlx::query_as::<_, HostedCustomer>("SELECT id::text,name,phone,email,notes,created_at::text,status,revision,merged_into_customer_id::text FROM hosted_customers WHERE tenant_id=$1::uuid AND id=$2 AND status='active'")
        .bind(tenant_id).bind(source_id).fetch_optional(&mut *tx).await.map_err(|_| CustomerError::Database)?.ok_or(CustomerError::Conflict)?;
    let target = sqlx::query_as::<_, HostedCustomer>("SELECT id::text,name,phone,email,notes,created_at::text,status,revision,merged_into_customer_id::text FROM hosted_customers WHERE tenant_id=$1::uuid AND id=$2 AND status='active'")
        .bind(tenant_id).bind(target_id).fetch_optional(&mut *tx).await.map_err(|_| CustomerError::Database)?.ok_or(CustomerError::Conflict)?;
    if source.revision != input.expected_source_revision
        || target.revision != input.expected_target_revision
    {
        return Err(CustomerError::Conflict);
    }
    let merged_source = sqlx::query_as::<_, HostedCustomer>(
        "UPDATE hosted_customers SET status='merged',merged_into_customer_id=$3,revision=revision+1,updated_at=now() WHERE tenant_id=$1::uuid AND id=$2 AND status='active' AND revision=$4 RETURNING id::text,name,phone,email,notes,created_at::text,status,revision,merged_into_customer_id::text",
    ).bind(tenant_id).bind(source_id).bind(target_id).bind(input.expected_source_revision)
      .fetch_optional(&mut *tx).await.map_err(|_| CustomerError::Database)?.ok_or(CustomerError::Conflict)?;
    sqlx::query("INSERT INTO hosted_customer_merges (tenant_id,source_customer_id,target_customer_id,branch_id,actor_membership_id,source_revision,target_revision,idempotency_key,request_hash) VALUES ($1::uuid,$2,$3,$4::uuid,$5::uuid,$6,$7,$8,$9)")
        .bind(tenant_id).bind(source_id).bind(target_id).bind(branch_id).bind(membership_id)
        .bind(merged_source.revision).bind(target.revision).bind(&input.idempotency_key).bind(request_hash)
        .execute(&mut *tx).await.map_err(|error| if error.as_database_error().is_some_and(|database| database.is_unique_violation()) { CustomerError::Conflict } else { CustomerError::Database })?;
    sqlx::query("INSERT INTO audit_events (tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES ($1::uuid,$2::uuid,'customer_merged','customer',$3,$4)")
        .bind(tenant_id).bind(membership_id).bind(source_id)
        .bind(serde_json::json!({"branchId":branch_id,"sourceCustomerId":source_id.to_string(),"targetCustomerId":target_id.to_string(),"sourceRevision":merged_source.revision,"targetRevision":target.revision}))
        .execute(&mut *tx).await.map_err(|_| CustomerError::Database)?;
    let (merged_at,) = sqlx::query_as::<_, (String,)>("SELECT merged_at::text FROM hosted_customer_merges WHERE tenant_id=$1::uuid AND source_customer_id=$2")
        .bind(tenant_id).bind(source_id).fetch_one(&mut *tx).await.map_err(|_| CustomerError::Database)?;
    tx.commit().await.map_err(|_| CustomerError::Database)?;
    Ok(CustomerMergeResult {
        source: merged_source,
        target,
        merged_at,
        replayed: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn customer_input_trims_optional_values_and_rejects_bad_lengths() {
        let customer = normalize(CustomerInput {
            name: "  Ama Mensah ".into(),
            phone: Some(" +233 20 123 4567 ".into()),
            email: Some(" ama@example.test ".into()),
            notes: Some("  walk-in  ".into()),
        })
        .unwrap();
        assert_eq!(customer.name, "Ama Mensah");
        assert_eq!(customer.phone.as_deref(), Some("+233 20 123 4567"));
        assert_eq!(customer.notes.as_deref(), Some("walk-in"));
        assert!(normalize(CustomerInput {
            name: "  ".into(),
            phone: None,
            email: None,
            notes: None
        })
        .is_err());
        assert!(normalize(CustomerInput {
            name: "A".into(),
            phone: None,
            email: Some("not-email".into()),
            notes: None
        })
        .is_err());
        assert!(normalize(CustomerInput {
            name: "A".into(),
            phone: None,
            email: None,
            notes: Some("x".repeat(1001))
        })
        .is_err());
    }

    #[test]
    fn customer_list_query_is_bounded_and_normalizes_search() {
        let (query, offset, page_size, include_archived) = CustomerListQuery {
            query: Some("  Ama  ".into()),
            offset: Some(100),
            page_size: Some(50),
            include_archived: None,
        }
        .validated()
        .unwrap();
        assert_eq!(query.as_deref(), Some("Ama"));
        assert_eq!((offset, page_size, include_archived), (100, 50, false));
        assert!(CustomerListQuery {
            query: None,
            offset: Some(-1),
            page_size: None,
            include_archived: None,
        }
        .validated()
        .is_err());
        assert!(CustomerListQuery {
            query: None,
            offset: None,
            page_size: Some(101),
            include_archived: None,
        }
        .validated()
        .is_err());
        assert!(CustomerListQuery {
            query: Some("x".repeat(101)),
            offset: None,
            page_size: None,
            include_archived: None,
        }
        .validated()
        .is_err());
    }
}
