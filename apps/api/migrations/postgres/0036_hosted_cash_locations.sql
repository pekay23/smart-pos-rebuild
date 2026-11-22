-- Physical cash subledger. Balances are derived from immutable events; these
-- tables deliberately do not create general-ledger postings.
CREATE TABLE hosted_cash_locations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    name TEXT NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    location_type TEXT NOT NULL CHECK (location_type IN ('register','safe','petty_cash','other')),
    station_id UUID,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','inactive')),
    created_by UUID NOT NULL,
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (branch_id,tenant_id) REFERENCES branches(id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (created_by,tenant_id) REFERENCES memberships(id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (station_id,branch_id,tenant_id) REFERENCES stations(id,branch_id,tenant_id) ON DELETE RESTRICT,
    CHECK ((location_type='register')=(station_id IS NOT NULL)),
    UNIQUE (tenant_id,branch_id,id),
    UNIQUE (tenant_id,branch_id,name),
    UNIQUE (tenant_id,branch_id,created_by,idempotency_key)
);
CREATE UNIQUE INDEX hosted_cash_location_register_station
  ON hosted_cash_locations(tenant_id,branch_id,station_id) WHERE location_type='register';

CREATE TABLE hosted_cash_transfers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    actor_membership_id UUID NOT NULL,
    from_location_id UUID NOT NULL,
    to_location_id UUID NOT NULL,
    amount_minor BIGINT NOT NULL CHECK (amount_minor BETWEEN 1 AND 9007199254740991),
    note TEXT NOT NULL CHECK (length(btrim(note)) BETWEEN 1 AND 240),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (branch_id,tenant_id) REFERENCES branches(id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_membership_id,tenant_id) REFERENCES memberships(id,tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id,branch_id,from_location_id) REFERENCES hosted_cash_locations(tenant_id,branch_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id,branch_id,to_location_id) REFERENCES hosted_cash_locations(tenant_id,branch_id,id) ON DELETE RESTRICT,
    CHECK (from_location_id<>to_location_id),
    UNIQUE (tenant_id,branch_id,id),
    UNIQUE (tenant_id,branch_id,actor_membership_id,idempotency_key)
);

CREATE TABLE hosted_cash_location_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    location_id UUID NOT NULL,
    event_type TEXT NOT NULL CHECK (event_type IN ('opening','transfer_out','transfer_in')),
    source_transfer_id UUID,
    signed_amount_minor BIGINT NOT NULL CHECK (signed_amount_minor BETWEEN -9007199254740991 AND 9007199254740991),
    actor_membership_id UUID NOT NULL,
    idempotency_key TEXT,
    request_hash BYTEA,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id,branch_id,location_id) REFERENCES hosted_cash_locations(tenant_id,branch_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id,branch_id,source_transfer_id) REFERENCES hosted_cash_transfers(tenant_id,branch_id,id) ON DELETE RESTRICT,
    FOREIGN KEY (actor_membership_id,tenant_id) REFERENCES memberships(id,tenant_id) ON DELETE RESTRICT,
    CHECK (
      (event_type='opening' AND source_transfer_id IS NULL AND signed_amount_minor>=0) OR
      (event_type='transfer_out' AND source_transfer_id IS NOT NULL AND signed_amount_minor<0) OR
      (event_type='transfer_in' AND source_transfer_id IS NOT NULL AND signed_amount_minor>0)
    ),
    CHECK ((event_type='opening')=(idempotency_key IS NOT NULL AND request_hash IS NOT NULL)),
    CHECK (request_hash IS NULL OR octet_length(request_hash)=32),
    UNIQUE (tenant_id,branch_id,id),
    UNIQUE (tenant_id,source_transfer_id,event_type)
);
CREATE INDEX hosted_cash_location_events_history
  ON hosted_cash_location_events(tenant_id,branch_id,location_id,created_at DESC,id DESC);
CREATE UNIQUE INDEX hosted_cash_location_one_opening
  ON hosted_cash_location_events(tenant_id,location_id) WHERE event_type='opening';
CREATE UNIQUE INDEX hosted_cash_event_opening_retry
  ON hosted_cash_location_events(tenant_id,branch_id,actor_membership_id,idempotency_key) WHERE event_type='opening';

-- Ensure each transfer commits exactly one matching outgoing and incoming leg.
CREATE FUNCTION check_hosted_cash_transfer_legs() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
  transfer_row hosted_cash_transfers%ROWTYPE;
  out_count INTEGER;
  in_count INTEGER;
  out_event hosted_cash_location_events%ROWTYPE;
  in_event hosted_cash_location_events%ROWTYPE;
BEGIN
  SELECT * INTO transfer_row FROM hosted_cash_transfers
   WHERE tenant_id=NEW.tenant_id AND branch_id=NEW.branch_id AND id=NEW.id;
  SELECT count(*) INTO out_count FROM hosted_cash_location_events
   WHERE tenant_id=NEW.tenant_id AND source_transfer_id=NEW.id AND event_type='transfer_out';
  SELECT count(*) INTO in_count FROM hosted_cash_location_events
   WHERE tenant_id=NEW.tenant_id AND source_transfer_id=NEW.id AND event_type='transfer_in';
  IF out_count<>1 OR in_count<>1 THEN RAISE EXCEPTION 'cash transfer requires two legs' USING ERRCODE='23514'; END IF;
  SELECT * INTO out_event FROM hosted_cash_location_events WHERE tenant_id=NEW.tenant_id AND source_transfer_id=NEW.id AND event_type='transfer_out';
  SELECT * INTO in_event FROM hosted_cash_location_events WHERE tenant_id=NEW.tenant_id AND source_transfer_id=NEW.id AND event_type='transfer_in';
  IF out_event.location_id<>transfer_row.from_location_id OR in_event.location_id<>transfer_row.to_location_id
     OR out_event.signed_amount_minor<>-transfer_row.amount_minor OR in_event.signed_amount_minor<>transfer_row.amount_minor
     OR out_event.actor_membership_id<>transfer_row.actor_membership_id OR in_event.actor_membership_id<>transfer_row.actor_membership_id
  THEN RAISE EXCEPTION 'cash transfer legs do not match header' USING ERRCODE='23514'; END IF;
  RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER hosted_cash_transfer_complete
  AFTER INSERT ON hosted_cash_transfers DEFERRABLE INITIALLY DEFERRED
  FOR EACH ROW EXECUTE FUNCTION check_hosted_cash_transfer_legs();

