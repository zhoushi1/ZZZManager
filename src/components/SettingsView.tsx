import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Code2, RefreshCw, ScrollText } from "lucide-react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "./ui/card";
import { Alert, AlertDescription } from "./ui/alert";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Input, Label } from "./ui/input";
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldTitle,
} from "./ui/field";
import { Select } from "./ui/select";
import { Separator } from "./ui/separator";
import { Skeleton } from "./ui/skeleton";
import { Switch } from "./ui/switch";
import * as api from "../lib/api";
import { APP_NAME, APP_PACKAGE_NAME, APP_VERSION } from "../lib/app-metadata";
import type { AppInfo, ImportReport, ProxyMode, UpdateCheckResult } from "../types";
import { useI18n, type Locale } from "../lib/i18n";
import { useTheme, type ThemePreference } from "../lib/theme";
import { cn } from "../lib/utils";
import appLogo from "../../src-tauri/icons/128x128.png";

const PROXY_MODES: ProxyMode[] = ["system", "none", "custom"];

interface SettingsViewProps {
  /**
   * Called after a successful import so the parent can refresh cached state
   * (e.g. the accounts list). May be async; failures are surfaced to the user.
   */
  onImportComplete?: () => Promise<void> | void;
  onUpdateAvailable?: (update: UpdateCheckResult) => void;
}

export function SettingsView({
  onImportComplete,
  onUpdateAvailable,
}: SettingsViewProps = {}) {
  return (
    <div className="min-w-0 flex-1 overflow-auto p-5 sm:p-7">
      <div className="mx-auto grid w-full max-w-[1500px] grid-cols-1 items-start gap-5 xl:grid-cols-2">
        <div className="flex min-w-0 flex-col gap-5">
          <GeneralSettingsCard />
          <ProxyCard />
        </div>
        <div className="flex min-w-0 flex-col gap-5">
          <CheckDefaultsCard />
          <ImportExportCard onImportComplete={onImportComplete} />
        </div>
        <AboutCard onUpdateAvailable={onUpdateAvailable} />
      </div>
    </div>
  );
}

