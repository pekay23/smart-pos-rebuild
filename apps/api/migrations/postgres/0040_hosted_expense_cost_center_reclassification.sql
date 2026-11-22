-- Expense attribution corrections are append-only. They change reporting
-- allocation, never the source expense, payment, cash event, or journal.
CREATE TABLE hosted_expense_cost_center_reclassifications (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    expense_id UUID NOT NULL,
    revision BIGINT NOT NULL CHECK (revision BETWEEN 1 AND 9007199254740991),
    expected_revision BIGINT NOT NULL CHECK (expected_revision>=0),
    previous_kind TEXT NOT NULL CHECK (previous_kind IN ('station_direct','store_overhead','legacy_unassigned')),
    previous_station_id UUID,
    previous_station_name_snapshot TEXT CHECK (previous_station_name_snapshot IS NULL OR length(btrim(previous_station_name_snapshot)) BETWEEN 1 AND 100),
    next_kind TEXT NOT NULL CHECK (next_kind IN ('station_direct','store_overhead','legacy_unassigned')),
    next_station_id UUID,
    next_station_name_snapshot TEXT CHECK (next_station_name_snapshot IS NULL OR length(btrim(next_station_name_snapshot)) BETWEEN 1 AND 100),
    reason TEXT NOT NULL CHECK (length(btrim(reason)) BETWEEN 5 AND 500),
    actor_membership_id UUID NOT NULL,
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id,branch_id,expense_id)
        REFERENCES hosted_expenses(tenant_id,branch_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_membership_id,tenant_id)
        REFERENCES memberships(id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (previous_station_id,branch_id,tenant_id)
        REFERENCES stations(id,branch_id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (next_station_id,branch_id,tenant_id)
        REFERENCES stations(id,branch_id,tenant_id) ON DELETE RESTRICT,
    CHECK (revision=expected_revision+1),
    CHECK ((previous_kind='station_direct')=(previous_station_id IS NOT NULL)),
    CHECK ((previous_kind='station_direct')=(previous_station_name_snapshot IS NOT NULL)),
    CHECK ((next_kind='station_direct')=(next_station_id IS NOT NULL)),
    CHECK ((next_kind='station_direct')=(next_station_name_snapshot IS NOT NULL)),
    CHECK ((previous_kind,previous_station_id) IS DISTINCT FROM (next_kind,next_station_id)),
    UNIQUE (tenant_id,branch_id,id),
    UNIQUE (tenant_id,branch_id,expense_id,revision),
    UNIQUE (tenant_id,branch_id,actor_membership_id,idempotency_key)
);
CREATE INDEX hosted_expense_cost_center_history
    ON hosted_expense_cost_center_reclassifications(tenant_id,branch_id,expense_id,revision);

ALTER TABLE hosted_expense_cost_center_reclassifications ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_expense_cost_center_reclassifications FORCE ROW LEVEL SECURITY;

CREATE POLICY hosted_expense_cost_center_read ON hosted_expense_cost_center_reclassifications FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g
      ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.tenant_id=hosted_expense_cost_center_reclassifications.tenant_id AND m.status='active'
        AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=hosted_expense_cost_center_reclassifications.branch_id)
        AND g.station_id IS NULL)
);
CREATE POLICY hosted_expense_cost_center_insert ON hosted_expense_cost_center_reclassifications FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g
      ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=hosted_expense_cost_center_reclassifications.actor_membership_id
        AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.tenant_id=hosted_expense_cost_center_reclassifications.tenant_id AND m.status='active'
        AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=hosted_expense_cost_center_reclassifications.branch_id)
        AND g.station_id IS NULL)
);

GRANT SELECT (id,tenant_id,branch_id,expense_id,revision,expected_revision,previous_kind,previous_station_id,previous_station_name_snapshot,next_kind,next_station_id,next_station_name_snapshot,reason,actor_membership_id,idempotency_key,request_hash,created_at)
    ON hosted_expense_cost_center_reclassifications TO pos_api_runtime;
GRANT INSERT (id,tenant_id,branch_id,expense_id,revision,expected_revision,previous_kind,previous_station_id,previous_station_name_snapshot,next_kind,next_station_id,next_station_name_snapshot,reason,actor_membership_id,idempotency_key,request_hash)
    ON hosted_expense_cost_center_reclassifications TO pos_api_runtime;
