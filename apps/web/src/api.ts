// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { sha256Hex, signStationMessage } from './stationCrypto'

export type StationAccess = { stationId: string; code: string; name: string }
export type BranchAccess = {
  branchId: string
  code: string
  name: string
  timezone: string
  currencyCode: string
  stations: StationAccess[]
}
export type BranchOpeningInterval = { open: string; close: string }
export type BranchOpeningHours = Record<'monday' | 'tuesday' | 'wednesday' | 'thursday' | 'friday' | 'saturday' | 'sunday', BranchOpeningInterval[]>
export type HostedBranchProfile = {
  address: string | null
  phone: string | null
  email: string | null
  openingHours: BranchOpeningHours
  settingsRevision: number
}
export type TenantMembership = {
  tenantId: string
  displayName: string
  membershipId: string
  roleGrants: { roleKey: string; branchId: string | null; stationId: string | null }[]
  branches: BranchAccess[]
}
export type StationPermissionSnapshot = {
  tenantId: string
  branchId: string
  stationId: string
  permissions: string[]
  tenantPermissions: string[]
  branchPermissions: string[]
}
export type StationCapacitySnapshot = {
  entitlementActive: boolean
  stationLimit: number | null
  activeStations: number
  availableSlots: number
}
export type BranchPermissionSnapshot = {
  tenantId: string
  branchId: string
  permissions: string[]
  tenantPermissions: string[]
}
export type BranchProduct = {
  productId: string
  identityRevision: number
  sku: string
  barcode: string | null
  name: string
  description: string | null
  category: string | null
  priceMinor: number
  taxRateBps: number
  stockQuantity: number
  reorderPoint: number
  targetStockQuantity: number
  revision: number
  currencyCode: string
  isActive: boolean
}
export type HostedImageEntityType = 'product' | 'customer' | 'supplier' | 'staff'
export type HostedStaffMember = { membershipId: string; displayName: string | null; roles: string[] }
export type HostedAttendanceStaff = HostedStaffMember
export type HostedWorkSchedule = { membershipId: string; branchId: string; timezone: string; weekdays: number[]; startTime: string; endTime: string; enabled: boolean; revision: number }
export type HostedWorkPeriod = { id: string; branchId: string; membershipId: string; scheduleRevision: number | null; timezone: string; scheduledStartAt: string; dueAt: string; startedAt: string; endedAt: string | null; endReason: string | null; revision: number }
export type HostedMyAttendance = { serverNow: string; schedule: HostedWorkSchedule | null; currentPeriod: HostedWorkPeriod | null }
export type NewBranchProduct = {
  sku: string
  barcode: string | null
  name: string
  description: string | null
  category: string | null
  priceMinor: number
  costMinor: number | null
  taxRateBps: number
  openingStock: number
  reorderPoint: number
  targetStockQuantity: number
}
export type CatalogProductIdentity = Pick<BranchProduct, 'productId' | 'identityRevision' | 'sku' | 'barcode' | 'name' | 'description' | 'category'>
export type AttachBranchProduct = Pick<NewBranchProduct, 'priceMinor' | 'costMinor' | 'taxRateBps' | 'openingStock' | 'reorderPoint' | 'targetStockQuantity'>
export type ProductBranchAvailability = {
  branchId: string
  branchCode: string
  branchName: string
  address: string | null
  phone: string | null
  email: string | null
  timeZone: string
  openingHours: BranchOpeningHours
  stockQuantity: number
  pickupReservedQuantity: number
  outboundCommittedQuantity: number
  availableToPromise: number
  commitmentShortfall: number
  priceMinor: number
  currencyCode: string
  observedAt: string
}
export type HostedStockTransfer = {
  transferId: string; sourceBranchId: string; destinationBranchId: string; status: string; revision: number
  requestNote: string | null; requestedAt: string; items: Array<{ productId: string; sku: string; name: string; requestedQuantity: number; acceptedQuantity: number | null; dispatchedQuantity: number | null; receivedQuantity: number | null; discrepancyNote: string | null }>
}
export type HostedPickupReservation = {
  reservationId: string; originBranchId: string; originStationId: string; pickupBranchId: string
  requestedByMembershipId: string; customerId: string | null; contactName: string; contactPhone: string
  status: 'held' | 'ready' | 'collected' | 'cancelled' | 'expired'; revision: number
  expiresAt: string; createdAt: string; replayed: boolean
  items: Array<{ productId: string; sku: string; name: string; quantity: number }>
}
export type NewHostedPickupReservation = {
  pickupBranchId: string; customerId?: string | null; contactName: string; contactPhone: string
  contactConsent: true; holdMinutes?: number; items: Array<{ productId: string; quantity: number }>
}
export type TenderKind = 'cash' | 'card' | 'mobile_money' | 'bank_transfer' | 'other'
export type BranchPaymentOption = {
  id: string
  methodKey: string
  label: string
  tenderKind: TenderKind
  providerName: string | null
  countryCode: string | null
  requiresReference: boolean
  enabled: boolean
  recordingMode: 'manual'
  revision: number
}
export type NewBranchPaymentOption = {
  methodKey: string
  label: string
  tenderKind: TenderKind
  providerName: string | null
  countryCode: string | null
  requiresReference: boolean
}
export type PaymentIntegrationRequestStatus = 'submitted' | 'under_review' | 'planned' | 'in_progress' | 'released' | 'not_planned'
export type PaymentIntegrationRequest = {
  id: string
  branchId: string
  countryCode: string
  providerName: string
  terminalModel: string | null
  capability: string
  businessImpact: 'business_blocked' | 'workaround_available' | 'exploring'
  details: string | null
  status: PaymentIntegrationRequestStatus
  customerVisibleUpdate: string | null
  revision: number
  createdAt: string
  updatedAt: string
}
export type NewPaymentIntegrationRequest = Omit<PaymentIntegrationRequest, 'id' | 'branchId' | 'status' | 'customerVisibleUpdate' | 'revision' | 'createdAt' | 'updatedAt'>
export type SupportRequestCategory = 'bug' | 'feature_request' | 'account_access' | 'data_issue' | 'training' | 'other'
export type SupportRequestPlatform = 'web' | 'windows' | 'macos' | 'linux' | 'android' | 'ios' | 'harmonyos'
export type SupportRequestStatus = 'submitted' | 'under_review' | 'planned' | 'in_progress' | 'released' | 'not_planned'
export type SupportRequest = {
  id: string
  branchId: string
  category: SupportRequestCategory
  title: string
  details: string | null
  diagnosticsConsent: boolean
  diagnosticsAppVersion: string | null
  diagnosticsPlatform: SupportRequestPlatform | null
  status: SupportRequestStatus
  customerVisibleUpdate: string | null
  revision: number
  createdAt: string
  updatedAt: string
}
export type NewSupportRequest = Pick<SupportRequest, 'category' | 'title' | 'details'> & Partial<Pick<SupportRequest, 'diagnosticsConsent' | 'diagnosticsAppVersion' | 'diagnosticsPlatform'>>

export class ApiError extends Error {
  readonly status: number

  constructor(message: string, status: number) {
    super(message)
    this.status = status
    this.name = 'ApiError'
  }
}

export type HostedStationKey = { privateKey: CryptoKey; publicKeyHex: string }
export type HostedLease = { leaseId: string; token: string; expiresAt: string }
export type HostedDrawerReport = {
  shiftId: string
  stationId: string
  openedByMembershipId: string
  openedAt: string
  closedAt: string | null
  openingCashMinor: number
  cashSalesMinor: number
  paidInMinor: number
  paidOutMinor: number
  cashDropsMinor: number
  expectedCashMinor: number
  countedCashMinor: number | null
  differenceMinor: number | null
  movementCount: number
  saleCount: number
  isClosed: boolean
}
export type HostedSale = {
  id: string
  customerId: string | null
  receiptNumber: string
  cashierName: string
  stationName: string
  currencyCode: string
  subtotalMinor: number
  taxMinor: number
  totalMinor: number
  onAccountMinor: number
  createdAt: string
  replayed: boolean
}
export type HostedHeldTicketPayload = {
  label: string
  lines: Array<{ productId: string; quantity: number }>
  selectedCustomerId: string
  selectedPaymentId: string
  reference: string
  pickupSale: { reservationId: string; expectedRevision: number; items: Array<{ productId: string; quantity: number }> } | null
}
export type HostedHeldTicket = {
  id: string
  revision: number
  status: 'held' | 'checkout_pending' | 'completed' | 'discarded'
  payload: HostedHeldTicketPayload
  checkoutBody: Record<string, unknown> | null
  checkoutKey: string | null
  claimLeaseId: string | null
  completedSaleId: string | null
  createdAt: string
  updatedAt: string
}
export type HostedSaleRefund = { id: string; saleId: string; receiptNumber: string; kind: 'refund' | 'void'; totalMinor: number; reason: string; paymentMethod: 'cash' | 'card' | 'mobile_money' | 'bank_transfer' | 'other'; paymentReference: string | null; createdAt: string; cashierName: string; stationName: string }
export type HostedRefundableItem = { saleLineId: string; productId: string; sku: string; name: string; soldQuantity: number; refundedQuantity: number; remainingQuantity: number; subtotalMinor: number; taxMinor: number; refundableTotalMinor: number }
export type HostedSaleRefundDetail = { saleId: string; receiptNumber: string; cashierName: string; status: string; currencyCode: string; totalMinor: number; onAccountMinor: number; items: HostedRefundableItem[]; refunds: HostedSaleRefund[] }
export type HostedSaleRefundInput = { kind: 'refund' | 'void'; reason: string; items: Array<{ saleLineId: string; quantity: number; restock: boolean }>; paymentMethod: HostedSaleRefund['paymentMethod']; paymentReference: string | null }
export type AttachedSaleCustomer = { saleId: string; customerId: string; customerName: string | null }
export type HostedCustomer = { id: string; name: string; phone: string | null; email: string | null; notes: string | null; createdAt: string; status: 'active' | 'archived' | 'merged'; revision: number; mergedIntoCustomerId: string | null }
export type HostedCustomerCreditPosition = {
  currencyCode: string
  hasCreditTerms: boolean
  termsRevision: number | null
  creditLimitMinor: string
  dueDays: number
  outstandingMinor: string
  availableMinor: string
}
export type HostedCustomerCreditTerm = {
  currencyCode: string
  creditLimitMinor: string
  dueDays: number
  revision: number
  reason: string
  updatedAt: string
}
export type HostedCustomerCreditTermEvent = {
  currencyCode: string
  revision: number
  previousCreditLimitMinor: string | null
  previousDueDays: number | null
  creditLimitMinor: string
  dueDays: number
  reason: string
  actorName: string
  createdAt: string
}
export type HostedOpenReceivableInvoice = {
  invoiceBranchId: string
  invoiceId: string
  issuedAt: string
  dueAt: string
  originalAmountMinor: string
  allocatedMinor: string
  openMinor: string
}
export type HostedCashCollection = { id: string; customerId: string; currencyCode: string; amountMinor: number; receivedAt: string; allocationCount: number }
export type HostedSupplier = { id: string; name: string; contactPerson: string | null; email: string | null; phone: string | null; address: string | null; notes: string | null; createdAt: string; status: 'active' | 'archived'; revision: number }
export type HostedPurchaseOrderLine = { id: string; productId: string; sku: string; name: string; orderedQuantity: number; receivedQuantity: number; expectedUnitCostMinor: number }
export type HostedPurchaseReceipt = { id: string; reference: string | null; totalMinor: number; receivedAt: string }
export type HostedPurchaseOrder = { id: string; orderNumber: string; supplierId: string | null; supplierName: string | null; reference: string | null; status: 'draft' | 'placed' | 'partially_received' | 'received' | 'cancelled'; revision: number; createdAt: string; placedAt: string | null; items: HostedPurchaseOrderLine[]; receipts: HostedPurchaseReceipt[] }
export type HostedPurchaseOrderItemInput = { productId: string; quantity: number; expectedUnitCostMinor: number }
export type HostedPurchaseReceiptItemInput = { orderLineId: string; quantity: number; actualUnitCostMinor: number }
export type HostedUnbilledReceipt = { receiptId: string; supplierId: string; supplierName: string; supplierReference: string | null; purchaseOrderId: string; receivedAt: string; totalMinor: number; billedMinor: number; availableMinor: number }
export type HostedSupplierBill = { id: string; supplierId: string; supplierName: string; supplierReference: string; invoiceDate: string; dueDate: string | null; currencyCode: string; totalMinor: number; paidMinor: number; creditedMinor: number; balanceMinor: number; createdAt: string }
export type HostedSupplierPayment = { id: string; supplierId: string; supplierName: string; paymentMethod: 'cash' | 'card' | 'mobile_money' | 'bank_transfer' | 'other'; cashLocationId: string | null; externalReference: string | null; amountMinor: number; currencyCode: string; paidAt: string }
export type HostedSupplierCreditNote = { id: string; billId: string; supplierId: string; supplierName: string; creditReference: string; effectiveDate: string; reason: string; currencyCode: string; totalMinor: number; createdAt: string }
export type HostedSupplierReturnCandidate = { billId: string; receiptId: string; receiptReference: string | null; receiptLineId: string; productId: string; sku: string; productName: string; unitCostMinor: number; maxQuantity: number }
export type HostedSupplierBillingWorkspace = { currencyCode: string; unbilledReceipts: HostedUnbilledReceipt[]; bills: HostedSupplierBill[]; payments: HostedSupplierPayment[]; creditNotes: HostedSupplierCreditNote[]; returnableItems: HostedSupplierReturnCandidate[] }
export type HostedExpenseCategory = 'rent' | 'utilities' | 'wages' | 'transport' | 'supplies' | 'maintenance' | 'marketing' | 'bank_fees' | 'taxes_fees' | 'other'
export type HostedExpensePaymentMethod = Exclude<HostedSupplierPayment['paymentMethod'], 'cash'>
export type HostedCostCenterKind = 'station_direct' | 'store_overhead' | 'legacy_unassigned'
export type HostedExpenseCostCenterReclassification = { id: string; expenseId: string; revision: number; previousKind: HostedCostCenterKind; previousStationId: string | null; previousStationNameSnapshot: string | null; nextKind: HostedCostCenterKind; nextStationId: string | null; nextStationNameSnapshot: string | null; reason: string; recordedAt: string; actorName: string }
export type HostedExpenseCostCenterHistory = { currentRevision: number; currentKind: HostedCostCenterKind; currentStationId: string | null; currentStationNameSnapshot: string | null; rows: HostedExpenseCostCenterReclassification[] }
export type HostedExpense = { id: string; categoryCode: HostedExpenseCategory; description: string; payee: string | null; amountMinor: number; reimbursedMinor: number; remainingReimbursableMinor: number; paymentMethod: HostedExpensePaymentMethod | 'cash'; drawerShiftId: string | null; cashLocationId: string | null; externalReference: string | null; occurredOn: string; currencyCode: string; costCenterKind: HostedCostCenterKind; costCenterStationId: string | null; costCenterStationNameSnapshot: string | null; costCenterRevision: number; recordedBy: string; createdAt: string }
export type HostedExpenseWorkspace = { currencyCode: string; expenses: HostedExpense[]; offset: number; pageSize: number; hasMore: boolean }
export type HostedExpenseReimbursement = { id: string; expenseId: string; amountMinor: number; returnMethod: HostedSupplierPayment['paymentMethod']; cashLocationId: string | null; externalReference: string | null; reason: string; returnedOn: string; recordedBy: string; createdAt: string }
export type HostedExpenseReimbursementWorkspace = { expenseId: string; amountMinor: number; reimbursedMinor: number; remainingMinor: number; reimbursements: HostedExpenseReimbursement[] }
export type HostedCashLocation = { id: string; name: string; locationType: 'register' | 'safe' | 'petty_cash' | 'other'; stationId: string | null; status: 'active' | 'inactive'; currencyCode: string; balanceMinor: number; initialized: boolean }
export type HostedCashLocationEvent = { id: string; eventType: 'opening' | 'transfer_out' | 'transfer_in' | 'expense_outflow' | 'supplier_payment_outflow' | 'expense_reimbursement_inflow' | 'sale_tender_inflow' | 'sale_refund_outflow' | 'register_movement_inflow' | 'register_movement_outflow' | 'cash_drop_inflow'; signedAmountMinor: number; sourceTransferId: string | null; sourceExpenseId: string | null; sourceSupplierPaymentId: string | null; sourceExpenseReimbursementId: string | null; sourceSaleTenderId: string | null; sourceRefundTenderId: string | null; sourceDrawerMovementId: string | null; actorName: string; createdAt: string }
export type HostedCashDropDestination = { id: string; name: string }
export type HostedCashLocationWorkspace = { locations: HostedCashLocation[] }
export type HostedCashReconciliation = { id: string; currencyCode: string; trackedCashMinor: number; ledgerCashBeforeMinor: number; adjustmentMinor: number; reason: string; createdAt: string }
export type HostedCashReconciliationStatus = {
  currencyCode: string; activeLocationCount: number; trackedCashMinor: number; ledgerCashMinor: number; varianceMinor: number
  uninitializedLocationCount: number; openShiftCount: number; unresolvedDrawerMovementCount: number
  latest: HostedCashReconciliation | null
  status: 'cutover_required' | 'incomplete_locations' | 'open_shifts' | 'unresolved_drawer_movements' | 'reconciled' | 'variance'
}
export type HostedExpenseInput = { categoryCode: HostedExpenseCategory; description: string; payee: string | null; amountMinor: number; paymentMethod: HostedExpense['paymentMethod']; cashLocationId: string | null; externalReference: string | null; occurredOn: string }
export type HostedSupplierBillAllocationInput = { receiptId: string; amountMinor: number }
export type HostedSupplierPaymentAllocationInput = { billId: string; amountMinor: number }
export type HostedSupplierCreditLineInput = { receiptId: string; receiptLineId: string; quantity: number }
export type HostedSalesReportRow = {
  id: string
  receiptNumber: string
  cashierName: string
  stationName: string
  currencyCode: string
  subtotalMinor: number
  taxMinor: number
  totalMinor: number
  paymentMethods: string
  createdAt: string
}
export type HostedSalesReport = {
  rows: HostedSalesReportRow[]
  offset: number
  pageSize: number
  matchedCount: number
  totalsByCurrency: Array<{
    currencyCode: string
    saleCount: number
    subtotalMinor: string
    taxMinor: string
    totalMinor: string
  }>
}
export type HostedAccountingCoverageException = {
  sourceType: 'sale' | 'refund' | 'partial_sale' | 'partial_refund'
  sourceId: string
  label: string
  occurredAt: string
  issueCode: HostedAccountingCoverageIssueCode
  amountMinor: number
  currencyCode: string
}
export type HostedAccountingCoverageIssueCode =
  | 'sale_lines_do_not_reconcile'
  | 'sale_tenders_missing'
  | 'sale_tenders_do_not_reconcile'
  | 'sale_journal_missing'
  | 'refund_lines_do_not_reconcile'
  | 'refund_tenders_missing'
  | 'refund_tenders_do_not_reconcile'
  | 'refund_journal_missing'
  | 'historical_cost_or_tax_classification_unavailable'
