// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

export const AVAILABILITY_STALE_AFTER_MS = 2 * 60 * 1000

export function isAvailabilityStale(observedAt: string, nowMs: number): boolean {
  const observedAtMs = Date.parse(observedAt)
  return !Number.isFinite(observedAtMs)
    || observedAtMs > nowMs + 30_000
    || nowMs - observedAtMs > AVAILABILITY_STALE_AFTER_MS
}

export function formatObservedAt(observedAt: string): string {
  const timestamp = Date.parse(observedAt)
  return Number.isFinite(timestamp) ? new Date(timestamp).toLocaleString() : 'time unavailable'
}
