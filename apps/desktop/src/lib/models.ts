// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

export type PaymentMethod = "cash" | "card" | "mobile_money" | "other";

export type BootstrapStatus = {
  needsSetup: boolean;
  businessName: string | null;
  storeName: string | null;
  currencyCode: string | null;
};

export type InitialSetupInput = {
  businessName: string;
  storeName: string;
  fullName: string;
  username: string;
  password: string;
  currencyCode: string;
};

export type SessionUser = {
  userId: string;
  businessId: string;
  storeId: string;
  fullName: string;
  username: string;
  roleKey: string;
  roleName: string;
  workspaceMode: "cashier" | "management";
  isOwner: boolean;
  permissions: string[];
  businessName: string;
  storeName: string;
  currencyCode: string;
};

export type LocalBackup = {
  id: string;
  createdAt: string;
  sizeBytes: number;
  retentionWarning?: string;
};

export type StorePreferences = {
  cashierStartAtRegister: boolean;
  cashierAutoFullscreen: boolean;
  customerDisplayEnabled: boolean;
  customerDisplayFullscreen: boolean;
  customerDisplayMonitorKey: string | null;
  customerDisplayIdleMessage: string;
  receiptHeaderText: string;
  receiptFooterText: string;
  receiptPaperWidthMm: 58 | 80;
};

export type StoreProfile = {
  businessName: string;
  storeName: string;
  address: string;
  phone: string;
  currencyCode: string;
  taxRateBps: number;
};

export type Product = {
  id: string;
  sku: string;
  barcode: string | null;
  name: string;
  description: string | null;
  categoryId: string | null;
  categoryName: string | null;
  brandId: string | null;
  brandName: string | null;
  tagIds: string[];
  tags: string[];
  supplierId: string | null;
  priceMinor: number;
  costMinor: number | null;
  stockQuantity: number;
  reorderPoint: number;
  targetStockQuantity: number;
  isActive: boolean;
};

export type ProductInput = Omit<Product, "categoryName" | "brandName" | "tags" | "id"> & { id?: string };

export type ProductLabel = { id: string; name: string; isActive: boolean };
export type ProductLabelInput = { id?: string; name: string; isActive: boolean };

export type StockCountSummary = {
  id: string;
  title: string;
  status: "draft" | "posted";
  revision: number;
  startedAt: string;
  postedAt: string | null;
  itemCount: number;
  countedCount: number;
};

export type StockCountLine = {
  productId: string;
  sku: string;
  productName: string;
  expectedQuantity: number;
  countedQuantity: number | null;
  countedAt: string | null;
};

export type StockCountDetails = Omit<StockCountSummary, "itemCount" | "countedCount"> & { lines: StockCountLine[] };
export type StockCountList = { counts: StockCountSummary[] };

export type ImageEntityType = "product" | "customer" | "supplier" | "staff";
export type EntityImage = { dataUrl: string; updatedAt: string };

export type Category = {
  id: string;
  name: string;
  description: string | null;
};

export type CategoryInput = {
  id?: string;
  name: string;
  description: string | null;
};

export type Customer = {
  id: string;
  name: string;
  email: string | null;
  phone: string | null;
  address: string | null;
  notes: string | null;
};

export type CustomerInput = Omit<Customer, "id"> & { id?: string };

export type Supplier = {
  id: string;
  name: string;
  contactPerson: string | null;
  email: string | null;
  phone: string | null;
  address: string | null;
  isActive: boolean;
};

export type SupplierInput = Omit<Supplier, "id"> & { id?: string };

export type CheckoutInput = {
  paymentOptionId?: string | null;
  paymentReference?: string;
  customerId: string | null;
  lines: { productId: string; quantity: number }[];
  discountMinor: number;
  paymentMethod: PaymentMethod;
  amountTenderedMinor: number;
  ticketId?: string;
  workspaceRevision?: number;
};

export type RegisterTicketLine = {
  productId: string;
  quantity: number;
  acceptedName: string;
  acceptedSku: string;
  acceptedUnitPriceMinor: number;
};

export type RegisterTicket = {
  paymentOptionId: string | null;
  paymentReference: string;
  id: string;
  ticketNumber: string;
  creatorUserId: string;
  creatorName: string;
  lastEditorUserId: string;
  lastEditorName: string;
  customerId: string | null;
  lines: RegisterTicketLine[];
  discountMinor: number;
  discountReason: string | null;
  discountRequestedByUserId: string | null;
  discountApprovalId: string | null;
  discountApproverName: string | null;
  discountApprovalExpiresAt: string | null;
  paymentMethod: PaymentMethod;
  amountTenderedMinor: number;
  createdAt: string;
  updatedAt: string;
};