export type HostedBranchAccountingCoverage = {
  salesCount: number
  unpostedSalesCount: number
  refundsCount: number
  unpostedRefundsCount: number
  partialJournalCount: number
  exceptions: HostedAccountingCoverageException[]
  exceptionsTruncated: boolean
}
export type HostedTrialBalanceRow = {
  currencyCode: string
  accountCode: string
  accountName: string
  accountType: string
  debitMinor: string
  creditMinor: string
  balanceMinor: string
  journalCount: number
}
export type HostedTrialBalance = {
  rows: HostedTrialBalanceRow[]
  currencies: { currencyCode: string; totalDebitsMinor: string; totalCreditsMinor: string; isBalanced: boolean }[]
  salesRefundCoverageComplete: boolean
  coverage: HostedBranchAccountingCoverage
}
export type HostedIncomeStatementCurrency = {
  currencyCode: string
  grossSalesMinor: string
  salesReturnsMinor: string
  netSalesMinor: string
  costOfGoodsSoldMinor: string
  grossProfitMinor: string
  operatingExpensesMinor: string
  operatingIncomeMinor: string
}
export type HostedIncomeStatement = {
  currencies: HostedIncomeStatementCurrency[]
  coverage: HostedBranchAccountingCoverage
  salesRefundCoverageComplete: boolean
}
export type HostedBalanceSheet = {
  asOfDate: string
  currencies: {
    currencyCode: string
    assetsMinor: string
    liabilitiesMinor: string
    contributedEquityMinor: string
    retainedEarningsMinor: string
    totalEquityMinor: string
    liabilitiesAndEquityMinor: string
    differenceMinor: string
    isBalanced: boolean
    accounts: { accountCode: string; accountName: string; accountType: string; balanceMinor: string }[]
  }[]
  coverage: HostedBranchAccountingCoverage
  salesRefundCoverageComplete: boolean
  complete: boolean
}
export type HostedCashFlowStatement = {
  fromDate: string
  toDate: string
  currencies: {
    currencyCode: string
    openingCashMinor: string
    operatingInflowsMinor: string
    operatingOutflowsMinor: string
    netOperatingCashFlowMinor: string
    cutoverAdjustmentMinor: string
    unclassifiedJournalCashMinor: string
    netChangeMinor: string
    endingCashMinor: string
    reconciliationDifferenceMinor: string
    isReconciled: boolean
    clearingAccounts: { accountCode: string; accountName: string; openingBalanceMinor: string; periodChangeMinor: string; endingBalanceMinor: string }[]
    unclassifiedRegisterMovementCount: number
    unclassifiedRegisterMovementNetMinor: string
  }[]
  coverage: HostedBranchAccountingCoverage
  salesRefundCoverageComplete: boolean
  cashReconciliationStatus: string
  cashLocationVarianceMinor: string
  unresolvedDrawerMovementCount: number
  complete: boolean
  limitations: string[]
}
export type HostedStationSalesHistoryRow = {
  id: string
  receiptNumber: string
  cashierName: string
  customerName: string | null
  currencyCode: string
  totalMinor: number
  createdAt: string
}
export type HostedStationSalesHistory = {
  rows: HostedStationSalesHistoryRow[]
  offset: number
  pageSize: number
  matchedCount: number
}
export type HostedReceipt = {
  id: string
  customerId: string | null
  customerName: string | null
  receiptNumber: string
  cashierName: string
  stationName: string
  currencyCode: string
  subtotalMinor: number
  taxMinor: number
  totalMinor: number
  onAccountMinor: number
  dueAt: string | null
  createdAt: string
  lines: Array<{ lineNumber: number; sku: string; name: string; quantity: number; unitPriceMinor: number; subtotalMinor: number; taxMinor: number; totalMinor: number }>
  tenders: Array<{ label: string; amountMinor: number }>
}
export type TenantOverviewBranch = {
  branchId: string
  branchName: string
  timezone: string
  currencyCode: string
  sales: null | {
    saleCount: number
    totalsByCurrency: Array<{ currencyCode: string; saleCount: number; subtotalMinor: string; taxMinor: string; totalMinor: string }>
  }
  inventory: null | {
    count: number
    products: Array<{ productId: string; sku: string; name: string; stockQuantity: number; reorderPoint: number }>
  }
  attendance: null | { activeStaffCount: number; startedTodayCount: number }
}
export type TenantOverview = {
  branches: TenantOverviewBranch[]
  offset: number
  pageSize: number
  totalBranchCount: number
  salesWindowHours: number
}

function isTenderKind(value: string): value is TenderKind {
  return value === 'cash' || value === 'card' || value === 'mobile_money' || value === 'bank_transfer' || value === 'other'
}

function isHostedExpenseCategory(value: string): value is HostedExpenseCategory {
  return value === 'rent' || value === 'utilities' || value === 'wages' || value === 'transport' || value === 'supplies' || value === 'maintenance' || value === 'marketing' || value === 'bank_fees' || value === 'taxes_fees' || value === 'other'
}

function isHostedExpensePaymentMethod(value: string): value is HostedExpensePaymentMethod {
  return value === 'card' || value === 'mobile_money' || value === 'bank_transfer' || value === 'other'
}

