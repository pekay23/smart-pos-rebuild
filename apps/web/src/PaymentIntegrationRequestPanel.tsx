import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react'
import { CircleAlert, CircleCheck, RefreshCw, Send } from 'lucide-react'
import {
  ApiError,
  createPaymentIntegrationRequest,
  listPaymentIntegrationRequests,
} from './api'
import type { NewPaymentIntegrationRequest, PaymentIntegrationRequest } from './api'

type Draft = NewPaymentIntegrationRequest
type LoadState = { kind: 'loading' } | { kind: 'ready'; requests: PaymentIntegrationRequest[] } | { kind: 'error'; message: string }

const EMPTY_DRAFT: Draft = {
  countryCode: '',
  providerName: '',
  terminalModel: null,
  capability: 'physical_terminal',
  businessImpact: 'workaround_available',
  details: null,
}

const STATUS_LABELS: Record<PaymentIntegrationRequest['status'], string> = {
  submitted: 'Submitted',
  under_review: 'Under review',
  planned: 'Planned',
  in_progress: 'In progress',
  released: 'Released',
  not_planned: 'Not planned',
}

const IMPACT_LABELS: Record<Draft['businessImpact'], string> = {
  business_blocked: 'This prevents us from taking payments',
  workaround_available: 'We have a manual workaround',
  exploring: 'We are checking whether this is supported',
}

function errorMessage(error: unknown): string {
  if (error instanceof ApiError && error.status === 409) return 'This request changed while it was being submitted. Refresh the list and submit it again.'
  return error instanceof Error ? error.message : 'The request could not be submitted.'
}

function displayDate(value: string): string {
  const timestamp = new Date(value)
  return Number.isNaN(timestamp.valueOf()) ? value : timestamp.toLocaleString()
}