ALTER TABLE hosted_cash_locations ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_cash_locations FORCE ROW LEVEL SECURITY;
ALTER TABLE hosted_cash_transfers ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_cash_transfers FORCE ROW LEVEL SECURITY;
ALTER TABLE hosted_cash_location_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_cash_location_events FORCE ROW LEVEL SECURITY;

CREATE POLICY hosted_cash_locations_branch_read ON hosted_cash_locations FOR SELECT USING (
 tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
 AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
 AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
   WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
   AND m.tenant_id=hosted_cash_locations.tenant_id AND m.status='active'
   AND g.role_key IN ('owner','admin','manager') AND (g.branch_id IS NULL OR g.branch_id=hosted_cash_locations.branch_id))
);
CREATE POLICY hosted_cash_locations_branch_insert ON hosted_cash_locations FOR INSERT WITH CHECK (
 tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
 AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
 AND hosted_cash_locations.created_by=NULLIF(current_setting('pos.membership_id',true),'')::uuid
 AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
   WHERE m.id=hosted_cash_locations.created_by AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
   AND m.tenant_id=hosted_cash_locations.tenant_id AND m.status='active' AND g.role_key IN ('owner','admin','manager')
   AND (g.branch_id IS NULL OR g.branch_id=hosted_cash_locations.branch_id)
   AND (hosted_cash_locations.location_type<>'register' OR EXISTS (SELECT 1 FROM stations s WHERE s.id=hosted_cash_locations.station_id AND s.branch_id=hosted_cash_locations.branch_id AND s.tenant_id=hosted_cash_locations.tenant_id AND s.status='active')))
);
CREATE POLICY hosted_cash_transfers_branch_read ON hosted_cash_transfers FOR SELECT USING (
 tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
 AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
   WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
   AND m.tenant_id=hosted_cash_transfers.tenant_id AND m.status='active' AND g.role_key IN ('owner','admin','manager')
   AND (g.branch_id IS NULL OR g.branch_id=hosted_cash_transfers.branch_id))
);
CREATE POLICY hosted_cash_transfers_branch_insert ON hosted_cash_transfers FOR INSERT WITH CHECK (
 tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
 AND hosted_cash_transfers.actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
 AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
   WHERE m.id=hosted_cash_transfers.actor_membership_id AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
   AND m.tenant_id=hosted_cash_transfers.tenant_id AND m.status='active' AND g.role_key IN ('owner','admin','manager')
   AND (g.branch_id IS NULL OR g.branch_id=hosted_cash_transfers.branch_id))
);
CREATE POLICY hosted_cash_events_branch_read ON hosted_cash_location_events FOR SELECT USING (
 tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
 AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
   WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
   AND m.tenant_id=hosted_cash_location_events.tenant_id AND m.status='active' AND g.role_key IN ('owner','admin','manager')
   AND (g.branch_id IS NULL OR g.branch_id=hosted_cash_location_events.branch_id))
);
CREATE POLICY hosted_cash_events_branch_insert ON hosted_cash_location_events FOR INSERT WITH CHECK (
 tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
 AND hosted_cash_location_events.actor_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
 AND EXISTS (SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
   WHERE m.id=hosted_cash_location_events.actor_membership_id AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
   AND m.tenant_id=hosted_cash_location_events.tenant_id AND m.status='active' AND g.role_key IN ('owner','admin','manager')
   AND (g.branch_id IS NULL OR g.branch_id=hosted_cash_location_events.branch_id))
);

GRANT SELECT (id,tenant_id,branch_id,name,location_type,station_id,status,created_by,idempotency_key,request_hash,created_at) ON hosted_cash_locations TO pos_api_runtime;
GRANT INSERT (id,tenant_id,branch_id,name,location_type,station_id,created_by,idempotency_key,request_hash) ON hosted_cash_locations TO pos_api_runtime;
GRANT SELECT (id,tenant_id,branch_id,actor_membership_id,from_location_id,to_location_id,amount_minor,note,idempotency_key,request_hash,created_at) ON hosted_cash_transfers TO pos_api_runtime;
GRANT INSERT (id,tenant_id,branch_id,actor_membership_id,from_location_id,to_location_id,amount_minor,note,idempotency_key,request_hash) ON hosted_cash_transfers TO pos_api_runtime;
GRANT SELECT (id,tenant_id,branch_id,location_id,event_type,source_transfer_id,signed_amount_minor,actor_membership_id,idempotency_key,request_hash,created_at) ON hosted_cash_location_events TO pos_api_runtime;
GRANT INSERT (tenant_id,branch_id,location_id,event_type,source_transfer_id,signed_amount_minor,actor_membership_id,idempotency_key,request_hash) ON hosted_cash_location_events TO pos_api_runtime;

