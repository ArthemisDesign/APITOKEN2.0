"use client";

// Только профиль и условия. Всё платёжное (кошелёк, выплаты) живёт в Payouts.

import { useState, type FormEvent } from "react";
import { api, ApiError, formatBps } from "@/lib/api";
import { usePartner } from "@/components/partner-context";
import { Button, Card, Field, Input, Notice, StatusBadge } from "@/components/ui";
import { useI18n } from "@/components/i18n";

export default function SettingsPage() {
  const { t } = useI18n();
  const partner = usePartner();
  const [displayName, setDisplayName] = useState(partner.displayName ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setSaved(false);
    setBusy(true);
    try {
      await api("/v1/partner/settings", {
        method: "PATCH",
        body: { displayName: displayName.trim() || undefined },
      });
      setSaved(true);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Could not save settings.", "Не удалось сохранить настройки."));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <h1 className="page-title">{t("Settings", "Настройки")}</h1>
      <p className="page-sub">{t("Your profile and commission terms.", "Ваш профиль и условия комиссии.")}</p>

      <div className="stack">
        <Card title={t("Profile", "Профиль")}>
          {error ? <Notice kind="error">{error}</Notice> : null}
          {saved ? <Notice kind="success">{t("Settings saved.", "Настройки сохранены.")}</Notice> : null}
          <form onSubmit={onSubmit}>
            <Field label="Telegram">
              <Input
                value={partner.telegramUsername ? `@${partner.telegramUsername}` : partner.email ?? "—"}
                readOnly
                disabled
              />
            </Field>
            <Field label={t("Display name", "Отображаемое имя")}>
              <Input
                value={displayName}
                onChange={(e) => setDisplayName(e.target.value)}
                placeholder={t("How we should address you", "Как к вам обращаться")}
              />
            </Field>
            <Button type="submit" loading={busy}>
              {t("Save changes", "Сохранить изменения")}
            </Button>
          </form>
        </Card>

        <Card
          title={t("Your commission terms", "Ваши условия комиссии")}
          sub={t(
            "You earn this share of what your referrals actually spend on API usage — the amount charged after their own discount, and only the part paid with real money (free bonus/promo balance is spent first and never counts). Not their top-ups, not list price. Rate set individually by the program.",
            "Вы получаете эту долю от того, что ваши рефералы реально тратят на использование API — от суммы, списанной после их скидки, и только с части, оплаченной реальными деньгами (бесплатный бонус/промо тратится первым и не считается). Не от пополнений и не от прайс-листа. Ставка устанавливается программой индивидуально.",
          )}
        >
          <div className="stat-grid" style={{ gridTemplateColumns: "repeat(2, 1fr)", marginBottom: 0 }}>
            <div className="stat-card">
              <div className="stat-label">{t("Direct commission", "Прямая комиссия")}</div>
              <div className="stat-value green">{formatBps(partner.commissionBps)}</div>
              <div className="stat-foot">{t("of your referrals' real spend", "от реального расхода ваших рефералов")}</div>
            </div>
            <div className="stat-card">
              <div className="stat-label">{t("Account status", "Статус аккаунта")}</div>
              <div className="stat-value" style={{ fontSize: 16, paddingTop: 6 }}>
                <StatusBadge status={partner.status} />
              </div>
            </div>
          </div>
        </Card>
      </div>
    </>
  );
}
