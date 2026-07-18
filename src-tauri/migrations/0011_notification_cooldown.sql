-- Add configurable notification cooldown setting to app_settings.
-- Default is 60 minutes (down from the hardcoded 6 hours).
ALTER TABLE app_settings ADD COLUMN notification_cooldown_minutes INTEGER NOT NULL DEFAULT 60;
