PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TABLE IF NOT EXISTS cash_locations (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL,
    store_id TEXT NOT NULL,
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 80),
    location_type TEXT NOT NULL CHECK (location_type IN ('register','safe','petty_cash','other')),
    currency_code TEXT NOT NULL,
    station_key TEXT,
    is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0,1)),
    created_by TEXT REFERENCES users(id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL,
    CHECK ((location_type='register' AND station_key IS NOT NULL) OR (location_type<>'register' AND station_key IS NULL)),
    FOREIGN KEY (store_id,business_id) REFERENCES stores(id,business_id) ON DELETE RESTRICT,
    FOREIGN KEY (store_id,station_key) REFERENCES register_stations(store_id,station_key) ON DELETE RESTRICT,
    UNIQUE (id,business_id,store_id),
    UNIQUE (store_id,name COLLATE NOCASE)
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_cash_locations_register_station
    ON cash_locations(store_id,station_key) WHERE location_type='register';
CREATE INDEX IF NOT EXISTS idx_cash_locations_store_active
    ON cash_locations(store_id,is_active,location_type,name);

INSERT OR IGNORE INTO cash_locations(id,business_id,store_id,name,location_type,currency_code,station_key,is_active,created_by,created_at)
SELECT lower(hex(randomblob(16))),rs.business_id,rs.store_id,rs.display_name,'register',b.currency_code,rs.station_key,rs.is_active,NULL,rs.created_at
FROM register_stations rs JOIN businesses b ON b.id=rs.business_id;

ALTER TABLE cash_shifts ADD COLUMN cash_location_id TEXT REFERENCES cash_locations(id) ON DELETE RESTRICT;
ALTER TABLE cash_movements ADD COLUMN transfer_id TEXT REFERENCES cash_location_transfers(id) ON DELETE RESTRICT;
ALTER TABLE cash_close_reports ADD COLUMN closing_transfer_id TEXT REFERENCES cash_location_transfers(id) ON DELETE RESTRICT;
ALTER TABLE cash_close_reports ADD COLUMN closing_transfer_minor INTEGER NOT NULL DEFAULT 0 CHECK (closing_transfer_minor >= 0);
ALTER TABLE cash_close_reports ADD COLUMN retained_cash_minor INTEGER NOT NULL DEFAULT 0 CHECK (retained_cash_minor >= 0);
ALTER TABLE refunds ADD COLUMN cash_shift_id TEXT REFERENCES cash_shifts(id) ON DELETE RESTRICT;
ALTER TABLE expenses ADD COLUMN cash_location_id TEXT REFERENCES cash_locations(id) ON DELETE RESTRICT;
ALTER TABLE supplier_payments ADD COLUMN cash_location_id TEXT REFERENCES cash_locations(id) ON DELETE RESTRICT;

CREATE TABLE IF NOT EXISTS cash_location_transfers (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL,
    store_id TEXT NOT NULL,
    from_location_id TEXT NOT NULL,
    to_location_id TEXT NOT NULL,
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0 AND amount_minor <= 9000000000000),
    idempotency_key TEXT NOT NULL CHECK (length(trim(idempotency_key)) BETWEEN 1 AND 100),
    note TEXT NOT NULL CHECK (length(trim(note)) BETWEEN 1 AND 200),
    source_type TEXT NOT NULL CHECK (source_type IN ('manual','drawer_movement','z_close')),
    source_id TEXT NOT NULL,
    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (store_id,business_id) REFERENCES stores(id,business_id) ON DELETE RESTRICT,
    FOREIGN KEY (from_location_id,business_id,store_id) REFERENCES cash_locations(id,business_id,store_id) ON DELETE RESTRICT,
    FOREIGN KEY (to_location_id,business_id,store_id) REFERENCES cash_locations(id,business_id,store_id) ON DELETE RESTRICT,
    CHECK (from_location_id<>to_location_id),
    UNIQUE (store_id,idempotency_key),
    UNIQUE (source_type,source_id)
);
CREATE INDEX IF NOT EXISTS idx_cash_location_transfers_store_created
    ON cash_location_transfers(store_id,created_at DESC,id DESC);

