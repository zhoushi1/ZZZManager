import { useCallback, useMemo, useState } from "react";
import {
  Clock3,
  Database,
  Globe2,
  GripVertical,
  LayoutList,
  Pencil,
  Plus,
  RefreshCw,
  Table2,
  Trash2,
  Wallet,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  DndContext,
  closestCenter,
  PointerSensor,
  useSensor,
  useSensors,
  DragEndEvent,
  DragStartEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  verticalListSortingStrategy,
  useSortable,
  arrayMove,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Alert, AlertDescription } from "./ui/alert";
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "./ui/empty";
import { Label } from "./ui/label";
import { Select } from "./ui/select";
import { Sheet } from "./ui/sheet";
import { Skeleton } from "./ui/skeleton";
import { ConfirmDialog } from "./ui/dialog";
import { Tooltip } from "./ui/tooltip";
import { AccountForm } from "./AccountForm";
import { useI18n } from "../lib/i18n";
import {
  convertUsdCreditsToCny,
  formatConvertedBalance,
  formatMoney,
  formatTime,
  resultIcon,
} from "../lib/format";
import type {
  AccountView,
  AccountCredentialsView,
  CreateAccountInput,
  UpdateAccountInput,
} from "../types";

type ViewMode = "list" | "table";
type AccountSortMode = "sort_order" | "balance";
type AccountStatusFilter = "all" | "enabled" | "disabled";

interface AccountsViewProps {
  accounts: AccountView[];
  loading: boolean;
  checkingId: string | null;
  rowErrors: Record<string, string>;
  loadError: string | null;
  formOpen: boolean;
  editing: AccountView | null;
  onOpenAdd: () => void;
  onOpenEdit: (account: AccountView) => void;
  onCloseForm: () => void;
  onCheck: (account: AccountView) => void;
  onCreate: (input: CreateAccountInput) => Promise<void>;
  onUpdate: (id: string, input: UpdateAccountInput) => Promise<void>;
  onDelete: (id: string) => Promise<void>;
  onLoadCredentials: (id: string) => Promise<AccountCredentialsView>;
  onReorder: (orderedIds: string[]) => Promise<void>;
  onDraggingChange: (isDragging: boolean) => void;
  onSetEnabled: (account: AccountView, enabled: boolean) => Promise<void>;
}

