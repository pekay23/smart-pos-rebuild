// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useRef, useState, type FormEvent } from "react";
import { Download, PackagePlus, Plus, Search, Truck, X, FileText, Pencil } from "lucide-react";
import { posApi } from "../lib/api";
import type { Product, PurchaseInput, PurchaseOrderInput, PurchaseOrderSummary, PurchaseQueryResult, PurchaseSummary, SessionUser, Supplier } from "../lib/models";
import { formatDateTime, formatMoney, parseMoneyMinor } from "../lib/money";
import { SupplierBillingPanel } from "../components/SupplierBillingPanel";
import { useToast } from "../components/useToast";

type Period = "all" | "today" | "7d" | "30d";
type DraftLine = { key: number; productId: string; quantity: string; unitCost: string };
type ParsedOrderLine = { line: DraftLine; quantity: number | null; cost: number | null };
type ValidParsedOrderLine = { line: DraftLine; quantity: number; cost: number };
type ParsedPurchaseLine = { line: DraftLine; quantity: number | null; unitCostMinor: number | null };
type ValidParsedPurchaseLine = { line: DraftLine; quantity: number; unitCostMinor: number };
const newLine = (key: number): DraftLine => ({ key, productId: "", quantity: "1", unitCost: "" });
const pageSize = 50;

function isValidOrderLine(value: ParsedOrderLine): value is ValidParsedOrderLine {
  return Boolean(value.line.productId) && value.quantity !== null && value.cost !== null;
}

function isValidPurchaseLine(value: ParsedPurchaseLine): value is ValidParsedPurchaseLine {
  return Boolean(value.line.productId) && value.quantity !== null && value.unitCostMinor !== null;
}

function csvCell(value: string | number) {
  const text = String(value);
  const safe = /^[\s]*[=+\-@]/.test(text) ? `'${text}` : text;
  return `"${safe.replaceAll('"', '""')}"`;
}