CREATE TABLE IF NOT EXISTS cash_location_events (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL,
    store_id TEXT NOT NULL,
    cash_location_id TEXT NOT NULL,
    shift_id TEXT REFERENCES cash_shifts(id) ON DELETE RESTRICT,
    event_type TEXT NOT NULL CHECK (event_type IN ('opening','sale','refund','expense','supplier_payment','transfer','manual_outflow','close_variance','shift_open_variance')),
    direction TEXT NOT NULL CHECK (direction IN ('in','out')),
    amount_minor INTEGER NOT NULL CHECK (amount_minor >= 0 AND amount_minor <= 9000000000000),
    source_type TEXT NOT NULL,
    source_id TEXT NOT NULL,
    transfer_id TEXT REFERENCES cash_location_transfers(id) ON DELETE RESTRICT,
    idempotency_key TEXT,
    note TEXT NOT NULL DEFAULT '',
    review_status TEXT NOT NULL DEFAULT 'cleared' CHECK (review_status IN ('cleared','pending','reviewed')),
    actor_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    effective_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (store_id,business_id) REFERENCES stores(id,business_id) ON DELETE RESTRICT,
    FOREIGN KEY (cash_location_id,business_id,store_id) REFERENCES cash_locations(id,business_id,store_id) ON DELETE RESTRICT,
    CHECK ((event_type='opening' AND direction='in') OR (event_type<>'opening' AND amount_minor>0)),
    UNIQUE (id,business_id,store_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_cash_location_events_one_opening
    ON cash_location_events(cash_location_id) WHERE event_type='opening';
CREATE UNIQUE INDEX IF NOT EXISTS idx_cash_location_events_source
    ON cash_location_events(store_id,cash_location_id,source_type,source_id,event_type);
CREATE UNIQUE INDEX IF NOT EXISTS idx_cash_location_events_idempotency
    ON cash_location_events(store_id,idempotency_key) WHERE idempotency_key IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_cash_location_events_balance
    ON cash_location_events(cash_location_id,effective_at,created_at,id);
CREATE INDEX IF NOT EXISTS idx_cash_location_events_shift
    ON cash_location_events(shift_id,event_type,created_at);

CREATE TABLE IF NOT EXISTS cash_location_event_reviews (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL,
    store_id TEXT NOT NULL,
    event_id TEXT NOT NULL UNIQUE,
    reviewed_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    note TEXT NOT NULL CHECK (length(trim(note)) BETWEEN 3 AND 200),
    reviewed_at TEXT NOT NULL,
    FOREIGN KEY (store_id,business_id) REFERENCES stores(id,business_id) ON DELETE RESTRICT,
    FOREIGN KEY (event_id,business_id,store_id) REFERENCES cash_location_events(id,business_id,store_id) ON DELETE RESTRICT
);
CREATE TRIGGER IF NOT EXISTS cash_location_event_reviews_validate_insert
BEFORE INSERT ON cash_location_event_reviews
WHEN NOT EXISTS (
    SELECT 1 FROM cash_location_events e WHERE e.id=NEW.event_id AND e.business_id=NEW.business_id
      AND e.store_id=NEW.store_id AND e.event_type IN ('shift_open_variance','close_variance')
      AND e.review_status='pending'
)
BEGIN
    SELECT RAISE(ABORT,'only a pending same-store cash count variance can be reviewed');
END;
CREATE TRIGGER IF NOT EXISTS cash_location_event_reviews_no_update
BEFORE UPDATE ON cash_location_event_reviews BEGIN
    SELECT RAISE(ABORT,'cash variance reviews are immutable');
END;
CREATE TRIGGER IF NOT EXISTS cash_location_event_reviews_no_delete
BEFORE DELETE ON cash_location_event_reviews BEGIN
    SELECT RAISE(ABORT,'cash variance reviews are immutable');
END;

CREATE TRIGGER IF NOT EXISTS cash_locations_core_immutable
BEFORE UPDATE ON cash_locations
WHEN NEW.business_id<>OLD.business_id OR NEW.store_id<>OLD.store_id OR NEW.location_type<>OLD.location_type
  OR NEW.currency_code<>OLD.currency_code OR COALESCE(NEW.station_key,'')<>COALESCE(OLD.station_key,'')
BEGIN
    SELECT RAISE(ABORT,'cash location identity is immutable');
END;
CREATE TRIGGER IF NOT EXISTS cash_locations_no_delete
BEFORE DELETE ON cash_locations BEGIN
    SELECT RAISE(ABORT,'cash locations are retained for audit history');
END;

CREATE TRIGGER IF NOT EXISTS cash_location_transfers_validate_insert
BEFORE INSERT ON cash_location_transfers
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM cash_locations a JOIN cash_locations b
          ON b.id=NEW.to_location_id AND b.store_id=a.store_id AND b.business_id=a.business_id
         WHERE a.id=NEW.from_location_id AND a.store_id=NEW.store_id AND a.business_id=NEW.business_id
           AND a.currency_code=b.currency_code AND a.is_active=1 AND b.is_active=1
    ) THEN RAISE(ABORT,'cash transfer locations must be active and share a store and currency') END;
END;
CREATE TRIGGER IF NOT EXISTS cash_location_transfers_no_update
BEFORE UPDATE ON cash_location_transfers BEGIN
    SELECT RAISE(ABORT,'cash transfers are immutable');
END;
CREATE TRIGGER IF NOT EXISTS cash_location_transfers_no_delete
BEFORE DELETE ON cash_location_transfers BEGIN
    SELECT RAISE(ABORT,'cash transfers are immutable');
END;

CREATE TRIGGER IF NOT EXISTS cash_location_events_validate_insert
BEFORE INSERT ON cash_location_events
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM cash_locations l JOIN stores s ON s.id=l.store_id AND s.business_id=l.business_id
        JOIN businesses b ON b.id=l.business_id
        WHERE l.id=NEW.cash_location_id AND l.store_id=NEW.store_id AND l.business_id=NEW.business_id
          AND l.currency_code=b.currency_code AND l.is_active=1
    ) THEN RAISE(ABORT,'cash event must use an active same-store location and currency') END;
    SELECT CASE WHEN NEW.event_type='opening' AND EXISTS (
        SELECT 1 FROM cash_location_events e WHERE e.cash_location_id=NEW.cash_location_id
    ) THEN RAISE(ABORT,'cash opening count must precede all location events') END;
    SELECT CASE WHEN NEW.event_type<>'opening' AND NOT EXISTS (
        SELECT 1 FROM cash_location_events e WHERE e.cash_location_id=NEW.cash_location_id AND e.event_type='opening'
    ) THEN RAISE(ABORT,'cash location needs an opening count before activity') END;
    SELECT CASE WHEN NEW.direction='out' AND (
        SELECT COALESCE(SUM(CASE WHEN direction='in' THEN amount_minor ELSE -amount_minor END),0)
        FROM cash_location_events WHERE cash_location_id=NEW.cash_location_id
    ) < NEW.amount_minor THEN RAISE(ABORT,'cash location does not have enough available funds') END;
    SELECT CASE WHEN NEW.event_type='transfer' AND NOT EXISTS (
        SELECT 1 FROM cash_location_transfers t WHERE t.id=NEW.transfer_id AND t.store_id=NEW.store_id
          AND t.business_id=NEW.business_id AND t.amount_minor=NEW.amount_minor
          AND NEW.source_type='transfer' AND NEW.source_id=t.id
          AND ((NEW.direction='out' AND t.from_location_id=NEW.cash_location_id)
            OR (NEW.direction='in' AND t.to_location_id=NEW.cash_location_id))
    ) THEN RAISE(ABORT,'cash transfer event does not match its transfer record') END;
    SELECT CASE WHEN NEW.event_type='opening' AND (NEW.source_type<>'opening' OR NEW.source_id<>NEW.cash_location_id)
        THEN RAISE(ABORT,'cash opening count source identity is invalid') END;
    SELECT CASE WHEN NEW.event_type='sale' AND NOT EXISTS (
        SELECT 1 FROM sales s JOIN payments p ON p.sale_id=s.id
        JOIN cash_shifts cs ON cs.id=s.cash_shift_id AND cs.store_id=s.store_id
        WHERE s.id=NEW.source_id AND s.store_id=NEW.store_id AND cs.cash_location_id=NEW.cash_location_id
          AND p.method='cash' AND p.amount_minor=NEW.amount_minor
    ) THEN RAISE(ABORT,'cash sale event does not match its register payment') END;
    SELECT CASE WHEN NEW.event_type='refund' AND NOT EXISTS (
        SELECT 1 FROM refunds r JOIN refund_tenders t ON t.refund_id=r.id
        JOIN cash_shifts cs ON cs.id=r.cash_shift_id AND cs.store_id=r.store_id
        WHERE r.id=NEW.source_id AND r.store_id=NEW.store_id AND cs.cash_location_id=NEW.cash_location_id
          AND t.method='cash' AND t.amount_minor=NEW.amount_minor
    ) THEN RAISE(ABORT,'cash refund event does not match its register refund') END;
    SELECT CASE WHEN NEW.event_type='expense' AND NOT EXISTS (
        SELECT 1 FROM expenses e WHERE e.id=NEW.source_id AND e.store_id=NEW.store_id
          AND e.cash_location_id=NEW.cash_location_id AND e.payment_method='cash'
          AND e.cash_origin IN ('drawer','other_cash') AND e.amount_minor=NEW.amount_minor
    ) THEN RAISE(ABORT,'cash expense event does not match its source') END;
    SELECT CASE WHEN NEW.event_type='supplier_payment' AND NOT EXISTS (
        SELECT 1 FROM supplier_payments p WHERE p.id=NEW.source_id AND p.store_id=NEW.store_id
          AND p.cash_location_id=NEW.cash_location_id AND p.payment_method='cash'
          AND p.cash_origin IN ('drawer','other_cash') AND p.amount_minor=NEW.amount_minor
    ) THEN RAISE(ABORT,'supplier cash event does not match its source') END;
    SELECT CASE WHEN NEW.event_type='manual_outflow' AND NOT EXISTS (
        SELECT 1 FROM cash_movements m JOIN cash_shifts cs ON cs.id=m.shift_id
        WHERE m.id=NEW.source_id AND m.store_id=NEW.store_id AND cs.cash_location_id=NEW.cash_location_id
          AND m.shift_id=NEW.shift_id AND m.movement_type='paid_out' AND m.amount_minor=NEW.amount_minor
          AND m.user_id=NEW.actor_user_id
    ) THEN RAISE(ABORT,'manual cash outflow does not match its drawer movement') END;
    SELECT CASE WHEN NEW.event_type='close_variance' AND NOT EXISTS (
        SELECT 1 FROM cash_close_reports z JOIN cash_shifts cs ON cs.id=z.shift_id
        WHERE z.shift_id=NEW.source_id AND z.store_id=NEW.store_id AND cs.cash_location_id=NEW.cash_location_id
          AND abs(z.difference_minor)=NEW.amount_minor
    ) THEN RAISE(ABORT,'cash close variance event does not match its Z report') END;
    SELECT CASE WHEN NEW.event_type='shift_open_variance' AND NOT EXISTS (
        SELECT 1 FROM cash_shifts cs WHERE cs.id=NEW.source_id AND cs.store_id=NEW.store_id
          AND cs.cash_location_id=NEW.cash_location_id
    ) THEN RAISE(ABORT,'cash opening count event does not match its shift') END;
