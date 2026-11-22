// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { UserManager, WebStorageStateStore } from 'oidc-client-ts'

export type OperatorAuthSetup =
  | { kind: 'ready'; manager: UserManager; apiBaseUrl: string }
  | { kind: 'missing'; names: string[] }
  | { kind: 'invalid'; message: string }

function isAllowedUrl(value: string): boolean {
  try {
    const url = new URL(value)
    return url.protocol === 'https:' || (
      url.protocol === 'http:' && ['localhost', '127.0.0.1'].includes(url.hostname)
    )
  } catch {
    return false
  }
}

export function createOperatorAuthSetup(): OperatorAuthSetup {
  const apiBaseUrl = import.meta.env.VITE_OPERATOR_API_BASE_URL?.trim()
  const authority = import.meta.env.VITE_OPERATOR_OIDC_AUTHORITY?.trim()
  const clientId = import.meta.env.VITE_OPERATOR_OIDC_CLIENT_ID?.trim()
  const missing = [
    !apiBaseUrl ? 'VITE_OPERATOR_API_BASE_URL' : '',
    !authority ? 'VITE_OPERATOR_OIDC_AUTHORITY' : '',
    !clientId ? 'VITE_OPERATOR_OIDC_CLIENT_ID' : '',
  ].filter(Boolean)

  if (missing.length) return { kind: 'missing', names: missing }
  if (!apiBaseUrl || !authority || !clientId) return { kind: 'missing', names: [] }
  if (!isAllowedUrl(apiBaseUrl)) {
    return { kind: 'invalid', message: 'The operator API URL must use HTTPS outside local development.' }
  }
  if (!isAllowedUrl(authority)) {
    return { kind: 'invalid', message: 'The operator identity provider URL must use HTTPS outside local development.' }
  }

  const manager = new UserManager({
    authority,
    client_id: clientId,
    redirect_uri: `${window.location.origin}/auth/callback`,
    post_logout_redirect_uri: window.location.origin,
    response_type: 'code',
    scope: 'openid profile email',
    userStore: new WebStorageStateStore({ store: window.sessionStorage }),
    automaticSilentRenew: false,
    monitorSession: false,
    loadUserInfo: false,
  })

  return { kind: 'ready', manager, apiBaseUrl: apiBaseUrl.replace(/\/+$/, '') }
}
