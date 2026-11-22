// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState } from 'react'
import { Mail, MessageCircle } from 'lucide-react'

type ReceiptLine = { name: string; quantity: number; lineTotalMinor: number }
type HandoffChannel = 'whatsapp' | 'email'
type HandoffState = { kind: 'idle' } | { kind: 'opened'; channel: HandoffChannel }

type ReceiptShareActionsProps = {
  businessName: string
  stationName: string
  receiptNumber: string
  createdAt: string
  cashierName: string
  currencyCode: string
  subtotalMinor: number
  taxMinor: number
  totalMinor: number
  paymentLabel: string
  lines: ReceiptLine[]
  customerName: string | null
  customerPhone: string | null
  customerEmail: string | null
}

function formatMoney(currencyCode: string, amountMinor: number): string {
  return `${currencyCode} ${(amountMinor / 100).toFixed(2)}`
}

function receiptText(receipt: ReceiptShareActionsProps): string {
  const lines = receipt.lines.map((line) => `${line.quantity} × ${line.name} — ${formatMoney(receipt.currencyCode, line.lineTotalMinor)}`)
  return [
    receipt.businessName,
    `Receipt ${receipt.receiptNumber}`,
    `${new Date(receipt.createdAt).toLocaleString()} · ${receipt.stationName}`,
    ...(receipt.customerName ? [`Customer: ${receipt.customerName}`] : []),
    '',
    ...lines,
    '',
    `Subtotal: ${formatMoney(receipt.currencyCode, receipt.subtotalMinor)}`,
    `Tax: ${formatMoney(receipt.currencyCode, receipt.taxMinor)}`,
    `Total: ${formatMoney(receipt.currencyCode, receipt.totalMinor)}`,
    `Paid by: ${receipt.paymentLabel}`,
    `Served by: ${receipt.cashierName}`,
    'Thank you for shopping with us.',
  ].join('\n')
}

function whatsappNumber(value: string): string | null {
  const normalized = value.trim().replace(/[\s().-]/g, '')
  return /^\+[1-9]\d{7,14}$/.test(normalized) ? normalized.slice(1) : null
}

function validEmail(value: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value.trim())
}

export function ReceiptShareActions(props: ReceiptShareActionsProps) {
  const [phone, setPhone] = useState(props.customerPhone ?? '')
  const [email, setEmail] = useState(props.customerEmail ?? '')
  const [consentedChannels, setConsentedChannels] = useState<ReadonlySet<HandoffChannel>>(() => new Set())
  const [handoff, setHandoff] = useState<HandoffState>({ kind: 'idle' })

  useEffect(() => {
    setPhone(props.customerPhone ?? '')
    setEmail(props.customerEmail ?? '')
    setConsentedChannels(new Set())
    setHandoff({ kind: 'idle' })
  }, [props.receiptNumber, props.customerPhone, props.customerEmail])

  const message = receiptText({ ...props, customerName: props.customerName?.trim() || null })
  const number = whatsappNumber(phone)
  const emailAddress = email.trim()
  const whatsappHref = number ? `https://wa.me/${number}?text=${encodeURIComponent(message)}` : null
  const emailHref = validEmail(emailAddress)
    ? `mailto:${encodeURIComponent(emailAddress).replace(/%40/gi, '@')}?subject=${encodeURIComponent(`Receipt ${props.receiptNumber}`)}&body=${encodeURIComponent(message)}`
    : null

  return <section className="register-customer-form receipt-share-panel" aria-label="Send receipt">
    <strong>Send receipt</strong>
    <p>Enter the customer’s contact or use the saved details, then confirm they agreed to receive this receipt.</p>
    <label>WhatsApp phone<input type="tel" autoComplete="tel" maxLength={40} value={phone} onChange={(event) => { setPhone(event.currentTarget.value); setHandoff({ kind: 'idle' }) }} placeholder="+233 24 000 0000" aria-invalid={phone.length > 0 && number === null}/></label>
    {phone.length > 0 && !number && <small className="receipt-share-hint">Use international format with country code, such as +233…</small>}
    <label>Email<input type="email" autoComplete="email" maxLength={254} value={email} onChange={(event) => { setEmail(event.currentTarget.value); setHandoff({ kind: 'idle' }) }} placeholder="customer@example.com" aria-invalid={emailAddress.length > 0 && !validEmail(emailAddress)}/></label>
    {number && <label className="receipt-share-consent"><input type="checkbox" checked={consentedChannels.has('whatsapp')} onChange={(event) => { const checked = event.currentTarget.checked; setConsentedChannels((current) => { const next = new Set(current); if (checked) next.add('whatsapp'); else next.delete('whatsapp'); return next }); setHandoff({ kind: 'idle' }) }}/> The customer agreed to receive this receipt by WhatsApp.</label>}
    {emailHref && <label className="receipt-share-consent"><input type="checkbox" checked={consentedChannels.has('email')} onChange={(event) => { const checked = event.currentTarget.checked; setConsentedChannels((current) => { const next = new Set(current); if (checked) next.add('email'); else next.delete('email'); return next }); setHandoff({ kind: 'idle' }) }}/> The customer agreed to receive this receipt by email.</label>}
    <div className="receipt-share-buttons">
      {whatsappHref && <a className="button button-secondary button-small" href={consentedChannels.has('whatsapp') ? whatsappHref : undefined} target="_blank" rel="noopener noreferrer" aria-disabled={!consentedChannels.has('whatsapp')} onClick={(event) => { if (!consentedChannels.has('whatsapp')) { event.preventDefault(); return } setHandoff({ kind: 'opened', channel: 'whatsapp' }) }}><MessageCircle size={14}/>Open WhatsApp</a>}
      {emailHref && <a className="button button-secondary button-small" href={consentedChannels.has('email') ? emailHref : undefined} target="_blank" rel="noopener noreferrer" aria-disabled={!consentedChannels.has('email')} onClick={(event) => { if (!consentedChannels.has('email')) { event.preventDefault(); return } setHandoff({ kind: 'opened', channel: 'email' }) }}><Mail size={14}/>Open email draft</a>}
      {!whatsappHref && !emailHref && <span className="receipt-share-hint">Enter a valid WhatsApp number or email address.</span>}
    </div>
    <small className="receipt-share-hint">This opens a message draft. The cashier must confirm sending in WhatsApp or the email app; delivery is not verified here.</small>
    {handoff.kind === 'opened' && <p className="receipt-share-status" role="status">{handoff.channel === 'whatsapp' ? 'WhatsApp opened with the receipt draft.' : 'The email draft was opened.'} Confirm it was sent in that app.</p>}
  </section>
}
