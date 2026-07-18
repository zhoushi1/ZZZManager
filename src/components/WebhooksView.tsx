import { useCallback, useEffect, useState } from "react";
import { CheckCircle2, Pencil, Plus, Send, Trash2, X, XCircle } from "lucide-react";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card, CardContent, CardHeader } from "./ui/card";
import { Input, Label } from "./ui/input";
import * as api from "../lib/api";
import type { DeliveryView, HookView, NotificationHookType } from "../types";
import { useI18n } from "../lib/i18n";

function formatTime(iso: string, locale: string) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(locale);
}

export function WebhooksView() {
  const { t, locale, eventLabel } = useI18n();
  const [hooks, setHooks] = useState<HookView[]>([]);
  const [deliveries, setDeliveries] = useState<DeliveryView[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<HookView | null>(null);

  // Per-hook transient state for test / delete.
  const [testingId, setTestingId] = useState<string | null>(null);
  const [rowStatus, setRowStatus] = useState<
    Record<string, { ok: boolean; message: string }>
  >({});

  const refresh = useCallback(async () => {
    try {
      const [hs, ds] = await Promise.all([
        api.listHooks(),
        api.recentDeliveries(10),
      ]);
      setHooks(hs);
      setDeliveries(ds);
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

  const handleDelete = useCallback(
    async (hook: HookView) => {
      if (!confirm(t("webhooks.deleteConfirm", { name: hook.name }))) return;
      try {
        await api.deleteHook(hook.id);
        await refresh();
      } catch (err) {
        setRowStatus((prev) => ({
          ...prev,
          [hook.id]: { ok: false, message: String(err) },
        }));
      }
    },
    [refresh],
  );

  const handleTest = useCallback(
    async (hook: HookView) => {
      setTestingId(hook.id);
      setRowStatus((prev) => {
        const next = { ...prev };
        delete next[hook.id];
        return next;
      });
      try {
        const ok = await api.testHook(hook.id);
        setRowStatus((prev) => ({
          ...prev,
          [hook.id]: {
            ok,
            message: ok
              ? t("webhooks.testOk")
              : t("webhooks.testFailed"),
          },
        }));
      } catch (err) {
        setRowStatus((prev) => ({
          ...prev,
          [hook.id]: { ok: false, message: String(err) },
        }));
      } finally {
        setTestingId(null);
      }
    },
    [],
  );

  const hookName = (id: string | null) =>
    hooks.find((h) => h.id === id)?.name ?? (id ? t("webhooks.deletedHook") : "—");

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-5 overflow-auto p-7">
      <div className="flex items-center justify-end">
        <Button
          variant="primary"
          size="sm"
          onClick={() => {
            setEditing(null);
            setFormOpen(true);
          }}
        >
          <Plus className="h-4 w-4" />
          {t("webhooks.add")}
        </Button>
      </div>

      {loadError && (
        <p className="rounded-md border border-rose-200 bg-rose-50 px-3 py-2 text-sm text-rose-700">
          {loadError}
        </p>
      )}

      <Card>
        <CardHeader>
          <h2 className="text-sm font-semibold">{t("webhooks.title")}</h2>
          <p className="text-sm text-slate-500">
            {t("webhooks.hint")}
          </p>
        </CardHeader>
        <CardContent className="p-0">
          {loading ? (
            <div className="px-4 py-8 text-center text-sm text-slate-500">
              {t("webhooks.loading")}
            </div>
          ) : hooks.length === 0 ? (
            <div className="px-4 py-8 text-center text-sm text-slate-500">
              {t("webhooks.empty")}
            </div>
          ) : (
            hooks.map((hook) => (
              <div
                key={hook.id}
                className="border-b border-slate-100 px-4 py-4 last:border-b-0"
              >
                <div className="flex items-start justify-between gap-4">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="truncate text-sm font-medium">
                        {hook.name}
                      </span>
                      <Badge variant="neutral" className="text-xs">
                        {t(`webhooks.type.${hook.hookType}` as any)}
                      </Badge>
                      {hook.enabled ? (
                        <Badge variant="success">{t("enabled")}</Badge>
                      ) : (
                        <Badge variant="neutral">{t("disabled")}</Badge>
                      )}
                    </div>
                    <div className="mt-1 truncate text-xs text-slate-500">
                      {hook.url}
                    </div>
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => void handleTest(hook)}
                      disabled={testingId === hook.id}
                      title={t("webhooks.testTitle")}
                    >
                      <Send className="h-4 w-4" />
                      {testingId === hook.id ? t("webhooks.testing") : t("webhooks.test")}
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      onClick={() => {
                        setEditing(hook);
                        setFormOpen(true);
                      }}
                      title={t("edit")}
                    >
                      <Pencil className="h-4 w-4" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      onClick={() => void handleDelete(hook)}
                      title={t("delete")}
                    >
                      <Trash2 className="h-4 w-4 text-rose-600" />
                    </Button>
                  </div>
                </div>

                {rowStatus[hook.id] && (
                  <p
                    className={`mt-2 rounded-md border px-3 py-2 text-xs ${
                      rowStatus[hook.id].ok
                        ? "border-emerald-200 bg-emerald-50 text-emerald-700"
                        : "border-rose-200 bg-rose-50 text-rose-700"
                    }`}
                  >
                    {rowStatus[hook.id].message}
                  </p>
                )}
              </div>
            ))
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <h2 className="text-sm font-semibold">{t("webhooks.recentDeliveries")}</h2>
          <p className="text-sm text-slate-500">
            {t("webhooks.recentDeliveriesHint")}
          </p>
        </CardHeader>
        <CardContent className="space-y-4">
          {deliveries.length === 0 ? (
            <p className="text-sm text-slate-500">{t("webhooks.noDeliveries")}</p>
          ) : (
            deliveries.map((d) => (
              <div key={d.id} className="flex gap-3">
                <div className="mt-0.5">
                  {d.success ? (
                    <CheckCircle2 className="h-4 w-4 text-emerald-600" />
                  ) : (
                    <XCircle className="h-4 w-4 text-rose-600" />
                  )}
                </div>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center justify-between gap-3">
                    <span className="truncate text-sm font-medium">
                      {eventLabel(d.eventType)} → {hookName(d.hookId)}
                    </span>
                    <span className="text-xs text-slate-500">
                      {formatTime(d.createdAt, locale)}
                    </span>
                  </div>
                  <p className="mt-1 text-sm leading-5 text-slate-500">
                    {d.errorMessage ??
                      (d.responseStatus != null
                        ? `HTTP ${d.responseStatus}`
                        : t("overview.delivered"))}
                  </p>
                </div>
              </div>
            ))
          )}
        </CardContent>
      </Card>

      {formOpen && (
        <HookForm
          hook={editing}
          onClose={() => setFormOpen(false)}
          onSaved={() => {
            setFormOpen(false);
            void refresh();
          }}
        />
      )}
    </div>
  );
}

interface HookFormProps {
  hook: HookView | null;
  onClose: () => void;
  onSaved: () => void;
}

function HookForm({ hook, onClose, onSaved }: HookFormProps) {
  const { t } = useI18n();
  const isEdit = hook !== null;
  const [name, setName] = useState(hook?.name ?? "");
  const [url, setUrl] = useState(hook?.url ?? "");
  const [enabled, setEnabled] = useState(hook?.enabled ?? true);
  const [hookType, setHookType] = useState<NotificationHookType>(hook?.hookType ?? "generic");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      if (isEdit && hook) {
        await api.updateHook(hook.id, { name, url, enabled, hookType });
      } else {
        await api.createHook({ name, url, enabled, hookType });
      }
      onSaved();
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/40 p-4">
      <div className="w-full max-w-lg rounded-lg border border-slate-200 bg-white shadow-lg">
        <div className="flex items-center justify-between border-b border-slate-100 px-5 py-4">
          <h2 className="text-sm font-semibold">
            {isEdit ? t("webhooks.edit") : t("webhooks.add")}
          </h2>
          <Button variant="ghost" size="icon" onClick={onClose} title={t("close")}>
            <X className="h-4 w-4" />
          </Button>
        </div>

        <form onSubmit={handleSubmit} className="space-y-4 px-5 py-4">
          <div>
            <Label htmlFor="hook-name">{t("name")}</Label>
            <Input
              id="hook-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Ops channel"
              required
            />
          </div>

          <div>
            <Label htmlFor="hook-type">{t("webhooks.type")}</Label>
            <select
              id="hook-type"
              value={hookType}
              onChange={(e) => setHookType(e.target.value as NotificationHookType)}
              className="w-full rounded-md border border-slate-300 px-3 py-2 text-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500"
            >
              <option value="generic">{t("webhooks.type.generic")}</option>
              <option value="feishu">{t("webhooks.type.feishu")}</option>
              <option value="wecom">{t("webhooks.type.wecom")}</option>
              <option value="dingtalk">{t("webhooks.type.dingtalk")}</option>
            </select>
            <p className="mt-1 text-xs text-slate-500">
              {t("webhooks.typeHint")}
            </p>
          </div>

          <div>
            <Label htmlFor="hook-url">{t("webhooks.url")}</Label>
            <Input
              id="hook-url"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder={t(`webhooks.urlPlaceholder.${hookType}` as any)}
              required
            />
            <p className="mt-1 text-xs text-slate-500">
              {t(`webhooks.urlHint.${hookType}` as any)}
            </p>
          </div>

          <label className="flex items-center gap-2 text-sm text-slate-700">
            <input
              type="checkbox"
              checked={enabled}
              onChange={(e) => setEnabled(e.target.checked)}
              className="h-4 w-4 rounded border-slate-300"
            />
            {t("webhooks.enabledHint")}
          </label>

          {error && (
            <p className="rounded-md border border-rose-200 bg-rose-50 px-3 py-2 text-sm text-rose-700">
              {error}
            </p>
          )}

          <div className="flex justify-end gap-2 pt-2">
            <Button type="button" variant="secondary" onClick={onClose}>
              {t("cancel")}
            </Button>
            <Button type="submit" variant="primary" disabled={submitting}>
              {submitting ? t("saving") : isEdit ? t("saveChanges") : t("create")}
            </Button>
          </div>
        </form>
      </div>
    </div>
  );
}
