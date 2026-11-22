// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import {
  Activity,
  ArrowDownRight,
  ArrowUpRight,
  ArrowUpRightFromSquare,
  Building2,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  Clock3,
  CreditCard,
  FileText,
  Filter,
  KeyRound,
  Columns3,
  List,
  LogOut,
  MessageSquareText,
  RefreshCw,
  Search,
  ShieldCheck,
  SlidersHorizontal,
  X,
} from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import type { FormEvent } from 'react'
import type { User } from 'oidc-client-ts'
import { createOperatorAuthSetup } from './auth'
import {
  getOperatorProfile,
  listRequestEvents,
  listPaymentRequests,
  listSupportIssues,
  listOperatorDirectory,
  createSupportIssue,
  updateSupportIssue,
  linkPaymentRequestToIssue,
  listOperatorSupportRequests,
  updateOperatorSupportRequest,
  listOperatorSupportRequestEvents,
  linkSupportRequestToIssue,
  listSupportIssueEvents,
  nextIssueStatuses,
  nextStatuses,
  OperatorApiError,
  type SupportIssue,
  type OperatorSupportRequest,
  type OperatorSupportRequestEvent,
  type SupportIssueEvent,
  type IssueCategory,
  type IssueSeverity,
  type IssueStatus,
  type PaymentIntegrationRequest,
  type QueueFilters,
  type RequestStatus,
  type RequestEvent,
  updatePaymentRequest,
} from './operatorApi'

type OperatorProfile = { id: string; role: string; displayName: string }
type Notice = { kind: 'success' | 'error' | 'info'; message: string }

const STATUS_LABELS: Record<RequestStatus, string> = {
  submitted: 'Submitted',
  under_review: 'Under review',
  planned: 'Planned',
  in_progress: 'In progress',
  released: 'Released',
  not_planned: 'Not planned',
}
const BOARD_COLUMNS: RequestStatus[] = ['submitted', 'under_review', 'planned', 'in_progress', 'released', 'not_planned']

const ISSUE_STATUS_LABELS: Record<IssueStatus, string> = {
  inbox: 'Inbox', triaged: 'Triaged', scheduled: 'Scheduled', in_progress: 'In progress',
  review: 'Review', qa: 'QA', release: 'Release', resolved: 'Resolved',
  blocked: 'Blocked', duplicate: 'Duplicate', reopened: 'Reopened',
}
const ISSUE_COLUMNS: IssueStatus[] = ['inbox', 'triaged', 'scheduled', 'in_progress', 'review', 'qa', 'release', 'blocked', 'resolved', 'reopened', 'duplicate']
const ISSUE_CATEGORY_LABELS: Record<IssueCategory, string> = {
  bug: 'Bug', feature_request: 'Feature request', payment_integration: 'Payment integration',
  account_access: 'Account access', data_issue: 'Data issue', other: 'Other',
}
const ISSUE_SEVERITY_LABELS: Record<IssueSeverity, string> = {
  critical: 'Critical', high: 'High', normal: 'Normal', low: 'Low',
}
const SUPPORT_ISSUE_CATEGORY: Record<OperatorSupportRequest['category'], IssueCategory> = {
  bug: 'bug',
  feature_request: 'feature_request',
  account_access: 'account_access',
  data_issue: 'data_issue',
  training: 'other',
  other: 'other',
}
function isRequestStatus(value: string): value is RequestStatus {
  switch (value) {
    case 'submitted':
    case 'under_review':
    case 'planned':
    case 'in_progress':
    case 'released':
    case 'not_planned':
      return true
    default:
      return false
  }
}

const IMPACT_LABELS = {
  business_blocked: 'Business blocked',
  workaround_available: 'Workaround available',
  exploring: 'Exploring options',
} as const

function displayDate(value: string): string {
  const date = new Date(value)
  return Number.isNaN(date.valueOf()) ? value : new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(date)
}

function dateTimeInputValue(value: string | null): string {
  if (!value) return ''
  const date = new Date(value)
  if (Number.isNaN(date.valueOf())) return ''
  return new Date(date.valueOf() - date.getTimezoneOffset() * 60_000).toISOString().slice(0, 16)
}

function displayIdentity(user: User): string {
  const name = user.profile.name
  if (typeof name === 'string' && name.trim()) return name.trim()
  const email = user.profile.email
  if (typeof email === 'string' && email.trim()) return email.trim()
  return 'Operator account'
}

function App() {
  const setup = useMemo(createOperatorAuthSetup, [])
  const [user, setUser] = useState<User | null>(null)
  const [profile, setProfile] = useState<OperatorProfile | null>(null)
  const [loading, setLoading] = useState(true)
  const [message, setMessage] = useState<string | null>(null)
  const didStart = useRef(false)

  useEffect(() => {
    if (didStart.current) return
    didStart.current = true
    if (setup.kind !== 'ready') {
      setLoading(false)
      return
    }
    let cancelled = false
    void (async () => {
      try {
        const isCallback = window.location.pathname === '/auth/callback'
        const session = isCallback
          ? await setup.manager.signinRedirectCallback()
          : await setup.manager.getUser()
        if (isCallback) window.history.replaceState({}, document.title, '/')
        if (cancelled) return
        if (!session || session.expired || !session.access_token) {
          setUser(null)
          setProfile(null)
          setLoading(false)
          return
        }
        const operator = await getOperatorProfile({
          apiBaseUrl: setup.apiBaseUrl,
          accessToken: session.access_token,
        })
        if (cancelled) return
        setUser(session)
        setProfile(operator)
        setLoading(false)
      } catch (reason) {
        if (cancelled) return
        setMessage(reason instanceof Error ? reason.message : 'The operator session could not be verified.')
        setLoading(false)
      }
    })()
    return () => { cancelled = true }
  }, [setup])

  async function signIn() {
    if (setup.kind !== 'ready') return
    setMessage(null)
    try {
      await setup.manager.signinRedirect()
    } catch {
      setMessage('Sign-in could not be started. Check the operator identity provider configuration.')
    }
  }

  async function signOut() {
    if (setup.kind !== 'ready') return
    try {
      await setup.manager.signoutRedirect()
    } catch {
      await setup.manager.removeUser()
      setUser(null)
      setProfile(null)
      setMessage('Your local operator session has ended. The identity provider sign-out could not be completed.')
    }
  }

  if (loading) return <LoadingScreen />
  if (setup.kind === 'missing') return <SignInScreen setupMessage={`Configure ${setup.names.join(', ')} for the separate operator client.`} onSignIn={signIn} disabled />
  if (setup.kind === 'invalid') return <SignInScreen setupMessage={setup.message} onSignIn={signIn} disabled />
  if (!user || !profile) return <SignInScreen setupMessage={message} onSignIn={signIn} disabled={false} />

  return <QueueWorkspace
    apiBaseUrl={setup.apiBaseUrl}
    user={user}
    profile={profile}
    onSignOut={signOut}
    onUnauthorized={async () => {
      await setup.manager.removeUser()
      setUser(null)
      setProfile(null)
      setMessage('Your operator session expired. Sign in again to continue.')
    }}
  />
}

function LoadingScreen() {
  return <main className="auth-screen"><div className="auth-card"><span className="brand-mark">P</span><span className="spinner"/><p>Checking operator access…</p></div></main>
}

