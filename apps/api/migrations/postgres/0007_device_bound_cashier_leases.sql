-- Bind each cashier lease to the precise activated station key that proved
-- possession when the lease was acquired.
ALTER TABLE station_actor_leases
    ADD COLUMN activation_id UUID;

UPDATE station_actor_leases l
SET activation_id = (
    SELECT a.id
    FROM station_activations a
    WHERE a.tenant_id = l.tenant_id
      AND a.branch_id = l.branch_id
      AND a.station_id = l.station_id
    ORDER BY a.activated_at DESC
    LIMIT 1
);

ALTER TABLE station_actor_leases
    ALTER COLUMN activation_id SET NOT NULL,
    ADD CONSTRAINT station_actor_lease_activation_fk
        FOREIGN KEY (tenant_id, activation_id)
        REFERENCES station_activations(tenant_id, id) ON DELETE RESTRICT;

CREATE TABLE station_actor_lease_challenges (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    station_id UUID NOT NULL,
    membership_id UUID NOT NULL,
    identity_id UUID NOT NULL,
    nonce BYTEA NOT NULL CHECK (octet_length(nonce) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (now() + interval '2 minutes'),
    consumed_at TIMESTAMPTZ,
    FOREIGN KEY (station_id, branch_id, tenant_id)
        REFERENCES stations(id, branch_id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (identity_id) REFERENCES person_identities(id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, id),
    CHECK (expires_at > created_at)
);
CREATE INDEX station_actor_lease_challenges_expiry
    ON station_actor_lease_challenges (tenant_id, station_id, expires_at);

ALTER TABLE station_actor_lease_challenges ENABLE ROW LEVEL SECURITY;
ALTER TABLE station_actor_lease_challenges FORCE ROW LEVEL SECURITY;

CREATE POLICY station_actor_lease_challenge_select
    ON station_actor_lease_challenges FOR SELECT USING (
        identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            WHERE m.id = station_actor_lease_challenges.membership_id
              AND m.tenant_id = station_actor_lease_challenges.tenant_id
              AND m.identity_id = station_actor_lease_challenges.identity_id
              AND m.status = 'active'
        )
    );

CREATE POLICY station_actor_lease_challenge_insert
    ON station_actor_lease_challenges FOR INSERT WITH CHECK (
        identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g
              ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            JOIN stations s
              ON s.id = station_actor_lease_challenges.station_id
             AND s.branch_id = station_actor_lease_challenges.branch_id
             AND s.tenant_id = station_actor_lease_challenges.tenant_id
            JOIN station_activations a
              ON a.station_id = s.id AND a.branch_id = s.branch_id AND a.tenant_id = s.tenant_id
             AND a.status = 'active'
            WHERE m.id = station_actor_lease_challenges.membership_id
              AND m.tenant_id = station_actor_lease_challenges.tenant_id
              AND m.identity_id = station_actor_lease_challenges.identity_id
              AND m.status = 'active' AND g.role_key = 'cashier'
              AND (g.branch_id IS NULL OR g.branch_id = s.branch_id)
              AND (g.station_id IS NULL OR g.station_id = s.id)
              AND s.status = 'active'
        )
    );

CREATE POLICY station_actor_lease_challenge_consume
    ON station_actor_lease_challenges FOR UPDATE
    USING (
        consumed_at IS NULL AND expires_at > now()
        AND identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            WHERE m.id = station_actor_lease_challenges.membership_id
              AND m.tenant_id = station_actor_lease_challenges.tenant_id
              AND m.identity_id = station_actor_lease_challenges.identity_id
              AND m.status = 'active'
        )
    )
    WITH CHECK (
        consumed_at IS NOT NULL
        AND identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
    );

-- Only newly created leases must reference the activation that signed their
-- challenge. Historical ended leases retain the activation selected above.
DROP POLICY station_actor_lease_insert ON station_actor_leases;
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
              ON a.id = station_actor_leases.activation_id
             AND a.tenant_id = s.tenant_id AND a.branch_id = s.branch_id AND a.station_id = s.id
            WHERE s.id = station_actor_leases.station_id
              AND s.branch_id = station_actor_leases.branch_id
              AND s.tenant_id = station_actor_leases.tenant_id
              AND s.status = 'active' AND a.status = 'active'
        )
    );
