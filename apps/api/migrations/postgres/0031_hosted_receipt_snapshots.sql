-- Preserve customer display names as they appeared when the receipt was made
-- or linked. Customer records may later be renamed or archived.
ALTER TABLE hosted_sales ADD COLUMN customer_name_snapshot TEXT;
UPDATE hosted_sales s
SET customer_name_snapshot = c.name
FROM hosted_customers c
WHERE c.tenant_id = s.tenant_id AND c.id = s.customer_id;

ALTER TABLE hosted_sale_customer_links ADD COLUMN customer_name_snapshot TEXT;
UPDATE hosted_sale_customer_links link
SET customer_name_snapshot = c.name
FROM hosted_customers c
WHERE c.tenant_id = link.tenant_id AND c.id = link.customer_id;
ALTER TABLE hosted_sale_customer_links ALTER COLUMN customer_name_snapshot SET NOT NULL;

-- Parent sale row visibility already enforces branch/station role scope; using
-- it here also lets branch report readers retrieve a linked receipt snapshot.
DROP POLICY hosted_sale_customer_links_select ON hosted_sale_customer_links;
CREATE POLICY hosted_sale_customer_links_select ON hosted_sale_customer_links
    FOR SELECT USING (
        tenant_id = NULLIF(current_setting('pos.tenant_id', true), '')::uuid
        AND branch_id = NULLIF(current_setting('pos.branch_id', true), '')::uuid
        AND EXISTS (
            SELECT 1 FROM hosted_sales s
            WHERE s.tenant_id = hosted_sale_customer_links.tenant_id
              AND s.branch_id = hosted_sale_customer_links.branch_id
              AND s.id = hosted_sale_customer_links.sale_id
        )
    );
