-- Move pickup holds through preparation/cancellation with scoped authorization,
-- inventory-safe cancellation and a database-enforced append-only history.

DROP POLICY pickup_reservations_transition ON pickup_reservations;
CREATE POLICY pickup_reservations_transition ON pickup_reservations FOR UPDATE
USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND (origin_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
         OR pickup_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid)
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
          AND m.tenant_id=pickup_reservations.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager','cashier')
          AND (
            (m.id=pickup_reservations.requested_by_membership_id
             AND pickup_reservations.origin_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
             AND pickup_reservations.origin_station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
             AND (g.branch_id IS NULL OR g.branch_id=pickup_reservations.origin_branch_id)
             AND (g.station_id IS NULL OR g.station_id=pickup_reservations.origin_station_id))
            OR (g.role_key IN ('owner','admin','manager')
                AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid)
                AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid))
            OR (g.role_key='cashier'
                AND pickup_reservations.pickup_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
                AND (g.branch_id IS NULL OR g.branch_id=pickup_reservations.pickup_branch_id)
                AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid))
          )
    )
) WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND (origin_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
         OR pickup_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid)
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
          AND m.tenant_id=pickup_reservations.tenant_id AND m.status='active'
          AND (
            (pickup_reservations.status='ready'
             AND pickup_reservations.pickup_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
             AND g.role_key IN ('owner','admin','manager','cashier')
             AND (g.branch_id IS NULL OR g.branch_id=pickup_reservations.pickup_branch_id)
             AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid))
            OR (pickup_reservations.status='cancelled' AND (
                (m.id=pickup_reservations.requested_by_membership_id
                 AND pickup_reservations.origin_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
                 AND pickup_reservations.origin_station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid
                 AND g.role_key IN ('owner','admin','manager','cashier')
                 AND (g.branch_id IS NULL OR g.branch_id=pickup_reservations.origin_branch_id)
                 AND (g.station_id IS NULL OR g.station_id=pickup_reservations.origin_station_id))
                OR (g.role_key IN ('owner','admin','manager')
                    AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid)
                    AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid))
            ))
          )
    )
);

DROP POLICY pickup_events_append ON pickup_reservation_events;
CREATE POLICY pickup_events_append ON pickup_reservation_events FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND acting_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND EXISTS (
        SELECT 1 FROM memberships m
        WHERE m.id=pickup_reservation_events.actor_membership_id
          AND m.tenant_id=pickup_reservation_events.tenant_id
          AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
          AND m.status='active'
    )
    AND (event_type<>'cancelled' OR reason IS NOT NULL)
);

DROP POLICY inventory_allocations_pickup_release ON inventory_allocations;
CREATE POLICY inventory_allocations_pickup_release ON inventory_allocations FOR UPDATE
USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND pickup_reservation_id IS NOT NULL AND released_at IS NULL
    AND EXISTS (
        SELECT 1 FROM pickup_reservations r
        JOIN pickup_reservation_events e ON e.tenant_id=r.tenant_id AND e.reservation_id=r.id
          AND e.revision=r.revision AND e.event_type='cancelled'
        WHERE r.tenant_id=inventory_allocations.tenant_id
          AND r.id=inventory_allocations.pickup_reservation_id AND r.status='cancelled'
          AND e.actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
          AND e.acting_branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
          AND e.acting_station_id IS NOT DISTINCT FROM NULLIF(current_setting('pos.station_id',true),'')::uuid
          AND EXISTS (
              SELECT 1 FROM memberships m
              WHERE m.id=e.actor_membership_id AND m.tenant_id=r.tenant_id
                AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
                AND m.status='active'
          )
    )
) WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND pickup_reservation_id IS NOT NULL AND released_at IS NOT NULL AND release_reason IS NOT NULL
    AND EXISTS (
        SELECT 1 FROM pickup_reservations r WHERE r.tenant_id=inventory_allocations.tenant_id
          AND r.id=inventory_allocations.pickup_reservation_id AND r.status='cancelled'
    )
);

