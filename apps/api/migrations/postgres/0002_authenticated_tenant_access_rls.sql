-- Bind each request to identity values verified by the API. Context uses SET LOCAL
-- (set_config(..., true)) so pooled connections cannot carry authority forward.

ALTER TABLE person_identities ENABLE ROW LEVEL SECURITY;
ALTER TABLE person_identities FORCE ROW LEVEL SECURITY;
CREATE POLICY identity_verified_self_read ON person_identities
    FOR SELECT
    USING (
        issuer = current_setting('pos.verified_issuer', true)
        AND subject = current_setting('pos.verified_subject', true)
    );

ALTER TABLE tenants ENABLE ROW LEVEL SECURITY;
ALTER TABLE tenants FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_active_members_read ON tenants
    FOR SELECT
    USING (
        status = 'active' AND EXISTS (
            SELECT 1
            FROM memberships m
            WHERE m.tenant_id = tenants.id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
    );

ALTER TABLE memberships ENABLE ROW LEVEL SECURITY;
ALTER TABLE memberships FORCE ROW LEVEL SECURITY;
CREATE POLICY membership_self_read ON memberships
    FOR SELECT
    USING (
        identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
    );

ALTER TABLE membership_role_grants ENABLE ROW LEVEL SECURITY;
ALTER TABLE membership_role_grants FORCE ROW LEVEL SECURITY;
CREATE POLICY membership_role_grant_self_read ON membership_role_grants
    FOR SELECT
    USING (
        EXISTS (
            SELECT 1
            FROM memberships m
            WHERE m.id = membership_role_grants.membership_id
              AND m.tenant_id = membership_role_grants.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
        AND EXISTS (
            SELECT 1 FROM tenants t
            WHERE t.id = membership_role_grants.tenant_id AND t.status = 'active'
        )
    );

ALTER TABLE branches ENABLE ROW LEVEL SECURITY;
ALTER TABLE branches FORCE ROW LEVEL SECURITY;
CREATE POLICY granted_branch_read ON branches
    FOR SELECT
    USING (
        EXISTS (
            SELECT 1
            FROM membership_role_grants g
            JOIN memberships m
              ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            JOIN tenants t ON t.id = branches.tenant_id AND t.status = 'active'
            WHERE g.tenant_id = branches.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND (g.branch_id IS NULL OR g.branch_id = branches.id)
        )
    );

ALTER TABLE stations ENABLE ROW LEVEL SECURITY;
ALTER TABLE stations FORCE ROW LEVEL SECURITY;
CREATE POLICY granted_station_read ON stations
    FOR SELECT
    USING (
        EXISTS (
            SELECT 1
            FROM membership_role_grants g
            JOIN memberships m
              ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            JOIN branches b
              ON b.id = stations.branch_id AND b.tenant_id = stations.tenant_id AND b.status = 'active'
            JOIN tenants t ON t.id = stations.tenant_id AND t.status = 'active'
            WHERE g.tenant_id = stations.tenant_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND (g.branch_id IS NULL OR g.branch_id = stations.branch_id)
              AND (g.station_id IS NULL OR g.station_id = stations.id)
        )
    );

-- Audit rows remain inaccessible through the runtime role until an explicit
-- permission-aware read/write policy and API command are designed.
ALTER TABLE audit_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_events FORCE ROW LEVEL SECURITY;
