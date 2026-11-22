-- Add accounting coverage for hosted sales/refunds that predate migration 0044.
-- Do not infer historical COGS from current catalog cost; these journals remain partial.

WITH eligible_sales AS (
    SELECT s.tenant_id, s.branch_id, s.id, s.currency_code, s.created_at,
           s.cashier_membership_id, s.subtotal_minor, s.tax_minor, s.total_minor
    FROM hosted_sales s
    JOIN hosted_sale_tenders t
      ON t.tenant_id=s.tenant_id AND t.branch_id=s.branch_id AND t.sale_id=s.id
    GROUP BY s.tenant_id, s.branch_id, s.id, s.currency_code, s.created_at,
             s.cashier_membership_id, s.subtotal_minor, s.tax_minor, s.total_minor
    HAVING s.subtotal_minor + s.tax_minor = s.total_minor
       AND SUM(t.amount_minor) = s.total_minor
), inserted_sales AS (
    INSERT INTO hosted_accounting_journals
        (id,tenant_id,branch_id,source_type,source_id,currency_code,effective_at,
         actor_membership_id,description,coverage_status)
    SELECT gen_random_uuid(), s.tenant_id, s.branch_id, 'sale', s.id, s.currency_code,
           s.created_at, s.cashier_membership_id,
           'Historical sale; cost and tax classification unavailable', 'partial'
    FROM eligible_sales s
    ON CONFLICT (tenant_id,branch_id,source_type,source_id) DO NOTHING
    RETURNING id,tenant_id,branch_id,source_id
), grouped_sale_tenders AS (
    SELECT j.id AS journal_id, j.tenant_id, j.branch_id,
           CASE t.tender_kind
             WHEN 'cash' THEN '1000'
             WHEN 'card' THEN '1010'
             WHEN 'mobile_money' THEN '1020'
             WHEN 'bank_transfer' THEN '1030'
             ELSE '1090'
           END AS account_code,
           SUM(t.amount_minor) AS amount_minor
    FROM inserted_sales j
    JOIN hosted_sale_tenders t
      ON t.tenant_id=j.tenant_id AND t.branch_id=j.branch_id AND t.sale_id=j.source_id
    GROUP BY j.id,j.tenant_id,j.branch_id,account_code
), numbered_sale_tenders AS (
    SELECT t.*,
           row_number() OVER (PARTITION BY t.journal_id ORDER BY t.account_code)::smallint AS line_number
    FROM grouped_sale_tenders t
), sale_line_counts AS (
    SELECT journal_id,MAX(line_number)::integer AS line_count
    FROM numbered_sale_tenders GROUP BY journal_id
), sale_summary_lines AS (
    SELECT j.id AS journal_id, j.tenant_id, j.branch_id,
           (c.line_count + 1)::smallint AS line_number, '4000'::text AS account_code,
           'Historical sales revenue'::text AS description,
           0::bigint AS debit_minor, s.subtotal_minor AS credit_minor
    FROM inserted_sales j
    JOIN eligible_sales s ON s.tenant_id=j.tenant_id AND s.branch_id=j.branch_id AND s.id=j.source_id
    JOIN sale_line_counts c ON c.journal_id=j.id
    WHERE s.subtotal_minor > 0
    UNION ALL
    SELECT j.id, j.tenant_id, j.branch_id,
           (c.line_count + 2)::smallint, '2100', 'Historical tax; jurisdiction classification unavailable',
           0::bigint, s.tax_minor
    FROM inserted_sales j
    JOIN eligible_sales s ON s.tenant_id=j.tenant_id AND s.branch_id=j.branch_id AND s.id=j.source_id
    JOIN sale_line_counts c ON c.journal_id=j.id
    WHERE s.tax_minor > 0
)
INSERT INTO hosted_accounting_journal_lines
    (tenant_id,branch_id,journal_id,line_number,account_code,description,debit_minor,credit_minor)
SELECT tenant_id,branch_id,journal_id,line_number,account_code,'Historical sale tender',amount_minor,0
FROM numbered_sale_tenders
UNION ALL
SELECT tenant_id,branch_id,journal_id,line_number,account_code,description,debit_minor,credit_minor
FROM sale_summary_lines;

