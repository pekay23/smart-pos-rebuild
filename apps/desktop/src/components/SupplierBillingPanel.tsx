// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { posApi } from "../lib/api";
import type { CashLocationOption, CreateSupplierCreditNoteInput, MatchSupplierBillInput, RecordSupplierPaymentInput, SessionUser, Supplier, SupplierBillingWorkspace } from "../lib/models";
import { formatDateTime, formatMoney, parseMoneyMinor } from "../lib/money";

type AllocationAmounts = Record<string, string>;
type PaymentMethod = RecordSupplierPaymentInput["paymentMethod"];
type CashOrigin = "drawer" | "other_cash";
type ReturnQuantities = Record<string, string>;
const paymentMethods: { value: PaymentMethod; label: string }[] = [
  { value: "cash", label: "Cash" }, { value: "card", label: "Card terminal" },
  { value: "mobile_money", label: "Mobile money" }, { value: "bank_transfer", label: "Bank transfer" },
  { value: "other", label: "Other" },
];

function newKey() { return crypto.randomUUID(); }

export function SupplierBillingPanel({ user, suppliers }: { user: SessionUser; suppliers: Supplier[] }) {
  const canInvoice = user.permissions.includes("purchases:invoice");
  const canSettle = user.permissions.includes("suppliers:settle");
  const [workspace, setWorkspace] = useState<SupplierBillingWorkspace>({ unbilledReceipts: [], openBills: [], returnableItems: [] });
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [billSupplierId, setBillSupplierId] = useState("");
  const [paymentSupplierId, setPaymentSupplierId] = useState("");
  const [supplierReference, setSupplierReference] = useState("");
  const [invoiceDate, setInvoiceDate] = useState(() => new Date().toISOString().slice(0, 10));
  const [dueDate, setDueDate] = useState("");
  const [invoiceAmount, setInvoiceAmount] = useState("");
  const [receiptAmounts, setReceiptAmounts] = useState<AllocationAmounts>({});
  const [paymentAmounts, setPaymentAmounts] = useState<AllocationAmounts>({});
  const [creditBillId, setCreditBillId] = useState("");
  const [creditReference, setCreditReference] = useState("");
  const [creditDate, setCreditDate] = useState(() => new Date().toISOString().slice(0, 10));
  const [creditReason, setCreditReason] = useState("Goods returned to supplier");
  const [returnQuantities, setReturnQuantities] = useState<ReturnQuantities>({});
  const creditAttempt = useRef<{ fingerprint: string; key: string } | null>(null);
  const [paymentMethod, setPaymentMethod] = useState<PaymentMethod>("cash");
  const [cashOrigin, setCashOrigin] = useState<CashOrigin>("other_cash");
  const [cashSourceNote, setCashSourceNote] = useState("");
  const [cashLocationId, setCashLocationId] = useState("");
  const [cashLocations, setCashLocations] = useState<CashLocationOption[]>([]);
  const paymentAttempt = useRef<{ fingerprint: string; key: string } | null>(null);
  const [externalReference, setExternalReference] = useState("");
  const [paymentAmount, setPaymentAmount] = useState("");

  const refresh = useCallback(async () => {
    try { setWorkspace(await posApi.supplierBillingWorkspace()); setError(""); }
    catch { setError("Could not load supplier invoices and outstanding balances."); }
    finally { setLoading(false); }
  }, []);

  useEffect(() => {
    let active = true;
    void posApi.supplierBillingWorkspace()
      .then((nextWorkspace) => {
        if (!active) return;
        setWorkspace(nextWorkspace);
        setError("");
      })
      .catch(() => { if (active) setError("Could not load supplier invoices and outstanding balances."); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    let active = true;
    void posApi.cashLocationOptions().then((rows) => {
      if (!active) return;
      setCashLocations(rows);
      setCashLocationId(rows.find((row) => row.locationType !== "register" && row.openingRecorded)?.id ?? "");
    }).catch(() => { if (active) setCashLocations([]); });
    return () => { active = false; };
  }, []);

  const eligibleReceipts = useMemo(() => workspace.unbilledReceipts.filter((receipt) => receipt.supplierId === billSupplierId), [billSupplierId, workspace.unbilledReceipts]);
  const eligibleBills = useMemo(() => workspace.openBills.filter((bill) => bill.supplierId === paymentSupplierId), [paymentSupplierId, workspace.openBills]);
  const eligibleReturnItems = useMemo(() => workspace.returnableItems.filter((item) => item.billId === creditBillId), [creditBillId, workspace.returnableItems]);
  const selectedCreditBill = workspace.openBills.find((bill) => bill.billId === creditBillId);
  const receiptTotal = eligibleReceipts.reduce((sum, row) => sum + (parseMoneyMinor(receiptAmounts[row.purchaseId] ?? "") ?? 0), 0);
  const paymentTotal = eligibleBills.reduce((sum, row) => sum + (parseMoneyMinor(paymentAmounts[row.billId] ?? "") ?? 0), 0);
  const creditTotal = eligibleReturnItems.reduce((sum, row) => {
    const raw = returnQuantities[row.purchaseItemId] ?? "";
    if (!/^\d{1,7}$/.test(raw)) return sum;
    const quantity = Number(raw);
    const lineTotal = quantity > 0 && quantity <= row.maxQuantity ? quantity * row.unitCostMinor : 0;
    return Number.isSafeInteger(lineTotal) && Number.isSafeInteger(sum + lineTotal) ? sum + lineTotal : sum;
  }, 0);

  async function matchInvoice(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError(""); setNotice("");
    const allocations = eligibleReceipts.flatMap((receipt) => {
      const amountMinor = parseMoneyMinor(receiptAmounts[receipt.purchaseId] ?? "");
      return amountMinor !== null && amountMinor > 0 ? [{ purchaseId: receipt.purchaseId, amountMinor }] : [];
    });
    const statedTotal = parseMoneyMinor(invoiceAmount);
    if (!billSupplierId || !supplierReference.trim() || allocations.length === 0 || statedTotal === null || statedTotal !== receiptTotal) {
      setError("Choose a supplier, enter the invoice reference and amount, and make the allocations equal the invoice total."); setBusy(false); return;
    }
    const input: MatchSupplierBillInput = { supplierId: billSupplierId, supplierReference: supplierReference.trim(), invoiceDate, dueDate: dueDate || null, totalMinor: statedTotal, idempotencyKey: newKey(), allocations };
    try {
      const result = await posApi.matchSupplierBill(input);
      setNotice(`Invoice matched for ${formatMoney(result.amountMinor, result.currencyCode)}.`);
      setSupplierReference(""); setDueDate(""); setInvoiceAmount(""); setReceiptAmounts({}); await refresh();
    } catch (reason) { setError(typeof reason === "string" ? reason : "The invoice could not be matched."); }
    finally { setBusy(false); }
  }

  async function recordPayment(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError(""); setNotice("");
    const allocations = eligibleBills.flatMap((bill) => {
      const amountMinor = parseMoneyMinor(paymentAmounts[bill.billId] ?? "");
      return amountMinor !== null && amountMinor > 0 ? [{ billId: bill.billId, amountMinor }] : [];
    });
    const statedAmount = parseMoneyMinor(paymentAmount);
    if (!paymentSupplierId || !allocations.length || statedAmount === null || statedAmount !== paymentTotal) { setError("Choose a supplier, enter the payment amount, and make the allocations equal the amount paid."); setBusy(false); return; }
    const details = {
      supplierId: paymentSupplierId,
      paymentMethod,
      cashOrigin: paymentMethod === "cash" ? cashOrigin : "not_applicable" as const,
      cashSourceNote: paymentMethod === "cash" && cashOrigin === "other_cash" ? (cashLocations.find((row) => row.id === cashLocationId)?.name ?? cashSourceNote.trim()) : null,
      cashLocationId: paymentMethod === "cash" && cashOrigin === "other_cash" ? cashLocationId || null : null,
      externalReference: externalReference.trim() || null,
      amountMinor: statedAmount,
      allocations,
    };
    if (details.cashOrigin === "other_cash" && !details.cashLocationId) { setError("Choose a tracked safe or petty cash location."); setBusy(false); return; }
    const fingerprint = JSON.stringify(details);
    const attempt = paymentAttempt.current?.fingerprint === fingerprint
      ? paymentAttempt.current
      : { fingerprint, key: newKey() };
    paymentAttempt.current = attempt;
    const input: RecordSupplierPaymentInput = { ...details, idempotencyKey: attempt.key };
    try {
      const result = await posApi.recordSupplierPayment(input);
      paymentAttempt.current = null;
      const cashSource = paymentMethod !== "cash" ? "" : cashOrigin === "drawer" ? " The register drawer was reduced by this amount." : ` Paid outside the drawer from ${cashSourceNote.trim()}.`;
      setNotice(`Payment recorded for ${formatMoney(result.amountMinor, result.currencyCode)}.${cashSource}`);
      setExternalReference(""); setPaymentAmount(""); setPaymentAmounts({}); setCashOrigin("other_cash"); setCashSourceNote(""); await refresh();
    } catch (reason) { setError(typeof reason === "string" ? reason : "The payment could not be recorded."); }
    finally { setBusy(false); }
  }

  async function recordCreditNote(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError(""); setNotice("");
    const returns = eligibleReturnItems.flatMap((item) => {
      const raw = returnQuantities[item.purchaseItemId] ?? "";
      if (!/^\d{1,7}$/.test(raw)) return [];
      const quantity = Number(raw);
      return quantity > 0 && quantity <= item.maxQuantity
        ? [{ purchaseId: item.purchaseId, purchaseItemId: item.purchaseItemId, quantity }]
        : [];
    });
    if (!creditBillId || !creditReference.trim() || !creditReason.trim() || returns.length === 0 || creditTotal <= 0) {
      setError("Choose an open invoice, enter its credit note reference and reason, and set at least one valid return quantity."); setBusy(false); return;
    }
    const details = { billId: creditBillId, creditReference: creditReference.trim(), effectiveDate: creditDate, reason: creditReason.trim(), returns };
    const fingerprint = JSON.stringify(details);
    const attempt = creditAttempt.current?.fingerprint === fingerprint
      ? creditAttempt.current
      : { fingerprint, key: newKey() };
    creditAttempt.current = attempt;
    const input: CreateSupplierCreditNoteInput = { ...details, idempotencyKey: attempt.key };
    try {
      const result = await posApi.createSupplierCreditNote(input);
      creditAttempt.current = null;
      setNotice(`Supplier credit recorded for ${formatMoney(result.amountMinor, result.currencyCode)}. Returned stock and the unpaid invoice balance were updated.`);
      setCreditReference(""); setCreditReason("Goods returned to supplier"); setReturnQuantities({}); await refresh();
    } catch (reason) { setError(typeof reason === "string" ? reason : "The supplier credit note could not be recorded."); }
    finally { setBusy(false); }
  }

  return <section className="supplier-billing" aria-labelledby="supplier-billing-title">
    <div className="supplier-billing-heading"><div><p className="eyebrow">SUPPLIER ACCOUNTS</p><h2 id="supplier-billing-title">Invoices & payments</h2><p>Match invoices to received deliveries and track what remains payable.</p></div><button type="button" className="button button-secondary button-small" disabled={loading || busy} onClick={() => void refresh()}>{loading ? "Refreshing…" : "Refresh"}</button></div>
    {error && <p className="expense-error" role="alert">{error}</p>}{notice && <p className="supplier-billing-notice" role="status">{notice}</p>}
    {canInvoice && <form className="supplier-billing-card" onSubmit={(event) => void matchInvoice(event)}>
      <div className="supplier-billing-card-heading"><div><h3>Match supplier invoice</h3><p>Allocate invoice value against unbilled deliveries from this supplier.</p></div><strong>{formatMoney(receiptTotal, user.currencyCode)}</strong></div>
      <div className="supplier-billing-fields"><label>Supplier<select className="text-input" required value={billSupplierId} onChange={(event) => setBillSupplierId(event.target.value)}><option value="">Choose supplier</option>{suppliers.filter((supplier) => supplier.isActive).map((supplier) => <option key={supplier.id} value={supplier.id}>{supplier.name}</option>)}</select></label><label>Invoice reference<input className="text-input" required maxLength={100} value={supplierReference} onChange={(event) => setSupplierReference(event.target.value)} placeholder="Supplier invoice number"/></label><label>Invoice amount<input className="text-input" required inputMode="decimal" value={invoiceAmount} onChange={(event) => setInvoiceAmount(event.target.value)} placeholder="0.00"/></label><label>Invoice date<input className="text-input" type="date" required value={invoiceDate} onChange={(event) => setInvoiceDate(event.target.value)}/></label><label>Due date <span className="expense-optional">Optional</span><input className="text-input" type="date" min={invoiceDate} value={dueDate} onChange={(event) => setDueDate(event.target.value)}/></label></div>
      {!eligibleReceipts.length ? <p className="supplier-billing-empty">No posted, unbilled deliveries are available. Receive stock from an active supplier first.</p> : <div className="supplier-billing-allocation-list">{eligibleReceipts.map((receipt) => <label className="supplier-billing-allocation" key={receipt.purchaseId}><span><strong>{receipt.reference || "Delivery"}</strong><small>{formatDateTime(receipt.receivedAt)} · Up to {formatMoney(receipt.unbilledMinor, user.currencyCode)}</small></span><input aria-label={`Invoice allocation for ${receipt.reference || "delivery"}`} className="text-input" inputMode="decimal" placeholder="0.00" value={receiptAmounts[receipt.purchaseId] ?? ""} onChange={(event) => setReceiptAmounts((current) => ({ ...current, [receipt.purchaseId]: event.target.value }))}/></label>)}</div>}
      <p className="supplier-billing-hint">The invoice total is the sum of allocations. Tax, freight, and price differences need separate handling and are not silently booked.</p>
      <button className="button button-primary" disabled={busy || loading || !eligibleReceipts.length}>{busy ? "Saving…" : "Match invoice"}</button>
    </form>}
    {canInvoice && <form className="supplier-billing-card" onSubmit={(event) => void recordCreditNote(event)}>
      <div className="supplier-billing-card-heading"><div><h3>Record goods returned</h3><p>Match a supplier credit note to returned items from the billed delivery.</p></div><strong>{formatMoney(creditTotal, selectedCreditBill?.currencyCode ?? user.currencyCode)}</strong></div>
      <p className="supplier-billing-disclosure">This reduces the invoice’s unpaid balance and on-hand stock at the delivery’s recorded unit cost. Only goods physically returned to the supplier are supported here; price changes, tax, freight, and credits for already paid invoices require separate accounting workflows.</p>
      <div className="supplier-billing-fields"><label>Open supplier invoice<select className="text-input" required value={creditBillId} onChange={(event) => { setCreditBillId(event.target.value); setReturnQuantities({}); }}><option value="">Choose invoice</option>{workspace.openBills.filter((bill) => workspace.returnableItems.some((item) => item.billId === bill.billId)).map((bill) => <option key={bill.billId} value={bill.billId}>{bill.supplierName} · {bill.supplierReference} · Outstanding {formatMoney(bill.outstandingMinor, bill.currencyCode)}</option>)}</select></label><label>Supplier credit note reference<input className="text-input" required maxLength={100} value={creditReference} onChange={(event) => setCreditReference(event.target.value)} placeholder="Supplier credit note number"/></label><label>Credit note date<input className="text-input" type="date" required value={creditDate} onChange={(event) => setCreditDate(event.target.value)}/></label><label>Reason<input className="text-input" required maxLength={500} value={creditReason} onChange={(event) => setCreditReason(event.target.value)} placeholder="Goods returned to supplier"/></label></div>
      {!eligibleReturnItems.length ? <p className="supplier-billing-empty">Choose an open invoice with billed goods still in stock. Only received item quantities at positive recorded cost can be returned.</p> : <div className="supplier-billing-allocation-list">{eligibleReturnItems.map((item) => <label className="supplier-billing-allocation" key={item.purchaseItemId}><span><strong>{item.productName}</strong><small>{item.purchaseReference || "Delivery"} · {formatMoney(item.unitCostMinor, selectedCreditBill?.currencyCode ?? user.currencyCode)} each · Up to {item.maxQuantity} units</small></span><input aria-label={`Return quantity for ${item.productName}`} className="text-input" type="number" min="0" max={item.maxQuantity} step="1" inputMode="numeric" placeholder="0" value={returnQuantities[item.purchaseItemId] ?? ""} onChange={(event) => setReturnQuantities((current) => ({ ...current, [item.purchaseItemId]: event.target.value }))}/></label>)}</div>}
      <button className="button button-primary" disabled={busy || loading || !eligibleReturnItems.length || creditTotal <= 0}>{busy ? "Saving…" : "Record supplier credit"}</button>
    </form>}
    {canSettle && <form className="supplier-billing-card" onSubmit={(event) => void recordPayment(event)}>
      <div className="supplier-billing-card-heading"><div><h3>Record external payment</h3><p>Record a payment already made and allocate it to open invoices.</p></div><strong>{formatMoney(paymentTotal, user.currencyCode)}</strong></div>
      <p className="supplier-billing-disclosure">This records the payment in Advanced POS. It does not initiate or verify a bank, card, or mobile money transfer.</p>
      <div className="supplier-billing-fields"><label>Supplier<select className="text-input" required value={paymentSupplierId} onChange={(event) => setPaymentSupplierId(event.target.value)}><option value="">Choose supplier</option>{suppliers.filter((supplier) => supplier.isActive).map((supplier) => <option key={supplier.id} value={supplier.id}>{supplier.name}</option>)}</select></label><label>Amount paid<input className="text-input" required inputMode="decimal" value={paymentAmount} onChange={(event) => setPaymentAmount(event.target.value)} placeholder="0.00"/></label><label>Payment method<select className="text-input" value={paymentMethod} onChange={(event) => { const selected = paymentMethods.find((method) => method.value === event.target.value); if (selected) setPaymentMethod(selected.value); }}>{paymentMethods.map((method) => <option key={method.value} value={method.value}>{method.label}</option>)}</select></label>{paymentMethod === "cash" && <><label>Cash paid from<select className="text-input" value={cashOrigin} onChange={(event) => setCashOrigin(event.target.value as CashOrigin)}><option value="drawer">This register drawer</option><option value="other_cash">Other cash location</option></select></label>{cashOrigin === "other_cash" && <label>Cash location<select className="text-input" required value={cashLocationId} onChange={(event) => { setCashLocationId(event.target.value); setCashSourceNote(cashLocations.find((row) => row.id === event.target.value)?.name ?? ""); }}><option value="">Choose a counted location</option>{cashLocations.filter((row) => row.locationType !== "register" && row.openingRecorded).map((row) => <option key={row.id} value={row.id}>{row.name} · {formatMoney(row.balanceMinor, user.currencyCode)}</option>)}</select></label>}</>}<label>External reference <span className="expense-optional">Optional</span><input className="text-input" maxLength={100} value={externalReference} onChange={(event) => setExternalReference(event.target.value)} placeholder="Terminal / transfer reference"/></label></div>
      {!eligibleBills.length ? <p className="supplier-billing-empty">Choose a supplier with an open invoice to allocate a payment.</p> : <div className="supplier-billing-allocation-list">{eligibleBills.map((bill) => <label className="supplier-billing-allocation" key={bill.billId}><span><strong>{bill.supplierReference}</strong><small>Due {bill.dueDate || "date not set"} · Outstanding {formatMoney(bill.outstandingMinor, bill.currencyCode)}</small></span><input aria-label={`Payment allocation for invoice ${bill.supplierReference}`} className="text-input" inputMode="decimal" placeholder="0.00" value={paymentAmounts[bill.billId] ?? ""} onChange={(event) => setPaymentAmounts((current) => ({ ...current, [bill.billId]: event.target.value }))}/></label>)}</div>}
      <button className="button button-primary" disabled={busy || loading || !eligibleBills.length}>{busy ? "Saving…" : "Record payment"}</button>
    </form>}
    {!canInvoice && !canSettle && <p className="supplier-billing-empty">Invoice matching and payment recording require separate finance permissions.</p>}
    <div className="supplier-billing-open"><h3>Open supplier invoices</h3>{workspace.openBills.length ? <div className="table-scroll"><table><thead><tr><th>SUPPLIER / INVOICE</th><th>INVOICE DATE</th><th className="align-right">INVOICE TOTAL</th><th className="align-right">PAID</th><th className="align-right">CREDITED</th><th className="align-right">OUTSTANDING</th></tr></thead><tbody>{workspace.openBills.map((bill) => <tr key={bill.billId}><td><strong>{bill.supplierName}</strong><small className="table-subvalue">{bill.supplierReference}</small></td><td>{bill.invoiceDate}</td><td className="align-right">{formatMoney(bill.totalMinor,bill.currencyCode)}</td><td className="align-right">{formatMoney(bill.paidMinor,bill.currencyCode)}</td><td className="align-right">{formatMoney(bill.creditedMinor,bill.currencyCode)}</td><td className="align-right"><strong>{formatMoney(bill.outstandingMinor,bill.currencyCode)}</strong></td></tr>)}</tbody></table></div> : <p className="supplier-billing-empty">{loading ? "Loading balances…" : "No open supplier invoices."}</p>}</div>
  </section>;
}
