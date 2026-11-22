# Advanced POS accounts, payments and commercial platform

> Continuation source of truth: [`IMPLEMENTATION_TRACKER.md`](IMPLEMENTATION_TRACKER.md). This document retains commercial, identity, payment and licensing decisions.

Updated 31 October 2026. This is the implementation and release plan, not a claim that the cloud services or licensing portal already exist.

## Product decisions

Use a one-time **Local** licence for the purchased major version, plus monthly or annual **Cloud Team** and **Cloud Business** plans. Charge mainly by branch and active station capacity, not individual staff accounts. Personal accounts improve accountability; security, passkeys, account recovery, receipts, core reports and data export should be available at every paid level.

Do not sell unlimited lifetime cloud service. Hosting, backup storage, messaging and support have continuing costs. A perpetual local licence can keep the purchased version working, with clearly stated maintenance, compatibility and upgrade terms. Set actual prices after measuring hosting/support costs and interviewing Ghanaian retailers. No prices in this plan are promises.

| Capability | Public demo | Local perpetual | Cloud Team | Cloud Business |
| --- | --- | --- | --- | --- |
| Sales, stock, receipts, customers, basic reports | Sample business | Included | Included | Included |
| Personal accounts, passkeys, role controls, attendance | Included | Included | Included | Included |
| Manual bank-terminal / merchant payments | Simulation | Included | Included | Included |
| Local backup and readable export | Export sample data | Included | Included | Included |
| Web access, encrypted cloud backup, device recovery | Preview with sample data | Optional recurring add-on | Included | Included |
| Several tills in one branch, staff scheduling | Preview | Local station pack after LAN coordinator is built | Included up to purchased station capacity | Included up to purchased station capacity |
| Multiple branches, transfers, consolidated reporting | Preview | Not included | Limited branch add-on | Included up to purchased branch capacity |
| Provider integrations, API access, automation | Sandbox | Online add-on needed | Standard integrations | Advanced integrations and API quotas |
| Scheduled analytics, central policies, priority support | Preview | Optional maintenance | Standard | Advanced |

Square's current retail pricing separates Free, Plus and Premium capabilities. This supports testing a tiered offer, not copying its rates or payment economics into Ghana. See [Square retail pricing](https://squareup.com/us/en/point-of-sale/retail/pricing).

### Station and staff billing unit

One registered physical POS terminal is the billable station; named staff accounts are not individually metered. Multiple cashiers can use one station across shifts by signing into their own profiles, while only one active cashier/temporary-cashier actor may hold the station's transaction lease at a time. Count a second checkout device as another station even when both are in the same shop. Branches remain a separate organizational and pricing dimension, so a business with several shops manages them under one tenant and purchases the required active station capacity across those branches.

This choice matches established POS licensing patterns that charge for the operating footprint: Shopify POS Pro is priced per location, and Lightspeed documents register licenses with additional-register charges on applicable plans. These are evidence that location/register capacity is a familiar commercial meter, not evidence for copying either vendor's current rates or plan boundaries. See [Shopify POS pricing](https://www.shopify.com/pos/pricing), [Lightspeed register licenses](https://shopkeep-support.lightspeedhq.com/hc/en-us/articles/47480001936667-Adding-and-removing-register-licenses), and [Lightspeed additional-register pricing](https://x-series-support.lightspeedhq.com/hc/en-us/articles/25533904130587-Will-adding-registers-increase-my-bill).

## Current delivery and remaining work

| Work | Current scope |
| --- | --- |
| Windows passkeys | Native Windows WebAuthn registration/login/removal and per-account public credential storage; every role can use My account. Human hardware enrollment is a release acceptance gate. |
| Attendance | Persistent work periods, start on successful password/passkey login and staff switch, explicit end work, schedules, overnight periods, periodic/startup recovery, staff/manager views. Local OS time remains the authority in this build. |
| Payments | Store-configured manual payment methods with provider, merchant account, terminal ID, instructions and transaction reference; receipt metadata and backend validation. No real payment processor is connected. |
| Station capacity | Owner workspace displays effective licensed station capacity, linked devices and available slots. Staff accounts remain unmetered. Changing capacity or purchasing another station is not yet available in-app. |
| Operator portal and billing | Planned below; not deployed. |
| Signed activation, trials and subscriptions | Planned below; not implemented. |
| Cloud, LAN synchronization and consolidated branches | Planned below; local installations still have separate databases. |
| Cash reconciliation / X and Z reports | Separate financial workflow still to build. Ending attendance does not certify cash balance or close a drawer. |

