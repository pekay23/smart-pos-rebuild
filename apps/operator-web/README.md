# Advanced POS Operator Desk

The operator desk is a separate Vite/React application for support staff. It is intentionally isolated from the tenant web client: it has a dedicated OIDC client, uses session storage for its short-lived browser session, and calls only the separate operator API at `/operator/v1/*`.

## Run locally

```powershell
Copy-Item .env.example .env
npm ci
npm run dev
```

Vite serves the app at `http://127.0.0.1:5174`. Configure the dedicated client in the operator identity provider with that callback URL (`/auth/callback`) and post-logout URL. Set `VITE_OPERATOR_API_BASE_URL` to the separate operator API origin, `VITE_OPERATOR_OIDC_AUTHORITY` to the operator identity provider, and `VITE_OPERATOR_OIDC_CLIENT_ID` to the operator-only client ID. Do not reuse the tenant API audience or tenant browser client.

The operator API also needs its own OIDC introspection settings, disjoint from tenant audiences/client IDs, a provisioned active operator account, and the `pos_operator_runtime` database credential. For local Compose, put the nonsecret OIDC settings and the outside-repository key file path in the untracked root `.env`, then run:

```powershell
docker compose -f docker-compose.yml -f docker-compose.operator-auth.yml up --build
```

The overlay refuses to start if operator and tenant identities/audiences or the private key file path are missing. The Compose API allows CORS from `127.0.0.1:5174` and `localhost:5174`. See [the API setup notes](../api/README.md#local-docker-development-stack).

## Implemented surface

- Operator identity verification against `/operator/v1/me` before showing the queue.
- Payment integration request queue with status, country and provider filters and cursor paging.
- Request context, tenant/branch identifiers, business impact, status and public progress.
- Revision-aware status updates, customer-visible progress and a separate private support note.
- Internal due dates on active requests, with revision-aware set/clear behavior and private audit-history entries.
- A Kanban-style payment request board grouped by server workflow status, with keyboard-accessible moves limited to permitted next stages.
- A separate internal support-issue Kanban with categories, severity, gated stage transitions, self-claim/unassign, internal due dates and target releases.
- Idempotent issue creation, primary/duplicate/related payment-request links and a private append-only activity history. Linked requests retain independent tenant-visible progress.
- Idempotency key per update, session expiry handling and confirmed sign-out.
- Responsive list/detail layout for desktop, tablet and phone.

Request and issue details/history are loaded from the restricted operator API. Due dates, issue stage, assignments, links and private history remain operator-only; each tenant sees only its own payment-request progress. The first assignment control lets an operator claim their own issues; a display-name-only team directory is still needed for assigning to colleagues. General tenant support intake, notifications, country/provider capability tracking, provider health, subscription controls and release management remain open. Configure and verify MFA/passkey policy in the identity provider before allowing real operator access. The browser application never receives the database or OIDC introspection credentials.
