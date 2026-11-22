// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState } from "react";
import { ArrowRightLeft, Check, Plus, Wallet } from "lucide-react";
import { posApi } from "../lib/api";
import { formatMoney, parseMoneyMinor } from "../lib/money";
import type { CashLocationWorkspace } from "../lib/models";

function todayUtc() { return new Date().toISOString(); }

export function CashLocationSettingsPanel() {
  const [workspace, setWorkspace] = useState<CashLocationWorkspace | null>(null);
  const [name, setName] = useState("");
  const [locationType, setLocationType] = useState<"safe" | "petty_cash" | "other">("safe");
  const [openingLocationId, setOpeningLocationId] = useState("");
  const [openingAmount, setOpeningAmount] = useState("");
  const [openingReason, setOpeningReason] = useState("Initial physical cash count");
  const [fromId, setFromId] = useState("");
  const [toId, setToId] = useState("");
  const [transferAmount, setTransferAmount] = useState("");
  const [transferNote, setTransferNote] = useState("");
  const [reviewNotes, setReviewNotes] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");

  async function refresh() {
    const value = await posApi.cashLocationWorkspace();
    setWorkspace(value);
    setOpeningLocationId((current) => current || value.locations.find((row) => !row.openingRecorded)?.id || "");
    setFromId((current) => current || value.locations.find((row) => row.openingRecorded && row.balanceMinor > 0)?.id || "");
    setToId((current) => current || value.locations.find((row) => row.id !== fromId)?.id || "");
  }

  useEffect(() => {
    let active = true;
    void posApi.cashLocationWorkspace().then((value) => {
      if (!active) return;
      setWorkspace(value);
      setOpeningLocationId(value.locations.find((row) => !row.openingRecorded)?.id ?? "");
      setFromId(value.locations.find((row) => row.openingRecorded && row.balanceMinor > 0)?.id ?? "");
      setToId(value.locations.find((row) => row.id !== value.locations.find((item) => item.openingRecorded && item.balanceMinor > 0)?.id)?.id ?? "");
    }).catch((reason) => {
      if (active) setError(typeof reason === "string" ? reason : "Cash locations could not be loaded.");
    });
    return () => { active = false; };
  }, []);

  async function run(action: () => Promise<CashLocationWorkspace>, success: string) {
    setBusy(true); setError(""); setMessage("");
    try { setWorkspace(await action()); setMessage(success); }
    catch (reason) { setError(typeof reason === "string" ? reason : "Cash location update failed."); }
    finally { setBusy(false); }
  }

  const openingMinor = parseMoneyMinor(openingAmount);
  const transferMinor = parseMoneyMinor(transferAmount);
  async function reviewVariance(eventId: string) {
    const note = reviewNotes[eventId]?.trim() ?? "";
    if (note.length < 3) { setError("Enter a short note explaining how the cash count was reviewed."); return; }
    await run(() => posApi.reviewCashLocationVariance({ eventId, note }), "Variance review recorded. The cash count and saved Z report stay unchanged.");
  }
  return <article className="settings-panel">
    <div className="settings-panel-heading"><span className="settings-icon settings-icon-blue"><Wallet size={19}/></span><div><h2>Cash locations</h2><p>Track register tills, safes and petty cash from counted opening balances and recorded movements.</p></div></div>
    <div className="cash-location-reconcile">
      <div><small>Tracked physical cash</small><strong>{workspace ? formatMoney(workspace.trackedCashMinor, workspace.locations[0]?.currencyCode ?? "GHS") : "…"}</strong></div>
      <div><small>Ledger cash control</small><strong>{workspace ? formatMoney(workspace.ledgerCashMinor, workspace.locations[0]?.currencyCode ?? "GHS") : "…"}</strong></div>
      <div><small>Unreconciled difference</small><strong>{workspace ? formatMoney(workspace.varianceMinor, workspace.locations[0]?.currencyCode ?? "GHS") : "…"}</strong></div>
    </div>
    <p className="settings-hint">Physical location counts are operational records. They do not post another accounting opening balance. Existing unassigned cash history stays unassigned.</p>
    <div className="cash-location-list" aria-label="Cash location balances">
      {workspace?.locations.map((location) => <div className="cash-location-row" key={location.id}><span><strong>{location.name}</strong><small>{location.locationType.replace("_", " ")} · {location.openingRecorded ? "Opening count recorded" : "Needs initial count"}{location.stationKey ? ` · Station ${location.stationKey}` : ""}</small></span><strong>{formatMoney(location.balanceMinor, location.currencyCode)}</strong></div>)}
      {!workspace && <p className="settings-hint">Loading cash locations…</p>}
    </div>
    {workspace?.pendingVariances.length ? <div className="cash-variance-list"><h3>Cash count differences to review</h3>{workspace.pendingVariances.map((variance) => <div className="cash-variance-row" key={variance.eventId}><div><strong>{variance.locationName} · {variance.varianceType === "close_variance" ? "Z close" : "Shift opening"}</strong><small>{variance.note} · {variance.direction === "in" ? "Over" : "Short"} {formatMoney(variance.amountMinor, workspace.locations[0]?.currencyCode ?? "GHS")}</small></div><div className="cash-location-fields"><input className="text-input" maxLength={200} value={reviewNotes[variance.eventId] ?? ""} onChange={(event) => setReviewNotes((current) => ({ ...current, [variance.eventId]: event.target.value }))} placeholder="Review note" aria-label={`Review note for ${variance.locationName}`}/><button className="button button-secondary" disabled={busy || (reviewNotes[variance.eventId]?.trim().length ?? 0) < 3} onClick={() => void reviewVariance(variance.eventId)}>Mark reviewed</button></div></div>)}</div> : null}
    <form className="cash-location-form" onSubmit={(event) => { event.preventDefault(); void run(() => posApi.createCashLocation({ name, locationType }), "Cash location added.").then(() => { setName(""); void refresh(); }); }}>
      <h3>Add a location</h3><div className="cash-location-fields"><input className="text-input" value={name} maxLength={80} onChange={(event) => setName(event.target.value)} placeholder="For example, Main safe" aria-label="Cash location name" required/><select className="text-input" value={locationType} onChange={(event) => setLocationType(event.target.value as typeof locationType)} aria-label="Cash location type"><option value="safe">Safe</option><option value="petty_cash">Petty cash</option><option value="other">Other cash</option></select><button className="button button-primary" type="submit" disabled={busy || !name.trim()}><Plus size={15}/>Add</button></div>
    </form>
    <form className="cash-location-form" onSubmit={(event) => { event.preventDefault(); if (openingMinor === null) return; void run(() => posApi.recordCashLocationOpening({ locationId: openingLocationId, amountMinor: openingMinor, effectiveAt: todayUtc(), reason: openingReason, idempotencyKey: crypto.randomUUID() }), "Opening count recorded.").then(() => setOpeningAmount("")); }}>
      <h3>Record a one-time opening count</h3><p className="settings-hint">Count each existing physical location once. New shift opening floats must come from its tracked till balance or a transfer.</p><div className="cash-location-fields"><select className="text-input" value={openingLocationId} onChange={(event) => setOpeningLocationId(event.target.value)} aria-label="Location for opening count"><option value="">Choose a location</option>{workspace?.locations.filter((row) => !row.openingRecorded).map((row) => <option key={row.id} value={row.id}>{row.name}</option>)}</select><input className="text-input" inputMode="decimal" value={openingAmount} onChange={(event) => setOpeningAmount(event.target.value)} placeholder="Counted amount" aria-label="Opening cash amount" required/><input className="text-input" value={openingReason} onChange={(event) => setOpeningReason(event.target.value)} maxLength={200} aria-label="Opening count reason" required/><button className="button button-primary" type="submit" disabled={busy || !openingLocationId || openingMinor === null || openingMinor < 0}>Record count</button></div>
    </form>
    <form className="cash-location-form" onSubmit={(event) => { event.preventDefault(); if (transferMinor === null) return; void run(() => posApi.transferCashLocations({ fromLocationId: fromId, toLocationId: toId, amountMinor: transferMinor, note: transferNote, idempotencyKey: crypto.randomUUID() }), "Transfer recorded. Syncing both location balances.").then(() => { setTransferAmount(""); setTransferNote(""); void refresh(); }); }}>
      <h3><ArrowRightLeft size={16}/> Transfer cash</h3><div className="cash-location-fields"><select className="text-input" value={fromId} onChange={(event) => setFromId(event.target.value)} aria-label="Transfer from">{workspace?.locations.filter((row) => row.openingRecorded).map((row) => <option key={row.id} value={row.id}>{row.name} · {formatMoney(row.balanceMinor, row.currencyCode)}</option>)}</select><select className="text-input" value={toId} onChange={(event) => setToId(event.target.value)} aria-label="Transfer to">{workspace?.locations.filter((row) => row.id !== fromId && row.openingRecorded).map((row) => <option key={row.id} value={row.id}>{row.name}</option>)}</select><input className="text-input" inputMode="decimal" value={transferAmount} onChange={(event) => setTransferAmount(event.target.value)} placeholder="Amount" aria-label="Transfer amount" required/><input className="text-input" value={transferNote} onChange={(event) => setTransferNote(event.target.value)} maxLength={200} placeholder="Reason" aria-label="Transfer note" required/><button className="button button-secondary" type="submit" disabled={busy || !fromId || !toId || fromId === toId || transferMinor === null || transferMinor <= 0 || !transferNote.trim()}>Transfer</button></div>
    </form>
    {message && <p className="settings-success" role="status"><Check size={16}/>{message}</p>}{error && <p className="form-error" role="alert">{error}</p>}
  </article>;
}
