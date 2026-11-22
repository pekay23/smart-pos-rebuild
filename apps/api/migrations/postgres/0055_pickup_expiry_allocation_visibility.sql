-- PostgreSQL also applies SELECT policies when validating rows changed by an
-- UPDATE. Keep the worker's post-release tuple visible, but only when the
-- matching system-expiry event and expired reservation authorize that release.
ALTER POLICY pickup_expiry_worker_allocation_read ON inventory_allocations
USING (
    pickup_reservation_id IS NOT NULL
    AND (released_at IS NULL OR (released_at IS NOT NULL AND release_reason='expired'))
    AND EXISTS (
        SELECT 1
          FROM pickup_reservations r
          JOIN pickup_reservation_events e
            ON e.tenant_id=r.tenant_id
           AND e.reservation_id=r.id
           AND e.revision=r.revision
           AND e.event_type='expired'
           AND e.actor_kind='system'
           AND e.actor_membership_id IS NULL
         WHERE r.tenant_id=inventory_allocations.tenant_id
           AND r.id=inventory_allocations.pickup_reservation_id
           AND r.status='expired'
           AND r.sale_id IS NULL
           AND r.expires_at<=clock_timestamp()
    )
);

-- Constraint triggers are deferred to transaction commit by default. If the
-- runner invokes the materializer directly, that commit happens after the
-- SECURITY DEFINER context ends. Validate the transition inside the restricted
-- worker context before returning to the execute-only runner.
CREATE FUNCTION pos_worker.materialize_expired_pickup_reservation() RETURNS BOOLEAN
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path=pg_catalog,pos_worker,public
AS $$
DECLARE
    materialized BOOLEAN;
BEGIN
    materialized:=pos_worker.materialize_next_expired_pickup_reservation();
    SET CONSTRAINTS pickup_reservation_header_event_check,
                    pickup_reservation_event_state_check IMMEDIATE;
    RETURN materialized;
END;
$$;

ALTER FUNCTION pos_worker.materialize_expired_pickup_reservation() OWNER TO pos_api_expiry_worker;
REVOKE ALL ON FUNCTION pos_worker.materialize_expired_pickup_reservation() FROM PUBLIC;
REVOKE EXECUTE ON FUNCTION pos_worker.materialize_next_expired_pickup_reservation()
    FROM pos_api_expiry_runner;
GRANT EXECUTE ON FUNCTION pos_worker.materialize_expired_pickup_reservation()
    TO pos_api_expiry_runner;
