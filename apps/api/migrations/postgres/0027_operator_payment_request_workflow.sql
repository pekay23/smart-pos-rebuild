-- Independent operator identity and narrowly scoped payment-request workflow.
-- The operator service has a separate login and its own transaction-local identity.
GRANT USAGE ON SCHEMA public TO pos_operator_runtime;

ALTER TABLE hosted_payment_integration_requests
    ADD COLUMN customer_visible_update TEXT
        CHECK (customer_visible_update IS NULL OR length(customer_visible_update) <= 1000),
    ADD COLUMN revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- Tenant request policies must not be evaluated for the operator credential.
-- Role-scoped policies avoid granting the operator runtime access to tenant
-- membership tables merely so PostgreSQL can evaluate another role's policy.
DROP POLICY hosted_payment_integration_requests_branch_select ON hosted_payment_integration_requests;
DROP POLICY hosted_payment_integration_requests_branch_insert ON hosted_payment_integration_requests;
CREATE POLICY hosted_payment_integration_requests_branch_select
ON hosted_payment_integration_requests FOR SELECT TO pos_api_runtime USING (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
          AND m.tenant_id=hosted_payment_integration_requests.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id=hosted_payment_integration_requests.branch_id)
          AND g.station_id IS NULL
    )
);
CREATE POLICY hosted_payment_integration_requests_branch_insert
ON hosted_payment_integration_requests FOR INSERT TO pos_api_runtime WITH CHECK (
    tenant_id=NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid
    AND created_by_membership_id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND status='submitted'
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=hosted_payment_integration_requests.created_by_membership_id
          AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
          AND m.tenant_id=hosted_payment_integration_requests.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id=hosted_payment_integration_requests.branch_id)
          AND g.station_id IS NULL
    )
);

CREATE TABLE operator_accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    issuer TEXT NOT NULL CHECK (length(issuer) BETWEEN 1 AND 512),
    subject TEXT NOT NULL CHECK (length(subject) BETWEEN 1 AND 512),
    role TEXT NOT NULL CHECK (role IN ('support','billing','release','security')),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','suspended','deactivated')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deactivated_at TIMESTAMPTZ,
    UNIQUE (issuer, subject),
    CHECK ((status = 'deactivated') = (deactivated_at IS NOT NULL))
);
ALTER TABLE operator_accounts ENABLE ROW LEVEL SECURITY;
ALTER TABLE operator_accounts FORCE ROW LEVEL SECURITY;
CREATE POLICY operator_accounts_verified_identity_select
ON operator_accounts FOR SELECT TO pos_operator_runtime USING (
    issuer = NULLIF(current_setting('pos.operator_issuer', true), '')
    AND subject = NULLIF(current_setting('pos.operator_subject', true), '')
    AND status = 'active'
);

