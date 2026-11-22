-- Append-only hosted item refunds and full-sale voids.
-- Payment execution is external/manual until a provider connector is approved.

ALTER TABLE hosted_sales
    DROP CONSTRAINT hosted_sales_status_check;
ALTER TABLE hosted_sales
    ADD CONSTRAINT hosted_sales_status_check
    CHECK (status IN ('completed','partially_refunded','refunded','voided'));

ALTER TABLE station_financial_proof_challenges
    DROP CONSTRAINT station_financial_proof_challenges_operation_check;
ALTER TABLE station_financial_proof_challenges
    ADD CONSTRAINT station_financial_proof_challenges_operation_check
    CHECK (operation IN ('sale.commit','sale.refund','drawer.open','drawer.movement','drawer.close','supplier.payment'));

CREATE TABLE hosted_sale_refunds (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    station_id UUID NOT NULL,
    sale_id UUID NOT NULL,
    actor_membership_id UUID NOT NULL,
    lease_id UUID NOT NULL,
    activation_id UUID NOT NULL,
    cash_shift_id UUID,
    refund_type TEXT NOT NULL CHECK (refund_type IN ('refund','void')),
    total_minor BIGINT NOT NULL CHECK (total_minor > 0 AND total_minor <= 9000000000000000),
    reason TEXT NOT NULL CHECK (length(btrim(reason)) BETWEEN 3 AND 250),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
    cashier_name_snapshot TEXT NOT NULL CHECK (length(btrim(cashier_name_snapshot)) BETWEEN 1 AND 160),
    station_name_snapshot TEXT NOT NULL CHECK (length(btrim(station_name_snapshot)) BETWEEN 1 AND 160),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id,branch_id,sale_id)
        REFERENCES hosted_sales(tenant_id,branch_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (station_id,branch_id,tenant_id)
        REFERENCES stations(id,branch_id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_membership_id,tenant_id)
        REFERENCES memberships(id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id,lease_id)
        REFERENCES station_actor_leases(tenant_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id,activation_id)
        REFERENCES station_activations(tenant_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id,branch_id,station_id,cash_shift_id)
        REFERENCES hosted_drawer_shifts(tenant_id,branch_id,station_id,id) ON DELETE RESTRICT,
    UNIQUE (tenant_id,branch_id,id),
    UNIQUE (tenant_id,branch_id,station_id,id),
    UNIQUE (tenant_id,branch_id,station_id,cash_shift_id,id),
    UNIQUE (tenant_id,station_id,actor_membership_id,idempotency_key),
    CHECK (cash_shift_id IS NULL OR refund_type IN ('refund','void'))
);
CREATE INDEX hosted_sale_refunds_sale_created
    ON hosted_sale_refunds(tenant_id,branch_id,sale_id,created_at DESC,id);

CREATE TABLE hosted_sale_refund_lines (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    sale_id UUID NOT NULL,
    refund_id UUID NOT NULL,
    sale_line_id UUID NOT NULL,
    product_id UUID NOT NULL,
    quantity BIGINT NOT NULL CHECK (quantity > 0),
    restock BOOLEAN NOT NULL DEFAULT false,
    subtotal_minor BIGINT NOT NULL CHECK (subtotal_minor >= 0),
    tax_minor BIGINT NOT NULL CHECK (tax_minor >= 0),
    amount_minor BIGINT NOT NULL CHECK (amount_minor >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id,branch_id,refund_id)
        REFERENCES hosted_sale_refunds(tenant_id,branch_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id,branch_id,sale_id)
        REFERENCES hosted_sales(tenant_id,branch_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (sale_line_id)
        REFERENCES hosted_sale_lines(id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id,product_id)
        REFERENCES catalog_products(tenant_id,id) ON DELETE RESTRICT,
    UNIQUE (tenant_id,refund_id,sale_line_id)
);
CREATE INDEX hosted_sale_refund_lines_sale_line
    ON hosted_sale_refund_lines(tenant_id,branch_id,sale_id,sale_line_id);

CREATE TABLE hosted_sale_refund_tenders (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    refund_id UUID NOT NULL,
    method TEXT NOT NULL CHECK (method IN ('cash','card','mobile_money','bank_transfer','other')),
    amount_minor BIGINT NOT NULL CHECK (amount_minor > 0),
    external_reference TEXT CHECK (external_reference IS NULL OR length(btrim(external_reference)) BETWEEN 1 AND 160),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id,branch_id,refund_id)
        REFERENCES hosted_sale_refunds(tenant_id,branch_id,id) ON DELETE RESTRICT,
    UNIQUE (tenant_id,branch_id,refund_id)
);