function isHostedExpenseRecordPaymentMethod(value: string): value is HostedExpense['paymentMethod'] {
  return value === 'cash' || isHostedExpensePaymentMethod(value)
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function readString(value: unknown, field: string): string {
  if (!isRecord(value) || typeof value[field] !== 'string') throw new Error(`The API response is missing ${field}.`)
  return value[field]
}

function readStringArray(value: unknown, field: string): string[] {
  if (!isRecord(value) || !Array.isArray(value[field]) || !value[field].every((item) => typeof item === 'string')) {
    throw new Error(`The API response contains an invalid ${field} list.`)
  }
  return value[field]
}

function readNumber(value: unknown, field: string): number {
  if (!isRecord(value) || typeof value[field] !== 'number' || !Number.isSafeInteger(value[field])) {
    throw new Error(`The API response contains an invalid ${field}.`)
  }
  return value[field]
}

function readBoolean(value: unknown, field: string): boolean {
  if (!isRecord(value) || typeof value[field] !== 'boolean') throw new Error(`The API response contains an invalid ${field}.`)
  return value[field]
}

function readMinorTotal(value: unknown, field: string): string {
  const amount = readString(value, field)
  if (!/^\d+$/.test(amount)) throw new Error(`The API returned an invalid ${field}.`)
  return amount
}

function readNullableString(value: unknown, field: string): string | null {
  if (!isRecord(value) || !(typeof value[field] === 'string' || value[field] === null)) {
    throw new Error(`The API response contains an invalid ${field}.`)
  }
  return value[field]
}

function readNullableNumber(value: unknown, field: string): number | null {
  if (!isRecord(value) || !(value[field] === null || (typeof value[field] === 'number' && Number.isSafeInteger(value[field])))) {
    throw new Error(`The API response contains an invalid ${field}.`)
  }
  return value[field]
}

function parseBranchProduct(value: unknown): BranchProduct {
  if (!isRecord(value) || typeof value.isActive !== 'boolean') throw new Error('The API returned an invalid product.')
  return {
    productId: readString(value, 'productId'),
    identityRevision: readNumber(value, 'identityRevision'),
    sku: readString(value, 'sku'),
    barcode: readNullableString(value, 'barcode'),
    name: readString(value, 'name'),
    description: readNullableString(value, 'description'),
    category: readNullableString(value, 'category'),
    priceMinor: readNumber(value, 'priceMinor'),
    taxRateBps: readNumber(value, 'taxRateBps'),
    stockQuantity: readNumber(value, 'stockQuantity'),
    reorderPoint: readNumber(value, 'reorderPoint'),
    targetStockQuantity: readNumber(value, 'targetStockQuantity'),
    revision: readNumber(value, 'revision'),
    currencyCode: readString(value, 'currencyCode'),
    isActive: value.isActive,
  }
}

function parseCatalogProductIdentity(value: unknown): CatalogProductIdentity {
  return {
    productId: readString(value, 'productId'),
    identityRevision: readNumber(value, 'revision'),
    sku: readString(value, 'sku'),
    barcode: readNullableString(value, 'barcode'),
    name: readString(value, 'name'),
    description: readNullableString(value, 'description'),
    category: readNullableString(value, 'category'),
  }
}

function parseStockTransfer(value: unknown): HostedStockTransfer {
  if (!isRecord(value) || !Array.isArray(value.items)) throw new Error('The API returned an invalid stock transfer.')
  return {
    transferId: readString(value, 'transferId'), sourceBranchId: readString(value, 'sourceBranchId'),
    destinationBranchId: readString(value, 'destinationBranchId'), status: readString(value, 'status'),
    revision: readNumber(value, 'revision'), requestNote: readNullableString(value, 'requestNote'), requestedAt: readString(value, 'requestedAt'),
    items: value.items.map((line) => ({
      productId: readString(line, 'productId'), sku: readString(line, 'sku'), name: readString(line, 'name'),
      requestedQuantity: readNumber(line, 'requestedQuantity'), acceptedQuantity: readNullableNumber(line, 'acceptedQuantity'),
      dispatchedQuantity: readNullableNumber(line, 'dispatchedQuantity'), receivedQuantity: readNullableNumber(line, 'receivedQuantity'),
      discrepancyNote: readNullableString(line, 'discrepancyNote'),
    })),
  }
}

function parsePickupReservation(value: unknown): HostedPickupReservation {
  if (!isRecord(value) || !Array.isArray(value.items)) throw new Error('The API returned an invalid pickup reservation.')
  const status = readString(value, 'status')
  if (!['held', 'ready', 'collected', 'cancelled', 'expired'].includes(status)) throw new Error('The API returned an invalid pickup status.')
  return {
    reservationId: readString(value, 'reservationId'), originBranchId: readString(value, 'originBranchId'),
    originStationId: readString(value, 'originStationId'), pickupBranchId: readString(value, 'pickupBranchId'),
    requestedByMembershipId: readString(value, 'requestedByMembershipId'), customerId: readNullableString(value, 'customerId'),
    contactName: readString(value, 'contactName'), contactPhone: readString(value, 'contactPhone'),
    status: status as HostedPickupReservation['status'], revision: readNumber(value, 'revision'),
    expiresAt: readString(value, 'expiresAt'), createdAt: readString(value, 'createdAt'), replayed: value.replayed === true,
    items: value.items.map((line) => ({ productId: readString(line, 'productId'), sku: readString(line, 'sku'), name: readString(line, 'name'), quantity: readNumber(line, 'quantity') })),
  }
}

function parseBranchPaymentOption(value: unknown): BranchPaymentOption {
  if (!isRecord(value) || typeof value.enabled !== 'boolean' || typeof value.requiresReference !== 'boolean' || value.recordingMode !== 'manual') {
    throw new Error('The API returned an invalid payment method.')
  }
  const tenderKind = readString(value, 'tenderKind')
  if (!isTenderKind(tenderKind)) throw new Error('The API returned an unsupported payment method type.')
  return {
    id: readString(value, 'id'),
    methodKey: readString(value, 'methodKey'),
    label: readString(value, 'label'),
    tenderKind,
    providerName: readNullableString(value, 'providerName'),
    countryCode: readNullableString(value, 'countryCode'),
    requiresReference: value.requiresReference,
    enabled: value.enabled,
    recordingMode: 'manual',
    revision: readNumber(value, 'revision'),
  }
}

function parsePaymentIntegrationRequest(value: unknown): PaymentIntegrationRequest {
  if (!isRecord(value)) throw new Error('The API returned an invalid integration request.')
  const status = readString(value, 'status')
  const businessImpact = readString(value, 'businessImpact')
  if (!['submitted', 'under_review', 'planned', 'in_progress', 'released', 'not_planned'].includes(status)
    || !['business_blocked', 'workaround_available', 'exploring'].includes(businessImpact)) {
    throw new Error('The API returned an unsupported integration request status.')
  }
  return {
    id: readString(value, 'id'),
    branchId: readString(value, 'branchId'),
    countryCode: readString(value, 'countryCode'),
    providerName: readString(value, 'providerName'),
    terminalModel: readNullableString(value, 'terminalModel'),
    capability: readString(value, 'capability'),
    businessImpact: businessImpact as PaymentIntegrationRequest['businessImpact'],
    details: readNullableString(value, 'details'),
    status: status as PaymentIntegrationRequestStatus,
    customerVisibleUpdate: readNullableString(value, 'customerVisibleUpdate'),
    revision: readNumber(value, 'revision'),
    createdAt: readString(value, 'createdAt'),
    updatedAt: readString(value, 'updatedAt'),
  }
}

function isSupportRequestCategory(value: string): value is SupportRequestCategory {
  return ['bug', 'feature_request', 'account_access', 'data_issue', 'training', 'other'].includes(value)
}

function isSupportRequestStatus(value: string): value is SupportRequestStatus {
  return ['submitted', 'under_review', 'planned', 'in_progress', 'released', 'not_planned'].includes(value)
}

function parseSupportRequest(value: unknown): SupportRequest {
  if (!isRecord(value)) throw new Error('The API returned an invalid support request.')
  const category = readString(value, 'category')
  const status = readString(value, 'status')
  const diagnosticsPlatform = readNullableString(value, 'diagnosticsPlatform')
  if (diagnosticsPlatform !== null && !['web', 'windows', 'macos', 'linux', 'android', 'ios', 'harmonyos'].includes(diagnosticsPlatform)) {
    throw new Error('The API returned an unsupported diagnostics platform.')
  }
  if (!isSupportRequestCategory(category) || !isSupportRequestStatus(status)) {
    throw new Error('The API returned an unsupported support request category or status.')
  }
  return {
    id: readString(value, 'id'),
    branchId: readString(value, 'branchId'),
    category,
    title: readString(value, 'title'),
    details: readNullableString(value, 'details'),
    diagnosticsConsent: readBoolean(value, 'diagnosticsConsent'),
    diagnosticsAppVersion: readNullableString(value, 'diagnosticsAppVersion'),
    diagnosticsPlatform: diagnosticsPlatform as SupportRequestPlatform | null,
    status,
    customerVisibleUpdate: readNullableString(value, 'customerVisibleUpdate'),
    revision: readNumber(value, 'revision'),
    createdAt: readString(value, 'createdAt'),
    updatedAt: readString(value, 'updatedAt'),
  }
}

function parseStation(value: unknown): StationAccess {
  return { stationId: readString(value, 'stationId'), code: readString(value, 'code'), name: readString(value, 'name') }
}

function parseBranch(value: unknown): BranchAccess {
  if (!isRecord(value) || !Array.isArray(value.stations)) throw new Error('The API response contains an invalid branch.')
  return {
    branchId: readString(value, 'branchId'),
    code: readString(value, 'code'),
    name: readString(value, 'name'),
    timezone: readString(value, 'timezone'),
    currencyCode: readString(value, 'currencyCode'),
    stations: value.stations.map(parseStation),
  }
}

function parseRoleGrant(value: unknown): TenantMembership['roleGrants'][number] {
  if (!isRecord(value)) throw new Error('The API response contains an invalid role grant.')
  const branchId = value.branchId
  const stationId = value.stationId
  if (!(typeof branchId === 'string' || branchId === null) || !(typeof stationId === 'string' || stationId === null)) {
    throw new Error('The API response contains an invalid role scope.')
  }
  return { roleKey: readString(value, 'roleKey'), branchId, stationId }
}

function parseMembership(value: unknown): TenantMembership {
  if (!isRecord(value) || !Array.isArray(value.roleGrants) || !Array.isArray(value.branches)) {
    throw new Error('The API response contains an invalid tenant membership.')
  }
  return {
    tenantId: readString(value, 'tenantId'),
    displayName: readString(value, 'displayName'),
    membershipId: readString(value, 'membershipId'),
    roleGrants: value.roleGrants.map(parseRoleGrant),
    branches: value.branches.map(parseBranch),
  }
}

function parseEnvelopeData(value: unknown): unknown {
  if (!isRecord(value) || !('data' in value)) throw new Error('The API returned an invalid response envelope.')
  return value.data
}

async function requestJson(input: {
  apiBaseUrl: string
  path: string
  accessToken: string
  method?: 'GET' | 'POST' | 'PATCH' | 'PUT'
  body?: unknown
  bodyText?: string
  headers?: Record<string, string>
}): Promise<unknown> {
  const response = await fetch(`${input.apiBaseUrl}${input.path}`, {
    method: input.method ?? 'GET',
    headers: {
      Authorization: `Bearer ${input.accessToken}`,
      Accept: 'application/json',
      ...(input.body === undefined && input.bodyText === undefined ? {} : { 'Content-Type': 'application/json' }),
      ...input.headers,
    },
    ...(input.body === undefined && input.bodyText === undefined ? {} : { body: input.bodyText ?? JSON.stringify(input.body) }),
  })
  const payload: unknown = await response.json().catch(() => null)
  if (!response.ok) {
    const message = isRecord(payload) && isRecord(payload.error) && typeof payload.error.message === 'string'
      ? payload.error.message
      : `The API request failed (${response.status}).`
    throw new ApiError(message, response.status)
  }
  return payload
}

const getJson = requestJson

function stationPath(tenantId: string, branchId: string, stationId: string): string {
  return `${tenantBranchPath(tenantId, branchId)}/stations/${encodeURIComponent(stationId)}`
}

function parseActivationChallenge(value: unknown): { challengeId: string; nonce: string } {
  const data = parseEnvelopeData(value)
  return { challengeId: readString(data, 'challengeId'), nonce: readString(data, 'nonce') }
}

function parseLease(value: unknown): HostedLease {
  const data = parseEnvelopeData(value)
  return { leaseId: readString(data, 'leaseId'), token: readString(data, 'token'), expiresAt: readString(data, 'expiresAt') }
}

function parseLeaseChallenge(value: unknown): { challengeId: string; messageToSign: string } {
  const data = parseEnvelopeData(value)
  return { challengeId: readString(data, 'challengeId'), messageToSign: readString(data, 'messageToSign') }
}

function parseDrawerReport(value: unknown): HostedDrawerReport {
  if (!isRecord(value) || typeof value.isClosed !== 'boolean') throw new Error('The API returned an invalid cash drawer report.')
  const readOptionalNumber = (field: string): number | null => {
    const item = value[field]
    if (item === null) return null
    if (typeof item !== 'number' || !Number.isSafeInteger(item)) throw new Error(`The cash drawer report contains an invalid ${field}.`)
    return item
  }
  return {
    shiftId: readString(value, 'shiftId'), stationId: readString(value, 'stationId'),
    openedByMembershipId: readString(value, 'openedByMembershipId'), openedAt: readString(value, 'openedAt'),
    closedAt: readNullableString(value, 'closedAt'), openingCashMinor: readNumber(value, 'openingCashMinor'),
    cashSalesMinor: readNumber(value, 'cashSalesMinor'), paidInMinor: readNumber(value, 'paidInMinor'),
    paidOutMinor: readNumber(value, 'paidOutMinor'), cashDropsMinor: readNumber(value, 'cashDropsMinor'),
    expectedCashMinor: readNumber(value, 'expectedCashMinor'), countedCashMinor: readOptionalNumber('countedCashMinor'),
    differenceMinor: readOptionalNumber('differenceMinor'), movementCount: readNumber(value, 'movementCount'),
    saleCount: readNumber(value, 'saleCount'), isClosed: value.isClosed,
  }
}

function parseHostedCashLocation(value: unknown): HostedCashLocation {
  if (!isRecord(value)) throw new Error('The API returned an invalid cash location.')
  const type = readString(value,'locationType')
  if (!['register','safe','petty_cash','other'].includes(type)) throw new Error('The API returned an unsupported cash-location type.')
  const status = readString(value,'status')
  if (status !== 'active' && status !== 'inactive') throw new Error('The API returned an invalid cash-location status.')
  if (typeof value.initialized !== 'boolean') throw new Error('The API response is missing initialized.')
  return {
    id: readString(value,'id'), name: readString(value,'name'), locationType: type as HostedCashLocation['locationType'],
    stationId: readNullableString(value,'stationId'), status, currencyCode: readString(value,'currencyCode'),
    balanceMinor: readNumber(value,'balanceMinor'), initialized: value.initialized,
  }
}

function parseHostedSale(value: unknown): HostedSale {
  return {
    id: readString(value, 'id'), customerId: readNullableString(value, 'customerId'), receiptNumber: readString(value, 'receiptNumber'),
    cashierName: readString(value, 'cashierName'), stationName: readString(value, 'stationName'),
    currencyCode: readString(value, 'currencyCode'), subtotalMinor: readNumber(value, 'subtotalMinor'),
    taxMinor: readNumber(value, 'taxMinor'), totalMinor: readNumber(value, 'totalMinor'), onAccountMinor: readNumber(value, 'onAccountMinor'),
    createdAt: readString(value, 'createdAt'), replayed: isRecord(value) && value.replayed === true,
  }
}

function parseHeldTicketLine(value: unknown): { productId: string; quantity: number } {
  if (!isRecord(value)) throw new Error('The API returned an invalid held-ticket item.')
  const productId = readString(value, 'productId')
  const quantity = readNumber(value, 'quantity')
  if (!productId || quantity < 1) throw new Error('The API returned an invalid held-ticket item.')
  return { productId, quantity }
}

function parseHeldTicketPayload(value: unknown): HostedHeldTicketPayload {
  if (!isRecord(value) || !Array.isArray(value.lines)) throw new Error('The API returned an invalid held-ticket draft.')
  let pickupSale: HostedHeldTicketPayload['pickupSale'] = null
  if (value.pickupSale !== null) {
    if (!isRecord(value.pickupSale) || !Array.isArray(value.pickupSale.items)) throw new Error('The API returned an invalid held pickup draft.')
    pickupSale = {
      reservationId: readString(value.pickupSale, 'reservationId'),
      expectedRevision: readNumber(value.pickupSale, 'expectedRevision'),
      items: value.pickupSale.items.map(parseHeldTicketLine),
    }
  }
  return {
    label: readString(value, 'label'),
    lines: value.lines.map(parseHeldTicketLine),
    selectedCustomerId: readString(value, 'selectedCustomerId'),
    selectedPaymentId: readString(value, 'selectedPaymentId'),
    reference: readString(value, 'reference'),
    pickupSale,
  }
}

function parseHostedHeldTicket(value: unknown): HostedHeldTicket {
  if (!isRecord(value)) throw new Error('The API returned an invalid held ticket.')
  const status = readString(value, 'status')
  if (status !== 'held' && status !== 'checkout_pending' && status !== 'completed' && status !== 'discarded') {
    throw new Error('The API returned an unsupported held-ticket status.')
  }
  const checkoutBodyValue = value.checkoutBody
  if (!(checkoutBodyValue === null || isRecord(checkoutBodyValue))) throw new Error('The API returned an invalid saved checkout.')
  return {
    id: readString(value, 'id'),
    revision: readNumber(value, 'revision'),
    status,
    payload: parseHeldTicketPayload(value.payload),
    checkoutBody: isRecord(checkoutBodyValue) ? checkoutBodyValue : null,
    checkoutKey: readNullableString(value, 'checkoutKey'),
    claimLeaseId: readNullableString(value, 'claimLeaseId'),
    completedSaleId: readNullableString(value, 'completedSaleId'),
    createdAt: readString(value, 'createdAt'),
    updatedAt: readString(value, 'updatedAt'),
  }
}

function parseHostedCustomer(value: unknown): HostedCustomer {
  const status = readString(value, 'status')
  const revision = readNumber(value, 'revision')
  if ((status !== 'active' && status !== 'archived' && status !== 'merged') || revision < 1) throw new Error('The API returned an invalid customer status or revision.')
  return {
    id: readString(value, 'id'), name: readString(value, 'name'), phone: readNullableString(value, 'phone'),
    email: readNullableString(value, 'email'), notes: readNullableString(value, 'notes'), createdAt: readString(value, 'createdAt'), status, revision,
    mergedIntoCustomerId: readNullableString(value, 'mergedIntoCustomerId'),
  }
}

function parseHostedCustomerCreditPosition(value: unknown): HostedCustomerCreditPosition {
  if (!isRecord(value) || typeof value.hasCreditTerms !== 'boolean') throw new Error('The API returned an invalid customer credit position.')
  const termsRevision = readNullableNumber(value, 'termsRevision')
  const dueDays = readNumber(value, 'dueDays')
  if (termsRevision !== null && termsRevision < 1 || dueDays < 0) throw new Error('The API returned invalid customer credit terms.')
  return {
    currencyCode: readString(value, 'currencyCode'),
    hasCreditTerms: value.hasCreditTerms,
    termsRevision,
    creditLimitMinor: readString(value, 'creditLimitMinor'),
    dueDays,
    outstandingMinor: readString(value, 'outstandingMinor'),
    availableMinor: readString(value, 'availableMinor'),
  }
}

function parseHostedOpenReceivableInvoice(value: unknown): HostedOpenReceivableInvoice {
  if (!isRecord(value)) throw new Error('The API returned an invalid open invoice.')
  return {
    invoiceBranchId: readString(value, 'invoiceBranchId'),
    invoiceId: readString(value, 'invoiceId'),
    issuedAt: readString(value, 'issuedAt'),
    dueAt: readString(value, 'dueAt'),
    originalAmountMinor: readString(value, 'originalAmountMinor'),
    allocatedMinor: readString(value, 'allocatedMinor'),
    openMinor: readString(value, 'openMinor'),
  }
}

function parseHostedCustomerCreditTerm(value: unknown): HostedCustomerCreditTerm {
  if (!isRecord(value)) throw new Error('The API returned an invalid customer credit term.')
  const revision = readNumber(value, 'revision')
  const dueDays = readNumber(value, 'dueDays')
  if (revision < 1 || dueDays < 0 || dueDays > 365) throw new Error('The API returned invalid customer credit terms.')
  return {
    currencyCode: readString(value, 'currencyCode'),
    creditLimitMinor: readString(value, 'creditLimitMinor'),
    dueDays,
    revision,
    reason: readString(value, 'reason'),
    updatedAt: readString(value, 'updatedAt'),
  }
}

function parseHostedCustomerCreditTermEvent(value: unknown): HostedCustomerCreditTermEvent {
  if (!isRecord(value)) throw new Error('The API returned an invalid customer credit history event.')
  const revision = readNumber(value, 'revision')
  const dueDays = readNumber(value, 'dueDays')
  const previousDueDays = value.previousDueDays
  if (revision < 1 || dueDays < 0 || dueDays > 365 || (previousDueDays !== null && (typeof previousDueDays !== 'number' || !Number.isInteger(previousDueDays) || previousDueDays < 0 || previousDueDays > 365))) {
    throw new Error('The API returned invalid customer credit history.')
  }
  const previousCreditLimitMinor = value.previousCreditLimitMinor
  if (previousCreditLimitMinor !== null && (typeof previousCreditLimitMinor !== 'string' || !/^\d+$/.test(previousCreditLimitMinor))) {
    throw new Error('The API returned an invalid previous credit limit.')
  }
  return {
    currencyCode: readString(value, 'currencyCode'),
    revision,
    previousCreditLimitMinor,
    previousDueDays,
    creditLimitMinor: readString(value, 'creditLimitMinor'),
    dueDays,
    reason: readString(value, 'reason'),
    actorName: readString(value, 'actorName'),
    createdAt: readString(value, 'createdAt'),
  }
}

function parseHostedSupplier(value: unknown): HostedSupplier {
  const status = readString(value, 'status')
  const revision = readNumber(value, 'revision')
  if ((status !== 'active' && status !== 'archived') || revision < 1) throw new Error('The API returned an invalid supplier status or revision.')
  return {
    id: readString(value, 'id'), name: readString(value, 'name'), contactPerson: readNullableString(value, 'contactPerson'),
    email: readNullableString(value, 'email'), phone: readNullableString(value, 'phone'), address: readNullableString(value, 'address'),
    notes: readNullableString(value, 'notes'), createdAt: readString(value, 'createdAt'), status, revision,
  }
}

function parseHostedPurchaseOrder(value: unknown): HostedPurchaseOrder {
  const status = readString(value, 'status')
  if (!['draft', 'placed', 'partially_received', 'received', 'cancelled'].includes(status)) throw new Error('The API returned an invalid purchase-order status.')
  if (!isRecord(value) || !Array.isArray(value.items) || !Array.isArray(value.receipts)) throw new Error('The API returned an invalid purchase order.')
  return {
    id: readString(value, 'id'), orderNumber: readString(value, 'orderNumber'), supplierId: readNullableString(value, 'supplierId'),
    supplierName: readNullableString(value, 'supplierName'), reference: readNullableString(value, 'reference'),
    status: status as HostedPurchaseOrder['status'], revision: readNumber(value, 'revision'), createdAt: readString(value, 'createdAt'),
    placedAt: readNullableString(value, 'placedAt'),
    items: value.items.map((line): HostedPurchaseOrderLine => ({
      id: readString(line, 'id'), productId: readString(line, 'productId'), sku: readString(line, 'sku'), name: readString(line, 'name'),
      orderedQuantity: readNumber(line, 'orderedQuantity'), receivedQuantity: readNumber(line, 'receivedQuantity'), expectedUnitCostMinor: readNumber(line, 'expectedUnitCostMinor'),
    })),
    receipts: value.receipts.map((receipt): HostedPurchaseReceipt => ({
      id: readString(receipt, 'id'), reference: readNullableString(receipt, 'reference'), totalMinor: readNumber(receipt, 'totalMinor'), receivedAt: readString(receipt, 'receivedAt'),
    })),
  }
}

function parseHostedSalesReport(value: unknown): HostedSalesReport {
  if (!isRecord(value) || !Array.isArray(value.rows) || !Array.isArray(value.totalsByCurrency)) throw new Error('The API returned an invalid sales report.')
  return {
    rows: value.rows.map((row): HostedSalesReportRow => ({
      id: readString(row, 'id'),
      receiptNumber: readString(row, 'receiptNumber'),
      cashierName: readString(row, 'cashierName'),
      stationName: readString(row, 'stationName'),
      currencyCode: readString(row, 'currencyCode'),
      subtotalMinor: readNumber(row, 'subtotalMinor'),
      taxMinor: readNumber(row, 'taxMinor'),
      totalMinor: readNumber(row, 'totalMinor'),
      paymentMethods: readString(row, 'paymentMethods'),
      createdAt: readString(row, 'createdAt'),
    })),
    offset: readNumber(value, 'offset'),
    pageSize: readNumber(value, 'pageSize'),
    matchedCount: readNumber(value, 'matchedCount'),
    totalsByCurrency: value.totalsByCurrency.map((entry) => ({
      currencyCode: readString(entry, 'currencyCode'),
      saleCount: readNumber(entry, 'saleCount'),
      subtotalMinor: readMinorTotal(entry, 'subtotalMinor'),
      taxMinor: readMinorTotal(entry, 'taxMinor'),
      totalMinor: readMinorTotal(entry, 'totalMinor'),
    })),
  }
}

function parseHostedBranchAccountingCoverage(value: unknown): HostedBranchAccountingCoverage {
  if (!isRecord(value) || !Array.isArray(value.exceptions) || typeof value.exceptionsTruncated !== 'boolean') {
    throw new Error('The API returned invalid accounting coverage.')
  }
  const exceptions = value.exceptions.map((entry): HostedAccountingCoverageException => {
    if (!isRecord(entry)) throw new Error('The API returned an invalid accounting exception.')
    const sourceType = readString(entry, 'sourceType')
    if (sourceType !== 'sale' && sourceType !== 'refund' && sourceType !== 'partial_sale' && sourceType !== 'partial_refund') {
      throw new Error('The API returned an unsupported accounting exception type.')
    }
    const issueCode = readString(entry, 'issueCode')
    if (!isHostedAccountingCoverageIssueCode(issueCode)) throw new Error('The API returned an unsupported accounting coverage issue.')
    return {
      sourceType,
      sourceId: readString(entry, 'sourceId'),
      label: readString(entry, 'label'),
      occurredAt: readString(entry, 'occurredAt'),
      issueCode,
      amountMinor: readNumber(entry, 'amountMinor'),
      currencyCode: readString(entry, 'currencyCode'),
    }
  })
  return {
    salesCount: readNumber(value, 'salesCount'),
    unpostedSalesCount: readNumber(value, 'unpostedSalesCount'),
    refundsCount: readNumber(value, 'refundsCount'),
    unpostedRefundsCount: readNumber(value, 'unpostedRefundsCount'),
    partialJournalCount: readNumber(value, 'partialJournalCount'),
    exceptions,
    exceptionsTruncated: value.exceptionsTruncated,
  }
}

function isHostedAccountingCoverageIssueCode(value: string): value is HostedAccountingCoverageIssueCode {
  switch (value) {
    case 'sale_lines_do_not_reconcile':
    case 'sale_tenders_missing':
    case 'sale_tenders_do_not_reconcile':
    case 'sale_journal_missing':
    case 'refund_lines_do_not_reconcile':
    case 'refund_tenders_missing':
    case 'refund_tenders_do_not_reconcile':
    case 'refund_journal_missing':
    case 'historical_cost_or_tax_classification_unavailable':
      return true
    default:
      return false
  }
}

function parseHostedStationSalesHistory(value: unknown): HostedStationSalesHistory {
  if (!isRecord(value) || !Array.isArray(value.rows)) throw new Error('The API returned invalid station sales history.')
  return {
    rows: value.rows.map((row): HostedStationSalesHistoryRow => ({
      id: readString(row, 'id'),
      receiptNumber: readString(row, 'receiptNumber'),
      cashierName: readString(row, 'cashierName'),
      customerName: readNullableString(row, 'customerName'),
      currencyCode: readString(row, 'currencyCode'),
      totalMinor: readNumber(row, 'totalMinor'),
      createdAt: readString(row, 'createdAt'),
    })),
    offset: readNumber(value, 'offset'),
    pageSize: readNumber(value, 'pageSize'),
    matchedCount: readNumber(value, 'matchedCount'),
  }
}

function parseHostedReceipt(value: unknown): HostedReceipt {
  if (!isRecord(value) || !Array.isArray(value.lines) || !Array.isArray(value.tenders)) throw new Error('The API returned an invalid receipt.')
  return {
    id: readString(value, 'id'), customerId: readNullableString(value, 'customerId'), customerName: readNullableString(value, 'customerName'),
    receiptNumber: readString(value, 'receiptNumber'), cashierName: readString(value, 'cashierName'), stationName: readString(value, 'stationName'),
    currencyCode: readString(value, 'currencyCode'), subtotalMinor: readNumber(value, 'subtotalMinor'), taxMinor: readNumber(value, 'taxMinor'), totalMinor: readNumber(value, 'totalMinor'), onAccountMinor: readNumber(value, 'onAccountMinor'), dueAt: readNullableString(value, 'dueAt'), createdAt: readString(value, 'createdAt'),
    lines: value.lines.map((line) => ({ lineNumber: readNumber(line, 'lineNumber'), sku: readString(line, 'sku'), name: readString(line, 'name'), quantity: readNumber(line, 'quantity'), unitPriceMinor: readNumber(line, 'unitPriceMinor'), subtotalMinor: readNumber(line, 'subtotalMinor'), taxMinor: readNumber(line, 'taxMinor'), totalMinor: readNumber(line, 'totalMinor') })),
    tenders: value.tenders.map((tender) => ({ label: readString(tender, 'label'), amountMinor: readNumber(tender, 'amountMinor') })),
  }
}

function parseTenantOverview(value: unknown): TenantOverview {
  if (!isRecord(value) || !Array.isArray(value.branches)) throw new Error('The API returned an invalid business overview.')
  return {
    branches: value.branches.map((branch): TenantOverviewBranch => {
      if (!isRecord(branch)) throw new Error('The API returned an invalid branch overview.')
      const salesValue = branch.sales
      let sales: TenantOverviewBranch['sales'] = null
      if (salesValue !== null) {
        if (!isRecord(salesValue) || !Array.isArray(salesValue.totalsByCurrency)) throw new Error('The API returned invalid branch sales totals.')
        sales = {
          saleCount: readNumber(salesValue, 'saleCount'),
          totalsByCurrency: salesValue.totalsByCurrency.map((item) => ({
            currencyCode: readString(item, 'currencyCode'),
            saleCount: readNumber(item, 'saleCount'),
            subtotalMinor: readMinorTotal(item, 'subtotalMinor'),
            taxMinor: readMinorTotal(item, 'taxMinor'),
            totalMinor: readMinorTotal(item, 'totalMinor'),
          })),
        }
      }
      const inventoryValue = branch.inventory
      let inventory: TenantOverviewBranch['inventory'] = null
      if (inventoryValue !== null) {
        if (!isRecord(inventoryValue) || !Array.isArray(inventoryValue.products)) throw new Error('The API returned invalid branch inventory totals.')
        inventory = {
          count: readNumber(inventoryValue, 'count'),
          products: inventoryValue.products.map((item) => ({
            productId: readString(item, 'productId'),
            sku: readString(item, 'sku'),
            name: readString(item, 'name'),
            stockQuantity: readNumber(item, 'stockQuantity'),
            reorderPoint: readNumber(item, 'reorderPoint'),
          })),
        }
      }
      const attendanceValue = branch.attendance
      let attendance: TenantOverviewBranch['attendance'] = null
      if (attendanceValue !== null) {
        if (!isRecord(attendanceValue)) throw new Error('The API returned invalid branch attendance totals.')
        attendance = {
          activeStaffCount: readNumber(attendanceValue, 'activeStaffCount'),
          startedTodayCount: readNumber(attendanceValue, 'startedTodayCount'),
        }
      }
      return {
        branchId: readString(branch, 'branchId'),
        branchName: readString(branch, 'branchName'),
        timezone: readString(branch, 'timezone'),
        currencyCode: readString(branch, 'currencyCode'),
        sales,
        inventory,
        attendance,
      }
    }),
    offset: readNumber(value, 'offset'),
    pageSize: readNumber(value, 'pageSize'),
    totalBranchCount: readNumber(value, 'totalBranchCount'),
    salesWindowHours: readNumber(value, 'salesWindowHours'),
  }
}

export async function activateHostedStation(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string; key: HostedStationKey
}): Promise<{ activeStationCount: number; stationLimit: number }> {
  const path = stationPath(input.tenantId, input.branchId, input.stationId)
  const challenge = parseActivationChallenge(await requestJson({ ...input, path: `${path}/activation-challenge`, method: 'POST' }))
  const message = `advanced-pos/station-activation/v1\n${input.tenantId}\n${input.branchId}\n${input.stationId}\n${challenge.challengeId}\n${challenge.nonce}\n${input.key.publicKeyHex}`
  const signature = await signStationMessage(input.key.privateKey, message)
  const data = parseEnvelopeData(await requestJson({
    ...input, path: `${path}/activation`, method: 'POST',
    body: { devicePublicKey: input.key.publicKeyHex, challengeId: challenge.challengeId, signature },
  }))
  return { activeStationCount: readNumber(data, 'activeStationCount'), stationLimit: readNumber(data, 'stationLimit') }
}

