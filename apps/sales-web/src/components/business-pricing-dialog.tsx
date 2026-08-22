"use client";

import { useEffect, useState } from "react";
import { api, ApiError, type ReferralRow } from "@/lib/api";
import { Button, Input, Notice } from "@/components/ui";
import { useI18n } from "@/components/i18n";
import { providerLabel } from "@/components/provider-breakdown";

// Провайдеры, для которых можно задать персональную ставку. Список закрыт и совпадает с
// DISCOUNT_PROVIDER_IDS на стороне commerce: неизвестный id там был бы отвергнут, а молча
// сохранённая опечатка никогда бы не сматчилась с запросом.
const PROVIDERS = ["anthropic", "openai", "google", "kimi", "glm"] as const;

type Draft = { default: string; providers: Record<string, string> };

function emptyDraft(row: ReferralRow): Draft {
  return {
    default: row.customerType === "b2b" && row.discountPercent != null ? String(row.discountPercent) : "",
    providers: Object.fromEntries(PROVIDERS.map((id) => [id, ""])),
  };
}

/** Целое число процентов в пределах потолка, либо null если поле пустое, либо ошибка. */
function parsePercent(raw: string, ceiling: number): number | null | "invalid" {
  const value = raw.trim();
  if (value === "") return null;
  if (!/^\d{1,2}$/.test(value)) return "invalid";
  const percent = Number(value);
  return percent > ceiling ? "invalid" : percent;
}

export function BusinessPricingDialog({
  row,
  ceilingPercent,
  onClose,
  onSaved,
}: {
  row: ReferralRow;
  ceilingPercent: number;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<Draft>(() => emptyDraft(row));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isB2b = row.customerType === "b2b";

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  async function save() {
    setError(null);
    const parsedDefault = parsePercent(draft.default, ceilingPercent);
    if (parsedDefault === "invalid") {
      return setError(t(
        `The base discount must be a whole percent no greater than ${ceilingPercent}.`,
        `Базовая скидка — целый процент не больше ${ceilingPercent}.`,
      ));
    }
    // Converting needs a base rate: provider overrides alone would leave every other model
    // on the ordinary B2C price.
    if (!isB2b && parsedDefault === null) {
      return setError(t(
        "Set the base discount to convert this customer to B2B.",
        "Задайте базовую скидку, чтобы перевести клиента в B2B.",
      ));
    }
    const providers: Record<string, number> = {};
    for (const id of PROVIDERS) {
      const parsed = parsePercent(draft.providers[id] ?? "", ceilingPercent);
      if (parsed === "invalid") {
        return setError(t(
          `${providerLabel(id, id)}: a whole percent no greater than ${ceilingPercent}.`,
          `${providerLabel(id, id)}: целый процент не больше ${ceilingPercent}.`,
        ));
      }
      if (parsed !== null) providers[id] = parsed;
    }

    setBusy(true);
    try {
      await api(`/v1/partner/referrals/${row.userRef}/business-pricing`, {
        method: "POST",
        body: {
          ...(parsedDefault === null ? {} : { discountPercent: parsedDefault }),
          ...(Object.keys(providers).length > 0 ? { providers } : {}),
        },
      });
      onSaved();
      onClose();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Could not save.", "Не удалось сохранить."));
      setBusy(false);
    }
  }

  return (
    <div className="auth-shell" style={{ position: "fixed", inset: 0, zIndex: 50, justifyContent: "center", padding: 16, overflowY: "auto" }}>
      <button
        type="button"
        aria-label={t("Close business pricing", "Закрыть бизнес-условия")}
        onClick={onClose}
        style={{ position: "absolute", inset: 0, border: 0, background: "rgba(0,0,0,.45)", cursor: "default" }}
      />
      <section className="auth-card" role="dialog" aria-modal="true" aria-labelledby="business-pricing-title" style={{ maxWidth: 460, position: "relative", overscrollBehavior: "contain" }}>
        <h1 id="business-pricing-title" style={{ fontSize: 18 }}>
          {isB2b
            ? t("Business pricing", "Бизнес-условия")
            : t("Convert to a business customer", "Перевести в бизнес-клиенты")}
        </h1>
        <p className="auth-sub">
          {t(
            `Your maximum is ${ceilingPercent}%. Leave a provider blank to keep it on the base discount.`,
            `Ваш максимум — ${ceilingPercent}%. Пустое поле провайдера — он остаётся на базовой скидке.`,
          )}
        </p>
        {error ? <Notice kind="error">{error}</Notice> : null}

        <label className="field-hint" htmlFor="b2b-default">
          {t("Base discount, %", "Базовая скидка, %")}
        </label>
        <Input
          id="b2b-default"
          inputMode="numeric"
          autoComplete="off"
          value={draft.default}
          placeholder={String(ceilingPercent)}
          onChange={(e) => setDraft({ ...draft, default: e.target.value.replace(/[^\d]/g, "") })}
          style={{ marginBottom: 12 }}
        />

        <p className="field-hint" style={{ marginBottom: 6 }}>
          {t("Per-provider overrides (optional)", "Ставки по провайдерам (необязательно)")}
        </p>
        <div style={{ display: "grid", gap: 8, marginBottom: 14 }}>
          {PROVIDERS.map((id) => (
            <div key={id} style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <span style={{ flex: 1, fontSize: 13 }}>{providerLabel(id, id)}</span>
              <Input
                inputMode="numeric"
                aria-label={providerLabel(id, id)}
                name={`provider-discount-${id}`}
                autoComplete="off"
                value={draft.providers[id] ?? ""}
                placeholder="—"
                onChange={(e) => setDraft({
                  ...draft,
                  providers: { ...draft.providers, [id]: e.target.value.replace(/[^\d]/g, "") },
                })}
                style={{ width: 90 }}
              />
              <span className="field-hint">%</span>
            </div>
          ))}
        </div>

        <div style={{ display: "flex", gap: 8 }}>
          <Button onClick={save} loading={busy} style={{ flex: 1 }}>
            {isB2b ? t("Save", "Сохранить") : t("Convert", "Перевести")}
          </Button>
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            {t("Cancel", "Отмена")}
          </Button>
        </div>
      </section>
    </div>
  );
}
