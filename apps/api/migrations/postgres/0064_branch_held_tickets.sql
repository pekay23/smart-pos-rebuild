-- Server-coordinated register drafts. These rows are not sales: they remain
-- editable until checkout is prepared, then bind to one atomic sale commit.
CREATE TABLE branch_held_tickets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    origin_station_id UUID NOT NULL,
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    status TEXT NOT NULL CHECK (status IN ('held','checkout_pending','completed','discarded')),
    payload JSONB NOT NULL CHECK (jsonb_typeof(payload) = 'object' AND pg_column_size(payload) <= 262144),
    create_key TEXT NOT NULL CHECK (length(create_key) BETWEEN 1 AND 128),
    create_fingerprint BYTEA NOT NULL CHECK (octet_length(create_fingerprint) = 32),
    checkout_body JSONB,
    checkout_key TEXT,
    checkout_fingerprint BYTEA,
    claim_lease_id UUID,
    claim_expires_at TIMESTAMPTZ,
    completed_sale_id UUID,
    created_by UUID NOT NULL,
    updated_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (origin_station_id, branch_id, tenant_id)
        REFERENCES stations(id, branch_id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (created_by, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (updated_by, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, claim_lease_id)
        REFERENCES station_actor_leases(tenant_id, id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, branch_id, id),
    UNIQUE (tenant_id, branch_id, created_by, create_key),
    CHECK (
        (status = 'checkout_pending' AND checkout_body IS NOT NULL
            AND checkout_key IS NOT NULL AND checkout_fingerprint IS NOT NULL
            AND octet_length(checkout_fingerprint) = 32)
        OR (status IN ('held','completed','discarded') AND (
            (checkout_body IS NULL AND checkout_key IS NULL AND checkout_fingerprint IS NULL)
            OR (status = 'completed' AND checkout_body IS NOT NULL
                AND checkout_key IS NOT NULL AND checkout_fingerprint IS NOT NULL
                AND octet_length(checkout_fingerprint) = 32)
        ))
    ),
    CHECK ((status = 'completed') = (completed_sale_id IS NOT NULL))
    ,CHECK ((claim_lease_id IS NULL) = (claim_expires_at IS NULL))
);

CREATE INDEX branch_held_tickets_branch_open
    ON branch_held_tickets (tenant_id, branch_id, updated_at DESC, id)
    WHERE status IN ('held','checkout_pending');
CREATE INDEX branch_held_tickets_claim
    ON branch_held_tickets (tenant_id, claim_lease_id)
    WHERE claim_lease_id IS NOT NULL;

ALTER TABLE hosted_sales ADD COLUMN held_ticket_id UUID;
ALTER TABLE hosted_sales
    ADD CONSTRAINT hosted_sales_held_ticket_fk
        FOREIGN KEY (tenant_id, branch_id, held_ticket_id)
        REFERENCES branch_held_tickets(tenant_id, branch_id, id) ON DELETE RESTRICT;
CREATE UNIQUE INDEX hosted_sales_held_ticket_unique
    ON hosted_sales (tenant_id, branch_id, held_ticket_id)
    WHERE held_ticket_id IS NOT NULL;
ALTER TABLE branch_held_tickets
    ADD CONSTRAINT branch_held_tickets_completed_sale_fk
        FOREIGN KEY (tenant_id, branch_id, completed_sale_id)
        REFERENCES hosted_sales(tenant_id, branch_id, id) ON DELETE RESTRICT;
GRANT INSERT (held_ticket_id) ON hosted_sales TO pos_api_runtime;
GRANT SELECT (held_ticket_id) ON hosted_sales TO pos_api_runtime;

-- A cashier on another authorized station in the same branch must be able to
-- retrieve a sale already committed from a shared ticket for an exact retry.
CREATE POLICY hosted_sales_held_ticket_branch_read ON hosted_sales
    FOR SELECT USING (
        held_ticket_id IS NOT NULL
        AND tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
            WHERE m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.tenant_id=hosted_sales.tenant_id
              AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status='active' AND g.role_key='cashier'
              AND (g.branch_id IS NULL OR g.branch_id=hosted_sales.branch_id)
              AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid)
        )
    );

ALTER TABLE branch_held_tickets ENABLE ROW LEVEL SECURITY;
ALTER TABLE branch_held_tickets FORCE ROW LEVEL SECURITY;

