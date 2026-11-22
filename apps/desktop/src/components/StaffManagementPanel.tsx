// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState, type FormEvent } from "react";
import { ImagePlus, KeyRound, RefreshCw, ShieldCheck, UserRoundPlus, UserRoundX, RotateCcw } from "lucide-react";
import { posApi } from "../lib/api";
import type { CreateStaffMemberInput, SessionUser, StaffAccount, StaffRole, StaffRolesView } from "../lib/models";
import { EntityImageField } from "./EntityImageField";
import { EntityImageThumbnail } from "./EntityImageThumbnail";
import { useEntityImage } from "./useEntityImage";
import { useToast } from "./useToast";
import { useAttemptCountdown } from "../lib/useAttemptCountdown";

const emptyForm: CreateStaffMemberInput = {
  fullName: "",
  username: "",
  password: "",
  pin: "",
  roleKey: "cashier",
};

export function StaffManagementPanel({ currentUser, onSessionChange }: { currentUser: SessionUser; onSessionChange: (user: SessionUser) => void }) {
  const toast = useToast();
  const attemptCountdown = useAttemptCountdown();
  const currentUserId = currentUser.userId;
  const [profiles, setProfiles] = useState<StaffAccount[] | null>(null);
  const [roleView, setRoleView] = useState<StaffRolesView | null>(null);
  const [editingRole, setEditingRole] = useState<StaffRole | null>(null);
  const [roleEditorOpen, setRoleEditorOpen] = useState(false);
  const [roleName, setRoleName] = useState("");
  const [roleTemplate, setRoleTemplate] = useState<"cashier" | "manager" | "admin">("cashier");
  const [roleMode, setRoleMode] = useState<"cashier" | "management">("cashier");
  const [rolePermissions, setRolePermissions] = useState<string[]>([]);
  const [reauthPassword, setReauthPassword] = useState("");
  const [claimPassword, setClaimPassword] = useState("");
  const [form, setForm] = useState<CreateStaffMemberInput>(emptyForm);
  const [refreshing, setRefreshing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [updatingUserId, setUpdatingUserId] = useState<string | null>(null);
  const [photoProfile, setPhotoProfile] = useState<StaffAccount | null>(null);
  const staffImage = useEntityImage("staff");
  const canManagePhotos = currentUser.isOwner || currentUser.roleKey === "admin";

  const loading = profiles === null || refreshing;

  useEffect(() => {
    let active = true;
    void Promise.all([posApi.staffAccounts(), posApi.staffRoles()]).then(([rows, roles]) => {
      if (!active) return;
      setProfiles(rows);
      setRoleView(roles);
      setError("");
    }).catch((reason) => {
      if (!active) return;
      setProfiles([]);
      setError(typeof reason === "string" ? reason : "Staff profiles could not be loaded.");
    });
    return () => { active = false; };
  }, []);

  async function refreshProfiles() {
    setRefreshing(true);
    try {
      const [rows, roles] = await Promise.all([posApi.staffAccounts(), posApi.staffRoles()]);
      setProfiles(rows);
      setRoleView(roles);
      setError("");
    } catch (reason) {
      setError(typeof reason === "string" ? reason : "Staff profiles could not be loaded.");
    } finally {
      setRefreshing(false);
    }
  }

  function startNewRole() {
    const template = roleView?.roles.find((role) => role.roleKey === roleTemplate);
    setEditingRole(null);
    setRoleEditorOpen(true);
    setRoleName("");
    setRoleMode(template?.workspaceMode ?? "cashier");
    setRolePermissions(template?.permissions.filter((permission) => roleView?.permissions.some((item) => item.key === permission)) ?? []);
    setReauthPassword("");
  }

  function startEditRole(role: StaffRole) {
    setEditingRole(role);
    setRoleEditorOpen(true);
    setRoleName(role.displayName);
    setRoleMode(role.workspaceMode);
    setRolePermissions(role.permissions.filter((permission) => roleView?.permissions.some((item) => item.key === permission)));
    setReauthPassword("");
  }

  async function saveRole(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setError(""); setMessage(""); setSaving(true);
    try {
      await posApi.saveStaffRole({roleKey: editingRole?.roleKey, templateKey: roleTemplate, displayName: roleName, permissions: rolePermissions, workspaceMode: roleMode, expectedRevision: editingRole?.revision, password: reauthPassword});
      setRoleView(await posApi.staffRoles()); setEditingRole(null); setRoleEditorOpen(false); setRoleName(""); setReauthPassword(""); setMessage("Role saved. Staff assigned to it will use its updated access on their next sign in.");
    } catch (reason) {
      const detail = typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "";
      const message = detail === "Your password was not recognized."
        ? `The sign-in password for ${currentUser.username} was not recognized. Profile-switch PINs cannot confirm role changes, and switching profiles does not change passwords.`
        : detail || "The role could not be saved.";
      attemptCountdown.observe(detail);
      setError(message);
      toast("error", message);
    }
    finally { setSaving(false); }
  }

  async function claimOwner() {
    setError(""); setMessage(""); setSaving(true);
    try { onSessionChange(await posApi.claimBusinessOwner(claimPassword)); setClaimPassword(""); setRoleView(await posApi.staffRoles()); setMessage("This account is now the designated business owner."); }
    catch (reason) { const message = typeof reason === "string" ? reason : "Ownership could not be assigned."; attemptCountdown.observe(message); setError(message); }
    finally { setSaving(false); }
  }

  async function createMember(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSaving(true);
    setError("");
    setMessage("");
    try {
      const created = await posApi.createStaffMember(form);
      setProfiles(await posApi.staffAccounts());
      setForm(emptyForm);
      const status = `${created.fullName} can now switch into their profile at the register.`; setMessage(status); toast("success", status);
    } catch (reason) {
      const message = typeof reason === "string" ? reason : "The staff profile could not be created."; setError(message); toast("error", message);
    } finally {
      setSaving(false);
    }
  }

  async function changeAccountStatus(profile: StaffAccount) {
    const nextActive = !profile.isActive;
    const prompt = nextActive
      ? `Restore ${profile.fullName} (${profile.username})? They will be able to sign in and switch into their profile again.`
      : `Deactivate ${profile.fullName} (${profile.username})? They will no longer be able to sign in or switch into this profile.`;
    if (!window.confirm(prompt)) return;
    setError("");
    setMessage("");
    setUpdatingUserId(profile.userId);
    try {
      await posApi.setStaffAccountActive({ userId: profile.userId, isActive: nextActive });
      setProfiles(await posApi.staffAccounts());
      const status = `${profile.fullName}’s account is ${nextActive ? "active" : "deactivated"}.`; setMessage(status); toast("success", status);
    } catch (reason) {
      const message = typeof reason === "string" ? reason : "The staff account could not be updated."; setError(message); toast("error", message);
    } finally {
      setUpdatingUserId(null);
    }
  }

  async function changeStaffRole(profile: StaffAccount, roleKey: string) {
    setError(""); setMessage(""); setUpdatingUserId(profile.userId);
    try { await posApi.assignStaffRole({userId:profile.userId,roleKey}); await refreshProfiles(); setMessage(`${profile.fullName}’s role was updated.`); }
    catch (reason) { setError(typeof reason === "string" ? reason : "The staff role could not be changed."); }
    finally { setUpdatingUserId(null); }
  }

  return <article className="settings-panel staff-management-panel">
    <div className="settings-panel-heading">
      <span className="settings-icon"><ShieldCheck size={19}/></span>
      <div><h2>Staff profiles</h2><p>Add separate cashier, manager, and administrator accounts for this business.</p></div>
      <button className="icon-button staff-refresh" type="button" onClick={() => void refreshProfiles()} disabled={loading} aria-label="Refresh staff profiles" title="Refresh staff profiles"><RefreshCw size={16}/></button>
    </div>

    <div className="staff-profile-list" aria-live="polite">
      {(profiles ?? []).map((profile) => <div className="staff-profile-row" key={profile.userId}>
        <EntityImageThumbnail entityType="staff" entityId={profile.userId} fallback={profile.fullName.split(/\s+/).map((part) => part[0]).slice(0, 2).join("").toUpperCase()} className="user-avatar staff-photo-avatar"/>
        <span><strong>{profile.fullName}</strong><small>{profile.username} · {profile.roleName}</small></span>
        <span className={profile.hasPin ? "staff-pin-state configured" : "staff-pin-state"}><KeyRound size={13}/>{profile.hasPin ? "PIN ready" : "Password only"}</span>
        {canManagePhotos && <button className="table-action staff-photo-action" type="button" onClick={() => { setPhotoProfile(profile); staffImage.reset(profile.userId); }} aria-label={`Edit ${profile.fullName}'s profile photo`} title={`Edit ${profile.fullName}'s profile photo`}><ImagePlus size={14}/><span>Photo</span></button>}
        <span className={`status-pill ${profile.isActive ? "status-active" : "status-inactive"}`}><i/>{profile.isActive ? "Active" : "Deactivated"}</span>
        {!profile.isOwner && profile.userId !== currentUserId && <select className="staff-role-select" aria-label={`Role for ${profile.fullName}`} value={profile.roleKey} disabled={updatingUserId !== null} onChange={(event)=>void changeStaffRole(profile,event.target.value)}>{(roleView?.roles ?? []).map((role)=><option key={role.roleKey} value={role.roleKey}>{role.displayName}</option>)}</select>}
        {profile.isOwner ? <span className="staff-current-account">Business owner</span> : profile.userId === currentUserId ? <span className="staff-current-account">Current account</span> : <button className="table-action staff-account-toggle" type="button" disabled={updatingUserId !== null} onClick={() => void changeAccountStatus(profile)} aria-label={`${profile.isActive ? "Deactivate" : "Restore"} ${profile.fullName}`} title={profile.isActive ? "Deactivate account" : "Restore account"}>{updatingUserId === profile.userId ? "Saving…" : profile.isActive ? <><UserRoundX size={14}/>Deactivate</> : <><RotateCcw size={14}/>Restore</>}</button>}
      </div>)}
      {!loading && profiles?.length === 0 && <p className="settings-hint">No staff profiles found.</p>}
      {loading && <p className="settings-hint">Loading staff profiles…</p>}
    </div>

    {photoProfile && <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) setPhotoProfile(null); }}><form className="edit-modal staff-photo-modal" onSubmit={(event) => { event.preventDefault(); setSaving(true); setError(""); void staffImage.commit(photoProfile.userId).then(() => { setMessage(`${photoProfile.fullName}’s profile photo was saved.`); setPhotoProfile(null); }).catch((reason: unknown) => setError(typeof reason === "string" ? reason : "The profile photo could not be saved.")).finally(() => setSaving(false)); }}><button type="button" className="modal-close" onClick={() => setPhotoProfile(null)} aria-label="Close">×</button><p className="eyebrow">STAFF PROFILE</p><h2>Profile photo</h2><p className="modal-subtitle">Only the business owner and administrators can manage staff photos.</p><EntityImageField image={staffImage} title={`${photoProfile.fullName}'s photo`} shape="portrait"/>{error && <p className="form-error" role="alert">{error}</p>}<div className="modal-actions"><button className="button button-secondary" type="button" onClick={() => setPhotoProfile(null)}>Cancel</button><button className="button button-primary" type="submit" disabled={saving}>{saving ? "Saving…" : "Save photo"}</button></div></form></div>}

    <form className="staff-create-form" onSubmit={(event) => void createMember(event)}>
      <div className="staff-form-heading"><UserRoundPlus size={17}/><strong>Add a staff profile</strong></div>
      <div className="staff-form-grid">
        <label className="field-label">Full name<input className="text-input" required maxLength={100} autoComplete="name" value={form.fullName} onChange={(event) => setForm((current) => ({ ...current, fullName: event.target.value }))}/></label>
        <label className="field-label">Username<input className="text-input" required minLength={3} maxLength={50} autoComplete="username" value={form.username} onChange={(event) => setForm((current) => ({ ...current, username: event.target.value }))}/></label>
        <label className="field-label">Role<select className="text-input" value={form.roleKey} onChange={(event) => setForm((current) => ({ ...current, roleKey: event.target.value }))}>{(roleView?.roles ?? []).map((role) => <option key={role.roleKey} value={role.roleKey}>{role.displayName}</option>)}</select></label>
        <label className="field-label">Sign-in password<input className="text-input" required type="password" minLength={12} autoComplete="new-password" value={form.password} onChange={(event) => setForm((current) => ({ ...current, password: event.target.value }))}/><small>At least 12 characters. Used for full sign-in and account recovery.</small></label>
        <label className="field-label">Shift PIN<input className="text-input" required type="password" inputMode="numeric" pattern="[0-9]{4,10}" minLength={4} maxLength={10} autoComplete="off" value={form.pin} onChange={(event) => setForm((current) => ({ ...current, pin: event.target.value.replace(/\D/g, "") }))}/><small>4 to 10 digits. The staff member uses this to switch profiles.</small></label>
      </div>
      <div className="settings-actions"><button className="button button-primary" type="submit" disabled={saving}>{saving ? "Adding profile…" : "Add staff profile"}<UserRoundPlus size={16}/></button></div>
      {message && <p className="settings-success" role="status">{message}</p>}
      {error && <p className="form-error" role="alert">{attemptCountdown.format(error)}</p>}
    </form>

    {roleView?.ownerUserId === null && currentUser.roleKey === "admin" && <section className="settings-subsection"><h3>Designate the business owner</h3><p>No owner was found for this existing workspace. Confirm this active administrator account with its password to establish ownership.</p><div className="staff-form-grid"><input className="text-input" type="password" autoComplete="current-password" placeholder="Confirm administrator password" value={claimPassword} onChange={(event) => setClaimPassword(event.target.value)}/><button className="button button-primary" type="button" disabled={saving || !claimPassword} onClick={() => void claimOwner()}>Make this account owner</button></div></section>}

    {currentUser.isOwner && <section className="settings-subsection staff-roles-section"><div className="staff-form-heading"><ShieldCheck size={17}/><strong>Custom roles</strong></div><p>Create focused roles from cashier, manager, or administrator access. Only the owner can change role permissions.</p>
      <div className="staff-role-list">{(roleView?.roles ?? []).filter((role) => !role.isSystem).map((role) => <div className="staff-role-row" key={role.roleKey}><span><strong>{role.displayName}</strong><small>{role.workspaceMode === "cashier" ? "Cashier workspace" : "Management workspace"} · {role.activeUserCount} active staff</small></span><button className="table-action" type="button" onClick={() => startEditRole(role)}>Edit role</button></div>)}</div>
      {!editingRole && <div className="staff-form-grid"><label className="field-label">Start from<select className="text-input" value={roleTemplate} onChange={(event) => {setRoleTemplate(event.target.value as typeof roleTemplate); const role=roleView?.roles.find((item)=>item.roleKey===event.target.value); setRoleMode(role?.workspaceMode ?? "cashier"); setRolePermissions(role?.permissions.filter((permission)=>roleView?.permissions.some((item)=>item.key===permission)) ?? []);}}><option value="cashier">Cashier</option><option value="manager">Manager</option><option value="admin">Administrator</option></select></label><button className="button button-secondary" type="button" onClick={startNewRole}>Create custom role</button></div>}
      {roleEditorOpen && <form className="staff-role-editor" onSubmit={(event) => void saveRole(event)}><label className="field-label">Role name<input className="text-input" required maxLength={40} value={roleName} onChange={(event)=>setRoleName(event.target.value)}/></label><label className="field-label">Workspace<select className="text-input" value={roleMode} onChange={(event)=>setRoleMode(event.target.value as typeof roleMode)}><option value="cashier">Cashier</option><option value="management">Management</option></select></label><div className="role-permission-grid">{(roleView?.permissions ?? []).map((permission)=><label key={permission.key}><input type="checkbox" checked={rolePermissions.includes(permission.key)} onChange={(event)=>setRolePermissions((current)=>event.target.checked?[...current,permission.key]:current.filter((key)=>key!==permission.key))}/><span><strong>{permission.label}</strong><small>{permission.description}</small></span></label>)}</div><label className="field-label">Confirm {currentUser.fullName}’s sign-in password<input className="text-input" required type="password" autoComplete="current-password" value={reauthPassword} onChange={(event)=>setReauthPassword(event.target.value)}/><small>Use the password for {currentUser.username} that you use to sign in. A staff-switch PIN is not accepted for role changes.</small></label><div className="settings-actions"><button className="button button-secondary" type="button" onClick={()=>{setEditingRole(null);setRoleEditorOpen(false);setRoleName("");}}>Cancel</button><button className="button button-primary" type="submit" disabled={saving}>{saving?"Saving…":"Save role"}</button></div></form>}
    </section>}
  </article>;
}