## Passkeys and identity

The local Windows build uses `webauthn-rs` to generate and verify challenges, and the Windows WebAuthn API through `webauthn-authenticator-rs` for the authenticator ceremony. Challenges stay in Rust memory, expire after two minutes and are never accepted from frontend storage. Registration requires the account password again. Removal is restricted to the signed-in credential owner and requires password verification. Each account can have several named passkeys and a recovery password. Private keys remain in the authenticator.

The local relying party is `nimbletill.localhost`, a reserved localhost domain. Local credential records are installation-specific. A future cloud service must choose and retain its real HTTPS authentication domain before public enrollment, and verify challenges on that service. Do not pretend that enrolling locally automatically enables sign-in to every branch or the future web app.

On shared tills, default to a personal roaming security key or personal authenticator offered by Windows. Windows Hello on a shared Windows account does not distinguish individual employees who know that Windows PIN. The enrollment UI requires an explicit personal-Windows-account choice before allowing a platform authenticator. Add macOS AuthenticationServices, Android Credential Manager and standards-based web ceremonies in their platform phases; unsupported builds retain password sign-in. Store owners need at least two recovery methods before removing their last recovery route.

Cloud auth endpoints: `POST /auth/passkeys/registration/options`, `/registration/verify`, `/authentication/options`, `/authentication/verify`; authenticated `GET /me/passkeys` and `DELETE /me/passkeys/:id`. Bind registrations to tenant/user/session/re-authentication, enforce exact RP/origin, UV, expiry, challenge single use, credential ownership, rate limits and revocation. Use secure HttpOnly sessions and CSRF protection. Native cloud login should open the system browser and redeem a one-use authorization code with PKCE. Never distribute cloud signing credentials or provider secrets with a client.

