// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState } from "react";
import { Banknote, ClipboardList, DoorClosed, DoorOpen, Download, History, TrendingDown, TrendingUp, X } from "lucide-react";
import { posApi } from "../lib/api";
import type { CashLocationOption, DrawerHistoryPage, DrawerReport, DrawerShift } from "../lib/models";
import { formatMoney, parseMoneyMinor } from "../lib/money";
import { downloadDrawerHistoryCsv } from "../lib/drawerHistoryExport";

type DialogMode = "open" | "movement" | "x" | "close" | "closed" | "history" | "historic" | null;
type MovementType = "paid_in" | "paid_out" | "cash_drop";

function amount(value: string) {
  const parsed = Number(value.replace(/,/g, ""));
  return Number.isFinite(parsed) && parsed >= 0 ? Math.round(parsed * 100) : 0;
}

export function CashDrawerControl({ currencyCode, canTransact }: { currencyCode: string; canTransact: boolean }) {
  const [shift, setShift] = useState<DrawerShift | null>(null);
  const [mode, setMode] = useState<DialogMode>(null);
  const [report, setReport] = useState<DrawerReport | null>(null);
  const [history, setHistory] = useState<DrawerHistoryPage | null>(null);
  const [openAmount, setOpenAmount] = useState("");
  const [countedAmount, setCountedAmount] = useState("");
  const [retainedAmount, setRetainedAmount] = useState("");
  const [movementAmount, setMovementAmount] = useState("");
  const [movementNote, setMovementNote] = useState("");
  const [movementType, setMovementType] = useState<MovementType>("paid_in");
  const [cashLocations, setCashLocations] = useState<CashLocationOption[]>([]);
  const [otherLocationId, setOtherLocationId] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [exportingHistory, setExportingHistory] = useState(false);
  const openingAmountMinor = parseMoneyMinor(openAmount);

  useEffect(() => {
    let alive = true;
    void posApi.drawerWorkspace().then((value) => { if (alive) setShift(value); })
      .catch((reason) => { if (alive) setError(typeof reason === "string" ? reason : "Drawer status could not be loaded."); })
      .finally(() => { if (alive) setLoading(false); });
    void posApi.cashLocationOptions().then((rows) => {
      if (!alive) return;
      const available = rows.filter((row) => row.locationType !== "register" && row.openingRecorded);
      setCashLocations(available);
      setOtherLocationId(available[0]?.id ?? "");
    }).catch(() => { if (alive) setCashLocations([]); });
    return () => { alive = false; };
  }, []);

  async function run(action: () => Promise<DrawerShift | DrawerReport>) {
    setBusy(true);
    setError("");
    try {
      const result = await action();
      if ("shiftId" in result) {
        setReport(result);
        if (result.isClosed) {
          setShift(null);
          setMode("closed");
          return;
        }
      } else setShift(result);
      setMode(null);
    } catch (reason) {
      setError(typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "The drawer action failed.");
    } finally {
      setBusy(false);
    }
  }

  async function showX() {
    setBusy(true);
    setError("");
    try { setReport(await posApi.xReport()); setMode("x"); }
    catch (reason) { setError(typeof reason === "string" ? reason : "The X report could not be created."); }
    finally { setBusy(false); }
  }

  async function showClose() {
    setBusy(true);
    setError("");
    try {
      setReport(await posApi.xReport());
      setCountedAmount("");
      setRetainedAmount("");
      setMode("close");
    } catch (reason) {
      setError(typeof reason === "string" ? reason : "The drawer totals could not be loaded.");
    } finally {
      setBusy(false);
    }
  }

  async function showHistory() {
    setBusy(true);
    setError("");
    try { setHistory(await posApi.closedDrawerHistory({ offset: 0, pageSize: 20 })); setMode("history"); }
    catch (reason) { setError(typeof reason === "string" ? reason : "Drawer history could not be loaded."); }
    finally { setBusy(false); }
  }

  async function changeHistoryPage(offset: number) {
    if (!history) return;
    setBusy(true);
    setError("");
    try {
      setHistory(await posApi.closedDrawerHistory({ offset, pageSize: 20, snapshotRowId: history.snapshotRowId }));
    } catch (reason) {
      setError(typeof reason === "string" ? reason : "Drawer history could not be loaded.");
    } finally { setBusy(false); }
  }

  async function exportHistory() {
    if (!history?.matchedCount) return;
    setExportingHistory(true);
    setError("");
    try { await downloadDrawerHistoryCsv({ snapshotRowId: history.snapshotRowId }, currencyCode); }
    catch { setError("The complete drawer history could not be exported. Try again."); }
    finally { setExportingHistory(false); }
  }

  if (loading) return <div className="drawer-status drawer-status-loading">Checking register drawer…</div>;

  return <>
    <div className="drawer-status" aria-live="polite">
      <div className="drawer-status-summary">
        {shift ? <><span className="drawer-live-dot"/><span><strong>Drawer open</strong> · {shift.stationName} · {shift.cashierName} · Expected {formatMoney(shift.expectedCashMinor, currencyCode)}</span></> : <><DoorClosed size={16}/><span><strong>Drawer closed</strong> · Open a cash session before checkout</span></>}
      </div>
      <div className="drawer-status-actions">
        {shift ? <>
          <button type="button" className="button button-small button-secondary" onClick={() => { setError(""); setMode("movement"); }} disabled={!canTransact}><Banknote size={14}/>Cash movement</button>
          <button type="button" className="button button-small button-secondary" onClick={() => void showX()} disabled={busy}><ClipboardList size={14}/>X report</button>
          <button type="button" className="button button-small button-secondary" onClick={() => void showHistory()} disabled={busy}><History size={14}/>History</button>
          <button type="button" className="button button-small button-secondary" onClick={() => void showClose()} disabled={busy || !canTransact}><DoorClosed size={14}/>Close day</button>
        </> : <><button type="button" className="button button-small button-primary" onClick={() => { setError(""); setOpenAmount(""); setMode("open"); }} disabled={!canTransact}><DoorOpen size={14}/>Open drawer</button><button type="button" className="button button-small button-secondary" onClick={() => void showHistory()} disabled={busy}><History size={14}/>History</button></>}
      </div>
    </div>
    {error && <p className="drawer-inline-error" role="alert">{error}</p>}

    {!shift && <div className="drawer-gate" role="status"><div className="drawer-gate-card"><span className="drawer-gate-icon"><DoorOpen size={24}/></span><p className="eyebrow">START REGISTER</p><h2>{canTransact ? "Open your cash drawer" : "Cash drawer is closed"}</h2><p>{canTransact ? "Enter the opening float to start this register session. End of day is closed with a counted cash total and a saved Z report." : "You can review products and prepare a ticket. Enable temporary cashier mode in the top bar to open the drawer or complete checkout."}</p>{error && <p className="form-error" role="alert">{error}</p>}{canTransact && <button className="button button-primary" type="button" onClick={() => { setError(""); setOpenAmount(""); setMode("open"); }}>Open register <DoorOpen size={16}/></button>}<button className="button button-secondary drawer-history-link" type="button" onClick={() => void showHistory()} disabled={busy}><History size={15}/>View closed drawer history</button></div></div>}

    {mode && <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !busy) setMode(null); }}><article className="receipt-modal drawer-modal" role="dialog" aria-modal="true" aria-labelledby="drawer-dialog-title"><button className="modal-close" type="button" onClick={() => setMode(null)} disabled={busy} aria-label="Close drawer dialog"><X size={18}/></button>
      {mode === "open" && <><p className="eyebrow">REGISTER SESSION</p><h2 id="drawer-dialog-title">Open cash drawer</h2><p className="receipt-number">Count the starting float. Only this till’s sales and cash movements will be included.</p><label className="field-label">Opening cash ({currencyCode})<input className="text-input" inputMode="decimal" autoFocus value={openAmount} onChange={(event) => setOpenAmount(event.target.value)} placeholder="0.00" aria-invalid={openAmount.trim() !== "" && openingAmountMinor === null}/></label><small className="settings-hint">First time for this till? A manager, administrator, or owner can use this count to initialize its cash tracking.</small>{error && <p className="form-error">{error}</p>}<button className="button button-primary receipt-done" disabled={busy || openingAmountMinor === null} onClick={() => void run(() => posApi.openDrawer(openingAmountMinor!))}>{busy ? "Opening…" : "Start register"}<DoorOpen size={16}/></button></>}
      {mode === "movement" && <><p className="eyebrow">CASH CONTROL</p><h2 id="drawer-dialog-title">Record cash movement</h2><p className="receipt-number">Transfers update the register and the paired safe together. Paid out records cash leaving the business cash locations.</p><label className="field-label">Movement<select className="text-input" value={movementType} onChange={(event) => setMovementType(event.target.value as MovementType)}><option value="paid_in">Cash paid in from safe</option><option value="paid_out">Cash paid out</option><option value="cash_drop">Cash drop to safe</option></select></label>{movementType !== "paid_out" && <label className="field-label">Paired cash location<select className="text-input" required value={otherLocationId} onChange={(event) => setOtherLocationId(event.target.value)}><option value="">Choose counted location</option>{cashLocations.map((row) => <option key={row.id} value={row.id}>{row.name} · {formatMoney(row.balanceMinor,currencyCode)}</option>)}</select></label>}<label className="field-label">Amount ({currencyCode})<input className="text-input" inputMode="decimal" autoFocus value={movementAmount} onChange={(event) => setMovementAmount(event.target.value)} placeholder="0.00"/></label><label className="field-label">Reason<input className="text-input" value={movementNote} maxLength={200} onChange={(event) => setMovementNote(event.target.value)} placeholder="For example, change float or paid delivery"/></label>{error && <p className="form-error">{error}</p>}<button className="button button-primary receipt-done" disabled={busy || amount(movementAmount) <= 0 || !movementNote.trim() || (movementType !== "paid_out" && !otherLocationId)} onClick={() => void run(() => posApi.recordDrawerMovement({ movementType, amountMinor: amount(movementAmount), note: movementNote.trim(), otherLocationId: movementType === "paid_out" ? null : otherLocationId }))}>{busy ? "Saving…" : "Save movement"}{movementType === "paid_in" ? <TrendingUp size={16}/> : <TrendingDown size={16}/>}</button></>}
      {mode === "history" && <><p className="eyebrow">CASH CONTROL</p><h2 id="drawer-dialog-title">Closed drawer history</h2><div className="drawer-history-toolbar"><span>{history?.matchedCount ? `${history.offset + 1}–${history.offset + (history.rows.length)} of ${history.matchedCount} sessions` : "No closed drawer sessions"}</span><button type="button" className="button button-small button-secondary" disabled={!history?.matchedCount || exportingHistory || busy} onClick={() => void exportHistory()}><Download size={14}/>{exportingHistory ? "Preparing…" : "Export all"}</button></div>{history?.rows.length ? <div className="drawer-history-list">{history.rows.map((entry) => <button key={entry.shiftId} type="button" className="drawer-history-row" onClick={() => { setReport(entry); setMode("historic"); }}><span><strong>{entry.stationName} · {entry.cashierName}</strong><small>{new Intl.DateTimeFormat("en", { dateStyle: "medium", timeStyle: "short" }).format(new Date(entry.closedAt ?? entry.openedAt))} · {entry.completedSaleCount} cash sales</small></span><span><strong>{formatMoney(entry.expectedCashMinor, currencyCode)}</strong><small>Counted {formatMoney(entry.countedCashMinor ?? 0, currencyCode)}</small></span></button>)}</div> : <p className="receipt-number">No closed drawer sessions have been recorded for this store yet.</p>}<div className="drawer-history-pagination"><button type="button" className="button button-secondary" disabled={busy || !history || history.offset === 0} onClick={() => void changeHistoryPage(Math.max(0, (history?.offset ?? 0) - 20))}>Previous</button><button type="button" className="button button-secondary" disabled={busy || !history || history.offset + history.rows.length >= history.matchedCount} onClick={() => void changeHistoryPage((history?.offset ?? 0) + 20)}>Next</button></div>{error && <p className="form-error" role="alert">{error}</p>}<button className="button button-primary receipt-done" onClick={() => setMode(null)}>Done</button></>}
      {(mode === "x" || mode === "close" || mode === "closed" || mode === "historic") && report && <><p className="eyebrow">{mode === "x" ? "IN-SHIFT SNAPSHOT" : mode === "historic" ? "SAVED HISTORY" : "END OF DAY"}</p><h2 id="drawer-dialog-title">{mode === "x" ? "X report" : mode === "historic" ? "Closed drawer report" : "Z close report"}</h2><p className="receipt-number">{report.stationName} · {report.cashierName} · Opened {new Intl.DateTimeFormat("en", { dateStyle: "medium", timeStyle: "short" }).format(new Date(report.openedAt))}</p><div className="drawer-report-rows"><div><span>Opening float</span><strong>{formatMoney(report.openingCashMinor, currencyCode)}</strong></div><div><span>Cash sales · {report.completedSaleCount} sales</span><strong>{formatMoney(report.cashSalesMinor, currencyCode)}</strong></div><div><span>Cash paid in</span><strong>{formatMoney(report.paidInMinor, currencyCode)}</strong></div>{report.expenseReimbursementPaidInMinor > 0 && <div><span>Including returned expenses</span><strong>{formatMoney(report.expenseReimbursementPaidInMinor, currencyCode)}</strong></div>}<div><span>Cash paid out</span><strong>−{formatMoney(report.paidOutMinor, currencyCode)}</strong></div>{report.expensePaidOutMinor > 0 && <div><span>Including expenses from this drawer</span><strong>−{formatMoney(report.expensePaidOutMinor, currencyCode)}</strong></div>}{report.supplierPaymentPaidOutMinor > 0 && <div><span>Including supplier payments from this drawer</span><strong>−{formatMoney(report.supplierPaymentPaidOutMinor, currencyCode)}</strong></div>}<div><span>Cash drops</span><strong>−{formatMoney(report.cashDropsMinor, currencyCode)}</strong></div><div className="drawer-report-total"><span>Expected in drawer</span><strong>{formatMoney(report.expectedCashMinor, currencyCode)}</strong></div></div>{mode === "close" && <><label className="field-label">Counted cash ({currencyCode})<input className="text-input" inputMode="decimal" autoFocus value={countedAmount} onChange={(event) => setCountedAmount(event.target.value)} placeholder="Enter physical cash count"/></label>{countedAmount.trim() && <p className="drawer-difference">Difference: <strong>{formatMoney(amount(countedAmount) - report.expectedCashMinor, currencyCode)}</strong></p>}<label className="field-label">Cash to retain in this till ({currencyCode})<input className="text-input" inputMode="decimal" value={retainedAmount} onChange={(event) => setRetainedAmount(event.target.value)} placeholder="Leave blank to retain all counted cash"/></label><p className="receipt-number">Cash above the retained float is swept to a counted safe or petty cash location. Leave blank to keep all counted cash here.</p>{retainedAmount.trim() && amount(countedAmount) > amount(retainedAmount) && <label className="field-label">Closing cash destination<select className="text-input" required value={otherLocationId} onChange={(event) => setOtherLocationId(event.target.value)}><option value="">Choose a counted safe</option>{cashLocations.map((row) => <option key={row.id} value={row.id}>{row.name}</option>)}</select></label>}{error && <p className="form-error">{error}</p>}<button className="button button-primary receipt-done" disabled={busy || !countedAmount.trim() || (retainedAmount.trim() !== "" && amount(retainedAmount) > amount(countedAmount)) || (retainedAmount.trim() !== "" && amount(countedAmount) > amount(retainedAmount) && !otherLocationId)} onClick={() => void run(() => posApi.closeDrawerZ({ shiftId: report.shiftId, countedCashMinor: amount(countedAmount), closingDestinationLocationId: retainedAmount.trim() && amount(countedAmount) > amount(retainedAmount) ? otherLocationId : null, retainedCashMinor: retainedAmount.trim() ? amount(retainedAmount) : amount(countedAmount) }))}>{busy ? "Closing…" : "Confirm Z close"}<DoorClosed size={16}/></button></>}{(mode === "closed" || mode === "historic") && <><div className="drawer-report-rows"><div><span>Counted cash</span><strong>{formatMoney(report.countedCashMinor ?? 0, currencyCode)}</strong></div><div><span>Difference</span><strong>{formatMoney(report.differenceMinor ?? 0, currencyCode)}</strong></div><div><span>Swept to safe</span><strong>{formatMoney(report.closingTransferMinor ?? 0, currencyCode)}</strong></div><div><span>Retained in till</span><strong>{formatMoney(report.retainedCashMinor ?? report.countedCashMinor ?? 0, currencyCode)}</strong></div></div><button className="button button-primary receipt-done" onClick={() => setMode(mode === "historic" ? "history" : null)}>{mode === "historic" ? "Back to history" : "Done"}</button></>}{mode === "x" && <button className="button button-primary receipt-done" onClick={() => setMode(null)}>Done</button>}</>}
    </article></div>}
  </>;
}