function GeneralSettingsCard() {
  const { t, locale, setLocale } = useI18n();
  const { theme, setTheme } = useTheme();
  const [enabled, setEnabled] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    api
      .getAutostartEnabled()
      .then((value) => {
        if (!active) return;
        setEnabled(value);
        setError(null);
      })
      .catch((err) => {
        if (active) {
          setError(t("settings.autostart.loadFailed", { error: String(err) }));
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [t]);

  async function handleToggle(next: boolean) {
    // Optimistically reflect the intent, then reconcile with whatever the OS
    // actually recorded. On failure we restore the real state instead of
    // trusting the optimistic value.
    setSaving(true);
    setError(null);
    setEnabled(next);
    try {
      const actual = await api.setAutostartEnabled(next);
      setEnabled(actual);
    } catch (err) {
      setError(t("settings.autostart.updateFailed", { error: String(err) }));
      // Re-query the real state so the switch matches reality after a failure.
      try {
        setEnabled(await api.getAutostartEnabled());
      } catch {
        // Leave the UI as-is; the error message already explains the failure.
      }
    } finally {
      setSaving(false);
    }
  }

  return (
    <Card className="w-full">
      <CardHeader>
        <CardTitle>{t("settings.general.title")}</CardTitle>
        <CardDescription>{t("settings.general.description")}</CardDescription>
      </CardHeader>
      <CardContent className="p-0">
        <FieldGroup className="gap-0">
          <Field orientation="responsive" className="px-5 py-4">
            <FieldContent>
              <FieldTitle>{t("language.title")}</FieldTitle>
              <FieldDescription>{t("language.description")}</FieldDescription>
            </FieldContent>
            <Select
              value={locale}
              onChange={(event) => setLocale(event.target.value as Locale)}
              className="w-full @md/field-group:w-44"
              aria-label={t("language.title")}
            >
              <option value="zh-CN">{t("language.zh")}</option>
              <option value="en-US">{t("language.en")}</option>
            </Select>
          </Field>
          <Separator />
          <Field orientation="responsive" className="px-5 py-4">
            <FieldContent>
              <FieldTitle>{t("theme.title")}</FieldTitle>
              <FieldDescription>{t("theme.description")}</FieldDescription>
            </FieldContent>
            <Select
              value={theme}
              onChange={(event) =>
                setTheme(event.target.value as ThemePreference)
              }
              className="w-full @md/field-group:w-44"
              aria-label={t("theme.title")}
            >
              <option value="system">{t("theme.system")}</option>
              <option value="light">{t("theme.light")}</option>
              <option value="dark">{t("theme.dark")}</option>
            </Select>
          </Field>
          <Separator />
          <Field
            orientation="horizontal"
            className="px-5 py-4"
            data-disabled={loading || saving || undefined}
          >
            <FieldContent>
              <FieldTitle>{t("settings.autostart.toggle")}</FieldTitle>
              <FieldDescription>
                {t("settings.autostart.toggleHint", { name: APP_NAME })}
              </FieldDescription>
            </FieldContent>
            {loading ? (
              <Skeleton className="h-[18px] w-8" />
            ) : (
              <Switch
                id="autostart-enabled"
                checked={enabled}
                disabled={saving}
                onCheckedChange={(checked) => void handleToggle(checked)}
                aria-label={t("settings.autostart.toggle")}
              />
            )}
          </Field>
        </FieldGroup>
        {error && (
          <div className="px-5 pb-4">
            <ErrorMessage error={error} />
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function ProxyCard() {
  const { t, proxyLabel } = useI18n();
  const [mode, setMode] = useState<ProxyMode>("system");
  const [customUrl, setCustomUrl] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    let active = true;
    api
      .getProxySettings()
      .then((settings) => {
        if (!active) return;
        setMode(settings.mode);
        setCustomUrl(settings.customUrl ?? "");
        setError(null);
      })
      .catch((err) => {
        if (active) setError(String(err));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);

  async function handleSave(e: React.FormEvent) {
    e.preventDefault();
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      const settings = await api.updateProxySettings({
        mode,
        customUrl: mode === "custom" ? customUrl : null,
      });
      setMode(settings.mode);
      setCustomUrl(settings.customUrl ?? "");
      setSaved(true);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  const hintFor = (option: ProxyMode) => {
    switch (option) {
      case "system":
        return t("settings.proxy.systemHint");
      case "none":
        return t("settings.proxy.noneHint");
      case "custom":
        return t("settings.proxy.customHint");
    }
  };

  return (
    <Card className="w-full">
      <CardHeader>
        <CardTitle>{t("settings.proxy")}</CardTitle>
        <CardDescription>{t("settings.proxyHint")}</CardDescription>
      </CardHeader>
      <CardContent>
        {loading ? (
          <div className="py-8 text-center text-sm text-muted-foreground">
            {t("loadingSettings")}
          </div>
        ) : (
          <form onSubmit={handleSave} className="flex flex-col gap-4">
            <FieldGroup>
              <Field>
                <Label htmlFor="proxy-mode">{t("settings.proxyMode")}</Label>
                <Select
                  id="proxy-mode"
                  value={mode}
                  onChange={(event) => {
                    setMode(event.target.value as ProxyMode);
                    setSaved(false);
                  }}
                >
                  {PROXY_MODES.map((option) => (
                    <option key={option} value={option}>
                      {proxyLabel(option)}
                    </option>
                  ))}
                </Select>
                <FieldDescription>{hintFor(mode)}</FieldDescription>
              </Field>

              {mode === "custom" && (
                <Field>
                  <Label htmlFor="proxy-url">{t("settings.proxyUrl")}</Label>
                  <Input
                    id="proxy-url"
                    value={customUrl}
                    onChange={(e) => {
                      setCustomUrl(e.target.value);
                      setSaved(false);
                    }}
                    placeholder="http://proxy.example.com:8080"
                    required
                  />
                  <FieldDescription>{t("settings.proxyUrlHint")}</FieldDescription>
                </Field>
              )}
            </FieldGroup>

            {error && <ErrorMessage error={error} />}
            {saved && !error && <SuccessMessage text={t("settings.proxySaved")} />}

            <div className="flex justify-end pt-2">
              <Button type="submit" variant="primary" disabled={saving}>
                {saving ? t("saving") : t("saveChanges")}
              </Button>
            </div>
          </form>
        )}
      </CardContent>
    </Card>
  );
}

function CheckDefaultsCard() {
  const { t } = useI18n();
  const [threshold, setThreshold] = useState("");
  const [interval, setInterval] = useState("");
  const [retention, setRetention] = useState("");
  const [userAgent, setUserAgent] = useState("");
  const [cooldown, setCooldown] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    let active = true;
    api
      .getAppSettings()
      .then((settings) => {
        if (!active) return;
        setThreshold(String(settings.defaultBalanceThreshold));
        setInterval(String(settings.defaultCheckIntervalMinutes));
        setRetention(String(settings.historyRetentionDays));
        setUserAgent(settings.userAgent);
        setCooldown(String(settings.notificationCooldownMinutes));
        setError(null);
      })
      .catch((err) => {
        if (active) setError(String(err));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);

  async function handleSave(e: React.FormEvent) {
    e.preventDefault();
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      const settings = await api.updateAppSettings({
        defaultBalanceThreshold: Number(threshold),
        defaultCheckIntervalMinutes: Number(interval),
        historyRetentionDays: Number(retention),
        userAgent,
        notificationCooldownMinutes: Number(cooldown),
      });
      setThreshold(String(settings.defaultBalanceThreshold));
      setInterval(String(settings.defaultCheckIntervalMinutes));
      setRetention(String(settings.historyRetentionDays));
      setUserAgent(settings.userAgent);
      setCooldown(String(settings.notificationCooldownMinutes));
      setSaved(true);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  const clearSaved = () => setSaved(false);

  return (
    <Card className="w-full">
      <CardHeader>
        <CardTitle>{t("settings.checkDefaults")}</CardTitle>
        <CardDescription>{t("settings.checkDefaultsHint")}</CardDescription>
      </CardHeader>
      <CardContent>
        {loading ? (
          <div className="py-8 text-center text-sm text-muted-foreground">
            {t("loadingSettings")}
          </div>
        ) : (
          <form onSubmit={handleSave} className="flex flex-col gap-4">
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <Field>
                <FieldLabel htmlFor="default-threshold">
                  {t("settings.globalThreshold")}
                </FieldLabel>
                <Input
                  id="default-threshold"
                  type="number"
                  step="any"
                  min="0"
                  value={threshold}
                  onChange={(e) => {
                    setThreshold(e.target.value);
                    clearSaved();
                  }}
                  required
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="default-interval">
                  {t("settings.defaultInterval")}
                </FieldLabel>
                <Input
                  id="default-interval"
                  type="number"
                  min="1"
                  value={interval}
                  onChange={(e) => {
                    setInterval(e.target.value);
                    clearSaved();
                  }}
                  required
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="retention-days">
                  {t("settings.retention")}
                </FieldLabel>
                <Input
                  id="retention-days"
                  type="number"
                  min="1"
                  value={retention}
                  onChange={(e) => {
                    setRetention(e.target.value);
                    clearSaved();
                  }}
                  required
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="user-agent">User-Agent</FieldLabel>
                <Input
                  id="user-agent"
                  value={userAgent}
                  onChange={(e) => {
                    setUserAgent(e.target.value);
                    clearSaved();
                  }}
                  placeholder={`${APP_PACKAGE_NAME}/${APP_VERSION}`}
                  required
                />
              </Field>
              <Field className="sm:col-span-2">
                <FieldLabel htmlFor="notification-cooldown">
                  {t("settings.notificationCooldown")}
                </FieldLabel>
                <Input
                  id="notification-cooldown"
                  type="number"
                  min="1"
                  value={cooldown}
                  onChange={(e) => {
                    setCooldown(e.target.value);
                    clearSaved();
                  }}
                  required
                />
                <FieldDescription>
                  {t("settings.notificationCooldownHint")}
                </FieldDescription>
              </Field>
            </div>

            {error && <ErrorMessage error={error} />}
            {saved && !error && <SuccessMessage text={t("settings.defaultsSaved")} />}

            <div className="flex justify-end pt-2">
              <Button type="submit" variant="primary" disabled={saving}>
                {saving ? t("saving") : t("saveChanges")}
              </Button>
            </div>
          </form>
        )}
      </CardContent>
    </Card>
  );
}

function ImportExportCard({
  onImportComplete,
}: {
  onImportComplete?: () => Promise<void> | void;
}) {
  const { t } = useI18n();
  const [includeCredentials, setIncludeCredentials] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);
  const [exportSuccess, setExportSuccess] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [importCredentials, setImportCredentials] = useState(false);
  const [overwriteExisting, setOverwriteExisting] = useState(false);
  const [importing, setImporting] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);
  const [refreshError, setRefreshError] = useState<string | null>(null);
  const [report, setReport] = useState<ImportReport | null>(null);

  async function handleExport() {
    setExporting(true);
    setExportError(null);
    setExportSuccess(null);
    try {
      const savedPath = await api.exportConfigToFile(includeCredentials);
      if (savedPath === null) {
        // User dismissed the save dialog: not an error, just nothing to report.
        setExportSuccess(t("settings.exportCancelled"));
        return;
      }
      setExportSuccess(t("settings.exportSuccess", { path: savedPath }));
    } catch (err) {
      setExportError(String(err));
    } finally {
      setExporting(false);
    }
  }

  async function handleFileSelected(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;

    setImporting(true);
    setImportError(null);
    setRefreshError(null);
    setReport(null);
    try {
      const text = await file.text();
      const parsed = JSON.parse(text);
      const result = await api.importConfig(parsed, {
        importCredentials,
        overwriteExisting,
      });
      setReport(result);
      // Import succeeded — ask the parent to refresh cached state (accounts
      // list, etc.). A refresh failure must not be swallowed: the import did
      // land, but the UI would show stale data until a full page reload.
      try {
        await onImportComplete?.();
      } catch (refreshErr) {
        setRefreshError(
          t("settings.importRefreshFailed", { error: String(refreshErr) }),
        );
      }
    } catch (err) {
      setImportError(String(err));
    } finally {
      setImporting(false);
    }
  }

  return (
    <Card className="w-full">
      <CardHeader>
        <CardTitle>{t("settings.importExport")}</CardTitle>
        <CardDescription>{t("settings.importExportHint")}</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="flex flex-col gap-5">
          <div className="flex flex-col gap-3">
            <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              {t("settings.export")}
            </h3>
            <SwitchSettingRow
              id="include-export-credentials"
              checked={includeCredentials}
              onChange={setIncludeCredentials}
              title={t("settings.includeCredentials")}
              description={t("settings.includeCredentialsHint")}
            />
            {includeCredentials && (
              <p className="rounded-md border border-warning/30 bg-warning-muted px-3 py-2 text-xs text-warning-foreground">
                {t("settings.credentialsWarning")}
              </p>
            )}
            {exportError && <ErrorMessage error={exportError} />}
            {exportSuccess && !exportError && (
              <SuccessMessage text={exportSuccess} />
            )}
            <div className="flex justify-end">
              <Button
                type="button"
                variant="primary"
                disabled={exporting}
                onClick={() => void handleExport()}
              >
                {exporting ? t("settings.exporting") : t("settings.exportJson")}
              </Button>
            </div>
          </div>

          <Separator />

          <div className="flex flex-col gap-3">
            <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              {t("settings.import")}
            </h3>
            <p className="text-xs text-muted-foreground">{t("settings.importHint")}</p>

            <SwitchSettingRow
              id="import-credentials"
              checked={importCredentials}
              onChange={setImportCredentials}
              title={t("settings.importCredentials")}
              description={t("settings.importCredentialsHint")}
            />
            <SwitchSettingRow
              id="overwrite-existing"
              checked={overwriteExisting}
              onChange={setOverwriteExisting}
              title={t("settings.overwriteExisting")}
              description={t("settings.overwriteExistingHint")}
            />

            <input
              ref={fileInputRef}
              type="file"
              accept="application/json,.json"
              className="hidden"
              onChange={(e) => void handleFileSelected(e)}
            />

            {importError && <ErrorMessage error={importError} />}
            {report && <ImportReportView report={report} />}
            {refreshError && (
              <p className="rounded-md border border-warning/30 bg-warning-muted px-3 py-2 text-sm text-warning-foreground">
                {refreshError}
              </p>
            )}

            <div className="flex justify-end">
              <Button
                type="button"
                variant="secondary"
                disabled={importing}
                onClick={() => fileInputRef.current?.click()}
              >
                {importing ? t("settings.importing") : t("settings.chooseJson")}
              </Button>
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

function AboutCard({
  onUpdateAvailable,
}: {
  onUpdateAvailable?: (update: UpdateCheckResult) => void;
}) {
  const { t } = useI18n();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<UpdateCheckResult | null>(null);
  const [checkError, setCheckError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    api
      .getAppInfo()
      .then((value) => {
        if (active) setInfo(value);
      })
      .catch((err) => {
        if (active) setLoadError(String(err));
      });
    return () => {
      active = false;
    };
  }, []);

  async function open(url: string) {
    try {
      await openUrl(url);
    } catch (err) {
      setCheckError(t("about.openFailed", { error: String(err) }));
    }
  }

  async function handleCheck() {
    setChecking(true);
    setCheckError(null);
    setResult(null);
    try {
      const nextResult = await api.checkForUpdate();
      setResult(nextResult);
      if (nextResult.updateAvailable) onUpdateAvailable?.(nextResult);
    } catch (err) {
      setCheckError(t("about.checkFailed", { error: String(err) }));
    } finally {
      setChecking(false);
    }
  }

  return (
    <Card className="col-span-full w-full">
      <CardHeader>
        <CardTitle>{t("about.title")}</CardTitle>
        <CardDescription>{t("about.subtitle")}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {loadError ? (
          <ErrorMessage error={loadError} />
        ) : !info ? (
          <div className="flex items-center gap-4 py-2">
            <Skeleton className="size-14" />
            <div className="flex flex-col gap-2">
              <Skeleton className="h-4 w-32" />
              <Skeleton className="h-3 w-44" />
            </div>
          </div>
        ) : (
          <>
            <div className="flex flex-wrap items-center justify-between gap-6 py-1">
              <div className="flex min-w-0 items-center gap-4">
                <img
                  src={appLogo}
                  alt={info.name}
                  className="size-14 shrink-0 rounded-md object-contain"
                />
                <div className="flex min-w-0 flex-col gap-1.5">
                  <div className="flex flex-wrap items-center gap-2">
                    <div className="truncate text-base font-semibold">
                      {info.name}
                    </div>
                    <Badge variant="neutral">v{info.version}</Badge>
                  </div>
                  <div className="text-xs text-muted-foreground">
                    {t("about.author")}: {info.author}
                  </div>
                </div>
              </div>

              <div className="flex flex-wrap items-center gap-2">
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  onClick={() => void open(info.githubUrl)}
                >
                  <Code2 data-icon="inline-start" />
                  {t("about.openGithub")}
                </Button>
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  onClick={() => void open(`${info.githubUrl}/releases`)}
                >
                  <ScrollText data-icon="inline-start" />
                  {t("about.changelog")}
                </Button>
                <Button
                  type="button"
                  variant="primary"
                  size="sm"
                  disabled={checking}
                  onClick={() => void handleCheck()}
                >
                  <RefreshCw
                    data-icon="inline-start"
                    className={cn(checking && "animate-spin")}
                  />
                  {checking ? t("about.checking") : t("about.checkUpdate")}
                </Button>
              </div>
            </div>

            {result && !checkError && (
              <Alert>
                <AlertDescription>
                  {result.updateAvailable
                    ? t("about.updateAvailable", { version: result.latestVersion })
                    : t("about.upToDate")}
                </AlertDescription>
              </Alert>
            )}
            {checkError && <ErrorMessage error={checkError} />}

            {result?.updateAvailable && (
              <div className="flex justify-end">
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  onClick={() => void open(result.releaseUrl)}
                >
                  {t("about.openRelease")}
                </Button>
              </div>
            )}
          </>
        )}
      </CardContent>
    </Card>
  );
}

function SwitchSettingRow({
  id,
  checked,
  onChange,
  title,
  description,
}: {
  id: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  title: string;
  description: string;
}) {
  return (
    <Field orientation="horizontal">
      <FieldContent>
        <FieldTitle>{title}</FieldTitle>
        <FieldDescription>{description}</FieldDescription>
      </FieldContent>
      <Switch
        id={id}
        checked={checked}
        onCheckedChange={onChange}
        aria-label={title}
      />
    </Field>
  );
}

function ImportReportView({ report }: { report: ImportReport }) {
  const { t } = useI18n();
  const rows: [string, number][] = [
    [t("settings.accountsCreated"), report.accountsCreated],
    [t("settings.accountsUpdated"), report.accountsUpdated],
    [t("settings.accountsSkipped"), report.accountsSkipped],
    [t("settings.hooksCreated"), report.hooksCreated],
    [t("settings.hooksUpdated"), report.hooksUpdated],
    [t("settings.settingsUpdated"), report.settingsUpdated],
  ];
  return (
    <div className="rounded-md border border-success/30 bg-success-muted px-3 py-3 text-sm text-success">
      <div className="font-medium">{t("settings.importComplete")}</div>
      <dl className="mt-2 grid grid-cols-2 gap-x-4 gap-y-1 text-xs">
        {rows.map(([label, value]) => (
          <div key={label} className="flex justify-between gap-2">
            <dt className="text-success">{label}</dt>
            <dd className="font-medium tabular-nums">{value}</dd>
          </div>
        ))}
      </dl>
      {report.warnings.length > 0 && (
        <div className="mt-3">
          <div className="text-xs font-medium text-warning-foreground">
            {t("settings.warnings")}
          </div>
          <ul className="mt-1 flex list-disc flex-col gap-0.5 pl-4 text-xs text-warning-foreground">
            {report.warnings.map((warning, i) => (
              <li key={i}>{warning}</li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

function ErrorMessage({ error }: { error: string }) {
  return (
    <p className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
      {error}
    </p>
  );
}

function SuccessMessage({ text }: { text: string }) {
  return (
    <p className="rounded-md border border-success/30 bg-success-muted px-3 py-2 text-sm text-success">
      {text}
    </p>
  );
}