export function AccountsView({
  accounts,
  loading,
  checkingId,
  rowErrors,
  loadError,
  formOpen,
  editing,
  onOpenAdd,
  onOpenEdit,
  onCloseForm,
  onCheck,
  onCreate,
  onUpdate,
  onDelete,
  onLoadCredentials,
  onReorder,
  onDraggingChange,
  onSetEnabled,
}: AccountsViewProps) {
  const { t } = useI18n();
  const [viewMode, setViewMode] = useState<ViewMode>("list");
  const [sortMode, setSortMode] = useState<AccountSortMode>("sort_order");
  const [statusFilter, setStatusFilter] =
    useState<AccountStatusFilter>("all");
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [accountToDelete, setAccountToDelete] = useState<AccountView | null>(
    null,
  );
  const [submitting, setSubmitting] = useState(false);
  const [openErrors, setOpenErrors] = useState<Record<string, string>>({});

  const handleCreate = useCallback(
    async (input: CreateAccountInput) => {
      setSubmitting(true);
      try {
        await onCreate(input);
        onCloseForm();
      } finally {
        setSubmitting(false);
      }
    },
    [onCreate, onCloseForm],
  );

  const handleUpdate = useCallback(
    async (id: string, input: UpdateAccountInput) => {
      setSubmitting(true);
      try {
        await onUpdate(id, input);
        onCloseForm();
      } finally {
        setSubmitting(false);
      }
    },
    [onUpdate, onCloseForm],
  );

  const handleDeleteClick = useCallback((account: AccountView) => {
    setAccountToDelete(account);
    setDeleteDialogOpen(true);
  }, []);

  const handleDeleteConfirm = useCallback(async () => {
    if (!accountToDelete) return;
    await onDelete(accountToDelete.id);
  }, [accountToDelete, onDelete]);

  const handleOpenOfficial = useCallback(
    async (account: AccountView) => {
      if (!account.officialUrl) return;
      try {
        await openUrl(account.officialUrl);
        setOpenErrors((prev) => {
          if (!(account.id in prev)) return prev;
          const next = { ...prev };
          delete next[account.id];
          return next;
        });
      } catch (err) {
        setOpenErrors((prev) => ({
          ...prev,
          [account.id]: t("accounts.openFailed", { error: String(err) }),
        }));
      }
    },
    [t],
  );

  // Derive displayed accounts from the status filter and sort mode.
  const displayedAccounts = useMemo(() => {
    const filteredAccounts =
      statusFilter === "all"
        ? accounts
        : accounts.filter((account) =>
            statusFilter === "enabled" ? account.enabled : !account.enabled,
          );

    if (sortMode === "sort_order") {
      // Preserve backend order exactly
      return filteredAccounts;
    }

    // Sort by balance ascending (lowest first), null/undefined last.
    return [...filteredAccounts].sort((a, b) => {
      const aBalance =
        convertUsdCreditsToCny(
          a.lastRemaining,
          a.lastUnit,
          a.usdCreditsPerCny,
        ) ?? a.lastRemaining;
      const bBalance =
        convertUsdCreditsToCny(
          b.lastRemaining,
          b.lastUnit,
          b.usdCreditsPerCny,
        ) ?? b.lastRemaining;

      if (aBalance == null && bBalance == null) {
        // Tie-break by sortOrder descending, then name
        if (a.sortOrder !== b.sortOrder) {
          return b.sortOrder - a.sortOrder;
        }
        return a.name.localeCompare(b.name);
      }
      if (aBalance == null) return 1;
      if (bBalance == null) return -1;

      if (aBalance !== bBalance) {
        return aBalance - bBalance;
      }

      if (a.sortOrder !== b.sortOrder) {
        return b.sortOrder - a.sortOrder;
      }
      return a.name.localeCompare(b.name);
    });
  }, [accounts, sortMode, statusFilter]);

  const canDragReorder = sortMode === "sort_order" && statusFilter === "all";

  return (
    <div className="flex flex-1 flex-col gap-4 overflow-auto p-7">
      {loadError && (
        <Alert variant="destructive">
          <AlertDescription>{t("accounts.loadFailed", { error: loadError })}</AlertDescription>
        </Alert>
      )}

      {/* View Toggle Buttons */}
      {!loading && accounts.length > 0 && (
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-card px-4 py-3 shadow-xs">
          <div className="flex flex-wrap items-center gap-3">
            {/* Sort Mode Selector */}
            <div className="flex items-center gap-2">
              <Label htmlFor="account-sort" className="text-xs text-muted-foreground">
                {t("accounts.sortMode")}
              </Label>
              <Select
                id="account-sort"
                value={sortMode}
                onChange={(event) => setSortMode(event.target.value as AccountSortMode)}
                className="w-40"
              >
                <option value="sort_order">{t("accounts.sortBySortOrder")}</option>
                <option value="balance">{t("accounts.sortByBalance")}</option>
              </Select>
            </div>

            {/* Status Filter Selector */}
            <div className="flex items-center gap-2">
              <Label htmlFor="account-status" className="text-xs text-muted-foreground">
                {t("status")}
              </Label>
              <Select
                id="account-status"
                value={statusFilter}
                onChange={(event) => setStatusFilter(event.target.value as AccountStatusFilter)}
                className="w-36"
              >
                <option value="all">{t("accounts.allStatuses")}</option>
                <option value="enabled">{t("accounts.enabledOnly")}</option>
                <option value="disabled">{t("accounts.disabledOnly")}</option>
              </Select>
            </div>
          </div>

          {/* View Mode Buttons */}
          <div className="flex items-center rounded-md border bg-background p-0.5">
            <Tooltip content={t("accounts.listView")}>
              <Button
                variant={viewMode === "list" ? "primary" : "secondary"}
                size="icon"
                onClick={() => setViewMode("list")}
                title={t("accounts.listView")}
                aria-label={t("accounts.listView")}
              >
                <LayoutList />
              </Button>
            </Tooltip>
            <Tooltip content={t("accounts.tableView")}>
              <Button
                variant={viewMode === "table" ? "primary" : "secondary"}
                size="icon"
                onClick={() => setViewMode("table")}
                title={t("accounts.tableView")}
                aria-label={t("accounts.tableView")}
              >
                <Table2 />
              </Button>
            </Tooltip>
          </div>
        </div>
      )}

      {loading ? (
        <div className="flex flex-col gap-2">
          {[1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-24 w-full" />
          ))}
        </div>
      ) : accounts.length === 0 ? (
        <Empty className="min-h-[420px] border bg-card">
          <EmptyHeader>
            <EmptyMedia variant="icon"><Database /></EmptyMedia>
            <EmptyTitle>{t("accounts.empty")}</EmptyTitle>
            <EmptyDescription>{t("accounts.emptyHint")}</EmptyDescription>
          </EmptyHeader>
          <EmptyContent>
            <Button variant="primary" onClick={onOpenAdd}>
              <Plus data-icon="inline-start" />
              {t("accounts.add")}
            </Button>
          </EmptyContent>
        </Empty>
      ) : displayedAccounts.length === 0 ? (
        <Empty className="min-h-64 border bg-card">
          <EmptyHeader>
            <EmptyTitle>{t("accounts.filterEmpty")}</EmptyTitle>
          </EmptyHeader>
        </Empty>
      ) : viewMode === "list" ? (
        <AccountListView
          accounts={displayedAccounts}
          checkingId={checkingId}
          rowErrors={rowErrors}
          openErrors={openErrors}
          onCheck={onCheck}
          onOpenEdit={onOpenEdit}
          onDeleteClick={handleDeleteClick}
          onOpenOfficial={handleOpenOfficial}
          onReorder={onReorder}
          sortMode={sortMode}
          setSortMode={setSortMode}
          canDragReorder={canDragReorder}
          onDraggingChange={onDraggingChange}
          onSetEnabled={onSetEnabled}
        />
      ) : (
        <AccountTableView
          accounts={displayedAccounts}
          checkingId={checkingId}
          rowErrors={rowErrors}
          openErrors={openErrors}
          onCheck={onCheck}
          onOpenEdit={onOpenEdit}
          onDeleteClick={handleDeleteClick}
          onOpenOfficial={handleOpenOfficial}
          onSetEnabled={onSetEnabled}
        />
      )}

      <Sheet
        open={formOpen}
        onClose={onCloseForm}
        title={editing ? t("form.editAccount") : t("form.addAccount")}
        footer={
          <div className="flex justify-end gap-3">
            <Button
              type="button"
              variant="secondary"
              onClick={onCloseForm}
            >
              {t("cancel")}
            </Button>
            <Button
              type="submit"
              variant="primary"
              disabled={submitting}
              form="account-form"
            >
              {submitting
                ? t("saving")
                : editing
                  ? t("saveChanges")
                  : t("create")}
            </Button>
          </div>
        }
      >
        <AccountForm
          account={editing}
          onClose={onCloseForm}
          onCreate={handleCreate}
          onUpdate={handleUpdate}
          onLoadCredentials={onLoadCredentials}
        />
      </Sheet>

      <ConfirmDialog
        open={deleteDialogOpen}
        onClose={() => setDeleteDialogOpen(false)}
        onConfirm={handleDeleteConfirm}
        title={t("accounts.deleteTitle")}
        description={t("accounts.deleteDescription", {
          name: accountToDelete?.name ?? "",
        })}
        confirmText={t("accounts.deleteButton")}
        cancelText={t("cancel")}
        variant="danger"
      />
    </div>
  );
}

