-- Branches configure the tender types shown at checkout. This records the
-- tenant's intended method; it does not claim that an external provider approved
-- or captured a payment. Provider adapters are added separately.
CREATE TABLE branch_payment_options (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    method_key TEXT NOT NULL CHECK (method_key ~ '^[a-z0-9][a-z0-9_-]{1,47}$'),
    label TEXT NOT NULL CHECK (length(btrim(label)) BETWEEN 1 AND 96),
    tender_kind TEXT NOT NULL CHECK (tender_kind IN ('cash', 'card', 'mobile_money', 'bank_transfer', 'other')),
    provider_name TEXT CHECK (provider_name IS NULL OR length(btrim(provider_name)) BETWEEN 1 AND 120),
    country_code CHAR(2) CHECK (country_code IS NULL OR country_code ~ '^[A-Z]{2}$'),
    requires_reference BOOLEAN NOT NULL DEFAULT FALSE,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    -- Kept explicit so clients cannot mistake a manually confirmed terminal
    -- payment for an integrated provider authorization.
    recording_mode TEXT NOT NULL DEFAULT 'manual' CHECK (recording_mode = 'manual'),
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_by_membership_id UUID NOT NULL,
    updated_by_membership_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (branch_id, tenant_id)
        REFERENCES branches(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (created_by_membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (updated_by_membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, id),
    UNIQUE (branch_id, method_key),
    CHECK ((tender_kind = 'cash' AND NOT requires_reference)
        OR tender_kind <> 'cash')
);

CREATE UNIQUE INDEX branch_payment_option_label_unique
    ON branch_payment_options (branch_id, lower(btrim(label)));
CREATE INDEX branch_payment_options_enabled
    ON branch_payment_options (tenant_id, branch_id, enabled, lower(label));

ALTER TABLE branch_payment_options ENABLE ROW LEVEL SECURITY;
ALTER TABLE branch_payment_options FORCE ROW LEVEL SECURITY;

CREATE POLICY branch_payment_options_read ON branch_payment_options
    FOR SELECT USING (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.tenant_id = branch_payment_options.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND g.role_key IN ('owner', 'admin', 'manager', 'cashier')
              AND (g.branch_id IS NULL OR g.branch_id = branch_payment_options.branch_id)
              AND (g.station_id IS NULL OR g.station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid)
        )
    );

CREATE POLICY branch_payment_options_management_insert ON branch_payment_options
    FOR INSERT WITH CHECK (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND created_by_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
        AND updated_by_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.id = created_by_membership_id
              AND m.tenant_id = branch_payment_options.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND g.role_key IN ('owner', 'admin', 'manager')
              AND (g.branch_id IS NULL OR g.branch_id = branch_payment_options.branch_id)
              AND g.station_id IS NULL
        )
    );

CREATE POLICY branch_payment_options_management_update ON branch_payment_options
    FOR UPDATE USING (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.tenant_id = branch_payment_options.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND g.role_key IN ('owner', 'admin', 'manager')
              AND (g.branch_id IS NULL OR g.branch_id = branch_payment_options.branch_id)
              AND g.station_id IS NULL
        )
    ) WITH CHECK (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND updated_by_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
    );