CREATE POLICY branch_held_tickets_read ON branch_held_tickets
    FOR SELECT USING (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
            WHERE m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.tenant_id=branch_held_tickets.tenant_id
              AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status='active' AND g.role_key IN ('owner','admin','manager','cashier')
              AND (g.branch_id IS NULL OR g.branch_id=branch_held_tickets.branch_id)
              AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid)
        )
    );

CREATE POLICY branch_held_tickets_insert ON branch_held_tickets
    FOR INSERT WITH CHECK (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND origin_station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid
        AND created_by = NULLIF(current_setting('pos.membership_id', true), '')::uuid
        AND updated_by = created_by
        AND status = 'held' AND claim_lease_id IS NOT NULL
        AND claim_expires_at > clock_timestamp()
        AND claim_lease_id = NULLIF(current_setting('pos.lease_id', true), '')::uuid
        AND checkout_body IS NULL AND completed_sale_id IS NULL
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
            WHERE m.id=created_by AND m.tenant_id=branch_held_tickets.tenant_id
              AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status='active' AND g.role_key='cashier'
              AND (g.branch_id IS NULL OR g.branch_id=branch_held_tickets.branch_id)
              AND (g.station_id IS NULL OR g.station_id=branch_held_tickets.origin_station_id)
        )
        AND EXISTS (
            SELECT 1 FROM station_actor_leases l
            JOIN station_actor_lease_locks k ON k.station_id=l.station_id AND k.lease_id=l.id
            WHERE l.tenant_id=branch_held_tickets.tenant_id AND l.branch_id=branch_held_tickets.branch_id
              AND l.station_id=branch_held_tickets.origin_station_id AND l.id=branch_held_tickets.claim_lease_id
              AND l.membership_id=created_by AND l.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND l.status='issued' AND l.expires_at>clock_timestamp()
        )
    );

CREATE POLICY branch_held_tickets_update ON branch_held_tickets
    FOR UPDATE USING (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND status IN ('held','checkout_pending')
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
            WHERE m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.tenant_id=branch_held_tickets.tenant_id
              AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status='active' AND g.role_key='cashier'
              AND (g.branch_id IS NULL OR g.branch_id=branch_held_tickets.branch_id)
              AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid)
        )
    ) WITH CHECK (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND updated_by = NULLIF(current_setting('pos.membership_id', true), '')::uuid
        AND status IN ('held','checkout_pending','completed','discarded')
        AND EXISTS (
            SELECT 1 FROM station_actor_leases active_lease
            JOIN station_actor_lease_locks active_lock
              ON active_lock.station_id=active_lease.station_id AND active_lock.lease_id=active_lease.id
            WHERE active_lease.id=NULLIF(current_setting('pos.lease_id', true), '')::uuid
              AND active_lease.tenant_id=branch_held_tickets.tenant_id
              AND active_lease.branch_id=branch_held_tickets.branch_id
              AND active_lease.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid
              AND active_lease.membership_id=updated_by
              AND active_lease.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND active_lease.status='issued' AND active_lease.expires_at>clock_timestamp()
        )
        AND (claim_lease_id IS NULL OR (
            claim_lease_id=NULLIF(current_setting('pos.lease_id', true), '')::uuid
            AND claim_expires_at > clock_timestamp()
        ))
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
            WHERE m.id=updated_by AND m.tenant_id=branch_held_tickets.tenant_id
              AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status='active' AND g.role_key='cashier'
              AND (g.branch_id IS NULL OR g.branch_id=branch_held_tickets.branch_id)
              AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid)
        )
        AND (
            claim_lease_id IS NULL OR EXISTS (
                SELECT 1 FROM station_actor_leases l
                JOIN station_actor_lease_locks k ON k.station_id=l.station_id AND k.lease_id=l.id
                WHERE l.tenant_id=branch_held_tickets.tenant_id AND l.branch_id=branch_held_tickets.branch_id
                  AND l.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid
                  AND l.id=branch_held_tickets.claim_lease_id
                  AND l.membership_id=updated_by AND l.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
                  AND l.status='issued' AND l.expires_at>clock_timestamp()
            )
        )
    );

GRANT SELECT, INSERT, UPDATE ON branch_held_tickets TO pos_api_runtime;
