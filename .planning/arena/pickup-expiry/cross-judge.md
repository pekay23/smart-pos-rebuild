# Pickup expiry design cross-judge

Reviewed the security, integrity, and operations candidates against the existing pickup locking protocol, forced-RLS model, API command layout, and Compose provisioning flow.

## Decision

Use a **dedicated scheduled worker command/container with an execute-only database login**, invoking a single database-owned, RLS-constrained transition function. The API process must not run the expiry loop under its general runtime credential. Keep expiry effectiveness deadline-based even when the worker is stopped.

This combines the security candidate's clear database identity and isolated worker lifecycle with the integrity candidate's narrower execute-only credential and atomic SQL transition. The operations candidate correctly identifies that scheduling needs a supervised, restartable process; its initial embedded API loop would put a maintenance capability in every API runtime and couple sweep scaling/health to HTTP replicas. The dedicated command can run locally in Compose and independently in hosted deployments.

## Reconciled design

- Provision `pos_api_expiry_worker` as a `NOLOGIN`, `NOSUPERUSER`, `NOBYPASSRLS`, `NOCREATEDB`, `NOCREATEROLE` role with narrowly scoped column grants and worker-only forced-RLS policies. It owns the transition function, but does not own touched tables and is not a member of the API or migration roles.
- Provision `pos_api_expiry_runner` as a login-only role with `EXECUTE` on the one transition function and no direct table/schema mutation grants. It receives no membership in the function-owner role. Keep its credential only in the worker service environment. Revoke function execution from `PUBLIC`; pin a safe function `search_path`.
- The function accepts no tenant or reservation ID and materializes at most one hold per invocation. It discovers a due `held`/`ready` reservation without row locks, reads only its line product IDs, acquires sorted `advanced-pos:inventory:v1:{tenant}:{pickup_branch}:{product}` advisory locks, then locks and rechecks the reservation. Capture `clock_timestamp()` after lock waits. If still due, transition to `expired`, increment revision, append exactly one immutable system event, and release only that reservation's active allocations with `release_reason='expired'`, atomically.
- Force RLS on every touched table. The function owner's policies must authorize only this expiry flow. Policies must bind to actual database role identity (`current_user` inside the definer function is the no-login function owner), never a caller-set GUC. Function code must constrain updates to the selected reservation and linked lines/allocations. Runner sessions can only execute the function.
- Add an explicit event actor kind. Existing human events remain membership-backed and identity-bound; a system expiry event has a null membership, no station, pickup branch as acting branch, no customer/contact snapshots, and the matching reservation/revision. Do not synthesize a human audit event.
- Run `pos-api pickup-expiry-worker` as its own supervised service. Poll every 5–10 seconds, cap a pass at 100 calls, stop a pass after a database error, log counts and opaque IDs only, retry with bounded backoff/jitter, and resume by rediscovering due IDs after restart. API health/checkout must not depend on the sweep. Multiple runners may discover the same candidate; advisory locking and post-lock recheck make processing idempotent.
- Keep ATP deadline semantics unchanged: expired allocations stop consuming availability at `expires_at` whether or not materialization has happened.
- Provision roles in local PostgreSQL bootstrap and the restricted verifier, and document equivalent production role/secret setup. Do not place runner credentials in the migrator, API, or browser. Do not use the migration credential in the worker.

## Comparative findings

| Concern | Finding |
| --- | --- |
| Least privilege and RLS | Integrity's execute-only runner plus a no-login RLS-constrained function owner is strongest. Direct worker table grants from security candidate are narrower than API grants, but leave a credential able to issue arbitrary permitted table statements outside the transition. |
| Lock order and races | All viable proposals align with the prior pickup council: discover without row locks, take sorted inventory locks first, then row-lock and recheck against fresh database time. Preserve that order exactly. |
| Audit semantics | Security and integrity correctly reject staff impersonation. A typed system event is the canonical immutable expiry record; null actor membership must be exclusive to system expiry. |
| Reliability and operations | Operations correctly requires a bounded supervised loop and observability; a separate runner command isolates sweep failure and scaling from API checkout. No cursor is needed because each successful transaction removes its candidate from the due set. |
| Runtime compatibility | Current `pos-api` has `serve` and `migrate` commands and Compose has no scheduler. Add a dedicated command/service and role provisioning rather than an in-process API loop or provider-specific `pg_cron`. |

## Verification gates

Before marking this slice complete, verify migrations apply twice; forced RLS and grants deny direct runner access and unauthorized cross-tenant access; function owner cannot read customer contact or mutate stock/sales; one due hold yields one event and releases only its allocations; not-yet-due and terminal/sold holds are unchanged; retries/restarts and multiple runners do not duplicate events; an expiry reached during lock wait is rechecked using database time; and deadline-based ATP remains correct with the worker offline. Include the worker service in the restricted PostgreSQL verifier. Authenticated browser review is a separate remaining pickup acceptance item.

No unresolved design conflict requires user input.