export function PurchasesPage({ user }: { user: SessionUser }) {
  const toast = useToast();
  const [products, setProducts] = useState<Product[]>([]);
  const [suppliers, setSuppliers] = useState<Supplier[]>([]);
  const [orders, setOrders] = useState<PurchaseOrderSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [query, setQuery] = useState("");
  const [period, setPeriod] = useState<Period>("all");
  const [now, setNow] = useState(() => new Date());
  const [modalOpen, setModalOpen] = useState(false);
  const [orderModalOpen, setOrderModalOpen] = useState(false);
  const [editingOrder, setEditingOrder] = useState<PurchaseOrderSummary | null>(null);
  const [orderLines, setOrderLines] = useState<DraftLine[]>([newLine(1000)]);
  const [orderSupplierId, setOrderSupplierId] = useState("");
  const [orderReference, setOrderReference] = useState("");
  const [receivingOrder, setReceivingOrder] = useState<PurchaseOrderSummary | null>(null);
  const [receiptKey, setReceiptKey] = useState(() => crypto.randomUUID());
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState("");
  const [supplierId, setSupplierId] = useState("");
  const [reference, setReference] = useState("");
  const [lines, setLines] = useState<DraftLine[]>([newLine(0)]);
  const [offset, setOffset] = useState(0);
  const [refreshKey, setRefreshKey] = useState(0);
  const [loaded, setLoaded] = useState<{ key: string; result: PurchaseQueryResult } | null>(null);
  const snapshot = useRef<{ filterKey: string; rowId: number } | null>(null);
  const nextLineKey = useRef(1);
  const canWrite = user.permissions.includes("purchases:write");

  useEffect(() => {
    let cancelled = false;
    void Promise.all([posApi.products(), posApi.suppliers(), posApi.listPurchaseOrders()])
      .then(([productRows, supplierRows, orderRows]) => {
        if (cancelled) return;
        setProducts(productRows);
        setSuppliers(supplierRows);
        setOrders(orderRows);
      })
      .catch(() => { if (!cancelled) setError("Could not load purchase history or store inventory."); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), 60_000);
    return () => window.clearInterval(timer);
  }, []);

  const periodStartOffset = period === "today" ? 0 : period === "7d" ? -6 : period === "30d" ? -29 : null;
  const fromUtc = periodStartOffset === null ? undefined : new Date(now.getFullYear(), now.getMonth(), now.getDate() + periodStartOffset).toISOString();
  const toUtcExclusive = periodStartOffset === null ? undefined : new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1).toISOString();
  const normalizedSearch = query.trim();
  const filterKey = `${normalizedSearch}\u0000${fromUtc ?? ""}\u0000${toUtcExclusive ?? ""}`;
  const requestKey = `${filterKey}\u0000${offset}\u0000${refreshKey}`;
  const result = loaded?.key === requestKey ? loaded.result : null;
  const rows = result?.rows ?? [];

  useEffect(() => {
    let active = true;
    const snapshotRowId = offset > 0 && snapshot.current?.filterKey === filterKey ? snapshot.current.rowId : undefined;
    void posApi.queryPurchases({ fromUtc, toUtcExclusive, search: normalizedSearch, offset, pageSize, snapshotRowId })
      .then((response) => {
        if (!active) return;
        snapshot.current = { filterKey, rowId: response.snapshotRowId };
        setLoaded({ key: requestKey, result: response });
        setError("");
      })
      .catch(() => { if (active) setError("Could not load purchase history for this store."); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [filterKey, fromUtc, normalizedSearch, offset, refreshKey, requestKey, toUtcExclusive]);

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

  const totalMinor = result?.totalMinor ?? 0;
  const outstandingByProduct = new Map<string, number>();
  for (const order of orders) {
    if (order.status !== "placed" && order.status !== "partially_received") continue;
    for (const line of order.lines) {
      const outstanding = Math.max(0, line.orderedQuantity - line.receivedQuantity);
      if (outstanding) outstandingByProduct.set(line.productId, (outstandingByProduct.get(line.productId) ?? 0) + outstanding);
    }
  }
  const reorderSuggestions = products
    .filter((product) => product.isActive && product.targetStockQuantity > 0
      && product.stockQuantity <= product.reorderPoint
      && product.stockQuantity < product.targetStockQuantity)
    .map((product) => ({
      product,
      supplier: suppliers.find((supplier) => supplier.id === product.supplierId) ?? null,
      onOrder: outstandingByProduct.get(product.id) ?? 0,
    }))
    .map(({ product, supplier, onOrder }) => ({
      product,
      supplier,
      onOrder,
      quantity: Math.max(0, product.targetStockQuantity - product.stockQuantity - onOrder),
    })).filter((entry) => entry.quantity > 0);
  const openOrders = orders.filter((order) => order.status === "placed" || order.status === "partially_received");

  function exportReorderList() {
    if (!reorderSuggestions.length) return;
    const content = [
      ["SKU", "Barcode", "Product", "Supplier", "On hand", "Reorder point", "Restock target", "On order", "Suggested quantity", "Unit cost", "Estimated total", "Currency"]
        .map(csvCell).join(","),
      ...reorderSuggestions.map(({ product, supplier, quantity }) => [
        product.sku,
        product.barcode ?? "",
        product.name,
        supplier?.name ?? "Supplier not assigned",
        product.stockQuantity,
        product.reorderPoint,
        product.targetStockQuantity,
        outstandingByProduct.get(product.id) ?? 0,
        quantity,
        product.costMinor === null || !Number.isSafeInteger(product.costMinor) ? "Unknown" : formatMoney(product.costMinor, user.currencyCode),
        product.costMinor === null || !Number.isSafeInteger(product.costMinor * quantity) ? "Unknown" : formatMoney(product.costMinor * quantity, user.currencyCode),
        user.currencyCode,
      ].map(csvCell).join(",")),
    ].join("\r\n");
    const url = URL.createObjectURL(new Blob([content], { type: "text/csv;charset=utf-8" }));
    const link = document.createElement("a");
    link.href = url;
    link.download = `advanced-pos-reorder-list-${new Date().toISOString().slice(0, 10)}.csv`;
    link.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 0);
  }

  function resetForm() {
    setSupplierId("");
    setReference("");
    setReceivingOrder(null);
    setReceiptKey(crypto.randomUUID());
    setLines([newLine(nextLineKey.current++)]);
    setFormError("");
  }

  function startOrder(order?: PurchaseOrderSummary) {
    setEditingOrder(order ?? null);
    setOrderSupplierId(order?.supplierId ?? "");
    setOrderReference(order?.reference ?? "");
    setOrderLines(order?.lines.map((line) => ({ key: nextLineKey.current++, productId: line.productId, quantity: String(line.orderedQuantity), unitCost: (line.expectedUnitCostMinor / 100).toFixed(2) })) ?? [newLine(nextLineKey.current++)]);
    setFormError("");
    setOrderModalOpen(true);
  }

  function startSuggestedOrder(product: Product, quantity: number, suggestedSupplierId: string | null) {
    setEditingOrder(null);
    setOrderSupplierId(suggestedSupplierId ?? "");
    setOrderReference("");
    setOrderLines([{ key: nextLineKey.current++, productId: product.id, quantity: String(quantity), unitCost: product.costMinor == null ? "" : (product.costMinor / 100).toFixed(2) }]);
    setFormError("");
    setOrderModalOpen(true);
  }

  async function refreshOrders() {
    const rows = await posApi.listPurchaseOrders();
    setOrders(rows);
  }

  async function saveOrder(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const parsed = orderLines.map((line) => ({ line, quantity: /^[1-9]\d{0,6}$/.test(line.quantity) ? Number(line.quantity) : null, cost: parseMoneyMinor(line.unitCost) }));
    const lines = parsed.filter(isValidOrderLine);
    if (lines.length !== parsed.length) { setFormError("Choose each product and enter a whole quantity and estimated unit cost."); return; }
    if (new Set(lines.map(({ line }) => line.productId)).size !== lines.length) { setFormError("Each product can appear once. Combine quantities on the same line."); return; }
    const input: PurchaseOrderInput = {
      id: editingOrder?.id,
      revision: editingOrder?.revision,
      supplierId: orderSupplierId || null,
      reference: orderReference.trim() || null,
      lines: lines.map(({ line, quantity, cost }) => ({ productId: line.productId, quantity, expectedUnitCostMinor: cost })),
    };
    setBusy(true); setFormError("");
    try {
      const savedOrder = await posApi.savePurchaseOrder(input);
      setOrders((current) => [savedOrder, ...current.filter((order) => order.id !== savedOrder.id)]);
      setOrderModalOpen(false);
      toast("success", editingOrder ? "Purchase order draft saved." : "Purchase order draft created.");
    } catch (reason) {
      const message = typeof reason === "string" ? reason : "The purchase-order draft could not be saved.";
      setFormError(message); toast("error", message);
    } finally { setBusy(false); }
  }

  async function transitionOrder(order: PurchaseOrderSummary, action: "place" | "cancel") {
    setBusy(true);
    try {
      const updatedOrder = action === "place"
        ? await posApi.placePurchaseOrder({ id: order.id, revision: order.revision })
        : await posApi.cancelPurchaseOrder({ id: order.id, revision: order.revision });
      setOrders((current) => current.map((entry) => entry.id === updatedOrder.id ? updatedOrder : entry));
      toast("success", action === "place" ? `${order.orderNumber} marked as placed locally.` : `${order.orderNumber} cancelled.`);
    } catch (reason) {
      const message = typeof reason === "string" ? reason : "The purchase order could not be updated.";
      setError(message); toast("error", message);
    } finally { setBusy(false); }
  }

  function startOrderReceipt(order: PurchaseOrderSummary) {
    const remaining = order.lines.filter((line) => line.orderedQuantity > line.receivedQuantity);
    setReceivingOrder(order);
    setSupplierId(order.supplierId ?? "");
    setReference("");
    setReceiptKey(crypto.randomUUID());
    setLines(remaining.map((line) => ({ key: nextLineKey.current++, productId: line.productId, quantity: String(line.orderedQuantity - line.receivedQuantity), unitCost: (line.expectedUnitCostMinor / 100).toFixed(2) })));
    setFormError("");
    setModalOpen(true);
  }

  function updateLine(key: number, patch: Partial<Omit<DraftLine, "key">>) {
    setLines((current) => current.map((line) => line.key === key ? { ...line, ...patch } : line));
  }

  function chooseProduct(key: number, productId: string) {
    const product = products.find((entry) => entry.id === productId);
    updateLine(key, {
      productId,
      unitCost: product?.costMinor == null ? "" : (product.costMinor / 100).toFixed(2),
    });
  }

  function parsedLines(): ParsedPurchaseLine[] {
    return lines.map((line) => ({
      line,
      quantity: /^[1-9]\d{0,6}$/.test(line.quantity) ? Number(line.quantity) : null,
      unitCostMinor: parseMoneyMinor(line.unitCost),
    }));
  }

  const draftTotalMinor = parsedLines().reduce((sum, item) => {
    if (item.quantity === null || item.unitCostMinor === null) return sum;
    const lineTotal = item.quantity * item.unitCostMinor;
    return Number.isSafeInteger(lineTotal) && Number.isSafeInteger(sum + lineTotal) ? sum + lineTotal : sum;
  }, 0);

  async function receive(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const parsed = parsedLines();
    const validLines = parsed.filter(isValidPurchaseLine);
    if (validLines.length !== parsed.length) {
      setFormError("Choose a product and enter a whole quantity and unit cost for every line.");
      return;
    }
    if (new Set(validLines.map((item) => item.line.productId)).size !== validLines.length) {
      setFormError("Each product can appear only once. Combine quantities on the same line.");
      return;
    }
    const input: PurchaseInput = {
      purchaseOrderId: receivingOrder?.id,
      idempotencyKey: receiptKey,
      supplierId: supplierId || null,
      reference: reference.trim() || null,
      lines: validLines.map((item) => ({
        productId: item.line.productId,
        quantity: item.quantity,
        unitCostMinor: item.unitCostMinor,
        purchaseOrderLineId: receivingOrder?.lines.find((orderLine) => orderLine.productId === item.line.productId)?.id ?? null,
      })),
    };
    setBusy(true);
    setFormError("");
    try {
      await posApi.receivePurchase(input);
      let orderRefreshFailed = false;
      try { await refreshOrders(); } catch { orderRefreshFailed = true; }
      setPeriod("all");
      setQuery("");
      snapshot.current = null;
      setOffset(0);
      setLoading(true);
      setRefreshKey((current) => current + 1);
      setModalOpen(false);
      resetForm();
      setError("");
      if (orderRefreshFailed) toast("error", "Delivery received, but order status could not be refreshed. Reload this page to see the latest status.");
      else toast("success", "Delivery received. Stock and purchase records were updated.");
      void posApi.products().then(setProducts).catch(() => setError("Delivery was received, but the inventory view could not be refreshed."));
    } catch (reason) {
      const message = typeof reason === "string" ? reason : "The delivery could not be received. Review the products and try again.";
      setFormError(message); toast("error", message);
    } finally {
      setBusy(false);
    }
  }

  async function exportCsv() {
    if (!result?.matchedCount) return;
    setBusy(true);
    setError("");
    try {
      const exportRows: PurchaseSummary[] = [];
      let exportOffset = 0;
      let exportSnapshot: number | undefined;
      let expectedCount = 0;
      do {
        const page = await posApi.queryPurchases({ fromUtc, toUtcExclusive, search: normalizedSearch, offset: exportOffset, pageSize: 100, snapshotRowId: exportSnapshot });
        exportSnapshot ??= page.snapshotRowId;
        expectedCount = page.matchedCount;
        exportRows.push(...page.rows);
        if (!page.rows.length) break;
        exportOffset += page.rows.length;
      } while (exportOffset < expectedCount);
      if (exportRows.length !== expectedCount) throw new Error("The purchase report changed during export.");
      const content = [
        ["Received", "Supplier", "Reference", "Items", "Item details", "Total", "Currency", "Received by"].map(csvCell).join(","),
        ...exportRows.map((purchase) => [
          formatDateTime(purchase.receivedAt), purchase.supplierName ?? "Unspecified supplier",
          purchase.reference ?? "", purchase.itemCount, purchase.itemsSummary,
          formatMoney(purchase.totalMinor, user.currencyCode), user.currencyCode, purchase.recordedBy,
        ].map(csvCell).join(",")),
      ].join("\r\n");
      const url = URL.createObjectURL(new Blob([content], { type: "text/csv;charset=utf-8" }));
      const link = document.createElement("a");
      link.href = url;
      link.download = `advanced-pos-purchases-${new Date().toISOString().slice(0, 10)}.csv`;
      link.click();
      window.setTimeout(() => URL.revokeObjectURL(url), 0);
    } catch {
      setError("The complete matching purchase report could not be exported. Try again.");
    } finally {
      setBusy(false);
    }
  }

  return <section className="purchase-page">
    <div className="page-heading">
      <div><p className="eyebrow">INVENTORY INTAKE</p><h1>Purchases & receiving</h1><p className="page-subtitle">Receive supplier deliveries and update on-hand stock together.</p></div>
      {canWrite && <button className="button button-primary" onClick={() => { resetForm(); setModalOpen(true); }} disabled={!products.length}><Plus size={17}/> Receive delivery</button>}
    </div>
    <article className="panel reorder-suggestions-panel">
      <div className="table-toolbar"><div><h2>Reorder suggestions</h2><p>Based on each product’s reorder point and restock target. Preparing this list does not change stock.</p></div><div className="table-tools"><span className="reorder-count">{reorderSuggestions.length} items</span><button className="button button-secondary button-small" onClick={exportReorderList} disabled={!reorderSuggestions.length}><Download size={15}/> Export list</button></div></div>
      {reorderSuggestions.length ? <div className="table-scroll"><table><thead><tr><th>PRODUCT</th><th>SUPPLIER</th><th>ON HAND</th><th>RESTOCK TO</th><th>ON ORDER</th><th>ORDER QTY</th><th>EST. UNIT COST</th><th><span className="sr-only">Actions</span></th></tr></thead><tbody>{reorderSuggestions.map(({ product, supplier, onOrder, quantity }) => <tr key={product.id}><td><strong className="purchase-supplier">{product.name}</strong><small className="table-subvalue">{product.sku}{product.barcode ? ` · ${product.barcode}` : ""}</small></td><td>{supplier?.name ?? <span className="muted-value">Not assigned</span>}</td><td><strong className="quantity-low">{product.stockQuantity}</strong><small className="table-subvalue">Alert at {product.reorderPoint}</small></td><td>{product.targetStockQuantity}</td><td>{onOrder || "—"}</td><td><strong>{quantity}</strong></td><td>{product.costMinor === null ? <span className="muted-value">Unknown</span> : formatMoney(product.costMinor, user.currencyCode)}</td><td>{canWrite && <button className="table-action" onClick={() => startSuggestedOrder(product, quantity, supplier?.id ?? null)}><Plus size={13}/> Draft order</button>}</td></tr>)}</tbody></table></div> : <div className="reorder-empty"><PackagePlus size={18}/><span>No products need reordering with a configured restock target.</span></div>}
    </article>
    <article className="panel purchase-orders-panel">
      <div className="table-toolbar"><div><h2>Purchase orders</h2><p>{orders.length} total · {openOrders.length} open. Marking placed records your action locally; it does not send the order to the supplier.</p></div>{canWrite && <button className="button button-secondary button-small" onClick={() => startOrder()}><Plus size={14}/> New order</button>}</div>
      {orders.length ? <div className="table-scroll"><table><thead><tr><th>ORDER</th><th>SUPPLIER</th><th>STATUS</th><th>ITEMS / RECEIVED</th><th>ESTIMATED</th><th><span className="sr-only">Actions</span></th></tr></thead><tbody>
        {orders.map((order) => <tr key={order.id}><td><strong>{order.orderNumber}</strong>{order.reference && <small className="table-subvalue">{order.reference}</small>}</td><td>{order.supplierName ?? <span className="muted-value">Unspecified supplier</span>}</td><td><span className={`status-pill ${order.status === "received" ? "status-active" : order.status === "cancelled" ? "status-inactive" : ""}`}><i/>{order.status.replaceAll("_", " ")}</span></td><td>{order.lines.reduce((sum, line) => sum + line.orderedQuantity, 0)} units ordered<small className="table-subvalue">{order.lines.reduce((sum, line) => sum + line.receivedQuantity, 0)} received</small></td><td>{formatMoney(order.totalEstimatedMinor, user.currencyCode)}</td><td className="purchase-order-actions">
          {canWrite && order.status === "draft" && <><button className="table-action" onClick={() => startOrder(order)} title="Edit draft"><Pencil size={14}/> Edit</button><button className="table-action" disabled={busy} onClick={() => void transitionOrder(order, "place")}>Mark placed</button></>}
          {canWrite && (order.status === "placed" || order.status === "partially_received") && <button className="table-action" disabled={busy} onClick={() => startOrderReceipt(order)}><Truck size={14}/> Receive</button>}
          {canWrite && ["draft", "placed", "partially_received"].includes(order.status) && <button className="table-action purchase-cancel-action" disabled={busy} onClick={() => { if (window.confirm(`Cancel ${order.orderNumber}? Any goods already received remain recorded.`)) void transitionOrder(order, "cancel"); }}>Cancel</button>}
        </td></tr>)}
      </tbody></table></div> : <div className="reorder-empty"><FileText size={18}/><span>No purchase orders yet. Create a draft from your reorder needs or start one here.</span></div>}
    </article>
    <div className="purchase-summary">
      <div><span className="purchase-summary-icon"><Truck size={17}/></span><span><small>{period === "all" ? "Matching purchases" : "Purchases in period"}</small><strong>{formatMoney(totalMinor, user.currencyCode)}</strong><em>{result?.matchedCount ?? 0} {(result?.matchedCount ?? 0) === 1 ? "delivery" : "deliveries"}</em></span></div>
      <p>Receiving updates stock, product cost, purchase history, and stock movements in one save.</p>
    </div>
    <article className="panel purchase-panel">
      <div className="table-toolbar purchase-toolbar">
        <div><h2>Received deliveries</h2><p>{user.storeName} · {pageSize} records per page</p></div>
        <div className="table-tools">
          <label className="table-search"><Search size={16}/><input value={query} onChange={(event) => updateSearch(event.target.value)} placeholder="Search deliveries…" aria-label="Search purchases"/></label>
          <label className="sr-only" htmlFor="purchase-period">Date range</label>
          <select id="purchase-period" className="expense-filter" value={period} onChange={(event) => updatePeriod(event.target.value as Period)}><option value="all">All dates</option><option value="today">Today</option><option value="7d">Last 7 days</option><option value="30d">Last 30 days</option></select>
          <button className="button button-secondary button-small" onClick={() => void exportCsv()} disabled={!result?.matchedCount || busy || loading}><Download size={15}/>{busy ? "Exporting…" : "Export all"}</button>
        </div>
      </div>
      {error && <p className="expense-error" role="alert">{error}</p>}
      {loading && !result ? <div className="table-empty"><span>Loading purchase history…</span></div> : rows.length ? <div className="table-scroll">
        <table className="purchase-table"><thead><tr><th>RECEIVED</th><th>SUPPLIER / REF</th><th>ITEMS RECEIVED</th><th>RECEIVED BY</th><th className="align-right">TOTAL COST</th></tr></thead><tbody>
          {rows.map((purchase) => <tr key={purchase.id}><td>{formatDateTime(purchase.receivedAt)}</td><td><strong className="purchase-supplier">{purchase.supplierName || "Unspecified supplier"}</strong>{purchase.reference && <small className="table-subvalue">Ref. {purchase.reference}</small>}</td><td><span className="purchase-item-count">{purchase.itemCount} units</span><small className="purchase-item-summary">{purchase.itemsSummary}</small></td><td>{purchase.recordedBy}</td><td className="align-right purchase-total">{formatMoney(purchase.totalMinor, user.currencyCode)}</td></tr>)}
        </tbody></table>
      </div> : <div className="table-empty"><span className="empty-icon"><PackagePlus size={22}/></span><strong>{query || period !== "all" ? "No deliveries match these filters" : "No deliveries received yet"}</strong><span>{query || period !== "all" ? "Try another search or date range." : products.length ? "Receive a supplier delivery to add stock and record its cost." : "Add active products before receiving stock."}</span>{canWrite && products.length > 0 && !query && period === "all" && <button className="button button-secondary button-small" onClick={() => { resetForm(); setModalOpen(true); }}>Receive a delivery <Plus size={15}/></button>}</div>}
      {result && result.matchedCount > pageSize && <div className="expense-pagination"><span>Showing {offset + 1}–{offset + rows.length} of {result.matchedCount}</span><div><button className="button button-secondary button-small" disabled={offset === 0 || loading} onClick={() => { setLoading(true); setOffset((current) => Math.max(0, current - pageSize)); }}>Previous</button><button className="button button-secondary button-small" disabled={offset + rows.length >= result.matchedCount || loading} onClick={() => { setLoading(true); setOffset((current) => current + pageSize); }}>Next</button></div></div>}
    </article>
    <SupplierBillingPanel user={user} suppliers={suppliers}/>
    <footer className="page-footer"><span>INVENTORY <b>·</b> Receiving records and stock movements</span><span>CSV export follows the current search and date filters</span></footer>

    {orderModalOpen && <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget && !busy) setOrderModalOpen(false); }}>
      <form className="edit-modal purchase-modal" onSubmit={(event) => void saveOrder(event)}>
        <button type="button" className="modal-close" onClick={() => setOrderModalOpen(false)} aria-label="Close" disabled={busy}><X size={18}/></button>
        <p className="eyebrow">PURCHASING</p><h2>{editingOrder ? `Edit ${editingOrder.orderNumber}` : "New purchase order"}</h2><p className="modal-subtitle">Save a draft, then mark it placed after you send it through your usual supplier channel. This app does not send it.</p>
        <div className="form-grid two-col product-form-grid"><div><label className="field-label" htmlFor="order-supplier">Supplier <span className="expense-optional">Optional</span></label><select className="text-input" id="order-supplier" value={orderSupplierId} onChange={(event) => setOrderSupplierId(event.target.value)}><option value="">Unspecified supplier</option>{suppliers.filter((supplier) => supplier.isActive).map((supplier) => <option key={supplier.id} value={supplier.id}>{supplier.name}</option>)}</select></div><div><label className="field-label" htmlFor="order-reference">Supplier quote / reference</label><input className="text-input" id="order-reference" maxLength={100} value={orderReference} onChange={(event) => setOrderReference(event.target.value)} placeholder="Optional quote or note"/></div></div>
        <div className="purchase-lines-heading"><strong>Products requested</strong><button type="button" className="text-action" onClick={() => setOrderLines((current) => [...current, newLine(nextLineKey.current++)])}><Plus size={14}/> Add product</button></div>
        <div className="purchase-lines">{orderLines.map((line, index) => { const selected = products.find((product) => product.id === line.productId); return <div className="purchase-line" key={line.key}><div className="purchase-product-field"><label className="field-label" htmlFor={`order-product-${line.key}`}>Product {index + 1}</label><select className="text-input" id={`order-product-${line.key}`} required value={line.productId} onChange={(event) => { const product = products.find((item) => item.id === event.target.value); setOrderLines((current) => current.map((entry) => entry.key === line.key ? { ...entry, productId: event.target.value, unitCost: product?.costMinor == null ? "" : (product.costMinor / 100).toFixed(2) } : entry)); }}><option value="">Choose a product</option>{products.filter((product) => product.isActive).map((product) => <option key={product.id} value={product.id}>{product.name} · {product.sku}</option>)}</select><small className="field-hint">{selected ? `${selected.stockQuantity} currently in stock` : "Estimated order quantity"}</small></div><div className="purchase-quantity-field"><label className="field-label" htmlFor={`order-quantity-${line.key}`}>Quantity</label><input className="text-input" id={`order-quantity-${line.key}`} type="number" min="1" max="1000000" step="1" required value={line.quantity} onChange={(event) => setOrderLines((current) => current.map((entry) => entry.key === line.key ? { ...entry, quantity: event.target.value } : entry))}/></div><div className="purchase-cost-field"><label className="field-label" htmlFor={`order-cost-${line.key}`}>Est. unit cost</label><input className="text-input" id={`order-cost-${line.key}`} inputMode="decimal" required value={line.unitCost} onChange={(event) => setOrderLines((current) => current.map((entry) => entry.key === line.key ? { ...entry, unitCost: event.target.value } : entry))} placeholder="0.00"/></div><button type="button" className="purchase-remove" aria-label={`Remove product ${index + 1}`} disabled={orderLines.length === 1 || busy} onClick={() => setOrderLines((current) => current.filter((entry) => entry.key !== line.key))}><X size={15}/></button></div>; })}</div>
        {formError && <p className="form-error" role="alert">{formError}</p>}
        <div className="modal-actions"><button type="button" className="button button-secondary" onClick={() => setOrderModalOpen(false)} disabled={busy}>Cancel</button><button className="button button-primary" disabled={busy || !products.length}>{busy ? "Saving…" : "Save draft"}</button></div>
      </form>
    </div>}
    {modalOpen && <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget && !busy) setModalOpen(false); }}>
      <form className="edit-modal purchase-modal" onSubmit={(event) => void receive(event)}>
        <button type="button" className="modal-close" onClick={() => setModalOpen(false)} aria-label="Close" disabled={busy}><X size={18}/></button>
        <p className="eyebrow">INVENTORY INTAKE</p><h2>{receivingOrder ? `Receive ${receivingOrder.orderNumber}` : "Receive a delivery"}</h2><p className="modal-subtitle">Enter the actual quantities and costs received. Stock and accounting are recorded together.</p>
        <div className="form-grid two-col product-form-grid">
          <div><label className="field-label" htmlFor="purchase-supplier">Supplier <span className="expense-optional">Optional</span></label><select className="text-input" id="purchase-supplier" value={supplierId} onChange={(event) => setSupplierId(event.target.value)}><option value="">Unspecified supplier</option>{suppliers.filter((supplier) => supplier.isActive).map((supplier) => <option key={supplier.id} value={supplier.id}>{supplier.name}</option>)}</select></div>
          <div><label className="field-label" htmlFor="purchase-reference">Supplier reference <span className="expense-optional">Optional</span></label><input className="text-input" id="purchase-reference" maxLength={100} value={reference} onChange={(event) => setReference(event.target.value)} placeholder="Invoice or delivery number"/></div>
        </div>
        <div className="purchase-lines-heading"><strong>Products received</strong>{!receivingOrder && <button type="button" className="text-action" onClick={() => setLines((current) => [...current, newLine(nextLineKey.current++)])}><Plus size={14}/> Add product</button>}</div>
        <div className="purchase-lines">{lines.map((line, index) => {
          const selected = products.find((product) => product.id === line.productId);
          const orderedLine = receivingOrder?.lines.find((orderLine) => orderLine.productId === line.productId);
          const actualUnitCost = parseMoneyMinor(line.unitCost);
          return <div className="purchase-line" key={line.key}>
            <div className="purchase-product-field"><label className="field-label" htmlFor={`purchase-product-${line.key}`}>Product {index + 1}</label>{receivingOrder ? <><input className="text-input" id={`purchase-product-${line.key}`} readOnly value={selected ? `${selected.name} · ${selected.sku}` : line.productId}/><small className="field-hint">Ordered {orderedLine?.orderedQuantity}; previously received {orderedLine?.receivedQuantity}</small></> : <><select className="text-input" id={`purchase-product-${line.key}`} required value={line.productId} onChange={(event) => chooseProduct(line.key, event.target.value)}><option value="">Choose a product</option>{products.map((product) => <option key={product.id} value={product.id}>{product.name} · {product.sku}</option>)}</select><small className="field-hint">{selected ? `${selected.stockQuantity} currently in stock` : "Choose from active catalog products"}</small></>}</div>
            <div className="purchase-quantity-field"><label className="field-label" htmlFor={`purchase-quantity-${line.key}`}>Quantity</label><input className="text-input" id={`purchase-quantity-${line.key}`} type="number" inputMode="numeric" min="1" max="1000000" step="1" required value={line.quantity} onChange={(event) => updateLine(line.key, { quantity: event.target.value })}/></div>
            <div className="purchase-cost-field"><label className="field-label" htmlFor={`purchase-cost-${line.key}`}>Actual unit cost</label><input className="text-input" id={`purchase-cost-${line.key}`} inputMode="decimal" required value={line.unitCost} onChange={(event) => updateLine(line.key, { unitCost: event.target.value })} placeholder="0.00"/>{orderedLine && <small className="field-hint">Estimated: {formatMoney(orderedLine.expectedUnitCostMinor, user.currencyCode)}{actualUnitCost !== null && actualUnitCost !== orderedLine.expectedUnitCostMinor ? ` · Variance ${formatMoney(actualUnitCost - orderedLine.expectedUnitCostMinor, user.currencyCode)} per unit` : ""}</small>}</div>
            {!receivingOrder && <button type="button" className="purchase-remove" aria-label={`Remove product ${index + 1}`} disabled={lines.length === 1 || busy} onClick={() => setLines((current) => current.filter((entry) => entry.key !== line.key))}><X size={15}/></button>}
          </div>;
        })}</div>
        <div className="purchase-total-preview"><span>Delivery total</span><strong>{formatMoney(draftTotalMinor, user.currencyCode)}</strong></div>
        {formError && <p className="form-error" role="alert">{formError}</p>}
        <div className="modal-actions"><button type="button" className="button button-secondary" onClick={() => setModalOpen(false)} disabled={busy}>Cancel</button><button className="button button-primary" disabled={busy || !products.length}>{busy ? "Receiving…" : "Receive delivery"}</button></div>
      </form>
    </div>}
  </section>;
}
