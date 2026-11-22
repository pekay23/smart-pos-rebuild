GRANT SELECT (id, issuer, subject, display_name, status) ON person_identities TO pos_api_runtime;
GRANT SELECT (id, tenant_id, identity_id, status) ON memberships TO pos_api_runtime;
GRANT SELECT (id, display_name, status) ON tenants TO pos_api_runtime;
GRANT SELECT (tenant_id, membership_id, role_key, branch_id, station_id)
  ON membership_role_grants TO pos_api_runtime;
GRANT SELECT (id, tenant_id, code, name, timezone, status, currency_code, address, phone, email, opening_hours, settings_revision) ON branches TO pos_api_runtime;
GRANT UPDATE (address, phone, email, opening_hours, settings_revision, updated_at) ON branches TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, code, name, status) ON stations TO pos_api_runtime;
GRANT SELECT (tenant_id, entitlement_key, enabled, numeric_limit, effective_at, expires_at)
  ON tenant_entitlements TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, station_id, device_public_key, status,
              activated_by_membership_id, activated_at, revoked_at,
              revoked_by_membership_id, revocation_reason)
  ON station_activations TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, station_id, device_public_key, status,
              activated_by_membership_id)
  ON station_activations TO pos_api_runtime;
GRANT UPDATE (status, revoked_at, revoked_by_membership_id, revocation_reason)
  ON station_activations TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, station_id, created_by_membership_id,
              nonce, created_at, expires_at, consumed_at)
  ON station_activation_challenges TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, station_id, created_by_membership_id, nonce)
  ON station_activation_challenges TO pos_api_runtime;
GRANT UPDATE (consumed_at) ON station_activation_challenges TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, station_id, membership_id, identity_id,
              activation_id, lease_token_hash, status, started_at, heartbeat_at,
              expires_at, ended_at, ended_reason)
  ON station_actor_leases TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, station_id, membership_id, identity_id,
              activation_id, lease_token_hash, expires_at)
  ON station_actor_leases TO pos_api_runtime;
GRANT UPDATE (status, heartbeat_at, expires_at, ended_at, ended_reason)
  ON station_actor_leases TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, station_id, membership_id, identity_id,
              nonce, created_at, expires_at, consumed_at)
  ON station_actor_lease_challenges TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, station_id, membership_id, identity_id, nonce)
  ON station_actor_lease_challenges TO pos_api_runtime;
GRANT UPDATE (consumed_at) ON station_actor_lease_challenges TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, station_id, membership_id, identity_id,
              activation_id, lease_id, operation, idempotency_key, payload_hash,
              nonce, created_at, expires_at, consumed_at)
  ON station_financial_proof_challenges TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, station_id, membership_id, identity_id,
              activation_id, lease_id, operation, idempotency_key, payload_hash, nonce)
  ON station_financial_proof_challenges TO pos_api_runtime;
GRANT UPDATE (consumed_at) ON station_financial_proof_challenges TO pos_api_runtime;
GRANT SELECT (station_id, tenant_id, branch_id, lease_id)
  ON station_actor_lease_locks TO pos_api_runtime;
GRANT INSERT (station_id, tenant_id, branch_id, lease_id)
  ON station_actor_lease_locks TO pos_api_runtime;
GRANT DELETE ON station_actor_lease_locks TO pos_api_runtime;
GRANT SELECT (id, tenant_id, sku, barcode, name, description, category, status,
              created_by_membership_id, revision)
  ON catalog_products TO pos_api_runtime;
GRANT INSERT (tenant_id, sku, barcode, name, description, category,
              created_by_membership_id)
  ON catalog_products TO pos_api_runtime;
GRANT UPDATE (sku, barcode, name, description, category, revision, updated_at)
  ON catalog_products TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, product_id, price_minor, tax_rate_bps,
              stock_quantity, reorder_point, target_stock_quantity, status, revision)
  ON branch_catalog_items TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, product_id, price_minor, cost_minor,
              tax_rate_bps, stock_quantity, reorder_point, target_stock_quantity)
  ON branch_catalog_items TO pos_api_runtime;
