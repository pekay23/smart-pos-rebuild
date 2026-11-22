// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { FormEvent } from 'react'
import { Barcode, Check, CircleAlert, Clock3, History, LockKeyhole, Minus, PackageCheck, Plus, Printer, RefreshCw, Search, ShieldCheck, ShoppingBag, Store, Trash2, Wallet, X } from 'lucide-react'
import {
  ApiError,
  acquireHostedCashierLease,
  activateHostedStation,
  attachStationSaleCustomer,
  claimBranchHeldTicket,
  clearBranchHeldTicketCheckoutAttempt,
  closeHostedDrawer,
  commitHostedSale,
  createHostedPickupReservation,
  createBranchHeldTicket,
  createStationHostedCustomer,
  getHostedDrawerHistory,
  getHostedDrawerShift,
  getHostedStationReceipt,
  getHostedStationSalesHistory,
  getHostedSaleRefundDetail,
  getStationHostedCustomerCreditPosition,
  listStationHostedOpenReceivables,
  getHostedXReport,
  getBranchSupplierBilling,
  getProductBranchAvailability,
  heartbeatHostedCashierLease,
  listBranchPaymentOptions,
  listBranchHeldTickets,
  listHostedCashDropDestinations,
  listStationProducts,
  listStationHostedCustomers,
  openHostedDrawer,
  recordHostedDrawerMovement,
  recordStationCashExpense,
  recordStationHostedCashCollection,
  recordStationSupplierCashPayment,
  prepareBranchHeldTicketCheckout,
  releaseBranchHeldTicket,
  releaseHostedCashierLease,
  refundHostedSale,
  requestHostedStockTransfer,
  saveBranchHeldTicket,
} from './api'
import type { BranchPaymentOption, BranchProduct, HostedCashDropDestination, HostedCustomer, HostedCustomerCreditPosition, HostedDrawerReport, HostedExpenseCategory, HostedExpenseInput, HostedHeldTicket, HostedHeldTicketPayload, HostedLease, HostedOpenReceivableInvoice, HostedPickupReservation, HostedReceipt, HostedSale, HostedSaleRefundDetail, HostedStationKey, HostedStationSalesHistory, HostedSupplierBillingWorkspace, ProductBranchAvailability } from './api'
import { getOrCreateStationKey } from './stationCrypto'
import './pickup-reservations.css'
import './held-register-tickets.css'
import { HostedPickupQueuePanel } from './HostedPickupQueuePanel'
import { ReceiptShareActions } from './ReceiptShareActions'
import { formatObservedAt, isAvailabilityStale } from './availabilityFreshness'

type CartLine = { product: BranchProduct; quantity: number }
type LoadState = 'loading' | 'ready' | 'error'
type CheckoutRequest = {
  customerId: string | null
  items: Array<{ productId: string; quantity: number }>
  paymentOptionId: string
  amountMinor: number
  onAccountMinor: number
  reference: string | null
  pickupReservation?: { reservationId: string; expectedRevision: number }
}
type MutationAttempt = { fingerprint: string; key: string }
type PendingSaleAttempt =
  | { kind: 'legacy'; fingerprint: string; key: string }
  | { kind: 'exact'; fingerprint: string; key: string; body: CheckoutRequest }
type AvailabilityState = { kind: 'loading' } | { kind: 'ready'; branches: ProductBranchAvailability[] } | { kind: 'error'; message: string }
type PickupDraft = { productId: string; pickupBranchId: string }
type PickupSale = { reservationId: string; expectedRevision: number; items: Array<{ productId: string; quantity: number }> }
type HeldTicket = {
  id: string
  createdAt: string
  label: string
  lines: Array<{ productId: string; quantity: number }>
  selectedCustomerId: string
  selectedPaymentId: string
  reference: string
  saleAttempt: PendingSaleAttempt | null
  pickupSale: PickupSale | null
  sharedTicketId?: string | null
  sharedRevision?: number | null
  shareKey?: string | null
}
const maxHeldTickets = 50

