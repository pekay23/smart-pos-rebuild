-- Branch contact details and opening hours are branch-owned settings. Runtime
-- updates remain permission checked by the API and branch scoped by RLS.
ALTER TABLE branches
    ADD COLUMN address TEXT CHECK (address IS NULL OR length(address) <= 500),
    ADD COLUMN phone TEXT CHECK (phone IS NULL OR length(phone) <= 40),
    ADD COLUMN email TEXT CHECK (email IS NULL OR length(email) <= 254),
    ADD COLUMN opening_hours JSONB NOT NULL DEFAULT '{"monday":[],"tuesday":[],"wednesday":[],"thursday":[],"friday":[],"saturday":[],"sunday":[]}'::jsonb,
    ADD COLUMN settings_revision BIGINT NOT NULL DEFAULT 1 CHECK (settings_revision > 0);

GRANT SELECT (address, phone, email, opening_hours, settings_revision)
    ON branches TO pos_api_runtime;
GRANT UPDATE (address, phone, email, opening_hours, settings_revision, updated_at)
    ON branches TO pos_api_runtime;

CREATE POLICY granted_branch_settings_update ON branches
    FOR UPDATE
    USING (
        EXISTS (
            SELECT 1
            FROM membership_role_grants g
            JOIN memberships m ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            JOIN tenants t ON t.id = branches.tenant_id AND t.status = 'active'
            WHERE g.tenant_id = branches.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND (g.branch_id IS NULL OR g.branch_id = branches.id)
        )
    )
    WITH CHECK (
        EXISTS (
            SELECT 1
            FROM membership_role_grants g
            JOIN memberships m ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            JOIN tenants t ON t.id = branches.tenant_id AND t.status = 'active'
            WHERE g.tenant_id = branches.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND (g.branch_id IS NULL OR g.branch_id = branches.id)
        )
    );
