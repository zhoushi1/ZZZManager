# Provider adapter strategy

ZZZManager will ship New API and Sub2API as built-in provider adapters, because their balance check contracts are known and can be tested with typed Rust code. The app will also reserve a structured Custom HTTP Adapter for URL, method, headers, and response field extraction, but it will not execute arbitrary JavaScript extractors in the first release because that would add sandboxing, error handling, and security complexity to the desktop app.
