# Hosted attendance design cross-judge

## Rubric scores

Scores use 1–5, where 5 is strongest.

| Criterion | Event-ledger candidate | Period-row candidate | Notes |
|---|---:|---:|---|
| Stable employee identity across temporary cashier switching | 5 | 5 | Both key attendance to tenant membership and explicitly keep temporary cashier mode out of attendance identity. |
| One open period per worker under concurrent requests | 4 | 5 | Both use a membership lock and a unique open projection/index; the period candidate states the partial unique constraint directly on the row that answers current-state queries. |
| Server timestamps and immutable schedule snapshot | 5 | 5 | Both snapshot resolved schedule boundaries and keep timestamps server-owned. |
| Automatic scheduled end, idempotent, never auto-start | 5 | 5 | Both require a restricted runner to write the scheduled end at the due instant, with idempotent transition behavior and no automatic clock-in. |
| Authorized API/UI/database scope | 4 | 5 | Both identify forced RLS, least grants, worker isolation and role scopes. The period candidate more explicitly separates self-service clock actions from manager schedule/report access and limits correction exposure. |
| **Total** | **23** | **25** | |

## Recommendation

Choose **candidate-periods.md** as the implementation base. Its row-per-period shape directly serves the dominant UI queries (my current shift and branch attendance history), and its partial unique index expresses the one-open-period invariant at the database boundary. It also gives the clock transition and worker a narrow row to lock and update without reconstructing a fold of prior events.

## Grafts

- Keep the event-ledger candidate's explicit invariant that each transition, audit event, idempotency result and projection/period update commit atomically.
- Retain its stable-order worker selection, bounded pass, and writing the scheduled end at `due_at` rather than worker execution time.
- Keep the event model's future correction path append-only. Do not expose manager correction in the first release; when added, a correction must be a reviewed compensating event and must not rewrite the original clock evidence.
- Add idempotency keys to explicit clock actions as in the ledger candidate; the period candidate did not state them as clearly for clock-in/out.

## Rejections

- Reject events as the sole authoritative source for current period state in this first release. Replaying the event stream adds query and concurrency complexity to everyday status and branch reports without improving the first-release behavior enough to justify it.
- Reject a separate mutable `attendance_open_periods` projection. The period row itself can be the current state and lock target; a second current-state table would duplicate state and create another consistency invariant.
- Preserve append-only `attendance_events` as immutable evidence in the same transaction, while the attendance period row is the operational current-state record. Corrections remain deferred until a reviewed policy exists.

## Decision

Use a period row plus an append-only event row written atomically. Enforce one open period per membership across all branches with a partial unique index and membership-scoped transaction lock. Snapshot timezone and resolved schedule boundaries at clock-in. Restrict scheduled end to a separate no-login worker interface, and never let reads or worker execution create an automatic start.