interface AccountActionsProps {
  account: AccountView;
  checkingId: string | null;
  onCheck: (account: AccountView) => void;
  onOpenEdit: (account: AccountView) => void;
  onDeleteClick: (account: AccountView) => void;
  onOpenOfficial: (account: AccountView) => void;
  onSetEnabled: (account: AccountView, enabled: boolean) => Promise<void>;
}

function AccountActions({
  account,
  checkingId,
  onCheck,
  onOpenEdit,
  onDeleteClick,
  onOpenOfficial,
  onSetEnabled,
}: AccountActionsProps) {
  const { t } = useI18n();

  return (
    <div className="flex items-center gap-1">
      <ScheduleToggle account={account} onSetEnabled={onSetEnabled} />

      <Tooltip content={t("accounts.checkTitle")}>
        <Button
          variant="ghost"
          size="icon"
          className="h-8 w-8"
          onClick={() => onCheck(account)}
          disabled={
            !account.hasCredentials || checkingId === account.id
          }
          title={
            account.hasCredentials
              ? t("accounts.checkTitle")
              : t("accounts.checkNoCredsTitle")
          }
        >
          {checkingId === account.id ? (
            <RefreshCw className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <RefreshCw className="h-3.5 w-3.5" />
          )}
        </Button>
      </Tooltip>

      {account.officialUrl && (
        <Tooltip content={t("accounts.openOfficial")}>
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8"
            onClick={() => onOpenOfficial(account)}
            title={t("accounts.openOfficial")}
          >
            <Globe2 className="h-3.5 w-3.5" />
          </Button>
        </Tooltip>
      )}

      <Tooltip content={t("edit")}>
        <Button
          variant="ghost"
          size="icon"
          className="h-8 w-8"
          onClick={() => onOpenEdit(account)}
          title={t("edit")}
        >
          <Pencil className="h-3.5 w-3.5" />
        </Button>
      </Tooltip>

      <Tooltip content={t("delete")}>
        <Button
          variant="ghost"
          size="icon"
          className="h-8 w-8 hover:text-rose-600"
          onClick={() => onDeleteClick(account)}
          title={t("delete")}
        >
          <Trash2 className="h-3.5 w-3.5" />
        </Button>
      </Tooltip>
    </div>
  );
}

