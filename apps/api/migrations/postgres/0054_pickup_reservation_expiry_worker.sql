-- Persist overdue pickup holds without giving the API runtime a global
-- maintenance capability. The no-login function owner is constrained by
-- forced RLS and column grants; the runner can only invoke this routine.

CREATE SCHEMA pos_worker AUTHORIZATION pos_api_expiry_worker;
GRANT USAGE ON SCHEMA pos_worker TO pos_api_expiry_runner;
GRANT USAGE ON SCHEMA public TO pos_api_expiry_worker;

-- Staff policies consult membership tables through the transaction's identity
-- settings. Keep them scoped to the API role so the system role uses only its
-- narrowly defined worker policies and needs no staff-table privileges.
ALTER POLICY pickup_reservations_participant_read ON pickup_reservations TO pos_api_runtime;
ALTER POLICY pickup_reservations_create ON pickup_reservations TO pos_api_runtime;
ALTER POLICY pickup_reservations_transition ON pickup_reservations TO pos_api_runtime;
ALTER POLICY pickup_lines_participant_read ON pickup_reservation_lines TO pos_api_runtime;
ALTER POLICY pickup_lines_request_insert ON pickup_reservation_lines TO pos_api_runtime;
ALTER POLICY pickup_events_participant_read ON pickup_reservation_events TO pos_api_runtime;
ALTER POLICY pickup_events_append ON pickup_reservation_events TO pos_api_runtime;
ALTER POLICY inventory_allocations_branch_or_availability_read ON inventory_allocations TO pos_api_runtime;
ALTER POLICY inventory_allocations_transfer_insert ON inventory_allocations TO pos_api_runtime;
ALTER POLICY inventory_allocations_transfer_release ON inventory_allocations TO pos_api_runtime;
ALTER POLICY inventory_allocations_pickup_release ON inventory_allocations TO pos_api_runtime;

ALTER TABLE pickup_reservation_events
    ADD COLUMN actor_kind TEXT NOT NULL DEFAULT 'membership'
        CHECK (actor_kind IN ('membership','system'));
ALTER TABLE pickup_reservation_events
    ALTER COLUMN actor_membership_id DROP NOT NULL;
ALTER TABLE pickup_reservation_events
    ADD CONSTRAINT pickup_event_actor_kind_check
    CHECK ((actor_kind='membership' AND actor_membership_id IS NOT NULL)
        OR (actor_kind='system' AND actor_membership_id IS NULL
            AND event_type='expired' AND acting_station_id IS NULL AND reason IS NULL));

CREATE POLICY pickup_expiry_worker_reservation_read ON pickup_reservations
    FOR SELECT TO pos_api_expiry_worker
    USING (
        (status IN ('held','ready') AND expires_at<=clock_timestamp())
        OR (status='expired' AND sale_id IS NULL AND expires_at<=clock_timestamp())
    );

CREATE POLICY pickup_expiry_worker_reservation_update ON pickup_reservations
    FOR UPDATE TO pos_api_expiry_worker
    USING (status IN ('held','ready') AND sale_id IS NULL AND expires_at<=clock_timestamp())
    WITH CHECK (status='expired' AND sale_id IS NULL AND expires_at<=clock_timestamp());

CREATE POLICY pickup_expiry_worker_lines_read ON pickup_reservation_lines
    FOR SELECT TO pos_api_expiry_worker
    USING (EXISTS (
        SELECT 1 FROM pickup_reservations r
        WHERE r.tenant_id=pickup_reservation_lines.tenant_id
          AND r.id=pickup_reservation_lines.reservation_id
          AND r.status IN ('held','ready','expired')
          AND r.sale_id IS NULL AND r.expires_at<=clock_timestamp()
    ));

CREATE POLICY pickup_expiry_worker_events_read ON pickup_reservation_events
    FOR SELECT TO pos_api_expiry_worker
    USING (EXISTS (
        SELECT 1 FROM pickup_reservations r
        WHERE r.tenant_id=pickup_reservation_events.tenant_id
          AND r.id=pickup_reservation_events.reservation_id
          AND r.status='expired' AND r.sale_id IS NULL
          AND r.expires_at<=clock_timestamp() AND r.revision=pickup_reservation_events.revision
    ));