function idempotencyFor(attempt: { current: MutationAttempt | null }, fingerprint: string): string {
  if (attempt.current?.fingerprint === fingerprint) return attempt.current.key
  const next = { fingerprint, key: crypto.randomUUID() }
  attempt.current = next
  return next.key
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function errorText(reason: unknown, fallback: string): string {
  return reason instanceof Error ? reason.message : fallback
}

function formatMoney(currency: string, minor: number): string {
  return `${currency} ${(minor / 100).toFixed(2)}`
}

function formatMinorUnits(currency: string, minorText: string): string {
  if (!/^\d+$/.test(minorText)) return `${currency} —`
  const minor = BigInt(minorText)
  return `${currency} ${minor / 100n}.${String(minor % 100n).padStart(2, '0')}`
}

function parseMinorAmount(value: string): number | null {
  const normalized = value.trim()
  if (!/^\d{1,12}(?:\.\d{1,2})?$/.test(normalized)) return null
  const [whole, fraction = ''] = normalized.split('.')
  const amount = Number(whole) * 100 + Number(fraction.padEnd(2, '0'))
  return Number.isSafeInteger(amount) ? amount : null
}

function localDateInputValue(): string {
  const date = new Date()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${date.getFullYear()}-${month}-${day}`
}

function parseMovementKind(value: string): 'paid_in' | 'paid_out' | 'cash_drop' {
  if (value === 'paid_in' || value === 'paid_out' || value === 'cash_drop') return value
  return 'paid_out'
}

function parseRefundMethod(value: string): 'cash' | 'card' | 'mobile_money' | 'bank_transfer' | 'other' {
  if (value === 'card' || value === 'mobile_money' || value === 'bank_transfer' || value === 'other') return value
  return 'cash'
}

function productTotalMinor(product: BranchProduct, quantity: number): number {
  const subtotal = product.priceMinor * quantity
  const tax = Math.floor((subtotal * product.taxRateBps + 5_000) / 10_000)
  return subtotal + tax
}

function createDraftKey(tenantId: string, branchId: string, stationId: string, subject: string): string {
  return `advanced-pos-register-draft:${tenantId}:${branchId}:${stationId}:${subject}`
}

function restoreCartLines(lines: unknown, products: BranchProduct[]): CartLine[] {
  if (!Array.isArray(lines)) return []
  return lines.flatMap((item): CartLine[] => {
    if (!isRecord(item) || typeof item.productId !== 'string' || typeof item.quantity !== 'number'
      || !Number.isSafeInteger(item.quantity) || item.quantity < 1) return []
    const product = products.find((candidate) => candidate.productId === item.productId)
    return product ? [{ product, quantity: item.quantity }] : []
  })
}

function restoreCheckoutRequest(value: unknown): CheckoutRequest | null {
  if (!isRecord(value) || !(typeof value.customerId === 'string' || value.customerId === null)
    || !Array.isArray(value.items) || typeof value.paymentOptionId !== 'string'
    || typeof value.amountMinor !== 'number' || !Number.isSafeInteger(value.amountMinor)
    || (value.onAccountMinor !== undefined && (typeof value.onAccountMinor !== 'number' || !Number.isSafeInteger(value.onAccountMinor) || value.onAccountMinor < 0))
    || !(typeof value.reference === 'string' || value.reference === null)) return null
  const items = value.items.flatMap((item): Array<{ productId: string; quantity: number }> =>
    isRecord(item) && typeof item.productId === 'string' && typeof item.quantity === 'number'
      && Number.isSafeInteger(item.quantity) && item.quantity > 0
      ? [{ productId: item.productId, quantity: item.quantity }]
      : [])
  if (items.length !== value.items.length) return null
  const pickup = value.pickupReservation
  let pickupReservation: CheckoutRequest['pickupReservation']
  if (pickup !== undefined) {
    if (!isRecord(pickup) || typeof pickup.reservationId !== 'string'
      || typeof pickup.expectedRevision !== 'number' || !Number.isSafeInteger(pickup.expectedRevision)) return null
    pickupReservation = { reservationId: pickup.reservationId, expectedRevision: pickup.expectedRevision }
  }
  return {
    customerId: value.customerId,
    items,
    paymentOptionId: value.paymentOptionId,
    amountMinor: value.amountMinor,
    onAccountMinor: typeof value.onAccountMinor === 'number' ? value.onAccountMinor : 0,
    reference: value.reference,
    ...(pickupReservation ? { pickupReservation } : {}),
  }
}

function restorePendingSaleAttempt(value: unknown): PendingSaleAttempt | null {
  if (!isRecord(value) || typeof value.fingerprint !== 'string' || typeof value.key !== 'string') return null
  const body = restoreCheckoutRequest(value.body)
  return body
    ? { kind: 'exact', fingerprint: value.fingerprint, key: value.key, body }
    : { kind: 'legacy', fingerprint: value.fingerprint, key: value.key }
}

function restoreHeldTicket(value: unknown): HeldTicket | null {
  if (!isRecord(value) || !Array.isArray(value.lines)) return null
  const id = typeof value.id === 'string' && value.id.length > 0 && value.id.length <= 128 ? value.id : crypto.randomUUID()
  const createdAt = typeof value.createdAt === 'string' && Number.isFinite(Date.parse(value.createdAt))
    ? new Date(value.createdAt).toISOString()
    : new Date().toISOString()
  const pickupValue = value.pickupSale
  const pickupSale = isRecord(pickupValue) && typeof pickupValue.reservationId === 'string'
      && pickupValue.reservationId.length > 0 && pickupValue.reservationId.length <= 128
      && typeof pickupValue.expectedRevision === 'number' && Number.isSafeInteger(pickupValue.expectedRevision)
      && Array.isArray(pickupValue.items)
    ? {
        reservationId: pickupValue.reservationId,
        expectedRevision: pickupValue.expectedRevision,
        items: pickupValue.items.flatMap((item): Array<{ productId: string; quantity: number }> =>
          isRecord(item) && typeof item.productId === 'string' && item.productId.length > 0 && item.productId.length <= 128
            && typeof item.quantity === 'number'
            && Number.isSafeInteger(item.quantity) && item.quantity > 0
            ? [{ productId: item.productId, quantity: item.quantity }]
            : []),
      }
    : null
  return {
    id,
    createdAt,
    label: typeof value.label === 'string' && value.label.length <= 160 ? value.label : 'Held sale',
    lines: value.lines.flatMap((item): Array<{ productId: string; quantity: number }> =>
      isRecord(item) && typeof item.productId === 'string' && item.productId.length > 0 && item.productId.length <= 128
        && typeof item.quantity === 'number'
        && Number.isSafeInteger(item.quantity) && item.quantity > 0
        ? [{ productId: item.productId, quantity: item.quantity }]
        : []),
    selectedCustomerId: typeof value.selectedCustomerId === 'string' ? value.selectedCustomerId : '',
    selectedPaymentId: typeof value.selectedPaymentId === 'string' ? value.selectedPaymentId : '',
    reference: typeof value.reference === 'string' ? value.reference.slice(0, 256) : '',
    saleAttempt: restorePendingSaleAttempt(value.saleAttempt),
    pickupSale,
    sharedTicketId: typeof value.sharedTicketId === 'string' && value.sharedTicketId.length <= 128 ? value.sharedTicketId : null,
    sharedRevision: typeof value.sharedRevision === 'number' && Number.isSafeInteger(value.sharedRevision) && value.sharedRevision > 0 ? value.sharedRevision : null,
    shareKey: typeof value.shareKey === 'string' && value.shareKey.length <= 128 ? value.shareKey : null,
  }
}

function restoreSharedCheckoutAttempt(
  ticket: HostedHeldTicket,
): PendingSaleAttempt | null {
  if (!ticket.checkoutKey || !isRecord(ticket.checkoutBody)) return null
  const body = ticket.checkoutBody
  if (body.heldTicketId !== ticket.id || !Array.isArray(body.tenders) || body.tenders.length > 1) return null
  const tender = body.tenders[0]
  if (tender !== undefined && !isRecord(tender)) return null
  const request = restoreCheckoutRequest({
    customerId: body.customerId,
    items: body.items,
    paymentOptionId: tender && isRecord(tender) ? String(tender.paymentOptionId ?? '') : '',
    amountMinor: tender && isRecord(tender) && typeof tender.amountMinor === 'number' ? tender.amountMinor : 0,
    onAccountMinor: typeof body.onAccountMinor === 'number' ? body.onAccountMinor : 0,
    reference: tender && isRecord(tender) && typeof tender.reference === 'string' ? tender.reference : null,
    ...(body.pickupReservation ? { pickupReservation: body.pickupReservation } : {}),
  })
  if (!request) return null
  return { kind: 'exact', fingerprint: JSON.stringify(request), key: ticket.checkoutKey, body: request }
}

function hostedHeldTicketPayload(input: {
  label: string
  lines: Array<{ productId: string; quantity: number }>
  selectedCustomerId: string
  selectedPaymentId: string
  reference: string
  pickupSale: PickupSale | null
}): HostedHeldTicketPayload {
  return {
    label: input.label,
    lines: input.lines,
    selectedCustomerId: input.selectedCustomerId,
    selectedPaymentId: input.selectedPaymentId,
    reference: input.reference,
    pickupSale: input.pickupSale,
  }
}

function hostedCommitBody(request: CheckoutRequest, heldTicketId: string): Record<string, unknown> {
  return {
    customerId: request.customerId,
    items: request.items,
    tenders: request.amountMinor > 0 ? [{ paymentOptionId: request.paymentOptionId, amountMinor: request.amountMinor, reference: request.reference }] : [],
    onAccountMinor: request.onAccountMinor,
    ...(request.pickupReservation ? { pickupReservation: request.pickupReservation } : {}),
    heldTicketId,
  }
}

export function HostedRegisterPanel({
  apiBaseUrl, accessToken, tenantId, branchId, stationId, stationName, branchName, subject,
  canView, canCheckout, canActivate, canCheckOtherBranches, canRequestTransfers, canRequestPickups, canReadPickups, canPreparePickups, canSettle, canRecordCashExpense, canRefund,
}: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  branchId: string
  stationId: string
  stationName: string
  branchName: string
  subject: string
  canView: boolean
  canCheckout: boolean
  canActivate: boolean
  canCheckOtherBranches: boolean
  canRequestTransfers: boolean
  canRequestPickups: boolean
  canReadPickups: boolean
  canPreparePickups: boolean
  canSettle: boolean
  canRecordCashExpense: boolean
  canRefund: boolean
}) {
  const [catalogState, setCatalogState] = useState<LoadState>('loading')
  const [products, setProducts] = useState<BranchProduct[]>([])
  const [customers, setCustomers] = useState<HostedCustomer[]>([])
  const [selectedCustomerId, setSelectedCustomerId] = useState('')
  const [creditPosition, setCreditPosition] = useState<HostedCustomerCreditPosition | null>(null)
  const [creditPositionLoading, setCreditPositionLoading] = useState(false)
  const [creditPositionError, setCreditPositionError] = useState<string | null>(null)
  const [openReceivables, setOpenReceivables] = useState<HostedOpenReceivableInvoice[]>([])
  const [receivableCollectionOpen, setReceivableCollectionOpen] = useState(false)
  const [receivableCollectionLoading, setReceivableCollectionLoading] = useState(false)
  const [receivableCollectionAmounts, setReceivableCollectionAmounts] = useState<Record<string, string>>({})
  const [receivableCollectionReference, setReceivableCollectionReference] = useState('')
  const [customerSearch, setCustomerSearch] = useState('')
  const [customerPageOffset, setCustomerPageOffset] = useState(100)
  const [customerHasMore, setCustomerHasMore] = useState(false)
  const [customerSearchBusy, setCustomerSearchBusy] = useState(false)
  const [customerFormOpen, setCustomerFormOpen] = useState(false)
  const [customerName, setCustomerName] = useState('')
  const [customerPhone, setCustomerPhone] = useState('')
  const [customerEmail, setCustomerEmail] = useState('')
  const [customerBusy, setCustomerBusy] = useState(false)
  const [paymentOptions, setPaymentOptions] = useState<BranchPaymentOption[]>([])
  const [query, setQuery] = useState('')
  const [cart, setCart] = useState<CartLine[]>([])
  const [pickupSale, setPickupSale] = useState<PickupSale | null>(null)
  const [draftRestored, setDraftRestored] = useState(false)
  const [heldTickets, setHeldTickets] = useState<HeldTicket[]>([])
  const [heldTicketsOpen, setHeldTicketsOpen] = useState(false)
  const [branchHeldTickets, setBranchHeldTickets] = useState<HostedHeldTicket[]>([])
  const [branchHeldLoading, setBranchHeldLoading] = useState(false)
  const [branchHeldError, setBranchHeldError] = useState<string | null>(null)
  const [activeSharedTicketId, setActiveSharedTicketId] = useState<string | null>(null)
  const [activeSharedRevision, setActiveSharedRevision] = useState<number | null>(null)
  const [activeSharedClaimReady, setActiveSharedClaimReady] = useState(false)
  const [saleAttempt, setSaleAttempt] = useState<PendingSaleAttempt | null>(null)
  const [pendingSale, setPendingSale] = useState(false)
  const [selectedPaymentId, setSelectedPaymentId] = useState('')
  const [onAccountAmount, setOnAccountAmount] = useState('')
  const [reference, setReference] = useState('')
  const [lease, setLease] = useState<HostedLease | null>(null)
  const [key, setKey] = useState<HostedStationKey | null>(null)
  const [drawer, setDrawer] = useState<HostedDrawerReport | null>(null)
  const [drawerHistory, setDrawerHistory] = useState<HostedDrawerReport[]>([])
  const [historyVisible, setHistoryVisible] = useState(false)
  const [historyBusy, setHistoryBusy] = useState(false)
  const [salesHistoryVisible, setSalesHistoryVisible] = useState(false)
  const [salesHistory, setSalesHistory] = useState<HostedStationSalesHistory | null>(null)
  const [salesHistorySearch, setSalesHistorySearch] = useState('')
  const [salesHistoryAppliedSearch, setSalesHistoryAppliedSearch] = useState('')
  const [salesHistoryBusy, setSalesHistoryBusy] = useState(false)
  const [salesHistoryError, setSalesHistoryError] = useState<string | null>(null)
  const [historicalReceipt, setHistoricalReceipt] = useState<HostedReceipt | null>(null)
  const [historicalReceiptBusyId, setHistoricalReceiptBusyId] = useState<string | null>(null)
  const [refundDetail, setRefundDetail] = useState<HostedSaleRefundDetail | null>(null)
  const [refundQuantities, setRefundQuantities] = useState<Record<string, string>>({})
  const [refundReason, setRefundReason] = useState('')
  const [refundKind, setRefundKind] = useState<'refund' | 'void'>('refund')
  const [refundMethod, setRefundMethod] = useState<'cash' | 'card' | 'mobile_money' | 'bank_transfer' | 'other'>('cash')
  const [refundReference, setRefundReference] = useState('')
  const [refundRestock, setRefundRestock] = useState(false)
  const [refundBusy, setRefundBusy] = useState(false)
  const [lastSale, setLastSale] = useState<HostedSale | null>(null)
  const [lastSaleLines, setLastSaleLines] = useState<Array<{ name: string; quantity: number; unitPriceMinor: number }>>([])
  const [lastSalePayment, setLastSalePayment] = useState('')
  const [receiptCustomerQuery, setReceiptCustomerQuery] = useState('')
  const [receiptCustomerRows, setReceiptCustomerRows] = useState<HostedCustomer[]>([])
  const [receiptCustomerId, setReceiptCustomerId] = useState('')
  const [receiptCustomerName, setReceiptCustomerName] = useState('')
  const [receiptCustomerPhone, setReceiptCustomerPhone] = useState('')
  const [receiptCustomerEmail, setReceiptCustomerEmail] = useState('')
  const [pendingReceiptCustomer, setPendingReceiptCustomer] = useState<Pick<HostedCustomer, 'id' | 'name' | 'phone' | 'email'> | null>(null)
  const [receiptCustomerBusy, setReceiptCustomerBusy] = useState(false)
  const [xReport, setXReport] = useState<HostedDrawerReport | null>(null)
  const [openingCash, setOpeningCash] = useState('')
  const [countedCash, setCountedCash] = useState('')
  const [movementKind, setMovementKind] = useState<'paid_in' | 'paid_out' | 'cash_drop'>('paid_out')
  const [movementAmount, setMovementAmount] = useState('')
  const [movementNote, setMovementNote] = useState('')
  const [cashDropDestinations, setCashDropDestinations] = useState<HostedCashDropDestination[] | null>(null)
  const [cashDropDestinationId, setCashDropDestinationId] = useState('')
  const [cashDropDestinationError, setCashDropDestinationError] = useState<string | null>(null)
  const [supplierBilling, setSupplierBilling] = useState<HostedSupplierBillingWorkspace | null>(null)
  const [supplierPaymentOpen, setSupplierPaymentOpen] = useState(false)
  const [supplierPaymentSupplier, setSupplierPaymentSupplier] = useState('')
  const [supplierPaymentAmounts, setSupplierPaymentAmounts] = useState<Record<string, string>>({})
  const [supplierPaymentReference, setSupplierPaymentReference] = useState('')
  const [cashExpenseOpen, setCashExpenseOpen] = useState(false)
  const [cashExpenseDescription, setCashExpenseDescription] = useState('')
  const [cashExpenseCategory, setCashExpenseCategory] = useState<HostedExpenseCategory>('other')
  const [cashExpensePayee, setCashExpensePayee] = useState('')
  const [cashExpenseAmount, setCashExpenseAmount] = useState('')
  const [cashExpenseReference, setCashExpenseReference] = useState('')
  const [cashExpenseDate, setCashExpenseDate] = useState(localDateInputValue)
  const [busy, setBusy] = useState<'activation' | 'lease' | 'drawer-open' | 'checkout' | 'movement' | 'drawer-close' | 'supplier-payment' | 'cash-expense' | 'receivable-collection' | null>(null)
  const [notice, setNotice] = useState<{ kind: 'error' | 'success'; text: string } | null>(null)
  const [availability, setAvailability] = useState<{ productId: string; result: AvailabilityState } | null>(null)
  const [availabilityNowMs, setAvailabilityNowMs] = useState(() => Date.now())
  const [transferQuantity, setTransferQuantity] = useState('1')
  const [transferBusy, setTransferBusy] = useState(false)
  const [pickupDraft, setPickupDraft] = useState<PickupDraft | null>(null)
  const [pickupQuantity, setPickupQuantity] = useState('1')
  const [pickupContactName, setPickupContactName] = useState('')
  const [pickupContactPhone, setPickupContactPhone] = useState('')
  const [pickupContactConsent, setPickupContactConsent] = useState(false)
  const [pickupBusy, setPickupBusy] = useState(false)
  const [pickupQueueOpen, setPickupQueueOpen] = useState(false)
  const scanRef = useRef<HTMLInputElement>(null)
  const leaseRef = useRef<HostedLease | null>(null)
  const keyRef = useRef<HostedStationKey | null>(null)
  const openingAttempt = useRef<MutationAttempt | null>(null)
  const movementAttempt = useRef<MutationAttempt | null>(null)
  const closingAttempt = useRef<MutationAttempt | null>(null)
  const storageWarningShown = useRef(false)
  const availabilityRequest = useRef(0)
  const activeSharedTicketRef = useRef<string | null>(null)
  const sharedRevisionRef = useRef<number | null>(null)
  const sharedSaveChainRef = useRef<Promise<void>>(Promise.resolve())
  const sharedSaveTimerRef = useRef<number | null>(null)
  const pickupQueueDialogRef = useRef<HTMLDialogElement>(null)
  const transferAttempt = useRef<MutationAttempt | null>(null)
  const pickupAttempt = useRef<MutationAttempt | null>(null)
  const customerAttempt = useRef<MutationAttempt | null>(null)
  const receiptCustomerAttempt = useRef<MutationAttempt | null>(null)
  const supplierPaymentAttempt = useRef<MutationAttempt | null>(null)
  const receivableCollectionAttempt = useRef<MutationAttempt | null>(null)
  const cashExpenseAttempt = useRef<MutationAttempt | null>(null)
  const refundAttempt = useRef<MutationAttempt | null>(null)
  const location = useMemo(() => ({ apiBaseUrl, accessToken, tenantId, branchId, stationId }), [accessToken, apiBaseUrl, branchId, stationId, tenantId])
  const payment = paymentOptions.find((option) => option.id === selectedPaymentId)

  function setActiveSharedTicket(ticketId: string | null, revision: number | null, claimReady: boolean) {
    activeSharedTicketRef.current = ticketId
    sharedRevisionRef.current = revision
    setActiveSharedTicketId(ticketId)
    setActiveSharedRevision(revision)
    setActiveSharedClaimReady(claimReady)
  }

  const refreshBranchHeldTickets = useCallback(async () => {
    setBranchHeldLoading(true)
    setBranchHeldError(null)
    try {
      setBranchHeldTickets(await listBranchHeldTickets({ apiBaseUrl, accessToken, tenantId, branchId }))
    } catch (reason) {
      setBranchHeldError(errorText(reason, 'Branch-held tickets could not be loaded.'))
    } finally {
      setBranchHeldLoading(false)
    }
  }, [accessToken, apiBaseUrl, branchId, tenantId])

  const persistSharedTicket = useCallback((
    ticketId: string,
    payload: HostedHeldTicketPayload,
    leaseSnapshot: HostedLease,
  ) => {
    const operation = sharedSaveChainRef.current.then(async () => {
      if (activeSharedTicketRef.current !== ticketId) return
      const expectedRevision = sharedRevisionRef.current
      if (!expectedRevision) return
      const saved = await saveBranchHeldTicket({
        ...location,
        lease: leaseSnapshot,
        ticketId,
        expectedRevision,
        payload,
      })
      if (activeSharedTicketRef.current === ticketId) {
        sharedRevisionRef.current = saved.revision
        setActiveSharedRevision(saved.revision)
        setBranchHeldTickets((current) => [saved, ...current.filter((ticket) => ticket.id !== saved.id)])
      }
    })
    sharedSaveChainRef.current = operation.catch(() => undefined)
    return operation
  }, [location])

  const loadWorkspace = useCallback(async () => {
    setCatalogState('loading')
    try {
      const [catalog, options, customerRows] = await Promise.all([
        listStationProducts(location),
        listBranchPaymentOptions({ apiBaseUrl, accessToken, tenantId, branchId }).catch(() => []),
        listStationHostedCustomers({ ...location, pageSize: 100 }).catch(() => []),
      ])
      setProducts(catalog.filter((product) => product.isActive))
      setCustomers(customerRows)
      setCustomerPageOffset(customerRows.length)
      setCustomerHasMore(customerRows.length === 100)
      setSelectedCustomerId((current) => customerRows.some((customer) => customer.id === current) ? current : '')
      const enabledOptions = options.filter((option) => option.enabled)
      setPaymentOptions(enabledOptions)
      setSelectedPaymentId((current) => enabledOptions.some((option) => option.id === current) ? current : enabledOptions[0]?.id ?? '')
      setCatalogState('ready')
    } catch (reason) {
      setCatalogState('error')
      setNotice({ kind: 'error', text: errorText(reason, 'Register products or payment methods could not be loaded.') })
    }
  }, [accessToken, apiBaseUrl, branchId, location, tenantId])

  useEffect(() => { void loadWorkspace() }, [loadWorkspace])

  useEffect(() => {
    let active = true
    setCreditPosition(null)
    setCreditPositionError(null)
    setOpenReceivables([])
    setReceivableCollectionOpen(false)
    setReceivableCollectionAmounts({})
    setReceivableCollectionReference('')
    if (!selectedCustomerId) {
      setCreditPositionLoading(false)
      return () => { active = false }
    }
    setCreditPositionLoading(true)
    void getStationHostedCustomerCreditPosition({ ...location, customerId: selectedCustomerId })
      .then((position) => { if (active) setCreditPosition(position) })
      .catch((reason: unknown) => { if (active) setCreditPositionError(errorText(reason, 'Customer credit position could not be loaded.')) })
      .finally(() => { if (active) setCreditPositionLoading(false) })
    return () => { active = false }
  }, [location, selectedCustomerId])

  useEffect(() => {
    const timer = window.setInterval(() => setAvailabilityNowMs(Date.now()), 15_000)
    return () => window.clearInterval(timer)
  }, [])

  useEffect(() => {
    let active = true
    if (canView) {
      void getHostedDrawerShift(location).then((report) => { if (active) setDrawer(report) }).catch((reason: unknown) => {
        if (active) setNotice({ kind: 'error', text: errorText(reason, 'The cash drawer status could not be loaded.') })
      })
    }
    return () => { active = false }
  }, [canView, location])

  useEffect(() => {
    let active = true
    if (canCheckout) {
      void listHostedCashDropDestinations(location).then((destinations) => {
        if (!active) return
        setCashDropDestinationError(null)
        setCashDropDestinations(destinations)
        setCashDropDestinationId((current) => destinations.some((destination) => destination.id === current) ? current : destinations[0]?.id ?? '')
      }).catch(() => { if (active) { setCashDropDestinations([]); setCashDropDestinationError('Cash destinations could not be loaded. Refresh the register and try again.') } })
    } else {
      setCashDropDestinations(null)
      setCashDropDestinationId('')
      setCashDropDestinationError(null)
    }
    return () => { active = false }
  }, [canCheckout, location])

  useEffect(() => {
    if (catalogState !== 'ready' || draftRestored) return
    const draftKey = createDraftKey(tenantId, branchId, stationId, subject)
    try {
      const saved = localStorage.getItem(draftKey)
      if (!saved) {
        setDraftRestored(true)
        return
      }
      const parsed: unknown = JSON.parse(saved)
      if (isRecord(parsed) && parsed.version === 2) {
        const active = restoreHeldTicket(parsed.active)
        if (active) {
          setCart(restoreCartLines(active.lines, products))
          setSelectedCustomerId(active.selectedCustomerId)
          setSelectedPaymentId(active.selectedPaymentId || paymentOptions[0]?.id || '')
          setReference(active.reference)
          setPickupSale(active.pickupSale)
          setSaleAttempt(active.saleAttempt)
          setPendingSale(Boolean(active.saleAttempt))
          if (active.sharedTicketId && active.sharedRevision) {
            setActiveSharedTicket(active.sharedTicketId, active.sharedRevision, false)
          }
        }
        setHeldTickets(Array.isArray(parsed.heldTickets)
          ? parsed.heldTickets.flatMap((item): HeldTicket[] => {
              const ticket = restoreHeldTicket(item)
              return ticket && (ticket.lines.length > 0 || ticket.saleAttempt) ? [ticket] : []
            }).slice(0, maxHeldTickets)
          : [])
      } else {
        const savedLines = Array.isArray(parsed) ? parsed : isRecord(parsed) ? parsed.lines : null
        if (!Array.isArray(savedLines)) {
          setDraftRestored(true)
          return
        }
        setCart(restoreCartLines(savedLines, products))
        const previousAttempt = isRecord(parsed) ? restorePendingSaleAttempt(parsed.saleAttempt) : null
        if (previousAttempt) {
          setSaleAttempt(previousAttempt)
          setPendingSale(true)
        }
      }
    } catch {
      // An unreadable local draft is ignored; no sale data is written to the server until checkout.
    } finally {
      setDraftRestored(true)
    }
  }, [branchId, catalogState, draftRestored, paymentOptions, products, stationId, subject, tenantId])

  useEffect(() => {
    if (catalogState !== 'ready' || !draftRestored) return
    const draftKey = createDraftKey(tenantId, branchId, stationId, subject)
    const draft = cart.map(({ product, quantity }) => ({ productId: product.productId, quantity }))
    const active: HeldTicket = {
      id: 'active',
      createdAt: new Date().toISOString(),
      label: pickupSale ? 'Pickup sale' : selectedCustomerId ? 'Customer sale' : 'Walk-in sale',
      lines: draft,
      selectedCustomerId,
      selectedPaymentId,
      reference,
      saleAttempt,
      pickupSale,
      sharedTicketId: activeSharedTicketId,
      sharedRevision: activeSharedRevision,
    }
    try {
      if (draft.length === 0 && !saleAttempt && heldTickets.length === 0) localStorage.removeItem(draftKey)
      else localStorage.setItem(draftKey, JSON.stringify({ version: 2, active, heldTickets }))
    } catch {
      if (!storageWarningShown.current) {
        storageWarningShown.current = true
        setNotice({ kind: 'error', text: 'This browser cannot save the ticket locally. The current ticket remains available until this page is closed.' })
      }
    }
  }, [activeSharedRevision, activeSharedTicketId, branchId, cart, catalogState, draftRestored, heldTickets, pickupSale, reference, saleAttempt, selectedCustomerId, selectedPaymentId, stationId, subject, tenantId])

  useEffect(() => {
    if (!heldTicketsOpen) return
    void refreshBranchHeldTickets()
    const timer = window.setInterval(() => void refreshBranchHeldTickets(), 10_000)
    return () => window.clearInterval(timer)
  }, [heldTicketsOpen, refreshBranchHeldTickets])

  useEffect(() => {
    if (!lease || !activeSharedTicketId || activeSharedClaimReady) return
    let active = true
    void listBranchHeldTickets({ apiBaseUrl, accessToken, tenantId, branchId }).then(async (rows) => {
      if (active) setBranchHeldTickets(rows)
      const ticket = rows.find((row) => row.id === activeSharedTicketId)
      if (!ticket) return
      try {
        const claimed = await claimBranchHeldTicket({ ...location, lease, ticketId: ticket.id, expectedRevision: activeSharedRevision ?? ticket.revision })
        if (active) {
          setActiveSharedTicket(claimed.id, claimed.revision, true)
          setBranchHeldTickets((rows) => [claimed, ...rows.filter((row) => row.id !== claimed.id)])
        }
      } catch (reason) {
        if (active) setNotice({ kind: 'error', text: `The shared ticket could not be reclaimed. ${errorText(reason, '')}`.trim() })
      }
    })
    return () => { active = false }
  }, [accessToken, activeSharedClaimReady, activeSharedRevision, activeSharedTicketId, apiBaseUrl, branchId, lease, location, tenantId])

  useEffect(() => {
    if (!activeSharedTicketId || !activeSharedClaimReady || !lease || !draftRestored || pendingSale) return
    if (sharedSaveTimerRef.current !== null) window.clearTimeout(sharedSaveTimerRef.current)
    sharedSaveTimerRef.current = window.setTimeout(() => {
      const label = pickupSale ? 'Pickup sale' : selectedCustomerId ? 'Customer sale' : 'Walk-in sale'
      void persistSharedTicket(activeSharedTicketId, hostedHeldTicketPayload({
        label,
        lines: cart.map(({ product, quantity }) => ({ productId: product.productId, quantity })),
        selectedCustomerId,
        selectedPaymentId,
        reference,
        pickupSale,
      }), lease).catch((reason: unknown) => setNotice({ kind: 'error', text: `Shared ticket autosave failed. Keep this register open and retry. ${errorText(reason, '')}`.trim() }))
    }, 800)
    return () => {
      if (sharedSaveTimerRef.current !== null) window.clearTimeout(sharedSaveTimerRef.current)
    }
  }, [activeSharedClaimReady, activeSharedTicketId, cart, draftRestored, lease, pendingSale, persistSharedTicket, pickupSale, reference, selectedCustomerId, selectedPaymentId])

  useEffect(() => {
    leaseRef.current = lease
    keyRef.current = key
  }, [key, lease])

  useEffect(() => {
    if (!lease) return
    const currentLease = lease
    const heartbeat = window.setInterval(() => {
      void heartbeatHostedCashierLease({ apiBaseUrl, accessToken, lease: currentLease }).catch((reason: unknown) => {
        setLease(null)
        setNotice({ kind: 'error', text: `Cashier session could not be renewed. Checkout is locked. ${errorText(reason, '')}`.trim() })
      })
    }, 15_000)
    return () => {
      window.clearInterval(heartbeat)
      if (leaseRef.current?.leaseId === currentLease.leaseId) {
        void releaseHostedCashierLease({ apiBaseUrl, accessToken, lease: currentLease }).catch(() => undefined)
      }
    }
  }, [accessToken, apiBaseUrl, lease])

  const visibleProducts = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase()
    return products.filter((product) => !needle || `${product.name} ${product.sku} ${product.barcode ?? ''} ${product.category ?? ''}`.toLocaleLowerCase().includes(needle))
  }, [products, query])

  const subtotalMinor = cart.reduce((sum, line) => sum + line.product.priceMinor * line.quantity, 0)
  const taxMinor = cart.reduce((sum, line) => sum + productTotalMinor(line.product, line.quantity) - line.product.priceMinor * line.quantity, 0)
  const totalMinor = subtotalMinor + taxMinor

  function snapshotCurrentTicket(): HeldTicket {
    return {
      id: crypto.randomUUID(),
      createdAt: new Date().toISOString(),
      label: pickupSale ? 'Pickup sale' : selectedCustomerId ? 'Customer sale' : 'Walk-in sale',
      lines: cart.map(({ product, quantity }) => ({ productId: product.productId, quantity })),
      selectedCustomerId,
      selectedPaymentId,
      reference,
      saleAttempt,
      pickupSale,
    }
  }

  function resetCurrentTicket() {
    setCart([])
    setPickupSale(null)
    setSelectedCustomerId('')
    setReference('')
    setSaleAttempt(null)
    setPendingSale(false)
    setSelectedPaymentId(paymentOptions[0]?.id ?? '')
  }

  async function holdCurrentTicket() {
    if (cart.length === 0 && !saleAttempt) return
    if (activeSharedTicketId) {
      if (!lease || !activeSharedClaimReady || saleAttempt) {
        setNotice({ kind: 'error', text: 'This shared ticket needs an active cashier claim and a confirmed checkout state before it can be held.' })
        return
      }
      try {
        await persistSharedTicket(activeSharedTicketId, hostedHeldTicketPayload({
          label: pickupSale ? 'Pickup sale' : selectedCustomerId ? 'Customer sale' : 'Walk-in sale',
          lines: cart.map(({ product, quantity }) => ({ productId: product.productId, quantity })),
          selectedCustomerId, selectedPaymentId, reference, pickupSale,
        }), lease)
        const savedRevision = sharedRevisionRef.current
        if (!savedRevision) throw new Error('The shared ticket revision could not be confirmed.')
        await releaseBranchHeldTicket({ ...location, lease, ticketId: activeSharedTicketId, expectedRevision: savedRevision })
        setActiveSharedTicket(null, null, false)
        resetCurrentTicket()
        await refreshBranchHeldTickets()
        setNotice({ kind: 'success', text: 'Shared ticket saved and released for another cashier at this branch.' })
      } catch (reason) {
        setNotice({ kind: 'error', text: `The shared ticket was not released. ${errorText(reason, '')}`.trim() })
      }
      return
    }
    if (heldTickets.length >= maxHeldTickets) {
      setNotice({ kind: 'error', text: `This device can keep up to ${maxHeldTickets} held tickets for this staff profile. Resume or discard one before holding another.` })
      return
    }
    const ticket = snapshotCurrentTicket()
    setHeldTickets((current) => [ticket, ...current])
    resetCurrentTicket()
    setNotice({ kind: 'success', text: 'Ticket held on this device. It stays available here for this staff profile.' })
  }

  async function shareHeldTicket(ticketId: string) {
    if (!lease || !key) {
      setNotice({ kind: 'error', text: 'Switch to an active cashier profile before sharing a ticket with this branch.' })
      return
    }
    const ticket = heldTickets.find((row) => row.id === ticketId)
    if (!ticket || ticket.saleAttempt) {
      setNotice({ kind: 'error', text: 'A ticket with an unresolved checkout cannot be shared. Retry its exact checkout first.' })
      return
    }
    const shareKey = ticket.shareKey ?? crypto.randomUUID()
    setHeldTickets((rows) => rows.map((row) => row.id === ticketId ? { ...row, shareKey } : row))
    try {
      const shared = await createBranchHeldTicket({
        ...location, lease, idempotencyKey: shareKey,
        payload: hostedHeldTicketPayload({
          label: ticket.label, lines: ticket.lines, selectedCustomerId: ticket.selectedCustomerId,
          selectedPaymentId: ticket.selectedPaymentId, reference: ticket.reference, pickupSale: ticket.pickupSale,
        }),
      })
      await releaseBranchHeldTicket({ ...location, lease, ticketId: shared.id, expectedRevision: shared.revision })
      setHeldTickets((rows) => rows.filter((row) => row.id !== ticketId))
      await refreshBranchHeldTickets()
      setNotice({ kind: 'success', text: 'Ticket shared with the branch and released for another cashier.' })
    } catch (reason) {
      setNotice({ kind: 'error', text: `Ticket sharing could not be confirmed. Retry sharing; it uses the same request key. ${errorText(reason, '')}`.trim() })
    }
  }

  function resumeHeldTicket(ticketId: string) {
    const ticket = heldTickets.find((item) => item.id === ticketId)
    if (!ticket) return
    const activeHasWork = cart.length > 0 || Boolean(saleAttempt)
    const current = activeHasWork ? snapshotCurrentTicket() : null
    setHeldTickets((tickets) => [
      ...(current ? [current] : []),
      ...tickets.filter((item) => item.id !== ticketId),
    ])
    setCart(restoreCartLines(ticket.lines, products))
    setPickupSale(ticket.pickupSale)
    setSelectedCustomerId(ticket.selectedCustomerId)
    setSelectedPaymentId(ticket.selectedPaymentId || paymentOptions[0]?.id || '')
    setReference(ticket.reference)
    setSaleAttempt(ticket.saleAttempt)
    setPendingSale(Boolean(ticket.saleAttempt))
    setHeldTicketsOpen(false)
    setNotice({ kind: 'success', text: ticket.saleAttempt ? 'Held checkout restored. Retry it unchanged to confirm the original result.' : `${ticket.label} restored.` })
  }

  async function resumeBranchHeldTicket(ticket: HostedHeldTicket) {
    if (!lease || !canCheckout) {
      setNotice({ kind: 'error', text: 'An active cashier profile is required to claim a shared ticket.' })
      return
    }
    if (cart.length > 0 || saleAttempt) {
      setNotice({ kind: 'error', text: 'Hold or clear the current ticket before resuming a branch ticket.' })
      return
    }
    try {
      const claimed = await claimBranchHeldTicket({ ...location, lease, ticketId: ticket.id, expectedRevision: ticket.revision })
      const restored = restoreCartLines(claimed.payload.lines, products)
      if (restored.length !== claimed.payload.lines.length) {
        await releaseBranchHeldTicket({ ...location, lease, ticketId: claimed.id, expectedRevision: claimed.revision })
        throw new Error('One or more products are no longer active at this branch, so the ticket was released unchanged.')
      }
      const pendingAttempt = restoreSharedCheckoutAttempt(claimed)
      setCart(restored)
      setSelectedCustomerId(claimed.payload.selectedCustomerId)
      setSelectedPaymentId(claimed.payload.selectedPaymentId || paymentOptions[0]?.id || '')
      setReference(claimed.payload.reference)
      setPickupSale(claimed.payload.pickupSale)
      setSaleAttempt(pendingAttempt)
      setPendingSale(Boolean(pendingAttempt))
      setActiveSharedTicket(claimed.id, claimed.revision, true)
      setBranchHeldTickets((rows) => [claimed, ...rows.filter((row) => row.id !== claimed.id)])
      setNotice({ kind: 'success', text: pendingAttempt ? 'Shared checkout restored. Retry it unchanged to confirm the original result.' : `${claimed.payload.label} claimed for this cashier.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: errorText(reason, 'The branch ticket could not be claimed.') })
      await refreshBranchHeldTickets()
    }
  }

  function discardHeldTicket(ticketId: string) {
    const ticket = heldTickets.find((item) => item.id === ticketId)
    if (ticket?.saleAttempt) {
      setNotice({ kind: 'error', text: 'This checkout may already have completed. Resume and retry it unchanged before discarding it.' })
      return
    }
    setHeldTickets((current) => current.filter((item) => item.id !== ticketId))
  }

  function addProduct(product: BranchProduct) {
    if (pendingSale || busy === 'checkout') return
    setCart((current) => {
      const found = current.find((line) => line.product.productId === product.productId)
      return found
        ? current.map((line) => line.product.productId === product.productId ? { ...line, quantity: line.quantity + 1 } : line)
        : [...current, { product, quantity: 1 }]
    })
  }

  function scanOrSearch(value: string) {
    if (pendingSale || busy === 'checkout') return
    const normalized = value.trim().toLocaleLowerCase()
    if (!normalized) return
    const product = products.find((entry) => entry.barcode?.toLocaleLowerCase() === normalized || entry.sku.toLocaleLowerCase() === normalized)
    if (product) {
      addProduct(product)
      setQuery('')
    }
  }

  function changeQuantity(productId: string, change: number) {
    if (pendingSale || busy === 'checkout') return
    const requiredHeldQuantity = pickupSale?.items.find((item) => item.productId === productId)?.quantity ?? 0
    setCart((current) => current.flatMap((line): CartLine[] => {
      if (line.product.productId !== productId) return [line]
      const quantity = line.quantity + change
      return quantity < requiredHeldQuantity ? [line] : quantity <= 0 ? [] : [{ ...line, quantity }]
    }))
  }

  async function saveCustomer() {
    const customer = { name: customerName.trim(), phone: customerPhone.trim() || null, email: customerEmail.trim() || null, notes: null }
    if (!customer.name) { setNotice({ kind: 'error', text: 'Enter the customer’s name.' }); return }
    const fingerprint = JSON.stringify(customer)
    const idempotencyKey = idempotencyFor(customerAttempt, fingerprint)
    setCustomerBusy(true)
    try {
      const created = await createStationHostedCustomer({ ...location, customer, idempotencyKey })
      customerAttempt.current = null
      setCustomers((current) => current.some((item) => item.id === created.id) ? current : [...current, created].sort((a, b) => a.name.localeCompare(b.name)))
      setSelectedCustomerId(created.id)
      setCustomerSearch('')
      setCustomerName(''); setCustomerPhone(''); setCustomerEmail(''); setCustomerFormOpen(false)
      setNotice({ kind: 'success', text: `${created.name} was added and selected for this sale.` })
    } catch (reason) { setNotice({ kind: 'error', text: `${errorText(reason, 'The customer could not be saved.')}${reason instanceof ApiError && reason.status >= 400 && reason.status < 500 ? '' : ' Retry without changing the details to avoid creating a duplicate.'}` }) }
    finally { setCustomerBusy(false) }
  }

  async function findCustomers(offset = 0) {
    setCustomerSearchBusy(true)
    try {
      const results = await listStationHostedCustomers({ ...location, query: customerSearch, offset, pageSize: 100 })
      setCustomers((current) => {
        if (offset !== 0) return [...current, ...results.filter((item) => !current.some((old) => old.id === item.id))]
        const selected = current.find((item) => item.id === selectedCustomerId)
        return selected && !results.some((item) => item.id === selected.id) ? [selected, ...results] : results
      })
      setCustomerPageOffset(offset + results.length)
      setCustomerHasMore(results.length === 100)
    } catch (reason) {
      setNotice({ kind: 'error', text: errorText(reason, 'Customer search could not be loaded.') })
    } finally { setCustomerSearchBusy(false) }
  }

  async function searchReceiptCustomers() {
    setReceiptCustomerBusy(true)
    try {
      setReceiptCustomerRows(await listStationHostedCustomers({ ...location, query: receiptCustomerQuery, pageSize: 100 }))
    } catch (reason) {
      setNotice({ kind: 'error', text: errorText(reason, 'Customer search could not be loaded.') })
    } finally { setReceiptCustomerBusy(false) }
  }

  async function linkReceiptCustomer(customer: Pick<HostedCustomer, 'id' | 'name' | 'phone' | 'email'>) {
    if (!lastSale) return
    setPendingReceiptCustomer(customer)
    setReceiptCustomerBusy(true)
    try {
      await attachStationSaleCustomer({ ...location, saleId: lastSale.id, customerId: customer.id })
      setLastSale((current) => current?.id === lastSale.id ? { ...current, customerId: customer.id } : current)
      setCustomers((current) => {
        const existing = current.find((row) => row.id === customer.id)
        const linked: HostedCustomer = existing
          ? { ...existing, name: customer.name, phone: customer.phone ?? existing.phone, email: customer.email ?? existing.email }
          : { id: customer.id, name: customer.name, phone: customer.phone, email: customer.email, notes: null, createdAt: new Date().toISOString(), status: 'active', revision: 1, mergedIntoCustomerId: null }
        return existing ? current.map((row) => row.id === customer.id ? linked : row) : [...current, linked]
      })
      setPendingReceiptCustomer(null)
      setReceiptCustomerId('')
      setNotice({ kind: 'success', text: `${customer.name} was linked to receipt ${lastSale.receiptNumber}.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: `${errorText(reason, 'The customer could not be linked.')} You can retry without creating another customer.` })
    } finally { setReceiptCustomerBusy(false) }
  }

  async function createAndLinkReceiptCustomer() {
    if (!lastSale || !receiptCustomerName.trim()) { setNotice({ kind: 'error', text: 'Enter the customer’s name.' }); return }
    const customer = { name: receiptCustomerName.trim(), phone: receiptCustomerPhone.trim() || null, email: receiptCustomerEmail.trim() || null, notes: null }
    const fingerprint = JSON.stringify(customer)
    setReceiptCustomerBusy(true)
    try {
      let created = pendingReceiptCustomer
      if (!created) {
        const idempotencyKey = idempotencyFor(receiptCustomerAttempt, fingerprint)
        const result = await createStationHostedCustomer({ ...location, customer, idempotencyKey })
        created = result
        setPendingReceiptCustomer(created)
        receiptCustomerAttempt.current = null
      }
      await linkReceiptCustomer(created)
      setReceiptCustomerName(''); setReceiptCustomerPhone(''); setReceiptCustomerEmail('')
    } catch (reason) {
      setNotice({ kind: 'error', text: `${errorText(reason, 'The customer could not be saved or linked.')} Retry without changing the details to avoid a duplicate.` })
    } finally { setReceiptCustomerBusy(false) }
  }

  async function checkOtherBranches(product: BranchProduct) {
    const requestId = ++availabilityRequest.current
    setAvailability({ productId: product.productId, result: { kind: 'loading' } })
    try {
      const branches = await getProductBranchAvailability({ ...location, productId: product.productId })
      if (availabilityRequest.current === requestId) setAvailability({ productId: product.productId, result: { kind: 'ready', branches } })
    } catch (reason) {
      if (availabilityRequest.current === requestId) setAvailability({ productId: product.productId, result: { kind: 'error', message: errorText(reason, 'Branch availability could not be loaded.') } })
    }
  }

  function freshBranchSnapshot(productId: string): ProductBranchAvailability[] | null {
    if (availability?.productId !== productId || availability.result.kind !== 'ready') return null
    if (availability.result.branches.some((branch) => isAvailabilityStale(branch.observedAt, Date.now()))) return null
    return availability.result.branches
  }

  async function requestTransfer(product: BranchProduct, sourceBranchId: string, availableQuantity: number) {
    const sourceBranch = freshBranchSnapshot(product.productId)?.find((branch) => branch.branchId === sourceBranchId)
    if (!sourceBranch) {
      setNotice({ kind: 'error', text: 'Branch availability is stale or missing. Refresh it before requesting a transfer.' }); return
    }
    const quantity = Number(transferQuantity)
    if (!Number.isSafeInteger(quantity) || quantity < 1 || quantity > Math.min(availableQuantity, sourceBranch.availableToPromise)) {
      setNotice({ kind: 'error', text: `Enter a whole number from 1 to ${availableQuantity}, based on the latest branch stock snapshot.` }); return
    }
    setTransferBusy(true)
    const note = `Requested from ${stationName} for a customer.`
    const fingerprint = JSON.stringify({ sourceBranchId, productId: product.productId, quantity, note })
    const idempotencyKey = idempotencyFor(transferAttempt, fingerprint)
    try {
      await requestHostedStockTransfer({ ...location, sourceBranchId, productId: product.productId, quantity, note, idempotencyKey })
      transferAttempt.current = null
      setNotice({ kind: 'success', text: `Requested ${quantity} × ${product.name}. The source branch must accept and dispatch it before stock moves.` })
    } catch (reason) { setNotice({ kind: 'error', text: errorText(reason, 'The stock request could not be submitted.') }) }
    finally { setTransferBusy(false) }
  }

  function openPickupForm(product: BranchProduct, branch: ProductBranchAvailability) {
    setPickupDraft({ productId: product.productId, pickupBranchId: branch.branchId })
    setPickupQuantity('1')
    const customer = customers.find((entry) => entry.id === selectedCustomerId)
    setPickupContactName(customer?.name ?? '')
    setPickupContactPhone(customer?.phone ?? '')
    setPickupContactConsent(false)
    setNotice(null)
  }

  async function requestPickup(product: BranchProduct, branch: ProductBranchAvailability) {
    const freshBranch = freshBranchSnapshot(product.productId)?.find((entry) => entry.branchId === branch.branchId)
    if (!freshBranch) {
      setNotice({ kind: 'error', text: 'Branch availability is stale or missing. Refresh it before reserving a pickup.' }); return
    }
    const quantity = Number(pickupQuantity)
    if (!Number.isSafeInteger(quantity) || quantity < 1 || quantity > freshBranch.availableToPromise) {
      setNotice({ kind: 'error', text: `Enter a whole number from 1 to ${freshBranch.availableToPromise}, based on the latest available-to-promise quantity.` })
      return
    }
    const contactName = pickupContactName.trim()
    const contactPhone = pickupContactPhone.trim()
    if (!contactName || !contactPhone || !pickupContactConsent) {
      setNotice({ kind: 'error', text: 'Enter the customer’s name and phone number, then record their consent to use those details for this pickup.' })
      return
    }
    const fingerprint = JSON.stringify({ pickupBranchId: branch.branchId, productId: product.productId, quantity, contactName, contactPhone, customerId: selectedCustomerId || null, holdMinutes: 60 })
    const idempotencyKey = idempotencyFor(pickupAttempt, fingerprint)
    setPickupBusy(true)
    setNotice(null)
    try {
      const reservation = await createHostedPickupReservation({
        ...location,
        reservation: {
          pickupBranchId: branch.branchId,
          customerId: selectedCustomerId || null,
          contactName,
          contactPhone,
          contactConsent: true,
          holdMinutes: 60,
          items: [{ productId: product.productId, quantity }],
        },
        idempotencyKey,
      })
      pickupAttempt.current = null
      setPickupDraft(null)
      setNotice({ kind: 'success', text: `Pickup ${reservation.reservationId.slice(0, 8)} reserved at ${branch.branchName} until ${new Date(reservation.expiresAt).toLocaleString()}.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: errorText(reason, 'The pickup hold could not be saved. Check the latest availability and retry.') })
    } finally {
      setPickupBusy(false)
    }
  }

  async function activateStation() {
    setBusy('activation')
    setNotice(null)
    try {
      const stationKey = await getOrCreateStationKey(stationId)
      const result = await activateHostedStation({ ...location, key: stationKey })
      setKey(stationKey)
      setNotice({ kind: 'success', text: `This register is linked to this browser. ${result.activeStationCount} of ${result.stationLimit} licensed station slots are active.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: errorText(reason, 'This register could not be linked to the device.') })
    } finally {
      setBusy(null)
    }
  }

  async function startCashierMode() {
    setBusy('lease')
    setNotice(null)
    try {
      const stationKey = await getOrCreateStationKey(stationId)
      const activeLease = await acquireHostedCashierLease({ ...location, key: stationKey })
      setKey(stationKey)
      setLease(activeLease)
      setDrawer(await getHostedDrawerShift(location))
      setNotice({ kind: 'success', text: 'Cashier mode is active on this station. Other staff cannot transact here until you switch out or your session expires.' })
    } catch (reason) {
      setNotice({ kind: 'error', text: errorText(reason, 'Cashier mode could not be started.') })
    } finally {
      setBusy(null)
    }
  }

  async function stopCashierMode() {
    const current = leaseRef.current
    if (!current) return
    setBusy('lease')
    setNotice(null)
    try {
      await releaseHostedCashierLease({ apiBaseUrl, accessToken, lease: current })
      leaseRef.current = null
      setLease(null)
      setNotice({ kind: 'success', text: 'Cashier mode has ended. The next assigned staff member can take this station.' })
    } catch (reason) {
      setNotice({ kind: 'error', text: `Cashier mode could not be released. It will expire automatically. ${errorText(reason, '')}`.trim() })
    } finally {
      setBusy(null)
    }
  }

  async function openDrawer() {
    const amount = parseMinorAmount(openingCash)
    if (!lease || !key || amount === null) {
      setNotice({ kind: 'error', text: 'Enter a valid opening float before opening the drawer.' })
      return
    }
    setBusy('drawer-open')
    setNotice(null)
    try {
      const body = { openingCashMinor: amount }
      const idempotencyKey = idempotencyFor(openingAttempt, JSON.stringify(body))
      setDrawer(await openHostedDrawer({ ...location, lease, key, ...body, idempotencyKey }))
      setOpeningCash('')
      openingAttempt.current = null
      setNotice({ kind: 'success', text: 'Cash drawer shift opened.' })
    } catch (reason) {
      setNotice({ kind: 'error', text: errorText(reason, 'The cash drawer could not be opened.') })
    } finally {
      setBusy(null)
    }
  }

  async function postMovement() {
    const amount = parseMinorAmount(movementAmount)
    if (!lease || !key || !drawer || amount === null || movementNote.trim().length < 1 || (movementKind === 'cash_drop' && !cashDropDestinationId)) {
      setNotice({ kind: 'error', text: movementKind === 'cash_drop' && !cashDropDestinationId ? 'Choose an initialized safe or cash location to receive the drop.' : 'Enter a valid amount and note for the drawer movement.' })
      return
    }
    setBusy('movement')
    setNotice(null)
    try {
      const movement = { shiftId: drawer.shiftId, movementType: movementKind, amountMinor: amount, note: movementNote.trim(), ...(movementKind === 'cash_drop' ? { destinationLocationId: cashDropDestinationId } : {}) }
      const idempotencyKey = idempotencyFor(movementAttempt, JSON.stringify(movement))
      setDrawer(await recordHostedDrawerMovement({ ...location, lease, key, ...movement, idempotencyKey }))
      setMovementAmount('')
      setMovementNote('')
      movementAttempt.current = null
      setNotice({ kind: 'success', text: 'Cash drawer movement recorded.' })
    } catch (reason) {
      setNotice({ kind: 'error', text: errorText(reason, 'The drawer movement could not be recorded.') })
    } finally {
      setBusy(null)
    }
  }

  async function openSupplierPayment() {
    if (!canSettle || !lease || !key || !drawer || drawer.isClosed) {
      setNotice({ kind: 'error', text: 'Switch to a cashier profile that also has supplier settlement permission, then open the drawer before recording a cash supplier payment.' })
      return
    }
    setBusy('supplier-payment')
    try {
      const workspace = await getBranchSupplierBilling({ apiBaseUrl, accessToken, tenantId, branchId })
      setSupplierBilling(workspace)
      setSupplierPaymentSupplier('')
      setSupplierPaymentAmounts({})
      setSupplierPaymentOpen(true)
    } catch (reason) {
      setNotice({ kind: 'error', text: errorText(reason, 'Supplier invoices could not be loaded.') })
    } finally {
      setBusy(null)
    }
  }

  async function submitSupplierCashPayment(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!supplierBilling || !supplierPaymentSupplier || !lease || !key || !drawer || drawer.isClosed) return
    const allocations = Object.entries(supplierPaymentAmounts)
      .flatMap(([billId, value]) => {
        const amountMinor = parseMinorAmount(value)
        return amountMinor !== null && amountMinor > 0 ? [{ billId, amountMinor }] : []
      })
      .sort((a, b) => a.billId.localeCompare(b.billId))
    const amountMinor = allocations.reduce((sum, allocation) => sum + allocation.amountMinor, 0)
    if (!allocations.length || !Number.isSafeInteger(amountMinor) || allocations.some((allocation) => {
      const bill = supplierBilling.bills.find((item) => item.id === allocation.billId)
      return !bill || bill.supplierId !== supplierPaymentSupplier || allocation.amountMinor > bill.balanceMinor
    })) {
      setNotice({ kind: 'error', text: 'Allocate a valid amount within each selected invoice balance.' })
      return
    }
    const payment = {
      supplierId: supplierPaymentSupplier,
      paymentMethod: 'cash' as const,
      externalReference: supplierPaymentReference.trim() || null,
      amountMinor,
      drawerShiftId: drawer.shiftId,
      allocations,
    }
    const fingerprint = JSON.stringify(payment)
    const idempotencyKey = idempotencyFor(supplierPaymentAttempt, fingerprint)
    setBusy('supplier-payment')
    setNotice(null)
    try {
      const result = await recordStationSupplierCashPayment({
        ...location, lease, key, idempotencyKey, drawerShiftId: drawer.shiftId,
        supplierId: payment.supplierId, externalReference: payment.externalReference,
        amountMinor, allocations,
      })
      supplierPaymentAttempt.current = null
      setSupplierPaymentReference('')
      setSupplierPaymentAmounts({})
      setSupplierPaymentOpen(false)
      const [workspace, updatedDrawer] = await Promise.all([
        getBranchSupplierBilling({ apiBaseUrl, accessToken, tenantId, branchId }),
        getHostedDrawerShift(location),
      ])
      setSupplierBilling(workspace)
      setDrawer(updatedDrawer)
      setNotice({ kind: 'success', text: `Cash supplier payment recorded and linked to this drawer${result.replayed ? ' (retry confirmed)' : ''}.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: `${errorText(reason, 'The supplier payment could not be recorded.')}${reason instanceof ApiError && reason.status >= 400 && reason.status < 500 ? '' : ' Retry unchanged to avoid a duplicate.'}` })
    } finally {
      setBusy(null)
    }
  }

  async function openReceivableCollection() {
    if (!selectedCustomerId || !canCheckout || !lease || !key || !drawer || drawer.isClosed) {
      setNotice({ kind: 'error', text: 'Select a customer and make sure the cashier lease and drawer shift are open.' })
      return
    }
    setReceivableCollectionLoading(true)
    try {
      const invoices = await listStationHostedOpenReceivables({ ...location, customerId: selectedCustomerId })
      setOpenReceivables(invoices)
      setReceivableCollectionAmounts({})
      setReceivableCollectionReference('')
      setReceivableCollectionOpen(true)
      if (invoices.length === 0) setNotice({ kind: 'success', text: 'This customer has no open invoice balances in this branch currency.' })
    } catch (reason) {
      setNotice({ kind: 'error', text: errorText(reason, 'Open customer invoices could not be loaded.') })
    } finally {
      setReceivableCollectionLoading(false)
    }
  }

  async function submitReceivableCashCollection(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selectedCustomerId || !lease || !key || !drawer || drawer.isClosed) return
    const allocations = openReceivables.flatMap((invoice) => {
      const amountMinor = parseMinorAmount(receivableCollectionAmounts[invoice.invoiceId] ?? '')
      return amountMinor !== null && amountMinor > 0
        ? [{ invoiceBranchId: invoice.invoiceBranchId, invoiceId: invoice.invoiceId, amountMinor }]
        : []
    }).sort((a, b) => `${a.invoiceBranchId}:${a.invoiceId}`.localeCompare(`${b.invoiceBranchId}:${b.invoiceId}`))
    const amountMinor = allocations.reduce((sum, allocation) => sum + allocation.amountMinor, 0)
    if (!allocations.length || !Number.isSafeInteger(amountMinor) || allocations.some((allocation) => {
      const invoice = openReceivables.find((item) => item.invoiceId === allocation.invoiceId && item.invoiceBranchId === allocation.invoiceBranchId)
      return !invoice || allocation.amountMinor > Number(invoice.openMinor)
    })) {
      setNotice({ kind: 'error', text: 'Allocate a valid cash amount within each invoice’s remaining balance.' })
      return
    }
    const collection = {
      customerId: selectedCustomerId,
      amountMinor,
      drawerShiftId: drawer.shiftId,
      externalReference: receivableCollectionReference.trim() || null,
      allocations,
    }
    const idempotencyKey = idempotencyFor(receivableCollectionAttempt, JSON.stringify(collection))
    setBusy('receivable-collection')
    setNotice(null)
    try {
      const result = await recordStationHostedCashCollection({ ...location, lease, key, idempotencyKey, collection })
      receivableCollectionAttempt.current = null
      setReceivableCollectionOpen(false)
      setReceivableCollectionAmounts({})
      setReceivableCollectionReference('')
      const [updatedInvoices, updatedPosition, updatedDrawer] = await Promise.all([
        listStationHostedOpenReceivables({ ...location, customerId: selectedCustomerId }),
        getStationHostedCustomerCreditPosition({ ...location, customerId: selectedCustomerId }),
        getHostedDrawerShift(location),
      ])
      setOpenReceivables(updatedInvoices)
      setCreditPosition(updatedPosition)
      setDrawer(updatedDrawer)
      setNotice({
        kind: 'success',
        text: `Cash payment of ${formatMoney(result.collection.currencyCode, result.collection.amountMinor)} recorded${result.replayed ? ' (retry confirmed)' : ''} and allocated to ${result.collection.allocationCount} invoice${result.collection.allocationCount === 1 ? '' : 's'}.`,
      })
    } catch (reason) {
      setNotice({ kind: 'error', text: `${errorText(reason, 'The customer payment could not be recorded.')}${reason instanceof ApiError && reason.status >= 400 && reason.status < 500 ? '' : ' Retry unchanged to avoid a duplicate.'}` })
    } finally {
      setBusy(null)
    }
  }

  async function submitCashExpense(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!canRecordCashExpense || !lease || !key || !drawer || drawer.isClosed) return
    const amountMinor = parseMinorAmount(cashExpenseAmount)
    if (!cashExpenseDescription.trim() || amountMinor === null || !cashExpenseDate) {
      setNotice({ kind: 'error', text: 'Enter an expense description, valid amount and date.' })
      return
    }
    const expense: HostedExpenseInput = {
      categoryCode: cashExpenseCategory, description: cashExpenseDescription.trim(), payee: cashExpensePayee.trim() || null,
      amountMinor, paymentMethod: 'other', cashLocationId: null, externalReference: cashExpenseReference.trim() || null, occurredOn: cashExpenseDate,
    }
    const request = { ...expense, paymentMethod: 'cash', drawerShiftId: drawer.shiftId }
    const idempotencyKey = idempotencyFor(cashExpenseAttempt, JSON.stringify(request))
    setBusy('cash-expense')
    setNotice(null)
    try {
      const result = await recordStationCashExpense({ ...location, lease, key, idempotencyKey, drawerShiftId: drawer.shiftId, expense })
      cashExpenseAttempt.current = null
      setCashExpenseDescription('')
      setCashExpensePayee('')
      setCashExpenseAmount('')
      setCashExpenseReference('')
      setCashExpenseDate(localDateInputValue())
      setCashExpenseOpen(false)
      setDrawer(await getHostedDrawerShift(location))
      setNotice({ kind: 'success', text: `Cash expense recorded and linked to this drawer${result.replayed ? ' (retry confirmed)' : ''}.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: `${errorText(reason, 'The cash expense could not be recorded.')}${reason instanceof ApiError && reason.status >= 400 && reason.status < 500 ? '' : ' Retry unchanged to avoid a duplicate.'}` })
    } finally {
      setBusy(null)
    }
  }

  async function loadXReport() {
    if (!drawer) return
    try {
      setXReport(await getHostedXReport({ ...location, shiftId: drawer.shiftId }))
    } catch (reason) {
      setNotice({ kind: 'error', text: errorText(reason, 'The current X report could not be loaded.') })
    }
  }

  async function loadDrawerHistory() {
    setHistoryBusy(true)
    try {
      setDrawerHistory(await getHostedDrawerHistory(location))
      setHistoryVisible(true)
    } catch (reason) {
      setNotice({ kind: 'error', text: errorText(reason, 'Closed drawer history could not be loaded.') })
    } finally {
      setHistoryBusy(false)
    }
  }

  async function loadSalesHistory(search = salesHistoryAppliedSearch, offset = 0) {
    setSalesHistoryBusy(true)
    setSalesHistoryError(null)
    try {
      const page = await getHostedStationSalesHistory({ ...location, search: search || undefined, offset, pageSize: 25 })
      setSalesHistory(page)
      setSalesHistoryAppliedSearch(search)
    } catch (reason) {
      setSalesHistoryError(errorText(reason, 'Station sales history could not be loaded.'))
    } finally {
      setSalesHistoryBusy(false)
    }
  }

  async function openSalesHistory() {
    setSalesHistoryVisible(true)
    await loadSalesHistory('', 0)
    setSalesHistorySearch('')
  }

  async function searchSalesHistory(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    await loadSalesHistory(salesHistorySearch.trim(), 0)
  }

  async function openHistoricalReceipt(saleId: string) {
    setHistoricalReceiptBusyId(saleId)
    setSalesHistoryVisible(false)
    setSalesHistoryError(null)
    try {
      setHistoricalReceipt(await getHostedStationReceipt({ ...location, saleId }))
    } catch (reason) {
      setSalesHistoryError(errorText(reason, 'The historical receipt could not be loaded.'))
      setSalesHistoryVisible(true)
    } finally {
      setHistoricalReceiptBusyId(null)
    }
  }

  async function openSaleRefund(saleId: string) {
    if (!canRefund || !lease || !key) return
    setRefundBusy(true)
    setSalesHistoryVisible(false)
    setSalesHistoryError(null)
    try {
      const detail = await getHostedSaleRefundDetail({ ...location, saleId })
      setRefundDetail(detail)
      setRefundQuantities(Object.fromEntries(detail.items.map((item) => [item.saleLineId, '0'])))
      setRefundReason('')
      setRefundKind('refund')
      setRefundReference('')
      setRefundRestock(false)
    } catch (reason) {
      setNotice({ kind: 'error', text: errorText(reason, 'Refund details could not be loaded.') })
      setSalesHistoryVisible(true)
    } finally {
      setRefundBusy(false)
    }
  }

  async function submitSaleRefund() {
    if (!refundDetail || !lease || !key || !canRefund) return
    const items = refundDetail.items.map((item) => ({ ...item, quantity: Number(refundQuantities[item.saleLineId] ?? 0) }))
      .filter((item) => Number.isInteger(item.quantity) && item.quantity > 0)
      .map((item) => ({ saleLineId: item.saleLineId, quantity: item.quantity, restock: refundRestock }))
    if (!items.length || items.some((item) => item.quantity > (refundDetail.items.find((line) => line.saleLineId === item.saleLineId)?.remainingQuantity ?? 0))) {
      setNotice({ kind: 'error', text: 'Enter a return quantity within each item’s remaining quantity.' })
      return
    }
    if (refundKind === 'void' && (refundDetail.status !== 'completed' || items.length !== refundDetail.items.length || items.some((item) => item.quantity !== refundDetail.items.find((line) => line.saleLineId === item.saleLineId)?.soldQuantity))) {
      setNotice({ kind: 'error', text: 'A void must include every item at its full quantity before any prior return.' })
      return
    }
    setRefundBusy(true)
    try {
      const refund = {
        kind: refundKind, reason: refundReason, items, paymentMethod: refundMethod,
        paymentReference: refundMethod === 'cash' ? null : refundReference.trim() || null,
      }
      const fingerprint = JSON.stringify({ saleId: refundDetail.saleId, refund })
      const idempotencyKey = idempotencyFor(refundAttempt, fingerprint)
      await refundHostedSale({ ...location, saleId: refundDetail.saleId, lease, key, idempotencyKey, refund })
      refundAttempt.current = null
      setRefundDetail(null)
      setNotice({ kind: 'success', text: 'Refund recorded. For non-cash methods, complete and verify the payout with the payment provider separately.' })
      await loadSalesHistory(salesHistoryAppliedSearch, salesHistory?.offset ?? 0)
    } catch (reason) {
      setNotice({ kind: 'error', text: errorText(reason, 'Refund could not be recorded. Check the sale and drawer, then retry.') })
    } finally {
      setRefundBusy(false)
    }
  }

  async function closeDrawer() {
    const amount = parseMinorAmount(countedCash)
    if (!lease || !key || !drawer || amount === null) {
      setNotice({ kind: 'error', text: 'Enter the cash counted in the drawer to close the shift.' })
      return
    }
    setBusy('drawer-close')
    setNotice(null)
    try {
      const closeBody = { shiftId: drawer.shiftId, countedCashMinor: amount }
      const idempotencyKey = idempotencyFor(closingAttempt, JSON.stringify(closeBody))
      const closed = await closeHostedDrawer({ ...location, lease, key, ...closeBody, idempotencyKey })
      setDrawer(closed)
      setCountedCash('')
      closingAttempt.current = null
      setNotice({ kind: 'success', text: `Drawer closed. Cash difference: ${formatMoney(closed.differenceMinor === null ? 'GHS' : products[0]?.currencyCode ?? 'GHS', closed.differenceMinor ?? 0)}.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: errorText(reason, 'The cash drawer could not be closed.') })
    } finally {
      setBusy(null)
    }
  }

  async function checkout() {
    const retryBody = saleAttempt?.kind === 'exact' ? saleAttempt.body : undefined
    if (!lease || !key || busy !== null || (!retryBody && (cart.length === 0 || (totalMinor > (parseMinorAmount(onAccountAmount) ?? 0) && !payment)))) return
    if (activeSharedTicketId && (!activeSharedClaimReady || activeSharedRevision === null)) {
      setNotice({ kind: 'error', text: 'Claim this shared ticket again before checkout.' })
      return
    }
    if (!retryBody && onAccountAmount.trim() !== '' && parseMinorAmount(onAccountAmount) === null) {
      setNotice({ kind: 'error', text: 'Enter the customer credit amount as a valid money value.' })
      return
    }
    if (!retryBody && (parseMinorAmount(onAccountAmount) ?? 0) > 0) {
      const accountAmount = parseMinorAmount(onAccountAmount) ?? 0
      const available = Number(creditPosition?.availableMinor ?? '0')
      if (!selectedCustomerId || creditPositionLoading || !creditPosition?.hasCreditTerms || !Number.isSafeInteger(available) || accountAmount > available || accountAmount > totalMinor) {
        setNotice({ kind: 'error', text: 'Select a customer with active credit terms and keep the on-account amount within both the sale total and available credit.' })
        return
      }
    }
    if (!retryBody && (parseMinorAmount(onAccountAmount) ?? 0) < totalMinor && payment?.tenderKind === 'cash' && (!drawer || drawer.isClosed)) {
      setNotice({ kind: 'error', text: 'Open the cash drawer before completing a cash sale.' })
      return
    }
    if (!retryBody && totalMinor > (parseMinorAmount(onAccountAmount) ?? 0) && payment?.requiresReference && reference.trim().length === 0) {
      setNotice({ kind: 'error', text: 'Enter the payment reference required by this payment method.' })
      return
    }
    setBusy('checkout')
    setNotice(null)
    let preparedSharedRevision: number | null = null
    try {
      const currentSaleBody: CheckoutRequest = {
        customerId: selectedCustomerId || null,
        items: cart.map(({ product, quantity }) => ({ productId: product.productId, quantity })),
        paymentOptionId: payment?.id ?? '',
        amountMinor: Math.max(0, totalMinor - (parseMinorAmount(onAccountAmount) ?? 0)),
        onAccountMinor: parseMinorAmount(onAccountAmount) ?? 0,
        reference: reference.trim() || null,
        ...(pickupSale ? { pickupReservation: { reservationId: pickupSale.reservationId, expectedRevision: pickupSale.expectedRevision } } : {}),
      }
      const saleBody = retryBody ?? currentSaleBody
      const fingerprint = JSON.stringify(saleBody)
      if (saleAttempt && saleAttempt.fingerprint !== fingerprint) {
        setNotice({ kind: 'error', text: 'A previous sale may still have completed. Retry that exact sale before changing this ticket.' })
        setPendingSale(true)
        return
      }
      const idempotencyKey = saleAttempt?.key ?? crypto.randomUUID()
      const attempt: PendingSaleAttempt = saleAttempt ?? { kind: 'exact', fingerprint, key: idempotencyKey, body: saleBody }
      setSaleAttempt(attempt)
      setPendingSale(true)
      try {
        const draftKey = createDraftKey(tenantId, branchId, stationId, subject)
        const active: HeldTicket = {
          id: 'active',
          createdAt: new Date().toISOString(),
          label: pickupSale ? 'Pickup sale' : selectedCustomerId ? 'Customer sale' : 'Walk-in sale',
          lines: cart.map(({ product, quantity }) => ({ productId: product.productId, quantity })),
          selectedCustomerId,
          selectedPaymentId,
          reference,
          saleAttempt: attempt,
          pickupSale,
        }
        localStorage.setItem(draftKey, JSON.stringify({ version: 2, active, heldTickets }))
      } catch {
        if (!storageWarningShown.current) {
          storageWarningShown.current = true
          setNotice({ kind: 'error', text: 'The browser could not save a recovery copy. Keep this page open until the sale is confirmed.' })
        }
      }
      if (activeSharedTicketId) {
        if (!activeSharedClaimReady || activeSharedRevision === null) throw new Error('Claim this shared ticket again before checkout.')
        const prepared = await prepareBranchHeldTicketCheckout({
          ...location,
          lease,
          ticketId: activeSharedTicketId,
          expectedRevision: activeSharedRevision,
          idempotencyKey,
          body: hostedCommitBody(saleBody, activeSharedTicketId),
        })
        preparedSharedRevision = prepared.revision
        setActiveSharedTicket(prepared.id, prepared.revision, true)
        setBranchHeldTickets((rows) => [prepared, ...rows.filter((row) => row.id !== prepared.id)])
      }
      const sale = await commitHostedSale({
        ...location, lease, key, idempotencyKey, ...saleBody,
        ...(activeSharedTicketId ? { heldTicketId: activeSharedTicketId } : {}),
      })
      const paymentLabel = paymentOptions.find((option) => option.id === saleBody.paymentOptionId)?.label ?? 'Payment'
      setLastSale(sale)
      setLastSaleLines(cart.map(({ product, quantity }) => ({ name: product.name, quantity, unitPriceMinor: product.priceMinor })))
      setLastSalePayment(paymentLabel)
      setReceiptCustomerRows([]); setReceiptCustomerId(''); setPendingReceiptCustomer(null)
      setSaleAttempt(null)
      setPendingSale(false)
      if (activeSharedTicketId) {
        setActiveSharedTicket(null, null, false)
        await refreshBranchHeldTickets()
      }
      setCart([])
      setPickupSale(null)
      setSelectedCustomerId('')
      setReference('')
      setOnAccountAmount('')
      setNotice({ kind: 'success', text: `Sale completed. Receipt ${sale.receiptNumber}. ${sale.cashierName || 'Cashier'} served this customer.${sale.onAccountMinor > 0 ? ` ${formatMoney(sale.currencyCode, sale.onAccountMinor)} added to customer credit; due in ${creditPosition?.dueDays ?? 0} days.` : ''}` })
      if (paymentOptions.find((option) => option.id === saleBody.paymentOptionId)?.tenderKind === 'cash') {
        setDrawer(await getHostedDrawerShift(location))
      }
      scanRef.current?.focus()
    } catch (reason) {
      const knownRejection = reason instanceof ApiError && reason.status >= 400 && reason.status < 500
      if (knownRejection) {
        if (activeSharedTicketId && lease && preparedSharedRevision !== null) {
          try {
            const released = await clearBranchHeldTicketCheckoutAttempt({
              ...location, lease, ticketId: activeSharedTicketId, expectedRevision: preparedSharedRevision,
            })
            setActiveSharedTicket(released.id, released.revision, true)
          } catch (clearReason) {
            setNotice({ kind: 'error', text: `Checkout was rejected, but the shared attempt could not be safely released. Keep this ticket unchanged and retry recovery. ${errorText(clearReason, '')}`.trim() })
            setPendingSale(true)
            return
          }
        }
        setSaleAttempt(null)
        setPendingSale(false)
      } else {
        setPendingSale(true)
      }
      setNotice({ kind: 'error', text: `${errorText(reason, 'The sale could not be confirmed.')} ${knownRejection ? 'Review the error, correct it, then try again.' : 'The sale status is unknown. Retry this exact sale; do not change the ticket.'}` })
    } finally {
      setBusy(null)
    }
  }

  function sellHeldReservation(reservation: HostedPickupReservation) {
    if (cart.length > 0 || pendingSale || busy !== null) {
      setNotice({ kind: 'error', text: 'Finish or clear the current ticket before loading a pickup sale so its items are not replaced.' })
      return
    }
    const heldLines = reservation.items.map((item) => {
      const product = products.find((candidate) => candidate.productId === item.productId)
      return product ? { product, quantity: item.quantity } : null
    })
    if (heldLines.some((line) => line === null)) {
      setNotice({ kind: 'error', text: 'One or more reserved products are no longer active at this branch. Ask a manager to review the pickup before selling.' })
      return
    }
    setCart(heldLines.filter((line): line is CartLine => line !== null))
    setPickupSale({ reservationId: reservation.reservationId, expectedRevision: reservation.revision, items: reservation.items.map(({ productId, quantity }) => ({ productId, quantity })) })
    setSelectedCustomerId(reservation.customerId ?? '')
    setNotice({ kind: 'success', text: `Pickup ${reservation.reservationId.slice(0, 8).toUpperCase()} loaded. Keep every held quantity on the ticket; checkout consumes the hold and sale together.` })
    pickupQueueDialogRef.current?.close()
    setPickupQueueOpen(false)
  }

  if (!canView) return null

  const lastSaleCustomer = lastSale?.customerId ? customers.find((customer) => customer.id === lastSale.customerId) ?? null : null

  return <section className="hosted-register" aria-labelledby="hosted-register-title">
    <header className="hosted-register-header">
      <div><p className="eyebrow">HOSTED REGISTER</p><h2 id="hosted-register-title">Sales terminal</h2><p>{branchName} <span>·</span> {stationName}</p></div>
      <div className="register-header-actions">
        <span className="register-presence"><span className={lease ? 'presence-dot presence-live' : 'presence-dot'}/>{lease ? 'Cashier mode active' : 'View mode'}</span>
        {canReadPickups && <button type="button" className="icon-button" onClick={() => { setPickupQueueOpen(true); pickupQueueDialogRef.current?.showModal() }} aria-label="Open pickup reservation queue" title="Pickup reservations"><PackageCheck size={17}/></button>}
        <button type="button" className="button button-quiet" onClick={() => void openSalesHistory()} disabled={salesHistoryBusy}><History size={15}/>{salesHistoryBusy ? 'Loading sales…' : 'Sales history'}</button>
        {canCheckout && !lease && <button type="button" className="button button-primary" onClick={() => void startCashierMode()} disabled={busy !== null}><ShoppingBag size={15}/>{busy === 'lease' ? 'Starting…' : 'Switch to cashier mode'}</button>}
        {lease && <button type="button" className="button button-quiet" onClick={() => void stopCashierMode()} disabled={busy !== null}><LockKeyhole size={15}/> Switch out</button>}
        <button type="button" className="icon-button register-refresh" onClick={() => void loadWorkspace()} disabled={catalogState === 'loading'} aria-label="Refresh register products" title="Refresh"><RefreshCw size={16}/></button>
      </div>
    </header>

    {notice && <div className={`notice ${notice.kind === 'error' ? 'notice-danger' : 'notice-success'}`} role={notice.kind === 'error' ? 'alert' : 'status'}>{notice.kind === 'error' ? <CircleAlert size={16}/> : <Check size={16}/>}<span>{notice.text}</span><button type="button" onClick={() => setNotice(null)} aria-label="Dismiss message">×</button></div>}

    {canActivate && <aside className="register-activation"><div><ShieldCheck size={17}/><span><strong>Station device link</strong><small>The owner links this browser to a licensed station once. Its private signing key stays in this browser’s protected key store.</small></span></div><button type="button" className="button button-quiet" onClick={() => void activateStation()} disabled={busy !== null}>{busy === 'activation' ? 'Linking…' : 'Link this browser'}</button></aside>}

    <div className="register-workspace">
      <section className="register-products" aria-label="Product catalog">
        <div className="register-search-row"><label className="catalog-search"><Search size={16}/><input ref={scanRef} type="search" value={query} onChange={(event) => setQuery(event.currentTarget.value)} onKeyDown={(event) => { if (event.key === 'Enter') scanOrSearch(event.currentTarget.value) }} placeholder="Scan barcode or search products" aria-label="Search or scan products" disabled={pendingSale || busy === 'checkout'}/><Barcode size={16}/></label></div>
        {catalogState === 'loading' && <div className="catalog-state"><span className="mini-spinner"/>Loading register products…</div>}
        {catalogState === 'error' && <div className="catalog-state catalog-state-error"><CircleAlert size={18}/><span>Product or payment data is unavailable. Retry when the connection returns.</span><button type="button" className="button button-quiet" onClick={() => void loadWorkspace()}>Retry</button></div>}
        {catalogState === 'ready' && visibleProducts.length === 0 && <div className="register-empty"><Store size={22}/><strong>{query ? 'No matching products' : 'No products available at this station'}</strong><span>{query ? 'Check the spelling or scan a product barcode.' : 'A manager can assign products to this branch catalog.'}</span></div>}
        <div className="register-product-grid">{visibleProducts.map((product) => {
          const productAvailability = availability?.productId === product.productId ? availability.result : null
          const availabilityFresh = productAvailability?.kind === 'ready'
            && productAvailability.branches.length > 0
            && productAvailability.branches.every((branch) => !isAvailabilityStale(branch.observedAt, availabilityNowMs))
          return <article className="register-product-card" key={product.productId}>
            <button type="button" className="register-product" onClick={() => addProduct(product)} disabled={pendingSale || busy === 'checkout'}>
              <span className="register-product-icon"><ShoppingBag size={18}/></span><span className="register-product-copy"><strong>{product.name}</strong><small>{product.sku}{product.stockQuantity <= product.reorderPoint ? ' · Low stock' : ''}</small></span>
              <span className="register-product-price">{formatMoney(product.currencyCode, product.priceMinor)}</span><span className="register-add"><Plus size={17}/></span>
            </button>
            {canCheckOtherBranches && <button type="button" className="register-other-branches" onClick={() => void checkOtherBranches(product)} aria-expanded={productAvailability !== null}>
              <Store size={13}/>{productAvailability?.kind === 'loading' ? 'Checking…' : 'Check other branches'}
            </button>}
            {productAvailability && <div className="station-availability register-availability" aria-live="polite">
              {productAvailability.kind === 'loading' && <span><span className="mini-spinner"/> Checking branch stock…</span>}
              {productAvailability.kind === 'error' && <span className="station-availability-error" role="alert"><CircleAlert size={15}/>{productAvailability.message}</span>}
              {productAvailability.kind === 'ready' && <>
                <div className="station-availability-heading"><strong>Availability across your business</strong><span className="station-availability-actions"><button type="button" onClick={() => void checkOtherBranches(product)} aria-label="Refresh branch availability" title="Refresh branch availability"><RefreshCw size={15}/></button><button type="button" onClick={() => { availabilityRequest.current++; setAvailability(null) }} aria-label="Close branch availability">×</button></span></div>
                {productAvailability.branches[0] && <p className={`station-availability-checked${availabilityFresh ? '' : ' stale'}`}>{availabilityFresh ? 'Server snapshot' : 'Stale snapshot'} · checked {formatObservedAt(productAvailability.branches[0].observedAt)}{availabilityFresh ? '' : ' · refresh before requesting stock or pickup'}</p>}
                {productAvailability.branches.map((branch) => {
                  const pickupOpen = pickupDraft?.productId === product.productId && pickupDraft.pickupBranchId === branch.branchId
                  const branchFresh = availabilityFresh && !isAvailabilityStale(branch.observedAt, availabilityNowMs)
                  return <div className={`station-branch-stock transfer-availability-row${branchFresh ? '' : ' stale'}`} key={branch.branchId}>
                    <span className="station-branch-name"><strong>{branch.branchName}</strong><small>{branch.branchCode}{branch.branchId === branchId ? ' · This branch' : ''}</small></span>
                    <span className="station-branch-metrics"><strong className={branch.stockQuantity === 0 ? 'stock-zero' : ''}>{branch.stockQuantity} on hand</strong><small>{branch.pickupReservedQuantity} held · {branch.outboundCommittedQuantity} transfer committed</small></span>
                    <span className="station-branch-metrics"><strong className={branch.availableToPromise === 0 ? 'stock-zero' : ''}>{branch.availableToPromise} available</strong><small>{formatMoney(branch.currencyCode, branch.priceMinor)} each</small></span>
                    {canRequestTransfers && branch.branchId !== branchId && branch.availableToPromise > 0 && <div className="availability-transfer-action"><label>Qty <input type="number" min="1" max={branch.availableToPromise} value={transferQuantity} onChange={(event) => setTransferQuantity(event.target.value)} aria-label={`Quantity to request from ${branch.branchName}`} disabled={!branchFresh || transferBusy}/></label><button className="button button-quiet" type="button" disabled={!branchFresh || transferBusy} title={branchFresh ? undefined : 'Refresh branch availability before requesting'} onClick={() => void requestTransfer(product, branch.branchId, branch.availableToPromise)}>{transferBusy ? 'Sending…' : branchFresh ? 'Request stock transfer' : 'Refresh before request'}</button></div>}
                    {canRequestPickups && branch.branchId !== branchId && branch.availableToPromise > 0 && <div className="availability-pickup-trigger"><button className="button button-quiet button-small" type="button" aria-expanded={pickupOpen} disabled={!branchFresh && !pickupOpen} title={branchFresh || pickupOpen ? undefined : 'Refresh branch availability before requesting'} onClick={() => pickupOpen ? setPickupDraft(null) : openPickupForm(product, branch)}><Store size={13}/>{pickupOpen ? 'Close pickup request' : branchFresh ? 'Reserve for pickup here' : 'Refresh before pickup'}</button></div>}
                    {pickupOpen && <div className="pickup-reservation-form" aria-label={`Pickup reservation for ${branch.branchName}`}>
                      <div className="pickup-reservation-heading"><strong>Hold stock at {branch.branchName}</strong><span>60-minute hold · only available-to-promise stock can be held.</span></div>
                      <label>Quantity<input type="number" min="1" max={branch.availableToPromise} step="1" inputMode="numeric" value={pickupQuantity} onChange={(event) => setPickupQuantity(event.currentTarget.value)} aria-label={`Pickup quantity at ${branch.branchName}`} disabled={pickupBusy}/></label>
                      <label>Customer name<input autoComplete="name" maxLength={160} value={pickupContactName} onChange={(event) => setPickupContactName(event.currentTarget.value)} disabled={pickupBusy}/></label>
                      <label>Phone number<input type="tel" autoComplete="tel" maxLength={40} value={pickupContactPhone} onChange={(event) => setPickupContactPhone(event.currentTarget.value)} disabled={pickupBusy}/></label>
                      <label className="pickup-consent"><input type="checkbox" checked={pickupContactConsent} onChange={(event) => setPickupContactConsent(event.currentTarget.checked)} disabled={pickupBusy}/> Customer agrees we can use these contact details to coordinate this pickup.</label>
                      <div className="pickup-reservation-actions"><span>Contact details are shared with staff handling this reservation.</span><button className="button button-primary button-small" type="button" disabled={!branchFresh || pickupBusy || !pickupContactConsent} onClick={() => void requestPickup(product, branch)}>{pickupBusy ? 'Saving hold…' : branchFresh ? 'Confirm 60-minute hold' : 'Refresh before reserving'}</button></div>
                    </div>}
                  </div>
                })}
                {productAvailability.branches.length === 0 && <p>No active branches have this product assigned.</p>}
                <p className="station-catalog-disclosure">Availability is a server snapshot. Transfer requests need source-branch approval; a pickup hold is confirmed only after the server saves it.</p>
              </>}
            </div>}
          </article>
        })}</div>
        <footer className="register-catalog-footer"><span>{visibleProducts.length} products</span><span>Prices and stock are checked again by the server at checkout.</span></footer>
      </section>

      <aside className="register-ticket" aria-label="Current sale">
        <div className="ticket-heading"><div><p className="eyebrow">CURRENT SALE</p><h3>Order ticket</h3>{pickupSale && <small>Pickup {pickupSale.reservationId.slice(0, 8).toUpperCase()} · held quantities stay on this sale</small>}</div><div className="register-ticket-actions">
          <button type="button" className="button button-quiet button-small" onClick={holdCurrentTicket} disabled={(cart.length === 0 && !saleAttempt) || busy !== null}><Clock3 size={14}/> Hold</button>
          <button type="button" className="button button-quiet button-small" onClick={() => setHeldTicketsOpen((open) => !open)} aria-expanded={heldTicketsOpen}><PackageCheck size={14}/> Held {heldTickets.length + branchHeldTickets.filter((ticket) => ticket.status === 'held' || ticket.status === 'checkout_pending').length}</button>
          <button type="button" className="button button-quiet button-small" onClick={resetCurrentTicket} disabled={cart.length === 0 || pendingSale || busy === 'checkout'} aria-label="Clear all items"><Trash2 size={14}/> Clear</button>
        </div></div>
        {heldTicketsOpen && <div className="register-held-tickets" aria-label="Held sales">
          {heldTickets.length === 0 ? <p>No held tickets on this device for this staff profile.</p> : heldTickets.map((ticket) => <article className="register-held-ticket" key={ticket.id}>
            <div><strong>{ticket.label}</strong><small>{ticket.lines.length} items · {new Date(ticket.createdAt).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}</small>{ticket.saleAttempt && <small className="register-held-ticket-warning">Checkout result unknown · retry unchanged</small>}</div>
            <button type="button" className="button button-quiet button-small" onClick={() => resumeHeldTicket(ticket.id)} disabled={busy !== null}>Resume</button>
            <button type="button" className="button button-quiet button-small" onClick={() => void shareHeldTicket(ticket.id)} disabled={!lease || Boolean(ticket.saleAttempt) || busy !== null} title="Share this ticket with cashiers at this branch">Share with branch</button>
            <button type="button" className="button button-quiet button-small" onClick={() => discardHeldTicket(ticket.id)} disabled={Boolean(ticket.saleAttempt) || busy !== null} aria-label={`Discard ${ticket.label}`} title={ticket.saleAttempt ? 'Resume and retry the unknown checkout before discarding.' : 'Discard held ticket'}><X size={14}/></button>
          </article>)}
          <section className="register-branch-held-tickets" aria-label="Tickets shared with this branch">
            <div className="register-held-ticket-heading"><strong>Shared with this branch</strong><button type="button" className="button button-quiet button-small" onClick={() => void refreshBranchHeldTickets()} disabled={branchHeldLoading}>{branchHeldLoading ? 'Refreshing…' : 'Refresh'}</button></div>
            {branchHeldError && <p role="alert" className="form-error">{branchHeldError}</p>}
            {branchHeldTickets.filter((ticket) => ticket.status === 'held' || ticket.status === 'checkout_pending').length === 0
              ? <p>No open tickets have been shared with this branch.</p>
              : branchHeldTickets.filter((ticket) => ticket.status === 'held' || ticket.status === 'checkout_pending').map((ticket) => <article className="register-held-ticket" key={ticket.id}>
                <div><strong>{ticket.payload.label}</strong><small>{ticket.payload.lines.length} items · {ticket.status === 'checkout_pending' ? 'checkout needs exact retry' : ticket.claimLeaseId ? 'in use by another cashier' : 'available to claim'}</small></div>
                <button type="button" className="button button-quiet button-small" onClick={() => void resumeBranchHeldTicket(ticket)} disabled={!canCheckout || !lease || Boolean(ticket.claimLeaseId && ticket.claimLeaseId !== lease.leaseId) || busy !== null || cart.length > 0 || Boolean(saleAttempt)}>{ticket.claimLeaseId === lease?.leaseId ? 'Claimed' : 'Claim / resume'}</button>
              </article>)}
          </section>
        </div>}
        <div className="ticket-items" aria-live="polite">{cart.length === 0 ? <div className="ticket-empty"><ShoppingBag size={20}/><span>{pendingSale ? 'A saved checkout is waiting to be retried unchanged.' : 'Scan or select a product to start this sale.'}</span></div> : cart.map(({ product, quantity }) => <article className="ticket-line" key={product.productId}>
          <div className="ticket-line-info"><strong>{product.name}</strong><small>{formatMoney(product.currencyCode, product.priceMinor)} each</small></div>
          <div className="ticket-quantity"><button type="button" onClick={() => changeQuantity(product.productId, -1)} disabled={pendingSale || busy === 'checkout' || quantity <= (pickupSale?.items.find((item) => item.productId === product.productId)?.quantity ?? 0)} aria-label={`Remove one ${product.name}`}><Minus size={13}/></button><span>{quantity}</span><button type="button" onClick={() => changeQuantity(product.productId, 1)} disabled={pendingSale || busy === 'checkout'} aria-label={`Add one ${product.name}`}><Plus size={13}/></button></div>
          <strong className="ticket-line-total">{formatMoney(product.currencyCode, product.priceMinor * quantity)}</strong>
        </article>)}</div>
        <div className="ticket-totals"><div><span>Subtotal</span><strong>{formatMoney(products[0]?.currencyCode ?? 'GHS', subtotalMinor)}</strong></div><div><span>Tax estimate</span><strong>{formatMoney(products[0]?.currencyCode ?? 'GHS', taxMinor)}</strong></div><div className="ticket-grand-total"><span>Estimated total</span><strong>{formatMoney(products[0]?.currencyCode ?? 'GHS', totalMinor)}</strong></div></div>
        <div className="ticket-payment">
          <label htmlFor="register-customer">Customer</label>
          <div className="register-customer-row"><input aria-label="Search customers by name, phone or email" placeholder="Search name, phone or email" value={customerSearch} onChange={(event) => setCustomerSearch(event.currentTarget.value)} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); void findCustomers(0) } }} disabled={pendingSale || busy === 'checkout' || customerSearchBusy}/><button type="button" className="button button-quiet button-small" onClick={() => void findCustomers(0)} disabled={pendingSale || busy === 'checkout' || customerSearchBusy}>{customerSearchBusy ? 'Searching…' : 'Search'}</button></div>
          <div className="register-customer-row"><select id="register-customer" value={selectedCustomerId} onChange={(event) => { setSelectedCustomerId(event.currentTarget.value); setOnAccountAmount('') }} disabled={pendingSale || busy === 'checkout'}><option value="">Walk-in customer</option>{customers.map((customer) => <option key={customer.id} value={customer.id}>{customer.name}{customer.phone ? ` · ${customer.phone}` : ''}</option>)}</select><button type="button" className="button button-quiet button-small" onClick={() => setCustomerFormOpen((value) => !value)} disabled={pendingSale || busy === 'checkout'}>{customerFormOpen ? 'Cancel' : 'Add customer'}</button></div>
          {customerHasMore && <button type="button" className="button button-quiet button-small" onClick={() => void findCustomers(customerPageOffset)} disabled={customerSearchBusy}>{customerSearchBusy ? 'Loading…' : 'Load more customers'}</button>}
          {customerFormOpen && <div className="register-customer-form"><label>Name<input autoComplete="name" maxLength={160} value={customerName} onChange={(event) => setCustomerName(event.target.value)} disabled={customerBusy}/></label><label>Phone<input autoComplete="tel" maxLength={40} value={customerPhone} onChange={(event) => setCustomerPhone(event.target.value)} placeholder="Optional" disabled={customerBusy}/></label><label>Email<input autoComplete="email" type="email" maxLength={254} value={customerEmail} onChange={(event) => setCustomerEmail(event.target.value)} placeholder="Optional" disabled={customerBusy}/></label><button type="button" className="button button-primary button-small" onClick={() => void saveCustomer()} disabled={customerBusy}>{customerBusy ? 'Saving…' : 'Save and select'}</button></div>}
          {selectedCustomerId && <section className="register-credit-preview" aria-live="polite" aria-label="Customer credit position">
            <strong>Credit position</strong>
            {creditPositionLoading && <small>Checking current balance…</small>}
            {creditPositionError && <small role="status">{creditPositionError}</small>}
            {creditPosition && !creditPosition.hasCreditTerms && <small>No credit terms are configured for this customer.</small>}
            {creditPosition?.hasCreditTerms && <div className="register-credit-values">
              <span>Limit <strong>{formatMinorUnits(creditPosition.currencyCode, creditPosition.creditLimitMinor)}</strong></span>
              <span>Outstanding <strong>{formatMinorUnits(creditPosition.currencyCode, creditPosition.outstandingMinor)}</strong></span>
              <span>Available <strong>{formatMinorUnits(creditPosition.currencyCode, creditPosition.availableMinor)}</strong></span>
              <small>Due in {creditPosition.dueDays} days · preview only; confirmed again at checkout.</small>
            </div>}
            {canCheckout && creditPosition && creditPosition.outstandingMinor !== '0' && <div className="register-receivable-collection">
              <button type="button" className="button button-quiet button-small" onClick={() => void openReceivableCollection()} disabled={receivableCollectionLoading || busy !== null || !lease || !key || !drawer || drawer.isClosed}>
                {receivableCollectionLoading ? 'Loading invoices…' : receivableCollectionOpen ? 'Refresh open invoices' : 'Receive customer payment'}
              </button>
              {receivableCollectionOpen && <form className="hosted-customer-form hosted-purchase-form" onSubmit={(event) => void submitReceivableCashCollection(event)}>
                <div className="hosted-customer-edit-heading"><div><strong>Cash collection</strong><small>Records cash already received and links it to the open drawer shift.</small></div><button type="button" className="button button-quiet button-small" onClick={() => setReceivableCollectionOpen(false)} disabled={busy !== null}>Close</button></div>
                {openReceivables.length === 0 ? <p className="register-receivable-empty">No invoice balance is available for another allocation. A pending external payment may already be reserved.</p> : openReceivables.map((invoice) => <label className="hosted-bill-receipt" key={`${invoice.invoiceBranchId}:${invoice.invoiceId}`}>
                  <span><strong>Due {new Date(invoice.dueAt).toLocaleDateString()}</strong><small>Issued {new Date(invoice.issuedAt).toLocaleDateString()} · {formatMinorUnits(creditPosition.currencyCode, invoice.openMinor)} remaining</small></span>
                  <span className="hosted-bill-allocation"><span>{creditPosition.currencyCode}</span><input aria-label={`Cash allocation for invoice due ${new Date(invoice.dueAt).toLocaleDateString()}`} inputMode="decimal" placeholder="0.00" value={receivableCollectionAmounts[invoice.invoiceId] ?? ''} onChange={(event) => setReceivableCollectionAmounts((current) => ({ ...current, [invoice.invoiceId]: event.currentTarget.value }))} disabled={busy !== null}/></span>
                </label>)}
                {openReceivables.length > 0 && <>
                  <label>Cash receipt or voucher reference<input maxLength={160} value={receivableCollectionReference} onChange={(event) => setReceivableCollectionReference(event.currentTarget.value)} placeholder="Optional" disabled={busy !== null}/></label>
                  <div className="hosted-purchase-total"><span>Total cash received</span><strong>{formatMoney(creditPosition.currencyCode, Object.values(receivableCollectionAmounts).reduce((sum, value) => sum + (parseMinorAmount(value) ?? 0), 0))}</strong></div>
                  <div className="hosted-purchase-form-actions"><button type="button" className="button button-quiet" onClick={() => setReceivableCollectionOpen(false)} disabled={busy !== null}>Cancel</button><button type="submit" className="button button-primary" disabled={busy !== null || !lease || !key || !Object.values(receivableCollectionAmounts).some((value) => (parseMinorAmount(value) ?? 0) > 0)}>{busy === 'receivable-collection' ? 'Recording…' : 'Record cash payment'}</button></div>
                </>}
              </form>}
            </div>}
          </section>}
          {selectedCustomerId && <label className="register-credit-sale">On account amount<input inputMode="decimal" aria-label="Amount to add to customer credit" placeholder="0.00" value={onAccountAmount} onChange={(event) => setOnAccountAmount(event.currentTarget.value)} disabled={pendingSale || busy === 'checkout'} /><small>{creditPosition?.hasCreditTerms ? `Available ${formatMinorUnits(creditPosition.currencyCode, creditPosition.availableMinor)} � due in ${creditPosition.dueDays} days` : creditPositionLoading ? 'Checking credit terms�' : 'No active credit terms; this must remain 0.00.'}</small></label>}
          <label htmlFor="register-payment-method">Payment method</label>
          <select id="register-payment-method" value={selectedPaymentId} onChange={(event) => setSelectedPaymentId(event.currentTarget.value)} disabled={paymentOptions.length === 0 || pendingSale || busy === 'checkout' || (parseMinorAmount(onAccountAmount) ?? 0) >= totalMinor}><option value="">Choose a configured method</option>{paymentOptions.map((option) => <option key={option.id} value={option.id}>{option.label}{option.recordingMode === 'manual' ? ' · confirm manually' : ''}</option>)}</select>
          {payment?.requiresReference && <label className="register-reference">Payment reference<input value={reference} onChange={(event) => setReference(event.currentTarget.value)} placeholder="Enter terminal, bank or transfer reference" disabled={pendingSale || busy === 'checkout'}/></label>}
          {payment && payment.tenderKind !== 'cash' && <p className="register-manual-note">This records a staff-confirmed payment method. It does not contact or verify the provider.</p>}
        </div>
        {canCheckout ? <button type="button" className="button button-primary register-checkout" onClick={() => void checkout()} disabled={!lease || !key || (!saleAttempt && (cart.length === 0 || (onAccountAmount.trim() !== '' && parseMinorAmount(onAccountAmount) === null) || (totalMinor > (parseMinorAmount(onAccountAmount) ?? 0) && !payment))) || (pendingSale && saleAttempt?.kind === 'legacy' && (cart.length === 0 || !payment)) || busy !== null}>
          {busy === 'checkout' ? 'Completing sale…' : pendingSale && cart.length === 0 ? 'Retry saved sale' : pendingSale ? `Retry sale · ${formatMoney(products[0]?.currencyCode ?? 'GHS', saleAttempt?.kind === 'exact' ? saleAttempt.body.amountMinor : totalMinor)}` : !lease ? 'Switch to cashier mode to sell' : `Complete sale · ${formatMoney(products[0]?.currencyCode ?? 'GHS', totalMinor)}`}
        </button> : <div className="register-readonly"><LockKeyhole size={15}/><span>View and prepare the ticket. Checkout requires an assigned cashier profile.</span></div>}
        {lastSale && <div className="register-receipt"><div><Check size={16}/><span><strong>Sale completed</strong><small>{lastSale.receiptNumber} · served by {lastSale.cashierName}</small></span></div><button type="button" className="button button-quiet button-small" onClick={() => window.print()}><Printer size={14}/> Print</button></div>}
        {lastSale && <div className="register-customer-form receipt-customer-link">
          <strong>{lastSale.customerId ? `Customer linked: ${customers.find((customer) => customer.id === lastSale.customerId)?.name ?? 'Customer'}` : 'Add a customer to this receipt'}</strong>
          {!lastSale.customerId && <>
            <div className="register-customer-row"><input aria-label="Search customers for this receipt" placeholder="Find existing customer" value={receiptCustomerQuery} onChange={(event) => setReceiptCustomerQuery(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); void searchReceiptCustomers() } }} disabled={receiptCustomerBusy}/><button type="button" className="button button-quiet button-small" onClick={() => void searchReceiptCustomers()} disabled={receiptCustomerBusy}>{receiptCustomerBusy ? 'Searching…' : 'Search'}</button></div>
            {receiptCustomerRows.length > 0 && <div className="register-customer-row"><select aria-label="Select customer to attach to receipt" value={receiptCustomerId} onChange={(event) => setReceiptCustomerId(event.target.value)}><option value="">Choose customer</option>{receiptCustomerRows.map((customer) => <option key={customer.id} value={customer.id}>{customer.name}{customer.phone ? ` · ${customer.phone}` : ''}</option>)}</select><button type="button" className="button button-quiet button-small" onClick={() => { const customer = receiptCustomerRows.find((row) => row.id === receiptCustomerId); if (customer) void linkReceiptCustomer(customer) }} disabled={!receiptCustomerId || receiptCustomerBusy}>Link</button></div>}
            <label>Name<input autoComplete="name" maxLength={160} value={receiptCustomerName} onChange={(event) => setReceiptCustomerName(event.target.value)} disabled={receiptCustomerBusy}/></label>
            <label>Phone<input autoComplete="tel" maxLength={40} value={receiptCustomerPhone} onChange={(event) => setReceiptCustomerPhone(event.target.value)} placeholder="Optional" disabled={receiptCustomerBusy}/></label>
            <label>Email<input autoComplete="email" type="email" maxLength={254} value={receiptCustomerEmail} onChange={(event) => setReceiptCustomerEmail(event.target.value)} placeholder="Optional" disabled={receiptCustomerBusy}/></label>
            <button type="button" className="button button-primary button-small" onClick={() => void createAndLinkReceiptCustomer()} disabled={receiptCustomerBusy}>{pendingReceiptCustomer ? 'Retry customer link' : 'Save customer and link'}</button>
          </>}
        </div>}
        {lastSale && <ReceiptShareActions
          businessName={branchName}
          stationName={lastSale.stationName}
          receiptNumber={lastSale.receiptNumber}
          createdAt={lastSale.createdAt}
          cashierName={lastSale.cashierName}
          currencyCode={lastSale.currencyCode}
          subtotalMinor={lastSale.subtotalMinor}
          taxMinor={lastSale.taxMinor}
          totalMinor={lastSale.totalMinor}
          paymentLabel={lastSalePayment}
          lines={lastSaleLines.map((line) => ({ name: line.name, quantity: line.quantity, lineTotalMinor: line.quantity * line.unitPriceMinor }))}
          customerName={lastSaleCustomer?.name ?? null}
          customerPhone={lastSaleCustomer?.phone ?? null}
          customerEmail={lastSaleCustomer?.email ?? null}
        />}
      </aside>
    </div>

    <section className="register-drawer" aria-labelledby="register-drawer-title">
      <header><span className="drawer-icon"><Wallet size={17}/></span><div><p className="eyebrow">CASH CONTROL</p><h3 id="register-drawer-title">{drawer && !drawer.isClosed ? 'Drawer shift open' : 'Drawer shift closed'}</h3></div><button type="button" className="button button-quiet button-small" onClick={() => historyVisible ? setHistoryVisible(false) : void loadDrawerHistory()} disabled={historyBusy}>{historyBusy ? 'Loading…' : historyVisible ? 'Hide history' : 'Shift history'}</button>{drawer && !drawer.isClosed && <button type="button" className="button button-quiet button-small" onClick={() => void loadXReport()}><RefreshCw size={14}/> X report</button>}</header>
      {drawer && !drawer.isClosed ? <>
        <div className="drawer-metrics"><div><span>Opening float</span><strong>{formatMoney(products[0]?.currencyCode ?? 'GHS', drawer.openingCashMinor)}</strong></div><div><span>Cash sales</span><strong>{formatMoney(products[0]?.currencyCode ?? 'GHS', drawer.cashSalesMinor)}</strong></div><div><span>Expected in drawer</span><strong>{formatMoney(products[0]?.currencyCode ?? 'GHS', drawer.expectedCashMinor)}</strong></div></div>
        {canRecordCashExpense && <div className="drawer-supplier-settlement"><div><strong>Cash expense</strong><span>Record a small expense paid from this shift’s drawer. The expense journal and cash outflow are linked.</span></div><button type="button" className="button button-quiet button-small" onClick={() => setCashExpenseOpen((open) => !open)} disabled={!lease || !key || busy !== null}>{cashExpenseOpen ? 'Close expense form' : 'Record cash expense'}</button></div>}
        {cashExpenseOpen && canRecordCashExpense && <form className="hosted-customer-edit hosted-purchase-form drawer-supplier-payment-form" onSubmit={(event) => void submitCashExpense(event)}>
          <div className="hosted-customer-edit-heading"><div><p className="eyebrow">SIGNED DRAWER OUTFLOW</p><h4>Record cash expense</h4><p>This records an expense already paid from the open drawer.</p></div><button type="button" className="button button-quiet button-small" onClick={() => setCashExpenseOpen(false)} disabled={busy !== null}>Close</button></div>
          <div className="hosted-purchase-meta"><label>Description<input required maxLength={240} value={cashExpenseDescription} onChange={(event) => setCashExpenseDescription(event.currentTarget.value)} disabled={busy !== null}/></label><label>Category<select value={cashExpenseCategory} onChange={(event) => setCashExpenseCategory(event.currentTarget.value as HostedExpenseCategory)} disabled={busy !== null}><option value="supplies">Supplies</option><option value="transport">Transport</option><option value="utilities">Utilities</option><option value="maintenance">Maintenance</option><option value="other">Other</option></select></label><label>Paid to<input maxLength={160} value={cashExpensePayee} onChange={(event) => setCashExpensePayee(event.currentTarget.value)} disabled={busy !== null}/></label><label>Amount<input required inputMode="decimal" placeholder="0.00" value={cashExpenseAmount} onChange={(event) => setCashExpenseAmount(event.currentTarget.value)} disabled={busy !== null}/></label><label>Expense date<input required type="date" value={cashExpenseDate} onChange={(event) => setCashExpenseDate(event.currentTarget.value)} disabled={busy !== null}/></label><label>Receipt or voucher reference<input maxLength={160} value={cashExpenseReference} onChange={(event) => setCashExpenseReference(event.currentTarget.value)} disabled={busy !== null}/></label></div>
          <div className="hosted-purchase-form-actions"><button type="button" className="button button-quiet" onClick={() => setCashExpenseOpen(false)} disabled={busy !== null}>Cancel</button><button type="submit" className="button button-primary" disabled={busy !== null || !lease || !key}>{busy === 'cash-expense' ? 'Recording…' : 'Record cash expense'}</button></div>
        </form>}
        {canSettle && <div className="drawer-supplier-settlement"><div><strong>Supplier cash payment</strong><span>Record a till payout with the supplier invoice allocation; the signed payment and drawer outflow post together.</span></div><button type="button" className="button button-quiet button-small" onClick={() => void openSupplierPayment()} disabled={!lease || !key || busy !== null}>{busy === 'supplier-payment' && !supplierPaymentOpen ? 'Loading invoices…' : 'Pay supplier from drawer'}</button></div>}
        {supplierPaymentOpen && supplierBilling && <form className="hosted-customer-edit hosted-purchase-form drawer-supplier-payment-form" onSubmit={(event) => void submitSupplierCashPayment(event)}>
          <div className="hosted-customer-edit-heading"><div><p className="eyebrow">SIGNED DRAWER OUTFLOW</p><h4>Record cash supplier payment</h4><p>The payment is recorded as already made. It will reduce this shift’s expected cash once.</p></div><button type="button" className="button button-quiet button-small" onClick={() => setSupplierPaymentOpen(false)} disabled={busy !== null}>Close</button></div>
          <div className="hosted-purchase-meta"><label>Supplier<select required value={supplierPaymentSupplier} onChange={(event) => { setSupplierPaymentSupplier(event.currentTarget.value); setSupplierPaymentAmounts({}) }} disabled={busy !== null}><option value="">Choose supplier</option>{Array.from(new Map(supplierBilling.bills.filter((bill) => bill.balanceMinor > 0).map((bill) => [bill.supplierId, bill.supplierName])).entries()).map(([id, name]) => <option key={id} value={id}>{name}</option>)}</select></label><label>Cash payment reference<input maxLength={100} value={supplierPaymentReference} onChange={(event) => setSupplierPaymentReference(event.currentTarget.value)} placeholder="Optional receipt or voucher" disabled={busy !== null}/></label></div>
          {supplierBilling.bills.filter((bill) => bill.supplierId === supplierPaymentSupplier && bill.balanceMinor > 0).map((bill) => <label className="hosted-bill-receipt" key={bill.id}><span><strong>{bill.supplierReference}</strong><small>Invoice {bill.invoiceDate} · {formatMoney(bill.currencyCode, bill.balanceMinor)} remaining</small></span><span className="hosted-bill-allocation"><span>{bill.currencyCode}</span><input aria-label={`Cash allocation for invoice ${bill.supplierReference}`} inputMode="decimal" placeholder="0.00" value={supplierPaymentAmounts[bill.id] ?? ''} onChange={(event) => setSupplierPaymentAmounts((current) => ({ ...current, [bill.id]: event.currentTarget.value }))} disabled={busy !== null}/></span></label>)}
          <div className="hosted-purchase-total"><span>Total paid from drawer</span><strong>{formatMoney(supplierBilling.currencyCode, Object.values(supplierPaymentAmounts).reduce((sum, value) => sum + (parseMinorAmount(value) ?? 0), 0))}</strong></div>
          <div className="hosted-purchase-form-actions"><button type="button" className="button button-quiet" onClick={() => setSupplierPaymentOpen(false)} disabled={busy !== null}>Cancel</button><button type="submit" className="button button-primary" disabled={busy !== null || !supplierPaymentSupplier || !Object.values(supplierPaymentAmounts).some((value) => (parseMinorAmount(value) ?? 0) > 0)}>{busy === 'supplier-payment' ? 'Recording…' : 'Record cash payment'}</button></div>
        </form>}
        {canSettle && !lease && <p className="drawer-not-open">To pay from this drawer, switch to an assigned cashier profile that also has supplier settlement permission.</p>}
        {lease && <div className="drawer-actions"><label>Movement<select value={movementKind} onChange={(event) => setMovementKind(parseMovementKind(event.currentTarget.value))}><option value="paid_out">Paid out</option><option value="paid_in">Paid in</option><option value="cash_drop">Cash drop</option></select></label>{movementKind === 'cash_drop' && <label>Drop destination<select required value={cashDropDestinationId} onChange={(event) => setCashDropDestinationId(event.currentTarget.value)} disabled={!cashDropDestinations || cashDropDestinations.length === 0}><option value="">Choose an initialized safe</option>{cashDropDestinations?.map((destination) => <option key={destination.id} value={destination.id}>{destination.name}</option>)}</select>{cashDropDestinationError && <small role="status">{cashDropDestinationError}</small>}{cashDropDestinations?.length === 0 && !cashDropDestinationError && <small>Ask an admin to add and initialize a safe or cash location first.</small>}</label>}<label>Amount<input inputMode="decimal" value={movementAmount} onChange={(event) => setMovementAmount(event.currentTarget.value)} placeholder="0.00"/></label><label>Reason<input value={movementNote} onChange={(event) => setMovementNote(event.currentTarget.value)} placeholder="Required note"/></label><button type="button" className="button button-quiet" onClick={() => void postMovement()} disabled={busy !== null || (movementKind === 'cash_drop' && !cashDropDestinationId)}>Record movement</button><label>Counted cash<input inputMode="decimal" value={countedCash} onChange={(event) => setCountedCash(event.currentTarget.value)} placeholder="0.00"/></label><button type="button" className="button button-primary" onClick={() => void closeDrawer()} disabled={busy !== null}>Close shift</button></div>}
        {xReport && <div className="register-x-report" role="status"><strong>Current X report</strong><span>{xReport.saleCount} cash sales · {formatMoney(products[0]?.currencyCode ?? 'GHS', xReport.expectedCashMinor)} expected cash</span></div>}
      </> : canCheckout && lease ? <div className="drawer-opening"><label>Opening cash float<input inputMode="decimal" value={openingCash} onChange={(event) => setOpeningCash(event.currentTarget.value)} placeholder="0.00"/></label><button type="button" className="button button-primary" onClick={() => void openDrawer()} disabled={busy !== null}>{busy === 'drawer-open' ? 'Opening…' : 'Open cash drawer'}</button><span>Cash sales remain unavailable until this station has an open drawer shift.</span></div>
        : <p className="drawer-not-open">Cash drawer is closed. A cashier can open it after switching into cashier mode.</p>}
      {historyVisible && <div className="drawer-history" aria-label="Closed drawer shift history">
        <h4>Last 50 closed shifts</h4>
        {drawerHistory.length === 0 ? <p>No closed shifts are available for this station yet.</p> : drawerHistory.map((shift) => <article className="drawer-history-row" key={shift.shiftId}>
          <div><strong>{new Date(shift.openedAt).toLocaleString()}</strong><small>Closed {shift.closedAt ? new Date(shift.closedAt).toLocaleString() : '—'} · {shift.saleCount} cash sales · {shift.movementCount} movements</small></div>
          <div><strong>{formatMoney(products[0]?.currencyCode ?? 'GHS', shift.countedCashMinor ?? 0)}</strong><small>Counted · variance {formatMoney(products[0]?.currencyCode ?? 'GHS', shift.differenceMinor ?? 0)}</small></div>
        </article>)}
      </div>}
    </section>

    {lastSale && <section className="register-print-receipt" aria-label="Printable receipt">
      <h1>{branchName}</h1><p>{lastSale.stationName}</p><hr/>
      <p>Receipt: {lastSale.receiptNumber}</p><p>{new Date(lastSale.createdAt).toLocaleString()}</p>
      <hr/><div className="print-receipt-lines">{lastSaleLines.map((line, index) => <div key={`${line.name}-${index}`}><span>{line.quantity} × {line.name}</span><strong>{formatMoney(lastSale.currencyCode, line.quantity * line.unitPriceMinor)}</strong></div>)}</div>
      <hr/><div><span>Subtotal</span><strong>{formatMoney(lastSale.currencyCode, lastSale.subtotalMinor)}</strong></div><div><span>Tax</span><strong>{formatMoney(lastSale.currencyCode, lastSale.taxMinor)}</strong></div><div className="print-receipt-total"><span>Total</span><strong>{formatMoney(lastSale.currencyCode, lastSale.totalMinor)}</strong></div>
      <hr/>{lastSale.customerId && <p>Customer: {customers.find((customer) => customer.id === lastSale.customerId)?.name ?? 'Customer'}</p>}<p>Paid by: {lastSalePayment}</p><p>Served by: {lastSale.cashierName}</p><p>Thank you for shopping with us.</p>
    </section>}

    {salesHistoryVisible && <div className="receipt-detail-backdrop">
      <section className="receipt-detail-card sales-history-card" role="dialog" aria-modal="true" aria-labelledby="station-sales-history-title">
        <header className="receipt-detail-actions"><div><p className="eyebrow">{stationName}</p><h3 id="station-sales-history-title">Sales history</h3></div><button type="button" className="button button-quiet button-small" onClick={() => setSalesHistoryVisible(false)} aria-label="Close sales history"><X size={15}/> Close</button></header>
        <form className="sales-report-filters station-sales-history-search" onSubmit={(event) => void searchSalesHistory(event)}>
          <label>Find a receipt, customer, item, barcode or payment<input autoFocus maxLength={120} type="search" value={salesHistorySearch} onChange={(event) => setSalesHistorySearch(event.currentTarget.value)} placeholder="Receipt number, customer, product…"/></label>
          <button type="submit" className="button button-primary" disabled={salesHistoryBusy}><Search size={14}/>{salesHistoryBusy ? 'Searching…' : 'Search'}</button>
        </form>
        {salesHistoryError && <div className="notice notice-danger" role="alert">{salesHistoryError}<button type="button" className="button button-quiet button-small" onClick={() => void loadSalesHistory()}>Retry</button></div>}
        {salesHistoryBusy && <p className="sales-report-empty" role="status">Loading sales history…</p>}
        {salesHistory && !salesHistoryBusy && <div className="station-sales-history-list">
          {salesHistory.rows.length === 0 ? <p className="sales-report-empty">No completed sales match this search.</p> : salesHistory.rows.map((sale) => <article className="station-sales-history-row" key={sale.id}>
            <div><strong>{sale.receiptNumber}</strong><small>{new Date(sale.createdAt).toLocaleString()} · {sale.cashierName}</small>{sale.customerName && <small>Customer: {sale.customerName}</small>}</div>
            <strong>{formatMoney(sale.currencyCode, sale.totalMinor)}</strong>
            <div className="station-sales-history-actions"><button type="button" className="button button-quiet button-small" onClick={() => void openHistoricalReceipt(sale.id)} disabled={historicalReceiptBusyId !== null}>{historicalReceiptBusyId === sale.id ? 'Loading…' : 'View / print'}</button>{canRefund && <button type="button" className="button button-quiet button-small" onClick={() => void openSaleRefund(sale.id)} disabled={refundBusy || !lease || !key}>{refundBusy ? 'Loading…' : 'Refund / void'}</button>}</div>
          </article>)}
        </div>}
        {salesHistory && <footer className="sales-report-pagination"><span>{salesHistory.matchedCount === 0 ? '0 sales' : `${salesHistory.offset + 1}–${Math.min(salesHistory.offset + salesHistory.rows.length, salesHistory.matchedCount)} of ${salesHistory.matchedCount}`}</span><div><button type="button" className="button button-quiet" onClick={() => void loadSalesHistory(salesHistoryAppliedSearch, Math.max(0, salesHistory.offset - salesHistory.pageSize))} disabled={salesHistoryBusy || salesHistory.offset === 0}>Previous</button><button type="button" className="button button-quiet" onClick={() => void loadSalesHistory(salesHistoryAppliedSearch, salesHistory.offset + salesHistory.pageSize)} disabled={salesHistoryBusy || salesHistory.offset + salesHistory.rows.length >= salesHistory.matchedCount || salesHistory.offset >= 50_000}>Next</button></div></footer>}
        <p className="sales-report-disclosure">Cashiers can search only receipts visible under their assigned station and staff profile. Payment entries are recorded tender snapshots and do not confirm external settlement.</p>
      </section>
    </div>}

    {historicalReceipt && <div className="receipt-detail-backdrop">
      <section className="receipt-detail-card" role="dialog" aria-modal="true" aria-labelledby="historical-receipt-title">
        <header className="receipt-detail-actions"><div><p className="eyebrow">COMPLETED SALE</p><h3 id="historical-receipt-title">{historicalReceipt.receiptNumber}</h3></div><button type="button" className="button button-quiet button-small" onClick={() => setHistoricalReceipt(null)} aria-label="Close receipt"><X size={15}/> Close</button></header>
        <div className="receipt-detail-printable">
          <h2>{branchName}</h2><p>{historicalReceipt.stationName} · {new Date(historicalReceipt.createdAt).toLocaleString()}</p>
          <p>Receipt: {historicalReceipt.receiptNumber}</p><p>Served by: {historicalReceipt.cashierName}</p>
          {historicalReceipt.customerName && <p>Customer: {historicalReceipt.customerName}</p>}
          <div className="receipt-detail-lines">{historicalReceipt.lines.map((line) => <div key={line.lineNumber}><span>{line.quantity} × {line.name}<small>{line.sku} · {formatMoney(historicalReceipt.currencyCode, line.unitPriceMinor)} each</small></span><strong>{formatMoney(historicalReceipt.currencyCode, line.totalMinor)}</strong></div>)}</div>
          <div className="receipt-detail-totals"><div><span>Subtotal</span><strong>{formatMoney(historicalReceipt.currencyCode, historicalReceipt.subtotalMinor)}</strong></div><div><span>Tax</span><strong>{formatMoney(historicalReceipt.currencyCode, historicalReceipt.taxMinor)}</strong></div><div className="receipt-detail-grand-total"><span>Total</span><strong>{formatMoney(historicalReceipt.currencyCode, historicalReceipt.totalMinor)}</strong></div></div>
          <div className="receipt-detail-tenders"><span>Payment</span>{historicalReceipt.tenders.map((tender, index) => <div key={`${tender.label}-${index}`}><span>{tender.label}</span><strong>{formatMoney(historicalReceipt.currencyCode, tender.amountMinor)}</strong></div>)}</div>
          <p className="receipt-detail-thanks">Thank you for shopping with us.</p>
        </div>
        <footer className="receipt-detail-actions"><span>Receipt values are read from the recorded sale.</span><button type="button" className="button button-primary" onClick={() => window.print()}><Printer size={15}/> Print receipt</button></footer>
      </section>
    </div>}

    {refundDetail && <div className="receipt-detail-backdrop">
      <section className="receipt-detail-card sales-history-card" role="dialog" aria-modal="true" aria-labelledby="sale-refund-title">
        <header className="receipt-detail-actions"><div><p className="eyebrow">{refundDetail.receiptNumber}</p><h3 id="sale-refund-title">Refund or void sale</h3></div><button type="button" className="button button-quiet button-small" onClick={() => setRefundDetail(null)} disabled={refundBusy}><X size={15}/> Close</button></header>
        {refundDetail.onAccountMinor > 0
          ? <p className="notice notice-warning" role="status">This sale includes customer credit. Refunds are unavailable until the credit-note workflow is implemented, so its receivable cannot be silently left outstanding.</p>
          : <p className="sales-report-disclosure">This records a manual refund and inventory disposition. Non-cash payouts must be completed and confirmed with the provider separately.</p>}
        <label>Action<select value={refundKind} onChange={(event) => setRefundKind(event.currentTarget.value === 'void' ? 'void' : 'refund')} disabled={refundBusy}><option value="refund">Return selected quantities</option><option value="void">Void entire sale (first full return only)</option></select></label>
        <div className="station-sales-history-list">{refundDetail.items.map((item) => <div className="station-sales-history-row" key={item.saleLineId}>
          <span><strong>{item.name}</strong><small>{item.sku} · {item.refundedQuantity} returned · {item.remainingQuantity} remaining</small></span>
          <label>Quantity<input aria-label={`Return quantity for ${item.name}`} type="number" min="0" max={item.remainingQuantity} step="1" value={refundQuantities[item.saleLineId] ?? '0'} onChange={(event) => setRefundQuantities((current) => ({ ...current, [item.saleLineId]: event.currentTarget.value }))} disabled={refundBusy || item.remainingQuantity === 0}/></label>
          <strong>{formatMoney(refundDetail.currencyCode, item.refundableTotalMinor)}</strong>
        </div>)}</div>
        <label>Reason<input maxLength={250} value={refundReason} onChange={(event) => setRefundReason(event.currentTarget.value)} placeholder="Reason for return" disabled={refundBusy}/></label>
        <label>Refund method<select value={refundMethod} onChange={(event) => setRefundMethod(parseRefundMethod(event.currentTarget.value))} disabled={refundBusy}><option value="cash">Cash from open drawer</option><option value="card">Card (manual payout)</option><option value="mobile_money">Mobile money (manual payout)</option><option value="bank_transfer">Bank transfer (manual payout)</option><option value="other">Other (manual payout)</option></select></label>
        {refundMethod !== 'cash' && <label>Provider reference<input required maxLength={160} value={refundReference} onChange={(event) => setRefundReference(event.currentTarget.value)} placeholder="Transaction or provider reference" disabled={refundBusy}/></label>}
        <label className="register-check-label"><input type="checkbox" checked={refundRestock} onChange={(event) => setRefundRestock(event.currentTarget.checked)} disabled={refundBusy}/> Restock returned goods as sellable</label>
        <footer className="receipt-detail-actions"><span>Current status: {refundDetail.status.replaceAll('_', ' ')}</span><button type="button" className="button button-primary" onClick={() => void submitSaleRefund()} disabled={refundBusy || refundReason.trim().length < 3 || refundDetail.onAccountMinor > 0}>{refundBusy ? 'Recording…' : 'Record refund'}</button></footer>
      </section>
    </div>}

    <footer className="register-disclosure"><ShieldCheck size={14}/><span>Sale prices, tax, stock, tender and cashier authorization are verified by the hosted API. Receipt printing uses this browser’s print dialog.</span></footer>
    <dialog ref={pickupQueueDialogRef} className="pickup-queue-dialog" aria-labelledby="pickup-queue-title" onClose={() => setPickupQueueOpen(false)}>
      {pickupQueueOpen && <HostedPickupQueuePanel apiBaseUrl={apiBaseUrl} accessToken={accessToken} tenantId={tenantId} branchId={branchId} branchName={branchName} scope={{ kind: 'station', stationId }} membershipId={subject} canPrepare={canPreparePickups} canCancel={canRequestPickups} canManage={false} onSellHeld={canCheckout ? sellHeldReservation : undefined} onClose={() => { pickupQueueDialogRef.current?.close(); setPickupQueueOpen(false) }} />}
    </dialog>
  </section>
}
