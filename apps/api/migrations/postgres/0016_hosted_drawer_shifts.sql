-- Station-owned hosted cash drawer shifts and immutable Z close reports.
ALTER TABLE station_financial_proof_challenges
    DROP CONSTRAINT station_financial_proof_challenges_operation_check;
ALTER TABLE station_financial_proof_challenges
    ADD CONSTRAINT station_financial_proof_challenges_operation_check
    CHECK (operation IN ('sale.commit', 'drawer.open', 'drawer.movement', 'drawer.close'));

CREATE TABLE hosted_drawer_shifts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    station_id UUID NOT NULL,
    opened_by_membership_id UUID NOT NULL,
    closed_by_membership_id UUID,
    opening_lease_id UUID NOT NULL,
    opening_activation_id UUID NOT NULL,
    closing_lease_id UUID,
    opening_idempotency_key TEXT NOT NULL CHECK (length(opening_idempotency_key) BETWEEN 1 AND 128),
    opening_request_hash BYTEA NOT NULL CHECK (octet_length(opening_request_hash) = 32),
    close_idempotency_key TEXT,
    close_request_hash BYTEA,
    opening_cash_minor BIGINT NOT NULL CHECK (opening_cash_minor >= 0),
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','closed')),
    opened_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    closed_at TIMESTAMPTZ,
    counted_cash_minor BIGINT CHECK (counted_cash_minor IS NULL OR counted_cash_minor >= 0),
    expected_cash_minor BIGINT,
    difference_minor BIGINT,
    report_json JSONB,
    FOREIGN KEY (station_id, branch_id, tenant_id)
        REFERENCES stations(id, branch_id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (opened_by_membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (closed_by_membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, opening_lease_id)
        REFERENCES station_actor_leases(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, opening_activation_id)
        REFERENCES station_activations(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, closing_lease_id)
        REFERENCES station_actor_leases(tenant_id, id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, branch_id, station_id, id),
    CHECK ((status = 'open' AND closed_at IS NULL AND closed_by_membership_id IS NULL
            AND close_idempotency_key IS NULL AND close_request_hash IS NULL
            AND counted_cash_minor IS NULL AND expected_cash_minor IS NULL AND report_json IS NULL)
        OR (status = 'closed' AND closed_at IS NOT NULL AND closed_by_membership_id IS NOT NULL
            AND closing_lease_id IS NOT NULL AND counted_cash_minor IS NOT NULL
            AND close_idempotency_key IS NOT NULL AND close_request_hash IS NOT NULL
            AND expected_cash_minor IS NOT NULL AND difference_minor IS NOT NULL AND report_json IS NOT NULL))
);
CREATE UNIQUE INDEX hosted_drawer_one_open_per_station
    ON hosted_drawer_shifts (tenant_id, station_id) WHERE status = 'open';
CREATE INDEX hosted_drawer_history ON hosted_drawer_shifts (tenant_id, branch_id, closed_at DESC, id)
    WHERE status = 'closed';

CREATE TABLE hosted_drawer_movements (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    station_id UUID NOT NULL,
    shift_id UUID NOT NULL,
    actor_membership_id UUID NOT NULL,
    lease_id UUID NOT NULL,
    activation_id UUID NOT NULL,
    movement_type TEXT NOT NULL CHECK (movement_type IN ('paid_in','paid_out','cash_drop')),
    amount_minor BIGINT NOT NULL CHECK (amount_minor > 0 AND amount_minor <= 9000000000000),
    note TEXT NOT NULL CHECK (length(btrim(note)) BETWEEN 1 AND 200),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, branch_id, station_id, shift_id)
        REFERENCES hosted_drawer_shifts(tenant_id, branch_id, station_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, lease_id)
        REFERENCES station_actor_leases(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, activation_id)
        REFERENCES station_activations(tenant_id, id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, station_id, actor_membership_id, idempotency_key)
);
CREATE INDEX hosted_drawer_movements_shift ON hosted_drawer_movements (tenant_id, shift_id, created_at, id);

ALTER TABLE hosted_sales
    ADD COLUMN cash_shift_id UUID;
ALTER TABLE hosted_sales
    ADD CONSTRAINT hosted_sales_cash_shift_fk
    FOREIGN KEY (tenant_id, branch_id, station_id, cash_shift_id)
    REFERENCES hosted_drawer_shifts(tenant_id, branch_id, station_id, id) ON DELETE RESTRICT;
ALTER TABLE hosted_sale_tenders
    DROP CONSTRAINT hosted_sale_tenders_tender_kind_check;
ALTER TABLE hosted_sale_tenders
    ADD CONSTRAINT hosted_sale_tenders_tender_kind_check
    CHECK (tender_kind IN ('cash','card','mobile_money','bank_transfer','other'));

ALTER TABLE hosted_drawer_shifts ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_drawer_shifts FORCE ROW LEVEL SECURITY;
ALTER TABLE hosted_drawer_movements ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_drawer_movements FORCE ROW LEVEL SECURITY;