export async function acquireHostedCashierLease(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string; key: HostedStationKey
}): Promise<HostedLease> {
  const path = stationPath(input.tenantId, input.branchId, input.stationId)
  const challenge = parseLeaseChallenge(await requestJson({ ...input, path: `${path}/actor-lease-challenge`, method: 'POST' }))
  const signature = await signStationMessage(input.key.privateKey, challenge.messageToSign)
  return parseLease(await requestJson({
    ...input, path: `${path}/actor-lease`, method: 'POST', body: { challengeId: challenge.challengeId, signature },
  }))
}

export async function heartbeatHostedCashierLease(input: {
  apiBaseUrl: string; accessToken: string; lease: HostedLease
}): Promise<void> {
  await requestJson({
    ...input, path: `/v1/station-actor-leases/${encodeURIComponent(input.lease.leaseId)}/heartbeat`,
    method: 'POST', body: { token: input.lease.token },
  })
}

export async function releaseHostedCashierLease(input: {
  apiBaseUrl: string; accessToken: string; lease: HostedLease
}): Promise<void> {
  await requestJson({
    ...input, path: `/v1/station-actor-leases/${encodeURIComponent(input.lease.leaseId)}/release`,
    method: 'POST', body: { token: input.lease.token },
  })
}

export async function getHostedCashLocations(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
}): Promise<HostedCashLocationWorkspace> {
  const payload = await getJson({ ...input, path: `${tenantBranchPath(input.tenantId,input.branchId)}/cash-locations` })
  const data = parseEnvelopeData(payload)
  if (!Array.isArray(data)) throw new Error('The API returned invalid cash-location data.')
  return { locations: data.map(parseHostedCashLocation) }
}

function parseHostedCashReconciliation(value: unknown): HostedCashReconciliation {
  if (!isRecord(value)) throw new Error('The API returned an invalid cash reconciliation.')
  return {
    id: readString(value, 'id'), currencyCode: readString(value, 'currencyCode'),
    trackedCashMinor: readNumber(value, 'trackedCashMinor'), ledgerCashBeforeMinor: readNumber(value, 'ledgerCashBeforeMinor'),
    adjustmentMinor: readNumber(value, 'adjustmentMinor'), reason: readString(value, 'reason'), createdAt: readString(value, 'createdAt'),
  }
}

export async function getHostedCashReconciliationStatus(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
}): Promise<HostedCashReconciliationStatus> {
  const value: unknown = parseEnvelopeData(await getJson({ ...input, path: `${tenantBranchPath(input.tenantId,input.branchId)}/accounting/cash-reconciliation` }))
  if (!isRecord(value) || !['cutover_required','incomplete_locations','open_shifts','unresolved_drawer_movements','reconciled','variance'].includes(readString(value,'status'))) {
    throw new Error('The API returned an invalid cash reconciliation status.')
  }
  return {
    currencyCode: readString(value,'currencyCode'), activeLocationCount: readNumber(value,'activeLocationCount'), trackedCashMinor: readNumber(value,'trackedCashMinor'),
    ledgerCashMinor: readNumber(value,'ledgerCashMinor'), varianceMinor: readNumber(value,'varianceMinor'),
    uninitializedLocationCount: readNumber(value,'uninitializedLocationCount'), openShiftCount: readNumber(value,'openShiftCount'),
    unresolvedDrawerMovementCount: readNumber(value,'unresolvedDrawerMovementCount'),
    latest: value.latest === null ? null : parseHostedCashReconciliation(value.latest),
    status: readString(value,'status') as HostedCashReconciliationStatus['status'],
  }
}

export async function postHostedCashReconciliation(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; idempotencyKey: string; reason: string
}): Promise<{ reconciliation: HostedCashReconciliation; replayed: boolean }> {
  const value: unknown = parseEnvelopeData(await requestJson({
    ...input, path: `${tenantBranchPath(input.tenantId,input.branchId)}/accounting/cash-reconciliation`, method: 'POST',
    body: { reason: input.reason }, headers: { 'Idempotency-Key': input.idempotencyKey },
  }))
  if (!isRecord(value) || !isRecord(value.reconciliation)) throw new Error('The API returned an invalid cash reconciliation response.')
  return { reconciliation: parseHostedCashReconciliation(value.reconciliation), replayed: value.replayed === true }
}

export async function createHostedCashLocation(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; idempotencyKey: string
  location: { name: string; locationType: HostedCashLocation['locationType']; stationId?: string }
}): Promise<{ location: HostedCashLocation; replayed: boolean }> {
  const data = parseEnvelopeData(await requestJson({ ...input, path: `${tenantBranchPath(input.tenantId,input.branchId)}/cash-locations`, method: 'POST', body: input.location, headers: { 'Idempotency-Key': input.idempotencyKey } }))
  if (!isRecord(data) || !isRecord(data.location)) throw new Error('The API returned an invalid cash location.')
  return { location: parseHostedCashLocation(data.location), replayed: data.replayed === true }
}

export async function openHostedCashLocation(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; locationId: string; idempotencyKey: string; amountMinor: number
}): Promise<{ location: HostedCashLocation; replayed: boolean }> {
  const path = `${tenantBranchPath(input.tenantId,input.branchId)}/cash-locations/${encodeURIComponent(input.locationId)}/opening`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'POST', body: { amountMinor: input.amountMinor }, headers: { 'Idempotency-Key': input.idempotencyKey } }))
  if (!isRecord(data) || !isRecord(data.location)) throw new Error('The API returned an invalid opening response.')
  return { location: parseHostedCashLocation(data.location), replayed: data.replayed === true }
}

export async function transferHostedCash(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; idempotencyKey: string
  fromLocationId: string; toLocationId: string; amountMinor: number; note: string
}): Promise<{ id: string; replayed: boolean }> {
  const data = parseEnvelopeData(await requestJson({ ...input, path: `${tenantBranchPath(input.tenantId,input.branchId)}/cash-transfers`, method: 'POST', body: { fromLocationId: input.fromLocationId, toLocationId: input.toLocationId, amountMinor: input.amountMinor, note: input.note }, headers: { 'Idempotency-Key': input.idempotencyKey } }))
  if (!isRecord(data)) throw new Error('The API returned an invalid transfer response.')
  return { id: readString(data,'id'), replayed: data.replayed === true }
}

export async function getHostedCashLocationEvents(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; locationId: string
}): Promise<HostedCashLocationEvent[]> {
  const data = parseEnvelopeData(await getJson({ ...input, path: `${tenantBranchPath(input.tenantId,input.branchId)}/cash-locations/${encodeURIComponent(input.locationId)}/events` }))
  if (!Array.isArray(data)) throw new Error('The API returned invalid cash-location history.')
  return data.map((value) => {
    if (!isRecord(value)) throw new Error('The API returned invalid cash-location history.')
    const eventType = readString(value,'eventType')
    if (!['opening','transfer_out','transfer_in','expense_outflow','supplier_payment_outflow','expense_reimbursement_inflow','sale_tender_inflow','sale_refund_outflow','register_movement_inflow','register_movement_outflow','cash_drop_inflow'].includes(eventType)) throw new Error('The API returned an unsupported cash event type.')
    return {
      id: readString(value,'id'), eventType: eventType as HostedCashLocationEvent['eventType'],
      signedAmountMinor: readNumber(value,'signedAmountMinor'), sourceTransferId: readNullableString(value,'sourceTransferId'), sourceExpenseId: readNullableString(value,'sourceExpenseId'), sourceSupplierPaymentId: readNullableString(value,'sourceSupplierPaymentId'), sourceExpenseReimbursementId: readNullableString(value,'sourceExpenseReimbursementId'), sourceSaleTenderId: readNullableString(value,'sourceSaleTenderId'), sourceRefundTenderId: readNullableString(value,'sourceRefundTenderId'), sourceDrawerMovementId: readNullableString(value,'sourceDrawerMovementId'),
      actorName: readString(value,'actorName'), createdAt: readString(value,'createdAt'),
    }
  })
}

export async function getHostedDrawerShift(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
}): Promise<HostedDrawerReport | null> {
  const data = parseEnvelopeData(await getJson({ ...input, path: `${stationPath(input.tenantId, input.branchId, input.stationId)}/drawer-shift` }))
  return data === null ? null : parseDrawerReport(data)
}

export async function listHostedCashDropDestinations(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
}): Promise<HostedCashDropDestination[]> {
  const path = `${stationPath(input.tenantId, input.branchId, input.stationId)}/drawer/cash-drop-destinations`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  if (!Array.isArray(data)) throw new Error('The API returned invalid cash-drop destinations.')
  return data.map((value) => {
    if (!isRecord(value)) throw new Error('The API returned invalid cash-drop destinations.')
    return { id: readString(value, 'id'), name: readString(value, 'name') }
  })
}

export async function getHostedDrawerHistory(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
}): Promise<HostedDrawerReport[]> {
  const path = `${stationPath(input.tenantId, input.branchId, input.stationId)}/drawer-shifts`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  if (!Array.isArray(data)) throw new Error('The API returned invalid drawer-shift history.')
  return data.map(parseDrawerReport)
}

export async function getHostedBranchSalesReport(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
  fromDate?: string; toDate?: string; offset?: number; pageSize?: number
}): Promise<HostedSalesReport> {
  const query = new URLSearchParams()
  if (input.fromDate) query.set('fromDate', input.fromDate)
  if (input.toDate) query.set('toDate', input.toDate)
  if (input.offset !== undefined) query.set('offset', String(input.offset))
  if (input.pageSize !== undefined) query.set('pageSize', String(input.pageSize))
  const suffix = query.size > 0 ? `?${query.toString()}` : ''
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/sales${suffix}`
  return parseHostedSalesReport(parseEnvelopeData(await getJson({ ...input, path })))
}

export async function getHostedBranchAccountingCoverage(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
  fromDate?: string; toDate?: string
}): Promise<HostedBranchAccountingCoverage> {
  const query = new URLSearchParams()
  if (input.fromDate) query.set('fromDate', input.fromDate)
  if (input.toDate) query.set('toDate', input.toDate)
  const suffix = query.size > 0 ? `?${query.toString()}` : ''
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/accounting/coverage${suffix}`
  return parseHostedBranchAccountingCoverage(parseEnvelopeData(await getJson({ ...input, path })))
}

export async function getHostedBranchTrialBalance(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
  fromDate?: string; toDate?: string
}): Promise<HostedTrialBalance> {
  const query = new URLSearchParams()
  if (input.fromDate) query.set('fromDate', input.fromDate)
  if (input.toDate) query.set('toDate', input.toDate)
  const suffix = query.size > 0 ? `?${query.toString()}` : ''
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/accounting/trial-balance${suffix}`
  const value: unknown = parseEnvelopeData(await getJson({ ...input, path }))
  if (!isRecord(value) || !Array.isArray(value.rows) || !Array.isArray(value.currencies) || !isRecord(value.coverage) || typeof value.salesRefundCoverageComplete !== 'boolean') {
    throw new Error('The API returned an invalid trial balance.')
  }
  const rows = value.rows.map((entry): HostedTrialBalanceRow => {
    if (!isRecord(entry)) throw new Error('The API returned an invalid trial balance row.')
    const accountType = readString(entry, 'accountType')
    if (!['asset', 'liability', 'equity', 'revenue', 'expense'].includes(accountType)) throw new Error('The API returned an unsupported trial balance account type.')
    return {
      currencyCode: readString(entry, 'currencyCode'),
      accountCode: readString(entry, 'accountCode'),
      accountName: readString(entry, 'accountName'),
      accountType,
      debitMinor: readString(entry, 'debitMinor'),
      creditMinor: readString(entry, 'creditMinor'),
      balanceMinor: readString(entry, 'balanceMinor'),
      journalCount: readNumber(entry, 'journalCount'),
    }
  })
  const currencies = value.currencies.map((entry) => {
    if (!isRecord(entry) || typeof entry.isBalanced !== 'boolean') throw new Error('The API returned an invalid trial balance currency total.')
    return {
      currencyCode: readString(entry, 'currencyCode'),
      totalDebitsMinor: readString(entry, 'totalDebitsMinor'),
      totalCreditsMinor: readString(entry, 'totalCreditsMinor'),
      isBalanced: entry.isBalanced,
    }
  })
  return {
    rows,
    currencies,
    coverage: parseHostedBranchAccountingCoverage(value.coverage),
    salesRefundCoverageComplete: value.salesRefundCoverageComplete,
  }
}

export async function getHostedBranchIncomeStatement(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
  fromDate?: string; toDate?: string
}): Promise<HostedIncomeStatement> {
  const query = new URLSearchParams()
  if (input.fromDate) query.set('fromDate', input.fromDate)
  if (input.toDate) query.set('toDate', input.toDate)
  const suffix = query.size > 0 ? `?${query.toString()}` : ''
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/accounting/income-statement${suffix}`
  const value: unknown = parseEnvelopeData(await getJson({ ...input, path }))
  if (!isRecord(value) || !Array.isArray(value.currencies) || !isRecord(value.coverage) || typeof value.salesRefundCoverageComplete !== 'boolean') {
    throw new Error('The API returned an invalid income statement.')
  }
  const currencies = value.currencies.map((entry): HostedIncomeStatementCurrency => {
    if (!isRecord(entry)) throw new Error('The API returned an invalid income statement currency total.')
    return {
      currencyCode: readString(entry, 'currencyCode'),
      grossSalesMinor: readString(entry, 'grossSalesMinor'),
      salesReturnsMinor: readString(entry, 'salesReturnsMinor'),
      netSalesMinor: readString(entry, 'netSalesMinor'),
      costOfGoodsSoldMinor: readString(entry, 'costOfGoodsSoldMinor'),
      grossProfitMinor: readString(entry, 'grossProfitMinor'),
      operatingExpensesMinor: readString(entry, 'operatingExpensesMinor'),
      operatingIncomeMinor: readString(entry, 'operatingIncomeMinor'),
    }
  })
  return {
    currencies,
    coverage: parseHostedBranchAccountingCoverage(value.coverage),
    salesRefundCoverageComplete: value.salesRefundCoverageComplete,
  }
}

