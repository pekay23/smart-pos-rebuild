-- A manually typed external reference is not proof of settlement. Keep its
-- allocation reserved, but subtract only cash recorded at the register from
-- the customer's outstanding receivable balance.
GRANT SELECT (confirmation_state)
    ON hosted_receivable_collections TO pos_receivables_worker;

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
    SELECT COALESCE(sum(i.original_amount_minor::numeric-COALESCE(a.confirmed_minor,0)),0)
      INTO v_outstanding
      FROM public.hosted_receivable_invoices i
      LEFT JOIN (
          SELECT allocation.tenant_id,allocation.invoice_branch_id,allocation.invoice_id,
                 sum(allocation.amount_minor::numeric) AS confirmed_minor
            FROM public.hosted_receivable_allocations allocation
            JOIN public.hosted_receivable_collections collection
              ON collection.tenant_id=allocation.tenant_id
             AND collection.branch_id=allocation.collection_branch_id
             AND collection.id=allocation.collection_id
             AND collection.customer_id=allocation.customer_id
             AND collection.currency_code=allocation.currency_code
           WHERE allocation.tenant_id=v_tenant AND allocation.customer_id=p_customer_id
             AND allocation.currency_code=v_currency
             AND collection.confirmation_state='cash_recorded'
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
