// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useCallback, useEffect, useState } from "react";
import { Download, RefreshCw, RotateCw } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

declare const __APP_VERSION__: string;
type UpdateStatus = { configured: boolean; installed: boolean; available: boolean; downloaded: boolean; version: string | null; notes: string | null; deltaAvailable: boolean; downloadSizeBytes: number; message: string | null };
type CheckState = "idle" | "checking" | "current" | "available" | "unconfigured" | "downloading" | "error";

function formatBytes(bytes: number): string {
  if (bytes <= 0) return "calculated by the updater";
  const units = ["B", "KB", "MB", "GB"];
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return `${(bytes / 1024 ** exponent).toFixed(exponent === 0 ? 0 : 1)} ${units[exponent]}`;
}

export function UpdateCenter() {
  const [state, setState] = useState<CheckState>("idle");
  const [release, setRelease] = useState<UpdateStatus | null>(null);
  const [error, setError] = useState("");
  const [downloadProgress, setDownloadProgress] = useState(0);

  const checkForUpdates = useCallback(async () => {
    setError("");
    setState("checking");
    try {
      const result = await invoke<UpdateStatus>("check_for_update");
      setRelease(result);
      if (!result.configured || !result.installed) setState("unconfigured");
      else setState(result.available ? "available" : "current");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setState("error");
    }
  }, []);

  useEffect(() => { void checkForUpdates(); }, [checkForUpdates]);

  const download = async () => {
    setState("downloading");
    setError("");
    setDownloadProgress(0);
    let unlisten: (() => void) | undefined;
    try {
      unlisten = await listen<number>("desktop-update-progress", (event) => setDownloadProgress(Math.max(0, Math.min(100, event.payload))));
      await invoke("download_update");
      setDownloadProgress(100);
      setRelease((current) => current ? { ...current, downloaded: true } : current);
      setState("available");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setState("error");
    } finally {
      unlisten?.();
    }
  };

  const apply = async () => {
    try { await invoke("apply_downloaded_update"); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); setState("error"); }
  };

  const text = state === "checking" ? "Checking the release channel…"
    : state === "current" ? "You’re using the latest published version."
      : state === "available" && release && !release.deltaAvailable ? "A delta update is not available for this installation. Nothing was downloaded; contact support for a one-time migration or recovery package."
      : state === "available" && release?.downloaded ? `Version ${release.version ?? "update"} is downloaded and ready to install.`
        : state === "available" && release?.version ? `Version ${release.version} is available.`
        : state === "downloading" ? "Downloading the update in the background…"
          : state === "unconfigured" ? release?.message ?? "Incremental updates are not configured for this installation yet."
            : state === "error" ? error || "Could not check the release channel. Try again when you’re online."
              : "Check the release channel for a newer version.";

  return <article className="settings-panel update-center-panel">
    <div className="settings-panel-heading"><span className="settings-icon settings-icon-blue"><RefreshCw size={19}/></span><div><h2>App updates</h2><p>Current version {__APP_VERSION__}. Only the update patches are downloaded, then applied to this installation when you choose to restart. The full setup installer is not downloaded or rerun.</p></div></div>
    <p className={state === "error" ? "form-error" : "settings-hint"} role={state === "error" ? "alert" : "status"}>{text}</p>
    {release?.available && <div className="update-release-details">{release.notes && <p>{release.notes}</p>}<small>{release.deltaAvailable ? `Delta patch download: ${formatBytes(release.downloadSizeBytes)}. If the required patch is missing, invalid, or cannot be applied, the update stops and reports an error; it never downloads the full application package in the background.` : "No delta patch is available for this installation. No package will download; contact support for a separately provided migration or recovery package."}</small></div>}
    {state === "downloading" && <div className="desktop-update-progress">
      <div className="desktop-update-progress-label"><span>Downloading update</span><span>{downloadProgress}%</span></div>
      <div className="desktop-update-progress-track" role="progressbar" aria-label="Desktop update download" aria-valuemin={0} aria-valuemax={100} aria-valuenow={downloadProgress}><span style={{ width: `${downloadProgress}%` }}/></div>
    </div>}
    {release?.available && <div className="settings-actions">
      {!release.downloaded ? <button className="button button-primary" type="button" onClick={() => void download()} disabled={state === "downloading" || !release.deltaAvailable}><Download size={16}/>{state === "downloading" ? "Downloading update patches…" : "Download update patches"}</button>
        : <button className="button button-primary" type="button" onClick={() => void apply()}><RotateCw size={16}/>Restart and apply update</button>}
    </div>}
    <div className="settings-actions"><button className="button button-secondary" type="button" onClick={() => void checkForUpdates()} disabled={state === "checking" || state === "downloading"}><RefreshCw size={15}/>{state === "checking" ? "Checking…" : "Check for updates"}</button></div>
    <p className="settings-hint">Save the current sale and close the register before restarting. Only the update is staged; your existing installation and business data are kept.</p>
  </article>;
}
