// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState } from "react";
import { ArchiveRestore, DatabaseBackup, Download, RefreshCw, RotateCcw, Upload } from "lucide-react";
import { posApi } from "../lib/api";
import type { LocalBackup } from "../lib/models";

function formatBytes(bytes: number) {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function BackupSettingsPanel() {
  const [backups, setBackups] = useState<LocalBackup[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [exportPassword, setExportPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [restorePassword, setRestorePassword] = useState("");
  const [retentionLimit, setRetentionLimit] = useState(0);
  const [selectedRetentionLimit, setSelectedRetentionLimit] = useState(0);

  async function refresh() {
    setLoading(true);
    setError("");
    try {
      const [rows, limit] = await Promise.all([posApi.listBackups(), posApi.getBackupRetentionLimit()]);
      setBackups(rows);
      setRetentionLimit(limit);
      setSelectedRetentionLimit(limit);
    }
    catch (reason) { setError(typeof reason === "string" ? reason : "Local backups could not be loaded."); }
    finally { setLoading(false); }
  }

  useEffect(() => {
    let active = true;
    void Promise.all([posApi.listBackups(), posApi.getBackupRetentionLimit()]).then(([result, limit]) => {
      if (active) {
        setBackups(result);
        setRetentionLimit(limit);
        setSelectedRetentionLimit(limit);
      }
    }).catch((reason) => {
      if (active) setError(typeof reason === "string" ? reason : "Local backups could not be loaded.");
    }).finally(() => {
      if (active) setLoading(false);
    });
    return () => { active = false; };
  }, []);

  async function create() {
    setBusy(true); setError(""); setMessage("");
    try {
      const backup = await posApi.createBackup();
      setBackups(await posApi.listBackups());
      setMessage(backup.retentionWarning ?? "Verified local backup created on this device.");
    } catch (reason) { setError(typeof reason === "string" ? reason : "Backup could not be created."); }
    finally { setBusy(false); }
  }

  async function saveRetention() {
    setBusy(true); setError(""); setMessage("");
    try {
      const saved = await posApi.saveBackupRetentionLimit(selectedRetentionLimit);
      setRetentionLimit(saved);
      setSelectedRetentionLimit(saved);
      setMessage(saved === 0
        ? "Retention set to keep all local backups."
        : `Retention set to keep the latest ${saved} regular backups after each new backup.`);
    } catch (reason) {
      setSelectedRetentionLimit(retentionLimit);
      setError(typeof reason === "string" ? reason : "Backup retention could not be saved.");
    } finally { setBusy(false); }
  }

  async function restore(backup: LocalBackup) {
    const when = new Date(backup.createdAt).toLocaleString();
    if (!window.confirm(`Restore the entire local business database from ${when}? Current data will be replaced. Advanced POS will make a safety backup first and ask you to sign in again.`)) return;
    setBusy(true); setError(""); setMessage("");
    try {
      await posApi.restoreBackup(backup.id);
      window.location.reload();
    } catch (reason) { setError(typeof reason === "string" ? reason : "Restore could not be completed."); }
    finally { setBusy(false); }
  }

  async function exportPortable() {
    if (exportPassword.length < 12) { setError("Use a passphrase with at least 12 characters."); return; }
    if (exportPassword !== confirmPassword) { setError("The passphrases do not match."); return; }
    setBusy(true); setError(""); setMessage("");
    try {
      const exported = await posApi.exportPortableBackup(exportPassword);
      if (exported) setMessage("Encrypted portable backup saved. Keep the passphrase separate; it cannot be recovered.");
    } catch (reason) { setError(typeof reason === "string" ? reason : "Encrypted backup could not be exported."); }
    finally { setExportPassword(""); setConfirmPassword(""); setBusy(false); }
  }

  async function restorePortable() {
    if (restorePassword.length < 12) { setError("Enter the passphrase used when this backup was exported."); return; }
    if (!window.confirm("Restore the entire business database from an encrypted portable backup? Current data will be replaced after Advanced POS verifies the file and creates a safety backup.")) return;
    setBusy(true); setError(""); setMessage("");
    try {
      const restored = await posApi.restorePortableBackup(restorePassword);
      if (restored) window.location.reload();
    } catch (reason) { setError(typeof reason === "string" ? reason : "Encrypted backup could not be restored."); }
    finally { setRestorePassword(""); setBusy(false); }
  }

  return <article className="settings-panel backup-settings-panel">
    <div className="settings-panel-heading"><span className="settings-icon settings-icon-blue"><DatabaseBackup size={19}/></span><div><h2>Local backup and recovery</h2><p>Verified database snapshots are stored on this device and belong to this business.</p></div></div>
    <div className="backup-actions"><button className="button button-primary" disabled={busy || loading} onClick={() => void create()}><DatabaseBackup size={16}/>{busy ? "Working…" : "Create verified backup"}</button><button className="button button-secondary button-small" disabled={busy || loading} onClick={() => void refresh()}><RefreshCw size={15}/>Refresh</button></div>
    <div className="backup-retention">
      <label><span className="field-label">Regular backup retention</span><select className="text-input" value={selectedRetentionLimit} disabled={busy || loading} onChange={(event) => setSelectedRetentionLimit(Number(event.target.value))}><option value={0}>Keep all backups</option><option value={10}>Keep latest 10</option><option value={25}>Keep latest 25</option><option value={50}>Keep latest 50</option></select></label>
      <button className="button button-secondary button-small" disabled={busy || loading || selectedRetentionLimit === retentionLimit} onClick={() => void saveRetention()}>Save retention</button>
    </div>
    <p className="settings-hint">Retention defaults to keep all. If you choose a limit, older regular backups are removed only after a new verified backup succeeds; restore rollback snapshots are retained separately.</p>
    {error && <p className="form-error" role="alert">{error}</p>}
    {message && <p className="settings-success" role="status">{message}</p>}
    {loading ? <p className="settings-hint">Checking local backups…</p> : backups.length === 0 ? <p className="settings-hint">io verified backups are available yet.</p> : <ul className="backup-list">{backups.map((backup) => <li key={backup.id}>
      <div className="backup-details"><strong>{new Date(backup.createdAt).toLocaleString()}</strong><small>{formatBytes(backup.sizeBytes)} · verified for this business</small></div>
      <button className="button button-secondary button-small" disabled={busy} onClick={() => void restore(backup)}><ArchiveRestore size={15}/><RotateCcw size={13}/>Restore</button>
    </li>)}</ul>}
    <div className="backup-portable">
      <div><h3>Encrypted portable copy</h3><p>Save an encrypted file to a drive or another location, then restore it on a device signed into this same business.</p></div>
      <div className="backup-portable-fields">
        <label><span className="field-label">New backup passphrase</span><input className="text-input" type="password" autoComplete="new-password" minLength={12} maxLength={1024} value={exportPassword} onChange={(event) => setExportPassword(event.target.value)} placeholder="At least 12 characters" /></label>
        <label><span className="field-label">Confirm passphrase</span><input className="text-input" type="password" autoComplete="new-password" minLength={12} maxLength={1024} value={confirmPassword} onChange={(event) => setConfirmPassword(event.target.value)} placeholder="Enter it again" /></label>
      </div>
      <button className="button button-secondary" disabled={busy || exportPassword.length < 12 || exportPassword !== confirmPassword} onClick={() => void exportPortable()}><Download size={16}/>Export encrypted copy</button>
      <div className="backup-portable-restore"><label><span className="field-label">Existing backup passphrase</span><input className="text-input" type="password" autoComplete="current-password" minLength={12} maxLength={1024} value={restorePassword} onChange={(event) => setRestorePassword(event.target.value)} placeholder="Enter the backup passphrase" /></label><button className="button button-secondary" disabled={busy || restorePassword.length < 12} onClick={() => void restorePortable()}><Upload size={16}/>Restore encrypted copy</button></div>
    </div>
    <p className="settings-hint">Local snapshots are unencrypted and stay on this device. Portable files use authenticated encryption; Advanced POS never stores the passphrase. If you lose it, that portable file cannot be restored.</p>
  </article>;
}
