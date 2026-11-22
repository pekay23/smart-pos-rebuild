// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { ArrowDown, ArrowUp, CornerDownLeft, Search, X } from "lucide-react";
import { posApi } from "../lib/api";
import type { Customer, Product, SaleSummary, SessionUser, Supplier } from "../lib/models";

type SearchView = "overview" | "register" | "products" | "people" | "sales" | "expenses" | "purchases" | "reports" | "accounting" | "audit" | "settings" | "account";
type SearchItem = { kind: "product" | "customer" | "supplier" | "sale" | "page"; id: string; label: string; detail: string; view: SearchView; directory?: "customers" | "suppliers" };

const pages: { view: SearchView; label: string; permission?: string; managementOnly?: boolean }[] = [
  { view: "overview", label: "Overview", managementOnly: true },
  { view: "register", label: "Point of sale", permission: "pos:use" },
  { view: "account", label: "My account & work" },
  { view: "products", label: "Products & stock", permission: "products:read", managementOnly: true },
  { view: "people", label: "Customers", permission: "customers:read", managementOnly: true },
  { view: "people", label: "Suppliers", permission: "suppliers:read", managementOnly: true },
  { view: "sales", label: "Sales history", permission: "sales:read" },
  { view: "expenses", label: "Expenses", permission: "expenses:read" },
  { view: "purchases", label: "Purchases & receiving", permission: "purchases:read" },
  { view: "reports", label: "Reports", permission: "reports:read" },
  { view: "accounting", label: "Accounting journal", permission: "reports:read" },
  { view: "audit", label: "Activity log", permission: "reports:read" },
  { view: "settings", label: "Settings", permission: "settings:write" },
];

function includes(value: string, query: string) { return value.toLocaleLowerCase().includes(query.toLocaleLowerCase()); }

