-- Tenant-wide product identities with branch-specific price and stock.
-- Stock is updated only by a transaction that also appends a stock movement.
ALTER TABLE branches
    ADD COLUMN currency_code CHAR(3) NOT NULL DEFAULT 'GHS',
    ADD CONSTRAINT branches_currency_code_check
        CHECK (currency_code ~ '^[A-Z]{3}$');

CREATE TABLE catalog_products (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    sku TEXT NOT NULL CHECK (length(btrim(sku)) BETWEEN 1 AND 64),
    barcode TEXT CHECK (barcode IS NULL OR length(btrim(barcode)) BETWEEN 1 AND 64),
    name TEXT NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 160),
    description TEXT CHECK (description IS NULL OR length(description) <= 2000),
    category TEXT CHECK (category IS NULL OR length(btrim(category)) BETWEEN 1 AND 120),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived')),
    created_by_membership_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (created_by_membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, id)
);
CREATE UNIQUE INDEX catalog_products_tenant_sku_unique
    ON catalog_products (tenant_id, lower(btrim(sku)));
CREATE UNIQUE INDEX catalog_products_tenant_barcode_unique
    ON catalog_products (tenant_id, lower(btrim(barcode))) WHERE barcode IS NOT NULL;
CREATE INDEX catalog_products_tenant_name
    ON catalog_products (tenant_id, lower(name), id);

CREATE TABLE branch_catalog_items (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    product_id UUID NOT NULL,
    price_minor BIGINT NOT NULL CHECK (price_minor >= 0),
    cost_minor BIGINT CHECK (cost_minor IS NULL OR cost_minor >= 0),
    tax_rate_bps INTEGER NOT NULL DEFAULT 0 CHECK (tax_rate_bps BETWEEN 0 AND 10000),
    stock_quantity BIGINT NOT NULL DEFAULT 0 CHECK (stock_quantity >= 0),
    reorder_point BIGINT NOT NULL DEFAULT 0 CHECK (reorder_point >= 0),
    target_stock_quantity BIGINT NOT NULL DEFAULT 0 CHECK (
        target_stock_quantity = 0 OR target_stock_quantity >= reorder_point
    ),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'inactive')),
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (branch_id, tenant_id)
        REFERENCES branches(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (product_id, tenant_id)
        REFERENCES catalog_products(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, branch_id, product_id),
    UNIQUE (tenant_id, id)
);
CREATE INDEX branch_catalog_items_register_lookup
    ON branch_catalog_items (tenant_id, branch_id, status, product_id);
CREATE INDEX branch_catalog_items_low_stock
    ON branch_catalog_items (tenant_id, branch_id, stock_quantity, reorder_point)
    WHERE status = 'active';

CREATE TABLE stock_movements (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    product_id UUID NOT NULL,
    actor_membership_id UUID NOT NULL,
    movement_kind TEXT NOT NULL CHECK (movement_kind IN (
        'opening_balance', 'adjustment', 'sale', 'purchase_receipt',
        'transfer_in', 'transfer_out', 'sale_return'
    )),
    quantity_delta BIGINT NOT NULL CHECK (quantity_delta <> 0),
    note TEXT CHECK (note IS NULL OR length(note) <= 500),
    source_id UUID,
    idempotency_key TEXT CHECK (idempotency_key IS NULL OR length(idempotency_key) BETWEEN 1 AND 128),
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (branch_id, tenant_id)
        REFERENCES branches(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (product_id, tenant_id)
        REFERENCES catalog_products(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT
);
CREATE UNIQUE INDEX stock_movements_idempotency_unique
    ON stock_movements (tenant_id, branch_id, actor_membership_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
CREATE INDEX stock_movements_product_history
    ON stock_movements (tenant_id, branch_id, product_id, occurred_at DESC, id);

ALTER TABLE catalog_products ENABLE ROW LEVEL SECURITY;
ALTER TABLE catalog_products FORCE ROW LEVEL SECURITY;
ALTER TABLE branch_catalog_items ENABLE ROW LEVEL SECURITY;
ALTER TABLE branch_catalog_items FORCE ROW LEVEL SECURITY;
ALTER TABLE stock_movements ENABLE ROW LEVEL SECURITY;
ALTER TABLE stock_movements FORCE ROW LEVEL SECURITY;

CREATE POLICY catalog_products_branch_read ON catalog_products
    FOR SELECT USING (
        EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.tenant_id = catalog_products.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND g.role_key IN ('owner', 'admin', 'manager', 'cashier')
              AND (g.branch_id IS NULL OR g.branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid)
              AND (g.station_id IS NULL OR g.station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid)
        )
    );
CREATE POLICY catalog_products_branch_insert ON catalog_products
    FOR INSERT WITH CHECK (
        EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.id = catalog_products.created_by_membership_id
              AND m.tenant_id = catalog_products.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND g.role_key IN ('owner', 'admin', 'manager')
              AND (g.branch_id IS NULL OR g.branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid)
              AND g.station_id IS NULL
        )
    );

CREATE POLICY branch_catalog_items_branch_read ON branch_catalog_items
    FOR SELECT USING (
        branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.tenant_id = branch_catalog_items.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND g.role_key IN ('owner', 'admin', 'manager', 'cashier')
              AND (g.branch_id IS NULL OR g.branch_id = branch_catalog_items.branch_id)
              AND (g.station_id IS NULL OR g.station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid)
        )
    );
CREATE POLICY branch_catalog_items_management_insert ON branch_catalog_items
    FOR INSERT WITH CHECK (
        branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.id = current_setting('pos.membership_id', true)::uuid
              AND m.tenant_id = branch_catalog_items.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND g.role_key IN ('owner', 'admin', 'manager')
              AND (g.branch_id IS NULL OR g.branch_id = branch_catalog_items.branch_id)
              AND g.station_id IS NULL
        )
    );

CREATE POLICY stock_movements_branch_read ON stock_movements
    FOR SELECT USING (
        branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.tenant_id = stock_movements.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND (g.branch_id IS NULL OR g.branch_id = stock_movements.branch_id)
        )
    );
CREATE POLICY stock_movements_management_insert ON stock_movements
    FOR INSERT WITH CHECK (
        branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND actor_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.id = stock_movements.actor_membership_id
              AND m.tenant_id = stock_movements.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND g.role_key IN ('owner', 'admin', 'manager')
              AND (g.branch_id IS NULL OR g.branch_id = stock_movements.branch_id)
              AND g.station_id IS NULL
        )
    );

-- Audit inserts are tenant scoped and must name the current active member.
CREATE POLICY audit_event_active_member_insert ON audit_events
    FOR INSERT WITH CHECK (
        actor_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            WHERE m.id = audit_events.actor_membership_id
              AND m.tenant_id = audit_events.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
    );
