-- Cashiers and managers can preview a selected customer's tenant-wide credit
-- exposure without receiving cross-branch invoice-row access.
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='pos_receivables_worker') THEN
        CREATE ROLE pos_receivables_worker NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
    END IF;
END $$;

GRANT USAGE, CREATE ON SCHEMA pos_worker TO pos_receivables_worker;

CREATE POLICY branches_receivables_worker_read ON branches
    FOR SELECT TO pos_receivables_worker USING (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    );
CREATE POLICY hosted_customers_receivables_worker_read ON hosted_customers
    FOR SELECT TO pos_receivables_worker USING (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND id=NULLIF(current_setting('pos.receivable_customer_id',true),'')::uuid
    );
CREATE POLICY hosted_customer_credit_terms_receivables_worker_read ON hosted_customer_credit_terms
    FOR SELECT TO pos_receivables_worker USING (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND customer_id=NULLIF(current_setting('pos.receivable_customer_id',true),'')::uuid
        AND currency_code=NULLIF(current_setting('pos.receivable_currency_code',true),'')
    );
CREATE POLICY hosted_receivable_invoices_receivables_worker_read ON hosted_receivable_invoices
    FOR SELECT TO pos_receivables_worker USING (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND customer_id=NULLIF(current_setting('pos.receivable_customer_id',true),'')::uuid
        AND currency_code=NULLIF(current_setting('pos.receivable_currency_code',true),'')
    );
-- The ordinary runtime invoice policy follows sale rows and its richer role
-- checks. Keep it for API queries only; the worker receives the aggregate
-- policy above and never needs direct sale-row access.
ALTER POLICY hosted_receivable_invoices_branch_read ON hosted_receivable_invoices
    TO pos_api_runtime;

GRANT SELECT (tenant_id,id,currency_code,status) ON branches TO pos_receivables_worker;
GRANT SELECT (tenant_id,id,status) ON hosted_customers TO pos_receivables_worker;
GRANT SELECT (tenant_id,customer_id,currency_code,credit_limit_minor,due_days,revision)
    ON hosted_customer_credit_terms TO pos_receivables_worker;
GRANT SELECT (tenant_id,customer_id,currency_code,original_amount_minor)
    ON hosted_receivable_invoices TO pos_receivables_worker;
-- Existing branch RLS policies validate the caller through these relations.
-- The worker can see only the authenticated membership and its scoped grants.
GRANT SELECT (id,tenant_id,identity_id,status) ON memberships TO pos_receivables_worker;
GRANT SELECT (tenant_id,membership_id,role_key,branch_id,station_id)
    ON membership_role_grants TO pos_receivables_worker;
GRANT SELECT (id,status) ON tenants TO pos_receivables_worker;
GRANT USAGE ON SCHEMA public TO pos_receivables_worker;

CREATE FUNCTION pos_worker.customer_credit_position(p_customer_id uuid)
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

    -- Do not inherit the API runtime's optional cross-branch inventory scope.
    -- The worker's branch policy is deliberately limited to the selected branch.
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

    SELECT c.status INTO v_customer_status
      FROM public.hosted_customers c
     WHERE c.tenant_id=v_tenant AND c.id=p_customer_id;
    IF NOT FOUND OR v_customer_status<>'active' THEN
        RAISE EXCEPTION 'Active customer was not found' USING ERRCODE='P0002';
    END IF;

    SELECT t.revision,t.credit_limit_minor,t.due_days
      INTO v_revision,v_limit,v_due_days
      FROM public.hosted_customer_credit_terms t
     WHERE t.tenant_id=v_tenant AND t.customer_id=p_customer_id
       AND t.currency_code=v_currency;

    SELECT COALESCE(sum(i.original_amount_minor::numeric),0)
      INTO v_outstanding
      FROM public.hosted_receivable_invoices i
     WHERE i.tenant_id=v_tenant AND i.customer_id=p_customer_id
       AND i.currency_code=v_currency;

    RETURN QUERY SELECT v_currency,
        v_revision IS NOT NULL AND COALESCE(v_limit,0)>0,
        v_revision,
        COALESCE(v_limit,0)::text,
        COALESCE(v_due_days,0),
        v_outstanding::text,
        CASE WHEN COALESCE(v_limit,0)>0
             THEN GREATEST(v_limit::numeric-v_outstanding,0)::text
             ELSE '0'::text END;
END;
$$;

ALTER FUNCTION pos_worker.customer_credit_position(uuid) OWNER TO pos_receivables_worker;
REVOKE CREATE ON SCHEMA pos_worker FROM pos_receivables_worker;
REVOKE ALL ON FUNCTION pos_worker.customer_credit_position(uuid) FROM PUBLIC;

