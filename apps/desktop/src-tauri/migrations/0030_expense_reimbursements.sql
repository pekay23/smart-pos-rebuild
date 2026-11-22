PRAGMA foreign_keys = OFF;
BEGIN IMMEDIATE;

CREATE TABLE expense_reimbursements (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE RESTRICT,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE RESTRICT,
    expense_id TEXT NOT NULL,
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    return_method TEXT NOT NULL CHECK (return_method IN ('cash','card','mobile_money','bank_transfer','other')),
    return_reference TEXT,
    reason TEXT NOT NULL CHECK (length(trim(reason)) BETWEEN 5 AND 500),
    cash_location_id TEXT,
    cash_shift_id TEXT REFERENCES cash_shifts(id) ON DELETE RESTRICT,
    idempotency_key TEXT NOT NULL CHECK (length(trim(idempotency_key)) BETWEEN 1 AND 100),
    recorded_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    returned_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (expense_id,store_id) REFERENCES expenses(id,store_id) ON DELETE RESTRICT,
    FOREIGN KEY (cash_location_id,business_id,store_id) REFERENCES cash_locations(id,business_id,store_id) ON DELETE RESTRICT,
    UNIQUE (id,business_id,store_id),
    UNIQUE (store_id,idempotency_key),
    CHECK (
        (return_method='cash' AND cash_location_id IS NOT NULL
            AND (return_reference IS NULL OR length(trim(return_reference))<=120))
        OR
        (return_method<>'cash' AND cash_location_id IS NULL AND cash_shift_id IS NULL
            AND return_reference IS NOT NULL AND length(trim(return_reference)) BETWEEN 1 AND 120)
    )
);
CREATE UNIQUE INDEX idx_expense_reimbursements_reference
    ON expense_reimbursements(store_id,return_method,return_reference)
    WHERE return_reference IS NOT NULL;
CREATE INDEX idx_expense_reimbursements_store_returned
    ON expense_reimbursements(store_id,returned_at DESC,created_at DESC,id DESC);
CREATE INDEX idx_expense_reimbursements_expense
    ON expense_reimbursements(expense_id,returned_at,id);

INSERT INTO expense_reimbursements
    (id,business_id,store_id,expense_id,amount_minor,return_method,return_reference,reason,
     cash_location_id,cash_shift_id,idempotency_key,recorded_by,returned_at,created_at)
SELECT id,business_id,store_id,expense_id,amount_minor,payment_method,return_reference,reason,
       NULL,NULL,idempotency_key,recorded_by,returned_at,created_at
FROM expense_reversals;

CREATE TRIGGER expense_reimbursements_no_update
BEFORE UPDATE ON expense_reimbursements BEGIN
    SELECT RAISE(ABORT,'expense reimbursements are immutable');
END;
CREATE TRIGGER expense_reimbursements_no_delete
BEFORE DELETE ON expense_reimbursements BEGIN
    SELECT RAISE(ABORT,'expense reimbursements are immutable');
END;

