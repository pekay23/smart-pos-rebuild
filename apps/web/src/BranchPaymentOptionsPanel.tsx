import { useCallback, useEffect, useState, type FormEvent } from 'react'
import { Check, CircleAlert, CreditCard, Plus, RefreshCw, Save, Wallet } from 'lucide-react'
import { ApiError, createBranchPaymentOption, listBranchPaymentOptions, updateBranchPaymentOption } from './api'
import type { BranchPaymentOption, NewBranchPaymentOption, TenderKind } from './api'
import { PaymentIntegrationRequestPanel } from './PaymentIntegrationRequestPanel'

type LoadState = { kind: 'loading' } | { kind: 'ready'; options: BranchPaymentOption[] } | { kind: 'error'; message: string }
type Draft = { methodKey: string; label: string; tenderKind: TenderKind; providerName: string; countryCode: string; requiresReference: boolean }

const EMPTY_DRAFT: Draft = { methodKey: '', label: '', tenderKind: 'cash', providerName: '', countryCode: '', requiresReference: false }
const KIND_LABELS: Record<TenderKind, string> = { cash: 'Cash', card: 'Card', mobile_money: 'Mobile money', bank_transfer: 'Bank transfer', other: 'Other' }

function errorText(reason: unknown, fallback: string): string {
  return reason instanceof Error ? reason.message : fallback
}

function normalize(draft: Draft): NewBranchPaymentOption {
  const methodKey = draft.methodKey.trim().toLowerCase()
  if (!/^[a-z0-9][a-z0-9_-]{1,47}$/.test(methodKey)) throw new Error('Use a 2–48 character key with letters, numbers, hyphens or underscores.')
  if (draft.label.trim().length === 0) throw new Error('Enter a label staff will recognize at checkout.')
  const countryCode = draft.countryCode.trim().toUpperCase()
  if (countryCode && !/^[A-Z]{2}$/.test(countryCode)) throw new Error('Enter the two-letter ISO country code, such as GH or NG.')
  if (draft.tenderKind === 'cash' && draft.requiresReference) throw new Error('Cash payments cannot require a terminal reference.')
  return {
    methodKey,
    label: draft.label.trim(),
    tenderKind: draft.tenderKind,
    providerName: draft.providerName.trim() || null,
    countryCode: countryCode || null,
    requiresReference: draft.tenderKind === 'cash' ? false : draft.requiresReference,
  }
}

