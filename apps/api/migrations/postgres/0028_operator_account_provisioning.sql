-- Offline operator account lifecycle with a separate, non-bypass credential.
CREATE TABLE operator_provisioning_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_label TEXT NOT NULL CHECK (length(btrim(actor_label)) BETWEEN 1 AND 128),
    action TEXT NOT NULL CHECK (action IN ('account_provisioned','account_status_changed')),
    target_operator_id UUID NOT NULL REFERENCES operator_accounts(id) ON DELETE RESTRICT,
    issuer TEXT NOT NULL CHECK (length(issuer) BETWEEN 1 AND 512),
    subject TEXT NOT NULL CHECK (length(subject) BETWEEN 1 AND 512),
    role TEXT NOT NULL CHECK (role IN ('support','billing','release','security')),
    status TEXT NOT NULL CHECK (status IN ('active','suspended','deactivated')),
    reason TEXT NOT NULL CHECK (length(btrim(reason)) BETWEEN 1 AND 500),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
ALTER TABLE operator_provisioning_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE operator_provisioning_events FORCE ROW LEVEL SECURITY;

CREATE POLICY operator_accounts_provisioner_select
ON operator_accounts FOR SELECT TO pos_operator_provisioner USING (true);
CREATE POLICY operator_accounts_provisioner_insert
ON operator_accounts FOR INSERT TO pos_operator_provisioner WITH CHECK (true);
CREATE POLICY operator_accounts_provisioner_update
ON operator_accounts FOR UPDATE TO pos_operator_provisioner USING (true) WITH CHECK (true);
CREATE POLICY operator_provisioning_events_provisioner_insert
ON operator_provisioning_events FOR INSERT TO pos_operator_provisioner WITH CHECK (true);

GRANT USAGE ON SCHEMA public TO pos_operator_provisioner;
GRANT SELECT (id, issuer, subject, role, status)
    ON operator_accounts TO pos_operator_provisioner;
GRANT INSERT (id, issuer, subject, role, status)
    ON operator_accounts TO pos_operator_provisioner;
GRANT UPDATE (role, status, deactivated_at)
    ON operator_accounts TO pos_operator_provisioner;
GRANT INSERT (actor_label, action, target_operator_id, issuer, subject, role, status, reason)
    ON operator_provisioning_events TO pos_operator_provisioner;
REVOKE DELETE, TRUNCATE ON operator_accounts, operator_provisioning_events FROM pos_operator_provisioner;
REVOKE UPDATE, DELETE, TRUNCATE ON operator_provisioning_events FROM pos_operator_provisioner;