GRANT UPDATE (price_minor, tax_rate_bps, stock_quantity, reorder_point,
              target_stock_quantity, cost_minor, revision, updated_at, status)
  ON branch_catalog_items TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, product_id, actor_membership_id,
              movement_kind, quantity_delta, note, source_id, idempotency_key,
              resulting_stock_quantity, occurred_at)
  ON stock_movements TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, product_id, actor_membership_id, movement_kind,
              quantity_delta, note, source_id, idempotency_key, resulting_stock_quantity)
  ON stock_movements TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, station_id, cashier_membership_id, lease_id,
              activation_id, receipt_number, cashier_name_snapshot, station_name_snapshot,
              currency_code, subtotal_minor, tax_minor, total_minor, on_account_minor, request_hash,
              idempotency_key, status, created_at, cash_shift_id, customer_id,
              customer_name_snapshot)
  ON hosted_sales TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, sale_id, customer_id, currency_code,
              original_amount_minor, credit_term_revision, due_days_snapshot,
              credit_limit_minor_snapshot, issued_at, due_at, issued_by_membership_id, created_at)
  ON hosted_receivable_invoices TO pos_api_runtime;
GRANT INSERT (id, tenant_id, branch_id, sale_id, customer_id, currency_code,
              original_amount_minor, credit_term_revision, due_days_snapshot,
              credit_limit_minor_snapshot, issued_at, due_at, issued_by_membership_id)
  ON hosted_receivable_invoices TO pos_api_runtime;
GRANT INSERT (id, tenant_id, branch_id, station_id, cashier_membership_id, lease_id,
              activation_id, receipt_number, cashier_name_snapshot, station_name_snapshot,
              currency_code, subtotal_minor, tax_minor, total_minor, request_hash,
              idempotency_key, cash_shift_id, customer_id, customer_name_snapshot, on_account_minor)
  ON hosted_sales TO pos_api_runtime;
GRANT SELECT (tenant_id, branch_id, sale_id, customer_id, linked_by_membership_id, linked_at, customer_name_snapshot)
  ON hosted_sale_customer_links TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, sale_id, customer_id, linked_by_membership_id, customer_name_snapshot)
  ON hosted_sale_customer_links TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, station_id, opened_by_membership_id,
              closed_by_membership_id, opening_lease_id, opening_activation_id,
              closing_lease_id, opening_idempotency_key, opening_request_hash,
              close_idempotency_key, close_request_hash, opening_cash_minor, status,
              opened_at, closed_at, counted_cash_minor, expected_cash_minor,
              difference_minor, report_json)
  ON hosted_drawer_shifts TO pos_api_runtime;
GRANT INSERT (id, tenant_id, branch_id, station_id, opened_by_membership_id,
              opening_lease_id, opening_activation_id, opening_idempotency_key,
              opening_request_hash, opening_cash_minor)
  ON hosted_drawer_shifts TO pos_api_runtime;
GRANT UPDATE (closed_by_membership_id, closing_lease_id, status, closed_at,
              counted_cash_minor, expected_cash_minor, difference_minor,
              close_idempotency_key, close_request_hash, report_json)
  ON hosted_drawer_shifts TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, station_id, shift_id, actor_membership_id,
              lease_id, activation_id, movement_type, amount_minor, note,
              idempotency_key, request_hash, created_at, source_payment_id)
  ON hosted_drawer_movements TO pos_api_runtime;
GRANT INSERT (id, tenant_id, branch_id, station_id, shift_id, actor_membership_id,
              lease_id, activation_id, movement_type, amount_minor, note,
              idempotency_key, request_hash, source_payment_id)
  ON hosted_drawer_movements TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, sale_id, line_number, product_id, sku_snapshot,
              barcode_snapshot, name_snapshot, quantity, unit_price_minor, tax_rate_bps,
              subtotal_minor, tax_minor, total_minor)
  ON hosted_sale_lines TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, sale_id, line_number, product_id, sku_snapshot,
              barcode_snapshot, name_snapshot, quantity, unit_price_minor, tax_rate_bps,
              subtotal_minor, tax_minor, total_minor)
  ON hosted_sale_lines TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, sale_id, payment_option_id, method_key_snapshot,
              label_snapshot, tender_kind, provider_name_snapshot, amount_minor,
              reference, recording_mode, approval_status, recorded_at)
  ON hosted_sale_tenders TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, sale_id, payment_option_id, method_key_snapshot,
              label_snapshot, tender_kind, provider_name_snapshot, amount_minor,
              reference)
  ON hosted_sale_tenders TO pos_api_runtime;
