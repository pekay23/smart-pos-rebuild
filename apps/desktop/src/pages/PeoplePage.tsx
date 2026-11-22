// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { Building2, Mail, MapPin, Pencil, Phone, Plus, Search, Users, X } from "lucide-react";
import { posApi } from "../lib/api";
import type { Customer, CustomerInput, SessionUser, Supplier, SupplierInput } from "../lib/models";
import { EntityImageField } from "../components/EntityImageField";
import { useEntityImage } from "../components/useEntityImage";
import { EntityImageThumbnail } from "../components/EntityImageThumbnail";

type Directory = "customers" | "suppliers";
type FormRecord = CustomerInput | SupplierInput;
const emptyCustomer: CustomerInput = { name: "", email: null, phone: null, address: null, notes: null };
const emptySupplier: SupplierInput = { name: "", contactPerson: null, email: null, phone: null, address: null, isActive: true };

export function PeoplePage({ user, initialDirectory = "customers" }: { user: SessionUser; initialDirectory?: Directory }) {
  const [directory, setDirectory] = useState<Directory>(initialDirectory);
  const [customers, setCustomers] = useState<Customer[]>([]);
  const [suppliers, setSuppliers] = useState<Supplier[]>([]);
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [editing, setEditing] = useState<FormRecord | null>(null);
  const [form, setForm] = useState<FormRecord>(emptyCustomer);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const customerImage = useEntityImage("customer");
  const supplierImage = useEntityImage("supplier");
  const canReadSuppliers = user.permissions.includes("suppliers:read");

  const load = useCallback(async () => { const supplierRequest = canReadSuppliers ? posApi.suppliers() : Promise.resolve([]); const [customerRows, supplierRows] = await Promise.all([posApi.customers(), supplierRequest]); setCustomers(customerRows); setSuppliers(supplierRows); }, [canReadSuppliers]);
  useEffect(() => { void load().catch(() => setError("Could not load this directory.")); }, [load]);
  const rows = useMemo(() => {
    const source = directory === "customers" ? customers : suppliers;
    return source.filter((entry) => `${entry.name} ${entry.email ?? ""} ${entry.phone ?? ""} ${"contactPerson" in entry ? entry.contactPerson ?? "" : entry.notes ?? ""}`.toLowerCase().includes(query.toLowerCase()));
  }, [customers, suppliers, directory, query]);

  function startAdd() { (directory === "customers" ? customerImage : supplierImage).reset(); setEditing(null); setForm(directory === "customers" ? { ...emptyCustomer } : { ...emptySupplier }); setError(""); setOpen(true); }
  function startEdit(entry: Customer | Supplier) { ("contactPerson" in entry ? supplierImage : customerImage).reset(entry.id); setEditing(entry); setForm({ ...entry }); setError(""); setOpen(true); }
  function change(key: keyof CustomerInput | keyof SupplierInput, value: string | boolean | null) { setForm((current) => ({ ...current, [key]: value }) as FormRecord); }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError("");
    const optional = (value: string | null | undefined) => value?.trim() || null;
    try {
      if (directory === "customers") {
        const input: CustomerInput = { id: form.id, name: form.name.trim(), email: optional(form.email), phone: optional(form.phone), address: optional(form.address), notes: optional("notes" in form ? form.notes : null) };
        const saved = await posApi.saveCustomer(input); setForm({ ...input, id: saved.id }); setEditing(saved); await customerImage.commit(saved.id); customerImage.reset(saved.id);
      } else {
        const input: SupplierInput = { id: form.id, name: form.name.trim(), contactPerson: optional("contactPerson" in form ? form.contactPerson : null), email: optional(form.email), phone: optional(form.phone), address: optional(form.address), isActive: "isActive" in form ? form.isActive : true };
        const saved = await posApi.saveSupplier(input); setForm({ ...input, id: saved.id }); setEditing(saved); await supplierImage.commit(saved.id); supplierImage.reset(saved.id);
      }
      await load(); setOpen(false);
    } catch (reason) { setError(typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "This contact could not be saved. Please check the details."); }
    finally { setBusy(false); }
  }

  const isCustomer = directory === "customers";
  const activeImage = isCustomer ? customerImage : supplierImage;
  return <section className="people-page"><div className="page-heading"><div><p className="eyebrow">STORE RELATIONSHIPS</p><h1>People & partners</h1><p className="page-subtitle">Keep customer and supplier contact details in one place.</p></div><button className="button button-primary" onClick={startAdd}><Plus size={17}/>{isCustomer ? "Add customer" : "Add supplier"}</button></div>
    <div className="directory-tabs" role="tablist" aria-label="Choose contacts"><button role="tab" aria-selected={isCustomer} className={isCustomer ? "directory-tab selected" : "directory-tab"} onClick={() => { setDirectory("customers"); setQuery(""); }}><Users size={16}/> Customers <span>{customers.length}</span></button>{canReadSuppliers && <button role="tab" aria-selected={!isCustomer} className={!isCustomer ? "directory-tab selected" : "directory-tab"} onClick={() => { setDirectory("suppliers"); setQuery(""); }}><Building2 size={16}/> Suppliers <span>{suppliers.length}</span></button>}</div>
    <article className="panel people-panel"><div className="table-toolbar"><div><h2>{isCustomer ? "Customer directory" : "Supplier directory"}</h2><p>{rows.length} {isCustomer ? "customers" : "suppliers"} listed · {user.storeName}</p></div><label className="table-search"><Search size={16}/><input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={`Search ${isCustomer ? "customers" : "suppliers"}…`} aria-label="Search directory"/></label></div>
      <div className="people-list">{rows.map((entry) => { const partner = "contactPerson" in entry; const isActive = !partner || entry.isActive; return <article className="person-row" key={entry.id}><EntityImageThumbnail entityType={partner ? "supplier" : "customer"} entityId={entry.id} fallback={entry.name.trim().charAt(0).toUpperCase()} className="person-avatar"/><div className="person-main"><strong>{entry.name}</strong><small>{partner ? entry.contactPerson || "Supplier" : "Customer"}</small></div><div className="person-detail"><Mail size={14}/><span>{entry.email || "No email added"}</span></div><div className="person-detail"><Phone size={14}/><span>{entry.phone || "No phone added"}</span></div><div className="person-detail person-address"><MapPin size={14}/><span>{entry.address || "No address added"}</span></div><span className={`status-pill ${isActive ? "status-active" : "status-inactive"}`}><i/>{isActive ? "Active" : "Archived"}</span><button className="table-action person-edit" onClick={() => startEdit(entry)}><Pencil size={13}/> Edit</button></article>; })}
        {!rows.length && <div className="table-empty"><span className="empty-icon">{isCustomer ? <Users size={23}/> : <Building2 size={23}/>}</span><strong>{query ? "No matching contacts" : `No ${isCustomer ? "customers" : "suppliers"} yet`}</strong><span>{query ? "Try a different name, email, or phone number." : `Add ${isCustomer ? "customers to keep useful contact details close at checkout" : "suppliers to organize who you buy stock from"}.`}</span>{!query && <button className="button button-secondary button-small" onClick={startAdd}>Add {isCustomer ? "customer" : "supplier"} <Plus size={15}/></button>}</div>}
      </div>
    </article><footer className="page-footer"><span>DIRECTORY <b>·</b> Contact details are stored on this device</span><span>Sales can be linked to a customer during checkout</span></footer>
    {open && <div className="modal-backdrop" onMouseDown={(e) => { if (e.target === e.currentTarget) setOpen(false); }}><form className="edit-modal person-modal" onSubmit={(e) => void save(e)}><button type="button" className="modal-close" onClick={() => setOpen(false)} aria-label="Close"><X size={18}/></button><p className="eyebrow">{isCustomer ? "CUSTOMER DIRECTORY" : "SUPPLIER DIRECTORY"}</p><h2>{editing ? `Edit ${isCustomer ? "customer" : "supplier"}` : `Add ${isCustomer ? "customer" : "supplier"}`}</h2><p className="modal-subtitle">Keep the details your team needs for everyday store operations.</p><EntityImageField image={activeImage} title={isCustomer ? "Customer photo" : "Supplier logo or photo"} shape="portrait"/><div className="form-grid two-col product-form-grid"><div className="form-grid-span"><label className="field-label" htmlFor="contactName">{isCustomer ? "Customer" : "Supplier"} name</label><input className="text-input" id="contactName" autoFocus required value={form.name} onChange={(e) => change("name", e.target.value)} placeholder={isCustomer ? "Customer or company name" : "Supplier or company name"}/></div>{!isCustomer && <div className="form-grid-span"><label className="field-label" htmlFor="contactPerson">Contact person</label><input className="text-input" id="contactPerson" value={"contactPerson" in form ? form.contactPerson ?? "" : ""} onChange={(e) => change("contactPerson", e.target.value || null)} placeholder="Optional"/></div>}<div><label className="field-label" htmlFor="contactPhone">Phone</label><input className="text-input" id="contactPhone" type="tel" value={form.phone ?? ""} onChange={(e) => change("phone", e.target.value || null)} placeholder="Optional"/></div><div><label className="field-label" htmlFor="contactEmail">Email</label><input className="text-input" id="contactEmail" type="email" value={form.email ?? ""} onChange={(e) => change("email", e.target.value || null)} placeholder="Optional"/></div><div className="form-grid-span"><label className="field-label" htmlFor="contactAddress">Address</label><input className="text-input" id="contactAddress" value={form.address ?? ""} onChange={(e) => change("address", e.target.value || null)} placeholder="Optional"/></div>{isCustomer && <div className="form-grid-span"><label className="field-label" htmlFor="contactNotes">Notes</label><input className="text-input" id="contactNotes" value={"notes" in form ? form.notes ?? "" : ""} onChange={(e) => change("notes", e.target.value || null)} placeholder="Optional internal note"/></div>}{!isCustomer && editing && <label className="checkbox-field form-grid-span"><input type="checkbox" checked={"isActive" in form ? form.isActive : true} onChange={(e) => change("isActive", e.target.checked)}/><span>Supplier is active</span></label>}</div>{error && <p className="form-error" role="alert">{error}</p>}<div className="modal-actions"><button type="button" className="button button-secondary" onClick={() => setOpen(false)}>Cancel</button><button className="button button-primary" disabled={busy}>{busy ? "Saving…" : editing ? "Save changes" : `Add ${isCustomer ? "customer" : "supplier"}`}</button></div></form></div>}
  </section>;
}