export function GlobalSearchDialog({ user, onClose, onNavigate }: { user: SessionUser; onClose: () => void; onNavigate: (view: SearchView, directory?: "customers" | "suppliers") => void }) {
  const [query, setQuery] = useState("");
  const [products, setProducts] = useState<Product[]>([]);
  const [customers, setCustomers] = useState<Customer[]>([]);
  const [suppliers, setSuppliers] = useState<Supplier[]>([]);
  const [sales, setSales] = useState<SaleSummary[]>([]);
  const [salesQuery, setSalesQuery] = useState("");
  const [loadingRecords, setLoadingRecords] = useState(true);
  const [loadingSales, setLoadingSales] = useState(false);
  const [error, setError] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const canSearchProducts = user.workspaceMode !== "cashier" && user.permissions.includes("products:read") && (user.permissions.includes("products:write") || user.permissions.includes("inventory:write"));
  const canSearchCustomers = user.workspaceMode !== "cashier" && user.permissions.includes("customers:read");
  const canSearchSuppliers = user.workspaceMode !== "cashier" && user.permissions.includes("suppliers:read");
  const canSearchSales = user.permissions.includes("sales:read");

  useEffect(() => { inputRef.current?.focus(); }, []);
  useEffect(() => {
    let current = true;
    const tasks: Promise<void>[] = [];
    if (canSearchProducts) tasks.push(posApi.products().then((rows) => { if (current) setProducts(rows); }));
    if (canSearchCustomers) tasks.push(posApi.customers().then((rows) => { if (current) setCustomers(rows); }));
    if (canSearchSuppliers) tasks.push(posApi.suppliers().then((rows) => { if (current) setSuppliers(rows); }));
    void Promise.all(tasks).catch(() => { if (current) setError("Some records could not be searched. Check your access and try again."); }).finally(() => { if (current) setLoadingRecords(false); });
    return () => { current = false; };
  }, [canSearchProducts, canSearchCustomers, canSearchSuppliers]);
  useEffect(() => {
    const term = query.trim();
    if (term.length < 2 || !canSearchSales) return;
    let current = true;
    const timer = window.setTimeout(() => {
      setLoadingSales(true);
      void posApi.querySales({ search: term, offset: 0, pageSize: 8 })
        .then((result) => { if (current) { setSales(result.rows); setSalesQuery(term); } })
        .catch(() => { if (current) setError("Some sales could not be searched. Check your access and try again."); })
        .finally(() => { if (current) setLoadingSales(false); });
    }, 140);
    return () => { current = false; window.clearTimeout(timer); };
  }, [query, canSearchSales]);

  const results = useMemo(() => {
    const term = query.trim();
    const items: SearchItem[] = [];
    if (term.length >= 2) {
      products.filter((p) => includes(`${p.name} ${p.sku} ${p.barcode ?? ""} ${p.categoryName ?? ""}`, term)).slice(0, 5).forEach((p) => items.push({ kind: "product", id: p.id, label: p.name, detail: [p.sku, p.barcode].filter(Boolean).join(" · "), view: "products" }));
      customers.filter((p) => includes(`${p.name} ${p.email ?? ""} ${p.phone ?? ""}`, term)).slice(0, 4).forEach((p) => items.push({ kind: "customer", id: p.id, label: p.name, detail: p.phone || p.email || "Customer", view: "people", directory: "customers" }));
      suppliers.filter((p) => includes(`${p.name} ${p.contactPerson ?? ""} ${p.email ?? ""} ${p.phone ?? ""}`, term)).slice(0, 4).forEach((p) => items.push({ kind: "supplier", id: p.id, label: p.name, detail: p.contactPerson || p.phone || "Supplier", view: "people", directory: "suppliers" }));
      if (salesQuery === term) sales.forEach((sale) => items.push({ kind: "sale", id: sale.id, label: `Receipt ${sale.receiptNumber}`, detail: `${sale.cashierName} · ${sale.status}`, view: "sales" }));
      pages.filter((page) => (!page.permission || user.permissions.includes(page.permission)) && (!page.managementOnly || user.workspaceMode !== "cashier") && includes(page.label, term)).forEach((page) => items.push({ kind: "page", id: `${page.view}:${page.label}`, label: page.label, detail: "Go to page", view: page.view, ...(page.label === "Suppliers" ? { directory: "suppliers" as const } : page.label === "Customers" ? { directory: "customers" as const } : {}) }));
    }
    return items.slice(0, 12);
  }, [query, products, customers, suppliers, sales, salesQuery, user.permissions, user.workspaceMode]);

  function select(item: SearchItem) { onNavigate(item.view, item.directory); onClose(); }
  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Escape") { event.preventDefault(); onClose(); }
    else if (event.key === "ArrowDown") { event.preventDefault(); setActiveIndex((index) => Math.min(index + 1, Math.max(results.length - 1, 0))); }
    else if (event.key === "ArrowUp") { event.preventDefault(); setActiveIndex((index) => Math.max(0, index - 1)); }
    else if (event.key === "Enter" && results[activeIndex]) { event.preventDefault(); select(results[activeIndex]); }
  }

  return <div className="search-overlay" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}><section className="global-search" role="dialog" aria-modal="true" aria-labelledby="global-search-title"><h2 className="sr-only" id="global-search-title">Search your workspace</h2><div className="global-search-input-wrap"><Search size={19}/><input ref={inputRef} value={query} onChange={(event) => { setQuery(event.target.value); setActiveIndex(0); }} onKeyDown={onKeyDown} placeholder="Search products, people, sales, pages…" aria-label="Search products, people, sales and pages" aria-controls="global-search-results" aria-activedescendant={results[activeIndex] ? `global-search-result-${activeIndex}` : undefined}/><kbd>ESC</kbd><button className="icon-button" type="button" onClick={onClose} aria-label="Close search"><X size={18}/></button></div><div className="global-search-results" id="global-search-results" role="listbox" aria-label="Search results">{query.trim().length < 2 ? <p className="global-search-hint">Type at least 2 characters. Results respect your account permissions.</p> : loadingRecords || loadingSales ? <p className="global-search-hint">Searching this store…</p> : error ? <p className="global-search-error" role="alert">{error}</p> : results.length === 0 ? <p className="global-search-hint">No matching results.</p> : results.map((item, index) => <button id={`global-search-result-${index}`} key={`${item.kind}:${item.id}`} type="button" className={`global-search-result ${index === activeIndex ? "selected" : ""}`} role="option" aria-selected={index === activeIndex} onMouseEnter={() => setActiveIndex(index)} onClick={() => select(item)}><span className={`global-search-kind kind-${item.kind}`}>{item.kind}</span><span className="global-search-result-copy"><strong>{item.label}</strong><small>{item.detail || "Workspace record"}</small></span>{index === activeIndex ? <CornerDownLeft size={15}/> : null}</button>)}</div><footer className="global-search-footer"><span><kbd><ArrowUp size={10}/></kbd><kbd><ArrowDown size={10}/></kbd> to navigate</span><span><kbd>↵</kbd> to open</span><span><kbd>ESC</kbd> to close</span></footer></section></div>;
}
