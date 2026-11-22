use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceivablesError {
    InvalidInput,
    NotFound,
    IdempotencyConflict,
    AllocationConflict,
    DrawerProof,
    DrawerLease,
    DrawerConflict,
    Database,
}

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const MAX_DRAWER_AMOUNT: i64 = 9_000_000_000_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CashCollectionInput {
    pub customer_id: String,
    pub amount_minor: i64,
    pub drawer_shift_id: String,
    #[serde(default)]
    pub external_reference: Option<String>,
    pub allocations: Vec<CashCollectionAllocationInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CashCollectionAllocationInput {
    pub invoice_branch_id: String,
    pub invoice_id: String,
    pub amount_minor: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CashCollection {
    pub id: String,
    pub customer_id: String,
    pub currency_code: String,
    pub amount_minor: i64,
    pub received_at: String,
    pub allocation_count: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CashCollectionResult {
    pub collection: CashCollection,
    pub replayed: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct CashCollectionDrawerProof<'a> {
    pub station_id: &'a str,
    pub lease_id: &'a str,
    pub lease_token: &'a str,
    pub challenge_id: &'a str,
    pub proof_hex: &'a str,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CustomerCreditPosition {
    pub currency_code: String,
    pub has_credit_terms: bool,
    pub terms_revision: Option<i64>,
    pub credit_limit_minor: String,
    pub due_days: i32,
    pub outstanding_minor: String,
    pub available_minor: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenReceivableInvoice {
    pub invoice_branch_id: String,
    pub invoice_id: String,
    pub issued_at: String,
    pub due_at: String,
    pub original_amount_minor: String,
    pub allocated_minor: String,
    pub open_minor: String,
}

pub(crate) async fn list_open_invoices(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: &str,
    customer_id: &str,
) -> Result<Vec<OpenReceivableInvoice>, ReceivablesError> {
    let customer = Uuid::parse_str(customer_id).map_err(|_| ReceivablesError::InvalidInput)?;
    let mut tx = pool.begin().await.map_err(|_| ReceivablesError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    let invoices = sqlx::query_as::<_, OpenReceivableInvoice>(
        "SELECT invoice_branch_id,invoice_id,issued_at,due_at,original_amount_minor,allocated_minor,open_minor FROM pos_worker.customer_open_invoices($1::uuid)",
    )
    .bind(customer)
    .fetch_all(&mut *tx)
    .await
    .map_err(|error| {
        match error
            .as_database_error()
            .and_then(|database| database.code())
            .as_deref()
        {
            Some("P0002") => ReceivablesError::NotFound,
            Some("42501") => ReceivablesError::DrawerLease,
            _ => ReceivablesError::Database,
        }
    })?;
    tx.commit().await.map_err(|_| ReceivablesError::Database)?;
    Ok(invoices)
}

pub(crate) async fn record_cash_collection(
    pool: &PgPool,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    key: &str,
    raw: &[u8],
    input: CashCollectionInput,
    proof: CashCollectionDrawerProof<'_>,
) -> Result<CashCollectionResult, ReceivablesError> {
    if !valid_idempotency_key(key)
        || Uuid::parse_str(proof.station_id).is_err()
        || !(1..=MAX_DRAWER_AMOUNT).contains(&input.amount_minor)
    {
        return Err(ReceivablesError::InvalidInput);
    }
    let customer =
        Uuid::parse_str(&input.customer_id).map_err(|_| ReceivablesError::InvalidInput)?;
    let shift =
        Uuid::parse_str(&input.drawer_shift_id).map_err(|_| ReceivablesError::InvalidInput)?;
    let reference = input
        .external_reference
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if reference.is_some_and(|value| value.chars().count() > 160)
        || input.allocations.is_empty()
        || input.allocations.len() > 100
    {
        return Err(ReceivablesError::InvalidInput);
    }
    let mut seen = std::collections::HashSet::new();
    let mut allocations = Vec::with_capacity(input.allocations.len());
    let mut allocated = 0_i64;
    for allocation in input.allocations {
        let invoice_branch = Uuid::parse_str(&allocation.invoice_branch_id)
            .map_err(|_| ReceivablesError::InvalidInput)?;
        let invoice =
            Uuid::parse_str(&allocation.invoice_id).map_err(|_| ReceivablesError::InvalidInput)?;
        if !(1..=MAX_SAFE_INTEGER).contains(&allocation.amount_minor)
            || !seen.insert((invoice_branch, invoice))
        {
            return Err(ReceivablesError::InvalidInput);
        }
        allocated = allocated
            .checked_add(allocation.amount_minor)
            .ok_or(ReceivablesError::InvalidInput)?;
        if allocated > MAX_SAFE_INTEGER {
            return Err(ReceivablesError::InvalidInput);
        }
        allocations.push((invoice_branch, invoice, allocation.amount_minor));
    }
    if allocated != input.amount_minor {
        return Err(ReceivablesError::InvalidInput);
    }
    allocations.sort_by_key(|(invoice_branch, invoice, _)| (*invoice_branch, *invoice));

    let mut hasher = Sha256::new();
    hasher.update(raw);
    hasher.update(b"\nbranch:");
    hasher.update(branch.as_bytes());
    hasher.update(b"\nstation:");
    hasher.update(proof.station_id.as_bytes());
    let request_hash = hasher.finalize().to_vec();

    let mut tx = pool.begin().await.map_err(|_| ReceivablesError::Database)?;
    set_context(
        &mut tx,
        identity,
        tenant,
        membership,
        branch,
        proof.station_id,
    )
    .await?;
    let lock_key = format!("receivable-cash-collection:{tenant}:{branch}:{membership}:{key}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(|_| ReceivablesError::Database)?;

    if let Some(previous_hash) = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT request_hash FROM hosted_receivable_collections WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND recorded_by_membership_id=$3::uuid AND idempotency_key=$4",
    )
    .bind(tenant).bind(branch).bind(membership).bind(key)
    .fetch_optional(&mut *tx).await.map_err(|_| ReceivablesError::Database)? {
        if previous_hash != request_hash {
            return Err(ReceivablesError::IdempotencyConflict);
        }
        let collection = load_cash_collection_by_key(&mut tx, tenant, branch, membership, key).await?;
        crate::drawer::record_receivable_collection_inflow(
            &mut tx, identity, membership, tenant, branch, proof.station_id, &shift.to_string(),
            proof.lease_id, proof.lease_token, key, proof.challenge_id, proof.proof_hex, raw,
            Uuid::parse_str(&collection.id).map_err(|_| ReceivablesError::Database)?, input.amount_minor,
        ).await.map_err(map_drawer_error)?;
        tx.commit().await.map_err(|_| ReceivablesError::Database)?;
        return Ok(CashCollectionResult { collection, replayed: true });
    }

    let currency = sqlx::query_scalar::<_, String>(
        "SELECT currency_code FROM branches WHERE tenant_id=$1::uuid AND id=$2::uuid AND status='active'",
    )
    .bind(tenant).bind(branch).fetch_optional(&mut *tx).await.map_err(|_| ReceivablesError::Database)?
    .ok_or(ReceivablesError::NotFound)?;
    let collection_id = Uuid::new_v4();
    let mut collection = sqlx::query_as::<_, CashCollection>(
        "INSERT INTO hosted_receivable_collections(id,tenant_id,branch_id,customer_id,currency_code,amount_minor,payment_method,confirmation_state,external_reference,recorded_by_membership_id,idempotency_key,request_hash,station_id,drawer_shift_id) VALUES($1,$2::uuid,$3::uuid,$4,$5,$6,'cash','cash_recorded',$7,$8::uuid,$9,$10,$11::uuid,$12::uuid) RETURNING id::text,customer_id::text,currency_code::text,amount_minor,received_at::text,0::integer AS allocation_count",
    )
    .bind(collection_id).bind(tenant).bind(branch).bind(customer).bind(&currency).bind(input.amount_minor)
    .bind(reference).bind(membership).bind(key).bind(&request_hash).bind(proof.station_id).bind(shift)
    .fetch_one(&mut *tx).await.map_err(map_collection_write_error)?;
    collection.allocation_count =
        i32::try_from(allocations.len()).map_err(|_| ReceivablesError::InvalidInput)?;
    for (invoice_branch, invoice, amount) in &allocations {
        sqlx::query("INSERT INTO hosted_receivable_allocations(tenant_id,collection_branch_id,collection_id,invoice_branch_id,invoice_id,customer_id,currency_code,amount_minor) VALUES($1::uuid,$2::uuid,$3,$4::uuid,$5,$6,$7,$8)")
            .bind(tenant).bind(branch).bind(collection_id).bind(invoice_branch).bind(invoice)
            .bind(customer).bind(&currency).bind(amount)
            .execute(&mut *tx).await.map_err(map_collection_write_error)?;
    }
    crate::drawer::record_receivable_collection_inflow(
        &mut tx,
        identity,
        membership,
        tenant,
        branch,
        proof.station_id,
        &shift.to_string(),
        proof.lease_id,
        proof.lease_token,
        key,
        proof.challenge_id,
        proof.proof_hex,
        raw,
        collection_id,
        input.amount_minor,
    )
    .await
    .map_err(map_drawer_error)?;
    insert_collection_journal(
        &mut tx,
        tenant,
        branch,
        membership,
        collection_id,
        &currency,
        &collection.received_at,
        input.amount_minor,
    )
    .await?;
    sqlx::query("INSERT INTO audit_events(tenant_id,actor_membership_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,$2::uuid,'receivable_cash_collection_recorded','receivable_collection',$3,$4)")
        .bind(tenant).bind(membership).bind(collection_id)
        .bind(serde_json::json!({"branchId":branch,"customerId":customer.to_string(),"currencyCode":currency,"amountMinor":input.amount_minor,"allocationCount":allocations.len()}))
        .execute(&mut *tx).await.map_err(|_| ReceivablesError::Database)?;
    tx.commit().await.map_err(|_| ReceivablesError::Database)?;
    Ok(CashCollectionResult {
        collection,
        replayed: false,
    })
}

async fn load_cash_collection_by_key(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    membership: &str,
    key: &str,
) -> Result<CashCollection, ReceivablesError> {
    sqlx::query_as::<_, CashCollection>(
        "SELECT c.id::text,c.customer_id::text,c.currency_code::text,c.amount_minor,c.received_at::text,
         (SELECT count(*)::integer FROM hosted_receivable_allocations a WHERE a.tenant_id=c.tenant_id AND a.collection_branch_id=c.branch_id AND a.collection_id=c.id) AS allocation_count
         FROM hosted_receivable_collections c WHERE c.tenant_id=$1::uuid AND c.branch_id=$2::uuid AND c.recorded_by_membership_id=$3::uuid AND c.idempotency_key=$4",
    )
    .bind(tenant).bind(branch).bind(membership).bind(key).fetch_optional(&mut **tx)
    .await.map_err(|_| ReceivablesError::Database)?.ok_or(ReceivablesError::NotFound)
}

async fn insert_collection_journal(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    branch: &str,
    membership: &str,
    collection_id: Uuid,
    currency: &str,
    effective_at: &str,
    amount: i64,
) -> Result<(), ReceivablesError> {
    let journal_id = Uuid::new_v4();
    sqlx::query("INSERT INTO hosted_accounting_journals(id,tenant_id,branch_id,source_type,source_id,currency_code,effective_at,actor_membership_id,description) VALUES($1,$2::uuid,$3::uuid,'receivable_collection',$4,$5,$6::timestamptz,$7::uuid,'Customer receivable cash collection')")
        .bind(journal_id).bind(tenant).bind(branch).bind(collection_id).bind(currency).bind(effective_at).bind(membership)
        .execute(&mut **tx).await.map_err(|_| ReceivablesError::Database)?;
    for (line_number, account_code, description, debit, credit) in [
        (1_i16, 1000, "Register cash", amount, 0_i64),
        (2_i16, 1100, "Accounts receivable", 0_i64, amount),
    ] {
        sqlx::query("INSERT INTO hosted_accounting_journal_lines(tenant_id,branch_id,journal_id,line_number,account_code,description,debit_minor,credit_minor) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6,$7,$8)")
            .bind(tenant).bind(branch).bind(journal_id).bind(line_number).bind(format!("{account_code:04}"))
            .bind(description).bind(debit).bind(credit).execute(&mut **tx).await
            .map_err(|_| ReceivablesError::Database)?;
    }
    Ok(())
}

fn map_drawer_error(error: crate::drawer::DrawerError) -> ReceivablesError {
    match error {
        crate::drawer::DrawerError::LeaseUnavailable => ReceivablesError::DrawerLease,
        crate::drawer::DrawerError::InvalidProof => ReceivablesError::DrawerProof,
        crate::drawer::DrawerError::NoOpenShift
        | crate::drawer::DrawerError::ShiftUnavailable
        | crate::drawer::DrawerError::RegisterCutoverRequired => ReceivablesError::DrawerConflict,
        crate::drawer::DrawerError::InvalidInput
        | crate::drawer::DrawerError::IdempotencyConflict => ReceivablesError::IdempotencyConflict,
        crate::drawer::DrawerError::ShiftAlreadyOpen
        | crate::drawer::DrawerError::AmountOverflow
        | crate::drawer::DrawerError::Database => ReceivablesError::Database,
    }
}

fn map_collection_write_error(error: sqlx::Error) -> ReceivablesError {
    match &error {
        sqlx::Error::Database(database) if database.code().as_deref() == Some("23514") => {
            ReceivablesError::AllocationConflict
        }
        sqlx::Error::Database(database) if database.code().as_deref() == Some("23503") => {
            ReceivablesError::NotFound
        }
        _ => {
            #[cfg(test)]
            eprintln!("receivable collection SQL failed: {error}");
            ReceivablesError::Database
        }
    }
}

fn valid_idempotency_key(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

pub(crate) async fn customer_credit_position(
    pool: &PgPool,
    identity_id: &str,
    tenant_id: &str,
    membership_id: &str,
    branch_id: &str,
    station_id: &str,
    customer_id: &str,
) -> Result<CustomerCreditPosition, ReceivablesError> {
    let customer = Uuid::parse_str(customer_id).map_err(|_| ReceivablesError::InvalidInput)?;
    let mut tx = pool.begin().await.map_err(|_| ReceivablesError::Database)?;
    set_context(
        &mut tx,
        identity_id,
        tenant_id,
        membership_id,
        branch_id,
        station_id,
    )
    .await?;
    let result = sqlx::query_as::<_, CustomerCreditPosition>(
        "SELECT currency_code,has_credit_terms,terms_revision,credit_limit_minor,
                due_days,outstanding_minor,available_minor
         FROM pos_worker.customer_credit_position($1::uuid)",
    )
    .bind(customer)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| {
        if error
            .as_database_error()
            .and_then(|database| database.code())
            .is_some_and(|code| code.as_ref() == "P0002")
        {
            ReceivablesError::NotFound
        } else {
            ReceivablesError::Database
        }
    })?
    .ok_or(ReceivablesError::NotFound)?;
    tx.commit().await.map_err(|_| ReceivablesError::Database)?;
    Ok(result)
}

async fn set_context(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    tenant: &str,
    membership: &str,
    branch: &str,
    station: &str,
) -> Result<(), ReceivablesError> {
    for (key, value) in [
        ("pos.identity_id", identity),
        ("pos.tenant_id", tenant),
        ("pos.membership_id", membership),
        ("pos.branch_id", branch),
        ("pos.station_id", station),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut **tx)
            .await
            .map_err(|_| ReceivablesError::Database)?;
    }
    Ok(())
}
