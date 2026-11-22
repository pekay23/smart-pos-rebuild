# Auth council: product and cashier workflow review

**Reviewer:** Product and cashier workflow architect  
**Scope:** Compare the four proposed cloud identity shapes against staff onboarding, shared tills, account recovery, passkeys, branches, and the multi-client roadmap. This is an independent product review, not a security implementation review.

## Product constraints taken from planning

The POS already models named staff, role-scoped capabilities, cashier shifts, PIN-backed approval, audit actors, and shared catalog/till operations. The business is the tenant across branches; tills are shared devices. The cloud plan is API-first with distinct native clients, including a native HarmonyOS ArkTS/ArkUI client. Local SQLite accounts/passkeys are installation-scoped and must not become cloud credentials. These constraints favor a durable human identity per staff member, with a separate short cashier unlock/switch mechanism at the till.

A passkey is a strong account credential, but it is not a sensible per-sale reauthentication step. Staff should authenticate to their named cloud account at sign-in/shift start, then use a quick, individually attributable till profile switch (PIN or device-bound unlock) with explicit locking, timeout, and audit boundaries. That local unlock must not silently mint or replace the cloud identity session.

## Comparative score

Scores are product/workflow fit out of 5, with 5 best. They assume a Rust API remains the authorization and tenant-policy boundary for every option.

| Shape | Staff without email | Passkey coverage and native clients | Shared till / quick switch fit | Recovery and owner control | Branch / tenant fit | Overall |
|---|---:|---:|---:|---:|---:|---:|
| ZITADEL OIDC + Rust API/Postgres | 3 | 4 | 3 | 3 | 4 | **17/25** |
| Supabase Auth/Postgres | 2 | 3 | 3 | 3 | 4 | **15/25** |
| Clerk + Neon/Rust | 2 | 3 | 3 | 4 | 3 | **15/25** |
| Self-hosted Better Auth + Postgres/Rust | 4* | 4 | 4 | 2* | 4 | **18/25** |

\*Better Auth's username plugin still documents email as required for its standard signup flow, so email-less users require an explicitly designed custom account-provisioning path. Its recovery and operations burden is ours. Scores assume that custom path is acceptable and tested.

## Assessment by shape

### 1. ZITADEL OIDC + Rust API/Postgres

**Workflow fit:** Strongest managed identity match for the owner/manager side and a reasonable fit for individual staff. ZITADEL supports local accounts by username, password, MFA, and passkeys; its hosted login offers password reset and passkey fallback. It also supports user provisioning through APIs and an organization-oriented identity model. However, the documented reset flow sends an email to the verified address, so an employee with no email cannot self-recover. Enrollment and recovery need an owner/manager assisted path.

**Passkeys:** Supported, but the documented login flow asks for the login name before using a passkey; this is not the frictionless discoverable-account picker some cashiers may expect. Domain binding means the relying-party domain and hosted/custom login origin need an early stable decision across web and native apps. ZITADEL's hosted UI can reduce implementation burden but may be visually and operationally awkward as an embedded shared-till unlock flow.

**Recommendation within shape:** Use ZITADEL for cloud identity and OIDC tokens, but keep staff/store/branch memberships, permission assignments, shifts, till profiles, and their quick-switch PINs in the application database. Require passkeys for owners/managers first; phase staff enrollment with supervised recovery and a documented no-email exception until native flows have been proven.

### 2. Supabase Auth/Postgres

**Workflow fit:** Fast path to managed auth plus the same Postgres platform, with common password/email/phone flows. Passkeys are now documented but explicitly experimental. Registration requires an existing confirmed, non-anonymous user; passkey sign-in also requires confirmed email or phone and an unbanned account. That makes the default account lifecycle a poor fit for invited staff who lack personal email or phone. A custom username or staff-code enrollment flow could bridge this, but would become product-owned auth glue around Supabase.

**Passkeys/native:** The docs describe browser WebAuthn plus native SDK support for web, iOS, Android and macOS, with an API for custom native ceremonies. Supabase documents Android app origins but only five allowed origins. HarmonyOS support is not established by these docs, so it must be verified with the native ArkTS client before making passkeys a mandatory sign-in gate. Changing RP ID invalidates all registered passkeys, making the RP choice a launch decision.

**Recovery:** Recovery is tied to confirmed contact channels in ordinary flows. Managers need a safe assisted recovery route for staff who lose access to both device and contact method.

### 3. Clerk + Neon/Rust

**Workflow fit:** Polished hosted and native auth experience; Clerk documents usernames and passkeys. But users can only create a passkey after signing up, so at least one other sign-up strategy is still needed. Usernames are Latin-only, and passkeys are a paid-plan production feature. This creates both a procurement dependency and an awkward email-less path unless a manager provisions accounts using a supported alternate identifier.

**Passkeys/native:** Clerk describes Android/Expo setup and browser compatibility constraints; docs mention native app support. HarmonyOS support is not established. The passkey RP domain and domain restrictions matter for branch-specific/custom tenant domains. Clerk can deliver a strong owner/manager experience, but there is no documented POS-style cashier quick-switch concept; that remains application-owned.

