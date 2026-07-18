import { useCallback, useEffect, useState } from "react";
import {
  AlarmClock,
  CheckCircle2,
  Clock3,
  KeyRound,
  RefreshCw,
  Settings2,
} from "lucide-react";
import { Button } from "./ui/button";
import { Card, CardContent, CardHeader } from "./ui/card";
import { Badge } from "./ui/badge";
import * as api from "../lib/api";
import type { ScheduleOverview, ScheduleRow, ScheduleStatus } from "../types";
import { formatInterval, formatTime } from "../lib/format";
import { useI18n } from "../lib/i18n";

/** Compact metric tile, matching the Overview page tiles. */
function MetricCard({
  icon: Icon,
  label,
  value,
  hint,
  tone = "neutral",
}: {
  icon: typeof Clock3;
  label: string;
  value: string;
  hint?: string;
  tone?: "neutral" | "attention";
}) {
  const iconClasses =
    tone === "attention"
      ? "bg-amber-50 text-amber-700"
      : "bg-cyan-50 text-cyan-700";
  return (
    <Card>
      <CardContent>
        <div className="flex items-center justify-between">
          <div className="min-w-0">
            <div className="truncate text-sm text-slate-500">{label}</div>
            <div className="mt-2 text-2xl font-semibold tracking-normal">
              {value}
            </div>
            {hint && <div className="mt-1 truncate text-xs text-slate-400">{hint}</div>}
          </div>
          <div
            className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-md ${iconClasses}`}
          >
            <Icon className="h-5 w-5" />
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

/** A colored badge for a schedule status. */
function statusBadge(status: ScheduleStatus, label: string) {
  switch (status) {
    case "due":
      return <Badge variant="info">{label}</Badge>;
    case "scheduled":
      return <Badge variant="success">{label}</Badge>;
    case "missing_credentials":
      return <Badge variant="warning">{label}</Badge>;
    case "disabled":
    default:
      return <Badge variant="neutral">{label}</Badge>;
  }
}

/** How this row describes its per-account interval relative to the default. */
function intervalLabel(
  row: ScheduleRow,
  t: ReturnType<typeof useI18n>["t"],
  locale: ReturnType<typeof useI18n>["locale"],
) {
  const usesOverride =
    row.checkIntervalMinutes != null && row.checkIntervalMinutes >= 1;
  const effective = formatInterval(row.effectiveIntervalMinutes, locale);
  return usesOverride
    ? t("schedules.accountInterval", { value: effective })
    : t("schedules.globalInterval", { value: effective });
}

export function SchedulesView() {
  const { t, locale, providerLabel, scheduleStatusLabel } = useI18n();
  const [overview, setOverview] = useState<ScheduleOverview | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const ov = await api.getScheduleOverview();
      setOverview(ov);
      setError(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  if (loading) {
    return (
      <div className="flex-1 overflow-auto p-6 text-sm text-slate-500">
        {t("schedules.loading")}
      </div>
    );
  }

  if (!overview) {
    return (
      <div className="min-w-0 flex-1 space-y-4 overflow-auto p-6">
        {error && (
          <p className="rounded-md border border-rose-200 bg-rose-50 px-3 py-2 text-sm text-rose-700">
            {error}
          </p>
        )}
      </div>
    );
  }

  const defaultLabel = formatInterval(overview.defaultCheckIntervalMinutes, locale);

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-5 overflow-auto p-7">
      <div className="flex items-center justify-between gap-3">
        <p className="text-sm text-slate-500">
          {t("schedules.description")}
        </p>
        <Button variant="secondary" size="sm" onClick={() => void refresh()}>
          <RefreshCw className="h-4 w-4" />
          {t("refresh")}
        </Button>
      </div>

      {error && (
        <p className="rounded-md border border-rose-200 bg-rose-50 px-3 py-2 text-sm text-rose-700">
          {error}
        </p>
      )}

      <div className="grid gap-4 lg:grid-cols-4">
        <MetricCard
          icon={CheckCircle2}
          label={t("enabled")}
          value={String(overview.enabledAccounts)}
          hint={t("schedules.disabledCount", {
            count: overview.disabledAccounts,
          })}
        />
        <MetricCard
          icon={Clock3}
          label={t("schedules.scheduled")}
          value={String(overview.schedulableAccounts)}
          hint={t("schedules.schedulableHint")}
        />
        <MetricCard
          icon={KeyRound}
          label={t("schedules.missingCredentials")}
          value={String(overview.unschedulableAccounts)}
          hint={t("schedules.missingHint")}
          tone={overview.unschedulableAccounts > 0 ? "attention" : "neutral"}
        />
        <MetricCard
          icon={AlarmClock}
          label={t("schedules.defaultInterval")}
          value={defaultLabel}
          hint={t("schedules.settingsHint")}
        />
      </div>

      <Card>
        <CardHeader>
          <div className="flex items-center justify-between gap-3">
            <div>
              <h2 className="text-sm font-semibold">{t("schedules.accountSchedules")}</h2>
              <p className="text-sm text-slate-500">
                {t("schedules.accountSchedulesHint")}
              </p>
            </div>
            <div className="flex items-center gap-1.5 text-xs text-slate-400">
              <Settings2 className="h-3.5 w-3.5" />
              {t("schedules.default", { value: defaultLabel })}
            </div>
          </div>
        </CardHeader>
        <CardContent className="p-0">
          {overview.rows.length === 0 ? (
            <div className="px-4 py-8 text-center text-sm text-slate-500">
              {t("schedules.empty")}
            </div>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-slate-100 text-left text-xs text-slate-400">
                    <th className="px-4 py-2 font-medium">{t("account")}</th>
                    <th className="px-4 py-2 font-medium">{t("schedules.interval")}</th>
                    <th className="px-4 py-2 font-medium">{t("schedules.lastChecked")}</th>
                    <th className="px-4 py-2 font-medium">{t("schedules.nextCheck")}</th>
                    <th className="px-4 py-2 font-medium">{t("status")}</th>
                  </tr>
                </thead>
                <tbody>
                  {overview.rows.map((row) => (
                    <tr
                      key={row.id}
                      className="border-b border-slate-50 last:border-b-0"
                    >
                      <td className="px-4 py-3">
                        <div className="truncate font-medium text-slate-800">
                          {row.name}
                        </div>
                        <div className="mt-0.5 text-xs text-slate-500">
                          {providerLabel(row.provider)}
                        </div>
                      </td>
                      <td className="px-4 py-3 text-slate-700">
                        {intervalLabel(row, t, locale)}
                      </td>
                      <td className="px-4 py-3 text-slate-500">
                        {formatTime(row.lastCheckedAt, locale)}
                      </td>
                      <td className="px-4 py-3 text-slate-500">
                        {row.nextCheckAt == null
                          ? "—"
                          : row.dueNow
                            ? t("schedules.dueNow")
                            : formatTime(row.nextCheckAt, locale)}
                      </td>
                      <td className="px-4 py-3">
                        {statusBadge(row.status, scheduleStatusLabel(row.status))}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
