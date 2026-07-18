-- Add structured Custom HTTP Adapter configuration per account.
--
-- Built-in providers leave this column NULL. `custom_http` accounts store a
-- validated JSON object that describes the request shape and response extractors;
-- raw JavaScript is intentionally not supported.
ALTER TABLE gateway_accounts
    ADD COLUMN custom_adapter_config TEXT;