DROP TRIGGER cash_location_events_validate_insert;
DROP TRIGGER cash_location_events_no_update;
DROP TRIGGER cash_location_events_no_delete;
DROP TRIGGER cash_location_event_reviews_validate_insert;
CREATE TABLE cash_location_events_new (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL,
    store_id TEXT NOT NULL,
    cash_location_id TEXT NOT NULL,
    shift_id TEXT REFERENCES cash_shifts(id) ON DELETE RESTRICT,
    event_type TEXT NOT NULL CHECK (event_type IN ('opening','sale','refund','expense','supplier_payment','expense_reimbursement','transfer','manual_outflow','close_variance','shift_open_variance')),
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
INSERT INTO cash_location_events_new
SELECT id,business_id,store_id,cash_location_id,shift_id,event_type,direction,amount_minor,source_type,source_id,
       transfer_id,idempotency_key,note,review_status,actor_user_id,effective_at,created_at
FROM cash_location_events;
DROP TABLE cash_location_events;
ALTER TABLE cash_location_events_new RENAME TO cash_location_events;
CREATE UNIQUE INDEX idx_cash_location_events_one_opening
    ON cash_location_events(cash_location_id) WHERE event_type='opening';
CREATE UNIQUE INDEX idx_cash_location_events_source
    ON cash_location_events(store_id,cash_location_id,source_type,source_id,event_type);
CREATE UNIQUE INDEX idx_cash_location_events_idempotency
    ON cash_location_events(store_id,idempotency_key) WHERE idempotency_key IS NOT NULL;
CREATE INDEX idx_cash_location_events_balance
    ON cash_location_events(cash_location_id,effective_at,created_at,id);
CREATE INDEX idx_cash_location_events_shift
    ON cash_location_events(shift_id,event_type,created_at);

CREATE TRIGGER cash_location_events_validate_insert
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
    SELECT CASE WHEN NEW.event_type='expense_reimbursement' AND NOT EXISTS (
        SELECT 1 FROM expense_reimbursements r WHERE r.id=NEW.source_id AND r.store_id=NEW.store_id
          AND r.business_id=NEW.business_id AND r.return_method='cash'
          AND r.cash_location_id=NEW.cash_location_id AND r.cash_shift_id IS NEW.shift_id
          AND r.amount_minor=NEW.amount_minor AND NEW.direction='in' AND NEW.source_type='expense_reimbursement'
    ) THEN RAISE(ABORT,'cash reimbursement event does not match its source') END;
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
CREATE TRIGGER cash_location_events_no_update
BEFORE UPDATE ON cash_location_events BEGIN
    SELECT RAISE(ABORT,'cash location events are immutable');
END;
CREATE TRIGGER cash_location_events_no_delete
BEFORE DELETE ON cash_location_events BEGIN
    SELECT RAISE(ABORT,'cash location events are immutable');
END;
CREATE TRIGGER cash_location_event_reviews_validate_insert
BEFORE INSERT ON cash_location_event_reviews
WHEN NOT EXISTS (
    SELECT 1 FROM cash_location_events e WHERE e.id=NEW.event_id AND e.business_id=NEW.business_id
      AND e.store_id=NEW.store_id AND e.event_type IN ('shift_open_variance','close_variance')
      AND e.review_status='pending'
)
BEGIN
    SELECT RAISE(ABORT,'only a pending same-store cash count variance can be reviewed');
END;

CREATE TRIGGER expense_reimbursements_validate_insert
BEFORE INSERT ON expense_reimbursements
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM expenses e JOIN stores s ON s.id=e.store_id
        WHERE e.id=NEW.expense_id AND e.store_id=NEW.store_id
          AND s.business_id=NEW.business_id
          AND NEW.amount_minor <= e.amount_minor - COALESCE((
              SELECT SUM(r.amount_minor) FROM expense_reimbursements r
              WHERE r.expense_id=e.id AND r.store_id=e.store_id
          ),0)
    ) THEN RAISE(ABORT,'reimbursement exceeds the remaining expense amount or has an invalid source') END;
    SELECT CASE WHEN NEW.return_method='cash' AND NOT EXISTS (
        SELECT 1 FROM cash_locations l JOIN businesses b ON b.id=l.business_id
        JOIN cash_location_events opening ON opening.cash_location_id=l.id AND opening.event_type='opening'
        WHERE l.id=NEW.cash_location_id AND l.business_id=NEW.business_id
          AND l.store_id=NEW.store_id AND l.is_active=1 AND l.currency_code=b.currency_code
          AND ((l.location_type='register' AND NEW.cash_shift_id IS NOT NULL AND EXISTS (
              SELECT 1 FROM cash_shifts sh WHERE sh.id=NEW.cash_shift_id AND sh.store_id=l.store_id
                AND sh.cash_location_id=l.id AND sh.status='open'
          )) OR (l.location_type<>'register' AND NEW.cash_shift_id IS NULL))
    ) THEN RAISE(ABORT,'cash reimbursement requires an initialized active same-store destination and valid open shift') END;
END;

DROP TRIGGER cash_movements_source_validate_insert;
CREATE TRIGGER cash_movements_source_validate_insert
BEFORE INSERT ON cash_movements
WHEN NEW.source_type IS NOT NULL
BEGIN
    SELECT CASE WHEN (
        (NEW.source_type='expense_reimbursement' AND NEW.movement_type<>'paid_in')
        OR (NEW.source_type<>'expense_reimbursement' AND NEW.movement_type<>'paid_out')
    ) THEN RAISE(ABORT,'source-linked cash movement direction is invalid') END;
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM cash_shifts s WHERE s.id=NEW.shift_id AND s.store_id=NEW.store_id
    ) THEN RAISE(ABORT,'cash movement shift must belong to its store') END;
    SELECT CASE WHEN NEW.source_type='expense' AND NOT EXISTS (
        SELECT 1 FROM expenses e WHERE e.id=NEW.source_id AND e.store_id=NEW.store_id
          AND e.payment_method='cash' AND e.cash_origin='drawer' AND e.amount_minor=NEW.amount_minor
          AND e.user_id=NEW.user_id AND e.created_at=NEW.created_at
    ) THEN RAISE(ABORT,'cash movement does not match its drawer-funded expense') END;
    SELECT CASE WHEN NEW.source_type='supplier_payment' AND NOT EXISTS (
        SELECT 1 FROM supplier_payments p WHERE p.id=NEW.source_id AND p.store_id=NEW.store_id
          AND p.payment_method='cash' AND p.cash_origin='drawer' AND p.amount_minor=NEW.amount_minor
          AND p.recorded_by=NEW.user_id AND p.paid_at=NEW.created_at
    ) THEN RAISE(ABORT,'cash movement does not match its drawer-funded supplier payment') END;
    SELECT CASE WHEN NEW.source_type='expense_reimbursement' AND NOT EXISTS (
        SELECT 1 FROM expense_reimbursements r JOIN cash_shifts s ON s.id=r.cash_shift_id
        WHERE r.id=NEW.source_id AND r.store_id=NEW.store_id AND r.return_method='cash'
          AND r.amount_minor=NEW.amount_minor AND r.cash_shift_id=NEW.shift_id
          AND r.recorded_by=NEW.user_id AND r.created_at=NEW.created_at
          AND s.store_id=NEW.store_id AND s.status='open'
    ) THEN RAISE(ABORT,'cash paid-in movement does not match its reimbursement source') END;
    SELECT CASE WHEN NEW.source_type NOT IN ('expense','supplier_payment','expense_reimbursement')
        THEN RAISE(ABORT,'unsupported source-linked cash movement') END;
END;

INSERT OR IGNORE INTO local_schema_migrations(version) VALUES (30);
COMMIT;
PRAGMA foreign_keys = ON;
