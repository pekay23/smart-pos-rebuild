-- One quantity-only ledger for stock already promised to customers or an
-- accepted outbound transfer. Pickup contact details stay in their own table.
CREATE TABLE inventory_allocations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    product_id UUID NOT NULL,
    stock_transfer_id UUID,
    pickup_reservation_id UUID,
    quantity BIGINT NOT NULL CHECK (quantity > 0),
    expires_at TIMESTAMPTZ,
    released_at TIMESTAMPTZ,
    release_reason TEXT CHECK (release_reason IS NULL OR length(btrim(release_reason)) BETWEEN 1 AND 500),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (branch_id, tenant_id) REFERENCES branches(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, product_id) REFERENCES catalog_products(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, stock_transfer_id) REFERENCES stock_transfers(tenant_id, id) ON DELETE RESTRICT,
    CHECK ((stock_transfer_id IS NOT NULL) <> (pickup_reservation_id IS NOT NULL)),
    CHECK (stock_transfer_id IS NULL OR expires_at IS NULL),
    CHECK ((released_at IS NULL) = (release_reason IS NULL)),
    UNIQUE (tenant_id, id)
);
CREATE UNIQUE INDEX inventory_allocations_transfer_product_unique
    ON inventory_allocations (tenant_id, stock_transfer_id, branch_id, product_id)
    WHERE stock_transfer_id IS NOT NULL;
CREATE UNIQUE INDEX inventory_allocations_pickup_product_unique
    ON inventory_allocations (tenant_id, pickup_reservation_id, branch_id, product_id)
    WHERE pickup_reservation_id IS NOT NULL;
CREATE INDEX inventory_allocations_branch_product_active
    ON inventory_allocations (tenant_id, branch_id, product_id, expires_at)
    WHERE released_at IS NULL;

-- Every stock mutation and allocation uses the same non-blocking advisory lock.
-- A try-lock avoids a row-lock/advisory-lock inversion for a future writer that
-- forgets to acquire the inventory lock before updating branch_catalog_items.
CREATE FUNCTION guard_inventory_allocation_insert() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    on_hand BIGINT;
    committed BIGINT;
BEGIN
    IF NEW.released_at IS NOT NULL
       OR (NEW.expires_at IS NOT NULL AND NEW.expires_at <= clock_timestamp()) THEN
        RETURN NEW;
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'advanced-pos:inventory:v1:' || NEW.tenant_id::text || ':' ||
        NEW.branch_id::text || ':' || NEW.product_id::text, 0)) THEN
        RAISE EXCEPTION 'Inventory commitment is being updated; retry the operation.'
            USING ERRCODE = '55P03';
    END IF;
    SELECT stock_quantity INTO on_hand
      FROM branch_catalog_items
     WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND product_id=NEW.product_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Inventory item is unavailable for commitment.'
            USING ERRCODE = '23503';
    END IF;
    SELECT COALESCE(SUM(quantity),0) INTO committed
      FROM inventory_allocations
     WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND product_id=NEW.product_id
       AND released_at IS NULL
       AND (expires_at IS NULL OR expires_at > clock_timestamp());
    IF committed > on_hand OR NEW.quantity > on_hand - committed THEN
        RAISE EXCEPTION 'Inventory commitment exceeds stock available to promise.'
            USING ERRCODE = '23514', CONSTRAINT = 'inventory_allocation_within_stock';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION guard_inventory_stock_decrease() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    committed BIGINT;
BEGIN
    IF NEW.stock_quantity >= OLD.stock_quantity THEN
        RETURN NEW;
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'advanced-pos:inventory:v1:' || OLD.tenant_id::text || ':' ||
        OLD.branch_id::text || ':' || OLD.product_id::text, 0)) THEN
        RAISE EXCEPTION 'Inventory commitment is being updated; retry the operation.'
            USING ERRCODE = '55P03';
    END IF;
    SELECT COALESCE(SUM(quantity),0) INTO committed
      FROM inventory_allocations
     WHERE tenant_id=OLD.tenant_id AND branch_id=OLD.branch_id AND product_id=OLD.product_id
       AND released_at IS NULL
       AND (expires_at IS NULL OR expires_at > clock_timestamp());
    IF NEW.stock_quantity < committed THEN
        RAISE EXCEPTION 'Stock cannot be reduced below live inventory commitments.'
            USING ERRCODE = '23514', CONSTRAINT = 'branch_stock_respects_inventory_allocations';
    END IF;
    RETURN NEW;
