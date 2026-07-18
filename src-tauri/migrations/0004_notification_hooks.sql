-- Notification hooks and delivery/event history.
--
-- A notification hook is a user-configured generic HTTP endpoint that receives
-- a fixed JSON POST when a Gateway Account emits a Notification Event
-- (low_balance, check_failed, invalid_credential). The first release supports
-- generic webhook POST only; no per-platform templates and no custom payloads.
CREATE TABLE IF NOT EXISTS notification_hooks (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    url         TEXT NOT NULL,
    enabled     INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

-- Notification delivery/event history: one row per (event, hook) delivery
-- attempt. These rows are the source of truth for the Notification Cooldown:
-- the 6h per (account_id, event_type) window is enforced against ALL rows
-- here, whether the HTTP delivery succeeded or failed, so a failing webhook
-- does not defeat the cooldown and cause repeated attempts every tick.
--
-- `hook_id` is nullable and not a foreign key: keeping delivery history after a
-- hook is deleted preserves the cooldown/audit trail. The account foreign key
-- cascades so deleting an account clears its notification history.
CREATE TABLE IF NOT EXISTS notification_deliveries (
    id              TEXT PRIMARY KEY NOT NULL,
    account_id      TEXT NOT NULL,
    event_type      TEXT NOT NULL,
    hook_id         TEXT,
    payload         TEXT NOT NULL,
    success         INTEGER NOT NULL DEFAULT 0,
    response_status INTEGER,
    error_message   TEXT,
    created_at      TEXT NOT NULL,
    FOREIGN KEY (account_id) REFERENCES gateway_accounts (id) ON DELETE CASCADE
);

-- Cooldown lookup: "most recent delivery for this account + event type".
CREATE INDEX IF NOT EXISTS idx_notification_deliveries_cooldown
    ON notification_deliveries (account_id, event_type, created_at DESC);
