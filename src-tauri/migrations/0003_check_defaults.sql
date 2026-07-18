-- Global check defaults added to the single app_settings row (id = 1).
-- These back the Settings page globals and the runtime balance-check scheduler:
--   * default_balance_threshold    - fallback threshold when an account has none
--   * default_check_interval_minutes - fallback schedule cadence
--   * history_retention_days       - how long balance_check_history is kept
--   * user_agent                   - User-Agent header for outbound checks
-- The existing proxy columns and their row are preserved untouched.
ALTER TABLE app_settings
    ADD COLUMN default_balance_threshold REAL NOT NULL DEFAULT 20.0;
ALTER TABLE app_settings
    ADD COLUMN default_check_interval_minutes INTEGER NOT NULL DEFAULT 30;
ALTER TABLE app_settings
    ADD COLUMN history_retention_days INTEGER NOT NULL DEFAULT 30;
ALTER TABLE app_settings
    ADD COLUMN user_agent TEXT NOT NULL DEFAULT 'ai-gateway-manager/0.1';
