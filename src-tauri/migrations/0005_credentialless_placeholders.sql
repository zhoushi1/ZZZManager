-- Allow credentialless imported account placeholders.
--
-- The default Configuration Export excludes credentials (see config_io.rs), but
-- should still be able to back up / migrate ordinary account *metadata*. Before
-- this migration `credential_fingerprint` was NOT NULL, so any account without
-- credentials could not be stored and the default export could not restore
-- account metadata at all.
--
-- We relax the schema so `credential_fingerprint` may be NULL, and split the
-- single identity index into two, each governing one kind of account:
--
--   * Credentialed accounts keep the original identity:
--       (provider, base_url, credential_fingerprint)
--     enforced only where a fingerprint is present. This is the same duplicate
--     rule used everywhere else, unchanged for existing data.
--
--   * Credentialless placeholders have no fingerprint to key on, so their
--     practical identity is (provider, base_url, name). Two imported placeholders
--     that share a provider + normalized base URL + name are the same account;
--     differing on any of those three makes them distinct. `name` is part of the
--     key precisely because it is the only remaining distinguishing metadata.
--
-- SQLite treats NULLs as distinct in unique indexes, so partial indexes are the
-- clean way to keep each rule scoped to its own account kind without collisions.
--
-- SQLite cannot drop a NOT NULL constraint in place, so the table is rebuilt.

PRAGMA foreign_keys = OFF;

CREATE TABLE gateway_accounts_new (
    id                     TEXT PRIMARY KEY NOT NULL,
    name                   TEXT NOT NULL,
    provider               TEXT NOT NULL,
    base_url               TEXT NOT NULL,
    enabled                INTEGER NOT NULL DEFAULT 1,
    balance_threshold      REAL,
    check_interval_minutes INTEGER,

    access_token           TEXT,
    user_id                TEXT,
    api_key                TEXT,

    -- Duplicate detection for credentialed accounts. NULL for credentialless
    -- imported placeholders, whose identity is (provider, base_url, name).
    credential_fingerprint TEXT,

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

INSERT INTO gateway_accounts_new
    SELECT id, name, provider, base_url, enabled, balance_threshold,
           check_interval_minutes, access_token, user_id, api_key,
           credential_fingerprint, last_result, last_remaining, last_used,
           last_total, last_unit, last_plan_name, last_message, last_checked_at,
           created_at, updated_at
    FROM gateway_accounts;

DROP TABLE gateway_accounts;
ALTER TABLE gateway_accounts_new RENAME TO gateway_accounts;

-- Credentialed identity: only rows that actually carry a fingerprint.
CREATE UNIQUE INDEX idx_gateway_accounts_identity
    ON gateway_accounts (provider, base_url, credential_fingerprint)
    WHERE credential_fingerprint IS NOT NULL;

-- Credentialless placeholder identity: provider + base_url + name.
CREATE UNIQUE INDEX idx_gateway_accounts_placeholder_identity
    ON gateway_accounts (provider, base_url, name)
    WHERE credential_fingerprint IS NULL;

CREATE INDEX IF NOT EXISTS idx_balance_check_history_account
    ON balance_check_history (account_id, checked_at DESC);

PRAGMA foreign_keys = ON;
