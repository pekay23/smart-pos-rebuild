import { useCallback, useEffect, useState, type FormEvent } from 'react'
import { Check, CircleAlert, LifeBuoy, Plus, RefreshCw, Send } from 'lucide-react'
import { ApiError, createSupportRequest, listSupportRequests } from './api'
import type { SupportRequest, SupportRequestCategory } from './api'

declare const __APP_VERSION__: string

type ViewState = { kind: 'loading' } | { kind: 'ready'; requests: SupportRequest[] } | { kind: 'error'; message: string }
type Notice = { kind: 'success' | 'error'; message: string }

const CATEGORIES: ReadonlyArray<{ value: SupportRequestCategory; label: string }> = [
  { value: 'bug', label: 'Something is not working' },
  { value: 'feature_request', label: 'Request a feature' },
  { value: 'account_access', label: 'Account access' },
  { value: 'data_issue', label: 'Business data issue' },
  { value: 'training', label: 'How-to or training' },
  { value: 'other', label: 'Other' },
]

const STATUS_LABELS = {
  submitted: 'Submitted',
  under_review: 'Under review',
  planned: 'Planned',
  in_progress: 'In progress',
  released: 'Resolved',
  not_planned: 'Not planned',
} satisfies Record<SupportRequest['status'], string>

function formatDate(value: string): string {
  const timestamp = Date.parse(value)
  return Number.isNaN(timestamp) ? 'Date unavailable' : new Date(timestamp).toLocaleString()
}