export type RegisterWorkspace = {
  schemaVersion: number;
  revision: number;
  active: RegisterTicket;
  held: RegisterTicket[];
  lastCompleted: { ticketId: string; customerId: string | null; receipt: SaleReceipt } | null;
};

export type RegisterAction =
  | { type: "addProduct"; productId: string }
  | { type: "setQuantity"; productId: string; quantity: number }
  | { type: "removeLines"; productIds: string[] }
  | { type: "clearActive" }
  | { type: "setDiscount"; discountMinor: number; reason?: string | null }
  | { type: "setPayment"; paymentMethod: PaymentMethod; amountTenderedMinor: number; paymentOptionId?: string | null; paymentReference?: string }
  | { type: "setPaymentReference"; reference: string }
  | { type: "selectCustomer"; customerId: string | null }
  | { type: "holdActive" }
  | { type: "resumeHeld"; ticketId: string; holdActive: boolean }
  | { type: "discardHeld"; ticketId: string }
  | { type: "acceptCurrentPrices" };

export type StaffProfile = {
  userId: string;
  fullName: string;
  roleKey: string;
  roleName: string;
  hasPin: boolean;
};

export type DiscountApprover = { userId: string; fullName: string; roleName: string };

export type DiscountPreview = {
  ticketId: string;
  workspaceRevision: number;
  currencyCode: string;
  reason: string;
  lines: { name: string; quantity: number; unitPriceMinor: number; lineTotalMinor: number }[];
  subtotalMinor: number;
  discountMinor: number;
  taxMinor: number;
  totalMinor: number;
};

export type StaffAccount = StaffProfile & {
  username: string;
  isActive: boolean;
  isOwner: boolean;
};

export type StaffRole = {
  roleKey: string;
  displayName: string;
  permissions: string[];
  workspaceMode: "cashier" | "management";
  isSystem: boolean;
  revision: number;
  activeUserCount: number;
};

export type PermissionDefinition = {
  key: string;
  label: string;
  group: string;
  description: string;
};

export type StaffRolesView = {
  roles: StaffRole[];
  permissions: PermissionDefinition[];
  ownerUserId: string | null;
};

export type SaveStaffRoleInput = {
  roleKey?: string | null;
  templateKey: "cashier" | "manager" | "admin";
  displayName: string;
  permissions: string[];
  workspaceMode: "cashier" | "management";
  expectedRevision?: number | null;
  password: string;
};

export type CreateStaffMemberInput = {
  fullName: string;
  username: string;
  password: string;
  pin: string;
  roleKey: string;
};

export type SaleTotals = {
  subtotalMinor: number;
  discountMinor: number;
  taxMinor: number;
  totalMinor: number;
};

export type SaleReceipt = {
  paymentDetail?: { label: string; provider: string; terminalId: string; reference: string; confirmationSource: "cashier_cash" | "manual_external" } | null;
  id: string;
  receiptNumber: string;
  createdAt: string;
  cashierName: string;
  items: {
    productId: string;
    sku: string;
    name: string;
    quantity: number;
    unitPriceMinor: number;
    lineTotalMinor: number;
  }[];
  totals: SaleTotals;
  paymentMethod: PaymentMethod;
  amountTenderedMinor: number;
  changeDueMinor: number;
};

export type SaleSummary = {
  id: string;
  receiptNumber: string;
  createdAt: string;
  cashierName: string;
  itemCount: number;
  totalMinor: number;
  status: "completed" | "partially_refunded" | "refunded" | "voided";
  paymentMethod: PaymentMethod | "multiple";
  stationKey: string | null;
  stationNameSnapshot: string;
};

export type RefundableSaleItem = {
  saleItemId: string;
  productId: string;
  sku: string;
  name: string;
  soldQuantity: number;
  refundedQuantity: number;
  remainingQuantity: number;
  lineSubtotalMinor: number;
  lineDiscountMinor: number;
  lineTaxMinor: number;
  refundableTotalMinor: number;
};

export type RefundReceipt = {
  id: string;
  saleId: string;
  receiptNumber: string;
  kind: "refund" | "void";
  totalMinor: number;
  reason: string;
  paymentMethod: PaymentMethod;
  paymentReference: string | null;
  createdAt: string;
  recordedBy: string;
  stationKey: string | null;
  stationNameSnapshot: string;
};

export type SaleRefundDetail = {
  saleId: string;
  receiptNumber: string;
  createdAt: string;
  cashierName: string;
  status: SaleSummary["status"];
  totalMinor: number;
  items: RefundableSaleItem[];
  refunds: RefundReceipt[];
};