CREATE POLICY pickup_expiry_worker_events_insert ON pickup_reservation_events
    FOR INSERT TO pos_api_expiry_worker
    WITH CHECK (
        actor_kind='system' AND actor_membership_id IS NULL
        AND event_type='expired' AND acting_station_id IS NULL AND reason IS NULL
        AND idempotency_key LIKE 'system-expiry-%'
        AND EXISTS (
            SELECT 1 FROM pickup_reservations r
            WHERE r.tenant_id=pickup_reservation_events.tenant_id
              AND r.id=pickup_reservation_events.reservation_id
              AND r.status='expired' AND r.sale_id IS NULL
              AND r.expires_at<=clock_timestamp() AND r.revision=pickup_reservation_events.revision
              AND r.pickup_branch_id=pickup_reservation_events.acting_branch_id
        )
    );

CREATE POLICY pickup_expiry_worker_allocation_read ON inventory_allocations
    FOR SELECT TO pos_api_expiry_worker
    USING (
        pickup_reservation_id IS NOT NULL AND released_at IS NULL
        AND EXISTS (
            SELECT 1 FROM pickup_reservations r
            WHERE r.tenant_id=inventory_allocations.tenant_id
              AND r.id=inventory_allocations.pickup_reservation_id
              AND r.status IN ('held','ready','expired') AND r.sale_id IS NULL
              AND r.expires_at<=clock_timestamp()
        )
    );

CREATE POLICY pickup_expiry_worker_allocation_release ON inventory_allocations
    FOR UPDATE TO pos_api_expiry_worker
    USING (
        pickup_reservation_id IS NOT NULL AND released_at IS NULL
        AND EXISTS (
            SELECT 1 FROM pickup_reservations r
            JOIN pickup_reservation_events e
              ON e.tenant_id=r.tenant_id AND e.reservation_id=r.id
             AND e.revision=r.revision AND e.event_type='expired'
             AND e.actor_kind='system' AND e.actor_membership_id IS NULL
            WHERE r.tenant_id=inventory_allocations.tenant_id
              AND r.id=inventory_allocations.pickup_reservation_id
              AND r.status='expired' AND r.sale_id IS NULL
              AND r.expires_at<=clock_timestamp()
        )
    )
    WITH CHECK (
        pickup_reservation_id IS NOT NULL AND released_at IS NOT NULL
        AND release_reason='expired'
        AND EXISTS (
            SELECT 1 FROM pickup_reservations r
            JOIN pickup_reservation_events e
              ON e.tenant_id=r.tenant_id AND e.reservation_id=r.id
             AND e.revision=r.revision AND e.event_type='expired'
             AND e.actor_kind='system' AND e.actor_membership_id IS NULL
            WHERE r.tenant_id=inventory_allocations.tenant_id
              AND r.id=inventory_allocations.pickup_reservation_id
              AND r.status='expired' AND r.sale_id IS NULL
              AND r.expires_at<=clock_timestamp()
        )
    );

GRANT SELECT (id,tenant_id,origin_branch_id,origin_station_id,pickup_branch_id,
              requested_by_membership_id,status,revision,expires_at,sale_id)
    ON pickup_reservations TO pos_api_expiry_worker;
GRANT UPDATE (status,revision,updated_at)
    ON pickup_reservations TO pos_api_expiry_worker;
GRANT SELECT (tenant_id,reservation_id,product_id,pickup_branch_id)
    ON pickup_reservation_lines TO pos_api_expiry_worker;
GRANT SELECT (tenant_id,reservation_id,acting_branch_id,acting_station_id,
              actor_membership_id,actor_kind,event_type,revision,reason)
    ON pickup_reservation_events TO pos_api_expiry_worker;
GRANT INSERT (tenant_id,reservation_id,acting_branch_id,acting_station_id,
              actor_membership_id,actor_kind,event_type,revision,idempotency_key,
              request_hash,reason)
    ON pickup_reservation_events TO pos_api_expiry_worker;
GRANT SELECT (tenant_id,branch_id,product_id,pickup_reservation_id,quantity,
              released_at,release_reason)
    ON inventory_allocations TO pos_api_expiry_worker;
GRANT UPDATE (released_at,release_reason)
    ON inventory_allocations TO pos_api_expiry_worker;

