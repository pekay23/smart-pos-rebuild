-- One-time owner-created challenges bind proof of station key possession to
-- one tenant, branch and station. Challenges expire after five minutes.
CREATE TABLE station_activation_challenges (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    station_id UUID NOT NULL,
    created_by_membership_id UUID NOT NULL,
    nonce BYTEA NOT NULL CHECK (octet_length(nonce) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (now() + interval '5 minutes'),
    consumed_at TIMESTAMPTZ,
    FOREIGN KEY (station_id, branch_id, tenant_id)
        REFERENCES stations(id, branch_id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (created_by_membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, id),
    CHECK (expires_at > created_at)
);
CREATE INDEX station_activation_challenges_expiry
    ON station_activation_challenges (tenant_id, station_id, expires_at);

ALTER TABLE station_activation_challenges ENABLE ROW LEVEL SECURITY;
ALTER TABLE station_activation_challenges FORCE ROW LEVEL SECURITY;
CREATE POLICY station_activation_challenge_owner_select
    ON station_activation_challenges FOR SELECT USING (
        EXISTS (
            SELECT 1 FROM membership_role_grants g
            JOIN memberships m ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            WHERE g.tenant_id = station_activation_challenges.tenant_id
              AND g.role_key = 'owner' AND g.branch_id IS NULL AND g.station_id IS NULL
              AND m.id = station_activation_challenges.created_by_membership_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
    );
CREATE POLICY station_activation_challenge_owner_insert
    ON station_activation_challenges FOR INSERT WITH CHECK (
        EXISTS (
            SELECT 1 FROM membership_role_grants g
            JOIN memberships m ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            WHERE g.tenant_id = station_activation_challenges.tenant_id
              AND g.role_key = 'owner' AND g.branch_id IS NULL AND g.station_id IS NULL
              AND m.id = station_activation_challenges.created_by_membership_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
        AND EXISTS (
            SELECT 1 FROM stations s
            WHERE s.id = station_activation_challenges.station_id
              AND s.branch_id = station_activation_challenges.branch_id
              AND s.tenant_id = station_activation_challenges.tenant_id
              AND s.status = 'active'
        )
    );
CREATE POLICY station_activation_challenge_owner_consume
    ON station_activation_challenges FOR UPDATE
    USING (
        consumed_at IS NULL AND expires_at > now()
        AND EXISTS (
            SELECT 1 FROM membership_role_grants g
            JOIN memberships m ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            WHERE g.tenant_id = station_activation_challenges.tenant_id
              AND g.role_key = 'owner' AND g.branch_id IS NULL AND g.station_id IS NULL
              AND m.id = station_activation_challenges.created_by_membership_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
    )
    WITH CHECK (
        consumed_at IS NOT NULL
        AND EXISTS (
            SELECT 1 FROM membership_role_grants g
            JOIN memberships m ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            WHERE g.tenant_id = station_activation_challenges.tenant_id
              AND g.role_key = 'owner' AND g.branch_id IS NULL AND g.station_id IS NULL
              AND m.id = station_activation_challenges.created_by_membership_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
    );
