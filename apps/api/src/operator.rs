// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use std::{
    env,
    net::{IpAddr, SocketAddr},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions, PgSslMode},
    PgPool, Postgres, Transaction,
};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    set_header::SetResponseHeaderLayer,
};
use uuid::Uuid;

use crate::Authenticator;

const MAX_BODY_BYTES: usize = 8 * 1024;
const MAX_TOKEN_LIFETIME_SECONDS: u64 = 300;

#[derive(Clone)]
pub struct OperatorConfig {
    pub bind_address: SocketAddr,
    pub allowed_origins: Vec<HeaderValue>,
    pub authenticator: Option<Authenticator>,
    pub database_options: Option<PgConnectOptions>,
}

impl OperatorConfig {
    pub fn from_env() -> Result<Self, String> {
        let bind_address = env::var("POS_OPERATOR_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8788".to_owned())
            .parse::<SocketAddr>()
            .map_err(|_| "POS_OPERATOR_BIND must be a valid socket address.".to_owned())?;
        let allowed_origins = env::var("POS_OPERATOR_ALLOWED_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(parse_origin)
            .collect::<Result<Vec<_>, _>>()?;
        let authenticator = Authenticator::from_env_with_prefix("POS_OPERATOR_OIDC")?;
        if let Some(operator_auth) = authenticator.as_ref() {
            let forbidden_audiences = env::var("POS_OPERATOR_FORBIDDEN_AUDIENCES")
                .map_err(|_| "POS_OPERATOR_FORBIDDEN_AUDIENCES must list the tenant API audience before operator auth is enabled.".to_owned())?;
            let forbidden_clients = env::var("POS_OPERATOR_FORBIDDEN_CLIENT_IDS")
                .map_err(|_| "POS_OPERATOR_FORBIDDEN_CLIENT_IDS must list tenant client IDs before operator auth is enabled.".to_owned())?;
            let audiences = forbidden_audiences
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>();
            let clients = forbidden_clients
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>();
            if audiences.is_empty()
                || clients.is_empty()
                || audiences
                    .iter()
                    .any(|audience| *audience == operator_auth.api_audience())
                || clients
                    .iter()
                    .any(|client| operator_auth.allows_client_id(client))
            {
                return Err(
                    "Operator and tenant API audiences/client IDs must be non-empty and disjoint."
                        .to_owned(),
                );
            }
        }
        let database_options = env::var("POS_OPERATOR_DATABASE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(|url| {
                parse_database_options(
                    &url,
                    env::var("POS_OPERATOR_ALLOW_INSECURE_DOCKER_DATABASE")
                        .is_ok_and(|value| value.eq_ignore_ascii_case("true")),
                )
            })
            .transpose()?;
        Ok(Self {
            bind_address,
            allowed_origins,
            authenticator,
            database_options,
        })
    }
}

pub(crate) fn parse_database_options(
    url: &str,
    allow_insecure_compose: bool,
) -> Result<PgConnectOptions, String> {
    let options = url
        .parse::<PgConnectOptions>()
        .map_err(|_| "POS_OPERATOR_DATABASE_URL must be a valid PostgreSQL URL.".to_owned())?;
    let host = options.get_host();
    let local = host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
    let compose = host.eq_ignore_ascii_case("postgres");
    if allow_insecure_compose && (!cfg!(debug_assertions) || !compose) {
        return Err("POS_OPERATOR_ALLOW_INSECURE_DOCKER_DATABASE is limited to the postgres Compose service in debug builds.".to_owned());
    }
    if !local && !matches!(options.get_ssl_mode(), PgSslMode::VerifyFull) && !allow_insecure_compose
    {
        return Err(
            "POS_OPERATOR_DATABASE_URL must use sslmode=verify-full for non-local hosts."
                .to_owned(),
        );
    }
    Ok(options.options([("statement_timeout", "5000")]))
}

fn parse_origin(origin: &str) -> Result<HeaderValue, String> {
    let valid_scheme = origin.starts_with("http://") || origin.starts_with("https://");
    let bad_authority = origin
        .split_once("://")
        .is_some_and(|(_, authority)| authority.is_empty() || authority.contains(['/', '?', '#']));
    if !valid_scheme || bad_authority || origin == "*" {
        return Err("POS_OPERATOR_ALLOWED_ORIGINS must contain exact http(s) origins without paths or wildcards.".to_owned());
    }
    HeaderValue::from_str(origin)
        .map_err(|_| "POS_OPERATOR_ALLOWED_ORIGINS contains an invalid origin.".to_owned())
}

