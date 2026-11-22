// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { ArrowDownLeft, Download, Pencil, Plus, RotateCcw, Search, Wallet, X } from "lucide-react";
import { posApi } from "../lib/api";
import type { CashLocationOption, DrawerShift, ExpenseInput, ExpensePaymentMethod, ExpenseQueryResult, ExpenseReportRow, ExpenseReturnMethod, SessionUser, StationWorkspace, ExpenseCostCenterHistory } from "../lib/models";
import { buildExpenseReportCsv, costCenterLabel, loadCompleteExpenseExport, paymentMethodLabel } from "../lib/expenseReportExport";
import { formatDateTime, formatMoney, parseMoneyMinor } from "../lib/money";

type Period = "all" | "today" | "7d" | "30d";
type CashOrigin = "drawer" | "other_cash";
const pageSize = 50;
const emptyExpenses: ExpenseReportRow[] = [];
const paymentMethods: { value: ExpensePaymentMethod; label: string }[] = [
  { value: "cash", label: "Cash" },
  { value: "mobile_money", label: "Mobile money" },
  { value: "bank_transfer", label: "Bank transfer" },
  { value: "other", label: "Other" },
];

function localDateBoundaryUtc(date: Date, dayOffset: number) {
  const boundary = new Date(date.getFullYear(), date.getMonth(), date.getDate());
  boundary.setDate(boundary.getDate() + dayOffset);
  return boundary.toISOString();
}

