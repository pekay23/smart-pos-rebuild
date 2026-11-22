// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SupplierError {
    InvalidInput,
    Conflict,
    Database,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SupplierInput {
    pub name: String,
    pub contact_person: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostedSupplier {
    pub id: String,
    pub name: String,
    pub contact_person: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    pub status: String,
    pub revision: i32,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SupplierListQuery {
    pub query: Option<String>,
    pub offset: Option<i64>,
    pub page_size: Option<i64>,
    pub include_archived: Option<bool>,
}

impl SupplierListQuery {
    pub(crate) fn validated(self) -> Result<(Option<String>, i64, i64, bool), SupplierError> {
        let query = self
            .query
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        if query
            .as_ref()
            .is_some_and(|value| value.chars().count() > 100)
        {
            return Err(SupplierError::InvalidInput);
        }
        let offset = self.offset.unwrap_or(0);
        let page_size = self.page_size.unwrap_or(100);
        if !(0..=100_000).contains(&offset) || !(1..=100).contains(&page_size) {
            return Err(SupplierError::InvalidInput);
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
pub(crate) struct SupplierUpdateInput {
    pub name: String,
    pub contact_person: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    pub expected_revision: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SupplierStatusInput {
    pub status: String,
    pub expected_revision: i32,
}

fn normalize(input: SupplierInput) -> Result<SupplierInput, SupplierError> {
    let trim = |value: Option<String>| value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    let input = SupplierInput {
        name: input.name.trim().to_owned(),
        contact_person: trim(input.contact_person),
        email: trim(input.email),
        phone: trim(input.phone),
        address: trim(input.address),
        notes: trim(input.notes),
    };
    if input.name.is_empty()
        || input.name.chars().count() > 160
        || input
            .contact_person
            .as_ref()
            .is_some_and(|v| v.chars().count() > 160)
        || input
            .email
            .as_ref()
            .is_some_and(|v| v.chars().count() > 254 || !v.contains('@'))
        || input.phone.as_ref().is_some_and(|v| v.chars().count() > 40)
        || input
            .address
            .as_ref()
            .is_some_and(|v| v.chars().count() > 500)
        || input
            .notes
            .as_ref()
            .is_some_and(|v| v.chars().count() > 1000)
    {
        return Err(SupplierError::InvalidInput);
    }
    Ok(input)
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
) -> Result<(), SupplierError> {
    for (key, value) in [
        ("pos.identity_id", identity_id),
        ("pos.tenant_id", tenant_id),
        ("pos.membership_id", membership_id),
        ("pos.branch_id", branch_id),
        ("pos.station_id", ""),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut **tx)
            .await
            .map_err(|_| SupplierError::Database)?;
    }
    Ok(())
}

const SELECT_COLUMNS: &str =
    "id::text,name,contact_person,email,phone,address,notes,status,revision,created_at::text";

pub(crate) async fn list(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    query: SupplierListQuery,
) -> Result<Vec<HostedSupplier>, SupplierError> {
    let (query, offset, page_size, include_archived) = query.validated()?;
    let pattern = query.map(|value| {
        format!(
            "%{}%",
            value
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        )
    });
    let mut tx = pool.begin().await.map_err(|_| SupplierError::Database)?;
    set_context(&mut tx, identity_id, tenant_id, membership_id, branch_id).await?;
    let sql = format!("SELECT {SELECT_COLUMNS} FROM hosted_suppliers WHERE tenant_id=$1::uuid AND (status='active' OR ($5 AND status='archived')) AND ($2::text IS NULL OR name ILIKE $2 ESCAPE E'\\\\' OR COALESCE(contact_person,'') ILIKE $2 ESCAPE E'\\\\' OR COALESCE(phone,'') ILIKE $2 ESCAPE E'\\\\' OR COALESCE(email,'') ILIKE $2 ESCAPE E'\\\\' OR COALESCE(address,'') ILIKE $2 ESCAPE E'\\\\') ORDER BY lower(name),id LIMIT $3 OFFSET $4");
    let rows = sqlx::query_as::<_, HostedSupplier>(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(tenant_id)
        .bind(pattern)
        .bind(page_size)
        .bind(offset)
        .bind(include_archived)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| SupplierError::Database)?;
    tx.commit().await.map_err(|_| SupplierError::Database)?;
    Ok(rows)
}

pub(crate) async fn create(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    idempotency_key: &str,
    raw_body: &[u8],
    input: SupplierInput,
) -> Result<HostedSupplier, SupplierError> {
    let input = normalize(input)?;
    if idempotency_key.is_empty()
        || idempotency_key.len() > 128
        || !idempotency_key.bytes().all(|b| (0x21..=0x7e).contains(&b))
    {
        return Err(SupplierError::InvalidInput);
    }
    let hash = Sha256::digest(raw_body).to_vec();
    let mut tx = pool.begin().await.map_err(|_| SupplierError::Database)?;
    set_context(&mut tx, identity_id, tenant_id, membership_id, branch_id).await?;
    let lock_key = format!("{tenant_id}:{membership_id}:{idempotency_key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(|_| SupplierError::Database)?;
    if let Some((id, old_hash)) = sqlx::query_as::<_, (String, Vec<u8>)>("SELECT id::text,creation_request_hash FROM hosted_suppliers WHERE tenant_id=$1::uuid AND created_by_membership_id=$2::uuid AND creation_idempotency_key=$3").bind(tenant_id).bind(membership_id).bind(idempotency_key).fetch_optional(&mut *tx).await.map_err(|_| SupplierError::Database)? {
        if old_hash != hash { return Err(SupplierError::Conflict); }
        let sql = format!("SELECT {SELECT_COLUMNS} FROM hosted_suppliers WHERE tenant_id=$1::uuid AND id=$2::uuid");
        let row = sqlx::query_as::<_, HostedSupplier>(sqlx::AssertSqlSafe(sql.as_str())).bind(tenant_id).bind(id).fetch_one(&mut *tx).await.map_err(|_| SupplierError::Database)?;
        tx.commit().await.map_err(|_| SupplierError::Database)?;
        return Ok(row);
    }
    let sql = format!("INSERT INTO hosted_suppliers (id,tenant_id,name,contact_person,email,phone,address,notes,status,created_by_membership_id,creation_idempotency_key,creation_request_hash) VALUES ($1,$2::uuid,$3,$4,$5,$6,$7,$8,'active',$9::uuid,$10,$11) RETURNING {SELECT_COLUMNS}");
    let row = sqlx::query_as::<_, HostedSupplier>(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(Uuid::new_v4())
        .bind(tenant_id)
        .bind(&input.name)
        .bind(&input.contact_person)
        .bind(&input.email)
        .bind(&input.phone)
        .bind(&input.address)
        .bind(&input.notes)
        .bind(membership_id)
        .bind(idempotency_key)
        .bind(hash)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| SupplierError::Database)?;
    sqlx::query("INSERT INTO audit_events (tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES ($1::uuid,$2::uuid,'supplier_created','supplier',$3::uuid,$4)").bind(tenant_id).bind(membership_id).bind(&row.id).bind(serde_json::json!({"branchId":branch_id})).execute(&mut *tx).await.map_err(|_| SupplierError::Database)?;
    tx.commit().await.map_err(|_| SupplierError::Database)?;
    Ok(row)
}

pub(crate) async fn update(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    supplier_id: &str,
    input: SupplierUpdateInput,
) -> Result<HostedSupplier, SupplierError> {
    if input.expected_revision < 1 {
        return Err(SupplierError::InvalidInput);
    }
    let expected_revision = input.expected_revision;
    let input = normalize(SupplierInput {
        name: input.name,
        contact_person: input.contact_person,
        email: input.email,
        phone: input.phone,
        address: input.address,
        notes: input.notes,
    })?;
    let mut tx = pool.begin().await.map_err(|_| SupplierError::Database)?;
    set_context(&mut tx, identity_id, tenant_id, membership_id, branch_id).await?;
    let sql = format!("UPDATE hosted_suppliers SET name=$4,contact_person=$5,email=$6,phone=$7,address=$8,notes=$9,revision=revision+1,updated_at=now() WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active' AND revision=$3 RETURNING {SELECT_COLUMNS}");
    let row = sqlx::query_as::<_, HostedSupplier>(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(tenant_id)
        .bind(supplier_id)
        .bind(expected_revision)
        .bind(&input.name)
        .bind(&input.contact_person)
        .bind(&input.email)
        .bind(&input.phone)
        .bind(&input.address)
        .bind(&input.notes)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| SupplierError::Database)?
        .ok_or(SupplierError::Conflict)?;
    sqlx::query("INSERT INTO audit_events (tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES ($1::uuid,$2::uuid,'supplier_updated','supplier',$3::uuid,$4)").bind(tenant_id).bind(membership_id).bind(supplier_id).bind(serde_json::json!({"branchId":branch_id,"revision":expected_revision+1})).execute(&mut *tx).await.map_err(|_| SupplierError::Database)?;
    tx.commit().await.map_err(|_| SupplierError::Database)?;
    Ok(row)
}

pub(crate) async fn set_status(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    supplier_id: &str,
    input: SupplierStatusInput,
) -> Result<HostedSupplier, SupplierError> {
    if input.expected_revision < 1 || !matches!(input.status.as_str(), "active" | "archived") {
        return Err(SupplierError::InvalidInput);
    }
    let mut tx = pool.begin().await.map_err(|_| SupplierError::Database)?;
    set_context(&mut tx, identity_id, tenant_id, membership_id, branch_id).await?;
    let sql = format!("UPDATE hosted_suppliers SET status=$4,revision=revision+1,updated_at=now() WHERE tenant_id=$1::uuid AND id=$2::uuid AND revision=$3 AND status<>$4 RETURNING {SELECT_COLUMNS}");
    let row = sqlx::query_as::<_, HostedSupplier>(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(tenant_id)
        .bind(supplier_id)
        .bind(input.expected_revision)
        .bind(&input.status)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| SupplierError::Database)?
        .ok_or(SupplierError::Conflict)?;
    let action = if input.status == "archived" {
        "supplier_archived"
    } else {
        "supplier_restored"
    };
    sqlx::query("INSERT INTO audit_events (tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES ($1::uuid,$2::uuid,$3,'supplier',$4::uuid,$5)").bind(tenant_id).bind(membership_id).bind(action).bind(supplier_id).bind(serde_json::json!({"branchId":branch_id,"revision":input.expected_revision+1})).execute(&mut *tx).await.map_err(|_| SupplierError::Database)?;
    tx.commit().await.map_err(|_| SupplierError::Database)?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supplier_inputs_and_page_bounds_are_validated() {
        let row = normalize(SupplierInput {
            name: " North Mill ".into(),
            contact_person: Some(" Kai ".into()),
            email: Some(" kai@example.test ".into()),
            phone: None,
            address: None,
            notes: Some("  ".into()),
        })
        .unwrap();
        assert_eq!(row.name, "North Mill");
        assert_eq!(row.contact_person.as_deref(), Some("Kai"));
        assert_eq!(row.notes, None);
        assert!(normalize(SupplierInput {
            name: " ".into(),
            contact_person: None,
            email: Some("nope".into()),
            phone: None,
            address: None,
            notes: None
        })
        .is_err());
        assert!(normalize(SupplierInput {
            name: "N".into(),
            contact_person: None,
            email: None,
            phone: None,
            address: Some("x".repeat(501)),
            notes: None
        })
        .is_err());
        assert!(SupplierListQuery {
            page_size: Some(101),
            ..Default::default()
        }
        .validated()
        .is_err());
        assert_eq!(
            SupplierListQuery {
                query: Some("  mill  ".into()),
                ..Default::default()
            }
            .validated()
            .unwrap()
            .0
            .as_deref(),
            Some("mill")
        );
        assert!(SupplierListQuery {
            query: Some("x".repeat(101)),
            ..Default::default()
        }
        .validated()
        .is_err());
    }
}
