import { useState, useEffect } from "react";
import { AlertTriangle, Plus, Trash2, Eye, EyeOff } from "lucide-react";
import { Button } from "./ui/button";
import { Input, Label } from "./ui/input";
import { useI18n } from "../lib/i18n";
import type {
  AccountView,
  AccountCredentialsView,
  CreateAccountInput,
  CustomHttpAdapterConfig,
  CustomHttpHeader,
  CustomHttpMethod,
  Provider,
  UpdateAccountInput,
} from "../types";

interface AccountFormProps {
  /** When set, the form edits this account; otherwise it creates a new one. */
  account: AccountView | null;
  onClose: () => void;
  onCreate: (input: CreateAccountInput) => Promise<void>;
  onUpdate: (id: string, input: UpdateAccountInput) => Promise<void>;
  onLoadCredentials: (id: string) => Promise<AccountCredentialsView>;
}

function toNumberOrNull(value: string): number | null {
  const trimmed = value.trim();
  if (trimmed === "") return null;
  const n = Number(trimmed);
  return Number.isFinite(n) ? n : null;
}

const SUB2API_TEMPLATE: CustomHttpAdapterConfig = {
  method: "GET",
  path: "/v1/usage",
  headers: [
    { name: "Authorization", value: "Bearer {{apiKey}}" },
    { name: "User-Agent", value: "{{userAgent}}" },
  ],
  // The structured adapter can't express the built-in `remaining ?? quota.remaining ?? balance`
  // fallback chain, so default to the primary `remaining` path and USD unit.
  remainingPath: "remaining",
  validPath: "",
  defaultUnit: "USD",
};

const NEW_API_TEMPLATE: CustomHttpAdapterConfig = {
  method: "GET",
  path: "/api/user/self",
  headers: [
    { name: "Authorization", value: "Bearer {{apiKey}}" },
    { name: "Content-Type", value: "application/json" },
    { name: "User-Agent", value: "{{userAgent}}" },
  ],
  remainingPath: "data.quota",
  usedPath: "data.used_quota",
  totalPath: "",
  validPath: "success",
  planNamePath: "data.group",
  messagePath: "message",
  numericDivisor: 500000,
  defaultUnit: "USD",
};

function cloneAdapter(config: CustomHttpAdapterConfig): CustomHttpAdapterConfig {
  return {
    ...config,
    headers: config.headers.map((h) => ({ ...h })),
  };
}

function defaultCustomAdapter(): CustomHttpAdapterConfig {
  return cloneAdapter(SUB2API_TEMPLATE);
}

function emptyToNull(value: string | null | undefined): string | null {
  const trimmed = value?.trim() ?? "";
  return trimmed === "" ? null : trimmed;
}

function normalizeAdapter(config: CustomHttpAdapterConfig): CustomHttpAdapterConfig {
  return {
    method: config.method,
    path: config.path.trim(),
    headers: config.headers
      .map((h) => ({ name: h.name.trim(), value: h.value }))
      .filter((h) => h.name !== ""),
    body: emptyToNull(config.body),
    validPath: emptyToNull(config.validPath),
    validEquals: config.validEquals,
    remainingPath: config.remainingPath.trim(),
    usedPath: emptyToNull(config.usedPath),
    totalPath: emptyToNull(config.totalPath),
    unitPath: emptyToNull(config.unitPath),
    planNamePath: emptyToNull(config.planNamePath),
    messagePath: emptyToNull(config.messagePath),
    numericDivisor:
      config.numericDivisor != null && Number.isFinite(config.numericDivisor)
        ? config.numericDivisor
        : null,
    defaultUnit: emptyToNull(config.defaultUnit),
  };
}

