// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use crate::inventory_allocations;
use serde::{Deserialize, Serialize};
use sqlx::{types::Json, PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CatalogError {
    InvalidInput,
    Conflict,
    NotFound,
    InsufficientStock,
    Database,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateProductInput {
    pub sku: String,
    pub barcode: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub price_minor: i64,
    pub cost_minor: Option<i64>,
    pub tax_rate_bps: i32,
    pub opening_stock: i64,
    pub reorder_point: i64,
    pub target_stock_quantity: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AttachProductInput {
    pub price_minor: i64,
    pub cost_minor: Option<i64>,
    pub tax_rate_bps: i32,
    pub opening_stock: i64,
    pub reorder_point: i64,
    pub target_stock_quantity: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProductIdentityInput {
    pub sku: String,
    pub barcode: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub expected_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CatalogProductIdentity {
    pub product_id: String,
    pub sku: String,
    pub barcode: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchProduct {
    pub product_id: String,
    pub identity_revision: i64,
    pub sku: String,
    pub barcode: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub price_minor: i64,
    pub tax_rate_bps: i32,
    pub stock_quantity: i64,
    pub reorder_point: i64,
    pub target_stock_quantity: i64,
    pub revision: i64,
    pub currency_code: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LowStockProduct {
    pub product_id: String,
    pub sku: String,
    pub name: String,
    pub stock_quantity: i64,
    pub reorder_point: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchLowStockSummary {
    pub count: i64,
    pub products: Vec<LowStockProduct>,
}

pub(crate) async fn branch_low_stock_summary(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
) -> Result<BranchLowStockSummary, CatalogError> {
    let mut transaction = pool.begin().await.map_err(|_| CatalogError::Database)?;
    set_request_context(
        &mut transaction,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        None,
    )
    .await?;
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint
         FROM branch_catalog_items i
         JOIN catalog_products p ON p.id=i.product_id AND p.tenant_id=i.tenant_id
         WHERE i.tenant_id=$1::uuid AND i.branch_id=$2::uuid
           AND i.status='active' AND p.status='active'
           AND i.stock_quantity <= i.reorder_point",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;
    let products = sqlx::query_as::<_, LowStockProduct>(
        "SELECT p.id::text AS product_id, p.sku, p.name,
                i.stock_quantity, i.reorder_point
         FROM branch_catalog_items i
         JOIN catalog_products p ON p.id=i.product_id AND p.tenant_id=i.tenant_id
         WHERE i.tenant_id=$1::uuid AND i.branch_id=$2::uuid
           AND i.status='active' AND p.status='active'
           AND i.stock_quantity <= i.reorder_point
         ORDER BY i.stock_quantity, lower(p.name), p.id
         LIMIT 6",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;
    transaction
        .commit()
        .await
        .map_err(|_| CatalogError::Database)?;
    Ok(BranchLowStockSummary { count, products })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StockAdjustmentInput {
    pub quantity_delta: i64,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BranchProductStatusInput {
    pub is_active: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BranchProductSettingsInput {
    pub price_minor: i64,
    pub tax_rate_bps: i32,
    pub reorder_point: i64,
    pub target_stock_quantity: i64,
    pub expected_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StockAdjustmentResult {
    pub movement_id: String,
    pub product_id: String,
    pub quantity_delta: i64,
    pub resulting_stock_quantity: i64,
    pub note: Option<String>,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProductBranchAvailability {
    pub branch_id: String,
    pub branch_code: String,
    pub branch_name: String,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub time_zone: String,
    pub opening_hours: Json<crate::branch_settings::OpeningHours>,
    pub stock_quantity: i64,
    pub pickup_reserved_quantity: i64,
    pub outbound_committed_quantity: i64,
    pub available_to_promise: i64,
    pub commitment_shortfall: i64,
    pub price_minor: i64,
    pub currency_code: String,
    pub observed_at: String,
}

pub(crate) async fn list_branch_products(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
) -> Result<Vec<BranchProduct>, CatalogError> {
    let mut transaction = pool.begin().await.map_err(|_| CatalogError::Database)?;
    set_request_context(
        &mut transaction,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    let products = sqlx::query_as::<_, BranchProduct>(
        "SELECT p.id::text AS product_id, p.revision AS identity_revision, p.sku, p.barcode, p.name, p.description, p.category,
                i.price_minor, i.tax_rate_bps, i.stock_quantity, i.reorder_point,
                i.target_stock_quantity, i.revision, b.currency_code::text AS currency_code,
                (p.status = 'active' AND i.status = 'active') AS is_active
         FROM branch_catalog_items i
         JOIN catalog_products p ON p.id = i.product_id AND p.tenant_id = i.tenant_id
         JOIN branches b ON b.id = i.branch_id AND b.tenant_id = i.tenant_id
         WHERE i.tenant_id = $1::uuid AND i.branch_id = $2::uuid
           AND ($3::boolean = false OR (p.status = 'active' AND i.status = 'active'))
         ORDER BY lower(p.name), p.id",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(station_id.is_some())
    .fetch_all(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;
    transaction
        .commit()
        .await
        .map_err(|_| CatalogError::Database)?;
    Ok(products)
}

pub(crate) async fn list_branch_catalog_candidates(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
) -> Result<Vec<CatalogProductIdentity>, CatalogError> {
    let mut transaction = pool.begin().await.map_err(|_| CatalogError::Database)?;
    set_request_context(
        &mut transaction,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        None,
    )
    .await?;
    let products = sqlx::query_as::<_, CatalogProductIdentity>(
        "SELECT p.id::text AS product_id, p.sku, p.barcode, p.name, p.description, p.category, p.revision
         FROM catalog_products p
         WHERE p.tenant_id = $1::uuid AND p.status = 'active'
           AND NOT EXISTS (
               SELECT 1 FROM branch_catalog_items i
               WHERE i.tenant_id = p.tenant_id AND i.branch_id = $2::uuid AND i.product_id = p.id
           )
         ORDER BY lower(p.name), p.id",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;
    transaction
        .commit()
        .await
        .map_err(|_| CatalogError::Database)?;
    Ok(products)
}

pub(crate) async fn attach_product_to_branch(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    product_id: &str,
    input: AttachProductInput,
) -> Result<BranchProduct, CatalogError> {
    let product_id = Uuid::parse_str(product_id).map_err(|_| CatalogError::InvalidInput)?;
    if input.price_minor < 0
        || input.cost_minor.is_some_and(|value| value < 0)
        || !(0..=10_000).contains(&input.tax_rate_bps)
        || input.opening_stock < 0
        || input.reorder_point < 0
        || input.target_stock_quantity < 0
        || (input.target_stock_quantity > 0 && input.target_stock_quantity < input.reorder_point)
    {
        return Err(CatalogError::InvalidInput);
    }

    let mut transaction = pool.begin().await.map_err(|_| CatalogError::Database)?;
    set_request_context(
        &mut transaction,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        None,
    )
    .await?;
    let sku = sqlx::query_scalar::<_, String>(
        "SELECT sku FROM catalog_products WHERE tenant_id = $1::uuid AND id = $2 AND status = 'active' FOR SHARE",
    )
    .bind(tenant_id)
    .bind(product_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?
    .ok_or(CatalogError::NotFound)?;

    sqlx::query(
        "INSERT INTO branch_catalog_items
            (tenant_id, branch_id, product_id, price_minor, cost_minor, tax_rate_bps,
             stock_quantity, reorder_point, target_stock_quantity)
         VALUES ($1::uuid, $2::uuid, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(product_id)
    .bind(input.price_minor)
    .bind(input.cost_minor)
    .bind(input.tax_rate_bps)
    .bind(input.opening_stock)
    .bind(input.reorder_point)
    .bind(input.target_stock_quantity)
    .execute(&mut *transaction)
    .await
    .map_err(map_write_error)?;

    if input.opening_stock > 0 {
        sqlx::query(
            "INSERT INTO stock_movements
                (tenant_id, branch_id, product_id, actor_membership_id, movement_kind, quantity_delta, note)
             VALUES ($1::uuid, $2::uuid, $3, $4::uuid, 'opening_balance', $5,
                     'Opening stock recorded when existing product was added to branch')",
        )
        .bind(tenant_id)
        .bind(branch_id)
        .bind(product_id)
        .bind(membership_id)
        .bind(input.opening_stock)
        .execute(&mut *transaction)
        .await
        .map_err(|_| CatalogError::Database)?;
    }

    sqlx::query(
        "INSERT INTO audit_events (tenant_id, actor_membership_id, action, entity_type, entity_id, metadata)
         VALUES ($1::uuid, $2::uuid, 'catalog.product_attached', 'catalog_product', $3,
                 jsonb_build_object('branchId', $4, 'sku', $5, 'openingStock', $6))",
    )
    .bind(tenant_id)
    .bind(membership_id)
    .bind(product_id)
    .bind(branch_id)
    .bind(sku)
    .bind(input.opening_stock)
    .execute(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;

    let product = sqlx::query_as::<_, BranchProduct>(
        "SELECT p.id::text AS product_id, p.revision AS identity_revision, p.sku, p.barcode, p.name, p.description, p.category,
                i.price_minor, i.tax_rate_bps, i.stock_quantity, i.reorder_point, i.target_stock_quantity,
                i.revision, b.currency_code::text AS currency_code,
                (p.status = 'active' AND i.status = 'active') AS is_active
         FROM branch_catalog_items i
         JOIN catalog_products p ON p.id = i.product_id AND p.tenant_id = i.tenant_id
         JOIN branches b ON b.id = i.branch_id AND b.tenant_id = i.tenant_id
         WHERE i.tenant_id = $1::uuid AND i.branch_id = $2::uuid AND p.id = $3",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(product_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;

    transaction
        .commit()
        .await
        .map_err(|_| CatalogError::Database)?;
    Ok(product)
}

pub(crate) async fn product_branch_availability(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    source_branch_id: &str,
    source_station_id: &str,
    product_id: &str,
) -> Result<Vec<ProductBranchAvailability>, CatalogError> {
    let mut transaction = pool.begin().await.map_err(|_| CatalogError::Database)?;
    set_request_context(
        &mut transaction,
        identity_id,
        tenant_id,
        membership_id,
        source_branch_id,
        Some(source_station_id),
    )
    .await?;
    sqlx::query("SELECT set_config('pos.inventory_availability', 'enabled', true)")
        .execute(&mut *transaction)
        .await
        .map_err(|_| CatalogError::Database)?;

    let available_at_source = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM branch_catalog_items i
            JOIN catalog_products p ON p.id = i.product_id AND p.tenant_id = i.tenant_id
            WHERE i.tenant_id = $1::uuid AND i.branch_id = $2::uuid
              AND i.product_id = $3::uuid AND i.status = 'active' AND p.status = 'active'
        )",
    )
    .bind(tenant_id)
    .bind(source_branch_id)
    .bind(product_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;
    if !available_at_source {
        return Err(CatalogError::NotFound);
    }

    let branches = sqlx::query_as::<_, ProductBranchAvailability>(
        "SELECT b.id::text AS branch_id, b.code AS branch_code, b.name AS branch_name,
                b.address,b.phone,b.email,b.timezone AS time_zone,b.opening_hours AS opening_hours,
                i.stock_quantity,
                COALESCE(commitments.pickup_reserved,0)::bigint AS pickup_reserved_quantity,
                COALESCE(commitments.outbound_committed,0)::bigint AS outbound_committed_quantity,
                GREATEST(i.stock_quantity-COALESCE(commitments.total_committed,0),0)::bigint AS available_to_promise,
                GREATEST(COALESCE(commitments.total_committed,0)-i.stock_quantity,0)::bigint AS commitment_shortfall,
                i.price_minor, b.currency_code::text AS currency_code,
                to_char(now() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS observed_at
         FROM branch_catalog_items i
         JOIN catalog_products p ON p.id = i.product_id AND p.tenant_id = i.tenant_id
         JOIN branches b ON b.id = i.branch_id AND b.tenant_id = i.tenant_id
         LEFT JOIN LATERAL (
             SELECT COALESCE(SUM(a.quantity),0) AS total_committed,
                    COALESCE(SUM(a.quantity) FILTER (WHERE a.pickup_reservation_id IS NOT NULL),0) AS pickup_reserved,
                    COALESCE(SUM(a.quantity) FILTER (WHERE a.stock_transfer_id IS NOT NULL),0) AS outbound_committed
             FROM inventory_allocations a
             WHERE a.tenant_id=i.tenant_id AND a.branch_id=i.branch_id AND a.product_id=i.product_id
               AND a.released_at IS NULL AND (a.expires_at IS NULL OR a.expires_at>clock_timestamp())
         ) commitments ON true
         WHERE i.tenant_id = $1::uuid AND i.product_id = $2::uuid
           AND i.status = 'active' AND p.status = 'active' AND b.status = 'active'
         ORDER BY lower(b.name), b.id",
    )
    .bind(tenant_id)
    .bind(product_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;
    transaction
        .commit()
        .await
        .map_err(|_| CatalogError::Database)?;
    Ok(branches)
}

pub(crate) async fn create_branch_product(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    input: CreateProductInput,
) -> Result<BranchProduct, CatalogError> {
    let input = validate_input(input)?;
    let mut transaction = pool.begin().await.map_err(|_| CatalogError::Database)?;
    set_request_context(
        &mut transaction,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        None,
    )
    .await?;

    let product_id = sqlx::query_scalar::<_, String>(
        "INSERT INTO catalog_products
            (tenant_id, sku, barcode, name, description, category, created_by_membership_id)
         VALUES ($1::uuid, $2, $3, $4, $5, $6, $7::uuid)
         RETURNING id::text",
    )
    .bind(tenant_id)
    .bind(&input.sku)
    .bind(&input.barcode)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.category)
    .bind(membership_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(map_write_error)?;

    sqlx::query(
        "INSERT INTO branch_catalog_items
            (tenant_id, branch_id, product_id, price_minor, cost_minor, tax_rate_bps,
             stock_quantity, reorder_point, target_stock_quantity)
         VALUES ($1::uuid, $2::uuid, $3::uuid, $4, $5, $6, $7, $8, $9)",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(&product_id)
    .bind(input.price_minor)
    .bind(input.cost_minor)
    .bind(input.tax_rate_bps)
    .bind(input.opening_stock)
    .bind(input.reorder_point)
    .bind(input.target_stock_quantity)
    .execute(&mut *transaction)
    .await
    .map_err(map_write_error)?;

    if input.opening_stock > 0 {
        sqlx::query(
            "INSERT INTO stock_movements
                (tenant_id, branch_id, product_id, actor_membership_id,
                 movement_kind, quantity_delta, note)
             VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid,
                     'opening_balance', $5, 'Opening stock recorded when product was created')",
        )
        .bind(tenant_id)
        .bind(branch_id)
        .bind(&product_id)
        .bind(membership_id)
        .bind(input.opening_stock)
        .execute(&mut *transaction)
        .await
        .map_err(|_| CatalogError::Database)?;
    }

    sqlx::query(
        "INSERT INTO audit_events
            (tenant_id, actor_membership_id, action, entity_type, entity_id, metadata)
         VALUES ($1::uuid, $2::uuid, 'catalog.product_created', 'catalog_product', $3,
                 jsonb_build_object('branchId', $4, 'sku', $5))",
    )
    .bind(tenant_id)
    .bind(membership_id)
    .bind(&product_id)
    .bind(branch_id)
    .bind(&input.sku)
    .execute(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;

    let product = sqlx::query_as::<_, BranchProduct>(
        "SELECT p.id::text AS product_id, p.revision AS identity_revision, p.sku, p.barcode, p.name, p.description, p.category,
                i.price_minor, i.tax_rate_bps, i.stock_quantity, i.reorder_point,
                i.target_stock_quantity, i.revision, b.currency_code::text AS currency_code,
                (p.status = 'active' AND i.status = 'active') AS is_active
         FROM branch_catalog_items i
         JOIN catalog_products p ON p.id = i.product_id AND p.tenant_id = i.tenant_id
         JOIN branches b ON b.id = i.branch_id AND b.tenant_id = i.tenant_id
         WHERE i.tenant_id = $1::uuid AND i.branch_id = $2::uuid AND p.id = $3::uuid",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(&product_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;

    transaction
        .commit()
        .await
        .map_err(|_| CatalogError::Database)?;
    Ok(product)
}

pub(crate) async fn update_product_identity(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    product_id: &str,
    input: ProductIdentityInput,
) -> Result<CatalogProductIdentity, CatalogError> {
    let input = validate_product_identity(input)?;
    let mut transaction = pool.begin().await.map_err(|_| CatalogError::Database)?;
    set_request_context(
        &mut transaction,
        identity_id,
        tenant_id,
        membership_id,
        "",
        None,
    )
    .await?;

    let previous = sqlx::query_as::<
        _,
        (
            i64,
            String,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
        ),
    >(
        "SELECT revision, sku, barcode, name, description, category
         FROM catalog_products WHERE tenant_id = $1::uuid AND id = $2::uuid FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(product_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?
    .ok_or(CatalogError::NotFound)?;
    if previous.0 != input.expected_revision {
        return Err(CatalogError::Conflict);
    }

    sqlx::query(
        "UPDATE catalog_products
         SET sku = $3, barcode = $4, name = $5, description = $6, category = $7,
             revision = revision + 1, updated_at = now()
         WHERE tenant_id = $1::uuid AND id = $2::uuid",
    )
    .bind(tenant_id)
    .bind(product_id)
    .bind(&input.sku)
    .bind(&input.barcode)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.category)
    .execute(&mut *transaction)
    .await
    .map_err(map_write_error)?;

    sqlx::query(
        "INSERT INTO audit_events
            (tenant_id, actor_membership_id, action, entity_type, entity_id, metadata)
         VALUES ($1::uuid, $2::uuid, 'catalog.product_identity_updated', 'catalog_product', $3,
                 jsonb_build_object('previousRevision', $4, 'previousSku', $5,
                                    'previousBarcode', $6, 'previousName', $7,
                                    'previousDescription', $8, 'previousCategory', $9,
                                    'sku', $10, 'barcode', $11, 'name', $12,
                                    'description', $13, 'category', $14))",
    )
    .bind(tenant_id)
    .bind(membership_id)
    .bind(product_id)
    .bind(previous.0)
    .bind(&previous.1)
    .bind(&previous.2)
    .bind(&previous.3)
    .bind(&previous.4)
    .bind(&previous.5)
    .bind(&input.sku)
    .bind(&input.barcode)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.category)
    .execute(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;

    let product = sqlx::query_as::<_, CatalogProductIdentity>(
        "SELECT id::text AS product_id, sku, barcode, name, description, category, revision
         FROM catalog_products WHERE tenant_id = $1::uuid AND id = $2::uuid",
    )
    .bind(tenant_id)
    .bind(product_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;
    transaction
        .commit()
        .await
        .map_err(|_| CatalogError::Database)?;
    Ok(product)
}

pub(crate) async fn update_branch_product_settings(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    product_id: &str,
    input: BranchProductSettingsInput,
) -> Result<BranchProduct, CatalogError> {
    if input.price_minor < 0
        || !(0..=10000).contains(&input.tax_rate_bps)
        || input.reorder_point < 0
        || input.target_stock_quantity < 0
        || (input.target_stock_quantity > 0 && input.target_stock_quantity < input.reorder_point)
        || input.expected_revision < 1
    {
        return Err(CatalogError::InvalidInput);
    }

    let mut transaction = pool.begin().await.map_err(|_| CatalogError::Database)?;
    set_request_context(
        &mut transaction,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        None,
    )
    .await?;

    let current_revision = sqlx::query_scalar::<_, i64>(
        "SELECT revision FROM branch_catalog_items
         WHERE tenant_id = $1::uuid AND branch_id = $2::uuid AND product_id = $3::uuid
         FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(product_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?
    .ok_or(CatalogError::NotFound)?;
    if current_revision != input.expected_revision {
        return Err(CatalogError::Conflict);
    }

    sqlx::query(
        "UPDATE branch_catalog_items
         SET price_minor = $4, tax_rate_bps = $5, reorder_point = $6,
             target_stock_quantity = $7, revision = revision + 1, updated_at = now()
         WHERE tenant_id = $1::uuid AND branch_id = $2::uuid AND product_id = $3::uuid",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(product_id)
    .bind(input.price_minor)
    .bind(input.tax_rate_bps)
    .bind(input.reorder_point)
    .bind(input.target_stock_quantity)
    .execute(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;

    sqlx::query(
        "INSERT INTO audit_events
            (tenant_id, actor_membership_id, action, entity_type, entity_id, metadata)
         VALUES ($1::uuid, $2::uuid, 'catalog.branch_product_settings_updated', 'catalog_product', $3,
                 jsonb_build_object('branchId', $4, 'priceMinor', $5, 'taxRateBps', $6,
                                    'reorderPoint', $7, 'targetStockQuantity', $8,
                                    'previousRevision', $9))",
    )
    .bind(tenant_id)
    .bind(membership_id)
    .bind(product_id)
    .bind(branch_id)
    .bind(input.price_minor)
    .bind(input.tax_rate_bps)
    .bind(input.reorder_point)
    .bind(input.target_stock_quantity)
    .bind(input.expected_revision)
    .execute(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;

    let product = sqlx::query_as::<_, BranchProduct>(
        "SELECT p.id::text AS product_id, p.revision AS identity_revision, p.sku, p.barcode, p.name, p.description, p.category,
                i.price_minor, i.tax_rate_bps, i.stock_quantity, i.reorder_point,
                i.target_stock_quantity, i.revision, b.currency_code::text AS currency_code,
                (p.status = 'active' AND i.status = 'active') AS is_active
         FROM branch_catalog_items i
         JOIN catalog_products p ON p.id = i.product_id AND p.tenant_id = i.tenant_id
         JOIN branches b ON b.id = i.branch_id AND b.tenant_id = i.tenant_id
         WHERE i.tenant_id = $1::uuid AND i.branch_id = $2::uuid AND p.id = $3::uuid",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(product_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;
    transaction
        .commit()
        .await
        .map_err(|_| CatalogError::Database)?;
    Ok(product)
}

pub(crate) async fn set_branch_product_active(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    product_id: &str,
    is_active: bool,
) -> Result<BranchProduct, CatalogError> {
    let mut transaction = pool.begin().await.map_err(|_| CatalogError::Database)?;
    set_request_context(
        &mut transaction,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        None,
    )
    .await?;

    let updated = sqlx::query(
        "UPDATE branch_catalog_items
         SET status = CASE WHEN $4 THEN 'active' ELSE 'inactive' END,
             revision = revision + 1, updated_at = now()
         WHERE tenant_id = $1::uuid AND branch_id = $2::uuid AND product_id = $3::uuid",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(product_id)
    .bind(is_active)
    .execute(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;
    if updated.rows_affected() != 1 {
        return Err(CatalogError::NotFound);
    }

    sqlx::query(
        "INSERT INTO audit_events
            (tenant_id, actor_membership_id, action, entity_type, entity_id, metadata)
         VALUES ($1::uuid, $2::uuid, $3, 'catalog_product', $4,
                 jsonb_build_object('branchId', $5, 'isActive', $6))",
    )
    .bind(tenant_id)
    .bind(membership_id)
    .bind(if is_active {
        "catalog.product_restored"
    } else {
        "catalog.product_archived"
    })
    .bind(product_id)
    .bind(branch_id)
    .bind(is_active)
    .execute(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;

    let product = sqlx::query_as::<_, BranchProduct>(
        "SELECT p.id::text AS product_id, p.revision AS identity_revision, p.sku, p.barcode, p.name, p.description, p.category,
                i.price_minor, i.tax_rate_bps, i.stock_quantity, i.reorder_point,
                i.target_stock_quantity, i.revision, b.currency_code::text AS currency_code,
                (p.status = 'active' AND i.status = 'active') AS is_active
         FROM branch_catalog_items i
         JOIN catalog_products p ON p.id = i.product_id AND p.tenant_id = i.tenant_id
         JOIN branches b ON b.id = i.branch_id AND b.tenant_id = i.tenant_id
         WHERE i.tenant_id = $1::uuid AND i.branch_id = $2::uuid AND p.id = $3::uuid",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(product_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;
    transaction
        .commit()
        .await
        .map_err(|_| CatalogError::Database)?;
    Ok(product)
}

pub(crate) async fn adjust_branch_stock(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    product_id: &str,
    idempotency_key: &str,
    input: StockAdjustmentInput,
) -> Result<StockAdjustmentResult, CatalogError> {
    let input = validate_stock_adjustment(input)?;
    let mut transaction = pool.begin().await.map_err(|_| CatalogError::Database)?;
    set_request_context(
        &mut transaction,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        None,
    )
    .await?;

    let lock_key = format!("{tenant_id}:{branch_id}:{membership_id}:{idempotency_key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(lock_key)
        .execute(&mut *transaction)
        .await
        .map_err(|_| CatalogError::Database)?;

    if let Some(existing) = sqlx::query_as::<_, (String, String, i64, Option<String>, i64)>(
        "SELECT id::text, product_id::text, quantity_delta, note, resulting_stock_quantity
         FROM stock_movements
         WHERE tenant_id = $1::uuid AND branch_id = $2::uuid
           AND actor_membership_id = $3::uuid AND idempotency_key = $4",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(membership_id)
    .bind(idempotency_key)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?
    {
        if existing.1 != product_id
            || existing.2 != input.quantity_delta
            || existing.3 != input.note
        {
            return Err(CatalogError::Conflict);
        }
        transaction
            .commit()
            .await
            .map_err(|_| CatalogError::Database)?;
        return Ok(StockAdjustmentResult {
            movement_id: existing.0,
            product_id: existing.1,
            quantity_delta: existing.2,
            note: existing.3,
            resulting_stock_quantity: existing.4,
            replayed: true,
        });
    }

    inventory_allocations::lock_products(&mut transaction, tenant_id, branch_id, [product_id])
        .await
        .map_err(|_| CatalogError::Database)?;

    let current = sqlx::query_as::<_, (i64, bool)>(
        "SELECT i.stock_quantity, (i.status = 'active' AND p.status = 'active')
         FROM branch_catalog_items i
         JOIN catalog_products p ON p.id = i.product_id AND p.tenant_id = i.tenant_id
         WHERE i.tenant_id = $1::uuid AND i.branch_id = $2::uuid AND i.product_id = $3::uuid
         FOR UPDATE OF i",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(product_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?
    .ok_or(CatalogError::NotFound)?;
    if !current.1 {
        return Err(CatalogError::NotFound);
    }
    let resulting_stock_quantity = current
        .0
        .checked_add(input.quantity_delta)
        .filter(|quantity| *quantity >= 0)
        .ok_or(CatalogError::InsufficientStock)?;
    if input.quantity_delta < 0 {
        let committed = inventory_allocations::allocated_quantity(
            &mut transaction,
            tenant_id,
            branch_id,
            product_id,
            None,
        )
        .await
        .map_err(|_| CatalogError::Database)?;
        let requested_decrease = input
            .quantity_delta
            .checked_neg()
            .ok_or(CatalogError::InvalidInput)?;
        if !inventory_allocations::can_consume(current.0, committed, requested_decrease) {
            return Err(CatalogError::InsufficientStock);
        }
    }

    sqlx::query(
        "UPDATE branch_catalog_items
         SET stock_quantity = $4, revision = revision + 1, updated_at = now()
         WHERE tenant_id = $1::uuid AND branch_id = $2::uuid AND product_id = $3::uuid",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(product_id)
    .bind(resulting_stock_quantity)
    .execute(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;

    let movement_id = sqlx::query_scalar::<_, String>(
        "INSERT INTO stock_movements
            (tenant_id, branch_id, product_id, actor_membership_id, movement_kind,
             quantity_delta, note, idempotency_key, resulting_stock_quantity)
         VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, 'adjustment', $5, $6, $7, $8)
         RETURNING id::text",
    )
    .bind(tenant_id)
    .bind(branch_id)
    .bind(product_id)
    .bind(membership_id)
    .bind(input.quantity_delta)
    .bind(&input.note)
    .bind(idempotency_key)
    .bind(resulting_stock_quantity)
    .fetch_one(&mut *transaction)
    .await
    .map_err(map_write_error)?;

    sqlx::query(
        "INSERT INTO audit_events
            (tenant_id, actor_membership_id, action, entity_type, entity_id, metadata)
         VALUES ($1::uuid, $2::uuid, 'inventory.stock_adjusted', 'catalog_product', $3,
                 jsonb_build_object('branchId', $4, 'movementId', $5,
                                    'quantityDelta', $6, 'resultingStockQuantity', $7))",
    )
    .bind(tenant_id)
    .bind(membership_id)
    .bind(product_id)
    .bind(branch_id)
    .bind(&movement_id)
    .bind(input.quantity_delta)
    .bind(resulting_stock_quantity)
    .execute(&mut *transaction)
    .await
    .map_err(|_| CatalogError::Database)?;

    transaction
        .commit()
        .await
        .map_err(|_| CatalogError::Database)?;
    Ok(StockAdjustmentResult {
        movement_id,
        product_id: product_id.to_owned(),
        quantity_delta: input.quantity_delta,
        resulting_stock_quantity,
        note: input.note,
        replayed: false,
    })
}

fn validate_input(mut input: CreateProductInput) -> Result<CreateProductInput, CatalogError> {
    input.sku = input.sku.trim().to_owned();
    if input.sku.is_empty() {
        input.sku = format!("AP-{}", &Uuid::new_v4().simple().to_string()[..12]);
    }
    input.barcode = normalize_optional(input.barcode);
    input.description = normalize_optional(input.description);
    input.category = normalize_optional(input.category);
    input.name = input.name.trim().to_owned();
    if input.sku.chars().count() > 64
        || input
            .barcode
            .as_ref()
            .is_some_and(|value| value.chars().count() > 64)
        || input.name.is_empty()
        || input.name.chars().count() > 160
        || input
            .description
            .as_ref()
            .is_some_and(|value| value.chars().count() > 2000)
        || input
            .category
            .as_ref()
            .is_some_and(|value| value.chars().count() > 120)
        || input.price_minor < 0
        || input.cost_minor.is_some_and(|value| value < 0)
        || !(0..=10000).contains(&input.tax_rate_bps)
        || input.opening_stock < 0
        || input.reorder_point < 0
        || input.target_stock_quantity < 0
        || (input.target_stock_quantity > 0 && input.target_stock_quantity < input.reorder_point)
    {
        return Err(CatalogError::InvalidInput);
    }
    Ok(input)
}

fn validate_product_identity(
    mut input: ProductIdentityInput,
) -> Result<ProductIdentityInput, CatalogError> {
    input.sku = input.sku.trim().to_owned();
    input.barcode = normalize_optional(input.barcode);
    input.description = normalize_optional(input.description);
    input.category = normalize_optional(input.category);
    input.name = input.name.trim().to_owned();
    if input.sku.is_empty()
        || input.sku.chars().count() > 64
        || input
            .barcode
            .as_ref()
            .is_some_and(|value| value.chars().count() > 64)
        || input.name.is_empty()
        || input.name.chars().count() > 160
        || input
            .description
            .as_ref()
            .is_some_and(|value| value.chars().count() > 2000)
        || input
            .category
            .as_ref()
            .is_some_and(|value| value.chars().count() > 120)
        || input.expected_revision < 1
    {
        return Err(CatalogError::InvalidInput);
    }
    Ok(input)
}

fn validate_stock_adjustment(
    mut input: StockAdjustmentInput,
) -> Result<StockAdjustmentInput, CatalogError> {
    input.note = normalize_optional(input.note);
    if input.quantity_delta == 0
        || input
            .note
            .as_ref()
            .is_some_and(|note| note.chars().count() > 500)
    {
        return Err(CatalogError::InvalidInput);
    }
    Ok(input)
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    })
}

fn map_write_error(error: sqlx::Error) -> CatalogError {
    if error
        .as_database_error()
        .is_some_and(|database_error| database_error.is_unique_violation())
    {
        CatalogError::Conflict
    } else {
        CatalogError::Database
    }
}

async fn set_request_context(
    transaction: &mut Transaction<'_, Postgres>,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
) -> Result<(), CatalogError> {
    sqlx::query("SELECT set_config('pos.identity_id', $1, true)")
        .bind(identity_id)
        .execute(&mut **transaction)
        .await
        .map_err(|_| CatalogError::Database)?;
    sqlx::query("SELECT set_config('pos.tenant_id', $1, true)")
        .bind(tenant_id)
        .execute(&mut **transaction)
        .await
        .map_err(|_| CatalogError::Database)?;
    sqlx::query("SELECT set_config('pos.membership_id', $1, true)")
        .bind(membership_id)
        .execute(&mut **transaction)
        .await
        .map_err(|_| CatalogError::Database)?;
    sqlx::query("SELECT set_config('pos.branch_id', $1, true)")
        .bind(branch_id)
        .execute(&mut **transaction)
        .await
        .map_err(|_| CatalogError::Database)?;
    sqlx::query("SELECT set_config('pos.station_id', $1, true)")
        .bind(station_id.unwrap_or_default())
        .execute(&mut **transaction)
        .await
        .map_err(|_| CatalogError::Database)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        validate_input, validate_stock_adjustment, CatalogError, CreateProductInput,
        StockAdjustmentInput,
    };

    fn input() -> CreateProductInput {
        CreateProductInput {
            sku: "  ".to_owned(),
            barcode: Some(" 12345 ".to_owned()),
            name: " Tea ".to_owned(),
            description: Some(" ".to_owned()),
            category: Some(" Grocery ".to_owned()),
            price_minor: 1200,
            cost_minor: Some(700),
            tax_rate_bps: 1500,
            opening_stock: 7,
            reorder_point: 4,
            target_stock_quantity: 12,
        }
    }

    #[test]
    fn product_input_normalizes_optional_fields_and_generates_missing_sku() {
        let normalized = validate_input(input()).unwrap();
        assert!(normalized.sku.starts_with("AP-"));
        assert_eq!(normalized.barcode.as_deref(), Some("12345"));
        assert_eq!(normalized.description, None);
        assert_eq!(normalized.category.as_deref(), Some("Grocery"));
        assert_eq!(normalized.name, "Tea");
    }

    #[test]
    fn product_input_rejects_negative_money_or_stock_and_invalid_reorder_targets() {
        let mut invalid = input();
        invalid.price_minor = -1;
        assert_eq!(
            validate_input(invalid).unwrap_err(),
            CatalogError::InvalidInput
        );

        let mut invalid = input();
        invalid.opening_stock = -1;
        assert_eq!(
            validate_input(invalid).unwrap_err(),
            CatalogError::InvalidInput
        );

        let mut invalid = input();
        invalid.target_stock_quantity = 3;
        assert_eq!(
            validate_input(invalid).unwrap_err(),
            CatalogError::InvalidInput
        );
    }

    #[test]
    fn stock_adjustments_require_a_nonzero_delta_and_normalize_notes() {
        let adjusted = validate_stock_adjustment(StockAdjustmentInput {
            quantity_delta: -2,
            note: Some("  damaged items  ".to_owned()),
        })
        .unwrap();
        assert_eq!(adjusted.note.as_deref(), Some("damaged items"));
        assert_eq!(
            validate_stock_adjustment(StockAdjustmentInput {
                quantity_delta: 0,
                note: None,
            })
            .unwrap_err(),
            CatalogError::InvalidInput
        );
        assert_eq!(
            validate_stock_adjustment(StockAdjustmentInput {
                quantity_delta: 1,
                note: Some("x".repeat(501)),
            })
            .unwrap_err(),
            CatalogError::InvalidInput
        );
    }
}