export function ExpensesPage({ user }: { user: SessionUser }) {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [query, setQuery] = useState("");
  const [period, setPeriod] = useState<Period>("all");
  const [now, setNow] = useState(() => new Date());
  const [modalOpen, setModalOpen] = useState(false);
  const [reimbursementTarget, setReimbursementTarget] = useState<ExpenseReportRow | null>(null);
  const [reclassificationTarget, setReclassificationTarget] = useState<ExpenseReportRow | null>(null);
  const [reclassificationHistory, setReclassificationHistory] = useState<ExpenseCostCenterHistory | null>(null);
  const [reclassificationKind, setReclassificationKind] = useState<"station_direct" | "store_overhead" | "legacy_unassigned">("store_overhead");
  const [reclassificationStationKey, setReclassificationStationKey] = useState("");
  const [reclassificationReason, setReclassificationReason] = useState("");
  const [reclassificationBusy, setReclassificationBusy] = useState(false);
  const [reimbursementReason, setReimbursementReason] = useState("");
  const [returnReference, setReturnReference] = useState("");
  const [returnAmount, setReturnAmount] = useState("");
  const [returnMethod, setReturnMethod] = useState<ExpenseReturnMethod>("cash");
  const [returnCashLocationId, setReturnCashLocationId] = useState("");
  const [drawerWorkspace, setDrawerWorkspace] = useState<DrawerShift | null>(null);
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState("");
  const [category, setCategory] = useState("");
  const [description, setDescription] = useState("");
  const [amount, setAmount] = useState("");
  const [paymentMethod, setPaymentMethod] = useState<ExpensePaymentMethod>("cash");
  const [cashOrigin, setCashOrigin] = useState<CashOrigin>("other_cash");
  const [cashSourceNote, setCashSourceNote] = useState("");
  const [cashLocationId, setCashLocationId] = useState("");
  const [cashLocations, setCashLocations] = useState<CashLocationOption[]>([]);
  const [stations, setStations] = useState<StationWorkspace["stations"]>([]);
  const [currentStationKey, setCurrentStationKey] = useState("");
  const [costCenterKind, setCostCenterKind] = useState<"station_direct" | "store_overhead">("store_overhead");
  const [costCenterStationKey, setCostCenterStationKey] = useState("");
  const expenseAttempt = useRef<{ fingerprint: string; key: string } | null>(null);
  const reimbursementAttempt = useRef<{ fingerprint: string; key: string } | null>(null);
  const [offset, setOffset] = useState(0);
  const [refreshKey, setRefreshKey] = useState(0);
  const [loaded, setLoaded] = useState<{ key: string; result: ExpenseQueryResult } | null>(null);
  const snapshot = useRef<{ filterKey: string; rowId: number; reimbursementRowId: number; reclassificationRowId: number } | null>(null);
  const canWrite = user.permissions.includes("expenses:write");
  const canReclassify = user.permissions.includes("expenses:reclassify");
  const mayAssignOtherStation = user.isOwner || user.permissions.some((permission) => permission === "settings:write" || permission === "reports:read");
  const canReimburse = canWrite && (user.isOwner || ["owner", "admin", "manager"].includes(user.roleKey));

  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), 60_000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    let active = true;
    void posApi.stationWorkspace().then((workspace) => {
      if (!active) return;
      setStations(workspace.stations);
      setCurrentStationKey(workspace.currentStationKey);
    }).catch(() => {
      if (active) { setStations([]); setCurrentStationKey(""); }
    });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    let active = true;
    void Promise.all([posApi.cashLocationOptions(), posApi.drawerWorkspace()]).then(([rows, shift]) => {
      if (!active) return;
      setCashLocations(rows);
      setDrawerWorkspace(shift);
      setCashLocationId(rows.find((row) => row.locationType !== "register" && row.openingRecorded)?.id ?? "");
    }).catch(() => { if (active) { setCashLocations([]); setDrawerWorkspace(null); } });
    return () => { active = false; };
  }, []);

  const periodStartOffset = period === "today" ? 0 : period === "7d" ? -6 : period === "30d" ? -29 : null;
  const fromUtc = periodStartOffset === null ? undefined : localDateBoundaryUtc(now, periodStartOffset);
  const toUtcExclusive = periodStartOffset === null ? undefined : localDateBoundaryUtc(now, 1);
  const normalizedSearch = query.trim();
  const filterKey = `${normalizedSearch}\u0000${fromUtc ?? ""}\u0000${toUtcExclusive ?? ""}`;
  const requestKey = `${filterKey}\u0000${offset}\u0000${refreshKey}`;
  const result = loaded?.key === requestKey ? loaded.result : null;
  const rows = result?.rows ?? emptyExpenses;
  const categories = useMemo(() => [...new Set(rows.map((expense) => expense.category))].sort((a, b) => a.localeCompare(b)), [rows]);

  useEffect(() => {
    let active = true;
    const snapshotRowId = offset > 0 && snapshot.current?.filterKey === filterKey ? snapshot.current.rowId : undefined;
    const reimbursementSnapshotRowId = offset > 0 && snapshot.current?.filterKey === filterKey ? snapshot.current.reimbursementRowId : undefined;
    const reclassificationSnapshotRowId = offset > 0 && snapshot.current?.filterKey === filterKey ? snapshot.current.reclassificationRowId : undefined;
    void posApi.queryExpenses({ fromUtc, toUtcExclusive, search: normalizedSearch, offset, pageSize, snapshotRowId, reimbursementSnapshotRowId, reclassificationSnapshotRowId })
      .then((response) => {
        if (!active) return;
        snapshot.current = { filterKey, rowId: response.snapshotRowId, reimbursementRowId: response.reimbursementSnapshotRowId, reclassificationRowId: response.reclassificationSnapshotRowId };
        setLoaded({ key: requestKey, result: response });
        setError("");
      })
      .catch(() => { if (active) setError("Could not load expense report data for this store."); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [filterKey, fromUtc, normalizedSearch, offset, refreshKey, requestKey, toUtcExclusive]);

  const totalMinor = result?.totalMinor ?? 0;

  function resetForm() {
    setCategory("");
    setDescription("");
    setAmount("");
    setPaymentMethod("cash");
    setCashOrigin("other_cash");
    setCashSourceNote("");
    setCashLocationId(cashLocations.find((row) => row.locationType !== "register" && row.openingRecorded)?.id ?? "");
    setCostCenterKind("store_overhead");
    setCostCenterStationKey("");
    expenseAttempt.current = null;
    setFormError("");
  }

  function updateSearch(value: string) {
    snapshot.current = null;
    setOffset(0);
    setLoading(true);
    setQuery(value);
  }

  function updatePeriod(value: Period) {
    snapshot.current = null;
    setOffset(0);
    setLoading(true);
    setPeriod(value);
  }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const parsedAmount = parseMoneyMinor(amount);
    if (!category.trim()) { setFormError("Enter a category for this expense."); return; }
    if (parsedAmount === null || parsedAmount <= 0) { setFormError("Enter an amount greater than zero, with up to two decimal places."); return; }

    const details = {
      category: category.trim(),
      description: description.trim() || null,
      amountMinor: parsedAmount,
      paymentMethod,
      cashOrigin: paymentMethod === "cash" ? cashOrigin : "not_applicable" as const,
      cashSourceNote: paymentMethod === "cash" && cashOrigin === "other_cash" ? (cashLocations.find((row) => row.id === cashLocationId)?.name ?? cashSourceNote.trim()) : null,
      cashLocationId: paymentMethod === "cash" && cashOrigin === "other_cash" ? cashLocationId || null : null,
      costCenterKind,
      costCenterStationKey: costCenterKind === "station_direct" ? costCenterStationKey : null,
    };
    if (details.costCenterKind === "station_direct" && !details.costCenterStationKey) { setFormError("Choose the station that incurred this expense."); return; }
    if (details.cashOrigin === "other_cash" && !details.cashLocationId) { setFormError("Choose a tracked safe or petty cash location."); return; }
    const fingerprint = JSON.stringify(details);
    if (expenseAttempt.current?.fingerprint !== fingerprint) expenseAttempt.current = { fingerprint, key: crypto.randomUUID() };
    const input: ExpenseInput = { ...details, idempotencyKey: expenseAttempt.current.key };
    setBusy(true);
    setFormError("");
    try {
      await posApi.saveExpense(input);
      expenseAttempt.current = null;
      snapshot.current = null;
      setOffset(0);
      setLoading(true);
      setRefreshKey((current) => current + 1);
      setModalOpen(false);
      resetForm();
      setError("");
    } catch (reason) {
      setFormError(typeof reason === "string" ? reason : "The expense could not be saved. Check the details and try again.");
    } finally {
      setBusy(false);
    }
  }

  async function saveReimbursement(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!reimbursementTarget) return;
    const parsedAmount = parseMoneyMinor(returnAmount);
    if (parsedAmount === null || parsedAmount <= 0 || parsedAmount > reimbursementTarget.remainingMinor) {
      setFormError(`Enter an amount up to ${formatMoney(reimbursementTarget.remainingMinor, user.currencyCode)}.`);
      return;
    }
    const reason = reimbursementReason.trim();
    const reference = returnReference.trim();
    if (reason.length < 5 || reason.length > 500) { setFormError("Explain the returned payment in 5 to 500 characters."); return; }
    if (returnMethod !== "cash" && (!reference || reference.length > 120)) { setFormError("Enter the returned-payment reference."); return; }
    const destination = returnMethod === "cash" ? cashLocations.find((row) => row.id === returnCashLocationId) : undefined;
    if (returnMethod === "cash" && (!destination || !destination.openingRecorded)) { setFormError("Choose an active, initialized cash location that received the money."); return; }
    if (destination?.locationType === "register" && (!drawerWorkspace || destination.stationKey !== drawerWorkspace.stationKey)) { setFormError("Register cash can only be received at the current register with an open shift."); return; }
    const shiftId = destination?.locationType === "register" ? drawerWorkspace?.id ?? null : null;
    const fingerprint = JSON.stringify({ expenseId: reimbursementTarget.expenseId, amount: parsedAmount, reason, reference: reference || null, method: returnMethod, locationId: destination?.id ?? null, shiftId });
    if (reimbursementAttempt.current?.fingerprint !== fingerprint) reimbursementAttempt.current = { fingerprint, key: crypto.randomUUID() };
    setBusy(true);
    setFormError("");
    try {
      await posApi.reimburseExpense({ expenseId: reimbursementTarget.expenseId, amountMinor: parsedAmount, returnMethod, reason, returnReference: reference || null, cashLocationId: destination?.id ?? null, cashShiftId: shiftId, idempotencyKey: reimbursementAttempt.current.key });
      reimbursementAttempt.current = null;
      setReimbursementTarget(null);
      setReimbursementReason("");
      setReturnReference("");
      setReturnAmount("");
      snapshot.current = null;
      setOffset(0);
      setLoading(true);
      setRefreshKey((current) => current + 1);
      setError("");
    } catch (reason) {
      setFormError(typeof reason === "string" ? reason : "The reimbursement could not be recorded. Check the details and try again.");
    } finally {
      setBusy(false);
    }
  }

  async function openReclassification(expense: ExpenseReportRow) {
    setError("");
    setFormError("");
    setReclassificationTarget(expense);
    setReclassificationKind(expense.costCenterKind);
    setReclassificationStationKey(expense.costCenterStationKey ?? "");
    setReclassificationReason("");
    setReclassificationHistory(null);
    try {
      setReclassificationHistory(await posApi.expenseCostCenterHistory(expense.expenseId));
    } catch {
      setFormError("The expense attribution history could not be loaded. Refresh and try again.");
    }
  }

  async function saveReclassification(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!reclassificationTarget || !reclassificationHistory) return;
    const reason = reclassificationReason.trim();
    if (reason.length < 5 || reason.length > 500) {
      setFormError("Explain the correction in 5 to 500 characters.");
      return;
    }
    if (reclassificationKind === "station_direct" && !reclassificationStationKey) {
      setFormError("Choose the station that incurred this expense.");
      return;
    }
    setReclassificationBusy(true);
    setFormError("");
    try {
      await posApi.reclassifyExpenseCostCenter({
        expenseId: reclassificationTarget.expenseId,
        expectedRevision: reclassificationHistory.currentRevision,
        costCenterKind: reclassificationKind,
        stationKey: reclassificationKind === "station_direct" ? reclassificationStationKey : null,
        reason,
        idempotencyKey: crypto.randomUUID(),
      });
      setReclassificationTarget(null);
      setReclassificationHistory(null);
      snapshot.current = null;
      setOffset(0);
      setLoading(true);
      setRefreshKey((current) => current + 1);
    } catch (reason) {
      setFormError(typeof reason === "string" ? reason : "The attribution correction could not be saved.");
    } finally {
      setReclassificationBusy(false);
    }
  }

  async function exportCsv() {
    if (!result?.matchedCount) return;
    setBusy(true);
    setError("");
    try {
      const exportRows = await loadCompleteExpenseExport({
        query: (input) => posApi.queryExpenses(input),
        filters: { fromUtc, toUtcExclusive, search: normalizedSearch },
      });
      const content = buildExpenseReportCsv({ rows: exportRows, currencyCode: user.currencyCode });
      const url = URL.createObjectURL(new Blob([content], { type: "text/csv;charset=utf-8" }));
      const link = document.createElement("a");
      link.href = url;
      link.download = `advanced-pos-expenses-${new Date().toISOString().slice(0, 10)}.csv`;
      link.click();
      window.setTimeout(() => URL.revokeObjectURL(url), 0);
    } catch {
      setError("The complete matching expense report could not be exported. Try again.");
    } finally {
      setBusy(false);
    }
  }

  return <section className="expense-page">
    <div className="page-heading">
      <div><p className="eyebrow">STORE OPERATIONS</p><h1>Expenses</h1><p className="page-subtitle">Record and review money spent by this store.</p></div>
      {canWrite && <button className="button button-primary" onClick={() => { resetForm(); setModalOpen(true); }}><Plus size={17}/> Record expense</button>}
    </div>

    <div className="expense-summary">
      <div><span className="expense-summary-icon"><Wallet size={17}/></span><span><small>{period === "all" ? "Matching expenses" : "Expenses in period"}</small><strong>{formatMoney(totalMinor, user.currencyCode)}</strong><em>{result?.matchedCount ?? 0} {(result?.matchedCount ?? 0) === 1 ? "record" : "records"}</em></span></div>
      <p>Original expenses stay immutable. Confirmed returns appear as linked entries and reduce net expense.</p>
    </div>

    <article className="panel expense-panel">
      <div className="table-toolbar expense-toolbar">
        <div><h2>Expense register</h2><p>{user.storeName} · {pageSize} records per page</p></div>
        <div className="table-tools">
          <label className="table-search"><Search size={16}/><input value={query} onChange={(event) => updateSearch(event.target.value)} placeholder="Search expenses…" aria-label="Search expenses"/></label>
          <label className="sr-only" htmlFor="expense-period">Date range</label>
          <select id="expense-period" className="expense-filter" value={period} onChange={(event) => updatePeriod(event.target.value as Period)}>
            <option value="all">All dates</option><option value="today">Today</option><option value="7d">Last 7 days</option><option value="30d">Last 30 days</option>
          </select>
          <button className="button button-secondary button-small" onClick={() => void exportCsv()} disabled={!result?.matchedCount || busy || loading}><Download size={15}/>{busy ? "Exporting…" : "Export all"}</button>
        </div>
      </div>
      {error && <p className="expense-error" role="alert">{error}</p>}
      {loading && !result ? <div className="table-empty"><span>Loading expenses…</span></div> : rows.length ? <div className="table-scroll">
        <table className="expense-table"><thead><tr><th>DATE</th><th>CATEGORY</th><th>DESCRIPTION</th><th>PAYMENT</th><th>CASH SOURCE</th><th>COST CENTER</th><th>RECORDED BY</th><th className="align-right">AMOUNT</th>{(canWrite || canReclassify) && <th>ACTION</th>}</tr></thead>
          <tbody>{rows.map((expense) => <tr key={expense.id}>
            <td>{formatDateTime(expense.occurredAt)}{expense.entryType === "reimbursement" && <small className="expense-entry-kind">Reimbursement</small>}</td>
            <td><span className="expense-category">{expense.category}</span></td>
            <td className={expense.description || expense.correctionReason ? "" : "muted-value"}>{expense.description || expense.correctionReason || "—"}{expense.externalReference && <small className="expense-entry-kind">Ref {expense.externalReference}</small>}</td>
            <td><span className="payment-method-mark">{paymentMethodLabel(expense.paymentMethod)}</span></td>
            <td>{expense.cashOrigin === "drawer" ? "Register drawer" : expense.cashOrigin === "other_cash" ? expense.cashSourceNote : expense.cashOrigin === "legacy_unknown" ? "Legacy / unknown" : "—"}</td>
            <td>{costCenterLabel(expense.costCenterKind, expense.costCenterStationNameSnapshot)}</td>
            <td>{expense.recordedBy}</td>
            <td className={`align-right expense-amount ${expense.amountMinor < 0 ? "expense-reversal-amount" : ""}`}>{formatMoney(expense.amountMinor, user.currencyCode)}</td>
            {(canWrite || canReclassify) && <td>{canWrite && canReimburse && expense.entryType === "expense" && expense.canReverse && <button className="button button-secondary button-small" type="button" onClick={() => {
              const currentRegister = cashLocations.find((row) => row.locationType === "register" && row.stationKey === drawerWorkspace?.stationKey && row.openingRecorded);
              const safe = cashLocations.find((row) => row.locationType !== "register" && row.openingRecorded);
              setReimbursementTarget(expense); setReimbursementReason(""); setReturnReference(""); setReturnAmount(formatMoney(expense.remainingMinor, user.currencyCode).replace(/[^0-9.,]/g, ""));
              setReturnMethod("cash"); setReturnCashLocationId(currentRegister?.id ?? safe?.id ?? ""); setFormError(""); reimbursementAttempt.current = null;
            }}><RotateCcw size={14}/>Record return</button>}{canReclassify && expense.entryType === "expense" && <button className="button button-secondary button-small" type="button" onClick={() => void openReclassification(expense)}><Pencil size={14}/>Correct attribution</button>}</td>}
          </tr>)}</tbody>
        </table>
      </div> : <div className="table-empty"><span className="empty-icon"><Wallet size={22}/></span><strong>{query || period !== "all" ? "No expenses match these filters" : "No expenses recorded yet"}</strong><span>{query || period !== "all" ? "Try another search or date range." : "Record rent, utilities, transport, and other store costs here."}</span>{canWrite && !query && period === "all" && <button className="button button-secondary button-small" onClick={() => { resetForm(); setModalOpen(true); }}>Record an expense <Plus size={15}/></button>}</div>}
      {result && result.matchedCount > pageSize && <div className="expense-pagination"><span>Showing {offset + 1}–{offset + rows.length} of {result.matchedCount}</span><div><button className="button button-secondary button-small" disabled={offset === 0 || loading} onClick={() => { setLoading(true); setOffset((current) => Math.max(0, current - pageSize)); }}>Previous</button><button className="button button-secondary button-small" disabled={offset + rows.length >= result.matchedCount || loading} onClick={() => { setLoading(true); setOffset((current) => current + pageSize); }}>Next</button></div></div>}
    </article>
    <footer className="page-footer"><span>EXPENSES <b>·</b> Store level records</span><span>CSV export follows the current search and date filters</span></footer>

    {modalOpen && <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget && !busy) setModalOpen(false); }}>
      <form className="edit-modal expense-modal" onSubmit={(event) => void save(event)}>
        <button type="button" className="modal-close" onClick={() => setModalOpen(false)} aria-label="Close" disabled={busy}><X size={18}/></button>
        <p className="eyebrow">STORE OPERATIONS</p><h2>Record an expense</h2><p className="modal-subtitle">Add the amount and payment method. Entries are kept in the store audit history.</p>
        <div className="form-grid two-col product-form-grid">
          <div className="form-grid-span"><label className="field-label" htmlFor="expense-category">Category</label><input className="text-input" id="expense-category" list="expense-categories" maxLength={80} required autoFocus value={category} onChange={(event) => setCategory(event.target.value)} placeholder="e.g. Utilities"/><datalist id="expense-categories">{categories.map((item) => <option key={item} value={item}/>)}</datalist></div>
          <div className="form-grid-span"><label className="field-label" htmlFor="expense-description">Description <span className="expense-optional">Optional</span></label><input className="text-input" id="expense-description" maxLength={500} value={description} onChange={(event) => setDescription(event.target.value)} placeholder="What was this expense for?"/></div>
          <div><label className="field-label" htmlFor="expense-amount">Amount ({user.currencyCode})</label><input className="text-input" id="expense-amount" inputMode="decimal" required value={amount} onChange={(event) => setAmount(event.target.value)} placeholder="0.00"/></div>
          <div><label className="field-label" htmlFor="expense-payment">Payment method</label><select className="text-input" id="expense-payment" value={paymentMethod} onChange={(event) => setPaymentMethod(event.target.value as ExpensePaymentMethod)}>{paymentMethods.map((method) => <option key={method.value} value={method.value}>{method.label}</option>)}</select></div>
          <div className="form-grid-span"><label className="field-label" htmlFor="expense-cost-center">Expense applies to</label><select className="text-input" id="expense-cost-center" value={costCenterKind === "station_direct" ? costCenterStationKey : "store_overhead"} onChange={(event) => { if (event.target.value === "store_overhead") { setCostCenterKind("store_overhead"); setCostCenterStationKey(""); } else { setCostCenterKind("station_direct"); setCostCenterStationKey(event.target.value); } }}><option value="store_overhead">Store overhead · shared cost</option>{stations.filter((station) => station.isActive && (mayAssignOtherStation || station.stationKey === currentStationKey)).map((station) => <option key={station.stationKey} value={station.stationKey}>{station.displayName}{station.stationKey === currentStationKey ? " · this register" : ""}</option>)}</select><small>This cost center is separate from the cash source selected below. Store-wide costs are not divided between registers.</small></div>
          {paymentMethod === "cash" && <><div><label className="field-label" htmlFor="expense-cash-origin">Cash paid from</label><select className="text-input" id="expense-cash-origin" value={cashOrigin} onChange={(event) => setCashOrigin(event.target.value as CashOrigin)}><option value="drawer">This register drawer</option><option value="other_cash">Other cash location</option></select></div>{cashOrigin === "other_cash" && <div><label className="field-label" htmlFor="expense-cash-source">Cash location</label><select className="text-input" id="expense-cash-source" required value={cashLocationId} onChange={(event) => { setCashLocationId(event.target.value); setCashSourceNote(cashLocations.find((row) => row.id === event.target.value)?.name ?? ""); }}><option value="">Choose a counted location</option>{cashLocations.filter((row) => row.locationType !== "register" && row.openingRecorded).map((row) => <option key={row.id} value={row.id}>{row.name} · {formatMoney(row.balanceMinor, user.currencyCode)}</option>)}</select><small>Cash paid here is included in the tracked location balance.</small></div>}</>}
        </div>
        {formError && <p className="form-error" role="alert">{formError}</p>}
        <div className="modal-actions"><button type="button" className="button button-secondary" onClick={() => setModalOpen(false)} disabled={busy}>Cancel</button><button className="button button-primary" disabled={busy}>{busy ? "Saving…" : "Save expense"}</button></div>
      </form>
    </div>}
    {reimbursementTarget && <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget && !busy) setReimbursementTarget(null); }}>
      <form className="edit-modal expense-modal" onSubmit={(event) => void saveReimbursement(event)}>
        <button type="button" className="modal-close" onClick={() => setReimbursementTarget(null)} aria-label="Close" disabled={busy}><X size={18}/></button>
        <p className="eyebrow">IMMUTABLE EXPENSE CORRECTION</p><h2>Record returned payment</h2>
        <p className="modal-subtitle">Record a return only after funds have arrived. Advanced POS records your confirmation; it does not contact or verify the payment provider.</p>
        <div className="expense-return-summary"><strong>{reimbursementTarget.category}</strong><span>Original {formatMoney(reimbursementTarget.amountMinor, user.currencyCode)} · returned {formatMoney(reimbursementTarget.amountMinor - reimbursementTarget.remainingMinor, user.currencyCode)} · remaining {formatMoney(reimbursementTarget.remainingMinor, user.currencyCode)} · paid by {paymentMethodLabel(reimbursementTarget.paymentMethod)}</span></div>
        <div className="form-grid two-col product-form-grid">
          <div><label className="field-label" htmlFor="expense-return-amount">Amount returned ({user.currencyCode})</label><input id="expense-return-amount" className="text-input" inputMode="decimal" required autoFocus value={returnAmount} onChange={(event) => setReturnAmount(event.target.value)}/></div>
          <div><label className="field-label" htmlFor="expense-return-method">Funds received by</label><select id="expense-return-method" className="text-input" value={returnMethod} onChange={(event) => setReturnMethod(event.target.value as ExpenseReturnMethod)}><option value="cash">Cash</option><option value="card">Card</option><option value="mobile_money">Mobile money</option><option value="bank_transfer">Bank transfer</option><option value="other">Other</option></select></div>
          {returnMethod === "cash" && <div className="form-grid-span"><label className="field-label" htmlFor="expense-return-location">Cash received at</label><select id="expense-return-location" className="text-input" required value={returnCashLocationId} onChange={(event) => setReturnCashLocationId(event.target.value)}><option value="">Choose the receiving location</option>{cashLocations.filter((row) => row.openingRecorded && (row.locationType !== "register" || row.stationKey === drawerWorkspace?.stationKey)).map((row) => <option key={row.id} value={row.id}>{row.name}{row.locationType === "register" ? ` · Current register${drawerWorkspace ? " (shift open)" : " (shift closed)"}` : ` · ${formatMoney(row.balanceMinor, user.currencyCode)}`}</option>)}</select><small>Cash in the current register is added to its open shift. Safe and petty-cash receipts update that location only.</small></div>}
          {returnMethod !== "cash" && <div className="form-grid-span"><label className="field-label" htmlFor="expense-return-reference">Returned-payment reference</label><input id="expense-return-reference" className="text-input" maxLength={120} required value={returnReference} onChange={(event) => setReturnReference(event.target.value)} placeholder="Bank, card, or provider reference"/></div>}
          {returnMethod === "cash" && <div className="form-grid-span"><label className="field-label" htmlFor="expense-return-reference">Reference <span className="expense-optional">Optional</span></label><input id="expense-return-reference" className="text-input" maxLength={120} value={returnReference} onChange={(event) => setReturnReference(event.target.value)} placeholder="Receipt or note reference"/></div>}
          <div className="form-grid-span"><label className="field-label" htmlFor="expense-return-reason">Reason for correction</label><textarea id="expense-return-reason" className="text-input" rows={3} maxLength={500} minLength={5} required value={reimbursementReason} onChange={(event) => setReimbursementReason(event.target.value)} placeholder="Explain why the expense was returned"/></div>
        </div>
        {formError && <p className="form-error" role="alert">{formError}</p>}
        <div className="modal-actions"><button type="button" className="button button-secondary" onClick={() => setReimbursementTarget(null)} disabled={busy}>Cancel</button><button className="button button-primary" disabled={busy}><ArrowDownLeft size={15}/>{busy ? "Saving return…" : "Confirm funds received"}</button></div>
      </form>
    </div>}
    {reclassificationTarget && <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget && !reclassificationBusy) setReclassificationTarget(null); }}>
      <form className="edit-modal expense-modal" onSubmit={(event) => void saveReclassification(event)}>
        <button type="button" className="modal-close" onClick={() => setReclassificationTarget(null)} aria-label="Close" disabled={reclassificationBusy}><X size={18}/></button>
        <p className="eyebrow">AUDITED REPORTING CORRECTION</p><h2>Correct expense attribution</h2>
        <p className="modal-subtitle">This changes which cost center carries the expense and its linked returns in profitability reports. It does not change journals, cash balances, drawer movements, or closed Z reports.</p>
        <div className="expense-return-summary"><strong>{reclassificationTarget.category} · {formatDateTime(reclassificationTarget.occurredAt)}</strong><span>Original {formatMoney(reclassificationTarget.amountMinor, user.currencyCode)} · returned {formatMoney(reclassificationTarget.amountMinor - reclassificationTarget.remainingMinor, user.currencyCode)} · net expense {formatMoney(reclassificationTarget.remainingMinor, user.currencyCode)} · currently {costCenterLabel(reclassificationHistory?.currentKind ?? reclassificationTarget.costCenterKind, reclassificationHistory?.currentStationNameSnapshot ?? reclassificationTarget.costCenterStationNameSnapshot)}</span></div>
        <div className="form-grid two-col product-form-grid">
          <div className="form-grid-span"><label className="field-label" htmlFor="expense-reclass-target">New cost center</label><select id="expense-reclass-target" className="text-input" value={reclassificationKind === "station_direct" ? `station:${reclassificationStationKey}` : reclassificationKind} onChange={(event) => {
            const value = event.target.value;
            if (value.startsWith("station:")) { setReclassificationKind("station_direct"); setReclassificationStationKey(value.slice("station:".length)); }
            else { setReclassificationKind(value as "store_overhead" | "legacy_unassigned"); setReclassificationStationKey(""); }
          }}><option value="store_overhead">Store overhead · shared cost</option><option value="legacy_unassigned">Not allocated to a station</option>{stations.filter((station) => station.isActive).map((station) => <option key={station.stationKey} value={`station:${station.stationKey}`}>{station.displayName}</option>)}</select></div>
          <div className="form-grid-span"><label className="field-label" htmlFor="expense-reclass-reason">Reason for correction</label><textarea id="expense-reclass-reason" className="text-input" rows={3} required minLength={5} maxLength={500} value={reclassificationReason} onChange={(event) => setReclassificationReason(event.target.value)} placeholder="Explain why this expense belongs to the selected cost center"/></div>
        </div>
        {reclassificationHistory && <div className="expense-return-summary"><strong>Linked expense returns</strong>{reclassificationHistory.reimbursements.length ? reclassificationHistory.reimbursements.map((item, index) => <span key={`${item.returnedAt}-${index}`}>{formatDateTime(item.returnedAt)} · {formatMoney(item.amountMinor, user.currencyCode)} · recorded by {item.recordedBy}</span>) : <span>No returns recorded.</span>}</div>}
        {reclassificationHistory?.rows.length ? <div className="expense-return-summary"><strong>Previous attribution corrections</strong>{reclassificationHistory.rows.map((item) => <span key={item.id}>#{item.revision} · {formatDateTime(item.recordedAt)} · {costCenterLabel(item.previousKind, item.previousStationNameSnapshot)} → {costCenterLabel(item.nextKind, item.nextStationNameSnapshot)} · {item.reason} · {item.actorName}</span>)}</div> : null}
        {formError && <p className="form-error" role="alert">{formError}</p>}
        <div className="modal-actions"><button type="button" className="button button-secondary" onClick={() => setReclassificationTarget(null)} disabled={reclassificationBusy}>Cancel</button><button className="button button-primary" disabled={reclassificationBusy || !reclassificationHistory}>{reclassificationBusy ? "Saving correction…" : "Confirm correction"}</button></div>
      </form>
    </div>}
  </section>;
}
