-- Advanced POS hosted API foundation.
-- Apply only through the migration runner after the PostgreSQL target is provisioned.
-- Authorization still belongs to the Rust API; these tables do not create an auth bypass.

CREATE TABLE tenants (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    display_name TEXT NOT NULL CHECK (length(btrim(display_name)) BETWEEN 1 AND 160),
    legal_name TEXT CHECK (legal_name IS NULL OR length(legal_name) <= 200),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'suspended', 'closed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Provider identity is stable issuer + subject. Contact fields are optional and are
-- deliberately not unique: neither email nor a mutable username identifies a person.
CREATE TABLE person_identities (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    issuer TEXT NOT NULL CHECK (length(issuer) BETWEEN 1 AND 512),
    subject TEXT NOT NULL CHECK (length(subject) BETWEEN 1 AND 512),
    email TEXT,
    display_name TEXT CHECK (display_name IS NULL OR length(display_name) <= 160),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'suspended', 'removed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (issuer, subject),
    UNIQUE (id, status)
);

CREATE TABLE memberships (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    identity_id UUID NOT NULL REFERENCES person_identities(id) ON DELETE RESTRICT,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('invited', 'active', 'suspended', 'removed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, identity_id),
    UNIQUE (id, tenant_id)
);

CREATE INDEX memberships_by_identity ON memberships (identity_id, status);

CREATE TABLE branches (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    code TEXT NOT NULL CHECK (length(btrim(code)) BETWEEN 1 AND 64),
    name TEXT NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 160),
    timezone TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'inactive', 'closed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, code),
    UNIQUE (id, tenant_id)
);

CREATE TABLE stations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    code TEXT NOT NULL CHECK (length(btrim(code)) BETWEEN 1 AND 64),
    name TEXT NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 160),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'inactive', 'revoked')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (branch_id, tenant_id) REFERENCES branches(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE (branch_id, code),
    UNIQUE (id, branch_id, tenant_id)
);

-- System and tenant-defined role keys are resolved by server policy. A grant has
-- exactly one scope: tenant, branch, or station. Composite foreign keys prevent
-- attaching a grant to a branch/station from a different tenant.
CREATE TABLE membership_role_grants (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL,
    membership_id UUID NOT NULL,
    role_key TEXT NOT NULL CHECK (length(btrim(role_key)) BETWEEN 1 AND 96),
    branch_id UUID,
    station_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE CASCADE,
    FOREIGN KEY (branch_id, tenant_id) REFERENCES branches(id, tenant_id) ON DELETE CASCADE,
    FOREIGN KEY (station_id, branch_id, tenant_id) REFERENCES stations(id, branch_id, tenant_id) ON DELETE CASCADE,
    CHECK (
        (branch_id IS NULL AND station_id IS NULL) OR
        (branch_id IS NOT NULL AND station_id IS NULL) OR
        (branch_id IS NOT NULL AND station_id IS NOT NULL)
    ),
    CHECK (branch_id IS NULL OR branch_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    CHECK (station_id IS NULL OR station_id <> '00000000-0000-0000-0000-000000000000'::uuid)
);

-- COALESCE preserves one logical tenant/branch/station grant on PostgreSQL 14+.
CREATE UNIQUE INDEX role_grants_unique_scope ON membership_role_grants (
    membership_id,
    tenant_id,
    role_key,
    COALESCE(branch_id, '00000000-0000-0000-0000-000000000000'::uuid),
    COALESCE(station_id, '00000000-0000-0000-0000-000000000000'::uuid)
);

CREATE INDEX role_grants_by_membership ON membership_role_grants (tenant_id, membership_id);
CREATE INDEX role_grants_by_branch ON membership_role_grants (tenant_id, branch_id) WHERE branch_id IS NOT NULL;
CREATE INDEX role_grants_by_station ON membership_role_grants (tenant_id, station_id) WHERE station_id IS NOT NULL;

CREATE TABLE audit_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    actor_membership_id UUID,
    action TEXT NOT NULL CHECK (length(btrim(action)) BETWEEN 1 AND 128),
    entity_type TEXT NOT NULL CHECK (length(btrim(entity_type)) BETWEEN 1 AND 96),
    entity_id TEXT,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(metadata) = 'object'),
    FOREIGN KEY (actor_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT
);

CREATE INDEX audit_events_tenant_time ON audit_events (tenant_id, occurred_at DESC, id);