WITH refund_line_totals AS (
    SELECT tenant_id,branch_id,refund_id,
           SUM(subtotal_minor) AS subtotal_minor,
           SUM(tax_minor) AS tax_minor,
           SUM(amount_minor) AS amount_minor
    FROM hosted_sale_refund_lines
    GROUP BY tenant_id,branch_id,refund_id
), refund_tender_totals AS (
    SELECT tenant_id,branch_id,refund_id,SUM(amount_minor) AS amount_minor
    FROM hosted_sale_refund_tenders
    GROUP BY tenant_id,branch_id,refund_id
), eligible_refunds AS (
    SELECT r.tenant_id,r.branch_id,r.id,s.currency_code,r.created_at,
           r.actor_membership_id,r.total_minor,l.subtotal_minor,l.tax_minor
    FROM hosted_sale_refunds r
    JOIN hosted_sales s
      ON s.tenant_id=r.tenant_id AND s.branch_id=r.branch_id AND s.id=r.sale_id
    JOIN refund_line_totals l
      ON l.tenant_id=r.tenant_id AND l.branch_id=r.branch_id AND l.refund_id=r.id
    JOIN refund_tender_totals t
      ON t.tenant_id=r.tenant_id AND t.branch_id=r.branch_id AND t.refund_id=r.id
    WHERE l.amount_minor=r.total_minor
      AND l.subtotal_minor + l.tax_minor=r.total_minor
      AND t.amount_minor=r.total_minor
), inserted_refunds AS (
    INSERT INTO hosted_accounting_journals
        (id,tenant_id,branch_id,source_type,source_id,currency_code,effective_at,
         actor_membership_id,description,coverage_status)
    SELECT gen_random_uuid(),r.tenant_id,r.branch_id,'sale_refund',r.id,r.currency_code,
           r.created_at,r.actor_membership_id,
           'Historical refund; original cost snapshot unavailable','partial'
    FROM eligible_refunds r
    ON CONFLICT (tenant_id,branch_id,source_type,source_id) DO NOTHING
    RETURNING id,tenant_id,branch_id,source_id
), grouped_refund_tenders AS (
    SELECT j.id AS journal_id,j.tenant_id,j.branch_id,
           CASE t.method
             WHEN 'cash' THEN '1000'
             WHEN 'card' THEN '1010'
             WHEN 'mobile_money' THEN '1020'
             WHEN 'bank_transfer' THEN '1030'
             ELSE '1090'
           END AS account_code,
           SUM(t.amount_minor) AS amount_minor
    FROM inserted_refunds j
    JOIN hosted_sale_refund_tenders t
      ON t.tenant_id=j.tenant_id AND t.branch_id=j.branch_id AND t.refund_id=j.source_id
    GROUP BY j.id,j.tenant_id,j.branch_id,account_code
), numbered_refund_tenders AS (
    SELECT t.*,
           row_number() OVER (PARTITION BY t.journal_id ORDER BY t.account_code)::smallint AS line_number
    FROM grouped_refund_tenders t
), refund_line_counts AS (
    SELECT journal_id,MAX(line_number)::integer AS line_count
    FROM numbered_refund_tenders GROUP BY journal_id
), refund_summary_lines AS (
    SELECT j.id AS journal_id,j.tenant_id,j.branch_id,
           (c.line_count + 1)::smallint AS line_number,'4020'::text AS account_code,
           'Historical sales return'::text AS description,
           r.subtotal_minor AS debit_minor,0::bigint AS credit_minor
    FROM inserted_refunds j
    JOIN eligible_refunds r ON r.tenant_id=j.tenant_id AND r.branch_id=j.branch_id AND r.id=j.source_id
    JOIN refund_line_counts c ON c.journal_id=j.id
    WHERE r.subtotal_minor > 0
    UNION ALL
    SELECT j.id,j.tenant_id,j.branch_id,
           (c.line_count + 2)::smallint,'2100','Historical tax reversal; jurisdiction classification unavailable',
           r.tax_minor,0::bigint
    FROM inserted_refunds j
    JOIN eligible_refunds r ON r.tenant_id=j.tenant_id AND r.branch_id=j.branch_id AND r.id=j.source_id
    JOIN refund_line_counts c ON c.journal_id=j.id
    WHERE r.tax_minor > 0
)
INSERT INTO hosted_accounting_journal_lines
    (tenant_id,branch_id,journal_id,line_number,account_code,description,debit_minor,credit_minor)
SELECT tenant_id,branch_id,journal_id,line_number,account_code,'Historical refund tender',0,amount_minor
FROM numbered_refund_tenders
UNION ALL
SELECT tenant_id,branch_id,journal_id,line_number,account_code,description,debit_minor,credit_minor
FROM refund_summary_lines;
