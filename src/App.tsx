import { useCallback, useEffect, useState } from "react";
import {
  Clock3,
  Gauge,
  History,
  KeyRound,
  Plus,
  RefreshCw,
  Settings2,
  Webhook,
} from "lucide-react";
import appLogo from "../src-tauri/icons/128x128.png";
import "./App.css";
import { Button } from "./components/ui/button";
import { Alert, AlertDescription } from "./components/ui/alert";
import { Switch } from "./components/ui/switch";
import { AccountsView } from "./components/AccountsView";
import { HistoryView } from "./components/HistoryView";
import { OverviewView } from "./components/OverviewView";
import { SchedulesView } from "./components/SchedulesView";
import { SettingsView } from "./components/SettingsView";
import { UpdateAvailableDialog } from "./components/UpdateAvailableDialog";
import { WebhooksView } from "./components/WebhooksView";
import * as api from "./lib/api";
import { APP_NAME } from "./lib/app-metadata";
import { useI18n } from "./lib/i18n";
import { cn } from "./lib/utils";
import type {
  AccountView,
  CreateAccountInput,
  UpdateCheckResult,
  UpdateAccountInput,
} from "./types";

type View =
  | "overview"
  | "accounts"
  | "history"
  | "schedules"
  | "settings"
  | "webhooks";

type HeaderRefreshAction = {
  busy: boolean;
  refresh: () => void;
};

