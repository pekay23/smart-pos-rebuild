-- Allow destination managers to post multiple physical receipt batches and
-- explicitly close a short shipment after investigating the difference.
ALTER TABLE stock_transfers
    DROP CONSTRAINT stock_transfers_status_check,
    DROP CONSTRAINT stock_transfers_check1;

ALTER TABLE stock_transfers
    ADD CONSTRAINT stock_transfers_status_check
        CHECK (status IN ('requested','accepted','rejected','in_transit','partially_received','received','discrepancy','discrepancy_resolved','cancelled')),
    ADD CONSTRAINT stock_transfers_state_timestamps_check CHECK (
        (status = 'requested' AND accepted_at IS NULL AND rejected_at IS NULL AND dispatched_at IS NULL AND received_at IS NULL AND cancelled_at IS NULL)
        OR (status = 'accepted' AND accepted_at IS NOT NULL AND rejected_at IS NULL AND dispatched_at IS NULL AND received_at IS NULL AND cancelled_at IS NULL)
        OR (status = 'rejected' AND rejected_at IS NOT NULL AND dispatched_at IS NULL AND received_at IS NULL AND cancelled_at IS NULL)
        OR (status = 'in_transit' AND accepted_at IS NOT NULL AND dispatched_at IS NOT NULL AND received_at IS NULL AND cancelled_at IS NULL)
        OR (status = 'partially_received' AND accepted_at IS NOT NULL AND dispatched_at IS NOT NULL AND received_at IS NULL AND cancelled_at IS NULL)
        OR (status IN ('received','discrepancy','discrepancy_resolved') AND accepted_at IS NOT NULL AND dispatched_at IS NOT NULL AND received_at IS NOT NULL AND cancelled_at IS NULL)
        OR (status = 'cancelled' AND dispatched_at IS NULL AND received_at IS NULL AND cancelled_at IS NOT NULL)
    );

ALTER TABLE stock_transfer_lines
    ADD CONSTRAINT stock_transfer_lines_received_not_over_dispatched
        CHECK (received_quantity IS NULL OR received_quantity <= dispatched_quantity);

ALTER TABLE stock_transfer_events
    DROP CONSTRAINT stock_transfer_events_event_type_check,
    ADD CONSTRAINT stock_transfer_events_event_type_check
        CHECK (event_type IN ('requested','accepted','rejected','dispatched','partially_received','received','discrepancy','discrepancy_resolved','cancelled'));
