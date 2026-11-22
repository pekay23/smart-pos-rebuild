// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useMemo, useRef, useState } from 'react'
import type { User } from 'oidc-client-ts'
import { ArrowRight, Boxes, Building2, Check, ChevronDown, KeyRound, LayoutDashboard, LockKeyhole, LogOut, Monitor, ShieldCheck, Store, Users } from 'lucide-react'
import { ApiError, getBranchPermissions, getStationPermissions, listTenantAccess } from './api'
import type { BranchAccess, BranchPermissionSnapshot, StationAccess, StationPermissionSnapshot, TenantMembership } from './api'
import { createAuthSetup } from './auth'
import type { AuthSetup } from './auth'
import { BranchCatalogPanel } from './BranchCatalogPanel'
import { BranchProfilePanel } from './BranchProfilePanel'
import { BranchPaymentOptionsPanel } from './BranchPaymentOptionsPanel'
import { SupportRequestsPanel } from './SupportRequestsPanel'
import { BranchSalesReportPanel } from './BranchSalesReportPanel'
import { BranchOverviewPanel } from './BranchOverviewPanel'
import { BranchStockTransfersPanel } from './BranchStockTransfersPanel'
import { HostedCustomersPanel } from './HostedCustomersPanel'
import { HostedSuppliersPanel } from './HostedSuppliersPanel'
import { HostedExpensesPanel } from './HostedExpensesPanel'
import { HostedCashLocationsPanel } from './HostedCashLocationsPanel'
import { HostedPurchasesPanel } from './HostedPurchasesPanel'
import { HostedRegisterPanel } from './HostedRegisterPanel'
import { HostedPickupQueuePanel } from './HostedPickupQueuePanel'
import { HostedStaffPanel } from './HostedStaffPanel'
import { HostedAttendancePanel } from './HostedAttendancePanel'
import { TenantStationCapacityPanel } from './TenantStationCapacityPanel'
import { TenantOverviewPanel } from './TenantOverviewPanel'
import { DeploymentUpdateNotice } from './DeploymentUpdateNotice'

type Selection =
  | { kind: 'none' }
  | { kind: 'branch-loading'; tenantId: string; branchId: string }
  | { kind: 'branch-ready'; tenantId: string; branchId: string; permissions: BranchPermissionSnapshot }
  | { kind: 'branch-error'; tenantId: string; branchId: string; message: string }
  | { kind: 'station-loading'; tenantId: string; branchId: string; stationId: string }
  | { kind: 'station-ready'; tenantId: string; branchId: string; stationId: string; permissions: StationPermissionSnapshot }
  | { kind: 'station-error'; tenantId: string; branchId: string; stationId: string; message: string }

type ScreenState =
  | { kind: 'booting' }
  | { kind: 'unconfigured'; message: string; names: string[] }
  | { kind: 'signed-out'; message: string | null }
  | { kind: 'loading-access'; user: User }
  | { kind: 'workspace'; user: User; tenants: TenantMembership[]; selection: Selection }
  | { kind: 'workspace-error'; user: User; message: string }

let signInCallback: Promise<User> | null = null

function displayName(user: User): string {
  const profileName = user.profile.name
  if (typeof profileName === 'string' && profileName.trim().length > 0) return profileName.trim()
  const email = user.profile.email
  return typeof email === 'string' && email.trim().length > 0 ? email.trim() : 'Your account'
}

function assertNever(value: never): never {
  throw new Error(`Unhandled state: ${JSON.stringify(value)}`)
}

function selectedLocation(selection: Selection, tenants: TenantMembership[]): {
  tenant: TenantMembership
  branch: BranchAccess
  station: StationAccess
} | null {
  if (selection.kind !== 'station-loading' && selection.kind !== 'station-ready' && selection.kind !== 'station-error') return null
  const tenant = tenants.find((item) => item.tenantId === selection.tenantId)
  const branch = tenant?.branches.find((item) => item.branchId === selection.branchId)
  const station = branch?.stations.find((item) => item.stationId === selection.stationId)
  if (!tenant || !branch || !station) return null
  return { tenant, branch, station }
}

