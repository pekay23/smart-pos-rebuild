// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useState, type FormEvent } from "react";
import { Archive, Plus, RotateCcw, Save, Tag, X } from "lucide-react";
import { posApi } from "../lib/api";
import type { ProductLabel, ProductLabelInput } from "../lib/models";
import { useToast } from "./useToast";

type LabelKind = "brand" | "tag";

export function ProductLabelManagerDialog({ brands, tags, onClose, onSaved }: {
  brands: ProductLabel[];
  tags: ProductLabel[];
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const toast = useToast();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState("");

  async function save(kind: LabelKind, input: ProductLabelInput) {
    const key = `${kind}:${input.id ?? "new"}`;
    setBusy(key);
    setError("");
    try {
      if (kind === "brand") await posApi.saveProductBrand(input);
      else await posApi.saveProductTag(input);
      await onSaved();
      toast("success", `${kind === "brand" ? "Brand" : "Tag"} saved.`);
    } catch (reason) {
      const message = typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "Catalog label could not be saved.";
      setError(message);
      toast("error", message);
      throw reason;
    } finally {
      setBusy("");
    }
  }

  return <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <section className="edit-modal product-label-dialog" role="dialog" aria-modal="true" aria-labelledby="product-label-title">
      <button type="button" className="modal-close" onClick={onClose} aria-label="Close"><X size={18}/></button>
      <p className="eyebrow">PRODUCT CATALOGUE</p>
      <h2 id="product-label-title">Brands and tags</h2>
      <p className="modal-subtitle">Organize products with labels shared by every store in this business. Archived labels stay on existing products.</p>
      {error && <p className="form-error" role="alert">{error}</p>}
      <div className="product-label-columns">
        <LabelList kind="brand" labels={brands} busy={busy} onSave={save}/>
        <LabelList kind="tag" labels={tags} busy={busy} onSave={save}/>
      </div>
      <div className="modal-actions"><button type="button" className="button button-secondary" onClick={onClose}>Done</button></div>
    </section>
  </div>;
}

function LabelList({ kind, labels, busy, onSave }: {
  kind: LabelKind;
  labels: ProductLabel[];
  busy: string;
  onSave: (kind: LabelKind, input: ProductLabelInput) => Promise<void>;
}) {
  const [newName, setNewName] = useState("");
  const [names, setNames] = useState<Record<string, string>>({});
  const noun = kind === "brand" ? "brand" : "tag";

  async function add(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const name = newName.trim();
    if (!name) return;
    try {
      await onSave(kind, { name, isActive: true });
      setNewName("");
    } catch { /* The dialog displays the command error. */ }
  }

  async function update(label: ProductLabel, isActive = label.isActive, nameValue?: string) {
    const name = (nameValue ?? names[label.id] ?? label.name).trim();
    if (!name) return;
    try {
      await onSave(kind, { id: label.id, name, isActive });
      setNames((current) => { const next = { ...current }; delete next[label.id]; return next; });
    } catch { /* The dialog displays the command error. */ }
  }

  return <section className="product-label-section">
    <h3><Tag size={16}/>{kind === "brand" ? "Brands" : "Tags"}<span>{labels.filter((label) => label.isActive).length} active</span></h3>
    <form className="product-label-add" onSubmit={(event) => void add(event)}>
      <input className="text-input" aria-label={`New ${noun} name`} maxLength={80} value={newName} onChange={(event) => setNewName(event.target.value)} placeholder={`Add a ${noun.toLowerCase()}`}/>
      <button className="button button-secondary button-small" type="submit" disabled={!newName.trim() || busy !== ""}><Plus size={14}/> Add</button>
    </form>
    <div className="product-label-list">
      {labels.length === 0 && <p className="muted-value">No {noun}s yet.</p>}
      {labels.map((label) => <form className={`product-label-row${label.isActive ? "" : " is-archived"}`} key={label.id} onSubmit={(event) => { event.preventDefault(); void update(label); }}>
        <input className="text-input" aria-label={`${noun} name`} maxLength={80} value={names[label.id] ?? label.name} onChange={(event) => setNames((current) => ({ ...current, [label.id]: event.target.value }))}/>
        <button type="submit" className="label-state-button" disabled={busy !== "" || (names[label.id] ?? label.name).trim() === label.name} title={`Save ${noun} name`}><Save size={15}/><span>Save</span></button>
        <button type="button" className="label-state-button" disabled={busy !== ""} onClick={() => void update(label, !label.isActive, label.name)} aria-label={`${label.isActive ? "Archive" : "Restore"} ${label.name}`} title={label.isActive ? "Archive label" : "Restore label"}>
          {label.isActive ? <Archive size={15}/> : <RotateCcw size={15}/>}
          <span>{label.isActive ? "Archive" : "Restore"}</span>
        </button>
      </form>)}
    </div>
  </section>;
}