CREATE POLICY hosted_drawer_shift_read ON hosted_drawer_shifts FOR SELECT USING (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.tenant_id=hosted_drawer_shifts.tenant_id
          AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND m.status='active'
          AND (g.branch_id IS NULL OR g.branch_id=hosted_drawer_shifts.branch_id)
          AND (g.station_id IS NULL OR g.station_id=hosted_drawer_shifts.station_id)
          AND g.role_key IN ('owner','admin','manager','cashier')
    )
);
CREATE POLICY hosted_drawer_shift_insert ON hosted_drawer_shifts FOR INSERT WITH CHECK (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
    AND station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid
    AND opened_by_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
    AND status='open'
    AND EXISTS (
        SELECT 1 FROM station_actor_leases l
        JOIN station_actor_lease_locks k ON k.tenant_id=l.tenant_id AND k.branch_id=l.branch_id
          AND k.station_id=l.station_id AND k.lease_id=l.id
        JOIN station_activations a ON a.tenant_id=l.tenant_id AND a.branch_id=l.branch_id
          AND a.station_id=l.station_id AND a.id=l.activation_id
        JOIN membership_role_grants g ON g.tenant_id=l.tenant_id AND g.membership_id=l.membership_id
        WHERE l.tenant_id=hosted_drawer_shifts.tenant_id AND l.branch_id=hosted_drawer_shifts.branch_id
          AND l.station_id=hosted_drawer_shifts.station_id AND l.id=hosted_drawer_shifts.opening_lease_id
          AND l.activation_id=hosted_drawer_shifts.opening_activation_id
          AND l.membership_id=hosted_drawer_shifts.opened_by_membership_id
          AND l.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND l.status='issued' AND l.expires_at>now() AND a.status='active' AND g.role_key='cashier'
          AND (g.branch_id IS NULL OR g.branch_id=l.branch_id)
          AND (g.station_id IS NULL OR g.station_id=l.station_id)
    )
);
CREATE POLICY hosted_drawer_shift_close ON hosted_drawer_shifts FOR UPDATE USING (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
    AND station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid
    AND status='open'
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        JOIN station_actor_leases l ON l.tenant_id=m.tenant_id AND l.membership_id=m.id
          AND l.branch_id=hosted_drawer_shifts.branch_id AND l.station_id=hosted_drawer_shifts.station_id
          AND l.identity_id=m.identity_id
        JOIN station_actor_lease_locks k ON k.tenant_id=l.tenant_id AND k.branch_id=l.branch_id
          AND k.station_id=l.station_id AND k.lease_id=l.id
        JOIN station_activations a ON a.tenant_id=l.tenant_id AND a.branch_id=l.branch_id
          AND a.station_id=l.station_id AND a.id=l.activation_id
        WHERE m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
          AND m.tenant_id=hosted_drawer_shifts.tenant_id
          AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND m.status='active' AND g.role_key='cashier'
          AND (g.branch_id IS NULL OR g.branch_id=l.branch_id)
          AND (g.station_id IS NULL OR g.station_id=l.station_id)
          AND l.status='issued' AND l.expires_at>now() AND a.status='active'
    )
) WITH CHECK (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
    AND station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid
    AND status='closed' AND counted_cash_minor>=0 AND report_json IS NOT NULL
    AND closed_by_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
    AND EXISTS (SELECT 1 FROM station_actor_leases l
        WHERE l.tenant_id=hosted_drawer_shifts.tenant_id AND l.branch_id=hosted_drawer_shifts.branch_id
          AND l.station_id=hosted_drawer_shifts.station_id AND l.id=hosted_drawer_shifts.closing_lease_id
          AND l.membership_id=closed_by_membership_id
          AND l.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND l.status='issued' AND l.expires_at>now())
);

CREATE POLICY hosted_drawer_movement_read ON hosted_drawer_movements FOR SELECT USING (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
    AND EXISTS (SELECT 1 FROM hosted_drawer_shifts s WHERE s.tenant_id=hosted_drawer_movements.tenant_id
      AND s.branch_id=hosted_drawer_movements.branch_id AND s.station_id=hosted_drawer_movements.station_id
      AND s.id=hosted_drawer_movements.shift_id)
);
CREATE POLICY hosted_drawer_movement_insert ON hosted_drawer_movements FOR INSERT WITH CHECK (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
    AND station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid
    AND actor_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
    AND EXISTS (SELECT 1 FROM hosted_drawer_shifts s WHERE s.tenant_id=hosted_drawer_movements.tenant_id
      AND s.branch_id=hosted_drawer_movements.branch_id AND s.station_id=hosted_drawer_movements.station_id
      AND s.id=hosted_drawer_movements.shift_id AND s.status='open')
    AND EXISTS (
        SELECT 1 FROM station_actor_leases l JOIN station_actor_lease_locks k
          ON k.tenant_id=l.tenant_id AND k.branch_id=l.branch_id AND k.station_id=l.station_id AND k.lease_id=l.id
        JOIN station_activations a ON a.tenant_id=l.tenant_id AND a.branch_id=l.branch_id
          AND a.station_id=l.station_id AND a.id=l.activation_id
        JOIN membership_role_grants g ON g.tenant_id=l.tenant_id AND g.membership_id=l.membership_id
        WHERE l.tenant_id=hosted_drawer_movements.tenant_id AND l.branch_id=hosted_drawer_movements.branch_id
          AND l.station_id=hosted_drawer_movements.station_id AND l.id=hosted_drawer_movements.lease_id
          AND l.activation_id=hosted_drawer_movements.activation_id
          AND l.membership_id=hosted_drawer_movements.actor_membership_id
          AND l.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND l.status='issued' AND l.expires_at>now() AND a.status='active' AND g.role_key='cashier'
    )
);
