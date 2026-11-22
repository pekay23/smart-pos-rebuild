-- Branch pickup holds are customer reservations, not stock transfers. The
-- quantity-only commitment remains in inventory_allocations.
CREATE TABLE pickup_reservations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    origin_branch_id UUID NOT NULL,
    origin_station_id UUID,
    pickup_branch_id UUID NOT NULL,
    requested_by_membership_id UUID NOT NULL,
    customer_id UUID,
    contact_name_snapshot TEXT NOT NULL CHECK (length(btrim(contact_name_snapshot)) BETWEEN 1 AND 160),
    contact_phone_snapshot TEXT NOT NULL CHECK (length(btrim(contact_phone_snapshot)) BETWEEN 1 AND 40),
    contact_consent_at TIMESTAMPTZ NOT NULL,
    status TEXT NOT NULL DEFAULT 'held'
        CHECK (status IN ('held','ready','collected','cancelled','expired')),
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    expires_at TIMESTAMPTZ NOT NULL,
    sale_id UUID,
    request_idempotency_key TEXT NOT NULL CHECK (length(btrim(request_idempotency_key)) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (origin_branch_id, tenant_id) REFERENCES branches(id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (origin_station_id, origin_branch_id, tenant_id) REFERENCES stations(id,branch_id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (pickup_branch_id, tenant_id) REFERENCES branches(id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (requested_by_membership_id, tenant_id) REFERENCES memberships(id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, customer_id) REFERENCES hosted_customers(tenant_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, sale_id) REFERENCES hosted_sales(tenant_id,id) ON DELETE RESTRICT,
    CHECK (origin_branch_id <> pickup_branch_id),
    CHECK (expires_at > created_at),
    CHECK ((status='collected') = (sale_id IS NOT NULL)),
    UNIQUE (tenant_id,id),
    UNIQUE (tenant_id,requested_by_membership_id,origin_station_id,request_idempotency_key)
);
CREATE INDEX pickup_reservations_origin_queue
    ON pickup_reservations (tenant_id,origin_branch_id,origin_station_id,status,created_at DESC,id);
CREATE INDEX pickup_reservations_destination_queue
    ON pickup_reservations (tenant_id,pickup_branch_id,status,expires_at,id);

CREATE TABLE pickup_reservation_lines (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    reservation_id UUID NOT NULL,
    product_id UUID NOT NULL,
    pickup_branch_id UUID NOT NULL,
    sku_snapshot TEXT NOT NULL CHECK (length(btrim(sku_snapshot)) BETWEEN 1 AND 64),
    name_snapshot TEXT NOT NULL CHECK (length(btrim(name_snapshot)) BETWEEN 1 AND 160),
    quantity BIGINT NOT NULL CHECK (quantity BETWEEN 1 AND 1000000),
    FOREIGN KEY (tenant_id,reservation_id) REFERENCES pickup_reservations(tenant_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id,product_id) REFERENCES catalog_products(tenant_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (pickup_branch_id,tenant_id) REFERENCES branches(id,tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id,reservation_id,product_id),
    UNIQUE (tenant_id,reservation_id,product_id,pickup_branch_id)
);
CREATE INDEX pickup_reservation_lines_product ON pickup_reservation_lines(tenant_id,product_id,reservation_id);

CREATE TABLE pickup_reservation_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    reservation_id UUID NOT NULL,
    acting_branch_id UUID NOT NULL,
    acting_station_id UUID,
    actor_membership_id UUID NOT NULL,
    event_type TEXT NOT NULL CHECK (event_type IN ('held','ready','cancelled','expired','collected')),
    revision BIGINT NOT NULL CHECK (revision > 0),
    idempotency_key TEXT NOT NULL CHECK (length(btrim(idempotency_key)) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
    reason TEXT CHECK (reason IS NULL OR length(btrim(reason)) BETWEEN 1 AND 500),
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (tenant_id,reservation_id) REFERENCES pickup_reservations(tenant_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (acting_branch_id,tenant_id) REFERENCES branches(id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (acting_station_id,acting_branch_id,tenant_id) REFERENCES stations(id,branch_id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_membership_id,tenant_id) REFERENCES memberships(id,tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id,reservation_id,revision),
    UNIQUE (tenant_id,reservation_id,actor_membership_id,idempotency_key)
);
CREATE INDEX pickup_reservation_events_history
    ON pickup_reservation_events(tenant_id,reservation_id,occurred_at,id);

ALTER TABLE inventory_allocations
    ADD CONSTRAINT inventory_allocations_pickup_reservation_fk
    FOREIGN KEY (tenant_id,pickup_reservation_id)
    REFERENCES pickup_reservations(tenant_id,id) ON DELETE RESTRICT;
ALTER TABLE inventory_allocations
    ADD CONSTRAINT inventory_allocations_pickup_line_fk
    FOREIGN KEY (tenant_id,pickup_reservation_id,product_id,branch_id)
    REFERENCES pickup_reservation_lines(tenant_id,reservation_id,product_id,pickup_branch_id)
    ON DELETE RESTRICT;

ALTER TABLE pickup_reservations ENABLE ROW LEVEL SECURITY;
ALTER TABLE pickup_reservations FORCE ROW LEVEL SECURITY;
ALTER TABLE pickup_reservation_lines ENABLE ROW LEVEL SECURITY;
ALTER TABLE pickup_reservation_lines FORCE ROW LEVEL SECURITY;
ALTER TABLE pickup_reservation_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE pickup_reservation_events FORCE ROW LEVEL SECURITY;

-- Contact snapshots are readable only at the origin station by the creator,
-- or by assigned managers and staff at one of the two participating branches.
CREATE POLICY pickup_reservations_participant_read ON pickup_reservations FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
          AND m.tenant_id=pickup_reservations.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager','cashier')
          AND (
            (g.role_key='cashier' AND m.id=pickup_reservations.requested_by_membership_id
             AND pickup_reservations.origin_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
             AND pickup_reservations.origin_station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
             AND (g.branch_id IS NULL OR g.branch_id=pickup_reservations.origin_branch_id)
             AND (g.station_id IS NULL OR g.station_id=pickup_reservations.origin_station_id))
            OR (pickup_reservations.pickup_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
                AND (g.branch_id IS NULL OR g.branch_id=pickup_reservations.pickup_branch_id)
                AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id',true),'' )::uuid))
            OR (g.role_key IN ('owner','admin','manager') AND g.station_id IS NULL
                AND (g.branch_id IS NULL OR g.branch_id IN (pickup_reservations.origin_branch_id,pickup_reservations.pickup_branch_id))
                AND (pickup_reservations.origin_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
                     OR pickup_reservations.pickup_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid))
          )
    )
);
CREATE POLICY pickup_reservations_create ON pickup_reservations FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND origin_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND origin_station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
    AND requested_by_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND status='held' AND revision=1 AND sale_id IS NULL
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=pickup_reservations.requested_by_membership_id
          AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
          AND m.tenant_id=pickup_reservations.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager','cashier')
          AND (g.branch_id IS NULL OR g.branch_id=pickup_reservations.origin_branch_id)
          AND (g.station_id IS NULL OR (g.role_key='cashier' AND g.station_id=pickup_reservations.origin_station_id))
    )
);
CREATE POLICY pickup_reservations_transition ON pickup_reservations FOR UPDATE
USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND (origin_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid OR pickup_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid)
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
          AND m.tenant_id=pickup_reservations.tenant_id AND m.status='active'
          AND (m.id=pickup_reservations.requested_by_membership_id
               OR (g.role_key IN ('owner','admin','manager') AND g.station_id IS NULL
                   AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid)))
    )
) WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND (origin_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid OR pickup_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid)
);

