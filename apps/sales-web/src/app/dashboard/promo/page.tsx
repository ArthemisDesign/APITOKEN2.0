"use client";

// Промокоды партнёра. Доступно, только если админ включил промо и задал лимиты.
// Каждый код — одноразовый, даёт юзеру баланс на apitoken.sale. Погашение — на главном сайте.

import { useCallback, useEffect, useState } from "react";
import { api, ApiError, formatUsd, type PromoListResponse } from "@/lib/api";
import { usePartner } from "@/components/partner-context";
import { useI18n } from "@/components/i18n";
import { Badge, Button, Card, CopyButton, EmptyState, Input, Loading, Notice, Table } from "@/components/ui";

export default function PromoPage() {
  const { t } = useI18n();
  const partner = usePartner();
  const [data, setData] = useState<PromoListResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [created, setCreated] = useState<{ code: string } | null>(null);

  const load = useCallback(async () => {
    try {
      setData(await api<PromoListResponse>("/v1/partner/promo-codes"));
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t("Failed to load promo codes.", "Не удалось загрузить промокоды."));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const maxUsd = data ? Number(BigInt(data.maxValueNano) / 1_000_000_000n) : 0;
  const used = data?.items.length ?? 0;
  const remaining = data ? Math.max(0, data.maxCount - used) : 0;

  async function create() {
    const v = Number(value);
    if (!Number.isInteger(v) || v <= 0) {
      setFormError(t("Enter a whole dollar amount.", "Введите целую сумму в долларах."));
      return;
    }
    if (v > maxUsd) {
      setFormError(t(`Maximum is $${maxUsd} per code.`, `Максимум $${maxUsd} за код.`));
      return;
    }
    setBusy(true);
    setFormError(null);
    try {
      const res = await api<{ code: string }>("/v1/partner/promo-codes", { method: "POST", body: { valueUsd: v } });
      setCreated(res);
      setValue("");
      void load();
    } catch (err) {
      setFormError(err instanceof ApiError ? err.message : t("Could not create the code.", "Не удалось создать код."));
    } finally {
      setBusy(false);
    }
  }

  async function disable(id: string) {
    try {
      await api(`/v1/partner/promo-codes/${id}/disable`, { method: "POST" });
      void load();
    } catch {
      /* ignore */
    }
  }

  if (!partner.promoEnabled) {
    return (
      <>
        <h1 className="page-title">{t("Promo codes", "Промокоды")}</h1>
        <div className="card">
          <EmptyState title={t("Not available", "Недоступно")}>
            {t(
              "Promo codes are not enabled for your account. Contact the program if you'd like access.",
              "Промокоды не подключены для вашего аккаунта. Напишите в программу, если хотите доступ.",
            )}
          </EmptyState>
        </div>
      </>
    );
  }

  return (
    <>
      <h1 className="page-title">{t("Promo codes", "Промокоды")}</h1>
      <p className="page-sub">
        {t(
          "Give out promo codes that credit balance to new users on apitoken.sale. Anyone who redeems your code — if they aren't already referred — becomes your referral. The promo balance itself is free credit: it's spent first and earns no commission — you earn only on their later real-money spend.",
          "Раздавайте промокоды, которые начисляют баланс новым пользователям на apitoken.sale. Кто погасит ваш код — если он ещё ни за кем не закреплён — становится вашим рефералом. Сам промо-баланс — это бесплатные деньги: он тратится первым и комиссию не приносит; вы зарабатываете только на их последующих реальных тратах.",
        )}
      </p>
      {error ? <Notice kind="error">{error}</Notice> : null}

      <div className="stack">
        <Card
          title={t("Create a code", "Создать код")}
          sub={t(
            `Up to $${maxUsd} per code · ${remaining} of ${data?.maxCount ?? 0} codes left.`,
            `До $${maxUsd} за код · осталось ${remaining} из ${data?.maxCount ?? 0}.`,
          )}
        >
          {formError ? <Notice kind="error">{formError}</Notice> : null}
          {created ? (
            <div style={{ marginBottom: 14 }}>
              <div className="reflink-row">
                <Input readOnly className="mono" value={created.code} onFocus={(e) => e.currentTarget.select()} />
                <CopyButton value={created.code} label={t("Copy code", "Копировать код")} />
              </div>
              <p className="field-hint" style={{ marginTop: 8 }}>
                {t("Users redeem it in their dashboard on ", "Пользователи вводят его в кабинете на ")}
                <a href="https://apitoken.sale" rel="noopener">apitoken.sale</a>.
              </p>
            </div>
          ) : null}
          {remaining <= 0 ? (
            <Notice kind="info">{t("You've reached your code limit.", "Вы достигли лимита кодов.")}</Notice>
          ) : (
            <div className="reflink-row">
              <Input
                inputMode="numeric"
                value={value}
                onChange={(e) => setValue(e.target.value.replace(/[^\d]/g, ""))}
                placeholder={t(`Value in USD (max ${maxUsd})`, `Сумма в USD (макс ${maxUsd})`)}
                style={{ maxWidth: 260 }}
              />
              <Button onClick={create} loading={busy}>{t("Create code", "Создать код")}</Button>
            </div>
          )}
        </Card>

        <Card title={t("Your codes", "Ваши коды")}>
          {!data ? (
            <Loading />
          ) : data.items.length === 0 ? (
            <EmptyState title={t("No codes yet", "Пока нет кодов")} />
          ) : (
            <Table
              head={
                <>
                  <th>{t("Code", "Код")}</th>
                  <th className="num">{t("Value", "Номинал")}</th>
                  <th>{t("Status", "Статус")}</th>
                  <th />
                </>
              }
            >
              {data.items.map((c) => (
                <tr key={c.id}>
                  <td className="mono" style={{ fontWeight: 600 }}>{c.code}</td>
                  <td className="num">{formatUsd(c.valueNano)}</td>
                  <td>
                    {c.status === "active" ? (
                      <Badge tone="green">{t("Active", "Активен")}</Badge>
                    ) : c.status === "redeemed" ? (
                      <Badge tone="neutral">{t("Redeemed", "Погашен")}</Badge>
                    ) : (
                      <Badge tone="yellow">{t("Disabled", "Выключен")}</Badge>
                    )}
                  </td>
                  <td>
                    {c.status === "active" ? (
                      <CopyButton value={c.code} label={t("Copy", "Копировать")} />
                    ) : null}
                    {c.status === "active" ? (
                      <Button size="sm" variant="ghost" onClick={() => disable(c.id)}>
                        {t("Disable", "Выключить")}
                      </Button>
                    ) : null}
                  </td>
                </tr>
              ))}
            </Table>
          )}
        </Card>
      </div>
    </>
  );
}
