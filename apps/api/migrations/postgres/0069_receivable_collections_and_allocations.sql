-- Immutable customer collections are allocated to specific invoices. A
-- collection and its allocations must commit together; open exposure is the
-- invoice amount less these allocations.

ALTER TABLE hosted_receivable_invoices
    ADD CONSTRAINT hosted_receivable_invoices_allocation_identity
    UNIQUE (tenant_id, branch_id, id, customer_id, currency_code);

CREATE TABLE hosted_receivable_collections (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    customer_id UUID NOT NULL,
    currency_code CHAR(3) NOT NULL CHECK (currency_code ~ '^[A-Z]{3}$'),
    amount_minor BIGINT NOT NULL CHECK (amount_minor BETWEEN 1 AND 9007199254740991),
    payment_method TEXT NOT NULL CHECK (payment_method IN ('cash','card','mobile_money','bank_transfer','other')),
    confirmation_state TEXT NOT NULL CHECK (confirmation_state IN ('cash_recorded','manual_unverified')),
    external_reference TEXT CHECK (external_reference IS NULL OR length(btrim(external_reference)) BETWEEN 1 AND 160),
    recorded_by_membership_id UUID NOT NULL,
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash) = 32),
    received_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((payment_method = 'cash' AND confirmation_state = 'cash_recorded')
        OR (payment_method <> 'cash' AND confirmation_state = 'manual_unverified')),
    FOREIGN KEY (tenant_id, branch_id) REFERENCES branches(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, customer_id) REFERENCES hosted_customers(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (recorded_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, branch_id, id),
    UNIQUE (tenant_id, branch_id, id, customer_id, currency_code),
    UNIQUE (tenant_id, branch_id, recorded_by_membership_id, idempotency_key)
);
CREATE INDEX hosted_receivable_collections_customer_received
    ON hosted_receivable_collections(tenant_id, customer_id, currency_code, received_at DESC, id);

CREATE TABLE hosted_receivable_allocations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    collection_branch_id UUID NOT NULL,
    collection_id UUID NOT NULL,
    invoice_branch_id UUID NOT NULL,
    invoice_id UUID NOT NULL,
    customer_id UUID NOT NULL,
    currency_code CHAR(3) NOT NULL CHECK (currency_code ~ '^[A-Z]{3}$'),
    amount_minor BIGINT NOT NULL CHECK (amount_minor BETWEEN 1 AND 9007199254740991),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, collection_branch_id, collection_id, customer_id, currency_code)
        REFERENCES hosted_receivable_collections(tenant_id, branch_id, id, customer_id, currency_code) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, invoice_branch_id, invoice_id, customer_id, currency_code)
        REFERENCES hosted_receivable_invoices(tenant_id, branch_id, id, customer_id, currency_code) ON DELETE RESTRICT,
    UNIQUE (tenant_id, collection_branch_id, collection_id, invoice_branch_id, invoice_id)
);
CREATE INDEX hosted_receivable_allocations_invoice
    ON hosted_receivable_allocations(tenant_id, customer_id, currency_code, invoice_branch_id, invoice_id);
CREATE INDEX hosted_receivable_allocations_collection
    ON hosted_receivable_allocations(tenant_id, collection_branch_id, collection_id);

ALTER TABLE hosted_receivable_collections ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_receivable_collections FORCE ROW LEVEL SECURITY;
ALTER TABLE hosted_receivable_allocations ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_receivable_allocations FORCE ROW LEVEL SECURITY;

CREATE POLICY hosted_receivable_collections_worker_read ON hosted_receivable_collections
    FOR SELECT TO pos_receivables_worker USING (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND customer_id=NULLIF(current_setting('pos.receivable_customer_id',true),'')::uuid
        AND currency_code=NULLIF(current_setting('pos.receivable_currency_code',true),'')
    );
CREATE POLICY hosted_receivable_allocations_worker_read ON hosted_receivable_allocations
    FOR SELECT TO pos_receivables_worker USING (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND customer_id=NULLIF(current_setting('pos.receivable_customer_id',true),'')::uuid
        AND currency_code=NULLIF(current_setting('pos.receivable_currency_code',true),'')
    );

GRANT SELECT (tenant_id, branch_id, id, customer_id, currency_code, amount_minor)
    ON hosted_receivable_collections TO pos_receivables_worker;
GRANT SELECT (tenant_id, invoice_branch_id, invoice_id, customer_id, currency_code, amount_minor)
    ON hosted_receivable_allocations TO pos_receivables_worker;
GRANT SELECT (tenant_id, branch_id, id, customer_id, currency_code, original_amount_minor)
    ON hosted_receivable_invoices TO pos_receivables_worker;

CREATE FUNCTION hosted_receivable_collection_immutable() RETURNS trigger
    LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'Receivable collections and allocations are append-only' USING ERRCODE='55000';
