// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

export type RequestStatus =
  | 'submitted'
  | 'under_review'
  | 'planned'
  | 'in_progress'
  | 'released'
  | 'not_planned'

export type PaymentIntegrationRequest = {
  id: string
  tenantId: string
  branchId: string
  countryCode: string
  providerName: string
  terminalModel: string | null
  capability: string
  businessImpact: 'business_blocked' | 'workaround_available' | 'exploring'
  details: string | null
  status: RequestStatus
  customerVisibleUpdate: string | null
  dueAt: string | null
  revision: number
  createdAt: string
  updatedAt: string
}

export type QueuePage = { items: PaymentIntegrationRequest[]; nextCursor: string | null }
export type RequestEvent = {
  id: string
  eventKind: 'status_changed' | 'public_update' | 'private_note' | 'schedule_changed'
  previousStatus: RequestStatus | null
  newStatus: RequestStatus | null
  customerVisibleUpdate: string | null
  privateNote: string | null
  dueAt: string | null
  resultingRevision: number
  createdAt: string
}
export type RequestEventPage = { items: RequestEvent[]; nextCursor: string | null }
export type QueueFilters = { status: string; country: string; provider: string; cursor?: string | null }

export type IssueStatus = 'inbox' | 'triaged' | 'scheduled' | 'in_progress' | 'review' | 'qa' | 'release' | 'resolved' | 'blocked' | 'duplicate' | 'reopened'
export type IssueCategory = 'bug' | 'feature_request' | 'payment_integration' | 'account_access' | 'data_issue' | 'other'
export type IssueSeverity = 'critical' | 'high' | 'normal' | 'low'
export type SupportIssue = {
  id: string
  title: string
  category: IssueCategory
  severity: IssueSeverity
  status: IssueStatus
  assigneeOperatorId: string | null
  assigneeOperatorName: string | null
  dueAt: string | null
  targetRelease: string | null
  revision: number
  linkedRequestCount: number
  createdAt: string
  updatedAt: string
}
export type SupportIssuePage = { items: SupportIssue[]; nextCursor: string | null }
export type OperatorDirectoryEntry = { id: string; displayName: string }
export type SupportIssueEvent = {
  id: string
  eventKind: 'created' | 'updated' | 'status_changed' | 'assignment_changed' | 'schedule_changed' | 'private_note' | 'payment_request_linked' | 'support_request_linked'
  previousStatus: IssueStatus | null
  newStatus: IssueStatus | null
  previousAssigneeOperatorId: string | null
  newAssigneeOperatorId: string | null
  dueAt: string | null
  targetRelease: string | null
  privateNote: string | null
  linkedRequestId: string | null
  linkedSupportRequestId: string | null
  resultingRevision: number
  createdAt: string
}
export type SupportIssueEventPage = { items: SupportIssueEvent[]; nextCursor: string | null }
export type OperatorSupportRequest = {
  id: string
  tenantId: string
  branchId: string
  category: 'bug' | 'feature_request' | 'account_access' | 'data_issue' | 'training' | 'other'
  title: string
  details: string | null
  diagnosticsConsent: boolean
  diagnosticsAppVersion: string | null
  diagnosticsPlatform: 'web' | 'windows' | 'macos' | 'linux' | 'android' | 'ios' | 'harmonyos' | null
  status: RequestStatus
  customerVisibleUpdate: string | null
  revision: number
  createdAt: string
  updatedAt: string
}
export type OperatorSupportRequestEvent = {
  id: string
  eventKind: 'public_update' | 'private_note'
  previousStatus: RequestStatus | null
  newStatus: RequestStatus | null
  customerVisibleUpdate: string | null
  privateNote: string | null
  resultingRevision: number
  createdAt: string
}
export type OperatorSupportRequestEventPage = { items: OperatorSupportRequestEvent[]; nextCursor: string | null }

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function readString(value: Record<string, unknown>, key: string): string {
  const field = value[key]
  if (typeof field !== 'string') throw new Error(`The operator API returned an invalid ${key} value.`)
  return field
}

function readNullableString(value: Record<string, unknown>, key: string): string | null {
  const field = value[key]
  if (field === null) return null
  if (typeof field !== 'string') throw new Error(`The operator API returned an invalid ${key} value.`)
  return field
}

function readNumber(value: Record<string, unknown>, key: string): number {
  const field = value[key]
  if (typeof field !== 'number' || !Number.isInteger(field)) throw new Error(`The operator API returned an invalid ${key} value.`)
  return field
}

