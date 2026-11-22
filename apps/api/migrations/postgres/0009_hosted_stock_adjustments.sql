-- Stock counts must update the branch balance and append a movement together.
ALTER TABLE stock_movements
    ADD COLUMN resulting_stock_quantity BIGINT
        CHECK (resulting_stock_quantity IS NULL OR resulting_stock_quantity >= 0);

CREATE POLICY branch_catalog_items_inventory_update ON branch_catalog_items
    FOR UPDATE
    USING (
        branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.tenant_id = branch_catalog_items.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND g.role_key IN ('owner', 'admin', 'manager')
              AND (g.branch_id IS NULL OR g.branch_id = branch_catalog_items.branch_id)
              AND g.station_id IS NULL
        )
    )
    WITH CHECK (
        branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.tenant_id = branch_catalog_items.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND g.role_key IN ('owner', 'admin', 'manager')
              AND (g.branch_id IS NULL OR g.branch_id = branch_catalog_items.branch_id)
              AND g.station_id IS NULL
        )
    );
