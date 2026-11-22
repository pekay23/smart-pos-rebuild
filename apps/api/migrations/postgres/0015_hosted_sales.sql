-- Atomic hosted sales with immutable price/tax/tender snapshots.
-- Cash tenders are recorded only after the hosted drawer workflow exists.
CREATE UNIQUE INDEX branch_payment_options_tenant_branch_id_unique
    ON branch_payment_options (tenant_id, branch_id, id);

CREATE TABLE hosted_sales (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    station_id UUID NOT NULL,
    cashier_membership_id UUID NOT NULL,
    lease_id UUID NOT NULL,
    activation_id UUID NOT NULL,
    receipt_number TEXT NOT NULL CHECK (length(btrim(receipt_number)) BETWEEN 1 AND 48),
    cashier_name_snapshot TEXT NOT NULL CHECK (length(btrim(cashier_name_snapshot)) BETWEEN 1 AND 160),
    station_name_snapshot TEXT NOT NULL CHECK (length(btrim(station_name_snapshot)) BETWEEN 1 AND 160),
    currency_code CHAR(3) NOT NULL CHECK (currency_code ~ '^[A-Z]{3}$'),
    subtotal_minor BIGINT NOT NULL CHECK (subtotal_minor >= 0),
    tax_minor BIGINT NOT NULL CHECK (tax_minor >= 0),
    total_minor BIGINT NOT NULL CHECK (total_minor >= subtotal_minor AND tax_minor = total_minor - subtotal_minor),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash) = 32),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    status TEXT NOT NULL DEFAULT 'completed' CHECK (status IN ('completed', 'partially_refunded', 'refunded')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (station_id, branch_id, tenant_id)
        REFERENCES stations(id, branch_id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (cashier_membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, lease_id)
        REFERENCES station_actor_leases(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, activation_id)
        REFERENCES station_activations(tenant_id, id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, branch_id, id),
    UNIQUE (tenant_id, branch_id, receipt_number),
    UNIQUE (tenant_id, station_id, cashier_membership_id, idempotency_key)
);
CREATE INDEX hosted_sales_branch_created ON hosted_sales (tenant_id, branch_id, created_at DESC, id);
CREATE INDEX hosted_sales_station_created ON hosted_sales (tenant_id, station_id, created_at DESC, id);

CREATE TABLE hosted_sale_lines (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    sale_id UUID NOT NULL,
    line_number INTEGER NOT NULL CHECK (line_number > 0),
    product_id UUID NOT NULL,
    sku_snapshot TEXT NOT NULL CHECK (length(btrim(sku_snapshot)) BETWEEN 1 AND 64),
    barcode_snapshot TEXT,
    name_snapshot TEXT NOT NULL CHECK (length(btrim(name_snapshot)) BETWEEN 1 AND 160),
    quantity BIGINT NOT NULL CHECK (quantity > 0),
    unit_price_minor BIGINT NOT NULL CHECK (unit_price_minor >= 0),
    tax_rate_bps INTEGER NOT NULL CHECK (tax_rate_bps BETWEEN 0 AND 10000),
    subtotal_minor BIGINT NOT NULL CHECK (subtotal_minor >= 0),
    tax_minor BIGINT NOT NULL CHECK (tax_minor >= 0),
    total_minor BIGINT NOT NULL CHECK (total_minor >= subtotal_minor AND tax_minor = total_minor - subtotal_minor),
    FOREIGN KEY (tenant_id, branch_id, sale_id)
        REFERENCES hosted_sales(tenant_id, branch_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, product_id)
        REFERENCES catalog_products(tenant_id, id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, sale_id, line_number)
);
CREATE INDEX hosted_sale_lines_sale ON hosted_sale_lines (tenant_id, sale_id, line_number);

CREATE TABLE hosted_sale_tenders (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    sale_id UUID NOT NULL,
    payment_option_id UUID NOT NULL,
    method_key_snapshot TEXT NOT NULL CHECK (length(method_key_snapshot) BETWEEN 2 AND 48),
    label_snapshot TEXT NOT NULL CHECK (length(btrim(label_snapshot)) BETWEEN 1 AND 96),
    tender_kind TEXT NOT NULL CHECK (tender_kind IN ('card', 'mobile_money', 'bank_transfer', 'other')),
    provider_name_snapshot TEXT,
    amount_minor BIGINT NOT NULL CHECK (amount_minor > 0),
    reference TEXT CHECK (reference IS NULL OR length(reference) <= 160),
    recording_mode TEXT NOT NULL DEFAULT 'manual' CHECK (recording_mode = 'manual'),
    approval_status TEXT NOT NULL DEFAULT 'cashier_confirmed' CHECK (approval_status = 'cashier_confirmed'),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, branch_id, sale_id)
        REFERENCES hosted_sales(tenant_id, branch_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, branch_id, payment_option_id)
        REFERENCES branch_payment_options(tenant_id, branch_id, id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, sale_id, id)
);
CREATE INDEX hosted_sale_tenders_sale ON hosted_sale_tenders (tenant_id, sale_id, recorded_at, id);

ALTER TABLE hosted_sales ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_sales FORCE ROW LEVEL SECURITY;
ALTER TABLE hosted_sale_lines ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_sale_lines FORCE ROW LEVEL SECURITY;
ALTER TABLE hosted_sale_tenders ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_sale_tenders FORCE ROW LEVEL SECURITY;

