-- Preserve the checkout-time customer field and keep later customer links
-- separately attributable. One post-sale association is allowed per sale.
CREATE TABLE hosted_sale_customer_links (
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    sale_id UUID NOT NULL,
    customer_id UUID NOT NULL,
    linked_by_membership_id UUID NOT NULL,
    linked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, sale_id),
    FOREIGN KEY (tenant_id, branch_id, sale_id)
        REFERENCES hosted_sales(tenant_id, branch_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, customer_id)
        REFERENCES hosted_customers(tenant_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (linked_by_membership_id, tenant_id)
        REFERENCES memberships(id, tenant_id) ON DELETE RESTRICT
);
CREATE INDEX hosted_sale_customer_links_branch
    ON hosted_sale_customer_links(tenant_id, branch_id, linked_at DESC);

ALTER TABLE hosted_sale_customer_links ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosted_sale_customer_links FORCE ROW LEVEL SECURITY;

CREATE POLICY hosted_sale_customer_links_select ON hosted_sale_customer_links
    FOR SELECT USING (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM hosted_sales s
            WHERE s.tenant_id = hosted_sale_customer_links.tenant_id
              AND s.branch_id = hosted_sale_customer_links.branch_id
              AND s.station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid
              AND s.id = hosted_sale_customer_links.sale_id
        )
    );

CREATE POLICY hosted_sale_customer_links_insert ON hosted_sale_customer_links
    FOR INSERT WITH CHECK (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND linked_by_membership_id = NULLIF(current_setting('pos.membership_id', true), '')::uuid
        AND EXISTS (
            SELECT 1
            FROM hosted_sales s
            JOIN memberships m ON m.id = linked_by_membership_id
                              AND m.tenant_id = s.tenant_id
            JOIN membership_role_grants g ON g.tenant_id = m.tenant_id
                                         AND g.membership_id = m.id
            WHERE s.tenant_id = hosted_sale_customer_links.tenant_id
              AND s.branch_id = hosted_sale_customer_links.branch_id
              AND s.station_id = NULLIF(current_setting('pos.station_id', true), '')::uuid
              AND s.id = hosted_sale_customer_links.sale_id
              AND s.status = 'completed'
              AND s.customer_id IS NULL
              AND m.identity_id = NULLIF(current_setting('pos.identity_id', true), '')::uuid
              AND m.status = 'active'
              AND g.role_key IN ('owner', 'admin', 'manager', 'cashier')
              AND (g.branch_id IS NULL OR g.branch_id = s.branch_id)
              AND (g.station_id IS NULL OR g.station_id = s.station_id)
              AND (g.role_key <> 'cashier' OR m.id = s.cashier_membership_id)
              AND EXISTS (
                  SELECT 1 FROM hosted_customers c
                  WHERE c.tenant_id = hosted_sale_customer_links.tenant_id
                    AND c.id = hosted_sale_customer_links.customer_id
                    AND c.status = 'active'
              )
        )
    );