function App() {
  const { t } = useI18n();
  const [view, setView] = useState<View>("overview");
  const [accounts, setAccounts] = useState<AccountView[]>([]);
  const [loading, setLoading] = useState(true);
  const [overviewRefreshAction, setOverviewRefreshAction] =
    useState<HeaderRefreshAction | null>(null);
  const [availableUpdate, setAvailableUpdate] =
    useState<UpdateCheckResult | null>(null);

  const [checkingId, setCheckingId] = useState<string | null>(null);
  const [rowErrors, setRowErrors] = useState<Record<string, string>>({});
  const [loadError, setLoadError] = useState<string | null>(null);

  // Global automatic-scheduling master switch, shown in the accounts header.
  const [schedulerEnabled, setSchedulerEnabled] = useState(true);
  const [schedulerToggleLoading, setSchedulerToggleLoading] = useState(false);
  const [schedulerToggleError, setSchedulerToggleError] = useState<string | null>(
    null,
  );

  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<AccountView | null>(null);
  const [isDraggingAccount, setIsDraggingAccount] = useState(false);

  type BatchRefreshStatus = 'pending' | 'running' | 'success' | 'failed' | 'skipped';
  interface BatchRefreshItem {
    id: string;
    name: string;
    status: BatchRefreshStatus;
    message?: string;
  }
  interface BatchRefreshState {
    open: boolean;
    phase: 'confirm' | 'progress' | 'done';
    items: BatchRefreshItem[];
    currentIndex: number;
  }

  const [batchRefresh, setBatchRefresh] = useState<BatchRefreshState>({
    open: false,
    phase: 'confirm',
    items: [],
    currentIndex: -1,
  });

  useEffect(() => {
    document.title = APP_NAME;
  }, []);

  useEffect(() => {
    let active = true;
    api
      .checkForUpdate()
      .then((result) => {
        if (active && result.updateAvailable) setAvailableUpdate(result);
      })
      .catch(() => {
        // Startup checks are silent; manual checks in Settings surface errors.
      });
    return () => {
      active = false;
    };
  }, []);

  const refresh = useCallback(async () => {
    try {
      const accs = await api.listAccounts();
      setAccounts(accs);
      setLoadError(null);
    } catch (err) {
      setLoadError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Load the global scheduler switch once at startup so the header reflects the
  // stored state. A failure here is non-fatal: leave the optimistic default on
  // and let a toggle attempt surface any real error.
  useEffect(() => {
    let active = true;
    api
      .getAppSettings()
      .then((settings) => {
        if (active) setSchedulerEnabled(settings.schedulerEnabled);
      })
      .catch(() => {
        // Non-fatal; the toggle handler reports errors when the user acts.
      });
    return () => {
      active = false;
    };
  }, []);

  const handleToggleScheduler = useCallback(async () => {
    const next = !schedulerEnabled;
    setSchedulerToggleLoading(true);
    setSchedulerToggleError(null);
    try {
      const settings = await api.setSchedulerEnabled(next);
      setSchedulerEnabled(settings.schedulerEnabled);
    } catch (err) {
      setSchedulerToggleError(String(err));
    } finally {
      setSchedulerToggleLoading(false);
    }
  }, [schedulerEnabled]);

  // Auto-refresh accounts every 30 seconds when on accounts view (but not while dragging)
  useEffect(() => {
    if (view !== 'accounts') {
      return;
    }

    let refreshing = false;
    const intervalId = setInterval(() => {
      if (!refreshing && !isDraggingAccount) {
        refreshing = true;
        refresh().finally(() => {
          refreshing = false;
        });
      }
    }, 30000);

    return () => {
      clearInterval(intervalId);
    };
  }, [view, refresh, isDraggingAccount]);

  const handleCreate = useCallback(
    async (input: CreateAccountInput) => {
      const created = await api.createAccount(input);
      await refresh(); // Show the new account in the list

      setCheckingId(created.id);
      setRowErrors((prev) => {
        const next = { ...prev };
        delete next[created.id];
        return next;
      });

      try {
        const updated = await api.checkAccount(created.id);
        setAccounts((prev) =>
          prev.map((a) => (a.id === updated.id ? updated : a)),
        );
      } catch (err) {
        setRowErrors((prev) => ({ ...prev, [created.id]: String(err) }));
        // Don't throw - creation succeeded, only balance check failed
      } finally {
        setCheckingId(null);
        await refresh(); // Ensure sorting/snapshot is latest
      }
    },
    [refresh],
  );

  const handleUpdate = useCallback(
    async (id: string, input: UpdateAccountInput) => {
      await api.updateAccount(id, input);
      await refresh();
    },
    [refresh],
  );

  const handleDelete = useCallback(
    async (id: string) => {
      try {
        await api.deleteAccount(id);
        await refresh();
      } catch (err) {
        setRowErrors((prev) => ({ ...prev, [id]: String(err) }));
      }
    },
    [refresh],
  );

  const handleCheck = useCallback(async (account: AccountView) => {
    setCheckingId(account.id);
    setRowErrors((prev) => {
      const next = { ...prev };
      delete next[account.id];
      return next;
    });
    try {
      const updated = await api.checkAccount(account.id);
      setAccounts((prev) => prev.map((a) => (a.id === updated.id ? updated : a)));
    } catch (err) {
      setRowErrors((prev) => ({ ...prev, [account.id]: String(err) }));
    } finally {
      setCheckingId(null);
    }
  }, []);

  const handleLoadCredentials = useCallback(async (id: string) => {
    return await api.getAccountCredentials(id);
  }, []);

  const handleImportComplete = useCallback(async () => {
    // Stale per-row/load errors refer to the pre-import account set; clear them
    // so the freshly imported list starts clean. We re-throw on failure so
    // SettingsView can tell the user the import landed but the refresh didn't.
    setRowErrors({});
    setLoadError(null);
    // The imported config can carry schedulerEnabled, so refresh both the
    // account list and the app settings; if either read fails we throw before
    // touching state so the header never shows a stale scheduler state.
    const [accs, settings] = await Promise.all([
      api.listAccounts(),
      api.getAppSettings(),
    ]);
    setAccounts(accs);
    setSchedulerEnabled(settings.schedulerEnabled);
    setSchedulerToggleError(null);
  }, []);

  const handleReorder = useCallback(async (orderedIds: string[]) => {
    const updated = await api.reorderAccounts(orderedIds);
    setAccounts(updated);
  }, []);

  const handleSetEnabled = useCallback(async (account: AccountView, enabled: boolean) => {
    try {
      const updated = await api.setAccountEnabled(account.id, enabled);
      setAccounts((prev) => prev.map((a) => (a.id === updated.id ? updated : a)));
      setRowErrors((prev) => {
        const next = { ...prev };
        delete next[account.id];
        return next;
      });
    } catch (err) {
      setRowErrors((prev) => ({ ...prev, [account.id]: String(err) }));
      throw err;
    }
  }, []);

  const openAdd = useCallback(() => {
    setEditing(null);
    setFormOpen(true);
  }, []);

  const openEdit = useCallback((account: AccountView) => {
    setEditing(account);
    setFormOpen(true);
  }, []);

  const openBatchRefresh = useCallback(() => {
    const items: BatchRefreshItem[] = accounts.map(acc => ({
      id: acc.id,
      name: acc.name,
      status: 'pending' as BatchRefreshStatus,
    }));

    setBatchRefresh({
      open: true,
      phase: 'confirm',
      items,
      currentIndex: -1,
    });
  }, [accounts]);

  const startBatchRefresh = useCallback(async () => {
    setBatchRefresh(prev => ({ ...prev, phase: 'progress' }));

    for (let i = 0; i < batchRefresh.items.length; i++) {
      const item = batchRefresh.items[i];
      const account = accounts.find(a => a.id === item.id);

      if (!account) continue;

      setBatchRefresh(prev => ({
        ...prev,
        currentIndex: i,
        items: prev.items.map((it, idx) =>
          idx === i ? { ...it, status: 'running' as BatchRefreshStatus } : it
        ),
      }));

      // Check if account has credentials
      if (!account.hasCredentials) {
        setBatchRefresh(prev => ({
          ...prev,
          items: prev.items.map((it, idx) =>
            idx === i
              ? {
                  ...it,
                  status: 'skipped' as BatchRefreshStatus,
                  message: t('accounts.refreshAllSkippedNoCredentials'),
                }
              : it
          ),
        }));
        continue;
      }

      // Set checking state and clear error
      setCheckingId(account.id);
      setRowErrors(prev => {
        const next = { ...prev };
        delete next[account.id];
        return next;
      });

      try {
        const updated = await api.checkAccount(account.id);
        setAccounts(prev => prev.map(a => (a.id === updated.id ? updated : a)));

        setBatchRefresh(prev => ({
          ...prev,
          items: prev.items.map((it, idx) =>
            idx === i
              ? { ...it, status: 'success' as BatchRefreshStatus }
              : it
          ),
        }));
      } catch (err) {
        const errorMsg = String(err);
        setRowErrors(prev => ({ ...prev, [account.id]: errorMsg }));

        setBatchRefresh(prev => ({
          ...prev,
          items: prev.items.map((it, idx) =>
            idx === i
              ? {
                  ...it,
                  status: 'failed' as BatchRefreshStatus,
                  message: errorMsg,
                }
              : it
          ),
        }));
      } finally {
        setCheckingId(null);
      }
    }

    // Final refresh to sync list
    await refresh();

    setBatchRefresh(prev => ({ ...prev, phase: 'done', currentIndex: -1 }));
  }, [batchRefresh.items, accounts, refresh, t]);

  const closeBatchRefresh = useCallback(() => {
    setBatchRefresh({
      open: false,
      phase: 'confirm',
      items: [],
      currentIndex: -1,
    });
  }, []);

  const viewLabels: Record<View, string> = {
    overview: t("view.overview"),
    accounts: t("view.accounts"),
    history: t("view.history"),
    schedules: t("view.schedules"),
    webhooks: t("view.webhooks"),
    settings: t("view.settings"),
  };

  const viewDescriptions: Record<View, string> = {
    overview: t("view.overviewDescription"),
    accounts: t("view.accountsDescription"),
    history: t("view.historyDescription"),
    schedules: t("view.schedulesDescription"),
    webhooks: t("view.webhooksDescription"),
    settings: t("view.settingsDescription"),
  };

  const navGroups: { label: string; items: [string, typeof Gauge, View][] }[] = [
    {
      label: t("nav.operations"),
      items: [
        [viewLabels.overview, Gauge, "overview"],
        [viewLabels.accounts, KeyRound, "accounts"],
        [viewLabels.history, History, "history"],
        [viewLabels.schedules, Clock3, "schedules"],
      ],
    },
    {
      label: t("nav.system"),
      items: [
        [viewLabels.webhooks, Webhook, "webhooks"],
        [viewLabels.settings, Settings2, "settings"],
      ],
    },
  ];

  return (
    <main className="h-screen overflow-hidden bg-background text-foreground">
      <div className="grid h-full grid-cols-[248px_1fr]">
        <aside className="flex h-full flex-col overflow-hidden border-r border-sidebar-border bg-sidebar text-sidebar-foreground">
          <div className="flex h-[76px] shrink-0 items-center gap-3 border-b border-sidebar-border px-5">
            <div className="flex size-10 items-center justify-center overflow-hidden rounded-md border border-sidebar-border bg-sidebar-foreground/5">
              <img src={appLogo} alt={APP_NAME} className="size-full object-contain" />
            </div>
            <div className="min-w-0">
              <div className="truncate text-sm font-semibold">{APP_NAME}</div>
              <div className="truncate text-xs text-sidebar-foreground/55">{t("app.subtitle")}</div>
            </div>
          </div>

          <nav className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto px-3 py-5">
            {navGroups.map((group) => (
              <div key={group.label} className="flex flex-col gap-1">
                <div className="px-3 pb-1.5 text-[11px] font-medium uppercase text-sidebar-foreground/40">
                  {group.label}
                </div>
                {group.items.map(([label, Icon, target]) => {
                  const active = view === target;
                  return (
                    <button
                      key={target}
                      type="button"
                      onClick={() => setView(target)}
                      aria-current={active ? "page" : undefined}
                      className={cn(
                        "flex h-10 w-full cursor-pointer items-center gap-3 rounded-md px-3 text-sm font-medium transition-colors outline-none focus-visible:ring-2 focus-visible:ring-sidebar-ring",
                        active
                          ? "bg-sidebar-accent text-sidebar-accent-foreground shadow-xs"
                          : "text-sidebar-foreground/65 hover:bg-sidebar-accent/65 hover:text-sidebar-accent-foreground",
                      )}
                    >
                      <Icon className={cn("size-4", active && "text-sidebar-primary")} />
                      <span className="truncate">{label}</span>
                    </button>
                  );
                })}
              </div>
            ))}
          </nav>

          <div className="border-t border-sidebar-border px-5 py-4">
            <div className="flex items-center gap-2 text-xs text-sidebar-foreground/45">
              <span className="size-1.5 rounded-full bg-sidebar-primary" />
              {t("accounts.masterScheduler")}
              <span className="ml-auto text-sidebar-foreground/70">
                {schedulerEnabled ? t("enabled") : t("disabled")}
              </span>
            </div>
          </div>
        </aside>

        <section className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-background">
          <header className="flex min-h-[76px] shrink-0 items-center justify-between gap-6 border-b bg-card px-7 py-3">
            <div className="min-w-0">
              <h1 className="truncate text-lg font-semibold">{viewLabels[view]}</h1>
              <p className="mt-0.5 truncate text-sm text-muted-foreground">
                {viewDescriptions[view]}
              </p>
            </div>

            <div className="flex shrink-0 items-center gap-2">
              {view === "overview" && (
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={overviewRefreshAction?.refresh}
                  disabled={!overviewRefreshAction || overviewRefreshAction.busy}
                >
                  <RefreshCw
                    data-icon="inline-start"
                    className={cn(overviewRefreshAction?.busy && "animate-spin")}
                  />
                  {t("refresh")}
                </Button>
              )}
              {view === "accounts" && (
                <>
                  <label
                    className="mr-1 flex h-9 cursor-pointer items-center gap-3 rounded-md border bg-background px-3 shadow-xs"
                    title={
                      schedulerEnabled
                        ? t("accounts.masterSchedulerOnTitle")
                        : t("accounts.masterSchedulerOffTitle")
                    }
                  >
                    <span className="text-sm font-medium">{t("accounts.masterScheduler")}</span>
                    <Switch
                      checked={schedulerEnabled}
                      onCheckedChange={() => void handleToggleScheduler()}
                      disabled={schedulerToggleLoading}
                      aria-label={t("accounts.masterScheduler")}
                    />
                  </label>
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={openBatchRefresh}
                    disabled={batchRefresh.phase === "progress"}
                  >
                    <RefreshCw
                      data-icon="inline-start"
                      className={cn(batchRefresh.phase === "progress" && "animate-spin")}
                    />
                    {t("refresh")}
                  </Button>
                  <Button variant="primary" size="sm" onClick={openAdd}>
                    <Plus data-icon="inline-start" />
                    {t("accounts.add")}
                  </Button>
                </>
              )}
            </div>
          </header>

          {view === "accounts" && schedulerToggleError && (
            <div className="shrink-0 border-b bg-card px-7 py-2">
              <Alert variant="destructive">
                <AlertDescription>
                  {t("accounts.masterSchedulerFailed", { error: schedulerToggleError })}
                </AlertDescription>
              </Alert>
            </div>
          )}

          <div className="min-h-0 flex flex-1 flex-col overflow-hidden">
            {view === "overview" ? (
              <OverviewView onHeaderRefreshChange={setOverviewRefreshAction} />
            ) : view === "accounts" ? (
              <AccountsView
                accounts={accounts}
                loading={loading}
                checkingId={checkingId}
                rowErrors={rowErrors}
                loadError={loadError}
                formOpen={formOpen}
                editing={editing}
                onOpenAdd={openAdd}
                onOpenEdit={openEdit}
                onCloseForm={() => setFormOpen(false)}
                onCheck={handleCheck}
                onCreate={handleCreate}
                onUpdate={handleUpdate}
                onDelete={handleDelete}
                onLoadCredentials={handleLoadCredentials}
                onReorder={handleReorder}
                onDraggingChange={setIsDraggingAccount}
                onSetEnabled={handleSetEnabled}
              />
            ) : view === "history" ? (
              <HistoryView />
            ) : view === "schedules" ? (
              <SchedulesView />
            ) : view === "settings" ? (
              <SettingsView
                onImportComplete={handleImportComplete}
                onUpdateAvailable={setAvailableUpdate}
              />
            ) : (
              <WebhooksView />
            )}
          </div>
        </section>
      </div>

      <UpdateAvailableDialog
        update={availableUpdate}
        onDismiss={() => setAvailableUpdate(null)}
      />

      {/* Batch Refresh Dialog */}
      {batchRefresh.open && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
          onClick={(e) => {
            if (batchRefresh.phase !== 'progress' && e.target === e.currentTarget) {
              closeBatchRefresh();
            }
          }}
        >
          <div className="w-full max-w-2xl rounded-lg bg-card p-6 shadow-xl">
            {batchRefresh.phase === 'confirm' && (
              <>
                <h2 className="text-xl font-semibold mb-2">
                  {t('accounts.refreshAllTitle')}
                </h2>
                <p className="text-sm text-muted-foreground mb-4">
                  {t('accounts.refreshAllDescription')}
                </p>
                {batchRefresh.items.length === 0 ? (
                  <p className="text-sm text-foreground mb-6">
                    {t('accounts.refreshAllEmpty')}
                  </p>
                ) : (
                  <p className="text-sm text-foreground mb-6">
                    {t('accounts.refreshAllCount', { count: batchRefresh.items.length })}
                  </p>
                )}
                <div className="flex justify-end gap-3">
                  <Button variant="secondary" onClick={closeBatchRefresh}>
                    {t('cancel')}
                  </Button>
                  <Button
                    variant="primary"
                    onClick={() => void startBatchRefresh()}
                    disabled={batchRefresh.items.length === 0}
                  >
                    {t('accounts.refreshAllConfirm')}
                  </Button>
                </div>
              </>
            )}

            {(batchRefresh.phase === 'progress' || batchRefresh.phase === 'done') && (
              <>
                <h2 className="text-xl font-semibold mb-4">
                  {batchRefresh.phase === 'progress'
                    ? t('accounts.refreshAllProgress')
                    : t('accounts.refreshAllDone')}
                </h2>

                {/* Progress bar */}
                <div className="mb-4">
                  <div className="flex justify-between text-sm text-muted-foreground mb-2">
                    <span>
                      {batchRefresh.items.filter(it => it.status !== 'pending' && it.status !== 'running').length} / {batchRefresh.items.length}
                    </span>
                    <span>
                      {batchRefresh.items.length > 0
                        ? Math.round(
                            (batchRefresh.items.filter(it => it.status !== 'pending' && it.status !== 'running').length /
                              batchRefresh.items.length) *
                              100
                          )
                        : 0}%
                    </span>
                  </div>
                  <div className="h-2 w-full rounded-full bg-muted">
                    <div
                      className="h-2 rounded-full bg-primary transition-all duration-300"
                      style={{
                        width: `${
                          batchRefresh.items.length > 0
                            ? (batchRefresh.items.filter(it => it.status !== 'pending' && it.status !== 'running').length /
                                batchRefresh.items.length) *
                              100
                            : 0
                        }%`,
                      }}
                    />
                  </div>
                </div>

                {/* Current item */}
                {batchRefresh.phase === 'progress' && batchRefresh.currentIndex >= 0 && (
                  <div className="mb-4 text-sm text-muted-foreground">
                    {t('accounts.refreshAllCurrent')}: {batchRefresh.items[batchRefresh.currentIndex]?.name}
                  </div>
                )}

                {/* Items list */}
                <div className="max-h-96 overflow-y-auto border border-border rounded-md mb-4">
                  {batchRefresh.items.map((item, idx) => (
                    <div
                      key={item.id}
                      className={`flex items-center justify-between px-4 py-3 border-b border-border last:border-b-0 ${
                        idx === batchRefresh.currentIndex ? 'bg-muted/50' : ''
                      }`}
                    >
                      <div className="flex-1 min-w-0">
                        <div className="font-medium text-sm truncate">{item.name}</div>
                        {item.status === 'failed' && item.message && (
                          <div className="text-xs text-destructive mt-1 truncate">
                            {item.message}
                          </div>
                        )}
                      </div>
                      <div className="ml-4 shrink-0">
                        {item.status === 'pending' && (
                          <span className="text-xs text-muted-foreground">
                            {t('accounts.refreshStatusPending')}
                          </span>
                        )}
                        {item.status === 'running' && (
                          <span className="text-xs text-info flex items-center gap-1">
                            <RefreshCw className="h-3 w-3 animate-spin" />
                            {t('accounts.refreshStatusRunning')}
                          </span>
                        )}
                        {item.status === 'success' && (
                          <span className="text-xs text-success font-medium">
                            {t('accounts.refreshStatusSuccess')}
                          </span>
                        )}
                        {item.status === 'failed' && (
                          <span className="text-xs text-destructive font-medium">
                            {t('accounts.refreshStatusFailed')}
                          </span>
                        )}
                        {item.status === 'skipped' && (
                          <span className="text-xs text-muted-foreground">
                            {t('accounts.refreshStatusSkipped')}
                          </span>
                        )}
                      </div>
                    </div>
                  ))}
                </div>

                {/* Close button (only when done) */}
                {batchRefresh.phase === 'done' && (
                  <div className="flex justify-end">
                    <Button variant="primary" onClick={closeBatchRefresh}>
                      {t('accounts.refreshAllClose')}
                    </Button>
                  </div>
                )}
              </>
            )}
          </div>
        </div>
      )}
    </main>
  );
}

export default App;
