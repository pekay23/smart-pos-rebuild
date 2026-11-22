-- Optimistic revisions support safe customer edits, archive, and restore.
ALTER TABLE hosted_customers
    ADD COLUMN revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0);

DROP POLICY hosted_customers_branch_read ON hosted_customers;
CREATE POLICY hosted_customers_branch_read ON hosted_customers FOR SELECT USING (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND (
        status='active'
        OR (status='archived' AND EXISTS (
            SELECT 1 FROM memberships m JOIN membership_role_grants g
              ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
            WHERE m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.tenant_id=hosted_customers.tenant_id AND m.status='active'
              AND g.role_key IN ('owner','admin','manager')
              AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid)
              AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid)
        ))
    )
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

CREATE POLICY hosted_customers_branch_update ON hosted_customers FOR UPDATE
USING (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND status IN ('active','archived')
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND m.tenant_id=hosted_customers.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid)
          AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid)
    )
)
WITH CHECK (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND status IN ('active','archived')
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND m.tenant_id=hosted_customers.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid)
          AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid)
    )
);

CREATE INDEX hosted_customers_tenant_updated ON hosted_customers (tenant_id, updated_at DESC, id);