function selectedBranch(selection: Selection, tenants: TenantMembership[]): { tenant: TenantMembership; branch: BranchAccess } | null {
  if (selection.kind !== 'branch-loading' && selection.kind !== 'branch-ready' && selection.kind !== 'branch-error') return null
  const tenant = tenants.find((item) => item.tenantId === selection.tenantId)
  const branch = tenant?.branches.find((item) => item.branchId === selection.branchId)
  if (!tenant || !branch) return null
  return { tenant, branch }
}

function roleLabels(tenant: TenantMembership): string[] {
  return [...new Set(tenant.roleGrants.map((grant) => grant.roleKey))]
}

function HomeScreen({ setup, message, onSignIn }: { setup: AuthSetup; message: string | null; onSignIn: () => Promise<void> }) {
  const configured = setup.kind === 'ready'
  return <main className="sign-in-layout">
    <section className="sign-in-story" aria-label="Advanced POS overview">
      <div className="brand-lockup"><span className="brand-mark">A</span><span><strong>Advanced POS</strong><small>POWERED SOLUTIONS</small></span></div>
      <div className="counter-illustration" aria-hidden="true">
        <div className="counter-window"><span className="window-sun"/><span className="window-shelf shelf-one"/><span className="window-shelf shelf-two"/><span className="window-stocks"><i/><i/><i/><i/></span></div>
        <div className="counter-desk"><div className="terminal"><span/><i/><i/><i/></div><div className="receipt-roll"/></div>
        <div className="counter-leg leg-left"/><div className="counter-leg leg-right"/>
      </div>
      <div className="story-copy"><span className="eyebrow">ONE WORKSPACE. EVERY BRANCH.</span><h1>Keep the day<br/>moving.</h1><p>Sign in to reach your assigned business, branch and station.</p></div>
      <div className="story-foot"><span><ShieldCheck size={14}/> Access follows your role</span><span>Advanced POS · WEB</span></div>
    </section>
    <section className="sign-in-panel">
      <div className="sign-in-card">
        <div className="lock-badge"><LockKeyhole size={19}/></div>
        <p className="eyebrow">BUSINESS ACCESS</p>
        <h2>Sign in to your workspace</h2>
        <p className="intro-copy">Use your company account to view the branches and stations assigned to you.</p>
        {setup.kind === 'missing' && <div className="notice notice-warning" role="status"><strong>Identity setup is required.</strong><span>Set {setup.names.join(', ')} in the web app environment before connecting a sign-in provider.</span></div>}
        {setup.kind === 'invalid' && <div className="notice notice-danger" role="alert">{setup.message}</div>}
        {message && <div className="notice notice-danger" role="alert">{message}</div>}
        <button className="button button-primary sign-in-button" type="button" disabled={!configured} onClick={() => void onSignIn()}>
          <KeyRound size={17}/><span>Continue with secure sign-in</span><ArrowRight size={16}/>
        </button>
        <div className="security-note"><ShieldCheck size={16}/><span>Protected by your business identity provider using OpenID Connect and PKCE.</span></div>
        <div className="sign-in-divider"><span/><small>YOUR WORKSPACE</small><span/></div>
        <div className="feature-list"><div><Building2 size={16}/><span>Business and branch access</span></div><div><Monitor size={16}/><span>Assigned register stations</span></div><div><Users size={16}/><span>Role-based permissions</span></div></div>
      </div>
      <footer className="sign-in-footer"><span>Powered Solutions</span><span>For access, contact your business owner.</span></footer>
    </section>
  </main>
}

