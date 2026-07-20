"use client";

import { useCallback, useEffect, useState } from "react";
import { api, ApiError, formatBps } from "@/lib/api";
import { useI18n } from "./i18n";
import { Badge, Button, Card, CopyButton, Field, Input, Notice } from "./ui";

type DiscountLink = {
  id: string;
  code: string;
  url: string;
  discountBps: number;
  consumed: boolean;
  consumedAt: string | null;
  createdAt: string;
};

// Персональные ОДНОРАЗОВЫЕ ссылки со скидкой (Part B). Обычная реф-ссылка ведёт по обычным b2c-тирам
// без скидки; здесь партнёр (с правом) выпускает ссылку под конкретного клиента со спец-скидкой,
// которая гаснет первым же привязанным пользователем. Показывается только если право выдано.
export function ReferralDiscountCard({ enabled, currentBps }: { enabled: boolean; currentBps: number }) {
  const { t } = useI18n();
  const [pct, setPct] = useState("");
  const [busy, setBusy] = useState(false);
  const [links, setLinks] = useState<DiscountLink[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const maxPct = currentBps / 100;

  const load = useCallback(async () => {
    try {
      const res = await api<{ items: DiscountLink[] }>("/v1/partner/discount-links");
      setLinks(res.items);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Could not load links.", "Не удалось загрузить ссылки."));
    }
  }, [t]);

  useEffect(() => { if (enabled) void load(); }, [enabled, load]);
  if (!enabled) return null;

  async function issue() {
    const value = Number(pct.trim());
    if (!Number.isFinite(value) || value < 0 || value > maxPct) {
      setError(t(`Enter a percent 0–${maxPct}.`, `Введите процент 0–${maxPct}.`));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api("/v1/partner/discount-links", { method: "POST", body: { discountBps: Math.round(value * 100) } });
      setPct("");
      await load();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Could not issue link.", "Не удалось выпустить ссылку."));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card
      title={t("Personal discount links", "Персональные ссылки со скидкой")}
      sub={t(
        `Your normal referral link gives no discount — referrals go on the usual tiers. Here you issue a one-time personal link with a discount (up to ${maxPct}%) for a specific client. It applies to the first person who signs up with it, as a price floor.`,
        `Ваша обычная реф-ссылка скидку не даёт — рефералы идут по обычным тирам. Здесь вы выпускаете одноразовую персональную ссылку со скидкой (до ${maxPct}%) под конкретного клиента. Скидка достаётся первому, кто по ней зарегистрируется, и работает как «пол» цены.`,
      )}
    >
      {error ? <Notice kind="error">{error}</Notice> : null}
      <div className="row-actions" style={{ alignItems: "flex-end", gap: 12, marginBottom: 14 }}>
        <Field label={t("Discount %", "Скидка %")} hint={t(`Max ${maxPct}%`, `Максимум ${maxPct}%`)}>
          <Input
            value={pct}
            onChange={(e) => setPct(e.target.value.replace(/[^\d.]/g, ""))}
            inputMode="decimal"
            placeholder={String(maxPct)}
            style={{ maxWidth: 140 }}
          />
        </Field>
        <Button onClick={issue} loading={busy}>
          {t("Issue link", "Выпустить ссылку")}
        </Button>
      </div>
      {links && links.length > 0 ? (
        <div className="stack" style={{ gap: 8 }}>
          {links.map((l) => (
            <div key={l.id} className="reflink-row" style={{ alignItems: "center", gap: 8 }}>
              <Input readOnly value={l.url} onFocus={(e) => e.currentTarget.select()} />
              <span style={{ fontSize: 13, whiteSpace: "nowrap" }}>{formatBps(l.discountBps)}</span>
              {l.consumed
                ? <Badge tone="green">{t("Used", "Использована")}</Badge>
                : <Badge tone="yellow">{t("Unused", "Свободна")}</Badge>}
              {!l.consumed ? <CopyButton value={l.url} label={t("Copy", "Копировать")} /> : null}
            </div>
          ))}
        </div>
      ) : (
        <p className="field-hint">{t("No personal links yet.", "Пока нет персональных ссылок.")}</p>
      )}
    </Card>
  );
}
