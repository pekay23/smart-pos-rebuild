-- Allow owner/admin API requests to render a tenant staff directory and its
-- membership-keyed profile images. The API sets this transaction-local value
-- only after checking users:write; the runtime database role is not exposed.

ALTER TABLE hosted_entity_images
    DROP CONSTRAINT hosted_entity_images_entity_type_check;
ALTER TABLE hosted_entity_images
    ADD CONSTRAINT hosted_entity_images_entity_type_check
    CHECK (entity_type IN ('product','customer','supplier','staff'));

CREATE POLICY membership_staff_roster_read ON memberships
    FOR SELECT USING (
        tenant_id::text = current_setting('pos.staff_roster_tenant_id', true)
        AND status = 'active'
    );

CREATE POLICY membership_role_grant_staff_roster_read ON membership_role_grants
    FOR SELECT USING (
        tenant_id::text = current_setting('pos.staff_roster_tenant_id', true)
        AND EXISTS (
            SELECT 1 FROM memberships m
            WHERE m.id = membership_role_grants.membership_id
              AND m.tenant_id = membership_role_grants.tenant_id
              AND m.status = 'active'
        )
    );

CREATE POLICY identity_staff_roster_read ON person_identities
    FOR SELECT USING (
        EXISTS (
            SELECT 1 FROM memberships m
            WHERE m.identity_id = person_identities.id
              AND m.tenant_id::text = current_setting('pos.staff_roster_tenant_id', true)
              AND m.status = 'active'
        )
    );

CREATE POLICY hosted_staff_images_select ON hosted_entity_images
    FOR SELECT USING (
        entity_type = 'staff'
        AND tenant_id::text = current_setting('pos.tenant_id', true)
        AND EXISTS (
            SELECT 1 FROM memberships actor
            JOIN membership_role_grants grant_row
              ON grant_row.tenant_id = actor.tenant_id
             AND grant_row.membership_id = actor.id
            WHERE actor.id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND actor.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND actor.tenant_id = hosted_entity_images.tenant_id
              AND actor.status = 'active'
              AND grant_row.role_key IN ('owner','admin')
              AND grant_row.branch_id IS NULL
              AND grant_row.station_id IS NULL
        )
        AND EXISTS (
            SELECT 1 FROM memberships staff
            WHERE staff.tenant_id = hosted_entity_images.tenant_id
              AND staff.id = hosted_entity_images.entity_id
              AND staff.status = 'active'
        )
    );

CREATE POLICY hosted_staff_images_insert ON hosted_entity_images
    FOR INSERT WITH CHECK (
        entity_type = 'staff'
        AND tenant_id::text = current_setting('pos.tenant_id', true)
        AND created_by_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
        AND updated_by_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships actor
            JOIN membership_role_grants grant_row
              ON grant_row.tenant_id = actor.tenant_id
             AND grant_row.membership_id = actor.id
            WHERE actor.id = created_by_membership_id
              AND actor.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND actor.tenant_id = hosted_entity_images.tenant_id
              AND actor.status = 'active'
              AND grant_row.role_key IN ('owner','admin')
              AND grant_row.branch_id IS NULL
              AND grant_row.station_id IS NULL
        )
        AND EXISTS (
            SELECT 1 FROM memberships staff
            WHERE staff.tenant_id = hosted_entity_images.tenant_id
              AND staff.id = hosted_entity_images.entity_id
              AND staff.status = 'active'
        )
    );

CREATE POLICY hosted_staff_images_update ON hosted_entity_images
    FOR UPDATE USING (
        entity_type = 'staff'
        AND tenant_id::text = current_setting('pos.tenant_id', true)
        AND EXISTS (
            SELECT 1 FROM memberships actor
            JOIN membership_role_grants grant_row
              ON grant_row.tenant_id = actor.tenant_id
             AND grant_row.membership_id = actor.id
            WHERE actor.id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND actor.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND actor.tenant_id = hosted_entity_images.tenant_id
              AND actor.status = 'active'
              AND grant_row.role_key IN ('owner','admin')
              AND grant_row.branch_id IS NULL
              AND grant_row.station_id IS NULL
        )
    ) WITH CHECK (
        entity_type = 'staff'
        AND tenant_id::text = current_setting('pos.tenant_id', true)
        AND updated_by_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships staff
            WHERE staff.tenant_id = hosted_entity_images.tenant_id
              AND staff.id = hosted_entity_images.entity_id
              AND staff.status = 'active'
        )
    );

CREATE POLICY hosted_staff_images_delete ON hosted_entity_images
    FOR DELETE USING (
        entity_type = 'staff'
        AND tenant_id::text = current_setting('pos.tenant_id', true)
        AND EXISTS (
            SELECT 1 FROM memberships actor
            JOIN membership_role_grants grant_row
              ON grant_row.tenant_id = actor.tenant_id
             AND grant_row.membership_id = actor.id
            WHERE actor.id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND actor.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND actor.tenant_id = hosted_entity_images.tenant_id
              AND actor.status = 'active'
              AND grant_row.role_key IN ('owner','admin')
              AND grant_row.branch_id IS NULL
              AND grant_row.station_id IS NULL
        )
    );

CREATE POLICY hosted_staff_images_owner_admin_only ON hosted_entity_images
    AS RESTRICTIVE FOR ALL
    USING (
        entity_type <> 'staff' OR EXISTS (
            SELECT 1 FROM memberships actor
            JOIN membership_role_grants grant_row
              ON grant_row.tenant_id = actor.tenant_id
             AND grant_row.membership_id = actor.id
            WHERE actor.id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND actor.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND actor.tenant_id = hosted_entity_images.tenant_id
              AND actor.status = 'active'
              AND grant_row.role_key IN ('owner','admin')
              AND grant_row.branch_id IS NULL
              AND grant_row.station_id IS NULL
        )
    )
    WITH CHECK (
        entity_type <> 'staff' OR EXISTS (
            SELECT 1 FROM memberships actor
            JOIN membership_role_grants grant_row
              ON grant_row.tenant_id = actor.tenant_id
             AND grant_row.membership_id = actor.id
            WHERE actor.id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND actor.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND actor.tenant_id = hosted_entity_images.tenant_id
              AND actor.status = 'active'
              AND grant_row.role_key IN ('owner','admin')
              AND grant_row.branch_id IS NULL
              AND grant_row.station_id IS NULL
        )
    );