function SignInScreen({ setupMessage, onSignIn, disabled }: { setupMessage: string | null; onSignIn: () => void; disabled: boolean }) {
  return <main className="auth-screen">
    <section className="auth-aside">
      <a className="brand-lockup" href="/" aria-label="Powered Solutions operator desk home">
        <span className="brand-mark">P</span><span><strong>Powered Solutions</strong><small>OPERATOR DESK</small></span>
      </a>
      <div className="auth-illustration" aria-hidden="true"><div className="orbit orbit-one"/><div className="orbit orbit-two"/><div className="orbit-core"><CreditCard size={35}/></div><span className="orbit-node node-one"/><span className="orbit-node node-two"/><span className="orbit-node node-three"/></div>
      <div className="auth-aside-copy"><span className="eyebrow">SUPPORT OPERATIONS</span><h1>Keep every<br/>request moving.</h1><p>A focused workspace for triaging payment and terminal integration requests from Advanced POS businesses.</p></div>
      <div className="auth-aside-foot"><span><ShieldCheck size={15}/> Separate operator access</span><span>INTERNAL WORKSPACE</span></div>
    </section>
    <section className="auth-main">
      <div className="auth-card">
        <div className="auth-icon"><KeyRound size={20}/></div>
        <span className="eyebrow">OPERATOR SIGN IN</span>
        <h2>Sign in to your support desk</h2>
        <p className="muted">Use your approved operator account. Business staff accounts do not grant access to this workspace.</p>
        {setupMessage && <div className="notice notice-info" role="status"><CircleAlert size={17}/><span>{setupMessage}</span></div>}
        <button className="button button-primary sign-in-button" type="button" disabled={disabled} onClick={onSignIn}>
          <KeyRound size={17}/><span>Continue with secure sign-in</span><ArrowUpRightFromSquare size={16}/>
        </button>
        <div className="auth-security"><ShieldCheck size={17}/><span>Sign-in uses a dedicated OpenID Connect client and a short-lived access token.</span></div>
        <div className="auth-rule"><span/><small>ACCESS IS VERIFIED SERVER-SIDE</small><span/></div>
        <p className="auth-help">Need access? Contact your Powered Solutions workspace administrator.</p>
      </div>
      <footer className="auth-footer"><span>Advanced POS · Powered Solutions</span><span>Internal support tools</span></footer>
    </section>
  </main>
}

