// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState } from "react";
import { Check, MonitorCog, Plus, Save } from "lucide-react";
import { posApi } from "../lib/api";
import type { StationWorkspace } from "../lib/models";

export function StationSettingsPanel() {
  const [workspace, setWorkspace] = useState<StationWorkspace | null>(null);
  const [newName, setNewName] = useState("");
  const [renameValues, setRenameValues] = useState<Record<string, string>>({});
  const [selectedStation, setSelectedStation] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");

  useEffect(() => {
    let active = true;
    void posApi.stationWorkspace().then((value) => {
      if (!active) return;
      setWorkspace(value);
      setSelectedStation(value.currentStationKey);
      setRenameValues(Object.fromEntries(value.stations.map((station) => [station.stationKey, station.displayName])));
    }).catch((reason) => {
      if (active) setError(typeof reason === "string" ? reason : "Register stations could not be loaded.");
    });
    return () => { active = false; };
  }, []);

  function accept(value: StationWorkspace, success: string) {
    setWorkspace(value);
    setSelectedStation(value.currentStationKey);
    setRenameValues(Object.fromEntries(value.stations.map((station) => [station.stationKey, station.displayName])));
    setError("");
    setMessage(success);
  }

  async function run(action: () => Promise<StationWorkspace>, success: string) {
    setBusy(true);
    setError("");
    setMessage("");
    try { accept(await action(), success); }
    catch (reason) { setError(typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "Station settings could not be saved."); }
    finally { setBusy(false); }
  }

  return <article className="settings-panel station-settings-panel">
    <div className="settings-panel-heading"><span className="settings-icon settings-icon-blue"><MonitorCog size={19}/></span><div><h2>Register stations</h2><p>One station represents one physical till. Staff use their own profiles and can take turns here; sales and drawer reports keep the station and active staff identity.</p></div></div>
    {workspace ? <>
      <div className="station-current"><span><small>This device is assigned to</small><strong>{workspace.currentDisplayName}</strong></span><span className="station-current-key">{workspace.currentStationKey}</span></div>
      <label className="field-label settings-field-label" htmlFor="station-assignment">Assign this device</label>
      <div className="station-create-row"><select id="station-assignment" className="text-input settings-select" value={selectedStation} onChange={(event) => setSelectedStation(event.target.value)}><option value="" disabled>Choose a station</option>{workspace.stations.filter((station) => station.isActive).map((station) => <option key={station.stationKey} value={station.stationKey}>{station.displayName}{station.isCurrent ? " · current" : ""}</option>)}</select><button type="button" className="button button-secondary" disabled={busy || !selectedStation || selectedStation === workspace.currentStationKey} onClick={() => void run(() => posApi.assignCurrentDeviceStation({ stationKey: selectedStation }), "This device is now assigned to the selected register.")}><Save size={15}/>Assign device</button></div>
      <div className="station-list">{workspace.stations.map((station) => <form className="station-row" key={station.stationKey} onSubmit={(event) => { event.preventDefault(); void run(() => posApi.renameStation({ stationKey: station.stationKey, displayName: renameValues[station.stationKey] ?? station.displayName }), "Register station name saved."); }}><div className="station-row-input"><input className="text-input" aria-label={`Name for ${station.displayName}`} maxLength={60} value={renameValues[station.stationKey] ?? station.displayName} onChange={(event) => setRenameValues((current) => ({ ...current, [station.stationKey]: event.target.value }))}/><small>{station.isCurrent ? "Current device" : station.isActive ? "Active" : "Inactive"}</small></div><button type="submit" className="button button-small button-secondary" disabled={busy || !renameValues[station.stationKey]?.trim() || renameValues[station.stationKey] === station.displayName}><Save size={14}/>Save name</button></form>)}</div>
      <form className="station-create-row station-create-form" onSubmit={(event) => { event.preventDefault(); void run(() => posApi.createStation({ displayName: newName }), "Register station added.").then(() => setNewName("")); }}><input className="text-input settings-select" maxLength={60} required value={newName} onChange={(event) => setNewName(event.target.value)} placeholder="For example, Front counter" aria-label="New register station name"/><button type="submit" className="button button-primary" disabled={busy || !newName.trim()}><Plus size={15}/>Add station</button></form>
    </> : <p className="settings-hint">Loading local register assignment…</p>}
    {message && <p className="settings-success" role="status"><Check size={16}/>{message}</p>}{error && <p className="form-error" role="alert">{error}</p>}
    <p className="settings-hint">Stations currently identify tills on this installation. Sharing tills or stock between computers requires the planned cloud sync service.</p>
  </article>;
}