export async function getHostedBranchBalanceSheet(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; asOfDate: string
}): Promise<HostedBalanceSheet> {
  const query = new URLSearchParams({ asOfDate: input.asOfDate })
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/accounting/balance-sheet?${query.toString()}`
  const value: unknown = parseEnvelopeData(await getJson({ ...input, path }))
  if (!isRecord(value) || !Array.isArray(value.currencies) || !isRecord(value.coverage) || typeof value.salesRefundCoverageComplete !== 'boolean' || typeof value.complete !== 'boolean') {
    throw new Error('The API returned an invalid balance sheet.')
  }
  const currencies = value.currencies.map((entry) => {
    if (!isRecord(entry) || typeof entry.isBalanced !== 'boolean' || !Array.isArray(entry.accounts)) throw new Error('The API returned an invalid balance sheet currency total.')
    const accounts = entry.accounts.map((account) => {
      if (!isRecord(account)) throw new Error('The API returned an invalid balance sheet account.')
      return {
        accountCode: readString(account, 'accountCode'),
        accountName: readString(account, 'accountName'),
        accountType: readString(account, 'accountType'),
        balanceMinor: readString(account, 'balanceMinor'),
      }
    })
    return {
      currencyCode: readString(entry, 'currencyCode'),
      assetsMinor: readString(entry, 'assetsMinor'),
      liabilitiesMinor: readString(entry, 'liabilitiesMinor'),
      contributedEquityMinor: readString(entry, 'contributedEquityMinor'),
      retainedEarningsMinor: readString(entry, 'retainedEarningsMinor'),
      totalEquityMinor: readString(entry, 'totalEquityMinor'),
      liabilitiesAndEquityMinor: readString(entry, 'liabilitiesAndEquityMinor'),
      differenceMinor: readString(entry, 'differenceMinor'),
      isBalanced: entry.isBalanced,
      accounts,
    }
  })
  return {
    asOfDate: readString(value, 'asOfDate'),
    currencies,
    coverage: parseHostedBranchAccountingCoverage(value.coverage),
    salesRefundCoverageComplete: value.salesRefundCoverageComplete,
    complete: value.complete,
  }
}

export async function getHostedBranchCashFlowStatement(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; fromDate: string; toDate: string
}): Promise<HostedCashFlowStatement> {
  const query = new URLSearchParams({ fromDate: input.fromDate, toDate: input.toDate })
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/accounting/cash-flow?${query.toString()}`
  const value: unknown = parseEnvelopeData(await getJson({ ...input, path }))
  if (!isRecord(value) || !Array.isArray(value.currencies) || !isRecord(value.coverage) || typeof value.complete !== 'boolean' || !Array.isArray(value.limitations) || !Array.isArray(value.currencies)) {
    throw new Error('The API returned an invalid cash-flow statement.')
  }
  const currencies = value.currencies.map((entry) => {
    if (!isRecord(entry) || typeof entry.isReconciled !== 'boolean' || !Array.isArray(entry.clearingAccounts)) throw new Error('The API returned an invalid cash-flow currency total.')
    return {
      currencyCode: readString(entry, 'currencyCode'),
      openingCashMinor: readString(entry, 'openingCashMinor'),
      operatingInflowsMinor: readString(entry, 'operatingInflowsMinor'),
      operatingOutflowsMinor: readString(entry, 'operatingOutflowsMinor'),
      netOperatingCashFlowMinor: readString(entry, 'netOperatingCashFlowMinor'),
      cutoverAdjustmentMinor: readString(entry, 'cutoverAdjustmentMinor'),
      unclassifiedJournalCashMinor: readString(entry, 'unclassifiedJournalCashMinor'),
      netChangeMinor: readString(entry, 'netChangeMinor'),
      endingCashMinor: readString(entry, 'endingCashMinor'),
      reconciliationDifferenceMinor: readString(entry, 'reconciliationDifferenceMinor'),
      isReconciled: entry.isReconciled,
      clearingAccounts: entry.clearingAccounts.map((account) => {
        if (!isRecord(account)) throw new Error('The API returned an invalid tender-clearing balance.')
        return {
          accountCode: readString(account, 'accountCode'),
          accountName: readString(account, 'accountName'),
          openingBalanceMinor: readString(account, 'openingBalanceMinor'),
          periodChangeMinor: readString(account, 'periodChangeMinor'),
          endingBalanceMinor: readString(account, 'endingBalanceMinor'),
        }
      }),
      unclassifiedRegisterMovementCount: readNumber(entry, 'unclassifiedRegisterMovementCount'),
      unclassifiedRegisterMovementNetMinor: readString(entry, 'unclassifiedRegisterMovementNetMinor'),
    }
  })
  return {
    fromDate: readString(value, 'fromDate'),
    toDate: readString(value, 'toDate'),
    currencies,
    coverage: parseHostedBranchAccountingCoverage(value.coverage),
    salesRefundCoverageComplete: value.salesRefundCoverageComplete === true,
    cashReconciliationStatus: readString(value, 'cashReconciliationStatus'),
    cashLocationVarianceMinor: readString(value, 'cashLocationVarianceMinor'),
    unresolvedDrawerMovementCount: readNumber(value, 'unresolvedDrawerMovementCount'),
    complete: value.complete,
    limitations: value.limitations.map((item) => {
      if (typeof item !== 'string') throw new Error('The API returned an invalid cash-flow limitation.')
      return item
    }),
  }
}

export async function getHostedBranchReceipt(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; saleId: string }): Promise<HostedReceipt> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/sales/${encodeURIComponent(input.saleId)}`
  return parseHostedReceipt(parseEnvelopeData(await getJson({ ...input, path })))
}

export async function getHostedStationReceipt(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string; saleId: string }): Promise<HostedReceipt> {
  const path = `${stationPath(input.tenantId, input.branchId, input.stationId)}/sales/${encodeURIComponent(input.saleId)}/receipt`
  return parseHostedReceipt(parseEnvelopeData(await getJson({ ...input, path })))
}

export async function getHostedStationSalesHistory(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
  search?: string; offset?: number; pageSize?: number
}): Promise<HostedStationSalesHistory> {
  const query = new URLSearchParams()
  if (input.search) query.set('search', input.search)
  if (input.offset !== undefined) query.set('offset', String(input.offset))
  if (input.pageSize !== undefined) query.set('pageSize', String(input.pageSize))
  const suffix = query.size > 0 ? `?${query.toString()}` : ''
  const path = `${stationPath(input.tenantId, input.branchId, input.stationId)}/sales${suffix}`
  return parseHostedStationSalesHistory(parseEnvelopeData(await getJson({ ...input, path })))
}

export async function getTenantOverview(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; offset?: number; pageSize?: number
}): Promise<TenantOverview> {
  const query = new URLSearchParams()
  if (input.offset !== undefined) query.set('offset', String(input.offset))
  if (input.pageSize !== undefined) query.set('pageSize', String(input.pageSize))
  const suffix = query.size > 0 ? `?${query.toString()}` : ''
  const path = `/v1/tenants/${encodeURIComponent(input.tenantId)}/overview${suffix}`
  return parseTenantOverview(parseEnvelopeData(await getJson({ ...input, path })))
}

export async function getHostedXReport(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string; shiftId: string
}): Promise<HostedDrawerReport> {
  const path = `${stationPath(input.tenantId, input.branchId, input.stationId)}/drawer-shifts/${encodeURIComponent(input.shiftId)}/x-report`
  return parseDrawerReport(parseEnvelopeData(await getJson({ ...input, path })))
}

async function signedHostedMutation(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
  lease: HostedLease; key: HostedStationKey; operation: string; resourceId?: string; idempotencyKey: string
  path: string; body: unknown
}): Promise<unknown> {
  const bodyText = JSON.stringify(input.body)
  const payloadHash = await sha256Hex(bodyText)
  const headers = {
    'x-station-lease-id': input.lease.leaseId,
    'x-station-lease-token': input.lease.token,
    'idempotency-key': input.idempotencyKey,
  }
  const challengeData = parseEnvelopeData(await requestJson({
    ...input, path: `${stationPath(input.tenantId, input.branchId, input.stationId)}/checkout-challenge`,
    method: 'POST', headers,
    body: { payloadHash, operation: input.operation, ...(input.resourceId ? { resourceId: input.resourceId } : {}) },
  }))
  const challengeId = readString(challengeData, 'challengeId')
  const signature = await signStationMessage(input.key.privateKey, readString(challengeData, 'messageToSign'))
  return requestJson({
    ...input, path: input.path, method: 'POST', bodyText,
    headers: { ...headers, 'x-station-challenge-id': challengeId, 'x-station-proof': signature },
  })
}

export async function openHostedDrawer(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
  lease: HostedLease; key: HostedStationKey; openingCashMinor: number; idempotencyKey: string
}): Promise<HostedDrawerReport> {
  const data = parseEnvelopeData(await signedHostedMutation({
    ...input, operation: 'drawer.open',
    path: `${stationPath(input.tenantId, input.branchId, input.stationId)}/drawer-shifts`,
    body: { openingCashMinor: input.openingCashMinor },
  }))
  return parseDrawerReport(data)
}

export async function closeHostedDrawer(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
  lease: HostedLease; key: HostedStationKey; shiftId: string; countedCashMinor: number; idempotencyKey: string
}): Promise<HostedDrawerReport> {
  const data = parseEnvelopeData(await signedHostedMutation({
    ...input, operation: 'drawer.close', resourceId: input.shiftId,
    path: `${stationPath(input.tenantId, input.branchId, input.stationId)}/drawer-shifts/${encodeURIComponent(input.shiftId)}/close`,
    body: { countedCashMinor: input.countedCashMinor },
  }))
  return parseDrawerReport(data)
}

export async function recordHostedDrawerMovement(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
  lease: HostedLease; key: HostedStationKey; shiftId: string; movementType: 'paid_in' | 'paid_out' | 'cash_drop'
  amountMinor: number; note: string; idempotencyKey: string; destinationLocationId?: string
}): Promise<HostedDrawerReport> {
  const data = parseEnvelopeData(await signedHostedMutation({
    ...input, operation: 'drawer.movement', resourceId: input.shiftId,
    path: `${stationPath(input.tenantId, input.branchId, input.stationId)}/drawer-shifts/${encodeURIComponent(input.shiftId)}/movements`,
    body: { movementType: input.movementType, amountMinor: input.amountMinor, note: input.note, ...(input.destinationLocationId ? { destinationLocationId: input.destinationLocationId } : {}) },
  }))
  return parseDrawerReport(data)
}

export async function commitHostedSale(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
  lease: HostedLease; key: HostedStationKey; idempotencyKey: string; items: Array<{ productId: string; quantity: number }>
  paymentOptionId: string; amountMinor: number; onAccountMinor?: number; reference: string | null; customerId: string | null
  pickupReservation?: { reservationId: string; expectedRevision: number } | null
  heldTicketId?: string | null
}): Promise<HostedSale> {
  const data = parseEnvelopeData(await signedHostedMutation({
    ...input, operation: 'sale.commit',
    path: `${stationPath(input.tenantId, input.branchId, input.stationId)}/sales`,
    body: {
      customerId: input.customerId,
      items: input.items,
      tenders: input.amountMinor > 0 ? [{ paymentOptionId: input.paymentOptionId, amountMinor: input.amountMinor, reference: input.reference }] : [],
      onAccountMinor: input.onAccountMinor ?? 0,
      ...(input.pickupReservation ? { pickupReservation: input.pickupReservation } : {}),
      ...(input.heldTicketId ? { heldTicketId: input.heldTicketId } : {}),
    },
  }))
  return parseHostedSale(data)
}

export async function listBranchHeldTickets(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
}): Promise<HostedHeldTicket[]> {
  const data = parseEnvelopeData(await requestJson({
    ...input,
    path: `${tenantBranchPath(input.tenantId, input.branchId)}/held-tickets`,
  }))
  if (!Array.isArray(data)) throw new Error('The API returned an invalid shared-ticket list.')
  return data.map(parseHostedHeldTicket)
}

export async function createBranchHeldTicket(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
  lease: HostedLease; idempotencyKey: string; payload: HostedHeldTicketPayload
}): Promise<HostedHeldTicket> {
  const data = parseEnvelopeData(await requestJson({
    ...input,
    path: `${stationPath(input.tenantId, input.branchId, input.stationId)}/held-tickets`,
    method: 'POST',
    body: { payload: input.payload },
    headers: {
      'x-station-lease-id': input.lease.leaseId,
      'x-station-lease-token': input.lease.token,
      'idempotency-key': input.idempotencyKey,
    },
  }))
  return parseHostedHeldTicket(data)
}

export async function claimBranchHeldTicket(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
  lease: HostedLease; ticketId: string; expectedRevision: number
}): Promise<HostedHeldTicket> {
  const data = parseEnvelopeData(await requestJson({
    ...input,
    path: `${stationPath(input.tenantId, input.branchId, input.stationId)}/held-tickets/${encodeURIComponent(input.ticketId)}/claim`,
    method: 'POST',
    body: { expectedRevision: input.expectedRevision },
    headers: { 'x-station-lease-id': input.lease.leaseId, 'x-station-lease-token': input.lease.token },
  }))
  return parseHostedHeldTicket(data)
}

export async function saveBranchHeldTicket(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
  lease: HostedLease; ticketId: string; expectedRevision: number; payload: HostedHeldTicketPayload
}): Promise<HostedHeldTicket> {
  const data = parseEnvelopeData(await requestJson({
    ...input,
    path: `${stationPath(input.tenantId, input.branchId, input.stationId)}/held-tickets/${encodeURIComponent(input.ticketId)}`,
    method: 'PUT',
    body: { expectedRevision: input.expectedRevision, payload: input.payload },
    headers: { 'x-station-lease-id': input.lease.leaseId, 'x-station-lease-token': input.lease.token },
  }))
  return parseHostedHeldTicket(data)
}

export async function prepareBranchHeldTicketCheckout(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
  lease: HostedLease; ticketId: string; expectedRevision: number; idempotencyKey: string; body: Record<string, unknown>
}): Promise<HostedHeldTicket> {
  const data = parseEnvelopeData(await requestJson({
    ...input,
    path: `${stationPath(input.tenantId, input.branchId, input.stationId)}/held-tickets/${encodeURIComponent(input.ticketId)}/checkout-attempt`,
    method: 'POST',
    body: { expectedRevision: input.expectedRevision, idempotencyKey: input.idempotencyKey, body: input.body },
    headers: { 'x-station-lease-id': input.lease.leaseId, 'x-station-lease-token': input.lease.token },
  }))
  return parseHostedHeldTicket(data)
}

export async function clearBranchHeldTicketCheckoutAttempt(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
  lease: HostedLease; ticketId: string; expectedRevision: number
}): Promise<HostedHeldTicket> {
  const data = parseEnvelopeData(await requestJson({
    ...input,
    path: `${stationPath(input.tenantId, input.branchId, input.stationId)}/held-tickets/${encodeURIComponent(input.ticketId)}/checkout-attempt/clear`,
    method: 'POST',
    body: { expectedRevision: input.expectedRevision },
    headers: { 'x-station-lease-id': input.lease.leaseId, 'x-station-lease-token': input.lease.token },
  }))
  return parseHostedHeldTicket(data)
}

export async function releaseBranchHeldTicket(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
  lease: HostedLease; ticketId: string; expectedRevision: number; discard?: boolean
}): Promise<HostedHeldTicket> {
  const action = input.discard ? 'discard' : 'release'
  const data = parseEnvelopeData(await requestJson({
    ...input,
    path: `${stationPath(input.tenantId, input.branchId, input.stationId)}/held-tickets/${encodeURIComponent(input.ticketId)}/${action}`,
    method: 'POST',
    body: { expectedRevision: input.expectedRevision },
    headers: { 'x-station-lease-id': input.lease.leaseId, 'x-station-lease-token': input.lease.token },
  }))
  return parseHostedHeldTicket(data)
}

function parseHostedRefund(value: unknown): HostedSaleRefund {
  if (!isRecord(value)) throw new Error('The API returned an invalid refund.')
  const kind = readString(value, 'kind')
  const method = readString(value, 'paymentMethod')
  if (kind !== 'refund' && kind !== 'void') throw new Error('The API returned an invalid refund type.')
  if (method === 'cash' || method === 'card' || method === 'mobile_money' || method === 'bank_transfer' || method === 'other') {
    return { id: readString(value, 'id'), saleId: readString(value, 'saleId'), receiptNumber: readString(value, 'receiptNumber'), kind, totalMinor: readNumber(value, 'totalMinor'), reason: readString(value, 'reason'), paymentMethod: method, paymentReference: readNullableString(value, 'paymentReference'), createdAt: readString(value, 'createdAt'), cashierName: readString(value, 'cashierName'), stationName: readString(value, 'stationName') }
  }
  throw new Error('The API returned an invalid refund method.')
}

function parseHostedSaleRefundDetail(value: unknown): HostedSaleRefundDetail {
  if (!isRecord(value) || !Array.isArray(value.items) || !Array.isArray(value.refunds)) throw new Error('The API returned invalid refund details.')
  return {
    saleId: readString(value, 'saleId'), receiptNumber: readString(value, 'receiptNumber'), cashierName: readString(value, 'cashierName'), status: readString(value, 'status'), currencyCode: readString(value, 'currencyCode'), totalMinor: readNumber(value, 'totalMinor'), onAccountMinor: readNumber(value, 'onAccountMinor'),
    items: value.items.map((row) => {
      if (!isRecord(row)) throw new Error('The API returned an invalid refundable item.')
      return { saleLineId: readString(row, 'saleLineId'), productId: readString(row, 'productId'), sku: readString(row, 'sku'), name: readString(row, 'name'), soldQuantity: readNumber(row, 'soldQuantity'), refundedQuantity: readNumber(row, 'refundedQuantity'), remainingQuantity: readNumber(row, 'remainingQuantity'), subtotalMinor: readNumber(row, 'subtotalMinor'), taxMinor: readNumber(row, 'taxMinor'), refundableTotalMinor: readNumber(row, 'refundableTotalMinor') }
    }),
    refunds: value.refunds.map(parseHostedRefund),
  }
}

export async function getHostedSaleRefundDetail(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string; saleId: string }): Promise<HostedSaleRefundDetail> {
  const path = `${stationPath(input.tenantId, input.branchId, input.stationId)}/sales/${encodeURIComponent(input.saleId)}/refunds`
  return parseHostedSaleRefundDetail(parseEnvelopeData(await getJson({ ...input, path })))
}

export async function refundHostedSale(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string; saleId: string; lease: HostedLease; key: HostedStationKey; idempotencyKey: string; refund: HostedSaleRefundInput }): Promise<HostedSaleRefund> {
  const data = parseEnvelopeData(await signedHostedMutation({
    ...input, operation: 'sale.refund', resourceId: input.saleId,
    path: `${stationPath(input.tenantId, input.branchId, input.stationId)}/sales/${encodeURIComponent(input.saleId)}/refunds`,
    body: input.refund,
  }))
  return parseHostedRefund(data)
}

export async function listStationHostedCustomers(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string; query?: string; offset?: number; pageSize?: number; includeArchived?: boolean }): Promise<HostedCustomer[]> {
  const params = new URLSearchParams()
  if (input.query?.trim()) params.set('query', input.query.trim())
  if (input.offset !== undefined) params.set('offset', String(input.offset))
  if (input.pageSize !== undefined) params.set('pageSize', String(input.pageSize))
  if (input.includeArchived) params.set('includeArchived', 'true')
  const suffix = params.size ? `?${params.toString()}` : ''
  const path = `${stationPath(input.tenantId, input.branchId, input.stationId)}/customers${suffix}`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  if (!Array.isArray(data)) throw new Error('The API returned an invalid customer list.')
  return data.map(parseHostedCustomer)
}

export async function getStationHostedCustomerCreditPosition(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string; customerId: string
}): Promise<HostedCustomerCreditPosition> {
  const path = `${stationPath(input.tenantId, input.branchId, input.stationId)}/customers/${encodeURIComponent(input.customerId)}/credit-position`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  return parseHostedCustomerCreditPosition(data)
}

export async function listTenantHostedCustomerCreditTerms(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; customerId: string
}): Promise<HostedCustomerCreditTerm[]> {
  const path = `/v1/tenants/${encodeURIComponent(input.tenantId)}/customers/${encodeURIComponent(input.customerId)}/credit-terms`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  if (!Array.isArray(data)) throw new Error('The API returned an invalid customer credit term list.')
  return data.map(parseHostedCustomerCreditTerm)
}

export async function listTenantHostedCustomerCreditTermHistory(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; customerId: string; currencyCode: string
}): Promise<HostedCustomerCreditTermEvent[]> {
  const path = `/v1/tenants/${encodeURIComponent(input.tenantId)}/customers/${encodeURIComponent(input.customerId)}/credit-terms/${encodeURIComponent(input.currencyCode)}/history`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  if (!Array.isArray(data)) throw new Error('The API returned invalid customer credit history.')
  return data.map(parseHostedCustomerCreditTermEvent)
}

export async function setTenantHostedCustomerCreditTerm(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; customerId: string; currencyCode: string
  idempotencyKey: string; expectedRevision: number; creditLimitMinor: number; dueDays: number; reason: string
}): Promise<HostedCustomerCreditTerm> {
  const path = `/v1/tenants/${encodeURIComponent(input.tenantId)}/customers/${encodeURIComponent(input.customerId)}/credit-terms/${encodeURIComponent(input.currencyCode)}`
  const data = parseEnvelopeData(await requestJson({
    ...input,
    path,
    method: 'PUT',
    body: {
      expectedRevision: input.expectedRevision,
      creditLimitMinor: input.creditLimitMinor,
      dueDays: input.dueDays,
      reason: input.reason,
    },
    headers: { 'Idempotency-Key': input.idempotencyKey },
  }))
  return parseHostedCustomerCreditTerm(data)
}

export async function listStationHostedOpenReceivables(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string; customerId: string
}): Promise<HostedOpenReceivableInvoice[]> {
  const path = `${stationPath(input.tenantId, input.branchId, input.stationId)}/customers/${encodeURIComponent(input.customerId)}/receivables`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  if (!Array.isArray(data)) throw new Error('The API returned an invalid open invoice list.')
  return data.map(parseHostedOpenReceivableInvoice)
}

export async function recordStationHostedCashCollection(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
  lease: HostedLease; key: HostedStationKey; idempotencyKey: string
  collection: { customerId: string; amountMinor: number; drawerShiftId: string; externalReference: string | null; allocations: Array<{ invoiceBranchId: string; invoiceId: string; amountMinor: number }> }
}): Promise<{ collection: HostedCashCollection; replayed: boolean }> {
  const data = parseEnvelopeData(await signedHostedMutation({
    ...input,
    operation: 'receivable.collection',
    resourceId: input.collection.drawerShiftId,
    path: `${stationPath(input.tenantId, input.branchId, input.stationId)}/customer-collections`,
    body: input.collection,
  }))
  if (!isRecord(data) || !isRecord(data.collection)) throw new Error('The API returned an invalid customer payment result.')
  return {
    collection: {
      id: readString(data.collection, 'id'),
      customerId: readString(data.collection, 'customerId'),
      currencyCode: readString(data.collection, 'currencyCode'),
      amountMinor: readNumber(data.collection, 'amountMinor'),
      receivedAt: readString(data.collection, 'receivedAt'),
      allocationCount: readNumber(data.collection, 'allocationCount'),
    },
    replayed: data.replayed === true,
  }
}

export async function listBranchHostedCustomers(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; query?: string; offset?: number; pageSize?: number; includeArchived?: boolean }): Promise<HostedCustomer[]> {
  const params = new URLSearchParams()
  if (input.query?.trim()) params.set('query', input.query.trim())
  if (input.offset !== undefined) params.set('offset', String(input.offset))
  if (input.pageSize !== undefined) params.set('pageSize', String(input.pageSize))
  if (input.includeArchived) params.set('includeArchived', 'true')
  const suffix = params.size ? `?${params.toString()}` : ''
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/customers${suffix}`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  if (!Array.isArray(data)) throw new Error('The API returned an invalid customer list.')
  return data.map(parseHostedCustomer)
}

