-- Lease rows are retained history. A separate station lock coordinates the
-- currently active actor so another cashier never mutates the previous actor's
-- audit row during expiry handoff.
CREATE TABLE station_actor_leases (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    station_id UUID NOT NULL,
    membership_id UUID NOT NULL,
    identity_id UUID NOT NULL,
    lease_token_hash BYTEA NOT NULL CHECK (octet_length(lease_token_hash) = 32),
    status TEXT NOT NULL DEFAULT 'issued' CHECK (status IN ('issued', 'released')),
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ,
    FOREIGN KEY (station_id, branch_id, tenant_id)
        REFERENCES stations(id, branch_id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (identity_id) REFERENCES person_identities(id) ON DELETE RESTRICT,
    CHECK (expires_at > started_at),
    CHECK ((status = 'issued' AND ended_at IS NULL) OR
           (status = 'released' AND ended_at IS NOT NULL)),
    UNIQUE (tenant_id, id)
);
CREATE INDEX station_actor_leases_by_actor
    ON station_actor_leases (tenant_id, membership_id, started_at DESC);

CREATE TABLE station_actor_lease_locks (
    station_id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    lease_id UUID NOT NULL UNIQUE,
    FOREIGN KEY (station_id, branch_id, tenant_id)
        REFERENCES stations(id, branch_id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, lease_id)
        REFERENCES station_actor_leases(tenant_id, id) ON DELETE RESTRICT
);

ALTER TABLE station_actor_leases ENABLE ROW LEVEL SECURITY;
ALTER TABLE station_actor_leases FORCE ROW LEVEL SECURITY;
ALTER TABLE station_actor_lease_locks ENABLE ROW LEVEL SECURITY;
ALTER TABLE station_actor_lease_locks FORCE ROW LEVEL SECURITY;

CREATE POLICY station_actor_lease_read ON station_actor_leases
    FOR SELECT USING (
        EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            WHERE m.id = station_actor_leases.membership_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
        OR EXISTS (
            SELECT 1 FROM membership_role_grants g
            JOIN memberships m ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            WHERE g.tenant_id = station_actor_leases.tenant_id
              AND g.role_key = 'owner' AND g.branch_id IS NULL AND g.station_id IS NULL
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
        OR (
            station_actor_leases.status = 'issued'
            AND station_actor_leases.expires_at <= now()
            AND EXISTS (
                SELECT 1 FROM memberships m
                JOIN membership_role_grants g
                  ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
                WHERE m.tenant_id = station_actor_leases.tenant_id
                  AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
                  AND m.status = 'active' AND g.role_key = 'cashier'
                  AND (g.branch_id IS NULL OR g.branch_id = station_actor_leases.branch_id)
                  AND (g.station_id IS NULL OR g.station_id = station_actor_leases.station_id)
            )
        )
    );

CREATE POLICY station_actor_lease_insert ON station_actor_leases
    FOR INSERT WITH CHECK (
        identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
        AND status = 'issued' AND ended_at IS NULL
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g
              ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.id = station_actor_leases.membership_id
              AND m.tenant_id = station_actor_leases.tenant_id
              AND m.identity_id = station_actor_leases.identity_id
              AND m.status = 'active' AND g.role_key = 'cashier'
              AND (g.branch_id IS NULL OR g.branch_id = station_actor_leases.branch_id)
              AND (g.station_id IS NULL OR g.station_id = station_actor_leases.station_id)
        )
        AND EXISTS (
            SELECT 1 FROM stations s
            JOIN station_activations a
              ON a.tenant_id = s.tenant_id AND a.branch_id = s.branch_id AND a.station_id = s.id
            WHERE s.id = station_actor_leases.station_id
              AND s.branch_id = station_actor_leases.branch_id
              AND s.tenant_id = station_actor_leases.tenant_id
              AND s.status = 'active' AND a.status = 'active'
        )
    );

CREATE POLICY station_actor_lease_update ON station_actor_leases
    FOR UPDATE
    USING (
        identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
        AND status = 'issued'
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g
              ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.id = station_actor_leases.membership_id
              AND m.tenant_id = station_actor_leases.tenant_id
              AND m.identity_id = station_actor_leases.identity_id
              AND m.status = 'active' AND g.role_key = 'cashier'
              AND (g.branch_id IS NULL OR g.branch_id = station_actor_leases.branch_id)
              AND (g.station_id IS NULL OR g.station_id = station_actor_leases.station_id)
        )
    )
    WITH CHECK (
        identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
        AND (
            (status = 'issued' AND ended_at IS NULL)
            OR (status = 'released' AND ended_at IS NOT NULL)
        )
    );

CREATE POLICY station_actor_lock_select ON station_actor_lease_locks
    FOR SELECT USING (
        EXISTS (
            SELECT 1 FROM station_actor_leases l
            WHERE l.id = station_actor_lease_locks.lease_id
              AND l.tenant_id = station_actor_lease_locks.tenant_id
        )
    );
CREATE POLICY station_actor_lock_insert ON station_actor_lease_locks
    FOR INSERT WITH CHECK (
        EXISTS (
            SELECT 1 FROM station_actor_leases l
            JOIN memberships m ON m.id = l.membership_id AND m.tenant_id = l.tenant_id
            JOIN membership_role_grants g ON g.membership_id = m.id AND g.tenant_id = m.tenant_id
            JOIN stations s ON s.id = l.station_id AND s.branch_id = l.branch_id AND s.tenant_id = l.tenant_id
            JOIN station_activations a ON a.station_id = s.id AND a.branch_id = s.branch_id AND a.tenant_id = s.tenant_id
            WHERE l.id = station_actor_lease_locks.lease_id
              AND l.tenant_id = station_actor_lease_locks.tenant_id
              AND l.branch_id = station_actor_lease_locks.branch_id
              AND l.station_id = station_actor_lease_locks.station_id
              AND l.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND l.status = 'issued' AND l.expires_at > now()
              AND m.identity_id = l.identity_id AND m.status = 'active'
              AND g.role_key = 'cashier'
              AND (g.branch_id IS NULL OR g.branch_id = l.branch_id)
              AND (g.station_id IS NULL OR g.station_id = l.station_id)
              AND s.status = 'active' AND a.status = 'active'
        )
    );
CREATE POLICY station_actor_lock_delete ON station_actor_lease_locks
    FOR DELETE USING (
        EXISTS (
            SELECT 1 FROM station_actor_leases l
            WHERE l.id = station_actor_lease_locks.lease_id
              AND l.tenant_id = station_actor_lease_locks.tenant_id
              AND (
                  (l.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
                   AND l.status IN ('issued', 'released'))
                  OR (
                      l.status = 'issued' AND l.expires_at <= now()
                      AND EXISTS (
                          SELECT 1 FROM memberships m
                          JOIN membership_role_grants g
                            ON g.membership_id = m.id AND g.tenant_id = m.tenant_id
                          WHERE m.tenant_id = l.tenant_id
                            AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
                            AND m.status = 'active' AND g.role_key = 'cashier'
                            AND (g.branch_id IS NULL OR g.branch_id = l.branch_id)
                            AND (g.station_id IS NULL OR g.station_id = l.station_id)
                      )
                  )
              )
        )
    );
