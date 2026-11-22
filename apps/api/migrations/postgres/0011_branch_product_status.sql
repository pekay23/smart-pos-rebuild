-- Branch managers may hide or restore a product at one branch without
-- deleting its tenant-wide identity or changing other branches.
CREATE POLICY branch_catalog_items_management_update ON branch_catalog_items
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
