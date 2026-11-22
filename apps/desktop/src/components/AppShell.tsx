// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Activity, Archive, BarChart3, BookOpen, Boxes, Building2, Check, ClipboardList, CreditCard, Expand, KeyRound, LayoutDashboard, LogOut, Menu, Minimize2, PackagePlus, PanelLeftClose, PanelLeftOpen, Search, Settings2, ShoppingBasket, Users, Wallet, X } from "lucide-react";
import { posApi } from "../lib/api";
import type { DashboardSummary, SessionUser, StaffProfile } from "../lib/models";
import { formatMoney } from "../lib/money";
import { openCustomerDisplay } from "../lib/customerDisplay";
import { RegisterPage } from "../pages/RegisterPage";
const ProductsPage = lazy(() => import("../pages/ProductsPage").then((module) => ({ default: module.ProductsPage })));
const SalesPage = lazy(() => import("../pages/SalesPage").then((module) => ({ default: module.SalesPage })));
const PeoplePage = lazy(() => import("../pages/PeoplePage").then((module) => ({ default: module.PeoplePage })));
const ExpensesPage = lazy(() => import("../pages/ExpensesPage").then((module) => ({ default: module.ExpensesPage })));
const PurchasesPage = lazy(() => import("../pages/PurchasesPage").then((module) => ({ default: module.PurchasesPage })));
const SettingsPage = lazy(() => import("../pages/SettingsPage").then((module) => ({ default: module.SettingsPage })));
const ReportsPage = lazy(() => import("../pages/ReportsPage").then((module) => ({ default: module.ReportsPage })));
const AccountPage = lazy(() => import("../pages/AccountPage").then((module) => ({ default: module.AccountPage })));
const AuditPage = lazy(() => import("../pages/AuditPage").then((module) => ({ default: module.AuditPage })));
const AccountingPage = lazy(() => import("../pages/AccountingPage").then((module) => ({ default: module.AccountingPage })));
import { WorkTimer } from "./WorkTimer";
import { GlobalSearchDialog } from "./GlobalSearchDialog";
import { useAttemptCountdown } from "../lib/useAttemptCountdown";
import "./global-search.css";
import brandMark from "../assets/nimbletill-mark.svg";

type View = "overview" | "register" | "products" | "people" | "sales" | "expenses" | "purchases" | "reports" | "accounting" | "audit" | "settings" | "account";

function readZoomPercent() {
  const value = Number(localStorage.getItem("nimbletill:zoom-percent"));
  return Number.isFinite(value) && value >= 80 && value <= 160 ? value : 100;
}

function readSidebarCollapsed() {
  return localStorage.getItem("nimbletill:sidebar-collapsed") === "true";
}

