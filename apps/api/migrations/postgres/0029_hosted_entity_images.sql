-- Tenant-owned images for products, customers, and suppliers. Keep image bytes
-- out of catalog and directory payloads so normal list requests stay small.
CREATE TABLE hosted_entity_images (
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    entity_type TEXT NOT NULL CHECK (entity_type IN ('product','customer','supplier')),
    entity_id UUID NOT NULL,
    content_type TEXT NOT NULL CHECK (content_type IN ('image/jpeg','image/png','image/webp')),
    byte_length INTEGER NOT NULL CHECK (byte_length BETWEEN 1 AND 2097152),
    image_data BYTEA NOT NULL CHECK (octet_length(image_data) = byte_length),
    created_by_membership_id UUID NOT NULL,
    updated_by_membership_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, entity_type, entity_id),
    FOREIGN KEY (created_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (updated_by_membership_id, tenant_id) REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT
);
CREATE INDEX hosted_entity_images_updated ON hosted_entity_images(tenant_id, updated_at DESC);
ALTER TABLE hosted_entity_images ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_entity_images FORCE ROW LEVEL SECURITY;

CREATE POLICY hosted_entity_images_select ON hosted_entity_images FOR SELECT USING (
    tenant_id = NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
          AND m.tenant_id=hosted_entity_images.tenant_id AND m.status='active'
          AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid)
          AND (g.station_id IS NULL OR g.station_id=NULLIF(current_setting('pos.station_id',true),'')::uuid)
          AND CASE hosted_entity_images.entity_type
              WHEN 'product' THEN g.role_key IN ('owner','admin','manager','cashier')
              WHEN 'customer' THEN g.role_key IN ('owner','admin','manager','cashier')
              WHEN 'supplier' THEN g.role_key IN ('owner','admin','manager')
              ELSE FALSE
          END
    )
    AND CASE entity_type
        WHEN 'product' THEN EXISTS (SELECT 1 FROM catalog_products p WHERE p.tenant_id=hosted_entity_images.tenant_id AND p.id=hosted_entity_images.entity_id)
        WHEN 'customer' THEN EXISTS (SELECT 1 FROM hosted_customers c WHERE c.tenant_id=hosted_entity_images.tenant_id AND c.id=hosted_entity_images.entity_id)
        WHEN 'supplier' THEN EXISTS (SELECT 1 FROM hosted_suppliers s WHERE s.tenant_id=hosted_entity_images.tenant_id AND s.id=hosted_entity_images.entity_id)
        ELSE FALSE
    END
);

CREATE POLICY hosted_entity_images_insert ON hosted_entity_images FOR INSERT WITH CHECK (
    tenant_id = NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND created_by_membership_id = NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND updated_by_membership_id = NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=created_by_membership_id
          AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
          AND m.tenant_id=hosted_entity_images.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid)
          AND g.station_id IS NULL
    )
    AND CASE entity_type
        WHEN 'product' THEN EXISTS (SELECT 1 FROM catalog_products p WHERE p.tenant_id=hosted_entity_images.tenant_id AND p.id=hosted_entity_images.entity_id)
        WHEN 'customer' THEN EXISTS (SELECT 1 FROM hosted_customers c WHERE c.tenant_id=hosted_entity_images.tenant_id AND c.id=hosted_entity_images.entity_id)
        WHEN 'supplier' THEN EXISTS (SELECT 1 FROM hosted_suppliers s WHERE s.tenant_id=hosted_entity_images.tenant_id AND s.id=hosted_entity_images.entity_id)
        ELSE FALSE
    END
);

CREATE POLICY hosted_entity_images_update ON hosted_entity_images FOR UPDATE
USING (
    tenant_id = NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
          AND m.tenant_id=hosted_entity_images.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid)
          AND g.station_id IS NULL
    )
)
WITH CHECK (
    tenant_id = NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND updated_by_membership_id = NULLIF(current_setting('pos.membership_id',true),'')::uuid
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=updated_by_membership_id AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
          AND m.tenant_id=hosted_entity_images.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid)
          AND g.station_id IS NULL
    )
    AND CASE entity_type
        WHEN 'product' THEN EXISTS (SELECT 1 FROM catalog_products p WHERE p.tenant_id=hosted_entity_images.tenant_id AND p.id=hosted_entity_images.entity_id)
        WHEN 'customer' THEN EXISTS (SELECT 1 FROM hosted_customers c WHERE c.tenant_id=hosted_entity_images.tenant_id AND c.id=hosted_entity_images.entity_id)
        WHEN 'supplier' THEN EXISTS (SELECT 1 FROM hosted_suppliers s WHERE s.tenant_id=hosted_entity_images.tenant_id AND s.id=hosted_entity_images.entity_id)
        ELSE FALSE
    END
);

CREATE POLICY hosted_entity_images_delete ON hosted_entity_images FOR DELETE USING (
    tenant_id = NULLIF(current_setting('pos.tenant_id',true),'')::uuid
    AND EXISTS (
        SELECT 1 FROM memberships m JOIN membership_role_grants g
          ON g.tenant_id=m.tenant_id AND g.membership_id=m.id
        WHERE m.id=NULLIF(current_setting('pos.membership_id',true),'')::uuid
          AND m.identity_id=NULLIF(current_setting('pos.identity_id',true),'')::uuid
          AND m.tenant_id=hosted_entity_images.tenant_id AND m.status='active'
          AND g.role_key IN ('owner','admin','manager')
          AND (g.branch_id IS NULL OR g.branch_id=NULLIF(current_setting('pos.branch_id',true),'')::uuid)
          AND g.station_id IS NULL
    )
);