interface ScheduleToggleProps {
  account: AccountView;
  onSetEnabled: (account: AccountView, enabled: boolean) => Promise<void>;
}

function ScheduleToggle({ account, onSetEnabled }: ScheduleToggleProps) {
  const { t } = useI18n();
  const [toggling, setToggling] = useState(false);

  const handleToggle = async () => {
    setToggling(true);
    try {
      await onSetEnabled(account, !account.enabled);
    } catch (err) {
      // Error is handled by parent component (sets rowErrors)
    } finally {
      setToggling(false);
    }
  };

  const title = account.enabled
    ? t("accounts.scheduleEnabledTitle")
    : t("accounts.scheduleDisabledTitle");

  return (
    <Tooltip content={t("accounts.scheduleToggle")}>
      <button
        type="button"
        role="switch"
        aria-checked={account.enabled}
        className={`relative inline-flex h-5 w-10 items-center rounded-full transition-colors disabled:opacity-50 disabled:cursor-not-allowed ${
          account.enabled
            ? "bg-emerald-500 hover:bg-emerald-600"
            : "bg-slate-300 hover:bg-slate-400"
        }`}
        onClick={handleToggle}
        disabled={toggling}
        title={title}
        aria-label={title}
      >
        <span
          className={`inline-flex h-4 w-4 items-center justify-center transform rounded-full bg-white shadow-sm transition-transform ${
            account.enabled ? "translate-x-5" : "translate-x-0.5"
          }`}
        >
          <Clock3 className="h-2.5 w-2.5 text-slate-600" />
        </span>
      </button>
    </Tooltip>
  );
}

function getAccountBorderColor(account: AccountView): {
  borderClass: string;
  bgClass: string;
} {
  const isFailed =
    account.lastResult === "failed" ||
    account.lastResult === "invalid_credential";
  const isLowBalance =
    account.lastResult === "low_balance" ||
    isBelowBalanceThreshold(account);

  if (isFailed) {
    return {
      borderClass: "border-l-rose-400",
      bgClass: "bg-rose-50/30",
    };
  } else if (isLowBalance) {
    return {
      borderClass: "border-l-amber-400",
      bgClass: "bg-amber-50/30",
    };
  }
  return {
    borderClass: "border-l-transparent",
    bgClass: "",
  };
}

function isBelowBalanceThreshold(account: AccountView): boolean {
  const remaining =
    convertUsdCreditsToCny(
      account.lastRemaining,
      account.lastUnit,
      account.usdCreditsPerCny,
    ) ?? account.lastRemaining;
  return (
    remaining != null &&
    account.balanceThreshold != null &&
    remaining < account.balanceThreshold
  );
}

