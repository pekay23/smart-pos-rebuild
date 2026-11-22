-- Let a register look up stock at other branches after the API has authorized
-- inventory:availability:read for the assigned source station. The GUC is set
-- transaction-locally by that endpoint only; it never comes from the request.
CREATE POLICY branches_inventory_availability_read ON branches
    FOR SELECT USING (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND current_setting('pos.inventory_availability', true) = 'enabled'
        AND EXISTS (
            SELECT 1
            FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.tenant_id = branches.tenant_id
              AND m.id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND g.role_key IN ('owner', 'admin', 'manager', 'cashier')
              AND (g.branch_id IS NULL OR g.branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid)
              AND (g.station_id IS NULL OR g.station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid)
        )
    );

CREATE POLICY branch_catalog_items_inventory_availability_read ON branch_catalog_items
    FOR SELECT USING (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND current_setting('pos.inventory_availability', true) = 'enabled'
        AND EXISTS (
            SELECT 1
            FROM memberships m
            JOIN tenants t ON t.id = m.tenant_id AND t.status = 'active'
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
            WHERE m.tenant_id = branch_catalog_items.tenant_id
              AND m.id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND g.role_key IN ('owner', 'admin', 'manager', 'cashier')
              AND (g.branch_id IS NULL OR g.branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid)
              AND (g.station_id IS NULL OR g.station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid)
        )
    );