CREATE POLICY pickup_lines_participant_read ON pickup_reservation_lines FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM pickup_reservations r WHERE r.tenant_id=pickup_reservation_lines.tenant_id AND r.id=pickup_reservation_lines.reservation_id)
);
CREATE POLICY pickup_lines_request_insert ON pickup_reservation_lines FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND pickup_branch_id=(SELECT r.pickup_branch_id FROM pickup_reservations r WHERE r.tenant_id=pickup_reservation_lines.tenant_id AND r.id=pickup_reservation_lines.reservation_id AND r.requested_by_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid AND r.status='held')
    AND EXISTS (SELECT 1 FROM branch_catalog_items i WHERE i.tenant_id=pickup_reservation_lines.tenant_id AND i.branch_id=pickup_reservation_lines.pickup_branch_id AND i.product_id=pickup_reservation_lines.product_id AND i.status='active')
);
CREATE POLICY pickup_events_participant_read ON pickup_reservation_events FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND EXISTS (SELECT 1 FROM pickup_reservations r WHERE r.tenant_id=pickup_reservation_events.tenant_id AND r.id=pickup_reservation_events.reservation_id)
);
CREATE POLICY pickup_events_append ON pickup_reservation_events FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND acting_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND EXISTS (
        SELECT 1 FROM pickup_reservations r
        JOIN memberships m ON m.id=pickup_reservation_events.actor_membership_id AND m.tenant_id=r.tenant_id
        JOIN membership_role_grants g ON g.membership_id=m.id AND g.tenant_id=m.tenant_id
        WHERE r.tenant_id=pickup_reservation_events.tenant_id AND r.id=pickup_reservation_events.reservation_id
          AND acting_branch_id IN (r.origin_branch_id,r.pickup_branch_id)
          AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid AND m.status='active'
          AND g.role_key IN ('owner','admin','manager','cashier')
          AND (g.branch_id IS NULL OR g.branch_id=acting_branch_id)
          AND (g.station_id IS NULL OR (g.role_key='cashier' AND m.id=r.requested_by_membership_id AND g.station_id=acting_station_id))
    )
);