END;
$$;

ALTER TABLE inventory_allocations ENABLE ROW LEVEL SECURITY;
ALTER TABLE inventory_allocations FORCE ROW LEVEL SECURITY;

CREATE POLICY inventory_allocations_branch_or_availability_read
    ON inventory_allocations FOR SELECT USING (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND EXISTS (
            SELECT 1
            FROM memberships m
            JOIN tenants t ON t.id=m.tenant_id AND t.status='active'
            JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
            WHERE m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.tenant_id=inventory_allocations.tenant_id AND m.status='active'
              AND g.role_key IN ('owner','admin','manager','cashier')
              AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid)
              AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id', true), '')::uuid)
              AND (
                  inventory_allocations.branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid
                  OR current_setting('pos.inventory_availability', true)='enabled'
              )
        )
    );

CREATE POLICY inventory_allocations_transfer_insert
    ON inventory_allocations FOR INSERT WITH CHECK (
        tenant_id=NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND stock_transfer_id IS NOT NULL AND pickup_reservation_id IS NULL
        AND EXISTS (
            SELECT 1 FROM stock_transfers t
            JOIN stock_transfer_lines l ON l.tenant_id=t.tenant_id AND l.transfer_id=t.id
            JOIN membership_role_grants g ON g.tenant_id=t.tenant_id
            JOIN memberships m ON m.tenant_id=g.tenant_id AND m.id=g.membership_id
            WHERE t.tenant_id=inventory_allocations.tenant_id
              AND t.id=inventory_allocations.stock_transfer_id
              AND t.source_branch_id=inventory_allocations.branch_id
              AND t.status IN ('requested','accepted')
              AND l.product_id=inventory_allocations.product_id
              AND l.accepted_quantity=inventory_allocations.quantity
              AND m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status='active'
              AND g.role_key IN ('owner','admin','manager')
              AND g.station_id IS NULL
              AND (g.branch_id IS NULL OR g.branch_id=inventory_allocations.branch_id)
        )
    );

CREATE POLICY inventory_allocations_transfer_release
    ON inventory_allocations FOR UPDATE
    USING (
        tenant_id=NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND stock_transfer_id IS NOT NULL AND released_at IS NULL
        AND EXISTS (
            SELECT 1 FROM membership_role_grants g
            JOIN memberships m ON m.tenant_id=g.tenant_id AND m.id=g.membership_id
            WHERE g.tenant_id=inventory_allocations.tenant_id
              AND m.id=NULLIF(current_setting('pos.membership_id', true), '')::uuid
              AND m.identity_id=NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status='active' AND g.role_key IN ('owner','admin','manager')
              AND g.station_id IS NULL
              AND (g.branch_id IS NULL OR g.branch_id=inventory_allocations.branch_id)
        )
    )
    WITH CHECK (
        tenant_id=NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id=NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND stock_transfer_id IS NOT NULL
        AND released_at IS NOT NULL AND release_reason IS NOT NULL
    );

-- Existing accepted transfers are already operational commitments. Preserve
-- them exactly, including any legacy shortage; do not fabricate stock.
INSERT INTO inventory_allocations
    (tenant_id, branch_id, product_id, stock_transfer_id, quantity)
SELECT t.tenant_id, t.source_branch_id, l.product_id, t.id, l.accepted_quantity
FROM stock_transfers t
JOIN stock_transfer_lines l ON l.tenant_id=t.tenant_id AND l.transfer_id=t.id
WHERE t.status='accepted' AND l.accepted_quantity > 0;

CREATE TRIGGER inventory_allocations_validate_insert
    BEFORE INSERT ON inventory_allocations
    FOR EACH ROW EXECUTE FUNCTION guard_inventory_allocation_insert();
CREATE TRIGGER branch_catalog_items_preserve_allocations
    BEFORE UPDATE OF stock_quantity ON branch_catalog_items
    FOR EACH ROW EXECUTE FUNCTION guard_inventory_stock_decrease();

GRANT SELECT (id, tenant_id, branch_id, product_id, stock_transfer_id,
              pickup_reservation_id, quantity, expires_at, released_at,
              release_reason, created_at)
    ON inventory_allocations TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, product_id, stock_transfer_id,
              pickup_reservation_id, quantity, expires_at)
    ON inventory_allocations TO pos_api_runtime;
GRANT UPDATE (released_at, release_reason)
    ON inventory_allocations TO pos_api_runtime;
