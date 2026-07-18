import { invoke } from "@tauri-apps/api/core";
import type {
  AccountCredentialsView,
  AccountView,
  AppInfo,
  AppSettings,
  CreateAccountInput,
  CreateHookInput,
  DeliveryView,
  ExportedConfig,
  HistoryEntry,
  HistoryQuery,
  HookView,
  ImportOptions,
  ImportReport,
  Overview,
  ProxySettings,
  ScheduleOverview,
  UpdateAccountInput,
  UpdateAppSettingsInput,
  UpdateCheckResult,
  UpdateHookInput,
  UpdateProxySettingsInput,
} from "../types";

export function listAccounts(): Promise<AccountView[]> {
  return invoke<AccountView[]>("list_accounts");
}

export function createAccount(input: CreateAccountInput): Promise<AccountView> {
  return invoke<AccountView>("create_account", { input });
}

export function updateAccount(
  id: string,
  input: UpdateAccountInput,
): Promise<AccountView> {
  return invoke<AccountView>("update_account", { id, input });
}

export function deleteAccount(id: string): Promise<void> {
  return invoke<void>("delete_account", { id });
}

export function getAccountCredentials(
  id: string,
): Promise<AccountCredentialsView> {
  return invoke<AccountCredentialsView>("get_account_credentials", { id });
}

export function reorderAccounts(
  orderedIds: string[],
): Promise<AccountView[]> {
  return invoke<AccountView[]>("reorder_accounts", { orderedIds });
}

export function checkAccount(id: string): Promise<AccountView> {
  return invoke<AccountView>("check_account", { id });
}

export function recentChecks(limit?: number): Promise<HistoryEntry[]> {
  return invoke<HistoryEntry[]>("recent_checks", { limit });
}

export function queryHistory(query: HistoryQuery): Promise<HistoryEntry[]> {
  return invoke<HistoryEntry[]>("query_history", { query });
}

export function getOverview(): Promise<Overview> {
  return invoke<Overview>("get_overview");
}

export function getScheduleOverview(): Promise<ScheduleOverview> {
  return invoke<ScheduleOverview>("get_schedule_overview");
}

export function getProxySettings(): Promise<ProxySettings> {
  return invoke<ProxySettings>("get_proxy_settings");
}

export function updateProxySettings(
  input: UpdateProxySettingsInput,
): Promise<ProxySettings> {
  return invoke<ProxySettings>("update_proxy_settings", { input });
}

export function getAppSettings(): Promise<AppSettings> {
  return invoke<AppSettings>("get_app_settings");
}

export function updateAppSettings(
  input: UpdateAppSettingsInput,
): Promise<AppSettings> {
  return invoke<AppSettings>("update_app_settings", { input });
}

/**
 * Toggle the global automatic-scheduling master switch. Returns the updated
 * settings. When disabled, the runtime scheduler performs no automatic balance
 * checks; manual checks and per-account participation are unaffected.
 */
export function setSchedulerEnabled(enabled: boolean): Promise<AppSettings> {
  return invoke<AppSettings>("set_scheduler_enabled", { enabled });
}

export function listHooks(): Promise<HookView[]> {
  return invoke<HookView[]>("list_hooks");
}

export function createHook(input: CreateHookInput): Promise<HookView> {
  return invoke<HookView>("create_hook", { input });
}

export function updateHook(
  id: string,
  input: UpdateHookInput,
): Promise<HookView> {
  return invoke<HookView>("update_hook", { id, input });
}

export function deleteHook(id: string): Promise<void> {
  return invoke<void>("delete_hook", { id });
}

export function testHook(id: string): Promise<boolean> {
  return invoke<boolean>("test_hook", { id });
}

export function recentDeliveries(limit?: number): Promise<DeliveryView[]> {
  return invoke<DeliveryView[]>("recent_deliveries", { limit });
}

export function exportConfig(
  includeCredentials: boolean,
): Promise<ExportedConfig> {
  return invoke<ExportedConfig>("export_config", { includeCredentials });
}

/**
 * Export the configuration through the native save dialog and write it to the
 * chosen file. Resolves with the written path, or `null` when the user cancels
 * the dialog.
 */
export function exportConfigToFile(
  includeCredentials: boolean,
): Promise<string | null> {
  return invoke<string | null>("export_config_to_file", { includeCredentials });
}

export function importConfig(
  input: ExportedConfig,
  options: ImportOptions,
): Promise<ImportReport> {
  return invoke<ImportReport>("import_config", { input, options });
}

export function setAccountEnabled(
  id: string,
  enabled: boolean,
): Promise<AccountView> {
  return invoke<AccountView>("set_account_enabled", { id, enabled });
}

/** Static app identity (name, running version, GitHub URL) for the About card. */
export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("get_app_info");
}

/** Check GitHub for a newer release than the running version. Rejects on
 *  network/API failure so the caller can surface the error. */
export function checkForUpdate(): Promise<UpdateCheckResult> {
  return invoke<UpdateCheckResult>("check_for_update");
}

/** Whether the app is registered to launch on login. Reflects the real
 *  OS-level state, not a stored value. */
export function getAutostartEnabled(): Promise<boolean> {
  return invoke<boolean>("get_autostart_enabled");
}

/** Enable or disable launch-on-login. Resolves with the re-queried state so the
 *  caller can reconcile its UI with what the OS actually recorded. */
export function setAutostartEnabled(enabled: boolean): Promise<boolean> {
  return invoke<boolean>("set_autostart_enabled", { enabled });
}
