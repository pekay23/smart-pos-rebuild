-- Durable inter-branch transfers. Inventory changes only at source dispatch
-- and destination receipt; requests and in-transit quantities are not sellable.
CREATE TABLE stock_transfers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    source_branch_id UUID NOT NULL,
    destination_branch_id UUID NOT NULL,
    status TEXT NOT NULL DEFAULT 'requested'
        CHECK (status IN ('requested','accepted','rejected','in_transit','received','discrepancy','cancelled')),
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    request_idempotency_key TEXT NOT NULL CHECK (length(btrim(request_idempotency_key)) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash) = 32),
    request_note TEXT CHECK (request_note IS NULL OR length(btrim(request_note)) <= 500),
    requested_by_membership_id UUID NOT NULL,
    requested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    accepted_by_membership_id UUID,
    accepted_at TIMESTAMPTZ,
    rejected_by_membership_id UUID,
    rejected_at TIMESTAMPTZ,
    rejection_note TEXT CHECK (rejection_note IS NULL OR length(btrim(rejection_note)) <= 500),
    dispatched_by_membership_id UUID,
    dispatched_at TIMESTAMPTZ,
    received_by_membership_id UUID,
    received_at TIMESTAMPTZ,
    receipt_note TEXT CHECK (receipt_note IS NULL OR length(btrim(receipt_note)) <= 500),
    cancelled_by_membership_id UUID,
    cancelled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (source_branch_id, tenant_id) REFERENCES branches(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (destination_branch_id, tenant_id) REFERENCES branches(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (requested_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (accepted_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (rejected_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (dispatched_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (received_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (cancelled_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    CHECK (source_branch_id <> destination_branch_id),
    CHECK (
        (status = 'requested' AND accepted_at IS NULL AND rejected_at IS NULL AND dispatched_at IS NULL AND received_at IS NULL AND cancelled_at IS NULL)
        OR (status = 'accepted' AND accepted_at IS NOT NULL AND rejected_at IS NULL AND dispatched_at IS NULL AND received_at IS NULL AND cancelled_at IS NULL)
        OR (status = 'rejected' AND rejected_at IS NOT NULL AND dispatched_at IS NULL AND received_at IS NULL AND cancelled_at IS NULL)
        OR (status = 'in_transit' AND accepted_at IS NOT NULL AND dispatched_at IS NOT NULL AND received_at IS NULL AND cancelled_at IS NULL)
        OR (status IN ('received','discrepancy') AND accepted_at IS NOT NULL AND dispatched_at IS NOT NULL AND received_at IS NOT NULL AND cancelled_at IS NULL)
        OR (status = 'cancelled' AND dispatched_at IS NULL AND received_at IS NULL AND cancelled_at IS NOT NULL)
    ),
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, destination_branch_id, requested_by_membership_id, request_idempotency_key)
);
CREATE INDEX stock_transfers_source_queue ON stock_transfers (tenant_id, source_branch_id, status, requested_at DESC, id);
CREATE INDEX stock_transfers_destination_queue ON stock_transfers (tenant_id, destination_branch_id, status, requested_at DESC, id);

CREATE TABLE stock_transfer_lines (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    transfer_id UUID NOT NULL,
    product_id UUID NOT NULL,
    sku_snapshot TEXT NOT NULL CHECK (length(btrim(sku_snapshot)) BETWEEN 1 AND 64),
    name_snapshot TEXT NOT NULL CHECK (length(btrim(name_snapshot)) BETWEEN 1 AND 160),
    requested_quantity BIGINT NOT NULL CHECK (requested_quantity > 0),
    accepted_quantity BIGINT CHECK (accepted_quantity IS NULL OR accepted_quantity > 0),
    dispatched_quantity BIGINT CHECK (dispatched_quantity IS NULL OR dispatched_quantity > 0),
    received_quantity BIGINT CHECK (received_quantity IS NULL OR received_quantity >= 0),
    discrepancy_note TEXT CHECK (discrepancy_note IS NULL OR length(btrim(discrepancy_note)) <= 500),
    FOREIGN KEY (tenant_id, transfer_id) REFERENCES stock_transfers(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, product_id) REFERENCES catalog_products(tenant_id, id) ON DELETE RESTRICT,
    CHECK (accepted_quantity IS NULL OR accepted_quantity <= requested_quantity),
    CHECK (dispatched_quantity IS NULL OR (accepted_quantity IS NOT NULL AND dispatched_quantity <= accepted_quantity)),
    CHECK (received_quantity IS NULL OR dispatched_quantity IS NOT NULL),
    UNIQUE (tenant_id, transfer_id, product_id),
    UNIQUE (tenant_id, id)
);
CREATE INDEX stock_transfer_lines_product ON stock_transfer_lines (tenant_id, product_id, transfer_id);

-- Append-only transition ledger. The API updates the transfer snapshot and
-- inserts its event in the same transaction, with a unique replay key.
CREATE TABLE stock_transfer_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    transfer_id UUID NOT NULL,
    acting_branch_id UUID NOT NULL,
    actor_membership_id UUID NOT NULL,
    event_type TEXT NOT NULL CHECK (event_type IN ('requested','accepted','rejected','dispatched','received','cancelled')),
    revision BIGINT NOT NULL CHECK (revision > 0),
    idempotency_key TEXT NOT NULL CHECK (length(btrim(idempotency_key)) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash) = 32),
    details JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(details) = 'object'),
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, transfer_id) REFERENCES stock_transfers(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (acting_branch_id, tenant_id) REFERENCES branches(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, transfer_id, actor_membership_id, idempotency_key),
    UNIQUE (tenant_id, transfer_id, revision)
);
CREATE INDEX stock_transfer_events_history ON stock_transfer_events (tenant_id, transfer_id, occurred_at, id);

ALTER TABLE stock_transfers ENABLE ROW LEVEL SECURITY;
ALTER TABLE stock_transfers FORCE ROW LEVEL SECURITY;
ALTER TABLE stock_transfer_lines ENABLE ROW LEVEL SECURITY;
ALTER TABLE stock_transfer_lines FORCE ROW LEVEL SECURITY;
ALTER TABLE stock_transfer_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE stock_transfer_events FORCE ROW LEVEL SECURITY;

CREATE POLICY stock_transfers_branch_read ON stock_transfers FOR SELECT USING (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND (source_branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
         OR destination_branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid)
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND m.tenant_id=stock_transfers.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager','cashier')
          AND (g.branch_id IS NULL OR g.branch_id IN (stock_transfers.source_branch_id, stock_transfers.destination_branch_id))
          AND (g.station_id IS NULL OR m.id=stock_transfers.requested_by_membership_id)
    )
);
CREATE POLICY stock_transfers_request_insert ON stock_transfers FOR INSERT WITH CHECK (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND destination_branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
    AND requested_by_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
    AND status='requested' AND revision=1
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=stock_transfers.requested_by_membership_id
          AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND m.tenant_id=stock_transfers.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager','cashier')
          AND (g.branch_id IS NULL OR g.branch_id=stock_transfers.destination_branch_id)
          AND (g.station_id IS NULL OR g.role_key='cashier')
    )
);
CREATE POLICY stock_transfers_transition_update ON stock_transfers FOR UPDATE
    USING (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND (source_branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
             OR destination_branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid)
        AND EXISTS (
            SELECT 1 FROM memberships m JOIN membership_role_grants g
              ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
            WHERE m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.tenant_id=stock_transfers.tenant_id AND m.status='active'
              AND g.role_key IN ('owner','admin','manager') AND g.station_id IS NULL
              AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid)
        )
    ) WITH CHECK (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND (source_branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
             OR destination_branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid)
    );

