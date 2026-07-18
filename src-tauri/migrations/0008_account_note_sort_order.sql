-- Add note and sort_order columns to gateway_accounts
ALTER TABLE gateway_accounts ADD COLUMN note TEXT;
ALTER TABLE gateway_accounts ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;

-- Create index for efficient sorting by sort_order DESC, created_at DESC
CREATE INDEX IF NOT EXISTS idx_accounts_sort_order ON gateway_accounts (sort_order DESC, created_at DESC);