ALTER TABLE hosted_drawer_movements
    ADD COLUMN source_refund_id UUID;
ALTER TABLE hosted_drawer_movements
    ADD CONSTRAINT hosted_drawer_refund_fk
    FOREIGN KEY (tenant_id,branch_id,station_id,shift_id,source_refund_id)
    REFERENCES hosted_sale_refunds(tenant_id,branch_id,station_id,cash_shift_id,id) ON DELETE RESTRICT;
ALTER TABLE hosted_drawer_movements
    ADD CONSTRAINT hosted_drawer_refund_is_paid_out
    CHECK (source_refund_id IS NULL OR movement_type='paid_out');
ALTER TABLE hosted_drawer_movements
    ADD CONSTRAINT hosted_drawer_one_financial_source
    CHECK (source_refund_id IS NULL OR source_payment_id IS NULL);
CREATE UNIQUE INDEX hosted_drawer_refund_once
    ON hosted_drawer_movements(tenant_id,source_refund_id)
    WHERE source_refund_id IS NOT NULL;

ALTER TABLE hosted_sale_refunds ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_sale_refunds FORCE ROW LEVEL SECURITY;
ALTER TABLE hosted_sale_refund_lines ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_sale_refund_lines FORCE ROW LEVEL SECURITY;
ALTER TABLE hosted_sale_refund_tenders ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_sale_refund_tenders FORCE ROW LEVEL SECURITY;

CREATE POLICY hosted_sale_refunds_select ON hosted_sale_refunds
    FOR SELECT USING (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
        AND EXISTS (SELECT 1 FROM hosted_sales s
                    WHERE s.tenant_id=hosted_sale_refunds.tenant_id
                      AND s.branch_id=hosted_sale_refunds.branch_id
                      AND s.id=hosted_sale_refunds.sale_id)
    );
CREATE POLICY hosted_sale_refunds_insert ON hosted_sale_refunds
    FOR INSERT WITH CHECK (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
        AND station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
        AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND EXISTS (SELECT 1 FROM hosted_sales s
                    WHERE s.tenant_id=hosted_sale_refunds.tenant_id
                      AND s.branch_id=hosted_sale_refunds.branch_id
                      AND s.station_id=hosted_sale_refunds.station_id
                      AND s.id=hosted_sale_refunds.sale_id)
        AND EXISTS (SELECT 1 FROM station_actor_leases l
                    JOIN station_actor_lease_locks k ON k.tenant_id=l.tenant_id AND k.branch_id=l.branch_id
                      AND k.station_id=l.station_id AND k.lease_id=l.id
                    JOIN station_activations a ON a.tenant_id=l.tenant_id AND a.branch_id=l.branch_id
                      AND a.station_id=l.station_id AND a.id=l.activation_id
                    JOIN membership_role_grants g ON g.tenant_id=l.tenant_id AND g.membership_id=l.membership_id
                    WHERE l.id=hosted_sale_refunds.lease_id
                      AND l.tenant_id=hosted_sale_refunds.tenant_id
                      AND l.branch_id=hosted_sale_refunds.branch_id
                      AND l.station_id=hosted_sale_refunds.station_id
                      AND l.membership_id=hosted_sale_refunds.actor_membership_id
                      AND l.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
                      AND l.activation_id=hosted_sale_refunds.activation_id
                      AND l.status='issued' AND l.expires_at>now() AND a.status='active'
                      AND g.role_key='cashier'
                      AND (g.branch_id IS NULL OR g.branch_id=l.branch_id)
                      AND (g.station_id IS NULL OR g.station_id=l.station_id))
        AND EXISTS (SELECT 1 FROM membership_role_grants g
                    WHERE g.tenant_id=hosted_sale_refunds.tenant_id
                      AND g.membership_id=hosted_sale_refunds.actor_membership_id
                      AND g.role_key IN ('owner','admin','manager')
                      AND (g.branch_id IS NULL OR g.branch_id=hosted_sale_refunds.branch_id)
                      AND g.station_id IS NULL)
        AND EXISTS (SELECT 1 FROM station_financial_proof_challenges c
                    WHERE c.id=NULLIF(current_setting('pos.refund_challenge_id',true),'')::uuid
                      AND c.tenant_id=hosted_sale_refunds.tenant_id
                      AND c.branch_id=hosted_sale_refunds.branch_id
                      AND c.station_id=hosted_sale_refunds.station_id
                      AND c.membership_id=hosted_sale_refunds.actor_membership_id
                      AND c.operation='sale.refund'
                      AND c.idempotency_key=hosted_sale_refunds.idempotency_key
                      AND c.consumed_at IS NOT NULL)
    );

