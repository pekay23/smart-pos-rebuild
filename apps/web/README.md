# Advanced POS web client

This is the browser-native React client for Advanced POS. It implements OIDC sign-in, authenticated tenant/branch/station discovery, branch catalog and payment-method management, station register checkout and cash drawer workflows, closed drawer reports, branch sales reports, cross-branch availability and transfers, cashier customer lookup and checkout association, manager customer and supplier directories, branch purchase-order drafting/placement/partial-final receiving, supplier invoice matching, payable balances, and permission-gated manual supplier-payment recording. These screens require a configured identity provider, provisioned tenant data, and the hosted API. Supplier payments create immutable balanced source-journal entries but only record an externally completed payment; they do not initiate or verify provider transfers and do not update the hosted drawer/cash-location balance. Broader financial statements remain incomplete until receipts, sales and expenses have full journal coverage. Customer merge, post-sale attachment, refunds, durable offline sync, subscriptions, and provider delivery are also not yet implemented. The separate SQLite desktop client includes supplier payment recording and local journal posting.

## Local setup

1. Copy `.env.example` to `.env.local` and enter the API URL, OIDC issuer URL, and public SPA client ID.
2. Register a public/User-Agent OIDC application with Authorization Code + PKCE. Add `http://localhost:5173/auth/callback` as a redirect URI, `http://localhost:5173` as a post-logout URI, and the local origin as an allowed origin.
3. Configure the API's issuer, audience, client and introspection settings to accept the access token issued to this SPA.
4. Run `npm ci`, then `npm run dev`.

The browser client contains no provider secret. It keeps OIDC user state in `sessionStorage`, disables silent renewal, and requires HTTPS for non-local issuer/API URLs. Configure the production issuer, client, API CORS origin, and registered Vercel redirect URLs before deployment.

## Build and deployment

Run `npm run build` to type-check and create the static bundle in `dist/`. `vercel.json` rewrites the OIDC callback route to the SPA entry point. Deploy the Rust API separately; it owns authorization and business data. Permission snapshots guide client navigation; each implemented API workflow also checks permission and tenant/location scope on the server. Not all product screens or mutations are available in the hosted product yet.

Each build also emits `build-info.json` with the app version and deployment build ID. The authenticated web workspace checks it without caching and offers a refresh when a newer deployment is live. Vercel's `vercel.json` keeps that metadata response uncached so open sessions can see new deployments.