interface AccountViewCommonProps {
  accounts: AccountView[];
  checkingId: string | null;
  rowErrors: Record<string, string>;
  openErrors: Record<string, string>;
  onCheck: (account: AccountView) => void;
  onOpenEdit: (account: AccountView) => void;
  onDeleteClick: (account: AccountView) => void;
  onOpenOfficial: (account: AccountView) => void;
  onReorder: (orderedIds: string[]) => Promise<void>;
  sortMode: AccountSortMode;
  setSortMode: (mode: AccountSortMode) => void;
  canDragReorder: boolean;
  onDraggingChange: (isDragging: boolean) => void;
  onSetEnabled: (account: AccountView, enabled: boolean) => Promise<void>;
}

type AccountTableViewProps = Omit<AccountViewCommonProps, "onReorder" | "sortMode" | "setSortMode" | "canDragReorder" | "onDraggingChange">;

function AccountListView({
  accounts,
  checkingId,
  rowErrors,
  openErrors,
  onCheck,
  onOpenEdit,
  onDeleteClick,
  onOpenOfficial,
  onReorder,
  sortMode,
  setSortMode,
  canDragReorder,
  onDraggingChange,
  onSetEnabled,
}: AccountViewCommonProps) {
  const { t } = useI18n();
  const [reorderError, setReorderError] = useState<string | null>(null);

  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: {
        distance: 8,
      },
    })
  );

  const handleDragStart = (_event: DragStartEvent) => {
    if (!canDragReorder) return;
    onDraggingChange(true);
  };

  const handleDragEnd = async (event: DragEndEvent) => {
    const { active, over } = event;

    onDraggingChange(false);

    if (!canDragReorder || !over || active.id === over.id) {
      return;
    }

    const oldIndex = accounts.findIndex((a) => a.id === active.id);
    const newIndex = accounts.findIndex((a) => a.id === over.id);

    if (oldIndex !== -1 && newIndex !== -1) {
      const newOrder = arrayMove(accounts, oldIndex, newIndex);
      const orderedIds = newOrder.map((a) => a.id);

      try {
        await onReorder(orderedIds);
        setReorderError(null);
      } catch (err) {
        setReorderError(t("accounts.reorderFailed", { error: String(err) }));
      }
    }
  };

  const handleDragCancel = () => {
    onDraggingChange(false);
  };

  const accountIds = useMemo(() => accounts.map((a) => a.id), [accounts]);

  return (
    <div className="space-y-2">
      {reorderError && (
        <div className="rounded-lg border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700">
          {reorderError}
        </div>
      )}
      <DndContext
        sensors={sensors}
        collisionDetection={closestCenter}
        onDragStart={handleDragStart}
        onDragEnd={handleDragEnd}
        onDragCancel={handleDragCancel}
      >
        <SortableContext items={accountIds} strategy={verticalListSortingStrategy}>
          {accounts.map((account) => (
            <SortableAccountItem
              key={account.id}
              account={account}
              checkingId={checkingId}
              rowErrors={rowErrors}
              openErrors={openErrors}
              onCheck={onCheck}
              onOpenEdit={onOpenEdit}
              onDeleteClick={onDeleteClick}
              onOpenOfficial={onOpenOfficial}
              sortMode={sortMode}
              setSortMode={setSortMode}
              canDragReorder={canDragReorder}
              onSetEnabled={onSetEnabled}
            />
          ))}
        </SortableContext>
      </DndContext>
    </div>
  );
}

interface SortableAccountItemProps {
  account: AccountView;
  checkingId: string | null;
  rowErrors: Record<string, string>;
  openErrors: Record<string, string>;
  onCheck: (account: AccountView) => void;
  onOpenEdit: (account: AccountView) => void;
  onDeleteClick: (account: AccountView) => void;
  onOpenOfficial: (account: AccountView) => void;
  sortMode: AccountSortMode;
  setSortMode: (mode: AccountSortMode) => void;
  canDragReorder: boolean;
  onSetEnabled: (account: AccountView, enabled: boolean) => Promise<void>;
}

