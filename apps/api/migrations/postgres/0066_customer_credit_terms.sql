-- Currency-specific tenant-wide customer credit terms. The write context is
-- tenant-scoped because a term affects exposure at every branch in that currency.
CREATE TABLE hosted_customer_credit_terms (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    customer_id UUID NOT NULL,
    currency_code CHAR(3) NOT NULL CHECK (currency_code ~ '^[A-Z]{3}$'),
    credit_limit_minor BIGINT NOT NULL CHECK (credit_limit_minor BETWEEN 0 AND 9007199254740991),
    due_days INTEGER NOT NULL CHECK (due_days BETWEEN 0 AND 365),
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    last_reason TEXT NOT NULL CHECK (length(btrim(last_reason)) BETWEEN 1 AND 500),
    updated_by_membership_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, customer_id)
        REFERENCES hosted_customers(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (updated_by_membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, customer_id, currency_code),
    UNIQUE (tenant_id, customer_id, currency_code, revision),
    UNIQUE (tenant_id, id)
);
CREATE INDEX hosted_customer_credit_terms_tenant_currency
    ON hosted_customer_credit_terms(tenant_id, currency_code, customer_id);

CREATE TABLE hosted_customer_credit_term_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    customer_id UUID NOT NULL,
    currency_code CHAR(3) NOT NULL CHECK (currency_code ~ '^[A-Z]{3}$'),
    revision BIGINT NOT NULL CHECK (revision > 0),
    previous_credit_limit_minor BIGINT,
    previous_due_days INTEGER,
    credit_limit_minor BIGINT NOT NULL CHECK (credit_limit_minor BETWEEN 0 AND 9007199254740991),
    due_days INTEGER NOT NULL CHECK (due_days BETWEEN 0 AND 365),
    reason TEXT NOT NULL CHECK (length(btrim(reason)) BETWEEN 1 AND 500),
    actor_membership_id UUID NOT NULL,
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((revision = 1 AND previous_credit_limit_minor IS NULL AND previous_due_days IS NULL)
        OR (revision > 1 AND previous_credit_limit_minor BETWEEN 0 AND 9007199254740991
            AND previous_due_days BETWEEN 0 AND 365)),
    FOREIGN KEY (tenant_id, customer_id)
        REFERENCES hosted_customers(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, customer_id, currency_code, revision),
    UNIQUE (tenant_id, customer_id, currency_code, actor_membership_id, idempotency_key)
);
CREATE INDEX hosted_customer_credit_term_events_customer
    ON hosted_customer_credit_term_events(tenant_id, customer_id, currency_code, revision DESC);

CREATE FUNCTION hosted_customer_credit_terms_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Customer credit terms cannot be deleted' USING ERRCODE='55000';
    END IF;
    IF NEW.tenant_id <> OLD.tenant_id OR NEW.customer_id <> OLD.customer_id
       OR NEW.currency_code <> OLD.currency_code
       OR NEW.revision <> OLD.revision + 1
       OR (NEW.credit_limit_minor, NEW.due_days) IS NOT DISTINCT FROM (OLD.credit_limit_minor, OLD.due_days) THEN
        RAISE EXCEPTION 'Customer credit terms require an identity-preserving, substantive next revision' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER hosted_customer_credit_terms_revision_guard
    BEFORE UPDATE OR DELETE ON hosted_customer_credit_terms
    FOR EACH ROW EXECUTE FUNCTION hosted_customer_credit_terms_guard();

CREATE FUNCTION hosted_customer_credit_term_event_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'Customer credit term history is append-only' USING ERRCODE='55000';
END;
$$;
CREATE TRIGGER hosted_customer_credit_term_events_no_update_delete
    BEFORE UPDATE OR DELETE ON hosted_customer_credit_term_events
    FOR EACH ROW EXECUTE FUNCTION hosted_customer_credit_term_event_immutable();

CREATE FUNCTION hosted_customer_credit_term_event_state_check() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE current_term hosted_customer_credit_terms%ROWTYPE;
BEGIN
    SELECT * INTO current_term FROM hosted_customer_credit_terms
    WHERE tenant_id=NEW.tenant_id AND customer_id=NEW.customer_id
      AND currency_code=NEW.currency_code AND revision=NEW.revision;
    IF NOT FOUND OR current_term.credit_limit_minor <> NEW.credit_limit_minor
       OR current_term.due_days <> NEW.due_days
       OR current_term.last_reason <> NEW.reason
       OR current_term.updated_by_membership_id <> NEW.actor_membership_id
       OR (NEW.revision = 1 AND (NEW.previous_credit_limit_minor IS NOT NULL OR NEW.previous_due_days IS NOT NULL)) THEN
        RAISE EXCEPTION 'Customer credit term event must match the committed revision' USING ERRCODE='23514';
    END IF;
    IF NEW.revision > 1 AND NOT EXISTS (
        SELECT 1 FROM hosted_customer_credit_terms t
        WHERE t.tenant_id=NEW.tenant_id AND t.customer_id=NEW.customer_id
          AND t.currency_code=NEW.currency_code AND t.revision=NEW.revision
          AND EXISTS (
              SELECT 1 FROM hosted_customer_credit_term_events prior
              WHERE prior.tenant_id=NEW.tenant_id AND prior.customer_id=NEW.customer_id
                AND prior.currency_code=NEW.currency_code AND prior.revision=NEW.revision-1
                AND prior.credit_limit_minor=NEW.previous_credit_limit_minor
                AND prior.due_days=NEW.previous_due_days
          )
    ) THEN
        RAISE EXCEPTION 'Customer credit term revision must continue its append-only history' USING ERRCODE='23514';
    END IF;
    RETURN NULL;
