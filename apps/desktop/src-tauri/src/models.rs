// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapStatus {
    pub needs_setup: bool,
    pub business_name: Option<String>,
    pub store_name: Option<String>,
    pub currency_code: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitialSetupInput {
    pub business_name: String,
    pub store_name: String,
    pub full_name: String,
    pub username: String,
    pub password: String,
    pub currency_code: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginInput {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUser {
    pub user_id: String,
    pub business_id: String,
    pub store_id: String,
    pub full_name: String,
    pub username: String,
    pub role_key: String,
    pub role_name: String,
    pub workspace_mode: String,
    pub is_owner: bool,
    pub permissions: Vec<String>,
    pub business_name: String,
    pub store_name: String,
    pub currency_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorePreferences {
    pub cashier_start_at_register: bool,
    pub cashier_auto_fullscreen: bool,
    pub customer_display_enabled: bool,
    pub customer_display_fullscreen: bool,
    pub customer_display_monitor_key: Option<String>,
    pub customer_display_idle_message: String,
    pub receipt_header_text: String,
    pub receipt_footer_text: String,
    pub receipt_paper_width_mm: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorePreferencesInput {
    pub cashier_start_at_register: bool,
    pub cashier_auto_fullscreen: bool,
    pub customer_display_enabled: bool,
    pub customer_display_fullscreen: bool,
    pub customer_display_monitor_key: Option<String>,
    pub customer_display_idle_message: String,
    pub receipt_header_text: String,
    pub receipt_footer_text: String,
    pub receipt_paper_width_mm: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    pub id: String,
    pub sku: String,
    pub barcode: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub category_id: Option<String>,
    pub category_name: Option<String>,
    pub brand_id: Option<String>,
    pub brand_name: Option<String>,
    pub tag_ids: Vec<String>,
    pub tags: Vec<String>,
    pub supplier_id: Option<String>,
    pub price_minor: i64,
    pub cost_minor: Option<i64>,
    pub stock_quantity: i64,
    pub reorder_point: i64,
    pub target_stock_quantity: i64,
    pub is_active: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductInput {
    pub id: Option<String>,
    pub sku: String,
    pub barcode: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub category_id: Option<String>,
    pub brand_id: Option<String>,
    #[serde(default)]
    pub tag_ids: Vec<String>,
    pub supplier_id: Option<String>,
    pub price_minor: i64,
    pub cost_minor: Option<i64>,
    pub stock_quantity: i64,
    pub reorder_point: i64,
    #[serde(default)]
    pub target_stock_quantity: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryInput {
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Customer {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerInput {
    pub id: Option<String>,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Supplier {
    pub id: String,
    pub name: String,
    pub contact_person: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupplierInput {
    pub id: Option<String>,
    pub name: String,
    pub contact_person: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StockAdjustmentInput {
    pub product_id: String,
    pub quantity_delta: i64,
    pub note: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutInput {
    #[serde(default)]
    pub payment_option_id: Option<String>,
    #[serde(default)]
    pub payment_reference: String,
    pub customer_id: Option<String>,
    pub lines: Vec<CheckoutLineInput>,
    pub discount_minor: i64,
    pub payment_method: PaymentMethod,
    pub amount_tendered_minor: i64,
    pub ticket_id: Option<String>,
    pub workspace_revision: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterWorkspaceView {
    pub schema_version: u16,
    pub revision: i64,
    pub active: RegisterTicket,
    pub held: Vec<RegisterTicket>,
    pub last_completed: Option<CompletedRegisterSale>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterTicket {
    #[serde(default)]
    pub payment_option_id: Option<String>,
    #[serde(default)]
    pub payment_reference: String,
    pub id: String,
    pub ticket_number: String,
    pub creator_user_id: String,
    pub creator_name: String,
    pub last_editor_user_id: String,
    pub last_editor_name: String,
    pub customer_id: Option<String>,
    pub lines: Vec<RegisterTicketLine>,
    pub discount_minor: i64,
    #[serde(default)]
    pub discount_reason: Option<String>,
    #[serde(default)]
    pub discount_requested_by_user_id: Option<String>,
    #[serde(default)]
    pub discount_approval_id: Option<String>,
    #[serde(default)]
    pub discount_approver_name: Option<String>,
    #[serde(default)]
    pub discount_approval_expires_at: Option<String>,
    pub payment_method: PaymentMethod,
    pub amount_tendered_minor: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterTicketLine {
    pub product_id: String,
    pub quantity: i64,
    pub accepted_name: String,
    pub accepted_sku: String,
    pub accepted_unit_price_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletedRegisterSale {
    pub ticket_id: String,
    pub customer_id: Option<String>,
    pub receipt: SaleReceipt,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterEditInput {
    pub expected_revision: i64,
    pub action: RegisterAction,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RegisterAction {
    AddProduct {
        product_id: String,
    },
    SetQuantity {
        product_id: String,
        quantity: i64,
    },
    RemoveLines {
        product_ids: Vec<String>,
    },
    ClearActive,
    SetDiscount {
        discount_minor: i64,
        #[serde(default)]
        reason: Option<String>,
    },
    SetPayment {
        #[serde(default)]
        payment_option_id: Option<String>,
        #[serde(default)]
        payment_reference: String,
        payment_method: PaymentMethod,
        amount_tendered_minor: i64,
    },
    SelectCustomer {
        customer_id: Option<String>,
    },
    SetPaymentReference {
        reference: String,
    },
    HoldActive,
    ResumeHeld {
        ticket_id: String,
        hold_active: bool,
    },
    DiscardHeld {
        ticket_id: String,
    },
    AcceptCurrentPrices,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveTicketDiscountInput {
    pub ticket_id: String,
    pub expected_workspace_revision: i64,
    pub expected_discount_minor: i64,
    pub approver_user_id: String,
    pub credential: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscountApprover {
    pub user_id: String,
    pub full_name: String,
    pub role_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscountPreviewInput {
    pub ticket_id: String,
    pub expected_workspace_revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscountQuoteLine {
    pub name: String,
    pub quantity: i64,
    pub unit_price_minor: i64,
    pub line_total_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscountPreview {
    pub ticket_id: String,
    pub workspace_revision: i64,
    pub currency_code: String,
    pub reason: String,
    pub lines: Vec<DiscountQuoteLine>,
    pub subtotal_minor: i64,
    pub discount_minor: i64,
    pub tax_minor: i64,
    pub total_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaffProfile {
    pub user_id: String,
    pub full_name: String,
    pub role_key: String,
    pub role_name: String,
    pub has_pin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaffAccount {
    pub user_id: String,
    pub full_name: String,
    pub username: String,
    pub role_key: String,
    pub role_name: String,
    pub has_pin: bool,
    pub is_active: bool,
    pub is_owner: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaffRole {
    pub role_key: String,
    pub display_name: String,
    pub permissions: Vec<String>,
    pub workspace_mode: String,
    pub is_system: bool,
    pub revision: i64,
    pub active_user_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionDefinition {
    pub key: String,
    pub label: String,
    pub group: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaffRolesView {
    pub roles: Vec<StaffRole>,
    pub permissions: Vec<PermissionDefinition>,
    pub owner_user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveStaffRoleInput {
    pub role_key: Option<String>,
    pub template_key: String,
    pub display_name: String,
    pub permissions: Vec<String>,
    pub workspace_mode: String,
    pub expected_revision: Option<i64>,
    pub password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimBusinessOwnerInput {
    pub password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaffAccountStatusInput {
    pub user_id: String,
    pub is_active: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaffAccountRoleInput {
    pub user_id: String,
    pub role_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateStaffMemberInput {
    pub full_name: String,
    pub username: String,
    pub password: String,
    pub pin: String,
    pub role_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchStaffProfileInput {
    pub user_id: String,
    pub credential: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryCashierModeInput {
    pub enabled: bool,
    pub credential: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachSaleCustomerInput {
    pub sale_id: String,
    pub customer_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutLineInput {
    pub product_id: String,
    pub quantity: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentMethod {
    Cash,
    Card,
    MobileMoney,
    Other,
}

impl PaymentMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cash => "cash",
            Self::Card => "card",
            Self::MobileMoney => "mobile_money",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleReceipt {
    #[serde(default)]
    pub payment_detail: Option<crate::payment_options::PaymentDetail>,
    pub id: String,
    pub receipt_number: String,
    pub created_at: String,
    pub cashier_name: String,
    pub items: Vec<SaleReceiptItem>,
    pub totals: pos_core::SaleTotals,
    pub payment_method: PaymentMethod,
    pub amount_tendered_minor: i64,
    pub change_due_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleReceiptItem {
    pub product_id: String,
    pub sku: String,
    pub name: String,
    pub quantity: i64,
    pub unit_price_minor: i64,
    pub line_total_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleSummary {
    pub id: String,
    pub receipt_number: String,
    pub created_at: String,
    pub cashier_name: String,
    pub item_count: i64,
    pub total_minor: i64,
    pub status: String,
    pub payment_method: String,
    pub station_key: Option<String>,
    pub station_name_snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSummary {
    pub today_sales_minor: i64,
    pub today_sales_count: i64,
    pub low_stock_count: i64,
    pub product_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEventSummary {
    pub id: String,
    pub action: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub actor_name: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEventQuery {
    pub search: Option<String>,
    pub offset: i64,
    pub page_size: i64,
    pub snapshot_row_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEventPage {
    pub rows: Vec<AuditEventSummary>,
    pub matched_count: i64,
    pub offset: i64,
    pub page_size: i64,
    pub snapshot_row_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Expense {
    pub id: String,
    pub category: String,
    pub description: Option<String>,
    pub amount_minor: i64,
    pub payment_method: String,
    pub cash_origin: String,
    pub cash_source_note: Option<String>,
    pub cost_center_kind: String,
    pub cost_center_station_key: Option<String>,
    pub cost_center_station_name_snapshot: Option<String>,
    pub occurred_at: String,
    pub created_at: String,
    pub recorded_by: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseInput {
    pub category: String,
    pub description: Option<String>,
    pub amount_minor: i64,
    pub payment_method: String,
    pub cash_origin: String,
    pub cash_source_note: Option<String>,
    pub cash_location_id: Option<String>,
    pub cost_center_kind: String,
    pub cost_center_station_key: Option<String>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseReportRow {
    pub id: String,
    pub expense_id: String,
    pub entry_type: String,
    pub category: String,
    pub description: Option<String>,
    /// Positive for a recorded expense and negative for an appended reimbursement.
    pub amount_minor: i64,
    pub payment_method: String,
    pub cash_origin: String,
    pub cash_source_note: Option<String>,
    pub cost_center_kind: String,
    pub cost_center_station_key: Option<String>,
    pub cost_center_station_name_snapshot: Option<String>,
    pub cost_center_revision: i64,
    pub occurred_at: String,
    pub created_at: String,
    pub recorded_by: String,
    pub correction_reason: Option<String>,
    pub external_reference: Option<String>,
    pub can_reverse: bool,
    pub remaining_minor: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseReimbursementInput {
    pub expense_id: String,
    pub amount_minor: i64,
    pub return_method: String,
    pub return_reference: Option<String>,
    pub reason: String,
    pub cash_location_id: Option<String>,
    pub cash_shift_id: Option<String>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseReimbursement {
    pub id: String,
    pub expense_id: String,
    pub amount_minor: i64,
    pub return_method: String,
    pub return_reference: Option<String>,
    pub reason: String,
    pub cash_location_id: Option<String>,
    pub cash_shift_id: Option<String>,
    pub returned_at: String,
    pub recorded_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseSummary {
    pub id: String,
    pub reference: Option<String>,
    pub supplier_name: Option<String>,
    pub received_at: String,
    pub total_minor: i64,
    pub item_count: i64,
    pub items_summary: String,
    pub recorded_by: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseInput {
    #[serde(default)]
    pub purchase_order_id: Option<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    pub supplier_id: Option<String>,
    pub reference: Option<String>,
    pub lines: Vec<PurchaseLineInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseLineInput {
    pub product_id: String,
    pub quantity: i64,
    pub unit_cost_minor: i64,
    #[serde(default)]
    pub purchase_order_line_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOrderLine {
    pub id: String,
    pub product_id: String,
    pub product_name: String,
    pub sku: String,
    pub ordered_quantity: i64,
    pub received_quantity: i64,
    pub expected_unit_cost_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOrderSummary {
    pub id: String,
    pub order_number: String,
    pub supplier_id: Option<String>,
    pub supplier_name: Option<String>,
    pub reference: Option<String>,
    pub status: String,
    pub revision: i64,
    pub placed_at: Option<String>,
    pub updated_at: String,
    pub total_estimated_minor: i64,
    pub lines: Vec<PurchaseOrderLine>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOrderInput {
    pub id: Option<String>,
    pub revision: Option<i64>,
    pub supplier_id: Option<String>,
    pub reference: Option<String>,
    pub lines: Vec<PurchaseOrderLineInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOrderLineInput {
    pub product_id: String,
    pub quantity: i64,
    pub expected_unit_cost_minor: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOrderTransitionInput {
    pub id: String,
    pub revision: i64,
}