GRANT INSERT (tenant_id, actor_membership_id, action, entity_type, entity_id, metadata)
  ON audit_events TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, method_key, label, tender_kind,
              provider_name, country_code, requires_reference, enabled,
              recording_mode, revision, created_by_membership_id,
              updated_by_membership_id, created_at, updated_at)
  ON branch_payment_options TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, method_key, label, tender_kind,
              provider_name, country_code, requires_reference, enabled,
              created_by_membership_id, updated_by_membership_id)
  ON branch_payment_options TO pos_api_runtime;
GRANT UPDATE (label, tender_kind, provider_name, country_code,
              requires_reference, enabled, revision, updated_by_membership_id,
              updated_at)
  ON branch_payment_options TO pos_api_runtime;
GRANT SELECT (id, tenant_id, source_branch_id, destination_branch_id, status, revision,
              request_idempotency_key, request_hash, request_note,
              requested_by_membership_id, requested_at, accepted_by_membership_id, accepted_at,
              rejected_by_membership_id, rejected_at, rejection_note,
              dispatched_by_membership_id, dispatched_at, received_by_membership_id,
              received_at, receipt_note, cancelled_by_membership_id, cancelled_at,
              created_at, updated_at)
  ON stock_transfers TO pos_api_runtime;
GRANT INSERT (id, tenant_id, source_branch_id, destination_branch_id, status, revision,
              request_idempotency_key, request_hash, request_note, requested_by_membership_id)
  ON stock_transfers TO pos_api_runtime;
GRANT UPDATE (status, revision, accepted_by_membership_id, accepted_at,
              rejected_by_membership_id, rejected_at, rejection_note,
              dispatched_by_membership_id, dispatched_at, received_by_membership_id,
              received_at, receipt_note, cancelled_by_membership_id, cancelled_at, updated_at)
  ON stock_transfers TO pos_api_runtime;
GRANT SELECT (id, tenant_id, transfer_id, product_id, sku_snapshot, name_snapshot,
              requested_quantity, accepted_quantity, dispatched_quantity,
              received_quantity, discrepancy_note)
  ON stock_transfer_lines TO pos_api_runtime;
GRANT INSERT (tenant_id, transfer_id, product_id, sku_snapshot, name_snapshot, requested_quantity)
  ON stock_transfer_lines TO pos_api_runtime;
GRANT UPDATE (accepted_quantity, dispatched_quantity, received_quantity, discrepancy_note)
  ON stock_transfer_lines TO pos_api_runtime;
GRANT SELECT (id, tenant_id, origin_branch_id, origin_station_id, pickup_branch_id,
              requested_by_membership_id, customer_id, contact_name_snapshot,
              contact_phone_snapshot, contact_consent_at, status, revision,
              expires_at, sale_id, request_idempotency_key, request_hash,
              created_at, updated_at)
  ON pickup_reservations TO pos_api_runtime;
GRANT INSERT (id, tenant_id, origin_branch_id, origin_station_id, pickup_branch_id,
              requested_by_membership_id, customer_id, contact_name_snapshot,
              contact_phone_snapshot, contact_consent_at, status, revision,
              expires_at, request_idempotency_key, request_hash)
  ON pickup_reservations TO pos_api_runtime;
GRANT UPDATE (status, revision, updated_at, sale_id)
  ON pickup_reservations TO pos_api_runtime;
GRANT SELECT (id, tenant_id, reservation_id, product_id, pickup_branch_id,
              sku_snapshot, name_snapshot, quantity)
  ON pickup_reservation_lines TO pos_api_runtime;
GRANT INSERT (tenant_id, reservation_id, product_id, pickup_branch_id,
              sku_snapshot, name_snapshot, quantity)
  ON pickup_reservation_lines TO pos_api_runtime;
-- Deferred reservation integrity triggers read the matching event row;
-- forced RLS limits runtime visibility to authorized reservation participants.
GRANT SELECT ON pickup_reservation_events TO pos_api_runtime;
GRANT INSERT (tenant_id, reservation_id, acting_branch_id, acting_station_id,
              actor_membership_id, event_type, revision, idempotency_key,
              request_hash, reason)
  ON pickup_reservation_events TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, product_id, stock_transfer_id,
              pickup_reservation_id, quantity, expires_at, released_at,
              release_reason, created_at)
  ON inventory_allocations TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, product_id, stock_transfer_id,
              pickup_reservation_id, quantity, expires_at)
  ON inventory_allocations TO pos_api_runtime;
GRANT UPDATE (released_at, release_reason)
  ON inventory_allocations TO pos_api_runtime;
