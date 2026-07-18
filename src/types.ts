export type Provider = "new_api" | "sub2api" | "custom_http";

export type CustomHttpMethod = "GET" | "POST";

export interface CustomHttpHeader {
  name: string;
  value: string;
}

/** Structured Custom HTTP Adapter config. Values may use {{apiKey}},
 *  {{baseUrl}}, and {{userAgent}} templates. */
export interface CustomHttpAdapterConfig {
  method: CustomHttpMethod;
  path: string;
  headers: CustomHttpHeader[];
  body?: string | null;
  validPath?: string | null;
  validEquals?: unknown;
  remainingPath: string;
  usedPath?: string | null;
  totalPath?: string | null;
  unitPath?: string | null;
  planNamePath?: string | null;
  messagePath?: string | null;
  numericDivisor?: number | null;
  defaultUnit?: string | null;
}

export type BalanceResult =
  | "healthy"
  | "low_balance"
  | "failed"
  | "invalid_credential";

/** Account view as returned by the backend. Never contains raw credentials. */
export interface AccountView {
  id: string;
  name: string;
  provider: Provider;
  baseUrl: string;
  enabled: boolean;
  balanceThreshold: number | null;
  /** Provider-reported USD credits received for one CNY paid. */
  usdCreditsPerCny: number | null;
  checkIntervalMinutes: number | null;
  /** Optional official website link (http(s) URL), or null when unset. */
  officialUrl: string | null;
  customAdapter: CustomHttpAdapterConfig | null;
  hasCredentials: boolean;
  lastResult: BalanceResult | null;
  lastRemaining: number | null;
  lastUsed: number | null;
  lastTotal: number | null;
  lastUnit: string | null;
  lastPlanName: string | null;
  lastMessage: string | null;
  lastCheckedAt: string | null;
  note: string | null;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

export interface HistoryEntry {
  id: string;
  accountId: string;
  /** Joined from the owning account; null if the account no longer exists. */
  accountName: string | null;
  /** Current account conversion rate; history amounts remain raw. */
  usdCreditsPerCny: number | null;
  provider: Provider;
  result: BalanceResult;
  remaining: number | null;
  used: number | null;
  total: number | null;
  unit: string | null;
  planName: string | null;
  message: string | null;
  checkedAt: string;
}

/** Filters for querying balance check history. All fields are optional. */
export interface HistoryQuery {
  accountId?: string | null;
  result?: BalanceResult | null;
  provider?: Provider | null;
  limit?: number | null;
}

/** A per-unit sum of remaining balance across accounts. */
export interface RemainingByUnit {
  unit: string;
  total: number;
  accountCount: number;
}

/** Dashboard overview computed from current DB state. Never contains credentials. */
export interface Overview {
  totalAccounts: number;
  enabledAccounts: number;
  uncheckedAccounts: number;
  lowBalanceCount: number;
  failedCount: number;
  invalidCredentialCount: number;
  remainingByUnit: RemainingByUnit[];
  recentChecks: HistoryEntry[];
  recentDeliveries: DeliveryView[];
}

/** Why an account is or is not part of the scheduled check rotation. Mirrors the
 *  scheduler's eligibility rules. */
export type ScheduleStatus =
  | "due"
  | "scheduled"
  | "missing_credentials"
  | "disabled";

/** One account's row on the Schedules page. Derived from stored state; never
 *  contains credentials. */
export interface ScheduleRow {
  id: string;
  name: string;
  provider: Provider;
  enabled: boolean;
  hasCredentials: boolean;
  /** The per-account interval override in minutes, if any (raw stored value). */
  checkIntervalMinutes: number | null;
  /** The interval actually used: the override when valid, else the default. */
  effectiveIntervalMinutes: number;
  lastCheckedAt: string | null;
  /** When the next scheduled check is expected; null for accounts the scheduler
   *  never checks (disabled or missing credentials). Equals "now" when due. */
  nextCheckAt: string | null;
  dueNow: boolean;
  status: ScheduleStatus;
}

/** The Schedules page snapshot: default cadence, counts, and per-account rows. */
export interface ScheduleOverview {
  defaultCheckIntervalMinutes: number;
  /** Global automatic-scheduling master switch. */
  schedulerEnabled: boolean;
  totalAccounts: number;
  enabledAccounts: number;
  disabledAccounts: number;
  schedulableAccounts: number;
  unschedulableAccounts: number;
  rows: ScheduleRow[];
}

export const SCHEDULE_STATUS_LABELS: Record<ScheduleStatus, string> = {
  due: "Due now",
  scheduled: "Scheduled",
  missing_credentials: "No credentials",
  disabled: "Disabled",
};

export interface CredentialsInput {
  accessToken?: string;
  userId?: string;
  apiKey?: string;
}

/** Account credentials view returned by backend, includes sensitive data. */
export interface AccountCredentialsView {
  provider: Provider;
  accessToken: string | null;
  userId: string | null;
  apiKey: string | null;
}

export interface CreateAccountInput {
  name: string;
  provider: Provider;
  baseUrl: string;
  enabled: boolean;
  balanceThreshold: number | null;
  usdCreditsPerCny: number | null;
  checkIntervalMinutes: number | null;
  officialUrl?: string | null;
  credentials: CredentialsInput;
  customAdapter?: CustomHttpAdapterConfig | null;
  note?: string | null;
  sortOrder?: number | null;
}

export interface UpdateAccountInput {
  name: string;
  baseUrl: string;
  enabled: boolean;
  balanceThreshold: number | null;
  usdCreditsPerCny: number | null;
  checkIntervalMinutes: number | null;
  officialUrl?: string | null;
  credentials: CredentialsInput;
  customAdapter?: CustomHttpAdapterConfig | null;
  note?: string | null;
  sortOrder?: number | null;
}

export const PROVIDER_LABELS: Record<Provider, string> = {
  new_api: "New API",
  sub2api: "Sub2API",
  custom_http: "Custom HTTP",
};

export type ProxyMode = "system" | "none" | "custom";

/** Outbound HTTP proxy configuration. */
export interface ProxySettings {
  mode: ProxyMode;
  /** Present only when `mode` is `custom`. */
  customUrl: string | null;
}

export interface UpdateProxySettingsInput {
  mode: ProxyMode;
  customUrl?: string | null;
}

/** Global check defaults, backing the Settings page globals and the scheduler. */
export interface AppSettings {
  defaultBalanceThreshold: number;
  defaultCheckIntervalMinutes: number;
  historyRetentionDays: number;
  userAgent: string;
  notificationCooldownMinutes: number;
  /** Global automatic-scheduling master switch. When false the runtime
   *  scheduler performs no automatic balance checks; manual checks and the
   *  per-account `enabled` flag are unaffected. */
  schedulerEnabled: boolean;
}

/** Payload for the Settings-page globals save. Intentionally excludes
 *  `schedulerEnabled`: the master switch is toggled from the accounts header via
 *  `setSchedulerEnabled`, so saving globals never resets it. */
export type UpdateAppSettingsInput = Omit<AppSettings, "schedulerEnabled">;

export const PROXY_MODE_LABELS: Record<ProxyMode, string> = {
  system: "System proxy",
  none: "No proxy",
  custom: "Custom proxy",
};

export type NotificationEventType =
  | "low_balance"
  | "check_failed"
  | "invalid_credential";

export type NotificationHookType = "generic" | "feishu" | "wecom" | "dingtalk";

/** A notification hook: a generic HTTP endpoint that receives webhook POSTs. */
export interface HookView {
  id: string;
  name: string;
  url: string;
  enabled: boolean;
  hookType: NotificationHookType;
  createdAt: string;
  updatedAt: string;
}

export interface CreateHookInput {
  name: string;
  url: string;
  enabled: boolean;
  hookType: NotificationHookType;
}

export interface UpdateHookInput {
  name: string;
  url: string;
  enabled: boolean;
  hookType: NotificationHookType;
}

/** A recorded notification delivery attempt (one row per event + hook). */
export interface DeliveryView {
  id: string;
  accountId: string;
  eventType: NotificationEventType;
  hookId: string | null;
  success: boolean;
  responseStatus: number | null;
  errorMessage: string | null;
  createdAt: string;
}

export const EVENT_TYPE_LABELS: Record<NotificationEventType, string> = {
  low_balance: "Low balance",
  check_failed: "Check failed",
  invalid_credential: "Invalid credential",
};

export const RESULT_LABELS: Record<BalanceResult, string> = {
  healthy: "Healthy",
  low_balance: "Low balance",
  failed: "Check failed",
  invalid_credential: "Invalid credential",
};

// --- About / update check -------------------------------------------------

/** Static app identity for the Settings "About" card. Version comes from the
 *  build, never hardcoded in the frontend. */
export interface AppInfo {
  name: string;
  author: string;
  version: string;
  githubUrl: string;
}

/** Result of comparing the running version against the latest GitHub release. */
export interface UpdateCheckResult {
  currentVersion: string;
  latestVersion: string;
  updateAvailable: boolean;
  releaseUrl: string;
  releaseName: string | null;
  publishedAt: string | null;
  /** RFC 3339 timestamp of when the check ran. */
  checkedAt: string;
}

// --- Configuration export / import ---------------------------------------

/** Global defaults + proxy settings bundled into an export. */
export interface ExportedSettings {
  defaultBalanceThreshold: number;
  defaultCheckIntervalMinutes: number;
  historyRetentionDays: number;
  userAgent: string;
  notificationCooldownMinutes?: number;
  /** Global automatic-scheduling switch. Older exports without it default to
   *  true on the backend. */
  schedulerEnabled?: boolean;
  proxyMode: ProxyMode;
  proxyUrl?: string | null;
}

/** Credentials as they appear inside an export. Present only when the export
 *  was produced with credentials included. */
export interface ExportedCredentials {
  accessToken?: string | null;
  userId?: string | null;
  apiKey?: string | null;
}

export interface ExportedAccount {
  name: string;
  provider: Provider;
  baseUrl: string;
  enabled: boolean;
  balanceThreshold: number | null;
  usdCreditsPerCny?: number | null;
  checkIntervalMinutes: number | null;
  officialUrl?: string | null;
  credentials?: ExportedCredentials | null;
  customAdapter?: CustomHttpAdapterConfig | null;
  note?: string | null;
  sortOrder?: number;
}

export interface ExportedHook {
  name: string;
  url: string;
  enabled: boolean;
  hookType?: NotificationHookType;
}

/** The full configuration export/import document. Never contains history. */
export interface ExportedConfig {
  formatVersion: number;
  exportedAt: string;
  includesCredentials: boolean;
  settings: ExportedSettings;
  accounts: ExportedAccount[];
  hooks: ExportedHook[];
}

/** Options controlling an import; both are opt-in for safety. */
export interface ImportOptions {
  importCredentials: boolean;
  overwriteExisting: boolean;
}

/** Readable summary of an import. */
export interface ImportReport {
  accountsCreated: number;
  accountsUpdated: number;
  accountsSkipped: number;
  hooksCreated: number;
  hooksUpdated: number;
  /** 1 when settings were updated, 0 otherwise. */
  settingsUpdated: number;
  warnings: string[];
}
