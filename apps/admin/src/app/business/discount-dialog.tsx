"use client";

// Per-provider discount editor for one B2B customer.
//
// The customer's price is one number — their default discount — plus, for a provider whose terms
// were negotiated separately, one override. There is no catalog to pick models from and no policy
// version to fence: a saved change is queued on the durable pricing lane and is live on the
// customer's next request.
import { useCallback, useEffect, useState } from "react";
import { api, send } from "@/lib/api";
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
  onSaved: () => void;
}) {
  const target = props.target;
  const [draft, setDraft] = useState<ProviderDraft>({});
  const [defaultPercent, setDefaultPercent] = useState("");
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!target) return;
    let active = true;
    setLoading(true);
    setError(null);
    setDefaultPercent(String(target.defaultPercent));
    void api<{ providers?: Record<string, number> }>(`/admin/business-users/${target.userId}/pricing`)
      .then((current) => {
        if (!active) return;
        const next: ProviderDraft = {};
        for (const provider of DISCOUNT_PROVIDERS) {
          const value = current.providers?.[provider.id];
          next[provider.id] = value === undefined ? "" : String(value);
        }
        setDraft(next);
        setLoading(false);
      })
      .catch((cause: unknown) => {
        if (!active) return;
        setError(cause instanceof Error ? cause.message : "не удалось прочитать скидки");
        setLoading(false);
      });
    return () => { active = false; };
  }, [target]);

  const save = useCallback(async () => {
    if (!target) return;
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
      await send(`/admin/business-users/${target.userId}/pricing`, "PATCH", {
        discountPercent: parsedDefault,
        providers,
        reason: props.reason,
      });
      props.onSaved();
      props.onClose();
    } catch (cause: unknown) {
      setError(cause instanceof Error ? cause.message : "не удалось сохранить");
    } finally {
      setSaving(false);
    }
  }, [defaultPercent, draft, props, target]);

  return (
    <Modal open={target !== null} title={target?.title ?? "Скидки"} onClose={props.onClose}>
      {loading ? <LoadingGrid count={2} /> : null}
      {error ? <div className="policy-rule-count bad">{error}</div> : null}
      {target && !loading ? (
        <div className="discount-editor">
          <label className="discount-row">
            <span>По умолчанию</span>
            <input
              inputMode="numeric"
              value={defaultPercent}
              disabled={saving}
              onChange={(event) => setDefaultPercent(event.target.value)}
            />
            <span className="discount-unit">%</span>
          </label>
          {DISCOUNT_PROVIDERS.map((provider) => (
            <label className="discount-row" key={provider.id}>
              <span>{provider.label}</span>
              <input
                inputMode="numeric"
                placeholder={`по умолчанию (${defaultPercent || target.defaultPercent}%)`}
                value={draft[provider.id] ?? ""}
                disabled={saving}
                onChange={(event) => setDraft((current) => ({ ...current, [provider.id]: event.target.value }))}
              />
              <span className="discount-unit">%</span>
            </label>
          ))}
          <p className="discount-hint">
            Пустое поле — у провайдера нет своей скидки, действует скидка по умолчанию.
          </p>
        </div>
      ) : null}
      <div className="dlg-actions">
        <button type="button" className="btn ghost" onClick={props.onClose}>Закрыть</button>
        <button type="button" className="btn" disabled={!target || loading || saving} onClick={() => void save()}>
          {saving ? "сохраняем…" : "сохранить скидки"}
        </button>
      </div>
    </Modal>
  );
}