export function SupportRequestsPanel({ apiBaseUrl, accessToken, tenantId, branchId, branchName }: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  branchId: string
  branchName: string
}) {
  const [state, setState] = useState<ViewState>({ kind: 'loading' })
  const [formOpen, setFormOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState<Notice | null>(null)

  const load = useCallback(async () => {
    setState({ kind: 'loading' })
    try {
      const requests = await listSupportRequests({ apiBaseUrl, accessToken, tenantId, branchId })
      setState({ kind: 'ready', requests })
    } catch (reason) {
      setState({ kind: 'error', message: reason instanceof Error ? reason.message : 'Support requests could not be loaded.' })
    }
  }, [accessToken, apiBaseUrl, branchId, tenantId])

  useEffect(() => { void load() }, [load])

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const formElement = event.currentTarget
    const form = new FormData(formElement)
    const category = String(form.get('category'))
    const title = String(form.get('title') ?? '').trim()
    const details = String(form.get('details') ?? '').trim()
    const diagnosticsConsent = form.get('diagnosticsConsent') === 'on'
    const selectedCategory = CATEGORIES.find((item) => item.value === category)?.value
    if (!selectedCategory || title.length < 3) {
      setNotice({ kind: 'error', message: 'Choose a category and enter a title with at least three characters.' })
      return
    }
    setBusy(true)
    setNotice(null)
    try {
      const created = await createSupportRequest({
        apiBaseUrl,
        accessToken,
        tenantId,
        branchId,
        request: {
          category: selectedCategory,
          title,
          details: details || null,
          ...(diagnosticsConsent ? {
            diagnosticsConsent: true,
            diagnosticsAppVersion: __APP_VERSION__,
            diagnosticsPlatform: 'web' as const,
          } : {}),
        },
        idempotencyKey: crypto.randomUUID(),
      })
      setState((current) => current.kind === 'ready'
        ? { kind: 'ready', requests: [created, ...current.requests.filter((request) => request.id !== created.id)] }
        : current)
      setFormOpen(false)
      formElement.reset()
      setNotice({ kind: 'success', message: 'Your support request was sent. Updates will appear here.' })
    } catch (reason) {
      setNotice({
        kind: 'error',
        message: reason instanceof ApiError && reason.status === 409
          ? 'This submission conflicted with an earlier request. Try sending it again.'
          : reason instanceof Error ? reason.message : 'The support request could not be sent.',
      })
    } finally {
      setBusy(false)
    }
  }

  return <section className="support-requests-panel" aria-labelledby="support-requests-title">
    <header className="support-requests-header">
      <div className="support-requests-heading">
        <span className="support-requests-icon"><LifeBuoy size={18}/></span>
        <div><p className="eyebrow">BUSINESS SUPPORT</p><h2 id="support-requests-title">Support for {branchName}</h2><p>Send a question or report a problem. Only staff with support access can see requests from this branch.</p></div>
      </div>
      <div className="support-requests-actions">
        <button type="button" className="button button-quiet" onClick={() => void load()} disabled={state.kind === 'loading'}><RefreshCw size={15}/> Refresh</button>
        <button type="button" className="button button-primary" onClick={() => { setFormOpen((open) => !open); setNotice(null) }}><Plus size={15}/>{formOpen ? 'Close' : 'New request'}</button>
      </div>
    </header>

    {notice && <div className={`support-requests-notice ${notice.kind}`} role={notice.kind === 'error' ? 'alert' : 'status'}><span>{notice.kind === 'error' ? <CircleAlert size={16}/> : <Check size={16}/>}</span>{notice.message}<button type="button" aria-label="Dismiss message" onClick={() => setNotice(null)}>×</button></div>}

    {formOpen && <form className="support-request-form" onSubmit={(event) => void submit(event)}>
      <div className="support-request-fields">
        <label>What do you need help with?<select name="category" defaultValue="bug">{CATEGORIES.map((item) => <option key={item.value} value={item.value}>{item.label}</option>)}</select></label>
        <label>Short title<input name="title" required minLength={3} maxLength={120} placeholder="For example: scanner stops reading barcodes"/></label>
        <label className="support-request-details">Details<textarea name="details" maxLength={2000} rows={4} placeholder="What happened, and what were you trying to do? Do not include passwords, PINs, payment details or API keys."/><small>Up to 2,000 characters. Attachments and contact details are not collected here.</small></label>
        <label className="support-request-diagnostics"><input type="checkbox" name="diagnosticsConsent"/><span><strong>Share basic app diagnostics</strong><small>Optional. If selected, support receives this app's version and client type only. No logs, business data, device identifiers or browser details are sent.</small></span></label>
      </div>
      <div className="support-request-form-actions"><button type="button" className="button button-quiet" onClick={() => setFormOpen(false)} disabled={busy}>Cancel</button><button type="submit" className="button button-primary" disabled={busy}>{busy ? 'Sending…' : <><Send size={14}/> Send request</>}</button></div>
    </form>}

    {state.kind === 'loading' && <div className="support-requests-state"><span className="mini-spinner"/>Loading support requests…</div>}
    {state.kind === 'error' && <div className="support-requests-state error" role="alert"><CircleAlert size={18}/><span>{state.message}</span><button type="button" className="button button-quiet" onClick={() => void load()}>Try again</button></div>}
    {state.kind === 'ready' && state.requests.length === 0 && <div className="support-requests-empty"><strong>No support requests yet</strong><span>Use New request if your team needs help.</span></div>}
    {state.kind === 'ready' && state.requests.length > 0 && <div className="support-request-list" aria-label="Support requests for this branch">{state.requests.map((request) => <article className="support-request-card" key={request.id}>
      <header><div><span className="support-request-category">{CATEGORIES.find((item) => item.value === request.category)?.label ?? 'Support request'}</span><h3>{request.title}</h3><small>Submitted {formatDate(request.createdAt)}</small></div><span className={`support-request-status ${request.status}`}>{STATUS_LABELS[request.status]}</span></header>
      {request.details && <p className="support-request-body">{request.details}</p>}
      {request.diagnosticsConsent && <small className="support-request-diagnostics-summary">Basic diagnostics shared: {request.diagnosticsAppVersion} · {request.diagnosticsPlatform}</small>}
      {request.customerVisibleUpdate && <div className="support-request-update"><strong>Latest update</strong><p>{request.customerVisibleUpdate}</p><small>Updated {formatDate(request.updatedAt)}</small></div>}
    </article>)}</div>}
  </section>
}
