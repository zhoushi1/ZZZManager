-- Add explicit hook_type column to notification_hooks table
ALTER TABLE notification_hooks ADD COLUMN hook_type TEXT NOT NULL DEFAULT 'generic';

-- Migrate existing data based on URL patterns
UPDATE notification_hooks
SET hook_type = CASE
    WHEN url LIKE '%open.feishu.cn%' OR url LIKE '%open.larksuite.com%' THEN 'feishu'
    WHEN url LIKE '%qyapi.weixin.qq.com%' THEN 'wecom'
    WHEN url LIKE '%oapi.dingtalk.com%' THEN 'dingtalk'
    ELSE 'generic'
END;