function Workspace({
  setup,
  user,
  tenants,
  selection,
  setSelection,
  onSignOut,
}: {
  setup: Extract<AuthSetup, { kind: 'ready' }>
  user: User
  tenants: TenantMembership[]
  selection: Selection
  setSelection: (selection: Selection) => void
  onSignOut: () => Promise<void>
}) {
  const [logoutConfirmOpen, setLogoutConfirmOpen] = useState(false)
  const logoutDialogRef = useRef<HTMLDialogElement>(null)
  const [tenantId, setTenantId] = useState(tenants[0]?.tenantId ?? '')
  const activeTenant = tenants.find((tenant) => tenant.tenantId === tenantId) ?? tenants[0]
  const location = selectedLocation(selection, tenants)
  const branchLocation = selectedBranch(selection, tenants)
  const profileName = displayName(user)
  const membershipsLabel = tenants.length === 1 ? '1 business' : `${tenants.length} businesses`
  const isTenantOwner = activeTenant?.roleGrants.some((grant) => grant.roleKey === 'owner' && grant.branchId === null && grant.stationId === null) ?? false
  const canManageTenantStaff = activeTenant?.roleGrants.some((grant) => ['owner', 'admin'].includes(grant.roleKey) && grant.branchId === null && grant.stationId === null) ?? false

  useEffect(() => {
    const dialog = logoutDialogRef.current
    if (logoutConfirmOpen && dialog && !dialog.open) dialog.showModal()
    return () => { if (dialog?.open) dialog.close() }
  }, [logoutConfirmOpen])

  async function chooseStation(branch: BranchAccess, station: StationAccess) {
    if (!activeTenant) return
    const current = { tenantId: activeTenant.tenantId, branchId: branch.branchId, stationId: station.stationId }
    setSelection({ kind: 'station-loading', ...current })
    try {
      const permissions = await getStationPermissions({
        apiBaseUrl: setup.apiBaseUrl,
        accessToken: user.access_token,
        ...current,
      })
      setSelection({ kind: 'station-ready', ...current, permissions })
    } catch (reason) {
      if (reason instanceof ApiError && reason.status === 401) {
        setSelection({ kind: 'station-error', ...current, message: 'Your session has expired. Sign in again to refresh your station access.' })
        return
      }
      setSelection({
        kind: 'station-error',
        ...current,
        message: reason instanceof Error ? reason.message : 'Station access could not be loaded.',
      })
    }
  }

  async function chooseBranch(branch: BranchAccess) {
    if (!activeTenant) return
    const current = { tenantId: activeTenant.tenantId, branchId: branch.branchId }
    setSelection({ kind: 'branch-loading', ...current })
    try {
      const permissions = await getBranchPermissions({
        apiBaseUrl: setup.apiBaseUrl,
        accessToken: user.access_token,
        ...current,
      })
      setSelection({ kind: 'branch-ready', ...current, permissions })
    } catch (reason) {
      setSelection({
        kind: 'branch-error',
        ...current,
        message: reason instanceof Error ? reason.message : 'Branch access could not be loaded.',
      })
    }
  }

  return <div className="workspace-shell">
    <aside className="workspace-sidebar" aria-label="Workspace navigation">
      <a className="sidebar-brand" href="#workspace" onClick={(event) => event.preventDefault()}>
        <span className="brand-mark">A</span><span><strong>Advanced POS</strong><small>BUSINESS DESK</small></span>
      </a>
      <div className="sidebar-section-label">WORKSPACE</div>
      <button className="side-nav-item active" type="button" aria-current="page"><LayoutDashboard size={17}/><span>Access overview</span></button>
      <div className="sidebar-section-label locations-label">YOUR LOCATIONS</div>
      <div className="location-count"><Store size={15}/><span>{membershipsLabel}</span><ChevronDown size={14}/></div>
      {activeTenant && <div className="sidebar-tenant"><strong>{activeTenant.displayName}</strong><small>{roleLabels(activeTenant).join(' · ') || 'No active role'}</small></div>}
      <div className="sidebar-spacer"/>
      <div className="sidebar-security"><ShieldCheck size={16}/><span><strong>Role protected</strong><small>Access is verified by the service</small></span></div>
      <button className="sidebar-user" type="button" onClick={() => setLogoutConfirmOpen(true)} aria-label={`Sign out ${profileName}`}>
        <span className="avatar">{profileName.slice(0, 1).toUpperCase()}</span><span className="user-copy"><strong>{profileName}</strong><small>Sign out</small></span><LogOut size={16}/>
      </button>
    </aside>
    <main className="workspace-main">
      <header className="workspace-topbar">
        <div><span className="topbar-overline">POWERED SOLUTIONS</span><strong>Business workspace</strong></div>
        <div className="topbar-account"><span className="connection-dot"/> API connected <span className="topbar-separator"/><span className="avatar avatar-small">{profileName.slice(0, 1).toUpperCase()}</span><span>{profileName}</span><button type="button" className="icon-button" onClick={() => setLogoutConfirmOpen(true)} aria-label="Sign out" title="Sign out"><LogOut size={17}/></button></div>
      </header>
      <div className="workspace-content">
        <DeploymentUpdateNotice />
        <section className="page-intro">
          <div><p className="eyebrow">YOUR ACCESS</p><h1>Good to have you, {profileName.split(/\s+/)[0]}.</h1><p className="intro-copy">Choose the location and register you are working with today.</p></div>
          <div className="verified-badge"><ShieldCheck size={15}/> Verified access</div>
        </section>
        {tenants.length === 0 ? <section className="empty-panel"><div className="empty-icon"><Building2 size={21}/></div><h2>No business access yet</h2><p>Your identity is signed in, but it has not been assigned to an Advanced POS business. Ask the business owner to invite you.</p></section> : <>
          <section className="tenant-strip" aria-label="Business selection">
            <span className="strip-label">BUSINESS</span>
            {tenants.map((tenant) => <button key={tenant.tenantId} className={`tenant-tab ${activeTenant?.tenantId === tenant.tenantId ? 'selected' : ''}`} type="button" onClick={() => { setTenantId(tenant.tenantId); setSelection({ kind: 'none' }); }} aria-pressed={activeTenant?.tenantId === tenant.tenantId}>
              <span className="tenant-avatar">{tenant.displayName.slice(0, 1).toUpperCase()}</span><span><strong>{tenant.displayName}</strong><small>{roleLabels(tenant).join(' · ') || 'Access assigned'}</small></span>{activeTenant?.tenantId === tenant.tenantId && <Check size={16}/>}</button>)}
          </section>
          {activeTenant && activeTenant.branches.length > 1 && activeTenant.roleGrants.some((grant) => ['owner', 'admin', 'manager'].includes(grant.roleKey)) && <TenantOverviewPanel
            key={activeTenant.tenantId}
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={activeTenant.tenantId}
            tenantName={activeTenant.displayName}
            onOpenBranch={(branchId) => {
              const branch = activeTenant.branches.find((item) => item.branchId === branchId)
              if (branch) void chooseBranch(branch)
            }}
          />}
          {activeTenant && isTenantOwner && <TenantStationCapacityPanel
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={activeTenant.tenantId}
            tenantName={activeTenant.displayName}
          />}
          <section className="location-section">
            <div className="section-heading"><div><p className="eyebrow">BRANCHES AND STATIONS</p><h2>Select where you are working</h2></div><span className="section-meta">Stock is shared by stations within a branch</span></div>
            {!activeTenant || activeTenant.branches.length === 0 ? <div className="empty-inline"><Building2 size={18}/><span>No branch or register station has been assigned for this business.</span></div> : <div className="branch-grid">
              {activeTenant.branches.map((branch) => <article className="branch-card" key={branch.branchId}>
                <div className="branch-card-head"><span className="branch-icon"><Building2 size={18}/></span><span className="branch-code">{branch.code}</span></div>
                <h3>{branch.name}</h3><p className="branch-timezone">{branch.timezone}</p>
                {(() => {
                  const selected = branchLocation?.branch.branchId === branch.branchId
                  const loading = selection.kind === 'branch-loading' && selection.branchId === branch.branchId
                  return <button className={`branch-access-button ${selected ? 'branch-access-selected' : ''}`} type="button" onClick={() => void chooseBranch(branch)} disabled={loading} aria-pressed={selected}>
                    <span>{loading ? 'Checking branch access…' : selected ? 'Branch access selected' : 'View branch workspace'}</span>{loading ? <span className="mini-spinner" aria-label="Loading access"/> : <ArrowRight size={14}/>}
                  </button>
                })()}
                <div className="station-label"><span>REGISTER STATIONS</span><span>{branch.stations.length}</span></div>
                {branch.stations.length === 0 ? <p className="no-stations">No station assigned at this branch.</p> : <div className="station-list">{branch.stations.map((station) => {
                  const isStationSelection = selection.kind === 'station-loading' || selection.kind === 'station-ready' || selection.kind === 'station-error'
                  const active = isStationSelection && selection.tenantId === activeTenant.tenantId && selection.branchId === branch.branchId && selection.stationId === station.stationId
                  const loading = selection.kind === 'station-loading' && active
                  return <button key={station.stationId} className={`station-row ${active ? 'station-active' : ''}`} type="button" onClick={() => void chooseStation(branch, station)} aria-pressed={active} disabled={loading}>
                    <span className="station-device"><Monitor size={16}/></span><span className="station-copy"><strong>{station.name}</strong><small>{station.code}</small></span><span className="station-action">{loading ? <span className="mini-spinner" aria-label="Loading access"/> : <ArrowRight size={15}/>}</span>
                  </button>
                })}</div>}
              </article>)}
            </div>}
          </section>
          {(selection.kind === 'station-error' || selection.kind === 'branch-error') && <div className="notice notice-danger location-error" role="alert">{selection.message}</div>}
          {branchLocation && selection.kind === 'branch-ready' && <section className="station-detail" aria-live="polite">
            <div className="detail-heading"><div className="detail-mark"><Check size={20}/></div><div><p className="eyebrow">BRANCH WORKSPACE READY</p><h2>{branchLocation.branch.name}</h2><p>{branchLocation.tenant.displayName} <span>/</span> {branchLocation.branch.code}</p></div><span className="verified-badge"><ShieldCheck size={14}/> Service verified</span></div>
            <div className="permission-heading"><div><h3>Branch permissions</h3><p>Your manager or administrator role controls these actions.</p></div><span>{selection.permissions.permissions.length} permissions</span></div>
            {selection.permissions.permissions.length === 0 ? <div className="empty-inline">No management actions are assigned for this branch.</div> : <div className="permission-list">{selection.permissions.permissions.map((permission) => <span className="permission-chip" key={permission}><Check size={13}/>{permission.replaceAll(':', ' · ')}</span>)}</div>}
            <div className="service-state"><span className="service-state-icon"><Boxes size={17}/></span><span><strong>Branch catalog and payment settings are available</strong><small>Hosted purchasing, checkout, cash control and full reporting are being connected here.</small></span></div>
          </section>}
          {branchLocation && selection.kind === 'branch-ready' && canManageTenantStaff && selection.permissions.permissions.includes('users:write') && <HostedStaffPanel
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={branchLocation.tenant.tenantId}
            branchId={branchLocation.branch.branchId}
            tenantName={branchLocation.tenant.displayName}
          />}
          {branchLocation && selection.kind === 'branch-ready' && selection.permissions.permissions.includes('attendance:self') && <HostedAttendancePanel
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={branchLocation.tenant.tenantId}
            branchId={branchLocation.branch.branchId}
            branchName={branchLocation.branch.name}
            canManage={selection.permissions.permissions.includes('attendance:manage')}
            canRead={selection.permissions.permissions.includes('attendance:read')}
          />}
          {branchLocation && selection.kind === 'branch-ready' && (selection.permissions.permissions.includes('sales:read') || selection.permissions.permissions.includes('products:read')) && <BranchOverviewPanel
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={branchLocation.tenant.tenantId}
            branchId={branchLocation.branch.branchId}
            branchName={branchLocation.branch.name}
            timeZone={branchLocation.branch.timezone}
            canViewSales={selection.permissions.permissions.includes('sales:read')}
            canViewInventory={selection.permissions.permissions.includes('products:read')}
          />}
          {branchLocation && selection.kind === 'branch-ready' && selection.permissions.permissions.includes('products:read') && <BranchProfilePanel
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={branchLocation.tenant.tenantId}
            branchId={branchLocation.branch.branchId}
            branchName={branchLocation.branch.name}
            canManage={selection.permissions.permissions.includes('branches:manage')}
          />}
          {branchLocation && selection.kind === 'branch-ready' && selection.permissions.permissions.includes('products:read') && <BranchCatalogPanel
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={branchLocation.tenant.tenantId}
            branchId={branchLocation.branch.branchId}
            branchName={branchLocation.branch.name}
            currencyCode={branchLocation.branch.currencyCode}
            canManageBranch={selection.permissions.permissions.includes('products:write')}
            canManageIdentity={selection.permissions.tenantPermissions.includes('products:write')}
          />}
          {branchLocation && selection.kind === 'branch-ready' && selection.permissions.permissions.includes('payments:read') && <BranchPaymentOptionsPanel
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={branchLocation.tenant.tenantId}
            branchId={branchLocation.branch.branchId}
            branchName={branchLocation.branch.name}
            canManage={selection.permissions.permissions.includes('payments:write')}
          />}
          {branchLocation && selection.kind === 'branch-ready' && selection.permissions.permissions.includes('support:write') && <SupportRequestsPanel
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={branchLocation.tenant.tenantId}
            branchId={branchLocation.branch.branchId}
            branchName={branchLocation.branch.name}
          />}
          {branchLocation && selection.kind === 'branch-ready' && selection.permissions.permissions.includes('sales:read') && <BranchSalesReportPanel
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={branchLocation.tenant.tenantId}
            branchId={branchLocation.branch.branchId}
            branchName={branchLocation.branch.name}
            timeZone={branchLocation.branch.timezone}
            canReviewAccounting={selection.permissions.permissions.includes('reports:read')}
          />}
          {branchLocation && selection.kind === 'branch-ready' && selection.permissions.permissions.includes('customers:manage') && <HostedCustomersPanel
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={branchLocation.tenant.tenantId}
            branchId={branchLocation.branch.branchId}
            branchName={branchLocation.branch.name}
            currencyCode={branchLocation.branch.currencyCode}
            canManageCreditTerms={selection.permissions.tenantPermissions.includes('customers:manage')}
          />}
          {branchLocation && selection.kind === 'branch-ready' && selection.permissions.permissions.includes('suppliers:write') && <HostedSuppliersPanel
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={branchLocation.tenant.tenantId}
            branchId={branchLocation.branch.branchId}
            branchName={branchLocation.branch.name}
          />}
          {branchLocation && selection.kind === 'branch-ready' && selection.permissions.permissions.includes('expenses:read') && <HostedExpensesPanel
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={branchLocation.tenant.tenantId}
            branchId={branchLocation.branch.branchId}
            branchName={branchLocation.branch.name}
            stations={branchLocation.branch.stations}
            canWrite={selection.permissions.permissions.includes('expenses:write')}
            canReclassify={selection.permissions.permissions.includes('expenses:reclassify')}
          />}
          {branchLocation && selection.kind === 'branch-ready' && selection.permissions.permissions.includes('cash:locations:read') && <HostedCashLocationsPanel
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={branchLocation.tenant.tenantId}
            branchId={branchLocation.branch.branchId}
            branchName={branchLocation.branch.name}
            stations={branchLocation.branch.stations}
            canManage={selection.permissions.permissions.includes('cash:locations:write')}
            canReconcile={selection.permissions.permissions.includes('accounting:close')}
          />}
          {branchLocation && selection.kind === 'branch-ready' && selection.permissions.permissions.includes('purchases:read') && <HostedPurchasesPanel
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={branchLocation.tenant.tenantId}
            branchId={branchLocation.branch.branchId}
            branchName={branchLocation.branch.name}
            currencyCode={branchLocation.branch.currencyCode}
            canWrite={selection.permissions.permissions.includes('purchases:write')}
            canInvoice={selection.permissions.permissions.includes('purchases:invoice')}
            canSettle={selection.permissions.permissions.includes('suppliers:settle')}
          />}
          {branchLocation && selection.kind === 'branch-ready' && selection.permissions.permissions.includes('inventory:transfer:read') && <BranchStockTransfersPanel
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={branchLocation.tenant.tenantId}
            branchId={branchLocation.branch.branchId}
            branchName={branchLocation.branch.name}
            canManage={selection.permissions.permissions.includes('inventory:transfer:manage')}
          />}
          {branchLocation && selection.kind === 'branch-ready' && selection.permissions.permissions.includes('inventory:pickup:manage') && <HostedPickupQueuePanel
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={branchLocation.tenant.tenantId}
            branchId={branchLocation.branch.branchId}
            branchName={branchLocation.branch.name}
            scope={{ kind: 'branch' }}
            membershipId={branchLocation.tenant.membershipId}
            canPrepare={selection.permissions.permissions.includes('inventory:pickup:prepare')}
            canCancel={selection.permissions.permissions.includes('inventory:pickup:request')}
            canManage
          />}
          {location && selection.kind === 'station-ready' && <section className="station-detail" aria-live="polite">
            <div className="detail-heading"><div className="detail-mark"><Check size={20}/></div><div><p className="eyebrow">STATION ACCESS READY</p><h2>{location.station.name}</h2><p>{location.tenant.displayName} <span>/</span> {location.branch.name} <span>/</span> {location.station.code}</p></div><span className="verified-badge"><ShieldCheck size={14}/> Service verified</span></div>
            <div className="permission-heading"><div><h3>What you can do here</h3><p>These permissions come from your assigned role and location scope.</p></div><span>{selection.permissions.permissions.length} permissions</span></div>
            {selection.permissions.permissions.length === 0 ? <div className="empty-inline">No actions are assigned for this station.</div> : <div className="permission-list">{selection.permissions.permissions.map((permission) => <span className="permission-chip" key={permission}><Check size={13}/>{permission.replaceAll(':', ' · ')}</span>)}</div>}
            <div className="service-state"><span className="service-state-icon"><Boxes size={17}/></span><span><strong>Station access is ready</strong><small>Register actions are controlled by your permissions. Only an assigned cashier profile can hold the station and complete sales.</small></span></div>
          </section>}
          {location && selection.kind === 'station-ready' && <HostedRegisterPanel
            key={`${location.tenant.tenantId}:${location.branch.branchId}:${location.station.stationId}:${location.tenant.membershipId}`}
            apiBaseUrl={setup.apiBaseUrl}
            accessToken={user.access_token}
            tenantId={location.tenant.tenantId}
            branchId={location.branch.branchId}
            stationId={location.station.stationId}
            branchName={location.branch.name}
            stationName={location.station.name}
            subject={location.tenant.membershipId}
            canView={selection.permissions.permissions.includes('pos:use')}
            canCheckout={selection.permissions.permissions.includes('sales:checkout')}
            canActivate={selection.permissions.tenantPermissions.includes('license:manage')}
            canCheckOtherBranches={selection.permissions.permissions.includes('inventory:availability:read')}
            canRequestTransfers={selection.permissions.permissions.includes('inventory:transfer:request')}
            canRequestPickups={selection.permissions.permissions.includes('inventory:pickup:request')}
            canReadPickups={selection.permissions.permissions.includes('inventory:pickup:read')}
            canPreparePickups={selection.permissions.permissions.includes('inventory:pickup:prepare')}
            canSettle={selection.permissions.permissions.includes('suppliers:settle')}
            canRecordCashExpense={selection.permissions.permissions.includes('expenses:cash:write')}
            canRefund={selection.permissions.permissions.includes('sales:refund')}
          />}
        </>}
        <footer className="page-footer"><span>Advanced POS <b>·</b> Powered Solutions</span><span>Business access is checked by the API for each location.</span></footer>
      </div>
      <dialog ref={logoutDialogRef} className="logout-dialog" aria-labelledby="logout-title" aria-describedby="logout-description" onCancel={(event) => { event.preventDefault(); setLogoutConfirmOpen(false) }}>
        <span className="lock-badge"><LogOut size={18}/></span><h2 id="logout-title">Sign out of Advanced POS?</h2><p id="logout-description">Your access token will be cleared when the identity provider signs you out.</p>
        <div className="dialog-actions"><button className="button button-quiet" type="button" onClick={() => setLogoutConfirmOpen(false)}>Stay signed in</button><button className="button button-primary" type="button" onClick={() => void onSignOut()}>Sign out</button></div>
      </dialog>
    </main>
  </div>
}