CREATE POLICY hosted_sale_refund_lines_select ON hosted_sale_refund_lines
    FOR SELECT USING (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
        AND EXISTS (SELECT 1 FROM hosted_sale_refunds r
                    WHERE r.tenant_id=hosted_sale_refund_lines.tenant_id
                      AND r.branch_id=hosted_sale_refund_lines.branch_id
                      AND r.id=hosted_sale_refund_lines.refund_id)
    );
CREATE POLICY hosted_sale_refund_lines_insert ON hosted_sale_refund_lines
    FOR INSERT WITH CHECK (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
        AND EXISTS (SELECT 1 FROM hosted_sale_refunds r
                    WHERE r.tenant_id=hosted_sale_refund_lines.tenant_id
                      AND r.branch_id=hosted_sale_refund_lines.branch_id
                      AND r.sale_id=hosted_sale_refund_lines.sale_id
                      AND r.id=hosted_sale_refund_lines.refund_id
                      AND r.actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid)
        AND EXISTS (SELECT 1 FROM hosted_sale_lines sl
                    WHERE sl.tenant_id=hosted_sale_refund_lines.tenant_id
                      AND sl.branch_id=hosted_sale_refund_lines.branch_id
                      AND sl.sale_id=hosted_sale_refund_lines.sale_id
                      AND sl.id=hosted_sale_refund_lines.sale_line_id
                      AND sl.product_id=hosted_sale_refund_lines.product_id)
    );

CREATE POLICY hosted_sale_refund_tenders_select ON hosted_sale_refund_tenders
    FOR SELECT USING (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
        AND EXISTS (SELECT 1 FROM hosted_sale_refunds r
                    WHERE r.tenant_id=hosted_sale_refund_tenders.tenant_id
                      AND r.branch_id=hosted_sale_refund_tenders.branch_id
                      AND r.id=hosted_sale_refund_tenders.refund_id)
    );
CREATE POLICY hosted_sale_refund_tenders_insert ON hosted_sale_refund_tenders
    FOR INSERT WITH CHECK (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
        AND EXISTS (SELECT 1 FROM hosted_sale_refunds r
                    WHERE r.tenant_id=hosted_sale_refund_tenders.tenant_id
                      AND r.branch_id=hosted_sale_refund_tenders.branch_id
                      AND r.id=hosted_sale_refund_tenders.refund_id
                      AND r.actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
                      AND r.total_minor=hosted_sale_refund_tenders.amount_minor)
        AND (method='cash' OR external_reference IS NOT NULL)
    );

CREATE POLICY hosted_sales_refund_status_update ON hosted_sales
    FOR UPDATE USING (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
        AND station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
        AND cashier_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND status IN ('completed','partially_refunded')
        AND EXISTS (SELECT 1 FROM station_actor_leases l
                    JOIN station_actor_lease_locks k ON k.tenant_id=l.tenant_id AND k.branch_id=l.branch_id
                      AND k.station_id=l.station_id AND k.lease_id=l.id
                    JOIN station_activations a ON a.tenant_id=l.tenant_id AND a.branch_id=l.branch_id
                      AND a.station_id=l.station_id AND a.id=l.activation_id
                    WHERE l.tenant_id=hosted_sales.tenant_id AND l.branch_id=hosted_sales.branch_id
                      AND l.station_id=hosted_sales.station_id
                      AND l.membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
                      AND l.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
                      AND l.status='issued' AND l.expires_at>now() AND a.status='active')
        AND EXISTS (SELECT 1 FROM membership_role_grants g
                    WHERE g.tenant_id=hosted_sales.tenant_id
                      AND g.membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
                      AND g.role_key IN ('owner','admin','manager')
                      AND (g.branch_id IS NULL OR g.branch_id=hosted_sales.branch_id)
                      AND g.station_id IS NULL)
    ) WITH CHECK (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
        AND station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
        AND cashier_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND status IN ('partially_refunded','refunded','voided')
        AND EXISTS (SELECT 1 FROM station_financial_proof_challenges c
                    WHERE c.id=NULLIF(current_setting('pos.refund_challenge_id',true),'')::uuid
                      AND c.tenant_id=hosted_sales.tenant_id
                      AND c.branch_id=hosted_sales.branch_id
                      AND c.station_id=hosted_sales.station_id
                      AND c.membership_id=hosted_sales.cashier_membership_id
                      AND c.operation='sale.refund'
                      AND c.consumed_at IS NOT NULL)
    );