END;
$$;
CREATE TRIGGER hosted_receivable_collections_no_update_delete
    BEFORE UPDATE OR DELETE ON hosted_receivable_collections
    FOR EACH ROW EXECUTE FUNCTION hosted_receivable_collection_immutable();
CREATE TRIGGER hosted_receivable_allocations_no_update_delete
    BEFORE UPDATE OR DELETE ON hosted_receivable_allocations
    FOR EACH ROW EXECUTE FUNCTION hosted_receivable_collection_immutable();

GRANT CREATE ON SCHEMA pos_worker TO pos_receivables_worker;

CREATE FUNCTION pos_worker.guard_receivable_allocation()
RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog,public,pos_worker
AS $$
DECLARE
    v_original BIGINT;
    v_allocated NUMERIC;
    v_old_customer TEXT;
    v_old_currency TEXT;
BEGIN
    v_old_customer:=current_setting('pos.receivable_customer_id',true);
    v_old_currency:=current_setting('pos.receivable_currency_code',true);
    PERFORM set_config('pos.receivable_customer_id',NEW.customer_id::text,true);
    PERFORM set_config('pos.receivable_currency_code',NEW.currency_code,true);

    -- Serialize all competing allocations to this invoice. The aggregate is
    -- read after the lock is acquired so a waiting transaction sees commits.
    PERFORM pg_advisory_xact_lock(hashtextextended(
        'receivable-invoice:' || NEW.tenant_id::text || ':' || NEW.invoice_branch_id::text || ':' || NEW.invoice_id::text, 0));
    SELECT i.original_amount_minor INTO v_original
      FROM public.hosted_receivable_invoices i
     WHERE i.tenant_id=NEW.tenant_id AND i.branch_id=NEW.invoice_branch_id AND i.id=NEW.invoice_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Receivable invoice was not found' USING ERRCODE='23503';
    END IF;
    SELECT COALESCE(sum(a.amount_minor::numeric),0) INTO v_allocated
      FROM public.hosted_receivable_allocations a
     WHERE a.tenant_id=NEW.tenant_id AND a.invoice_branch_id=NEW.invoice_branch_id
       AND a.invoice_id=NEW.invoice_id;
    IF v_allocated + NEW.amount_minor > v_original THEN
        RAISE EXCEPTION 'Receivable allocations exceed the invoice open balance' USING ERRCODE='23514';
    END IF;

    PERFORM set_config('pos.receivable_customer_id',COALESCE(v_old_customer,''),true);
    PERFORM set_config('pos.receivable_currency_code',COALESCE(v_old_currency,''),true);
    RETURN NEW;
END;
$$;
ALTER FUNCTION pos_worker.guard_receivable_allocation() OWNER TO pos_receivables_worker;
REVOKE ALL ON FUNCTION pos_worker.guard_receivable_allocation() FROM PUBLIC;
CREATE TRIGGER hosted_receivable_allocations_open_balance_guard
    BEFORE INSERT ON hosted_receivable_allocations
    FOR EACH ROW EXECUTE FUNCTION pos_worker.guard_receivable_allocation();

CREATE FUNCTION pos_worker.check_receivable_collection_allocation_total()
RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog,public,pos_worker
AS $$
DECLARE
    v_collection hosted_receivable_collections%ROWTYPE;
    v_allocated NUMERIC;
    v_tenant UUID;
    v_branch UUID;
    v_collection_id UUID;
    v_customer UUID;
    v_currency TEXT;
    v_old_customer TEXT;
    v_old_currency TEXT;
BEGIN
    IF TG_TABLE_NAME = 'hosted_receivable_collections' THEN
        v_tenant:=NEW.tenant_id; v_branch:=NEW.branch_id; v_collection_id:=NEW.id; v_customer:=NEW.customer_id; v_currency:=NEW.currency_code::text;
    ELSE
        v_tenant:=NEW.tenant_id; v_branch:=NEW.collection_branch_id; v_collection_id:=NEW.collection_id; v_customer:=NEW.customer_id; v_currency:=NEW.currency_code::text;
    END IF;
    v_old_customer:=current_setting('pos.receivable_customer_id',true);
    v_old_currency:=current_setting('pos.receivable_currency_code',true);
    PERFORM set_config('pos.receivable_customer_id',v_customer::text,true);
    PERFORM set_config('pos.receivable_currency_code',v_currency,true);

    SELECT * INTO v_collection FROM public.hosted_receivable_collections c
     WHERE c.tenant_id=v_tenant AND c.branch_id=v_branch AND c.id=v_collection_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Receivable collection was not found' USING ERRCODE='23503';
    END IF;
    SELECT COALESCE(sum(a.amount_minor::numeric),0) INTO v_allocated
      FROM public.hosted_receivable_allocations a
     WHERE a.tenant_id=v_tenant AND a.collection_branch_id=v_branch AND a.collection_id=v_collection_id;
    IF v_allocated <> v_collection.amount_minor THEN
        RAISE EXCEPTION 'Receivable collection allocations must equal the collection amount' USING ERRCODE='23514';
    END IF;

    PERFORM set_config('pos.receivable_customer_id',COALESCE(v_old_customer,''),true);
    PERFORM set_config('pos.receivable_currency_code',COALESCE(v_old_currency,''),true);
    RETURN NULL;