**Neon Auth limitation:** Neon Auth is a Better Auth-backed integrated service, but its documented user creation API requires an email. It should not be treated as a drop-in username-only staff directory. The API-first Rust service also means the backend still needs to verify Clerk-issued tokens and resolve its own business/branch authorization. This shape has two provider surfaces (Clerk and Neon) without an obvious cashier workflow advantage over ZITADEL.

### 4. Self-hosted Better Auth + Postgres/Rust

**Workflow fit:** Most adaptable to the POS's named users, organization membership, multi-session needs, and API boundary. Better Auth has passkey, username, organization, and multi-session plugins. However, the standard username plugin augments email/password authentication and explicitly requires email during signup. To support employees with no email, we must implement and own a carefully constrained staff-invite/provisioning flow, account recovery, and anti-enumeration behavior rather than assuming the username plugin solves it.

**Passkeys/native:** The passkey plugin supports WebAuthn and custom authenticator options, but does not by itself prove HarmonyOS compatibility. Better Auth is TypeScript; it would be a separate auth service or an additional JS runtime alongside the Rust API. Native clients will need a stable token/session contract and native WebAuthn integration. This is the most flexible shape, but also the largest long-term maintenance and operational commitment.

**Recovery:** Self-hosted means we own delivery, abuse controls, account recovery policy, incident response, and migration behavior. Email-only reset reproduces the exact problem for email-less cashiers. Build owner-mediated reset/re-enrollment and recovery codes/device replacement before allowing a passkey-only account.

## Independent recommendation

**Choose ZITADEL OIDC + Rust API/Postgres for the first cloud identity provider**, with an application-owned staff and till model. Its standards-based OIDC boundary suits web plus multiple native clients, and the documented username/password/passkey options align better with organization-managed staff accounts than the default email-first workflows in the other managed choices. This recommendation is conditional: first verify that ZITADEL can provision a human user without requiring a staff-owned email, and prove native passkey ceremony support on Windows desktop, Android, iOS, and HarmonyOS.

The product should distinguish three things clearly:

1. **Person identity:** one durable named cloud identity per owner, manager, and cashier. Never shared cashier credentials.
2. **Client session:** OIDC session/token per client/device, revocable on lost devices and staff deactivation.
3. **Till profile unlock:** fast per-person switch on a shared register, linked to that identity and shift. PIN is a local convenience/step-up mechanism, not a substitute for the passkey requirement on account sign-in or for backend authorization.

To honor “passkeys for every account,” make passkey enrollment an onboarding requirement for every human account before the account becomes active for cloud clients. Support invitation/setup via a manager-controlled, expiring one-time enrollment code or verified recovery channel so employees without email can enroll. Provide at least one recoverable second passkey or owner-mediated recovery process before declaring enrollment complete. Do not make a shared till require a fresh platform passkey ceremony on every cashier switch; make it require passkeys at identity/session establishment and use the individual quick-unlock path on the till.

## Unresolved product questions for the decision owner

- Does “passkeys for every account” mean every person must register one, or that every account must support passkey sign-in while retaining a recovery method? The latter is safer operationally for device loss and older hardware.
- Will every cashier have a personal phone/security key, or must a cashier be able to enroll and authenticate with only the shared register? The answer changes whether device-bound passkeys can be required.
- Who verifies that an email-less employee is the intended invitee: owner, manager, in-person code, phone, or hardware key? Shared inboxes should not be used as personal recovery identity.
- Should employee accounts be globally unique across all branches of one business, with branch membership/permissions assigned separately? This best fits one owner across branches and named audit actors.
- Can the platform accept a passkey synced through a personal password manager, or must a business issue managed security keys/devices?
- Which HarmonyOS NEXT/ArkTS WebAuthn APIs and OIDC browser handoff are available on the supported device fleet? Vendor-specific validation is required before “mandatory on every account” is a release claim.
- What happens on a fully offline shared till when cloud token refresh expires? Define allowed local work, visible identity freshness, and queued audit attribution separately from cloud login.
- Is a second passkey, recovery code, or verified manager-assisted reset required before a cashier can complete enrollment?

## Primary sources

- ZITADEL hosted login and reset/passkey behavior: https://zitadel.com/docs/guides/integrate/login/hosted-login
- ZITADEL user onboarding and local username/passkey options: https://zitadel.com/docs/guides/integrate/onboarding/end-users
- ZITADEL create user and passkey enrollment link: https://zitadel.com/docs/guides/manage/user/reg-create-user
- ZITADEL passkey custom login requirements: https://zitadel.com/docs/guides/integrate/login-ui/passkey
- Supabase passkey status, confirmation requirements, native ceremonies, RP limits: https://supabase.com/docs/guides/auth/passkeys
- Supabase Swift native passkey availability: https://supabase.com/docs/reference/swift/auth-signinwithpasskey
- Clerk sign-up strategies, usernames, passkeys, and limitations: https://clerk.com/docs/guides/configure/auth-strategies/sign-up-sign-in-options
- Better Auth username plugin and required email on signup: https://better-auth.com/docs/plugins/username
- Better Auth passkey plugin: https://better-auth.com/docs/1.6/plugins/passkey
- Neon Auth integration and supported provider: https://api-docs.neon.tech/reference/createneonauth
- Neon Auth user creation requires email: https://api-docs.neon.tech/reference/createbranchneonauthnewuser

