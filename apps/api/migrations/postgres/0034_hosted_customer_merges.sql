-- Explicit manager-reviewed customer deduplication. Historical sales and receipt
-- snapshots keep their original customer identity; this table records the alias.
ALTER TABLE hosted_customers
    ADD COLUMN merged_into_customer_id UUID;

ALTER TABLE hosted_customers
    DROP CONSTRAINT hosted_customers_status_check,
    ADD CONSTRAINT hosted_customers_status_check
        CHECK (status IN ('active','archived','merged')),
    ADD CONSTRAINT hosted_customers_merge_state_check
        CHECK ((status='merged') = (merged_into_customer_id IS NOT NULL)),
    ADD CONSTRAINT hosted_customers_merge_not_self_check
        CHECK (merged_into_customer_id IS NULL OR merged_into_customer_id <> id),
    ADD CONSTRAINT hosted_customers_merge_target_fk
        FOREIGN KEY (tenant_id,merged_into_customer_id)
        REFERENCES hosted_customers(tenant_id,id) ON DELETE RESTRICT;

CREATE TABLE hosted_customer_merges (
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    source_customer_id UUID NOT NULL,
    target_customer_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    actor_membership_id UUID NOT NULL,
    source_revision INTEGER NOT NULL CHECK (source_revision > 1),
    target_revision INTEGER NOT NULL CHECK (target_revision > 0),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
    merged_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id,source_customer_id),
    UNIQUE (tenant_id,actor_membership_id,idempotency_key),
    CHECK (source_customer_id <> target_customer_id),
    FOREIGN KEY (branch_id,tenant_id) REFERENCES branches(id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_membership_id,tenant_id) REFERENCES memberships(id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id,source_customer_id) REFERENCES hosted_customers(tenant_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id,target_customer_id) REFERENCES hosted_customers(tenant_id,id) ON DELETE RESTRICT
);

ALTER TABLE hosted_customer_merges ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_customer_merges FORCE ROW LEVEL SECURITY;
CREATE POLICY hosted_customer_merges_read ON hosted_customer_merges FOR SELECT USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (
      SELECT 1 FROM memberships m JOIN membership_role_grants g
        ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.tenant_id=hosted_customer_merges.tenant_id AND m.status='active'
        AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=hosted_customer_merges.branch_id)
        AND g.station_id IS NULL
    )
);
CREATE POLICY hosted_customer_merges_insert ON hosted_customer_merges FOR INSERT WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND EXISTS (
      SELECT 1 FROM hosted_customers source
      WHERE source.tenant_id=hosted_customer_merges.tenant_id
        AND source.id=hosted_customer_merges.source_customer_id
        AND source.status='merged'
        AND source.merged_into_customer_id=hosted_customer_merges.target_customer_id
        AND source.revision=hosted_customer_merges.source_revision
    )
    AND EXISTS (
      SELECT 1 FROM hosted_customers target
      WHERE target.tenant_id=hosted_customer_merges.tenant_id
        AND target.id=hosted_customer_merges.target_customer_id
        AND target.status='active'
        AND target.revision=hosted_customer_merges.target_revision
    )
    AND EXISTS (
      SELECT 1 FROM memberships m JOIN membership_role_grants g
        ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=actor_membership_id
        AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.tenant_id=hosted_customer_merges.tenant_id AND m.status='active'
        AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=hosted_customer_merges.branch_id)
        AND g.station_id IS NULL
    )
);

CREATE FUNCTION require_customer_merge_event() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.status='merged' AND NOT EXISTS (
      SELECT 1 FROM hosted_customer_merges m
      WHERE m.tenant_id=NEW.tenant_id AND m.source_customer_id=NEW.id
        AND m.target_customer_id=NEW.merged_into_customer_id
        AND m.source_revision=NEW.revision
    ) THEN
      RAISE EXCEPTION 'A merged customer must have its matching append-only merge event.'
        USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER hosted_customer_merge_event_required
    AFTER INSERT OR UPDATE ON hosted_customers
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW WHEN (NEW.status='merged')
    EXECUTE FUNCTION require_customer_merge_event();

CREATE POLICY hosted_customers_merge_read ON hosted_customers FOR SELECT USING (
    status='merged'
    AND tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND EXISTS (
      SELECT 1 FROM memberships m JOIN membership_role_grants g
        ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.tenant_id=hosted_customers.tenant_id AND m.status='active'
        AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid)
        AND g.station_id IS NULL
    )
);

CREATE POLICY hosted_customers_merge_update ON hosted_customers FOR UPDATE
USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND status='active'
    AND EXISTS (
      SELECT 1 FROM memberships m JOIN membership_role_grants g
        ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.tenant_id=hosted_customers.tenant_id AND m.status='active'
        AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid)
        AND g.station_id IS NULL
    )
)
WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND status='merged'
    AND merged_into_customer_id IS NOT NULL
    AND EXISTS (
      SELECT 1 FROM memberships m JOIN membership_role_grants g
        ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
      WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
        AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
        AND m.tenant_id=hosted_customers.tenant_id AND m.status='active'
        AND g.role_key IN ('owner','admin','manager')
        AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid)
        AND g.station_id IS NULL
    )
);

GRANT SELECT (merged_into_customer_id) ON hosted_customers TO pos_api_runtime;
GRANT UPDATE (merged_into_customer_id) ON hosted_customers TO pos_api_runtime;
GRANT SELECT (tenant_id,source_customer_id,target_customer_id,branch_id,actor_membership_id,source_revision,target_revision,idempotency_key,request_hash,merged_at)
  ON hosted_customer_merges TO pos_api_runtime;
GRANT INSERT (tenant_id,source_customer_id,target_customer_id,branch_id,actor_membership_id,source_revision,target_revision,idempotency_key,request_hash)
  ON hosted_customer_merges TO pos_api_runtime;