END;
CREATE TRIGGER IF NOT EXISTS cash_location_events_no_update
BEFORE UPDATE ON cash_location_events BEGIN
    SELECT RAISE(ABORT,'cash location events are immutable');
END;
CREATE TRIGGER IF NOT EXISTS cash_location_events_no_delete
BEFORE DELETE ON cash_location_events BEGIN
    SELECT RAISE(ABORT,'cash location events are immutable');
END;

DROP TRIGGER IF EXISTS expenses_cash_origin_validate_insert;
CREATE TRIGGER expenses_cash_origin_validate_insert
BEFORE INSERT ON expenses
WHEN (NEW.payment_method='cash' AND NEW.cash_origin NOT IN ('drawer','other_cash'))
  OR (NEW.payment_method<>'cash' AND (NEW.cash_origin<>'not_applicable' OR NEW.cash_source_note IS NOT NULL OR NEW.cash_location_id IS NOT NULL))
  OR (NEW.cash_origin IN ('drawer','other_cash') AND NOT EXISTS (
      SELECT 1 FROM cash_locations l WHERE l.id=NEW.cash_location_id AND l.store_id=NEW.store_id
        AND l.is_active=1 AND ((NEW.cash_origin='drawer' AND l.location_type='register') OR (NEW.cash_origin='other_cash' AND l.location_type<>'register'))
  ))
BEGIN
    SELECT RAISE(ABORT,'expense cash origin must use an active same-store cash location');
END;

DROP TRIGGER IF EXISTS supplier_payments_cash_origin_validate_insert;
CREATE TRIGGER supplier_payments_cash_origin_validate_insert
BEFORE INSERT ON supplier_payments
WHEN (NEW.payment_method='cash' AND NEW.cash_origin NOT IN ('drawer','other_cash'))
  OR (NEW.payment_method<>'cash' AND (NEW.cash_origin<>'not_applicable' OR NEW.cash_source_note IS NOT NULL OR NEW.cash_location_id IS NOT NULL))
  OR (NEW.cash_origin IN ('drawer','other_cash') AND NOT EXISTS (
      SELECT 1 FROM cash_locations l WHERE l.id=NEW.cash_location_id AND l.store_id=NEW.store_id
        AND l.is_active=1 AND ((NEW.cash_origin='drawer' AND l.location_type='register') OR (NEW.cash_origin='other_cash' AND l.location_type<>'register'))
  ))
BEGIN
    SELECT RAISE(ABORT,'supplier payment cash origin must use an active same-store cash location');
END;

INSERT OR IGNORE INTO local_schema_migrations(version) VALUES (28);
COMMIT;