export type SalesQuery = {
  fromUtc?: string;
  toUtcExclusive?: string;
  search?: string;
  stationKey?: string;
  offset: number;
  pageSize: number;
  snapshotRowId?: number;
};

export type SalesQueryResult = {
  rows: SaleSummary[];
  offset: number;
  pageSize: number;
  matchedCount: number;
  completedCount: number;
  completedSubtotalMinor: number;
  completedDiscountMinor: number;
  completedTaxMinor: number;
  completedTotalMinor: number;
  paymentSummary: { method: string; saleCount: number; totalMinor: number }[];
  snapshotRowId: number;
};

export type ProfitReport = {
  stationKey: string | null;
  saleCount: number;
  unassignedSalesCount: number;
  unassignedSalesRevenueMinor: number;
  netRevenueBeforeTaxMinor: number;
  taxCollectedMinor: number;
  customerTotalMinor: number;
  costOfGoodsMinor: number;
  costedUnits: number;
  uncostedUnits: number;
  directExpensesMinor: number;
  storeOverheadExpensesMinor: number;
  legacyUnassignedExpensesMinor: number;
  operatingExpensesMinor: number;
  grossProfitMinor: number | null;
  stationContributionMinor: number | null;
  netOperatingProfitMinor: number | null;
};

export type AccountingCoverage = {
  salesCount: number;
  unpostedSalesCount: number;
  refundsCount: number;
  unpostedRefundsCount: number;
  expensesCount: number;
  unpostedExpensesCount: number;
  purchasesCount: number;
  unpostedPurchasesCount: number;
  supplierBillsCount: number;
  unpostedSupplierBillsCount: number;
  supplierPaymentsCount: number;
  unpostedSupplierPaymentsCount: number;
  partialJournalCount: number;
};

export type TrialBalanceRow = {
  code: string;
  name: string;
  accountType: "asset" | "liability" | "equity" | "revenue" | "contra_revenue" | "expense";
  debitMinor: number;
  creditMinor: number;
  balanceMinor: number;
  journalCount: number;
};

export type TrialBalance = {
  rows: TrialBalanceRow[];
  totalDebitsMinor: number;
  totalCreditsMinor: number;
  isBalanced: boolean;
  coverage: AccountingCoverage;
};

export type AccountingPeriodClosure = {
  id: string;
  businessId: string;
  storeId: string;
  fromUtc: string;
  toUtcExclusive: string;
  timezone: string;
  currencyCode: string;
  snapshotJson: string;
  snapshotSha256: string;
  closedBy: string;
  closedAt: string;
};

export type CashOpeningBalance = {
  amountMinor: number;
  currencyCode: string;
  effectiveAt: string;
  reason: string;
  recordedAt: string;
};

export type DrawerShift = {
  id: string;
  stationKey: string;
  stationName: string;
  cashierName: string;
  openedAt: string;
  openingCashMinor: number;
  cashSalesMinor: number;
  paidInMinor: number;
  paidOutMinor: number;
  cashDropsMinor: number;
  expectedCashMinor: number;
  movementCount: number;
};

export type DrawerReport = {
  shiftId: string;
  stationKey: string;
  stationName: string;
  cashierName: string;
  openedAt: string;
  closedAt: string | null;
  openingCashMinor: number;
  cashSalesMinor: number;
  paidInMinor: number;
  expenseReimbursementPaidInMinor: number;
  paidOutMinor: number;
  expensePaidOutMinor: number;
  supplierPaymentPaidOutMinor: number;
  cashDropsMinor: number;
  expectedCashMinor: number;
  countedCashMinor: number | null;
  differenceMinor: number | null;
  closingTransferId: string | null;
  closingTransferMinor: number;
  retainedCashMinor: number;
  movementCount: number;
  completedSaleCount: number;
  isClosed: boolean;
};

export type RegisterStation = {
  stationKey: string;
  displayName: string;
  isActive: boolean;
  isCurrent: boolean;
};

export type StationWorkspace = {
  currentStationKey: string;
  currentDisplayName: string;
  stations: RegisterStation[];
};

export type DrawerHistoryPage = {
  rows: DrawerReport[];
  matchedCount: number;
  offset: number;
  snapshotRowId: number;
};

export type DashboardSummary = {
  todaySalesMinor: number;
  todaySalesCount: number;
  lowStockCount: number;
  productCount: number;
};

export type AuditEventSummary = {
  id: string;
  action: string;
  entityType: string;
  entityId: string | null;
  actorName: string | null;
  createdAt: string;
};

