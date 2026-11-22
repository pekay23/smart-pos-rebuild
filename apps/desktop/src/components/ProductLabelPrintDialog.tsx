// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useMemo, useRef, useState } from "react";
import { Printer, X } from "lucide-react";
import JsBarcode from "jsbarcode";
import type { Product } from "../lib/models";
import { formatMoney } from "../lib/money";

const labelSizes = {
  compact: { label: "40 × 25 mm", width: 40, height: 25 },
  standard: { label: "50 × 30 mm", width: 50, height: 30 },
  large: { label: "60 × 40 mm", width: 60, height: 40 },
} as const;
type LabelSize = keyof typeof labelSizes;

export function ProductLabelPrintDialog({ products, currencyCode, onClose }: { products: Product[]; currencyCode: string; onClose: () => void }) {
  const [copies, setCopies] = useState(1);
  const [size, setSize] = useState<LabelSize>("standard");
  const [showPrice, setShowPrice] = useState(true);
  const settings = labelSizes[size];
  const labels = useMemo(() => Array.from({ length: copies }, () => products).flat(), [copies, products]);
  const skuFallbacks = products.filter((product) => !product.barcode?.trim()).length;

  return <div className="modal-backdrop product-label-print-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <style>{`@media print{@page{size:${settings.width}mm ${settings.height}mm;margin:0}html,body{width:${settings.width}mm!important;min-width:${settings.width}mm!important;height:auto!important;background:#fff!important}body *{visibility:hidden!important}.product-label-print-backdrop,.product-label-print-backdrop *{visibility:visible!important}.product-label-print-backdrop{position:absolute!important;inset:0!important;display:block!important;padding:0!important;background:#fff!important}.product-label-print-dialog{display:none!important}.product-label-print-sheet{display:block!important}.product-label-print-item{width:${settings.width}mm!important;height:${settings.height}mm!important;padding:2mm!important;break-after:page;page-break-after:always;overflow:hidden}.product-label-print-item:last-child{break-after:auto;page-break-after:auto}.product-label-print-item svg{max-width:100%!important;max-height:14mm!important}}`}</style>
    <section className="edit-modal product-label-print-dialog" role="dialog" aria-modal="true" aria-labelledby="label-print-title">
      <button type="button" className="modal-close" onClick={onClose} aria-label="Close"><X size={18}/></button>
      <p className="eyebrow">INVENTORY LABELS</p>
      <h2 id="label-print-title">Print product labels</h2>
      <p className="modal-subtitle">Print {products.length} selected product{products.length === 1 ? "" : "s"} using your system’s configured printer.</p>
      <div className="form-grid two-col product-label-print-options">
        <div><label className="field-label" htmlFor="productLabelSize">Label size</label><select className="text-input" id="productLabelSize" value={size} onChange={(event) => setSize(event.target.value as LabelSize)}>{Object.entries(labelSizes).map(([key, value]) => <option key={key} value={key}>{value.label}</option>)}</select></div>
        <div><label className="field-label" htmlFor="productLabelCopies">Copies per product</label><input className="text-input" id="productLabelCopies" type="number" min="1" max="100" step="1" value={copies} onChange={(event) => setCopies(Math.min(100, Math.max(1, Number(event.target.value) || 1)))}/></div>
        <label className="checkbox-field form-grid-span"><input type="checkbox" checked={showPrice} onChange={(event) => setShowPrice(event.target.checked)}/><span>Show selling price on labels</span></label>
      </div>
      {skuFallbacks > 0 && <p className="field-hint">{skuFallbacks} selected item{skuFallbacks === 1 ? "" : "s"} have no barcode; the SKU will be printed as a scannable Code 128 barcode.</p>}
      <div className="product-label-preview" aria-label="Label preview">
        {products.slice(0, 3).map((product) => <ProductLabel key={product.id} product={product} showPrice={showPrice} currencyCode={currencyCode} size={settings}/>)}
        {products.length > 3 && <span className="product-label-preview-more">+{products.length - 3} more products</span>}
      </div>
      <div className="modal-actions"><button type="button" className="button button-secondary" onClick={onClose}>Cancel</button><button type="button" className="button button-primary" onClick={() => window.print()}><Printer size={16}/> Print {labels.length} labels</button></div>
    </section>
    <div className="product-label-print-sheet" aria-hidden="true">
      {labels.map((product, index) => <div className="product-label-print-item" key={`${product.id}:${index}`}><ProductLabel product={product} showPrice={showPrice} currencyCode={currencyCode} size={settings}/></div>)}
    </div>
  </div>;
}

function ProductLabel({ product, showPrice, currencyCode, size }: { product: Product; showPrice: boolean; currencyCode: string; size: { width: number; height: number } }) {
  const svg = useRef<SVGSVGElement>(null);
  const value = product.barcode?.trim() || product.sku;
  useEffect(() => {
    if (!svg.current) return;
    try {
      JsBarcode(svg.current, value, { format: "CODE128", displayValue: true, font: "Arial", fontSize: 10, textMargin: 1, margin: 0, height: Math.max(24, Math.round(size.height * 0.42)), width: size.width > 50 ? 1.8 : 1.4 });
    } catch {
      svg.current.replaceChildren();
    }
  }, [size.height, size.width, value]);

  return <div className="product-label-content"><strong className="product-label-name">{product.name}</strong>{product.brandName && <small className="product-label-brand">{product.brandName}</small>}<svg ref={svg} className="product-label-barcode" role="img" aria-label={`Barcode ${value}`}/>{showPrice && <strong className="product-label-price">{formatMoney(product.priceMinor, currencyCode)}</strong>}</div>;
}
