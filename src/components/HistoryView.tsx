import { useCallback, useEffect, useMemo, useState } from "react";
import { RefreshCw } from "lucide-react";
import { Button } from "./ui/button";
import { Card, CardContent, CardHeader } from "./ui/card";
import { Label } from "./ui/input";
import { Select } from "./ui/select";
import * as api from "../lib/api";
import type { AccountView, BalanceResult, HistoryEntry, Provider } from "../types";
import {
  formatConvertedBalance,
  formatMoney,
  formatTime,
  resultBadge,
} from "../lib/format";
import { useI18n } from "../lib/i18n";

const RESULT_OPTIONS: BalanceResult[] = [
  "healthy",
  "low_balance",
  "failed",
  "invalid_credential",
];

const PROVIDER_OPTIONS: Provider[] = ["new_api", "sub2api", "custom_http"];

const LIMIT_OPTIONS = [50, 100, 250, 500];

export function HistoryView() {
  const { t, locale, providerLabel, resultLabel } = useI18n();
  const [accounts, setAccounts] = useState<AccountView[]>([]);
  const [rows, setRows] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Filter state. Empty string means "no filter".
  const [accountId, setAccountId] = useState<string>("");
  const [result, setResult] = useState<BalanceResult | "">("");
  const [provider, setProvider] = useState<Provider | "">("");
  const [limit, setLimit] = useState<number>(100);

  const accountName = useMemo(() => {
    const map = new Map(accounts.map((a) => [a.id, a.name]));
    return (row: HistoryEntry) =>
      row.accountName ?? map.get(row.accountId) ?? t("account");
  }, [accounts, t]);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const hist = await api.queryHistory({
        accountId: accountId || null,
        result: result || null,
        provider: provider || null,
        limit,
      });
      setRows(hist);
      setError(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [accountId, result, provider, limit]);

  // Load accounts once for the account filter dropdown.
  useEffect(() => {
    api
      .listAccounts()
      .then(setAccounts)
      .catch((err) => setError(String(err)));
  }, []);

  // Re-query whenever a filter changes.
  useEffect(() => {
    void load();
  }, [load]);

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-5 overflow-auto p-7">
      <Card>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            <div>
              <Label htmlFor="filter-account">{t("account")}</Label>
              <Select
                id="filter-account"
                value={accountId}
                onChange={(e) => setAccountId(e.target.value)}
              >
                <option value="">{t("history.allAccounts")}</option>
                {accounts.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </Select>
            </div>
            <div>
              <Label htmlFor="filter-result">{t("result")}</Label>
              <Select
                id="filter-result"
                value={result}
                onChange={(e) => setResult(e.target.value as BalanceResult | "")}
              >
                <option value="">{t("history.allResults")}</option>
                {RESULT_OPTIONS.map((r) => (
                  <option key={r} value={r}>
                    {resultLabel(r)}
                  </option>
                ))}
              </Select>
            </div>
            <div>
              <Label htmlFor="filter-provider">{t("provider")}</Label>
              <Select
                id="filter-provider"
                value={provider}
                onChange={(e) => setProvider(e.target.value as Provider | "")}
              >
                <option value="">{t("history.allProviders")}</option>
                {PROVIDER_OPTIONS.map((p) => (
                  <option key={p} value={p}>
                    {providerLabel(p)}
                  </option>
                ))}
              </Select>
            </div>
            <div>
              <Label htmlFor="filter-limit">{t("history.limit")}</Label>
              <Select
                id="filter-limit"
                value={String(limit)}
                onChange={(e) => setLimit(Number(e.target.value))}
              >
                {LIMIT_OPTIONS.map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </Select>
            </div>
          </div>
        </CardContent>
      </Card>

      {error && (
        <p className="rounded-md border border-rose-200 bg-rose-50 px-3 py-2 text-sm text-rose-700">
          {error}
        </p>
      )}

      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-sm font-semibold">{t("history.title")}</h2>
              <p className="text-sm text-slate-500">
                {loading
                  ? t("history.loading")
                  : t("history.rows", {
                      count: rows.length,
                      plural: rows.length === 1 ? "" : "s",
                    })}
              </p>
            </div>
            <Button variant="secondary" size="sm" onClick={() => void load()}>
              <RefreshCw className={`h-4 w-4 ${loading ? "animate-spin" : ""}`} />
              {t("refresh")}
            </Button>
          </div>
        </CardHeader>
        <CardContent className="p-0">
          {!loading && rows.length === 0 ? (
            <div className="px-4 py-8 text-center text-sm text-slate-500">
              {t("history.empty")}
            </div>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full min-w-[720px] table-fixed border-collapse text-sm">
                <thead>
                  <tr className="border-b border-slate-200 text-left text-xs font-medium text-slate-500">
                    <th className="w-[168px] px-4 py-2.5">{t("time")}</th>
                    <th className="w-[160px] px-4 py-2.5">{t("account")}</th>
                    <th className="w-[96px] px-4 py-2.5">{t("provider")}</th>
                    <th className="w-[132px] px-4 py-2.5">{t("result")}</th>
                    <th className="w-[132px] px-4 py-2.5">{t("accounts.remainingStat")}</th>
                    <th className="px-4 py-2.5">{t("message")}</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((row) => (
                    <tr
                      key={row.id}
                      className="border-b border-slate-100 last:border-b-0 align-top"
                    >
                      <td className="px-4 py-3 text-xs text-slate-500">
                        {formatTime(row.checkedAt, locale)}
                      </td>
                      <td className="truncate px-4 py-3 font-medium text-slate-800">
                        {accountName(row)}
                      </td>
                      <td className="px-4 py-3 text-slate-600">
                        {providerLabel(row.provider)}
                      </td>
                      <td className="px-4 py-3">{resultBadge(row.result, resultLabel)}</td>
                      <td className="px-4 py-3 text-slate-700">
                        <div>{formatMoney(row.remaining, row.unit)}</div>
                        <ConvertedHistoryValue row={row} />
                        <div className="text-xs text-slate-400">
                          {row.used != null || row.total != null
                            ? t("history.usedTotal", {
                                used: formatMoney(row.used, row.unit),
                                total: formatMoney(row.total, row.unit),
                              })
                            : ""}
                        </div>
                      </td>
                      <td className="px-4 py-3 text-slate-600">
                        <div className="truncate" title={row.message ?? undefined}>
                          {row.message ?? "—"}
                        </div>
                        {row.planName && (
                          <div className="truncate text-xs text-slate-400">
                            {t("history.plan", { name: row.planName })}
                          </div>
                        )}
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

function ConvertedHistoryValue({ row }: { row: HistoryEntry }) {
  const { t } = useI18n();
  const converted = formatConvertedBalance(
    row.remaining,
    row.unit,
    row.usdCreditsPerCny,
  );

  return converted ? (
    <div className="text-xs font-medium text-slate-500">
      {t("accounts.actualValue", { amount: converted })}
    </div>
  ) : null;
}
