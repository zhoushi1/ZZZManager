-- Rename only the legacy application default. User-customized values remain unchanged.
UPDATE app_settings
SET user_agent = 'zzz-balance-monitor/0.1'
WHERE user_agent = 'ai-gateway-manager/0.1';