function SortableAccountItem({
  account,
  checkingId,
  rowErrors,
  openErrors,
  onCheck,
  onOpenEdit,
  onDeleteClick,
  onOpenOfficial,
  sortMode,
  setSortMode,
  canDragReorder,
  onSetEnabled,
}: SortableAccountItemProps) {
  const { t } = useI18n();
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: account.id, disabled: !canDragReorder });

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
  };

  const { borderClass, bgClass } = getAccountBorderColor(account);
  const displayError = rowErrors[account.id] || openErrors[account.id];
  const convertedBalance = formatConvertedBalance(
    account.lastRemaining,
    account.lastUnit,
    account.usdCreditsPerCny,
  );

  const handleDragHandlePointerDown = () => {
    if (canDragReorder) return;

    if (sortMode !== "sort_order") {
      setSortMode("sort_order");
    }
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      className={`overflow-hidden rounded-lg border border-slate-200 bg-white shadow-sm transition-all hover:shadow-md ${
        isDragging ? "opacity-50 shadow-2xl z-10" : ""
      }`}
    >
      <div
        className={`flex flex-wrap items-center gap-x-4 gap-y-2 border-l-4 ${borderClass} px-4 py-2.5 ${bgClass}`}
      >
        {/* Drag Handle */}
        <div
          {...attributes}
          {...listeners}
          onPointerDownCapture={handleDragHandlePointerDown}
          className={`flex h-8 w-8 flex-shrink-0 items-center justify-center rounded hover:bg-slate-100 ${
            canDragReorder
              ? "cursor-grab active:cursor-grabbing"
              : "cursor-not-allowed opacity-50"
          }`}
          title={
            canDragReorder
              ? t("accounts.dragToReorder")
              : t("accounts.dragOnlyInAllStatusSortMode")
          }
        >
          <GripVertical className="h-5 w-5 text-slate-400" />
        </div>

        {/* Left: Status Icon + Name + Badges + Context */}
        <div className="flex min-w-0 flex-1 items-start gap-3">
          <div className="mt-0.5 flex-shrink-0">
            {account.lastResult ? (
              resultIcon(account.lastResult)
            ) : (
              <div className="h-4 w-4" />
            )}
          </div>

          <div className="min-w-0 flex-1">
            {/* Name + Badges */}
            <div className="flex items-center gap-2 flex-wrap">
              <span className="truncate font-semibold text-slate-900 text-base">
                {account.name}
              </span>
              {!account.enabled && (
                <Badge variant="neutral" className="text-[10px] px-1.5 py-0">
                  {t("accounts.disabledBadge")}
                </Badge>
              )}
              {account.enabled && !account.hasCredentials && (
                <Badge variant="warning" className="text-[10px] px-1.5 py-0">
                  {t("accounts.noCredentials")}
                </Badge>
              )}
            </div>

            {/* Context: Provider, Base URL, Threshold, Plan, Sort Order */}
            <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-slate-600">
              <span className="flex items-center gap-1">
                <Badge variant="neutral" className="text-[10px] px-1.5 py-0">
                  {t(`provider.${account.provider}`)}
                </Badge>
              </span>
              <span className="min-w-0 truncate">{account.baseUrl}</span>
              {account.balanceThreshold != null && (
                <span className="flex-shrink-0 text-slate-500">
                  {t(
                    account.usdCreditsPerCny != null
                      ? "accounts.convertedThreshold"
                      : "accounts.threshold",
                    {
                    value: account.balanceThreshold,
                    },
                  )}
                </span>
              )}
              {account.usdCreditsPerCny != null && (
                <span className="flex-shrink-0 text-slate-500">
                  {t("accounts.creditRatio", {
                    rate: account.usdCreditsPerCny,
                  })}
                </span>
              )}
              {account.lastPlanName && (
                <span className="min-w-0 truncate text-slate-500">
                  {t("accounts.plan", { name: account.lastPlanName })}
                </span>
              )}
            </div>

            {/* Note */}
            {account.note && (
              <div className="mt-1 text-xs text-slate-500 line-clamp-2">
                {account.note}
              </div>
            )}
          </div>
        </div>

        {/* Right: Compact Balance Info + Actions */}
        <div className="flex flex-shrink-0 items-center gap-3">
          {/* Balance Info */}
          {account.lastRemaining != null ? (
            <div className="flex flex-col items-end gap-0.5 text-right text-xs">
              {/* Line 1: Last Checked Time (small, muted) */}
              <div className="text-[11px] text-slate-400">
                {account.lastCheckedAt
                  ? formatTime(account.lastCheckedAt)
                  : t("time.never")}
              </div>

              {/* Line 2: Used + Remaining (horizontal, compact) */}
              <div className="flex items-center gap-2 whitespace-nowrap text-xs">
                {account.lastUsed != null && (
                  <span className="text-slate-600">
                    {t("accounts.usedStat")}:{" "}
                    <span className="text-slate-700">
                      {formatMoney(account.lastUsed, account.lastUnit)}
                    </span>
                  </span>
                )}
                <span className="text-slate-600">
                  {t("accounts.remainingStat")}:{" "}
                  <span
                    className={`font-semibold ${
                      account.lastResult === "failed" ||
                      account.lastResult === "invalid_credential"
                        ? "text-rose-600"
                        : account.lastResult === "low_balance" ||
                          isBelowBalanceThreshold(account)
                        ? "text-amber-600"
                        : account.lastRemaining > 0
                        ? "text-emerald-600"
                        : "text-slate-500"
                    }`}
                  >
                    {formatMoney(account.lastRemaining, account.lastUnit)}
                  </span>
                </span>
              </div>

              {convertedBalance != null && (
                <div className="text-[11px] font-medium text-slate-500">
                  {t("accounts.actualValue", {
                    amount: convertedBalance,
                  })}
                </div>
              )}

              {/* Line 3: Total (optional, small) */}
              {account.lastTotal != null && (
                <div className="text-[11px] text-slate-500">
                  {t("accounts.totalStat")}:{" "}
                  <span className="text-slate-600">
                    {formatMoney(account.lastTotal, account.lastUnit)}
                  </span>
                </div>
              )}
            </div>
          ) : (
            <div className="text-xs text-slate-400">
              {t("accounts.noBalanceData")}
            </div>
          )}

          {/* Actions */}
          <AccountActions
            account={account}
            checkingId={checkingId}
            onCheck={onCheck}
            onOpenEdit={onOpenEdit}
            onDeleteClick={onDeleteClick}
            onOpenOfficial={onOpenOfficial}
            onSetEnabled={onSetEnabled}
          />
        </div>
      </div>

      {/* Error Row */}
      {displayError && (
        <div className="border-t border-rose-200 bg-rose-50 px-4 py-2 text-xs text-rose-700">
          {displayError}
        </div>
      )}
    </div>
  );
}

