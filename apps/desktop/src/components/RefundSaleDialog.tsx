// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useMemo, useRef, useState } from "react";
import { Check, RotateCcw, X } from "lucide-react";
import { posApi } from "../lib/api";
import type { RefundReceipt, SaleRefundDetail, SessionUser } from "../lib/models";
import { formatDateTime, formatMoney } from "../lib/money";
import "./refunds.css";

type RefundMethod = "cash" | "card" | "mobile_money" | "other";

export function RefundSaleDialog({
  saleId,
  user,
  onClose,
  onUpdated,
}: {
  saleId: string;
  user: SessionUser;
  onClose: () => void;
  onUpdated: () => void;
}) {
  const canRefund = user.permissions.includes("sales:refund");
  const [detail, setDetail] = useState<SaleRefundDetail | null>(null);
  const [quantities, setQuantities] = useState<Record<string, number>>({});
  const [method, setMethod] = useState<RefundMethod>("cash");
  const [reference, setReference] = useState("");
  const [reason, setReason] = useState("");
  const [kind, setKind] = useState<"refund" | "void">("refund");
  const [result, setResult] = useState<RefundReceipt | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const idempotencyKey = useRef("");

  async function reload() {
    const next = await posApi.saleRefundDetail(saleId);
    setDetail(next);
    setQuantities(Object.fromEntries(next.items.map((item) => [item.saleItemId, 0])));
  }

  useEffect(() => {
    let active = true;
    void posApi.saleRefundDetail(saleId).then((next) => {
      if (!active) return;
      setDetail(next);
      setQuantities(Object.fromEntries(next.items.map((item) => [item.saleItemId, 0])));
    }).catch((cause) => {
      if (active) setError(typeof cause === "string" ? cause : "Sale details could not be loaded.");
    }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [saleId]);

  const refundTotal = useMemo(() => {
    if (!detail) return 0;
    return detail.items.reduce((sum, item) => {
      const quantity = Math.max(0, Math.min(item.remainingQuantity, quantities[item.saleItemId] ?? 0));
      if (!quantity) return sum;
      const cumulativeBefore = BigInt(item.refundedQuantity);
      const cumulativeAfter = cumulativeBefore + BigInt(quantity);
      const sold = BigInt(item.soldQuantity);
      const allocated = (minor: number) => {
        const value = BigInt(minor);
        return value * cumulativeAfter / sold - value * cumulativeBefore / sold;
      };
      const total = allocated(item.lineSubtotalMinor) - allocated(item.lineDiscountMinor) + allocated(item.lineTaxMinor);
      return sum + Number(total);
    }, 0);
  }, [detail, quantities]);

  function setVoidMode(next: boolean) {
    const nextKind = next ? "void" : "refund";
    setKind(nextKind);
    if (next && detail) {
      setQuantities(Object.fromEntries(detail.items.map((item) => [item.saleItemId, item.remainingQuantity])));
    } else if (detail) {
      setQuantities(Object.fromEntries(detail.items.map((item) => [item.saleItemId, 0])));
    }
    setError("");
  }

  async function submitRefund() {
    if (!detail || !canRefund || busy) return;
    const items = detail.items
      .map((item) => ({ saleItemId: item.saleItemId, quantity: quantities[item.saleItemId] ?? 0 }))
      .filter((item) => item.quantity > 0);
    if (!items.length) {
      setError("Choose at least one item and quantity to return.");
      return;
    }
    if (method !== "cash" && !reference.trim()) {
      setError("Enter the confirmed external refund reference before saving.");
      return;
    }
    setBusy(true);
    setError("");
    if (!idempotencyKey.current) idempotencyKey.current = crypto.randomUUID();
    try {
      const saved = await posApi.refundSale({
        saleId,
        idempotencyKey: idempotencyKey.current,
        kind,
        reason: reason.trim(),
        items,
        paymentMethod: method,
        paymentReference: reference.trim(),
      });
      idempotencyKey.current = "";
      setResult(saved);
      setReason("");
      setReference("");
      await reload();
      onUpdated();
    } catch (cause) {
      setError(typeof cause === "string" ? cause : cause instanceof Error ? cause.message : "The refund could not be saved. Try again; retry is safe.");
    } finally {
      setBusy(false);
    }
  }

  return <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !busy) onClose(); }}>
    <article className="receipt-modal sale-detail-modal" role="dialog" aria-modal="true" aria-labelledby="sale-detail-title">
      <button className="modal-close" type="button" onClick={onClose} disabled={busy} aria-label="Close sale details"><X size={18}/></button>
      {loading && <><p className="eyebrow">TRANSACTION</p><h2 id="sale-detail-title">Loading sale…</h2></>}
      {!loading && error && !detail && <><p className="eyebrow">TRANSACTION</p><h2 id="sale-detail-title">Sale details unavailable</h2><p className="form-error">{error}</p></>}
      {!loading && detail && <>
        <p className="eyebrow">TRANSACTION DETAIL</p>
        <h2 id="sale-detail-title">{detail.receiptNumber}</h2>
        <p className="receipt-number">{formatDateTime(detail.createdAt)} · Served by {detail.cashierName} · {detail.status.replaceAll("_", " ")}</p>
        <div className="sale-detail-total"><span>Original sale</span><strong>{formatMoney(detail.totalMinor, user.currencyCode)}</strong></div>
        <div className="sale-detail-lines">
          {detail.items.map((item) => <div className="sale-detail-line" key={item.saleItemId}>
            <div><strong>{item.name}</strong><small>{item.sku} · {item.refundedQuantity} returned of {item.soldQuantity}</small></div>
            <label><span>Return qty</span><input aria-label={"Return quantity for " + item.name} type="number" min={0} max={item.remainingQuantity} value={quantities[item.saleItemId] ?? 0} disabled={!canRefund || item.remainingQuantity === 0 || kind === "void"} onChange={(event) => setQuantities((current) => ({ ...current, [item.saleItemId]: Math.min(item.remainingQuantity, Math.max(0, Number(event.target.value) || 0)) }))}/></label>
          </div>)}
        </div>
        {detail.refunds.length > 0 && <section className="sale-refund-history"><h3>Refund history</h3>{detail.refunds.map((refund) => <div key={refund.id}><span>{refund.kind === "void" ? "Voided" : "Refund"} · {formatDateTime(refund.createdAt)} · {refund.recordedBy}</span><strong>{formatMoney(refund.totalMinor, user.currencyCode)}</strong></div>)}</section>}
        {canRefund && detail.status !== "refunded" && detail.status !== "voided" && <section className="refund-form">
          <div className="refund-kind-toggle"><label><input type="checkbox" checked={kind === "void"} onChange={(event) => setVoidMode(event.target.checked)} disabled={detail.status !== "completed"}/>Void entire sale</label><span>Refund total: <strong>{formatMoney(refundTotal, user.currencyCode)}</strong></span></div>
          <label className="field-label">Refund method<select className="text-input" value={method} onChange={(event) => setMethod(event.target.value as RefundMethod)}><option value="cash">Cash from open drawer</option><option value="card">Card terminal</option><option value="mobile_money">Mobile money</option><option value="other">Other / bank transfer</option></select></label>
          {method !== "cash" && <label className="field-label">Confirmed refund reference<input className="text-input" maxLength={120} value={reference} onChange={(event) => setReference(event.target.value)} placeholder="Terminal or provider reference"/></label>}
          <label className="field-label">Reason<input className="text-input" required maxLength={250} value={reason} onChange={(event) => setReason(event.target.value)} placeholder="For example, damaged item"/></label>
          {error && <p className="form-error" role="alert">{error}</p>}
          <button className="button button-primary receipt-done" type="button" disabled={busy || refundTotal <= 0 || !reason.trim() || (method !== "cash" && !reference.trim())} onClick={() => void submitRefund()}>{busy ? "Saving refund…" : kind === "void" ? "Confirm void and refund" : "Record refund"}<RotateCcw size={16}/></button>
        </section>}
        {!canRefund && <p className="receipt-number">Your role can review this sale but cannot issue refunds or voids.</p>}
        {result && <div className="refund-success" role="status"><Check size={16}/><span>{result.kind === "void" ? "Sale voided" : "Refund recorded"} · {formatMoney(result.totalMinor, user.currencyCode)} · {result.paymentMethod.replaceAll("_", " ")}</span></div>}
      </>}
      <button className="button button-secondary receipt-done" type="button" onClick={onClose}>Done</button>
    </article>
  </div>;
}
