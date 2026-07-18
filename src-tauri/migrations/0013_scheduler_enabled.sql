-- Global automatic-scheduling switch on the single app_settings row (id = 1).
--
-- When 0, the runtime balance-check scheduler skips all automatic account
-- balance checks (and their notifications); manual checks are unaffected. The
-- per-account `enabled` flag still gates account-level participation while this
-- master switch is on. Defaults to 1 so existing installs keep auto-checking.
ALTER TABLE app_settings
    ADD COLUMN scheduler_enabled INTEGER NOT NULL DEFAULT 1;
