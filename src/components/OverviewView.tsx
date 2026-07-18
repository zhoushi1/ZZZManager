import { useCallback, useEffect, useState } from "react";
import {
  AlertTriangle,
  Bell,
  CheckCircle2,
  Database,
  Gauge,
  HelpCircle,
  History as HistoryIcon,
  XCircle,
} from "lucide-react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "./ui/card";
import { Alert, AlertDescription } from "./ui/alert";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "./ui/empty";
import { Separator } from "./ui/separator";
import { Skeleton } from "./ui/skeleton";
import * as api from "../lib/api";
import type { HistoryEntry, Overview } from "../types";
import {
  formatConvertedBalance,
  formatMoney,
  formatTime,
  resultIcon,
} from "../lib/format";
import { useI18n } from "../lib/i18n";
import { cn } from "../lib/utils";

function MetricTile({
  icon: Icon,
  label,
  value,
  tone = "neutral",
}: {
  icon: typeof Gauge;
  label: string;
  value: string;
  tone?: "neutral" | "attention";
}) {
  const iconClasses = tone === "attention"
    ? "bg-warning-muted text-warning-foreground"
    : "bg-info-muted text-info";
  return (
    <div className="flex min-w-0 items-center justify-between gap-4 px-5 py-4">
      <div className="min-w-0">
        <div className="truncate text-xs font-medium text-muted-foreground">{label}</div>
        <div className="mt-1 text-2xl font-semibold tabular-nums">{value}</div>
      </div>
      <div className={cn("flex size-9 shrink-0 items-center justify-center rounded-md", iconClasses)}>
        <Icon className="size-4" />
      </div>
    </div>
  );
}

function ConvertedRecentValue({ item }: { item: HistoryEntry }) {
  const { t } = useI18n();
  const converted = formatConvertedBalance(
    item.remaining,
    item.unit,
    item.usdCreditsPerCny,
  );

  return converted ? (
    <p className="mt-0.5 text-xs font-medium text-muted-foreground">
      {t("accounts.actualValue", { amount: converted })}
    </p>
  ) : null;
}

type OverviewViewProps = {
  onHeaderRefreshChange?: (
    action: { busy: boolean; refresh: () => void } | null,
  ) => void;
};