export async function createStationHostedCustomer(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string; idempotencyKey: string; customer: { name: string; phone: string | null; email: string | null; notes: string | null } }): Promise<HostedCustomer> {
  const path = `${stationPath(input.tenantId, input.branchId, input.stationId)}/customers`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'POST', headers: { 'Idempotency-Key': input.idempotencyKey }, body: input.customer }))
  return parseHostedCustomer(data)
}

export async function attachStationSaleCustomer(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string; saleId: string; customerId: string }): Promise<AttachedSaleCustomer> {
  const path = `${stationPath(input.tenantId, input.branchId, input.stationId)}/sales/${encodeURIComponent(input.saleId)}/customer`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'PUT', body: { customerId: input.customerId } }))
  if (!isRecord(data)) throw new Error('The API returned an invalid sale customer link.')
  return { saleId: readString(data, 'saleId'), customerId: readString(data, 'customerId'), customerName: readNullableString(data, 'customerName') }
}

export async function createBranchHostedCustomer(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; idempotencyKey: string; customer: { name: string; phone: string | null; email: string | null; notes: string | null } }): Promise<HostedCustomer> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/customers`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'POST', headers: { 'Idempotency-Key': input.idempotencyKey }, body: input.customer }))
  return parseHostedCustomer(data)
}

export async function updateBranchHostedCustomer(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; customerId: string; expectedRevision: number; customer: { name: string; phone: string | null; email: string | null; notes: string | null } }): Promise<HostedCustomer> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/customers/${encodeURIComponent(input.customerId)}`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'PATCH', body: { ...input.customer, expectedRevision: input.expectedRevision } }))
  return parseHostedCustomer(data)
}

export async function setBranchHostedCustomerStatus(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; customerId: string; expectedRevision: number; status: 'active' | 'archived' }): Promise<HostedCustomer> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/customers/${encodeURIComponent(input.customerId)}/status`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'PATCH', body: { expectedRevision: input.expectedRevision, status: input.status } }))
  return parseHostedCustomer(data)
}

export type HostedCustomerMergeResult = { source: HostedCustomer; target: HostedCustomer; mergedAt: string; replayed: boolean }

export async function mergeBranchHostedCustomer(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; sourceCustomerId: string
  targetCustomerId: string; expectedSourceRevision: number; expectedTargetRevision: number; idempotencyKey: string
}): Promise<HostedCustomerMergeResult> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/customers/${encodeURIComponent(input.sourceCustomerId)}/merge`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'POST', body: {
    targetCustomerId: input.targetCustomerId, expectedSourceRevision: input.expectedSourceRevision,
    expectedTargetRevision: input.expectedTargetRevision, idempotencyKey: input.idempotencyKey,
  } }))
  if (!isRecord(data)) throw new Error('The API returned an invalid customer merge.')
  return { source: parseHostedCustomer(data.source), target: parseHostedCustomer(data.target), mergedAt: readString(data, 'mergedAt'), replayed: data.replayed === true }
}

export type HostedSupplierInput = { name: string; contactPerson: string | null; email: string | null; phone: string | null; address: string | null; notes: string | null }

export async function listBranchHostedSuppliers(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; query?: string; offset?: number; pageSize?: number; includeArchived?: boolean }): Promise<HostedSupplier[]> {
  const params = new URLSearchParams()
  if (input.query?.trim()) params.set('query', input.query.trim())
  if (input.offset !== undefined) params.set('offset', String(input.offset))
  if (input.pageSize !== undefined) params.set('pageSize', String(input.pageSize))
  if (input.includeArchived) params.set('includeArchived', 'true')
  const suffix = params.size ? `?${params.toString()}` : ''
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/suppliers${suffix}`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  if (!Array.isArray(data)) throw new Error('The API returned an invalid supplier list.')
  return data.map(parseHostedSupplier)
}

export async function createBranchHostedSupplier(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; idempotencyKey: string; supplier: HostedSupplierInput }): Promise<HostedSupplier> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/suppliers`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'POST', headers: { 'Idempotency-Key': input.idempotencyKey }, body: input.supplier }))
  return parseHostedSupplier(data)
}

export async function updateBranchHostedSupplier(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; supplierId: string; expectedRevision: number; supplier: HostedSupplierInput }): Promise<HostedSupplier> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/suppliers/${encodeURIComponent(input.supplierId)}`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'PATCH', body: { ...input.supplier, expectedRevision: input.expectedRevision } }))
  return parseHostedSupplier(data)
}

export async function setBranchHostedSupplierStatus(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; supplierId: string; expectedRevision: number; status: 'active' | 'archived' }): Promise<HostedSupplier> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/suppliers/${encodeURIComponent(input.supplierId)}/status`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'PATCH', body: { expectedRevision: input.expectedRevision, status: input.status } }))
  return parseHostedSupplier(data)
}

export async function listBranchPurchaseOrders(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string }): Promise<HostedPurchaseOrder[]> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/purchase-orders`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  if (!Array.isArray(data)) throw new Error('The API returned an invalid purchase-order list.')
  return data.map(parseHostedPurchaseOrder)
}

export async function createBranchPurchaseOrder(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; idempotencyKey: string; supplierId: string | null; reference: string | null; items: HostedPurchaseOrderItemInput[] }): Promise<HostedPurchaseOrder> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/purchase-orders`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'POST', headers: { 'Idempotency-Key': input.idempotencyKey }, body: { supplierId: input.supplierId, reference: input.reference, items: input.items } }))
  return parseHostedPurchaseOrder(data)
}

export async function updateBranchPurchaseOrderDraft(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; orderId: string; expectedRevision: number; supplierId: string | null; reference: string | null; items: HostedPurchaseOrderItemInput[] }): Promise<HostedPurchaseOrder> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/purchase-orders/${encodeURIComponent(input.orderId)}`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'PATCH', body: { supplierId: input.supplierId, reference: input.reference, expectedRevision: input.expectedRevision, items: input.items } }))
  return parseHostedPurchaseOrder(data)
}

export async function transitionBranchPurchaseOrder(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; orderId: string; action: 'place' | 'cancel'; expectedRevision: number }): Promise<HostedPurchaseOrder> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/purchase-orders/${encodeURIComponent(input.orderId)}/${input.action}`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'POST', body: { expectedRevision: input.expectedRevision } }))
  return parseHostedPurchaseOrder(data)
}

export async function receiveBranchPurchaseOrder(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; orderId: string; idempotencyKey: string; expectedRevision: number; reference: string | null; items: HostedPurchaseReceiptItemInput[] }): Promise<HostedPurchaseOrder> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/purchase-orders/${encodeURIComponent(input.orderId)}/receipts`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'POST', headers: { 'Idempotency-Key': input.idempotencyKey }, body: { expectedRevision: input.expectedRevision, reference: input.reference, items: input.items } }))
  return parseHostedPurchaseOrder(data)
}

export async function getBranchSupplierBilling(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string }): Promise<HostedSupplierBillingWorkspace> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/supplier-billing`
  const value = parseEnvelopeData(await getJson({ ...input, path }))
  if (!isRecord(value) || !Array.isArray(value.unbilledReceipts) || !Array.isArray(value.bills) || !Array.isArray(value.payments) || !Array.isArray(value.creditNotes) || !Array.isArray(value.returnableItems)) throw new Error('The API returned an invalid supplier billing workspace.')
  return {
    currencyCode: readString(value, 'currencyCode'),
    unbilledReceipts: value.unbilledReceipts.map((row): HostedUnbilledReceipt => ({
      receiptId: readString(row, 'receiptId'), supplierId: readString(row, 'supplierId'), supplierName: readString(row, 'supplierName'),
      supplierReference: readNullableString(row, 'supplierReference'), purchaseOrderId: readString(row, 'purchaseOrderId'),
      receivedAt: readString(row, 'receivedAt'), totalMinor: readNumber(row, 'totalMinor'), billedMinor: readNumber(row, 'billedMinor'), availableMinor: readNumber(row, 'availableMinor'),
    })),
    bills: value.bills.map((row): HostedSupplierBill => ({
      id: readString(row, 'id'), supplierId: readString(row, 'supplierId'), supplierName: readString(row, 'supplierName'),
      supplierReference: readString(row, 'supplierReference'), invoiceDate: readString(row, 'invoiceDate'), dueDate: readNullableString(row, 'dueDate'),
      currencyCode: readString(row, 'currencyCode'), totalMinor: readNumber(row, 'totalMinor'), paidMinor: readNumber(row, 'paidMinor'), creditedMinor: readNumber(row, 'creditedMinor'), balanceMinor: readNumber(row, 'balanceMinor'), createdAt: readString(row, 'createdAt'),
    })),
    payments: value.payments.map((row): HostedSupplierPayment => {
      const paymentMethod = readString(row, 'paymentMethod')
      if (!isTenderKind(paymentMethod)) throw new Error('The API returned an unsupported supplier payment method.')
      return {
        id: readString(row, 'id'), supplierId: readString(row, 'supplierId'), supplierName: readString(row, 'supplierName'),
        paymentMethod, cashLocationId: readNullableString(row, 'cashLocationId'), externalReference: readNullableString(row, 'externalReference'), amountMinor: readNumber(row, 'amountMinor'),
        currencyCode: readString(row, 'currencyCode'), paidAt: readString(row, 'paidAt'),
      }
    }),
    creditNotes: value.creditNotes.map((row): HostedSupplierCreditNote => ({
      id: readString(row, 'id'), billId: readString(row, 'billId'), supplierId: readString(row, 'supplierId'), supplierName: readString(row, 'supplierName'),
      creditReference: readString(row, 'creditReference'), effectiveDate: readString(row, 'effectiveDate'), reason: readString(row, 'reason'),
      currencyCode: readString(row, 'currencyCode'), totalMinor: readNumber(row, 'totalMinor'), createdAt: readString(row, 'createdAt'),
    })),
    returnableItems: value.returnableItems.map((row): HostedSupplierReturnCandidate => ({
      billId: readString(row, 'billId'), receiptId: readString(row, 'receiptId'), receiptReference: readNullableString(row, 'receiptReference'),
      receiptLineId: readString(row, 'receiptLineId'), productId: readString(row, 'productId'), sku: readString(row, 'sku'), productName: readString(row, 'productName'),
      unitCostMinor: readNumber(row, 'unitCostMinor'), maxQuantity: readNumber(row, 'maxQuantity'),
    })),
  }
}

function parseHostedExpense(value: unknown): HostedExpense {
  const categoryCode = readString(value, 'categoryCode')
  const paymentMethod = readString(value, 'paymentMethod')
  if (!isHostedExpenseCategory(categoryCode) || !isHostedExpenseRecordPaymentMethod(paymentMethod)) {
    throw new Error('The API returned an unsupported expense category or payment method.')
  }
  const costCenterKind = readString(value, 'costCenterKind')
  if (!isHostedCostCenterKind(costCenterKind)) throw new Error('The API returned an unsupported expense cost center.')
  return {
    id: readString(value, 'id'), categoryCode, description: readString(value, 'description'),
    payee: readNullableString(value, 'payee'), amountMinor: readNumber(value, 'amountMinor'), reimbursedMinor: readNumber(value, 'reimbursedMinor'), remainingReimbursableMinor: readNumber(value, 'remainingReimbursableMinor'), paymentMethod,
    drawerShiftId: readNullableString(value, 'drawerShiftId'),
    cashLocationId: readNullableString(value, 'cashLocationId'),
    externalReference: readNullableString(value, 'externalReference'), occurredOn: readString(value, 'occurredOn'),
    currencyCode: readString(value, 'currencyCode'), costCenterKind, costCenterStationId: readNullableString(value, 'costCenterStationId'),
    costCenterStationNameSnapshot: readNullableString(value, 'costCenterStationNameSnapshot'), costCenterRevision: readNumber(value, 'costCenterRevision'),
    recordedBy: readString(value, 'recordedBy'), createdAt: readString(value, 'createdAt'),
  }
}

function isHostedCostCenterKind(value: string): value is HostedCostCenterKind {
  return value === 'station_direct' || value === 'store_overhead' || value === 'legacy_unassigned'
}

function parseHostedExpenseCostCenterReclassification(value: unknown): HostedExpenseCostCenterReclassification {
  if (!isRecord(value)) throw new Error('The API returned an invalid expense attribution correction.')
  const previousKind = readString(value, 'previousKind')
  const nextKind = readString(value, 'nextKind')
  if (!isHostedCostCenterKind(previousKind) || !isHostedCostCenterKind(nextKind)) throw new Error('The API returned an unsupported expense cost center.')
  return {
    id: readString(value, 'id'), expenseId: readString(value, 'expenseId'), revision: readNumber(value, 'revision'),
    previousKind, previousStationId: readNullableString(value, 'previousStationId'), previousStationNameSnapshot: readNullableString(value, 'previousStationNameSnapshot'),
    nextKind, nextStationId: readNullableString(value, 'nextStationId'), nextStationNameSnapshot: readNullableString(value, 'nextStationNameSnapshot'),
    reason: readString(value, 'reason'), recordedAt: readString(value, 'recordedAt'), actorName: readString(value, 'actorName'),
  }
}

