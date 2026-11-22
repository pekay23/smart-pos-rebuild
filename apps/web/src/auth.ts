// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { UserManager, WebStorageStateStore } from 'oidc-client-ts'

export type AuthSetup =
  | { kind: 'ready'; manager: UserManager; apiBaseUrl: string }
  | { kind: 'missing'; names: string[] }
  | { kind: 'invalid'; message: string }

function isAllowedWebUrl(value: string, allowLocalHttp: boolean): boolean {
  try {
    const url = new URL(value)
    return url.protocol === 'https:' || (allowLocalHttp && url.protocol === 'http:' && ['localhost', '127.0.0.1'].includes(url.hostname))
  } catch {
    return false
  }
}

export function createAuthSetup(): AuthSetup {
  const apiBaseUrl = import.meta.env.VITE_API_BASE_URL?.trim()
  const authority = import.meta.env.VITE_OIDC_AUTHORITY?.trim()
  const clientId = import.meta.env.VITE_OIDC_CLIENT_ID?.trim()
  const missing = [
    !apiBaseUrl ? 'VITE_API_BASE_URL' : '',
    !authority ? 'VITE_OIDC_AUTHORITY' : '',
    !clientId ? 'VITE_OIDC_CLIENT_ID' : '',
  ].filter((name) => name.length > 0)

  if (missing.length > 0) return { kind: 'missing', names: missing }
  if (!apiBaseUrl || !authority || !clientId) return { kind: 'missing', names: [] }
  if (!isAllowedWebUrl(apiBaseUrl, true)) {
    return { kind: 'invalid', message: 'The API URL must use HTTPS outside local development.' }
  }
  if (!isAllowedWebUrl(authority, true)) {
    return { kind: 'invalid', message: 'The identity provider URL must use HTTPS outside local development.' }
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
