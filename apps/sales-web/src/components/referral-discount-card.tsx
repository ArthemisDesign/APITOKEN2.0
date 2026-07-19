"use client";

import { useState } from "react";
import { api, ApiError, formatBps } from "@/lib/api";
import { useI18n } from "./i18n";
import { Button, Card, Field, Input, Notice } from "./ui";

// Кабинет: партнёр с правом ставит скидку своим рефам (≤90%). Реф остаётся обычным аккаунтом —
// скидка это «пол» цены (не платят дороже неё). Показывается только если право выдано.
export function ReferralDiscountCard({ enabled, currentBps }: { enabled: boolean; currentBps: number }) {
  const { t } = useI18n();
  const [pct, setPct] = useState(String(currentBps / 100));
  const [busy, setBusy] = useState(false);
  const [savedBps, setSavedBps] = useState(currentBps);
  const [error, setError] = useState<string | null>(null);
  const [ok, setOk] = useState(false);
  if (!enabled) return null;

  async function save() {
    const value = Number(pct.trim());
    if (!Number.isFinite(value) || value < 0 || value > 90) {
      setError(t("Enter a percent 0–90.", "Введите процент 0–90."));
      return;
    }
    setBusy(true);
    setError(null);
    setOk(false);
    try {
      const bps = Math.round(value * 100);
      await api("/v1/partner/referral-discount", { method: "PATCH", body: { referralDiscountBps: bps } });
      setSavedBps(bps);
      setOk(true);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Could not save.", "Не удалось сохранить."));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card
      title={t("Referral discount", "Скидка вашим рефералам")}
      sub={t(
        "You can give your referrals a discount. They stay normal accounts on the usual tiers — this is a floor, so they never pay above it.",
        "Вы можете дать своим рефералам скидку. Они остаются обычными аккаунтами на обычных тирах — это «пол»: дороже они не платят.",
      )}
    >
      {error ? <Notice kind="error">{error}</Notice> : null}
      {ok ? <Notice kind="success">{t("Saved — now", "Сохранено — теперь")} {formatBps(savedBps)}.</Notice> : null}
      <div className="row-actions" style={{ alignItems: "flex-end", gap: 12 }}>
        <Field label={t("Discount %", "Скидка %")} hint={t("Max 90%", "Максимум 90%")}>
          <Input
            value={pct}
            onChange={(e) => setPct(e.target.value.replace(/[^\d.]/g, ""))}
            inputMode="decimal"
            style={{ maxWidth: 140 }}
          />
        </Field>
        <Button onClick={save} loading={busy}>
          {t("Save discount", "Сохранить скидку")}
        </Button>
      </div>
    </Card>
  );
}
