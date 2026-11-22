// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useMemo, useState, type FormEvent } from "react";
import { ArrowLeft, ClipboardCheck, Clock3, PackageCheck, Plus, Save, X } from "lucide-react";
import { posApi } from "../lib/api";
import type { StockCountDetails, StockCountSummary } from "../lib/models";
import { useToast } from "./useToast";

function errorMessage(reason: unknown, fallback: string) {
  return typeof reason === "string" ? reason : reason instanceof Error ? reason.message : fallback;
}

function startTitle() {
  return `Stock count · ${new Date().toISOString().slice(0, 16).replace("T", " ")} UTC`;
}

function formatDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? value : new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

export function StockCountDialog({ onClose, onPosted }: { onClose: () => void; onPosted: () => void }) {
  const toast = useToast();
  const [counts, setCounts] = useState<StockCountSummary[]>([]);
  const [details, setDetails] = useState<StockCountDetails | null>(null);
  const [quantities, setQuantities] = useState<Record<string, string>>({});
  const [filter, setFilter] = useState("");
  const [title, setTitle] = useState(startTitle);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function loadCounts() {
    const result = await posApi.stockCounts();
    setCounts(result.counts);
  }

  useEffect(() => {
    let active = true;
    void posApi.stockCounts().then((result) => { if (active) setCounts(result.counts); }).catch((reason: unknown) => {
      if (active) setError(errorMessage(reason, "Stock counts could not be loaded."));
    });
    return () => { active = false; };
  }, []);

  const filteredLines = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    return (details?.lines ?? []).filter((line) => !needle || `${line.productName} ${line.sku}`.toLowerCase().includes(needle));
  }, [details, filter]);
  const completeLineCount = details?.lines.filter((line) => quantities[line.productId]?.trim() !== "").length ?? 0;

  function showDetails(value: StockCountDetails) {
    setDetails(value);
    setFilter("");
    setQuantities(Object.fromEntries(value.lines.map((line) => [line.productId, line.countedQuantity === null ? "" : String(line.countedQuantity)])));
    setError("");
  }

  async function startCount(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError("");
    try {
      const value = await posApi.startStockCount(title);
      showDetails(value);
      await loadCounts();
      toast("success", "Stock count started. Count each product, then save your progress.");
    } catch (reason) {
      const message = errorMessage(reason, "The stock count could not be started."); setError(message); toast("error", message);
    } finally { setBusy(false); }
  }

  function readCountValues(value: StockCountDetails) {
    return value.lines.map((line) => {
      const raw = quantities[line.productId]?.trim() ?? "";
      if (!raw) return { productId: line.productId, countedQuantity: null };
      const parsed = Number(raw);
      return { productId: line.productId, countedQuantity: Number.isSafeInteger(parsed) && parsed >= 0 ? parsed : -1 };
    });
  }

  async function saveProgress() {
    if (!details || details.status !== "draft") return details;
    setBusy(true); setError("");
    try {
      const result = await posApi.saveStockCountCounts({ countId: details.id, expectedRevision: details.revision, lines: readCountValues(details) });
      showDetails(result); await loadCounts(); toast("success", "Count progress saved."); return result;
    } catch (reason) {
      const message = errorMessage(reason, "Count progress could not be saved."); setError(message); toast("error", message); return null;
    } finally { setBusy(false); }
  }

  async function postCount() {
    if (!details || details.status !== "draft") return;
    if (completeLineCount !== details.lines.length) { setError("Enter a quantity for every product before posting this count."); return; }
    if (readCountValues(details).some((line) => line.countedQuantity === -1)) { setError("Use whole numbers from zero to one billion for counted quantities."); return; }
    setBusy(true); setError("");
    try {
      const saved = await posApi.saveStockCountCounts({ countId: details.id, expectedRevision: details.revision, lines: readCountValues(details) });
      showDetails(saved);
      const posted = await posApi.postStockCount({ countId: saved.id, expectedRevision: saved.revision });
      showDetails(posted); await loadCounts(); onPosted(); toast("success", "Stock count posted. Inventory quantities and movement history have been updated.");
    } catch (reason) {
      const message = errorMessage(reason, "The stock count could not be posted."); setError(message); toast("error", message);
      try { showDetails(await posApi.stockCount(details.id)); await loadCounts(); } catch { /* Preserve the original posting error. */ }
    } finally { setBusy(false); }
  }

  return <div className="modal-backdrop stock-count-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget && !busy) onClose(); }}>
    <section className="edit-modal stock-count-dialog" role="dialog" aria-modal="true" aria-labelledby="stockCountTitle">
      <button type="button" className="modal-close" onClick={onClose} aria-label="Close stock counts" disabled={busy}><X size={18}/></button>
      <p className="eyebrow">INVENTORY CONTROL</p>
      <h2 id="stockCountTitle">{details ? details.title : "Stock counts"}</h2>
      <p className="modal-subtitle">{details ? details.status === "draft" ? "Record the physical quantity for each item. Save progress to resume later." : `Posted ${details.postedAt ? formatDate(details.postedAt) : ""}. This count is read-only.` : "Count physical stock and reconcile the difference with recorded inventory."}</p>

      {!details && <>
        <form className="stock-count-start" onSubmit={(event) => void startCount(event)}><label className="field-label" htmlFor="stockCountName">Count name<input className="text-input" id="stockCountName" required maxLength={120} value={title} onChange={(event) => setTitle(event.target.value)}/></label><button className="button button-primary" disabled={busy}><Plus size={16}/>{busy ? "Starting…" : "Start a count"}</button></form>
        <div className="stock-count-history"><div className="stock-count-section-title"><strong>Recent counts</strong><span>{counts.length}</span></div>
          {counts.length === 0 ? <p className="stock-count-empty">No stock counts yet. Start one to record a physical inventory check.</p> : counts.map((count) => <button type="button" className="stock-count-history-row" key={count.id} onClick={() => { setBusy(true); void posApi.stockCount(count.id).then(showDetails).catch((reason: unknown) => { const message = errorMessage(reason, "That count could not be opened."); setError(message); toast("error", message); }).finally(() => setBusy(false)); }} disabled={busy}><span className={`stock-count-state ${count.status}`}>{count.status === "draft" ? <Clock3 size={14}/> : <PackageCheck size={14}/>}</span><span className="stock-count-history-copy"><strong>{count.title}</strong><small>{formatDate(count.startedAt)} · {count.countedCount} of {count.itemCount} counted</small></span><span className={`status-pill ${count.status === "draft" ? "status-pending" : "status-active"}`}><i/>{count.status === "draft" ? "In progress" : "Posted"}</span></button>)}
        </div>
      </>}

      {details && <>
        <div className="stock-count-toolbar"><button className="button button-secondary button-small" type="button" onClick={() => { setDetails(null); setError(""); void loadCounts().catch((reason: unknown) => setError(errorMessage(reason, "Stock counts could not be loaded."))); }} disabled={busy}><ArrowLeft size={14}/> All counts</button><input className="text-input" type="search" value={filter} onChange={(event) => setFilter(event.target.value)} placeholder="Filter by product or SKU" aria-label="Filter counted products"/><span>{completeLineCount}/{details.lines.length} counted</span></div>
        <div className="stock-count-lines-scroll"><table className="stock-count-lines"><thead><tr><th>PRODUCT</th><th>EXPECTED</th><th>COUNTED</th><th>VARIANCE</th></tr></thead><tbody>{filteredLines.map((line) => {
          const raw = quantities[line.productId] ?? "";
          const entered = raw.trim() === "" ? null : Number(raw);
          const variance = entered !== null && Number.isSafeInteger(entered) ? entered - line.expectedQuantity : null;
          return <tr key={line.productId}><td><strong>{line.productName}</strong><small>{line.sku}</small></td><td>{line.expectedQuantity}</td><td><input className="text-input stock-count-quantity" type="number" min="0" max="1000000000" step="1" inputMode="numeric" value={raw} onChange={(event) => setQuantities((current) => ({ ...current, [line.productId]: event.target.value }))} aria-label={`Counted quantity for ${line.productName}`} disabled={busy || details.status !== "draft"}/></td><td className={variance === null ? "" : variance === 0 ? "stock-count-even" : "stock-count-variance"}>{variance === null ? "—" : variance > 0 ? `+${variance}` : variance}</td></tr>;
        })}</tbody></table>{filteredLines.length === 0 && <p className="stock-count-empty">No products match that filter.</p>}</div>
        {error && <p className="form-error" role="alert">{error}</p>}
        {details.status === "draft" && <div className="modal-actions"><button className="button button-secondary" type="button" onClick={() => void saveProgress()} disabled={busy}><Save size={15}/>{busy ? "Saving…" : "Save progress"}</button><button className="button button-primary" type="button" onClick={() => void postCount()} disabled={busy || details.lines.length === 0 || completeLineCount !== details.lines.length}><ClipboardCheck size={15}/>{busy ? "Posting…" : "Post stock count"}</button></div>}
        {details.status === "posted" && <div className="stock-count-posted-note"><PackageCheck size={17}/> Inventory was updated and this count is preserved in history.</div>}
      </>}

      {!details && error && <p className="form-error" role="alert">{error}</p>}
    </section>
  </div>;
}