CREATE FUNCTION guard_pickup_reservation_transition() RETURNS trigger
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
        IF NEW.sale_id IS DISTINCT FROM OLD.sale_id THEN
            RAISE EXCEPTION 'A pickup sale can only be linked by checkout.';
        END IF;
        IF NEW.revision<>OLD.revision+1 OR NEW.updated_at<OLD.updated_at
           OR NOT ((OLD.status='held' AND NEW.status IN ('ready','cancelled'))
                   OR (OLD.status='ready' AND NEW.status='cancelled')) THEN
            RAISE EXCEPTION 'Pickup reservation transition is invalid.';
        END IF;
    ELSIF TG_OP='INSERT' AND (NEW.status<>'held' OR NEW.revision<>1 OR NEW.sale_id IS NOT NULL) THEN
        RAISE EXCEPTION 'Pickup reservation must begin as a held reservation.';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER pickup_reservations_transition_guard
    BEFORE INSERT OR UPDATE ON pickup_reservations
    FOR EACH ROW EXECUTE FUNCTION guard_pickup_reservation_transition();

CREATE FUNCTION enforce_pickup_reservation_event_revision() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    current_status TEXT;
    current_revision BIGINT;
    matching_event TEXT;
    reservation_origin UUID;
    reservation_origin_station UUID;
    reservation_pickup UUID;
    reservation_requester UUID;
    event_branch UUID;
    event_station UUID;
    event_actor UUID;
    event_reason TEXT;
    event_authorized BOOLEAN;
BEGIN
    IF TG_TABLE_NAME='pickup_reservations' THEN
        current_status := NEW.status;
        current_revision := NEW.revision;
        SELECT event_type,acting_branch_id,acting_station_id,actor_membership_id,reason
          INTO matching_event,event_branch,event_station,event_actor,event_reason
          FROM pickup_reservation_events
         WHERE tenant_id=NEW.tenant_id AND reservation_id=NEW.id AND revision=NEW.revision;
        reservation_origin := NEW.origin_branch_id;
        reservation_origin_station := NEW.origin_station_id;
        reservation_pickup := NEW.pickup_branch_id;
        reservation_requester := NEW.requested_by_membership_id;
    ELSE
        SELECT status,revision,origin_branch_id,origin_station_id,pickup_branch_id,requested_by_membership_id
          INTO current_status,current_revision,reservation_origin,reservation_origin_station,reservation_pickup,reservation_requester
          FROM pickup_reservations
         WHERE tenant_id=NEW.tenant_id AND id=NEW.reservation_id;
        matching_event := NEW.event_type;
        event_branch := NEW.acting_branch_id;
        event_station := NEW.acting_station_id;
        event_actor := NEW.actor_membership_id;
        event_reason := NEW.reason;
    END IF;
    IF current_status IS NULL OR current_revision IS NULL OR matching_event IS DISTINCT FROM current_status THEN
        RAISE EXCEPTION 'Pickup reservation state must have a matching immutable event.';
    END IF;
    IF TG_TABLE_NAME<>'pickup_reservations' AND NEW.revision<>current_revision THEN
        RAISE EXCEPTION 'Pickup event revision does not match the reservation.';
    END IF;
    IF matching_event='held' THEN
        event_authorized := event_branch=reservation_origin AND event_station=reservation_origin_station
            AND event_actor=reservation_requester;
    ELSIF matching_event='ready' THEN
        event_authorized := event_branch=reservation_pickup AND EXISTS (
            SELECT 1 FROM membership_role_grants g
             WHERE g.tenant_id=NEW.tenant_id AND g.membership_id=event_actor
               AND g.role_key IN ('owner','admin','manager','cashier')
               AND (g.branch_id IS NULL OR g.branch_id=event_branch)
               AND (g.station_id IS NULL OR g.station_id IS NOT DISTINCT FROM event_station)
        );
    ELSIF matching_event='cancelled' THEN
        -- The transition's header UPDATE RLS WITH CHECK is status-aware and
        -- authorizes requesters/managers while excluding pickup cashiers.
        -- Here require the event to stay within the reservation branches and
        -- carry a reason; event RLS binds its actor to the active DB context.
        event_authorized := event_reason IS NOT NULL
            AND event_branch IN (reservation_origin,reservation_pickup);
    ELSE
        event_authorized := FALSE;
    END IF;
    IF NOT COALESCE(event_authorized,FALSE) THEN
        RAISE EXCEPTION 'Pickup reservation event actor or branch is not authorized.';
    END IF;
    RETURN NULL;
END;
$$;
CREATE CONSTRAINT TRIGGER pickup_reservation_header_event_check
    AFTER INSERT OR UPDATE ON pickup_reservations
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
    EXECUTE FUNCTION enforce_pickup_reservation_event_revision();
CREATE CONSTRAINT TRIGGER pickup_reservation_event_state_check
    AFTER INSERT ON pickup_reservation_events
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
    EXECUTE FUNCTION enforce_pickup_reservation_event_revision();
