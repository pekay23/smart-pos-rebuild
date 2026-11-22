-- Branch-scoped purchase orders and append-only delivery receipts.
CREATE TABLE hosted_purchase_orders (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    supplier_id UUID,
    order_number TEXT NOT NULL CHECK (length(order_number) BETWEEN 3 AND 40),
    reference TEXT CHECK (reference IS NULL OR length(reference) <= 100),
    status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','placed','partially_received','received','cancelled')),
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_by_membership_id UUID NOT NULL,
    creation_idempotency_key TEXT NOT NULL CHECK (length(creation_idempotency_key) BETWEEN 1 AND 128),
    creation_request_hash BYTEA NOT NULL CHECK (octet_length(creation_request_hash)=32),
    placed_at TIMESTAMPTZ,
    cancelled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (branch_id, tenant_id) REFERENCES branches(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, supplier_id) REFERENCES hosted_suppliers(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (created_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, branch_id, id),
    UNIQUE (tenant_id, branch_id, order_number),
    UNIQUE (tenant_id, branch_id, created_by_membership_id, creation_idempotency_key),
    CHECK ((status='draft' AND placed_at IS NULL AND cancelled_at IS NULL)
        OR (status='placed' AND placed_at IS NOT NULL AND cancelled_at IS NULL)
        OR (status='partially_received' AND placed_at IS NOT NULL AND cancelled_at IS NULL)
        OR (status='received' AND placed_at IS NOT NULL AND cancelled_at IS NULL)
        OR (status='cancelled' AND cancelled_at IS NOT NULL))
);
CREATE INDEX hosted_purchase_orders_branch_queue ON hosted_purchase_orders(tenant_id, branch_id, created_at DESC, id);

CREATE TABLE hosted_purchase_order_lines (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    order_id UUID NOT NULL,
    product_id UUID NOT NULL,
    sku_snapshot TEXT NOT NULL CHECK (length(btrim(sku_snapshot)) BETWEEN 1 AND 64),
    name_snapshot TEXT NOT NULL CHECK (length(btrim(name_snapshot)) BETWEEN 1 AND 160),
    ordered_quantity BIGINT NOT NULL CHECK (ordered_quantity BETWEEN 1 AND 1000000),
    received_quantity BIGINT NOT NULL DEFAULT 0 CHECK (received_quantity BETWEEN 0 AND ordered_quantity),
    expected_unit_cost_minor BIGINT NOT NULL CHECK (expected_unit_cost_minor >= 0),
    FOREIGN KEY (tenant_id, branch_id, order_id) REFERENCES hosted_purchase_orders(tenant_id, branch_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, branch_id, product_id) REFERENCES branch_catalog_items(tenant_id, branch_id, product_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, order_id, product_id)
);
CREATE INDEX hosted_purchase_order_lines_order ON hosted_purchase_order_lines(tenant_id, branch_id, order_id);

CREATE TABLE hosted_purchase_receipts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    order_id UUID NOT NULL,
    supplier_id UUID,
    actor_membership_id UUID NOT NULL,
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
    reference TEXT CHECK (reference IS NULL OR length(reference) <= 100),
    total_minor BIGINT NOT NULL CHECK (total_minor >= 0),
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, branch_id, order_id) REFERENCES hosted_purchase_orders(tenant_id, branch_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, supplier_id) REFERENCES hosted_suppliers(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, branch_id, id),
    UNIQUE (tenant_id, branch_id, actor_membership_id, idempotency_key)
);
CREATE INDEX hosted_purchase_receipts_order ON hosted_purchase_receipts(tenant_id, branch_id, order_id, received_at DESC);

CREATE TABLE hosted_purchase_receipt_lines (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    receipt_id UUID NOT NULL,
    order_line_id UUID NOT NULL,
    product_id UUID NOT NULL,
    quantity BIGINT NOT NULL CHECK (quantity BETWEEN 1 AND 1000000),
    actual_unit_cost_minor BIGINT NOT NULL CHECK (actual_unit_cost_minor >= 0),
    line_total_minor BIGINT NOT NULL CHECK (line_total_minor >= 0),
    FOREIGN KEY (tenant_id, branch_id, receipt_id) REFERENCES hosted_purchase_receipts(tenant_id, branch_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, order_line_id) REFERENCES hosted_purchase_order_lines(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, product_id) REFERENCES catalog_products(tenant_id, id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, receipt_id, order_line_id)
);
CREATE INDEX hosted_purchase_receipt_lines_receipt ON hosted_purchase_receipt_lines(tenant_id, branch_id, receipt_id);