export function AppShell({ user, onSignOut, onSessionChange }: { user: SessionUser; onSignOut: () => Promise<void>; onSessionChange: (user: SessionUser) => void }) {
  const [view, setView] = useState<View>("overview");
  const [peopleDirectory, setPeopleDirectory] = useState<"customers" | "suppliers">("customers");
  const [summary, setSummary] = useState<DashboardSummary | null>(null);
  const [refresh, setRefresh] = useState(0);
  const [now, setNow] = useState(() => new Date());
  const [zoomPercent, setZoomPercent] = useState(readZoomPercent);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(readSidebarCollapsed);
  const [mobileSidebarOpen, setMobileSidebarOpen] = useState(false);
  const [globalSearchOpen, setGlobalSearchOpen] = useState(false);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [cashierStartsAtRegister, setCashierStartsAtRegister] = useState(true);
  const [staffPickerOpen, setStaffPickerOpen] = useState(false);
  const [logoutConfirmOpen, setLogoutConfirmOpen] = useState(false);
  const [signingOut, setSigningOut] = useState(false);
  const [staffProfiles, setStaffProfiles] = useState<StaffProfile[]>([]);
  const [selectedStaffId, setSelectedStaffId] = useState("");
  const [staffCredential, setStaffCredential] = useState("");
  const [staffPickerError, setStaffPickerError] = useState("");
  const [loadingStaff, setLoadingStaff] = useState(false);
  const [switchingStaff, setSwitchingStaff] = useState(false);
  const [cashierModeDialogOpen, setCashierModeDialogOpen] = useState(false);
  const [cashierModeCredential, setCashierModeCredential] = useState("");
  const [cashierModeError, setCashierModeError] = useState("");
  const [changingCashierMode, setChangingCashierMode] = useState(false);
  const attemptCountdown = useAttemptCountdown();
  const registerController = useRef<{ prepareForStaffSwitch: () => Promise<void> } | null>(null);
  const isCashier = user.workspaceMode === "cashier" && !["owner", "admin", "manager"].includes(user.roleKey);
  const isTemporaryCashier = user.permissions.includes("pos:transact");
  const canTransact = isCashier || isTemporaryCashier;
  const registerMode = view === "register" && user.permissions.includes("pos:use");

  useEffect(() => {
    document.body.classList.toggle("register-mode-active", registerMode);
    return () => document.body.classList.remove("register-mode-active");
  }, [registerMode]);

  async function changeTemporaryCashierMode() {
    if (!cashierModeCredential || changingCashierMode) return;
    setChangingCashierMode(true);
    setCashierModeError("");
    try {
      const nextUser = await posApi.setTemporaryCashierMode({
        enabled: !isTemporaryCashier,
        credential: cashierModeCredential,
      });
      setCashierModeCredential("");
      setCashierModeDialogOpen(false);
      onSessionChange(nextUser);
    } catch (reason) {
      const message = typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "Cashier mode could not be changed.";
      attemptCountdown.observe(message);
      setCashierModeError(message);
    } finally {
      setChangingCashierMode(false);
    }
  }

  useEffect(() => { void posApi.dashboardSummary().then(setSummary).catch(() => setSummary(null)); }, [refresh]);
  useEffect(() => {
    localStorage.setItem("nimbletill:zoom-percent", String(zoomPercent));
    void getCurrentWebviewWindow().setZoom(zoomPercent / 100).catch(() => {});
  }, [zoomPercent]);
  useEffect(() => {
    localStorage.setItem("nimbletill:sidebar-collapsed", String(sidebarCollapsed));
  }, [sidebarCollapsed]);
  useEffect(() => {
    if (user.workspaceMode !== "cashier") return;
    let active = true;
    void posApi.registerPreferences().then(async (preferences) => {
      if (active) setCashierStartsAtRegister(preferences.cashierStartAtRegister);
      if (!active || !preferences.cashierStartAtRegister) return;
      setView("register");
      if (preferences.cashierAutoFullscreen) {
        await getCurrentWindow().setFullscreen(true);
        if (active) setIsFullscreen(true);
      }
      if (preferences.customerDisplayEnabled && active) {
        await openCustomerDisplay(preferences).catch(() => {});
      }
    }).catch(() => {});
    return () => { active = false; };
  }, [user.workspaceMode]);
  useEffect(() => { const timer = window.setInterval(() => setNow(new Date()), 60_000); return () => window.clearInterval(timer); }, []);

  const toggleFullscreen = useCallback(async () => {
    const next = !(await getCurrentWindow().isFullscreen());
    await getCurrentWindow().setFullscreen(next);
    setIsFullscreen(next);
  }, []);

  const navigateTo = useCallback((next: View) => {
    setView(next);
    setMobileSidebarOpen(false);
    if (next !== "register") return;
    void posApi.registerPreferences().then(async (preferences) => {
      if (user.workspaceMode === "cashier" && preferences.cashierAutoFullscreen) {
        await getCurrentWindow().setFullscreen(true);
        setIsFullscreen(true);
      }
      if (preferences.customerDisplayEnabled) await openCustomerDisplay(preferences);
    }).catch(() => {});
  }, [user.workspaceMode]);
  const setRegisterController = useCallback((controller: { prepareForStaffSwitch: () => Promise<void> } | null) => {
    registerController.current = controller;
  }, []);

  async function openStaffPicker() {
    setStaffPickerOpen(true);
    setLoadingStaff(true);
    setStaffPickerError("");
    setStaffCredential("");
    attemptCountdown.reset();
    try {
      const profiles = await posApi.staffProfiles();
      setStaffProfiles(profiles);
      setSelectedStaffId(user.userId);
    } catch (reason) {
      setStaffPickerError(typeof reason === "string" ? reason : "Staff profiles could not be loaded.");
    } finally {
      setLoadingStaff(false);
    }
  }

  async function switchStaffProfile() {
    if (!selectedStaffId || !staffCredential) return;
    setSwitchingStaff(true);
    setStaffPickerError("");
    try {
      if (user.permissions.includes("pos:use")) {
        if (registerController.current) {
          await registerController.current.prepareForStaffSwitch();
        } else {
          const workspace = await posApi.registerWorkspace();
          const active = workspace.active;
          const hasWork = active.lines.length > 0 || active.customerId !== null || active.discountMinor > 0 || active.amountTenderedMinor > 0;
          if (hasWork) await posApi.editRegisterWorkspace({ expectedRevision: workspace.revision, action: { type: "holdActive" } });
        }
      }
      const nextUser = await posApi.switchStaffProfile({ userId: selectedStaffId, credential: staffCredential });
      setStaffPickerOpen(false);
      onSessionChange(nextUser);
    } catch (reason) {
      const message = typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "The profile could not be switched. Your current session is still active.";
      attemptCountdown.observe(message);
      setStaffPickerError(message);
    } finally {
      setSwitchingStaff(false);
    }
  }
  async function confirmSignOut() {
    setSigningOut(true);
    try {
      await onSignOut();
      setLogoutConfirmOpen(false);
    } finally {
      setSigningOut(false);
    }
  }
  function toggleSidebar() {
    if (window.matchMedia("(max-width: 760px)").matches) {
      setMobileSidebarOpen((open) => !open);
      return;
    }
    setSidebarCollapsed((collapsed) => !collapsed);
  }
  useEffect(() => {
    function onShortcut(event: KeyboardEvent) {
      if (event.key === "F2") { event.preventDefault(); navigateTo("register"); }
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") { event.preventDefault(); setGlobalSearchOpen(true); }
      if (event.key === "F11") { event.preventDefault(); void toggleFullscreen(); }
      if (event.key === "Escape") setMobileSidebarOpen(false);
    }
    window.addEventListener("keydown", onShortcut);
    return () => window.removeEventListener("keydown", onShortcut);
  }, [navigateTo, toggleFullscreen]);

  const nav = [
    ...(!isCashier || !cashierStartsAtRegister ? [{ id: "overview" as const, label: "Overview", icon: LayoutDashboard }] : []),
    { id: "register" as const, label: "Point of sale", icon: ShoppingBasket },
    { id: "account" as const, label: "My account & work", icon: KeyRound },
    ...(!isCashier && user.permissions.includes("products:read") ? [{ id: "products" as const, label: "Products & stock", icon: Boxes }] : []),
    ...(!isCashier && user.permissions.includes("sales:read") ? [{ id: "sales" as const, label: "Sales history", icon: ClipboardList }] : []),
  ];

  return <div className={`app-frame ${registerMode ? "register-mode" : ""} ${sidebarCollapsed ? "sidebar-layout-collapsed" : ""} ${mobileSidebarOpen ? "mobile-sidebar-open" : ""}`}>
    <aside className="sidebar" id="primary-sidebar" aria-label="Primary navigation">
      <div className="sidebar-header"><a className="brand-lockup" href="#home" title="Advanced POS — Overview" onClick={(e) => { e.preventDefault(); setView("overview"); setMobileSidebarOpen(false); }}><span className="brand-mark"><img src={brandMark} alt=""/></span><span><strong>Advanced <span>POS</span></strong><small>POWERED SOLUTIONS</small></span></a><button className="sidebar-toggle" type="button" onClick={toggleSidebar} aria-label={mobileSidebarOpen ? "Close navigation" : sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"} aria-controls="primary-sidebar" aria-expanded={mobileSidebarOpen || !sidebarCollapsed} title={mobileSidebarOpen ? "Close navigation" : sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}>{mobileSidebarOpen ? <X size={17}/> : sidebarCollapsed ? <PanelLeftOpen size={17}/> : <PanelLeftClose size={17}/>}</button></div>
      <div className="store-switcher" title={sidebarCollapsed ? `${user.storeName} — ${user.businessName}` : undefined}><span className="store-avatar">{user.businessName.slice(0, 1).toUpperCase()}</span><span className="store-meta"><strong>{user.storeName}</strong><small>{user.businessName}</small></span><span className="switch-dot" /></div>
      {!isCashier && <p className="nav-label">WORKSPACE</p>}
      <nav className="primary-nav" aria-label="Main navigation">{nav.map((item) => <button key={item.id} className={`nav-item ${view === item.id ? "active" : ""}`} onClick={() => navigateTo(item.id)} aria-label={sidebarCollapsed ? item.label : undefined} title={sidebarCollapsed ? item.label : undefined}><item.icon size={18} /><span>{item.label}</span>{item.id === "register" && <kbd>F2</kbd>}</button>)}</nav>
      {!isCashier && <><p className="nav-label nav-label-spaced">MANAGE</p>
      <nav className="primary-nav" aria-label="Management navigation">
        <button className={`nav-item ${view === "people" && peopleDirectory === "customers" ? "active" : "muted-nav"}`} onClick={() => { setPeopleDirectory("customers"); setView("people"); setMobileSidebarOpen(false); }} aria-label={sidebarCollapsed ? "Customers" : undefined} title={sidebarCollapsed ? "Customers" : undefined}><Users size={18}/><span>Customers</span></button>
        {user.permissions.includes("suppliers:read") && <button className={`nav-item ${view === "people" && peopleDirectory === "suppliers" ? "active" : "muted-nav"}`} onClick={() => { setPeopleDirectory("suppliers"); setView("people"); setMobileSidebarOpen(false); }} aria-label={sidebarCollapsed ? "Suppliers" : undefined} title={sidebarCollapsed ? "Suppliers" : undefined}><Building2 size={18}/><span>Suppliers</span></button>}
        {user.permissions.includes("expenses:read") && <button className={`nav-item ${view === "expenses" ? "active" : "muted-nav"}`} onClick={() => { setView("expenses"); setMobileSidebarOpen(false); }} aria-label={sidebarCollapsed ? "Expenses" : undefined} title={sidebarCollapsed ? "Expenses" : undefined}><Wallet size={18}/><span>Expenses</span></button>}
        {user.permissions.includes("purchases:read") && <button className={`nav-item ${view === "purchases" ? "active" : "muted-nav"}`} onClick={() => { setView("purchases"); setMobileSidebarOpen(false); }} aria-label={sidebarCollapsed ? "Purchases" : undefined} title={sidebarCollapsed ? "Purchases" : undefined}><PackagePlus size={18}/><span>Purchases</span></button>}
        {user.permissions.includes("reports:read") && <button className={`nav-item ${view === "reports" ? "active" : "muted-nav"}`} onClick={() => navigateTo("reports")} aria-label={sidebarCollapsed ? "Reports" : undefined} title={sidebarCollapsed ? "Reports" : undefined}><BarChart3 size={18}/><span>Reports</span></button>}
        {user.permissions.includes("reports:read") && <button className={`nav-item ${view === "accounting" ? "active" : "muted-nav"}`} onClick={() => navigateTo("accounting")} aria-label={sidebarCollapsed ? "Accounting journal" : undefined} title={sidebarCollapsed ? "Accounting journal" : undefined}><BookOpen size={18}/><span>Accounting</span></button>}
        {user.permissions.includes("reports:read") && <button className={`nav-item ${view === "audit" ? "active" : "muted-nav"}`} onClick={() => navigateTo("audit")} aria-label={sidebarCollapsed ? "Activity log" : undefined} title={sidebarCollapsed ? "Activity log" : undefined}><Activity size={18}/><span>Activity log</span></button>}
        {user.permissions.includes("settings:write") && <button className={`nav-item ${view === "settings" ? "active" : "muted-nav"}`} onClick={() => { setView("settings"); setMobileSidebarOpen(false); }} aria-label={sidebarCollapsed ? "Settings" : undefined} title={sidebarCollapsed ? "Settings" : undefined}><Settings2 size={18}/><span>Settings</span></button>}
      </nav></>}
      <div className="sidebar-spacer" />
      <div className="sidebar-status"><span className="status-light"/><span><strong>Local store</strong><small>Saved on this device</small></span><Activity size={16}/></div>
      <button className="user-card" onClick={() => setLogoutConfirmOpen(true)} title={sidebarCollapsed ? `${user.fullName} — Sign out` : undefined} aria-label={sidebarCollapsed ? `${user.fullName}, ${user.roleName}. Sign out` : undefined}><span className="user-avatar">{user.fullName.split(/\s+/).map((n) => n[0]).slice(0, 2).join("").toUpperCase()}</span><span className="user-meta"><strong>{user.fullName}</strong><small>{user.roleName}</small></span><LogOut size={16}/></button>
    </aside>
    {mobileSidebarOpen && <button className="sidebar-backdrop" type="button" onClick={() => setMobileSidebarOpen(false)} aria-label="Close navigation" />}
    <main className="workspace">
      <header className="topbar">
        <div className="breadcrumb">
          <button className="mobile-sidebar-toggle" type="button" onClick={() => setMobileSidebarOpen(true)} aria-label="Open navigation" aria-controls="primary-sidebar" aria-expanded={mobileSidebarOpen}><Menu size={18}/></button>
          <span>Workspace</span><span className="crumb-slash">/</span>
          <strong>{nav.find((item) => item.id === view)?.label ?? (view === "people" ? "People & partners" : view === "expenses" ? "Expenses" : view === "purchases" ? "Purchases & receiving" : view === "reports" ? "Reports" : view === "accounting" ? "Accounting journal" : view === "audit" ? "Activity log" : view === "settings" ? "Settings" : "Overview")}</strong>
        </div>
        <div className="topbar-actions">
          <WorkTimer userId={user.userId} temporaryCashierMode={isTemporaryCashier} onClick={() => navigateTo("account")}/>
          <span className="local-badge"><span/> Offline ready</span>
          {!isCashier && <button className="icon-button" aria-label="Search workspace" title="Search workspace (Ctrl+K)" onClick={() => setGlobalSearchOpen(true)}><Search size={18}/></button>}
          {view === "register" && <>
            <span className="register-topbar-status" aria-label="Register open"><span className="session-light"/><span>Register open</span></span>
            {!isCashier && user.permissions.includes("pos:use") && <button className={`button button-small ${isTemporaryCashier ? "button-primary" : "button-secondary"}`} type="button" onClick={() => { setCashierModeError(""); setCashierModeCredential(""); setCashierModeDialogOpen(true); }}>{isTemporaryCashier ? "End cashier mode" : "Enable checkout"}</button>}
            <button className="button button-secondary button-small register-fullscreen-toggle" type="button" onClick={() => void toggleFullscreen()} aria-label={isFullscreen ? "Exit full screen" : "Enter full screen"} title={isFullscreen ? "Exit full screen" : "Enter full screen"}>
              {isFullscreen ? <Minimize2 size={15}/> : <Expand size={15}/>}<span>{isFullscreen ? "Exit full screen" : "Full screen"}</span>
            </button>
          </>}
          <button className="staff-switch-trigger" type="button" onClick={() => void openStaffPicker()} aria-label={`Switch staff profile. Current profile: ${user.fullName}, ${user.roleName}`}><span className="user-avatar">{user.fullName.split(/\s+/).map((n) => n[0]).slice(0, 2).join("").toUpperCase()}</span><span className="staff-switch-copy"><strong>{user.fullName}</strong><small>{user.roleName} · Switch</small></span><KeyRound size={14}/></button>
          <span className="topbar-divider"/><span className="topbar-date">{new Intl.DateTimeFormat("en", { dateStyle: "medium" }).format(now)}</span>
        </div>
      </header>
      <div className="page-area">
        <Suspense fallback={<div className="page-loading" role="status">Opening workspace…</div>}>
        {view === "account" && <AccountPage user={user} onSignOut={() => setLogoutConfirmOpen(true)}/>}
        {view === "overview" && (!isCashier || !cashierStartsAtRegister) && <Overview user={user} summary={summary} now={now} onNavigate={navigateTo} />}
        {view === "register" && user.permissions.includes("pos:use") && <RegisterPage user={user} canTransact={canTransact} onComplete={() => setRefresh((n) => n + 1)} onControllerReady={setRegisterController} />}
        {view === "products" && !isCashier && (user.permissions.includes("products:write") || user.permissions.includes("inventory:write")) && <ProductsPage user={user} onUpdated={() => setRefresh((n) => n + 1)} />}
        {view === "people" && !isCashier && <PeoplePage key={peopleDirectory} user={user} initialDirectory={peopleDirectory} />}
        {view === "sales" && user.permissions.includes("sales:read") && <SalesPage user={user} />}
        {view === "expenses" && user.permissions.includes("expenses:read") && <ExpensesPage user={user} />}
        {view === "purchases" && user.permissions.includes("purchases:read") && <PurchasesPage user={user} />}
        {view === "reports" && user.permissions.includes("reports:read") && <ReportsPage user={user} />}
        {view === "accounting" && user.permissions.includes("reports:read") && <AccountingPage user={user} />}
        {view === "audit" && user.permissions.includes("reports:read") && <AuditPage />}
        {view === "settings" && user.permissions.includes("settings:write") && <SettingsPage user={user} zoomPercent={zoomPercent} onZoomChange={setZoomPercent} onSessionChange={onSessionChange} />}
        </Suspense>
      </div>
    </main>
    {cashierModeDialogOpen && <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !changingCashierMode) setCashierModeDialogOpen(false); }}><article className="receipt-modal" role="dialog" aria-modal="true" aria-labelledby="temporary-cashier-title"><button className="modal-close" type="button" onClick={() => setCashierModeDialogOpen(false)} disabled={changingCashierMode} aria-label="Close cashier mode dialog"><X size={18}/></button><p className="eyebrow">REGISTER PERMISSION</p><h2 id="temporary-cashier-title">{isTemporaryCashier ? "End temporary cashier mode?" : "Enable temporary cashier mode?"}</h2><p className="receipt-number">{isTemporaryCashier ? "Checkout, drawer opening, cash movements, and end-of-day closing will be locked again. Your manager or owner access stays active." : "You can inspect and prepare register tickets without this. Confirm your own PIN or password to enable checkout and cash drawer actions for this session."}</p><label className="field-label" htmlFor="temporary-cashier-credential">Your PIN or password<input id="temporary-cashier-credential" className="text-input" type="password" autoComplete="current-password" value={cashierModeCredential} onChange={(event) => setCashierModeCredential(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void changeTemporaryCashierMode(); }} autoFocus /></label>{cashierModeError && <p className="form-error" role="alert">{attemptCountdown.format(cashierModeError)}</p>}{attemptCountdown.remainingSeconds > 0 && <p className="settings-hint" role="status">Try again in {attemptCountdown.remainingSeconds} seconds.</p>}<div className="modal-actions"><button className="button button-secondary" type="button" onClick={() => setCashierModeDialogOpen(false)} disabled={changingCashierMode}>Cancel</button><button className="button button-primary" type="button" onClick={() => void changeTemporaryCashierMode()} disabled={changingCashierMode || !cashierModeCredential || attemptCountdown.remainingSeconds > 0}>{changingCashierMode ? "Updating…" : isTemporaryCashier ? "End cashier mode" : "Enable checkout"}<KeyRound size={15}/></button></div></article></div>}
    {staffPickerOpen && <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !switchingStaff) setStaffPickerOpen(false); }}><article className="receipt-modal staff-profile-picker" role="dialog" aria-modal="true" aria-labelledby="staff-picker-title"><button className="modal-close" type="button" onClick={() => setStaffPickerOpen(false)} disabled={switchingStaff} aria-label="Close staff profile picker"><X size={19}/></button><p className="eyebrow">SHIFT ACCESS</p><h2 id="staff-picker-title">Switch staff profile</h2><p className="receipt-number">Each team member uses their own access code. An open ticket is saved before the change.</p><div className="staff-picker-list">{staffProfiles.map((profile) => <button key={profile.userId} type="button" className={`staff-picker-option ${selectedStaffId === profile.userId ? "selected" : ""} ${profile.userId === user.userId ? "current-profile" : ""}`} onClick={() => { setSelectedStaffId(profile.userId); setStaffCredential(""); setStaffPickerError(""); attemptCountdown.reset(); }}><span className="user-avatar">{profile.fullName.split(/\s+/).map((n) => n[0]).slice(0, 2).join("").toUpperCase()}</span><span><strong>{profile.fullName}</strong><small>{profile.roleName} · {profile.userId === user.userId ? "Current profile" : profile.hasPin ? "PIN or password" : "Password"}</small></span>{profile.userId === user.userId ? <span className="staff-current-badge">Current</span> : selectedStaffId === profile.userId && <Check size={17}/>}</button>)}{loadingStaff && <p className="settings-hint">Loading staff profiles…</p>}</div><label className="field-label" htmlFor="staff-switch-credential">{staffProfiles.find((profile) => profile.userId === selectedStaffId)?.hasPin ? "PIN or password" : "Password"}<input id="staff-switch-credential" className="text-input" type="password" autoComplete="current-password" value={staffCredential} onChange={(event) => setStaffCredential(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void switchStaffProfile(); }} /></label>{staffPickerError && <p className="form-error" role="alert">{attemptCountdown.format(staffPickerError)}</p>}{attemptCountdown.remainingSeconds > 0 && <p className="settings-hint" role="status">Profile switching unlocks in {attemptCountdown.remainingSeconds} seconds.</p>}<button className="button button-primary receipt-done" type="button" disabled={loadingStaff || switchingStaff || attemptCountdown.remainingSeconds > 0 || !selectedStaffId || !staffCredential} onClick={() => void switchStaffProfile()}>{switchingStaff ? "Switching profile…" : attemptCountdown.remainingSeconds > 0 ? `Try again in ${attemptCountdown.remainingSeconds}s` : "Continue as selected staff"}<KeyRound size={16}/></button></article></div>}
    {logoutConfirmOpen && <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !signingOut) setLogoutConfirmOpen(false); }}><article className="receipt-modal logout-confirm-modal" role="dialog" aria-modal="true" aria-labelledby="logout-confirm-title"><button className="modal-close" type="button" onClick={() => setLogoutConfirmOpen(false)} disabled={signingOut} aria-label="Cancel sign out"><X size={18}/></button><p className="eyebrow">END THIS SESSION</p><h2 id="logout-confirm-title">Sign out of Advanced POS?</h2><p className="receipt-number">Your saved sales and work record stay on this device. You’ll need your sign-in details to return.</p><div className="modal-actions"><button className="button button-secondary" type="button" disabled={signingOut} onClick={() => setLogoutConfirmOpen(false)}>Stay signed in</button><button className="button button-primary" type="button" disabled={signingOut} onClick={() => void confirmSignOut()}>{signingOut ? "Signing out…" : "Sign out"}<LogOut size={15}/></button></div></article></div>}
    {globalSearchOpen && <GlobalSearchDialog user={user} onClose={() => setGlobalSearchOpen(false)} onNavigate={(next, directory) => { if (directory) setPeopleDirectory(directory); navigateTo(next); }}/>}
  </div>;
}

