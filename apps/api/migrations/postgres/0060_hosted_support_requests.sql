-- General tenant support requests remain tenant-owned records. Operator issues
-- are internal work items that can link to several source requests.
CREATE TABLE hosted_support_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    created_by_membership_id UUID NOT NULL,
    category TEXT NOT NULL CHECK (category IN ('bug','feature_request','account_access','data_issue','training','other')),
    title TEXT NOT NULL CHECK (length(btrim(title)) BETWEEN 3 AND 120),
    details TEXT CHECK (details IS NULL OR length(details) <= 2000),
    status TEXT NOT NULL DEFAULT 'submitted' CHECK (status IN ('submitted','under_review','planned','in_progress','released','not_planned')),
    customer_visible_update TEXT CHECK (customer_visible_update IS NULL OR length(customer_visible_update) <= 1000),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (branch_id, tenant_id) REFERENCES branches(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (created_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (tenant_id, branch_id, id),
    UNIQUE (tenant_id, created_by_membership_id, idempotency_key)
);
CREATE INDEX hosted_support_requests_branch_date
    ON hosted_support_requests(tenant_id, branch_id, created_at DESC, id);
ALTER TABLE hosted_support_requests ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_support_requests FORCE ROW LEVEL SECURITY;
CREATE POLICY hosted_support_requests_tenant_select
ON hosted_support_requests FOR SELECT TO pos_api_runtime
USING (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
        WHERE m.id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
          AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND m.tenant_id = hosted_support_requests.tenant_id AND m.status = 'active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id = hosted_support_requests.branch_id)
          AND g.station_id IS NULL
    )
);
CREATE POLICY hosted_support_requests_tenant_insert
ON hosted_support_requests FOR INSERT TO pos_api_runtime
WITH CHECK (
    tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
    AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
    AND created_by_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
    AND status = 'submitted' AND revision = 1 AND customer_visible_update IS NULL
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id = m.tenant_id AND g.membership_id = m.id
        WHERE m.id = hosted_support_requests.created_by_membership_id
          AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
          AND m.tenant_id = hosted_support_requests.tenant_id AND m.status = 'active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id = hosted_support_requests.branch_id)
          AND g.station_id IS NULL
    )
);
-- Support operators may read source submissions and write only the public
-- projection. Private events are kept in the operator-only history table below.
CREATE POLICY hosted_support_requests_operator_select
ON hosted_support_requests FOR SELECT TO pos_operator_runtime
USING (
    EXISTS (
        SELECT 1 FROM operator_accounts oa
        WHERE oa.id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
          AND oa.status = 'active' AND oa.role = 'support'
    )
);
CREATE POLICY hosted_support_requests_operator_update
ON hosted_support_requests FOR UPDATE TO pos_operator_runtime
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
GRANT SELECT (id, tenant_id, branch_id, created_by_membership_id, category,
              title, details, status, customer_visible_update, revision,
              created_at, updated_at)
    ON hosted_support_requests TO pos_api_runtime, pos_operator_runtime;
GRANT INSERT (tenant_id, branch_id, created_by_membership_id, category, title,
              details, status, customer_visible_update, revision,
              idempotency_key, request_hash)
    ON hosted_support_requests TO pos_api_runtime;
GRANT SELECT (idempotency_key, request_hash)
    ON hosted_support_requests TO pos_api_runtime;
GRANT UPDATE (status, customer_visible_update, revision, updated_at)
    ON hosted_support_requests TO pos_operator_runtime;
REVOKE UPDATE, DELETE, TRUNCATE ON hosted_support_requests FROM pos_api_runtime;
REVOKE DELETE, TRUNCATE ON hosted_support_requests FROM pos_operator_runtime;

CREATE TABLE operator_support_issue_support_requests (
    issue_id UUID NOT NULL REFERENCES operator_support_issues(id) ON DELETE RESTRICT,
    support_request_id UUID NOT NULL UNIQUE REFERENCES hosted_support_requests(id) ON DELETE RESTRICT,
    relationship TEXT NOT NULL CHECK (relationship IN ('primary','duplicate','related')),
    linked_by_operator_id UUID NOT NULL REFERENCES operator_accounts(id) ON DELETE RESTRICT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (issue_id, support_request_id)
);
CREATE UNIQUE INDEX operator_support_issue_one_primary_support_request
    ON operator_support_issue_support_requests(issue_id) WHERE relationship = 'primary';
