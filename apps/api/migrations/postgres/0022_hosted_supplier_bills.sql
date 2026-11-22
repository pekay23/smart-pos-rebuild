-- Immutable invoice records matched to received supplier deliveries.
CREATE TABLE hosted_supplier_bills (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    supplier_id UUID NOT NULL,
    supplier_reference TEXT NOT NULL CHECK (length(btrim(supplier_reference)) BETWEEN 1 AND 120),
    invoice_date DATE NOT NULL,
    due_date DATE,
    currency_code TEXT NOT NULL CHECK (currency_code ~ '^[A-Z]{3}$'),
    total_minor BIGINT NOT NULL CHECK (total_minor > 0),
    actor_membership_id UUID NOT NULL,
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (branch_id, tenant_id) REFERENCES branches(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, supplier_id) REFERENCES hosted_suppliers(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, branch_id, id),
    UNIQUE (tenant_id, branch_id, actor_membership_id, idempotency_key),
    CHECK (due_date IS NULL OR due_date >= invoice_date)
);
CREATE INDEX hosted_supplier_bills_branch_date ON hosted_supplier_bills(tenant_id, branch_id, invoice_date DESC, id);
CREATE UNIQUE INDEX hosted_supplier_bills_unique_reference ON hosted_supplier_bills(tenant_id, branch_id, supplier_id, lower(btrim(supplier_reference)));

CREATE TABLE hosted_supplier_bill_allocations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    bill_id UUID NOT NULL,
    receipt_id UUID NOT NULL,
    amount_minor BIGINT NOT NULL CHECK (amount_minor > 0),
    FOREIGN KEY (tenant_id, branch_id, bill_id) REFERENCES hosted_supplier_bills(tenant_id, branch_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, branch_id, receipt_id) REFERENCES hosted_purchase_receipts(tenant_id, branch_id, id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, bill_id, receipt_id)
);
CREATE INDEX hosted_supplier_bill_allocations_receipt ON hosted_supplier_bill_allocations(tenant_id, branch_id, receipt_id);

ALTER TABLE hosted_supplier_bills ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_supplier_bills FORCE ROW LEVEL SECURITY;
ALTER TABLE hosted_supplier_bill_allocations ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_supplier_bill_allocations FORCE ROW LEVEL SECURITY;

CREATE POLICY hosted_supplier_bills_branch_read ON hosted_supplier_bills FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.tenant_id=hosted_supplier_bills.tenant_id AND m.status='active'
        AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=hosted_supplier_bills.branch_id) AND g.station_id IS NULL)
);
CREATE POLICY hosted_supplier_bills_branch_insert ON hosted_supplier_bills FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=actor_membership_id AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.tenant_id=hosted_supplier_bills.tenant_id AND m.status='active'
        AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=hosted_supplier_bills.branch_id) AND g.station_id IS NULL)
);
CREATE POLICY hosted_supplier_bill_allocations_branch_read ON hosted_supplier_bill_allocations FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_supplier_bills b WHERE b.tenant_id=hosted_supplier_bill_allocations.tenant_id AND b.id=hosted_supplier_bill_allocations.bill_id)
);
CREATE POLICY hosted_supplier_bill_allocations_branch_insert ON hosted_supplier_bill_allocations FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_supplier_bills b WHERE b.tenant_id=hosted_supplier_bill_allocations.tenant_id AND b.id=hosted_supplier_bill_allocations.bill_id)
    AND EXISTS (SELECT 1 FROM hosted_purchase_receipts r WHERE r.tenant_id=hosted_supplier_bill_allocations.tenant_id AND r.branch_id=hosted_supplier_bill_allocations.branch_id AND r.id=hosted_supplier_bill_allocations.receipt_id)
);
