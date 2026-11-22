// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState, type FormEvent } from "react";
import { Copy, Send } from "lucide-react";
import { paymentApi, type PaymentIntegrationRequest, type PaymentOption } from "../lib/paymentApi";
import { getErrorMessage } from "../lib/errors";

const emptyOption: PaymentOption = { id: null, name: "", kind: "card", provider: "", merchantAccount: "", terminalId: "", instructions: "", referenceRequired: true, enabled: true };
const emptyRequest = { country: "", providerName: "", terminalModel: "", capability: "amount_and_approval", details: "" };

function requestExport(request: PaymentIntegrationRequest) {
  return JSON.stringify({
    kind: "payment_integration_request",
    version: 1,
    request: {
      country: request.country,
      provider: request.providerName,
      terminalModel: request.terminalModel || null,
      requestedCapability: request.capability,
      details: request.details || null,
      createdAt: request.createdAt,
      localStatus: request.status,
    },
  }, null, 2);
}

export function PaymentSettingsPanel() {
  const [options, setOptions] = useState<PaymentOption[]>([]);
  const [requests, setRequests] = useState<PaymentIntegrationRequest[]>([]);
  const [form, setForm] = useState<PaymentOption>(emptyOption);
  const [requestForm, setRequestForm] = useState(emptyRequest);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [requestBusy, setRequestBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [requestMessage, setRequestMessage] = useState("");

  useEffect(() => {
    void Promise.all([paymentApi.options(), paymentApi.integrationRequests()])
      .then(([savedOptions, savedRequests]) => { setOptions(savedOptions); setRequests(savedRequests); })
      .catch((reason: unknown) => setError(getErrorMessage(reason, "Payment settings could not be loaded.")));
  }, []);

  async function saveRequest(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setRequestBusy(true);
    setError("");
    setRequestMessage("");
    try {
      const saved = await paymentApi.saveIntegrationRequest(requestForm);
      setRequests((current) => [saved, ...current]);
      setRequestForm(emptyRequest);
      setRequestMessage("Saved on this device. It has not been sent to the developer because the support service is not connected yet.");
    } catch (reason) {
      setError(getErrorMessage(reason, "The integration request could not be saved."));
    } finally {
      setRequestBusy(false);
    }
  }

  async function copyRequest(request: PaymentIntegrationRequest) {
    try {
      await navigator.clipboard.writeText(requestExport(request));
      setRequestMessage("Request copied. Review the free-text details before sharing; the export adds no store ID, account number, or payment credentials.");
    } catch {
      setRequestMessage("Clipboard access is unavailable on this device. The request remains saved locally.");
    }
  }

  return <>
    <article className="settings-panel"><h2>Customer payment methods</h2>
      <p>Add your bank POS terminal, merchant mobile-money account, cash, or another external payment method. The cashier confirms payment and records its reference. These methods do not automatically charge or verify payments.</p>
      <p className="settings-hint">Once you add a method, only your configured enabled methods appear at checkout. Add Cash too if you accept it. Provider API keys and card data must not be entered here.</p>
      <div className="account-key-list">{options.map((option) => <div className="setting-row" key={option.id}><span className="setting-copy"><strong>{option.name}</strong><small>{option.provider} {option.terminalId} · {option.enabled ? "Enabled" : "Disabled"} · Manual confirmation</small></span><button className="button button-secondary button-small" onClick={() => { setForm(option); setMessage(""); }}>Edit</button></div>)}</div>
      <form className="account-form" onSubmit={(event) => { event.preventDefault(); setBusy(true); setError(""); setMessage(""); void paymentApi.save(form).then(async () => { setOptions(await paymentApi.options()); setForm(emptyOption); setMessage("Payment method saved."); }).catch((reason: unknown) => setError(getErrorMessage(reason, "Payment method could not be saved."))).finally(() => setBusy(false)); }}>
        <label className="field-label">Checkout name<input className="text-input" value={form.name} onChange={(event) => setForm({ ...form, name: event.target.value })} placeholder="Bank card terminal 1" maxLength={80} required/></label>
        <label className="field-label">Method<select className="text-input" value={form.kind} onChange={(event) => { const kind = event.target.value; if (kind === "cash" || kind === "card" || kind === "mobile_money" || kind === "other") setForm({ ...form, kind, referenceRequired: kind !== "cash" }); }}><option value="cash">Cash</option><option value="card">Bank / card terminal</option><option value="mobile_money">Mobile money</option><option value="other">Bank transfer / other</option></select></label>
        <label className="field-label">Bank or provider<input className="text-input" value={form.provider} onChange={(event) => setForm({ ...form, provider: event.target.value })} maxLength={120}/></label>
        <label className="field-label">Merchant number / payment account<input className="text-input" value={form.merchantAccount} onChange={(event) => setForm({ ...form, merchantAccount: event.target.value })} maxLength={120}/></label>
        <label className="field-label">Terminal ID / nickname<input className="text-input" value={form.terminalId} onChange={(event) => setForm({ ...form, terminalId: event.target.value })} maxLength={120}/></label>
        <label className="field-label">Instructions<input className="text-input" value={form.instructions} onChange={(event) => setForm({ ...form, instructions: event.target.value })} maxLength={500}/></label>
        <label className="account-checkbox"><input type="checkbox" checked={form.referenceRequired} onChange={(event) => setForm({ ...form, referenceRequired: event.target.checked })}/>Require the approved transaction reference</label>
        <label className="account-checkbox"><input type="checkbox" checked={form.enabled} onChange={(event) => setForm({ ...form, enabled: event.target.checked })}/>Enabled at checkout</label>
        <div className="settings-actions"><button className="button button-secondary" type="button" onClick={() => setForm(emptyOption)}>New method</button><button className="button button-primary" disabled={busy}>{busy ? "Saving…" : "Save method"}</button></div>
      </form>{error && <p className="form-error" role="alert">{error}</p>}{message && <p className="settings-success" role="status">{message}</p>}
    </article>
    <article className="settings-panel payment-request-panel"><div className="settings-panel-heading"><span className="settings-icon settings-icon-blue"><Send size={18}/></span><div><h2>Request payment integration</h2><p>Tell the product team which country, bank or provider, and terminal capability your store needs.</p></div></div>
      <p className="settings-hint">Do not enter API keys, card numbers, PINs, or customer data. Requests are stored on this device for now; developer inbox delivery becomes available with the cloud support service.</p>
      <form className="account-form" onSubmit={(event) => void saveRequest(event)}>
        <label className="field-label">Country<input className="text-input" value={requestForm.country} onChange={(event) => setRequestForm({ ...requestForm, country: event.target.value })} placeholder="Ghana" maxLength={80} required/></label>
        <label className="field-label">Bank or payment provider<input className="text-input" value={requestForm.providerName} onChange={(event) => setRequestForm({ ...requestForm, providerName: event.target.value })} placeholder="Bank, Paystack, or processor" maxLength={120} required/></label>
        <label className="field-label">Terminal model (if applicable)<input className="text-input" value={requestForm.terminalModel} onChange={(event) => setRequestForm({ ...requestForm, terminalModel: event.target.value })} placeholder="Model printed on the terminal" maxLength={120}/></label>
        <label className="field-label">What should the integration do?<select className="text-input" value={requestForm.capability} onChange={(event) => setRequestForm({ ...requestForm, capability: event.target.value })}><option value="amount_and_approval">Send amount and receive approval or decline</option><option value="payment_status">Check payment status</option><option value="refund">Issue or confirm refunds</option><option value="reconciliation">Settlement and reconciliation</option><option value="other">Other integration capability</option></select></label>
        <label className="field-label">Extra detail<textarea className="text-input" rows={3} value={requestForm.details} onChange={(event) => setRequestForm({ ...requestForm, details: event.target.value })} placeholder="Describe the workflow and any public product documentation." maxLength={1000}/></label>
        <div className="settings-actions"><button className="button button-primary" disabled={requestBusy}>{requestBusy ? "Saving request…" : "Save request on this device"}</button></div>
      </form>
      {requestMessage && <p className="settings-success" role="status">{requestMessage}</p>}
      {requests.length > 0 && <div className="account-key-list payment-request-list"><h3>Saved integration requests</h3>{requests.map((request) => <div className="setting-row" key={request.id}><span className="setting-copy"><strong>{request.providerName} · {request.country}</strong><small>{request.terminalModel || "No terminal model"} · {request.capability.replaceAll("_", " ")} · Saved locally</small></span><button className="button button-secondary button-small" type="button" onClick={() => void copyRequest(request)}><Copy size={14}/>Copy request</button></div>)}</div>}
      {error && <p className="form-error" role="alert">{error}</p>}
    </article>
  </>;
}