export async function getBranchExpenses(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; offset?: number; pageSize?: number }): Promise<HostedExpenseWorkspace> {
  const params = new URLSearchParams({ offset: String(input.offset ?? 0), pageSize: String(input.pageSize ?? 50) })
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/expenses?${params.toString()}`
  const value = parseEnvelopeData(await getJson({ ...input, path }))
  if (!isRecord(value) || !Array.isArray(value.expenses) || typeof value.hasMore !== 'boolean') throw new Error('The API returned an invalid expenses workspace.')
  return {
    currencyCode: readString(value, 'currencyCode'), expenses: value.expenses.map(parseHostedExpense),
    offset: readNumber(value, 'offset'), pageSize: readNumber(value, 'pageSize'), hasMore: value.hasMore,
  }
}

export async function getBranchExpenseCostCenterHistory(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; expenseId: string }): Promise<HostedExpenseCostCenterHistory> {
  const path = `${tenantBranchPath(input.tenantId,input.branchId)}/expenses/${encodeURIComponent(input.expenseId)}/cost-center`
  const value = parseEnvelopeData(await getJson({ ...input, path }))
  if (!isRecord(value) || !Array.isArray(value.rows)) throw new Error('The API returned invalid expense attribution history.')
  const currentKind = readString(value, 'currentKind')
  if (!isHostedCostCenterKind(currentKind)) throw new Error('The API returned an unsupported expense cost center.')
  return { currentRevision: readNumber(value, 'currentRevision'), currentKind, currentStationId: readNullableString(value, 'currentStationId'), currentStationNameSnapshot: readNullableString(value, 'currentStationNameSnapshot'), rows: value.rows.map(parseHostedExpenseCostCenterReclassification) }
}

export async function reclassifyBranchExpense(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; expenseId: string; idempotencyKey: string
  correction: { expectedRevision: number; costCenterKind: HostedCostCenterKind; stationId: string | null; reason: string }
}): Promise<HostedExpenseCostCenterReclassification> {
  const path = `${tenantBranchPath(input.tenantId,input.branchId)}/expenses/${encodeURIComponent(input.expenseId)}/cost-center/reclassifications`
  const value = parseEnvelopeData(await requestJson({ ...input, path, method: 'POST', headers: { 'Idempotency-Key': input.idempotencyKey }, body: input.correction }))
  return parseHostedExpenseCostCenterReclassification(value)
}

export async function createBranchExpense(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; idempotencyKey: string; expense: HostedExpenseInput
}): Promise<{ expense: HostedExpense; replayed: boolean }> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/expenses`
  const value = parseEnvelopeData(await requestJson({
    ...input, path, method: 'POST', headers: { 'Idempotency-Key': input.idempotencyKey }, body: input.expense,
  }))
  if (!isRecord(value) || typeof value.replayed !== 'boolean') throw new Error('The API returned an invalid expense result.')
  return { expense: parseHostedExpense(value), replayed: value.replayed }
}

function parseHostedExpenseReimbursement(value: unknown): HostedExpenseReimbursement {
  if (!isRecord(value)) throw new Error('The API returned an invalid expense reimbursement.')
  const returnMethod = readString(value, 'returnMethod')
  if (!isTenderKind(returnMethod)) throw new Error('The API returned an unsupported reimbursement method.')
  return {
    id: readString(value, 'id'), expenseId: readString(value, 'expenseId'), amountMinor: readNumber(value, 'amountMinor'),
    returnMethod, cashLocationId: readNullableString(value, 'cashLocationId'), externalReference: readNullableString(value, 'externalReference'),
    reason: readString(value, 'reason'), returnedOn: readString(value, 'returnedOn'), recordedBy: readString(value, 'recordedBy'), createdAt: readString(value, 'createdAt'),
  }
}

export async function getBranchExpenseReimbursements(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; expenseId: string }): Promise<HostedExpenseReimbursementWorkspace> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/expenses/${encodeURIComponent(input.expenseId)}/reimbursements`
  const value = parseEnvelopeData(await getJson({ ...input, path }))
  if (!isRecord(value) || !Array.isArray(value.reimbursements)) throw new Error('The API returned an invalid reimbursement history.')
  return {
    expenseId: readString(value, 'expenseId'), amountMinor: readNumber(value, 'amountMinor'),
    reimbursedMinor: readNumber(value, 'reimbursedMinor'), remainingMinor: readNumber(value, 'remainingMinor'),
    reimbursements: value.reimbursements.map(parseHostedExpenseReimbursement),
  }
}

export async function createBranchExpenseReimbursement(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; expenseId: string; idempotencyKey: string
  reimbursement: { amountMinor: number; returnMethod: HostedExpenseReimbursement['returnMethod']; cashLocationId: string | null; externalReference: string | null; reason: string; returnedOn: string }
}): Promise<{ reimbursement: HostedExpenseReimbursement; replayed: boolean }> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/expenses/${encodeURIComponent(input.expenseId)}/reimbursements`
  const value = parseEnvelopeData(await requestJson({ ...input, path, method: 'POST', headers: { 'Idempotency-Key': input.idempotencyKey }, body: input.reimbursement }))
  if (!isRecord(value) || typeof value.replayed !== 'boolean') throw new Error('The API returned an invalid reimbursement result.')
  return { reimbursement: parseHostedExpenseReimbursement(value.reimbursement), replayed: value.replayed }
}

export async function recordStationCashExpense(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
  lease: HostedLease; key: HostedStationKey; idempotencyKey: string; drawerShiftId: string; expense: HostedExpenseInput
}): Promise<{ expense: HostedExpense; replayed: boolean }> {
  const path = `${stationPath(input.tenantId, input.branchId, input.stationId)}/expenses`
  const body = { ...input.expense, paymentMethod: 'cash' as const, drawerShiftId: input.drawerShiftId }
  const value = parseEnvelopeData(await signedHostedMutation({
    ...input, operation: 'expense.payment', resourceId: input.drawerShiftId,
    path, body, idempotencyKey: input.idempotencyKey,
  }))
  if (!isRecord(value) || typeof value.replayed !== 'boolean') throw new Error('The API returned an invalid cash expense result.')
  return { expense: parseHostedExpense(value), replayed: value.replayed }
}

export async function matchBranchSupplierBill(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; idempotencyKey: string
  supplierId: string; supplierReference: string; invoiceDate: string; dueDate: string | null; totalMinor: number
  allocations: HostedSupplierBillAllocationInput[]
}): Promise<{ bill: HostedSupplierBill; replayed: boolean }> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/supplier-bills`
  const value = parseEnvelopeData(await requestJson({ ...input, path, method: 'POST', headers: { 'Idempotency-Key': input.idempotencyKey }, body: {
    supplierId: input.supplierId, supplierReference: input.supplierReference, invoiceDate: input.invoiceDate,
    dueDate: input.dueDate, totalMinor: input.totalMinor, allocations: input.allocations,
  } }))
  if (!isRecord(value)) throw new Error('The API returned an invalid supplier invoice result.')
  return {
    bill: {
      id: readString(value, 'id'), supplierId: readString(value, 'supplierId'), supplierName: readString(value, 'supplierName'),
      supplierReference: readString(value, 'supplierReference'), invoiceDate: readString(value, 'invoiceDate'), dueDate: readNullableString(value, 'dueDate'),
      currencyCode: readString(value, 'currencyCode'), totalMinor: readNumber(value, 'totalMinor'), paidMinor: readNumber(value, 'paidMinor'), creditedMinor: readNumber(value, 'creditedMinor'), balanceMinor: readNumber(value, 'balanceMinor'), createdAt: readString(value, 'createdAt'),
    },
    replayed: value.replayed === true,
  }
}

export async function createBranchSupplierCreditNote(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; idempotencyKey: string
  billId: string; creditReference: string; effectiveDate: string; reason: string; lines: HostedSupplierCreditLineInput[]
}): Promise<{ creditNote: HostedSupplierCreditNote; replayed: boolean }> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/supplier-credit-notes`
  const value = parseEnvelopeData(await requestJson({ ...input, path, method: 'POST', headers: { 'Idempotency-Key': input.idempotencyKey }, body: {
    billId: input.billId, creditReference: input.creditReference, effectiveDate: input.effectiveDate, reason: input.reason, lines: input.lines,
  } }))
  if (!isRecord(value)) throw new Error('The API returned an invalid supplier credit note result.')
  return {
    creditNote: {
      id: readString(value, 'id'), billId: readString(value, 'billId'), supplierId: readString(value, 'supplierId'), supplierName: readString(value, 'supplierName'),
      creditReference: readString(value, 'creditReference'), effectiveDate: readString(value, 'effectiveDate'), reason: readString(value, 'reason'),
      currencyCode: readString(value, 'currencyCode'), totalMinor: readNumber(value, 'totalMinor'), createdAt: readString(value, 'createdAt'),
    },
    replayed: value.replayed === true,
  }
}

export async function recordBranchSupplierPayment(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; idempotencyKey: string
  supplierId: string; paymentMethod: HostedSupplierPayment['paymentMethod']; externalReference: string | null
  amountMinor: number; allocations: HostedSupplierPaymentAllocationInput[]; cashLocationId?: string | null
}): Promise<{ payment: HostedSupplierPayment; replayed: boolean }> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/supplier-payments`
  const value = parseEnvelopeData(await requestJson({ ...input, path, method: 'POST', headers: { 'Idempotency-Key': input.idempotencyKey }, body: {
    supplierId: input.supplierId, paymentMethod: input.paymentMethod, cashLocationId: input.cashLocationId ?? null, externalReference: input.externalReference,
    amountMinor: input.amountMinor, allocations: input.allocations,
  } }))
  if (!isRecord(value) || typeof value.paymentMethod !== 'string' || !isTenderKind(value.paymentMethod)) throw new Error('The API returned an invalid supplier payment result.')
  return {
    payment: {
      id: readString(value, 'id'), supplierId: readString(value, 'supplierId'), supplierName: readString(value, 'supplierName'),
      paymentMethod: value.paymentMethod, externalReference: readNullableString(value, 'externalReference'),
      cashLocationId: readNullableString(value, 'cashLocationId'),
      amountMinor: readNumber(value, 'amountMinor'), currencyCode: readString(value, 'currencyCode'), paidAt: readString(value, 'paidAt'),
    },
    replayed: value.replayed === true,
  }
}

export async function recordStationSupplierCashPayment(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
  lease: HostedLease; key: HostedStationKey; idempotencyKey: string; drawerShiftId: string
  supplierId: string; externalReference: string | null; amountMinor: number
  allocations: HostedSupplierPaymentAllocationInput[]
}): Promise<{ payment: HostedSupplierPayment; replayed: boolean }> {
  const path = `${stationPath(input.tenantId, input.branchId, input.stationId)}/supplier-payments`
  const body = {
    supplierId: input.supplierId, paymentMethod: 'cash' as const, externalReference: input.externalReference,
    amountMinor: input.amountMinor, drawerShiftId: input.drawerShiftId, allocations: input.allocations,
  }
  const value = parseEnvelopeData(await signedHostedMutation({
    ...input, operation: 'supplier.payment', resourceId: input.drawerShiftId,
    path, body, idempotencyKey: input.idempotencyKey,
  }))
  if (!isRecord(value) || typeof value.paymentMethod !== 'string' || !isTenderKind(value.paymentMethod)) throw new Error('The API returned an invalid supplier payment result.')
  return {
    payment: {
      id: readString(value, 'id'), supplierId: readString(value, 'supplierId'), supplierName: readString(value, 'supplierName'),
      paymentMethod: value.paymentMethod, externalReference: readNullableString(value, 'externalReference'),
      cashLocationId: readNullableString(value, 'cashLocationId'),
      amountMinor: readNumber(value, 'amountMinor'), currencyCode: readString(value, 'currencyCode'), paidAt: readString(value, 'paidAt'),
    },
    replayed: value.replayed === true,
  }
}

function tenantBranchPath(tenantId: string, branchId: string): string {
  return `/v1/tenants/${encodeURIComponent(tenantId)}/branches/${encodeURIComponent(branchId)}`
}

function hostedEntityImagePath(input: {
  tenantId: string; branchId: string; entityType: HostedImageEntityType; entityId: string
}): string {
  return `${tenantBranchPath(input.tenantId, input.branchId)}/entities/${input.entityType}/${encodeURIComponent(input.entityId)}/image`
}

async function imageRequestError(response: Response): Promise<ApiError> {
  const payload: unknown = await response.json().catch(() => null)
  const message = isRecord(payload) && isRecord(payload.error) && typeof payload.error.message === 'string'
    ? payload.error.message
    : `The image request failed (${response.status}).`
  return new ApiError(message, response.status)
}

export async function getHostedEntityImage(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
  entityType: HostedImageEntityType; entityId: string
}): Promise<Blob | null> {
  let response: Response
  try {
    response = await fetch(`${input.apiBaseUrl}${hostedEntityImagePath(input)}`, {
      headers: { Authorization: `Bearer ${input.accessToken}`, Accept: 'image/jpeg,image/png,image/webp' },
      cache: 'no-store',
    })
  } catch {
    throw new ApiError('The image could not be loaded. Check the connection and try again.', 0)
  }
  if (response.status === 404) return null
  if (!response.ok) throw await imageRequestError(response)
  const contentType = response.headers.get('Content-Type')?.split(';', 1)[0]
  if (contentType !== 'image/jpeg' && contentType !== 'image/png' && contentType !== 'image/webp') {
    throw new Error('The API returned an unsupported image type.')
  }
  return response.blob()
}

export async function putHostedEntityImage(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
  entityType: HostedImageEntityType; entityId: string; file: File
}): Promise<void> {
  if (input.file.size < 1 || input.file.size > 2 * 1024 * 1024) {
    throw new Error('Choose an image no larger than 2 MiB.')
  }
  if (input.file.type !== 'image/jpeg' && input.file.type !== 'image/png' && input.file.type !== 'image/webp') {
    throw new Error('Choose a PNG, JPEG, or WebP image.')
  }
  let response: Response
  try {
    response = await fetch(`${input.apiBaseUrl}${hostedEntityImagePath(input)}`, {
      method: 'PUT',
      headers: { Authorization: `Bearer ${input.accessToken}`, 'Content-Type': input.file.type },
      body: input.file,
      cache: 'no-store',
    })
  } catch {
    throw new ApiError('The image could not be uploaded. Check the connection and retry.', 0)
  }
  if (!response.ok) throw await imageRequestError(response)
}

export async function deleteHostedEntityImage(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
  entityType: HostedImageEntityType; entityId: string
}): Promise<void> {
  let response: Response
  try {
    response = await fetch(`${input.apiBaseUrl}${hostedEntityImagePath(input)}`, {
      method: 'DELETE',
      headers: { Authorization: `Bearer ${input.accessToken}` },
      cache: 'no-store',
    })
  } catch {
    throw new ApiError('The image could not be removed. Check the connection and retry.', 0)
  }
  if (!response.ok) throw await imageRequestError(response)
}

export async function listBranchProducts(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
}): Promise<BranchProduct[]> {
  const data = parseEnvelopeData(await getJson({ ...input, path: `${tenantBranchPath(input.tenantId, input.branchId)}/products` }))
  if (!Array.isArray(data)) throw new Error('The API returned an invalid branch catalog.')
  return data.map(parseBranchProduct)
}

export async function listStationProducts(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
}): Promise<BranchProduct[]> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/stations/${encodeURIComponent(input.stationId)}/products`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  if (!Array.isArray(data)) throw new Error('The API returned an invalid register catalog.')
  return data.map(parseBranchProduct)
}

export async function listBranchPaymentOptions(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
}): Promise<BranchPaymentOption[]> {
  const data = parseEnvelopeData(await getJson({ ...input, path: `${tenantBranchPath(input.tenantId, input.branchId)}/payment-options` }))
  if (!Array.isArray(data)) throw new Error('The API returned invalid branch payment methods.')
  return data.map(parseBranchPaymentOption)
}

export async function createBranchPaymentOption(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; option: NewBranchPaymentOption
}): Promise<BranchPaymentOption> {
  const data = parseEnvelopeData(await requestJson({
    apiBaseUrl: input.apiBaseUrl, accessToken: input.accessToken,
    path: `${tenantBranchPath(input.tenantId, input.branchId)}/payment-options`,
    method: 'POST', body: input.option,
  }))
  return parseBranchPaymentOption(data)
}

export async function updateBranchPaymentOption(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; option: BranchPaymentOption
}): Promise<BranchPaymentOption> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/payment-options/${encodeURIComponent(input.option.id)}`
  const { option } = input
  const data = parseEnvelopeData(await requestJson({
    apiBaseUrl: input.apiBaseUrl, accessToken: input.accessToken, path, method: 'PATCH',
    body: {
      label: option.label,
      tenderKind: option.tenderKind,
      providerName: option.providerName,
      countryCode: option.countryCode,
      requiresReference: option.requiresReference,
      enabled: option.enabled,
      expectedRevision: option.revision,
    },
  }))
  return parseBranchPaymentOption(data)
}

export async function listPaymentIntegrationRequests(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
}): Promise<PaymentIntegrationRequest[]> {
  const data = parseEnvelopeData(await getJson({ ...input, path: `${tenantBranchPath(input.tenantId, input.branchId)}/payment-integration-requests` }))
  if (!Array.isArray(data)) throw new Error('The API returned invalid payment integration requests.')
  return data.map(parsePaymentIntegrationRequest)
}

export async function createPaymentIntegrationRequest(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
  request: NewPaymentIntegrationRequest; idempotencyKey: string
}): Promise<PaymentIntegrationRequest> {
  const data = parseEnvelopeData(await requestJson({
    apiBaseUrl: input.apiBaseUrl,
    accessToken: input.accessToken,
    path: `${tenantBranchPath(input.tenantId, input.branchId)}/payment-integration-requests`,
    method: 'POST',
    body: input.request,
    headers: { 'Idempotency-Key': input.idempotencyKey },
  }))
  return parsePaymentIntegrationRequest(data)
}

export async function listSupportRequests(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
}): Promise<SupportRequest[]> {
  const data = parseEnvelopeData(await getJson({
    ...input,
    path: `${tenantBranchPath(input.tenantId, input.branchId)}/support-requests`,
  }))
  if (!Array.isArray(data)) throw new Error('The API returned invalid support requests.')
  return data.map(parseSupportRequest)
}

export async function createSupportRequest(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
  request: NewSupportRequest; idempotencyKey: string
}): Promise<SupportRequest> {
  const data = parseEnvelopeData(await requestJson({
    apiBaseUrl: input.apiBaseUrl,
    accessToken: input.accessToken,
    path: `${tenantBranchPath(input.tenantId, input.branchId)}/support-requests`,
    method: 'POST',
    body: input.request,
    headers: { 'Idempotency-Key': input.idempotencyKey },
  }))
  return parseSupportRequest(data)
}

export async function getProductBranchAvailability(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string; productId: string
}): Promise<ProductBranchAvailability[]> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/stations/${encodeURIComponent(input.stationId)}/products/${encodeURIComponent(input.productId)}/availability`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  if (!Array.isArray(data)) throw new Error('The API returned invalid branch availability.')
  return data.map((entry): ProductBranchAvailability => {
    const observedAt = readString(entry, 'observedAt')
    if (Number.isNaN(Date.parse(observedAt))) throw new Error('The API returned an invalid availability timestamp.')
    return {
      branchId: readString(entry, 'branchId'),
      branchCode: readString(entry, 'branchCode'),
      branchName: readString(entry, 'branchName'),
      address: readNullableString(entry, 'address'),
      phone: readNullableString(entry, 'phone'),
      email: readNullableString(entry, 'email'),
      timeZone: readString(entry, 'timeZone'),
      openingHours: parseBranchOpeningHours(entry.openingHours),
      stockQuantity: readNumber(entry, 'stockQuantity'),
      pickupReservedQuantity: readNumber(entry, 'pickupReservedQuantity'),
      outboundCommittedQuantity: readNumber(entry, 'outboundCommittedQuantity'),
      availableToPromise: readNumber(entry, 'availableToPromise'),
      commitmentShortfall: readNumber(entry, 'commitmentShortfall'),
      priceMinor: readNumber(entry, 'priceMinor'),
      currencyCode: readString(entry, 'currencyCode'),
      observedAt,
    }
  })
}

