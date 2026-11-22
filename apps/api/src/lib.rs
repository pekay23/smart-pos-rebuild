// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

mod attendance;
mod auth;
mod authorization;
pub(crate) mod branch_settings;
mod cash_locations;
mod catalog;
mod config;
mod credit_terms;
mod customers;
mod drawer;
mod expense_attribution;
mod expense_reimbursements;
mod expenses;
mod held_tickets;
mod hosted_images;
mod hosted_staff;
mod identity;
mod inventory_allocations;
mod migrations;
pub mod operator;
pub mod operator_provisioning;
mod overview;
mod payment_options;
mod payment_requests;
mod pickup_reservations;
mod purchasing;
mod receivables;
mod refunds;
mod sale_accounting;
mod sale_calculations;
mod sales;
mod station_activation;
mod station_capacity;
mod station_lease;
mod station_proof;
mod stock_transfers;
mod supplier_billing;
mod suppliers;
mod support_requests;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};

pub use auth::{Authenticator, VerifiedIdentity};
pub use config::Config;

pub async fn run_migrations() -> Result<(), Box<dyn std::error::Error>> {
    migrations::run_from_environment().await
}

const API_BODY_LIMIT: usize = hosted_images::MAX_IMAGE_BYTES;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceResponse {
    license: &'static str,
    source_url: &'static str,
}

pub fn app(allowed_origins: Vec<HeaderValue>) -> Router {
    app_with_auth(allowed_origins, None)
}

pub fn app_with_auth(
    allowed_origins: Vec<HeaderValue>,
    authenticator: Option<Authenticator>,
) -> Router {
    app_with_runtime(allowed_origins, authenticator, None)
}

pub fn app_with_runtime(
    allowed_origins: Vec<HeaderValue>,
    authenticator: Option<Authenticator>,
    identity_database: Option<PgPool>,
) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed_origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            header::HeaderName::from_static("idempotency-key"),
            header::HeaderName::from_static("x-station-lease-id"),
            header::HeaderName::from_static("x-station-lease-token"),
            header::HeaderName::from_static("x-station-challenge-id"),
            header::HeaderName::from_static("x-station-proof"),
        ]);

    let identity_routes = Router::new()
        .route(
            "/v1/tenants/{tenant_id}/products/{product_id}",
            patch(update_product_identity_endpoint),
        )
        .route("/v1/auth/identity", get(authenticated_identity))
        .route("/v1/me/tenants", get(authenticated_tenants))
        .route("/v1/tenants/{tenant_id}/staff", get(hosted_staff_endpoint))
        .route(
            "/v1/tenants/{tenant_id}/customers/{customer_id}/credit-terms",
            get(tenant_customer_credit_terms_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/customers/{customer_id}/credit-terms/{currency_code}/history",
            get(tenant_customer_credit_term_history_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/customers/{customer_id}/credit-terms/{currency_code}",
            put(set_tenant_customer_credit_term_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/station-capacity",
            get(tenant_station_capacity_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/overview",
            get(tenant_overview_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/permissions",
            get(authenticated_branch_permissions),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/profile",
            get(branch_profile_endpoint).put(update_branch_profile_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/attendance/me",
            get(my_attendance_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/attendance/me/clock-in",
            post(attendance_clock_in_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/attendance/me/clock-out",
            post(attendance_clock_out_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/attendance/schedules",
            get(attendance_schedules_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/attendance/staff",
            get(attendance_staff_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/attendance/schedules/{membership_id}",
            put(save_attendance_schedule_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/attendance/periods",
            get(attendance_periods_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/permissions",
            get(authenticated_station_permissions),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/products",
            get(list_branch_products_endpoint).post(create_branch_product_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/product-candidates",
            get(list_branch_catalog_candidates_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/products/{product_id}/attach",
            post(attach_product_to_branch_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/sales",
            get(branch_sales_report_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/accounting/coverage",
            get(branch_accounting_coverage_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/accounting/trial-balance",
            get(branch_trial_balance_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/accounting/income-statement",
            get(branch_income_statement_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/accounting/balance-sheet",
            get(branch_balance_sheet_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/accounting/cash-flow",
            get(branch_cash_flow_statement_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/accounting/cash-reconciliation",
            get(branch_cash_reconciliation_status_endpoint).post(branch_cash_reconciliation_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/sales/{sale_id}",
            get(branch_sale_receipt_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/customers",
            get(list_branch_customers_endpoint).post(create_branch_customer_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/customers/{customer_id}",
            patch(update_branch_customer_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/customers/{customer_id}/status",
            patch(set_branch_customer_status_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/customers/{customer_id}/merge",
            post(merge_branch_customer_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/suppliers",
            get(list_branch_suppliers_endpoint).post(create_branch_supplier_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/suppliers/{supplier_id}",
            patch(update_branch_supplier_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/suppliers/{supplier_id}/status",
            patch(set_branch_supplier_status_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/entities/{entity_type}/{entity_id}/image",
            get(get_hosted_entity_image_endpoint)
                .put(put_hosted_entity_image_endpoint)
                .delete(delete_hosted_entity_image_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/purchase-orders",
            get(list_purchase_orders_endpoint).post(create_purchase_order_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/purchase-orders/{order_id}",
            patch(update_purchase_order_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/purchase-orders/{order_id}/place",
            post(place_purchase_order_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/purchase-orders/{order_id}/cancel",
            post(cancel_purchase_order_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/purchase-orders/{order_id}/receipts",
            post(receive_purchase_order_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/supplier-billing",
            get(supplier_billing_workspace_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/supplier-bills",
            post(match_supplier_bill_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/supplier-credit-notes",
            post(create_supplier_credit_note_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/supplier-payments",
            post(record_supplier_payment_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/expenses",
            get(hosted_expenses_endpoint).post(create_hosted_expense_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/expenses/{expense_id}/reimbursements",
            get(hosted_expense_reimbursements_endpoint).post(create_hosted_expense_reimbursement_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/expenses/{expense_id}/cost-center",
            get(hosted_expense_attribution_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/expenses/{expense_id}/cost-center/reclassifications",
            post(reclassify_hosted_expense_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/cash-locations",
            get(cash_locations_endpoint).post(create_cash_location_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/cash-locations/{location_id}/opening",
            post(open_cash_location_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/cash-locations/{location_id}/events",
            get(cash_location_events_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/cash-transfers",
            post(cash_transfer_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/supplier-payments",
            post(record_station_supplier_payment_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/expenses",
            post(record_station_cash_expense_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/customers",
            get(list_station_customers_endpoint).post(create_station_customer_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/customers/{customer_id}/credit-position",
            get(station_customer_credit_position_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/customers/{customer_id}/receivables",
            get(station_customer_open_receivables_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/customer-collections",
            post(record_station_cash_collection_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/customers/{customer_id}",
            patch(update_station_customer_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/customers/{customer_id}/status",
            patch(set_station_customer_status_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/products/{product_id}",
            patch(set_branch_product_status_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/products/{product_id}/settings",
            patch(update_branch_product_settings_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/products/{product_id}/stock-adjustments",
            post(adjust_branch_stock_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/payment-options",
            get(list_branch_payment_options_endpoint)
                .post(create_branch_payment_option_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/payment-options/{option_id}",
            patch(update_branch_payment_option_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/payment-integration-requests",
            get(list_payment_integration_requests_endpoint)
                .post(create_payment_integration_request_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/support-requests",
            get(list_support_requests_endpoint).post(create_support_request_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/held-tickets",
            get(list_held_tickets_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/payment-options",
            get(list_station_payment_options_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/products",
            get(list_station_products_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/products/{product_id}/availability",
            get(product_branch_availability_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stock-transfers",
            get(list_branch_stock_transfers_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stock-transfers/{transfer_id}/events",
            post(transition_stock_transfer_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/pickup-reservations",
            get(list_branch_pickup_reservations_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/pickup-reservations/{reservation_id}/events",
            post(transition_branch_pickup_reservation_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/stock-transfers",
            get(list_station_stock_transfers_endpoint).post(request_stock_transfer_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/pickup-reservations",
            get(list_station_pickup_reservations_endpoint).post(create_pickup_reservation_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/pickup-reservations/{reservation_id}/events",
            post(transition_station_pickup_reservation_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/activation",
            post(activate_station_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/activation-challenge",
            post(create_station_activation_challenge_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/activation-revocation",
            post(revoke_station_activation_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/actor-lease-challenge",
            post(create_station_lease_challenge_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/actor-lease",
            post(acquire_station_lease_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/checkout-challenge",
            post(create_checkout_challenge_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/sales",
            get(station_sales_history_endpoint).post(commit_sale_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/held-tickets",
            post(create_held_ticket_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/held-tickets/{ticket_id}/claim",
            post(claim_held_ticket_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/held-tickets/{ticket_id}",
            put(save_held_ticket_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/held-tickets/{ticket_id}/checkout-attempt",
            post(prepare_held_ticket_checkout_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/held-tickets/{ticket_id}/checkout-attempt/clear",
            post(clear_held_ticket_checkout_attempt_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/held-tickets/{ticket_id}/release",
            post(release_held_ticket_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/held-tickets/{ticket_id}/discard",
            post(discard_held_ticket_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/sales/{sale_id}/customer",
            put(attach_sale_customer_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/sales/{sale_id}/receipt",
            get(station_sale_receipt_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/sales/{sale_id}/refunds",
            get(station_sale_refunds_endpoint).post(refund_sale_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/drawer/cash-drop-destinations",
            get(cash_drop_destinations_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/drawer-shift",
            get(get_drawer_shift_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/drawer-shifts",
            get(get_drawer_history_endpoint).post(open_drawer_shift_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/drawer-shifts/{shift_id}/movements",
            post(record_drawer_movement_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/drawer-shifts/{shift_id}/x-report",
            get(get_x_drawer_report_endpoint),
        )
        .route(
            "/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/drawer-shifts/{shift_id}/close",
            post(close_drawer_shift_endpoint),
        )
        .route(
            "/v1/station-actor-leases/{lease_id}/heartbeat",
            post(heartbeat_station_lease_endpoint),
        )
        .route(
            "/v1/station-actor-leases/{lease_id}/release",
            post(release_station_lease_endpoint),
        )
        .with_state(Arc::new(AppState {
            authenticator,
            identity_database,
        }));

    Router::new()
        .route("/healthz", get(health))
        .route("/source", get(source))
        .nest("/v1", sale_calculations::router())
        .merge(identity_routes)
        .layer(axum::extract::DefaultBodyLimit::max(API_BODY_LIMIT))
        .layer(cors)
}

#[derive(Clone)]
struct AppState {
    authenticator: Option<Authenticator>,
    identity_database: Option<PgPool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BalanceSheetQuery {
    as_of_date: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IdentityResponse {
    data: identity::InternalIdentity,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TenantMembershipsResponse {
    data: Vec<identity::TenantMembership>,
}

#[derive(Serialize)]
struct HostedStaffResponse {
    data: Vec<hosted_staff::HostedStaffMember>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StationCapacityEnvelope {
    data: station_capacity::StationCapacitySnapshot,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StationPermissionsResponse {
    data: StationPermissionSnapshot,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StationActivationRequest {
    device_public_key: String,
    challenge_id: String,
    signature: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StationActivationRevocationRequest {
    reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StationActivationChallengeResponse {
    data: StationActivationChallengeSnapshot,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StationActivationChallengeSnapshot {
    challenge_id: String,
    nonce: String,
    expires_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StationActivationResponse {
    data: StationActivationSnapshot,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StationActivationSnapshot {
    tenant_id: String,
    branch_id: String,
    station_id: String,
    activation_id: String,
    active_station_count: i32,
    station_limit: i32,
    created: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StationActivationRevocationResponse {
    data: StationActivationRevocationSnapshot,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StationActivationRevocationSnapshot {
    tenant_id: String,
    branch_id: String,
    station_id: String,
    activation_id: String,
    active_station_count: i32,
    revoked_at: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StationLeaseCommand {
    token: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StationLeaseAcquireRequest {
    challenge_id: String,
    signature: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StationLeaseResponse {
    data: StationLeaseSnapshot,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StationLeaseSnapshot {
    lease_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
    expires_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StationLeaseChallengeResponse {
    data: StationLeaseChallengeSnapshot,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StationLeaseChallengeSnapshot {
    challenge_id: String,
    nonce: String,
    expires_at: String,
    message_to_sign: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BranchProductsResponse {
    data: Vec<catalog::BranchProduct>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HeldTicketCreateRequest {
    payload: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HeldTicketRevisionRequest {
    expected_revision: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HeldTicketSaveRequest {
    expected_revision: i64,
    payload: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HeldTicketCheckoutRequest {
    expected_revision: i64,
    body: serde_json::Value,
    idempotency_key: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HeldTicketsResponse {
    data: Vec<held_tickets::HeldTicket>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HeldTicketResponse {
    data: held_tickets::HeldTicket,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BranchCatalogCandidatesResponse {
    data: Vec<catalog::CatalogProductIdentity>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BranchProductResponse {
    data: catalog::BranchProduct,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogProductIdentityResponse {
    data: catalog::CatalogProductIdentity,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StockAdjustmentResponse {
    data: catalog::StockAdjustmentResult,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProductAvailabilityResponse {
    data: Vec<catalog::ProductBranchAvailability>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PaymentOptionsResponse {
    data: Vec<payment_options::PaymentOption>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PaymentOptionResponse {
    data: payment_options::PaymentOption,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CheckoutChallengeRequest {
    payload_hash: String,
    #[serde(default = "default_sale_operation")]
    operation: String,
    resource_id: Option<String>,
}

fn default_sale_operation() -> String {
    "sale.commit".to_owned()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckoutChallengeResponse {
    data: station_proof::CheckoutChallenge,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StationPermissionSnapshot {
    tenant_id: String,
    branch_id: String,
    station_id: String,
    permissions: Vec<&'static str>,
    tenant_permissions: Vec<&'static str>,
    branch_permissions: Vec<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BranchPermissionsResponse {
    data: BranchPermissionSnapshot,
}

#[derive(Serialize)]
struct BranchProfileResponse {
    data: branch_settings::BranchProfile,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BranchPermissionSnapshot {
    tenant_id: String,
    branch_id: String,
    permissions: Vec<&'static str>,
    tenant_permissions: Vec<&'static str>,
}

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ApiError,
}

#[derive(Serialize)]
struct ApiError {
    code: &'static str,
    message: &'static str,
}

async fn authenticated_identity(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    match resolve_identity(&state, &headers).await {
        Ok(record) => Json(IdentityResponse { data: record }).into_response(),
        Err(response) => response,
    }
}

async fn authenticated_tenants(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let identity = match resolve_identity(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(identity_database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match identity::list_tenant_memberships(identity_database, &identity.identity_id).await {
        Ok(data) => Json(TenantMembershipsResponse { data }).into_response(),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "tenant_memberships_unavailable",
            "Business access could not be loaded right now.",
        ),
    }
}

async fn hosted_staff_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(tenant_id): Path<String>,
) -> Response {
    let (identity, membership) =
        match authorize_tenant_catalog_scope(&state, &headers, &tenant_id, "users:write").await {
            Ok(value) => value,
            Err(response) => return response,
        };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match hosted_staff::list(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
    )
    .await
    {
        Ok(data) => Json(HostedStaffResponse { data }).into_response(),
        Err(()) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "staff_directory_unavailable",
            "The staff directory could not be loaded right now.",
        ),
    }
}

async fn tenant_station_capacity_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(tenant_id): Path<String>,
) -> Response {
    let identity = match resolve_identity(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Station capacity could not be loaded right now.",
        );
    };
    let memberships = match identity::list_tenant_memberships(database, &identity.identity_id).await
    {
        Ok(memberships) => memberships,
        Err(_) => {
            return auth_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "tenant_memberships_unavailable",
                "Business access could not be loaded right now.",
            );
        }
    };
    let Some(membership) = memberships.iter().find(|item| item.tenant_id == tenant_id) else {
        return auth_error(
            StatusCode::NOT_FOUND,
            "station_capacity_not_found",
            "Station capacity is not available to this account.",
        );
    };
    if !authorization::has_permission(
        &membership.role_grants,
        "license:manage",
        authorization::ResourceScope::Tenant,
    ) {
        return auth_error(
            StatusCode::FORBIDDEN,
            "station_capacity_permission_denied",
            "Only the business owner can view licensed station capacity.",
        );
    }

    match station_capacity::load(database, &identity.identity_id, &tenant_id).await {
        Ok(data) => Json(StationCapacityEnvelope { data }).into_response(),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "station_capacity_unavailable",
            "Station capacity could not be loaded right now.",
        ),
    }
}

async fn tenant_overview_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(tenant_id): Path<String>,
    query: Result<Query<overview::OverviewQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_overview_query",
                "Overview page values are invalid.",
            );
        }
    };
    let (offset, page_size) = match query.page() {
        Ok(page) => page,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_overview_query",
                "Overview offset must be at most 10000 and page size must be between 1 and 50.",
            );
        }
    };
    let identity = match resolve_identity(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Business overview is unavailable right now.",
        );
    };
    let memberships = match identity::list_tenant_memberships(database, &identity.identity_id).await
    {
        Ok(memberships) => memberships,
        Err(_) => {
            return auth_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "tenant_memberships_unavailable",
                "Business access could not be loaded right now.",
            );
        }
    };
    let Some(membership) = memberships.iter().find(|item| item.tenant_id == tenant_id) else {
        return auth_error(
            StatusCode::NOT_FOUND,
            "tenant_overview_not_found",
            "Business overview is not available to this account.",
        );
    };
    match overview::load(
        database,
        &identity.identity_id,
        membership,
        offset,
        page_size,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({ "data": data })).into_response(),
        Err(overview::OverviewError::InvalidPage) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_overview_query",
            "Overview page values are invalid.",
        ),
        Err(overview::OverviewError::PermissionDenied) => auth_error(
            StatusCode::FORBIDDEN,
            "tenant_overview_permission_denied",
            "No branch overview permissions are assigned to this account.",
        ),
        Err(overview::OverviewError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "tenant_overview_unavailable",
            "Business overview could not be loaded right now.",
        ),
    }
}

async fn authenticated_station_permissions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
) -> Response {
    let identity = match resolve_identity(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(identity_database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    let memberships =
        match identity::list_tenant_memberships(identity_database, &identity.identity_id).await {
            Ok(memberships) => memberships,
            Err(_) => {
                return auth_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "tenant_memberships_unavailable",
                    "Business access could not be loaded right now.",
                );
            }
        };
    let Some(membership) = memberships.iter().find(|item| item.tenant_id == tenant_id) else {
        return auth_error(
            StatusCode::NOT_FOUND,
            "station_access_not_found",
            "The requested station is not available to this account.",
        );
    };
    let Some(branch) = membership
        .branches
        .iter()
        .find(|item| item.branch_id == branch_id)
    else {
        return auth_error(
            StatusCode::NOT_FOUND,
            "station_access_not_found",
            "The requested station is not available to this account.",
        );
    };
    if !branch
        .stations
        .iter()
        .any(|item| item.station_id == station_id)
    {
        return auth_error(
            StatusCode::NOT_FOUND,
            "station_access_not_found",
            "The requested station is not available to this account.",
        );
    }

    let scope = authorization::ResourceScope::Station {
        branch_id: &branch_id,
        station_id: &station_id,
    };
    if !authorization::has_permission(&membership.role_grants, "pos:use", scope) {
        return auth_error(
            StatusCode::FORBIDDEN,
            "station_permission_denied",
            "This account does not have permission to use the requested station.",
        );
    }
    let permissions = authorization::permissions_for(&membership.role_grants, scope);
    let tenant_permissions = authorization::permissions_for(
        &membership.role_grants,
        authorization::ResourceScope::Tenant,
    );
    let branch_permissions = authorization::permissions_for(
        &membership.role_grants,
        authorization::ResourceScope::Branch {
            branch_id: &branch_id,
        },
    );

    Json(StationPermissionsResponse {
        data: StationPermissionSnapshot {
            tenant_id,
            branch_id,
            station_id,
            permissions,
            tenant_permissions,
            branch_permissions,
        },
    })
    .into_response()
}

async fn authorize_hosted_image_scope(
    state: &AppState,
    headers: &HeaderMap,
    tenant_id: &str,
    branch_id: &str,
    raw_entity_type: &str,
    write: bool,
) -> Result<
    (
        hosted_images::EntityImageType,
        identity::InternalIdentity,
        identity::TenantMembership,
    ),
    Response,
> {
    let entity_type = hosted_images::EntityImageType::parse(raw_entity_type).map_err(|_| {
        auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_image_entity",
            "Choose a product, customer, supplier, or staff image.",
        )
    })?;
    let permission = match (entity_type, write) {
        (hosted_images::EntityImageType::Product, true) => "products:write",
        (hosted_images::EntityImageType::Product, false) => "products:read",
        (hosted_images::EntityImageType::Customer, true) => "customers:manage",
        (hosted_images::EntityImageType::Customer, false) => "customers:read",
        (hosted_images::EntityImageType::Supplier, true) => "suppliers:write",
        (hosted_images::EntityImageType::Supplier, false) => "suppliers:read",
        (hosted_images::EntityImageType::Staff, true) => "users:write",
        (hosted_images::EntityImageType::Staff, false) => "users:write",
    };
    let (identity, membership) = if entity_type == hosted_images::EntityImageType::Staff {
        let (identity, membership) =
            authorize_tenant_catalog_scope(state, headers, tenant_id, permission).await?;
        if !membership
            .branches
            .iter()
            .any(|branch| branch.branch_id == branch_id)
        {
            return Err(auth_error(
                StatusCode::NOT_FOUND,
                "catalog_access_not_found",
                "The requested branch catalog is unavailable.",
            ));
        }
        (identity, membership)
    } else {
        authorize_catalog_scope(state, headers, tenant_id, branch_id, None, permission).await?
    };
    Ok((entity_type, identity, membership))
}

fn hosted_image_error(error: hosted_images::ImageError) -> Response {
    match error {
        hosted_images::ImageError::InvalidInput => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_image",
            "Use a valid PNG, JPEG, or WebP image no larger than 2 MiB.",
        ),
        hosted_images::ImageError::NotFound => auth_error(
            StatusCode::NOT_FOUND,
            "image_entity_not_found",
            "The requested business record was not found.",
        ),
        hosted_images::ImageError::Database => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "image_storage_unavailable",
            "The image could not be loaded or saved right now.",
        ),
    }
}

async fn get_hosted_entity_image_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, entity_type, entity_id)): Path<(String, String, String, String)>,
) -> Response {
    let (entity_type, identity, membership) = match authorize_hosted_image_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        &entity_type,
        false,
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Image storage is not configured.",
        );
    };
    match hosted_images::get(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        entity_type,
        &entity_id,
    )
    .await
    {
        Ok(Some(image)) => {
            let mut response = Response::new(axum::body::Body::from(image.bytes));
            *response.status_mut() = StatusCode::OK;
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(image.content_type),
            );
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-store"),
            );
            response.headers_mut().insert(
                header::HeaderName::from_static("x-content-type-options"),
                HeaderValue::from_static("nosniff"),
            );
            response
        }
        Ok(None) => auth_error(
            StatusCode::NOT_FOUND,
            "image_not_found",
            "No image is stored for this record.",
        ),
        Err(error) => hosted_image_error(error),
    }
}

async fn put_hosted_entity_image_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, entity_type, entity_id)): Path<(String, String, String, String)>,
    body: Bytes,
) -> Response {
    if body.len() > hosted_images::MAX_IMAGE_BYTES {
        return hosted_image_error(hosted_images::ImageError::InvalidInput);
    }
    let (entity_type, identity, membership) = match authorize_hosted_image_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        &entity_type,
        true,
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(content_type) = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    else {
        return hosted_image_error(hosted_images::ImageError::InvalidInput);
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Image storage is not configured.",
        );
    };
    match hosted_images::put(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        entity_type,
        &entity_id,
        content_type,
        &body,
    )
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => hosted_image_error(error),
    }
}

async fn delete_hosted_entity_image_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, entity_type, entity_id)): Path<(String, String, String, String)>,
) -> Response {
    let (entity_type, identity, membership) = match authorize_hosted_image_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        &entity_type,
        true,
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Image storage is not configured.",
        );
    };
    match hosted_images::delete(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        entity_type,
        &entity_id,
    )
    .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => hosted_image_error(error),
    }
}

async fn list_branch_products_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "products:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match catalog::list_branch_products(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        None,
    )
    .await
    {
        Ok(data) => Json(BranchProductsResponse { data }).into_response(),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "catalog_unavailable",
            "The branch product catalog could not be loaded right now.",
        ),
    }
}

async fn list_branch_catalog_candidates_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "products:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match catalog::list_branch_catalog_candidates(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
    )
    .await
    {
        Ok(data) => Json(BranchCatalogCandidatesResponse { data }).into_response(),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "catalog_unavailable",
            "Products available to add to this branch could not be loaded right now.",
        ),
    }
}

async fn attach_product_to_branch_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, product_id)): Path<(String, String, String)>,
    Json(input): Json<catalog::AttachProductInput>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "products:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match catalog::attach_product_to_branch(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &product_id,
        input,
    )
    .await
    {
        Ok(data) => (StatusCode::CREATED, Json(BranchProductResponse { data })).into_response(),
        Err(catalog::CatalogError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_branch_product",
            "Branch price, cost, tax, stock and alert quantities are invalid.",
        ),
        Err(catalog::CatalogError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "product_already_in_branch",
            "This product is already assigned to the branch. Refresh the catalog.",
        ),
        Err(catalog::CatalogError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "catalog_product_not_found",
            "This active product is not available in the business catalog.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "catalog_unavailable",
            "The product could not be added to the branch right now.",
        ),
    }
}

async fn branch_sales_report_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    Query(query): Query<sales::SalesReportQuery>,
) -> Response {
    let (identity, membership) =
        match authorize_catalog_scope(&state, &headers, &tenant_id, &branch_id, None, "sales:read")
            .await
        {
            Ok(value) => value,
            Err(response) => return response,
        };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match sales::branch_report(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        query,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(sales::SaleError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_sales_report_query",
            "Use valid ISO dates with an exclusive end date, an offset up to 50000 and a page size from 1 to 100.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "sales_report_unavailable",
            "The branch sales report could not be loaded right now.",
        ),
    }
}

async fn branch_accounting_coverage_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    Query(query): Query<sales::SalesReportQuery>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "reports:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match sales::branch_accounting_coverage(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        query,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(sales::SaleError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_accounting_coverage_query",
            "Use valid ISO dates with an exclusive end date.",
        ),
        Err(sales::SaleError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "branch_not_found",
            "The requested branch is unavailable.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "accounting_coverage_unavailable",
            "Branch accounting coverage could not be loaded right now.",
        ),
    }
}

async fn branch_trial_balance_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    Query(query): Query<sales::SalesReportQuery>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "reports:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match sales::branch_trial_balance(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        query,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(sales::SaleError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_trial_balance_query",
            "Use valid ISO dates with an exclusive end date.",
        ),
        Err(sales::SaleError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "branch_not_found",
            "The requested branch is unavailable.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "trial_balance_unavailable",
            "Branch trial balance could not be loaded right now.",
        ),
    }
}

async fn branch_income_statement_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    Query(query): Query<sales::SalesReportQuery>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "reports:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match sales::branch_income_statement(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        query,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(sales::SaleError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_income_statement_query",
            "Use valid ISO dates with an exclusive end date.",
        ),
        Err(sales::SaleError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "branch_not_found",
            "The requested branch is unavailable.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "income_statement_unavailable",
            "Branch income statement could not be loaded right now.",
        ),
    }
}

async fn branch_balance_sheet_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    Query(query): Query<BalanceSheetQuery>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "reports:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match sales::branch_balance_sheet(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        query.as_of_date,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(sales::SaleError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_balance_sheet_query",
            "Use a valid ISO asOfDate in the branch timezone.",
        ),
        Err(sales::SaleError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "branch_not_found",
            "The requested branch is unavailable.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "balance_sheet_unavailable",
            "Branch balance sheet could not be loaded right now.",
        ),
    }
}

async fn branch_cash_flow_statement_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    Query(query): Query<sales::SalesReportQuery>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "reports:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match sales::branch_cash_flow_statement(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        query,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(sales::SaleError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_cash_flow_query",
            "Use a valid fromDate and an exclusive toDate in the branch timezone.",
        ),
        Err(sales::SaleError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "branch_not_found",
            "The requested branch is unavailable.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "cash_flow_unavailable",
            "Branch cash flow could not be loaded right now.",
        ),
    }
}

async fn station_sales_history_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
    Query(query): Query<sales::StationSalesHistoryQuery>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "pos:use",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match sales::station_sales_history(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &station_id,
        query,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(sales::SaleError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_station_sales_history_query",
            "Search must be at most 120 characters; offset must be at most 50000 and page size from 1 to 100.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "station_sales_history_unavailable",
            "Station sales history could not be loaded right now.",
        ),
    }
}

async fn branch_sale_receipt_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, sale_id)): Path<(String, String, String)>,
) -> Response {
    let (identity, membership) =
        match authorize_catalog_scope(&state, &headers, &tenant_id, &branch_id, None, "sales:read")
            .await
        {
            Ok(value) => value,
            Err(response) => return response,
        };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match sales::receipt(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        None,
        &sale_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(sales::SaleError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_sale_id",
            "The receipt ID must be a UUID.",
        ),
        Err(sales::SaleError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "sale_not_found",
            "The receipt is unavailable in this branch.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "sale_receipt_unavailable",
            "The receipt could not be loaded right now.",
        ),
    }
}

async fn station_sale_receipt_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, sale_id)): Path<(String, String, String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "pos:use",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match sales::receipt(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        Some(&station_id),
        &sale_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(sales::SaleError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_sale_id",
            "The receipt ID must be a UUID.",
        ),
        Err(sales::SaleError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "sale_not_found",
            "The receipt is unavailable at this station.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "sale_receipt_unavailable",
            "The receipt could not be loaded right now.",
        ),
    }
}

async fn station_sale_refunds_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, sale_id)): Path<(String, String, String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "pos:use",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match refunds::detail(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &station_id,
        &sale_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(refunds::RefundError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_sale_id",
            "The sale ID must be a UUID.",
        ),
        Err(refunds::RefundError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "sale_not_found",
            "The sale is unavailable at this station.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "sale_refunds_unavailable",
            "Refund details could not be loaded right now.",
        ),
    }
}

async fn refund_sale_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, sale_id)): Path<(String, String, String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "sales:refund",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    let header_value = |name: &'static str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
    };
    let (Some(lease_id), Some(lease_token), Some(idempotency_key), Some(challenge_id), Some(proof)) = (
        header_value("x-station-lease-id"),
        header_value("x-station-lease-token"),
        header_value("idempotency-key"),
        header_value("x-station-challenge-id"),
        header_value("x-station-proof"),
    ) else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "refund_proof_headers_required",
            "Refunds require the active cashier lease, idempotency key, challenge ID, and station signature.",
        );
    };
    let input: refunds::RefundInput = match serde_json::from_slice(&body) {
        Ok(input) => input,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_refund_request",
                "The refund request must match the supported schema.",
            );
        }
    };
    match refunds::refund(
        database,
        &identity.identity_id,
        identity.display_name.as_deref().unwrap_or("Cashier"),
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &station_id,
        &sale_id,
        lease_id,
        lease_token,
        idempotency_key,
        challenge_id,
        proof,
        &body,
        input,
    )
    .await
    {
        Ok(data) => (StatusCode::CREATED, Json(serde_json::json!({"data":data}))).into_response(),
        Err(refunds::RefundError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_refund_request",
            "Refund lines, quantities, reason, method, or reference are invalid.",
        ),
        Err(refunds::RefundError::InvalidProof) => auth_error(
            StatusCode::CONFLICT,
            "invalid_refund_proof",
            "The station refund proof is invalid, expired, or already used.",
        ),
        Err(refunds::RefundError::LeaseUnavailable) => auth_error(
            StatusCode::CONFLICT,
            "station_lease_unavailable",
            "The active cashier lease is unavailable.",
        ),
        Err(refunds::RefundError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "sale_not_found",
            "The sale is unavailable at this station.",
        ),
        Err(refunds::RefundError::SaleNotRefundable) => auth_error(
            StatusCode::CONFLICT,
            "sale_not_refundable",
            "This sale cannot be refunded in its current state.",
        ),
        Err(refunds::RefundError::IdempotencyConflict) => auth_error(
            StatusCode::CONFLICT,
            "refund_idempotency_conflict",
            "This refund key was already used for a different request.",
        ),
        Err(refunds::RefundError::DrawerRequired) => auth_error(
            StatusCode::CONFLICT,
            "cash_drawer_required",
            "Open the station drawer before issuing a cash refund.",
        ),
        Err(refunds::RefundError::RegisterCutoverRequired) => auth_error(
            StatusCode::CONFLICT,
            "register_cash_cutover_required",
            "A manager must record this register's physical opening count before cash refunds can be issued.",
        ),
        Err(refunds::RefundError::DrawerCashInsufficient) => auth_error(
            StatusCode::CONFLICT,
            "cash_drawer_insufficient",
            "The drawer does not contain enough expected cash for this refund.",
        ),
        Err(refunds::RefundError::StockOverflow) => auth_error(
            StatusCode::CONFLICT,
            "stock_limit_reached",
            "Returned stock would exceed the supported quantity.",
        ),
        Err(refunds::RefundError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "refund_unavailable",
            "The refund could not be recorded right now.",
        ),
    }
}

async fn update_product_identity_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, product_id)): Path<(String, String)>,
    Json(input): Json<catalog::ProductIdentityInput>,
) -> Response {
    let (identity, membership) = match authorize_tenant_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        "products:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match catalog::update_product_identity(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &product_id,
        input,
    )
    .await
    {
        Ok(data) => Json(CatalogProductIdentityResponse { data }).into_response(),
        Err(catalog::CatalogError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_product_identity",
            "SKU, barcode, name or product description is invalid.",
        ),
        Err(catalog::CatalogError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "product_identity_conflict",
            "The product changed or its SKU/barcode is already used. Refresh the catalog and retry.",
        ),
        Err(catalog::CatalogError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "catalog_product_not_found",
            "The product is not in this business catalog.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "catalog_unavailable",
            "Product identity could not be updated right now.",
        ),
    }
}

async fn list_station_products_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "products:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match catalog::list_branch_products(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        Some(&station_id),
    )
    .await
    {
        Ok(data) => Json(BranchProductsResponse { data }).into_response(),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "catalog_unavailable",
            "The register product catalog could not be loaded right now.",
        ),
    }
}

async fn product_branch_availability_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, product_id)): Path<(String, String, String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "inventory:availability:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match catalog::product_branch_availability(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &station_id,
        &product_id,
    )
    .await
    {
        Ok(data) => Json(ProductAvailabilityResponse { data }).into_response(),
        Err(catalog::CatalogError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "catalog_product_not_found",
            "The product is not available in this register's catalog.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "inventory_availability_unavailable",
            "Stock availability across branches could not be loaded right now.",
        ),
    }
}

async fn list_branch_stock_transfers_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "inventory:transfer:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Branch stock transfers could not be loaded right now.",
        );
    };
    match stock_transfers::list(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        None,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({ "data": data })).into_response(),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "stock_transfers_unavailable",
            "Branch stock transfers could not be loaded right now.",
        ),
    }
}

async fn list_branch_suppliers_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    query: Result<Query<suppliers::SupplierListQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Query(query) = match query {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_supplier_query",
                "Supplier search or page values are invalid.",
            );
        }
    };
    let permission = if query.include_archived.unwrap_or(false) {
        "suppliers:write"
    } else {
        "suppliers:read"
    };
    let (identity, membership) =
        match authorize_catalog_scope(&state, &headers, &tenant_id, &branch_id, None, permission)
            .await
        {
            Ok(value) => value,
            Err(response) => return response,
        };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Suppliers could not be loaded right now.",
        );
    };
    match suppliers::list(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        query,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(suppliers::SupplierError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_supplier_query",
            "Supplier search or page values are invalid.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "suppliers_unavailable",
            "Suppliers could not be loaded right now.",
        ),
    }
}

async fn list_purchase_orders_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "purchases:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Purchase orders could not be loaded right now.",
        );
    };
    match purchasing::list(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "purchases_unavailable",
            "Purchase orders could not be loaded right now.",
        ),
    }
}

async fn create_purchase_order_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "purchases:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input = match serde_json::from_slice::<purchasing::PurchaseOrderInput>(&body) {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_purchase_order",
                "Provide a supplier, optional reference and valid product lines.",
            );
        }
    };
    let Some(key) = headers.get("idempotency-key").and_then(|v| v.to_str().ok()) else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_purchase_order_key",
            "A visible ASCII Idempotency-Key of at most 128 characters is required.",
        );
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The purchase order could not be saved right now.",
        );
    };
    match purchasing::save(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        key,
        &body,
        input,
    )
    .await
    {
        Ok(data) => (StatusCode::CREATED, Json(serde_json::json!({"data":data}))).into_response(),
        Err(purchasing::PurchaseError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_purchase_order",
            "Purchase order fields or lines are invalid.",
        ),
        Err(purchasing::PurchaseError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "purchase_order_reference_not_found",
            "The selected supplier or active branch product is unavailable.",
        ),
        Err(purchasing::PurchaseError::IdempotencyConflict) => auth_error(
            StatusCode::CONFLICT,
            "purchase_order_request_conflict",
            "This request key was already used with different purchase-order details.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "purchases_unavailable",
            "The purchase order could not be saved right now.",
        ),
    }
}

async fn supplier_billing_workspace_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "purchases:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Supplier billing could not be loaded right now.",
        );
    };
    match supplier_billing::workspace(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "supplier_billing_unavailable",
            "Supplier billing could not be loaded right now.",
        ),
    }
}

async fn cash_locations_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "cash:locations:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Cash locations are unavailable right now.",
        );
    };
    match cash_locations::workspace(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "cash_locations_unavailable",
            "Cash locations are unavailable right now.",
        ),
    }
}

async fn branch_cash_reconciliation_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "accounting:close",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(key) = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
    else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "idempotency_key_required",
            "Provide an idempotency key for this reconciliation.",
        );
    };
    let input = match serde_json::from_slice::<cash_locations::ReconciliationInput>(&body) {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_cash_reconciliation",
                "Provide a short reason for the cash cutover.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Cash reconciliation is unavailable right now.",
        );
    };
    match cash_locations::reconcile(database, &identity.identity_id, &tenant_id, &membership.membership_id, &branch_id, key, &body, input).await {
        Ok(result) => (StatusCode::CREATED, Json(serde_json::json!({"data":{"reconciliation":result.value,"replayed":result.replayed}}))).into_response(),
        Err(error) => cash_location_error(error, "Cash reconciliation could not be completed right now."),
    }
}

async fn branch_cash_reconciliation_status_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "reports:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Cash reconciliation is unavailable right now.",
        );
    };
    match cash_locations::reconciliation_status(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "cash_reconciliation_unavailable",
            "Cash reconciliation status could not be loaded right now.",
        ),
    }
}

async fn create_cash_location_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "cash:locations:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(key) = headers.get("idempotency-key").and_then(|v| v.to_str().ok()) else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "idempotency_key_required",
            "Provide an idempotency key for this cash location.",
        );
    };
    let input = match serde_json::from_slice::<cash_locations::CreateLocationInput>(&body) {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_cash_location",
                "Provide a valid name and location type.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The cash location could not be created right now.",
        );
    };
    match cash_locations::create_location(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        key,
        &body,
        input,
    )
    .await
    {
        Ok(result) => {
            Json(serde_json::json!({"data":{"location":result.value,"replayed":result.replayed}}))
                .into_response()
        }
        Err(error) => {
            cash_location_error(error, "The cash location could not be created right now.")
        }
    }
}

async fn open_cash_location_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, location_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "cash:locations:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(key) = headers.get("idempotency-key").and_then(|v| v.to_str().ok()) else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "idempotency_key_required",
            "Provide an idempotency key for the opening count.",
        );
    };
    let input = match serde_json::from_slice::<cash_locations::OpeningInput>(&body) {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_cash_opening",
                "Provide a non-negative opening count in minor currency units.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The cash location could not be opened right now.",
        );
    };
    match cash_locations::record_opening(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &location_id,
        key,
        &body,
        input,
    )
    .await
    {
        Ok(result) => {
            Json(serde_json::json!({"data":{"location":result.value,"replayed":result.replayed}}))
                .into_response()
        }
        Err(error) => {
            cash_location_error(error, "The opening count could not be recorded right now.")
        }
    }
}

async fn cash_location_events_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, location_id)): Path<(String, String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "cash:locations:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Cash history is unavailable right now.",
        );
    };
    match cash_locations::events(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &location_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(error) => cash_location_error(error, "Cash history is unavailable right now."),
    }
}

async fn cash_transfer_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "cash:locations:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(key) = headers.get("idempotency-key").and_then(|v| v.to_str().ok()) else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "idempotency_key_required",
            "Provide an idempotency key for this transfer.",
        );
    };
    let input = match serde_json::from_slice::<cash_locations::TransferInput>(&body) {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_cash_transfer",
                "Provide two locations, a positive amount and a note.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The cash transfer could not be recorded right now.",
        );
    };
    match cash_locations::transfer(database,&identity.identity_id,&tenant_id,&membership.membership_id,&branch_id,key,&body,input).await {
        Ok(result)=>Json(serde_json::json!({"data":{"id":result.value.to_string(),"replayed":result.replayed},"reconciliation":{"status":"incomplete"}})).into_response(),
        Err(error)=>cash_location_error(error,"The cash transfer could not be recorded right now."),
    }
}

fn cash_location_error(
    error: cash_locations::CashLocationError,
    message: &'static str,
) -> Response {
    match error {
        cash_locations::CashLocationError::InvalidInput => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_cash_location_request",
            "The cash location request is invalid.",
        ),
        cash_locations::CashLocationError::NotFound => auth_error(
            StatusCode::NOT_FOUND,
            "cash_location_not_found",
            "The cash location is unavailable.",
        ),
        cash_locations::CashLocationError::Conflict => auth_error(
            StatusCode::CONFLICT,
            "cash_location_conflict",
            "The cash location changed or this request key was already used with different details.",
        ),
        cash_locations::CashLocationError::InsufficientFunds => auth_error(
            StatusCode::CONFLICT,
            "cash_location_insufficient_funds",
            "The source location does not have enough tracked cash.",
        ),
        cash_locations::CashLocationError::CutoverBlocked => auth_error(
            StatusCode::CONFLICT,
            "cash_cutover_not_ready",
            "Add opening counts for every active cash location and close every drawer shift before reconciling.",
        ),
        cash_locations::CashLocationError::Database => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "cash_locations_unavailable",
            message,
        ),
    }
}

async fn hosted_expenses_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    Query(query): Query<expenses::ExpenseQuery>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "expenses:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let (offset, page_size) = match query.page() {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_expense_page",
                "Expense page offset and size are out of range.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Expenses could not be loaded right now.",
        );
    };
    match expenses::workspace(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        offset,
        page_size,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(expenses::ExpenseError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_expense_page",
            "Expense page offset and size are out of range.",
        ),
        Err(expenses::ExpenseError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "expense_branch_not_found",
            "The branch is unavailable.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "expenses_unavailable",
            "Expenses could not be loaded right now.",
        ),
    }
}

async fn create_hosted_expense_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "expenses:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(key) = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
    else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_expense_key",
            "A visible ASCII Idempotency-Key of at most 128 characters is required.",
        );
    };
    let input = match serde_json::from_slice::<expenses::ExpenseInput>(&body) {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_expense",
                "Expense category, description, amount, payment method or date is invalid.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The expense could not be recorded right now.",
        );
    };
    match expenses::create(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        key,
        &body,
        input,
        None,
    )
    .await
    {
        Ok(result) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"data":result})),
        )
            .into_response(),
        Err(expenses::ExpenseError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_expense",
            "Expense category, description, amount, non-cash payment method or date is invalid.",
        ),
        Err(expenses::ExpenseError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "expense_branch_not_found",
            "The branch is unavailable.",
        ),
        Err(expenses::ExpenseError::IdempotencyConflict) => auth_error(
            StatusCode::CONFLICT,
            "expense_request_conflict",
            "This request key was already used with different expense details.",
        ),
        Err(expenses::ExpenseError::CashLocationUnavailable) => auth_error(
            StatusCode::NOT_FOUND,
            "expense_cash_location_unavailable",
            "Choose an active, initialized safe or petty-cash location in this branch.",
        ),
        Err(
            expenses::ExpenseError::CashLocationInsufficientFunds
            | expenses::ExpenseError::CashLocationConflict,
        ) => auth_error(
            StatusCode::CONFLICT,
            "expense_cash_location_conflict",
            "The cash location changed or does not have enough tracked cash.",
        ),
        Err(
            expenses::ExpenseError::Database
            | expenses::ExpenseError::DrawerProof
            | expenses::ExpenseError::DrawerLease
            | expenses::ExpenseError::DrawerConflict,
        ) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "expense_unavailable",
            "The expense could not be recorded right now.",
        ),
    }
}

async fn hosted_expense_attribution_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, expense_id)): Path<(String, String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "expenses:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Expense attribution history is unavailable right now.",
        );
    };
    match expense_attribution::history(database, &identity.identity_id, &tenant_id, &membership.membership_id, &branch_id, &expense_id).await {
        Ok(history) => Json(serde_json::json!({"data":history,"effectiveAt":"original expense and reimbursement dates"})).into_response(),
        Err(expense_attribution::AttributionError::InvalidInput) => auth_error(StatusCode::BAD_REQUEST, "invalid_expense_id", "The expense identifier is invalid."),
        Err(expense_attribution::AttributionError::NotFound) => auth_error(StatusCode::NOT_FOUND, "expense_not_found", "The expense is unavailable in this branch."),
        Err(_) => auth_error(StatusCode::SERVICE_UNAVAILABLE, "expense_attribution_unavailable", "Expense attribution history could not be loaded right now."),
    }
}

async fn reclassify_hosted_expense_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, expense_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "expenses:reclassify",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(key) = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
    else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_expense_attribution_key",
            "A visible ASCII Idempotency-Key of at most 128 characters is required.",
        );
    };
    let input = match serde_json::from_slice::<expense_attribution::ReclassifyInput>(&body) {
        Ok(input) => input,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_expense_attribution",
                "Choose a cost center and provide a reason for the correction.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Expense attribution could not be corrected right now.",
        );
    };
    match expense_attribution::reclassify(database, &identity.identity_id, &tenant_id, &membership.membership_id, &branch_id, &expense_id, key, &body, input).await {
        Ok(result) => (StatusCode::CREATED, Json(serde_json::json!({"data":result,"effectiveAt":"original expense and reimbursement dates"}))).into_response(),
        Err(expense_attribution::AttributionError::InvalidInput) => auth_error(StatusCode::BAD_REQUEST, "invalid_expense_attribution", "Choose a different cost center and provide a reason between 5 and 500 characters."),
        Err(expense_attribution::AttributionError::NotFound) => auth_error(StatusCode::NOT_FOUND, "expense_or_station_not_found", "The expense or active station is unavailable in this branch."),
        Err(expense_attribution::AttributionError::Conflict) => auth_error(StatusCode::CONFLICT, "expense_attribution_conflict", "This correction key was used with different details or the expense attribution changed. Refresh and try again."),
        Err(expense_attribution::AttributionError::Database) => auth_error(StatusCode::SERVICE_UNAVAILABLE, "expense_attribution_unavailable", "Expense attribution could not be corrected right now."),
    }
}

async fn hosted_expense_reimbursements_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, expense_id)): Path<(String, String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "expenses:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Expense reimbursements could not be loaded right now.",
        );
    };
    match expense_reimbursements::workspace(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &expense_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(expense_reimbursements::ReimbursementError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_expense_id",
            "The expense identifier is invalid.",
        ),
        Err(expense_reimbursements::ReimbursementError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "expense_not_found",
            "The expense is unavailable in this branch.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "expense_reimbursements_unavailable",
            "Expense reimbursements could not be loaded right now.",
        ),
    }
}

async fn create_hosted_expense_reimbursement_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, expense_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "expenses:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(key) = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
    else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_expense_reimbursement_key",
            "A visible ASCII Idempotency-Key of at most 128 characters is required.",
        );
    };
    let input = match serde_json::from_slice::<expense_reimbursements::ReimbursementInput>(&body) {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_expense_reimbursement",
                "Provide a positive amount, return method, reason and valid return reference or cash location.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The expense reimbursement could not be recorded right now.",
        );
    };
    match expense_reimbursements::create(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &expense_id,
        key,
        &body,
        input,
    )
    .await
    {
        Ok(data) => (StatusCode::CREATED, Json(serde_json::json!({"data":data}))).into_response(),
        Err(expense_reimbursements::ReimbursementError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_expense_reimbursement",
            "Provide a positive amount, supported method, valid date and reason; cash requires an initialized non-register location, while non-cash returns require a reference.",
        ),
        Err(expense_reimbursements::ReimbursementError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "expense_not_found",
            "The expense or cash location is unavailable in this branch.",
        ),
        Err(expense_reimbursements::ReimbursementError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "expense_reimbursement_conflict",
            "The request key or external reference was already used with different details, or the cash location is no longer available.",
        ),
        Err(expense_reimbursements::ReimbursementError::InsufficientFunds) => auth_error(
            StatusCode::CONFLICT,
            "expense_reimbursement_exceeds_balance",
            "This return exceeds the remaining reimbursable expense amount.",
        ),
        Err(expense_reimbursements::ReimbursementError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "expense_reimbursement_unavailable",
            "The expense reimbursement could not be recorded right now.",
        ),
    }
}

async fn match_supplier_bill_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "suppliers:settle",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input = match serde_json::from_slice::<supplier_billing::MatchBillInput>(&body) {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_supplier_bill",
                "Provide a supplier invoice, valid dates, total and receipt allocations.",
            );
        }
    };
    let Some(key) = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
    else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_supplier_bill_key",
            "A visible ASCII Idempotency-Key of at most 128 characters is required.",
        );
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The supplier invoice could not be matched right now.",
        );
    };
    match supplier_billing::match_bill(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        key,
        &body,
        input,
    )
    .await
    {
        Ok(data) => (StatusCode::CREATED, Json(serde_json::json!({"data":data}))).into_response(),
        Err(supplier_billing::BillingError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_supplier_bill",
            "Invoice dates, amounts, references or receipt allocations are invalid.",
        ),
        Err(supplier_billing::BillingError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "supplier_bill_reference_not_found",
            "The supplier, branch or selected delivery receipt is unavailable.",
        ),
        Err(supplier_billing::BillingError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "supplier_bill_conflict",
            "The invoice reference is already recorded or the selected delivery balance has changed.",
        ),
        Err(supplier_billing::BillingError::IdempotencyConflict) => auth_error(
            StatusCode::CONFLICT,
            "supplier_bill_request_conflict",
            "This request key was already used with different invoice details.",
        ),
        Err(
            supplier_billing::BillingError::DrawerProof
            | supplier_billing::BillingError::DrawerLease,
        ) => auth_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "supplier_billing_internal",
            "The supplier invoice could not be matched right now.",
        ),
        Err(
            supplier_billing::BillingError::CashLocationUnavailable
            | supplier_billing::BillingError::CashLocationConflict
            | supplier_billing::BillingError::CashLocationInsufficientFunds,
        ) => auth_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "supplier_billing_internal",
            "The supplier invoice could not be matched right now.",
        ),
        Err(supplier_billing::BillingError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "supplier_billing_unavailable",
            "The supplier invoice could not be matched right now.",
        ),
    }
}

async fn create_supplier_credit_note_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "suppliers:settle",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input =
        match serde_json::from_slice::<supplier_billing::CreateSupplierCreditNoteInput>(&body) {
            Ok(value) => value,
            Err(_) => {
                return auth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_supplier_credit_note",
                    "Provide a credit reference, date, reason and valid returned receipt lines.",
                )
            }
        };
    let Some(key) = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
    else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_supplier_credit_note_key",
            "A visible ASCII Idempotency-Key of at most 128 characters is required.",
        );
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The supplier credit note could not be recorded right now.",
        );
    };
    match supplier_billing::create_credit_note(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        key,
        &body,
        input,
    )
    .await
    {
        Ok(data) => {
            let status = if data.replayed {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            };
            (status, Json(serde_json::json!({"data":data}))).into_response()
        }
        Err(supplier_billing::BillingError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_supplier_credit_note",
            "Credit details, dates, amounts or returned receipt lines are invalid.",
        ),
        Err(supplier_billing::BillingError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "supplier_credit_source_not_found",
            "The invoice, receipt or returned product is unavailable.",
        ),
        Err(supplier_billing::BillingError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "supplier_credit_conflict",
            "The invoice balance, receipt allocation, stock or supplier reference has changed.",
        ),
        Err(supplier_billing::BillingError::IdempotencyConflict) => auth_error(
            StatusCode::CONFLICT,
            "supplier_credit_request_conflict",
            "This request key was already used with different credit note details.",
        ),
        Err(
            supplier_billing::BillingError::DrawerProof
            | supplier_billing::BillingError::DrawerLease
            | supplier_billing::BillingError::CashLocationUnavailable
            | supplier_billing::BillingError::CashLocationConflict
            | supplier_billing::BillingError::CashLocationInsufficientFunds,
        ) => auth_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "supplier_credit_internal",
            "The supplier credit note could not be recorded right now.",
        ),
        Err(supplier_billing::BillingError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "supplier_credit_unavailable",
            "The supplier credit note could not be recorded right now.",
        ),
    }
}

async fn record_supplier_payment_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "suppliers:settle",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Supplier payment could not be recorded right now.",
        );
    };
    let key = match headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
    {
        Some(value) => value,
        None => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_supplier_payment_key",
                "A valid Idempotency-Key header is required.",
            );
        }
    };
    let input = match serde_json::from_slice::<supplier_billing::RecordSupplierPaymentInput>(&body)
    {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_supplier_payment",
                "Supplier payment details are invalid.",
            );
        }
    };
    match supplier_billing::record_payment(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        key,
        &body,
        input,
        None,
    )
    .await
    {
        Ok(result) => Json(serde_json::json!({"data":result})).into_response(),
        Err(supplier_billing::BillingError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_supplier_payment",
            "Payment method, amount, reference or bill allocations are invalid.",
        ),
        Err(supplier_billing::BillingError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "supplier_payment_reference_not_found",
            "The supplier, branch or selected invoice is unavailable.",
        ),
        Err(supplier_billing::BillingError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "supplier_payment_conflict",
            "A selected invoice balance has changed or the allocation exceeds its open amount.",
        ),
        Err(supplier_billing::BillingError::IdempotencyConflict) => auth_error(
            StatusCode::CONFLICT,
            "supplier_payment_request_conflict",
            "This request key was already used with different payment details.",
        ),
        Err(supplier_billing::BillingError::CashLocationUnavailable) => auth_error(
            StatusCode::NOT_FOUND,
            "supplier_payment_cash_location_unavailable",
            "Choose an active, initialized safe or petty-cash location in this branch.",
        ),
        Err(
            supplier_billing::BillingError::CashLocationConflict
            | supplier_billing::BillingError::CashLocationInsufficientFunds,
        ) => auth_error(
            StatusCode::CONFLICT,
            "supplier_payment_cash_location_conflict",
            "The cash location changed or does not have enough tracked cash.",
        ),
        Err(supplier_billing::BillingError::DrawerProof) => auth_error(
            StatusCode::FORBIDDEN,
            "invalid_supplier_payment_proof",
            "The signed station proof is invalid, expired, or already consumed.",
        ),
        Err(supplier_billing::BillingError::DrawerLease) => auth_error(
            StatusCode::CONFLICT,
            "station_lease_unavailable",
            "A cash payment requires the active cashier lease for this station.",
        ),
        Err(supplier_billing::BillingError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "supplier_payment_unavailable",
            "The supplier payment could not be recorded right now.",
        ),
    }
}

async fn record_station_cash_expense_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "expenses:cash:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The cash expense could not be recorded right now.",
        );
    };
    let header = |name: &'static str| headers.get(name).and_then(|value| value.to_str().ok());
    let (Some(key), Some(lease_id), Some(lease_token), Some(challenge_id), Some(proof_hex)) = (
        header("idempotency-key"),
        header("x-station-lease-id"),
        header("x-station-lease-token"),
        header("x-station-challenge-id"),
        header("x-station-proof"),
    ) else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "cash_expense_proof_headers_required",
            "Cash expense requires a signed active station lease and an Idempotency-Key.",
        );
    };
    let input = match serde_json::from_slice::<expenses::ExpenseInput>(&body) {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_cash_expense",
                "Provide valid cash expense details and the open drawer shift.",
            );
        }
    };
    match expenses::create(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        key,
        &body,
        input,
        Some(expenses::DrawerExpenseProof {
            station_id: &station_id,
            lease_id,
            lease_token,
            challenge_id,
            proof_hex,
        }),
    )
    .await
    {
        Ok(result) => Json(serde_json::json!({"data":result})).into_response(),
        Err(expenses::ExpenseError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_cash_expense",
            "Cash expense, amount, date or drawer shift is invalid.",
        ),
        Err(expenses::ExpenseError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "cash_expense_branch_not_found",
            "The branch or open drawer is unavailable.",
        ),
        Err(expenses::ExpenseError::IdempotencyConflict) => auth_error(
            StatusCode::CONFLICT,
            "cash_expense_request_conflict",
            "This request key was already used with different expense details.",
        ),
        Err(expenses::ExpenseError::DrawerProof) => auth_error(
            StatusCode::FORBIDDEN,
            "invalid_cash_expense_proof",
            "The signed station proof is invalid, expired, or already consumed.",
        ),
        Err(expenses::ExpenseError::DrawerLease) => auth_error(
            StatusCode::CONFLICT,
            "station_lease_unavailable",
            "A cash expense requires the active cashier lease for this station.",
        ),
        Err(expenses::ExpenseError::DrawerConflict) => auth_error(
            StatusCode::CONFLICT,
            "cash_expense_drawer_conflict",
            "The selected drawer shift is not open at this station.",
        ),
        Err(expenses::ExpenseError::CashLocationUnavailable) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_cash_expense_source",
            "Station cash expenses must use the open register drawer.",
        ),
        Err(
            expenses::ExpenseError::CashLocationInsufficientFunds
            | expenses::ExpenseError::CashLocationConflict,
        ) => auth_error(
            StatusCode::CONFLICT,
            "cash_expense_source_conflict",
            "The selected cash source changed or has insufficient funds.",
        ),
        Err(expenses::ExpenseError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "cash_expense_unavailable",
            "The expense and drawer could not be updated right now.",
        ),
    }
}

async fn record_station_supplier_payment_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "suppliers:settle",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Supplier payment could not be recorded right now.",
        );
    };
    let header = |name: &'static str| headers.get(name).and_then(|value| value.to_str().ok());
    let (Some(key), Some(lease_id), Some(lease_token), Some(challenge_id), Some(proof_hex)) = (
        header("idempotency-key"),
        header("x-station-lease-id"),
        header("x-station-lease-token"),
        header("x-station-challenge-id"),
        header("x-station-proof"),
    ) else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "supplier_payment_proof_headers_required",
            "Cash supplier payment requires a signed active station lease and an Idempotency-Key.",
        );
    };
    let input = match serde_json::from_slice::<supplier_billing::RecordSupplierPaymentInput>(&body)
    {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_supplier_payment",
                "Supplier payment details are invalid.",
            );
        }
    };
    match supplier_billing::record_payment(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        key,
        &body,
        input,
        Some(supplier_billing::DrawerPaymentProof {
            station_id: &station_id,
            lease_id,
            lease_token,
            challenge_id,
            proof_hex,
        }),
    )
    .await
    {
        Ok(result) => Json(serde_json::json!({"data":result})).into_response(),
        Err(supplier_billing::BillingError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_supplier_payment",
            "Cash method, open drawer shift, amount, reference or bill allocations are invalid.",
        ),
        Err(supplier_billing::BillingError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "supplier_payment_reference_not_found",
            "The supplier, branch or selected invoice is unavailable.",
        ),
        Err(supplier_billing::BillingError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "supplier_payment_conflict",
            "The drawer shift or selected invoice balance has changed.",
        ),
        Err(supplier_billing::BillingError::IdempotencyConflict) => auth_error(
            StatusCode::CONFLICT,
            "supplier_payment_request_conflict",
            "This request key was already used with different payment details.",
        ),
        Err(
            supplier_billing::BillingError::CashLocationUnavailable
            | supplier_billing::BillingError::CashLocationConflict
            | supplier_billing::BillingError::CashLocationInsufficientFunds,
        ) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_supplier_payment_cash_source",
            "Station cash supplier payments must use the signed open register drawer.",
        ),
        Err(supplier_billing::BillingError::DrawerProof) => auth_error(
            StatusCode::FORBIDDEN,
            "invalid_supplier_payment_proof",
            "The signed station proof is invalid, expired, or already consumed.",
        ),
        Err(supplier_billing::BillingError::DrawerLease) => auth_error(
            StatusCode::CONFLICT,
            "station_lease_unavailable",
            "A cash payment requires the active cashier lease for this station.",
        ),
        Err(supplier_billing::BillingError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "supplier_billing_unavailable",
            "Supplier payment could not be recorded right now.",
        ),
    }
}

async fn update_purchase_order_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, order_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    if uuid::Uuid::parse_str(&order_id).is_err() {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_purchase_order_id",
            "The purchase-order identifier is invalid.",
        );
    }
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "purchases:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input = match serde_json::from_slice::<purchasing::PurchaseOrderInput>(&body) {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_purchase_order",
                "Provide valid draft fields, product lines and expectedRevision.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The purchase order could not be updated right now.",
        );
    };
    match purchasing::update_draft(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &order_id,
        input,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(purchasing::PurchaseError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_purchase_order",
            "Purchase order fields or lines are invalid.",
        ),
        Err(purchasing::PurchaseError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "purchase_order_product_not_found",
            "A selected active branch product could not be found.",
        ),
        Err(purchasing::PurchaseError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "purchase_order_revision_conflict",
            "The order changed or is no longer a draft. Reload before editing.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "purchases_unavailable",
            "The purchase order could not be updated right now.",
        ),
    }
}

async fn transition_purchase_order_endpoint(
    state: Arc<AppState>,
    headers: HeaderMap,
    tenant_id: String,
    branch_id: String,
    order_id: String,
    action: &'static str,
    body: Bytes,
) -> Response {
    if uuid::Uuid::parse_str(&order_id).is_err() {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_purchase_order_id",
            "The purchase-order identifier is invalid.",
        );
    }
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "purchases:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input = match serde_json::from_slice::<purchasing::TransitionInput>(&body) {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_purchase_order_transition",
                "Provide a positive expectedRevision.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The purchase order could not be changed right now.",
        );
    };
    match purchasing::transition(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &order_id,
        action,
        input,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(purchasing::PurchaseError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_purchase_order_transition",
            "The order revision or current state does not permit this action.",
        ),
        Err(purchasing::PurchaseError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "purchase_order_not_found",
            "The purchase order is not available in this branch.",
        ),
        Err(purchasing::PurchaseError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "purchase_order_revision_conflict",
            "The order changed or cannot make that status transition. Reload it.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "purchases_unavailable",
            "The purchase order could not be changed right now.",
        ),
    }
}

async fn place_purchase_order_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, order_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    transition_purchase_order_endpoint(
        state, headers, tenant_id, branch_id, order_id, "place", body,
    )
    .await
}
async fn cancel_purchase_order_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, order_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    transition_purchase_order_endpoint(
        state, headers, tenant_id, branch_id, order_id, "cancel", body,
    )
    .await
}

async fn receive_purchase_order_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, order_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    if uuid::Uuid::parse_str(&order_id).is_err() {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_purchase_order_id",
            "The purchase-order identifier is invalid.",
        );
    }
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "purchases:invoice",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input = match serde_json::from_slice::<purchasing::ReceiptInput>(&body) {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_purchase_receipt",
                "Provide actual received quantities, unit costs and expectedRevision.",
            );
        }
    };
    let Some(key) = headers.get("idempotency-key").and_then(|v| v.to_str().ok()) else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_purchase_receipt_key",
            "A visible ASCII Idempotency-Key of at most 128 characters is required.",
        );
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The delivery could not be received right now.",
        );
    };
    match purchasing::receive(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &order_id,
        key,
        &body,
        input,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(purchasing::PurchaseError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_purchase_receipt",
            "Received quantities or unit costs are invalid or exceed the remaining order.",
        ),
        Err(purchasing::PurchaseError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "purchase_order_line_not_found",
            "An order line is not available in this purchase order.",
        ),
        Err(purchasing::PurchaseError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "purchase_order_revision_conflict",
            "The order changed or is not available for receiving. Reload it.",
        ),
        Err(purchasing::PurchaseError::IdempotencyConflict) => auth_error(
            StatusCode::CONFLICT,
            "purchase_receipt_request_conflict",
            "This delivery key was already used with different receipt details.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "purchases_unavailable",
            "The delivery could not be received right now.",
        ),
    }
}

async fn create_branch_supplier_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "suppliers:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input = match serde_json::from_slice::<suppliers::SupplierInput>(&body) {
        Ok(input) => input,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_supplier",
                "Provide a supplier name and valid optional contact details.",
            );
        }
    };
    let Some(key) = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
    else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_supplier_idempotency_key",
            "A visible ASCII Idempotency-Key of at most 128 characters is required.",
        );
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The supplier could not be saved right now.",
        );
    };
    match suppliers::create(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        key,
        &body,
        input,
    )
    .await
    {
        Ok(data) => (StatusCode::CREATED, Json(serde_json::json!({"data":data}))).into_response(),
        Err(suppliers::SupplierError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_supplier",
            "Supplier fields exceed allowed limits or contain invalid contact details.",
        ),
        Err(suppliers::SupplierError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "supplier_request_conflict",
            "This supplier request key was already used with different details.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "suppliers_unavailable",
            "The supplier could not be saved right now.",
        ),
    }
}

async fn update_branch_supplier_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, supplier_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    if uuid::Uuid::parse_str(&supplier_id).is_err() {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_supplier_id",
            "The supplier identifier is invalid.",
        );
    }
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "suppliers:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input = match serde_json::from_slice::<suppliers::SupplierUpdateInput>(&body) {
        Ok(input) => input,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_supplier",
                "Provide supplier details and expectedRevision.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The supplier could not be updated right now.",
        );
    };
    match suppliers::update(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &supplier_id,
        input,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(suppliers::SupplierError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_supplier",
            "Supplier fields exceed allowed limits or contain invalid contact details.",
        ),
        Err(suppliers::SupplierError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "supplier_revision_conflict",
            "The supplier changed or is no longer active. Reload it before editing.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "suppliers_unavailable",
            "The supplier could not be updated right now.",
        ),
    }
}

async fn set_branch_supplier_status_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, supplier_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    if uuid::Uuid::parse_str(&supplier_id).is_err() {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_supplier_id",
            "The supplier identifier is invalid.",
        );
    }
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "suppliers:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input = match serde_json::from_slice::<suppliers::SupplierStatusInput>(&body) {
        Ok(input) => input,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_supplier_status",
                "Provide active or archived status and expectedRevision.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The supplier status could not be changed right now.",
        );
    };
    match suppliers::set_status(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &supplier_id,
        input,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(suppliers::SupplierError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_supplier_status",
            "Provide active or archived status and a positive expectedRevision.",
        ),
        Err(suppliers::SupplierError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "supplier_revision_conflict",
            "The supplier changed or is no longer available. Reload before changing its status.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "suppliers_unavailable",
            "The supplier status could not be changed right now.",
        ),
    }
}

async fn list_customers_for_scope(
    state: Arc<AppState>,
    headers: HeaderMap,
    tenant_id: String,
    branch_id: String,
    station_id: Option<String>,
    query: customers::CustomerListQuery,
) -> Response {
    let permission = if query.include_archived.unwrap_or(false) {
        "customers:manage"
    } else {
        "customers:read"
    };
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        station_id.as_deref(),
        permission,
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Customers could not be loaded right now.",
        );
    };
    match customers::list(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        station_id.as_deref(),
        query,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(customers::CustomerError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_customer_query",
            "Customer search or page values are invalid.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "customers_unavailable",
            "Customers could not be loaded right now.",
        ),
    }
}

async fn create_customer_for_scope(
    state: Arc<AppState>,
    headers: HeaderMap,
    tenant_id: String,
    branch_id: String,
    station_id: Option<String>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        station_id.as_deref(),
        "customers:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input = match serde_json::from_slice::<customers::CustomerInput>(&body) {
        Ok(input) => input,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_customer",
                "Provide a valid customer name and optional contact details.",
            );
        }
    };
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok());
    let Some(idempotency_key) = idempotency_key else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_customer_idempotency_key",
            "A visible ASCII Idempotency-Key header of at most 128 characters is required.",
        );
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The customer could not be saved right now.",
        );
    };
    match customers::create(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        station_id.as_deref(),
        idempotency_key,
        &body,
        input,
    )
    .await
    {
        Ok(data) => (StatusCode::CREATED, Json(serde_json::json!({"data":data}))).into_response(),
        Err(customers::CustomerError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_customer",
            "Customer fields exceed the allowed limits or contain invalid contact details.",
        ),
        Err(customers::CustomerError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "customer_request_conflict",
            "This customer request key was already used with different details.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "customers_unavailable",
            "The customer could not be saved right now.",
        ),
    }
}

async fn list_branch_customers_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    query: Result<Query<customers::CustomerListQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Query(query) = match query {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_customer_query",
                "Customer search or page values are invalid.",
            );
        }
    };
    list_customers_for_scope(state, headers, tenant_id, branch_id, None, query).await
}

async fn list_station_customers_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
    query: Result<Query<customers::CustomerListQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Query(query) = match query {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_customer_query",
                "Customer search or page values are invalid.",
            );
        }
    };
    list_customers_for_scope(
        state,
        headers,
        tenant_id,
        branch_id,
        Some(station_id),
        query,
    )
    .await
}

async fn tenant_customer_credit_terms_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, customer_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_tenant_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        "customers:manage",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Customer credit terms could not be loaded right now.",
        );
    };
    match credit_terms::list(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &customer_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(credit_terms::CreditTermError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_credit_customer",
            "The customer identifier is invalid.",
        ),
        Err(credit_terms::CreditTermError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "credit_customer_not_found",
            "The active customer is unavailable in this business.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "customer_credit_terms_unavailable",
            "Customer credit terms could not be loaded right now.",
        ),
    }
}

async fn tenant_customer_credit_term_history_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, customer_id, currency_code)): Path<(String, String, String)>,
) -> Response {
    let (identity, membership) = match authorize_tenant_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        "customers:manage",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Customer credit history could not be loaded right now.",
        );
    };
    match credit_terms::history(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &customer_id,
        &currency_code,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(credit_terms::CreditTermError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_credit_customer_or_currency",
            "The customer identifier or currency code is invalid.",
        ),
        Err(credit_terms::CreditTermError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "credit_customer_not_found",
            "The active customer is unavailable in this business.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "customer_credit_history_unavailable",
            "Customer credit history could not be loaded right now.",
        ),
    }
}

async fn set_tenant_customer_credit_term_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, customer_id, currency_code)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_tenant_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        "customers:manage",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input = match serde_json::from_slice::<credit_terms::SetCustomerCreditTermInput>(&body) {
        Ok(input) => input,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_customer_credit_terms",
                "Provide a valid limit, due period, reason and expected revision.",
            );
        }
    };
    let Some(idempotency_key) = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
    else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "credit_terms_idempotency_required",
            "Provide a visible ASCII Idempotency-Key of at most 128 characters.",
        );
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Customer credit terms could not be saved right now.",
        );
    };
    match credit_terms::set(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &customer_id,
        &currency_code,
        idempotency_key,
        &body,
        input,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(credit_terms::CreditTermError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_customer_credit_terms",
            "Limit must be a non-negative safe amount, due days 0–365, and reason 1–500 characters. Use expectedRevision 0 to create terms.",
        ),
        Err(credit_terms::CreditTermError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "credit_customer_not_found",
            "The active customer is unavailable in this business.",
        ),
        Err(credit_terms::CreditTermError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "customer_credit_terms_conflict",
            "The terms changed or this request key was reused. Refresh the customer terms and retry with a new key.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "customer_credit_terms_unavailable",
            "Customer credit terms could not be saved right now.",
        ),
    }
}

async fn station_customer_credit_position_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, customer_id)): Path<(String, String, String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "customers:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Customer credit availability could not be loaded right now.",
        );
    };
    match receivables::customer_credit_position(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &station_id,
        &customer_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data": data})).into_response(),
        Err(receivables::ReceivablesError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_credit_customer",
            "Choose a valid customer to view credit availability.",
        ),
        Err(receivables::ReceivablesError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "credit_customer_not_found",
            "The selected active customer is not available in this branch.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "customer_credit_position_unavailable",
            "Customer credit availability could not be loaded right now.",
        ),
    }
}

async fn station_customer_open_receivables_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, customer_id)): Path<(String, String, String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "sales:checkout",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Open customer invoices could not be loaded right now.",
        );
    };
    match receivables::list_open_invoices(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &station_id,
        &customer_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(receivables::ReceivablesError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_receivable_customer",
            "Choose a valid customer to view open invoices.",
        ),
        Err(receivables::ReceivablesError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "receivable_customer_not_found",
            "The selected active customer is not available in this branch.",
        ),
        Err(receivables::ReceivablesError::DrawerLease) => auth_error(
            StatusCode::FORBIDDEN,
            "cashier_station_permission_required",
            "An active cashier profile assigned to this station is required.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "open_receivables_unavailable",
            "Open customer invoices could not be loaded right now.",
        ),
    }
}

async fn record_station_cash_collection_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "sales:checkout",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The customer payment could not be recorded right now.",
        );
    };
    let header = |name: &'static str| headers.get(name).and_then(|value| value.to_str().ok());
    let (Some(key), Some(lease_id), Some(lease_token), Some(challenge_id), Some(proof_hex)) = (
        header("idempotency-key"),
        header("x-station-lease-id"),
        header("x-station-lease-token"),
        header("x-station-challenge-id"),
        header("x-station-proof"),
    ) else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "receivable_collection_proof_required",
            "A signed cashier station proof and Idempotency-Key are required to record cash.",
        );
    };
    let input = match serde_json::from_slice::<receivables::CashCollectionInput>(&body) {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_receivable_collection",
                "Provide a customer, open drawer shift, positive amount and matching invoice allocations.",
            );
        }
    };
    match receivables::record_cash_collection(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        key,
        &body,
        input,
        receivables::CashCollectionDrawerProof {
            station_id: &station_id,
            lease_id,
            lease_token,
            challenge_id,
            proof_hex,
        },
    )
    .await
    {
        Ok(result) => (StatusCode::CREATED, Json(serde_json::json!({"data":result}))).into_response(),
        Err(receivables::ReceivablesError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_receivable_collection",
            "Provide a customer, open drawer shift, positive amount and matching invoice allocations.",
        ),
        Err(receivables::ReceivablesError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "receivable_invoice_unavailable",
            "The selected customer, branch or invoice is unavailable.",
        ),
        Err(receivables::ReceivablesError::IdempotencyConflict) => auth_error(
            StatusCode::CONFLICT,
            "receivable_collection_request_conflict",
            "This request key was already used with different payment details.",
        ),
        Err(receivables::ReceivablesError::AllocationConflict) => auth_error(
            StatusCode::CONFLICT,
            "receivable_allocation_conflict",
            "An invoice balance changed or the payment exceeds the remaining amount due.",
        ),
        Err(receivables::ReceivablesError::DrawerProof) => auth_error(
            StatusCode::FORBIDDEN,
            "invalid_receivable_collection_proof",
            "The signed station proof is invalid, expired or already used.",
        ),
        Err(receivables::ReceivablesError::DrawerLease) => auth_error(
            StatusCode::CONFLICT,
            "station_lease_unavailable",
            "Cash collection requires the active cashier lease for this station.",
        ),
        Err(receivables::ReceivablesError::DrawerConflict) => auth_error(
            StatusCode::CONFLICT,
            "receivable_collection_drawer_conflict",
            "The selected drawer shift is not open at this station.",
        ),
        Err(receivables::ReceivablesError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "receivable_collection_unavailable",
            "The customer payment, drawer and accounting records could not be updated right now.",
        ),
    }
}

async fn update_customer_for_scope(
    state: Arc<AppState>,
    headers: HeaderMap,
    tenant_id: String,
    branch_id: String,
    station_id: Option<String>,
    customer_id: String,
    body: Bytes,
) -> Response {
    if uuid::Uuid::parse_str(&customer_id).is_err() {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_customer_id",
            "The customer identifier is invalid.",
        );
    }
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        station_id.as_deref(),
        "customers:manage",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input = match serde_json::from_slice::<customers::CustomerUpdateInput>(&body) {
        Ok(input) => input,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_customer",
                "Provide valid customer details and expectedRevision.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The customer could not be updated right now.",
        );
    };
    match customers::update(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        station_id.as_deref(),
        &customer_id,
        input,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(customers::CustomerError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_customer",
            "Customer fields exceed allowed limits or contain invalid contact details.",
        ),
        Err(customers::CustomerError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "customer_revision_conflict",
            "The customer changed or is no longer active. Reload it before editing.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "customers_unavailable",
            "The customer could not be updated right now.",
        ),
    }
}

async fn set_customer_status_for_scope(
    state: Arc<AppState>,
    headers: HeaderMap,
    tenant_id: String,
    branch_id: String,
    station_id: Option<String>,
    customer_id: String,
    body: Bytes,
) -> Response {
    if uuid::Uuid::parse_str(&customer_id).is_err() {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_customer_id",
            "The customer identifier is invalid.",
        );
    }
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        station_id.as_deref(),
        "customers:manage",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input = match serde_json::from_slice::<customers::CustomerStatusInput>(&body) {
        Ok(input) => input,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_customer_status",
                "Provide status active or archived and expectedRevision.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The customer status could not be changed right now.",
        );
    };
    match customers::set_status(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        station_id.as_deref(),
        &customer_id,
        input,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(customers::CustomerError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_customer_status",
            "Provide status active or archived and a positive expectedRevision.",
        ),
        Err(customers::CustomerError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "customer_revision_conflict",
            "The customer changed or is no longer available. Reload it before changing status.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "customers_unavailable",
            "The customer status could not be changed right now.",
        ),
    }
}

async fn update_branch_customer_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, customer_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    update_customer_for_scope(
        state,
        headers,
        tenant_id,
        branch_id,
        None,
        customer_id,
        body,
    )
    .await
}

async fn set_branch_customer_status_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, customer_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    set_customer_status_for_scope(
        state,
        headers,
        tenant_id,
        branch_id,
        None,
        customer_id,
        body,
    )
    .await
}

async fn merge_branch_customer_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, customer_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    if uuid::Uuid::parse_str(&customer_id).is_err() {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_customer_id",
            "The customer identifier is invalid.",
        );
    }
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "customers:manage",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input = match serde_json::from_slice::<customers::CustomerMergeInput>(&body) {
        Ok(input) => input,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_customer_merge",
                "Provide a target customer, both current revisions and an idempotency key.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The customer records could not be merged right now.",
        );
    };
    match customers::merge(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &customer_id,
        &body,
        input,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(customers::CustomerError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_customer_merge",
            "Choose two different customers and provide valid current revisions and idempotency key.",
        ),
        Err(customers::CustomerError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "customer_merge_conflict",
            "A customer changed, was already merged, or is no longer active. Refresh the directory before retrying.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "customers_unavailable",
            "The customer records could not be merged right now.",
        ),
    }
}

async fn update_station_customer_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, customer_id)): Path<(String, String, String, String)>,
    body: Bytes,
) -> Response {
    update_customer_for_scope(
        state,
        headers,
        tenant_id,
        branch_id,
        Some(station_id),
        customer_id,
        body,
    )
    .await
}

async fn set_station_customer_status_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, customer_id)): Path<(String, String, String, String)>,
    body: Bytes,
) -> Response {
    set_customer_status_for_scope(
        state,
        headers,
        tenant_id,
        branch_id,
        Some(station_id),
        customer_id,
        body,
    )
    .await
}

async fn create_branch_customer_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    create_customer_for_scope(state, headers, tenant_id, branch_id, None, body).await
}

async fn create_station_customer_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    create_customer_for_scope(state, headers, tenant_id, branch_id, Some(station_id), body).await
}

async fn list_station_pickup_reservations_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "inventory:pickup:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Pickup reservations could not be loaded right now.",
        );
    };
    match pickup_reservations::list(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        Some(&station_id),
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "pickup_reservations_unavailable",
            "Pickup reservations could not be loaded right now.",
        ),
    }
}

async fn list_branch_pickup_reservations_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "inventory:pickup:manage",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Pickup reservations could not be loaded right now.",
        );
    };
    match pickup_reservations::list(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        None,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "pickup_reservations_unavailable",
            "Pickup reservations could not be loaded right now.",
        ),
    }
}

async fn create_pickup_reservation_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "inventory:pickup:request",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .filter(|v| {
            !v.is_empty() && v.len() <= 128 && v.bytes().all(|b| (0x21..=0x7e).contains(&b))
        });
    let Some(idempotency_key) = idempotency_key else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_pickup_idempotency_key",
            "A visible ASCII Idempotency-Key header of at most 128 characters is required.",
        );
    };
    let input = match serde_json::from_slice::<pickup_reservations::CreateInput>(&body) {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_pickup_reservation",
                "Provide a pickup branch, products, contact details, and customer contact consent.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The pickup request could not be saved right now.",
        );
    };
    match pickup_reservations::create(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &station_id,
        idempotency_key,
        &body,
        input,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(pickup_reservations::ReservationError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_pickup_reservation",
            "Choose one to 100 unique products, valid quantities, consented contact details, and an optional 15-minute to 24-hour hold.",
        ),
        Err(pickup_reservations::ReservationError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "pickup_option_unavailable",
            "The pickup branch, customer, or requested product is unavailable.",
        ),
        Err(
            pickup_reservations::ReservationError::Conflict
            | pickup_reservations::ReservationError::IdempotencyConflict,
        ) => auth_error(
            StatusCode::CONFLICT,
            "pickup_reservation_conflict",
            "This request conflicts with an earlier hold. Refresh and retry with a new idempotency key.",
        ),
        Err(pickup_reservations::ReservationError::InsufficientStock) => auth_error(
            StatusCode::CONFLICT,
            "pickup_stock_unavailable",
            "The pickup branch no longer has enough stock available to promise these items.",
        ),
        Err(pickup_reservations::ReservationError::Forbidden) => auth_error(
            StatusCode::FORBIDDEN,
            "pickup_transition_forbidden",
            "You cannot make this pickup status change in the current branch or station scope.",
        ),
        Err(pickup_reservations::ReservationError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "pickup_reservation_unavailable",
            "The pickup request could not be saved right now.",
        ),
    }
}

async fn transition_station_pickup_reservation_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, reservation_id)): Path<(
        String,
        String,
        String,
        String,
    )>,
    body: Bytes,
) -> Response {
    let input = match serde_json::from_slice::<pickup_reservations::TransitionInput>(&body) {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_pickup_transition",
                "Provide a ready or cancel action, the current revision, and a cancellation reason when cancelling.",
            );
        }
    };
    let permission = match input.action.as_str() {
        "ready" => "inventory:pickup:prepare",
        "cancel" => "inventory:pickup:request",
        _ => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_pickup_transition",
                "Choose ready or cancel.",
            );
        }
    };
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        permission,
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    transition_pickup_reservation(
        &state,
        headers,
        tenant_id,
        branch_id,
        Some(station_id),
        reservation_id,
        identity,
        membership,
        body,
        input,
    )
    .await
}

async fn transition_branch_pickup_reservation_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, reservation_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let input = match serde_json::from_slice::<pickup_reservations::TransitionInput>(&body) {
        Ok(value) => value,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_pickup_transition",
                "Provide a ready or cancel action, the current revision, and a cancellation reason when cancelling.",
            );
        }
    };
    if !["ready", "cancel"].contains(&input.action.as_str()) {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_pickup_transition",
            "Choose ready or cancel.",
        );
    }
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "inventory:pickup:manage",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    transition_pickup_reservation(
        &state,
        headers,
        tenant_id,
        branch_id,
        None,
        reservation_id,
        identity,
        membership,
        body,
        input,
    )
    .await
}

async fn transition_pickup_reservation(
    state: &Arc<AppState>,
    headers: HeaderMap,
    tenant_id: String,
    branch_id: String,
    station_id: Option<String>,
    reservation_id: String,
    identity: identity::InternalIdentity,
    membership: identity::TenantMembership,
    body: Bytes,
    input: pickup_reservations::TransitionInput,
) -> Response {
    let Some(idempotency_key) = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        })
    else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_pickup_idempotency_key",
            "A visible ASCII Idempotency-Key header of at most 128 characters is required.",
        );
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The pickup reservation could not be updated right now.",
        );
    };
    match pickup_reservations::transition(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        station_id.as_deref(),
        &reservation_id,
        idempotency_key,
        &body,
        input,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(pickup_reservations::ReservationError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_pickup_transition",
            "Provide a valid ready or cancel action, current revision, idempotency key, and a reason for cancellation.",
        ),
        Err(pickup_reservations::ReservationError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "pickup_reservation_not_found",
            "The pickup reservation is not available in this branch or station.",
        ),
        Err(pickup_reservations::ReservationError::Forbidden) => auth_error(
            StatusCode::FORBIDDEN,
            "pickup_transition_forbidden",
            "You cannot make this pickup status change in the current branch or station scope.",
        ),
        Err(
            pickup_reservations::ReservationError::Conflict
            | pickup_reservations::ReservationError::IdempotencyConflict,
        ) => auth_error(
            StatusCode::CONFLICT,
            "pickup_reservation_conflict",
            "The reservation changed or this request key was used for different content. Refresh the queue and retry.",
        ),
        Err(pickup_reservations::ReservationError::InsufficientStock) => auth_error(
            StatusCode::CONFLICT,
            "pickup_stock_unavailable",
            "The pickup branch no longer has enough stock available to promise these items.",
        ),
        Err(pickup_reservations::ReservationError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "pickup_reservation_unavailable",
            "The pickup reservation could not be updated right now.",
        ),
    }
}

async fn list_station_stock_transfers_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "inventory:transfer:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Station stock transfers could not be loaded right now.",
        );
    };
    match stock_transfers::list(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        Some(&station_id),
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({ "data": data })).into_response(),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "stock_transfers_unavailable",
            "Station stock transfers could not be loaded right now.",
        ),
    }
}

async fn request_stock_transfer_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "inventory:transfer:request",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        });
    let Some(idempotency_key) = idempotency_key else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_stock_transfer_idempotency_key",
            "A visible ASCII Idempotency-Key header of at most 128 characters is required.",
        );
    };
    let input = match serde_json::from_slice::<stock_transfers::RequestInput>(&body) {
        Ok(input) => input,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_stock_transfer_request",
                "The stock transfer request body is invalid.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The stock transfer request could not be saved right now.",
        );
    };
    match stock_transfers::request(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &station_id,
        idempotency_key,
        &body,
        input,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({ "data": data })).into_response(),
        Err(stock_transfers::TransferError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_stock_transfer_request",
            "Choose an active source branch, one to 100 distinct mapped products, and valid positive quantities.",
        ),
        Err(stock_transfers::TransferError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "stock_transfer_source_unavailable",
            "The source branch or one of the products is unavailable to this business.",
        ),
        Err(
            stock_transfers::TransferError::Conflict
            | stock_transfers::TransferError::IdempotencyConflict
            | stock_transfers::TransferError::InsufficientStock,
        ) => auth_error(
            StatusCode::CONFLICT,
            "stock_transfer_request_conflict",
            "This transfer request conflicts with an earlier request. Refresh and retry with a new idempotency key.",
        ),
        Err(stock_transfers::TransferError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "stock_transfer_unavailable",
            "The stock transfer request could not be saved right now.",
        ),
    }
}

async fn transition_stock_transfer_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, transfer_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "inventory:transfer:manage",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        });
    let Some(idempotency_key) = idempotency_key else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_stock_transfer_idempotency_key",
            "A visible ASCII Idempotency-Key header of at most 128 characters is required.",
        );
    };
    let input = match serde_json::from_slice::<stock_transfers::TransitionInput>(&body) {
        Ok(input) => input,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_stock_transfer_transition",
                "The stock transfer transition body is invalid.",
            );
        }
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The stock transfer could not be updated right now.",
        );
    };
    match stock_transfers::transition(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &transfer_id,
        idempotency_key,
        &body,
        input,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data": data})).into_response(),
        Err(stock_transfers::TransferError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_stock_transfer_transition",
            "Provide per-delivery quantities and the current revision. Rejecting, reducing a dispatch, or closing a short shipment requires a note.",
        ),
        Err(stock_transfers::TransferError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "stock_transfer_not_found",
            "The transfer is unavailable in this branch.",
        ),
        Err(
            stock_transfers::TransferError::Conflict
            | stock_transfers::TransferError::IdempotencyConflict,
        ) => auth_error(
            StatusCode::CONFLICT,
            "stock_transfer_conflict",
            "The transfer changed or this request key was already used for different details. Refresh and try again.",
        ),
        Err(stock_transfers::TransferError::InsufficientStock) => auth_error(
            StatusCode::CONFLICT,
            "stock_transfer_insufficient_stock",
            "The source branch cannot cover this transfer while preserving other stock commitments.",
        ),
        Err(stock_transfers::TransferError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "stock_transfer_unavailable",
            "The stock transfer could not be updated right now.",
        ),
    }
}

async fn create_branch_product_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    Json(input): Json<catalog::CreateProductInput>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "products:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match catalog::create_branch_product(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        input,
    )
    .await
    {
        Ok(data) => (StatusCode::CREATED, Json(BranchProductResponse { data })).into_response(),
        Err(catalog::CatalogError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_product",
            "Product values are invalid. Check identifiers, text lengths, price, tax, stock and reorder quantities.",
        ),
        Err(catalog::CatalogError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "product_identifier_conflict",
            "A product with this SKU or barcode already exists in this business.",
        ),
        Err(catalog::CatalogError::NotFound | catalog::CatalogError::InsufficientStock) => {
            auth_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "catalog_invariant_failed",
                "The product could not be saved because the catalog state was inconsistent.",
            )
        }
        Err(catalog::CatalogError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "catalog_unavailable",
            "The product could not be saved right now.",
        ),
    }
}

async fn adjust_branch_stock_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, product_id)): Path<(String, String, String)>,
    Json(input): Json<catalog::StockAdjustmentInput>,
) -> Response {
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        });
    let Some(idempotency_key) = idempotency_key else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_idempotency_key",
            "Send an Idempotency-Key header containing 1 to 128 visible ASCII characters.",
        );
    };
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "inventory:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match catalog::adjust_branch_stock(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &product_id,
        idempotency_key,
        input,
    )
    .await
    {
        Ok(data) => {
            let status = if data.replayed {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            };
            (status, Json(StockAdjustmentResponse { data })).into_response()
        }
        Err(catalog::CatalogError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_stock_adjustment",
            "Stock adjustment requires a nonzero quantity change and a note of at most 500 characters.",
        ),
        Err(catalog::CatalogError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "stock_adjustment_conflict",
            "This idempotency key was already used for a different adjustment.",
        ),
        Err(catalog::CatalogError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "catalog_product_not_found",
            "The active product is not assigned to this branch.",
        ),
        Err(catalog::CatalogError::InsufficientStock) => auth_error(
            StatusCode::CONFLICT,
            "insufficient_stock",
            "The adjustment would make branch stock negative or exceed the supported quantity.",
        ),
        Err(catalog::CatalogError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "inventory_unavailable",
            "Branch stock could not be adjusted right now.",
        ),
    }
}

async fn list_branch_payment_options_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    list_payment_options(&state, &headers, &tenant_id, &branch_id, None).await
}

async fn list_station_payment_options_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
) -> Response {
    list_payment_options(&state, &headers, &tenant_id, &branch_id, Some(&station_id)).await
}

async fn list_payment_options(
    state: &AppState,
    headers: &HeaderMap,
    tenant_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        state,
        headers,
        tenant_id,
        branch_id,
        station_id,
        "payments:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match payment_options::list(
        database,
        &identity.identity_id,
        tenant_id,
        &membership.membership_id,
        branch_id,
        station_id,
    )
    .await
    {
        Ok(data) => Json(PaymentOptionsResponse { data }).into_response(),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "payment_options_unavailable",
            "Branch payment options could not be loaded right now.",
        ),
    }
}

async fn create_branch_payment_option_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    Json(input): Json<payment_options::PaymentOptionInput>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "payments:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match payment_options::create(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        input,
    )
    .await
    {
        Ok(data) => (StatusCode::CREATED, Json(PaymentOptionResponse { data })).into_response(),
        Err(payment_options::PaymentOptionError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_payment_option",
            "Payment method fields are invalid. Use a short method key, label, optional provider name and ISO country code; cash cannot require a reference.",
        ),
        Err(payment_options::PaymentOptionError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "payment_option_conflict",
            "A payment method with this key or label already exists at this branch.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "payment_option_unavailable",
            "Branch payment option could not be saved right now.",
        ),
    }
}

async fn update_branch_payment_option_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, option_id)): Path<(String, String, String)>,
    Json(input): Json<payment_options::UpdatePaymentOptionInput>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "payments:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match payment_options::update(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &option_id,
        input,
    )
    .await
    {
        Ok(data) => Json(PaymentOptionResponse { data }).into_response(),
        Err(payment_options::PaymentOptionError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_payment_option",
            "Payment method fields or expected revision are invalid.",
        ),
        Err(payment_options::PaymentOptionError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "payment_option_conflict",
            "The payment method changed or no longer exists. Refresh branch settings and retry.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "payment_option_unavailable",
            "Branch payment option could not be updated right now.",
        ),
    }
}

async fn set_branch_product_status_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, product_id)): Path<(String, String, String)>,
    Json(input): Json<catalog::BranchProductStatusInput>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "products:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match catalog::set_branch_product_active(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &product_id,
        input.is_active,
    )
    .await
    {
        Ok(data) => Json(BranchProductResponse { data }).into_response(),
        Err(catalog::CatalogError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "catalog_product_not_found",
            "The product is not available in this branch catalog.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "catalog_unavailable",
            "The product status could not be updated right now.",
        ),
    }
}

async fn update_branch_product_settings_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, product_id)): Path<(String, String, String)>,
    Json(input): Json<catalog::BranchProductSettingsInput>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "products:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match catalog::update_branch_product_settings(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &product_id,
        input,
    )
    .await
    {
        Ok(data) => Json(BranchProductResponse { data }).into_response(),
        Err(catalog::CatalogError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_product_settings",
            "Price, tax, reorder and target quantities must be valid.",
        ),
        Err(catalog::CatalogError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "stale_product_revision",
            "The product changed since it was loaded. Refresh it and retry your changes.",
        ),
        Err(catalog::CatalogError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "catalog_product_not_found",
            "The product is not available in this branch catalog.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "catalog_unavailable",
            "The product settings could not be updated right now.",
        ),
    }
}

async fn authorize_held_ticket_scope(
    state: &AppState,
    headers: &HeaderMap,
    tenant_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
    permission: &str,
) -> Result<
    (
        identity::InternalIdentity,
        identity::TenantMembership,
        PgPool,
    ),
    Response,
> {
    let (identity, membership) =
        authorize_catalog_scope(state, headers, tenant_id, branch_id, station_id, permission)
            .await?;
    let Some(database) = state.identity_database.as_ref() else {
        return Err(auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        ));
    };
    Ok((identity, membership, database.clone()))
}

fn held_ticket_lease_headers(headers: &HeaderMap) -> Result<(String, String), Response> {
    let lease_id = headers
        .get("x-station-lease-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty());
    let lease_token = headers
        .get("x-station-lease-token")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty());
    match (lease_id, lease_token) {
        (Some(id), Some(token)) => Ok((id.to_owned(), token.to_owned())),
        _ => Err(auth_error(
            StatusCode::BAD_REQUEST,
            "cashier_lease_required",
            "Ticket changes require the active cashier station lease.",
        )),
    }
}

fn held_ticket_idempotency_key(headers: &HeaderMap) -> Result<String, Response> {
    headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .map(str::to_owned)
        .ok_or_else(|| {
            auth_error(
                StatusCode::BAD_REQUEST,
                "held_ticket_idempotency_key_required",
                "Ticket creation requires an idempotency key.",
            )
        })
}

fn held_ticket_error_response(error: held_tickets::TicketError) -> Response {
    match error {
        held_tickets::TicketError::InvalidInput => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_held_ticket",
            "The held-ticket request is invalid or too large.",
        ),
        held_tickets::TicketError::NotFound => auth_error(
            StatusCode::NOT_FOUND,
            "held_ticket_not_found",
            "The held ticket is no longer available in this branch.",
        ),
        held_tickets::TicketError::Conflict => auth_error(
            StatusCode::CONFLICT,
            "held_ticket_conflict",
            "Another register changed or claimed this ticket. Refresh the shared list and try again.",
        ),
        held_tickets::TicketError::InvalidLease => auth_error(
            StatusCode::UNAUTHORIZED,
            "cashier_lease_invalid",
            "The cashier station lease is expired or no longer active.",
        ),
        held_tickets::TicketError::Database => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "held_ticket_unavailable",
            "Shared tickets are temporarily unavailable.",
        ),
    }
}

async fn list_held_tickets_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership, database) = match authorize_held_ticket_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "sales:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    match held_tickets::list(
        &database,
        &identity.identity_id,
        &membership.membership_id,
        &tenant_id,
        &branch_id,
    )
    .await
    {
        Ok(data) => Json(HeldTicketsResponse { data }).into_response(),
        Err(error) => held_ticket_error_response(error),
    }
}

async fn create_held_ticket_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
    Json(input): Json<HeldTicketCreateRequest>,
) -> Response {
    let (identity, membership, database) = match authorize_held_ticket_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "sales:checkout",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let (lease_id, lease_token) = match held_ticket_lease_headers(&headers) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let idempotency_key = match held_ticket_idempotency_key(&headers) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match held_tickets::create(
        &database,
        &identity.identity_id,
        &membership.membership_id,
        &tenant_id,
        &branch_id,
        &station_id,
        &lease_id,
        &lease_token,
        &idempotency_key,
        input.payload,
    )
    .await
    {
        Ok(data) => Json(HeldTicketResponse { data }).into_response(),
        Err(error) => held_ticket_error_response(error),
    }
}

async fn claim_held_ticket_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, ticket_id)): Path<(String, String, String, String)>,
    Json(input): Json<HeldTicketRevisionRequest>,
) -> Response {
    let (identity, membership, database) = match authorize_held_ticket_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "sales:checkout",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let (lease_id, lease_token) = match held_ticket_lease_headers(&headers) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match held_tickets::claim(
        &database,
        &identity.identity_id,
        &membership.membership_id,
        &tenant_id,
        &branch_id,
        &station_id,
        &ticket_id,
        input.expected_revision,
        &lease_id,
        &lease_token,
    )
    .await
    {
        Ok(data) => Json(HeldTicketResponse { data }).into_response(),
        Err(error) => held_ticket_error_response(error),
    }
}

async fn save_held_ticket_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, ticket_id)): Path<(String, String, String, String)>,
    Json(input): Json<HeldTicketSaveRequest>,
) -> Response {
    let (identity, membership, database) = match authorize_held_ticket_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "sales:checkout",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let (lease_id, lease_token) = match held_ticket_lease_headers(&headers) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match held_tickets::save(
        &database,
        &identity.identity_id,
        &membership.membership_id,
        &tenant_id,
        &branch_id,
        &station_id,
        &ticket_id,
        input.expected_revision,
        &lease_id,
        &lease_token,
        input.payload,
    )
    .await
    {
        Ok(data) => Json(HeldTicketResponse { data }).into_response(),
        Err(error) => held_ticket_error_response(error),
    }
}

async fn prepare_held_ticket_checkout_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, ticket_id)): Path<(String, String, String, String)>,
    Json(input): Json<HeldTicketCheckoutRequest>,
) -> Response {
    let (identity, membership, database) = match authorize_held_ticket_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "sales:checkout",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let (lease_id, lease_token) = match held_ticket_lease_headers(&headers) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match held_tickets::prepare_checkout(
        &database,
        &identity.identity_id,
        &membership.membership_id,
        &tenant_id,
        &branch_id,
        &station_id,
        &ticket_id,
        input.expected_revision,
        &lease_id,
        &lease_token,
        input.body,
        &input.idempotency_key,
    )
    .await
    {
        Ok(data) => Json(HeldTicketResponse { data }).into_response(),
        Err(error) => held_ticket_error_response(error),
    }
}

async fn clear_held_ticket_checkout_attempt_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, ticket_id)): Path<(String, String, String, String)>,
    Json(input): Json<HeldTicketRevisionRequest>,
) -> Response {
    let (identity, membership, database) = match authorize_held_ticket_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "sales:checkout",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let (lease_id, lease_token) = match held_ticket_lease_headers(&headers) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match held_tickets::clear_checkout_attempt(
        &database,
        &identity.identity_id,
        &membership.membership_id,
        &tenant_id,
        &branch_id,
        &station_id,
        &ticket_id,
        input.expected_revision,
        &lease_id,
        &lease_token,
    )
    .await
    {
        Ok(data) => Json(HeldTicketResponse { data }).into_response(),
        Err(error) => held_ticket_error_response(error),
    }
}

async fn release_held_ticket_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, ticket_id)): Path<(String, String, String, String)>,
    Json(input): Json<HeldTicketRevisionRequest>,
) -> Response {
    mutate_held_ticket_endpoint(
        state, headers, tenant_id, branch_id, station_id, ticket_id, input, false,
    )
    .await
}

async fn discard_held_ticket_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, ticket_id)): Path<(String, String, String, String)>,
    Json(input): Json<HeldTicketRevisionRequest>,
) -> Response {
    mutate_held_ticket_endpoint(
        state, headers, tenant_id, branch_id, station_id, ticket_id, input, true,
    )
    .await
}

async fn mutate_held_ticket_endpoint(
    state: Arc<AppState>,
    headers: HeaderMap,
    tenant_id: String,
    branch_id: String,
    station_id: String,
    ticket_id: String,
    input: HeldTicketRevisionRequest,
    discard: bool,
) -> Response {
    let (identity, membership, database) = match authorize_held_ticket_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "sales:checkout",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let (lease_id, lease_token) = match held_ticket_lease_headers(&headers) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match held_tickets::release(
        &database,
        &identity.identity_id,
        &membership.membership_id,
        &tenant_id,
        &branch_id,
        &station_id,
        &ticket_id,
        input.expected_revision,
        &lease_id,
        &lease_token,
        discard,
    )
    .await
    {
        Ok(data) => Json(HeldTicketResponse { data }).into_response(),
        Err(error) => held_ticket_error_response(error),
    }
}

async fn authorize_catalog_scope(
    state: &AppState,
    headers: &HeaderMap,
    tenant_id: &str,
    branch_id: &str,
    station_id: Option<&str>,
    permission: &str,
) -> Result<(identity::InternalIdentity, identity::TenantMembership), Response> {
    let identity = resolve_identity(state, headers).await?;
    let Some(database) = state.identity_database.as_ref() else {
        return Err(auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        ));
    };
    let memberships = identity::list_tenant_memberships(database, &identity.identity_id)
        .await
        .map_err(|_| {
            auth_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "tenant_memberships_unavailable",
                "Business access could not be loaded right now.",
            )
        })?;
    let Some(membership) = memberships
        .into_iter()
        .find(|value| value.tenant_id == tenant_id)
    else {
        return Err(auth_error(
            StatusCode::NOT_FOUND,
            "catalog_access_not_found",
            "The requested branch catalog is unavailable.",
        ));
    };
    let Some(branch) = membership
        .branches
        .iter()
        .find(|value| value.branch_id == branch_id)
    else {
        return Err(auth_error(
            StatusCode::NOT_FOUND,
            "catalog_access_not_found",
            "The requested branch catalog is unavailable.",
        ));
    };
    let scope = if let Some(station_id) = station_id {
        if !branch
            .stations
            .iter()
            .any(|station| station.station_id == station_id)
        {
            return Err(auth_error(
                StatusCode::NOT_FOUND,
                "catalog_access_not_found",
                "The requested register catalog is unavailable.",
            ));
        }
        authorization::ResourceScope::Station {
            branch_id,
            station_id,
        }
    } else {
        authorization::ResourceScope::Branch { branch_id }
    };
    if !authorization::has_permission(&membership.role_grants, permission, scope) {
        return Err(auth_error(
            StatusCode::FORBIDDEN,
            "catalog_permission_denied",
            "This account does not have permission to access the requested product catalog.",
        ));
    }
    Ok((identity, membership))
}

async fn authorize_tenant_catalog_scope(
    state: &AppState,
    headers: &HeaderMap,
    tenant_id: &str,
    permission: &str,
) -> Result<(identity::InternalIdentity, identity::TenantMembership), Response> {
    let identity = resolve_identity(state, headers).await?;
    let Some(database) = state.identity_database.as_ref() else {
        return Err(auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        ));
    };
    let memberships = identity::list_tenant_memberships(database, &identity.identity_id)
        .await
        .map_err(|_| {
            auth_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "tenant_memberships_unavailable",
                "Business access could not be loaded right now.",
            )
        })?;
    let Some(membership) = memberships
        .into_iter()
        .find(|value| value.tenant_id == tenant_id)
    else {
        return Err(auth_error(
            StatusCode::NOT_FOUND,
            "catalog_access_not_found",
            "The requested business catalog is unavailable.",
        ));
    };
    if !authorization::has_permission(
        &membership.role_grants,
        permission,
        authorization::ResourceScope::Tenant,
    ) {
        return Err(auth_error(
            StatusCode::FORBIDDEN,
            "catalog_permission_denied",
            "This account does not have tenant-wide permission to edit product identity.",
        ));
    }
    Ok((identity, membership))
}

async fn create_station_activation_challenge_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
) -> Response {
    let identity = match resolve_identity(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    let memberships = match identity::list_tenant_memberships(database, &identity.identity_id).await
    {
        Ok(memberships) => memberships,
        Err(_) => {
            return auth_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "tenant_memberships_unavailable",
                "Business access could not be loaded right now.",
            );
        }
    };
    let Some(membership) = memberships.iter().find(|item| item.tenant_id == tenant_id) else {
        return auth_error(
            StatusCode::NOT_FOUND,
            "station_access_not_found",
            "The requested station is not available to this account.",
        );
    };
    let assigned = membership.branches.iter().any(|branch| {
        branch.branch_id == branch_id
            && branch
                .stations
                .iter()
                .any(|station| station.station_id == station_id)
    });
    if !assigned {
        return auth_error(
            StatusCode::NOT_FOUND,
            "station_access_not_found",
            "The requested station is not available to this account.",
        );
    }
    if !authorization::has_permission(
        &membership.role_grants,
        "license:manage",
        authorization::ResourceScope::Tenant,
    ) {
        return auth_error(
            StatusCode::FORBIDDEN,
            "station_activation_denied",
            "Only a business owner can create a station activation challenge.",
        );
    }
    match station_activation::create_challenge(
        database,
        &identity.identity_id,
        &membership.membership_id,
        &tenant_id,
        &branch_id,
        &station_id,
    )
    .await
    {
        Ok(challenge) => Json(StationActivationChallengeResponse {
            data: StationActivationChallengeSnapshot {
                challenge_id: challenge.challenge_id,
                nonce: challenge.nonce,
                expires_at: challenge.expires_at,
            },
        })
        .into_response(),
        Err(station_activation::ActivationError::StationUnavailable) => auth_error(
            StatusCode::NOT_FOUND,
            "station_unavailable",
            "The register station is inactive or unavailable.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "station_challenge_unavailable",
            "A station activation challenge could not be created right now.",
        ),
    }
}

async fn activate_station_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
    Json(input): Json<StationActivationRequest>,
) -> Response {
    let identity = match resolve_identity(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(identity_database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    let memberships =
        match identity::list_tenant_memberships(identity_database, &identity.identity_id).await {
            Ok(memberships) => memberships,
            Err(_) => {
                return auth_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "tenant_memberships_unavailable",
                    "Business access could not be loaded right now.",
                );
            }
        };
    let Some(membership) = memberships.iter().find(|item| item.tenant_id == tenant_id) else {
        return auth_error(
            StatusCode::NOT_FOUND,
            "station_access_not_found",
            "The requested station is not available to this account.",
        );
    };
    let station_is_assigned = membership.branches.iter().any(|branch| {
        branch.branch_id == branch_id
            && branch
                .stations
                .iter()
                .any(|station| station.station_id == station_id)
    });
    if !station_is_assigned {
        return auth_error(
            StatusCode::NOT_FOUND,
            "station_access_not_found",
            "The requested station is not available to this account.",
        );
    }
    if !authorization::has_permission(
        &membership.role_grants,
        "license:manage",
        authorization::ResourceScope::Tenant,
    ) {
        return auth_error(
            StatusCode::FORBIDDEN,
            "station_activation_denied",
            "Only a business owner can activate a licensed station.",
        );
    }

    match station_activation::activate_station(
        identity_database,
        &identity.identity_id,
        &membership.membership_id,
        &tenant_id,
        &branch_id,
        &station_id,
        &input.device_public_key,
        &input.challenge_id,
        &input.signature,
    )
    .await
    {
        Ok(result) => Json(StationActivationResponse {
            data: StationActivationSnapshot {
                tenant_id,
                branch_id,
                station_id,
                activation_id: result.activation_id,
                active_station_count: result.active_stations,
                station_limit: result.station_limit,
                created: result.created,
            },
        })
        .into_response(),
        Err(station_activation::ActivationError::InvalidDeviceKey) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_device_key",
            "The station public key must be 32 bytes encoded as 64 hexadecimal characters.",
        ),
        Err(station_activation::ActivationError::InvalidChallenge) => auth_error(
            StatusCode::CONFLICT,
            "activation_challenge_invalid",
            "The activation challenge is expired, already used, or does not match this station.",
        ),
        Err(station_activation::ActivationError::InvalidProof) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_device_proof",
            "The device signature does not prove possession of the station private key.",
        ),
        Err(station_activation::ActivationError::NoStationEntitlement) => auth_error(
            StatusCode::PAYMENT_REQUIRED,
            "station_entitlement_required",
            "This business has no active station entitlement. Contact the account owner or support.",
        ),
        Err(station_activation::ActivationError::StationUnavailable) => auth_error(
            StatusCode::NOT_FOUND,
            "station_unavailable",
            "The register station is inactive or unavailable.",
        ),
        Err(station_activation::ActivationError::StationAlreadyActivated) => auth_error(
            StatusCode::CONFLICT,
            "station_already_activated",
            "This station is already bound to another device. An owner must replace its activation first.",
        ),
        Err(station_activation::ActivationError::DeviceAlreadyActivated) => auth_error(
            StatusCode::CONFLICT,
            "device_already_activated",
            "This device key is already active on another station in this business.",
        ),
        Err(station_activation::ActivationError::CapacityReached) => auth_error(
            StatusCode::CONFLICT,
            "station_capacity_reached",
            "All licensed station slots are in use. Increase the station limit or deactivate another till.",
        ),
        Err(
            station_activation::ActivationError::InvalidRevocationReason
            | station_activation::ActivationError::ActivationNotFound,
        ) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "station_activation_unavailable",
            "The station activation could not be completed right now.",
        ),
        Err(station_activation::ActivationError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "station_activation_unavailable",
            "The station activation could not be completed right now.",
        ),
    }
}

async fn revoke_station_activation_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
    Json(input): Json<StationActivationRevocationRequest>,
) -> Response {
    let identity = match resolve_identity(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    let memberships = match identity::list_tenant_memberships(database, &identity.identity_id).await
    {
        Ok(memberships) => memberships,
        Err(_) => {
            return auth_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "tenant_memberships_unavailable",
                "Business access could not be loaded right now.",
            );
        }
    };
    let Some(membership) = memberships.iter().find(|item| item.tenant_id == tenant_id) else {
        return auth_error(
            StatusCode::NOT_FOUND,
            "station_access_not_found",
            "The requested station is not available to this account.",
        );
    };
    let assigned = membership.branches.iter().any(|branch| {
        branch.branch_id == branch_id
            && branch
                .stations
                .iter()
                .any(|station| station.station_id == station_id)
    });
    if !assigned {
        return auth_error(
            StatusCode::NOT_FOUND,
            "station_access_not_found",
            "The requested station is not available to this account.",
        );
    }
    if !authorization::has_permission(
        &membership.role_grants,
        "license:manage",
        authorization::ResourceScope::Tenant,
    ) {
        return auth_error(
            StatusCode::FORBIDDEN,
            "station_activation_denied",
            "Only a business owner can revoke a station activation.",
        );
    }

    match station_activation::revoke_station(
        database,
        &identity.identity_id,
        &membership.membership_id,
        &tenant_id,
        &branch_id,
        &station_id,
        &input.reason,
    )
    .await
    {
        Ok(result) => Json(StationActivationRevocationResponse {
            data: StationActivationRevocationSnapshot {
                tenant_id,
                branch_id,
                station_id,
                activation_id: result.activation_id,
                active_station_count: result.active_stations,
                revoked_at: result.revoked_at,
            },
        })
        .into_response(),
        Err(station_activation::ActivationError::InvalidRevocationReason) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_revocation_reason",
            "Provide a reason between 3 and 500 characters.",
        ),
        Err(station_activation::ActivationError::ActivationNotFound) => auth_error(
            StatusCode::CONFLICT,
            "station_activation_not_active",
            "This station has no active device activation to revoke.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "station_activation_revoke_unavailable",
            "The station activation could not be revoked right now.",
        ),
    }
}

async fn acquire_station_lease_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
    Json(input): Json<StationLeaseAcquireRequest>,
) -> Response {
    let identity = match resolve_identity(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    let memberships = match identity::list_tenant_memberships(database, &identity.identity_id).await
    {
        Ok(memberships) => memberships,
        Err(_) => {
            return auth_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "tenant_memberships_unavailable",
                "Business access could not be loaded right now.",
            );
        }
    };
    let Some(membership) = memberships.iter().find(|item| item.tenant_id == tenant_id) else {
        return auth_error(
            StatusCode::NOT_FOUND,
            "station_access_not_found",
            "The requested station is not available to this account.",
        );
    };
    let assigned = membership.branches.iter().any(|branch| {
        branch.branch_id == branch_id
            && branch
                .stations
                .iter()
                .any(|station| station.station_id == station_id)
    });
    let cashier_grants: Vec<_> = membership
        .role_grants
        .iter()
        .filter(|grant| grant.role_key == "cashier")
        .cloned()
        .collect();
    let cashier_can_use_station = authorization::has_permission(
        &cashier_grants,
        "pos:use",
        authorization::ResourceScope::Station {
            branch_id: &branch_id,
            station_id: &station_id,
        },
    );
    if !assigned {
        return auth_error(
            StatusCode::NOT_FOUND,
            "station_access_not_found",
            "The requested station is not available to this account.",
        );
    }
    if !cashier_can_use_station {
        return auth_error(
            StatusCode::FORBIDDEN,
            "cashier_station_permission_required",
            "A cashier profile assigned to this station is required to start checkout.",
        );
    }

    match station_lease::acquire(
        database,
        &identity.identity_id,
        &membership.membership_id,
        &tenant_id,
        &branch_id,
        &station_id,
        &input.challenge_id,
        &input.signature,
    )
    .await
    {
        Ok(lease) => Json(StationLeaseResponse {
            data: StationLeaseSnapshot {
                lease_id: lease.lease_id,
                token: Some(lease.token),
                expires_at: lease.expires_at,
            },
        })
        .into_response(),
        Err(station_lease::LeaseError::NotAvailable) => auth_error(
            StatusCode::NOT_FOUND,
            "station_not_available",
            "The assigned active cashier station is unavailable.",
        ),
        Err(station_lease::LeaseError::AlreadyHeld) => auth_error(
            StatusCode::CONFLICT,
            "station_in_use",
            "Another cashier currently holds this station. Wait for them to switch profiles or for their lease to expire.",
        ),
        Err(station_lease::LeaseError::InvalidOrExpired) => auth_error(
            StatusCode::CONFLICT,
            "station_lease_expired",
            "The cashier station lease has expired. Sign in again to continue.",
        ),
        Err(station_lease::LeaseError::InvalidChallenge) => auth_error(
            StatusCode::CONFLICT,
            "station_lease_challenge_invalid",
            "The station proof challenge is expired, already used, or does not match this cashier and register.",
        ),
        Err(station_lease::LeaseError::InvalidProof) => auth_error(
            StatusCode::FORBIDDEN,
            "station_device_proof_invalid",
            "This device did not prove possession of the activated register key.",
        ),
        Err(station_lease::LeaseError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "station_lease_unavailable",
            "The cashier station could not be reserved right now.",
        ),
    }
}

async fn create_station_lease_challenge_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
) -> Response {
    let identity = match resolve_identity(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    let memberships = match identity::list_tenant_memberships(database, &identity.identity_id).await
    {
        Ok(memberships) => memberships,
        Err(_) => {
            return auth_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "tenant_memberships_unavailable",
                "Business access could not be loaded right now.",
            );
        }
    };
    let Some(membership) = memberships.iter().find(|item| item.tenant_id == tenant_id) else {
        return auth_error(
            StatusCode::NOT_FOUND,
            "station_access_not_found",
            "The requested station is not available to this account.",
        );
    };
    let assigned = membership.branches.iter().any(|branch| {
        branch.branch_id == branch_id
            && branch
                .stations
                .iter()
                .any(|station| station.station_id == station_id)
    });
    let cashier_grants: Vec<_> = membership
        .role_grants
        .iter()
        .filter(|grant| grant.role_key == "cashier")
        .cloned()
        .collect();
    let cashier_can_use_station = authorization::has_permission(
        &cashier_grants,
        "pos:use",
        authorization::ResourceScope::Station {
            branch_id: &branch_id,
            station_id: &station_id,
        },
    );
    if !assigned {
        return auth_error(
            StatusCode::NOT_FOUND,
            "station_access_not_found",
            "The requested station is not available to this account.",
        );
    }
    if !cashier_can_use_station {
        return auth_error(
            StatusCode::FORBIDDEN,
            "cashier_station_permission_required",
            "A cashier profile assigned to this station is required to start checkout.",
        );
    }

    match station_lease::create_challenge(
        database,
        &identity.identity_id,
        &membership.membership_id,
        &tenant_id,
        &branch_id,
        &station_id,
    )
    .await
    {
        Ok(challenge) => Json(StationLeaseChallengeResponse {
            data: StationLeaseChallengeSnapshot {
                challenge_id: challenge.challenge_id,
                nonce: challenge.nonce,
                expires_at: challenge.expires_at,
                message_to_sign: challenge.message_to_sign,
            },
        })
        .into_response(),
        Err(station_lease::LeaseError::NotAvailable) => auth_error(
            StatusCode::NOT_FOUND,
            "station_not_available",
            "The assigned active cashier station is unavailable.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "station_lease_challenge_unavailable",
            "A device-bound cashier challenge could not be created right now.",
        ),
    }
}

async fn heartbeat_station_lease_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(lease_id): Path<String>,
    Json(input): Json<StationLeaseCommand>,
) -> Response {
    update_station_lease_endpoint(state, headers, lease_id, input.token, true).await
}

async fn release_station_lease_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(lease_id): Path<String>,
    Json(input): Json<StationLeaseCommand>,
) -> Response {
    update_station_lease_endpoint(state, headers, lease_id, input.token, false).await
}

async fn create_checkout_challenge_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
    Json(input): Json<CheckoutChallengeRequest>,
) -> Response {
    let required_permission = if input.operation == "sale.refund" {
        "sales:refund"
    } else {
        "sales:checkout"
    };
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        required_permission,
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    let header_value = |name: &'static str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
    };
    let (Some(lease_id), Some(lease_token), Some(idempotency_key)) = (
        header_value("x-station-lease-id"),
        header_value("x-station-lease-token"),
        header_value("idempotency-key"),
    ) else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "checkout_proof_headers_required",
            "Checkout proof preparation requires the active station lease and an Idempotency-Key.",
        );
    };
    match station_proof::create_financial_challenge(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &station_id,
        lease_id,
        lease_token,
        &input.operation,
        input.resource_id.as_deref(),
        idempotency_key,
        &input.payload_hash,
    )
    .await
    {
        Ok(data) => Json(CheckoutChallengeResponse { data }).into_response(),
        Err(station_proof::ProofError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_checkout_proof_request",
            "Financial proof preparation requires an allowlisted operation, valid target and 32-byte payload hash.",
        ),
        Err(station_proof::ProofError::LeaseUnavailable) => auth_error(
            StatusCode::CONFLICT,
            "station_lease_unavailable",
            "The cashier lease is expired, revoked, or held by another staff account.",
        ),
        Err(station_proof::ProofError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "checkout_proof_unavailable",
            "Checkout proof could not be prepared right now.",
        ),
    }
}

async fn commit_sale_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "sales:checkout",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    let header_value = |name: &'static str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
    };
    let (Some(lease_id), Some(lease_token), Some(idempotency_key), Some(challenge_id), Some(proof)) = (
        header_value("x-station-lease-id"),
        header_value("x-station-lease-token"),
        header_value("idempotency-key"),
        header_value("x-station-challenge-id"),
        header_value("x-station-proof"),
    ) else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "checkout_proof_headers_required",
            "Checkout requires the active cashier lease, idempotency key, challenge ID, and station signature.",
        );
    };
    let input: sales::CommitSaleInput = match serde_json::from_slice(&body) {
        Ok(input) => input,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_sale_request",
                "The sale request must match the supported checkout schema.",
            );
        }
    };
    match sales::commit(
        database,
        &identity.identity_id,
        identity.display_name.as_deref(),
        &membership.membership_id,
        &tenant_id,
        &branch_id,
        &station_id,
        lease_id,
        lease_token,
        idempotency_key,
        challenge_id,
        proof,
        &body,
        input,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data": data})).into_response(),
        Err(sales::SaleError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_sale_request",
            "Sale lines and tenders must contain valid positive amounts and configured products.",
        ),
        Err(sales::SaleError::LeaseUnavailable) => auth_error(
            StatusCode::CONFLICT,
            "station_lease_unavailable",
            "The cashier lease is expired or no longer active.",
        ),
        Err(sales::SaleError::InvalidProof) => auth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_station_proof",
            "The station signature is invalid, expired, or bound to a different request.",
        ),
        Err(sales::SaleError::InsufficientStock) => auth_error(
            StatusCode::CONFLICT,
            "insufficient_stock",
            "One or more products no longer have enough stock.",
        ),
        Err(sales::SaleError::TenderUnavailable) => auth_error(
            StatusCode::CONFLICT,
            "tender_unavailable",
            "The selected payment method is disabled or its required reference is missing.",
        ),
        Err(sales::SaleError::PickupUnavailable) => auth_error(
            StatusCode::CONFLICT,
            "pickup_reservation_unavailable",
            "The pickup hold is not ready, has expired or changed, or its complete quantity is not in the sale.",
        ),
        Err(sales::SaleError::DrawerRequired) => auth_error(
            StatusCode::CONFLICT,
            "drawer_required",
            "Cash sales require an open hosted cash drawer before checkout is available.",
        ),
        Err(sales::SaleError::RegisterCutoverRequired) => auth_error(
            StatusCode::CONFLICT,
            "register_cash_cutover_required",
            "A manager must record this register's physical opening count before cash sales can be completed.",
        ),
        Err(sales::SaleError::IdempotencyConflict) => auth_error(
            StatusCode::CONFLICT,
            "idempotency_conflict",
            "This idempotency key was already used for a different sale.",
        ),
        Err(sales::SaleError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "sale_item_not_found",
            "A product or branch needed for this sale is unavailable.",
        ),
        Err(sales::SaleError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "sale_unavailable",
            "The sale could not be committed right now.",
        ),
    }
}

async fn attach_sale_customer_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, sale_id)): Path<(String, String, String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "customers:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    let input: sales::AttachSaleCustomerInput = match serde_json::from_slice(&body) {
        Ok(input) => input,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_sale_customer_request",
                "Provide a valid customerId.",
            );
        }
    };
    match sales::attach_customer(
        database,
        &identity.identity_id,
        &membership.membership_id,
        &tenant_id,
        &branch_id,
        &station_id,
        &sale_id,
        &input.customer_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data": data})).into_response(),
        Err(sales::AttachCustomerError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_sale_customer_request",
            "Sale and customer IDs must be valid UUIDs.",
        ),
        Err(sales::AttachCustomerError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "sale_or_customer_not_found",
            "The sale or active customer is unavailable.",
        ),
        Err(sales::AttachCustomerError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "sale_customer_conflict",
            "This completed sale already has a different customer or cannot be changed.",
        ),
        Err(sales::AttachCustomerError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "sale_customer_unavailable",
            "The customer could not be linked to the sale right now.",
        ),
    }
}

async fn get_drawer_shift_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "pos:use",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match drawer::current(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &station_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(error) => drawer_error_response(error),
    }
}

async fn cash_drop_destinations_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "sales:checkout",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Cash drop destinations are unavailable right now.",
        );
    };
    match drawer::cash_drop_destinations(
        database,
        &identity.identity_id,
        &membership.membership_id,
        &tenant_id,
        &branch_id,
        &station_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(error) => drawer_error_response(error),
    }
}

async fn get_drawer_history_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "pos:use",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match drawer::history(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &station_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(error) => drawer_error_response(error),
    }
}

async fn get_x_drawer_report_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, shift_id)): Path<(String, String, String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "pos:use",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match drawer::current(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &station_id,
    )
    .await
    {
        Ok(Some(report)) if report.shift_id == shift_id => {
            Json(serde_json::json!({"data": report})).into_response()
        }
        Ok(_) => auth_error(
            StatusCode::CONFLICT,
            "drawer_shift_unavailable",
            "An X report is available only for the current open drawer shift.",
        ),
        Err(error) => drawer_error_response(error),
    }
}

async fn open_drawer_shift_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    drawer_mutation_endpoint(
        state, headers, tenant_id, branch_id, station_id, None, body, "open",
    )
    .await
}

async fn record_drawer_movement_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, shift_id)): Path<(String, String, String, String)>,
    body: Bytes,
) -> Response {
    drawer_mutation_endpoint(
        state,
        headers,
        tenant_id,
        branch_id,
        station_id,
        Some(shift_id),
        body,
        "movement",
    )
    .await
}

async fn close_drawer_shift_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, station_id, shift_id)): Path<(String, String, String, String)>,
    body: Bytes,
) -> Response {
    drawer_mutation_endpoint(
        state,
        headers,
        tenant_id,
        branch_id,
        station_id,
        Some(shift_id),
        body,
        "close",
    )
    .await
}

async fn drawer_mutation_endpoint(
    state: Arc<AppState>,
    headers: HeaderMap,
    tenant_id: String,
    branch_id: String,
    station_id: String,
    shift_id: Option<String>,
    body: Bytes,
    kind: &'static str,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        Some(&station_id),
        "sales:checkout",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    let value = |name: &'static str| {
        headers
            .get(name)
            .and_then(|header| header.to_str().ok())
            .filter(|v| !v.is_empty())
    };
    let (Some(lease_id), Some(token), Some(key), Some(challenge), Some(proof)) = (
        value("x-station-lease-id"),
        value("x-station-lease-token"),
        value("idempotency-key"),
        value("x-station-challenge-id"),
        value("x-station-proof"),
    ) else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "financial_proof_headers_required",
            "This drawer change requires an active cashier lease, idempotency key, challenge ID and station signature.",
        );
    };
    let result = match kind {
        "open" => match serde_json::from_slice::<drawer::OpenInput>(&body) {
            Ok(input) => {
                drawer::open(
                    database,
                    &identity.identity_id,
                    &membership.membership_id,
                    &tenant_id,
                    &branch_id,
                    &station_id,
                    lease_id,
                    token,
                    key,
                    challenge,
                    proof,
                    &body,
                    input,
                )
                .await
            }
            Err(_) => {
                return auth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_drawer_request",
                    "Opening cash must be a valid non-negative amount in minor currency units.",
                );
            }
        },
        "movement" => match (
            serde_json::from_slice::<drawer::MovementInput>(&body),
            shift_id.as_deref(),
        ) {
            (Ok(input), Some(id)) => {
                drawer::movement(
                    database,
                    &identity.identity_id,
                    &membership.membership_id,
                    &tenant_id,
                    &branch_id,
                    &station_id,
                    id,
                    lease_id,
                    token,
                    key,
                    challenge,
                    proof,
                    &body,
                    input,
                )
                .await
            }
            _ => {
                return auth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_drawer_request",
                    "Drawer movement requires a valid shift, movement type, positive amount and note.",
                );
            }
        },
        "close" => match (
            serde_json::from_slice::<drawer::CloseInput>(&body),
            shift_id.as_deref(),
        ) {
            (Ok(input), Some(id)) => {
                drawer::close(
                    database,
                    &identity.identity_id,
                    &membership.membership_id,
                    &tenant_id,
                    &branch_id,
                    &station_id,
                    id,
                    lease_id,
                    token,
                    key,
                    challenge,
                    proof,
                    &body,
                    input,
                )
                .await
            }
            _ => {
                return auth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_drawer_request",
                    "Drawer close requires a valid shift and counted cash amount.",
                );
            }
        },
        _ => unreachable!("drawer mutation routes use a fixed action"),
    };
    match result {
        Ok(data) => Json(serde_json::json!({"data":data})).into_response(),
        Err(error) => drawer_error_response(error),
    }
}

fn drawer_error_response(error: drawer::DrawerError) -> Response {
    match error {
        drawer::DrawerError::InvalidInput => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_drawer_request",
            "Drawer amounts, identifiers, idempotency key or cash-drop destination are invalid.",
        ),
        drawer::DrawerError::LeaseUnavailable => auth_error(
            StatusCode::CONFLICT,
            "station_lease_unavailable",
            "The cashier lease is expired or no longer active.",
        ),
        drawer::DrawerError::InvalidProof => auth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_station_proof",
            "The station signature is invalid, expired or bound to a different request.",
        ),
        drawer::DrawerError::ShiftAlreadyOpen => auth_error(
            StatusCode::CONFLICT,
            "drawer_shift_already_open",
            "This station already has an open drawer shift.",
        ),
        drawer::DrawerError::ShiftUnavailable => auth_error(
            StatusCode::NOT_FOUND,
            "drawer_shift_unavailable",
            "The drawer shift is unavailable or has already been closed.",
        ),
        drawer::DrawerError::IdempotencyConflict => auth_error(
            StatusCode::CONFLICT,
            "idempotency_conflict",
            "This idempotency key was already used for a different drawer operation.",
        ),
        drawer::DrawerError::NoOpenShift => auth_error(
            StatusCode::CONFLICT,
            "drawer_shift_required",
            "Open the station drawer before recording this cash operation.",
        ),
        drawer::DrawerError::RegisterCutoverRequired => auth_error(
            StatusCode::CONFLICT,
            "register_cash_cutover_required",
            "A manager must record this register's physical opening count before recording drawer movements.",
        ),
        drawer::DrawerError::AmountOverflow => auth_error(
            StatusCode::BAD_REQUEST,
            "drawer_amount_overflow",
            "The drawer total exceeds the supported amount range.",
        ),
        drawer::DrawerError::Database => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "drawer_unavailable",
            "The drawer operation could not be completed right now.",
        ),
    }
}

async fn update_station_lease_endpoint(
    state: Arc<AppState>,
    headers: HeaderMap,
    lease_id: String,
    token: String,
    renew: bool,
) -> Response {
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_lease_token",
            "The station lease token is invalid.",
        );
    }
    let identity = match resolve_identity(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    let result = if renew {
        station_lease::heartbeat(database, &identity.identity_id, &lease_id, &token).await
    } else {
        station_lease::release(database, &identity.identity_id, &lease_id, &token).await
    };
    match result {
        Ok(expires_at) => Json(StationLeaseResponse {
            data: StationLeaseSnapshot {
                lease_id,
                token: None,
                expires_at,
            },
        })
        .into_response(),
        Err(station_lease::LeaseError::InvalidOrExpired) => auth_error(
            StatusCode::CONFLICT,
            "station_lease_invalid_or_expired",
            "This station lease is no longer active. Sign in again before checkout.",
        ),
        Err(
            station_lease::LeaseError::NotAvailable
            | station_lease::LeaseError::AlreadyHeld
            | station_lease::LeaseError::InvalidChallenge
            | station_lease::LeaseError::InvalidProof,
        ) => auth_error(
            StatusCode::CONFLICT,
            "station_lease_conflict",
            "The station lease could not be updated.",
        ),
        Err(station_lease::LeaseError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "station_lease_unavailable",
            "The cashier station could not be updated right now.",
        ),
    }
}

async fn authenticated_branch_permissions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let identity = match resolve_identity(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(identity_database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    let memberships =
        match identity::list_tenant_memberships(identity_database, &identity.identity_id).await {
            Ok(memberships) => memberships,
            Err(_) => {
                return auth_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "tenant_memberships_unavailable",
                    "Business access could not be loaded right now.",
                );
            }
        };
    let Some(membership) = memberships.iter().find(|item| item.tenant_id == tenant_id) else {
        return auth_error(
            StatusCode::NOT_FOUND,
            "branch_access_not_found",
            "The requested branch is not available to this account.",
        );
    };
    if !membership
        .branches
        .iter()
        .any(|item| item.branch_id == branch_id)
    {
        return auth_error(
            StatusCode::NOT_FOUND,
            "branch_access_not_found",
            "The requested branch is not available to this account.",
        );
    }

    let scope = authorization::ResourceScope::Branch {
        branch_id: &branch_id,
    };
    let permissions = authorization::permissions_for(&membership.role_grants, scope);
    if permissions.is_empty() {
        return auth_error(
            StatusCode::FORBIDDEN,
            "branch_permission_denied",
            "This account does not have management permissions for the requested branch.",
        );
    }
    let tenant_permissions = authorization::permissions_for(
        &membership.role_grants,
        authorization::ResourceScope::Tenant,
    );

    Json(BranchPermissionsResponse {
        data: BranchPermissionSnapshot {
            tenant_id,
            branch_id,
            permissions,
            tenant_permissions,
        },
    })
    .into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AttendanceListQuery {
    limit: Option<i64>,
}

fn attendance_failure(error: attendance::AttendanceError) -> Response {
    match error {
        attendance::AttendanceError::InvalidInput => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_attendance_input",
            "Provide a valid schedule, period limit, or idempotency key.",
        ),
        attendance::AttendanceError::NotFound => auth_error(
            StatusCode::NOT_FOUND,
            "attendance_record_not_found",
            "The staff profile, schedule, or open work period is unavailable.",
        ),
        attendance::AttendanceError::Conflict => auth_error(
            StatusCode::CONFLICT,
            "attendance_conflict",
            "A work period is already open or the schedule changed. Refresh and try again.",
        ),
        attendance::AttendanceError::IdempotencyConflict => auth_error(
            StatusCode::CONFLICT,
            "attendance_idempotency_conflict",
            "That idempotency key was already used for a different attendance action.",
        ),
        attendance::AttendanceError::Database => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "attendance_unavailable",
            "Attendance could not be loaded or saved right now.",
        ),
    }
}

async fn my_attendance_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "attendance:self",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match attendance::get_my(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data": data})).into_response(),
        Err(error) => attendance_failure(error),
    }
}

async fn attendance_clock_in_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "attendance:self",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(key) = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
    else {
        return attendance_failure(attendance::AttendanceError::InvalidInput);
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match attendance::clock_in(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        key,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data": data})).into_response(),
        Err(error) => attendance_failure(error),
    }
}

async fn attendance_clock_out_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "attendance:self",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(key) = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
    else {
        return attendance_failure(attendance::AttendanceError::InvalidInput);
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match attendance::clock_out(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        key,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data": data})).into_response(),
        Err(error) => attendance_failure(error),
    }
}

async fn attendance_schedules_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "attendance:manage",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match attendance::list_schedules(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data": data})).into_response(),
        Err(error) => attendance_failure(error),
    }
}

async fn attendance_staff_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "attendance:manage",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match attendance::list_staff(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data": data})).into_response(),
        Err(error) => attendance_failure(error),
    }
}

async fn save_attendance_schedule_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id, target_membership_id)): Path<(String, String, String)>,
    Json(input): Json<attendance::ScheduleInput>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "attendance:manage",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match attendance::save_schedule(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        &target_membership_id,
        input,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data": data})).into_response(),
        Err(error) => attendance_failure(error),
    }
}

async fn attendance_periods_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    Query(query): Query<AttendanceListQuery>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "attendance:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match attendance::list_periods(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        query.limit,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({"data": data})).into_response(),
        Err(error) => attendance_failure(error),
    }
}

async fn branch_profile_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "products:read",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match branch_settings::get(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
    )
    .await
    {
        Ok(data) => Json(BranchProfileResponse { data }).into_response(),
        Err(branch_settings::BranchSettingsError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "branch_profile_not_found",
            "The requested branch is unavailable.",
        ),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "branch_profile_unavailable",
            "Branch details could not be loaded right now.",
        ),
    }
}

async fn update_branch_profile_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    Json(input): Json<branch_settings::UpdateBranchProfileInput>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "branches:manage",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        );
    };
    match branch_settings::update(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        input,
    )
    .await
    {
        Ok(data) => Json(BranchProfileResponse { data }).into_response(),
        Err(branch_settings::BranchSettingsError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_branch_profile",
            "Provide valid contact details, a complete weekly schedule with up to four non-overlapping intervals per day, and a positive expectedRevision.",
        ),
        Err(branch_settings::BranchSettingsError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "branch_profile_revision_conflict",
            "Branch details changed since they were loaded. Refresh and try again.",
        ),
        Err(branch_settings::BranchSettingsError::NotFound) => auth_error(
            StatusCode::NOT_FOUND,
            "branch_profile_not_found",
            "The requested branch is unavailable.",
        ),
        Err(branch_settings::BranchSettingsError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "branch_profile_unavailable",
            "Branch details could not be saved right now.",
        ),
    }
}

async fn resolve_identity(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<identity::InternalIdentity, Response> {
    let Some(identity_database) = state.identity_database.as_ref() else {
        return Err(auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is not configured.",
        ));
    };
    let Some(authenticator) = state.authenticator.as_ref() else {
        return Err(auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_provider_unavailable",
            "Identity verification is not configured.",
        ));
    };
    let token = match Authenticator::bearer_token(headers) {
        Ok(token) => token,
        Err(_) => {
            return Err(auth_error(
                StatusCode::UNAUTHORIZED,
                "invalid_bearer_token",
                "The bearer token is invalid or expired.",
            ));
        }
    };
    match authenticator.introspect(token).await {
        Ok(identity) => active_identity_lookup(
            identity::find_identity(identity_database, &identity.issuer, &identity.subject).await,
        ),
        Err(auth::AuthenticationError::Unauthorized) => Err(auth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_bearer_token",
            "The bearer token is invalid or expired.",
        )),
        Err(auth::AuthenticationError::Unavailable) => Err(auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_provider_unavailable",
            "Identity verification is temporarily unavailable.",
        )),
    }
}

fn active_identity_lookup(
    lookup: Result<Option<identity::InternalIdentity>, identity::IdentityLookupError>,
) -> Result<identity::InternalIdentity, Response> {
    match lookup {
        Ok(Some(record)) if record.status == identity::IdentityStatus::Active => Ok(record),
        Ok(Some(_)) => Err(auth_error(
            StatusCode::FORBIDDEN,
            "identity_inactive",
            "This account is not active.",
        )),
        Ok(None) => Err(auth_error(
            StatusCode::NOT_FOUND,
            "identity_not_provisioned",
            "This identity is not provisioned for Advanced POS.",
        )),
        Err(_) => Err(auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Account identity lookup is temporarily unavailable.",
        )),
    }
}

async fn list_payment_integration_requests_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "payments:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Payment integration requests are unavailable right now.",
        );
    };
    match payment_requests::list(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({ "data": data })).into_response(),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "payment_integration_requests_unavailable",
            "Payment integration requests could not be loaded right now.",
        ),
    }
}

async fn create_payment_integration_request_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "payments:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let input = match serde_json::from_slice::<payment_requests::PaymentRequestInput>(&body) {
        Ok(input) => input,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_payment_integration_request",
                "Provide a country code, provider name, requested capability and optional safe details.",
            );
        }
    };
    let Some(idempotency_key) = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
    else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_payment_request_idempotency_key",
            "A visible ASCII Idempotency-Key of at most 128 characters is required.",
        );
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The payment integration request could not be submitted right now.",
        );
    };
    match payment_requests::create(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        idempotency_key,
        &body,
        input,
    )
    .await
    {
        Ok(data) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "data": data })),
        )
            .into_response(),
        Err(payment_requests::PaymentRequestError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_payment_integration_request",
            "Check the country code, provider, terminal model, capability and details. Do not include API keys, passwords, PINs or payment account data.",
        ),
        Err(payment_requests::PaymentRequestError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "payment_integration_request_conflict",
            "This request key was already used with different details.",
        ),
        Err(payment_requests::PaymentRequestError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "payment_integration_request_unavailable",
            "The payment integration request could not be submitted right now.",
        ),
    }
}

async fn list_support_requests_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "support:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "Support requests are unavailable right now.",
        );
    };
    match support_requests::list(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
    )
    .await
    {
        Ok(data) => Json(serde_json::json!({ "data": data })).into_response(),
        Err(_) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "support_requests_unavailable",
            "Support requests could not be loaded right now.",
        ),
    }
}

async fn create_support_request_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((tenant_id, branch_id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let (identity, membership) = match authorize_catalog_scope(
        &state,
        &headers,
        &tenant_id,
        &branch_id,
        None,
        "support:write",
    )
    .await
    {
        Ok(value) => value,
        Err(response) => return response,
    };
    if body.is_empty() || body.len() > 8 * 1024 {
        return auth_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "support_request_too_large",
            "Support requests must be 8 KiB or smaller.",
        );
    }
    let input = match serde_json::from_slice::<support_requests::SupportRequestInput>(&body) {
        Ok(input) => input,
        Err(_) => {
            return auth_error(
                StatusCode::BAD_REQUEST,
                "invalid_support_request",
                "Provide a category, short title and optional safe details.",
            );
        }
    };
    let Some(idempotency_key) = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
    else {
        return auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_support_request_idempotency_key",
            "A visible ASCII Idempotency-Key of at most 128 characters is required.",
        );
    };
    let Some(database) = state.identity_database.as_ref() else {
        return auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "identity_database_unavailable",
            "The support request could not be submitted right now.",
        );
    };
    match support_requests::create(
        database,
        &identity.identity_id,
        &tenant_id,
        &membership.membership_id,
        &branch_id,
        idempotency_key,
        &body,
        input,
    )
    .await
    {
        Ok(data) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "data": data })),
        )
            .into_response(),
        Err(support_requests::SupportRequestError::InvalidInput) => auth_error(
            StatusCode::BAD_REQUEST,
            "invalid_support_request",
            "Check the category, title and details. Do not include passwords, PINs, API keys, card data or account numbers.",
        ),
        Err(support_requests::SupportRequestError::Conflict) => auth_error(
            StatusCode::CONFLICT,
            "support_request_conflict",
            "This request key was already used with different details.",
        ),
        Err(support_requests::SupportRequestError::Database) => auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "support_request_unavailable",
            "The support request could not be submitted right now.",
        ),
    }
}

fn auth_error(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (
        status,
        Json(ErrorEnvelope {
            error: ApiError { code, message },
        }),
    )
        .into_response()
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn source() -> Json<SourceResponse> {
    Json(SourceResponse {
        license: "AGPL-3.0-or-later",
        source_url: "https://github.com/Pekay23/smart-pos-rebuild",
    })
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{header, HeaderValue, Request},
    };
    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};
    use sqlx::postgres::PgPoolOptions;
    use std::env;
    use tower::ServiceExt;
    use uuid::Uuid;

    use super::{
        active_identity_lookup, app, app_with_auth, app_with_runtime, BranchPermissionSnapshot,
        StationPermissionSnapshot,
    };
    use crate::identity::{IdentityLookupError, IdentityStatus, InternalIdentity};

    async fn signed_activation_payload(
        client: &reqwest::Client,
        base_url: &str,
        tenant_id: &str,
        branch_id: &str,
        station_id: &str,
        seed: u8,
    ) -> serde_json::Value {
        let challenge_url = format!(
            "{base_url}/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/activation-challenge"
        );
        let response = client
            .post(challenge_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let challenge: serde_json::Value = response.json().await.unwrap();
        let challenge_id = challenge["data"]["challengeId"].as_str().unwrap();
        let nonce = challenge["data"]["nonce"].as_str().unwrap();
        let signing_key = SigningKey::from_bytes(&[seed; 32]);
        let public_key = signing_key.verifying_key().to_bytes();
        let public_key_hex = public_key
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let message = format!(
            "advanced-pos/station-activation/v1\n{tenant_id}\n{branch_id}\n{station_id}\n{challenge_id}\n{nonce}\n{public_key_hex}"
        );
        let signature = signing_key.sign(message.as_bytes()).to_bytes();
        let signature_hex = signature
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        serde_json::json!({
            "devicePublicKey": public_key_hex,
            "challengeId": challenge_id,
            "signature": signature_hex
        })
    }

    async fn signed_lease_payload(
        client: &reqwest::Client,
        base_url: &str,
        tenant_id: &str,
        branch_id: &str,
        station_id: &str,
        membership_id: &str,
        identity_id: &str,
        activation_id: &str,
        seed: u8,
    ) -> serde_json::Value {
        let challenge_url = format!(
            "{base_url}/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/actor-lease-challenge"
        );
        let response = client
            .post(challenge_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let challenge: serde_json::Value = response.json().await.unwrap();
        let challenge_id = challenge["data"]["challengeId"].as_str().unwrap();
        let nonce = challenge["data"]["nonce"].as_str().unwrap();
        let signing_key = SigningKey::from_bytes(&[seed; 32]);
        let message = challenge["data"]["messageToSign"].as_str().unwrap();
        let expected_message = format!(
            "advanced-pos/station-lease/v1\n{tenant_id}\n{branch_id}\n{station_id}\n{membership_id}\n{identity_id}\n{activation_id}\n{challenge_id}\n{nonce}"
        );
        assert_eq!(message, expected_message);
        let signature = signing_key.sign(message.as_bytes()).to_bytes();
        let signature_hex = signature
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        serde_json::json!({
            "challengeId": challenge_id,
            "signature": signature_hex
        })
    }

    async fn signed_financial_proof(
        client: &reqwest::Client,
        challenge_url: &str,
        tenant_id: &str,
        branch_id: &str,
        station_id: &str,
        lease_id: &str,
        lease_token: &str,
        idempotency_key: &str,
        operation: &str,
        resource_id: Option<&str>,
        payload: &serde_json::Value,
        seed: u8,
    ) -> (String, String, Vec<u8>) {
        let api_origin = challenge_url
            .split_once("/v1/tenants/")
            .map(|(origin, _)| origin)
            .expect("financial challenge URL must be a versioned tenant route");
        let heartbeat = client
            .post(format!(
                "{api_origin}/v1/station-actor-leases/{lease_id}/heartbeat"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({"token": lease_token}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            heartbeat.status(),
            axum::http::StatusCode::OK,
            "cashier lease must be live before a financial proof is requested"
        );
        let body = serde_json::to_vec(payload).unwrap();
        let payload_hash = Sha256::digest(&body)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let mut challenge_request = serde_json::json!({
            "payloadHash": payload_hash,
            "operation": operation
        });
        if let Some(resource_id) = resource_id {
            challenge_request["resourceId"] = serde_json::Value::String(resource_id.to_owned());
        }
        let response = client
            .post(challenge_url)
            .bearer_auth("opaque-token")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", idempotency_key)
            .json(&challenge_request)
            .send()
            .await
            .unwrap();
        if response.status() != axum::http::StatusCode::OK {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            panic!("financial challenge for {operation} failed with {status}: {body}");
        }
        let challenge: serde_json::Value = response.json().await.unwrap();
        let challenge_id = challenge["data"]["challengeId"]
            .as_str()
            .unwrap()
            .to_owned();
        let message = challenge["data"]["messageToSign"].as_str().unwrap();
        let base = format!("/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}");
        let target = match (operation, resource_id) {
            ("sale.commit", None) => format!("{base}/sales"),
            ("sale.refund", Some(id)) => format!("{base}/sales/{id}/refunds"),
            ("drawer.open", None) => format!("{base}/drawer-shifts"),
            ("drawer.movement", Some(id)) => format!("{base}/drawer-shifts/{id}/movements"),
            ("drawer.close", Some(id)) => format!("{base}/drawer-shifts/{id}/close"),
            ("supplier.payment", Some(_)) => format!("{base}/supplier-payments"),
            ("expense.payment", Some(_)) => format!("{base}/expenses"),
            _ => panic!("unsupported test operation"),
        };
        assert!(message.starts_with(&format!(
            "advanced-pos/financial-operation/v1\nPOST\n{target}\n{operation}\n{tenant_id}\n{branch_id}\n{station_id}\n{lease_id}\n"
        )));
        let signature = SigningKey::from_bytes(&[seed; 32])
            .sign(message.as_bytes())
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        (challenge_id, signature, body)
    }

    async fn post_test_drawer_movement(
        client: &reqwest::Client,
        base_url: &str,
        tenant_id: &str,
        branch_id: &str,
        station_id: &str,
        shift_id: &str,
        lease_id: &str,
        lease_token: &str,
        idempotency_key: &str,
        movement_type: &str,
        amount_minor: i64,
        destination_location_id: Option<&str>,
        seed: u8,
    ) -> serde_json::Value {
        let challenge_url = format!(
            "{base_url}/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/checkout-challenge"
        );
        let movement_url = format!(
            "{base_url}/v1/tenants/{tenant_id}/branches/{branch_id}/stations/{station_id}/drawer-shifts/{shift_id}/movements"
        );
        let payload = serde_json::json!({
            "movementType": movement_type,
            "amountMinor": amount_minor,
            "note": "Cash ledger integration check",
            "destinationLocationId": destination_location_id
        });
        let (challenge_id, signature, body) = signed_financial_proof(
            client,
            &challenge_url,
            tenant_id,
            branch_id,
            station_id,
            lease_id,
            lease_token,
            idempotency_key,
            "drawer.movement",
            Some(shift_id),
            &payload,
            seed,
        )
        .await;
        let response = client
            .post(movement_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", idempotency_key)
            .header("x-station-challenge-id", challenge_id)
            .header("x-station-proof", signature)
            .body(body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let value: serde_json::Value = response.json().await.unwrap();
        assert_eq!(
            status,
            axum::http::StatusCode::OK,
            "drawer movement failed: {value}"
        );
        value
    }

    #[tokio::test]
    #[ignore = "requires the disposable PostgreSQL verifier environment"]
    async fn hosted_access_endpoints_use_restricted_pooled_database_context() {
        let seed_url = env::var("POS_API_TEST_SEED_DATABASE_URL")
            .expect("the verifier must provide a disposable migration credential");
        let runtime_url = env::var("POS_API_TEST_DATABASE_URL")
            .expect("the verifier must provide the restricted runtime login URL");

        let tenant_a = Uuid::new_v4().to_string();
        let tenant_b = Uuid::new_v4().to_string();
        let identity_id = Uuid::new_v4().to_string();
        let membership_id = Uuid::new_v4().to_string();
        let identity_id_two = Uuid::new_v4().to_string();
        let membership_id_two = Uuid::new_v4().to_string();
        let branch_a = Uuid::new_v4().to_string();
        let branch_a_two = Uuid::new_v4().to_string();
        let branch_b = Uuid::new_v4().to_string();
        let station_a = Uuid::new_v4().to_string();
        let station_a2 = Uuid::new_v4().to_string();
        let station_a3 = Uuid::new_v4().to_string();
        let station_b = Uuid::new_v4().to_string();
        let pickup_station = Uuid::new_v4().to_string();
        let subject = format!("identity-{}", Uuid::new_v4());
        let subject_two = format!("identity-{}", Uuid::new_v4());
        let (issuer, authenticator, introspection_server) =
            crate::auth::tests::start_test_authenticator(&subject).await;
        let (issuer_two, authenticator_two, introspection_server_two) =
            crate::auth::tests::start_test_authenticator(&subject_two).await;

        let seed_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&seed_url)
            .await
            .expect("migration role should connect to the disposable database");
        sqlx::query("INSERT INTO tenants (id, display_name) VALUES ($1::uuid, $2), ($3::uuid, $4)")
            .bind(&tenant_a)
            .bind("Tenant A")
            .bind(&tenant_b)
            .bind("Tenant B")
            .execute(&seed_pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO person_identities (id, issuer, subject, display_name) VALUES ($1::uuid, $2, $3, 'First Cashier')",
        )
        .bind(&identity_id)
        .bind(&issuer)
        .bind(&subject)
        .execute(&seed_pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO person_identities (id, issuer, subject) VALUES ($1::uuid, $2, $3)",
        )
        .bind(&identity_id_two)
        .bind(&issuer_two)
        .bind(&subject_two)
        .execute(&seed_pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO memberships (id, tenant_id, identity_id) VALUES ($1::uuid, $2::uuid, $3::uuid)")
            .bind(&membership_id)
            .bind(&tenant_a)
            .bind(&identity_id)
            .execute(&seed_pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO memberships (id, tenant_id, identity_id) VALUES ($1::uuid, $2::uuid, $3::uuid)")
            .bind(&membership_id_two)
            .bind(&tenant_a)
            .bind(&identity_id_two)
            .execute(&seed_pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO tenant_entitlements (tenant_id, entitlement_key, enabled, numeric_limit) VALUES ($1::uuid, 'stations.active', true, 1)")
            .bind(&tenant_a)
            .execute(&seed_pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO branches (id, tenant_id, code, name, timezone) VALUES ($1::uuid, $2::uuid, 'A', 'A Branch', 'Africa/Accra'), ($3::uuid, $4::uuid, 'B', 'B Branch', 'Africa/Accra'), ($5::uuid, $6::uuid, 'B', 'Other A Branch', 'Africa/Accra')")
            .bind(&branch_a)
            .bind(&tenant_a)
            .bind(&branch_b)
            .bind(&tenant_b)
            .bind(&branch_a_two)
            .bind(&tenant_a)
            .execute(&seed_pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO stations (id, tenant_id, branch_id, code, name) VALUES ($1::uuid, $2::uuid, $3::uuid, 'A1', 'A Station'), ($4::uuid, $5::uuid, $6::uuid, 'A2', 'A Station 2'), ($7::uuid, $8::uuid, $9::uuid, 'A3', 'Owner Station'), ($10::uuid, $11::uuid, $12::uuid, 'B1', 'B Station'), ($13::uuid,$14::uuid,$15::uuid,'P1','Pickup Station')")
            .bind(&station_a)
            .bind(&tenant_a)
            .bind(&branch_a)
            .bind(&station_a2)
            .bind(&tenant_a)
            .bind(&branch_a)
            .bind(&station_a3)
            .bind(&tenant_a)
            .bind(&branch_a)
            .bind(&station_b)
            .bind(&tenant_b)
            .bind(&branch_b)
            .bind(&pickup_station)
            .bind(&tenant_a)
            .bind(&branch_a_two)
            .execute(&seed_pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO membership_role_grants (tenant_id, membership_id, role_key, branch_id, station_id) VALUES ($1::uuid, $2::uuid, 'owner', NULL, NULL), ($1::uuid, $2::uuid, 'cashier', $3::uuid, $4::uuid), ($1::uuid, $2::uuid, 'cashier', $3::uuid, $6::uuid), ($1::uuid, $5::uuid, 'manager', $8::uuid, $9::uuid), ($1::uuid, $5::uuid, 'cashier', $3::uuid, NULL), ($1::uuid, $5::uuid, 'cashier', $3::uuid, $4::uuid), ($1::uuid, $5::uuid, 'cashier', $3::uuid, $7::uuid)")
            .bind(&tenant_a)
            .bind(&membership_id)
            .bind(&branch_a)
            .bind(&station_a)
            .bind(&membership_id_two)
            .bind(&station_a2)
            .bind(&station_a2)
            .bind(&branch_a_two)
            .bind(&pickup_station)
            .execute(&seed_pool)
            .await
            .unwrap();
        seed_pool.close().await;

        let runtime_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&runtime_url)
            .await
            .expect("the restricted runtime login should connect through the API pool");
        let api_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_address = api_listener.local_addr().unwrap();
        let api = app_with_runtime(
            Vec::new(),
            Some(authenticator.clone()),
            Some(runtime_pool.clone()),
        );
        let api_server = tokio::spawn(async move {
            axum::serve(api_listener, api).await.unwrap();
        });
        let runtime_pool_two = PgPoolOptions::new()
            .max_connections(1)
            .connect(&runtime_url)
            .await
            .expect("a second restricted connection should be available for the race check");
        let api_listener_two = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_address_two = api_listener_two.local_addr().unwrap();
        let api_two = app_with_runtime(
            Vec::new(),
            Some(authenticator),
            Some(runtime_pool_two.clone()),
        );
        let api_server_two = tokio::spawn(async move {
            axum::serve(api_listener_two, api_two).await.unwrap();
        });
        let runtime_pool_three = PgPoolOptions::new()
            .max_connections(1)
            .connect(&runtime_url)
            .await
            .expect(
                "a third restricted runtime connection should be available for cashier rotation",
            );
        let api_listener_three = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_address_three = api_listener_three.local_addr().unwrap();
        let api_three = app_with_runtime(
            Vec::new(),
            Some(authenticator_two),
            Some(runtime_pool_three.clone()),
        );
        let api_server_three = tokio::spawn(async move {
            axum::serve(api_listener_three, api_three).await.unwrap();
        });
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();
        let base_url = format!("http://{api_address}");

        let customers_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/customers");
        let customer_create = client.post(&customers_url).bearer_auth("opaque-token")
            .header("Idempotency-Key", "credit-customer-create-001")
            .json(&serde_json::json!({"name":"Credit Terms Customer","phone":"+233200000001","email":null,"notes":null}))
            .send().await.unwrap();
        assert_eq!(customer_create.status(), axum::http::StatusCode::CREATED);
        let customer_body: serde_json::Value = customer_create.json().await.unwrap();
        let customer_id = customer_body["data"]["id"].as_str().unwrap().to_owned();
        let credit_terms_url =
            format!("{base_url}/v1/tenants/{tenant_a}/customers/{customer_id}/credit-terms");
        let empty_terms = client
            .get(&credit_terms_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(empty_terms.status(), axum::http::StatusCode::OK);
        assert_eq!(
            empty_terms.json::<serde_json::Value>().await.unwrap()["data"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        let credit_term_history_url = format!("{credit_terms_url}/GHS/history");
        let empty_credit_history = client
            .get(&credit_term_history_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(empty_credit_history.status(), axum::http::StatusCode::OK);
        assert!(empty_credit_history
            .json::<serde_json::Value>()
            .await
            .unwrap()["data"]
            .as_array()
            .unwrap()
            .is_empty());

        let credit_term_url = format!("{credit_terms_url}/GHS");
        let first_term_body = serde_json::json!({"expectedRevision":0,"creditLimitMinor":6000,"dueDays":30,"reason":"Approved starter account"});
        let first_term = client
            .put(&credit_term_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "credit-term-ghs-001")
            .json(&first_term_body)
            .send()
            .await
            .unwrap();
        assert_eq!(first_term.status(), axum::http::StatusCode::OK);
        let first_term_result: serde_json::Value = first_term.json().await.unwrap();
        assert_eq!(first_term_result["data"]["revision"], 1);
        assert_eq!(first_term_result["data"]["creditLimitMinor"], "6000");
        assert_eq!(first_term_result["data"]["currencyCode"], "GHS");
        let first_term_retry = client
            .put(&credit_term_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "credit-term-ghs-001")
            .json(&first_term_body)
            .send()
            .await
            .unwrap();
        assert_eq!(first_term_retry.status(), axum::http::StatusCode::OK);
        assert_eq!(
            first_term_retry.json::<serde_json::Value>().await.unwrap(),
            first_term_result
        );
        let changed_term_retry = client.put(&credit_term_url).bearer_auth("opaque-token")
            .header("Idempotency-Key", "credit-term-ghs-001")
            .json(&serde_json::json!({"expectedRevision":0,"creditLimitMinor":7000,"dueDays":30,"reason":"Changed request"}))
            .send().await.unwrap();
        assert_eq!(
            changed_term_retry.status(),
            axum::http::StatusCode::CONFLICT
        );
        let stale_term = client
            .put(&credit_term_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "credit-term-ghs-stale")
            .json(&first_term_body)
            .send()
            .await
            .unwrap();
        assert_eq!(stale_term.status(), axum::http::StatusCode::CONFLICT);

        let second_term_body = serde_json::json!({"expectedRevision":1,"creditLimitMinor":9000,"dueDays":45,"reason":"Limit reviewed by owner"});
        let second_term = client
            .put(&credit_term_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "credit-term-ghs-002")
            .json(&second_term_body)
            .send()
            .await
            .unwrap();
        assert_eq!(second_term.status(), axum::http::StatusCode::OK);
        let second_term_result: serde_json::Value = second_term.json().await.unwrap();
        assert_eq!(second_term_result["data"]["revision"], 2);
        assert_eq!(second_term_result["data"]["creditLimitMinor"], "9000");
        let credit_history = client
            .get(&credit_term_history_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(credit_history.status(), axum::http::StatusCode::OK);
        let credit_history_body: serde_json::Value = credit_history.json().await.unwrap();
        assert_eq!(credit_history_body["data"].as_array().unwrap().len(), 2);
        assert_eq!(credit_history_body["data"][0]["revision"], 2);
        assert_eq!(
            credit_history_body["data"][0]["previousCreditLimitMinor"],
            "6000"
        );
        assert_eq!(credit_history_body["data"][0]["creditLimitMinor"], "9000");
        assert_eq!(
            credit_history_body["data"][0]["reason"],
            "Limit reviewed by owner"
        );
        assert_eq!(credit_history_body["data"][0]["actorName"], "First Cashier");
        assert_eq!(credit_history_body["data"][1]["revision"], 1);
        let manager_credit_history = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/customers/{customer_id}/credit-terms/GHS/history"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            manager_credit_history.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let terms_for_branch_manager = client.get(&format!("http://{api_address_three}/v1/tenants/{tenant_a}/customers/{customer_id}/credit-terms"))
            .bearer_auth("opaque-token").send().await.unwrap();
        assert_eq!(
            terms_for_branch_manager.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let final_terms = client
            .get(&credit_terms_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(final_terms.status(), axum::http::StatusCode::OK);
        let final_terms_body: serde_json::Value = final_terms.json().await.unwrap();
        assert_eq!(final_terms_body["data"].as_array().unwrap().len(), 1);
        assert_eq!(final_terms_body["data"][0]["revision"], 2);

        let cash_locations_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/cash-locations");
        let safe_request = serde_json::json!({"name":"Main Safe","locationType":"safe"});
        let safe_create = client
            .post(&cash_locations_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "cash-location-safe-001")
            .json(&safe_request)
            .send()
            .await
            .unwrap();
        assert_eq!(safe_create.status(), axum::http::StatusCode::OK);
        let safe_body: serde_json::Value = safe_create.json().await.unwrap();
        let safe_id = safe_body["data"]["location"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(safe_body["data"]["location"]["initialized"], false);
        let safe_retry = client
            .post(&cash_locations_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "cash-location-safe-001")
            .json(&safe_request)
            .send()
            .await
            .unwrap();
        assert_eq!(safe_retry.status(), axum::http::StatusCode::OK);
        assert_eq!(
            safe_retry.json::<serde_json::Value>().await.unwrap()["data"]["replayed"],
            true
        );
        let changed_safe = client
            .post(&cash_locations_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "cash-location-safe-001")
            .json(&serde_json::json!({"name":"Other Safe","locationType":"safe"}))
            .send()
            .await
            .unwrap();
        assert_eq!(changed_safe.status(), axum::http::StatusCode::CONFLICT);
        let register_location = client
            .post(&cash_locations_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "cash-location-register-001")
            .json(&serde_json::json!({"name":"Register","locationType":"register"}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            register_location.status(),
            axum::http::StatusCode::BAD_REQUEST
        );

        let opening_url = format!("{cash_locations_url}/{safe_id}/opening");
        let opening_body = serde_json::json!({"amountMinor":12500});
        let opening = client
            .post(&opening_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "cash-opening-safe-001")
            .json(&opening_body)
            .send()
            .await
            .unwrap();
        assert_eq!(opening.status(), axum::http::StatusCode::OK);
        assert_eq!(
            opening.json::<serde_json::Value>().await.unwrap()["data"]["location"]["balanceMinor"],
            12500
        );
        let opening_retry = client
            .post(&opening_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "cash-opening-safe-001")
            .json(&opening_body)
            .send()
            .await
            .unwrap();
        assert_eq!(
            opening_retry.json::<serde_json::Value>().await.unwrap()["data"]["replayed"],
            true
        );
        let second_safe = client
            .post(&cash_locations_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "cash-location-petty-001")
            .json(&serde_json::json!({"name":"Petty Cash","locationType":"petty_cash"}))
            .send()
            .await
            .unwrap();
        assert_eq!(second_safe.status(), axum::http::StatusCode::OK);
        let second_safe_id = second_safe.json::<serde_json::Value>().await.unwrap()["data"]
            ["location"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let second_opening = client
            .post(format!("{cash_locations_url}/{second_safe_id}/opening"))
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "cash-opening-petty-001")
            .json(&serde_json::json!({"amountMinor":30000}))
            .send()
            .await
            .unwrap();
        assert_eq!(second_opening.status(), axum::http::StatusCode::OK);
        let transfer_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/cash-transfers");
        let transfer_body = serde_json::json!({"fromLocationId":safe_id,"toLocationId":second_safe_id,"amountMinor":4500,"note":"Move float to petty cash"});
        let transfer = client
            .post(&transfer_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "cash-transfer-001")
            .json(&transfer_body)
            .send()
            .await
            .unwrap();
        assert_eq!(transfer.status(), axum::http::StatusCode::OK);
        let transfer_result: serde_json::Value = transfer.json().await.unwrap();
        let transfer_id = transfer_result["data"]["id"].as_str().unwrap().to_owned();
        let transfer_retry = client
            .post(&transfer_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "cash-transfer-001")
            .json(&transfer_body)
            .send()
            .await
            .unwrap();
        assert_eq!(
            transfer_retry.json::<serde_json::Value>().await.unwrap()["data"]["replayed"],
            true
        );
        let overdraw=client.post(&transfer_url).bearer_auth("opaque-token")
            .header("Idempotency-Key","cash-transfer-overdraw").json(&serde_json::json!({"fromLocationId":safe_id,"toLocationId":second_safe_id,"amountMinor":9000,"note":"Too much"})).send().await.unwrap();
        assert_eq!(overdraw.status(), axum::http::StatusCode::CONFLICT);
        let cashier_cash_location=client.post(format!("http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/cash-locations"))
            .bearer_auth("opaque-token").header("Idempotency-Key","cash-location-cashier-denied")
            .json(&serde_json::json!({"name":"Cashier Safe","locationType":"safe"})).send().await.unwrap();
        assert_eq!(
            cashier_cash_location.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let location_history = client
            .get(format!("{cash_locations_url}/{safe_id}/events"))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(location_history.status(), axum::http::StatusCode::OK);
        let location_events = location_history.json::<serde_json::Value>().await.unwrap();
        assert_eq!(location_events["data"].as_array().unwrap().len(), 2);
        let cash_workspace = client
            .get(&cash_locations_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(cash_workspace.status(), axum::http::StatusCode::OK);
        let cash_workspace_body: serde_json::Value = cash_workspace.json().await.unwrap();
        let safe_balance = cash_workspace_body["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == safe_id)
            .unwrap()["balanceMinor"]
            .as_i64()
            .unwrap();
        let petty_balance = cash_workspace_body["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == second_safe_id)
            .unwrap()["balanceMinor"]
            .as_i64()
            .unwrap();
        assert_eq!(safe_balance, 8000);
        assert_eq!(petty_balance, 34500);
        assert!(cash_workspace_body.get("reconciliation").is_none());
        let verify_cash_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&seed_url)
            .await
            .unwrap();
        let (transfer_count,leg_count): (i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM hosted_cash_transfers WHERE id=$1::uuid),(SELECT count(*) FROM hosted_cash_location_events WHERE source_transfer_id=$1::uuid)")
            .bind(&transfer_id).fetch_one(&verify_cash_pool).await.unwrap();
        assert_eq!((transfer_count, leg_count), (1, 2));
        verify_cash_pool.close().await;

        let supplier_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/suppliers");
        let supplier_payload = serde_json::json!({
            "name": "  North Mill Supply ", "contactPerson": " Ama Mensah ",
            "email": " ama@supply.example ", "phone": "+233 20 111 2222",
            "address": "Accra", "notes": null
        });
        let created_supplier = client
            .post(&supplier_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-create-001")
            .json(&supplier_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(created_supplier.status(), axum::http::StatusCode::CREATED);
        let created_supplier_body: serde_json::Value = created_supplier.json().await.unwrap();
        assert_eq!(created_supplier_body["data"]["name"], "North Mill Supply");
        assert_eq!(created_supplier_body["data"]["contactPerson"], "Ama Mensah");
        let supplier_id = created_supplier_body["data"]["id"].as_str().unwrap();
        let supplier_image_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/entities/supplier/{supplier_id}/image"
        );
        let png_bytes = b"\x89PNG\r\n\x1a\nprivate-image-fixture";
        let uploaded_image = client
            .put(&supplier_image_url)
            .bearer_auth("opaque-token")
            .header("Content-Type", "image/png")
            .body(png_bytes.to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(uploaded_image.status(), axum::http::StatusCode::NO_CONTENT);
        let fetched_image = client
            .get(&supplier_image_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(fetched_image.status(), axum::http::StatusCode::OK);
        assert_eq!(
            fetched_image.headers().get("content-type").unwrap(),
            "image/png"
        );
        assert_eq!(
            fetched_image.headers().get("cache-control").unwrap(),
            "private, no-store"
        );
        assert_eq!(
            fetched_image
                .headers()
                .get("x-content-type-options")
                .unwrap(),
            "nosniff"
        );
        assert_eq!(fetched_image.bytes().await.unwrap().as_ref(), png_bytes);
        let invalid_signature = client
            .put(&supplier_image_url)
            .bearer_auth("opaque-token")
            .header("Content-Type", "image/png")
            .body(&b"<svg/>"[..])
            .send()
            .await
            .unwrap();
        assert_eq!(
            invalid_signature.status(),
            axum::http::StatusCode::BAD_REQUEST
        );
        let cashier_image_url = format!(
            "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/entities/supplier/{supplier_id}/image"
        );
        let cashier_image_upload = client
            .put(&cashier_image_url)
            .bearer_auth("opaque-token")
            .header("Content-Type", "image/png")
            .body(png_bytes.to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_image_upload.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let deleted_image = client
            .delete(&supplier_image_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(deleted_image.status(), axum::http::StatusCode::NO_CONTENT);
        let missing_image = client
            .get(&supplier_image_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(missing_image.status(), axum::http::StatusCode::NOT_FOUND);
        let supplier_retry = client
            .post(&supplier_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-create-001")
            .json(&supplier_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(supplier_retry.status(), axum::http::StatusCode::CREATED);
        let supplier_retry_body: serde_json::Value = supplier_retry.json().await.unwrap();
        assert_eq!(supplier_retry_body["data"]["id"], supplier_id);
        let supplier_search = client
            .get(format!("{supplier_url}?query=north&pageSize=10"))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(supplier_search.status(), axum::http::StatusCode::OK);
        let supplier_search_body: serde_json::Value = supplier_search.json().await.unwrap();
        assert_eq!(supplier_search_body["data"][0]["id"], supplier_id);
        let supplier_item_url = format!("{supplier_url}/{supplier_id}");
        let updated_supplier = client.patch(&supplier_item_url).bearer_auth("opaque-token")
            .json(&serde_json::json!({"name":"North Mill Supply Ltd","contactPerson":"Ama Mensah","email":"ama@supply.example","phone":"+233 20 111 2222","address":"Accra","notes":"Timber","expectedRevision":created_supplier_body["data"]["revision"]}))
            .send().await.unwrap();
        assert_eq!(updated_supplier.status(), axum::http::StatusCode::OK);
        let updated_supplier_body: serde_json::Value = updated_supplier.json().await.unwrap();
        assert_eq!(updated_supplier_body["data"]["revision"], 2);
        let cashier_supplier_list = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/suppliers"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_supplier_list.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let supplier_status_url = format!("{supplier_item_url}/status");
        let archived_supplier = client
            .patch(&supplier_status_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({"status":"archived","expectedRevision":2}))
            .send()
            .await
            .unwrap();
        assert_eq!(archived_supplier.status(), axum::http::StatusCode::OK);
        let archived_supplier_body: serde_json::Value = archived_supplier.json().await.unwrap();
        assert_eq!(archived_supplier_body["data"]["status"], "archived");
        let restored_supplier = client
            .patch(&supplier_status_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({"status":"active","expectedRevision":3}))
            .send()
            .await
            .unwrap();
        assert_eq!(restored_supplier.status(), axum::http::StatusCode::OK);

        let customer_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a}/customers"
        );
        let customer_payload = serde_json::json!({
            "name": "  Ama Mensah ",
            "phone": " +233 20 123 4567 ",
            "email": " ama@example.test ",
            "notes": null
        });
        let first_customer = client
            .post(&customer_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "customer-create-001")
            .json(&customer_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(first_customer.status(), axum::http::StatusCode::CREATED);
        let first_customer_body: serde_json::Value = first_customer.json().await.unwrap();
        assert_eq!(first_customer_body["data"]["name"], "Ama Mensah");
        assert_eq!(first_customer_body["data"]["phone"], "+233 20 123 4567");
        let customer_id = first_customer_body["data"]["id"].as_str().unwrap();
        let manager_created_customer = client
            .post(format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/customers"))
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "customer-manager-create-001")
            .json(&serde_json::json!({ "name": "Kojo Manager", "phone": null, "email": null, "notes": null }))
            .send().await.unwrap();
        assert_eq!(
            manager_created_customer.status(),
            axum::http::StatusCode::CREATED
        );
        let manager_created_customer_body: serde_json::Value =
            manager_created_customer.json().await.unwrap();
        assert_eq!(
            manager_created_customer_body["data"]["name"],
            "Kojo Manager"
        );
        let manager_customer_id = manager_created_customer_body["data"]["id"]
            .as_str()
            .unwrap();

        let exact_customer_retry = client
            .post(&customer_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "customer-create-001")
            .json(&customer_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            exact_customer_retry.status(),
            axum::http::StatusCode::CREATED
        );
        let exact_customer_retry_body: serde_json::Value =
            exact_customer_retry.json().await.unwrap();
        assert_eq!(exact_customer_retry_body["data"]["id"], customer_id);

        let changed_customer_retry = client
            .post(&customer_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "customer-create-001")
            .json(&serde_json::json!({ "name": "Different details" }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            changed_customer_retry.status(),
            axum::http::StatusCode::CONFLICT
        );

        let customer_list = client
            .get(format!(
                "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a2}/customers?query=ama&pageSize=1"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(customer_list.status(), axum::http::StatusCode::OK);
        let customer_list_body: serde_json::Value = customer_list.json().await.unwrap();
        assert!(customer_list_body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|customer| customer["id"] == customer_id));

        let credit_position_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a2}/customers/{customer_id}/credit-position"
        );
        let credit_position = client
            .get(&credit_position_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(credit_position.status(), axum::http::StatusCode::OK);
        let credit_position_body: serde_json::Value = credit_position.json().await.unwrap();
        assert_eq!(credit_position_body["data"]["currencyCode"], "GHS");
        assert_eq!(credit_position_body["data"]["hasCreditTerms"], false);
        assert_eq!(
            credit_position_body["data"]["termsRevision"],
            serde_json::Value::Null
        );
        assert_eq!(credit_position_body["data"]["creditLimitMinor"], "0");
        assert_eq!(credit_position_body["data"]["outstandingMinor"], "0");
        assert_eq!(credit_position_body["data"]["availableMinor"], "0");

        let cross_tenant_credit_position = client
            .get(format!(
                "{base_url}/v1/tenants/{tenant_b}/branches/{branch_b}/stations/{station_b}/customers/{customer_id}/credit-position"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cross_tenant_credit_position.status(),
            axum::http::StatusCode::NOT_FOUND
        );

        let customer_item_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/customers/{customer_id}");
        let customer_edit = client
            .patch(&customer_item_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "name": "Ama Mensah Updated",
                "phone": "+233 20 123 4567",
                "email": "ama@example.test",
                "notes": "walk-in",
                "expectedRevision": first_customer_body["data"]["revision"]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(customer_edit.status(), axum::http::StatusCode::OK);
        let customer_edit_body: serde_json::Value = customer_edit.json().await.unwrap();
        assert_eq!(customer_edit_body["data"]["name"], "Ama Mensah Updated");
        assert_eq!(customer_edit_body["data"]["revision"], 2);
        let stale_customer_edit = client
            .patch(&customer_item_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "name": "Stale edit", "phone": null, "email": null, "notes": null,
                "expectedRevision": first_customer_body["data"]["revision"]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            stale_customer_edit.status(),
            axum::http::StatusCode::CONFLICT
        );
        let cashier_customer_edit = client
            .patch(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/customers/{customer_id}"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "name": "Unauthorized edit", "phone": null, "email": null, "notes": null,
                "expectedRevision": 2
            }))
            .send().await.unwrap();
        assert_eq!(
            cashier_customer_edit.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let customer_status_url = format!("{customer_item_url}/status");
        let archived_customer = client
            .patch(&customer_status_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({ "status": "archived", "expectedRevision": 2 }))
            .send()
            .await
            .unwrap();
        assert_eq!(archived_customer.status(), axum::http::StatusCode::OK);
        let archived_customer_body: serde_json::Value = archived_customer.json().await.unwrap();
        assert_eq!(archived_customer_body["data"]["status"], "archived");
        assert_eq!(archived_customer_body["data"]["revision"], 3);
        let archived_customer_list = client
            .get(format!("{customer_url}?query=ama&includeArchived=true"))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(archived_customer_list.status(), axum::http::StatusCode::OK);
        let archived_customer_list_body: serde_json::Value =
            archived_customer_list.json().await.unwrap();
        assert_eq!(archived_customer_list_body["data"][0]["status"], "archived");
        let cashier_archived_customer_list = client
            .get(format!("http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/customers?includeArchived=true"))
            .bearer_auth("opaque-token")
            .send().await.unwrap();
        assert_eq!(
            cashier_archived_customer_list.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let restored_customer = client
            .patch(&customer_status_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({ "status": "active", "expectedRevision": 3 }))
            .send()
            .await
            .unwrap();
        assert_eq!(restored_customer.status(), axum::http::StatusCode::OK);
        let restored_customer_body: serde_json::Value = restored_customer.json().await.unwrap();
        assert_eq!(restored_customer_body["data"]["status"], "active");
        assert_eq!(restored_customer_body["data"]["revision"], 4);
        let second_customer_page = client
            .get(format!(
                "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a2}/customers?query=ama&offset=1&pageSize=1"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(second_customer_page.status(), axum::http::StatusCode::OK);
        let second_customer_page_body: serde_json::Value =
            second_customer_page.json().await.unwrap();
        assert!(second_customer_page_body["data"]
            .as_array()
            .unwrap()
            .is_empty());
        let invalid_customer_page = client
            .get(format!(
                "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a2}/customers?pageSize=101"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            invalid_customer_page.status(),
            axum::http::StatusCode::BAD_REQUEST
        );

        let customer_from_another_tenant_is_hidden = client
            .post(format!(
                "http://{api_address_three}/v1/tenants/{tenant_b}/branches/{branch_b}/stations/{station_b}/customers"
            ))
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "customer-other-tenant")
            .json(&serde_json::json!({ "name": "Unauthorized" }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            customer_from_another_tenant_is_hidden.status(),
            axum::http::StatusCode::NOT_FOUND
        );

        let tenants = client
            .get(format!("{base_url}/v1/me/tenants"))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(tenants.status(), axum::http::StatusCode::OK);
        let tenant_body: serde_json::Value = tenants.json().await.unwrap();
        assert_eq!(tenant_body["data"].as_array().unwrap().len(), 1);
        assert_eq!(tenant_body["data"][0]["tenantId"], tenant_a);
        assert_eq!(
            tenant_body["data"][0]["branches"].as_array().unwrap().len(),
            2
        );
        let source_branch_access = tenant_body["data"][0]["branches"]
            .as_array()
            .unwrap()
            .iter()
            .find(|branch| branch["branchId"] == branch_a)
            .expect("tenant access includes the requested source branch");
        let assigned_stations = source_branch_access["stations"].as_array().unwrap();
        assert_eq!(assigned_stations.len(), 3);
        assert!(assigned_stations
            .iter()
            .any(|station| station["stationId"] == station_a));

        let station_capacity_url = format!("{base_url}/v1/tenants/{tenant_a}/station-capacity");
        let station_capacity = client
            .get(&station_capacity_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(station_capacity.status(), axum::http::StatusCode::OK);
        let station_capacity_body: serde_json::Value = station_capacity.json().await.unwrap();
        assert_eq!(station_capacity_body["data"]["entitlementActive"], true);
        assert_eq!(station_capacity_body["data"]["stationLimit"], 1);
        assert_eq!(station_capacity_body["data"]["activeStations"], 0);
        assert_eq!(station_capacity_body["data"]["availableSlots"], 1);
        let cashier_station_capacity = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/station-capacity"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_station_capacity.status(),
            axum::http::StatusCode::FORBIDDEN
        );

        let payment_options_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/payment-options");
        let created_payment_option = client
            .post(&payment_options_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "methodKey": "mtn-momo",
                "label": "MTN Mobile Money",
                "tenderKind": "mobile_money",
                "providerName": "Paystack",
                "countryCode": "gh",
                "requiresReference": true
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            created_payment_option.status(),
            axum::http::StatusCode::CREATED
        );
        let created_payment_option_body: serde_json::Value =
            created_payment_option.json().await.unwrap();
        let payment_option_id = created_payment_option_body["data"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(created_payment_option_body["data"]["countryCode"], "GH");
        assert_eq!(
            created_payment_option_body["data"]["recordingMode"],
            "manual"
        );

        let station_payment_options = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a}/payment-options"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(station_payment_options.status(), axum::http::StatusCode::OK);
        let station_options_body: serde_json::Value = station_payment_options.json().await.unwrap();
        assert_eq!(station_options_body["data"].as_array().unwrap().len(), 1);
        assert_eq!(station_options_body["data"][0]["providerName"], "Paystack");

        let cashier_cannot_create_payment_option = client
            .post(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/payment-options"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "methodKey": "cashier-card",
                "label": "Unauthorized card",
                "tenderKind": "card",
                "providerName": null,
                "countryCode": null,
                "requiresReference": true
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_cannot_create_payment_option.status(),
            axum::http::StatusCode::FORBIDDEN
        );

        let payment_requests_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/payment-integration-requests"
        );
        let payment_request_body = serde_json::json!({
            "countryCode": "gh",
            "providerName": "Example Bank",
            "terminalModel": "PX-100",
            "capability": "physical_terminal",
            "businessImpact": "workaround_available",
            "details": "Cashiers need to record terminal approval references."
        });
        let payment_request = client
            .post(&payment_requests_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "integration-request-1")
            .json(&payment_request_body)
            .send()
            .await
            .unwrap();
        assert_eq!(payment_request.status(), axum::http::StatusCode::CREATED);
        let payment_request_response: serde_json::Value = payment_request.json().await.unwrap();
        let payment_request_id = payment_request_response["data"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(payment_request_response["data"]["countryCode"], "GH");
        assert_eq!(payment_request_response["data"]["status"], "submitted");
        assert_eq!(
            payment_request_response["data"]["businessImpact"],
            "workaround_available"
        );

        let replayed_payment_request = client
            .post(&payment_requests_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "integration-request-1")
            .json(&payment_request_body)
            .send()
            .await
            .unwrap();
        assert_eq!(
            replayed_payment_request.status(),
            axum::http::StatusCode::CREATED
        );
        let replayed_payment_request_body: serde_json::Value =
            replayed_payment_request.json().await.unwrap();
        assert_eq!(
            replayed_payment_request_body["data"]["id"],
            payment_request_id
        );

        let changed_payment_request = client
            .post(&payment_requests_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "integration-request-1")
            .json(&serde_json::json!({
                "countryCode": "NG",
                "providerName": "Other Bank",
                "capability": "bank_transfer",
                "businessImpact": "exploring"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            changed_payment_request.status(),
            axum::http::StatusCode::CONFLICT
        );

        let unsafe_payment_request = client
            .post(&payment_requests_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "integration-request-unsafe")
            .json(&serde_json::json!({
                "countryCode": "GH",
                "providerName": "Example Bank",
                "capability": "physical_terminal",
                "businessImpact": "business_blocked",
                "details": "My terminal PIN is 1234"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            unsafe_payment_request.status(),
            axum::http::StatusCode::BAD_REQUEST
        );

        let listed_payment_requests = client
            .get(&payment_requests_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(listed_payment_requests.status(), axum::http::StatusCode::OK);
        let listed_payment_requests_body: serde_json::Value =
            listed_payment_requests.json().await.unwrap();
        assert_eq!(
            listed_payment_requests_body["data"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            listed_payment_requests_body["data"][0]["id"],
            payment_request_id
        );

        let cashier_cannot_submit_payment_request = client
            .post(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/payment-integration-requests"
            ))
            .bearer_auth("opaque-token")
            .header("idempotency-key", "cashier-integration-request")
            .json(&payment_request_body)
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_cannot_submit_payment_request.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let cashier_cannot_list_payment_requests = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/payment-integration-requests"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_cannot_list_payment_requests.status(),
            axum::http::StatusCode::FORBIDDEN
        );

        let support_requests_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/support-requests");
        let support_request_payload = serde_json::json!({
            "category": "bug",
            "title": "Held sale cannot be restored",
            "details": "The parked order disappears after reconnecting the scanner.",
            "diagnosticsConsent": true,
            "diagnosticsAppVersion": "0.1.0",
            "diagnosticsPlatform": "web"
        });
        let support_request = client
            .post(&support_requests_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "support-request-http-1")
            .json(&support_request_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(support_request.status(), axum::http::StatusCode::CREATED);
        let support_request_body: serde_json::Value = support_request.json().await.unwrap();
        let support_request_id = support_request_body["data"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(support_request_body["data"]["status"], "submitted");
        assert_eq!(support_request_body["data"]["revision"], 1);
        assert_eq!(support_request_body["data"]["diagnosticsConsent"], true);
        assert_eq!(
            support_request_body["data"]["diagnosticsAppVersion"],
            "0.1.0"
        );
        assert_eq!(support_request_body["data"]["diagnosticsPlatform"], "web");

        let support_request_replay = client
            .post(&support_requests_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "support-request-http-1")
            .json(&support_request_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            support_request_replay.status(),
            axum::http::StatusCode::CREATED
        );
        let support_request_replay_body: serde_json::Value =
            support_request_replay.json().await.unwrap();
        assert_eq!(
            support_request_replay_body["data"]["id"],
            support_request_id
        );

        let unconsented_support_request = client
            .post(&support_requests_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "support-request-http-no-diagnostics")
            .json(&serde_json::json!({
                "category": "training",
                "title": "How do I restore a held order?"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            unconsented_support_request.status(),
            axum::http::StatusCode::CREATED
        );
        let unconsented_support_request_body: serde_json::Value =
            unconsented_support_request.json().await.unwrap();
        assert_eq!(
            unconsented_support_request_body["data"]["diagnosticsConsent"],
            false
        );
        assert!(unconsented_support_request_body["data"]["diagnosticsAppVersion"].is_null());
        assert!(unconsented_support_request_body["data"]["diagnosticsPlatform"].is_null());

        let conflicting_support_request = client
            .post(&support_requests_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "support-request-http-1")
            .json(&serde_json::json!({
                "category": "feature_request",
                "title": "Different request"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            conflicting_support_request.status(),
            axum::http::StatusCode::CONFLICT
        );

        let unsafe_support_request = client
            .post(&support_requests_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "support-request-http-unsafe")
            .json(&serde_json::json!({
                "category": "other",
                "title": "Login problem",
                "details": "My password is shared with the cashier."
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            unsafe_support_request.status(),
            axum::http::StatusCode::BAD_REQUEST
        );

        let unconsented_diagnostics = client
            .post(&support_requests_url)
            .bearer_auth("opaque-token")
            .header(
                "idempotency-key",
                "support-request-http-unconsented-diagnostics",
            )
            .json(&serde_json::json!({
                "category": "other",
                "title": "Unconsented diagnostics",
                "diagnosticsAppVersion": "0.1.0",
                "diagnosticsPlatform": "web"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            unconsented_diagnostics.status(),
            axum::http::StatusCode::BAD_REQUEST
        );

        let unsupported_diagnostics = client
            .post(&support_requests_url)
            .bearer_auth("opaque-token")
            .header(
                "idempotency-key",
                "support-request-http-unsupported-diagnostics",
            )
            .json(&serde_json::json!({
                "category": "other",
                "title": "Unsupported diagnostics",
                "diagnosticsConsent": true,
                "diagnosticsAppVersion": "0.1.0",
                "diagnosticsPlatform": "web Chrome 132 Windows C:\\Users\\Pekay"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            unsupported_diagnostics.status(),
            axum::http::StatusCode::BAD_REQUEST
        );

        let raw_diagnostics_rejected = client
            .post(&support_requests_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "support-request-http-raw-diagnostics")
            .json(&serde_json::json!({
                "category": "other",
                "title": "Raw diagnostics request",
                "diagnosticsConsent": true,
                "diagnosticsAppVersion": "0.1.0",
                "diagnosticsPlatform": "web",
                "rawLogs": "session and payment trace"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            raw_diagnostics_rejected.status(),
            axum::http::StatusCode::BAD_REQUEST
        );

        let listed_support_requests = client
            .get(&support_requests_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(listed_support_requests.status(), axum::http::StatusCode::OK);
        let listed_support_requests_body: serde_json::Value =
            listed_support_requests.json().await.unwrap();
        assert_eq!(
            listed_support_requests_body["data"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(listed_support_requests_body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|request| request["id"] == support_request_id));
        assert!(listed_support_requests_body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|request| request["diagnosticsConsent"] == false));

        let cashier_cannot_read_support_requests = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/support-requests"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_cannot_read_support_requests.status(),
            axum::http::StatusCode::FORBIDDEN
        );

        let updated_payment_option = client
            .patch(format!("{payment_options_url}/{payment_option_id}"))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "label": "MTN Mobile Money",
                "tenderKind": "mobile_money",
                "providerName": "Paystack",
                "countryCode": "GH",
                "requiresReference": true,
                "enabled": false,
                "expectedRevision": 1
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(updated_payment_option.status(), axum::http::StatusCode::OK);
        let updated_payment_option_body: serde_json::Value =
            updated_payment_option.json().await.unwrap();
        assert_eq!(updated_payment_option_body["data"]["enabled"], false);
        assert_eq!(updated_payment_option_body["data"]["revision"], 2);
        let stale_payment_option = client
            .patch(format!("{payment_options_url}/{payment_option_id}"))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "label": "Stale edit",
                "tenderKind": "mobile_money",
                "providerName": "Paystack",
                "countryCode": "GH",
                "requiresReference": true,
                "enabled": true,
                "expectedRevision": 1
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            stale_payment_option.status(),
            axum::http::StatusCode::CONFLICT
        );

        let branch_products_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/products");
        let initial_catalog = client
            .get(&branch_products_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(initial_catalog.status(), axum::http::StatusCode::OK);
        let empty_catalog: serde_json::Value = initial_catalog.json().await.unwrap();
        assert_eq!(empty_catalog["data"].as_array().unwrap().len(), 0);

        let created_product = client
            .post(&branch_products_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "sku": "TEA-001",
                "barcode": "0123456789012",
                "name": "Black Tea",
                "description": "Hosted catalog fixture",
                "category": "Grocery",
                "priceMinor": 1234,
                "costMinor": 700,
                "taxRateBps": 1500,
                "openingStock": 7,
                "reorderPoint": 4,
                "targetStockQuantity": 12
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(created_product.status(), axum::http::StatusCode::CREATED);
        let product_body: serde_json::Value = created_product.json().await.unwrap();
        let product_id = product_body["data"]["productId"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(product_body["data"]["sku"], "TEA-001");
        assert_eq!(product_body["data"]["priceMinor"], 1234);
        assert_eq!(product_body["data"]["taxRateBps"], 1500);
        assert_eq!(product_body["data"]["stockQuantity"], 7);
        assert_eq!(product_body["data"]["currencyCode"], "GHS");
        assert!(product_body["data"].get("costMinor").is_none());

        let verification_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&seed_url)
            .await
            .unwrap();
        let product_effects = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            "SELECT
                (SELECT count(*) FROM catalog_products WHERE tenant_id = $1::uuid AND id = $2::uuid),
                (SELECT count(*) FROM branch_catalog_items WHERE tenant_id = $1::uuid AND branch_id = $3::uuid AND product_id = $2::uuid AND stock_quantity = 7),
                (SELECT count(*) FROM stock_movements WHERE tenant_id = $1::uuid AND branch_id = $3::uuid AND product_id = $2::uuid AND movement_kind = 'opening_balance' AND quantity_delta = 7),
                (SELECT count(*) FROM audit_events WHERE tenant_id = $1::uuid AND entity_type = 'catalog_product' AND entity_id = $2::text)"
        )
        .bind(&tenant_a)
        .bind(&product_id)
        .bind(&branch_a)
        .fetch_one(&verification_pool)
        .await
        .unwrap();
        assert_eq!(product_effects, (1, 1, 1, 1));

        let purchase_orders_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/purchase-orders");
        let purchase_payload = serde_json::json!({
            "supplierId": supplier_id,
            "reference": "TIMBER-QUOTE-14",
            "items": [{ "productId": product_id, "quantity": 5, "expectedUnitCostMinor": 700 }]
        });
        let created_order = client
            .post(&purchase_orders_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "hosted-purchase-create-001")
            .json(&purchase_payload)
            .send()
            .await
            .unwrap();
        let created_order_status = created_order.status();
        let created_order_text = created_order.text().await.unwrap();
        assert_eq!(
            created_order_status,
            axum::http::StatusCode::CREATED,
            "{created_order_text}"
        );
        let created_order_body: serde_json::Value =
            serde_json::from_str(&created_order_text).unwrap();
        assert_eq!(created_order_body["data"]["status"], "draft");
        let order_id = created_order_body["data"]["id"].as_str().unwrap();
        let create_order_replay = client
            .post(&purchase_orders_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "hosted-purchase-create-001")
            .json(&purchase_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            create_order_replay.status(),
            axum::http::StatusCode::CREATED
        );
        let replay_body: serde_json::Value = create_order_replay.json().await.unwrap();
        assert_eq!(replay_body["data"]["id"], order_id);
        let order_item_url = format!("{purchase_orders_url}/{order_id}");
        let updated_order = client.patch(&order_item_url).bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "supplierId": supplier_id, "reference": "TIMBER-QUOTE-14-REV1", "expectedRevision": 1,
                "items": [{ "productId": product_id, "quantity": 5, "expectedUnitCostMinor": 710 }]
            })).send().await.unwrap();
        assert_eq!(updated_order.status(), axum::http::StatusCode::OK);
        let updated_order_body: serde_json::Value = updated_order.json().await.unwrap();
        assert_eq!(updated_order_body["data"]["revision"], 2);
        let order_line_id = updated_order_body["data"]["items"][0]["id"]
            .as_str()
            .unwrap();
        let placed_order = client
            .post(format!("{order_item_url}/place"))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({ "expectedRevision": 2 }))
            .send()
            .await
            .unwrap();
        assert_eq!(placed_order.status(), axum::http::StatusCode::OK);
        let placed_order_body: serde_json::Value = placed_order.json().await.unwrap();
        assert_eq!(placed_order_body["data"]["status"], "placed");
        let receipt_url = format!("{order_item_url}/receipts");
        let partial_receipt_payload = serde_json::json!({
            "expectedRevision": 3, "reference": "DELIVERY-1",
            "items": [{ "orderLineId": order_line_id, "quantity": 2, "actualUnitCostMinor": 750 }]
        });
        let partial_receipt = client
            .post(&receipt_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "hosted-purchase-receipt-001")
            .json(&partial_receipt_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(partial_receipt.status(), axum::http::StatusCode::OK);
        let partial_receipt_body: serde_json::Value = partial_receipt.json().await.unwrap();
        assert_eq!(partial_receipt_body["data"]["status"], "partially_received");
        let partial_replay = client
            .post(&receipt_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "hosted-purchase-receipt-001")
            .json(&partial_receipt_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(partial_replay.status(), axum::http::StatusCode::OK);
        let partial_replay_body: serde_json::Value = partial_replay.json().await.unwrap();
        assert_eq!(
            partial_replay_body["data"]["revision"],
            partial_receipt_body["data"]["revision"]
        );
        let cashier_purchase_list = client
            .get(format!(
            "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/purchase-orders"
        ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_purchase_list.status(),
            axum::http::StatusCode::FORBIDDEN
        );

        let finishing_revision = partial_receipt_body["data"]["revision"].as_i64().unwrap();
        let final_receipt = client.post(&receipt_url).bearer_auth("opaque-token")
            .header("Idempotency-Key", "hosted-purchase-receipt-002")
            .json(&serde_json::json!({
                "expectedRevision": finishing_revision, "reference": "DELIVERY-2",
                "items": [{ "orderLineId": order_line_id, "quantity": 3, "actualUnitCostMinor": 760 }]
            })).send().await.unwrap();
        assert_eq!(final_receipt.status(), axum::http::StatusCode::OK);
        let final_receipt_body: serde_json::Value = final_receipt.json().await.unwrap();
        assert_eq!(final_receipt_body["data"]["status"], "received");
        let purchase_effects = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            "SELECT stock_quantity,cost_minor,
             (SELECT count(*) FROM stock_movements WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid AND movement_kind='purchase_receipt'),
             (SELECT count(*) FROM hosted_purchase_receipts WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND order_id=$4::uuid)
             FROM branch_catalog_items WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid"
        ).bind(&tenant_a).bind(&branch_a).bind(&product_id).bind(order_id)
            .fetch_one(&verification_pool).await.unwrap();
        assert_eq!(purchase_effects, (12, 760, 2, 2));
        let billing_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/supplier-billing");
        let billing_before = client
            .get(&billing_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(billing_before.status(), axum::http::StatusCode::OK);
        let billing_before_body: serde_json::Value = billing_before.json().await.unwrap();
        assert_eq!(billing_before_body["data"]["currencyCode"], "GHS");
        assert_eq!(
            billing_before_body["data"]["unbilledReceipts"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            billing_before_body["data"]["unbilledReceipts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["availableMinor"].as_i64().unwrap())
                .sum::<i64>(),
            3780
        );
        let cashier_billing = client.get(format!("http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/supplier-billing"))
            .bearer_auth("opaque-token").send().await.unwrap();
        assert_eq!(cashier_billing.status(), axum::http::StatusCode::FORBIDDEN);

        let receipt_rows = final_receipt_body["data"]["receipts"].as_array().unwrap();
        let first_receipt = receipt_rows
            .iter()
            .find(|row| row["reference"] == "DELIVERY-1")
            .unwrap()["id"]
            .as_str()
            .unwrap();
        let second_receipt = receipt_rows
            .iter()
            .find(|row| row["reference"] == "DELIVERY-2")
            .unwrap()["id"]
            .as_str()
            .unwrap();
        let bill_payload = serde_json::json!({
            "supplierId": supplier_id,
            "supplierReference": " INV-2026-104 ",
            "invoiceDate": "2026-10-30",
            "dueDate": "2026-11-29",
            "totalMinor": 1500,
            "allocations": [{"receiptId": first_receipt, "amountMinor": 1500}]
        });
        let bills_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/supplier-bills");
        let matched_bill = client
            .post(&bills_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-bill-match-001")
            .json(&bill_payload)
            .send()
            .await
            .unwrap();
        let matched_status = matched_bill.status();
        let matched_text = matched_bill.text().await.unwrap();
        assert_eq!(
            matched_status,
            axum::http::StatusCode::CREATED,
            "{matched_text}"
        );
        let matched_body: serde_json::Value = serde_json::from_str(&matched_text).unwrap();
        assert_eq!(matched_body["data"]["totalMinor"], 1500);
        assert_eq!(matched_body["data"]["supplierReference"], "INV-2026-104");
        assert_eq!(matched_body["data"]["replayed"], false);
        let bill_replay = client
            .post(&bills_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-bill-match-001")
            .json(&bill_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(bill_replay.status(), axum::http::StatusCode::CREATED);
        let bill_replay_body: serde_json::Value = bill_replay.json().await.unwrap();
        assert_eq!(bill_replay_body["data"]["id"], matched_body["data"]["id"]);
        assert_eq!(bill_replay_body["data"]["replayed"], true);
        let duplicate_invoice = client
            .post(&bills_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-bill-match-002")
            .json(&serde_json::json!({
                "supplierId": supplier_id,
                "supplierReference": "inv-2026-104",
                "invoiceDate": "2026-10-31",
                "totalMinor": 2280,
                "allocations": [{"receiptId": second_receipt, "amountMinor": 2280}]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(duplicate_invoice.status(), axum::http::StatusCode::CONFLICT);
        let excessive_allocation = client
            .post(&bills_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-bill-match-over-001")
            .json(&serde_json::json!({
                "supplierId": supplier_id, "supplierReference": "INV-OVER-ALLOCATED",
                "invoiceDate": "2026-10-31", "totalMinor": 2281,
                "allocations": [{"receiptId": second_receipt, "amountMinor": 2281}]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            excessive_allocation.status(),
            axum::http::StatusCode::CONFLICT
        );
        let second_bill = client
            .post(&bills_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-bill-match-003")
            .json(&serde_json::json!({
                "supplierId": supplier_id, "supplierReference": "INV-2026-105",
                "invoiceDate": "2026-10-31", "dueDate": "2026-11-30", "totalMinor": 2280,
                "allocations": [{"receiptId": second_receipt, "amountMinor": 2280}]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(second_bill.status(), axum::http::StatusCode::CREATED);
        let second_bill_body: serde_json::Value = second_bill.json().await.unwrap();
        let billing_after = client
            .get(&billing_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        let billing_after_body: serde_json::Value = billing_after.json().await.unwrap();
        assert_eq!(
            billing_after_body["data"]["bills"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            billing_after_body["data"]["unbilledReceipts"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        let first_receipt_line_id: String = sqlx::query_scalar(
            "SELECT id::text FROM hosted_purchase_receipt_lines WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND receipt_id=$3::uuid AND product_id=$4::uuid",
        )
        .bind(&tenant_a)
        .bind(&branch_a)
        .bind(first_receipt)
        .bind(&product_id)
        .fetch_one(&verification_pool)
        .await
        .unwrap();
        let credit_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/supplier-credit-notes");
        let credit_payload = serde_json::json!({
            "billId": matched_body["data"]["id"],
            "creditReference": "CR-RETURN-104",
            "effectiveDate": "2026-11-02",
            "reason": "One sealed tea pack returned",
            "lines": [{"receiptId": first_receipt, "receiptLineId": first_receipt_line_id, "quantity": 1}]
        });
        let created_credit = client
            .post(&credit_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-credit-001")
            .json(&credit_payload)
            .send()
            .await
            .unwrap();
        let created_credit_status = created_credit.status();
        let created_credit_text = created_credit.text().await.unwrap();
        assert_eq!(
            created_credit_status,
            axum::http::StatusCode::CREATED,
            "{created_credit_text}"
        );
        let created_credit_body: serde_json::Value =
            serde_json::from_str(&created_credit_text).unwrap();
        assert_eq!(created_credit_body["data"]["totalMinor"], 750);
        assert_eq!(created_credit_body["data"]["replayed"], false);
        let credit_replay = client
            .post(&credit_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-credit-001")
            .json(&credit_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(credit_replay.status(), axum::http::StatusCode::OK);
        let credit_replay_body: serde_json::Value = credit_replay.json().await.unwrap();
        assert_eq!(
            credit_replay_body["data"]["id"],
            created_credit_body["data"]["id"]
        );
        assert_eq!(credit_replay_body["data"]["replayed"], true);
        let credit_key_conflict = client.post(&credit_url).bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-credit-001")
            .json(&serde_json::json!({"billId":matched_body["data"]["id"],"creditReference":"CR-RETURN-104","effectiveDate":"2026-11-02","reason":"Changed retry","lines":[{"receiptId":first_receipt,"receiptLineId":first_receipt_line_id,"quantity":1}]}))
            .send().await.unwrap();
        assert_eq!(
            credit_key_conflict.status(),
            axum::http::StatusCode::CONFLICT
        );
        let over_allocation_credit = client.post(&credit_url).bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-credit-over-allocation")
            .json(&serde_json::json!({"billId":matched_body["data"]["id"],"creditReference":"CR-RETURN-OVER","effectiveDate":"2026-11-02","reason":"Over allocation attempt","lines":[{"receiptId":first_receipt,"receiptLineId":first_receipt_line_id,"quantity":2}]}))
            .send().await.unwrap();
        assert_eq!(
            over_allocation_credit.status(),
            axum::http::StatusCode::CONFLICT
        );
        let credit_effects = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            "SELECT i.stock_quantity,
              (SELECT count(*) FROM hosted_supplier_credit_notes WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND bill_id=$3::uuid),
              (SELECT count(*) FROM stock_movements WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND source_id=$4::uuid AND quantity_delta=-1),
              (SELECT count(*) FROM hosted_accounting_journals WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND source_type='supplier_credit' AND source_id=$4::uuid)
             FROM branch_catalog_items i WHERE i.tenant_id=$1::uuid AND i.branch_id=$2::uuid AND i.product_id=$5::uuid",
        ).bind(&tenant_a).bind(&branch_a).bind(matched_body["data"]["id"].as_str().unwrap()).bind(created_credit_body["data"]["id"].as_str().unwrap()).bind(&product_id)
            .fetch_one(&verification_pool).await.unwrap();
        assert_eq!(credit_effects, (11, 1, 1, 1));
        let credit_journal_lines = sqlx::query_as::<_, (String, i64, i64)>(
            "SELECT l.account_code,SUM(l.debit_minor)::bigint,SUM(l.credit_minor)::bigint FROM hosted_accounting_journals j JOIN hosted_accounting_journal_lines l ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND j.source_type='supplier_credit' AND j.source_id=$3::uuid GROUP BY l.account_code ORDER BY l.account_code",
        )
        .bind(&tenant_a)
        .bind(&branch_a)
        .bind(created_credit_body["data"]["id"].as_str().unwrap())
        .fetch_all(&verification_pool)
        .await
        .unwrap();
        assert_eq!(
            credit_journal_lines,
            vec![("1300".to_owned(), 0, 750), ("2000".to_owned(), 750, 0)]
        );
        let invalid_credit_id = uuid::Uuid::new_v4();
        let mut invalid_credit = verification_pool.begin().await.unwrap();
        sqlx::query("INSERT INTO hosted_supplier_credit_notes(id,tenant_id,branch_id,bill_id,supplier_id,credit_reference,reference_key,effective_date,reason,currency_code,total_minor,actor_membership_id,idempotency_key,request_hash) VALUES($1,$2::uuid,$3::uuid,$4::uuid,$5::uuid,'CR-INVALID-TOTAL','cr-invalid-total','2026-11-02','Constraint verifier','GHS',751,$6::uuid,'supplier-credit-invalid-total',$7)")
            .bind(invalid_credit_id).bind(&tenant_a).bind(&branch_a).bind(matched_body["data"]["id"].as_str().unwrap())
            .bind(&supplier_id).bind(&membership_id).bind(vec![0_u8; 32])
            .execute(&mut *invalid_credit).await.unwrap();
        sqlx::query("INSERT INTO hosted_supplier_credit_lines(tenant_id,branch_id,credit_note_id,receipt_id,receipt_line_id,product_id,quantity,unit_cost_minor,amount_minor) VALUES($1::uuid,$2::uuid,$3,$4::uuid,$5::uuid,$6::uuid,1,750,750)")
            .bind(&tenant_a).bind(&branch_a).bind(invalid_credit_id).bind(first_receipt).bind(&first_receipt_line_id).bind(&product_id)
            .execute(&mut *invalid_credit).await.unwrap();
        assert!(invalid_credit.commit().await.is_err(), "deferred database constraint must reject a supplier credit header that does not match its lines");
        let invalid_credit_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM hosted_supplier_credit_notes WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3",
        )
        .bind(&tenant_a)
        .bind(&branch_a)
        .bind(invalid_credit_id)
        .fetch_one(&verification_pool)
        .await
        .unwrap();
        assert_eq!(
            invalid_credit_rows, 0,
            "failed deferred validation must roll the entire credit transaction back"
        );
        let payment_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/supplier-payments");
        let payment_payload = serde_json::json!({
            "supplierId": supplier_id,
            "paymentMethod": "bank_transfer",
            "externalReference": "BANK-TRANSFER-77",
            "amountMinor": 700,
            "allocations": [{"billId": matched_body["data"]["id"], "amountMinor": 700}]
        });
        let recorded_payment = client
            .post(&payment_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-payment-001")
            .json(&payment_payload)
            .send()
            .await
            .unwrap();
        let recorded_payment_status = recorded_payment.status();
        let recorded_payment_text = recorded_payment.text().await.unwrap();
        assert_eq!(
            recorded_payment_status,
            axum::http::StatusCode::OK,
            "{recorded_payment_text}"
        );
        let recorded_payment_body: serde_json::Value =
            serde_json::from_str(&recorded_payment_text).unwrap();
        assert_eq!(recorded_payment_body["data"]["amountMinor"], 700);
        assert_eq!(recorded_payment_body["data"]["replayed"], false);
        let paid_balance_over_credit = client.post(&credit_url).bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-credit-paid-balance-overflow")
            .json(&serde_json::json!({"billId":matched_body["data"]["id"],"creditReference":"CR-RETURN-BALANCE-OVER","effectiveDate":"2026-11-02","reason":"Exceeds remaining invoice balance","lines":[{"receiptId":first_receipt,"receiptLineId":first_receipt_line_id,"quantity":1}]}))
            .send().await.unwrap();
        assert_eq!(
            paid_balance_over_credit.status(),
            axum::http::StatusCode::CONFLICT
        );
        let payment_replay = client
            .post(&payment_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-payment-001")
            .json(&payment_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(payment_replay.status(), axum::http::StatusCode::OK);
        let payment_replay_body: serde_json::Value = payment_replay.json().await.unwrap();
        assert_eq!(
            payment_replay_body["data"]["id"],
            recorded_payment_body["data"]["id"]
        );
        assert_eq!(payment_replay_body["data"]["replayed"], true);
        let payment_key_conflict = client
            .post(&payment_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-payment-001")
            .json(&serde_json::json!({
                "supplierId": supplier_id, "paymentMethod": "bank_transfer", "amountMinor": 701,
                "allocations": [{"billId": matched_body["data"]["id"], "amountMinor": 701}]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            payment_key_conflict.status(),
            axum::http::StatusCode::CONFLICT
        );
        let payment_over_allocation = client
            .post(&payment_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-payment-over-001")
            .json(&serde_json::json!({
                "supplierId": supplier_id, "paymentMethod": "bank_transfer", "amountMinor": 801,
                "allocations": [{"billId": matched_body["data"]["id"], "amountMinor": 801}]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            payment_over_allocation.status(),
            axum::http::StatusCode::CONFLICT
        );
        sqlx::query("UPDATE membership_role_grants SET role_key='manager' WHERE tenant_id=$1::uuid AND membership_id=$2::uuid AND role_key='cashier'")
            .bind(&tenant_a).bind(&membership_id_two).execute(&verification_pool).await.unwrap();
        let manager_payment_denied = client.post(format!("http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/supplier-payments"))
            .bearer_auth("opaque-token").header("Idempotency-Key", "supplier-payment-manager-denied")
            .json(&payment_payload).send().await.unwrap();
        assert_eq!(
            manager_payment_denied.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let manager_credit_denied = client.post(format!("http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/supplier-credit-notes"))
            .bearer_auth("opaque-token").header("Idempotency-Key", "supplier-credit-manager-denied")
            .json(&credit_payload).send().await.unwrap();
        assert_eq!(
            manager_credit_denied.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        sqlx::query("UPDATE membership_role_grants SET role_key='cashier' WHERE tenant_id=$1::uuid AND membership_id=$2::uuid AND role_key='manager'")
            .bind(&tenant_a).bind(&membership_id_two).execute(&verification_pool).await.unwrap();
        let cashier_payment_denied = client.post(format!("http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/supplier-payments"))
            .bearer_auth("opaque-token").header("Idempotency-Key", "supplier-payment-cashier-denied")
            .json(&payment_payload).send().await.unwrap();
        assert_eq!(
            cashier_payment_denied.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let settled_billing = client
            .get(&billing_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(settled_billing.status(), axum::http::StatusCode::OK);
        let settled_body: serde_json::Value = settled_billing.json().await.unwrap();
        let settled_bill = settled_body["data"]["bills"]
            .as_array()
            .unwrap()
            .iter()
            .find(|bill| bill["id"] == matched_body["data"]["id"])
            .unwrap();
        assert_eq!(settled_bill["paidMinor"], 700);
        assert_eq!(settled_bill["creditedMinor"], 750);
        assert_eq!(settled_bill["balanceMinor"], 50);
        assert_eq!(
            settled_body["data"]["payments"].as_array().unwrap().len(),
            1
        );
        let journal_balance = sqlx::query_as::<_, (i64,i64,i64)>(
            "SELECT SUM(l.debit_minor)::bigint,SUM(l.credit_minor)::bigint,COUNT(*) FROM hosted_accounting_journals j JOIN hosted_accounting_journal_lines l ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND j.source_type='supplier_payment' AND j.source_id=$3::uuid GROUP BY j.id"
        ).bind(&tenant_a).bind(&branch_a).bind(recorded_payment_body["data"]["id"].as_str().unwrap())
            .fetch_one(&verification_pool).await.unwrap();
        assert_eq!(journal_balance, (700, 700, 2));
        let failed_payment_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM hosted_supplier_payments WHERE tenant_id=$1::uuid AND branch_id=$2::uuid")
            .bind(&tenant_a).bind(&branch_a).fetch_one(&verification_pool).await.unwrap();
        assert_eq!(
            failed_payment_rows, 1,
            "conflicts and unauthorized attempts must not leave payment sources"
        );

        let safe_supplier_payment_payload = serde_json::json!({
            "supplierId": supplier_id, "paymentMethod": "cash", "cashLocationId": safe_id,
            "externalReference": "SAFE-PAYMENT-500", "amountMinor": 500,
            "allocations": [{"billId": second_bill_body["data"]["id"], "amountMinor": 500}]
        });
        let safe_supplier_payment = client
            .post(&payment_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-payment-safe-cash-001")
            .json(&safe_supplier_payment_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(safe_supplier_payment.status(), axum::http::StatusCode::OK);
        let safe_supplier_payment_body: serde_json::Value =
            safe_supplier_payment.json().await.unwrap();
        let safe_supplier_payment_id = safe_supplier_payment_body["data"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            safe_supplier_payment_body["data"]["cashLocationId"],
            safe_id
        );
        let safe_supplier_payment_retry = client
            .post(&payment_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-payment-safe-cash-001")
            .json(&safe_supplier_payment_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            safe_supplier_payment_retry.status(),
            axum::http::StatusCode::OK
        );
        assert_eq!(
            safe_supplier_payment_retry
                .json::<serde_json::Value>()
                .await
                .unwrap()["data"]["replayed"],
            true
        );
        let safe_supplier_payment_conflict = client.post(&payment_url).bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-payment-safe-cash-001")
            .json(&serde_json::json!({"supplierId":supplier_id,"paymentMethod":"cash","cashLocationId":safe_id,"externalReference":"CHANGED","amountMinor":500,"allocations":[{"billId":second_bill_body["data"]["id"],"amountMinor":500}]})).send().await.unwrap();
        assert_eq!(
            safe_supplier_payment_conflict.status(),
            axum::http::StatusCode::CONFLICT
        );
        let safe_supplier_payment_overdraw = client.post(&payment_url).bearer_auth("opaque-token")
            .header("Idempotency-Key", "supplier-payment-safe-cash-overdraw")
            .json(&serde_json::json!({"supplierId":supplier_id,"paymentMethod":"cash","cashLocationId":safe_id,"externalReference":null,"amountMinor":999999,"allocations":[{"billId":second_bill_body["data"]["id"],"amountMinor":999999}]})).send().await.unwrap();
        assert_eq!(
            safe_supplier_payment_overdraw.status(),
            axum::http::StatusCode::CONFLICT
        );
        let (supplier_event_count, supplier_outflow, supplier_cash_balance, linked_drawer_count): (i64,i64,i64,i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND source_supplier_payment_id=$2::uuid),(SELECT signed_amount_minor FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND source_supplier_payment_id=$2::uuid),(SELECT COALESCE(sum(signed_amount_minor),0)::bigint FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND location_id=$3::uuid),(SELECT count(*) FROM hosted_drawer_movements WHERE tenant_id=$1::uuid AND source_payment_id=$2::uuid)",
        ).bind(&tenant_a).bind(&safe_supplier_payment_id).bind(&safe_id).fetch_one(&verification_pool).await.unwrap();
        assert_eq!(
            (
                supplier_event_count,
                supplier_outflow,
                supplier_cash_balance,
                linked_drawer_count
            ),
            (1, -500, 7500, 0)
        );
        let safe_supplier_journal = sqlx::query_as::<_,(i64,i64,i64)>(
            "SELECT SUM(l.debit_minor)::bigint,SUM(l.credit_minor)::bigint,COUNT(*)::bigint FROM hosted_accounting_journals j JOIN hosted_accounting_journal_lines l ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND j.source_type='supplier_payment' AND j.source_id=$3::uuid GROUP BY j.id",
        ).bind(&tenant_a).bind(&branch_a).bind(&safe_supplier_payment_id).fetch_one(&verification_pool).await.unwrap();
        assert_eq!(safe_supplier_journal, (500, 500, 2));

        let expenses_url = format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/expenses");
        let expense_payload = serde_json::json!({
            "categoryCode": "utilities",
            "description": "Branch electricity bill",
            "payee": "Power Company",
            "amountMinor": 1250,
            "paymentMethod": "bank_transfer",
            "externalReference": "BANK-EXP-125",
            "occurredOn": "2026-10-30"
        });
        let recorded_expense = client
            .post(&expenses_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "expense-bank-transfer-001")
            .json(&expense_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(recorded_expense.status(), axum::http::StatusCode::CREATED);
        let recorded_expense_body: serde_json::Value = recorded_expense.json().await.unwrap();
        assert_eq!(recorded_expense_body["data"]["amountMinor"], 1250);
        assert_eq!(recorded_expense_body["data"]["currencyCode"], "GHS");
        assert_eq!(recorded_expense_body["data"]["replayed"], false);
        let expense_id = recorded_expense_body["data"]["id"]
            .as_str()
            .unwrap()
            .to_owned();

        let reimbursement_url = format!("{expenses_url}/{expense_id}/reimbursements");
        let reimbursement_payload = serde_json::json!({
            "amountMinor": 300, "returnMethod": "cash", "cashLocationId": safe_id,
            "externalReference": null, "reason": "Duplicate utility payment returned", "returnedOn": "2026-10-31"
        });
        let recorded_reimbursement = client
            .post(&reimbursement_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "expense-return-cash-safe-001")
            .json(&reimbursement_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            recorded_reimbursement.status(),
            axum::http::StatusCode::CREATED
        );
        let recorded_reimbursement_body: serde_json::Value =
            recorded_reimbursement.json().await.unwrap();
        assert_eq!(recorded_reimbursement_body["data"]["replayed"], false);
        let reimbursement_id = recorded_reimbursement_body["data"]["reimbursement"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let reimbursement_retry = client
            .post(&reimbursement_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "expense-return-cash-safe-001")
            .json(&reimbursement_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            reimbursement_retry.status(),
            axum::http::StatusCode::CREATED
        );
        assert_eq!(
            reimbursement_retry
                .json::<serde_json::Value>()
                .await
                .unwrap()["data"]["replayed"],
            true
        );
        let reimbursement_changed = client.post(&reimbursement_url).bearer_auth("opaque-token")
            .header("Idempotency-Key", "expense-return-cash-safe-001")
            .json(&serde_json::json!({"amountMinor":301,"returnMethod":"cash","cashLocationId":safe_id,"externalReference":null,"reason":"Duplicate utility payment returned","returnedOn":"2026-10-31"})).send().await.unwrap();
        assert_eq!(
            reimbursement_changed.status(),
            axum::http::StatusCode::CONFLICT
        );
        let reimbursement_over_return = client.post(&reimbursement_url).bearer_auth("opaque-token")
            .header("Idempotency-Key", "expense-return-over-source-001")
            .json(&serde_json::json!({"amountMinor":951,"returnMethod":"bank_transfer","cashLocationId":null,"externalReference":"OVER-RETURN-951","reason":"Return exceeds unpaid expense","returnedOn":"2026-10-31"})).send().await.unwrap();
        assert_eq!(
            reimbursement_over_return.status(),
            axum::http::StatusCode::CONFLICT
        );
        let reimbursement_workspace = client
            .get(&reimbursement_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(reimbursement_workspace.status(), axum::http::StatusCode::OK);
        let reimbursement_workspace_body: serde_json::Value =
            reimbursement_workspace.json().await.unwrap();
        assert_eq!(reimbursement_workspace_body["data"]["amountMinor"], 1250);
        assert_eq!(reimbursement_workspace_body["data"]["reimbursedMinor"], 300);
        assert_eq!(reimbursement_workspace_body["data"]["remainingMinor"], 950);
        let (reimbursement_event_count, reimbursement_inflow, reimbursed_safe_balance): (i64,i64,i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND source_expense_reimbursement_id=$2::uuid),(SELECT signed_amount_minor FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND source_expense_reimbursement_id=$2::uuid),(SELECT COALESCE(sum(signed_amount_minor),0)::bigint FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND location_id=$3::uuid)",
        ).bind(&tenant_a).bind(&reimbursement_id).bind(&safe_id).fetch_one(&verification_pool).await.unwrap();
        assert_eq!(
            (
                reimbursement_event_count,
                reimbursement_inflow,
                reimbursed_safe_balance
            ),
            (1, 300, 7800)
        );
        let reimbursement_journal = sqlx::query_as::<_,(i64,i64,i64)>(
            "SELECT SUM(l.debit_minor)::bigint,SUM(l.credit_minor)::bigint,COUNT(*)::bigint FROM hosted_accounting_journals j JOIN hosted_accounting_journal_lines l ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND j.source_type='expense_reimbursement' AND j.source_id=$3::uuid GROUP BY j.id",
        ).bind(&tenant_a).bind(&branch_a).bind(&reimbursement_id).fetch_one(&verification_pool).await.unwrap();
        assert_eq!(reimbursement_journal, (300, 300, 2));

        let expense_retry = client
            .post(&expenses_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "expense-bank-transfer-001")
            .json(&expense_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(expense_retry.status(), axum::http::StatusCode::CREATED);
        let expense_retry_body: serde_json::Value = expense_retry.json().await.unwrap();
        assert_eq!(expense_retry_body["data"]["id"], expense_id);
        assert_eq!(expense_retry_body["data"]["replayed"], true);

        let expense_key_conflict = client
            .post(&expenses_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "expense-bank-transfer-001")
            .json(&serde_json::json!({
                "categoryCode": "utilities", "description": "Changed payload", "payee": "Power Company",
                "amountMinor": 1251, "paymentMethod": "bank_transfer", "externalReference": "BANK-EXP-125",
                "occurredOn": "2026-10-30"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            expense_key_conflict.status(),
            axum::http::StatusCode::CONFLICT
        );

        let unsupported_cash_expense = client
            .post(&expenses_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "expense-cash-without-ledger")
            .json(&serde_json::json!({
                "categoryCode": "utilities", "description": "Unlinked cash expense", "payee": null,
                "amountMinor": 500, "paymentMethod": "cash", "externalReference": null,
                "occurredOn": "2026-10-30"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            unsupported_cash_expense.status(),
            axum::http::StatusCode::BAD_REQUEST
        );

        let expense_workspace = client
            .get(format!("{expenses_url}?offset=0&pageSize=25"))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(expense_workspace.status(), axum::http::StatusCode::OK);
        let expense_workspace_body: serde_json::Value = expense_workspace.json().await.unwrap();
        assert_eq!(
            expense_workspace_body["data"]["expenses"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            expense_workspace_body["data"]["expenses"][0]["id"],
            expense_id
        );

        let cashier_expense_read = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/expenses"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_expense_read.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let cashier_reimbursement_denied = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/expenses/{expense_id}/reimbursements"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_reimbursement_denied.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let cashier_cost_center_denied = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/expenses/{expense_id}/cost-center"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_cost_center_denied.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let cashier_expense_write = client
            .post(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/expenses"
            ))
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "cashier-expense-denied")
            .json(&expense_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_expense_write.status(),
            axum::http::StatusCode::FORBIDDEN
        );

        sqlx::query("UPDATE membership_role_grants SET role_key='manager' WHERE tenant_id=$1::uuid AND membership_id=$2::uuid AND role_key='cashier'")
            .bind(&tenant_a).bind(&membership_id_two).execute(&verification_pool).await.unwrap();
        let manager_expense_payload = serde_json::json!({
            "categoryCode": "supplies", "description": "Printer paper", "payee": "Office Store",
            "amountMinor": 725, "paymentMethod": "mobile_money", "externalReference": "MM-EXP-725",
            "occurredOn": "2026-10-31"
        });
        let manager_expense = client
            .post(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/expenses"
            ))
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "expense-manager-mobile-money-001")
            .json(&manager_expense_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(manager_expense.status(), axum::http::StatusCode::CREATED);
        let manager_expense_body: serde_json::Value = manager_expense.json().await.unwrap();
        assert_eq!(manager_expense_body["data"]["categoryCode"], "supplies");
        let manager_expense_id = manager_expense_body["data"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let manager_attribution_url = format!(
            "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/expenses/{manager_expense_id}/cost-center"
        );
        let manager_attribution_before = client
            .get(&manager_attribution_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            manager_attribution_before.status(),
            axum::http::StatusCode::OK
        );
        assert_eq!(
            manager_attribution_before
                .json::<serde_json::Value>()
                .await
                .unwrap()["data"]["currentRevision"],
            0
        );
        let manager_attribution_payload = serde_json::json!({
            "expectedRevision": 0, "costCenterKind": "station_direct", "stationId": station_a,
            "reason": "Allocate supplies to the correct station"
        });
        let manager_attribution = client
            .post(format!("{manager_attribution_url}/reclassifications"))
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "manager-expense-attribution-001")
            .json(&manager_attribution_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            manager_attribution.status(),
            axum::http::StatusCode::CREATED
        );
        let manager_attribution_body: serde_json::Value = manager_attribution.json().await.unwrap();
        assert_eq!(manager_attribution_body["data"]["revision"], 1);
        let manager_attribution_id = manager_attribution_body["data"]["id"].as_str().unwrap();
        let manager_attribution_retry = client
            .post(format!("{manager_attribution_url}/reclassifications"))
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "manager-expense-attribution-001")
            .json(&manager_attribution_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            manager_attribution_retry.status(),
            axum::http::StatusCode::CREATED
        );
        assert_eq!(
            manager_attribution_retry
                .json::<serde_json::Value>()
                .await
                .unwrap()["data"]["id"],
            manager_attribution_id
        );
        let manager_attribution_conflict = client
            .post(format!("{manager_attribution_url}/reclassifications"))
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "manager-expense-attribution-001")
            .json(&serde_json::json!({"expectedRevision":0,"costCenterKind":"store_overhead","stationId":null,"reason":"Changed request"}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            manager_attribution_conflict.status(),
            axum::http::StatusCode::CONFLICT
        );
        let manager_attribution_stale = client
            .post(format!("{manager_attribution_url}/reclassifications"))
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "manager-expense-attribution-stale-001")
            .json(&manager_attribution_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            manager_attribution_stale.status(),
            axum::http::StatusCode::CONFLICT
        );
        let manager_attribution_history = client
            .get(&manager_attribution_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        let manager_attribution_history_body: serde_json::Value =
            manager_attribution_history.json().await.unwrap();
        assert_eq!(
            manager_attribution_history_body["data"]["currentKind"],
            "station_direct"
        );
        assert_eq!(
            manager_attribution_history_body["data"]["rows"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let manager_expense_workspace = client
            .get(format!("http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/expenses?offset=0&pageSize=25"))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        let manager_expense_workspace_body: serde_json::Value =
            manager_expense_workspace.json().await.unwrap();
        let manager_expense_row = manager_expense_workspace_body["data"]["expenses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == manager_expense_id)
            .unwrap();
        assert_eq!(manager_expense_row["costCenterKind"], "station_direct");
        assert_eq!(manager_expense_row["costCenterStationId"], station_a);
        let (attributed_expense_amount, attributed_reimbursement_count, attributed_journal_count): (i64,i64,i64) = sqlx::query_as(
            "SELECT e.amount_minor,(SELECT count(*) FROM hosted_expense_reimbursements r WHERE r.tenant_id=e.tenant_id AND r.branch_id=e.branch_id AND r.expense_id=e.id),(SELECT count(*) FROM hosted_accounting_journals j WHERE j.tenant_id=e.tenant_id AND j.branch_id=e.branch_id AND j.source_type='expense' AND j.source_id=e.id) FROM hosted_expenses e WHERE e.tenant_id=$1::uuid AND e.branch_id=$2::uuid AND e.id=$3::uuid"
        ).bind(&tenant_a).bind(&branch_a).bind(&manager_expense_id).fetch_one(&verification_pool).await.unwrap();
        assert_eq!(
            (
                attributed_expense_amount,
                attributed_reimbursement_count,
                attributed_journal_count
            ),
            (725, 0, 1)
        );
        let safe_cash_expense_payload = serde_json::json!({
            "categoryCode": "supplies", "description": "Safe-funded cleaning supplies", "payee": "Local shop",
            "amountMinor": 1200, "paymentMethod": "cash", "cashLocationId": safe_id,
            "externalReference": null, "occurredOn": "2026-10-31"
        });
        let safe_cash_expense = client
            .post(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/expenses"
            ))
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "expense-safe-cash-001")
            .json(&safe_cash_expense_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(safe_cash_expense.status(), axum::http::StatusCode::CREATED);
        let safe_cash_expense_body: serde_json::Value = safe_cash_expense.json().await.unwrap();
        let safe_cash_expense_id = safe_cash_expense_body["data"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(safe_cash_expense_body["data"]["cashLocationId"], safe_id);
        let safe_cash_retry = client
            .post(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/expenses"
            ))
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "expense-safe-cash-001")
            .json(&safe_cash_expense_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(safe_cash_retry.status(), axum::http::StatusCode::CREATED);
        assert_eq!(
            safe_cash_retry.json::<serde_json::Value>().await.unwrap()["data"]["replayed"],
            true
        );
        let safe_cash_overdraw = client
            .post(format!("http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/expenses"))
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "expense-safe-cash-overdraw")
            .json(&serde_json::json!({"categoryCode":"supplies","description":"Too much","amountMinor":999999999,"paymentMethod":"cash","cashLocationId":safe_id,"occurredOn":"2026-10-31"}))
            .send().await.unwrap();
        assert_eq!(
            safe_cash_overdraw.status(),
            axum::http::StatusCode::CONFLICT
        );
        let manager_expense_list = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/expenses"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(manager_expense_list.status(), axum::http::StatusCode::OK);
        let manager_expense_list_body: serde_json::Value =
            manager_expense_list.json().await.unwrap();
        assert_eq!(
            manager_expense_list_body["data"]["expenses"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        sqlx::query("UPDATE membership_role_grants SET role_key='cashier' WHERE tenant_id=$1::uuid AND membership_id=$2::uuid AND role_key='manager'")
            .bind(&tenant_a).bind(&membership_id_two).execute(&verification_pool).await.unwrap();

        let expense_effects = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            "SELECT e.amount_minor,COALESCE(SUM(l.debit_minor),0)::bigint,COALESCE(SUM(l.credit_minor),0)::bigint,COUNT(l.id)::bigint
             FROM hosted_expenses e JOIN hosted_accounting_journals j ON j.tenant_id=e.tenant_id AND j.branch_id=e.branch_id AND j.source_type='expense' AND j.source_id=e.id
             JOIN hosted_accounting_journal_lines l ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id
             WHERE e.tenant_id=$1::uuid AND e.branch_id=$2::uuid AND e.id=$3::uuid GROUP BY e.id",
        )
        .bind(&tenant_a)
        .bind(&branch_a)
        .bind(&expense_id)
        .fetch_one(&verification_pool)
        .await
        .unwrap();
        assert_eq!(expense_effects, (1250, 1250, 1250, 2));
        let expense_source_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM hosted_expenses WHERE tenant_id=$1::uuid AND branch_id=$2::uuid",
        )
        .bind(&tenant_a)
        .bind(&branch_a)
        .fetch_one(&verification_pool)
        .await
        .unwrap();
        assert_eq!(
            expense_source_count, 3,
            "failed expense attempts leave no source rows"
        );
        let manager_journal_balance = sqlx::query_as::<_, (i64, i64, i64)>(
            "SELECT SUM(l.debit_minor)::bigint,SUM(l.credit_minor)::bigint,COUNT(*)::bigint
             FROM hosted_accounting_journals j JOIN hosted_accounting_journal_lines l
               ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id
             WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND j.source_type='expense' AND j.source_id=$3::uuid GROUP BY j.id",
        )
        .bind(&tenant_a)
        .bind(&branch_a)
        .bind(&manager_expense_id)
        .fetch_one(&verification_pool)
        .await
        .unwrap();
        assert_eq!(manager_journal_balance, (725, 725, 2));
        let (safe_event_count, safe_outflow, safe_balance): (i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND source_expense_id=$2::uuid), (SELECT signed_amount_minor FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND source_expense_id=$2::uuid), (SELECT COALESCE(sum(signed_amount_minor),0)::bigint FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND location_id=$3::uuid)",
        ).bind(&tenant_a).bind(&safe_cash_expense_id).bind(&safe_id).fetch_one(&verification_pool).await.unwrap();
        assert_eq!(
            (safe_event_count, safe_outflow, safe_balance),
            (1, -1200, 6600)
        );

        sqlx::query("UPDATE branch_catalog_items SET stock_quantity=7,cost_minor=700,revision=revision-2 WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid")
            .bind(&tenant_a).bind(&branch_a).bind(&product_id).execute(&verification_pool).await.unwrap();

        sqlx::query(
            "INSERT INTO branch_catalog_items
                (tenant_id, branch_id, product_id, price_minor, tax_rate_bps, stock_quantity)
             VALUES ($1::uuid, $2::uuid, $3::uuid, 1300, 1500, 3)",
        )
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(&product_id)
        .execute(&verification_pool)
        .await
        .unwrap();

        let stock_adjustment_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/products/{product_id}/stock-adjustments"
        );
        let stock_adjustment = client
            .post(&stock_adjustment_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "tea-count-2026-10-31-01")
            .json(&serde_json::json!({
                "quantityDelta": 3,
                "note": "Physical count correction"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(stock_adjustment.status(), axum::http::StatusCode::CREATED);
        let stock_adjustment_body: serde_json::Value = stock_adjustment.json().await.unwrap();
        let movement_id = stock_adjustment_body["data"]["movementId"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(stock_adjustment_body["data"]["quantityDelta"], 3);
        assert_eq!(stock_adjustment_body["data"]["resultingStockQuantity"], 10);
        assert_eq!(stock_adjustment_body["data"]["replayed"], false);

        let stock_adjustment_replay = client
            .post(&stock_adjustment_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "tea-count-2026-10-31-01")
            .json(&serde_json::json!({
                "quantityDelta": 3,
                "note": "Physical count correction"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(stock_adjustment_replay.status(), axum::http::StatusCode::OK);
        let stock_adjustment_replay_body: serde_json::Value =
            stock_adjustment_replay.json().await.unwrap();
        assert_eq!(
            stock_adjustment_replay_body["data"]["movementId"],
            movement_id
        );
        assert_eq!(
            stock_adjustment_replay_body["data"]["resultingStockQuantity"],
            10
        );
        assert_eq!(stock_adjustment_replay_body["data"]["replayed"], true);

        let reused_adjustment_key = client
            .post(&stock_adjustment_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "tea-count-2026-10-31-01")
            .json(&serde_json::json!({
                "quantityDelta": 4,
                "note": "Different physical count"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            reused_adjustment_key.status(),
            axum::http::StatusCode::CONFLICT
        );

        let negative_stock_adjustment = client
            .post(&stock_adjustment_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "tea-count-2026-10-31-02")
            .json(&serde_json::json!({
                "quantityDelta": -11,
                "note": "Invalid overdraw"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            negative_stock_adjustment.status(),
            axum::http::StatusCode::CONFLICT
        );

        let adjusted_stock = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            "SELECT
                (SELECT stock_quantity FROM branch_catalog_items WHERE tenant_id = $1::uuid AND branch_id = $2::uuid AND product_id = $3::uuid),
                (SELECT count(*) FROM stock_movements WHERE tenant_id = $1::uuid AND branch_id = $2::uuid AND product_id = $3::uuid AND movement_kind = 'adjustment' AND idempotency_key = 'tea-count-2026-10-31-01'),
                (SELECT count(*) FROM audit_events WHERE tenant_id = $1::uuid AND entity_type = 'catalog_product' AND entity_id = $3::text AND action = 'inventory.stock_adjusted'),
                (SELECT count(*) FROM stock_movements WHERE tenant_id = $1::uuid AND branch_id = $2::uuid AND product_id = $3::uuid AND idempotency_key = 'tea-count-2026-10-31-02')"
        )
        .bind(&tenant_a)
        .bind(&branch_a)
        .bind(&product_id)
        .fetch_one(&verification_pool)
        .await
        .unwrap();
        assert_eq!(adjusted_stock, (10, 1, 1, 0));

        let destination_profile_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a_two}/profile");
        let destination_profile = client
            .put(&destination_profile_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "address":"25 Cocoa Street",
                "phone":"+233244100200",
                "email":"branch@example.test",
                "openingHours":{
                    "monday":[{"open":"08:00","close":"16:00"}],
                    "tuesday":[],"wednesday":[],"thursday":[],
                    "friday":[],"saturday":[],"sunday":[]
                },
                "expectedRevision":1
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(destination_profile.status(), axum::http::StatusCode::OK);

        let availability = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a}/products/{product_id}/availability"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(availability.status(), axum::http::StatusCode::OK);
        let availability_body: serde_json::Value = availability.json().await.unwrap();
        let availability_rows = availability_body["data"].as_array().unwrap();
        assert_eq!(availability_rows.len(), 2);
        assert!(availability_rows.iter().all(|row| {
            row["observedAt"].as_str().is_some_and(|value| {
                value.len() == 24
                    && value.as_bytes().get(4) == Some(&b'-')
                    && value.as_bytes().get(7) == Some(&b'-')
                    && value.as_bytes().get(10) == Some(&b'T')
                    && value.as_bytes().get(19) == Some(&b'.')
                    && value.ends_with('Z')
            })
        }));
        assert!(availability_rows.iter().any(|row| {
            row["branchId"] == branch_a
                && row["stockQuantity"] == 10
                && row["availableToPromise"] == 10
                && row["pickupReservedQuantity"] == 0
                && row["outboundCommittedQuantity"] == 0
                && row["commitmentShortfall"] == 0
        }));
        assert!(availability_rows.iter().any(|row| {
            row["branchId"] == branch_a_two
                && row["branchName"] == "Other A Branch"
                && row["stockQuantity"] == 3
                && row["availableToPromise"] == 3
                && row["priceMinor"] == 1300
                && row["address"] == "25 Cocoa Street"
                && row["phone"] == "+233244100200"
                && row["email"] == "branch@example.test"
                && row["timeZone"] == "Africa/Accra"
                && row["openingHours"]["monday"].as_array().unwrap().len() == 1
        }));
        assert!(availability_rows
            .iter()
            .all(|row| row.get("costMinor").is_none()));

        let pickup_url = format!(
            "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a}/pickup-reservations"
        );
        let pickup_body = serde_json::json!({
            "pickupBranchId": branch_a_two,
            "contactName": "Walk-in customer",
            "contactPhone": "+233200000123",
            "contactConsent": true,
            "holdMinutes": 60,
            "items": [{"productId": product_id, "quantity": 1}]
        });
        let pickup_create = client
            .post(&pickup_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "pickup-create-001")
            .json(&pickup_body)
            .send()
            .await
            .unwrap();
        let pickup_status = pickup_create.status();
        let pickup_text = pickup_create.text().await.unwrap();
        assert_eq!(pickup_status, axum::http::StatusCode::OK, "{pickup_text}");
        let pickup_value: serde_json::Value = serde_json::from_str(&pickup_text).unwrap();
        assert_eq!(pickup_value["data"]["status"], "held");
        assert_eq!(pickup_value["data"]["items"][0]["quantity"], 1);
        assert_eq!(pickup_value["data"]["contactPhone"], "+233200000123");
        let pickup_reservation_id = pickup_value["data"]["reservationId"]
            .as_str()
            .unwrap()
            .to_owned();
        let pickup_replay = client
            .post(&pickup_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "pickup-create-001")
            .json(&pickup_body)
            .send()
            .await
            .unwrap();
        assert_eq!(pickup_replay.status(), axum::http::StatusCode::OK);
        let pickup_replay_value: serde_json::Value = pickup_replay.json().await.unwrap();
        assert_eq!(
            pickup_replay_value["data"]["reservationId"],
            pickup_reservation_id
        );
        assert_eq!(pickup_replay_value["data"]["replayed"], true);
        let pickup_changed=client.post(&pickup_url).bearer_auth("opaque-token")
            .header("idempotency-key","pickup-create-001")
            .json(&serde_json::json!({"pickupBranchId":branch_a_two,"contactName":"Changed","contactPhone":"+233200000123","contactConsent":true,"holdMinutes":60,"items":[{"productId":product_id,"quantity":2}]}))
            .send().await.unwrap();
        assert_eq!(pickup_changed.status(), axum::http::StatusCode::CONFLICT);
        let pickup_queue = client
            .get(&pickup_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(pickup_queue.status(), axum::http::StatusCode::OK);
        assert!(
            pickup_queue.json::<serde_json::Value>().await.unwrap()["data"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["reservationId"] == pickup_reservation_id)
        );
        let sibling_station_queue=client.get(format!(
            "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a2}/pickup-reservations"
        )).bearer_auth("opaque-token").send().await.unwrap();
        assert_eq!(sibling_station_queue.status(), axum::http::StatusCode::OK);
        assert!(sibling_station_queue
            .json::<serde_json::Value>()
            .await
            .unwrap()["data"]
            .as_array()
            .unwrap()
            .is_empty());
        let held_availability=client.get(format!(
            "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a}/products/{product_id}/availability"
        )).bearer_auth("opaque-token").send().await.unwrap().json::<serde_json::Value>().await.unwrap();
        assert!(held_availability["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["branchId"] == branch_a_two
                && row["pickupReservedQuantity"] == 1
                && row["availableToPromise"] == 2
                && row.get("contactPhone").is_none()
                && row.get("contactName").is_none()));

        let pickup_transition_url = format!(
            "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a_two}/stations/{pickup_station}/pickup-reservations/{pickup_reservation_id}/events"
        );
        let ready_body = serde_json::json!({"action":"ready","expectedRevision":1,"reason":null});
        let pickup_ready = client
            .post(&pickup_transition_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "pickup-ready-001")
            .json(&ready_body)
            .send()
            .await
            .unwrap();
        let ready_status = pickup_ready.status();
        let ready_text = pickup_ready.text().await.unwrap();
        assert_eq!(ready_status, axum::http::StatusCode::OK, "{ready_text}");
        let ready_value: serde_json::Value = serde_json::from_str(&ready_text).unwrap();
        assert_eq!(ready_value["data"]["status"], "ready");
        assert_eq!(ready_value["data"]["revision"], 2);
        let pickup_ready_replay = client
            .post(&pickup_transition_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "pickup-ready-001")
            .json(&ready_body)
            .send()
            .await
            .unwrap();
        assert_eq!(pickup_ready_replay.status(), axum::http::StatusCode::OK);
        assert_eq!(
            pickup_ready_replay
                .json::<serde_json::Value>()
                .await
                .unwrap()["data"]["replayed"],
            true
        );

        let cancel_body = serde_json::json!({"action":"cancel","expectedRevision":2,"reason":"Customer changed pickup plans"});
        let pickup_cashier_cancel = client
            .post(&pickup_transition_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "pickup-cashier-cancel-denied-001")
            .json(&cancel_body)
            .send()
            .await
            .unwrap();
        let cashier_cancel_status = pickup_cashier_cancel.status();
        let cashier_cancel_text = pickup_cashier_cancel.text().await.unwrap();
        assert_eq!(
            cashier_cancel_status,
            axum::http::StatusCode::FORBIDDEN,
            "{cashier_cancel_text}"
        );
        sqlx::query("INSERT INTO membership_role_grants(tenant_id,membership_id,role_key,branch_id,station_id) VALUES($1::uuid,$2::uuid,'manager',$3::uuid,NULL) ON CONFLICT DO NOTHING")
            .bind(&tenant_a).bind(&membership_id_two).bind(&branch_a_two)
            .execute(&verification_pool).await.unwrap();
        let manager_pickup_transition_url = format!(
            "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a_two}/pickup-reservations/{pickup_reservation_id}/events"
        );
        let pickup_cancel = client
            .post(&manager_pickup_transition_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "pickup-cancel-001")
            .json(&cancel_body)
            .send()
            .await
            .unwrap();
        let cancel_status = pickup_cancel.status();
        let cancel_text = pickup_cancel.text().await.unwrap();
        assert_eq!(cancel_status, axum::http::StatusCode::OK, "{cancel_text}");
        let cancel_value: serde_json::Value = serde_json::from_str(&cancel_text).unwrap();
        assert_eq!(cancel_value["data"]["status"], "cancelled");
        assert_eq!(cancel_value["data"]["revision"], 3);
        sqlx::query("DELETE FROM membership_role_grants WHERE tenant_id=$1::uuid AND membership_id=$2::uuid AND role_key='manager' AND branch_id=$3::uuid AND station_id IS NULL")
            .bind(&tenant_a).bind(&membership_id_two).bind(&branch_a_two)
            .execute(&verification_pool).await.unwrap();
        let released_pickup = sqlx::query_as::<_, (i64, i64)>(
            "SELECT count(*) FILTER(WHERE released_at IS NOT NULL),count(*) FROM inventory_allocations WHERE tenant_id=$1::uuid AND pickup_reservation_id=$2::uuid",
        ).bind(&tenant_a).bind(&pickup_reservation_id).fetch_one(&verification_pool).await.unwrap();
        assert_eq!(released_pickup, (1, 1));
        let cancelled_availability = client.get(format!(
            "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a}/products/{product_id}/availability"
        )).bearer_auth("opaque-token").send().await.unwrap().json::<serde_json::Value>().await.unwrap();
        assert!(cancelled_availability["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["branchId"] == branch_a_two
                && row["pickupReservedQuantity"] == 0
                && row["availableToPromise"] == 3));

        let transfer_url = format!(
            "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a}/stock-transfers"
        );
        let transfer_request_body = serde_json::json!({
            "sourceBranchId": branch_a_two,
            "note": "Customer needs stock at this branch",
            "items": [{ "productId": product_id, "quantity": 3 }]
        });
        let transfer_request = client
            .post(&transfer_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "branch-transfer-request-001")
            .json(&transfer_request_body)
            .send()
            .await
            .unwrap();
        let transfer_status = transfer_request.status();
        let transfer_response_body = transfer_request.text().await.unwrap();
        assert_eq!(
            transfer_status,
            axum::http::StatusCode::OK,
            "{transfer_response_body}"
        );
        let transfer_body: serde_json::Value =
            serde_json::from_str(&transfer_response_body).unwrap();
        assert_eq!(transfer_body["data"]["status"], "requested");
        assert_eq!(transfer_body["data"]["destinationBranchId"], branch_a);
        assert_eq!(transfer_body["data"]["sourceBranchId"], branch_a_two);
        assert_eq!(transfer_body["data"]["revision"], 1);
        assert_eq!(transfer_body["data"]["replayed"], false);
        assert_eq!(transfer_body["data"]["items"][0]["productId"], product_id);
        assert_eq!(transfer_body["data"]["items"][0]["requestedQuantity"], 3);

        let transfer_replay = client
            .post(&transfer_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "branch-transfer-request-001")
            .json(&transfer_request_body)
            .send()
            .await
            .unwrap();
        assert_eq!(transfer_replay.status(), axum::http::StatusCode::OK);
        let replay_body: serde_json::Value = transfer_replay.json().await.unwrap();
        assert_eq!(
            replay_body["data"]["transferId"],
            transfer_body["data"]["transferId"]
        );
        assert_eq!(replay_body["data"]["replayed"], true);

        let changed_transfer_replay = client
            .post(&transfer_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "branch-transfer-request-001")
            .json(&serde_json::json!({
                "sourceBranchId": branch_a_two,
                "note": "Changed body",
                "items": [{ "productId": product_id, "quantity": 2 }]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            changed_transfer_replay.status(),
            axum::http::StatusCode::CONFLICT
        );

        let transfer_id = transfer_body["data"]["transferId"].as_str().unwrap();
        let branch_permission_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/permissions");
        let branch_permissions = client
            .get(&branch_permission_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(branch_permissions.status(), axum::http::StatusCode::OK);
        let branch_permissions_body: serde_json::Value = branch_permissions.json().await.unwrap();
        assert!(branch_permissions_body["data"]["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|permission| permission == "inventory:transfer:manage"));
        let branch_profile_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/profile");
        let branch_profile = client
            .get(&branch_profile_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(branch_profile.status(), axum::http::StatusCode::OK);
        let branch_profile_body: serde_json::Value = branch_profile.json().await.unwrap();
        assert_eq!(branch_profile_body["data"]["settingsRevision"], 1);
        let branch_hours = serde_json::json!({
            "monday": [{"open":"08:00","close":"12:00"},{"open":"13:00","close":"18:00"}],
            "tuesday": [], "wednesday": [], "thursday": [], "friday": [], "saturday": [], "sunday": []
        });
        let branch_profile_update = serde_json::json!({
            "address":"1 Market Road", "phone":"+233200000001", "email":"branch@example.test",
            "openingHours":branch_hours, "expectedRevision":1
        });
        let branch_profile_saved = client
            .put(&branch_profile_url)
            .bearer_auth("opaque-token")
            .json(&branch_profile_update)
            .send()
            .await
            .unwrap();
        assert_eq!(branch_profile_saved.status(), axum::http::StatusCode::OK);
        let branch_profile_saved_body: serde_json::Value =
            branch_profile_saved.json().await.unwrap();
        assert_eq!(
            branch_profile_saved_body["data"]["address"],
            "1 Market Road"
        );
        assert_eq!(
            branch_profile_saved_body["data"]["openingHours"]["monday"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(branch_profile_saved_body["data"]["settingsRevision"], 2);
        let stale_branch_profile = client
            .put(&branch_profile_url)
            .bearer_auth("opaque-token")
            .json(&branch_profile_update)
            .send()
            .await
            .unwrap();
        assert_eq!(
            stale_branch_profile.status(),
            axum::http::StatusCode::CONFLICT
        );
        let cashier_branch_profile = client
            .put(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/profile"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "address":"Unauthorized", "phone":null, "email":null,
                "openingHours":branch_hours, "expectedRevision":2
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_branch_profile.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let attendance_base =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/attendance");
        let attendance_staff = client
            .get(format!("{attendance_base}/staff"))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(attendance_staff.status(), axum::http::StatusCode::OK);
        let attendance_staff_body: serde_json::Value = attendance_staff.json().await.unwrap();
        assert_eq!(attendance_staff_body["data"].as_array().unwrap().len(), 2);
        assert_eq!(
            attendance_staff_body["data"][0]["displayName"],
            "First Cashier"
        );
        assert!(attendance_staff_body["data"][0].get("subject").is_none());

        let attendance_schedules = client
            .get(format!("{attendance_base}/schedules"))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(attendance_schedules.status(), axum::http::StatusCode::OK);
        assert!(attendance_schedules
            .json::<serde_json::Value>()
            .await
            .unwrap()["data"]
            .as_array()
            .unwrap()
            .is_empty());
        let schedule_input = serde_json::json!({
            "weekdays": [0,1,2,3,4], "startTime": "08:00", "endTime": "17:00",
            "enabled": true, "expectedRevision": 0
        });
        let saved_schedule = client
            .put(format!("{attendance_base}/schedules/{membership_id}"))
            .bearer_auth("opaque-token")
            .json(&schedule_input)
            .send()
            .await
            .unwrap();
        let saved_schedule_status = saved_schedule.status();
        let saved_schedule_body: serde_json::Value = saved_schedule.json().await.unwrap();
        assert_eq!(
            saved_schedule_status,
            axum::http::StatusCode::OK,
            "schedule save response: {saved_schedule_body}"
        );
        assert_eq!(saved_schedule_body["data"]["revision"], 1);
        assert_eq!(saved_schedule_body["data"]["timezone"], "Africa/Accra");
        let stale_attendance_schedule = client
            .put(format!("{attendance_base}/schedules/{membership_id}"))
            .bearer_auth("opaque-token")
            .json(&schedule_input)
            .send()
            .await
            .unwrap();
        assert_eq!(
            stale_attendance_schedule.status(),
            axum::http::StatusCode::CONFLICT
        );
        let cashier_schedule_write = client
            .put(format!("http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/attendance/schedules/{membership_id_two}"))
            .bearer_auth("opaque-token")
            .json(&schedule_input)
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_schedule_write.status(),
            axum::http::StatusCode::FORBIDDEN
        );

        let my_attendance_url = format!("{attendance_base}/me");
        let before_clock_in = client
            .get(&my_attendance_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(before_clock_in.status(), axum::http::StatusCode::OK);
        assert!(
            before_clock_in.json::<serde_json::Value>().await.unwrap()["data"]["currentPeriod"]
                .is_null()
        );

        let clock_in_url = format!("{attendance_base}/me/clock-in");
        let first_clock_in = client
            .post(&clock_in_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "attendance-clock-in-test-001")
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
        let first_clock_in_status = first_clock_in.status();
        let first_clock_in_body: serde_json::Value = first_clock_in.json().await.unwrap();
        assert_eq!(
            first_clock_in_status,
            axum::http::StatusCode::OK,
            "clock-in response: {first_clock_in_body}"
        );
        let attendance_period_id = first_clock_in_body["data"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(first_clock_in_body["data"]["membershipId"], membership_id);
        assert_eq!(first_clock_in_body["data"]["scheduleRevision"], 1);
        let replay_clock_in = client
            .post(&clock_in_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "attendance-clock-in-test-001")
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(replay_clock_in.status(), axum::http::StatusCode::OK);
        assert_eq!(
            replay_clock_in.json::<serde_json::Value>().await.unwrap()["data"]["id"],
            attendance_period_id
        );
        let duplicate_clock_in = client
            .post(&clock_in_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "attendance-clock-in-test-002")
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            duplicate_clock_in.status(),
            axum::http::StatusCode::CONFLICT
        );
        let other_branch_clock_in = client
            .post(format!(
                "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a_two}/attendance/me/clock-in"
            ))
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "attendance-clock-in-other-branch")
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            other_branch_clock_in.status(),
            axum::http::StatusCode::CONFLICT
        );

        let seed_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&seed_url)
            .await
            .unwrap();
        sqlx::query("UPDATE attendance_periods SET started_at=clock_timestamp()-interval '3 seconds',scheduled_start_at=clock_timestamp()-interval '3 seconds',due_at=clock_timestamp()-interval '1 second' WHERE id=$1::uuid")
            .bind(&attendance_period_id).execute(&seed_pool).await.unwrap();
        let worker_url = env::var("POS_API_TEST_ATTENDANCE_WORKER_DATABASE_URL")
            .expect("the verifier must provide the execute-only attendance worker login");
        let worker_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&worker_url)
            .await
            .unwrap();
        let ended: bool = sqlx::query_scalar("SELECT pos_worker.end_next_due_attendance_period()")
            .fetch_one(&worker_pool)
            .await
            .unwrap();
        assert!(
            ended,
            "the worker should close the overdue attendance period once"
        );
        let replayed_end: bool =
            sqlx::query_scalar("SELECT pos_worker.end_next_due_attendance_period()")
                .fetch_one(&worker_pool)
                .await
                .unwrap();
        assert!(
            !replayed_end,
            "a repeated worker pass must not emit a second end event"
        );
        let worker_result: (String, i64, i64) = sqlx::query_as(
            "SELECT end_reason,revision,(SELECT count(*) FROM attendance_events WHERE period_id=$1::uuid AND event_type='scheduled_end') FROM attendance_periods WHERE id=$1::uuid",
        ).bind(&attendance_period_id).fetch_one(&seed_pool).await.unwrap();
        assert_eq!(worker_result, ("schedule".into(), 2, 1));
        worker_pool.close().await;
        seed_pool.close().await;

        let attendance_history = client
            .get(format!("{attendance_base}/periods?limit=20"))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(attendance_history.status(), axum::http::StatusCode::OK);
        let attendance_history_body: serde_json::Value = attendance_history.json().await.unwrap();
        assert!(attendance_history_body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|period| {
                period["id"] == attendance_period_id && period["endReason"] == "schedule"
            }));

        let second_clock_in = client
            .post(&clock_in_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "attendance-clock-in-test-003")
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(second_clock_in.status(), axum::http::StatusCode::OK);
        let second_period: serde_json::Value = second_clock_in.json().await.unwrap();
        let clock_out = client
            .post(format!("{attendance_base}/me/clock-out"))
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "attendance-clock-out-test-001")
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(clock_out.status(), axum::http::StatusCode::OK);
        let clock_out_body: serde_json::Value = clock_out.json().await.unwrap();
        assert_eq!(clock_out_body["data"]["id"], second_period["data"]["id"]);
        assert_eq!(clock_out_body["data"]["endReason"], "staff");
        let replay_clock_out = client
            .post(format!("{attendance_base}/me/clock-out"))
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "attendance-clock-out-test-001")
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(replay_clock_out.status(), axum::http::StatusCode::OK);
        assert_eq!(
            replay_clock_out.json::<serde_json::Value>().await.unwrap()["data"]["id"],
            second_period["data"]["id"]
        );

        let cashier_self_attendance = client
            .get(format!("http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/attendance/me"))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(cashier_self_attendance.status(), axum::http::StatusCode::OK);
        let cashier_attendance_schedules = client
            .get(format!("http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/attendance/schedules"))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_attendance_schedules.status(),
            axum::http::StatusCode::FORBIDDEN
        );

        let accept_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a_two}/stock-transfers/{transfer_id}/events"
        );
        let accept_body = serde_json::json!({
            "action": "accept", "expectedRevision": 1, "note": null,
            "items": [{ "productId": product_id, "quantity": 3 }]
        });
        let accept = client
            .post(&accept_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "transfer-accept-001")
            .json(&accept_body)
            .send()
            .await
            .unwrap();
        let accept_status = accept.status();
        let accept_text = accept.text().await.unwrap();
        assert_eq!(accept_status, axum::http::StatusCode::OK, "{accept_text}");
        let accepted: serde_json::Value = serde_json::from_str(&accept_text).unwrap();
        assert_eq!(accepted["data"]["status"], "accepted");
        assert_eq!(accepted["data"]["revision"], 2);

        let allocated_availability = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a}/products/{product_id}/availability"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(allocated_availability.status(), axum::http::StatusCode::OK);
        let allocated_rows = allocated_availability
            .json::<serde_json::Value>()
            .await
            .unwrap()["data"]
            .as_array()
            .unwrap()
            .clone();
        assert!(allocated_rows.iter().any(|row| {
            row["branchId"] == branch_a_two
                && row["stockQuantity"] == 3
                && row["pickupReservedQuantity"] == 0
                && row["outboundCommittedQuantity"] == 3
                && row["availableToPromise"] == 0
                && row["commitmentShortfall"] == 0
        }));

        let database_stock_guard = sqlx::query(
            "UPDATE branch_catalog_items SET stock_quantity=1
             WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid",
        )
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(&product_id)
        .execute(&verification_pool)
        .await;
        assert!(
            database_stock_guard.is_err(),
            "database guard must preserve accepted transfer commitments"
        );

        let mut allocation_guard_tx = verification_pool.begin().await.unwrap();
        let database_allocation_guard = sqlx::query(
            "INSERT INTO inventory_allocations
                (tenant_id,branch_id,product_id,pickup_reservation_id,quantity)
             VALUES ($1::uuid,$2::uuid,$3::uuid,$4::uuid,2)",
        )
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(&product_id)
        .bind(&pickup_reservation_id)
        .execute(&mut *allocation_guard_tx)
        .await;
        assert_eq!(
            database_allocation_guard
                .unwrap_err()
                .as_database_error()
                .and_then(|error| error.constraint()),
            Some("inventory_allocation_within_stock"),
            "database guard must reject a new commitment above available-to-promise"
        );
        allocation_guard_tx.rollback().await.unwrap();

        let held_stock_adjustment = client
            .post(format!(
                "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a_two}/products/{product_id}/stock-adjustments"
            ))
            .bearer_auth("opaque-token")
            .header("idempotency-key", "reject-adjustment-below-transfer-commitment")
            .json(&serde_json::json!({ "quantityDelta": -3, "note": "Must preserve accepted transfer" }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            held_stock_adjustment.status(),
            axum::http::StatusCode::CONFLICT
        );

        let accept_replay = client
            .post(&accept_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "transfer-accept-001")
            .json(&accept_body)
            .send()
            .await
            .unwrap();
        assert_eq!(accept_replay.status(), axum::http::StatusCode::OK);
        assert_eq!(
            accept_replay.json::<serde_json::Value>().await.unwrap()["data"]["replayed"],
            true
        );

        let dispatch_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a_two}/stock-transfers/{transfer_id}/events"
        );
        let dispatch_body = serde_json::json!({
            "action": "dispatch", "expectedRevision": 2, "note": null,
            "items": [{ "productId": product_id, "quantity": 3 }]
        });
        let dispatch = client
            .post(&dispatch_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "transfer-dispatch-001")
            .json(&dispatch_body)
            .send()
            .await
            .unwrap();
        let dispatch_status = dispatch.status();
        let dispatch_text = dispatch.text().await.unwrap();
        assert_eq!(
            dispatch_status,
            axum::http::StatusCode::OK,
            "{dispatch_text}"
        );
        let dispatched: serde_json::Value = serde_json::from_str(&dispatch_text).unwrap();
        assert_eq!(dispatched["data"]["status"], "in_transit");
        assert_eq!(dispatched["data"]["revision"], 3);
        assert_eq!(dispatched["data"]["items"][0]["dispatchedQuantity"], 3);
        let dispatch_replay = client
            .post(&dispatch_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "transfer-dispatch-001")
            .json(&dispatch_body)
            .send()
            .await
            .unwrap();
        assert_eq!(dispatch_replay.status(), axum::http::StatusCode::OK);
        let dispatch_replay_body: serde_json::Value = dispatch_replay.json().await.unwrap();
        assert_eq!(dispatch_replay_body["data"]["replayed"], true);

        let dispatched_availability = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a}/products/{product_id}/availability"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(dispatched_availability.status(), axum::http::StatusCode::OK);
        let dispatched_rows = dispatched_availability
            .json::<serde_json::Value>()
            .await
            .unwrap()["data"]
            .as_array()
            .unwrap()
            .clone();
        assert!(dispatched_rows.iter().any(|row| {
            row["branchId"] == branch_a_two
                && row["stockQuantity"] == 0
                && row["pickupReservedQuantity"] == 0
                && row["outboundCommittedQuantity"] == 0
                && row["availableToPromise"] == 0
        }));

        let receive_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stock-transfers/{transfer_id}/events"
        );

        let receipt_body = serde_json::json!({
            "action": "receive", "expectedRevision": 3, "note": null,
            "items": [{ "productId": product_id, "quantity": 1 }]
        });
        let receive = client
            .post(&receive_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "transfer-receive-001")
            .json(&receipt_body)
            .send()
            .await
            .unwrap();
        let receive_status = receive.status();
        let receive_text = receive.text().await.unwrap();
        assert_eq!(receive_status, axum::http::StatusCode::OK, "{receive_text}");
        let received: serde_json::Value = serde_json::from_str(&receive_text).unwrap();
        assert_eq!(received["data"]["status"], "partially_received");
        assert_eq!(received["data"]["revision"], 4);
        assert_eq!(received["data"]["items"][0]["receivedQuantity"], 1);
        assert_eq!(
            received["data"]["items"][0]["discrepancyNote"],
            serde_json::Value::Null
        );
        let replay_receipt = client
            .post(&receive_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "transfer-receive-001")
            .json(&receipt_body)
            .send()
            .await
            .unwrap();
        assert_eq!(replay_receipt.status(), axum::http::StatusCode::OK);
        let replay_receipt_body: serde_json::Value = replay_receipt.json().await.unwrap();
        assert_eq!(replay_receipt_body["data"]["replayed"], true);
        assert_eq!(
            replay_receipt_body["data"]["items"][0]["receivedQuantity"],
            1
        );

        let second_receipt_body = serde_json::json!({
            "action": "receive", "expectedRevision": 4, "note": null,
            "items": [{ "productId": product_id, "quantity": 1 }]
        });
        let second_receipt = client
            .post(&receive_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "transfer-receive-002")
            .json(&second_receipt_body)
            .send()
            .await
            .unwrap();
        let second_receipt_status = second_receipt.status();
        let second_receipt_text = second_receipt.text().await.unwrap();
        assert_eq!(
            second_receipt_status,
            axum::http::StatusCode::OK,
            "{second_receipt_text}"
        );
        let second_receipt_data: serde_json::Value =
            serde_json::from_str(&second_receipt_text).unwrap();
        assert_eq!(second_receipt_data["data"]["status"], "partially_received");
        assert_eq!(second_receipt_data["data"]["revision"], 5);
        assert_eq!(
            second_receipt_data["data"]["items"][0]["receivedQuantity"],
            2
        );
        let second_receipt_replay = client
            .post(&receive_url)
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "transfer-receive-002")
            .json(&second_receipt_body)
            .send()
            .await
            .unwrap();
        assert_eq!(second_receipt_replay.status(), axum::http::StatusCode::OK);
        let second_receipt_replay_data: serde_json::Value =
            second_receipt_replay.json().await.unwrap();
        assert_eq!(second_receipt_replay_data["data"]["replayed"], true);
        assert_eq!(
            second_receipt_replay_data["data"]["items"][0]["receivedQuantity"],
            2
        );

        let resolve_shortage_body = serde_json::json!({
            "action": "resolve_discrepancy", "expectedRevision": 5,
            "note": "Supplier issued credit for the missing unit", "items": []
        });
        let resolve_shortage = client
            .post(&receive_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "transfer-resolve-shortage-001")
            .json(&resolve_shortage_body)
            .send()
            .await
            .unwrap();
        let resolve_shortage_status = resolve_shortage.status();
        let resolve_shortage_text = resolve_shortage.text().await.unwrap();
        assert_eq!(
            resolve_shortage_status,
            axum::http::StatusCode::OK,
            "{resolve_shortage_text}"
        );
        let resolved_shortage: serde_json::Value =
            serde_json::from_str(&resolve_shortage_text).unwrap();
        assert_eq!(resolved_shortage["data"]["status"], "discrepancy_resolved");
        assert_eq!(resolved_shortage["data"]["revision"], 6);
        assert_eq!(resolved_shortage["data"]["items"][0]["receivedQuantity"], 2);
        assert_eq!(
            resolved_shortage["data"]["items"][0]["discrepancyNote"],
            "Supplier issued credit for the missing unit"
        );

        let station_transfers = client
            .get(&transfer_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(station_transfers.status(), axum::http::StatusCode::OK);
        let station_transfers_body: serde_json::Value = station_transfers.json().await.unwrap();
        assert!(station_transfers_body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|transfer| {
                transfer["transferId"] == transfer_body["data"]["transferId"]
                    && transfer["items"][0]["requestedQuantity"] == 3
            }));

        let transfer_stock_state = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            "SELECT
                (SELECT stock_quantity FROM branch_catalog_items WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid),
                (SELECT stock_quantity FROM branch_catalog_items WHERE tenant_id=$1::uuid AND branch_id=$4::uuid AND product_id=$3::uuid),
                (SELECT count(*) FROM stock_movements WHERE tenant_id=$1::uuid AND product_id=$3::uuid AND movement_kind IN ('transfer_in','transfer_out')),
                (SELECT count(*) FROM stock_transfer_events WHERE tenant_id=$1::uuid AND transfer_id=$5::uuid AND event_type='requested'),
                (SELECT count(*) FROM inventory_allocations WHERE tenant_id=$1::uuid AND branch_id=$4::uuid AND stock_transfer_id=$5::uuid AND released_at IS NOT NULL)",
        )
        .bind(&tenant_a)
        .bind(&branch_a)
        .bind(&product_id)
        .bind(&branch_a_two)
        .bind(transfer_body["data"]["transferId"].as_str().unwrap())
        .fetch_one(&verification_pool)
        .await
        .unwrap();
        assert_eq!(transfer_stock_state, (12, 0, 3, 1, 1));

        let product_status_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/products/{product_id}");
        let archived_product = client
            .patch(&product_status_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({ "isActive": false }))
            .send()
            .await
            .unwrap();
        assert_eq!(archived_product.status(), axum::http::StatusCode::OK);
        let archived_product_body: serde_json::Value = archived_product.json().await.unwrap();
        assert_eq!(archived_product_body["data"]["isActive"], false);
        let archived_availability = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a}/products/{product_id}/availability"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            archived_availability.status(),
            axum::http::StatusCode::NOT_FOUND
        );
        let branch_statuses: (String, i64, String) = sqlx::query_as(
            "SELECT i.status, i.stock_quantity,
                    (SELECT other.status FROM branch_catalog_items other
                     WHERE other.tenant_id = i.tenant_id AND other.branch_id = $4::uuid
                       AND other.product_id = i.product_id)
             FROM branch_catalog_items i
             WHERE i.tenant_id = $1::uuid AND i.branch_id = $2::uuid AND i.product_id = $3::uuid",
        )
        .bind(&tenant_a)
        .bind(&branch_a)
        .bind(&product_id)
        .bind(&branch_a_two)
        .fetch_one(&verification_pool)
        .await
        .unwrap();
        assert_eq!(
            branch_statuses,
            ("inactive".to_owned(), 12, "active".to_owned())
        );
        let cashier_cannot_archive = client
            .patch(&format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/products/{product_id}"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({ "isActive": false }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_cannot_archive.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let restored_product = client
            .patch(&product_status_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({ "isActive": true }))
            .send()
            .await
            .unwrap();
        assert_eq!(restored_product.status(), axum::http::StatusCode::OK);
        let restored_product_body: serde_json::Value = restored_product.json().await.unwrap();
        assert_eq!(restored_product_body["data"]["isActive"], true);
        assert_eq!(restored_product_body["data"]["revision"], 7);
        let settings_url = format!("{product_status_url}/settings");
        let update_settings = client
            .patch(&settings_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "priceMinor": 1500,
                "taxRateBps": 1000,
                "reorderPoint": 5,
                "targetStockQuantity": 15,
                "expectedRevision": 7
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(update_settings.status(), axum::http::StatusCode::OK);
        let updated_settings_body: serde_json::Value = update_settings.json().await.unwrap();
        assert_eq!(updated_settings_body["data"]["priceMinor"], 1500);
        assert_eq!(updated_settings_body["data"]["stockQuantity"], 12);
        assert_eq!(updated_settings_body["data"]["revision"], 8);
        let other_branch_settings: (i64, i64) = sqlx::query_as(
            "SELECT price_minor, stock_quantity FROM branch_catalog_items
             WHERE tenant_id = $1::uuid AND branch_id = $2::uuid AND product_id = $3::uuid",
        )
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(&product_id)
        .fetch_one(&verification_pool)
        .await
        .unwrap();
        assert_eq!(other_branch_settings, (1300, 0));
        let stale_settings = client
            .patch(&settings_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "priceMinor": 1400,
                "taxRateBps": 1000,
                "reorderPoint": 5,
                "targetStockQuantity": 15,
                "expectedRevision": 5
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(stale_settings.status(), axum::http::StatusCode::CONFLICT);
        let invalid_settings = client
            .patch(&settings_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "priceMinor": 1400,
                "taxRateBps": 1000,
                "reorderPoint": 8,
                "targetStockQuantity": 4,
                "expectedRevision": 7
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            invalid_settings.status(),
            axum::http::StatusCode::BAD_REQUEST
        );
        let cashier_cannot_edit_settings = client
            .patch(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/products/{product_id}/settings"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "priceMinor": 1400,
                "taxRateBps": 1000,
                "reorderPoint": 5,
                "targetStockQuantity": 15,
                "expectedRevision": 4
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_cannot_edit_settings.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let cashier_cannot_edit_identity = client
            .patch(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/products/{product_id}"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "sku": "tea-002",
                "barcode": "123456789",
                "name": "Green Tea Premium",
                "description": "Updated product identity",
                "category": "Grocery",
                "expectedRevision": 1
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_cannot_edit_identity.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let updated_identity = client
            .patch(format!(
                "{base_url}/v1/tenants/{tenant_a}/products/{product_id}"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "sku": "tea-002",
                "barcode": "123456789",
                "name": "Green Tea Premium",
                "description": "Updated product identity",
                "category": "Grocery",
                "expectedRevision": 1
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(updated_identity.status(), axum::http::StatusCode::OK);
        let updated_identity_body: serde_json::Value = updated_identity.json().await.unwrap();
        assert_eq!(updated_identity_body["data"]["sku"], "tea-002");
        assert_eq!(updated_identity_body["data"]["name"], "Green Tea Premium");
        assert_eq!(updated_identity_body["data"]["revision"], 2);
        let shared_identity: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT p.sku, p.name, i.stock_quantity
             FROM catalog_products p
             JOIN branch_catalog_items i ON i.tenant_id = p.tenant_id AND i.product_id = p.id
             WHERE p.tenant_id = $1::uuid AND p.id = $2::uuid
             ORDER BY i.branch_id",
        )
        .bind(&tenant_a)
        .bind(&product_id)
        .fetch_all(&verification_pool)
        .await
        .unwrap();
        assert_eq!(shared_identity.len(), 2);
        assert!(shared_identity
            .iter()
            .all(|row| row.0 == "tea-002" && row.1 == "Green Tea Premium"));
        let mut shared_stock = shared_identity.iter().map(|row| row.2).collect::<Vec<_>>();
        shared_stock.sort_unstable();
        assert_eq!(shared_stock, vec![0, 12]);
        let stale_identity = client
            .patch(format!(
                "{base_url}/v1/tenants/{tenant_a}/products/{product_id}"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "sku": "tea-003",
                "barcode": "123456789",
                "name": "Stale update",
                "description": null,
                "category": null,
                "expectedRevision": 1
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(stale_identity.status(), axum::http::StatusCode::CONFLICT);
        verification_pool.close().await;

        let duplicate_product = client
            .post(&branch_products_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "sku": "tea-002",
                "barcode": null,
                "name": "Duplicate tea",
                "description": null,
                "category": null,
                "priceMinor": 100,
                "costMinor": null,
                "taxRateBps": 0,
                "openingStock": 0,
                "reorderPoint": 0,
                "targetStockQuantity": 0
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(duplicate_product.status(), axum::http::StatusCode::CONFLICT);

        let station_products_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a}/products"
        );
        let cashier_catalog = client
            .get(&station_products_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(cashier_catalog.status(), axum::http::StatusCode::OK);
        let cashier_products: serde_json::Value = cashier_catalog.json().await.unwrap();
        assert_eq!(cashier_products["data"].as_array().unwrap().len(), 1);
        assert_eq!(cashier_products["data"][0]["productId"], product_id);

        let second_cashier_catalog = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a}/products"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(second_cashier_catalog.status(), axum::http::StatusCode::OK);
        let second_cashier_products: serde_json::Value =
            second_cashier_catalog.json().await.unwrap();
        assert_eq!(second_cashier_products["data"].as_array().unwrap().len(), 1);
        assert_eq!(second_cashier_products["data"][0]["productId"], product_id);

        let second_cashier_cannot_create = client
            .post(&format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/products"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "sku": "CASHIER-001",
                "barcode": null,
                "name": "Unauthorized",
                "description": null,
                "category": null,
                "priceMinor": 100,
                "costMinor": null,
                "taxRateBps": 0,
                "openingStock": 0,
                "reorderPoint": 0,
                "targetStockQuantity": 0
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            second_cashier_cannot_create.status(),
            axum::http::StatusCode::FORBIDDEN
        );

        let second_cashier_cannot_adjust_stock = client
            .post(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/products/{product_id}/stock-adjustments"
            ))
            .bearer_auth("opaque-token")
            .header("idempotency-key", "cashier-stock-attempt-01")
            .json(&serde_json::json!({
                "quantityDelta": -1,
                "note": "Cashier must not adjust stock"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            second_cashier_cannot_adjust_stock.status(),
            axum::http::StatusCode::FORBIDDEN
        );

        let cross_tenant_catalog = client
            .get(format!(
                "{base_url}/v1/tenants/{tenant_b}/branches/{branch_b}/products"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cross_tenant_catalog.status(),
            axum::http::StatusCode::NOT_FOUND
        );

        let activation_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a}/activation"
        );
        let activation_url_two = format!(
            "http://{api_address_two}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a2}/activation"
        );
        let payload_a =
            signed_activation_payload(&client, &base_url, &tenant_a, &branch_a, &station_a, 0xa1)
                .await;
        let payload_a2 = signed_activation_payload(
            &client,
            &format!("http://{api_address_two}"),
            &tenant_a,
            &branch_a,
            &station_a2,
            0xb2,
        )
        .await;
        let (activation, activation_two) = tokio::join!(
            client
                .post(&activation_url)
                .bearer_auth("opaque-token")
                .json(&payload_a)
                .send(),
            client
                .post(&activation_url_two)
                .bearer_auth("opaque-token")
                .json(&payload_a2)
                .send()
        );
        let activation = activation.unwrap();
        let activation_two = activation_two.unwrap();
        let activation_status = activation.status();
        let activation_two_status = activation_two.status();
        assert!(
            (activation_status == axum::http::StatusCode::OK
                && activation_two_status == axum::http::StatusCode::CONFLICT)
                || (activation_status == axum::http::StatusCode::CONFLICT
                    && activation_two_status == axum::http::StatusCode::OK),
            "concurrent activations must allocate exactly one licensed station slot"
        );
        let (active_station_url, active_body, over_capacity_url, over_capacity_body) =
            if activation_status == axum::http::StatusCode::OK {
                (
                    activation_url,
                    activation.json::<serde_json::Value>().await.unwrap(),
                    activation_url_two,
                    activation_two.json::<serde_json::Value>().await.unwrap(),
                )
            } else {
                (
                    activation_url_two,
                    activation_two.json::<serde_json::Value>().await.unwrap(),
                    activation_url,
                    activation.json::<serde_json::Value>().await.unwrap(),
                )
            };
        assert_eq!(active_body["data"]["activeStationCount"], 1);
        assert_eq!(active_body["data"]["stationLimit"], 1);
        assert_eq!(active_body["data"]["created"], true);
        assert_eq!(
            over_capacity_body["error"]["code"],
            "station_capacity_reached"
        );
        let capacity_after_activation = client
            .get(&station_capacity_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            capacity_after_activation.status(),
            axum::http::StatusCode::OK
        );
        let capacity_after_activation_body: serde_json::Value =
            capacity_after_activation.json().await.unwrap();
        assert_eq!(capacity_after_activation_body["data"]["activeStations"], 1);
        assert_eq!(capacity_after_activation_body["data"]["availableSlots"], 0);

        let idempotent_payload = signed_activation_payload(
            &client,
            &base_url,
            &tenant_a,
            &branch_a,
            if active_station_url.contains(&station_a2) {
                &station_a2
            } else {
                &station_a
            },
            if active_station_url.contains(&station_a2) {
                0xb2
            } else {
                0xa1
            },
        )
        .await;
        let mut invalid_proof_payload = idempotent_payload.clone();
        invalid_proof_payload["signature"] = serde_json::json!("00".repeat(64));
        let invalid_proof = client
            .post(&active_station_url)
            .bearer_auth("opaque-token")
            .json(&invalid_proof_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(invalid_proof.status(), axum::http::StatusCode::BAD_REQUEST);
        let idempotent_activation = client
            .post(&active_station_url)
            .bearer_auth("opaque-token")
            .json(&idempotent_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(idempotent_activation.status(), axum::http::StatusCode::OK);
        let repeated_body: serde_json::Value = idempotent_activation.json().await.unwrap();
        assert_eq!(repeated_body["data"]["created"], false);
        let replayed_activation = client
            .post(&active_station_url)
            .bearer_auth("opaque-token")
            .json(&idempotent_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            replayed_activation.status(),
            axum::http::StatusCode::CONFLICT
        );
        let replayed_activation_body: serde_json::Value = replayed_activation.json().await.unwrap();
        assert_eq!(
            replayed_activation_body["error"]["code"],
            "activation_challenge_invalid"
        );

        let base_url_two = format!("http://{api_address_two}");
        let (active_base_url, alternate_base_url) = if active_station_url.starts_with(&base_url_two)
        {
            (&base_url_two, &base_url)
        } else {
            (&base_url, &base_url_two)
        };
        let active_station_id = if active_station_url.contains(&station_a2) {
            &station_a2
        } else {
            &station_a
        };
        let active_station_seed = if active_station_id == &station_a2 {
            0xb2
        } else {
            0xa1
        };
        let active_activation_id = active_body["data"]["activationId"].as_str().unwrap();
        let station_lease_path = format!(
            "/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/actor-lease"
        );
        let lease_url = format!("{active_base_url}{station_lease_path}");
        let lease_url_two = format!("{alternate_base_url}{station_lease_path}");
        let lease_payload = signed_lease_payload(
            &client,
            active_base_url,
            &tenant_a,
            &branch_a,
            active_station_id,
            &membership_id,
            &identity_id,
            active_activation_id,
            active_station_seed,
        )
        .await;
        let competing_lease_payload = signed_lease_payload(
            &client,
            alternate_base_url,
            &tenant_a,
            &branch_a,
            active_station_id,
            &membership_id,
            &identity_id,
            active_activation_id,
            active_station_seed,
        )
        .await;
        let mut forged_lease_payload = lease_payload.clone();
        forged_lease_payload["signature"] = serde_json::json!("00".repeat(64));
        let forged_lease = client
            .post(&lease_url)
            .bearer_auth("opaque-token")
            .json(&forged_lease_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            forged_lease.status(),
            axum::http::StatusCode::FORBIDDEN,
            "a forged station signature must not acquire a lease"
        );
        let (lease, competing_lease) = tokio::join!(
            client
                .post(&lease_url)
                .bearer_auth("opaque-token")
                .json(&lease_payload)
                .send(),
            client
                .post(&lease_url_two)
                .bearer_auth("opaque-token")
                .json(&competing_lease_payload)
                .send()
        );
        let lease = lease.unwrap();
        let competing_lease = competing_lease.unwrap();
        assert!(
            (lease.status() == axum::http::StatusCode::OK
                && competing_lease.status() == axum::http::StatusCode::CONFLICT)
                || (lease.status() == axum::http::StatusCode::CONFLICT
                    && competing_lease.status() == axum::http::StatusCode::OK),
            "concurrent hosted cashier lease acquisition must grant exactly one actor"
        );
        let lease_body: serde_json::Value = if lease.status() == axum::http::StatusCode::OK {
            lease.json().await.unwrap()
        } else {
            competing_lease.json().await.unwrap()
        };
        let lease_id = lease_body["data"]["leaseId"].as_str().unwrap();
        let lease_token = lease_body["data"]["token"].as_str().unwrap();
        assert_eq!(lease_token.len(), 64);

        let sale_seed_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&seed_url)
            .await
            .unwrap();
        sqlx::query("UPDATE branch_catalog_items SET price_minor=1000,tax_rate_bps=0,stock_quantity=5,status='active' WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid")
            .bind(&tenant_a).bind(&branch_a).bind(&product_id).execute(&sale_seed_pool).await.unwrap();
        sqlx::query("UPDATE branch_payment_options SET enabled=true WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3::uuid")
            .bind(&tenant_a).bind(&branch_a).bind(&payment_option_id).execute(&sale_seed_pool).await.unwrap();
        sqlx::query("UPDATE station_actor_leases SET expires_at=now()+interval '15 minutes' WHERE id=$1::uuid AND status='issued'")
            .bind(lease_id).execute(&sale_seed_pool).await.unwrap();

        // Exercise branch sharing from a second active station with two distinct
        // station leases. The tenant fixture normally allows one station; raise
        // it temporarily, then revoke this test station and restore the limit.
        sqlx::query("UPDATE tenant_entitlements SET numeric_limit=2 WHERE tenant_id=$1::uuid AND entitlement_key='stations.active'")
            .bind(&tenant_a).execute(&sale_seed_pool).await.unwrap();
        let second_station_id = if active_station_id == &station_a {
            station_a2.clone()
        } else {
            station_a.clone()
        };
        let second_station_seed = if second_station_id == station_a {
            0xa1
        } else {
            0xb2
        };
        let second_station_activation_payload = signed_activation_payload(
            &client,
            active_base_url,
            &tenant_a,
            &branch_a,
            &second_station_id,
            second_station_seed,
        )
        .await;
        let second_station_activation_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{second_station_id}/activation"
        );
        let second_station_activation_response = client
            .post(&second_station_activation_url)
            .bearer_auth("opaque-token")
            .json(&second_station_activation_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            second_station_activation_response.status(),
            axum::http::StatusCode::OK
        );
        let second_station_activation: serde_json::Value =
            second_station_activation_response.json().await.unwrap();
        let second_activation_id = second_station_activation["data"]["activationId"]
            .as_str()
            .unwrap();
        let second_station_lease_payload = signed_lease_payload(
            &client,
            active_base_url,
            &tenant_a,
            &branch_a,
            &second_station_id,
            &membership_id,
            &identity_id,
            second_activation_id,
            second_station_seed,
        )
        .await;
        let second_station_lease_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{second_station_id}/actor-lease"
        );
        let second_station_lease_response = client
            .post(&second_station_lease_url)
            .bearer_auth("opaque-token")
            .json(&second_station_lease_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            second_station_lease_response.status(),
            axum::http::StatusCode::OK
        );
        let second_station_lease: serde_json::Value =
            second_station_lease_response.json().await.unwrap();
        let second_lease_id = second_station_lease["data"]["leaseId"]
            .as_str()
            .unwrap()
            .to_owned();
        let second_lease_token = second_station_lease["data"]["token"]
            .as_str()
            .unwrap()
            .to_owned();
        sqlx::query("UPDATE station_actor_leases SET expires_at=now()+interval '15 minutes' WHERE id=$1::uuid AND status='issued'")
            .bind(&second_lease_id).execute(&sale_seed_pool).await.unwrap();

        let shared_payload = serde_json::json!({
            "label": "Two station retry test",
            "lines": [{"productId": product_id, "quantity": 1}],
            "selectedCustomerId": "",
            "selectedPaymentId": payment_option_id,
            "reference": "",
            "pickupSale": null
        });
        let shared_create_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/held-tickets"
        );
        let shared_create = client
            .post(&shared_create_url)
            .bearer_auth("opaque-token")
            .header("X-Station-Lease-Id", lease_id)
            .header("X-Station-Lease-Token", lease_token)
            .header("Idempotency-Key", "shared-ticket-create-integration-001")
            .json(&serde_json::json!({"payload": shared_payload}))
            .send()
            .await
            .unwrap();
        assert_eq!(shared_create.status(), axum::http::StatusCode::OK);
        let shared_created: serde_json::Value = shared_create.json().await.unwrap();
        let shared_ticket_id = shared_created["data"]["id"].as_str().unwrap().to_owned();
        let shared_created_revision = shared_created["data"]["revision"].as_i64().unwrap();
        let shared_create_retry = client
            .post(&shared_create_url)
            .bearer_auth("opaque-token")
            .header("X-Station-Lease-Id", lease_id)
            .header("X-Station-Lease-Token", lease_token)
            .header("Idempotency-Key", "shared-ticket-create-integration-001")
            .json(&serde_json::json!({"payload": shared_payload}))
            .send()
            .await
            .unwrap();
        assert_eq!(shared_create_retry.status(), axum::http::StatusCode::OK);
        assert_eq!(
            shared_create_retry
                .json::<serde_json::Value>()
                .await
                .unwrap()["data"]["id"],
            shared_ticket_id
        );
        let shared_release_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/held-tickets/{shared_ticket_id}/release"
        );
        let shared_release = client
            .post(&shared_release_url)
            .bearer_auth("opaque-token")
            .header("X-Station-Lease-Id", lease_id)
            .header("X-Station-Lease-Token", lease_token)
            .json(&serde_json::json!({"expectedRevision": shared_created_revision}))
            .send()
            .await
            .unwrap();
        assert_eq!(shared_release.status(), axum::http::StatusCode::OK);
        let released_shared: serde_json::Value = shared_release.json().await.unwrap();
        let released_revision = released_shared["data"]["revision"].as_i64().unwrap();
        let shared_ticket_branch_url =
            format!("{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/held-tickets");
        let branch_shared_list = client
            .get(&shared_ticket_branch_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(branch_shared_list.status(), axum::http::StatusCode::OK);
        assert!(branch_shared_list
            .json::<serde_json::Value>()
            .await
            .unwrap()["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|ticket| ticket["id"] == shared_ticket_id));

        let claim_url_one = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/held-tickets/{shared_ticket_id}/claim"
        );
        let claim_url_two = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{second_station_id}/held-tickets/{shared_ticket_id}/claim"
        );
        let (claim_one, claim_two) = tokio::join!(
            client
                .post(&claim_url_one)
                .bearer_auth("opaque-token")
                .header("X-Station-Lease-Id", lease_id)
                .header("X-Station-Lease-Token", lease_token)
                .json(&serde_json::json!({"expectedRevision": released_revision}))
                .send(),
            client
                .post(&claim_url_two)
                .bearer_auth("opaque-token")
                .header("X-Station-Lease-Id", &second_lease_id)
                .header("X-Station-Lease-Token", &second_lease_token)
                .json(&serde_json::json!({"expectedRevision": released_revision}))
                .send()
        );
        let claim_one = claim_one.unwrap();
        let claim_two = claim_two.unwrap();
        assert!(
            (claim_one.status() == axum::http::StatusCode::OK
                && claim_two.status() == axum::http::StatusCode::CONFLICT)
                || (claim_two.status() == axum::http::StatusCode::OK
                    && claim_one.status() == axum::http::StatusCode::CONFLICT),
            "two station leases must not claim the same shared ticket"
        );
        let (winner_station_id, winner_lease_id, winner_lease_token, winner_claim) =
            if claim_one.status() == axum::http::StatusCode::OK {
                (
                    active_station_id.to_owned(),
                    lease_id.to_owned(),
                    lease_token.to_owned(),
                    claim_one.json::<serde_json::Value>().await.unwrap(),
                )
            } else {
                (
                    second_station_id.clone(),
                    second_lease_id.clone(),
                    second_lease_token.clone(),
                    claim_two.json::<serde_json::Value>().await.unwrap(),
                )
            };
        let winner_revision = winner_claim["data"]["revision"].as_i64().unwrap();
        let winner_ticket_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{winner_station_id}/held-tickets/{shared_ticket_id}"
        );
        let changed_shared_payload = serde_json::json!({
            "label": "Updated by winning station",
            "lines": [{"productId": product_id, "quantity": 2}],
            "selectedCustomerId": "",
            "selectedPaymentId": payment_option_id,
            "reference": "",
            "pickupSale": null
        });
        let shared_save = client.put(&winner_ticket_url).bearer_auth("opaque-token")
            .header("X-Station-Lease-Id", &winner_lease_id)
            .header("X-Station-Lease-Token", &winner_lease_token)
            .json(&serde_json::json!({"expectedRevision": winner_revision, "payload": changed_shared_payload})).send().await.unwrap();
        assert_eq!(shared_save.status(), axum::http::StatusCode::OK);
        let shared_saved: serde_json::Value = shared_save.json().await.unwrap();
        let saved_revision = shared_saved["data"]["revision"].as_i64().unwrap();
        let checkout_attempt_url = format!("{winner_ticket_url}/checkout-attempt");
        let exact_shared_body = serde_json::json!({
            "customerId": null,
            "items": [{"productId": product_id, "quantity": 2}],
            "tenders": [{"paymentOptionId": payment_option_id, "amountMinor": 2000, "reference": null}],
            "heldTicketId": shared_ticket_id
        });
        let checkout_attempt_request = serde_json::json!({
            "expectedRevision": saved_revision,
            "idempotencyKey": "shared-ticket-checkout-integration-001",
            "body": exact_shared_body
        });
        let prepared_shared = client
            .post(&checkout_attempt_url)
            .bearer_auth("opaque-token")
            .header("X-Station-Lease-Id", &winner_lease_id)
            .header("X-Station-Lease-Token", &winner_lease_token)
            .json(&checkout_attempt_request)
            .send()
            .await
            .unwrap();
        assert_eq!(prepared_shared.status(), axum::http::StatusCode::OK);
        let prepared_shared: serde_json::Value = prepared_shared.json().await.unwrap();
        assert_eq!(prepared_shared["data"]["status"], "checkout_pending");
        let prepared_revision = prepared_shared["data"]["revision"].as_i64().unwrap();
        let prepare_retry_request = serde_json::json!({
            "expectedRevision": saved_revision,
            "idempotencyKey": "shared-ticket-checkout-integration-001",
            "body": exact_shared_body
        });
        let prepare_retry = client
            .post(&checkout_attempt_url)
            .bearer_auth("opaque-token")
            .header("X-Station-Lease-Id", &winner_lease_id)
            .header("X-Station-Lease-Token", &winner_lease_token)
            .json(&prepare_retry_request)
            .send()
            .await
            .unwrap();
        assert_eq!(prepare_retry.status(), axum::http::StatusCode::OK);
        assert_eq!(
            prepare_retry.json::<serde_json::Value>().await.unwrap()["data"]["revision"],
            prepared_revision
        );
        let clear_attempt_url = format!("{winner_ticket_url}/checkout-attempt/clear");
        let clear_attempt = client
            .post(&clear_attempt_url)
            .bearer_auth("opaque-token")
            .header("X-Station-Lease-Id", &winner_lease_id)
            .header("X-Station-Lease-Token", &winner_lease_token)
            .json(&serde_json::json!({"expectedRevision": prepared_revision}))
            .send()
            .await
            .unwrap();
        assert_eq!(clear_attempt.status(), axum::http::StatusCode::OK);
        let cleared_ticket: serde_json::Value = clear_attempt.json().await.unwrap();
        assert_eq!(cleared_ticket["data"]["status"], "held");
        let final_release_url = format!("{winner_ticket_url}/release");
        let final_release = client
            .post(&final_release_url)
            .bearer_auth("opaque-token")
            .header("X-Station-Lease-Id", &winner_lease_id)
            .header("X-Station-Lease-Token", &winner_lease_token)
            .json(&serde_json::json!({"expectedRevision": cleared_ticket["data"]["revision"]}))
            .send()
            .await
            .unwrap();
        assert_eq!(final_release.status(), axum::http::StatusCode::OK);

        let second_lease_release = client
            .post(format!(
                "{active_base_url}/v1/station-actor-leases/{second_lease_id}/release"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({"token": second_lease_token}))
            .send()
            .await
            .unwrap();
        assert_eq!(second_lease_release.status(), axum::http::StatusCode::OK);
        let second_station_revocation_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{second_station_id}/activation-revocation"
        );
        let second_station_revocation = client
            .post(second_station_revocation_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({"reason":"Temporary integration verification station"}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            second_station_revocation.status(),
            axum::http::StatusCode::OK
        );
        sqlx::query("UPDATE tenant_entitlements SET numeric_limit=1 WHERE tenant_id=$1::uuid AND entitlement_key='stations.active'")
            .bind(&tenant_a).execute(&sale_seed_pool).await.unwrap();

        let checkout_lease_heartbeat = client
            .post(format!(
                "{active_base_url}/v1/station-actor-leases/{lease_id}/heartbeat"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({"token": lease_token}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            checkout_lease_heartbeat.status(),
            axum::http::StatusCode::OK
        );
        let consumable_pickup_url=format!(
            "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a_two}/stations/{pickup_station}/pickup-reservations"
        );
        let consumable_pickup_create = client
            .post(&consumable_pickup_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "pickup-consume-create-001")
            .json(&serde_json::json!({
                "pickupBranchId":branch_a,
                "contactName":"Checkout pickup",
                "contactPhone":"+233200000321",
                "contactConsent":true,
                "holdMinutes":60,
                "items":[{"productId":product_id,"quantity":1}]
            }))
            .send()
            .await
            .unwrap();
        let consumable_pickup_status = consumable_pickup_create.status();
        let consumable_pickup_text = consumable_pickup_create.text().await.unwrap();
        assert_eq!(
            consumable_pickup_status,
            axum::http::StatusCode::OK,
            "{consumable_pickup_text}"
        );
        let consumable_pickup_value: serde_json::Value =
            serde_json::from_str(&consumable_pickup_text).unwrap();
        let consumable_pickup_id = consumable_pickup_value["data"]["reservationId"]
            .as_str()
            .unwrap()
            .to_owned();
        let consumable_pickup_ready_url=format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/pickup-reservations/{consumable_pickup_id}/events"
        );
        let consumable_pickup_ready = client
            .post(&consumable_pickup_ready_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "pickup-consume-ready-001")
            .json(&serde_json::json!({"action":"ready","expectedRevision":1,"reason":null}))
            .send()
            .await
            .unwrap();
        let consumable_pickup_ready_status = consumable_pickup_ready.status();
        let consumable_pickup_ready_text = consumable_pickup_ready.text().await.unwrap();
        assert_eq!(
            consumable_pickup_ready_status,
            axum::http::StatusCode::OK,
            "{consumable_pickup_ready_text}"
        );
        let consumable_pickup_ready_value: serde_json::Value =
            serde_json::from_str(&consumable_pickup_ready_text).unwrap();
        assert_eq!(consumable_pickup_ready_value["data"]["status"], "ready");
        assert_eq!(consumable_pickup_ready_value["data"]["revision"], 2);
        let checkout_challenge_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/checkout-challenge"
        );
        let checkout_payload = serde_json::json!({
            "customerId": customer_id,
            "items": [{"productId": product_id, "quantity": 1}],
            "tenders": [{"paymentOptionId": payment_option_id, "amountMinor": 1000, "reference": "test-approved-reference"}],
            "pickupReservation":{"reservationId":consumable_pickup_id,"expectedRevision":2}
        });
        let sale_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/sales"
        );
        let mut failed_pickup_checkout_payload = checkout_payload.clone();
        failed_pickup_checkout_payload["tenders"][0]["paymentOptionId"] =
            serde_json::Value::String(Uuid::new_v4().to_string());
        let (failed_pickup_challenge, failed_pickup_proof, failed_pickup_bytes) =
            signed_financial_proof(
                &client,
                &checkout_challenge_url,
                &tenant_a,
                &branch_a,
                active_station_id,
                lease_id,
                lease_token,
                "pickup-checkout-failed-001",
                "sale.commit",
                None,
                &failed_pickup_checkout_payload,
                active_station_seed,
            )
            .await;
        let failed_pickup_checkout = client
            .post(&sale_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "pickup-checkout-failed-001")
            .header("x-station-challenge-id", &failed_pickup_challenge)
            .header("x-station-proof", &failed_pickup_proof)
            .body(failed_pickup_bytes)
            .send()
            .await
            .unwrap();
        assert_eq!(
            failed_pickup_checkout.status(),
            axum::http::StatusCode::CONFLICT
        );
        let preserved_pickup=sqlx::query_as::<_,(String,i64,Option<String>,bool)>(
            "SELECT r.status,r.revision,r.sale_id::text,a.released_at IS NOT NULL
             FROM pickup_reservations r JOIN inventory_allocations a ON a.tenant_id=r.tenant_id AND a.pickup_reservation_id=r.id
             WHERE r.tenant_id=$1::uuid AND r.id=$2::uuid"
        ).bind(&tenant_a).bind(&consumable_pickup_id).fetch_one(&sale_seed_pool).await.unwrap();
        assert_eq!(
            preserved_pickup,
            ("ready".to_owned(), 2, None, false),
            "failed checkout must keep the ready hold and its allocation"
        );
        let checkout_payload_bytes = serde_json::to_vec(&checkout_payload).unwrap();
        let payload_hash = Sha256::digest(&checkout_payload_bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let invalid_checkout_lease = client
            .post(&checkout_challenge_url)
            .bearer_auth("opaque-token")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", "00".repeat(32))
            .header("idempotency-key", "checkout-proof-01")
            .json(&serde_json::json!({"payloadHash": payload_hash.clone()}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            invalid_checkout_lease.status(),
            axum::http::StatusCode::CONFLICT
        );

        let checkout_challenge = client
            .post(&checkout_challenge_url)
            .bearer_auth("opaque-token")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "checkout-proof-01")
            .json(&serde_json::json!({"payloadHash": payload_hash.clone()}))
            .send()
            .await
            .unwrap();
        assert_eq!(checkout_challenge.status(), axum::http::StatusCode::OK);
        let checkout_challenge_body: serde_json::Value = checkout_challenge.json().await.unwrap();
        let challenge_id = checkout_challenge_body["data"]["challengeId"]
            .as_str()
            .unwrap();
        let nonce = checkout_challenge_body["data"]["nonce"].as_str().unwrap();
        let expected_signed_message = format!(
            "advanced-pos/financial-operation/v1\nPOST\n/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/sales\nsale.commit\n{tenant_a}\n{branch_a}\n{active_station_id}\n{lease_id}\n{membership_id}\n{active_activation_id}\n{challenge_id}\ncheckout-proof-01\n{payload_hash}\n{nonce}"
        );
        assert_eq!(
            checkout_challenge_body["data"]["messageToSign"],
            expected_signed_message
        );
        let sale_message = checkout_challenge_body["data"]["messageToSign"]
            .as_str()
            .unwrap();
        let sale_signature = SigningKey::from_bytes(&[active_station_seed; 32])
            .sign(sale_message.as_bytes())
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let sale_response = client
            .post(&sale_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "checkout-proof-01")
            .header("x-station-challenge-id", challenge_id)
            .header("x-station-proof", &sale_signature)
            .body(checkout_payload_bytes.clone())
            .send()
            .await
            .unwrap();
        let sale_status = sale_response.status();
        let sale_body: serde_json::Value = sale_response.json().await.unwrap();
        assert_eq!(
            sale_status,
            axum::http::StatusCode::OK,
            "hosted sale failed: {sale_body}"
        );
        assert_eq!(sale_body["data"]["totalMinor"], 1000);
        assert_eq!(sale_body["data"]["cashierName"], "First");
        assert_eq!(sale_body["data"]["currencyCode"], "GHS");
        assert_eq!(sale_body["data"]["customerId"], customer_id);
        assert_eq!(sale_body["data"]["replayed"], false);
        let committed_sale_id = sale_body["data"]["id"].as_str().unwrap();
        let collected_pickup=sqlx::query_as::<_,(String,i64,Option<String>,i64,i64)>(
            "SELECT r.status,r.revision,r.sale_id::text,
                    (SELECT count(*) FROM pickup_reservation_events e WHERE e.tenant_id=r.tenant_id AND e.reservation_id=r.id AND e.event_type='collected'),
                    (SELECT count(*) FROM inventory_allocations a WHERE a.tenant_id=r.tenant_id AND a.pickup_reservation_id=r.id AND a.released_at IS NOT NULL AND a.release_reason='collected')
             FROM pickup_reservations r WHERE r.tenant_id=$1::uuid AND r.id=$2::uuid"
        ).bind(&tenant_a).bind(&consumable_pickup_id).fetch_one(&sale_seed_pool).await.unwrap();
        assert_eq!(
            collected_pickup,
            (
                "collected".to_owned(),
                3,
                Some(committed_sale_id.to_owned()),
                1,
                1
            )
        );
        let expiring_pickup_create = client
            .post(&consumable_pickup_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "pickup-expiry-create-001")
            .json(&serde_json::json!({
                "pickupBranchId":branch_a,
                "contactName":"Expiry verification",
                "contactPhone":"+233200000322",
                "contactConsent":true,
                "holdMinutes":15,
                "items":[{"productId":product_id,"quantity":1}]
            }))
            .send()
            .await
            .unwrap();
        let expiring_pickup_status = expiring_pickup_create.status();
        let expiring_pickup_text = expiring_pickup_create.text().await.unwrap();
        assert_eq!(
            expiring_pickup_status,
            axum::http::StatusCode::OK,
            "{expiring_pickup_text}"
        );
        let expiring_pickup_value: serde_json::Value =
            serde_json::from_str(&expiring_pickup_text).unwrap();
        let expiring_pickup_id = expiring_pickup_value["data"]["reservationId"]
            .as_str()
            .unwrap()
            .to_owned();
        let stock_before_expiry = sqlx::query_scalar::<_, i64>(
            "SELECT stock_quantity FROM branch_catalog_items
             WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid",
        )
        .bind(&tenant_a)
        .bind(&branch_a)
        .bind(&product_id)
        .fetch_one(&sale_seed_pool)
        .await
        .unwrap();
        let not_yet_due = sqlx::query_scalar::<_, bool>(
            "SELECT pos_worker.materialize_next_expired_pickup_reservation()",
        )
        .fetch_one(&sale_seed_pool)
        .await
        .unwrap();
        assert!(!not_yet_due, "a future hold must not be materialized early");
        let held_before_deadline = sqlx::query_as::<_, (String, i64, bool)>(
            "SELECT r.status,r.revision,a.released_at IS NOT NULL
             FROM pickup_reservations r JOIN inventory_allocations a
               ON a.tenant_id=r.tenant_id AND a.pickup_reservation_id=r.id
             WHERE r.tenant_id=$1::uuid AND r.id=$2::uuid",
        )
        .bind(&tenant_a)
        .bind(&expiring_pickup_id)
        .fetch_one(&sale_seed_pool)
        .await
        .unwrap();
        assert_eq!(held_before_deadline, ("held".to_owned(), 1, false));

        let mut expire_seed = sale_seed_pool.begin().await.unwrap();
        sqlx::query(
            "ALTER TABLE pickup_reservations DISABLE TRIGGER pickup_reservations_transition_guard",
        )
        .execute(&mut *expire_seed)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE pickup_reservations
             SET created_at=clock_timestamp()-interval '2 seconds',
                 expires_at=clock_timestamp()-interval '1 second'
             WHERE tenant_id=$1::uuid AND id=$2::uuid",
        )
        .bind(&tenant_a)
        .bind(&expiring_pickup_id)
        .execute(&mut *expire_seed)
        .await
        .unwrap();
        expire_seed.commit().await.unwrap();
        sqlx::query(
            "UPDATE inventory_allocations
             SET expires_at=clock_timestamp()-interval '1 second'
             WHERE tenant_id=$1::uuid AND pickup_reservation_id=$2::uuid AND released_at IS NULL",
        )
        .bind(&tenant_a)
        .bind(&expiring_pickup_id)
        .execute(&sale_seed_pool)
        .await
        .unwrap();
        sqlx::query(
            "ALTER TABLE pickup_reservations ENABLE TRIGGER pickup_reservations_transition_guard",
        )
        .execute(&sale_seed_pool)
        .await
        .unwrap();

        let deadline_no_longer_allocates = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(SUM(quantity),0)::bigint FROM inventory_allocations
             WHERE tenant_id=$1::uuid AND pickup_reservation_id=$2::uuid
               AND released_at IS NULL AND (expires_at IS NULL OR expires_at>clock_timestamp())",
        )
        .bind(&tenant_a)
        .bind(&expiring_pickup_id)
        .fetch_one(&sale_seed_pool)
        .await
        .unwrap();
        assert_eq!(
            deadline_no_longer_allocates, 0,
            "the deadline remains authoritative while the worker is idle"
        );
        assert!(sqlx::query_scalar::<_, bool>(
            "SELECT pos_worker.materialize_next_expired_pickup_reservation()",
        )
        .fetch_one(&sale_seed_pool)
        .await
        .unwrap());
        assert!(
            !sqlx::query_scalar::<_, bool>(
                "SELECT pos_worker.materialize_next_expired_pickup_reservation()",
            )
            .fetch_one(&sale_seed_pool)
            .await
            .unwrap(),
            "replay must not materialize or audit the same hold twice"
        );
        let expired_pickup = sqlx::query_as::<_, (String, i64, Option<String>, i64, i64)>(
            "SELECT r.status,r.revision,r.sale_id::text,
                    (SELECT count(*) FROM pickup_reservation_events e WHERE e.tenant_id=r.tenant_id AND e.reservation_id=r.id AND e.event_type='expired' AND e.actor_kind='system' AND e.actor_membership_id IS NULL),
                    (SELECT count(*) FROM inventory_allocations a WHERE a.tenant_id=r.tenant_id AND a.pickup_reservation_id=r.id AND a.released_at IS NOT NULL AND a.release_reason='expired')
             FROM pickup_reservations r WHERE r.tenant_id=$1::uuid AND r.id=$2::uuid",
        )
        .bind(&tenant_a)
        .bind(&expiring_pickup_id)
        .fetch_one(&sale_seed_pool)
        .await
        .unwrap();
        assert_eq!(expired_pickup, ("expired".to_owned(), 2, None, 1, 1));
        let stock_after_expiry = sqlx::query_scalar::<_, i64>(
            "SELECT stock_quantity FROM branch_catalog_items
             WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid",
        )
        .bind(&tenant_a)
        .bind(&branch_a)
        .bind(&product_id)
        .fetch_one(&sale_seed_pool)
        .await
        .unwrap();
        assert_eq!(
            stock_after_expiry, stock_before_expiry,
            "expiry releases commitments without changing physical stock"
        );
        let station_receipt_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/sales/{committed_sale_id}/receipt"
        );
        let historical_station_receipt = client
            .get(&station_receipt_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            historical_station_receipt.status(),
            axum::http::StatusCode::OK
        );
        let historical_station_receipt_body: serde_json::Value =
            historical_station_receipt.json().await.unwrap();
        assert_eq!(
            historical_station_receipt_body["data"]["customerName"],
            "Ama Mensah Updated"
        );
        assert_eq!(
            historical_station_receipt_body["data"]["lines"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            historical_station_receipt_body["data"]["tenders"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(historical_station_receipt_body["data"]["totalMinor"], 1000);
        let customer_merge_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/customers/{manager_customer_id}/merge"
        );
        let customer_merge_payload = serde_json::json!({
            "targetCustomerId": customer_id,
            "expectedSourceRevision": 1,
            "expectedTargetRevision": 4,
            "idempotencyKey": "customer-merge-001"
        });
        let customer_merge = client
            .post(&customer_merge_url)
            .bearer_auth("opaque-token")
            .json(&customer_merge_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(customer_merge.status(), axum::http::StatusCode::OK);
        let customer_merge_body: serde_json::Value = customer_merge.json().await.unwrap();
        assert_eq!(customer_merge_body["data"]["source"]["status"], "merged");
        assert_eq!(
            customer_merge_body["data"]["source"]["mergedIntoCustomerId"],
            customer_id
        );
        assert_eq!(customer_merge_body["data"]["target"]["status"], "active");
        assert_eq!(customer_merge_body["data"]["replayed"], false);
        let customer_merge_retry = client
            .post(&customer_merge_url)
            .bearer_auth("opaque-token")
            .json(&customer_merge_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(customer_merge_retry.status(), axum::http::StatusCode::OK);
        let customer_merge_retry_body: serde_json::Value =
            customer_merge_retry.json().await.unwrap();
        assert_eq!(customer_merge_retry_body["data"]["replayed"], true);
        let changed_customer_merge = client
            .post(&customer_merge_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "targetCustomerId": customer_id,
                "expectedSourceRevision": 1,
                "expectedTargetRevision": 5,
                "idempotencyKey": "customer-merge-001"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            changed_customer_merge.status(),
            axum::http::StatusCode::CONFLICT
        );
        let cashier_customer_merge = client.post(format!(
            "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/customers/{manager_customer_id}/merge"
        )).bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "targetCustomerId": customer_id,
                "expectedSourceRevision": 1,
                "expectedTargetRevision": 4,
                "idempotencyKey": "cashier-merge-denied"
            })).send().await.unwrap();
        assert_eq!(
            cashier_customer_merge.status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let merged_customer_list = client
            .get(format!("{customer_url}?query=kojo&includeArchived=true"))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(merged_customer_list.status(), axum::http::StatusCode::OK);
        let merged_customer_list_body: serde_json::Value =
            merged_customer_list.json().await.unwrap();
        assert!(merged_customer_list_body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == manager_customer_id
                && row["status"] == "merged"
                && row["mergedIntoCustomerId"] == customer_id));
        let archive_merge_target = client
            .patch(format!(
                "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/customers/{customer_id}/status"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({ "status": "archived", "expectedRevision": 4 }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            archive_merge_target.status(),
            axum::http::StatusCode::CONFLICT
        );
        let receipt_after_customer_merge = client
            .get(&station_receipt_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            receipt_after_customer_merge.status(),
            axum::http::StatusCode::OK
        );
        let receipt_after_customer_merge_body: serde_json::Value =
            receipt_after_customer_merge.json().await.unwrap();
        assert_eq!(
            receipt_after_customer_merge_body["data"]["customerName"],
            "Ama Mensah Updated"
        );
        let station_sales_history_response = client
            .get(format!(
                "{sale_url}?search={}",
                sale_body["data"]["receiptNumber"].as_str().unwrap()
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            station_sales_history_response.status(),
            axum::http::StatusCode::OK
        );
        let station_sales_history_body: serde_json::Value =
            station_sales_history_response.json().await.unwrap();
        assert_eq!(station_sales_history_body["data"]["matchedCount"], 1);
        assert_eq!(
            station_sales_history_body["data"]["rows"][0]["id"],
            committed_sale_id
        );
        assert_eq!(
            station_sales_history_body["data"]["rows"][0]["customerName"],
            "Ama Mensah Updated"
        );
        let invalid_station_sales_history = client
            .get(format!("{sale_url}?search={}", "a".repeat(121)))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            invalid_station_sales_history.status(),
            axum::http::StatusCode::BAD_REQUEST
        );
        sqlx::query("UPDATE hosted_customers SET name='Ama Mensah Later' WHERE tenant_id=$1::uuid AND id=$2::uuid")
            .bind(&tenant_a)
            .bind(customer_id)
            .execute(&sale_seed_pool)
            .await
            .unwrap();
        let receipt_after_customer_rename = client
            .get(&station_receipt_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        let receipt_after_customer_rename_body: serde_json::Value =
            receipt_after_customer_rename.json().await.unwrap();
        assert_eq!(
            receipt_after_customer_rename_body["data"]["customerName"],
            "Ama Mensah Updated"
        );
        sqlx::query("UPDATE hosted_customers SET name='Ama Mensah Updated' WHERE tenant_id=$1::uuid AND id=$2::uuid")
            .bind(&tenant_a)
            .bind(customer_id)
            .execute(&sale_seed_pool)
            .await
            .unwrap();
        let mut post_sale_tx = sale_seed_pool.begin().await.unwrap();
        let (post_sale_id, post_sale_subtotal, post_sale_tax, post_sale_total, post_sale_currency, post_sale_actor, post_sale_created) = sqlx::query_as::<_, (String, i64, i64, i64, String, String, String)>(
            "INSERT INTO hosted_sales(id,tenant_id,branch_id,station_id,cashier_membership_id,lease_id,activation_id,receipt_number,cashier_name_snapshot,station_name_snapshot,currency_code,subtotal_minor,tax_minor,total_minor,request_hash,idempotency_key,status,created_at,cash_shift_id,customer_id) SELECT gen_random_uuid(),tenant_id,branch_id,station_id,cashier_membership_id,lease_id,activation_id,'POS-POST-' || substr(gen_random_uuid()::text,1,8),cashier_name_snapshot,station_name_snapshot,currency_code,subtotal_minor,tax_minor,total_minor,request_hash,'post-sale-association-test','completed',now(),NULL,NULL FROM hosted_sales WHERE id=$1::uuid RETURNING id::text,subtotal_minor,tax_minor,total_minor,currency_code::text,cashier_membership_id::text,created_at::text",
        )
        .bind(committed_sale_id)
        .fetch_one(&mut *post_sale_tx)
        .await
        .unwrap();
        let post_sale_journal_id = Uuid::new_v4();
        sqlx::query("INSERT INTO hosted_accounting_journals(id,tenant_id,branch_id,source_type,source_id,currency_code,effective_at,actor_membership_id,description,coverage_status) VALUES($1,$2::uuid,$3::uuid,'sale',$4::uuid,$5,$6::timestamptz,$7::uuid,'Post-sale customer association fixture','partial')")
            .bind(post_sale_journal_id).bind(&tenant_a).bind(&branch_a).bind(&post_sale_id).bind(&post_sale_currency).bind(&post_sale_created).bind(&post_sale_actor)
            .execute(&mut *post_sale_tx).await.unwrap();
        let mut post_sale_line_number = 1_i16;
        for (account, description, debit, credit) in [
            ("1000", "Fixture tender clearing", post_sale_total, 0_i64),
            ("4000", "Fixture sales revenue", 0_i64, post_sale_subtotal),
            ("2100", "Fixture tax collected", 0_i64, post_sale_tax),
        ] {
            if debit == 0 && credit == 0 {
                continue;
            }
            sqlx::query("INSERT INTO hosted_accounting_journal_lines(tenant_id,branch_id,journal_id,line_number,account_code,description,debit_minor,credit_minor) VALUES($1::uuid,$2::uuid,$3,$4,$5,$6,$7,$8)")
                .bind(&tenant_a).bind(&branch_a).bind(post_sale_journal_id).bind(post_sale_line_number).bind(account).bind(description).bind(debit).bind(credit)
                .execute(&mut *post_sale_tx).await.unwrap();
            post_sale_line_number += 1;
        }
        post_sale_tx.commit().await.unwrap();
        let post_sale_customer_url = format!("{sale_url}/{post_sale_id}/customer");
        let linked_after_checkout = client
            .put(&post_sale_customer_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({"customerId": customer_id}))
            .send()
            .await
            .unwrap();
        assert_eq!(linked_after_checkout.status(), axum::http::StatusCode::OK);
        let linked_after_checkout_body: serde_json::Value =
            linked_after_checkout.json().await.unwrap();
        assert_eq!(linked_after_checkout_body["data"]["saleId"], post_sale_id);
        assert_eq!(
            linked_after_checkout_body["data"]["customerId"],
            customer_id
        );
        let attached_sale_receipt = client
            .get(format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/sales/{post_sale_id}/receipt"))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(attached_sale_receipt.status(), axum::http::StatusCode::OK);
        let attached_sale_receipt_body: serde_json::Value =
            attached_sale_receipt.json().await.unwrap();
        assert_eq!(
            attached_sale_receipt_body["data"]["customerName"],
            "Ama Mensah Updated"
        );
        let attached_branch_receipt = client
            .get(format!(
                "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/sales/{post_sale_id}"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(attached_branch_receipt.status(), axum::http::StatusCode::OK);
        let attached_branch_receipt_body: serde_json::Value =
            attached_branch_receipt.json().await.unwrap();
        assert_eq!(
            attached_branch_receipt_body["data"]["customerName"],
            "Ama Mensah Updated"
        );
        let exact_link_retry = client
            .put(&post_sale_customer_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({"customerId": customer_id}))
            .send()
            .await
            .unwrap();
        assert_eq!(exact_link_retry.status(), axum::http::StatusCode::OK);
        let different_link_conflict = client
            .put(&post_sale_customer_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({"customerId": manager_customer_id}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            different_link_conflict.status(),
            axum::http::StatusCode::CONFLICT
        );
        let link_effects: (i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM hosted_sale_customer_links WHERE sale_id=$1::uuid AND customer_id=$2::uuid),(SELECT count(*) FROM audit_events WHERE action='sale_customer_linked' AND entity_id=$1::text),(SELECT count(*) FROM hosted_sales WHERE id=$1::uuid AND total_minor=1000 AND customer_id IS NULL)",
        )
        .bind(&post_sale_id)
        .bind(customer_id)
        .fetch_one(&sale_seed_pool)
        .await
        .unwrap();
        assert_eq!(link_effects, (1, 1, 1));
        let sale_effects = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            "SELECT
                (SELECT count(*) FROM hosted_sales WHERE id=$1::uuid AND total_minor=1000),
                (SELECT count(*) FROM hosted_sale_lines WHERE sale_id=$1::uuid AND quantity=1),
                (SELECT count(*) FROM hosted_sale_tenders WHERE sale_id=$1::uuid AND amount_minor=1000),
                (SELECT count(*) FROM stock_movements WHERE source_id=$1::uuid AND movement_kind='sale' AND quantity_delta=-1 AND resulting_stock_quantity=4),
                (SELECT count(*) FROM audit_events WHERE entity_type='sale' AND entity_id=$1::text)"
        ).bind(committed_sale_id).fetch_one(&sale_seed_pool).await.unwrap();
        assert_eq!(sale_effects, (1, 1, 1, 1, 1));
        let resulting_stock: i64 = sqlx::query_scalar("SELECT stock_quantity FROM branch_catalog_items WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid")
            .bind(&tenant_a).bind(&branch_a).bind(&product_id).fetch_one(&sale_seed_pool).await.unwrap();
        assert_eq!(resulting_stock, 4);
        let retry_challenge = client
            .post(&checkout_challenge_url)
            .bearer_auth("opaque-token")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "checkout-proof-01")
            .json(&serde_json::json!({"payloadHash": payload_hash}))
            .send()
            .await
            .unwrap();
        assert_eq!(retry_challenge.status(), axum::http::StatusCode::OK);
        let retry_challenge_body: serde_json::Value = retry_challenge.json().await.unwrap();
        let retry_challenge_id = retry_challenge_body["data"]["challengeId"]
            .as_str()
            .unwrap();
        let retry_message = retry_challenge_body["data"]["messageToSign"]
            .as_str()
            .unwrap();
        let retry_signature = SigningKey::from_bytes(&[active_station_seed; 32])
            .sign(retry_message.as_bytes())
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let sale_replay = client
            .post(&sale_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "checkout-proof-01")
            .header("x-station-challenge-id", retry_challenge_id)
            .header("x-station-proof", retry_signature)
            .body(checkout_payload_bytes.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(sale_replay.status(), axum::http::StatusCode::OK);
        let sale_replay_body: serde_json::Value = sale_replay.json().await.unwrap();
        assert_eq!(sale_replay_body["data"]["id"], committed_sale_id);
        assert_eq!(sale_replay_body["data"]["replayed"], true);
        let replay_stock: i64 = sqlx::query_scalar("SELECT stock_quantity FROM branch_catalog_items WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid")
            .bind(&tenant_a).bind(&branch_a).bind(&product_id).fetch_one(&sale_seed_pool).await.unwrap();
        assert_eq!(
            replay_stock, 4,
            "exact sale retry must not decrement inventory twice"
        );

        let cash_option_response = client
            .post(&payment_options_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "methodKey": "cash",
                "label": "Cash",
                "tenderKind": "cash",
                "providerName": null,
                "countryCode": "GH",
                "requiresReference": false
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            cash_option_response.status(),
            axum::http::StatusCode::CREATED
        );
        let cash_option_body: serde_json::Value = cash_option_response.json().await.unwrap();
        let cash_option_id = cash_option_body["data"]["id"].as_str().unwrap();
        let cash_sale_body = serde_json::json!({
            "items": [{"productId": product_id, "quantity": 1}],
            "tenders": [{"paymentOptionId": cash_option_id, "amountMinor": 1000, "reference": null}]
        });
        let (no_shift_challenge, no_shift_proof, no_shift_bytes) = signed_financial_proof(
            &client,
            &checkout_challenge_url,
            &tenant_a,
            &branch_a,
            &active_station_id,
            lease_id,
            lease_token,
            "cash-sale-no-drawer",
            "sale.commit",
            None,
            &cash_sale_body,
            active_station_seed,
        )
        .await;
        let no_shift_sale = client
            .post(&sale_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "cash-sale-no-drawer")
            .header("x-station-challenge-id", &no_shift_challenge)
            .header("x-station-proof", &no_shift_proof)
            .body(no_shift_bytes)
            .send()
            .await
            .unwrap();
        assert_eq!(no_shift_sale.status(), axum::http::StatusCode::CONFLICT);
        let no_shift_body: serde_json::Value = no_shift_sale.json().await.unwrap();
        assert_eq!(no_shift_body["error"]["code"], "drawer_required");
        let stock_after_rejection: i64 = sqlx::query_scalar("SELECT stock_quantity FROM branch_catalog_items WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid")
            .bind(&tenant_a).bind(&branch_a).bind(&product_id).fetch_one(&sale_seed_pool).await.unwrap();
        assert_eq!(stock_after_rejection, 4);

        let cash_locations_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/cash-locations");
        let register_location = client
            .post(&cash_locations_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "register-location-01")
            .json(&serde_json::json!({
                "name": "A Station Register",
                "locationType": "register",
                "stationId": active_station_id
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(register_location.status(), axum::http::StatusCode::OK);
        let register_location_body: serde_json::Value = register_location.json().await.unwrap();
        let register_location_id = register_location_body["data"]["location"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let register_opening = client
            .post(format!(
                "{cash_locations_url}/{register_location_id}/opening"
            ))
            .bearer_auth("opaque-token")
            .header("idempotency-key", "register-cutover-01")
            .json(&serde_json::json!({"amountMinor": 10000}))
            .send()
            .await
            .unwrap();
        assert_eq!(register_opening.status(), axum::http::StatusCode::OK);

        let cash_reconciliation_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/accounting/cash-reconciliation"
        );
        let before_cutover = client
            .get(&cash_reconciliation_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(before_cutover.status(), axum::http::StatusCode::OK);
        let before_cutover_body: serde_json::Value = before_cutover.json().await.unwrap();
        assert_eq!(before_cutover_body["data"]["status"], "cutover_required");
        let tracked_cash_before_cutover: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(e.signed_amount_minor),0)::bigint FROM hosted_cash_locations l
             LEFT JOIN hosted_cash_location_events e ON e.tenant_id=l.tenant_id AND e.branch_id=l.branch_id AND e.location_id=l.id
             WHERE l.tenant_id=$1::uuid AND l.branch_id=$2::uuid AND l.status='active'",
        ).bind(&tenant_a).bind(&branch_a).fetch_one(&sale_seed_pool).await.unwrap();
        let cutover_request = serde_json::json!({"reason":"Initial branch cash count"});
        let cutover = client
            .post(&cash_reconciliation_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "branch-cash-cutover-01")
            .json(&cutover_request)
            .send()
            .await
            .unwrap();
        assert_eq!(cutover.status(), axum::http::StatusCode::CREATED);
        let cutover_body: serde_json::Value = cutover.json().await.unwrap();
        assert_eq!(
            cutover_body["data"]["reconciliation"]["trackedCashMinor"],
            tracked_cash_before_cutover
        );
        assert_eq!(
            cutover_body["data"]["reconciliation"]["adjustmentMinor"],
            tracked_cash_before_cutover
                - cutover_body["data"]["reconciliation"]["ledgerCashBeforeMinor"]
                    .as_i64()
                    .unwrap()
        );
        let cutover_retry = client
            .post(&cash_reconciliation_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "branch-cash-cutover-01")
            .json(&cutover_request)
            .send()
            .await
            .unwrap();
        assert_eq!(cutover_retry.status(), axum::http::StatusCode::CREATED);
        let cutover_retry_body: serde_json::Value = cutover_retry.json().await.unwrap();
        assert_eq!(cutover_retry_body["data"]["replayed"], true);
        let changed_cutover = client
            .post(&cash_reconciliation_url)
            .bearer_auth("opaque-token")
            .header("idempotency-key", "branch-cash-cutover-01")
            .json(&serde_json::json!({"reason":"Different count reason"}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            changed_cutover.status(),
            axum::http::StatusCode::CONFLICT,
            "a retry with a different request body must not reuse the key"
        );
        sqlx::query("INSERT INTO membership_role_grants(tenant_id,membership_id,role_key,branch_id,station_id) VALUES($1::uuid,$2::uuid,'manager',$3::uuid,NULL) ON CONFLICT DO NOTHING")
            .bind(&tenant_a).bind(&membership_id_two).bind(&branch_a_two).execute(&sale_seed_pool).await.unwrap();
        let manager_cash_reconciliation = client.get(format!("http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a_two}/accounting/cash-reconciliation"))
            .bearer_auth("opaque-token").send().await.unwrap();
        assert_eq!(
            manager_cash_reconciliation.status(),
            axum::http::StatusCode::OK
        );
        let manager_cutover = client.post(format!("http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a_two}/accounting/cash-reconciliation"))
            .bearer_auth("opaque-token").header("idempotency-key", "manager-cash-cutover-denied")
            .json(&serde_json::json!({"reason":"Manager cutover attempt"})).send().await.unwrap();
        assert_eq!(manager_cutover.status(), axum::http::StatusCode::FORBIDDEN);
        sqlx::query("DELETE FROM membership_role_grants WHERE tenant_id=$1::uuid AND membership_id=$2::uuid AND role_key='manager' AND branch_id=$3::uuid AND station_id IS NULL")
            .bind(&tenant_a).bind(&membership_id_two).bind(&branch_a_two).execute(&sale_seed_pool).await.unwrap();
        let cashier_cutover = client.post(format!("http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/accounting/cash-reconciliation"))
            .bearer_auth("opaque-token").header("idempotency-key", "cashier-cash-cutover-denied")
            .json(&serde_json::json!({"reason":"Cashier cutover attempt"})).send().await.unwrap();
        assert_eq!(cashier_cutover.status(), axum::http::StatusCode::FORBIDDEN);
        let initial_reconciliation = client
            .get(&cash_reconciliation_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        let initial_reconciliation_body: serde_json::Value =
            initial_reconciliation.json().await.unwrap();
        assert_eq!(initial_reconciliation_body["data"]["status"], "reconciled");
        assert_eq!(initial_reconciliation_body["data"]["varianceMinor"], 0);

        let drawer_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/drawer-shifts"
        );
        let open_body = serde_json::json!({"openingCashMinor": 10000});
        let (open_challenge, open_proof, open_bytes) = signed_financial_proof(
            &client,
            &checkout_challenge_url,
            &tenant_a,
            &branch_a,
            &active_station_id,
            lease_id,
            lease_token,
            "drawer-open-01",
            "drawer.open",
            None,
            &open_body,
            active_station_seed,
        )
        .await;
        let opened = client
            .post(&drawer_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "drawer-open-01")
            .header("x-station-challenge-id", &open_challenge)
            .header("x-station-proof", &open_proof)
            .body(open_bytes)
            .send()
            .await
            .unwrap();
        let opened_status = opened.status();
        let opened_body: serde_json::Value = opened.json().await.unwrap();
        assert_eq!(
            opened_status,
            axum::http::StatusCode::OK,
            "drawer open failed: {opened_body}"
        );
        let shift_id = opened_body["data"]["shiftId"].as_str().unwrap().to_owned();
        let station_payment_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/supplier-payments"
        );
        let cash_payment_payload = serde_json::json!({
            "supplierId": supplier_id,
            "paymentMethod": "cash",
            "externalReference": "CASH-VOUCHER-12",
            "amountMinor": 300,
            "drawerShiftId": shift_id,
            "allocations": [{"billId": second_bill_body["data"]["id"], "amountMinor": 300}]
        });
        let (cash_payment_challenge, cash_payment_proof, cash_payment_bytes) =
            signed_financial_proof(
                &client,
                &checkout_challenge_url,
                &tenant_a,
                &branch_a,
                &active_station_id,
                lease_id,
                lease_token,
                "supplier-cash-payment-001",
                "supplier.payment",
                Some(&shift_id),
                &cash_payment_payload,
                active_station_seed,
            )
            .await;
        let cash_payment = client
            .post(&station_payment_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "supplier-cash-payment-001")
            .header("x-station-challenge-id", &cash_payment_challenge)
            .header("x-station-proof", &cash_payment_proof)
            .body(cash_payment_bytes)
            .send()
            .await
            .unwrap();
        let cash_payment_status = cash_payment.status();
        let cash_payment_body: serde_json::Value = cash_payment.json().await.unwrap();
        assert_eq!(
            cash_payment_status,
            axum::http::StatusCode::OK,
            "cash supplier payment failed: {cash_payment_body}"
        );
        assert_eq!(cash_payment_body["data"]["paymentMethod"], "cash");
        assert_eq!(cash_payment_body["data"]["replayed"], false);
        let cash_payment_id = cash_payment_body["data"]["id"].as_str().unwrap();
        let (cash_retry_challenge, cash_retry_proof, cash_retry_bytes) = signed_financial_proof(
            &client,
            &checkout_challenge_url,
            &tenant_a,
            &branch_a,
            &active_station_id,
            lease_id,
            lease_token,
            "supplier-cash-payment-001",
            "supplier.payment",
            Some(&shift_id),
            &cash_payment_payload,
            active_station_seed,
        )
        .await;
        let cash_payment_retry = client
            .post(&station_payment_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "supplier-cash-payment-001")
            .header("x-station-challenge-id", &cash_retry_challenge)
            .header("x-station-proof", &cash_retry_proof)
            .body(cash_retry_bytes)
            .send()
            .await
            .unwrap();
        assert_eq!(cash_payment_retry.status(), axum::http::StatusCode::OK);
        let cash_payment_retry_body: serde_json::Value = cash_payment_retry.json().await.unwrap();
        assert_eq!(cash_payment_retry_body["data"]["id"], cash_payment_id);
        assert_eq!(cash_payment_retry_body["data"]["replayed"], true);
        let linked_cash_outflows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM hosted_drawer_movements WHERE tenant_id=$1::uuid AND source_payment_id=$2::uuid AND shift_id=$3::uuid AND movement_type='paid_out' AND amount_minor=300")
            .bind(&tenant_a).bind(cash_payment_id).bind(&shift_id).fetch_one(&sale_seed_pool).await.unwrap();
        assert_eq!(linked_cash_outflows, 1);
        let supplier_cash_event: (String, i64, i64) = sqlx::query_as(
            "SELECT e.event_type,e.signed_amount_minor,COUNT(*) OVER()::bigint
             FROM hosted_cash_location_events e JOIN hosted_drawer_movements d
               ON d.tenant_id=e.tenant_id AND d.branch_id=e.branch_id AND d.id=e.source_drawer_movement_id
             WHERE e.tenant_id=$1::uuid AND e.source_supplier_payment_id=$2::uuid AND d.shift_id=$3::uuid",
        )
        .bind(&tenant_a)
        .bind(cash_payment_id)
        .bind(&shift_id)
        .fetch_one(&sale_seed_pool)
        .await
        .unwrap();
        assert_eq!(
            supplier_cash_event,
            ("supplier_payment_outflow".to_owned(), -300, 1)
        );
        let current_shift_response = client
            .get(format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/drawer-shift"))
            .bearer_auth("opaque-token").send().await.unwrap();
        assert_eq!(current_shift_response.status(), axum::http::StatusCode::OK);
        let current_shift_body: serde_json::Value = current_shift_response.json().await.unwrap();
        assert_eq!(current_shift_body["data"]["expectedCashMinor"], 9700);
        // Cash expense recording is a cashier capability; prove it with the
        // elevated owner grant removed while preserving the signed station lease.
        sqlx::query("DELETE FROM membership_role_grants WHERE tenant_id=$1::uuid AND membership_id=$2::uuid AND role_key='owner' AND branch_id IS NULL AND station_id IS NULL")
            .bind(&tenant_a).bind(&membership_id).execute(&sale_seed_pool).await.unwrap();
        let station_expense_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/expenses"
        );
        let cash_expense_payload = serde_json::json!({
            "categoryCode": "supplies", "description": "Register receipt paper", "payee": "Market stall",
            "amountMinor": 125, "paymentMethod": "cash", "externalReference": "PETTY-001",
            "occurredOn": "2026-11-01", "drawerShiftId": shift_id
        });
        let (expense_challenge, expense_proof, expense_bytes) = signed_financial_proof(
            &client,
            &checkout_challenge_url,
            &tenant_a,
            &branch_a,
            &active_station_id,
            lease_id,
            lease_token,
            "cash-expense-001",
            "expense.payment",
            Some(&shift_id),
            &cash_expense_payload,
            active_station_seed,
        )
        .await;
        let cash_expense = client
            .post(&station_expense_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "cash-expense-001")
            .header("x-station-challenge-id", &expense_challenge)
            .header("x-station-proof", &expense_proof)
            .body(expense_bytes)
            .send()
            .await
            .unwrap();
        let cash_expense_status = cash_expense.status();
        let cash_expense_body: serde_json::Value = cash_expense.json().await.unwrap();
        assert_eq!(
            cash_expense_status,
            axum::http::StatusCode::OK,
            "cash expense failed: {cash_expense_body}"
        );
        let cash_expense_id = cash_expense_body["data"]["id"].as_str().unwrap();
        assert_eq!(cash_expense_body["data"]["paymentMethod"], "cash");
        assert_eq!(cash_expense_body["data"]["drawerShiftId"], shift_id);
        assert_eq!(cash_expense_body["data"]["replayed"], false);
        let expense_cash_event: (String, i64, i64) = sqlx::query_as(
            "SELECT e.event_type,e.signed_amount_minor,COUNT(*) OVER()::bigint
             FROM hosted_cash_location_events e JOIN hosted_drawer_movements d
               ON d.tenant_id=e.tenant_id AND d.branch_id=e.branch_id AND d.id=e.source_drawer_movement_id
             WHERE e.tenant_id=$1::uuid AND e.source_expense_id=$2::uuid AND d.shift_id=$3::uuid",
        )
        .bind(&tenant_a)
        .bind(cash_expense_id)
        .bind(&shift_id)
        .fetch_one(&sale_seed_pool)
        .await
        .unwrap();
        assert_eq!(expense_cash_event, ("expense_outflow".to_owned(), -125, 1));
        let (expense_retry_challenge, expense_retry_proof, expense_retry_bytes) =
            signed_financial_proof(
                &client,
                &checkout_challenge_url,
                &tenant_a,
                &branch_a,
                &active_station_id,
                lease_id,
                lease_token,
                "cash-expense-001",
                "expense.payment",
                Some(&shift_id),
                &cash_expense_payload,
                active_station_seed,
            )
            .await;
        let cash_expense_retry = client
            .post(&station_expense_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "cash-expense-001")
            .header("x-station-challenge-id", &expense_retry_challenge)
            .header("x-station-proof", &expense_retry_proof)
            .body(expense_retry_bytes)
            .send()
            .await
            .unwrap();
        assert_eq!(cash_expense_retry.status(), axum::http::StatusCode::OK);
        let expense_retry_body: serde_json::Value = cash_expense_retry.json().await.unwrap();
        assert_eq!(expense_retry_body["data"]["id"], cash_expense_id);
        assert_eq!(expense_retry_body["data"]["replayed"], true);
        let changed_expense_payload = serde_json::json!({
            "categoryCode": "supplies", "description": "Register receipt paper", "payee": "Market stall",
            "amountMinor": 126, "paymentMethod": "cash", "externalReference": "PETTY-001",
            "occurredOn": "2026-11-01", "drawerShiftId": shift_id
        });
        let (changed_expense_challenge, changed_expense_proof, changed_expense_bytes) =
            signed_financial_proof(
                &client,
                &checkout_challenge_url,
                &tenant_a,
                &branch_a,
                &active_station_id,
                lease_id,
                lease_token,
                "cash-expense-001",
                "expense.payment",
                Some(&shift_id),
                &changed_expense_payload,
                active_station_seed,
            )
            .await;
        let changed_expense = client
            .post(&station_expense_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "cash-expense-001")
            .header("x-station-challenge-id", &changed_expense_challenge)
            .header("x-station-proof", &changed_expense_proof)
            .body(changed_expense_bytes)
            .send()
            .await
            .unwrap();
        assert_eq!(changed_expense.status(), axum::http::StatusCode::CONFLICT);
        let linked_expense_outflows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM hosted_drawer_movements WHERE tenant_id=$1::uuid AND source_expense_id=$2::uuid AND shift_id=$3::uuid AND movement_type='paid_out' AND amount_minor=125")
            .bind(&tenant_a).bind(cash_expense_id).bind(&shift_id).fetch_one(&sale_seed_pool).await.unwrap();
        assert_eq!(linked_expense_outflows, 1);
        let cash_expense_journal = sqlx::query_as::<_, (i64, i64, i64, i64)>("SELECT SUM(l.debit_minor)::bigint,SUM(l.credit_minor)::bigint,COUNT(*),COUNT(*) FILTER (WHERE l.account_code='1000' AND l.credit_minor=125) FROM hosted_accounting_journals j JOIN hosted_accounting_journal_lines l ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND j.source_type='expense' AND j.source_id=$3::uuid GROUP BY j.id")
            .bind(&tenant_a).bind(&branch_a).bind(cash_expense_id).fetch_one(&sale_seed_pool).await.unwrap();
        assert_eq!(cash_expense_journal, (125, 125, 2, 1));
        let shift_after_cash_expense = client.get(format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/drawer-shift"))
            .bearer_auth("opaque-token").send().await.unwrap();
        let shift_after_cash_expense_body: serde_json::Value =
            shift_after_cash_expense.json().await.unwrap();
        assert_eq!(
            shift_after_cash_expense_body["data"]["expectedCashMinor"],
            9575
        );
        let x_report_response = client
            .get(format!("{drawer_url}/{shift_id}/x-report"))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(x_report_response.status(), axum::http::StatusCode::OK);
        let x_report_body: serde_json::Value = x_report_response.json().await.unwrap();
        assert_eq!(x_report_body["data"]["isClosed"], false);

        let (cash_sale_challenge, cash_sale_proof, cash_sale_bytes) = signed_financial_proof(
            &client,
            &checkout_challenge_url,
            &tenant_a,
            &branch_a,
            &active_station_id,
            lease_id,
            lease_token,
            "cash-sale-01",
            "sale.commit",
            None,
            &cash_sale_body,
            active_station_seed,
        )
        .await;
        let cash_sale = client
            .post(&sale_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "cash-sale-01")
            .header("x-station-challenge-id", &cash_sale_challenge)
            .header("x-station-proof", &cash_sale_proof)
            .body(cash_sale_bytes)
            .send()
            .await
            .unwrap();
        let cash_sale_status = cash_sale.status();
        let cash_sale_body: serde_json::Value = cash_sale.json().await.unwrap();
        assert_eq!(
            cash_sale_status,
            axum::http::StatusCode::OK,
            "cash sale failed: {cash_sale_body}"
        );
        let cash_sale_id = cash_sale_body["data"]["id"].as_str().unwrap();
        let sale_cash_event: (i64, Option<i64>) = sqlx::query_as(
            "SELECT count(e.id),max(e.signed_amount_minor) FROM hosted_sale_tenders t
             LEFT JOIN hosted_cash_location_events e ON e.tenant_id=t.tenant_id AND e.branch_id=t.branch_id AND e.source_sale_tender_id=t.id
             WHERE t.tenant_id=$1::uuid AND t.sale_id=$2::uuid AND t.tender_kind='cash'",
        )
        .bind(&tenant_a)
        .bind(cash_sale_id)
        .fetch_one(&sale_seed_pool)
        .await
        .unwrap();
        assert_eq!(sale_cash_event, (1, Some(1000)));
        let sale_journal: (i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT count(DISTINCT j.id)::bigint,sum(l.debit_minor)::bigint,sum(l.credit_minor)::bigint,
                    COALESCE(sum(l.debit_minor) FILTER (WHERE l.account_code='1000'),0)::bigint,
                    COALESCE(sum(l.credit_minor) FILTER (WHERE l.account_code='4000'),0)::bigint,
                    COALESCE(sum(l.credit_minor) FILTER (WHERE l.account_code='2100'),0)::bigint,
                    COALESCE(sum(l.debit_minor) FILTER (WHERE l.account_code='5000'),0)::bigint,
                    COALESCE(sum(l.credit_minor) FILTER (WHERE l.account_code='1300'),0)::bigint
             FROM hosted_accounting_journals j JOIN hosted_accounting_journal_lines l
               ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id
             WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND j.source_type='sale' AND j.source_id=$3::uuid",
        )
        .bind(&tenant_a)
        .bind(&branch_a)
        .bind(cash_sale_id)
        .fetch_one(&sale_seed_pool)
        .await
        .unwrap();
        assert_eq!(sale_journal.0, 1);
        assert_eq!(sale_journal.1, sale_journal.2);
        assert_eq!(
            sale_journal.3,
            cash_sale_body["data"]["totalMinor"].as_i64().unwrap()
        );
        assert_eq!(
            sale_journal.4,
            cash_sale_body["data"]["subtotalMinor"].as_i64().unwrap()
        );
        assert_eq!(
            sale_journal.5,
            cash_sale_body["data"]["taxMinor"].as_i64().unwrap()
        );
        assert_eq!(sale_journal.6, sale_journal.7);
        let cash_shift_link: Option<String> =
            sqlx::query_scalar("SELECT cash_shift_id::text FROM hosted_sales WHERE id=$1::uuid")
                .bind(cash_sale_id)
                .fetch_one(&sale_seed_pool)
                .await
                .unwrap();
        assert_eq!(cash_shift_link.as_deref(), Some(shift_id.as_str()));
        let cash_drop_destinations = client.get(format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/drawer/cash-drop-destinations"
        )).bearer_auth("opaque-token").send().await.unwrap();
        assert_eq!(cash_drop_destinations.status(), axum::http::StatusCode::OK);
        let cash_drop_destinations_body: serde_json::Value =
            cash_drop_destinations.json().await.unwrap();
        assert!(cash_drop_destinations_body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == safe_id));
        let _paid_in = post_test_drawer_movement(
            &client,
            &base_url,
            &tenant_a,
            &branch_a,
            &active_station_id,
            &shift_id,
            lease_id,
            lease_token,
            "ledger-paid-in-01",
            "paid_in",
            100,
            None,
            active_station_seed,
        )
        .await;
        let _cash_drop = post_test_drawer_movement(
            &client,
            &base_url,
            &tenant_a,
            &branch_a,
            &active_station_id,
            &shift_id,
            lease_id,
            lease_token,
            "ledger-cash-drop-01",
            "cash_drop",
            100,
            Some(&safe_id),
            active_station_seed,
        )
        .await;
        let drawer_movement_cash_events: Vec<(String, String, i64, String)> = sqlx::query_as(
            "SELECT d.movement_type,e.event_type,e.signed_amount_minor,e.location_id::text
             FROM hosted_drawer_movements d JOIN hosted_cash_location_events e
               ON e.tenant_id=d.tenant_id AND e.branch_id=d.branch_id AND e.source_drawer_movement_id=d.id
             WHERE d.tenant_id=$1::uuid AND d.idempotency_key IN ('ledger-paid-in-01','ledger-cash-drop-01')
             ORDER BY d.idempotency_key,e.event_type",
        )
        .bind(&tenant_a)
        .fetch_all(&sale_seed_pool)
        .await
        .unwrap();
        assert_eq!(
            drawer_movement_cash_events,
            vec![
                (
                    "cash_drop".to_owned(),
                    "cash_drop_inflow".to_owned(),
                    100,
                    safe_id.clone()
                ),
                (
                    "cash_drop".to_owned(),
                    "register_movement_outflow".to_owned(),
                    -100,
                    sqlx::query_scalar::<_, String>("SELECT id::text FROM hosted_cash_locations WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND location_type='register' AND station_id=$3::uuid")
                        .bind(&tenant_a).bind(&branch_a).bind(&active_station_id).fetch_one(&sale_seed_pool).await.unwrap()
                ),
                (
                    "paid_in".to_owned(),
                    "register_movement_inflow".to_owned(),
                    100,
                    sqlx::query_scalar::<_, String>("SELECT id::text FROM hosted_cash_locations WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND location_type='register' AND station_id=$3::uuid")
                        .bind(&tenant_a).bind(&branch_a).bind(&active_station_id).fetch_one(&sale_seed_pool).await.unwrap()
                ),
            ]
        );
        // Refunds are elevated financial actions; restore the owner grant after
        // proving the cashier-only sale and its event passed the restricted RLS policy.
        sqlx::query("INSERT INTO membership_role_grants(tenant_id,membership_id,role_key,branch_id,station_id) VALUES($1::uuid,$2::uuid,'owner',NULL,NULL)")
            .bind(&tenant_a).bind(&membership_id).execute(&sale_seed_pool).await.unwrap();
        let sale_line_id: String = sqlx::query_scalar(
            "SELECT id::text FROM hosted_sale_lines WHERE sale_id=$1::uuid LIMIT 1",
        )
        .bind(cash_sale_id)
        .fetch_one(&sale_seed_pool)
        .await
        .unwrap();
        let refund_url = format!("{sale_url}/{cash_sale_id}/refunds");
        let refund_payload = serde_json::json!({
            "kind": "void",
            "reason": "Customer returned the complete order",
            "items": [{"saleLineId": sale_line_id, "quantity": 1, "restock": true}],
            "paymentMethod": "cash",
            "paymentReference": null
        });
        let (refund_challenge, refund_proof, refund_bytes) = signed_financial_proof(
            &client,
            &checkout_challenge_url,
            &tenant_a,
            &branch_a,
            &active_station_id,
            lease_id,
            lease_token,
            "cash-refund-01",
            "sale.refund",
            Some(&cash_sale_id),
            &refund_payload,
            active_station_seed,
        )
        .await;
        let refund_response = client
            .post(&refund_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "cash-refund-01")
            .header("x-station-challenge-id", &refund_challenge)
            .header("x-station-proof", &refund_proof)
            .body(refund_bytes)
            .send()
            .await
            .unwrap();
        let refund_status = refund_response.status();
        let refund_body: serde_json::Value = refund_response.json().await.unwrap();
        assert_eq!(
            refund_status,
            axum::http::StatusCode::CREATED,
            "cash refund failed: {refund_body}"
        );
        assert_eq!(refund_body["data"]["kind"], "void");
        let refund_id = refund_body["data"]["id"].as_str().unwrap();
        let (refund_retry_challenge, refund_retry_proof, refund_retry_bytes) =
            signed_financial_proof(
                &client,
                &checkout_challenge_url,
                &tenant_a,
                &branch_a,
                &active_station_id,
                lease_id,
                lease_token,
                "cash-refund-01",
                "sale.refund",
                Some(&cash_sale_id),
                &refund_payload,
                active_station_seed,
            )
            .await;
        let refund_retry = client
            .post(&refund_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "cash-refund-01")
            .header("x-station-challenge-id", &refund_retry_challenge)
            .header("x-station-proof", &refund_retry_proof)
            .body(refund_retry_bytes)
            .send()
            .await
            .unwrap();
        assert_eq!(refund_retry.status(), axum::http::StatusCode::CREATED);
        let refund_retry_body: serde_json::Value = refund_retry.json().await.unwrap();
        assert_eq!(refund_retry_body["data"]["id"], refund_id);
        let linked_refund_outflows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM hosted_drawer_movements WHERE tenant_id=$1::uuid AND source_refund_id=$2::uuid AND shift_id=$3::uuid AND movement_type='paid_out' AND amount_minor=1000")
            .bind(&tenant_a).bind(refund_id).bind(&shift_id).fetch_one(&sale_seed_pool).await.unwrap();
        assert_eq!(linked_refund_outflows, 1);
        let refund_cash_event: (i64, Option<i64>, i64) = sqlx::query_as(
            "SELECT count(e.id),max(e.signed_amount_minor),COALESCE((SELECT sum(signed_amount_minor) FROM hosted_cash_location_events WHERE tenant_id=$1::uuid AND location_id=$3::uuid),0)::bigint
             FROM hosted_sale_refund_tenders t LEFT JOIN hosted_cash_location_events e
               ON e.tenant_id=t.tenant_id AND e.branch_id=t.branch_id AND e.source_refund_tender_id=t.id
             WHERE t.tenant_id=$1::uuid AND t.refund_id=$2::uuid AND t.method='cash'",
        )
        .bind(&tenant_a)
        .bind(refund_id)
        .bind(&register_location_id)
        .fetch_one(&sale_seed_pool)
        .await
        .unwrap();
        assert_eq!(refund_cash_event, (1, Some(-1000), 9575));
        let refund_journal: (i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT count(DISTINCT j.id)::bigint,sum(l.debit_minor)::bigint,sum(l.credit_minor)::bigint,
                    COALESCE(sum(l.debit_minor) FILTER (WHERE l.account_code='4020'),0)::bigint,
                    COALESCE(sum(l.debit_minor) FILTER (WHERE l.account_code='2100'),0)::bigint,
                    COALESCE(sum(l.credit_minor) FILTER (WHERE l.account_code='1000'),0)::bigint,
                    COALESCE(sum(l.debit_minor) FILTER (WHERE l.account_code='1300'),0)::bigint,
                    COALESCE(sum(l.credit_minor) FILTER (WHERE l.account_code='5000'),0)::bigint
             FROM hosted_accounting_journals j JOIN hosted_accounting_journal_lines l
               ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id
             WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND j.source_type='sale_refund' AND j.source_id=$3::uuid",
        )
        .bind(&tenant_a)
        .bind(&branch_a)
        .bind(refund_id)
        .fetch_one(&sale_seed_pool)
        .await
        .unwrap();
        assert_eq!(refund_journal.0, 1);
        assert_eq!(refund_journal.1, refund_journal.2);
        assert_eq!(refund_journal.3 + refund_journal.4, 1000);
        assert_eq!(refund_journal.5, 1000);
        assert_eq!(refund_journal.6, refund_journal.7);
        let restocked_quantity: i64 = sqlx::query_scalar("SELECT stock_quantity FROM branch_catalog_items WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid")
            .bind(&tenant_a).bind(&branch_a).bind(&product_id).fetch_one(&sale_seed_pool).await.unwrap();
        assert_eq!(restocked_quantity, 4);
        let refund_detail = client
            .get(&refund_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(refund_detail.status(), axum::http::StatusCode::OK);
        let refund_detail_body: serde_json::Value = refund_detail.json().await.unwrap();
        assert_eq!(refund_detail_body["data"]["status"], "voided");
        assert_eq!(
            refund_detail_body["data"]["items"][0]["remainingQuantity"],
            0
        );
        sqlx::query("UPDATE branch_catalog_items SET price_minor=101,tax_rate_bps=333,stock_quantity=4 WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid")
            .bind(&tenant_a).bind(&branch_a).bind(&product_id).execute(&sale_seed_pool).await.unwrap();
        let taxable_sale_body = serde_json::json!({
            "items": [{"productId": product_id, "quantity": 3}],
            "tenders": [{"paymentOptionId": payment_option_id, "amountMinor": 313, "reference": "TEST-MOMO-SALE-01"}]
        });
        let (taxable_sale_challenge, taxable_sale_proof, taxable_sale_bytes) =
            signed_financial_proof(
                &client,
                &checkout_challenge_url,
                &tenant_a,
                &branch_a,
                &active_station_id,
                lease_id,
                lease_token,
                "taxable-sale-01",
                "sale.commit",
                None,
                &taxable_sale_body,
                active_station_seed,
            )
            .await;
        let taxable_sale = client
            .post(&sale_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "taxable-sale-01")
            .header("x-station-challenge-id", &taxable_sale_challenge)
            .header("x-station-proof", &taxable_sale_proof)
            .body(taxable_sale_bytes)
            .send()
            .await
            .unwrap();
        let taxable_sale_status = taxable_sale.status();
        let taxable_sale_response_body: serde_json::Value = taxable_sale.json().await.unwrap();
        assert_eq!(
            taxable_sale_status,
            axum::http::StatusCode::OK,
            "taxable sale failed: {taxable_sale_response_body}"
        );
        assert_eq!(taxable_sale_response_body["data"]["totalMinor"], 313);
        let taxable_sale_id = taxable_sale_response_body["data"]["id"].as_str().unwrap();
        let taxable_sale_line_id: String = sqlx::query_scalar(
            "SELECT id::text FROM hosted_sale_lines WHERE sale_id=$1::uuid LIMIT 1",
        )
        .bind(taxable_sale_id)
        .fetch_one(&sale_seed_pool)
        .await
        .unwrap();
        let taxable_refund_url = format!("{sale_url}/{taxable_sale_id}/refunds");
        let missing_reference_body = serde_json::json!({
            "kind": "refund", "reason": "Return one of three taxable units",
            "items": [{"saleLineId": taxable_sale_line_id, "quantity": 1, "restock": false}],
            "paymentMethod": "mobile_money", "paymentReference": null
        });
        let (missing_reference_challenge, missing_reference_proof, missing_reference_bytes) =
            signed_financial_proof(
                &client,
                &checkout_challenge_url,
                &tenant_a,
                &branch_a,
                &active_station_id,
                lease_id,
                lease_token,
                "partial-refund-no-ref-01",
                "sale.refund",
                Some(&taxable_sale_id),
                &missing_reference_body,
                active_station_seed,
            )
            .await;
        let missing_reference_response = client
            .post(&taxable_refund_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "partial-refund-no-ref-01")
            .header("x-station-challenge-id", &missing_reference_challenge)
            .header("x-station-proof", &missing_reference_proof)
            .body(missing_reference_bytes)
            .send()
            .await
            .unwrap();
        assert_eq!(
            missing_reference_response.status(),
            axum::http::StatusCode::BAD_REQUEST
        );
        let missing_reference_response_body: serde_json::Value =
            missing_reference_response.json().await.unwrap();
        assert_eq!(
            missing_reference_response_body["error"]["code"],
            "invalid_refund_request"
        );

        let first_partial_body = serde_json::json!({
            "kind": "refund", "reason": "Return one of three taxable units",
            "items": [{"saleLineId": taxable_sale_line_id, "quantity": 1, "restock": false}],
            "paymentMethod": "mobile_money", "paymentReference": "MM-REFUND-ONE"
        });
        let (first_partial_challenge, first_partial_proof, first_partial_bytes) =
            signed_financial_proof(
                &client,
                &checkout_challenge_url,
                &tenant_a,
                &branch_a,
                &active_station_id,
                lease_id,
                lease_token,
                "partial-refund-01",
                "sale.refund",
                Some(&taxable_sale_id),
                &first_partial_body,
                active_station_seed,
            )
            .await;
        let first_partial_refund = client
            .post(&taxable_refund_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "partial-refund-01")
            .header("x-station-challenge-id", &first_partial_challenge)
            .header("x-station-proof", &first_partial_proof)
            .body(first_partial_bytes)
            .send()
            .await
            .unwrap();
        let first_partial_status = first_partial_refund.status();
        let first_partial_response_body: serde_json::Value =
            first_partial_refund.json().await.unwrap();
        assert_eq!(
            first_partial_status,
            axum::http::StatusCode::CREATED,
            "partial refund failed: {first_partial_response_body}"
        );
        assert_eq!(first_partial_response_body["data"]["totalMinor"], 104);
        assert_eq!(
            first_partial_response_body["data"]["paymentReference"],
            "MM-REFUND-ONE"
        );

        let second_partial_body = serde_json::json!({
            "kind": "refund", "reason": "Return another taxable unit",
            "items": [{"saleLineId": taxable_sale_line_id, "quantity": 1, "restock": true}],
            "paymentMethod": "mobile_money", "paymentReference": "MM-REFUND-TWO"
        });
        let (second_partial_challenge, second_partial_proof, second_partial_bytes) =
            signed_financial_proof(
                &client,
                &checkout_challenge_url,
                &tenant_a,
                &branch_a,
                &active_station_id,
                lease_id,
                lease_token,
                "partial-refund-02",
                "sale.refund",
                Some(&taxable_sale_id),
                &second_partial_body,
                active_station_seed,
            )
            .await;
        let second_partial_refund = client
            .post(&taxable_refund_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "partial-refund-02")
            .header("x-station-challenge-id", &second_partial_challenge)
            .header("x-station-proof", &second_partial_proof)
            .body(second_partial_bytes)
            .send()
            .await
            .unwrap();
        let second_partial_status = second_partial_refund.status();
        let second_partial_response_body: serde_json::Value =
            second_partial_refund.json().await.unwrap();
        assert_eq!(
            second_partial_status,
            axum::http::StatusCode::CREATED,
            "second partial refund failed: {second_partial_response_body}"
        );
        assert_eq!(second_partial_response_body["data"]["totalMinor"], 104);
        assert_eq!(
            second_partial_response_body["data"]["paymentReference"],
            "MM-REFUND-TWO"
        );
        let partial_detail = client
            .get(&taxable_refund_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(partial_detail.status(), axum::http::StatusCode::OK);
        let partial_detail_body: serde_json::Value = partial_detail.json().await.unwrap();
        assert_eq!(partial_detail_body["data"]["status"], "partially_refunded");
        assert_eq!(
            partial_detail_body["data"]["items"][0]["refundedQuantity"],
            2
        );
        assert_eq!(
            partial_detail_body["data"]["items"][0]["remainingQuantity"],
            1
        );
        assert_eq!(
            partial_detail_body["data"]["items"][0]["refundableTotalMinor"],
            105
        );
        assert_eq!(
            partial_detail_body["data"]["refunds"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let stock_after_partial_returns: i64 = sqlx::query_scalar(
            "SELECT stock_quantity FROM branch_catalog_items WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid",
        ).bind(&tenant_a).bind(&branch_a).bind(&product_id).fetch_one(&sale_seed_pool).await.unwrap();
        assert_eq!(
            stock_after_partial_returns, 2,
            "only the explicitly restocked returned unit should increase stock"
        );
        let sales_report_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/sales");
        let sales_report = client
            .get(&sales_report_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(sales_report.status(), axum::http::StatusCode::OK);
        let sales_report_body: serde_json::Value = sales_report.json().await.unwrap();
        assert_eq!(sales_report_body["data"]["matchedCount"], 4);
        assert_eq!(
            sales_report_body["data"]["totalsByCurrency"][0]["currencyCode"],
            "GHS"
        );
        assert_eq!(
            sales_report_body["data"]["totalsByCurrency"][0]["totalMinor"],
            "3313"
        );
        assert_eq!(
            sales_report_body["data"]["rows"].as_array().unwrap().len(),
            4
        );
        let accounting_coverage_url = format!(
            "http://{api_address}/v1/tenants/{tenant_a}/branches/{branch_a}/accounting/coverage"
        );
        let accounting_coverage = client
            .get(&accounting_coverage_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(accounting_coverage.status(), axum::http::StatusCode::OK);
        let accounting_coverage_body: serde_json::Value = accounting_coverage.json().await.unwrap();
        assert_eq!(accounting_coverage_body["data"]["salesCount"], 4);
        assert_eq!(accounting_coverage_body["data"]["unpostedSalesCount"], 0);
        assert_eq!(accounting_coverage_body["data"]["unpostedRefundsCount"], 0);
        let trial_balance = client
            .get(format!(
                "http://{api_address}/v1/tenants/{tenant_a}/branches/{branch_a}/accounting/trial-balance"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(trial_balance.status(), axum::http::StatusCode::OK);
        let trial_balance_body: serde_json::Value = trial_balance.json().await.unwrap();
        assert_eq!(
            trial_balance_body["data"]["salesRefundCoverageComplete"],
            false
        );
        assert_eq!(
            trial_balance_body["data"]["currencies"][0]["currencyCode"],
            "GHS"
        );
        assert_eq!(
            trial_balance_body["data"]["currencies"][0]["isBalanced"],
            true
        );
        assert_eq!(
            trial_balance_body["data"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["accountCode"] == "4000")
                .unwrap()["creditMinor"],
            "3303"
        );
        let income_statement = client
            .get(format!(
                "http://{api_address}/v1/tenants/{tenant_a}/branches/{branch_a}/accounting/income-statement"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(income_statement.status(), axum::http::StatusCode::OK);
        let income_statement_body: serde_json::Value = income_statement.json().await.unwrap();
        let income = &income_statement_body["data"]["currencies"][0];
        assert_eq!(income["currencyCode"], "GHS");
        let gross_sales = income["grossSalesMinor"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .unwrap();
        let sales_returns = income["salesReturnsMinor"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .unwrap();
        let net_sales = income["netSalesMinor"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .unwrap();
        assert_eq!(net_sales, gross_sales - sales_returns);
        let cogs = income["costOfGoodsSoldMinor"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .unwrap();
        let gross_profit = income["grossProfitMinor"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .unwrap();
        assert_eq!(gross_profit, net_sales - cogs);
        let operating_expenses = income["operatingExpensesMinor"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .unwrap();
        let operating_income = income["operatingIncomeMinor"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .unwrap();
        assert_eq!(operating_income, gross_profit - operating_expenses);
        assert_eq!(
            income_statement_body["data"]["salesRefundCoverageComplete"],
            false
        );
        let balance_sheet = client
            .get(format!(
                "http://{api_address}/v1/tenants/{tenant_a}/branches/{branch_a}/accounting/balance-sheet?asOfDate=2099-12-31"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(balance_sheet.status(), axum::http::StatusCode::OK);
        let balance_sheet_body: serde_json::Value = balance_sheet.json().await.unwrap();
        assert_eq!(balance_sheet_body["data"]["asOfDate"], "2099-12-31");
        assert_eq!(balance_sheet_body["data"]["complete"], false);
        assert_eq!(
            balance_sheet_body["data"]["currencies"][0]["currencyCode"],
            "GHS"
        );
        let balance = &balance_sheet_body["data"]["currencies"][0];
        let assets = balance["assetsMinor"]
            .as_str()
            .unwrap()
            .parse::<i128>()
            .unwrap();
        let liabilities_and_equity = balance["liabilitiesAndEquityMinor"]
            .as_str()
            .unwrap()
            .parse::<i128>()
            .unwrap();
        let difference = balance["differenceMinor"]
            .as_str()
            .unwrap()
            .parse::<i128>()
            .unwrap();
        assert_eq!(difference, assets - liabilities_and_equity);
        assert_eq!(balance["isBalanced"], difference == 0);
        let invalid_balance_sheet = client
            .get(format!(
                "http://{api_address}/v1/tenants/{tenant_a}/branches/{branch_a}/accounting/balance-sheet?asOfDate=2026-02-30"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            invalid_balance_sheet.status(),
            axum::http::StatusCode::BAD_REQUEST
        );
        let cash_flow = client
            .get(format!(
                "http://{api_address}/v1/tenants/{tenant_a}/branches/{branch_a}/accounting/cash-flow?fromDate=2026-01-01&toDate=2099-12-31"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(cash_flow.status(), axum::http::StatusCode::OK);
        let cash_flow_body: serde_json::Value = cash_flow.json().await.unwrap();
        assert_eq!(cash_flow_body["data"]["complete"], false);
        assert_eq!(cash_flow_body["data"]["fromDate"], "2026-01-01");
        assert_eq!(cash_flow_body["data"]["toDate"], "2099-12-31");
        let cash_flow_currency = &cash_flow_body["data"]["currencies"][0];
        assert_eq!(cash_flow_currency["currencyCode"], "GHS");
        let cash_opening = cash_flow_currency["openingCashMinor"]
            .as_str()
            .unwrap()
            .parse::<i128>()
            .unwrap();
        let cash_ending = cash_flow_currency["endingCashMinor"]
            .as_str()
            .unwrap()
            .parse::<i128>()
            .unwrap();
        let cash_net_change = cash_flow_currency["netChangeMinor"]
            .as_str()
            .unwrap()
            .parse::<i128>()
            .unwrap();
        assert_eq!(cash_net_change, cash_ending - cash_opening);
        let operating = cash_flow_currency["netOperatingCashFlowMinor"]
            .as_str()
            .unwrap()
            .parse::<i128>()
            .unwrap();
        let cutover = cash_flow_currency["cutoverAdjustmentMinor"]
            .as_str()
            .unwrap()
            .parse::<i128>()
            .unwrap();
        let unclassified = cash_flow_currency["unclassifiedJournalCashMinor"]
            .as_str()
            .unwrap()
            .parse::<i128>()
            .unwrap();
        let bridge_difference = cash_flow_currency["reconciliationDifferenceMinor"]
            .as_str()
            .unwrap()
            .parse::<i128>()
            .unwrap();
        assert_eq!(
            cash_net_change,
            operating + cutover + unclassified + bridge_difference
        );
        assert_eq!(
            cash_flow_currency["clearingAccounts"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert!(
            cash_flow_body["data"]["limitations"]
                .as_array()
                .unwrap()
                .len()
                >= 3
        );
        let invalid_sales_report = client
            .get(format!("{sales_report_url}?fromDate=2026-02-30"))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            invalid_sales_report.status(),
            axum::http::StatusCode::BAD_REQUEST
        );
        let cashier_sales_report = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/sales"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_sales_report.status(),
            axum::http::StatusCode::FORBIDDEN,
            "cashiers without sales:read cannot view branch financial reports"
        );
        let cashier_accounting_coverage = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/accounting/coverage"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_accounting_coverage.status(),
            axum::http::StatusCode::FORBIDDEN,
            "cashiers cannot view branch accounting coverage"
        );
        let cashier_trial_balance = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/accounting/trial-balance"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_trial_balance.status(),
            axum::http::StatusCode::FORBIDDEN,
            "cashiers cannot view branch trial balances"
        );
        let cashier_income_statement = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/accounting/income-statement"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_income_statement.status(),
            axum::http::StatusCode::FORBIDDEN,
            "cashiers cannot view branch income statements"
        );
        let cashier_balance_sheet = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/accounting/balance-sheet?asOfDate=2099-12-31"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_balance_sheet.status(),
            axum::http::StatusCode::FORBIDDEN,
            "cashiers cannot view branch balance sheets"
        );
        let cashier_cash_flow = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/accounting/cash-flow?fromDate=2026-01-01&toDate=2099-12-31"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_cash_flow.status(),
            axum::http::StatusCode::FORBIDDEN,
            "cashiers cannot view branch cash-flow reports"
        );
        let active_attendance = client
            .post(format!("{attendance_base}/me/clock-in"))
            .bearer_auth("opaque-token")
            .header("Idempotency-Key", "attendance-overview-active-test")
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(active_attendance.status(), axum::http::StatusCode::OK);

        let tenant_overview_url =
            format!("http://{api_address}/v1/tenants/{tenant_a}/overview?pageSize=1");
        let tenant_overview = client
            .get(&tenant_overview_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(tenant_overview.status(), axum::http::StatusCode::OK);
        let tenant_overview_body: serde_json::Value = tenant_overview.json().await.unwrap();
        assert_eq!(tenant_overview_body["data"]["totalBranchCount"], 2);
        assert_eq!(tenant_overview_body["data"]["salesWindowHours"], 168);
        assert_eq!(
            tenant_overview_body["data"]["branches"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            tenant_overview_body["data"]["branches"][0]["branchId"],
            branch_a
        );
        assert_eq!(
            tenant_overview_body["data"]["branches"][0]["sales"]["saleCount"],
            4
        );
        assert_eq!(
            tenant_overview_body["data"]["branches"][0]["sales"]["totalsByCurrency"][0]
                ["totalMinor"],
            "3313"
        );
        assert!(tenant_overview_body["data"]["branches"][0]["inventory"]["count"].is_number());
        assert_eq!(
            tenant_overview_body["data"]["branches"][0]["attendance"]["activeStaffCount"],
            1
        );
        assert!(
            tenant_overview_body["data"]["branches"][0]["attendance"]["startedTodayCount"]
                .as_i64()
                .unwrap()
                >= 1
        );
        assert!(
            tenant_overview_body["data"]["branches"][0]["inventory"]["products"]
                .as_array()
                .unwrap()
                .len()
                <= 6
        );
        let tenant_overview_second_page = client
            .get(format!(
                "http://{api_address}/v1/tenants/{tenant_a}/overview?offset=1&pageSize=1"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            tenant_overview_second_page.status(),
            axum::http::StatusCode::OK
        );
        let tenant_overview_second_page_body: serde_json::Value =
            tenant_overview_second_page.json().await.unwrap();
        assert_eq!(
            tenant_overview_second_page_body["data"]["branches"][0]["branchId"],
            branch_a_two
        );
        assert_eq!(
            tenant_overview_second_page_body["data"]["branches"][0]["sales"]["saleCount"],
            0
        );
        assert_eq!(
            tenant_overview_second_page_body["data"]["branches"][0]["attendance"]
                ["activeStaffCount"],
            0
        );
        let invalid_tenant_overview = client
            .get(format!(
                "http://{api_address}/v1/tenants/{tenant_a}/overview?pageSize=0"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            invalid_tenant_overview.status(),
            axum::http::StatusCode::BAD_REQUEST
        );
        let cashier_tenant_overview = client
            .get(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/overview"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            cashier_tenant_overview.status(),
            axum::http::StatusCode::FORBIDDEN,
            "cashiers without sales:read cannot view owner or manager dashboards"
        );

        let movement_url = format!("{drawer_url}/{shift_id}/movements");
        let movement_input = serde_json::json!({"movementType":"paid_out","amountMinor":200,"note":"Test petty cash"});
        let (movement_challenge, movement_proof, movement_bytes) = signed_financial_proof(
            &client,
            &checkout_challenge_url,
            &tenant_a,
            &branch_a,
            &active_station_id,
            lease_id,
            lease_token,
            "drawer-move-01",
            "drawer.movement",
            Some(&shift_id),
            &movement_input,
            active_station_seed,
        )
        .await;
        let movement = client
            .post(&movement_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "drawer-move-01")
            .header("x-station-challenge-id", &movement_challenge)
            .header("x-station-proof", &movement_proof)
            .body(movement_bytes)
            .send()
            .await
            .unwrap();
        let movement_status = movement.status();
        let movement_body: serde_json::Value = movement.json().await.unwrap();
        assert_eq!(
            movement_status,
            axum::http::StatusCode::OK,
            "drawer movement failed: {movement_body}"
        );
        assert_eq!(movement_body["data"]["expectedCashMinor"], 9375);
        let (movement_retry_challenge, movement_retry_proof, movement_retry_bytes) =
            signed_financial_proof(
                &client,
                &checkout_challenge_url,
                &tenant_a,
                &branch_a,
                &active_station_id,
                lease_id,
                lease_token,
                "drawer-move-01",
                "drawer.movement",
                Some(&shift_id),
                &movement_input,
                active_station_seed,
            )
            .await;
        let movement_retry = client
            .post(&movement_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "drawer-move-01")
            .header("x-station-challenge-id", &movement_retry_challenge)
            .header("x-station-proof", &movement_retry_proof)
            .body(movement_retry_bytes)
            .send()
            .await
            .unwrap();
        assert_eq!(movement_retry.status(), axum::http::StatusCode::OK);
        let movement_retry_body: serde_json::Value = movement_retry.json().await.unwrap();
        assert_eq!(movement_retry_body["data"]["movementCount"], 6);

        let close_url = format!("{drawer_url}/{shift_id}/close");
        let close_body = serde_json::json!({"countedCashMinor":9375});
        let (close_challenge, close_proof, close_bytes) = signed_financial_proof(
            &client,
            &checkout_challenge_url,
            &tenant_a,
            &branch_a,
            &active_station_id,
            lease_id,
            lease_token,
            "drawer-close-01",
            "drawer.close",
            Some(&shift_id),
            &close_body,
            active_station_seed,
        )
        .await;
        let closed = client
            .post(&close_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "drawer-close-01")
            .header("x-station-challenge-id", &close_challenge)
            .header("x-station-proof", &close_proof)
            .body(close_bytes)
            .send()
            .await
            .unwrap();
        let closed_status = closed.status();
        let closed_body: serde_json::Value = closed.json().await.unwrap();
        assert_eq!(
            closed_status,
            axum::http::StatusCode::OK,
            "drawer close failed: {closed_body}"
        );
        assert_eq!(closed_body["data"]["expectedCashMinor"], 9375);
        assert_eq!(closed_body["data"]["differenceMinor"], 0);
        assert_eq!(closed_body["data"]["isClosed"], true);
        let final_cash_reconciliation = client
            .get(&cash_reconciliation_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            final_cash_reconciliation.status(),
            axum::http::StatusCode::OK
        );
        let final_cash_reconciliation_body: serde_json::Value =
            final_cash_reconciliation.json().await.unwrap();
        assert_eq!(
            final_cash_reconciliation_body["data"]["status"],
            "unresolved_drawer_movements"
        );
        assert_eq!(
            final_cash_reconciliation_body["data"]["varianceMinor"],
            -100
        );
        assert_eq!(
            final_cash_reconciliation_body["data"]["unresolvedDrawerMovementCount"], 2,
            "cash drop should net to zero while manual till movements stay unresolved"
        );
        let drawer_history = client
            .get(&drawer_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(drawer_history.status(), axum::http::StatusCode::OK);
        let drawer_history_body: serde_json::Value = drawer_history.json().await.unwrap();
        assert_eq!(drawer_history_body["data"].as_array().unwrap().len(), 1);
        assert_eq!(drawer_history_body["data"][0]["shiftId"], shift_id);
        assert_eq!(drawer_history_body["data"][0]["differenceMinor"], 0);
        let (close_retry_challenge, close_retry_proof, close_retry_bytes) = signed_financial_proof(
            &client,
            &checkout_challenge_url,
            &tenant_a,
            &branch_a,
            &active_station_id,
            lease_id,
            lease_token,
            "drawer-close-01",
            "drawer.close",
            Some(&shift_id),
            &close_body,
            active_station_seed,
        )
        .await;
        let close_retry = client
            .post(&close_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "drawer-close-01")
            .header("x-station-challenge-id", &close_retry_challenge)
            .header("x-station-proof", &close_retry_proof)
            .body(close_retry_bytes)
            .send()
            .await
            .unwrap();
        assert_eq!(close_retry.status(), axum::http::StatusCode::OK);
        let close_retry_body: serde_json::Value = close_retry.json().await.unwrap();
        assert_eq!(close_retry_body["data"]["shiftId"], shift_id);
        assert_eq!(close_retry_body["data"]["differenceMinor"], 0);
        let different_close_body = serde_json::json!({"countedCashMinor":10801});
        let (different_close_challenge, different_close_proof, different_close_bytes) =
            signed_financial_proof(
                &client,
                &checkout_challenge_url,
                &tenant_a,
                &branch_a,
                &active_station_id,
                lease_id,
                lease_token,
                "drawer-close-01",
                "drawer.close",
                Some(&shift_id),
                &different_close_body,
                active_station_seed,
            )
            .await;
        let changed_close = client
            .post(&close_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "drawer-close-01")
            .header("x-station-challenge-id", &different_close_challenge)
            .header("x-station-proof", &different_close_proof)
            .body(different_close_bytes)
            .send()
            .await
            .unwrap();
        assert_eq!(changed_close.status(), axum::http::StatusCode::CONFLICT);

        let owner_only_station_challenge = client
            .post(format!(
                "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a3}/checkout-challenge"
            ))
            .bearer_auth("opaque-token")
            .header("x-station-lease-id", lease_id)
            .header("x-station-lease-token", lease_token)
            .header("idempotency-key", "owner-cannot-checkout")
            .json(&serde_json::json!({"payloadHash": payload_hash.clone()}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            owner_only_station_challenge.status(),
            axum::http::StatusCode::FORBIDDEN,
            "owner POS view access does not grant checkout permission"
        );

        let heartbeat = client
            .post(format!(
                "{base_url}/v1/station-actor-leases/{lease_id}/heartbeat"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({"token": lease_token}))
            .send()
            .await
            .unwrap();
        assert_eq!(heartbeat.status(), axum::http::StatusCode::OK);
        let other_cashier_during_active_lease = client
            .post(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/actor-lease"
            ))
            .bearer_auth("opaque-token")
            .json(&signed_lease_payload(
                &client,
                &format!("http://{api_address_three}"),
                &tenant_a,
                &branch_a,
                active_station_id,
                &membership_id_two,
                &identity_id_two,
                active_activation_id,
                active_station_seed,
            ).await)
            .send()
            .await
            .unwrap();
        let other_cashier_status = other_cashier_during_active_lease.status();
        let other_cashier_body = other_cashier_during_active_lease.text().await.unwrap();
        assert_eq!(
            other_cashier_status,
            axum::http::StatusCode::CONFLICT,
            "second cashier should pass device proof and be denied only because station is held: {other_cashier_body}"
        );
        let seed_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&seed_url)
            .await
            .unwrap();
        sqlx::query("UPDATE station_actor_leases SET started_at = now() - interval '1 minute', expires_at = now() - interval '1 second' WHERE id = $1::uuid")
            .bind(lease_id)
            .execute(&seed_pool)
            .await
            .unwrap();
        seed_pool.close().await;
        let rotation_url = format!(
            "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/actor-lease"
        );
        let rotation_payload = signed_lease_payload(
            &client,
            &format!("http://{api_address_three}"),
            &tenant_a,
            &branch_a,
            active_station_id,
            &membership_id_two,
            &identity_id_two,
            active_activation_id,
            active_station_seed,
        )
        .await;
        let rotated_lease = client
            .post(rotation_url)
            .bearer_auth("opaque-token")
            .json(&rotation_payload)
            .send()
            .await
            .unwrap();
        let rotated_status = rotated_lease.status();
        let rotated_body: serde_json::Value = rotated_lease.json().await.unwrap();
        assert_eq!(
            rotated_status,
            axum::http::StatusCode::OK,
            "cashier should reclaim an expired station lease: {rotated_body}"
        );
        let rotated_id = rotated_body["data"]["leaseId"].as_str().unwrap();
        let rotated_token = rotated_body["data"]["token"].as_str().unwrap();
        let seed_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&seed_url)
            .await
            .unwrap();
        let previous_lease_status = sqlx::query_scalar::<_, String>(
            "SELECT status FROM station_actor_leases WHERE id = $1::uuid",
        )
        .bind(lease_id)
        .fetch_one(&seed_pool)
        .await
        .unwrap();
        assert_eq!(previous_lease_status, "issued");
        let old_lock_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM station_actor_lease_locks WHERE lease_id = $1::uuid",
        )
        .bind(lease_id)
        .fetch_one(&seed_pool)
        .await
        .unwrap();
        assert_eq!(
            old_lock_count, 0,
            "expired lease history is kept while its station lock is reclaimed"
        );
        seed_pool.close().await;
        let released = client
            .post(format!(
                "http://{api_address_three}/v1/station-actor-leases/{rotated_id}/release"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({"token": rotated_token}))
            .send()
            .await
            .unwrap();
        assert_eq!(released.status(), axum::http::StatusCode::OK);
        let replayed_heartbeat = client
            .post(format!(
                "{base_url}/v1/station-actor-leases/{lease_id}/heartbeat"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({"token": lease_token}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            replayed_heartbeat.status(),
            axum::http::StatusCode::CONFLICT
        );

        let owner_without_cashier_profile = client
            .post(format!(
                "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a3}/actor-lease"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "challengeId": Uuid::new_v4().to_string(),
                "signature": "00".repeat(64)
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            owner_without_cashier_profile.status(),
            axum::http::StatusCode::FORBIDDEN,
            "tenant owner rights alone must not allow checkout lease acquisition"
        );

        let over_capacity_payload = signed_activation_payload(
            &client,
            &base_url,
            &tenant_a,
            &branch_a,
            if over_capacity_url.contains(&station_a2) {
                &station_a2
            } else {
                &station_a
            },
            0xc3,
        )
        .await;
        let over_capacity = client
            .post(&over_capacity_url)
            .bearer_auth("opaque-token")
            .json(&over_capacity_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(over_capacity.status(), axum::http::StatusCode::CONFLICT);
        let capacity_body: serde_json::Value = over_capacity.json().await.unwrap();
        assert_eq!(capacity_body["error"]["code"], "station_capacity_reached");

        let station_permissions = client
            .get(format!(
                "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{station_a}/permissions"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(station_permissions.status(), axum::http::StatusCode::OK);
        let permission_body: serde_json::Value = station_permissions.json().await.unwrap();
        assert!(permission_body["data"]["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|permission| permission == "pos:use"));

        let cross_tenant = client
            .get(format!(
                "{base_url}/v1/tenants/{tenant_b}/branches/{branch_b}/stations/{station_b}/permissions"
            ))
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(cross_tenant.status(), axum::http::StatusCode::NOT_FOUND);

        let activation_revocation_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/activation-revocation"
        );
        let non_owner_revocation = client
            .post(format!(
                "http://{api_address_three}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/activation-revocation"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({"reason": "Device replacement"}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            non_owner_revocation.status(),
            axum::http::StatusCode::FORBIDDEN
        );

        let invalid_revocation = client
            .post(&activation_revocation_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({"reason": "  "}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            invalid_revocation.status(),
            axum::http::StatusCode::BAD_REQUEST
        );
        let stale_challenge_payload = signed_activation_payload(
            &client,
            active_base_url,
            &tenant_a,
            &branch_a,
            active_station_id,
            0xd4,
        )
        .await;

        let active_lease_payload = signed_lease_payload(
            &client,
            active_base_url,
            &tenant_a,
            &branch_a,
            active_station_id,
            &membership_id,
            &identity_id,
            active_activation_id,
            active_station_seed,
        )
        .await;
        let active_lease_for_revocation = client
            .post(&lease_url)
            .bearer_auth("opaque-token")
            .json(&active_lease_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            active_lease_for_revocation.status(),
            axum::http::StatusCode::OK
        );
        let active_lease_body: serde_json::Value =
            active_lease_for_revocation.json().await.unwrap();
        let active_lease_id = active_lease_body["data"]["leaseId"].as_str().unwrap();
        let active_lease_token = active_lease_body["data"]["token"].as_str().unwrap();

        let revocation = client
            .post(&activation_revocation_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({"reason": "Lost station hardware"}))
            .send()
            .await
            .unwrap();
        let revocation_status = revocation.status();
        let revocation_body: serde_json::Value = revocation.json().await.unwrap();
        assert_eq!(
            revocation_status,
            axum::http::StatusCode::OK,
            "owner station revocation should succeed: {revocation_body}"
        );
        assert_eq!(revocation_body["data"]["activeStationCount"], 0);
        assert_eq!(
            revocation_body["data"]["activationId"],
            active_body["data"]["activationId"]
        );
        let invalidated_challenge = client
            .post(&active_station_url)
            .bearer_auth("opaque-token")
            .json(&stale_challenge_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            invalidated_challenge.status(),
            axum::http::StatusCode::CONFLICT
        );
        let invalidated_challenge_body: serde_json::Value =
            invalidated_challenge.json().await.unwrap();
        assert_eq!(
            invalidated_challenge_body["error"]["code"],
            "activation_challenge_invalid"
        );

        let revoked_lease_heartbeat = client
            .post(format!(
                "{base_url}/v1/station-actor-leases/{active_lease_id}/heartbeat"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({"token": active_lease_token}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            revoked_lease_heartbeat.status(),
            axum::http::StatusCode::CONFLICT
        );

        let seed_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&seed_url)
            .await
            .unwrap();
        let (revoked_status, revoked_by, revocation_reason) =
            sqlx::query_as::<_, (String, String, String)>(
                "SELECT status, revoked_by_membership_id::text, revocation_reason
             FROM station_activations WHERE id = $1::uuid",
            )
            .bind(revocation_body["data"]["activationId"].as_str().unwrap())
            .fetch_one(&seed_pool)
            .await
            .unwrap();
        assert_eq!(revoked_status, "revoked");
        assert_eq!(revoked_by, membership_id);
        assert_eq!(revocation_reason, "Lost station hardware");
        let (lease_status, lease_end_reason, lock_count) =
            sqlx::query_as::<_, (String, String, i64)>(
                "SELECT l.status, l.ended_reason,
                    (SELECT count(*) FROM station_actor_lease_locks k WHERE k.lease_id = l.id)
             FROM station_actor_leases l WHERE l.id = $1::uuid",
            )
            .bind(active_lease_id)
            .fetch_one(&seed_pool)
            .await
            .unwrap();
        assert_eq!(lease_status, "released");
        assert_eq!(lease_end_reason, "station_activation_revoked");
        assert_eq!(lock_count, 0);
        seed_pool.close().await;

        let reactivation_payload = signed_activation_payload(
            &client,
            active_base_url,
            &tenant_a,
            &branch_a,
            active_station_id,
            0xd4,
        )
        .await;
        let reactivation = client
            .post(&active_station_url)
            .bearer_auth("opaque-token")
            .json(&reactivation_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(reactivation.status(), axum::http::StatusCode::OK);
        let reactivation_body: serde_json::Value = reactivation.json().await.unwrap();
        assert_eq!(reactivation_body["data"]["activeStationCount"], 1);
        assert_eq!(reactivation_body["data"]["created"], true);
        assert_ne!(
            reactivation_body["data"]["activationId"],
            revocation_body["data"]["activationId"]
        );

        let backfill_sale_id = Uuid::new_v4();
        let backfill_sale_line_id = Uuid::new_v4();
        let backfill_sale_tender_id = Uuid::new_v4();
        let invalid_backfill_sale_id = Uuid::new_v4();
        let invalid_backfill_tender_id = Uuid::new_v4();
        let backfill_refund_id = Uuid::new_v4();
        let backfill_refund_line_id = Uuid::new_v4();
        let backfill_refund_tender_id = Uuid::new_v4();
        let legacy_other_expense_id = Uuid::new_v4();
        let legacy_other_expense_journal_id = Uuid::new_v4();
        let legacy_other_reimbursement_id = Uuid::new_v4();
        let legacy_other_reimbursement_journal_id = Uuid::new_v4();
        let backfill_lease_id = Uuid::new_v4();
        let backfill_activation_id = Uuid::new_v4();
        let backfill_product_id = Uuid::new_v4();
        let backfill_payment_option_id = Uuid::new_v4();
        let backfill_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&seed_url)
            .await
            .expect("the migration credential should reconnect for the legacy accounting fixture");
        let mut backfill_tx = backfill_pool.begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role='replica'")
            .execute(&mut *backfill_tx)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO hosted_sales(id,tenant_id,branch_id,station_id,cashier_membership_id,lease_id,activation_id,receipt_number,cashier_name_snapshot,station_name_snapshot,currency_code,subtotal_minor,tax_minor,total_minor,request_hash,idempotency_key,created_at)
             VALUES($1,$2::uuid,$3::uuid,$4::uuid,$5::uuid,$6,$7,$8,'Legacy cashier','Legacy station','GHS',10000,0,10000,decode(repeat('ab',32),'hex'),$9,'2026-10-01T00:00:00Z')",
        )
        .bind(backfill_sale_id)
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(Uuid::parse_str(&station_a3).unwrap())
        .bind(&membership_id)
        .bind(backfill_lease_id)
        .bind(backfill_activation_id)
        .bind(format!("legacy-{}", Uuid::new_v4()))
        .bind(format!("legacy-sale-{}", Uuid::new_v4()))
        .execute(&mut *backfill_tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO hosted_sale_lines(id,tenant_id,branch_id,sale_id,line_number,product_id,sku_snapshot,name_snapshot,quantity,unit_price_minor,tax_rate_bps,subtotal_minor,tax_minor,total_minor)
             VALUES($1,$2::uuid,$3::uuid,$4,1,$5,'LEGACY-1','Legacy item',2,5000,0,10000,0,10000)",
        )
        .bind(backfill_sale_line_id)
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(backfill_sale_id)
        .bind(backfill_product_id)
        .execute(&mut *backfill_tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO hosted_sale_tenders(id,tenant_id,branch_id,sale_id,payment_option_id,method_key_snapshot,label_snapshot,tender_kind,amount_minor)
             VALUES($1,$2::uuid,$3::uuid,$4,$5,'cash','Cash','cash',10000)",
        )
        .bind(backfill_sale_tender_id)
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(backfill_sale_id)
        .bind(backfill_payment_option_id)
        .execute(&mut *backfill_tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO hosted_sales(id,tenant_id,branch_id,station_id,cashier_membership_id,lease_id,activation_id,receipt_number,cashier_name_snapshot,station_name_snapshot,currency_code,subtotal_minor,tax_minor,total_minor,request_hash,idempotency_key,created_at)
             VALUES($1,$2::uuid,$3::uuid,$4::uuid,$5::uuid,$6,$7,$8,'Legacy cashier','Legacy station','GHS',1000,0,1000,decode(repeat('ef',32),'hex'),$9,'2026-10-01T00:00:00Z')",
        )
        .bind(invalid_backfill_sale_id)
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(Uuid::parse_str(&station_a3).unwrap())
        .bind(&membership_id)
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(format!("inv-{}", Uuid::new_v4().simple()))
        .bind(format!("legacy-invalid-sale-{}", Uuid::new_v4()))
        .execute(&mut *backfill_tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO hosted_sale_tenders(id,tenant_id,branch_id,sale_id,payment_option_id,method_key_snapshot,label_snapshot,tender_kind,amount_minor)
             VALUES($1,$2::uuid,$3::uuid,$4,$5,'cash','Cash','cash',900)",
        )
        .bind(invalid_backfill_tender_id)
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(invalid_backfill_sale_id)
        .bind(Uuid::new_v4())
        .execute(&mut *backfill_tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO hosted_sale_refunds(id,tenant_id,branch_id,station_id,sale_id,actor_membership_id,lease_id,activation_id,refund_type,total_minor,reason,idempotency_key,request_hash,cashier_name_snapshot,station_name_snapshot,created_at)
             VALUES($1,$2::uuid,$3::uuid,$4,$5,$6::uuid,$7,$8,'refund',5000,'Legacy partial return',$9,decode(repeat('cd',32),'hex'),'Legacy manager','Legacy station','2026-10-02T00:00:00Z')",
        )
        .bind(backfill_refund_id)
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(Uuid::parse_str(&station_a3).unwrap())
        .bind(backfill_sale_id)
        .bind(&membership_id)
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(format!("legacy-refund-{}", Uuid::new_v4()))
        .execute(&mut *backfill_tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO hosted_sale_refund_lines(id,tenant_id,branch_id,sale_id,refund_id,sale_line_id,product_id,quantity,restock,subtotal_minor,tax_minor,amount_minor)
             VALUES($1,$2::uuid,$3::uuid,$4,$5,$6,$7,1,true,5000,0,5000)",
        )
        .bind(backfill_refund_line_id)
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(backfill_sale_id)
        .bind(backfill_refund_id)
        .bind(backfill_sale_line_id)
        .bind(backfill_product_id)
        .execute(&mut *backfill_tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO hosted_sale_refund_tenders(id,tenant_id,branch_id,refund_id,method,amount_minor)
             VALUES($1,$2::uuid,$3::uuid,$4,'cash',5000)",
        )
        .bind(backfill_refund_tender_id)
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(backfill_refund_id)
        .execute(&mut *backfill_tx)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO hosted_expenses(id,tenant_id,branch_id,currency_code,category_code,description,amount_minor,payment_method,occurred_on,actor_membership_id,idempotency_key,request_hash)
             VALUES($1,$2::uuid,$3::uuid,'GHS','other','Legacy miscellaneous expense',1000,'card','2026-10-03',$4::uuid,$5,decode(repeat('11',32),'hex'))",
        )
        .bind(legacy_other_expense_id)
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(&membership_id)
        .bind(format!("legacy-other-{}", Uuid::new_v4()))
        .execute(&mut *backfill_tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO hosted_expense_reimbursements(id,tenant_id,branch_id,expense_id,currency_code,amount_minor,return_method,external_reference,reason,returned_on,actor_membership_id,idempotency_key,request_hash)
             VALUES($1,$2::uuid,$3::uuid,$4,'GHS',200,'card','LEGACY-RETURN','Legacy partial reimbursement','2026-10-04',$5::uuid,$6,decode(repeat('22',32),'hex'))",
        )
        .bind(legacy_other_reimbursement_id)
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(legacy_other_expense_id)
        .bind(&membership_id)
        .bind(format!("legacy-other-reimbursement-{}", Uuid::new_v4()))
        .execute(&mut *backfill_tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO hosted_accounting_journals(id,tenant_id,branch_id,source_type,source_id,currency_code,effective_at,actor_membership_id,description)
             VALUES($1,$2::uuid,$3::uuid,'expense',$4,'GHS','2026-10-03T00:00:00Z',$5::uuid,'Expense: other'),
                   ($6,$2::uuid,$3::uuid,'expense_reimbursement',$7,'GHS','2026-10-04T00:00:00Z',$5::uuid,'Reimburse expense: other')",
        )
        .bind(legacy_other_expense_journal_id)
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(legacy_other_expense_id)
        .bind(&membership_id)
        .bind(legacy_other_reimbursement_journal_id)
        .bind(legacy_other_reimbursement_id)
        .execute(&mut *backfill_tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO hosted_accounting_journal_lines(tenant_id,branch_id,journal_id,line_number,account_code,description,debit_minor,credit_minor)
             VALUES($1::uuid,$2::uuid,$3,1,'5000','Expense',1000,0),($1::uuid,$2::uuid,$3,2,'1010','Payment clearing',0,1000),
                   ($1::uuid,$2::uuid,$4,1,'1010','Payment clearing',200,0),($1::uuid,$2::uuid,$4,2,'5000','Reimbursement',0,200)",
        )
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(legacy_other_expense_journal_id)
        .bind(legacy_other_reimbursement_journal_id)
        .execute(&mut *backfill_tx)
        .await
        .unwrap();

        let backfill_sql = include_str!(
            "../migrations/postgres/0045_hosted_historical_sale_accounting_backfill.sql"
        );
        sqlx::raw_sql(backfill_sql)
            .execute(&mut *backfill_tx)
            .await
            .unwrap();
        sqlx::raw_sql(backfill_sql)
            .execute(&mut *backfill_tx)
            .await
            .unwrap();
        let expense_reclassification_sql =
            include_str!("../migrations/postgres/0046_reclassify_other_expense_account.sql");
        let expense_reclassification_data_sql = expense_reclassification_sql
            .split_once("\nINSERT INTO hosted_accounting_journals")
            .map(|(_, data)| format!("INSERT INTO hosted_accounting_journals{data}"))
            .expect("the expense reclassification migration should include its idempotent journal data section");
        // This replay is derived only from the compile-time embedded migration; its DDL prefix is
        // removed so the legacy idempotent data statements can be checked without narrowing the
        // newer source-type constraint.
        sqlx::raw_sql(sqlx::AssertSqlSafe(
            expense_reclassification_data_sql.as_str(),
        ))
        .execute(&mut *backfill_tx)
        .await
        .unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(
            expense_reclassification_data_sql.as_str(),
        ))
        .execute(&mut *backfill_tx)
        .await
        .unwrap();
        backfill_tx.commit().await.unwrap();

        let reclassification_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM hosted_accounting_journals WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND source_type='accounting_adjustment' AND source_id IN ($3,$4)",
        )
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(legacy_other_expense_id)
        .bind(legacy_other_reimbursement_id)
        .fetch_one(&backfill_pool)
        .await
        .unwrap();
        assert_eq!(
            reclassification_count, 2,
            "other expense and reimbursement corrections are created once"
        );
        let (legacy_cogs_net, corrected_expense_net, adjustment_debits, adjustment_credits) =
            sqlx::query_as::<_, (i64, i64, i64, i64)>(
                "SELECT
                   COALESCE(SUM(l.debit_minor-l.credit_minor) FILTER (WHERE l.account_code='5000'),0)::bigint,
                   COALESCE(SUM(l.debit_minor-l.credit_minor) FILTER (WHERE l.account_code='6000'),0)::bigint,
                   COALESCE(SUM(l.debit_minor) FILTER (WHERE j.source_type='accounting_adjustment'),0)::bigint,
                   COALESCE(SUM(l.credit_minor) FILTER (WHERE j.source_type='accounting_adjustment'),0)::bigint
                 FROM hosted_accounting_journals j JOIN hosted_accounting_journal_lines l
                   ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id
                 WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND j.source_id IN ($3,$4)",
            )
            .bind(&tenant_a)
            .bind(&branch_a_two)
            .bind(legacy_other_expense_id)
            .bind(legacy_other_reimbursement_id)
            .fetch_one(&backfill_pool)
            .await
            .unwrap();
        assert_eq!(
            legacy_cogs_net, 0,
            "legacy Other costs no longer distort cost of goods sold"
        );
        assert_eq!(
            corrected_expense_net, 800,
            "net other expense is classified as operating expense"
        );
        assert_eq!(adjustment_debits, 1200);
        assert_eq!(adjustment_credits, 1200);

        for (source_type, source_id, expected_total, expected_cost_lines) in [
            ("sale", backfill_sale_id, 10_000_i64, 0_i64),
            ("sale_refund", backfill_refund_id, 5_000_i64, 0_i64),
        ] {
            let journal_count = sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM hosted_accounting_journals WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND source_type=$3 AND source_id=$4",
            )
            .bind(&tenant_a)
            .bind(&branch_a_two)
            .bind(source_type)
            .bind(source_id)
            .fetch_one(&backfill_pool)
            .await
            .unwrap();
            assert_eq!(
                journal_count, 1,
                "legacy source backfill must be idempotent"
            );
            let (coverage, debits, credits, line_count, cost_lines) = sqlx::query_as::<
                _,
                (String, i64, i64, i64, i64),
            >(
                "SELECT j.coverage_status,COALESCE(SUM(l.debit_minor),0)::bigint,COALESCE(SUM(l.credit_minor),0)::bigint,COUNT(l.id)::bigint,COUNT(*) FILTER (WHERE l.account_code IN ('1300','5000'))::bigint
                 FROM hosted_accounting_journals j JOIN hosted_accounting_journal_lines l ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id
                 WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND j.source_type=$3 AND j.source_id=$4
                 GROUP BY j.id,j.coverage_status",
            )
            .bind(&tenant_a)
            .bind(&branch_a_two)
            .bind(source_type)
            .bind(source_id)
            .fetch_one(&backfill_pool)
            .await
            .unwrap();
            assert_eq!(coverage, "partial");
            assert_eq!(debits, expected_total);
            assert_eq!(credits, expected_total);
            assert_eq!(line_count, 2);
            assert_eq!(cost_lines, expected_cost_lines);
            let (cash_net, revenue_net, returns_net) = sqlx::query_as::<_, (i64, i64, i64)>(
                "SELECT COALESCE(SUM(l.debit_minor-l.credit_minor) FILTER (WHERE l.account_code='1000'),0)::bigint,
                        COALESCE(SUM(l.debit_minor-l.credit_minor) FILTER (WHERE l.account_code='4000'),0)::bigint,
                        COALESCE(SUM(l.debit_minor-l.credit_minor) FILTER (WHERE l.account_code='4020'),0)::bigint
                 FROM hosted_accounting_journals j JOIN hosted_accounting_journal_lines l ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id
                 WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND j.source_type=$3 AND j.source_id=$4",
            )
            .bind(&tenant_a)
            .bind(&branch_a_two)
            .bind(source_type)
            .bind(source_id)
            .fetch_one(&backfill_pool)
            .await
            .unwrap();
            let expected_accounts = if source_type == "sale" {
                (expected_total, -expected_total, 0)
            } else {
                (-expected_total, 0, expected_total)
            };
            assert_eq!((cash_net, revenue_net, returns_net), expected_accounts);
        }
        let invalid_sale_journal_count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM hosted_accounting_journals WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND source_type='sale' AND source_id=$3",
        )
        .bind(&tenant_a)
        .bind(&branch_a_two)
        .bind(invalid_backfill_sale_id)
        .fetch_one(&backfill_pool)
        .await
        .unwrap();
        assert_eq!(
            invalid_sale_journal_count, 0,
            "invalid historical source totals must be excluded for reconciliation review"
        );
        backfill_pool.close().await;

        let mapped_product_create = client
            .post(format!(
                "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/products"
            ))
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "sku": "MAP-TEST-001",
                "barcode": "MAPBAR-001",
                "name": "Explicitly shared branch item",
                "description": null,
                "category": "Test fixtures",
                "priceMinor": 2500,
                "costMinor": null,
                "taxRateBps": 0,
                "openingStock": 0,
                "reorderPoint": 0,
                "targetStockQuantity": 0
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            mapped_product_create.status(),
            axum::http::StatusCode::CREATED
        );
        let mapped_product: serde_json::Value = mapped_product_create.json().await.unwrap();
        let mapped_product_id = mapped_product["data"]["productId"]
            .as_str()
            .unwrap()
            .to_owned();

        let candidates_url =
            format!("{base_url}/v1/tenants/{tenant_a}/branches/{branch_a_two}/product-candidates");
        let candidates = client
            .get(&candidates_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap();
        assert_eq!(candidates.status(), axum::http::StatusCode::OK);
        let candidates_body: serde_json::Value = candidates.json().await.unwrap();
        assert!(candidates_body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["productId"] == mapped_product_id
                    && item["sku"] == "MAP-TEST-001"
                    && item["barcode"] == "MAPBAR-001"
            }));

        let attach_url = format!(
            "{base_url}/v1/tenants/{tenant_a}/branches/{branch_a_two}/products/{mapped_product_id}/attach"
        );
        let attached_product = client
            .post(&attach_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "priceMinor": 3100,
                "costMinor": 1200,
                "taxRateBps": 1500,
                "openingStock": 7,
                "reorderPoint": 2,
                "targetStockQuantity": 10
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(attached_product.status(), axum::http::StatusCode::CREATED);
        let attached_body: serde_json::Value = attached_product.json().await.unwrap();
        assert_eq!(attached_body["data"]["productId"], mapped_product_id);
        assert_eq!(attached_body["data"]["sku"], "MAP-TEST-001");
        assert_eq!(attached_body["data"]["priceMinor"], 3100);
        assert_eq!(attached_body["data"]["stockQuantity"], 7);

        let candidate_replay = client
            .get(&candidates_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap();
        assert!(!candidate_replay["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| { item["productId"] == mapped_product_id }));
        let duplicate_attach = client
            .post(&attach_url)
            .bearer_auth("opaque-token")
            .json(&serde_json::json!({
                "priceMinor": 3100,
                "costMinor": 1200,
                "taxRateBps": 1500,
                "openingStock": 7,
                "reorderPoint": 2,
                "targetStockQuantity": 10
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(duplicate_attach.status(), axum::http::StatusCode::CONFLICT);

        // Finish a shared checkout only after the rest of this large fixture
        // has made its assertions, then prove an exact retry from the other
        // station returns the original sale without a second stock/sale write.
        let end_seed_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&seed_url)
            .await
            .unwrap();
        sqlx::query("UPDATE tenant_entitlements SET numeric_limit=2 WHERE tenant_id=$1::uuid AND entitlement_key='stations.active'")
            .bind(&tenant_a).execute(&end_seed_pool).await.unwrap();
        let second_activation_payload = signed_activation_payload(
            &client,
            active_base_url,
            &tenant_a,
            &branch_a,
            &second_station_id,
            second_station_seed,
        )
        .await;
        let second_activation_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{second_station_id}/activation"
        );
        let second_activation_response = client
            .post(&second_activation_url)
            .bearer_auth("opaque-token")
            .json(&second_activation_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            second_activation_response.status(),
            axum::http::StatusCode::OK
        );
        let second_activation_body: serde_json::Value =
            second_activation_response.json().await.unwrap();
        sqlx::query("UPDATE branch_catalog_items SET price_minor=1000,tax_rate_bps=0,stock_quantity=5,status='active' WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid")
            .bind(&tenant_a).bind(&branch_a).bind(&product_id).execute(&end_seed_pool).await.unwrap();
        sqlx::query("UPDATE branch_payment_options SET enabled=true WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND id=$3::uuid")
            .bind(&tenant_a).bind(&branch_a).bind(&payment_option_id).execute(&end_seed_pool).await.unwrap();

        let first_end_lease_payload = signed_lease_payload(
            &client,
            active_base_url,
            &tenant_a,
            &branch_a,
            active_station_id,
            &membership_id,
            &identity_id,
            reactivation_body["data"]["activationId"].as_str().unwrap(),
            0xd4,
        )
        .await;
        let first_end_lease_response = client
            .post(&lease_url)
            .bearer_auth("opaque-token")
            .json(&first_end_lease_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            first_end_lease_response.status(),
            axum::http::StatusCode::OK
        );
        let first_end_lease: serde_json::Value = first_end_lease_response.json().await.unwrap();
        let first_end_lease_id = first_end_lease["data"]["leaseId"]
            .as_str()
            .unwrap()
            .to_owned();
        let first_end_lease_token = first_end_lease["data"]["token"]
            .as_str()
            .unwrap()
            .to_owned();
        let second_end_lease_payload = signed_lease_payload(
            &client,
            active_base_url,
            &tenant_a,
            &branch_a,
            &second_station_id,
            &membership_id,
            &identity_id,
            second_activation_body["data"]["activationId"]
                .as_str()
                .unwrap(),
            second_station_seed,
        )
        .await;
        let second_end_lease_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{second_station_id}/actor-lease"
        );
        let second_end_lease_response = client
            .post(&second_end_lease_url)
            .bearer_auth("opaque-token")
            .json(&second_end_lease_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(
            second_end_lease_response.status(),
            axum::http::StatusCode::OK
        );
        let second_end_lease: serde_json::Value = second_end_lease_response.json().await.unwrap();
        let second_end_lease_id = second_end_lease["data"]["leaseId"]
            .as_str()
            .unwrap()
            .to_owned();
        let second_end_lease_token = second_end_lease["data"]["token"]
            .as_str()
            .unwrap()
            .to_owned();
        sqlx::query("UPDATE station_actor_leases SET expires_at=now()+interval '15 minutes' WHERE id IN ($1::uuid,$2::uuid) AND status='issued'")
            .bind(&first_end_lease_id).bind(&second_end_lease_id).execute(&end_seed_pool).await.unwrap();

        let shared_tickets_now: serde_json::Value = client
            .get(&shared_ticket_branch_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let shared_ticket_row = shared_tickets_now["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|ticket| ticket["id"] == shared_ticket_id)
            .expect("released shared ticket remains in branch queue");
        let claim_for_sale_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/held-tickets/{shared_ticket_id}/claim"
        );
        let sale_claim = client
            .post(&claim_for_sale_url)
            .bearer_auth("opaque-token")
            .header("X-Station-Lease-Id", &first_end_lease_id)
            .header("X-Station-Lease-Token", &first_end_lease_token)
            .json(&serde_json::json!({"expectedRevision": shared_ticket_row["revision"]}))
            .send()
            .await
            .unwrap();
        assert_eq!(sale_claim.status(), axum::http::StatusCode::OK);
        let sale_claim_body: serde_json::Value = sale_claim.json().await.unwrap();
        let shared_sale_body = serde_json::json!({
            "customerId": null,
            "items": [{"productId": product_id, "quantity": 2}],
            "tenders": [{"paymentOptionId": payment_option_id, "amountMinor": 2000, "reference": "SHARED-TEST-001"}],
            "heldTicketId": shared_ticket_id
        });
        let shared_sale_idempotency_key = "shared-sale-exact-replay-integration-001";
        let shared_sale_checkout_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/held-tickets/{shared_ticket_id}/checkout-attempt"
        );
        let prepared_sale_attempt = client
            .post(&shared_sale_checkout_url)
            .bearer_auth("opaque-token")
            .header("X-Station-Lease-Id", &first_end_lease_id)
            .header("X-Station-Lease-Token", &first_end_lease_token)
            .json(&serde_json::json!({
                "expectedRevision": sale_claim_body["data"]["revision"],
                "idempotencyKey": shared_sale_idempotency_key,
                "body": shared_sale_body
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(prepared_sale_attempt.status(), axum::http::StatusCode::OK);
        assert_eq!(
            prepared_sale_attempt
                .json::<serde_json::Value>()
                .await
                .unwrap()["data"]["status"],
            "checkout_pending"
        );
        let shared_sale_challenge_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/checkout-challenge"
        );
        let (sale_challenge_id, sale_proof, sale_raw_body) = signed_financial_proof(
            &client,
            &shared_sale_challenge_url,
            &tenant_a,
            &branch_a,
            active_station_id,
            &first_end_lease_id,
            &first_end_lease_token,
            shared_sale_idempotency_key,
            "sale.commit",
            None,
            &shared_sale_body,
            0xd4,
        )
        .await;
        let first_shared_sale_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/sales"
        );
        let first_shared_sale = client
            .post(&first_shared_sale_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", &first_end_lease_id)
            .header("x-station-lease-token", &first_end_lease_token)
            .header("idempotency-key", shared_sale_idempotency_key)
            .header("x-station-challenge-id", &sale_challenge_id)
            .header("x-station-proof", &sale_proof)
            .body(sale_raw_body)
            .send()
            .await
            .unwrap();
        assert_eq!(first_shared_sale.status(), axum::http::StatusCode::OK);
        let first_shared_sale_body: serde_json::Value = first_shared_sale.json().await.unwrap();
        assert_eq!(first_shared_sale_body["data"]["replayed"], false);
        let completed_shared_sale_id = first_shared_sale_body["data"]["id"].as_str().unwrap();

        let second_station_sale_challenge_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{second_station_id}/checkout-challenge"
        );
        let (replay_challenge_id, replay_proof, replay_raw_body) = signed_financial_proof(
            &client,
            &second_station_sale_challenge_url,
            &tenant_a,
            &branch_a,
            &second_station_id,
            &second_end_lease_id,
            &second_end_lease_token,
            shared_sale_idempotency_key,
            "sale.commit",
            None,
            &shared_sale_body,
            second_station_seed,
        )
        .await;
        let replay_sale_url = format!(
            "{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{second_station_id}/sales"
        );
        let replayed_shared_sale = client
            .post(&replay_sale_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", &second_end_lease_id)
            .header("x-station-lease-token", &second_end_lease_token)
            .header("idempotency-key", shared_sale_idempotency_key)
            .header("x-station-challenge-id", &replay_challenge_id)
            .header("x-station-proof", &replay_proof)
            .body(replay_raw_body)
            .send()
            .await
            .unwrap();
        let replayed_shared_sale_status = replayed_shared_sale.status();
        let replayed_shared_sale_text = replayed_shared_sale.text().await.unwrap();
        assert_eq!(
            replayed_shared_sale_status,
            axum::http::StatusCode::OK,
            "cross-station exact sale retry failed: {replayed_shared_sale_text}"
        );
        let replayed_shared_sale_body: serde_json::Value =
            serde_json::from_str(&replayed_shared_sale_text).unwrap();
        assert_eq!(
            replayed_shared_sale_body["data"]["id"],
            completed_shared_sale_id
        );
        assert_eq!(replayed_shared_sale_body["data"]["replayed"], true);
        let (shared_sale_count, shared_ticket_status, linked_sale_id): (i64, String, String) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM hosted_sales WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND held_ticket_id=$3::uuid),
                    t.status,t.completed_sale_id::text
             FROM branch_held_tickets t WHERE t.tenant_id=$1::uuid AND t.branch_id=$2::uuid AND t.id=$3::uuid",
        ).bind(&tenant_a).bind(&branch_a).bind(&shared_ticket_id).fetch_one(&end_seed_pool).await.unwrap();
        assert_eq!(
            shared_sale_count, 1,
            "cross-station retry must not create a duplicate hosted sale"
        );
        assert_eq!(shared_ticket_status, "completed");
        assert_eq!(linked_sale_id, completed_shared_sale_id);
        let cashier_credit_term_url =
            format!("{base_url}/v1/tenants/{tenant_a}/customers/{customer_id}/credit-terms/GHS");
        let cashier_credit_term = client.put(&cashier_credit_term_url).bearer_auth("opaque-token")
            .header("Idempotency-Key", "cashier-credit-sale-term-001")
            .json(&serde_json::json!({"expectedRevision":0,"creditLimitMinor":1500,"dueDays":14,"reason":"Integration test credit sale"}))
            .send().await.unwrap();
        assert_eq!(cashier_credit_term.status(), axum::http::StatusCode::OK);

        sqlx::query("UPDATE branch_catalog_items SET price_minor=1000,cost_minor=400,tax_rate_bps=0,stock_quantity=5 WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid")
            .bind(&tenant_a).bind(&branch_a).bind(&product_id).execute(&end_seed_pool).await.unwrap();
        let credit_sale_body = serde_json::json!({
            "customerId": customer_id,
            "items": [{"productId": product_id, "quantity": 1}],
            "tenders": [],
            "onAccountMinor": 1000
        });
        let credit_sale_key = "credit-sale-integration-001";
        let credit_sale_challenge_url = format!("{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/checkout-challenge");
        let (credit_sale_challenge, credit_sale_proof, credit_sale_bytes) = signed_financial_proof(
            &client,
            &credit_sale_challenge_url,
            &tenant_a,
            &branch_a,
            &active_station_id,
            &first_end_lease_id,
            &first_end_lease_token,
            credit_sale_key,
            "sale.commit",
            None,
            &credit_sale_body,
            0xd4,
        )
        .await;
        let credit_sale_url = format!("{active_base_url}/v1/tenants/{tenant_a}/branches/{branch_a}/stations/{active_station_id}/sales");
        let credit_sale = client
            .post(&credit_sale_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", &first_end_lease_id)
            .header("x-station-lease-token", &first_end_lease_token)
            .header("idempotency-key", credit_sale_key)
            .header("x-station-challenge-id", &credit_sale_challenge)
            .header("x-station-proof", &credit_sale_proof)
            .body(credit_sale_bytes)
            .send()
            .await
            .unwrap();
        let credit_sale_status = credit_sale.status();
        let credit_sale_response: serde_json::Value = credit_sale.json().await.unwrap();
        assert_eq!(
            credit_sale_status,
            axum::http::StatusCode::OK,
            "on-account sale failed: {credit_sale_response}"
        );
        assert_eq!(credit_sale_response["data"]["onAccountMinor"], 1000);
        let credit_sale_id = credit_sale_response["data"]["id"].as_str().unwrap();
        let invoice_snapshot: (i64, i32, i64) = sqlx::query_as(
            "SELECT original_amount_minor,due_days_snapshot,credit_limit_minor_snapshot FROM hosted_receivable_invoices WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND sale_id=$3::uuid",
        ).bind(&tenant_a).bind(&branch_a).bind(credit_sale_id).fetch_one(&end_seed_pool).await.unwrap();
        assert_eq!(invoice_snapshot, (1000, 14, 1500));
        let ar_debit: i64 = sqlx::query_scalar(
            "SELECT l.debit_minor FROM hosted_accounting_journals j JOIN hosted_accounting_journal_lines l ON l.tenant_id=j.tenant_id AND l.branch_id=j.branch_id AND l.journal_id=j.id WHERE j.tenant_id=$1::uuid AND j.branch_id=$2::uuid AND j.source_type='sale' AND j.source_id=$3::uuid AND l.account_code='1100'",
        ).bind(&tenant_a).bind(&branch_a).bind(credit_sale_id).fetch_one(&end_seed_pool).await.unwrap();
        assert_eq!(ar_debit, 1000);
        let credit_receipt_url = format!("{credit_sale_url}/{credit_sale_id}/receipt");
        let credit_receipt: serde_json::Value = client
            .get(&credit_receipt_url)
            .bearer_auth("opaque-token")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(credit_receipt["data"]["onAccountMinor"], 1000);
        assert!(credit_receipt["data"]["dueAt"].as_str().is_some());

        let excessive_credit_body = serde_json::json!({
            "customerId": customer_id,
            "items": [{"productId": product_id, "quantity": 1}],
            "tenders": [],
            "onAccountMinor": 1000
        });
        let excessive_credit_key = "credit-sale-over-limit-integration-001";
        let (excessive_challenge, excessive_proof, excessive_bytes) = signed_financial_proof(
            &client,
            &credit_sale_challenge_url,
            &tenant_a,
            &branch_a,
            &active_station_id,
            &first_end_lease_id,
            &first_end_lease_token,
            excessive_credit_key,
            "sale.commit",
            None,
            &excessive_credit_body,
            0xd4,
        )
        .await;
        let excessive_credit = client
            .post(&credit_sale_url)
            .bearer_auth("opaque-token")
            .header("content-type", "application/json")
            .header("x-station-lease-id", &first_end_lease_id)
            .header("x-station-lease-token", &first_end_lease_token)
            .header("idempotency-key", excessive_credit_key)
            .header("x-station-challenge-id", &excessive_challenge)
            .header("x-station-proof", &excessive_proof)
            .body(excessive_bytes)
            .send()
            .await
            .unwrap();
        assert_eq!(excessive_credit.status(), axum::http::StatusCode::CONFLICT);
        let credit_test_stock: i64 = sqlx::query_scalar("SELECT stock_quantity FROM branch_catalog_items WHERE tenant_id=$1::uuid AND branch_id=$2::uuid AND product_id=$3::uuid")
            .bind(&tenant_a).bind(&branch_a).bind(&product_id).fetch_one(&end_seed_pool).await.unwrap();
        assert_eq!(credit_test_stock, 4, "excess credit must not consume stock");
        end_seed_pool.close().await;

        api_server.abort();
        api_server_two.abort();
        api_server_three.abort();
        introspection_server.abort();
        introspection_server_two.abort();
        let visible_memberships: i64 = sqlx::query_scalar("SELECT count(*) FROM memberships")
            .fetch_one(&runtime_pool)
            .await
            .unwrap();
        assert_eq!(
            visible_memberships, 0,
            "transaction-local identity context must not leak through a reused pooled connection"
        );
        let visible_memberships_two: i64 = sqlx::query_scalar("SELECT count(*) FROM memberships")
            .fetch_one(&runtime_pool_two)
            .await
            .unwrap();
        assert_eq!(
            visible_memberships_two, 0,
            "transaction-local identity context must not leak through the second pooled connection"
        );
        let visible_memberships_three: i64 = sqlx::query_scalar("SELECT count(*) FROM memberships")
            .fetch_one(&runtime_pool_three)
            .await
            .unwrap();
        assert_eq!(
            visible_memberships_three, 0,
            "transaction-local identity context must not leak through the third pooled connection"
        );
        runtime_pool.close().await;
        runtime_pool_two.close().await;
        runtime_pool_three.close().await;
    }

    #[tokio::test]
    async fn cors_echoes_only_an_explicitly_allowed_origin() {
        let allowed = HeaderValue::from_static("http://localhost:5173");
        let allowed_response = app(vec![allowed])
            .oneshot(
                Request::get("/healthz")
                    .header(header::ORIGIN, "http://localhost:5173")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            allowed_response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(&HeaderValue::from_static("http://localhost:5173"))
        );

        let denied_response = app(vec![HeaderValue::from_static("http://localhost:5173")])
            .oneshot(
                Request::get("/healthz")
                    .header(header::ORIGIN, "https://unlisted.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(denied_response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none());
    }

    #[tokio::test]
    async fn identity_endpoint_fails_closed_when_provider_is_not_configured() {
        let response = app_with_auth(Vec::new(), None)
            .oneshot(
                Request::get("/v1/auth/identity")
                    .header(header::AUTHORIZATION, "Bearer opaque-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn tenant_membership_endpoint_is_not_public_without_authenticated_runtime() {
        let response = app_with_auth(Vec::new(), None)
            .oneshot(
                Request::get("/v1/me/tenants")
                    .header(header::AUTHORIZATION, "Bearer opaque-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value.get("data").is_none());
        assert_eq!(value["error"]["code"], "identity_database_unavailable");
    }

    #[tokio::test]
    async fn station_permissions_endpoint_fails_closed_without_authenticated_runtime() {
        let response = app_with_auth(Vec::new(), None)
            .oneshot(
                Request::get("/v1/tenants/tenant/branches/branch/stations/station/permissions")
                    .header(header::AUTHORIZATION, "Bearer opaque-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value.get("data").is_none());
        assert_eq!(value["error"]["code"], "identity_database_unavailable");
    }

    #[tokio::test]
    async fn branch_permissions_endpoint_fails_closed_without_authenticated_runtime() {
        let response = app_with_auth(Vec::new(), None)
            .oneshot(
                Request::get("/v1/tenants/tenant/branches/branch/permissions")
                    .header(header::AUTHORIZATION, "Bearer opaque-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value.get("data").is_none());
        assert_eq!(value["error"]["code"], "identity_database_unavailable");
    }

    #[test]
    fn branch_permission_response_contains_scope_without_identity_data() {
        let value = serde_json::to_value(BranchPermissionSnapshot {
            tenant_id: "tenant-1".to_owned(),
            branch_id: "branch-1".to_owned(),
            permissions: vec!["reports:read", "products:read"],
            tenant_permissions: vec![],
        })
        .unwrap();

        assert_eq!(value["tenantId"], "tenant-1");
        assert_eq!(value["branchId"], "branch-1");
        assert_eq!(value["permissions"][0], "reports:read");
        assert!(value.get("identityId").is_none());
    }

    #[test]
    fn station_permission_response_exposes_each_scope_without_identity_data() {
        let value = serde_json::to_value(StationPermissionSnapshot {
            tenant_id: "tenant-1".to_owned(),
            branch_id: "branch-1".to_owned(),
            station_id: "station-1".to_owned(),
            permissions: vec!["pos:use", "products:read"],
            tenant_permissions: vec![],
            branch_permissions: vec![],
        })
        .unwrap();

        assert_eq!(value["tenantId"], "tenant-1");
        assert_eq!(value["permissions"][0], "pos:use");
        assert!(value["tenantPermissions"].as_array().unwrap().is_empty());
        assert!(value["branchPermissions"].as_array().unwrap().is_empty());
        assert!(value.get("identityId").is_none());
    }

    #[tokio::test]
    async fn identity_mapping_returns_only_active_internal_account_or_fail_closed_status() {
        let active = active_identity_lookup(Ok(Some(InternalIdentity {
            identity_id: "person-123".to_owned(),
            status: IdentityStatus::Active,
            display_name: None,
        })))
        .unwrap();
        assert_eq!(active.identity_id, "person-123");

        let missing = active_identity_lookup(Ok(None)).unwrap_err();
        assert_eq!(missing.status(), axum::http::StatusCode::NOT_FOUND);

        for status in [IdentityStatus::Suspended, IdentityStatus::Removed] {
            let inactive = active_identity_lookup(Ok(Some(InternalIdentity {
                identity_id: "person-123".to_owned(),
                status,
                display_name: None,
            })))
            .unwrap_err();
            assert_eq!(inactive.status(), axum::http::StatusCode::FORBIDDEN);
        }

        let unavailable =
            active_identity_lookup(Err(IdentityLookupError::InvalidStatus)).unwrap_err();
        assert_eq!(
            unavailable.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
