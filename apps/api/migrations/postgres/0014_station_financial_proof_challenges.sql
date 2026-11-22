-- A station must prove fresh private-key possession before a financial write.
-- The challenge is tied to one cashier lease, operation, retry key, and body hash.
CREATE TABLE station_financial_proof_challenges (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    station_id UUID NOT NULL,
    membership_id UUID NOT NULL,
    identity_id UUID NOT NULL,
    activation_id UUID NOT NULL,
    lease_id UUID NOT NULL,
    operation TEXT NOT NULL CHECK (operation IN ('sale.commit')),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    payload_hash BYTEA NOT NULL CHECK (octet_length(payload_hash) = 32),
    nonce BYTEA NOT NULL CHECK (octet_length(nonce) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (now() + interval '60 seconds'),
    consumed_at TIMESTAMPTZ,
    FOREIGN KEY (station_id, branch_id, tenant_id)
        REFERENCES stations(id, branch_id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (identity_id) REFERENCES person_identities(id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, activation_id)
        REFERENCES station_activations(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, lease_id)
        REFERENCES station_actor_leases(tenant_id, id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, id),
    CHECK (expires_at > created_at)
);

CREATE INDEX station_financial_proof_challenges_active
    ON station_financial_proof_challenges (tenant_id, station_id, membership_id, expires_at)
    WHERE consumed_at IS NULL;

ALTER TABLE station_financial_proof_challenges ENABLE ROW LEVEL SECURITY;
ALTER TABLE station_financial_proof_challenges FORCE ROW LEVEL SECURITY;

CREATE POLICY station_financial_proof_challenge_read
    ON station_financial_proof_challenges FOR SELECT USING (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid
        AND membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
        AND identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
    );

CREATE POLICY station_financial_proof_challenge_insert
    ON station_financial_proof_challenges FOR INSERT WITH CHECK (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid
        AND membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
        AND identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g
              ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            JOIN station_actor_leases l
              ON l.tenant_id = station_financial_proof_challenges.tenant_id
             AND l.branch_id = station_financial_proof_challenges.branch_id
             AND l.station_id = station_financial_proof_challenges.station_id
             AND l.membership_id = station_financial_proof_challenges.membership_id
             AND l.identity_id = station_financial_proof_challenges.identity_id
             AND l.activation_id = station_financial_proof_challenges.activation_id
             AND l.id = station_financial_proof_challenges.lease_id
            JOIN station_actor_lease_locks k
              ON k.tenant_id = l.tenant_id AND k.branch_id = l.branch_id
             AND k.station_id = l.station_id AND k.lease_id = l.id
            JOIN station_activations a
              ON a.tenant_id = l.tenant_id AND a.branch_id = l.branch_id
             AND a.station_id = l.station_id AND a.id = l.activation_id
            WHERE m.id = station_financial_proof_challenges.membership_id
              AND m.tenant_id = station_financial_proof_challenges.tenant_id
              AND m.identity_id = station_financial_proof_challenges.identity_id
              AND m.status = 'active'
              AND g.role_key = 'cashier'
              AND (g.branch_id IS NULL OR g.branch_id = l.branch_id)
              AND (g.station_id IS NULL OR g.station_id = l.station_id)
              AND l.status = 'issued' AND l.expires_at > now()
              AND a.status = 'active'
        )
    );

CREATE POLICY station_financial_proof_challenge_consume
    ON station_financial_proof_challenges FOR UPDATE
    USING (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid
        AND membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
        AND identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
        AND consumed_at IS NULL AND expires_at > now()
    ) WITH CHECK (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid
        AND membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
        AND identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
        AND consumed_at IS NOT NULL
    );