export function AccountForm({
  account,
  onClose,
  onCreate,
  onUpdate,
  onLoadCredentials,
}: AccountFormProps) {
  const { t, providerLabel } = useI18n();
  const isEdit = account !== null;

  const [name, setName] = useState(account?.name ?? "");
  const [provider, setProvider] = useState<Provider>(
    account?.provider ?? "new_api",
  );
  const [baseUrl, setBaseUrl] = useState(account?.baseUrl ?? "");
  const [enabled, setEnabled] = useState(account?.enabled ?? true);
  const [threshold, setThreshold] = useState(
    account?.balanceThreshold != null ? String(account.balanceThreshold) : "",
  );
  const [usdCreditsPerCny, setUsdCreditsPerCny] = useState(
    account?.usdCreditsPerCny != null ? String(account.usdCreditsPerCny) : "",
  );
  const parsedUsdCreditsPerCny = toNumberOrNull(usdCreditsPerCny);
  const usesConvertedThreshold =
    parsedUsdCreditsPerCny != null && parsedUsdCreditsPerCny > 0;
  const [interval, setInterval] = useState(
    account?.checkIntervalMinutes != null
      ? String(account.checkIntervalMinutes)
      : "",
  );
  const [officialUrl, setOfficialUrl] = useState(account?.officialUrl ?? "");
  const [note, setNote] = useState(account?.note ?? "");
  const [sortOrder, setSortOrder] = useState(
    account?.sortOrder != null ? String(account.sortOrder) : "",
  );
  const [accessToken, setAccessToken] = useState("");
  const [userId, setUserId] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [customAdapter, setCustomAdapter] = useState<CustomHttpAdapterConfig>(
    () => cloneAdapter(account?.customAdapter ?? defaultCustomAdapter()),
  );

  const [credentialsLoading, setCredentialsLoading] = useState(false);
  const [showAccessToken, setShowAccessToken] = useState(false);
  const [showApiKey, setShowApiKey] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (isEdit && account) {
      setCredentialsLoading(true);
      onLoadCredentials(account.id)
        .then((creds) => {
          if (creds.provider === "new_api") {
            if (creds.accessToken) setAccessToken(creds.accessToken);
            if (creds.userId) setUserId(creds.userId);
          } else if (creds.provider === "sub2api" || creds.provider === "custom_http") {
            if (creds.apiKey) setApiKey(creds.apiKey);
          }
        })
        .catch((err) => {
          setError(t("form.loadCredentialsFailed", { error: String(err) }));
        })
        .finally(() => {
          setCredentialsLoading(false);
        });
    }
  }, [isEdit, account?.id, onLoadCredentials, t]);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    try {
      const credentials =
        provider === "new_api"
          ? { accessToken: accessToken || undefined, userId: userId || undefined }
          : { apiKey: apiKey || undefined };
      const adapter =
        provider === "custom_http" ? normalizeAdapter(customAdapter) : null;

      if (isEdit && account) {
        await onUpdate(account.id, {
          name,
          baseUrl,
          enabled,
          balanceThreshold: toNumberOrNull(threshold),
          usdCreditsPerCny: parsedUsdCreditsPerCny,
          checkIntervalMinutes: toNumberOrNull(interval),
          officialUrl: emptyToNull(officialUrl),
          note: emptyToNull(note),
          sortOrder: toNumberOrNull(sortOrder),
          credentials,
          customAdapter: adapter,
        });
      } else {
        await onCreate({
          name,
          provider,
          baseUrl,
          enabled,
          balanceThreshold: toNumberOrNull(threshold),
          usdCreditsPerCny: parsedUsdCreditsPerCny,
          checkIntervalMinutes: toNumberOrNull(interval),
          officialUrl: emptyToNull(officialUrl),
          note: emptyToNull(note),
          sortOrder: toNumberOrNull(sortOrder),
          credentials,
          customAdapter: adapter,
        });
      }
      onClose();
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <form id="account-form" onSubmit={handleSubmit} className="space-y-8">
      <section className="space-y-4">
        <h3 className="text-sm font-semibold text-slate-900">
          {t("form.sectionBasics")}
        </h3>
        <div>
          <Label htmlFor="account-name">{t("name")}</Label>
          <Input
            id="account-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="New API Main"
            required
          />
        </div>

        <div className="grid grid-cols-2 gap-3">
          <div>
            <Label htmlFor="account-provider">{t("provider")}</Label>
            <select
              id="account-provider"
              value={provider}
              disabled={isEdit}
              onChange={(e) => setProvider(e.target.value as Provider)}
              className="h-9 w-full rounded-md border border-slate-200 bg-white px-3 text-sm text-slate-900 shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-slate-900 disabled:opacity-50"
            >
              {(["new_api", "sub2api", "custom_http"] as Provider[]).map((p) => (
                <option key={p} value={p}>
                  {providerLabel(p)}
                </option>
              ))}
            </select>
          </div>
          <div>
            <Label htmlFor="account-baseurl">{t("baseUrl")}</Label>
            <Input
              id="account-baseurl"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder="https://api.example.com"
              required
            />
          </div>
        </div>

        <div>
          <Label htmlFor="account-note">{t("form.note")}</Label>
          <textarea
            id="account-note"
            value={note}
            onChange={(e) => setNote(e.target.value)}
            placeholder={t("form.notePlaceholder")}
            rows={3}
            className="w-full rounded-md border border-slate-200 bg-white px-3 py-2 text-sm text-slate-900 shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-slate-900"
          />
        </div>
      </section>

      <section className="space-y-4">
        <h3 className="text-sm font-semibold text-slate-900">
          {t("form.sectionCredentials")}
        </h3>
        {credentialsLoading && (
          <p className="text-xs text-slate-500">{t("form.loadingCredentials")}</p>
        )}
        <CredentialFields
          provider={provider}
          isEdit={isEdit}
          hasCredentials={account?.hasCredentials ?? false}
          accessToken={accessToken}
          setAccessToken={setAccessToken}
          userId={userId}
          setUserId={setUserId}
          apiKey={apiKey}
          setApiKey={setApiKey}
          showAccessToken={showAccessToken}
          setShowAccessToken={setShowAccessToken}
          showApiKey={showApiKey}
          setShowApiKey={setShowApiKey}
          customAdapter={customAdapter}
          setCustomAdapter={setCustomAdapter}
        />
      </section>

      <section className="space-y-4">
        <h3 className="text-sm font-semibold text-slate-900">
          {t("form.sectionCheck")}
        </h3>
        <div className="grid grid-cols-2 gap-3">
          <div>
            <Label htmlFor="account-threshold">
              {t(
                usesConvertedThreshold
                  ? "form.convertedBalanceThreshold"
                  : "form.balanceThreshold",
              )}
            </Label>
            <Input
              id="account-threshold"
              type="number"
              step="any"
              value={threshold}
              onChange={(e) => setThreshold(e.target.value)}
              placeholder="20"
            />
          </div>
          <div>
            <Label htmlFor="account-usd-credits-per-cny">
              {t("form.usdCreditsPerCny")}
            </Label>
            <Input
              id="account-usd-credits-per-cny"
              type="number"
              min="0.000001"
              step="any"
              value={usdCreditsPerCny}
              onChange={(e) => setUsdCreditsPerCny(e.target.value)}
              placeholder="12"
            />
          </div>
          <div>
            <Label htmlFor="account-interval">{t("form.checkInterval")}</Label>
            <Input
              id="account-interval"
              type="number"
              value={interval}
              onChange={(e) => setInterval(e.target.value)}
              placeholder="30"
            />
          </div>
        </div>

        {usesConvertedThreshold && (
          <div className="flex gap-2 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs leading-5 text-amber-900">
            <AlertTriangle className="mt-0.5 h-4 w-4 flex-shrink-0" />
            <span>
              {t("form.convertedThresholdHint", {
                rate: parsedUsdCreditsPerCny,
              })}
            </span>
          </div>
        )}

        <div>
          <Label htmlFor="account-sortorder">{t("form.sortOrder")}</Label>
          <Input
            id="account-sortorder"
            type="number"
            value={sortOrder}
            onChange={(e) => setSortOrder(e.target.value)}
            placeholder="0"
          />
          <p className="mt-1 text-xs text-slate-500">{t("form.sortOrderHint")}</p>
        </div>

        <label className="flex items-center gap-2 text-sm text-slate-700">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => setEnabled(e.target.checked)}
            className="h-4 w-4 rounded border-slate-300"
          />
          {t("form.enabledHint")}
        </label>
      </section>

      <section className="space-y-4">
        <h3 className="text-sm font-semibold text-slate-900">
          {t("form.sectionAdvanced")}
        </h3>
        <div>
          <Label htmlFor="account-official-url">{t("form.officialUrl")}</Label>
          <Input
            id="account-official-url"
            type="url"
            value={officialUrl}
            onChange={(e) => setOfficialUrl(e.target.value)}
            placeholder={t("form.officialUrlPlaceholder")}
          />
        </div>
      </section>

      {error && (
        <p className="rounded-md border border-rose-200 bg-rose-50 px-3 py-2 text-sm text-rose-700">
          {error}
        </p>
      )}
    </form>
  );
}

