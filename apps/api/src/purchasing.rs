// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PurchaseError {
    InvalidInput,
    NotFound,
    Conflict,
    IdempotencyConflict,
    Database,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PurchaseOrderLineInput {
    pub product_id: String,
    pub quantity: i64,
    pub expected_unit_cost_minor: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PurchaseOrderInput {
    pub supplier_id: Option<String>,
    pub reference: Option<String>,
    pub expected_revision: Option<i64>,
    pub items: Vec<PurchaseOrderLineInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TransitionInput {
    pub expected_revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReceiptLineInput {
    pub order_line_id: String,
    pub quantity: i64,
    pub actual_unit_cost_minor: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReceiptInput {
    pub expected_revision: i64,
    pub reference: Option<String>,
    pub items: Vec<ReceiptLineInput>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PurchaseOrderLine {
    pub id: String,
    pub product_id: String,
    pub sku: String,
    pub name: String,
    pub ordered_quantity: i64,
    pub received_quantity: i64,
    pub expected_unit_cost_minor: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostedPurchaseOrder {
    pub id: String,
    pub order_number: String,
    pub supplier_id: Option<String>,
    pub supplier_name: Option<String>,
    pub reference: Option<String>,
    pub status: String,
    pub revision: i64,
    pub created_at: String,
    pub placed_at: Option<String>,
    pub items: Vec<PurchaseOrderLine>,
    pub receipts: Vec<PurchaseReceipt>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PurchaseReceipt {
    pub id: String,
    pub reference: Option<String>,
    pub total_minor: i64,
    pub received_at: String,
}

fn normalize_optional(value: Option<String>, max: usize) -> Result<Option<String>, PurchaseError> {
    let value = value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    if value.as_ref().is_some_and(|v| v.chars().count() > max) {
        return Err(PurchaseError::InvalidInput);
    }
    Ok(value)
}

fn validate_items(
    items: Vec<PurchaseOrderLineInput>,
) -> Result<Vec<PurchaseOrderLineInput>, PurchaseError> {
    if items.is_empty() || items.len() > 100 {
        return Err(PurchaseError::InvalidInput);
    }
    let mut seen = std::collections::HashSet::with_capacity(items.len());
    for item in &items {
        if Uuid::parse_str(&item.product_id).is_err()
            || !seen.insert(item.product_id.as_str())
            || !(1..=1_000_000).contains(&item.quantity)
            || item.expected_unit_cost_minor < 0
            || item
                .expected_unit_cost_minor
                .checked_mul(item.quantity)
                .is_none()
        {
            return Err(PurchaseError::InvalidInput);
        }
    }
    Ok(items)
}

async fn context(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
) -> Result<(), PurchaseError> {
    for (key, value) in [
        ("pos.identity_id", identity),
        ("pos.tenant_id", tenant),
        ("pos.membership_id", membership),
        ("pos.branch_id", branch),
        ("pos.station_id", ""),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut **tx)
            .await
            .map_err(|_| PurchaseError::Database)?;
    }
    Ok(())
}

async fn load(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    id: &str,
) -> Result<HostedPurchaseOrder, PurchaseError> {
    let header=sqlx::query_as::<_,(String,Option<String>,Option<String>,Option<String>,String,i64,String,Option<String>)>(
        "SELECT o.order_number,o.supplier_id::text,s.name,o.reference,o.status,o.revision,o.created_at::text,o.placed_at::text FROM hosted_purchase_orders o LEFT JOIN hosted_suppliers s ON s.tenant_id=o.tenant_id AND s.id=o.supplier_id WHERE o.tenant_id=$1::uuid AND o.branch_id=$2::uuid AND o.id=$3::uuid"
    ).bind(tenant).bind(branch).bind(id).fetch_optional(&mut **tx).await.map_err(|_|PurchaseError::Database)?.ok_or(PurchaseError::NotFound)?;
    let items=sqlx::query_as::<_,PurchaseOrderLine>("SELECT id::text,product_id::text,sku_snapshot AS sku,name_snapshot AS name,ordered_quantity,received_quantity,expected_unit_cost_minor FROM hosted_purchase_order_lines WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND order_id=$3::uuid ORDER BY id")
        .bind(tenant).bind(branch).bind(id).fetch_all(&mut **tx).await.map_err(|_|PurchaseError::Database)?;
    let receipts=sqlx::query_as::<_,PurchaseReceipt>("SELECT id::text,reference,total_minor,received_at::text FROM hosted_purchase_receipts WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND order_id=$3::uuid ORDER BY received_at,id")
        .bind(tenant).bind(branch).bind(id).fetch_all(&mut **tx).await.map_err(|_|PurchaseError::Database)?;
    Ok(HostedPurchaseOrder {
        id: id.to_owned(),
        order_number: header.0,
        supplier_id: header.1,
        supplier_name: header.2,
        reference: header.3,
        status: header.4,
        revision: header.5,
        created_at: header.6,
        placed_at: header.7,
        items,
        receipts,
    })
}

pub(crate) async fn list(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
) -> Result<Vec<HostedPurchaseOrder>, PurchaseError> {
    let mut tx = pool.begin().await.map_err(|_| PurchaseError::Database)?;
    context(&mut tx, identity, tenant, membership, branch).await?;
    let ids=sqlx::query_scalar::<_,String>("SELECT id::text FROM hosted_purchase_orders WHERE tenant_id=$1::uuid AND branch_id=$2::uuid ORDER BY created_at DESC,id DESC LIMIT 100")
        .bind(tenant).bind(branch).fetch_all(&mut *tx).await.map_err(|_|PurchaseError::Database)?;
    let mut result = Vec::with_capacity(ids.len());
    for id in ids {
        result.push(load(&mut tx, tenant, branch, &id).await?)
    }
    tx.commit().await.map_err(|_| PurchaseError::Database)?;
    Ok(result)
}

pub(crate) async fn save(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    key: &str,
    raw: &[u8],
    input: PurchaseOrderInput,
) -> Result<HostedPurchaseOrder, PurchaseError> {
    let line_count = input.items.len();
    let supplier = normalize_optional(input.supplier_id, 36)?;
    if supplier
        .as_ref()
        .is_some_and(|id| Uuid::parse_str(id).is_err())
    {
        return Err(PurchaseError::InvalidInput);
    }
    let reference = normalize_optional(input.reference, 100)?;
    let items = validate_items(input.items)?;
    if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(PurchaseError::InvalidInput);
    }
    let hash = Sha256::digest(raw).to_vec();
    let mut tx = pool.begin().await.map_err(|_| PurchaseError::Database)?;
    context(&mut tx, identity, tenant, membership, branch).await?;
    let lock = format!("{tenant}:{branch}:{membership}:{key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock)
        .execute(&mut *tx)
        .await
        .map_err(|_| PurchaseError::Database)?;
    if let Some((id,old_hash))=sqlx::query_as::<_,(String,Vec<u8>)>("SELECT id::text,creation_request_hash FROM hosted_purchase_orders WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND created_by_membership_id=$3::uuid AND creation_idempotency_key=$4")
        .bind(tenant).bind(branch).bind(membership).bind(key).fetch_optional(&mut *tx).await.map_err(|_|PurchaseError::Database)?{
        if old_hash!=hash{return Err(PurchaseError::IdempotencyConflict)}
        let result=load(&mut tx,tenant,branch,&id).await?;tx.commit().await.map_err(|_|PurchaseError::Database)?;return Ok(result)
    }
    if let Some(supplier_id) = supplier.as_deref() {
        let valid=sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM hosted_suppliers WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active')").bind(tenant).bind(supplier_id).fetch_one(&mut *tx).await.map_err(|_|PurchaseError::Database)?;
        if !valid {
            return Err(PurchaseError::NotFound);
        }
    }
    let id = Uuid::new_v4();
    let number = format!("PO-{}", &id.simple().to_string()[..12].to_uppercase());
    sqlx::query("INSERT INTO hosted_purchase_orders(id,tenant_id,branch_id,supplier_id,order_number,reference,status,revision,created_by_membership_id,creation_idempotency_key,creation_request_hash) VALUES($1,$2::uuid,$3::uuid,$4::uuid,$5,$6,'draft',1,$7::uuid,$8,$9)")
        .bind(id).bind(tenant).bind(branch).bind(&supplier).bind(&number).bind(&reference).bind(membership).bind(key).bind(&hash).execute(&mut *tx).await.map_err(|e|if e.as_database_error().is_some_and(|db|db.is_unique_violation()){PurchaseError::Conflict}else{PurchaseError::Database})?;
    insert_lines(&mut tx, tenant, branch, &id.to_string(), items).await?;
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,'purchase_order_created','purchase_order',$3::uuid,$4)").bind(tenant).bind(membership).bind(id).bind(serde_json::json!({"branchId":branch,"lineCount":line_count})).execute(&mut *tx).await.map_err(|_|PurchaseError::Database)?;
    let result = load(&mut tx, tenant, branch, &id.to_string()).await?;
    tx.commit().await.map_err(|_| PurchaseError::Database)?;
    Ok(result)
}

async fn insert_lines(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    order_id: &str,
    items: Vec<PurchaseOrderLineInput>,
) -> Result<(), PurchaseError> {
    for line in items {
        let product=sqlx::query_as::<_,(String,String)>("SELECT p.sku,p.name FROM catalog_products p JOIN branch_catalog_items i ON i.tenant_id=p.tenant_id AND i.product_id=p.id WHERE p.tenant_id=$1::uuid AND p.id=$2::uuid AND p.status='active' AND i.branch_id=$3::uuid AND i.status='active'").bind(tenant).bind(&line.product_id).bind(branch).fetch_optional(&mut **tx).await.map_err(|_|PurchaseError::Database)?.ok_or(PurchaseError::NotFound)?;
        sqlx::query("INSERT INTO hosted_purchase_order_lines(tenant_id,branch_id,order_id,product_id,sku_snapshot,name_snapshot,ordered_quantity,expected_unit_cost_minor) VALUES($1::uuid,$2::uuid,$3::uuid,$4::uuid,$5,$6,$7,$8)").bind(tenant).bind(branch).bind(order_id).bind(&line.product_id).bind(product.0).bind(product.1).bind(line.quantity).bind(line.expected_unit_cost_minor).execute(&mut **tx).await.map_err(|e|if e.as_database_error().is_some_and(|db|db.is_unique_violation()){PurchaseError::InvalidInput}else{PurchaseError::Database})?;
    }
    Ok(())
}

pub(crate) async fn update_draft(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    id: &str,
    input: PurchaseOrderInput,
) -> Result<HostedPurchaseOrder, PurchaseError> {
    let revision = input.expected_revision.ok_or(PurchaseError::InvalidInput)?;
    if revision < 1 {
        return Err(PurchaseError::InvalidInput);
    }
    let supplier = normalize_optional(input.supplier_id, 36)?;
    if supplier
        .as_ref()
        .is_some_and(|s| Uuid::parse_str(s).is_err())
    {
        return Err(PurchaseError::InvalidInput);
    }
    let reference = normalize_optional(input.reference, 100)?;
    let items = validate_items(input.items)?;
    let mut tx = pool.begin().await.map_err(|_| PurchaseError::Database)?;
    context(&mut tx, identity, tenant, membership, branch).await?;
    if let Some(supplier_id) = supplier.as_deref() {
        let active = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM hosted_suppliers WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active')",
        )
        .bind(tenant)
        .bind(supplier_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| PurchaseError::Database)?;
        if !active {
            return Err(PurchaseError::NotFound);
        }
    }
    let changed=sqlx::query("UPDATE hosted_purchase_orders SET supplier_id=$4::uuid,reference=$5,revision=revision+1,updated_at=now() WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3::uuid AND status='draft' AND revision=$6")
        .bind(tenant).bind(branch).bind(id).bind(&supplier).bind(&reference).bind(revision).execute(&mut *tx).await.map_err(|_|PurchaseError::Database)?;
    if changed.rows_affected() != 1 {
        return Err(PurchaseError::Conflict);
    }
    sqlx::query("DELETE FROM hosted_purchase_order_lines WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND order_id=$3::uuid").bind(tenant).bind(branch).bind(id).execute(&mut *tx).await.map_err(|_|PurchaseError::Database)?;
    insert_lines(&mut tx, tenant, branch, id, items).await?;
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,'purchase_order_updated','purchase_order',$3::uuid,$4)").bind(tenant).bind(membership).bind(id).bind(serde_json::json!({"branchId":branch,"revision":revision+1})).execute(&mut *tx).await.map_err(|_|PurchaseError::Database)?;
    let result = load(&mut tx, tenant, branch, id).await?;
    tx.commit().await.map_err(|_| PurchaseError::Database)?;
    Ok(result)
}

pub(crate) async fn transition(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    id: &str,
    action: &str,
    input: TransitionInput,
) -> Result<HostedPurchaseOrder, PurchaseError> {
    if input.expected_revision < 1 || !matches!(action, "place" | "cancel") {
        return Err(PurchaseError::InvalidInput);
    }
    let mut tx = pool.begin().await.map_err(|_| PurchaseError::Database)?;
    context(&mut tx, identity, tenant, membership, branch).await?;
    let status=sqlx::query_scalar::<_,String>("SELECT status FROM hosted_purchase_orders WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3::uuid AND revision=$4 FOR UPDATE").bind(tenant).bind(branch).bind(id).bind(input.expected_revision).fetch_optional(&mut *tx).await.map_err(|_|PurchaseError::Database)?.ok_or(PurchaseError::Conflict)?;
    match action {
        "place" if status == "draft" => {
            let count=sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM hosted_purchase_order_lines WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND order_id=$3::uuid").bind(tenant).bind(branch).bind(id).fetch_one(&mut *tx).await.map_err(|_|PurchaseError::Database)?;
            if count == 0 {
                return Err(PurchaseError::InvalidInput);
            }
            sqlx::query("UPDATE hosted_purchase_orders SET status='placed',placed_at=now(),revision=revision+1,updated_at=now() WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3::uuid").bind(tenant).bind(branch).bind(id).execute(&mut *tx).await.map_err(|_|PurchaseError::Database)?;
        }
        "cancel" if matches!(status.as_str(), "draft" | "placed" | "partially_received") => {
            sqlx::query("UPDATE hosted_purchase_orders SET status='cancelled',cancelled_at=now(),revision=revision+1,updated_at=now() WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3::uuid").bind(tenant).bind(branch).bind(id).execute(&mut *tx).await.map_err(|_|PurchaseError::Database)?;
        }
        _ => return Err(PurchaseError::Conflict),
    }
    let next = if action == "place" {
        "placed"
    } else {
        "cancelled"
    };
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,$3,'purchase_order',$4::uuid,$5)").bind(tenant).bind(membership).bind(if action=="place"{"purchase_order_placed"}else{"purchase_order_cancelled"}).bind(id).bind(serde_json::json!({"branchId":branch,"from":status,"to":next,"revision":input.expected_revision+1})).execute(&mut *tx).await.map_err(|_|PurchaseError::Database)?;
    let result = load(&mut tx, tenant, branch, id).await?;
    tx.commit().await.map_err(|_| PurchaseError::Database)?;
    Ok(result)
}

pub(crate) async fn receive(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    id: &str,
    key: &str,
    raw: &[u8],
    input: ReceiptInput,
) -> Result<HostedPurchaseOrder, PurchaseError> {
    if key.is_empty()
        || key.len() > 128
        || !key.bytes().all(|b| (0x21..=0x7e).contains(&b))
        || input.expected_revision < 1
        || input.items.is_empty()
        || input.items.len() > 100
    {
        return Err(PurchaseError::InvalidInput);
    }
    let reference = normalize_optional(input.reference, 100)?;
    let mut seen = std::collections::HashSet::new();
    let mut total = 0_i64;
    for line in &input.items {
        if Uuid::parse_str(&line.order_line_id).is_err()
            || !seen.insert(line.order_line_id.as_str())
            || !(1..=1_000_000).contains(&line.quantity)
            || line.actual_unit_cost_minor < 0
        {
            return Err(PurchaseError::InvalidInput);
        }
        total = total
            .checked_add(
                line.quantity
                    .checked_mul(line.actual_unit_cost_minor)
                    .ok_or(PurchaseError::InvalidInput)?,
            )
            .ok_or(PurchaseError::InvalidInput)?;
    }
    let mut receipt_hash = Sha256::new();
    receipt_hash.update(raw);
    receipt_hash.update(b"\norder:");
    receipt_hash.update(id.as_bytes());
    let hash = receipt_hash.finalize().to_vec();
    let mut tx = pool.begin().await.map_err(|error| {
        #[cfg(test)]
        eprintln!("purchase receipt SQL failed: {error}");
        #[cfg(not(test))]
        let _ = error;
        PurchaseError::Database
    })?;
    context(&mut tx, identity, tenant, membership, branch).await?;
    let lock = format!("receipt:{tenant}:{branch}:{membership}:{key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock)
        .execute(&mut *tx)
        .await
        .map_err(|error| {
            #[cfg(test)]
            eprintln!("purchase receipt SQL failed: {error}");
            #[cfg(not(test))]
            let _ = error;
            PurchaseError::Database
        })?;
    if let Some(old_hash)=sqlx::query_scalar::<_,Vec<u8>>("SELECT request_hash FROM hosted_purchase_receipts WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND actor_membership_id=$3::uuid AND idempotency_key=$4").bind(tenant).bind(branch).bind(membership).bind(key).fetch_optional(&mut *tx).await.map_err(|error| { #[cfg(test)] eprintln!("purchase receipt SQL failed: {error}"); #[cfg(not(test))] let _ = error; PurchaseError::Database })?{if old_hash!=hash{return Err(PurchaseError::IdempotencyConflict)} let result=load(&mut tx,tenant,branch,id).await?;tx.commit().await.map_err(|error| { #[cfg(test)] eprintln!("purchase receipt SQL failed: {error}"); #[cfg(not(test))] let _ = error; PurchaseError::Database })?;return Ok(result)}
    let status=sqlx::query_scalar::<_,String>("SELECT status FROM hosted_purchase_orders WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3::uuid AND revision=$4 FOR UPDATE").bind(tenant).bind(branch).bind(id).bind(input.expected_revision).fetch_optional(&mut *tx).await.map_err(|error| { #[cfg(test)] eprintln!("purchase receipt SQL failed: {error}"); #[cfg(not(test))] let _ = error; PurchaseError::Database })?.ok_or(PurchaseError::Conflict)?;
    if !matches!(status.as_str(), "placed" | "partially_received") {
        return Err(PurchaseError::Conflict);
    }
    let order=sqlx::query_as::<_,(Option<String>,)>("SELECT supplier_id::text FROM hosted_purchase_orders WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3::uuid").bind(tenant).bind(branch).bind(id).fetch_one(&mut *tx).await.map_err(|error| { #[cfg(test)] eprintln!("purchase receipt SQL failed: {error}"); #[cfg(not(test))] let _ = error; PurchaseError::Database })?;
    let receipt = Uuid::new_v4();
    sqlx::query("INSERT INTO hosted_purchase_receipts(id,tenant_id,branch_id,order_id,supplier_id,actor_membership_id,idempotency_key,request_hash,reference,total_minor) VALUES($1,$2::uuid,$3::uuid,$4::uuid,$5::uuid,$6::uuid,$7,$8,$9,$10)").bind(receipt).bind(tenant).bind(branch).bind(id).bind(order.0).bind(membership).bind(key).bind(&hash).bind(&reference).bind(total).execute(&mut *tx).await.map_err(|error| { #[cfg(test)] eprintln!("purchase receipt SQL failed: {error}"); #[cfg(not(test))] let _ = error; PurchaseError::Database })?;
    for line in input.items {
        let product=sqlx::query_as::<_,(String,String,i64)>("SELECT product_id::text,name_snapshot,ordered_quantity-received_quantity FROM hosted_purchase_order_lines WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND order_id=$3::uuid AND id=$4::uuid FOR UPDATE").bind(tenant).bind(branch).bind(id).bind(&line.order_line_id).fetch_optional(&mut *tx).await.map_err(|error| { #[cfg(test)] eprintln!("purchase receipt SQL failed: {error}"); #[cfg(not(test))] let _ = error; PurchaseError::Database })?.ok_or(PurchaseError::NotFound)?;
        if line.quantity > product.2 {
            return Err(PurchaseError::InvalidInput);
        }
        let item_total = line.quantity * line.actual_unit_cost_minor;
        let resulting=sqlx::query_scalar::<_,i64>("UPDATE branch_catalog_items SET stock_quantity=stock_quantity+$4,cost_minor=$5,revision=revision+1,updated_at=now() WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid AND status='active' AND stock_quantity<=9223372036854775807-$4 RETURNING stock_quantity").bind(tenant).bind(branch).bind(&product.0).bind(line.quantity).bind(line.actual_unit_cost_minor).fetch_optional(&mut *tx).await.map_err(|error| { #[cfg(test)] eprintln!("purchase receipt SQL failed: {error}"); #[cfg(not(test))] let _ = error; PurchaseError::Database })?.ok_or(PurchaseError::Conflict)?;
        sqlx::query("UPDATE hosted_purchase_order_lines SET received_quantity=received_quantity+$4 WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND order_id=$3::uuid AND id=$5::uuid").bind(tenant).bind(branch).bind(id).bind(line.quantity).bind(&line.order_line_id).execute(&mut *tx).await.map_err(|error| { #[cfg(test)] eprintln!("purchase receipt SQL failed: {error}"); #[cfg(not(test))] let _ = error; PurchaseError::Database })?;
        sqlx::query("INSERT INTO hosted_purchase_receipt_lines(tenant_id,branch_id,receipt_id,order_line_id,product_id,quantity,actual_unit_cost_minor,line_total_minor) VALUES($1::uuid,$2::uuid,$3,$4::uuid,$5::uuid,$6,$7,$8)").bind(tenant).bind(branch).bind(receipt).bind(&line.order_line_id).bind(&product.0).bind(line.quantity).bind(line.actual_unit_cost_minor).bind(item_total).execute(&mut *tx).await.map_err(|error| { #[cfg(test)] eprintln!("purchase receipt SQL failed: {error}"); #[cfg(not(test))] let _ = error; PurchaseError::Database })?;
        let movement_key = format!(
            "purchase:{}:{}",
            receipt.simple(),
            &product.0[..8.min(product.0.len())]
        );
        sqlx::query("INSERT INTO stock_movements(tenant_id,branch_id,product_id,actor_membership_id,movement_kind,quantity_delta,note,source_id,idempotency_key,resulting_stock_quantity) VALUES($1::uuid,$2::uuid,$3::uuid,$4::uuid,'purchase_receipt',$5,$6,$7::uuid,$8,$9)").bind(tenant).bind(branch).bind(&product.0).bind(membership).bind(line.quantity).bind(format!("Purchase order receipt {}",id)).bind(receipt).bind(movement_key).bind(resulting).execute(&mut *tx).await.map_err(|error| { #[cfg(test)] eprintln!("purchase receipt SQL failed: {error}"); #[cfg(not(test))] let _ = error; PurchaseError::Database })?;
    }
    let remaining=sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM hosted_purchase_order_lines WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND order_id=$3::uuid AND received_quantity<ordered_quantity").bind(tenant).bind(branch).bind(id).fetch_one(&mut *tx).await.map_err(|error| { #[cfg(test)] eprintln!("purchase receipt SQL failed: {error}"); #[cfg(not(test))] let _ = error; PurchaseError::Database })?;
    let next = if remaining == 0 {
        "received"
    } else {
        "partially_received"
    };
    sqlx::query("UPDATE hosted_purchase_orders SET status=$4,revision=revision+1,updated_at=now() WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3::uuid").bind(tenant).bind(branch).bind(id).bind(next).execute(&mut *tx).await.map_err(|error| { #[cfg(test)] eprintln!("purchase receipt SQL failed: {error}"); #[cfg(not(test))] let _ = error; PurchaseError::Database })?;
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,'purchase_order_received','purchase_order',$3::uuid,$4)").bind(tenant).bind(membership).bind(id).bind(serde_json::json!({"branchId":branch,"receiptId":receipt.to_string(),"status":next,"totalMinor":total,"reference":reference})).execute(&mut *tx).await.map_err(|error| { #[cfg(test)] eprintln!("purchase receipt SQL failed: {error}"); #[cfg(not(test))] let _ = error; PurchaseError::Database })?;
    let result = load(&mut tx, tenant, branch, id).await?;
    tx.commit().await.map_err(|error| {
        #[cfg(test)]
        eprintln!("purchase receipt SQL failed: {error}");
        #[cfg(not(test))]
        let _ = error;
        PurchaseError::Database
    })?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn order_lines_reject_duplicates_bounds_and_overflow() {
        let product = "20000000-0000-0000-0000-000000000002";
        assert!(validate_items(vec![
            PurchaseOrderLineInput {
                product_id: product.into(),
                quantity: 1,
                expected_unit_cost_minor: 0
            },
            PurchaseOrderLineInput {
                product_id: product.into(),
                quantity: 1,
                expected_unit_cost_minor: 0
            }
        ])
        .is_err());
        assert!(validate_items(vec![PurchaseOrderLineInput {
            product_id: product.into(),
            quantity: 1_000_001,
            expected_unit_cost_minor: 0
        }])
        .is_err());
        assert!(validate_items(vec![PurchaseOrderLineInput {
            product_id: product.into(),
            quantity: i64::MAX,
            expected_unit_cost_minor: 2
        }])
        .is_err());
        assert!(validate_items(vec![PurchaseOrderLineInput {
            product_id: product.into(),
            quantity: 2,
            expected_unit_cost_minor: 10
        }])
        .is_ok());
    }
}