References: [WebAuthn relying-party library](https://docs.rs/webauthn-rs/latest/webauthn_rs/struct.Webauthn.html), [Windows authenticator backend](https://docs.rs/webauthn-authenticator-rs/latest/webauthn_authenticator_rs/win10/struct.Win10.html).

## Work periods and financial shifts

Cashier login records actual arrival. Staff in a configured schedule also receive a period when they sign in during its active window; an unscheduled owner/admin/manager sign-in does not fabricate attendance. Repeated login, profile switching, logout, a crash or app restart must not create overlapping work periods. Staff can deliberately press Start work, and an owner/admin/manager enabling temporary cashier mode is also an explicit signal that work has begun. If that action occurs inside the saved schedule, retain its scheduled end; if it occurs outside the window, record the actual start and apply the 16-hour safety limit in the schedule's timezone. Turning temporary cashier mode off does not end attendance because the person may continue management work. A period ends through explicit End work, the saved scheduled end, or the safety limit; none of these actions closes a cash drawer.

An administrator assigns working days, start/end times and an IANA timezone per worker/store. Overnight shifts belong to the day they start. Existing periods snapshot their boundaries so later schedule edits cannot silently rewrite payroll history. Without a schedule, the local build uses a disclosed 16-hour safety limit. After a manual end, logging in again during the same period does not reopen it. A manager-approved overtime/reopen and correction flow with reason and append-only adjustments is a later task.

The backend periodically closes due records and repeats reconciliation at startup and attendance reads. If the PC was off at 17:00 and restarts at 09:00 tomorrow, the end remains 17:00. Records say whether the worker, schedule or safety limit closed them. Local wall-clock changes can still distort timestamps. Before using attendance for payroll, add trusted server time anchors, monotonic elapsed measurements while running, clock-jump detection, review flags, absence rules, breaks and approved corrections. Offline records need reconciliation; an entirely offline customer-controlled computer cannot provide tamper-proof payroll evidence.

Keep `work_period` separate from `cash_drawer_session`. A drawer needs opening float, cashier assignment, cash movements, expected cash, counted cash, variance, manager approval and an immutable Z report. Schedule end may lock or request cashier handover, but must not falsely declare the drawer balanced. Non-sensitive receipts show **Served by: first name**; internal sale rows and audit events retain the staff ID and full identity.

Acceptance: late login, early end, logout/login resume, overnight Friday-to-Saturday, no weekend start, PC sleep/offline past cutoff, repeated reconciliation, clock backwards/forwards, two simultaneous station logins and schedule changes mid-shift. In cloud mode enforce one open period per tenant/staff member transactionally. Offline duplicates must be flagged for review instead of summing their hours.

## Payments, merchant setup and bank terminals

Static inspection of `C:\Program Files\Smart POS\resources\app.asar` found merchant QR configuration for MTN, Telecel, AT, GhanaPay and custom providers. It stores merchant name/number, QR image, instructions and active state. The customer display signals provider selection; checkout sends the cashier's completed sale to its backend. These facts establish configurable payment presentation and recording. They do not establish automated bank settlement verification.

Evidence: archive SHA-256 `f4053e26b09bc968909bf629a85f7d4d86492d072a8c4b60b4670c26b951421b`; members `out/_next/static/chunks/5317-e47fad33866b3729.js`, `out/_next/static/chunks/app/settings/business/page-b64c8fbb8f35c431.js`, `out/_next/static/chunks/app/customer/order/page-16460dfd1f54b80b.js`, `out/_next/static/chunks/app/dashboard/sales/new/page-78753f3ddcefdf84.js`, `routes/sales.js`. The package was read without executing it or bypassing sign-in. Its minified/obfuscated backend and inaccessible authenticated runtime limit equivalence claims.

Tenant admins configure enabled methods per branch/station. Fields: name, cash/card/MoMo/transfer classification, country, currency, bank/acquirer/provider, terminal nickname, model and ID, merchant account, instructions, merchant-issued QR and reference policy. A country/provider picker checks the connector capability registry and shows whether the selected combination supports hosted checkout, payment status webhooks, refund, terminal amount push, or manual reference only. Never infer a merchant collection number from its general contact phone. Disable a method without deleting past receipt snapshots. Record cashier, amount, currency, provider, terminal, external reference and confirmation source per payment.

Current slice: branch managers can save country, provider, terminal model, requested capability, business impact and bounded detail through a hosted API with branch RLS, audit and idempotency. Credential/account-data markers are rejected, and managers can see a submitted status. No support contact, diagnostic attachment, operator delivery or provider action is implied; the separate operator identity and inbox are not connected yet.

When a tenant cannot find a country/provider/terminal combination, offer “Request an integration” from provider setup. The complete operator workflow should collect only the selected country, bank/acquirer or processor, terminal/model/firmware if known, currency, branch/station and affected workflow, impact, app/OS version, optional sanitized diagnostic bundle, and explicit consent for follow-up. Do not ask for API keys, card data, PINs, merchant passwords, customer data or full database exports. Create an `issues` record with type `payment_integration`, link the reporting tenant through `issue_tenant_links`, and record severity, status, reproduction details, assignee, due date, target release and visible updates in the operator Kanban. Let other tenants subscribe to an existing request without exposing the original tenant's identity. Keep credentials in a separate connector onboarding flow that uses a provider-hosted OAuth/tokenization path or encrypted server-side secret storage; never put secrets in issue comments or client logs.

Any bank POS terminal can be recorded manually: charge on the terminal, confirm approval, then record the reference in Advanced POS. Automatic amount push and result collection require the issuing bank/acquirer's approved ECR protocol, API or SDK for that exact terminal model. USB, Bluetooth or Wi-Fi availability alone does not imply integration support. Maintain a hardware registry with bank, model, firmware, protocol, supported operations and certification evidence. Test purchase, decline, cancellation, timeout, duplicate callback, reversal, refund and settlement reconciliation before enabling it for production.

For online providers, create server-side payment attempts with idempotency keys and immutable tenant/order/amount/currency binding. State machine: created → awaiting_customer → pending → succeeded/failed/cancelled, with separate refund/reversal records. A network timeout remains unknown/pending until reconciled. Never charge again merely because the client timed out. Validate signed webhooks on the raw body, deduplicate event IDs, verify provider transaction details and only then atomically complete sale/inventory. Client redirects, customer screenshots and terminal-event delivery are not payment approval. Store secrets in a server secret manager with encryption and rotation. Use hosted/tokenized provider collection so Advanced POS does not receive PAN, CVV, card PIN or MoMo PIN.

Paystack documents payment acceptance, signature-verified webhooks and terminal APIs. Its physical Terminal availability page currently says Nigeria, while Virtual Terminal has an official Ghana expansion. This is not evidence that an arbitrary Ghana bank-issued terminal supports Paystack's terminal API. Actual bank/model details and provider merchant onboarding are needed before connecting real money.

Sources: [Accept payments](https://paystack.com/docs/payments/accept-payments/), [Webhook verification](https://paystack.com/docs/payments/webhooks/), [Terminal API](https://paystack.com/docs/api/terminal/), [Physical Terminal availability](https://support.paystack.com/en/articles/2132994), [Virtual Terminal in Ghana](https://paystack.com/blog/product-engineering/virtual-terminal-expansion).

## Business, branch and station ownership

Model `tenant/business → branches → stations`, with staff memberships scoped independently to tenant, branch or station. Add an explicit owner role with ownership transfer and recovery. The current local administrator role is not yet a cross-business owner portal.

One owner can view all branches, assign branch administrators, manage station activations, configure policies and consolidate sales/stock/attendance. Cashiers operate only assigned branches and tills. No tenant identifier supplied by a client grants access: derive membership from the authenticated session and validate every referenced resource against it. Use PostgreSQL row-level security as defense in depth plus application authorization. Service-role database credentials stay server-side.

Each station is one registered physical POS terminal/device and consumes one licensed active-station slot. Staff accounts are independent of stations: do not charge per cashier, manager or admin seat. Several named employees can sign in or switch profiles on the same terminal across shifts; each sale, refund, drawer action and attendance period must identify the actual person. Only one active transaction actor may hold a station at a time. An owner or manager may browse/prepare a ticket, but checkout and financial drawer actions require cashier permission or that user's explicit, audited temporary-cashier mode. Profile switching must transfer the local actor context atomically and preserve the previous user's drafts without attributing another user's work to them.

Each station has a generated cryptographic identity. Bind activation to its public key and an opaque install ID, optionally TPM-backed. Hardware fingerprints can support fraud review but must not be the sole identity; NICs, disks and motherboards change. Offer owner-controlled device replacement and deactivation with audit and reasonable transfer limits. A business licence authorizes installed capacity; it does not replace each employee's login or convey administrator privileges to every user. Count active station activations transactionally against the tenant entitlement; creating a disabled station record or inviting another staff account must not consume another slot. The currently implemented local station list is for attribution/assignment only and is not yet a commercial activation or subscription check.

For hosted/multi-device use, acquire a short-lived station actor lease before a staff profile can transact. Store station, membership, actor mode, lease token hash, issue/heartbeat/expiry timestamps and revocation state server-side; enforce one unexpired lease per station with a database uniqueness/locking rule. Checkout and drawer mutations must validate the current lease in the same server transaction as the financial mutation. Heartbeats renew the lease; explicit profile switch/logout releases it. A crashed client becomes reclaimable after expiry, with a manager recovery action and audit trail. Offline local mode relies on the one installed device's local session and signed station entitlement; never imply that offline leases coordinate separate devices. On reconnect, report conflicting or stale lease attempts for review rather than merging two cashiers' sales silently.

Station capacity is the natural commercial meter: an owner can start with one terminal and buy more active station slots for additional tills or branches without buying more staff seats. Plans may bundle station capacity, but the entitlement catalog must expose a numeric `stations.active` limit independently of branch count, staff count, and staff permissions. Deactivation/replacement frees a slot only after server confirmation; downgrades require the owner to choose which activations remain active and must preserve sales/history on excess stations as read-only/exportable data.

Cloud sync uses a durable local outbox, immutable event IDs, per-device sequence/revision and idempotent server ingestion. Financial records are append-only; refunds/voids are new events. Prevent duplicate receipts across stations with a station namespace. Stock sales offline can exceed shared stock, so expose a clear offline policy and reconciliation queue. LAN sharing needs a local coordinator or authenticated service, never a SQLite database placed on a shared drive.

## Operator / developer console

Build a separate web application and auth audience for the operator console. Do not add hidden superadmin buttons to tenant clients. Operator users use mandatory passkeys/MFA, short sessions and explicit roles: support, billing, release manager and security administrator. Tenant accounts and operator sessions must coexist without privilege crossover.

Screens: tenant directory and business health; subscription/entitlement timeline; branch/station activation and replacement; billing/grace/refunds; incidents and reported feature problems; a payment integration request queue with country/provider/model filters and connector capability status; Kanban delivery board; release channels and staged rollout; integration/webhook health; consented diagnostics; audit search; feature flags; status announcements and service settings. Provider requests should be mergeable as duplicates, linked to connector work, prioritized by affected tenants and market, and updated with safe customer-visible progress.

Issue workflow: Inbox → Triaged → Scheduled → In progress → Review → QA → Release → Resolved, with blocked, duplicate and reopened transitions. Store severity, tenant impact, assignee, due date, reproduction steps, app/OS/version, branch/station, redacted diagnostic attachment, related feature, target release and customer-visible status. Link many reports to one defect. Keep customer communication, internal notes, scheduling and resolution evidence distinct. Dragging a card updates the same versioned issue record; concurrent edits must not silently overwrite each other.

Core server tables: `tenants`, `memberships`, `branches`, `stations`, `device_keys`, `plans`, `plan_versions`, `subscriptions`, `entitlement_grants`, `activation_codes`, `license_leases`, `billing_events`, `payment_providers`, `provider_capabilities`, `provider_connections`, `terminal_models`, `issues`, `issue_events`, `issue_tenant_links`, `releases`, `rollout_rules`, `support_access_grants`, `operator_audit_events`. Treat the provider registry as versioned, country-scoped capability data; `provider_connections` contain references to encrypted secrets, never plaintext secrets.

Support access requires a time-limited, tenant-approved grant with purpose, scope and audit. Default diagnostics exclude full databases, credentials and payment information. Break-glass access needs a distinct audited emergency policy. Operators cannot retrieve employee passkey private keys or plaintext passwords. Never add a universal support PIN.

## Demo distribution and activation

Ship a signed public demo with a separate sample business and visible Demo labels on screen and receipts. Simulated transactions must never create provider charges. Use a separate application ID/data directory or explicit isolated demo workspace. Public demo accounts are demonstration-only and never copied into real businesses. Starting a real business requires owner setup, fresh credentials and an explicit choice about importing product data; do not import sample financial history.

Offer a 14-day online-activated real-data trial as a distinct option. The server records its start; reinstalling cannot reset it. Issue a signed trial lease to the station key. Offline evaluation can be a fixed feature-limited demo rather than promising an unbreakable local countdown. After expiry keep records and export available; show purchase/activation steps and a clear policy before blocking new transactions.

An activation serial is a high-entropy, one-use redemption code stored hashed on the server. The owner redeems it to a tenant and station key. The server returns a signed licence payload containing licence ID, tenant, device public-key hash, plan/version, features, branch/station caps, issued/expiry time, offline validity, revocation epoch and signing-key ID. The app embeds only verification public keys. Never embed the licence-signing private key, a reversible master serial formula or a cloud admin credential.

Paid local perpetual licences identify the purchased version and explicit update rights. Cloud leases renew periodically, with a documented short offline grace window. Revocation reaches offline devices on next contact or lease expiry; instant offline revocation is impossible. Keep a signed last-seen time anchor and flag rollback. Do not erase data after expiry or suspension. Owner recovery and hardware replacement must remain practical.

## Entitlement enforcement

Use a versioned entitlement catalog shared by server and client with stable keys such as `pos.sell`, `cloud.sync`, `branches.manage`, `reports.consolidated`, `integrations.api`, plus numeric limits including `stations.active` and `branches.active`. Keep plan entitlements separate from staff permissions, station activation and resource ownership. A staff operation requires plan entitlement, active station activation, staff permission and resource ownership; checkout additionally requires the active station actor lease (cloud) or local cashier/temporary-cashier context (offline desktop). Do not make staff account count a paid limit.

Enforce cloud gates in every API, background job, export endpoint and sync ingestion path. Check limits transactionally when allocating stations/branches. Verify signed offline entitlements inside Rust before protected local operations; hiding a React route is only presentation. Assume customer-controlled desktop binaries can be patched: protect cloud resources and signing secrets on the server, and set realistic offline enforcement expectations.

Billing webhooks verify provider signatures, deduplicate IDs, compare stored subscription/customer IDs and update entitlements transactionally. Preserve an entitlement history and reconciliation job. Use states trial, active, past_due, grace, suspended and cancelled with explicit effective times. Downgrades preserve data and let owners select active stations/branches; they must not silently delete excess resources. Pin entitlements to plan versions so editing a plan cannot change existing contracts accidentally.

Test cross-tenant IDs, expired leases, device-key mismatch, forged signatures, clock rollback, replayed webhooks, concurrent activation, background-job bypass, direct API access to gated features, downgrade overflow and offline reconnect. Security/recovery fixes must not require buying a higher tier.

## Architecture and delivery sequence

Keep React/TypeScript for shared desktop/web UI and Rust for local transactions, native hardware and offline verification. Use a separately deployable Rust HTTP API with a versioned OpenAPI contract so hosted requests can share the Rust domain rules. Put hosted PostgreSQL behind the API. Neon or Supabase can support a development environment; production needs backup/restore tests, cost/quotas, availability and data-location review. Free limits can change and are not a production reliability commitment.

Vercel can host the web and operator interfaces. Keep the Rust API and durable queue/worker on an independently deployable runtime unless a Vercel Rust Functions spike proves the required database connections, latency, background work and execution limits. Use a durable scheduler for payment reconciliation, sync processing, automatic cloud attendance closure, billing and notification retries. Never depend on an open browser tab to perform those jobs. Separate development, test and production providers/databases/keys.

1. Finish and validate local passkeys, durable attendance, configurable manual payments and first-name receipts. Hardware enrollment and negative security tests are required before public release.
2. Implement drawer opening/closing, refunds, receipt printer hardware acceptance, backup/restore, diagnostic redaction, and staff recovery/overtime correction. Close current production-readiness gaps before public payment use.
3. Build the cloud identity/tenant API and migrations with authorization, RLS, idempotency and audit. Reserve the authentication domain. Deliver owner/branch/station management and conflict-aware sync.
4. Build operator portal, issue Kanban, subscription adapters, signed activation and entitlement enforcement. Test trial expiry and device replacement end to end.
5. Integrate approved payment providers and selected bank terminals in sandbox; complete merchant/provider certification and reconciliation acceptance before enabling live money.
6. Sign Windows installers and updater manifests, generate dependency/license inventory, establish backup/rollback and crash reporting, release an isolated public demo, then a limited real-business pilot.
7. Add web/macOS/Android clients and their native passkey/hardware adapters. HarmonyOS requires its own platform integration research and availability validation. Publish a tested device/OS matrix instead of promising all hardware compatibility.

Release channels: internal → pilot → stable. Require reviewed versioned migrations, reproducible build inputs, signed update metadata, per-version rollback constraints and a tested restore procedure. Keep release signing keys in a controlled signing service. An operator feature flag must not substitute for an entitlement check or silently bypass tenant authorization.
