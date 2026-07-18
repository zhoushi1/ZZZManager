import {
  AlertTriangle,
  CheckCircle2,
  HelpCircle,
  XCircle,
} from "lucide-react";
import { Badge } from "../components/ui/badge";
import type { BalanceResult } from "../types";
import type { Locale } from "./i18n";

/** A colored status badge for a balance check result (or unchecked when null). */
export function resultBadge(
  result: BalanceResult | null,
  label: (result: BalanceResult | null) => string = (r) => {
    switch (r) {
      case "healthy":
        return "Healthy";
      case "low_balance":
        return "Low balance";
      case "invalid_credential":
        return "Invalid credential";
      case "failed":
        return "Check failed";
      default:
        return "Unchecked";
    }
  },
) {
  switch (result) {
    case "healthy":
      return <Badge variant="success">{label(result)}</Badge>;
    case "low_balance":
      return <Badge variant="warning">{label(result)}</Badge>;
    case "invalid_credential":
      return <Badge variant="danger">{label(result)}</Badge>;
    case "failed":
      return <Badge variant="danger">{label(result)}</Badge>;
    default:
      return <Badge variant="neutral">{label(result)}</Badge>;
  }
}

/** A small status icon matching a balance check result. */
export function resultIcon(result: BalanceResult | null) {
  switch (result) {
    case "healthy":
      return <CheckCircle2 className="h-4 w-4 text-emerald-600" />;
    case "low_balance":
      return <AlertTriangle className="h-4 w-4 text-amber-600" />;
    case "invalid_credential":
    case "failed":
      return <XCircle className="h-4 w-4 text-rose-600" />;
    default:
      return <HelpCircle className="h-4 w-4 text-slate-400" />;
  }
}

/** Format a monetary value with an optional unit suffix; "—" when absent. */
export function formatMoney(value: number | null, unit: string | null) {
  if (value == null) return "—";
  const suffix = unit ? ` ${unit}` : "";
  return `${value.toFixed(2)}${suffix}`;
}

/** Convert provider-reported USD credits to their CNY purchase value. */
export function convertUsdCreditsToCny(
  value: number | null,
  unit: string | null,
  usdCreditsPerCny: number | null,
): number | null {
  if (
    value == null ||
    unit?.toUpperCase() !== "USD" ||
    usdCreditsPerCny == null ||
    !Number.isFinite(usdCreditsPerCny) ||
    usdCreditsPerCny <= 0
  ) {
    return null;
  }
  return value / usdCreditsPerCny;
}

export function formatConvertedBalance(
  value: number | null,
  unit: string | null,
  usdCreditsPerCny: number | null,
): string | null {
  const converted = convertUsdCreditsToCny(value, unit, usdCreditsPerCny);
  return converted == null ? null : `≈ ¥${converted.toFixed(2)}`;
}

/** Format an ISO timestamp as a locale string; "never" when null. */
export function formatTime(iso: string | null, locale?: Locale) {
  if (!iso) return locale === "zh-CN" ? "从未" : "never";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(locale);
}

/** Format a minute count as a short cadence label, e.g. "30 min" or "2 hr". */
export function formatInterval(minutes: number, locale?: Locale) {
  if (minutes % 60 === 0 && minutes >= 60) {
    const hours = minutes / 60;
    return locale === "zh-CN" ? `${hours} 小时` : `${hours} hr`;
  }
  return locale === "zh-CN" ? `${minutes} 分钟` : `${minutes} min`;
}
