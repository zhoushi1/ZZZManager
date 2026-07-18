# Rust service layer with sqlx

ZZZManager will access SQLite through a Rust service layer using `sqlx` and database migrations. The React frontend will call Tauri commands instead of reading SQLite directly, because account credentials, provider adapters, webhook delivery, and scheduled balance checks belong on the Rust side and need a stable boundary for future schema upgrades.
