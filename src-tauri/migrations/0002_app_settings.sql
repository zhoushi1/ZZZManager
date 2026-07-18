-- Application-wide settings, stored as a single row (id = 1).
-- The first setting is the outbound HTTP proxy configuration; it applies to
-- provider balance checks now and future webhook delivery.
CREATE TABLE IF NOT EXISTS app_settings (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    -- Proxy mode: 'system' (default), 'none', or 'custom'.
    proxy_mode  TEXT NOT NULL DEFAULT 'system',
    -- Custom proxy URL; populated only when proxy_mode = 'custom'.
    proxy_url   TEXT,
    updated_at  TEXT NOT NULL
);

-- Ensure existing installs default to the system proxy. IGNORE keeps the row
-- untouched if it already exists.
INSERT OR IGNORE INTO app_settings (id, proxy_mode, proxy_url, updated_at)
VALUES (1, 'system', NULL, '1970-01-01T00:00:00Z');