function QueueWorkspace({
  apiBaseUrl,
  user,
  profile,
  onSignOut,
  onUnauthorized,
}: {
  apiBaseUrl: string
  user: User
  profile: OperatorProfile
  onSignOut: () => Promise<void>
  onUnauthorized: () => Promise<void>
}) {
  const operatorName = displayIdentity(user)
  const [filters, setFilters] = useState<QueueFilters>({ status: '', country: '', provider: '' })
  const [draftFilters, setDraftFilters] = useState({ status: '', country: '', provider: '' })
  const [cursorStack, setCursorStack] = useState<(string | null)[]>([null])
  const [nextCursor, setNextCursor] = useState<string | null>(null)
  const [requests, setRequests] = useState<PaymentIntegrationRequest[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [movingId, setMovingId] = useState<string | null>(null)
  const [view, setView] = useState<'list' | 'board'>('board')
  const [activeSection, setActiveSection] = useState<'requests' | 'issues'>('requests')
  const [notice, setNotice] = useState<Notice | null>(null)
  const [refreshIndex, setRefreshIndex] = useState(0)
  const [mobileDetailOpen, setMobileDetailOpen] = useState(false)
  const [logoutOpen, setLogoutOpen] = useState(false)
  const logoutDialog = useRef<HTMLDialogElement>(null)
  const accessToken = user.access_token
  const currentCursor = cursorStack[cursorStack.length - 1]

  useEffect(() => {
    const dialog = logoutDialog.current
    if (logoutOpen && dialog && !dialog.open) dialog.showModal()
    if (!logoutOpen && dialog?.open) dialog.close()
  }, [logoutOpen])

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    void listPaymentRequests({ apiBaseUrl, accessToken, filters: { ...filters, cursor: currentCursor } })
      .then((page) => {
        if (cancelled) return
        setRequests(page.items)
        setNextCursor(page.nextCursor)
        setSelectedId((current) => page.items.some((item) => item.id === current) ? current : (page.items[0]?.id ?? null))
        setLoading(false)
        setNotice(null)
      })
      .catch((reason: unknown) => {
        if (cancelled) return
        if (reason instanceof OperatorApiError && reason.status === 401) void onUnauthorized()
        setRequests([])
        setNextCursor(null)
        setLoading(false)
        setNotice({ kind: 'error', message: reason instanceof Error ? reason.message : 'Requests could not be loaded.' })
      })
    return () => { cancelled = true }
  }, [accessToken, apiBaseUrl, currentCursor, filters, onUnauthorized, refreshIndex])

  const selected = requests.find((item) => item.id === selectedId) ?? null
  const currentPage = cursorStack.length
  const needsReview = requests.filter((item) => item.status === 'submitted' || item.status === 'under_review').length

  function applyFilters(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const country = draftFilters.country.trim().toUpperCase()
    if (country && !/^[A-Z]{2}$/.test(country)) {
      setNotice({ kind: 'error', message: 'Enter a two-letter country code, such as GH or GB.' })
      return
    }
    setCursorStack([null])
    setFilters({ ...draftFilters, country })
    setNotice(null)
  }

  function clearFilters() {
    const empty = { status: '', country: '', provider: '' }
    setDraftFilters(empty)
    setFilters(empty)
    setCursorStack([null])
  }

  async function saveUpdate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selected) return
    const form = new FormData(event.currentTarget)
    const status = String(form.get('status')) as RequestStatus
    const customerVisibleUpdate = String(form.get('customerVisibleUpdate') ?? '')
    const privateNote = String(form.get('privateNote') ?? '')
    const dueAtInput = String(form.get('dueAt') ?? '')
    if (status !== selected.status && !nextStatuses(selected.status).includes(status)) {
      setNotice({ kind: 'error', message: 'Choose an allowed next status for this request.' })
      return
    }
    setSaving(true)
    setNotice(null)
    try {
      await updatePaymentRequest({
        apiBaseUrl,
        accessToken,
        requestId: selected.id,
        expectedRevision: selected.revision,
        status,
        customerVisibleUpdate,
        privateNote,
        dueAt: dueAtInput ? new Date(dueAtInput).toISOString() : null,
      })
      setNotice({ kind: 'success', message: 'Request changes saved. The business can see its status and public progress; the due date is internal.' })
      setRefreshIndex((current) => current + 1)
    } catch (reason) {
      if (reason instanceof OperatorApiError && reason.status === 401) void onUnauthorized()
      if (reason instanceof OperatorApiError && reason.status === 409) {
        setNotice({ kind: 'error', message: 'This request changed since it was opened. Refresh the queue before making another update.' })
      } else {
        setNotice({ kind: 'error', message: reason instanceof Error ? reason.message : 'The request could not be updated.' })
      }
    } finally {
      setSaving(false)
    }
  }

  async function moveRequest(item: PaymentIntegrationRequest, status: RequestStatus) {
    if (movingId || !nextStatuses(item.status).includes(status)) return
    setMovingId(item.id)
    setNotice(null)
    try {
      await updatePaymentRequest({
        apiBaseUrl,
        accessToken,
        requestId: item.id,
        expectedRevision: item.revision,
        status,
        customerVisibleUpdate: '',
        privateNote: '',
      })
      setNotice({ kind: 'success', message: `Request moved to ${STATUS_LABELS[status]}. The business can see the updated status.` })
      setRefreshIndex((current) => current + 1)
    } catch (reason) {
      if (reason instanceof OperatorApiError && reason.status === 401) void onUnauthorized()
      setNotice({
        kind: 'error',
        message: reason instanceof OperatorApiError && reason.status === 409
          ? 'This request changed in another session. Refresh the queue before moving it.'
          : reason instanceof Error ? reason.message : 'The request could not be moved.',
      })
    } finally {
      setMovingId(null)
    }
  }

  function openRequest(item: PaymentIntegrationRequest) {
    setSelectedId(item.id)
    setMobileDetailOpen(true)
  }

  return <div className="operator-shell">
    <aside className="sidebar" aria-label="Operator navigation">
      <a className="sidebar-brand" href="#queue" onClick={(event) => event.preventDefault()}>
        <span className="brand-mark">P</span><span><strong>Powered Solutions</strong><small>OPERATOR DESK</small></span>
      </a>
      <div className="sidebar-label">OPERATIONS</div>
      <button className={`nav-item ${activeSection === 'requests' ? 'active' : ''}`} type="button" aria-current={activeSection === 'requests' ? 'page' : undefined} onClick={() => setActiveSection('requests')}><CreditCard size={17}/><span>Payment requests</span><span className="nav-count">{needsReview}</span></button>
      <button className={`nav-item ${activeSection === 'issues' ? 'active' : ''}`} type="button" aria-current={activeSection === 'issues' ? 'page' : undefined} onClick={() => setActiveSection('issues')}><Columns3 size={17}/><span>Support issues</span></button>
      <div className="sidebar-label sidebar-label-lower">WORKSPACE</div>
      <div className="sidebar-info"><span className="sidebar-info-icon"><Activity size={16}/></span><span><strong>Support queue</strong><small>Tenant requests and progress</small></span></div>
      <div className="sidebar-spacer"/>
      <div className="security-card"><ShieldCheck size={17}/><span><strong>Protected workspace</strong><small>Separate operator identity</small></span></div>
      <button className="sidebar-account" type="button" onClick={() => setLogoutOpen(true)}>
        <span className="avatar">{operatorName.slice(0, 1).toUpperCase()}</span><span className="account-copy"><strong>{operatorName}</strong><small>{profile.role} operator</small></span><ChevronDown size={15}/>
      </button>
    </aside>

    <main className="main-area">
      <header className="topbar">
        <div className="crumb"><span>Operations</span><ChevronRight size={14}/><strong>{activeSection === 'requests' ? 'Payment requests' : 'Support issues'}</strong></div>
        <div className="topbar-actions"><span className="live-badge"><i/> Service connected</span><span className="topbar-divider"/><span className="topbar-name">{operatorName}</span><button className="icon-button" type="button" aria-label="Sign out" title="Sign out" onClick={() => setLogoutOpen(true)}><LogOut size={17}/></button></div>
      </header>
      <div className="page-wrap">
        {activeSection === 'requests' ? <>
        <section className="page-heading">
          <div><span className="eyebrow">CUSTOMER SUPPORT</span><h1>Payment integration requests</h1><p>Review tenant requests for bank terminals, payment processors and checkout capabilities.</p></div>
          <button className="button button-quiet refresh-button" type="button" onClick={() => setRefreshIndex((current) => current + 1)} disabled={loading}><RefreshCw size={16} className={loading ? 'spin' : ''}/>Refresh queue</button>
        </section>

        <section className="summary-grid" aria-label="Queue summary">
          <article className="summary-card"><span className="summary-icon icon-amber"><FileText size={17}/></span><div><span>Requests on this page</span><strong>{loading ? '—' : requests.length}</strong></div><span className="summary-tail">Latest first</span></article>
          <article className="summary-card"><span className="summary-icon icon-blue"><Clock3 size={17}/></span><div><span>Needs attention</span><strong>{loading ? '—' : needsReview}</strong></div><span className="summary-tail">Submitted or under review</span></article>
          <article className="summary-card"><span className="summary-icon icon-green"><Check size={17}/></span><div><span>Operator access</span><strong>Active</strong></div><span className="summary-tail">{profile.role} role</span></article>
        </section>

        <form className="filter-bar" onSubmit={applyFilters}>
          <div className="filter-title"><span className="filter-icon"><SlidersHorizontal size={16}/></span><span><strong>Filter requests</strong><small>Search the support queue</small></span></div>
          <label className="filter-control"><span>Status</span><select value={draftFilters.status} onChange={(event) => setDraftFilters((old) => ({ ...old, status: event.target.value }))}><option value="">All statuses</option>{Object.entries(STATUS_LABELS).map(([value, label]) => <option value={value} key={value}>{label}</option>)}</select></label>
          <label className="filter-control country-filter"><span>Country</span><input value={draftFilters.country} maxLength={2} autoCapitalize="characters" placeholder="e.g. GH" onChange={(event) => setDraftFilters((old) => ({ ...old, country: event.target.value }))}/></label>
          <label className="filter-control provider-filter"><span>Provider</span><div className="filter-search"><Search size={15}/><input value={draftFilters.provider} maxLength={120} placeholder="Search provider" onChange={(event) => setDraftFilters((old) => ({ ...old, provider: event.target.value }))}/></div></label>
          <button className="button button-primary filter-apply" type="submit"><Filter size={15}/>Apply filters</button>
          {(filters.status || filters.country || filters.provider) && <button className="clear-filter" type="button" onClick={clearFilters}>Clear</button>}
        </form>

        {notice && <div className={`notice notice-${notice.kind}`} role={notice.kind === 'error' ? 'alert' : 'status'}><span className="notice-mark">{notice.kind === 'error' ? <CircleAlert size={17}/> : <Check size={17}/>}</span><span>{notice.message}</span><button type="button" aria-label="Dismiss notification" onClick={() => setNotice(null)}><X size={16}/></button></div>}

        <div className={`queue-layout ${view === 'board' ? 'board-layout' : ''} ${mobileDetailOpen ? 'show-detail' : ''}`}>
          <section className="queue-panel" aria-label="Payment integration request list">
            <div className="panel-heading"><div><span className="eyebrow">{view === 'board' ? 'WORKFLOW' : 'INBOX'}</span><h2>{view === 'board' ? 'Request board' : 'Request queue'} <span>{loading ? '' : requests.length}</span></h2></div><div className="panel-heading-actions"><div className="sort-label"><ArrowDownRight size={14}/> Newest first</div><div className="queue-view-toggle" role="group" aria-label="Request display"><button type="button" aria-pressed={view === 'list'} onClick={() => { setView('list'); setMobileDetailOpen(false) }}><List size={14}/>List</button><button type="button" aria-pressed={view === 'board'} onClick={() => { setView('board'); setMobileDetailOpen(false) }}><Columns3 size={14}/>Board</button></div></div></div>
            {view === 'list' ? <>
              <div className="queue-table-head"><span>PROVIDER / REQUEST</span><span>BUSINESS</span><span>STATUS</span><span>RECEIVED</span><span/></div>
              <div className="queue-list" aria-live="polite" aria-busy={loading}>
                {loading ? <div className="list-state"><span className="spinner"/><strong>Loading requests</strong><small>Checking the operator queue…</small></div> : requests.length === 0 ? <div className="list-state"><span className="list-state-icon"><Search size={20}/></span><strong>No matching requests</strong><small>Change the filters or clear them to see the full queue.</small><button className="button button-quiet" type="button" onClick={clearFilters}>Clear filters</button></div> : requests.map((item) => <RequestRow key={item.id} item={item} active={item.id === selected?.id} onClick={() => openRequest(item)}/>) }
              </div>
            </> : <div className="kanban-board" aria-label="Payment integration requests by workflow status" aria-busy={loading}>
              {loading ? <div className="list-state"><span className="spinner"/><strong>Loading requests</strong><small>Checking the operator queue…</small></div> : BOARD_COLUMNS.map((status) => {
                const columnItems = requests.filter((item) => item.status === status)
                return <section className="kanban-column" key={status} aria-labelledby={`kanban-${status}-title`}>
                  <header className="kanban-column-heading"><h3 id={`kanban-${status}-title`}>{STATUS_LABELS[status]}</h3><span>{columnItems.length}</span></header>
                  {columnItems.length === 0 ? <p className="kanban-empty">No requests in this stage.</p> : <div className="kanban-cards">
                    {columnItems.map((item) => <article className={`kanban-card ${item.id === selected?.id ? 'selected' : ''}`} key={item.id}>
                      <button className="kanban-card-open" type="button" aria-pressed={item.id === selected?.id} aria-label={`Open ${item.providerName} request details`} onClick={() => openRequest(item)}>
                        <span className="kanban-card-top"><strong>{item.providerName}</strong><span>{item.countryCode}</span></span>
                        <span className="kanban-card-capability">{item.capability}{item.terminalModel ? ` · ${item.terminalModel}` : ''}</span>
                        <span className={`kanban-impact impact-${item.businessImpact}`}>{IMPACT_LABELS[item.businessImpact]}</span>
                        <span className="kanban-card-date">Received {displayDate(item.createdAt)}</span>
                      </button>
                      {nextStatuses(item.status).length > 0 ? <label className="kanban-move"><span>{movingId === item.id ? 'Saving stage…' : 'Move to'}</span><select aria-label={`Move ${item.providerName} request to next stage`} value="" disabled={movingId !== null} onChange={(event) => { const nextStatus = event.target.value as RequestStatus; if (nextStatus) void moveRequest(item, nextStatus) }}><option value="">Choose next stage</option>{nextStatuses(item.status).map((nextStatus) => <option value={nextStatus} key={nextStatus}>{STATUS_LABELS[nextStatus]}</option>)}</select></label> : <p className="kanban-closed">No further stage is available.</p>}
                    </article>)}
                  </div>}
                </section>
              })}
              {!loading && requests.length === 0 && <div className="kanban-empty-state"><strong>No matching requests</strong><span>Change the filters or clear them to see the full queue.</span><button className="button button-quiet" type="button" onClick={clearFilters}>Clear filters</button></div>}
            </div>}
            <div className="queue-footer"><span>Page {currentPage} · maximum 50 per page</span><div className="pagination"><button type="button" className="icon-button" aria-label="Previous page" disabled={cursorStack.length <= 1 || loading} onClick={() => { setCursorStack((old) => old.slice(0, -1)); setMobileDetailOpen(false) }}><ChevronLeft size={17}/></button><span>{currentPage}</span><button type="button" className="icon-button" aria-label="Next page" disabled={!nextCursor || loading} onClick={() => { if (nextCursor) setCursorStack((old) => [...old, nextCursor]); setMobileDetailOpen(false) }}><ChevronRight size={17}/></button></div></div>
          </section>

          <section className="detail-panel" aria-label="Selected request details">
            <button className="mobile-back" type="button" onClick={() => setMobileDetailOpen(false)}><ChevronLeft size={16}/> Back to requests</button>
            {!selected ? <div className="detail-empty"><span><MessageSquareText size={22}/></span><strong>Select a request</strong><p>Choose an item in the queue to view its details and update its status.</p></div> : <RequestDetails key={`${selected.id}:${selected.revision}`} item={selected} saving={saving || movingId === selected.id} onSave={saveUpdate} apiBaseUrl={apiBaseUrl} accessToken={accessToken} onUnauthorized={onUnauthorized}/>}
          </section>
        </div>
        <footer className="page-footer"><span><ShieldCheck size={14}/> Internal support actions are audited</span><span>Advanced POS · Powered Solutions</span></footer>
        </> : <SupportIssueWorkspace apiBaseUrl={apiBaseUrl} accessToken={accessToken} operatorId={profile.id} onUnauthorized={onUnauthorized}/>}
      </div>
    </main>

    <dialog className="logout-dialog" ref={logoutDialog} onClose={() => setLogoutOpen(false)}>
      <div className="dialog-icon"><LogOut size={19}/></div><h2>Sign out of operator desk?</h2><p>Your operator session will end on this device.</p>
      <div className="dialog-actions"><button className="button button-quiet" type="button" onClick={() => setLogoutOpen(false)}>Stay signed in</button><button className="button button-primary" type="button" onClick={() => void onSignOut()}>Sign out</button></div>
    </dialog>
  </div>
}

