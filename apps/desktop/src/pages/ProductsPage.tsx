// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { Archive, Boxes, CircleAlert, ClipboardList, PackagePlus, Plus, Printer, Search, Sparkles, X } from "lucide-react";
import { posApi } from "../lib/api";
import type { Category, Product, ProductInput, ProductLabel, SessionUser } from "../lib/models";
import { formatMoney } from "../lib/money";
import { EntityImageField } from "../components/EntityImageField";
import { useEntityImage } from "../components/useEntityImage";
import { EntityImageThumbnail } from "../components/EntityImageThumbnail";
import { useToast } from "../components/useToast";
import { StockCountDialog } from "../components/StockCountDialog";
import { ProductLabelManagerDialog } from "../components/ProductLabelManagerDialog";
import { ProductLabelPrintDialog } from "../components/ProductLabelPrintDialog";

const blank: ProductInput = { sku: "", barcode: null, name: "", description: null, categoryId: null, brandId: null, tagIds: [], supplierId: null, priceMinor: 0, costMinor: null, stockQuantity: 0, reorderPoint: 5, targetStockQuantity: 0, isActive: true };

export function ProductsPage({ user, onUpdated }: { user: SessionUser; onUpdated: () => void }) {
  const toast = useToast();
  const [products, setProducts] = useState<Product[]>([]);
  const [categories, setCategories] = useState<Category[]>([]);
  const [brands, setBrands] = useState<ProductLabel[]>([]);
  const [tags, setTags] = useState<ProductLabel[]>([]);
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [editing, setEditing] = useState<Product | null>(null);
  const [form, setForm] = useState<ProductInput>(blank);
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const [stock, setStock] = useState<Product | null>(null);
  const [delta, setDelta] = useState("1");
  const [note, setNote] = useState("Stock count correction");
  const [stockError, setStockError] = useState("");
  const [categoryOpen, setCategoryOpen] = useState(false);
  const [categoryName, setCategoryName] = useState("");
  const [categoryDescription, setCategoryDescription] = useState("");
  const [stockCountOpen, setStockCountOpen] = useState(false);
  const [labelsOpen, setLabelsOpen] = useState(false);
  const [printLabelsOpen, setPrintLabelsOpen] = useState(false);
  const [selectedProductIds, setSelectedProductIds] = useState<Set<string>>(() => new Set());
  const image = useEntityImage("product");

  const load = useCallback(async () => { const [p, c, b, t] = await Promise.all([posApi.products(), posApi.categories(), posApi.productBrands(), posApi.productTags()]); setProducts(p); setCategories(c); setBrands(b); setTags(t); }, []);
  useEffect(() => { void load().catch(() => setError("Could not load inventory.")); }, [load]);
  const filtered = useMemo(() => products.filter((p) => `${p.name} ${p.sku} ${p.barcode ?? ""} ${p.categoryName ?? ""} ${p.brandName ?? ""} ${p.tags.join(" ")}`.toLowerCase().includes(query.toLowerCase())), [products, query]);
  const selectedProducts = useMemo(() => products.filter((product) => selectedProductIds.has(product.id)), [products, selectedProductIds]);
  const allFilteredSelected = filtered.length > 0 && filtered.every((product) => selectedProductIds.has(product.id));
  const lowStock = products.filter((p) => p.isActive && p.stockQuantity <= p.reorderPoint).length;
  function generateSku() {
    const used = new Set(products.filter((product) => product.id !== editing?.id).map((product) => product.sku.trim().toUpperCase()));
    setForm((current) => ({ ...current, sku: createSku(used) }));
  }
  function generateBarcode() {
    const used = new Set(products.map((product) => product.barcode?.trim().toUpperCase()).filter((value): value is string => Boolean(value)));
    setForm((current) => ({ ...current, barcode: createBarcode(used) }));
  }
  function selectProduct(productId: string, selected: boolean) { setSelectedProductIds((current) => { const next = new Set(current); if (selected) next.add(productId); else next.delete(productId); return next; }); }
  function startAdd() {
    image.reset();
    setEditing(null);
    const used = new Set(products.map((product) => product.sku.trim().toUpperCase()));
    setForm({ ...blank, sku: createSku(used) });
    setError("");
    setOpen(true);
  }
  function startEdit(product: Product) { image.reset(product.id); setEditing(product); setForm({ id: product.id, sku: product.sku, barcode: product.barcode, name: product.name, description: product.description, categoryId: product.categoryId, brandId: product.brandId, tagIds: product.tagIds, supplierId: product.supplierId, priceMinor: product.priceMinor, costMinor: product.costMinor, stockQuantity: product.stockQuantity, reorderPoint: product.reorderPoint, targetStockQuantity: product.targetStockQuantity, isActive: product.isActive }); setError(""); setOpen(true); }
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setSaving(true); setError("");
    try {
      const input: ProductInput = { ...form, name: form.name.trim(), sku: form.sku.trim(), barcode: form.barcode?.trim() || null, priceMinor: Number(form.priceMinor), costMinor: form.costMinor === null ? null : Number(form.costMinor), stockQuantity: Number(form.stockQuantity), reorderPoint: Number(form.reorderPoint) };
      const saved = await posApi.saveProduct(input); setForm({ ...input, id: saved.id }); setEditing(saved); await image.commit(saved.id); image.reset(saved.id); await load(); onUpdated(); setOpen(false); toast("success", `${saved.name} saved to the product catalog.`);
    } catch (reason) { const message = typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "Product could not be saved. SKU and barcode values must be unique."; setError(message); toast("error", message); }
    finally { setSaving(false); }
  }
  async function adjustStock(event: FormEvent<HTMLFormElement>) { event.preventDefault(); setStockError(""); try { if (!stock) return; await posApi.adjustStock({ productId: stock.id, quantityDelta: Number(delta), note }); await load(); onUpdated(); setStock(null); toast("success", `Stock updated for ${stock.name}.`); } catch (reason) { const message = typeof reason === "string" ? reason : "Stock adjustment could not be saved."; setStockError(message); toast("error", message); } }
  async function saveCategory(event: FormEvent<HTMLFormElement>) { event.preventDefault(); setError(""); try { await posApi.saveCategory({ name: categoryName.trim(), description: categoryDescription.trim() || null }); await load(); setCategoryName(""); setCategoryDescription(""); setCategoryOpen(false); toast("success", "Product category added."); } catch (reason) { const message = typeof reason === "string" ? reason : "Category could not be saved."; setError(message); toast("error", message); } }

  return <section className="products-page"><div className="page-heading"><div><p className="eyebrow">CATALOGUE & INVENTORY</p><h1>Products & stock</h1><p className="page-subtitle">Manage the items you sell and keep an eye on what needs replenishing.</p></div><div className="page-heading-actions">{user.permissions.includes("inventory:write") && <button className="button button-secondary" type="button" onClick={() => setStockCountOpen(true)}><ClipboardList size={16}/> Stock count</button>}<button className="button button-primary" onClick={startAdd}><Plus size={17}/> Add product</button></div></div>
    <div className="inventory-stats"><article><span className="stat-icon"><Boxes size={18}/></span><span><small>Active products</small><strong>{products.filter((p) => p.isActive).length}</strong></span></article><article><span className="stat-icon stat-icon-amber"><CircleAlert size={18}/></span><span><small>Low stock items</small><strong>{lowStock}</strong></span></article><article><span className="stat-icon stat-icon-blue"><Archive size={18}/></span><span><small>Catalog items</small><strong>{products.length}</strong></span></article><div className="inventory-cta"><PackagePlus size={20}/><span><strong>Keep your catalog current</strong><small>Accurate prices and counts help every checkout go smoothly.</small></span></div></div>
    <article className="panel inventory-panel"><div className="table-toolbar"><div><h2>All products</h2><p>{filtered.length} items in this view · {categories.length} categories · {brands.filter((label) => label.isActive).length} brands · {tags.filter((label) => label.isActive).length} tags</p></div><div className="table-tools"><label className="table-search"><Search size={16}/><input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search products…" aria-label="Search products"/></label><button className="button button-secondary button-small" disabled={selectedProducts.length === 0} onClick={() => setPrintLabelsOpen(true)}><Printer size={14}/> Labels ({selectedProducts.length})</button><button className="button button-secondary button-small" onClick={() => setLabelsOpen(true)}><Plus size={14}/> Brands & tags</button><button className="button button-secondary button-small" onClick={() => { setCategoryOpen(true); setError(""); }}><Plus size={14}/> Category</button></div></div>
      <div className="table-scroll"><table><thead><tr><th><input type="checkbox" aria-label="Select all visible products for label printing" checked={allFilteredSelected} onChange={(event) => setSelectedProductIds((current) => { const next = new Set(current); for (const product of filtered) { if (event.target.checked) next.add(product.id); else next.delete(product.id); } return next; })}/></th><th>PRODUCT</th><th>SKU / BARCODE</th><th>CATEGORY</th><th>BRAND / TAGS</th><th>PRICE</th><th>IN STOCK</th><th>STATUS</th><th><span className="sr-only">Actions</span></th></tr></thead><tbody>{filtered.map((p) => <tr key={p.id}><td><input type="checkbox" aria-label={`Select ${p.name} for label printing`} checked={selectedProductIds.has(p.id)} onChange={(event) => selectProduct(p.id, event.target.checked)}/></td><td><button className="product-table-name" onClick={() => startEdit(p)}><EntityImageThumbnail entityType="product" entityId={p.id} fallback={p.name.charAt(0).toUpperCase()} className={`table-product-mark mark-${p.name.charCodeAt(0) % 5}`}/><span><strong>{p.name}</strong><small>{p.description || "—"}</small></span></button></td><td><span className="mono-value">{p.sku}</span><small className="table-subvalue">{p.barcode || "No barcode"}</small></td><td>{p.categoryName || <span className="muted-value">Uncategorized</span>}</td><td><span>{p.brandName || <span className="muted-value">No brand</span>}</span><small className="table-subvalue">{p.tags.length ? p.tags.join(", ") : "No tags"}</small></td><td><strong className="table-price">{formatMoney(p.priceMinor, user.currencyCode)}</strong></td><td><strong className={p.stockQuantity <= p.reorderPoint ? "quantity-low" : "quantity-normal"}>{p.stockQuantity}</strong><small className="table-subvalue">Reorder at {p.reorderPoint} · Target {p.targetStockQuantity || "—"}</small></td><td><span className={p.isActive ? "status-pill status-active" : "status-pill status-inactive"}><i/>{p.isActive ? (p.stockQuantity <= p.reorderPoint ? "Low stock" : "Active") : "Archived"}</span></td><td><button className="table-action" onClick={() => { setStock(p); setDelta("1"); setNote("Stock count correction"); setStockError(""); }} title="Adjust stock">Adjust</button></td></tr>)}
        {filtered.length === 0 && <tr><td colSpan={9}><div className="table-empty"><span className="empty-icon"><Boxes size={23}/></span><strong>{products.length ? "No products found" : "Add your first product"}</strong><span>{products.length ? "Try a different product name or code." : "Your catalog starts with the items you sell. Add a product to make it available in the register."}</span>{!products.length && <button className="button button-secondary button-small" onClick={startAdd}>Add first product <Plus size={15}/></button>}</div></td></tr>}</tbody></table></div>
    </article><footer className="page-footer"><span>INVENTORY <b>·</b> Stock counts update automatically after checkout</span><span>Changes are recorded in this device’s activity log</span></footer>
    {stockCountOpen && <StockCountDialog onClose={() => setStockCountOpen(false)} onPosted={() => { void load().then(onUpdated).catch(() => toast("error", "Count was posted, but product quantities could not be refreshed.")); }} />}{labelsOpen && <ProductLabelManagerDialog brands={brands} tags={tags} onClose={() => setLabelsOpen(false)} onSaved={load}/>}{printLabelsOpen && <ProductLabelPrintDialog products={selectedProducts} currencyCode={user.currencyCode} onClose={() => setPrintLabelsOpen(false)}/>}
    {open && <div className="modal-backdrop" onMouseDown={(e) => { if (e.target === e.currentTarget) setOpen(false); }}><form className="edit-modal" onSubmit={(e) => void save(e)}><button type="button" className="modal-close" onClick={() => setOpen(false)} aria-label="Close"><X size={18}/></button><p className="eyebrow">PRODUCT CATALOGUE</p><h2>{editing ? "Edit product" : "Add a product"}</h2><p className="modal-subtitle">Product details are stored locally on this device.</p><EntityImageField image={image} title="Product photo"/><div className="form-grid two-col product-form-grid"><div className="form-grid-span"><label className="field-label" htmlFor="productName">Product name</label><input className="text-input" id="productName" required autoFocus value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} placeholder="e.g. Local product"/></div><div><label className="field-label" htmlFor="productSku">SKU</label><div className="sku-input-row"><input className="text-input" id="productSku" value={form.sku} onChange={(e) => setForm({ ...form, sku: e.target.value })} placeholder="Leave blank to generate automatically"/><button className="button button-secondary button-small" type="button" onClick={generateSku} title="Generate a unique SKU"><Sparkles size={14}/>Generate</button></div><small className="field-hint">A unique SKU is generated automatically when left blank. You can enter your own or generate one now.</small></div><div><label className="field-label" htmlFor="productBarcode">Barcode</label><div className="sku-input-row"><input className="text-input" id="productBarcode" value={form.barcode ?? ""} onChange={(e) => setForm({ ...form, barcode: e.target.value || null })} placeholder="Optional"/><button className="button button-secondary button-small" type="button" onClick={generateBarcode} title="Generate a unique internal Code 128 barcode"><Sparkles size={14}/>Generate</button></div><small className="field-hint">Optional internal Code 128 value for labels and scanning; this is not a GS1 retail number.</small></div><div><label className="field-label" htmlFor="productPrice">Selling price ({user.currencyCode})</label><input className="text-input" id="productPrice" required inputMode="decimal" min="0" step="0.01" value={formatInputMinor(form.priceMinor)} onChange={(e) => setForm({ ...form, priceMinor: parseMinor(e.target.value) })} placeholder="0.00"/></div><div><label className="field-label" htmlFor="productCost">Cost price ({user.currencyCode})</label><input className="text-input" id="productCost" inputMode="decimal" min="0" step="0.01" value={form.costMinor === null ? "" : formatInputMinor(form.costMinor)} onChange={(e) => setForm({ ...form, costMinor: e.target.value ? parseMinor(e.target.value) : null })} placeholder="Optional"/></div><div><label className="field-label" htmlFor="productCategory">Category</label><select className="text-input" id="productCategory" value={form.categoryId ?? ""} onChange={(e) => setForm({ ...form, categoryId: e.target.value || null })}><option value="">Uncategorized</option>{categories.map((c) => <option value={c.id} key={c.id}>{c.name}</option>)}</select></div><div><label className="field-label" htmlFor="productBrand">Brand</label><select className="text-input" id="productBrand" value={form.brandId ?? ""} onChange={(e) => setForm({ ...form, brandId: e.target.value || null })}><option value="">No brand</option>{brands.filter((brand) => brand.isActive || brand.id === form.brandId).map((brand) => <option value={brand.id} key={brand.id}>{brand.name}{brand.isActive ? "" : " (archived)"}</option>)}</select></div><fieldset className="product-tag-picker form-grid-span"><legend className="field-label">Tags</legend>{tags.length === 0 && <small className="field-hint">Add tags from the Brands &amp; tags manager.</small>}<div className="product-tag-options">{tags.filter((tag) => tag.isActive || form.tagIds.includes(tag.id)).map((tag) => <label className="product-tag-option" key={tag.id}><input type="checkbox" checked={form.tagIds.includes(tag.id)} disabled={!tag.isActive && !form.tagIds.includes(tag.id)} onChange={(e) => setForm((current) => ({ ...current, tagIds: e.target.checked ? [...current.tagIds, tag.id] : current.tagIds.filter((id) => id !== tag.id) }))}/><span>{tag.name}{tag.isActive ? "" : " (archived)"}</span></label>)}</div></fieldset><div><label className="field-label" htmlFor="productStock">Opening stock</label><input className="text-input" id="productStock" type="number" required min="0" value={form.stockQuantity} onChange={(e) => setForm({ ...form, stockQuantity: Number(e.target.value) })} disabled={!!editing} title={editing ? "Use Adjust stock to change quantities" : undefined}/></div><div><label className="field-label" htmlFor="reorderPoint">Reorder at</label><input className="text-input" id="reorderPoint" type="number" required min="0" value={form.reorderPoint} onChange={(e) => setForm({ ...form, reorderPoint: Number(e.target.value) })}/></div><div><label className="field-label" htmlFor="targetStockQuantity">Restock to</label><input className="text-input" id="targetStockQuantity" type="number" min="0" max="1000000" step="1" value={form.targetStockQuantity} onChange={(e) => setForm({ ...form, targetStockQuantity: Number(e.target.value) })}/><small className="field-hint">Optional. Set a target above the reorder point for reorder suggestions. Zero disables suggestions.</small></div><div className="form-grid-span"><label className="field-label" htmlFor="productDescription">Description</label><input className="text-input" id="productDescription" value={form.description ?? ""} onChange={(e) => setForm({ ...form, description: e.target.value || null })} placeholder="Optional product details"/></div>{editing && <label className="checkbox-field form-grid-span"><input type="checkbox" checked={form.isActive} onChange={(e) => setForm({ ...form, isActive: e.target.checked })}/><span>Product is active and available at checkout</span></label>}</div>{error && <p className="form-error" role="alert">{error}</p>}<div className="modal-actions"><button type="button" className="button button-secondary" onClick={() => setOpen(false)}>Cancel</button><button className="button button-primary" disabled={saving}>{saving ? "Saving…" : editing ? "Save changes" : "Add product"}</button></div></form></div>}
    {stock && <div className="modal-backdrop" onMouseDown={(e) => { if (e.target === e.currentTarget) setStock(null); }}><form className="edit-modal stock-modal" onSubmit={(e) => void adjustStock(e)}><button type="button" className="modal-close" onClick={() => setStock(null)} aria-label="Close"><X size={18}/></button><p className="eyebrow">INVENTORY MOVEMENT</p><h2>Adjust stock</h2><p className="modal-subtitle">{stock.name} currently has <strong>{stock.stockQuantity}</strong> units.</p><label className="field-label" htmlFor="stockDelta">Quantity change</label><input className="text-input" id="stockDelta" type="number" required value={delta} onChange={(e) => setDelta(e.target.value)}/><small className="field-hint">Use a negative number to remove stock.</small><label className="field-label stock-note-label" htmlFor="stockNote">Reason</label><input className="text-input" id="stockNote" required value={note} onChange={(e) => setNote(e.target.value)}/>{stockError && <p className="form-error" role="alert">{stockError}</p>}<div className="modal-actions"><button type="button" className="button button-secondary" onClick={() => setStock(null)}>Cancel</button><button className="button button-primary">Save adjustment</button></div></form></div>}
    {categoryOpen && <div className="modal-backdrop" onMouseDown={(e) => { if (e.target === e.currentTarget) setCategoryOpen(false); }}><form className="edit-modal stock-modal" onSubmit={(e) => void saveCategory(e)}><button type="button" className="modal-close" onClick={() => setCategoryOpen(false)} aria-label="Close"><X size={18}/></button><p className="eyebrow">PRODUCT CATALOGUE</p><h2>Add category</h2><p className="modal-subtitle">Group related products together for faster browsing at checkout.</p><label className="field-label" htmlFor="categoryName">Category name</label><input className="text-input" id="categoryName" required autoFocus value={categoryName} onChange={(e) => setCategoryName(e.target.value)} placeholder="e.g. Beverages"/><label className="field-label stock-note-label" htmlFor="categoryDescription">Description</label><input className="text-input" id="categoryDescription" value={categoryDescription} onChange={(e) => setCategoryDescription(e.target.value)} placeholder="Optional"/>{error && <p className="form-error" role="alert">{error}</p>}<div className="modal-actions"><button type="button" className="button button-secondary" onClick={() => setCategoryOpen(false)}>Cancel</button><button className="button button-primary">Save category</button></div></form></div>}
  </section>;
}

function parseMinor(value: string) { const n = Number(value.replace(/,/g, "")); return Number.isFinite(n) && n > 0 ? Math.round(n * 100) : 0; }
function formatInputMinor(value: number) { return (value / 100).toFixed(2); }
function createBarcode(used: Set<string>) {
  let value: string;
  do {
    const suffix = typeof crypto.randomUUID === "function"
      ? crypto.randomUUID().replaceAll("-", "").slice(0, 12).toUpperCase()
      : `${Date.now().toString(36)}${Math.floor(Math.random() * 0x100000000).toString(36)}`.toUpperCase();
    value = `APC-${suffix}`;
  } while (used.has(value));
  return value;
}

function createSku(used: Set<string>) {
  let sku: string;
  do {
    const suffix = typeof crypto.randomUUID === "function"
      ? crypto.randomUUID().replaceAll("-", "").slice(0, 8).toUpperCase()
      : `${Date.now().toString(36)}${Math.floor(Math.random() * 0x10000).toString(36)}`.toUpperCase();
    sku = `SKU-${suffix}`;
  } while (used.has(sku));
  return sku;
}
