CREATE TABLE IF NOT EXISTS discount_approvals (
    id TEXT PRIMARY KEY,
    business_id TEXT NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
    ticket_id TEXT NOT NULL,
    requester_user_id TEXT NOT NULL REFERENCES users(id),
    approver_user_id TEXT NOT NULL REFERENCES users(id),
    discount_minor INTEGER NOT NULL CHECK (discount_minor > 0),
    reason TEXT NOT NULL,
    quote_json TEXT NOT NULL,
    approved_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    invalidated_at TEXT,
    invalidation_reason TEXT,
    consumed_sale_id TEXT UNIQUE REFERENCES sales(id)
);
CREATE INDEX IF NOT EXISTS idx_discount_approvals_ticket
    ON discount_approvals(store_id,ticket_id,approved_at DESC);

CREATE TABLE IF NOT EXISTS discount_approval_attempts (
    store_id TEXT NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
    requester_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    approver_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    failed_attempts INTEGER NOT NULL DEFAULT 0 CHECK (failed_attempts >= 0),
    locked_until TEXT,
    PRIMARY KEY(store_id,requester_user_id,approver_user_id)
);

UPDATE roles
SET permissions_json=json_insert(permissions_json,'$[#]','sales:discount:approve')
WHERE is_system=1 AND role_key IN ('owner','admin','manager')
  AND NOT EXISTS (
      SELECT 1 FROM json_each(roles.permissions_json)
      WHERE value='sales:discount:approve'
  );