GRANT SELECT (id, tenant_id, transfer_id, acting_branch_id, actor_membership_id,
              event_type, revision, idempotency_key, request_hash, details, occurred_at)
  ON stock_transfer_events TO pos_api_runtime;
GRANT INSERT (tenant_id, transfer_id, acting_branch_id, actor_membership_id,
              event_type, revision, idempotency_key, request_hash, details)
  ON stock_transfer_events TO pos_api_runtime;
GRANT SELECT (id, tenant_id, name, phone, email, notes, status,
              revision, created_by_membership_id, creation_idempotency_key,
              creation_request_hash, created_at, updated_at)
  ON hosted_customers TO pos_api_runtime;
GRANT INSERT (id, tenant_id, name, phone, email, notes, status,
              created_by_membership_id, creation_idempotency_key,
              creation_request_hash)
  ON hosted_customers TO pos_api_runtime;
GRANT UPDATE (name, phone, email, notes, status, revision, updated_at)
  ON hosted_customers TO pos_api_runtime;
GRANT SELECT (merged_into_customer_id) ON hosted_customers TO pos_api_runtime;
GRANT UPDATE (merged_into_customer_id) ON hosted_customers TO pos_api_runtime;
GRANT SELECT (tenant_id, source_customer_id, target_customer_id, branch_id,
              actor_membership_id, source_revision, target_revision,
              idempotency_key, request_hash, merged_at)
  ON hosted_customer_merges TO pos_api_runtime;
GRANT INSERT (tenant_id, source_customer_id, target_customer_id, branch_id,
              actor_membership_id, source_revision, target_revision,
              idempotency_key, request_hash)
  ON hosted_customer_merges TO pos_api_runtime;
GRANT SELECT (id, tenant_id, name, contact_person, email, phone, address, notes,
              status, revision, created_by_membership_id, creation_idempotency_key,
              creation_request_hash, created_at, updated_at)
  ON hosted_suppliers TO pos_api_runtime;
GRANT INSERT (id, tenant_id, name, contact_person, email, phone, address, notes,
              status, created_by_membership_id, creation_idempotency_key,
              creation_request_hash)
  ON hosted_suppliers TO pos_api_runtime;
GRANT UPDATE (name, contact_person, email, phone, address, notes, status, revision, updated_at)
  ON hosted_suppliers TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, supplier_id, order_number, reference, status,
              revision, created_by_membership_id, creation_idempotency_key,
              creation_request_hash, placed_at, cancelled_at, created_at, updated_at)
  ON hosted_purchase_orders TO pos_api_runtime;
GRANT INSERT (id, tenant_id, branch_id, supplier_id, order_number, reference, status, revision,
              created_by_membership_id, creation_idempotency_key, creation_request_hash)
  ON hosted_purchase_orders TO pos_api_runtime;
GRANT UPDATE (supplier_id, reference, status, revision, placed_at, cancelled_at, updated_at)
  ON hosted_purchase_orders TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, order_id, product_id, sku_snapshot, name_snapshot,
              ordered_quantity, received_quantity, expected_unit_cost_minor)
  ON hosted_purchase_order_lines TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, order_id, product_id, sku_snapshot, name_snapshot,
              ordered_quantity, expected_unit_cost_minor)
  ON hosted_purchase_order_lines TO pos_api_runtime;
GRANT UPDATE (received_quantity) ON hosted_purchase_order_lines TO pos_api_runtime;
GRANT DELETE ON hosted_purchase_order_lines TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, order_id, supplier_id, actor_membership_id,
              idempotency_key, request_hash, reference, total_minor, received_at)
  ON hosted_purchase_receipts TO pos_api_runtime;
GRANT INSERT (id, tenant_id, branch_id, order_id, supplier_id, actor_membership_id,
              idempotency_key, request_hash, reference, total_minor)
  ON hosted_purchase_receipts TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, receipt_id, order_line_id, product_id,
              quantity, actual_unit_cost_minor, line_total_minor)
  ON hosted_purchase_receipt_lines TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, receipt_id, order_line_id, product_id,
              quantity, actual_unit_cost_minor, line_total_minor)
  ON hosted_purchase_receipt_lines TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, supplier_id, supplier_reference,
              invoice_date, due_date, currency_code, total_minor, actor_membership_id,
              idempotency_key, request_hash, created_at)
  ON hosted_supplier_bills TO pos_api_runtime;
