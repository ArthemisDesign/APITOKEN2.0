"use client";

// Per-provider discount editor for one B2B customer.
//
// The customer's price is one number — their default discount — plus, for a provider whose terms
// were negotiated separately, one override. There is no catalog to pick models from and no policy
// version to fence: a saved change is queued on the durable pricing lane and is live on the
// customer's next request.
import { useCallback, useState } from "react";
import { send } from "@/lib/api";
import { useResource } from "@/lib/resources";
import { LoadingGrid, Modal } from "@/components/ui";

export const DISCOUNT_PROVIDERS = [
  { id: "anthropic", label: "Anthropic (Claude)" },
  { id: "openai", label: "OpenAI (GPT)" },
  { id: "google", label: "Google (Gemini)" },
  { id: "kimi", label: "KIMI" },
  { id: "glm", label: "GLM (Zhipu)" },
] as const;

export type DiscountProviderId = (typeof DISCOUNT_PROVIDERS)[number]["id"];

export interface DiscountDialogTarget {
  userId: string;
  title: string;
  /** The customer's current default discount, shown as the fallback for every provider. */
  defaultPercent: number;
}

type ProviderDraft = Record<string, string>;

/** "" means "no override — use the default". Anything else must be an integer 0..100. */
function parsePercent(raw: string): number | null | undefined {
  const trimmed = raw.trim();
  if (trimmed === "") return null;
  if (!/^\d{1,3}$/.test(trimmed)) return undefined;
  const value = Number(trimmed);
  return value >= 0 && value <= 100 ? value : undefined;
}

export function DiscountDialog(props: {
  target: DiscountDialogTarget | null;
  reason: string;
  onClose: () => void;
}) {
  const target = props.target;
  return (
    <Modal
      open={target !== null}
      title={target?.title ?? "Скидки"}
      message="Базовая скидка действует для всех провайдеров. Отдельное значение заменяет её только для выбранного провайдера."
      wide
      onClose={props.onClose}
    >
      {target ? <DiscountDialogContent key={target.userId} {...props} target={target} /> : null}
    </Modal>
  );
}

function DiscountDialogContent(props: {
  target: DiscountDialogTarget;
  reason: string;
  onClose: () => void;
}) {
  const target = props.target;
  const pricingPath = `/admin/business-users/${target.userId}/pricing`;
  const { data: current, error: loadError, isLoading: loading } = useResource<{
    providers?: Record<string, number>;
  }>(pricingPath);
  if (loading) return <LoadingGrid count={2} />;
  if (loadError || !current) {
    return <div className="business-discount-error" role="alert">{loadError?.message ?? "не удалось прочитать скидки"}</div>;
  }
  return <DiscountEditor {...props} current={current} pricingPath={pricingPath} />;
}

function DiscountEditor(props: {
  target: DiscountDialogTarget;
  reason: string;
  onClose: () => void;
  current: { providers?: Record<string, number> };
  pricingPath: string;
}) {
  const target = props.target;
  const [draft, setDraft] = useState<ProviderDraft>(() => {
    const next: ProviderDraft = {};
    for (const provider of DISCOUNT_PROVIDERS) {
      const value = props.current.providers?.[provider.id];
      next[provider.id] = value === undefined ? "" : String(value);
    }
    return next;
  });
  const [defaultPercent, setDefaultPercent] = useState(() => String(target.defaultPercent));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = useCallback(async () => {
    const providers: Record<string, number | null> = {};
    for (const provider of DISCOUNT_PROVIDERS) {
      const parsed = parsePercent(draft[provider.id] ?? "");
      if (parsed === undefined) {
        setError(`${provider.label}: скидка должна быть целым числом 0–100 или пустой`);
        return;
      }
      providers[provider.id] = parsed;
    }
    const parsedDefault = parsePercent(defaultPercent);
    if (parsedDefault === undefined || parsedDefault === null) {
      setError("скидка по умолчанию должна быть целым числом 0–100");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await send(props.pricingPath, "PATCH", {
        discountPercent: parsedDefault,
        providers,
        reason: props.reason,
      });
      props.onClose();
    } catch (cause: unknown) {
      setError(cause instanceof Error ? cause.message : "не удалось сохранить");
    } finally {
      setSaving(false);
    }
  }, [defaultPercent, draft, props]);

  return (
    <>
      {error ? <div className="business-discount-error" role="alert">{error}</div> : null}
      <div className="business-discount-editor">
          <section className="business-default-discount" aria-labelledby="business-default-discount-label">
            <div>
              <span className="business-discount-kicker">Опорная ставка</span>
              <b id="business-default-discount-label">Базовая скидка</b>
              <p>Применяется ко всем провайдерам без собственного исключения.</p>
            </div>
            <label className="business-percent-control">
              <span className="sr-only">Базовая скидка, процентов</span>
              <input
                type="number"
                inputMode="numeric"
                min={0}
                max={100}
                step={1}
                value={defaultPercent}
                disabled={saving}
                aria-label="Базовая скидка, процентов"
                onChange={(event) => setDefaultPercent(event.target.value)}
              />
              <span>%</span>
            </label>
          </section>

          <section className="business-provider-section" aria-labelledby="business-provider-title">
            <div className="business-provider-title-row">
              <div>
                <b id="business-provider-title">Условия по провайдерам</b>
                <p>Пустое поле означает базовую скидку.</p>
              </div>
              <span>{DISCOUNT_PROVIDERS.length} провайдеров</span>
            </div>
            <div className="business-provider-rail">
              <div className="business-provider-head" aria-hidden="true">
                <span>Провайдер</span>
                <span>Условие</span>
                <span>Скидка</span>
              </div>
              {DISCOUNT_PROVIDERS.map((provider) => {
                const value = draft[provider.id] ?? "";
                const inherited = value.trim() === "";
                return (
                  <div className="business-provider-row" key={provider.id}>
                    <div className="business-provider-identity">
                      <span className={`business-provider-mark ${provider.id}`} aria-hidden="true">{provider.label.slice(0, 1)}</span>
                      <b>{provider.label}</b>
                    </div>
                    <span className={inherited ? "business-provider-state inherited" : "business-provider-state override"}>
                      {inherited ? `по умолчанию ${defaultPercent || target.defaultPercent}%` : "своя скидка"}
                    </span>
                    <label className="business-percent-control provider">
                      <span className="sr-only">Скидка {provider.label}, процентов; оставьте пустой для базовой</span>
                      <input
                        type="number"
                        inputMode="numeric"
                        min={0}
                        max={100}
                        step={1}
                        placeholder="—"
                        value={value}
                        disabled={saving}
                        aria-label={`Скидка ${provider.label}, процентов; пусто — базовая`}
                        onChange={(event) => setDraft((current) => ({ ...current, [provider.id]: event.target.value }))}
                      />
                      <span>%</span>
                    </label>
                  </div>
                );
              })}
            </div>
          </section>
      </div>
      <div className="dlg-actions">
        <button type="button" className="btn ghost" onClick={props.onClose}>Отмена</button>
        <button type="button" className="btn" disabled={saving} onClick={() => void save()}>
          {saving ? "Сохраняем…" : "Сохранить условия"}
        </button>
      </div>
    </>
  );
}
