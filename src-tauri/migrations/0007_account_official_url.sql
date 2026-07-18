-- Add an optional official website link per account.
--
-- Purely informational: a validated http(s) URL the App User can open in their
-- default browser from the account list. NULL when unset.
ALTER TABLE gateway_accounts
    ADD COLUMN official_url TEXT;
