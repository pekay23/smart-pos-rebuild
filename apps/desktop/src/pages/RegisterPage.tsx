// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AlertTriangle,
  Barcode,
  Check,
  ChevronDown,
  CircleMinus,
  CirclePlus,
  Clock3,
  Copy,
  CreditCard,
  Mail,
  MessageCircle,
  Pause,
  Printer,
  Search,
  ShoppingBasket,
  Trash2,
  UserPlus,
  UserRound,
  X,
} from "lucide-react";
import { posApi } from "../lib/api";
import { EntityImageThumbnail } from "../components/EntityImageThumbnail";
import { CashDrawerControl } from "../components/CashDrawerControl";
import type {
  Customer,
  DiscountApprover,
  DiscountPreview,
  Product,
  RegisterAction,
  RegisterTicket,
  RegisterWorkspace,
  SaleReceipt,
  SessionUser,
  StorePreferences,
} from "../lib/models";
import { formatMoney } from "../lib/money";
import { publishCustomerDisplayCart } from "../lib/customerDisplay";
import { paymentApi, defaultPaymentOptions, type PaymentOption } from "../lib/paymentApi";
import { useAttemptCountdown } from "../lib/useAttemptCountdown";

type CustomerForm = { name: string; phone: string; email: string; notes: string };
type CustomerModalMode = "active-ticket" | "completed-sale";
type CartLineView = {
  ticketLine: RegisterTicket["lines"][number];
  product: Product | null;
};
type RegisterController = { prepareForStaffSwitch: () => Promise<void> };

const emptyCustomerForm: CustomerForm = { name: "", phone: "", email: "", notes: "" };
const defaultReceiptPreferences: Pick<StorePreferences, "receiptHeaderText" | "receiptFooterText" | "receiptPaperWidthMm"> = {
  receiptHeaderText: "",
  receiptFooterText: "Thank you for shopping with us.",
  receiptPaperWidthMm: 80,
};