END;
$$;
ALTER FUNCTION pos_worker.check_receivable_collection_allocation_total() OWNER TO pos_receivables_worker;
REVOKE ALL ON FUNCTION pos_worker.check_receivable_collection_allocation_total() FROM PUBLIC;
CREATE CONSTRAINT TRIGGER hosted_receivable_collection_requires_full_allocation
    AFTER INSERT ON hosted_receivable_collections DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION pos_worker.check_receivable_collection_allocation_total();
CREATE CONSTRAINT TRIGGER hosted_receivable_allocation_matches_collection
    AFTER INSERT ON hosted_receivable_allocations DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION pos_worker.check_receivable_collection_allocation_total();

CREATE OR REPLACE FUNCTION pos_worker.customer_credit_position(p_customer_id uuid)
RETURNS TABLE (
    currency_code text,
    has_credit_terms boolean,
    terms_revision bigint,
    credit_limit_minor text,
    due_days integer,
    outstanding_minor text,
    available_minor text
)
LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog,public,pos_worker
AS $$
DECLARE
    v_tenant uuid;
    v_branch uuid;
    v_currency text;
    v_customer_status text;
    v_revision bigint;
    v_limit bigint;
    v_due_days integer;
    v_outstanding numeric;
BEGIN
    v_tenant:=NULLIF(current_setting('pos.tenant_id',true),'')::uuid;
    v_branch:=NULLIF(current_setting('pos.branch_id',true),'')::uuid;
    IF v_tenant IS NULL OR v_branch IS NULL OR p_customer_id IS NULL THEN
        RAISE EXCEPTION 'Customer credit position requires an authenticated branch context'
            USING ERRCODE='42501';
    END IF;
    PERFORM set_config('pos.inventory_availability','',true);
    PERFORM set_config('pos.staff_roster_tenant_id','',true);
    PERFORM set_config('pos.receivable_customer_id',p_customer_id::text,true);
    SELECT b.currency_code::text INTO v_currency
      FROM public.branches b
     WHERE b.tenant_id=v_tenant AND b.id=v_branch AND b.status='active';
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Active branch was not found' USING ERRCODE='P0002';
    END IF;
    PERFORM set_config('pos.receivable_currency_code',v_currency,true);
    SELECT c.status INTO v_customer_status FROM public.hosted_customers c
     WHERE c.tenant_id=v_tenant AND c.id=p_customer_id;
    IF NOT FOUND OR v_customer_status<>'active' THEN
        RAISE EXCEPTION 'Active customer was not found' USING ERRCODE='P0002';
    END IF;
    SELECT t.revision,t.credit_limit_minor,t.due_days INTO v_revision,v_limit,v_due_days
      FROM public.hosted_customer_credit_terms t
     WHERE t.tenant_id=v_tenant AND t.customer_id=p_customer_id AND t.currency_code=v_currency;
    SELECT COALESCE(sum(i.original_amount_minor::numeric-COALESCE(a.allocated_minor,0)),0)
      INTO v_outstanding
      FROM public.hosted_receivable_invoices i
      LEFT JOIN (
          SELECT allocation.tenant_id,allocation.invoice_branch_id,allocation.invoice_id,
                 sum(allocation.amount_minor::numeric) AS allocated_minor
            FROM public.hosted_receivable_allocations allocation
           WHERE allocation.tenant_id=v_tenant AND allocation.customer_id=p_customer_id
             AND allocation.currency_code=v_currency
           GROUP BY allocation.tenant_id,allocation.invoice_branch_id,allocation.invoice_id
      ) a ON a.tenant_id=i.tenant_id AND a.invoice_branch_id=i.branch_id AND a.invoice_id=i.id
     WHERE i.tenant_id=v_tenant AND i.customer_id=p_customer_id AND i.currency_code=v_currency;
    RETURN QUERY SELECT v_currency,v_revision IS NOT NULL AND COALESCE(v_limit,0)>0,v_revision,
        COALESCE(v_limit,0)::text,COALESCE(v_due_days,0),v_outstanding::text,
        CASE WHEN COALESCE(v_limit,0)>0 THEN GREATEST(v_limit::numeric-v_outstanding,0)::text ELSE '0'::text END;
END;
$$;
ALTER FUNCTION pos_worker.customer_credit_position(uuid) OWNER TO pos_receivables_worker;
REVOKE ALL ON FUNCTION pos_worker.customer_credit_position(uuid) FROM PUBLIC;

REVOKE CREATE ON SCHEMA pos_worker FROM pos_receivables_worker;