export type AuditEventQuery = {
  search?: string;
  offset: number;
  pageSize: number;
  snapshotRowId?: number;
};

export type AuditEventPage = {
  rows: AuditEventSummary[];
  matchedCount: number;
  offset: number;
  pageSize: number;
  snapshotRowId: number;
};

export type ExpensePaymentMethod = "cash" | "mobile_money" | "bank_transfer" | "other";

export type CashLocation = {
  id: string;
  name: string;
  locationType: "register" | "safe" | "petty_cash" | "other";
  stationKey: string | null;
  isActive: boolean;
  openingRecorded: boolean;
  balanceMinor: number;
  currencyCode: string;
};

export type CashLocationWorkspace = {
  locations: CashLocation[];
  pendingVariances: CashLocationVariance[];
  trackedCashMinor: number;
  ledgerCashMinor: number;
  varianceMinor: number;
  allLocationsInitialized: boolean;
};

export type CashLocationVariance = {
  eventId: string;
  locationName: string;
  varianceType: "shift_open_variance" | "close_variance" | string;
  direction: "in" | "out";
  amountMinor: number;
  note: string;
  effectiveAt: string;
};

export type CashLocationOption = {
  id: string;
  name: string;
  locationType: CashLocation["locationType"];
  stationKey: string | null;
  balanceMinor: number;
  openingRecorded: boolean;
};

export type Expense = {
  id: string;
  category: string;
  description: string | null;
  amountMinor: number;
  paymentMethod: ExpensePaymentMethod;
  cashOrigin: "drawer" | "other_cash" | "legacy_unknown" | "not_applicable";
  cashSourceNote: string | null;
  costCenterKind: "station_direct" | "store_overhead" | "legacy_unassigned";
  costCenterStationKey: string | null;
  costCenterStationNameSnapshot: string | null;
  occurredAt: string;
  createdAt: string;
  recordedBy: string;
};

export type ExpenseQuery = {
  fromUtc?: string;
  toUtcExclusive?: string;
  search?: string;
  offset: number;
  pageSize: number;
  snapshotRowId?: number;
  reimbursementSnapshotRowId?: number;
  reclassificationSnapshotRowId?: number;
};

export type ExpenseQueryResult = {
  rows: ExpenseReportRow[];
  offset: number;
  pageSize: number;
  matchedCount: number;
  totalMinor: number;
  snapshotRowId: number;
  reimbursementSnapshotRowId: number;
  reclassificationSnapshotRowId: number;
};

export type ExpenseReportRow = {
  id: string;
  expenseId: string;
  entryType: "expense" | "reimbursement";
  category: string;
  description: string | null;
  amountMinor: number;
  paymentMethod: string;
  cashOrigin: string;
  cashSourceNote: string | null;
  costCenterKind: "station_direct" | "store_overhead" | "legacy_unassigned";
  costCenterStationKey: string | null;
  costCenterStationNameSnapshot: string | null;
  costCenterRevision: number;
  occurredAt: string;
  createdAt: string;
  recordedBy: string;
  correctionReason: string | null;
  externalReference: string | null;
  canReverse: boolean;
  remainingMinor: number;
};

export type ReclassifyExpenseCostCenterInput = {
  expenseId: string;
  expectedRevision: number;
  costCenterKind: "station_direct" | "store_overhead" | "legacy_unassigned";
  stationKey: string | null;
  reason: string;
  idempotencyKey: string;
};

export type ExpenseCostCenterReclassification = {
  id: string;
  expenseId: string;
  revision: number;
  previousKind: "station_direct" | "store_overhead" | "legacy_unassigned";
  previousStationKey: string | null;
  previousStationNameSnapshot: string | null;
  nextKind: "station_direct" | "store_overhead" | "legacy_unassigned";
  nextStationKey: string | null;
  nextStationNameSnapshot: string | null;
  reason: string;
  recordedAt: string;
  actorUserId: string;
  actorName: string;
};

export type ExpenseCostCenterHistory = {
  currentRevision: number;
  currentKind: "station_direct" | "store_overhead" | "legacy_unassigned";
  currentStationKey: string | null;
  currentStationNameSnapshot: string | null;
  rows: ExpenseCostCenterReclassification[];
  reimbursements: { amountMinor: number; returnedAt: string; recordedBy: string }[];
};

export type ExpenseReturnMethod = "cash" | "card" | "mobile_money" | "bank_transfer" | "other";