export function OverviewView({ onHeaderRefreshChange }: OverviewViewProps) {
  const { t, locale, eventLabel, resultLabel } = useI18n();
  const [overview, setOverview] = useState<Overview | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setRefreshing(true);
    try {
      const ov = await api.getOverview();
      setOverview(ov);
      setError(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, []);

  useEffect(() => {
    onHeaderRefreshChange?.({
      busy: loading || refreshing,
      refresh: () => void refresh(),
    });

    return () => {
      onHeaderRefreshChange?.(null);
    };
  }, [loading, onHeaderRefreshChange, refresh, refreshing]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  if (loading) {
    return (
      <div className="flex flex-1 flex-col gap-5 overflow-auto p-7">
        <Card>
          <CardContent className="grid grid-cols-4 divide-x p-0">
            {[0, 1, 2, 3].map((item) => (
              <div key={item} className="flex items-center justify-between gap-4 px-5 py-4">
                <div className="flex flex-1 flex-col gap-2">
                  <Skeleton className="h-3 w-24" />
                  <Skeleton className="h-7 w-16" />
                </div>
                <Skeleton className="size-9" />
              </div>
            ))}
          </CardContent>
        </Card>
        <div className="grid gap-5 xl:grid-cols-[minmax(260px,0.7fr)_minmax(0,1.3fr)]">
          <Skeleton className="h-72 w-full" />
          <Skeleton className="h-72 w-full" />
        </div>
      </div>
    );
  }

  if (!overview) {
    return (
      <div className="min-w-0 flex-1 overflow-auto p-7">
        {error && (
          <Alert variant="destructive">
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        )}
      </div>
    );
  }

  const attention =
    overview.lowBalanceCount +
    overview.failedCount +
    overview.invalidCredentialCount;

  // Prefer USD (the built-in providers' unit) for the headline remaining tile,
  // falling back to the first available unit.
  const primaryRemaining =
    overview.remainingByUnit.find((r) => r.unit === "USD") ??
    overview.remainingByUnit[0] ??
    null;

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-5 overflow-auto p-7">
      {error && (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      <Card>
        <CardContent className="grid grid-cols-4 divide-x p-0">
        <MetricTile
          icon={Database}
          label={t("accounts")}
          value={String(overview.totalAccounts)}
        />
        <MetricTile
          icon={CheckCircle2}
          label={t("enabled")}
          value={String(overview.enabledAccounts)}
        />
        <MetricTile
          icon={Bell}
          label={t("overview.needsAttention")}
          value={String(attention)}
          tone={attention > 0 ? "attention" : "neutral"}
        />
        <MetricTile
          icon={Gauge}
          label={
            primaryRemaining
              ? t("overview.totalRemainingUnit", { unit: primaryRemaining.unit })
              : t("overview.totalRemaining")
          }
          value={primaryRemaining ? primaryRemaining.total.toFixed(2) : "—"}
        />
        </CardContent>
      </Card>

      {overview.remainingByUnit.length > 1 && (
        <Card>
          <CardHeader>
            <CardTitle>{t("overview.remainingByUnit")}</CardTitle>
          </CardHeader>
          <CardContent className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {overview.remainingByUnit.map((r) => (
              <div
                key={r.unit}
                className="rounded-md border bg-muted/25 px-3 py-2.5"
              >
                <div className="text-xs text-muted-foreground">{r.unit}</div>
                <div className="mt-0.5 font-medium tabular-nums">
                  {r.total.toFixed(2)} {r.unit}
                </div>
                <div className="text-xs text-muted-foreground">
                  {t("overview.accountCount", {
                    count: r.accountCount,
                    plural: r.accountCount === 1 ? "" : "s",
                  })}
                </div>
              </div>
            ))}
          </CardContent>
        </Card>
      )}

      <div className="grid gap-5 xl:grid-cols-[minmax(260px,0.7fr)_minmax(0,1.3fr)]">
        <Card>
          <CardHeader>
            <CardTitle>{t("overview.attentionBreakdown")}</CardTitle>
            <CardDescription>{t("overview.attentionHint")}</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            <AttentionRow
              icon={<AlertTriangle className="size-4 text-warning" />}
              label={t("overview.lowBalance")}
              count={overview.lowBalanceCount}
            />
            <AttentionRow
              icon={<XCircle className="size-4 text-destructive" />}
              label={t("overview.checkFailed")}
              count={overview.failedCount}
            />
            <AttentionRow
              icon={<XCircle className="size-4 text-destructive" />}
              label={t("overview.invalidCredential")}
              count={overview.invalidCredentialCount}
            />
            <AttentionRow
              icon={<HelpCircle className="size-4 text-muted-foreground" />}
              label={t("overview.unchecked")}
              count={overview.uncheckedAccounts}
            />
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>{t("overview.recentChecks")}</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-0 py-1">
            {overview.recentChecks.length === 0 ? (
              <Empty className="min-h-44 border-0">
                <EmptyHeader>
                  <EmptyMedia variant="icon"><HistoryIcon /></EmptyMedia>
                  <EmptyTitle>{t("overview.noChecks")}</EmptyTitle>
                </EmptyHeader>
              </Empty>
            ) : (
              overview.recentChecks.map((item, index) => (
                <div key={item.id}>
                <div className="flex gap-3 py-3">
                  <div className="mt-0.5">{resultIcon(item.result)}</div>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center justify-between gap-3">
                      <span className="truncate text-sm font-medium">
                        {item.accountName ?? t("account")}
                      </span>
                      <span className="shrink-0 text-xs text-muted-foreground">
                        {formatTime(item.checkedAt, locale)}
                      </span>
                    </div>
                    <p className="mt-1 truncate text-sm leading-5 text-muted-foreground">
                      {item.message ??
                        (item.remaining != null
                          ? t("overview.remaining", {
                              amount: formatMoney(item.remaining, item.unit),
                            })
                          : resultLabel(item.result))}
                    </p>
                    <ConvertedRecentValue item={item} />
                  </div>
                </div>
                {index < overview.recentChecks.length - 1 && <Separator />}
                </div>
              ))
            )}
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>{t("overview.recentNotifications")}</CardTitle>
          <CardDescription>{t("overview.notificationsHint")}</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-0 py-1">
          {overview.recentDeliveries.length === 0 ? (
            <Empty className="min-h-40 border-0">
              <EmptyHeader>
                <EmptyMedia variant="icon"><Bell /></EmptyMedia>
                <EmptyTitle>{t("overview.noNotifications")}</EmptyTitle>
                <EmptyDescription>{t("overview.notificationsHint")}</EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            overview.recentDeliveries.map((d, index) => (
              <div key={d.id}>
              <div className="flex gap-3 py-3">
                <div className="mt-0.5">
                  {d.success ? (
                    <CheckCircle2 className="size-4 text-success" />
                  ) : (
                    <XCircle className="size-4 text-destructive" />
                  )}
                </div>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center justify-between gap-3">
                    <span className="truncate text-sm font-medium">
                      {eventLabel(d.eventType)}
                    </span>
                    <span className="shrink-0 text-xs text-muted-foreground">
                      {formatTime(d.createdAt, locale)}
                    </span>
                  </div>
                  <p className="mt-1 truncate text-sm leading-5 text-muted-foreground">
                    {d.errorMessage ??
                      (d.responseStatus != null
                        ? `HTTP ${d.responseStatus}`
                        : t("overview.delivered"))}
                  </p>
                </div>
              </div>
              {index < overview.recentDeliveries.length - 1 && <Separator />}
              </div>
            ))
          )}
        </CardContent>
      </Card>
    </div>
  );
}

function AttentionRow({
  icon,
  label,
  count,
}: {
  icon: React.ReactNode;
  label: string;
  count: number;
}) {
  return (
    <div className="flex items-center justify-between rounded-md bg-muted/35 px-3 py-2.5">
      <div className="flex items-center gap-2 text-sm">
        {icon}
        {label}
      </div>
      <span className={cn("text-sm font-semibold tabular-nums", count === 0 && "text-muted-foreground")}>
        {count}
      </span>
    </div>
  );
}
