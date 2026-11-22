-- Cashiers can see compact open-invoice summaries for the selected customer
-- without receiving direct cross-branch invoice or allocation table access.
GRANT SELECT (tenant_id,id,branch_id,customer_id,currency_code,original_amount_minor,issued_at,due_at)
  ON hosted_receivable_invoices TO pos_receivables_worker;

GRANT SELECT (tenant_id,invoice_branch_id,invoice_id,customer_id,currency_code,amount_minor)
  ON hosted_receivable_allocations TO pos_receivables_worker;

CREATE FUNCTION pos_worker.customer_open_invoices(p_customer_id uuid)
RETURNS TABLE (
  invoice_branch_id text,
  invoice_id text,
  issued_at text,
  due_at text,
  original_amount_minor text,
  allocated_minor text,
  open_minor text
)
LANGUAGE plpgsql SECURITY DEFINER
SET search_path=pg_catalog,public,pos_worker
AS $$
DECLARE
  v_tenant uuid;
  v_branch uuid;
  v_station uuid;
  v_membership uuid;
  v_identity uuid;
  v_currency text;
  v_customer_status text;
BEGIN
  v_tenant:=NULLIF(current_setting('pos.tenant_id',true),'')::uuid;
  v_branch:=NULLIF(current_setting('pos.branch_id',true),'')::uuid;
  v_station:=NULLIF(current_setting('pos.station_id',true),'')::uuid;
  v_membership:=NULLIF(current_setting('pos.membership_id',true),'')::uuid;
  v_identity:=NULLIF(current_setting('pos.identity_id',true),'')::uuid;
  IF v_tenant IS NULL OR v_branch IS NULL OR v_station IS NULL
     OR v_membership IS NULL OR v_identity IS NULL OR p_customer_id IS NULL THEN
    RAISE EXCEPTION 'Open invoices require an authenticated cashier station context'
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
  IF NOT EXISTS (
    SELECT 1 FROM public.memberships m
    JOIN public.membership_role_grants g
      ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
    WHERE m.id=v_membership AND m.identity_id=v_identity
      AND m.tenant_id=v_tenant AND m.status='active'
      AND g.role_key='cashier' AND (g.station_id IS NULL OR g.station_id=v_station)
      AND (g.branch_id IS NULL OR g.branch_id=v_branch)
  ) THEN
    RAISE EXCEPTION 'An active cashier profile assigned to this station is required'
      USING ERRCODE='42501';
  END IF;

  RETURN QUERY
  SELECT i.branch_id::text,i.id::text,
         to_char(i.issued_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
         to_char(i.due_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
         i.original_amount_minor::text,
         COALESCE(a.allocated_minor,0)::text,
         (i.original_amount_minor::numeric-COALESCE(a.allocated_minor,0))::text
    FROM public.hosted_receivable_invoices i
    LEFT JOIN (
      SELECT allocation.tenant_id,allocation.invoice_branch_id,allocation.invoice_id,
             sum(allocation.amount_minor::numeric) AS allocated_minor
        FROM public.hosted_receivable_allocations allocation
       WHERE allocation.tenant_id=v_tenant AND allocation.customer_id=p_customer_id
         AND allocation.currency_code=v_currency
       GROUP BY allocation.tenant_id,allocation.invoice_branch_id,allocation.invoice_id
    ) a ON a.tenant_id=i.tenant_id AND a.invoice_branch_id=i.branch_id AND a.invoice_id=i.id
   WHERE i.tenant_id=v_tenant AND i.customer_id=p_customer_id
     AND i.currency_code=v_currency
     AND i.original_amount_minor::numeric>COALESCE(a.allocated_minor,0)
   ORDER BY i.due_at,i.issued_at,i.id
   LIMIT 1000;
END;
$$;

ALTER FUNCTION pos_worker.customer_open_invoices(uuid) OWNER TO pos_receivables_worker;
REVOKE ALL ON FUNCTION pos_worker.customer_open_invoices(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION pos_worker.customer_open_invoices(uuid) TO pos_api_runtime;
