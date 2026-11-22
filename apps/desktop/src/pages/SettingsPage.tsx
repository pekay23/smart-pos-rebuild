// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState } from "react";
import { Accessibility, Check, Expand, Monitor, Projector, Save, Type } from "lucide-react";
import { posApi } from "../lib/api";
import { StaffManagementPanel } from "../components/StaffManagementPanel";
import { listDisplayTargets, openCustomerDisplay, type DisplayTarget } from "../lib/customerDisplay";
import type { SessionUser, StorePreferences } from "../lib/models";
import { PaymentSettingsPanel } from "../components/PaymentSettingsPanel";
import { BackupSettingsPanel } from "../components/BackupSettingsPanel";
import { StationSettingsPanel } from "../components/StationSettingsPanel";
import { StoreProfileSettingsPanel } from "../components/StoreProfileSettingsPanel";
import { CashLocationSettingsPanel } from "../components/CashLocationSettingsPanel";

const defaultPreferences: StorePreferences = {
  cashierStartAtRegister: true,
  cashierAutoFullscreen: true,
  customerDisplayEnabled: false,
  customerDisplayFullscreen: false,
  customerDisplayMonitorKey: null,
  customerDisplayIdleMessage: "Welcome to our store.",
  receiptHeaderText: "",
  receiptFooterText: "Thank you for shopping with us.",
  receiptPaperWidthMm: 80,
};

