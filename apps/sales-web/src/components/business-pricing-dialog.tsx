"use client";

import { useEffect, useRef, useState } from "react";
import { api, ApiError, type ReferralRow } from "@/lib/api";
import { Button, Input, Notice, Textarea } from "@/components/ui";
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
  mode,
  onClose,
  onSaved,
}: {
  row: ReferralRow;
  ceilingPercent: number;
  mode: "direct" | "request";
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<Draft>(() => emptyDraft(row));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reason, setReason] = useState("");
  const dialogRef = useRef<HTMLElement>(null);
  const busyRef = useRef(false);
  const onCloseRef = useRef(onClose);

  const isB2b = row.customerType === "b2b";
  const effectiveCeiling = mode === "request" ? 95 : ceilingPercent;

  useEffect(() => {
    busyRef.current = busy;
  }, [busy]);

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    const previousFocus = document.activeElement;
    const previousOverflow = document.body.style.overflow;
    const dialog = dialogRef.current;
    document.body.style.overflow = "hidden";
    window.requestAnimationFrame(() => dialog?.querySelector<HTMLElement>("input, textarea, button")?.focus());
    const handleKeyboard = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (!busyRef.current) onCloseRef.current();
        return;
      }
      if (event.key !== "Tab" || !dialog) return;
      const focusable = [...dialog.querySelectorAll<HTMLElement>(
        "button:not([disabled]), input:not([disabled]), textarea:not([disabled]), select:not([disabled]), a[href]",
      )].filter((element) => element.tabIndex !== -1);
      if (!focusable.length) return;
      const first = focusable[0]!;
      const last = focusable[focusable.length - 1]!;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleKeyboard);
    return () => {
      document.removeEventListener("keydown", handleKeyboard);
      document.body.style.overflow = previousOverflow;
      if (previousFocus instanceof HTMLElement) previousFocus.focus();
    };
  }, []);

  function showError(message: string, fieldId: string) {
    setError(message);
    window.requestAnimationFrame(() => document.getElementById(fieldId)?.focus());
  }

  async function save() {
    setError(null);
    const parsedDefault = parsePercent(draft.default, effectiveCeiling);
    if (parsedDefault === "invalid") {
      return showError(t(
        `The base discount must be a whole percent no greater than ${effectiveCeiling}.`,
        `Базовая скидка — целый процент не больше ${effectiveCeiling}.`,
      ), "b2b-default");
    }
    // Converting needs a base rate: provider overrides alone would leave every other model
    // on the ordinary B2C price.
    if ((mode === "request" || !isB2b) && parsedDefault === null) {
      return showError(t(
        "Set the base discount to convert this customer to B2B.",
        "Задайте базовую скидку, чтобы перевести клиента в B2B.",
      ), "b2b-default");
    }
    const providers: Record<string, number> = {};
    for (const id of PROVIDERS) {
      const parsed = parsePercent(draft.providers[id] ?? "", effectiveCeiling);
      if (parsed === "invalid") {
        return showError(t(
          `${providerLabel(id, id)}: a whole percent no greater than ${effectiveCeiling}.`,
          `${providerLabel(id, id)}: целый процент не больше ${effectiveCeiling}.`,
        ), `b2b-provider-${id}`);
      }
      if (parsed !== null) providers[id] = parsed;
    }

    const cleanReason = reason.trim();
    if (mode === "request" && !cleanReason) {
      return showError(t(
        "Explain why this customer needs B2B status or these terms.",
        "Объясните, почему клиенту нужен B2B-статус или такие условия.",
      ), "b2b-request-reason");
    }

    setBusy(true);
    try {
      await api(mode === "direct"
        ? `/v1/partner/referrals/${row.userRef}/business-pricing`
        : `/v1/partner/referrals/${row.userRef}/b2b-requests`, {
        method: "POST",
        body: {
          ...(parsedDefault === null ? {} : { discountPercent: parsedDefault }),
          ...(Object.keys(providers).length > 0 ? { providers } : {}),
          ...(mode === "request" ? { reason: cleanReason } : {}),
        },
        ...(mode === "request"
          ? { headers: { "Idempotency-Key": crypto.randomUUID() } }
          : {}),
      });
      onSaved();
      onClose();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Could not save.", "Не удалось сохранить."));
      setBusy(false);
    }
  }

  return (
    <div className="auth-shell business-dialog-shell">
      <button
        type="button"
        className="business-dialog-backdrop"
        aria-label={t("Close business pricing", "Закрыть бизнес-условия")}
        onClick={() => { if (!busy) onClose(); }}
        disabled={busy}
      />
      <section ref={dialogRef} className="auth-card business-dialog" role="dialog" aria-modal="true" aria-labelledby="business-pricing-title" aria-describedby="business-pricing-description">
        <h1 id="business-pricing-title" className="business-dialog-title">
          {mode === "request"
            ? (isB2b ? t("Request new business pricing", "Запросить новые B2B-условия") : t("Request B2B conversion", "Запросить перевод в B2B"))
            : isB2b
            ? t("Business pricing", "Бизнес-условия")
            : t("Convert to a business customer", "Перевести в бизнес-клиенты")}
        </h1>
        <p className="auth-sub" id="business-pricing-description">
          {t(
            mode === "request"
              ? "Choose the terms to send for review. An administrator may approve different values. Leave a provider blank to use the base discount."
              : `Your maximum is ${ceilingPercent}%. Leave a provider blank to keep it on the base discount.`,
            mode === "request"
              ? "Укажите условия для рассмотрения. Администратор может одобрить другие значения. Пустое поле провайдера означает базовую скидку."
              : `Ваш максимум — ${ceilingPercent}%. Пустое поле провайдера — он остаётся на базовой скидке.`,
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
          placeholder={t("For example, 15…", "Например, 15…")}
          onChange={(e) => setDraft({ ...draft, default: e.target.value.replace(/[^\d]/g, "") })}
          className="business-dialog-base"
        />

        <p className="field-hint business-dialog-provider-label">
          {t("Per-provider overrides (optional)", "Ставки по провайдерам (необязательно)")}
        </p>
        <div className="business-dialog-providers">
          {PROVIDERS.map((id) => (
            <div key={id} className="business-dialog-provider-row">
              <label htmlFor={`b2b-provider-${id}`} translate="no">{providerLabel(id, id)}</label>
              <Input
                id={`b2b-provider-${id}`}
                inputMode="numeric"
                name={`provider-discount-${id}`}
                autoComplete="off"
                value={draft.providers[id] ?? ""}
                placeholder={t("Base…", "База…")}
                onChange={(e) => setDraft({
                  ...draft,
                  providers: { ...draft.providers, [id]: e.target.value.replace(/[^\d]/g, "") },
                })}
                className="business-dialog-provider-input"
              />
              <span className="field-hint">%</span>
            </div>
          ))}
        </div>

        {mode === "request" ? (
          <label className="field" htmlFor="b2b-request-reason">
            <span>{t("Business reason", "Обоснование")}</span>
            <Textarea
              id="b2b-request-reason"
              autoComplete="off"
              value={reason}
              maxLength={4000}
              placeholder={t("Expected volume, customer needs and why these terms are required…", "Ожидаемый объём, потребности клиента и зачем нужны эти условия…")}
              onChange={(event) => setReason(event.target.value)}
            />
          </label>
        ) : null}

        <div className="business-dialog-actions">
          <Button type="button" onClick={save} loading={busy} className="business-dialog-submit">
            {mode === "request" ? t("Send request", "Отправить заявку") : isB2b ? t("Save", "Сохранить") : t("Convert", "Перевести")}
          </Button>
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            {t("Cancel", "Отмена")}
          </Button>
        </div>
      </section>
    </div>
  );
}
