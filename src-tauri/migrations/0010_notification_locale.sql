-- Add notification/UI locale setting to app_settings.
-- Supported values: 'zh-CN' (default) and 'en-US'.
ALTER TABLE app_settings ADD COLUMN notification_locale TEXT NOT NULL DEFAULT 'zh-CN';
