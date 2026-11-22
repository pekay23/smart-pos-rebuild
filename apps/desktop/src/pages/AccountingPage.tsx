// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useMemo, useState } from "react";
import { AlertTriangle, BookOpen, Download, Wallet } from "lucide-react";
import { posApi } from "../lib/api";
import type { AccountingPeriodClosure, CashOpeningBalance, SessionUser, TrialBalance } from "../lib/models";
import { formatMoney, parseMoneyMinor } from "../lib/money";

function dateInputValue(date: Date) {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function initialRange() {
  const today = new Date();
  const start = new Date(today.getFullYear(), today.getMonth(), 1);
  return { from: dateInputValue(start), to: dateInputValue(today) };
}

function csvCell(value: string | number) {
  const text = String(value);
  const safe = /^[\s]*[=+\-@]/.test(text) ? `'${text}` : text;
  return `"${safe.replaceAll('"', '""')}"`;
}

function closeSnapshot(closure: AccountingPeriodClosure): { trialBalance: TrialBalance } | null {
  try {
    return JSON.parse(closure.snapshotJson) as { trialBalance: TrialBalance };
  } catch {
    return null;
  }
}

export function AccountingPage({ user }: { user: SessionUser }) {
  const [initial] = useState(initialRange);
  const [from, setFrom] = useState(initial.from);
  const [to, setTo] = useState(initial.to);
  const [result, setResult] = useState<TrialBalance | null>(null);
  const [loadedKey, setLoadedKey] = useState("");
  const [loadError, setLoadError] = useState<{ key: string; message: string } | null>(null);
  const [exporting, setExporting] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const [cashOpening, setCashOpening] = useState<CashOpeningBalance | null>(null);
  const [openingAmount, setOpeningAmount] = useState("");
  const [openingDate, setOpeningDate] = useState(() => dateInputValue(new Date()));
  const [openingReason, setOpeningReason] = useState("Initial cash on hand at Advanced POS cutover");
  const [openingLoading, setOpeningLoading] = useState(user.isOwner);
  const [openingSaving, setOpeningSaving] = useState(false);
  const [openingError, setOpeningError] = useState("");
  const [openingMessage, setOpeningMessage] = useState("");
  const [closures, setClosures] = useState<AccountingPeriodClosure[]>([]);
  const [closureLoading, setClosureLoading] = useState(true);
  const [closureSaving, setClosureSaving] = useState(false);
  const [closureError, setClosureError] = useState("");
  const [closureMessage, setClosureMessage] = useState("");
  const [selectedClosureId, setSelectedClosureId] = useState<string | null>(null);
  const validRange = Boolean(from && to && from <= to);
  const key = `${from}\u0000${to}\u0000${refresh}`;
  const current = loadedKey === key ? result : null;
  const error = loadError?.key === key ? loadError.message : "";
  const loading = validRange && loadedKey !== key && !error;
  const canClosePeriod = user.permissions.includes("accounting:close");

  useEffect(() => {
    if (!validRange) return;
    let active = true;
    void posApi.queryTrialBalance({
      fromDate: from,
      toDateInclusive: to,
    }).then((next) => {
      if (!active) return;
      setResult(next);
      setLoadedKey(key);
    }).catch((reason) => {
      if (active) setLoadError({ key, message: typeof reason === "string" ? reason : "The accounting journal could not be loaded." });
    });
    return () => { active = false; };
  }, [from, key, to, validRange]);

  useEffect(() => {
    let active = true;
    void posApi.accountingPeriodClosures().then((rows) => {
      if (active) setClosures(rows);
    }).catch((reason) => {
      if (active) setClosureError(typeof reason === "string" ? reason : "Closed accounting periods could not be loaded.");
    }).finally(() => { if (active) setClosureLoading(false); });
    return () => { active = false; };
  }, [user.businessId, user.storeId]);

  useEffect(() => {
    if (!user.isOwner) return;
    let active = true;
    void posApi.cashOpeningBalance().then((balance) => {
      if (active) setCashOpening(balance);
    }).catch((reason) => {
      if (active) setOpeningError(typeof reason === "string" ? reason : "Opening cash setup could not be loaded.");
    }).finally(() => { if (active) setOpeningLoading(false); });
    return () => { active = false; };
  }, [user.isOwner]);

  async function saveOpeningBalance() {
    const amountMinor = parseMoneyMinor(openingAmount);
    if (amountMinor === null || amountMinor <= 0 || !openingDate || openingReason.trim().length < 3) {
      setOpeningError("Enter a valid positive amount, effective date, and reason.");
      return;
    }
    if (!window.confirm("This records the physical cash already held by this store at the selected cutover date. It is a one-time opening balance and cannot be edited. Continue?")) return;
    setOpeningSaving(true);
    setOpeningError("");
    setOpeningMessage("");
    try {
      const saved = await posApi.setCashOpeningBalance({
        amountMinor,
        effectiveAt: new Date(`${openingDate}T00:00:00`).toISOString(),
        reason: openingReason.trim(),
        idempotencyKey: crypto.randomUUID(),
      });
      setCashOpening(saved);
      setOpeningMessage("Opening cash balance recorded in the accounting journal.");
      setOpeningAmount("");
      setRefresh((value) => value + 1);
    } catch (reason) {
      setOpeningError(typeof reason === "string" ? reason : "Opening cash balance could not be saved.");
    } finally {
      setOpeningSaving(false);
    }
  }

  const unpostedCount = useMemo(() => current
    ? current.coverage.unpostedSalesCount + current.coverage.unpostedRefundsCount + current.coverage.unpostedExpensesCount + current.coverage.unpostedPurchasesCount + current.coverage.unpostedSupplierBillsCount + current.coverage.unpostedSupplierPaymentsCount
    : 0, [current]);

  function exportCsv() {
    if (!current) return;
    setExporting(true);
    try {
      const rows = [
        ["Account code", "Account", "Type", "Debits", "Credits", "Net debit balance", "Journal entries", "Currency"].map(csvCell).join(","),
        ...current.rows.map((row) => [
          row.code, row.name, row.accountType, row.debitMinor, row.creditMinor, row.balanceMinor, row.journalCount, user.currencyCode,
        ].map(csvCell).join(",")),
        ["TOTAL", "", "", current.totalDebitsMinor, current.totalCreditsMinor, current.totalDebitsMinor - current.totalCreditsMinor, "", user.currencyCode].map(csvCell).join(","),
      ];
      const url = URL.createObjectURL(new Blob([rows.join("\r\n")], { type: "text/csv;charset=utf-8" }));
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `advanced-pos-trial-balance-${from}-to-${to}.csv`;
      anchor.click();
      window.setTimeout(() => URL.revokeObjectURL(url), 0);
    } catch {
      setLoadError({ key, message: "The trial balance could not be exported." });
    } finally {
      setExporting(false);
    }
  }

  async function closePeriod() {
    if (!current || !window.confirm("Finalize this store accounting period? The app will save an immutable report snapshot and reject later entries dated inside the closed range.")) return;
    setClosureSaving(true);
    setClosureError("");
    setClosureMessage("");
    try {
      const closure = await posApi.closeAccountingPeriod({
        fromDate: from,
        toDateInclusive: to,
      });
      setClosures(await posApi.accountingPeriodClosures());
      setSelectedClosureId(closure.id);
      setClosureMessage("Accounting period closed. The saved snapshot is available below.");
    } catch (reason) {
      setClosureError(typeof reason === "string" ? reason : "The accounting period could not be closed.");
    } finally {
      setClosureSaving(false);
    }
  }

  const selectedClosure = closures.find((closure) => closure.id === selectedClosureId) ?? null;
  const selectedSnapshot = selectedClosure ? closeSnapshot(selectedClosure) : null;

  return <section className="accounting-page">
    <div className="page-heading">
      <div><p className="eyebrow">FINANCE</p><h1>Accounting journal</h1><p className="page-subtitle">Double-entry activity posted from store transactions and supplier settlements.</p></div>
      <button className="button button-secondary" type="button" onClick={exportCsv} disabled={!current || exporting}><Download size={17}/>{exporting ? "Preparing CSV…" : "Export trial balance"}</button>
    </div>

    <article className="panel accounting-period">
      <div><strong>Trial balance · period movement</strong><small>Each posted source transaction balances in the business currency.</small></div>
      <label>From<input type="date" value={from} max={to} onChange={(event) => setFrom(event.target.value)}/></label>
      <label>To<input type="date" value={to} min={from} onChange={(event) => setTo(event.target.value)}/></label>
      {canClosePeriod && <button className="button button-primary" type="button" onClick={() => void closePeriod()} disabled={!current || closureSaving}>{closureSaving ? "Closing period…" : "Close accounting period"}</button>}
    </article>
    {closureMessage && <p className="settings-success" role="status">{closureMessage}</p>}
    {closureError && <p className="form-error" role="alert">{closureError}</p>}

    {user.isOwner && <article className="panel cash-opening-panel">
      <div className="table-toolbar"><div><h2><Wallet size={16}/> Store cash cutover</h2><p>Record physical cash already held when starting Advanced POS accounting for this store.</p></div></div>
      {cashOpening ? <div className="cash-opening-record"><strong>{formatMoney(cashOpening.amountMinor, cashOpening.currencyCode)}</strong><span>Effective {new Date(cashOpening.effectiveAt).toLocaleDateString()} · recorded {new Date(cashOpening.recordedAt).toLocaleString()}</span><small>{cashOpening.reason}</small></div> : <>
        <p className="cash-opening-guidance">This one-time entry debits cash on hand and credits opening balance equity. Choose a cutoff before the store’s first cash-on-hand journal activity; the system blocks cutoffs that would duplicate earlier cash transactions. Register shifts remain operational records and are not added to the general ledger by opening a drawer.</p>
        <div className="cash-opening-fields">
          <label>Cash physically on hand<input className="text-input" inputMode="decimal" value={openingAmount} onChange={(event) => setOpeningAmount(event.target.value)} placeholder="0.00" disabled={openingLoading || openingSaving}/></label>
          <label>Effective date<input className="text-input" type="date" value={openingDate} onChange={(event) => setOpeningDate(event.target.value)} disabled={openingLoading || openingSaving}/></label>
          <label>Reason<input className="text-input" value={openingReason} maxLength={200} onChange={(event) => setOpeningReason(event.target.value)} disabled={openingLoading || openingSaving}/></label>
          <button className="button button-primary" type="button" onClick={() => void saveOpeningBalance()} disabled={openingLoading || openingSaving || !openingAmount}>{openingSaving ? "Recording…" : "Record opening balance"}</button>
        </div>
      </>}
      {openingMessage && <p className="settings-success" role="status">{openingMessage}</p>}
      {openingError && <p className="form-error" role="alert">{openingError}</p>}
      {openingLoading && <p className="settings-hint">Checking store cutover status…</p>}
    </article>}

    {!validRange && <p className="form-error" role="alert">Choose a valid date range.</p>}
    {error && <p className="form-error" role="alert">{error}</p>}

    <div className="accounting-metrics">
      <article className="panel"><small>Total debits</small><strong>{current ? formatMoney(current.totalDebitsMinor, user.currencyCode) : loading ? "Loading…" : "—"}</strong></article>
      <article className="panel"><small>Total credits</small><strong>{current ? formatMoney(current.totalCreditsMinor, user.currencyCode) : loading ? "Loading…" : "—"}</strong></article>
      <article className="panel"><small>Journal balance</small><strong>{current ? current.isBalanced ? "Balanced" : "Needs review" : loading ? "Loading…" : "—"}</strong></article>
      <article className="panel"><small>Entries requiring coverage review</small><strong>{current ? current.coverage.partialJournalCount : loading ? "Loading…" : "—"}</strong></article>
    </div>

    {current && (unpostedCount > 0 || current.coverage.partialJournalCount > 0) && <aside className="accounting-notice" role="status"><AlertTriangle size={18}/><div><strong>Partial local ledger coverage</strong><span>{current.coverage.unpostedSalesCount} sales, {current.coverage.unpostedRefundsCount} refunds, {current.coverage.unpostedExpensesCount} expenses, {current.coverage.unpostedPurchasesCount} purchases, {current.coverage.unpostedSupplierBillsCount} supplier bills, and {current.coverage.unpostedSupplierPaymentsCount} supplier payments are not posted in this period. Partial entries may include unmatched supplier receipts, unclassified tender or tax, or missing saved cost.</span></div></aside>}

    <article className="panel accounting-table-panel">
      <div className="table-toolbar"><div><h2>Account activity</h2><p>{current?.rows.length ?? 0} standard accounts · {from} to {to}</p></div><button type="button" className="button button-secondary button-small" onClick={() => setRefresh((value) => value + 1)} disabled={loading}>Refresh</button></div>
      <div className="table-scroll"><table><thead><tr><th>CODE</th><th>ACCOUNT</th><th>TYPE</th><th className="align-right">DEBITS</th><th className="align-right">CREDITS</th><th className="align-right">NET DEBIT</th><th className="align-right">ENTRIES</th></tr></thead><tbody>
        {current?.rows.map((row) => <tr key={row.code}><td className="mono-value">{row.code}</td><td><strong>{row.name}</strong></td><td>{row.accountType.replaceAll("_", " ")}</td><td className="align-right">{formatMoney(row.debitMinor, user.currencyCode)}</td><td className="align-right">{formatMoney(row.creditMinor, user.currencyCode)}</td><td className="align-right">{formatMoney(row.balanceMinor, user.currencyCode)}</td><td className="align-right">{row.journalCount}</td></tr>)}
        {current && <tr className="accounting-total-row"><td colSpan={3}><strong>Total movement</strong></td><td className="align-right"><strong>{formatMoney(current.totalDebitsMinor, user.currencyCode)}</strong></td><td className="align-right"><strong>{formatMoney(current.totalCreditsMinor, user.currencyCode)}</strong></td><td className="align-right"><strong>{formatMoney(current.totalDebitsMinor - current.totalCreditsMinor, user.currencyCode)}</strong></td><td/></tr>}
        {!current && <tr><td colSpan={7}><div className="table-empty"><span className="empty-icon"><BookOpen size={22}/></span><strong>{error || (loading ? "Loading accounting activity…" : "No journal activity for this period")}</strong><span>{error ? "Check the local store and try again." : "Complete sales or record expenses to create accounting entries."}</span></div></td></tr>}
      </tbody></table></div>
    </article>

    <article className="panel accounting-table-panel">
      <div className="table-toolbar"><div><h2>Closed accounting periods</h2><p>Immutable local snapshots for this store · tamper-evident checksum</p></div></div>
      {closureLoading ? <p className="settings-hint">Loading closed periods…</p> : closures.length === 0 ? <p className="settings-hint">No accounting periods have been closed.</p> : <div className="table-scroll"><table><thead><tr><th>PERIOD (UTC)</th><th>CLOSED</th><th>BY</th><th>SNAPSHOT SHA-256</th><th/></tr></thead><tbody>{closures.map((closure) => <tr key={closure.id}><td>{new Date(closure.fromUtc).toLocaleString()} – {new Date(closure.toUtcExclusive).toLocaleString()}<small>{closure.timezone}</small></td><td>{new Date(closure.closedAt).toLocaleString()}</td><td>{closure.closedBy}</td><td className="mono-value">{closure.snapshotSha256.slice(0, 16)}…</td><td><button className="button button-secondary button-small" type="button" onClick={() => setSelectedClosureId(selectedClosureId === closure.id ? null : closure.id)}>{selectedClosureId === closure.id ? "Hide snapshot" : "View snapshot"}</button></td></tr>)}</tbody></table></div>}
      {selectedClosure && <div className="accounting-close-snapshot"><h3>Saved snapshot</h3><p>SHA-256: <code>{selectedClosure.snapshotSha256}</code></p>{selectedSnapshot ? <><div className="accounting-metrics"><article className="panel"><small>Debits</small><strong>{formatMoney(selectedSnapshot.trialBalance.totalDebitsMinor, selectedClosure.currencyCode)}</strong></article><article className="panel"><small>Credits</small><strong>{formatMoney(selectedSnapshot.trialBalance.totalCreditsMinor, selectedClosure.currencyCode)}</strong></article><article className="panel"><small>Status</small><strong>{selectedSnapshot.trialBalance.isBalanced ? "Balanced" : "Needs review"}</strong></article></div><div className="table-scroll"><table><thead><tr><th>CODE</th><th>ACCOUNT</th><th>DEBITS</th><th>CREDITS</th></tr></thead><tbody>{selectedSnapshot.trialBalance.rows.map((row) => <tr key={row.code}><td>{row.code}</td><td>{row.name}</td><td>{formatMoney(row.debitMinor, selectedClosure.currencyCode)}</td><td>{formatMoney(row.creditMinor, selectedClosure.currencyCode)}</td></tr>)}</tbody></table></div></> : <p className="form-error">This saved snapshot could not be read.</p>}</div>}
    </article>

    <p className="accounting-disclaimer">This local management ledger and period close are not certified financial statements or a tax return. New supplier receipts are recorded to a temporary GRNI liability until the supplier invoice is matched. Historical receipts remain unposted because their settlement status is unknown. Store cash cutover is recorded once by the owner; physical cash locations, till/safe transfers, source correction workflows, and certified tax invoicing are not yet covered.</p>
    <footer className="page-footer"><span>ACCOUNTING <b>·</b> Store-scoped local journal</span><span>Transactions remain the operational source of truth</span></footer>
  </section>;
}