export function RegisterPage({
  user,
  canTransact,
  onComplete,
  onControllerReady,
}: {
  user: SessionUser;
  canTransact: boolean;
  onComplete: () => void;
  onControllerReady: (controller: RegisterController | null) => void;
}) {
  const attemptCountdown = useAttemptCountdown();
  const [products, setProducts] = useState<Product[]>([]);
  const [paymentOptions, setPaymentOptions] = useState<PaymentOption[]>([]);
  const [customers, setCustomers] = useState<Customer[]>([]);
  const [workspace, setWorkspace] = useState<RegisterWorkspace | null>(null);
  const [query, setQuery] = useState("");
  const [categoryFilter, setCategoryFilter] = useState("");
  const [selectedLines, setSelectedLines] = useState<Set<string>>(() => new Set());
  const [discountInput, setDiscountInput] = useState("");
  const [discountReasonInput, setDiscountReasonInput] = useState("");
  const [discountApprovalOpen, setDiscountApprovalOpen] = useState(false);
  const [discountApprovers, setDiscountApprovers] = useState<DiscountApprover[]>([]);
  const [discountPreview, setDiscountPreview] = useState<DiscountPreview | null>(null);
  const [selectedApproverId, setSelectedApproverId] = useState("");
  const [approverCredential, setApproverCredential] = useState("");
  const [approvingDiscount, setApprovingDiscount] = useState(false);
  const [discountCheckTime, setDiscountCheckTime] = useState(() => Date.now());
  const [tenderInput, setTenderInput] = useState("");
  const [error, setError] = useState("");
  const [receipt, setReceipt] = useState<SaleReceipt | null>(null);
  const [receiptCustomer, setReceiptCustomer] = useState<Customer | null>(null);
  const [shareNotice, setShareNotice] = useState("");
  const [receiptPreferences, setReceiptPreferences] = useState(defaultReceiptPreferences);
  const [busy, setBusy] = useState(false);
  const [savingActions, setSavingActions] = useState(0);
  const [loading, setLoading] = useState(true);
  const [ticketsOpen, setTicketsOpen] = useState(false);
  const [customerModal, setCustomerModal] = useState<CustomerModalMode | null>(null);
  const [customerForm, setCustomerForm] = useState<CustomerForm>(emptyCustomerForm);
  const [savedCustomer, setSavedCustomer] = useState<Customer | null>(null);
  const [selectedExistingCustomerId, setSelectedExistingCustomerId] = useState("");
  const [customerError, setCustomerError] = useState("");
  const [customerBusy, setCustomerBusy] = useState(false);
  const [mobilePanel, setMobilePanel] = useState<"catalog" | "ticket">("catalog");
  const [hideLastCompleted, setHideLastCompleted] = useState(false);
  const workspaceRef = useRef<RegisterWorkspace | null>(null);
  const actionQueue = useRef<Promise<void>>(Promise.resolve());
  const fieldTimers = useRef<{ discount: number | null; tender: number | null }>({ discount: null, tender: null });
  const pendingFields = useRef<{ discount: boolean; tender: boolean }>({ discount: false, tender: false });
  const discountInputRef = useRef(discountInput);
  const discountReasonInputRef = useRef(discountReasonInput);
  const tenderInputRef = useRef(tenderInput);

  const setCurrentWorkspace = useCallback((next: RegisterWorkspace) => {
    workspaceRef.current = next;
    setWorkspace(next);
  }, []);

  const reloadCatalog = useCallback(async () => {
    const [productRows, customerRows] = await Promise.all([posApi.products(), posApi.customers()]);
    setProducts(productRows);
    setCustomers(customerRows);
  }, []);

  useEffect(() => {
    let active = true;
    void Promise.all([posApi.products(), posApi.customers(), posApi.registerWorkspace(), paymentApi.options(), posApi.registerPreferences()])
      .then(([productRows, customerRows, savedWorkspace, options, preferences]) => {
        if (!active) return;
        setProducts(productRows);
        setPaymentOptions(options.length ? options.filter((option) => option.enabled) : defaultPaymentOptions);
        setCustomers(customerRows);
        setCurrentWorkspace(savedWorkspace);
        setReceiptPreferences(preferences);
      })
      .catch((reason) => {
        if (active) setError(typeof reason === "string" ? reason : "The register could not be opened.");
      })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [setCurrentWorkspace]);

  const activeTicket = workspace?.active ?? null;
  const activeTicketId = activeTicket?.id;
  const activeDiscountMinor = activeTicket?.discountMinor;
  const activeDiscountReason = activeTicket?.discountReason;
  const activeTenderedMinor = activeTicket?.amountTenderedMinor;
  const activeApprovalId = activeTicket?.discountApprovalId;
  const activeApprovalExpiresAt = activeTicket?.discountApprovalExpiresAt;
  // This effect mirrors the persisted ticket into the cashier's editable fields.
  useEffect(() => {
    if (!activeTicketId || activeDiscountMinor === undefined || activeTenderedMinor === undefined) return;
    setDiscountInput(minorToInput(activeDiscountMinor));
    discountInputRef.current = minorToInput(activeDiscountMinor);
    setDiscountReasonInput(activeDiscountReason ?? "");
    discountReasonInputRef.current = activeDiscountReason ?? "";
    setTenderInput(minorToInput(activeTenderedMinor));
    tenderInputRef.current = minorToInput(activeTenderedMinor);
    setSelectedLines(new Set());
  }, [activeTicketId, activeDiscountMinor, activeDiscountReason, activeTenderedMinor]);

  useEffect(() => {
    if (!activeApprovalId || !activeApprovalExpiresAt) return;
    const expiresAt = Date.parse(activeApprovalExpiresAt);
    if (!Number.isFinite(expiresAt)) return;
    const timer = window.setTimeout(() => setDiscountCheckTime(Date.now()), Math.max(0, expiresAt - Date.now()) + 25);
    return () => window.clearTimeout(timer);
  }, [activeApprovalId, activeApprovalExpiresAt]);

  const categoryNames = useMemo(
    () => [...new Set(products.map((product) => product.categoryName).filter((name): name is string => Boolean(name)))].sort(),
    [products],
  );
  const filtered = useMemo(() => products.filter((product) =>
    product.isActive
    && (!categoryFilter || product.categoryName === categoryFilter)
    && (!query || `${product.name} ${product.sku} ${product.barcode ?? ""} ${product.categoryName ?? ""} ${product.brandName ?? ""} ${product.tags.join(" ")}`.toLowerCase().includes(query.toLowerCase())),
  ), [products, query, categoryFilter]);
  const cart = useMemo<CartLineView[]>(() => (activeTicket?.lines ?? []).map((ticketLine) => ({
    ticketLine,
    product: products.find((product) => product.id === ticketLine.productId) ?? null,
  })), [activeTicket?.lines, products]);
  const subtotal = cart.reduce((sum, line) => sum + line.ticketLine.acceptedUnitPriceMinor * line.ticketLine.quantity, 0);
  const discountMinor = activeTicket?.discountMinor ?? 0;
  const discountAuthorized = discountMinor > 0 && Boolean(activeTicket && discountApprovalIsCurrent(activeTicket, discountCheckTime));
  const discountNeedsApproval = discountMinor > 0 && !discountAuthorized;
  const total = Math.max(0, subtotal - (discountAuthorized ? discountMinor : 0));
  const discountInputChanged = parseMinor(discountInput) !== discountMinor || discountReasonInput.trim() !== (activeTicket?.discountReason ?? "");
  const tenderedMinor = activeTicket?.amountTenderedMinor ?? 0;
  const itemCount = cart.reduce((sum, line) => sum + line.ticketLine.quantity, 0);
  const priceChangedLines = cart.filter((line) => line.product && line.product.priceMinor !== line.ticketLine.acceptedUnitPriceMinor);
  const unavailableLines = cart.filter((line) => !line.product || !line.product.isActive);
  const shortStockLines = cart.filter((line) => line.product && line.product.stockQuantity < line.ticketLine.quantity);
  const heldCount = workspace?.held.length ?? 0;

  useEffect(() => {
    if (!activeTicketId) return;
    const method = paymentOptions.find((option) => option.id === activeTicket?.paymentOptionId && option.kind === activeTicket?.paymentMethod);
    void publishCustomerDisplayCart({
      paymentInstructions: method ? { name: method.name, provider: method.provider, account: method.merchantAccount, instructions: method.instructions } : null,
      businessName: user.businessName,
      storeName: user.storeName,
      currencyCode: user.currencyCode,
      lines: cart.map(({ ticketLine }) => ({
        name: ticketLine.acceptedName,
        quantity: ticketLine.quantity,
        unitPriceMinor: ticketLine.acceptedUnitPriceMinor,
        lineTotalMinor: ticketLine.acceptedUnitPriceMinor * ticketLine.quantity,
      })),
      subtotalMinor: subtotal,
      discountMinor: discountAuthorized ? discountMinor : 0,
      discountPendingApproval: discountMinor > 0 && !discountAuthorized,
      totalMinor: total,
      status: receipt && cart.length === 0 ? "paid" : cart.length ? "basket" : "idle",
      receiptNumber: receipt?.receiptNumber ?? null,
    }).catch(() => {});
  }, [user.businessName, user.storeName, user.currencyCode, activeTicketId, activeTicket?.paymentOptionId, activeTicket?.paymentMethod, activeApprovalId, activeApprovalExpiresAt, paymentOptions, cart, subtotal, discountMinor, discountAuthorized, total, discountCheckTime, receipt]);

  const dispatchAction = useCallback((action: RegisterAction) => {
    const operation = actionQueue.current.then(async () => {
      const current = workspaceRef.current ?? await posApi.registerWorkspace();
      if (!workspaceRef.current) setCurrentWorkspace(current);
      setSavingActions((count) => count + 1);
      try {
        const next = await posApi.editRegisterWorkspace({ expectedRevision: current.revision, action });
        setCurrentWorkspace(next);
        setError("");
      } catch (reason) {
        const message = typeof reason === "string" ? reason : "The register update could not be saved.";
        if (message.includes("changed in another window")) {
          try { setCurrentWorkspace(await posApi.registerWorkspace()); } catch { /* keep the current ticket visible for recovery */ }
        }
        setError(message);
        throw reason;
      } finally {
        setSavingActions((count) => Math.max(0, count - 1));
      }
    });
    actionQueue.current = operation.then(() => undefined, () => undefined);
    return operation;
  }, [setCurrentWorkspace]);

  const commitDiscount = useCallback(async () => {
    if (fieldTimers.current.discount !== null) window.clearTimeout(fieldTimers.current.discount);
    fieldTimers.current.discount = null;
    if (!pendingFields.current.discount) return;
    const amount = parseMinor(discountInputRef.current);
    const reason = discountReasonInputRef.current.trim();
    if (amount > 0 && (reason.length < 3 || reason.length > 200)) {
      setError("Add a short reason between 3 and 200 characters for this discount.");
      throw new Error("A discount reason is required.");
    }
    pendingFields.current.discount = false;
    await dispatchAction({ type: "setDiscount", discountMinor: amount, reason: amount > 0 ? reason : null });
  }, [dispatchAction]);

  const commitTender = useCallback(async () => {
    if (fieldTimers.current.tender !== null) window.clearTimeout(fieldTimers.current.tender);
    fieldTimers.current.tender = null;
    if (!pendingFields.current.tender) return;
    pendingFields.current.tender = false;
    const current = workspaceRef.current?.active;
    if (!current) return;
    await dispatchAction({
      type: "setPayment",
      paymentMethod: current.paymentMethod,
      paymentOptionId: current.paymentOptionId,
      paymentReference: current.paymentReference,
      amountTenderedMinor: parseMinor(tenderInputRef.current),
    });
  }, [dispatchAction]);

  const flushPendingFields = useCallback(async () => {
    await Promise.all([commitDiscount(), commitTender()]);
    await actionQueue.current;
  }, [commitDiscount, commitTender]);

  const prepareForStaffSwitch = useCallback(async () => {
    await flushPendingFields();
    const current = workspaceRef.current?.active;
    if (current && ticketHasContent(current)) {
      await dispatchAction({ type: "holdActive" });
    }
  }, [dispatchAction, flushPendingFields]);

  useEffect(() => {
    onControllerReady({ prepareForStaffSwitch });
    return () => onControllerReady(null);
  }, [onControllerReady, prepareForStaffSwitch]);

  async function addProduct(product: Product) {
    if (product.stockQuantity < 1) return;
    setError("");
    try { await dispatchAction({ type: "addProduct", productId: product.id }); }
    catch { /* the register error remains visible */ }
    setMobilePanel("ticket");
  }

  function scanOrResolve(code: string) {
    const normalized = code.trim().toLocaleLowerCase();
    const exactMatches = products.filter((product) => product.isActive
      && [product.barcode, product.sku].some((value) => value?.trim().toLocaleLowerCase() === normalized));
    if (exactMatches.length === 1) {
      void addProduct(exactMatches[0]);
      setQuery("");
    } else if (exactMatches.length > 1) {
      setError(`More than one product matches “${code.trim()}”. Choose the correct product from the results.`);
    } else {
      setError(`No active product matches “${code.trim()}”.`);
    }
  }

  function scheduleDiscount(value: string) {
    setDiscountInput(value);
    discountInputRef.current = value;
    pendingFields.current.discount = true;
  }

  function scheduleDiscountReason(value: string) {
    setDiscountReasonInput(value);
    discountReasonInputRef.current = value;
    pendingFields.current.discount = true;
  }

  async function requestDiscount() {
    setError("");
    try {
      await commitDiscount();
      if (parseMinor(discountInputRef.current) > 0) await openDiscountApproval();
    } catch { /* the validation or save error remains visible */ }
  }

  async function openDiscountApproval() {
    try {
      const current=workspaceRef.current;
      if(!current)throw new Error("The saved ticket could not be loaded.");
      const [approvers,preview] = await Promise.all([
        posApi.discountApprovers(),
        posApi.ticketDiscountPreview({ticketId:current.active.id,expectedWorkspaceRevision:current.revision}),
      ]);
      setDiscountApprovers(approvers);
      setDiscountPreview(preview);
      setSelectedApproverId(approvers.find((approver) => approver.userId === user.userId)?.userId ?? approvers[0]?.userId ?? "");
      setApproverCredential("");
      setDiscountApprovalOpen(true);
    } catch (reason) {
      setError(typeof reason === "string" ? reason : "Discount approvers could not be loaded.");
    }
  }

  async function approveDiscount(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const current = workspaceRef.current;
    if (!current || !discountPreview || !current.active.discountMinor || !selectedApproverId) return;
    setApprovingDiscount(true);
    setError("");
    try {
      const next = await posApi.approveTicketDiscount({ticketId:discountPreview.ticketId,expectedWorkspaceRevision:discountPreview.workspaceRevision,expectedDiscountMinor:discountPreview.discountMinor,approverUserId:selectedApproverId,credential:approverCredential});
      setCurrentWorkspace(next);
      setDiscountApprovalOpen(false);
      setApproverCredential("");
    } catch (reason) {
      const message = typeof reason === "string" ? reason : "The discount could not be approved.";
      attemptCountdown.observe(message);
      setError(message);
    } finally { setApprovingDiscount(false); }
  }

  function scheduleTender(value: string) {
    setTenderInput(value);
    tenderInputRef.current = value;
    pendingFields.current.tender = true;
    if (fieldTimers.current.tender !== null) window.clearTimeout(fieldTimers.current.tender);
    fieldTimers.current.tender = window.setTimeout(() => {
      fieldTimers.current.tender = null;
      void commitTender().catch(() => {});
    }, 180);
  }

  async function holdCurrentTicket() {
    setError("");
    try {
      await flushPendingFields();
      await dispatchAction({ type: "holdActive" });
      setSelectedLines(new Set());
      setMobilePanel("catalog");
    } catch { /* keep the active basket when hold fails */ }
  }

  async function clearTicket() {
    setError("");
    try {
      await flushPendingFields();
      await dispatchAction({ type: "clearActive" });
      setSelectedLines(new Set());
    } catch { /* the active ticket stays available */ }
  }

  async function resumeTicket(ticketId: string) {
    setError("");
    try {
      await flushPendingFields();
      const current = workspaceRef.current?.active;
      await dispatchAction({ type: "resumeHeld", ticketId, holdActive: Boolean(current && ticketHasContent(current)) });
      await reloadCatalog();
      setTicketsOpen(false);
      setMobilePanel("ticket");
    } catch { /* both tickets remain saved if the swap fails */ }
  }

  async function discardHeldTicket(ticketId: string) {
    try { await dispatchAction({ type: "discardHeld", ticketId }); }
    catch { /* the list will show the ticket again if removal fails */ }
  }

  async function completeSale() {
    if (!canTransact || !activeTicket || !cart.length || busy) return;
    setBusy(true);
    setError("");
    try {
      await flushPendingFields();
      await actionQueue.current;
      const currentWorkspace = workspaceRef.current;
      const currentTicket = currentWorkspace?.active;
      if (!currentWorkspace || !currentTicket) throw new Error("The register ticket could not be reloaded.");
      if (currentTicket.discountMinor > 0 && !discountApprovalIsCurrent(currentTicket)) {
        setError("This discount needs current manager approval before checkout.");
        await openDiscountApproval();
        return;
      }
      const currentCustomer = customers.find((customer) => customer.id === currentTicket.customerId) ?? null;
      const result = await posApi.checkout({
        ticketId: currentTicket.id,
        workspaceRevision: currentWorkspace.revision,
        customerId: currentTicket.customerId,
        lines: currentTicket.lines.map((line) => ({ productId: line.productId, quantity: line.quantity })),
        discountMinor: currentTicket.discountMinor,
        paymentMethod: currentTicket.paymentMethod,
        paymentOptionId: currentTicket.paymentOptionId,
        paymentReference: currentTicket.paymentReference,
        amountTenderedMinor: currentTicket.amountTenderedMinor,
      });
      setReceiptCustomer(currentCustomer);
      setReceipt(result);
      setShareNotice("");
      setHideLastCompleted(true);
      const nextWorkspace = await posApi.registerWorkspace();
      setCurrentWorkspace(nextWorkspace);
      setSelectedLines(new Set());
      onComplete();
      await reloadCatalog();
    } catch (reason) {
      setError(typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "Sale could not be completed. Your ticket is still saved.");
      const current = workspaceRef.current?.active;
      if (current?.discountMinor && !discountApprovalIsCurrent(current)) void openDiscountApproval();
      try { setProducts(await posApi.products()); } catch { /* keep the current catalog on screen */ }
    } finally {
      setBusy(false);
    }
  }

  function openCustomerForm(mode: CustomerModalMode) {
    setCustomerModal(mode);
    setCustomerForm(emptyCustomerForm);
    setSavedCustomer(null);
    setSelectedExistingCustomerId("");
    setCustomerError("");
  }

  async function createOrLinkCustomer(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!customerModal) return;
    setCustomerBusy(true);
    setCustomerError("");
    let customerWasSaved = savedCustomer !== null;
    try {
      let customer = savedCustomer
        ?? (customerModal === "completed-sale" && selectedExistingCustomerId
          ? customers.find((item) => item.id === selectedExistingCustomerId) ?? null
          : null);
      if (selectedExistingCustomerId && !customer) {
        throw new Error("That customer is no longer available. Refresh the register and try again.");
      }
      if (!customer) {
        customer = await posApi.saveCustomer({
          name: customerForm.name.trim(),
          phone: customerForm.phone.trim() || null,
          email: customerForm.email.trim() || null,
          address: null,
          notes: customerForm.notes.trim() || null,
        });
        customerWasSaved = true;
        setSavedCustomer(customer);
      }
      setCustomers((current) => current.some((item) => item.id === customer.id) ? current : [...current, customer].sort((a, b) => a.name.localeCompare(b.name)));
      if (customerModal === "active-ticket") {
        await dispatchAction({ type: "selectCustomer", customerId: customer.id });
        setCustomerModal(null);
      } else if (receipt) {
        await posApi.attachSaleCustomer({ saleId: receipt.id, customerId: customer.id });
        setReceiptCustomer(customer);
        setCustomerModal(null);
      }
    } catch (reason) {
      setCustomerError(typeof reason === "string"
        ? reason
        : customerWasSaved
          ? "The customer is saved, but the register could not finish linking it. Try again."
          : selectedExistingCustomerId
            ? "The customer could not be linked to this sale. Try again."
            : "The customer could not be saved.");
    } finally {
      setCustomerBusy(false);
    }
  }

  function receiptMessage(current: SaleReceipt) {
    const issued = new Intl.DateTimeFormat("en", { dateStyle: "medium", timeStyle: "short" }).format(new Date(current.createdAt));
    return [
      receiptPreferences.receiptHeaderText,
      user.businessName,
      user.storeName,
      `Served by: ${current.cashierName.trim().split(/\s+/)[0]}`,
      `Receipt ${current.receiptNumber}`,
      ...current.items.map((item) => `${item.quantity} × ${item.name} — ${formatMoney(item.lineTotalMinor, user.currencyCode)}`),
      `Total paid: ${formatMoney(current.totals.totalMinor, user.currencyCode)}`,
      `Payment: ${current.paymentDetail?.label ?? current.paymentMethod.replace("_", " ")}`,
      ...(current.paymentDetail?.reference ? [`Reference: ${current.paymentDetail.reference}`] : []),
      `Date: ${issued}`,
      receiptPreferences.receiptFooterText,
    ].filter(Boolean).join("\n");
  }

  async function shareReceipt(channel: "email" | "whatsapp" | "copy") {
    if (!receipt) return;
    const message = receiptMessage(receipt);
    setShareNotice("");
    try {
      if (channel === "copy") {
        await navigator.clipboard.writeText(message);
        setShareNotice("Receipt copied. You can paste it into another channel.");
      } else if (channel === "email" && receiptCustomer?.email) {
        const subject = `Receipt ${receipt.receiptNumber} from ${user.businessName}`;
        await openUrl(`mailto:${encodeURIComponent(receiptCustomer.email)}?subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(message)}`);
        setShareNotice("Email draft opened. Review it and send it from your mail app.");
      } else if (channel === "whatsapp" && receiptCustomer?.phone) {
        const normalized = receiptCustomer.phone.trim().replace(/^00/, "+").replace(/[\s().-]/g, "");
        if (!/^\+[1-9]\d{7,14}$/.test(normalized)) {
          setShareNotice("Add the customer’s number in international format, such as +233… .");
          return;
        }
        await openUrl(`https://wa.me/${normalized.slice(1)}?text=${encodeURIComponent(message)}`);
        setShareNotice("WhatsApp opened with a draft. Review it and send it from WhatsApp.");
      }
    } catch {
      setShareNotice("Could not open that channel. Copy the receipt text and share it manually.");
    }
  }

  if (loading || !workspace) {
    return <section className="register-page register-loading"><p>{loading ? "Opening the saved register…" : error}</p></section>;
  }

  const ticketCanBeHeld = ticketHasContent(workspace.active);
  const paymentOkay = workspace.active.paymentMethod !== "cash" || tenderedMinor >= total;
  const selectedPayment = paymentOptions.find((option) => option.id === workspace.active.paymentOptionId && option.kind === workspace.active.paymentMethod);
  const canComplete = canTransact && cart.length > 0 && !busy && savingActions === 0 && priceChangedLines.length === 0
    && unavailableLines.length === 0 && shortStockLines.length === 0 && paymentOkay
    && !discountNeedsApproval && !discountInputChanged
    && Boolean(selectedPayment) && (!selectedPayment?.referenceRequired || Boolean(workspace.active.paymentReference.trim()));

  return <section className={`register-page ${workspace.lastCompleted && !hideLastCompleted ? "has-checkout-recovery" : ""}`}>
    {workspace.lastCompleted && !hideLastCompleted && <div className="register-recovery-banner"><Check size={15}/><span>Last checkout completed · {workspace.lastCompleted.receipt.receiptNumber}</span><button type="button" onClick={() => { setReceipt(workspace.lastCompleted?.receipt ?? null); setReceiptCustomer(customers.find((customer) => customer.id === workspace.lastCompleted?.customerId) ?? null); }}>View receipt</button><button type="button" onClick={() => setHideLastCompleted(true)} aria-label="Hide checkout recovery note"><X size={13}/></button></div>}

    <CashDrawerControl currencyCode={user.currencyCode} canTransact={canTransact}/>

    <div className="register-panel-switch" role="tablist" aria-label="Register panels">
      <button className={mobilePanel === "catalog" ? "active" : ""} role="tab" aria-selected={mobilePanel === "catalog"} onClick={() => setMobilePanel("catalog")}>Products</button>
      <button className={mobilePanel === "ticket" ? "active" : ""} role="tab" aria-selected={mobilePanel === "ticket"} onClick={() => setMobilePanel("ticket")}>Current sale{itemCount ? ` · ${itemCount}` : ""}</button>
    </div>

    <div className={`register-layout show-${mobilePanel}`}>
      <div className="catalog-panel">
        <div className="catalog-toolbar">
          <div className="searchbox">
            <Search size={18}/>
            <input
              autoFocus
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key !== "Enter" || !query.trim()) return;
                event.preventDefault();
                scanOrResolve(query);
              }}
              placeholder="Search or scan barcode…"
              aria-label="Search products or scan a barcode"
            />
            <kbd>Enter</kbd>
          </div>
          <label className="filter-select-label"><span className="sr-only">Filter category</span><select className="button button-secondary filter-button" value={categoryFilter} onChange={(event) => setCategoryFilter(event.target.value)}><option value="">All products</option>{categoryNames.map((name) => <option key={name} value={name}>{name}</option>)}</select><ChevronDown size={15}/></label>
        </div>
        <div className="catalog-caption"><span>{query || categoryFilter ? `${filtered.length} results` : `${products.filter((product) => product.isActive).length} products`}</span><span><Barcode size={15}/> Scanner ready</span></div>
        <div className="product-grid">
          {filtered.map((product, index) => <button key={product.id} className={`product-tile tile-${index % 5}`} onClick={() => void addProduct(product)} disabled={product.stockQuantity < 1}>
            <span className="product-art"><EntityImageThumbnail entityType="product" entityId={product.id} fallback={product.name.trim().charAt(0).toUpperCase()} className="register-product-photo"/><i>＋</i></span>
            <span className="product-tile-name">{product.name}</span>
            <span className="product-tile-bottom"><strong>{formatMoney(product.priceMinor, user.currencyCode)}</strong><small className={product.stockQuantity <= product.reorderPoint ? "stock-warning" : ""}>{product.stockQuantity} in stock</small></span>
          </button>)}
          {!filtered.length && <div className="catalog-empty"><span className="empty-icon"><ShoppingBasket size={24}/></span><strong>{products.length ? "No matching products" : "Your catalog is ready"}</strong><span>{products.length ? "Try a different name, SKU, barcode, or category." : "Add products in Products & stock, then return here to make a sale."}</span></div>}
        </div>
      </div>

      <aside className="ticket-panel">
        <div className="ticket-heading">
          <div><span className="ticket-kicker">CURRENT SALE</span><h2>Order ticket <span>#{workspace.active.ticketNumber}</span></h2></div>
          <span className="ticket-count">{itemCount}</span>
        </div>
        <label className="ticket-customer"><UserRound size={15}/><select value={workspace.active.customerId ?? ""} onChange={(event) => void dispatchAction({ type: "selectCustomer", customerId: event.target.value || null })} aria-label="Customer for this sale"><option value="">Walk-in customer</option>{customers.map((customer) => <option key={customer.id} value={customer.id}>{customer.name}{customer.phone ? ` · ${customer.phone}` : ""}</option>)}</select><ChevronDown size={14}/></label>
        <div className="ticket-tools">
          <button type="button" onClick={() => openCustomerForm("active-ticket")}><UserPlus size={14}/>New customer</button>
          <button type="button" onClick={() => void holdCurrentTicket()} disabled={!ticketCanBeHeld || savingActions > 0}><Pause size={14}/>Hold ticket</button>
          <button type="button" onClick={() => setTicketsOpen(true)}><Clock3 size={14}/>Held {heldCount > 0 ? `(${heldCount})` : "tickets"}</button>
          <button type="button" onClick={() => void clearTicket()} disabled={!ticketCanBeHeld || savingActions > 0} aria-label="Clear the active ticket"><Trash2 size={14}/>Clear all</button>
        </div>

        {cart.length ? <div className="cart-lines">
          {cart.map(({ ticketLine, product }) => {
            const unavailable = !product || !product.isActive;
            const priceChanged = product !== null && product.priceMinor !== ticketLine.acceptedUnitPriceMinor;
            const shortStock = product !== null && product.stockQuantity < ticketLine.quantity;
            return <div className="cart-line" key={ticketLine.productId}>
              <input className="cart-line-select" type="checkbox" checked={selectedLines.has(ticketLine.productId)} onChange={(event) => setSelectedLines((current) => {
                const next = new Set(current);
                if (event.target.checked) next.add(ticketLine.productId); else next.delete(ticketLine.productId);
                return next;
              })} aria-label={`Select ${ticketLine.acceptedName}`}/>
              <div className="cart-product-icon">{ticketLine.acceptedName.charAt(0).toUpperCase()}</div>
              <div className="cart-product">
                <strong>{ticketLine.acceptedName}</strong>
                <small>{formatMoney(ticketLine.acceptedUnitPriceMinor, user.currencyCode)} each</small>
                {priceChanged && <small className="cart-line-warning">Price now {formatMoney(product.priceMinor, user.currencyCode)} · review below</small>}
                {unavailable && <small className="cart-line-warning">No longer active · remove to continue</small>}
                {shortStock && <small className="cart-line-warning">Only {product.stockQuantity} left · reduce quantity</small>}
                <div className="quantity-control"><button onClick={() => ticketLine.quantity <= 1 ? void dispatchAction({ type: "removeLines", productIds: [ticketLine.productId] }) : void dispatchAction({ type: "setQuantity", productId: ticketLine.productId, quantity: ticketLine.quantity - 1 })} aria-label={`Remove one ${ticketLine.acceptedName}`}><CircleMinus size={16}/></button><span>{ticketLine.quantity}</span><button onClick={() => void dispatchAction({ type: "setQuantity", productId: ticketLine.productId, quantity: ticketLine.quantity + 1 })} aria-label={`Add one ${ticketLine.acceptedName}`} disabled={!product || ticketLine.quantity >= product.stockQuantity}><CirclePlus size={16}/></button></div>
              </div>
              <div className="cart-line-total"><strong>{formatMoney(ticketLine.acceptedUnitPriceMinor * ticketLine.quantity, user.currencyCode)}</strong><button onClick={() => void dispatchAction({ type: "removeLines", productIds: [ticketLine.productId] })} aria-label={`Remove ${ticketLine.acceptedName}`}><Trash2 size={15}/></button></div>
            </div>;
          })}
        </div> : <div className="ticket-empty"><span className="ticket-empty-icon"><ShoppingBasket size={23}/></span><strong>Your ticket is empty</strong><span>Choose a product or scan its barcode to add it.</span></div>}

        <div className="ticket-bottom">
          <div className="ticket-summary-scroll">
          {selectedLines.size > 0 && <div className="selected-line-tools"><span>{selectedLines.size} selected</span><button type="button" onClick={() => { void dispatchAction({ type: "removeLines", productIds: [...selectedLines] }); setSelectedLines(new Set()); }}><Trash2 size={13}/>Remove selected</button></div>}
          <div className="discount-row"><span>Discount</span><span className="money-input"><span>{user.currencyCode}</span><input inputMode="decimal" value={discountInput} onChange={(event) => scheduleDiscount(event.target.value)} placeholder="0.00" aria-label="Discount amount"/><button className="discount-apply" type="button" onClick={() => void requestDiscount()} disabled={savingActions>0}>{discountMinor>0 && parseMinor(discountInput)===0 ? "Clear" : "Apply"}</button></span></div>
          {(parseMinor(discountInput)>0 || discountMinor>0) && <div className="discount-request-controls"><label className="field-label">Reason for discount<input className="text-input" maxLength={200} value={discountReasonInput} onChange={(event)=>scheduleDiscountReason(event.target.value)} placeholder="For example: damaged packaging"/></label>{discountMinor>0 && <div className={`discount-approval-state ${discountNeedsApproval?"pending":"approved"}`}>{discountNeedsApproval ? <><span>Discount awaiting manager approval</span><button type="button" onClick={()=>void openDiscountApproval()}>Get approval</button></> : <span>Approved by {activeTicket?.discountApproverName} · until {new Intl.DateTimeFormat("en",{timeStyle:"short"}).format(new Date(activeTicket?.discountApprovalExpiresAt ?? ""))}</span>}</div>}{discountInputChanged && <small>Apply the changed amount and reason before checkout.</small>}</div>}
          <div className="total-row"><span>Subtotal</span><strong>{formatMoney(subtotal, user.currencyCode)}</strong></div>
          <div className="total-row total-grand"><span>Total due</span><strong>{formatMoney(total, user.currencyCode)}</strong></div>

          {priceChangedLines.length > 0 && <div className="price-review-notice"><AlertTriangle size={15}/><span>Catalog prices changed. Review the new prices, then accept them before taking payment.</span><button className="button button-small button-secondary" type="button" onClick={() => void dispatchAction({ type: "acceptCurrentPrices" })}>Accept prices</button></div>}
          {unavailableLines.length > 0 && <p className="form-error checkout-error">Remove unavailable products before completing this sale.</p>}
          {shortStockLines.length > 0 && <p className="form-error checkout-error">Reduce quantities to match current stock.</p>}

          <div className="payment-label">PAYMENT METHOD</div>
          <div className="payment-options">{paymentOptions.map((option) => <button key={option.id ?? option.kind} className={selectedPayment === option ? "payment-option selected" : "payment-option"} onClick={() => void dispatchAction({ type: "setPayment", paymentMethod: option.kind, paymentOptionId: option.id, paymentReference: "", amountTenderedMinor: option.kind === "cash" ? tenderedMinor : total })}><CreditCard size={16}/>{option.name}</button>)}</div>
          {!selectedPayment && <p className="settings-hint">Choose an enabled payment method. Ask an administrator if no methods are available.</p>}
          {selectedPayment && <div className="checkout-payment-info"><p>{selectedPayment.instructions}</p>{selectedPayment.merchantAccount && <p>{selectedPayment.provider} · {selectedPayment.merchantAccount}</p>}{selectedPayment.terminalId && <p>Terminal: {selectedPayment.terminalId}</p>}
            {(selectedPayment.kind !== "cash" || selectedPayment.referenceRequired) && <label className="field-label">Transaction reference{selectedPayment.referenceRequired ? " (required)" : ""}<input key={`${workspace.active.id}-${selectedPayment.id ?? selectedPayment.kind}`} className="text-input" defaultValue={workspace.active.paymentReference} maxLength={120} onChange={(event) => void dispatchAction({ type: "setPaymentReference", reference: event.target.value })}/></label>}
            {selectedPayment.kind !== "cash" && <small>Record only after confirming approval on the terminal or merchant account. This is a manual payment record.</small>}
          </div>}
          {workspace.active.paymentMethod === "cash" && <div className="tender-row"><label htmlFor="tendered">Amount received</label><span className="money-input tender-input"><span>{user.currencyCode}</span><input id="tendered" inputMode="decimal" value={tenderInput} onChange={(event) => scheduleTender(event.target.value)} onBlur={() => void commitTender().catch(() => {})} placeholder="0.00"/></span></div>}
          {workspace.active.paymentMethod === "cash" && tenderedMinor >= total && total > 0 && <div className="change-row"><span>Change due</span><strong>{formatMoney(tenderedMinor - total, user.currencyCode)}</strong></div>}
          </div>
          <div className="ticket-checkout-actions">
            {error && <p className="form-error checkout-error" role="alert">{error}</p>}
            {!canTransact && <p className="settings-hint checkout-permission-note">Ticket is ready to review. An owner, administrator, or manager must enable temporary cashier mode before checkout.</p>}
            <button className="button button-primary complete-sale" disabled={!canComplete} title={!canTransact ? "Enable temporary cashier mode to check out" : undefined} onClick={() => void completeSale()}>{busy ? "Completing sale…" : savingActions ? "Saving ticket…" : workspace.active.paymentMethod === "cash" ? "Complete cash sale" : "Record confirmed payment"}<span>{formatMoney(total, user.currencyCode)} <span>→</span></span></button>
            <p className="ticket-footnote"><span className="register-save-state">{savingActions ? "Saving ticket…" : "Ticket saved on this device"}</span> · Sale and stock updates save together.</p>
          </div>
        </div>
      </aside>
    </div>

    {discountApprovalOpen && discountPreview && <div className="modal-backdrop" role="presentation" onMouseDown={(event)=>{if(event.target===event.currentTarget&&!approvingDiscount)setDiscountApprovalOpen(false);}}><article className="receipt-modal discount-approval-modal" role="dialog" aria-modal="true" aria-labelledby="discount-approval-title"><button className="modal-close" type="button" onClick={()=>setDiscountApprovalOpen(false)} disabled={approvingDiscount} aria-label="Close discount approval"><X size={19}/></button><p className="eyebrow">MANAGER REVIEW</p><h2 id="discount-approval-title">Approve this discount</h2><p className="receipt-number">Review the basket and enter the selected approver’s personal PIN or password. The cashier stays signed in.</p><div className="discount-approval-lines">{discountPreview.lines.map((line,index)=><div key={`${line.name}-${index}`}><span>{line.quantity} × {line.name}</span><strong>{formatMoney(line.lineTotalMinor,discountPreview.currencyCode)}</strong></div>)}</div><div className="discount-approval-totals"><div><span>Subtotal</span><strong>{formatMoney(discountPreview.subtotalMinor,discountPreview.currencyCode)}</strong></div><div><span>Requested discount</span><strong>−{formatMoney(discountPreview.discountMinor,discountPreview.currencyCode)}</strong></div><div><span>Tax</span><strong>{formatMoney(discountPreview.taxMinor,discountPreview.currencyCode)}</strong></div><div className="discount-approval-grand"><span>Amount due</span><strong>{formatMoney(discountPreview.totalMinor,discountPreview.currencyCode)}</strong></div></div><p className="discount-approval-reason"><strong>Reason:</strong> {discountPreview.reason}</p><form onSubmit={(event)=>void approveDiscount(event)}><label className="field-label">Approver<select className="text-input" required value={selectedApproverId} onChange={(event)=>setSelectedApproverId(event.target.value)}>{discountApprovers.map((approver)=><option key={approver.userId} value={approver.userId}>{approver.fullName} · {approver.roleName}</option>)}</select></label><label className="field-label">Personal PIN or password<input className="text-input" required type="password" autoComplete="current-password" value={approverCredential} onChange={(event)=>setApproverCredential(event.target.value)}/></label>{discountApprovers.length===0&&<p className="form-error" role="alert">No active account has discount approval permission.</p>}{error&&<p className="form-error" role="alert">{attemptCountdown.format(error)}</p>}{attemptCountdown.remainingSeconds>0&&<p className="settings-hint" role="status">Approval unlocks in {attemptCountdown.remainingSeconds} seconds.</p>}<div className="settings-actions"><button className="button button-secondary" type="button" onClick={()=>setDiscountApprovalOpen(false)} disabled={approvingDiscount}>Cancel</button><button className="button button-primary" type="submit" disabled={approvingDiscount||attemptCountdown.remainingSeconds>0||discountApprovers.length===0||!approverCredential}>{approvingDiscount?"Checking approval…":attemptCountdown.remainingSeconds>0?`Try again in ${attemptCountdown.remainingSeconds}s`:"Approve discount"}</button></div></form></article></div>}

    {ticketsOpen && <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) setTicketsOpen(false); }}><article className="receipt-modal held-tickets-modal" role="dialog" aria-modal="true" aria-labelledby="held-tickets-title"><button className="modal-close" type="button" onClick={() => setTicketsOpen(false)} aria-label="Close held tickets"><X size={19}/></button><p className="eyebrow">REGISTER QUEUE</p><h2 id="held-tickets-title">Held tickets</h2><p className="receipt-number">Pick up a saved basket. It stays here if payment failed or you switch staff.</p><div className="held-tickets-list">{workspace.held.map((ticket) => <div className="held-ticket-row" key={ticket.id}><div className="held-ticket-info"><strong>{ticket.ticketNumber} · {ticket.customerId ? customers.find((customer) => customer.id === ticket.customerId)?.name ?? "Customer" : "Walk-in"}</strong><small>{ticket.lines.reduce((sum, line) => sum + line.quantity, 0)} items · Last updated {new Intl.DateTimeFormat("en", { dateStyle: "short", timeStyle: "short" }).format(new Date(ticket.updatedAt))}</small><small>Started by {ticket.creatorName} · last handled by {ticket.lastEditorName}</small></div><div className="held-ticket-actions"><button className="button button-primary button-small" type="button" onClick={() => void resumeTicket(ticket.id)}>Resume</button><button className="button button-secondary button-small" type="button" onClick={() => void discardHeldTicket(ticket.id)} aria-label={`Discard ticket ${ticket.ticketNumber}`}><Trash2 size={14}/></button></div></div>)}{workspace.held.length === 0 && <div className="ticket-empty"><span className="ticket-empty-icon"><Clock3 size={23}/></span><strong>No held tickets</strong><span>Hold a current sale to serve another customer.</span></div>}</div></article></div>}

    {customerModal && <div className={`modal-backdrop ${customerModal === "completed-sale" ? "post-sale-customer-backdrop" : ""}`} role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !customerBusy) setCustomerModal(null); }}><article className="receipt-modal customer-create-modal" role="dialog" aria-modal="true" aria-labelledby="customer-create-title"><button className="modal-close" type="button" onClick={() => setCustomerModal(null)} disabled={customerBusy} aria-label="Close customer form"><X size={19}/></button><p className="eyebrow">CUSTOMER DETAILS</p><h2 id="customer-create-title">{customerModal === "completed-sale" ? "Add this customer to your list" : "Add a walk-in customer"}</h2><p className="receipt-number">{customerModal === "completed-sale" ? "Link an existing customer or save a new walk-in. Phone and email are optional." : "Save a name and contact detail for this customer. Phone and email are optional."}</p><form className="customer-create-form" onSubmit={(event) => void createOrLinkCustomer(event)}>{customerModal === "completed-sale" && customers.length > 0 && !savedCustomer && <label className="field-label">Existing customer<select className="input-control" value={selectedExistingCustomerId} onChange={(event) => { setSelectedExistingCustomerId(event.target.value); setCustomerError(""); }}><option value="">Create a new customer</option>{customers.map((customer) => <option key={customer.id} value={customer.id}>{customer.name}{customer.phone ? ` · ${customer.phone}` : ""}</option>)}</select></label>}{!selectedExistingCustomerId && <><label className="field-label">Customer name<input className="text-input" required maxLength={100} autoFocus disabled={Boolean(savedCustomer)} value={savedCustomer?.name ?? customerForm.name} onChange={(event) => setCustomerForm((current) => ({ ...current, name: event.target.value }))}/></label><label className="field-label">Phone number<input className="text-input" type="tel" maxLength={40} autoComplete="tel" disabled={Boolean(savedCustomer)} value={savedCustomer?.phone ?? customerForm.phone} onChange={(event) => setCustomerForm((current) => ({ ...current, phone: event.target.value }))}/></label><label className="field-label">Email address<input className="text-input" type="email" maxLength={254} autoComplete="email" disabled={Boolean(savedCustomer)} value={savedCustomer?.email ?? customerForm.email} onChange={(event) => setCustomerForm((current) => ({ ...current, email: event.target.value }))}/></label><label className="field-label">Note or identifying detail<input className="text-input" maxLength={500} disabled={Boolean(savedCustomer)} value={savedCustomer?.notes ?? customerForm.notes} onChange={(event) => setCustomerForm((current) => ({ ...current, notes: event.target.value }))}/></label></>}{customerError && <p className="form-error" role="alert">{customerError}</p>}<button className="button button-primary receipt-done" type="submit" disabled={customerBusy}>{customerBusy ? "Linking customer…" : savedCustomer ? "Retry linking to sale" : selectedExistingCustomerId ? "Link existing customer" : customerModal === "completed-sale" ? "Save and link to sale" : "Save and add to ticket"}<Check size={16}/></button></form></article></div>}

    {receipt && <div className="modal-backdrop receipt-print-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) setReceipt(null); }}><article className="receipt-modal receipt-printable" role="dialog" aria-modal="true" aria-labelledby="receipt-title"><style>{`@media print{@page{size:${receiptPreferences.receiptPaperWidthMm}mm auto;margin:4mm}html,body{width:${receiptPreferences.receiptPaperWidthMm}mm}.receipt-modal{width:calc(${receiptPreferences.receiptPaperWidthMm}mm - 8mm)!important}}`}</style><button className="modal-close" type="button" onClick={() => setReceipt(null)} aria-label="Close receipt"><X size={19}/></button><span className="receipt-success"><Check size={22}/></span><p className="eyebrow">{user.businessName.toUpperCase()}</p>{receiptPreferences.receiptHeaderText && <p className="receipt-config-line">{receiptPreferences.receiptHeaderText}</p>}<h2 id="receipt-title">Sale recorded</h2><p className="receipt-number">{user.storeName} · Receipt {receipt.receiptNumber}</p><p className="receipt-number">{new Intl.DateTimeFormat("en", { dateStyle: "medium", timeStyle: "short" }).format(new Date(receipt.createdAt))} · Served by {receipt.cashierName.trim().split(/\s+/)[0]}</p><div className="receipt-total">{formatMoney(receipt.totals.totalMinor, user.currencyCode)}</div><div className="receipt-details">{receipt.items.map((item) => <div key={item.productId}><span>{item.quantity} × {item.name}</span><strong>{formatMoney(item.lineTotalMinor, user.currencyCode)}</strong></div>)}<div className="receipt-detail-total"><span>Payment · {receipt.paymentDetail?.label ?? receipt.paymentMethod.replace("_", " ")}</span><strong>{formatMoney(receipt.amountTenderedMinor, user.currencyCode)}</strong></div>{receipt.changeDueMinor > 0 && <div><span>Change</span><strong>{formatMoney(receipt.changeDueMinor, user.currencyCode)}</strong></div>}</div>{receipt.paymentDetail?.reference && <p className="receipt-number">Reference: {receipt.paymentDetail.reference}{receipt.paymentDetail.terminalId ? ` · Terminal ${receipt.paymentDetail.terminalId}` : ""}</p>}{receiptCustomer ? <p className="receipt-customer-linked">Customer · {receiptCustomer.name}{receiptCustomer.phone ? ` · ${receiptCustomer.phone}` : ""}</p> : <button className="receipt-link-customer" type="button" onClick={() => openCustomerForm("completed-sale")}><UserPlus size={15}/>Add customer details to this sale</button>}{receiptPreferences.receiptFooterText && <p className="receipt-config-line">{receiptPreferences.receiptFooterText}</p>}<div className="receipt-actions"><button className="button button-secondary" onClick={() => window.print()}><Printer size={16}/>Print</button>{receiptCustomer?.email && <button className="button button-secondary" onClick={() => void shareReceipt("email")}><Mail size={16}/>Email</button>}{receiptCustomer?.phone && <button className="button button-secondary" onClick={() => void shareReceipt("whatsapp")}><MessageCircle size={16}/>WhatsApp</button>}<button className="button button-secondary" onClick={() => void shareReceipt("copy")}><Copy size={16}/>Copy</button></div>{shareNotice && <p className="receipt-share-notice" role="status">{shareNotice}</p>}<button className="button button-primary receipt-done" onClick={() => setReceipt(null)}>Done <Check size={17}/></button></article></div>}
  </section>;
}

function parseMinor(value: string) {
  const numeric = Number(value.replace(/,/g, ""));
  return Number.isFinite(numeric) && numeric > 0 ? Math.round(numeric * 100) : 0;
}

function discountApprovalIsCurrent(ticket: RegisterTicket, now = Date.now()) {
  const expiresAt = ticket.discountApprovalExpiresAt ? Date.parse(ticket.discountApprovalExpiresAt) : Number.NaN;
  return Boolean(ticket.discountApprovalId && Number.isFinite(expiresAt) && expiresAt > now);
}

function minorToInput(value: number) {
  return value > 0 ? (value / 100).toFixed(2) : "";
}

function ticketHasContent(ticket: RegisterTicket) {
  return ticket.lines.length > 0 || ticket.customerId !== null || ticket.discountMinor > 0 || ticket.amountTenderedMinor > 0;
}