export function SettingsPage({
  user,
  zoomPercent,
  onZoomChange,
  onSessionChange,
}: {
  user: SessionUser;
  zoomPercent: number;
  onZoomChange: (value: number) => void;
  onSessionChange: (user: SessionUser) => void;
}) {
  const [preferences, setPreferences] = useState<StorePreferences>(defaultPreferences);
  const [displays, setDisplays] = useState<DisplayTarget[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [openingDisplay, setOpeningDisplay] = useState(false);
  const [message, setMessage] = useState("");
  const [receiptMessage, setReceiptMessage] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    let active = true;
    void Promise.all([posApi.storePreferences(), listDisplayTargets()]).then(([stored, monitors]) => {
      if (!active) return;
      setPreferences(stored);
      setDisplays(monitors);
    }).catch((reason) => {
      if (active) setError(typeof reason === "string" ? reason : "Settings could not be loaded.");
    }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);

  function update<K extends keyof StorePreferences>(key: K, value: StorePreferences[K]) {
    setPreferences((current) => ({ ...current, [key]: value }));
    setMessage("");
    setError("");
  }

  async function save() {
    setSaving(true);
    setMessage("");
    setError("");
    try {
      setPreferences(await posApi.saveStorePreferences(preferences));
      setMessage("Settings saved for this store.");
    } catch (reason) {
      setError(typeof reason === "string" ? reason : "Settings could not be saved.");
    } finally {
      setSaving(false);
    }
  }

  async function previewDisplay() {
    setOpeningDisplay(true);
    setMessage("");
    setError("");
    try {
      const saved = await posApi.saveStorePreferences(preferences);
      setPreferences(saved);
      await openCustomerDisplay(saved);
      setMessage("Customer display opened. Its screen shows the live basket during checkout.");
    } catch (reason) {
      setError(typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "Customer display could not be opened.");
    } finally {
      setOpeningDisplay(false);
    }
  }

  async function saveReceiptSettings() {
    setSaving(true);
    setError("");
    setReceiptMessage("");
    try {
      setPreferences(await posApi.saveStorePreferences(preferences));
      setReceiptMessage("Receipt layout saved. Print opens the system dialog for the selected paper width.");
    } catch (reason) {
      setError(typeof reason === "string" ? reason : "Receipt settings could not be saved.");
    } finally {
      setSaving(false);
    }
  }

  const hasSeparateScreen = displays.some((display) => !display.isPrimary);

  return <section className="settings-page">
    <div className="page-heading"><div><p className="eyebrow">STORE SETUP</p><h1>Settings</h1><p className="page-subtitle">Set up the register for {user.storeName}, its staff, and the customer screen.</p></div></div>

    {user.permissions.includes("settings:write") && <StoreProfileSettingsPanel onSessionChange={onSessionChange}/>}
    {user.permissions.includes("settings:write") && <CashLocationSettingsPanel/>}

    {user.permissions.includes("settings:write") && <article className="settings-panel">
      <div className="settings-panel-heading"><span className="settings-icon settings-icon-blue"><Monitor size={19}/></span><div><h2>Receipt and printer</h2><p>Set a receipt header/footer and choose the paper roll width used by the print layout.</p></div></div>
      <label className="field-label settings-field-label" htmlFor="receipt-paper-width">Receipt paper width</label>
      <select id="receipt-paper-width" className="text-input settings-select" value={preferences.receiptPaperWidthMm} onChange={(event) => setPreferences((current) => ({ ...current, receiptPaperWidthMm: Number(event.target.value) as 58 | 80 }))}><option value={58}>58 mm thermal roll</option><option value={80}>80 mm thermal roll</option></select>
      <label className="field-label settings-field-label" htmlFor="receipt-header">Receipt header<textarea id="receipt-header" className="text-input" rows={2} maxLength={160} value={preferences.receiptHeaderText} onChange={(event) => setPreferences((current) => ({ ...current, receiptHeaderText: event.target.value }))} placeholder="Optional welcome, registration, or tax line"/></label>
      <label className="field-label settings-field-label" htmlFor="receipt-footer">Receipt footer<textarea id="receipt-footer" className="text-input" rows={2} maxLength={240} value={preferences.receiptFooterText} onChange={(event) => setPreferences((current) => ({ ...current, receiptFooterText: event.target.value }))} placeholder="Thank you for shopping with us."/></label>
      <p className="settings-hint">Print opens the operating system’s printer dialog; supported printer models and direct Bluetooth/USB printing still depend on their driver or approved device integration.</p>
      <div className="settings-actions"><button className="button button-primary" disabled={saving || loading} onClick={() => void saveReceiptSettings()}><Save size={16}/>{saving ? "Saving…" : "Save receipt settings"}</button></div>
      {receiptMessage && <p className="settings-success" role="status">{receiptMessage}</p>}
      {error && <p className="form-error" role="alert">{error}</p>}
    </article>}

    <article className="settings-panel">
      <div className="settings-panel-heading"><span className="settings-icon"><Accessibility size={19}/></span><div><h2>Cashier workspace</h2><p>Keep checkout ready and comfortable for each staff member.</p></div></div>
      <label className="setting-row"><span className="setting-copy"><strong>Open cashiers at the register</strong><small>Cashier accounts skip the overview and start with Point of sale.</small></span><input type="checkbox" checked={preferences.cashierStartAtRegister} onChange={(event) => update("cashierStartAtRegister", event.target.checked)}/></label>
      <label className="setting-row"><span className="setting-copy"><strong>Start cashier sessions full screen</strong><small>F11 or the on-screen control always returns to windowed mode.</small></span><input type="checkbox" checked={preferences.cashierAutoFullscreen} onChange={(event) => update("cashierAutoFullscreen", event.target.checked)}/></label>
      <div className="setting-row setting-zoom-row"><span className="setting-copy"><strong><Type size={15}/> Text and screen size</strong><small>Applies to this device. Keyboard zoom shortcuts are also available.</small></span><div className="zoom-control"><input type="range" min={80} max={160} step={10} value={zoomPercent} onChange={(event) => onZoomChange(Number(event.target.value))} aria-label="Application zoom"/><strong>{zoomPercent}%</strong><button className="button button-secondary button-small" onClick={() => onZoomChange(100)}>Reset</button></div></div>
      <p className="settings-hint"><Expand size={15}/> Use <kbd>F11</kbd> to switch between full screen and windowed checkout.</p>
    </article>

    <article className="settings-panel">
      <div className="settings-panel-heading"><span className="settings-icon settings-icon-blue"><Projector size={19}/></span><div><h2>Customer display</h2><p>Project a clean basket and payment summary on a second screen.</p></div></div>
      <label className="setting-row"><span className="setting-copy"><strong>Enable customer display</strong><small>Open it when a cashier starts the register. Only basket details are mirrored.</small></span><input type="checkbox" checked={preferences.customerDisplayEnabled} onChange={(event) => update("customerDisplayEnabled", event.target.checked)}/></label>
      <label className="field-label settings-field-label" htmlFor="customer-display-monitor">Customer screen</label>
      <select id="customer-display-monitor" className="text-input settings-select" value={preferences.customerDisplayMonitorKey ?? ""} onChange={(event) => update("customerDisplayMonitorKey", event.target.value || null)}>
        <option value="">Choose automatically</option>
        {displays.map((display) => <option key={display.key} value={display.key}>{display.label}</option>)}
      </select>
      {!hasSeparateScreen && <p className="settings-hint">Only one screen is connected. The preview opens in a window; connect a second display before enabling full screen.</p>}
      <label className="setting-row"><span className="setting-copy"><strong>Use full screen on the customer display</strong><small>Requires a separate screen so checkout stays visible to the cashier.</small></span><input type="checkbox" checked={preferences.customerDisplayFullscreen} disabled={!hasSeparateScreen || !preferences.customerDisplayEnabled} onChange={(event) => update("customerDisplayFullscreen", event.target.checked)}/></label>
      <label className="field-label settings-field-label" htmlFor="customer-display-greeting">Idle greeting</label>
      <input id="customer-display-greeting" className="text-input settings-select" maxLength={160} value={preferences.customerDisplayIdleMessage} onChange={(event) => update("customerDisplayIdleMessage", event.target.value)} />
      <div className="settings-actions"><button className="button button-secondary" disabled={openingDisplay} onClick={() => void previewDisplay()}><Monitor size={16}/>{openingDisplay ? "Opening display…" : "Preview customer display"}</button><button className="button button-primary" disabled={saving || loading} onClick={() => void save()}><Save size={16}/>{saving ? "Saving…" : "Save settings"}</button></div>
      {message && <p className="settings-success" role="status"><Check size={16}/>{message}</p>}
      {error && <p className="form-error" role="alert">{error}</p>}
      {loading && <p className="settings-hint">Loading device and store settings…</p>}
    </article>
    {user.permissions.includes("users:write") && <StaffManagementPanel currentUser={user} onSessionChange={onSessionChange}/>}
    {user.permissions.includes("settings:write") && <StationSettingsPanel/>}
    <BackupSettingsPanel/>
    <PaymentSettingsPanel/>
  </section>;
}