ALTER TABLE operator_support_issue_support_requests ENABLE ROW LEVEL SECURITY;
ALTER TABLE operator_support_issue_support_requests FORCE ROW LEVEL SECURITY;
CREATE POLICY operator_support_issue_support_requests_access
ON operator_support_issue_support_requests FOR ALL TO pos_operator_runtime
USING (
    EXISTS (
        SELECT 1 FROM operator_accounts oa
        WHERE oa.id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
          AND oa.status = 'active' AND oa.role = 'support'
    )
)
WITH CHECK (
    linked_by_operator_id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
    AND EXISTS (
        SELECT 1 FROM operator_accounts oa
        WHERE oa.id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
          AND oa.status = 'active' AND oa.role = 'support'
    )
);
GRANT SELECT (issue_id, support_request_id, relationship, linked_by_operator_id, created_at)
    ON operator_support_issue_support_requests TO pos_operator_runtime;
GRANT INSERT (issue_id, support_request_id, relationship, linked_by_operator_id)
    ON operator_support_issue_support_requests TO pos_operator_runtime;
REVOKE UPDATE, DELETE, TRUNCATE ON operator_support_issue_support_requests FROM pos_operator_runtime;

ALTER TABLE operator_support_issue_events
    ADD COLUMN linked_support_request_id UUID REFERENCES hosted_support_requests(id) ON DELETE RESTRICT;
ALTER TABLE operator_support_issue_events
    DROP CONSTRAINT operator_support_issue_events_event_kind_check;
ALTER TABLE operator_support_issue_events
    ADD CONSTRAINT operator_support_issue_events_event_kind_check
    CHECK (event_kind IN ('created','updated','status_changed','assignment_changed',
                          'schedule_changed','private_note','payment_request_linked',
                          'support_request_linked'));
ALTER TABLE operator_support_issue_events
    ADD CONSTRAINT operator_support_issue_event_support_link_check
    CHECK ((event_kind = 'support_request_linked') = (linked_support_request_id IS NOT NULL));
GRANT SELECT (linked_support_request_id) ON operator_support_issue_events TO pos_operator_runtime;
GRANT INSERT (linked_support_request_id) ON operator_support_issue_events TO pos_operator_runtime;

CREATE TABLE operator_support_request_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    support_request_id UUID NOT NULL REFERENCES hosted_support_requests(id) ON DELETE RESTRICT,
    operator_id UUID NOT NULL REFERENCES operator_accounts(id) ON DELETE RESTRICT,
    event_kind TEXT NOT NULL CHECK (event_kind IN ('public_update','private_note')),
    previous_status TEXT,
    new_status TEXT,
    customer_visible_update TEXT CHECK (customer_visible_update IS NULL OR length(customer_visible_update) <= 1000),
    private_note TEXT CHECK (private_note IS NULL OR length(private_note) <= 2000),
    resulting_revision INTEGER NOT NULL CHECK (resulting_revision > 0),
    idempotency_key TEXT CHECK (idempotency_key IS NULL OR length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA CHECK (request_hash IS NULL OR octet_length(request_hash) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((idempotency_key IS NULL) = (request_hash IS NULL)),
    UNIQUE (operator_id, idempotency_key),
    CHECK ((event_kind = 'private_note') = (private_note IS NOT NULL)),
    CHECK ((event_kind = 'public_update') = (customer_visible_update IS NOT NULL OR new_status IS NOT NULL))
);
CREATE INDEX operator_support_request_events_history
    ON operator_support_request_events(support_request_id, created_at DESC, id DESC);
ALTER TABLE operator_support_request_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE operator_support_request_events FORCE ROW LEVEL SECURITY;
CREATE POLICY operator_support_request_events_access
ON operator_support_request_events FOR ALL TO pos_operator_runtime
USING (
    EXISTS (
        SELECT 1 FROM operator_accounts oa
        WHERE oa.id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
          AND oa.status = 'active' AND oa.role = 'support'
    )
)
WITH CHECK (
    operator_id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
    AND EXISTS (
        SELECT 1 FROM operator_accounts oa
        WHERE oa.id = operator_support_request_events.operator_id
          AND oa.status = 'active' AND oa.role = 'support'
    )
);
GRANT SELECT (id, support_request_id, operator_id, event_kind, previous_status,
              new_status, customer_visible_update, private_note, resulting_revision,
              idempotency_key, request_hash, created_at)
    ON operator_support_request_events TO pos_operator_runtime;
GRANT INSERT (support_request_id, operator_id, event_kind, previous_status,
              new_status, customer_visible_update, private_note, resulting_revision,
              idempotency_key, request_hash)
    ON operator_support_request_events TO pos_operator_runtime;
REVOKE UPDATE, DELETE, TRUNCATE ON operator_support_request_events FROM pos_operator_runtime;