export default function App() {
  const setup = useMemo(createAuthSetup, [])
  const [state, setState] = useState<ScreenState>({ kind: 'booting' })

  useEffect(() => {
    if (setup.kind !== 'ready') {
      setState({ kind: 'unconfigured', names: setup.kind === 'missing' ? setup.names : [], message: setup.kind === 'invalid' ? setup.message : '' })
      return
    }
    const manager = setup.manager
    const apiBaseUrl = setup.apiBaseUrl
    let active = true
    async function loadSession() {
      try {
        if (window.location.pathname === '/auth/callback') {
          signInCallback ??= manager.signinRedirectCallback()
        }
        const user = window.location.pathname === '/auth/callback'
          ? await signInCallback
          : await manager.getUser()
        if (window.location.pathname === '/auth/callback') window.history.replaceState({}, document.title, '/')
        if (!active) return
        if (!user || user.expired || user.access_token.length === 0) {
          await manager.removeUser()
          setState({ kind: 'signed-out', message: null })
          return
        }
        setState({ kind: 'loading-access', user })
        const tenants = await listTenantAccess({ apiBaseUrl, accessToken: user.access_token })
        if (active) setState({ kind: 'workspace', user, tenants, selection: { kind: 'none' } })
      } catch (reason) {
        if (!active) return
        setState({ kind: 'signed-out', message: reason instanceof Error ? reason.message : 'Sign-in could not be completed.' })
      }
    }
    void loadSession()
    return () => { active = false }
  }, [setup])

  async function signIn() {
    if (setup.kind !== 'ready') return
    try {
      await setup.manager.signinRedirect()
    } catch (reason) {
      setState({ kind: 'signed-out', message: reason instanceof Error ? reason.message : 'The identity provider could not be opened.' })
    }
  }

  async function signOut() {
    if (setup.kind !== 'ready' || state.kind !== 'workspace') return
    try {
      await setup.manager.signoutRedirect({ id_token_hint: state.user.id_token })
    } catch (reason) {
      setState({ kind: 'workspace-error', user: state.user, message: reason instanceof Error ? reason.message : 'Sign-out could not be started.' })
    }
  }

  switch (state.kind) {
    case 'booting':
    case 'loading-access':
      return <main className="boot-screen" aria-live="polite"><span className="brand-mark">A</span><span className="loading-dot"/>Loading your business access…</main>
    case 'unconfigured':
      return <HomeScreen setup={setup} message={state.message || null} onSignIn={signIn}/>
    case 'signed-out':
      return <HomeScreen setup={setup} message={state.message} onSignIn={signIn}/>
    case 'workspace':
      if (setup.kind !== 'ready') return <HomeScreen setup={setup} message="Identity configuration changed. Reload the page to continue." onSignIn={signIn}/>
      return <Workspace setup={setup} user={state.user} tenants={state.tenants} selection={state.selection} setSelection={(selection) => setState({ ...state, selection })} onSignOut={signOut}/>
    case 'workspace-error':
      return <HomeScreen setup={setup} message={state.message} onSignIn={signIn}/>
    default:
      return assertNever(state)
  }
}