export type ExpenseReimbursementInput = {
  expenseId: string;
  amountMinor: number;
  returnMethod: ExpenseReturnMethod;
  reason: string;
  returnReference: string | null;
  cashLocationId: string | null;
  cashShiftId: string | null;
  idempotencyKey: string;
};

export type ExpenseReimbursement = {
  id: string;
  expenseId: string;
  amountMinor: number;
  returnMethod: ExpenseReturnMethod;
  returnReference: string | null;
  reason: string;
  cashLocationId: string | null;
  cashShiftId: string | null;
  returnedAt: string;
  recordedBy: string;
};

export type ExpenseInput = {
  category: string;
  description: string | null;
  amountMinor: number;
  paymentMethod: ExpensePaymentMethod;
  cashOrigin: "drawer" | "other_cash" | "not_applicable";
  cashSourceNote: string | null;
  cashLocationId: string | null;
  costCenterKind: "station_direct" | "store_overhead";
  costCenterStationKey: string | null;
  idempotencyKey: string;
};

export type PurchaseSummary = {
  id: string;
  reference: string | null;
  supplierName: string | null;
  receivedAt: string;
  totalMinor: number;
  itemCount: number;
  itemsSummary: string;
  recordedBy: string;
};

export type PurchaseQuery = {
  fromUtc?: string;
  toUtcExclusive?: string;
  search?: string;
  offset: number;
  pageSize: number;
  snapshotRowId?: number;
};

export type PurchaseQueryResult = {
  rows: PurchaseSummary[];
  offset: number;
  pageSize: number;
  matchedCount: number;
  totalMinor: number;
  snapshotRowId: number;
};

export type PurchaseInput = {
  purchaseOrderId?: string | null;
  idempotencyKey?: string | null;
  supplierId: string | null;
  reference: string | null;
  lines: { productId: string; quantity: number; unitCostMinor: number; purchaseOrderLineId?: string | null }[];
};

export type PurchaseOrderLine = {
  id: string;
  productId: string;
  productName: string;
  sku: string;
  orderedQuantity: number;
  receivedQuantity: number;
  expectedUnitCostMinor: number;
};

export type PurchaseOrderSummary = {
  id: string;
  orderNumber: string;
  supplierId: string | null;
  supplierName: string | null;
  reference: string | null;
  status: "draft" | "placed" | "partially_received" | "received" | "cancelled";
  revision: number;
  placedAt: string | null;
  updatedAt: string;
  totalEstimatedMinor: number;
  lines: PurchaseOrderLine[];
};

export type PurchaseOrderInput = {
  id?: string | null;
  revision?: number | null;
  supplierId: string | null;
  reference: string | null;
  lines: { productId: string; quantity: number; expectedUnitCostMinor: number }[];
};

export type PurchaseOrderTransitionInput = { id: string; revision: number };

export type SupplierBillingWorkspace = {
  unbilledReceipts: {
    purchaseId: string;
    supplierId: string;
    reference: string | null;
    receivedAt: string;
    totalMinor: number;
    unbilledMinor: number;
  }[];
  openBills: {
    billId: string;
    supplierId: string;
    supplierName: string;
    supplierReference: string;
    invoiceDate: string;
    dueDate: string | null;
    currencyCode: string;
    totalMinor: number;
    paidMinor: number;
    creditedMinor: number;
    outstandingMinor: number;
  }[];
  returnableItems: {
    billId: string;
    purchaseId: string;
    purchaseReference: string | null;
    purchaseItemId: string;
    productId: string;
    productName: string;
    unitCostMinor: number;
    maxQuantity: number;
  }[];
};

export type SupplierBillingRecord = {
  id: string;
  amountMinor: number;
  currencyCode: string;
  replayed: boolean;
};

export type MatchSupplierBillInput = {
  supplierId: string;
  supplierReference: string;
  invoiceDate: string;
  dueDate: string | null;
  totalMinor: number;
  idempotencyKey: string;
  allocations: { purchaseId: string; amountMinor: number }[];
};

export type CreateSupplierCreditNoteInput = {
  billId: string;
  creditReference: string;
  effectiveDate: string;
  reason: string;
  idempotencyKey: string;
  returns: { purchaseId: string; purchaseItemId: string; quantity: number }[];
};

export type RecordSupplierPaymentInput = {
  supplierId: string;
  paymentMethod: "cash" | "card" | "mobile_money" | "bank_transfer" | "other";
  cashOrigin: "drawer" | "other_cash" | "not_applicable";
  cashSourceNote: string | null;
  cashLocationId: string | null;
  externalReference: string | null;
  amountMinor: number;
  idempotencyKey: string;
  allocations: { billId: string; amountMinor: number }[];
};
