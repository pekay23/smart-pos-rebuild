-- Tenant-wide supplier directory, scoped by the manager's current branch grant.
CREATE TABLE hosted_suppliers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    name TEXT NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 160),
    contact_person TEXT CHECK (contact_person IS NULL OR length(contact_person) <= 160),
    email TEXT CHECK (email IS NULL OR length(email) <= 254),
    phone TEXT CHECK (phone IS NULL OR length(phone) <= 40),
    address TEXT CHECK (address IS NULL OR length(address) <= 500),
    notes TEXT CHECK (notes IS NULL OR length(notes) <= 1000),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','archived')),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_by_membership_id UUID NOT NULL,
    creation_idempotency_key TEXT NOT NULL CHECK (length(creation_idempotency_key) BETWEEN 1 AND 128),
    creation_request_hash BYTEA NOT NULL CHECK (octet_length(creation_request_hash)=32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (created_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, created_by_membership_id, creation_idempotency_key)
);
CREATE INDEX hosted_suppliers_tenant_name ON hosted_suppliers (tenant_id, lower(name), id) WHERE status='active';

ALTER TABLE hosted_suppliers ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_suppliers FORCE ROW LEVEL SECURITY;
CREATE POLICY hosted_suppliers_branch_read ON hosted_suppliers FOR SELECT USING (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND (status='active' OR (status='archived' AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND m.tenant_id=hosted_suppliers.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid)
          AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid)
    )))
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND m.tenant_id=hosted_suppliers.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid)
          AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid)
    )
);
CREATE POLICY hosted_suppliers_branch_insert ON hosted_suppliers FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND created_by_membership_id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
    AND status='active'
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=hosted_suppliers.created_by_membership_id
          AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND m.tenant_id=hosted_suppliers.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid)
          AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid)
    )
);
CREATE POLICY hosted_suppliers_branch_update ON hosted_suppliers FOR UPDATE USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND m.tenant_id=hosted_suppliers.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid)
          AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid)
    )
) WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND status IN ('active','archived')
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND m.tenant_id=hosted_suppliers.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid)
          AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid)
    )
);