CREATE POLICY stock_transfer_lines_branch_read ON stock_transfer_lines FOR SELECT USING (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND EXISTS (SELECT 1 FROM stock_transfers t WHERE t.tenant_id=stock_transfer_lines.tenant_id AND t.id=stock_transfer_lines.transfer_id)
);
CREATE POLICY stock_transfer_lines_request_insert ON stock_transfer_lines FOR INSERT WITH CHECK (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND EXISTS (SELECT 1 FROM stock_transfers t WHERE t.tenant_id=stock_transfer_lines.tenant_id
        AND t.id=stock_transfer_lines.transfer_id AND t.status='requested'
        AND t.destination_branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND t.requested_by_membership_id=NULLIF(current_setting('pos.membership_id', true), '')::uuid)
    AND EXISTS (SELECT 1 FROM branch_catalog_items i WHERE i.tenant_id=stock_transfer_lines.tenant_id
        AND i.product_id=stock_transfer_lines.product_id AND i.branch_id=(SELECT t.source_branch_id FROM stock_transfers t WHERE t.tenant_id=stock_transfer_lines.tenant_id AND t.id=stock_transfer_lines.transfer_id) AND i.status='active')
    AND EXISTS (SELECT 1 FROM branch_catalog_items i WHERE i.tenant_id=stock_transfer_lines.tenant_id
        AND i.product_id=stock_transfer_lines.product_id AND i.branch_id=(SELECT t.destination_branch_id FROM stock_transfers t WHERE t.tenant_id=stock_transfer_lines.tenant_id AND t.id=stock_transfer_lines.transfer_id) AND i.status='active')
);
CREATE POLICY stock_transfer_lines_manager_update ON stock_transfer_lines FOR UPDATE
    USING (tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND EXISTS (SELECT 1 FROM stock_transfers t JOIN membership_role_grants g ON g.tenant_id=t.tenant_id
            JOIN memberships m ON m.tenant_id=g.tenant_id AND m.id=g.membership_id
            WHERE t.tenant_id=stock_transfer_lines.tenant_id AND t.id=stock_transfer_lines.transfer_id
              AND NULLIF(current_setting('pos.branch_id', true), '')::uuid IN (t.source_branch_id,t.destination_branch_id)
              AND m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid AND m.status='active'
              AND g.role_key IN ('owner','admin','manager') AND g.station_id IS NULL
              AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid)))
    WITH CHECK (tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid);

CREATE POLICY stock_transfer_events_branch_read ON stock_transfer_events FOR SELECT USING (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND EXISTS (SELECT 1 FROM stock_transfers t WHERE t.tenant_id=stock_transfer_events.tenant_id AND t.id=stock_transfer_events.transfer_id)
);
CREATE POLICY stock_transfer_events_append ON stock_transfer_events FOR INSERT WITH CHECK (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND acting_branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
    AND actor_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
    AND EXISTS (SELECT 1 FROM stock_transfers t JOIN membership_role_grants g ON g.tenant_id=t.tenant_id
        JOIN memberships m ON m.tenant_id=g.tenant_id AND m.id=g.membership_id
        WHERE t.tenant_id=stock_transfer_events.tenant_id AND t.id=stock_transfer_events.transfer_id
          AND acting_branch_id IN (t.source_branch_id,t.destination_branch_id)
          AND m.id=stock_transfer_events.actor_membership_id
          AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid AND m.status='active'
          AND g.role_key IN ('owner','admin','manager','cashier')
          AND (g.branch_id IS NULL OR g.branch_id=acting_branch_id)
          AND (g.station_id IS NULL OR (g.role_key='cashier' AND actor_membership_id=t.requested_by_membership_id)))
);