-- Extend the shared allocation policies for pickup records without granting
-- access to the contact snapshot table.
CREATE POLICY inventory_allocations_pickup_insert ON inventory_allocations FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND pickup_reservation_id IS NOT NULL AND stock_transfer_id IS NULL
    AND branch_id=(SELECT r.pickup_branch_id FROM pickup_reservations r WHERE r.tenant_id=inventory_allocations.tenant_id AND r.id=inventory_allocations.pickup_reservation_id AND r.requested_by_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid AND r.origin_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid AND r.origin_station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid AND r.status='held' AND r.expires_at>clock_timestamp())
    AND EXISTS (SELECT 1 FROM pickup_reservation_lines l WHERE l.tenant_id=inventory_allocations.tenant_id AND l.reservation_id=inventory_allocations.pickup_reservation_id AND l.product_id=inventory_allocations.product_id AND l.pickup_branch_id=inventory_allocations.branch_id AND l.quantity=inventory_allocations.quantity)
);
CREATE POLICY inventory_allocations_pickup_release ON inventory_allocations FOR UPDATE
USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND pickup_reservation_id IS NOT NULL AND released_at IS NULL
    AND EXISTS (SELECT 1 FROM pickup_reservations r WHERE r.tenant_id=inventory_allocations.tenant_id AND r.id=inventory_allocations.pickup_reservation_id AND (r.origin_branch_id=branch_id OR r.pickup_branch_id=branch_id))
) WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND pickup_reservation_id IS NOT NULL AND released_at IS NOT NULL AND release_reason IS NOT NULL
);

GRANT SELECT (id,tenant_id,origin_branch_id,origin_station_id,pickup_branch_id,requested_by_membership_id,customer_id,contact_name_snapshot,contact_phone_snapshot,contact_consent_at,status,revision,expires_at,sale_id,request_idempotency_key,request_hash,created_at,updated_at) ON pickup_reservations TO pos_api_runtime;
GRANT INSERT (id,tenant_id,origin_branch_id,origin_station_id,pickup_branch_id,requested_by_membership_id,customer_id,contact_name_snapshot,contact_phone_snapshot,contact_consent_at,status,revision,expires_at,request_idempotency_key,request_hash) ON pickup_reservations TO pos_api_runtime;
GRANT UPDATE (status,revision,updated_at,sale_id) ON pickup_reservations TO pos_api_runtime;
GRANT SELECT (id,tenant_id,reservation_id,product_id,pickup_branch_id,sku_snapshot,name_snapshot,quantity) ON pickup_reservation_lines TO pos_api_runtime;
GRANT INSERT (tenant_id,reservation_id,product_id,pickup_branch_id,sku_snapshot,name_snapshot,quantity) ON pickup_reservation_lines TO pos_api_runtime;
-- The deferred integrity trigger reads the complete matching event row with
-- invoker privileges. Forced RLS still limits visibility to reservation
-- participants; runtime writes remain append-only through the column grant.
GRANT SELECT ON pickup_reservation_events TO pos_api_runtime;
GRANT INSERT (tenant_id,reservation_id,acting_branch_id,acting_station_id,actor_membership_id,event_type,revision,idempotency_key,request_hash,reason) ON pickup_reservation_events TO pos_api_runtime;
GRANT SELECT (id,tenant_id,branch_id,product_id,stock_transfer_id,pickup_reservation_id,quantity,expires_at,released_at,release_reason,created_at) ON inventory_allocations TO pos_api_runtime;
GRANT INSERT (tenant_id,branch_id,product_id,stock_transfer_id,pickup_reservation_id,quantity,expires_at) ON inventory_allocations TO pos_api_runtime;
GRANT UPDATE (released_at,release_reason) ON inventory_allocations TO pos_api_runtime;
