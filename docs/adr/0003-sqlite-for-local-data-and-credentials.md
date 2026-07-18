# SQLite for local data and credentials

ZZZManager will use SQLite as the local store for account configuration, thresholds, balance check history, notification hooks, settings, and credentials in the first release. This keeps the desktop app simple and portable while the core workflow is being built; a future Secret Store abstraction can move sensitive values to the operating system credential store without changing the rest of the domain model.
