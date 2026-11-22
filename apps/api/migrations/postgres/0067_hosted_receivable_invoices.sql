-- Immutable receivable invoices and an explicit, balanced on-account portion
-- for hosted sales. Checkout/API enforcement is introduced in the following slice.
ALTER TABLE hosted_sales
    ADD COLUMN on_account_minor BIGINT NOT NULL DEFAULT 0
        CHECK (on_account_minor BETWEEN 0 AND total_minor);

CREATE TABLE hosted_receivable_invoices (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    sale_id UUID NOT NULL,
    customer_id UUID NOT NULL,
    currency_code CHAR(3) NOT NULL CHECK (currency_code ~ '^[A-Z]{3}$'),
    original_amount_minor BIGINT NOT NULL CHECK (original_amount_minor > 0),
    credit_term_revision BIGINT NOT NULL CHECK (credit_term_revision > 0),
    due_days_snapshot INTEGER NOT NULL CHECK (due_days_snapshot BETWEEN 0 AND 365),
    credit_limit_minor_snapshot BIGINT NOT NULL CHECK (credit_limit_minor_snapshot > 0),
    issued_at TIMESTAMPTZ NOT NULL,
    due_at TIMESTAMPTZ NOT NULL,
    issued_by_membership_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, branch_id, sale_id)
        REFERENCES hosted_sales(tenant_id, branch_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, customer_id)
        REFERENCES hosted_customers(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (issued_by_membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, sale_id),
    UNIQUE (tenant_id, branch_id, id)
);
CREATE INDEX hosted_receivable_invoices_customer_due
    ON hosted_receivable_invoices(tenant_id, customer_id, currency_code, due_at, id);
CREATE INDEX hosted_receivable_invoices_branch_issued
    ON hosted_receivable_invoices(tenant_id, branch_id, issued_at DESC, id);

CREATE FUNCTION hosted_receivable_invoice_immutable() RETURNS trigger
    LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'Receivable invoices are immutable' USING ERRCODE='55000';
END;
$$;
CREATE TRIGGER hosted_receivable_invoices_no_update_delete
    BEFORE UPDATE OR DELETE ON hosted_receivable_invoices
    FOR EACH ROW EXECUTE FUNCTION hosted_receivable_invoice_immutable();

CREATE FUNCTION hosted_receivable_sale_state_check() RETURNS trigger
    LANGUAGE plpgsql AS $$
DECLARE sale_row hosted_sales%ROWTYPE;
DECLARE tender_total BIGINT;
DECLARE invoice_count BIGINT;
BEGIN
    IF TG_TABLE_NAME = 'hosted_sales' THEN
        SELECT * INTO sale_row FROM hosted_sales
        WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND id=NEW.id;
    ELSE
        SELECT * INTO sale_row FROM hosted_sales
        WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND id=NEW.sale_id;
    END IF;
    IF NOT FOUND THEN
        RETURN NULL;
    END IF;

    SELECT COALESCE(sum(amount_minor), 0) INTO tender_total
    FROM hosted_sale_tenders
    WHERE tenant_id=sale_row.tenant_id AND branch_id=sale_row.branch_id AND sale_id=sale_row.id;
    SELECT count(*) INTO invoice_count FROM hosted_receivable_invoices
    WHERE tenant_id=sale_row.tenant_id AND branch_id=sale_row.branch_id AND sale_id=sale_row.id;
    IF sale_row.on_account_minor = 0 THEN
        IF invoice_count <> 0 THEN
            RAISE EXCEPTION 'A fully paid sale cannot have a receivable invoice' USING ERRCODE='23514';
        END IF;
        RETURN NULL;
    END IF;
    IF tender_total + sale_row.on_account_minor <> sale_row.total_minor
       OR sale_row.customer_id IS NULL THEN
        RAISE EXCEPTION 'Sale tenders and receivable amount must equal the sale total and require a customer' USING ERRCODE='23514';
    END IF;
    IF invoice_count <> 1 THEN
        RAISE EXCEPTION 'An on-account sale requires exactly one receivable invoice' USING ERRCODE='23514';
    END IF;
    IF sale_row.on_account_minor > 0 AND NOT EXISTS (
        SELECT 1 FROM hosted_receivable_invoices i
        WHERE i.tenant_id=sale_row.tenant_id AND i.branch_id=sale_row.branch_id
          AND i.sale_id=sale_row.id AND i.customer_id=sale_row.customer_id
          AND i.currency_code=sale_row.currency_code
          AND i.original_amount_minor=sale_row.on_account_minor
          AND i.issued_at=sale_row.created_at
          AND i.issued_by_membership_id=sale_row.cashier_membership_id
    ) THEN
        RAISE EXCEPTION 'Receivable invoice must match its source sale' USING ERRCODE='23514';
    END IF;
    RETURN NULL;
END;
$$;
CREATE CONSTRAINT TRIGGER hosted_receivable_sale_state_from_sale
    AFTER INSERT OR UPDATE ON hosted_sales DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION hosted_receivable_sale_state_check();
CREATE CONSTRAINT TRIGGER hosted_receivable_sale_state_from_tender
    AFTER INSERT ON hosted_sale_tenders DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION hosted_receivable_sale_state_check();
CREATE CONSTRAINT TRIGGER hosted_receivable_sale_state_from_invoice
    AFTER INSERT ON hosted_receivable_invoices DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION hosted_receivable_sale_state_check();

ALTER TABLE hosted_receivable_invoices ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_receivable_invoices FORCE ROW LEVEL SECURITY;
CREATE POLICY hosted_receivable_invoices_branch_read ON hosted_receivable_invoices
    FOR SELECT USING (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
            JOIN hosted_sales s ON s.tenant_id=hosted_receivable_invoices.tenant_id
              AND s.branch_id=hosted_receivable_invoices.branch_id AND s.id=hosted_receivable_invoices.sale_id
            WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
              AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
              AND m.tenant_id=hosted_receivable_invoices.tenant_id AND m.status='active'
              AND (g.branch_id IS NULL OR g.branch_id=hosted_receivable_invoices.branch_id)
              AND (g.station_id IS NULL OR g.station_id=s.station_id)
              AND (g.role_key IN ('owner','admin','manager')
                   OR (g.role_key='cashier' AND m.id=s.cashier_membership_id))
        )
    );
CREATE POLICY hosted_receivable_invoices_cashier_insert ON hosted_receivable_invoices
    FOR INSERT WITH CHECK (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
        AND issued_by_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND EXISTS (
            SELECT 1 FROM hosted_sales s
            WHERE s.tenant_id=hosted_receivable_invoices.tenant_id
              AND s.branch_id=hosted_receivable_invoices.branch_id AND s.id=hosted_receivable_invoices.sale_id
              AND s.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
              AND s.cashier_membership_id=hosted_receivable_invoices.issued_by_membership_id
              AND s.customer_id=hosted_receivable_invoices.customer_id
              AND s.currency_code=hosted_receivable_invoices.currency_code
              AND s.on_account_minor=hosted_receivable_invoices.original_amount_minor
              AND s.created_at=hosted_receivable_invoices.issued_at
        )
    );

