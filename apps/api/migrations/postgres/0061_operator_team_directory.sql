-- Keep the operator directory separate from authentication identifiers.
CREATE TABLE operator_directory_profiles (
    operator_id UUID PRIMARY KEY REFERENCES operator_accounts(id) ON DELETE RESTRICT,
    display_name TEXT NOT NULL CHECK (length(btrim(display_name)) BETWEEN 1 AND 80),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','inactive')),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO operator_directory_profiles(operator_id,display_name,status)
SELECT id,'Support operator',CASE WHEN status='active' THEN 'active' ELSE 'inactive' END
FROM operator_accounts;
ALTER TABLE operator_directory_profiles ENABLE ROW LEVEL SECURITY;
ALTER TABLE operator_directory_profiles FORCE ROW LEVEL SECURITY;
CREATE POLICY operator_directory_profiles_support_read
ON operator_directory_profiles FOR SELECT TO pos_operator_runtime
USING (
    status='active'
    AND EXISTS (
        SELECT 1 FROM operator_accounts oa
        WHERE oa.id=NULLIF(current_setting('pos.operator_id',true),'')::uuid
          AND oa.role='support' AND oa.status='active'
    )
);
CREATE POLICY operator_directory_profiles_provisioner_read
ON operator_directory_profiles FOR SELECT TO pos_operator_provisioner USING (true);
CREATE POLICY operator_directory_profiles_provisioner_insert
ON operator_directory_profiles FOR INSERT TO pos_operator_provisioner WITH CHECK (true);
CREATE POLICY operator_directory_profiles_provisioner_update
ON operator_directory_profiles FOR UPDATE TO pos_operator_provisioner USING (true) WITH CHECK (true);
GRANT SELECT (operator_id,display_name,status) ON operator_directory_profiles TO pos_operator_runtime;
REVOKE INSERT,UPDATE,DELETE,TRUNCATE ON operator_directory_profiles FROM pos_operator_runtime;
GRANT SELECT (operator_id,display_name,status) ON operator_directory_profiles TO pos_operator_provisioner;
GRANT INSERT (operator_id,display_name,status) ON operator_directory_profiles TO pos_operator_provisioner;
GRANT UPDATE (display_name,status,updated_at) ON operator_directory_profiles TO pos_operator_provisioner;
REVOKE DELETE,TRUNCATE ON operator_directory_profiles FROM pos_operator_provisioner;

CREATE FUNCTION sync_operator_directory_profile() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
BEGIN
    INSERT INTO public.operator_directory_profiles(operator_id,display_name,status)
    VALUES(NEW.id,'Support operator',CASE WHEN NEW.status='active' THEN 'active' ELSE 'inactive' END)
    ON CONFLICT (operator_id) DO UPDATE
       SET status=EXCLUDED.status,updated_at=now();
    RETURN NEW;
END;
$$;
REVOKE ALL ON FUNCTION sync_operator_directory_profile() FROM PUBLIC;
CREATE TRIGGER operator_directory_profile_sync
AFTER INSERT OR UPDATE OF status ON operator_accounts
FOR EACH ROW EXECUTE FUNCTION sync_operator_directory_profile();

ALTER TABLE operator_provisioning_events
    DROP CONSTRAINT operator_provisioning_events_action_check;
ALTER TABLE operator_provisioning_events
    ADD CONSTRAINT operator_provisioning_events_action_check
    CHECK (action IN ('account_provisioned','account_status_changed','display_name_changed'));
ALTER TABLE operator_provisioning_events ADD COLUMN display_name TEXT
    CHECK (display_name IS NULL OR length(btrim(display_name)) BETWEEN 1 AND 80);
GRANT INSERT (display_name) ON operator_provisioning_events TO pos_operator_provisioner;

GRANT SELECT (assignee_operator_id) ON operator_support_issues TO pos_operator_runtime;
