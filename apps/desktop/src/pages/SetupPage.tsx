// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useState, type FormEvent } from "react";
import { ArrowRight, LockKeyhole, Store, Upload } from "lucide-react";
import { AuthFrame } from "../components/AuthFrame";
import { getErrorMessage } from "../lib/errors";
import { posApi } from "../lib/api";
import type { SessionUser } from "../lib/models";

export function SetupPage({ onComplete }: { onComplete: (user: SessionUser) => void }) {
  const [businessName, setBusinessName] = useState("");
  const [storeName, setStoreName] = useState("");
  const [fullName, setFullName] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [currencyCode, setCurrencyCode] = useState("GHS");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [restorePassword, setRestorePassword] = useState("");
  const [restoreError, setRestoreError] = useState("");

  async function loadDemoWorkspace() {
    setBusy(true);
    setError("");
    try {
      const user = await posApi.seedDemoWorkspace();
      onComplete(user);
    } catch (reason: unknown) {
      setError(getErrorMessage(reason, "The demo workspace could not be loaded."));
    } finally {
      setBusy(false);
    }
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (password !== confirmPassword) {
      setError("The passwords do not match.");
      return;
    }
    setBusy(true);
    setError("");
    try {
      const user = await posApi.setupStore({ businessName, storeName, fullName, username, password, currencyCode });
      onComplete(user);
    } catch (reason: unknown) {
      setError(getErrorMessage(reason, "Setup could not be completed. Check the details and try again."));
    } finally {
      setBusy(false);
    }
  }

  async function restorePortable() {
    if (restorePassword.length < 12) {
      setRestoreError("Enter the passphrase used when the backup was exported.");
      return;
    }
    if (!window.confirm("Restore this business onto this device? The encrypted backup replaces the empty local workspace.")) return;
    setBusy(true);
    setRestoreError("");
    try {
      if (await posApi.restorePortableBackup(restorePassword)) window.location.reload();
    } catch (reason: unknown) {
      setRestoreError(getErrorMessage(reason, "The encrypted backup could not be restored."));
    } finally {
      setRestorePassword("");
      setBusy(false);
    }
  }

  return (
    <AuthFrame
      eyebrow="FIRST-TIME SETUP"
      title="Set up your store"
      description="Create your local store and administrator account. You can add staff and products after setup."
    >
      <form className="auth-form setup-form" onSubmit={(event) => void submit(event)}>
        <div className="form-grid two-col">
          <div>
            <label className="field-label" htmlFor="businessName">Business name</label>
            <div className="input-with-icon"><Store size={17} aria-hidden="true" /><input id="businessName" value={businessName} onChange={(event) => setBusinessName(event.target.value)} required placeholder="Your business" /></div>
          </div>
          <div>
            <label className="field-label" htmlFor="storeName">Store / location</label>
            <input className="text-input" id="storeName" value={storeName} onChange={(event) => setStoreName(event.target.value)} required placeholder="Main shop" />
          </div>
          <div>
            <label className="field-label" htmlFor="fullName">Administrator name</label>
            <input className="text-input" id="fullName" value={fullName} onChange={(event) => setFullName(event.target.value)} required autoComplete="name" placeholder="Your name" />
          </div>
          <div>
            <label className="field-label" htmlFor="currency">Currency code</label>
            <input className="text-input currency-input" id="currency" value={currencyCode} onChange={(event) => setCurrencyCode(event.target.value.toUpperCase())} required minLength={3} maxLength={3} placeholder="GHS" />
          </div>
        </div>
        <div className="form-divider" />
        <div className="form-grid two-col">
          <div>
            <label className="field-label" htmlFor="newUsername">Admin username</label>
            <input className="text-input" id="newUsername" value={username} onChange={(event) => setUsername(event.target.value)} required minLength={3} autoComplete="username" placeholder="Choose a username" />
          </div>
          <div>
            <label className="field-label" htmlFor="newPassword">Password</label>
            <div className="input-with-icon"><LockKeyhole size={17} aria-hidden="true" /><input id="newPassword" type="password" value={password} onChange={(event) => setPassword(event.target.value)} required minLength={12} autoComplete="new-password" placeholder="At least 12 characters" /></div>
          </div>
          <div className="form-grid-span">
            <label className="field-label" htmlFor="confirmPassword">Confirm password</label>
            <input className="text-input" id="confirmPassword" type="password" value={confirmPassword} onChange={(event) => setConfirmPassword(event.target.value)} required minLength={12} autoComplete="new-password" placeholder="Enter the same password again" />
          </div>
        </div>
        {error && <p className="form-error" role="alert">{error}</p>}
        <button className="button button-primary auth-submit" type="submit" disabled={busy}>
          {busy ? "Setting up…" : "Create store"}<ArrowRight size={17} />
        </button>
      </form>
      <p className="auth-note">Your store data is saved on this device.</p>
      <aside className="portable-setup-restore">
        <div><p className="eyebrow">MOVE AN EXISTING STORE</p><strong>Restore an encrypted backup</strong><p>Choose a portable backup file and its passphrase. You can use this on a new device before creating a local account.</p></div>
        <label className="field-label" htmlFor="portable-setup-passphrase">Backup passphrase</label>
        <input id="portable-setup-passphrase" className="text-input" type="password" autoComplete="current-password" minLength={12} maxLength={1024} value={restorePassword} onChange={(event) => setRestorePassword(event.target.value)} placeholder="At least 12 characters" />
        {restoreError && <p className="form-error" role="alert">{restoreError}</p>}
        <button type="button" className="button button-secondary" disabled={busy || restorePassword.length < 12} onClick={() => void restorePortable()}><Upload size={16}/>{busy ? "Checking backup…" : "Restore existing store"}</button>
      </aside>
      {import.meta.env.DEV && <aside className="demo-workspace-card">
        <div><p className="eyebrow">LOCAL REVIEW ONLY</p><strong>Load a sample store</strong><p>Creates demo inventory, one received delivery, one expense, and separate owner, administrator, manager, and cashier sign-ins in this empty local database.</p></div>
        <div className="demo-account-list"><span>Owner <code>owner / OwnerDemo2026!</code></span><span>Administrator <code>admin / AdminDemo2026!</code></span><span>Manager <code>manager / ManagerDemo2026!</code></span><span>Cashier <code>cashier / CashierDemo2026!</code></span></div>
        <button type="button" className="button button-secondary demo-seed-button" onClick={() => void loadDemoWorkspace()} disabled={busy}>{busy ? "Loading demo…" : "Create demo workspace"}</button>
      </aside>}
    </AuthFrame>
  );
}