export async function listHostedStockTransfers(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string }): Promise<HostedStockTransfer[]> {
  const data = parseEnvelopeData(await getJson({ ...input, path: `${tenantBranchPath(input.tenantId, input.branchId)}/stock-transfers` }))
  if (!Array.isArray(data)) throw new Error('The API returned an invalid stock transfer list.')
  return data.map(parseStockTransfer)
}

export async function requestHostedStockTransfer(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string; sourceBranchId: string; productId: string; quantity: number; note: string; idempotencyKey: string }): Promise<HostedStockTransfer> {
  const path = `${stationPath(input.tenantId, input.branchId, input.stationId)}/stock-transfers`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'POST', headers: { 'Idempotency-Key': input.idempotencyKey }, body: { sourceBranchId: input.sourceBranchId, note: input.note || null, items: [{ productId: input.productId, quantity: input.quantity }] } }))
  return parseStockTransfer(data)
}

export async function transitionHostedStockTransfer(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; transferId: string; action: 'accept' | 'reject' | 'dispatch' | 'receive' | 'resolve_discrepancy'; revision: number; note: string; items: Array<{ productId: string; quantity: number }>; idempotencyKey: string }): Promise<HostedStockTransfer> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/stock-transfers/${encodeURIComponent(input.transferId)}/events`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'POST', headers: { 'Idempotency-Key': input.idempotencyKey }, body: { action: input.action, expectedRevision: input.revision, note: input.note || null, items: input.items } }))
  return parseStockTransfer(data)
}

export async function listHostedPickupReservations(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string }): Promise<HostedPickupReservation[]> {
  const path = `${stationPath(input.tenantId, input.branchId, input.stationId)}/pickup-reservations`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  if (!Array.isArray(data)) throw new Error('The API returned an invalid pickup reservation list.')
  return data.map(parsePickupReservation)
}

export async function listBranchHostedPickupReservations(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string }): Promise<HostedPickupReservation[]> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/pickup-reservations`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  if (!Array.isArray(data)) throw new Error('The API returned an invalid pickup reservation list.')
  return data.map(parsePickupReservation)
}

export async function createHostedPickupReservation(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; stationId: string
  reservation: NewHostedPickupReservation; idempotencyKey: string
}): Promise<HostedPickupReservation> {
  const path = `${stationPath(input.tenantId, input.branchId, input.stationId)}/pickup-reservations`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'POST', headers: { 'Idempotency-Key': input.idempotencyKey }, body: input.reservation }))
  return parsePickupReservation(data)
}

export async function transitionHostedPickupReservation(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; reservationId: string
  scope: { kind: 'branch' } | { kind: 'station'; stationId: string }
  action: 'ready' | 'cancel'; expectedRevision: number; reason: string | null; idempotencyKey: string
}): Promise<HostedPickupReservation> {
  const base = input.scope.kind === 'branch'
    ? tenantBranchPath(input.tenantId, input.branchId)
    : stationPath(input.tenantId, input.branchId, input.scope.stationId)
  const path = `${base}/pickup-reservations/${encodeURIComponent(input.reservationId)}/events`
  const data = parseEnvelopeData(await requestJson({
    apiBaseUrl: input.apiBaseUrl,
    accessToken: input.accessToken,
    path,
    method: 'POST',
    headers: { 'Idempotency-Key': input.idempotencyKey },
    body: { action: input.action, expectedRevision: input.expectedRevision, reason: input.reason },
  }))
  return parsePickupReservation(data)
}

export async function createBranchProduct(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; product: NewBranchProduct
}): Promise<BranchProduct> {
  const data = parseEnvelopeData(await requestJson({
    ...input,
    path: `${tenantBranchPath(input.tenantId, input.branchId)}/products`,
    method: 'POST',
    body: input.product,
  }))
  return parseBranchProduct(data)
}

export async function listBranchCatalogCandidates(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
}): Promise<CatalogProductIdentity[]> {
  const data = parseEnvelopeData(await getJson({
    ...input,
    path: `${tenantBranchPath(input.tenantId, input.branchId)}/product-candidates`,
  }))
  if (!Array.isArray(data)) throw new Error('The API returned an invalid product candidate list.')
  return data.map(parseCatalogProductIdentity)
}

export async function attachBranchCatalogProduct(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; productId: string; settings: AttachBranchProduct
}): Promise<BranchProduct> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/products/${encodeURIComponent(input.productId)}/attach`
  const data = parseEnvelopeData(await requestJson({
    ...input,
    path,
    method: 'POST',
    body: input.settings,
  }))
  return parseBranchProduct(data)
}

export async function updateBranchProductSettings(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; productId: string
  priceMinor: number; taxRateBps: number; reorderPoint: number; targetStockQuantity: number; expectedRevision: number
}): Promise<BranchProduct> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/products/${encodeURIComponent(input.productId)}/settings`
  const body = {
    priceMinor: input.priceMinor,
    taxRateBps: input.taxRateBps,
    reorderPoint: input.reorderPoint,
    targetStockQuantity: input.targetStockQuantity,
    expectedRevision: input.expectedRevision,
  }
  const data = parseEnvelopeData(await requestJson({ apiBaseUrl: input.apiBaseUrl, accessToken: input.accessToken, path, method: 'PATCH', body }))
  return parseBranchProduct(data)
}

export async function setBranchProductActive(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; productId: string; isActive: boolean
}): Promise<BranchProduct> {
  const path = `${tenantBranchPath(input.tenantId, input.branchId)}/products/${encodeURIComponent(input.productId)}`
  const data = parseEnvelopeData(await requestJson({ ...input, path, method: 'PATCH', body: { isActive: input.isActive } }))
  return parseBranchProduct(data)
}

export async function updateProductIdentity(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; productId: string; sku: string
  barcode: string | null; name: string; description: string | null; category: string | null; expectedRevision: number
}): Promise<CatalogProductIdentity> {
  const path = `/v1/tenants/${encodeURIComponent(input.tenantId)}/products/${encodeURIComponent(input.productId)}`
  const data = parseEnvelopeData(await requestJson({
    apiBaseUrl: input.apiBaseUrl,
    accessToken: input.accessToken,
    path,
    method: 'PATCH',
    body: {
      sku: input.sku,
      barcode: input.barcode,
      name: input.name,
      description: input.description,
      category: input.category,
      expectedRevision: input.expectedRevision,
    },
  }))
  return parseCatalogProductIdentity(data)
}

export async function listTenantAccess(input: { apiBaseUrl: string; accessToken: string }): Promise<TenantMembership[]> {
  const data = parseEnvelopeData(await getJson({ ...input, path: '/v1/me/tenants' }))
  if (!Array.isArray(data)) throw new Error('The API returned an invalid tenant list.')
  return data.map(parseMembership)
}

export async function listHostedStaff(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string
}): Promise<HostedStaffMember[]> {
  const data = parseEnvelopeData(await getJson({
    ...input,
    path: `/v1/tenants/${encodeURIComponent(input.tenantId)}/staff`,
  }))
  if (!Array.isArray(data)) throw new Error('The API returned an invalid staff directory.')
  return data.map((value) => {
    if (!isRecord(value) || !Array.isArray(value.roles) || !value.roles.every((role) => typeof role === 'string')) {
      throw new Error('The API returned an invalid staff profile.')
    }
    return {
      membershipId: readString(value, 'membershipId'),
      displayName: typeof value.displayName === 'string' ? value.displayName : null,
      roles: value.roles,
    }
  })
}

function parseHostedWorkSchedule(value: unknown): HostedWorkSchedule {
  if (!isRecord(value) || !Array.isArray(value.weekdays)) {
    throw new Error('The API returned an invalid attendance schedule.')
  }
  const weekdays = value.weekdays
  if (!weekdays.every((day): day is number => typeof day === 'number' && Number.isInteger(day) && day >= 0 && day <= 6)) {
    throw new Error('The API returned an invalid attendance schedule.')
  }
  return {
    membershipId: readString(value, 'membershipId'), branchId: readString(value, 'branchId'),
    timezone: readString(value, 'timezone'), weekdays,
    startTime: readString(value, 'startTime'), endTime: readString(value, 'endTime'),
    enabled: value.enabled === true, revision: readNumber(value, 'revision'),
  }
}

function parseHostedWorkPeriod(value: unknown): HostedWorkPeriod {
  if (!isRecord(value)) throw new Error('The API returned an invalid attendance period.')
  const scheduleRevision = value.scheduleRevision
  if (scheduleRevision !== null && (!Number.isInteger(scheduleRevision) || typeof scheduleRevision !== 'number')) throw new Error('The API returned an invalid attendance schedule revision.')
  const endedAt = value.endedAt
  const endReason = value.endReason
  if (endedAt !== null && typeof endedAt !== 'string') throw new Error('The API returned an invalid attendance end time.')
  if (endReason !== null && typeof endReason !== 'string') throw new Error('The API returned an invalid attendance end reason.')
  return {
    id: readString(value, 'id'), branchId: readString(value, 'branchId'), membershipId: readString(value, 'membershipId'),
    scheduleRevision, timezone: readString(value, 'timezone'), scheduledStartAt: readString(value, 'scheduledStartAt'),
    dueAt: readString(value, 'dueAt'), startedAt: readString(value, 'startedAt'), endedAt, endReason,
    revision: readNumber(value, 'revision'),
  }
}

function attendancePath(input: { tenantId: string; branchId: string }): string {
  return `${tenantBranchPath(input.tenantId, input.branchId)}/attendance`
}

export async function getMyHostedAttendance(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string }): Promise<HostedMyAttendance> {
  const value: unknown = parseEnvelopeData(await getJson({ ...input, path: `${attendancePath(input)}/me` }))
  if (!isRecord(value)) throw new Error('The API returned invalid attendance status.')
  return {
    serverNow: readString(value, 'serverNow'),
    schedule: value.schedule === null ? null : parseHostedWorkSchedule(value.schedule),
    currentPeriod: value.currentPeriod === null ? null : parseHostedWorkPeriod(value.currentPeriod),
  }
}

async function attendanceClockAction(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; action: 'clock-in' | 'clock-out' }): Promise<HostedWorkPeriod> {
  const value: unknown = parseEnvelopeData(await requestJson({
    ...input, path: `${attendancePath(input)}/me/${input.action}`, method: 'POST',
    headers: { 'Idempotency-Key': crypto.randomUUID() }, body: {},
  }))
  return parseHostedWorkPeriod(value)
}

export function clockInHostedAttendance(input: Omit<Parameters<typeof attendanceClockAction>[0], 'action'>): Promise<HostedWorkPeriod> {
  return attendanceClockAction({ ...input, action: 'clock-in' })
}

export function clockOutHostedAttendance(input: Omit<Parameters<typeof attendanceClockAction>[0], 'action'>): Promise<HostedWorkPeriod> {
  return attendanceClockAction({ ...input, action: 'clock-out' })
}

export async function listHostedAttendanceStaff(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string }): Promise<HostedAttendanceStaff[]> {
  const value: unknown = parseEnvelopeData(await getJson({ ...input, path: `${attendancePath(input)}/staff` }))
  if (!Array.isArray(value)) throw new Error('The API returned an invalid attendance staff list.')
  return value.map((person) => {
    if (!isRecord(person) || !Array.isArray(person.roles) || !person.roles.every((role) => typeof role === 'string')) throw new Error('The API returned an invalid attendance staff profile.')
    return { membershipId: readString(person, 'membershipId'), displayName: typeof person.displayName === 'string' ? person.displayName : null, roles: person.roles }
  })
}

export async function listHostedAttendanceSchedules(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string }): Promise<HostedWorkSchedule[]> {
  const value: unknown = parseEnvelopeData(await getJson({ ...input, path: `${attendancePath(input)}/schedules` }))
  if (!Array.isArray(value)) throw new Error('The API returned an invalid attendance schedule list.')
  return value.map(parseHostedWorkSchedule)
}

export async function saveHostedAttendanceSchedule(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; membershipId: string
  weekdays: number[]; startTime: string; endTime: string; enabled: boolean; expectedRevision: number
}): Promise<HostedWorkSchedule> {
  const path = `${attendancePath(input)}/schedules/${encodeURIComponent(input.membershipId)}`
  const value = parseEnvelopeData(await requestJson({
    ...input, path, method: 'PUT',
    body: { weekdays: input.weekdays, startTime: input.startTime, endTime: input.endTime, enabled: input.enabled, expectedRevision: input.expectedRevision },
  }))
  return parseHostedWorkSchedule(value)
}

export async function listHostedAttendancePeriods(input: { apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string; limit?: number }): Promise<HostedWorkPeriod[]> {
  const query = new URLSearchParams({ limit: String(input.limit ?? 50) })
  const value: unknown = parseEnvelopeData(await getJson({ ...input, path: `${attendancePath(input)}/periods?${query}` }))
  if (!Array.isArray(value)) throw new Error('The API returned an invalid attendance period list.')
  return value.map(parseHostedWorkPeriod)
}

export async function getTenantStationCapacity(input: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
}): Promise<StationCapacitySnapshot> {
  const path = `/v1/tenants/${encodeURIComponent(input.tenantId)}/station-capacity`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  if (!isRecord(data) || typeof data.entitlementActive !== 'boolean') {
    throw new Error('The API returned invalid station capacity.')
  }
  return {
    entitlementActive: data.entitlementActive,
    stationLimit: readNullableNumber(data, 'stationLimit'),
    activeStations: readNumber(data, 'activeStations'),
    availableSlots: readNumber(data, 'availableSlots'),
  }
}

export async function getStationPermissions(input: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  branchId: string
  stationId: string
}): Promise<StationPermissionSnapshot> {
  const path = `/v1/tenants/${encodeURIComponent(input.tenantId)}/branches/${encodeURIComponent(input.branchId)}/stations/${encodeURIComponent(input.stationId)}/permissions`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  return {
    tenantId: readString(data, 'tenantId'),
    branchId: readString(data, 'branchId'),
    stationId: readString(data, 'stationId'),
    permissions: readStringArray(data, 'permissions'),
    tenantPermissions: readStringArray(data, 'tenantPermissions'),
    branchPermissions: readStringArray(data, 'branchPermissions'),
  }
}

export async function getBranchPermissions(input: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  branchId: string
}): Promise<BranchPermissionSnapshot> {
  const path = `/v1/tenants/${encodeURIComponent(input.tenantId)}/branches/${encodeURIComponent(input.branchId)}/permissions`
  const data = parseEnvelopeData(await getJson({ ...input, path }))
  return {
    tenantId: readString(data, 'tenantId'),
    branchId: readString(data, 'branchId'),
    permissions: readStringArray(data, 'permissions'),
    tenantPermissions: readStringArray(data, 'tenantPermissions'),
  }
}

const BRANCH_WEEKDAYS = ['monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday', 'sunday'] as const

function parseBranchOpeningHours(value: unknown): BranchOpeningHours {
  if (!isRecord(value)) throw new Error('The API returned invalid branch opening hours.')
  const result = {} as BranchOpeningHours
  for (const day of BRANCH_WEEKDAYS) {
    const intervals = value[day]
    if (!Array.isArray(intervals) || intervals.length > 4) throw new Error('The API returned invalid branch opening hours.')
    result[day] = intervals.map((interval) => {
      if (!isRecord(interval) || typeof interval.open !== 'string' || typeof interval.close !== 'string') {
        throw new Error('The API returned invalid branch opening hours.')
      }
      return { open: interval.open, close: interval.close }
    })
  }
  return result
}

function parseHostedBranchProfile(data: unknown): HostedBranchProfile {
  if (!isRecord(data)) throw new Error('The API returned invalid branch details.')
  return {
    address: readNullableString(data, 'address'),
    phone: readNullableString(data, 'phone'),
    email: readNullableString(data, 'email'),
    openingHours: parseBranchOpeningHours(data.openingHours),
    settingsRevision: readNumber(data, 'settingsRevision'),
  }
}

export async function getHostedBranchProfile(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
}): Promise<HostedBranchProfile> {
  const path = `/v1/tenants/${encodeURIComponent(input.tenantId)}/branches/${encodeURIComponent(input.branchId)}/profile`
  return parseHostedBranchProfile(parseEnvelopeData(await getJson({ ...input, path })))
}

export async function updateHostedBranchProfile(input: {
  apiBaseUrl: string; accessToken: string; tenantId: string; branchId: string
  address: string | null; phone: string | null; email: string | null; openingHours: BranchOpeningHours; expectedRevision: number
}): Promise<HostedBranchProfile> {
  const path = `/v1/tenants/${encodeURIComponent(input.tenantId)}/branches/${encodeURIComponent(input.branchId)}/profile`
  const data = parseEnvelopeData(await requestJson({
    ...input,
    path,
    method: 'PUT',
    body: {
      address: input.address,
      phone: input.phone,
      email: input.email,
      openingHours: input.openingHours,
      expectedRevision: input.expectedRevision,
    },
  }))
  return parseHostedBranchProfile(data)
}
