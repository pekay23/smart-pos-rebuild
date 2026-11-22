-- Tenant-wide customer records, exposed only through an authorized branch or station.
CREATE TABLE hosted_customers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    name TEXT NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 160),
    phone TEXT CHECK (phone IS NULL OR length(phone) BETWEEN 1 AND 40),
    email TEXT CHECK (email IS NULL OR length(email) BETWEEN 1 AND 254),
    notes TEXT CHECK (notes IS NULL OR length(notes) <= 1000),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','archived')),
    created_by_membership_id UUID NOT NULL,
    creation_idempotency_key TEXT NOT NULL CHECK (length(creation_idempotency_key) BETWEEN 1 AND 128),
    creation_request_hash BYTEA NOT NULL CHECK (octet_length(creation_request_hash)=32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (created_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, created_by_membership_id, creation_idempotency_key)
);
CREATE INDEX hosted_customers_tenant_name ON hosted_customers (tenant_id, lower(name), id) WHERE status='active';
CREATE INDEX hosted_customers_tenant_phone ON hosted_customers (tenant_id, phone) WHERE phone IS NOT NULL AND status='active';

ALTER TABLE hosted_customers ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_customers FORCE ROW LEVEL SECURITY;
CREATE POLICY hosted_customers_branch_read ON hosted_customers FOR SELECT USING (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND status='active'
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND m.tenant_id=hosted_customers.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager','cashier')
          AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid)
          AND (g.station_id IS NULL OR (g.role_key='cashier' AND g.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid))
    )
);
CREATE POLICY hosted_customers_branch_insert ON hosted_customers FOR INSERT WITH CHECK (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND created_by_membership_id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
    AND status='active'
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=hosted_customers.created_by_membership_id
          AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND m.tenant_id=hosted_customers.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager','cashier')
          AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid)
          AND (g.station_id IS NULL OR (g.role_key='cashier' AND g.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid))
    )
);

ALTER TABLE hosted_sales ADD COLUMN customer_id UUID;
ALTER TABLE hosted_sales ADD CONSTRAINT hosted_sales_customer_fk
    FOREIGN KEY (tenant_id, customer_id) REFERENCES hosted_customers(tenant_id, id) ON DELETE RESTRICT;
CREATE POLICY hosted_sales_customer_visible ON hosted_sales AS RESTRICTIVE FOR INSERT WITH CHECK (
    customer_id IS NULL OR EXISTS (
        SELECT 1 FROM hosted_customers c
        WHERE c.tenant_id=hosted_sales.tenant_id AND c.id=hosted_sales.customer_id
    )
);
