CREATE TABLE IF NOT EXISTS payment_options (
    id TEXT PRIMARY KEY,
    store_id TEXT NOT NULL REFERENCES stores(id),
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('cash','card','mobile_money','other')),
    provider TEXT NOT NULL DEFAULT '',
    merchant_account TEXT NOT NULL DEFAULT '',
    terminal_id TEXT NOT NULL DEFAULT '',
    instructions TEXT NOT NULL DEFAULT '',
    reference_required INTEGER NOT NULL CHECK(reference_required IN (0,1)),
    enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
    UNIQUE(store_id,name)
);
CREATE TABLE IF NOT EXISTS sale_payment_details (
    sale_id TEXT PRIMARY KEY REFERENCES sales(id),
    option_id TEXT REFERENCES payment_options(id),
    label TEXT NOT NULL,
    provider TEXT NOT NULL,
    terminal_id TEXT NOT NULL,
    reference TEXT NOT NULL,
    confirmation_source TEXT NOT NULL CHECK(confirmation_source IN ('cashier_cash','manual_external'))
);
