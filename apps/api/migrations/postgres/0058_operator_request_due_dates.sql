-- Operator-only scheduling for payment integration requests.
-- Due dates and scheduling history remain private to the support workspace.
ALTER TABLE hosted_payment_integration_requests
    ADD COLUMN due_at TIMESTAMPTZ;

ALTER TABLE operator_payment_request_events
    ADD COLUMN due_at TIMESTAMPTZ;

ALTER TABLE operator_payment_request_events
    DROP CONSTRAINT operator_payment_request_events_event_kind_check,
    ADD CONSTRAINT operator_payment_request_events_event_kind_check
        CHECK (event_kind IN ('status_changed','public_update','private_note','schedule_changed'));

GRANT SELECT (due_at) ON hosted_payment_integration_requests TO pos_operator_runtime;
GRANT UPDATE (due_at) ON hosted_payment_integration_requests TO pos_operator_runtime;
GRANT SELECT (due_at) ON operator_payment_request_events TO pos_operator_runtime;
GRANT INSERT (due_at) ON operator_payment_request_events TO pos_operator_runtime;