CREATE POLICY branch_catalog_items_cashier_refund_update ON branch_catalog_items
    FOR UPDATE USING (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
        AND EXISTS (SELECT 1 FROM hosted_sale_refund_lines ri
                    JOIN hosted_sale_refunds r ON r.tenant_id=ri.tenant_id AND r.branch_id=ri.branch_id
                      AND r.id=ri.refund_id
                    WHERE ri.tenant_id=branch_catalog_items.tenant_id
                      AND ri.branch_id=branch_catalog_items.branch_id
                      AND ri.product_id=branch_catalog_items.product_id
                      AND ri.restock=true
                      AND r.id=NULLIF(current_setting('pos.refund_id',true),'')::uuid
                      AND r.actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid)
    ) WITH CHECK (stock_quantity>=0);

CREATE POLICY stock_movements_cashier_sale_return_insert ON stock_movements
    FOR INSERT WITH CHECK (
        movement_kind='sale_return' AND quantity_delta>0
        AND tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
        AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND EXISTS (SELECT 1 FROM hosted_sale_refund_lines ri
                    JOIN hosted_sale_refunds r ON r.tenant_id=ri.tenant_id AND r.branch_id=ri.branch_id
                      AND r.id=ri.refund_id
                    WHERE r.id=stock_movements.source_id
                      AND r.actor_membership_id=stock_movements.actor_membership_id
                      AND ri.product_id=stock_movements.product_id
                      AND ri.restock=true AND ri.quantity=stock_movements.quantity_delta)
    );

GRANT SELECT (id,tenant_id,branch_id,station_id,sale_id,actor_membership_id,lease_id,
              activation_id,cash_shift_id,refund_type,total_minor,reason,idempotency_key,
              request_hash,cashier_name_snapshot,station_name_snapshot,created_at)
  ON hosted_sale_refunds TO pos_api_runtime;
GRANT INSERT (id,tenant_id,branch_id,station_id,sale_id,actor_membership_id,lease_id,
              activation_id,cash_shift_id,refund_type,total_minor,reason,idempotency_key,
              request_hash,cashier_name_snapshot,station_name_snapshot)
  ON hosted_sale_refunds TO pos_api_runtime;
GRANT SELECT (id,tenant_id,branch_id,sale_id,refund_id,sale_line_id,product_id,quantity,
              restock,subtotal_minor,tax_minor,amount_minor,created_at)
  ON hosted_sale_refund_lines TO pos_api_runtime;
GRANT INSERT (tenant_id,branch_id,sale_id,refund_id,sale_line_id,product_id,quantity,
              restock,subtotal_minor,tax_minor,amount_minor)
  ON hosted_sale_refund_lines TO pos_api_runtime;
GRANT SELECT (id,tenant_id,branch_id,refund_id,method,amount_minor,external_reference,created_at)
  ON hosted_sale_refund_tenders TO pos_api_runtime;
GRANT INSERT (tenant_id,branch_id,refund_id,method,amount_minor,external_reference)
  ON hosted_sale_refund_tenders TO pos_api_runtime;
GRANT UPDATE (status) ON hosted_sales TO pos_api_runtime;
GRANT SELECT (id,tenant_id,branch_id,station_id,shift_id,actor_membership_id,lease_id,
              activation_id,movement_type,amount_minor,note,idempotency_key,request_hash,
              created_at,source_payment_id,source_refund_id)
  ON hosted_drawer_movements TO pos_api_runtime;
GRANT INSERT (id,tenant_id,branch_id,station_id,shift_id,actor_membership_id,lease_id,
              activation_id,movement_type,amount_minor,note,idempotency_key,request_hash,
              source_payment_id,source_refund_id)
  ON hosted_drawer_movements TO pos_api_runtime;