function AccountTableView({
  accounts,
  checkingId,
  rowErrors,
  openErrors,
  onCheck,
  onOpenEdit,
  onDeleteClick,
  onOpenOfficial,
  onSetEnabled,
}: AccountTableViewProps) {
  const { t, resultLabel } = useI18n();

  return (
    <div className="overflow-hidden rounded-lg border border-slate-200 bg-white shadow-sm">
      {/* Table Header */}
      <div className="grid grid-cols-[2fr_1fr_1fr_1.5fr_1fr_auto] gap-4 border-b border-slate-200 bg-slate-50 px-4 py-3 text-xs font-semibold uppercase tracking-wide text-slate-600">
        <div>{t("account")}</div>
        <div>{t("provider")}</div>
        <div>{t("status")}</div>
        <div>{t("accounts.remainingStat")}</div>
        <div>{t("accounts.lastChecked")}</div>
        <div className="w-36 text-right">{t("accounts.actions")}</div>
      </div>

      {/* Table Body */}
      <div className="divide-y divide-slate-100">
        {accounts.map((account) => {
          const { borderClass, bgClass } = getAccountBorderColor(account);
          const displayError = rowErrors[account.id] || openErrors[account.id];

          return (
            <div
              key={account.id}
              className={`grid grid-cols-[2fr_1fr_1fr_1.5fr_1fr_auto] gap-4 border-l-2 ${borderClass} px-4 py-3 transition-colors hover:bg-slate-50 ${bgClass}`}
            >
              {/* Account Name & Base URL */}
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="truncate font-medium text-slate-900">
                    {account.name}
                  </span>
                  {!account.enabled && (
                    <Badge variant="neutral" className="text-[10px] px-1.5 py-0">
                      {t("accounts.disabledBadge")}
                    </Badge>
                  )}
                  {account.enabled && !account.hasCredentials && (
                    <Badge variant="warning" className="text-[10px] px-1.5 py-0">
                      {t("accounts.noCredentials")}
                    </Badge>
                  )}
                </div>
                <div className="mt-0.5 truncate text-xs text-slate-500">
                  {account.baseUrl}
                </div>
                {/* Note */}
                {account.note && (
                  <div className="mt-0.5 text-[11px] text-slate-400 truncate">
                    {account.note}
                  </div>
                )}
              </div>

              {/* Provider */}
              <div className="flex items-center">
                <Badge variant="neutral" className="text-xs">
                  {t(`provider.${account.provider}`)}
                </Badge>
              </div>

              {/* Status */}
              <div className="flex items-center text-xs">
                {account.lastResult ? (
                  <div className="flex items-center gap-1.5">
                    {resultIcon(account.lastResult)}
                    <span className="text-slate-700">
                      {resultLabel(account.lastResult)}
                    </span>
                  </div>
                ) : (
                  <span className="text-slate-400">—</span>
                )}
              </div>

              {/* Remaining Balance */}
              <div className="flex items-center">
                <RemainingStat account={account} />
              </div>

              {/* Last Checked */}
              <div className="flex items-center text-xs text-slate-600">
                {account.lastCheckedAt ? (
                  formatTime(account.lastCheckedAt)
                ) : (
                  <span className="text-slate-400">{t("time.never")}</span>
                )}
              </div>

              {/* Actions */}
              <div className="flex items-center justify-end">
                <AccountActions
                  account={account}
                  checkingId={checkingId}
                  onCheck={onCheck}
                  onOpenEdit={onOpenEdit}
                  onDeleteClick={onDeleteClick}
                  onOpenOfficial={onOpenOfficial}
                  onSetEnabled={onSetEnabled}
                />
              </div>

              {/* Error Row */}
              {displayError && (
                <div className="col-span-6 -mt-1 rounded border border-rose-200 bg-rose-50 px-3 py-2 text-xs text-rose-700">
                  {displayError}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function RemainingStat({
  account,
  prominent = false,
}: {
  account: AccountView;
  prominent?: boolean;
}) {
  const { t } = useI18n();
  const convertedBalance = formatConvertedBalance(
    account.lastRemaining,
    account.lastUnit,
    account.usdCreditsPerCny,
  );
  const belowThreshold = isBelowBalanceThreshold(account);

  let textColor: string;
  let borderColor: string;
  if (
    account.lastResult === "failed" ||
    account.lastResult === "invalid_credential"
  ) {
    textColor = "text-rose-600";
    borderColor = "border-rose-300";
  } else if (account.lastResult === "low_balance" || belowThreshold) {
    textColor = "text-amber-600";
    borderColor = "border-amber-300";
  } else if (account.lastRemaining != null && account.lastRemaining > 0) {
    textColor = "text-emerald-600";
    borderColor = "border-emerald-300";
  } else {
    textColor = "text-slate-500";
    borderColor = "border-slate-300";
  }

  if (prominent) {
    return (
      <div className="text-center">
        <div className={`inline-flex items-center gap-1.5 ${textColor}`}>
          <Wallet className="h-4 w-4 flex-shrink-0" />
          <div className="min-w-0">
            <div className="text-lg font-bold">
              {formatMoney(account.lastRemaining, account.lastUnit)}
            </div>
            {convertedBalance && (
              <div className="text-xs font-semibold text-slate-500">
                {convertedBalance}
              </div>
            )}
            <div className="text-[10px] font-medium uppercase tracking-wide text-slate-500">
              {t("accounts.remainingStat")}
            </div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className={`inline-flex items-center gap-1.5 rounded border ${borderColor} bg-white px-2 py-1 ${textColor}`}>
      <Wallet className="h-3.5 w-3.5 flex-shrink-0" />
      <span className="min-w-0 text-sm font-semibold">
        <span className="block">
          {formatMoney(account.lastRemaining, account.lastUnit)}
        </span>
        {convertedBalance && (
          <span className="block text-[10px] font-medium text-slate-500">
            {convertedBalance}
          </span>
        )}
      </span>
    </div>
  );
}