END;
$$;
CREATE FUNCTION hosted_customer_credit_term_event_required_check() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM hosted_customer_credit_term_events e
        WHERE e.tenant_id=NEW.tenant_id AND e.customer_id=NEW.customer_id
          AND e.currency_code=NEW.currency_code AND e.revision=NEW.revision
          AND e.credit_limit_minor=NEW.credit_limit_minor AND e.due_days=NEW.due_days
          AND e.reason=NEW.last_reason AND e.actor_membership_id=NEW.updated_by_membership_id
    ) THEN
        RAISE EXCEPTION 'Customer credit term revision requires a matching append-only event' USING ERRCODE='23514';
    END IF;
    RETURN NULL;
END;
$$;
CREATE CONSTRAINT TRIGGER hosted_customer_credit_term_event_required
    AFTER INSERT OR UPDATE ON hosted_customer_credit_terms DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION hosted_customer_credit_term_event_required_check();
CREATE CONSTRAINT TRIGGER hosted_customer_credit_term_event_state_matches
    AFTER INSERT ON hosted_customer_credit_term_events DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION hosted_customer_credit_term_event_state_check();

ALTER TABLE hosted_customer_credit_terms ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_customer_credit_terms FORCE ROW LEVEL SECURITY;
ALTER TABLE hosted_customer_credit_term_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_customer_credit_term_events FORCE ROW LEVEL SECURITY;

CREATE POLICY hosted_customer_credit_terms_branch_read ON hosted_customer_credit_terms
    FOR SELECT USING (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
            WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
              AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
              AND m.tenant_id=hosted_customer_credit_terms.tenant_id AND m.status='active'
              AND g.role_key IN ('owner','admin','manager','cashier')
              AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid)
              AND (g.station_id IS NULL OR (g.role_key='cashier'
                   AND g.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid))
        )
    );
CREATE POLICY hosted_customer_credit_terms_tenant_insert ON hosted_customer_credit_terms
    FOR INSERT WITH CHECK (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND updated_by_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND revision=1
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
            WHERE m.id=updated_by_membership_id
              AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
              AND m.tenant_id=hosted_customer_credit_terms.tenant_id AND m.status='active'
              AND g.role_key IN ('owner','admin','manager')
              AND g.branch_id IS NULL AND g.station_id IS NULL
        )
    );
CREATE POLICY hosted_customer_credit_terms_tenant_update ON hosted_customer_credit_terms
    FOR UPDATE USING (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
            WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
              AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
              AND m.tenant_id=hosted_customer_credit_terms.tenant_id AND m.status='active'
              AND g.role_key IN ('owner','admin','manager')
              AND g.branch_id IS NULL AND g.station_id IS NULL
        )
    ) WITH CHECK (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND updated_by_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
            WHERE m.id=updated_by_membership_id
              AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
              AND m.tenant_id=hosted_customer_credit_terms.tenant_id AND m.status='active'
              AND g.role_key IN ('owner','admin','manager')
              AND g.branch_id IS NULL AND g.station_id IS NULL
        )
    );

CREATE POLICY hosted_customer_credit_term_events_branch_read ON hosted_customer_credit_term_events
    FOR SELECT USING (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
            WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
              AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
              AND m.tenant_id=hosted_customer_credit_term_events.tenant_id AND m.status='active'
              AND g.role_key IN ('owner','admin','manager')
              AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid)
              AND g.station_id IS NULL
        )
    );
CREATE POLICY hosted_customer_credit_term_events_tenant_insert ON hosted_customer_credit_term_events
    FOR INSERT WITH CHECK (
        tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
        AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND EXISTS (
            SELECT 1 FROM memberships m
            JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
            WHERE m.id=actor_membership_id
              AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
              AND m.tenant_id=hosted_customer_credit_term_events.tenant_id AND m.status='active'
              AND g.role_key IN ('owner','admin','manager')
              AND g.branch_id IS NULL AND g.station_id IS NULL
        )
    );