function readBoolean(value: Record<string, unknown>, key: string): boolean {
  const field = value[key]
  if (typeof field !== 'boolean') throw new Error(`The operator API returned an invalid ${key} value.`)
  return field
}

function isSupportRequestCategory(value: string): value is OperatorSupportRequest['category'] {
  return ['bug', 'feature_request', 'account_access', 'data_issue', 'training', 'other'].includes(value)
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

function parseOperatorSupportRequest(value: unknown): OperatorSupportRequest {
  if (!isRecord(value)) throw new Error('The operator API returned an invalid support request.')
  const category = readString(value, 'category')
  const status = readString(value, 'status')
  const diagnosticsPlatform = readNullableString(value, 'diagnosticsPlatform')
  if (!isSupportRequestCategory(category)) {
    throw new Error('The operator API returned an unsupported support request category.')
  }
  if (!isRequestStatus(status)) {
    throw new Error('The operator API returned an unsupported support request status.')
  }
  if (diagnosticsPlatform !== null && !['web', 'windows', 'macos', 'linux', 'android', 'ios', 'harmonyos'].includes(diagnosticsPlatform)) {
    throw new Error('The operator API returned an unsupported diagnostics platform.')
  }
  return {
    id: readString(value, 'id'),
    tenantId: readString(value, 'tenantId'),
    branchId: readString(value, 'branchId'),
    category,
    title: readString(value, 'title'),
    details: readNullableString(value, 'details'),
    diagnosticsConsent: readBoolean(value, 'diagnosticsConsent'),
    diagnosticsAppVersion: readNullableString(value, 'diagnosticsAppVersion'),
    diagnosticsPlatform: diagnosticsPlatform as OperatorSupportRequest['diagnosticsPlatform'],
    status,
    customerVisibleUpdate: readNullableString(value, 'customerVisibleUpdate'),
    revision: readNumber(value, 'revision'),
    createdAt: readString(value, 'createdAt'),
    updatedAt: readString(value, 'updatedAt'),
  }
}

export class OperatorApiError extends Error {
  readonly status: number

  constructor(message: string, status: number) {
    super(message)
    this.status = status
    this.name = 'OperatorApiError'
  }
}

async function request<T>(input: {
  apiBaseUrl: string
  accessToken: string
  path: string
  method?: 'GET' | 'PATCH' | 'POST'
  idempotencyKey?: string
  body?: unknown
}): Promise<T> {
  let response: Response
  try {
    response = await fetch(`${input.apiBaseUrl}${input.path}`, {
      method: input.method ?? 'GET',
      headers: {
        Authorization: `Bearer ${input.accessToken}`,
        ...(input.body === undefined ? {} : { 'Content-Type': 'application/json' }),
        ...(input.idempotencyKey ? { 'Idempotency-Key': input.idempotencyKey } : {}),
      },
      body: input.body === undefined ? undefined : JSON.stringify(input.body),
      cache: 'no-store',
    })
  } catch {
    throw new OperatorApiError('The operator service could not be reached. Check the connection and try again.', 0)
  }

  if (!response.ok) {
    let message = response.status === 401
      ? 'Your operator session expired. Sign in again.'
      : response.status === 403
        ? 'This identity does not have active operator access.'
        : 'The operator request could not be completed.'
    try {
      const payload = await response.json() as { error?: { message?: string } }
      if (payload.error?.message) message = payload.error.message
    } catch {
      // Use the stable status-specific message when the service body is not JSON.
    }
    throw new OperatorApiError(message, response.status)
  }

  return await response.json() as T
}

export function getOperatorProfile(input: { apiBaseUrl: string; accessToken: string }) {
  return request<{ id: string; role: string; displayName: string }>({ ...input, path: '/operator/v1/me' })
}

export function listOperatorDirectory(input: { apiBaseUrl: string; accessToken: string }) {
  return request<{ items: OperatorDirectoryEntry[] }>({ ...input, path: '/operator/v1/operators' })
}

export function listPaymentRequests(input: {
  apiBaseUrl: string
  accessToken: string
  filters: QueueFilters
}) {
  const query = new URLSearchParams({ limit: '50' })
  if (input.filters.status) query.set('status', input.filters.status)
  if (input.filters.country) query.set('country', input.filters.country)
  if (input.filters.provider.trim()) query.set('provider', input.filters.provider.trim())
  if (input.filters.cursor) query.set('cursor', input.filters.cursor)
  return request<QueuePage>({
    apiBaseUrl: input.apiBaseUrl,
    accessToken: input.accessToken,
    path: `/operator/v1/payment-integration-requests?${query}`,
  })
}

export function updatePaymentRequest(input: {
  apiBaseUrl: string
  accessToken: string
  requestId: string
  expectedRevision: number
  status: RequestStatus
  customerVisibleUpdate: string
  privateNote: string
  dueAt?: string | null
}) {
  return request<PaymentIntegrationRequest>({
    apiBaseUrl: input.apiBaseUrl,
    accessToken: input.accessToken,
    path: `/operator/v1/payment-integration-requests/${encodeURIComponent(input.requestId)}`,
    method: 'PATCH',
    idempotencyKey: crypto.randomUUID(),
    body: {
      expectedRevision: input.expectedRevision,
      status: input.status,
      customerVisibleUpdate: input.customerVisibleUpdate.trim() || null,
      privateNote: input.privateNote.trim() || null,
      ...(input.dueAt === undefined ? {} : input.dueAt === null ? { clearDueAt: true } : { dueAt: input.dueAt }),
    },
  })
}

export function listRequestEvents(input: {
  apiBaseUrl: string
  accessToken: string
  requestId: string
  cursor?: string | null
}) {
  const query = new URLSearchParams({ limit: '50' })
  if (input.cursor) query.set('cursor', input.cursor)
  return request<RequestEventPage>({
    apiBaseUrl: input.apiBaseUrl,
    accessToken: input.accessToken,
    path: `/operator/v1/payment-integration-requests/${encodeURIComponent(input.requestId)}/events?${query}`,
  })
}

export function listSupportIssues(input: {
  apiBaseUrl: string
  accessToken: string
  status?: IssueStatus | ''
  category?: IssueCategory | ''
  cursor?: string | null
}) {
  const query = new URLSearchParams({ limit: '50' })
  if (input.status) query.set('status', input.status)
  if (input.category) query.set('category', input.category)
  if (input.cursor) query.set('cursor', input.cursor)
  return request<SupportIssuePage>({ ...input, path: `/operator/v1/issues?${query}` })
}

export function listOperatorSupportRequests(input: {
  apiBaseUrl: string
  accessToken: string
  status?: RequestStatus | ''
  category?: OperatorSupportRequest['category'] | ''
  cursor?: string | null
}) {
  const query = new URLSearchParams({ limit: '50' })
  if (input.status) query.set('status', input.status)
  if (input.category) query.set('category', input.category)
  if (input.cursor) query.set('cursor', input.cursor)
  return request<unknown>({ ...input, path: `/operator/v1/support-requests?${query}` }).then((value) => {
    if (!isRecord(value) || !Array.isArray(value.items)) throw new Error('The operator API returned an invalid support request page.')
    const nextCursor = value.nextCursor
    if (nextCursor !== null && typeof nextCursor !== 'string') throw new Error('The operator API returned an invalid support request cursor.')
    return { items: value.items.map(parseOperatorSupportRequest), nextCursor }
  })
}

export function createSupportIssue(input: {
  apiBaseUrl: string
  accessToken: string
  title: string
  category: IssueCategory
  severity: IssueSeverity
  source?: { kind: 'payment_request'; id: string } | { kind: 'support_request'; id: string }
}) {
  return request<SupportIssue>({
    ...input,
    path: '/operator/v1/issues',
    method: 'POST',
    idempotencyKey: crypto.randomUUID(),
    body: {
      title: input.title.trim(),
      category: input.category,
      severity: input.severity,
      ...(input.source?.kind === 'payment_request' ? { paymentRequestId: input.source.id } : {}),
      ...(input.source?.kind === 'support_request' ? { supportRequestId: input.source.id } : {}),
    },
  })
}

export function updateSupportIssue(input: {
  apiBaseUrl: string
  accessToken: string
  issueId: string
  expectedRevision: number
  status?: IssueStatus
  title?: string
  category?: IssueCategory
  severity?: IssueSeverity
  assignment?: 'claim' | 'unassign'
  assigneeOperatorId?: string
  dueAt?: string | null
  targetRelease?: string | null
  privateNote?: string
}) {
  return request<SupportIssue>({
    ...input,
    path: `/operator/v1/issues/${encodeURIComponent(input.issueId)}`,
    method: 'PATCH',
    idempotencyKey: crypto.randomUUID(),
    body: {
      expectedRevision: input.expectedRevision,
      ...(input.status ? { status: input.status } : {}),
      ...(input.title !== undefined ? { title: input.title } : {}),
      ...(input.category ? { category: input.category } : {}),
      ...(input.severity ? { severity: input.severity } : {}),
      ...(input.assignment ? { assignment: input.assignment } : {}),
      ...(input.assigneeOperatorId ? { assigneeOperatorId: input.assigneeOperatorId } : {}),
      ...(input.dueAt === undefined ? {} : input.dueAt === null ? { clearDueAt: true } : { dueAt: input.dueAt }),
      ...(input.targetRelease === undefined ? {} : input.targetRelease === null ? { clearTargetRelease: true } : { targetRelease: input.targetRelease }),
      ...(input.privateNote ? { privateNote: input.privateNote } : {}),
    },
  })
}

export function linkPaymentRequestToIssue(input: {
  apiBaseUrl: string
  accessToken: string
  issueId: string
  requestId: string
  expectedRevision: number
  relationship: 'duplicate' | 'related'
}) {
  return request<SupportIssue>({
    ...input,
    path: `/operator/v1/issues/${encodeURIComponent(input.issueId)}/payment-requests/${encodeURIComponent(input.requestId)}`,
    method: 'POST',
    idempotencyKey: crypto.randomUUID(),
    body: { expectedRevision: input.expectedRevision, relationship: input.relationship },
  })
}

export function linkSupportRequestToIssue(input: {
  apiBaseUrl: string
  accessToken: string
  issueId: string
  requestId: string
  expectedRevision: number
  relationship: 'duplicate' | 'related'
}) {
  return request<SupportIssue>({
    ...input,
    path: `/operator/v1/issues/${encodeURIComponent(input.issueId)}/support-requests/${encodeURIComponent(input.requestId)}`,
    method: 'POST',
    idempotencyKey: crypto.randomUUID(),
    body: { expectedRevision: input.expectedRevision, relationship: input.relationship },
  })
}

export function updateOperatorSupportRequest(input: {
  apiBaseUrl: string
  accessToken: string
  requestId: string
  expectedRevision: number
  status: RequestStatus
  customerVisibleUpdate: string
  privateNote: string
}) {
  return request<OperatorSupportRequest>({
    ...input,
    path: `/operator/v1/support-requests/${encodeURIComponent(input.requestId)}`,
    method: 'PATCH',
    idempotencyKey: crypto.randomUUID(),
    body: {
      expectedRevision: input.expectedRevision,
      status: input.status,
      customerVisibleUpdate: input.customerVisibleUpdate.trim() || null,
      privateNote: input.privateNote.trim() || null,
    },
  })
}

export function listOperatorSupportRequestEvents(input: {
  apiBaseUrl: string
  accessToken: string
  requestId: string
  cursor?: string | null
}) {
  const query = new URLSearchParams({ limit: '50' })
  if (input.cursor) query.set('cursor', input.cursor)
  return request<OperatorSupportRequestEventPage>({
    ...input,
    path: `/operator/v1/support-requests/${encodeURIComponent(input.requestId)}/events?${query}`,
  })
}

export function listSupportIssueEvents(input: {
  apiBaseUrl: string
  accessToken: string
  issueId: string
  cursor?: string | null
}) {
  const query = new URLSearchParams({ limit: '50' })
  if (input.cursor) query.set('cursor', input.cursor)
  return request<SupportIssueEventPage>({
    ...input,
    path: `/operator/v1/issues/${encodeURIComponent(input.issueId)}/events?${query}`,
  })
}

export function nextIssueStatuses(status: IssueStatus): IssueStatus[] {
  const transitions: Record<IssueStatus, IssueStatus[]> = {
    inbox: ['triaged', 'blocked', 'duplicate'],
    triaged: ['scheduled', 'in_progress', 'blocked', 'duplicate'],
    scheduled: ['in_progress', 'blocked', 'duplicate'],
    in_progress: ['review', 'blocked', 'duplicate'],
    review: ['qa', 'in_progress', 'blocked'],
    qa: ['release', 'in_progress', 'blocked'],
    release: ['resolved', 'in_progress'],
    resolved: ['reopened'],
    blocked: ['triaged', 'scheduled', 'in_progress', 'duplicate'],
    duplicate: ['triaged'],
    reopened: ['triaged', 'in_progress', 'blocked', 'duplicate'],
  }
  return transitions[status]
}

export function nextStatuses(status: RequestStatus): RequestStatus[] {
  const transitions: Record<RequestStatus, RequestStatus[]> = {
    submitted: ['under_review', 'not_planned'],
    under_review: ['planned', 'in_progress', 'not_planned'],
    planned: ['in_progress', 'not_planned'],
    in_progress: ['released', 'not_planned'],
    released: [],
    not_planned: [],
  }
  return transitions[status]
}