CREATE POLICY hosted_sales_branch_read ON hosted_sales
    FOR SELECT USING (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.tenant_id = hosted_sales.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND (g.branch_id IS NULL OR g.branch_id = hosted_sales.branch_id)
              AND (g.station_id IS NULL OR g.station_id = hosted_sales.station_id)
              AND (g.role_key IN ('owner', 'admin', 'manager')
                   OR (g.role_key = 'cashier' AND m.id = hosted_sales.cashier_membership_id))
        )
    );

CREATE POLICY hosted_sales_cashier_insert ON hosted_sales
    FOR INSERT WITH CHECK (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid
        AND cashier_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g
              ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            JOIN station_actor_leases l
              ON l.tenant_id = hosted_sales.tenant_id AND l.branch_id = hosted_sales.branch_id
             AND l.station_id = hosted_sales.station_id AND l.membership_id = hosted_sales.cashier_membership_id
             AND l.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
             AND l.id = hosted_sales.lease_id AND l.activation_id = hosted_sales.activation_id
            JOIN station_actor_lease_locks k
              ON k.tenant_id = l.tenant_id AND k.branch_id = l.branch_id
             AND k.station_id = l.station_id AND k.lease_id = l.id
            JOIN station_activations a
              ON a.tenant_id = l.tenant_id AND a.branch_id = l.branch_id
             AND a.station_id = l.station_id AND a.id = l.activation_id
            WHERE m.id = hosted_sales.cashier_membership_id
              AND m.tenant_id = hosted_sales.tenant_id
              AND m.identity_id = l.identity_id AND m.status = 'active'
              AND g.role_key = 'cashier'
              AND (g.branch_id IS NULL OR g.branch_id = hosted_sales.branch_id)
              AND (g.station_id IS NULL OR g.station_id = hosted_sales.station_id)
              AND l.status = 'issued' AND l.expires_at > now()
              AND a.status = 'active'
        )
    );

CREATE POLICY hosted_sale_lines_branch_read ON hosted_sale_lines
    FOR SELECT USING (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (SELECT 1 FROM hosted_sales s WHERE s.tenant_id = hosted_sale_lines.tenant_id
                    AND s.branch_id = hosted_sale_lines.branch_id AND s.id = hosted_sale_lines.sale_id)
    );
CREATE POLICY hosted_sale_lines_cashier_insert ON hosted_sale_lines
    FOR INSERT WITH CHECK (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (SELECT 1 FROM hosted_sales s WHERE s.tenant_id = hosted_sale_lines.tenant_id
                    AND s.branch_id = hosted_sale_lines.branch_id AND s.id = hosted_sale_lines.sale_id
                    AND s.cashier_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
                    AND s.station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid)
    );

CREATE POLICY hosted_sale_tenders_branch_read ON hosted_sale_tenders
    FOR SELECT USING (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (SELECT 1 FROM hosted_sales s WHERE s.tenant_id = hosted_sale_tenders.tenant_id
                    AND s.branch_id = hosted_sale_tenders.branch_id AND s.id = hosted_sale_tenders.sale_id)
    );
CREATE POLICY hosted_sale_tenders_cashier_insert ON hosted_sale_tenders
    FOR INSERT WITH CHECK (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (SELECT 1 FROM hosted_sales s WHERE s.tenant_id = hosted_sale_tenders.tenant_id
                    AND s.branch_id = hosted_sale_tenders.branch_id AND s.id = hosted_sale_tenders.sale_id
                    AND s.cashier_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
                    AND s.station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid)
    );

CREATE POLICY branch_catalog_items_cashier_sale_update ON branch_catalog_items
    FOR UPDATE USING (
        branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            JOIN station_actor_leases l
              ON l.tenant_id = branch_catalog_items.tenant_id AND l.branch_id = branch_catalog_items.branch_id
             AND l.station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid
             AND l.membership_id = m.id AND l.identity_id = m.identity_id
            JOIN station_actor_lease_locks k ON k.tenant_id = l.tenant_id AND k.branch_id = l.branch_id
             AND k.station_id = l.station_id AND k.lease_id = l.id
            JOIN station_activations a ON a.tenant_id = l.tenant_id AND a.branch_id = l.branch_id
             AND a.station_id = l.station_id AND a.id = l.activation_id
            WHERE m.id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.tenant_id = branch_catalog_items.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active' AND g.role_key = 'cashier'
              AND (g.branch_id IS NULL OR g.branch_id = branch_catalog_items.branch_id)
              AND (g.station_id IS NULL OR g.station_id = l.station_id)
              AND l.status = 'issued' AND l.expires_at > now() AND a.status = 'active'
        )
    ) WITH CHECK (
        branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND stock_quantity >= 0
    );

CREATE POLICY stock_movements_cashier_sale_insert ON stock_movements
    FOR INSERT WITH CHECK (
        movement_kind = 'sale' AND quantity_delta < 0 AND source_id IS NOT NULL
        AND tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND actor_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM hosted_sales s
            WHERE s.tenant_id = stock_movements.tenant_id AND s.branch_id = stock_movements.branch_id
              AND s.id = stock_movements.source_id
              AND s.cashier_membership_id = stock_movements.actor_membership_id
              AND s.station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid
        )
    );

