ALTER TABLE catalog_products
    ADD COLUMN revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0);

CREATE POLICY catalog_products_tenant_update ON catalog_products
    FOR UPDATE
    USING (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.tenant_id = catalog_products.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND g.branch_id IS NULL
              AND g.station_id IS NULL
        )
    )
    WITH CHECK (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.tenant_id = catalog_products.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND g.branch_id IS NULL
              AND g.station_id IS NULL
        )
    );