export function BranchPaymentOptionsPanel({ apiBaseUrl, accessToken, tenantId, branchId, branchName, canManage }: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  branchId: string
  branchName: string
  canManage: boolean
}) {
  const [state, setState] = useState<LoadState>({ kind: 'loading' })
  const [draft, setDraft] = useState(EMPTY_DRAFT)
  const [formOpen, setFormOpen] = useState(false)
  const [busyId, setBusyId] = useState<string | null>(null)
  const [notice, setNotice] = useState<{ kind: 'success' | 'error'; text: string } | null>(null)

  const load = useCallback(async () => {
    setState({ kind: 'loading' })
    try {
      const options = await listBranchPaymentOptions({ apiBaseUrl, accessToken, tenantId, branchId })
      setState({ kind: 'ready', options })
    } catch (reason) {
      setState({ kind: 'error', message: errorText(reason, 'Payment options could not be loaded.') })
    }
  }, [accessToken, apiBaseUrl, branchId, tenantId])

  useEffect(() => { void load() }, [load])

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setNotice(null)
    try {
      const option = await createBranchPaymentOption({ apiBaseUrl, accessToken, tenantId, branchId, option: normalize(draft) })
      setState((current) => current.kind === 'ready' ? { kind: 'ready', options: [...current.options, option].sort((a, b) => a.label.localeCompare(b.label)) } : current)
      setDraft(EMPTY_DRAFT)
      setFormOpen(false)
      setNotice({ kind: 'success', text: `${option.label} was added to ${branchName}. It records manually until an integration is configured.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: reason instanceof ApiError && reason.status === 409
        ? 'That method key or label is already used at this branch.'
        : errorText(reason, 'Payment option could not be saved.') })
    }
  }

  async function toggleEnabled(option: BranchPaymentOption) {
    setBusyId(option.id)
    setNotice(null)
    try {
      const updated = await updateBranchPaymentOption({ apiBaseUrl, accessToken, tenantId, branchId, option: { ...option, enabled: !option.enabled } })
      setState((current) => current.kind === 'ready'
        ? { kind: 'ready', options: current.options.map((item) => item.id === updated.id ? updated : item) }
        : current)
      setNotice({ kind: 'success', text: `${updated.label} is ${updated.enabled ? 'available' : 'disabled'} at ${branchName}.` })
    } catch (reason) {
      setNotice({ kind: 'error', text: reason instanceof ApiError && reason.status === 409
        ? 'This option changed in another session. Refresh and retry.'
        : errorText(reason, 'Payment option could not be updated.') })
      if (reason instanceof ApiError && reason.status === 409) void load()
    } finally {
      setBusyId(null)
    }
  }

  return <section className="payment-options-panel" aria-labelledby="payment-options-title">
    <header className="payment-options-header"><div className="catalog-title-group"><span className="catalog-icon"><CreditCard size={18}/></span><div><p className="eyebrow">CHECKOUT SETTINGS</p><h2 id="payment-options-title">Payment methods at {branchName}</h2><p>Configure the options staff can record during checkout. Provider names here are labels, not connected payment processing.</p></div></div><div className="payment-options-actions"><button type="button" className="button button-quiet catalog-refresh" onClick={() => void load()} disabled={state.kind === 'loading'}><RefreshCw size={15}/> Refresh</button>{canManage && <button type="button" className="button button-primary" onClick={() => setFormOpen((open) => !open)}><Plus size={15}/> Add method</button>}</div></header>

    {notice && <div className={`catalog-notice ${notice.kind}`} role={notice.kind === 'error' ? 'alert' : 'status'}><span>{notice.kind === 'error' ? <CircleAlert size={16}/> : <Check size={16}/>}</span>{notice.text}<button type="button" aria-label="Dismiss message" onClick={() => setNotice(null)}>×</button></div>}

    {formOpen && canManage && <form className="payment-option-form" onSubmit={(event) => void save(event)}>
      <div className="payment-option-fields">
        <label>Method key<input required minLength={2} maxLength={48} pattern="[a-zA-Z0-9][a-zA-Z0-9_-]{1,47}" value={draft.methodKey} onChange={(event) => setDraft({ ...draft, methodKey: event.target.value })} placeholder="e.g. mtn-momo"/></label>
        <label>Checkout label<input required maxLength={96} value={draft.label} onChange={(event) => setDraft({ ...draft, label: event.target.value })} placeholder="e.g. MTN Mobile Money"/></label>
        <label>Type<select value={draft.tenderKind} onChange={(event) => { const tenderKind = event.target.value as TenderKind; setDraft({ ...draft, tenderKind, requiresReference: tenderKind !== 'cash' && draft.requiresReference }) }}>{Object.entries(KIND_LABELS).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
        <label>Provider name<input maxLength={120} value={draft.providerName} onChange={(event) => setDraft({ ...draft, providerName: event.target.value })} placeholder="e.g. Paystack, MTN, bank name"/></label>
        <label>Country code<input maxLength={2} value={draft.countryCode} onChange={(event) => setDraft({ ...draft, countryCode: event.target.value })} placeholder="GH" autoCapitalize="characters"/></label>
      </div>
      {draft.tenderKind !== 'cash' && <label className="payment-reference-setting"><input type="checkbox" checked={draft.requiresReference} onChange={(event) => setDraft({ ...draft, requiresReference: event.target.checked })}/> Require staff to enter a terminal or transfer reference</label>}
      <div className="payment-option-disclosure"><Wallet size={15}/><span>This method is marked as manually recorded. It does not send a charge or verify payment with the named provider.</span></div>
      <div className="catalog-form-actions"><button type="button" className="button button-quiet" onClick={() => { setFormOpen(false); setDraft(EMPTY_DRAFT) }}>Cancel</button><button type="submit" className="button button-primary"><Save size={14}/> Save method</button></div>
    </form>}

    {state.kind === 'loading' && <div className="catalog-state"><span className="mini-spinner"/><span>Loading payment methods…</span></div>}
    {state.kind === 'error' && <div className="catalog-state catalog-state-error" role="alert"><CircleAlert size={18}/><span>{state.message}</span><button type="button" className="button button-quiet" onClick={() => void load()}>Try again</button></div>}
    {state.kind === 'ready' && state.options.length === 0 && <div className="catalog-state catalog-empty"><strong>No payment methods configured</strong><span>A manager can add the methods this branch accepts.</span></div>}
    {state.kind === 'ready' && state.options.length > 0 && <div className="payment-option-list">{state.options.map((option) => <article className={`payment-option-row${option.enabled ? '' : ' disabled'}`} key={option.id}>
      <span className="payment-option-icon"><CreditCard size={16}/></span><span className="payment-option-copy"><strong>{option.label}</strong><small>{KIND_LABELS[option.tenderKind]}{option.providerName ? ` · ${option.providerName}` : ''}{option.countryCode ? ` · ${option.countryCode}` : ''}{option.requiresReference ? ' · Reference required' : ''}</small></span><span className="payment-mode-badge">Manual</span>{canManage && <button type="button" className={`button ${option.enabled ? 'button-quiet' : 'button-primary'} payment-option-toggle`} onClick={() => void toggleEnabled(option)} disabled={busyId === option.id}>{busyId === option.id ? 'Saving…' : option.enabled ? 'Disable' : 'Enable'}</button>}
    </article>)}</div>}
    {canManage && <PaymentIntegrationRequestPanel apiBaseUrl={apiBaseUrl} accessToken={accessToken} tenantId={tenantId} branchId={branchId}/>}
    <p className="payment-options-note">Payment methods remain manual records until a separately reviewed provider integration is configured. Do not enter provider secrets here.</p>
  </section>
}