CREATE OR REPLACE FUNCTION guard_pickup_reservation_transition() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='UPDATE' THEN
        IF ROW(NEW.tenant_id,NEW.origin_branch_id,NEW.origin_station_id,NEW.pickup_branch_id,
               NEW.requested_by_membership_id,NEW.customer_id,NEW.contact_name_snapshot,
               NEW.contact_phone_snapshot,NEW.contact_consent_at,NEW.expires_at,
               NEW.request_idempotency_key,NEW.request_hash,NEW.created_at)
           IS DISTINCT FROM
           ROW(OLD.tenant_id,OLD.origin_branch_id,OLD.origin_station_id,OLD.pickup_branch_id,
               OLD.requested_by_membership_id,OLD.customer_id,OLD.contact_name_snapshot,
               OLD.contact_phone_snapshot,OLD.contact_consent_at,OLD.expires_at,
               OLD.request_idempotency_key,OLD.request_hash,OLD.created_at) THEN
            RAISE EXCEPTION 'Pickup reservation details are immutable.';
        END IF;
        IF NEW.revision<>OLD.revision+1 OR NEW.updated_at<OLD.updated_at
           OR NOT ((OLD.status='held' AND NEW.status IN ('ready','cancelled') AND NEW.sale_id IS NOT DISTINCT FROM OLD.sale_id)
                   OR (OLD.status='ready' AND NEW.status='cancelled' AND NEW.sale_id IS NOT DISTINCT FROM OLD.sale_id)
                   OR (OLD.status='ready' AND NEW.status='collected' AND OLD.sale_id IS NULL AND NEW.sale_id IS NOT NULL)
                   OR (OLD.status IN ('held','ready') AND NEW.status='expired'
                       AND OLD.sale_id IS NULL AND NEW.sale_id IS NULL)) THEN
            RAISE EXCEPTION 'Pickup reservation transition is invalid.';
        END IF;
    ELSIF TG_OP='INSERT' AND (NEW.status<>'held' OR NEW.revision<>1 OR NEW.sale_id IS NOT NULL) THEN
        RAISE EXCEPTION 'Pickup reservation must begin as a held reservation.';
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_pickup_reservation_event_revision() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    current_status TEXT;
    current_revision BIGINT;
    matching_event TEXT;
    reservation_origin UUID;
    reservation_origin_station UUID;
    reservation_pickup UUID;
    reservation_requester UUID;
    reservation_sale UUID;
    reservation_expires TIMESTAMPTZ;
    event_branch UUID;
    event_station UUID;
    event_actor UUID;
    event_actor_kind TEXT;
    event_reason TEXT;
    event_authorized BOOLEAN;
BEGIN
    IF TG_TABLE_NAME='pickup_reservations' THEN
        current_status:=NEW.status;
        current_revision:=NEW.revision;
        SELECT event_type,acting_branch_id,acting_station_id,actor_membership_id,
               actor_kind,reason
          INTO matching_event,event_branch,event_station,event_actor,event_actor_kind,event_reason
          FROM pickup_reservation_events
         WHERE tenant_id=NEW.tenant_id AND reservation_id=NEW.id AND revision=NEW.revision;
        reservation_origin:=NEW.origin_branch_id;
        reservation_origin_station:=NEW.origin_station_id;
        reservation_pickup:=NEW.pickup_branch_id;
        reservation_requester:=NEW.requested_by_membership_id;
        reservation_sale:=NEW.sale_id;
        reservation_expires:=NEW.expires_at;
    ELSE
        SELECT status,revision,origin_branch_id,origin_station_id,pickup_branch_id,
               requested_by_membership_id,sale_id,expires_at
          INTO current_status,current_revision,reservation_origin,reservation_origin_station,
               reservation_pickup,reservation_requester,reservation_sale,reservation_expires
          FROM pickup_reservations
         WHERE tenant_id=NEW.tenant_id AND id=NEW.reservation_id;
        matching_event:=NEW.event_type;
        event_branch:=NEW.acting_branch_id;
        event_station:=NEW.acting_station_id;
        event_actor:=NEW.actor_membership_id;
        event_actor_kind:=NEW.actor_kind;
        event_reason:=NEW.reason;
    END IF;
    IF current_status IS NULL OR current_revision IS NULL OR matching_event IS DISTINCT FROM current_status THEN
        RAISE EXCEPTION 'Pickup reservation state must have a matching immutable event.';
    END IF;
    IF TG_TABLE_NAME<>'pickup_reservations' AND NEW.revision<>current_revision THEN
        RAISE EXCEPTION 'Pickup event revision does not match the reservation.';
    END IF;
    IF matching_event='held' THEN
        event_authorized:=event_actor_kind='membership'
            AND event_branch=reservation_origin AND event_station=reservation_origin_station
            AND event_actor=reservation_requester;
    ELSIF matching_event IN ('ready','collected') THEN
        event_authorized:=event_actor_kind='membership' AND event_branch=reservation_pickup
            AND EXISTS (
                SELECT 1 FROM membership_role_grants g
                 WHERE g.tenant_id=NEW.tenant_id AND g.membership_id=event_actor
                   AND g.role_key IN ('owner','admin','manager','cashier')
                   AND (g.branch_id IS NULL OR g.branch_id=event_branch)
                   AND (g.station_id IS NULL OR g.station_id IS NOT DISTINCT FROM event_station)
            );
    ELSIF matching_event='cancelled' THEN
        event_authorized:=event_actor_kind='membership' AND event_reason IS NOT NULL
            AND event_branch IN (reservation_origin,reservation_pickup);
    ELSIF matching_event='expired' THEN
        event_authorized:=event_actor_kind='system' AND event_actor IS NULL
            AND event_station IS NULL AND event_reason IS NULL
            AND event_branch=reservation_pickup AND reservation_sale IS NULL
            AND reservation_expires<=clock_timestamp();
    ELSE
        event_authorized:=FALSE;
    END IF;
    IF NOT COALESCE(event_authorized,FALSE) THEN
        RAISE EXCEPTION 'Pickup reservation event actor or branch is not authorized.';
    END IF;
    RETURN NULL;
