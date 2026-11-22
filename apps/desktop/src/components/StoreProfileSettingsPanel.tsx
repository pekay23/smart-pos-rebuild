// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState, type FormEvent } from "react";
import { Building2, Save } from "lucide-react";
import { posApi } from "../lib/api";
import type { SessionUser, StoreProfile } from "../lib/models";
import { getErrorMessage } from "../lib/errors";

const blankProfile: StoreProfile = {
  businessName: "",
  storeName: "",
  address: "",
  phone: "",
  currencyCode: "GHS",
  taxRateBps: 0,
};

export function StoreProfileSettingsPanel({ onSessionChange }: { onSessionChange: (user: SessionUser) => void }) {
  const [profile, setProfile] = useState(blankProfile);
  const [taxPercent, setTaxPercent] = useState("0.00");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    let active = true;
    void posApi.storeProfile().then((value) => {
      if (active) {
        setProfile(value);
        setTaxPercent((value.taxRateBps / 100).toFixed(2));
      }
    }).catch((reason: unknown) => {
      if (active) setError(getErrorMessage(reason, "Store profile could not be loaded."));
    }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);

  function update<K extends keyof StoreProfile>(key: K, value: StoreProfile[K]) {
    setProfile((current) => ({ ...current, [key]: value }));
    setError("");
    setMessage("");
  }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSaving(true);
    setError("");
    setMessage("");
    try {
      const submitted = { ...profile, taxRateBps: Math.round(Number(taxPercent) * 100) };
      const session = await posApi.saveStoreProfile(submitted);
      onSessionChange(session);
      setProfile({ ...submitted, currencyCode: session.currencyCode });
      setMessage("Business profile and tax settings saved. New sales will use this tax rate.");
    } catch (reason) {
      setError(getErrorMessage(reason, "Business profile could not be saved."));
    } finally {
      setSaving(false);
    }
  }

  return <article className="settings-panel">
    <div className="settings-panel-heading"><span className="settings-icon"><Building2 size={19}/></span><div><h2>Business and store profile</h2><p>Control the receipt identity, operating currency, and tax added to new sales.</p></div></div>
    <form className="account-form" onSubmit={(event) => void save(event)}>
      <div className="two-col">
        <label className="field-label">Business name<input className="text-input" value={profile.businessName} onChange={(event) => update("businessName", event.target.value)} maxLength={120} required/></label>
        <label className="field-label">Store or branch name<input className="text-input" value={profile.storeName} onChange={(event) => update("storeName", event.target.value)} maxLength={120} required/></label>
        <label className="field-label">Store phone<input className="text-input" value={profile.phone} onChange={(event) => update("phone", event.target.value)} maxLength={40} autoComplete="tel"/></label>
        <label className="field-label">Currency code<input className="text-input" value={profile.currencyCode} onChange={(event) => update("currencyCode", event.target.value.toUpperCase())} minLength={3} maxLength={3} pattern="[A-Za-z]{3}" required/><small>Use the ISO 4217 code, such as GHS, USD, or NGN.</small></label>
        <label className="field-label form-grid-span">Store address<textarea className="text-input" rows={2} value={profile.address} onChange={(event) => update("address", event.target.value)} maxLength={240}/></label>
        <label className="field-label">Sales tax rate (%)<input className="text-input" type="number" min="0" max="100" step="0.01" value={taxPercent} onChange={(event) => { setTaxPercent(event.target.value); setError(""); setMessage(""); }}/><small>Saved receipts keep their original tax. This applies to future sales.</small></label>
      </div>
      <p className="settings-hint">Currency can change only before products or financial activity are recorded, so existing prices and accounting records are never relabelled.</p>
      <div className="settings-actions"><button className="button button-primary" disabled={saving || loading}>{saving ? "Saving profile…" : <><Save size={16}/>Save business settings</>}</button></div>
    </form>
    {loading && <p className="settings-hint">Loading the current business profile…</p>}
    {message && <p className="settings-success" role="status">{message}</p>}
    {error && <p className="form-error" role="alert">{error}</p>}
  </article>;
}