DO $tables$
DECLARE table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY['hosted_purchase_orders','hosted_purchase_order_lines','hosted_purchase_receipts','hosted_purchase_receipt_lines'] LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', table_name);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', table_name);
    END LOOP;
END
$tables$;

CREATE POLICY hosted_purchase_orders_branch_read ON hosted_purchase_orders FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.tenant_id=hosted_purchase_orders.tenant_id AND m.status='active' AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=hosted_purchase_orders.branch_id) AND g.station_id IS NULL)
);
CREATE POLICY hosted_purchase_orders_branch_insert ON hosted_purchase_orders FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND created_by_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND status='draft' AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=created_by_membership_id AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
        AND m.tenant_id=hosted_purchase_orders.tenant_id AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=hosted_purchase_orders.branch_id) AND g.station_id IS NULL)
);
CREATE POLICY hosted_purchase_orders_branch_update ON hosted_purchase_orders FOR UPDATE USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.tenant_id=hosted_purchase_orders.tenant_id AND m.status='active' AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=hosted_purchase_orders.branch_id) AND g.station_id IS NULL)
) WITH CHECK (tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid);

CREATE POLICY hosted_purchase_order_lines_branch_read ON hosted_purchase_order_lines FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_purchase_orders o WHERE o.tenant_id=hosted_purchase_order_lines.tenant_id AND o.id=hosted_purchase_order_lines.order_id)
);
CREATE POLICY hosted_purchase_order_lines_branch_insert ON hosted_purchase_order_lines FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_purchase_orders o WHERE o.tenant_id=hosted_purchase_order_lines.tenant_id AND o.id=hosted_purchase_order_lines.order_id AND o.status='draft')
    AND EXISTS (SELECT 1 FROM branch_catalog_items i JOIN catalog_products p ON p.tenant_id=i.tenant_id AND p.id=i.product_id
      WHERE i.tenant_id=hosted_purchase_order_lines.tenant_id AND i.branch_id=hosted_purchase_order_lines.branch_id
        AND i.product_id=hosted_purchase_order_lines.product_id AND i.status='active' AND p.status='active')
);
CREATE POLICY hosted_purchase_order_lines_branch_update ON hosted_purchase_order_lines FOR UPDATE USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_purchase_orders o WHERE o.tenant_id=hosted_purchase_order_lines.tenant_id AND o.id=hosted_purchase_order_lines.order_id AND o.status='draft')
);
CREATE POLICY hosted_purchase_order_lines_draft_delete ON hosted_purchase_order_lines FOR DELETE USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_purchase_orders o WHERE o.tenant_id=hosted_purchase_order_lines.tenant_id AND o.id=hosted_purchase_order_lines.order_id AND o.status='draft')
);
CREATE POLICY hosted_purchase_order_lines_receipt_update ON hosted_purchase_order_lines FOR UPDATE USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_purchase_orders o WHERE o.tenant_id=hosted_purchase_order_lines.tenant_id AND o.id=hosted_purchase_order_lines.order_id AND o.status IN ('placed','partially_received'))
);

CREATE POLICY hosted_purchase_receipts_branch_read ON hosted_purchase_receipts FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_purchase_orders o WHERE o.tenant_id=hosted_purchase_receipts.tenant_id AND o.id=hosted_purchase_receipts.order_id)
);
CREATE POLICY hosted_purchase_receipts_branch_insert ON hosted_purchase_receipts FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_purchase_orders o WHERE o.tenant_id=hosted_purchase_receipts.tenant_id AND o.id=hosted_purchase_receipts.order_id
      AND o.status IN ('placed','partially_received'))
);
CREATE POLICY hosted_purchase_receipt_lines_branch_read ON hosted_purchase_receipt_lines FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_purchase_receipts r WHERE r.tenant_id=hosted_purchase_receipt_lines.tenant_id AND r.id=hosted_purchase_receipt_lines.receipt_id)
);
CREATE POLICY hosted_purchase_receipt_lines_branch_insert ON hosted_purchase_receipt_lines FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM hosted_purchase_receipts r WHERE r.tenant_id=hosted_purchase_receipt_lines.tenant_id AND r.id=hosted_purchase_receipt_lines.receipt_id)
);
