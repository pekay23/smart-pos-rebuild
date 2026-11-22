-- Operator-only work items group tenant requests without merging tenant data.
CREATE TABLE operator_support_issues (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title TEXT NOT NULL CHECK (length(btrim(title)) BETWEEN 3 AND 200),
    category TEXT NOT NULL CHECK (category IN ('bug','feature_request','payment_integration','account_access','data_issue','other')),
    severity TEXT NOT NULL DEFAULT 'normal' CHECK (severity IN ('critical','high','normal','low')),
    status TEXT NOT NULL DEFAULT 'inbox' CHECK (status IN ('inbox','triaged','scheduled','in_progress','review','qa','release','resolved','blocked','duplicate','reopened')),
    assignee_operator_id UUID REFERENCES operator_accounts(id) ON DELETE RESTRICT,
    due_at TIMESTAMPTZ,
    target_release TEXT CHECK (target_release IS NULL OR length(target_release) <= 80),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_by_operator_id UUID NOT NULL REFERENCES operator_accounts(id) ON DELETE RESTRICT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX operator_support_issues_queue ON operator_support_issues(status, due_at, updated_at DESC, id DESC);
ALTER TABLE operator_support_issues ENABLE ROW LEVEL SECURITY;
ALTER TABLE operator_support_issues FORCE ROW LEVEL SECURITY;
CREATE POLICY operator_support_issues_support_select
ON operator_support_issues FOR SELECT TO pos_operator_runtime
USING (
    EXISTS (
        SELECT 1 FROM operator_accounts oa
        WHERE oa.id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
          AND oa.status = 'active' AND oa.role = 'support'
    )
);
CREATE POLICY operator_support_issues_support_insert
ON operator_support_issues FOR INSERT TO pos_operator_runtime
WITH CHECK (
    created_by_operator_id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
    AND
    EXISTS (
        SELECT 1 FROM operator_accounts oa
        WHERE oa.id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
          AND oa.status = 'active' AND oa.role = 'support'
    )
);
CREATE POLICY operator_support_issues_support_update
ON operator_support_issues FOR UPDATE TO pos_operator_runtime
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

CREATE TABLE operator_support_issue_payment_requests (
    issue_id UUID NOT NULL REFERENCES operator_support_issues(id) ON DELETE RESTRICT,
    request_id UUID NOT NULL UNIQUE REFERENCES hosted_payment_integration_requests(id) ON DELETE RESTRICT,
    relationship TEXT NOT NULL CHECK (relationship IN ('primary','duplicate','related')),
    linked_by_operator_id UUID NOT NULL REFERENCES operator_accounts(id) ON DELETE RESTRICT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (issue_id, request_id)
);
CREATE UNIQUE INDEX operator_support_issue_one_primary_request
    ON operator_support_issue_payment_requests(issue_id) WHERE relationship = 'primary';
ALTER TABLE operator_support_issue_payment_requests ENABLE ROW LEVEL SECURITY;
ALTER TABLE operator_support_issue_payment_requests FORCE ROW LEVEL SECURITY;
CREATE POLICY operator_support_issue_links_support_access
ON operator_support_issue_payment_requests FOR ALL TO pos_operator_runtime
USING (
    EXISTS (
        SELECT 1 FROM operator_accounts oa
        WHERE oa.id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
          AND oa.status = 'active' AND oa.role = 'support'
    )
)
WITH CHECK (
    linked_by_operator_id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
    AND
    EXISTS (
        SELECT 1 FROM operator_accounts oa
        WHERE oa.id = NULLIF(current_setting('pos.operator_id', true), '')::uuid
          AND oa.status = 'active' AND oa.role = 'support'
    )
);

CREATE TABLE operator_support_issue_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    issue_id UUID NOT NULL REFERENCES operator_support_issues(id) ON DELETE RESTRICT,
    operator_id UUID NOT NULL REFERENCES operator_accounts(id) ON DELETE RESTRICT,
    event_kind TEXT NOT NULL CHECK (event_kind IN ('created','updated','status_changed','assignment_changed','schedule_changed','private_note','payment_request_linked')),
    previous_status TEXT,
    new_status TEXT,
    previous_assignee_operator_id UUID REFERENCES operator_accounts(id) ON DELETE RESTRICT,
    new_assignee_operator_id UUID REFERENCES operator_accounts(id) ON DELETE RESTRICT,
    due_at TIMESTAMPTZ,
    target_release TEXT,
    private_note TEXT CHECK (private_note IS NULL OR length(private_note) <= 2000),
    linked_request_id UUID REFERENCES hosted_payment_integration_requests(id) ON DELETE RESTRICT,
    resulting_revision INTEGER NOT NULL CHECK (resulting_revision > 0),
    idempotency_key TEXT CHECK (idempotency_key IS NULL OR length(idempotency_key) BETWEEN 1 AND 128),
    request_hash BYTEA CHECK (request_hash IS NULL OR octet_length(request_hash) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((idempotency_key IS NULL) = (request_hash IS NULL)),
    UNIQUE (operator_id, idempotency_key),
    CHECK ((event_kind = 'private_note') = (private_note IS NOT NULL)),
    CHECK ((event_kind = 'payment_request_linked') = (linked_request_id IS NOT NULL))
);
CREATE INDEX operator_support_issue_events_history ON operator_support_issue_events(issue_id, created_at DESC, id DESC);
ALTER TABLE operator_support_issue_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE operator_support_issue_events FORCE ROW LEVEL SECURITY;
CREATE POLICY operator_support_issue_events_support_access
ON operator_support_issue_events FOR ALL TO pos_operator_runtime
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
        WHERE oa.id = operator_support_issue_events.operator_id
          AND oa.status = 'active' AND oa.role = 'support'
    )
);

GRANT SELECT (id, title, category, severity, status, assignee_operator_id,
              due_at, target_release, revision, created_by_operator_id, created_at, updated_at)
    ON operator_support_issues TO pos_operator_runtime;
GRANT INSERT (title, category, severity, status, assignee_operator_id, due_at,
              target_release, created_by_operator_id)
    ON operator_support_issues TO pos_operator_runtime;
GRANT UPDATE (title, category, severity, status, assignee_operator_id,
              due_at, target_release, revision, updated_at)
    ON operator_support_issues TO pos_operator_runtime;
GRANT SELECT (issue_id, request_id, relationship, linked_by_operator_id, created_at)
    ON operator_support_issue_payment_requests TO pos_operator_runtime;
GRANT INSERT (issue_id, request_id, relationship, linked_by_operator_id)
    ON operator_support_issue_payment_requests TO pos_operator_runtime;
GRANT SELECT (id, issue_id, operator_id, event_kind, previous_status, new_status,
              previous_assignee_operator_id, new_assignee_operator_id, due_at,
              target_release, private_note, linked_request_id, resulting_revision,
              idempotency_key, request_hash, created_at)
    ON operator_support_issue_events TO pos_operator_runtime;
GRANT INSERT (issue_id, operator_id, event_kind, previous_status, new_status,
              previous_assignee_operator_id, new_assignee_operator_id, due_at,
              target_release, private_note, linked_request_id, resulting_revision,
              idempotency_key, request_hash)
    ON operator_support_issue_events TO pos_operator_runtime;
REVOKE DELETE, TRUNCATE ON operator_support_issues, operator_support_issue_payment_requests,
    operator_support_issue_events FROM pos_operator_runtime;
REVOKE UPDATE, DELETE, TRUNCATE ON operator_support_issue_events FROM pos_operator_runtime;