CREATE TABLE operator_payment_request_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id UUID NOT NULL REFERENCES hosted_payment_integration_requests(id) ON DELETE RESTRICT,
    operator_id UUID NOT NULL REFERENCES operator_accounts(id) ON DELETE RESTRICT,
    event_kind TEXT NOT NULL CHECK (event_kind IN ('status_changed','public_update','private_note')),
    previous_status TEXT,
    new_status TEXT,
    customer_visible_update TEXT CHECK (customer_visible_update IS NULL OR length(customer_visible_update) <= 1000),
    private_note TEXT CHECK (private_note IS NULL OR length(private_note) <= 2000),
    resulting_revision INTEGER NOT NULL CHECK (resulting_revision > 0),
    idempotency_key TEXT CHECK (idempotency_key IS NULL OR length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA CHECK (request_hash IS NULL OR octet_length(request_hash) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((idempotency_key IS NULL) = (request_hash IS NULL)),
    UNIQUE (operator_id, request_id, idempotency_key)
);
CREATE INDEX operator_payment_request_events_request
    ON operator_payment_request_events(request_id, created_at, id);
ALTER TABLE operator_payment_request_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE operator_payment_request_events FORCE ROW LEVEL SECURITY;

CREATE TABLE operator_audit_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    operator_id UUID NOT NULL REFERENCES operator_accounts(id) ON DELETE RESTRICT,
    action TEXT NOT NULL CHECK (length(action) BETWEEN 1 AND 100),
    entity_type TEXT NOT NULL CHECK (length(entity_type) BETWEEN 1 AND 100),
    entity_id UUID,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
ALTER TABLE operator_audit_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE operator_audit_events FORCE ROW LEVEL SECURITY;

CREATE POLICY hosted_payment_requests_operator_select
ON hosted_payment_integration_requests FOR SELECT TO pos_operator_runtime USING (
    EXISTS (
        SELECT 1 FROM operator_accounts oa
        WHERE oa.id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
          AND oa.status = 'active' AND oa.role = 'support'
    )
);
CREATE POLICY hosted_payment_requests_operator_update
ON hosted_payment_integration_requests FOR UPDATE TO pos_operator_runtime
USING (
    EXISTS (
        SELECT 1 FROM operator_accounts oa
        WHERE oa.id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
          AND oa.status = 'active' AND oa.role = 'support'
    )
)
WITH CHECK (
    EXISTS (
        SELECT 1 FROM operator_accounts oa
        WHERE oa.id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
          AND oa.status = 'active' AND oa.role = 'support'
    )
);

CREATE POLICY operator_request_events_support_access
ON operator_payment_request_events FOR ALL TO pos_operator_runtime
USING (
    EXISTS (
        SELECT 1 FROM operator_accounts oa
        WHERE oa.id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
          AND oa.status = 'active' AND oa.role = 'support'
    )
)
WITH CHECK (
    EXISTS (
        SELECT 1 FROM operator_accounts oa
        WHERE oa.id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
          AND oa.status = 'active' AND oa.role = 'support'
    )
);
CREATE POLICY operator_audit_events_support_insert
ON operator_audit_events FOR INSERT TO pos_operator_runtime WITH CHECK (
    operator_id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
    AND EXISTS (
        SELECT 1 FROM operator_accounts oa
        WHERE oa.id = operator_audit_events.operator_id
          AND oa.status = 'active' AND oa.role = 'support'
    )
);

-- Tenant runtime may read only the public progress projection. It cannot change
-- status/progress, inspect operator accounts, read private notes or access audit.
GRANT SELECT (customer_visible_update, revision, updated_at)
    ON hosted_payment_integration_requests TO pos_api_runtime;
-- The operator login is created separately by runtime provisioning. Its grants
-- are limited to the support queue and append-only event/audit rows.
GRANT SELECT (id, issuer, subject, role, status)
    ON operator_accounts TO pos_operator_runtime;
GRANT SELECT (id, tenant_id, branch_id, country_code, provider_name, terminal_model,
              capability, business_impact, details, status, customer_visible_update,
              revision, created_at, updated_at)
    ON hosted_payment_integration_requests TO pos_operator_runtime;
GRANT UPDATE (status, customer_visible_update, revision, updated_at)
    ON hosted_payment_integration_requests TO pos_operator_runtime;
GRANT SELECT (id, request_id, operator_id, event_kind, previous_status, new_status,
              customer_visible_update, private_note, resulting_revision,
              idempotency_key, request_hash, created_at)
    ON operator_payment_request_events TO pos_operator_runtime;
GRANT INSERT (id, request_id, operator_id, event_kind, previous_status, new_status,
              customer_visible_update, private_note, resulting_revision,
              idempotency_key, request_hash)
    ON operator_payment_request_events TO pos_operator_runtime;
GRANT INSERT (id, operator_id, action, entity_type, entity_id, metadata)
    ON operator_audit_events TO pos_operator_runtime;

REVOKE UPDATE, DELETE, TRUNCATE ON operator_payment_request_events FROM pos_operator_runtime;
REVOKE SELECT, UPDATE, DELETE, TRUNCATE ON operator_audit_events FROM pos_operator_runtime;
REVOKE INSERT, UPDATE, DELETE, TRUNCATE ON operator_accounts FROM pos_operator_runtime;