#[derive(Clone)]
struct OperatorState {
    authenticator: Option<Authenticator>,
    database: Option<PgPool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorDetail {
    code: &'static str,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Health {
    status: &'static str,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct QueueItem {
    id: String,
    tenant_id: String,
    branch_id: String,
    country_code: String,
    provider_name: String,
    terminal_model: Option<String>,
    capability: String,
    business_impact: String,
    details: Option<String>,
    status: String,
    customer_visible_update: Option<String>,
    due_at: Option<String>,
    revision: i32,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct SupportIssue {
    id: String,
    title: String,
    category: String,
    severity: String,
    status: String,
    assignee_operator_id: Option<String>,
    assignee_operator_name: Option<String>,
    due_at: Option<DateTime<Utc>>,
    target_release: Option<String>,
    revision: i32,
    linked_request_count: i64,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SupportIssuePage {
    items: Vec<SupportIssue>,
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SupportIssueQuery {
    status: Option<String>,
    category: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateSupportIssueInput {
    title: String,
    category: String,
    severity: String,
    payment_request_id: Option<String>,
    support_request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateSupportIssueInput {
    expected_revision: i32,
    status: Option<String>,
    title: Option<String>,
    category: Option<String>,
    severity: Option<String>,
    assignment: Option<String>,
    assignee_operator_id: Option<String>,
    due_at: Option<String>,
    #[serde(default)]
    clear_due_at: bool,
    target_release: Option<String>,
    #[serde(default)]
    clear_target_release: bool,
    private_note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LinkPaymentRequestInput {
    expected_revision: i32,
    relationship: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateSupportRequestInput {
    expected_revision: i32,
    status: String,
    customer_visible_update: Option<String>,
    private_note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LinkSupportRequestInput {
    expected_revision: i32,
    relationship: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct OperatorSupportRequest {
    id: String,
    tenant_id: String,
    branch_id: String,
    category: String,
    title: String,
    details: Option<String>,
    diagnostics_consent: bool,
    diagnostics_app_version: Option<String>,
    diagnostics_platform: Option<String>,
    status: String,
    customer_visible_update: Option<String>,
    revision: i32,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OperatorSupportRequestPage {
    items: Vec<OperatorSupportRequest>,
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct OperatorSupportRequestEvent {
    id: String,
    event_kind: String,
    previous_status: Option<String>,
    new_status: Option<String>,
    customer_visible_update: Option<String>,
    private_note: Option<String>,
    resulting_revision: i32,
    created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OperatorSupportRequestEventPage {
    items: Vec<OperatorSupportRequestEvent>,
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct SupportIssueEvent {
    id: String,
    event_kind: String,
    previous_status: Option<String>,
    new_status: Option<String>,
    previous_assignee_operator_id: Option<String>,
    new_assignee_operator_id: Option<String>,
    due_at: Option<DateTime<Utc>>,
    target_release: Option<String>,
    private_note: Option<String>,
    linked_request_id: Option<String>,
    linked_support_request_id: Option<String>,
    resulting_revision: i32,
    created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SupportIssueEventPage {
    items: Vec<SupportIssueEvent>,
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueueQuery {
    status: Option<String>,
    country: Option<String>,
    provider: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QueuePage {
    items: Vec<QueueItem>,
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct RequestEvent {
    id: String,
    event_kind: String,
    previous_status: Option<String>,
    new_status: Option<String>,
    customer_visible_update: Option<String>,
    private_note: Option<String>,
    due_at: Option<String>,
    resulting_revision: i32,
    created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestEventPage {
    items: Vec<RequestEvent>,
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EventQuery {
    cursor: Option<String>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateInput {
    expected_revision: i32,
    status: String,
    customer_visible_update: Option<String>,
    private_note: Option<String>,
    due_at: Option<String>,
    #[serde(default)]
    clear_due_at: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OperatorProfile {
    id: String,
    role: String,
    display_name: String,
}

#[derive(Debug, sqlx::FromRow)]
struct OperatorRow {
    id: String,
    role: String,
    display_name: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct OperatorDirectoryEntry {
    id: String,
    display_name: String,
}

pub async fn serve(config: OperatorConfig) -> Result<(), Box<dyn std::error::Error>> {
    let database = match config.database_options {
        Some(options) => Some(
            PgPoolOptions::new()
                .max_connections(8)
                .acquire_timeout(std::time::Duration::from_secs(3))
                .connect_with(options)
                .await?,
        ),
        None => None,
    };
    let listener = tokio::net::TcpListener::bind(config.bind_address).await?;
    eprintln!(
        "NimbleTill operator API listening on {}",
        listener.local_addr()?
    );
    axum::serve(
        listener,
        app(config.allowed_origins, config.authenticator, database),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}

pub fn app(
    origins: Vec<HeaderValue>,
    authenticator: Option<Authenticator>,
    database: Option<PgPool>,
) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::PATCH, Method::POST, Method::OPTIONS])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::HeaderName::from_static("idempotency-key"),
        ]);
    Router::new()
        .route("/healthz", get(|| async { Json(Health { status: "ok" }) }))
        .route("/operator/v1/me", get(me))
        .route("/operator/v1/operators", get(list_operators))
        .route(
            "/operator/v1/payment-integration-requests",
            get(list_requests),
        )
        .route(
            "/operator/v1/payment-integration-requests/{request_id}",
            patch(update_request),
        )
        .route(
            "/operator/v1/payment-integration-requests/{request_id}/events",
            get(list_request_events),
        )
        .route("/operator/v1/support-requests", get(list_support_requests))
        .route(
            "/operator/v1/support-requests/{request_id}",
            patch(update_support_request),
        )
        .route(
            "/operator/v1/support-requests/{request_id}/events",
            get(list_support_request_events),
        )
        .route(
            "/operator/v1/issues",
            get(list_support_issues).post(create_support_issue),
        )
        .route(
            "/operator/v1/issues/{issue_id}",
            patch(update_support_issue),
        )
        .route(
            "/operator/v1/issues/{issue_id}/events",
            get(list_support_issue_events),
        )
        .route(
            "/operator/v1/issues/{issue_id}/payment-requests/{request_id}",
            post(link_payment_request),
        )
        .route(
            "/operator/v1/issues/{issue_id}/support-requests/{request_id}",
            post(link_support_request),
        )
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(cors)
        .with_state(OperatorState {
            authenticator,
            database,
        })
}

async fn authenticated_identity(
    state: &OperatorState,
    headers: &HeaderMap,
) -> Result<crate::VerifiedIdentity, Response> {
    let auth = state.authenticator.as_ref().ok_or_else(|| {
        failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "operator_auth_not_configured",
        )
    })?;
    let token = Authenticator::bearer_token(headers)
        .map_err(|_| failure(StatusCode::UNAUTHORIZED, "unauthorized"))?;
    let identity = auth
        .introspect(token)
        .await
        .map_err(|_| failure(StatusCode::UNAUTHORIZED, "unauthorized"))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "clock_unavailable"))?
        .as_secs();
    if identity.expires_at <= now
        || identity.expires_at.saturating_sub(now) > MAX_TOKEN_LIFETIME_SECONDS
    {
        return Err(failure(StatusCode::UNAUTHORIZED, "unauthorized"));
    }
    Ok(identity)
}

async fn begin_operator_tx(
    state: &OperatorState,
    identity: &crate::VerifiedIdentity,
) -> Result<(Transaction<'static, Postgres>, OperatorRow), Response> {
    begin_operator_tx_for_role(state, identity, Some("support")).await
}

async fn begin_operator_tx_for_role(
    state: &OperatorState,
    identity: &crate::VerifiedIdentity,
    required_role: Option<&str>,
) -> Result<(Transaction<'static, Postgres>, OperatorRow), Response> {
    let pool = state.database.as_ref().ok_or_else(|| {
        failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "operator_database_not_configured",
        )
    })?;
    // The transaction owns a pool connection, so it can safely be returned with a 'static lifetime.
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
    for (key, value) in [
        ("pos.operator_issuer", identity.issuer.as_str()),
        ("pos.operator_subject", identity.subject.as_str()),
    ] {
        sqlx::query("SELECT set_config($1,$2,true)")
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
    }
    let (operator_id, role) = sqlx::query_as::<_, (String, String)>("SELECT id::text,role FROM operator_accounts WHERE issuer=$1 AND subject=$2 AND status='active'")
        .bind(&identity.issuer).bind(&identity.subject).fetch_optional(&mut *tx).await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?
        .ok_or_else(|| failure(StatusCode::FORBIDDEN, "operator_access_not_provisioned"))?;
    if required_role.is_some_and(|required| role != required) {
        return Err(failure(
            StatusCode::FORBIDDEN,
            "operator_action_not_allowed",
        ));
    }
    sqlx::query("SELECT set_config('pos.operator_id',$1,true)")
        .bind(&operator_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
    let display_name = sqlx::query_scalar::<_, String>(
        "SELECT display_name FROM operator_directory_profiles WHERE operator_id=$1::uuid AND status='active'",
    )
    .bind(&operator_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?
    .ok_or_else(|| failure(StatusCode::FORBIDDEN, "operator_access_not_provisioned"))?;
    let operator = OperatorRow {
        id: operator_id,
        role,
        display_name,
    };
    Ok((tx, operator))
}

async fn me(State(state): State<OperatorState>, headers: HeaderMap) -> Response {
    let result = async {
        let identity = authenticated_identity(&state, &headers).await?;
        let (tx, operator) = begin_operator_tx_for_role(&state, &identity, None).await?;
        tx.commit()
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        Ok::<_, Response>(
            Json(OperatorProfile {
                id: operator.id,
                role: operator.role,
                display_name: operator.display_name,
            })
            .into_response(),
        )
    }
    .await;
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn list_operators(State(state): State<OperatorState>, headers: HeaderMap) -> Response {
    let result = async {
        let identity = authenticated_identity(&state, &headers).await?;
        let (mut tx, operator) = begin_operator_tx(&state, &identity).await?;
        let operators = sqlx::query_as::<_, OperatorDirectoryEntry>(
            "SELECT operator_id::text AS id,display_name FROM operator_directory_profiles WHERE status='active' ORDER BY lower(display_name),operator_id LIMIT 100",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        sqlx::query("INSERT INTO operator_audit_events(operator_id,action,entity_type,metadata) VALUES($1::uuid,'operator_directory_read','operator_directory',jsonb_build_object('count',$2))")
            .bind(&operator.id)
            .bind(operators.len() as i64)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        tx.commit().await.map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        Ok::<_, Response>(Json(serde_json::json!({ "items": operators })).into_response())
    }
    .await;
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn list_requests(
    State(state): State<OperatorState>,
    headers: HeaderMap,
    Query(query): Query<QueueQuery>,
) -> Response {
    let result = async {
        let limit = query.limit.unwrap_or(50);
        if !(1..=100).contains(&limit) || query.status.as_deref().is_some_and(|s| !valid_status(s))
            || query.country.as_deref().is_some_and(|s| s.len() != 2 || !s.bytes().all(|b| b.is_ascii_uppercase()))
            || query.provider.as_deref().is_some_and(|s| s.trim().is_empty() || s.chars().count() > 120) {
            return Err(failure(StatusCode::BAD_REQUEST, "invalid_queue_filter"));
        }
        let (cursor_time, cursor_id) = parse_cursor(query.cursor.as_deref()).map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_cursor"))?;
        let identity = authenticated_identity(&state, &headers).await?;
        let (mut tx, operator) = begin_operator_tx(&state, &identity).await?;
        let mut items = sqlx::query_as::<_, QueueItem>(
            "SELECT id::text,tenant_id::text,branch_id::text,country_code,provider_name,terminal_model,capability,business_impact,details,status,customer_visible_update,due_at::text,revision,created_at::text,updated_at::text FROM hosted_payment_integration_requests WHERE ($1::text IS NULL OR status=$1) AND ($2::text IS NULL OR country_code=$2) AND ($3::text IS NULL OR provider_name ILIKE '%' || $3 || '%') AND ($4::text IS NULL OR (created_at,id)<($4::timestamptz,$5::uuid)) ORDER BY created_at DESC,id DESC LIMIT $6"
        ).bind(&query.status).bind(&query.country).bind(query.provider.as_deref().map(str::trim)).bind(&cursor_time).bind(&cursor_id).bind(limit+1)
        .fetch_all(&mut *tx).await.map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        sqlx::query("INSERT INTO operator_audit_events(operator_id,action,entity_type,metadata) VALUES($1::uuid,'payment_request_queue_read','payment_integration_request',jsonb_build_object('count',$2))")
            .bind(&operator.id).bind(items.len().min(limit as usize) as i64).execute(&mut *tx).await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        tx.commit().await.map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        let has_more = items.len() > limit as usize;
        items.truncate(limit as usize);
        let next_cursor = if has_more { items.last().map(|item| format!("{}|{}", item.created_at, item.id)) } else { None };
        Ok::<_, Response>(Json(QueuePage { items, next_cursor }).into_response())
    }.await;
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn update_request(
    State(state): State<OperatorState>,
    Path(request_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let result = async {
        let id = Uuid::parse_str(&request_id).map_err(|_| failure(StatusCode::NOT_FOUND, "not_found"))?;
        if body.is_empty() || body.len() > MAX_BODY_BYTES { return Err(failure(StatusCode::PAYLOAD_TOO_LARGE, "request_too_large")); }
        let key = headers.get("idempotency-key").and_then(|h| h.to_str().ok()).unwrap_or("");
        if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| (0x21..=0x7e).contains(&b)) { return Err(failure(StatusCode::BAD_REQUEST, "idempotency_key_required")); }
        let input: UpdateInput = serde_json::from_slice(&body).map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_request"))?;
        validate_update(&input).map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_update"))?;
        if input.clear_due_at && input.due_at.is_some() {
            return Err(failure(StatusCode::BAD_REQUEST, "invalid_update"));
        }
        let due_at = match input.due_at.as_ref() {
            Some(value) => Some(DateTime::parse_from_rfc3339(value)
                .map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_update"))?
                .with_timezone(&Utc)),
            None => None,
        };
        let hash = Sha256::digest(&body).to_vec();
        let identity = authenticated_identity(&state, &headers).await?;
        let (mut tx, operator) = begin_operator_tx(&state, &identity).await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))").bind(format!("{}:{}:{}", operator.id, id, key)).execute(&mut *tx).await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        if let Some(old_hash) = sqlx::query_scalar::<_, Vec<u8>>("SELECT request_hash FROM operator_payment_request_events WHERE operator_id=$1::uuid AND request_id=$2::uuid AND idempotency_key=$3 LIMIT 1")
            .bind(&operator.id).bind(id).bind(key).fetch_optional(&mut *tx).await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))? {
            if old_hash != hash { return Err(failure(StatusCode::CONFLICT, "idempotency_conflict")); }
            let item = fetch_request(&mut tx, id).await?;
            tx.commit().await.map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
            return Ok(Json(item).into_response());
        }
        let current = fetch_request(&mut tx, id).await?;
        if current.revision != input.expected_revision
            || (current.status != input.status && !allowed_transition(&current.status, &input.status))
        {
            return Err(failure(StatusCode::CONFLICT, "revision_or_transition_conflict"));
        }
        let public_update = input.customer_visible_update.as_deref().map(str::trim).filter(|v| !v.is_empty());
        let due_date_changed = input.due_at.is_some() || input.clear_due_at;
        if current.status == input.status && public_update.is_none() && !due_date_changed && input.private_note.as_deref().is_none_or(|n| n.trim().is_empty()) {
            return Err(failure(StatusCode::BAD_REQUEST, "empty_update"));
        }
        let new_revision = input
            .expected_revision
            .checked_add(1)
            .ok_or_else(|| failure(StatusCode::CONFLICT, "revision_or_transition_conflict"))?;
        let updated = sqlx::query_as::<_, QueueItem>(
            "UPDATE hosted_payment_integration_requests SET status=$3,customer_visible_update=COALESCE($4,customer_visible_update),due_at=CASE WHEN $6 THEN NULL WHEN $7 THEN $8 ELSE due_at END,revision=$5,updated_at=now() WHERE id=$1::uuid AND revision=$2 RETURNING id::text,tenant_id::text,branch_id::text,country_code,provider_name,terminal_model,capability,business_impact,details,status,customer_visible_update,due_at::text,revision,created_at::text,updated_at::text"
        ).bind(id).bind(input.expected_revision).bind(&input.status).bind(public_update).bind(new_revision).bind(input.clear_due_at).bind(input.due_at.is_some()).bind(due_at)
            .fetch_optional(&mut *tx).await.map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?
            .ok_or_else(|| failure(StatusCode::CONFLICT, "revision_or_transition_conflict"))?;
        let event_kind = if current.status != input.status { "status_changed" } else if due_date_changed { "schedule_changed" } else { "public_update" };
        sqlx::query("INSERT INTO operator_payment_request_events(request_id,operator_id,event_kind,previous_status,new_status,customer_visible_update,due_at,resulting_revision,idempotency_key,request_hash) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6,$7::timestamptz,$8,$9,$10)")
            .bind(id).bind(&operator.id).bind(event_kind).bind(&current.status).bind(&updated.status).bind(public_update).bind(if due_date_changed { updated.due_at.as_deref() } else { None }).bind(new_revision).bind(key).bind(&hash).execute(&mut *tx).await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        if let Some(note) = input.private_note.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            sqlx::query("INSERT INTO operator_payment_request_events(request_id,operator_id,event_kind,previous_status,new_status,private_note,resulting_revision) VALUES($1::uuid,$2::uuid,'private_note',$3,$3,$4,$5)")
                .bind(id).bind(&operator.id).bind(&updated.status).bind(note).bind(new_revision).execute(&mut *tx).await
                .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        }
        sqlx::query("INSERT INTO operator_audit_events(operator_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,'payment_request_updated','payment_integration_request',$2::uuid,jsonb_build_object('previousStatus',$3,'status',$4,'revision',$5,'hasPublicUpdate',$6,'hasPrivateNote',$7,'hasDueDate',$8))")
            .bind(&operator.id).bind(id).bind(&current.status).bind(&updated.status).bind(new_revision).bind(public_update.is_some()).bind(input.private_note.as_deref().is_some_and(|v| !v.trim().is_empty())).bind(due_date_changed).execute(&mut *tx).await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        tx.commit().await.map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        Ok(Json(updated).into_response())
    }.await;
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn fetch_request(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<QueueItem, Response> {
    sqlx::query_as::<_, QueueItem>("SELECT id::text,tenant_id::text,branch_id::text,country_code,provider_name,terminal_model,capability,business_impact,details,status,customer_visible_update,due_at::text,revision,created_at::text,updated_at::text FROM hosted_payment_integration_requests WHERE id=$1::uuid")
        .bind(id).fetch_optional(&mut **tx).await.map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?
        .ok_or_else(|| failure(StatusCode::NOT_FOUND, "not_found"))
}

async fn fetch_operator_support_request(
    tx: &mut Transaction<'_, Postgres>,
    request_id: Uuid,
) -> Result<OperatorSupportRequest, Response> {
    sqlx::query_as::<_, OperatorSupportRequest>(
        "SELECT id::text,tenant_id::text,branch_id::text,category,title,details,(diagnostics_consent_at IS NOT NULL) AS diagnostics_consent,diagnostics_app_version,diagnostics_platform,status,customer_visible_update,revision,created_at::text,updated_at::text FROM hosted_support_requests WHERE id=$1::uuid",
    )
    .bind(request_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?
    .ok_or_else(|| failure(StatusCode::NOT_FOUND, "not_found"))
}

async fn list_request_events(
    State(state): State<OperatorState>,
    Path(request_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<EventQuery>,
) -> Response {
    let result = async {
        let id = Uuid::parse_str(&request_id)
            .map_err(|_| failure(StatusCode::NOT_FOUND, "not_found"))?;
        let limit = query.limit.unwrap_or(50);
        if !(1..=100).contains(&limit) {
            return Err(failure(StatusCode::BAD_REQUEST, "invalid_event_filter"));
        }
        let (cursor_time, cursor_id) = parse_cursor(query.cursor.as_deref())
            .map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_cursor"))?;
        let identity = authenticated_identity(&state, &headers).await?;
        let (mut tx, operator) = begin_operator_tx(&state, &identity).await?;
        fetch_request(&mut tx, id).await?;
        let mut items = sqlx::query_as::<_, RequestEvent>(
            "SELECT id::text,event_kind,previous_status,new_status,customer_visible_update,private_note,due_at::text,resulting_revision,created_at::text FROM operator_payment_request_events WHERE request_id=$1::uuid AND ($2::text IS NULL OR (created_at,id)<($2::timestamptz,$3::uuid)) ORDER BY created_at DESC,id DESC LIMIT $4",
        )
        .bind(id)
        .bind(&cursor_time)
        .bind(&cursor_id)
        .bind(limit + 1)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        sqlx::query("INSERT INTO operator_audit_events(operator_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,'payment_request_history_read','payment_integration_request',$2::uuid,jsonb_build_object('count',$3))")
            .bind(&operator.id).bind(id).bind(items.len().min(limit as usize) as i64).execute(&mut *tx).await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        tx.commit().await.map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        let has_more = items.len() > limit as usize;
        items.truncate(limit as usize);
        let next_cursor = if has_more {
            items.last().map(|item| format!("{}|{}", item.created_at, item.id))
        } else {
            None
        };
        Ok::<_, Response>(Json(RequestEventPage { items, next_cursor }).into_response())
    }
    .await;
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn list_support_requests(
    State(state): State<OperatorState>,
    headers: HeaderMap,
    Query(query): Query<SupportIssueQuery>,
) -> Response {
    let result = async {
        let limit = query.limit.unwrap_or(50);
        if !(1..=100).contains(&limit)
            || query.status.as_deref().is_some_and(|value| !valid_status(value))
            || query.category.as_deref().is_some_and(|value| !valid_support_request_category(value))
        {
            return Err(failure(StatusCode::BAD_REQUEST, "invalid_support_request_filter"));
        }
        let (cursor_time, cursor_id) = parse_cursor(query.cursor.as_deref())
            .map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_cursor"))?;
        let identity = authenticated_identity(&state, &headers).await?;
        let (mut tx, operator) = begin_operator_tx(&state, &identity).await?;
        let mut items = sqlx::query_as::<_, OperatorSupportRequest>(
            "SELECT id::text,tenant_id::text,branch_id::text,category,title,details,(diagnostics_consent_at IS NOT NULL) AS diagnostics_consent,diagnostics_app_version,diagnostics_platform,status,customer_visible_update,revision,created_at::text,updated_at::text FROM hosted_support_requests WHERE ($1::text IS NULL OR status=$1) AND ($2::text IS NULL OR category=$2) AND ($3::text IS NULL OR (created_at,id)<($3::timestamptz,$4::uuid)) ORDER BY created_at DESC,id DESC LIMIT $5",
        )
        .bind(&query.status)
        .bind(&query.category)
        .bind(&cursor_time)
        .bind(&cursor_id)
        .bind(limit + 1)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        sqlx::query("INSERT INTO operator_audit_events(operator_id,action,entity_type,metadata) VALUES($1::uuid,'support_request_queue_read','support_request',jsonb_build_object('count',$2))")
            .bind(&operator.id)
            .bind(items.len().min(limit as usize) as i64)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        tx.commit().await.map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        let has_more = items.len() > limit as usize;
        items.truncate(limit as usize);
        let next_cursor = if has_more {
            items.last().map(|item| format!("{}|{}", item.created_at, item.id))
        } else {
            None
        };
        Ok::<_, Response>(Json(OperatorSupportRequestPage { items, next_cursor }).into_response())
    }
    .await;
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn update_support_request(
    State(state): State<OperatorState>,
    Path(request_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let result = async {
        let request_id = Uuid::parse_str(&request_id)
            .map_err(|_| failure(StatusCode::NOT_FOUND, "not_found"))?;
        if body.is_empty() || body.len() > MAX_BODY_BYTES {
            return Err(failure(StatusCode::PAYLOAD_TOO_LARGE, "request_too_large"));
        }
        let key = idempotency_key(&headers)?;
        let input: UpdateSupportRequestInput = serde_json::from_slice(&body)
            .map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_request"))?;
        let public_update = input.customer_visible_update.as_deref().map(str::trim).filter(|value| !value.is_empty());
        let private_note = input.private_note.as_deref().map(str::trim).filter(|value| !value.is_empty());
        if input.expected_revision < 1
            || !valid_status(&input.status)
            || public_update.is_some_and(|value| value.chars().count() > 1000 || contains_sensitive_marker(value))
            || private_note.is_some_and(|value| value.chars().count() > 2000 || contains_sensitive_marker(value))
        {
            return Err(failure(StatusCode::BAD_REQUEST, "invalid_support_request_update"));
        }
        let hash = scoped_idempotency_hash(&format!("support-request:update:{request_id}"), &body);
        let identity = authenticated_identity(&state, &headers).await?;
        let (mut tx, operator) = begin_operator_tx(&state, &identity).await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("support-request:{}:{}:{key}", operator.id, request_id))
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        if let Some((old_request_id, old_hash)) = sqlx::query_as::<_, (String, Vec<u8>)>(
            "SELECT support_request_id::text,request_hash FROM operator_support_request_events WHERE operator_id=$1::uuid AND idempotency_key=$2 LIMIT 1",
        )
        .bind(&operator.id)
        .bind(key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?
        {
            if old_hash != hash || old_request_id != request_id.to_string() {
                return Err(failure(StatusCode::CONFLICT, "idempotency_conflict"));
            }
            let item = fetch_operator_support_request(&mut tx, request_id).await?;
            tx.commit().await.map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
            return Ok(Json(item).into_response());
        }
        let current = sqlx::query_as::<_, (i32, String)>(
            "SELECT revision,status FROM hosted_support_requests WHERE id=$1::uuid FOR UPDATE",
        )
        .bind(request_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?
        .ok_or_else(|| failure(StatusCode::NOT_FOUND, "not_found"))?;
        if current.0 != input.expected_revision
            || (current.1 != input.status && !allowed_transition(&current.1, &input.status))
        {
            return Err(failure(StatusCode::CONFLICT, "revision_or_transition_conflict"));
        }
        if current.1 == input.status && public_update.is_none() && private_note.is_none() {
            return Err(failure(StatusCode::BAD_REQUEST, "empty_update"));
        }
        let revision = input.expected_revision.checked_add(1)
            .ok_or_else(|| failure(StatusCode::CONFLICT, "revision_or_transition_conflict"))?;
        let item = sqlx::query_as::<_, OperatorSupportRequest>(
            "UPDATE hosted_support_requests SET status=$2,customer_visible_update=COALESCE($3,customer_visible_update),revision=$4,updated_at=now() WHERE id=$1::uuid AND revision=$5 RETURNING id::text,tenant_id::text,branch_id::text,category,title,details,(diagnostics_consent_at IS NOT NULL) AS diagnostics_consent,diagnostics_app_version,diagnostics_platform,status,customer_visible_update,revision,created_at::text,updated_at::text",
        )
        .bind(request_id)
        .bind(&input.status)
        .bind(public_update)
        .bind(revision)
        .bind(input.expected_revision)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?
        .ok_or_else(|| failure(StatusCode::CONFLICT, "revision_or_transition_conflict"))?;
        let public_changed = current.1 != input.status || public_update.is_some();
        let idempotent_event_kind = if public_changed { "public_update" } else { "private_note" };
        sqlx::query("INSERT INTO operator_support_request_events(support_request_id,operator_id,event_kind,previous_status,new_status,customer_visible_update,private_note,resulting_revision,idempotency_key,request_hash) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6,CASE WHEN $3='private_note' THEN $7 ELSE NULL END,$8,$9,$10)")
            .bind(request_id)
            .bind(&operator.id)
            .bind(idempotent_event_kind)
            .bind(if public_changed { Some(current.1.as_str()) } else { None })
            .bind(if public_changed { Some(item.status.as_str()) } else { None })
            .bind(if public_changed { public_update } else { None })
            .bind(private_note)
            .bind(revision)
            .bind(key)
            .bind(&hash)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        if public_changed && private_note.is_some() {
            sqlx::query("INSERT INTO operator_support_request_events(support_request_id,operator_id,event_kind,private_note,resulting_revision) VALUES($1::uuid,$2::uuid,'private_note',$3,$4)")
                .bind(request_id)
                .bind(&operator.id)
                .bind(private_note)
                .bind(revision)
                .execute(&mut *tx)
                .await
                .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        }
        sqlx::query("INSERT INTO operator_audit_events(operator_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,'support_request_updated','support_request',$2::uuid,jsonb_build_object('previousStatus',$3,'status',$4,'revision',$5,'hasPublicUpdate',$6,'hasPrivateNote',$7))")
            .bind(&operator.id)
            .bind(request_id)
            .bind(&current.1)
            .bind(&item.status)
            .bind(revision)
            .bind(public_update.is_some())
            .bind(private_note.is_some())
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        tx.commit().await.map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        Ok(Json(item).into_response())
    }
    .await;
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn list_support_request_events(
    State(state): State<OperatorState>,
    Path(request_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<EventQuery>,
) -> Response {
    let result = async {
        let request_id = Uuid::parse_str(&request_id)
            .map_err(|_| failure(StatusCode::NOT_FOUND, "not_found"))?;
        let limit = query.limit.unwrap_or(50);
        if !(1..=100).contains(&limit) {
            return Err(failure(StatusCode::BAD_REQUEST, "invalid_event_filter"));
        }
        let (cursor_time, cursor_id) = parse_cursor(query.cursor.as_deref())
            .map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_cursor"))?;
        let identity = authenticated_identity(&state, &headers).await?;
        let (mut tx, operator) = begin_operator_tx(&state, &identity).await?;
        fetch_operator_support_request(&mut tx, request_id).await?;
        let mut items = sqlx::query_as::<_, OperatorSupportRequestEvent>(
            "SELECT id::text,event_kind,previous_status,new_status,customer_visible_update,private_note,resulting_revision,created_at::text FROM operator_support_request_events WHERE support_request_id=$1::uuid AND ($2::text IS NULL OR (created_at,id)<($2::timestamptz,$3::uuid)) ORDER BY created_at DESC,id DESC LIMIT $4",
        )
        .bind(request_id)
        .bind(&cursor_time)
        .bind(&cursor_id)
        .bind(limit + 1)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        sqlx::query("INSERT INTO operator_audit_events(operator_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,'support_request_history_read','support_request',$2::uuid,jsonb_build_object('count',$3))")
            .bind(&operator.id)
            .bind(request_id)
            .bind(items.len().min(limit as usize) as i64)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        tx.commit().await.map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        let has_more = items.len() > limit as usize;
        items.truncate(limit as usize);
        let next_cursor = if has_more { items.last().map(|item| format!("{}|{}", item.created_at, item.id)) } else { None };
        Ok::<_, Response>(Json(OperatorSupportRequestEventPage { items, next_cursor }).into_response())
    }
    .await;
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn list_support_issues(
    State(state): State<OperatorState>,
    headers: HeaderMap,
    Query(query): Query<SupportIssueQuery>,
) -> Response {
    let result = async {
        let limit = query.limit.unwrap_or(50);
        if !(1..=100).contains(&limit)
            || query.status.as_deref().is_some_and(|value| !valid_issue_status(value))
            || query.category.as_deref().is_some_and(|value| !valid_issue_category(value))
        {
            return Err(failure(StatusCode::BAD_REQUEST, "invalid_issue_filter"));
        }
        let (cursor_time, cursor_id) = parse_cursor(query.cursor.as_deref())
            .map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_cursor"))?;
        let identity = authenticated_identity(&state, &headers).await?;
        let (mut tx, operator) = begin_operator_tx(&state, &identity).await?;
        let mut items = sqlx::query_as::<_, SupportIssue>(
            "SELECT i.id::text,i.title,i.category,i.severity,i.status,i.assignee_operator_id::text,
                    (SELECT dp.display_name FROM operator_directory_profiles dp WHERE dp.operator_id=i.assignee_operator_id AND dp.status='active') AS assignee_operator_name,
                    i.due_at,i.target_release,i.revision,
                    ((SELECT COUNT(*) FROM operator_support_issue_payment_requests pl WHERE pl.issue_id=i.id)
                     + (SELECT COUNT(*) FROM operator_support_issue_support_requests sl WHERE sl.issue_id=i.id))::bigint AS linked_request_count,
                    i.created_at::text,i.updated_at::text
             FROM operator_support_issues i
             WHERE ($1::text IS NULL OR i.status=$1)
               AND ($2::text IS NULL OR i.category=$2)
               AND ($3::text IS NULL OR (i.created_at,i.id)<($3::timestamptz,$4::uuid))
             ORDER BY i.created_at DESC,i.id DESC LIMIT $5",
        )
        .bind(&query.status)
        .bind(&query.category)
        .bind(&cursor_time)
        .bind(&cursor_id)
        .bind(limit + 1)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        sqlx::query("INSERT INTO operator_audit_events(operator_id,action,entity_type,metadata) VALUES($1::uuid,'support_issue_queue_read','support_issue',jsonb_build_object('count',$2))")
            .bind(&operator.id)
            .bind(items.len().min(limit as usize) as i64)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        tx.commit()
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        let has_more = items.len() > limit as usize;
        items.truncate(limit as usize);
        let next_cursor = if has_more {
            items.last().map(|item| format!("{}|{}", item.created_at, item.id))
        } else {
            None
        };
        Ok::<_, Response>(Json(SupportIssuePage { items, next_cursor }).into_response())
    }
    .await;
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn create_support_issue(
    State(state): State<OperatorState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let result = async {
        if body.is_empty() || body.len() > MAX_BODY_BYTES {
            return Err(failure(StatusCode::PAYLOAD_TOO_LARGE, "request_too_large"));
        }
        let key = idempotency_key(&headers)?;
        let input: CreateSupportIssueInput = serde_json::from_slice(&body)
            .map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_request"))?;
        let title = input.title.trim();
        if !(3..=200).contains(&title.chars().count())
            || contains_sensitive_marker(title)
            || !valid_issue_category(&input.category)
            || !valid_issue_severity(&input.severity)
            || (input.payment_request_id.is_some() && input.support_request_id.is_some())
        {
            return Err(failure(StatusCode::BAD_REQUEST, "invalid_issue"));
        }
        let request_id = input
            .payment_request_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()
            .map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_payment_request_id"))?;
        let support_request_id = input
            .support_request_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()
            .map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_support_request_id"))?;
        let hash = scoped_idempotency_hash("support-issue:create", &body);
        let identity = authenticated_identity(&state, &headers).await?;
        let (mut tx, operator) = begin_operator_tx(&state, &identity).await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("support-issue:{}:{key}", operator.id))
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        if let Some((old_issue_id, old_hash, old_kind)) = sqlx::query_as::<_, (String, Vec<u8>, String)>(
            "SELECT issue_id::text,request_hash,event_kind FROM operator_support_issue_events WHERE operator_id=$1::uuid AND idempotency_key=$2 LIMIT 1",
        )
        .bind(&operator.id)
        .bind(key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?
        {
            if old_hash != hash || old_kind != "created" {
                return Err(failure(StatusCode::CONFLICT, "idempotency_conflict"));
            }
            let issue_id = Uuid::parse_str(&old_issue_id)
                .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
            let issue = fetch_support_issue(&mut tx, issue_id).await?;
            tx.commit()
                .await
                .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
            return Ok(Json(issue).into_response());
        }
        if let Some(request_id) = request_id {
            fetch_request(&mut tx, request_id).await?;
        }
        if let Some(support_request_id) = support_request_id {
            fetch_operator_support_request(&mut tx, support_request_id).await?;
        }
        let issue_id = sqlx::query_scalar::<_, String>(
            "INSERT INTO operator_support_issues(title,category,severity,status,created_by_operator_id) VALUES($1,$2,$3,'inbox',$4::uuid) RETURNING id::text",
        )
        .bind(title)
        .bind(&input.category)
        .bind(&input.severity)
        .bind(&operator.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        let issue_uuid = Uuid::parse_str(&issue_id)
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        sqlx::query("INSERT INTO operator_support_issue_events(issue_id,operator_id,event_kind,new_status,resulting_revision,idempotency_key,request_hash) VALUES($1::uuid,$2::uuid,'created','inbox',1,$3,$4)")
            .bind(issue_uuid)
            .bind(&operator.id)
            .bind(key)
            .bind(&hash)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        if let Some(request_id) = request_id {
            sqlx::query("INSERT INTO operator_support_issue_payment_requests(issue_id,request_id,relationship,linked_by_operator_id) VALUES($1::uuid,$2::uuid,'primary',$3::uuid)")
                .bind(issue_uuid)
                .bind(request_id)
                .bind(&operator.id)
                .execute(&mut *tx)
                .await
                .map_err(|_| failure(StatusCode::CONFLICT, "payment_request_already_linked"))?;
            sqlx::query("INSERT INTO operator_support_issue_events(issue_id,operator_id,event_kind,linked_request_id,resulting_revision) VALUES($1::uuid,$2::uuid,'payment_request_linked',$3::uuid,1)")
                .bind(issue_uuid)
                .bind(&operator.id)
                .bind(request_id)
                .execute(&mut *tx)
                .await
                .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        }
        if let Some(support_request_id) = support_request_id {
            sqlx::query("INSERT INTO operator_support_issue_support_requests(issue_id,support_request_id,relationship,linked_by_operator_id) VALUES($1::uuid,$2::uuid,'primary',$3::uuid)")
                .bind(issue_uuid)
                .bind(support_request_id)
                .bind(&operator.id)
                .execute(&mut *tx)
                .await
                .map_err(|_| failure(StatusCode::CONFLICT, "support_request_already_linked"))?;
            sqlx::query("INSERT INTO operator_support_issue_events(issue_id,operator_id,event_kind,linked_support_request_id,resulting_revision) VALUES($1::uuid,$2::uuid,'support_request_linked',$3::uuid,1)")
                .bind(issue_uuid)
                .bind(&operator.id)
                .bind(support_request_id)
                .execute(&mut *tx)
                .await
                .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        }
        sqlx::query("INSERT INTO operator_audit_events(operator_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,'support_issue_created','support_issue',$2::uuid,jsonb_build_object('category',$3,'hasPrimaryRequest',$4))")
            .bind(&operator.id)
            .bind(issue_uuid)
            .bind(&input.category)
            .bind(request_id.is_some() || support_request_id.is_some())
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        let issue = fetch_support_issue(&mut tx, issue_uuid).await?;
        tx.commit()
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        Ok(Json(issue).into_response())
    }
    .await;
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn update_support_issue(
    State(state): State<OperatorState>,
    Path(issue_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let result = async {
        let issue_id = Uuid::parse_str(&issue_id)
            .map_err(|_| failure(StatusCode::NOT_FOUND, "not_found"))?;
        if body.is_empty() || body.len() > MAX_BODY_BYTES {
            return Err(failure(StatusCode::PAYLOAD_TOO_LARGE, "request_too_large"));
        }
        let key = idempotency_key(&headers)?;
        let input: UpdateSupportIssueInput = serde_json::from_slice(&body)
            .map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_request"))?;
        if !validate_issue_update(&input) {
            return Err(failure(StatusCode::BAD_REQUEST, "invalid_issue_update"));
        }
        let due_at = input.due_at.as_deref().map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|parsed| parsed.with_timezone(&Utc))
                .map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_issue_update"))
        }).transpose()?;
        let hash = scoped_idempotency_hash(&format!("support-issue:update:{issue_id}"), &body);
        let identity = authenticated_identity(&state, &headers).await?;
        let (mut tx, operator) = begin_operator_tx(&state, &identity).await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("support-issue:{}:{key}", operator.id))
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        if let Some((old_issue_id, old_hash, old_kind)) = sqlx::query_as::<_, (String, Vec<u8>, String)>(
            "SELECT issue_id::text,request_hash,event_kind FROM operator_support_issue_events WHERE operator_id=$1::uuid AND idempotency_key=$2 LIMIT 1",
        )
        .bind(&operator.id)
        .bind(key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?
        {
            if old_hash != hash || old_kind == "created" || old_issue_id != issue_id.to_string() {
                return Err(failure(StatusCode::CONFLICT, "idempotency_conflict"));
            }
            let issue = fetch_support_issue(&mut tx, issue_id).await?;
            tx.commit()
                .await
                .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
            return Ok(Json(issue).into_response());
        }
        lock_support_issue(&mut tx, issue_id).await?;
        let current = fetch_support_issue(&mut tx, issue_id).await?;
        let status = input.status.as_deref().unwrap_or(&current.status);
        let category = input.category.as_deref().unwrap_or(&current.category);
        let severity = input.severity.as_deref().unwrap_or(&current.severity);
        let title = input.title.as_deref().map(str::trim).unwrap_or(&current.title);
        if !allowed_issue_transition(&current.status, status) {
            return Err(failure(StatusCode::CONFLICT, "revision_or_transition_conflict"));
        }
        let requested_assignee = match input.assignment.as_deref() {
            Some("claim") => {
                if current.assignee_operator_id.as_deref().is_some_and(|id| id != operator.id) {
                    return Err(failure(StatusCode::CONFLICT, "assignment_conflict"));
                }
                Some(operator.id.clone())
            }
            Some("unassign") => None,
            Some(_) => return Err(failure(StatusCode::BAD_REQUEST, "invalid_issue_update")),
            None => match input.assignee_operator_id.as_deref() {
                Some(value) => {
                    let assignee_id = Uuid::parse_str(value)
                        .map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_assignee"))?;
                    let active = sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM operator_directory_profiles WHERE operator_id=$1 AND status='active')",
                    )
                    .bind(assignee_id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
                    if !active { return Err(failure(StatusCode::BAD_REQUEST, "invalid_assignee")); }
                    Some(assignee_id.to_string())
                }
                None => current.assignee_operator_id.clone(),
            },
        };
        let assignment_changed = requested_assignee != current.assignee_operator_id;
        let due_changed = if input.clear_due_at {
            current.due_at.is_some()
        } else {
            due_at.as_ref().is_some_and(|value| current.due_at.as_ref() != Some(value))
        };
        let release_changed = if input.clear_target_release {
            current.target_release.is_some()
        } else {
            input.target_release.as_deref().is_some_and(|value| current.target_release.as_deref() != Some(value.trim()))
        };
        let status_changed = status != current.status;
        let has_private_note = input.private_note.as_deref().is_some_and(|value| !value.trim().is_empty());
        let scalar_changed = input.title.as_deref().is_some_and(|value| value.trim() != current.title)
            || input.category.as_deref().is_some_and(|value| value != current.category)
            || input.severity.as_deref().is_some_and(|value| value != current.severity);
        if !status_changed && !assignment_changed && !due_changed && !release_changed && !scalar_changed && !has_private_note {
            return Err(failure(StatusCode::BAD_REQUEST, "empty_update"));
        }
        let next_revision = input.expected_revision.checked_add(1)
            .ok_or_else(|| failure(StatusCode::CONFLICT, "revision_or_transition_conflict"))?;
        let assignee_id = requested_assignee.as_deref();
        let updated = sqlx::query_as::<_, SupportIssue>(
            "UPDATE operator_support_issues i
             SET title=$3,category=$4,severity=$5,status=$6,
                 assignee_operator_id=CASE WHEN $7 THEN $8::uuid ELSE i.assignee_operator_id END,
                 due_at=CASE WHEN $9 THEN NULL WHEN $10 THEN $11 ELSE i.due_at END,
                 target_release=CASE WHEN $12 THEN NULL WHEN $13 THEN $14 ELSE i.target_release END,
                 revision=$15,updated_at=now()
             WHERE i.id=$1::uuid AND i.revision=$2
             RETURNING i.id::text,i.title,i.category,i.severity,i.status,i.assignee_operator_id::text,
                       (SELECT dp.display_name FROM operator_directory_profiles dp WHERE dp.operator_id=i.assignee_operator_id AND dp.status='active') AS assignee_operator_name,
                       i.due_at,i.target_release,i.revision,
                ((SELECT COUNT(*)::bigint FROM operator_support_issue_payment_requests l WHERE l.issue_id=i.id)
                 + (SELECT COUNT(*)::bigint FROM operator_support_issue_support_requests s WHERE s.issue_id=i.id)) AS linked_request_count,
                       i.created_at::text,i.updated_at::text",
        )
        .bind(issue_id)
        .bind(input.expected_revision)
        .bind(title)
        .bind(category)
        .bind(severity)
        .bind(status)
        .bind(assignment_changed)
        .bind(assignee_id)
        .bind(input.clear_due_at)
        .bind(input.due_at.is_some())
        .bind(due_at)
        .bind(input.clear_target_release)
        .bind(input.target_release.is_some())
        .bind(input.target_release.as_deref().map(str::trim))
        .bind(next_revision)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?
        .ok_or_else(|| failure(StatusCode::CONFLICT, "revision_or_transition_conflict"))?;
        let mut event_kinds = Vec::new();
        if status_changed { event_kinds.push("status_changed"); }
        if assignment_changed { event_kinds.push("assignment_changed"); }
        if due_changed || release_changed { event_kinds.push("schedule_changed"); }
        if scalar_changed { event_kinds.push("updated"); }
        for (index, event_kind) in event_kinds.iter().enumerate() {
            sqlx::query("INSERT INTO operator_support_issue_events(issue_id,operator_id,event_kind,previous_status,new_status,previous_assignee_operator_id,new_assignee_operator_id,due_at,target_release,resulting_revision,idempotency_key,request_hash) VALUES($1::uuid,$2::uuid,$3,CASE WHEN $4 THEN $5 ELSE NULL END,CASE WHEN $4 THEN $6 ELSE NULL END,CASE WHEN $7 THEN $8::uuid ELSE NULL END,CASE WHEN $7 THEN $9::uuid ELSE NULL END,CASE WHEN $10 THEN $11::timestamptz ELSE NULL END,CASE WHEN $12 THEN $13 ELSE NULL END,$14,$15,$16)")
                .bind(issue_id)
                .bind(&operator.id)
                .bind(event_kind)
                .bind(status_changed)
                .bind(&current.status)
                .bind(&updated.status)
                .bind(assignment_changed)
                .bind(&current.assignee_operator_id)
                .bind(&updated.assignee_operator_id)
                .bind(due_changed)
                .bind(&updated.due_at)
                .bind(release_changed)
                .bind(&updated.target_release)
                .bind(next_revision)
                .bind((index == 0).then_some(key))
                .bind((index == 0).then_some(hash.as_slice()))
                .execute(&mut *tx)
                .await
                .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        }
        if event_kinds.is_empty() && has_private_note {
            let note = input.private_note.as_deref().unwrap_or_default().trim();
            sqlx::query("INSERT INTO operator_support_issue_events(issue_id,operator_id,event_kind,private_note,resulting_revision,idempotency_key,request_hash) VALUES($1::uuid,$2::uuid,'private_note',$3,$4,$5,$6)")
                .bind(issue_id)
                .bind(&operator.id)
                .bind(note)
                .bind(next_revision)
                .bind(key)
                .bind(&hash)
                .execute(&mut *tx)
                .await
                .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        }
        if let Some(note) = input.private_note.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
            if !event_kinds.is_empty() {
                sqlx::query("INSERT INTO operator_support_issue_events(issue_id,operator_id,event_kind,private_note,resulting_revision) VALUES($1::uuid,$2::uuid,'private_note',$3,$4)")
                .bind(issue_id)
                .bind(&operator.id)
                .bind(note)
                .bind(next_revision)
                .execute(&mut *tx)
                .await
                .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
            }
        }
        sqlx::query("INSERT INTO operator_audit_events(operator_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,'support_issue_updated','support_issue',$2::uuid,jsonb_build_object('revision',$3,'status',$4,'assignmentChanged',$5,'scheduleChanged',$6,'hasPrivateNote',$7))")
            .bind(&operator.id)
            .bind(issue_id)
            .bind(next_revision)
            .bind(&updated.status)
            .bind(assignment_changed)
            .bind(due_changed || release_changed)
            .bind(has_private_note)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        tx.commit()
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        Ok(Json(updated).into_response())
    }
    .await;
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn link_payment_request(
    State(state): State<OperatorState>,
    Path((issue_id, request_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let result = async {
        let issue_id = Uuid::parse_str(&issue_id)
            .map_err(|_| failure(StatusCode::NOT_FOUND, "not_found"))?;
        let request_id = Uuid::parse_str(&request_id)
            .map_err(|_| failure(StatusCode::NOT_FOUND, "not_found"))?;
        if body.is_empty() || body.len() > MAX_BODY_BYTES {
            return Err(failure(StatusCode::PAYLOAD_TOO_LARGE, "request_too_large"));
        }
        let key = idempotency_key(&headers)?;
        let input: LinkPaymentRequestInput = serde_json::from_slice(&body)
            .map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_request"))?;
        if input.expected_revision < 1
            || !matches!(input.relationship.as_str(), "primary" | "duplicate" | "related")
        {
            return Err(failure(StatusCode::BAD_REQUEST, "invalid_issue_link"));
        }
        let hash = scoped_idempotency_hash(&format!("support-issue:link:{issue_id}:{request_id}"), &body);
        let identity = authenticated_identity(&state, &headers).await?;
        let (mut tx, operator) = begin_operator_tx(&state, &identity).await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("support-issue:{}:{key}", operator.id))
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        if let Some((old_issue_id, old_hash, old_kind)) = sqlx::query_as::<_, (String, Vec<u8>, String)>(
            "SELECT issue_id::text,request_hash,event_kind FROM operator_support_issue_events WHERE operator_id=$1::uuid AND idempotency_key=$2 LIMIT 1",
        )
        .bind(&operator.id)
        .bind(key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?
        {
            if old_hash != hash || old_kind != "payment_request_linked" || old_issue_id != issue_id.to_string() {
                return Err(failure(StatusCode::CONFLICT, "idempotency_conflict"));
            }
            let issue = fetch_support_issue(&mut tx, issue_id).await?;
            tx.commit()
                .await
                .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
            return Ok(Json(issue).into_response());
        }
        lock_support_issue(&mut tx, issue_id).await?;
        let current = fetch_support_issue(&mut tx, issue_id).await?;
        if current.revision != input.expected_revision {
            return Err(failure(StatusCode::CONFLICT, "revision_or_transition_conflict"));
        }
        fetch_request(&mut tx, request_id).await?;
        let next_revision = input.expected_revision.checked_add(1)
            .ok_or_else(|| failure(StatusCode::CONFLICT, "revision_or_transition_conflict"))?;
        sqlx::query("INSERT INTO operator_support_issue_payment_requests(issue_id,request_id,relationship,linked_by_operator_id) VALUES($1::uuid,$2::uuid,$3,$4::uuid)")
            .bind(issue_id)
            .bind(request_id)
            .bind(&input.relationship)
            .bind(&operator.id)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::CONFLICT, "payment_request_already_linked_or_primary_exists"))?;
        sqlx::query("UPDATE operator_support_issues SET revision=$2,updated_at=now() WHERE id=$1::uuid AND revision=$3")
            .bind(issue_id)
            .bind(next_revision)
            .bind(input.expected_revision)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        sqlx::query("INSERT INTO operator_support_issue_events(issue_id,operator_id,event_kind,linked_request_id,resulting_revision,idempotency_key,request_hash) VALUES($1::uuid,$2::uuid,'payment_request_linked',$3::uuid,$4,$5,$6)")
            .bind(issue_id)
            .bind(&operator.id)
            .bind(request_id)
            .bind(next_revision)
            .bind(key)
            .bind(&hash)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        sqlx::query("INSERT INTO operator_audit_events(operator_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,'support_issue_request_linked','support_issue',$2::uuid,jsonb_build_object('relationship',$3))")
            .bind(&operator.id)
            .bind(issue_id)
            .bind(&input.relationship)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        let issue = fetch_support_issue(&mut tx, issue_id).await?;
        tx.commit()
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        Ok(Json(issue).into_response())
    }
    .await;
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn link_support_request(
    State(state): State<OperatorState>,
    Path((issue_id, request_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let result = async {
        let issue_id = Uuid::parse_str(&issue_id)
            .map_err(|_| failure(StatusCode::NOT_FOUND, "not_found"))?;
        let request_id = Uuid::parse_str(&request_id)
            .map_err(|_| failure(StatusCode::NOT_FOUND, "not_found"))?;
        if body.is_empty() || body.len() > MAX_BODY_BYTES {
            return Err(failure(StatusCode::PAYLOAD_TOO_LARGE, "request_too_large"));
        }
        let key = idempotency_key(&headers)?;
        let input: LinkSupportRequestInput = serde_json::from_slice(&body)
            .map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_request"))?;
        if input.expected_revision < 1
            || !matches!(input.relationship.as_str(), "primary" | "duplicate" | "related")
        {
            return Err(failure(StatusCode::BAD_REQUEST, "invalid_issue_link"));
        }
        let hash = scoped_idempotency_hash(&format!("support-issue:link-source:{issue_id}:{request_id}"), &body);
        let identity = authenticated_identity(&state, &headers).await?;
        let (mut tx, operator) = begin_operator_tx(&state, &identity).await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("support-issue:{}:{key}", operator.id))
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        if let Some((old_issue_id, old_request_id, old_hash, old_kind)) = sqlx::query_as::<_, (String, Option<String>, Vec<u8>, String)>(
            "SELECT issue_id::text,linked_support_request_id::text,request_hash,event_kind FROM operator_support_issue_events WHERE operator_id=$1::uuid AND idempotency_key=$2 LIMIT 1",
        )
        .bind(&operator.id)
        .bind(key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?
        {
            let request_id_string = request_id.to_string();
            if old_hash != hash
                || old_kind != "support_request_linked"
                || old_issue_id != issue_id.to_string()
                || old_request_id.as_deref() != Some(request_id_string.as_str())
            {
                return Err(failure(StatusCode::CONFLICT, "idempotency_conflict"));
            }
            let issue = fetch_support_issue(&mut tx, issue_id).await?;
            tx.commit().await.map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
            return Ok(Json(issue).into_response());
        }
        lock_support_issue(&mut tx, issue_id).await?;
        let current = fetch_support_issue(&mut tx, issue_id).await?;
        if current.revision != input.expected_revision {
            return Err(failure(StatusCode::CONFLICT, "revision_or_transition_conflict"));
        }
        fetch_operator_support_request(&mut tx, request_id).await?;
        let next_revision = input.expected_revision.checked_add(1)
            .ok_or_else(|| failure(StatusCode::CONFLICT, "revision_or_transition_conflict"))?;
        sqlx::query("INSERT INTO operator_support_issue_support_requests(issue_id,support_request_id,relationship,linked_by_operator_id) VALUES($1::uuid,$2::uuid,$3,$4::uuid)")
            .bind(issue_id)
            .bind(request_id)
            .bind(&input.relationship)
            .bind(&operator.id)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::CONFLICT, "support_request_already_linked_or_primary_exists"))?;
        sqlx::query("UPDATE operator_support_issues SET revision=$2,updated_at=now() WHERE id=$1::uuid AND revision=$3")
            .bind(issue_id)
            .bind(next_revision)
            .bind(input.expected_revision)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        sqlx::query("INSERT INTO operator_support_issue_events(issue_id,operator_id,event_kind,linked_support_request_id,resulting_revision,idempotency_key,request_hash) VALUES($1::uuid,$2::uuid,'support_request_linked',$3::uuid,$4,$5,$6)")
            .bind(issue_id)
            .bind(&operator.id)
            .bind(request_id)
            .bind(next_revision)
            .bind(key)
            .bind(&hash)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        sqlx::query("INSERT INTO operator_audit_events(operator_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,'support_issue_request_linked','support_issue',$2::uuid,jsonb_build_object('sourceType','support_request','relationship',$3))")
            .bind(&operator.id)
            .bind(issue_id)
            .bind(&input.relationship)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        let issue = fetch_support_issue(&mut tx, issue_id).await?;
        tx.commit().await.map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        Ok(Json(issue).into_response())
    }
    .await;
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn list_support_issue_events(
    State(state): State<OperatorState>,
    Path(issue_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<EventQuery>,
) -> Response {
    let result = async {
        let issue_id = Uuid::parse_str(&issue_id)
            .map_err(|_| failure(StatusCode::NOT_FOUND, "not_found"))?;
        let limit = query.limit.unwrap_or(50);
        if !(1..=100).contains(&limit) {
            return Err(failure(StatusCode::BAD_REQUEST, "invalid_event_filter"));
        }
        let (cursor_time, cursor_id) = parse_cursor(query.cursor.as_deref())
            .map_err(|_| failure(StatusCode::BAD_REQUEST, "invalid_cursor"))?;
        let identity = authenticated_identity(&state, &headers).await?;
        let (mut tx, operator) = begin_operator_tx(&state, &identity).await?;
        fetch_support_issue(&mut tx, issue_id).await?;
        let mut items = sqlx::query_as::<_, SupportIssueEvent>(
            "SELECT id::text,event_kind,previous_status,new_status,
                    previous_assignee_operator_id::text,new_assignee_operator_id::text,
                    due_at,target_release,private_note,linked_request_id::text,linked_support_request_id::text,
                    resulting_revision,created_at::text
             FROM operator_support_issue_events
             WHERE issue_id=$1::uuid
               AND ($2::text IS NULL OR (created_at,id)<($2::timestamptz,$3::uuid))
             ORDER BY created_at DESC,id DESC LIMIT $4",
        )
        .bind(issue_id)
        .bind(&cursor_time)
        .bind(&cursor_id)
        .bind(limit + 1)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        sqlx::query("INSERT INTO operator_audit_events(operator_id,action,entity_type,entity_id,metadata) VALUES($1::uuid,'support_issue_history_read','support_issue',$2::uuid,jsonb_build_object('count',$3))")
            .bind(&operator.id)
            .bind(issue_id)
            .bind(items.len().min(limit as usize) as i64)
            .execute(&mut *tx)
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        tx.commit()
            .await
            .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
        let has_more = items.len() > limit as usize;
        items.truncate(limit as usize);
        let next_cursor = if has_more {
            items.last().map(|item| format!("{}|{}", item.created_at, item.id))
        } else {
            None
        };
        Ok::<_, Response>(Json(SupportIssueEventPage { items, next_cursor }).into_response())
    }
    .await;
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn fetch_support_issue(
    tx: &mut Transaction<'_, Postgres>,
    issue_id: Uuid,
) -> Result<SupportIssue, Response> {
    sqlx::query_as::<_, SupportIssue>(
        "SELECT i.id::text,i.title,i.category,i.severity,i.status,i.assignee_operator_id::text,
                (SELECT dp.display_name FROM operator_directory_profiles dp WHERE dp.operator_id=i.assignee_operator_id AND dp.status='active') AS assignee_operator_name,
                i.due_at,i.target_release,i.revision,
                ((SELECT COUNT(*)::bigint FROM operator_support_issue_payment_requests l WHERE l.issue_id=i.id)
                 + (SELECT COUNT(*)::bigint FROM operator_support_issue_support_requests s WHERE s.issue_id=i.id)) AS linked_request_count,
                i.created_at::text,i.updated_at::text
         FROM operator_support_issues i WHERE i.id=$1::uuid",
    )
    .bind(issue_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?
    .ok_or_else(|| failure(StatusCode::NOT_FOUND, "not_found"))
}

async fn lock_support_issue(
    tx: &mut Transaction<'_, Postgres>,
    issue_id: Uuid,
) -> Result<(), Response> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM operator_support_issues WHERE id=$1::uuid FOR UPDATE",
    )
    .bind(issue_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| failure(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?
    .ok_or_else(|| failure(StatusCode::NOT_FOUND, "not_found"))?;
    Ok(())
}

fn idempotency_key(headers: &HeaderMap) -> Result<&str, Response> {
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if key.is_empty() || key.len() > 128 || !key.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) {
        return Err(failure(StatusCode::BAD_REQUEST, "idempotency_key_required"));
    }
    Ok(key)
}

fn scoped_idempotency_hash(scope: &str, body: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(scope.as_bytes());
    hasher.update([0]);
    hasher.update(body);
    hasher.finalize().to_vec()
}

fn valid_issue_category(value: &str) -> bool {
    matches!(
        value,
        "bug"
            | "feature_request"
            | "payment_integration"
            | "account_access"
            | "data_issue"
            | "other"
    )
}

fn valid_support_request_category(value: &str) -> bool {
    matches!(
        value,
        "bug" | "feature_request" | "account_access" | "data_issue" | "training" | "other"
    )
}

fn valid_issue_severity(value: &str) -> bool {
    matches!(value, "critical" | "high" | "normal" | "low")
}

fn valid_issue_status(value: &str) -> bool {
    matches!(
        value,
        "inbox"
            | "triaged"
            | "scheduled"
            | "in_progress"
            | "review"
            | "qa"
            | "release"
            | "resolved"
            | "blocked"
            | "duplicate"
            | "reopened"
    )
}

fn allowed_issue_transition(from: &str, to: &str) -> bool {
    from == to
        || matches!(
            (from, to),
            ("inbox", "triaged" | "blocked" | "duplicate")
                | (
                    "triaged",
                    "scheduled" | "in_progress" | "blocked" | "duplicate"
                )
                | ("scheduled", "in_progress" | "blocked" | "duplicate")
                | ("in_progress", "review" | "blocked" | "duplicate")
                | ("review", "qa" | "in_progress" | "blocked")
                | ("qa", "release" | "in_progress" | "blocked")
                | ("release", "resolved" | "in_progress")
                | ("resolved", "reopened")
                | (
                    "reopened",
                    "triaged" | "in_progress" | "blocked" | "duplicate"
                )
                | (
                    "blocked",
                    "triaged" | "scheduled" | "in_progress" | "duplicate"
                )
                | ("duplicate", "triaged")
        )
}

fn validate_issue_update(input: &UpdateSupportIssueInput) -> bool {
    let too_long_or_sensitive =
        |value: &str, max: usize| value.chars().count() > max || contains_sensitive_marker(value);
    input.expected_revision > 0
        && input.status.as_deref().is_none_or(valid_issue_status)
        && input.category.as_deref().is_none_or(valid_issue_category)
        && input.severity.as_deref().is_none_or(valid_issue_severity)
        && input
            .assignment
            .as_deref()
            .is_none_or(|value| matches!(value, "claim" | "unassign"))
        && !(input.assignment.is_some() && input.assignee_operator_id.is_some())
        && input
            .assignee_operator_id
            .as_deref()
            .is_none_or(|value| Uuid::parse_str(value).is_ok())
        && !(input.clear_due_at && input.due_at.is_some())
        && !(input.clear_target_release && input.target_release.is_some())
        && input.title.as_deref().is_none_or(|value| {
            (3..=200).contains(&value.trim().chars().count())
                && !too_long_or_sensitive(value.trim(), 200)
        })
        && input.target_release.as_deref().is_none_or(|value| {
            !value.trim().is_empty() && !too_long_or_sensitive(value.trim(), 80)
        })
        && input
            .private_note
            .as_deref()
            .is_none_or(|value| !too_long_or_sensitive(value.trim(), 2000))
        && input
            .private_note
            .as_deref()
            .is_none_or(|value| value.trim().is_empty() || value.trim().chars().count() >= 3)
}

fn parse_cursor(cursor: Option<&str>) -> Result<(Option<String>, Option<String>), ()> {
    match cursor {
        None => Ok((None, None)),
        Some(value) if value.len() <= 100 => {
            let (timestamp, id) = value.split_once('|').ok_or(())?;
            Uuid::parse_str(id).map_err(|_| ())?;
            if timestamp.is_empty() {
                return Err(());
            }
            Ok((Some(timestamp.to_owned()), Some(id.to_owned())))
        }
        _ => Err(()),
    }
}

fn valid_status(status: &str) -> bool {
    matches!(
        status,
        "submitted" | "under_review" | "planned" | "in_progress" | "released" | "not_planned"
    )
}

fn allowed_transition(from: &str, to: &str) -> bool {
    matches!(
        (from, to),
        ("submitted", "under_review" | "not_planned")
            | ("under_review", "planned" | "in_progress" | "not_planned")
            | ("planned", "in_progress" | "not_planned")
            | ("in_progress", "released" | "not_planned")
    )
}

fn validate_update(input: &UpdateInput) -> Result<(), ()> {
    let bad_text =
        |value: &str, max: usize| value.chars().count() > max || contains_sensitive_marker(value);
    if input.expected_revision < 1
        || !valid_status(&input.status)
        || input
            .customer_visible_update
            .as_deref()
            .is_some_and(|v| bad_text(v, 1000))
        || input
            .private_note
            .as_deref()
            .is_some_and(|v| bad_text(v, 2000))
    {
        return Err(());
    }
    Ok(())
}

pub(crate) fn contains_sensitive_marker(value: &str) -> bool {
    let parts = value
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    parts.iter().any(|s| {
        matches!(
            s.as_str(),
            "password" | "passphrase" | "secret" | "cvv" | "pin"
        )
    }) || parts.windows(2).any(|pair| {
        matches!(
            (pair[0].as_str(), pair[1].as_str()),
            ("api", "key")
                | ("access", "token")
                | ("refresh", "token")
                | ("card", "number")
                | ("account", "number")
        )
    })
}

fn failure(status: StatusCode, error: &'static str) -> Response {
    let message = match error {
        "unauthorized" => "A valid operator access token is required.",
        "operator_access_not_provisioned" => {
            "This identity is not provisioned for operator access."
        }
        "operator_action_not_allowed" => "The operator role does not allow this action.",
        "revision_or_transition_conflict" => {
            "The request changed or this status transition is no longer allowed."
        }
        "idempotency_conflict" => "The idempotency key was already used for a different update.",
        "invalid_queue_filter"
        | "invalid_cursor"
        | "invalid_update"
        | "idempotency_key_required"
        | "empty_update" => "The request fields are invalid or incomplete.",
        "request_too_large" => "The request body is too large.",
        "invalid_issue_filter" | "invalid_issue" | "invalid_issue_update" | "invalid_issue_link"
        | "invalid_payment_request_id" | "invalid_assignee" => "The issue fields are invalid or incomplete.",
        "payment_request_already_linked" | "payment_request_already_linked_or_primary_exists" => {
            "This payment request is already linked to an issue, or the issue already has a primary request."
        }
        "assignment_conflict" => "Another operator owns this issue assignment. Refresh the issue before changing it.",
        "not_found" => "The requested record was not found.",
        _ => "The operator service is temporarily unavailable.",
    };
    (
        status,
        Json(ErrorBody {
            error: ErrorDetail {
                code: error,
                message,
            },
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn serve_test_api(
        authenticator: Authenticator,
        database: sqlx::PgPool,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                app(Vec::new(), Some(authenticator), Some(database)),
            )
            .await
            .unwrap();
        });
        (format!("http://{address}"), server)
    }

    async fn serve_tenant_test_api(
        authenticator: Authenticator,
        database: sqlx::PgPool,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                crate::app_with_runtime(Vec::new(), Some(authenticator), Some(database)),
            )
            .await
            .unwrap();
        });
        (format!("http://{address}"), server)
    }

    #[test]
    fn status_transitions_are_forward_only_and_terminal_states_stay_terminal() {
        assert!(allowed_transition("submitted", "under_review"));
        assert!(allowed_transition("under_review", "in_progress"));
        assert!(allowed_transition("in_progress", "released"));
        assert!(!allowed_transition("released", "in_progress"));
        assert!(!allowed_transition("not_planned", "under_review"));
        assert!(!allowed_transition("submitted", "released"));
    }

    #[test]
    fn operator_updates_reject_secret_markers_and_oversized_text() {
        let valid = UpdateInput {
            expected_revision: 1,
            status: "under_review".to_owned(),
            customer_visible_update: Some("We are checking support for the model.".to_owned()),
            private_note: None,
            due_at: None,
            clear_due_at: false,
        };
        assert!(validate_update(&valid).is_ok());
        let secret = UpdateInput {
            customer_visible_update: Some("Send us the API key".to_owned()),
            ..valid
        };
        assert!(validate_update(&secret).is_err());
    }

    #[test]
    fn queue_cursor_requires_a_timestamp_and_uuid_pair() {
        assert_eq!(parse_cursor(None), Ok((None, None)));
        let id = "b0000000-0000-0000-0000-000000000001";
        let parsed = parse_cursor(Some(&format!("2026-11-01 12:00:00+00|{id}"))).unwrap();
        assert_eq!(parsed.1.as_deref(), Some(id));
        assert!(parse_cursor(Some("not-a-cursor")).is_err());
        assert!(parse_cursor(Some("2026-11-01|invalid")).is_err());
    }

    #[tokio::test]
    #[ignore = "requires the disposable PostgreSQL verifier environment"]
    async fn operator_http_auth_and_updates_use_the_restricted_operator_database_role() {
        let seed_url = std::env::var("POS_API_TEST_SEED_DATABASE_URL")
            .expect("the verifier must provide a disposable migration credential");
        let runtime_url = std::env::var("POS_OPERATOR_TEST_DATABASE_URL")
            .expect("the verifier must provide the restricted operator runtime login URL");

        let active_subject = format!("operator-{}", Uuid::new_v4());
        let inactive_subject = format!("inactive-{}", Uuid::new_v4());
        let unprovisioned_subject = format!("unprovisioned-{}", Uuid::new_v4());
        let tenant_subject = "tenant-manager";
        let (active_issuer, active_auth, active_idp) =
            crate::auth::tests::start_test_authenticator_for(
                &active_subject,
                "operator-token",
                "operator-api",
                "operator-console",
                240,
            )
            .await;
        let (inactive_issuer, inactive_auth, inactive_idp) =
            crate::auth::tests::start_test_authenticator_for(
                &inactive_subject,
                "inactive-token",
                "operator-api",
                "operator-console",
                240,
            )
            .await;
        let (_unprovisioned_issuer, unprovisioned_auth, unprovisioned_idp) =
            crate::auth::tests::start_test_authenticator_for(
                &unprovisioned_subject,
                "unprovisioned-token",
                "operator-api",
                "operator-console",
                240,
            )
            .await;
        let (tenant_issuer, tenant_auth, tenant_idp) =
            crate::auth::tests::start_test_authenticator_for(
                tenant_subject,
                "tenant-token",
                "pos-api",
                "cashier-app",
                240,
            )
            .await;
        let tenant_two_subject = format!("tenant-manager-{}", Uuid::new_v4());
        let (tenant_two_issuer, tenant_two_auth, _tenant_two_idp) =
            crate::auth::tests::start_test_authenticator_for(
                &tenant_two_subject,
                "tenant-two-token",
                "pos-api",
                "cashier-app",
                240,
            )
            .await;

        let tenant_id = Uuid::new_v4();
        let branch_id = Uuid::new_v4();
        let identity_id = Uuid::new_v4();
        let membership_id = Uuid::new_v4();
        let request_id = Uuid::new_v4();
        let support_request_id = Uuid::new_v4();
        let tenant_two_id = Uuid::new_v4();
        let branch_two_id = Uuid::new_v4();
        let identity_two_id = Uuid::new_v4();
        let membership_two_id = Uuid::new_v4();
        let request_two_id = Uuid::new_v4();
        let active_operator_id = Uuid::new_v4();
        let teammate_operator_id = Uuid::new_v4();
        let inactive_operator_id = Uuid::new_v4();
        let seed = sqlx::PgPool::connect(&seed_url).await.unwrap();
        sqlx::query(
            "INSERT INTO tenants(id,display_name) VALUES($1,'Operator HTTP verifier tenant')",
        )
        .bind(tenant_id)
        .execute(&seed)
        .await
        .unwrap();
        sqlx::query("INSERT INTO tenants(id,display_name) VALUES($1,'Second linked tenant')")
            .bind(tenant_two_id)
            .execute(&seed)
            .await
            .unwrap();
        sqlx::query("INSERT INTO branches(id,tenant_id,code,name,timezone) VALUES($1,$2,'OP-HTTP-2','Second operator tenant','Africa/Accra')")
            .bind(branch_two_id)
            .bind(tenant_two_id)
            .execute(&seed)
            .await
            .unwrap();
        sqlx::query("INSERT INTO person_identities(id,issuer,subject) VALUES($1,$2,$3)")
            .bind(identity_two_id)
            .bind(&tenant_two_issuer)
            .bind(&tenant_two_subject)
            .execute(&seed)
            .await
            .unwrap();
        sqlx::query("INSERT INTO memberships(id,tenant_id,identity_id) VALUES($1,$2,$3)")
            .bind(membership_two_id)
            .bind(tenant_two_id)
            .bind(identity_two_id)
            .execute(&seed)
            .await
            .unwrap();
        sqlx::query("INSERT INTO membership_role_grants(tenant_id,membership_id,role_key) VALUES($1,$2,'owner')")
            .bind(tenant_two_id)
            .bind(membership_two_id)
            .execute(&seed)
            .await
            .unwrap();
        sqlx::query("INSERT INTO hosted_payment_integration_requests(id,tenant_id,branch_id,created_by_membership_id,country_code,provider_name,capability,business_impact,idempotency_key,request_hash) VALUES($1,$2,$3,$4,'GH','Verifier Bank','Terminal integration','business_blocked','operator-http-seed-2',$5)")
            .bind(request_two_id)
            .bind(tenant_two_id)
            .bind(branch_two_id)
            .bind(membership_two_id)
            .bind(vec![1_u8; 32])
            .execute(&seed)
            .await
            .unwrap();
        sqlx::query("INSERT INTO branches(id,tenant_id,code,name,timezone) VALUES($1,$2,'OP-HTTP','Operator HTTP verifier','Africa/Accra')")
            .bind(branch_id)
            .bind(tenant_id)
            .execute(&seed)
            .await
            .unwrap();
        sqlx::query("INSERT INTO person_identities(id,issuer,subject) VALUES($1,$2,$3)")
            .bind(identity_id)
            .bind(&tenant_issuer)
            .bind(tenant_subject)
            .execute(&seed)
            .await
            .unwrap();
        sqlx::query("INSERT INTO memberships(id,tenant_id,identity_id) VALUES($1,$2,$3)")
            .bind(membership_id)
            .bind(tenant_id)
            .bind(identity_id)
            .execute(&seed)
            .await
            .unwrap();
        sqlx::query("INSERT INTO membership_role_grants(tenant_id,membership_id,role_key) VALUES($1,$2,'owner')")
            .bind(tenant_id)
            .bind(membership_id)
            .execute(&seed)
            .await
            .unwrap();
        sqlx::query("INSERT INTO hosted_payment_integration_requests(id,tenant_id,branch_id,created_by_membership_id,country_code,provider_name,capability,business_impact,idempotency_key,request_hash) VALUES($1,$2,$3,$4,'GH','Verifier Bank','Terminal integration','workaround_available','operator-http-seed',$5)")
            .bind(request_id)
            .bind(tenant_id)
            .bind(branch_id)
            .bind(membership_id)
            .bind(vec![0_u8; 32])
            .execute(&seed)
            .await
            .unwrap();
        sqlx::query("INSERT INTO hosted_support_requests(id,tenant_id,branch_id,created_by_membership_id,category,title,details,diagnostics_consent_at,diagnostics_app_version,diagnostics_platform,idempotency_key,request_hash) VALUES($1,$2,$3,$4,'bug','Cash drawer warning does not clear','The warning remains after the drawer is closed.',now(),'0.1.0','web','operator-support-http-seed',$5)")
            .bind(support_request_id)
            .bind(tenant_id)
            .bind(branch_id)
            .bind(membership_id)
            .bind(vec![2_u8; 32])
            .execute(&seed)
            .await
            .unwrap();
        sqlx::query("INSERT INTO operator_accounts(id,issuer,subject,role,status,deactivated_at) VALUES($1,$2,$3,'support','active',NULL),($4,'https://operator.example',$5,'support','active',NULL),($6,$7,$8,'support','deactivated',now())")
            .bind(active_operator_id)
            .bind(&active_issuer)
            .bind(&active_subject)
            .bind(teammate_operator_id)
            .bind(format!("teammate-{}", Uuid::new_v4()))
            .bind(inactive_operator_id)
            .bind(&inactive_issuer)
            .bind(&inactive_subject)
            .execute(&seed)
            .await
            .unwrap();
        sqlx::query("UPDATE operator_directory_profiles SET display_name=CASE operator_id WHEN $1 THEN 'Alex Support' WHEN $2 THEN 'Morgan Support' ELSE display_name END WHERE operator_id IN ($1,$2)")
            .bind(active_operator_id)
            .bind(teammate_operator_id)
            .execute(&seed)
            .await
            .unwrap();
        seed.close().await;

        let database = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&runtime_url)
            .await
            .expect("operator API must use the restricted operator runtime login");
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();

        let (active_base, active_server) = serve_test_api(active_auth, database.clone()).await;
        let profile = client
            .get(format!("{active_base}/operator/v1/me"))
            .bearer_auth("operator-token")
            .send()
            .await
            .unwrap();
        assert_eq!(profile.status(), StatusCode::OK);
        let profile_json: serde_json::Value = profile.json().await.unwrap();
        assert_eq!(profile_json["id"], active_operator_id.to_string());
        assert_eq!(profile_json["displayName"], "Alex Support");

        let directory = client
            .get(format!("{active_base}/operator/v1/operators"))
            .bearer_auth("operator-token")
            .send()
            .await
            .unwrap();
        assert_eq!(directory.status(), StatusCode::OK);
        let directory_json: serde_json::Value = directory.json().await.unwrap();
        assert!(directory_json["items"].as_array().unwrap().len() >= 2);
        assert!(directory_json["items"].as_array().unwrap().iter().any(|item|
            item == &serde_json::json!({"id": teammate_operator_id.to_string(), "displayName": "Morgan Support"})
        ));
        assert!(directory_json["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| {
                item.get("issuer").is_none()
                    && item.get("subject").is_none()
                    && item["id"] != inactive_operator_id.to_string()
            }));

        let tenant_token = client
            .get(format!("{active_base}/operator/v1/me"))
            .bearer_auth("tenant-token")
            .send()
            .await
            .unwrap();
        assert_eq!(tenant_token.status(), StatusCode::UNAUTHORIZED);

        let queue_url = format!("{active_base}/operator/v1/payment-integration-requests");
        let queue = client
            .get(&queue_url)
            .bearer_auth("operator-token")
            .send()
            .await
            .unwrap();
        assert_eq!(queue.status(), StatusCode::OK);
        let queue_json: serde_json::Value = queue.json().await.unwrap();
        assert!(queue_json["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == request_id.to_string()));
        assert!(queue_json["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == request_two_id.to_string()));

        let support_queue = client
            .get(format!("{active_base}/operator/v1/support-requests"))
            .bearer_auth("operator-token")
            .send()
            .await
            .unwrap();
        assert_eq!(support_queue.status(), StatusCode::OK);
        let support_queue_json: serde_json::Value = support_queue.json().await.unwrap();
        assert!(support_queue_json["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == support_request_id.to_string()));
        let opted_in_support_request = support_queue_json["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == support_request_id.to_string())
            .unwrap();
        assert_eq!(opted_in_support_request["diagnosticsConsent"], true);
        assert_eq!(opted_in_support_request["diagnosticsAppVersion"], "0.1.0");
        assert_eq!(opted_in_support_request["diagnosticsPlatform"], "web");

        let support_issue = client
            .post(format!("{active_base}/operator/v1/issues"))
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-support-issue-create-1")
            .json(&serde_json::json!({
                "title": "Investigate cash drawer warning",
                "category": "bug",
                "severity": "normal",
                "supportRequestId": support_request_id.to_string()
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(support_issue.status(), StatusCode::OK);
        let support_issue_json: serde_json::Value = support_issue.json().await.unwrap();
        let support_issue_id = support_issue_json["id"].as_str().unwrap();
        assert_eq!(support_issue_json["linkedRequestCount"], 1);

        let support_request_url =
            format!("{active_base}/operator/v1/support-requests/{support_request_id}");
        let support_request_update = client
            .patch(&support_request_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-support-request-update-1")
            .json(&serde_json::json!({
                "expectedRevision": 1,
                "status": "under_review",
                "customerVisibleUpdate": "We are reviewing the drawer workflow.",
                "privateNote": "Check the drawer sensor logs with QA."
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(support_request_update.status(), StatusCode::OK);
        let support_request_updated: serde_json::Value =
            support_request_update.json().await.unwrap();
        assert_eq!(support_request_updated["status"], "under_review");
        assert_eq!(support_request_updated["revision"], 2);
        assert_eq!(
            support_request_updated["customerVisibleUpdate"],
            "We are reviewing the drawer workflow."
        );
        assert!(support_request_updated.get("privateNote").is_none());

        let support_request_events = client
            .get(format!("{support_request_url}/events"))
            .bearer_auth("operator-token")
            .send()
            .await
            .unwrap();
        assert_eq!(support_request_events.status(), StatusCode::OK);
        let support_request_events_json: serde_json::Value =
            support_request_events.json().await.unwrap();
        let support_request_event_items = support_request_events_json["items"].as_array().unwrap();
        assert!(support_request_event_items.iter().any(|event| {
            event["eventKind"] == "public_update"
                && event["customerVisibleUpdate"] == "We are reviewing the drawer workflow."
        }));
        assert!(support_request_event_items.iter().any(|event| {
            event["eventKind"] == "private_note"
                && event["privateNote"] == "Check the drawer sensor logs with QA."
        }));
        assert!(!support_issue_id.is_empty());

        let tenant_issue_queue = client
            .get(format!("{active_base}/operator/v1/issues"))
            .bearer_auth("tenant-token")
            .send()
            .await
            .unwrap();
        assert_eq!(tenant_issue_queue.status(), StatusCode::UNAUTHORIZED);

        let create_issue_url = format!("{active_base}/operator/v1/issues");
        let create_issue_payload = serde_json::json!({
            "title": "Support Verifier Bank terminal setup",
            "category": "payment_integration",
            "severity": "high",
            "paymentRequestId": request_id.to_string()
        });
        let created_issue_response = client
            .post(&create_issue_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-issue-create-1")
            .json(&create_issue_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(created_issue_response.status(), StatusCode::OK);
        let created_issue: serde_json::Value = created_issue_response.json().await.unwrap();
        let issue_id = created_issue["id"].as_str().unwrap().to_owned();
        assert_eq!(created_issue["status"], "inbox");
        assert_eq!(created_issue["revision"], 1);
        assert_eq!(created_issue["linkedRequestCount"], 1);
        assert!(created_issue.get("tenantId").is_none());
        assert!(created_issue.get("branchId").is_none());

        let create_issue_replay = client
            .post(&create_issue_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-issue-create-1")
            .json(&create_issue_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(create_issue_replay.status(), StatusCode::OK);
        let replayed_issue: serde_json::Value = create_issue_replay.json().await.unwrap();
        assert_eq!(replayed_issue["id"], issue_id);
        assert_eq!(replayed_issue["revision"], 1);

        let conflicting_issue_replay = client
            .post(&create_issue_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-issue-create-1")
            .json(&serde_json::json!({
                "title": "Different issue with reused key",
                "category": "bug",
                "severity": "normal"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(conflicting_issue_replay.status(), StatusCode::CONFLICT);

        let link_duplicate_url = format!(
            "{active_base}/operator/v1/issues/{issue_id}/payment-requests/{request_two_id}"
        );
        let link_duplicate_payload =
            serde_json::json!({ "expectedRevision": 1, "relationship": "duplicate" });
        let linked = client
            .post(&link_duplicate_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-issue-link-1")
            .json(&link_duplicate_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(linked.status(), StatusCode::OK);
        let linked_issue: serde_json::Value = linked.json().await.unwrap();
        assert_eq!(linked_issue["revision"], 2);
        assert_eq!(linked_issue["linkedRequestCount"], 2);

        let linked_replay = client
            .post(&link_duplicate_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-issue-link-1")
            .json(&link_duplicate_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(linked_replay.status(), StatusCode::OK);
        let linked_replay_json: serde_json::Value = linked_replay.json().await.unwrap();
        assert_eq!(linked_replay_json["revision"], 2);

        let stale_issue_link = client
            .post(&link_duplicate_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-issue-link-stale")
            .json(&link_duplicate_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(stale_issue_link.status(), StatusCode::CONFLICT);

        let update_issue_url = format!("{active_base}/operator/v1/issues/{issue_id}");
        let update_issue_payload = serde_json::json!({
            "expectedRevision": 2,
            "status": "triaged",
            "assignment": "claim",
            "dueAt": "2026-11-20T09:00:00Z",
            "targetRelease": "1.1.0",
            "privateNote": "Coordinate with the processor team."
        });
        let issue_updated_response = client
            .patch(&update_issue_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-issue-update-1")
            .json(&update_issue_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(issue_updated_response.status(), StatusCode::OK);
        let issue_updated: serde_json::Value = issue_updated_response.json().await.unwrap();
        assert_eq!(issue_updated["revision"], 3);
        assert_eq!(issue_updated["status"], "triaged");
        assert_eq!(
            issue_updated["assigneeOperatorId"],
            active_operator_id.to_string()
        );
        assert_eq!(issue_updated["dueAt"], "2026-11-20T09:00:00Z");
        assert_eq!(issue_updated["targetRelease"], "1.1.0");
        assert!(issue_updated.get("privateNote").is_none());

        let issue_update_replay = client
            .patch(&update_issue_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-issue-update-1")
            .json(&update_issue_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(issue_update_replay.status(), StatusCode::OK);
        let issue_update_replay_json: serde_json::Value = issue_update_replay.json().await.unwrap();
        assert_eq!(issue_update_replay_json["revision"], 3);

        let assign_teammate = client
            .patch(&update_issue_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-issue-assign-teammate")
            .json(&serde_json::json!({
                "expectedRevision": 3,
                "assigneeOperatorId": teammate_operator_id.to_string()
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(assign_teammate.status(), StatusCode::OK);
        let teammate_issue: serde_json::Value = assign_teammate.json().await.unwrap();
        assert_eq!(teammate_issue["revision"], 4);
        assert_eq!(
            teammate_issue["assigneeOperatorId"],
            teammate_operator_id.to_string()
        );
        assert_eq!(teammate_issue["assigneeOperatorName"], "Morgan Support");

        let assign_inactive = client
            .patch(&update_issue_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-issue-assign-inactive")
            .json(&serde_json::json!({
                "expectedRevision": 4,
                "assigneeOperatorId": inactive_operator_id.to_string()
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(assign_inactive.status(), StatusCode::BAD_REQUEST);

        let issue_events = client
            .get(format!(
                "{active_base}/operator/v1/issues/{issue_id}/events"
            ))
            .bearer_auth("operator-token")
            .send()
            .await
            .unwrap();
        assert_eq!(issue_events.status(), StatusCode::OK);
        let issue_events_json: serde_json::Value = issue_events.json().await.unwrap();
        let issue_event_items = issue_events_json["items"].as_array().unwrap();
        assert!(issue_event_items
            .iter()
            .any(|event| event["eventKind"] == "private_note"
                && event["privateNote"] == "Coordinate with the processor team."));
        assert!(issue_event_items
            .iter()
            .any(|event| event["eventKind"] == "assignment_changed"
                && event["newAssigneeOperatorId"] == active_operator_id.to_string()));
        assert!(issue_event_items
            .iter()
            .any(|event| event["eventKind"] == "schedule_changed"
                && event["dueAt"] == "2026-11-20T09:00:00Z"));
        assert_eq!(
            issue_event_items
                .iter()
                .filter(|event| event["eventKind"] == "payment_request_linked")
                .count(),
            2
        );

        let source_check = sqlx::PgPool::connect(&seed_url).await.unwrap();
        let source_states = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT status,customer_visible_update FROM hosted_payment_integration_requests WHERE id = ANY($1)",
        )
        .bind(vec![request_id, request_two_id])
        .fetch_all(&source_check)
        .await
        .unwrap();
        assert_eq!(source_states.len(), 2);
        assert!(source_states
            .iter()
            .all(|(status, update)| status == "submitted" && update.is_none()));
        source_check.close().await;

        let (inactive_base, inactive_server) =
            serve_test_api(inactive_auth, database.clone()).await;
        let inactive = client
            .get(format!("{inactive_base}/operator/v1/me"))
            .bearer_auth("inactive-token")
            .send()
            .await
            .unwrap();
        assert_eq!(inactive.status(), StatusCode::FORBIDDEN);

        let (unprovisioned_base, unprovisioned_server) =
            serve_test_api(unprovisioned_auth, database.clone()).await;
        let unprovisioned = client
            .get(format!("{unprovisioned_base}/operator/v1/me"))
            .bearer_auth("unprovisioned-token")
            .send()
            .await
            .unwrap();
        assert_eq!(unprovisioned.status(), StatusCode::FORBIDDEN);

        let update_url =
            format!("{active_base}/operator/v1/payment-integration-requests/{request_id}");
        let payload = serde_json::json!({
            "expectedRevision": 1,
            "status": "under_review",
            "customerVisibleUpdate": "We are reviewing this integration request.",
            "privateNote": "Confirm terminal model during support review.",
            "dueAt": "2026-11-12T15:30:00Z"
        });
        let update = client
            .patch(&update_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-http-update-1")
            .json(&payload)
            .send()
            .await
            .unwrap();
        assert_eq!(update.status(), StatusCode::OK);
        let updated: serde_json::Value = update.json().await.unwrap();
        assert_eq!(updated["revision"], 2);
        assert_eq!(updated["dueAt"], "2026-11-12 15:30:00+00");
        assert!(updated.get("privateNote").is_none());

        let replay = client
            .patch(&update_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-http-update-1")
            .json(&payload)
            .send()
            .await
            .unwrap();
        assert_eq!(replay.status(), StatusCode::OK);
        let replayed: serde_json::Value = replay.json().await.unwrap();
        assert_eq!(replayed["revision"], 2);

        let clear_due_date = client
            .patch(&update_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-http-clear-due-date")
            .json(&serde_json::json!({
                "expectedRevision": 2,
                "status": "under_review",
                "clearDueAt": true
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(clear_due_date.status(), StatusCode::OK);
        let cleared: serde_json::Value = clear_due_date.json().await.unwrap();
        assert_eq!(cleared["revision"], 3);
        assert!(cleared["dueAt"].is_null());

        let invalid_due_date = client
            .patch(&update_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-http-invalid-due-date")
            .json(&serde_json::json!({
                "expectedRevision": 3,
                "status": "under_review",
                "dueAt": "next Thursday"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(invalid_due_date.status(), StatusCode::BAD_REQUEST);

        let conflicting_replay = client
            .patch(&update_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-http-update-1")
            .json(&serde_json::json!({
                "expectedRevision": 1,
                "status": "under_review",
                "customerVisibleUpdate": "A different request with the same key."
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(conflicting_replay.status(), StatusCode::CONFLICT);

        let stale_revision = client
            .patch(&update_url)
            .bearer_auth("operator-token")
            .header("Idempotency-Key", "operator-http-stale-revision")
            .json(&serde_json::json!({
                "expectedRevision": 1,
                "status": "under_review",
                "customerVisibleUpdate": "Stale revision must fail."
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(stale_revision.status(), StatusCode::CONFLICT);

        let events_url =
            format!("{active_base}/operator/v1/payment-integration-requests/{request_id}/events");
        let events = client
            .get(&events_url)
            .bearer_auth("operator-token")
            .send()
            .await
            .unwrap();
        assert_eq!(events.status(), StatusCode::OK);
        let events_json: serde_json::Value = events.json().await.unwrap();
        assert_eq!(events_json["items"].as_array().unwrap().len(), 3);
        let event_items = events_json["items"].as_array().unwrap();
        let private_event = event_items
            .iter()
            .find(|event| event["eventKind"] == "private_note")
            .unwrap();
        assert_eq!(
            private_event["privateNote"],
            "Confirm terminal model during support review."
        );
        let public_event = event_items
            .iter()
            .find(|event| event["eventKind"] == "status_changed")
            .unwrap();
        assert_eq!(
            public_event["customerVisibleUpdate"],
            "We are reviewing this integration request."
        );
        assert_eq!(public_event["dueAt"], "2026-11-12 15:30:00+00");
        assert!(public_event["privateNote"].is_null());
        let schedule_event = event_items
            .iter()
            .find(|event| event["eventKind"] == "schedule_changed")
            .unwrap();
        assert!(schedule_event["dueAt"].is_null());
        let tenant_events_rejected = client
            .get(&events_url)
            .bearer_auth("tenant-token")
            .send()
            .await
            .unwrap();
        assert_eq!(tenant_events_rejected.status(), StatusCode::UNAUTHORIZED);

        let tenant_runtime_url = std::env::var("POS_API_TEST_DATABASE_URL")
            .expect("the verifier must provide the restricted tenant runtime login URL");
        let tenant_database = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&tenant_runtime_url)
            .await
            .expect("tenant progress must use the restricted tenant runtime login");
        let tenant_issue_table_access =
            sqlx::query_scalar::<_, String>("SELECT id::text FROM operator_support_issues LIMIT 1")
                .fetch_optional(&tenant_database)
                .await;
        assert!(tenant_issue_table_access.is_err());
        let tenant_issue_link_access = sqlx::query_scalar::<_, String>(
            "SELECT request_id::text FROM operator_support_issue_payment_requests LIMIT 1",
        )
        .fetch_optional(&tenant_database)
        .await;
        assert!(tenant_issue_link_access.is_err());
        let tenant_issue_event_access = sqlx::query_scalar::<_, String>(
            "SELECT id::text FROM operator_support_issue_events LIMIT 1",
        )
        .fetch_optional(&tenant_database)
        .await;
        assert!(tenant_issue_event_access.is_err());
        let (tenant_base, tenant_server) =
            serve_tenant_test_api(tenant_auth, tenant_database.clone()).await;
        let tenant_requests_url = format!(
            "{tenant_base}/v1/tenants/{tenant_id}/branches/{branch_id}/payment-integration-requests"
        );
        let tenant_progress = client
            .get(&tenant_requests_url)
            .bearer_auth("tenant-token")
            .send()
            .await
            .unwrap();
        assert_eq!(tenant_progress.status(), StatusCode::OK);
        let tenant_progress_json: serde_json::Value = tenant_progress.json().await.unwrap();
        assert_eq!(tenant_progress_json["data"][0]["status"], "under_review");
        assert!(tenant_progress_json["data"][0].get("dueAt").is_none());
        assert_eq!(
            tenant_progress_json["data"][0]["customerVisibleUpdate"],
            "We are reviewing this integration request."
        );
        assert!(tenant_progress_json["data"][0].get("privateNote").is_none());
        assert!(tenant_progress_json["data"][0]
            .get("supportIssueId")
            .is_none());
        let (tenant_two_base, tenant_two_server) =
            serve_tenant_test_api(tenant_two_auth, tenant_database.clone()).await;
        let tenant_two_requests_url = format!(
            "{tenant_two_base}/v1/tenants/{tenant_two_id}/branches/{branch_two_id}/payment-integration-requests"
        );
        let tenant_two_progress = client
            .get(&tenant_two_requests_url)
            .bearer_auth("tenant-two-token")
            .send()
            .await
            .unwrap();
        assert_eq!(tenant_two_progress.status(), StatusCode::OK);
        let tenant_two_progress_json: serde_json::Value = tenant_two_progress.json().await.unwrap();
        assert_eq!(
            tenant_two_progress_json["data"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            tenant_two_progress_json["data"][0]["id"],
            request_two_id.to_string()
        );
        assert!(tenant_two_progress_json["data"][0]
            .get("supportIssueId")
            .is_none());
        let tenant_two_issue_queue = client
            .get(format!("{tenant_two_base}/operator/v1/issues"))
            .bearer_auth("tenant-two-token")
            .send()
            .await
            .unwrap();
        // The tenant API does not mount operator routes, so the route itself is absent.
        assert_eq!(tenant_two_issue_queue.status(), StatusCode::NOT_FOUND);
        let operator_token_rejected_by_tenant = client
            .get(&tenant_requests_url)
            .bearer_auth("operator-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            operator_token_rejected_by_tenant.status(),
            StatusCode::UNAUTHORIZED
        );

        let inspection = sqlx::PgPool::connect(&seed_url).await.unwrap();
        let audit = sqlx::query_as::<_, (i32, i32)>(
            "SELECT revision, (SELECT count(*)::int FROM operator_payment_request_events WHERE request_id=$1) FROM hosted_payment_integration_requests WHERE id=$1",
        )
        .bind(request_id)
        .fetch_one(&inspection)
        .await
        .unwrap();
        assert_eq!(audit, (3, 3));
        inspection.close().await;
        database.close().await;
        tenant_database.close().await;
        active_idp.abort();
        inactive_idp.abort();
        unprovisioned_idp.abort();
        tenant_idp.abort();
        _tenant_two_idp.abort();
        active_server.abort();
        inactive_server.abort();
        unprovisioned_server.abort();
        tenant_server.abort();
        tenant_two_server.abort();
    }
}
