-- Hosted attendance is keyed to the employee's tenant membership, not the
-- temporary cashier profile used for a POS transaction.

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'pos_attendance_worker') THEN
        CREATE ROLE pos_attendance_worker NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'pos_attendance_runner') THEN
        CREATE ROLE pos_attendance_runner LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
    END IF;
END $$;

CREATE TABLE attendance_schedules (
    tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    branch_id uuid NOT NULL,
    membership_id uuid NOT NULL,
    weekdays smallint[] NOT NULL,
    start_time time without time zone NOT NULL,
    end_time time without time zone NOT NULL,
    enabled boolean NOT NULL DEFAULT true,
    revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_by_membership_id uuid NOT NULL,
    updated_by_membership_id uuid NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (tenant_id, branch_id, membership_id),
    FOREIGN KEY (branch_id, tenant_id) REFERENCES branches(id, tenant_id) ON DELETE CASCADE,
    FOREIGN KEY (membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE CASCADE,
    FOREIGN KEY (created_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id),
    FOREIGN KEY (updated_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id),
    CHECK (cardinality(weekdays) BETWEEN 1 AND 7),
    CHECK (array_position(weekdays, NULL) IS NULL),
    CHECK (weekdays <@ ARRAY[0,1,2,3,4,5,6]::smallint[]),
    CHECK (start_time <> end_time)
);

CREATE TABLE attendance_periods (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    branch_id uuid NOT NULL,
    membership_id uuid NOT NULL,
    schedule_revision bigint,
    timezone text NOT NULL,
    scheduled_start_at timestamptz NOT NULL,
    due_at timestamptz NOT NULL,
    started_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    ended_at timestamptz,
    end_reason text,
    revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    started_by_membership_id uuid NOT NULL,
    ended_by_membership_id uuid,
    UNIQUE (tenant_id, id),
    FOREIGN KEY (branch_id, tenant_id) REFERENCES branches(id, tenant_id) ON DELETE CASCADE,
    FOREIGN KEY (membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE CASCADE,
    FOREIGN KEY (started_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id),
    FOREIGN KEY (ended_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id),
    CHECK (due_at > scheduled_start_at),
    CHECK (ended_at IS NULL OR ended_at >= started_at),
    CHECK ((ended_at IS NULL AND end_reason IS NULL AND ended_by_membership_id IS NULL)
        OR (ended_at IS NOT NULL AND end_reason IN ('staff', 'schedule')
            AND ((end_reason = 'staff' AND ended_by_membership_id IS NOT NULL)
              OR (end_reason = 'schedule' AND ended_by_membership_id IS NULL))))
);

CREATE UNIQUE INDEX attendance_one_open_period_per_membership
    ON attendance_periods (tenant_id, membership_id) WHERE ended_at IS NULL;
CREATE INDEX attendance_periods_branch_history
    ON attendance_periods (tenant_id, branch_id, started_at DESC, id DESC);
CREATE INDEX attendance_periods_due
    ON attendance_periods (due_at, id) WHERE ended_at IS NULL;

CREATE TABLE attendance_events (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    period_id uuid NOT NULL,
    branch_id uuid NOT NULL,
    membership_id uuid NOT NULL,
    actor_membership_id uuid,
    event_type text NOT NULL CHECK (event_type IN ('clocked_in', 'clocked_out', 'scheduled_end')),
    occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    idempotency_key text,
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    FOREIGN KEY (tenant_id, period_id) REFERENCES attendance_periods(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (branch_id, tenant_id) REFERENCES branches(id, tenant_id) ON DELETE CASCADE,
    FOREIGN KEY (membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE CASCADE,
    FOREIGN KEY (actor_membership_id, tenant_id) REFERENCES memberships(id, tenant_id),
    CHECK (idempotency_key IS NULL OR (length(idempotency_key) BETWEEN 1 AND 128)),
    CHECK (event_type <> 'scheduled_end' OR (actor_membership_id IS NULL AND idempotency_key IS NULL)),
    CHECK (event_type = 'scheduled_end' OR (actor_membership_id IS NOT NULL AND idempotency_key IS NOT NULL))
);
CREATE UNIQUE INDEX attendance_event_idempotency
    ON attendance_events(tenant_id, membership_id, idempotency_key) WHERE idempotency_key IS NOT NULL;
CREATE INDEX attendance_events_period ON attendance_events(tenant_id, period_id, occurred_at, id);

ALTER TABLE attendance_schedules ENABLE ROW LEVEL SECURITY;
ALTER TABLE attendance_schedules FORCE ROW LEVEL SECURITY;
ALTER TABLE attendance_periods ENABLE ROW LEVEL SECURITY;
ALTER TABLE attendance_periods FORCE ROW LEVEL SECURITY;
ALTER TABLE attendance_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE attendance_events FORCE ROW LEVEL SECURITY;

CREATE FUNCTION attendance_actor_can_manage(target_tenant uuid, target_branch uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY INVOKER
SET search_path = pg_catalog, public
AS $$
    SELECT EXISTS (
        SELECT 1 FROM public.memberships actor
        JOIN public.membership_role_grants grant_row
          ON grant_row.tenant_id = actor.tenant_id AND grant_row.membership_id = actor.id
        WHERE actor.id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
          AND actor.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND actor.tenant_id = target_tenant AND actor.status = 'active'
          AND (grant_row.role_key IN ('owner','admin') AND grant_row.branch_id IS NULL AND grant_row.station_id IS NULL
            OR grant_row.role_key = 'manager' AND (grant_row.branch_id IS NULL OR grant_row.branch_id = target_branch) AND grant_row.station_id IS NULL)
    )
$$;
REVOKE ALL ON FUNCTION attendance_actor_can_manage(uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION attendance_actor_can_manage(uuid, uuid) TO pos_api_runtime;

CREATE POLICY attendance_schedules_read ON attendance_schedules FOR SELECT USING (
    tenant_id::text = current_setting('pos.tenant_id', true)
    AND branch_id::text = current_setting('pos.branch_id', true)
    AND (membership_id::text = current_setting('pos.membership_id', true)
         OR attendance_actor_can_manage(tenant_id, branch_id))
);
CREATE POLICY attendance_schedules_write ON attendance_schedules FOR ALL USING (
    tenant_id::text = current_setting('pos.tenant_id', true)
    AND branch_id::text = current_setting('pos.branch_id', true)
    AND attendance_actor_can_manage(tenant_id, branch_id)
) WITH CHECK (
    tenant_id::text = current_setting('pos.tenant_id', true)
    AND branch_id::text = current_setting('pos.branch_id', true)
    AND attendance_actor_can_manage(tenant_id, branch_id)
);

CREATE POLICY attendance_periods_read ON attendance_periods FOR SELECT USING (
    (current_user = 'pos_attendance_worker' AND current_setting('pos.attendance_worker', true) = 'true')
    OR (tenant_id::text = current_setting('pos.tenant_id', true)
        AND branch_id::text = current_setting('pos.branch_id', true)
        AND (membership_id::text = current_setting('pos.membership_id', true)
             OR current_setting('pos.attendance_manager_read', true) = 'true'))
);
CREATE POLICY attendance_periods_insert ON attendance_periods FOR INSERT WITH CHECK (
    tenant_id::text = current_setting('pos.tenant_id', true)
    AND branch_id::text = current_setting('pos.branch_id', true)
    AND membership_id::text = current_setting('pos.membership_id', true)
    AND started_by_membership_id::text = current_setting('pos.membership_id', true)
    AND ended_at IS NULL
);
CREATE POLICY attendance_periods_update ON attendance_periods FOR UPDATE USING (
    tenant_id::text = current_setting('pos.tenant_id', true)
    AND branch_id::text = current_setting('pos.branch_id', true)
    AND membership_id::text = current_setting('pos.membership_id', true)
    AND ended_at IS NULL
) WITH CHECK (
    tenant_id::text = current_setting('pos.tenant_id', true)
    AND branch_id::text = current_setting('pos.branch_id', true)
    AND membership_id::text = current_setting('pos.membership_id', true)
);

CREATE POLICY attendance_events_read ON attendance_events FOR SELECT USING (
    tenant_id::text = current_setting('pos.tenant_id', true)
    AND branch_id::text = current_setting('pos.branch_id', true)
    AND (membership_id::text = current_setting('pos.membership_id', true)
         OR attendance_actor_can_manage(tenant_id, branch_id))
);
CREATE POLICY attendance_events_insert ON attendance_events FOR INSERT WITH CHECK (
    tenant_id::text = current_setting('pos.tenant_id', true)
    AND branch_id::text = current_setting('pos.branch_id', true)
    AND (
        (membership_id::text = current_setting('pos.membership_id', true)
         AND actor_membership_id::text = current_setting('pos.membership_id', true)
         AND event_type IN ('clocked_in', 'clocked_out'))
        OR (current_user = 'pos_attendance_worker' AND current_setting('pos.attendance_worker', true) = 'true'
            AND actor_membership_id IS NULL AND event_type = 'scheduled_end')
    )
);

CREATE POLICY attendance_worker_periods ON attendance_periods FOR ALL USING (
    current_user = 'pos_attendance_worker' AND current_setting('pos.attendance_worker', true) = 'true'
) WITH CHECK (current_user = 'pos_attendance_worker' AND current_setting('pos.attendance_worker', true) = 'true');
CREATE POLICY attendance_worker_events ON attendance_events FOR ALL USING (
    current_user = 'pos_attendance_worker' AND current_setting('pos.attendance_worker', true) = 'true'
) WITH CHECK (current_user = 'pos_attendance_worker' AND current_setting('pos.attendance_worker', true) = 'true');

GRANT SELECT (tenant_id, branch_id, membership_id, weekdays, start_time, end_time, enabled, revision, updated_at),
      INSERT (tenant_id, branch_id, membership_id, weekdays, start_time, end_time, enabled, created_by_membership_id, updated_by_membership_id),
      UPDATE (weekdays, start_time, end_time, enabled, revision, updated_by_membership_id, updated_at)
    ON attendance_schedules TO pos_api_runtime;
GRANT SELECT (tenant_id, id, branch_id, membership_id, schedule_revision, timezone, scheduled_start_at, due_at, started_at, ended_at, end_reason, revision, started_by_membership_id, ended_by_membership_id),
      INSERT (tenant_id, branch_id, membership_id, schedule_revision, timezone, scheduled_start_at, due_at, started_by_membership_id),
      UPDATE (ended_at, end_reason, revision, ended_by_membership_id)
    ON attendance_periods TO pos_api_runtime;
GRANT SELECT (tenant_id, id, period_id, branch_id, membership_id, actor_membership_id, event_type, occurred_at, idempotency_key, metadata),
      INSERT (tenant_id, period_id, branch_id, membership_id, actor_membership_id, event_type, idempotency_key, metadata)
    ON attendance_events TO pos_api_runtime;
GRANT USAGE ON SCHEMA public TO pos_api_runtime;

GRANT SELECT (id, due_at, ended_at, tenant_id, branch_id, membership_id, revision) ON attendance_periods TO pos_attendance_worker;
GRANT UPDATE (ended_at, end_reason, revision) ON attendance_periods TO pos_attendance_worker;
GRANT INSERT (tenant_id, period_id, branch_id, membership_id, actor_membership_id, event_type, occurred_at) ON attendance_events TO pos_attendance_worker;
GRANT USAGE ON SCHEMA public TO pos_attendance_worker;
GRANT USAGE, CREATE ON SCHEMA pos_worker TO pos_attendance_worker;

CREATE FUNCTION pos_worker.end_next_due_attendance_period() RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public, pos_worker
AS $$
DECLARE
    target record;
BEGIN
    PERFORM set_config('pos.attendance_worker', 'true', true);
    SELECT id, due_at, tenant_id, branch_id, membership_id
      INTO target
      FROM public.attendance_periods
     WHERE ended_at IS NULL AND due_at <= clock_timestamp()
     ORDER BY due_at, id
     FOR UPDATE SKIP LOCKED
     LIMIT 1;
    IF NOT FOUND THEN RETURN false; END IF;

    PERFORM set_config('pos.tenant_id', target.tenant_id::text, true);
    PERFORM set_config('pos.branch_id', target.branch_id::text, true);
    PERFORM set_config('pos.membership_id', target.membership_id::text, true);

    UPDATE public.attendance_periods
       SET ended_at = due_at, end_reason = 'schedule', revision = revision + 1
     WHERE id = target.id AND ended_at IS NULL;
    IF NOT FOUND THEN RETURN false; END IF;

    INSERT INTO public.attendance_events(tenant_id, period_id, branch_id, membership_id, actor_membership_id, event_type, occurred_at)
    VALUES(target.tenant_id, target.id, target.branch_id, target.membership_id, NULL, 'scheduled_end', target.due_at);
    RETURN true;
END;
$$;
ALTER FUNCTION pos_worker.end_next_due_attendance_period() OWNER TO pos_attendance_worker;
REVOKE CREATE ON SCHEMA pos_worker FROM pos_attendance_worker;
REVOKE ALL ON FUNCTION pos_worker.end_next_due_attendance_period() FROM PUBLIC;
REVOKE ALL ON FUNCTION pos_worker.end_next_due_attendance_period() FROM pos_api_runtime;
GRANT USAGE ON SCHEMA pos_worker TO pos_attendance_runner;
GRANT EXECUTE ON FUNCTION pos_worker.end_next_due_attendance_period() TO pos_attendance_runner;
