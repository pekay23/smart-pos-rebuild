// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { formatMoney } from "../lib/money";
import type { CustomerDisplayCart } from "../lib/customerDisplay";
import brandMark from "../assets/nimbletill-mark.svg";

const emptyCart: CustomerDisplayCart = {
  businessName: "Advanced POS",
  storeName: "",
  currencyCode: "GHS",
  lines: [],
  subtotalMinor: 0,
  discountMinor: 0,
  discountPendingApproval: false,
  totalMinor: 0,
  status: "idle",
  receiptNumber: null,
};

export function CustomerDisplayPage() {
  const idleMessage = new URLSearchParams(window.location.search).get("idle") || "Welcome to our store.";
  const [cart, setCart] = useState<CustomerDisplayCart>(emptyCart);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void listen<CustomerDisplayCart>("nimbletill:customer-display-cart", (event) => setCart(event.payload)).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    });
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") void getCurrentWindow().setFullscreen(false);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => { disposed = true; unlisten?.(); window.removeEventListener("keydown", onKeyDown); };
  }, []);

  const isIdle = cart.status === "idle";
  const isPaid = cart.status === "paid";

  return <main className="customer-display-page">
    <header className="customer-display-header"><span className="brand-mark"><img src={brandMark} alt=""/></span><div><strong>{cart.businessName}</strong><small>{cart.storeName || "Customer display"}</small></div><span className="customer-display-live"><i/>{isIdle ? "READY" : isPaid ? "PAYMENT COMPLETE" : "CURRENT SALE"}</span></header>
    {isIdle ? <section className="customer-display-idle"><span className="customer-display-symbol">✳</span><p>{idleMessage}</p><small>Your items will appear here as they are added.</small></section> : isPaid ? <section className="customer-display-thanks"><span className="customer-display-symbol">✓</span><p>Thank you for shopping with us</p><strong>{cart.receiptNumber ? `Receipt ${cart.receiptNumber}` : "Payment complete"}</strong></section> : <>
      <section className="customer-display-items" aria-label="Items in your basket">{cart.lines.map((line, index) => <article className="customer-display-line" key={`${line.name}-${index}`}><span className="customer-display-quantity">{line.quantity}×</span><span className="customer-display-product"><strong>{line.name}</strong><small>{formatMoney(line.unitPriceMinor, cart.currencyCode)} each</small></span><strong className="customer-display-line-total">{formatMoney(line.lineTotalMinor, cart.currencyCode)}</strong></article>)}</section>
      {cart.discountPendingApproval && <p className="customer-discount-pending">A requested discount is being reviewed. The total below does not include it.</p>}<section className="customer-display-total"><div><span>Subtotal</span><strong>{formatMoney(cart.subtotalMinor, cart.currencyCode)}</strong></div>{cart.discountMinor > 0 && <div><span>Discount</span><strong>−{formatMoney(cart.discountMinor, cart.currencyCode)}</strong></div>}<div className="customer-display-grand"><span>Total due</span><strong>{formatMoney(cart.totalMinor, cart.currencyCode)}</strong></div></section>
      {cart.paymentInstructions && <section className="customer-payment-instructions"><strong>{cart.paymentInstructions.name}</strong><p>{cart.paymentInstructions.provider} {cart.paymentInstructions.account}</p><p>{cart.paymentInstructions.instructions}</p><small>Please let the cashier confirm your payment.</small></section>}
    </>}
    <footer className="customer-display-footer">Thank you for supporting {cart.businessName}</footer>
  </main>;
}