GRANT INSERT (id, tenant_id, branch_id, supplier_id, supplier_reference,
              invoice_date, due_date, currency_code, total_minor, actor_membership_id,
              idempotency_key, request_hash)
  ON hosted_supplier_bills TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, bill_id, receipt_id, amount_minor)
  ON hosted_supplier_bill_allocations TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, bill_id, receipt_id, amount_minor)
  ON hosted_supplier_bill_allocations TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, supplier_id, currency_code, payment_method,
              external_reference, amount_minor, actor_membership_id, idempotency_key,
              request_hash, paid_at, created_at)
  ON hosted_supplier_payments TO pos_api_runtime;
GRANT INSERT (id, tenant_id, branch_id, supplier_id, currency_code, payment_method,
              external_reference, amount_minor, actor_membership_id, idempotency_key,
              request_hash)
  ON hosted_supplier_payments TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, payment_id, bill_id, amount_minor, created_at)
  ON hosted_supplier_payment_allocations TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, payment_id, bill_id, amount_minor)
  ON hosted_supplier_payment_allocations TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, source_type, source_id, currency_code,
              effective_at, actor_membership_id, description, created_at)
  ON hosted_accounting_journals TO pos_api_runtime;
GRANT INSERT (id, tenant_id, branch_id, source_type, source_id, currency_code,
              effective_at, actor_membership_id, description)
  ON hosted_accounting_journals TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, journal_id, line_number, account_code,
              description, debit_minor, credit_minor)
  ON hosted_accounting_journal_lines TO pos_api_runtime;
GRANT INSERT (tenant_id, branch_id, journal_id, line_number, account_code,
              description, debit_minor, credit_minor)
  ON hosted_accounting_journal_lines TO pos_api_runtime;
GRANT SELECT (id, tenant_id, branch_id, country_code, provider_name, terminal_model,
              capability, business_impact, details, status, created_at)
  ON hosted_payment_integration_requests TO pos_api_runtime;
GRANT SELECT (tenant_id, created_by_membership_id, idempotency_key, request_hash)
  ON hosted_payment_integration_requests TO pos_api_runtime;
GRANT INSERT (id, tenant_id, branch_id, created_by_membership_id, country_code,
              provider_name, terminal_model, capability, details, status,
              business_impact,
              idempotency_key, request_hash)
  ON hosted_payment_integration_requests TO pos_api_runtime;
GRANT SELECT (tenant_id, entity_type, entity_id, content_type, byte_length,
              image_data, updated_by_membership_id, created_at, updated_at)
  ON hosted_entity_images TO pos_api_runtime;
GRANT INSERT (tenant_id, entity_type, entity_id, content_type, byte_length,
              image_data, created_by_membership_id, updated_by_membership_id)
  ON hosted_entity_images TO pos_api_runtime;
GRANT UPDATE (content_type, byte_length, image_data, updated_by_membership_id, updated_at)
  ON hosted_entity_images TO pos_api_runtime;
GRANT DELETE ON hosted_entity_images TO pos_api_runtime;
GRANT SELECT (id, tenant_id, customer_id, currency_code, credit_limit_minor, due_days,
              revision, last_reason, updated_by_membership_id, created_at, updated_at)
  ON hosted_customer_credit_terms TO pos_api_runtime;
GRANT INSERT (id, tenant_id, customer_id, currency_code, credit_limit_minor, due_days,
              revision, last_reason, updated_by_membership_id)
  ON hosted_customer_credit_terms TO pos_api_runtime;
GRANT UPDATE (credit_limit_minor, due_days, revision, last_reason,
              updated_by_membership_id, updated_at)
  ON hosted_customer_credit_terms TO pos_api_runtime;
GRANT SELECT (id, tenant_id, customer_id, currency_code, revision,
              previous_credit_limit_minor, previous_due_days, credit_limit_minor,
              due_days, reason, actor_membership_id, idempotency_key, request_hash, created_at)
  ON hosted_customer_credit_term_events TO pos_api_runtime;
GRANT INSERT (id, tenant_id, customer_id, currency_code, revision,
              previous_credit_limit_minor, previous_due_days, credit_limit_minor,
              due_days, reason, actor_membership_id, idempotency_key, request_hash)
  ON hosted_customer_credit_term_events TO pos_api_runtime;
GRANT USAGE ON SCHEMA pos_worker TO pos_api_runtime;
GRANT EXECUTE ON FUNCTION pos_worker.customer_credit_position(uuid) TO pos_api_runtime;
