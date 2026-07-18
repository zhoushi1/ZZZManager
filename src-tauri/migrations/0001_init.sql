-- Gateway accounts: configuration + credentials + latest check snapshot.
-- Credentials are stored in SQLite for the first release, per ADR-0003.
CREATE TABLE IF NOT EXISTS gateway_accounts (
    id                     TEXT PRIMARY KEY NOT NULL,
    name                   TEXT NOT NULL,
    provider               TEXT NOT NULL,
    base_url               TEXT NOT NULL,
    enabled                INTEGER NOT NULL DEFAULT 1,
    balance_threshold      REAL,
    check_interval_minutes INTEGER,

    -- Credentials (nullable; which columns are populated depends on provider).
    access_token           TEXT,
    user_id                TEXT,
    api_key                TEXT,

    -- Duplicate detection: provider + normalized base_url + credential fingerprint.
    credential_fingerprint TEXT NOT NULL,

    -- Latest balance check snapshot (nullable until first check).
    last_result            TEXT,
    last_remaining         REAL,
    last_used              REAL,
    last_total             REAL,
    last_unit              TEXT,
    last_plan_name         TEXT,
    last_message           TEXT,
    last_checked_at        TEXT,

    created_at             TEXT NOT NULL,
    updated_at             TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_gateway_accounts_identity
    ON gateway_accounts (provider, base_url, credential_fingerprint);

-- Balance check history: one row per check.
CREATE TABLE IF NOT EXISTS balance_check_history (
    id           TEXT PRIMARY KEY NOT NULL,
    account_id   TEXT NOT NULL,
    provider     TEXT NOT NULL,
    result       TEXT NOT NULL,
    remaining    REAL,
    used         REAL,
    total        REAL,
    unit         TEXT,
    plan_name    TEXT,
    message      TEXT,
    notified     INTEGER NOT NULL DEFAULT 0,
    checked_at   TEXT NOT NULL,
    FOREIGN KEY (account_id) REFERENCES gateway_accounts (id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_balance_check_history_account
    ON balance_check_history (account_id, checked_at DESC);
