# ZZZManager

ZZZManager monitors account balances, thresholds, and notification behavior across AI gateway platforms.

## Language

**Gateway Account**:
An account or API key configuration on an AI gateway platform that this app manages. It may represent a New API account, a Sub2API account, or another gateway provider, and can include credentials such as username/password or API keys. The first release rejects duplicate accounts by provider, normalized base URL, and credential fingerprint.
_Avoid_: User account, login user, app account

**App User**:
The person using the local desktop app on the current machine. The first release does not model app login, teams, roles, or multi-user sharing.
_Avoid_: Tenant, workspace user

**Gateway Account State**:
Whether a Gateway Account is enabled or disabled. Enabled accounts participate in scheduled Balance Checks; disabled accounts are skipped by the schedule but can still be checked manually.
_Avoid_: Health, check status

**Balance Check Result**:
The outcome of the latest Balance Check, such as unchecked, healthy, low balance, failed, or invalid credentials. This is separate from Gateway Account State. Results include a unit; built-in New API and Sub2API adapters return USD, and the app does not convert currencies in the first release.
_Avoid_: Account status

**Provider**:
A gateway platform type with its own balance query contract, credential requirements, and response shape. New API and Sub2API are providers.
_Avoid_: Site type, platform kind

**Balance Check**:
A request made against a Gateway Account's provider endpoint to determine whether the credential is valid and how much balance or quota remains.
_Avoid_: Sync, refresh

**Provider Adapter**:
The provider-specific balance check definition that knows which endpoint, headers, and response fields to use for a Provider. Provider adapters receive a normalized base URL without a trailing slash and append their own fixed paths.
_Avoid_: Parser, crawler

**Credential Profile**:
The set of credential fields required by a Provider for Balance Checks. New API requires base URL, access token, and user ID; Sub2API requires base URL and API key.
_Avoid_: Login form, auth schema

**Configuration Export**:
A JSON export of app configuration for backup or migration. The first release excludes credentials by default, and only includes them when the App User explicitly chooses to export sensitive values.
_Avoid_: Backup, dump

**Credentialless Placeholder**:
A gateway account imported without credentials, so a default (credentials-excluded) export can still restore account metadata. It is always created disabled and is skipped by scheduled and manual balance checks until the App User adds credentials via the edit form. Its duplicate identity is provider + normalized base URL + name (a credentialed account instead keys on the credential fingerprint).
_Avoid_: Stub account, empty account

**Proxy Configuration**:
The app's network proxy setting for outbound HTTP requests. The App User can choose to use no proxy, the system proxy, or a custom proxy URL, and the setting applies to Provider Balance Checks and Notification Hook delivery. The default mode is system proxy; custom proxy URLs support HTTP, HTTPS, and SOCKS5 schemes.
_Avoid_: Network mode, tunnel

**Balance Threshold**:
A minimum remaining value that determines when a Gateway Account needs attention. It uses CNY purchase value when the account has a Credit Conversion Rate, otherwise it uses the Provider's reported unit; the global default applies when an account has no override.
_Avoid_: Alert value, warning line

**Credit Conversion Rate**:
An optional per-account ratio describing how many provider-reported USD credits are received for one CNY paid. It converts USD credits to CNY purchase value for display, Balance Threshold evaluation, and Notification Events while preserving raw Provider values in history.
_Avoid_: Exchange rate, currency rate

**Notification Hook**:
A user-configured HTTP endpoint that receives POST JSON when a Gateway Account needs attention.
_Avoid_: Bot, channel

**Notification Event**:
An attention-worthy account state such as low balance, repeated balance check failures, or invalid credentials. Disabled Gateway Accounts do not emit Notification Events from manual checks.
_Avoid_: Alarm, message

**Webhook Payload**:
The fixed JSON body sent to a Notification Hook for a Notification Event. The first release supports `low_balance`, `check_failed`, and `invalid_credential` event types and does not support user-defined message templates.
_Avoid_: Template, bot message

**Notification Cooldown**:
The period during which the same Gateway Account should not emit repeated Notification Events of the same kind. The first release uses a six-hour cooldown, and repeated check failures only emit an event after three consecutive failures.
_Avoid_: Silence period, suppression

**Balance Check History**:
The retained record of past Balance Checks, including the checked account, provider, timestamp, result values, errors, and whether a Notification Event was emitted. The default retention window is 30 days and can be changed in settings.
_Avoid_: Log, audit trail

**Check Schedule**:
The cadence for automatic Balance Checks while the app is running. The global default interval is 30 minutes, each Gateway Account may override it, and manual checks are allowed outside the schedule.
_Avoid_: Cron, timer
