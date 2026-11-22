// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState } from 'react'
import { Clock3, PackageCheck, RefreshCw, Store, X } from 'lucide-react'
import { listBranchHostedPickupReservations, listHostedPickupReservations, transitionHostedPickupReservation } from './api'
import type { HostedPickupReservation } from './api'
import './pickup-reservations.css'

type QueueScope = { kind: 'branch' } | { kind: 'station'; stationId: string }
type QueueState =
  | { kind: 'loading' }
  | { kind: 'ready'; reservations: HostedPickupReservation[] }
  | { kind: 'error'; message: string }

type Props = {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  branchId: string
  branchName: string
  membershipId: string
  canPrepare: boolean
  canCancel: boolean
  canManage: boolean
  scope: QueueScope
  onClose?: () => void
  onSellHeld?: (reservation: HostedPickupReservation) => void
}

function readableError(reason: unknown): string {
  return reason instanceof Error ? reason.message : 'Pickup reservations could not be loaded.'
}

function formatDateTime(value: string): string {
  const date = new Date(value)
  return Number.isFinite(date.getTime()) ? date.toLocaleString() : 'Time unavailable'
}

export function HostedPickupQueuePanel({ apiBaseUrl, accessToken, tenantId, branchId, branchName, membershipId, canPrepare, canCancel, canManage, scope, onClose, onSellHeld }: Props) {
  const [state, setState] = useState<QueueState>({ kind: 'loading' })
  const [refreshVersion, setRefreshVersion] = useState(0)
  const [busyId, setBusyId] = useState<string | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const stationId = scope.kind === 'station' ? scope.stationId : null

  useEffect(() => {
    let active = true
    setState({ kind: 'loading' })
    const request = stationId === null
      ? listBranchHostedPickupReservations({ apiBaseUrl, accessToken, tenantId, branchId })
      : listHostedPickupReservations({ apiBaseUrl, accessToken, tenantId, branchId, stationId })
    void request.then((reservations) => {
      if (active) setState({ kind: 'ready', reservations })
    }).catch((reason: unknown) => {
      if (active) setState({ kind: 'error', message: readableError(reason) })
    })
    return () => { active = false }
  }, [accessToken, apiBaseUrl, branchId, refreshVersion, stationId, tenantId])

  const reservations = state.kind === 'ready' ? state.reservations : []
  const activeCount = reservations.filter((reservation) => reservation.status === 'held' || reservation.status === 'ready').length

  async function transition(reservation: HostedPickupReservation, action: 'ready' | 'cancel') {
    let reason: string | null = null
    if (action === 'cancel') {
      reason = window.prompt('Reason for cancelling this pickup hold:')?.trim() ?? null
      if (!reason) return
    }
    setBusyId(reservation.reservationId)
    setActionError(null)
    try {
      await transitionHostedPickupReservation({
        apiBaseUrl, accessToken, tenantId, branchId, reservationId: reservation.reservationId,
        scope, action, expectedRevision: reservation.revision, reason,
        idempotencyKey: globalThis.crypto.randomUUID(),
      })
      setRefreshVersion((value) => value + 1)
    } catch (reason) {
      setActionError(readableError(reason))
    } finally {
      setBusyId(null)
    }
  }

  return <section className="pickup-queue" aria-labelledby="pickup-queue-title">
    <header className="pickup-queue-header">
      <div className="pickup-queue-title">
        <span className="pickup-queue-icon"><PackageCheck size={17}/></span>
        <div><p className="eyebrow">CUSTOMER PICKUPS</p><h2 id="pickup-queue-title">Pickup reservations</h2><p>{branchName}{scope.kind === 'station' ? ' · this station and destination branch' : ' · branch-wide queue'}</p></div>
      </div>
      <div className="pickup-queue-header-actions"><button className="button button-quiet button-small" type="button" onClick={() => setRefreshVersion((value) => value + 1)} disabled={state.kind === 'loading'}><RefreshCw size={14}/>{state.kind === 'loading' ? 'Refreshing…' : 'Refresh'}</button>{onClose && <button className="icon-button" type="button" onClick={onClose} aria-label="Close pickup queue"><X size={16}/></button>}</div>
    </header>

    {state.kind === 'loading' && <div className="pickup-queue-state"><span className="mini-spinner"/>Loading pickup reservations…</div>}
    {state.kind === 'error' && <div className="pickup-queue-state pickup-queue-error" role="alert">{state.message}<button className="button button-quiet button-small" type="button" onClick={() => setRefreshVersion((value) => value + 1)}>Try again</button></div>}
    {actionError && <div className="pickup-queue-state pickup-queue-error" role="alert">{actionError}<button className="button button-quiet button-small" type="button" onClick={() => setActionError(null)}>Dismiss</button></div>}
    {state.kind === 'ready' && <>
      <div className="pickup-queue-summary"><span><Store size={13}/>{activeCount} active hold{activeCount === 1 ? '' : 's'}</span><span><Clock3 size={13}/>Holds expire at the time shown</span></div>
      {reservations.length === 0 ? <div className="pickup-queue-empty">No pickup reservations are visible in this scope.</div> : <div className="pickup-reservation-list">
        {reservations.map((reservation) => <article className="pickup-reservation-card" key={reservation.reservationId}>
          <header className="pickup-reservation-card-header">
            <div><strong>Pickup {reservation.reservationId.slice(0, 8).toUpperCase()}</strong><small>From branch {reservation.originBranchId.slice(0, 8)} · requested {formatDateTime(reservation.createdAt)}</small></div>
            <span className={`pickup-status pickup-status-${reservation.status}`}>{reservation.status}</span>
          </header>
          <div className="pickup-reservation-customer"><strong>{reservation.contactName}</strong><a href={`tel:${reservation.contactPhone.replace(/[^\d+]/g, '')}`}>{reservation.contactPhone}</a></div>
          <ul className="pickup-reservation-items">{reservation.items.map((item) => <li key={item.productId}><span>{item.name}<small>{item.sku}</small></span><strong>× {item.quantity}</strong></li>)}</ul>
          <footer className="pickup-reservation-expiry"><Clock3 size={13}/>{reservation.status === 'expired' ? 'Expired' : reservation.status === 'cancelled' ? 'Cancelled' : `Hold expires ${formatDateTime(reservation.expiresAt)}`}</footer>
          {(reservation.status === 'held' || reservation.status === 'ready') && <div className="pickup-reservation-actions">
            {canPrepare && branchId === reservation.pickupBranchId && reservation.status === 'held' && <button className="button button-primary button-small" type="button" disabled={busyId !== null} onClick={() => void transition(reservation, 'ready')}>{busyId === reservation.reservationId ? 'Saving…' : 'Mark ready for pickup'}</button>}
            {onSellHeld && canPrepare && reservation.status === 'ready' && <button className="button button-primary button-small" type="button" disabled={busyId !== null} onClick={() => onSellHeld(reservation)}>Sell held items</button>}
            {(canManage || (canCancel && scope.kind === 'station' && branchId === reservation.originBranchId && scope.stationId === reservation.originStationId && membershipId === reservation.requestedByMembershipId)) && <button className="button button-quiet button-small" type="button" disabled={busyId !== null} onClick={() => void transition(reservation, 'cancel')}>{busyId === reservation.reservationId ? 'Saving…' : 'Cancel hold'}</button>}
          </div>}
        </article>)}
      </div>}
      <p className="pickup-queue-disclosure">Contact details are shown only to staff assigned to the originating or pickup branch. Status changes and sale completion are handled in the pickup workflow.</p>
    </>}
  </section>
}
