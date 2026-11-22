-- Station capacity is a tenant entitlement, independent of staff membership count.
-- Only the migration/operator credential provisions or changes entitlement rows.
CREATE TABLE tenant_entitlements (
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    entitlement_key TEXT NOT NULL CHECK (length(btrim(entitlement_key)) BETWEEN 1 AND 96),
    enabled BOOLEAN NOT NULL DEFAULT false,
    numeric_limit INTEGER CHECK (numeric_limit IS NULL OR numeric_limit >= 0),
    effective_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, entitlement_key),
    CHECK (expires_at IS NULL OR expires_at > effective_at),
    CHECK (entitlement_key <> 'stations.active' OR numeric_limit IS NOT NULL)
);

ALTER TABLE tenant_entitlements ENABLE ROW LEVEL SECURITY;
ALTER TABLE tenant_entitlements FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_entitlement_member_read ON tenant_entitlements
    FOR SELECT
    USING (
        EXISTS (
            SELECT 1
            FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            WHERE m.tenant_id = tenant_entitlements.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
        AND effective_at <= now()
        AND (expires_at IS NULL OR expires_at > now())
    );

CREATE TABLE station_activations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    station_id UUID NOT NULL,
    device_public_key BYTEA NOT NULL CHECK (octet_length(device_public_key) = 32),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'revoked')),
    activated_by_membership_id UUID NOT NULL,
    activated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ,
    FOREIGN KEY (station_id, branch_id, tenant_id)
        REFERENCES stations(id, branch_id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (activated_by_membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, id),
    CHECK ((status = 'active' AND revoked_at IS NULL) OR
           (status = 'revoked' AND revoked_at IS NOT NULL))
);

CREATE UNIQUE INDEX station_activation_one_active_per_station
    ON station_activations (station_id) WHERE status = 'active';
CREATE UNIQUE INDEX station_activation_one_active_per_device
    ON station_activations (tenant_id, device_public_key) WHERE status = 'active';
CREATE INDEX station_activations_by_tenant_status
    ON station_activations (tenant_id, status);

ALTER TABLE station_activations ENABLE ROW LEVEL SECURITY;
ALTER TABLE station_activations FORCE ROW LEVEL SECURITY;
CREATE POLICY station_activation_granted_read ON station_activations
    FOR SELECT
    USING (
        EXISTS (
            SELECT 1
            FROM membership_role_grants g
            JOIN memberships m
              ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            JOIN tenants t ON t.id = g.tenant_id AND t.status = 'active'
            WHERE g.tenant_id = station_activations.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND (g.branch_id IS NULL OR g.branch_id = station_activations.branch_id)
              AND (g.station_id IS NULL OR g.station_id = station_activations.station_id)
        )
    );
CREATE POLICY station_activation_owner_insert ON station_activations
    FOR INSERT
    WITH CHECK (
        EXISTS (
            SELECT 1
            FROM membership_role_grants g
            JOIN memberships m
              ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            WHERE g.tenant_id = station_activations.tenant_id
              AND g.role_key = 'owner'
              AND g.branch_id IS NULL AND g.station_id IS NULL
              AND m.id = station_activations.activated_by_membership_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
    );
CREATE POLICY station_activation_owner_update ON station_activations
    FOR UPDATE
    USING (
        EXISTS (
            SELECT 1
            FROM membership_role_grants g
            JOIN memberships m
              ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            WHERE g.tenant_id = station_activations.tenant_id
              AND g.role_key = 'owner'
              AND g.branch_id IS NULL AND g.station_id IS NULL
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
    )
    WITH CHECK (
        EXISTS (
            SELECT 1
            FROM membership_role_grants g
            JOIN memberships m
              ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            WHERE g.tenant_id = station_activations.tenant_id
              AND g.role_key = 'owner'
              AND g.branch_id IS NULL AND g.station_id IS NULL
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
    );

