-- Retain who revoked a device binding and why so a lost/replaced till can be
-- safely deactivated before a new device is activated into the freed slot.
ALTER TABLE station_activations
    ADD COLUMN revoked_by_membership_id UUID,
    ADD COLUMN revocation_reason TEXT,
    ADD CONSTRAINT station_activation_revoker_membership_fk
        FOREIGN KEY (revoked_by_membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    ADD CONSTRAINT station_activation_revocation_details_check CHECK (
        (status = 'active'
            AND revoked_at IS NULL
            AND revoked_by_membership_id IS NULL
            AND revocation_reason IS NULL)
        OR
        (status = 'revoked'
            AND revoked_at IS NOT NULL
            AND (
                (revoked_by_membership_id IS NULL AND revocation_reason IS NULL)
                OR
                (revoked_by_membership_id IS NOT NULL
                    AND revocation_reason IS NOT NULL
                    AND char_length(btrim(revocation_reason)) BETWEEN 3 AND 500)
            ))
    );

-- Explicitly close any hosted cashier lease and drop its lock when an owner
-- revokes the station. Keep the actor and token history while recording the
-- forced terminal state, timestamps and revocation reason.
ALTER TABLE station_actor_leases
    ADD COLUMN ended_reason TEXT,
    ADD CONSTRAINT station_actor_lease_ended_reason_check CHECK (
        ended_reason IS NULL OR ended_reason IN ('cashier_logout', 'station_activation_revoked')
    );

DROP POLICY station_activation_owner_update ON station_activations;
CREATE POLICY station_activation_owner_revoke ON station_activations
    FOR UPDATE
    USING (
        status = 'active'
        AND EXISTS (
            SELECT 1 FROM membership_role_grants g
            JOIN memberships m ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            WHERE g.tenant_id = station_activations.tenant_id
              AND g.role_key = 'owner' AND g.branch_id IS NULL AND g.station_id IS NULL
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
    )
    WITH CHECK (
        status = 'revoked' AND revoked_at IS NOT NULL
        AND revoked_by_membership_id IS NOT NULL AND revocation_reason IS NOT NULL
        AND EXISTS (
            SELECT 1 FROM membership_role_grants g
            JOIN memberships m ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            WHERE g.tenant_id = station_activations.tenant_id
              AND g.role_key = 'owner' AND g.branch_id IS NULL AND g.station_id IS NULL
              AND m.id = station_activations.revoked_by_membership_id
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
    );

-- Revocation cancels outstanding challenges for the device being removed.
-- Owners may cancel challenges created by any owner membership in the tenant.
CREATE POLICY station_activation_challenge_owner_cancel ON station_activation_challenges
    FOR UPDATE
    USING (
        consumed_at IS NULL
        AND EXISTS (
            SELECT 1 FROM membership_role_grants g
            JOIN memberships m ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            WHERE g.tenant_id = station_activation_challenges.tenant_id
              AND g.role_key = 'owner' AND g.branch_id IS NULL AND g.station_id IS NULL
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
    )
    WITH CHECK (
        consumed_at IS NOT NULL
        AND EXISTS (
            SELECT 1 FROM membership_role_grants g
            JOIN memberships m ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            WHERE g.tenant_id = station_activation_challenges.tenant_id
              AND g.role_key = 'owner' AND g.branch_id IS NULL AND g.station_id IS NULL
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
    );

CREATE POLICY station_actor_lease_owner_revoke ON station_actor_leases
    FOR UPDATE
    USING (
        status = 'issued'
        AND EXISTS (
            SELECT 1 FROM membership_role_grants g
            JOIN memberships m ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            WHERE g.tenant_id = station_actor_leases.tenant_id
              AND g.role_key = 'owner' AND g.branch_id IS NULL AND g.station_id IS NULL
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
    )
    WITH CHECK (
        status = 'released' AND ended_at IS NOT NULL
        AND expires_at <= now() AND ended_reason = 'station_activation_revoked'
        AND EXISTS (
            SELECT 1 FROM membership_role_grants g
            JOIN memberships m ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            WHERE g.tenant_id = station_actor_leases.tenant_id
              AND g.role_key = 'owner' AND g.branch_id IS NULL AND g.station_id IS NULL
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
    );

CREATE POLICY station_actor_lock_owner_delete ON station_actor_lease_locks
    FOR DELETE USING (
        EXISTS (
            SELECT 1 FROM station_actor_leases l
            JOIN membership_role_grants g ON g.tenant_id = l.tenant_id
            JOIN memberships m ON m.id = g.membership_id AND m.tenant_id = g.tenant_id
            WHERE l.id = station_actor_lease_locks.lease_id
              AND l.tenant_id = station_actor_lease_locks.tenant_id
              AND l.station_id = station_actor_lease_locks.station_id
              AND g.role_key = 'owner' AND g.branch_id IS NULL AND g.station_id IS NULL
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
        )
    );
