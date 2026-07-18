-- Optional provider credit conversion: how many reported USD credits are
-- received for one CNY paid (for example, 12 means $120 is worth CNY 10).
ALTER TABLE gateway_accounts
    ADD COLUMN usd_credits_per_cny REAL;