function Overview({ user, summary, now, onNavigate }: { user: SessionUser; summary: DashboardSummary | null; now: Date; onNavigate: (view: View) => void }) {
  const sales = summary?.todaySalesMinor ?? 0;
  return <section className="overview-page">
    <div className="page-heading"><div><p className="eyebrow">{new Intl.DateTimeFormat("en", { weekday: "long", month: "long", day: "numeric" }).format(now).toUpperCase()}</p><h1>Good day, {user.fullName.split(" ")[0]}<span className="wave">✳</span></h1><p className="page-subtitle">Here’s what’s happening at {user.storeName} today.</p></div><button className="button button-primary" onClick={() => onNavigate("register")}><ShoppingBasket size={17}/> New sale <kbd>F2</kbd></button></div>
    <div className="metric-grid">
      <article className="metric-card metric-primary"><div className="metric-top"><span>Sales today</span><span className="metric-icon"><CreditCard size={18}/></span></div><strong>{formatMoney(sales, user.currencyCode)}</strong><small>{summary?.todaySalesCount ?? 0} completed transactions</small><div className="metric-sparkline"><i/><i/><i/><i/><i/><i/><i/><i/><i/><i/><i/><i/></div></article>
      <article className="metric-card"><div className="metric-top"><span>Transactions</span><span className="metric-icon icon-blue"><ClipboardList size={18}/></span></div><strong>{summary?.todaySalesCount ?? 0}</strong><small>Sales recorded today</small><span className="metric-footnote">Across all payment types</span></article>
      <article className="metric-card"><div className="metric-top"><span>Products</span><span className="metric-icon icon-amber"><Archive size={18}/></span></div><strong>{summary?.productCount ?? 0}</strong><small>Active items in your catalog</small><span className="metric-footnote">Your current inventory</span></article>
      <article className="metric-card"><div className="metric-top"><span>Low stock</span><span className="metric-icon icon-coral"><Boxes size={18}/></span></div><strong>{summary?.lowStockCount ?? 0}</strong><small>Items at or below reorder point</small>{user.permissions.includes("inventory:write") && <button className="text-action" onClick={() => onNavigate("products")}>Review inventory <span>→</span></button>}</article>
    </div>
    {user.permissions.includes("settings:write") && <article className="panel admin-tools-panel"><div className="panel-heading"><div><p className="eyebrow">ADMINISTRATOR</p><h2>Store controls</h2><p>Set how cashiers open the register and what customers see at checkout.</p></div><button className="button button-small button-secondary" onClick={() => onNavigate("settings")}>Open settings <span>→</span></button></div><button className="quick-action" onClick={() => onNavigate("settings")}><span className="quick-action-icon quick-green"><Settings2 size={18}/></span><span><strong>Cashier workspace & customer display</strong><small>Configure register startup, full screen, text size, and the second screen.</small></span><span className="quick-arrow">→</span></button></article>}
    <div className="overview-columns"><article className="panel sales-panel"><div className="panel-heading"><div><h2>Today’s sales</h2><p>Live summary from this device</p></div>{user.permissions.includes("reports:read") && <button className="button button-small button-secondary" onClick={() => onNavigate("reports")}>Open reports <span>→</span></button>}</div><div className="empty-chart"><div className="chart-lines"><i/><i/><i/><i/></div><div className="chart-message"><span className="chart-icon"><BarChart3 size={21}/></span><strong>{sales ? "Your sales are in" : "Your sales story starts here"}</strong><span>{sales ? "Today's total is shown above. Review sales breakdowns in reports." : "Complete your first sale to see activity here."}</span></div><div className="chart-axis"><span>8 am</span><span>10 am</span><span>12 pm</span><span>2 pm</span><span>4 pm</span><span>6 pm</span></div></div></article>
      <article className="panel quick-panel"><div className="panel-heading"><div><h2>Quick actions</h2><p>Common tasks, one click away</p></div><span className="quick-star">✳</span></div><button className="quick-action" onClick={() => onNavigate("register")}><span className="quick-action-icon quick-green"><ShoppingBasket size={18}/></span><span><strong>Start a sale</strong><small>Open the register and serve a customer</small></span><span className="quick-arrow">→</span></button>{user.permissions.includes("products:write") && <button className="quick-action" onClick={() => onNavigate("products")}><span className="quick-action-icon quick-blue"><PackagePlus size={18}/></span><span><strong>Add a product</strong><small>Grow your catalog with a new item</small></span><span className="quick-arrow">→</span></button>}{user.permissions.includes("inventory:write") && <button className="quick-action" onClick={() => onNavigate("products")}><span className="quick-action-icon quick-amber"><Boxes size={18}/></span><span><strong>Adjust stock</strong><small>Keep inventory quantities accurate</small></span><span className="quick-arrow">→</span></button>}</article></div>
    <footer className="page-footer"><span>Advanced POS <b>·</b> Local-first edition</span><span>All changes are saved on this device</span></footer>
  </section>;
}