function SupportIssueWorkspace({ apiBaseUrl, accessToken, operatorId, onUnauthorized }: {
  apiBaseUrl: string
  accessToken: string
  operatorId: string
  onUnauthorized: () => Promise<void>
}) {
  const [issues, setIssues] = useState<SupportIssue[]>([])
  const [operators, setOperators] = useState<{ id: string; displayName: string }[]>([])
  const [requests, setRequests] = useState<PaymentIntegrationRequest[]>([])
  const [events, setEvents] = useState<SupportIssueEvent[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [refreshKey, setRefreshKey] = useState(0)
  const [cursorStack, setCursorStack] = useState<(string | null)[]>([null])
  const [nextCursor, setNextCursor] = useState<string | null>(null)
  const [notice, setNotice] = useState<Notice | null>(null)
  const [categoryFilter, setCategoryFilter] = useState<IssueCategory | ''>('')
  const selected = issues.find((issue) => issue.id === selectedId) ?? null
  const currentCursor = cursorStack[cursorStack.length - 1]

  useEffect(() => {
    let active = true
    setLoading(true)
    void Promise.all([
      listSupportIssues({ apiBaseUrl, accessToken, category: categoryFilter, cursor: currentCursor }),
      listPaymentRequests({ apiBaseUrl, accessToken, filters: { status: '', country: '', provider: '' } }),
      listOperatorDirectory({ apiBaseUrl, accessToken }),
    ]).then(([page, requestPage, operatorPage]) => {
      if (!active) return
      setIssues(page.items)
      setNextCursor(page.nextCursor)
      setRequests(requestPage.items)
      setOperators(operatorPage.items)
      setSelectedId((current) => page.items.some((issue) => issue.id === current) ? current : page.items[0]?.id ?? null)
      setLoading(false)
    }).catch((reason: unknown) => {
      if (!active) return
      if (reason instanceof OperatorApiError && reason.status === 401) void onUnauthorized()
      setIssues([])
      setNextCursor(null)
      setRequests([])
      setOperators([])
      setNotice({ kind: 'error', message: reason instanceof Error ? reason.message : 'Support issues could not be loaded.' })
      setLoading(false)
    })
    return () => { active = false }
  }, [accessToken, apiBaseUrl, categoryFilter, currentCursor, onUnauthorized, refreshKey])

  useEffect(() => {
    if (!selected) { setEvents([]); return }
    let active = true
    void listSupportIssueEvents({ apiBaseUrl, accessToken, issueId: selected.id }).then((page) => {
      if (active) setEvents(page.items)
    }).catch((reason: unknown) => {
      if (active) setNotice({ kind: 'error', message: reason instanceof Error ? reason.message : 'Issue history could not be loaded.' })
    })
    return () => { active = false }
  }, [accessToken, apiBaseUrl, selected?.id, selected?.revision])

  async function createIssue(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const formElement = event.currentTarget
    const form = new FormData(formElement)
    const title = String(form.get('title') ?? '').trim()
    const category = String(form.get('category')) as IssueCategory
    const severity = String(form.get('severity')) as IssueSeverity
    const paymentRequestId = String(form.get('paymentRequestId') ?? '')
    if (title.length < 3) { setNotice({ kind: 'error', message: 'Enter an issue title with at least three characters.' }); return }
    setBusy(true)
    setNotice(null)
    try {
      const issue = await createSupportIssue({
        apiBaseUrl,
        accessToken,
        title,
        category,
        severity,
        ...(paymentRequestId ? { source: { kind: 'payment_request', id: paymentRequestId } } : {}),
      })
      setSelectedId(issue.id)
      setNotice({ kind: 'success', message: 'Support issue created. Its linked tenant request keeps its own public progress.' })
      setRefreshKey((value) => value + 1)
      formElement.reset()
    } catch (reason) {
      if (reason instanceof OperatorApiError && reason.status === 401) void onUnauthorized()
      setNotice({ kind: 'error', message: reason instanceof Error ? reason.message : 'The support issue could not be created.' })
    } finally { setBusy(false) }
  }

  async function saveIssue(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selected) return
    const form = new FormData(event.currentTarget)
    const assignment = String(form.get('assignment') ?? '')
    const dueValue = String(form.get('dueAt') ?? '')
    const targetRelease = String(form.get('targetRelease') ?? '').trim()
    setBusy(true)
    setNotice(null)
    try {
      await updateSupportIssue({
        apiBaseUrl, accessToken, issueId: selected.id, expectedRevision: selected.revision,
        status: String(form.get('status')) as IssueStatus,
        title: String(form.get('title') ?? '').trim(),
        category: String(form.get('category')) as IssueCategory,
        severity: String(form.get('severity')) as IssueSeverity,
        ...(assignment === 'claim' || assignment === 'unassign' || assignment === 'operator:none' ? { assignment: assignment === 'operator:none' ? 'unassign' : assignment } : {}),
        ...(assignment.startsWith('operator:') ? { assigneeOperatorId: assignment.slice('operator:'.length) } : {}),
        dueAt: dueValue ? new Date(dueValue).toISOString() : null,
        targetRelease: targetRelease || null,
        privateNote: String(form.get('privateNote') ?? '').trim(),
      })
      setNotice({ kind: 'success', message: 'Issue updated. Internal notes and scheduling remain private to the operator workspace.' })
      setRefreshKey((value) => value + 1)
    } catch (reason) {
      if (reason instanceof OperatorApiError && reason.status === 401) void onUnauthorized()
      setNotice({ kind: 'error', message: reason instanceof OperatorApiError && reason.status === 409
        ? 'This issue changed in another session. Refresh before saving again.'
        : reason instanceof Error ? reason.message : 'The issue could not be updated.' })
    } finally { setBusy(false) }
  }

  async function linkDuplicate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selected) return
    const requestId = String(new FormData(event.currentTarget).get('requestId') ?? '')
    if (!requestId) { setNotice({ kind: 'error', message: 'Choose a payment request to link.' }); return }
    setBusy(true)
    setNotice(null)
    try {
      await linkPaymentRequestToIssue({ apiBaseUrl, accessToken, issueId: selected.id, requestId, expectedRevision: selected.revision, relationship: 'duplicate' })
      setNotice({ kind: 'success', message: 'Request linked as a duplicate. Its customer-visible progress remains separate.' })
      setRefreshKey((value) => value + 1)
    } catch (reason) {
      if (reason instanceof OperatorApiError && reason.status === 401) void onUnauthorized()
      setNotice({ kind: 'error', message: reason instanceof OperatorApiError && reason.status === 409
        ? 'The issue or request changed, or that request is already linked. Refresh and try again.'
        : reason instanceof Error ? reason.message : 'The request could not be linked.' })
    } finally { setBusy(false) }
  }

  const knownLinkedIds = new Set(events.flatMap((entry) => entry.eventKind === 'payment_request_linked' && entry.linkedRequestId ? [entry.linkedRequestId] : []))
  const availableRequests = requests.filter((request) => !knownLinkedIds.has(request.id))
  const linkedRequests = requests.filter((request) => knownLinkedIds.has(request.id))
  const eventText = (entry: SupportIssueEvent) => {
    if (entry.eventKind === 'private_note') return entry.privateNote ? `Private note: ${entry.privateNote}` : 'Private note added'
    if (entry.eventKind === 'payment_request_linked') return `Linked payment request ${entry.linkedRequestId ?? ''}`
    if (entry.eventKind === 'support_request_linked') return `Linked tenant support request ${entry.linkedSupportRequestId ?? ''}`
    if (entry.eventKind === 'status_changed') return `Moved from ${entry.previousStatus ? ISSUE_STATUS_LABELS[entry.previousStatus] : 'new'} to ${entry.newStatus ? ISSUE_STATUS_LABELS[entry.newStatus] : 'unknown'}`
    if (entry.eventKind === 'assignment_changed') return entry.newAssigneeOperatorId === operatorId ? 'Assigned to you' : entry.newAssigneeOperatorId ? `Assigned to ${operators.find((item) => item.id === entry.newAssigneeOperatorId)?.displayName ?? 'inactive operator'}` : 'Unassigned'
    if (entry.eventKind === 'schedule_changed') return `Schedule changed${entry.dueAt ? ` · due ${displayDate(entry.dueAt)}` : ' · due date cleared'}${entry.targetRelease ? ` · ${entry.targetRelease}` : ''}`
    return 'Issue created or details updated'
  }

  return <>
    <section className="page-heading">
      <div><span className="eyebrow">INTERNAL WORKFLOW</span><h1>Support issues</h1><p>Group related tenant reports, assign an owner, and track engineering work without sharing private issue details with tenants.</p></div>
      <button className="button button-quiet refresh-button" type="button" onClick={() => setRefreshKey((value) => value + 1)} disabled={loading}><RefreshCw size={16} className={loading ? 'spin' : ''}/>Refresh issues</button>
    </section>

    {notice && <div className={`notice notice-${notice.kind}`} role={notice.kind === 'error' ? 'alert' : 'status'}><span className="notice-mark">{notice.kind === 'error' ? <CircleAlert size={17}/> : <Check size={17}/>}</span><span>{notice.message}</span><button type="button" aria-label="Dismiss notification" onClick={() => setNotice(null)}><X size={16}/></button></div>}

    <section className="support-issue-create panel-card" aria-labelledby="issue-create-title">
      <div className="panel-heading"><div><span className="eyebrow">TRACK A WORK ITEM</span><h2 id="issue-create-title">New support issue</h2></div></div>
      <form className="support-issue-create-form" onSubmit={(event) => void createIssue(event)}>
        <label><span>Title</span><input name="title" maxLength={200} required minLength={3} placeholder="Describe the issue or feature request"/></label>
        <label><span>Category</span><select name="category" defaultValue="payment_integration">{Object.entries(ISSUE_CATEGORY_LABELS).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
        <label><span>Severity</span><select name="severity" defaultValue="normal">{Object.entries(ISSUE_SEVERITY_LABELS).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
        <label><span>Primary payment request (optional)</span><select name="paymentRequestId" defaultValue=""><option value="">No request linked</option>{requests.map((request) => <option key={request.id} value={request.id}>{request.providerName} · {request.countryCode} · {request.id.slice(0, 8)}</option>)}</select></label>
        <button className="button button-primary" type="submit" disabled={busy}>{busy ? 'Creating…' : 'Create issue'}</button>
      </form>
    </section>

    <div className="support-issue-toolbar"><label className="filter-control"><span>Category</span><select value={categoryFilter} onChange={(event) => { setCursorStack([null]); setCategoryFilter(event.target.value as IssueCategory | '') }}><option value="">All categories</option>{Object.entries(ISSUE_CATEGORY_LABELS).map(([value, label]) => <option value={value} key={value}>{label}</option>)}</select></label><span className="summary-tail">{loading ? 'Loading…' : `${issues.length} issues · page ${cursorStack.length}`}</span><div className="pagination"><button type="button" className="icon-button" aria-label="Previous issue page" disabled={cursorStack.length <= 1 || loading} onClick={() => setCursorStack((old) => old.slice(0, -1))}><ChevronLeft size={17}/></button><span>{cursorStack.length}</span><button type="button" className="icon-button" aria-label="Next issue page" disabled={!nextCursor || loading} onClick={() => { if (nextCursor) setCursorStack((old) => [...old, nextCursor]) }}><ChevronRight size={17}/></button></div></div>

    <div className="support-issue-layout">
      <section className="support-issue-board" aria-label="Support issues by workflow stage" aria-busy={loading}>
        {loading ? <div className="list-state"><span className="spinner"/><strong>Loading issues</strong><small>Checking the internal work queue…</small></div> : ISSUE_COLUMNS.map((status) => {
          const columnItems = issues.filter((issue) => issue.status === status)
          return <section className="support-issue-column" key={status} aria-label={ISSUE_STATUS_LABELS[status]}>
            <header><h2>{ISSUE_STATUS_LABELS[status]}</h2><span>{columnItems.length}</span></header>
            {columnItems.length === 0 ? <p className="kanban-empty">No issues here.</p> : columnItems.map((issue) => <button type="button" key={issue.id} className={`support-issue-card ${selected?.id === issue.id ? 'selected' : ''}`} aria-pressed={selected?.id === issue.id} onClick={() => setSelectedId(issue.id)}>
              <span className="support-issue-card-top"><strong>{issue.title}</strong><span className={`severity severity-${issue.severity}`}>{ISSUE_SEVERITY_LABELS[issue.severity]}</span></span>
              <span className="support-issue-meta">{ISSUE_CATEGORY_LABELS[issue.category]} · {issue.linkedRequestCount} linked</span>
              <span className="support-issue-meta">{issue.assigneeOperatorId === operatorId ? 'Assigned to you' : issue.assigneeOperatorId ? `Assigned to ${issue.assigneeOperatorName ?? 'inactive operator'}` : 'Unassigned'}</span>
              {issue.dueAt && <span className="support-issue-due">Due {displayDate(issue.dueAt)}</span>}
            </button>)}
          </section>
        })}
      </section>

      <section className="support-issue-detail panel-card" aria-label="Selected support issue details">
        {!selected ? <div className="detail-empty"><span><MessageSquareText size={22}/></span><strong>Select an issue</strong><p>Choose a work item to update its stage, schedule, assignment, linked reports, and private history.</p></div> : <>
          <div className="detail-header"><div><span className="eyebrow">INTERNAL ISSUE</span><span className="request-ref">ISS-{selected.id.slice(0, 8).toUpperCase()}</span></div><span className={`status-pill issue-status-${selected.status}`}><i/>{ISSUE_STATUS_LABELS[selected.status]}</span></div>
          <form className="support-issue-edit" key={`${selected.id}:${selected.revision}`} onSubmit={(event) => void saveIssue(event)}>
            <label><span>Title</span><input name="title" maxLength={200} required minLength={3} defaultValue={selected.title}/></label>
            <div className="support-issue-fields">
              <label><span>Stage</span><select name="status" defaultValue={selected.status}>{[selected.status, ...nextIssueStatuses(selected.status)].map((status) => <option key={status} value={status}>{ISSUE_STATUS_LABELS[status]}</option>)}</select></label>
              <label><span>Severity</span><select name="severity" defaultValue={selected.severity}>{Object.entries(ISSUE_SEVERITY_LABELS).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
              <label><span>Category</span><select name="category" defaultValue={selected.category}>{Object.entries(ISSUE_CATEGORY_LABELS).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
              <label><span>Assignment</span><select name="assignment" defaultValue=""><option value="">No change</option><option value="operator:none">Unassigned</option>{operators.map((person) => <option key={person.id} value={`operator:${person.id}`}>{person.displayName}{person.id === operatorId ? ' (you)' : ''}</option>)}{selected.assigneeOperatorId && !operators.some((person) => person.id === selected.assigneeOperatorId) && <option value="">Inactive operator (keep current)</option>}</select></label>
              <label><span>Internal due date</span><input name="dueAt" type="datetime-local" defaultValue={dateTimeInputValue(selected.dueAt)}/></label>
              <label><span>Target release</span><input name="targetRelease" maxLength={80} defaultValue={selected.targetRelease ?? ''} placeholder="e.g. 1.2.0"/></label>
            </div>
            <label><span>Private note</span><textarea name="privateNote" maxLength={2000} rows={3} placeholder="Visible only to authorized support operators"/></label>
            <button className="button button-primary" type="submit" disabled={busy}>{busy ? 'Saving…' : 'Save issue'}</button>
          </form>

          {knownLinkedIds.size > 0 && <div className="support-issue-linked-reports"><span className="eyebrow">LINKED REPORTS</span>{[...knownLinkedIds].map((requestId) => {
            const request = linkedRequests.find((candidate) => candidate.id === requestId)
            return <div key={requestId}><strong>{request ? `${request.providerName} · ${request.countryCode}` : `Request ${requestId.slice(0, 8)}`}</strong><small>{request ? `${request.capability} · ${request.businessImpact.replaceAll('_', ' ')} · ${request.status.replaceAll('_', ' ')}` : requestId}</small></div>
          })}</div>}

          <form className="support-issue-link-form" onSubmit={(event) => void linkDuplicate(event)}>
            <label><span>Link a duplicate request</span><select name="requestId" defaultValue=""><option value="">Choose an unlinked request</option>{availableRequests.map((request) => <option key={request.id} value={request.id}>{request.providerName} · {request.countryCode} · {request.id.slice(0, 8)}</option>)}</select></label>
            <button className="button button-quiet" type="submit" disabled={busy || availableRequests.length === 0}>Link duplicate</button>
          </form>
          <div className="support-issue-history"><div className="panel-heading"><div><span className="eyebrow">PRIVATE HISTORY</span><h2>Activity</h2></div></div>{events.length === 0 ? <p className="kanban-empty">No events recorded.</p> : <ol>{events.map((entry) => <li key={entry.id}><span>{eventText(entry)}</span><time>{displayDate(entry.createdAt)}</time></li>)}</ol>}</div>
        </>}
      </section>
    </div>
    <OperatorSupportRequestsPanel
      apiBaseUrl={apiBaseUrl}
      accessToken={accessToken}
      selectedIssue={selected}
      refreshKey={refreshKey}
      onUnauthorized={onUnauthorized}
      onIssueCreated={(issue) => { setSelectedId(issue.id); setRefreshKey((value) => value + 1) }}
    />
    <footer className="page-footer"><span><ShieldCheck size={14}/> Internal support actions are audited</span><span>Advanced POS · Powered Solutions</span></footer>
  </>
}

function OperatorSupportRequestsPanel({ apiBaseUrl, accessToken, selectedIssue, refreshKey, onUnauthorized, onIssueCreated }: {
  apiBaseUrl: string
  accessToken: string
  selectedIssue: SupportIssue | null
  refreshKey: number
  onUnauthorized: () => Promise<void>
  onIssueCreated: (issue: SupportIssue) => void
}) {
  const [requests, setRequests] = useState<OperatorSupportRequest[]>([])
  const [events, setEvents] = useState<OperatorSupportRequestEvent[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [cursorStack, setCursorStack] = useState<(string | null)[]>([null])
  const [nextCursor, setNextCursor] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [reloadKey, setReloadKey] = useState(0)
  const [notice, setNotice] = useState<Notice | null>(null)
  const selected = requests.find((request) => request.id === selectedId) ?? null
  const currentCursor = cursorStack[cursorStack.length - 1]

  useEffect(() => {
    let active = true
    setLoading(true)
    void listOperatorSupportRequests({ apiBaseUrl, accessToken, cursor: currentCursor }).then((page) => {
      if (!active) return
      setRequests(page.items)
      setNextCursor(page.nextCursor)
      setSelectedId((current) => page.items.some((request) => request.id === current) ? current : page.items[0]?.id ?? null)
      setLoading(false)
    }).catch((reason: unknown) => {
      if (!active) return
      if (reason instanceof OperatorApiError && reason.status === 401) void onUnauthorized()
      setRequests([])
      setNextCursor(null)
      setNotice({ kind: 'error', message: reason instanceof Error ? reason.message : 'Tenant support requests could not be loaded.' })
      setLoading(false)
    })
    return () => { active = false }
  }, [accessToken, apiBaseUrl, currentCursor, onUnauthorized, refreshKey, reloadKey])

  useEffect(() => {
    if (!selected) { setEvents([]); return }
    let active = true
    void listOperatorSupportRequestEvents({ apiBaseUrl, accessToken, requestId: selected.id }).then((page) => {
      if (active) setEvents(page.items)
    }).catch((reason: unknown) => {
      if (active) setNotice({ kind: 'error', message: reason instanceof Error ? reason.message : 'Support request history could not be loaded.' })
    })
    return () => { active = false }
  }, [accessToken, apiBaseUrl, selected?.id, selected?.revision])

  async function createIssueFromRequest() {
    if (!selected) return
    setBusy(true)
    setNotice(null)
    try {
      const issue = await createSupportIssue({
        apiBaseUrl,
        accessToken,
        title: selected.title,
        category: SUPPORT_ISSUE_CATEGORY[selected.category],
        severity: 'normal',
        source: { kind: 'support_request', id: selected.id },
      })
      setNotice({ kind: 'success', message: 'Internal issue created. This request keeps its own tenant-visible progress.' })
      onIssueCreated(issue)
      setReloadKey((value) => value + 1)
    } catch (reason) {
      if (reason instanceof OperatorApiError && reason.status === 401) void onUnauthorized()
      setNotice({ kind: 'error', message: reason instanceof Error ? reason.message : 'The internal issue could not be created.' })
    } finally { setBusy(false) }
  }

  async function saveRequest(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selected) return
    const form = new FormData(event.currentTarget)
    const statusValue = String(form.get('status') ?? '')
    if (!isRequestStatus(statusValue)) { setNotice({ kind: 'error', message: 'Choose a valid support status.' }); return }
    const customerVisibleUpdate = String(form.get('customerVisibleUpdate') ?? '').trim()
    const privateNote = String(form.get('privateNote') ?? '').trim()
    if (statusValue === selected.status && !customerVisibleUpdate && !privateNote) {
      setNotice({ kind: 'error', message: 'Change the status or add a public update or private note.' })
      return
    }
    setBusy(true)
    setNotice(null)
    try {
      await updateOperatorSupportRequest({
        apiBaseUrl,
        accessToken,
        requestId: selected.id,
        expectedRevision: selected.revision,
        status: statusValue,
        customerVisibleUpdate,
        privateNote,
      })
      setNotice({ kind: 'success', message: 'Request updated. Only the public message and status are visible to that business.' })
      setReloadKey((value) => value + 1)
    } catch (reason) {
      if (reason instanceof OperatorApiError && reason.status === 401) void onUnauthorized()
      setNotice({ kind: 'error', message: reason instanceof OperatorApiError && reason.status === 409
        ? 'This request changed in another session. Refresh before saving.'
        : reason instanceof Error ? reason.message : 'The support request could not be updated.' })
    } finally { setBusy(false) }
  }

  async function linkRequest(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!selected || !selectedIssue) return
    const value = String(new FormData(event.currentTarget).get('relationship') ?? '')
    if (value !== 'duplicate' && value !== 'related') { setNotice({ kind: 'error', message: 'Choose duplicate or related.' }); return }
    setBusy(true)
    setNotice(null)
    try {
      const issue = await linkSupportRequestToIssue({
        apiBaseUrl,
        accessToken,
        issueId: selectedIssue.id,
        requestId: selected.id,
        expectedRevision: selectedIssue.revision,
        relationship: value,
      })
      onIssueCreated(issue)
      setNotice({ kind: 'success', message: 'Tenant request linked to the selected issue. Its public progress remains separate.' })
      setReloadKey((current) => current + 1)
    } catch (reason) {
      if (reason instanceof OperatorApiError && reason.status === 401) void onUnauthorized()
      setNotice({ kind: 'error', message: reason instanceof OperatorApiError && reason.status === 409
        ? 'The issue changed or this request is already linked. Refresh and retry.'
        : reason instanceof Error ? reason.message : 'The request could not be linked.' })
    } finally { setBusy(false) }
  }

  return <section className="operator-support-requests panel-card" aria-labelledby="tenant-support-requests-title">
    <div className="panel-heading"><div><span className="eyebrow">TENANT INTAKE</span><h2 id="tenant-support-requests-title">General support requests</h2><p>Update each business’s progress separately. Internal issue details and notes stay in the operator workspace.</p></div><div className="pagination"><button type="button" className="icon-button" aria-label="Previous support request page" disabled={cursorStack.length <= 1 || loading} onClick={() => setCursorStack((old) => old.slice(0, -1))}><ChevronLeft size={17}/></button><span>{cursorStack.length}</span><button type="button" className="icon-button" aria-label="Next support request page" disabled={!nextCursor || loading} onClick={() => { if (nextCursor) setCursorStack((old) => [...old, nextCursor]) }}><ChevronRight size={17}/></button></div></div>
    {notice && <div className={`notice notice-${notice.kind}`} role={notice.kind === 'error' ? 'alert' : 'status'}><span className="notice-mark">{notice.kind === 'error' ? <CircleAlert size={17}/> : <Check size={17}/>}</span><span>{notice.message}</span><button type="button" aria-label="Dismiss notification" onClick={() => setNotice(null)}><X size={16}/></button></div>}
    {loading ? <div className="list-state"><span className="spinner"/><strong>Loading requests</strong><small>Reading tenant-submitted support requests…</small></div> : requests.length === 0 ? <p className="kanban-empty">No general support requests on this page.</p> : <div className="operator-support-request-layout">
      <div className="operator-support-request-list" aria-label="Tenant support requests">{requests.map((request) => <button type="button" key={request.id} className={`operator-support-request-row${selected?.id === request.id ? ' selected' : ''}`} onClick={() => setSelectedId(request.id)} aria-pressed={selected?.id === request.id}>
        <span><strong>{request.title}</strong><small>{request.category.replaceAll('_', ' ')} · branch {request.branchId.slice(0, 8)}</small></span><span className={`status-pill status-${request.status}`}><i/>{STATUS_LABELS[request.status]}</span>
      </button>)}</div>
      {selected && <div className="operator-support-request-detail">
        <header><div><span className="eyebrow">BUSINESS REQUEST</span><h3>{selected.title}</h3><small>{selected.category.replaceAll('_', ' ')} · tenant {selected.tenantId.slice(0, 8)} · branch {selected.branchId.slice(0, 8)}</small></div><span className={`status-pill status-${selected.status}`}><i/>{STATUS_LABELS[selected.status]}</span></header>
        <p className="operator-support-request-context">{selected.details || 'No additional details were submitted.'}</p>
        <div className="progress-card"><strong>Client diagnostics</strong><p>{selected.diagnosticsConsent
          ? `${selected.diagnosticsAppVersion} · ${selected.diagnosticsPlatform}`
          : 'Not shared. The business did not opt in to sharing diagnostics.'}</p></div>
        {selected.customerVisibleUpdate && <div className="progress-card"><strong>Current public update</strong><p>{selected.customerVisibleUpdate}</p></div>}
        <div className="operator-support-request-actions"><button type="button" className="button button-quiet" onClick={() => void createIssueFromRequest()} disabled={busy}>Create internal issue</button>
          <form onSubmit={(event) => void linkRequest(event)}><label><span>Link to selected issue</span><select name="relationship" defaultValue="duplicate"><option value="duplicate">Duplicate</option><option value="related">Related</option></select></label><button type="submit" className="button button-quiet" disabled={busy || !selectedIssue}>{selectedIssue ? `Link to ISS-${selectedIssue.id.slice(0, 8).toUpperCase()}` : 'Select an issue above'}</button></form>
        </div>
        <form className="support-request-update-form" onSubmit={(event) => void saveRequest(event)} key={`${selected.id}:${selected.revision}`}>
          <div className="form-heading"><div><span className="eyebrow">CUSTOMER PROGRESS</span><h3>Update this request</h3></div><span className="revision-tag">REV {selected.revision}</span></div>
          <label className="form-field"><span>Status</span><select name="status" defaultValue={selected.status}>{[selected.status, ...nextStatuses(selected.status)].map((status) => <option key={status} value={status}>{STATUS_LABELS[status]}</option>)}</select></label>
          <label className="form-field"><span>Public update <em>Optional</em></span><textarea name="customerVisibleUpdate" maxLength={1000} rows={3} placeholder="A clear update the business can act on…"/></label>
          <label className="form-field"><span>Private note <em>Optional</em></span><textarea name="privateNote" maxLength={2000} rows={2} placeholder="Internal-only operator context…"/></label>
          <button className="button button-primary" type="submit" disabled={busy}>{busy ? 'Saving…' : 'Save request update'}</button>
        </form>
        <div className="operator-support-request-history"><strong>Operator history</strong>{events.length === 0 ? <p className="kanban-empty">No updates yet.</p> : <ol>{events.map((entry) => <li key={entry.id}><span>{entry.eventKind === 'private_note' ? `Private note: ${entry.privateNote ?? ''}` : `${entry.newStatus ? STATUS_LABELS[entry.newStatus] : 'Public update'}${entry.customerVisibleUpdate ? ` · ${entry.customerVisibleUpdate}` : ''}`}</span><time>{displayDate(entry.createdAt)}</time></li>)}</ol>}</div>
      </div>}
    </div>}
  </section>
}

function RequestRow({ item, active, onClick }: { item: PaymentIntegrationRequest; active: boolean; onClick: () => void }) {
  return <button type="button" className={`queue-row ${active ? 'selected' : ''}`} onClick={onClick} aria-pressed={active}>
    <span className="provider-cell"><span className="provider-avatar"><CreditCard size={17}/></span><span className="provider-copy"><strong>{item.providerName}</strong><small>{item.capability}{item.terminalModel ? ` · ${item.terminalModel}` : ''}</small></span></span>
    <span className="business-cell"><strong>{item.countryCode} <span className="business-dot">·</span> {item.businessImpact === 'business_blocked' ? 'Blocked' : item.businessImpact === 'workaround_available' ? 'Workaround' : 'Exploring'}</strong><small>Branch {item.branchId.slice(0, 8)}</small></span>
    <span className={`status-pill status-${item.status}`}><i/>{STATUS_LABELS[item.status]}</span>
    <span className="received-cell">{item.dueAt ? `Due ${displayDate(item.dueAt)}` : displayDate(item.createdAt)}</span>
    <span className="row-arrow"><ChevronRight size={16}/></span>
  </button>
}

function RequestDetails({ item, saving, onSave, apiBaseUrl, accessToken, onUnauthorized }: { item: PaymentIntegrationRequest; saving: boolean; onSave: (event: FormEvent<HTMLFormElement>) => void; apiBaseUrl: string; accessToken: string; onUnauthorized: () => Promise<void> }) {
  const allowed = nextStatuses(item.status)
  const [events, setEvents] = useState<RequestEvent[]>([])
  const [eventCursor, setEventCursor] = useState<string | null>(null)
  const [eventCursorStack, setEventCursorStack] = useState<(string | null)[]>([null])
  const [eventsLoading, setEventsLoading] = useState(true)
  const [eventsError, setEventsError] = useState<string | null>(null)
  const [eventsReload, setEventsReload] = useState(0)
  const eventCursorCurrent = eventCursorStack[eventCursorStack.length - 1]
  useEffect(() => {
    let cancelled = false
    setEventsLoading(true)
    void listRequestEvents({ apiBaseUrl, accessToken, requestId: item.id, cursor: eventCursorCurrent })
      .then((page) => {
        if (cancelled) return
        setEvents(page.items)
        setEventCursor(page.nextCursor)
        setEventsError(null)
        setEventsLoading(false)
      })
      .catch((reason: unknown) => {
        if (cancelled) return
        if (reason instanceof OperatorApiError && reason.status === 401) void onUnauthorized()
        setEventsError(reason instanceof Error ? reason.message : 'Activity history could not be loaded.')
        setEventsLoading(false)
      })
    return () => { cancelled = true }
  }, [accessToken, apiBaseUrl, eventCursorCurrent, eventsReload, item.id, onUnauthorized])
  return <>
    <div className="detail-header"><div><span className="eyebrow">REQUEST DETAILS</span><span className="request-ref">REQ-{item.id.slice(0, 8).toUpperCase()}</span></div><span className={`status-pill status-${item.status}`}><i/>{STATUS_LABELS[item.status]}</span></div>
    <h2 className="detail-title">{item.providerName}</h2>
    <p className="detail-subtitle">{item.capability}{item.terminalModel ? ` · ${item.terminalModel}` : ''}</p>
    <div className="detail-meta-grid"><div><span>COUNTRY</span><strong><Building2 size={14}/>{item.countryCode}</strong></div><div><span>BUSINESS IMPACT</span><strong className={`impact impact-${item.businessImpact}`}>{item.businessImpact === 'business_blocked' ? <ArrowUpRight size={14}/> : item.businessImpact === 'workaround_available' ? <ArrowDownRight size={14}/> : <Activity size={14}/>} {IMPACT_LABELS[item.businessImpact]}</strong></div><div><span>BRANCH ID</span><strong className="mono-text">{item.branchId}</strong></div><div><span>TENANT ID</span><strong className="mono-text">{item.tenantId}</strong></div></div>
    <div className="detail-section"><div className="detail-section-heading"><span className="section-mini-icon"><FileText size={15}/></span><span><strong>Request context</strong><small>Submitted {displayDate(item.createdAt)}</small></span></div><p className="request-context">{item.details || 'No additional context was provided.'}</p></div>
    <div className="progress-card"><div className="progress-card-top"><span className="section-mini-icon"><MessageSquareText size={15}/></span><span><strong>Business-visible progress</strong><small>Shown to authorized business managers</small></span></div>{item.customerVisibleUpdate ? <p>{item.customerVisibleUpdate}</p> : <p className="no-update">No progress message has been shared yet.</p>}<small className="updated-at">Last updated {displayDate(item.updatedAt)} · revision {item.revision}</small></div>
    <section className="event-history" aria-label="Request activity history">
      <div className="event-history-heading"><div><span className="eyebrow">AUDIT TRAIL</span><h3>Request activity</h3></div>{eventCursorStack.length > 1 && <button className="text-button" type="button" disabled={eventsLoading} onClick={() => setEventCursorStack((old) => old.slice(0, -1))}>Newer activity</button>}</div>
      {eventsLoading ? <p className="event-state">Loading request activity…</p> : eventsError ? <div className="event-state event-error" role="alert">{eventsError}<button className="text-button" type="button" onClick={() => setEventsReload((value) => value + 1)}>Retry</button></div> : events.length === 0 ? <p className="event-state">No operator updates have been recorded.</p> : <ol className="event-list">{events.map((event) => <li className={`event-item ${event.eventKind === 'private_note' ? 'event-private' : ''}`} key={event.id}><span className="event-dot"/><div><div className="event-title"><strong>{event.eventKind === 'private_note' ? 'Internal support note' : event.eventKind === 'status_changed' ? 'Status changed' : event.eventKind === 'schedule_changed' ? 'Due date changed' : 'Progress shared'}</strong><time>{displayDate(event.createdAt)}</time></div>{event.previousStatus && event.newStatus && event.previousStatus !== event.newStatus && <p className="event-status">{STATUS_LABELS[event.previousStatus]} → {STATUS_LABELS[event.newStatus]}</p>}{event.dueAt && <p>Due {displayDate(event.dueAt)}</p>}{event.eventKind === 'schedule_changed' && !event.dueAt && <p>Due date cleared</p>}{event.customerVisibleUpdate && <p>{event.customerVisibleUpdate}</p>}{event.privateNote && <p>{event.privateNote}</p>}<small>Revision {event.resultingRevision}{event.eventKind === 'private_note' || event.eventKind === 'schedule_changed' ? ' · Internal only' : ''}</small></div></li>)}</ol>}
      {eventCursor && !eventsLoading && !eventsError && <button className="text-button event-more" type="button" onClick={() => setEventCursorStack((old) => [...old, eventCursor])}>Load earlier activity</button>}
    </section>
    <form className="update-form" onSubmit={onSave}>
      <div className="form-heading"><div><span className="eyebrow">TRIAGE ACTION</span><h3>Update request</h3></div><span className="revision-tag">REV {item.revision}</span></div>
      {allowed.length ? <>
        <label className="form-field"><span>Move to status</span><select name="status" defaultValue={item.status}><option value={item.status}>Keep {STATUS_LABELS[item.status]}</option>{allowed.map((status) => <option key={status} value={status}>{STATUS_LABELS[status]}</option>)}</select><small>Only permitted next steps are available.</small></label>
        <label className="form-field"><span>Internal due date <em>Optional</em></span><input name="dueAt" type="datetime-local" defaultValue={dateTimeInputValue(item.dueAt)}/><small>Visible to operators only. Leave blank to clear the due date.</small></label>
        <label className="form-field"><span>Progress message for the business <em>Optional</em></span><textarea name="customerVisibleUpdate" maxLength={1000} rows={3} placeholder="Share a clear update the business can act on…"/><small>This message appears in their payment settings.</small></label>
        <label className="form-field"><span>Private support note <em>Optional</em></span><textarea name="privateNote" maxLength={2000} rows={3} placeholder="Internal context for the support team…"/><small>Visible only to authorized operators. Do not include passwords, payment credentials or card details.</small></label>
        <button className="button button-primary submit-update" type="submit" disabled={saving}>{saving ? <span className="button-spinner"/> : <Check size={16}/>}<span>{saving ? 'Saving update…' : 'Save request changes'}</span><ArrowUpRight size={15}/></button>
      </> : <div className="terminal-state"><Check size={17}/><span><strong>This request is closed</strong><small>No further status changes are available.</small></span></div>}
    </form>
  </>
}

export default App
