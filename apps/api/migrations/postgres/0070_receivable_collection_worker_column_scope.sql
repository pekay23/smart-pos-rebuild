-- Keep the collection checker within the worker's least-column SELECT grant.
-- The initial implementation selected a whole row despite needing only amount_minor.
CREATE OR REPLACE FUNCTION pos_worker.check_receivable_collection_allocation_total()
RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog,public,pos_worker
AS $$
DECLARE
    v_collection_amount BIGINT;
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

    SELECT c.amount_minor INTO v_collection_amount
      FROM public.hosted_receivable_collections c
     WHERE c.tenant_id=v_tenant AND c.branch_id=v_branch AND c.id=v_collection_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Receivable collection was not found' USING ERRCODE='23503';
    END IF;
    SELECT COALESCE(sum(a.amount_minor::numeric),0) INTO v_allocated
      FROM public.hosted_receivable_allocations a
     WHERE a.tenant_id=v_tenant AND a.collection_branch_id=v_branch AND a.collection_id=v_collection_id;
    IF v_allocated <> v_collection_amount THEN
        RAISE EXCEPTION 'Receivable collection allocations must equal the collection amount' USING ERRCODE='23514';
    END IF;

    PERFORM set_config('pos.receivable_customer_id',COALESCE(v_old_customer,''),true);
    PERFORM set_config('pos.receivable_currency_code',COALESCE(v_old_currency,''),true);
    RETURN NULL;
END;
$$;
ALTER FUNCTION pos_worker.check_receivable_collection_allocation_total() OWNER TO pos_receivables_worker;
REVOKE ALL ON FUNCTION pos_worker.check_receivable_collection_allocation_total() FROM PUBLIC;
GRANT SELECT (collection_branch_id, collection_id)
    ON hosted_receivable_allocations TO pos_receivables_worker;