END;
$$;

CREATE FUNCTION pos_worker.materialize_next_expired_pickup_reservation() RETURNS BOOLEAN
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path=pg_catalog,public
AS $$
DECLARE
    candidate RECORD;
    product RECORD;
    current_row RECORD;
    checked_at TIMESTAMPTZ;
BEGIN
    -- Discovery intentionally takes no reservation row lock. Inventory locks
    -- must precede the row lock to match cancellation and checkout ordering.
    SELECT id,tenant_id,pickup_branch_id
      INTO candidate
      FROM public.pickup_reservations
     WHERE status IN ('held','ready') AND sale_id IS NULL
       AND expires_at<=clock_timestamp()
     ORDER BY expires_at,tenant_id,id
     LIMIT 1;
    IF NOT FOUND THEN
        RETURN FALSE;
    END IF;

    FOR product IN
        SELECT product_id
          FROM public.pickup_reservation_lines
         WHERE tenant_id=candidate.tenant_id AND reservation_id=candidate.id
         ORDER BY product_id
    LOOP
        PERFORM pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended(
            format('advanced-pos:inventory:v1:%s:%s:%s',
                   candidate.tenant_id,candidate.pickup_branch_id,product.product_id),0));
    END LOOP;

    SELECT status,revision,expires_at,sale_id,pickup_branch_id
      INTO current_row
      FROM public.pickup_reservations
     WHERE tenant_id=candidate.tenant_id AND id=candidate.id
     FOR UPDATE;
    IF NOT FOUND THEN
        RETURN FALSE;
    END IF;

    checked_at:=clock_timestamp();
    IF current_row.status NOT IN ('held','ready') OR current_row.sale_id IS NOT NULL
       OR current_row.expires_at>checked_at THEN
        RETURN FALSE;
    END IF;

    UPDATE public.pickup_reservations
       SET status='expired',revision=revision+1,updated_at=checked_at
     WHERE tenant_id=candidate.tenant_id AND id=candidate.id
       AND revision=current_row.revision AND status=current_row.status
       AND sale_id IS NULL AND expires_at<=checked_at;
    IF NOT FOUND THEN
        RETURN FALSE;
    END IF;

    INSERT INTO public.pickup_reservation_events
        (tenant_id,reservation_id,acting_branch_id,acting_station_id,
         actor_membership_id,actor_kind,event_type,revision,idempotency_key,
         request_hash,reason)
    VALUES
        (candidate.tenant_id,candidate.id,current_row.pickup_branch_id,NULL,
         NULL,'system','expired',current_row.revision+1,
         'system-expiry-'||(current_row.revision+1)::TEXT,
         pg_catalog.decode(pg_catalog.repeat('00',32),'hex'),NULL);

    UPDATE public.inventory_allocations
       SET released_at=clock_timestamp(),release_reason='expired'
     WHERE tenant_id=candidate.tenant_id
       AND pickup_reservation_id=candidate.id AND released_at IS NULL;

    RETURN TRUE;
END;
$$;

ALTER FUNCTION pos_worker.materialize_next_expired_pickup_reservation() OWNER TO pos_api_expiry_worker;
REVOKE ALL ON FUNCTION pos_worker.materialize_next_expired_pickup_reservation() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION pos_worker.materialize_next_expired_pickup_reservation() TO pos_api_expiry_runner;