interface CredentialFieldsProps {
  provider: Provider;
  isEdit: boolean;
  hasCredentials: boolean;
  accessToken: string;
  setAccessToken: (v: string) => void;
  userId: string;
  setUserId: (v: string) => void;
  apiKey: string;
  setApiKey: (v: string) => void;
  showAccessToken: boolean;
  setShowAccessToken: (v: boolean) => void;
  showApiKey: boolean;
  setShowApiKey: (v: boolean) => void;
  customAdapter: CustomHttpAdapterConfig;
  setCustomAdapter: (v: CustomHttpAdapterConfig) => void;
}

function CredentialFields({
  provider,
  isEdit,
  hasCredentials,
  accessToken,
  setAccessToken,
  userId,
  setUserId,
  apiKey,
  setApiKey,
  showAccessToken,
  setShowAccessToken,
  showApiKey,
  setShowApiKey,
  customAdapter,
  setCustomAdapter,
}: CredentialFieldsProps) {
  const { t } = useI18n();
  // On edit, leaving a field blank preserves the stored credential — but only
  // when there is one. A credentialless placeholder (imported account) must be
  // given credentials, so we require them and prompt for them here.
  const isPlaceholder = isEdit && !hasCredentials;
  const credsRequired = !isEdit || isPlaceholder;
  const keepHint =
    isEdit && hasCredentials ? t("form.keepStored") : undefined;

  if (provider === "new_api") {
    return (
      <div className="space-y-3 rounded-md border border-slate-100 bg-slate-50 p-3">
        {isPlaceholder && (
          <p className="text-xs text-amber-800">
            {t("form.placeholderCreds")}
          </p>
        )}
        <div>
          <Label htmlFor="cred-access-token">{t("accessToken")}</Label>
          <div className="relative">
            <Input
              id="cred-access-token"
              type={showAccessToken ? "text" : "password"}
              value={accessToken}
              onChange={(e) => setAccessToken(e.target.value)}
              placeholder={keepHint ?? t("accessToken")}
              required={credsRequired}
              className="pr-10"
            />
            <button
              type="button"
              onClick={() => setShowAccessToken(!showAccessToken)}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-slate-500 hover:text-slate-700"
              aria-label={showAccessToken ? t("form.hideSecret") : t("form.showSecret")}
              title={showAccessToken ? t("form.hideSecret") : t("form.showSecret")}
            >
              {showAccessToken ? (
                <EyeOff className="h-4 w-4" />
              ) : (
                <Eye className="h-4 w-4" />
              )}
            </button>
          </div>
        </div>
        <div>
          <Label htmlFor="cred-user-id">{t("userId")}</Label>
          <Input
            id="cred-user-id"
            value={userId}
            onChange={(e) => setUserId(e.target.value)}
            placeholder={keepHint ?? t("userId")}
            required={credsRequired}
          />
        </div>
      </div>
    );
  }

  if (provider === "custom_http") {
    return (
      <div className="space-y-3 rounded-md border border-slate-100 bg-slate-50 p-3">
        {isPlaceholder && (
          <p className="text-xs text-amber-800">
            {t("form.placeholderCreds")}
          </p>
        )}
        <div>
          <Label htmlFor="cred-custom-api-key">{t("apiKey")}</Label>
          <div className="relative">
            <Input
              id="cred-custom-api-key"
              type={showApiKey ? "text" : "password"}
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder={keepHint ?? t("apiKey")}
              required={credsRequired}
              className="pr-10"
            />
            <button
              type="button"
              onClick={() => setShowApiKey(!showApiKey)}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-slate-500 hover:text-slate-700"
              aria-label={showApiKey ? t("form.hideSecret") : t("form.showSecret")}
              title={showApiKey ? t("form.hideSecret") : t("form.showSecret")}
            >
              {showApiKey ? (
                <EyeOff className="h-4 w-4" />
              ) : (
                <Eye className="h-4 w-4" />
              )}
            </button>
          </div>
        </div>
        <CustomAdapterEditor
          value={customAdapter}
          onChange={setCustomAdapter}
        />
      </div>
    );
  }

  return (
    <div className="space-y-3 rounded-md border border-slate-100 bg-slate-50 p-3">
      {isPlaceholder && (
        <p className="text-xs text-amber-800">
          {t("form.placeholderCreds")}
        </p>
      )}
      <div>
        <Label htmlFor="cred-api-key">{t("apiKey")}</Label>
        <div className="relative">
          <Input
            id="cred-api-key"
            type={showApiKey ? "text" : "password"}
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder={keepHint ?? t("apiKey")}
            required={credsRequired}
            className="pr-10"
          />
          <button
            type="button"
            onClick={() => setShowApiKey(!showApiKey)}
            className="absolute right-2 top-1/2 -translate-y-1/2 text-slate-500 hover:text-slate-700"
            aria-label={showApiKey ? t("form.hideSecret") : t("form.showSecret")}
            title={showApiKey ? t("form.hideSecret") : t("form.showSecret")}
          >
            {showApiKey ? (
              <EyeOff className="h-4 w-4" />
            ) : (
              <Eye className="h-4 w-4" />
            )}
          </button>
        </div>
      </div>
    </div>
  );
}