export function PaymentIntegrationRequestPanel({ apiBaseUrl, accessToken, tenantId, branchId }: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  branchId: string
}) {
  const [state, setState] = useState<LoadState>({ kind: 'loading' })
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT)
  const [isOpen, setIsOpen] = useState(false)
  const [isSaving, setIsSaving] = useState(false)
  const [notice, setNotice] = useState<{ kind: 'success' | 'error'; text: string } | null>(null)
  const idempotencyKey = useRef(crypto.randomUUID())

  const load = useCallback(async () => {
    setState({ kind: 'loading' })
    try {
      const requests = await listPaymentIntegrationRequests({ apiBaseUrl, accessToken, tenantId, branchId })
      setState({ kind: 'ready', requests })
    } catch (error) {
      setState({ kind: 'error', message: errorMessage(error) })
    }
  }, [accessToken, apiBaseUrl, branchId, tenantId])

  useEffect(() => { void load() }, [load])

  function change<K extends keyof Draft>(key: K, value: Draft[K]) {
    idempotencyKey.current = crypto.randomUUID()
    setDraft((current) => ({ ...current, [key]: value }))
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setIsSaving(true)
    setNotice(null)
    try {
      const request = await createPaymentIntegrationRequest({
        apiBaseUrl,
        accessToken,
        tenantId,
        branchId,
        request: {
          ...draft,
          countryCode: draft.countryCode.trim().toUpperCase(),
          providerName: draft.providerName.trim(),
          terminalModel: draft.terminalModel?.trim() || null,
          details: draft.details?.trim() || null,
        },
        idempotencyKey: idempotencyKey.current,
      })
      setState((current) => ({
        kind: 'ready',
        requests: [request, ...(current.kind === 'ready' ? current.requests : []).filter((item) => item.id !== request.id)].slice(0, 100),
      }))
      setDraft(EMPTY_DRAFT)
      idempotencyKey.current = crypto.randomUUID()
      setIsOpen(false)
      setNotice({ kind: 'success', text: `Request recorded for operator triage. No notification or payment connection is triggered. Reference ${request.id.slice(0, 8)}.` })
    } catch (error) {
      setNotice({ kind: 'error', text: errorMessage(error) })
    } finally {
      setIsSaving(false)
    }
  }

  return <section className="payment-integration-requests" aria-labelledby="integration-requests-title">
    <header className="payment-integration-requests-header">
      <div>
        <p className="eyebrow">PROVIDER SUPPORT</p>
        <h3 id="integration-requests-title">Request a payment integration</h3>
        <p>Record which bank, terminal or processor this branch uses. An operator queue API is available; its dashboard and notifications are still being built. Saving a request does not connect or approve payments.</p>
      </div>
      <div className="payment-integration-requests-actions">
        <button type="button" className="button button-quiet" onClick={() => void load()} disabled={state.kind === 'loading'} aria-label="Refresh integration requests"><RefreshCw size={15}/> Refresh</button>
        <button type="button" className="button button-primary" onClick={() => setIsOpen((open) => !open)}>{isOpen ? 'Close form' : 'Request support'}</button>
      </div>
    </header>

    {notice && <div className={`catalog-notice ${notice.kind}`} role={notice.kind === 'error' ? 'alert' : 'status'}>
      <span>{notice.kind === 'error' ? <CircleAlert size={16}/> : <CircleCheck size={16}/>}</span>{notice.text}
      <button type="button" aria-label="Dismiss message" onClick={() => setNotice(null)}>×</button>
    </div>}

    {isOpen && <form className="payment-integration-request-form" onSubmit={(event) => void submit(event)}>
      <div className="payment-integration-request-fields">
        <label>Country<input required minLength={2} maxLength={2} pattern="[A-Za-z]{2}" autoCapitalize="characters" autoComplete="country" value={draft.countryCode} onChange={(event) => change('countryCode', event.target.value)} placeholder="GH"/><small>Use the two-letter country code, such as GH or NG.</small></label>
        <label>Bank or payment provider<input required maxLength={120} value={draft.providerName} onChange={(event) => change('providerName', event.target.value)} placeholder="e.g. a bank, Paystack or MTN MoMo"/></label>
        <label>Terminal model, if relevant<input maxLength={120} value={draft.terminalModel ?? ''} onChange={(event) => change('terminalModel', event.target.value || null)} placeholder="e.g. model printed on the device"/></label>
        <label>What should the integration support?<select required value={draft.capability} onChange={(event) => change('capability', event.target.value)}>
          <option value="physical_terminal">Physical bank terminal</option>
          <option value="mobile_money">Mobile money collection</option>
          <option value="bank_transfer">Bank transfer confirmation</option>
          <option value="online_checkout">Online checkout or payment link</option>
          <option value="refunds">Refunds or reversals</option>
          <option value="other">Other payment workflow</option>
        </select></label>
        <label>Business impact<select value={draft.businessImpact} onChange={(event) => change('businessImpact', event.target.value as Draft['businessImpact'])}>
          {Object.entries(IMPACT_LABELS).map(([value, label]) => <option value={value} key={value}>{label}</option>)}
        </select></label>
        <label className="payment-integration-request-details">What happens today?<textarea maxLength={1000} rows={3} value={draft.details ?? ''} onChange={(event) => change('details', event.target.value || null)} placeholder="Describe the step you need help with. Do not include credentials or payment account details."/></label>
      </div>
      <div className="payment-option-disclosure"><CircleAlert size={15}/><span>Never include API keys, passwords, PINs, card details, account numbers or customer payment data. The provider will remain manually recorded until an integration is built and enabled.</span></div>
      <div className="catalog-form-actions"><button type="button" className="button button-quiet" onClick={() => setIsOpen(false)} disabled={isSaving}>Cancel</button><button type="submit" className="button button-primary" disabled={isSaving}><Send size={14}/>{isSaving ? 'Saving…' : 'Save request'}</button></div>
    </form>}

    {state.kind === 'loading' && <div className="catalog-state"><span className="mini-spinner"/><span>Loading requests…</span></div>}
    {state.kind === 'error' && <div className="catalog-state catalog-state-error" role="alert"><CircleAlert size={18}/><span>{state.message}</span><button type="button" className="button button-quiet" onClick={() => void load()}>Try again</button></div>}
    {state.kind === 'ready' && state.requests.length === 0 && <p className="payment-integration-empty">No integration requests have been saved for this branch.</p>}
    {state.kind === 'ready' && state.requests.length > 0 && <div className="payment-integration-request-list" aria-label="Saved payment integration requests">
      {state.requests.map((request) => <article className="payment-integration-request-card" key={request.id}>
        <div className="payment-integration-request-card-heading"><strong>{request.providerName} · {request.countryCode}</strong><span className={`payment-integration-status ${request.status}`}>{STATUS_LABELS[request.status]}</span></div>
        <p>{request.capability.replaceAll('_', ' ')}{request.terminalModel ? ` · ${request.terminalModel}` : ''}</p>
        <small>{IMPACT_LABELS[request.businessImpact]} · {displayDate(request.createdAt)}</small>
        {request.details && <details><summary>Request details</summary><p>{request.details}</p></details>}
        {request.customerVisibleUpdate && <div className="payment-integration-public-update"><strong>Latest update</strong><p>{request.customerVisibleUpdate}</p><small>Updated {displayDate(request.updatedAt)}</small></div>}
      </article>)}
    </div>}
  </section>
}
