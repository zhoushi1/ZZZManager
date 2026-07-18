-- Normalize account sort_order to consecutive integers based on current ordering
-- Orders by: sort_order DESC, created_at DESC
-- Result: First account gets N, second gets N-1, ..., last gets 1

WITH ordered_accounts AS (
    SELECT
        id,
        ROW_NUMBER() OVER (ORDER BY sort_order DESC, created_at DESC) as new_sort_order
    FROM gateway_accounts
)
UPDATE gateway_accounts
SET sort_order = (
    SELECT new_sort_order
    FROM ordered_accounts
    WHERE ordered_accounts.id = gateway_accounts.id
);
