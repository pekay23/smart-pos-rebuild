-- Tenant-submitted requests for bank, terminal and payment processor support.
-- Operator workflow is added separately and uses a distinct auth audience.
CREATE TABLE hosted_payment_integration_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    created_by_membership_id UUID NOT NULL,
    country_code TEXT NOT NULL CHECK (country_code ~ '^[A-Z]{2}$'),
    provider_name TEXT NOT NULL CHECK (length(btrim(provider_name)) BETWEEN 1 AND 120),
    terminal_model TEXT CHECK (terminal_model IS NULL OR length(terminal_model) <= 120),
    capability TEXT NOT NULL CHECK (length(btrim(capability)) BETWEEN 1 AND 100),
    business_impact TEXT NOT NULL CHECK (business_impact IN ('business_blocked','workaround_available','exploring')),
    details TEXT CHECK (details IS NULL OR length(details) <= 1000),
    status TEXT NOT NULL DEFAULT 'submitted' CHECK (status IN ('submitted','under_review','planned','in_progress','released','not_planned')),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (branch_id,tenant_id) REFERENCES branches(id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (created_by_membership_id,tenant_id) REFERENCES memberships(id,tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id,branch_id,id),
    UNIQUE (tenant_id,created_by_membership_id,idempotency_key)
);
CREATE INDEX hosted_payment_integration_requests_branch_date
    ON hosted_payment_integration_requests(tenant_id,branch_id,created_at DESC,id);

ALTER TABLE hosted_payment_integration_requests ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_payment_integration_requests FORCE ROW LEVEL SECURITY;
CREATE POLICY hosted_payment_integration_requests_branch_select
ON hosted_payment_integration_requests FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
          AND m.tenant_id=hosted_payment_integration_requests.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id=hosted_payment_integration_requests.branch_id)
          AND g.station_id IS NULL
    )
);
CREATE POLICY hosted_payment_integration_requests_branch_insert
ON hosted_payment_integration_requests FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND created_by_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND status='submitted'
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=hosted_payment_integration_requests.created_by_membership_id
          AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
          AND m.tenant_id=hosted_payment_integration_requests.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id=hosted_payment_integration_requests.branch_id)
          AND g.station_id IS NULL
    )
);