function CustomAdapterEditor({
  value,
  onChange,
}: {
  value: CustomHttpAdapterConfig;
  onChange: (v: CustomHttpAdapterConfig) => void;
}) {
  const { t } = useI18n();
  const set = (patch: Partial<CustomHttpAdapterConfig>) =>
    onChange({ ...value, ...patch });

  const setHeader = (index: number, patch: Partial<CustomHttpHeader>) => {
    const headers = value.headers.map((header, i) =>
      i === index ? { ...header, ...patch } : header,
    );
    onChange({ ...value, headers });
  };

  const addHeader = () =>
    onChange({
      ...value,
      headers: [...value.headers, { name: "", value: "" }],
    });

  const removeHeader = (index: number) =>
    onChange({
      ...value,
      headers: value.headers.filter((_, i) => i !== index),
    });

  return (
    <div className="space-y-3 border-t border-slate-200 pt-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="text-xs font-medium text-slate-600">{t("adapter.config")}</div>
        <div className="flex gap-2">
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => onChange(cloneAdapter(SUB2API_TEMPLATE))}
          >
            {t("adapter.sub2Template")}
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => onChange(cloneAdapter(NEW_API_TEMPLATE))}
          >
            {t("adapter.newApiTemplate")}
          </Button>
        </div>
      </div>

      <div className="grid grid-cols-[110px_1fr] gap-3">
        <div>
          <Label htmlFor="custom-method">{t("adapter.method")}</Label>
          <select
            id="custom-method"
            value={value.method}
            onChange={(e) => set({ method: e.target.value as CustomHttpMethod })}
            className="h-9 w-full rounded-md border border-slate-200 bg-white px-3 text-sm text-slate-900 shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-slate-900"
          >
            <option value="GET">GET</option>
            <option value="POST">POST</option>
          </select>
        </div>
        <div>
          <Label htmlFor="custom-path">{t("adapter.path")}</Label>
          <Input
            id="custom-path"
            value={value.path}
            onChange={(e) => set({ path: e.target.value })}
            placeholder="/v1/usage"
            required
          />
        </div>
      </div>

      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <Label className="mb-0">{t("adapter.headers")}</Label>
          <Button type="button" variant="ghost" size="sm" onClick={addHeader}>
            <Plus className="h-4 w-4" />
            {t("adapter.add")}
          </Button>
        </div>
        {value.headers.map((header, index) => (
          <div key={index} className="grid grid-cols-[1fr_1.5fr_36px] gap-2">
            <Input
              value={header.name}
              onChange={(e) => setHeader(index, { name: e.target.value })}
              placeholder="Authorization"
            />
            <Input
              value={header.value}
              onChange={(e) => setHeader(index, { value: e.target.value })}
              placeholder="Bearer {{apiKey}}"
            />
            <Button
              type="button"
              variant="ghost"
              size="icon"
              onClick={() => removeHeader(index)}
              title={t("adapter.removeHeader")}
            >
              <Trash2 className="h-4 w-4 text-rose-600" />
            </Button>
          </div>
        ))}
      </div>

      {value.method === "POST" && (
        <div>
          <Label htmlFor="custom-body">{t("adapter.bodyTemplate")}</Label>
          <textarea
            id="custom-body"
            value={value.body ?? ""}
            onChange={(e) => set({ body: e.target.value })}
            className="min-h-20 w-full rounded-md border border-slate-200 bg-white px-3 py-2 text-sm text-slate-900 shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-slate-900"
            placeholder='{"key":"{{apiKey}}"}'
          />
        </div>
      )}

      <div className="grid grid-cols-2 gap-3">
        <Field
          id="remaining-path"
          label={t("adapter.remainingPath")}
          value={value.remainingPath}
          onChange={(v) => set({ remainingPath: v })}
          placeholder="balance"
          required
        />
        <Field
          id="valid-path"
          label={t("adapter.validPath")}
          value={value.validPath ?? ""}
          onChange={(v) => set({ validPath: v })}
          placeholder="success"
        />
        <Field
          id="used-path"
          label={t("adapter.usedPath")}
          value={value.usedPath ?? ""}
          onChange={(v) => set({ usedPath: v })}
          placeholder="data.used"
        />
        <Field
          id="total-path"
          label={t("adapter.totalPath")}
          value={value.totalPath ?? ""}
          onChange={(v) => set({ totalPath: v })}
          placeholder="data.total"
        />
        <Field
          id="unit-path"
          label={t("adapter.unitPath")}
          value={value.unitPath ?? ""}
          onChange={(v) => set({ unitPath: v })}
          placeholder="data.unit"
        />
        <Field
          id="default-unit"
          label={t("adapter.defaultUnit")}
          value={value.defaultUnit ?? ""}
          onChange={(v) => set({ defaultUnit: v })}
          placeholder="USD"
        />
        <Field
          id="plan-path"
          label={t("adapter.planPath")}
          value={value.planNamePath ?? ""}
          onChange={(v) => set({ planNamePath: v })}
          placeholder="data.group"
        />
        <Field
          id="message-path"
          label={t("adapter.messagePath")}
          value={value.messagePath ?? ""}
          onChange={(v) => set({ messagePath: v })}
          placeholder="message"
        />
        <div>
          <Label htmlFor="numeric-divisor">{t("adapter.numericDivisor")}</Label>
          <Input
            id="numeric-divisor"
            type="number"
            step="any"
            value={value.numericDivisor ?? ""}
            onChange={(e) =>
              set({
                numericDivisor:
                  e.target.value.trim() === "" ? null : Number(e.target.value),
              })
            }
            placeholder="1"
          />
        </div>
      </div>
    </div>
  );
}

function Field({
  id,
  label,
  value,
  onChange,
  placeholder,
  required,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  required?: boolean;
}) {
  return (
    <div>
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        required={required}
      />
    </div>
  );
}
